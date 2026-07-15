# syslog-generator

[![CI](https://github.com/pharmacolog/syslog-generator/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pharmacolog/syslog-generator/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pharmacolog/syslog-generator?sort=semver)](https://github.com/pharmacolog/syslog-generator/releases)
[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue)](https://github.com/pharmacolog/syslog-generator/blob/main/Cargo.toml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/syslog-generator)](https://crates.io/crates/syslog-generator)
[![docs.rs](https://img.shields.io/docsrs/syslog-generator)](https://docs.rs/syslog-generator)
[![codecov](https://img.shields.io/codecov/c/github/pharmacolog/syslog-generator?label=coverage&token=)](https://codecov.io/gh/pharmacolog/syslog-generator)
[![Security Audit](https://img.shields.io/badge/security%20audit-cargo--deny%20passing-brightgreen)](https://github.com/pharmacolog/syslog-generator/actions/workflows/ci.yml)
[![Fuzzing](https://img.shields.io/badge/fuzzing-cargo--fuzz%205%20targets-blueviolet)](https://github.com/pharmacolog/syslog-generator/tree/main/fuzz)
[![Docker](https://img.shields.io/badge/docker-multi--arch%20%28amd64%2Barm64%29-blue)](https://github.com/pharmacolog/syslog-generator/pkgs/container/syslog-generator)
[![Release Date](https://img.shields.io/github/release-date/pharmacolog/syslog-generator)](https://github.com/pharmacolog/syslog-generator/releases)
[![Last commit](https://img.shields.io/github/last-commit/pharmacolog/syslog-generator/main)](https://github.com/pharmacolog/syslog-generator/commits/main)

**Промышленный генератор нагрузки для syslog-серверов на Rust.**
Высокопроизводительный, многопротокольный (file/TCP/UDP/TLS/Kafka),
с детальным контролем payload'а (faker, regex, межполевые корреляции),
профилями нагрузки во времени и метриками Prometheus.

---

## ✨ Возможности

### 🚀 Производительность
- **Zero-copy** в hot path: `BytesMut` (8 KiB) для TCP/TLS, `BufWriter` для файла
- **LTO + codegen-units=1** в release (5-15% throughput gain)
- **Hot-path оптимизации**: `String::with_capacity(N)` + `write!()` для faker-генераторов
- **Pre-resolve templates/schema** per phase (PhaseContext): -30-50% syscalls
- **Throughput**: 100k msg/s на одной ноде (10 KB payload, TCP)

### 🔌 Транспорты (5 типов)
- **file** — atomic append через `O_APPEND` + `BufWriter`
- **tcp** / **udp** — async с фреймингом RFC 6587 (non-transparent / octet-counting)
- **tls** — `rustls 0.23` (pure Rust), проверка сертификата по умолчанию, mTLS, cipher policy
- **kafka** (opt-in feature) — `rskafka 0.6` с compression gzip/lz4/snappy/zstd
- **File rotation** (size/time/max_files, LRU cleanup)
- **Exponential backoff reconnect** с jitter

### 📝 Форматы (7 типов)
- **RFC 5424** (default) — полный syslog с PRI/TIMESTAMP/HOSTNAME/APP-NAME/PROCID/MSGID/SD
- **RFC 3164** (BSD legacy) — `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`
- **raw** — passthrough без обёртки
- **protobuf** — настоящий wire-format (varint + length-delimited)
- **CEF** (ArcSight SIEM), **LEEF** (IBM QRadar), **JSON-lines** (NDJSON)

### 🎲 Payload
- **Faker-токены**: ipv4/ipv6/mac/uuid/hostname/username/user_agent/url/http_status
- **Schema fields**: int (min..max), enum (uniform/weighted/zipf), string (len),
  datetime (jitter), regex-генерация
- **Межполевые корреляции**: `depends_on` + `mapping` + `mapping_default`
- **Seed-детерминизм**: один `seed` даёт одинаковый вывод (inter-process reproducible)

### 📊 Наблюдаемость
- **HTTP `/metrics`** (Prometheus text exposition v0.0.4) на лёгком tokio-сервере
- **24+ метрик**: counters/histograms/gauges по (transport, phase, target, format)
- **Structured logging** через `tracing` (env-filter, RUST_LOG support)
- **Progress bar** `indicatif` (только при duration > 30s И TTY)
- **Double Ctrl-C** = hard shutdown (per-process counter)

### 🧪 Профили нагрузки (F3)
- `constant` / `linear` (ramp) / `sine` / `burst` (спайки)
- **Аномалии (F17)**: `burst-injection` (×M), `slow-drip` (÷D), `packet-loss` (дроп %)
- **Multi-template** с весами
- **Multi-target** dispatch: `broadcast` / `round-robin` / `weighted`
- **Connection pool**: `connections: N` воркеров на target

### 🛡️ Безопасность (N4)
- **TLS проверка сертификата по умолчанию** (`tls_insecure: false`)
- **mTLS**: client identity через `tls_client_cert_file`/`tls_client_key_file`
- **Cipher policy**: IANA-имена через `tls_cipher_suites`
- **Min TLS version**: 1.2 / 1.3 через `tls_min_protocol_version`
- **Fail-fast validation**: F13 (семантическая) + D3 (JSON Schema)

---

## 🚀 Quick start

```bash
# Из исходников (release, ~3 мин компиляция)
git clone https://github.com/pharmacolog/syslog-generator.git
cd syslog-generator
cargo build --release

# Генерация 1000 сообщений на syslog-сервер
./target/release/syslog-generator -t 127.0.0.1:514:udp -m 'event {{sequence}}' --rate 100 --total 1000

# Запуск готового примера
./target/release/syslog-generator --profile examples/multi_target_roundrobin.yaml

# Docker (multi-arch, ~25 MB image)
docker run --rm ghcr.io/pharmacolog/syslog-generator:v10.7.9 --version
```

---

## 📦 Установка

### Из исходников
```bash
# Стабильный Rust toolchain (≥ 1.95)
rustup install stable

# Сборка (debug быстрее, release быстрее в runtime)
cargo build                                    # debug
cargo build --release                          # release с LTO

# С Kafka/Redpanda support (opt-in)
cargo build --release --features kafka

# С дополнительными test-helpers
cargo build --release --features test-helpers
```

### Docker
```bash
# Pre-built images (multi-arch: linux/amd64, linux/arm64)
docker pull ghcr.io/pharmacolog/syslog-generator:v10.7.9

# Запуск
docker run --rm \
  -v $PWD/examples:/examples:ro \
  ghcr.io/pharmacolog/syslog-generator:v10.7.9 \
  --profile /examples/multi_target_roundrobin.json

# Полный стек (syslog-generator + syslog-ng + Prometheus + Grafana)
docker compose up
```

### Из исходников (development)
```bash
git clone https://github.com/pharmacolog/syslog-generator
cd syslog-generator
cargo install --path .   # установит в ~/.cargo/bin/syslog-generator
```

---

## 🛠️ CLI

```text
syslog-generator [OPTIONS] --profile <FILE>
syslog-generator [OPTIONS] -t <ADDR[:TRANSPORT]> -m <TEMPLATE> --rate <N> --total <N>
syslog-generator completions <bash|zsh|fish|powershell|elvish>
syslog-generator man
```

### Основные флаги

| Флаг | Описание |
|------|----------|
| `-p, --profile <FILE>` | JSON/YAML профиль нагрузки |
| `-t, --target <ADDR[:TRANSPORT]>` | Цель (повторяемый; TRANSPORT: `tcp`/`udp`/`tls`/`file`/`kafka`) |
| `--transport <T>` | Transport override (v10.1.0+, deprecated `ADDR:TRANSPORT` alias до v11.0.0) |
| `--distribution <D>` | `round-robin` / `broadcast` / `weighted` |
| `--rate <N>` | messages_per_second |
| `--duration <SEC>` | duration фазы |
| `--total <N>` | total_messages фазы |
| `--format <F>` | `rfc5424` / `rfc3164` / `raw` / `protobuf` / `cef` / `leef` / `json_lines` |
| `--seed <N>` | RNG seed (детерминизм) |
| `-m, --message <TPL>` | Inline template (быстрый режим без файла-профиля) |
| `--validate` | Только проверить профиль (dry-run) |
| `--schema-strict` | С JSON Schema validation (вместе с `--validate`) |
| `--print-config` | Вывести эффективный профиль и выйти |
| `--dry-run` | План без отправки (v10.7.0+) |
| `--metrics-addr <ADDR>` | HTTP `/metrics` endpoint |
| `--version` | Версия |
| `--help` | Помощь |

### Коды возврата
- `0` — успех / профиль валиден
- `1` — ошибка чтения, парсинга или валидации
- `2` — hard shutdown (двойной Ctrl-C / SIGTERM)

### Примеры

```bash
# 1. UDP на localhost с inline template
syslog-generator -t 127.0.0.1:514:udp -m 'evt {{sequence}}' --rate 100 --total 1000 --seed 42

# 2. Файл с rotation
syslog-generator -t /tmp/out.log:file -m 'line {{sequence}}' --total 10000 --format raw

# 3. TLS с самоподписанным CA
syslog-generator --profile examples/tls_with_ca.json

# 4. Multi-target round-robin с метриками
syslog-generator --profile examples/multi_target_roundrobin.json --metrics-addr 0.0.0.0:9090
curl http://127.0.0.1:9090/metrics

# 5. Shell completions (bash)
syslog-generator completions bash > /etc/bash_completion.d/syslog-generator
```

---

## 📄 Формат профиля

Полная схема: [`schemas/profile.schema.json`](schemas/profile.schema.json).
Минимальный пример:

```json
{
  "targets": [
    {
      "address": "127.0.0.1:514",
      "transport": "udp",
      "format": "rfc5424",
      "framing": "non-transparent"
    }
  ],
  "phases": [
    {
      "name": "warmup",
      "duration_secs": 10,
      "messages_per_second": 100,
      "templates": [
        "<165>1 {{timestamp}} {{hostname}} {{real_app}}[{{pid}}]: event {{sequence}}"
      ],
      "syslog": {
        "facility": 16,
        "severity": 6
      }
    }
  ]
}
```

YAML формат эквивалентен. Подробнее: [docs/USER_GUIDE.md](docs/USER_GUIDE.md).

### Плейсхолдеры

| `{{placeholder}}` | Источник |
|-------------------|----------|
| `{{sequence}}` | 1..N (порядковый номер) |
| `{{timestamp}}` | RFC3339 UTC |
| `{{pid}}` | 1..65535 |
| `{{faker.ipv4}}` | Валидный IPv4 |
| `{{faker.uuid}}` | UUID v4 |
| `{{faker.username}}` | Случайный username |
| `{{faker.hostname}}` | Случайный hostname |
| `{{faker.user_agent}}` | User-Agent |
| `{{faker.url}}` | URL |
| `{{faker.http_status}}` | HTTP status code |
| `{{faker.ipv6}}`, `{{faker.mac}}` | IPv6 / MAC |

**Всего 9 faker-типов + sequence/timestamp/pid/hostname/app.**

---

## 🏗️ Архитектура

```
src/
├── cli/             # clap derive Args, subcommands
├── config/          # Profile, Phase, TargetConfig (loadable из JSON/YAML)
├── format/          # 7 форматов через Format trait
│   ├── rfc5424.rs
│   ├── rfc3164.rs
│   ├── raw.rs
│   ├── protobuf.rs
│   ├── cef.rs / leef.rs / json_lines.rs
│   └── mod.rs       # Format trait, FormatKind enum, FormatContext
├── transport/       # 5 транспортов через Transport trait
│   ├── tcp.rs / udp.rs / tls.rs / file.rs
│   ├── kafka.rs     (feature=kafka)
│   ├── reconnect.rs  (exponential backoff)
│   └── mod.rs       # Transport trait, TransportKind
├── observability/   # Prometheus + HTTP /metrics
│   ├── metrics.rs
│   └── server.rs
├── generator/       # orchestrator
│   ├── core.rs      # run_profile, run_phase_multi, PhaseContext
│   └── config.rs
├── payload.rs       # F4–F6, F14: faker, regex, корреляции, distribution
├── template.rs      # N5: CompiledTemplate (one-pass parser)
├── schema.rs        # F5: schema-per-phase загрузка
├── load_shape.rs    # F3: профили нагрузки (constant/linear/sine/burst)
├── anomaly.rs       # F17: AnomalyKind (BurstInjection/SlowDrip/PacketLoss)
├── shutdown.rs      # N7 + PR-2: SIGINT/SIGTERM graceful drain
├── error.rs         # N7: типизированные ошибки (RuntimeError + подтипы)
├── validate.rs      # F13: семантическая валидация (43 варианта)
└── lib.rs           # pub use re-exports + backward-compat алиасы
```

Слои строго разделены по dependency direction:
- `format/` → только `config::*` (leaf types) и `chrono`
- `transport/` → только `observability::Metrics`, `error::*`
- `observability/` → только `error::*`
- `generator/` → оркестратор всех слоёв

**Подробнее:** [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md) (v10.7.4).

---

## 📊 Производительность

Бенчмарки на M1 Pro 8-core, macOS 14 (release build с LTO):

| Workload | CPU | Memory | Throughput |
|----------|-----|--------|------------|
| UDP 127.0.0.1:514, 100 msg/s, 256 B | 0.5% | 8 MB | 100 msg/s stable |
| TCP 127.0.0.1:514, 10k msg/s, 1 KiB | 25% | 15 MB | 10k msg/s stable |
| TLS 127.0.0.1:6514, 5k msg/s, 1 KiB | 35% | 25 MB | 5k msg/s stable |
| File /tmp/out.log, 50k msg/s, 256 B | 15% | 20 MB | 50k msg/s |

**Per-message overhead** (target ≤ 2 µs/msg в v10.7.10+):
- `generate_message_from_template`: 5.17 µs (v10.2.0 baseline)
- Zero allocations в hot path (N6 v8.7.0: BytesMut + BufWriter)
- Static dispatch через `FormatKind`/`TransportKind` enums (no vtable lookups)

```bash
# Запуск бенчмарков
cargo bench --bench message_generation -- --quick
cargo bench --bench sender_throughput -- --quick
cargo bench --bench format_cef -- --quick
cargo bench --bench transport_tls -- --quick
```

Подробнее: [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

---

## 🛡️ Безопасность

### TLS по умолчанию

```text
tls_insecure: false  (default — проверка сертификата + hostname)
tls_insecure: true   (opt-in — WARNING в stderr, отключает verify)
```

| Поле | Назначение |
|------|-----------|
| `tls_domain` | SNI + hostname verification (default: host часть `address`) |
| `tls_ca_file` | PEM доверенного CA (для self-signed) |
| `tls_insecure` | `true` отключает verify |
| `tls_client_cert_file` + `tls_client_key_file` | mTLS client identity |
| `tls_min_protocol_version` | `"1.2"` / `"1.3"` (anti-downgrade) |
| `tls_cipher_suites` | IANA-имена (8 поддерживаемых) |

### Secure SDLC (SSDLC) practices

- ✅ **Fail-fast validation** (F13): профиль проверяется до запуска (collect-all errors)
- ✅ **Type-safe errors** (N7): `RuntimeError` через `thiserror`, пробрасывание через `?`
- ✅ **No `.unwrap()`/`.expect()`** в runtime коде (verified через grep в CI)
- ✅ **`deny(unsafe_code)`** на crate level (PR-4)
- ✅ **cargo-deny**: blocking gate для security advisories + license compliance
- ✅ **cargo-machete**: blocking gate для unused dependencies
- ✅ **MSRV check**: blocking в CI (rust-toolchain.toml)
- ✅ **Dependabot**: еженедельные PR для dependencies + GitHub Actions
- ✅ **cargo-fuzz**: 5 таргетов для format/profile parsing
- ✅ **Property-based tests** (N8): 6 proptest тестов для payload invariants

См. [SECURITY.md](SECURITY.md) для vulnerability disclosure policy.

---

## 🧪 Качество кода

| Метрика | Значение |
|---------|----------|
| Тесты | 339 (242 unit + 86 integration + 11 N7) |
| Coverage | ≥ 97% lines (PR-11 target) |
| Clippy | `-D warnings` strict |
| Format | `cargo fmt --all --check` strict |
| Fuzz | 5 таргетов (profile + 4 formats) |
| Public API | `cargo-public-api` snapshot в CI |
| Deps | `cargo-deny` + `cargo-machete` blocking |

---

## 🤝 Contributing

См. [CONTRIBUTING.md](CONTRIBUTING.md). Краткий workflow:

```bash
# 1. Fork & clone
git clone https://github.com/YOUR_USERNAME/syslog-generator.git

# 2. Создать feature branch от dev
git checkout dev
git checkout -b feature/your-feature

# 3. Сделать изменения + добавить тесты
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --features test-helpers

# 4. Commit + push + PR в dev
git commit -m "feat: your feature"
git push origin feature/your-feature
gh pr create --base dev

# 5. После зелёного CI на dev → merge → release/v*.*.* → main → tag
```

**Требования к PR:**
- Все Quality Gates зелёные (см. `.github/workflows/ci.yml`)
- Тесты добавлены для новой функциональности
- `CHANGELOG.md` обновлён (секция для нового релиза)
- Документация обновлена при необходимости (`docs/USER_GUIDE.md`)
- Backward-compatible (или breaking changes явно документированы в `CHANGELOG.md`)

---

## 📚 Документация

| Документ | Описание |
|----------|----------|
| [docs/USER_GUIDE.md](docs/USER_GUIDE.md) | Полное руководство пользователя (v10.7.4) |
| [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md) | Архитектура + как добавить свой формат/транспорт (v10.7.4) |
| [docs/MIGRATION.md](docs/MIGRATION.md) | Breaking changes + миграция между версиями (v10.7.4) |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Оптимизации + методика замера (v10.7.4) |
| [docs/COVERAGE.md](docs/COVERAGE.md) | Coverage отчёты (v10.3.0 → v10.4.0 baseline) |
| [docs/FUZZING.md](docs/FUZZING.md) | Инструкции по cargo-fuzz (v10.4.0) |
| [CHANGELOG.md](CHANGELOG.md) | История всех релизов |
| [AUDIT.md](AUDIT.md) | Реестр задач (базис v10.7.2) |
| [CLAUDE_HANDOFF.md](CLAUDE_HANDOFF.md) | Перенос контекста для Claude |
| [SECURITY.md](SECURITY.md) | Vulnerability disclosure policy |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contributing guide |

---

## 📜 Лицензия

Copyright © 2026 Anton E. Gerasimov.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

See [LICENSE](LICENSE) for the full text.

---

## 🙏 Acknowledgments

Built with these amazing crates:
- [`tokio`](https://tokio.rs/) — async runtime
- [`rustls`](https://github.com/rustls/rustls) — TLS implementation
- [`clap`](https://github.com/clap-rs/clap) — CLI parsing
- [`prometheus`](https://github.com/tikv/rust-prometheus) — metrics
- [`governor`](https://github.com/bozaro/rust-governor) — rate-limiting
- [`rskafka`](https://github.com/influxdata/rskafka) — Kafka client
- [`tracing`](https://github.com/tokio-rs/tracing) — structured logging
- [`serde`](https://serde.rs/) — serialization
- [`criterion`](https://github.com/bheisler/criterion.rs) — benchmarking

---

<p align="center">
Сделано с ❤️ для syslog-сообщества
</p>