# PLAN: v10.x — Веха E «Зрелость» (P2)

> Статус: **начало вехи E**. v9.0.0 (закрытие вехи D) уже выпущен.
> Все P0+P1 задачи AUDIT.md §4 выполнены. Теперь реализуем P2:
> новые форматы, транспорты, сценарии аномалий, Docker.

Дата: 2026-07-13. Цель: v9.1.0 (N10) → v9.2.0 (F15) → v9.3.0 (F16) → v9.4.0 (F17) → v9.5.0 (N12) → v9.6.0 (cipher_policy).

---

## 1. Полная инвентаризация вехи E (P2)

| Задача | Где в коде | План |
|--------|------------|------|
| **N10 (полная)** | `src/format/{rfc5424,rfc3164,raw,protobuf}.rs` имеют `build()`. `src/transport/{file,tcp,udp,tls}.rs` — `target_sender_*`. `src/observability/` (metrics + server). `src/generator/` (run_profile, run_phase_multi, generate_message, profile config). | Создать trait `Format` (метод `render`) + `Transport` (метод `run`) с dyn-dispatch через enum. Без breaking changes — текущие функции оставлены, добавляются trait. |
| **F15 (CEF/LEEF/JSON-lines)** | Не реализован. | Создать `src/format/cef.rs`, `leef.rs`, `json_lines.rs` с реализацией trait `Format`. |
| **F16 (Kafka/Redpanda + файловая ротация + reconnect-стратегия)** | `src/transport/{file,tcp,udp,tls}.rs` — sender'ы. Нет Kafka. | Добавить `src/transport/kafka.rs` (через `rdkafka` crate — клиент librdkafka). Добавить `src/transport/file_rotation.rs` (ротация по size/time). Улучшить reconnect-стратегию в tcp/tls (exponential backoff). |
| **F17 (сценарии аномалий)** | Не реализован. | Добавить `phases.anomalies: Option<Vec<Anomaly>>` в `Phase` — burst-injection, slow_drip, packet_loss симуляция. Интеграционные тесты. |
| **N12 (Docker/musl/docker-compose)** | Не реализован. | `Dockerfile` (multi-stage: rust:1.97 → distroless/cc-debian12), `docker-compose.yml` (generator + rsyslog/syslog-ng + prometheus + grafana). |
| **N4.cipher_policy (отложено из P1)** | Не реализован. | Нужен `openssl` или `rustls` crate. Добавить `tls_cipher_suites: Option<Vec<String>>` в TargetConfig. Передать через `SslContextBuilder::set_cipher_list`. |

---

## 2. План релизов

| Релиз | Тип | Что | Зависит от |
|-------|-----|-----|------------|
| **v9.1.0** | minor | **N10 (полная)**: trait `Format` (dyn-dispatch) + trait `Transport` (dyn-dispatch). Без breaking changes — добавляются как новая инфраструктура. | — |
| **v9.2.0** | minor | **F15**: CEF + LEEF + JSON-lines форматы через trait `Format`. | v9.1.0 (нужен trait) |
| **v9.3.0** | minor | **F16**: Kafka transport (через `rdkafka` или нативный TCP fallback) + файловая ротация + reconnect-стратегия (exponential backoff) | v9.1.0 (нужен trait) |
| **v9.4.0** | minor | **F17**: сценарии аномалий (`phases.anomalies: burst/slow_drip/packet_loss`) + интеграционные тесты | — |
| **v9.5.0** | minor | **N4.cipher_policy**: добавление `tls_cipher_suites: Option<Vec<String>>` через `openssl` crate. `SslContextBuilder::set_cipher_list` для TLS-сервера/клиента. | — |
| **v9.6.0** | minor | **N12 (Docker)**: `Dockerfile` (multi-stage: rust:1.97 → distroless/cc-debian12), `docker-compose.yml` (generator + rsyslog + prometheus + grafana) | — |

---

## 3. Детальный план каждого релиза

### 3.1 v9.1.0 (N10 — trait Format/Transport полная)

Цель: сделать `Format` и `Transport` настоящими trait'ами, а текущие реализации — конкретные impl'ы.

`src/format/mod.rs`:
```rust
pub trait Format: Send + Sync {
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>;
    fn name(&self) -> &'static str;
}

pub enum FormatKind {
    Rfc5424,
    Rfc3164,
    Raw,
    Protobuf,
    // v9.2.0+: Cef, Leef, JsonLines
}

impl Format for FormatKind {
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8> {
        match self {
            Self::Rfc5424 => rfc5424::build(h, msg),
            Self::Rfc3164 => rfc3164::build(h, msg),
            Self::Raw => raw::build(h, msg),
            Self::Protobuf => protobuf::serialize_protobuf(&parse_schema(&h.protobuf_schema), ...),
        }
    }
    fn name(&self) -> &'static str { match self { ... } }
}
```

`src/transport/mod.rs`:
```rust
#[async_trait]
pub trait Transport: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, rx: SharedRx, metrics: Metrics, shutdown: CancellationToken) -> Result<()>;
}

pub enum TransportKind {
    File(FileConfig),
    Tcp(TcpConfig),
    Udp(UdpConfig),
    Tls(TlsConfig),
    // v9.3.0+: Kafka(KafkaConfig)
}

#[async_trait]
impl Transport for TransportKind { ... }
```

