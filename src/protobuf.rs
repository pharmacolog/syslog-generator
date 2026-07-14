//! Backward-compat thin re-export для protobuf API.
//!
//! Реальная реализация — в [`crate::format::protobuf`] (canonical source).
//! Этот модуль сохранён для backward-compat: пользователи, импортировавшие
//! `syslog_generator::protobuf::{serialize_protobuf, apply_protobuf_schema,
//! PbType, serialize_protobuf_like}`, продолжат работать.
//!
//! История:
//! - До v8.8.0: единственная реализация в `src/protobuf.rs`.
//! - v8.8.0 (N10): рефакторинг слоёв, реализация перенесена в `src/format/protobuf.rs`,
//!   но `src/protobuf.rs` ошибочно оставлен как **полная копия** (354 строки
//!   дублирующего кода) вместо thin re-export.
//! - v10.7.x (PR-1): исправлено — `src/protobuf.rs` заменён на thin re-export.

pub use crate::format::protobuf::{
    apply_protobuf_schema, resolve_fields, serialize_protobuf, serialize_protobuf_like,
    write_varint, PbType,
};
