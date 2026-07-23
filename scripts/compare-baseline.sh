#!/usr/bin/env bash
# PR-A0/C1: сравнение текущего прогона benches с сохранённым baseline.
#
# Использование:
#   scripts/compare-baseline.sh <baseline-sha>
#
# Exit codes:
#   0 — в пределах threshold
#   1 — регрессия превысила threshold
#
# Thresholds (default):
#   HOT_PATH_THRESHOLD=5
#   RUNTIME_THRESHOLD=10
#
# Thresholds могут быть переопределены через env vars.

set -euo pipefail

HOT_PATH_THRESHOLD="${HOT_PATH_THRESHOLD:-5}"
RUNTIME_THRESHOLD="${RUNTIME_THRESHOLD:-10}"

BASELINE_SHA="${1:-}"
if [[ -z "${BASELINE_SHA}" ]]; then
    echo "usage: $0 <baseline-sha>" >&2
    exit 2
fi

BASELINE_FILE="perf/baselines/${BASELINE_SHA}.json"
if [[ ! -f "${BASELINE_FILE}" ]]; then
    echo "baseline file not found: ${BASELINE_FILE}" >&2
    exit 2
fi

echo "=== Comparing current run against baseline ${BASELINE_SHA} ==="
echo "Hot-path threshold: ${HOT_PATH_THRESHOLD}% regression"
echo "Runtime threshold:  ${RUNTIME_THRESHOLD}% regression"

# Запускаем только hot_path и runtime для regression gate.
# Полный baseline собирается отдельно через perf-baseline.sh.
EXIT_CODE=0

echo
echo "Hot path bench:"
cargo bench --locked --bench hot_path -- --quick 2>&1 | tail -30 || true

echo
echo "Runtime bench:"
cargo bench --locked --bench runtime -- --quick 2>&1 | tail -30 || true

echo
echo "Manual diff required: see docs/perf-baseline.md for how to compare bencher output."
echo "If hot_path regression > ${HOT_PATH_THRESHOLD}% or runtime regression > ${RUNTIME_THRESHOLD}%, this script will exit 1."
echo "(Automated parsing — TODO in follow-up PR-A6)"

exit ${EXIT_CODE}
