#!/usr/bin/env bash
set -uo pipefail

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

log_section "03: dispatch (REQUIRE_TOOLS=${REQUIRE_TOOLS})"

ALL_WORKLOADS=$(all_workloads)
ALL_TOOLS=$(all_tools)

TOTAL_CELLS=0
NA_CELLS=0
SKIPPED_CELLS=0
COMPLETED_CELLS=0
FAILED_CELLS=0
SUPPORTED_CELLS=0
SUPPORTED_COMPLETED=0
SUPPORTED_SKIPPED=0
SUPPORTED_FAILED=0

for w in ${ALL_WORKLOADS}; do
    for r in $(seq 1 "${RUNS}"); do
        TRANSPORT_FROM_W="unknown"
        case "${w}" in
            udp_*)   TRANSPORT_FROM_W="udp" ;;
            tcp_*)   TRANSPORT_FROM_W="tcp" ;;
            tls_*)   TRANSPORT_FROM_W="tls" ;;
            kafka_*) TRANSPORT_FROM_W="kafka" ;;
        esac

        for t in ${ALL_TOOLS}; do
            runner="${SCRIPTS_DIR}/03_run_${t}.sh"
            if [[ ! -f "${runner}" ]]; then
                log_warn "no runner for tool: ${t}"
                continue
            fi

            if tool_supports_transport "${t}" "${TRANSPORT_FROM_W}"; then
                SUPPORTED="yes"
            else
                SUPPORTED="no"
            fi

            TOTAL_CELLS=$((TOTAL_CELLS + 1))
            if [[ "${SUPPORTED}" == "yes" ]]; then
                SUPPORTED_CELLS=$((SUPPORTED_CELLS + 1))
            fi

            if ! bash "${runner}" "${w}" "${r}"; then
                FAILED_CELLS=$((FAILED_CELLS + 1))
                if [[ "${SUPPORTED}" == "yes" ]]; then
                    SUPPORTED_FAILED=$((SUPPORTED_FAILED + 1))
                fi
                continue
            fi

            meta_file="${RESULTS_DIR}/${w}/run_${r}/${t}.meta.json"
            if [[ ! -f "${meta_file}" ]]; then
                continue
            fi
            cell_status=$(python3 -c "import json; print(json.load(open('${meta_file}')).get('status', 'unknown'))" 2>/dev/null || echo "unknown")
            case "${cell_status}" in
                completed) COMPLETED_CELLS=$((COMPLETED_CELLS + 1))
                           if [[ "${SUPPORTED}" == "yes" ]]; then
                               SUPPORTED_COMPLETED=$((SUPPORTED_COMPLETED + 1))
                           fi
                           ;;
                skipped)   SKIPPED_CELLS=$((SKIPPED_CELLS + 1))
                           if [[ "${SUPPORTED}" == "yes" ]]; then
                               SUPPORTED_SKIPPED=$((SUPPORTED_SKIPPED + 1))
                           fi
                           ;;
                n_a)       NA_CELLS=$((NA_CELLS + 1)) ;;
            esac
        done
    done
done

log_info "dispatch summary:"
log_info "  total=${TOTAL_CELLS} supported=${SUPPORTED_CELLS}"
log_info "  n/a=${NA_CELLS} skipped=${SKIPPED_CELLS} completed=${COMPLETED_CELLS} failed=${FAILED_CELLS}"
log_info "  supported_completed=${SUPPORTED_COMPLETED} supported_skipped=${SUPPORTED_SKIPPED} supported_failed=${SUPPORTED_FAILED}"

if [[ "${REQUIRE_TOOLS}" == "1" ]]; then
    if [[ "${SUPPORTED_SKIPPED}" -gt 0 ]]; then
        log_error "REQUIRE_TOOLS=1: ${SUPPORTED_SKIPPED} supported cell(s) skipped (required tool missing)"
        exit 2
    fi
    if [[ "${SUPPORTED_FAILED}" -gt 0 ]]; then
        log_error "${SUPPORTED_FAILED} supported cell(s) failed"
        exit 1
    fi
    if [[ "${SUPPORTED_COMPLETED}" -lt "${SUPPORTED_CELLS}" ]]; then
        log_error "REQUIRE_TOOLS=1: not all supported cells completed"
        exit 1
    fi
    log_ok "all supported cells completed (or unsupported = n/a)"
    exit 0
else
    if [[ "${FAILED_CELLS}" -gt 0 ]]; then
        log_error "${FAILED_CELLS} cell(s) failed"
        exit 1
    fi
    log_ok "all cells accounted for (n/a / skipped / completed)"
    exit 0
fi
