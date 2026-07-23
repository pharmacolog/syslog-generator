#!/usr/bin/env bash
# PR-A0 (v10.8.0): запуск всех perf benches + сохранение structured baseline.
#
# Использование:
#   scripts/perf-baseline.sh                       # quick mode (--quick)
#   scripts/perf-baseline.sh full                  # полный прогон
#   scripts/perf-baseline.sh update <git-sha>      # сохранить baseline в perf/baselines/<sha>.json
#
# Выходы:
#   - perf/baselines/<sha>.json: structured baseline с estimates per benchmark
#   - exit 0 при успехе всех bench, exit 1 при любом failure

set -euo pipefail

MODE="${1:-quick}"
SHA="${2:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"

if [[ "${MODE}" != "quick" && "${MODE}" != "full" && "${MODE}" != "update" ]]; then
    echo "usage: $0 [quick|full|update] [<sha>]" >&2
    exit 2
fi

# Имена bench-таргетов из Cargo.toml.
BENCHES_QUICK=(
    hot_path
    runtime
    format_matrix
    transport_matrix
    dispatch_matrix
)

BENCHES_FULL=(
    "${BENCHES_QUICK[@]}"
    message_generation
    sender_throughput
    format_cef
    format_leef
    format_json_lines
    transport_tls
    transport_file_rotation
    transport_reconnect
)

if [[ "${MODE}" == "quick" ]]; then
    BENCHES=("${BENCHES_QUICK[@]}")
else
    BENCHES=("${BENCHES_FULL[@]}")
fi

QUICK_FLAG="--quick"
if [[ "${MODE}" == "full" || "${MODE}" == "update" ]]; then
    QUICK_FLAG=""
fi

mkdir -p perf/baselines
OUT="perf/baselines/${SHA}.json"

# Парсим Criterion native JSON estimates.json (надёжнее bencher text format).

ESTIMATES_JSONL="$(mktemp)"
trap 'rm -f "${ESTIMATES_JSONL}"' EXIT
: > "${ESTIMATES_JSONL}"

OVERALL_STATUS=0

for bench in "${BENCHES[@]}"; do
    echo "=== Running ${bench} ===" >&2
    # Очищаем старые estimates чтобы получить только новые.
    rm -rf "target/criterion/${bench}"

    if ! cargo bench --locked --bench "${bench}" -- ${QUICK_FLAG} >/dev/null 2>&1; then
        echo "FAILED: ${bench}" >&2
        OVERALL_STATUS=1
        continue
    fi

    if [[ ! -d "target/criterion/${bench}" ]]; then
        echo "  no estimates dir for ${bench}" >&2
        continue
    fi

    # Парсим все estimates.json файлы рекурсивно.
    while IFS= read -r estimates; do
        # path: target/criterion/<bench>/<group>/<sub>/<kind>/estimates.json
        # или:  target/criterion/<bench>/<group>/<kind>/estimates.json
        python3 -c "
import json, os, sys
path = '${estimates}'
parts = path.split('/')
# последний = 'estimates.json', предпоследний = 'base|change|new',
# остальное = group/sub.
# Берём group/sub = все между 'criterion/<bench>/' и '<kind>/'.
try:
    idx = parts.index('criterion')
    bench_name = parts[idx + 1]
    rest = parts[idx + 2:-2]  # exclude 'kind' and 'estimates.json'
except ValueError:
    sys.exit(0)
label = '/'.join(rest) if rest else bench_name
data = json.load(open(path))
ns = data['mean']['point_estimate']
lower = data['mean']['confidence_interval']['lower_bound']
upper = data['mean']['confidence_interval']['upper_bound']
print(json.dumps({
    'bench': bench_name,
    'label': label,
    'time_ns_median': ns,
    'time_ns_lower': lower,
    'time_ns_upper': upper,
}))
" >> "${ESTIMATES_JSONL}" 2>/dev/null || true
    done < <(find "target/criterion/${bench}" -name "estimates.json" -type f 2>/dev/null)
done

TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
COUNT=$(wc -l < "${ESTIMATES_JSONL}" | tr -d ' ')
{
    echo "{"
    echo "  \"sha\": \"${SHA}\","
    echo "  \"mode\": \"${MODE}\","
    echo "  \"timestamp\": \"${TIMESTAMP}\","
    echo "  \"status\": \"$([ ${OVERALL_STATUS} -eq 0 ] && echo ok || echo partial)\","
    echo "  \"estimate_count\": ${COUNT},"
    echo "  \"estimates\": ["
    FIRST=1
    while IFS= read -r line; do
        if [[ -z "${line}" ]]; then continue; fi
        if [[ ${FIRST} -eq 0 ]]; then echo ","; fi
        FIRST=0
        printf "    %s" "${line}"
    done < "${ESTIMATES_JSONL}"
    echo
    echo "  ]"
    echo "}"
} > "${OUT}.tmp"

if ! python3 -c "import json; json.load(open('${OUT}.tmp'))" 2>/dev/null; then
    echo "ERROR: invalid JSON output" >&2
    rm -f "${OUT}.tmp"
    exit 1
fi

mv "${OUT}.tmp" "${OUT}"
echo "Baseline saved to ${OUT} (${COUNT} estimates)" >&2

exit ${OVERALL_STATUS}
