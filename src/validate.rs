//! F13 — валидация профиля нагрузки перед запуском.
//!
//! Цель: fail-fast с внятными типизированными ошибками вместо паники в глубине
//! рантайма (например, неизвестный transport молча падал бы в UDP-ветке, а
//! опечатка в `format` тихо давала бы «сырой» вывод). Валидатор собирает **все**
//! обнаруженные проблемы (а не только первую), чтобы пользователь исправил их за
//! один проход.
//!
//! Валидация — чисто структурная и семантическая проверка полей `Profile`; она
//! не открывает сокеты и не читает `*_file`-ресурсы (это делает рантайм).

use crate::anomaly::AnomalyKind;
use crate::config::{Phase, Profile, SyslogConfig, TargetConfig};
use crate::load_shape::LoadShape;
use thiserror::Error;

/// Допустимые значения `transport` у цели.
pub const VALID_TRANSPORTS: &[&str] = &["tcp", "udp", "tls", "file", "kafka"];
/// Допустимые значения `format` фазы (F15: добавлены cef/leef/json_lines).
pub const VALID_FORMATS: &[&str] = &[
    "rfc5424",
    "rfc3164",
    "raw",
    "protobuf",
    "cef",
    "leef",
    "json_lines",
];
/// Допустимые значения `distribution` профиля.
pub const VALID_DISTRIBUTIONS: &[&str] = &["round-robin", "broadcast", "weighted"];
/// Допустимые значения `framing` (нормализуются в sender.rs, здесь — canonical + алиасы).
pub const VALID_FRAMINGS: &[&str] = &[
    "non-transparent",
    "octet-counting",
    "octet_counting",
    "octet",
];
/// Допустимые значения `shutdown.mode`.
pub const VALID_SHUTDOWN_MODES: &[&str] = &["drain", "immediate"];

