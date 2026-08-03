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

log_section "03: run_flog ${WORKLOAD_ID} (run ${RUN_IDX})"

CONFIG_PATH="${CONFIGS_DIR}/workload_${WORKLOAD_ID}.json"
[[ ! -f "${CONFIG_PATH}" ]] && { log_error "config not found: ${CONFIG_PATH}"; exit 2; }

RESULT_DIR="${RESULTS_DIR}/${WORKLOAD_ID}/run_${RUN_IDX}"
mkdir -p "${RESULT_DIR}"
OUT_FILE="${RESULT_DIR}/flog.stdout"
ERR_FILE="${RESULT_DIR}/flog.stderr"
META_FILE="${RESULT_DIR}/flog.meta.json"

SUPPORTED="false"
NA_REASON="flog (mingrammer v0.4.3) has no native network output (stdout/file only) and fixed record size per log type; N/A for all network workloads"

AVAILABLE="false"
[[ -n "${FLOG_BIN}" && -x "${FLOG_BIN}" ]] && AVAILABLE="true"

write_meta() {
    local mode="$1"
    local status="$2"
    python3 -c "
import json
meta = {
    'workload_id': '${WORKLOAD_ID}',
    'run_idx': ${RUN_IDX},
    'tool': 'flog',
    'mode': '${mode}',
    'status': '${status}',
    'available': $([[ "${AVAILABLE}" == "true" ]] && echo True || echo False),
    'supported_for_transport': False,
    'is_compared_tool': True,
    'argv': [],
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}
if '${NA_REASON}':
    meta['na_reason'] = '${NA_REASON}'
open('${META_FILE}', 'w').write(json.dumps(meta, indent=2, ensure_ascii=False) + '\n')
"
}

if [[ "${REQUIRE_TOOLS}" != "1" ]]; then
    log_na "flog N/A for all network workloads"
    write_meta "dry_run" "n_a"
    : > "${OUT_FILE}"; : > "${ERR_FILE}"
    exit 0
fi

log_na "flog N/A for ${WORKLOAD_ID}: ${NA_REASON}"
write_meta "real" "n_a"
: > "${OUT_FILE}"; : > "${ERR_FILE}"
exit 0
