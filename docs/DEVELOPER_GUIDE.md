# DEVELOPER GUIDE

Версия документа: `v10.7.4`.

Это руководство для разработчиков, расширяющих `syslog-generator`. Документ
описывает архитектуру, слои, точки расширения и процесс добавления новых
форматов, транспортов и аномалий. Все ссылки на исходники — актуальные
на v10.7.4.

---

## 1. Структура проекта

```
syslog-generator/
├── src/                          # ~11,640 LOC на v10.7.4
│   ├── main.rs                   # точка входа; ExitCode; CLI dispatch
│   ├── lib.rs                    # pub use реэкспорты + backward-compat алиасы
│   ├── cli.rs                    # clap derive Args + subcommands (v10.6+)
│   ├── error.rs                  # N7 типизированные ошибки (RuntimeError и подтипы)
│   ├── validate.rs               # F13 семантическая валидация (44 варианта)
│   ├── schema_check.rs           # D3 структурная JSON Schema валидация
│   ├── format/                   # N10 слой форматов
│   │   ├── mod.rs                # Format trait, FormatKind enum, Header, FormatContext
│   │   ├── rfc5424.rs            # RFC 5424
│   │   ├── rfc3164.rs            # RFC 3164 (BSD)
│   │   ├── raw.rs                # passthrough
│   │   ├── protobuf.rs           # честный wire-format (F10) — canonical source
│   │   ├── cef.rs                # F15 ArcSight CEF
│   │   ├── leef.rs               # F15 IBM QRadar LEEF
│   │   └── json_lines.rs         # F15 NDJSON
│   ├── transport/                # N10 слой транспортов
│   │   ├── mod.rs                # Transport trait + TransportKind enum, SharedRx, Framing
│   │   ├── file.rs               # target_sender_file + rotation (F16)
│   │   ├── tcp.rs                # target_sender_tcp (BytesMut, N6)
│   │   ├── udp.rs                # target_sender_udp (zero-copy)
│   │   ├── tls.rs                # target_sender_tls + build_tls_connector (rustls 0.23)
│   │   ├── kafka.rs              # F16 Kafka producer (feature=kafka)
│   │   └── reconnect.rs          # F16 exponential backoff
│   ├── generator/                # N10 оркестрация
│   │   ├── mod.rs
│   │   ├── core.rs               # run_profile, run_phase_multi, generate_message
│   │   └── config.rs             # Profile, Phase, TargetConfig, все конфиг-структуры
│   ├── observability/            # N10 Prometheus + HTTP
│   │   ├── mod.rs
│   │   ├── metrics.rs            # Metrics, create_metrics, gather_metrics (24 метрики)
│   │   └── server.rs             # HTTP /metrics endpoint
│   ├── payload.rs                # F4-F6, F14: faker, regex, корреляции, distribute
│   ├── payload_proptests.rs      # N8 proptest (#[cfg(test)])
│   ├── template.rs               # N5 CompiledTemplate (one-pass parsing)
│   ├── schema.rs                 # F5 schema-per-phase загрузка
│   ├── load_shape.rs             # F3 профили нагрузки (constant/linear/sine/burst)
│   ├── anomaly.rs                # F17 AnomalyKind + AnomalyPlanner
│   ├── shutdown.rs               # N7 graceful drain + SIGINT/SIGTERM handler (PR-2)
│   ├── protobuf.rs               # Backward-compat re-export → format::protobuf (PR-1)
│   ├── syslog.rs                 # Backward-compat re-export → format::*
│   ├── sender.rs                 # Backward-compat re-export → transport::*
│   ├── core.rs                   # Backward-compat re-export → generator::*
│   ├── config.rs                 # Backward-compat re-export → generator::config
│   ├── metrics.rs                # Backward-compat re-export → observability::metrics
│   └── metrics_server.rs         # Backward-compat re-export → observability::server
│
├── tests/
│   ├── integration_tests.rs      # 86 e2e тестов (mixed-target, F12/N4, F16/F17, F15)
│   └── n7_runtime_errors.rs      # 11 тестов типизированных ошибок (N7)
│
├── benches/
│   ├── message_generation.rs     # Criterion: template render, generate_message, dispatcher
│   └── sender_throughput.rs      # Criterion: TCP/UDP throughput
│
├── fuzz/                         # cargo-fuzz (v10.4.0)
│   ├── Cargo.toml
│   └── fuzz_targets/
│       ├── profile_parser.rs
│       ├── format_rfc5424.rs
│       ├── format_cef.rs
│       ├── format_leef.rs
│       └── format_json_lines.rs
│
├── schemas/
│   └── profile.schema.json       # формальная JSON Schema (D3)
│
├── examples/                     # 41 пример профилей
│   ├── README.md
│   └── *.json / *.yaml / *.yml
│
├── docs/
│   ├── USER_GUIDE.md             # полное руководство пользователя
│   ├── DEVELOPER_GUIDE.md        # ← этот файл
│   ├── COVERAGE.md               # отчёты cargo-llvm-cov
│   └── FUZZING.md                # инструкции по cargo-fuzz
│
├── dashboards/
│   └── grafana.json              # Grafana dashboard (синхронизирован с N2)
│
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                # fmt → clippy → build → test → bench → coverage → docker
│   │   └── docker.yml            # multi-arch build + push в ghcr.io
│   ├── dependabot.yml            # Dependabot (v10.5.0)
│   └── workflows/msrv/           # rust-toolchain.toml для MSRV-check
│
├── docker/                       # N12 docker-compose stack
│   ├── syslog-ng.conf
│   └── prometheus.yml
│
├── Dockerfile                    # multi-stage (distroless, ~25 MB)
├── docker-compose.yml
├── Cargo.toml                    # v10.7.4, MSRV 1.95, features: kafka, test-helpers
├── Cargo.lock
├── rust-toolchain.toml           # MSRV pin (v10.5.0)
├── CHANGELOG.md                  # все релизы от v7.4.0 до v10.7.4
├── AUDIT.md                      # реестр задач (базис v10.7.2)
├── CLAUDE_HANDOFF.md             # перенос контекста для Claude
├── PLAN-v10.0.0.md               # план вехи F (закрыта на v10.7.1)
├── PLAN-веха-E.md                # история вехи E (закрыта)
└── README.md
```

