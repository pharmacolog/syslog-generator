#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS_DIR="${SCRIPT_DIR}"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=../scripts/lib.sh
source "${ROOT_DIR}/scripts/lib.sh"

FAILS=0
PASSES=0

assert_eq() {
    local got="$1" expected="$2" msg="$3"
    if [[ "${got}" == "${expected}" ]]; then
        log_ok "PASS: ${msg}"
        PASSES=$((PASSES + 1))
    else
        log_error "FAIL: ${msg} (got '${got}', expected '${expected}')"
        FAILS=$((FAILS + 1))
    fi
}

assert_contains() {
    local haystack="$1" needle="$2" msg="$3"
    if [[ "${haystack}" == *"${needle}"* ]]; then
        log_ok "PASS: ${msg}"
        PASSES=$((PASSES + 1))
    else
        log_error "FAIL: ${msg} (expected to contain '${needle}')"
        FAILS=$((FAILS + 1))
    fi
}

assert_file_exists() {
    local path="$1" msg="$2"
    if [[ -f "${path}" ]]; then
        log_ok "PASS: ${msg}"
        PASSES=$((PASSES + 1))
    else
        log_error "FAIL: ${msg} (${path} not found)"
        FAILS=$((FAILS + 1))
    fi
}

log_section "test: ROOT_DIR resolves to harness parent"
EXPECTED_ROOT=$(cd "${HARNESS_DIR}/.." && pwd)
assert_eq "${ROOT_DIR}" "${EXPECTED_ROOT}" "ROOT_DIR = harness parent (benchmarks/whitepaper-2026)"

log_section "test: env.sh loads"
TEST_OUT=$(bash -c "source '${ROOT_DIR}/configs/env.sh' && echo OK_LOADED" 2>&1)
assert_eq "${TEST_OUT}" "OK_LOADED" "env.sh source is clean"

log_section "test: in_whitelist"
if in_whitelist "tcp" "udp" "tcp" "tls"; then
    log_ok "PASS: in_whitelist finds match"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: in_whitelist should find tcp"
    FAILS=$((FAILS + 1))
fi
if in_whitelist "kafka" "udp" "tcp"; then
    log_error "FAIL: in_whitelist should not find kafka"
    FAILS=$((FAILS + 1))
else
    log_ok "PASS: in_whitelist correctly rejects non-match"
    PASSES=$((PASSES + 1))
fi

log_section "test: all_workloads / all_tools"
DEFAULT_W=$(WORKLOADS="" all_workloads)
assert_eq "${DEFAULT_W}" "udp_100rps_256b tcp_10krps_1kb tls_5krps_1kb kafka_50krps_256b" "all_workloads default"

DEFAULT_T=$(TOOLS="" all_tools)
assert_eq "${DEFAULT_T}" "syslog_generator loggen flog tcpkali" "all_tools default (4 tools, no kcat)"

WHITELIST_W=$(WORKLOADS="udp_100rps_256b kafka_50krps_256b" all_workloads)
assert_eq "${WHITELIST_W}" "udp_100rps_256b kafka_50krps_256b" "all_workloads whitelist"

WHITELIST_T=$(TOOLS="loggen tcpkali" all_tools)
assert_eq "${WHITELIST_T}" "loggen tcpkali" "all_tools whitelist"

log_section "test: tool_supports_transport (Issue #106 4-tool matrix)"
# Issue #197 (v11.6): syslog_generator теперь supports kafka (real consumer
# через kafka-python). Остальные 3×4 cells без изменений.
if tool_supports_transport "syslog_generator" "udp" && \
   tool_supports_transport "syslog_generator" "tcp" && \
   tool_supports_transport "syslog_generator" "tls" && \
   tool_supports_transport "syslog_generator" "kafka" && \
   tool_supports_transport "loggen" "udp" && \
   ! tool_supports_transport "loggen" "tcp" && \
   ! tool_supports_transport "loggen" "tls" && \
   ! tool_supports_transport "loggen" "kafka" && \
   ! tool_supports_transport "flog" "udp" && \
   ! tool_supports_transport "flog" "tcp" && \
   ! tool_supports_transport "flog" "tls" && \
   ! tool_supports_transport "flog" "kafka" && \
   tool_supports_transport "tcpkali" "tcp" && \
   tool_supports_transport "tcpkali" "tls" && \
   ! tool_supports_transport "tcpkali" "udp" && \
   ! tool_supports_transport "tcpkali" "kafka"; then
    log_ok "PASS: tool_supports_transport matrix correct (syslog_generator now supports kafka per Issue #197)"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: tool_supports_transport matrix wrong"
    FAILS=$((FAILS + 1))
