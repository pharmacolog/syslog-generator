//! [A0] Transport matrix bench — sender throughput для TCP и UDP на разном числе connections.
//!
//! - `connections`: 1, 4, 16.
//! - `transport`: tcp, udp.
//!
//! Примеры:
//!     cargo bench --bench transport_matrix
//!     cargo bench --bench transport_matrix -- --quick

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;

fn make_profile(target: TargetConfig, mps: u64, total: u64) -> Profile {
    Profile {
        targets: vec![target],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "bench".into(),
            messages_per_second: mps,
            total_messages: Some(total),
            templates: vec!["<13>Feb 10 12:00:00 host app: seq={{sequence}}".to_string()],
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

fn bench_tcp_connections(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("transport_matrix_tcp");
    for conns in [1usize, 4, 16] {
        let total = 5000u64;
        group.throughput(Throughput::Elements(total));
        group.bench_with_input(BenchmarkId::from_parameter(conns), &conns, |b, &conns| {
            b.to_async(&rt).iter(|| async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap().to_string();
                let server = tokio::spawn(async move {
                    let (s, _) = listener.accept().await.unwrap();
                    let mut buf = [0u8; 4096];
                    loop {
                        match s.try_read(&mut buf) {
                            Ok(0) => break,
                            Ok(_) => continue,
                            Err(_) => {
                                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                                continue;
                            }
                        }
                    }
                });
                let profile = make_profile(
                    TargetConfig {
                        address: addr,
                        transport: "tcp".into(),
                        connections: conns,
                        ..Default::default()
                    },
                    0,
                    total,
                );
                let _ = run_profile(
                    black_box(&profile),
                    black_box(create_metrics().expect("metrics")),
                )
                .await;
                drop(server);
            });
        });
    }
    group.finish();
}

fn bench_udp_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("transport_matrix_udp");
    for conns in [1usize, 4, 16] {
        let total = 5000u64;
        group.throughput(Throughput::Elements(total));
        group.bench_with_input(BenchmarkId::from_parameter(conns), &conns, |b, &conns| {
            b.to_async(&rt).iter(|| async move {
                use std::net::UdpSocket;
                let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
                let addr = sock.local_addr().unwrap().to_string();
                drop(sock);
                let profile = make_profile(
                    TargetConfig {
                        address: addr,
                        transport: "udp".into(),
                        connections: conns,
                        ..Default::default()
                    },
                    0,
                    total,
                );
                let _ = run_profile(
                    black_box(&profile),
                    black_box(create_metrics().expect("metrics")),
                )
                .await;
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tcp_connections, bench_udp_throughput);
criterion_main!(benches);
