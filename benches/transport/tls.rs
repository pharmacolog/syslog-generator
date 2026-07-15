//! PR-6 (v10.7.8): bench TLS (rustls 0.23) setup overhead.
//!
//! До PR-6 benches не покрывали TLS — был только TCP/UDP в sender_throughput.
//! v9.5.0 (N4.cipher_policy) сделал BREAKING миграцию native-tls → rustls.
//! Реальный throughput TLS-канала зависит от handshake, но для hot-path
//! критичен overhead `build_tls_connector` (per-target setup, не per-msg).
//!
//! Реальный TLS handshake бенчится через integration tests
//! (`tests/integration_tests.rs::test_n4_cipher_policy_e2e_tls_handshake`).

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use syslog_generator::transport::tls::{TlsParams, TlsVersion};

fn bench_tls_build_connector_insecure(c: &mut Criterion) {
    c.bench_function("tls_build_connector_insecure", |b| {
        b.iter(|| {
            let params = TlsParams {
                domain: "localhost".to_string(),
                ca_pem: None,
                insecure: true, // skip verification (быстрее)
                client_cert_pem: None,
                client_key_pem: None,
                min_protocol: Some(TlsVersion::Tls12),
                cipher_suites: None,
            };
            let _ = black_box(syslog_generator::transport::build_tls_connector(&params));
        });
    });
}

fn bench_tls_build_connector_with_min_version(c: &mut Criterion) {
    c.bench_function("tls_build_connector_tls13", |b| {
        b.iter(|| {
            let params = TlsParams {
                domain: "localhost".to_string(),
                ca_pem: None,
                insecure: false,
                client_cert_pem: None,
                client_key_pem: None,
                min_protocol: Some(TlsVersion::Tls13),
                cipher_suites: None,
            };
            // CA-less with secure=true → may error, but we measure build path.
            let _ = black_box(syslog_generator::transport::build_tls_connector(&params));
        });
    });
}

criterion_group!(
    benches,
    bench_tls_build_connector_insecure,
    bench_tls_build_connector_with_min_version
);
criterion_main!(benches);
