//! F15 (v9.2.0): ArcSight Common Event Format (CEF).
//!
//! Спецификация: <https://community.microfocus.com/t5/ArcSight-Connectors/Common-Event-Format-CEF/ta-p/1585555>
//!
//! Формат:
//! ```text
//! CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension
//! ```
//!
//! - Version — обычно 0
//! - Severity — 0..=10 (CEF, не syslog 0..=7)
//! - Extension — последовательность `key=value` пар, разделённых пробелами.
//!
//! Экранирование (ArcSight CEF Implementation Guide):
//! - В header-полях (Vendor, Product, Version, Signature ID, Name): экранируются
//!   `\` → `\\` и `|` → `\|`.
//! - В extension-значениях: экранируются `\` → `\\`, `|` → `\|`, `=` → `\=`.
//! - В extension-ключах: НЕ экранируются (ключ — фиксированное ASCII-имя).
//!
//! Замечание о горячем пути (PR-17a): сборка ведётся напрямую в `Vec<u8>`
//! с предвычисленной ёмкостью (худший случай — каждый символ удваивается при
//! escape). Никаких промежуточных `String` — экранирование пишет байты
//! прямо в общий буфер через `escape_*_into`. Это устраняет O(N_extensions)
//! промежуточных аллокаций `String` per call.

use crate::generator::config::CefConfig;

/// Собрать CEF-сообщение: `CEF:0|Vendor|Product|Ver|SigID|Name|Sev|ext1=val1 ext2=val2`.
/// `msg` добавляется как extension-поле `msg=<body>` (если body непустой)
/// с экранированием CEF. Это упрощает интеграцию с ArcSight SmartConnector'ами,
/// которые ожидают полезную нагрузку в стандартном extension-поле.
///
/// Hot-path оптимизация (PR-17a): одна аллокация `Vec<u8>` с заранее
/// вычисленной ёмкостью; escape-функции пишут байты прямо в этот буфер.
#[inline]
pub fn build(cfg: &CefConfig, msg: &[u8]) -> Vec<u8> {
    let severity = cfg.severity.unwrap_or(0);

    // Предвычисление ёмкости (худший случай: каждый символ удваивается).
    let hdr_sum: usize = cfg.device_vendor.len()
        + cfg.device_product.len()
        + cfg.device_version.len()
        + cfg.signature_id.len()
        + cfg.name.len();
    let (ext_pairs, ext_keys_len, ext_vals_len): (usize, usize, usize) = match &cfg.extensions {
        Some(m) => (
            m.len(),
            m.keys().map(|k| k.len()).sum(),
            m.values().map(|v| v.len()).sum(),
        ),
        None => (0, 0, 0),
    };
    let msg_str = std::str::from_utf8(msg).unwrap_or("");
    // "CEF:0|"=7; 7 '|' (5 между полей + 1 после name + 1 после severity);
    // 2 цифры sev; ext_pairs пробелов; ext_pairs '='; " msg=" = 5.
    let estimated_capacity = 7
        + 2 * hdr_sum
        + 7
        + 2
        + ext_pairs
        + ext_keys_len
        + ext_pairs
        + 2 * ext_vals_len
        + 5
        + 2 * msg_str.len();

    let mut out = Vec::with_capacity(estimated_capacity);

    // Header: "CEF:0|" + 5 escaped fields + "|" + severity + "|".
    out.extend_from_slice(b"CEF:0|");
    escape_header_into(&mut out, &cfg.device_vendor);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.device_product);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.device_version);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.signature_id);
    out.push(b'|');
    escape_header_into(&mut out, &cfg.name);
    out.push(b'|');
    push_u8_decimal(&mut out, severity);
    out.push(b'|');

    // Extension pairs (BTreeMap — отсортированы по ключу для детерминизма).
    if let Some(extensions) = &cfg.extensions {
        for (i, (k, v)) in extensions.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(k.as_bytes());
            out.push(b'=');
            escape_extension_value_into(&mut out, v);
        }
    }
    // `msg=` всегда присутствует — SmartConnector ожидает полезную нагрузку.
    if extensions_has_pairs(cfg) {
        out.push(b' ');
    }
    out.extend_from_slice(b"msg=");
    escape_extension_value_into(&mut out, msg_str);

    out
}

/// `true`, если extension-блок непустой (нужен разделитель-пробел перед `msg=`).
#[inline]
fn extensions_has_pairs(cfg: &CefConfig) -> bool {
    matches!(&cfg.extensions, Some(m) if !m.is_empty())
}

