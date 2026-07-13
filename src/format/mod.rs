//! N10 (v8.8.0): слой форматов (RFC 5424, RFC 3164, raw, protobuf).
//!
//! Это абстракция над типами syslog-сообщений, которые мы умеем генерировать.
//! До N10 вся логика была в одном `src/syslog.rs` (RFC 5424/3164) и
//! `src/protobuf.rs` (wire-format). После рефакторинга:
//!
//! - `mod.rs` — общие утилиты: `Header`, `prival`, `escape_sd_value`,
//!   `BOM`, `NILVALUE`, `sanitize_header` (private).
//! - `rfc5424` — `build_rfc5424(&Header, &[u8]) -> Vec<u8>` (RFC 5424 §6.4).
//! - `rfc3164` — `build_rfc3164(&Header, &[u8]) -> Vec<u8>` (RFC 3164).
//! - `raw` — passthrough: `build_raw(&Header, &[u8]) -> Vec<u8>` (копия).
//! - `protobuf` — `apply_protobuf_schema`, `serialize_protobuf`,
//!   `serialize_protobuf_like` (wire-format varint + length-delimited).
//!
//! Старые пути `syslog_generator::build_rfc5424` и
//! `syslog_generator::serialize_protobuf` сохранены как backward-compat
//! re-exports в `src/syslog.rs` и `src/protobuf.rs`.

use chrono::Utc;

/// Параметры заголовка, уже с подставленными значениями шаблона.
pub struct Header {
    pub facility: u8,
    pub severity: u8,
    pub hostname: String,
    pub app_name: String,
    pub procid: String,
    pub msgid: String,
    pub structured_data: String,
    pub bom: bool,
}

pub(crate) const NILVALUE: &str = "-";
pub(crate) const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// PRIVAL = facility*8 + severity (RFC 5424 §6.2.1).
/// facility зажимается в 0..=23, severity в 0..=7.
pub fn prival(facility: u8, severity: u8) -> u16 {
    let f = facility.min(23) as u16;
    let s = severity.min(7) as u16;
    f * 8 + s
}

/// Санитизация printable-US-ASCII поля заголовка: пустое → NILVALUE, пробелы и
/// непечатаемые символы заменяются на '_', длина обрезается до `max` октетов.
/// Пустое значение или явный "-" даёт NILVALUE.
pub(crate) fn sanitize_header(value: &str, max: usize) -> String {
    if value.is_empty() || value == NILVALUE {
        return NILVALUE.to_string();
    }
    let cleaned: String = value
        .chars()
        .map(|c| if ('!'..='~').contains(&c) { c } else { '_' })
        .take(max)
        .collect();
    if cleaned.is_empty() {
        NILVALUE.to_string()
    } else {
        cleaned
    }
}

/// TIMESTAMP по RFC 5424: RFC3339, UTC, миллисекунды, суффикс Z.
/// Пример: 2026-07-11T14:30:00.123Z
pub(crate) fn rfc5424_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Экранирование PARAM-VALUE для STRUCTURED-DATA (RFC 5424 §6.3.3):
/// символы `"`, `\`, `]` экранируются обратным слэшем.
pub fn escape_sd_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' | '\\' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

// Подмодули реализации конкретных форматов. Каждый предоставляет функцию
// `build_<format>(&Header, &[u8]) -> Vec<u8>`.
pub mod protobuf;
pub mod raw;
pub mod rfc3164;
pub mod rfc5424;

/// Trait `Format` (N10, v9.1.0) — абстракция для динамического выбора
/// формата. Реализуется в [`FormatKind`] для dyn-dispatch через enum
/// (вместо `Box<dyn Format>` — экономия heap-аллокаций на горячем пути).
/// Используется в `run_phase_multi` через `FormatKind::from(phase)`.
pub trait Format {
    /// Собрать полное сообщение в данном формате.
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>;
    /// Имя формата (rfc5424, rfc3164, raw, protobuf, ...) — для логирования
    /// и метрик (`syslog_messages_by_format_total`).
    fn name(&self) -> &'static str;
}

/// Конкретный выбор формата для фазы (N10, v9.1.0).
///
/// Используется в `run_phase_multi` для dyn-dispatch — `match self { ... }`
/// в `render` компилируется в branchless jumps. Стоимость: 0 heap-аллокаций
/// на сообщение, 0 vtable lookups (в отличие от `Box<dyn Format>`).
///
/// v9.2.0+: добавим `Cef`, `Leef`, `JsonLines` варианты (F15).
pub enum FormatKind {
    Rfc5424,
    Rfc3164,
    Raw,
    /// Protobuf с опциональной схемой. `None` — пустое сообщение
    /// (валидный пустой protobuf-блоб).
    Protobuf(Option<crate::generator::config::ProtobufSchemaFieldMap>),
}

