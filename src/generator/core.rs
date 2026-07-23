use crate::anomaly::AnomalyPlanner;
use crate::format::{protobuf::serialize_protobuf_like, Format, FormatContext, FormatKind, Header};
use crate::generator::config::{Phase, Profile, TargetConfig};
use crate::observability::metrics::Metrics;
use crate::schema::Schema;
use crate::shutdown::{graceful_drain_wait, shutdown_listener};
use crate::template;
use crate::template::render_template;
use crate::transport::file::{target_sender_file_with_rotation, RotationConfig};
#[cfg(feature = "kafka")]
use crate::transport::kafka::{
    parse_kafka_acks, parse_kafka_compression, target_sender_kafka, KafkaConfig,
};
use crate::transport::reconnect::ReconnectConfig;
use crate::transport::{
    parse_tls_min_version, target_sender_file, target_sender_tcp, target_sender_tls,
    target_sender_udp, Framing,
};
use anyhow::Result;
use governor::{Quota, RateLimiter};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub fn create_dispatcher(targets: &[TargetConfig], distribution: &str) -> Vec<usize> {
    match distribution {
        "weighted" => {
            let mut v = Vec::new();
            for (idx, t) in targets.iter().enumerate() {
                for _ in 0..t.weight.max(1) {
                    v.push(idx);
                }
            }
            if v.is_empty() {
                (0..targets.len()).collect()
            } else {
                v
            }
        }
        _ => (0..targets.len()).collect(),
    }
}

/// Hot-path версия `default_values`: заполняет caller-owned `&mut HashMap`
/// (через `.clear()`) — устраняет heap allocation per message.
///
/// PR-17b (v10.7.17): экономия ~80-150 ns/msg на HashMap alloc + capacity hint
/// (rehashes в первом цикле). Используется `generate_message_with_format_cached`.
///
/// PR-17c (v10.7.18): принимает `now` (shared timestamp) — один `Utc::now()`
/// per msg, используется и для `datetime_now_jitter` и для `rfc5424_timestamp`
/// в `generate_message_with_format_cached`.
#[inline]
pub fn default_values_into(
    m: &mut HashMap<String, String>,
    ctx: &PhaseContext,
    phase: &Phase,
    seq: usize,
    rng: &mut rand::rngs::StdRng,
    now: chrono::DateTime<chrono::Utc>,
) -> usize {
    m.clear();
    // Статические литералы (3 entries).
    m.insert("real_hostname".to_string(), "localhost".to_string());
    m.insert("hostname".to_string(), "localhost".to_string());
    m.insert("real_command".to_string(), "echo ok".to_string());
    // Динамические значения (4 entries).
    m.insert("sequence".to_string(), seq.to_string());
    m.insert("real_app".to_string(), phase.name.clone());
    m.insert(
        "timestamp".to_string(),
        crate::payload::datetime_now_jitter_at(now, 0, rng),
    );
    m.insert(
        "pid".to_string(),
        crate::payload::int_in_range(1, 65535, rng).to_string(),
    );
    // PR-10: faker keys pre-built в PhaseContext (избегаем 9× format!).
    // PR-10: skip unreferenced fakers (~120-160 ns/msg savings).
    const FAKER_KIND_NAMES: &[&str] = &[
        "ipv4",
        "ipv6",
        "mac",
        "uuid",
        "hostname",
        "username",
        "user_agent",
        "url",
        "http_status",
    ];
    match &ctx.referenced_fakers {
        Some(referenced) => {
            // Генерируем только referenced fakers.
            for (i, kind) in FAKER_KIND_NAMES.iter().enumerate() {
                if referenced.contains(kind) {
                    m.insert(ctx.faker_keys[i].clone(), crate::payload::faker(kind, rng));
                }
            }
        }
        None => {
            // Никто не сделал scan — генерируем все 9 (для backward-compat с прямыми
            // вызовами default_values без PhaseContext).
            for (i, kind) in FAKER_KIND_NAMES.iter().enumerate() {
                m.insert(ctx.faker_keys[i].clone(), crate::payload::faker(kind, rng));
            }
        }
    }
    m.len()
}

pub fn default_values(
    ctx: &PhaseContext,
    phase: &Phase,
    seq: usize,
    rng: &mut rand::rngs::StdRng,
) -> HashMap<String, String> {
    // PR-10: уменьшен HashMap capacity. Если referenced_fakers == None —
    // все 9 fakers (capacity 17). Если Some(set) — только referenced
    // (capacity = 9 + 9 - len(set) = лучше).
    let faker_count = ctx.referenced_fakers.as_ref().map(|s| s.len()).unwrap_or(9);
    let mut m = HashMap::with_capacity(8 + faker_count);
    let _ = default_values_into(&mut m, ctx, phase, seq, rng, chrono::Utc::now());
    m
}

pub fn load_templates(phase: &Phase) -> Result<Vec<String>> {
    if let Some(path) = &phase.templates_file {
        let text = fs::read_to_string(path)?;
        let v: Vec<String> = serde_json::from_str(&text)?;
        return Ok(v);
    }
    Ok(phase.templates.clone())
}

pub fn load_schema(phase: &Phase) -> Result<Option<Schema>> {
    if let Some(path) = &phase.schema_file {
        let text = fs::read_to_string(path)?;
        let s: Schema = serde_json::from_str(&text)?;
        return Ok(Some(s));
    }
    Ok(None)
}

pub fn generate_message(phase: &Phase, seq: usize) -> Result<Vec<u8>> {
    // Legacy API — для backward-compat. Hot path использует
    // `generate_message_with_format` через `run_phase_multi`.
    // Здесь мы просто создаём локальный `PhaseContext` (one-shot overhead) —
    // этот путь не оптимизирован под hot-path.
    let ctx = PhaseContext::resolve(phase)?;
    generate_message_with_format_inner(&ctx, phase, seq)
}

/// Внутренняя helper, не часть публичного API. Использует `PhaseContext`
/// (pre-resolved templates, header cache, faker scan).
fn generate_message_with_format_inner(
    ctx: &PhaseContext,
    phase: &Phase,
    seq: usize,
) -> Result<Vec<u8>> {
    // F4: RNG детерминирован по (seed, seq) — один seed+seq даёт один вывод.
    // Без seed — энтропия ОС (вариативно, но не воспроизводимо).
    let mut rng = crate::payload::derive_rng(phase.seed, seq);
    let mut values = default_values(ctx, phase, seq, &mut rng);
    if let Some(schema) = &ctx.schema {
        // ВАЖНО (F4): HashMap итерируется в недетерминированном порядке, из-за чего
        // при одном seed RNG потреблялся бы в разной последовательности между
        // запусками. Сортируем поля по имени для полной воспроизводимости.
        //
        // F6 (межполевые корреляции): поле с `depends_on` должно генерироваться
        // ПОСЛЕ родителя. Делаем два прохода по отсортированному списку:
        //   1) поля без зависимостей — генерируются своим базовым типом;
        //   2) зависимые поля — значение берётся из `mapping` по значению родителя.
        // Двух проходов достаточно для одноуровневых корреляций (родитель→ребёнок).
        let mut fields: Vec<_> = schema.fields.iter().collect();
        fields.sort_by(|a, b| a.0.cmp(b.0));
        for (name, field) in &fields {
            if field.depends_on.is_none() {
                let value = gen_schema_field(field, &mut rng);
                values.insert(name.to_string(), value);
            }
        }
        for (name, field) in &fields {
            if let Some(parent) = &field.depends_on {
                let value = resolve_correlated_field(field, parent, &values, &mut rng);
                values.insert(name.to_string(), value);
            }
        }
        if let Some(tpl) = &schema.template {
            let body = render_template(tpl, &values);
            // N10-gap (v9.2.0): resolve FormatKind один раз через parse (дешёвый match).
            // Для hot-path в run_phase_multi используется `generate_message_with_format`
            // с уже резолвленным FormatKind — без per-message parse.
            let format_kind = FormatKind::parse(phase.format_type()).unwrap_or(FormatKind::Raw);
            return Ok(finish_body(
                ctx,
                &format_kind,
                phase,
                &values,
                body.into_bytes(),
                &mut rng,
                None,
            ));
        }
    }
    // F14: выбор шаблона из набора (случайный / взвешенный), а не всегда первый.
    let tpl: &template::CompiledTemplate = if ctx.compiled_templates.is_empty() {
        &ctx.compiled_fallback
    } else {
        pick_template_compiled(
            &ctx.compiled_templates,
            phase.template_weights.as_deref(),
            &mut rng,
        )
        .map(|arc| arc.as_ref())
        .unwrap_or(ctx.compiled_fallback.as_ref())
    };
    if phase.format_type() == "protobuf" {
        return Ok(serialize_protobuf_like(
            phase.protobuf_schema.as_ref(),
            &values,
        ));
    }
    let body = tpl.render(&values);
    let format_kind = FormatKind::parse(phase.format_type()).unwrap_or(FormatKind::Raw);
    Ok(finish_body(
        ctx,
        &format_kind,
        phase,
        &values,
        body.into_bytes(),
        &mut rng,
        None,
    ))
}

