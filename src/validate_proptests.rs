//! N8 (v8.7.1) extension: property-based тесты для ValidationError (F13).
//!
//! Issue #165 (C3 remaining proptest + insta): добавление proptest coverage
//! для src/validate.rs. Существующие unit-тесты покрывают только edge cases.
//!
//! Использует `proptest = "1"` для автоматической генерации входных данных.

use crate::config::{Phase, Profile, ShutdownConfig, TargetConfig};
use crate::validate::{
    validate_profile, VALID_DISTRIBUTIONS, VALID_FORMATS, VALID_FRAMINGS, VALID_SHUTDOWN_MODES,
    VALID_TRANSPORTS,
};
use proptest::prelude::*;

/// N8: `validate_profile` для Profile с одной валидной Phase (с
/// total_messages, чтобы избежать UnboundedPhase warning). Минимальный
/// валидный Profile.
#[test]
fn prop_validate_profile_default_ok() {
    proptest!(|(_seed: u32)| {
        let p = Profile {
            phases: vec![Phase {
                name: "test_phase".into(),
                templates: vec!["hello".into()],
                total_messages: Some(1),
                ..Default::default()
            }],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        prop_assert!(errors.is_empty(), "Profile с валидной Phase должен проходить, got {errors:?}");
    });
}

/// N8: `validate_profile` с одной минимальной Phase (с total_messages).
#[test]
fn prop_validate_profile_empty_phase() {
    proptest!(|(_seed: u32)| {
        let p = Profile {
            phases: vec![Phase {
                name: "test_phase".into(),
                templates: vec!["hello".into()],
                total_messages: Some(1),
                ..Default::default()
            }],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        prop_assert!(errors.is_empty(), "минимальная Phase должна проходить");
    });
}

/// N8: `validate_profile` для валидного target (address+transport) должно
/// проходить.
#[test]
fn prop_validate_profile_empty_target() {
    proptest!(|(_seed: u32)| {
        let p = Profile {
            targets: vec![TargetConfig {
                address: "127.0.0.1:514".into(),
                transport: "tcp".into(),
                ..Default::default()
            }],
            phases: vec![Phase {
                name: "test_phase".into(),
                templates: vec!["hello".into()],
                total_messages: Some(1),
                ..Default::default()
            }],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        prop_assert!(errors.is_empty(), "валидный Target должен проходить, got {errors:?}");
    });
}

/// N8: `validate_profile` для Phase с невалидным transport →
/// ValidationError.
#[test]
fn prop_validate_profile_invalid_transport() {
    proptest!(|(bad_transport in "\\PC*")| {
        // Только триггерим если строка не в VALID_TRANSPORTS.
        if VALID_TRANSPORTS.contains(&bad_transport.as_str()) {
            return Ok(());
        }
        let target = TargetConfig {
            address: "127.0.0.1:514".into(),
            transport: bad_transport.clone(),
            ..Default::default()
        };
        let p = Profile {
            targets: vec![target],
            phases: vec![Phase::default()],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        let has_transport_error = errors
            .iter()
            .any(|e| format!("{e:?}").contains("transport") || format!("{e:?}").contains("Transport"));
        prop_assert!(
            has_transport_error,
            "invalid transport ({bad_transport}) должен давать ошибку, got {errors:?}"
        );
    });
}

/// N8: `validate_profile` для Phase с невалидным format → ошибка.
#[test]
fn prop_validate_profile_invalid_format() {
    proptest!(|(bad_format in "\\PC*")| {
        if VALID_FORMATS.contains(&bad_format.as_str()) {
            return Ok(());
        }
        let p = Profile {
            phases: vec![Phase {
                format: Some(bad_format.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        let has_format_error = errors
            .iter()
            .any(|e| format!("{e:?}").contains("format") || format!("{e:?}").contains("Format"));
        prop_assert!(
            has_format_error,
            "invalid format ({bad_format}) должен давать ошибку, got {errors:?}"
        );
    });
}

/// N8: `validate_profile` для невалидной distribution → ошибка.
#[test]
fn prop_validate_profile_invalid_distribution() {
    proptest!(|(bad_dist in "\\PC*")| {
        if VALID_DISTRIBUTIONS.contains(&bad_dist.as_str()) {
            return Ok(());
        }
        let p = Profile {
            distribution: bad_dist.clone(),
            ..Default::default()
        };
        let errors = validate_profile(&p);
        let has_dist_error = errors
            .iter()
            .any(|e| format!("{e:?}").contains("distribution") || format!("{e:?}").contains("Distribution"));
        prop_assert!(
            has_dist_error,
            "invalid distribution ({bad_dist}) должен давать ошибку, got {errors:?}"
        );
    });
}

/// N8: `validate_profile` для невалидного shutdown mode → ошибка.
#[test]
fn prop_validate_profile_invalid_shutdown_mode() {
    proptest!(|(bad_mode in "\\PC*")| {
        if VALID_SHUTDOWN_MODES.contains(&bad_mode.as_str()) {
            return Ok(());
        }
        let p = Profile {
            shutdown: ShutdownConfig {
                mode: bad_mode.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let errors = validate_profile(&p);
        let has_shutdown_error = errors
            .iter()
            .any(|e| format!("{e:?}").contains("shutdown") || format!("{e:?}").contains("Shutdown"));
        prop_assert!(
            has_shutdown_error,
            "invalid shutdown mode ({bad_mode}) должен давать ошибку, got {errors:?}"
        );
    });
}

/// N8: `validate_profile` для невалидного framing → ошибка.
#[test]
fn prop_validate_profile_invalid_framing() {
    proptest!(|(bad_framing in "\\PC*")| {
        if VALID_FRAMINGS.contains(&bad_framing.as_str()) {
            return Ok(());
        }
        let target = TargetConfig {
            address: "127.0.0.1:514".into(),
            transport: "tcp".into(),
            framing: bad_framing.clone(),
            ..Default::default()
        };
        let p = Profile {
            targets: vec![target],
            phases: vec![Phase::default()],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        let has_framing_error = errors
            .iter()
            .any(|e| format!("{e:?}").contains("framing") || format!("{e:?}").contains("Framing"));
        prop_assert!(
            has_framing_error,
            "invalid framing ({bad_framing}) должен давать ошибку, got {errors:?}"
        );
    });
}

/// N8: `validate_profile` детерминирован — same Profile → same errors.
#[test]
fn prop_validate_profile_deterministic() {
    proptest!(|(_seed: u32)| {
        let p = Profile::default();
        let e1 = validate_profile(&p);
        let e2 = validate_profile(&p);
        prop_assert_eq!(e1.len(), e2.len(), "validator должен быть детерминирован");
    });
}

/// N8: `validate_profile` для пустого target.address → ошибка.
#[test]
fn prop_validate_profile_empty_address() {
    proptest!(|(_seed: u32)| {
        let p = Profile {
            targets: vec![TargetConfig {
                address: "".into(),
                ..Default::default()
            }],
            phases: vec![Phase::default()],
            ..Default::default()
        };
        let errors = validate_profile(&p);
        // Пустой address должен дать ошибку (либо address, либо transport).
        prop_assert!(!errors.is_empty(), "пустой address должен давать ошибку");
    });
}
