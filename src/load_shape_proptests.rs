//! Phase 9 (v10.7.16, PR-Q.4): property-based тесты для `src/load_shape.rs`.
//!
//! Цель — **invariants** кривых интенсивности во времени:
//! - `Linear` ramp монотонно растёт (или убывает) между `start_rate` и `end_rate`.
//! - `Sine` rate всегда >= `min_rate` (без отрицательных значений, защита от
//!   деления на ноль в интервальной петле).
//! - `Burst` средняя интенсивность за полный цикл согласована с параметрами
//!   (burst_rate × burst_secs + base_rate × (cycle - burst_secs)) / cycle.
//!
//! Дополняет существующие unit-тесты в `src/load_shape.rs::tests`.

use crate::load_shape::LoadShape;
use proptest::prelude::*;

proptest! {
    /// `Linear { start, end, duration }`: rate монотонно НЕ убывает если start <= end,
    /// и монотонно НЕ возрастает если start >= end. Между `min(start, end)` и
    /// `max(start, end)`. После duration — держим `end`.
    #[test]
    fn prop_linear_ramp_monotonic(
        base_rate in 1u64..1000,
        peak_rate in 1u64..1000,
        duration_secs in 1u64..1000,
    ) {
        let (start, end) = if base_rate <= peak_rate {
            (base_rate as f64, peak_rate as f64)
        } else {
            (peak_rate as f64, base_rate as f64)
        };
        let shape = LoadShape::Linear { start_rate: start, end_rate: end };
        let min_r = start.min(end);
        let max_r = start.max(end);

        let mut prev = shape.rate_at(0.0, duration_secs as f64, 0.0);
        prop_assert!(
            (prev - start).abs() < 1e-6,
            "rate_at(0) должен быть start={}, got {}",
            start, prev
        );
        for t in 0..=duration_secs {
            let rate = shape.rate_at(t as f64, duration_secs as f64, 0.0);
            // Инвариант: в пределах [min_r, max_r]
            prop_assert!(
                rate >= min_r - 1e-6,
                "rate {} < min(start, end)={} at t={}",
                rate, min_r, t
            );
            prop_assert!(
                rate <= max_r + 1e-6,
                "rate {} > max(start, end)={} at t={}",
                rate, max_r, t
            );
            // Монотонность: rate(t+1) >= rate(t) (для start <= end)
            if end >= start {
                prop_assert!(
                    rate >= prev - 1e-6,
                    "rate должен быть не убывающим: rate({})={} < prev={}",
                    t, rate, prev
                );
            } else {
                prop_assert!(
                    rate <= prev + 1e-6,
                    "rate должен быть не возрастающим: rate({})={} > prev={}",
                    t, rate, prev
                );
            }
            prev = rate;
        }
        // После duration — держим end
        let after = shape.rate_at((duration_secs as f64) + 100.0, duration_secs as f64, 0.0);
        prop_assert!(
            (after - end).abs() < 1e-6,
            "rate после duration должен быть end={}, got {}",
            end, after
        );
    }

    /// `Sine { base, amplitude, period }`: rate >= 0 всегда.
    /// base = floor, amplitude = максимальное отклонение; mid = base + amplitude,
    /// min = base (теоретически), max = base + 2*amplitude. Реальный rate =
    /// mid - amplitude * cos(2π·t/period) ∈ [base, base + 2*amplitude].
    /// С `base >= 1` и `amplitude < base` rate >= 1 - amplitude >= -499 ...
    /// поэтому используем `rate_at(t)` и проверяем rate >= 0 (функция
    /// дополнительно клампит `r.max(0.0)` в load_shape.rs:118).
    #[test]
    fn prop_sine_rate_non_negative(
        base in 1u64..1000,
        amplitude in 0u64..500,
        period in 1u64..3600,
    ) {
        // min_rate = base (0 в терминах кода), max_rate = base + amplitude
        // Чтобы получить минимум = base, в коде используется mid - amp*cos.
        // mid = base + amplitude/2, amp = amplitude/2.
        // Минимум: mid - amp = base, максимум: mid + amp = base + amplitude.
        // Для property: достаточно проверить rate >= 0.
        let min_rate = base as f64;
        let max_rate = (base + amplitude) as f64;
        let shape = LoadShape::Sine {
            min_rate,
            max_rate,
            period_secs: period as f64,
        };
        for t in 0..10_000u64 {
            let rate = shape.rate_at(t as f64, 0.0, 0.0);
            prop_assert!(
                rate >= 0.0,
                "rate must be >= 0, got {} at t={}",
                rate, t
            );
            // Верхняя граница: rate <= max_rate (из определения sine)
            prop_assert!(
                rate <= max_rate + 1e-6,
                "rate must be <= max_rate={}, got {} at t={}",
                max_rate, rate, t
            );
            // Нижняя граница: rate >= min_rate (после клампа кода)
            // Если min_rate < 0, код клампит к 0, но мы передаём min_rate >= 1.
            prop_assert!(
                rate >= min_rate - 1e-6,
                "rate must be >= min_rate={}, got {} at t={}",
                min_rate, rate, t
            );
        }
    }

    /// `Burst`: средняя интенсивность за полный цикл согласована с
    /// параметрами. Точная формула:
    /// `avg = (burst_rate * burst_secs + base_rate * (cycle - burst_secs)) / cycle`
    /// Для `burst_rate >> base_rate` и `burst_secs << cycle` среднее близко
    /// к `base_rate` (всплески — это пики, не устойчивое повышение).
    /// Допуск: ±10% (для выборки 100 циклов стохастическая погрешность мала,
    /// но мы интегрируем непрерывно — должно быть точное совпадение).
    #[test]
    fn prop_burst_average_equals_expected(
        base in 10u64..1000,
        burst_rate in 100u64..10000,
        burst_secs in 1u64..10,
        every_secs in 10u64..100,
    ) {
        // Гарантия: burst_secs <= every_secs (иначе burst перекрывает весь цикл).
        // Не prop_assume! — clamp'им значения чтобы тест остался детерминированным.
        let cycle = every_secs as f64;
        let burst_dur = (burst_secs as f64).min(cycle);
        let shape = LoadShape::Burst {
            base_rate: base as f64,
            burst_rate: burst_rate as f64,
            every_secs: cycle,
            burst_secs: burst_dur,
        };
        // Аналитическая средняя:
        // avg_exact = (burst_rate * burst_dur + base * (cycle - burst_dur)) / cycle
        let avg_exact = (burst_rate as f64 * burst_dur
            + base as f64 * (cycle - burst_dur)) / cycle;
        // Симулируем 100 циклов (1000 секунд) и усредняем.
        let total_secs = 100u64 * every_secs;
        let sum: f64 = (0..total_secs)
            .map(|t| shape.rate_at(t as f64, 0.0, 0.0))
            .sum();
        let avg_simulated = sum / total_secs as f64;
        // ±5% tolerance: на 100 циклах погрешность дискретизации (один
        // шаг на секунду) накапливается, но burst_secs / cycle <= 10/10 = 1.
        // В пределе burst_secs == every_secs avg = burst_rate полностью.
        let tolerance = avg_exact.abs() * 0.05 + 0.5;
        prop_assert!(
            (avg_simulated - avg_exact).abs() <= tolerance,
            "burst average: expected {:.3}, got {:.3} (diff {:.3}, tol {:.3}, base={}, burst_rate={}, burst_secs={}, every_secs={})",
            avg_exact, avg_simulated, avg_simulated - avg_exact, tolerance, base, burst_rate, burst_secs, every_secs
        );
    }

    /// `Constant { rate: Some(x) }`: rate_at всегда возвращает x (не зависит от t).
    #[test]
    fn prop_constant_rate_is_constant(rate in 0.0f64..1e6, t_offset in 0.0f64..10000.0) {
        let shape = LoadShape::Constant { rate: Some(rate) };
        let got = shape.rate_at(t_offset, 100.0, 999.0); // base игнорируется при Some
        prop_assert!(
            (got - rate).abs() < 1e-9,
            "Constant {{ rate: Some({}) }} при t={}: got {}",
            rate, t_offset, got
        );
    }
}
