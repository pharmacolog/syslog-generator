//! N10 (v8.8.0): backward-compat обёртка для src/observability/metrics.
//!
//! Реальная реализация Prometheus-метрик переехала в
//! `src/observability/metrics.rs`. Этот модуль сохранён как thin re-export
//! для backward-compat: `syslog_generator::Metrics`, `create_metrics`,
//! `gather_metrics` продолжают работать.

pub use crate::observability::metrics::{create_metrics, gather_metrics, Metrics};
