#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT
BIN_NAME="syslog-generator"
MIN_REDUCTION_PERCENT="${MIN_REDUCTION_PERCENT:-30}"
VARIANTS="${VARIANTS:-baseline symbols true debuginfo}"

prepare() {
    local name="$1"
    local source="${WORK}/${name}-source"
    mkdir -p "${source}"
    cp -R "${ROOT}/." "${source}/"
    rm -rf "${source}/target" "${source}/.git"
    python3 - "${source}/Cargo.toml" "${name}" <<'PY'
import pathlib
import sys
path = pathlib.Path(sys.argv[1])
variant = sys.argv[2]
source = path.read_text()
for line in ('panic = "abort"\n', 'strip = "symbols"\n', 'strip = true\n', 'strip = "debuginfo"\n', 'debug = 2\n', 'split-debuginfo = "packed"\n', 'opt-level = "s"\n', 'opt-level = "z"\n'):
    source = source.replace(line, "")
if variant != "baseline":
    strip = {"true": "true", "debuginfo": '"debuginfo"'}.get(variant, '"symbols"')
    opt = {"opt-s": 'opt-level = "s"\n', "opt-z": 'opt-level = "z"\n'}.get(variant, "")
    marker = 'codegen-units = 1\n'
    profile = f'panic = "abort"\nstrip = {strip}\ndebug = 2\nsplit-debuginfo = "packed"\n{opt}'
    source = source.replace(marker, marker + profile, 1)
path.write_text(source)
PY
}

build() {
    local name="$1"
    CARGO_TARGET_DIR="${WORK}/${name}-target" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo build --release --locked --manifest-path "${WORK}/${name}-source/Cargo.toml"
}

size() {
    wc -c < "${WORK}/$1-target/release/${BIN_NAME}" | tr -d ' '
}

symbolicate() {
    local name="$1"
    local target="${WORK}/${name}-target/release"
    local binary="${target}/${BIN_NAME}"
    case "$(uname -s)" in
        Darwin)
            local packed="${binary}.dSYM"
            local dwarf="${packed}/Contents/Resources/DWARF/${BIN_NAME}"
            test -f "${dwarf}"
            xcrun dwarfdump --uuid "${packed}" >/dev/null
            local address
            address="$(nm -an "${dwarf}" | awk '$2 ~ /^[Tt]$/ { print $1; exit }')"
            test -n "${address}"
            local result
            result="$(xcrun atos -o "${dwarf}" -arch "$(uname -m)" "0x${address}")"
            test -n "${result}"
            test "${result}" != "0x${address}"
            printf '%s\n' "${packed}"
            ;;
        Linux)
            local packed="${binary}.dwp"
            test -f "${packed}"
            local address
            address="$(readelf --debug-dump=info "${packed}" 2>/dev/null | awk '/DW_AT_low_pc/ && $NF !~ /^0x0+$/ { print $NF; exit }')"
            test -n "${address}"
            local result
            result="$(addr2line -f -C -e "${binary}" --dwp="${packed}" "${address}")"
            test -n "${result}"
            test "${result}" != $'??\n??:0'
            test "${result}" != $'??\n??:?'
            printf '%s\n' "${packed}"
            ;;
        *)
            echo "unsupported platform: $(uname -s)" >&2
            exit 2
            ;;
    esac
}

for variant in ${VARIANTS}; do
    prepare "${variant}"
    build "${variant}"
done

baseline_size="$(size baseline)"
printf 'baseline_bytes=%s\n' "${baseline_size}"
status=0
for variant in ${VARIANTS}; do
    test "${variant}" = baseline && continue
    variant_size="$(size "${variant}")"
    artifact="$(symbolicate "${variant}")"
    if ! python3 - "${variant}" "${baseline_size}" "${variant_size}" "${artifact}" "${MIN_REDUCTION_PERCENT}" <<'PY'
import sys
name, baseline, current, artifact, minimum = sys.argv[1:]
baseline, current, minimum = int(baseline), int(current), float(minimum)
reduction = (baseline - current) / baseline * 100
print(f"{name}_bytes={current}")
print(f"{name}_reduction_percent={reduction:.2f}")
print(f"{name}_debug_artifact={artifact}")
print(f"{name}_symbolication=pass")
if reduction < minimum:
    print(f"{name}_reduction_gate=fail")
    raise SystemExit(1)
print(f"{name}_reduction_gate=pass")
PY
    then
        status=1
    fi
done
exit "${status}"
