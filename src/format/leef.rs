//! F15 (v9.2.0): IBM QRadar Log Event Extended Format (LEEF).
//!
//! Спецификация LEEF v2.0:
//! <https://www.ibm.com/docs/en/dsm?topic=overview-leef-event-components>
//!
//! Формат:
//! ```text
//! LEEF:2.0|Vendor|Product|Version|EventID<TAB>key=value<TAB>key=value\n
//! ```
//!
//! - Header-поля (Vendor, Product, Version, EventID) разделены `|`, после
//!   EventID идёт символ TAB (`\t`), затем атрибуты.
//! - Атрибуты разделены TAB.
//! - Трейлинг `\n` обязателен для LEEF-события.
//!
//! Экранирование (LEEF 2.0):
//! - В header-полях: `\` → `\\`, `|` → `\|`.
//! - В значениях атрибутов: `\` → `\\`, `=` → `\=`, TAB → `\t` (escape
//!   литералом `\t`), LF → `\n` (escape литералом `\n`).
//! - Ключи атрибутов: НЕ экранируются.
//!
//! Горячий путь: O(N_attrs) экранирований; обычно 3-15 атрибутов. Сборка
//! в `String` с предвычисленной ёмкостью (одна аллокация, без realloc'ов).

use crate::generator::config::LeefConfig;

/// Собрать LEEF v2.0-сообщение: `LEEF:2.0|Vendor|Product|Version|EventID<TAB>attrs\n`.
/// `msg` добавляется как атрибут `msg=<body>` (если body непустой) с экранированием.
pub fn build(cfg: &LeefConfig, msg: &[u8]) -> Vec<u8> {
    let header = format!(
        "LEEF:2.0|{}|{}|{}|{}",
        escape_header(&cfg.vendor),
        escape_header(&cfg.product),
        escape_header(&cfg.version),
        escape_header(&cfg.event_id),
    );

    // Сборка атрибутов: key=value, TAB-разделитель.
    let mut attrs = String::new();
    if let Some(attributes) = &cfg.attributes {
        for (i, (k, v)) in attributes.iter().enumerate() {
            if i > 0 {
                attrs.push('\t');
            }
            attrs.push_str(k);
            attrs.push('=');
            attrs.push_str(&escape_attr_value(v));
        }
    }
    // `msg=<body>` всегда в конце (если есть пользовательские атрибуты — через TAB).
    if !attrs.is_empty() {
        attrs.push('\t');
    }
    attrs.push_str("msg=");
    attrs.push_str(&escape_attr_value(std::str::from_utf8(msg).unwrap_or("")));

    // Header + TAB + attrs + LF.
    let mut out = String::with_capacity(header.len() + 1 + attrs.len() + 1);
    out.push_str(&header);
    out.push('\t');
    out.push_str(&attrs);
    out.push('\n');
    out.into_bytes()
}

/// Экранирование для header-полей LEEF: `\` → `\\`, `|` → `\|`.
fn escape_header(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => {
                out.push('\\');
                out.push('\\');
            }
            '|' => {
                out.push('\\');
                out.push('|');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Экранирование для значений атрибутов LEEF 2.0:
/// `\` → `\\`, `=` → `\=`, TAB → `\t`, LF → `\n` (escape-литералы).
fn escape_attr_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => {
                out.push('\\');
                out.push('\\');
            }
            '=' => {
                out.push('\\');
                out.push('=');
            }
            '\t' => {
                out.push('\\');
                out.push('t');
            }
            '\n' => {
                out.push('\\');
                out.push('n');
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LeefConfig {
        LeefConfig {
            vendor: "Acme".into(),
            product: "SyslogGen".into(),
            version: "9.2".into(),
            event_id: "evt001".into(),
            attributes: None,
        }
    }

    #[test]
    fn builds_minimal_leef() {
        let out = build(&cfg(), b"hello");
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(s, "LEEF:2.0|Acme|SyslogGen|9.2|evt001\tmsg=hello\n");
    }

    #[test]
    fn escapes_pipe_in_header_fields() {
        let mut c = cfg();
        c.vendor = "Acme|Inc".into();
        c.event_id = "evt|001".into();
        let out = build(&c, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("Acme\\|Inc"), "got: {s}");
        assert!(s.contains("evt\\|001"), "got: {s}");
    }

    #[test]
    fn escapes_backslash_in_header_fields() {
        let mut c = cfg();
        c.product = "P\\rod".into();
        let out = build(&c, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("P\\\\rod"), "got: {s}");
    }

    #[test]
    fn escapes_specials_in_attribute_values() {
        let mut c = cfg();
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("src".into(), "10.0.0.1".into());
        attrs.insert("user".into(), "alice=bob".into());
        attrs.insert("path".into(), "C:\\Windows".into());
        c.attributes = Some(attrs);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        // Значения экранируются, ключи — нет.
        assert!(s.contains("user=alice\\=bob"), "got: {s}");
        assert!(s.contains("path=C:\\\\Windows"), "got: {s}");
        // src — без спецсимволов.
        assert!(s.contains("src=10.0.0.1"), "got: {s}");
        // Атрибуты разделены TAB.
        assert!(s.matches('\t').count() >= 3, "got: {s}");
    }

    #[test]
    fn escapes_tab_and_newline_in_attribute_values() {
        let mut c = cfg();
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("msg2".into(), "line1\nline2\tcol2".into());
        c.attributes = Some(attrs);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        // \n → \n (escape), \t → \t (escape)
        assert!(s.contains("msg2=line1\\nline2\\tcol2"), "got: {s}");
    }

    #[test]
    fn message_trailing_newline_is_mandatory() {
        let out = build(&cfg(), b"x");
        assert!(out.ends_with(b"\n"));
    }

    #[test]
    fn attributes_sorted_for_determinism() {
        let mut c = cfg();
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("z".into(), "1".into());
        attrs.insert("a".into(), "2".into());
        attrs.insert("m".into(), "3".into());
        c.attributes = Some(attrs);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        let a_pos = s.find("a=2").unwrap();
        let m_pos = s.find("m=3").unwrap();
        let z_pos = s.find("z=1").unwrap();
        assert!(a_pos < m_pos && m_pos < z_pos, "got: {s}");
    }
}
