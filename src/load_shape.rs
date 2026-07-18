//! Профили нагрузки во времени (F3, Веха A).
//!
//! Задаёт форму кривой интенсивности внутри одной фазы: как целевой rate
//! (сообщений в секунду) меняется в зависимости от времени `t`, прошедшего с
//! начала фазы. Планировщик в `core.rs` вычисляет мгновенный rate `rate_at(t)`
//! и выдерживает соответствующий межсообщенческий интервал.
//!
//! Ориентиры кривых — как в промышленных генераторах (ramp-up/steady/spike/
//! ramp-down): constant, linear ramp, sine и burst.

use serde::{Deserialize, Serialize};

/// Форма кривой интенсивности во времени.
///
/// Тегируется полем `type` в JSON, например:
/// `{"type":"linear","start_rate":10,"end_rate":1000}`.
/// Если поле `load_shape` в фазе не задано, применяется постоянная интенсивность
/// из `messages_per_second` (обратная совместимость), что эквивалентно
/// `{"type":"constant"}` с тем же rate.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LoadShape {
    /// Постоянная интенсивность. Если `rate` не задан, используется
    /// `messages_per_second` фазы (через [`LoadShape::effective_base`]).
    Constant {
        #[serde(default)]
        rate: Option<f64>,
    },
    /// Линейный ramp от `start_rate` до `end_rate` за `duration_secs` фазы.
    /// После окончания длительности (или если она не задана) держит `end_rate`.
    Linear { start_rate: f64, end_rate: f64 },
    /// Синусоида между `min_rate` и `max_rate` с периодом `period_secs`.
    /// В `t=0` находится в минимуме и растёт (фаза -pi/2).
    Sine {
        min_rate: f64,
        max_rate: f64,
        #[serde(default = "default_period")]
        period_secs: f64,
    },
    /// Всплески: базовая интенсивность `base_rate`, каждые `every_secs` —
    /// всплеск `burst_rate` длительностью `burst_secs`.
    Burst {
        base_rate: f64,
        burst_rate: f64,
        #[serde(default = "default_every")]
        every_secs: f64,
        #[serde(default = "default_burst")]
        burst_secs: f64,
    },
}

fn default_period() -> f64 {
    60.0
}
fn default_every() -> f64 {
    10.0
}
fn default_burst() -> f64 {
    1.0
}

impl LoadShape {
    /// Мгновенная целевая интенсивность (сообщений в секунду) в момент `t_secs`
    /// от начала фазы. `phase_duration_secs` — заявленная длительность фазы
    /// (0 = не задана), нужна для линейного ramp. `base_rate` —
    /// `messages_per_second` фазы, используется для варианта `Constant` без `rate`.
    ///
    /// Возвращает неотрицательное значение. 0.0 трактуется вызывающим кодом как
    /// «без ограничения скорости» — так же, как `messages_per_second == 0`.
    pub fn rate_at(&self, t_secs: f64, phase_duration_secs: f64, base_rate: f64) -> f64 {
        let t = t_secs.max(0.0);
        let r = match self {
            LoadShape::Constant { rate } => rate.unwrap_or(base_rate),
            LoadShape::Linear {
                start_rate,
                end_rate,
            } => {
                if phase_duration_secs <= 0.0 {
                    // Без длительности линейная интерполяция не определена —
                    // держим конечное значение.
                    *end_rate
                } else {
                    let frac = (t / phase_duration_secs).clamp(0.0, 1.0);
                    start_rate + (end_rate - start_rate) * frac
                }
            }
            LoadShape::Sine {
                min_rate,
                max_rate,
                period_secs,
            } => {
                let period = if *period_secs <= 0.0 {
                    1.0
                } else {
                    *period_secs
                };
                let mid = (min_rate + max_rate) / 2.0;
                let amp = (max_rate - min_rate) / 2.0;
                // Старт в минимуме: -cos даёт -1 при t=0.
                let phase = 2.0 * std::f64::consts::PI * (t / period);
                mid - amp * phase.cos()
            }
            LoadShape::Burst {
                base_rate: base,
                burst_rate,
                every_secs,
                burst_secs,
            } => {
                let cycle = if *every_secs <= 0.0 { 1.0 } else { *every_secs };
                let pos = t % cycle;
                if pos < *burst_secs {
                    *burst_rate
                } else {
                    *base
                }
            }
        };
        r.max(0.0)
    }

