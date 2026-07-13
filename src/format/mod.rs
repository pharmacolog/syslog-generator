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

/// Trait `Format` — будущая абстракция для динамического выбора формата
/// (планируется в вехе E, см. F15). На текущем этапе — статические функции
/// `build_*` экспортируются напрямую.
pub trait Format {
    /// Собрать полное сообщение в данном формате.
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>;
    /// Имя формата (rfc5424, rfc3164, raw, protobuf) — для логирования
    /// и метрик (`syslog_messages_by_format_total`).
    fn name(&self) -> &'static str;
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
}
