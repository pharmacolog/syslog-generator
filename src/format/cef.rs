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
//! Замечание о горячем пути: на каждое сообщение выполняется O(N_extensions)
//! экранирований. CEF-extension обычно 5-20 полей — приемлемо. Сообщение
//! целиком собирается в `String` (одна аллокация) → `into_bytes()`.
//! `String::with_capacity` задаёт ёмкость = суммарная длина всех частей,
//! что исключает realloc'и.

use crate::generator::config::CefConfig;

/// Собрать CEF-сообщение: `CEF:0|Vendor|Product|Ver|SigID|Name|Sev|ext1=val1 ext2=val2`.
/// `msg` добавляется как extension-поле `msg=<body>` (если body непустой)
/// с экранированием CEF. Это упрощает интеграцию с ArcSight SmartConnector'ами,
/// которые ожидают полезную нагрузку в стандартном extension-поле.
pub fn build(cfg: &CefConfig, msg: &[u8]) -> Vec<u8> {
    let severity = cfg.severity.unwrap_or(0);
    let header = format!(
        "CEF:0|{}|{}|{}|{}|{}|{}",
        escape_header(&cfg.device_vendor),
        escape_header(&cfg.device_product),
        escape_header(&cfg.device_version),
        escape_header(&cfg.signature_id),
        escape_header(&cfg.name),
        severity,
    );

    // Сборка extension-блока. Если есть пользовательские extensions — добавляем.
    // Затем — `msg=<body>` с экранированием CEF.
    let mut ext = String::new();
    if let Some(extensions) = &cfg.extensions {
        for (i, (k, v)) in extensions.iter().enumerate() {
            if i > 0 {
                ext.push(' ');
            }
            ext.push_str(k);
            ext.push('=');
            ext.push_str(&escape_extension_value(v));
        }
    }
    // `msg=` всегда присутствует — SmartConnector ожидает полезную нагрузку.
    if !ext.is_empty() {
        ext.push(' ');
    }
    ext.push_str("msg=");
    ext.push_str(&escape_extension_value(
        std::str::from_utf8(msg).unwrap_or(""),
    ));

    let mut out = String::with_capacity(header.len() + ext.len() + 1);
    out.push_str(&header);
    out.push('|');
    out.push_str(&ext);
    out.into_bytes()
}

/// Экранирование для header-полей CEF: `\` → `\\`, `|` → `\|`.
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

/// Экранирование для extension-значений CEF: `\` → `\\`, `|` → `\|`, `=` → `\=`.
/// Новой строки (`\n`) НЕ экранируются — CEF-парсер ArcSight обычно принимает
/// multi-line значения в кавычках; для простоты передаём как есть (ArcSight
/// SmartConnector обрабатывает LF внутри значения).
fn escape_extension_value(s: &str) -> String {
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
            '=' => {
                out.push('\\');
                out.push('=');
            }
            _ => out.push(c),
        }
    }
    out
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
        // `|` в header экранируется → `\|`. Двойной `|` между полями не экранируется.
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
        // Значения экранируются, ключи — нет.
        assert!(s.contains("user=alice\\=bob"), "got: {s}");
        assert!(s.contains("path=C:\\\\Windows\\|\\=foo"), "got: {s}");
        // src — без спецсимволов.
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
        // Бинарный msg (не UTF-8) — body экранируется как пустая строка.
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
        // BTreeMap гарантирует порядок ключей; это важно для детерминизма
        // (тот же seed → тот же вывод).
        let mut c = cfg();
        let mut exts = std::collections::BTreeMap::new();
        exts.insert("z".into(), "1".into());
        exts.insert("a".into(), "2".into());
        exts.insert("m".into(), "3".into());
        c.extensions = Some(exts);
        let out = build(&c, b"");
        let s = std::str::from_utf8(&out).unwrap();
        // Должен быть порядок a, m, z.
        let a_pos = s.find("a=2").unwrap();
        let m_pos = s.find("m=3").unwrap();
        let z_pos = s.find("z=1").unwrap();
        assert!(a_pos < m_pos && m_pos < z_pos, "got: {s}");
    }
}
