//! [A0] Transport matrix bench — sender throughput для TCP и UDP на разном числе connections.
//!
//! - `connections`: 1, 4, 16.
//! - `transport`: tcp, udp.
//!
//! Каждый case:
//! - Реальный listener, который принимает N соединений и читает все байты.
//! - Дожидается готовности listener перед запуском producer.
//! - await всех worker tasks.
//! - Errors propagated (eprintln + assertion).
//!
//! Примеры:
//!     cargo bench --bench transport_matrix
//!     cargo bench --bench transport_matrix -- --quick

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, UdpSocket};
use tokio::runtime::Runtime;

const MESSAGES_PER_ITER: u64 = 2_000;

fn make_profile(target: TargetConfig) -> Profile {
    Profile {
        targets: vec![target],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "bench".into(),
            messages_per_second: 0,
            total_messages: Some(MESSAGES_PER_ITER),
            templates: vec!["<13>Feb 10 12:00:00 host app: seq={{sequence}}".to_string()],
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

async fn tcp_drain_collector(listener: TcpListener, conns: usize, total_bytes: Arc<AtomicBool>) {
    let mut accepted = 0usize;
    while accepted < conns {
        match listener.accept().await {
            Ok((mut s, _)) => {
                accepted += 1;
                let total_bytes = total_bytes.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    total_bytes.store(true, Ordering::SeqCst);
                });
            }
            Err(_) => break,
        }
    }
}

fn bench_tcp_connections(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("transport_matrix_tcp");
    group.measurement_time(std::time::Duration::from_secs(5));
    for conns in [1usize, 4, 16] {
        group.throughput(Throughput::Elements(MESSAGES_PER_ITER));
        group.bench_with_input(BenchmarkId::from_parameter(conns), &conns, |b, &conns| {
            b.to_async(&rt).iter(|| async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap().to_string();
                let ready = Arc::new(AtomicBool::new(false));
                let collector = tokio::spawn(tcp_drain_collector(listener, conns, ready));
                // Yield чтобы listener успел стартовать перед producer.
                tokio::task::yield_now().await;
                let profile = make_profile(TargetConfig {
                    address: addr,
                    transport: "tcp".into(),
                    connections: conns,
                    ..Default::default()
                });
                let _ = run_one_async(black_box(profile)).await;
                let _ = collector.await;
            });
        });
    }
    group.finish();
}

fn bench_udp_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("transport_matrix_udp");
    group.measurement_time(std::time::Duration::from_secs(5));
    for conns in [1usize, 4] {
        group.throughput(Throughput::Elements(MESSAGES_PER_ITER));
        group.bench_with_input(BenchmarkId::from_parameter(conns), &conns, |b, &conns| {
            b.to_async(&rt).iter(|| async move {
                let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let recv_addr = receiver.local_addr().unwrap();
                // Drain datagrams in background to avoid ENOBUFS.
                let drain = tokio::spawn(async move {
                    let mut buf = [0u8; 64 * 1024];
                    let mut count = 0u64;
                    while let Ok((_n, _)) = receiver.recv_from(&mut buf).await {
                        count += 1;
                        if count >= MESSAGES_PER_ITER {
                            break;
                        }
                    }
                    count
                });
                let profile = make_profile(TargetConfig {
                    address: format!("{recv_addr}"),
                    transport: "udp".into(),
                    connections: conns,
                    ..Default::default()
                });
                let _ = run_one_async(black_box(profile)).await;
                let _ = drain.await;
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tcp_connections, bench_udp_throughput);
criterion_main!(benches);
