//! Issue #211: allocations bench — throughput-based benchmark для alloc profile.
//!
//! Этот bench измеряет **throughput** (msg/sec) для hot-path операций.
//! Variance снижается через Criterion's iter_batched (очистка allocations
//! между батчами) + median-of-3-runs в workflow (Issue #214).
//!
//! Threshold для gate: 15% (allocations category в perf-regression gate).
//!
//! Использование:
//!     cargo bench --bench allocations -- --quick
//!
//! Coverage:
//! - `message_generation/rfc5424_with_faker` — Vec<u8> per message
//! - `format_encoding/rfc5424` — RFC 5424 format alloc pattern
//! - `format_encoding/json_lines` — JSON alloc pattern (highest variance)
//! - `transport_buffering/bytesmut` — bytes::BytesMut accumulation
//! - `payload/render_template` — template + faker alloc pattern

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use syslog_generator::{
    format::{build_rfc5424, FormatKind, Header},
    generate_message_with_format_cached, load_profile_from_yaml_str, render_template, PhaseContext,
};

const PROFILE_YAML: &str = r#"
targets:
  - address: /tmp/syslog-gen-bench.log
    transport: file
distribution: round-robin
phases:
  - name: bench
    duration_secs: 0
    total_messages: 100000
    messages_per_second: 0
    templates:
      - "<165>1 {{timestamp}} {{hostname}} {{real_app}}[{{pid}}]: user {{faker.username}} from {{faker.ipv4}} login seq={{sequence}}"
    syslog:
      facility: 16
      severity: 6
"#;

/// Benchmark: message generation throughput (RFC 5424 with faker tokens).
///
/// Измеряет msg/sec с полным hot-path (template render + format encoding +
/// payload assembly). Alloc profile: Vec<u8> per message + String for faker
/// tokens + HashMap entries.
fn bench_message_generation(c: &mut Criterion) {
    let profile = load_profile_from_yaml_str(PROFILE_YAML).expect("profile parses");
    let phase = profile.phases.first().expect("phase exists").clone();
    let ctx = PhaseContext::resolve(&phase).expect("ctx resolves");
    let format_kind =
        FormatKind::parse(phase.format.as_deref().unwrap_or("rfc5424")).expect("format parses");
    let metrics = syslog_generator::create_metrics().expect("metrics ok");

    let mut group = c.benchmark_group("allocations/message_generation");
    group.throughput(Throughput::Elements(1));

    group.bench_function("rfc5424_with_faker", |b| {
        let mut values = HashMap::with_capacity(16);
        let msg_counter = metrics
            .messages_generated_total
            .with_label_values(&["bench"]);
        b.iter(|| {
            let msg = generate_message_with_format_cached(
                black_box(&ctx),
                black_box(&phase),
                black_box(&format_kind),
                black_box(1),
                black_box(&mut values),
            )
            .expect("generate ok");
            msg_counter.inc();
            black_box(msg);
        });
    });

    group.finish();
}

/// Benchmark: format encoding (RFC 5424) — без message context overhead.
///
/// Измеряет только format encoding step (Header + payload → Vec<u8>).
/// Alloc profile: Vec<u8> growth from format::build_rfc5424.
fn bench_format_encoding_rfc5424(c: &mut Criterion) {
    let payload = b"user alice from 192.168.1.10 login seq=42";
    let header = Header {
        facility: 16,
        severity: 6,
        hostname: Arc::from("hostname"),
        app_name: Arc::from("appname"),
        procid: Arc::from("1234"),
        msgid: Arc::from("-"),
        structured_data: Arc::from("-"),
        timestamp: Arc::from("2026-08-04T10:00:00.000Z"),
        bom: false,
    };

    let mut group = c.benchmark_group("allocations/format_encoding");
    group.throughput(Throughput::Elements(1));

    group.bench_function("rfc5424", |b| {
        b.iter(|| {
            let buf = build_rfc5424(black_box(&header), black_box(payload));
            black_box(buf);
        });
    });

    group.finish();
}

/// Benchmark: format encoding (JSON lines) — самый высокий alloc variance.
///
/// JSON serialization создаёт много мелких allocations (String per field).
/// Это benchmark для worst-case alloc scenario.
fn bench_format_encoding_json(c: &mut Criterion) {
    use serde_json::json;

    let mut group = c.benchmark_group("allocations/format_encoding");
    group.throughput(Throughput::Elements(1));

    group.bench_function("json_lines", |b| {
        b.iter(|| {
            let entry = json!({
                "timestamp": "2026-08-04T10:00:00Z",
                "hostname": "host-01",
                "appname": "syslog-gen",
                "pid": 1234,
                "message": "user alice from 192.168.1.10 login seq=42",
            });
            let serialized = serde_json::to_string(black_box(&entry)).expect("serialize ok");
            black_box(serialized);
        });
    });

    group.finish();
}

/// Benchmark: transport buffering (BytesMut accumulation).
///
/// Симулирует typical transport buffer pattern: append message bytes,
/// flush when capacity exceeded. Используется для UDP/TCP/file rotation.
fn bench_transport_buffering(c: &mut Criterion) {
    use bytes::BytesMut;

    let mut group = c.benchmark_group("allocations/transport_buffering");
    group.throughput(Throughput::Elements(1));

    group.bench_function("bytesmut_accumulation", |b| {
        b.iter(|| {
            let mut buf = BytesMut::with_capacity(8192);
            for _ in 0..16 {
                let chunk: &[u8] = b"<165>1 2026-08-04T10:00:00Z host app 1234 - - user alice from 192.168.1.10 login seq=42\n";
                buf.extend_from_slice(black_box(chunk));
            }
            black_box(buf);
        });
    });

    group.finish();
}

