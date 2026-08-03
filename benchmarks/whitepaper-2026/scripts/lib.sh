#!/usr/bin/env bash
set -uo pipefail

_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../configs/env.sh
source "${_LIB_DIR}/../configs/env.sh"

: "${RATE_TOLERANCE_FRACTION:=0.05}"
: "${SIZE_TOLERANCE_FRACTION:=0.05}"

log_info() {
    printf '%s[INFO]%s %s\n' "${C_BLUE}" "${C_OFF}" "$*" >&2
}
log_warn() {
    printf '%s[WARN]%s %s\n' "${C_YELLOW}" "${C_OFF}" "$*" >&2
}
log_error() {
    printf '%s[ERROR]%s %s\n' "${C_RED}" "${C_OFF}" "$*" >&2
}
log_ok() {
    printf '%s[OK]%s   %s\n' "${C_GREEN}" "${C_OFF}" "$*" >&2
}
log_dry() {
    printf '%s[DRY]%s  %s\n' "${C_BOLD}" "${C_OFF}" "$*" >&2
}
log_section() {
    printf '\n%s========== %s ==========%s\n' "${C_BOLD}" "$*" "${C_OFF}" >&2
}
log_na() {
    printf '%s[N/A]%s  %s\n' "${C_YELLOW}" "${C_OFF}" "$*" >&2
}

require_env() {
    local var="$1"
    if [[ -z "${!var:-}" ]]; then
        log_error "required env var not set: ${var}"
        return 1
    fi
}
require_cmd() {
    local cmd="$1"
    if ! command -v "${cmd}" >/dev/null 2>&1; then
        log_error "required command not found: ${cmd}"
        return 1
    fi
}

iso_timestamp() {
    date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || \
    python3 -c "import datetime; print(datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))"
}

run_capture() {
    local stdout_path="$1"
    local stderr_path="$2"
    local meta_path="$3"
    local expected_exit="${4:-0}"
    shift 4

    mkdir -p "$(dirname "${stdout_path}")" \
             "$(dirname "${stderr_path}")" \
             "$(dirname "${meta_path}")"

    : > "${stdout_path}"
    : > "${stderr_path}"

    local t0 t1
    t0=$(python3 -c 'import time; print(time.monotonic_ns())')

    set +e
    "$@" >"${stdout_path}" 2>"${stderr_path}"
    RUN_EXIT_CODE=$?
    set -e

    t1=$(python3 -c 'import time; print(time.monotonic_ns())')
    RUN_DURATION_SECS=$(python3 -c "print(($t1 - $t0) / 1e9)")

    python3 -c "
import json
open('${meta_path}.timing', 'w').write(json.dumps({
    'exit_code': ${RUN_EXIT_CODE},
    'expected_exit_code': ${expected_exit},
    'argv': $(python3 -c "import json, sys; print(json.dumps(sys.argv[1:]))" "$@"),
    'cwd': '$(pwd)',
    'duration_secs': ${RUN_DURATION_SECS},
    'started_at': '$(iso_timestamp)',
    'finished_at': '$(iso_timestamp)',
}, indent=2) + '\n')
"
}

in_whitelist() {
    local needle="$1"; shift
    local item
    for item in "$@"; do
        if [[ "${item}" == "${needle}" ]]; then
            return 0
        fi
    done
    return 1
}

all_workloads() {
    if [[ -z "${WORKLOADS}" ]]; then
        echo "udp_100rps_256b tcp_10krps_1kb tls_5krps_1kb kafka_50krps_256b"
    else
        echo "${WORKLOADS}"
    fi
}

all_tools() {
    if [[ -z "${TOOLS}" ]]; then
        echo "syslog_generator loggen flog tcpkali"
    else
        echo "${TOOLS}"
    fi
}

COMPARED_TOOLS="syslog_generator loggen flog tcpkali"

tool_supports_transport() {
    local tool="$1"
    local transport="$2"
    case "${tool}:${transport}" in
        syslog_generator:udp|syslog_generator:tcp|syslog_generator:tls) return 0 ;;
        loggen:udp) return 0 ;;
        flog:*) return 1 ;;
        tcpkali:tcp|tcpkali:tls) return 0 ;;
        *) return 1 ;;
    esac
}

register_cleanup() {
    CLEANUP_PIDS+=("$1")
}

run_cleanup() {
    local pid
    for pid in "${CLEANUP_PIDS[@]:-}"; do
        if [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null; then
            kill "${pid}" 2>/dev/null || true
            wait "${pid}" 2>/dev/null || true
        fi
    done
}

CLEANUP_PIDS=()
trap run_cleanup EXIT INT TERM
