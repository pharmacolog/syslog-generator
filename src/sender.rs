//! N10 (v8.8.0): backward-compat обёртка для src/transport/.
//!
//! Реальная реализация sender'ов переехала в `src/transport/` (file/tcp/udp/tls
//! подмодули). Этот модуль сохранён как thin re-export для backward-compat:
//! `syslog_generator::target_sender_file`, `target_sender_tcp` и т.д.
//! продолжают работать (используются в `core.rs::run_phase_multi` и тестах).

pub use crate::transport::{
    build_tls_connector, parse_tls_min_version, record_error, record_send, target_sender_file,
    target_sender_tcp, target_sender_tls, target_sender_udp, Framing, SharedRx, TlsParams,
};
