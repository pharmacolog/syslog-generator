//! N10 (v8.8.0): переработанный lib.rs с явными слоями.
//!
//! Структура (от внешнего слоя к внутреннему):
//! - `cli` — парсинг аргументов командной строки (clap).
//! - `validate` — F13 семантическая валидация профиля.
//! - `schema_check` — D3 структурная JSON Schema валидация.
//! - `format` — форматы (RFC 5424, RFC 3164, raw, protobuf, cef, leef, json_lines).
//! - `transport` — транспорты (file, tcp, udp, tls).
//! - `generator` — оркестрация (run_profile, run_phase_multi, generate_message,
//!   конфиг Profile/Phase/TargetConfig, загрузка профиля).
//! - `observability` — метрики (metrics + metrics_server HTTP /metrics).
//! - `error` — типизированные ошибки (RuntimeError и под-типы).
//! - `payload` — генератор payload (F4-F6, F14: faker, regex, корреляции).
//! - `template` — рендеринг `{{placeholder}}` (CompiledTemplate, F5).
//! - `schema` — загрузка schema.json для F5 schema-per-phase.
//! - `load_shape` — F3 профили нагрузки во времени.
//! - `shutdown` — graceful drain и shutdown listener.
//!
//! Старые модули `core`, `config`, `sender`, `syslog`, `metrics`,
//! `metrics_server`, `protobuf` сохранены как backward-compat обёртки
//! (thin re-export из новых слоёв). Сигнатура публичного API не меняется.

// N4.cipher_policy (v9.5.0): rustls 0.23 требует явной установки
// crypto provider'а (мы используем ring). Делаем лениво через Once —
// вызывается при первом обращении к TLS API. Если пользователь вызывает
// только non-TLS функции — provider не нужен.
static RUSTLS_PROVIDER_INIT: std::sync::Once = std::sync::Once::new();

pub(crate) fn ensure_rustls_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Публичный wrapper для интеграционных тестов, которые строят TLS-сервер
/// напрямую через rustls (минуя наш `build_tls_connector`). Безопасный к
/// множественным вызовам.
pub fn ensure_rustls_provider_for_tests() {
    ensure_rustls_provider();
}

pub mod cli;
pub mod error;
pub mod format;
pub mod generator;
pub mod load_shape;
pub mod observability;
pub mod payload;
#[cfg(test)]
mod payload_proptests;
pub mod schema;
pub mod schema_check;
pub mod shutdown;
pub mod template;
pub mod transport;
pub mod validate;

// Backward-compat обёртки — старые имена модулей переэкспортируют API из
// новых слоёв. Это сохраняет `syslog_generator::run_profile`, `Profile` и т.д.
pub mod config; // → src/config.rs: pub use crate::generator::*
pub mod core; // → src/core.rs: pub use crate::generator::*
pub mod metrics; // → src/metrics.rs: pub use crate::observability::metrics::*
pub mod metrics_server; // → src/metrics_server.rs: pub use crate::observability::server::*
pub mod protobuf;
pub mod sender; // → src/sender.rs: pub use crate::transport::*
pub mod syslog; // → src/syslog.rs: pub use crate::format::* // → src/protobuf.rs: pub use crate::format::protobuf::*

// === Re-exports: новые слои (предпочтительные пути для нового кода) ===

pub use cli::{apply_overrides, parse_target, Args, Overrides};
pub use error::{ConfigError, DrainError, MetricsError, RuntimeError};
pub use format::{
    build_rfc3164, build_rfc5424, escape_sd_value, prival, raw, rfc3164, rfc5424, Header,
};
pub use generator::{
    create_dispatcher, default_values, generate_message, generate_message_with_format,
    load_profile_from_json_str, load_profile_from_path, load_profile_from_yaml_str, load_schema,
    load_templates, run_phase_multi, run_profile, CefConfig, LeefConfig, Phase, Profile,
    ProtobufSchemaFieldMap, ShutdownConfig, SyslogConfig, TargetConfig,
};
pub use load_shape::LoadShape;
pub use observability::{
    build_http_response, create_metrics, gather_metrics, parse_request_line, route, serve_metrics,
    Metrics,
};
pub use payload::{
    derive_rng, faker, gen_from_regex, int_in_range, pad_to_size, random_string, weighted_index,
    zipf_index,
};
pub use schema::{Schema, SchemaField};
pub use schema_check::{
    validate_against_embedded_schema, validate_against_schema, SchemaCheckError, PROFILE_SCHEMA,
};
pub use shutdown::{graceful_drain_wait, shutdown_listener};
pub use template::render_template;
pub use transport::{
    build_tls_connector, parse_cipher_suite, parse_tls_min_version, record_send,
    target_sender_file, target_sender_tcp, target_sender_tls, target_sender_udp, Framing, SharedRx,
    TlsParams, TlsVersion,
};
pub use validate::{format_errors, validate_profile, ValidationError};

// Re-exports из backward-compat обёрток для имён, которых НЕТ в основных
// re-exports. Например, `format::prival` уже в `pub use format::...` выше.
// protobuf re-exports нужен: `pub use generator::...` не покрывает.
pub use self::protobuf::{
    apply_protobuf_schema, serialize_protobuf, serialize_protobuf_like, PbType,
};
