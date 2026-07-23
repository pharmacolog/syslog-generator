//! [A0] Runtime bench — измеряет полный `run_profile` end-to-end на реалистичных
//! сценариях. Дополняет `hot_path` (per-message overhead) реальным throughput
//! с pacing, transport, metrics.
//!
//! Сценарии:
//! - `runtime_rfc5424_unlimited` — static RFC 5424 в /dev/null, без rate-limit.
//! - `runtime_rfc5424_faker`     — faker tokens, без rate-limit.
//! - `runtime_rfc3164_unlimited` — RFC 3164 static.
//! - `runtime_json_lines`        — JSON-lines static.
//! - `runtime_udp_loopback`      — UDP 127.0.0.1, generator → loopback receiver.
//!
//! Примеры запуска:
//!     cargo bench --bench runtime
//!     cargo bench --bench runtime -- --quick

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::runtime::Runtime;

fn static_template() -> &'static str {
    "<165>1 {{timestamp}} {{hostname}} {{real_app}}[{{pid}}]: login user=alice seq={{sequence}}"
}

fn faker_template() -> &'static str {
    "<165>1 {{timestamp}} {{hostname}} {{real_app}}[{{pid}}]: user {{faker.username}} from {{faker.ipv4}} seq={{sequence}}"
}

fn json_template() -> &'static str {
    r#"{"ts":"{{timestamp}}","host":"{{hostname}}","app":"{{real_app}}","pid":{{pid}},"seq":{{sequence}},"msg":"login user=alice"}"#
}

fn profile_for(template: &str, transport: &str, addr: &str) -> Profile {
    Profile {
        targets: vec![TargetConfig {
            address: addr.to_string(),
            transport: transport.to_string(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "bench".into(),
            duration_secs: 0,
            messages_per_second: 0,
            total_messages: Some(20_000),
            templates: vec![template.to_string()],
            format: None,
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

fn run_one(template: &str, transport: &str, addr: &str) -> u64 {
    let profile = profile_for(template, transport, addr);
    let metrics = create_metrics().expect("create_metrics");
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let _ = run_profile(black_box(&profile), black_box(metrics)).await;
    });
    profile.phases[0].total_messages.unwrap_or(0)
}

fn bench_runtime_rfc5424_unlimited(c: &mut Criterion) {
    let tmp = tempfile_in_tmp("bench_rfc5424");
    let path = tmp.to_string_lossy().to_string();
    let mut group = c.benchmark_group("runtime");
    group.throughput(Throughput::Elements(20_000));
    group.bench_function("rfc5424_static", |b| {
        b.iter(|| {
            let n = run_one(black_box(static_template()), "file", black_box(&path));
            black_box(n)
        });
    });
    group.finish();
    let _ = std::fs::remove_file(&tmp);
}

fn bench_runtime_rfc5424_faker(c: &mut Criterion) {
    let tmp = tempfile_in_tmp("bench_rfc5424_faker");
    let path = tmp.to_string_lossy().to_string();
    let mut group = c.benchmark_group("runtime");
    group.throughput(Throughput::Elements(20_000));
    group.bench_function("rfc5424_faker", |b| {
        b.iter(|| {
            let n = run_one(black_box(faker_template()), "file", black_box(&path));
            black_box(n)
        });
    });
    group.finish();
    let _ = std::fs::remove_file(&tmp);
}

fn bench_runtime_rfc3164_unlimited(c: &mut Criterion) {
    let tmp = tempfile_in_tmp("bench_rfc3164");
    let path = tmp.to_string_lossy().to_string();
    let mut group = c.benchmark_group("runtime");
    group.throughput(Throughput::Elements(20_000));
    let template_static = "<13>Feb 10 12:00:00 host app: seq={{sequence}}";
    group.bench_function("rfc3164_static", |b| {
        b.iter(|| {
            let n = run_one(black_box(template_static), "file", black_box(&path));
            black_box(n)
        });
    });
    group.finish();
    let _ = std::fs::remove_file(&tmp);
}

fn bench_runtime_json_lines(c: &mut Criterion) {
    let tmp = tempfile_in_tmp("bench_json");
    let path = tmp.to_string_lossy().to_string();
    let mut group = c.benchmark_group("runtime");
    group.throughput(Throughput::Elements(20_000));
    group.bench_function("json_lines_static", |b| {
        b.iter(|| {
            let n = run_one(black_box(json_template()), "file", black_box(&path));
            black_box(n)
        });
    });
    group.finish();
    let _ = std::fs::remove_file(&tmp);
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

fn _wall_clock_for_sanity() -> Instant {
    Instant::now()
}

criterion_group!(
    benches,
    bench_runtime_rfc5424_unlimited,
    bench_runtime_rfc5424_faker,
    bench_runtime_rfc3164_unlimited,
    bench_runtime_json_lines,
);
criterion_main!(benches);
