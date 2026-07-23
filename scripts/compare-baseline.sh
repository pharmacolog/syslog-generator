#!/usr/bin/env bash
# PR-A0 (v10.8.0): сравнение текущего прогона benches с сохранённым baseline.
#
# Использование:
#   scripts/compare-baseline.sh <baseline-sha>
#
# Exit codes:
#   0 — в пределах threshold
#   1 — регрессия превысила threshold
#   2 — usage error / missing files
#
# Thresholds (default):
#   HOT_PATH_THRESHOLD=5 (%)
#   RUNTIME_THRESHOLD=10 (%)
#   TRANSPORT_THRESHOLD=15 (%)
#
# Thresholds могут быть переопределены через env vars.

set -euo pipefail

HOT_PATH_THRESHOLD="${HOT_PATH_THRESHOLD:-5}"
RUNTIME_THRESHOLD="${RUNTIME_THRESHOLD:-10}"
TRANSPORT_THRESHOLD="${TRANSPORT_THRESHOLD:-15}"

BASELINE_SHA="${1:-}"
if [[ -z "${BASELINE_SHA}" ]]; then
    echo "usage: $0 <baseline-sha>" >&2
    exit 2
fi

BASELINE_FILE="perf/baselines/${BASELINE_SHA}.json"
if [[ ! -f "${BASELINE_FILE}" ]]; then
    echo "baseline file not found: ${BASELINE_FILE}" >&2
    exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 required for baseline comparison" >&2
    exit 2
fi

# Запускаем benches и собираем estimates в новый временный baseline.
CURRENT_SHA="compare-$(date +%s)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

scripts/perf-baseline.sh quick "${CURRENT_SHA}" > "${TMP_DIR}/run.log" 2>&1 || true
CURRENT_FILE="perf/baselines/${CURRENT_SHA}.json"
if [[ ! -f "${CURRENT_FILE}" ]]; then
    echo "failed to produce current baseline; see ${TMP_DIR}/run.log" >&2
    exit 1
fi

# Сравниваем через python.
python3 - "${BASELINE_FILE}" "${CURRENT_FILE}" \
    "${HOT_PATH_THRESHOLD}" "${RUNTIME_THRESHOLD}" "${TRANSPORT_THRESHOLD}" <<'PYEOF'
import json, sys

base_file, curr_file, hot_t, run_t, trans_t = sys.argv[1:6]
hot_t, run_t, trans_t = float(hot_t), float(run_t), float(trans_t)

with open(base_file) as f:
    base = json.load(f)
with open(curr_file) as f:
    curr = json.load(f)

base_e = {e["label"]: e for e in base["estimates"]}
curr_e = {e["label"]: e for e in curr["estimates"]}

# Группируем benchmarks по категории.
def category(label):
    if label.startswith("hot_path/"):
        return ("hot_path", hot_t)
    if label.startswith("runtime/"):
        return ("runtime", run_t)
    if label.startswith("transport_matrix_"):
        return ("transport", trans_t)
    if label.startswith("format_matrix/"):
        return ("format", run_t)
    if label.startswith("dispatch_matrix/"):
        return ("dispatch", run_t)
    return ("other", run_t)

regressions = []
improvements = []
for label, b in base_e.items():
    if label not in curr_e:
        continue
    c = curr_e[label]
    if b["time_ns_median"] == 0:
        continue
    delta_pct = (c["time_ns_median"] - b["time_ns_median"]) / b["time_ns_median"] * 100.0
    cat, threshold = category(label)
    if delta_pct > threshold:
        regressions.append((label, cat, delta_pct, threshold, b["time_ns_median"], c["time_ns_median"]))
    elif delta_pct < -threshold:
        improvements.append((label, cat, delta_pct, threshold, b["time_ns_median"], c["time_ns_median"]))

print(f"\n=== Baseline comparison ===")
print(f"Baseline: {base_file}")
print(f"Current:  {curr_file}")
print(f"Thresholds: hot_path={hot_t}%, runtime/format/dispatch={run_t}%, transport={trans_t}%")
print(f"\nRegressions ({len(regressions)}):")
for label, cat, delta, th, b_ns, c_ns in regressions:
    print(f"  REGRESS [{cat}] {label}: {b_ns:.0f}ns → {c_ns:.0f}ns (+{delta:.1f}%, threshold +{th}%)")
print(f"\nImprovements ({len(improvements)}):")
for label, cat, delta, th, b_ns, c_ns in improvements[:20]:
    print(f"  IMPROVE [{cat}] {label}: {b_ns:.0f}ns → {c_ns:.0f}ns ({delta:.1f}%)")
if len(improvements) > 20:
    print(f"  ... and {len(improvements) - 20} more")

if regressions:
    print(f"\nFAIL: {len(regressions)} regression(s) exceed threshold")
    sys.exit(1)
print("\nPASS: no regressions exceed threshold")
sys.exit(0)
PYEOF

RESULT=$?
# Очищаем временный baseline.
rm -f "${CURRENT_FILE}"

exit ${RESULT}
