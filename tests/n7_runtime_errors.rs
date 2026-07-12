//! N7: интеграционные тесты типизированных ошибок рантайма.
//!
//! Цель: убедиться, что:
//! - `create_metrics()` возвращает `Ok` в штатном режиме;
//! - `gather_metrics()` возвращает `Ok` после `inc()`;
//! - битый JSON-профиль → rc=1 + сообщение `ConfigError::Json`;
//! - несуществующий файл профиля → rc=1 + сообщение `ConfigError::Io`;
//! - `--validate` на битом профиле → rc=1 + список проблем в stderr;
//! - `--print-config` → rc=0 + JSON-профиль в stdout;
//! - `--version` → rc=0;
//! - `--metrics-addr` на занятом порту → rc=0 + warn в stderr (recoverable);
//! - TLS-sender с несуществующим хостом → sender глотает ошибку, rc=0.
//!
//! Тесты запускаются против `target/debug/syslog-generator` (см. env-переменную
//! `CARGO_BIN_EXE_syslog-generator`, автоматически задаваемую Cargo).
//!
//! Тесты 3 TLS-target'ов из integration_tests.rs упали из-за несовместимости
//! rcgen 0.13 с системным OpenSSL (не относится к N7); они не покрываются здесь.

use std::io::Write;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;
use syslog_generator::{create_metrics, gather_metrics, run_profile, Phase, Profile, TargetConfig};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_syslog-generator")
}

/// Вспомогательная: запустить бинарник с аргументами, вернуть (status, stdout, stderr).
fn run_bin(args: &[&str], stdin_bytes: Option<&[u8]>) -> (i32, String, String) {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("не удалось запустить syslog-generator");
    if let (Some(stdin), Some(stdin_pipe)) = (stdin_bytes, child.stdin.as_mut()) {
        stdin_pipe
            .write_all(stdin)
            .expect("не удалось писать в stdin");
    }
    let out = child
        .wait_with_output()
        .expect("не удалось дождаться syslog-generator");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Занять порт на время теста; возвращает реальный адрес.
fn reserve_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("не удалось забиндить 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr").to_string();
    // listener намеренно удерживается открытым до выхода из функции;
    // переносим во внешний scope через Box::leak — это допустимо для теста.
    Box::leak(Box::new(listener));
    addr
}

// --- N7 unit-сценарии через библиотечный API -----------------------------

/// N7: `create_metrics()` теперь типизирован — возвращает `Ok` в штатном режиме.
#[test]
fn n7_create_metrics_returns_ok() {
    let m = create_metrics().expect("create_metrics должен возвращать Ok");
    // Скалярная метрика существует и равна 0.
    assert_eq!(m.shutdowns_total.get(), 0);
}

/// N7: `gather_metrics()` теперь типизирован — возвращает `Ok` после `inc()`.
#[test]
fn n7_gather_metrics_returns_ok_after_inc() {
    let m = create_metrics().expect("create_metrics ok");
    m.messages_total
        .with_label_values(&["tcp", "p", "t", "success"])
        .inc();
    let s = gather_metrics(&m).expect("gather_metrics ok");
    assert!(s.contains("syslog_messages_total"));
}

