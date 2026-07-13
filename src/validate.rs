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

use crate::config::{Phase, Profile, SyslogConfig, TargetConfig};
use crate::load_shape::LoadShape;
use thiserror::Error;

/// Допустимые значения `transport` у цели.
pub const VALID_TRANSPORTS: &[&str] = &["tcp", "udp", "tls", "file"];
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
        let v = match field {
            "device_vendor" => &c.device_vendor,
            "device_product" => &c.device_product,
            "device_version" => &c.device_version,
            "signature_id" => &c.signature_id,
            "name" => &c.name,
            _ => unreachable!(),
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
        let v = match field {
            "vendor" => &l.vendor,
            "product" => &l.product,
            "version" => &l.version,
            "event_id" => &l.event_id,
            _ => unreachable!(),
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
}
