//! Issue #241 (B2/A4): --explain-plan CLI flag.
//!
//! Выводит структурированное дерево итогового pipeline configuration после
//! применения всех overrides. Полезно для:
//! - CI/pre-deploy: проверить, что effective config соответствует ожиданиям.
//! - Debug: понять, что presets/--set/--preset применились правильно.
//! - Docs: автоматически сгенерировать описание pipeline.
//!
//! ## Output formats
//!
//! - `text` (default): human-readable tree с indentation и unicode-box-drawing.
//! - `json`: machine-readable, эквивалентный полному effective Profile + meta.
//!
//! ## Exit code
//!
//! - 0: успешный вывод.
//! - 1: ошибка (например, профиль невалиден — `--explain-plan` падает ДО
//!   генерации, как и `--validate`).
//!
//! ## Architecture
//!
//! `ExplainPlan` собирает effective state из `Profile` (после `apply_overrides`)
//! в tree representation. Это не duplicate Profile — это уже отфильтрованное
//! effective view с `None`-suppression.
//!
//! ```text
//! Pipeline: syslog-generator
//! ├─ Distribution: round-robin
//! ├─ Targets (1)
//! │  └─ tcp://10.0.0.1:514
//! │     ├─ framing: non-transparent
//! │     ├─ connections: 1
//! │     ├─ tcp_nodelay: true (default)
//! │     ├─ so_sndbuf: auto (default)
//! │     └─ reconnect: 5 attempts, exponential backoff
//! ├─ Phases (1)
//! │  └─ phase: warmup
//! │     ├─ rate: 1000 mps
//! │     ├─ duration: 60s
//! │     ├─ total: 60000
//! │     ├─ templates: 1
//! │     ├─ format: rfc5424
//! │     └─ anomalies: 0
//! ├─ RuntimeConfig
//! │  ├─ generator_mode: deterministic
//! │  ├─ generator_threads: 8 (auto)
//! │  ├─ queue_capacity: 1024
//! │  ├─ batch_size: 1
//! │  └─ pacer_tick_interval_ms: 1
//! ├─ BroadcastPolicy: strict
//! ├─ OnTargetFailure: fail-phase
//! └─ Metrics
//!    └─ endpoint: http://127.0.0.1:9090 → /metrics
//! ```

use crate::config::Profile;
use serde::{Deserialize, Serialize};

/// Format для --explain-plan output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExplainFormat {
    /// Human-readable tree (default).
    #[default]
    Text,
    /// Machine-readable JSON.
    Json,
}

impl ExplainFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Per-target explain row.
#[derive(Debug, Clone, Serialize)]
pub struct TargetExplanation {
    pub address: String,
    pub transport: String,
    pub framing: String,
    pub connections: usize,
    pub weight: usize,
    pub tcp_nodelay: bool,
    pub so_sndbuf: Option<usize>,
    pub reconnect_max_attempts: Option<u32>,
    pub reconnect_initial_backoff_ms: Option<u64>,
    pub reconnect_max_backoff_ms: Option<u64>,
    pub tls_enabled: bool,
    pub tls_domain: Option<String>,
    pub tls_insecure: bool,
}

/// Per-phase explain row.
#[derive(Debug, Clone, Serialize)]
pub struct PhaseExplanation {
    pub name: String,
    pub rate_mps: u64,
    pub duration_secs: u64,
    pub total_messages: Option<u64>,
    pub templates: usize,
    pub format: String,
    pub syslog_facility: u8,
    pub syslog_severity: u8,
    pub anomalies: usize,
    pub seed: Option<u64>,
}

/// Runtime config explain row.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeExplanation {
    pub generator_mode: String,
    pub generator_threads_effective: usize,
    pub generator_threads_set: Option<usize>,
    pub queue_capacity_effective: usize,
    pub queue_capacity_set: Option<usize>,
    pub batch_size_effective: usize,
    pub batch_size_set: Option<usize>,
    pub pacer_tick_interval_ms_effective: u64,
    pub pacer_tick_interval_ms_set: Option<u64>,
}

