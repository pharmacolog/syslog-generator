# Opt-level decision report (Issue #194)

> Generated: 2026-08-04, hardware=Apple M1 (darwin/arm64).

> Goal: ≥30% size reduction AND ≤5% perf regression vs `opt-level=3`.


## Build artifacts

| Variant | Binary size (bytes) | MB | Δ vs opt=3 |
|---------|-------------------:|---:|-----------:|
| opt=3 | 10,937,184 | 10.43 | +0.0% |
| opt=s | 10,937,184 | 10.43 | +0.0% |
| opt=z | 10,937,184 | 10.43 | +0.0% |

## Bench deltas (hot_path, --quick)

| Bench | opt=3 (ns) | opt=s (ns) | opt=z (ns) | Δs vs 3 | Δz vs 3 |
|-------|-----------:|-----------:|-----------:|--------:|--------:|
| faker_ipv4 | 88 | 88 | 88 | +0.9% | +0.0% |
| faker_username | 20 | 20 | 20 | +0.9% | +0.4% |
| faker_uuid | 32 | 33 | 33 | +0.7% | +1.3% |
| hot_path/rfc5424_with_faker | 1687 | 1685 | 1670 | -0.1% | -1.0% |
| template_render_only | 104 | 102 | 102 | -1.7% | -2.0% |

## Decision


**No winner**: ни один вариант не проходит dual criteria.
Recommend: оставить opt-level=3, не применять change.
