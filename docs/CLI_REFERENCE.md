# CLI Reference (v10.8.0)

> PR-C2 (Issue #91): полный reference для CLI флагов и presets.
> Обновлено: 2026-07-24.

## Synopsis

```
syslog-generator [OPTIONS] --profile <FILE>
```

## Global options

| Flag | Description | Default |
|---|---|---|
| `--profile <FILE>` | JSON/YAML profile path | required (or --target) |
| `--target <ADDR>` | Add target (`-t`) — repeatable | - |
| `--transport <MODE>` | Transport for all targets | tcp |
| `--distribution <MODE>` | round-robin, broadcast, weighted | round-robin |
| `--rate <N>` | Messages per second | 0 (unlimited) |
| `--duration <SEC>` | Phase duration | 0 (unlimited) |
| `--total <N>` | Total messages | - |
| `--format <MODE>` | Override phase format | - |
| `--seed <N>` | RNG seed | - |
| `--message <TPL>` | Override templates (`-m`) | - |
| `--metrics-addr <ADDR>` | HTTP /metrics endpoint | - |
| `--preset <NAME>` | Apply named preset (see below) | - |
| `--set KEY=VALUE` | Point-Profile override (repeatable) | - |
| `--tls-ca-file <PATH>` | Override TargetConfig::tls_ca_file | - |
| `--tls-domain <DOMAIN>` | Override TargetConfig::tls_domain | - |
| `--tls-insecure` | Override TargetConfig::tls_insecure | - |
| `--connections <N>` | Override TargetConfig::connections | - |
| `--framing <MODE>` | Override TargetConfig::framing (octet-counting, non-transparent) | - |
| `--dry-run` | Validate profile, don't send | - |
| `--print-config` | Print effective config and exit | - |
| `--schema-strict` | Validate against embedded JSON Schema | - |
| `--validate` | Validate and exit (don't send) | - |

## Presets (--preset NAME)

### max-throughput

Максимальная пропускная способность. Настройки:
- `queue_capacity` = 65536
- `broadcast_policy` = "independent" (Issue #89)
- `on_target_failure` = "continue"

### balanced

Defaults. No-op. Подходит для smoke-тестов и базового использования.

### low-latency

Минимальная latency. Настройки:
- `queue_capacity` = 64
- `broadcast_policy` = "strict" (sequential send per target)
- `on_target_failure` = "fail-phase" (fail on first error)

## --set KEY=VALUE

Точечные overrides любого публичного поля Profile. Поддерживает JSON path syntax:

```bash
# Top-level field
--set distribution=broadcast

# Nested array + field
--set 'targets[0].connections=8'

# Multiple
--set 'phases[0].messages_per_second=500000' \
--set metrics_addr=127.0.0.1:9090
```

Поддерживаются типы значений:
- Numbers: `--set queue_capacity=65536` → u64
- Booleans: `--set tls_insecure=true` → bool
- Strings: `--set distribution=broadcast` → String

## Examples

```bash
# Простой TCP load test
syslog-generator --target 127.0.0.1:514 --rate 10000

# С profile и preset
syslog-generator --preset max-throughput --profile bench.yaml

# С --set overrides
syslog-generator --preset balanced \
  --set 'targets[0].connections=16' \
  --set 'phases[0].messages_per_second=200000'

# Dry-run
syslog-generator --profile prod.yaml --dry-run --schema-strict

# Validate
syslog-generator --profile prod.yaml --validate
```

## Subcommands

| Subcommand | Description |
|---|---|
| `completions bash\|zsh\|fish\|powershell\|elvish` | Generate shell completions |
| `man` | Generate man page (stdout) |

## Exit codes

| Code | Description |
|---|---|
| 0 | Success |
| 1 | Failure (config, network, runtime) |
| 78 | EX_CONFIG (config error, F13) |
| 130 | SIGINT (Ctrl-C) |

## Environment variables

- `RUST_LOG` — tracing log level (default: `info`)
- `RUST_BACKTRACE` — enable backtrace on panic (0/1)

## Files

- `docs/USER_GUIDE.md` — user-facing guide
- `docs/PERFORMANCE.md` — performance tuning
- `docs/TESTING.md` — testing strategy
- `docs/COORDINATION.md` — multi-agent coordination

## See also

- `examples/` — примеры профилей
- `src/transport/` — transport implementations
- `src/plan/` — CompiledPlan (PR-A2)

Refs #91