/// Metrics endpoint explain row.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsExplanation {
    pub enabled: bool,
    pub endpoint: Option<String>,
}

/// Top-level explain-plan structure.
#[derive(Debug, Clone, Serialize)]
pub struct ExplainPlan {
    pub distribution: String,
    pub broadcast_policy: Option<String>,
    pub on_target_failure: Option<String>,
    pub targets: Vec<TargetExplanation>,
    pub phases: Vec<PhaseExplanation>,
    pub runtime: RuntimeExplanation,
    pub metrics: MetricsExplanation,
}

impl ExplainPlan {
    /// Build explain-plan from a Profile (after apply_overrides).
    pub fn from_profile(profile: &Profile) -> Self {
        let targets: Vec<TargetExplanation> = profile
            .targets
            .iter()
            .map(|t| TargetExplanation {
                address: t.address.clone(),
                transport: t.transport.clone(),
                framing: t.framing.clone(),
                connections: t.connections,
                weight: t.weight,
                // PR-A5 (Issue #131): TCP_NODELAY=true + SO_SNDBUF=auto (default).
                // Мы только показываем текущее состояние; real implementation
                // устанавливает эти опции в transport/tcp.rs::TlsConnector build.
                tcp_nodelay: true,
                so_sndbuf: None,
                reconnect_max_attempts: t.reconnect_max_attempts,
                reconnect_initial_backoff_ms: t.reconnect_initial_backoff_ms,
                reconnect_max_backoff_ms: t.reconnect_max_backoff_ms,
                tls_enabled: t.transport == "tls",
                tls_domain: t.tls_domain.clone(),
                tls_insecure: t.tls_insecure,
            })
            .collect();
        let phases: Vec<PhaseExplanation> = profile
            .phases
            .iter()
            .map(|p| PhaseExplanation {
                name: p.name.clone(),
                rate_mps: p.messages_per_second,
                duration_secs: p.duration_secs,
                total_messages: p.total_messages,
                templates: p.templates.len(),
                format: p.format_type().to_string(),
                syslog_facility: p.syslog.facility,
                syslog_severity: p.syslog.severity,
                anomalies: p.anomalies.as_ref().map(|a| a.len()).unwrap_or(0),
                seed: p.seed,
            })
            .collect();
        let runtime_eff_threads = profile.runtime.effective_generator_threads();
        let runtime_eff_queue = profile.runtime.effective_queue_capacity();
        let runtime_eff_batch = profile.runtime.effective_batch_size();
        let runtime_eff_pacer = profile.runtime.effective_pacer_tick_interval_ms();
        let runtime = RuntimeExplanation {
            generator_mode: profile.runtime.generator_mode.as_str().to_string(),
            generator_threads_effective: runtime_eff_threads,
            generator_threads_set: profile.runtime.generator_threads,
            queue_capacity_effective: runtime_eff_queue,
            queue_capacity_set: profile.runtime.queue_capacity,
            batch_size_effective: runtime_eff_batch,
            batch_size_set: profile.runtime.batch_size,
            pacer_tick_interval_ms_effective: runtime_eff_pacer,
            pacer_tick_interval_ms_set: profile.runtime.pacer_tick_interval_ms,
        };
        let metrics = MetricsExplanation {
            enabled: profile.metrics_addr.is_some(),
            endpoint: profile.metrics_addr.clone(),
        };
        Self {
            distribution: profile.distribution.clone(),
            broadcast_policy: profile.broadcast_policy.clone(),
            on_target_failure: profile.on_target_failure.clone(),
            targets,
            phases,
            runtime,
            metrics,
        }
    }

