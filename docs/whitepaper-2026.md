# Whitepaper 2026 — Сравнительный benchmark syslog-генераторов

> **Issue:** #106 (GTM-1, milestone v11.6)
> **Статус:** Harness + methodology готовы. Замеры не выполнялись.
> **Дата:** 2026-07-25
> **Версия harness:** 1.0.0

## TL;DR

`benchmarks/whitepaper-2026/` содержит reproducible harness для сравнения
**4 инструментов** из Issue #106: `syslog-generator` vs `loggen` vs `flog`
vs `tcpkali`. kcat (kafkacat) **НЕ** входит в сравниваемый набор — это
consumer-side зависимость, и в v1 нет ни одного рабочего Kafka consumer'а
(для всех 4 tools Kafka cells помечены `N/A`).

Из коробки:

1. **Reproducible harness** (`benchmarks/whitepaper-2026/`) — Makefile + shell/Python
   скрипты, которые умеют запускать, валидировать, собирать и репортить
   результаты для 4 сравниваемых инструментов.
2. **Dry-run по умолчанию** — `make all` без установленных внешних утилит
   **всегда завершается с documented exit status (0 + warnings)**, не маскируя
   отсутствие замеров. Для реального прогона — `make all REQUIRE_TOOLS=1`.
3. **Schema/status artifact** (`perf/whitepaper-results.json`) — структурированный
   вывод harness'а. В текущем коммите содержит только schema + status="schema_only",
   никаких fabricated measurements.
4. **Methodology** — `benchmarks/whitepaper-2026/METHODOLOGY.md` описывает
   fairness-протокол, reproducibility contract и метрики.
5. **Harness self-tests** — `benchmarks/whitepaper-2026/harness/` запускает
   проверки логики harness на fixtures'ах, не требуя самих инструментов.

## Сравниваемые инструменты (4) и их реальные возможности

(Основано на подтверждённом research, не на предположениях.)

| Tool | Source | UDP | TCP | TLS | Kafka | Сетевой вывод | Размер |
|---|---|---|---|---|---|---|---|
| `syslog-generator` | этот репозиторий | ✓ | ✓ | ✓ | **N/A** (нет real consumer) | native | настраиваемый (template) |
| `loggen` | **syslog-ng** (НЕ rsyslog) | ✓ | **N/A** (UDP-only) | **N/A** (UDP-only) | **N/A** | native | -s SIZE |
| `flog` | mingrammer v0.4.3 | **N/A** | **N/A** | **N/A** | **N/A** | stdout/file only | fixed by log type |
| `tcpkali` | machinezone | **N/A** | ✓ | ✓ | **N/A** | native | -f FILE (pre-generated) |

CLI формы (verified):

- `loggen -i -D -s SIZE -r RATE -I SECONDS HOST PORT` (syslog-ng loggen, UDP only)
- `flog -f syslog -n COUNT -o stdout` (no network support → N/A for all network workloads)
- `tcpkali -T SECS --message-rate RATE -c 1 -f MESSAGE_FILE HOST:PORT`;
  `--ssl --cert --key` для TLS; port as positional (NO -P flag)

## Capability matrix (4 tools × 4 transports)

| | syslog_generator | loggen | flog | tcpkali |
|---|---|---|---|---|
| UDP 100 msg/s 256B | supported | supported | **N/A** | **N/A** |
| TCP 10k msg/s 1KB | supported | **N/A** (UDP-only) | **N/A** | supported |
| TLS 5k msg/s 1KB | supported | **N/A** (UDP-only) | **N/A** | supported |
| Kafka 50k msg/s 256B | **N/A** (no real consumer) | **N/A** | **N/A** | **N/A** |

## Чего здесь НЕТ (явно)

### В scope этого issue (что в PR)

