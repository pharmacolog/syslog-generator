# Presets

> **Issue**: #240 (track B, docs, milestone v11.9) — закрывает Issue #92
> task 3 (документация).
>
> **Refs**: Issue #92 (parent B3), `src/cli/preset.rs` (implementation),
> Issue #89 (broadcast_policy), Issue #163 (queue_capacity).
>
> **Обновлено**: 2026-08-04.

## Overview

`syslog-generator` поддерживает **named presets** — pre-configured bundles
параметров для типичных use cases. Presets применяются через CLI-флаг
`--preset NAME` и могут быть overridden позже через `--set KEY=VALUE`.

**Доступные presets (3):**

1. **`max-throughput`** — максимальная пропускная способность.
2. **`balanced`** — defaults (no-op). Default если `--preset` не указан.
3. **`low-latency`** — минимальная latency.

**Архитектура:** `Preset` struct содержит `Vec<(String, String)>` —
(key, value) пары, applied через `apply_set_overrides()` (тот же
механизм, что и `--set`). Это позволяет preset'ам иметь частичные
overrides — None для полей, которые должны остаться defaults.

Реализация: [`src/cli/preset.rs`](../../src/cli/preset.rs).

## Available Presets

### max-throughput

**Назначение:** Максимальная пропускная способность для load testing
и high-throughput production scenarios.

**Когда использовать:**

- Load testing (benchmarking SIEM, log aggregators, syslog servers).
- Production fan-out с high message rate (>100k msg/s).
- Multi-target broadcast, где throughput важнее гарантии порядка.
- Stress testing инфраструктуры (capacity planning).

**Настройки:**

| Параметр | Значение | Эффект |
|---|---|---|
| `queue_capacity` | `65536` | Большая per-target queue — реже drop'ы при burst'ах |
| `broadcast_policy` | `"independent"` | Параллельная отправка во все targets |
| `on_target_failure` | `"continue"` | Не падать на ошибке одного target |

**Реализация:** [`Preset::max_throughput()`](../../src/cli/preset.rs:44).

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Максимальный throughput — все targets получают параллельно | ❌ Порядок между targets НЕ гарантирован |
| ✅ Медленный target не блокирует быстрые | ❌ Требует мониторинга `messages_dropped_by_target_total` |
| ✅ Resilient к single-target failures (`continue`) | ❌ Память: `queue_capacity × num_targets × msg_size` |

**Метрики для мониторинга:**

- `syslog_messages_dropped_by_target_total` — должен быть 0 при нормальной
  нагрузке. Рост → увеличить `queue_capacity` или перейти на `strict`.
- `syslog_messages_total{transport="..."}` — проверять rate по targets.
- `send_duration_seconds` — p99 latency для каждого target.

### balanced (default)

**Назначение:** Разумные defaults, подходящие для большинства use cases.

**Когда использовать:**

- Smoke-тесты и CI runs (predictable behavior).
- Production telemetry без специфических требований к throughput/latency.
- First-time users (default работает, ничего настраивать не нужно).
- Документация / примеры (default = "что работает out-of-box").

**Настройки:**

| Параметр | Значение | Эффект |
|---|---|---|
| (no overrides) | — | Используются defaults из Profile |

**Реализация:** [`Preset::balanced()`](../../src/cli/preset.rs:56).

**Default Profile values (применяются, если `--preset balanced` или
preset не указан):**

- `broadcast_policy`: `"strict"` (sequential send per target)
- `queue_capacity`: `1024`
- `on_target_failure`: `"fail-phase"` (loud failure)
- `distribution`: `"round-robin"` (default в Profile)

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Safe default — гарантия порядка (strict) | ❌ Не оптимален для high-throughput |
| ✅ Loud failures (`fail-phase`) — простая диагностика | ❌ Predictable, но не max performance |
| ✅ Минимальное использование памяти (queue_capacity=1024) | ❌ Sequential send — медленнее independent |

### low-latency

**Назначение:** Минимальная end-to-end latency. Подходит для real-time
alerts и latency-sensitive workload.

**Когда использовать:**

