#!/usr/bin/env python3
import json
import sys
from pathlib import Path

CONFIGS_DIR = Path(__file__).resolve().parent

ISSUE_106_SPEC = {
    "udp_100rps_256b":   {"transport": "udp",   "rate": 100,   "size": 256,  "framing": "datagram"},
    "tcp_10krps_1kb":    {"transport": "tcp",   "rate": 10000, "size": 1024, "framing": "octet-counting"},
    "tls_5krps_1kb":     {"transport": "tls",   "rate": 5000,  "size": 1024, "framing": "octet-counting"},
    "kafka_50krps_256b": {"transport": "kafka", "rate": 50000, "size": 256,  "framing": "kafka-protocol"},
}

REQUIRED_FIELDS = (
    "_workload_id",
    "_issue_spec",
    "_receiver",
    "targets",
    "phases",
)

SIZE_TOLERANCE = 2


def fail(msg, workload=""):
    prefix = f"[{workload}] " if workload else ""
    print(f"FAIL {prefix}{msg}", file=sys.stderr)
    sys.exit(1)


def validate_one(workload_id):
    spec = ISSUE_106_SPEC.get(workload_id)
    if spec is None:
        fail(f"unknown workload_id", workload_id)

    path = CONFIGS_DIR / f"workload_{workload_id}.json"
    if not path.exists():
        fail(f"missing config: {path}", workload_id)

    try:
        cfg = json.loads(path.read_text())
    except json.JSONDecodeError as e:
        fail(f"invalid JSON: {e}", workload_id)

    for field in REQUIRED_FIELDS:
        if field not in cfg:
            fail(f"missing required field: {field}", workload_id)

    if cfg["_workload_id"] != workload_id:
        fail(f"_workload_id mismatch", workload_id)

    is_spec = cfg["_issue_spec"]
    if is_spec.get("transport") != spec["transport"]:
        fail(f"_issue_spec.transport != {spec['transport']}", workload_id)
    if is_spec.get("target_msg_per_sec") != spec["rate"]:
        fail(f"_issue_spec.target_msg_per_sec != {spec['rate']}", workload_id)
    if is_spec.get("target_bytes_per_msg") != spec["size"]:
        fail(f"_issue_spec.target_bytes_per_msg != {spec['size']}", workload_id)
    if is_spec.get("framing") != spec["framing"]:
        fail(f"_issue_spec.framing != {spec['framing']}", workload_id)
    if is_spec.get("duration_secs", 0) <= 0:
        fail("_issue_spec.duration_secs must be > 0", workload_id)

    rcv = cfg["_receiver"]
    expected_proto = "kafka" if spec["transport"] == "kafka" else spec["transport"]
    if rcv.get("protocol") != expected_proto:
        fail(f"_receiver.protocol != {expected_proto}", workload_id)

    if not isinstance(cfg.get("targets"), list) or len(cfg["targets"]) == 0:
        fail("targets must be non-empty list", workload_id)
    if not isinstance(cfg.get("phases"), list) or len(cfg["phases"]) == 0:
        fail("phases must be non-empty list", workload_id)

    target = cfg["targets"][0]
    if spec["transport"] != "kafka":
        target_transport = target.get("transport", "")
        if target_transport != spec["transport"]:
            fail(f"target.transport={target_transport!r} != {spec['transport']!r}", workload_id)
        if spec["transport"] in ("tcp", "tls"):
            if target.get("framing") != "octet-counting":
                fail(f"target.framing must be 'octet-counting'", workload_id)
    else:
        if not target.get("kafka_topic"):
            fail("kafka target must have kafka_topic", workload_id)

    phase = cfg["phases"][0]
    if phase.get("messages_per_second") != spec["rate"]:
        fail(f"phase.messages_per_second != {spec['rate']}", workload_id)

    templates = phase.get("templates", [])
    if not templates:
        fail("phase.templates must be non-empty", workload_id)
    template = templates[0]
    header_overhead = is_spec.get("header_overhead_bytes", 0)
    expected_template_size = spec["size"] - header_overhead
    if abs(len(template) - expected_template_size) > SIZE_TOLERANCE:
        fail(f"template length={len(template)} != expected {expected_template_size}", workload_id)

    declared_template_size = is_spec.get("template_body_bytes")
    if declared_template_size is not None and declared_template_size != expected_template_size:
        fail(f"_issue_spec.template_body_bytes mismatch", workload_id)

    print(f"OK  {workload_id:18s}  transport={spec['transport']:5s}  "
          f"rate={spec['rate']:>6d}  size={spec['size']:>4d}B  "
          f"body={len(template):>4d}B  framing={spec['framing']}")


def main(argv):
    if "--all" in argv or not argv:
        workload_ids = list(ISSUE_106_SPEC.keys())
    else:
        workload_ids = argv

    print(f"Validating {len(workload_ids)} workload(s) against Issue #106 spec...")
    for wid in workload_ids:
        validate_one(wid)

    print(f"\nAll {len(workload_ids)} workload(s) OK.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
