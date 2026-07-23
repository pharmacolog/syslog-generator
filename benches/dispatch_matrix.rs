//! [A0] Dispatch matrix bench — round-robin / weighted / broadcast на разном числе targets.
//!
//! Использует `target_sender_file` (наиболее стабильный для bench) для разных
//! конфигураций распределения. Каждый iteration создаёт N временных файлов,
//! запускает run_profile, удаляет их.
//!
//! Примеры:
//!     cargo bench --bench dispatch_matrix
//!     cargo bench --bench dispatch_matrix -- --quick

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::path::PathBuf;
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::runtime::Runtime;

fn make_paths(n: usize) -> Vec<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    (0..n)
        .map(|i| {
            let mut p = std::env::temp_dir();
            p.push(format!("sg_a0_dispatch_{nanos}_{i}.log"));
            p
        })
        .collect()
}

fn make_profile(paths: Vec<PathBuf>, distribution: &str, weights: Vec<usize>) -> Profile {
    let targets = paths
        .into_iter()
        .enumerate()
        .map(|(i, p)| TargetConfig {
            address: p.to_string_lossy().to_string(),
            transport: "file".into(),
            weight: weights.get(i).copied().unwrap_or(1),
            ..Default::default()
        })
        .collect();
    Profile {
        targets,
        distribution: distribution.to_string(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "bench".into(),
            messages_per_second: 0,
            total_messages: Some(10_000),
            templates: vec!["seq={{sequence}}".to_string()],
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

fn cleanup(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

fn bench_one(
    c: &mut Criterion,
    name: &str,
    distribution: &str,
    n_targets: usize,
    weights: Vec<usize>,
) {
    let rt = Runtime::new().unwrap();
    let distribution_owned = distribution.to_string();
    let mut group = c.benchmark_group("dispatch_matrix");
    group.throughput(Throughput::Elements(10_000));
    group.bench_with_input(
        BenchmarkId::from_parameter(name),
        &n_targets,
        move |b, _| {
            let distribution = distribution_owned.clone();
            let weights = weights.clone();
            b.to_async(&rt).iter(move || {
                let distribution = distribution.clone();
                let weights = weights.clone();
                async move {
                    let paths = make_paths(n_targets);
                    let profile = make_profile(paths.clone(), &distribution, weights);
                    let _ = run_profile(
                        black_box(&profile),
                        black_box(create_metrics().expect("metrics")),
                    )
                    .await;
                    cleanup(&paths);
                }
            });
        },
    );
    group.finish();
}

fn bench_round_robin(c: &mut Criterion) {
    for n in [1usize, 4, 16] {
        bench_one(c, &format!("rr_{n}"), "round-robin", n, vec![1; n]);
    }
}

fn bench_weighted(c: &mut Criterion) {
    for n in [1usize, 4, 16] {
        bench_one(c, &format!("weighted_{n}"), "weighted", n, vec![1; n]);
    }
}

fn bench_broadcast(c: &mut Criterion) {
    for n in [1usize, 4, 16] {
        bench_one(c, &format!("broadcast_{n}"), "broadcast", n, vec![1; n]);
    }
}

criterion_group!(benches, bench_round_robin, bench_weighted, bench_broadcast);
criterion_main!(benches);
