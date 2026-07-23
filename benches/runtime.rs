//! [A0] Runtime bench — измеряет полный `run_profile` end-to-end на реалистичных
//! сценариях. Дополняет `hot_path` (per-message overhead) реальным throughput
//! с pacing, transport, metrics.
//!
//! Сценарии:
//! - `runtime_rfc5424_static` — body-only template, no fakers, RFC 5424.
//! - `runtime_rfc5424_faker`  — body-only faker template.
//! - `runtime_rfc3164_static` — body-only template, RFC 3164.
//! - `runtime_json_static`   — body-only JSON-lines.
//!
//! Все runtime benches используют:
//! - Fixed seed (42) для воспроизводимости.
//! - Уникальный tempfile на каждый case (cleanup в конце).
//! - Assert на `run_profile` Ok.
//!
//! Примеры запуска:
//!     cargo bench --bench runtime
//!     cargo bench --bench runtime -- --quick

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::runtime::Runtime;

const MESSAGES_PER_ITER: u64 = 20_000;

fn profile_for(path: &Path, format: &str, body: &str) -> Profile {
    Profile {
        targets: vec![TargetConfig {
            address: path.to_string_lossy().to_string(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "bench".into(),
            duration_secs: 0,
            messages_per_second: 0,
            total_messages: Some(MESSAGES_PER_ITER),
            templates: vec![body.to_string()],
            format: Some(format.to_string()),
            seed: Some(42),
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

async fn run_one_async(profile: Profile) -> u64 {
    let metrics = create_metrics().expect("create_metrics");
    run_profile(black_box(&profile), black_box(metrics))
        .await
        .expect("run_profile ok");
    MESSAGES_PER_ITER
}

fn runtime_bench(c: &mut Criterion, name: &str, format: &str, body: &str) {
    let tmp = tempfile_in_tmp(name);
    let profile = profile_for(&tmp, format, body);
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("runtime");
    group.throughput(Throughput::Elements(MESSAGES_PER_ITER));
    let tmp_for_iter = tmp.clone();
    group.bench_function(name, move |b| {
        b.to_async(&rt).iter(|| async {
            // Удаляем предыдущий файл перед каждой итерацией чтобы измерить
            // cold-write performance, а не append-to-growing-file.
            let _ = std::fs::remove_file(&tmp_for_iter);
            let _ = run_one_async(black_box(profile.clone())).await;
        });
    });
    group.finish();
    let _ = std::fs::remove_file(&tmp);
}

fn bench_runtime_rfc5424_static(c: &mut Criterion) {
    let body = "user=alice seq={{sequence}}";
    runtime_bench(c, "rfc5424_static", "rfc5424", body);
}

fn bench_runtime_rfc5424_faker(c: &mut Criterion) {
    let body = "user {{faker.username}} from {{faker.ipv4}} seq={{sequence}}";
    runtime_bench(c, "rfc5424_faker", "rfc5424", body);
}

fn bench_runtime_rfc3164_static(c: &mut Criterion) {
    let body = "user=alice seq={{sequence}}";
    runtime_bench(c, "rfc3164_static", "rfc3164", body);
}

fn bench_runtime_json_lines(c: &mut Criterion) {
    let body = r#"{"host":"{{hostname}}","app":"{{real_app}}","seq":{{sequence}},"msg":"login"}"#;
    runtime_bench(c, "json_lines_static", "json_lines", body);
}

fn tempfile_in_tmp(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("sg_a0_{nanos}_{name}.log"));
    p
}

criterion_group!(
    benches,
    bench_runtime_rfc5424_static,
    bench_runtime_rfc5424_faker,
    bench_runtime_rfc3164_static,
    bench_runtime_json_lines,
);
criterion_main!(benches);
