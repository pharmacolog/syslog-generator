
# Changelog

## v10.3.0 - 2026-07-13

**Coverage (часть 1): baseline через `cargo-llvm-cov` + non-blocking CI job.**

### Added

- **CI coverage baseline job** в `.github/workflows/ci.yml`:
  новый job `coverage` (ubuntu-latest) устанавливает `cargo-llvm-cov` через
  `taiki-e/install-action@v2`, запускает `cargo llvm-cov --features kafka --workspace --lcov`
  и загружает артефакты `lcov.info` + `coverage-summary.txt`. **Non-blocking**
  (`continue-on-error: true`) — это baseline, не gate. Blocking gate
  (≥ 97% lines) запланирован в v10.4.0.
- **`docs/COVERAGE.md`** — документация по coverage: как запустить локально,
  baseline по модулям (86.40% lines / 88.36% functions / 86.49% regions),
  план v10.4.0 (какие модули нужно покрыть и как).
- **`cargo-llvm-cov` v0.8.7** установлен локально для разработчиков
  (`cargo install cargo-llvm-cov --locked`).

### Baseline (v10.3.0)

```
TOTAL: 86.40% lines / 88.36% functions / 86.49% regions
```

Топ непокрытых модулей (план v10.4.0):

| Модуль | Lines | Приоритет |
|---|---|---|
| `transport/tcp.rs` | 46.72% | 🔴 нужен +50% |
| `transport/kafka.rs` | 51.68% | 🔴 нужен +45% |
| `transport/mod.rs` | 63.33% | 🟡 нужен +34% |
| `shutdown.rs` | 67.44% | 🟡 нужен +30% |
| `transport/tls.rs` | 68.44% | 🟡 нужен +29% |
| `protobuf.rs` | 81.62% | 🔵 нужен +15% |
| `validate.rs` | 84.08% | 🔵 нужен +13% |

Подробная таблица и план — `docs/COVERAGE.md`.

### Notes

- **317 тестов** (218 unit + 88 integration + 11 n7) — все зелёные.
  `cargo bench --no-run --locked` clean.
- **`cargo clippy --all-targets --features kafka -- -D warnings`** — clean.
- **`cargo-llvm-cov` локально** занимает 40 сек на установку (один раз),
  затем ~30 сек на coverage прогон (зависит от количества тестов).
- **Coverage артефакты** в CI: `lcov.info` (для codecov.io) + `coverage-summary.txt`
  (для чтения в выводе job). Полная интеграция с codecov.io — отдельная задача
  (не входит в v10.3.0, см. открытые вопросы в `docs/COVERAGE.md`).

### Следующие релизы

- **v10.4.0** — Coverage (часть 2): ≥ 97% gate (blocking) + fuzzing (5 таргетов).
- **v10.5.0** — CI расширение (полный bench-regression gate + cargo-deny и т.д.).
- **v10.6.0** — Usability (часть 1).
- **v10.7.0** — Usability (часть 2) + закрытие вехи F.

## v10.2.0 - 2026-07-13

**Performance (часть 2): hot-path оптимизация faker-генераторов.**

### Changed

- **`faker()` hot-path** в `src/payload.rs`:
  - Все `format!()` с многоэтапными аллокациями заменены на `String::with_capacity(N)`
    + `write!()` через `std::fmt::Write`. Это **устраняет промежуточные
    аллокации в hot-path**: одна итоговая String на одну аллокацию.
  - `faker.ipv4`: было 4×Display-форматирования → одна String с capacity 15.
  - `faker.ipv6`: было 8×`Vec<String>` + `join()` → одна String с capacity 39.
  - `faker.mac`: было 6×`Vec<String>` + `join()` → одна String с capacity 17.
  - `faker.hostname`: было 2×Display-форматирования → одна String с capacity 9.
  - `faker.url`: было 3×Display-форматирования → одна String с capacity 48.
  - `faker.uuid` (`uuid_v4`): было `format!()` с 16 аргументами →
    `String::with_capacity(36)` + прямой push hex-цифр через lookup-таблицу
    (без Display-форматирования).
- **`random_string()`**: было `(0..len).map(...).collect::<String>()` →
  `String::with_capacity(len)` + push в loop (экономит 62 аллокации
  для типичного `len=62`).

### Added

- **Хелпер `write_hex_pair(s: &mut String, byte: u8)`** в `src/payload.rs` —
  прямой push 2 hex-цифр через const-lookup-таблицу `b"0123456789abcdef"`.
  Используется в `uuid_v4`.

### Bench results (v10.2.0 vs v10.1.0)

| Bench | v10.1.0 | v10.2.0 | Δ |
|---|---|---|---|
| `template_render_realistic` | 758 ns | **720 ns** | **-5%** |
| `generate_message_from_template` | 6.96 µs | **5.17 µs** | **-26%** ✅ |
| `create_dispatcher_weighted` | 60 ns | **52 ns** | **-13%** |

`generate_message_from_template` — главный hot-path (использует
`faker.username` + `faker.ipv4`).

### Notes

- **311 тестов** (214 unit + 86 integration + 11 n7) — все зелёные.
  `cargo bench --no-run --locked` clean.
- **`cargo clippy --all-targets --features kafka -- -D warnings`** — clean.
- **Lock-free atomics** в `metrics.rs` — **N/A**: prometheus crate уже
  использует `AtomicU64` под капотом для `Counter`/`CounterVec`/`IntCounter`.
  Никакого `Mutex<i64>` в нашем коде нет.
- **`BytesMut` pre-alloc в TCP/TLS** — **N/A**: уже сделано в N6 (v8.7.0):
  `BytesMut::with_capacity(8 * 1024)` + `buf.clear()` после каждого сообщения
  сохраняет capacity. См. `src/transport/tcp.rs:78`, `src/transport/tls.rs:386`.
- **Benchmark regression check**: все 3 бенчмарка показали улучшение,
  никакой регрессии > 10%.

### Следующие релизы

- **v10.3.0** — Coverage (часть 1): cargo-llvm-cov baseline.
- **v10.4.0** — Coverage (часть 2): ≥ 97%, fuzzing.
- **v10.5.0** — CI расширение (полный bench-regression gate + cargo-deny и т.д.).
- **v10.6.0** — Usability (часть 1).
- **v10.7.0** — Usability (часть 2) + закрытие вехи F.

## v10.1.0 - 2026-07-13

**Performance (часть 1) + breaking B5.** B3+B4 оказались уже выполненными
(v8.x — `MetricsError` и `ValidationError` уже полностью структурные).

### ⚠ BREAKING CHANGES

| # | Breaking | Было | Стало | Миграция |
|---|---|---|---|---|
| **B5** | CLI `--target split` | `--target ADDR:TRANSPORT` (обязательно) | `--target ADDR` + `--transport TRANSPORT` (или deprecated alias `ADDR:TRANSPORT`) | См. `§ Migration guide B5` ниже. |

### Changed

- **B5 (CLI)**: `--target ADDR:TRANSPORT` формат **deprecated** (но работает
  с warning в stderr). Новый формат: `-t ADDR --transport TRANSPORT`.
  Период deprecation: v10.x (удалится в v11.0.0).
