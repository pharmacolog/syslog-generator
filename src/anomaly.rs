//! F17 (v9.4.0): сценарии аномалий нагрузки.
//!
//! Цель: дать генератору возможность имитировать «шумные»/«больные» паттерны
//! нагрузки для тестирования SIEM-правил (всплески ошибок, low-and-slow
//! атаки, потери пакетов, MITRE ATT&CK-подобные последовательности).
//!
//! Архитектурно аномалии — **декларативный слой над существующим rate-loop'ом**:
//! каждая аномалия задаёт, как модифицировать целевую интенсивность (или
//! дропать сообщения) в конкретные моменты времени фазы. В `run_phase_multi`
//! перед каждым `tx.send` вызывается [`rate_multiplier`] (влияет на интервал
//! между сообщениями) и [`should_drop_packet`] (влияет на факт отправки).
//!
//! Все параметры — `f64` (Rate — дробный), serde — `tag = "type"` для удобного
//! YAML/JSON-представления:
//!
//! ```yaml
//! anomalies:
//!   - type: burst-injection
//!     rate_multiplier: 10.0
//!     interval_secs: 30.0
//!     duration_secs: 2.0
//!   - type: slow-drip
//!     rate_divisor: 5.0
//!     duration_secs: 60.0
//!   - type: packet-loss
//!     loss_percent: 20.0
//! ```
//!
//! Дизайн:
//! - **strong typing**: tagged enum [`AnomalyKind`] вместо `HashMap<String, Value>`
//!   (план §3.4 v9.4.0 предполагал map-вариант — enum надёжнее для компилятора
//!   и IDE; serde-схема при этом остаётся такой же компактной);
//! - **детерминизм F4**: drop-решения [`should_drop_packet`] используют тот же
//!   [`crate::payload::derive_rng`] с F17-salt в `seq` — при заданном `phase.seed`
//!   паттерн потерь воспроизводим, как и контент сообщений;
//! - **backward-compat**: `Phase.anomalies: Option<Vec<Anomaly>>` с
//!   `#[serde(default)]` — старые профили без поля работают без изменений.

use serde::{Deserialize, Serialize};

/// F17-salt для RNG-решения о drop'е. Добавляется к `seq` перед
/// `derive_rng`, чтобы поток не коррелировал с основной генерацией
/// контента в `generate_message`. Магическое число выбрано произвольно
/// и не должно меняться без миграционного флага — иначе сломается
/// воспроизводимость `seed`-профилей между версиями.
const DROP_DECISION_SEQ_SALT: usize = 0x0F17_0F17;

/// Конкретный сценарий аномалии. Тегируется полем `type` в JSON/YAML.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AnomalyKind {
    /// Всплеск интенсивности: каждые `interval_secs` секунд в течение
    /// `duration_secs` секунд rate умножается на `rate_multiplier`.
    ///
    /// Семантика времени: `pos = t mod interval_secs`. Если `pos < duration_secs`
    /// — аномалия активна (rate *= multiplier). Иначе — пассивна (rate *= 1.0).
    /// Допустимо `duration_secs >= interval_secs` — тогда аномалия всегда
    /// активна (постоянный ×M rate, удобно для нагрузочных тестов).
    ///
    /// Use cases: DDoS-всплеск, spike-нагрузка, проверка back-pressure.
    #[serde(rename = "burst-injection")]
    BurstInjection {
        rate_multiplier: f64,
        interval_secs: f64,
        duration_secs: f64,
    },

    /// Медленная утечка: первые `duration_secs` секунд фазы rate делится на
    /// `rate_divisor` (низкая фоновая активность).
    ///
    /// Семантика: если `t < duration_secs` — rate /= divisor. Иначе — rate без
    /// изменений. Применяется в начале фазы, удобно комбинировать с burst'ами.
    ///
    /// Use cases: low-and-slow атаки, тестирование порогов SIEM по низкому
    /// фоновому шуму, эмуляция редких событий.
    #[serde(rename = "slow-drip")]
    SlowDrip {
        rate_divisor: f64,
        duration_secs: f64,
    },

    /// Потеря пакетов: каждое сгенерированное сообщение с вероятностью
    /// `loss_percent` (0.0..=100.0) дропается **до отправки** в транспорт.
    /// Дропнутые сообщения не инкрементируют `syslog_messages_total`,
    /// но инкрементируют `syslog_anomalies_dropped_total{type="packet-loss"}`.
    ///
    /// Детерминировано по `(phase.seed, seq)` через [`crate::payload::derive_rng`]
    /// с F17-salt в seq — воспроизводимо при заданном seed.
    ///
    /// Use cases: эмуляция нестабильного канала, потеря UDP-датаграмм,
    /// тестирование идемпотентности/ретраев на стороне приёмника.
    #[serde(rename = "packet-loss")]
    PacketLoss { loss_percent: f64 },
}

