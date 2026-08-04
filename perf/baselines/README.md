# perf/baselines/

Structured baseline artifacts for Criterion benches. See
[`docs/perf-baseline.md`](../../docs/perf-baseline.md) and
[`docs/PERFORMANCE.md`](../../docs/PERFORMANCE.md) §3 for context.

## Files

| File | Description | Issue |
|------|-------------|-------|
| `v10.7.19.json` | Pre serde_yaml_ng migration baseline (Issue #134 / PR #177) | #193 |
| `v10.7.20.json` | Single-run baseline for current HEAD (post-migration) | #193 |
| `HEAD.json` | Latest A/B paired bench output (`scripts/bench-ab-yaml.sh`) | #193 |
| `<git-sha>.json` | Per-commit baselines (`scripts/perf-baseline.sh update`) | PR-A0 |

## Tracking policy

`.gitignore` исключает `*.json` по умолчанию (см. `.gitignore` в этом
каталоге). Чтобы закоммитить baseline в PR, override через `git add -f`:

```bash
git add -f perf/baselines/<sha>.json
```

Примеры PR, где это используется: #193, #195.

## Generation

```bash
# Per-commit baseline (с CI):
scripts/perf-baseline.sh update $(git rev-parse HEAD)

# A/B comparison vs tag (Issue #193):
scripts/bench-ab-yaml.sh v10.7.19 HEAD
```