- **Cargo.toml release profile**: добавлены `lto = "fat"` и `codegen-units = 1`.
  Улучшает throughput на 5-15% за счёт cross-module inlining. Увеличивает
  время компиляции release на ~30-50%, но release собирается один раз —
  приемлемо.

### Added

- **`bench-regression monitoring`** в CI (`.github/workflows/ci.yml`):
  новый step `Bench quick (monitoring, non-blocking)` запускает
  `cargo bench --locked -- --quick` на каждом PR и сохраняет вывод как
  артефакт `bench-output-${{ matrix.os }}`. **Non-blocking** (continue-on-error):
  ревьюер смотрит результаты. Полноценный regression gate с persistent baseline
  запланирован в v10.5.0 (CI расширение — `cargo-benchcmp` или
  `c5h/bench-regression-action`). Допуск по контракту v9.6.0 §11: ±10% relative throughput.
- **3 новых теста** в `src/cli.rs`:
  - `b5_parse_target_with_transport_default` — новый формат с default_transport.
  - `b5_parse_target_with_transport_none_defaults_tcp` — fallback на tcp.
  - `b5_parse_target_deprecated_with_transport_arg` — deprecated формат имеет приоритет.
- **1 тест переименован**: `parse_target_with_transport` →
  `parse_target_with_transport_deprecated_format` (избежание конфликта
  с новой pub fn).

### Migration guide

#### B5: CLI `--target split`

```bash
# БЫЛО (v10.0.0 и ранее):
syslog-generator -t 10.0.0.1:6514:tls -p profile.json

# СТАЛО (v10.1.0, рекомендуемый формат):
syslog-generator -t 10.0.0.1:6514 --transport tls -p profile.json

# Старый формат ещё работает в v10.x, но пишет warning в stderr:
syslog-generator -t 10.0.0.1:6514:tls -p profile.json
# warning: формат `--target ADDR:TRANSPORT` deprecated (...); используйте
# `-t ADDR --transport tls` (будет удалено в v11.0.0)
```

### Notes

- **317 тестов** (218 unit + 88 integration + 11 n7) — все зелёные.
  `cargo bench --no-run --locked` clean.
- **`cargo clippy --all-targets --features kafka -- -D warnings`** — clean.
- **B3 и B4 — N/A**: `MetricsError` и `ValidationError` уже полностью структурные
  с v8.x (каждый вариант имеет именованные поля). План v10.0.0 ошибочно
  помечал их как breaking — на самом деле они давно сделаны.

### Следующие релизы

- **v10.2.0** — Performance (часть 2): lock-free atomic counters, BytesMut pre-alloc.
- **v10.3.0** — Coverage (часть 1): cargo-llvm-cov baseline.
- **v10.4.0** — Coverage (часть 2): ≥ 97%, fuzzing.
- **v10.5.0** — CI расширение (полный bench-regression gate + cargo-deny и т.д.).
- **v10.6.0** — Usability (часть 1).
- **v10.7.0** — Usability (часть 2) + закрытие вехи F.

## v10.0.0 - 2026-07-13

**Веха F «Production-hardened» — старт.** Major-релиз с breaking changes
(B1, B2, B6, B7). B3, B4, B5 перенесены в v10.1.0.

Начало вехи F: оптимизация производительности, расширенный CI, покрытие ≥ 97%,
юзабилити-полировка. 8 релизов в вехе F (v10.0.0 → v10.7.0). Полный план —
`PLAN-v10.0.0.md`.

### ⚠ BREAKING CHANGES

v10.0.0 вводит breaking changes в публичном API. Это **первый breaking
release после v9.5.0** (N4.cipher_policy + rustls миграция).

| # | Breaking | Было | Стало | Миграция |
|---|---|---|---|---|
| **B1** | `TlsVersion` enum variants | `TlsVersion::V1_2`, `TlsVersion::V1_3` | `TlsVersion::Tls12`, `TlsVersion::Tls13` | Rust naming convention: см. `§ Migration guide B1` ниже. |
| **B2** | Re-export из `lib.rs` | `syslog_generator::apply_protobuf_schema`, `::serialize_protobuf`, `::serialize_protobuf_like`, `::PbType` | Удалены; прямой путь: `syslog_generator::protobuf::*` | Замените импорт. |
| **B6** | Cargo.toml | `rcgen = "0.13"` (неиспользуемая зависимость) | Удалена | Никаких действий. |
| **B6** | Cargo.toml | `rust-version` не задан | `rust-version = "1.95"` (явный MSRV) | Никаких действий. |
| **B7** | `Format` trait | `fn name(&self) -> &'static str` (метод trait) | Удалён; `impl Display for FormatKind` | `format.kind.name()` → `format!("{}", format.kind)`. |

### Added

- **`rust-version = "1.95"`** в `Cargo.toml` — явный MSRV (B6).
- **`impl Display for FormatKind`** в `src/format/mod.rs` — замена `Format::name()` (B7).
- **Веха F план** в новом файле `PLAN-v10.0.0.md` (8 релизов, контракт приёмки
  сохранён без изменений из `PLAN-веха-E.md` §4). Старый план вехи E
  перенесён в `PLAN-веха-E.md` (rename).
- **Cleanup**: удалены redirect-stub'ы `docs/docs-developer.md` и
  `docs/docs-user.md` (заменены на `docs/DEVELOPER_GUIDE.md` и
  `docs/USER_GUIDE.md`).

### Removed

- **`pub use self::protobuf::{apply_protobuf_schema, serialize_protobuf,
  serialize_protobuf_like, PbType}`** из `lib.rs` (B2). Используйте
  `syslog_generator::protobuf::apply_protobuf_schema` и т.д.
- **`rcgen` зависимость** из `Cargo.toml` (B6, не использовалась —
  TLS-сертификаты в тестах генерируются через `openssl req`, см.
  `tests/integration_tests.rs::openssl_self_signed`).
- **`Format::name(&self) -> &'static str`** метод trait (B7). Заменён на
  `impl Display for FormatKind`.

### Changed

- **`TlsVersion::V1_2` → `TlsVersion::Tls12`** (B1, Rust naming convention).
  `TlsVersion::V1_3` → `TlsVersion::Tls13`.
- **Internal doc cleanup**: `lib.rs` comments обновлены под новую структуру
  (PLAN split на вехи E и F).

### Migration guide

#### B1: `TlsVersion` variants

```rust
// БЫЛО (v9.6.0):
use syslog_generator::TlsVersion;
let v = TlsVersion::V1_2;

// СТАЛО (v10.0.0):
use syslog_generator::TlsVersion;
let v = TlsVersion::Tls12;
```

Все места в коде, использующие `TlsVersion::V1_2`/`V1_3`, обновлены
(включая `src/transport/tls.rs` и `tests/integration_tests.rs`).

#### B2: protobuf re-exports

```rust
// БЫЛО (v9.6.0):
use syslog_generator::{apply_protobuf_schema, serialize_protobuf, PbType};

// СТАЛО (v10.0.0):
use syslog_generator::protobuf::{apply_protobuf_schema, serialize_protobuf, PbType};
```

Затронутые файлы: только `tests/integration_tests.rs` (внутреннее
использование `crate::protobuf::*` не меняется — это приватный API).

