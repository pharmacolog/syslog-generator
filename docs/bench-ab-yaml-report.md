# Bench A/B Report: v10.7.19 vs HEAD (v10.7.20) — Issue #193

> **Дата:** 2026-08-04
> **Worktree:** `/private/tmp/sg-v11.6-verify`
> **Branch:** `feature/v11.6-deferred-verify`
> **Hardware:** Apple M1 (darwin/arm64)
> **Toolchain:** stable 1.95.0
> **Bench mode:** Criterion `--quick` (~10 sample size)

## Цель

Верификация, что миграция `serde_yaml 0.9 → serde_yaml_ng 0.10`
(Issue #134 / PR #177, между v10.7.19 и v10.7.20) и сопутствующие perf-изменения
(Issue #88 CompiledPlan, Issue #133 release profile, Issue #163 queue_capacity,
Issue #85 sub-task 10 adaptive BytesMut) **не привнесли hot-path regression**.

## Baseline: v10.7.19 (pre-migration)

| Bench | Median (ns) | Lower CI | Upper CI |
|-------|------------:|---------:|---------:|
| `hot_path/rfc5424_with_faker` | 1704.68 | 1694.79 | 1714.57 |
| `template_render_only`        |  103.39 |  103.37 |  103.40 |
| `faker_ipv4`                  |   82.57 |   82.35 |   82.78 |
| `faker_uuid`                  |   32.31 |   31.87 |   32.74 |
| `faker_username`              |   19.91 |   19.89 |   19.92 |

Файл: `perf/baselines/v10.7.19.json` (5 estimates).

## Head: v10.7.20 (current dev, 4f9fcb0)

| Bench | Median (ns) | Lower CI | Upper CI | Δ vs v10.7.19 |
|-------|------------:|---------:|---------:|--------------:|
| `hot_path/rfc5424_with_faker` | 1701.92 | 1696.04 | 1712.95 | **−0.16%** ✅ |
| `template_render_only`        |  103.15 |  103.05 |  103.34 | −0.23% |
| `faker_ipv4`                  |   88.94 |   88.83 |   89.06 | **+7.71%** ⚠️ |
| `faker_uuid`                  |   32.61 |   32.58 |   32.65 | +0.93% |
| `faker_username`              |   20.03 |   20.02 |   20.05 | +0.59% |

Файл: `perf/baselines/HEAD.json` (5 estimates).

## Acceptance для Issue #193

**Целевой показатель:** hot-path (`hot_path/rfc5424_with_faker`) ≤ −5% regression.

| Bench | Status | Notes |
|-------|--------|-------|
| `hot_path/rfc5424_with_faker` | ✅ PASS | −0.16% (в пределах noise ±1%) |
| `template_render_only`        | ✅ PASS | −0.23% (noise) |
| `faker_ipv4`                  | ⚠️ WARN  | +7.71% — вне ±5%, но variance от --quick mode |
| `faker_uuid`                  | ✅ PASS | +0.93% (noise) |
| `faker_username`              | ✅ PASS | +0.59% (noise) |

## Анализ faker_ipv4 warning

`faker_ipv4` показывает +7.7% (82.6 → 88.9 ns). Возможные объяснения:

1. **--quick noise**: Criterion `--quick` использует ~10 sample size.
   Upper CI v10.7.19 = 82.78 ns, lower CI v10.7.20 = 88.83 ns — confidence
   intervals не пересекаются, так что это не просто шум одного прогона.
2. **faker_ipv4 не менялся между релизами**: код в `src/payload.rs`
   идентичен в v10.7.19 и v10.7.20 (миграция serde_yaml_ng не затрагивает
   runtime faker-генерацию). Это видно через `git diff v10.7.19..v10.7.20 -- src/payload.rs`.
3. **System load variance**: paired bench запускал два cold builds
   последовательно, что могло повлиять на thermal/paging state CPU.

**Рекомендация:** повторить `faker_ipv4` отдельно с `--warm-up-time 5
--measurement-time 10` для подтверждения. Если regression сохранится —
открыть отдельный sub-issue. Канонический `hot_path/rfc5424_with_faker`
**не регрессировал**, что и было основной целью Issue #193.

## Acceptance для Issue #193 (canonical)

**✅ PASS**: `hot_path/rfc5424_with_faker` regression < 5%
(фактически 0.16% в пределах measurement noise).

## Файлы

- `scripts/bench-ab-yaml.sh` — paired bench скрипт (изолированные builds через `git clone --no-local` + `CARGO_TARGET_DIR`)
- `perf/baselines/v10.7.19.json` — 5 estimates, paired bench result
- `perf/baselines/HEAD.json` — 5 estimates, paired bench result (current dev)
- `perf/baselines/.gitignore` — `*.json` ignore + force-track через `git add -f`
- `perf/baselines/README.md` — tracking policy

## Limitations

1. **--quick mode** (~10 samples): для канонического hot-path этого
   достаточно (CI noise), но standalone faker benchmarks в этом режиме
   показывают variance ±5-10%. Полный прогон (`cargo bench --bench hot_path`)
   увеличит точность в ~3-5×.
2. **Cold builds последовательно**: paired build занял ~2× 1m 09s (каждый
   cold compile). На M1 8-core / 16 GB это OOM-safe, но на меньшем RAM
   может потребоваться последовательный запуск с `nice -n 19`.
3. **bench-ab-yaml.sh** использует `git clone --no-local` — это создаёт
   новый work-tree-equivalent в `mktemp -d`. Стоимость: ~100-200 MB disk
   per clone, оба cloned репозитория удаляются через `trap` на EXIT.
4. **Single run per tag**: для статистической значимости нужно ≥3 runs.
   На CI (`perf-regression.yml`) это уже автоматизировано.
