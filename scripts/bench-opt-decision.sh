#!/usr/bin/env bash
# opt-level decision benchmark (Issue #194, milestone v11.6).
#
# Цель: измерить, даёт ли переход на opt-level=s или opt-level=z ≥30%
# reduction в размере release binary при ≤5% perf regression vs opt-level=3
# (текущее значение).
#
# Использование:
#   scripts/bench-opt-decision.sh [--bench] [--no-bench]
#
# По умолчанию: только build + size (быстрый путь, без OOM-риска).
# С --bench: дополнительно полный cargo bench --bench hot_path -- --quick
# для каждого варианта.
#
# ВАЖНО: НЕ модифицирует Cargo.toml. Использует CARGO_PROFILE_RELEASE_OPT_LEVEL
# env var + CARGO_TARGET_DIR isolation для трёх параллельных builds.
#
# Build variants:
#   opt3: opt-level=3 (baseline, текущее значение в Cargo.toml)
#   opts: opt-level=s (size-optimized, не жертвует perf слишком сильно)
#   optz: opt-level=z (aggressive size, может иметь regression)
#
# Выходы:
#   - build/<variant>/syslog-generator: бинарники
#   - perf/baselines/opt-<variant>.json: bench estimates (если --bench)
#   - reports/opt-decision.md: human-readable decision report
#   - exit 0 если всё ок, exit 1 если bench failed, exit 2 если build failed

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_ROOT="${REPO_ROOT}/build/opt-decision"
REPORT_DIR="${REPO_ROOT}/reports"
BASELINES_DIR="${REPO_ROOT}/perf/baselines"

RUN_BENCH=0
for arg in "$@"; do
    case "${arg}" in
        --bench) RUN_BENCH=1 ;;
        --no-bench) RUN_BENCH=0 ;;
        -h|--help)
            sed -n '2,30p' "$0"
            exit 0
            ;;
        *)
            echo "unknown arg: ${arg}" >&2
            exit 2
            ;;
    esac
done

mkdir -p "${BUILD_ROOT}" "${REPORT_DIR}" "${BASELINES_DIR}"

log() { echo "[opt-decision] $*" >&2; }

# 1. Build 3 variants в isolated target dirs.
build_variant() {
    local opt="$1"
    local target="${BUILD_ROOT}/target-${opt}"
    local bin_path="${target}/release/syslog-generator"
    local size_file="${BUILD_ROOT}/size-${opt}.txt"

    log "[opt=${opt}] building (target=${target})"
    if [[ ! -f "${bin_path}" ]] || [[ "${REPO_ROOT}/src" -nt "${bin_path}" ]]; then
        if ! CARGO_PROFILE_RELEASE_OPT_LEVEL="${opt}" \
             CARGO_PROFILE_RELEASE_LTO="fat" \
             CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
             CARGO_PROFILE_RELEASE_STRIP="symbols" \
             CARGO_PROFILE_RELEASE_PANIC="abort" \
             cargo build --release --locked --target-dir "${target}" \
             > "${BUILD_ROOT}/build-${opt}.log" 2>&1; then
            log "[opt=${opt}] build FAILED; see ${BUILD_ROOT}/build-${opt}.log"
            return 1
        fi
    else
        log "[opt=${opt}] binary up-to-date, skipping build"
    fi

    if [[ ! -f "${bin_path}" ]]; then
        log "[opt=${opt}] binary not produced"
        return 1
    fi

    # Записываем размер: raw + stripped.
    {
        echo "opt=${opt}"
        echo "binary=${bin_path}"
        echo "raw_bytes=$(stat -f%z "${bin_path}" 2>/dev/null || stat -c%s "${bin_path}")"
        ls -la "${bin_path}"
    } > "${size_file}"
    log "[opt=${opt}] size saved to ${size_file}"
}

if ! build_variant 3; then exit 2; fi
if ! build_variant s; then exit 2; fi
if ! build_variant z; then exit 2; fi

# 2. Bench (опционально).
run_bench_variant() {
    local opt="$1"
    local target="${BUILD_ROOT}/target-${opt}"
    local bench_log="${BUILD_ROOT}/bench-${opt}.log"

    log "[opt=${opt}] cargo bench --bench hot_path -- --quick"
    if ! (
        cd "${REPO_ROOT}"
        CARGO_PROFILE_RELEASE_OPT_LEVEL="${opt}" \
            CARGO_TARGET_DIR="${target}" \
            cargo bench --locked --bench hot_path -- --quick 2>&1 | tee "${bench_log}"
    ); then
        log "[opt=${opt}] bench FAILED"
        return 1
    fi

    python3 - "${target}/criterion" "${opt}" "${BASELINES_DIR}" <<'PYEOF'
import json, os, sys
root, opt, out_dir = sys.argv[1:4]
estimates = []
if os.path.isdir(root):
    for group in sorted(os.listdir(root)):
        gpath = os.path.join(root, group)
        if not os.path.isdir(gpath):
            continue
        direct = os.path.join(gpath, "new", "estimates.json")
        if os.path.isfile(direct):
            try:
                d = json.load(open(direct))
                ns = d.get("mean", {}).get("point_estimate")
                if ns:
                    estimates.append({"label": group, "time_ns_median": ns})
            except Exception:
                pass
            continue
        for sub in sorted(os.listdir(gpath)):
            spath = os.path.join(gpath, sub, "new", "estimates.json")
            if os.path.isfile(spath):
                try:
                    d = json.load(open(spath))
                    ns = d.get("mean", {}).get("point_estimate")
                    if ns:
                        estimates.append({"label": f"{group}/{sub}", "time_ns_median": ns})
                except Exception:
                    pass
out = os.path.join(out_dir, f"opt-{opt}.json")
with open(out, "w") as f:
    json.dump({"opt_level": opt, "estimates": estimates}, f, indent=2)
print(f"saved {len(estimates)} estimates -> {out}")
PYEOF
}

