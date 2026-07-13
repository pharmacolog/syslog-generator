# DEVELOPER GUIDE

Версия документа: `v8.8.1`.

## Модульная архитектура (N10, v8.8.0)

Слои (от внешнего к внутреннему), с явным разделением ответственности:

```
src/
├── main.rs           # точка входа; ExitCode; load_profile_from_path + --schema-strict
├── cli.rs            # clap derive; Args с флагами-оверрайдами
├── validate.rs       # F13: семантическая валидация профиля
├── schema_check.rs   # D3: структурная JSON Schema валидация
├── format/           # N10: форматы (rfc5424/rfc3164/raw/protobuf)
│   ├── mod.rs        # общие утилиты (Header, prival, escape_sd_value, BOM, NILVALUE)
│   ├── rfc5424.rs
│   ├── rfc3164.rs
│   ├── raw.rs
│   └── protobuf.rs   # wire-format varint + length-delimited (F10)
├── transport/        # N10: транспорты (file/tcp/udp/tls)
│   ├── mod.rs        # общая инфраструктура (SharedRx, Framing, record_*, frame_into)
│   ├── file.rs       # target_sender_file (BufWriter, N6)
│   ├── tcp.rs        # target_sender_tcp + reconnect_tcp (BytesMut, N6)
│   ├── udp.rs        # target_sender_udp (zero-copy по дизайну)
│   └── tls.rs        # N4: target_sender_tls + build_tls_connector + parse_tls_min_version
│                     # N4.mTLS: client identity + min_protocol
├── observability/    # N10: Prometheus метрики + HTTP /metrics
│   ├── mod.rs
│   ├── metrics.rs    # Metrics, create_metrics, gather_metrics
│   └── server.rs     # parse_request_line, route, build_http_response, serve
├── generator/        # N10: оркестрация профиля
│   ├── mod.rs
│   ├── core.rs       # run_profile, run_phase_multi, generate_message, create_dispatcher
│   └── config.rs     # Profile, Phase, TargetConfig, load_profile_*
├── payload.rs        # F4-F6, F14: faker, regex, корреляции
├── template.rs       # N5: CompiledTemplate (one-pass парсинг)
├── schema.rs         # F5: schema.json для schema-per-phase
├── load_shape.rs     # F3: профили нагрузки (constant/linear/sine/burst)
├── shutdown.rs       # N7: graceful drain_wait
├── error.rs          # N7: RuntimeError + под-типы (MetricsError, ConfigError, DrainError)
└── lib.rs            # pub use реэкспорты + backward-compat алиасы (core/config/...)

# Backward-compat: pub mod core/config/sender/syslog/metrics/metrics_server/protobuf
# (старые имена) — это thin re-export обёртки из новых слоёв. Публичный API
# не меняется: syslog_generator::run_profile, syslog_generator::Metrics,
# syslog_generator::build_rfc5424 и т.д. продолжают работать.
```

### Trait `Format` (план для F15, веха E)

```rust
pub trait Format {
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>;
    fn name(&self) -> &'static str;
}
```

Реализации (в `src/format/`):
- `rfc5424::build` → `name = "rfc5424"`
- `rfc3164::build` → `name = "rfc3164"`
- `raw::build` → `name = "raw"`
- `protobuf::serialize_protobuf` → `name = "protobuf"`

В вехе E (F15) добавим `Cef`, `Leef`, `JsonLines` форматы с trait-имплементациями.

### Trait `Transport` (план для F16, веха E)

```rust
pub trait Transport: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, rx: SharedRx, metrics: Metrics, shutdown: CancellationToken) -> anyhow::Result<()>;
}
```

В вехе E (F16) добавим `Kafka`, `Redis`, persistent-queue с trait-имплементациями.

## Загрузка и валидация профиля (N10 + D3)

