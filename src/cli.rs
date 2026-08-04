//! F11 — интерфейс командной строки и применение CLI-оверрайдов к профилю.
//!
//! Разделение ответственности:
//!   * [`Args`] — декларативный разбор аргументов (clap derive).
//!   * [`Overrides`] — «чистое» представление переопределений без завязки на clap,
//!     что делает [`apply_overrides`] полностью юнит-тестируемым.
//!
//! Оверрайды применяются к загруженному из файла (или пустому) [`Profile`] ПЕРЕД
//! валидацией и запуском. Скалярные оверрайды фаз (rate/duration/total/format/seed)
//! применяются ко ВСЕМ фазам — это осознанный выбор для быстрых экспериментов из
//! CLI; для тонкой настройки отдельных фаз используйте JSON-профиль.

use crate::config::{GeneratorMode, Phase, Profile, RuntimeConfig, TargetConfig};
use clap::{Parser, Subcommand};

/// v10.6.0: subcommand'ы для генерации shell completions и man page.
/// Главный `Args` (default subcommand) продолжает работать как раньше —
/// все существующие CLI флаги работают без `--<subcommand>`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate shell completions для указанной shell.
    /// Использование: `syslog-generator completions bash > /etc/bash_completion.d/syslog-generator`
    Completions {
        /// Shell для генерации (bash, zsh, fish, powershell, elvish).
        shell: clap_complete::Shell,
    },

    /// Generate man page в stdout (roff format).
    /// Использование: `syslog-generator man > syslog-generator.1`
    Man,
}

#[derive(Parser, Debug, Default)]
#[command(
    name = "syslog-generator",
    version,
    about = "Промышленный генератор нагрузки на syslog (multi-target, профили нагрузки, вариативный пейлоад)",
    long_about = None
)]
pub struct Args {
    /// v10.6.0: subcommand для utility-операций (completions, man).
    /// По умолчанию None — main run profile (как раньше).
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Путь к JSON/YAML-профилю нагрузки. Без него профиль собирается из CLI-флагов
    /// (нужен хотя бы один --target и источник контента, напр. --message).
    #[arg(short, long, visible_alias = "config")]
    pub profile: Option<String>,

    /// Цель в форме ADDR (адрес). Транспорт задаётся отдельно через --transport.
    /// Флаг повторяемый; заменяет targets из профиля.
    /// Примеры: -t 127.0.0.1:514, -t 10.0.0.1:6514, -t /tmp/out.log
    ///
    /// **Deprecated**: формат `ADDR:TRANSPORT` (например `-t 10.0.0.1:6514:tls`)
    /// ещё работает в v10.x как alias, но будет удалён в v11.0.0. Вместо этого
    /// используйте `-t ADDR` + `--transport TRANSPORT`.
    #[arg(short = 't', long = "target")]
    pub target: Vec<String>,

    /// B5 (v10.1.0): транспорт для всех --target (tcp|udp|tls|file; default tcp).
    /// Применяется к каждой цели, если её transport не указан в `ADDR:TRANSPORT`
    /// (последний имеет приоритет, но deprecated).
    #[arg(long = "transport", value_parser = ["tcp", "udp", "tls", "file"])]
    pub transport: Option<String>,

    /// Переопределить distribution (round-robin|broadcast|weighted) во всём профиле.
    #[arg(long)]
    pub distribution: Option<String>,

    /// Переопределить messages_per_second во ВСЕХ фазах.
    #[arg(long)]
    pub rate: Option<u64>,

    /// Переопределить duration_secs во ВСЕХ фазах.
    #[arg(long)]
    pub duration: Option<u64>,

    /// Переопределить total_messages во ВСЕХ фазах.
    #[arg(long)]
    pub total: Option<u64>,

    /// Переопределить format (rfc5424|rfc3164|raw|protobuf) во ВСЕХ фазах.
    #[arg(long)]
    pub format: Option<String>,

    /// Переопределить seed ГПСЧ во ВСЕХ фазах (детерминированная генерация).
    #[arg(long)]
    pub seed: Option<u64>,

