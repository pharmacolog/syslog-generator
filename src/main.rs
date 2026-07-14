use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;
use std::process::ExitCode;
use syslog_generator::{
    apply_overrides, create_metrics, format_errors, load_profile_from_path, run_profile,
    validate_against_embedded_schema, validate_profile, Args, Command, Profile,
};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> ExitCode {
    // v10.7.0: инициализация structured logging через tracing-subscriber.
    // RUST_LOG env var управляет уровнем (default: info).
    // Например: RUST_LOG=debug syslog-generator -p profile.json
    //          RUST_LOG=syslog_generator=trace,syslog_generator::transport=debug
    // tracing-subscriber::EnvFilter::from_default_env() читает RUST_LOG.
    // На Windows tracing-subscriber игнорирует RUST_LOG (использует свой формат),
    // но на macOS/Linux (где это критично) работает.
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .try_init();

    match run().await {
        Ok(code) => code,
        Err(e) => {
            // anywho::Error::Display для цепочки; #[from] в RuntimeError/MetricsError/
            // ConfigError даёт внятные русскоязычные сообщения.
            // v10.7.0: используем eprintln! для ошибок (НЕ tracing::error!)
            // — tracing буферизирует вывод и не flushed до exit, что ломает
            // N7 тесты которые проверяют содержимое stderr.
            use owo_colors::OwoColorize;
            eprintln!("{}", format!("ошибка: {e:#}").red().bold());
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let args = Args::parse();

    // v10.6.0: dispatch subcommand'ы (completions, man) до основной логики.
    if let Some(cmd) = &args.command {
        return handle_command(cmd);
    }

    // 1. Загружаем профиль из файла или начинаем с пустого (быстрый CLI-режим).
    // D3 (v8.5.0): формат определяется по расширению — .json/.yaml/.yml.
    // Для обратной совместимости путь без расширения или с неизвестным
    // расширением отвергается явной ошибкой `ConfigError::UnsupportedFormat`.
    let mut profile: Profile = match &args.profile {
        Some(path) => {
            info!("загрузка профиля: {path:?}");
            load_profile_from_path(Path::new(path))
                .with_context(|| format!("загрузка профиля {path:?}"))?
        }
        None => Profile::default(),
    };

    // 2. Применяем CLI-оверрайды.
    let overrides = args.to_overrides().context("разбор --target")?;
    apply_overrides(&mut profile, &overrides);

    // Если ни профиль, ни оверрайды ничего не задали — показываем подсказку.
    if profile.phases.is_empty() && profile.targets.is_empty() {
        use owo_colors::OwoColorize;
        let msg = "нечего запускать: укажите --profile <file> или --target и --message"
            .yellow()
            .to_string();
        warn!("{}", msg);
        eprintln!("подробнее: syslog-generator --help");
        return Ok(ExitCode::FAILURE);
    }

    // 3. D3: структурная проверка через формальную JSON Schema.
    if args.schema_strict {
        validate_against_embedded_schema(&profile)
            .map_err(|e| anyhow::anyhow!("ошибка структурной валидации (schema-strict): {e}"))?;
    }

    // 4. F13: семантическая валидация. При --validate — только проверка и выход.
    let errors = validate_profile(&profile);
    if !errors.is_empty() {
        eprint!("{}", format_errors(&errors));
        return Ok(ExitCode::FAILURE);
    }
    if args.validate {
        info!(
            "профиль валиден: {} фаз(ы), {} цел(ей)",
            profile.phases.len(),
            profile.targets.len()
        );
        return Ok(ExitCode::SUCCESS);
    }

    // v10.7.0: --dry-run — загрузить, валидировать, но НЕ запускать.
    // Полезно для CI/CD pipeline: проверка профиля без реальной нагрузки.
    if args.dry_run {
        use owo_colors::OwoColorize;
        let phases: Vec<String> = profile
            .phases
            .iter()
            .map(|p| {
                format!(
                    "  фаза {:?}: {} шаблон(ов), rate={}, duration={}с, total={:?}",
                    p.name,
                    p.templates.len(),
                    p.messages_per_second,
                    p.duration_secs,
                    p.total_messages
                )
            })
            .collect();
        let targets: Vec<String> = profile
            .targets
            .iter()
            .map(|t| {
                format!(
                    "  target {}://{} (framing={:?})",
                    t.transport, t.address, t.framing
                )
            })
            .collect();
        info!(
            "{}",
            "DRY-RUN: профиль валиден, нагрузка НЕ отправляется"
                .green()
                .to_string()
        );
        println!("\nPhases ({}):", profile.phases.len());
        for p in &phases {
            println!("{p}");
        }
        println!("\nTargets ({}):", profile.targets.len());
        for t in &targets {
            println!("{t}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    // 5. --print-config: вывести итоговый профиль и выйти.
    if args.print_config {
        println!("{}", serde_json::to_string_pretty(&profile)?);
        return Ok(ExitCode::SUCCESS);
    }

    // 6. Запуск.
    let metrics = create_metrics().context("инициализация Prometheus-метрик")?;
    run_profile(&profile, metrics)
        .await
        .context("выполнение профиля")?;
    Ok(ExitCode::SUCCESS)
}

/// v10.6.0: handler для subcommand'ов (completions, man).
fn handle_command(cmd: &Command) -> Result<ExitCode> {
    use clap::CommandFactory;
    match cmd {
        Command::Completions { shell } => {
            info!("генерация completions для {:?}", shell);
            let mut app = Args::command();
            let bin_name = app.get_name().to_string();
            clap_complete::generate(*shell, &mut app, bin_name, &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        Command::Man => {
            info!("генерация man page");
            let app = Args::command();
            let man = clap_mangen::Man::new(app);
            let mut buffer: Vec<u8> = Vec::new();
            man.render(&mut buffer)
                .map_err(|e| anyhow::anyhow!("ошибка генерации man page: {e}"))?;
            use std::io::Write;
            std::io::stdout().write_all(&buffer)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
