#!/usr/bin/env bash
<<<<<<< HEAD
# Issue #164: tests for perf-estimate-parse.py and perf-regression-collect.sh
=======
# Issue #164, #214: tests for perf-estimate-parse.py, compute-median.py, perf-regression-collect.sh
>>>>>>> origin/main
#
# Coverage:
# - Positive: нормальный Criterion estimates.json → parsed JSON line
# - Negative: пустой estimates.json → graceful exit 0
# - Edge: malformed JSON → не crash (exit != 0)
<<<<<<< HEAD
=======
# - Median: compute-median.py корректно агрегирует N runs
>>>>>>> origin/main
#
# Usage:
#   bash scripts/tests/test_perf_scripts.sh
#
# Exit codes:
#   0 — все тесты прошли
#   1 — один или более тестов fail'нули

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PASS=0
FAIL=0

log() { echo "[test_perf_scripts] $*"; }

run_test() {
    local name="$1"
    local cmd="$2"
    local expected_rc="$3"

    if bash -c "${cmd}" >/dev/null 2>&1; then
        actual_rc=0
    else
        actual_rc=$?
    fi

    if [[ "${actual_rc}" -eq "${expected_rc}" ]]; then
        log "PASS: ${name} (rc=${actual_rc})"
        PASS=$((PASS + 1))
    else
        log "FAIL: ${name} expected rc=${expected_rc} got rc=${actual_rc}"
        FAIL=$((FAIL + 1))
    fi
}

# Test 1: perf-estimate-parse.py — нормальный input (positive)
# Note: script expects RELATIVE path вида "target/criterion/<group>/<sub>/new/estimates.json"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT
cd "${TMPDIR}"

mkdir -p "target/criterion/hot_path/rfc5424_with_faker/new"
cat > "target/criterion/hot_path/rfc5424_with_faker/new/estimates.json" <<EOF
{
  "median": {
    "point_estimate": 1684,
    "lower_bound": 1650,
    "upper_bound": 1720
  }
}
EOF

run_test "perf-estimate-parse.py positive" \
    "python3 ${REPO_ROOT}/scripts/perf-estimate-parse.py hot_path target/criterion/hot_path/rfc5424_with_faker/new/estimates.json" \
    0

# Test 2: perf-estimate-parse.py — пустой файл (negative → graceful exit 0)
echo "" > "empty.json"
run_test "perf-estimate-parse.py empty" \
    "python3 ${REPO_ROOT}/scripts/perf-estimate-parse.py hot_path empty.json" \
    0

# Test 3: perf-estimate-parse.py — relative path without target/criterion (edge → exit 0, потому что IndexError)
echo '{"median":{"point_estimate":100}}' > "malformed.json"
run_test "perf-estimate-parse.py short path" \
    "python3 ${REPO_ROOT}/scripts/perf-estimate-parse.py hot_path malformed.json" \
    0

# Test 4: perf-regression-collect.sh — syntax check (без запуска cargo bench)
run_test "perf-regression-collect.sh syntax check" \
    "bash -n ${REPO_ROOT}/scripts/perf-regression-collect.sh" \
    0

# Test 5: perf-estimate-parse.py — syntax check
run_test "perf-estimate-parse.py syntax check" \
    "python3 -m py_compile ${REPO_ROOT}/scripts/perf-estimate-parse.py" \
    0

<<<<<<< HEAD
=======
# Issue #214: compute-median.py tests
# Test 6: compute-median.py — syntax check
run_test "compute-median.py syntax check" \
    "python3 -m py_compile ${REPO_ROOT}/scripts/compute-median.py" \
    0

# Test 7: compute-median.py — positive (3 runs, 1 label)
RUN1="${TMPDIR}/run1.jsonl"
RUN2="${TMPDIR}/run2.jsonl"
RUN3="${TMPDIR}/run3.jsonl"
cat > "${RUN1}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo", "time_ns_median": 1000, "time_ns_lower": 950, "time_ns_upper": 1050}
EOF
cat > "${RUN2}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo", "time_ns_median": 1100, "time_ns_lower": 1050, "time_ns_upper": 1150}
EOF
cat > "${RUN3}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo", "time_ns_median": 1200, "time_ns_lower": 1150, "time_ns_upper": 1250}
EOF
MEDIAN_OUT="${TMPDIR}/median-out.json"
run_test "compute-median.py positive (3 runs, 1 label)" \
    "python3 ${REPO_ROOT}/scripts/compute-median.py ${MEDIAN_OUT} ${RUN1} ${RUN2} ${RUN3}" \
    0

