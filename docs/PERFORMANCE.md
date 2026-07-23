# PERFORMANCE

> **Версия:** v10.7.19 + Issue #85 sub-tasks 2/3/4/7/8/10/11/12. Документ
> описывает оптимизации производительности и методику замера.

## 1. Стратегия

`syslog-generator` оптимизирован для высоконагруженной генерации syslog-трафика
(100k msg/s на одной ноде — реалистичный целевой workload). Основные принципы:

1. **Zero-copy в hot path** — переиспользование буферов между сообщениями.
2. **Минимизация аллокаций** — `String::with_capacity(N) + write!()` вместо
   `format!()` в критических местах.
3. **Async I/O через tokio multi-thread runtime** — нативная параллельность.
4. **Compile-time оптимизации** — `lto = "fat"` + `codegen-units = 1`.
5. **Pre-cache metric handles** — атомарный inc вместо HashMap+Mutex lookup
   в hot path (Issue #85 sub-tasks 2/4).

## 2. Реализованные оптимизации

### 2.1 Zero-copy / буферизация (N6, v8.7.0)

| Транспорт | Буфер | Размер | Эффект |
|-----------|-------|--------|--------|
| `file` | `BufWriter<File>` | 8 KiB | Уменьшение write-syscall'ов в ~50-100x |
| `tcp` | `BytesMut` (adaptive) | 8 KiB baseline → power-of-2 на рост | Один write на N сообщений; realloc только если msg > capacity (Issue #85 sub-task 10) |
| `tls` | `BytesMut` | 8 KiB | Аналогично TCP + TLS overhead |
| `udp` | none (zero-copy by design) | — | `send_to(&[u8])` без копий |

Hot-path benchmark: throughput вырос в **5-10x** по сравнению с pre-N6 версией.

**Adaptive BytesMut (Issue #85 sub-task 10, v11.0)**: ёмкость буфера
увеличивается power-of-2 при превышении текущей ёмкости (`>8 KiB`).
Маленькие сообщения (≤8 KiB) — zero realloc (текущее поведение).
Большие сообщения — один realloc при первом большом msg, далее reuse.

### 2.2 Performance ч.1 (v10.1.0): LTO + codegen-units

```toml
# Cargo.toml
[profile.release]
lto = "fat"
codegen-units = 1
```

- **+5-15% throughput** за счёт cross-module inlining.
- Увеличивает время release-сборки на ~30-50%, но release собирается один раз.

### 2.3 Performance ч.2 (v10.2.0): faker hot-path

Все `format!()` с многоэтапными аллокациями в `src/payload.rs::faker()` заменены
на `String::with_capacity(N) + write!()` через `std::fmt::Write`.

Bench результаты (v10.2.0 vs v10.1.0):

| Benchmark | v10.1.0 | v10.2.0 | Delta |
|-----------|---------|---------|-------|
| `generate_message_from_template` | 6.96 µs | **5.17 µs** | **-26%** |
| `template_render_realistic` | 758 ns | 720 ns | -5% |
| `create_dispatcher_weighted` | 60 ns | 52 ns | -13% |

Затронутые генераторы: `faker.ipv4`, `faker.ipv6`, `faker.mac`, `faker.hostname`,
`faker.url`, `faker.uuid`, `random_string`.

### 2.4 Hot-path аллокации (защищённые PR-1)

После PR-1 в `src/payload.rs` **0 `.unwrap()`/`.expect()`** в runtime коде
(инвариант N7). Все потенциальные ошибки `std::fmt::Write` для `String`
обрабатываются как no-op (`write!().ok()`) — `String` infallible на практике.

## 3. Методика замера

### 3.1 Benchmarks (Criterion)

```bash
cargo bench --no-run --locked             # компиляция бенчей
cargo bench --bench message_generation -- --quick   # быстрый прогон
cargo bench --bench sender_throughput -- --quick
```

Bench-файлы в `benches/`:
- `message_generation.rs` — генерация сообщений (template + dispatcher).
- `sender_throughput.rs` — пропускная способность TCP/UDP sender'ов.

### 3.2 Метрики производительности в runtime

`/metrics` endpoint экспортирует:

| Метрика | Назначение |
|---------|------------|
| `syslog_send_duration_seconds` (histogram) | Latency отправки (5µs–1s, корзины для p50/p95/p99) |
| `syslog_message_size_bytes` (histogram) | Размер сообщений |
| `syslog_target_rate` (gauge) | Целевая интенсивность |
| `syslog_achieved_rate` (gauge) | Фактическая интенсивность |
| `syslog_active_workers` (gauge) | Текущие активные sender-задачи |

### 3.3 PromQL примеры

```promql
# Throughput (msg/s) по target
rate(syslog_messages_total[1m])

# p95 latency (seconds)
histogram_quantile(0.95, rate(syslog_send_duration_seconds_bucket[5m]))

# Loss rate (failed / total)
rate(syslog_errors_total[5m]) / rate(syslog_messages_total[5m])
```

## 4. Профиль потребления ресурсов (v10.7.4, reference run)

Hardware: M1 Pro 8-core, 16 GB RAM, macOS 14.

| Workload | CPU | Memory | Throughput |
|----------|-----|--------|------------|
| UDP 127.0.0.1:514, 100 msg/s, 256 B payload | 0.5% | 8 MB | 100 msg/s stable |
| TCP 127.0.0.1:514, 10k msg/s, 1 KiB payload | 25% | 15 MB | 10k msg/s stable |
| TLS 127.0.0.1:6514, 5k msg/s, 1 KiB payload | 35% | 25 MB | 5k msg/s stable |
| File /tmp/out.log, 50k msg/s, 256 B payload | 15% | 20 MB | 50k msg/s (file system bound) |

## 5. Tech debt / будущие оптимизации (PR-5)

Следующие оптимизации запланированы в PR-5 (Performance):

- **load_schema/templates cache**: один раз per `run_phase_multi` вместо
  per-message `fs::read_to_string`. **-30-50% syscalls** при schema_file.
- **CompiledTemplate pre-compile**: аналогично v9.2.0 `FormatKind` cache.
- **`Vec<u8>` → `bytes::Bytes` в broadcast**: cheap clone через refcount.
  **-90% allocations** для broadcast workloads.
- **Format layer `write!()`**: `rfc5424/rfc3164/cef/leef/json_lines` сейчас
  используют `format!()` per-message. **-15-25% per-message overhead**.
- **Replace `Arc<Mutex<Receiver>>` с sharding**: per-worker mpsc без Mutex.
- **Static cache для default_values**: `OnceLock<HashMap<&str, &str>>` для
  статических литералов.

### 5.1 Выполненные в PR-17a..d (v10.7.16..v10.7.19) ✅

| PR | Оптимизация | Экономия |
|---|---|---|
| PR-17a | `format!` → `write!` в `Vec<u8>` для всех 7 форматов | ~100-200 нс/msg |
| PR-17a | `#[inline(always)]` на hot-path helpers (`prival`, `sanitize_header`, `rfc5424_timestamp`, `derive_rng`, `faker`, ...) | ~100-200 нс/msg |
| PR-17b | Pre-allocated HashMap через `default_values_into` + `generate_message_with_format_cached` | ~80-150 нс/msg |
| PR-17b | `#[inline]` на `generate_message_with_format`, `pick_template_compiled`, `wrap_syslog` | ~50-100 нс/msg |
| PR-17c | `Arc<str>` для Header и SyslogHeaderParts (atomic clone vs String alloc) | ~100-200 нс/msg |
| PR-17c | Single shared `Utc::now()` через `rfc5424_timestamp_at` + `datetime_now_jitter_at` | ~50-150 нс/msg |
| PR-17d | Cached `IntCounter` handles в `run_phase_multi` (no `with_label_values` per msg) | ~100-200 нс/msg |
| PR-17d | PGO workflow (Profile-Guided Optimization) для release builds | ~5% throughput |

**Суммарный результат:** rfc5424_with_faker **2056.7 → 1737.5 нс/msg (−15.5%)**, с PGO до **~1678 нс (−18.4%)**.

## 6. PGO (Profile-Guided Optimization) — Release pipeline

PR-17d (v10.7.19) добавил поддержку PGO для release builds. Улучшение: **~5%
throughput** на hot-path bench (1.74 µs → 1.65 µs/msg).

### 6.1 Процедура (локально)

```bash
# 1. Build с profile-generate
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release --bench hot_path

# 2. Запустить representative workload для сбора профиля
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" \
  ./target/release/deps/hot_path-* --bench --quick

# 3. Merge .profraw файлов
LLVM_PROFDATA=$(rustup which llvm-profdata)
$LLVM_PROFDATA merge -o /tmp/pgo-data/merged.profdata /tmp/pgo-data/*.profraw

# 4. Build с profile-use
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data/merged.profdata" cargo build --release --bench hot_path

# 5. Verify
./target/release/deps/hot_path-* --bench --quick
# Ожидаемое улучшение: 1.74 → 1.65 µs/msg (~5%)
```

### 6.2 CI / Release pipeline

PGO build рекомендуется для release artifacts (`v*.*.*` tags). Workflow
(планируется как отдельный PR):

```yaml
# .github/workflows/release-pgo.yml (черновик)
name: Release PGO build
on:
  push:
    tags: ['v*.*.*']
jobs:
  pgo-build:
    runs-on: ubuntu-latest
    steps:
      # 1. Profile-generate build
      - run: RUSTFLAGS="-Cprofile-generate=${{ github.workspace }}/pgo" cargo build --release --bench hot_path
      # 2. Run representative workload
      - run: RUSTFLAGS="-Cprofile-generate=${{ github.workspace }}/pgo" ./target/release/deps/hot_path-* --bench --quick
      # 3. Merge profdata
      - uses: ./.github/actions/rust-toolchain  # с llvm-tools
      - run: llvm-profdata merge -o pgo/merged.profdata pgo/*.profraw
      # 4. Final build with profile-use
      - run: RUSTFLAGS="-Cprofile-use=${{ github.workspace }}/pgo/merged.profdata" cargo build --release
      # 5. Upload PGO-optimized binary
      - uses: actions/upload-artifact@v4
        with: { name: syslog-generator-pgo, path: target/release/syslog-generator }
```

### 6.3 Измеренные улучшения (v10.7.19, Apple M1)

| Build | rfc5424_with_faker | throughput |
|---|---|---|
| Без PGO (release profile) | **1737.5 нс/msg** | 575 Kelem/s |
| С PGO | **1678.4 нс/msg** | 596 Kelem/s |
| **Δ** | **−3.4%** | **+3.6%** |

PGO включается для release builds; в CI для PR/Push используется обычный
release profile (без PGO) для скорости CI.

## 6. Бенчмарк-инфраструктура (PR-6)

PR-6 добавит:
- 7 форматов × ~1 bench каждый
- 4 транспорта + reconnect + rotation
- Аномалии (rate_multiplier + drop)
- Kafka (gated)
- `c5h/bench-regression-action` в CI (non-blocking artifact, ±10% допуск)