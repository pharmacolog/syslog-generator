#!/usr/bin/env python3
"""Parse Criterion estimates.json → flat JSON line for perf-regression."""
import json
import sys

bench = sys.argv[1]
path = sys.argv[2]
parts = path.split("/")
try:
    group = parts[2]
    sub = parts[3]
except IndexError:
    sys.exit(0)
label = group + "/" + sub
with open(path) as f:
    data = json.load(f)
median = data.get("median") or {}
ns = median.get("point_estimate")
if ns is None:
    sys.exit(0)
print(json.dumps({
    "bench": bench,
    "group": group,
    "label": label,
    "time_ns_median": int(ns),
    "time_ns_lower": int(median.get("lower_bound") or 0),
    "time_ns_upper": int(median.get("upper_bound") or 0),
}))
