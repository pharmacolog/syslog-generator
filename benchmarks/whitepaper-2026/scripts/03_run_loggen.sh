#!/usr/bin/env bash
set -uo pipefail

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

WORKLOAD_ID="${1:-}"
RUN_IDX="${2:-1}"
if [[ -z "${WORKLOAD_ID}" || "${RUN_IDX}" =~ ^[^0-9]+$ ]]; then
    log_error "usage: $0 <workload_id> [<run_idx>]"
    exit 2
fi

log_section "03: run_loggen ${WORKLOAD_ID} (run ${RUN_IDX})"

CONFIG_PATH="${CONFIGS_DIR}/workload_${WORKLOAD_ID}.json"
[[ ! -f "${CONFIG_PATH}" ]] && { log_error "config not found: ${CONFIG_PATH}"; exit 2; }

RESULT_DIR="${RESULTS_DIR}/${WORKLOAD_ID}/run_${RUN_IDX}"
mkdir -p "${RESULT_DIR}"

OUT_FILE="${RESULT_DIR}/loggen.stdout"
ERR_FILE="${RESULT_DIR}/loggen.stderr"
META_FILE="${RESULT_DIR}/loggen.meta.json"

TRANSPORT=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['transport'])")
RATE=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['target_msg_per_sec'])")
SIZE=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['target_bytes_per_msg'])")
PORT=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_receiver']['port'])")

if [[ "${TRANSPORT}" == "udp" ]]; then
    SUPPORTED="true"
    NA_REASON=""
else
    SUPPORTED="false"
    NA_REASON="loggen (syslog-ng) is UDP-only; ${TRANSPORT} not supported"
fi

AVAILABLE="false"
[[ -n "${LOGGEN_BIN}" && -x "${LOGGEN_BIN}" ]] && AVAILABLE="true"

SECS_INT=$(python3 -c "import math; print(int(math.ceil(${DURATION_SECS})))")
TOTAL_MESSAGES=$((RATE * SECS_INT))

META_ARGV=("${LOGGEN_BIN:-loggen}" "-i" "-D" "-s" "${SIZE}" "-r" "${RATE}" "-I" "${SECS_INT}" "127.0.0.1" "${PORT}")

write_meta() {
    local mode="$1"
    local status="$2"
    python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'loggen',
    'mode': '${mode}',
    'status': '${status}',
    'available': $([[ "${AVAILABLE}" == "true" ]] && echo True || echo False),
    'supported_for_transport': $([[ "${SUPPORTED}" == "true" ]] && echo True || echo False),
    'argv': $(python3 -c "import json,sys; print(json.dumps(sys.argv[1:]))" "${META_ARGV[@]}"),
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}
if '${NA_REASON}':
    meta['na_reason'] = '${NA_REASON}'
open('${META_FILE}', 'w').write(json.dumps(meta, indent=2, ensure_ascii=False) + '\n')
"
}

if [[ "${REQUIRE_TOOLS}" != "1" ]]; then
    if [[ "${SUPPORTED}" != "true" ]]; then
        log_na "loggen N/A for ${TRANSPORT}: ${NA_REASON}"
        write_meta "dry_run" "n_a"
    elif [[ "${AVAILABLE}" != "true" ]]; then
        log_dry "would skip loggen: not installed"
        write_meta "dry_run" "skipped"
    else
        log_dry "would run: ${META_ARGV[*]}"
        write_meta "dry_run" "dry_run"
    fi
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

if [[ "${SUPPORTED}" != "true" ]]; then
    log_na "loggen N/A for ${TRANSPORT}: ${NA_REASON}"
    write_meta "real" "n_a"
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

if [[ "${AVAILABLE}" != "true" ]]; then
    log_error "loggen not installed (REQUIRE_TOOLS=1)"
    python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'loggen',
    'mode': 'real',
    'status': 'skipped',
    'available': False,
    'supported_for_transport': True,
    'skip_reason': 'loggen not installed (syslog-ng not found)',
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}
open('${META_FILE}', 'w').write(json.dumps(meta, indent=2, ensure_ascii=False) + '\n')
"
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

RECV_ARGS=("udp" "127.0.0.1" "${PORT}" "${DURATION_SECS}")

python3 "$(dirname "$0")/receiver.py" "${RECV_ARGS[@]}" > "${RESULT_DIR}/receiver.stdout.json" 2>&1 &
RECV_PID=$!
register_cleanup "${RECV_PID}"
sleep 0.5

log_info "running: ${META_ARGV[*]}"
run_capture "${OUT_FILE}" "${ERR_FILE}" "${META_FILE}" 0 "${META_ARGV[@]}"
RUN_EXIT_CODE_LOCAL=${RUN_EXIT_CODE}

wait "${RECV_PID}" 2>/dev/null || true

STARTED_AT="$(iso_timestamp)"
FINISHED_AT="$(iso_timestamp)"

export REC_OUT="${RESULT_DIR}/receiver.stdout.json"
export META_OUT="${META_FILE}"
export WORKLOAD_ID RUN_IDX
export RUN_EXIT_CODE="${RUN_EXIT_CODE_LOCAL}"
export RUN_DURATION_SECS DURATION_SECS
export RATE_TOLERANCE_FRACTION SIZE_TOLERANCE_FRACTION
export TOOL_BIN="${LOGGEN_BIN}"
export TOOL_NAME="loggen"
export TARGET_RATE="${RATE}"
export SPEC_SIZE="${SIZE}"
export META_ARGV_STR=$(python3 -c "import json, sys; print(json.dumps(sys.argv[1:]))" "${META_ARGV[@]}")
export STARTED_AT FINISHED_AT

python3 "$(dirname "$0")/measure.py"

CELL_STATUS=$(python3 -c "import json; print(json.load(open('${META_FILE}'))['status'])")
SUMMARY=$(python3 -c "
import json
m = json.load(open('${META_FILE}'))
meas = m.get('measurements', {})
print(f\"rate={meas.get('achieved_msg_per_sec', 0)} msg/s ({meas.get('rate_pct_of_target', 0)}% of target) size={meas.get('actual_bytes_per_msg', 0)}B (dev {meas.get('size_pct_deviation', 0)}%)\")
")
if [[ "${CELL_STATUS}" == "failed" ]]; then
    log_warn "loggen ${WORKLOAD_ID} run ${RUN_IDX}: status=failed ${SUMMARY}"
    exit 1
fi
log_ok "loggen ${WORKLOAD_ID} run ${RUN_IDX}: ${SUMMARY}"
exit 0
