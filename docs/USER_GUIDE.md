# USER GUIDE

Версия документа: `v10.7.4`.

Это полное руководство пользователя по `syslog-generator` — промышленному генератору
нагрузки на syslog-серверы. Документ покрывает все релизы вплоть до v10.7.4
(веха F закрыта на v10.7.1, патч-релизы v10.7.2–v10.7.4 включительно).

---

## 1. Что такое `syslog-generator`

`syslog-generator` — это CLI-инструмент и Rust-библиотека для генерации
**высоконагруженного** syslog-трафика с детальным контролем над:

- форматом сообщений (RFC 5424, RFC 3164, raw, protobuf, CEF, LEEF, JSON-lines);
- транспортом доставки (file, TCP, UDP, TLS/rustls, Kafka/Redpanda);
- нагрузочным профилем во времени (constant/linear/sine/burst + аномалии);
- вариативностью payload (faker-токены, regex-генерация, межполевые корреляции);
- наблюдаемостью (Prometheus metrics через HTTP /metrics, structured logging);
- безопасностью (TLS по умолчанию с проверкой сертификата, mTLS, cipher policy).

Архитектура — модульный async на tokio, реальный TLS handshake через rustls,
zero-copy буферизация, compile-verified releases.

---

## 2. Возможности (по релизам)

### 2.1 Базовые (вехи A/B/C — до v8.0.0)

- **Multi-target профили** с диспетчеризацией `broadcast` / `round-robin` / `weighted`.
- **Транспорты**: `file`, `tcp`, `udp`, `tls` (с проверкой сертификата).
- **Профили нагрузки (F3)**: `constant`, `linear`, `sine`, `burst`.
- **Rate-limiting (F1)**: токен-бакет через `governor`, истинная интенсивность.
- **Connection pool (F2)**: пул воркеров на target через `connections`.
- **Multi-template (F14)**: случайный выбор из массива шаблонов с весами.
- **Вариативный payload (F4–F6)**: seed-детерминированный RNG, faker-токены,
  int/enum/datetime, regex-генерация строк, межполевые корреляции через
  `depends_on`/`mapping`/`mapping_default`.
- **Форматы**: `rfc5424`, `rfc3164`, `raw`, `protobuf` (честный wire-format).
- **Graceful shutdown (F11)**: Ctrl-C / SIGTERM → drain sender-задач.
- **HTTP `/metrics` (F12)**: Prometheus text exposition на лёгком tokio HTTP-сервере.
- **CLI (F11)**: `--profile/-p`, `--target/-t`, `--message/-m`, `--rate`, `--duration`,
  `--total`, `--format`, `--seed`, `--validate`, `--print-config`, `--schema-strict`,
  `--version`, `--help`.
- **Валидация (F13 + D3)**: семантическая (`validate_profile`) + структурная через
  JSON Schema (`--schema-strict`).
- **Типизированные ошибки (N7)**: `RuntimeError`/`MetricsError`/`ConfigError`/`DrainError`
  через `thiserror`, корректные коды возврата через `ExitCode`.
- **Zero-copy (N6)**: `BytesMut` (8 KiB) переиспользуется в TCP/TLS, `BufWriter`
  для файла. Уменьшение syscall'ов в ~50-100 раз.
- **CompiledTemplate (N5)**: one-pass парсинг `{{placeholder}}` — O(N) вместо O(N×M).
- **Property-based тесты (N8)**: 6 proptest-тестов покрывают инварианты генераторов.

### 2.2 Веха D (v8.1.0 — v8.8.1) — «Продакшн-готовность»

- **TLS по умолчанию проверяет сертификат (N4)**: `tls_ca_file`, `tls_domain`,
  `tls_insecure` (явный opt-in).
- **mTLS (N4.mTLS, v8.7.2)**: `tls_client_cert_file`, `tls_client_key_file`,
  `tls_min_protocol_version` (1.2/1.3).
- **CI-пайплайн (N9, v8.4.0)**: GitHub Actions matrix (ubuntu + macos).
- **JSON Schema + YAML-ввод (D3, v8.5.0)**: формальная schema, `--schema-strict`,
  автоопределение формата по расширению.
- **Grafana-дашборд синхронизация (N2, v8.6.0)**.
- **P1-пробелы (v8.6.1)**: N5 (CompiledTemplate), N8 (round-trip RFC 5424),
  N11 (документация как контракт).
