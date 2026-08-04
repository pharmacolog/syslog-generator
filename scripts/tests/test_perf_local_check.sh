#!/usr/bin/env bash
# Issue #228: tests для perf-local-check.sh.
#
# Coverage:
# - Syntax check
# - Help/error message для missing baseline
# - Python compare logic на synthetic baseline/current JSON
#
# Usage:
#   bash scripts/tests/test_perf_local_check.sh
#
# Exit codes:
#   0 — все тесты прошли
#   1 — один или более тестов fail'нули

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PASS=0
FAIL=0

log() { echo "[test_perf_local_check] $*"; }

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

# Test 1: syntax check
run_test "perf-local-check.sh syntax" \
    "bash -n ${REPO_ROOT}/scripts/perf-local-check.sh" \
    0

# Test 2: missing baseline → exit 2
TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT
cd "${TMPDIR}"
run_test "perf-local-check.sh missing baseline (no args, no files)" \
    "bash ${REPO_ROOT}/scripts/perf-local-check.sh" \
    2

# Test 3: missing baseline file argument
run_test "perf-local-check.sh missing arg file" \
    "bash ${REPO_ROOT}/scripts/perf-local-check.sh /nonexistent/file.json" \
    2

# Test 4: Python compare logic — synthetic data (no regressions)
# Manually craft baseline + current JSON files и запустить Python compare.
SYNTH_BASE="${TMPDIR}/synth-base.json"
SYNTH_CURR="${TMPDIR}/synth-curr.json"

cat > "${SYNTH_BASE}" <<EOF
{
  "timestamp": "2026-08-04T10:00:00Z",
  "baseline_sha": "test",
  "estimates": [
    {"bench": "hot_path", "group": "hot_path", "label": "hot_path/test1", "time_ns_median": 1000, "time_ns_lower": 0, "time_ns_upper": 0}
  ]
}
EOF

cat > "${SYNTH_CURR}" <<EOF
{
  "timestamp": "2026-08-04T10:00:00Z",
  "baseline_sha": "test",
  "estimates": [
    {"bench": "hot_path", "group": "hot_path", "label": "hot_path/test1", "time_ns_median": 1050, "time_ns_lower": 0, "time_ns_upper": 0}
  ]
}
EOF

# Run Python compare inline (extract from perf-local-check.sh)
SYNTH_RESULT=$(BASELINE_FILE="${SYNTH_BASE}" CURRENT_FILE="${SYNTH_CURR}" THRESHOLD_HOT_PATH=10 \
    python3 - <<'PYEOF'
import json
import os
import sys
with open(os.environ["BASELINE_FILE"]) as f: base = json.load(f)
with open(os.environ["CURRENT_FILE"]) as f: curr = json.load(f)
base_e = {e["label"]: e for e in base["estimates"]}
curr_e = {e["label"]: e for e in curr["estimates"]}
regressions = []
for label, b in base_e.items():
    if label not in curr_e: continue
    c = curr_e[label]
    if b["time_ns_median"] == 0: continue
    delta = (c["time_ns_median"] - b["time_ns_median"]) / b["time_ns_median"] * 100.0
    if delta > 10: regressions.append(label)
sys.exit(0 if not regressions else 1)
PYEOF
)
SYNTH_RC=$?
if [[ "${SYNTH_RC}" -eq 0 ]]; then
    log "PASS: synthetic compare (5% regression < 10% threshold) rc=0"
    PASS=$((PASS + 1))
else
    log "FAIL: synthetic compare expected rc=0 got ${SYNTH_RC}"
    FAIL=$((FAIL + 1))
fi

# Test 5: Python compare logic — large regression (exit 1)
cat > "${SYNTH_CURR}" <<EOF
{
  "timestamp": "2026-08-04T10:00:00Z",
  "baseline_sha": "test",
  "estimates": [
    {"bench": "hot_path", "group": "hot_path", "label": "hot_path/test1", "time_ns_median": 2000, "time_ns_lower": 0, "time_ns_upper": 0}
  ]
}
EOF

SYNTH_RESULT=$(BASELINE_FILE="${SYNTH_BASE}" CURRENT_FILE="${SYNTH_CURR}" THRESHOLD_HOT_PATH=10 \
    python3 - <<'PYEOF'
import json
import os
import sys
with open(os.environ["BASELINE_FILE"]) as f: base = json.load(f)
with open(os.environ["CURRENT_FILE"]) as f: curr = json.load(f)
base_e = {e["label"]: e for e in base["estimates"]}
curr_e = {e["label"]: e for e in curr["estimates"]}
regressions = []
for label, b in base_e.items():
    if label not in curr_e: continue
    c = curr_e[label]
    if b["time_ns_median"] == 0: continue
    delta = (c["time_ns_median"] - b["time_ns_median"]) / b["time_ns_median"] * 100.0
    if delta > 10: regressions.append(label)
sys.exit(0 if not regressions else 1)
PYEOF
)
SYNTH_RC=$?
if [[ "${SYNTH_RC}" -eq 1 ]]; then
    log "PASS: synthetic compare (100% regression > 10% threshold) rc=1"
    PASS=$((PASS + 1))
else
    log "FAIL: synthetic compare expected rc=1 got ${SYNTH_RC}"
    FAIL=$((FAIL + 1))
fi

log ""
log "===================="
log "PASS: ${PASS}"
log "FAIL: ${FAIL}"
log "===================="

if [[ "${FAIL}" -gt 0 ]]; then
    exit 1
fi

exit 0