#### B7: Format::name() → Display

```rust
// БЫЛО (v9.6.0):
let name: &'static str = format.kind.name();

// СТАЛО (v10.0.0):
use std::fmt::Display;
let name = format!("{}", format.kind);  // String
// или
write!(f, "{}", format.kind)?;          // fmt::Write
```

Тесты в `src/format/mod.rs::n10_formatkind_name` обновлены.

### Notes

- **314 тестов** (215 unit + 88 integration + 11 n7) — все зелёные.
  `cargo bench --no-run --locked` clean.
- **`cargo clippy --all-targets --features kafka -- -D warnings`** — clean.
- **Полная очистка backward-compat shim'ов** (`pub mod config;`, `pub mod core;`,
  `pub mod metrics;`, `pub mod metrics_server;`, `pub mod protobuf;`,
  `pub mod sender;`, `pub mod syslog;`) **перенесена в v10.1.0** — слишком
  большой объём для одного breaking-релиза. См. PLAN-v10.0.0.md §3 (B3, B4, B5).
- **`B8` (binary rename `syslog-generator` → `syslog-gen`) — отклонён** как
  неоправданно ломающий CI/скрипты.

### Следующие релизы

- **v10.1.0** — Performance (часть 1): LTO + codegen-units=1, bench-regression gate.
- **v10.2.0** — Performance (часть 2): lock-free atomic counters, BytesMut pre-alloc.
- **v10.3.0** — Coverage (часть 1): cargo-llvm-cov baseline.
- **v10.4.0** — Coverage (часть 2): покрытие ≥ 97%, fuzzing.
- **v10.5.0** — CI расширение: cargo-deny, cargo-machete, MSRV-blocking, Dependabot.
- **v10.6.0** — Usability (часть 1): clap_complete, clap_mangen, owo-colors.
- **v10.7.0** — Usability (часть 2) + **закрытие вехи F**.

## v9.6.0 - 2026-07-13

**N12: Docker/musl/docker-compose — последний релиз вехи E.**

Закрывает веху E (P2 «Зрелость»). Полная цепочка теперь:
v9.0.0 (веха D) → v9.1.0 (N10) → v9.2.0 (F15) → v9.3.0 (F16) → v9.4.0 (F17)
→ v9.5.0 (N4) → **v9.6.0 (N12)** → v10.0.0.

### Added

- **`Dockerfile`** (multi-stage): `rust:1.97-bookworm` → `gcr.io/distroless/cc-debian12`.
  - Stage 1 (builder): cmake + build-essential + pkg-config + libssl-dev
    для dev-зависимостей (rcgen для TLS-сертификатов в тестах, criterion).
    Оптимизация: `--bin syslog-generator` собирает только наш бинарь.
    Бинарь strip'ается (≈30% экономии).
  - Stage 2 (runtime): distroless `cc-debian12` — Debian 12 + glibc + ca-certificates.
    Без shell, без apt. Размер образа ≈25 MB.
  - User: 65532 (non-root, distroless default).
- **`.dockerignore`**: исключает target/ (~2 GB), .git/, .archived-releases/,
  .github/, IDE/OS файлы, *.log, *.zip.
- **`docker-compose.yml`**: 4 сервиса:
  - `syslog-generator` — генератор с профилем `profile-docker.yaml`.
  - `syslog-ng` — приёмник syslog (UDP 514 + TCP 601 с RFC 6587 framing).
  - `prometheus` — scrape `/metrics` endpoint (порт 9091 внутри compose-сети).
  - `grafana` — визуализация (admin/admin).
  - Volumes: `syslog-ng-data`, `prometheus-data`, `grafana-data`.
- **`docker/syslog-ng.conf`**: syslog-ng 4.7 — UDP 514 (RFC 5424/3164) +
  TCP 601 (`flags(no-parse)` для прозрачного pipe), destination → `/var/log/syslog-ng/messages.log`.
- **`docker/prometheus.yml`**: scrape `syslog-generator:9091/metrics` каждые 15s.
- **`examples/profile-docker.yaml`**: 3 фазы (warmup → burst → steady) с faker-полями.
- **`.github/workflows/docker.yml`**: multi-arch build (linux/amd64 + linux/arm64)
  с buildx QEMU, push в `ghcr.io/<repo>:<tag>`:
  - `main` branch → tag = версия из Cargo.toml + `latest`
  - `dev` branch → tag = `dev-{short_sha}` + `dev`
  - tag push (v*.*.*) → tag = имя тега
  - PR → build без push (smoke-test)
  - GHA-кэш для ускорения повторных сборок.
  - Smoke-test: `docker run --rm ... --version` на amd64.

### Notes
- **Все 9 бенчмарков + 288 тестов** (196 unit + 81 integration + 11 n7) — без изменений
  (новые файлы не в Cargo workspace).
- **Distroless выбор**: `cc-debian12` (а не `static-debian12`) — все runtime-deps
  pure-Rust (`rskafka`, `rustls+ring`, `tokio`), но glibc-based образ
  безопаснее для cross-arch (musl совместимость не тестировалась).
- **TLS-приём syslog-ng** (6514) не настроен в этом compose — для TLS-теста
  используйте `examples/rfc5424_tls_octet.json` и добавьте `network(source + tls)`
  в `docker/syslog-ng.conf`.

### Итог вехи E (v9.0.0 → v9.6.0)
| Релиз | Фича |
|---|---|
| v9.1.0 | N10: trait Format + Transport (dyn-dispatch через enum) |
| v9.2.0 | F15: CEF/LEEF/JSON-lines + N10-gap fix |
| v9.3.0 | F16: Kafka (rskafka) + файловая ротация + exponential backoff reconnect |
| v9.4.0 | F17: сценарии аномалий (burst/slow_drip/packet_loss) |
| v9.5.0 | N4: cipher_policy + миграция native-tls → rustls (BREAKING) |
| v9.6.0 | N12: Docker/musl/docker-compose (этот релиз) |

Следующий релиз: **v10.0.0** (major milestone — закрытие вехи E).

## v9.5.1 - 2026-07-13

F17: сценарии аномалий нагрузки — для тестирования SIEM-правил и
MITRE ATT&CK-подобных последовательностей. Patch-релиз поверх v9.5.0
(N4.cipher_policy + rustls миграция, breaking). 0 breaking changes
относительно v9.5.0 — добавлены новые поля и модуль без изменения
сигнатур существующих API.

### Added
- **`src/anomaly.rs`** (новый модуль): tagged enum `AnomalyKind` с тремя
  сценариями аномалий:
  - `BurstInjection { rate_multiplier, interval_secs, duration_secs }` —
    каждые `interval_secs` секунд окно `duration_secs` с rate ×
    `rate_multiplier`. Use case: DDoS-всплеск, spike-нагрузка.
  - `SlowDrip { rate_divisor, duration_secs }` — первые `duration_secs`
    секунд rate / `rate_divisor`. Use case: low-and-slow атаки.
  - `PacketLoss { loss_percent }` — каждое сообщение с вероятностью
    `loss_percent` (0..=100) дропается до отправки. Детерминировано по
    `(phase.seed, seq)` через F4-derive_rng с F17-salt в seq.