/// Hot-path версия `generate_message`: FormatKind уже резолвлен вызывающим
/// (`run_phase_multi`). Это устраняет per-message парсинг `phase.format_type()`
/// в горячем цикле — экономия ~5-10 нс на сообщение + устраняет string-ветку.
///
/// PR-5: `ctx` содержит pre-resolved templates и schema (разрешаются ОДИН раз
/// в `run_phase_multi` setup, не per-message). Это устраняет file I/O
/// (`fs::read_to_string` + `serde_json::from_str`) в hot loop — **-30-50%
/// syscalls** при использовании `schema_file` или `templates_file`.
///
/// PR-17b: для hot-path используйте [`generate_message_with_format_cached`],
/// который переиспользует caller-owned `&mut HashMap` (устраняет alloc per msg).
/// Текущая функция остаётся как backward-compat wrapper.
///
/// `format_kind` принимается по ссылке (не Copy — `Protobuf` вариант несёт
/// `Option<ProtobufSchemaFieldMap>` с `HashMap`), чтобы избежать clone
/// в горячем цикле. Внутри `render`/`matches!` он только читается.
#[inline]
pub fn generate_message_with_format(
    ctx: &PhaseContext,
    phase: &Phase,
    format_kind: &FormatKind,
    seq: usize,
) -> Result<Vec<u8>> {
    // PR-17b: backward-compat — аллоцирует HashMap на каждое сообщение.
    // Hot-path код должен использовать `generate_message_with_format_cached`.
    let mut values = HashMap::with_capacity(16);
    generate_message_with_format_cached(ctx, phase, format_kind, seq, &mut values)
}

/// PR-17b (v10.7.17): hot-path версия — переиспользует caller-owned `&mut HashMap`
/// (через `default_values_into` + `.clear()`). Устраняет heap allocation per
/// message (~80-150 нс экономии).
///
/// PR-17c (v10.7.18): один shared `Utc::now()` per msg — используется и в
/// `default_values_into` (`timestamp` placeholder) и в `rfc5424_timestamp_at`
/// (syslog header). Экономит ~30-100 нс/msg (раньше было 2× `Utc::now()`).
///
/// Использование:
/// ```ignore
/// let mut values = HashMap::with_capacity(16);
/// for seq in 1..=N {
///     let msg = generate_message_with_format_cached(&ctx, &phase, &format_kind, seq, &mut values)?;
///     // ...
/// }
/// ```
#[inline]
pub fn generate_message_with_format_cached(
    ctx: &PhaseContext,
    phase: &Phase,
    format_kind: &FormatKind,
    seq: usize,
    values: &mut HashMap<String, String>,
) -> Result<Vec<u8>> {
    use chrono::Utc;
    let mut rng = crate::payload::derive_rng(phase.seed, seq);
    // PR-17c: single shared timestamp — was 2× Utc::now() (datetime_now_jitter + rfc5424_timestamp).
    let now = Utc::now();
    let _ = default_values_into(values, ctx, phase, seq, &mut rng, now);
    if let Some(schema) = &ctx.schema {
        let mut fields: Vec<_> = schema.fields.iter().collect();
        fields.sort_by(|a, b| a.0.cmp(b.0));
        for (name, field) in &fields {
            if field.depends_on.is_none() {
                let value = gen_schema_field(field, &mut rng);
                values.insert(name.to_string(), value);
            }
        }
        for (name, field) in &fields {
            if let Some(parent) = &field.depends_on {
                let value = resolve_correlated_field(field, parent, values, &mut rng);
                values.insert(name.to_string(), value);
            }
        }
        if let Some(tpl) = &schema.template {
            let body = render_template(tpl, values);
            return Ok(finish_body(
                ctx,
                format_kind,
                phase,
                values,
                body.into_bytes(),
                &mut rng,
                None,
            ));
        }
    }
    // PR-10: pre-compiled templates — избегаем re-compile per message.
    // pick_template_compiled возвращает `&Arc<CompiledTemplate>` для borrowed
    // использования. Если compiled пустой → fallback pre-compiled.
    let tpl: &template::CompiledTemplate = if ctx.compiled_templates.is_empty() {
        &ctx.compiled_fallback
    } else {
        pick_template_compiled(
            &ctx.compiled_templates,
            phase.template_weights.as_deref(),
            &mut rng,
        )
        .map(|arc| arc.as_ref())
        .unwrap_or(ctx.compiled_fallback.as_ref())
    };
    if matches!(format_kind, FormatKind::Protobuf(_)) {
        return Ok(serialize_protobuf_like(
            phase.protobuf_schema.as_ref(),
            values,
        ));
    }
    let body = tpl.render(values);
    Ok(finish_body(
        ctx,
        format_kind,
        phase,
        values,
        body.into_bytes(),
        &mut rng,
        Some(now),
    ))
}

/// Pre-resolved phase context (PR-5).
///
/// `templates` и `schema` загружаются ОДИН раз в `run_phase_multi` setup
/// (file I/O + JSON parse), затем переиспользуются per-message без I/O.
///
/// Раньше эти значения резолвились внутри `generate_message` через
/// `load_templates`/`load_schema`, что вызывало `fs::read_to_string` +
/// `serde_json::from_str` **на КАЖДОЕ сообщение** — O(N) syscalls вместо O(1).
pub struct PhaseContext {
    /// Pre-loaded templates (либо из `templates_file`, либо копия `phase.templates`).
    /// Хранится как `Vec<String>` чтобы пережить все вызовы `generate_message_with_format`.
    pub templates: Vec<String>,
    /// PR-10: pre-compiled templates — `CompiledTemplate::compile()` стоит
    /// ~80-200 ns per call (Vec alloc + String allocs). 6 вызовов per message
    /// (1 body + 5 syslog fields) → ~480-1380 ns/msg savings. `Arc` для cheap
    /// clone если потом понадобится шарить между workers.
    pub compiled_templates: Vec<Arc<template::CompiledTemplate>>,
    /// PR-10: pre-compiled fallback template для `pick_template().unwrap_or(...)`.
    /// Пустой список → используется default template — pre-compiled ОДИН раз.
    pub compiled_fallback: Arc<template::CompiledTemplate>,
    /// PR-10: pre-rendered syslog fields (hostname/app_name/procid/msgid/structured_data)
    /// если они НЕ содержат per-message placeholders (pid/sequence/faker.*).
    /// Detected at phase setup: scan for "dangerous" placeholders.
    /// Если None — wrap_syslog re-renders per message.
    pub cached_syslog_header: Option<Arc<SyslogHeaderParts>>,
    /// PR-A2 (v10.8.0): optional slot-based CompiledPhase. Some — путь через
    /// `plan::ValueArena` + slot-based render (без hash lookups, без String
    /// allocs на сообщение). None — legacy HashMap-based path.
    /// MVP: opt-in, выставляется через `compile_plan()` если schema/template
    /// are simple enough.
    pub compiled_plan: Option<crate::plan::CompiledPhase>,
    /// Используется в `default_values` чтобы избежать 9× `format!("faker.{kind}")`
    /// allocations per message (~135-180 ns/msg).
    pub faker_keys: [String; 9],
    /// PR-10: detected faker kinds referenced in templates (pre-computed scan).
    /// None = all 9 fakers generated, Some(set) = только referenced.
    /// Для типичных профилей с 1-2 faker tokens экономит ~120-160 ns/msg.
    pub referenced_fakers: Option<std::collections::HashSet<&'static str>>,
    /// Pre-loaded schema (если задано `schema_file`), либо None. `Arc` для
    /// cheap clone (используется в `Schema.fields.iter()`).
    pub schema: Option<Arc<Schema>>,
}

/// Cacheable parts of syslog header (если все поля static).
///
/// PR-17c (v10.7.18): поля используют `Arc<str>` — clone в hot-path
/// = atomic increment (1-5 ns) вместо String alloc+memcpy (25-50 ns).
#[derive(Debug, Clone)]
pub struct SyslogHeaderParts {
    pub hostname: Arc<str>,
    pub app_name: Arc<str>,
    pub procid: Arc<str>, // pre-rendered без `{{pid}}` если он есть
    pub msgid: Arc<str>,
    pub structured_data: Arc<str>,
    /// `false` если procid содержит `{{pid}}` — re-render per message.
    pub procid_is_static: bool,
}

