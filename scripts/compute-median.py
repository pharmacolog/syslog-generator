#!/usr/bin/env python3
"""Compute median estimates across multiple runs of perf-regression-collect.sh.

Usage:
    python3 scripts/compute-median.py <output.json> <run1.jsonl> [run2.jsonl ...]

Each input JSONL has one JSON object per line, each with fields:
    {"bench": "hot_path", "group": "hot_path", "label": "hot_path/foo",
     "time_ns_median": 1684, "time_ns_lower": 1650, "time_ns_upper": 1720}

Output JSON:
    {"timestamp": "...", "baseline_sha": "...", "n_runs": N,
     "estimates": [{"bench": "...", "group": "...", "label": "...",
                    "time_ns_median": MEDIAN, "time_ns_lower": MEDIAN_LOWER,
                    "time_ns_upper": MEDIAN_UPPER, "n_samples": N}]}

Median — арифметическая медиана N значений (для нечётных N — middle value,
для чётных N — average of two middle values).

time_ns_median — median of time_ns_median across runs.
time_ns_lower — median of time_ns_lower (lower bound) across runs.
time_ns_upper — median of time_ns_upper (upper bound) across runs.

Exit codes:
    0 — success
    1 — invalid input (no runs, no estimates)
"""
import json
import statistics
import sys
import os
import datetime


def median_or_none(values):
    """Return median of values, or None if empty."""
    if not values:
        return None
    return statistics.median(values)


def main():
    if len(sys.argv) < 3:
        print("usage: compute-median.py <output.json> <run1.jsonl> [run2.jsonl ...]", file=sys.stderr)
        sys.exit(2)

    output_path = sys.argv[1]
    run_files = sys.argv[2:]

    if not run_files:
        print("ERROR: no run files provided", file=sys.stderr)
        sys.exit(1)

    # Load all runs: {label: [estimates across runs]}
    by_label = {}  # label -> list of estimate dicts

    for run_file in run_files:
        if not os.path.exists(run_file):
            print(f"WARNING: run file not found: {run_file}", file=sys.stderr)
            continue
        with open(run_file) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    est = json.loads(line)
                except json.JSONDecodeError as e:
                    print(f"WARNING: malformed line in {run_file}: {e}", file=sys.stderr)
                    continue
                label = est.get("label")
                if not label:
                    continue
                by_label.setdefault(label, []).append(est)

    if not by_label:
        print("ERROR: no estimates collected across runs", file=sys.stderr)
        sys.exit(1)

    # Compute median per label
    estimates = []
    for label, runs in sorted(by_label.items()):
        medians = [r.get("time_ns_median", 0) for r in runs]
        lowers = [r.get("time_ns_lower", 0) for r in runs]
        uppers = [r.get("time_ns_upper", 0) for r in runs]

        median_val = median_or_none(medians)
        if median_val is None:
            continue

        # Use bench/group from first run (consistent across runs)
        first = runs[0]
        estimates.append({
            "bench": first.get("bench"),
            "group": first.get("group"),
            "label": label,
            "time_ns_median": int(median_val),
            "time_ns_lower": int(median_or_none(lowers) or 0),
            "time_ns_upper": int(median_or_none(uppers) or 0),
            "n_samples": len(runs),
        })

    if not estimates:
        print("ERROR: no estimates after median aggregation", file=sys.stderr)
        sys.exit(1)

    output = {
        "timestamp": datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ"),
        "baseline_sha": os.environ.get("BASELINE_SHA") or "unknown",
        "n_runs": len(run_files),
        "estimates": estimates,
    }

    with open(output_path, "w") as f:
        json.dump(output, f, indent=2)
        f.write("\n")

    print(f"Generated: {output_path} ({len(estimates)} estimates, median of {len(run_files)} runs)")


if __name__ == "__main__":
    main()