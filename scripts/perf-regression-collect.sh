#!/usr/bin/env bash
<<<<<<< HEAD
# Issue #164: helper for perf-regression workflow.
#
# Генерирует baseline JSON только для указанных bench targets (default: hot_path).
=======
# Issue #164, #214: helper for perf-regression workflow.
#
# Генерирует baseline JSON через median-of-N-runs агрегацию.
>>>>>>> origin/main
# Используется в PR-blocking gate где CI-time budget не позволяет запускать
# все 5 benches через scripts/perf-baseline.sh.
#
# Usage:
#   scripts/perf-regression-collect.sh <output-json> [bench1 bench2 ...]
#
<<<<<<< HEAD
# Output format:
#   { "timestamp": "...", "baseline_sha": "...", "estimates": [...] }
=======
# Environment:
#   BENCH_RUNS  Number of runs для median aggregation (default: 3).
#               Больше runs = меньше variance, но дольше CI.
#
# Output format:
#   { "timestamp": "...", "baseline_sha": "...", "n_runs": N,
#     "estimates": [...] }  (median per benchmark across N runs)
>>>>>>> origin/main
# Compatible with perf-regression.yml Compare step.
#
# Exit codes:
#   0 — success
#   1 — bench failed or no estimates collected

set -euo pipefail

OUT="${1:-perf/baselines/regression.json}"
shift || true

if [[ $# -eq 0 ]]; then
    BENCHES=(hot_path)
else
    BENCHES=("$@")
fi

<<<<<<< HEAD
mkdir -p "$(dirname "${OUT}")"
ESTIMATES_JSONL="$(mktemp)"
trap 'rm -f "${ESTIMATES_JSONL}"' EXIT
: > "${ESTIMATES_JSONL}"

for bench in "${BENCHES[@]}"; do
    echo "=== Running ${bench} ===" >&2
    rm -rf "target/criterion/${bench}"* 2>/dev/null || true

    if ! cargo bench --locked --bench "${bench}" -- --quick >/dev/null 2>&1; then
        echo "FAILED: ${bench}" >&2
        exit 1
    fi

    while IFS= read -r estimates; do
        [[ -f "${estimates}" ]] || continue
        python3 "$(dirname "$0")/perf-estimate-parse.py" "${bench}" "${estimates}" >> "${ESTIMATES_JSONL}" 2>/dev/null || true
    done < <(find "target/criterion" -path "*/${bench}*/new/estimates.json" -type f 2>/dev/null)
done

COUNT=$(wc -l < "${ESTIMATES_JSONL}" | tr -d ' ')
if [[ "${COUNT}" -eq 0 ]]; then
    echo "ERROR: no estimates collected" >&2
    exit 1
fi

TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SHA="$(git rev-parse HEAD 2>/dev/null || echo unknown)"

{
    echo "{"
    echo "  \"timestamp\": \"${TIMESTAMP}\","
    echo "  \"baseline_sha\": \"${SHA}\","
    echo "  \"estimates\": ["
    FIRST=1
    while IFS= read -r line; do
        if [[ "${FIRST}" -eq 1 ]]; then
            printf "    %s" "${line}"
            FIRST=0
        else
            printf ",\n    %s" "${line}"
        fi
    done < "${ESTIMATES_JSONL}"
    echo
    echo "  ]"
    echo "}"
} > "${OUT}"

echo "Generated: ${OUT} (${COUNT} estimates)"
=======
# Issue #214: median-of-N-runs для variance reduction.
N_RUNS="${BENCH_RUNS:-3}"
if ! [[ "${N_RUNS}" =~ ^[1-9][0-9]*$ ]]; then
    echo "ERROR: BENCH_RUNS must be positive integer, got: ${N_RUNS}" >&2
    exit 2
fi

mkdir -p "$(dirname "${OUT}")"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT

# Cleanup criterion cache once (all N runs share compiled benches).
for bench in "${BENCHES[@]}"; do
    rm -rf "target/criterion/${bench}"* 2>/dev/null || true
done

# Collect per-run estimates.
declare -a RUN_FILES=()
for run_idx in $(seq 1 "${N_RUNS}"); do
    echo "=== Run ${run_idx}/${N_RUNS} ===" >&2
    RUN_JSONL="${WORKDIR}/run-${run_idx}.jsonl"
    : > "${RUN_JSONL}"
    RUN_FILES+=("${RUN_JSONL}")

    for bench in "${BENCHES[@]}"; do
        # Re-clear criterion cache before each bench (per-run isolation)
        rm -rf "target/criterion/${bench}"* 2>/dev/null || true

        if ! cargo bench --locked --bench "${bench}" -- --quick >/dev/null 2>&1; then
            echo "FAILED: ${bench} (run ${run_idx})" >&2
            exit 1
        fi

        while IFS= read -r estimates; do
            [[ -f "${estimates}" ]] || continue
            python3 "$(dirname "$0")/perf-estimate-parse.py" "${bench}" "${estimates}" >> "${RUN_JSONL}" 2>/dev/null || true
        done < <(find "target/criterion" -path "*/${bench}*/new/estimates.json" -type f 2>/dev/null)
    done

    COUNT=$(wc -l < "${RUN_JSONL}" | tr -d ' ')
    if [[ "${COUNT}" -eq 0 ]]; then
        echo "ERROR: no estimates collected in run ${run_idx}" >&2
        exit 1
    fi
done

# Aggregate median via compute-median.py.
TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SHA="$(git rev-parse HEAD 2>/dev/null || echo unknown)"

BASELINE_SHA="${SHA}" TIMESTAMP="${TIMESTAMP}" \
    python3 "$(dirname "$0")/compute-median.py" "${OUT}" "${RUN_FILES[@]}"

echo "Generated: ${OUT} (median of ${N_RUNS} runs)"
>>>>>>> origin/main
