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

use crate::config::{Phase, Profile, TargetConfig};
use clap::Parser;

#[derive(Parser, Debug, Default)]
#[command(
    name = "syslog-generator",
    version,
    about = "Промышленный генератор нагрузки на syslog (multi-target, профили нагрузки, вариативный пейлоад)",
    long_about = None
)]
pub struct Args {
    /// Путь к JSON-профилю нагрузки. Без него профиль собирается из CLI-флагов
    /// (нужен хотя бы один --target и источник контента, напр. --message).
    #[arg(short, long)]
    pub profile: Option<String>,

    /// Цель в форме ADDR или ADDR:TRANSPORT (transport: tcp|udp|tls|file;
    /// по умолчанию tcp). Флаг повторяемый; заменяет targets из профиля.
    /// Примеры: -t 127.0.0.1:514, -t 10.0.0.1:6514:tls, -t /tmp/out.log:file
    #[arg(short = 't', long = "target")]
    pub target: Vec<String>,

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

    /// Вывести итоговый профиль (после оверрайдов) как JSON и выйти.
    #[arg(long)]
    pub print_config: bool,

    /// F12: адрес HTTP-эндпоинта /metrics (напр. 127.0.0.1:9090).
    /// Переопределяет metrics_addr из профиля.
    #[arg(long)]
    pub metrics_addr: Option<String>,
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
}

/// Ошибка разбора спецификации цели `--target`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TargetParseError {
    #[error("пустая спецификация --target")]
    Empty,
    #[error("--target {0:?}: слишком много компонентов (ожидается ADDR или ADDR:TRANSPORT)")]
    TooManyParts(String),
}

/// Разбирает `ADDR` или `ADDR:TRANSPORT` в [`TargetConfig`].
///
/// Для `host:port` (две части, где вторая — числовой порт) весь ввод трактуется
/// как адрес с транспортом по умолчанию tcp. Если последняя часть — известный
/// транспорт (tcp/udp/tls/file), она отделяется. Для `file` адрес — это путь.
pub fn parse_target(spec: &str) -> Result<TargetConfig, TargetParseError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(TargetParseError::Empty);
    }
    let known = ["tcp", "udp", "tls", "file"];

    // Отделяем транспорт, только если последний сегмент после ':' — известный транспорт.
    if let Some((addr, last)) = spec.rsplit_once(':') {
        if known.contains(&last) {
            if addr.is_empty() {
                return Err(TargetParseError::Empty);
            }
            return Ok(TargetConfig {
                address: addr.to_string(),
                transport: last.to_string(),
                ..Default::default()
            });
        }
    }

    // Иначе весь ввод — адрес (host:port или путь), транспорт по умолчанию tcp.
    Ok(TargetConfig {
        address: spec.to_string(),
        transport: "tcp".to_string(),
        ..Default::default()
    })
}

impl Args {
    /// Преобразует разобранные аргументы в [`Overrides`], разбирая `--target`.
    pub fn to_overrides(&self) -> Result<Overrides, TargetParseError> {
        let mut targets = Vec::new();
        for t in &self.target {
            targets.push(parse_target(t)?);
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
    fn parse_target_with_transport() {
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
                Phase { name: "a".into(), ..Default::default() },
                Phase { name: "b".into(), ..Default::default() },
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
        assert_eq!(p.phases[0].templates, vec!["hello {{sequence}}".to_string()]);
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
}