---

## 2. Архитектура слоёв (N10, v8.8.0)

Слои (от внешнего к внутреннему), с явным разделением ответственности:

```
┌─────────────────────────────────────────────────┐
│ main.rs / cli.rs                               │  ← entrypoint
└────────────────┬────────────────────────────────┘
                 │
┌────────────────▼────────────────────────────────┐
│ generator/                                      │  ← orchestrator
│   ├── core.rs (run_profile, run_phase_multi)   │
│   └── config.rs (Profile, Phase, TargetConfig) │
└─┬──────────┬──────────┬──────────┬──────────────┘
  │          │          │          │
  ▼          ▼          ▼          ▼
┌────┐  ┌────────┐  ┌────────┐  ┌─────────────┐
│fmt │  │transprt│  │paylod  │  │observability│
│    │  │        │  │        │  │             │
│ F  │  │  T     │  │ P      │  │   O         │
└────┘  └────────┘  └────────┘  └─────────────┘
   ▲          ▲          ▲          ▲
   └──────────┴──────────┴──────────┘
            │ metrics::Metrics
            ▼
       ┌─────────┐
       │ error.rs│  ← shared типизированные ошибки
       └─────────┘
```

### 2.1 Направление зависимостей (verified, v10.7.4)

- **`format/`**: зависит только от `generator::config::{CefConfig, LeefConfig}` (leaf types),
  `error::*`, `chrono`, `bytes`. **Не** зависит от `transport/`, `generator/`, `observability/`.
- **`transport/`**: зависит от `observability::metrics::Metrics`, `error::*`, `bytes`,
  `tokio`, `rustls`. **Не** зависит от `generator/`, `format/` (транспорт не знает
  про форматы — он отправляет `Vec<u8>` отформатированных байт).
- **`observability/`**: зависит только от `error::MetricsError`. **Не** зависит от
  `generator/`, `format/`, `transport/`.
- **`generator/`**: оркестратор — зависит от всех остальных слоёв.
- **`error.rs`**: leaf, зависит от `thiserror` + `std::io`.

Циркулярных зависимостей нет.

---

## 3. Trait `Format` (F15, v9.2.0)

