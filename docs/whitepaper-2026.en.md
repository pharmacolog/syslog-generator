# Syslog load generators benchmark: 4 tools comparison

> **English draft** for dev.to / personal blog.
> **Issues:** [#106](https://github.com/pharmacolog/syslog-generator/issues/106) (harness),
> [#196](https://github.com/pharmacolog/syslog-generator/issues/196) (real VM run),
> [#199](https://github.com/pharmacolog/syslog-generator/issues/199) (publication).
> **Milestone:** v11.6
> **Status:** Harness + methodology ready (`schema_only`). Numerical results
> **NOT included** — see `perf/whitepaper-results.json::status` (currently
> `schema_only`). Real run planned per
> [`benchmarks/whitepaper-2026/docs/benchmark-runbook.md`](../../benchmarks/whitepaper-2026/docs/benchmark-runbook.md).
> **EXTERNAL PENDING:** Habr publication, dev.to publication, Telegram
> announcements, Twitter/X thread, Reddit/HN posts, GitHub stars via UTM
> — **none of these are executed in this PR**. See [Issue #199](https://github.com/pharmacolog/syslog-generator/issues/199).

## TL;DR

This is the **English draft** of the Whitepaper 2026. The harness and
methodology are complete; numerical results are **deliberately absent**
because the actual recorded run has not been performed yet (see
`perf/whitepaper-results.json::status = "schema_only"`).

The harness compares **4 tools** (Issue #106 spec): `syslog-generator`,
`loggen` (syslog-ng), `flog`, `tcpkali`. **kcat is NOT in the compared-tools
set** — there is no working Kafka consumer in v1, so all Kafka cells
are marked `N/A`.

What is in this document:

1. **Why** an honest benchmark (not marketing claims).
2. **What** we measure (4 workloads × 4 tools, 16 cells).
3. **How** we measure (fairness protocol, hardware lock, RM-friendly tooling).
4. **Methodology** ([`benchmarks/whitepaper-2026/METHODOLOGY.md`](../../benchmarks/whitepaper-2026/METHODOLOGY.md)).
5. **When to use** which tool (decision tree).
6. **What this draft does NOT contain** (honest limitations).
7. **How to reproduce** (3 commands).
8. **Call to action** (stars, citations, feedback).

## When to use which tool

This is the most practically useful section. **Before** numerical results,
it is grounded in the verified capability matrix (Issue #106 §3.4), not
on speed comparisons.

### `syslog-generator` — for SIEM soak-testing

**Choose when:**

- You need **high sustained load** on a SIEM / log management pipeline
  (10k–100k+ msg/s) for hours or days.
- You need **native UDP / TCP / TLS** in a single binary with no extra
  dependencies.
- You want **predictable latency** (p50/p95/p99 via Prometheus exporter).
- You need **configuration presets** for protocols (RFC 5424, RFC 3164,
  CEF, LEEF, JSON).

**Don't choose when:**

- You need **HTTP-based load** (REST / gRPC). `syslog-generator` is
  single-protocol per workload.
- You need a **Kafka producer**. Kafka transport in v1 is marked `N/A`
  (no real consumer; see Issue #106 §external_pending).
- You need **multi-protocol mixed mode** in a single workload — each
  workload is one transport.

### `loggen` (syslog-ng) — for TCP `octet-counting` regression tests

**Choose when:**

- You need to verify **TCP octet-counting framing** against a real
  syslog-ng server (for example, regression test after a syslog-ng upgrade).
- You already have syslog-ng on the host and don't want to install anything.
- **Works out of the box** on Ubuntu 22.04 (`apt install syslog-ng-core`).

**Don't choose when:**

- You need **TCP or TLS**: loggen is **UDP-only** since 4.6.x (see METHODOLOGY §3.4).
- You are load-testing a **production SIEM** — loggen is single-threaded
  by default; syslog-generator parallelizes.
- You need **TLS** — loggen does not support it (see capability matrix).

### `flog` — for JSON / HTTP logs

**Choose when:**

- You need **fake structured log** in JSON / Common Log Format / Apache
  access log (e.g., for ELK / Loki testing).
- You need a **fixed format** and **deterministic randomization** (flog
  supports `--seed`).
- You don't need to send over the network — stdout/file output is enough.

**Don't choose when:**

- **Any** network workload — flog v0.4.3 has **no native network output**.
  All 4 network cells = `N/A`. We **do not** pipe through `nc` — that
  would violate the fairness protocol.
- You need **custom templates** — flog fixes record size per log type.

### `tcpkali` — for capacity planning

**Choose when:**

- You need **TCP/TLS load test at connection bandwidth level** (multi-connection,
  connection rate, message rate).
- You need **detailed latency stats** (tcpkali collects its own histogram).
- Capacity planning for **brokers / pub-sub** (RabbitMQ, NATS, Kafka wire
  protocol bypass).

**Don't choose when:**

- You need **UDP** — tcpkali is TCP/TLS only.
- You need **syslog framing validation** — tcpkali sends bytes without
  validating syslog parser. Useful for capacity, **not** for correctness.

### Decision tree

```text
Need UDP?
  ├─ Yes → syslog-generator (>10k msg/s) or loggen (≤10k msg/s, syslog-ng bundled)
  └─ No  → TCP?
              ├─ Yes  → TCP framing — syslog-generator or tcpkali
              └─ No  → TLS?
                          ├─ Yes → syslog-generator (single binary) or tcpkali (bandwidth)
                          └─ No  → Kafka?
                                       ├─ Yes → v1: none of 4 tools (N/A);
                                       │         wait for Issue #106 Kafka consumer
                                       └─ No  → HTTP/REST?  → NOT from 4 tools,
                                                            see flog (file/stdout only)
```

## Placeholder tables (filled after Issue #196)

> **Where to get the numbers:** [`perf/whitepaper-results.json`](../../perf/whitepaper-results.json).
> After Issue #196 run, copy them into these tables. See also
> [`benchmarks/whitepaper-2026/results/EXPECTED.md`](../../benchmarks/whitepaper-2026/results/EXPECTED.md)
> for the schema.

### Throughput (msg/s) — measured cells only

| Workload | syslog_generator | loggen | flog | tcpkali |
|---|---|---|---|---|
| `udp_100rps_256b`  | _pending_ | _pending_ | n/a | n/a |
| `tcp_10krps_1kb`   | _pending_ | n/a | n/a | _pending_ |
| `tls_5krps_1kb`    | _pending_ | n/a | n/a | _pending_ |
| `kafka_50krps_256b`| n/a | n/a | n/a | n/a |

| Tool | Throughput | p50 | p95 | p99 |
|---|---|---|---|---|
| `syslog-generator` | _pending_ | _pending_ | _pending_ | _pending_ |
| `loggen`           | _pending_ | n/a | n/a | n/a |
| `flog`             | n/a (network) | n/a | n/a | n/a |
| `tcpkali`          | _pending_ | _pending_ | _pending_ | _pending_ |

### Latency (p50/p95/p99) — `syslog-generator` only

(METHODOLOGY §5.3: external tools latency = `null` in v1. tcpkali collects
its own histogram, but it does not enter the common artifact — out of
scope v1.)

| Workload | p50 (ms) | p95 (ms) | p99 (ms) |
|---|---|---|---|
| `udp_100rps_256b`  | _pending_ | _pending_ | _pending_ |
| `tcp_10krps_1kb`   | _pending_ | _pending_ | _pending_ |
| `tls_5krps_1kb`    | _pending_ | _pending_ | _pending_ |

### Resource usage (after `MEASURE_RESOURCES=1` — opt-in)

| Workload | Tool | CPU% (avg) | RSS (MiB) | Wall time (s) |
|---|---|---|---|---|
| _pending_ | _pending_ | _pending_ | _pending_ | _pending_ |

> Resource usage is **opt-in** (`MEASURE_RESOURCES=1`); `make all` does
> not collect it by default. Issue #196 acceptance — main metrics
> (throughput, latency), resources are bonus.

## Honest limitations

> This section is **intentionally detailed**. Without these caveats the
> whitepaper stops being honest, and the dev.to / Habr publication risks
> becoming marketing claims. After Issue #196 run, this section is **not
> shortened** — numbers are added, limitations remain.

### What is **NOT** in v1

1. **Kafka cell = `N/A` for all 4 tools.** Not because they don't
   support Kafka, but because the v1 harness receiver is a TCP liveness
   probe, with no real consumer. To make the Kafka cell `completed`,
   a librdkafka-based consumer is needed (separate issue, see
   `perf/whitepaper-results.json::external_pending`).
2. **flog network = `N/A` for all 4 cells.** flog v0.4.3 supports
   stdout/stderr/file only. We **do not** pipe through `nc` — that
   would violate the fairness protocol (see METHODOLOGY §3.4).
3. **Latency p50/p95/p99 — `syslog-generator` only** (Prometheus
   exporter). For loggen / flog / tcpkali, latency = `null`. tcpkali
   collects its own histogram, but it does not enter the common
   `runs[]` — out of scope v1.
4. **Cross-host network is not tested.** All 4 workloads go to
   `127.0.0.1` (loopback). Real network benchmarking is a separate issue.
5. **Linux x86_64 only.** c5.2xlarge (Intel Xeon Platinum 8275CL).
   ARM (Graviton) and macOS are out of scope v1 (see METHODOLOGY §2).

### What **MAY** change

- **Issue #196 run is planned but not executed.** Numbers in this draft
  are pending. If the run shows a significant deviation from expectations
  (e.g., syslog-generator does not reach 10k msg/s over TCP), we will
  revise claims in the "When to use which tool" section — either remove
  or rephrase.
- **Synthetic variance.** CPU frequency, IRQ, OS jitter give ±3–5%
  spread. Best-of-3 (see EXPECTED.md §4.2) smooths but does not
  eliminate. **Numeric reproducibility is not guaranteed** (claimed
  in METHODOLOGY §7).
- **Tool versions are pinned.** `syslog_generator=v11.6.0`,
  `loggen=4.6.4`, `flog=v0.4.3`, `tcpkali=2.10.0`. Newer versions may
  show different numbers; the harness deliberately does NOT support
  "latest" — reproducibility wins over freshness.

### What will **NEVER** be

- No fabricated measurements. The `no_fabricated_measurements` invariant
  is enforced in `benchmarks/whitepaper-2026/harness/test_collect.py`.
- No "X is faster than Y by 47%". Numbers will be concrete per cell,
  not relative.
- No vendor-benchmark adaptations. All numbers come from our own
  harness.
- No "fully containerized" claims. Dockerfiles with guessed versions
  have been removed (see `benchmarks/whitepaper-2026/docker/README.md`).

## 1. Why this benchmark

When you search "syslog load generator", you get a long list of tools.
Each tool's README claims "high throughput" or "low latency" — but
**claims are not comparable**: different machines, different networks,
different message sizes, different receivers, different durations. You
cannot multiply them.

The goal of this benchmark is to put all **4 tools** on the **same
machine**, on the **same network**, with **identical message sizes**
and **identical duration** and **identical receiver**. After the run,
the numbers are public and reproducible.

## 2. The 4 compared tools and their real capabilities

(Per confirmed research, not assumptions.)

| Tool | Source | UDP | TCP | TLS | Kafka |
|---|---|---|---|---|---|
| `syslog-generator` | this repo | yes | yes | yes | N/A (no real consumer) |
| `loggen` | **syslog-ng** (not rsyslog) | yes | N/A (UDP-only) | N/A (UDP-only) | N/A |
| `flog` | mingrammer v0.4.3 | N/A | N/A | N/A | N/A |
| `tcpkali` | machinezone | N/A | yes | yes | N/A |

## 3. Workloads (Issue #106 spec)

| ID | Transport | Rate | Payload | Duration |
|---|---|---|---|---|
| `udp_100rps_256b`  | UDP  | 100 msg/s  | 256 B | 30 s |
| `tcp_10krps_1kb`   | TCP  | 10 000 msg/s | 1 KB | 30 s |
| `tls_5krps_1kb`    | TLS  | 5 000 msg/s  | 1 KB | 30 s |
| `kafka_50krps_256b`| Kafka | 50 000 msg/s | 256 B | 30 s |

16 cells total (4 workloads × 4 compared tools). 6 will be `completed`,
10 `n/a` (see capability matrix).

## 4. Methodology

> **Full methodology:** [`benchmarks/whitepaper-2026/METHODOLOGY.md`](../../benchmarks/whitepaper-2026/METHODOLOGY.md).
> **Reproduction runbook:** [`benchmarks/whitepaper-2026/docs/benchmark-runbook.md`](../../benchmarks/whitepaper-2026/docs/benchmark-runbook.md).

Highlights (per Issue #106 spec):

- **Same hardware** (single machine, documented).
- **Same network** (loopback 127.0.0.1).
- **Same receiver** (Python byte-counter, no syslog-parser-side bias).
- **Same duration** (30s default, configurable via `DURATION_SECS`).
- **Same target rate** (Issue #106 spec; achieved rate must be 95–105% of target).
- **Same message size** (256B / 1KB; ±5% tolerance).
- **Measurement window** = sender runtime (no warmup, no time subtraction).
- **No parallelism** (sequential to avoid CPU steal / cache contention).
- **No fabricated measurements** (invariant enforced by `harness/test_collect.py`).

## 5. CLI forms (verified per research)

- `loggen -i -D -s SIZE -r RATE -I SECONDS HOST PORT` (syslog-ng loggen, UDP only)
- `flog -f syslog -n COUNT -o stdout` (no network support → N/A for all network workloads)
- `tcpkali -T SECS --message-rate RATE -c 1 -f MESSAGE_FILE HOST:PORT`;
  `--ssl --cert --key` for TLS; port as positional (NO -P flag)
- `syslog-generator -p CONFIG --duration N` (native UDP/TCP/TLS; Kafka pending consumer)

## 6. How to reproduce

```bash
git clone https://github.com/pharmacolog/syslog-generator
cd syslog-generator
git checkout <commit-sha>  # tag v11.6.0 after Issue #191

# Dry-run (always succeeds, no tools required):
make -C benchmarks/whitepaper-2026 all

# Real run (requires installed tools):
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1

# Full VM runbook (Issue #196, c5.2xlarge, 3-4 hours):
# see benchmarks/whitepaper-2026/docs/benchmark-runbook.md
```

## 7. EXTERNAL PENDING — outside this issue

- ❌ **dev.to publication** (EN, this draft) — `EXTERNAL PENDING` (Issue #199).
- ❌ **Habr publication** (RU) — `EXTERNAL PENDING` (Issue #199).
- ❌ **Telegram announcements** (DevOps Moscow, SRE Russia, Rust Russia) — `EXTERNAL PENDING`.
- ❌ **Twitter/X thread** — `EXTERNAL PENDING`.
- ❌ **Reddit r/rust, r/sysadmin** — `EXTERNAL PENDING`.
- ❌ **Hacker News** — `EXTERNAL PENDING`.
- ❌ **Citation count ≥ 5 / 3 months** — `EXTERNAL PENDING` (Issue #200).
- ❌ **GitHub stars ≥ 50 attributed via UTM** — `EXTERNAL PENDING` (Issue #200).
- ❌ **Containerized Dockerfiles** for all 4 tools — `EXTERNAL PENDING`.

## 8. License & attribution

- Code: Apache-2.0 (same as `syslog-generator`).
- Benchmark harness: `benchmarks/whitepaper-2026/`.
- Methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.
- Runbook: `benchmarks/whitepaper-2026/docs/benchmark-runbook.md`.
- Raw data: `perf/whitepaper-results.json`.
- Tools compared: `loggen` (GPLv2, syslog-ng project), `flog` (MIT,
  mingrammer), `tcpkali` (Apache-2.0, machinezone), `syslog-generator`
  (Apache-2.0, pharmacolog).

## 9. Call to action

- If this tool is useful — **star** on GitHub:
  [https://github.com/pharmacolog/syslog-generator](https://github.com/pharmacolog/syslog-generator/?utm_campaign=whitepaper-2026)
  (UTM-tagged for citation tracking).
- Cite this whitepaper in your own articles / RFC proposals / benchmarks.
  Backlinks tracking — [`docs/whitepaper-2026-tracking.md`](whitepaper-2026-tracking.md).
- Habr permalink: **TBD** (Issue #199, after publication).
- dev.to permalink: **TBD** (Issue #199, after publication).
- Feedback: open an issue or PR on [`docs/whitepaper-2026.en.md`](whitepaper-2026.en.md).