fi

log_section "test: validate.py"
if python3 "${ROOT_DIR}/configs/validate.py" >/dev/null 2>&1; then
    log_ok "PASS: validate.py with no args exits 0"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: validate.py fails on default workload set"
    FAILS=$((FAILS + 1))
fi

if python3 "${ROOT_DIR}/configs/validate.py" udp_100rps_256b >/dev/null 2>&1; then
    log_ok "PASS: validate.py with single workload exits 0"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: validate.py with single workload"
    FAILS=$((FAILS + 1))
fi

OUT=$(python3 "${ROOT_DIR}/configs/validate.py" bogus_workload 2>&1 || true)
assert_contains "${OUT}" "unknown workload_id" "validate.py rejects unknown workload"

log_section "test: configs JSON-parseable"
for w in udp_100rps_256b tcp_10krps_1kb tls_5krps_1kb kafka_50krps_256b; do
    if python3 -c "import json; json.load(open('${ROOT_DIR}/configs/workload_${w}.json'))" 2>/dev/null; then
        log_ok "PASS: configs/workload_${w}.json is valid JSON"
        PASSES=$((PASSES + 1))
    else
        log_error "FAIL: configs/workload_${w}.json is invalid JSON"
        FAILS=$((FAILS + 1))
    fi
done

log_section "test: runner scripts exist (4 compared, no kcat)"
for t in syslog_generator loggen flog tcpkali; do
    assert_file_exists "${ROOT_DIR}/scripts/03_run_${t}.sh" "03_run_${t}.sh exists"
done
if [[ ! -f "${ROOT_DIR}/scripts/03_run_kcat.sh" ]]; then
    log_ok "PASS: 03_run_kcat.sh does NOT exist (kcat out of scope)"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: 03_run_kcat.sh exists (kcat should be removed)"
    FAILS=$((FAILS + 1))
fi

log_section "test: dry-run metadata"
TMPDIR=$(mktemp -d)
mkdir -p "${TMPDIR}/udp_100rps_256b/run_1"

REQUIRE_TOOLS=0 bash "${ROOT_DIR}/scripts/03_run_syslog_generator.sh" "udp_100rps_256b" 1 \
    > "${TMPDIR}/run.stdout" 2> "${TMPDIR}/run.stderr"
RC=$?
assert_eq "${RC}" "0" "dry-run of syslog_generator exits 0"

META="${ROOT_DIR}/results/udp_100rps_256b/run_1/syslog_generator.meta.json"
assert_file_exists "${META}" "dry-run wrote meta.json"

if python3 -c "import json; json.load(open('${META}'))" 2>/dev/null; then
    log_ok "PASS: dry-run meta.json is valid JSON"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: dry-run meta.json is invalid JSON"
    FAILS=$((FAILS + 1))
fi

MODE=$(python3 -c "import json; print(json.load(open('${META}'))['mode'])")
assert_eq "${MODE}" "dry_run" "dry-run meta has mode=dry_run"

HAS_MEAS=$(python3 -c "import json; d=json.load(open('${META}')); print('yes' if 'measurements' in d else 'no')")
assert_eq "${HAS_MEAS}" "no" "dry-run meta does NOT have measurements (no fabrication)"

rm -rf "${ROOT_DIR}/results/udp_100rps_256b/run_1"
rmdir "${ROOT_DIR}/results/udp_100rps_256b" 2>/dev/null || true
rm -rf "${TMPDIR}"

