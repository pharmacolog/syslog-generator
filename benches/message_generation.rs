use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use syslog_generator::{create_dispatcher, generate_message, render_template, Phase, TargetConfig};

fn bench_template_render(c: &mut Criterion) {
    let mut values = HashMap::new();
    values.insert("real_app".to_string(), "authsvc".to_string());
    values.insert("pid".to_string(), "1000".to_string());
    values.insert("real_user".to_string(), "alice".to_string());
    values.insert("random_ip".to_string(), "192.0.2.10".to_string());
    values.insert("real_command".to_string(), "echo ok".to_string());
    values.insert("sequence".to_string(), "123".to_string());
    let template =
        "{{real_app}}[{{pid}}]: login user={{real_user}} src={{random_ip}} cmd={{real_command}} seq={{sequence}}";
    c.bench_function("template_render_realistic", |b| {
        b.iter(|| {
            let msg = render_template(black_box(template), black_box(&values));
            black_box(msg)
        })
    });
}

fn bench_generate_message_template(c: &mut Criterion) {
    let phase = Phase {
        name: "bench".to_string(),
        duration_secs: 1,
        messages_per_second: 1000,
        format: Some("rfc5424".to_string()),
        templates: vec![
            "{{real_app}}: login for {{faker.username}} from {{faker.ipv4}} seq={{sequence}}"
                .to_string(),
        ],
        ..Default::default()
    };
    c.bench_function("generate_message_from_template", |b| {
        let mut seq = 0usize;
        b.iter(|| {
            seq += 1;
            let msg = generate_message(black_box(&phase), black_box(seq)).unwrap();
            black_box(msg)
        })
    });
}

fn bench_dispatcher_weighted(c: &mut Criterion) {
    let targets = vec![
        TargetConfig {
            address: "a".into(),
            transport: "file".into(),
            ..Default::default()
        },
        TargetConfig {
            address: "b".into(),
            transport: "tcp".into(),
            weight: 3,
            ..Default::default()
        },
        TargetConfig {
            address: "c".into(),
            transport: "udp".into(),
            weight: 2,
            ..Default::default()
        },
    ];
    c.bench_function("create_dispatcher_weighted", |b| {
        b.iter(|| {
            let plan = create_dispatcher(black_box(&targets), black_box("weighted"));
            black_box(plan)
        })
    });
}

criterion_group!(
    benches,
    bench_template_render,
    bench_generate_message_template,
    bench_dispatcher_weighted
);
criterion_main!(benches);