/// Benchmark: payload rendering (template + faker).
///
/// Измеряет только payload rendering step без format encoding. Alloc profile:
/// HashMap<String, String> для faker cache + format!() for templated output.
fn bench_payload_render(c: &mut Criterion) {
    let template_str = "user {{faker.username}} from {{faker.ipv4}} login seq={{sequence}}";
    let mut values = HashMap::new();
    values.insert("faker.username".to_string(), "alice".to_string());
    values.insert("faker.ipv4".to_string(), "192.168.1.10".to_string());
    values.insert("sequence".to_string(), "42".to_string());

    let mut group = c.benchmark_group("allocations/payload");
    group.throughput(Throughput::Elements(1));

    group.bench_function("render_template", |b| {
        b.iter(|| {
            let payload = render_template(black_box(template_str), black_box(&values));
            black_box(payload);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_message_generation,
    bench_format_encoding_rfc5424,
    bench_format_encoding_json,
    bench_transport_buffering,
    bench_payload_render
);
criterion_main!(benches);

// Issue #211 acceptance: smoke tests для allocations bench.
// Проверяют что bench target компилируется + корректно исполняется один раз.
#[cfg(test)]
#[allow(unused_imports, dead_code)]
mod tests {
    use super::{load_profile_from_yaml_str, FormatKind, PhaseContext, PROFILE_YAML};
    use std::time::Instant;
    use syslog_generator::{
        format::{build_rfc5424, Header},
        generate_message_with_format_cached, render_template,
    };

    /// Sanity test: message generation работает в bench context (без Criterion).
    /// Возвращает Vec<u8> после одного вызова generate_message_with_format_cached.
    #[test]
    fn test_message_generation_smoke() {
        let profile = load_profile_from_yaml_str(PROFILE_YAML).expect("profile parses");
        let phase = profile.phases.first().expect("phase exists").clone();
        let ctx = PhaseContext::resolve(&phase).expect("ctx resolves");
        let format_kind =
            FormatKind::parse(phase.format.as_deref().unwrap_or("rfc5424")).expect("format parses");
        let metrics = syslog_generator::create_metrics().expect("metrics ok");
        let _msg_counter = metrics
            .messages_generated_total
            .with_label_values(&["test"]);

        let mut values = HashMap::with_capacity(16);
        let msg = generate_message_with_format_cached(&ctx, &phase, &format_kind, 1, &mut values)
            .expect("generate ok");

        // RFC 5424 message должен начинаться с "<" (priority).
        assert!(!msg.is_empty(), "message should not be empty");
        assert!(msg[0] == b'<', "message should start with priority '<'");
    }

    /// Sanity test: format encoding (RFC 5424) производит валидный output.
    #[test]
    fn test_format_encoding_rfc5424_smoke() {
        let payload = b"test message";
        let header = Header {
            facility: 16,
            severity: 6,
            hostname: Arc::from("host"),
            app_name: Arc::from("app"),
            procid: Arc::from("1"),
            msgid: Arc::from("-"),
            structured_data: Arc::from("-"),
            timestamp: Arc::from("2026-08-04T10:00:00.000Z"),
            bom: false,
        };
        let buf = build_rfc5424(&header, payload);
        assert!(!buf.is_empty(), "encoded message should not be empty");
        // RFC 5424 формат начинается с "<PRI>1 ".
        assert!(buf.starts_with(b"<"), "RFC 5424 should start with priority");
    }

    /// Sanity test: render_template возвращает непустую строку.
    #[test]
    fn test_render_template_smoke() {
        let template_str = "user {{faker.username}} seq={{sequence}}";
        let mut values = HashMap::new();
        values.insert("faker.username".to_string(), "alice".to_string());
        values.insert("sequence".to_string(), "42".to_string());
        let rendered = render_template(template_str, &values);
        assert!(!rendered.is_empty());
        assert!(
            rendered.contains("alice"),
            "rendered should contain username"
        );
        assert!(rendered.contains("42"), "rendered should contain sequence");
    }

    /// Sanity test: BytesMut accumulation работает.
    #[test]
    fn test_bytesmut_accumulation_smoke() {
        use bytes::BytesMut;
        let mut buf = BytesMut::with_capacity(1024);
        let chunk: &[u8] = b"hello world\n";
        for _ in 0..10 {
            buf.extend_from_slice(chunk);
        }
        let expected_len = chunk.len() * 10;
        assert_eq!(
            buf.len(),
            expected_len,
            "BytesMut should accumulate all chunks"
        );
    }

    /// Sanity test: bench выполняется за разумное время (< 30s single iteration).
    /// Это protection against infinite loops в bench logic.
    #[test]
    fn test_bench_no_infinite_loop() {
        let start = Instant::now();
        let profile = load_profile_from_yaml_str(PROFILE_YAML).expect("profile parses");
        let phase = profile.phases.first().expect("phase exists").clone();
        let ctx = PhaseContext::resolve(&phase).expect("ctx resolves");
        let format_kind =
            FormatKind::parse(phase.format.as_deref().unwrap_or("rfc5424")).expect("format parses");

        let mut values = HashMap::with_capacity(16);
        for i in 0..100 {
            let _msg =
                generate_message_with_format_cached(&ctx, &phase, &format_kind, i, &mut values)
                    .expect("generate ok");
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs() < 30,
            "100 messages should complete in <30s, got {:?}",
            elapsed
        );
    }
}
