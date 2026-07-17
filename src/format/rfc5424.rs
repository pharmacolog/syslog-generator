//! N10 (v8.8.0): реализация RFC 5424 в слое format.
//!
//! PR-17a (v10.7.16): hot-path — пишем напрямую в `Vec<u8>` без
//! промежуточной `String` через `format!`. `Vec<u8>` impl `std::io::Write`,
//! поэтому `write!(&mut out, ...)` работает идентично `format!`+`as_bytes`,
//! но экономим одну аллокацию + memcpy.

use super::{sanitize_header, Header, BOM, NILVALUE};
use std::io::Write as _;

/// Собрать полное сообщение RFC 5424:
/// `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID [BOM]MSG`
///
/// PR-17c (v10.7.18): использует `h.timestamp` (pre-computed) если не пустой —
/// устраняет `Utc::now()` + `chrono::format!` в hot-path (~80-150 нс/msg).
#[inline]
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let pri = super::prival(h.facility, h.severity);
    let hostname = sanitize_header(&h.hostname, 255);
    let app_name = sanitize_header(&h.app_name, 48);
    let procid = sanitize_header(&h.procid, 128);
    let msgid = sanitize_header(&h.msgid, 32);

    // Грубая оценка ёмкости: <pri>1(4) + ts(24) + 5 hostname/app/procid/msgid (≤463)
    // + sd (variable) + BOM(3) + msg + пробелы.
    let estimated = 200
        + hostname.len()
        + app_name.len()
        + procid.len()
        + msgid.len()
        + h.structured_data.len()
        + msg.len();
    let mut out = Vec::with_capacity(estimated);

    // `<PRI>1 TIMESTAMP`
    let _ = write!(out, "<{}>1 ", pri);
    // PR-17c: используем pre-computed timestamp из Header (если есть),
    // иначе legacy path с Utc::now() внутри.
    if h.timestamp.is_empty() {
        let ts = super::rfc5424_timestamp();
        out.extend_from_slice(ts.as_bytes());
    } else {
        out.extend_from_slice(h.timestamp.as_bytes());
    }

    // ` HOSTNAME APP-NAME PROCID MSGID SD`
    out.push(b' ');
    out.extend_from_slice(hostname.as_bytes());
    out.push(b' ');
    out.extend_from_slice(app_name.as_bytes());
    out.push(b' ');
    out.extend_from_slice(procid.as_bytes());
    out.push(b' ');
    out.extend_from_slice(msgid.as_bytes());
    out.push(b' ');
    if h.structured_data.is_empty() {
        out.extend_from_slice(NILVALUE.as_bytes());
    } else {
        out.extend_from_slice(h.structured_data.as_bytes());
    }

    // ` [BOM]MSG`
    out.push(b' ');
    if h.bom {
        out.extend_from_slice(BOM);
    }
    out.extend_from_slice(msg);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr() -> Header {
        Header {
            facility: 16,
            severity: 6,
            hostname: "host".into(),
            app_name: "app".into(),
            procid: "1234".into(),
            msgid: "ID47".into(),
            structured_data: "-".into(),
            timestamp: "".into(),
            bom: false,
        }
    }

    #[test]
    fn builds_minimal_rfc5424() {
        let out = build(&hdr(), b"hello");
        let s = std::str::from_utf8(&out).unwrap();
        // PRI = 16*8+6 = 134
        assert!(s.starts_with("<134>1 "), "got: {s}");
        assert!(s.ends_with(" hello"), "got: {s}");
        // ts is real-time, so just verify structure.
        assert!(s.contains(" host "), "got: {s}");
        assert!(s.contains(" app "), "got: {s}");
        assert!(s.contains(" 1234 "), "got: {s}");
        assert!(s.contains(" ID47 "), "got: {s}");
        assert!(s.contains(" - "), "got: {s}");
    }

    #[test]
    fn includes_bom_when_requested() {
        let mut h = hdr();
        h.bom = true;
        let out = build(&h, b"x");
        // BOM EF BB BF
        assert!(
            out.windows(3).any(|w| w == BOM),
            "expected BOM, got: {out:?}"
        );
    }

    #[test]
    fn empty_structured_data_becomes_nilvalue() {
        let mut h = hdr();
        h.structured_data = "".into();
        let out = build(&h, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains(" - "),
            "expected NILVALUE for empty SD, got: {s}"
        );
    }

    #[test]
    fn preserves_custom_structured_data() {
        let mut h = hdr();
        h.structured_data = "[example@32473 iut=\"3\"]".into();
        let out = build(&h, b"x");
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains(" [example@32473 iut=\"3\"] "),
            "expected custom SD, got: {s}"
        );
    }

    #[test]
    fn prival_high_bits() {
        // Facility=23, Severity=7 → 23*8+7 = 191.
        let mut h = hdr();
        h.facility = 23;
        h.severity = 7;
        let out = build(&h, b"x");
        assert!(out.starts_with(b"<191>1 "), "got: {out:?}");
    }
}
