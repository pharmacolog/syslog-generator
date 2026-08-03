# Docker / compose for Whitepaper 2026 (Issue #106 / GTM-1)

This directory does NOT ship pre-baked Dockerfiles.

Earlier revisions of this harness included 5 pinned Dockerfiles (one per
tool) plus a docker-compose.yml and references to Redpanda. Those were
removed in the cleanup because:

1. Version pins (rsyslog=8.2112.0-2ubuntu2, flog@v0.4.3, kcat=1.7.1,
   tcpkali=v2.3.3) and SHA-256 sums were placeholder values, not
   verified against current upstream releases. Keeping them would have
   been worse than not having them.
2. The earlier `Dockerfile.loggen` claimed to be rsyslog. loggen is
   the **syslog-ng** loggen, not rsyslog's. Verified against the
   syslog-ng toolchain.
3. flog v0.4.3 has no native network output and a fixed record size per
   log type. A "flog container" that pipes to a network receiver does not
   satisfy the fairness protocol (no control over rate, no control over
   message size). flog cells are N/A for all network workloads.
4. tcpkali's upstream release artifacts change SHA-256 with each
   release. We refuse to ship a hardcoded SHA that may silently rot.
5. distroless base images do not have `/bin/sh`, so a `sleep infinity`
   entrypoint used in an earlier `docker-compose.yml` revision would
   fail to start on distroless.
6. Redpanda / Kafka cluster images were not used by the harness —
   kcat is out of scope as a compared tool, and syslog-generator's
   Kafka transport is N/A until a real consumer is wired in.

## What is provided

- `gen-cert.sh` — generates a self-signed TLS cert for `127.0.0.1` for
  the TLS workload. Run explicitly with
  `bash benchmarks/whitepaper-2026/docker/gen-cert.sh`. The cert is
  written to `docker/certs/server.pem` and `docker/certs/server.key`.

## What the user must do (out of scope of this harness)

If the user wants a fully containerized harness, the right path is:

1. Use the host-built syslog-generator binary at
   `target/release/syslog-generator` (no container needed for v1).
2. Install the external tools on the host (or use a separate, user-maintained
   container) and point the harness at them via env vars:
   - `LOGGEN_BIN=/path/to/loggen` (syslog-ng; `apt install syslog-ng-core`
     on Debian/Ubuntu, or build from source).
   - `FLOG_BIN=/path/to/flog` (no network support; cells will be marked N/A).
   - `TCPKALI_BIN=/path/to/tcpkali` (TCP/TLS only).
3. For Kafka, a real consumer (e.g., kcat consumer or librdkafka-based
   client) is required for completed Kafka measurements. The harness
   receiver is a TCP liveness probe only.

The harness intentionally does NOT claim "fully containerized" support.
See `docs/whitepaper-2026.md` and `benchmarks/whitepaper-2026/METHODOLOGY.md`
for the full reproducibility contract.
