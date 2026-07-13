use crate::format::{build_rfc3164, build_rfc5424, protobuf::serialize_protobuf_like, Header};
use crate::generator::config::{Phase, Profile, TargetConfig};
use crate::observability::metrics::Metrics;
use crate::schema::Schema;
use crate::shutdown::{graceful_drain_wait, shutdown_listener};
use crate::template::render_template;
use crate::transport::{
    parse_tls_min_version, target_sender_file, target_sender_tcp, target_sender_tls,
    target_sender_udp, Framing,
};
use anyhow::Result;
use governor::{Quota, RateLimiter};
use std::collections::HashMap;
use std::fs;
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
    phase: &Phase,
    seq: usize,
    rng: &mut rand::rngs::StdRng,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("sequence".to_string(), seq.to_string());
    m.insert("real_app".to_string(), phase.name.clone());
    m.insert("real_hostname".to_string(), "localhost".to_string());
    m.insert("hostname".to_string(), "localhost".to_string());
    m.insert("real_command".to_string(), "echo ok".to_string());
    // Реальное «now» в RFC3339 UTC (вместо захардкоженного времени).
    m.insert(
        "timestamp".to_string(),
        crate::payload::datetime_now_jitter(0, rng),
    );
    m.insert(
        "pid".to_string(),
        crate::payload::int_in_range(1, 65535, rng).to_string(),
    );
    // Вариативный faker-набор по умолчанию (F5): токены {{faker.*}}.
    for kind in [
        "ipv4",
        "ipv6",
        "mac",
        "uuid",
        "hostname",
        "username",
        "user_agent",
        "url",
        "http_status",
    ] {
        m.insert(format!("faker.{kind}"), crate::payload::faker(kind, rng));
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
    // F4: RNG детерминирован по (seed, seq) — один seed+seq даёт один вывод.
    // Без seed — энтропия ОС (вариативно, но не воспроизводимо).
    let mut rng = crate::payload::derive_rng(phase.seed, seq);
    let mut values = default_values(phase, seq, &mut rng);
    if let Some(schema) = load_schema(phase)? {
        // ВАЖНО (F4): HashMap итерируется в недетерминированном порядке, из-за чего
        // при одном seed RNG потреблялся бы в разной последовательности между
        // запусками. Сортируем поля по имени для полной воспроизводимости.
        //
        // F6 (межполевые корреляции): поле с `depends_on` должно генерироваться
        // ПОСЛЕ родителя. Делаем два прохода по отсортированному списку:
        //   1) поля без зависимостей — генерируются своим базовым типом;
        //   2) зависимые поля — значение берётся из `mapping` по значению родителя.
        // Двух проходов достаточно для одноуровневых корреляций (родитель→ребёнок).
        let mut fields: Vec<_> = schema.fields.into_iter().collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, field) in &fields {
            if field.depends_on.is_none() {
                let value = gen_schema_field(field, &mut rng);
                values.insert(name.clone(), value);
            }
        }
        for (name, field) in &fields {
            if let Some(parent) = &field.depends_on {
                let value = resolve_correlated_field(field, parent, &values, &mut rng);
                values.insert(name.clone(), value);
            }
        }
        if let Some(tpl) = schema.template {
            let body = render_template(&tpl, &values);
            return Ok(finish_body(phase, &values, body.into_bytes(), &mut rng));
        }
    }
    // F14: выбор шаблона из набора (случайный / взвешенный), а не всегда первый.
    let templates = load_templates(phase)?;
    let tpl = pick_template(&templates, phase.template_weights.as_deref(), &mut rng)
        .unwrap_or_else(|| "{{timestamp}} {{real_app}} seq={{sequence}}".to_string());
    if phase.format_type() == "protobuf" {
        return Ok(serialize_protobuf_like(
            phase.protobuf_schema.as_ref(),
            &values,
        ));
    }
    let body = render_template(&tpl, &values);
    Ok(finish_body(phase, &values, body.into_bytes(), &mut rng))
}

