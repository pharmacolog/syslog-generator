#!/usr/bin/env python3
import json
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
WHITEPAPER_DIR = SCRIPT_DIR.parent
REPO_ROOT = WHITEPAPER_DIR.parent.parent
WP_RESULTS = REPO_ROOT / "perf" / "whitepaper-results.json"
RESULTS_DIR = WHITEPAPER_DIR / "results"

WORKLOAD_IDS = (
    "udp_100rps_256b",
    "tcp_10krps_1kb",
    "tls_5krps_1kb",
    "kafka_50krps_256b",
)
COMPARED_TOOLS = ("syslog_generator", "loggen", "flog", "tcpkali")


def load_template():
    if not WP_RESULTS.exists():
        print(f"ERROR: template not found: {WP_RESULTS}", file=sys.stderr)
        sys.exit(2)
    return json.loads(WP_RESULTS.read_text())


def load_check_tools_output():
    script = SCRIPT_DIR / "01_check_tools.sh"
    try:
        result = subprocess.run(
            ["bash", str(script)], capture_output=True, text=True, timeout=30,
        )
    except (subprocess.SubprocessError, FileNotFoundError, OSError) as e:
        print(f"WARN: check_tools.sh failed: {e}", file=sys.stderr)
        return {}
    if result.returncode != 0:
        return {}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as e:
        print(f"WARN: check_tools.sh output not JSON: {e}", file=sys.stderr)
        return {}


def load_cell_meta(workload_id, run_idx, tool):
    path = RESULTS_DIR / workload_id / f"run_{run_idx}" / f"{tool}.meta.json"
    if not path.exists():
        return {
            "workload_id": workload_id,
            "run_idx": run_idx,
            "tool": tool,
            "status": "skipped",
            "available": False,
            "skip_reason": "no meta.json produced (runner never executed or did not write)",
            "exit_code": 0,
        }
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError as e:
        print(f"ERROR: invalid meta.json at {path}: {e}", file=sys.stderr)
        sys.exit(3)


def list_run_indices():
    if not RESULTS_DIR.exists():
        return [1]
    indices = set()
    for d in RESULTS_DIR.iterdir():
        rundir = d / "run_1"
        if rundir.exists():
            indices.add(1)
    for d in RESULTS_DIR.iterdir():
        if not d.is_dir():
            continue
        for r in d.iterdir():
            if not r.is_dir():
                continue
            n = r.name
            if n.startswith("run_"):
                try:
                    indices.add(int(n[4:]))
                except ValueError:
                    pass
    return sorted(indices) if indices else [1]


def invariant_no_fabricated(runs):
    for r in runs:
        status = r.get("status")
        meas = r.get("measurements")
        if status == "completed" and not meas:
            return False, f"completed without measurements: {r.get('workload_id')}/{r.get('tool')}"
        if status == "skipped" and meas:
            return False, f"skipped with measurements: {r.get('workload_id')}/{r.get('tool')}"
        if status == "n_a" and meas:
            return False, f"n/a with measurements: {r.get('workload_id')}/{r.get('tool')}"
    return True, ""


def compute_aggregate_status(runs):
    if not runs:
        return "schema_only"
    statuses = {r.get("status") for r in runs}
    n = len(runs)
    n_completed = sum(1 for r in runs if r.get("status") == "completed")
    n_other = sum(1 for r in runs if r.get("status") in ("skipped", "dry_run", "n_a"))
    if n_completed == n:
        return "complete"
    if n_other == n:
        return "dry_run"
    if n_completed > 0:
        return "partial"
    return "partial"


def main(argv):
    template = load_template()
    check_tools_output = load_check_tools_output()

    if "host" in check_tools_output:
        template["host"] = check_tools_output["host"]
    if "tool_versions" in check_tools_output:
        template["tool_versions"] = check_tools_output["tool_versions"]

    run_indices = list_run_indices()

    runs = []
    for w in WORKLOAD_IDS:
        for r in run_indices:
            for t in COMPARED_TOOLS:
                meta = load_cell_meta(w, r, t)
                if "status" not in meta:
                    if meta.get("mode") == "dry_run":
                        meta["status"] = "dry_run"
                    elif meta.get("mode") == "skipped":
                        meta["status"] = "skipped"
                    elif meta.get("mode") == "real":
                        meta["status"] = "completed" if meta.get("exit_code") == 0 else "failed"
                    else:
                        meta["status"] = "unknown"
                runs.append(meta)

    ok, msg = invariant_no_fabricated(runs)
    if not ok:
        print(f"ERROR: invariant violated: {msg}", file=sys.stderr)
        sys.exit(1)

    template["runs"] = runs
    template["status"] = compute_aggregate_status(runs)
    template["generated_at"] = (
        __import__("datetime").datetime.now(__import__("datetime").timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    )

    WP_RESULTS.write_text(json.dumps(template, indent=2, ensure_ascii=False) + "\n")
    print(f"Wrote {WP_RESULTS}: status={template['status']} runs={len(runs)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
