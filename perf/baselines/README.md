# perf/baselines/ holds per-SHA Criterion estimate baselines for the perf-regression gate.

This directory is intentionally tracked in git so that the perf-regression workflow can:
1. Read baseline from `perf/baselines/<origin/main-sha>.json` (immutable, SHA-pinned).
2. Compare against current PR's `target/criterion/<bench>/new/estimates.json`.
3. Fail the gate if any category (hot_path/format/transport/allocations) exceeds its threshold.

See:
- [Issue #164](https://github.com/pharmacolog/syslog-generator/issues/164) — gate blocking (v11.8).
- [PR #210](https://github.com/pharmacolog/syslog-generator/pull/210) — gate implementation.
- [docs/perf-governance.md](../docs/perf-governance.md) — refresh procedure.

Initial baseline generation:
```bash
scripts/perf-baseline.sh update "$(git rev-parse HEAD)"
```