```rust
// src/format/mod.rs
pub trait Format {
    fn render(&self, ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8>;
}
```

- **dyn-compatible**: ✅ нет `async fn`, нет `Self: Sized`, нет associated types.
- **dispatch**: статический через `FormatKind` enum (0 vtable lookups, 0 heap).

### 3.1 Добавление нового формата

Шаблон для нового формата `XYZ`:

1. Создать `src/format/xyz.rs`:

```rust
//! XYZ формат для <use-case>.

use super::{Header, FormatContext};

pub fn build(ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8> {
    let h = &ctx.header;
    // ... format-specific serialization ...
    result
}
```

2. Добавить вариант в `FormatKind` (src/format/mod.rs):

```rust
pub enum FormatKind {
    Rfc5424,
    Rfc3164,
    Raw,
    Protobuf,
    Cef,
    Leef,
    JsonLines,
    Xyz,  // ← NEW
}

impl FormatKind {
    pub fn from_str_or_default(s: Option<&str>) -> Self {
        match s.unwrap_or("rfc5424") {
            // ...
            "xyz" => Self::Xyz,
            _ => Self::Rfc5424,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Xyz => "xyz",
            // ...
        }
    }
}

impl Format for FormatKind {
    fn render(&self, ctx: &FormatContext<'_>, msg: &[u8]) -> Vec<u8> {
        match self {
            // ...
            Self::Xyz => xyz::build(ctx, msg),
        }
    }
}
```

3. Реэкспорт в `src/lib.rs` если публичный API нужен.

4. Тесты в `src/format/xyz.rs::tests`.

5. Добавить в `schemas/profile.schema.json` (enum в `format`).

6. Добавить пример в `examples/`.

---

## 4. Trait `Transport` (v9.1.0)

```rust
// src/transport/mod.rs
pub trait Transport: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(
        &self,
        addr: &str,
        phase_name: &str,
        rx: SharedRx,
        metrics: Metrics,
        shutdown: CancellationToken,
        reconnect: Option<ReconnectConfig>,
        tls_params: Option<TlsParams>,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
```

- **dyn-compatible**: ✅ `Send + Sync` supertrait, `async fn` через RPITIT (Rust 1.75+).
- **dispatch**: статический через `TransportKind` enum (`File | Tcp | Udp | Tls`).
  Kafka — отдельная ветка в `run_phase_multi` (имеет специфичный `KafkaConfig`).

### 4.1 Добавление нового транспорта

Шаблон для нового транспорта `xyz`:

1. Создать `src/transport/xyz.rs`:

```rust
//! XYZ transport — <use-case>.

use crate::metrics::Metrics;
use crate::transport::{record_send, record_error, next_msg, SharedRx};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub async fn target_sender_xyz(
    addr: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
) -> Result<()> {
    // ... open connection, loop on rx, send, record metrics ...
    Ok(())
}
```

2. Добавить вариант в `TransportKind` (src/transport/mod.rs):

```rust
pub enum TransportKind {
    File,
    Tcp,
    Udp,
    Tls,
    Xyz,  // ← NEW (если не требует специфичного config)
}
```

3. Добавить в `Transport for TransportKind::run` (если используется через trait)
   ИЛИ добавить отдельную ветку в `run_phase_multi` (если специфичный config).

4. Добавить поле в `TargetConfig` (если нужны параметры):

```rust
// src/generator/config.rs
pub struct TargetConfig {
    // ...
    #[serde(default)]
    pub xyz_param: Option<String>,
}
```

5. Валидация в `src/validate.rs` (новый `ValidationError` вариант).

6. CI: отдельная test-job если требует feature (как Kafka).

7. Тесты в `src/transport/xyz.rs::tests`.

---

## 5. Trait `FormatContext` (v9.2.0)

```rust
pub struct FormatContext<'a> {
    pub header: &'a Header,
    pub cef: Option<&'a CefConfig>,
    pub leef: Option<&'a LeefConfig>,
    pub json_lines_fields: Option<&'a [String]>,
}
```

Передаётся в `Format::render` чтобы формат имел доступ к per-target/per-phase
конфигурации. Если новый формат требует своего конфига — добавьте поле в
`FormatContext` (с `#[serde(default)]` для backward-compat).

---

## 6. Аномалии (F17)

