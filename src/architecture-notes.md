# Архитектура syslog-generator v8.8.0+

## Слои (от внешнего к внутреннему)

```
┌─────────────────────────────────────────────────────────────┐
│ CLI (clap): parse_target, apply_overrides, Args, Overrides  │
└────────────┬────────────────────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────────┐
│ main.rs — точка входа; ExitCode; load_profile_from_path;      │
│ --validate/--print-config/--schema-strict/--metrics-addr    │
└────────────┬────────────────────────────────────────────────┘
             │
   ┌─────────┴─────────┬────────────────┬──────────────┐
   ▼                   ▼                ▼              ▼
validate.rs    schema_check.rs    generator/     observability/
(F13)         (D3 JSON        (run_profile,   (metrics +
              Schema)        generate_message, metrics_server
                             load_profile)    HTTP /metrics)
                                  │
                  ┌───────────────┼───────────────┐
                  ▼               ▼               ▼
              transport/      format/         payload/   template/
              (file/tcp/     (rfc5424/        (F4-F6,    CompiledTemplate
               udp/tls)      rfc3164/        F14)        с one-pass
                            raw/                               парсингом)
                            protobuf)
```

## Детальное описание слоёв (N10 v8.8.0)

### `src/format/` — форматы syslog-сообщений
- `mod.rs` — общие утилиты: `Header`, `prival`, `escape_sd_value`, `BOM`, `NILVALUE`, `sanitize_header`.
- `rfc5424.rs` — `build_rfc5424(&Header, &[u8]) -> Vec<u8>` (RFC 5424 §6.4).
- `rfc3164.rs` — `build_rfc3164(&Header, &[u8]) -> Vec<u8>` (BSD syslog).
- `raw.rs` — passthrough (без обёртки, для интеграций где syslog-фрейм уже есть).
- `protobuf.rs` — `apply_protobuf_schema`, `serialize_protobuf`, `serialize_protobuf_like` (wire-format varint + length-delimited, F10).

### `src/transport/` — способы доставки
- `mod.rs` — общая инфраструктура: `SharedRx` (Arc<Mutex<Receiver<Vec<u8>>>>), `Framing` (RFC 6587), `record_send`, `record_send_latency`, `record_reconnect`, `record_error`, `drain_as_errors`, `next_msg`, `frame_into` (N6 zero-copy BytesMut).
- `file.rs` — `target_sender_file` (BufWriter, N6).
- `tcp.rs` — `target_sender_tcp` + `reconnect_tcp` (BytesMut, N6).
- `udp.rs` — `target_sender_udp` (zero-copy по дизайну).
- `tls.rs` — `target_sender_tls` + `tls_connect` + `TlsParams` + `build_tls_connector` + `parse_tls_min_version` (N4 + N4.mTLS).

### `src/generator/` — оркестрация профиля
- `core.rs` — `run_profile`, `run_phase_multi`, `generate_message`, `create_dispatcher`, `default_values`, `load_schema`, `load_templates` (F1-F3, F11, F13, F14).
- `config.rs` — `Profile`, `Phase`, `TargetConfig`, `SyslogConfig`, `ShutdownConfig`, `ProtobufSchemaFieldMap`, `load_profile_from_path` (D3 — YAML/JSON auto-detect), `load_profile_from_json_str`, `load_profile_from_yaml_str`.

### `src/observability/` — Prometheus метрики
- `metrics.rs` — `Metrics`, `create_metrics`, `gather_metrics` (F12, N3, 18 метрик).
- `server.rs` — лёгкий HTTP-эндпоинт на tokio (F12): `parse_request_line`, `route`, `build_http_response`, `serve`, `spawn`.

### Базовые (не переехали)
- `cli.rs` — clap derive (Args, Overrides).
- `error.rs` — RuntimeError + под-типы (N7 thiserror).
- `payload.rs` — генератор payload: faker, regex, корреляции (F4-F6, F14).
- `template.rs` — CompiledTemplate (one-pass парсинг, N5 v8.6.1).
- `schema.rs` — загрузка schema.json (F5).
- `load_shape.rs` — F3 профили нагрузки.
- `shutdown.rs` — graceful drain.
- `validate.rs` — F13 семантическая валидация профиля.

## Backward-compat

Старые модули `core`, `config`, `sender`, `syslog`, `metrics`, `metrics_server`,
`protobuf` сохранены как **thin re-export обёртки** в `src/`. Например:
- `src/core.rs` — `pub use crate::generator::*;`
- `src/config.rs` — `pub use crate::generator::config::*;`
- `src/sender.rs` — `pub use crate::transport::*;`
- `src/syslog.rs` — `pub use crate::format::*;`
- `src/metrics.rs` — `pub use crate::observability::metrics::*;`
- `src/metrics_server.rs` — `pub use crate::observability::server::*;`
- `src/protobuf.rs` — `pub use crate::format::protobuf::*;`

Это гарантирует что `syslog_generator::run_profile`,
`syslog_generator::Profile`, `syslog_generator::build_rfc5424` и т.д.
**продолжают работать** без изменений в пользовательском коде.

## Trait `Format` (планируется в веху E)

```rust
pub trait Format {
    fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>;
    fn name(&self) -> &'static str;
}
```

Реализации (в `src/format/`):
- `rfc5424::build` → `Format::name = "rfc5424"`
- `rfc3164::build` → `Format::name = "rfc3164"`
- `raw::build` → `Format::name = "raw"`
- `protobuf::serialize_protobuf` → `Format::name = "protobuf"`

В вехе E (F15) добавим `Cef`, `Leef`, `JsonLines` форматы с trait-имплементациями.

## Trait `Transport` (планируется в веху E)

```rust
pub trait Transport: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, rx: SharedRx, metrics: Metrics, shutdown: CancellationToken) -> anyhow::Result<()>;
}
```

В вехе E (F16) добавим `Kafka`, `Redis`, persistent-queue с trait-имплементациями.