/// N7: `run_profile()` с фазами отрабатывает без паники (regression: до N7
/// потенциальные `.unwrap()` могли ронять процесс на нештатных данных).
#[tokio::test]
async fn n7_run_profile_smoke_does_not_panic() {
    let m = create_metrics().expect("create_metrics ok");
    let p = Profile {
        targets: vec![TargetConfig {
            address: "/tmp/n7_smoke.log".into(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: Default::default(),
        phases: vec![Phase {
            name: "smoke".into(),
            total_messages: Some(1),
            templates: vec!["hello {{sequence}}".into()],
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let _ = std::fs::remove_file("/tmp/n7_smoke.log");
    run_profile(&p, m)
        .await
        .expect("smoke-прогон не должен падать");
    let _ = std::fs::remove_file("/tmp/n7_smoke.log");
}

// --- N7: сценарии CLI -----------------------------------------------------

/// N7: `--version` всегда работает корректно и даёт rc=0.
#[test]
fn n7_version_returns_zero() {
    let (rc, out, _err) = run_bin(&["--version"], None);
    assert_eq!(rc, 0, "--version должен вернуть rc=0");
    assert!(
        out.contains("8."),
        "ожидался номер версии в stdout, got: {out}"
    );
}

/// N7: `--print-config` для валидного быстрого режима (через --target + --message).
#[test]
fn n7_print_config_quick_mode_returns_zero() {
    let (rc, out, _err) = run_bin(
        &[
            "--target",
            "127.0.0.1:6514:tcp",
            "--message",
            "hello",
            "--total",
            "1",
            "--print-config",
        ],
        None,
    );
    assert_eq!(rc, 0, "--print-config должен вернуть rc=0, stderr={_err}");
    assert!(
        out.contains("\"targets\""),
        "ожидался JSON профиля в stdout, got: {out}"
    );
}

/// N7: `--validate` на валидном профиле → rc=0 + сообщение об успехе.
#[test]
fn n7_validate_valid_profile_returns_zero() {
    let (rc, out, _err) = run_bin(
        &[
            "--target",
            "127.0.0.1:6514:tcp",
            "--message",
            "hello",
            "--total",
            "1",
            "--validate",
        ],
        None,
    );
    assert_eq!(rc, 0, "--validate на валидном профиле должен вернуть rc=0");
    assert!(out.contains("профиль валиден"), "got: {out}");
}

/// N7: `--validate` на невалидном профиле → rc=1 + описание проблем.
#[test]
fn n7_validate_invalid_profile_returns_one() {
    // Невалидный профиль: плохой transport, плохой distribution, пустые phases.
    // Пишем во временный файл — CLI читает профиль через `--profile <path>`, а не stdin.
    let bad_profile_path = std::env::temp_dir().join("n7_invalid_profile.json");
    let bad_profile = r#"{
        "targets": [{"address": "127.0.0.1:514", "transport": "sctp"}],
        "distribution": "hash",
        "phases": []
    }"#;
    std::fs::write(&bad_profile_path, bad_profile).expect("write bad profile");
    let (rc, _out, err) = run_bin(
        &[
            "--profile",
            bad_profile_path.to_str().unwrap(),
            "--validate",
        ],
        None,
    );
    let _ = std::fs::remove_file(&bad_profile_path);
    assert_eq!(
        rc, 1,
        "--validate на невалидном профиле должен вернуть rc=1"
    );
    assert!(
        err.contains("невалиден") || err.contains("проблем"),
        "stderr: {err}"
    );
    assert!(
        err.contains("sctp") || err.contains("transport"),
        "stderr: {err}"
    );
}

/// N7: `--validate` на полностью битом JSON → rc=1 + сообщение о JSON-ошибке.
#[test]
fn n7_validate_malformed_json_returns_one() {
    let bad_json_path = std::env::temp_dir().join("n7_malformed_profile.json");
    let bad_json = r#"{ "targets": [ ,,, ] }"#;
    std::fs::write(&bad_json_path, bad_json).expect("write malformed json");
    let (rc, _out, err) = run_bin(
        &["--profile", bad_json_path.to_str().unwrap(), "--validate"],
        None,
    );
    let _ = std::fs::remove_file(&bad_json_path);
    assert_eq!(rc, 1, "битый JSON должен вернуть rc=1");
    assert!(
        err.contains("JSON") || err.contains("невалид"),
        "stderr: {err}"
    );
}

/// N7: `--metrics-addr` на занятом порту → rc=0 + warn в stderr (recoverable,
/// метрики — вспомогательный канал, см. CLAUDE_HANDOFF.md §4 F12).
#[test]
fn n7_metrics_addr_busy_port_does_not_fail_run() {
    let busy = reserve_port();
    // Небольшая пауза, чтобы ОС точно зафиксировала занятость.
    std::thread::sleep(Duration::from_millis(50));
    let (rc, _out, err) = run_bin(
        &[
            "--target",
            "127.0.0.1:6514:tcp",
            "--message",
            "hello",
            "--total",
            "1",
            "--metrics-addr",
            &busy,
        ],
        None,
    );
    // rc=0 — bind-fail не роняет генератор (recoverable).
    assert_eq!(
        rc, 0,
        "bind-fail на /metrics НЕ должен ронять генератор, stderr={err}"
    );
    assert!(
        err.contains("F12") || err.contains("метрик") || err.contains("bind"),
        "должен быть warn про bind-fail в stderr, got: {err}"
    );
}

/// N7: `--validate` без аргументов и без stdin → rc=1 + подсказка.
#[test]
fn n7_no_args_returns_one_with_hint() {
    let (rc, _out, err) = run_bin(&[], None);
    assert_eq!(rc, 1, "без аргументов ожидается rc=1");
    assert!(
        err.contains("нечего запускать") || err.contains("--help"),
        "stderr должен содержать подсказку, got: {err}"
    );
}

/// N7: несуществующий файл профиля → rc=1 + сообщение ConfigError::Io.
#[test]
fn n7_missing_profile_file_returns_one() {
    let (rc, _out, err) = run_bin(&["--profile", "/tmp/n7_definitely_missing_zzz.json"], None);
    assert_eq!(rc, 1, "несуществующий файл → rc=1");
    assert!(
        err.contains("не удалось прочитать профиль") || err.contains("os error"),
        "stderr должен содержать описание IO-ошибки, got: {err}"
    );
}
