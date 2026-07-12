# DEVELOPER GUIDE

Версия документа: `v8.6`.

## Модульная архитектура

```
src/
├── main.rs           # точка входа; ExitCode; load_profile_from_path + --schema-strict
├── cli.rs            # clap derive; Args с флагами-оверрайдами
├── config.rs         # Profile, Phase, TargetConfig, SyslogConfig; load_profile_from_path
├── error.rs          # RuntimeError, ConfigError, MetricsError, DrainError (thiserror)
├── validate.rs       # F13: семантическая валидация профиля (ValidationError)
├── schema_check.rs   # D3: runtime JSON Schema (jsonschema = "0.18")
├── load_shape.rs     # F3: кривые нагрузки (constant/linear/sine/burst)
├── template.rs       # CompiledTemplate: one-pass парсинг `{{placeholder}}`
├── payload.rs        # F4-F6, F14: seed-детерминированный RNG, faker, regex, корреляции
├── syslog.rs         # F7-F8: RFC 5424 / RFC 3164 builders
├── protobuf.rs       # F10: wire-format varint + length-delimited
├── sender.rs         # F2, N4: target_sender_file/tcp/udp/tls + build_tls_connector
├── metrics.rs        # F12, N3: Prometheus registry + 18 метрик
├── metrics_server.rs # F12: лёгкий HTTP /metrics на tokio
├── shutdown.rs       # N7: graceful drain_wait с типизированной DrainError
├── core.rs           # run_profile, run_phase_multi, generate_message, create_dispatcher
├── schema.rs         # F5: schema-файлы (отдельные JSON-описания полей)
└── lib.rs            # pub use реэкспорты всех публичных API
```

## Загрузка и валидация профиля

```rust
// JSON или YAML — определяется по расширению.
let profile = syslog_generator::load_profile_from_path(Path::new("profile.yaml"))?;

// Семантическая валидация (F13): F13Error-типизированные ошибки.
let errors = syslog_generator::validate_profile(&profile);
if !errors.is_empty() {
    eprint!("{}", syslog_generator::format_errors(&errors));
    std::process::exit(1);
}

// Структурная валидация (D3): против schemas/profile.schema.json.
syslog_generator::validate_against_embedded_schema(&profile)?;
```

## Генерация сообщения

```rust
let phase = Phase { /* ... */ };
let msg = syslog_generator::generate_message(&phase, seq)?;
// seq используется для детерминированного RNG (seed × seq → SplitMix64 → StdRng).
```

Для горячего пути `CompiledTemplate::compile()` один раз, затем многократный
`render()` — O(N) по длине шаблона вместо O(N×M) `String::replace`-цикла.

## Отправка через транспорты

```rust
use syslog_generator::{run_profile, create_metrics, Metrics};

let metrics: Metrics = create_metrics()?;       // типизированная ошибка MetricsError
run_profile(&profile, metrics).await?;            // Result<(), anyhow::Error>
```

`run_profile` сначала валидирует (F13), затем поднимает HTTP /metrics (F12)
если задан `metrics_addr`, потом запускает фазы с rate-limiting и drain
по завершении.

## Бенчмарки

Объявлены в `Cargo.toml` как `[[bench]]` с `harness = false`:

- `benches/message_generation.rs` — `template_render`, `generate_message`,
  `create_dispatcher`.
- `benches/sender_throughput.rs` — пропускная способность отправки через
  `run_profile` с реальными TCP/UDP коллекторами.

CI-проверка: `cargo bench --no-run --locked` (компиляция). Для прогона
бенчей локально: `cargo bench --bench <name> -- --quick`.

## Архитектурные решения (v8.x)

- **N7 (v8.3) — типизированные ошибки рантайма**: `RuntimeError` через
  `thiserror` с пробросом через `?` в `anyhow::Error` на границе CLI.
  Все `.unwrap()`/`.expect()` в рантайм-коде устранены.
- **v8.3.1 — TLS-тесты**: `benches/sender_throughput.rs::make_profile`
  выставляет явный `total_messages` (без этого F13-валидация отвергала
  фазу как `UnboundedPhase`).
- **D3 (v8.5) — JSON Schema + YAML**: `serde_yaml = "0.9"`,
  `jsonschema = "0.18"` (runtime). Схема встроена через `include_str!`.
- **N2 (v8.6) — синхронизация дашборда**: реальная
  `syslog_messages_by_format_total` + 6 panels в `dashboards/grafana.json`.
  Удалены фейковые gauge-ы `cpu_usage_percent`/`memory_usage_bytes`.
- **v8.6.1 (планируется)** — `CompiledTemplate` (one-pass парсинг вместо
  `String::replace` цикла), round-trip парсер RFC 5424, обновлённые
  `docs/`, исправленные `.meta.json` (v4.0 → v8.6).

## Quality state (v8.6)

- 115 unit + 55 integration + 11 N7 = **181 тестов, все зелёные**.
- 9 бенчей (3 + 6), все проходят `cargo bench -- --quick`.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.
- CI: GitHub Actions (`.github/workflows/ci.yml`) — fmt/clippy/build/test/bench.