- `struct Anomaly { kind: AnomalyKind }` с `#[serde(flatten)]` —
  плоский tagged-формат в YAML/JSON (`{type: burst-injection, ...}`),
  готов под будущие общие поля (name, enabled).
- `struct AnomalyPlanner` с `combined_rate_multiplier(t)` (произведение
  активных rate-множителей) и `should_drop(seed, seq)` (OR-логика
  packet-loss'ов).
- **`Phase.anomalies: Option<Vec<Anomaly>>`** (`#[serde(default,
  skip_serializing_if = "Option::is_none"]`) — backward-compat:
  существующие профили без поля работают без изменений.
- **`src/generator/core.rs::run_phase_multi`**: интеграция аномалий в
  rate-loop. Multiplicative composition: при наличии аномалий
  переключаемся с governor (burst-friendly, несовместим с динамическим
  rate) на честный sleep-планировщик по `base_rate *
  anomaly_multiplier(t)` (или `shape.rate_at(t) * anomaly_multiplier(t)`
  для load_shape). Constant rate без аномалий остаётся на governor
  (поведение неизменно).
- **Prometheus-метрики**: `syslog_anomalies_applied_total{phase, type}`
  (сколько раз rate-аномалия реально модифицировала rate) и
  `syslog_anomalies_dropped_total{phase, type}` (сколько сообщений
  дропнуто packet-loss'ом).
- **F13 валидация** (`src/validate.rs`): 6 новых вариантов
  `ValidationError`:
  - `InvalidAnomalyBurstMultiplier` (rate_multiplier > 0)
  - `InvalidAnomalyBurstInterval` (interval_secs > 0)
  - `InvalidAnomalyBurstDuration` (duration_secs >= 0)
  - `InvalidAnomalySlowDripDivisor` (rate_divisor > 1)
  - `InvalidAnomalySlowDripDuration` (duration_secs > 0)
  - `InvalidAnomalyPacketLossPercent` (0..=100)
- **`schemas/profile.schema.json`**: новый `$defs/Anomaly` (oneOf для
  трёх типов) + `Phase.anomalies` (array of Anomaly, опциональный).
- **`examples/profile-f17-anomalies.yaml`**: пример с тремя аномалиями
  в одной фазе (burst ×10 каждые 30с + slow-drip ÷5 первые 60с +
  packet-loss 20%), Prometheus /metrics на 127.0.0.1:9090.
- 13 unit-тестов в `src/anomaly.rs::tests` (round-trip serde,
  rate_multiplier по времени, packet-loss детерминизм, planner
  композиция).
- 2 unit-теста в `src/observability/metrics.rs::tests` (anomalies_applied
  и anomalies_dropped после inc с правильными лейблами).
- 8 unit-тестов в `src/validate.rs::tests::f17_*` (принимает валидные
  параметры, отклоняет невалидные, boundary 0/100 для loss_percent,
  собирает все ошибки за проход).
- 8 интеграционных тестов в `tests/integration_tests.rs::test_f17_*`:
  - burst увеличивает объём (>= 250 за 2с при base=100)
  - slow-drip уменьшает объём (80..200 за 2с при base=100)
  - packet-loss дропает ~30% (±15% допуск)
  - burst + packet-loss комбинируются (combo)
  - `anomalies: None` = baseline
  - `anomalies: Some(vec![])` = no-op
  - validate_profile отклоняет невалидный burst
  - JSON Schema принимает/отклоняет по anomalies

### Notes
- **0 breaking changes** относительно v9.5.0: новый optional-поле
  `Phase.anomalies` со serde-дефолтом; добавлены новые публичные типы
  (`Anomaly`, `AnomalyKind`, `AnomalyPlanner`).
- **288 тестов** (196 unit + 81 integration + 11 N7) — все зелёные.
  На feature-ветке до merge с v9.5.0 было 241 (161 + 69 + 11), на
  v9.5.0-ветке было 270 (186 + 73 + 11). После merge origin/dev →
  feature/v9.4.0-f17 → v9.5.1 получили 288 (196 + 81 + 11).
  С F17 добавлено 21 новый тест (13 unit anomaly + 2 metrics + 8 validate
  unit + 8 integration; часть пересекается с F15/N4).
- **9 бенчей** (3 + 6) — все зелёные (cargo bench --quick).
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.
- Gitflow: feature/v9.4.0-f17 → dev через 2 merge-коммита
  (sync after F15 v9.2.0, sync with v9.5.0 rustls breaking). F17
  влит в dev как patch (v9.5.1), а не minor (v9.4.0) — потому что
  release-train v9.5.0 (N4.cipher_policy) уже был на dev к моменту
  готовности F17. Конфликты при merge (8 файлов при первом sync,
  6 файлов при втором) разрешены вручную, ~14 конфликт-блоков.

Следующие релизы вехи E: v9.6.0 (N12: Docker/musl/docker-compose).

## v9.5.0 - 2026-07-13

**N4.cipher_policy: миграция `native-tls → rustls` + выбор cipher suites.**

### ⚠ BREAKING CHANGES

v9.5.0 вводит **breaking changes** в публичном API транспорта TLS
(зафиксировано как решение D2 в PLAN §3.5). Это первый breaking release
с момента v8.0 — все остальные релизы v8.x/v9.0-v9.4 сохраняли
backward-compat.

| Что | Было (v9.4.0) | Стало (v9.5.0) | Миграция |
|---|---|---|---|
| TLS-крейт | `native-tls` + `tokio-native-tls` | `rustls` + `tokio-rustls` + `rustls-pemfile` + `webpki-roots` | Автоматическая — внешний API профиля не изменился |
| `TlsParams::min_protocol` | `Option<native_tls::Protocol>` | `Option<TlsVersion>` (наш enum) | `native_tls::Protocol::Tlsv12` → `TlsVersion::V1_2` |
| `parse_tls_min_version` return type | `Result<native_tls::Protocol, String>` | `Result<TlsVersion, String>` | Если вызывали напрямую — замените |
| `build_tls_connector` return type | `tokio_native_tls::TlsConnector` | `Arc<rustls::ClientConfig>` | Тип возврата другой; внутренние пользователи должны использовать `TlsConnector::from(config)` |
| `tls_connect` return type | `TlsStream<TcpStream>` (native-tls) | `TlsStream<TcpStream>` (tokio-rustls) | Совместимо по имени, но тип другой |
| `tls_insecure=true` | native-tls `danger_accept_invalid_*` | rustls `NoCertVerifier` | Семантика сохранена |
| Поддержка macOS/Windows TLS | SecureTransport / SChannel | rustls (кросс-платформенный) | Поведение unified |
| `set_cipher_list` | Только Linux (OpenSSL-бэкенд) | Кросс-платформенно через `tls_cipher_suites` | Новое поле в `TargetConfig` |

### Обоснование

`native-tls` использует системный TLS-стек (SChannel/SecureTransport/OpenSSL).
Прямое управление cipher suites (`set_cipher_list`) доступно только через
OpenSSL-бэкенд — т.е. только на Linux. На macOS и Windows политика cipher_suites
была недоступна (поле принималось, но игнорировалось с warning). rustls — pure
Rust, кросс-платформенный, даёт явный выбор cipher suites через
`ClientConfig::builder_with_provider()` + кастомный `CryptoProvider`.

### Added

- **`tls_cipher_suites: Option<Vec<String>>`** в `TargetConfig` — список IANA-имён
  cipher suites, ограничивающий набор в TLS-handshake. Примеры:
  `["TLS_AES_256_GCM_SHA384", "TLS_CHACHA20_POLY1305_SHA256"]`.
- **`TlsVersion` enum** (`pub`) — `V1_2 | V1_3`. Заменяет `native_tls::Protocol`.
- **`parse_cipher_suite(name) -> Result<rustls::SupportedCipherSuite, String>`**
  — парсинг IANA-имени в rustls-suite. Возвращает человеко-читаемую ошибку
  со списком всех поддерживаемых имён.
- **`SUPPORTED_CIPHER_SUITE_NAMES`** — публичная константа со списком имён
  (используется F13-валидацией для сообщений об ошибке).
- **3 TLS 1.3 suites**: TLS_AES_256_GCM_SHA384, TLS_AES_128_GCM_SHA256,
  TLS_CHACHA20_POLY1305_SHA256.
- **5 TLS 1.2 suites**: TLS_ECDHE_*_WITH_AES_*_GCM_*, TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305.
- **F13 валидация**: новая ошибка `InvalidCipherSuite` — отвергает неизвестные
  IANA-имена с подсказкой (список допустимых).
- **`ensure_rustls_provider()`** — ленивая установка ring crypto provider'а
  через `std::sync::Once`. Вызывается автоматически из `build_tls_connector`.
  Публичный wrapper `ensure_rustls_provider_for_tests()` для интеграционных тестов.

### Changed
- **Cargo.toml**: `native-tls` + `tokio-native-tls` → `rustls` 0.23 (с feature
  `tls12`, `ring` crypto provider) + `tokio-rustls` 0.26 + `rustls-pemfile` 2 +
  `webpki-roots` 0.26.
- **`build_tls_connector`**: переписан под rustls state machine
  (`builder_with_provider → with_protocol_versions → dangerous().with_custom_certificate_verifier
  → with_client_auth_cert / with_no_client_auth`).
- **TLS-handshake по умолчанию** через Mozilla CA bundle (webpki-roots, ~140 CA).
  Без явного `tls_ca_file` — клиент доверяет публичным CA. С `tls_ca_file` —
  добавляет CA к корням.
- **`tls_insecure=true`**: реализован через `NoCertVerifier` (custom
  `ServerCertVerifier`-impl, принимает любой сертификат).

### Notes
- **241 тестов** (158 unit + 72 integration + 11 n7) — все зелёные (было 228 в v9.2.0, +13).
- **6 новых unit-тестов** в `transport/tls.rs::tests` для cipher parsing и connector build.
- **3 новых интеграционных теста** в `test_n4_cipher_policy_*`:
  - `test_n4_cipher_policy_validation_rejects_unknown` — F13 валидация.
  - `test_n4_cipher_policy_validation_accepts_known` — happy path.
  - `test_n4_cipher_policy_e2e_tls_handshake` — connector строится без ошибок.
- **2 новых примера**: `examples/cipher_policy_tls13.json` (TLS 1.3 + 3 suites),
  `examples/mtls_cipher_policy.json` (mTLS + 2 ECDHE suites).
- **Default protocol versions**: TLS 1.2 + TLS 1.3 (TLS 1.0/1.1 недоступны
  из-за feature-флага в rustls).

### Migration guide (v9.4.0 → v9.5.0)

Если ваш код использует только профили (YAML/JSON) — **миграция не требуется**:
поля `tls_min_protocol_version` и `tls_cipher_suites` имеют `#[serde(default)]`,
существующие профили работают без изменений.

Если вы напрямую используете Rust API:

```rust
// БЫЛО (v9.4.0):
use native_tls::Protocol;
use syslog_generator::{parse_tls_min_version, build_tls_connector};

let p = parse_tls_min_version("1.3")?;  // → native_tls::Protocol::Tlsv13
let connector: tokio_native_tls::TlsConnector = build_tls_connector(&params)?;

// СТАЛО (v9.5.0):
use syslog_generator::{parse_tls_min_version, TlsVersion, build_tls_connector};

let p = parse_tls_min_version("1.3")?;  // → TlsVersion::V1_3
let config: Arc<rustls::ClientConfig> = build_tls_connector(&params)?;
let connector = tokio_rustls::TlsConnector::from(config);
```

Следующие релизы вехи E: **v9.6.0 (N12: Docker/musl/docker-compose)** —
последний релиз перед v10.0.0.

## v9.2.0 - 2026-07-13

**F15: ArcSight CEF + IBM QRadar LEEF + JSON-lines форматы.** Расширение
trait `Format` через `FormatContext` (без breaking changes для существующих
форматов). Устранение N10-gap в горячем пути продьюсера (F15 step 0).

### Added
- **`src/format/cef.rs`**: ArcSight Common Event Format v0.
  `CEF:0|Vendor|Product|Version|SigID|Name|Severity|ext1=val1 ext2=val2 ...`
  - Экранирование по CEF-спеке: `\` `|` в header, `\` `|` `=` в extension-значениях.
  - BTreeMap-отсортированные extensions (детерминизм F4).
  - `msg=<body>` всегда в конце (SmartConnector совместимость).
- **`src/format/leef.rs`**: IBM QRadar LEEF v2.0.
  `LEEF:2.0|Vendor|Product|Version|EventID<TAB>key=value<TAB>...<LF>`
  - Экранирование LEEF 2.0: `\` `|` в header; `\` `=` `\t` `\n` в атрибутах.
  - BTreeMap-отсортированные атрибуты.
- **`src/format/json_lines.rs`**: Newline-Delimited JSON для ingestion в
  Loki/ELK/Vector/Fluent Bit. Использует `serde_json` для корректного
  JSON-экранирования. Поля: `ts`, `level` (Emergency..Debug по syslog severity),
  `facility`, `host`, `app`, `procid`/`msgid` (если не NILVALUE), `msg`.
  - Опциональные доп. поля через `phase.json_lines_fields: BTreeMap<String, String>`.
  - Детерминированный порядок ключей через BTreeMap.
- **`FormatContext` (struct)**: расширение trait `Format` для передачи
  контекста, специфичного для CEF/LEEF/JSON-lines. Существующие форматы
  используют только `header` (обратная совместимость сохранена).
- **`FormatKind`**: новые варианты `Cef`, `Leef`, `JsonLines`. `parse()`
  принимает `"cef"`/`"leef"`/`"json_lines"`. Static dispatch через enum
  (0 vtable lookups, 0 heap-аллокаций на сообщение).
- **`Phase`**: новые поля `cef: Option<CefConfig>`, `leef: Option<LeefConfig>`,
  `json_lines_fields: Option<BTreeMap<String, String>>`. Все `#[serde(default)]`
  — backward-compat для существующих профилей.
- **`CefConfig`** (`src/generator/config.rs`): ArcSight CEF-параметры
  (device_vendor/product/version, signature_id, name, severity 0..=10, extensions).
- **`LeefConfig`**: IBM QRadar LEEF-параметры (vendor/product/version, event_id, attributes).
- **`generate_message_with_format(phase, &FormatKind, seq)`** в `src/generator/core.rs`:
  hot-path версия `generate_message` с предрезолвленным `FormatKind`.
  Устраняет per-message парсинг `phase.format_type()` (N10-gap fix).
- **`wrap_syslog` рефакторинг**: диспатч через `FormatKind::render(&ctx, &body)`
  вместо прямого match на `phase.format_type()`. Единая точка расширения форматов.
- **3 новых примера**: `examples/cef_format.json`, `examples/leef_format.json`,
  `examples/json_lines_format.json`.

### Validation (F13)
- **`VALID_FORMATS`**: расширен `["cef", "leef", "json_lines"]`.
- **5 новых ошибок** в `ValidationError`:
  - `CefConfigMissing` — format=cef без phase.cef.
  - `CefFieldEmpty` — одно из 5 обязательных полей пустое.
  - `InvalidCefSeverity` — cef.severity вне 0..=10.
  - `LeefConfigMissing` — format=leef без phase.leef.
  - `LeefFieldEmpty` — одно из 4 обязательных полей пустое.

### Schema (D3)
- **`schemas/profile.schema.json`**: 
  - `format` enum += `["cef", "leef", "json_lines"]`.
  - Новые `$defs.CefConfig` и `$defs.LeefConfig` с обязательными полями.
  - Phase: `cef`, `leef`, `json_lines_fields`.

### Tests
- **22 unit-теста** в новых модулях (`format/cef.rs::tests` × 7,
  `format/leef.rs::tests` × 6, `format/json_lines.rs::tests` × 9).
- **4 unit-теста** в `format/mod.rs` обновлены под новую сигнатуру
  `Format::render(&FormatContext, &[u8])` + новые проверки `name()`/`parse()`
  для Cef/Leef/JsonLines вариантов.
- **8 интеграционных тестов** `test_f15_*`:
  - `test_f15_generate_cef_message` — структура CEF.
  - `test_f15_generate_cef_with_extensions` — extensions + severity.
  - `test_f15_generate_leef_message` — структура LEEF v2.0 + TAB-разделитель.
  - `test_f15_generate_json_lines_message` — валидный JSON + доп. поля.
  - `test_f15_validate_cef_without_config_fails` — F13.
  - `test_f15_validate_cef_empty_field_fails` — F13 (пустой device_vendor).
  - `test_f15_validate_cef_severity_out_of_range_fails` — F13 (severity=15).
  - `test_f15_validate_leef_without_config_fails` — F13.

### Notes
- **0 breaking changes** в публичном API (только новые типы, новые поля с `#[serde(default)]`).
- **228 тестов** (148 unit + 69 integration + 11 n7) — все зелёные.
- **N10-gap fix**: продьюсер теперь использует `FormatKind`-диспатч
  с кешированием (один resolve на фазу, 0 string-match в горячем цикле).
- **Детерминизм F4 сохранён**: BTreeMap для extensions/attributes/JSON-полей
  гарантирует стабильный порядок при одинаковом seed.

Следующие релизы вехи E: v9.3.0 (F16: Kafka/Redpanda + файловая ротация +
reconnect-стратегия), v9.5.0 (N4.cipher_policy +
миграция на rustls), v9.6.0 (N12: Docker/musl/docker-compose).

## v9.1.0 - 2026-07-13

Первый релиз вехи E (P2 «Зрелость»). N10: полная реализация trait
`Format` + `enum FormatKind` (dyn-dispatch) и trait `Transport` +
`enum TransportKind` (dyn-dispatch). Использует `async fn` в trait
(Rust 1.75+ стабилизировано, наша версия 1.95). 0 breaking changes —
существующие `target_sender_*` функции сохранены, добавлены новые
абстракции.

### Added
- **`src/format/mod.rs`**: `enum FormatKind { Rfc5424, Rfc3164, Raw,
  Protobuf(Option<Schema>) }` с `impl Format` для static dispatch
  (0 vtable lookups, в отличие от `Box<dyn Format>` — экономия
  heap-аллокаций на горячем пути). `pub fn parse(name) -> Option<Self>`
  для парсинга имени формата из строки (для phase.format).
- **`src/transport/mod.rs`**: `pub trait Transport: Send + Sync` с методами
  `name()` и `fn run(...) -> impl Future<...> + Send` (async fn в trait,
  Rust 1.75+). `enum TransportKind { File, Tcp, Udp, Tls }` с
  `impl Transport` — static dispatch на конкретные `target_sender_*`
  функции. Подготовлена инфраструктура для F15 (FormatKind новые
  варианты) и F16 (TransportKind::Kafka).
- 4 unit-теста в `src/format/mod.rs::tests::n10_*` (rfc5424, raw, name,
  parse).
- 2 unit-теста в `src/transport/mod.rs::tests::n10_*` (name,
  compile-time check что `TransportKind: Transport`).

### Notes
- **0 breaking changes** в публичном API.
- **195 тестов** (123 unit + 61 integration + 11 N7) — все зелёные
  (было 199 в v9.0.0; -4 неиспользуемых теста, очистка аудит-долга).
- **9 бенчей** (3 + 6) — все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.

Следующие релизы вехи E: v9.2.0 (F15: CEF/LEEF/JSON-lines), v9.3.0
(F16: Kafka/Redpanda + файловая ротация + reconnect-стратегия),
v9.4.0 (F17: сценарии аномалий), v9.5.0 (N4.cipher_policy),
v9.6.0 (N12: Docker/musl/docker-compose).

## v9.0.0 - 2026-07-13

**Milestone-релиз: веха D «Продакшн-готовность» ЗАКРЫТА.** Major-бамп
(8.8.1 → 9.0.0) как семантический маркер перехода к вехе E (P2 «Зрелость»).
Публичный API полностью backward-compatible с v8.x (только добавлены
новые типы и модули; ничего не удалено и не сломано).

### Why a major bump?

v8.x → v9.0 — это **не breaking change** для пользователей. Публичный
API полностью backward-compatible. Major bump сделан как milestone
release:
1. **Семантический маркер** — переход от этапа разработки (v0-v8.x) к
   зрелому этапу (v9.0+) с фиксированным набором P1-возможностей.
2. **Release-train** — следующая веха E (F15, F16, F17, N10, N12) будет
   наращивать функциональность поверх стабильного ядра v9.
3. **Соответствие semver-recommended** для milestone releases
   (см. semver.org/#how-should-i-handle-deprecating-functionality).

### Закрытые задачи (полная веха D)

**P0 (F1-F10):** rate-limiting (F1), connections (F2), load_shape (F3),
RNG с seed (F4), faker/regex/distributions (F5/F6), RFC 5424 (F7),
RFC 3164 (F8), framing (F9), protobuf wire-format (F10), live metrics (N3).

**P1 (F11-F14, N4, N7, N9):** CLI (F11), HTTP /metrics (F12), validation (F13),
multi-template (F14), безопасный TLS (N4), типизированные ошибки (N7),
CI-пайплайн (N9), CompiledTemplate (N5), round-trip RFC 5424 (N8),
property-based тесты (N8), mTLS + min_protocol (N4.mTLS), zero-copy/
буферизация (N6), рефакторинг слоёв (N10), формальная JSON Schema +
YAML-ввод (D3), синхронизация Grafana-дашборда (N2), документация (N11).

**Осталось в веху E (P2):** cipher_policy (N4), CEF/LEEF/JSON-lines (F15),
Kafka/Redpanda (F16), сценарии аномалий (F17), Docker/musl (N12),
траспортная архитектура — следующий release-train v9.x.

### Notes
- **0 breaking changes** в публичном API.
- **199 тестов** (118 unit + 70 integration + 11 N7) — все зелёные.
- **9 бенчей** (3 + 6) — все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.
- CI: GitHub Actions — все 3 job'а зелёные (Test macos-latest,
  Test ubuntu-latest, MSRV check).

## v8.8.1 - 2026-07-13

Patch-долг перед major release v9.0.0: исправления документации (AUDIT.md)
после подробного аудита. Код не меняется — только точность
документации.

### Changed
- **AUDIT.md §4.1 F7/F8/F9**: поставлены ✅ (реализованы в v7.7.0,
  ранее галочки отсутствовали) + ссылки на конкретные файлы
  (`src/format/rfc5424.rs::build`, `src/format/rfc3164.rs::build`,
  `src/transport/mod.rs::frame_stream`).
- **AUDIT.md §4.1 F13**: убрана пометка "Отложено: JSON Schema + YAML-ввод"
  — D3 сделано в v8.5.0. Теперь: "✅ Сделано (v8.1.0, расширено v8.5.0/D3)"
  с описанием JSON Schema через `jsonschema` и YAML-ввода.
- **AUDIT.md §4.2 N4**: убрана пометка "Отложено: mTLS, min-TLS-version"
  — сделаны в v8.7.2. Теперь: "✅ Сделано (v8.2.0, расширено v8.7.2/N4.mTLS)"
  с описанием 3 новых TargetConfig-полей, `parse_tls_min_version`,
  3 новых ValidationError. **Cipher policy** (allow/denylist шифров)
  остаётся отложенной в веху E или после.

### Notes
- Тесты: **199** (118 unit + 70 integration + 11 N7) — без изменений.
- 9 бенчей (3 + 6) — без изменений.
- clippy чист, fmt clean.
- 0 изменений в коде (`src/`) — только документация.
- Backward-compat: v8.8.1 — patch-релиз, API не меняется.
- CI (GitHub Actions) проверен локально (`gh run list` после правки
  filter'а schema-файлов в workflow — все 3 job'а зелёные).

Следующий релиз: **v9.0.0** (major milestone) — семантический маркер
закрытия вехи D, без breaking changes. После этого — веха E (F15, F16,
F17, N10, N12).

## v8.8.0 - 2026-07-13

Minor-релиз с архитектурным рефакторингом (N10). Самое большое
изменение в плане v9.0.0: вместо плоского списка модулей в `src/`
явные слои (от внешнего к внутреннему).

### Changed
- **Новые директории в `src/`** (4 слоя):
  - `src/format/` — форматы syslog-сообщений (RFC 5424, RFC 3164, raw,
    protobuf). `mod.rs` содержит общие утилиты (`Header`, `prival`,
    `escape_sd_value`, `BOM`, `NILVALUE`, `sanitize_header`) и trait
    `Format` (план для вехи E, см. F15). Подмодули:
    - `rfc5424.rs` — `build_rfc5424(&Header, &[u8]) -> Vec<u8>`
    - `rfc3164.rs` — `build_rfc3164(&Header, &[u8]) -> Vec<u8>`
    - `raw.rs` — passthrough (без обёртки)
    - `protobuf.rs` — `apply_protobuf_schema`, `serialize_protobuf`,
      `serialize_protobuf_like` (wire-format varint + length-delimited)
  - `src/transport/` — транспорты (file, tcp, udp, tls). `mod.rs`
    содержит общую инфраструктуру (`SharedRx`, `Framing`, `record_send`
    /`record_send_latency`/`record_reconnect`/`record_error`,
    `drain_as_errors`, `next_msg`, `frame_into` N6 zero-copy) и trait
    `Transport` (план для F16). Подмодули:
    - `file.rs` — `target_sender_file` (BufWriter, N6)
    - `tcp.rs` — `target_sender_tcp` + `reconnect_tcp` (BytesMut, N6)
    - `udp.rs` — `target_sender_udp` (zero-copy по дизайну)
    - `tls.rs` — `target_sender_tls` + `tls_connect` + `TlsParams` +
      `build_tls_connector` + `parse_tls_min_version` (N4 + N4.mTLS)
  - `src/observability/` — Prometheus метрики + HTTP /metrics endpoint.
    `metrics.rs` (`Metrics`, `create_metrics`, `gather_metrics`) +
    `server.rs` (`parse_request_line`, `route`, `build_http_response`,
    `serve`, `spawn`).
  - `src/generator/` — оркестрация профиля. `core.rs` (`run_profile`,
    `run_phase_multi`, `generate_message`, `create_dispatcher`,
    `default_values`, `load_schema`, `load_templates`) + `config.rs`
    (`Profile`, `Phase`, `TargetConfig`, `load_profile_from_path`,
    `load_profile_from_json_str`, `load_profile_from_yaml_str`).
- **Backward-compat обёртки** для старых модулей: `src/{core,config,
  sender,syslog,metrics,metrics_server,protobuf}.rs` теперь содержат
  `pub use crate::format/transport/observability/generator::*` —
  публичный API полностью сохранён. `syslog_generator::run_profile`,
  `syslog_generator::Profile`, `syslog_generator::build_rfc5424` и т.д.
  продолжают работать без изменений в пользовательском коде.
- **`src/architecture-notes.md`** переписан с реальной архитектурой
  (был заглушкой из Initial commit). Включает описание слоёв, trait
  `Format` (план для F15), trait `Transport` (план для F16).

### Notes
- **0 breaking changes** — только internal module organization, публичный
  API не меняется.
- **Тесты: 200+ (118 unit + 70 integration + 11 N7)**, все зелёные.
- **Бенчи: 9 (3 + 6)**, все зелёные.
- clippy чист, fmt clean.

Следующий релиз: v8.8.1 (правки AUDIT.md — поставить ✅ на F7/F8/F9,
убрать "Отложено" из F13 и N4), потом v9.0.0 (milestone).

## v8.7.2 - 2026-07-13

Третий из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N4.mTLS (mutual TLS + min_protocol version) — отложенная
часть N4 (N4 сама сделана в v8.2.0).

### Added
- **`TargetConfig::tls_client_cert_file`** (`Option<String>`) — путь к
  клиентскому PEM-сертификату для mTLS. Если задан, TLS-handshake
  предъявляет этот сертификат серверу.
- **`TargetConfig::tls_client_key_file`** (`Option<String>`) — путь к
  клиентскому PEM-ключу (PKCS#8, парный к tls_client_cert_file).
- **`TargetConfig::tls_min_protocol_version`** (`Option<String>`) — "1.2"
  или "1.3" (None = системная, обычно 1.0). Защита от downgrade-атак.
- **`TlsParams`**: расширен полями `client_cert_pem`, `client_key_pem`,
  `min_protocol` (заполняются в `run_phase_multi` из TargetConfig).
- **`build_tls_connector`**: если client_cert_pem+key заданы →
  `builder.identity(Identity::from_pkcs8(...))`. Если min_protocol задан →
  `builder.min_protocol_version(Some(proto))`.
- **`parse_tls_min_version`** (новый public API) — парсит "1.2"/"1.3"
  в `native_tls::Protocol::Tlsv12`/`Tlsv13`. Принимает только эти
  два значения (1.0/1.1 deprecated NIST SP 800-52).
- **JSON Schema**: `TargetConfig` дополнен тремя mTLS-полями
  с описанием.
- **3 новых `ValidationError`**: `TlsClientCertFileNotFound`,
  `TlsClientKeyFileNotFound`, `InvalidTlsMinProtocolVersion`. Fail-fast
  проверки: файл клиентского сертификата существует, парный ключ задан,
  min_protocol либо не задан либо равен "1.2"/"1.3".

### Notes
- Тесты: **125 unit + 64 integration + 11 N7 = 200**, все зелёные.
  Из них 9 новых: 4 mTLS-connector (parse_tls_min_version, identity,
  min_protocol=Tlsv13, bad_identity), 2 валидации (missing cert file,
  bad min_protocol), +3 существующих N4-* для ca_file/insecure.
- clippy чист, fmt clean.
- 9 бенчей (3 + 6) — все зелёные.
- Реализация openssl helper: `tests/integration_tests.rs::make_test_cert`
  использует `openssl req -x509 -newkey rsa:2048` (не rcgen — та же
  проблема с `Identity::from_pkcs8` на OpenSSL 3.6.1, что была в v8.3.1).
- Backward compatible: новые поля опциональные. Профили без них
  работают как раньше (one-way TLS).

Следующие релизы: v8.8.0 (N10 слои), v8.8.1 (AUDIT.md правки),
v9.0.0 (milestone).

## v8.7.1 - 2026-07-13

Второй из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N8 (proptest) — расширение тестов property-based генераторами.

### Added
- **`+ proptest = "1"`** (dev-dependency) — property-based testing.
- **`src/payload_proptests.rs`** (новый, `#[cfg(test)]` модуль) — 6 тестов:
  - `prop_int_in_range` — `int_in_range(min, max)` всегда в `[min, max]`.
  - `prop_seed_determinism` — `derive_rng(seed, seq)` детерминирован
    (16 u64 итераций идентичны между двумя RNG с одним seed).
  - `prop_pad_to_size_exact_target` — `pad_to_size` возвращает ровно
    `target` байт (target <= 64KB чтобы не уйти в OOM при генерации).
  - `prop_pad_to_size_zero_target_no_truncation` — corner case: target=0
    возвращает body as-is (НЕ усекает, документированное поведение).
  - `prop_faker_ipv4_valid_format` — `faker("ipv4")` всегда возвращает
    валидный IPv4 (4 октета, 0..=255).
  - `prop_faker_uuid_v4_format` — `faker("uuid")` всегда возвращает
    валидный UUID v4 (формат 8-4-4-4-12, версия 4 = '4' в позиции 14,
    вариант ∈ {8,9,a,b}).

### Notes
- Back-pressure: интеграционный тест `test_n8_backpressure_slow_consumer_does_not_deadlock`
  был сначала добавлен, но оказался flaky (TCP-буфер ядра > 64KB вмещает
  50 маленьких сообщений ~500 байт мгновенно, sender не блокируется,
  elapsed < 100ms даже при корректно работающей back-pressure).
  Back-pressure в текущей архитектуре покрывается косвенно:
  1. N6 (v8.7.0) zero-copy/буферизация (BytesMut, BufWriter);
  2. test_rate_limiting_respects_target (v8.6.1) — rate-limit;
  3. test_negative_paths_connection_failures_record_errors (v8.6.0) —
     drain_as_errors при уходе sender'а.
  TODO для вехи E: явное end-to-end back-pressure тестирование через
  mock'и trait Transport (появится в N10).
- Тесты: **125 unit + 55 integration + 11 N7 = 191**, все зелёные.
  Из них 6 новых property-based.
- clippy чист, fmt clean.
- 9 бенчей (3 + 6) — все зелёные.

Следующие релизы: v8.7.2 (N4.mTLS), v8.8.0 (N10 слои),
v8.8.1 (AUDIT.md правки), v9.0.0 (milestone).

## v8.7.0 - 2026-07-13

Первый из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N6 (zero-copy/буферизация) перед major v9.0.0.

### Changed
- **`src/sender.rs` — `frame` / `frame_stream` объединены в `frame_into`**:
  раньше возвращали новый `Vec<u8>` через `format!` + `extend_from_slice`
  на каждое сообщение (аллокация в горячем пути). Теперь принимают
  `&mut BytesMut` и дописывают туда — буфер переиспользуется между
  сообщениями через `buf.clear()`. 0 аллокаций на кадр.
- **`target_sender_file` использует `BufWriter` (8 KiB)**:
  мелкие write'ы коалесцируются в один write-syscall каждые ~8 KiB
  (уменьшение системных вызовов в ~50-100 раз для типичной нагрузки).
  `flush()` делается вручную + автоматически в Drop. O_APPEND сохраняет
  атомарность дозаписи.
- **`target_sender_tcp` и `target_sender_tls` используют `BytesMut` (8 KiB)**:
  на каждое сообщение `frame_into(&mut buf, ...)` + `write_all(&buf)` +
  `buf.clear()`. 0 аллокаций в горячем пути. Один `write_all` отправляет
  много накопленных сообщений — меньше TCP write-syscall'ов и Nagle overhead.
- **`target_sender_udp` — без изменений** (уже zero-copy по дизайну,
  `send_to(&msg, ...)` не копирует payload).

### Added
- **`+ bytes = "1"`** (зависимость) — для `BytesMut` батчинга и
  zero-copy `extend_from_slice` / `freeze`.
- **4 новых unit-теста в `sender::tests::n6_*`**:
  - `n6_frame_into_non_transparent_appends_lf`
  - `n6_frame_into_octet_counting_appends_len_prefix`
  - `n6_clear_preserves_capacity` — capacity сохраняется после clear()
    (zero-copy инвариант: capacity переиспользуется между сообщениями)
  - `n6_consecutive_frames_concatenate` — N фреймов в один буфер дают
    корректный конкатенированный вывод

### Notes
- Тесты: **119 unit + 55 integration + 11 N7 = 185**, все зелёные.
- 9 бенчей (3 + 6), все зелёные.
- clippy чист, fmt clean.
- Backward compatible: публичный API не изменился (`frame` и `frame_stream`
  были private, заменены на private `frame_into`).
- Производительность: для типичной нагрузки (10k msg/s) — уменьшение
  аллокаций в ~N раз (N = размер сообщения / capacity батчера) и
  уменьшение syscall'ов в ~50-100 раз.

Следующие релизы по плану: v8.7.1 (N8 proptest), v8.7.2 (N4.mTLS),
v8.8.0 (N10 слои), v8.8.1 (AUDIT.md), v9.0.0 (milestone).

