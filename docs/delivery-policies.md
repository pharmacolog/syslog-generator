# Delivery Policies

> **Issue**: #239 (track A, docs, milestone v11.9) — закрывает Issue #89 task 8
> (документация trade-offs).
>
> **Refs**: Issue #89 (parent A3), Issue #163 (queue_capacity,
> on_target_failure), Issue #237 (queue_capacity implementation).
>
> **Обновлено**: 2026-08-04.

## Overview

`syslog-generator` поддерживает три **delivery policy** для multi-target
distribution (`distribution: "broadcast"`):

1. **`strict`** (default) — sequential `send().await` по всем targets.
2. **`independent`** — per-target queues с независимым send в параллель.
3. **`best-effort`** — `try_send` без await; неблокирующий режим.

Политика выбирается через поле `broadcast_policy` в Profile
(`src/generator/config.rs:377`) или CLI-флагом `--broadcast-policy`.
Для target failures используется связанный параметр `on_target_failure`
(`--on-target-failure`), который комбинируется с delivery policy и
определяет поведение при сбое конкретного target.

Документ описывает trade-offs, backpressure behavior, decision tree и
failure-mode handling для каждой policy, чтобы пользователь мог выбрать
осознанно.

## Policies

### Strict (default)

**Поведение:** Producer последовательно отправляет каждое сообщение во все
targets, ожидая завершения `send()` для каждого. Если любой target блокирует
(down/backpressure), producer ожидает его перед переходом к следующему.

**Реализация:** Последовательный `send().await` per target в
`run_phase_multi`. Используется single-producer/single-consumer loop
(`src/transport/mod.rs:97-100`).

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Гарантированный порядок доставки между targets (slow target не отстаёт) | ❌ Самый медленный — ждёт каждого target |
| ✅ Backward-compat default (v10.7.x поведение) | � Один медленный target тормозит весь broadcast |
| ✅ Простая семантика — один target failed → фаза failed | ❌ Нет защиты от backpressure (один TCP target с zero window блокирует всё) |
| ✅ Predictable latency per message | ❌ Не использует multi-core для параллельной отправки |

**Когда использовать:**

- Production telemetry, где порядок сообщений между targets важен
  (например, mirroring в SIEM + cold storage, и порядок должен совпадать).
- Smoke-тесты / CI, где predictable behavior важнее throughput.
- Маленькие target counts (1-3), где разница в throughput между
  policies минимальна.
- Сценарии, где `on_target_failure: "fail-phase"` (default) — strict
  даёт чёткий сигнал ошибки.

**Не использовать когда:**

- Multi-target high-throughput (10+ targets, >100k msg/s).
- Targets с разной пропускной способностью (file local vs TLS remote).
- Latency-sensitive workload, где нельзя ждать медленный target.

### Independent

**Поведение:** Каждый target получает собственную bounded queue
(`queue_capacity`, default 1024, настраивается). Producer кладёт сообщение
в очередь каждого target независимо через `try_send`. Per-target
consumer-воркеры читают из очереди и отправляют параллельно.

**Реализация:** `mpsc::channel` per target + `try_send` без await. При
переполнении очереди — drop newest с инкрементом
`messages_dropped_by_target_total` (метрика для alerting).

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Самый быстрый broadcast — все targets получают параллельно | ❌ Порядок доставки между targets НЕ гарантирован (каждый target независим) |
| ✅ Медленный target не блокирует быстрые (изолированная queue) | ❌ Требует больше памяти (`queue_capacity × num_targets × msg_size`) |
| ✅ Использует multi-core: N targets = N воркеров параллельно | ❌ Drop semantics при overflow — нужны метрики для мониторинга |
| ✅ Predictable throughput: bounded latency per target | ❌ `queue_capacity` tuning обязателен (default 1024 может быть мал) |

**Когда использовать:**

