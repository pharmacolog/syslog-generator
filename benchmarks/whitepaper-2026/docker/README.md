# Docker / compose for Whitepaper 2026 (Issue #106 / GTM-1, Issue #198)

Этот каталог содержит **5 сервисов** для whitepaper harness:

| Сервис | Dockerfile / image | Назначение | Статус |
|---|---|---|---|
| `redpanda` | `redpandadata/redpanda:v24.2.7` | Kafka-совместимый broker для Kafka workload'ов (Issue #197) | tagged |
| `syslog-ng-loggen` | `Dockerfile.syslog-ng-loggen` (`ubuntu:24.04` + `syslog-ng-core`) | `loggen` для UDP workload'ов | tagged (no digest pin) |
| `flog` | `Dockerfile.flog` (`golang:1.22` + `mingrammer/flog@v0.4.3`) | N/A для network cells (см. §3) | tagged (no digest pin) |
| `tcpkali` | `Dockerfile.tcpkali` (`ubuntu:24.04` + build-from-source `MumiaiGene/tcpkali@v2.3.3`) | TCP/TLS workload'ы | **UNVERIFIED** (no commit SHA) |
| `kafka-receiver` | `Dockerfile.kafka-receiver` (`python:3.12-slim` + `kafka-python>=2.0`) | Real Kafka consumer (Issue #197) | tagged (no digest pin) |

Запуск:

```bash
# из корня репозитория:
make docker/up        # build + start 5 сервисов в фоне
make docker/logs      # tail logs
make docker/down      # stop + remove
make docker/exec TOOL=loggen ARGS="--help"
```

После `make docker/up` бинари доступны через `docker compose exec`:

```bash
docker compose -f benchmarks/whitepaper-2026/docker/docker-compose.yml exec syslog-ng-loggen loggen --help
docker compose -f benchmarks/whitepaper-2026/docker/docker-compose.yml exec tcpkali --version
docker compose -f benchmarks/whitepaper-2026/docker/docker-compose.yml exec flog --version
```

Harness использует эти бинари через `LOGGEN_BIN=docker compose exec -T whitepaper-loggen loggen`
(после публикации `make docker/exec-bin-wrapper` target — Issue #198 deferred sub-task).

## Honest disclosure (Issue #198)

> **Этот раздел — primary contract** между maintainer'ом и пользователем
> compose'а. Не пропускайте.

1. **Pinned-by-TAG, NOT by digest.** Все `FROM`/`image:` references
   используют tag (например `ubuntu:24.04`, `redpandadata/redpanda:v24.2.7`).
   Docker Hub manifest digest'ы НЕ pinned, что означает: `docker pull`
   может вернуть новый manifest после security rebuild'а. Для строгого
   pinning замените на `<image>:<tag>@sha256:<digest>` (после `docker
   pull` → `docker images --digests` → копирование digest'а в Dockerfile).
2. **tcpkali Dockerfile помечен `UNVERIFIED`.** Upstream fork
   `MumiaiGene/tcpkali` не проверен лично (см. `Dockerfile.tcpkali`
   Honest disclosure §1). **TODO перед production use:** clone upstream,
   зафиксировать commit SHA, обновить `--branch` → `--branch <sha>`.
   Tracking: [Issue #198](https://github.com/pharmacolog/syslog-generator/issues/198)
   + comments.
3. **flog Dockerfile использует `@v0.4.3` (Go module pseudo-version),**
   не commit SHA. v0.4.3 — последний official release 2021-03; с тех пор
   upstream `mingrammer/flog` не обновлялся. Network-output НЕ поддерживается
   (issue #25 open с 2018). flog cells — N/A в harness (см. ниже).
4. **Только для dev/CI/laptop runs.** Production-grade Kafka → Redpanda
   operator / ansible / helm chart. Этот compose НЕ intended для
   production (нет TLS, нет SASL, нет ACL, single-node test broker).
5. **kafka-receiver build context = `benchmarks/whitepaper-2026/`** (родитель
   `docker/`), чтобы `COPY ../requirements.txt` и `COPY ../scripts/receiver.py`
   работали. Это non-standard pattern; explicit для читаемости.
6. **ULimits / memory / cpu — dev defaults** (Redpanda `--smp=1 --memory=1G`).
   Не scale'ить этот compose для production; для CI достаточно.

## Что есть в каталоге

- `Dockerfile.syslog-ng-loggen` — Ubuntu 24.04 + syslog-ng-core.
- `Dockerfile.flog` — multi-stage Go 1.22 + debian:bookworm-slim.
- `Dockerfile.tcpkali` — **UNVERIFIED** build-from-source.
- `Dockerfile.redpanda` — reference (uses official upstream image, не
  build). Создан для completeness (Issue #198 explicit requirement);
  build context пустой, image — official.
- `Dockerfile.kafka-receiver` — Python 3.12 + kafka-python (Issue #197).
- `docker-compose.yml` — 5 сервисов в bridge network `whitepaper-benchmarks`.
- `gen-cert.sh` — generates self-signed TLS cert for `127.0.0.1` for the
  TLS workload. Не относится к Issue #198, оставлен для backwards-compat.

## Что НЕ делает compose

- **Не запускает benchmarks автоматически.** Это infrastructure layer.
  Запуск workload'ов — через `make all REQUIRE_TOOLS=1 LOGGEN_BIN=$(which loggen) ...`
  с override'нутыми путями или через `docker compose exec`. Полная
  интеграция в Makefile — sub-task Issue #198.
- **Не подменяет хост-benchmarks.** Если вы запускаете `make all`
  нативно, compose НЕ нужен. Compose нужен для environment'а, где
  хост-benchmarks нежелателен (CI, reproduce, Mac без brew).

## История изменений

| Дата | Issue | Что |
|---|---|---|
| 2026-07-24 | (pre #198) | Каталог содержал только `gen-cert.sh` + README с запретом Dockerfile'ов (см. archived PR). |
| 2026-08-04 | #198 | Добавлены 5 Dockerfile'ов (один UNVERIFIED), `docker-compose.yml`, `make docker/up`/`down` target'ы. **Honest disclosure** в каждом Dockerfile и в этом README. |

## Связанные документы

- [benchmarks/whitepaper-2026/METHODOLOGY.md](../METHODOLOGY.md) —
  reproducibility contract, N/A rationale для flog.
- [benchmarks/whitepaper-2026/README.md](../README.md) — общий обзор harness'а.
- [Issue #198](https://github.com/pharmacolog/syslog-generator/issues/198) —
  parent issue для docker image pinning + tcpkali unverified.
- [Issue #197](https://github.com/pharmacolog/syslog-generator/issues/197) —
  Kafka real consumer (зависимость для `kafka-receiver` service).
- [Issue #106](https://github.com/pharmacolog/syslog-generator/issues/106) —
  GTM-1 / whitepaper harness (parent).
