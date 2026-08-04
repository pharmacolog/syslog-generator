//! Issue #165 (C3): snapshot tests для output formats (rfc5424, rfc3164,
//! cef, leef, json_lines, raw).
//!
//! Использует `insta` crate для byte-for-byte verification. Snapshot tests
//! дополняют unit-тесты проверкой фиксированного output (regression detection):
//! если output format изменится, snapshot test падает и требует обновления.
//!
//! При первом запуске insta генерирует `*.snap` файлы в `src/format/snapshots/`
//! (inline snapshots в source code через `assert_snapshot!`).
//!
//! CI проверяет `INSTA_UPDATE=no` чтобы snapshots не обновлялись автоматически.

use crate::format::{cef, json_lines, leef, rfc3164, rfc5424, Header};
use crate::generator::config::{CefConfig, LeefConfig};
use insta::assert_snapshot;

/// Snapshot для rfc5424 с минимальным header.
#[test]
fn snapshot_rfc5424_basic() {
    let h = header_rfc5424();
    let out = rfc5424::build(&h, b"hello world");
    assert_snapshot!(String::from_utf8_lossy(&out));
}

/// Snapshot для rfc5424 с пустым body.
#[test]
fn snapshot_rfc5424_empty_body() {
    let h = header_rfc5424();
    let out = rfc5424::build(&h, b"");
    assert_snapshot!(String::from_utf8_lossy(&out));
}

/// Snapshot для rfc3164 — формат содержит Local::now() timestamp,
/// поэтому сравниваем prefix без timestamp (стабильная часть).
#[test]
fn snapshot_rfc3164_basic() {
    let h = header_rfc3164();
    let out = rfc3164::build(&h, b"test msg");
    let s = String::from_utf8_lossy(&out);
    // Стабильная часть: всё начиная с hostname
    // (timestamp и pri остаются, но Local::now() делает timestamp non-deterministic).
    // Используем snapshot только для prefix от `test-host`.
    if let Some(idx) = s.find("test-host") {
        assert_snapshot!(s[idx..].to_string());
    } else {
        panic!("test-host not found in rfc3164 output: {}", s);
    }
}

/// Snapshot для CEF формата.
#[test]
fn snapshot_cef_basic() {
    let cfg = CefConfig {
        device_vendor: "TestVendor".into(),
        device_product: "TestProduct".into(),
        device_version: "1.0".into(),
        signature_id: "100".into(),
        name: "test_event".into(),
        severity: Some(5),
        extensions: None,
    };
    let out = cef::build(&cfg, b"test payload");
    assert_snapshot!(String::from_utf8_lossy(&out));
}

/// Snapshot для LEEF формата.
#[test]
fn snapshot_leef_basic() {
    let cfg = LeefConfig {
        vendor: "TestVendor".into(),
        product: "TestProduct".into(),
        version: "1.0".into(),
        event_id: "evt001".into(),
        attributes: None,
    };
    let out = leef::build(&cfg, b"test event");
    assert_snapshot!(String::from_utf8_lossy(&out));
}

/// Snapshot для JSON lines формата.
#[test]
fn snapshot_json_lines_basic() {
    let h = header_rfc5424();
    let out = json_lines::build(&h, None, b"json msg");
    assert_snapshot!(String::from_utf8_lossy(&out));
}

/// Helper: стандартный Header для rfc5424 формата (с фиксированным
/// timestamp для детерминированных snapshot tests).
fn header_rfc5424() -> Header {
    Header {
        facility: 16,
        severity: 6,
        hostname: "test-host".into(),
        app_name: "test-app".into(),
        procid: "12345".into(),
        msgid: "TST".into(),
        structured_data: "-".into(),
        // Фиксированный timestamp — иначе snapshot tests дают разный output
        // при каждом запуске (rfc5424 fallback на Utc::now()).
        timestamp: "2026-01-01T00:00:00.000Z".into(),
        bom: false,
    }
}

/// Helper: стандартный Header для rfc3164 формата.
fn header_rfc3164() -> Header {
    Header {
        facility: 16,
        severity: 6,
        hostname: "test-host".into(),
        app_name: "test-app".into(),
        procid: "12345".into(),
        msgid: "".into(),
        structured_data: "-".into(),
        timestamp: "2026-01-01T00:00:00".into(),
        bom: false,
    }
}
