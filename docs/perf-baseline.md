# Perf Baseline

> PR-A0 (v10.8.0): зафиксированные baseline-цифры runtime benches.
> Обновлено: 2026-07-23.
> Hardware: Apple M1, Criterion `--quick` (~10 sample size).

## Цель

Baseline — это измерения реального `run_profile`, а не только `hot_path` bench.
Hot-path показывает per-message overhead в наносекундах; runtime — end-to-end
throughput (msg/s) с pacing, transport, metrics.

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

## Profile

С PR-A0 введён `[profile.bench] inherits = "release"` с `debug=false`. Bench
компилируется с LTO + opt-level=3 как release. Дефолтные `bench`-профили в
Cargo используют `dev`-настройки; без `inherits = "release"` benches
измеряли бы debug-режим (≈ −50% throughput).

## Baseline v10.8.0-A0 (Apple M1, --quick)

### `runtime` (file transport, 20 000 msg, unlimited rate, seed=42)

| Bench | time (median) | throughput |
|---|---|---|
| `runtime/rfc5424_static`     | ~50 ms | ~400 K msg/s |
| `runtime/rfc5424_faker`      | ~52 ms | ~385 K msg/s |
| `runtime/rfc3164_static`     | ~42 ms | ~474 K msg/s |
| `runtime/json_lines_static`  | ~56 ms | ~357 K msg/s |

### `format_matrix` (per-message, ns/msg, seed=42)

| Bench | time | throughput | Delta vs initial bench |
|---|---|---|---|
| `rfc5424_static`    | 1202 ns | 832 K msg/s | −39% (referenced_fakers fix) |
| `rfc3164_static`    | 1495 ns | 669 K msg/s | −37% |
| `raw_static`        | 1024 ns | 977 K msg/s | −45% |
| `protobuf_static`   |  677 ns | 1477 K msg/s | −66% |
| `cef_static`        | 1003 ns | 997 K msg/s | −44% |
| `leef_static`       | 1011 ns | 990 K msg/s | −43% |
| `json_lines_static` | 1541 ns | 649 K msg/s | −34% |
| `rfc5424_faker`     | 1558 ns | 642 K msg/s | −1% (already faker-aware) |
| `json_lines_faker`  | 2007 ns | 498 K msg/s |  ±0% |

### `transport_matrix` (2000 msg, real listener)

| Bench | time | throughput |
|---|---|---|
| `tcp/1`  |  4.4 ms | 453 K msg/s |
| `tcp/4`  |  5.2 ms | 387 K msg/s |
| `tcp/16` |  5.9 ms | 337 K msg/s |
| `udp/1`  |  9.0 ms | 221 K msg/s |
| `udp/4`  |  6.2 ms | 324 K msg/s |

### `dispatch_matrix` (10 000 msg, file transport, seed=42)

| Bench | time | throughput |
|---|---|---|
| `rr/1`           | ~48 ms | ~210 K msg/s |
| `rr/4`           | ~31 ms | ~320 K msg/s |
| `rr/16`          | ~36 ms | ~280 K msg/s |
| `weighted/1`     | ~28 ms | ~360 K msg/s |
| `weighted/4`     | ~33 ms | ~300 K msg/s |
| `weighted/16`    | ~28 ms | ~360 K msg/s (non-uniform: 70/20/...) |
| `broadcast/1`    | ~20 ms | ~498 K msg/s |
| `broadcast/4`    | ~34 ms | ~298 K msg/s |
| `broadcast/16`   | ~96 ms | ~104 K msg/s (serial `send().await`) |

## Acceptance criteria для PR-A1+

- `hot_path/rfc5424_with_faker`: ≤ −10% ns/msg (baseline: 1783 ns → target ≤ 1605 ns)
- `runtime/rfc5424_static`: ≤ −5% wall-time (baseline: ~400 K msg/s → target ≥ 420 K)
- `format_matrix/rfc5424_static`: ≤ −10% ns/msg (baseline: 1202 ns → target ≤ 1082 ns)

## Наблюдения из baseline

1. **`tcp/16` падает в 1.3×** vs `tcp/1` (453→337 K msg/s) — SharedRx mutex
   contention подтверждена, но меньше ожидаемого. PR-A4 (per-worker channels)
   устранит остаток.
2. **`broadcast/16` падает в 5×** vs `broadcast/1` — сериальный `send().await`
   подтверждён. Цель PR-A3.
3. **`protobuf_static` теперь самый быстрый** (677 ns, 1477 K msg/s) — после
   fix faker-scan это пустой protobuf без overhead.
4. **`rfc3164_static` быстрее `rfc5424_static`** в runtime — нет tag/procid
   wrapping overhead, формат компактнее.
5. **`referenced_fakers` fix дал −34..−66% на static templates** — это
   самое большое открытие baseline. Без него все 9 fakers генерировались
   даже для шаблонов без `{{faker.*}}`.

## Известные ограничения

- `transport_matrix/udp/16` пропущен в `--quick` (deadlock при быстром send):
  bench остаётся доступным через `cargo bench --bench transport_matrix` без `--quick`.
- `dhatch_heap` (PR-A0 issue #81) не реализован в этом PR — добавлен в backlog
  PR-A6 (perf governance) для DHAT integration.

## Roadmap

- PR-A0: benches + baselines + скрипты + CI workflow (этот PR).
- PR-A1: hot-path micro-optics → ожидаемый прирост на faker-насыщенных шаблонах.
- PR-A2: CompiledPlan → ожидаемый −30–50% allocations/msg (DHAT будет в PR-A6).
- PR-A6: blocking CI gate на основе этих baselines + DHAT integration.