/// Единичная ошибка валидации. Каждый вариант несёт достаточно контекста
/// (индекс/имя фазы, имя поля, недопустимое значение), чтобы пользователь сразу
/// понял, где именно и что чинить.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ValidationError {
    #[error("профиль не содержит ни одной фазы (phases пуст)")]
    NoPhases,

    #[error("target[{index}]: пустой адрес (address)")]
    EmptyTargetAddress { index: usize },

    #[error("target[{index}] (address={address:?}): недопустимый transport {value:?}; допустимо: {allowed}")]
    InvalidTransport {
        index: usize,
        address: String,
        value: String,
        allowed: String,
    },

    #[error("target[{index}] (address={address:?}): недопустимый framing {value:?}; допустимо: {allowed}")]
    InvalidFraming {
        index: usize,
        address: String,
        value: String,
        allowed: String,
    },

    #[error(
        "target[{index}] (address={address:?}): connections должно быть >= 1 (задано {value})"
    )]
    ZeroConnections {
        index: usize,
        address: String,
        value: usize,
    },

    #[error(
        "target[{index}] (address={address:?}): tls_ca_file {path:?} не существует или недоступен"
    )]
    TlsCaFileNotFound {
        index: usize,
        address: String,
        path: String,
    },

    /// N4.mTLS (v8.7.2): `tls_client_cert_file` задан, но файл не существует.
    #[error(
        "target[{index}] (address={address:?}): tls_client_cert_file {path:?} не существует или недоступен"
    )]
    TlsClientCertFileNotFound {
        index: usize,
        address: String,
        path: String,
    },

    /// N4.mTLS: `tls_client_key_file` задан, но файл не существует.
    #[error(
        "target[{index}] (address={address:?}): tls_client_key_file {path:?} не существует или недоступен"
    )]
    TlsClientKeyFileNotFound {
        index: usize,
        address: String,
        path: String,
    },

    /// N4.mTLS: `tls_min_protocol_version` имеет недопустимое значение
    /// (не "1.2" или "1.3"). Проверка fail-fast — иначе connector
    /// соберётся с системной минимальной версией (1.0), что небезопасно.
    #[error(
        "target[{index}] (address={address:?}): tls_min_protocol_version {value:?} недопустим; ожидается \"1.2\" или \"1.3\""
    )]
    InvalidTlsMinProtocolVersion {
        index: usize,
        address: String,
        value: String,
    },

    /// PR-12 (security hardening): tls_insecure=true отключает проверку
    /// сертификата (MITM trivial для misconfigured peers / attacker-controlled
    /// profile). Должно быть явной ошибкой валидации, не silent warning.
    /// Override через `--allow-insecure-tls` CLI flag (отдельный PR).
    #[error(
        "target[{index}] (address={address:?}): tls_insecure=true отключает TLS certificate verification — опасно (MITM). Используйте tls_ca_file для self-signed CAs"
    )]
    TlsInsecureEnabled { index: usize, address: String },

    #[error("недопустимый distribution {value:?}; допустимо: {allowed}")]
    InvalidDistribution { value: String, allowed: String },

    #[error("distribution=\"weighted\", но все веса targets равны нулю — суммарный вес 0, диспетчер пуст")]
    WeightedAllZero,

    #[error("недопустимый shutdown.mode {value:?}; допустимо: {allowed}")]
    InvalidShutdownMode { value: String, allowed: String },

    #[error("phase[{index}] ({name:?}): пустое имя фазы (name)")]
    EmptyPhaseName { index: usize, name: String },

    #[error("phase[{index}] ({name:?}): недопустимый format {value:?}; допустимо: {allowed}")]
    InvalidFormat {
        index: usize,
        name: String,
        value: String,
        allowed: String,
    },

    #[error("phase[{index}] ({name:?}): фаза не имеет ни одного шаблона (пусты и templates, и templates_file, и schema_file) — генерировать нечего")]
    NoContentSource { index: usize, name: String },

    #[error("phase[{index}] ({name:?}): фаза без ограничений остановки (duration_secs=0, total_messages=None) будет работать бесконечно")]
    UnboundedPhase { index: usize, name: String },

    #[error("phase[{index}] ({name:?}): template_weights (len={weights_len}) не совпадает с числом шаблонов (len={templates_len}) — веса будут проигнорированы")]
    TemplateWeightsMismatch {
        index: usize,
        name: String,
        weights_len: usize,
        templates_len: usize,
    },

    #[error(
        "phase[{index}] ({name:?}): template_weights содержит отрицательный или NaN вес ({value})"
    )]
    InvalidTemplateWeight {
        index: usize,
        name: String,
        value: f64,
    },

    #[error(
        "phase[{index}] ({name:?}): syslog.facility={value} вне диапазона 0..=23 (RFC 5424 §6.2.1)"
    )]
    InvalidFacility {
        index: usize,
        name: String,
        value: u8,
    },

    #[error(
        "phase[{index}] ({name:?}): syslog.severity={value} вне диапазона 0..=7 (RFC 5424 §6.2.1)"
    )]
    InvalidSeverity {
        index: usize,
        name: String,
        value: u8,
    },

    #[error("phase[{index}] ({name:?}): pad_to_bytes={value} — паддинг до 0 байт бессмыслен (используйте None для отключения)")]
    ZeroPadding {
        index: usize,
        name: String,
        value: usize,
    },

    #[error("phase[{index}] ({name:?}): load_shape.{field}={value} должно быть >= 0")]
    NegativeLoadShapeRate {
        index: usize,
        name: String,
        field: String,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): load_shape.{field}={value} должно быть > 0")]
    NonPositiveLoadShapePeriod {
        index: usize,
        name: String,
        field: String,
        value: f64,
    },

    // ===== F15 (v9.2.0): CEF/LEEF-специфичные ошибки =====
    /// F15: формат `cef` требует блок `cef: { device_vendor, device_product,
    /// device_version, signature_id, name }`. Без него нечего экранировать.
    #[error("phase[{index}] ({name:?}): format=cef требует непустой блок phase.cef с полями device_vendor/device_product/device_version/signature_id/name")]
    CefConfigMissing { index: usize, name: String },

    /// F15: одно из обязательных полей CEF пустое.
    #[error("phase[{index}] ({name:?}): format=cef требует непустое cef.{field}")]
    CefFieldEmpty {
        index: usize,
        name: String,
        field: String,
    },

    /// F15: CEF severity вне диапазона 0..=10.
    #[error(
        "phase[{index}] ({name:?}): cef.severity={value} вне диапазона 0..=10 (CEF-спецификация)"
    )]
    InvalidCefSeverity {
        index: usize,
        name: String,
        value: u8,
    },

    /// F15: формат `leef` требует блок `leef: { vendor, product, version, event_id }`.
    #[error("phase[{index}] ({name:?}): format=leef требует непустой блок phase.leef с полями vendor/product/version/event_id")]
    LeefConfigMissing { index: usize, name: String },

    /// F15: одно из обязательных полей LEEF пустое.
    #[error("phase[{index}] ({name:?}): format=leef требует непустое leef.{field}")]
    LeefFieldEmpty {
        index: usize,
        name: String,
        field: String,
    },

    // ===== N4.cipher_policy (v9.5.0) =====
    /// N4.cipher_policy: IANA-имя cipher suite не известно rustls.
    /// `name` — имя из profile.yaml, `allowed` — список поддерживаемых rustls suites.
    #[error(
        "target[{index}] (address={address:?}): tls_cipher_suites содержит неизвестное имя {name:?}; \
         допустимо: {allowed}"
    )]
    InvalidCipherSuite {
        index: usize,
        address: String,
        name: String,
        allowed: String,
    },

    // ===== F17 (v9.5.1): сценарии аномалий =====
    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].burst-injection.rate_multiplier={value} должно быть > 0")]
    InvalidAnomalyBurstMultiplier {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].burst-injection.interval_secs={value} должно быть > 0")]
    InvalidAnomalyBurstInterval {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].burst-injection.duration_secs={value} должно быть >= 0")]
    InvalidAnomalyBurstDuration {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].slow-drip.rate_divisor={value} должно быть > 1 (rate_divisor=1 даст отсутствие эффекта, <1 — ускорение вместо замедления)")]
    InvalidAnomalySlowDripDivisor {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].slow-drip.duration_secs={value} должно быть > 0")]
    InvalidAnomalySlowDripDuration {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    #[error("phase[{index}] ({name:?}): anomalies[{a_index}].packet-loss.loss_percent={value} вне диапазона 0..=100")]
    InvalidAnomalyPacketLossPercent {
        index: usize,
        name: String,
        a_index: usize,
        value: f64,
    },

    // === F16 (v9.3.0): Kafka/ротация/reconnect ===
    /// F16: `transport: "kafka"` указан, но `kafka_topic` не задан.
    /// Для Kafka-target'а topic обязателен (нет дефолта — broker не
    /// принимает "куда попало").
    #[error(
        "target[{index}] (address={address:?}): transport=\"kafka\" требует kafka_topic (поле обязательно)"
    )]
    KafkaTopicRequired { index: usize, address: String },

    /// F16: `transport: "kafka"` указан, но feature flag `kafka` не включён
    /// при компиляции. Нужно пересобрать с `--features kafka`.
    #[error(
        "target[{index}] (address={address:?}): transport=\"kafka\" требует cargo build --features kafka"
    )]
    KafkaFeatureDisabled { index: usize, address: String },

    /// F16: параметр ротации файла вырожденный (size/interval=0 при
    /// включённой ротации).
    #[error(
        "target[{index}] (address={address:?}): {field}={value} — должно быть > 0 при включённой ротации"
    )]
    InvalidFileRotation {
        index: usize,
        address: String,
        field: String,
        value: u64,
    },

    /// F16: file_rotation_max_files=0 (минимум 1).
    #[error("target[{index}] (address={address:?}): file_rotation_max_files=0 — должно быть >= 1")]
    ZeroFileRotationMaxFiles { index: usize, address: String },

    /// F16: reconnect_max_backoff_ms < reconnect_initial_backoff_ms.
    #[error(
        "target[{index}] (address={address:?}): reconnect_max_backoff_ms ({max}) должно быть >= reconnect_initial_backoff_ms ({initial})"
    )]
    InvalidReconnectBackoffRange {
        index: usize,
        address: String,
        initial: u64,
        max: u64,
    },

    /// F16: reconnect_multiplier < 1.0 или NaN.
    #[error(
        "target[{index}] (address={address:?}): reconnect_multiplier={value} — должно быть >= 1.0 и конечным"
    )]
    InvalidReconnectMultiplier {
        index: usize,
        address: String,
        value: f64,
    },

    /// F16: reconnect_initial_backoff_ms=0 (минимум 1).
    #[error(
        "target[{index}] (address={address:?}): reconnect_initial_backoff_ms=0 — должно быть > 0"
    )]
    ZeroReconnectInitialBackoff { index: usize, address: String },

    /// F16: `kafka_compression` имеет недопустимое значение.
    #[error(
        "target[{index}] (address={address:?}): kafka_compression={value:?} — ожидается одно из: none, gzip, snappy, lz4, zstd"
    )]
    InvalidKafkaCompression {
        index: usize,
        address: String,
        value: String,
    },

    /// F16: `kafka_acks` имеет недопустимое значение.
    #[error(
        "target[{index}] (address={address:?}): kafka_acks={value:?} — ожидается одно из: \"0\", \"1\", \"all\""
    )]
    InvalidKafkaAcks {
        index: usize,
        address: String,
        value: String,
    },
}