```rust
// src/anomaly.rs
pub struct AnomalyPlanner<'a> {
    pub anomalies: &'a [Anomaly],
}

impl<'a> AnomalyPlanner<'a> {
    pub fn new(anomalies: &'a [Anomaly]) -> Self { ... }
    pub fn combined_rate_multiplier(&self, t_secs: f64) -> f64 { ... }
    pub fn should_drop(&self, phase_seed: Option<u64>, seq: usize) -> bool { ... }
}
```

`AnomalyKind` — tagged enum с тремя вариантами: `BurstInjection`, `SlowDrip`,
`PacketLoss`. `should_drop` детерминирован по `(phase_seed, seq)` через
SplitMix64 с фиксированным salt (`DROP_DECISION_SEQ_SALT`).

### 6.1 Добавление нового типа аномалии

1. Новый variant в `AnomalyKind`.
2. Логика в `rate_multiplier()` и/или `should_drop()`.
3. `AnomalyPlanner::combined_rate_multiplier` учитывает новый тип.
4. Метрика `syslog_anomalies_applied_total{phase,type}` автоматически подхватит.
5. Валидация в `src/validate.rs`.

---

## 7. Профили нагрузки (F3)

```rust
// src/load_shape.rs
pub enum LoadShape {
    Constant,
    Linear { ramp_secs: u64 },
    Sine { period_secs: u64, amplitude: f64 },
    Burst { spike_interval_secs: u64, spike_multiplier: f64 },
}
```

`LoadShape` даёт `target_rate(t: f64) -> f64` — функция интенсивности от
времени в секундах. Используется в `run_phase_multi` для корректировки
`messages_per_second` каждый цикл.

---

## 8. Observability — Prometheus метрики

`Metrics` struct (`src/observability/metrics.rs`) — 24 метрики, organized:

```rust
pub struct Metrics {
    pub messages_total: IntCounterVec,        // CounterVec по (transport, phase, target, status)
    pub bytes_total: IntCounterVec,           // по (transport, phase, target)
    pub errors_total: IntCounterVec,          // по target
    pub reconnects_total: IntCounterVec,      // по (transport, target)
    pub send_duration: HistogramVec,          // по transport
    pub message_size_bytes: HistogramVec,     // по transport
    pub target_rate: Gauge,
    pub achieved_rate: Gauge,
    pub active_workers: Gauge,
    pub messages_by_format_total: IntCounterVec,
    pub anomalies_applied_total: IntCounterVec,
    pub anomalies_dropped_total: IntCounterVec,
    pub shutdowns_total: IntCounter,
    pub drain_duration: Histogram,
    pub drain_timeouts_total: IntCounter,
    // ...
}
```

### 8.1 Добавление новой метрики

1. Поле в `Metrics` struct с типом из `prometheus::*`.
2. Регистрация в `Metrics::new()` через `Registry::register`.
3. Использование в нужном слое (sender / generator / etc.).
4. Если per-phase/per-target — `IntCounterVec` с label dimensions.
5. Pre-resolve handle per phase если hot-path (избежать HashMap lookup per msg).

---

## 9. CI/CD пайплайн (N9, v8.4.0 + расширения v10.3-v10.5)

`.github/workflows/ci.yml` — main pipeline:

| Job | Trigger | Blocking | Что делает |
|-----|---------|----------|-----------|
| Test (ubuntu-latest + macos-latest) | push/PR | ✅ | fmt → clippy → build --release → test → bench --no-run → bench --quick (artifact) |
| test-kafka (ubuntu-latest) | push | ✅ | то же с `--features kafka,test-helpers` + validate ALL examples |
| msrv (ubuntu-latest) | push | ✅ (v10.5.0+) | build + test с MSRV toolchain из rust-toolchain.toml |
| cargo-deny | push | ✅ | security advisories + license compliance |
| cargo-machete | push | ✅ | unused dependencies |
| Coverage (cargo-llvm-cov) | push | ❌ (non-blocking) | baseline отчёт + artifact |
| Docker (multi-arch) | push to main + release/v*.*.* | ✅ | build + push в ghcr.io |

### 9.1 Quality Gates (обязательные перед merge в main)