    /// Шаблон сообщения для быстрого запуска без файла профиля (повторяемый).
    /// Создаёт единственную фазу с этими шаблонами.
    #[arg(short = 'm', long = "message")]
    pub message: Vec<String>,

    /// Только проверить профиль (валидация) и выйти; ничего не отправлять.
    #[arg(long)]
    pub validate: bool,

    /// v10.7.0: --dry-run — загрузить и валидировать профиль, но НЕ отправлять.
    /// Полезно для CI: проверяет профиль без реальной нагрузки.
    /// Требует --profile (иначе нечего валидировать).
    #[arg(long)]
    pub dry_run: bool,

    /// D3: дополнительно к семантической валидации (F13) проверить профиль
    /// против формальной JSON Schema (`schemas/profile.schema.json`). Полезно
    /// для CI и для отлова структурных ошибок (неправильные типы, неизвестные
    /// ключи, значения вне диапазонов 0..=23/0..=7) до старта прогона.
    #[arg(long)]
    pub schema_strict: bool,

    /// Вывести итоговый профиль (после оверрайдов) как JSON и выйти.
    #[arg(long)]
    pub print_config: bool,

    /// F12: адрес HTTP-эндпоинта /metrics (напр. 127.0.0.1:9090).
    /// Переопределяет metrics_addr из профиля.
    #[arg(long)]
    pub metrics_addr: Option<String>,

    /// PR-B2 (Issue #83): --set KEY=VALUE для точечного override любого
    /// публичного поля профиля. KEY — точечный JSON path:
    /// `targets[0].connections`, `phases[0].messages_per_second`, etc.
    /// Может повторяться. Сначала применяется profile → затем --set overrides.
    #[arg(long, value_name = "KEY=VALUE")]
    pub set: Vec<String>,

    /// PR-B3 (Issue #92): --preset NAME — применить named preset с готовыми
    /// defaults. Доступные presets:
    /// - `max-throughput`: generator_threads=max, batch_size=large,
    ///   broadcast_policy=independent, metrics=minimal.
    /// - `balanced`: defaults (default).
    /// - `low-latency`: generator_threads=1, batch_size=1,
    ///   broadcast_policy=strict, metrics=sampled.
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    /// PR-B1 (Issue #93): --tls-ca-file PATH — переопределить TargetConfig::tls_ca_file.
    #[arg(long, value_name = "PATH")]
    pub tls_ca_file: Option<String>,

    /// PR-B1: --tls-domain DOMAIN — переопределить TargetConfig::tls_domain.
    #[arg(long, value_name = "DOMAIN")]
    pub tls_domain: Option<String>,

    /// PR-B1: --tls-insecure — переопределить TargetConfig::tls_insecure.
    #[arg(long)]
    pub tls_insecure: bool,

    /// PR-B1: --connections N — переопределить TargetConfig::connections.
    #[arg(long, value_name = "N")]
    pub connections: Option<usize>,

    /// PR-B1: --framing MODE — переопределить TargetConfig::framing
    /// (octet-counting | non-transparent).
    #[arg(long, value_name = "MODE")]
    pub framing: Option<String>,

    /// A4 (Issue #236): --generator-mode MODE — переопределить
    /// `RuntimeConfig::generator_mode` (deterministic|fast).
    /// Deterministic — byte-for-byte identical output (replay-friendly).
    /// Fast — thread-local RNG, max throughput.
    #[arg(long, value_name = "MODE", value_parser = ["deterministic", "fast"])]
    pub generator_mode: Option<String>,

    /// A4 (Issue #236): --generator-threads N — число generator worker threads.
    /// None → автоопределение (NCPU или 1).
    #[arg(long, value_name = "N")]
    pub generator_threads: Option<usize>,

    /// Issue #238 / A4: --batch-size N — батч размер для pipeline mode.
    /// None → 1 (no batching). Clamped до >= 1.
    #[arg(long, value_name = "N")]
    pub batch_size: Option<usize>,

    /// A4 (Issue #236): --pacer-tick-interval MS — pacer tick interval в мс.
    /// None → 1ms (sub-millisecond precision). Clamped до >= 1.
    #[arg(long, value_name = "MS")]
    pub pacer_tick_interval: Option<u64>,
}

