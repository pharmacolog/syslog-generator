//! N10 (v8.8.0): слой generator — оркестрация профиля, генерация сообщений,
//! запуск фаз.
//!
//! - `core` — `run_profile`, `run_phase_multi`, `generate_message`,
//!   `create_dispatcher`, `default_values`, `load_schema`, `load_templates`
//!   (перенесён из `src/core.rs`).
//! - `config` — `Profile`, `Phase`, `TargetConfig`, `load_profile_from_path`,
//!   `load_profile_from_json_str`, `load_profile_from_yaml_str`
//!   (перенесён из `src/config.rs`).
//!
//! Старые пути `syslog_generator::run_profile`, `syslog_generator::Profile` и
//! т.д. сохранены как backward-compat re-exports в `src/core.rs` и
//! `src/config.rs`.

pub mod config;
pub mod core;

// Re-exports для API, экспортируемого из `pub use` в `lib.rs`.
pub use config::{
    load_profile_from_json_str, load_profile_from_path, load_profile_from_yaml_str, Phase, Profile,
    ProtobufSchemaFieldMap, ShutdownConfig, SyslogConfig, TargetConfig,
};
pub use core::{
    create_dispatcher, default_values, generate_message, load_schema, load_templates,
    run_phase_multi, run_profile,
};
