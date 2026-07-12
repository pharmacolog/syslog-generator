//! Сборка синтаксически валидных syslog-сообщений по RFC 5424 и RFC 3164.
//!
//! Источники:
//! - RFC 5424 (The Syslog Protocol): формат HEADER = PRI VERSION SP TIMESTAMP SP
//!   HOSTNAME SP APP-NAME SP PROCID SP MSGID; PRIVAL = facility*8 + severity;
//!   TIMESTAMP — RFC3339 (T и Z в верхнем регистре, TIME-SECFRAC 1..6 цифр);
//!   STRUCTURED-DATA = NILVALUE / 1*SD-ELEMENT; в PARAM-VALUE экранируются `"` `\` `]`;
//!   MSG может начинаться с UTF-8 BOM (%xEF.BB.BF).
//! - RFC 3164 (BSD syslog): `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`, день с ведущим
//!   пробелом для 1..9, максимум 1024 октета.

use chrono::{Local, Utc};

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

const NILVALUE: &str = "-";
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

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
fn sanitize_header(value: &str, max: usize) -> String {
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
fn rfc5424_timestamp() -> String {
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

/// Собрать полное сообщение RFC 5424:
/// `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID SD [BOM]MSG`
pub fn build_rfc5424(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = prival(h.facility, h.severity);
    let hostname = sanitize_header(&h.hostname, 255);
    let app_name = sanitize_header(&h.app_name, 48);
    let procid = sanitize_header(&h.procid, 128);
    let msgid = sanitize_header(&h.msgid, 32);
    let sd = if h.structured_data.is_empty() {
        NILVALUE.to_string()
    } else {
        h.structured_data.clone()
    };

    let header = format!(
        "<{}>1 {} {} {} {} {} {}",
        pri,
        rfc5424_timestamp(),
        hostname,
        app_name,
        procid,
        msgid,
        sd,
    );

    let mut out = Vec::with_capacity(header.len() + msg.len() + 4);
    out.extend_from_slice(header.as_bytes());
    out.push(b' ');
    if h.bom {
        out.extend_from_slice(BOM);
    }
    out.extend_from_slice(msg);
    out
}

/// Собрать сообщение RFC 3164 (BSD):
/// `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`
/// TIMESTAMP — локальное время; день с ведущим пробелом для 1..9.
/// TAG формируется из app_name (+ [procid], если не NILVALUE).
pub fn build_rfc3164(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = prival(h.facility, h.severity);
    let ts = Local::now().format("%b %e %H:%M:%S").to_string();
    let hostname = {
        // RFC3164 HOSTNAME без пробелов; NILVALUE неуместен — берём "localhost".
        let s = sanitize_header(&h.hostname, 255);
        if s == NILVALUE {
            "localhost".to_string()
        } else {
            s
        }
    };
    let app = sanitize_header(&h.app_name, 32);
    let app = if app == NILVALUE {
        "app".to_string()
    } else {
        app
    };
    let tag = if h.procid.is_empty() || h.procid == NILVALUE {
        format!("{}:", app)
    } else {
        let pid = sanitize_header(&h.procid, 128);
        format!("{}[{}]:", app, pid)
    };

    let header = format!("<{}>{} {} {}", pri, ts, hostname, tag);
    let mut out = Vec::with_capacity(header.len() + msg.len() + 2);
    out.extend_from_slice(header.as_bytes());
    out.push(b' ');
    out.extend_from_slice(msg);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prival_examples() {
        // RFC 5424 §6.2.1: facility=0 severity=0 → 0; facility=20 severity=5 → 165.
        assert_eq!(prival(0, 0), 0);
        assert_eq!(prival(20, 5), 165);
        assert_eq!(prival(1, 6), 14); // user.info
                                      // Зажим диапазонов.
        assert_eq!(prival(255, 255), 23 * 8 + 7);
    }

    #[test]
    fn test_escape_sd_value() {
        assert_eq!(escape_sd_value(r#"a"b\c]d"#), r#"a\"b\\c\]d"#);
        assert_eq!(escape_sd_value("plain"), "plain");
    }

    #[test]
    fn test_rfc5424_structure() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "host1".into(),
            app_name: "app1".into(),
            procid: "123".into(),
            msgid: "ID47".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc5424(&h, b"hello")).unwrap();
        assert_eq!(out, "<14>1 ".to_string() + &out[6..]);
        assert!(out.starts_with("<14>1 "));
        assert!(out.ends_with(" host1 app1 123 ID47 - hello"));
    }

    #[test]
    fn test_rfc5424_bom() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "h".into(),
            app_name: "a".into(),
            procid: "-".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: true,
        };
        let out = build_rfc5424(&h, b"x");
        // BOM должен стоять непосредственно перед MSG.
        assert!(out.ends_with(&[0xEF, 0xBB, 0xBF, b'x']));
    }

    #[test]
    fn test_rfc3164_structure() {
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "srv".into(),
            app_name: "sshd".into(),
            procid: "42".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc3164(&h, b"login")).unwrap();
        assert!(out.starts_with("<14>"));
        assert!(out.contains(" srv sshd[42]: login"));
    }

    #[test]
    fn test_header_sanitization() {
        // Пробелы заменяются на '_', пустое поле → NILVALUE, обрезка по длине.
        let h = Header {
            facility: 1,
            severity: 6,
            hostname: "has space".into(),
            app_name: "".into(),
            procid: "-".into(),
            msgid: "-".into(),
            structured_data: "-".into(),
            bom: false,
        };
        let out = String::from_utf8(build_rfc5424(&h, b"m")).unwrap();
        assert!(out.contains(" has_space "));
        // app_name пустой → NILVALUE.
        assert!(out.contains(" has_space - - - "));
    }
}