log_section "test: n/a-on-unsupported-tool (Kafka for loggen)"
TMPDIR=$(mktemp -d)
mkdir -p "${TMPDIR}/kafka_50krps_256b/run_1"

FAKE_LOGGEN="${TMPDIR}/fake_loggen"
cat > "${FAKE_LOGGEN}" <<'EOF'
#!/usr/bin/env bash
echo "fake loggen" >&2
exit 0
EOF
chmod +x "${FAKE_LOGGEN}"

LOGGEN_BIN="${FAKE_LOGGEN}" REQUIRE_TOOLS=0 \
    bash "${ROOT_DIR}/scripts/03_run_loggen.sh" kafka_50krps_256b 1 \
    > "${TMPDIR}/loggen.stdout" 2> "${TMPDIR}/loggen.stderr"
RC=$?
assert_eq "${RC}" "0" "loggen on Kafka workload exits 0 (n/a)"

META="${ROOT_DIR}/results/kafka_50krps_256b/run_1/loggen.meta.json"
assert_file_exists "${META}" "loggen Kafka meta.json exists"

STATUS=$(python3 -c "import json; d=json.load(open('${META}')); print(d.get('status'))")
assert_eq "${STATUS}" "n_a" "loggen status is n_a for Kafka"

NA_REASON=$(python3 -c "import json; d=json.load(open('${META}')); print(d.get('na_reason', ''))")
assert_contains "${NA_REASON}" "UDP" "loggen n/a reason mentions UDP-only"

rm -rf "${ROOT_DIR}/results/kafka_50krps_256b"
rm -rf "${TMPDIR}"

log_section "test: flog always N/A for network workloads"
TMPDIR=$(mktemp -d)
mkdir -p "${TMPDIR}/udp_100rps_256b/run_1"

FAKE_FLOG="${TMPDIR}/fake_flog"
cat > "${FAKE_FLOG}" <<'EOF'
#!/usr/bin/env bash
echo "fake flog" >&2
exit 0
EOF
chmod +x "${FAKE_FLOG}"

FLOG_BIN="${FAKE_FLOG}" REQUIRE_TOOLS=0 \
    bash "${ROOT_DIR}/scripts/03_run_flog.sh" udp_100rps_256b 1 \
    > "${TMPDIR}/flog.stdout" 2> "${TMPDIR}/flog.stderr"
RC=$?
assert_eq "${RC}" "0" "flog on UDP workload exits 0 (n/a)"

META="${ROOT_DIR}/results/udp_100rps_256b/run_1/flog.meta.json"
assert_file_exists "${META}" "flog UDP meta.json exists"

STATUS=$(python3 -c "import json; print(json.load(open('${META}'))['status'])")
assert_eq "${STATUS}" "n_a" "flog status is n_a for UDP"

rm -rf "${ROOT_DIR}/results/udp_100rps_256b"
rm -rf "${TMPDIR}"

log_section "test: schema files"
assert_file_exists "${REPO_ROOT}/perf/whitepaper-results.json" "perf/whitepaper-results.json exists"
if python3 -c "import json; json.load(open('${REPO_ROOT}/perf/whitepaper-results.json'))" 2>/dev/null; then
    log_ok "PASS: perf/whitepaper-results.json is valid JSON"
    PASSES=$((PASSES + 1))
else
    log_error "FAIL: perf/whitepaper-results.json is invalid JSON"
    FAILS=$((FAILS + 1))
fi

SCHEMA_VER=$(python3 -c "import json; print(json.load(open('${REPO_ROOT}/perf/whitepaper-results.json'))['schema_version'])")
assert_eq "${SCHEMA_VER}" "1.0.0" "schema_version is 1.0.0"

echo
echo "=== harness/test_harness.sh summary ==="
echo "PASSES: ${PASSES}"
echo "FAILS:  ${FAILS}"

if [[ "${FAILS}" -gt 0 ]]; then
    exit 1
fi
exit 0