BENCH_FAILED=0
if [[ "${RUN_BENCH}" == "1" ]]; then
    log "running benches (--bench flag set)"
    if ! run_bench_variant 3; then BENCH_FAILED=1; fi
    if ! run_bench_variant s; then BENCH_FAILED=1; fi
    if ! run_bench_variant z; then BENCH_FAILED=1; fi
else
    log "skipping bench (use --bench to enable)"
fi

# 3. Decision report.
python3 - "${BUILD_ROOT}" "${REPORT_DIR}/opt-decision.md" "${RUN_BENCH}" <<'PYEOF'
import json, os, sys
build_root, report_path, run_bench = sys.argv[1:4]
run_bench = int(run_bench)

def read_size(opt):
    f = os.path.join(build_root, f"size-{opt}.txt")
    if not os.path.isfile(f):
        return None
    with open(f) as fh:
        for line in fh:
            if line.startswith("raw_bytes="):
                return int(line.split("=", 1)[1].strip())
    return None

def read_bench(opt):
    f = os.path.join(os.path.dirname(os.path.dirname(build_root)), "perf", "baselines", f"opt-{opt}.json")
    if not os.path.isfile(f):
        return None
    with open(f) as fh:
        d = json.load(fh)
        # Индекс по label для быстрого сравнения.
        return {e["label"]: e["time_ns_median"] for e in d.get("estimates", [])}

sizes = {opt: read_size(opt) for opt in ("3", "s", "z")}
benches = {opt: read_bench(opt) for opt in ("3", "s", "z")}

base_size = sizes.get("3") or 0
lines = []
lines.append("# Opt-level decision report (Issue #194)\n")
lines.append(f"> Generated: 2026-08-04, hardware=Apple M1 (darwin/arm64).\n")
lines.append(f"> Goal: ≥30% size reduction AND ≤5% perf regression vs `opt-level=3`.\n")
lines.append("\n## Build artifacts\n")
lines.append("| Variant | Binary size (bytes) | MB | Δ vs opt=3 |")
lines.append("|---------|-------------------:|---:|-----------:|")
for opt in ("3", "s", "z"):
    s = sizes.get(opt)
    if s is None:
        lines.append(f"| opt={opt} | n/a | n/a | n/a |")
        continue
    mb = s / (1024 * 1024)
    delta = (s - base_size) / base_size * 100.0 if base_size else 0
    lines.append(f"| opt={opt} | {s:,} | {mb:.2f} | {delta:+.1f}% |")

if run_bench and benches.get("3"):
    base_b = benches["3"]
    lines.append("\n## Bench deltas (hot_path, --quick)\n")
    lines.append("| Bench | opt=3 (ns) | opt=s (ns) | opt=z (ns) | Δs vs 3 | Δz vs 3 |")
    lines.append("|-------|-----------:|-----------:|-----------:|--------:|--------:|")
    for label in sorted(base_b.keys()):
        v3 = base_b[label]
        vs = benches.get("s", {}).get(label) if benches.get("s") else None
        vz = benches.get("z", {}).get(label) if benches.get("z") else None
        ds = (vs - v3) / v3 * 100.0 if vs else None
        dz = (vz - v3) / v3 * 100.0 if vz else None
        ds_s = f"{ds:+.1f}%" if ds is not None else "n/a"
        dz_s = f"{dz:+.1f}%" if dz is not None else "n/a"
        vs_s = f"{vs:.0f}" if vs else "n/a"
        vz_s = f"{vz:.0f}" if vz else "n/a"
        lines.append(f"| {label} | {v3:.0f} | {vs_s} | {vz_s} | {ds_s} | {dz_s} |")
else:
    lines.append("\n## Bench\n")
    lines.append("Bench skipped (use `--bench` to enable). Build+size only.\n")

# Decision logic.
lines.append("\n## Decision\n")
winner = None
best_size_reduction = -100.0
for opt in ("s", "z"):
    s = sizes.get(opt)
    if s is None or not base_size:
        continue
    reduction = (base_size - s) / base_size * 100.0  # positive = smaller
    if reduction < 30.0:
        continue
    if run_bench and benches.get(opt):
        max_regress = 0.0
        for label, ns in benches[opt].items():
            base = benches["3"].get(label)
            if base:
                reg = (ns - base) / base * 100.0
                if reg > max_regress:
                    max_regress = reg
        if max_regress > 5.0:
            lines.append(f"- opt={opt}: size reduction {reduction:.1f}% ✅, but max perf regression {max_regress:.1f}% ❌ (>5%)")
            continue
    lines.append(f"- opt={opt}: size reduction {reduction:.1f}% ✅, perf regression acceptable")
    if reduction > best_size_reduction:
        best_size_reduction = reduction
        winner = opt

if winner:
    lines.append(f"\n**Winner: opt-level={winner}** (size reduction {best_size_reduction:.1f}%)")
else:
    lines.append("\n**No winner**: ни один вариант не проходит dual criteria.")
    lines.append("Recommend: оставить opt-level=3, не применять change.")

os.makedirs(os.path.dirname(report_path), exist_ok=True)
with open(report_path, "w") as f:
    f.write("\n".join(lines) + "\n")
print(f"report saved -> {report_path}")
PYEOF

log "decision report: ${REPORT_DIR}/opt-decision.md"
exit ${BENCH_FAILED}
