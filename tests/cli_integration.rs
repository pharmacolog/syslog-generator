//! CLI integration tests через assert_cmd.
//!
//! Тестирует subcommands и flags через реальный бинарник (build + execute).
//! Цель — покрыть entry points (`src/main.rs`), которые не покрываются
//! unit-тестами в `src/cli.rs`:
//!
//! - `--dry-run` flag (cycles через `run()` без реальной отправки)
//! - `--print-config` (выводит JSON-профиль в stdout)
//! - `--schema-strict` (валидация через JSON Schema)
//! - `completions <shell>` subcommand (bash/zsh/fish/powershell/elvish)
//! - `man` subcommand (roff-генерация)
//! - `--version` flag
//! - `--help` flag
//! - invalid args → error exit code
//! - missing required args → error
//!
//! Отдельный test-файл (не `tests/integration_tests.rs`) потому что тот файл
//! уже 3233 строк и перегружен TLS/socket setup. Разделение позволяет
//! cargo test запускать CLI-тесты параллельно без блокировки на сокетах.
//!
//! PR-Q.2 (Phase 7a): добавлено 11 тестов.

use assert_cmd::Command;
use predicates::boolean::PredicateBooleanExt;

/// Build `Command` для бинарника `syslog-generator` через assert_cmd.
/// `cargo_bin` сам пересобирает бинарь при необходимости.
fn bin() -> Command {
    Command::cargo_bin("syslog-generator").expect("binary should build")
}

/// `--version` печатает версию пакета (Cargo.toml `version`).
#[test]
fn cli_version_flag_prints_version() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
}

/// `--help` печатает usage и упоминает ключевые флаги.
#[test]
fn cli_help_flag_prints_help() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Usage:"))
        .stdout(predicates::str::contains("--target"))
        .stdout(predicates::str::contains("--dry-run"));
}

/// Без `--target`/`--profile`/`--message` (и без фазы) — нет данных для запуска,
/// main возвращает `ExitCode::FAILURE` (1) и пишет подсказку в stderr.
#[test]
fn cli_missing_required_target_errors() {
    bin().arg("--rate").arg("100").assert().failure().code(1);
}

/// `--dry-run` с inline target: загружает, валидирует, печатает phases/targets,
/// завершается успешно (rc=0) без реальной отправки.
#[test]
fn cli_dry_run_with_inline_target_succeeds() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:9999")
        .arg("--message")
        .arg("hello {{sequence}}")
        .arg("--rate")
        .arg("100")
        .arg("--duration")
        .arg("1")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicates::str::contains("DRY-RUN"))
        .stdout(predicates::str::contains("Targets"));
}

/// `--print-config` печатает итоговый профиль как JSON и завершается успешно.
#[test]
fn cli_print_config_prints_json() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:9999")
        .arg("--message")
        .arg("hi")
        .arg("--rate")
        .arg("50")
        .arg("--duration")
        .arg("10")
        .arg("--print-config")
        .assert()
        .success()
        .stdout(predicates::str::contains("\"targets\""))
        .stdout(predicates::str::contains("\"phases\""));
}

/// `completions bash` — генерирует bash completion script (содержит `complete`).
#[test]
fn cli_completions_bash_outputs_completion_script() {
    bin()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("complete"));
}

/// `completions zsh` — генерирует zsh completion script.
#[test]
fn cli_completions_zsh_outputs_completion_script() {
    bin()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicates::str::contains("#compdef").or(predicates::str::contains("compdef")));
}

/// `completions fish` — генерирует fish completion script.
#[test]
fn cli_completions_fish_outputs_completion_script() {
    bin()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicates::str::contains("complete"));
}

/// `completions powershell` — генерирует powershell completion script.
#[test]
fn cli_completions_powershell_outputs_completion_script() {
    bin()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("Register-ArgumentCompleter")
                .or(predicates::str::contains("using namespace")),
        );
}

/// `completions elvish` — генерирует elvish completion script.
#[test]
fn cli_completions_elvish_outputs_completion_script() {
    bin().args(["completions", "elvish"]).assert().success();
}

/// `man` subcommand — генерирует man page (roff format, начинается с `.TH`).
#[test]
fn cli_man_subcommand_outputs_manpage() {
    bin()
        .args(["man"])
        .assert()
        .success()
        .stdout(predicates::str::contains(".TH"));
}

/// `--schema-strict` с валидным inline-профилем проходит формальную JSON Schema
/// валидацию (semantic + structural).
#[test]
fn cli_schema_strict_accepts_valid_profile() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:9999")
        .arg("--message")
        .arg("ok")
        .arg("--rate")
        .arg("10")
        .arg("--duration")
        .arg("1")
        .arg("--schema-strict")
        .arg("--validate")
        .assert()
        .success();
}

/// `--transport` с недопустимым значением (clap value_parser) → clap error,
/// exit code != 0.
#[test]
fn cli_invalid_transport_value_errors() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:514")
        .arg("--transport")
        .arg("ipx") // не входит в ["tcp", "udp", "tls", "file"]
        .assert()
        .failure();
}

/// `--rate` принимает только u64 — невалидное значение → clap error.
#[test]
fn cli_invalid_rate_value_errors() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:514")
        .arg("--rate")
        .arg("not-a-number")
        .assert()
        .failure();
}

/// `--validate` на inline-профиле без валидного phase контента: либо успех
/// (если всё заполнено), либо exit != 0 (если есть validation errors).
/// Проверяем просто что процесс завершается с каким-то определённым rc.
#[test]
fn cli_validate_completes() {
    bin()
        .arg("--target")
        .arg("127.0.0.1:514")
        .arg("--message")
        .arg("test {{sequence}}")
        .arg("--rate")
        .arg("10")
        .arg("--duration")
        .arg("1")
        .arg("--validate")
        .assert()
        .success();
}

/// PR-Q.2 (Phase 7b): SIGTERM через libc::kill запускает graceful shutdown.
///
/// Запускаем бинарь с долгим профилем (--duration=30), посылаем SIGTERM
/// через libc::kill и проверяем что бинарь завершается с rc=0 (graceful)
/// в течение 10 секунд. Покрывает unit-тест branch'и в `shutdown.rs`:
/// `shutdown_listener` → tokio::select! → SIGTERM recv() → handle_signal.
#[cfg(unix)]
#[test]
fn sigterm_signal_triggers_graceful_shutdown() {
    use std::process::Stdio;
    use std::time::Duration;

    let bin_path = env!("CARGO_BIN_EXE_syslog-generator");
    let mut child = std::process::Command::new(bin_path)
        .args([
            "--target",
            "127.0.0.1:9999",
            "--message",
            "x",
            "--rate",
            "1",
            "--duration",
            "30", // долгий, чтобы успеть послать SIGTERM
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn syslog-generator");

    // Даём процессу стартовать и зарегистрировать SIGTERM handler.
    std::thread::sleep(Duration::from_millis(500));

    // Посылаем SIGTERM.
    let pid = child.id() as i32;
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(rc, 0, "libc::kill должен вернуть 0");

    // Polling wait: graceful shutdown обычно < 5 сек, даём 10.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "graceful shutdown → rc=0, got rc={:?}",
                    status.code()
                );
                return;
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("timeout: syslog-generator не завершился за 10 сек после SIGTERM");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("try_wait failed: {e}"),
        }
    }
}
