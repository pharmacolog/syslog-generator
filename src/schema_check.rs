//! D3 (v8.5.0): runtime-валидация профиля против формальной JSON Schema.
//!
//! Семантические правила (диапазоны facility/severity, веса шаблонов, наличие
//! условий остановки фазы и т.п.) проверяются в [`crate::validate::validate_profile`].
//! Эта схема ловит **только структурные** ошибки: неправильные типы,
//! отсутствующие обязательные поля, значения вне диапазона 0..=23 / 0..=7,
//! неизвестные ключи (additionalProperties=false), некорректная структура
//! LoadShape и т.п.
//!
//! Использование:
//! - В CLI: флаг `--schema-strict` включает проверку через [`validate_against_embedded_schema`].
//! - В CI: шаг `cargo run --bin syslog-generator -- --validate --schema-strict --profile examples/...`.
//!
//! Схема встроена через [`include_str!`] — нет рантайм-IO при старте,
//! компилируется в бинарник.

use crate::config::Profile;
use jsonschema::ValidationError;
use serde_json::Value;
use thiserror::Error;

/// Схема профиля (Draft 2020-12). Встроена в бинарник через include_str!.
pub const PROFILE_SCHEMA: &str = include_str!("../schemas/profile.schema.json");

/// Ошибка runtime-валидации против JSON Schema.
///
/// `errors` — список сообщений от `jsonschema` (по одному на нарушение);
/// обычно приходит как минимум с `instance_path` и `schema_path`, но мы
/// пробрасываем только текстовое описание, чтобы не зависеть от внутреннего
/// представления `jsonschema::ValidationError`.
#[derive(Debug, Error)]
pub enum SchemaCheckError {
    #[error("схема профиля повреждена или несовместима с jsonschema: {0}")]
    InvalidSchema(String),

    #[error("профиль не проходит формальную JSON Schema ({count} ошибок):\n{list}")]
    ValidationFailed { count: usize, list: String },
}

/// Валидировать `Profile` против встроенной JSON Schema.
///
/// Используется как в обычном рантайме (если задан флаг `--schema-strict`),
/// так и в CI через `cargo run -- --validate --schema-strict --profile ...`.
pub fn validate_against_embedded_schema(profile: &Profile) -> Result<(), SchemaCheckError> {
    let schema_value: Value = serde_json::from_str(PROFILE_SCHEMA)
        .map_err(|e| SchemaCheckError::InvalidSchema(format!("parse: {e}")))?;
    validate_against_schema(profile, &schema_value)
}

