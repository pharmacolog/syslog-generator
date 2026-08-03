#!/usr/bin/env python3
import json
import sys
from pathlib import Path


def main(argv):
    if len(argv) < 1:
        print("usage: reset_schema.py <path-to-whitepaper-results.json>", file=sys.stderr)
        return 2

    path = Path(argv[0])
    if not path.exists():
        print(f"ERROR: {path} not found", file=sys.stderr)
        return 2

    data = json.loads(path.read_text())
    data["status"] = "schema_only"
    data["runs"] = []
    data["generated_at"] = "2026-07-25T00:00:00Z"
    if "host" in data:
        data["host"]["cpu_count"] = 12
        data["host"]["memory_bytes"] = None
    for tool in data.get("tool_versions", {}).values():
        tool["available"] = False
        tool["path"] = None
        tool["version"] = None

    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    print(f"Reset {path} to schema_only state.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
