//! N10 (v8.8.0): backward-compat обёртка для src/generator/config.
//!
//! Реальная реализация Profile/Phase/TargetConfig и load_profile_from_path
//! переехала в `src/generator/config.rs`. Этот модуль сохранён как thin
//! re-export для backward-compat: `syslog_generator::Profile` и др. продолжают
//! работать.

pub use crate::generator::{
    load_profile_from_json_str, load_profile_from_path, load_profile_from_yaml_str, Phase, Profile,
    ProtobufSchemaFieldMap, ShutdownConfig, SyslogConfig, TargetConfig,
};
