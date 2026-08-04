# Сравнительный benchmark генераторов нагрузки для syslog

> **RU-драфт** для Habr.
> **Issues:** [#106](https://github.com/pharmacolog/syslog-generator/issues/106) (harness),
> [#196](https://github.com/pharmacolog/syslog-generator/issues/196) (real VM run),
> [#199](https://github.com/pharmacolog/syslog-generator/issues/199) (publication).
> **Milestone:** v11.6
> **Статус:** Harness + methodology готовы (`schema_only`).
> Численных результатов **НЕТ** — см. `perf/whitepaper-results.json::status`
> (текущее `schema_only`). Прогон запланирован по
> [`benchmarks/whitepaper-2026/docs/benchmark-runbook.md`](../../benchmarks/whitepaper-2026/docs/benchmark-runbook.md).
> **EXTERNAL PENDING:** Habr-публикация, dev.to-публикация, Telegram, Twitter/X,
> Reddit/HN, цитирования, UTM-звёзды — **ни одно из этих действий не выполняется
> в этом PR**. См. [Issue #199](https://github.com/pharmacolog/syslog-generator/issues/199).

## TL;DR

Это **черновик** Whitepaper 2026. Цифр в нём **намеренно нет** — они
появятся после реального прогона на `c5.2xlarge` (Issue #196) и попадут
в `perf/whitepaper-results.json`. Harness сравнивает **4 инструмента**
из Issue #106: `syslog-generator` vs `loggen` (syslog-ng) vs `flog` vs
`tcpkali`. kcat (kafkacat) **НЕ** входит в сравниваемый набор — в v1
нет рабочего Kafka consumer'а, поэтому для всех 4 tools Kafka cells
помечены `N/A`.

В этом документе:

1. **Зачем** нужен честный benchmark (а не marketing-claims).
2. **Что** будем мерить (4 workload'а × 4 инструмента, 16 ячеек).
3. **Как** будем мерить (fairness protocol, hardware lock, RM-friendly tooling).
4. **Методология** ([`benchmarks/whitepaper-2026/METHODOLOGY.md`](../../benchmarks/whitepaper-2026/METHODOLOGY.md)).
5. **Когда использовать** какой инструмент (decision tree).
6. **Чего в драфте НЕТ** (honest limitations: flog network N/A, Kafka stub, etc.).
7. **Как воспроизвести** (3 команды).
8. **Call to action** (звёзды, цитирования, feedback).

## Когда использовать какой инструмент

Это самая практически полезная часть. **До** численных результатов
она базируется на verified capability matrix (Issue #106 §3.4), не на
сравнении скоростей.

### `syslog-generator` — для SIEM soak-testing

**Когда выбрать:**

- Вам нужна **высокая константная нагрузка** на SIEM / log management
  pipeline (10k–100k+ msg/s) на часы или дни.
- Нужны **native UDP / TCP / TLS** на одном бинаре без зависимостей.
- Хотите **предсказуемый latency** (p50/p95/p99 через Prometheus exporter).
- Нужны **configuration presets** для протоколов (RFC 5424, RFC 3164, CEF, LEEF, JSON).

**Когда НЕ выбрать:**

- Нужен **HTTP-based load** (REST / gRPC) — `syslog-generator` per design
  не генерирует HTTP.
- Нужен **Kafka producer** — Kafka transport в v1 помечен `N/A` (нет
  real consumer; см. Issue #106 §external_pending).
- Нужен **multi-protocol mixed mode** в одном workload'е — каждый
  workload один транспорт.

### `loggen` (syslog-ng) — для тестирования TCP `octet-counting`

**Когда выбрать:**

- Вам нужно проверить **TCP octet-counting framing** против реального
  syslog-ng сервера (например, regression test после upgrade).
- У вас уже есть syslog-ng на хосте и не хочется ничего ставить.
- Работает **из коробки** на Ubuntu 22.04 (`apt install syslog-ng-core`).

**Когда НЕ выбрать:**

- Только **UDP** (loggen UDP-only с 4.6.x; см. METHODOLOGY §3.4).
- Вы тестируете **нагрузку в SIEM** в production-режиме — loggen по
  умолчанию однопоточный, syslog-generator параллелит.
- Нужен **TLS** — loggen его не умеет (см. capability matrix).

### `flog` — для JSON/HTTP logs

**Когда выбрать:**

- Нужен **fake structured log** в JSON / Common Log Format / Apache
  access log (например, для тестирования ELK / Loki).
- Нужен **фиксированный формат** и **детерминированная рандомизация**
  (flog поддерживает seed).
- Вам не нужно отправлять по сети — output в stdout/file достаточен.

**Когда НЕ выбрать:**

- **Любая** network workload — flog v0.4.3 **не имеет** native network
  output. Все 4 сетевых ячейки = `N/A`. (Мы не пытаемся pipe'ить через
  `nc` — это нарушило бы fairness protocol.)
- Нужен **кастомный template** — flog фиксирует размер записи per log type.

### `tcpkali` — для capacity planning

**Когда выбрать:**

- Нужен **TCP/TLS load test на уровне connection bandwidth** (multi-connection,
  connection rate, message rate).
- Нужен **подробный latency stats** (tcpkali сам собирает histogram).
- Capacity planning для **брокеров / pub-sub** (RabbitMQ, NATS, Kafka wire
  protocol bypass).

**Когда НЕ выбрать:**

- Только **UDP** — tcpkali TCP/TLS only.
- Нужен **syslog framing** — tcpkali не валидирует syslog parser, просто
  шлёт байты. Полезно для емкости, **не** для корректности.

### Решающее дерево

```text
Нужен UDP?
  ├─ Да  → syslog-generator (>10k msg/s) или loggen (≤10k msg/s, syslog-ng bundled)
  └─ Нет → TCP?
              ├─ Да  → TCP framing — syslog-generator или tcpkali
              └─ Нет → TLS?
                          ├─ Да → syslog-generator (single binary) или tcpkali (bandwidth)
                          └─ Нет → Kafka?
                                       ├─ Да → v1: ни один из 4 tools (N/A);
                                       │         ждать Issue #106 Kafka consumer
                                       └─ Нет → HTTP/REST?  → НЕ из 4 tools,
                                                            см. flog (file/stdout only)
```

## Placeholder таблицы (заполнятся после Issue #196)

> **Где взять числа:** [`perf/whitepaper-results.json`](../../perf/whitepaper-results.json).
> После Issue #196 run их можно копировать в эти таблицы. См. также
> [`benchmarks/whitepaper-2026/results/EXPECTED.md`](../../benchmarks/whitepaper-2026/results/EXPECTED.md)
> для schema.

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

### Latency (p50/p95/p99) — только `syslog-generator`

(METHODOLOGY §5.3: external tools latency = `null` в v1. tcpkali даёт
свою гистограмму, но в общий артефакт не попадает — out of scope v1.)

| Workload | p50 (ms) | p95 (ms) | p99 (ms) |
|---|---|---|---|
| `udp_100rps_256b`  | _pending_ | _pending_ | _pending_ |
| `tcp_10krps_1kb`   | _pending_ | _pending_ | _pending_ |
| `tls_5krps_1kb`    | _pending_ | _pending_ | _pending_ |

### Resource usage (после `MEASURE_RESOURCES=1` — opt-in)

| Workload | Tool | CPU% (avg) | RSS (MiB) | Wall time (s) |
|---|---|---|---|---|
| _pending_ | _pending_ | _pending_ | _pending_ | _pending_ |

> Resource usage **opt-in** (`MEASURE_RESOURCES=1`); по умолчанию
> `make all` не собирает. Issue #196 acceptance — main metrics
> (throughput, latency), ресурсы — бонус.

## Honest limitations

> Этот раздел **намеренно подробный**. Без этих оговорок whitepaper
> перестаёт быть honest, и публикация на Habr / dev.to рискует стать
> marketing-claims. После Issue #196 run этот раздел **не сокращается** —
> цифры добавляются, ограничения остаются.

### Что **НЕ** входит в v1

1. **Kafka cell = `N/A` для всех 4 tools.** Не потому что они не умеют
   Kafka, а потому что v1 harness receiver — это TCP liveness probe,
   без реального consumer'а. Чтобы Kafka cell стал `completed`,
   нужен librdkafka-based consumer (отдельный issue, см.
   `perf/whitepaper-results.json::external_pending`).
2. **flog network = `N/A` для всех 4 cell'ов.** flog v0.4.3 поддерживает
   только stdout/stderr/file. Мы **не пытаемся** pipe'ить через `nc` —
   это нарушило бы fairness protocol (см. METHODOLOGY §3.4).
3. **Latency p50/p95/p99 — только `syslog-generator`** (Prometheus
   exporter). Для loggen / flog / tcpkali latency = `null`. tcpkali
   собирает свою гистограмму, но в общий `runs[]` не попадает — это
   out of scope v1.
4. **Cross-host network не тестируется.** Все 4 workload'а идут на
   `127.0.0.1` (loopback). Real network benchmarking — отдельный issue.
5. **Только Linux x86_64.** c5.2xlarge (Intel Xeon Platinum 8275CL).
   ARM (Graviton) и macOS — out of scope v1 (см. METHODOLOGY §2).

### Что **МОЖЕТ** измениться

- **Issue #196 run запланирован, но не выполнен.** Цифры в этом
  драфте — pending. Если run покажет существенное отклонение от
  ожиданий (например, syslog-generator не достигает 10k msg/s по TCP),
  мы пересмотрим claims в §«Когда использовать» и либо уберём, либо
  переформулируем.
- **Synthetic variance.** CPU frequency, IRQ, OS jitter дают
  ±3–5% разброс. Best-of-3 (см. EXPECTED.md §4.2) сглаживает, но не
  устраняет. **Numeric reproducibility не гарантируется** (claimed
  в METHODOLOGY §7).
- **Tool versions pinning.** Привязка версий к
  `syslog_generator=v11.6.0`, `loggen=4.6.4`, `flog=v0.4.3`,
  `tcpkali=2.10.0`. Более новые версии могут показать другие числа;
  harness сознательно НЕ поддерживает "latest" — reproducibility
  важнее актуальности.

### Что **точно НЕ** будет

- ❌ Никаких fabricated measurements. Invariant `no_fabricated_measurements`
  enforced в `benchmarks/whitepaper-2026/harness/test_collect.py`.
- ❌ Никаких "X быстрее Y на 47%". Цифры будут concretely per-cell,
  не relative.
- ❌ Никаких vendor-benchmark adaptations. Все числа — наш harness.
- ❌ Никаких "fully containerized" claims. Dockerfile'ы с guessed
  versions удалены (см. `benchmarks/whitepaper-2026/docker/README.md`).

## 1. Зачем

Когда гуглишь «syslog load generator», выпадает длинный список. Каждый
инструмент в README пишет "high throughput" или "low latency". Но
**claims несравнимы**: разные машины, разные сети, разные message size,
разные receivers, разные durations. Перемножать нельзя.

Цель benchmark'а — поставить все 4 инструмента на **одну машину**, в
**одну сеть**, с **одинаковым message size**, **одинаковой длительностью**
и **одинаковым receiver'ом**. После прогона цифры публичны и
воспроизводимы.

## 2. 4 сравниваемых инструмента (verified)

| Tool | Source | UDP | TCP | TLS | Kafka |
|---|---|---|---|---|---|
| `syslog-generator` | этот репозиторий | ✓ | ✓ | ✓ | **N/A** (нет real consumer) |
| `loggen` | **syslog-ng** (НЕ rsyslog) | ✓ | **N/A** (UDP-only) | **N/A** (UDP-only) | **N/A** |
| `flog` | mingrammer v0.4.3 | **N/A** | **N/A** | **N/A** | **N/A** |
| `tcpkali` | machinezone | **N/A** | ✓ | ✓ | **N/A** |

## 3. Workloads (Issue #106 spec)

| ID | Транспорт | Rate | Payload | Длительность |
|---|---|---|---|---|
| `udp_100rps_256b`  | UDP  | 100 msg/s  | 256 B | 30 s |
| `tcp_10krps_1kb`   | TCP  | 10 000 msg/s | 1 KB | 30 s |
| `tls_5krps_1kb`    | TLS  | 5 000 msg/s  | 1 KB | 30 s |
| `kafka_50krps_256b`| Kafka | 50 000 msg/s | 256 B | 30 s |

16 ячеек (4 workload'а × 4 инструмента). 6 будут `completed`, 10 `n/a`
(см. capability matrix).

## 4. Методология

> **Полная methodology:** [`benchmarks/whitepaper-2026/METHODOLOGY.md`](../../benchmarks/whitepaper-2026/METHODOLOGY.md).
> **Runbook для воспроизведения:** [`benchmarks/whitepaper-2026/docs/benchmark-runbook.md`](../../benchmarks/whitepaper-2026/docs/benchmark-runbook.md).

Краткая версия (per Issue #106 spec):

- **Одно железо**, **одна сеть** (loopback), **один receiver** (Python byte-counter).
- **Одинаковая длительность**: 30s default.
- **Одинаковый target rate**: per Issue #106 spec; achieved rate 95–105%.
- **Одинаковый размер сообщения**: 256B / 1KB; допуск ±5%.
- **Measurement window = sender runtime** (без warmup, без time subtraction).
- **Без параллелизма**: ячейки запускаются последовательно.
- **Без фабрикации**: invariant `no_fabricated_measurements: true`,
  проверяется в `harness/test_collect.py`.

## 5. CLI (verified)

- `loggen -i -D -s SIZE -r RATE -I SECONDS HOST PORT` (syslog-ng loggen, UDP only)
- `flog -f syslog -n COUNT -o stdout` (нет network support → N/A для всех network workloads)
- `tcpkali -T SECS --message-rate RATE -c 1 -f MESSAGE_FILE HOST:PORT`;
  `--ssl --cert --key` для TLS; port positional (НЕ -P)
- `syslog-generator -p CONFIG --duration N` (native UDP/TCP/TLS)

## 6. Как воспроизвести

```bash
git clone https://github.com/pharmacolog/syslog-generator
cd syslog-generator
git checkout <commit-sha>  # tag v11.6.0 после Issue #191

# Dry-run (всегда успех, инструменты не нужны):
make -C benchmarks/whitepaper-2026 all

# Реальный прогон (нужны syslog-generator, loggen (syslog-ng), flog, tcpkali):
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1

# Полный runbook (Issue #196, c5.2xlarge, 3-4 часа):
# см. benchmarks/whitepaper-2026/docs/benchmark-runbook.md
```

## 7. EXTERNAL PENDING — вне scope этого issue

- ❌ **Habr-публикация** (RU, этот драфт) — `EXTERNAL PENDING` (Issue #199).
- ❌ **dev.to-публикация** (EN) — `EXTERNAL PENDING` (Issue #199).
- ❌ **Telegram-анонсы** (DevOps Moscow, SRE Russia, Rust Russia) — `EXTERNAL PENDING`.
- ❌ **Twitter/X thread** — `EXTERNAL PENDING`.
- ❌ **Reddit r/rust, r/sysadmin** — `EXTERNAL PENDING`.
- ❌ **Hacker News** — `EXTERNAL PENDING`.
- ❌ **Цитирования / backlinks ≥ 5 / 3 мес** (Issue #200) — `EXTERNAL PENDING`.
- ❌ **GitHub stars ≥ 50 через UTM** (Issue #200) — `EXTERNAL PENDING`.
- ❌ **Containerized Dockerfiles** для всех 4 инструментов — `EXTERNAL PENDING`.

## 8. Предыстория

`syslog-generator` — Rust-native syslog-генератор, заточенный под
высокую throughput. Изначально писался для нагрузочного тестирования
rsyslog/syslog-ng в CI. Когда новые пользователи спрашивают "а как он
по сравнению с loggen?", честный ответ — "не знаем, надо правильно
измерить". Этот whitepaper — результат этого измерения.

## 9. Call to action

- Если инструмент полезен — **star** на GitHub:
  [https://github.com/pharmacolog/syslog-generator](https://github.com/pharmacolog/syslog-generator)
  (UTM-tagged для трекинга цитирований: `?utm_campaign=whitepaper-2026`).
- Сcылайтесь на этот whitepaper в профильных статьях / RFC proposals
  / benchmark'ах. Backlinks tracking —
  [`docs/whitepaper-2026-tracking.md`](whitepaper-2026-tracking.md).
- Habr permalink: **TBD** (Issue #199, после публикации).
- dev.to permalink: **TBD** (Issue #199, после публикации).
- Feedback: открыть issue или PR на [`docs/whitepaper-2026.ru.md`](whitepaper-2026.ru.md).

## 10. Лицензия и атрибуция

- Код: Apache-2.0 (как и `syslog-generator`).
- Benchmark harness: `benchmarks/whitepaper-2026/`.
- Methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.
- Runbook: `benchmarks/whitepaper-2026/docs/benchmark-runbook.md`.
- Raw data: `perf/whitepaper-results.json`.
- Сравниваемые инструменты: `loggen` (GPLv2, syslog-ng project), `flog`
  (MIT, mingrammer), `tcpkali` (Apache-2.0, machinezone), `syslog-generator`
  (Apache-2.0, pharmacolog).
