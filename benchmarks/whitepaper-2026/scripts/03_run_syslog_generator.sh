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

log_section "03: run_syslog_generator ${WORKLOAD_ID} (run ${RUN_IDX})"

CONFIG_PATH="${CONFIGS_DIR}/workload_${WORKLOAD_ID}.json"
[[ ! -f "${CONFIG_PATH}" ]] && { log_error "config not found: ${CONFIG_PATH}"; exit 2; }

RESULT_DIR="${RESULTS_DIR}/${WORKLOAD_ID}/run_${RUN_IDX}"
mkdir -p "${RESULT_DIR}"

OUT_FILE="${RESULT_DIR}/syslog_generator.stdout"
ERR_FILE="${RESULT_DIR}/syslog_generator.stderr"
META_FILE="${RESULT_DIR}/syslog_generator.meta.json"

TRANSPORT=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['transport'])")

SUPPORTED="false"
NA_REASON=""
if [[ "${TRANSPORT}" == "udp" || "${TRANSPORT}" == "tcp" || "${TRANSPORT}" == "tls" ]]; then
    SUPPORTED="true"
elif [[ "${TRANSPORT}" == "kafka" ]]; then
    SUPPORTED="false"
    NA_REASON="syslog-generator Kafka transport exists but no real consumer; marked N/A until kafka consumer dependency is satisfied"
fi

SG_AVAILABLE="false"
[[ -x "${SYSLOG_GENERATOR_BIN}" ]] && SG_AVAILABLE="true"

write_meta() {
    local mode="$1"
    local status="$2"
    python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'syslog_generator',
    'mode': '${mode}',
    'status': '${status}',
    'available': $([[ "${SG_AVAILABLE}" == "true" ]] && echo True || echo False),
    'supported_for_transport': $([[ "${SUPPORTED}" == "true" ]] && echo True || echo False),
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
        log_na "syslog_generator N/A for ${TRANSPORT}: ${NA_REASON}"
        write_meta "dry_run" "n_a"
    else
        log_dry "would run: ${SYSLOG_GENERATOR_BIN} -p ${CONFIG_PATH} --duration ${DURATION_SECS}"
        META_ARGV=("${SYSLOG_GENERATOR_BIN}" "-p" "${CONFIG_PATH}" "--duration" "${DURATION_SECS}")
        [[ -n "${HARNESS_RATE:-}" ]] && META_ARGV+=("--rate" "${HARNESS_RATE}")
        python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'syslog_generator',
    'mode': 'dry_run',
    'available': $([[ "${SG_AVAILABLE}" == "true" ]] && echo True || echo False),
    'status': 'dry_run',
    'exit_code': 0,
    'duration_secs': 0.0,
    'argv': $(python3 -c "import json,sys; print(json.dumps(sys.argv[1:]))" "${META_ARGV[@]}"),
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}
open('${META_FILE}', 'w').write(json.dumps(meta, indent=2, ensure_ascii=False) + '\n')
"
    fi
    : > "${OUT_FILE}"
    : > "${ERR_FILE}"
    log_ok "dry_run metadata written to ${META_FILE}"
    exit 0
fi

if [[ "${SUPPORTED}" != "true" ]]; then
    log_na "syslog_generator N/A for ${TRANSPORT}: ${NA_REASON}"
    write_meta "real" "n_a"
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

if [[ "${SG_AVAILABLE}" != "true" ]]; then
    log_error "syslog_generator not available (REQUIRE_TOOLS=1)"
    python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'syslog_generator',
    'mode': 'real',
    'status': 'skipped',
    'available': False,
    'supported_for_transport': True,
    'skip_reason': 'syslog_generator not built; cargo build --release failed',
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}
open('${META_FILE}', 'w').write(json.dumps(meta, indent=2, ensure_ascii=False) + '\n')
"
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

FRAMING=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['framing'])")
RECV_PORT=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_receiver']['port'])")
TARGET_RATE=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['target_msg_per_sec'])")
SPEC_SIZE=$(python3 -c "import json; print(json.load(open('${CONFIG_PATH}'))['_issue_spec']['target_bytes_per_msg'])")
RECV_HOST="127.0.0.1"
RECV_OUT_FILE="${RESULT_DIR}/receiver.stdout.json"

RECV_ARGS=("${TRANSPORT}" "${RECV_HOST}" "${RECV_PORT}" "${DURATION_SECS}")
if [[ "${TRANSPORT}" == "tls" ]]; then
    RECV_ARGS+=("${TLS_CERT}" "${TLS_KEY}")
fi
if [[ "${TRANSPORT}" == "tcp" || "${TRANSPORT}" == "tls" ]]; then
    RECV_ARGS+=("${FRAMING}")
fi

log_info "starting receiver: ${RECV_ARGS[*]}"
python3 "$(dirname "$0")/receiver.py" "${RECV_ARGS[@]}" > "${RECV_OUT_FILE}" 2>&1 &
RECV_PID=$!
register_cleanup "${RECV_PID}"
sleep 0.5

META_ARGV=("${SYSLOG_GENERATOR_BIN}" "-p" "${CONFIG_PATH}" "--duration" "${DURATION_SECS}")
if [[ -n "${HARNESS_RATE:-}" ]]; then
    META_ARGV+=("--rate" "${HARNESS_RATE}")
    log_info "HARNESS_RATE override: ${HARNESS_RATE}"
fi

log_info "running: ${META_ARGV[*]}"
run_capture "${OUT_FILE}" "${ERR_FILE}" "${META_FILE}" 0 \
    "${META_ARGV[@]}"
RUN_EXIT_CODE_LOCAL=${RUN_EXIT_CODE}

wait "${RECV_PID}" 2>/dev/null || true
RECV_EXIT=$?

STARTED_AT="$(iso_timestamp)"
FINISHED_AT="$(iso_timestamp)"

export REC_OUT="${RECV_OUT_FILE}"
export META_OUT="${META_FILE}"
export WORKLOAD_ID RUN_IDX
export RUN_EXIT_CODE="${RUN_EXIT_CODE_LOCAL}"
export RUN_DURATION_SECS DURATION_SECS
export RATE_TOLERANCE_FRACTION SIZE_TOLERANCE_FRACTION
export TOOL_BIN="${SYSLOG_GENERATOR_BIN}"
export TOOL_NAME="syslog_generator"
export TARGET_RATE="${TARGET_RATE}"
export SPEC_SIZE="${SPEC_SIZE}"
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
    log_warn "syslog_generator ${WORKLOAD_ID} run ${RUN_IDX}: status=failed ${SUMMARY}"
    exit 1
fi
log_ok "syslog_generator ${WORKLOAD_ID} run ${RUN_IDX}: ${SUMMARY}"
exit 0