| Пункт Issue #106 | Статус |
|---|---|
| Методология (4 workload'а × 4 compared tools, fairness protocol) | Готово |
| Воспроизводимый harness (Makefile, scripts, configs) | Готово |
| `perf/whitepaper-results.json` schema/status artifact | Готово (status="schema_only") |
| Harness self-tests (без необходимости в инструментах) | Готово |
| Documentation (RU + EN drafts) | Готово |
| **Численные результаты** (throughput, latency, CPU/memory) | **НЕ В ЭТОМ PR** — требует живой прогон с `REQUIRE_TOOLS=1` |

### ВНЕ scope этого issue — `EXTERNAL PENDING`

- ❌ **Habr-публикация** (RU) — `EXTERNAL PENDING`.
- ❌ **dev.to-публикация** (EN) — `EXTERNAL PENDING`.
- ❌ **Telegram-анонсы** (DevOps Moscow, SRE Russia, Rust Russia) — `EXTERNAL PENDING`.
- ❌ **Twitter/X thread** — `EXTERNAL PENDING`.
- ❌ **Reddit r/rust, r/sysadmin** — `EXTERNAL PENDING`.
- ❌ **Hacker News** — `EXTERNAL PENDING`.
- ❌ **Цитирования / backlinks** — `EXTERNAL PENDING`. Acceptance-метрика Issue #106.
- ❌ **GitHub stars via UTM** — `EXTERNAL PENDING`. Acceptance-метрика.
- ❌ **Containerized Dockerfiles** для всех 4 инструментов — `EXTERNAL PENDING`.
  Ранние версии содержали Dockerfile'ы с guessed version pins; они удалены.
  См. `benchmarks/whitepaper-2026/docker/README.md`.
- ❌ **kafka_consumer** — `EXTERNAL PENDING`. Реальный consumer требует
  librdkafka; harness receiver — TCP liveness probe only.

## Структура

```
benchmarks/whitepaper-2026/
├── Makefile                       # entrypoint: all / dry-run / validate / report
├── README.md                      # quickstart
├── METHODOLOGY.md                 # fairness protocol + reproducibility contract
├── configs/                       # 4 workload definitions (Issue #106 spec)
│   ├── env.sh
│   ├── validate.py
│   └── workload_{udp,tcp,tls,kafka}_*.json
├── scripts/                       # shell/Python (stdlib-only)
│   ├── 01_check_tools.sh
│   ├── 02_dry_run.sh
│   ├── 03_dispatch.sh
│   ├── 03_run_{syslog_generator,loggen,flog,tcpkali}.sh
│   ├── 04_collect.py
│   ├── 05_report.py
│   ├── measure.py                 # shared fairness validation
│   ├── receiver.py                # shared Python byte-counter
│   ├── lib.sh
│   └── reset_schema.py
├── docker/                        # gen-cert.sh + README only (no Dockerfile'ы)
├── harness/                       # self-tests
└── results/                       # raw output каждого прогона (.gitkeep only)

docs/
├── whitepaper-2026.md             # этот файл
├── whitepaper-2026.en.md          # EN draft
└── whitepaper-2026.ru.md          # RU draft

perf/
└── whitepaper-results.json        # schema/status artifact (status="schema_only")
```

## Как воспроизвести

```bash
# Dry-run (всегда exit 0, не запускает ничего трудоёмкого):
make -C benchmarks/whitepaper-2026 all

# Валидация конфигов + harness self-tests:
make -C benchmarks/whitepaper-2026 validate
make -C benchmarks/whitepaper-2026 harness-tests

# Реальный прогон (требует установленных инструментов):
#   - syslog-generator: cargo build --release
#   - loggen:           apt install syslog-ng-core  (syslog-ng, НЕ rsyslog)
#   - flog:             go install github.com/mingrammer/flog@latest
#   - tcpkali:          https://github.com/machinezone/tcpkali/releases
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1

# Опционально: задать пользовательский rate для pacing теста.
# syslog-generator --rate — soft cap, фактический rate может быть выше
# target. Для строгого 95–105% gate используйте HARNESS_RATE, найденный
# эмпирически (например 95 для target=100):
make -C benchmarks/whitepaper-2026 all REQUIRE_TOOLS=1 HARNESS_RATE=95
```

## Fairness протокол (короткая версия)

Полный текст — `benchmarks/whitepaper-2026/METHODOLOGY.md`.

- **Hardware lock:** задокументировать `uname -a`, `sysctl hw.ncpu`, `sysctl hw.memsize`.
- **Same network:** все 4 workload'а шлют на `127.0.0.1` (loopback).
- **Same receiver:** `benchmarks/whitepaper-2026/scripts/receiver.py` (Python byte-counter).
- **Same duration:** `DURATION_SECS` (default 30s).
- **Same message size:** 256B / 1KB per Issue #106 spec. ±5% tolerance.
- **Same target rate:** rate-limited cells must achieve 95–105% of target
  (configurable via `RATE_TOLERANCE_FRACTION`; short tests may use larger).
- **Measurement window:** фактический sender runtime / receiver duration.
  No time subtraction; no warmup. `WARMUP_SECS=0` only.
- **No parallelism:** cells run sequentially.
- **No fabricated measurements:** invariant enforced by `harness/test_collect.py`.

## Exit code policy

| Ситуация | Exit code |
|---|---|
| `make all` (dry-run) | 0 |
| `make all REQUIRE_TOOLS=1`, все supported cells completed | 0 |
| `make all REQUIRE_TOOLS=1`, supported cell skipped (tool missing) | 2 |
| `make all REQUIRE_TOOLS=1`, supported cell failed (rate out of band) | 1 |
| Unsupported workload/tool combination | `n/a` (не считается failure) |

## Где смотреть

| Что | Где |
|---|---|
| Методология | `benchmarks/whitepaper-2026/METHODOLOGY.md` |
| Конфиги workload'ов | `benchmarks/whitepaper-2026/configs/workload_*.json` |
| Скрипты | `benchmarks/whitepaper-2026/scripts/` |
| Self-tests harness'а | `benchmarks/whitepaper-2026/harness/test_*.{sh,py}` |
| Schema/status artifact | `perf/whitepaper-results.json` |
| EN draft | `docs/whitepaper-2026.en.md` |
| RU draft | `docs/whitepaper-2026.ru.md` |
| TLS cert generation | `benchmarks/whitepaper-2026/docker/gen-cert.sh` |

## Связанные Issue / PR

- #106 — этот issue (GTM-1 whitepaper)
- #84 — perf metrics gate
- #87 — baseline
- #107 — .deb/.rpm (GTM-2)

## Известные ограничения (v1)

1. **External tools могут быть недоступны** — harness это
   прозрачно отражает в `tool_versions[*].available = false`. `REQUIRE_TOOLS=1`
   с любым отсутствующим required tool выходит с кодом 2.
2. **TLS workload** требует self-signed сертификат. Запустить
   `benchmarks/whitepaper-2026/docker/gen-cert.sh` для генерации.
3. **Kafka workload** — все 4 tools = `N/A` (нет real consumer).
4. **Latency p50/p95/p99** — только для syslog-generator (через Prometheus
   exporter). Для external tools latency = `null`.
5. **No fabricated measurements.** Issue #106 явно требует честности.
   Invariant `no_fabricated_measurements` enforced в `harness/test_collect.py`.
6. **External pending items** (см. "ВНЕ scope этого issue" выше) —
   маркетинговые публикации, цитирования, UTM-звёзды, containerized
   Dockerfiles, Kafka consumer НЕ выполняются этим PR.
7. **syslog-generator `--rate` — soft cap.** Фактический rate может быть
   выше target. Используйте `HARNESS_RATE` для pacing (например,
   `HARNESS_RATE=95` для target=100 даёт actual ~103 msg/s, в 95–105% gate).
8. **No "fully containerized" claim.** Dockerfile'ы с guessed versions
   были удалены. См. `benchmarks/whitepaper-2026/docker/README.md`.
9. **flog N/A для network workloads** — flog v0.4.3 не имеет network
   outputs и fixed record size. Не пытаемся обойти это (pipe через nc
   не удовлетворяет fairness protocol).
10. **loggen — syslog-ng** (НЕ rsyslog). CLI подтверждён:
    `loggen -i -D|-S [-P] -s SIZE -r RATE -I SECONDS HOST PORT`. UDP only.
11. **tcpkali --message-rate** (NOT -r, which is connection rate). Payload
    via pre-generated file (-f), NOT -m SIZE. One connection (-c 1).
12. **No warmup** в v1. Window = sender runtime directly.
