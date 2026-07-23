#!/usr/bin/env bash
# PR-A0 (v10.8.0): запуск всех perf benches + сохранение structured baseline.
#
# Использование:
#   scripts/perf-baseline.sh                       # quick mode (--quick)
#   scripts/perf-baseline.sh full                  # полный прогон
#   scripts/perf-baseline.sh update <git-sha>      # сохранить baseline в perf/baselines/<sha>.json
#
# Выходы:
#   - stdout: criterion output
#   - perf/baselines/<sha>.json: structured baseline с estimates per benchmark
#   - exit 0 при успехе всех bench, exit 1 при любом failure

set -euo pipefail

MODE="${1:-quick}"
SHA="${2:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"

if [[ "${MODE}" != "quick" && "${MODE}" != "full" && "${MODE}" != "update" ]]; then
    echo "usage: $0 [quick|full|update] [<sha>]" >&2
    exit 2
fi

BENCHES=(
    hot_path
    runtime
    format_matrix
    transport_matrix
    dispatch_matrix
    message_generation
    sender_throughput
    format/cef
    format/leef
    format/json_lines
    transport/tls
    transport/file_rotation
    transport/reconnect
)

QUICK_FLAG="--quick"
if [[ "${MODE}" == "full" || "${MODE}" == "update" ]]; then
    QUICK_FLAG=""
fi

mkdir -p perf/baselines
OUT="perf/baselines/${SHA}.json"

# Используем Criterion machine summary через --output-format.
# Это формат `bench -- "${BENCH} ${KIND} ${ARGS} ..."` который легко парсить.

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

JSON_OUT="${TMP}/estimates.jsonl"
: > "${JSON_OUT}"

OVERALL_STATUS=0

for bench in "${BENCHES[@]}"; do
    echo "=== Running ${bench} ===" >&2
    # Criterion не имеет стабильного JSON-output, но --output-format bencher даёт CSV-подобный формат:
    #   group,N,label,time_lower,time_median,time_upper,thrpt_lower,thrpt_median,thrpt_upper
    if ! cargo bench --locked --bench "${bench}" -- ${QUICK_FLAG} --output-format bencher \
        > "${TMP}/${bench}.bencher" 2> "${TMP}/${bench}.err"; then
        echo "FAILED: ${bench}" >&2
        OVERALL_STATUS=1
        continue
    fi
    # Преобразуем bencher CSV → JSONL.
    awk -F',' -v bench="${bench}" 'NR > 1 && NF >= 6 {
        gsub(/[\[\] ]/, "", $0);
        split($0, a, ",");
        printf("{\"bench\":\"%s\",\"group\":\"%s\",\"label\":\"%s\",\"time_ns_lower\":%s,\"time_ns_median\":%s,\"time_ns_upper\":%s,\"thrpt_lower\":%s,\"thrpt_median\":%s,\"thrpt_upper\":%s}\n",
            bench, a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]);
    }' "${TMP}/${bench}.bencher" >> "${JSON_OUT}" || true
done

# Сериализуем JSONL в единый JSON-массив.
TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
{
    echo "{"
    echo "  \"sha\": \"${SHA}\","
    echo "  \"mode\": \"${MODE}\","
    echo "  \"timestamp\": \"${TIMESTAMP}\","
    echo "  \"status\": \"$([ ${OVERALL_STATUS} -eq 0 ] && echo ok || echo partial)\","
    echo "  \"estimates\": ["
    FIRST=1
    while IFS= read -r line; do
        if [[ ${FIRST} -eq 0 ]]; then echo ","; fi
        FIRST=0
        printf "    %s" "${line}"
    done < "${JSON_OUT}"
    echo
    echo "  ]"
    echo "}"
} > "${OUT}.tmp"

# Проверяем валидность JSON через python (если есть), иначе перемещаем as-is.
if command -v python3 >/dev/null 2>&1; then
    if ! python3 -c "import json,sys; json.load(open('${OUT}.tmp'))" 2>/dev/null; then
        echo "WARN: output JSON failed validation, saving anyway" >&2
    fi
fi

mv "${OUT}.tmp" "${OUT}"

if [[ ${OVERALL_STATUS} -eq 0 ]]; then
    echo "Baseline saved to ${OUT}" >&2
fi

exit ${OVERALL_STATUS}
