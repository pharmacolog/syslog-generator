//! PR-6 (v10.7.8): bench reconnect exponential backoff (F16, v9.3.0).
//!
//! До PR-6 benches не покрывали reconnect — был только TCP/UDP. Exponential
//! backoff вычисляется per-reconnect-attempt; замеряем overhead публичного
//! API (per-target setup, не per-msg).

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use syslog_generator::transport::reconnect::ReconnectConfig;

fn bench_reconnect_default(c: &mut Criterion) {
    c.bench_function("reconnect_default", |b| {
        b.iter(|| {
            let _ = black_box(ReconnectConfig::default());
        });
    });
}

fn bench_reconnect_resolve(c: &mut Criterion) {
    c.bench_function("reconnect_resolve_defaults", |b| {
        b.iter(|| {
            let _ = black_box(ReconnectConfig::resolve(None, None, None, None));
        });
    });
}

fn bench_reconnect_resolve_full(c: &mut Criterion) {
    c.bench_function("reconnect_resolve_full", |b| {
        b.iter(|| {
            let _ = black_box(ReconnectConfig::resolve(
                Some(5),
                Some(100),
                Some(30000),
                Some(2.0),
            ));
        });
    });
}

fn bench_reconnect_validate(c: &mut Criterion) {
    let cfg = ReconnectConfig::default();
    c.bench_function("reconnect_validate", |b| {
        b.iter(|| {
            let _ = black_box(cfg.validate());
        });
    });
}

criterion_group!(
    benches,
    bench_reconnect_default,
    bench_reconnect_resolve,
    bench_reconnect_resolve_full,
    bench_reconnect_validate
);
criterion_main!(benches);