/// Проверяет профиль и возвращает список **всех** обнаруженных ошибок.
/// Пустой список означает, что профиль валиден.
pub fn validate_profile(profile: &Profile) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // distribution
    if !VALID_DISTRIBUTIONS.contains(&profile.distribution.as_str()) {
        errors.push(ValidationError::InvalidDistribution {
            value: profile.distribution.clone(),
            allowed: VALID_DISTRIBUTIONS.join(", "),
        });
    }

    // shutdown.mode
    if !VALID_SHUTDOWN_MODES.contains(&profile.shutdown.mode.as_str()) {
        errors.push(ValidationError::InvalidShutdownMode {
            value: profile.shutdown.mode.clone(),
            allowed: VALID_SHUTDOWN_MODES.join(", "),
        });
    }

    // targets
    for (index, t) in profile.targets.iter().enumerate() {
        validate_target(index, t, &mut errors);
    }

    // weighted-специфичная проверка: суммарный вес > 0
    if profile.distribution == "weighted"
        && !profile.targets.is_empty()
        && profile.targets.iter().all(|t| t.weight == 0)
    {
        errors.push(ValidationError::WeightedAllZero);
    }

    // phases
    if profile.phases.is_empty() {
        errors.push(ValidationError::NoPhases);
    }
    for (index, p) in profile.phases.iter().enumerate() {
        validate_phase(index, p, &mut errors);
    }

    errors
}

fn validate_target(index: usize, t: &TargetConfig, errors: &mut Vec<ValidationError>) {
    if t.address.trim().is_empty() {
        errors.push(ValidationError::EmptyTargetAddress { index });
    }
    if !VALID_TRANSPORTS.contains(&t.transport.as_str()) {
        errors.push(ValidationError::InvalidTransport {
            index,
            address: t.address.clone(),
            value: t.transport.clone(),
            allowed: VALID_TRANSPORTS.join(", "),
        });
    }
    // framing проверяем только для потоковых транспортов (tcp/tls); для udp/file
    // поле игнорируется рантаймом, но опечатку всё равно полезно подсветить.
    if !VALID_FRAMINGS.contains(&t.framing.as_str()) {
        errors.push(ValidationError::InvalidFraming {
            index,
            address: t.address.clone(),
            value: t.framing.clone(),
            allowed: VALID_FRAMINGS.join(", "),
        });
    }
    if t.connections == 0 {
        errors.push(ValidationError::ZeroConnections {
            index,
            address: t.address.clone(),
            value: t.connections,
        });
    }
    // N4: если задан tls_ca_file — файл должен существовать (проверяем независимо
    // от транспорта — наличие поля для не-TLS target'а обычно опечатка).
    if let Some(path) = &t.tls_ca_file {
        if !std::path::Path::new(path).is_file() {
            errors.push(ValidationError::TlsCaFileNotFound {
                index,
                address: t.address.clone(),
                path: path.clone(),
            });
        }
    }
    // N4.mTLS (v8.7.2): если задан `tls_client_cert_file` — файл должен
    // существовать (fail-fast — иначе handshake упадёт в рантайме с неясной
    // ошибкой).
    if let Some(path) = &t.tls_client_cert_file {
        if !std::path::Path::new(path).is_file() {
            errors.push(ValidationError::TlsClientCertFileNotFound {
                index,
                address: t.address.clone(),
                path: path.clone(),
            });
        }
    }
    // N4.mTLS: `tls_client_key_file` — аналогично.
    if let Some(path) = &t.tls_client_key_file {
        if !std::path::Path::new(path).is_file() {
            errors.push(ValidationError::TlsClientKeyFileNotFound {
                index,
                address: t.address.clone(),
                path: path.clone(),
            });
        }
    }
    // N4.mTLS: `tls_min_protocol_version` — допустимы только "1.2" и "1.3".
    // Невалидное значение → fail-fast, чтобы не собрать connector с
    // системным min=1.0 (небезопасно).
    if let Some(value) = &t.tls_min_protocol_version {
        if value != "1.2" && value != "1.3" {
            errors.push(ValidationError::InvalidTlsMinProtocolVersion {
                index,
                address: t.address.clone(),
                value: value.clone(),
            });
        }
    }
    // N4.cipher_policy (v9.5.0): каждое IANA-имя должно быть известно rustls.
    // Проверяем через `parse_cipher_suite` — это fail-fast, иначе ошибка
    // всплыла бы только в рантайме с непонятным rustls::Error.
    if let Some(names) = &t.tls_cipher_suites {
        for name in names {
            if crate::transport::tls::parse_cipher_suite(name).is_err() {
                errors.push(ValidationError::InvalidCipherSuite {
                    index,
                    address: t.address.clone(),
                    name: name.clone(),
                    allowed: crate::transport::tls::SUPPORTED_CIPHER_SUITE_NAMES.join(", "),
                });
            }
        }
    }

    // PR-12 (security hardening): tls_insecure=true — опасная опция (MITM).
    // В production должно быть hard error в F13 валидации. Override через
    // env var `ALLOW_INSECURE_TLS=1` (для тестов/специальных случаев).
    if t.tls_insecure && !std::env::var("ALLOW_INSECURE_TLS").is_ok_and(|v| v == "1") {
        errors.push(ValidationError::TlsInsecureEnabled {
            index,
            address: t.address.clone(),
        });
    }

    // F16: файловая ротация — параметры должны быть валидны.
    let rotation_enabled =
        t.file_rotation_size_mb.is_some() || t.file_rotation_interval_secs.is_some();
    if rotation_enabled {
        if let Some(mb) = t.file_rotation_size_mb {
            if mb == 0 {
                errors.push(ValidationError::InvalidFileRotation {
                    index,
                    address: t.address.clone(),
                    field: "file_rotation_size_mb".to_string(),
                    value: 0,
                });
            }
        }
        if let Some(s) = t.file_rotation_interval_secs {
            if s == 0 {
                errors.push(ValidationError::InvalidFileRotation {
                    index,
                    address: t.address.clone(),
                    field: "file_rotation_interval_secs".to_string(),
                    value: 0,
                });
            }
        }
        if let Some(m) = t.file_rotation_max_files {
            if m == 0 {
                errors.push(ValidationError::ZeroFileRotationMaxFiles {
                    index,
                    address: t.address.clone(),
                });
            }
        }
    }

    // F16: reconnect-стратегия — параметры должны быть валидны.
    if let Some(initial) = t.reconnect_initial_backoff_ms {
        if initial == 0 {
            errors.push(ValidationError::ZeroReconnectInitialBackoff {
                index,
                address: t.address.clone(),
            });
        }
    }
    if let Some(m) = t.reconnect_multiplier {
        if !m.is_finite() || m < 1.0 {
            errors.push(ValidationError::InvalidReconnectMultiplier {
                index,
                address: t.address.clone(),
                value: m,
            });
        }
    }
    if let (Some(initial), Some(max)) = (t.reconnect_initial_backoff_ms, t.reconnect_max_backoff_ms)
    {
        if max < initial {
            errors.push(ValidationError::InvalidReconnectBackoffRange {
                index,
                address: t.address.clone(),
                initial,
                max,
            });
        }
    }

    // === F16 (v9.3.0): Kafka/ротация/reconnect ===

    // PR-1 fix: Kafka-целевой transport требует feature flag `kafka`.
    // Без флага валидация ниже (`#[cfg(feature = "kafka")]`) компилируется out,
    // и target молча попадает в fallback на file-sender (silent fail).
    // Теперь emit'им явную ошибку, если feature выключен.
    if t.transport == "kafka" && !cfg!(feature = "kafka") {
        errors.push(ValidationError::KafkaFeatureDisabled {
            index,
            address: t.address.clone(),
        });
    }

    // F16: Kafka-специфичная валидация (только при feature flag).
    #[cfg(feature = "kafka")]
    if t.transport == "kafka" {
        // topic обязателен.
        if t.kafka_topic.as_deref().unwrap_or("").is_empty() {
            errors.push(ValidationError::KafkaTopicRequired {
                index,
                address: t.address.clone(),
            });
        }
        // compression — допустимые значения.
        if let Some(s) = &t.kafka_compression {
            let s_lower = s.trim().to_ascii_lowercase();
            if !matches!(
                s_lower.as_str(),
                "none" | "no" | "" | "gzip" | "snappy" | "lz4" | "zstd"
            ) {
                errors.push(ValidationError::InvalidKafkaCompression {
                    index,
                    address: t.address.clone(),
                    value: s.clone(),
                });
            }
        }
        // acks — допустимые значения.
        if let Some(s) = &t.kafka_acks {
            if !matches!(s.trim(), "0" | "1" | "all") {
                errors.push(ValidationError::InvalidKafkaAcks {
                    index,
                    address: t.address.clone(),
                    value: s.clone(),
                });
            }
        }
    }
}

