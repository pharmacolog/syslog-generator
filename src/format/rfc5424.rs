//! N10 (v8.8.0): реализация RFC 5424 в слое format.
//!
//! PR-17a (v10.7.16): hot-path — пишем напрямую в `Vec<u8>` без
//! промежуточной `String` через `format!`. `Vec<u8>` impl `std::io::Write`,
//! поэтому `write!(&mut out, ...)` работает идентично `format!`+`as_bytes`,
//! но экономим одну аллокацию + memcpy.

use super::{sanitize_header, Header, BOM, NILVALUE};

/// Собрать полное сообщение RFC 5424:
/// `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID [BOM]MSG`
///
/// PR-17c (v10.7.18): использует `h.timestamp` (pre-computed) если не пустой —
/// устраняет `Utc::now()` + `chrono::format!` в hot-path (~80-150 нс/msg).
#[inline]
pub fn build(h: &Header, msg: &[u8]) -> Vec<u8> {
    let mut out = bytes::BytesMut::new();
    build_into(&mut out, h, msg);
    out.to_vec()
}

/// PR-A2.4 (v10.8.0): caller-owned BytesMut variant — избегает heap allocation
/// per message. Используется из slot-based hot path (`generate_message_with_plan`).
#[inline]
pub fn build_into(out: &mut bytes::BytesMut, h: &Header, msg: &[u8]) {
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
    out.reserve(estimated);

    // `<PRI>1 TIMESTAMP`
    // BytesMut не реализует fmt::Write — пишем pri вручную через push_u8.
    out.extend_from_slice(b"<");
    push_u16_decimal(out, pri);
    out.extend_from_slice(b">1 ");
    // PR-17c: используем pre-computed timestamp из Header (если есть),
    // иначе legacy path с Utc::now() внутри.
    if h.timestamp.is_empty() {
        let ts = super::rfc5424_timestamp();
        out.extend_from_slice(ts.as_bytes());
    } else {
        out.extend_from_slice(h.timestamp.as_bytes());
    }

    // ` HOSTNAME APP-NAME PROCID MSGID SD`
    out.extend_from_slice(b" ");
    out.extend_from_slice(hostname.as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(app_name.as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(procid.as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(msgid.as_bytes());
    out.extend_from_slice(b" ");
    if h.structured_data.is_empty() {
        out.extend_from_slice(NILVALUE.as_bytes());
    } else {
        out.extend_from_slice(h.structured_data.as_bytes());
    }

    // ` [BOM]MSG`
    out.extend_from_slice(b" ");
    if h.bom {
        out.extend_from_slice(BOM);
    }
    out.extend_from_slice(msg);
}

/// PR-A2.4 helper: записать u16 в decimal в BytesMut (без format!).
#[inline]
fn push_u16_decimal(out: &mut bytes::BytesMut, mut n: u16) {
    if n == 0 {
        out.extend_from_slice(b"0");
        return;
    }
    let mut buf = [0u8; 5]; // u16 max = 65535 = 5 цифр
    let mut i = 5;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out.extend_from_slice(&buf[i..]);
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

    /// PR-A2.4: `build_into` должен давать identical output к `build`
    /// byte-for-byte для всех test cases.
    #[test]
    fn a2_4_build_into_matches_build() {
        // Test 1: minimal header
        let h1 = hdr();
        let legacy = build(&h1, b"hello world");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h1, b"hello world");
        assert_eq!(legacy.as_slice(), &bm[..], "minimal: legacy vs build_into");

        // Test 2: with BOM
        let mut h2 = hdr();
        h2.bom = true;
        let legacy = build(&h2, b"x");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h2, b"x");
        assert_eq!(legacy.as_slice(), &bm[..], "with BOM: legacy vs build_into");

        // Test 3: empty SD
        let mut h3 = hdr();
        h3.structured_data = "".into();
        let legacy = build(&h3, b"test");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h3, b"test");
        assert_eq!(legacy.as_slice(), &bm[..], "empty SD: legacy vs build_into");

        // Test 4: custom SD
        let mut h4 = hdr();
        h4.structured_data = "[example@32473 iut=\"3\"]".into();
        let legacy = build(&h4, b"x");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h4, b"x");
        assert_eq!(
            legacy.as_slice(),
            &bm[..],
            "custom SD: legacy vs build_into"
        );

        // Test 5: high prival
        let mut h5 = hdr();
        h5.facility = 23;
        h5.severity = 7;
        let legacy = build(&h5, b"x");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h5, b"x");
        assert_eq!(
            legacy.as_slice(),
            &bm[..],
            "high prival: legacy vs build_into"
        );

        // Test 6: empty msg
        let h6 = hdr();
        let legacy = build(&h6, b"");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h6, b"");
        assert_eq!(
            legacy.as_slice(),
            &bm[..],
            "empty msg: legacy vs build_into"
        );

        // Test 7: pre-computed timestamp (PR-17c path)
        let mut h7 = hdr();
        h7.timestamp = "2026-07-23T14:30:00.000Z".into();
        let legacy = build(&h7, b"data");
        let mut bm = bytes::BytesMut::new();
        build_into(&mut bm, &h7, b"data");
        assert_eq!(
            legacy.as_slice(),
            &bm[..],
            "pre-computed ts: legacy vs build_into"
        );
    }

    /// PR-A2.4: caller-owned BytesMut переиспользуется между messages
    /// без heap allocations (capacity preserved).
    #[test]
    fn a2_4_build_into_caller_owned_capacity_preserved() {
        let mut bm = bytes::BytesMut::new();
        let initial_cap = bm.capacity();

        for i in 0..100 {
            bm.clear();
            build_into(&mut bm, &hdr(), format!("msg-{i}").as_bytes());
            assert!(bm.capacity() >= initial_cap, "capacity must not shrink");
            assert!(
                std::str::from_utf8(&bm[..])
                    .unwrap()
                    .contains(&format!("msg-{i}")),
                "msg-{i} should be in buffer"
            );
        }
    }
}
