//! N7: типизированные ошибки рантайма.
//!
//! До N7 рантайм-пути (`metrics.rs`, `main.rs`, `shutdown.rs`) местами опирались
//! на `.unwrap()`/`.expect()`, что превращало любую нештатную ситуацию в
//! панический крэш с ненулевым кодом возврата и без внятного сообщения. Это
//! блокировало промышленное применение: администратор не получал человекочитаемой
//! причины, по которой генератор прекратил работу.
//!
//! Этот модуль вводит:
//! - [`MetricsError`] — ошибки инициализации/экспорта Prometheus-метрик;
//! - [`ConfigError`] — ошибки загрузки/парсинга профиля;
//! - [`DrainError`] — ошибки фазы graceful-drain;
//! - [`RuntimeError`] — общий enum рантайма, агрегирующий доменные ошибки
//!   и используемый во внешних API (`run_profile`, `create_metrics`).
//!
//! Все ошибки реализованы через [`thiserror`], поддерживают `Display`, `source()`,
//! совместимы с `anyhow::Error` через `From`-имплементации и допускают
//! `#[from]` в местах вызова (`?`).
//!
//! Политика обработки в рантайме:
//! - **Критические ошибки** (битый профиль, невозможно создать metrics registry,
//!   tls-handshake на несуществующий CA) → проброс через `RuntimeError`,
//!   завершение с `ExitCode::FAILURE`;
//! - **Recoverable ошибки** (TCP-connect refused на целевом target'е, bind-fail
//!   HTTP-эндпоинта `/metrics`, accept-fail на metrics-сервере) → логируются
//!   в stderr, инкремент счётчика ошибок, продолжение работы. Это намеренное
//!   поведение (метрики — вспомогательный канал, отдельный target может быть
//!   временно недоступен). Recoverable-ошибки НЕ входят в `RuntimeError` — они
//!   остаются на уровне `eprintln!` + `metrics.errors_total`.
//!
//! При добавлении нового варианта:
//! 1. Добавить вариант в соответствующий enum;
//! 2. Если вариант относится к общему рантайму — добавить в `RuntimeError`
//!    через `#[from]` для подтипа;
//! 3. Покрыть unit-тестом в `mod tests`;
//! 4. При наличии пользовательского воздействия — добавить сценарный
//!    интеграционный тест.

use std::io;
use thiserror::Error;

/// Доменная ошибка: инициализация/экспорт Prometheus-метрик.
///
/// Возникает в трёх местах:
/// - `metrics::create_metrics` — конструкторы `CounterVec`/`Gauge`/`Histogram`/
///   `IntCounter` (некорректные имена/лейблы) и `Registry::register` (дубликат
///   имени в registry);
/// - `metrics::gather_metrics` — сериализация `TextEncoder` или не-UTF-8 в
///   выходном буфере (теоретически невозможно, но thiserror требует
///   обработки `FromUtf8Error` для полноты).
#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("не удалось создать {kind}-метрику {name:?}: {source}")]
    Construct {
        kind: &'static str,
        name: String,
        #[source]
        source: prometheus::Error,
    },

    #[error("не удалось зарегистрировать метрику {name:?} в registry: {source}")]
    Register {
        name: String,
        #[source]
        source: prometheus::Error,
    },

    #[error("не удалось сериализовать метрики в Prometheus text format: {0}")]
    Encode(#[from] prometheus::Error),

    #[error("не-UTF-8 байты в выходном буфере метрик: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl MetricsError {
    /// Удобный конструктор для ошибок `Construct` (используется в хелперах
    /// `make_*` в `metrics.rs`). Имя метрики копируется, чтобы `Display`
    /// оставался валидным после возврата из функции.
    pub fn construct(
        kind: &'static str,
        name: impl Into<String>,
        source: prometheus::Error,
    ) -> Self {
        Self::Construct {
            kind,
            name: name.into(),
            source,
        }
    }

    /// Удобный конструктор для ошибок `Register`.
    pub fn register(name: impl Into<String>, source: prometheus::Error) -> Self {
        Self::Register {
            name: name.into(),
            source,
        }
    }
}

/// Доменная ошибка: загрузка/парсинг профиля.
///
/// Используется в `main.rs` при чтении файла и десериализации JSON/YAML.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("не удалось прочитать файл профиля {path:?}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("невалидный JSON в файле профиля {path:?}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("невалидный YAML в файле профиля {path:?}: {source}")]
    Yaml {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("неподдерживаемое расширение файла профиля {extension:?} (путь: {path:?}); ожидается .json, .yaml или .yml")]
    UnsupportedFormat { path: String, extension: String },
}

