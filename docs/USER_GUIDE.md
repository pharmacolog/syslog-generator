# USER GUIDE

Версия документа: `v8.6`.

## Возможности

- **Multi-target профили**: одновременная отправка в несколько целей с разной
  диспетчеризацией (`broadcast`, `round-robin`, `weighted`).
- **Транспорты**: `file` (с атомарной записью через `O_APPEND`), `tcp`,
  `udp`, `tls` (с проверкой сертификата по умолчанию — N4).
- **Профили нагрузки во времени (F3)**: `constant`, `linear`, `sine`,
  `burst` через поле `load_shape` фазы.
- **Rate-limiting (F1)**: токен-бакет через `governor`, истинная
  интенсивность `messages_per_second`.
- **Connection pool (F2)**: пул воркеров на target через `connections`.
- **Условия остановки фазы**: `duration_secs`, `total_messages` (F13
  валидирует, что хотя бы одно задано).
- **Multi-template (F14)**: случайный выбор из массива шаблонов на каждое
  сообщение, с весами через `template_weights`.
- **Вариативный payload (F4-F6, F14)**: `seed`-детерминированный RNG,
  faker-токены (`ipv4`/`ipv6`/`mac`/`uuid`/`hostname`/`username`/
  `user_agent`/`url`/`http_status`), `int` с диапазоном, `enum` со
  случайным выбором (uniform/weighted/zipf), `datetime` с реальным
  временем и джиттером, `regex`-генерация строк, межполевые
  корреляции через `depends_on`/`mapping`/`mapping_default`.
- **Форматы**: `rfc5424` (по умолчанию), `rfc3164` (BSD), `raw`,
  `protobuf` (wire-format varint + length-delimited).
- **Graceful shutdown (F11)**: Ctrl-C → drain sender-задач с
  таймаутом `shutdown.drain_timeout_secs`.
- **HTTP `/metrics` (F12)**: Prometheus text exposition format v0.0.4
  через лёгкий HTTP-сервер на `tokio` (без hyper/axum). Порт
  настраивается полем `metrics_addr` и флагом `--metrics-addr`.
- **CLI (F11)**: `syslog-generator --profile <file> [--target ...]
  [--message ...] [--rate ...] [--format ...]`, плюс `--validate`,
  `--print-config`, `--schema-strict`, `--version`, `--help`.
- **Загрузка профиля**: JSON (`.json`) и YAML (`.yaml`/`.yml`) с
  автоопределением формата по расширению (D3).
- **Валидация (F13 + D3)**: семантическая (`validate_profile`) +
  структурная через формальную JSON Schema (`--schema-strict`).
- **Типизированные ошибки рантайма (N7)**: `RuntimeError` через
  `thiserror` с пробросом через `?` в `anyhow::Error` на границе CLI.
  Корректные коды возврата через `ExitCode`.

## Быстрый старт

```bash
# Сборка
cargo build --release

# Запуск примера из коробки
./target/release/syslog-generator --profile examples/multi_target_roundrobin.json

# Только проверить профиль (dry-run, exit code 0/1)
./target/release/syslog-generator --validate --profile examples/load_shape_burst.yaml

# Только проверить профиль + структурную JSON Schema
./target/release/syslog-generator --validate --schema-strict --profile examples/multi_target_roundrobin.yaml
```

## Ограничения и поведение

- **TLS по умолчанию проверяет сертификат** (N4). Для self-signed CA
  укажите `tls_ca_file` в TargetConfig или явный `tls_insecure: true`
  (с предупреждением в stderr).
- **Framing** (F9): для TCP/TLS — `non-transparent` (по умолчанию) или
  `octet-counting` (RFC 6587). Для UDP/file — каждое сообщение
  отдельная единица.
- **Транспортные сбои**: sender фиксирует `syslog_errors_total`,
  дренирует входную очередь, продолжает фазу (recoverable).
- **Backpressure**: `mpsc(1024)` на target — при переполнении продюсер
  блокируется.
- **Метрики Prometheus**: `CounterVec` без наблюдённых меток не
  экспортируются до первого `inc()`. Скалярные `IntCounter` и
  `Histogram` экспортируются всегда.

## Test coverage (v8.6)

- `tests/integration_tests.rs`: 55 end-to-end тестов на mixed-target
  профилях (file+tcp+udp+tls) во всех режимах диспетчеризации.
- `tests/n7_runtime_errors.rs`: 11 сценарных тестов для типизированных
  ошибок рантайма (CLI flags, edge cases).
- 9 бенчей (`message_generation` x3, `sender_throughput` x6) на
  Criterion.
- `cargo bench --no-run --locked` — стадия CI-гейта.
- Property-based тесты для payload (через `cargo test`) покрывают
  детерминизм по seed и round-trip парсинг RFC 5424.
