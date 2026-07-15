//! PR-6 (v10.7.8): bench file rotation overhead.
//!
//! До PR-6 benches не покрывали file rotation (F16, v9.3.0). Rotation
//! per-message check встроен в sender loop; замеряем overhead публичного
//! `RotationConfig` API (per-target setup, не per-msg).

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use syslog_generator::transport::file::RotationConfig;

fn bench_rotation_is_enabled(c: &mut Criterion) {
    let cfg = RotationConfig {
        size_mb: Some(10),
        interval_secs: None,
        max_files: Some(5),
    };
    c.bench_function("rotation_is_enabled", |b| {
        b.iter(|| {
            let _ = black_box(cfg.is_enabled());
        });
    });
}

fn bench_rotation_is_disabled(c: &mut Criterion) {
    let cfg = RotationConfig::default();
    c.bench_function("rotation_is_disabled", |b| {
        b.iter(|| {
            let _ = black_box(cfg.is_enabled());
        });
    });
}

fn bench_rotation_effective_max_files(c: &mut Criterion) {
    let cfg = RotationConfig {
        size_mb: Some(10),
        interval_secs: None,
        max_files: Some(5),
    };
    c.bench_function("rotation_effective_max_files", |b| {
        b.iter(|| {
            let _ = black_box(cfg.effective_max_files());
        });
    });
}

fn bench_rotation_validate(c: &mut Criterion) {
    let cfg = RotationConfig {
        size_mb: Some(10),
        interval_secs: None,
        max_files: Some(5),
    };
    c.bench_function("rotation_validate", |b| {
        b.iter(|| {
            let _ = black_box(cfg.validate());
        });
    });
}

criterion_group!(
    benches,
    bench_rotation_is_enabled,
    bench_rotation_is_disabled,
    bench_rotation_effective_max_files,
    bench_rotation_validate
);
criterion_main!(benches);