    /// Базовая (для метрики target_rate) интенсивность — пиковое или
    /// характерное значение кривой, чтобы отобразить намерение на дашборде.
    pub fn effective_base(&self, base_rate: f64) -> f64 {
        match self {
            LoadShape::Constant { rate } => rate.unwrap_or(base_rate),
            LoadShape::Linear {
                start_rate,
                end_rate,
            } => start_rate.max(*end_rate),
            LoadShape::Sine { max_rate, .. } => *max_rate,
            LoadShape::Burst { burst_rate, .. } => *burst_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn test_constant_uses_base_when_none() {
        let s = LoadShape::Constant { rate: None };
        approx(s.rate_at(0.0, 10.0, 500.0), 500.0);
        approx(s.rate_at(9.0, 10.0, 500.0), 500.0);
    }

    #[test]
    fn test_constant_explicit_rate() {
        let s = LoadShape::Constant { rate: Some(123.0) };
        approx(s.rate_at(5.0, 10.0, 500.0), 123.0);
    }

    #[test]
    fn test_linear_ramp_endpoints_and_mid() {
        let s = LoadShape::Linear {
            start_rate: 100.0,
            end_rate: 1100.0,
        };
        approx(s.rate_at(0.0, 10.0, 0.0), 100.0); // старт
        approx(s.rate_at(5.0, 10.0, 0.0), 600.0); // середина
        approx(s.rate_at(10.0, 10.0, 0.0), 1100.0); // конец
        approx(s.rate_at(999.0, 10.0, 0.0), 1100.0); // после конца — держим end
    }

    #[test]
    fn test_linear_without_duration_holds_end() {
        let s = LoadShape::Linear {
            start_rate: 100.0,
            end_rate: 1100.0,
        };
        approx(s.rate_at(3.0, 0.0, 0.0), 1100.0);
    }

    #[test]
    fn test_sine_starts_at_min_and_peaks_at_half_period() {
        let s = LoadShape::Sine {
            min_rate: 100.0,
            max_rate: 300.0,
            period_secs: 20.0,
        };
        approx(s.rate_at(0.0, 0.0, 0.0), 100.0); // минимум в t=0
        approx(s.rate_at(10.0, 0.0, 0.0), 300.0); // максимум в t=period/2
        approx(s.rate_at(20.0, 0.0, 0.0), 100.0); // снова минимум в t=period
        approx(s.rate_at(5.0, 0.0, 0.0), 200.0); // середина = mid
    }

    #[test]
    fn test_burst_windows() {
        let s = LoadShape::Burst {
            base_rate: 50.0,
            burst_rate: 5000.0,
            every_secs: 10.0,
            burst_secs: 2.0,
        };
        approx(s.rate_at(0.0, 0.0, 0.0), 5000.0); // начало окна всплеска
        approx(s.rate_at(1.9, 0.0, 0.0), 5000.0); // ещё в всплеске
        approx(s.rate_at(2.0, 0.0, 0.0), 50.0); // всплеск закончился
        approx(s.rate_at(9.5, 0.0, 0.0), 50.0); // база
        approx(s.rate_at(10.0, 0.0, 0.0), 5000.0); // новый цикл
        approx(s.rate_at(12.5, 0.0, 0.0), 50.0); // снова база
    }

    #[test]
    fn test_rate_never_negative() {
        let s = LoadShape::Linear {
            start_rate: -100.0,
            end_rate: -50.0,
        };
        assert!(s.rate_at(0.0, 10.0, 0.0) >= 0.0);
    }

    #[test]
    fn test_deserialize_tagged() {
        let s: LoadShape =
            serde_json::from_str(r#"{"type":"linear","start_rate":10,"end_rate":1000}"#).unwrap();
        assert_eq!(
            s,
            LoadShape::Linear {
                start_rate: 10.0,
                end_rate: 1000.0
            }
        );
        let s2: LoadShape = serde_json::from_str(
            r#"{"type":"burst","base_rate":50,"burst_rate":5000,"every_secs":10,"burst_secs":2}"#,
        )
        .unwrap();
        assert_eq!(
            s2,
            LoadShape::Burst {
                base_rate: 50.0,
                burst_rate: 5000.0,
                every_secs: 10.0,
                burst_secs: 2.0
            }
        );
    }

    #[test]
    fn test_effective_base() {
        approx(
            LoadShape::Linear {
                start_rate: 10.0,
                end_rate: 1000.0,
            }
            .effective_base(0.0),
            1000.0,
        );
        approx(
            LoadShape::Sine {
                min_rate: 10.0,
                max_rate: 900.0,
                period_secs: 5.0,
            }
            .effective_base(0.0),
            900.0,
        );
    }

    /// PR-16 (coverage): load_shape_linear_target_function_basic.
    /// `Linear { start_rate, end_rate }` rate_at body (line 78) was uncovered.
    #[test]
    fn load_shape_linear_rate_at_basic() {
        use crate::load_shape::LoadShape;
        let shape = LoadShape::Linear {
            start_rate: 0.0,
            end_rate: 100.0,
        };
        // t=0 → start_rate=0, t=end → end_rate=100.
        assert_eq!(shape.rate_at(0.0, 10.0, 0.0), 0.0);
        assert_eq!(shape.rate_at(10.0, 10.0, 0.0), 100.0);
        // Линейная интерполяция: середина = 50.
        assert_eq!(shape.rate_at(5.0, 10.0, 0.0), 50.0);
    }

    /// PR-16 (coverage): load_shape_burst_target_function_basic.
    /// `Burst` rate_at body (line 103) was uncovered.
    #[test]
    fn load_shape_burst_rate_at_basic() {
        use crate::load_shape::LoadShape;
        let shape = LoadShape::Burst {
            base_rate: 10.0,
            burst_rate: 100.0,
            every_secs: 10.0,
            burst_secs: 2.0,
        };
        // Внутри burst window (t % 10 < 2) — burst_rate.
        assert_eq!(shape.rate_at(0.0, 0.0, 0.0), 100.0);
        assert_eq!(shape.rate_at(1.0, 0.0, 0.0), 100.0);
        // Вне burst window — base_rate.
        assert_eq!(shape.rate_at(5.0, 0.0, 0.0), 10.0);
        assert_eq!(shape.rate_at(9.999, 0.0, 0.0), 10.0);
        // Следующий burst window.
        assert_eq!(shape.rate_at(10.0, 0.0, 0.0), 100.0);
        assert_eq!(shape.rate_at(11.5, 0.0, 0.0), 100.0);
    }

    /// Phase 11 (Tier 1): serde defaults вызываются при отсутствии полей в JSON.
    /// Phase: для Sine без period_secs, Burst без every_secs/burst_secs.
    #[test]
    fn load_shape_serde_defaults_applied_when_field_absent() {
        // Sine без period_secs → default_period() = 60.0.
        let s: LoadShape = serde_json::from_str(r#"{"type":"sine","min_rate":0,"max_rate":100}"#)
            .expect("parse sine");
        match s {
            LoadShape::Sine { period_secs, .. } => {
                assert!((period_secs - 60.0).abs() < 1e-9);
            }
            _ => panic!("expected Sine"),
        }

        // Burst без every_secs → default_every() = 10.0.
        let s2: LoadShape = serde_json::from_str(
            r#"{"type":"burst","base_rate":1,"burst_rate":2,"burst_secs":0.5}"#,
        )
        .expect("parse burst");
        match s2 {
            LoadShape::Burst {
                every_secs,
                burst_secs,
                ..
            } => {
                assert!((every_secs - 10.0).abs() < 1e-9);
                assert!((burst_secs - 0.5).abs() < 1e-9);
            }
            _ => panic!("expected Burst"),
        }

        // Burst без burst_secs → default_burst() = 1.0.
        let s3: LoadShape =
            serde_json::from_str(r#"{"type":"burst","base_rate":1,"burst_rate":2,"every_secs":5}"#)
                .expect("parse burst 2");
        match s3 {
            LoadShape::Burst {
                every_secs,
                burst_secs,
                ..
            } => {
                assert!((every_secs - 5.0).abs() < 1e-9);
                assert!((burst_secs - 1.0).abs() < 1e-9);
            }
            _ => panic!("expected Burst"),
        }
    }

    /// Phase 11 (Tier 1): Sine c period_secs <= 0 использует fallback period = 1.0.
    /// Защита от деления на ноль в `rate_at`.
    #[test]
    fn load_shape_sine_zero_or_negative_period_falls_back_to_one() {
        let s_zero = LoadShape::Sine {
            min_rate: 0.0,
            max_rate: 100.0,
            period_secs: 0.0,
        };
        let s_neg = LoadShape::Sine {
            min_rate: 0.0,
            max_rate: 100.0,
            period_secs: -5.0,
        };
        // Для period=1: t=0 → cos(0) = 1 → mid - amp*1 = min_rate.
        approx(s_zero.rate_at(0.0, 0.0, 0.0), 0.0);
        // Для period=1: t=0.5 → cos(pi) = -1 → mid + amp = max_rate.
        approx(s_zero.rate_at(0.5, 0.0, 0.0), 100.0);
        // Отрицательный период также сводится к 1.0.
        approx(s_neg.rate_at(0.0, 0.0, 0.0), 0.0);
        approx(s_neg.rate_at(0.5, 0.0, 0.0), 100.0);
    }

    /// Phase 11 (Tier 1): Burst c every_secs <= 0 использует fallback cycle = 1.0.
    /// Без защиты был бы деление на ноль в `t % cycle`.
    #[test]
    fn load_shape_burst_zero_or_negative_every_secs_falls_back_to_one() {
        let s_zero = LoadShape::Burst {
            base_rate: 10.0,
            burst_rate: 1000.0,
            every_secs: 0.0,
            burst_secs: 0.4,
        };
        let s_neg = LoadShape::Burst {
            base_rate: 10.0,
            burst_rate: 1000.0,
            every_secs: -3.0,
            burst_secs: 0.4,
        };
        // cycle=1.0, burst_secs=0.4: t%1 < 0.4 → burst; иначе base.
        approx(s_zero.rate_at(0.0, 0.0, 0.0), 1000.0);
        approx(s_zero.rate_at(0.3, 0.0, 0.0), 1000.0);
        approx(s_zero.rate_at(0.5, 0.0, 0.0), 10.0);
        approx(s_zero.rate_at(0.99, 0.0, 0.0), 10.0);
        approx(s_neg.rate_at(0.0, 0.0, 0.0), 1000.0);
        approx(s_neg.rate_at(0.5, 0.0, 0.0), 10.0);
    }

    /// Phase 11 (Tier 1): rate_at с t<0 клэмпится к 0 (защита от отрицательного t).
    #[test]
    fn load_shape_negative_t_is_clamped_to_zero() {
        let s = LoadShape::Burst {
            base_rate: 1.0,
            burst_rate: 50.0,
            every_secs: 10.0,
            burst_secs: 1.0,
        };
        // t=-5 → clamp к 0 → pos=0 < burst_secs=1 → burst_rate.
        approx(s.rate_at(-5.0, 0.0, 0.0), 50.0);
        approx(s.rate_at(-1.0, 0.0, 0.0), 50.0);
    }

    /// Phase 11 (Tier 1): effective_base для Constant { None } → base_rate.
    /// Phase 11 (Tier 1): effective_base для Constant { Some(r) } → r.
    #[test]
    fn load_shape_effective_base_constant_branches() {
        approx(
            LoadShape::Constant { rate: None }.effective_base(500.0),
            500.0,
        );
        approx(
            LoadShape::Constant { rate: Some(123.0) }.effective_base(500.0),
            123.0,
        );
        // Linear: max(start, end).
        approx(
            LoadShape::Linear {
                start_rate: 50.0,
                end_rate: 30.0,
            }
            .effective_base(0.0),
            50.0,
        );
        // Sine: max_rate.
        approx(
            LoadShape::Sine {
                min_rate: 0.0,
                max_rate: 999.0,
                period_secs: 5.0,
            }
            .effective_base(0.0),
            999.0,
        );
    }
}
