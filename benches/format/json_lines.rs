//! PR-6 (v10.7.8): bench JSON-lines format build (NDJSON).
//!
//! До PR-6 не покрывался. Используется для стриминга в JSON-aware приёмники
//! (Elasticsearch, Loki, Datadog).

use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::BTreeMap;
use std::hint::black_box;
use syslog_generator::format::{json_lines, Header};

fn make_header() -> Header {
    Header {
        facility: 16,
        severity: 6,
        hostname: "host01".to_string(),
        app_name: "app".to_string(),
        procid: "1234".to_string(),
        msgid: "ID47".to_string(),
        structured_data: "-".to_string(),
        bom: false,
    }
}

fn make_extra_fields() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for i in 0..5 {
        m.insert(format!("field_{i}"), format!("value_{i}"));
    }
    m
}

fn bench_json_lines_build(c: &mut Criterion) {
    let header = make_header();
    let extra = make_extra_fields();
    let body = b"User alice logged in from 192.168.1.10";
    c.bench_function("json_lines_build", |b| {
        b.iter(|| {
            let _ = black_box(json_lines::build(
                black_box(&header),
                black_box(Some(&extra)),
                black_box(body),
            ));
        });
    });
}

criterion_group!(benches, bench_json_lines_build);
criterion_main!(benches);
