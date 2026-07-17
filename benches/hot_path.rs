//! PR-9 (perf baseline): hot-path bench для per-message overhead.
//!
//! Этот bench фокусируется на per-message cost (в микросекундах),
//! в отличие от `sender_throughput` который измеряет end-to-end throughput.
//!
//! Целевой показатель PR-10: ≤ 2 µs/msg на generate_message_from_template.
//!
//! Использование:
//!     cargo bench --bench hot_path -- --quick

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::collections::HashMap;
use std::hint::black_box;
use syslog_generator::{
    format::FormatKind, generate_message_with_format_cached, load_profile_from_yaml_str,
    PhaseContext,
};

const PROFILE_YAML: &str = r#"
targets:
  - address: /tmp/syslog-gen-bench.log
    transport: file
distribution: round-robin
phases:
  - name: bench
    duration_secs: 0
    total_messages: 100000
    messages_per_second: 0
    templates:
      - "<165>1 {{timestamp}} {{hostname}} {{real_app}}[{{pid}}]: user {{faker.username}} from {{faker.ipv4}} login seq={{sequence}}"
    syslog:
      facility: 16
      severity: 6
"#;

/// Benchmark: per-message generation overhead.
fn bench_generate_message_per_msg(c: &mut Criterion) {
    let profile = load_profile_from_yaml_str(PROFILE_YAML).expect("profile parses");
    let phase = profile.phases.first().expect("phase exists").clone();
    let ctx = PhaseContext::resolve(&phase).expect("ctx resolves");
    let format_kind =
        FormatKind::parse(phase.format.as_deref().unwrap_or("rfc5424")).expect("format parses");
    let metrics = syslog_generator::create_metrics().expect("metrics ok");

    let mut group = c.benchmark_group("hot_path");
    group.throughput(Throughput::Elements(1));

    group.bench_function("rfc5424_with_faker", |b| {
        // PR-17b: hot-path использует `_cached` API с caller-owned HashMap.
        // Устраняет heap allocation per message (~80-150 ns/msg savings).
        let mut values = HashMap::with_capacity(16);
        b.iter(|| {
            let msg = generate_message_with_format_cached(
                black_box(&ctx),
                black_box(&phase),
                black_box(&format_kind),
                black_box(1),
                black_box(&mut values),
            )
            .expect("generate ok");
            metrics
                .messages_generated_total
                .with_label_values(&["bench"])
                .inc();
            black_box(msg);
        });
    });

    group.finish();
}

/// Benchmark: just template render + faker (no format).
fn bench_template_render_only(c: &mut Criterion) {
    use std::collections::HashMap;
    use syslog_generator::template::CompiledTemplate;

    let tpl = CompiledTemplate::compile(
        "user {{faker.username}} from {{faker.ipv4}} login seq={{sequence}}",
    );
    let mut values = HashMap::new();
    values.insert("faker.username".to_string(), "alice".to_string());
    values.insert("faker.ipv4".to_string(), "192.168.1.10".to_string());
    values.insert("sequence".to_string(), "42".to_string());

    c.bench_function("template_render_only", |b| {
        b.iter(|| {
            let out = tpl.render(black_box(&values));
            black_box(out);
        });
    });
}

/// Benchmark: faker overhead (single token generation).
fn bench_faker_overhead(c: &mut Criterion) {
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    let mut rng = StdRng::seed_from_u64(42);

    c.bench_function("faker_ipv4", |b| {
        b.iter(|| {
            let s = syslog_generator::faker("ipv4", black_box(&mut rng));
            black_box(s);
        });
    });

    c.bench_function("faker_uuid", |b| {
        b.iter(|| {
            let s = syslog_generator::faker("uuid", black_box(&mut rng));
            black_box(s);
        });
    });

    c.bench_function("faker_username", |b| {
        b.iter(|| {
            let s = syslog_generator::faker("username", black_box(&mut rng));
            black_box(s);
        });
    });
}

criterion_group!(
    benches,
    bench_generate_message_per_msg,
    bench_template_render_only,
    bench_faker_overhead
);
criterion_main!(benches);