- Real-time alerting (< 10ms target).
- Latency-sensitive metrics collection.
- Тестирование SLA-sensitive инфраструктуры.
- Benchmarking latency-bound систем (HFT-style logs, IDS alerts).

**Настройки:**

| Параметр | Значение | Эффект |
|---|---|---|
| `queue_capacity` | `64` | Минимальная queue — drop'ы при burst'ах, но нет batching delay |
| `broadcast_policy` | `"strict"` | Sequential, но короткая queue → нет накопления |
| `on_target_failure` | `"fail-phase"` | Loud failure — latency-critical workload не tolerates silent loss |

**Реализация:** [`Preset::low_latency()`](../../src/cli/preset.rs:67).

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Минимальная latency — короткая queue не накапливает batching delay | ❌ Высокие drop'ы при burst'ах (queue_capacity=64) |
| ✅ Loud failures — простая диагностика для latency issues | ❌ Sequential send медленнее independent |
| ✅ Predictable end-to-end latency (без batching surprises) | ❌ Не подходит для high-throughput (>10k msg/s) |

**Метрики для мониторинга:**

- `send_duration_seconds` — p99 должен быть < target SLA. Рост →
  проблемы с target connectivity.
- `syslog_messages_dropped_by_target_total` — при low-latency preset
  ожидаемый (queue мал). Alerting на rate > N/sec.
- `errors_total` — должен быть 0. Любая ошибка → fail phase.

## Decision Tree

```
                ┌─ Что важнее: throughput или latency?
                │
                ├── Throughput ─┬─ Гарантия порядка?
                │              │
                │              ├── Да → strict + max-throughput settings (см. --set)
                │              │
                │              └── Нет → max-throughput
                │
                ├── Latency ────┬─ Гарантия доставки?
                │              │
                │              ├── Да → low-latency
                │              │
                │              └── Нет → best-effort policy + low-latency preset
                │
                └── Не знаю / smoke-test → balanced (default)
```

**Типичные сценарии:**

| Сценарий | Preset | Обоснование |
|---|---|---|
| CI smoke test | `balanced` | Predictable, simple diagnostics |
| Load testing (max msg/s) | `max-throughput` | Max throughput |
| Load testing (p99 latency) | `low-latency` | Min latency |
| Production SIEM fan-out | `max-throughput` | Throughput + multi-target |
| Real-time alerting | `low-latency` | Latency critical |
| First-time user | `balanced` | Safe default |
| Compliance audit | `balanced` + `strict` override | Гарантия порядка + loud failures |
| Capacity planning benchmark | `max-throughput` + `--duration` | Max throughput за интервал |

## Examples

### CLI usage

```bash
# Smoke test (balanced)
syslog-generator --profile smoke-test.yaml

# Load testing
syslog-generator --preset max-throughput --profile bench.yaml

# Real-time metrics
syslog-generator --preset low-latency --target metrics-collector:8125

# Manual trigger workflow_dispatch
gh workflow run fuzz-nightly.yml \
  -f fuzz_seconds=3600 \
  -f max_len=8192
```

### YAML override

Preset можно override'нуть через `--set` flags. Это позволяет
комбинировать preset'ы с custom settings:

```bash
# max-throughput + custom queue_capacity
syslog-generator \
  --preset max-throughput \
  --set queue_capacity=131072 \
  --profile bench.yaml

# low-latency + best-effort policy
syslog-generator \
  --preset low-latency \
  --set broadcast_policy=best-effort \
  --set on_target_failure=continue \
  --target alerts.internal:514

# balanced + strict policy + high capacity
syslog-generator \
  --preset balanced \
  --set broadcast_policy=strict \
  --set queue_capacity=16384 \
  --profile production.yaml
```

**Правила override:**

- `--set` flags применяются **после** preset'а, поэтому preset defaults
  могут быть overridden.
- Если preset задаёт `queue_capacity=65536`, а `--set queue_capacity=2048`,
  финальное значение — `2048`.
- `--set` syntax: `--set KEY=VALUE` (point-Profile JSON path).
- Multi-level paths поддерживаются: `--set 'targets[0].connections=16'`.