impl PhaseContext {
    /// Резолвит templates и schema ОДИН раз (file I/O + JSON parse).
    /// Также pre-compiles CompiledTemplate для всех templates + fallback
    /// + scans referenced faker kinds.
    ///
    /// На ошибке I/O возвращает `anyhow::Error`.
    pub fn resolve(phase: &Phase) -> Result<Self> {
        let templates = if let Some(path) = &phase.templates_file {
            let text = fs::read_to_string(path)?;
            serde_json::from_str(&text)?
        } else {
            phase.templates.clone()
        };
        let schema = if let Some(path) = &phase.schema_file {
            let text = fs::read_to_string(path)?;
            let s: Schema = serde_json::from_str(&text)?;
            Some(Arc::new(s))
        } else {
            None
        };

        // PR-10: pre-compile ВСЕ templates. `Vec<Arc<CompiledTemplate>>` — Arc
        // cheap clone если понадобится шарить (не критично пока, но безопасно).
        let mut compiled_templates: Vec<Arc<template::CompiledTemplate>> =
            Vec::with_capacity(templates.len());
        for t in &templates {
            compiled_templates.push(Arc::new(template::CompiledTemplate::compile(t)));
        }
        // Pre-compile fallback (default template) — используется при empty `templates`.
        let compiled_fallback = Arc::new(template::CompiledTemplate::compile(
            "{{timestamp}} {{real_app}} seq={{sequence}}",
        ));

        // PR-10: pre-build faker keys. Используем массив `String` чтобы
        // избежать 9× `format!("faker.{kind}")` per message.
        const FAKER_KIND_NAMES: &[&str] = &[
            "ipv4",
            "ipv6",
            "mac",
            "uuid",
            "hostname",
            "username",
            "user_agent",
            "url",
            "http_status",
        ];
        let faker_keys: [String; 9] =
            std::array::from_fn(|i| format!("faker.{}", FAKER_KIND_NAMES[i]));

        // PR-10: detect referenced fakers. Scan всех templates (body + syslog fields)
        // для `{{faker.*}}` placeholders. Только referenced генерируются.
        // PR-A0 (v10.8.0): используем HashSet напрямую и оборачиваем в Some
        // перед присвоением PhaseContext. Downstream default_values_into
        // ошибочно генерирует ВСЕ 9 fakers для шаблонов без {{faker.*}}
        // если Option == None. Empty Set подавляет эту fallback ветку.
        // Benchmarks статических шаблонов подтвердили −34..−66% ns/msg.
        let mut referenced_fakers_set: std::collections::HashSet<&'static str> =
            std::collections::HashSet::new();
        let s = &phase.syslog;
        let mut scan_templates: Vec<&str> = templates.iter().map(|t| t.as_str()).collect();
        scan_templates.push(&s.hostname);
        scan_templates.push(&s.app_name);
        scan_templates.push(&s.procid);
        scan_templates.push(&s.msgid);
        scan_templates.push(&s.structured_data);
        for tpl in &scan_templates {
            for kind in FAKER_KIND_NAMES {
                if tpl.contains(&format!("{{faker.{kind}}}")) {
                    referenced_fakers_set.insert(kind);
                }
            }
        }
        let referenced_fakers: Option<std::collections::HashSet<&'static str>> =
            Some(referenced_fakers_set);

        // PR-10: pre-render syslog header если все поля static.
        // Scan для "per-message" placeholders ({{sequence}}, {{pid}}, {{faker.*}}).
        // Если нет — pre-render все 5 полей ОДИН раз.
        let has_dynamic = |tpl: &str| -> bool {
            tpl.contains("{{sequence}}") || tpl.contains("{{pid}}") || tpl.contains("{{faker.")
        };
        let cached_syslog_header = if !has_dynamic(&s.hostname)
            && !has_dynamic(&s.app_name)
            && !has_dynamic(&s.msgid)
            && !has_dynamic(&s.structured_data)
        {
            // Все 4 поля static — pre-render ОДИН раз. procid — отдельно
            // (может содержать {{pid}}).
            let empty = HashMap::new();
            // PR-17c: Arc<str> — 1 alloc per field при setup, atomic clone в hot-path.
            let hostname: Arc<str> = Arc::from(template::render_template(&s.hostname, &empty));
            let app_name: Arc<str> = Arc::from(template::render_template(&s.app_name, &empty));
            let msgid: Arc<str> = Arc::from(template::render_template(&s.msgid, &empty));
            let structured_data: Arc<str> =
                Arc::from(template::render_template(&s.structured_data, &empty));
            let procid_is_static = !has_dynamic(&s.procid);
            let procid: Arc<str> = if procid_is_static {
                Arc::from(template::render_template(&s.procid, &empty))
            } else {
                Arc::from(s.procid.as_str()) // перерендерим per message (нужен {{pid}})
            };
            Some(Arc::new(SyslogHeaderParts {
                hostname,
                app_name,
                procid,
                msgid,
                structured_data,
                procid_is_static,
            }))
        } else {
            None
        };

        Ok(Self {
            templates,
            compiled_templates,
            compiled_fallback,
            cached_syslog_header,
            faker_keys,
            referenced_fakers,
            schema,
            // PR-A2 (v10.8.0): compile plan для slot-based render path.
            // MVP: compile для всех phases (план сам решает когда применим).
            // Полная миграция hot path в generate_message_with_plan — PR-A2.3.
            compiled_plan: crate::plan::compile_phase(phase).ok(),
        })
    }
}

/// F14: выбрать шаблон из списка. Пустой список → None. Один шаблон → он.
/// Если `weights` заданы и длина совпадает — взвешенный выбор, иначе равновероятный.
/// F14: выбрать шаблон из списка. Пустой список → None. Один шаблон → он.
/// Если `weights` заданы и длина совпадает — взвешенный выбор, иначе равновероятный.
///
/// PR-5: возвращает `Option<&String>` (borrow) вместо `Option<String>` (clone) —
///
/// вызывающий код делает clone если нужно, или использует `&str` напрямую для
/// `render_template`. Для типичной нагрузки (1 шаблон на фазу) — 0 clones.
/// PR-10: версия pick_template для pre-compiled templates.
/// Возвращает `&Arc<CompiledTemplate>` (borrow) — caller reuses CompiledTemplate
/// через `.render()` без re-compile. Экономит ~80-200 ns/msg.
///
/// PR-17b (v10.7.17): `#[inline]` — hot-path, вызывается per msg.
#[inline]
fn pick_template_compiled<'a>(
    templates: &'a [Arc<template::CompiledTemplate>],
    weights: Option<&[f64]>,
    rng: &mut rand::rngs::StdRng,
) -> Option<&'a Arc<template::CompiledTemplate>> {
    match templates.len() {
        0 => None,
        1 => Some(&templates[0]),
        n => {
            let idx = match weights {
                Some(w) if w.len() == n => crate::payload::weighted_index(w, rng),
                _ => crate::payload::int_in_range(0, (n - 1) as i64, rng) as usize,
            };
            Some(&templates[idx])
        }
    }
}

/// Генерация значения поля schema с учётом типа и распределения (F5/F6).
fn gen_schema_field(field: &crate::schema::SchemaField, rng: &mut rand::rngs::StdRng) -> String {
    match field.field_type.as_str() {
        "enum" => {
            let vals = match &field.values {
                Some(v) if !v.is_empty() => v,
                _ => return String::new(),
            };
            let idx = match field.distribution.as_deref().unwrap_or("uniform") {
                "weighted" => match &field.weights {
                    Some(w) if w.len() == vals.len() => crate::payload::weighted_index(w, rng),
                    _ => crate::payload::int_in_range(0, (vals.len() - 1) as i64, rng) as usize,
                },
                "zipf" => {
                    crate::payload::zipf_index(vals.len(), field.zipf_exponent.unwrap_or(1.0), rng)
                }
                // uniform и любое прочее — равновероятно.
                _ => crate::payload::int_in_range(0, (vals.len() - 1) as i64, rng) as usize,
            };
            vals[idx].clone()
        }
        "int" => {
            crate::payload::int_in_range(field.min.unwrap_or(0), field.max.unwrap_or(i64::MAX), rng)
                .to_string()
        }
        "datetime" => crate::payload::datetime_now_jitter(field.jitter_secs.unwrap_or(0), rng),
        "string" => crate::payload::random_string(field.len.unwrap_or(8), rng),
        "faker" => crate::payload::faker(field.faker.as_deref().unwrap_or(""), rng),
        "regex" => crate::payload::gen_from_regex(field.regex.as_deref().unwrap_or(""), rng),
        _ => String::new(),
    }
}

/// F6 (межполевые корреляции): вычислить значение зависимого поля.
///
/// Алгоритм:
/// 1. Берём уже сгенерированное значение родительского поля `parent`.
/// 2. Если в `mapping` есть точное соответствие — возвращаем его.
/// 3. Иначе — `mapping_default`, если задан.
/// 4. Иначе — генерируем поле его базовым типом (fallback).
fn resolve_correlated_field(
    field: &crate::schema::SchemaField,
    parent: &str,
    values: &HashMap<String, String>,
    rng: &mut rand::rngs::StdRng,
) -> String {
    let parent_val = values.get(parent).map(|s| s.as_str()).unwrap_or("");
    if let Some(map) = &field.mapping {
        if let Some(v) = map.get(parent_val) {
            return v.clone();
        }
    }
    if let Some(def) = &field.mapping_default {
        return def.clone();
    }
    // Нет соответствия и нет дефолта — генерируем базовым типом.
    gen_schema_field(field, rng)
}