/// Обёртка одной аномалии в списке `Phase.anomalies`.
///
/// Сейчас содержит только `kind` (через `#[serde(flatten)]`), но обёртка
/// оставлена для будущих общих полей (например, `name` для логирования
/// или `enabled: bool` для kill-switch). С `flatten` Anomaly сериализуется
/// как плоский tagged-объект вида `{"type":"burst-injection", ...}`,
/// что интуитивно для пользователя и совместимо с AnomalyKind tagged enum.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Anomaly {
    #[serde(flatten)]
    pub kind: AnomalyKind,
}

impl Anomaly {
    /// Каноническое имя типа аномалии (для метрик и логирования).
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            AnomalyKind::BurstInjection { .. } => "burst-injection",
            AnomalyKind::SlowDrip { .. } => "slow-drip",
            AnomalyKind::PacketLoss { .. } => "packet-loss",
        }
    }
}

/// Множитель rate, который нужно применить к базовому rate в момент
/// `t_secs` от начала фазы для данной аномалии.
///
/// Возвращает `1.0` если аномалия пассивна (rate применяется без изменений).
/// `PacketLoss` всегда возвращает `1.0` — он не про rate, а про факт отправки
/// (см. [`should_drop_packet`]).
///
/// Контракт:
/// - BurstInjection: `> 1.0` в окне всплеска, `1.0` иначе.
/// - SlowDrip: `< 1.0` в начале фазы, `1.0` иначе.
/// - PacketLoss: всегда `1.0`.
///
/// Защита от деления на ноль / NaN: при невалидных параметрах (валидация F13
/// их отсекает заранее) возвращается `1.0`, чтобы не сломать rate-loop.
pub fn rate_multiplier(kind: &AnomalyKind, t_secs: f64) -> f64 {
    match kind {
        AnomalyKind::BurstInjection {
            rate_multiplier,
            interval_secs,
            duration_secs,
        } => {
            if !interval_secs.is_finite() || *interval_secs <= 0.0 {
                return 1.0;
            }
            if !duration_secs.is_finite() || *duration_secs < 0.0 {
                return 1.0;
            }
            if !rate_multiplier.is_finite() || *rate_multiplier <= 0.0 {
                return 1.0;
            }
            let pos = t_secs.rem_euclid(*interval_secs);
            if pos < *duration_secs {
                *rate_multiplier
            } else {
                1.0
            }
        }
        AnomalyKind::SlowDrip {
            rate_divisor,
            duration_secs,
        } => {
            if !duration_secs.is_finite() || *duration_secs <= 0.0 {
                return 1.0;
            }
            if !rate_divisor.is_finite() || *rate_divisor <= 1.0 {
                return 1.0;
            }
            if t_secs < *duration_secs {
                1.0 / *rate_divisor
            } else {
                1.0
            }
        }
        AnomalyKind::PacketLoss { .. } => 1.0,
    }
}