```rust
use syslog_generator::{load_profile_from_path, load_profile_from_json_str, load_profile_from_yaml_str};
use std::path::Path;

// JSON или YAML — определяется по расширению.
let profile = load_profile_from_path(Path::new("profile.yaml"))?;

// Только JSON или только YAML.
let p_json = load_profile_from_json_str(r#"{"distribution":"round-robin","phases":[]}"#)?;
let p_yaml = load_profile_from_yaml_str("distribution: round-robin\nphases: []\n")?;

// Семантическая валидация (F13).
let errors: Vec<ValidationError> = validate_profile(&profile);
if !errors.is_empty() { /* fail-fast */ }

// Структурная JSON Schema (D3).
syslog_generator::validate_against_embedded_schema(&profile)?;

// Транспорт (N10): вызывается из src/transport/, автоматически
// резолвится через main.rs.
```

## Генерация сообщения (N5 CompiledTemplate)

```rust
use syslog_generator::{CompiledTemplate, generate_message, Phase};
use std::collections::HashMap;

let phase = Phase { /* ... */ };
let mut values = HashMap::new();
values.insert("seq".to_string(), "42".to_string());
let msg = generate_message(&phase, 42)?;

// One-pass парсинг (N5): CompiledTemplate компилирует шаблон один раз.
let ct = CompiledTemplate::compile("hello {{name}}!");
let rendered = ct.render(&values); // O(N) вместо O(N×M)
```

## Отправка через транспорты (N10)

```rust
use syslog_generator::{run_profile, create_metrics, Metrics, TlsParams};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

let metrics: Metrics = create_metrics()?;
let profile = syslog_generator::load_profile_from_path(Path::new("profile.yaml"))?;

// run_profile делает fail-fast: validate_profile, потом run.
let result = tokio::runtime::Runtime::new()?.block_on(async {
    run_profile(&profile, metrics).await
});
```

`run_profile` валидирует (F13), поднимает HTTP /metrics (F12) если задан
`metrics_addr`, запускает фазы с rate-limiting (F1) и drain (N7) по завершении.

## Бенчмарки

Объявлены в `Cargo.toml` как `[[bench]]` с `harness = false`:

- `benches/message_generation.rs` — `template_render` (N5 CompiledTemplate),
  `generate_message`, `create_dispatcher`. Проверяет zero-copy: O(N) вместо O(N×M).
- `benches/sender_throughput.rs` — пропускная способность отправки через
  `run_profile` с реальными TCP/UDP коллекторами. Проверяет эффективность
  zero-copy/буферизации (N6): BufWriter (file), BytesMut (TCP/TLS).

CI-проверка: `cargo bench --no-run --locked` — стадия CI-гейта. Для
полного прогона локально: `cargo bench --bench <name> -- --quick`.

## Архитектурные решения (v8.x)