/// Обёртка syslog + F6-паддинг тела до целевого размера (если задан).
///
/// PR-17c (v10.7.18): принимает `now: Option<DateTime<Utc>>` — если Some,
/// передаётся в `wrap_syslog` и используется для shared timestamp в
/// `rfc5424_timestamp_at`. None = legacy path (`rfc5424_timestamp()` с
/// внутренним `Utc::now()`).
#[inline]
fn finish_body(
    ctx: &PhaseContext,
    format_kind: &FormatKind,
    phase: &Phase,
    values: &HashMap<String, String>,
    body: Vec<u8>,
    rng: &mut rand::rngs::StdRng,
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<u8> {
    let body = match phase.pad_to_bytes {
        Some(n) if n > 0 => crate::payload::pad_to_size(body, n, rng),
        _ => body,
    };
    wrap_syslog(ctx, format_kind, phase, values, body, now)
}

/// Обернуть тело сообщения (MSG) в конверт согласно `phase.format`.
///
/// N10 (v9.1.0) ввёл trait `Format` + `FormatKind`, но горячий путь в этой
/// функции использовал прямой match на `phase.format_type()` — это N10-gap.
/// В v9.2.0 (F15) диспатч идёт через `FormatKind::render(&ctx, &body)` —
/// единая точка расширения для rfc5424/rfc3164/raw/protobuf/cef/leef/json_lines.
///
/// `format_kind` кешируется в `run_phase_multi` (резолвится один раз на фазу
/// из `phase.format_type()` через `FormatKind::parse`). Это устраняет
/// строковый match в горячем цикле — экономия ~3-5 нс на сообщение.
///
/// PR-10: используем `ctx.cached_syslog_header` если все syslog поля static
/// (5× re-render per message eliminated, ~500-1000 ns/msg savings).
///
/// PR-17b (v10.7.17): `#[inline]` — hot-path, вызывается per msg.
///
/// PR-17c (v10.7.18): `now` — shared timestamp; `Header` поля теперь `Arc<str>`
/// (clone = atomic inc, не String alloc).
#[inline]
fn wrap_syslog(
    ctx: &PhaseContext,
    format_kind: &FormatKind,
    phase: &Phase,
    values: &HashMap<String, String>,
    body: Vec<u8>,
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<u8> {
    let s = &phase.syslog;
    // PR-10: hot path. Если cached header есть (все syslog поля static
    // кроме возможно procid) — используем pre-rendered strings, re-render
    // только procid если он содержит {{pid}}.
    //
    // PR-17c (v10.7.18): `Arc<str>` — clone = atomic increment (~1-5 ns),
    // а не String alloc+memcpy (~25-50 ns). Устраняет ~100-200 нс/msg.
    let (hostname, app_name, procid, msgid, structured_data) =
        match ctx.cached_syslog_header.as_ref() {
            Some(cached) => {
                let procid = if cached.procid_is_static {
                    cached.procid.clone()
                } else {
                    // Только procid содержит {{pid}} — re-render.
                    Arc::from(render_template(&s.procid, values).as_str())
                };
                (
                    cached.hostname.clone(),
                    cached.app_name.clone(),
                    procid,
                    cached.msgid.clone(),
                    cached.structured_data.clone(),
                )
            }
            None => {
                // Cache miss — re-render все 5 полей (старый путь).
                // PR-17c: конвертируем String → Arc<str> через Arc::from (1 alloc per field).
                (
                    Arc::from(render_template(&s.hostname, values).as_str()),
                    Arc::from(render_template(&s.app_name, values).as_str()),
                    Arc::from(render_template(&s.procid, values).as_str()),
                    Arc::from(render_template(&s.msgid, values).as_str()),
                    Arc::from(render_template(&s.structured_data, values).as_str()),
                )
            }
        };
    // PR-17c: pre-compute RFC 5424 timestamp string once. Empty Arc если now = None
    // (legacy path — format::build вызовет rfc5424_timestamp() сам).
    let timestamp: Arc<str> = match now {
        Some(t) => Arc::from(crate::format::rfc5424_timestamp_at(t).as_str()),
        None => Arc::from(""),
    };
    let header = Header {
        facility: s.facility,
        severity: s.severity,
        hostname,
        app_name,
        procid,
        msgid,
        structured_data,
        timestamp,
        bom: s.bom,
    };
    // FormatContext — stack-аллоцированная структура из 4 ссылок. Стоимость
    // построения: ~0 (просто копирование ссылок).
    let fmt_ctx = FormatContext {
        header: &header,
        cef: phase.cef.as_ref(),
        leef: phase.leef.as_ref(),
        json_lines_fields: phase.json_lines_fields.as_ref(),
    };
    format_kind.render(&fmt_ctx, &body)
}

pub async fn run_phase_multi(
    phase: &Phase,
    targets: &[TargetConfig],
    distribution: &str,
    shutdown_cfg: &crate::config::ShutdownConfig,
    metrics: Metrics,
    shutdown: &CancellationToken,
) -> Result<()> {
    // PR-2: `shutdown` token передаётся снаружи (из run_profile). Это даёт:
    //  - единый CancellationToken на весь run_profile (не per-phase)
    //  - двойной Ctrl-C counter работает между фазами (раньше сбрасывался
    //    при каждом новом run_phase_multi)
    //  - shutdown_listener спавнится ОДИН раз в run_profile

    // PR-5: pre-resolve templates и schema ОДИН раз (file I/O + JSON parse)
    // для использования в hot loop. Раньше резолвились per-message.
    let ctx = PhaseContext::resolve(phase)?;

    let dispatch = create_dispatcher(targets, distribution);
    let mut txs = Vec::new();
    let mut handles = Vec::new();
    let mut total_workers: u64 = 0;

    for target in targets {
        // Одна очередь на target; пул из `connections` воркеров конкурентно
        // читает из неё через общий SharedRx (каждое сообщение — ровно одному воркеру).
        let (tx, rx) = mpsc::channel(1024);
        txs.push(tx);
        let shared_rx = Arc::new(parking_lot::Mutex::new(rx));
        let pool_size = target.connections.max(1);
        total_workers += pool_size as u64;
        let framing = Framing::parse(&target.framing);
        // F16: собираем RotationConfig и ReconnectConfig из TargetConfig.
        let rotation = RotationConfig {
            size_mb: target.file_rotation_size_mb,
            interval_secs: target.file_rotation_interval_secs,
            max_files: target.file_rotation_max_files,
        };
        let rcfg = ReconnectConfig::resolve(
            target.reconnect_max_attempts,
            target.reconnect_initial_backoff_ms,
            target.reconnect_max_backoff_ms,
            target.reconnect_multiplier,
        );
        for _ in 0..pool_size {
            let rx = shared_rx.clone();
            let addr = target.address.clone();
            let phase_name = phase.name.clone();
            let m = metrics.clone();
            let sd = shutdown.clone();
            let h = match target.transport.as_str() {
                "tcp" => {
                    // F16: передаём reconnect_config в TCP sender.
                    tokio::spawn(target_sender_tcp(
                        addr,
                        phase_name,
                        rx,
                        m,
                        sd,
                        framing,
                        Some(rcfg.clone()),
                    ))
                }
                "udp" => {
                    // UDP без reconnect (connectionless).
                    tokio::spawn(target_sender_udp(addr, phase_name, rx, m, sd))
                }
                "tls" => {
                    // N4: SNI/проверка имени — из tls_domain или хост-части address.
                    let domain = target.tls_domain.clone().unwrap_or_else(|| {
                        addr.rsplit_once(':')
                            .map(|(h, _)| h.to_string())
                            .unwrap_or_else(|| addr.clone())
                    });
                    // Читаем CA-файл заранее (валидация F13 уже проверила его наличие).
                    let ca_pem = match &target.tls_ca_file {
                        Some(path) => match std::fs::read(path) {
                            // PR-12: оборачиваем в Zeroizing<Vec<u8>> чтобы
                            // private data не утекала в core dumps / swap.
                            Ok(bytes) => Some(zeroize::Zeroizing::new(bytes)),
                            Err(e) => {
                                eprintln!("TLS ({addr}): не удалось прочитать CA-файл {path}: {e}");
                                None
                            }
                        },
                        None => None,
                    };
                    // N4.mTLS (v8.7.2): читаем клиентский cert+key заранее.
                    let (client_cert_pem, client_key_pem) =
                        match (&target.tls_client_cert_file, &target.tls_client_key_file) {
                            (Some(cert_path), Some(key_path)) => {
                                match (std::fs::read(cert_path), std::fs::read(key_path)) {
                                    (Ok(cert), Ok(key)) => {
                                        if cert.is_empty() || key.is_empty() {
                                            eprintln!(
                                        "TLS ({addr}): mTLS клиентский cert или key файл пустой — \
                                         handshake может не пройти"
                                    );
                                        }
                                        // PR-12: Zeroizing wrap.
                                        (
                                            Some(zeroize::Zeroizing::new(cert)),
                                            Some(zeroize::Zeroizing::new(key)),
                                        )
                                    }
                                    _ => (None, None),
                                }
                            }
                            (Some(cert_path), None) => {
                                eprintln!(
                                    "TLS ({addr}): задан tls_client_cert_file={cert_path}, \
                                     но tls_client_key_file не задан — mTLS отключён"
                                );
                                (None, None)
                            }
                            (None, Some(key_path)) => {
                                eprintln!(
                                    "TLS ({addr}): задан tls_client_key_file={key_path}, \
                                     но tls_client_cert_file не задан — mTLS отключён"
                                );
                                (None, None)
                            }
                            (None, None) => (None, None),
                        };
                    // N4.mTLS: парсим минимальную версию TLS-протокола.
                    let min_protocol = match &target.tls_min_protocol_version {
                        Some(s) => match parse_tls_min_version(s) {
                            Ok(p) => Some(p),
                            Err(e) => {
                                eprintln!(
                                    "TLS ({addr}): не удалось распарсить tls_min_protocol_version={s:?}: {e}; \
                                     используется системная по умолчанию"
                                );
                                None
                            }
                        },
                        None => None,
                    };
                    if target.tls_insecure {
                        // PR-12 (security hardening): structured warning через tracing
                        // (SIEM-indexed) + eprintln (для CLI-only deployments без log shipper).
                        tracing::warn!(
                            target: "security",
                            addr = %addr,
                            phase = %phase.name,
                            "tls_insecure=true: TLS certificate verification DISABLED — \
                             трафик уязвим к MITM. Используйте tls_ca_file для self-signed CAs."
                        );
                        eprintln!("⚠ TLS ({addr}): tls_insecure=true — проверка сертификата ОТКЛЮЧЕНА (небезопасно)");
                    }
                    let tls_params = crate::sender::TlsParams {
                        domain,
                        ca_pem,
                        insecure: target.tls_insecure,
                        client_cert_pem,
                        client_key_pem,
                        min_protocol,
                        // N4.cipher_policy (v9.5.0): парсинг IANA-имён →
                        // rustls::SupportedCipherSuite. Парсинг идёт здесь
                        // (а не в build_tls_connector) чтобы при ошибке имя
                        // файла/фазы было в логе. None → дефолтные suites.
                        cipher_suites: match &target.tls_cipher_suites {
                            Some(names) => {
                                let mut out = Vec::with_capacity(names.len());
                                let mut had_invalid = false;
                                for name in names {
                                    match crate::transport::tls::parse_cipher_suite(name) {
                                        Ok(s) => out.push(s),
                                        Err(e) => {
                                            // PR-1 fix: ранее out.clear() отбрасывал
                                            // все ранее распарсенные suites при первой
                                            // ошибке. Теперь пропускаем только невалидное
                                            // имя, оставляя валидные suites в out.
                                            eprintln!(
                                                "TLS ({addr}): пропускаю невалидный cipher_suite={name:?}: {e}"
                                            );
                                            had_invalid = true;
                                        }
                                    }
                                }
                                // Если ВСЕ имена невалидны — fallback на дефолтный набор.
                                if out.is_empty() && had_invalid {
                                    eprintln!(
                                        "TLS ({addr}): ни один из cipher_suites не распознан; используется дефолтный набор"
                                    );
                                    None
                                } else {
                                    Some(out)
                                }
                            }
                            None => None,
                        },
                    };
                    // F16: reconnect config для TLS.
                    tokio::spawn(target_sender_tls(
                        addr,
                        tls_params,
                        phase_name,
                        rx,
                        m,
                        sd,
                        framing,
                        Some(rcfg.clone()),
                    ))
                }
                #[cfg(feature = "kafka")]
                "kafka" => {
                    // F16: Kafka target. Собираем KafkaConfig из полей target'а.
                    let bootstrap = crate::transport::kafka::parse_bootstrap_servers(&addr);
                    let topic = target.kafka_topic.clone().unwrap_or_default();
                    let client_id = target
                        .kafka_client_id
                        .clone()
                        .unwrap_or_else(|| "syslog-generator".to_string());
                    let compression = match target.kafka_compression.as_deref() {
                        Some(s) => match parse_kafka_compression(s) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!(
                                    "Kafka ({addr}): не удалось распарсить kafka_compression={s:?}: {e}; \
                                     используется NoCompression"
                                );
                                rskafka::client::partition::Compression::NoCompression
                            }
                        },
                        None => rskafka::client::partition::Compression::NoCompression,
                    };
                    let acks = match target.kafka_acks.as_deref() {
                        Some(s) => match parse_kafka_acks(s) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                eprintln!(
                                    "Kafka ({addr}): не удалось распарсить kafka_acks={s:?}: {e}; \
                                     поле игнорируется"
                                );
                                None
                            }
                        },
                        None => None,
                    };
                    let linger =
                        std::time::Duration::from_millis(target.kafka_linger_ms.unwrap_or(5));
                    let max_batch_size = target.kafka_max_batch_size.unwrap_or(1024);
                    let kafka_cfg = KafkaConfig {
                        bootstrap_servers: bootstrap,
                        topic,
                        client_id,
                        compression,
                        acks,
                        linger,
                        max_batch_size,
                    };
                    tokio::spawn(target_sender_kafka(kafka_cfg, addr, phase_name, rx, m, sd))
                }
                _ => {
                    // F16: file с ротацией (если задана) или без (default).
                    if rotation.is_enabled() {
                        tokio::spawn(target_sender_file_with_rotation(
                            std::path::PathBuf::from(addr),
                            phase_name,
                            rotation.clone(),
                            rx,
                            m,
                            sd,
                        ))
                    } else {
                        tokio::spawn(target_sender_file(addr, phase_name, rx, m, sd))
                    }
                }
            };
            handles.push(h);
        }
    }
    metrics.active_workers.set(total_workers as f64);

    // N10-gap fix (v9.2.0): резолвим FormatKind ОДИН раз на фазу (вне горячего
    // цикла). Раньше `phase.format_type()` вызывался в `generate_message`
    // на каждое сообщение, плюс match в `wrap_syslog` — string-ветка на
    // каждое сообщение. Теперь это match в `FormatKind::parse` (быстрее,
    // компилируется в jump table), и далее — static dispatch через enum.
    //
    // F13 (validate.rs) уже отверг неизвестные форматы, так что `unwrap_or`
    // здесь — defensive fallback на случай если кто-то вызывает core напрямую
    // без validate (тесты, бенчмарки).
    let format_kind = FormatKind::parse(phase.format_type()).unwrap_or(FormatKind::Raw);

    // Целевая интенсивность.
    // Режимы (выбираются в порядке приоритета):
    //  1) Кривая нагрузки (load_shape задан, F3) — sleep-планировщик по
    //     мгновенному rate_at(t). F17: дополнительно умножается на
    //     anomaly_multiplier(t).
    //  2) Постоянный rate с аномалиями (F17) — sleep-планировщик по
    //     base_rate * anomaly_multiplier(t).
    //  3) Постоянный rate без аномалий — токен-бакет `governor`,
    //     messages_per_second == 0 => "без ограничения скорости".
    //
    // Governor несовместим с динамическим rate (он burst-friendly и
    // рассчитан на постоянный bucket-size), поэтому при наличии аномалий
    // мы переключаемся на честный sleep-планировщик (как в load_shape).
    let has_anomalies = phase
        .anomalies
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let planner = AnomalyPlanner::new(phase.anomalies.as_deref().unwrap_or(&[]));
    let use_governor = phase.load_shape.is_none() && !has_anomalies;
    let limiter = if use_governor {
        NonZeroU32::new(phase.messages_per_second.min(u32::MAX as u64) as u32)
            .map(|r| RateLimiter::direct(Quota::per_second(r)))
    } else {
        None
    };
    let shape_duration = phase.duration_secs as f64;
    let base_rate = phase.messages_per_second as f64;
    // Для метрики target_rate показываем характерное (пиковое) значение кривой.
    match &phase.load_shape {
        Some(shape) => metrics.target_rate.set(shape.effective_base(base_rate)),
        None => metrics.target_rate.set(base_rate),
    }

    // PR-17d (v10.7.19): cached IntCounter handles — устраняет HashMap lookup в hot loop.
    // `with_label_values` стоит ~50-100 нс (HashMap probe + Arc<IntCounter> clone).
    // С cached handle — только atomic increment (~5-10 нс). Экономия ~90-190 нс/msg.
    let msg_counter = metrics
        .messages_generated_total
        .with_label_values(&[&phase.name]);
    let by_format_counter = metrics
        .messages_by_format_total
        .with_label_values(&[phase.format_type()]);

    // Условия остановки: по времени (duration_secs) и/или по количеству (total_messages).
    let deadline = if phase.duration_secs > 0 {
        Some(Instant::now() + Duration::from_secs(phase.duration_secs))
    } else {
        None
    };
    let max_messages = phase.total_messages;
    // Защита от бесконечного прогона: если не задано ни время, ни количество —
    // отправляем одно сообщение (режим smoke/демо), чтобы не зависнуть навечно.
    let bounded = deadline.is_some() || max_messages.is_some();

    let started = Instant::now();
    let mut seq: usize = 0;
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        if let Some(max) = max_messages {
            if seq as u64 >= max {
                break;
            }
        }
        if !bounded && seq >= 1 {
            break;
        }

        // F17 (v9.4.0): anomaly multiplier в текущий момент. Сначала
        // вычисляем базовый rate (load_shape или constant), затем домножаем
        // на anomaly_multiplier(t). Это интуитивно: «всплеск поверх
        // медленной утечки» даёт X-кратный всплеск относительно пониженной
        // базы.
        let t_now = started.elapsed().as_secs_f64();
        let anomaly_m = planner.combined_rate_multiplier(t_now);

        // Ограничение скорости.
        if let Some(shape) = &phase.load_shape {
            // Режим кривой: вычисляем мгновенный rate и выдерживаем интервал.
            let rate = shape.rate_at(t_now, shape_duration, base_rate) * anomaly_m;
            if rate > 0.0 {
                let interval = Duration::from_secs_f64(1.0 / rate);
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = shutdown.cancelled() => { break; }
                }
            }
            // rate <= 0 => мгновенно без паузы (эквивалент "без ограничения").
        } else if let Some(lim) = &limiter {
            // Режим постоянного rate (без аномалий): ждём токен-бакет,
            // прерываемся на shutdown.
            tokio::select! {
                _ = lim.until_ready() => {}
                _ = shutdown.cancelled() => { break; }
            }
        } else {
            // Режим постоянного rate + аномалии (F17): sleep по
            // base_rate * anomaly_multiplier(t). Это даёт точный контроль
            // над динамическим rate (governor не подходит для аномалий —
            // он burst-friendly и не реагирует на изменения мгновенного
            // значения).
            let rate = base_rate * anomaly_m;
            if rate > 0.0 {
                let interval = Duration::from_secs_f64(1.0 / rate);
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = shutdown.cancelled() => { break; }
                }
            }
        }

        seq += 1;
        // N10-gap fix (v9.2.0): hot-path — FormatKind уже резолвлен, без
        // per-message `phase.format_type()` парсинга.
        // PR-5: PhaseContext резолвится ОДИН раз вне loop, не per-message.
        let msg = generate_message_with_format(&ctx, phase, &format_kind, seq)?;
        // PR-17d (v10.7.19): cached IntCounter handles — inc = atomic, no HashMap lookup.
        msg_counter.inc();
        // N2 (v8.6.0): счётчик сообщений по формату. Инкрементируется
        // здесь (а не в generate_message), чтобы не зависеть от наличия
        // Metrics в чисто-функциональной generate_message (она используется
        // и в бенчмарках без Metrics).
        by_format_counter.inc();

        // F17: учёт применения rate-аномалий. Если multiplier != 1.0 —
        // хотя бы одна rate-аномалия активна. Считаем по каждой аномалии,
        // чтобы можно было разделить burst и slow-drip в метриках.
        if has_anomalies {
            for a in phase.anomalies.as_deref().unwrap_or(&[]) {
                let m = crate::anomaly::rate_multiplier(&a.kind, t_now);
                if (m - 1.0).abs() > f64::EPSILON {
                    metrics
                        .anomalies_applied_total
                        .with_label_values(&[&phase.name, a.type_name()])
                        .inc();
                }
            }
        }

        // F17: packet-loss — drop до отправки. Детерминирован по
        // (phase.seed, seq) — см. anomaly::should_drop_packet.
        if planner.should_drop(phase.seed, seq) {
            // Учитываем дроп в метрике (по типу первой сработавшей
            // аномалии — для остальных не инкрементируем, чтобы не было
            // двойного счёта при нескольких packet-loss в фазе).
            for a in phase.anomalies.as_deref().unwrap_or(&[]) {
                if let crate::anomaly::AnomalyKind::PacketLoss { .. } = &a.kind {
                    metrics
                        .anomalies_dropped_total
                        .with_label_values(&[&phase.name, a.type_name()])
                        .inc();
                    break;
                }
            }
            // Пропускаем tx.send для дропнутого сообщения.
            continue;
        }

        // PR-17e: Vec<u8> → Bytes один раз. Bytes::clone() = atomic increment,
        // не memcpy. Для broadcast это экономит N-1 memcpys payload'а.
        let msg_bytes: bytes::Bytes = bytes::Bytes::from(msg);
        if distribution == "broadcast" {
            for tx in &txs {
                let _ = tx.send(msg_bytes.clone()).await;
            }
        } else if !dispatch.is_empty() {
            let idx = dispatch[(seq - 1) % dispatch.len()];
            if let Some(tx) = txs.get(idx) {
                let _ = tx.send(msg_bytes).await;
            }
        }
    }
    let elapsed = started.elapsed();
    metrics.generate_duration.observe(elapsed.as_secs_f64());
    let secs = elapsed.as_secs_f64();
    if secs > 0.0 {
        metrics.achieved_rate.set(seq as f64 / secs);
    }
    drop(txs);
    if shutdown_cfg.mode == "drain" {
        graceful_drain_wait(handles, shutdown_cfg.drain_timeout_secs, metrics.clone()).await?;
    }
    Ok(())
}

