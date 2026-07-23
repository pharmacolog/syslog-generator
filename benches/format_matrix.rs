//! [A0] Format matrix bench — `PhaseContext` + `generate_message_with_format_cached`
//! по 7 форматам и 2 payload-вариантам.
//!
//! Форматы: rfc5424, rfc3164, raw, protobuf, cef, leef, json_lines.
//! Payload:
//! - static:  без placeholders.
//! - faker:   {{faker.username}}, {{faker.ipv4}}.
//!
//! Каждый формат использует правильную конфигурацию:
//! - CEF требует CefConfig (иначе fallback на raw).
//! - LEEF требует LeefConfig.
//! - Protobuf использует пустую схему (валидный empty protobuf).
//!
//! Примеры:
//!     cargo bench --bench format_matrix
//!     cargo bench --bench format_matrix -- --quick

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::collections::HashMap;
use std::hint::black_box;
use syslog_generator::{
    format::FormatKind, generate_message_with_format_cached, load_profile_from_yaml_str, CefConfig,
    LeefConfig, Phase, PhaseContext, ProtobufSchemaFieldMap,
};

fn yaml_for(format: &str, body: &str) -> String {
    let phase_extra = match format {
        "cef" => {
            r#"
      cef:
        device_vendor: Acme
        device_product: SyslogGen
        device_version: "1.0"
        signature_id: "42"
        name: login
        severity: 5
"#
        }
        "leef" => {
            r#"
      leef:
        vendor: Acme
        product: SyslogGen
        version: "1.0"
        event_id: "42"
"#
        }
        "protobuf" => {
            r#"
      protobuf_schema:
        fields: {}
"#
        }
        _ => "",
    };
    format!(
        r#"
targets:
  - address: /tmp/bench.log
    transport: file
distribution: round-robin
phases:
  - name: bench
    duration_secs: 0
    total_messages: 1000
    messages_per_second: 0
    templates:
      - {body}
    format: {format}
    seed: 42
    syslog:
      facility: 16
      severity: 6
{phase_extra}
"#
    )
}

fn build(format: &str, body: &str) -> (PhaseContext, Phase, FormatKind) {
    let profile = load_profile_from_yaml_str(&yaml_for(format, body)).expect("parse");
    let phase = profile.phases.into_iter().next().expect("phase");
    let ctx = PhaseContext::resolve(&phase).expect("ctx");
    let fk = FormatKind::parse(format).unwrap_or(FormatKind::Raw);
    (ctx, phase, fk)
}

fn bench_one(c: &mut Criterion, name: &str, ctx: PhaseContext, phase: Phase, fk: FormatKind) {
    let mut group = c.benchmark_group("format_matrix");
    group.throughput(Throughput::Elements(1));
    let mut values = HashMap::with_capacity(16);
    group.bench_function(name, |b| {
        let mut seq = 0usize;
        b.iter(|| {
            seq += 1;
            let msg = generate_message_with_format_cached(
                black_box(&ctx),
                black_box(&phase),
                black_box(&fk),
                black_box(seq),
                black_box(&mut values),
            )
            .unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

fn bench_static(c: &mut Criterion) {
    for fmt in [
        "rfc5424",
        "rfc3164",
        "raw",
        "protobuf",
        "cef",
        "leef",
        "json_lines",
    ] {
        let (ctx, phase, fk) = build(fmt, "seq={{sequence}}");
        bench_one(c, &format!("{fmt}_static"), ctx, phase, fk);
    }
}

fn bench_faker(c: &mut Criterion) {
    let body = "user {{faker.username}} from {{faker.ipv4}} seq={{sequence}}";
    for fmt in ["rfc5424", "json_lines"] {
        let (ctx, phase, fk) = build(fmt, body);
        bench_one(c, &format!("{fmt}_faker"), ctx, phase, fk);
    }
}

// Compile-time guards: ensure config types are reachable from this bench
// so future CEF/LEEF/Protobuf enhancements stay wired.
#[allow(dead_code)]
fn _type_guards(
    cef: CefConfig,
    leef: LeefConfig,
    proto: ProtobufSchemaFieldMap,
) -> (CefConfig, LeefConfig, ProtobufSchemaFieldMap) {
    (cef, leef, proto)
}

criterion_group!(benches, bench_static, bench_faker);
criterion_main!(benches);