- `cargo fmt --all -- --check` clean
- `cargo clippy --no-default-features --all-targets -- -D warnings` clean
- `cargo clippy --features kafka --all-targets -- -D warnings` clean
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` clean
- `cargo test --locked --features test-helpers` all green (339 тестов)
- `cargo test --locked --features kafka,test-helpers` all green
- `cargo bench --no-run --locked` success
- Все CI jobs зелёные на dev

---

## 10. Benchmarks (Criterion)

Два bench файла в `benches/`:

- `message_generation.rs`: `bench_template_render`, `bench_generate_message_template`,
  `bench_dispatcher_weighted`.
- `sender_throughput.rs`: `bench_tcp_sender_throughput`, `bench_udp_sender_throughput`.

PR-6 расширит до полного покрытия (все 7 форматов + 4 транспорта + reconnect +
file rotation + kafka).

---

## 11. Fuzzing (v10.4.0)

`fuzz/` директория с 5 таргетами (cargo-fuzz + libFuzzer):

- `profile_parser.rs` — `load_profile_from_json_str` / `load_profile_from_yaml_str`
- `format_rfc5424.rs` — `FormatKind::Rfc5424.render`
- `format_cef.rs` — `FormatKind::Cef.render`
- `format_leef.rs` — `FormatKind::Leef.render`
- `format_json_lines.rs` — `FormatKind::JsonLines.render`

Запуск: `cargo +nightly fuzz run <target> -- -max_total_time=60`.
См. `docs/FUZZING.md`.

---

## 12. Процесс релиза

```
feature/pr-N-* → dev (CI green) → release/v*.*.* (CI green) →
main (release-gate) → tag → push
```

Каждый релиз обязан:
1. Пройти все Quality Gates (раздел 9.1).
2. Bump `Cargo.toml version`.
3. Обновить `CHANGELOG.md` (новая секция `## vX.Y.Z - ГГГГ-ММ-ДД`).
4. Обновить `README.md` (version badge).
5. Обновить `CLAUDE_HANDOFF.md` (история версий).
6. `cargo clean` после сборки.
7. Архив в `.archived-releases/` (НЕ в git).

---

## 13. Стиль кода и соглашения

- **Rust edition 2021**, MSRV 1.95.
- **Ошибки**: типизированные через `thiserror` (N7); `.unwrap()/.expect()` в
  рантайм-коде **запрещены** (проверяется через `grep` в CI).
- **Логирование**: `tracing::{info,warn,error}!` (v10.7.0+), не `eprintln!`.
- **Doc comments**: обязательны для всех `pub` items (PR-4 добавит `#![warn(missing_docs)]`).
- **Tests**: inline `#[cfg(test)] mod tests` для unit; `tests/integration_tests.rs`
  для e2e.
- **Benchmarks**: Criterion, замеряют реальное поведение, не синтетику.
- **CI**: все PR проверяются через GitHub Actions matrix (ubuntu + macos).

---

## 14. Тесты (v10.7.4)

- **339 тестов** всего: 242 unit + 86 integration + 11 n7.
- 9 бенчей (Criterion).
- 5 fuzz таргетов (cargo-fuzz).
- Все Quality Gates зелёные.

---

## 15. Известные ограничения / Tech debt

- `rand 0.10` миграция отложена (breaking API) — PR-7.
- `rustls 0.23 → 0.27+` отложена — PR-8.
- Orphan re-exports в `src/lib.rs` (документированы как deprecated) —
  удаление в v11.0.0 (breaking).
- `pub use Transport` trait не используется `run_phase_multi` напрямую
  (Kafka требует специфичного config) — PR-4 архитектурная чистка.
- `Arc<Mutex<Receiver<Vec<u8>>>>` в `SharedRx` — сериализует recv между
  workers (TODO: sharding, PR-5).

---

## 16. Точки входа для расширения

| Что добавить | Где смотреть | Сложность |
|--------------|--------------|-----------|
| Новый формат | раздел 3 | S (1-2 дня) |
| Новый транспорт | раздел 4 | M (2-5 дней) |
| Новая аномалия (F17) | раздел 6 | S (1 день) |
| Новый LoadShape (F3) | раздел 7 | S (1 день) |
| Новая метрика | раздел 8 | XS (1 час) |
| Новый fuzz target | `fuzz/fuzz_targets/` | XS (1 час) |
| Новый bench | `benches/` | S (1 день) |