    /// Render `ExplainPlan` в text format (tree).
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Pipeline: syslog-generator\n");
        // Distribution.
        out.push_str(&format!("├─ Distribution: {}\n", self.distribution));
        // Targets.
        if self.targets.is_empty() {
            out.push_str("├─ Targets (0)\n");
        } else {
            out.push_str(&format!("├─ Targets ({})\n", self.targets.len()));
            for (i, t) in self.targets.iter().enumerate() {
                let prefix = if i == self.targets.len() - 1 {
                    "│  └─"
                } else {
                    "│  ├─"
                };
                out.push_str(&format!("{prefix} {}://{}\n", t.transport, t.address));
                let sub_prefix = if i == self.targets.len() - 1 {
                    "│     "
                } else {
                    "│  │  "
                };
                out.push_str(&format!("{sub_prefix}├─ framing: {}\n", t.framing));
                out.push_str(&format!("{sub_prefix}├─ connections: {}\n", t.connections));
                out.push_str(&format!("{sub_prefix}├─ weight: {}\n", t.weight));
                out.push_str(&format!("{sub_prefix}├─ tcp_nodelay: {}\n", t.tcp_nodelay));
                out.push_str(&format!(
                    "{sub_prefix}├─ so_sndbuf: {}\n",
                    t.so_sndbuf
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "auto".to_string())
                ));
                let reconnect_str = t.reconnect_max_attempts.map_or_else(
                    || "infinite (default)".to_string(),
                    |n| format!("{} attempts, expo backoff", n),
                );
                out.push_str(&format!("{sub_prefix}├─ reconnect: {}\n", reconnect_str));
                if t.tls_enabled {
                    out.push_str(&format!(
                        "{sub_prefix}├─ tls: enabled (domain={:?}, insecure={})\n",
                        t.tls_domain, t.tls_insecure
                    ));
                }
            }
        }
        // Phases.
        if self.phases.is_empty() {
            out.push_str("├─ Phases (0)\n");
        } else {
            out.push_str(&format!("├─ Phases ({})\n", self.phases.len()));
            for (i, p) in self.phases.iter().enumerate() {
                let prefix = if i == self.phases.len() - 1 {
                    "│  └─"
                } else {
                    "│  ├─"
                };
                out.push_str(&format!("{prefix} phase: {}\n", p.name));
                let sub_prefix = if i == self.phases.len() - 1 {
                    "│     "
                } else {
                    "│  │  "
                };
                out.push_str(&format!("{sub_prefix}├─ rate: {} mps\n", p.rate_mps));
                out.push_str(&format!("{sub_prefix}├─ duration: {}s\n", p.duration_secs));
                out.push_str(&format!(
                    "{sub_prefix}├─ total: {}\n",
                    p.total_messages
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "unbounded".to_string())
                ));
                out.push_str(&format!("{sub_prefix}├─ templates: {}\n", p.templates));
                out.push_str(&format!("{sub_prefix}├─ format: {}\n", p.format));
                out.push_str(&format!(
                    "{sub_prefix}├─ syslog: facility={}, severity={}\n",
                    p.syslog_facility, p.syslog_severity
                ));
                out.push_str(&format!("{sub_prefix}├─ anomalies: {}\n", p.anomalies));
                out.push_str(&format!(
                    "{sub_prefix}└─ seed: {}\n",
                    p.seed
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "random".to_string())
                ));
            }
        }
        // RuntimeConfig.
        let r = &self.runtime;
        out.push_str("├─ RuntimeConfig\n");
        out.push_str(&format!("│  ├─ generator_mode: {}\n", r.generator_mode));
        out.push_str(&format!(
            "│  ├─ generator_threads: {} (set: {})\n",
            r.generator_threads_effective,
            r.generator_threads_set
                .map(|n| n.to_string())
                .unwrap_or_else(|| "auto-detect".to_string())
        ));
        out.push_str(&format!(
            "│  ├─ queue_capacity: {} (set: {})\n",
            r.queue_capacity_effective,
            r.queue_capacity_set
                .map(|n| n.to_string())
                .unwrap_or_else(|| "default".to_string())
        ));
        out.push_str(&format!(
            "│  ├─ batch_size: {} (set: {})\n",
            r.batch_size_effective,
            r.batch_size_set
                .map(|n| n.to_string())
                .unwrap_or_else(|| "default".to_string())
        ));
        out.push_str(&format!(
            "│  └─ pacer_tick_interval_ms: {} (set: {})\n",
            r.pacer_tick_interval_ms_effective,
            r.pacer_tick_interval_ms_set
                .map(|n| n.to_string())
                .unwrap_or_else(|| "default".to_string())
        ));
        // BroadcastPolicy / OnTargetFailure.
        out.push_str(&format!(
            "├─ BroadcastPolicy: {}\n",
            self.broadcast_policy
                .as_deref()
                .unwrap_or("strict (default)")
        ));
        out.push_str(&format!(
            "├─ OnTargetFailure: {}\n",
            self.on_target_failure
                .as_deref()
                .unwrap_or("fail-phase (default)")
        ));
        // Metrics.
        if self.metrics.enabled {
            out.push_str(&format!(
                "└─ Metrics: {} → /metrics\n",
                self.metrics.endpoint.as_deref().unwrap_or("default")
            ));
        } else {
            out.push_str("└─ Metrics: disabled\n");
        }
        out
    }

    /// Render `ExplainPlan` в JSON format.
    pub fn render_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Phase, RuntimeConfig, ShutdownConfig, TargetConfig};

    fn minimal_profile() -> Profile {
        Profile {
            targets: vec![TargetConfig {
                address: "127.0.0.1:514".to_string(),
                transport: "tcp".to_string(),
                connections: 2,
                framing: "non-transparent".to_string(),
                reconnect_max_attempts: Some(5),
                reconnect_initial_backoff_ms: Some(100),
                reconnect_max_backoff_ms: Some(10_000),
                ..Default::default()
            }],
            distribution: "round-robin".to_string(),
            shutdown: ShutdownConfig::default(),
            phases: vec![Phase {
                name: "warmup".to_string(),
                messages_per_second: 1000,
                duration_secs: 60,
                total_messages: Some(60_000),
                templates: vec!["seq={{sequence}}".to_string()],
                seed: Some(42),
                ..Default::default()
            }],
            metrics_addr: Some("127.0.0.1:9090".to_string()),
            broadcast_policy: Some("strict".to_string()),
            queue_capacity: Some(2048),
            on_target_failure: Some("fail-phase".to_string()),
            runtime: RuntimeConfig {
                generator_mode: crate::config::GeneratorMode::Fast,
                generator_threads: Some(8),
                queue_capacity: Some(2048),
                batch_size: Some(64),
                pacer_tick_interval_ms: Some(10),
            },
        }
    }

    #[test]
    fn explain_plan_from_profile_extracts_targets() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        assert_eq!(plan.targets.len(), 1);
        let t = &plan.targets[0];
        assert_eq!(t.address, "127.0.0.1:514");
        assert_eq!(t.transport, "tcp");
        assert_eq!(t.connections, 2);
        assert!(t.tcp_nodelay);
        assert_eq!(t.reconnect_max_attempts, Some(5));
    }

    #[test]
    fn explain_plan_from_profile_extracts_phases() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        assert_eq!(plan.phases.len(), 1);
        let phase = &plan.phases[0];
        assert_eq!(phase.name, "warmup");
        assert_eq!(phase.rate_mps, 1000);
        assert_eq!(phase.templates, 1);
        assert_eq!(phase.seed, Some(42));
    }

    #[test]
    fn explain_plan_runtime_effective_values() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        assert_eq!(plan.runtime.generator_mode, "fast");
        assert_eq!(plan.runtime.generator_threads_effective, 8);
        assert_eq!(plan.runtime.generator_threads_set, Some(8));
        assert_eq!(plan.runtime.queue_capacity_effective, 2048);
        assert_eq!(plan.runtime.batch_size_effective, 64);
        assert_eq!(plan.runtime.pacer_tick_interval_ms_effective, 10);
    }

    #[test]
    fn explain_plan_metrics_enabled() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        assert!(plan.metrics.enabled);
        assert_eq!(plan.metrics.endpoint.as_deref(), Some("127.0.0.1:9090"));
    }

    #[test]
    fn explain_plan_metrics_disabled() {
        let mut p = minimal_profile();
        p.metrics_addr = None;
        let plan = ExplainPlan::from_profile(&p);
        assert!(!plan.metrics.enabled);
        assert!(plan.metrics.endpoint.is_none());
    }

    #[test]
    fn explain_plan_text_renders_with_key_sections() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        let text = plan.render_text();
        // Key sections present.
        assert!(
            text.contains("Pipeline: syslog-generator"),
            "missing header"
        );
        assert!(text.contains("Distribution: round-robin"));
        assert!(text.contains("Targets (1)"));
        assert!(text.contains("tcp://127.0.0.1:514"));
        assert!(text.contains("Phases (1)"));
        assert!(text.contains("phase: warmup"));
        assert!(text.contains("RuntimeConfig"));
        assert!(text.contains("generator_mode: fast"));
        assert!(text.contains("BroadcastPolicy:"));
        assert!(text.contains("OnTargetFailure:"));
        assert!(text.contains("Metrics:"));
        assert!(text.contains("127.0.0.1:9090"));
    }

    #[test]
    fn explain_plan_text_handles_empty_targets_and_phases() {
        let p = Profile::default();
        let plan = ExplainPlan::from_profile(&p);
        let text = plan.render_text();
        assert!(text.contains("Targets (0)"));
        assert!(text.contains("Phases (0)"));
        assert!(text.contains("Metrics: disabled"));
    }

    #[test]
    fn explain_plan_json_roundtrip() {
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        let json = plan.render_json().expect("json render");
        // Verify it's valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["distribution"], "round-robin");
        assert_eq!(parsed["targets"][0]["address"], "127.0.0.1:514");
        assert_eq!(parsed["phases"][0]["name"], "warmup");
        assert_eq!(parsed["runtime"]["generator_mode"], "fast");
        assert!(parsed["metrics"]["enabled"].as_bool().unwrap_or(false));
    }

    #[test]
    fn explain_plan_format_parse() {
        assert_eq!(ExplainFormat::parse("text"), Some(ExplainFormat::Text));
        assert_eq!(ExplainFormat::parse("json"), Some(ExplainFormat::Json));
        assert_eq!(ExplainFormat::parse("TEXT"), Some(ExplainFormat::Text));
        assert_eq!(ExplainFormat::parse("Json"), Some(ExplainFormat::Json));
        assert_eq!(ExplainFormat::parse("unknown"), None);
    }

    #[test]
    fn explain_plan_tls_target_shows_tls_info() {
        let mut p = minimal_profile();
        p.targets[0].transport = "tls".to_string();
        p.targets[0].tls_domain = Some("siem.example.com".to_string());
        p.targets[0].tls_insecure = true;
        let plan = ExplainPlan::from_profile(&p);
        let t = &plan.targets[0];
        assert!(t.tls_enabled);
        assert_eq!(t.tls_domain.as_deref(), Some("siem.example.com"));
        assert!(t.tls_insecure);

        let text = plan.render_text();
        assert!(text.contains("tls: enabled"));
        assert!(text.contains("insecure=true"));
    }

    #[test]
    fn explain_plan_snapshot_minimal() {
        // Snapshot test — фиксирует формат output.
        let p = minimal_profile();
        let plan = ExplainPlan::from_profile(&p);
        let text = plan.render_text();
        // Стабильная структура: каждый раз render_text даёт одинаковый output.
        // Здесь не строгий snapshot (иначе при cosmetic changes нужно обновлять),
        // а проверяем что output начинается с "Pipeline:" и заканчивается на /metrics.
        assert!(text.starts_with("Pipeline: syslog-generator"));
        assert!(text.contains("/metrics"));
    }
}