- **N6 (v8.7.0)**: zero-copy/буферизация.
- **N8 proptest (v8.7.1)**: property-based тесты.
- **Рефакторинг слоёв N10 (v8.8.0)**: `src/format/`, `src/transport/`,
  `src/observability/`, `src/generator/`.

### 2.3 Веха E (v9.1.0 — v9.6.0) — «Зрелость»

- **Дополнительные форматы (F15, v9.2.0)**: `CEF` (ArcSight), `LEEF` (IBM QRadar),
  `JSON-lines` (NDJSON).
- **Kafka/Redpanda transport (F16, v9.3.0)**: opt-in feature `kafka` через
  pure-Rust клиент `rskafka`. Поддержка compression (gzip/lz4/snappy/zstd).
- **Файловая ротация (F16)**: `file_rotation_size_mb`, `file_rotation_interval_secs`,
  `file_rotation_max_files` с LRU cleanup.
- **Exponential backoff reconnect (F16)**: `reconnect_max_attempts`,
  `reconnect_initial_backoff_ms`, `reconnect_max_backoff_ms`, `reconnect_multiplier`.
- **N4.cipher_policy + rustls миграция (v9.5.0, BREAKING)**: `native-tls` → `rustls 0.23`,
  `tls_cipher_suites` (IANA-имена cipher suites, 8 поддерживаемых).
  `TlsVersion::V1_2`/`V1_3` enum (Rust naming).
- **Сценарии аномалий (F17, v9.5.1)**: `BurstInjection`, `SlowDrip`, `PacketLoss`
  через `Phase.anomalies`. Метрики `syslog_anomalies_applied_total`/
  `syslog_anomalies_dropped_total`.
- **Docker/musl/docker-compose (N12, v9.6.0)**: multi-stage `Dockerfile`
  (distroless/cc-debian12, ~25 MB), `.dockerignore`, docker-compose со стеком
  syslog-generator + syslog-ng + Prometheus + Grafana. Multi-arch CI build
  (linux/amd64 + linux/arm64, push в ghcr.io).

### 2.4 Веха F (v10.0.0 — v10.7.4) — «Production-hardened»

- **Breaking cleanup (v10.0.0)**: `TlsVersion::Tls12`/`Tls13` (Rust naming),
  удалён deprecated `pub use self::protobuf::*`, `Format::name()` → `Display`,
  `rust-version = "1.95"`, удалён `rcgen`.
- **CLI split (v10.1.0)**: deprecated alias `ADDR:TRANSPORT` (warning в stderr),
  новый формат `-t ADDR --transport TRANSPORT`. Полный deprecation в v11.0.0.
- **Performance ч.1 (v10.1.0)**: `lto = "fat"` + `codegen-units = 1` (5-15% throughput).
- **Performance ч.2 (v10.2.0)**: hot-path оптимизация faker-генераторов,
  `String::with_capacity(N) + write!()`. Bench: `generate_message_from_template`
  6.96µs → **5.17µs** (-26%).
- **Coverage (v10.3.0)**: `cargo-llvm-cov` baseline 86.40%.
- **Coverage + Fuzzing (v10.4.0)**: покрытие 87.07%, 5 fuzz-таргетов
  (`profile_parser`, `format_rfc5424`, `format_cef`, `format_leef`,
  `format_json_lines`). См. `docs/FUZZING.md`.
- **CI расширение (v10.5.0)**: `cargo-deny`, `cargo-machete`, MSRV-blocking,
  Dependabot.
- **Usability ч.1 (v10.6.0)**: shell completions (bash/zsh/fish/powershell/elvish)
  через `clap_complete`, man page через `clap_mangen`, colored errors через
  `owo-colors` (auto-detect `NO_COLOR` env).
- **Usability ч.2 (v10.7.0)**: structured logging через `tracing` +
  `tracing-subscriber` (RUST_LOG поддержка), progress bar `indicatif` (только при
  `duration_secs > 30` И TTY), `--dry-run`.
- **Закрытие вехи F (v10.7.1)**: breaking deps cleanup, двойной Ctrl-C = hard
  shutdown через `AtomicUsize` counter, `--config` (auto-detect JSON/YAML по
  расширению, alias `--profile`).
