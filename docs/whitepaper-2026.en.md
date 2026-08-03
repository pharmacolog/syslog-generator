# Syslog load generators benchmark: 4 tools comparison

> **English draft** for dev.to / personal blog.
> **Issue:** [#106](https://github.com/pharmacolog/syslog-generator/issues/106)
> **Status:** Draft. Numerical results **NOT included** — see `perf/whitepaper-results.json::status` for the actual recorded state (currently `schema_only`).
> **EXTERNAL PENDING:** Habr publication, dev.to publication, Telegram announcements, Twitter/X thread, Reddit/HN posts, citation counts, GitHub stars via UTM — **none of these are executed in this PR**. They require post-benchmark marketing amplification as separate tasks.

## TL;DR

This is the **English draft** of the Whitepaper 2026. The harness and
methodology are complete; numerical results are **deliberately absent**
because the actual recorded run has not been performed yet (see
`perf/whitepaper-results.json::status = "schema_only"`).

The harness compares **4 tools** (Issue #106 spec): `syslog-generator`,
`loggen` (syslog-ng), `flog`, `tcpkali`. **kcat is NOT in the compared-tools
set** — there is no working Kafka consumer in v1, so all Kafka cells
are marked `N/A`.

What is delivered:

1. **Reproducible harness** (4 tools × 4 workloads).
2. **Fairness protocol** (same hardware, network, receiver, duration, message size).
3. **Schema/status artifact** (`perf/whitepaper-results.json`) — no fabricated measurements.

## 1. The 4 compared tools and their real capabilities

(Per confirmed research, not assumptions.)

| Tool | Source | UDP | TCP | TLS | Kafka |
|---|---|---|---|---|---|
| `syslog-generator` | this repo | yes | yes | yes | N/A (no real consumer) |
| `loggen` | **syslog-ng** (not rsyslog) | yes | N/A (UDP-only) | N/A (UDP-only) | N/A |
| `flog` | mingrammer v0.4.3 | N/A | N/A | N/A | N/A |
| `tcpkali` | machinezone | N/A | yes | yes | N/A |

## 2. Why this benchmark

When you search "syslog load generator", you get a long list of tools.
Each tool has a README that claims "high throughput" or "low latency" — but
the claims are **not comparable**. The goal of this benchmark is to put all
**4 tools** on the **same machine**, sending to the **same receiver**, with
**identical message sizes** and **identical duration**.

## 3. Workloads (Issue #106 spec)

| ID | Transport | Rate | Payload | Duration |
|---|---|---|---|---|
| `udp_100rps_256b`  | UDP  | 100 msg/s  | 256 B | 30 s |
| `tcp_10krps_1kb`   | TCP  | 10 000 msg/s | 1 KB | 30 s |
| `tls_5krps_1kb`    | TLS  | 5 000 msg/s  | 1 KB | 30 s |
| `kafka_50krps_256b`| Kafka | 50 000 msg/s | 256 B | 30 s |

16 cells total (4 workloads × 4 compared tools).

## 4. Capability matrix (verified)

- `loggen` (syslog-ng tests/loggen) is **UDP-only**. TCP/TLS/Kafka are N/A.
- `flog` v0.4.3 has **no native network output** (stdout/stderr/file only) and
  a **fixed record size per log type**. All network workloads are N/A.
- `tcpkali` is **TCP/TLS only**. UDP/Kafka are N/A.
- `syslog-generator` supports UDP, TCP, TLS natively. Kafka transport
  exists but is **N/A until a real consumer** is wired in (out of scope v1).

## 5. Methodology (highlights)

- **Same hardware** (single machine, documented).
- **Same network** (loopback 127.0.0.1).
- **Same receiver** (Python byte-counter, no syslog-parser-side bias).
- **Same duration** (30s default, configurable via `DURATION_SECS`).
- **Same target rate** (Issue #106 spec; achieved rate must be 95–105% of target).
- **Same message size** (256B / 1KB; ±5% tolerance).
- **Measurement window** = sender runtime (no warmup, no time subtraction).
- **No parallelism** (sequential to avoid CPU steal / cache contention).
- **No fabricated measurements** (invariant enforced by `harness/test_collect.py`).

Full methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.

## 6. CLI forms (verified per research)

- `loggen -i -D -s SIZE -r RATE -I SECONDS HOST PORT` (syslog-ng loggen, UDP only)
- `flog -f syslog -n COUNT -o stdout` (no network support → N/A for all network workloads)
- `tcpkali -T SECS --message-rate RATE -c 1 -f MESSAGE_FILE HOST:PORT`;
  `--ssl --cert --key` for TLS; port as positional (NO -P flag)
- `syslog-generator -p CONFIG --duration N` (native UDP/TCP/TLS; Kafka pending consumer)

## 7. How to reproduce

```bash
git clone https://github.com/pharmacolog/syslog-generator
cd syslog-generator
git checkout <commit-sha>

# Dry-run (always succeeds, no tools required):
make -C benchmarks/whitepaper-2026 all

# Real run (requires installed tools):
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1
```

## 8. EXTERNAL PENDING — outside this issue

The following items from Issue #106 are **explicitly out of scope** of
the harness PR and are **EXTERNAL PENDING** (will be tracked separately):

- ❌ **dev.to publication** (EN, this draft) — `EXTERNAL PENDING`.
- ❌ **Habr publication** (RU) — `EXTERNAL PENDING`.
- ❌ **Telegram announcements** (DevOps Moscow, SRE Russia, Rust Russia) — `EXTERNAL PENDING`.
- ❌ **Twitter/X thread** — `EXTERNAL PENDING`.
- ❌ **Reddit r/rust, r/sysadmin** — `EXTERNAL PENDING`.
- ❌ **Hacker News** — `EXTERNAL PENDING`.
- ❌ **Citation count ≥ 5 / 3 months** — `EXTERNAL PENDING`.
- ❌ **GitHub stars ≥ 50 attributed via UTM** — `EXTERNAL PENDING`.
- ❌ **Containerized Dockerfiles** for all 4 tools — `EXTERNAL PENDING`.

## 9. What this draft does NOT contain

- **No numerical throughput / latency numbers.** These will be filled in
  after a real run. The current schema is `status: "schema_only"`.
- **No marketing claims** like "X is faster than Y". We don't have the data.
- **No adaptation of vendor benchmarks.** All numbers come from our own
  harness, on a single well-documented machine.

## 10. License & attribution

- Code: Apache-2.0 (same as `syslog-generator`).
- Benchmark harness: `benchmarks/whitepaper-2026/`.
- Methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.
- Raw data: `perf/whitepaper-results.json`.
- Tools compared: `loggen` (GPLv2, syslog-ng project), `flog` (MIT,
  mingrammer), `tcpkali` (Apache-2.0, machinezone), `syslog-generator`
  (Apache-2.0, pharmacolog).