- **v8.3.0 (N7):** типизированные ошибки рантайма. `RuntimeError`
  через `thiserror` с пробросом через `?` в `anyhow::Error` на границе
  CLI. Все `.unwrap()`/`.expect()` в рантайм-коде устранены. Политика
  recoverability (bind-fail на `/metrics`, transport-fail sender'ов)
  сохранена. 11 новых интеграционных тестов в `tests/n7_runtime_errors.rs`;
  +14 unit-тестов в `error::tests`/`metrics::tests`.
- **v8.3.1:** починка 3 TLS-target тестов (mixed_*_end_to_end), которые
  до этого падали из-за `Identity::from_pkcs8` не парсящего rcgen-PEM.
  Перешли на `openssl_self_signed` (через системный `openssl`).
- **v8.4.0 (N9):** CI-пайплайн на GitHub Actions. Все PR и push в
  `main`/`dev` проходят через `fmt --check` → `clippy --all-targets -D warnings`
  → `build --release` → `cargo test --no-run --locked` → `cargo test --locked`
  → `cargo bench --no-run --locked`. Кэш cargo через `Swatinem/rust-cache@v2`.
  На Linux устанавливается `libssl-dev` для `openssl-sys`. Best-effort MSRV-job
  читает канал из `rust-toolchain.toml` (если файл есть).
- **v8.4.1:** починка `sender_throughput` бенчмарка (F13-валидация профиля
  требует `total_messages` в Phase; `make_profile` теперь принимает
  `total_messages: u64`).
- **v8.5.0 (D3):** формальная JSON Schema (`schemas/profile.schema.json`)
  + YAML-ввод. `load_profile_from_yaml_str`, `serde_yaml = "0.9"`. Профили
  можно загружать как JSON, так и YAML с автоопределением по расширению.
  Новый флаг `--schema-strict` для runtime-валидации через `jsonschema`.
  6 examples YAML-профилей.
- **v8.6.0 (N2):** синхронизация Grafana-дашборда. `syslog_messages_by_format_total`
  (N2, N7: новый CounterVec) + 6 panels в `dashboards/grafana.json`
  (rate/latency/active workers/errors/messages by format). Удалены
  фейковые `cpu_usage_percent`/`memory_usage_bytes` Gauge'ы — не обновлялись
  в runtime. CPU-метрики остаются как TODO (N1 в P2, веха E).
- **v8.6.1 (N5 + N8 + N11):** `CompiledTemplate` (N5) с one-pass
  парсингом (`frame_into` / `template::CompiledTemplate`) —
  0 аллокаций на сообщение в горячем пути. 6 proptest-тестов
  (N8): `prop_int_in_range`, `prop_seed_determinism`,
  `prop_pad_to_size_*`, `prop_faker_*` (RFC 5424 round-trip). Документация
  (N11) — `docs/USER_GUIDE.md`, `docs/DEVELOPER_GUIDE.md` обновлены.
- **v8.7.0 (N6):** zero-copy/буферизация. `BufWriter<File>` (8 KiB) для
  файлового транспорта, `BytesMut` (8 KiB) для TCP/TLS. Уменьшение
  syscall'ов в ~50-100 раз для типичной нагрузки (10k msg/s). Все 4
  транспорта используют переиспользуемые буферы.
- **v8.7.1 (N8):** property-based тесты через `proptest = "1"`.
  6 тестов в `src/payload_proptests.rs` (int-диапазон, seed-детерминизм,
  faker IPv4/UUID формат, pad_to_size edge cases). Back-pressure
  integration-тест отозван (TCP-буфер ядра > 64KB вмещает маленькие
  сообщения мгновенно) — покрыто косвенно через N6 + rate-limit
  + `drain_as_errors`.
- **v8.7.2 (N4.mTLS):** 3 новых TargetConfig-поля для mTLS.
  `tls_client_cert_file` + `tls_client_key_file` (PEM-пара) →
  `build_tls_connector` загружает через `Identity::from_pkcs8`. `tls_min_protocol_version`
  (значения `"1.2"`/`"1.3"`) → `builder.min_protocol_version`. 3 новых
  `ValidationError` для fail-fast. 9 новых integration-тестов.
- **v8.8.0 (N10):** рефакторинг слоёв. `src/format/` (RFC 5424/3164/
  raw/protobuf), `src/transport/` (file/tcp/udp/tls), `src/observability/`
  (Prometheus + HTTP /metrics), `src/generator/` (orchestration).
  `src/architecture-notes.md` переписан с реальной архитектурой
  (был заглушкой v7.4.0). Trait `Format` и trait `Transport` объявлены
  как план для вехи E.
- **v8.8.1:** patch-долг AUDIT.md. Поставлены ✅ на F7/F8/F9
  (реализованы в v7.7.0, но галочки отсутствовали), убраны устаревшие
  "Отложено" из F13 (D3 сделано в v8.5.0) и N4.mTLS (сделано в
  v8.7.2). **N4.cipher_policy** (allow/denylist шифров) остаётся
  отложенным в веху E.

## Качество (v8.8.1)

- **199 тестов** (118 unit + 70 integration + 11 N7) — все зелёные.
- **9 бенчей** (3 + 6) — все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.
- CI: GitHub Actions (`.github/workflows/ci.yml`) — все 3 job'а
  зелёные (Test macos-latest, Test ubuntu-latest, MSRV check).