- **Maintenance (v10.7.2)**: Dependabot bumps (`clap_mangen 0.3`, `indicatif 0.18`).
- **PR-1 (v10.7.3)**: critical fixes (см. CHANGELOG).
- **PR-2 (v10.7.4)**: safety & correctness (см. CHANGELOG) — SIGTERM handler,
  TLS close_notify, JoinHandle tracking, etc.

---

## 3. Установка и сборка

### 3.1 Из исходников

```bash
git clone https://github.com/pharmacolog/syslog-generator.git
cd syslog-generator
cargo build --release          # бинарь: target/release/syslog-generator

# С поддержкой Kafka/Redpanda (opt-in feature)
cargo build --release --features kafka

# С дополнительными helpers для тестов
cargo build --release --features test-helpers
```

### 3.2 Через Docker

```bash
# Из корня репозитория (содержит Dockerfile)
docker build -t syslog-generator:dev .
docker run --rm syslog-generator:dev --version

# С примером профиля через volume
docker run --rm \
  -v $PWD/examples:/examples:ro \
  syslog-generator:dev \
  --profile /examples/single_target.json

# Или через docker-compose (стек: generator + syslog-ng + Prometheus + Grafana)
docker compose up
```

Multi-arch образы (linux/amd64 + linux/arm64) публикуются в
`ghcr.io/pharmacolog/syslog-generator:<tag>` через CI.

### 3.3 Готовые примеры

41 файл в `examples/` — все валидируются через `--validate --schema-strict`.
Полный список с описаниями в `examples/README.md`.

---

## 4. Быстрый старт

```bash
# 1. Запуск примера из коробки (UDP на 127.0.0.1:514, 100 msg/s, 60 сек)
./target/release/syslog-generator --profile examples/multi_target_roundrobin.json

# 2. Только проверить профиль (dry-run, exit code 0/1)
./target/release/syslog-generator --validate --profile examples/load_shape_burst.yaml

# 3. Только проверить профиль + структурную JSON Schema
./target/release/syslog-generator --validate --schema-strict --profile examples/multi_target_roundrobin.yaml

# 4. Быстрый запуск без файла-профиля (все настройки через CLI)
./target/release/syslog-generator \
  -t 127.0.0.1:514:udp \
  -m 'event seq={{sequence}} user={{faker.username}}' \
  --rate 1000 --total 10000 --seed 42

# 5. С HTTP /metrics на 9090
./target/release/syslog-generator \
  --profile examples/single_target.json \
  --metrics-addr 0.0.0.0:9090
# В другом терминале:
curl http://127.0.0.1:9090/metrics
```

---

## 5. Структура профиля

