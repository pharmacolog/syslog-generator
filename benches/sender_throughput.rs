use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use syslog_generator::{create_metrics, run_profile, Phase, Profile, ShutdownConfig, TargetConfig};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, UdpSocket};
use tokio::runtime::Runtime;

fn make_profile(target: TargetConfig, mps: u64, name: &str, total_messages: u64) -> Profile {
    Profile {
        targets: vec![target],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: name.into(),
            messages_per_second: mps,
            total_messages: Some(total_messages),
            templates: vec!["seq={{sequence}}".to_string()],
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

fn bench_tcp_sender_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("tcp_sender_throughput");
    for count in [10u64, 50, 100] {
        group.throughput(Throughput::Elements(count));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.to_async(&rt).iter(|| async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap().to_string();
                let server = tokio::spawn(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut buf = [0u8; 4096];
                    let mut total = 0usize;
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => total += n,
                            Err(_) => break,
                        }
                    }
                    total
                });
                let profile = make_profile(
                    TargetConfig {
                        address: addr,
                        transport: "tcp".into(),
                        ..Default::default()
                    },
                    count,
                    "tcp_bench",
                    count,
                );
                run_profile(
                    &profile,
                    create_metrics().expect("create_metrics ok in bench"),
                )
                .await
                .unwrap();
                let _ = server.await;
            });
        });
    }
    group.finish();
}

fn bench_udp_sender_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("udp_sender_throughput");
    for count in [10u64, 50, 100] {
        group.throughput(Throughput::Elements(count));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.to_async(&rt).iter(|| async move {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let addr = socket.local_addr().unwrap().to_string();
                let profile = make_profile(
                    TargetConfig {
                        address: addr,
                        transport: "udp".into(),
                        ..Default::default()
                    },
                    count,
                    "udp_bench",
                    count,
                );
                run_profile(
                    &profile,
                    create_metrics().expect("create_metrics ok in bench"),
                )
                .await
                .unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_tcp_sender_throughput,
    bench_udp_sender_throughput
);
criterion_main!(benches);
