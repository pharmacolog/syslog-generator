# Сравнительный benchmark генераторов нагрузки для syslog

> **RU-драфт** для Habr.
> **Issue:** [#106](https://github.com/pharmacolog/syslog-generator/issues/106)
> **Статус:** Черновик. Численных результатов **НЕТ** — см. `perf/whitepaper-results.json::status` (текущее `schema_only`).
> **EXTERNAL PENDING:** Habr-публикация, dev.to-публикация, Telegram-анонсы, Twitter/X thread, Reddit/HN, цитирования, UTM-звёзды — **ни одно из этих действий не выполняется в этом PR**. Это post-benchmark marketing-amplification, отдельные задачи.

## TL;DR

Это **черновик** Whitepaper 2026. Цифр в нём **намеренно нет** — они
появятся после реального прогона и попадут в `perf/whitepaper-results.json`.
Harness сравнивает **4 инструмента** из Issue #106: `syslog-generator` vs
`loggen` (syslog-ng) vs `flog` vs `tcpkali`. kcat (kafkacat) **НЕ** входит
в сравниваемый набор — в v1 нет рабочего Kafka consumer'а, и для всех
4 tools Kafka cells помечены `N/A`.

Здесь:

1. **Зачем** нам нужен честный benchmark.
2. **Что** мы будем мерить (4 workload'а × 4 инструмента).
3. **Как** мы будем мерить (fairness protocol, methodology).
4. **Как** будем публиковать (Habr + dev.to, в обеих локалях).

## 1. Зачем

Когда гуглишь «syslog load generator», выпадает длинный список. Каждый
инструмент в README пишет "high throughput" или "low latency". Но
**claims несравнимы**.

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

16 ячеек (4 workload'а × 4 инструмента).

## 4. Методология (кратко)

- **Одно железо**, **одна сеть** (loopback), **один receiver** (Python byte-counter).
- **Одинаковая длительность**: 30s default.
- **Одинаковый target rate**: per Issue #106 spec; achieved rate должен быть 95–105% target.
- **Одинаковый размер сообщения**: 256B / 1KB; допуск ±5%.
- **Measurement window = sender runtime** (без warmup, без time subtraction).
- **Без параллелизма**: ячейки запускаются последовательно.
- **Без фабрикации**: `perf/whitepaper-results.json` имеет invariant
  `no_fabricated_measurements: true`, проверяется в `harness/test_collect.py`.

Полная methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.

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
git checkout <commit-sha>

# Dry-run (всегда успех, инструменты не нужны):
make -C benchmarks/whitepaper-2026 all

# Реальный прогон (нужны syslog-generator, loggen (syslog-ng), flog, tcpkali):
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1
```

## 7. EXTERNAL PENDING — вне scope этого issue

Следующие пункты Issue #106 **явно вне scope** harness-PR и помечены
`EXTERNAL PENDING` (отслеживаются отдельными задачами):

- ❌ **Habr-публикация** (RU, этот драфт) — `EXTERNAL PENDING`.
- ❌ **dev.to-публикация** (EN) — `EXTERNAL PENDING`.
- ❌ **Telegram-анонсы** (DevOps Moscow, SRE Russia, Rust Russia) — `EXTERNAL PENDING`.
- ❌ **Twitter/X thread** — `EXTERNAL PENDING`.
- ❌ **Reddit r/rust, r/sysadmin** — `EXTERNAL PENDING`.
- ❌ **Hacker News** — `EXTERNAL PENDING`.
- ❌ **Цитирования / backlinks ≥ 5 / 3 мес** (acceptance-метрика Issue #106) — `EXTERNAL PENDING`.
- ❌ **GitHub stars ≥ 50 через UTM** (acceptance-метрика) — `EXTERNAL PENDING`.
- ❌ **Containerized Dockerfiles** для всех 4 инструментов — `EXTERNAL PENDING`.

## 8. Чего в драфте НЕТ

- **Никаких численных throughput / latency.** Они появятся после реального
  прогона. Сейчас schema = `status: "schema_only"`.
- **Никаких marketing-claims** типа "X быстрее Y". Данных нет.
- **Никаких адаптированных vendor benchmarks.** Все цифры — от нашего
  harness'а, на одной документированной машине.

## 9. Предыстория

`syslog-generator` — Rust-native syslog-генератор, заточенный под
высокую throughput. Изначально писался для нагрузочного тестирования
rsyslog/syslog-ng в CI. Когда новые пользователи спрашивают "а как он
по сравнению с loggen?", честный ответ — "не знаем, надо правильно
измерить". Этот whitepaper — результат этого измерения.

## 10. Лицензия и атрибуция

- Код: Apache-2.0 (как и `syslog-generator`).
- Benchmark harness: `benchmarks/whitepaper-2026/`.
- Methodology: `benchmarks/whitepaper-2026/METHODOLOGY.md`.
- Raw data: `perf/whitepaper-results.json`.
- Сравниваемые инструменты: `loggen` (GPLv2, syslog-ng project), `flog`
  (MIT, mingrammer), `tcpkali` (Apache-2.0, machinezone), `syslog-generator`
  (Apache-2.0, pharmacolog).