Полная структура определена в `schemas/profile.schema.json` (JSON Schema v7).
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
      ]
    }
  ]
}
```

YAML формат эквивалентен. Поддерживаются оба расширения (`.yaml`/`.yml`).

---

## 6. Поддерживаемые поля

### 6.1 Profile (корень)

| Поле | Тип | Описание |
|------|-----|----------|
| `targets` | `[TargetConfig]` | Список целевых endpoint'ов |
| `distribution` | `enum: broadcast/round-robin/weighted` | Как распределять сообщения между targets |
| `phases` | `[Phase]` | Последовательность фаз |
| `shutdown` | `ShutdownConfig` | Настройки graceful shutdown |
| `metrics_addr` | `string?` | Адрес HTTP /metrics (например `0.0.0.0:9090`) |

### 6.2 TargetConfig

| Поле | Тип | По умолчанию | Описание |
|------|-----|--------------|----------|
| `address` | `string` | — | Адрес (путь для file, host:port для tcp/udp/tls, bootstrap_servers для kafka) |
| `transport` | `enum: file/tcp/udp/tls/kafka` | — | Транспорт |
| `connections` | `u32 ≥ 1` | `1` | Размер пула воркеров на target |
| `weight` | `u32` | `1` | Вес для `weighted` distribution |
| `framing` | `enum: non-transparent/octet-counting` | `non-transparent` | Framing для TCP/TLS |
| `tls_domain` | `string?` | host-часть address | SNI / проверка имени |
| `tls_ca_file` | `string?` | — | PEM доверенного CA (для self-signed) |
| `tls_insecure` | `bool` | `false` | Opt-in в небезопасный режим (warning в stderr) |
| `tls_client_cert_file` | `string?` | — | Клиентский сертификат (mTLS, v8.7.2) |
| `tls_client_key_file` | `string?` | — | Клиентский ключ (mTLS, v8.7.2) |
| `tls_min_protocol_version` | `enum: "1.2"/"1.3"` | — | Минимальная версия TLS (v8.7.2) |
| `tls_cipher_suites` | `[string]?` | — | IANA-имена cipher suites (v9.5.0) |
| `kafka_*` | — | — | См. F16: `kafka_topic`, `kafka_compression`, `kafka_acks`, `kafka_batch_size` |
| `file_rotation_*` | — | — | См. F16: `file_rotation_size_mb`, `file_rotation_interval_secs`, `file_rotation_max_files` |
| `reconnect_*` | — | — | См. F16: `reconnect_max_attempts`, `reconnect_initial_backoff_ms`, `reconnect_max_backoff_ms`, `reconnect_multiplier` |

### 6.3 Phase

| Поле | Тип | Описание |
|------|-----|----------|
| `name` | `string` | Имя фазы (для метрик и логов) |
| `duration_secs` | `u64` | Длительность фазы (хотя бы одно из `duration_secs`/`total_messages` должно быть задано) |
| `total_messages` | `u64?` | Абсолютное количество сообщений |
| `messages_per_second` | `u64` | Целевая интенсивность |
| `templates` | `[string]` | Массив шаблонов (random choice per msg) |
| `templates_file` | `string?` | Путь к JSON-файлу с шаблонами |
| `schema_file` | `string?` | Путь к schema.json |
| `format` | `enum: rfc5424/rfc3164/raw/protobuf/cef/leef/json_lines` | Формат сообщения |
| `output` | `string?` | Override пути для file-transport |
| `seed` | `u64?` | Seed для RNG (детерминизм) |
| `protobuf_schema` | `object?` | Protobuf-схема (field_number, type, template) |
| `syslog` | `SyslogConfig?` | Поля syslog (facility, severity, hostname, etc.) |
| `load_shape` | `enum: constant/linear/sine/burst` | Профиль нагрузки во времени (F3) |
| `template_weights` | `[f64]?` | Веса для шаблонов (длина == templates.length) |
| `pad_to_bytes` | `u64?` | Дополнить payload до размера |
| `anomalies` | `[Anomaly]?` | F17: сценарии аномалий нагрузки |
| `cef` | `CefConfig?` | CEF-конфигурация (при format=cef) |
| `leef` | `LeefConfig?` | LEEF-конфигурация (при format=leef) |
| `json_lines_fields` | `[string]?` | Поля для JSON-lines |

### 6.4 Anomaly (F17, v9.5.1)

```yaml
anomalies:
  - kind: burst-injection
    rate_multiplier: 3.0
    interval_secs: 60
    duration_secs: 5
  - kind: slow-drip
    rate_divisor: 10
    duration_secs: 30
  - kind: packet-loss
    loss_percent: 5
```

---

## 7. Шаблонные плейсхолдеры

`{{placeholder}}` подставляются из `default_values` (см. `src/generator/core.rs:48`):

| Placeholder | Значение |
|-------------|----------|
| `{{sequence}}` | Порядковый номер сообщения (1..N) |
| `{{timestamp}}` | RFC3339 UTC (datetime_now_jitter, 0 jitter) |
| `{{pid}}` | Случайный PID в диапазоне 1..65535 |
| `{{hostname}}` | "localhost" (статический) |
| `{{real_hostname}}` | "localhost" (статический, для syslog-header) |
| `{{real_app}}` | `phase.name` |
| `{{real_command}}` | "echo ok" |
| `{{faker.ipv4}}` | Валидный IPv4 (например "192.168.1.42") |
| `{{faker.ipv6}}` | Валидный IPv6 |
| `{{faker.mac}}` | MAC-адрес (например "aa:bb:cc:dd:ee:ff") |
| `{{faker.uuid}}` | UUID v4 |
| `{{faker.hostname}}` | Случайный hostname |
| `{{faker.username}}` | Случайный username |
| `{{faker.user_agent}}` | User-Agent string |
| `{{faker.url}}` | URL |
| `{{faker.http_status}}` | HTTP status code |

Дополнительные placeholder'ы добавляются из schema fields и CEF/LEEF конфигов.

---

## 8. Метрики Prometheus

HTTP-эндпоинт `/metrics` (и алиас `/`) экспортирует метрики в формате
Prometheus text exposition v0.0.4. Полный список — в
`src/observability/metrics.rs`.

### 8.1 Ключевые метрики

| Метрика | Тип | Описание |
|---------|-----|----------|
| `syslog_messages_total{transport,phase,target,status}` | CounterVec | Всего сообщений отправлено |
| `syslog_bytes_total{transport,phase,target}` | CounterVec | Всего байт отправлено |
| `syslog_errors_total{target}` | CounterVec | Ошибки отправки |
| `syslog_reconnects_total{transport,target}` | CounterVec | Reconnect-попытки |
| `syslog_send_duration_seconds{transport}` | Histogram | Latency отправки (5µs–1s) |
| `syslog_message_size_bytes{transport}` | Histogram | Размер сообщений (16B–64KB) |
| `syslog_target_rate` | Gauge | Целевая интенсивность |
| `syslog_achieved_rate` | Gauge | Фактически достигнутая |
| `syslog_active_workers` | Gauge | Текущих активных sender-задач |
| `syslog_messages_by_format_total{phase,format}` | CounterVec | По формату |
| `syslog_anomalies_applied_total{phase,type}` | CounterVec | F17 |
| `syslog_anomalies_dropped_total{phase,type}` | CounterVec | F17 |
| `syslog_shutdowns_total` | Counter | SIGINT/SIGTERM получен |
| `syslog_drain_duration_seconds` | Histogram | Время drain |
| `syslog_drain_timeouts_total` | Counter | Drain не успел |

### 8.2 Пример парсинга в Grafana

```promql
# Throughput (msg/s) по target
rate(syslog_messages_total[1m])