/// «Чистое» представление CLI-оверрайдов, не зависящее от clap.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub targets: Vec<TargetConfig>,
    pub distribution: Option<String>,
    pub rate: Option<u64>,
    pub duration: Option<u64>,
    pub total: Option<u64>,
    pub format: Option<String>,
    pub seed: Option<u64>,
    pub messages: Vec<String>,
    pub metrics_addr: Option<String>,
    /// PR-B2: KEY=VALUE точечные overrides, применённые после parsing
    /// профиля и стандартных overrides.
    pub set_overrides: Vec<(String, String)>,
    /// PR-B3: имя preset для применения готовых defaults.
    pub preset: Option<String>,
    /// PR-B1: per-target overrides из CLI.
    pub tls_ca_file: Option<String>,
    pub tls_domain: Option<String>,
    pub tls_insecure: bool,
    pub connections: Option<usize>,
    pub framing: Option<String>,
    /// A4 (Issue #236): Runtime configuration overrides из CLI.
    /// None внутри Option означает "не переопределять соответствующее поле".
    pub runtime: RuntimeConfig,
    /// A4 tracker: какие runtime-поля явно заданы через CLI
    /// (используется в `apply_overrides` чтобы не затирать существующие
    /// значения из Profile `runtime` блока).
    #[doc(hidden)]
    pub runtime_overrides_set: RuntimeOverridesSet,
}

/// Helper: tracks which A4 runtime fields explicitly set via CLI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuntimeOverridesSet {
    pub generator_mode: bool,
    pub generator_threads: bool,
    pub queue_capacity: bool,
    pub batch_size: bool,
    pub pacer_tick_interval: bool,
}

/// Ошибка разбора спецификации цели `--target`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TargetParseError {
    #[error("пустая спецификация --target")]
    Empty,
    #[error("--target {0:?}: слишком много компонентов (ожидается ADDR или ADDR:TRANSPORT)")]
    TooManyParts(String),
}

/// Разбирает `ADDR` или (deprecated) `ADDR:TRANSPORT` в [`TargetConfig`].
///
/// **B5 (v10.1.0)**: `ADDR:TRANSPORT` формат deprecated, будет удалён в v11.0.0.
/// Используйте `parse_target_with_transport(spec, default_transport)` или
/// задавайте транспорт через `--transport` флаг.
///
/// Для `host:port` (две части, где вторая — числовой порт) весь ввод трактуется
/// как адрес с транспортом по умолчанию tcp. Если последняя часть — известный
/// транспорт (tcp/udp/tls/file), она отделяется. Для `file` адрес — это путь.
pub fn parse_target(spec: &str) -> Result<TargetConfig, TargetParseError> {
    parse_target_with_transport(spec, None)
}

/// B5 (v10.1.0): разбирает spec с явным дефолтным транспортом.
///
/// Если spec в формате `ADDR:TRANSPORT` (deprecated), возвращает Result с
/// уже распарсенным транспортом и пишет warning в stderr.
///
/// Если spec в формате `ADDR`, используется `default_transport` (или "tcp"
/// если None).
pub fn parse_target_with_transport(
    spec: &str,
    default_transport: Option<&str>,
) -> Result<TargetConfig, TargetParseError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(TargetParseError::Empty);
    }
    let known = ["tcp", "udp", "tls", "file"];

    // Deprecated формат ADDR:TRANSPORT.
    if let Some((addr, last)) = spec.rsplit_once(':') {
        if known.contains(&last) {
            if addr.is_empty() {
                return Err(TargetParseError::Empty);
            }
            eprintln!(
                "warning: формат `--target ADDR:TRANSPORT` deprecated (получено {:?}); \
                 используйте `-t ADDR --transport {}` (будет удалено в v11.0.0)",
                spec, last
            );
            return Ok(TargetConfig {
                address: addr.to_string(),
                transport: last.to_string(),
                ..Default::default()
            });
        }
    }

    // Новый формат: ADDR + default_transport (или "tcp").
    Ok(TargetConfig {
        address: spec.to_string(),
        transport: default_transport.unwrap_or("tcp").to_string(),
        ..Default::default()
    })
}

