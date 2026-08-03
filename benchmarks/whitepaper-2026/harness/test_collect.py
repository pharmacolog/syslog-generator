#!/usr/bin/env python3
import importlib.util
import json
import sys
import tempfile
from pathlib import Path
from unittest.mock import patch

SCRIPT_DIR = Path(__file__).resolve().parent
COLLECT_PY = SCRIPT_DIR.parent / "scripts" / "04_collect.py"
WP_RESULTS = SCRIPT_DIR.parent.parent.parent / "perf" / "whitepaper-results.json"

_spec = importlib.util.spec_from_file_location("collect_mod", COLLECT_PY)
collect = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(collect)


class TestCollect:
    def __init__(self):
        self.passes = 0
        self.fails = 0

    def assert_eq(self, got, expected, msg):
        if got == expected:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg} (got {got!r}, expected {expected!r})")

    def assert_true(self, cond, msg):
        if cond:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg}")

    def assert_false(self, cond, msg):
        if not cond:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg}")

    def test_invariant_completed_with_measurements(self):
        runs = [
            {
                "workload_id": "udp_100rps_256b",
                "tool": "syslog_generator",
                "run_idx": 1,
                "status": "completed",
                "measurements": {"achieved_msg_per_sec": 100.0},
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_true(ok, "legal completed+measurements passes")
        self.assert_eq(msg, "", "no error message")

    def test_invariant_completed_no_measurements_fails(self):
        runs = [
            {
                "workload_id": "udp_100rps_256b",
                "tool": "syslog_generator",
                "run_idx": 1,
                "status": "completed",
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_false(ok, "completed without measurements FAILS invariant")
        self.assert_true("completed" in msg and "measurements" in msg,
                        "error message mentions status and measurements")

    def test_invariant_skipped_with_measurements_fails(self):
        runs = [
            {
                "workload_id": "tls_5krps_1kb",
                "tool": "loggen",
                "run_idx": 1,
                "status": "skipped",
                "measurements": {"achieved_msg_per_sec": 5000.0},
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_false(ok, "skipped with measurements FAILS invariant")
        self.assert_true("skipped" in msg and "measurements" in msg,
                        "error message mentions skipped and measurements")

    def test_invariant_skipped_no_measurements(self):
        runs = [
            {
                "workload_id": "tls_5krps_1kb",
                "tool": "loggen",
                "run_idx": 1,
                "status": "skipped",
                "skip_reason": "loggen not installed",
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_true(ok, "skipped without measurements passes")

    def test_invariant_n_a_with_measurements_fails(self):
        runs = [
            {
                "workload_id": "udp_100rps_256b",
                "tool": "flog",
                "run_idx": 1,
                "status": "n_a",
                "measurements": {"achieved_msg_per_sec": 100.0},
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_false(ok, "n/a with measurements FAILS invariant")
        self.assert_true("n/a" in msg and "measurements" in msg,
                        "error message mentions n/a and measurements")

    def test_invariant_n_a_no_measurements(self):
        runs = [
            {
                "workload_id": "udp_100rps_256b",
                "tool": "flog",
                "run_idx": 1,
                "status": "n_a",
                "na_reason": "flog has no native network output",
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_true(ok, "n/a without measurements passes")

    def test_invariant_dry_run_no_measurements(self):
        runs = [
            {
                "workload_id": "udp_100rps_256b",
                "tool": "syslog_generator",
                "run_idx": 1,
                "status": "dry_run",
            }
        ]
        ok, msg = collect.invariant_no_fabricated(runs)
        self.assert_true(ok, "dry_run without measurements passes")

    def test_aggregate_status_empty(self):
        self.assert_eq(collect.compute_aggregate_status([]), "schema_only",
                       "empty runs -> schema_only")

    def test_aggregate_status_all_skipped(self):
        runs = [
            {"status": "skipped", "tool": "loggen"},
            {"status": "skipped", "tool": "flog"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "dry_run",
                       "all skipped -> dry_run")

    def test_aggregate_status_all_dry_run(self):
        runs = [
            {"status": "dry_run", "tool": "syslog_generator"},
            {"status": "dry_run", "tool": "loggen"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "dry_run",
                       "all dry_run -> dry_run")

    def test_aggregate_status_partial(self):
        runs = [
            {"status": "completed", "tool": "syslog_generator"},
            {"status": "skipped", "tool": "loggen"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "partial",
                       "completed + skipped -> partial (not complete)")

    def test_aggregate_status_complete(self):
        runs = [
            {"status": "completed"},
            {"status": "completed"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "complete",
                       "all completed -> complete")

    def test_aggregate_status_with_na(self):
        runs = [
            {"status": "completed"},
            {"status": "n_a"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "partial",
                       "completed + n_a -> partial")

    def test_aggregate_status_failed(self):
        runs = [
            {"status": "completed"},
            {"status": "failed"},
        ]
        self.assert_eq(collect.compute_aggregate_status(runs), "partial",
                       "completed + failed -> partial")

    def test_load_cell_meta_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake_results = Path(tmp)
            with patch.object(collect, "RESULTS_DIR", fake_results):
                meta = collect.load_cell_meta("udp_100rps_256b", 1, "loggen")
                self.assert_eq(meta["status"], "skipped",
                               "missing meta returns skipped")
                self.assert_eq(meta["workload_id"], "udp_100rps_256b",
                               "workload_id preserved")
                self.assert_eq(meta["tool"], "loggen", "tool preserved")
                self.assert_true("skip_reason" in meta,
                                "skip_reason provided")

    def test_load_cell_meta_present(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake_results = Path(tmp)
            cell_dir = fake_results / "udp_100rps_256b" / "run_1"
            cell_dir.mkdir(parents=True)
            meta_in = {
                "workload_id": "udp_100rps_256b",
                "tool": "syslog_generator",
                "run_idx": 1,
                "status": "completed",
                "measurements": {"achieved_msg_per_sec": 99.5},
            }
            (cell_dir / "syslog_generator.meta.json").write_text(json.dumps(meta_in))
            with patch.object(collect, "RESULTS_DIR", fake_results):
                meta = collect.load_cell_meta("udp_100rps_256b", 1, "syslog_generator")
                self.assert_eq(meta["status"], "completed", "status preserved")
                self.assert_true("measurements" in meta, "measurements preserved")

    def test_wp_results_template(self):
        if not WP_RESULTS.exists():
            self.fails += 1
            print(f"FAIL: WP_RESULTS not found: {WP_RESULTS}")
            return
        data = json.loads(WP_RESULTS.read_text())
        for field in ("schema_version", "issue", "status", "tool_versions",
                      "workloads", "runs", "constraints"):
            self.assert_true(field in data, f"template has {field!r}")
        self.assert_eq(data["schema_version"], "1.0.0", "schema_version is 1.0.0")
        self.assert_eq(data["issue"], 106, "issue is 106")
        self.assert_true(
            data["status"] in ("schema_only", "dry_run", "partial", "complete"),
            f"status is a valid bucket (got: {data['status']!r})",
        )
        self.assert_eq(data["constraints"]["no_fabricated_measurements"], True,
                       "no_fabricated_measurements is True")

        self.assert_eq(data["compared_tools"],
                       ["syslog_generator", "loggen", "flog", "tcpkali"],
                       "compared_tools = 4 (no kcat)")

        for i, r in enumerate(data["runs"]):
            if r.get("status") == "completed":
                self.assert_true("measurements" in r and r["measurements"],
                                f"runs[{i}].completed has measurements")
            if r.get("status") in ("skipped", "n_a"):
                self.assert_true("measurements" not in r or not r["measurements"],
                                f"runs[{i}].{r.get('status')} has no measurements")

    def test_template_workloads_match_issue(self):
        if not WP_RESULTS.exists():
            self.fails += 1
            return
        data = json.loads(WP_RESULTS.read_text())
        workloads = {w["id"] for w in data["workloads"]}
        expected = {"udp_100rps_256b", "tcp_10krps_1kb",
                    "tls_5krps_1kb", "kafka_50krps_256b"}
        self.assert_eq(workloads, expected, "workload IDs match Issue #106 spec")

    def test_template_tool_versions_initial(self):
        if not WP_RESULTS.exists():
            self.fails += 1
            return
        data = json.loads(WP_RESULTS.read_text())
        tv = data["tool_versions"]
        for tool in ("syslog_generator", "loggen", "flog", "tcpkali"):
            self.assert_true(tool in tv, f"tool_versions has {tool!r}")
            self.assert_true("available" in tv[tool],
                            f"{tool}.available field exists")
            self.assert_true("path" in tv[tool],
                            f"{tool}.path field exists")
            self.assert_true("version" in tv[tool],
                            f"{tool}.version field exists")
            if tv[tool]["available"]:
                self.assert_true(tv[tool]["path"],
                                f"{tool}.path set when available")
            else:
                self.assert_eq(tv[tool]["path"], None,
                              f"{tool}.path is None when not available")
        self.assert_true("kcat" not in tv, "kcat is NOT in tool_versions (out of scope)")


def main():
    t = TestCollect()
    methods = [
        t.test_invariant_completed_with_measurements,
        t.test_invariant_completed_no_measurements_fails,
        t.test_invariant_skipped_with_measurements_fails,
        t.test_invariant_skipped_no_measurements,
        t.test_invariant_n_a_with_measurements_fails,
        t.test_invariant_n_a_no_measurements,
        t.test_invariant_dry_run_no_measurements,
        t.test_aggregate_status_empty,
        t.test_aggregate_status_all_skipped,
        t.test_aggregate_status_all_dry_run,
        t.test_aggregate_status_partial,
        t.test_aggregate_status_complete,
        t.test_aggregate_status_with_na,
        t.test_aggregate_status_failed,
        t.test_load_cell_meta_missing,
        t.test_load_cell_meta_present,
        t.test_wp_results_template,
        t.test_template_workloads_match_issue,
        t.test_template_tool_versions_initial,
    ]
    for m in methods:
        try:
            print(f"\n--- {m.__name__} ---")
            m()
        except Exception as e:
            t.fails += 1
            print(f"FAIL: {m.__name__} raised {e!r}")

    print(f"\n=== harness/test_collect.py summary ===")
    print(f"PASSES: {t.passes}")
    print(f"FAILS:  {t.fails}")
    return 0 if t.fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
