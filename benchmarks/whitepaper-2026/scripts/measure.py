#!/usr/bin/env python3
import json
import os
import sys


REC_OUT = os.environ["REC_OUT"]
META_OUT = os.environ["META_OUT"]
WORKLOAD_ID = os.environ["WORKLOAD_ID"]
RUN_IDX = int(os.environ["RUN_IDX"])
RUN_EXIT_CODE = int(os.environ["RUN_EXIT_CODE"])
RUN_DURATION_SECS = float(os.environ["RUN_DURATION_SECS"])
DURATION_SECS = float(os.environ["DURATION_SECS"])
RATE_TOLERANCE_FRACTION = float(os.environ["RATE_TOLERANCE_FRACTION"])
SIZE_TOLERANCE_FRACTION = float(os.environ["SIZE_TOLERANCE_FRACTION"])
TOOL_BIN = os.environ["TOOL_BIN"]
TOOL_NAME = os.environ["TOOL_NAME"]
TARGET_RATE = int(os.environ["TARGET_RATE"])
SPEC_SIZE = int(os.environ["SPEC_SIZE"])
META_ARGV = json.loads(os.environ["META_ARGV_STR"])
STARTED_AT = os.environ["STARTED_AT"]
FINISHED_AT = os.environ["FINISHED_AT"]
HARNESS_RATE_OVERRIDE = os.environ.get("HARNESS_RATE") or None

try:
    rec = json.load(open(REC_OUT))
except (FileNotFoundError, json.JSONDecodeError) as e:
    rec = {"bytes": 0, "wire_bytes": 0, "messages": 0,
           "duration_secs": 0.0,
           "framing": "unknown", "protocol": "unknown",
           "error": f"receiver output unreadable: {e}"}

body_bytes = rec.get("bytes", 0)
wire_bytes = rec.get("wire_bytes", 0)
messages = rec.get("messages", 0)
rec_dur = float(rec.get("duration_secs", 0.0))
rec_err = rec.get("error")
rec_framing = rec.get("framing", "unknown")
rec_proto = rec.get("protocol", "unknown")

window = max(0.001, rec_dur)

achieved = round(messages / window, 2) if messages else 0.0
rate_pct = round(100.0 * messages / window / TARGET_RATE, 1) if messages else 0.0
actual_bpm = round(body_bytes / max(1, messages), 2) if messages else 0.0
size_pct = round(100.0 * abs(actual_bpm - SPEC_SIZE) / SPEC_SIZE, 2) if messages else 0.0

status = "completed"
fail_reasons = []

if RUN_EXIT_CODE != 0:
    status = "failed"
    fail_reasons.append(f"runner_exit_code={RUN_EXIT_CODE}")
if rec_err is not None:
    status = "failed"
    fail_reasons.append(f"receiver_error={rec_err}")
if messages == 0:
    status = "failed"
    fail_reasons.append("zero_messages_received")

if messages > 0:
    low_pct = 100.0 * (1.0 - RATE_TOLERANCE_FRACTION)
    high_pct = 100.0 * (1.0 + RATE_TOLERANCE_FRACTION)
    if rate_pct < low_pct:
        status = "failed"
        fail_reasons.append(f"rate_below_{int(100*RATE_TOLERANCE_FRACTION)}%_tolerance={rate_pct}%")
    if rate_pct > high_pct:
        status = "failed"
        fail_reasons.append(f"rate_above_{int(100*RATE_TOLERANCE_FRACTION)}%_tolerance={rate_pct}%")
    if size_pct > 100.0 * SIZE_TOLERANCE_FRACTION:
        status = "failed"
        fail_reasons.append(f"size_outside_{int(100*SIZE_TOLERANCE_FRACTION)}%_tolerance={size_pct}%")

meta = {
    "workload_id": WORKLOAD_ID,
    "run_idx": RUN_IDX,
    "tool": TOOL_NAME,
    "mode": "real",
    "status": status,
    "available": True,
    "exit_code": RUN_EXIT_CODE,
    "argv": META_ARGV,
    "duration_secs": RUN_DURATION_SECS,
    "started_at": STARTED_AT,
    "finished_at": FINISHED_AT,
    "fairness": {
        "spec_target_rate": TARGET_RATE,
        "effective_target_rate": TARGET_RATE,
        "harness_rate_override": HARNESS_RATE_OVERRIDE,
        "rate_tolerance_fraction": RATE_TOLERANCE_FRACTION,
        "spec_body_size": SPEC_SIZE,
        "size_tolerance_fraction": SIZE_TOLERANCE_FRACTION,
    },
    "measurements": {
        "messages": messages,
        "bytes_received": body_bytes,
        "wire_bytes": wire_bytes,
        "window_secs": round(window, 3),
        "achieved_msg_per_sec": achieved,
        "target_msg_per_sec": TARGET_RATE,
        "rate_pct_of_target": rate_pct,
        "actual_bytes_per_msg": actual_bpm,
        "size_pct_deviation": size_pct,
        "receiver_error": rec_err,
        "receiver_framing": rec_framing,
        "receiver_protocol": rec_proto,
        "fail_reasons": fail_reasons,
    },
}
open(META_OUT, "w").write(json.dumps(meta, indent=2, ensure_ascii=False) + "\n")
print(f"wrote {META_OUT}: status={status}")
