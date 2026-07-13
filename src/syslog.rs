//! N10 (v8.8.0): backward-compat обёртка для src/format/.
//!
//! Реальная реализация RFC 5424/3164 переехала в `src/format/`.
//! Этот модуль сохранён как thin re-export для backward-compat:
//! `syslog_generator::build_rfc5424` и `syslog_generator::build_rfc3164`
//! продолжают работать (используются в тестах и в `core.rs::run_phase_multi`).

pub use crate::format::{
    build_rfc3164, build_rfc5424, escape_sd_value, prival, rfc3164, rfc5424, Header,
};
