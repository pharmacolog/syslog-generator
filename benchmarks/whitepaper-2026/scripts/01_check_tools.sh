#!/usr/bin/env bash
set -uo pipefail

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

log_section "01: check_tools"

HOST_OS=$(uname -s)
HOST_ARCH=$(uname -m)
HOST_KERNEL=$(uname -r)
HOST_CPU_COUNT=$(sysctl -n hw.ncpu 2>/dev/null || nproc)

HOST_CPU_MODEL="unknown"
if [[ "${HOST_OS}" == "Darwin" ]]; then
    HOST_CPU_MODEL=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown")
elif [[ -r /proc/cpuinfo ]]; then
    HOST_CPU_MODEL=$(grep -m1 'model name' /proc/cpuinfo | sed 's/^[^:]*: *//' || echo "unknown")
fi

HOST_MEMORY_BYTES=0
if [[ "${HOST_OS}" == "Darwin" ]]; then
    HOST_MEMORY_BYTES=$(sysctl -n hw.memsize 2>/dev/null || echo 0)
elif [[ -r /proc/meminfo ]]; then
    HOST_MEMORY_BYTES=$(awk '/MemTotal/ {print $2 * 1024}' /proc/meminfo 2>/dev/null || echo 0)
fi

SG_AVAILABLE="false"
SG_PATH="null"
SG_VERSION="null"
if [[ -x "${SYSLOG_GENERATOR_BIN}" ]]; then
    SG_AVAILABLE="true"
    SG_PATH="${SYSLOG_GENERATOR_BIN}"
    SG_VERSION=$("${SYSLOG_GENERATOR_BIN}" --version 2>&1 | head -1 || echo "unknown")
    log_ok "syslog_generator: ${SG_PATH} (${SG_VERSION})"
elif [[ "${REQUIRE_TOOLS}" == "1" && "${SKIP_BUILD}" != "1" ]]; then
    log_warn "syslog_generator not built; building with cargo build --release..."
    if (cd "${REPO_ROOT}" && cargo build --release --locked --bin syslog-generator >/dev/null 2>&1); then
        if [[ -x "${SYSLOG_GENERATOR_BIN}" ]]; then
            SG_AVAILABLE="true"
            SG_PATH="${SYSLOG_GENERATOR_BIN}"
            SG_VERSION=$("${SYSLOG_GENERATOR_BIN}" --version 2>&1 | head -1 || echo "unknown")
            log_ok "syslog_generator: ${SG_PATH} (${SG_VERSION}) [after build]"
        fi
    else
        log_error "cargo build --release failed"
    fi
else
    log_warn "syslog_generator: not built (run \`cargo build --release\` in repo root)"
fi

export HOST_OS HOST_ARCH HOST_KERNEL HOST_CPU_COUNT HOST_CPU_MODEL HOST_MEMORY_BYTES
export SG_AVAILABLE SG_PATH SG_VERSION
export LOGGEN_BIN FLOG_BIN TCPKALI_BIN

python3 <<'PYEOF'
import json
import os
import subprocess


def get_version(bin_path):
    if not bin_path or not os.path.isfile(bin_path) or not os.access(bin_path, os.X_OK):
        return None
    try:
        result = subprocess.run(
            [bin_path, "--version"],
            capture_output=True, text=True, timeout=5,
        )
        first = (result.stdout or result.stderr or "").strip().splitlines()
        return first[0] if first else "unknown"
    except (subprocess.SubprocessError, FileNotFoundError, OSError):
        return "unknown"


def discover(bin_path):
    available = bool(bin_path and os.path.isfile(bin_path) and os.access(bin_path, os.X_OK))
    if not available:
        return {"available": False, "path": None, "version": None}
    return {
        "available": True,
        "path": bin_path,
        "version": get_version(bin_path),
    }


host = {
    "os": os.environ["HOST_OS"],
    "arch": os.environ["HOST_ARCH"],
    "kernel": os.environ["HOST_KERNEL"],
    "cpu_count": int(os.environ["HOST_CPU_COUNT"]),
    "cpu_model": os.environ["HOST_CPU_MODEL"],
    "memory_bytes": int(os.environ["HOST_MEMORY_BYTES"]),
}

syslog_generator = {
    "available": os.environ["SG_AVAILABLE"] == "true",
    "path": None if os.environ["SG_PATH"] in ("null", "") else os.environ["SG_PATH"],
    "version": None if os.environ["SG_VERSION"] in ("null", "") else os.environ["SG_VERSION"],
}

tool_versions = {
    "syslog_generator": syslog_generator,
    "loggen":  discover(os.environ["LOGGEN_BIN"]),
    "flog":    discover(os.environ["FLOG_BIN"]),
    "tcpkali": discover(os.environ["TCPKALI_BIN"]),
}

for name, info in tool_versions.items():
    info.setdefault("available", False)
    info.setdefault("path", None)
    info.setdefault("version", None)

out = {"host": host, "tool_versions": tool_versions}
print(json.dumps(out, indent=2, ensure_ascii=False))
PYEOF
