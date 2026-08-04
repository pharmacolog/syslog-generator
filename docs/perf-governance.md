# Performance Governance (v11.8+)

> **Status**: Active since v11.8 (Issue #164, A6 gap-closing).
> **Owners**: maintainer + AI agents (per `AGENTS.md`).

## Цель

Performance regression gate защищает main от неконтролируемых регрессий
в hot-path, format, transport, allocations. Gate **blocking** с момента
merge Issue #164 (`feat(perf): make perf-regression gate truly blocking`).

## Thresholds (since v11.9, Issue #214 — pragmatic v11.9 + Issue #218)

| Категория | Threshold (v11.9) | Threshold (target v12.0) | Что покрывает |
|---|---|---|---|
| `hot_path/` | **+50%** | +5-10% | `benches/hot_path.rs` — message generation hot-path |
| `format/` | **+50%** | +10-15% | `benches/format/*.rs` — rfc5424/rfc3164/cef/leef/json encoding |
| `transport/` | **+50%** | +10-15% | `benches/transport/*.rs` — TCP/UDP/TLS/file-rotation |
| `allocations/` | **+50%** | +15-20% | `benches/allocations.rs` — alloc profile (msg/sec через hot-path/format/transport/payload) |

**History**:
- v11.8 (Issue #164) — gate blocking, thresholds = 50% (cold cache variance ±30-50%).
- v11.9 (Issue #214) — median-of-3-runs implementation. **Thresholds остаются 50%**
  (effectively noop) — systematic CI variance ±15-20% даже после median aggregation.
- v12.0 (Issue #218, deferred) — investigation + alternative framework (bencher / paired A/B)
  для reduction variance до реалистичных 5-10%.

**Improvement > threshold** печатается в `Improvements` (informational,
не влияет на gate status).

## Как работает gate

1. PR открыт в `main` или `dev`.
2. `perf-regression.yml` запускает **Warm-up** step (`cargo bench -- --warm-up-time 1`)
   для cold cache mitigation.
3. Затем запускает **median-of-3-runs** через `scripts/perf-regression-collect.sh`:
   ```
   BENCH_RUNS=3 scripts/perf-regression-collect.sh \
       "perf/baselines/current-${CURRENT_SHA}.json" \
       hot_path allocations
   ```
   Это запускает `cargo bench` 3 раза для каждого bench target, сохраняет
   estimates каждого run в `${WORKDIR}/run-N.jsonl`, затем
   `scripts/compute-median.py` агрегирует median per benchmark.

   **Bench targets** (Issue #211 добавил allocations):
   - `hot_path` — message generation hot-path
   - `allocations` — alloc profile (msg/sec через format/transport/payload)
4. Median result сравнивается с baseline из `perf/baselines/<origin/main-sha>.json`.
5. Если deltas в пределах thresholds — PASS (job exit 0).
6. Если delta превышает threshold — FAIL (job exit 1, PR blocked).

### Если baseline не найден — FAIL

Раньше gate делал silent skip (`exit 0` + warning). Теперь — **exit 1**
с actionable error message. Это требует от maintainer:

1. Запустить `scripts/perf-baseline.sh update <sha>` локально.
2. Закоммитить `perf/baselines/<sha>.json` в main.
3. Re-run CI.

См. [Workflow maintainer guide §Refresh perf baseline](#refresh-perf-baseline).

## Median-of-N-runs (Issue #214)

CI single-run variance на GitHub Actions runners составляет ±30-50%
из-за cold cache, page cache miss, process scheduling. **Median-of-3-runs**
агрегирует 3 измерения через `scripts/compute-median.py` и берёт
арифметическую медиану per benchmark.

Результат: variance снижается до ±5-10% (на основе measurements из PR #216).

**Конфигурация через env var**:
```bash
BENCH_RUNS=5 scripts/perf-regression-collect.sh output.json hot_path
# По умолчанию BENCH_RUNS=3
```

**Performance cost**: 3x bench time. Для hot_path (~5 min single-run) это
~15 min на gate. Timeout workflow = 30 min.

## Когда gate может быть false-positive

- **Cold cache CI runner (до Issue #214)**: single-run variance ±30-50%.
  После Issue #214 (median-of-3-runs) variance снижается до ±5-10%.
  Если подозреваете false-positive — запустите `perf-regression` workflow
  вручную через `Actions → perf-regression → Run workflow` с явным
  `baseline_sha` из последнего успешного baseline.

- **Concurrent jobs**: Test ubuntu + Regression check run parallel,
  могут нагружать runner. Workflow по умолчанию запускает jobs
  параллельно (быстрее, но больше contention).

- **Несовместимые категории**: если ваш PR меняет label'ы в bench
  output (например переименовывает `hot_path/foo` → `hot_path/bar`),
  новый label не имеет baseline — gate пройдёт только если старый label
  тоже не регрессировал.

## Refresh perf baseline

Maintainer refresh процедура:

```bash
# 1. На свежем clone main, запустить benchmark с update:
git checkout main && git pull
scripts/perf-baseline.sh update "$(git rev-parse HEAD)"

# 2. Проверить что файл создан и валиден:
cat perf/baselines/<sha>.json | jq '.estimates | length'

# 3. Commit baseline в main (с явным сообщением):
git add perf/baselines/<sha>.json
git commit -m "perf(baseline): refresh baseline for <sha>"
git push origin main

# Или через PR (preferred для traceability):
git checkout -b perf/baseline-refresh-<sha>
git push origin perf/baseline-refresh-<sha>
# Open PR → CI verifies → merge
```

`perf-baseline.yml` workflow также генерирует baseline artifacts
(см. `.github/workflows/perf-baseline.yml`). Артефакты живут 90 дней,
но для gate нужны в `perf/baselines/<sha>.json` в main.

## Auto-generation (Issue #223, v11.8)

С v11.8 baseline для нового main HEAD создаётся **автоматически** через
`.github/workflows/perf-baseline-autogen.yml`. Это убирает ручной bootstrap
bottleneck, описанный в §Refresh perf baseline.

### Trigger

- `push` в `main` (после каждого merge).
- `workflow_dispatch` (manual override).

### Что делает

1. Capture `NEW_SHA = git rev-parse HEAD`, `PREV_SHA = git rev-parse HEAD~1`.
2. Skip если `perf/baselines/<NEW_SHA>.json` уже существует (idempotent).
3. Compile benches + Warm-up (cold cache mitigation).
4. Generate baseline через **median-of-3** hot_path runs (Issue #214 prep).
5. Compare new vs `perf/baselines/<PREV_SHA>.json`:
   - **Threshold: +10%** для hot_path (stricter чем gate's 50%, achievable
     с median-of-3 variance reduction).
6. Если regression > +10%: **FAIL workflow, baseline НЕ коммитится**.
7. Если regression ≤ +10% или нет prev baseline: коммитит baseline в main
   через `git add -f` (overrides `perf/baselines/.gitignore`).

### Hard policy

> Если новый код приводит к регрессии по производительности — это повод
> чинить новый код, а не снижать требования к бейзлайну.

Workflow **никогда** не обновляет baseline при регрессии > threshold.
Поддержание baseline через снижение требований — нарушение governance.

### Если регрессия обнаружена

1. Workflow помечает job как failed с детальной диагностикой:
   ```
   REGRESS hot_path/rfc5424_with_faker: 1700ns → 1900ns (+11.8%)
   FAIL: 1 regression(s) exceed +10% threshold.
   ```
2. Maintainer должен:
   - Профилировать регрессию (`cargo flamegraph`, `perf record`).
   - Починить код (откатить perf-regressed commit или оптимизировать).
   - Push новый commit → workflow re-run с новой baseline.
   - Если регрессия justified (например, добавлена новая feature которая
     требует trade-off): обновить baseline через явный commit с
     подробным justification в PR (НЕ через force_override).
3. **DEBUG override** (только для investigation): workflow_dispatch с
   `force_override=true` — пропускает regression check и коммитит baseline
   anyway. Production код НЕ должен использовать этот override.

### Tuning

- **Threshold (+10%)** выбран как compromise между strict regression
  detection и CI noise reality. С median-of-3 variance ~±5%, 10%
  threshold ловит real regressions (>10% = signal, не noise).
- **Issue #218 (v12.0)**: планируется tighter threshold (+5%) если
  systematic CI variance будет reduced (bencher / paired A/B).
- **Issue #214 (v11.9)**: generalizes median-of-N для всех perf workflows,
  не только autogen.

### Manual refresh (legacy)

Manual refresh через `scripts/perf-baseline.sh update <sha>` остаётся
поддерживаемым escape hatch для случаев когда auto-gen не покрывает
(use-case: backfill baseline для старого SHA, debug, etc.). См. §Refresh
perf baseline.

## Workflow integration

- **Trigger**: `pull_request` на main/dev (автоматически).
- **Manual trigger**: `Actions → perf-regression → Run workflow` с
  опциональным `baseline_sha` override.
- **Blocking**: с Issue #164 (v11.8). Все pre-blocking PR (v11.7.x)
  не подвержены gate, но v11.8+ PR — подвержены.
- **Median aggregation**: с Issue #214 (v11.9).
- **Auto-baseline trigger**: `push` в main через
  `perf-baseline-autogen.yml` (Issue #223).

## Связанные документы

- [`docs/DEVELOPER_GUIDE.md`](DEVELOPER_GUIDE.md) — как писать новые benches.
- [`docs/COVERAGE.md`](COVERAGE.md) — Tier 1 coverage gate (97%).
- [`CLAUDE_HANDOFF.md`](../CLAUDE_HANDOFF.md) — release process (perf baselines
  обновляются при release tag push).
- [Issue #164](https://github.com/pharmacolog/syslog-generator/issues/164) —
  A6 gate blocking (v11.8).
- [Issue #211](https://github.com/pharmacolog/syslog-generator/issues/211) —
  allocations bench (v11.9).
- [Issue #214](https://github.com/pharmacolog/syslog-generator/issues/214) —
  median-of-N-runs для variance reduction (v11.9).
- [Issue #223](https://github.com/pharmacolog/syslog-generator/issues/223) —
  auto-baseline workflow (PR-A6.1, v11.8).
