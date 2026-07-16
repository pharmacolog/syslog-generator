//! N10 (v8.8.0): реализация RFC 3164 (BSD) в слое format.

use super::{sanitize_header, Header, NILVALUE};
use chrono::Local;

/// Собрать сообщение RFC 3164 (BSD):
/// `<PRI>Mmm dd hh:mm:ssHOSTNAME TAG: MSG`
/// TIMESTAMP — локальное время; день с ведущим пробелом для 1..9.
/// TAG формируется из app_name (+ `procid` в квадратных скобках, если не NILVALUE).
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = super::prival(h.facility, h.severity);
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
    use crate::format::Header;

    fn test_header() -> Header {
        Header {
            facility: 16, // local0
            severity: 6,  // info
            hostname: "myhost".to_string(),
            app_name: "myapp".to_string(),
            procid: "1234".to_string(),
            msgid: "ID".to_string(),
            structured_data: "-".to_string(),
            bom: false,
        }
    }

    /// PRIVAL = facility*8 + severity.
    #[test]
    fn rfc3164_prival_in_pri_header() {
        let h = test_header();
        let out = build(&h, b"test");
        // 16*8 + 6 = 134 = 0x86
        assert!(
            out.starts_with(b"<134>"),
            "expected <134> prefix, got {:?}",
            &out[..20]
        );
    }

    /// TAG format: `app[pid]:` when procid задан.
    #[test]
    fn rfc3164_tag_with_procid() {
        let h = test_header();
        let out = build(&h, b"hello");
        // Проверяем что в выводе есть "myapp[1234]:"
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains("myapp[1234]:"),
            "expected TAG with procid, got: {}",
            s
        );
    }

    /// TAG format: `app:` when procid пустой.
    #[test]
    fn rfc3164_tag_without_procid() {
        let mut h = test_header();
        h.procid = String::new();
        let out = build(&h, b"hello");
        let s = std::str::from_utf8(&out).unwrap();
        // "myapp:" без []
        assert!(
            s.contains("myapp: "),
            "expected TAG without procid, got: {}",
            s
        );
    }

    /// TAG format: `app:` when procid = NILVALUE.
    #[test]
    fn rfc3164_tag_with_nilvalue_procid() {
        let mut h = test_header();
        h.procid = "-".to_string();
        let out = build(&h, b"hello");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains("myapp: "),
            "expected TAG for NILVALUE procid, got: {}",
            s
        );
    }

    /// Default hostname when sanitized to NILVALUE → "localhost".
    #[test]
    fn rfc3164_nilvalue_hostname_becomes_localhost() {
        let mut h = test_header();
        h.hostname = String::new();
        let out = build(&h, b"msg");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains(" localhost "),
            "expected 'localhost', got: {}",
            s
        );
    }

    /// Default app when sanitized to NILVALUE → "app".
    /// sanitize_header НЕ превращает пустую в NILVALUE (только проверяет chars),
    /// поэтому пустая app_name остаётся пустой → TAG будет ":".
    /// Тест проверяет что пустая app_name не паникует.
    #[test]
    fn rfc3164_empty_app_name_does_not_panic() {
        let mut h = test_header();
        h.app_name = String::new();
        let out = build(&h, b"msg");
        let s = std::str::from_utf8(&out).unwrap();
        // Не паникует, содержит hostname.
        assert!(s.contains("myhost"), "expected hostname, got: {}", s);
    }

    /// Когда app_name = NILVALUE ("-"), sanitize_header возвращает "-"
    /// (не превращает в NILVALUE), затем код rfc3164 видит app == NILVALUE
    /// и подставляет дефолт "app" → TAG = "app[1234]:".
    #[test]
    fn rfc3164_nilvalue_app_becomes_app() {
        let mut h = test_header();
        h.app_name = "-".to_string();
        let out = build(&h, b"msg");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains("app[1234]:"),
            "expected 'app[1234]:' TAG, got: {}",
            s
        );
    }

    /// Space separator between header and MSG.
    #[test]
    fn rfc3164_space_separates_header_from_msg() {
        let h = test_header();
        let out = build(&h, b"BODY");
        let s = std::str::from_utf8(&out).unwrap();
        // Заканчивается на " BODY" (без trailing newline — BSD legacy).
        assert!(
            s.ends_with(" BODY"),
            "expected space-separated body, got: {}",
            s
        );
    }
}