# Verify median value is correct (median of [1000, 1100, 1200] = 1100)
MEDIAN_VALUE=$(python3 -c "import json; print(json.load(open('${MEDIAN_OUT}'))['estimates'][0]['time_ns_median'])" 2>/dev/null || echo "ERROR")
if [[ "${MEDIAN_VALUE}" == "1100" ]]; then
    log "PASS: compute-median.py median value (1000,1100,1200 → 1100)"
    PASS=$((PASS + 1))
else
    log "FAIL: compute-median.py median value expected 1100 got ${MEDIAN_VALUE}"
    FAIL=$((FAIL + 1))
fi

# Test 8: compute-median.py — empty input (negative → exit 1)
EMPTY_OUT="${TMPDIR}/empty-out.json"
run_test "compute-median.py empty input" \
    "python3 ${REPO_ROOT}/scripts/compute-median.py ${EMPTY_OUT} ${TMPDIR}/nonexistent.jsonl" \
    1

# Test 9: compute-median.py — missing args (negative → exit 2)
run_test "compute-median.py no args" \
    "python3 ${REPO_ROOT}/scripts/compute-median.py" \
    2

# Test 10: compute-median.py — 5 runs (median = 3rd sorted value)
declare -a RUNS5=()
for i in 1 2 3 4 5; do
    R="${TMPDIR}/run5-${i}.jsonl"
    RUNS5+=("${R}")
    VAL=$((900 + i * 100))  # 1000, 1100, 1200, 1300, 1400
    cat > "${R}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/bar", "time_ns_median": ${VAL}, "time_ns_lower": 0, "time_ns_upper": 0}
EOF
done
MEDIAN5_OUT="${TMPDIR}/median5-out.json"
run_test "compute-median.py positive (5 runs, 1 label)" \
    "python3 ${REPO_ROOT}/scripts/compute-median.py ${MEDIAN5_OUT} ${RUNS5[0]} ${RUNS5[1]} ${RUNS5[2]} ${RUNS5[3]} ${RUNS5[4]}" \
    0

MEDIAN5_VALUE=$(python3 -c "import json; print(json.load(open('${MEDIAN5_OUT}'))['estimates'][0]['time_ns_median'])" 2>/dev/null || echo "ERROR")
if [[ "${MEDIAN5_VALUE}" == "1200" ]]; then
    log "PASS: compute-median.py median of 5 (1000..1400 → 1200)"
    PASS=$((PASS + 1))
else
    log "FAIL: compute-median.py median of 5 expected 1200 got ${MEDIAN5_VALUE}"
    FAIL=$((FAIL + 1))
fi

# Test 11: compute-median.py — multiple labels across runs
M1="${TMPDIR}/m1.jsonl"
M2="${TMPDIR}/m2.jsonl"
cat > "${M1}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo", "time_ns_median": 100, "time_ns_lower": 0, "time_ns_upper": 0}
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/bar", "time_ns_median": 200, "time_ns_lower": 0, "time_ns_upper": 0}
EOF
cat > "${M2}" <<EOF
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo", "time_ns_median": 300, "time_ns_lower": 0, "time_ns_upper": 0}
{"bench": "hot_path", "group": "hot_path", "label": "hot_path/bar", "time_ns_median": 400, "time_ns_lower": 0, "time_ns_upper": 0}
EOF
MULTI_OUT="${TMPDIR}/multi-out.json"
run_test "compute-median.py multiple labels" \
    "python3 ${REPO_ROOT}/scripts/compute-median.py ${MULTI_OUT} ${M1} ${M2}" \
    0

MULTI_COUNT=$(python3 -c "import json; print(len(json.load(open('${MULTI_OUT}'))['estimates']))" 2>/dev/null || echo "ERROR")
if [[ "${MULTI_COUNT}" == "2" ]]; then
    log "PASS: compute-median.py multiple labels (2 labels preserved)"
    PASS=$((PASS + 1))
else
    log "FAIL: compute-median.py expected 2 labels got ${MULTI_COUNT}"
    FAIL=$((FAIL + 1))
fi

>>>>>>> origin/main
log ""
log "===================="
log "PASS: ${PASS}"
log "FAIL: ${FAIL}"
log "===================="

if [[ "${FAIL}" -gt 0 ]]; then
    exit 1
fi

exit 0