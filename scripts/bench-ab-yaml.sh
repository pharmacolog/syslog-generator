#!/usr/bin/env bash
# A/B bench: hot-path v10.7.19 (pre serde_yaml_ng migration baseline) vs
# текущий HEAD (v10.7.20) (Issue #193, milestone v11.6).
#
# Использование:
#   scripts/bench-ab-yaml.sh [baseline-tag] [head-sha-or-tag]
#
# Defaults:
#   baseline-tag = v10.7.19  (последний релиз ПЕРЕД serde_yaml_ng migration, Issue #134 / PR #177)
#   head         = HEAD      (текущая работа)
#
# Процедура:
#   1. Build v10.7.19 release binary в isolated target dir.
#   2. Run cargo bench --bench hot_path -- --quick.
#   3. Сохранить estimates в perf/baselines/v10.7.19.json.
#   4. Repeat для текущего HEAD.
#   5. Сравнить через python (аналогично scripts/compare-baseline.sh, но без
#      генерации CURRENT_FILE — мы хотим сохранить оба как artifacts).
#
# ВАЖНО: builds кэшируются НЕ пересекаются (isolated target dir per baseline).
# Cold build hot_path bench занимает 2-5 минут.
#
# Exit codes:
#   0 — оба бенча прошли, comparison внутри threshold
#   1 — regression > 5% на hot_path
#   2 — usage / build / bench failure

set -euo pipefail