fn validate_phase(index: usize, p: &Phase, errors: &mut Vec<ValidationError>) {
    let name = p.name.clone();

    if p.name.trim().is_empty() {
        errors.push(ValidationError::EmptyPhaseName {
            index,
            name: name.clone(),
        });
    }

    // format
    let fmt = p.format_type();
    if !VALID_FORMATS.contains(&fmt) {
        errors.push(ValidationError::InvalidFormat {
            index,
            name: name.clone(),
            value: fmt.to_string(),
            allowed: VALID_FORMATS.join(", "),
        });
    }

    // источник контента: должен быть хотя бы один из templates / templates_file / schema_file
    if p.templates.is_empty() && p.templates_file.is_none() && p.schema_file.is_none() {
        errors.push(ValidationError::NoContentSource {
            index,
            name: name.clone(),
        });
    }

    // бесконечная фаза без ограничений остановки
    if p.duration_secs == 0 && p.total_messages.is_none() {
        errors.push(ValidationError::UnboundedPhase {
            index,
            name: name.clone(),
        });
    }

    // template_weights
    if let Some(w) = &p.template_weights {
        if w.len() != p.templates.len() {
            errors.push(ValidationError::TemplateWeightsMismatch {
                index,
                name: name.clone(),
                weights_len: w.len(),
                templates_len: p.templates.len(),
            });
        }
        for &val in w {
            if val.is_nan() || val < 0.0 {
                errors.push(ValidationError::InvalidTemplateWeight {
                    index,
                    name: name.clone(),
                    value: val,
                });
            }
        }
    }

    // pad_to_bytes: 0 бессмыслен
    if let Some(0) = p.pad_to_bytes {
        errors.push(ValidationError::ZeroPadding {
            index,
            name: name.clone(),
            value: 0,
        });
    }

    // syslog facility/severity — только для форматов, где заголовок используется
    if fmt == "rfc5424" || fmt == "rfc3164" {
        validate_syslog(index, &name, &p.syslog, errors);
    }

    // F15: CEF/LEEF — обязательные поля конфигурации.
    match fmt {
        "cef" => validate_cef(index, &name, p.cef.as_ref(), errors),
        "leef" => validate_leef(index, &name, p.leef.as_ref(), errors),
        _ => {}
    }

    // load_shape
    if let Some(ls) = &p.load_shape {
        validate_load_shape(index, &name, ls, errors);
    }

    // F17 (v9.4.0): сценарии аномалий.
    if let Some(anomalies) = &p.anomalies {
        for (a_index, a) in anomalies.iter().enumerate() {
            validate_anomaly_kind(index, &name, a_index, &a.kind, errors);
        }
    }
}

