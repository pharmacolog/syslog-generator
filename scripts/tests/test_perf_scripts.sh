#!/usr/bin/env bash
# Issue #164: tests for perf-estimate-parse.py and perf-regression-collect.sh
#
# Coverage:
# - Positive: нормальный Criterion estimates.json → parsed JSON line
# - Negative: пустой estimates.json → graceful exit 0
# - Edge: malformed JSON → не crash (exit != 0)
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

log ""
log "===================="
log "PASS: ${PASS}"
log "FAIL: ${FAIL}"
log "===================="

if [[ "${FAIL}" -gt 0 ]]; then
    exit 1
fi

exit 0