impl Format for FormatKind {
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8> {
        match self {
            Self::Rfc5424 => rfc5424::build(h, msg),
            Self::Rfc3164 => rfc3164::build(h, msg),
            Self::Raw => msg.to_vec(),
            Self::Protobuf(map) => {
                // Преобразуем текущий `Header` в `HashMap<String, String>` для
                // совместимости с существующим API `serialize_protobuf`.
                // TODO v9.5.0: явное поле protobuf_schema в Header.
                let values: std::collections::HashMap<String, String> = [
                    ("hostname", h.hostname.clone()),
                    ("app_name", h.app_name.clone()),
                    ("procid", h.procid.clone()),
                    ("msgid", h.msgid.clone()),
                ]
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
                protobuf::serialize_protobuf(map.as_ref(), &values)
            }
        }
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Rfc5424 => "rfc5424",
            Self::Rfc3164 => "rfc3164",
            Self::Raw => "raw",
            Self::Protobuf(_) => "protobuf",
        }
    }
}

impl FormatKind {
    /// Конвертация из строки имени формата (phase.format) в [`FormatKind`].
    /// Возвращает `None` для неизвестных имён (F13 валидация должна была их отклонить).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "rfc5424" => Some(Self::Rfc5424),
            "rfc3164" => Some(Self::Rfc3164),
            "raw" => Some(Self::Raw),
            "protobuf" => Some(Self::Protobuf(None)),
            _ => None,
        }
    }
}

pub mod local_time {
    //! Local time helper for RFC 3164 timestamps.
    pub use chrono::Local;
}

/// Local helper — алиас для тестов и обратной совместимости.
pub fn build_rfc5424(h: &Header, msg: &[u8]) -> Vec<u8> {
    rfc5424::build(h, msg)
}

pub fn build_rfc3164(h: &Header, msg: &[u8]) -> Vec<u8> {
    rfc3164::build(h, msg)
}

pub fn build_raw(_h: &Header, msg: &[u8]) -> Vec<u8> {
    // Raw: сообщение передаётся как есть, без обёртки. Удобно для
    // интеграций где syslog-фрейм уже есть (например, передача через
    // syslog-фронтенд).
    msg.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-export основного приватного helper-а для тестов. Реальные тесты
    /// остались в `src/syslog.rs::tests` для backward-compat.
    #[test]
    fn prival_formula_correct() {
        // 0*8+0 = 0; 23*8+7 = 191; 16*8+6 = 134.
        assert_eq!(prival(0, 0), 0);
        assert_eq!(prival(23, 7), 191);
        assert_eq!(prival(16, 6), 134);
        // Clamping при out-of-range значениях (F13 валидация не пропустит
        // эти значения в проде, но защита в prival остаётся).
        assert_eq!(prival(100, 100), (23u16 * 8) + 7);
    }

    // ===== N10 (v9.1.0): тесты trait Format =====

    /// N10: `FormatKind::Rfc5424` рендерит валидный RFC 5424.
    #[test]
    fn n10_formatkind_rfc5424_renders_valid() {
        let h = Header {
            facility: 1,
            severity: 4,
            hostname: "host".into(),
            app_name: "app".into(),
            procid: "1".into(),
            msgid: "X".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = FormatKind::Rfc5424.render(&h, b"hello");
        assert!(out.starts_with(b"<12>1 "));
        assert!(out.ends_with(b" hello"));
        assert_eq!(out[out.len() - 1], b'o');
    }

    /// N10: `FormatKind::Raw` = passthrough (копия msg).
    #[test]
    fn n10_formatkind_raw_is_passthrough() {
        let h = Header {
            facility: 0, severity: 0, hostname: "h".into(), app_name: "a".into(),
            procid: "".into(), msgid: "".into(), structured_data: "-".into(), bom: false,
        };
        let out = FormatKind::Raw.render(&h, b"hello world");
        assert_eq!(out, b"hello world");
    }

    /// N10: имя формата через `name()`.
    #[test]
    fn n10_formatkind_name() {
        assert_eq!(FormatKind::Rfc5424.name(), "rfc5424");
        assert_eq!(FormatKind::Rfc3164.name(), "rfc3164");
        assert_eq!(FormatKind::Raw.name(), "raw");
        assert_eq!(FormatKind::Protobuf(None).name(), "protobuf");
    }

    /// N10: `parse("rfc5424")` → `Some(Rfc5424)`, `parse("unknown")` → `None`.
    #[test]
    fn n10_formatkind_parse() {
        assert!(matches!(FormatKind::parse("rfc5424"), Some(FormatKind::Rfc5424)));
        assert!(matches!(FormatKind::parse("rfc3164"), Some(FormatKind::Rfc3164)));
        assert!(matches!(FormatKind::parse("raw"), Some(FormatKind::Raw)));
        assert!(matches!(FormatKind::parse("protobuf"), Some(FormatKind::Protobuf(_))));
        assert!(FormatKind::parse("unknown").is_none());
    }
}
