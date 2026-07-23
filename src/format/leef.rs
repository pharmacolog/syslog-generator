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
//! Issue #85 (A1 quick wins): горячий путь — пишем напрямую в `Vec<u8>`
//! с предвычисленной ёмкостью (одна аллокация), экранирование пишет байты
//! через `escape_*_into` (как `cef.rs`). Это устраняет O(N_attrs) промежуточных
//! аллокаций `String` per call.

use crate::generator::config::LeefConfig;

/// Собрать LEEF v2.0-сообщение: `LEEF:2.0|Vendor|Product|Version|EventID<TAB>attrs\n`.
/// `msg` добавляется как атрибут `msg=<body>` (если body непустой) с экранированием.
///
/// Issue #85: одна аллокация `Vec<u8>` с заранее вычисленной ёмкостью;
/// escape-функции пишут байты прямо в этот буфер.
#[inline]
pub fn build(cfg: &LeefConfig, msg: &[u8]) -> Vec<u8> {
    // Предвычисление ёмкости (худший случай: каждый символ удваивается).
    let hdr_sum: usize =
        cfg.vendor.len() + cfg.product.len() + cfg.version.len() + cfg.event_id.len();
    let (ext_pairs, ext_keys_len, ext_vals_len): (usize, usize, usize) = match &cfg.attributes {
        Some(m) => (
            m.len(),
            m.keys().map(|k| k.len()).sum(),
            m.values().map(|v| v.len()).sum(),
        ),
        None => (0, 0, 0),
    };
    // "LEEF:2.0|" = 9; 4 '|' между полей; ext_pairs TAB-разделителей;
    // ext_pairs '='; "msg=" = 4; LF trailing = 1.
    let estimated_capacity = 9
        + 2 * hdr_sum
        + 4
        + ext_pairs.saturating_sub(1)
        + ext_keys_len
        + ext_pairs
        + 2 * ext_vals_len
        + 4
        + msg.len()
        + 1;

    let mut out = Vec::with_capacity(estimated_capacity);

    // Header: "LEEF:2.0|" + 4 escaped fields + "|" + EventID.
    out.extend_from_slice(b"LEEF:2.0|");
    escape_header_into(&mut out, &cfg.vendor);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.product);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.version);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.event_id);

    // TAB после EventID (header всегда заканчивается EventID, потом TAB → attrs).
    out.push(b'\t');

    // Атрибуты: key=value, TAB-разделитель.
    if let Some(attributes) = &cfg.attributes {
        if !attributes.is_empty() {
            for (i, (k, v)) in attributes.iter().enumerate() {
                if i > 0 {
                    out.push(b'\t');
                }
                out.extend_from_slice(k.as_bytes());
                out.push(b'=');
                escape_attr_value_into(&mut out, v);
            }
            // TAB перед msg= если есть пользовательские атрибуты.
            out.push(b'\t');
        }
    }
    out.extend_from_slice(b"msg=");
    // Issue #85 sub-task 12: проверяем UTF-8 до lossy — для валидного msg
    // (типичный случай) избегаем `from_utf8_lossy().into_owned()` аллокации.
    match std::str::from_utf8(msg) {
        Ok(s) => escape_attr_value_into(&mut out, s),
        Err(_) => escape_attr_value_into(&mut out, ""),
    }

    // Трейлинг LF обязателен для LEEF-события.
    out.push(b'\n');

    out
}

/// Экранирование для header-полей LEEF: `\` → `\\`, `|` → `\|`.
/// Пишет байты прямо в `out` — без промежуточных `String`.
#[inline]
fn escape_header_into(out: &mut Vec<u8>, s: &str) {
    for &b in s.as_bytes() {
        match b {
            b'\\' => {
                out.push(b'\\');
                out.push(b'\\');
            }
            b'|' => {
                out.push(b'\\');
                out.push(b'|');
            }
            _ => out.push(b),
        }
    }
}

/// Экранирование для значений атрибутов LEEF 2.0:
/// `\` → `\\`, `=` → `\=`, TAB → `\t`, LF → `\n` (escape-литералы).
/// Пишет байты прямо в `out` — без промежуточных `String`.
#[inline]
fn escape_attr_value_into(out: &mut Vec<u8>, s: &str) {
    for &b in s.as_bytes() {
        match b {
            b'\\' => {
                out.push(b'\\');
                out.push(b'\\');
            }
            b'=' => {
                out.push(b'\\');
                out.push(b'=');
            }
            b'\t' => {
                out.push(b'\\');
                out.push(b't');
            }
            b'\n' => {
                out.push(b'\\');
                out.push(b'n');
            }
            _ => out.push(b),
        }
    }
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

    /// Issue #85 (A1 sub-task 12): невалидный UTF-8 в msg → пустая строка
    /// (не паника, не мусорные байты).
    #[test]
    fn handles_invalid_utf8_msg_as_empty() {
        let out = build(&cfg(), &[0xFF, 0xFE, 0xFD]);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.ends_with("\tmsg=\n"), "got: {s}");
    }
}
