#!/usr/bin/env bash
# Issue #228: local perf-regression check helper для maintainer'а.
#
# Запускает median-of-N-runs perf regression локально и сравнивает с baseline.
# Если local regression > threshold — exit 1 (fix before push).
#
# Это **honest baseline check** — median-of-3 local vs baseline median-of-3
# remote. CI variance на GitHub Actions runners ±15-20% (Issue #218), но
# local variance должна быть <5% на warm cache.
#
# Usage:
#   bash scripts/perf-local-check.sh [baseline-file]
#
# Если baseline-file не указан, используется latest `perf/baselines/*.json`
# отсортированный по modification time (newest first).
#
# Environment:
#   BENCH_RUNS       Number of runs (default: 3, matches CI).
#   THRESHOLD_HOT_PATH    hot_path threshold % (default: 10).
#   THRESHOLD_FORMAT      format threshold % (default: 15).
#   THRESHOLD_TRANSPORT   transport threshold % (default: 15).
#   THRESHOLD_ALLOC       allocations threshold % (default: 100).
#   SKIP_WARMUP     Skip warm-up step if "1" (faster but more variance).
#
# Exit codes:
#   0 — no regressions exceed threshold (PASS, push OK)
#   1 — regressions detected (FAIL, fix before push)
#   2 — invalid usage / missing files
#
# Example:
#   bash scripts/perf-local-check.sh perf/baselines/943a08c...json
#
# Refs: #228, #164, #214, #211, #218

set -euo pipefail

# Defaults
N_RUNS="${BENCH_RUNS:-3}"
THRESHOLD_HOT_PATH="${THRESHOLD_HOT_PATH:-10}"
THRESHOLD_FORMAT="${THRESHOLD_FORMAT:-15}"
THRESHOLD_TRANSPORT="${THRESHOLD_TRANSPORT:-15}"
THRESHOLD_ALLOC="${THRESHOLD_ALLOC:-100}"

