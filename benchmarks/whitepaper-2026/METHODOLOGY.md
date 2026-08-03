# Methodology — Whitepaper 2026

> **Issue:** #106 (GTM-1, milestone v11.6)
> **Дата:** 2026-07-25
> **Версия:** 1.0.0

## 1. Цель

Опубликовать reproducible-воспроизводимый сравнительный benchmark
`syslog-generator vs loggen vs flog vs tcpkali` (4 инструмента из
Issue #106), который планируется использовать как marketing-материал
(Habr / dev.to) и technical reference.

Этот документ — methodology + reproducibility contract. Numerics
отсутствуют намеренно: ни одно число в `perf/whitepaper-results.json`
не должно появиться без живого прогона на определённой в §2 машине.

## 2. Hardware lock

Issue #106 указывает reference-машину: **8-core, 32GB RAM**.

Harness это не enforcing — он принимает любую машину, но требует
**явно задокументировать** в `perf/whitepaper-results.json::host`:
- `os` (`uname -s`)
- `arch` (`uname -m`)
- `kernel` (`uname -r`)
- `cpu_count` (`sysctl -n hw.ncpu` или `nproc`)
- `cpu_model` (`sysctl -n machdep.cpu.brand_string` на macOS, `/proc/cpuinfo` на Linux)
- `memory_bytes` (`sysctl -n hw.memsize` или `/proc/meminfo`)

## 3. Workloads (Issue #106 spec)

| ID | Transport | Rate | Payload (B) | Framing |
|---|---|---|---|---|
| `udp_100rps_256b`   | UDP  | 100   | 256 | datagram |
| `tcp_10krps_1kb`    | TCP  | 10000 | 1024 | octet-counting |
| `tls_5krps_1kb`     | TLS  | 5000  | 1024 | octet-counting |
| `kafka_50krps_256b` | Kafka | 50000 | 256 | kafka-protocol |

20 cells = 4 workloads × **4 compared tools** (syslog_generator, loggen,
flog, tcpkali). kcat is NOT in the compared-tools set.

### 3.1 Payload size

Целевой payload size — это **syslog message body** (MSG field of RFC 5424).
Harness reports `body_bytes / messages_received` and asserts the average
is within ±5% of the spec'd `target_bytes_per_msg`.

syslog-generator templates use a fixed-size literal padding. The template
length equals `target_bytes_per_msg - header_overhead_bytes` (51 bytes for
our config). The receiver counts the full body (not the on-wire overhead).

### 3.2 Per-tool CLI (verified)

| Tool | CLI | Notes |
|---|---|---|
| syslog-generator | `-p CONFIG --duration N` | native UDP/TCP/TLS; Kafka pending consumer |
| loggen (syslog-ng) | `loggen -i -D -s SIZE -r RATE -I SECONDS HOST PORT` | UDP only |
| flog | `flog -f syslog -n COUNT -o stdout` | NO native network output; N/A for all network |
| tcpkali | `tcpkali -T SECS --message-rate RATE -c 1 -f FILE HOST:PORT` | TCP/TLS only |

### 3.3 Throughput target

For cells that support a rate config, the harness sets all 4 tools
with the **same target rate** from the Issue #106 spec. The harness
measures what each tool actually delivers and compares to the target.

If a tool cannot achieve target rate, the cell is marked `failed` (not
`skipped`). This is essential for an honest benchmark.

### 3.4 Capability matrix (4 compared tools)

| | syslog_generator | loggen | flog | tcpkali |
|---|---|---|---|---|
| UDP 100 msg/s 256B | supported | supported | **N/A** (no network) | **N/A** (TCP/TLS only) |
| TCP 10k msg/s 1KB | supported | **N/A** (UDP only) | **N/A** | supported |
| TLS 5k msg/s 1KB | supported | **N/A** (UDP only) | **N/A** | supported |
| Kafka 50k msg/s 256B | **N/A** (no real consumer) | **N/A** | **N/A** | **N/A** |

N/A is not a failure — it's a documented capability gap.

## 4. Fairness protocol

### 4.1 Same hardware

All 4 cells run sequentially on one machine. **No parallelism** — otherwise
CPU steal / cache contention would distort the results.

### 4.2 Same network

All 4 workloads send to `127.0.0.1` (loopback). Cross-host is out of
scope for v1.

### 4.3 Same receiver

Single receiver — `benchmarks/whitepaper-2026/scripts/receiver.py` (Python
byte-counter). This is **not** a syslog-parser, to avoid parser-side bias.

For TLS, a single self-signed cert (`docker/gen-cert.sh`). Different CAs
would be different workloads, not considered.

### 4.4 Same duration

`DURATION_SECS` (default 30s) per cell. Sequential, not parallel.

### 4.5 Same message size

256B / 1KB per Issue #106 spec. ±5% tolerance.

### 4.6 Same target rate

All rate-limited cells configured with the spec'd target. Achieved rate
must be 95–105% of target (`RATE_TOLERANCE_FRACTION=0.05`).

### 4.7 Measurement window

`window = sender runtime / receiver duration` (the actual seconds the
receiver was listening). **No time subtraction, no warmup, no fake
duration tricks.** If `DURATION_SECS` is set, the receiver runs for that
many seconds, the workload runs for that many seconds, and the achieved
rate is `messages / window`.

`WARMUP_SECS` is **not used in v1**. The earlier 5s warmup subtraction
that produced the 329k msg/s bug has been removed entirely. If a future
revision needs warmup, it must be implemented as either:
- a separate discarded run, or
- a receiver-side message discard (with explicit `messages_post_warmup`
  field and adjusted `effective_window_secs`).

### 4.8 No warmup in v1

`WARMUP_SECS` is hard-coded to 0. Short tests may use `HARNESS_RATE` to
tune the rate to within the 95–105% gate. For example, target=100 with
`HARNESS_RATE=95` produces actual ~103 msg/s, well within the gate.

### 4.9 No parallelism

Cells run sequentially. No `&`, no `xargs -P`, no backgrounded batches.

### 4.10 No fabricated measurements

`harness/test_collect.py` enforces: completed status requires
non-empty `measurements`; n_a / skipped status requires no
`measurements` field. Invariant runs at every harness test invocation.

## 5. Metrics

### 5.1 Throughput (msg/s)

`messages_received / window_secs` where `window_secs` is the receiver's
actual listening duration. Recorded as `achieved_msg_per_sec` and
`rate_pct_of_target` (= 100 × achieved / target).

### 5.2 Throughput (bytes/s)

`bytes_received / window_secs`. Not currently displayed in REPORT.md
but recorded in `measurements`.

### 5.3 Latency p50/p95/p99

**Only for syslog-generator** (via its built-in `--metrics-addr` Prometheus
exporter, which produces `syslog_generator_send_duration_seconds_bucket`).
For external tools latency = `null`. This is a documented v1 limitation.

### 5.4 CPU% / memory

Optional via `MEASURE_RESOURCES=1` (using `time -l` on macOS, `time -v`
on Linux). Not part of v1's default run.

### 5.5 Setup complexity (LOC)

Static metric — `wc -l configs/workload_*.json` + runner scripts.
Computed once at publication.

## 6. Validation

Each cell before measurement goes through:

1. **Tool availability** — `command -v <tool>`.
2. **Config syntax** — `json.load(...)` + schema validation.
3. **Capability matrix** — `tool_supports_transport(tool, transport)`.
4. **Smoke test** — 2s mini-run, verify bytes_received > 0.

Any failure → cell marked `n_a`, `skipped`, or `failed` with reason.

## 7. Reproducibility contract

```bash
git clone https://github.com/pharmacolog/syslog-generator
cd syslog-generator
git checkout <commit-sha>
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1
```

**Guaranteed reproducible**:
- Same 4 workloads (`configs/workload_*.json`)
- Same dependency versions (`Cargo.lock`)
- Same commands (`scripts/03_run_*.sh`)
- Same output format (`perf/whitepaper-results.json`)

**NOT guaranteed** (by design):
- **Same throughput** — single-machine variance, OS jitter, CPU
  thermal throttling. Numeric values are recorded honestly but not
  asserted reproducible.
- **Same p50/p95/p99** — depends on machine load at run time.

## 8. No fabricated measurements

`harness/test_collect.py` enforces:
- `runs[i].status == 'completed'` AND `runs[i].measurements` is non-empty
- `runs[i].status in ('skipped', 'dry_run', 'n_a')` AND `runs[i].measurements` is null/absent
- `runs[i].status == 'n_a'` requires `na_reason` field

This invariant is tested by `make harness-tests`. Any failure exits
non-zero and blocks dispatch.

## 9. What v1 does NOT cover (out of scope)

- Cross-host network (only loopback)
- Real Kafka consumer (librdkafka)
- Long-running soak tests (>5 min)
- Memory leak detection (valgrind / heaptrack)
- IPv6
- DNS load distribution
- Realistic syslog-parser-side benchmarks (rsyslog queue full, etc.)
- TLS variants beyond 1.3

Each of these is a potential separate issue.

## 10. v1 limitations

1. **No warmup**. Window = sender runtime. Use `HARNESS_RATE` to pace
   short tests into the 95–105% gate.
2. **flog N/A for all network workloads** — flog v0.4.3 has no native
   network output and fixed record size. We do NOT attempt to fake
   network support by piping through nc.
3. **loggen = syslog-ng** (NOT rsyslog). UDP only.
4. **tcpkali = TCP/TLS only.** `--message-rate` (not -r, which is connection rate).
   Payload via pre-generated file (-f), NOT -m SIZE. One connection (-c 1).
5. **syslog-generator Kafka = N/A** until real consumer dependency.
6. **No "fully containerized" claim.** Dockerfiles with guessed versions
   were removed; see `benchmarks/whitepaper-2026/docker/README.md`.
7. **External pending items** (marketing publications, citations, UTM
   stars, Kafka consumer, containerized Dockerfiles) are tracked
   separately, not in this PR.
