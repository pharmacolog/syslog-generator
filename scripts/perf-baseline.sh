#!/usr/bin/env bash
# PR-A0 (v10.8.0): запуск всех perf benches + сохранение baseline.
#
# Использование:
#   scripts/perf-baseline.sh                       # quick mode (--quick)
#   scripts/perf-baseline.sh full                  # полный прогон
#   scripts/perf-baseline.sh update <git-sha>      # сохранить baseline в perf/baselines/<sha>.json
#
# Выходы:
#   - stdout: criterion output
#   - perf/baselines/<sha>.json: сохранённый baseline (update mode)

set -euo pipefail

MODE="${1:-quick}"
SHA="${2:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"

BENCHES=(
    hot_path
    runtime
    format_matrix
    transport_matrix
    dispatch_matrix
)

QUICK_FLAG="--quick"
if [[ "${MODE}" == "full" ]]; then
    QUICK_FLAG=""
fi

mkdir -p perf/baselines

OUT="perf/baselines/${SHA}.json"
echo "=== Perf baseline: mode=${MODE}, sha=${SHA} ==="
echo "=== Output: ${OUT} ==="

{
    echo "{\"sha\": \"${SHA}\", \"mode\": \"${MODE}\", \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\", \"results\": {"
    FIRST=1
    for bench in "${BENCHES[@]}"; do
        echo "Running ${bench} ..."
        if [[ ${FIRST} -eq 0 ]]; then echo ","; fi
        FIRST=0
        echo "\"${bench}\":"
        # --output-format bencher совместим с cargo-benchcmp и ditto.
        cargo bench --locked --bench "${bench}" -- ${QUICK_FLAG} --output-format bencher 2>&1 || true
    done
    echo "}}"
} > "${OUT}"

echo
echo "Baseline saved to ${OUT}"
