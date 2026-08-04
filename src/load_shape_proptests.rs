//! N8 (v8.7.1) extension: property-based тесты для LoadShape (F3).
//!
//! Issue #165 (C3 remaining proptest + insta): добавление proptest coverage
//! для src/load_shape.rs. Существующие unit-тесты покрывают только edge cases.
//!
//! Использует `proptest = "1"` для автоматической генерации входных данных.
//!
//! Ограничиваем диапазоны чтобы избежать subnormal/NaN в f64 (proptest
//! может генерировать очень маленькие значения вроде 1e-265 которые
//! дают ложные negative результаты в assertions).

use crate::load_shape::LoadShape;
use proptest::prelude::*;

// Ограниченные диапазоны (subnormal/NaN filtering ниже).
fn safe_f64() -> impl Strategy<Value = f64> {
    (0.0f64..1_000_000.0_f64).prop_filter("finite", |v: &f64| v.is_finite())
}

fn safe_optional_f64() -> impl Strategy<Value = Option<f64>> {
    proptest::option::of(safe_f64())
}

/// N8: `rate_at` для `Constant` всегда возвращает `rate.unwrap_or(base_rate)`.
/// Используем ограниченные диапазоны чтобы избежать subnormal floats.
#[test]
fn prop_rate_at_constant() {
    proptest!(|(rate_opt in safe_optional_f64(), base_rate in safe_f64(), t_secs in -100.0f64..10000.0f64)| {
        let shape = LoadShape::Constant { rate: rate_opt };
        let expected = rate_opt.unwrap_or(base_rate);
        let actual = shape.rate_at(t_secs, 60.0, base_rate);
        let diff = (actual - expected).abs();
        prop_assert!(
            diff < 1e-6,
            "rate_at для Constant должен быть rate.unwrap_or(base_rate): actual={actual} expected={expected}"
        );
    });
}

/// N8: `rate_at` для `Linear` при t=0 возвращает `start_rate`, при t=duration
/// возвращает `end_rate`, в промежутке линейная интерполяция.
#[test]
fn prop_rate_at_linear() {
    proptest!(|(start_rate in safe_f64(), end_rate in safe_f64(), duration in 0.1f64..3600.0f64, t_secs in 0.0f64..3600.0f64)| {
        let shape = LoadShape::Linear { start_rate, end_rate };
        let actual = shape.rate_at(t_secs, duration, 0.0);
        if t_secs <= 0.0 {
            let diff = (actual - start_rate).abs();
            prop_assert!(diff < 1e-6, "t<=0 → start_rate, got {actual}");
        } else if t_secs >= duration {
            let diff = (actual - end_rate).abs();
            prop_assert!(diff < 1e-6, "t>=duration → end_rate, got {actual}");
        } else {
            // Линейная интерполяция: start + (end - start) * (t / duration).
            let expected = start_rate + (end_rate - start_rate) * (t_secs / duration);
            let diff = (actual - expected).abs();
            // Погрешность для f64.
            prop_assert!(diff < 1e-3, "линейная интерполяция: actual={actual} expected={expected}");
        }
    });
}

/// N8: `rate_at` для `Linear` при phase_duration_secs == 0 — без длительности
/// интерполяция не определена, должен вернуть `end_rate`.
#[test]
fn prop_rate_at_linear_zero_duration() {
    proptest!(|(start_rate in safe_f64(), end_rate in safe_f64(), t_secs in 0.0f64..10000.0f64)| {
        let shape = LoadShape::Linear { start_rate, end_rate };
        let actual = shape.rate_at(t_secs, 0.0, 0.0);
        let diff = (actual - end_rate).abs();
        prop_assert!(diff < 1e-6, "phase_duration_secs=0 → end_rate, got {actual}");
    });
}

/// N8: `rate_at` для `Sine` всегда в диапазоне `[min_rate, max_rate]`.
#[test]
fn prop_rate_at_sine_bounds() {
    proptest!(|(min_rate in safe_f64(), max_rate in safe_f64(), period in 1.0f64..1000.0f64, t_secs in 0.0f64..10000.0f64)| {
        // Гарантируем min_rate <= max_rate.
        let (lo, hi) = if min_rate <= max_rate { (min_rate, max_rate) } else { (max_rate, min_rate) };
        let shape = LoadShape::Sine { min_rate: lo, max_rate: hi, period_secs: period };
        let actual = shape.rate_at(t_secs, 0.0, 0.0);
        // Допуск на f64 precision.
        prop_assert!(actual >= lo - 1e-6, "rate_at Sine должен быть >= min_rate, got {actual}");
        prop_assert!(actual <= hi + 1e-6, "rate_at Sine должен быть <= max_rate, got {actual}");
    });
}

/// N8: `rate_at` для `Burst` при t вне burst window возвращает `base_rate`.
/// Используем `t_secs = (N + 0.5) * every` где N = floor(t_offset) для
/// гарантии gap (при условии `burst_secs < every / 2`).
#[test]
fn prop_rate_at_burst_outside_window() {
    proptest!(|(base_rate in safe_f64(), burst_rate in safe_f64(), every in 10.0f64..100.0f64, burst_secs in 0.1f64..4.9f64, t_offset in 0.0f64..1000.0f64)| {
        let shape = LoadShape::Burst { base_rate, burst_rate, every_secs: every, burst_secs };
        // Гарантируем burst_secs < every / 2 (есть gap между bursts).
        prop_assume!(burst_secs < every / 2.0);
        // t в середине gap-периода: t = N * every + every/2 для некоторого N.
        let t_out = (t_offset.floor() + 0.5) * every;
        let actual = shape.rate_at(t_out, 0.0, 0.0);
        let diff = (actual - base_rate).abs();
        prop_assert!(diff < 1e-6, "вне burst window → base_rate, got {actual}");
    });
}

/// N8: `rate_at` для `Burst` в burst window возвращает `burst_rate`.
/// Используем `t = N * every + burst_secs / 2`, что в середине burst.
#[test]
fn prop_rate_at_burst_inside_window() {
    proptest!(|(base_rate in safe_f64(), burst_rate in safe_f64(), every in 10.0f64..100.0f64, burst_secs in 0.5f64..9.0f64, n_bursts in 0u32..5u32)| {
        let shape = LoadShape::Burst { base_rate, burst_rate, every_secs: every, burst_secs };
        // Гарантируем что t внутри burst window: t в [N*every, N*every + burst_secs).
        let burst_start = (n_bursts as f64) * every;
        let t_in = burst_start + burst_secs * 0.5;
        let actual = shape.rate_at(t_in, 0.0, 0.0);
        let diff = (actual - burst_rate).abs();
        prop_assert!(diff < 1e-6, "внутри burst window → burst_rate, got {actual}");
    });
}

/// N8: `rate_at` всегда неотрицательное (>= 0).
#[test]
fn prop_rate_at_non_negative() {
    proptest!(|(start in safe_f64(), end in safe_f64(), t in -100.0f64..10000.0f64)| {
        let shapes = vec![
            LoadShape::Constant { rate: Some(start) },
            LoadShape::Linear { start_rate: start, end_rate: end },
            LoadShape::Sine { min_rate: start.min(end), max_rate: start.max(end), period_secs: 60.0 },
        ];
        for shape in &shapes {
            let r = shape.rate_at(t, 60.0, 0.0);
            prop_assert!(r >= 0.0, "rate_at должен быть >= 0, got {r} for shape {shape:?}");
        }
    });
}