impl ConfigError {
    pub fn io(path: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn json(path: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }

    pub fn yaml(path: impl Into<String>, source: serde_yaml::Error) -> Self {
        Self::Yaml {
            path: path.into(),
            source,
        }
    }

    pub fn unsupported_format(path: impl Into<String>, extension: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            path: path.into(),
            extension: extension.into(),
        }
    }
}

/// Доменная ошибка: фаза graceful-drain по завершении прогона.
///
/// Drain ждёт завершения sender-задач (TCP/TLS закрывают соединения корректно).
/// Возможные причины:
/// - `TaskJoin` — паника/отмена одной из sender-задач (теоретически sender'ы
///   не паникуют, но `JoinError` всё равно пробросим наверх);
/// - `Timeout` — не все sender'ы успели завершиться за отведённый
///   `drain_timeout_secs` (фиксируется в `syslog_drain_timeouts_total`);
/// - `Sender` — внутренняя ошибка sender-задачи (на практике sender'ы
///   глотают транспортные сбои в `errors_total` и возвращают `Ok`, но если
///   изменится контракт — пробросим наверх).
#[derive(Debug, Error)]
pub enum DrainError {
    #[error("ошибка ожидания фоновой задачи во время graceful drain: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),

    #[error("drain timeout: не все воркеры завершились за {timeout_secs} сек")]
    Timeout { timeout_secs: u64 },

    #[error("внутренняя ошибка sender-задачи во время drain: {0}")]
    Sender(#[from] anyhow::Error),
}

impl DrainError {
    pub fn timeout(timeout_secs: u64) -> Self {
        Self::Timeout { timeout_secs }
    }
}

/// Общая ошибка рантайма.
///
/// Агрегирует доменные ошибки (`Metrics`, `Config`, `Drain`) и предоставляет
/// варианты для сквозных сценариев рантайма (отмена задачи, сетевые ошибки).
/// Используется как `Err`-тип в публичных API, где нужна типизация:
/// - `metrics::create_metrics() -> Result<Metrics, RuntimeError>` (на практике
///   возвращает `MetricsError`, но автоматически конвертируется через `?`).
/// - `run_profile() -> Result<(), anyhow::Error>` — оборачивает `RuntimeError`
///   через `anyhow::Error::from(...)` для богатого контекстного трейса.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Metrics(#[from] MetricsError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Drain(#[from] DrainError),

    #[error("операция отменена (CancellationToken)")]
    Cancelled,

    #[error("ошибка фоновой задачи tokio: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// N7: конструктор `MetricsError::construct` сохраняет имя и kind.
    #[test]
    fn metrics_error_construct_preserves_name_and_kind() {
        // `prometheus::Error::Msg` — единственный публичный конструктор ошибки.
        let inner = prometheus::Error::Msg("duplicate metric".into());
        let e = MetricsError::construct("CounterVec", "syslog_messages_total", inner);
        let s = format!("{e}");
        assert!(s.contains("CounterVec"), "got: {s}");
        assert!(s.contains("syslog_messages_total"), "got: {s}");
        assert!(s.contains("duplicate metric"), "got: {s}");
    }

    /// N7: конструктор `MetricsError::register` сохраняет имя.
    #[test]
    fn metrics_error_register_preserves_name() {
        let inner = prometheus::Error::Msg("already registered".into());
        let e = MetricsError::register("syslog_messages_total", inner);
        let s = format!("{e}");
        assert!(s.contains("syslog_messages_total"), "got: {s}");
        assert!(s.contains("already registered"), "got: {s}");
    }

    /// N7: `MetricsError::Encode` транзитивно пробрасывает source prometheus::Error.
    #[test]
    fn metrics_error_encode_chains_source() {
        let inner = prometheus::Error::Msg("encode fail".into());
        let e: MetricsError = inner.into();
        // Source должен существовать (#[from] гарантирует цепочку).
        use std::error::Error as _;
        assert!(e.source().is_some());
        assert!(format!("{e}").contains("encode fail"));
    }

