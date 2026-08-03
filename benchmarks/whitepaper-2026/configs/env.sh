#!/usr/bin/env bash
: "${DURATION_SECS:=30}"
: "${RUNS:=1}"

: "${WORKLOADS:=}"
: "${TOOLS:=}"

: "${UDP_RECV_PORT:=5140}"
: "${TCP_RECV_PORT:=6010}"
: "${TLS_RECV_PORT:=6514}"
: "${KAFKA_RECV_PORT:=9092}"

: "${TLS_CERT_DIR:=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/docker/certs}"
: "${TLS_CERT:=}"
: "${TLS_KEY:=}"

: "${REQUIRE_TOOLS:=0}"
: "${SKIP_BUILD:=0}"
: "${PRESERVE_RESULTS:=0}"
: "${RATE_TOLERANCE_FRACTION:=0.05}"
: "${SIZE_TOLERANCE_FRACTION:=0.05}"

: "${SCRIPT_DIR:=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
: "${WHITEPAPER_DIR:=${SCRIPT_DIR}}"
: "${REPO_ROOT:=$(cd "${SCRIPT_DIR}/../.." && pwd)}"
: "${CONFIGS_DIR:=${SCRIPT_DIR}/configs}"
: "${SCRIPTS_DIR:=${SCRIPT_DIR}/scripts}"
: "${DOCKER_DIR:=${SCRIPT_DIR}/docker}"
: "${HARNESS_DIR:=${SCRIPT_DIR}/harness}"
: "${RESULTS_DIR:=${SCRIPT_DIR}/results}"
: "${PERF_DIR:=${REPO_ROOT}/perf}"
: "${WP_RESULTS:=${PERF_DIR}/whitepaper-results.json}"
: "${WP_REPORT:=${RESULTS_DIR}/REPORT.md}"

: "${SYSLOG_GENERATOR_BIN:=${REPO_ROOT}/target/release/syslog-generator}"
: "${LOGGEN_BIN:=$(command -v loggen 2>/dev/null || true)}"
: "${FLOG_BIN:=$(command -v flog 2>/dev/null || true)}"
: "${TCPKALI_BIN:=$(command -v tcpkali 2>/dev/null || true)}"

if [[ -t 1 ]]; then
    C_RED=$'\033[0;31m'
    C_GREEN=$'\033[0;32m'
    C_YELLOW=$'\033[0;33m'
    C_BLUE=$'\033[0;34m'
    C_BOLD=$'\033[1m'
    C_OFF=$'\033[0m'
else
    C_RED=; C_GREEN=; C_YELLOW=; C_BLUE=; C_BOLD=; C_OFF=
fi

export DURATION_SECS RUNS WORKLOADS TOOLS
export UDP_RECV_PORT TCP_RECV_PORT TLS_RECV_PORT KAFKA_RECV_PORT
export TLS_CERT_DIR TLS_CERT TLS_KEY
export REQUIRE_TOOLS SKIP_BUILD PRESERVE_RESULTS
export RATE_TOLERANCE_FRACTION SIZE_TOLERANCE_FRACTION
export SCRIPT_DIR WHITEPAPER_DIR REPO_ROOT CONFIGS_DIR SCRIPTS_DIR DOCKER_DIR HARNESS_DIR RESULTS_DIR PERF_DIR WP_RESULTS WP_REPORT
export SYSLOG_GENERATOR_BIN LOGGEN_BIN FLOG_BIN TCPKALI_BIN
export C_RED C_GREEN C_YELLOW C_BLUE C_BOLD C_OFF