# p95 latency (seconds)
histogram_quantile(0.95, rate(syslog_send_duration_seconds_bucket[5m]))

# Процент ошибок
rate(syslog_errors_total[5m]) / rate(syslog_messages_total[5m]) * 100
```

Grafana dashboard — `dashboards/grafana.json`.

---

## 9. TLS / Безопасность

### 9.1 Безопасный TLS по умолчанию

`tls_insecure: false` (default) включает проверку сертификата через rustls.
Для self-signed CA укажите `tls_ca_file`. Для полностью невалидного режима —
`tls_insecure: true` (warning в stderr).

### 9.2 mTLS (N4.mTLS)

```json
{
  "address": "syslog.example.com:6514",
  "transport": "tls",
  "tls_client_cert_file": "/etc/syslog/client.crt",
  "tls_client_key_file": "/etc/syslog/client.key",
  "tls_ca_file": "/etc/syslog/ca.crt",
  "tls_min_protocol_version": "1.3"
}
```

`tls_client_cert_file` + `tls_client_key_file` оба обязательны (иначе warning
в stderr и mTLS отключён). Поддерживаются цепочки сертификатов (full-chain).

### 9.3 Cipher Policy (N4.cipher_policy, v9.5.0)

```json
{
  "tls_cipher_suites": [
    "TLS_AES_256_GCM_SHA384",
    "TLS_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384"
  ]
}
```

8 поддерживаемых suites (3 для TLS 1.3, 5 для TLS 1.2 ECDHE_*_WITH_AES_*_GCM_*).
Невалидные имена пропускаются с warning, остальные сохраняются (PR-1 fix).

### 9.4 Версия TLS

```json
{ "tls_min_protocol_version": "1.3" }  // или "1.2"
```

Защита от downgrade-атак на устаревшие версии.

---

## 10. CLI-флаги (v10.7.4)

### 10.1 Subcommands

| Subcommand | Описание |
|------------|----------|
| `<без subcommand>` | Запуск с профилем (по умолчанию) |
| `completions <shell>` | (v10.6.0) Сгенерировать shell completions (bash/zsh/fish/powershell/elvish) |
| `man` | (v10.6.0) Сгенерировать man page в stdout |

### 10.2 Глобальные флаги

| Флаг | Описание |
|------|----------|
| `--profile/-p <path>` | Путь к JSON/YAML профилю |
| `--config <path>` | (v10.7.1) Alias `--profile` |
| `--target/-t <ADDR[:TRANSPORT]>` | Target override (deprecated в v10.1.0, удалится в v11.0.0) |
| `--transport <TRANSPORT>` | Transport override для `--target` |
| `--distribution <DIST>` | Distribution override |
| `--rate <N>` | Rate override (msg/sec) |
| `--duration <SECS>` | Duration override |
| `--total <N>` | Total messages override |
| `--format <FMT>` | Format override |
| `--seed <N>` | RNG seed override |
| `--message/-m <MSG>` | Inline template (без файла-профиля) |
| `--metrics-addr <ADDR>` | HTTP /metrics address override |
| `--validate` | Только валидация профиля (dry-run) |
| `--schema-strict` | С JSON Schema (вместе с `--validate`) |
| `--print-config` | Печатает эффективный конфиг |
| `--dry-run` | (v10.7.0) Печатает план без отправки |
| `--version` | Версия |
| `--help` | Помощь |

### 10.3 Environment variables

| Env var | Описание |
|---------|----------|
| `RUST_LOG` | (v10.7.0) Уровень логирования (`tracing-subscriber` env-filter) |
| `NO_COLOR` | (v10.6.0) Отключает цветной вывод ошибок |

---

## 11. Graceful shutdown и double Ctrl-C

(v10.7.1 + v10.7.4)

- **Первый Ctrl-C (SIGINT) или SIGTERM** → graceful drain sender-задач с
  таймаутом `shutdown.drain_timeout_secs` (по умолчанию 30 сек).
- **Второй Ctrl-C/SIGTERM** (если процесс ещё не завершился) → hard exit с
  кодом 2.

`SIGTERM` обрабатывается с v10.7.4 (важно для Docker/K8d).

---

## 12. Валидация и тесты

### 12.1 Валидация профиля

```bash
# Семантическая (F13)
syslog-generator --validate --profile myprofile.yaml