# Determine baseline file
if [[ $# -ge 1 ]]; then
    BASELINE_FILE="$1"
else
    BASELINE_FILE="$(ls -t perf/baselines/*.json 2>/dev/null | grep -v '\.gitignore\|README' | head -1 || true)"
fi

if [[ -z "${BASELINE_FILE}" || ! -f "${BASELINE_FILE}" ]]; then
    echo "ERROR: baseline file not found" >&2
    echo "Usage: $0 <baseline-file>" >&2
    echo "Or: ensure perf/baselines/*.json exists" >&2
    exit 2
fi

# Determine bench targets from baseline
# Issue #211: hot_path + allocations. Format/transport/others могут быть
# добавлены через BENCH_TARGETS env var.
BENCH_TARGETS="${BENCH_TARGETS:-hot_path allocations}"

# Output file for current estimates
CURRENT_FILE="$(mktemp -t perf-current-XXXXXX.json)"
trap 'rm -f "${CURRENT_FILE}"' EXIT

echo "=== Local perf-regression check ==="
echo "Baseline: ${BASELINE_FILE}"
echo "Bench targets: ${BENCH_TARGETS}"
echo "Runs: ${N_RUNS} (median)"
echo "Thresholds: hot_path=${THRESHOLD_HOT_PATH}%, format=${THRESHOLD_FORMAT}%, transport=${THRESHOLD_TRANSPORT}%, allocations=${THRESHOLD_ALLOC}%"
echo ""

# Warm-up step (cold cache mitigation, Issue #164)
if [[ "${SKIP_WARMUP:-0}" != "1" ]]; then
    echo "=== Warm-up (cold cache mitigation) ==="
    # Use first bench target for warm-up
    FIRST_BENCH="${BENCH_TARGETS%% *}"
    cargo bench --locked --bench "${FIRST_BENCH}" -- --warm-up-time 1 --measurement-time 1 >/dev/null 2>&1 || true
    echo "Warm-up complete"
    echo ""
fi

# Generate current estimates (median-of-N)
echo "=== Generating current estimates (median of ${N_RUNS} runs) ==="
BENCH_RUNS="${N_RUNS}" scripts/perf-regression-collect.sh "${CURRENT_FILE}" ${BENCH_TARGETS} 2>&1 | tail -5
echo ""

# Compare against baseline (similar to perf-regression.yml Compare step)
echo "=== Compare against baseline ==="
BASELINE_FILE="${BASELINE_FILE}" \
CURRENT_FILE="${CURRENT_FILE}" \
THRESHOLD_HOT_PATH="${THRESHOLD_HOT_PATH}" \
THRESHOLD_FORMAT="${THRESHOLD_FORMAT}" \
THRESHOLD_TRANSPORT="${THRESHOLD_TRANSPORT}" \
THRESHOLD_ALLOC="${THRESHOLD_ALLOC}" \
python3 - <<'PYEOF'
import json
import os
import sys

base_path = os.environ["BASELINE_FILE"]
curr_path = os.environ["CURRENT_FILE"]
hot_thr = float(os.environ.get("THRESHOLD_HOT_PATH", "10"))
fmt_thr = float(os.environ.get("THRESHOLD_FORMAT", "15"))
tra_thr = float(os.environ.get("THRESHOLD_TRANSPORT", "15"))
alloc_thr = float(os.environ.get("THRESHOLD_ALLOC", "100"))

with open(base_path) as f:
    base = json.load(f)
with open(curr_path) as f:
    curr = json.load(f)

# Schema tolerance: baseline может быть {sha, mode, timestamp, estimates: [...]}
# или {timestamp, baseline_sha, n_runs, estimates: [...]}. Оба работают.
base_e = {e["label"]: e for e in base["estimates"]}
curr_e = {e["label"]: e for e in curr["estimates"]}


def category(label):
    group = label.split("/", 1)[0]
    if group == "hot_path":
        return ("hot_path", hot_thr)
    if group.startswith("format"):
        return ("format", fmt_thr)
    if group.startswith("transport"):
        return ("transport", tra_thr)
    if group.startswith("alloc"):
        return ("allocations", alloc_thr)
    return ("other", hot_thr)


regressions = []
improvements = []
for label, b in base_e.items():
    if label not in curr_e:
        continue
    c = curr_e[label]
    if b["time_ns_median"] == 0:
        continue
    delta = (c["time_ns_median"] - b["time_ns_median"]) / b["time_ns_median"] * 100.0
    cat, thr = category(label)
    if delta > thr:
        regressions.append((label, cat, delta, thr, b["time_ns_median"], c["time_ns_median"]))
    elif delta < -thr:
        improvements.append((label, cat, delta, thr, b["time_ns_median"], c["time_ns_median"]))

print(f"\n=== Local perf-regression gate ===")
print(f"Baseline: {base_path}")
print(f"Current:  {curr_path}")
print(f"Thresholds: hot_path={hot_thr}%, format={fmt_thr}%, transport={tra_thr}%, allocations={alloc_thr}%")
print(f"Regressions ({len(regressions)}):")
for label, cat, delta, thr, b_ns, c_ns in regressions:
    print(f"  REGRESS [{cat}] {label}: {b_ns:.0f}ns → {c_ns:.0f}ns (+{delta:.1f}%, threshold +{thr}%)")
print(f"Improvements ({len(improvements)}):")
for label, cat, delta, thr, b_ns, c_ns in improvements[:20]:
    print(f"  IMPROVE [{cat}] {label}: {b_ns:.0f}ns → {c_ns:.0f}ns ({delta:.1f}%)")
if regressions:
    print(f"\nFAIL: {len(regressions)} regression(s) exceed threshold", flush=True)
    print(f"Fix before push. Use `cargo build --release` + local debugging.", flush=True)
    sys.exit(1)
print(f"\nPASS: no regressions exceed threshold", flush=True)
sys.exit(0)
PYEOF