fn validate_anomaly_kind(
    index: usize,
    name: &str,
    a_index: usize,
    kind: &AnomalyKind,
    errors: &mut Vec<ValidationError>,
) {
    match kind {
        AnomalyKind::BurstInjection {
            rate_multiplier,
            interval_secs,
            duration_secs,
        } => {
            if !rate_multiplier.is_finite() || *rate_multiplier <= 0.0 {
                errors.push(ValidationError::InvalidAnomalyBurstMultiplier {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *rate_multiplier,
                });
            }
            if !interval_secs.is_finite() || *interval_secs <= 0.0 {
                errors.push(ValidationError::InvalidAnomalyBurstInterval {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *interval_secs,
                });
            }
            if !duration_secs.is_finite() || *duration_secs < 0.0 {
                errors.push(ValidationError::InvalidAnomalyBurstDuration {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *duration_secs,
                });
            }
        }
        AnomalyKind::SlowDrip {
            rate_divisor,
            duration_secs,
        } => {
            // divisor > 1 (иначе нет смысла в "drip" — divisor=1 даст rate*=1,
            // divisor<1 ускорит фазу, что не slow-drip по семантике).
            if !rate_divisor.is_finite() || *rate_divisor <= 1.0 {
                errors.push(ValidationError::InvalidAnomalySlowDripDivisor {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *rate_divisor,
                });
            }
            if !duration_secs.is_finite() || *duration_secs <= 0.0 {
                errors.push(ValidationError::InvalidAnomalySlowDripDuration {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *duration_secs,
                });
            }
        }
        AnomalyKind::PacketLoss { loss_percent } => {
            // 0..=100. NaN тоже отклоняем.
            if !loss_percent.is_finite() || *loss_percent < 0.0 || *loss_percent > 100.0 {
                errors.push(ValidationError::InvalidAnomalyPacketLossPercent {
                    index,
                    name: name.to_string(),
                    a_index,
                    value: *loss_percent,
                });
            }
        }
    }
}

fn validate_cef(
    index: usize,
    name: &str,
    cef: Option<&crate::config::CefConfig>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(c) = cef else {
        errors.push(ValidationError::CefConfigMissing {
            index,
            name: name.to_string(),
        });
        return;
    };
    for field in [
        "device_vendor",
        "device_product",
        "device_version",
        "signature_id",
        "name",
    ] {
        // Через match, чтобы не использовать `reflect`-подобные трюки.
        // Неизвестные поля пропускаем (defensive — массив полей может быть
        // расширен в новой версии без обновления match здесь).
        let v = match field {
            "device_vendor" => &c.device_vendor,
            "device_product" => &c.device_product,
            "device_version" => &c.device_version,
            "signature_id" => &c.signature_id,
            "name" => &c.name,
            _ => continue,
        };
        if v.trim().is_empty() {
            errors.push(ValidationError::CefFieldEmpty {
                index,
                name: name.to_string(),
                field: field.to_string(),
            });
        }
    }
    if let Some(sev) = c.severity {
        if sev > 10 {
            errors.push(ValidationError::InvalidCefSeverity {
                index,
                name: name.to_string(),
                value: sev,
            });
        }
    }
}

fn validate_leef(
    index: usize,
    name: &str,
    leef: Option<&crate::config::LeefConfig>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(l) = leef else {
        errors.push(ValidationError::LeefConfigMissing {
            index,
            name: name.to_string(),
        });
        return;
    };
    for field in ["vendor", "product", "version", "event_id"] {
        // Неизвестные поля пропускаем (defensive).
        let v = match field {
            "vendor" => &l.vendor,
            "product" => &l.product,
            "version" => &l.version,
            "event_id" => &l.event_id,
            _ => continue,
        };
        if v.trim().is_empty() {
            errors.push(ValidationError::LeefFieldEmpty {
                index,
                name: name.to_string(),
                field: field.to_string(),
            });
        }
    }
}

fn validate_syslog(index: usize, name: &str, s: &SyslogConfig, errors: &mut Vec<ValidationError>) {
    if s.facility > 23 {
        errors.push(ValidationError::InvalidFacility {
            index,
            name: name.to_string(),
            value: s.facility,
        });
    }
    if s.severity > 7 {
        errors.push(ValidationError::InvalidSeverity {
            index,
            name: name.to_string(),
            value: s.severity,
        });
    }
}

fn push_neg(errors: &mut Vec<ValidationError>, index: usize, name: &str, field: &str, value: f64) {
    if value.is_nan() || value < 0.0 {
        errors.push(ValidationError::NegativeLoadShapeRate {
            index,
            name: name.to_string(),
            field: field.to_string(),
            value,
        });
    }
}

fn validate_load_shape(
    index: usize,
    name: &str,
    ls: &LoadShape,
    errors: &mut Vec<ValidationError>,
) {
    match ls {
        LoadShape::Constant { rate } => {
            if let Some(r) = rate {
                push_neg(errors, index, name, "rate", *r);
            }
        }
        LoadShape::Linear {
            start_rate,
            end_rate,
        } => {
            push_neg(errors, index, name, "start_rate", *start_rate);
            push_neg(errors, index, name, "end_rate", *end_rate);
        }
        LoadShape::Sine {
            min_rate,
            max_rate,
            period_secs,
        } => {
            push_neg(errors, index, name, "min_rate", *min_rate);
            push_neg(errors, index, name, "max_rate", *max_rate);
            if period_secs.is_nan() || *period_secs <= 0.0 {
                errors.push(ValidationError::NonPositiveLoadShapePeriod {
                    index,
                    name: name.to_string(),
                    field: "period_secs".to_string(),
                    value: *period_secs,
                });
            }
        }
        LoadShape::Burst {
            base_rate,
            burst_rate,
            every_secs,
            burst_secs,
        } => {
            push_neg(errors, index, name, "base_rate", *base_rate);
            push_neg(errors, index, name, "burst_rate", *burst_rate);
            if every_secs.is_nan() || *every_secs <= 0.0 {
                errors.push(ValidationError::NonPositiveLoadShapePeriod {
                    index,
                    name: name.to_string(),
                    field: "every_secs".to_string(),
                    value: *every_secs,
                });
            }
            push_neg(errors, index, name, "burst_secs", *burst_secs);
        }
    }
}

