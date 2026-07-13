# USER GUIDE

Версия документа: `v8.8.1`.

## Возможности

- **Multi-target профили**: одновременная отправка в несколько целей с разной
  диспетчеризацией (`broadcast`, `round-robin`, `weighted`).
- **Транспорты**: `file` (с атомарной записью через `O_APPEND` + `BufWriter`),
  `tcp`, `udp`, `tls` (с проверкой сертификата по умолчанию — N4; mTLS через
  `tls_client_cert_file`/`tls_client_key_file` — N4.mTLS).
- **Профили нагрузки во времени (F3)**: `constant`, `linear`, `sine`,
  `burst` через поле `load_shape` фазы.
- **Rate-limiting (F1)**: токен-бакет через `governor`, истинная
  интенсивность `messages_per_second`.
- **Connection pool (F2)**: пул воркеров на target через `connections`.
- **Условия остановки фазы**: `duration_secs`, `total_messages` (F13
  валидирует, что хотя бы одно задано).
- **Multi-template (F14)**: случайный выбор из массива шаблонов на каждое
  сообщение, с весами через `template_weights`.
- **Вариативный payload (F4-F6, F14)**: `seed`-детерминированный RNG
  (inter-process одинаковый), faker-токены (`ipv4`/`ipv6`/`mac`/`uuid`/
  `hostname`/`username`/`user_agent`/`url`/`http_status`), `int` с
  диапазоном, `enum` со случайным выбором (uniform/weighted/zipf),
  `datetime` с реальным временем и джиттером, `regex`-генерация строк,
  межполевые корреляции через `depends_on`/`mapping`/`mapping_default`.
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
- **Zero-copy/буферизация (N6)**: `BytesMut` (8 KiB) переиспользуется
  между сообщениями в TCP/TLS, `BufWriter` для файла. Уменьшение
  syscall'ов в ~50-100 раз для типичной нагрузки (10k msg/s).
- **CompiledTemplate (N5)**: one-pass парсинг `{{placeholder}}` —
  O(N) вместо O(N×M) `String::replace`-цикла.
- **Property-based тесты (N8)**: 6 proptest-тестов в `src/payload_proptests.rs`
  покрывают инварианты генераторов (int-диапазон, seed-детерминизм,
  faker IPv4/UUID формат, pad_to_size, и т.д.).
- **mTLS (N4.mTLS)**: 3 новых поля `TargetConfig` — `tls_client_cert_file`,
  `tls_client_key_file`, `tls_min_protocol_version` (значение `"1.2"` или
  `"1.3"`). `parse_tls_min_version` парсит строку в `native_tls::Protocol`.
- **Рефакторинг слоёв (N10)**: `src/format/`, `src/transport/`,
  `src/observability/`, `src/generator/`. 0 breaking changes (старые модули
  сохранены как thin re-export обёртки). `src/architecture-notes.md`
  переписан с реальной архитектурой.

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
- **mTLS (N4.mTLS)**: если задан `tls_client_cert_file`, должен быть
  задан и `tls_client_key_file` (и наоборот) — иначе mTLS отключён
  (warning в stderr). `tls_min_protocol_version` должен быть `"1.2"` или
  `"1.3"` (защита от downgrade-атак на устаревшие версии).
- **Framing (F9)**: для TCP/TLS — `non-transparent` (по умолчанию) или
  `octet-counting` (RFC 6587). Для UDP/file — каждое сообщение
  отдельная единица.
- **Транспортные сбои**: sender фиксирует `syslog_errors_total`,
  дренирует входную очередь, продолжает фазу (recoverable).
- **Backpressure**: `mpsc(1024)` на target — при переполнении продюсер
  блокируется. Покрыто косвенно через N6 (батчинг), rate-limit и
  `drain_as_errors` (тест на back-pressure отозван как flaky — TCP-буфер
  ядра > 64KB вмещает маленькие сообщения мгновенно).
- **Метрики Prometheus**: `CounterVec` без наблюдённых меток не
  экспортируются до первого `inc()`. Скалярные `IntCounter` и
  `Histogram` экспортируются всегда.

## Отложено в веху E

- **N4.cipher_policy** (allow/denylist шифров): `native-tls` не имеет
  прямого API для кастомных cipher lists — использует OS defaults. Для
  кастомных списков шифров нужен переход на `rustls` или
  `openssl-sys`. **Перенесено в веху E (F16).**

## Test coverage (v8.8.1)

- `tests/integration_tests.rs`: 70 end-to-end тестов на mixed-target
  профилях (file+tcp+udp+tls) во всех режимах диспетчеризации.
- `tests/n7_runtime_errors.rs`: 11 сценарных тестов для типизированных
  ошибок рантайма (CLI flags, edge cases).
- 6 property-based тестов в `src/payload_proptests.rs` (N8) — proptest.
- 3 round-trip теста в `src/syslog.rs::tests` (N8) — RFC 5424 парсинг.
- 4 N4.mTLS теста в `src/transport/tls.rs::tests` — клиентский
  identity + min_protocol.
- 9 бенчей (`message_generation` x3, `sender_throughput` x6) на
  Criterion.
- `cargo bench --no-run --locked` — стадия CI-гейта.
- Итого: **199 тестов** (118 unit + 70 integration + 11 N7), все зелёные.