/// F14: выбрать шаблон из списка. Пустой список → None. Один шаблон → он.
/// Если `weights` заданы и длина совпадает — взвешенный выбор, иначе равновероятный.
fn pick_template(
    templates: &[String],
    weights: Option<&[f64]>,
    rng: &mut rand::rngs::StdRng,
) -> Option<String> {
    match templates.len() {
        0 => None,
        1 => Some(templates[0].clone()),
        n => {
            let idx = match weights {
                Some(w) if w.len() == n => crate::payload::weighted_index(w, rng),
                _ => crate::payload::int_in_range(0, (n - 1) as i64, rng) as usize,
            };
            Some(templates[idx].clone())
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
    phase: &Phase,
    values: &HashMap<String, String>,
    body: Vec<u8>,
    rng: &mut rand::rngs::StdRng,
) -> Vec<u8> {
    let body = match phase.pad_to_bytes {
        Some(n) if n > 0 => crate::payload::pad_to_size(body, n, rng),
        _ => body,
    };
    wrap_syslog(phase, values, body)
}

/// Обернуть тело сообщения (MSG) в syslog-конверт согласно `format`.
/// rfc5424/rfc3164 — валидный syslog; raw — сырой рендер без обёртки
/// (обратная совместимость). Строковые поля заголовка проходят подстановку шаблона.
fn wrap_syslog(phase: &Phase, values: &HashMap<String, String>, body: Vec<u8>) -> Vec<u8> {
    let s = &phase.syslog;
    let header = Header {
        facility: s.facility,
        severity: s.severity,
        hostname: render_template(&s.hostname, values),
        app_name: render_template(&s.app_name, values),
        procid: render_template(&s.procid, values),
        msgid: render_template(&s.msgid, values),
        structured_data: render_template(&s.structured_data, values),
        bom: s.bom,
    };
    match phase.format_type() {
        "rfc5424" => build_rfc5424(&header, &body),
        "rfc3164" => build_rfc3164(&header, &body),
        // "raw" и любые прочие значения — сырой рендер шаблона без обёртки.
        _ => body,
    }
}

pub async fn run_phase_multi(
    phase: &Phase,
    targets: &[TargetConfig],
    distribution: &str,
    shutdown_cfg: &crate::config::ShutdownConfig,
    metrics: Metrics,
) -> Result<()> {
    let shutdown = CancellationToken::new();
    let listener_token = shutdown.clone();
    let listener_metrics = metrics.clone();
    tokio::spawn(async move {
        shutdown_listener(listener_token, listener_metrics).await;
    });

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
        for _ in 0..pool_size {
            let rx = shared_rx.clone();
            let addr = target.address.clone();
            let phase_name = phase.name.clone();
            let m = metrics.clone();
            let sd = shutdown.clone();
            let h = match target.transport.as_str() {
                "tcp" => tokio::spawn(target_sender_tcp(addr, phase_name, rx, m, sd, framing)),
                "udp" => tokio::spawn(target_sender_udp(addr, phase_name, rx, m, sd)),
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
                    };
                    tokio::spawn(target_sender_tls(
                        addr, tls_params, phase_name, rx, m, sd, framing,
                    ))
                }
                _ => tokio::spawn(target_sender_file(addr, phase_name, rx, m, sd)),
            };
            handles.push(h);
        }
    }
    metrics.active_workers.set(total_workers as f64);

    // Целевая интенсивность.
    // Два режима:
    //  1) Постоянный rate (load_shape не задан) — токен-бакет `governor`,
    //     messages_per_second == 0 => “без ограничения скорости”.
    //  2) Кривая нагрузки (load_shape задан, F3) — sleep-планировщик по
    //     мгновенному rate_at(t); говернор не используется.
    let limiter = if phase.load_shape.is_none() {
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

        // Ограничение скорости.
        if let Some(shape) = &phase.load_shape {
            // Режим кривой: вычисляем мгновенный rate и выдерживаем интервал.
            let t = started.elapsed().as_secs_f64();
            let rate = shape.rate_at(t, shape_duration, base_rate);
            if rate > 0.0 {
                let interval = Duration::from_secs_f64(1.0 / rate);
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = shutdown.cancelled() => { break; }
                }
            }
            // rate <= 0 => мгновенно без паузы (эквивалент “без ограничения”).
        } else if let Some(lim) = &limiter {
            // Режим постоянного rate: ждём токен-бакет, прерываемся на shutdown.
            tokio::select! {
                _ = lim.until_ready() => {}
                _ = shutdown.cancelled() => { break; }
            }
        }

        seq += 1;
        let msg = generate_message(phase, seq)?;
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
    let result: Result<()> = async {
        for phase in &profile.phases {
            run_phase_multi(
                phase,
                &profile.targets,
                &profile.distribution,
                &profile.shutdown,
                metrics.clone(),
            )
            .await?;
        }
        Ok(())
    }
    .await;
    // Останавливаем HTTP-сервер метрик (если запускался).
    metrics_shutdown.cancel();
    result
}