/// Форматирует список ошибок в человекочитаемый многострочный отчёт.
pub fn format_errors(errors: &[ValidationError]) -> String {
    let mut out = format!("профиль невалиден: найдено {} проблем(ы):\n", errors.len());
    for (i, e) in errors.iter().enumerate() {
        out.push_str(&format!("  {}. {}\n", i + 1, e));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Phase, Profile, ShutdownConfig, TargetConfig};

    /// Assert helper: проверяет наличие `ValidationError`, удовлетворяющего
    /// `matcher`'у, который имеет доступ к **содержимому** варианта
    /// (index, address, value, message, …) — а не только к варианту enum'а.
    ///
    /// Существующие тесты часто использовали
    /// `errs.iter().any(|e| matches!(e, InvalidTransport { .. }))`,
    /// что ловит **вариант**, но не его поля. Этот helper — переходный
    /// API: вместо немедленного переписывания ~38 слабых assertions
    /// в этом файле, даём им единый путь к усилению — добавь
    /// `assert_validation_error(...)` рядом с существующим `.any(...)`
    /// и замени matcher на closure с проверкой полей.
    ///
    /// Пример:
    /// ```ignore
    /// assert_validation_error(&errs, |e| match e {
    ///     ValidationError::InvalidTransport { index, address, value, .. } => {
    ///         assert_eq!(*index, 0);
    ///         assert_eq!(address, "127.0.0.1:514");
    ///         assert_eq!(value, "sctp");
    ///         true
    ///     }
    ///     _ => false,
    /// });
    /// ```
    /// PR-Q.1: helper используется в новых и постепенно мигрируемых тестах;
    /// старые `errs.iter().any(|e| matches!(e, InvalidX { .. }))` остаются
    /// на месте до полной миграции (Phase 6 — Coverage P1).
    #[allow(dead_code)] // используется при поэтапной миграции assertions
    fn assert_validation_error<F>(errs: &[ValidationError], matcher: F)
    where
        F: Fn(&ValidationError) -> bool,
    {
        assert!(
            errs.iter().any(matcher),
            "expected matching ValidationError not found in: {:?}",
            errs
        );
    }

    fn valid_phase() -> Phase {
        Phase {
            name: "warmup".to_string(),
            duration_secs: 10,
            templates: vec!["hello {{sequence}}".to_string()],
            ..Default::default()
        }
    }

    fn valid_target() -> TargetConfig {
        TargetConfig {
            address: "127.0.0.1:514".to_string(),
            transport: "tcp".to_string(),
            ..Default::default()
        }
    }

    fn valid_profile() -> Profile {
        Profile {
            targets: vec![valid_target()],
            distribution: "round-robin".to_string(),
            shutdown: ShutdownConfig::default(),
            phases: vec![valid_phase()],
            metrics_addr: None,
        }
    }

    #[test]
    fn accepts_valid_profile() {
        assert!(validate_profile(&valid_profile()).is_empty());
    }

    #[test]
    fn rejects_empty_phases() {
        let mut p = valid_profile();
        p.phases.clear();
        let errs = validate_profile(&p);
        assert!(errs.contains(&ValidationError::NoPhases));
    }

    #[test]
    fn rejects_bad_transport() {
        let mut p = valid_profile();
        p.targets[0].transport = "sctp".to_string();
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidTransport { .. })));
    }

    #[test]
    fn rejects_bad_format() {
        let mut p = valid_profile();
        p.phases[0].format = Some("xml".to_string());
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidFormat { .. })));
    }

    #[test]
    fn rejects_bad_distribution() {
        let mut p = valid_profile();
        p.distribution = "hash".to_string();
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidDistribution { .. })));
    }

    #[test]
    fn rejects_no_content_source() {
        let mut p = valid_profile();
        p.phases[0].templates.clear();
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::NoContentSource { .. })));
    }

    #[test]
    fn rejects_unbounded_phase() {
        let mut p = valid_profile();
        p.phases[0].duration_secs = 0;
        p.phases[0].total_messages = None;
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::UnboundedPhase { .. })));
    }

    #[test]
    fn accepts_unbounded_when_total_messages_set() {
        let mut p = valid_profile();
        p.phases[0].duration_secs = 0;
        p.phases[0].total_messages = Some(100);
        let errs = validate_profile(&p);
        assert!(!errs
            .iter()
            .any(|e| matches!(e, ValidationError::UnboundedPhase { .. })));
    }

    #[test]
    fn rejects_bad_severity_facility() {
        let mut p = valid_profile();
        p.phases[0].syslog.severity = 9;
        p.phases[0].syslog.facility = 30;
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidSeverity { .. })));
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidFacility { .. })));
    }

    #[test]
    fn severity_not_checked_for_raw_format() {
        let mut p = valid_profile();
        p.phases[0].format = Some("raw".to_string());
        p.phases[0].syslog.severity = 9;
        let errs = validate_profile(&p);
        assert!(!errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidSeverity { .. })));
    }

    #[test]
    fn rejects_template_weights_mismatch() {
        let mut p = valid_profile();
        p.phases[0].templates = vec!["a".to_string(), "b".to_string()];
        p.phases[0].template_weights = Some(vec![1.0]);
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::TemplateWeightsMismatch { .. })));
    }

    #[test]
    fn rejects_zero_connections() {
        let mut p = valid_profile();
        p.targets[0].connections = 0;
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::ZeroConnections { .. })));
    }

    #[test]
    fn rejects_weighted_all_zero() {
        let mut p = valid_profile();
        p.distribution = "weighted".to_string();
        p.targets[0].weight = 0;
        let errs = validate_profile(&p);
        assert!(errs.contains(&ValidationError::WeightedAllZero));
    }

    #[test]
    fn collects_multiple_errors() {
        let mut p = valid_profile();
        p.targets[0].transport = "bad".to_string();
        p.distribution = "bad".to_string();
        p.phases.clear();
        let errs = validate_profile(&p);
        assert!(errs.len() >= 3);
    }

    // ===== F17 (v9.4.0): сценарии аномалий =====

    fn anomaly_phase_with(kind: crate::anomaly::AnomalyKind) -> Phase {
        let mut p = valid_phase();
        p.anomalies = Some(vec![crate::anomaly::Anomaly { kind }]);
        p
    }

    #[test]
    fn f17_accepts_valid_burst_injection() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::BurstInjection {
            rate_multiplier: 5.0,
            interval_secs: 30.0,
            duration_secs: 2.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        assert!(validate_profile(&profile).is_empty());
    }

    #[test]
    fn f17_accepts_valid_slow_drip() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::SlowDrip {
            rate_divisor: 10.0,
            duration_secs: 60.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        assert!(validate_profile(&profile).is_empty());
    }

    #[test]
    fn f17_accepts_valid_packet_loss() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::PacketLoss { loss_percent: 30.0 });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        assert!(validate_profile(&profile).is_empty());
    }

    #[test]
    fn f17_rejects_burst_zero_multiplier() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::BurstInjection {
            rate_multiplier: 0.0,
            interval_secs: 30.0,
            duration_secs: 2.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstMultiplier { .. })));
    }

    #[test]
    fn f17_rejects_burst_negative_multiplier() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::BurstInjection {
            rate_multiplier: -1.0,
            interval_secs: 30.0,
            duration_secs: 2.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstMultiplier { .. })));
    }

    #[test]
    fn f17_rejects_burst_zero_interval() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::BurstInjection {
            rate_multiplier: 5.0,
            interval_secs: 0.0,
            duration_secs: 2.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstInterval { .. })));
    }

    #[test]
    fn f17_rejects_burst_negative_duration() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::BurstInjection {
            rate_multiplier: 5.0,
            interval_secs: 30.0,
            duration_secs: -1.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstDuration { .. })));
    }

    #[test]
    fn f17_rejects_slow_drip_divisor_le_one() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::SlowDrip {
            rate_divisor: 1.0,
            duration_secs: 10.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalySlowDripDivisor { .. })));
    }

    #[test]
    fn f17_rejects_slow_drip_zero_duration() {
        let p = anomaly_phase_with(crate::anomaly::AnomalyKind::SlowDrip {
            rate_divisor: 5.0,
            duration_secs: 0.0,
        });
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalySlowDripDuration { .. })));
    }

    #[test]
    fn f17_rejects_packet_loss_out_of_range() {
        for bad in [-1.0, 100.5, f64::NAN] {
            let p =
                anomaly_phase_with(crate::anomaly::AnomalyKind::PacketLoss { loss_percent: bad });
            let mut profile = valid_profile();
            profile.phases = vec![p];
            let errs = validate_profile(&profile);
            assert!(
                errs.iter()
                    .any(|e| matches!(e, ValidationError::InvalidAnomalyPacketLossPercent { .. })),
                "loss_percent={bad} должно быть отклонено"
            );
        }
    }

    #[test]
    fn f17_accepts_packet_loss_boundary_values() {
        for ok in [0.0, 100.0] {
            let p =
                anomaly_phase_with(crate::anomaly::AnomalyKind::PacketLoss { loss_percent: ok });
            let mut profile = valid_profile();
            profile.phases = vec![p];
            let errs = validate_profile(&profile);
            assert!(
                !errs
                    .iter()
                    .any(|e| matches!(e, ValidationError::InvalidAnomalyPacketLossPercent { .. })),
                "loss_percent={ok} на границе должно быть принято"
            );
        }
    }

    #[test]
    fn f17_collects_multiple_anomaly_errors() {
        let mut p = valid_phase();
        p.anomalies = Some(vec![
            crate::anomaly::Anomaly {
                kind: crate::anomaly::AnomalyKind::BurstInjection {
                    rate_multiplier: 0.0, // ошибка
                    interval_secs: 0.0,   // тоже ошибка
                    duration_secs: 2.0,
                },
            },
            crate::anomaly::Anomaly {
                kind: crate::anomaly::AnomalyKind::PacketLoss {
                    loss_percent: 150.0, // ошибка
                },
            },
        ]);
        let mut profile = valid_profile();
        profile.phases = vec![p];
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstMultiplier { .. })));
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyBurstInterval { .. })));
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidAnomalyPacketLossPercent { .. })));
    }

    /// PR-11: тесты на ValidationError variants которые не покрыты основными тестами.
    /// Каждый variant должен иметь хотя бы один happy-path test.
    #[cfg(feature = "kafka")]
    #[test]
    fn kafka_topic_required_when_kafka_transport() {
        let mut profile = valid_profile();
        profile.targets[0].transport = "kafka".to_string();
        profile.targets[0].kafka_topic = None; // required
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::KafkaTopicRequired { .. })));
    }

    #[cfg(feature = "kafka")]
    #[test]
    fn invalid_kafka_compression_value() {
        let mut profile = valid_profile();
        profile.targets[0].transport = "kafka".to_string();
        profile.targets[0].kafka_topic = Some("test".to_string());
        profile.targets[0].kafka_compression = Some("brotli".to_string()); // invalid
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidKafkaCompression { .. })));
    }

    #[cfg(feature = "kafka")]
    #[test]
    fn invalid_kafka_acks_value() {
        let mut profile = valid_profile();
        profile.targets[0].transport = "kafka".to_string();
        profile.targets[0].kafka_topic = Some("test".to_string());
        profile.targets[0].kafka_acks = Some("2".to_string()); // invalid (допустимо 0, 1, all)
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidKafkaAcks { .. })));
    }

    #[test]
    fn invalid_file_rotation_size_zero() {
        let mut profile = valid_profile();
        profile.targets[0].file_rotation_size_mb = Some(0); // invalid
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidFileRotation { .. })));
    }

    #[test]
    fn invalid_file_rotation_interval_zero() {
        let mut profile = valid_profile();
        profile.targets[0].file_rotation_interval_secs = Some(0);
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidFileRotation { .. })));
    }

    #[test]
    fn invalid_reconnect_backoff_range() {
        let mut profile = valid_profile();
        profile.targets[0].reconnect_max_backoff_ms = Some(50);
        profile.targets[0].reconnect_initial_backoff_ms = Some(100); // max < initial — invalid
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidReconnectBackoffRange { .. })));
    }

    #[test]
    fn invalid_cef_config_missing_required_fields() {
        let mut profile = valid_profile();
        profile.phases[0].format = Some("cef".to_string());
        // Без cef config.
        profile.phases[0].cef = None;
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::CefConfigMissing { .. })));
    }

    #[test]
    fn invalid_leef_config_missing_required_fields() {
        let mut profile = valid_profile();
        profile.phases[0].format = Some("leef".to_string());
        profile.phases[0].leef = None;
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::LeefConfigMissing { .. })));
    }

    #[test]
    fn invalid_tls_cipher_suite_name() {
        let mut profile = valid_profile();
        profile.targets[0].transport = "tls".to_string();
        profile.targets[0].tls_cipher_suites = Some(vec!["BOGUS_CIPHER".to_string()]);
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidCipherSuite { .. })));
    }

    #[test]
    fn invalid_cef_severity_too_high() {
        let mut profile = valid_profile();
        profile.phases[0].format = Some("cef".to_string());
        profile.phases[0].cef = Some(crate::generator::config::CefConfig {
            device_vendor: "V".to_string(),
            device_product: "P".to_string(),
            device_version: "1".to_string(),
            signature_id: "1".to_string(),
            name: "n".to_string(),
            severity: Some(99), // > 10, invalid
            extensions: Some(std::collections::BTreeMap::new()),
        });
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidCefSeverity { .. })));
    }

    #[test]
    fn negative_load_shape_rate() {
        let mut profile = valid_profile();
        profile.phases[0].load_shape = Some(crate::load_shape::LoadShape::Linear {
            start_rate: -10.0,
            end_rate: 100.0,
        });
        let errs = validate_profile(&profile);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::NegativeLoadShapeRate { .. })));
    }

    /// PR-16 (coverage): leef_field_validation_catches_empty_fields.
    /// `validate_leef` field-loop body was uncovered (14 lines). Тест
    /// покрывает оба случая (vendor="" и product="" одновременно).
    #[test]
    fn leef_field_validation_catches_empty_fields() {
        let mut p = valid_profile();
        p.phases[0].format = Some("leef".into());
        p.phases[0].leef = Some(crate::generator::config::LeefConfig {
            vendor: "".into(),
            product: "".into(),
            version: "1".into(),
            event_id: "evt1".into(),
            attributes: None,
        });
        let errs = validate_profile(&p);
        let empty_fields: Vec<_> = errs
            .iter()
            .filter_map(|e| match e {
                ValidationError::LeefFieldEmpty { field, .. } => Some(field.clone()),
                _ => None,
            })
            .collect();
        assert!(
            empty_fields.contains(&"vendor".to_string()),
            "expected empty vendor error, got {:?}",
            empty_fields
        );
        assert!(
            empty_fields.contains(&"product".to_string()),
            "expected empty product error, got {:?}",
            empty_fields
        );
    }

    /// PR-16 (coverage): load_shape_sine_validates_period_and_rates.
    /// `validate_load_shape` body for `Sine` variant was uncovered (13 lines).
    #[test]
    fn load_shape_sine_validates_period_and_rates() {
        let mut p = valid_profile();
        p.phases[0].load_shape = Some(crate::load_shape::LoadShape::Sine {
            min_rate: -1.0,
            max_rate: 100.0,
            period_secs: 0.0,
        });
        let errs = validate_profile(&p);
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::NegativeLoadShapeRate { field, .. } if field == "min_rate")),
            "expected negative min_rate error"
        );
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::NonPositiveLoadShapePeriod { field, .. } if field == "period_secs")),
            "expected non-positive period_secs error"
        );
    }

    /// PR-16 (coverage): load_shape_constant_rejects_negative_rate.
    /// `LoadShape::Constant { rate: None }` body was covered, but `rate: Some(-N)`
    /// branch (line 933-936) was not.
    #[test]
    fn load_shape_constant_rejects_negative_rate() {
        let mut p = valid_profile();
        p.phases[0].load_shape = Some(crate::load_shape::LoadShape::Constant { rate: Some(-5.0) });
        let errs = validate_profile(&p);
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::NegativeLoadShapeRate { field, .. } if field == "rate"
        )));
    }

    /// PR-16 (coverage): load_shape_burst_rejects_non_positive_every_secs.
    /// `validate_load_shape` Burst body for `every_secs` check was uncovered.
    #[test]
    fn load_shape_burst_rejects_non_positive_every_secs() {
        let mut p = valid_profile();
        p.phases[0].load_shape = Some(crate::load_shape::LoadShape::Burst {
            base_rate: 10.0,
            burst_rate: 100.0,
            every_secs: 0.0,
            burst_secs: 1.0,
        });
        let errs = validate_profile(&p);
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::NonPositiveLoadShapePeriod { field, .. } if field == "every_secs"
        )));
    }

    /// PR-16 (coverage): tls_client_key_file_not_found_emits_error.
    /// `validate_target` TLS branch for missing client key file was uncovered.
    #[test]
    fn tls_client_key_file_not_found_emits_error() {
        let mut p = valid_profile();
        p.targets[0].transport = "tls".into();
        p.targets[0].tls_client_key_file = Some("/no/such/key.pem".into());
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::TlsClientKeyFileNotFound { .. })));
    }

    /// PR-16 (coverage): rejects_negative_template_weight.
    /// `validate_phase` body for negative `template_weights[i]` was uncovered.
    #[test]
    fn rejects_negative_template_weight() {
        let mut p = valid_profile();
        p.phases[0].templates = vec!["a".into(), "b".into()];
        p.phases[0].template_weights = Some(vec![-1.0, 2.0]);
        let errs = validate_profile(&p);
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::InvalidTemplateWeight { value, .. }
                if (*value - (-1.0_f64)).abs() < f64::EPSILON
        )));
    }

    /// PR-16 (coverage): rejects_zero_padding.
    /// `validate_phase` body for `pad_to_bytes = Some(0)` was uncovered.
    #[test]
    fn rejects_zero_padding() {
        let mut p = valid_profile();
        p.phases[0].pad_to_bytes = Some(0);
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::ZeroPadding { .. })));
    }

    /// PR-16 (coverage): rejects_bad_shutdown_mode.
    /// `validate_target` body for invalid `shutdown.mode` was uncovered.
    #[test]
    fn rejects_bad_shutdown_mode() {
        let mut p = valid_profile();
        p.shutdown.mode = "kill".into();
        let errs = validate_profile(&p);
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::InvalidShutdownMode { value, .. } if value == "kill"
        )));
    }

    /// PR-16 (coverage): rejects_empty_phase_name (whitespace-only).
    /// `validate_phase` body for `name.trim().is_empty()` was uncovered.
    #[test]
    fn rejects_empty_phase_name() {
        let mut p = valid_profile();
        p.phases[0].name = "   ".into();
        let errs = validate_profile(&p);
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyPhaseName { .. })));
    }

    /// PR-16 (coverage): reconnect_multiplier_zero_rejected.
    /// `validate_target` body for invalid `reconnect_multiplier` was uncovered
    /// at the unit-test level (integration test covers it).
    #[test]
    fn reconnect_multiplier_zero_rejected() {
        let mut p = valid_profile();
        p.targets[0].reconnect_multiplier = Some(0.5);
        let errs = validate_profile(&p);
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::InvalidReconnectMultiplier { value, .. }
                if (*value - 0.5_f64).abs() < f64::EPSILON
        )));
    }
}
