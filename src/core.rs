//! N10 (v8.8.0): backward-compat обёртка для src/generator/.
//!
//! Реальная реализация run_profile/run_phase_multi/generate_message переехала
//! в `src/generator/core.rs`. Этот модуль сохранён как thin re-export
//! для backward-compat: `syslog_generator::run_profile` и др. продолжают
//! работать.

pub use crate::generator::{
    create_dispatcher, default_values, generate_message, load_schema, load_templates,
    run_phase_multi, run_profile,
};
