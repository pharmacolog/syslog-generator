# Whitepaper 2026 — benchmark harness

> **Issue:** #106 (GTM-1, milestone v11.6)
> **Что это:** reproducible harness для сравнительного benchmark'а
> **4 инструментов** из Issue #106: `syslog-generator` vs `loggen` (syslog-ng)
> vs `flog` vs `tcpkali`. kcat (kafkacat) **не** входит в сравниваемый набор —
> в v1 нет ни одного рабочего Kafka consumer'а.
> **Главный документ:** `../../docs/whitepaper-2026.md`.
> **Методология:** `METHODOLOGY.md`.
> **Версия harness:** 1.0.0

## TL;DR

```bash
# Dry-run: напечатает все команды, ничего не запуская. Всегда exit 0.
make all

# Валидация конфигов + harness self-tests (без внешних инструментов).
make validate
make harness-tests

# Реальный прогон (нужны установленные syslog-generator, loggen, flog, tcpkali).
make all REQUIRE_TOOLS=1

# Только syslog-generator (например, для release-blocker perf check).
make all TOOLS=syslog_generator

# Сгенерировать REPORT.md из perf/whitepaper-results.json.
make report
```

## Структура

| Dir | Что внутри |
|---|---|
| `Makefile` | entrypoint — все таргеты, dependency graph (build → dispatch в real mode) |
| `README.md` | этот файл |
| `METHODOLOGY.md` | fairness protocol + reproducibility contract |
| `configs/` | 4 workload-конфига (Issue #106 spec) + `env.sh` + `validate.py` |
| `scripts/` | shell/Python (stdlib-only) скрипты для запуска, нормализации, отчёта |
| `docker/` | `gen-cert.sh` (TLS cert generation) + README — **NO Dockerfiles** (no "fully containerized" claim) |
| `harness/` | self-tests harness'а (bash + Python tests) |
| `results/` | raw output каждого прогона (raw stdout/stderr + per-run JSON) |

## Что делает каждый таргет

### `make all` (default = dry-run)

1. `check-tools` — определяет какие из 4 инструментов установлены.
2. `validate` — парсит `configs/workload_*.json`, проверяет Issue #106 spec.
3. `harness-tests` — self-tests на fixtures (без инструментов).
4. `dry-run` — печатает команды для всех (workload, tool) ячеек.
5. `dispatch` — запускает runners.
6. `collect` — пишет `perf/whitepaper-results.json`.
7. `report` — генерирует `results/REPORT.md`.

### `make validate`

Только шаги 2 + 3. Не запускает инструменты.

### `make report`

Только шаг 7.

### `make clean`

Удаляет `results/*` (сохраняет schema в `perf/`).

### `make reset`

Возвращает `perf/whitepaper-results.json` к `status="schema_only"`.

## Exit code policy

| Ситуация | Exit code |
|---|---|
| `make all` (dry-run) | 0 |
| `make all REQUIRE_TOOLS=1`, все supported cells completed | 0 |
| `make all REQUIRE_TOOLS=1`, supported cell skipped (tool missing) | 2 |
| `make all REQUIRE_TOOLS=1`, supported cell failed | 1 |
| Unsupported workload/tool combination | `n/a` (не failure) |

## Конвенции

### Имена файлов

- `scripts/NN_<verb>_<subject>.sh` — NN = порядок выполнения.
- `configs/workload_<transport>_<rate>rps_<size>b.json` — UE-friendly имена.
- `results/<workload_id>/run_<idx>/<tool>.{stdout,stderr,meta.json}` — raw output.

### Переменные окружения

| Var | Default | Что делает |
|---|---|---|
| `REQUIRE_TOOLS` | `0` | Если `1` — реально запускает инструменты. |
| `SKIP_BUILD` | `0` | Пропустить `cargo build --release` (если бинарь уже есть). |
| `DURATION_SECS` | `30` | Длительность каждого workload. |
| `RUNS` | `1` | Replications per cell. |
| `RATE_TOLERANCE_FRACTION` | `0.05` | 95–105% gate |
| `SIZE_TOLERANCE_FRACTION` | `0.05` | ±5% size gate |
| `HARNESS_RATE` | (none) | Pacing для syslog-generator (--rate) |
| `PRESERVE_RESULTS` | `0` | Если `1` — не очищать `results/` перед прогоном. |
| `WP_RESULTS` | `../../perf/whitepaper-results.json` | Куда пишем schema/status. |
| `WP_REPORT` | `results/REPORT.md` | Куда пишем markdown-отчёт. |

## Что НЕ делает harness

- **Не запускает** loggen/flog/tcpkali если они не установлены.
- **Не придумывает** measurement'ы — `collect.py` явно проверяет, что
  `runs[i].measurements` заполняется только если `runs[i].status == "completed"`.
- **Не трогает** Rust-файлы, `Cargo.toml`, `CHANGELOG.md`. Harness
  использует только `cargo build --release` (если `SKIP_BUILD=0`),
  никаких изменений в source tree.
- **Не пушит и не коммитит** — `make all` локальный.

## Известные ограничения

1. На macOS `git`/`make`/`python3` есть везде, `cargo` — через brew или
   rustup. `docker` — optional (для воспроизведения в CI).
2. На Ubuntu-latest (GH Actions runner) `loggen`/`flog`/`tcpkali`
   не установлены — это OK, harness отметит их как unavailable.
3. TLS workload требует self-signed cert. `bash docker/gen-cert.sh`.
4. Kafka workload — все 4 tools = N/A в v1.
5. Latency p50/p95/p99 измеряется только для syslog-generator (через
   `--metrics-addr` Prometheus). Для external tools latency = `null`.
6. **No fabricated measurements.** Issue #106 явно требует честности.
   Invariant `no_fabricated_measurements` enforced в `harness/test_collect.py`.
7. **No "fully containerized" claim.** Dockerfiles с guessed versions
   удалены; см. `docker/README.md`.
8. **No warmup в v1.** Window = sender runtime. Используйте `HARNESS_RATE`
   для pacing short tests.