/// Записать `u8` (0..=10) как 1-2 десятичных цифры в `Vec<u8>` без аллокаций.
#[inline]
fn push_u8_decimal(out: &mut Vec<u8>, mut n: u8) {
    if n >= 100 {
        let q = n / 100;
        out.push(b'0' + q);
        n -= q * 100;
    }
    if n >= 10 {
        let q = n / 10;
        out.push(b'0' + q);
        n -= q * 10;
    }
    out.push(b'0' + n);
}

/// Экранирование для header-полей CEF: `\` → `\\`, `|` → `\|`.
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

/// Экранирование для extension-значений CEF: `\` → `\\`, `|` → `\|`, `=` → `\=`.
/// Пишет байты прямо в `out` — без промежуточных `String`.
/// Новой строки (`\n`) НЕ экранируются — CEF-парсер ArcSight обычно принимает
/// multi-line значения в кавычках; для простоты передаём как есть (ArcSight
/// SmartConnector обрабатывает LF внутри значения).
#[inline]
fn escape_extension_value_into(out: &mut Vec<u8>, s: &str) {
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
            b'=' => {
                out.push(b'\\');
                out.push(b'=');
            }
            _ => out.push(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CefConfig {
        CefConfig {
            device_vendor: "Acme".into(),
            device_product: "SyslogGen".into(),
            device_version: "9.2".into(),
            signature_id: "100".into(),
            name: "test event".into(),
            severity: Some(5),
            extensions: None,
        }
    }

    #[test]
    fn builds_minimal_cef() {
        let out = build(&cfg(), b"hello");
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(s, "CEF:0|Acme|SyslogGen|9.2|100|test event|5|msg=hello");
    }

    #[test]
    fn escapes_pipe_in_header_fields() {
        let mut c = cfg();
        c.device_vendor = "Acme|Inc".into();
        c.name = "alert|critical".into();
        let out = build(&c, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("Acme\\|Inc"), "got: {s}");
        assert!(s.contains("alert\\|critical"), "got: {s}");
    }

    #[test]
    fn escapes_backslash_in_header_fields() {
        let mut c = cfg();
        c.device_product = "P\\rod".into();
        let out = build(&c, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("P\\\\rod"), "got: {s}");
    }

    #[test]
    fn escapes_specials_in_extension_values() {
        let mut c = cfg();
        let mut exts = std::collections::BTreeMap::new();
        exts.insert("src".into(), "10.0.0.1".into());
        exts.insert("user".into(), "alice=bob".into());
        exts.insert("path".into(), "C:\\Windows|=foo".into());
        c.extensions = Some(exts);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("user=alice\\=bob"), "got: {s}");
        assert!(s.contains("path=C:\\\\Windows\\|\\=foo"), "got: {s}");
        assert!(s.contains("src=10.0.0.1"), "got: {s}");
    }

    #[test]
    fn includes_msg_extension_even_with_empty_extensions() {
        let out = build(&cfg(), b"payload");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.ends_with("msg=payload"), "got: {s}");
    }

    #[test]
    fn handles_invalid_utf8_msg_as_empty() {
        let out = build(&cfg(), &[0xFF, 0xFE]);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.ends_with("|msg="), "got: {s}");
    }

    #[test]
    fn default_severity_is_zero_when_none() {
        let mut c = cfg();
        c.severity = None;
        let out = build(&c, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("|0|msg=x"), "got: {s}");
    }

    #[test]
    fn extensions_sorted_by_key_for_determinism() {
        let mut c = cfg();
        let mut exts = std::collections::BTreeMap::new();
        exts.insert("z".into(), "1".into());
        exts.insert("a".into(), "2".into());
        exts.insert("m".into(), "3".into());
        c.extensions = Some(exts);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        let a_pos = s.find("a=2").unwrap();
        let m_pos = s.find("m=3").unwrap();
        let z_pos = s.find("z=1").unwrap();
        assert!(a_pos < m_pos && m_pos < z_pos, "got: {s}");
    }

    /// Phase 11 (Tier 1): `push_u8_decimal` покрытие для n >= 10 ветки.
    /// В production вызывается только с severity ∈ [0, 10], но функция
    /// поддерживает 2-цифровой диапазон — тестируем ветку `n >= 10`.
    #[test]
    fn push_u8_decimal_handles_two_digit_numbers() {
        let mut out = Vec::new();
        // 1 цифра (severity 0..=9).
        push_u8_decimal(&mut out, 0);
        assert_eq!(out, b"0");
        out.clear();
        push_u8_decimal(&mut out, 5);
        assert_eq!(out, b"5");
        out.clear();
        push_u8_decimal(&mut out, 9);
        assert_eq!(out, b"9");
        // 2 цифры (n >= 10 → покрывает if n >= 10 branch).
        out.clear();
        push_u8_decimal(&mut out, 10);
        assert_eq!(out, b"10");
        out.clear();
        push_u8_decimal(&mut out, 99);
        assert_eq!(out, b"99");
    }
}