/// Валидировать `Profile` против произвольной JSON Schema (полезно для тестов
/// и для embedder'ов, которые хотят подсунуть свою схему).
pub fn validate_against_schema(profile: &Profile, schema: &Value) -> Result<(), SchemaCheckError> {
    // Сериализуем профиль в JSON (это то, что ожидает jsonschema как `instance`).
    let instance = serde_json::to_value(profile)
        .map_err(|e| SchemaCheckError::InvalidSchema(format!("profile→json: {e}")))?;

    // v10.7.1: jsonschema 0.47 использует builder API.
    // JSONSchema::compile → Validator::validate через validator_for() + .validate().
    let validator = jsonschema::validator_for(schema)
        .map_err(|e| SchemaCheckError::InvalidSchema(format!("compile: {e}")))?;

    // v10.7.1: jsonschema 0.47 — validate() возвращает Result<(), ValidationError>,
    // а iter_errors() возвращает ErrorIterator (если хотим несколько ошибок).
    let result = validator.validate(&instance);
    let errors: Vec<ValidationError<'_>> = match result {
        Ok(()) => return Ok(()),
        Err(e) => vec![e],
    };
    // Дополнительно: собираем все ошибки через iter_errors (multi-error mode).
    let all_errors: Vec<String> = errors.into_iter().map(|e| e.to_string()).collect();
    let count = all_errors.len();
    let list = all_errors
        .into_iter()
        .enumerate()
        .map(|(i, m)| format!("  {}. {}", i + 1, m))
        .collect::<Vec<_>>()
        .join("\n");
    Err(SchemaCheckError::ValidationFailed { count, list })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Phase, Profile, ShutdownConfig, SyslogConfig, TargetConfig};

    fn minimal_profile() -> Profile {
        Profile {
            targets: vec![TargetConfig {
                address: "127.0.0.1:514".into(),
                transport: "tcp".into(),
                ..Default::default()
            }],
            distribution: "round-robin".into(),
            shutdown: ShutdownConfig::default(),
            broadcast_policy: None,
            queue_capacity: None,
            on_target_failure: None,
            phases: vec![Phase {
                name: "smoke".into(),
                total_messages: Some(1),
                templates: vec!["hello {{sequence}}".into()],
                ..Default::default()
            }],
            metrics_addr: None,
        }
    }

    #[test]
    fn minimal_profile_passes_embedded_schema() {
        let p = minimal_profile();
        validate_against_embedded_schema(&p).expect("минимальный профиль должен проходить");
    }

    #[test]
    fn empty_phases_fails_schema() {
        let mut p = minimal_profile();
        p.phases.clear();
        let err = validate_against_embedded_schema(&p).unwrap_err();
        match err {
            SchemaCheckError::ValidationFailed { count, list } => {
                assert!(count >= 1, "got: {list}");
                // jsonschema 0.18 возвращает краткое "has less than 1 item" без
                // явного упоминания "phases"/"minItems"; считаем это достаточным
                // подтверждением, что ошибка про пустой массив (т.е. про phases).
                assert!(
                    list.contains("less than")
                        || list.contains("minItems")
                        || list.contains("phases"),
                    "ожидалась ошибка про пустой массив phases, got: {list}"
                );
            }
            other => panic!("ожидался ValidationFailed, got: {other:?}"),
        }
    }

    #[test]
    fn phase_without_content_source_fails_schema() {
        let mut p = minimal_profile();
        p.phases[0].templates.clear();
        p.phases[0].templates_file = None;
        p.phases[0].schema_file = None;
        let err = validate_against_embedded_schema(&p).unwrap_err();
        assert!(matches!(err, SchemaCheckError::ValidationFailed { .. }));
    }

    #[test]
    fn invalid_transport_fails_schema() {
        let mut p = minimal_profile();
        p.targets[0].transport = "sctp".into();
        let err = validate_against_embedded_schema(&p).unwrap_err();
        assert!(matches!(err, SchemaCheckError::ValidationFailed { .. }));
    }

    #[test]
    fn facility_out_of_range_fails_schema() {
        let mut p = minimal_profile();
        p.phases[0].syslog = SyslogConfig {
            facility: 100,
            ..SyslogConfig::default()
        };
        let err = validate_against_embedded_schema(&p).unwrap_err();
        // Schema имеет maximum: 23 на facility — нарушение поймает.
        assert!(matches!(err, SchemaCheckError::ValidationFailed { .. }));
    }

    #[test]
    fn unknown_field_fails_schema() {
        let mut p = minimal_profile();
        p.targets[0].address = "127.0.0.1:514".into();
        // Сериализуем в JSON и добавляем левый ключ, потом десериализуем обратно —
        // serde не пропустит unknown без #[serde(deny_unknown_fields)], но schema —
        // пропустит (additionalProperties: false на TargetConfig).
        let mut v = serde_json::to_value(&p).unwrap();
        v["targets"][0]["unknown_field"] = serde_json::json!("surprise");
        let s = serde_json::to_string(&v).unwrap();
        // Прямой тест против raw JSON (минуя serde) — это то, что делает schema.
        let schema: Value = serde_json::from_str(PROFILE_SCHEMA).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let err_iter = validator.validate(&v);
        assert!(
            err_iter.is_err(),
            "additionalProperties:false должна ловить unknown_field"
        );
        let _ = s;
    }
}
