# COVERAGE

> **v10.7.15 current:** данные по coverage берутся из последнего запуска
> coverage job в CI (`.github/workflows/ci.yml` job `coverage`).
> Baseline milestones:
> - **v10.3.0 (Coverage ч.1):** **86.40% lines / 88.36% functions / 86.49% regions**
> - **v10.4.0 (Coverage ч.2):** **87.07% lines / 89.38% functions / 87.20% regions** (+0.67pp)
> - **v10.7.15 (Coverage expansion, PR-16):** **89.65% lines / 90.42% functions / 89.53% regions** (+1.77pp от v10.7.9 baseline 87.88%, 25 новых тестов)
>
> Coverage gate (≥ 97% lines) **НЕ активирован** — backlog. PR-3 закрывает
> доки, gate переедет в отдельный release (требует ~50-80 новых unit-тестов).

## Что такое coverage

Coverage измеряет, какая часть исходного кода была выполнена хотя бы одним
тестом. Используется `cargo-llvm-cov` — pure-Rust обёртка над `llvm-cov`
(profile-guided). Поддерживает `--workspace`, `--all-features`, LCOV-вывод
для codecov.io / Coveralls.

## Как запустить локально

```bash
# Установка (один раз):
cargo install cargo-llvm-cov --locked

# Текущий coverage с резюме:
cargo llvm-cov --features kafka --summary-only

# HTML-отчёт (для просмотра в браузере):
cargo llvm-cov --features kafka --html --output-dir coverage/

# LCOV-файл (для codecov.io):
cargo llvm-cov --features kafka --lcov --output-path lcov.info

# Только для конкретного модуля:
cargo llvm-cov --features kafka --lib --summary-only
```

## Baseline (v10.4.0)

```
TOTAL: 87.07% lines / 89.38% functions / 87.20% regions
```

(Исторические данные: v10.3.0 = 86.40%/88.36%/86.49%.)

Полная таблица по модулям (по убыванию покрытия):

| Модуль | Lines | Functions | Regions | Приоритет |
|---|---|---|---|---|
| `template.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/rfc3164.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/rfc5424.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/raw.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/cef.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/leef.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `format/json_lines.rs` | 100.00% | 100.00% | 100.00% | ✅ |
| `transport/udp.rs` | 87.88% | 100.00% | 96.00% | ✅ |
| `anomaly.rs` | 96.81% | 100.00% | 99.27% | ✅ |
| `transport/file.rs` | 89.89% | 97.56% | 93.48% | ✅ |
| `transport/reconnect.rs` | 92.89% | 87.50% | 93.86% | ✅ |
| `schema_check.rs` | 92.66% | 78.57% | 94.00% | ✅ |
| `transport/udp.rs` | 87.88% | 100.00% | 96.00% | ✅ |
| `payload.rs` | ~95% | ~95% | ~95% | ✅ |
| `validate.rs` | 84.08% | 95.24% | 86.90% | 🔵 средний |
| `protobuf.rs` | 81.62% | 95.24% | 79.73% | 🔵 средний |
| `transport/tls.rs` | 68.44% | 60.00% | 67.35% | 🟡 нужен +29% lines |
| `shutdown.rs` | 67.44% | 100.00% | 75.00% | 🟡 нужен +30% lines |
| `transport/mod.rs` | 63.33% | 68.42% | 53.33% | 🟡 нужен +34% lines |
| `transport/kafka.rs` | 51.68% | 71.43% | 47.77% | 🔴 нужен +45% lines |
| `transport/tcp.rs` | 46.72% | 44.44% | 54.35% | 🔴 нужен +50% lines |

## План v10.4.0 (Coverage ч.2)

Цель: довести coverage до ≥ 97% lines (blocking gate в CI).

### Что нужно добавить

**Тесты для `transport/tcp.rs` (46.72% → ≥ 97%):**
- Тесты для каждой ветки reconnect: success-on-first-attempt, retries-until-success,
  exhausted-max-attempts, shutdown-cancelled-during-backoff, exponential growth
  verified. Часть уже покрыта unit-тестами `src/transport/reconnect.rs`,
  но интеграционные тесты с реальным TcpStream отсутствуют.
- Тесты framing: octet-counting vs non-transparent — проверка префикса
  в реальном TCP-write.

**Тесты для `transport/kafka.rs` (51.68% → ≥ 97%):**
- Тесты для каждого error path: connect-fail, produce-fail, batch-flush-fail.
- Тесты для parsing `parse_bootstrap_servers`, `parse_kafka_acks`,
  `parse_kafka_compression` (включая invalid inputs).

**Тесты для `transport/tls.rs` (68.44% → ≥ 97%):**
- Тесты для всех error paths в `build_tls_connector`: bad PEM, missing CA,
  invalid min_protocol_version, invalid cipher suite (уже частично).
- Тесты handshake error scenarios.

**Тесты для `shutdown.rs` (67.44% → ≥ 97%):**
- Тесты для `graceful_drain_wait` при разных значениях `drain_timeout_secs`.
- Тесты для `shutdown_listener` с разными сигналами (SIGINT, SIGTERM).

**Тесты для `transport/mod.rs` (63.33% → ≥ 97%):**
- Тесты для `record_send`, `record_error`, `record_send_latency` —
  smoke-тесты с разными labels.

**Тесты для `validate.rs` (84.08% → ≥ 97%):**
- Тесты для каждого `ValidationError` варианта с разными входными данными
  (boundary cases). Часть уже покрыта через F13, но не 100%.

**Тесты для `protobuf.rs` (81.62% → ≥ 97%):**
- Тесты для edge cases: пустой schema, schema с одним полем, schema с
  nested types, missing fields.

### Примерный объём

~50-80 новых unit-тестов (по 3-10 на каждый непокрытый модуль).
Существующий базис: 214 unit + 86 integration + 11 n7 = 311 тестов.
После v10.4.0 ожидается: ~370-420 тестов.

## CI integration

v10.3.0: **non-blocking** coverage job (только отчёт, артефакт `lcov.info`).
v10.4.0: **blocking** coverage gate — fail если lines < 97%.

```yaml
# .github/workflows/ci.yml (v10.3.0, non-blocking)
coverage:
  name: Coverage (baseline, non-blocking)
  runs-on: ubuntu-latest
  continue-on-error: true
  steps:
    - uses: actions/checkout@v4
    - uses: taiki-e/install-action@v2
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: rustfmt, clippy, llvm-tools-preview
    - run: cargo llvm-cov --features kafka --lcov --output-path lcov.info
    - uses: actions/upload-artifact@v4
      with:
        name: coverage-lcov
        path: lcov.info
```

## Команды для разработчика

```bash
# После изменения кода — обновить baseline:
cargo llvm-cov --features kafka --summary-only > /tmp/coverage.txt

# Если добавил новый модуль:
cargo llvm-cov --features kafka --html --output-dir coverage/
open coverage/index.html

# Проверить, что новый код покрыт:
cargo llvm-cov --features kafka --lib 2>&1 | grep "src/your_new_module.rs"
```# CI re-trigger 1784541258