Backward compat: текущие `target_sender_file`/`_tcp`/`_udp`/`_tls` остаются (используются в `core::run_phase_multi` через match). Новые `FormatKind`/`TransportKind` — это API для F15/F16, не ломают существующий код.

### 3.2 v9.2.0 (F15 — CEF/LEEF/JSON-lines)

- `src/format/cef.rs`: ArcSight Common Event Format
  - `CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension`
  - Extensions — key=value пары, escaping `\`, `=`
- `src/format/leef.rs`: IBM QRadar LEEF
  - `LEEF:Version|Vendor|Product|Version|EventID|...|key=value\n`
- `src/format/json_lines.rs`: `{"ts":"...","level":"...","msg":"..."}\n`
- Каждый через `impl Format for FormatKind::*`
- Тесты: round-trip парсинг каждого формата, faker поля (ip, host, user)

### 3.3 v9.3.0 (F16 — Kafka/Redpanda/файловая ротация/reconnect)

- `Cargo.toml`: добавить `rdkafka = "0.36"` (если Kafka через librdkafka) ИЛИ нативный TCP fallback (просто TCP к Kafka без librdkafka — не рекомендуется)
- `src/transport/kafka.rs`: target_sender_kafka с rdkafka Producer
- `src/transport/file_rotation.rs`: target_sender_file с rotation по size/time (rotate_size_mb, rotate_interval_secs)
- `src/transport/tcp.rs` + `tls.rs`: добавить exponential backoff в reconnect (текущий код делает 1 попытку)
- 3 новых transport-вида

### 3.4 v9.4.0 (F17 — сценарии аномалий)

- `src/config.rs::Phase`: добавить `anomalies: Option<Vec<Anomaly>>`
- `src/config.rs::Anomaly { kind: AnomalyKind, params: HashMap<String, Value> }`
- `AnomalyKind`: BurstInjection { rate_multiplier, interval_secs }, SlowDrip { rate_divisor, duration_secs }, PacketLoss { loss_percent }
- `src/generator/core.rs::run_phase_multi`: применять anomalies в loop (multiplier messages_per_second, sleep между батчами, drop messages)
- Интеграционные тесты: target throughput > config, target throughput < config, messages dropped = expected

### 3.5 v9.5.0 (N4.cipher_policy)

- `Cargo.toml`: добавить `openssl = "0.10"` (нужен для кастомных cipher lists на Linux/macOS)
- `src/config.rs::TargetConfig`: `tls_cipher_suites: Option<Vec<String>>` (например `["TLS_AES_256_GCM_SHA384", "TLS_CHACHA20_POLY1305_SHA256"]`)
- `src/transport/tls.rs::build_tls_connector`: если `cipher_suites` заданы, парсим через `openssl::ssl::SslContextBuilder::set_cipher_list`
- Тест: `--cipher-suites` flag + handshake с конкретным cipher

### 3.6 v9.6.0 (N12 — Docker)

- `Dockerfile` (multi-stage):
  ```dockerfile
  FROM rust:1.97-bookworm AS builder
  WORKDIR /app
  COPY . .
  RUN cargo build --release --bin syslog-generator
  
  FROM gcr.io/distroless/cc-debian12
  COPY --from=builder /app/target/release/syslog-generator /usr/local/bin/
  ENTRYPOINT ["/usr/local/bin/syslog-generator"]
  ```
- `docker-compose.yml`:
  ```yaml
  services:
    syslog-generator:
      build: .
      command: --profile /etc/syslog-generator/profile.yaml
      volumes: ["./examples:/examples:ro"]
    
    rsyslog:
      image: rsyslog/rsyslog:latest
      ports: ["514:514"]
    
    prometheus:
      image: prom/prometheus:latest
      ports: ["9090:9090"]
    
    grafana:
      image: grafana/grafana:latest
      ports: ["3000:3000"]
  ```
- `examples/profile-docker.yaml`: пример профиля для Docker-окружения
- Интеграция с .github/workflows/ci.yml: build Docker image + push в ghcr.io

---

## 4. Критерии приёмки (для каждого релиза)

1. ✅ Все ранее зелёные тесты остаются зелёными (никаких регрессий)
2. ✅ Новые тесты добавлены для новой функциональности
3. ✅ `cargo fmt --all -- --check` clean
4. ✅ `cargo clippy --all-targets -- -D warnings` clean
5. ✅ `cargo build --release` успех
6. ✅ `cargo test --locked` все зелёные
7. ✅ `cargo bench --no-run --locked` успех
8. ✅ `cargo bench --quick` 9/9 Success
9. ✅ Live-проверка бинарника: `./target/release/syslog-generator --version` показывает 8.7.0+
10. ✅ Уборка: `target/` удалён, zip удалён
11. ✅ Gitflow: feature → dev → release → main → tag → push
12. ✅ CI: все 3 job'а зелёные на GitHub Actions
13. ✅ Документация: README, CHANGELOG, CLAUDE_HANDOFF, AUDIT обновлены
14. ✅ Архив в `.archived-releases/` сохранён (НЕ в git)
15. ✅ feature/release ветки НЕ удаляются (по требованию)

---

## 5. Roadmap (после v9.6.0)

После завершения вехи E (v9.6.0) следующий этап — **v10.0.0** major release
(полная milestone вехи E, может быть с breaking changes если потребуется
для новых архитектурных решений).
