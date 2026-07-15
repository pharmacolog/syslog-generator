//! PR-6 (v10.7.8): bench CEF format build (ArcSight Common Event Format).
//!
//! До PR-6 benches не покрывали CEF (F15, v9.2.0) — был только rfc5424
//! в message_generation. CEF используется в SIEM (ArcSight, Elastic Stack
//! с коннектором), и его throughput критичен для workloads, эмулирующих
//! SIEM приём данных.

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use syslog_generator::format::cef;
use syslog_generator::CefConfig;

fn make_context() -> CefConfig {
    let mut extensions = std::collections::BTreeMap::new();
    extensions.insert("src".to_string(), "192.168.1.10".to_string());
    extensions.insert("dst".to_string(), "10.0.0.5".to_string());
    extensions.insert("suser".to_string(), "alice".to_string());
    CefConfig {
        device_vendor: "Security".to_string(),
        device_product: "fw".to_string(),
        device_version: "1.0".to_string(),
        signature_id: "1001".to_string(),
        name: "login_success".to_string(),
        severity: Some(5),
        extensions: Some(extensions),
    }
}

fn bench_cef_build(c: &mut Criterion) {
    let cfg = make_context();
    let body = b"User alice logged in from 192.168.1.10".to_vec();
    c.bench_function("cef_build", |b| {
        b.iter(|| {
            let _ = black_box(cef::build(black_box(&cfg), black_box(&body)));
        });
    });
}

criterion_group!(benches, bench_cef_build);
criterion_main!(benches);
