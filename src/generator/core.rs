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
use tokio::sync::{mpsc, Mutex};
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
    // Статические литералы (3 entries).
    m.insert("real_hostname".to_string(), "localhost".to_string());
    m.insert("hostname".to_string(), "localhost".to_string());
    m.insert("real_command".to_string(), "echo ok".to_string());
    // Динамические значения (4 entries).
    m.insert("sequence".to_string(), seq.to_string());
    m.insert("real_app".to_string(), phase.name.clone());
    m.insert(
        "timestamp".to_string(),
        crate::payload::datetime_now_jitter(0, rng),
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
/// `format_kind` принимается по ссылке (не Copy — `Protobuf` вариант несёт
/// `Option<ProtobufSchemaFieldMap>` с `HashMap`), чтобы избежать clone
/// в горячем цикле. Внутри `render`/`matches!` он только читается.
pub fn generate_message_with_format(
    ctx: &PhaseContext,
    phase: &Phase,
    format_kind: &FormatKind,
    seq: usize,
) -> Result<Vec<u8>> {
    let mut rng = crate::payload::derive_rng(phase.seed, seq);
    let mut values = default_values(ctx, phase, seq, &mut rng);
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
                let value = resolve_correlated_field(field, parent, &values, &mut rng);
                values.insert(name.to_string(), value);
            }
        }
        if let Some(tpl) = &schema.template {
            let body = render_template(tpl, &values);
            return Ok(finish_body(
                ctx,
                format_kind,
                phase,
                &values,
                body.into_bytes(),
                &mut rng,
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
            &values,
        ));
    }
    let body = tpl.render(&values);
    Ok(finish_body(
        ctx,
        format_kind,
        phase,
        &values,
        body.into_bytes(),
        &mut rng,
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
    /// PR-10: pre-built faker keys (статический `&'static [String; 9]`).
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
#[derive(Debug)]
pub struct SyslogHeaderParts {
    pub hostname: String,
    pub app_name: String,
    pub procid: String, // pre-rendered без `{{pid}}` если он есть
    pub msgid: String,
    pub structured_data: String,
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
        let mut referenced_fakers: Option<std::collections::HashSet<&'static str>> = None;
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
                    referenced_fakers
                        .get_or_insert_with(std::collections::HashSet::new)
                        .insert(kind);
                }
            }
        }

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
            let hostname = template::render_template(&s.hostname, &empty);
            let app_name = template::render_template(&s.app_name, &empty);
            let msgid = template::render_template(&s.msgid, &empty);
            let structured_data = template::render_template(&s.structured_data, &empty);
            let procid_is_static = !has_dynamic(&s.procid);
            let procid = if procid_is_static {
                template::render_template(&s.procid, &empty)
            } else {
                s.procid.clone() // перерендерим per message (нужен {{pid}})
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
fn finish_body(
    ctx: &PhaseContext,
    format_kind: &FormatKind,
    phase: &Phase,
    values: &HashMap<String, String>,
    body: Vec<u8>,
    rng: &mut rand::rngs::StdRng,
) -> Vec<u8> {
    let body = match phase.pad_to_bytes {
        Some(n) if n > 0 => crate::payload::pad_to_size(body, n, rng),
        _ => body,
    };
    wrap_syslog(ctx, format_kind, phase, values, body)
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
fn wrap_syslog(
    ctx: &PhaseContext,
    format_kind: &FormatKind,
    phase: &Phase,
    values: &HashMap<String, String>,
    body: Vec<u8>,
) -> Vec<u8> {
    let s = &phase.syslog;
    // PR-10: hot path. Если cached header есть (все syslog поля static
    // кроме возможно procid) — используем pre-rendered strings, re-render
    // только procid если он содержит {{pid}}.
    let (hostname, app_name, procid, msgid, structured_data) =
        match ctx.cached_syslog_header.as_ref() {
            Some(cached) => {
                let procid = if cached.procid_is_static {
                    cached.procid.clone()
                } else {
                    // Только procid содержит {{pid}} — re-render.
                    render_template(&s.procid, values)
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
                (
                    render_template(&s.hostname, values),
                    render_template(&s.app_name, values),
                    render_template(&s.procid, values),
                    render_template(&s.msgid, values),
                    render_template(&s.structured_data, values),
                )
            }
        };
    let header = Header {
        facility: s.facility,
        severity: s.severity,
        hostname,
        app_name,
        procid,
        msgid,
        structured_data,
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
        let shared_rx = Arc::new(Mutex::new(rx));
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
                            Ok(bytes) => Some(bytes),
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
                                        (Some(cert), Some(key))
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
        metrics
            .messages_generated_total
            .with_label_values(&[&phase.name])
            .inc();
        // N2 (v8.6.0): счётчик сообщений по формату. Инкрементируется
        // здесь (а не в generate_message), чтобы не зависеть от наличия
        // Metrics в чисто-функциональной generate_message (она используется
        // и в бенчмарках без Metrics).
        metrics
            .messages_by_format_total
            .with_label_values(&[phase.format_type()])
            .inc();

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

        if distribution == "broadcast" {
            for tx in &txs {
                let _ = tx.send(msg.clone()).await;
            }
        } else if !dispatch.is_empty() {
            let idx = dispatch[(seq - 1) % dispatch.len()];
            if let Some(tx) = txs.get(idx) {
                let _ = tx.send(msg).await;
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
                    .unwrap()
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
