//! N10 (v8.8.0): реализация RFC 3164 (BSD) в слое format.
//!
//! PR-17a (v10.7.16): hot-path — пишем напрямую в `Vec<u8>` без
//! промежуточных `String` через `format!`. `Vec<u8>` impl `std::io::Write`,
//! поэтому `write!(&mut out, ...)` работает идентично.

use super::{sanitize_header, Header, NILVALUE};
use chrono::Local;
use std::io::Write as _;

/// Собрать сообщение RFC 3164 (BSD):
/// `<PRI>Mmm dd hh:mm:ssHOSTNAME TAG: MSG`
/// TIMESTAMP — локальное время; день с ведущим пробелом для 1..9.
/// TAG формируется из app_name (+ `procid` в квадратных скобках, если не NILVALUE).
#[inline]
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = super::prival(h.facility, h.severity);

    // TS: 15 chars ("Mmm dd hh:mm:ss") — локальное время.
    let ts = Local::now().format("%b %e %H:%M:%S").to_string();

    // HOSTNAME: sanitized, NILVALUE → "localhost".
    let hostname_raw = sanitize_header(&h.hostname, 255);
    let hostname: &[u8] = if hostname_raw == NILVALUE {
        b"localhost"
    } else {
        hostname_raw.as_bytes()
    };

    // APP: sanitized, NILVALUE → "app".
    let app_raw = sanitize_header(&h.app_name, 32);
    let app: &[u8] = if app_raw == NILVALUE {
        b"app"
    } else {
        app_raw.as_bytes()
    };

    // PID: sanitized (если есть и не NILVALUE).
    let pid = sanitize_header(&h.procid, 128);
    let has_pid = !pid.is_empty() && pid != NILVALUE;

    // Оценка ёмкости: <pri>(4) + ts(15) + ' ' + hostname(≤255) + ' ' + app(≤32)
    // + [pid](≤130) + ':' + ' ' + msg
    let estimated = 4 + ts.len() + 1 + hostname.len() + 1 + app.len()
        + if has_pid { pid.len() + 3 } else { 1 }
        + 1 + msg.len();
    let mut out = Vec::with_capacity(estimated);

    // <PRI>TIMESTAMP HOSTNAME TAG MSG
    let _ = write!(out, "<{}>{} ", pri, ts);

    out.extend_from_slice(hostname);
    out.push(b' ');

    // TAG: "app:" или "app[pid]:"
    out.extend_from_slice(app);
    if has_pid {
        out.push(b'[');
        out.extend_from_slice(pid.as_bytes());
        out.extend_from_slice(b"]:");
    } else {
        out.push(b':');
    }

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
    #[test]
    fn rfc3164_empty_app_name_does_not_panic() {
        let mut h = test_header();
        h.app_name = String::new();
        let out = build(&h, b"msg");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("myhost"), "expected hostname, got: {}", s);
    }

    /// Когда app_name = NILVALUE ("-"), sanitize_header возвращает "-",
    /// затем код rfc3164 видит app == NILVALUE и подставляет дефолт "app".
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
        assert!(
            s.ends_with(" BODY"),
            "expected space-separated body, got: {}",
            s
        );
    }
}