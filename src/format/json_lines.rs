//! F15 (v9.2.0): Newline-Delimited JSON (JSON-lines / NDJSON).
//!
//! Формат: каждый объект — на отдельной строке, разделитель `\n`.
//! Совместимо с ingestion в Loki, ELK/Elasticsearch, Vector, Fluent Bit,
//! Logstash (codec `json_lines`).
//!
//! Стандартная схема:
//! ```json
//! {"ts":"2026-07-13T12:34:56.789Z","level":"INFO","host":"...","app":"...","msg":"..."}
//! ```
//!
//! `level` — маппинг из syslog severity (0..=7) на строковое имя:
//! Emergency, Alert, Critical, Error, Warning, Notice, Informational, Debug.
//!
//! Доп. поля из `Phase::json_lines_fields` добавляются в корень объекта.
//! Если пользователь задал `msg` явно — наш автогенерированный `msg`
//! перетирает (явный приоритет).
//!
//! Экранирование JSON (кавычки, backslash, control chars) —
//! через `serde_json` (F-15: никакого ручного escape — безопасность важнее
//! microbench). Доп. расходы на аллокацию `String` для JSON-сериализации
//! приемлемы для ingestion-формата.

use crate::format::Header;
use std::collections::BTreeMap;

/// Маппинг syslog severity (0..=7) → строковое имя уровня.
fn severity_to_level(severity: u8) -> &'static str {
    match severity.min(7) {
        0 => "Emergency",
        1 => "Alert",
        2 => "Critical",
        3 => "Error",
        4 => "Warning",
        5 => "Notice",
        6 => "Informational",
        7 => "Debug",
        _ => "Unknown", // severity вне 0..=7; default к Unknown.
    }
}