/// Решить, нужно ли дропнуть сообщение при packet-loss.
///
/// Возвращает `false` для всех не-`PacketLoss`-аномалий (BurstInjection и
/// SlowDrip не дропают сообщения — они меняют rate).
///
/// Детерминировано по `(phase_seed, seq)`. Используется
/// [`crate::payload::derive_rng`] с F17-salt — паттерн потерь не коррелирует
/// с основной генерацией контента в `generate_message`.
pub fn should_drop_packet(kind: &AnomalyKind, phase_seed: Option<u64>, seq: usize) -> bool {
    if let AnomalyKind::PacketLoss { loss_percent } = kind {
        if !loss_percent.is_finite() {
            return false;
        }
        if *loss_percent <= 0.0 {
            return false;
        }
        if *loss_percent >= 100.0 {
            return true;
        }
        let salted_seq = seq.wrapping_add(DROP_DECISION_SEQ_SALT);
        let mut rng = crate::payload::derive_rng(phase_seed, salted_seq);
        let roll = crate::payload::int_in_range(0, 99, &mut rng);
        (roll as f64) < *loss_percent
    } else {
        false
    }
}

/// Планировщик аномалий для фазы: агрегирует множители rate от всех
/// rate-модифицирующих аномалий (BurstInjection/SlowDrip) и собирает
/// все packet-loss-аномалии в один список для решения о drop'е.
///
/// Multiplier policy: **произведение** всех активных множителей
/// (BurstInjection ×M в окне даёт ×M, плюс активный SlowDrip ÷D даёт ×(1/D),
/// итого ×(M/D)). Это интуитивно: «всплеск поверх медленной утечки» —
/// даёт X-кратный всплеск относительно пониженной базы.
///
/// Packet-loss policy: если в фазе несколько `PacketLoss` — для каждого
/// сообщения дроп-решение принимается по **первому** активному
/// packet-loss в списке (OR-логика не накапливается — иначе loss_percent
/// мог бы превысить 100% при композиции).
#[derive(Debug, Clone)]
pub struct AnomalyPlanner<'a> {
    pub anomalies: &'a [Anomaly],
}

impl<'a> AnomalyPlanner<'a> {
    pub fn new(anomalies: &'a [Anomaly]) -> Self {
        Self { anomalies }
    }

    /// Комбинированный множитель rate в момент `t_secs` (произведение
    /// множителей всех активных rate-аномалий). Если аномалий нет — 1.0.
    pub fn combined_rate_multiplier(&self, t_secs: f64) -> f64 {
        if self.anomalies.is_empty() {
            return 1.0;
        }
        let mut m = 1.0_f64;
        for a in self.anomalies {
            m *= rate_multiplier(&a.kind, t_secs);
        }
        m
    }

