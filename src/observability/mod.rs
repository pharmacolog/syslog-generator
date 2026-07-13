//! N10 (v8.8.0): слой observability — Prometheus метрики + HTTP /metrics endpoint.
//!
//! - `metrics` — `Metrics` (struct), `create_metrics()`, `gather_metrics()`
//!   (F12, N3). Перенесён из `src/metrics.rs`.
//! - `server` — лёгкий HTTP-эндпоинт на `tokio` (F12). Перенесён из
//!   `src/metrics_server.rs` (parse_request_line, route, build_http_response,
//!   serve, spawn).
//!
//! Старые пути `syslog_generator::Metrics`, `syslog_generator::create_metrics`,
//! `syslog_generator::gather_metrics` сохранены как backward-compat
//! re-exports в `src/metrics.rs`.

pub mod metrics;
pub mod server;

// Re-exports для API, экспортируемого из `pub use` в `lib.rs`.
pub use metrics::{create_metrics, gather_metrics, Metrics};
pub use server::{build_http_response, parse_request_line, route, serve as serve_metrics};
