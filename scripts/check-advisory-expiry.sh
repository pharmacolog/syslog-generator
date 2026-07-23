#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from datetime import date
from pathlib import Path
import re
import sys

text = Path("deny.toml").read_text(encoding="utf-8")
match = re.search(r"(?ms)^\s*ignore\s*=\s*\[(.*?)^\s*\]", text)
if match is None:
    print("No advisory ignore entries found")
    raise SystemExit(0)

block = match.group(1)
advisories = sorted(set(re.findall(r"RUSTSEC-\d{4}-\d{4}", block)))
failures = []
for advisory in advisories:
    entry = re.search(
        rf'\{{\s*id\s*=\s*"{re.escape(advisory)}"\s*,\s*reason\s*=\s*"([^"]+)"\s*\}}',
        block,
    )
    if entry is None:
        failures.append(f"{advisory}: structured reason with expiry is required")
        continue
    expiry = re.search(r"expires:\s*(\d{4}-\d{2}-\d{2})", entry.group(1))
    if expiry is None:
        failures.append(f"{advisory}: missing expires: YYYY-MM-DD marker")
        continue
    try:
        expiry_date = date.fromisoformat(expiry.group(1))
    except ValueError:
        failures.append(f"{advisory}: invalid expiry date {expiry.group(1)}")
        continue
    if expiry_date <= date.today():
        failures.append(f"{advisory}: expiry {expiry_date.isoformat()} is not in the future")
        continue
    print(f"{advisory}: expires {expiry_date.isoformat()}")

if failures:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)
    raise SystemExit(1)
PY
