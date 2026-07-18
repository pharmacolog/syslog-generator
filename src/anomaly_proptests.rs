//! Phase 9 (v10.7.16, PR-Q.4): property-based тесты для `src/anomaly.rs`.
//!
//! Цель — **invariants** декларативных сценариев аномалий нагрузки:
//! `AnomalyPlanner::combined_rate_multiplier` всегда положителен
//! (rate-loop'у не нужны inf/NaN/<=0) и rate-модификаторы `BurstInjection`
//! неактивны вне burst-окна (множитель = 1.0).
//! `PacketLoss.should_drop_packet` распределение дропов близко к `loss_percent`
//! (±2% на выборке 10k trials).
//!
//! Дополняет существующие unit-тесты в `src/anomaly.rs::tests`,
//! которые покрывают конкретные edge cases.
//!
//! Конвенция имён: `<module>_proptests.rs` (имя уже используется
//! `payload_proptests.rs`); этот файл — `anomaly_proptests.rs` чтобы
//! избежать конфликта.

use crate::anomaly::{Anomaly, AnomalyKind, AnomalyPlanner};
use proptest::prelude::*;

proptest! {
    /// `AnomalyPlanner::combined_rate_multiplier(t) > 0` всегда
    /// для любого `t` ∈ [0, 2000) при `BurstInjection` с валидными параметрами.
    /// Защита от inf/NaN/<=0 в rate-loop'е (деление на ноль в интервале
    /// между сообщениями).
    #[test]
    fn prop_combined_rate_multiplier_positive(
        burst_interval in 1u64..1000,
        burst_duration in 1u64..100,
        burst_multiplier in 0.5f64..10.0,
    ) {
        let anomalies = vec![Anomaly {
            kind: AnomalyKind::BurstInjection {
                rate_multiplier: burst_multiplier,
                interval_secs: burst_interval as f64,
                duration_secs: burst_duration as f64,
            },
        }];
        let planner = AnomalyPlanner::new(&anomalies);
        for t in 0..2000u64 {
            let rate = planner.combined_rate_multiplier(t as f64);
            prop_assert!(
                rate > 0.0,
                "rate must be positive, got {} at t={}",
                rate, t
            );
            prop_assert!(
                rate.is_finite(),
                "rate must be finite, got {} at t={}",
                rate, t
            );
        }
    }

    /// `BurstInjection` rate-multiplier вне burst-окна должен быть ровно 1.0
    /// (rate не модифицируется). Это инвариант семантики `BurstInjection` —
    /// множитель `1.0` означает «нет эффекта».
    #[test]
    fn prop_burst_no_effect_outside_interval(
        burst_interval in 2u64..100,
        burst_duration in 1u64..50,
    ) {
        // Гарантия: burst_duration < burst_interval (строго). При
        // duration == interval burst покрывает весь цикл (всегда активен),
        // и тест невалиден.
        prop_assume!(burst_duration < burst_interval);

        let anomalies = vec![Anomaly {
            kind: AnomalyKind::BurstInjection {
                rate_multiplier: 5.0,
                interval_secs: burst_interval as f64,
                duration_secs: burst_duration as f64,
            },
        }];
        let planner = AnomalyPlanner::new(&anomalies);
        // t = interval + duration: позиция в цикле = (interval + duration) mod interval = duration.
        // pos == duration → pos < duration == false → burst неактивен → multiplier = 1.0.
        let t = burst_interval + burst_duration;
        prop_assert_eq!(
            planner.combined_rate_multiplier(t as f64),
            1.0,
            "rate multiplier вне burst-окна должен быть 1.0"
        );
        // Ещё дальше — тоже 1.0
        let t2 = 2 * burst_interval + burst_duration;
        prop_assert_eq!(
            planner.combined_rate_multiplier(t2 as f64),
            1.0,
            "rate multiplier в паузе между циклами должен быть 1.0"
        );
    }

    /// `PacketLoss.should_drop_packet` — частота дропов близка к
    /// `loss_percent` (±2% на 10k trials). Это эмпирическая проверка
    /// равномерности RNG, который лежит в основе `derive_rng`.
    /// При `loss_percent` вне (0, 100) тест пропускается (граничные
    /// значения обрабатываются отдельно — см. unit-тесты
    /// `packet_loss_zero_never_drops` / `packet_loss_hundred_always_drops`).
    #[test]
    fn prop_packet_loss_distribution(
        loss_percent in 1u32..50,
        seed in 1u64..1000,
    ) {
        let anomalies = vec![Anomaly {
            kind: AnomalyKind::PacketLoss {
                loss_percent: loss_percent as f64,
            },
        }];
        let planner = AnomalyPlanner::new(&anomalies);
        let mut drops = 0u32;
        let trials = 10_000u32;
        for s in 0..trials {
            if planner.should_drop(Some(seed), s as usize) {
                drops += 1;
            }
        }
        let actual_percent = drops as f64 / trials as f64 * 100.0;
        let expected = loss_percent as f64;
        // ±2% tolerance (для 10k trials std error ≈ 0.5%, 4σ bound ≈ 2%).
        prop_assert!(
            (actual_percent - expected).abs() < 2.0,
            "loss expected {}%, got {:.2}% (drops={}/{})",
            expected, actual_percent, drops, trials
        );
    }

    /// `PacketLoss` c `loss_percent = 0` → 0 дропов за 10k trials.
    /// С `loss_percent = 100` → все 10k дропов. Граничные значения —
    /// ранний short-circuit в `should_drop_packet` (см. anomaly.rs:188-194).
    #[test]
    fn prop_packet_loss_boundaries(seed in 0u64..1000, seq_offset in 0u64..1000) {
        let anomalies_zero = vec![Anomaly {
            kind: AnomalyKind::PacketLoss { loss_percent: 0.0 },
        }];
        let planner_zero = AnomalyPlanner::new(&anomalies_zero);
        for s in 0..100 {
            prop_assert!(
                !planner_zero.should_drop(Some(seed), (s + seq_offset) as usize),
                "loss_percent=0 не должен дропать"
            );
        }

        let anomalies_full = vec![Anomaly {
            kind: AnomalyKind::PacketLoss { loss_percent: 100.0 },
        }];
        let planner_full = AnomalyPlanner::new(&anomalies_full);
        for s in 0..100 {
            prop_assert!(
                planner_full.should_drop(Some(seed), (s + seq_offset) as usize),
                "loss_percent=100 должен дропать всё"
            );
        }
    }

    /// `SlowDrip` в окне даёт `1/divisor`, вне окна — 1.0.
    /// Multiplier всегда положителен и не превышает 1.0 (drip замедляет, не ускоряет).
    #[test]
    fn prop_slow_drip_in_window_divides_rate(
        rate_divisor in 1.5f64..100.0,
        duration_secs in 1u64..1000,
    ) {
        let anomalies = vec![Anomaly {
            kind: AnomalyKind::SlowDrip {
                rate_divisor,
                duration_secs: duration_secs as f64,
            },
        }];
        let planner = AnomalyPlanner::new(&anomalies);
        // В окне (t < duration_secs): rate = 1/divisor
        let mid = (duration_secs / 2) as f64;
        let in_window_rate = planner.combined_rate_multiplier(mid);
        prop_assert!(
            (in_window_rate - 1.0 / rate_divisor).abs() < 1e-9,
            "slow_drip mid rate: expected 1/{}, got {}",
            rate_divisor, in_window_rate
        );
        prop_assert!(
            in_window_rate > 0.0 && in_window_rate <= 1.0,
            "slow_drip rate должен быть (0, 1], got {}",
            in_window_rate
        );
        // Вне окна (t >= duration_secs): rate = 1.0
        let after = duration_secs as f64 + 1.0;
        prop_assert_eq!(
            planner.combined_rate_multiplier(after),
            1.0,
            "slow_drip после окна должен быть 1.0"
        );
    }
}
