use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;
use std::process::ExitCode;
use syslog_generator::{
    apply_overrides, create_metrics, format_errors, load_profile_from_path, run_profile,
    validate_against_embedded_schema, validate_profile, Args, Profile,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(e) => {
            // anywho::Error::Display для цепочки; #[from] в RuntimeError/MetricsError/
            // ConfigError даёт внятные русскоязычные сообщения.
            eprintln!("ошибка: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let args = Args::parse();

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
        eprintln!("нечего запускать: укажите --profile <file> или --target и --message");
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