impl Args {
    /// Преобразует разобранные аргументы в [`Overrides`], разбирая `--target`.
    ///
    /// B5 (v10.1.0): `--transport` применяется ко всем targets, у которых не
    /// указан транспорт в deprecated `ADDR:TRANSPORT` формате.
    pub fn to_overrides(&self) -> Result<Overrides, TargetParseError> {
        let mut targets = Vec::new();
        for t in &self.target {
            let mut tgt = parse_target_with_transport(t, self.transport.as_deref())?;
            // PR-B1: apply per-target CLI overrides to ALL targets.
            if let Some(ref ca) = self.tls_ca_file {
                tgt.tls_ca_file = Some(ca.clone());
            }
            if let Some(ref domain) = self.tls_domain {
                tgt.tls_domain = Some(domain.clone());
            }
            if self.tls_insecure {
                tgt.tls_insecure = true;
            }
            if let Some(conns) = self.connections {
                tgt.connections = conns;
            }
            if let Some(ref f) = self.framing {
                tgt.framing = f.clone();
            }
            targets.push(tgt);
        }
        // A4 (Issue #236): build RuntimeConfig from CLI flags.
        // `RuntimeConfig::default()` имеет все None / defaults, поэтому
        // эффективно конструируем fields в зависимости от того, что задано.
        let mut runtime = RuntimeConfig::default();
        let mut runtime_overrides_set = RuntimeOverridesSet::default();
        if let Some(ref mode) = self.generator_mode {
            runtime.generator_mode = GeneratorMode::parse(mode).unwrap_or_else(|| {
                // clap value_parser уже отфильтровал невалидные значения,
                // но на всякий случай — fallback на Deterministic.
                eprintln!(
                    "warning: --generator-mode {mode:?} неизвестный; \
                     используется deterministic"
                );
                GeneratorMode::Deterministic
            });
            runtime_overrides_set.generator_mode = true;
        }
        if let Some(n) = self.generator_threads {
            runtime.generator_threads = Some(n);
            runtime_overrides_set.generator_threads = true;
        }
        if let Some(n) = self.batch_size {
            runtime.batch_size = Some(n);
            runtime_overrides_set.batch_size = true;
        }
        if let Some(ms) = self.pacer_tick_interval {
            runtime.pacer_tick_interval_ms = Some(ms);
            runtime_overrides_set.pacer_tick_interval = true;
        }

        Ok(Overrides {
            targets,
            distribution: self.distribution.clone(),
            rate: self.rate,
            duration: self.duration,
            total: self.total,
            format: self.format.clone(),
            seed: self.seed,
            messages: self.message.clone(),
            metrics_addr: self.metrics_addr.clone(),
            set_overrides: self
                .set
                .iter()
                .filter_map(|s| {
                    let mut parts = s.splitn(2, '=');
                    let key = parts.next()?.to_string();
                    let value = parts.next()?.to_string();
                    if key.is_empty() || value.is_empty() {
                        None
                    } else {
                        Some((key, value))
                    }
                })
                .collect(),
            preset: self.preset.clone(),
            tls_ca_file: self.tls_ca_file.clone(),
            tls_domain: self.tls_domain.clone(),
            tls_insecure: self.tls_insecure,
            connections: self.connections,
            framing: self.framing.clone(),
            runtime,
            runtime_overrides_set,
        })
    }
}

