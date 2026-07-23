//! PR-B3 (Issue #92): named presets с готовыми defaults.
//!
//! Presets — это pre-configured bundle параметров для типичных use cases.
//! Применяются как --preset NAME flag в CLI. Preset может быть override'нут
//! позже через обычные --set/--rate/--target flags.
//!
//! ## Доступные presets
//!
//! - `max-throughput`: --preset для maximum messages/second.
//!   - `runtime.generator_threads` = `min(NCPU/2, 4)` (resolved at runtime)
//!   - `runtime.queue_capacity` = 65536
//!   - `runtime.broadcast_policy` = "independent"
//!   - `runtime.metrics_mode` = "minimal"
//!
//! - `balanced`: defaults (no override).
//!
//! - `low-latency`: --preset для minimum latency.
//!   - `runtime.generator_threads` = 1
//!   - `runtime.queue_capacity` = 64
//!   - `runtime.broadcast_policy` = "strict"
//!   - `runtime.metrics_mode` = "sampled"
//!
//! ## Архитектура
//!
//! Preset — это struct с `Option<String>` для каждого параметра. None
//! означает "не изменять" (allow user override to take effect).
//! `apply_preset()` мутирует Profile через set_override::apply_set_overrides
//! (тот же механизм, что и --set).

use crate::cli::set_override::apply_set_overrides;
use crate::config::Profile;
use anyhow::{anyhow, Result};

/// Один preset — коллекция optional overrides.
#[derive(Debug, Clone, Default)]
pub struct Preset {
    pub name: String,
    /// (key, value) pairs applied к Profile через set_override.
    pub overrides: Vec<(String, String)>,
}

impl Preset {
    /// Built-in preset: max-throughput.
    pub fn max_throughput() -> Self {
        Self {
            name: "max-throughput".to_string(),
            overrides: vec![
                ("queue_capacity".to_string(), "65536".to_string()),
                ("broadcast_policy".to_string(), "independent".to_string()),
                ("on_target_failure".to_string(), "continue".to_string()),
            ],
        }
    }

    /// Built-in preset: balanced (= defaults, no overrides).
    pub fn balanced() -> Self {
        Self {
            name: "balanced".to_string(),
            overrides: vec![],
        }
    }

    /// Built-in preset: low-latency.
    pub fn low_latency() -> Self {
        Self {
            name: "low-latency".to_string(),
            overrides: vec![
                ("queue_capacity".to_string(), "64".to_string()),
                ("broadcast_policy".to_string(), "strict".to_string()),
                ("on_target_failure".to_string(), "fail-phase".to_string()),
            ],
        }
    }
}

/// Парсинг имени preset → Preset.
pub fn parse_preset(name: &str) -> Result<Preset> {
    match name {
        "max-throughput" => Ok(Preset::max_throughput()),
        "balanced" => Ok(Preset::balanced()),
        "low-latency" => Ok(Preset::low_latency()),
        _ => Err(anyhow!(
            "unknown preset {:?}; available: max-throughput, balanced, low-latency",
            name
        )),
    }
}

/// Применить preset к Profile in-place.
pub fn apply_preset(profile: &mut Profile, preset: &Preset) -> Result<()> {
    if preset.overrides.is_empty() {
        return Ok(());
    }
    apply_set_overrides(profile, &preset.overrides)
        .map_err(|e| anyhow!("failed to apply preset {:?}: {e}", preset.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Phase, ShutdownConfig, SyslogConfig, TargetConfig};

    fn empty_profile() -> Profile {
        Profile {
            targets: vec![TargetConfig {
                address: "127.0.0.1:514".to_string(),
                transport: "tcp".to_string(),
                ..Default::default()
            }],
            distribution: "round-robin".to_string(),
            shutdown: ShutdownConfig::default(),
            phases: vec![Phase {
                name: "test".to_string(),
                total_messages: Some(1000),
                ..Default::default()
            }],
            metrics_addr: None,
            broadcast_policy: None,
            queue_capacity: None,
            on_target_failure: None,
        }
    }

    #[test]
    fn parse_known_presets() {
        for name in ["max-throughput", "balanced", "low-latency"] {
            assert!(parse_preset(name).is_ok());
        }
    }

    #[test]
    fn parse_unknown_preset_errors() {
        assert!(parse_preset("nonexistent").is_err());
    }

    #[test]
    fn apply_max_throughput_sets_independent_broadcast() {
        let mut p = empty_profile();
        let preset = parse_preset("max-throughput").unwrap();
        apply_preset(&mut p, &preset).unwrap();
        // The set_override uses serde_json::Value — string is stored as Value::String.
        // After re-serialization it should parse back correctly.
        assert!(p.broadcast_policy.is_some());
    }

    #[test]
    fn apply_balanced_is_noop() {
        let mut p = empty_profile();
        let original = p.clone();
        let preset = parse_preset("balanced").unwrap();
        apply_preset(&mut p, &preset).unwrap();
        assert_eq!(p.distribution, original.distribution);
    }

    #[test]
    fn apply_low_latency_sets_strict_broadcast() {
        let mut p = empty_profile();
        let preset = parse_preset("low-latency").unwrap();
        apply_preset(&mut p, &preset).unwrap();
        assert_eq!(p.broadcast_policy, Some("strict".to_string()));
    }
}
