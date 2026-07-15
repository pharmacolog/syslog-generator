# Security Policy

## Supported Versions

| Version | Supported          | EOL Date |
|---------|--------------------|----------|
| v10.7.x (latest) | ✅ Yes         | TBD      |
| v10.6.x | ✅ Security fixes only | 2027-01-01 |
| v10.5.x | ✅ Security fixes only | 2027-01-01 |
| v10.0.x — v10.4.x | ❌ No      | 2026-09-01 |
| < v10.0 | ❌ No              | 2026-01-01 |

We follow [semver](https://semver.org/) for versioning. Security fixes are
backported to the latest minor release of the previous major version for
**6 months** after the next major release.

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via one of the following methods:

### Primary: GitHub Security Advisories
- Go to https://github.com/pharmacolog/syslog-generator/security/advisories/new
- Click "New draft security advisory"
- Fill in the affected versions, description, and reproduction steps

### Secondary: Email
Send an encrypted email to **security@pharmacolog.example.com** (replace with
your actual security contact). Use our PGP key for sensitive reports
(see [SECURITY-PGP-KEY.asc](SECURITY-PGP-KEY.asc) if available).

### What to Include

Please include the following information:

1. **Type of vulnerability** (e.g., buffer overflow, injection, info disclosure)
2. **Affected versions** (e.g., v10.7.5 and earlier)
3. **Attack scenario** — how can an attacker exploit this?
4. **Reproduction steps** — minimal example or test case
5. **Potential impact** — what can an attacker achieve?
6. **Suggested fix** (if you have one)
7. **Your name/handle** (for credit, optional — you can request anonymity)

### Response Timeline

| Stage | Time |
|-------|------|
| Initial acknowledgment | within 48 hours |
| Triage and severity assessment | within 7 days |
| Patch for critical (CVSS ≥ 7.0) | within 30 days |
| Patch for high (CVSS 4.0–6.9) | within 60 days |
| Patch for medium/low (CVSS < 4.0) | next minor release |
| Public disclosure | after patch is released + 7 days |

We aim to follow [Google's Project Zero](https://googleprojectzero.blogspot.com/p/disclosure-policy.html)
90-day disclosure deadline.

## Security Update Policy

Security patches are released as **patch releases** (e.g., v10.7.5 → v10.7.6):

- **Critical (CVSS ≥ 9.0):** immediate patch release, hot-patched to main + dev
- **High (CVSS 7.0–8.9):** within 7 days
- **Medium (CVSS 4.0–6.9):** next scheduled release
- **Low (CVSS < 4.0):** bundled with next feature release

We use [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) to monitor
advisories automatically. See [docs/MIGRATION.md](docs/MIGRATION.md) for
upgrade instructions.

## Security Architecture

syslog-generator is a **load generator** (not a server), which has specific
security characteristics:

### Threat Model

| Threat | Mitigation |
|--------|-----------|
| **RCE via malicious profile** | Profiles are parsed as JSON/YAML via serde (safe parsers, no eval). Templates are rendered via `CompiledTemplate` (safe substitution, no shell exec). |
| **SSRF via target address** | No HTTP fetches in normal operation. Kafka target validates broker addresses against user profile only. |
| **TLS MITM** | Default `tls_insecure: false` enables certificate + hostname verification via rustls. Custom CA via `tls_ca_file`. |
| **Resource exhaustion** | Rate-limiting via governor (F1). Backpressure via bounded `mpsc(1024)`. Graceful shutdown via CancellationToken + SIGTERM. |
| **Credential leakage** | mTLS keys read once at startup, held in `TlsParams`, never logged. `tls_insecure: true` prints WARNING to stderr but doesn't leak secrets. |
| **Path traversal** | `load_profile_from_path` validates extension (`.json`/`.yaml`/`.yml`) before opening. File content not interpreted as code. |
| **DoS via large profile** | F13 validates profile size constraints. `templates_file`/`schema_file` are bounded by `serde_json::from_str` (no streaming for huge files). |
| **Integer overflow** | Most counters are `u64` (saturating naturally). Rate limiter uses `governor::Quota`. |

### Out-of-Scope Threats

The following are **explicitly out of scope** for syslog-generator:

- **DoS attacks against syslog SERVERS** — that's the server's responsibility
- **Privacy violations** — syslog-generator doesn't collect telemetry
- **Supply chain attacks via malicious syslog messages** — output is untrusted
  data going to user-controlled endpoints

### Cryptographic Inventory

| Crypto | Implementation | Notes |
|--------|---------------|-------|
| TLS handshake | rustls 0.23 | Pure Rust, audited |
| TLS cipher suites | ring (default) | FIPS-compatible when needed |
| CA bundle | webpki-roots 1.0 | Mozilla's trusted roots |
| Random number generator | ring (via rustls) | Cryptographically secure |
| Hash (cert fingerprint) | ring | SHA-256 via rustls |
| Kafka TLS | rskafka + rustls | Same stack |

### Dependency Policy

- **Minimum supported Rust version (MSRV):** 1.95 (enforced via `rust-toolchain.toml`)
- **Security policy:** `cargo-deny` with strict advisories level
- **Update policy:** Dependabot weekly, manual review for major version bumps
- **License policy:** Apache-2.0 only for direct deps (see `deny.toml`)

## Security Audits

| Date | Type | Findings |
|------|------|----------|
| 2026-07-13 | Self-audit (release audit, v10.7.2) | [AUDIT.md](AUDIT.md) |
| 2026-07-14 | cargo-deny | clean (no advisories) |
| 2026-07-15 | cargo-machete | clean (no unused deps) |
| TBD | External audit (OSS-friends or Trail of Bits) | pending funding |

## Reporting False Positives

If you believe a security advisory has been incorrectly flagged, please
open an issue with:
- The advisory ID
- Why you believe it's a false positive
- Your use case

We'll review and update if appropriate.

---

**Last updated:** 2026-07-15
**Contact:** security@pharmacolog.example.com (replace with your actual contact)