# Структурная через JSON Schema (D3)
syslog-generator --validate --schema-strict --profile myprofile.yaml
```

`--validate` возвращает exit code 0 (OK) или 1 (есть ошибки). Все ошибки
выводятся в stderr на русском с указанием target/phase/index.

### 12.2 Существующие тесты

```bash
# Все 339 тестов (242 unit + 86 integration + 11 n7)
cargo test --locked --features test-helpers

# С Kafka (343+ тестов)
cargo test --locked --features kafka,test-helpers

# Бенчмарки (Criterion)
cargo bench --no-run --locked
cargo bench --bench message_generation -- --quick
cargo bench --bench sender_throughput -- --quick
```

### 12.3 Fuzzing (v10.4.0)

См. `docs/FUZZING.md`. 5 таргетов: `profile_parser`, `format_rfc5424`,
`format_cef`, `format_leef`, `format_json_lines`.

---

## 13. Ограничения и поведение

- **TLS по умолчанию проверяет сертификат** (N4). Для self-signed — `tls_ca_file`.
- **mTLS**: оба файла (`tls_client_cert_file` + `tls_client_key_file`) обязательны.
- **Framing (F9)**: для TCP/TLS — `non-transparent` (default) или `octet-counting`.
- **Backpressure**: `mpsc(1024)` на target — продюсер блокируется при переполнении.
- **Sender-fail**: drain очереди + продолжение фазы (recoverable).
- **CounterVec без наблюдённых меток** не экспортируется до первого `inc()`.
- **File-transport с rotation**: при `size_mb == 0` или `interval_secs == 0`
  валидатор возвращает ошибку.

---

## 14. История версий (полная)

См. `CHANGELOG.md`. Кратко:

- v8.0.0 — закрытие вех A/B/C
- v8.1.0 — v8.8.1 — веха D (Production-ready)
- v9.0.0 — milestone ЗАКРЫТИЕ вехи D
- v9.1.0 — v9.6.0 — веха E (Зрелость: F15/F16/F17/N4.cipher_policy/N12)
- v10.0.0 — v10.7.4 — веха F (Production-hardened)
- **Вехи D/E/F закрыты.** Следующие кандидаты (см. PLAN-v10.0.0.md §6).

---

## 15. См. также

- **DEVELOPER_GUIDE.md** — архитектура, слои, добавление формата/транспорта.
- **CHANGELOG.md** — детальная история релизов.
- **PLAN-v10.0.0.md** — план вехи F (закрыта).
- **AUDIT.md** — реестр задач и аудит (v10.7.2).
- **CLAUDE_HANDOFF.md** — перенос контекста для Claude.
- **examples/README.md** — каталог 41 примера.
- **schemas/profile.schema.json** — формальная JSON Schema.
- **docs/FUZZING.md** — fuzzing instructions.
- **docs/COVERAGE.md** — coverage отчёты.