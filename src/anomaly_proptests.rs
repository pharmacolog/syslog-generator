//! N8 (v8.7.1) extension: property-based тесты для AnomalyKind.
//!
//! Issue #165 (C3 remaining proptest + insta): добавление proptest coverage
//! для src/anomaly.rs (F17). Существующие unit-тесты покрывают только edge cases.
//!
//! Использует `proptest = "1"` для автоматической генерации входных данных.

use crate::anomaly::{rate_multiplier, should_drop_packet, AnomalyKind};
use proptest::prelude::*;

/// N8 (Issue #165): `rate_multiplier` для `BurstInjection` всегда >= 1.0
/// (т.к. это multiplier ОТ base rate, и burst = увеличение). Вне burst window
/// возвращает 1.0 (default).
#[test]
fn prop_rate_multiplier_burst_injection_geq_one() {
    proptest!(|(rate_mult in 1.0f64..100.0f64, t_secs in 0.0f64..10000.0f64, interval_secs in 1.0f64..3600.0f64, duration_secs in 0.1f64..600.0f64)| {
        let kind = AnomalyKind::BurstInjection {
            rate_multiplier: rate_mult,
            interval_secs,
            duration_secs,
        };
        let m = rate_multiplier(&kind, t_secs);
        prop_assert!(m >= 1.0, "burst multiplier должен быть >= 1.0, got {m}");
    });
}

/// N8: `rate_multiplier` для `SlowDrip` всегда <= 1.0
/// (slow drip = уменьшение rate).
#[test]
fn prop_rate_multiplier_slow_drip_leq_one() {
    proptest!(|(rate_divisor in 1.0f64..100.0f64, t_secs in 0.0f64..10000.0f64, duration_secs in 0.1f64..600.0f64)| {
        let kind = AnomalyKind::SlowDrip {
            rate_divisor,
            duration_secs,
        };
        let m = rate_multiplier(&kind, t_secs);
        prop_assert!(m <= 1.0, "slow_drip multiplier должен быть <= 1.0, got {m}");
        prop_assert!(m > 0.0, "rate_multiplier должен быть > 0");
    });
}

/// N8: `rate_multiplier` для `PacketLoss` всегда ровно 1.0
/// (packet loss не меняет rate, только дропает сообщения).
#[test]
fn prop_rate_multiplier_packet_loss_is_one() {
    proptest!(|(loss_percent in 0.0f64..100.0f64, t_secs in 0.0f64..10000.0f64)| {
        let kind = AnomalyKind::PacketLoss { loss_percent };
        let m = rate_multiplier(&kind, t_secs);
        prop_assert_eq!(m, 1.0, "packet_loss не меняет rate, должен быть 1.0");
    });
}

/// N8: `should_drop_packet` детерминирован — same (kind, seed, seq) → same result.
#[test]
fn prop_should_drop_packet_deterministic() {
    proptest!(|(seed: u64, seq in 0usize..1000usize)| {
        let kind = AnomalyKind::PacketLoss { loss_percent: 50.0 };
        let r1 = should_drop_packet(&kind, Some(seed), seq);
        let r2 = should_drop_packet(&kind, Some(seed), seq);
        prop_assert_eq!(r1, r2, "drop decision должен быть детерминирован");
    });
}

/// N8: `should_drop_packet` для `PacketLoss` с loss_percent=0 → никогда не дропает.
#[test]
fn prop_should_drop_packet_zero_loss_never_drops() {
    proptest!(|(seed: u64, seq in 0usize..1000usize)| {
        let kind = AnomalyKind::PacketLoss { loss_percent: 0.0 };
        prop_assert!(!should_drop_packet(&kind, Some(seed), seq));
    });
}

/// N8: `should_drop_packet` для `PacketLoss` с loss_percent=100 → всегда дропает.
#[test]
fn prop_should_drop_packet_full_loss_always_drops() {
    proptest!(|(seed: u64, seq in 0usize..100usize)| {
        let kind = AnomalyKind::PacketLoss { loss_percent: 100.0 };
        prop_assert!(should_drop_packet(&kind, Some(seed), seq));
    });
}

/// N8: `should_drop_packet` для `BurstInjection` / `SlowDrip` → никогда не дропает
/// (они влияют на rate, не на drop).
#[test]
fn prop_should_drop_packet_non_packet_loss_never_drops() {
    proptest!(|(seed: u64, seq in 0usize..100usize, rate_mult in 1.0f64..10.0f64, rate_div in 1.0f64..10.0f64)| {
        let kinds = vec![
            AnomalyKind::BurstInjection {
                rate_multiplier: rate_mult,
                interval_secs: 60.0,
                duration_secs: 5.0,
            },
            AnomalyKind::SlowDrip {
                rate_divisor: rate_div,
                duration_secs: 5.0,
            },
        ];
        for kind in &kinds {
            prop_assert!(!should_drop_packet(kind, Some(seed), seq),
                "только packet_loss может дропать, {:?} не должна", kind);
        }
    });
}

/// N8: `rate_multiplier` для `BurstInjection` > 1.0 в burst window
/// (между interval и interval+duration), и = 1.0 вне burst.
/// Используем `interval > 2*duration` чтобы gap был достаточно велик
/// (offset 0.5 безопасно в gap [duration, interval)).
#[test]
fn prop_rate_multiplier_burst_window() {
    proptest!(|(rate_mult in 1.0f64..10.0f64, _base_time in 0.0f64..100.0f64, interval in 10.0f64..1000.0f64, duration in 0.1f64..4.5f64)| {
        // Гарантируем gap >= duration (interval > 2*duration).
        prop_assume!(interval > 2.0 * duration);
        let kind = AnomalyKind::BurstInjection {
            rate_multiplier: rate_mult,
            interval_secs: interval,
            duration_secs: duration,
        };
        // В burst window: m > 1.0. t_in в середине первого burst [0, duration).
        let t_in = duration * 0.5;
        let m_in = rate_multiplier(&kind, t_in);
        prop_assert!(m_in >= 1.0, "burst внутри window должен быть >= 1.0");
        // Вне burst window: m == 1.0. t_out в gap [duration, interval).
        // t_out mod cycle = duration + 0.5, что ∈ (duration, interval) при duration < interval.
        let t_out = duration + 0.5;
        let m_out = rate_multiplier(&kind, t_out);
        prop_assert_eq!(m_out, 1.0, "burst вне window должен быть 1.0");
    });
}