/// Применяет оверрайды к профилю на месте.
///
/// Порядок:
///   1. `--target` заменяет `targets` (если задан хотя бы один).
///   2. `--distribution` заменяет `distribution`.
///   3. `--message` создаёт фазу, если фаз нет (быстрый режим из CLI).
///   4. Скалярные оверрайды фаз применяются ко всем фазам.
pub fn apply_overrides(profile: &mut Profile, o: &Overrides) {
    if !o.targets.is_empty() {
        profile.targets = o.targets.clone();
    }
    if let Some(d) = &o.distribution {
        profile.distribution = d.clone();
    }
    if let Some(addr) = &o.metrics_addr {
        profile.metrics_addr = Some(addr.clone());
    }

    // Быстрый режим: если фаз нет, но заданы сообщения — создаём фазу.
    if profile.phases.is_empty() && !o.messages.is_empty() {
        profile.phases.push(Phase {
            name: "cli".to_string(),
            templates: o.messages.clone(),
            ..Default::default()
        });
    } else if !o.messages.is_empty() {
        // Если фазы есть и заданы сообщения — переопределяем шаблоны во всех фазах.
        for p in &mut profile.phases {
            p.templates = o.messages.clone();
            p.templates_file = None;
        }
    }

    for p in &mut profile.phases {
        if let Some(r) = o.rate {
            p.messages_per_second = r;
        }
        if let Some(d) = o.duration {
            p.duration_secs = d;
        }
        if let Some(t) = o.total {
            p.total_messages = Some(t);
        }
        if let Some(f) = &o.format {
            p.format = Some(f.clone());
        }
        if let Some(s) = o.seed {
            p.seed = Some(s);
        }
    }
    // PR-B3 (Issue #92): apply preset до --set overrides.
    // Preset — это pre-configured bundle (например, max-throughput,
    // low-latency), --set может override'ить отдельные поля preset'а.
    if let Some(preset_name) = &o.preset {
        match crate::cli::preset::parse_preset(preset_name) {
            Ok(preset) => {
                if let Err(e) = crate::cli::preset::apply_preset(profile, &preset) {
                    eprintln!("warning: --preset {preset_name:?} failed: {e}");
                }
            }
            Err(e) => {
                eprintln!("warning: {e}");
            }
        }
    }

    // PR-B2 (Issue #83): apply --set точечные overrides (последний шаг,
    // перезаписывает всё предыдущее). Errors логируются в stderr, но
    // не паникуют — N7 invariant запрещает .expect()/.unwrap() в prod.
    if !o.set_overrides.is_empty() {
        if let Err(e) = crate::cli::set_override::apply_set_overrides(profile, &o.set_overrides) {
            eprintln!("warning: --set overrides failed: {e}");
        }
    }

    // A4 (Issue #236): apply runtime overrides LAST (после --set, после preset),
    // чтобы CLI флаги явно override'или всё предыдущее. Для каждого поля
    // из `runtime_overrides_set` — перезаписываем соответствующее поле
    // `profile.runtime`. Если поле НЕ задано через CLI — оставляем как было
    // (из Profile YAML или RuntimeConfig::default()).
    let set = &o.runtime_overrides_set;
    if set.generator_mode {
        profile.runtime.generator_mode = o.runtime.generator_mode;
    }
    if set.generator_threads {
        profile.runtime.generator_threads = o.runtime.generator_threads;
    }
    if set.queue_capacity {
        // Issue #238: queue_capacity хранится в runtime, но для
        // backward-compat (и для `run_phase_multi`) также синхронизируем
        // с profile.queue_capacity (которое historically используется
        // как Profile-уровневое поле).
        profile.runtime.queue_capacity = o.runtime.queue_capacity;
        profile.queue_capacity = o.runtime.queue_capacity;
    }
    if set.batch_size {
        profile.runtime.batch_size = o.runtime.batch_size;
    }
    if set.pacer_tick_interval {
        profile.runtime.pacer_tick_interval_ms = o.runtime.pacer_tick_interval_ms;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_addr_only_defaults_tcp() {
        let t = parse_target("127.0.0.1:514").unwrap();
        assert_eq!(t.address, "127.0.0.1:514");
        assert_eq!(t.transport, "tcp");
    }

    #[test]
    fn parse_target_with_transport_deprecated_format() {
        // B5: старая тест-функция, переименована во избежание конфликта с
        // новой pub fn parse_target_with_transport(spec, default_transport).
        let t = parse_target("10.0.0.1:6514:tls").unwrap();
        assert_eq!(t.address, "10.0.0.1:6514");
        assert_eq!(t.transport, "tls");
    }

    #[test]
    fn parse_target_file_path() {
        let t = parse_target("/tmp/out.log:file").unwrap();
        assert_eq!(t.address, "/tmp/out.log");
        assert_eq!(t.transport, "file");
    }

    #[test]
    fn parse_target_udp() {
        let t = parse_target("192.168.1.1:514:udp").unwrap();
        assert_eq!(t.address, "192.168.1.1:514");
        assert_eq!(t.transport, "udp");
    }

    #[test]
    fn parse_target_empty_errors() {
        assert!(matches!(parse_target("  "), Err(TargetParseError::Empty)));
    }

    #[test]
    fn b5_parse_target_with_transport_default() {
        // Новый формат: --transport TRANSPORT применяется ко всем targets.
        let t = parse_target_with_transport("10.0.0.1:514", Some("tls")).unwrap();
        assert_eq!(t.address, "10.0.0.1:514");
        assert_eq!(t.transport, "tls");
    }

    #[test]
    fn b5_parse_target_with_transport_none_defaults_tcp() {
        // Без default — fallback на tcp (обратная совместимость).
        let t = parse_target_with_transport("10.0.0.1:514", None).unwrap();
        assert_eq!(t.address, "10.0.0.1:514");
        assert_eq!(t.transport, "tcp");
    }

    #[test]
    fn b5_parse_target_deprecated_with_transport_arg() {
        // Deprecated формат ADDR:TRANSPORT работает даже если задан default_transport.
        // Формат ADDR:TRANSPORT имеет приоритет (но пишет warning в stderr).
        let t = parse_target_with_transport("10.0.0.1:6514:tls", Some("udp")).unwrap();
        assert_eq!(t.address, "10.0.0.1:6514");
        assert_eq!(t.transport, "tls"); // из deprecated формата, не из --transport
    }

    #[test]
    fn apply_overrides_replaces_targets() {
        let mut p = Profile::default();
        let o = Overrides {
            targets: vec![parse_target("1.2.3.4:514:udp").unwrap()],
            ..Default::default()
        };
        apply_overrides(&mut p, &o);
        assert_eq!(p.targets.len(), 1);
        assert_eq!(p.targets[0].transport, "udp");
    }

    #[test]
    fn apply_overrides_scalars_all_phases() {
        let mut p = Profile {
            phases: vec![
                Phase {
                    name: "a".into(),
                    ..Default::default()
                },
                Phase {
                    name: "b".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let o = Overrides {
            rate: Some(500),
            duration: Some(30),
            total: Some(1000),
            format: Some("rfc3164".into()),
            seed: Some(42),
            ..Default::default()
        };
        apply_overrides(&mut p, &o);
        for ph in &p.phases {
            assert_eq!(ph.messages_per_second, 500);
            assert_eq!(ph.duration_secs, 30);
            assert_eq!(ph.total_messages, Some(1000));
            assert_eq!(ph.format.as_deref(), Some("rfc3164"));
            assert_eq!(ph.seed, Some(42));
        }
    }

    #[test]
    fn apply_overrides_message_creates_phase() {
        let mut p = Profile::default();
        let o = Overrides {
            messages: vec!["hello {{sequence}}".into()],
            ..Default::default()
        };
        apply_overrides(&mut p, &o);
        assert_eq!(p.phases.len(), 1);
        assert_eq!(
            p.phases[0].templates,
            vec!["hello {{sequence}}".to_string()]
        );
    }

    // === v10.6.0 (Usability ч.1): тесты subcommand'ов ===

    /// `Args::command()` содержит subcommands `completions` и `man`.
    #[test]
    fn v10_6_0_args_has_completions_and_man_subcommands() {
        use clap::CommandFactory;
        let app = Args::command();
        let subcommands: Vec<&str> = app.get_subcommands().map(|c| c.get_name()).collect();
        assert!(
            subcommands.contains(&"completions"),
            "missing 'completions' subcommand; got: {:?}",
            subcommands
        );
        assert!(
            subcommands.contains(&"man"),
            "missing 'man' subcommand; got: {:?}",
            subcommands
        );
    }

    /// `Command` enum имеет 2 варианта: Completions и Man.
    #[test]
    fn v10_6_0_command_enum_variants() {
        // Compile-time check: варианты существуют с правильными полями.
        // Runtime: проверяем Debug output (используется в clap error messages).
        let c = Command::Completions {
            shell: clap_complete::Shell::Bash,
        };
        let s = format!("{:?}", c);
        assert!(s.contains("Completions"));
        assert!(s.contains("Bash"));
        let m = Command::Man;
        let s = format!("{:?}", m);
        assert!(s.contains("Man"));
    }

    /// `Args::parse_from(["binary", "completions", "bash"])` корректно
    /// dispatch'ит в subcommand.
    #[test]
    fn v10_6_0_args_parses_completions_subcommand() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "completions", "bash"]);
        match args.command {
            Some(Command::Completions { shell }) => {
                assert!(matches!(shell, clap_complete::Shell::Bash));
            }
            other => panic!("expected Completions(Bash), got {other:?}"),
        }
    }

    /// `Args::parse_from(["binary", "man"])` корректно dispatch'ит.
    #[test]
    fn v10_6_0_args_parses_man_subcommand() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "man"]);
        assert!(matches!(args.command, Some(Command::Man)));
    }

    /// Без subcommand — `command` = None (default main run profile).
    #[test]
    fn v10_6_0_args_no_subcommand_means_main() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json"]);
        assert!(args.command.is_none(), "default subcommand не задан");
        assert_eq!(args.profile.as_deref(), Some("x.json"));
    }

    // === v10.7.0 (Usability ч.2): тесты для --dry-run ===

    /// `--dry-run` парсится корректно.
    #[test]
    fn v10_7_0_dry_run_flag_parses() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json", "--dry-run"]);
        assert!(args.dry_run, "--dry-run должен быть true");
        // --dry-run не отменяет другие флаги.
        assert_eq!(args.profile.as_deref(), Some("x.json"));
    }

    /// `--dry-run` по умолчанию false.
    #[test]
    fn v10_7_0_dry_run_default_false() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json"]);
        assert!(!args.dry_run, "--dry-run по умолчанию false");
    }

    // === A4 (Issue #236): CLI flags для RuntimeConfig ===

    /// `--generator-mode fast` парсится в RuntimeConfig.
    #[test]
    fn a4_generator_mode_fast_parses() {
        use clap::Parser;
        let args = Args::parse_from([
            "syslog-generator",
            "-p",
            "x.json",
            "--generator-mode",
            "fast",
        ]);
        assert_eq!(args.generator_mode.as_deref(), Some("fast"));

        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime.generator_mode, GeneratorMode::Fast);
        assert!(o.runtime_overrides_set.generator_mode);
    }

    /// `--generator-mode deterministic` (default).
    #[test]
    fn a4_generator_mode_deterministic_default() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json"]);
        assert!(args.generator_mode.is_none());
        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime.generator_mode, GeneratorMode::Deterministic);
        assert!(!o.runtime_overrides_set.generator_mode);
    }

    /// `--generator-mode` отвергает невалидные значения (clap value_parser).
    #[test]
    fn a4_generator_mode_invalid_rejected() {
        use clap::Parser;
        let result = Args::try_parse_from([
            "syslog-generator",
            "-p",
            "x.json",
            "--generator-mode",
            "wrong",
        ]);
        assert!(result.is_err(), "clap должен reject'ить unknown mode");
    }

    /// `--generator-threads N` парсится.
    #[test]
    fn a4_generator_threads_parses() {
        use clap::Parser;
        let args = Args::parse_from([
            "syslog-generator",
            "-p",
            "x.json",
            "--generator-threads",
            "8",
        ]);
        assert_eq!(args.generator_threads, Some(8));
        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime.generator_threads, Some(8));
        assert!(o.runtime_overrides_set.generator_threads);
    }

    /// `--batch-size N` парсится.
    #[test]
    fn a4_batch_size_parses() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json", "--batch-size", "64"]);
        assert_eq!(args.batch_size, Some(64));
        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime.batch_size, Some(64));
        assert!(o.runtime_overrides_set.batch_size);
    }

    /// `--pacer-tick-interval MS` парсится.
    #[test]
    fn a4_pacer_tick_interval_parses() {
        use clap::Parser;
        let args = Args::parse_from([
            "syslog-generator",
            "-p",
            "x.json",
            "--pacer-tick-interval",
            "10",
        ]);
        assert_eq!(args.pacer_tick_interval, Some(10));
        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime.pacer_tick_interval_ms, Some(10));
        assert!(o.runtime_overrides_set.pacer_tick_interval);
    }

    /// По умолчанию все runtime flags — None.
    #[test]
    fn a4_runtime_flags_default_none() {
        use clap::Parser;
        let args = Args::parse_from(["syslog-generator", "-p", "x.json"]);
        let o = args.to_overrides().expect("to_overrides ok");
        assert_eq!(o.runtime, RuntimeConfig::default());
        assert!(!o.runtime_overrides_set.generator_mode);
        assert!(!o.runtime_overrides_set.generator_threads);
        assert!(!o.runtime_overrides_set.batch_size);
        assert!(!o.runtime_overrides_set.pacer_tick_interval);
    }

    /// `apply_overrides` применяет runtime overrides к Profile.
    #[test]
    fn a4_apply_overrides_sets_runtime_config() {
        let mut p = Profile::default();
        let o = Overrides {
            runtime: RuntimeConfig {
                generator_mode: GeneratorMode::Fast,
                generator_threads: Some(4),
                queue_capacity: Some(65536),
                batch_size: Some(64),
                pacer_tick_interval_ms: Some(10),
            },
            runtime_overrides_set: RuntimeOverridesSet {
                generator_mode: true,
                generator_threads: true,
                queue_capacity: true,
                batch_size: true,
                pacer_tick_interval: true,
            },
            ..Default::default()
        };
        apply_overrides(&mut p, &o);
        assert_eq!(p.runtime.generator_mode, GeneratorMode::Fast);
        assert_eq!(p.runtime.generator_threads, Some(4));
        assert_eq!(p.runtime.batch_size, Some(64));
        assert_eq!(p.runtime.pacer_tick_interval_ms, Some(10));
        // Issue #238: queue_capacity синхронизируется BOTH в profile.runtime
        // (для YAML-style config) AND в profile.queue_capacity (для
        // backward-compat с `run_phase_multi`).
        assert_eq!(p.runtime.queue_capacity, Some(65536));
        assert_eq!(p.queue_capacity, Some(65536));
    }

    /// `apply_overrides` сохраняет runtime из Profile если CLI флаги не заданы.
    #[test]
    fn a4_apply_overrides_preserves_profile_runtime() {
        let mut p = Profile {
            runtime: RuntimeConfig {
                generator_mode: GeneratorMode::Fast,
                generator_threads: Some(8),
                ..Default::default()
            },
            ..Default::default()
        };
        let o = Overrides::default();
        apply_overrides(&mut p, &o);
        // Profile.runtime НЕ затёрт (CLI не задал overrides).
        assert_eq!(p.runtime.generator_mode, GeneratorMode::Fast);
        assert_eq!(p.runtime.generator_threads, Some(8));
    }

    /// `apply_overrides` частично override'ит RuntimeConfig (per-field).
    #[test]
    fn a4_apply_overrides_partial_override() {
        let mut p = Profile {
            runtime: RuntimeConfig {
                generator_mode: GeneratorMode::Fast,
                generator_threads: None,
                ..Default::default()
            },
            ..Default::default()
        };
        let o = Overrides {
            runtime: RuntimeConfig {
                generator_threads: Some(16),
                ..Default::default()
            },
            runtime_overrides_set: RuntimeOverridesSet {
                generator_threads: true,
                ..Default::default()
            },
            ..Default::default()
        };
        apply_overrides(&mut p, &o);
        // generator_mode profile сохранён (CLI не override).
        assert_eq!(p.runtime.generator_mode, GeneratorMode::Fast);
        // generator_threads override'нут CLI.
        assert_eq!(p.runtime.generator_threads, Some(16));
    }
}

#[test]
fn apply_overrides_no_targets_keeps_existing() {
    let mut p = Profile {
        targets: vec![parse_target("9.9.9.9:514").unwrap()],
        ..Default::default()
    };
    let o = Overrides::default();
    apply_overrides(&mut p, &o);
    assert_eq!(p.targets.len(), 1);
    assert_eq!(p.targets[0].address, "9.9.9.9:514");
}

// === v10.6.0 (Usability ч.1): тесты subcommand'ов живут в `mod tests`
//     выше (top-level дубликаты удалены в PR-Q.1). ===
pub mod preset;
pub mod set_override;