    /// Решить, дропать ли сообщение с номером `seq`. True, если хотя бы
    /// одна `PacketLoss`-аномалия решила дропнуть (на практике в фазе
    /// одна packet-loss имеет смысл — несколько объединяются по OR).
    pub fn should_drop(&self, phase_seed: Option<u64>, seq: usize) -> bool {
        for a in self.anomalies {
            if should_drop_packet(&a.kind, phase_seed, seq) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    // ===== AnomalyKind: serde round-trip =====

    #[test]
    fn burst_injection_round_trip() {
        let yaml = "type: burst-injection\nrate_multiplier: 10.0\ninterval_secs: 30.0\nduration_secs: 2.0\n";
        let a: Anomaly = serde_yaml::from_str(yaml).expect("yaml");
        assert_eq!(
            a.kind,
            AnomalyKind::BurstInjection {
                rate_multiplier: 10.0,
                interval_secs: 30.0,
                duration_secs: 2.0
            }
        );
        let back = serde_yaml::to_string(&a).expect("to_string");
        assert!(back.contains("burst-injection"), "got: {back}");
    }

    #[test]
    fn slow_drip_round_trip() {
        let yaml = "type: slow-drip\nrate_divisor: 5.0\nduration_secs: 60.0\n";
        let a: Anomaly = serde_yaml::from_str(yaml).expect("yaml");
        assert_eq!(
            a.kind,
            AnomalyKind::SlowDrip {
                rate_divisor: 5.0,
                duration_secs: 60.0
            }
        );
    }

    #[test]
    fn packet_loss_round_trip() {
        let yaml = "type: packet-loss\nloss_percent: 20.0\n";
        let a: Anomaly = serde_yaml::from_str(yaml).expect("yaml");
        assert_eq!(a.kind, AnomalyKind::PacketLoss { loss_percent: 20.0 });
    }

    #[test]
    fn json_tagged_round_trip() {
        let json = r#"{"type":"burst-injection","rate_multiplier":5.0,"interval_secs":10.0,"duration_secs":1.0}"#;
        let a: Anomaly = serde_json::from_str(json).expect("json");
        assert!(matches!(a.kind, AnomalyKind::BurstInjection { .. }));
    }

    #[test]
    fn unknown_kind_rejected() {
        let yaml = "type: not-a-kind\n";
        let res: Result<Anomaly, _> = serde_yaml::from_str(yaml);
        assert!(res.is_err(), "expected unknown tag to be rejected");
    }

    #[test]
    fn type_name() {
        assert_eq!(
            Anomaly {
                kind: AnomalyKind::BurstInjection {
                    rate_multiplier: 1.0,
                    interval_secs: 1.0,
                    duration_secs: 1.0
                }
            }
            .type_name(),
            "burst-injection"
        );
        assert_eq!(
            Anomaly {
                kind: AnomalyKind::SlowDrip {
                    rate_divisor: 1.0,
                    duration_secs: 1.0
                }
            }
            .type_name(),
            "slow-drip"
        );
        assert_eq!(
            Anomaly {
                kind: AnomalyKind::PacketLoss { loss_percent: 0.0 }
            }
            .type_name(),
            "packet-loss"
        );
    }

    // ===== rate_multiplier: поведение по времени =====

    #[test]
    fn burst_active_in_window() {
        let k = AnomalyKind::BurstInjection {
            rate_multiplier: 8.0,
            interval_secs: 10.0,
            duration_secs: 2.0,
        };
        approx(rate_multiplier(&k, 0.0), 8.0); // начало окна
        approx(rate_multiplier(&k, 1.5), 8.0); // внутри окна
        approx(rate_multiplier(&k, 2.0), 1.0); // конец окна
        approx(rate_multiplier(&k, 9.5), 1.0); // база
        approx(rate_multiplier(&k, 10.0), 8.0); // новый цикл
        approx(rate_multiplier(&k, 12.0), 1.0); // после всплеска
    }

    #[test]
    fn burst_with_full_coverage_always_active() {
        let k = AnomalyKind::BurstInjection {
            rate_multiplier: 3.0,
            interval_secs: 5.0,
            duration_secs: 10.0, // > interval_secs → всегда ×3
        };
        approx(rate_multiplier(&k, 0.0), 3.0);
        approx(rate_multiplier(&k, 4.5), 3.0);
        approx(rate_multiplier(&k, 5.0), 3.0);
    }

    #[test]
    fn burst_invalid_interval_returns_one() {
        let k = AnomalyKind::BurstInjection {
            rate_multiplier: 5.0,
            interval_secs: 0.0,
            duration_secs: 1.0,
        };
        approx(rate_multiplier(&k, 1.0), 1.0);
    }

    #[test]
    fn burst_negative_multiplier_returns_one() {
        let k = AnomalyKind::BurstInjection {
            rate_multiplier: -1.0,
            interval_secs: 5.0,
            duration_secs: 1.0,
        };
        approx(rate_multiplier(&k, 1.0), 1.0);
    }

    #[test]
    fn slow_drip_only_at_start() {
        let k = AnomalyKind::SlowDrip {
            rate_divisor: 4.0,
            duration_secs: 10.0,
        };
        approx(rate_multiplier(&k, 0.0), 0.25); // 1/4
        approx(rate_multiplier(&k, 5.0), 0.25);
        approx(rate_multiplier(&k, 10.0), 1.0); // конец окна
        approx(rate_multiplier(&k, 100.0), 1.0);
    }

    #[test]
    fn slow_drip_zero_duration_returns_one() {
        let k = AnomalyKind::SlowDrip {
            rate_divisor: 5.0,
            duration_secs: 0.0,
        };
        approx(rate_multiplier(&k, 0.0), 1.0);
    }

    #[test]
    fn slow_drip_divisor_le_one_returns_one() {
        // divisor <= 1 → без уменьшения (валидация F13 должна была отсеять,
        // но защита в runtime остаётся).
        let k = AnomalyKind::SlowDrip {
            rate_divisor: 0.5,
            duration_secs: 10.0,
        };
        approx(rate_multiplier(&k, 5.0), 1.0);
    }

    #[test]
    fn packet_loss_does_not_affect_rate() {
        let k = AnomalyKind::PacketLoss { loss_percent: 50.0 };
        approx(rate_multiplier(&k, 0.0), 1.0);
        approx(rate_multiplier(&k, 100.0), 1.0);
    }

    // ===== should_drop_packet: граничные условия =====

    #[test]
    fn packet_loss_zero_never_drops() {
        let k = AnomalyKind::PacketLoss { loss_percent: 0.0 };
        for seq in 0..200 {
            assert!(!should_drop_packet(&k, Some(42), seq));
        }
    }

    #[test]
    fn packet_loss_hundred_always_drops() {
        let k = AnomalyKind::PacketLoss {
            loss_percent: 100.0,
        };
        for seq in 0..200 {
            assert!(should_drop_packet(&k, Some(42), seq));
        }
    }

    #[test]
    fn packet_loss_deterministic_for_seed() {
        let k = AnomalyKind::PacketLoss { loss_percent: 50.0 };
        let mut a_count = 0;
        let mut b_count = 0;
        for seq in 0..1000 {
            if should_drop_packet(&k, Some(12345), seq) {
                a_count += 1;
            }
            if should_drop_packet(&k, Some(12345), seq) {
                b_count += 1;
            }
        }
        assert_eq!(a_count, b_count, "детерминизм сломан");
        // Примерная оценка для loss_percent=50: ~500 дропов из 1000.
        assert!(
            (400..=600).contains(&a_count),
            "ожидалось ~500 дропов, got: {a_count}"
        );
    }

    #[test]
    fn packet_loss_different_seeds_produce_different_patterns() {
        let k = AnomalyKind::PacketLoss { loss_percent: 30.0 };
        let mut s1 = 0;
        let mut s2 = 0;
        for seq in 0..1000 {
            if should_drop_packet(&k, Some(1), seq) {
                s1 += 1;
            }
            if should_drop_packet(&k, Some(2), seq) {
                s2 += 1;
            }
        }
        assert_ne!(s1, s2, "разные seed'ы должны давать разные паттерны");
    }

    #[test]
    fn packet_loss_no_seed_still_drops() {
        // Без seed — используется OS-энтропия. Проверяем только что
        // возвращается bool (не паникует) и примерно соответствует
        // заявленному проценту (грубая оценка для недетерминированного
        // случая с 5000 trials).
        let k = AnomalyKind::PacketLoss { loss_percent: 50.0 };
        let mut drops = 0;
        for seq in 0..5000 {
            if should_drop_packet(&k, None, seq) {
                drops += 1;
            }
        }
        // Очень грубая проверка: ±20% от ожидания. Для entropy-источника
        // может быть значительный разброс, но 50% не должно давать 0% или 100%.
        assert!(
            (1000..=4000).contains(&drops),
            "дропов: {drops}/5000 (ожидалось ~2500)"
        );
    }

    #[test]
    fn non_packet_loss_never_drops() {
        let burst = AnomalyKind::BurstInjection {
            rate_multiplier: 5.0,
            interval_secs: 1.0,
            duration_secs: 1.0,
        };
        let slow = AnomalyKind::SlowDrip {
            rate_divisor: 5.0,
            duration_secs: 10.0,
        };
        for seq in 0..100 {
            assert!(!should_drop_packet(&burst, Some(1), seq));
            assert!(!should_drop_packet(&slow, Some(1), seq));
        }
    }

    // ===== AnomalyPlanner: комбинирование =====

    #[test]
    fn planner_empty_returns_one_and_no_drop() {
        let p = AnomalyPlanner::new(&[]);
        approx(p.combined_rate_multiplier(0.0), 1.0);
        assert!(!p.should_drop(Some(1), 1));
    }

    #[test]
    fn planner_multiplies_active_multipliers() {
        // burst ×4 в окне + slow_drip ÷2 в начале → ×2 в окне ×4×0.5 = ×2.
        let anomalies = vec![
            Anomaly {
                kind: AnomalyKind::BurstInjection {
                    rate_multiplier: 4.0,
                    interval_secs: 10.0,
                    duration_secs: 2.0,
                },
            },
            Anomaly {
                kind: AnomalyKind::SlowDrip {
                    rate_divisor: 2.0,
                    duration_secs: 5.0,
                },
            },
        ];
        let p = AnomalyPlanner::new(&anomalies);
        // t=1.0: burst активен (4.0), slow_drip активен (0.5) → 2.0
        approx(p.combined_rate_multiplier(1.0), 2.0);
        // t=4.0: burst неактивен (pos=4 >= duration_secs=2, multiplier=1.0),
        // slow_drip активен (0.5) → 0.5
        approx(p.combined_rate_multiplier(4.0), 0.5);
        // t=8.0: burst неактивен (pos=8 >= duration_secs=2, multiplier=1.0),
        // slow_drip неактивен (1.0) → 1.0
        approx(p.combined_rate_multiplier(8.0), 1.0);
        // t=20.0: burst снова активен (pos=20 mod 10 = 0 < 2, multiplier=4.0),
        // slow_drip неактивен (1.0) → 4.0
        approx(p.combined_rate_multiplier(20.0), 4.0);
        // t=11.0: burst снова активен (pos=11 mod 10 = 1 < 2, multiplier=4.0),
        // slow_drip неактивен (1.0) → 4.0
        approx(p.combined_rate_multiplier(11.0), 4.0);
    }

    #[test]
    fn planner_drop_first_packet_loss_wins() {
        let anomalies = vec![
            Anomaly {
                kind: AnomalyKind::PacketLoss {
                    loss_percent: 100.0,
                },
            },
            Anomaly {
                kind: AnomalyKind::PacketLoss { loss_percent: 0.0 },
            },
        ];
        let p = AnomalyPlanner::new(&anomalies);
        // Первая packet-loss с 100% → всегда дроп.
        assert!(p.should_drop(Some(1), 1));
    }

    #[test]
    fn planner_drop_or_logic() {
        // Две packet-loss: 100% и 50%. Первая выигрывает → всегда дроп.
        let anomalies = vec![
            Anomaly {
                kind: AnomalyKind::PacketLoss {
                    loss_percent: 100.0,
                },
            },
            Anomaly {
                kind: AnomalyKind::PacketLoss { loss_percent: 0.0 },
            },
        ];
        let p = AnomalyPlanner::new(&anomalies);
        for seq in 0..50 {
            assert!(p.should_drop(Some(1), seq));
        }
        // Реверс: 0% и 100% — вторая выигрывает (первая говорит «нет»,
        // вторая «да», OR=true).
        let anomalies = vec![
            Anomaly {
                kind: AnomalyKind::PacketLoss { loss_percent: 0.0 },
            },
            Anomaly {
                kind: AnomalyKind::PacketLoss {
                    loss_percent: 100.0,
                },
            },
        ];
        let p = AnomalyPlanner::new(&anomalies);
        for seq in 0..50 {
            assert!(p.should_drop(Some(1), seq));
        }
    }
}

#[cfg(test)]
mod tests_proptest {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `AnomalyPlanner::combined_rate_multiplier` всегда > 0.
        #[test]
        fn prop_combined_rate_multiplier_positive(
            burst_interval_secs in 1u64..1000,
            burst_duration_secs in 1u64..100,
            burst_multiplier in 0.5f64..10.0,
        ) {
            let anomalies = vec![Anomaly {
                kind: AnomalyKind::BurstInjection {
                    rate_multiplier: burst_multiplier,
                    interval_secs: burst_interval_secs as f64,
                    duration_secs: burst_duration_secs as f64,
                },
            }];
            let planner = AnomalyPlanner::new(&anomalies);
            for t in 0..2000u64 {
                let rate = planner.combined_rate_multiplier(t as f64);
                prop_assert!(rate > 0.0, "rate must be positive, got {} at t={}", rate, t);
            }
        }

        /// Burst injection не изменяет rate вне burst window.
        #[test]
        fn prop_burst_no_effect_outside_window(
            burst_interval in 1u64..100,
            burst_duration in 1u64..50,
            seed in 0u64..1000,
        ) {
            prop_assume!(burst_duration < burst_interval);
            let anomalies = vec![Anomaly {
                kind: AnomalyKind::BurstInjection {
                    rate_multiplier: 5.0,
                    interval_secs: burst_interval as f64,
                    duration_secs: burst_duration as f64,
                },
            }];
            let planner = AnomalyPlanner::new(&anomalies);
            let t = seed * burst_interval + burst_duration;
            prop_assert_eq!(planner.combined_rate_multiplier(t as f64), 1.0);
        }

        /// Пустой список аномалий даёт единичный множитель rate.
        #[test]
        fn prop_empty_anomalies_yields_unit_rate(t in any::<u64>()) {
            let planner = AnomalyPlanner::new(&[]);
            prop_assert_eq!(planner.combined_rate_multiplier(t as f64), 1.0);
        }

        /// Packet loss детерминирован для одного `(seed, packet_number)`.
        #[test]
        fn prop_packet_loss_deterministic(
            seed in 0u64..1000,
            loss_percent in 1u32..50,
            packet_number in 0usize..1000,
        ) {
            let anomalies = vec![Anomaly {
                kind: AnomalyKind::PacketLoss {
                    loss_percent: loss_percent as f64,
                },
            }];
            let planner = AnomalyPlanner::new(&anomalies);
            let result1 = planner.should_drop(Some(seed), packet_number);
            let result2 = planner.should_drop(Some(seed), packet_number);
            prop_assert_eq!(result1, result2, "should_drop must be deterministic");
        }

        /// Доля packet loss на большой выборке близка к заданному проценту.
        #[test]
        fn prop_packet_loss_distribution(
            loss_percent in 5u32..50,
            seed in 0u64..100,
        ) {
            let anomalies = vec![Anomaly {
                kind: AnomalyKind::PacketLoss {
                    loss_percent: loss_percent as f64,
                },
            }];
            let planner = AnomalyPlanner::new(&anomalies);
            let trials = 5000usize;
            let drops = (0..trials)
                .filter(|&seq| planner.should_drop(Some(seed), seq))
                .count();
            let actual_percent = drops as f64 / trials as f64 * 100.0;
            let target = loss_percent as f64;
            prop_assert!(
                (actual_percent - target).abs() < 3.0,
                "expected ~{}% drops, got {:.2}%",
                target,
                actual_percent
            );
        }
    }
}
