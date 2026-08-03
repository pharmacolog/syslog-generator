#!/usr/bin/env bash
set -uo pipefail

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

log_section "02: dry-run enumeration"

ALL_WORKLOADS=$(all_workloads)
ALL_TOOLS=$(all_tools)

TOTAL=0
for w in ${ALL_WORKLOADS}; do
    for r in $(seq 1 "${RUNS}"); do
        for t in ${ALL_TOOLS}; do
            runner="${SCRIPTS_DIR}/03_run_${t}.sh"
            if [[ -f "${runner}" ]]; then
                printf '  [DRY] %s / %s / run %d -> %s %s %d\n' \
                       "${w}" "${t}" "${r}" "${runner}" "${w}" "${r}"
                TOTAL=$((TOTAL + 1))
            fi
        done
    done
done

log_ok "dry-run enumerated ${TOTAL} (workload, tool, run) cells"
echo
echo "Run with REQUIRE_TOOLS=1 to actually execute them:"
echo "  make all REQUIRE_TOOLS=1"
