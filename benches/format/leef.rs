//! PR-6 (v10.7.8): bench LEEF format build (IBM QRadar).
//!
//! До PR-6 не покрывался. LEEF — основной формат для IBM QRadar SIEM.

use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::BTreeMap;
use std::hint::black_box;
use syslog_generator::format::leef;
use syslog_generator::LeefConfig;

fn make_config() -> LeefConfig {
    let mut attrs = BTreeMap::new();
    attrs.insert("src".to_string(), "192.168.1.10".to_string());
    attrs.insert("dst".to_string(), "10.0.0.5".to_string());
    attrs.insert("suser".to_string(), "alice".to_string());
    LeefConfig {
        vendor: "IBM".to_string(),
        product: "QRadar".to_string(),
        version: "7.0".to_string(),
        event_id: "login_success".to_string(),
        attributes: Some(attrs),
    }
}

fn bench_leef_build(c: &mut Criterion) {
    let cfg = make_config();
    let body = b"User alice logged in from 192.168.1.10";
    c.bench_function("leef_build", |b| {
        b.iter(|| {
            let _ = black_box(leef::build(black_box(&cfg), black_box(body)));
        });
    });
}

criterion_group!(benches, bench_leef_build);
criterion_main!(benches);
