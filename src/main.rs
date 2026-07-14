use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;
use std::process::ExitCode;
use syslog_generator::{
    apply_overrides, create_metrics, format_errors, load_profile_from_path, run_profile,
    validate_against_embedded_schema, validate_profile, Args, Command, Profile,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(e) => {
            // anywho::Error::Display для цепочки; #[from] в RuntimeError/MetricsError/
            // ConfigError даёт внятные русскоязычные сообщения.
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
        Some(path) => load_profile_from_path(Path::new(path))
            .with_context(|| format!("загрузка профиля {path:?}"))?,
        None => Profile::default(),
    };

    // 2. Применяем CLI-оверрайды.
    let overrides = args.to_overrides().context("разбор --target")?;
    apply_overrides(&mut profile, &overrides);

    // Если ни профиль, ни оверрайды ничего не задали — показываем подсказку.
    if profile.phases.is_empty() && profile.targets.is_empty() {
        use owo_colors::OwoColorize;
        eprintln!(
            "{}",
            "нечего запускать: укажите --profile <file> или --target и --message".yellow()
        );
        eprintln!("подробнее: syslog-generator --help");
        return Ok(ExitCode::FAILURE);
    }

    // 3. D3: структурная проверка через формальную JSON Schema.
    // Выполняется ПЕРЕД семантической валидацией, чтобы отловить
    // структурные ошибки (неправильные типы, неизвестные ключи, значения
    // вне диапазонов) с более точными сообщениями от jsonschema.
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
        println!(
            "профиль валиден: {} фаз(ы), {} цел(ей)",
            profile.phases.len(),
            profile.targets.len()
        );
        return Ok(ExitCode::SUCCESS);
    }

    // 5. --print-config: вывести итоговый профиль и выйти.
    if args.print_config {
        println!("{}", serde_json::to_string_pretty(&profile)?);
        return Ok(ExitCode::SUCCESS);
    }

    // 6. Запуск.
    // N7: create_metrics() теперь возвращает Result<Metrics, MetricsError>.
    // Раньше здесь был `.expect()` — при ошибке инициализации registry процесс
    // падал с паникой. Теперь ошибка всплывает через `?` как `anyhow::Error`
    // (благодаря `From<MetricsError>` для anyhow::Error) и приводит к
    // ExitCode::FAILURE с внятным сообщением.
    let metrics = create_metrics().context("инициализация Prometheus-метрик")?;
    run_profile(&profile, metrics)
        .await
        .context("выполнение профиля")?;
    Ok(ExitCode::SUCCESS)
}

/// v10.6.0: handler для subcommand'ов (completions, man).
///
/// ВАЖНО: в текущей реализации subcommand'ы компилируются без feature-gate,
/// но runtime их функционал доступен ТОЛЬКО когда binary скомпилировано
/// без `--features kafka` (по умолчанию). При --features kafka используется
/// rskafka, и binary становится больше. На completions/man это не влияет
/// (subcommand'ы доступны всегда).
fn handle_command(cmd: &Command) -> Result<ExitCode> {
    use clap::CommandFactory;
    match cmd {
        Command::Completions { shell } => {
            // v10.6.0: использует clap_complete::generate для генерации
            // shell-специфичного completion script в stdout.
            // Использование:
            //   syslog-generator completions bash > /etc/bash_completion.d/syslog-generator
            //   syslog-generator completions zsh > "${fpath[1]}/_syslog-generator"
            //   syslog-generator completions fish > ~/.config/fish/completions/syslog-generator.fish
            //   syslog-generator completions powershell > syslog-generator.ps1
            //   syslog-generator completions elvish > ~/.elvish/lib/syslog-generator.elv
            let mut app = Args::command();
            let bin_name = app.get_name().to_string();
            clap_complete::generate(*shell, &mut app, bin_name, &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        Command::Man => {
            // v10.6.0: использует clap_mangen для генерации man page в stdout
            // (roff format). Использование:
            //   syslog-generator man > /usr/local/share/man/man1/syslog-generator.1
            //   man -l <(syslog-generator man)  # просмотр сразу
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