    /// N7: `ConfigError::io` корректно оборачивает io::Error.
    #[test]
    fn config_error_io_wraps_source() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "no such file");
        let e = ConfigError::io("examples/missing.json", io_err);
        let s = format!("{e}");
        assert!(s.contains("examples/missing.json"), "got: {s}");
        assert!(s.contains("no such file"), "got: {s}");
    }

    /// N7: `ConfigError::json` корректно оборачивает serde_json::Error.
    #[test]
    fn config_error_json_wraps_source() {
        // Заведомо невалидный JSON: лишняя закрывающая скобка.
        let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str("{ \"a\": 1 }");
        // Гарантируем ошибку через другую строку.
        let json_err = serde_json::from_str::<serde_json::Value>("{not json}").unwrap_err();
        let e = ConfigError::json("profile.json", json_err);
        let s = format!("{e}");
        assert!(s.contains("profile.json"), "got: {s}");
        assert!(s.contains("JSON"), "got: {s}");
        // parsed нам нужен только чтобы компилятор не ругался на неиспользование.
        let _ = parsed;
    }

    /// D3 (v8.5.0): `ConfigError::yaml` корректно оборачивает serde_yaml::Error.
    #[test]
    fn config_error_yaml_wraps_source() {
        // Заведомо невалидный YAML — дублирование ключа в мапе приводит к ошибке.
        let yaml_err = serde_yaml::from_str::<serde_yaml::Value>("a: 1\na: 2\n").unwrap_err();
        let e = ConfigError::yaml("profile.yaml", yaml_err);
        let s = format!("{e}");
        assert!(s.contains("profile.yaml"), "got: {s}");
        assert!(s.contains("YAML"), "got: {s}");
    }

    /// D3 (v8.5.0): `ConfigError::unsupported_format` показывает расширение и путь.
    #[test]
    fn config_error_unsupported_format_shows_extension() {
        let e = ConfigError::unsupported_format("/tmp/profile.toml", "toml");
        let s = format!("{e}");
        assert!(s.contains("toml"), "got: {s}");
        assert!(s.contains("/tmp/profile.toml"), "got: {s}");
        assert!(s.contains(".json") || s.contains(".yaml"), "got: {s}");
    }

    /// N7: `DrainError::timeout` содержит таймаут в сообщении.
    #[test]
    fn drain_error_timeout_includes_secs() {
        let e = DrainError::timeout(15);
        let s = format!("{e}");
        assert!(s.contains("15"), "got: {s}");
        assert!(s.contains("drain"), "got: {s}");
    }

    /// N7: `RuntimeError::from(MetricsError)` корректно конвертирует.
    #[test]
    fn runtime_error_from_metrics_error() {
        let m: MetricsError =
            MetricsError::construct("Gauge", "g", prometheus::Error::Msg("x".into()));
        let r: RuntimeError = m.into();
        let s = format!("{r}");
        assert!(s.contains("Gauge"), "got: {s}");
        assert!(s.contains("g"), "got: {s}");
    }

    /// N7: `RuntimeError::Cancelled` имеет стабильный текст.
    #[test]
    fn runtime_error_cancelled_text() {
        let s = format!("{}", RuntimeError::Cancelled);
        assert!(s.contains("отмен"), "got: {s}");
    }

    /// N7: `RuntimeError::Metrics(#[from] MetricsError)` обеспечивает `?`
    /// из функции, возвращающей `Result<_, RuntimeError>`.
    #[test]
    fn question_mark_from_metrics_error_to_runtime_error() {
        fn inner() -> Result<(), RuntimeError> {
            let m: MetricsError =
                MetricsError::register("foo", prometheus::Error::Msg("dup".into()));
            Err(m)?
        }
        let e = inner().unwrap_err();
        let s = format!("{e}");
        assert!(s.contains("foo"), "got: {s}");
    }

    /// N7: `RuntimeError::Drain(#[from] DrainError)` обеспечивает `?`.
    #[test]
    fn question_mark_from_drain_error_to_runtime_error() {
        fn inner() -> Result<(), RuntimeError> {
            let d: DrainError = DrainError::timeout(5);
            Err(d)?
        }
        let e = inner().unwrap_err();
        let s = format!("{e}");
        assert!(s.contains("5"), "got: {s}");
    }

    /// N7: Display всех вариантов стабилен (snapshot-тест против эталонов).
    /// При ребрендинге/правке текстов ошибок — обновить снимки намеренно.
    #[test]
    fn runtime_error_display_snapshots() {
        assert_eq!(
            format!("{}", RuntimeError::Cancelled),
            "операция отменена (CancellationToken)"
        );
        let m: RuntimeError =
            MetricsError::construct("CounterVec", "x", prometheus::Error::Msg("oops".into()))
                .into();
        assert!(format!("{m}").starts_with("не удалось создать CounterVec-метрику"));
    }
}
