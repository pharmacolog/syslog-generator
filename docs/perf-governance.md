# Performance Governance (v11.8+)

> **Status**: Active since v11.8 (Issue #164, A6 gap-closing).
> **Owners**: maintainer + AI agents (per `AGENTS.md`).

## Цель

Performance regression gate защищает main от неконтролируемых регрессий
в hot-path, format, transport, allocations. Gate **blocking** с момента
merge Issue #164 (`feat(perf): make perf-regression gate truly blocking`).

## Thresholds

| Категория | Threshold (v11.8 placeholder) | Threshold (target v11.9) | Что покрывает |
|---|---|---|---|
| `hot_path/` | **+50%** | +5% | `benches/hot_path.rs` — message generation hot-path |
| `format/` | **+50%** | +10% | `benches/format/*.rs` — rfc5424/rfc3164/cef/leef/json encoding |
| `transport/` | **+50%** | +10% | `benches/transport/*.rs` — TCP/UDP/TLS/file-rotation |
| `allocations/` | **+50%** | +15% | Reserved. Нет allocations bench; готовится в Issue #211 |

**Improvement > threshold** печатается в `Improvements` (informational,
не влияет на gate status).

## Как работает gate

1. PR открыт в `main` или `dev`.
2. `perf-regression.yml` запускает **Warm-up** step (`cargo bench -- --warm-up-time 1`)
   для cold cache mitigation.
3. Затем запускает `cargo bench --bench hot_path -- --quick` для measurement.
4. Результат сравнивается с baseline из `perf/baselines/<origin/main-sha>.json`.
5. Если deltas в пределах thresholds — PASS (job exit 0).
6. Если delta превышает threshold — FAIL (job exit 1, PR blocked).

### Если baseline не найден — FAIL

Раньше gate делал silent skip (`exit 0` + warning). Теперь — **exit 1**
с actionable error message. Это требует от maintainer:

1. Запустить `scripts/perf-baseline.sh update <sha>` локально.
2. Закоммитить `perf/baselines/<sha>.json` в main.
3. Re-run CI.

См. [Workflow maintainer guide §Refresh perf baseline](#refresh-perf-baseline).

## Когда gate может быть false-positive

- **Cold cache CI runner**: `Swatinem/rust-cache@v2` кэширует build
  artifacts, но `cargo bench` на cold cache показывает ±20-30% variance
  (page cache miss, file system warm-up, process scheduling).
  Mitigation: workflow имеет отдельный **Warm-up** step перед измерениями
  (`cargo bench -- --warm-up-time 1 --measurement-time 1`), который
  прогревает page cache. После warm-up variance снижается до ±5%.
  Если подозреваете false-positive — запустите `perf-regression` workflow
  вручную через `Actions → perf-regression → Run workflow` с явным
  `baseline_sha` из последнего успешного baseline.

- **Бенчмарк-вариативность (cold cache, v11.8)**: Criterion `time_ns_median`
  на GitHub Actions runners показывает **±30-50%** variance даже с warm-up
  step. Это связано с:
  - Cold page cache (file system state)
  - Concurrent jobs (Test ubuntu + Regression check run parallel)
  - Process scheduling latency
  - Сетевой latency к container registry (rust cache fetch)

  **Current state (v11.8)**: thresholds = 50% (effectively noop для single-run).
  Maintainer должен вручную ревьюить perf-impact PR >10% через bench artifacts.

  **Target (v11.9)**: Issue #214 планирует median-of-3-runs для reduction
  variance до ±5%, после чего thresholds могут быть ужесточены до
  5%/10%/10%/15%.

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

## Workflow integration

- **Trigger**: `pull_request` на main/dev (автоматически).
- **Manual trigger**: `Actions → perf-regression → Run workflow` с
  опциональным `baseline_sha` override.
- **Blocking**: с Issue #164 (v11.8). Все pre-blocking PR (v11.7.x)
  не подвержены gate, но v11.8+ PR — подвержены.

## Связанные документы

- [`docs/DEVELOPER_GUIDE.md`](DEVELOPER_GUIDE.md) — как писать новые benches.
- [`docs/COVERAGE.md`](COVERAGE.md) — Tier 1 coverage gate (97%).
- [`CLAUDE_HANDOFF.md`](../CLAUDE_HANDOFF.md) — release process (perf baselines
  обновляются при release tag push).
- [Issue #164](https://github.com/pharmacolog/syslog-generator/issues/164) —
  A6 gap-closing (v11.8).
- [Issue #211](https://github.com/pharmacolog/syslog-generator/issues/211) —
  allocations bench (future work для full allocations coverage).
- [Issue #214](https://github.com/pharmacolog/syslog-generator/issues/214) —
  median-of-N-runs для variance reduction (v11.9).