- Load testing (максимальная пропускная способность).
- Multi-target fan-out с разными latencies (TCP + UDP + file).
- Production, где throughput важнее гарантии порядка.
- Используется в preset `max-throughput` (Issue #92).

**Не использовать когда:**

- Порядок сообщений между targets критичен (replication scenarios).
- Память ограничена (например, embedded / OOM-prone окружение).
- Невозможно мониторить `messages_dropped_by_target_total` (drop semantics
  нужно отслеживать).

**Tuning:**

- `queue_capacity` — увеличить до 16384/65536 для high-throughput.
  Default 1024 — баланс memory/loss.
- Мониторинг `syslog_messages_dropped_by_target_total{target="..."}` —
  если растёт, увеличить capacity или переключиться на strict.

### Best Effort

**Поведение:** Producer использует `try_send` без await ни на одном target.
Неблокирующий режим с минимальной latency. При переполнении любой очереди —
drop newest без retries.

**Реализация:** Non-blocking `try_send` в каждую per-target queue. Producer
никогда не ожидает — даже для самых медленных targets.

**Trade-offs:**

| Преимущества | Недостатки |
|---|---|
| ✅ Минимальная latency (никогда не блокирует producer) | ❌ Наибольшие потери при переполнении (нет даже retry на await) |
| ✅ Predictable producer throughput | ❌ Не подходит для durability-критичных workload |
| ✅ Подходит для метрик/alerts, где потери допустимы | ❌ Порядок между targets НЕ гарантирован |

**Когда использовать:**

- Real-time метрики / alerts, где latency < 1ms критична, а потери
  приемлемы (например, system health telemetry).
- Stress-testing producer без backpressure.
- Сценарии, где drop semantics — это feature (sampling вместо buffering).

**Не использовать когда:**

- Durability важна (audit logs, financial transactions).
- Production с compliance constraints.
- Нагрузка неравномерная (burst → overflow → loss).

**Tuning:**

- `queue_capacity` должен быть достаточно большим, чтобы покрыть типичный
  burst. Default 1024 может быть недостаточен.
- `on_target_failure: "continue"` обязательно — strict semantics даст
  fail на первом dropped message, что для best-effort не имеет смысла.

## Decision Tree

```
                    ┌─ Гарантия порядка между targets?
                    │
                    ├── Да → strict
                    │
                    └── Нет → Гарантия доставки?
                                 │
                                 ├── Да → Independent (с мониторингом drops)
                                 │
                                 └── Нет → Latency < 1ms критична?
                                              │
                                              ├── Да → best-effort
                                              │
                                              └── Нет → Independent
```

**Типичные сценарии:**

| Сценарий | Рекомендация | Обоснование |
|---|---|---|
| Smoke test / CI | `strict` | Predictable, простая диагностика |
| Load testing (single target) | `strict` или default | Нет multi-target fan-out |
| Load testing (multi-target) | `independent` | Max throughput, мониторинг drops |
| Production SIEM fan-out | `independent` | Throughput важнее порядка |
| Mirroring в SIEM + cold storage | `strict` | Порядок критичен |
| Real-time alerting | `best-effort` | Latency критична, потери допустимы |
| Compliance audit | `strict` + `fail-phase` | Гарантия доставки + fail loud |
| Default (не знаете что выбрать) | `strict` | Safe default, predictable |

## Backpressure Behavior

Backpressure — это ситуация, когда target (TCP/TLS) не успевает принимать
сообщения. Поведение различается по policy:

### Strict

- Producer вызывает `send().await` на медленном target.
- Если TCP window = 0, producer блокируется до освобождения буфера.
- Все targets ждут этого медленного target перед получением следующего
  сообщения.
- **Метрика для мониторинга:** `send_duration_seconds` histogram — если
  p99 растёт, backpressure на одном из targets.

### Independent

- Producer вызывает `try_send` в каждую queue.
- Если queue медленного target заполнена — drop, инкремент
  `messages_dropped_by_target_total{target="..."}`.
- Быстрые targets продолжают получать сообщения без блокировки.
- **Метрика:** `syslog_messages_dropped_by_target_total` — alerting
  при rate > 0.

### Best Effort

- Producer вызывает `try_send` всегда, даже при заполненной queue.
- Drop semantics — единственный mode работы при backpressure.
- **Метрика:** `syslog_messages_dropped_by_target_total` — должно быть
  alerting обязательно.

## Failure Mode Handling

Параметр `on_target_failure` (`--on-target-failure`) определяет реакцию
на target failure (transport error, connection reset, TLS handshake fail).
Комбинируется с delivery policy.

### `fail-phase` (default)

Поведение: При ошибке отправки в любой target фаза завершается с ошибкой.
Producer останавливается, error propagate в exit code 1.

**Комбинация с policy:**

- **strict + fail-phase:** Самый строгий — последовательная семантика
  + fail loud. Подходит для compliance scenarios.
- **independent + fail-phase:** Параллельная отправка, но при ошибке
  любого target фаза падает. Менее полезно (лучше continue).
- **best-effort + fail-phase:** Противоречие — best-effort подразумевает
  "продолжать при потерях", а fail-phase требует "упасть". **Не
  рекомендуется**.

### `continue`

Поведение: Ошибка target логируется в `errors_total{target="..."}`,
фаза продолжает работу. Producer не останавливается.

**Комбинация с policy:**

- **strict + continue:** Ошибка на одном target логируется, но producer
  переходит к следующему target. Семантика: "потеряли в target-X,
  но остальные получили".
- **independent + continue:** Стандартная production конфигурация для
  high-availability fan-out. Используется в preset `max-throughput`.
- **best-effort + continue:** Идемпотентно — оба режима подразумевают
  "продолжать всегда".

### `disable-target`

Поведение: При ошибке target помечается как disabled и исключается из
дальнейшей отправки. Остальные targets продолжают получать сообщения.
Disabled target может быть re-enabled через reconnect (Issue #163 F16).

**Комбинация с policy:**

- **strict + disable-target:** Полезно для long-running фазы с
  flaky targets — после первого fail проблемный target исключается,
  остальные продолжают. Producer не тратит время на retries.
- **independent + disable-target:** Подходит для fan-out, где один
  target может быть down длительное время (maintenance).
- **best-effort + disable-target:** Семантически бессмысленно (best-effort
  уже drop'ает). **Не рекомендуется**.

### Decision Matrix

| Политика | use `fail-phase` when... | use `continue` when... | use `disable-target` when... |
|---|---|---|---|
| `strict` | Compliance, audit logs | Допустимы потери в одном target | Long-running + flaky targets |
| `independent` | (Не рекомендуется) | Production HA fan-out | Maintenance windows |
| `best-effort` | (Противоречие) | Real-time alerts, drop semantics | (Не рекомендуется) |

## Examples

### YAML

#### Strict (default)

```yaml
distribution: broadcast
broadcast_policy: strict
on_target_failure: fail-phase
queue_capacity: 1024

targets:
  - address: siem-primary.example.com:514
    transport: tls
  - address: cold-storage.example.com:514
    transport: tcp

phases:
  - name: production-mirror
    messages_per_second: 1000
    duration_secs: 3600
    templates:
      - "app=auth user={{faker.username}} action=login"
```

#### Independent

```yaml
distribution: broadcast
broadcast_policy: independent
on_target_failure: continue
queue_capacity: 65536

targets:
  - address: 10.0.0.10:514
    transport: tcp
  - address: 10.0.0.11:514
    transport: tcp
  - address: 10.0.0.12:514
    transport: udp

phases:
  - name: high-throughput-fanout
    messages_per_second: 100000
    duration_secs: 600
    templates:
      - "<14>host={{hostname}} seq={{sequence}}"
```

#### Best Effort

```yaml
distribution: broadcast
broadcast_policy: best-effort
on_target_failure: continue
queue_capacity: 4096

targets:
  - address: metrics-collector.internal:8125
    transport: udp

phases:
  - name: real-time-metrics
    messages_per_second: 50000
    duration_secs: 86400
    templates:
      - "metric=cpu value={{faker.floating_between(0,100)}}"
```

### CLI

```bash
# Strict (default)
syslog-generator \
  --preset balanced \
  --target siem-primary:514 \
  --target cold-storage:514 \
  --distribution broadcast

# Independent с max-throughput preset
syslog-generator \
  --preset max-throughput \
  --target 10.0.0.10:514 \
  --target 10.0.0.11:514 \
  --target 10.0.0.12:514 \
  --distribution broadcast \
  --broadcast-policy independent \
  --on-target-failure continue

# Best Effort
syslog-generator \
  --target metrics-collector:8125 \
  --broadcast-policy best-effort \
  --on-target-failure continue \
  --queue-capacity 4096 \
  --rate 50000

# Override preset values через --set
syslog-generator \
  --preset max-throughput \
  --set queue_capacity=131072 \
  --set broadcast_policy=independent \
  --target fast-target:514
```

## Cross-references

- **Issue #89** — parent (A3: broadcast distribution + queue).
- **Issue #163** — queue_capacity, on_target_failure.
- **Issue #237** — queue_capacity implementation.
- **Issue #92** — presets (max-throughput, balanced, low-latency).
- **`src/transport/mod.rs`** — `BroadcastPolicy`, `TargetFailurePolicy` enums.
- **`src/generator/config.rs`** — `Profile.broadcast_policy`,
  `Profile.on_target_failure`, `Profile.queue_capacity` fields.
- **`docs/CLI_REFERENCE.md`** — `--broadcast-policy`, `--on-target-failure`,
  `--queue-capacity` flags.
- **`docs/USER_GUIDE.md`** — multi-target profile examples.
- **`docs/presets.md`** — preset definitions и их policy choices.

## See also

- **PR-A3** (Issue #89) — original broadcast distribution implementation.
- **PR-A4** (Issue #161) — concurrent generator pipeline, использует
  `independent` policy.
- **docs/PERFORMANCE.md** — benchmark results для каждой policy.
- **Metrics reference** — `syslog_messages_dropped_by_target_total`,
  `syslog_errors_total{target="..."}`, `send_duration_seconds`.