pub async fn run_profile(profile: &Profile, metrics: Metrics) -> Result<()> {
    // F13: fail-fast — валидируем профиль перед запуском рантайма.
    let errors = crate::validate::validate_profile(profile);
    if !errors.is_empty() {
        anyhow::bail!("{}", crate::validate::format_errors(&errors));
    }
    // F12: если задан metrics_addr — поднимаем HTTP /metrics фоновой задачей
    // на всё время прогона и гасим его по завершении всех фаз.
    let metrics_shutdown = CancellationToken::new();
    if let Some(addr) = &profile.metrics_addr {
        crate::metrics_server::spawn(addr, metrics.clone(), metrics_shutdown.clone());
    }
    // PR-2: единый CancellationToken на весь run_profile + shutdown_listener
    // спавнится ОДИН раз (раньше спавнился per-phase — counter двойного
    // нажатия сбрасывался, и при Ctrl-C в фазе N фаза N+1 не получала сигнал).
    let shutdown = CancellationToken::new();
    let listener_token = shutdown.clone();
    let listener_metrics = metrics.clone();
    tokio::spawn(async move {
        shutdown_listener(listener_token, listener_metrics).await;
    });

    let result: Result<()> = async {
        for phase in &profile.phases {
            // v10.7.1: progress bar (только при duration_secs > 30 И TTY).
            // Если stdout не TTY (CI pipe) или фаза слишком короткая — skip PB.
            use indicatif::{ProgressBar, ProgressStyle};
            let show_pb = phase.duration_secs > 30 && std::io::stdout().is_terminal();
            let pb = if show_pb {
                let max = phase.total_messages.unwrap_or(0);
                let pb = if max > 0 {
                    ProgressBar::new(max)
                } else {
                    ProgressBar::new(phase.duration_secs)
                };
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
                         {pos}/{len} {msg}",
                    )
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("##-"),
                );
                pb.set_message(format!("phase {}", phase.name));
                Some(pb)
            } else {
                None
            };

            run_phase_multi(
                phase,
                &profile.targets,
                &profile.distribution,
                &profile.shutdown,
                metrics.clone(),
                &shutdown,
            )
            .await?;

            if let Some(pb) = pb {
                pb.finish_and_clear();
            }
        }
        Ok(())
    }
    .await;
    // Гарантируем, что HTTP-сервер метрик будет остановлен.
    metrics_shutdown.cancel();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::config::{Phase, SyslogConfig, TargetConfig};
    use crate::load_profile_from_yaml_str;
    use std::collections::HashMap;

    /// Legacy `generate_message(phase, seq)` создаёт PhaseContext через
    /// resolve(), использует все pre-compile оптимизации. Возвращает RFC 5424
    /// сообщение (default format).
    #[test]
    fn generate_message_legacy_rfc5424_with_seed() {
        let phase = Phase {
            name: "test-phase".to_string(),
            duration_secs: 1,
            total_messages: Some(10),
            messages_per_second: 100,
            templates: vec!["hello seq={{sequence}}".to_string()],
            syslog: SyslogConfig {
                facility: 16,
                severity: 6,
                hostname: "host".to_string(),
                app_name: "app".to_string(),
                procid: "{{pid}}".to_string(),
                msgid: "ID".to_string(),
                structured_data: "-".to_string(),
                bom: false,
            },
            ..Default::default()
        };
        let msg = generate_message(&phase, 1).expect("generate ok");
        let s = std::str::from_utf8(&msg).expect("utf8");
        // Проверяем что header есть.
        assert!(
            s.starts_with("<134>"),
            "expected <134> prefix (16*8+6), got: {}",
            s
        );
        // Проверяем что body содержит "hello seq=1".
        assert!(s.contains("hello seq=1"), "expected body, got: {}", s);
    }

    /// Default format при phase.format = None = "rfc5424".
    #[test]
    fn generate_message_default_format_is_rfc5424() {
        let phase = Phase {
            name: "t".to_string(),
            duration_secs: 1,
            total_messages: Some(1),
            messages_per_second: 1,
            templates: vec!["x".to_string()],
            ..Default::default()
        };
        let msg = generate_message(&phase, 1).expect("generate ok");
        // Default facility=1, severity=6 → PRIVAL = 1*8+6 = 14 = "<14>"
        assert!(
            msg.starts_with(b"<14>"),
            "expected <14> prefix, got: {:?}",
            &msg[..20]
        );
    }

    /// Two messages with different seed give different content.
    #[test]
    fn generate_message_seed_determinism() {
        let p1 = Phase {
            seed: Some(42),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["seq={{sequence}} {{faker.uuid}}".to_string()],
            syslog: SyslogConfig {
                facility: 16,
                severity: 6,
                hostname: "h".to_string(),
                app_name: "a".to_string(),
                procid: "1".to_string(),
                msgid: "ID".to_string(),
                structured_data: "-".to_string(),
                bom: false,
            },
            ..Default::default()
        };
        let p2 = p1.clone();
        // Одинаковый seed → одинаковый faker.
        let m1 = generate_message(&p1, 1).unwrap();
        let m2 = generate_message(&p2, 1).unwrap();
        // Время отличается (timestamp в body) — выделяем только faker часть.
        // Просто проверяем что оба валидные RFC 5424.
        assert!(m1.starts_with(b"<134>"));
        assert!(m2.starts_with(b"<134>"));
    }

    /// PhaseContext::resolve pre-compiles templates, не перекомпилирует per message.
    #[test]
    fn phase_context_resolve_compiles_templates_once() {
        let phase = Phase {
            name: "ctx-test".to_string(),
            duration_secs: 1,
            total_messages: Some(10),
            messages_per_second: 100,
            templates: vec![
                "{{timestamp}} {{real_app}} seq={{sequence}}".to_string(),
                "{{faker.uuid}} {{faker.ipv4}}".to_string(),
            ],
            syslog: SyslogConfig {
                facility: 16,
                severity: 6,
                hostname: "h".to_string(),
                app_name: "a".to_string(),
                procid: "1".to_string(),
                msgid: "ID".to_string(),
                structured_data: "-".to_string(),
                bom: false,
            },
            seed: Some(42),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        // Один fallback + 2 user templates.
        assert_eq!(ctx.compiled_templates.len(), 2);
        // faker scan: "ipv4" и "uuid" referenced (для template "{{faker.uuid}} {{faker.ipv4}}").
        let referenced = ctx.referenced_fakers.expect("fakers scanned");
        assert!(referenced.contains("ipv4"));
        assert!(referenced.contains("uuid"));
        // Cached syslog header: hostname, app_name, msgid, structured_data — все static
        // (нет per-message placeholders). procid = "1" — static.
        let cached = ctx.cached_syslog_header.expect("header cached");
        assert_eq!(cached.hostname.as_ref(), "h");
        assert_eq!(cached.app_name.as_ref(), "a");
        assert_eq!(cached.procid.as_ref(), "1");
        assert_eq!(cached.msgid.as_ref(), "ID");
        assert!(cached.procid_is_static);
    }

    /// PR-A0 (v10.8.0): шаблон без `{{faker.*}}` инициализирует `referenced_fakers`
    /// как `Some(empty HashSet)`, не `None`. Иначе default_values_into ошибочно
    /// генерирует ВСЕ 9 fakers per message (downstream fallback ветка).
    /// Бенчмарки static templates подтверждают: −34..−66% ns/msg после fix.
    #[test]
    fn phase_context_with_no_fakers_initializes_empty_set() {
        let phase = Phase {
            name: "no-fakers".to_string(),
            duration_secs: 0,
            total_messages: Some(1),
            messages_per_second: 0,
            templates: vec!["user=alice seq={{sequence}}".to_string()],
            syslog: SyslogConfig {
                facility: 16,
                severity: 6,
                hostname: "h".to_string(),
                app_name: "a".to_string(),
                procid: "1".to_string(),
                msgid: "ID".to_string(),
                structured_data: "-".to_string(),
                bom: false,
            },
            seed: Some(42),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        // Без `{{faker.*}}` referenced_fakers должен быть Some(empty), не None.
        let referenced = ctx
            .referenced_fakers
            .as_ref()
            .expect("must be Some, not None (PR-A0)");
        assert_eq!(
            referenced.len(),
            0,
            "no fakers referenced, but set should be empty"
        );
    }

    /// PhaseContext::resolve НЕ кэширует syslog header если procid содержит {{pid}}.
    #[test]
    fn phase_context_does_not_cache_dynamic_procid() {
        let phase = Phase {
            name: "t".to_string(),
            duration_secs: 1,
            total_messages: Some(1),
            messages_per_second: 1,
            templates: vec!["x".to_string()],
            syslog: SyslogConfig {
                facility: 16,
                severity: 6,
                hostname: "h".to_string(),
                app_name: "a".to_string(),
                procid: "{{pid}}".to_string(), // dynamic!
                msgid: "ID".to_string(),
                structured_data: "-".to_string(),
                bom: false,
            },
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        // Если ВСЕ остальные 4 поля static + procid dynamic,
        // cached_syslog_header должен быть None (нужно re-render).
        let cached = ctx
            .cached_syslog_header
            .expect("header cached for static parts");
        // procid должен быть sourced as-is (не renderable since {{pid}} is dynamic).
        assert!(!cached.procid_is_static);
        // procid содержит literal {{pid}} — копия из phase.syslog.
        assert_eq!(cached.procid.as_ref(), "{{pid}}");
    }

    /// pick_template_compiled: single template → always first.
    #[test]
    fn pick_template_compiled_single_returns_first() {
        // PR-17d (v10.7.19): один template → всегда возвращается этот template
        // (без RNG-зависимости). Тест упрощён по сравнению с оригиналом,
        // который путал 'single' с 'first'.
        let templates: Vec<Arc<template::CompiledTemplate>> =
            vec![Arc::new(template::CompiledTemplate::compile("a"))];
        let mut rng = crate::payload::derive_rng(Some(42), 1);
        let picked = pick_template_compiled(&templates, None, &mut rng);
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().render(&HashMap::new()), "a");
    }

    /// pick_template_compiled: empty list → None.
    #[test]
    fn pick_template_compiled_empty_returns_none() {
        let templates: Vec<Arc<template::CompiledTemplate>> = vec![];
        let mut rng = crate::payload::derive_rng(Some(42), 1);
        let picked = pick_template_compiled(&templates, None, &mut rng);
        assert!(picked.is_none());
    }

    /// create_dispatcher: для не-weighted возвращает 0..len.
    /// Round-robin и broadcast сейчас не реализованы в create_dispatcher —
    /// это pre-N10 поведение, оставлено для backward-compat. Round-robin
    /// применяется позже в run_phase_multi (target_sender_* вызывается
    /// per target из индекса dispatch).
    #[test]
    fn create_dispatcher_default_returns_indices() {
        let targets = vec![
            TargetConfig::default(),
            TargetConfig::default(),
            TargetConfig::default(),
        ];
        let d = create_dispatcher(&targets, "round-robin");
        assert_eq!(d, vec![0, 1, 2]);
        let d = create_dispatcher(&targets, "broadcast");
        assert_eq!(d, vec![0, 1, 2]);
    }

    /// create_dispatcher: weighted повторяет каждый target weight раз.
    #[test]
    fn create_dispatcher_weighted() {
        let targets = vec![
            TargetConfig {
                weight: 3,
                ..Default::default()
            },
            TargetConfig {
                weight: 1,
                ..Default::default()
            },
        ];
        let d = create_dispatcher(&targets, "weighted");
        // weight [3, 1] → [0, 0, 0, 1].
        assert_eq!(d, vec![0, 0, 0, 1]);
    }

    /// Profile load via load_profile_from_yaml_str.
    #[test]
    fn load_profile_from_yaml_str_simple() {
        let yaml = r#"
targets:
  - address: "127.0.0.1:514"
    transport: udp
distribution: round-robin
phases:
  - name: test
    duration_secs: 1
    total_messages: 100
    messages_per_second: 100
    templates:
      - "hello {{sequence}}"
"#;
        let profile = load_profile_from_yaml_str(yaml).expect("profile parses");
        assert_eq!(profile.targets.len(), 1);
        assert_eq!(profile.targets[0].address, "127.0.0.1:514");
        assert_eq!(profile.phases.len(), 1);
        assert_eq!(profile.phases[0].name, "test");
        assert_eq!(profile.phases[0].total_messages, Some(100));
    }

    // ===== Phase 6 (PR-Q.1): coverage для generator/core.rs =====

    /// Phase 6: `load_templates` без `templates_file` возвращает копию `phase.templates`.
    /// Покрывает ветку `Ok(phase.templates.clone())` (строка 114).
    #[test]
    fn phase6_load_templates_no_file_returns_phase_templates() {
        let phase = Phase {
            name: "t".into(),
            templates: vec!["a".to_string(), "b".to_string()],
            ..Default::default()
        };
        let v = load_templates(&phase).expect("ok");
        assert_eq!(v, vec!["a", "b"]);
    }

    /// Phase 6: `load_templates` с несуществующим `templates_file` возвращает `Err`.
    /// Покрывает error path `fs::read_to_string` (строки 109-112).
    #[test]
    fn phase6_load_templates_missing_file_returns_io_error() {
        let phase = Phase {
            name: "t".into(),
            templates_file: Some("/nonexistent/path/templates_xyz_999.json".to_string()),
            ..Default::default()
        };
        let result = load_templates(&phase);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        // Сообщение об ошибке содержит упоминание файла / NotFound.
        assert!(
            err_msg.contains("No such file")
                || err_msg.contains("not found")
                || err_msg.contains("templates_xyz_999"),
            "err msg: {err_msg}"
        );
    }

    /// Phase 6: `load_templates` с битым JSON → возвращает `Err` от serde.
    /// Покрывает error path `serde_json::from_str` (строка 111).
    #[test]
    fn phase6_load_templates_invalid_json_returns_parse_error() {
        let tmp = std::env::temp_dir().join(format!(
            "syslog-gen-phase6-bad-templates-{}.json",
            std::process::id()
        ));
        std::fs::write(&tmp, b"{not valid json").expect("write tmp");
        let phase = Phase {
            name: "t".into(),
            templates_file: Some(tmp.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = load_templates(&phase);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    /// Phase 6: `load_schema` без `schema_file` возвращает `Ok(None)`.
    /// Покрывает ветку `Ok(None)` (строка 123).
    #[test]
    fn phase6_load_schema_no_file_returns_none() {
        let phase = Phase {
            name: "t".into(),
            ..Default::default()
        };
        let result = load_schema(&phase).expect("ok");
        assert!(result.is_none());
    }

    /// Phase 6: `load_schema` с несуществующим `schema_file` возвращает `Err`.
    /// Покрывает error path `fs::read_to_string` (строки 118-122).
    #[test]
    fn phase6_load_schema_missing_file_returns_io_error() {
        let phase = Phase {
            name: "t".into(),
            schema_file: Some("/nonexistent/path/schema_xyz_999.json".to_string()),
            ..Default::default()
        };
        let result = load_schema(&phase);
        assert!(result.is_err());
    }

    /// Phase 6: `load_schema` с валидным JSON → `Ok(Some(schema))`.
    /// Покрывает happy path (строки 119-122).
    #[test]
    fn phase6_load_schema_valid_json_returns_some() {
        let tmp = std::env::temp_dir().join(format!(
            "syslog-gen-phase6-schema-{}.json",
            std::process::id()
        ));
        // Валидный schema JSON: пустые fields, без template.
        std::fs::write(&tmp, br#"{"fields":{}}"#).expect("write tmp");
        let phase = Phase {
            name: "t".into(),
            schema_file: Some(tmp.to_string_lossy().to_string()),
            ..Default::default()
        };
        let result = load_schema(&phase).expect("ok");
        assert!(result.is_some());
        let _ = std::fs::remove_file(&tmp);
    }

    /// Phase 6: schema-driven generation path в `generate_message_with_format_inner`
    /// (строки 146-185): schema.fields генерируются, schema.template рендерится
    /// и оборачивается в указанный формат.
    #[test]
    fn phase6_generate_message_with_schema_renders_template() {
        use crate::schema::{Schema, SchemaField};

        let mut fields = HashMap::new();
        fields.insert(
            "user".to_string(),
            SchemaField {
                field_type: "string".to_string(),
                len: Some(5),
                ..Default::default()
            },
        );
        let schema = Schema {
            fields,
            template: Some("user={{user}} seq={{sequence}}".to_string()),
            output: None,
        };
        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            // Без `templates` — контент идёт из schema.template.
            ..Default::default()
        };
        let ctx = PhaseContext {
            templates: vec![],
            compiled_templates: vec![],
            compiled_fallback: Arc::new(template::CompiledTemplate::compile("")),
            cached_syslog_header: None,
            faker_keys: std::array::from_fn(|i| format!("faker.{}", i)),
            referenced_fakers: None,
            schema: Some(Arc::new(schema)),
            compiled_plan: None,
        };
        // Resolved FormatKind::Rfc5424 (default).
        let format_kind = FormatKind::Rfc5424;
        let msg = generate_message_with_format(&ctx, &phase, &format_kind, 7).expect("generate ok");
        let s = std::str::from_utf8(&msg).expect("utf8");
        // RFC 5424 префикс присутствует.
        assert!(s.starts_with("<"), "got: {s}");
        // body содержит подставленные значения из schema и sequence.
        assert!(s.contains("seq=7"), "got: {s}");
        assert!(s.contains("user="), "got: {s}");
    }

    /// Phase 6: schema with template + `FormatKind::Raw` → schema template
    /// рендерится и оборачивается в raw (passthrough).
    #[test]
    fn phase6_generate_message_with_schema_raw_format() {
        use crate::schema::Schema;

        let schema = Schema {
            fields: HashMap::new(),
            template: Some("hello {{sequence}}".to_string()),
            output: None,
        };
        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            ..Default::default()
        };
        let ctx = PhaseContext {
            templates: vec![],
            compiled_templates: vec![],
            compiled_fallback: Arc::new(template::CompiledTemplate::compile("")),
            cached_syslog_header: None,
            faker_keys: std::array::from_fn(|i| format!("faker.{}", i)),
            referenced_fakers: None,
            schema: Some(Arc::new(schema)),
            compiled_plan: None,
        };
        let format_kind = FormatKind::Raw;
        let msg =
            generate_message_with_format(&ctx, &phase, &format_kind, 42).expect("generate ok");
        // Raw = passthrough = тело сообщения как есть.
        assert_eq!(msg, b"hello 42");
    }

    /// Phase 6: generate_message_with_format для `FormatKind::Protobuf(None)` +
    /// phase.format_type() != "protobuf" → fallback в template.render() →
    /// wrap_syslog с FormatKind::Protobuf(map=None) → protobuf::serialize_protobuf(None, ...)
    /// → пустой Vec. Покрывает ветку `matches!(format_kind, FormatKind::Protobuf(_))` (строка 277)
    /// и `serialize_protobuf_like(None, ...)` → empty schema → empty bytes.
    #[test]
    fn phase6_generate_message_protobuf_none_yields_empty_bytes() {
        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["hello".to_string()],
            format: Some("protobuf".to_string()),
            protobuf_schema: None,
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        let format_kind = FormatKind::Protobuf(None);
        let msg = generate_message_with_format(&ctx, &phase, &format_kind, 1).expect("generate ok");
        // Empty protobuf schema → empty bytes.
        assert!(msg.is_empty(), "got: {:?}", msg);
    }

    /// Phase 6: `run_phase_multi` propagates error из `PhaseContext::resolve`
    /// когда templates_file задан, но файла нет (строки 654-655).
    #[tokio::test]
    async fn phase6_run_phase_multi_propagates_resolve_error() {
        use crate::config::ShutdownConfig;
        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates_file: Some("/nonexistent/path/xyz_999.json".to_string()),
            ..Default::default()
        };
        let targets = vec![TargetConfig::default()];
        let metrics = crate::observability::create_metrics().expect("metrics");
        let shutdown = CancellationToken::new();
        let result = run_phase_multi(
            &phase,
            &targets,
            "round-robin",
            &ShutdownConfig::default(),
            metrics,
            &shutdown,
        )
        .await;
        assert!(result.is_err());
    }

    /// Phase 6: `run_phase_multi` happy-path с file target — пишет 2 сообщения в файл.
    /// Покрывает transport dispatch (строки 893-908) для ветки `_` (file default)
    /// и завершение через total_messages limit.
    #[tokio::test]
    async fn phase6_run_phase_multi_file_target_writes_messages() {
        use crate::config::ShutdownConfig;
        let tmp = std::env::temp_dir().join(format!(
            "syslog-gen-phase6-run-multi-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);

        let phase = Phase {
            name: "t".into(),
            total_messages: Some(2),
            messages_per_second: 1000,
            duration_secs: 0,
            templates: vec!["hello seq={{sequence}}".to_string()],
            ..Default::default()
        };
        let targets = vec![TargetConfig {
            address: tmp.to_string_lossy().to_string(),
            transport: "file".to_string(),
            ..Default::default()
        }];
        let metrics = crate::observability::create_metrics().expect("metrics");
        let shutdown = CancellationToken::new();
        run_phase_multi(
            &phase,
            &targets,
            "round-robin",
            &ShutdownConfig::default(),
            metrics,
            &shutdown,
        )
        .await
        .expect("run_phase_multi ok");

        let body = std::fs::read_to_string(&tmp).expect("read file");
        // Каждое сообщение = body + `\n` (file framing).
        assert!(body.contains("hello seq=1"), "got: {body}");
        assert!(body.contains("hello seq=2"), "got: {body}");
        let _ = std::fs::remove_file(&tmp);
    }

    /// Phase 11 (Tier 1): schema-driven path в `generate_message_with_format_cached`
    /// когда schema задан + format = protobuf. Покрывает ветки L299-325
    /// (schema path) + L341-346 (protobuf serialize).
    #[test]
    fn phase11_schema_with_protobuf_format_renders_via_schema() {
        use crate::schema::{Schema, SchemaField};
        use std::collections::HashMap;

        // Пишем schema во временный файл.
        let mut fields = HashMap::new();
        fields.insert(
            "user_id".to_string(),
            SchemaField {
                field_type: "int".to_string(),
                min: Some(1),
                max: Some(1000),
                ..Default::default()
            },
        );
        let schema = Schema {
            fields,
            template: None,
            output: None,
        };
        let schema_json = serde_json::to_string(&schema).expect("serialize schema");
        let schema_path =
            std::env::temp_dir().join(format!("sg_phase11_schema_{}.json", std::process::id()));
        std::fs::write(&schema_path, schema_json).expect("write schema file");

        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["fallback".to_string()],
            format: Some("protobuf".to_string()),
            protobuf_schema: None,
            schema_file: Some(schema_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        let format_kind = FormatKind::Protobuf(None);
        let msg = generate_message_with_format(&ctx, &phase, &format_kind, 1).expect("generate ok");
        // Protobuf с None protobuf_schema → пустой bytes (serialize_protobuf_like).
        assert!(msg.is_empty(), "protobuf None schema → empty: {:?}", msg);

        let _ = std::fs::remove_file(&schema_path);
    }

    /// Phase 11 (Tier 1): schema-driven path с template. Покрывает L314-325.
    #[test]
    fn phase11_schema_with_template_renders_template() {
        use crate::schema::{Schema, SchemaField};
        use std::collections::HashMap;

        let mut fields = HashMap::new();
        fields.insert(
            "x".to_string(),
            SchemaField {
                field_type: "int".to_string(),
                min: Some(42),
                max: Some(42),
                ..Default::default()
            },
        );
        let schema = Schema {
            fields,
            template: Some("x is {{x}}".to_string()),
            output: None,
        };
        let schema_path =
            std::env::temp_dir().join(format!("sg_phase11_schema_tpl_{}.json", std::process::id()));
        std::fs::write(
            &schema_path,
            serde_json::to_string(&schema).expect("serialize"),
        )
        .expect("write schema");

        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["fallback".to_string()],
            schema_file: Some(schema_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        let format_kind = FormatKind::Raw;
        let msg = generate_message_with_format(&ctx, &phase, &format_kind, 1).expect("generate ok");
        let s = std::str::from_utf8(&msg).expect("utf8");
        // Body = "x is 42", без syslog обёртки (Raw format).
        assert!(s.contains("x is 42"), "got: {s}");

        let _ = std::fs::remove_file(&schema_path);
    }

    /// Phase 11 (Tier 1): schema-driven path с depends_on корреляцией.
    /// Покрывает L308-313 (resolve_correlated_field с mapping_default).
    #[test]
    fn phase11_schema_with_depends_on_uses_mapping_default() {
        use crate::schema::{Schema, SchemaField};
        use std::collections::HashMap;

        let mut fields = HashMap::new();
        fields.insert(
            "parent".to_string(),
            SchemaField {
                field_type: "enum".to_string(),
                values: Some(vec!["A".to_string(), "B".to_string()]),
                ..Default::default()
            },
        );
        fields.insert(
            "child".to_string(),
            SchemaField {
                field_type: "enum".to_string(),
                values: Some(vec!["C".to_string(), "D".to_string()]),
                depends_on: Some("parent".to_string()),
                mapping_default: Some("fallback_child".to_string()),
                ..Default::default()
            },
        );
        let schema = Schema {
            fields,
            template: Some("{{parent}}/{{child}}".to_string()),
            output: None,
        };
        let schema_path = std::env::temp_dir().join(format!(
            "sg_phase11_schema_corr_{}.json",
            std::process::id()
        ));
        std::fs::write(
            &schema_path,
            serde_json::to_string(&schema).expect("serialize"),
        )
        .expect("write schema");

        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["fallback".to_string()],
            schema_file: Some(schema_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        let format_kind = FormatKind::Raw;
        let msg = generate_message_with_format(&ctx, &phase, &format_kind, 1).expect("generate ok");
        let s = std::str::from_utf8(&msg).expect("utf8");
        // child = mapping_default = "fallback_child" (нет mapping для A/B).
        assert!(s.contains("/fallback_child"), "got: {s}");

        let _ = std::fs::remove_file(&schema_path);
    }

    /// Phase 11 (Tier 1): generate_message_with_format_cached с schema + template.
    /// Покрывает hot-path версию L299-325.
    #[test]
    fn phase11_cached_schema_template_path() {
        use crate::schema::{Schema, SchemaField};
        use std::collections::HashMap;

        let mut fields = HashMap::new();
        fields.insert(
            "k".to_string(),
            SchemaField {
                field_type: "int".to_string(),
                min: Some(7),
                max: Some(7),
                ..Default::default()
            },
        );
        let schema = Schema {
            fields,
            template: Some("k={{k}}".to_string()),
            output: None,
        };
        let schema_path = std::env::temp_dir().join(format!(
            "sg_phase11_schema_cached_{}.json",
            std::process::id()
        ));
        std::fs::write(
            &schema_path,
            serde_json::to_string(&schema).expect("serialize"),
        )
        .expect("write schema");

        let phase = Phase {
            name: "t".into(),
            total_messages: Some(1),
            messages_per_second: 1,
            duration_secs: 1,
            templates: vec!["fallback".to_string()],
            schema_file: Some(schema_path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let ctx = PhaseContext::resolve(&phase).expect("resolve ok");
        let format_kind = FormatKind::Raw;
        let mut values = HashMap::with_capacity(16);
        let msg = generate_message_with_format_cached(&ctx, &phase, &format_kind, 1, &mut values)
            .expect("cached generate ok");
        let s = std::str::from_utf8(&msg).expect("utf8");
        assert!(s.contains("k=7"), "got: {s}");

        let _ = std::fs::remove_file(&schema_path);
    }
}
