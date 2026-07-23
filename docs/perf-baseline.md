# Perf Baseline

> PR-A0 (v10.8.0): зафиксированные baseline-цифры runtime benches.
> Обновлено: 2026-07-23.

## Цель

Baseline — это измерения реального `run_profile`, а не только `hot_path` bench.
Hot-path показывает per-message overhead в наносекундах; runtime — end-to-end
throughput (msg/s) с pacing, transport, metrics.

## Как запустить

```bash
scripts/perf-baseline.sh quick         # быстрый прогон (--quick)
scripts/perf-baseline.sh full          # полный прогон (10s measurement_time)
scripts/perf-baseline.sh update <sha>  # сохранить baseline в perf/baselines/<sha>.json
```

CI: `.github/workflows/perf-baseline.yml` запускает nightly + on-tag,
результат публикуется как non-blocking artifact.

## Bench matrix

| Bench | Назначение | Файл |
|---|---|---|
| `hot_path` | per-message overhead, ns/msg | `benches/hot_path.rs` |
| `runtime` | end-to-end `run_profile` (file transport) | `benches/runtime.rs` |
| `format_matrix` | 7 форматов × {static, faker} | `benches/format_matrix.rs` |
| `transport_matrix` | TCP/UDP × {1, 4, 16} connections | `benches/transport_matrix.rs` |
| `dispatch_matrix` | round-robin/weighted/broadcast × {1, 4, 16} targets | `benches/dispatch_matrix.rs` |

## Метрики

| Метрика | Где | Как интерпретировать |
|---|---|---|
| `time/msg (ns)` | все benches | меньше — лучше |
| `throughput (elem/s)` | runtime/transport/dispatch | больше — лучше |
| `allocations/msg` | DHAT в `benches/dhat_runtime.rs` | меньше — лучше (PR-A1+) |
| `syscalls/msg` | perf stat (опционально) | меньше — лучше (PR-A5+) |

## Profile

С PR-A0 введён `[profile.bench] inherits = "release"` с `debug=false`. Это
гарантирует, что benches компилируются с теми же оптимизациями, что и release
(LTO, codegen-units=1, opt-level=3), без debug-информации.

## Baseline (PR-A0, v10.7.19 baseline, Apple M1 / Linux x86_64)

Измерено в quick mode (`--quick`, ~5 sample size). Абсолютные цифры зависят
от hardware; относительные delta — главная метрика между версиями.

### `runtime` (file transport, 20 000 msg, unlimited rate)

| Bench | time (median) | throughput |
|---|---|---|
| `runtime/rfc5424_static`  | 61.9 ms  | 323 K msg/s |
| `runtime/rfc5424_faker`   | 51.4 ms  | 389 K msg/s |
| `runtime/rfc3164_static`  | 57.3 ms  | 349 K msg/s |
| `runtime/json_lines_static` | 61.3 ms | 326 K msg/s |

### `format_matrix` (per-message, ns/msg)

| Bench | time | throughput |
|---|---|---|
| `rfc5424_static`    | 1957 ns | 511 K msg/s |
| `rfc3164_static`    | 2354 ns | 425 K msg/s |
| `raw_static`        | 1858 ns | 538 K msg/s |
| `protobuf_static`   | 1997 ns | 501 K msg/s |
| `cef_static`        | 1779 ns | 562 K msg/s |
| `leef_static`       | 1762 ns | 568 K msg/s |
| `json_lines_static` | 2332 ns | 429 K msg/s |
| `rfc5424_faker`     | 1578 ns | 634 K msg/s |
| `json_lines_faker`  | 2000 ns | 500 K msg/s |

### `transport_matrix` (TCP/UDP, 5000 msg each)

| Bench | time | throughput |
|---|---|---|
| `tcp/1`  | 25.4 ms | 197 K msg/s |
| `tcp/4`  | 57.9 ms |  86 K msg/s |
| `tcp/16` | 814 ms  |   6 K msg/s (SharedRx mutex contention — PR-A4 issue) |
| `udp/1`  | 35.4 ms | 141 K msg/s |
| `udp/4`  | 59.3 ms |  84 K msg/s |
| `udp/16` | 80.6 ms |  62 K msg/s |

### `dispatch_matrix` (10 000 msg, file transport)

| Bench | time | throughput |
|---|---|---|
| `rr/1`           | 48.5 ms | 206 K msg/s |
| `rr/4`           | 31.4 ms | 318 K msg/s |
| `rr/16`          | 36.0 ms | 278 K msg/s |
| `weighted/1`     | 27.8 ms | 360 K msg/s |
| `weighted/4`     | 33.3 ms | 301 K msg/s |
| `weighted/16`    | 46.3 ms | 216 K msg/s |
| `broadcast/1`    | 31.2 ms | 321 K msg/s |
| `broadcast/4`    | 45.1 ms | 222 K msg/s |
| `broadcast/16`   | 112.4 ms |  89 K msg/s (serial await per target — PR-A3 issue) |

## Acceptance criteria для PR-A1 и далее

- `hot_path/rfc5424_with_faker`: ≤ −10% ns/msg
- `runtime/rfc5424_static`: ≤ −5% wall-time
- `format_matrix/rfc5424_static`: ≤ −10% ns/msg
- allocation count (DHAT): ≤ −15% allocations/msg

## Наблюдения из baseline

1. **`tcp/16` падает в 30×** — подтверждает P14 из анализа: `SharedRx` mutex
   сериализует connections при росте пула. PR-A4 (per-worker channels)
   устранит это.
2. **`broadcast/16` падает в 3.5×** относительно `broadcast/1` —
   подтверждает P1.4 (serial `send().await` per target). PR-A3 + PR-A4
   решают через per-target queues и broadcast policy.
3. **`runtime/json_lines` ≈ `runtime/rfc5424_static`** — JSON-lines
   BTreeMap overhead компенсируется отсутствием syslog header.
4. **`rfc3164` медленнее `rfc5424`** за счёт `Local::now()` per message
   (~200 нс). PR-A1 candidate.

## Roadmap

- PR-A0: benches + baselines (этот PR)
- PR-A1: hot-path micro-optics → первый видимый прирост
- PR-A2: CompiledPlan → −30–50% allocations/msg
- PR-A6: blocking CI gate на основе этих baselines