BASELINE_TAG="${1:-v10.7.19}"
HEAD_REF="${2:-HEAD}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK_DIR="$(mktemp -d -t sg-bench-ab.XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

BASELINES_DIR="${REPO_ROOT}/perf/baselines"
mkdir -p "${BASELINES_DIR}"

log() { echo "[bench-ab] $*" >&2; }

# 1. Клонируем в work dir чтобы изолировать builds.
log "cloning ${BASELINE_TAG} and HEAD into ${WORK_DIR}"
git clone --quiet --no-local "${REPO_ROOT}" "${WORK_DIR}/baseline"
(cd "${WORK_DIR}/baseline" && git checkout --quiet "${BASELINE_TAG}")
git clone --quiet --no-local "${REPO_ROOT}" "${WORK_DIR}/head"
(cd "${WORK_DIR}/head" && git checkout --quiet "${HEAD_REF}")

# 2. Build + bench для каждой копии.
run_bench() {
    local label="$1"
    local work_dir="$2"
    local target_dir="${work_dir}/target-bench-ab"
    local log_file="${WORK_DIR}/bench-${label}.log"

    log "[${label}] cargo bench --bench hot_path -- --quick (target=${target_dir})"
    if ! (
        cd "${work_dir}"
        CARGO_TARGET_DIR="${target_dir}" \
            cargo bench --locked --bench hot_path -- --quick 2>&1 | tee "${log_file}"
    ); then
        log "[${label}] bench FAILED; see ${log_file}"
        return 1
    fi
}

if ! run_bench "${BASELINE_TAG}" "${WORK_DIR}/baseline"; then
    log "baseline build/bench failed"
    exit 2
fi
if ! run_bench "${HEAD_REF}"     "${WORK_DIR}/head";     then
    log "head build/bench failed"
    exit 2
fi

# 3. Парсим Criterion estimates для каждого.
parse_estimates() {
    local label="$1"
    local target_dir="$2"
    python3 - "${label}" "${target_dir}" "${BASELINES_DIR}" <<'PYEOF'
import json, os, sys
label, target_dir, out_dir = sys.argv[1:4]
estimates = []
root = os.path.join(target_dir, "criterion")
if not os.path.isdir(root):
    print(f"criterion dir missing: {root}", file=sys.stderr)
    sys.exit(1)

# Criterion создаёт два варианта структуры:
#   case A (top-level bench): <root>/<group>/new/estimates.json
#     пример: target/criterion/faker_ipv4/new/estimates.json
#   case B (grouped bench):  <root>/<group>/<sub>/new/estimates.json
#     пример: target/criterion/hot_path/rfc5424_with_faker/new/estimates.json
for group_dir in sorted(os.listdir(root)):
    gpath = os.path.join(root, group_dir)
    if not os.path.isdir(gpath):
        continue
    # Сначала пробуем case A (прямой child).
    direct_est = os.path.join(gpath, "new", "estimates.json")
    if os.path.isfile(direct_est):
        try:
            data = json.load(open(direct_est))
            ns = data.get("mean", {}).get("point_estimate")
            ci = data.get("mean", {}).get("confidence_interval", {})
            if ns is not None:
                estimates.append({
                    "group": group_dir,
                    "label": group_dir,
                    "time_ns_median": ns,
                    "time_ns_lower": ci.get("lower_bound"),
                    "time_ns_upper": ci.get("upper_bound"),
                })
        except Exception:
            pass
        continue
    # Иначе пробуем case B (двухуровневый).
    for sub in sorted(os.listdir(gpath)):
        spath = os.path.join(gpath, sub)
        if not os.path.isdir(spath):
            continue
        estimates_path = os.path.join(spath, "new", "estimates.json")
        if not os.path.isfile(estimates_path):
            continue
        try:
            data = json.load(open(estimates_path))
        except Exception:
            continue
        ns = data.get("mean", {}).get("point_estimate")
        ci = data.get("mean", {}).get("confidence_interval", {})
        if ns is None:
            continue
        estimates.append({
            "group": group_dir,
            "label": f"{group_dir}/{sub}",
            "time_ns_median": ns,
            "time_ns_lower": ci.get("lower_bound"),
            "time_ns_upper": ci.get("upper_bound"),
        })

out_file = os.path.join(out_dir, f"{label}.json")
with open(out_file, "w") as f:
    json.dump({
        "label": label,
        "estimate_count": len(estimates),
        "estimates": estimates,
    }, f, indent=2)
print(f"saved {len(estimates)} estimates -> {out_file}")
PYEOF
}

log "parsing estimates"
parse_estimates "${BASELINE_TAG}" "${WORK_DIR}/baseline/target-bench-ab"
parse_estimates "${HEAD_REF}"     "${WORK_DIR}/head/target-bench-ab"

# 4. Сравнение.
log "comparing ${BASELINE_TAG} vs ${HEAD_REF}"
python3 - "${BASELINES_DIR}/${BASELINE_TAG}.json" "${BASELINES_DIR}/${HEAD_REF}.json" <<'PYEOF'
import json, sys
HOT_T = 5.0
base_file, head_file = sys.argv[1], sys.argv[2]
base = json.load(open(base_file))
head = json.load(open(head_file))
base_e = {e["label"]: e for e in base["estimates"]}
head_e = {e["label"]: e for e in head["estimates"]}
print(f"\n=== A/B comparison ===")
print(f"Baseline ({base['label']}): {base['estimate_count']} estimates")
print(f"Head     ({head['label']}): {head['estimate_count']} estimates")
print(f"\nHot-path deltas (threshold +/-{HOT_T}%):")
regressions = []
for label, b in sorted(base_e.items()):
    if label not in head_e:
        print(f"  MISSING in head: {label}")
        continue
    h = head_e[label]
    if b["time_ns_median"] == 0:
        continue
    delta = (h["time_ns_median"] - b["time_ns_median"]) / b["time_ns_median"] * 100.0
    flag = ""
    if delta > HOT_T:
        flag = " REGRESS"
        regressions.append((label, delta))
    elif delta < -HOT_T:
        flag = " IMPROVE"
    print(f"  {label}: {b['time_ns_median']:.0f}ns -> {h['time_ns_median']:.0f}ns ({delta:+.1f}%){flag}")

if regressions:
    print(f"\nFAIL: {len(regressions)} regressions > {HOT_T}%")
    sys.exit(1)
print("\nPASS: no hot_path regressions > threshold")
PYEOF
