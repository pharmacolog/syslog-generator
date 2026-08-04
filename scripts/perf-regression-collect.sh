#!/usr/bin/env bash
# Issue #164: helper for perf-regression workflow.
#
# Генерирует baseline JSON только для указанных bench targets (default: hot_path).
# Используется в PR-blocking gate где CI-time budget не позволяет запускать
# все 5 benches через scripts/perf-baseline.sh.
#
# Usage:
#   scripts/perf-regression-collect.sh <output-json> [bench1 bench2 ...]
#
# Output format:
#   { "timestamp": "...", "baseline_sha": "...", "estimates": [...] }
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
