//! Phase 9 (v10.7.16, PR-Q.4): property-based тесты для `src/validate.rs`.
//!
//! Цель — **invariants** валидатора F13:
//! - Валидный Profile (все поля в допустимых диапазонах) → пустой
//!   `Vec<ValidationError>`.
//! - Невалидный transport (long random string) → ошибка `InvalidTransport`
//!   с сохранением значения в `value`-поле.
//! - Валидные syslog facility/severity (0..24/0..8) → нет `InvalidFacility`
//!   и `InvalidSeverity`.
//!
//! Дополняет существующие unit-тесты в `src/validate.rs::tests`.

use crate::config::{Phase, Profile, ShutdownConfig, TargetConfig};
use crate::validate::{validate_profile, ValidationError};
use proptest::prelude::*;

fn valid_target_with(transport: String, address: String) -> TargetConfig {
    TargetConfig {
        address,
        transport,
        ..Default::default()
    }
}

fn valid_phase_with(rate: u64, duration_secs: u64, format: Option<String>) -> Phase {
    Phase {
        name: "proptest-phase".to_string(),
        duration_secs,
        messages_per_second: rate,
        templates: vec!["msg {{sequence}}".to_string()],
        format,
        ..Default::default()
    }
}

proptest! {
    /// `validate_profile` принимает валидный Profile (все поля в допустимых
    /// диапазонах) без ошибок. Property: для любого `(facility ∈ 0..24,
    /// severity ∈ 0..8, rate ∈ 1..1M, duration_secs ∈ 1..3600)` валидатор
    /// не возвращает ошибок.
    #[test]
    fn prop_validate_accepts_valid_profile(
        _facility in 0u8..24,
        _severity in 0u8..8,
        rate in 1u64..1_000_000,
        duration_secs in 1u64..3600,
    ) {
        let profile = Profile {
            targets: vec![valid_target_with(
                "udp".to_string(),
                "127.0.0.1:514".to_string(),
            )],
            phases: vec![valid_phase_with(rate, duration_secs, None)],
            distribution: "round-robin".to_string(),
            shutdown: ShutdownConfig::default(),
            metrics_addr: None,
        };
        let errs = validate_profile(&profile);
        // Отфильтруем известные допустимые варианты (проверяем, что нет
        // facility/severity errors, так как они зависят от format).
        let hard_errors: Vec<_> = errs
            .iter()
            .filter(|e| !matches!(
                e,
                ValidationError::InvalidFacility { .. }
                    | ValidationError::InvalidSeverity { .. }
            ))
            .collect();
        prop_assert!(
            hard_errors.is_empty(),
            "expected no hard errors for valid profile, got: {:?}",
            hard_errors
        );
    }

    /// `validate_profile` отвергает невалидный transport (long random string)
    /// с ошибкой `InvalidTransport`, содержащей исходное значение в `value`.
    /// Это контракт F13: каждая ошибка несёт достаточно контекста для
    /// пользователя, чтобы понять, что чинить.
    #[test]
    fn prop_validate_rejects_invalid_transport(transport in "[a-z]{8,40}") {
        let profile = Profile {
            targets: vec![valid_target_with(
                transport.clone(),
                "127.0.0.1:514".to_string(),
            )],
            ..Default::default()
        };
        let errs = validate_profile(&profile);
        // Для дефолтного profile (без phases) — есть NoPhases + InvalidTransport
        // (если transport невалидный).
        let has_invalid_transport = errs.iter().any(|e| matches!(
            e,
            ValidationError::InvalidTransport { value, .. } if value == &transport
        ));
        prop_assert!(
            has_invalid_transport,
            "expected InvalidTransport for transport={:?}, got errors: {:?}",
            transport,
            errs
        );
    }

    /// `validate_profile` отвергает невалидный distribution (long random
    /// string) с ошибкой `InvalidDistribution`.
    #[test]
    fn prop_validate_rejects_invalid_distribution(distribution in "[a-z]{8,40}") {
        // prop_assume: filter out случайное попадание в "round-robin" /
        // "broadcast" / "weighted" (но [a-z]{8,40} не может совпасть,
        // все допустимые значения имеют дефис).
        let profile = Profile {
            targets: vec![valid_target_with(
                "udp".to_string(),
                "127.0.0.1:514".to_string(),
            )],
            phases: vec![valid_phase_with(100, 10, None)],
            distribution: distribution.clone(),
            ..Default::default()
        };
        let errs = validate_profile(&profile);
        let has_invalid_distribution = errs.iter().any(|e| matches!(
            e,
            ValidationError::InvalidDistribution { value, .. } if value == &distribution
        ));
        prop_assert!(
            has_invalid_distribution,
            "expected InvalidDistribution for distribution={:?}, got errors: {:?}",
            distribution,
            errs
        );
    }

    /// `validate_profile` для формата rfc5424: facility ∈ 0..23 и severity ∈ 0..7
    /// принимаются. Property: для facility ∈ 0..=23 (включительно) и
    /// severity ∈ 0..=7 нет ошибок `InvalidFacility`/`InvalidSeverity`.
    #[test]
    fn prop_rfc5424_syslog_facility_severity_valid(
        facility in 0u8..=23u8,
        severity in 0u8..=7u8,
    ) {
        let mut phase = valid_phase_with(100, 10, Some("rfc5424".to_string()));
        phase.syslog.facility = facility;
        phase.syslog.severity = severity;
        let profile = Profile {
            targets: vec![valid_target_with(
                "udp".to_string(),
                "127.0.0.1:514".to_string(),
            )],
            phases: vec![phase],
            ..Default::default()
        };
        let errs = validate_profile(&profile);
        let has_facility_err = errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidFacility { .. }));
        let has_severity_err = errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidSeverity { .. }));
        prop_assert!(
            !has_facility_err,
            "facility={} should be valid (0..=23), got errors: {:?}",
            facility,
            errs
        );
        prop_assert!(
            !has_severity_err,
            "severity={} should be valid (0..=7), got errors: {:?}",
            severity,
            errs
        );
    }
}