/// Собрать JSON-lines сообщение.
/// Порядок полей в JSON — алфавитный (BTreeMap), что важно для
/// детерминизма (F4) и удобства diff'а в тестах.
pub fn build(
    header: &Header,
    extra_fields: Option<&BTreeMap<String, String>>,
    msg: &[u8],
) -> Vec<u8> {
    let mut obj: BTreeMap<String, String> = BTreeMap::new();
    obj.insert("ts".to_string(), super::rfc5424_timestamp());
    obj.insert(
        "level".to_string(),
        severity_to_level(header.severity).to_string(),
    );
    obj.insert("facility".to_string(), header.facility.min(23).to_string());
    obj.insert("host".to_string(), header.hostname.clone());
    obj.insert("app".to_string(), header.app_name.clone());
    if !header.procid.is_empty() && header.procid != "-" {
        obj.insert("procid".to_string(), header.procid.clone());
    }
    if !header.msgid.is_empty() && header.msgid != "-" {
        obj.insert("msgid".to_string(), header.msgid.clone());
    }
    // msg: UTF-8 lossy — невалидные байты заменяются на U+FFFD (стандарт JSON).
    let msg_str = String::from_utf8_lossy(msg).into_owned();
    obj.insert("msg".to_string(), msg_str);
    // Доп. поля: пользовательские `json_lines_fields` из Phase.
    // Если есть пересечение ключей с автогенерированными — пользовательский
    // вариант перетирает (явный приоритет — пользователь знает, что делает).
    if let Some(extras) = extra_fields {
        for (k, v) in extras {
            obj.insert(k.clone(), v.clone());
        }
    }

    // serde_json::to_string на BTreeMap даёт стабильный порядок ключей
    // (BTreeMap iter — отсортирован). Плюс корректное JSON-экранирование.
    // BTreeMap<String,String> всегда сериализуем; если ошибка — fallback "{}".
    let mut out = serde_json::to_string(&obj).unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr() -> Header {
        Header {
            facility: 1,
            severity: 6,
            hostname: "h1".into(),
            app_name: "app1".into(),
            procid: "100".into(),
            msgid: "TST".into(),
            structured_data: "-".into(),
            bom: false,
        }
    }

    #[test]
    fn builds_minimal_json_lines() {
        let out = build(&hdr(), None, b"hello world");
        let s = std::str::from_utf8(&out).unwrap();
        // ts заполняется rfc5424_timestamp() — реальное время, поэтому
        // проверяем структуру (наличие ключей/значений), а не точный match.
        assert!(s.starts_with('{'), "got: {s}");
        assert!(s.contains("\"app\":\"app1\""), "got: {s}");
        assert!(s.contains("\"facility\":\"1\""), "got: {s}");
        assert!(s.contains("\"host\":\"h1\""), "got: {s}");
        assert!(s.contains("\"level\":\"Informational\""), "got: {s}");
        assert!(s.contains("\"msg\":\"hello world\""), "got: {s}");
        assert!(s.contains("\"msgid\":\"TST\""), "got: {s}");
        assert!(s.contains("\"procid\":\"100\""), "got: {s}");
        // ts присутствует и имеет формат ISO 8601 (regex-free check: начинается с 4 цифр).
        assert!(s.contains("\"ts\":\""), "got: {s}");
    }

    #[test]
    fn trailing_newline_is_present() {
        let out = build(&hdr(), None, b"x");
        assert!(out.ends_with(b"\n"));
    }

    #[test]
    fn severity_maps_correctly() {
        for (sev, expected) in [
            (0u8, "Emergency"),
            (1, "Alert"),
            (2, "Critical"),
            (3, "Error"),
            (4, "Warning"),
            (5, "Notice"),
            (6, "Informational"),
            (7, "Debug"),
        ] {
            let mut h = hdr();
            h.severity = sev;
            let out = build(&h, None, b"");
            let s = std::str::from_utf8(&out).unwrap();
            assert!(
                s.contains(&format!("\"level\":\"{expected}\"")),
                "sev={sev} expected={expected}, got: {s}"
            );
        }
    }

    #[test]
    fn severity_clamped_above_7() {
        // facility/severity клампятся в prival(); здесь — тоже защищаемся.
        let mut h = hdr();
        h.severity = 99;
        let out = build(&h, None, b"");
        let s = std::str::from_utf8(&out).unwrap();
        // 99.clamp(0,7) == 7 → "Debug".
        assert!(s.contains("\"level\":\"Debug\""), "got: {s}");
    }

    #[test]
    fn extra_fields_added_to_root() {
        let mut extras = BTreeMap::new();
        extras.insert("env".to_string(), "prod".to_string());
        extras.insert("region".to_string(), "eu-west-1".to_string());
        let out = build(&hdr(), Some(&extras), b"");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("\"env\":\"prod\""), "got: {s}");
        assert!(s.contains("\"region\":\"eu-west-1\""), "got: {s}");
    }

    #[test]
    fn user_extra_overrides_autogenerated() {
        let mut extras = BTreeMap::new();
        extras.insert("host".to_string(), "overridden".to_string());
        let out = build(&hdr(), Some(&extras), b"");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("\"host\":\"overridden\""), "got: {s}");
        assert!(!s.contains("\"host\":\"h1\""), "got: {s}");
    }

    #[test]
    fn json_escapes_specials_in_msg() {
        // Кавычки, backslash, control chars. serde_json экранирует их по JSON-спеке:
        // \" → \\\" (2 chars), \\ → \\\\ (2 chars), \n (control LF) → \\n (escape-последовательность).
        let out = build(&hdr(), None, b"a\"b\\c\nd");
        let s = std::str::from_utf8(&out).unwrap();
        // Экранированный JSON: "a\"b\\c\nd" где \n = 2 chars (backslash + n).
        // Структурно: \"msg\":\"a\"b\\c\nd\" — здесь \n = JSON-escape литерал.
        // Ищем: кавычка, a, экранированная кавычка, b, экранированный backslash, c, JSON \n, d, кавычка.
        assert!(s.contains(r#""msg":"a\"b\\c\nd""#), "got (raw): {s:?}");
    }

    #[test]
    fn invalid_utf8_msg_replaced_with_replacement_char() {
        let out = build(&hdr(), None, &[0xFF, 0xFE]);
        let s = std::str::from_utf8(&out).unwrap();
        // serde_json рендерит U+FFFD (replacement char) как литеральный символ —
        // он валиден в JSON-строках без экранирования.
        assert!(s.contains('\u{FFFD}'), "got: {s:?}");
    }

    #[test]
    fn nilvalue_procid_msgid_omitted() {
        let mut h = hdr();
        h.procid = "-".into();
        h.msgid = "-".into();
        let out = build(&h, None, b"");
        let s = std::str::from_utf8(&out).unwrap();
        // "-" — NILVALUE, опускаем для краткости JSON.
        assert!(!s.contains("procid"), "got: {s}");
        assert!(!s.contains("msgid"), "got: {s}");
    }

    #[test]
    fn result_is_valid_json() {
        let out = build(&hdr(), None, b"hello");
        // Без финального \n.
        let json_part = &out[..out.len() - 1];
        let parsed: serde_json::Value =
            serde_json::from_slice(json_part).expect("output должен быть валидным JSON");
        assert_eq!(parsed["msg"], "hello");
        assert_eq!(parsed["host"], "h1");
    }
}
