//! N10 (v8.8.0): backward-compat обёртка для src/observability/server.
//!
//! Реальная реализация HTTP-эндпоинта /metrics переехала в
//! `src/observability/server.rs`. Этот модуль сохранён как thin re-export
//! для backward-compat: `parse_request_line`, `route`, `build_http_response`,
//! `serve`, `spawn` продолжают работать (используется в integration
//! тестах D3 — `syslog_generator::metrics_server::spawn`).

pub use crate::observability::server::{
    build_http_response, parse_request_line, route, serve as serve_metrics, spawn,
};