**Типичные комбинации:**

| Цель | Preset | --set override |
|---|---|---|
| Max throughput + большой burst buffer | `max-throughput` | `queue_capacity=131072` |
| Low latency + non-blocking | `low-latency` | `broadcast_policy=best-effort` |
| Balanced + max capacity | `balanced` | `queue_capacity=65536` |
| Max throughput + strict ordering | `max-throughput` | `broadcast_policy=strict` |
| Low latency + multi-target | `low-latency` | `targets[1].address=secondary:514` |

## Custom Preset Configuration

Built-in presets (`max-throughput`, `balanced`, `low-latency`) hard-coded
в [`src/cli/preset.rs`](../../src/cli/preset.rs:42-74). Для custom
preset'ов есть два пути:

### Option 1: Inline `--set` flags (рекомендуется)

Создайте shell alias или wrapper script:

```bash
# ~/.bashrc
alias syslog-bench='syslog-generator --preset max-throughput \
  --set queue_capacity=131072 \
  --set broadcast_policy=independent \
  --set on_target_failure=continue'

# Usage
syslog-bench --profile benchmark.yaml --duration 300
```

### Option 2: Custom YAML profile (для complex configs)

Для сложных конфигураций с custom targets, phases, etc.:

```yaml
# profiles/custom-throughput.yaml
distribution: broadcast
broadcast_policy: independent
queue_capacity: 131072
on_target_failure: continue

targets:
  - address: primary.example.com:514
    transport: tls
    tls_ca_file: /etc/ca.pem
  - address: secondary.example.com:514
    transport: tcp
    connections: 8

phases:
  - name: production-load
    messages_per_second: 250000
    duration_secs: 3600
    templates:
      - "<14>host={{hostname}} seq={{sequence}} msg=normal"
      - "<11>host={{hostname}} seq={{sequence}} msg=alert"
    template_weights: [0.95, 0.05]  # 95% normal, 5% alerts
```

```bash
syslog-generator --profile profiles/custom-throughput.yaml
```

### Option 3: Code modification (для maintainers)

Добавить новый preset в [`src/cli/preset.rs`](../../src/cli/preset.rs):

```rust
impl Preset {
    /// Built-in preset: custom-enterprise.
    pub fn custom_enterprise() -> Self {
        Self {
            name: "custom-enterprise".to_string(),
            overrides: vec![
                ("queue_capacity".to_string(), "32768".to_string()),
                ("broadcast_policy".to_string(), "independent".to_string()),
                ("on_target_failure".to_string(), "continue".to_string()),
            ],
        }
    }
}

pub fn parse_preset(name: &str) -> Result<Preset> {
    match name {
        // ... existing presets ...
        "custom-enterprise" => Ok(Preset::custom_enterprise()),
        _ => Err(anyhow!("unknown preset {:?}", name)),
    }
}
```

**⚠️ Warning:** Модификация `preset.rs` требует:
- PR с review (изменяет публичный API).
- Unit-тесты в `src/cli/preset.rs::tests` (parse + apply).
- Обновление документации (этот файл + `docs/CLI_REFERENCE.md`).

## Cross-references

- **Issue #92** — parent (B3: named presets).
- **`src/cli/preset.rs`** — implementation (Preset struct + parse + apply).
- **`src/cli/set_override.rs`** — `apply_set_overrides()` механизм,
  используемый preset'ами.
- **`docs/CLI_REFERENCE.md`** — `--preset` flag reference.
- **`docs/delivery-policies.md`** — детальное описание
  `broadcast_policy`, `queue_capacity`, `on_target_failure`.
- **`docs/USER_GUIDE.md`** — use-case examples с preset'ами.

## See also

- **PR-B3** (Issue #92) — original preset implementation.
- **PR-A3** (Issue #89) — broadcast_policy, on_target_failure.
- **PR-A4** (Issue #161) — concurrent pipeline (использует
  `max-throughput` defaults).
- **Issue #163** — queue_capacity tuning guide.
- **Issue #237** — queue_capacity implementation details.
