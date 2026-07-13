//! N10 (v8.8.0): слой транспортов (file, tcp, udp, tls).
//!
//! Это абстракция над способами доставки syslog-сообщений. Каждый
//! транспорт реализует свою функцию `target_sender_*`. До N10 всё было
//! в одном `src/sender.rs` (~554 строк). После рефакторинга:
//!
//! - `mod.rs` — общая инфраструктура: `SharedRx` (Arc<Mutex<Receiver<Vec<u8>>>>),
//!   `Framing` (RFC 6587), `record_send`/`record_send_latency`/`record_reconnect`/
//!   `record_error`/`drain_as_errors`/`next_msg` (приватные).
//! - `file` — `target_sender_file` (BufWriter, N6 zero-copy).
//! - `tcp` — `target_sender_tcp` + `reconnect_tcp` (BytesMut, N6 zero-copy).
//! - `udp` — `target_sender_udp` (zero-copy по дизайну).
//! - `tls` — `target_sender_tls` + `tls_connect` + `TlsParams` +
//!   `build_tls_connector` + `parse_tls_min_version` (N4.mTLS).
//!
//! Старый `src/sender.rs` сохранён как thin re-export для backward-compat.

use crate::metrics::Metrics;
use bytes::BytesMut;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

/// Общий приёмник очереди target'а, из которого читают несколько воркеров пула.
pub type SharedRx = Arc<Mutex<mpsc::Receiver<Vec<u8>>>>;

pub async fn record_send(
    metrics: &Metrics,
    transport: &str,
    phase: &str,
    target: &str,
    bytes: u64,
    shutdown: &CancellationToken,
) {
    metrics
        .messages_by_sink
        .with_label_values(&[transport])
        .inc();
    metrics
        .messages_total
        .with_label_values(&[transport, phase, target, "success"])
        .inc();
    metrics
        .bytes_total
        .with_label_values(&[transport, phase, target])
        .inc_by(bytes as f64);
    metrics.message_size_bytes.observe(bytes as f64);
    if shutdown.is_cancelled() {
        metrics
            .messages_drained_total
            .with_label_values(&[target])
            .inc();
    }
}

/// Зафиксировать латентность отправки одного сообщения (в секундах).
pub(crate) fn record_send_latency(metrics: &Metrics, elapsed: Duration) {
    metrics.send_duration.observe(elapsed.as_secs_f64());
}

/// Отметить попытку переустановки соединения.
pub(crate) fn record_reconnect(metrics: &Metrics, transport: &str, target: &str) {
    metrics
        .reconnects_total
        .with_label_values(&[transport, target])
        .inc();
}

pub async fn record_error(metrics: &Metrics, target: &str) {
    metrics.errors_total.with_label_values(&[target]).inc();
}

/// Взять следующее сообщение из общей очереди пула.
/// Блокировка Mutex удерживается только на время `recv`, поэтому воркеры
/// разбирают сообщения конкурентно (каждое сообщение достаётся ровно одному воркеру).
pub(crate) async fn next_msg(rx: &SharedRx) -> Option<Vec<u8>> {
    let mut guard = rx.lock().await;
    guard.recv().await
}

/// Способ фрейминга для потоковых транспортов (RFC 6587).
#[derive(Clone, Copy)]
pub enum Framing {
    /// non-transparent-framing: SYSLOG-MSG + LF (%d10).
    NonTransparent,
    /// octet-counting: MSG-LEN SP SYSLOG-MSG (без trailer).
    OctetCounting,
}

impl Framing {
    pub fn parse(s: &str) -> Self {
        match s {
            "octet-counting" | "octet_counting" | "octet" => Framing::OctetCounting,
            _ => Framing::NonTransparent,
        }
    }
}

/// Слить остаток очереди в счётчик ошибок (для нерабочих target'ов),
/// чтобы продюсер не блокировался на переполненном канале.
pub(crate) async fn drain_as_errors(rx: &SharedRx, metrics: &Metrics, addr: &str) {
    while next_msg(rx).await.is_some() {
        record_error(metrics, addr).await;
    }
}

/// Общий хелпер для фрейминга сообщения в переиспользуемый буфер.
/// N6 (v8.7.0): zero-copy/буферизация — раньше `frame()` и `frame_stream()`
/// возвращали новый `Vec<u8>` на каждое сообщение (аллокация в горячем пути).
/// Теперь они принимают `&mut BytesMut` и дописывают туда — буфер
/// переиспользуется между сообщениями через `buf.clear()`.
///
/// - non-transparent: `SYSLOG-MSG LF`
/// - octet-counting:   `MSG-LEN SP SYSLOG-MSG`, где MSG-LEN — число октетов SYSLOG-MSG.
pub(crate) fn frame_into(buf: &mut BytesMut, msg: &[u8], framing: Framing) {
    match framing {
        Framing::NonTransparent => {
            buf.extend_from_slice(msg);
            buf.extend_from_slice(b"\n");
        }
        Framing::OctetCounting => {
            // BytesMut реализует std::fmt::Write — пишем длину напрямую в буфер.
            let _ = write!(buf, "{} ", msg.len());
            buf.extend_from_slice(msg);
        }
    }
}

/// Trait `Transport` (N10, v9.1.0) — абстракция для динамического выбора
/// транспорта. Реализуется в [`TransportKind`] для static dispatch через
/// enum (вместо `Box<dyn Transport>` — экономия heap-аллокаций на горячем
/// пути). Используется в `run_phase_multi` через `TransportKind::from(target)`.
/// `async fn` в trait работает нативно с Rust 1.75+ (наша версия 1.95).
///
/// В v9.3.0: добавим `Kafka(KafkaConfig)` вариант (F16 — Kafka/Redpanda).
pub trait Transport: Send + Sync {
    /// Имя транспорта для метрик ("file", "tcp", "udp", "tls", "kafka").
    fn name(&self) -> &'static str;
    /// Запустить цикл отправки: читать из `rx`, отправлять через транспорт.
    /// `addr` — конфигурация target'а (путь для file, host:port для tcp/udp/tls,
    /// bootstrap_servers для kafka). Использует `async fn` (Rust 1.75+).
    fn run(
        &self,
        addr: &str,
        phase_name: &str,
        rx: SharedRx,
        metrics: Metrics,
        shutdown: CancellationToken,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

/// Конкретный выбор транспорта для фазы (N10, v9.1.0).
/// Используется в `run_phase_multi` для static dispatch через enum —
/// 0 heap-аллокаций, 0 vtable lookups.
///
/// F16 (v9.3.0): Kafka-транспорт НЕ включён в `TransportKind` — он
/// обрабатывается отдельной веткой в `run_phase_multi` через прямой
/// вызов `kafka::target_sender_kafka`, потому что требует отдельной
/// `KafkaConfig` (feature-gated). Это упрощает тип `TransportKind`
/// (все варианты — теги без данных) и избавляет от cfg-ветвлений
/// внутри `Transport::run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    File,
    Tcp,
    Udp,
    Tls,
}

impl Transport for TransportKind {
    fn name(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Tls => "tls",
        }
    }

    /// `async fn` в trait (Rust 1.75+). Для каждого варианта enum
    /// вызываем соответствующую `target_sender_*` функцию с переданными
    /// `addr`/`phase_name`. Это даёт static dispatch (0 vtable lookups).
    /// Send bound автоматически выводится из captures (все наши типы Send).
    ///
    /// F16 (v9.3.0): `Self::Kafka` диспатчит `kafka::target_sender_kafka`.
    /// Конфиг (`KafkaConfig`) не хранится в варианте enum (чтобы не
    /// требовать feature `kafka` для пользователей без Kafka) — он
    /// собирается в `run_phase_multi` из полей `TargetConfig.kafka_*`
    /// и передаётся явно.
    async fn run(
        &self,
        addr: &str,
        phase_name: &str,
        rx: SharedRx,
        metrics: Metrics,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        match self {
            Self::File => {
                file::target_sender_file(
                    addr.to_string(),
                    phase_name.to_string(),
                    rx,
                    metrics,
                    shutdown,
                )
                .await
            }
            Self::Tcp => {
                tcp::target_sender_tcp(
                    addr.to_string(),
                    phase_name.to_string(),
                    rx,
                    metrics,
                    shutdown,
                    crate::transport::Framing::NonTransparent,
                    None, // default reconnect
                )
                .await
            }
            Self::Udp => {
                udp::target_sender_udp(
                    addr.to_string(),
                    phase_name.to_string(),
                    rx,
                    metrics,
                    shutdown,
                )
                .await
            }
            Self::Tls => {
                tls::target_sender_tls(
                    addr.to_string(),
                    crate::transport::tls::TlsParams::default(),
                    phase_name.to_string(),
                    rx,
                    metrics,
                    shutdown,
                    crate::transport::Framing::NonTransparent,
                    None, // default reconnect
                )
                .await
            }
        }
    }
}

// Подмодули реализации конкретных транспортов.
pub mod file;
#[cfg(feature = "kafka")]
pub mod kafka;
pub(crate) mod reconnect;
pub mod tcp;
pub mod tls;
pub mod udp;

// Re-exports для API, экспортируемого из `pub use` в `lib.rs`.
// (`reconnect_tcp` и `tls_connect` остаются pub(crate) — это внутренние
// helpers sender'ов, не часть публичного API.)

// Обёртки для backward-compat: `syslog_generator::target_sender_file` и т.д.
pub use file::target_sender_file;
pub use tcp::target_sender_tcp;
pub use tls::{
    build_tls_connector, parse_cipher_suite, parse_tls_min_version, target_sender_tls, TlsParams,
    TlsVersion,
};
pub use udp::target_sender_udp;

// ===== N10 (v9.1.0): тесты trait Transport =====
#[cfg(test)]
mod tests {
    use super::*;

    /// N10: `TransportKind::name()` возвращает правильное имя.
    #[test]
    fn n10_transportkind_name() {
        assert_eq!(TransportKind::File.name(), "file");
        assert_eq!(TransportKind::Tcp.name(), "tcp");
        assert_eq!(TransportKind::Udp.name(), "udp");
        assert_eq!(TransportKind::Tls.name(), "tls");
    }

    /// N10: `Transport` реализован для `TransportKind` (compile-time check).
    /// Если сигнатура trait изменится, перестанет компилироваться.
    /// (Используется как compile-only assertion через `fn f<T: Transport>()
    /// { }` — аналог `_assert_impl` в других модулях, проверяет что
    /// `TransportKind: Transport` без вызова.)
    #[allow(dead_code)]
    fn _assert_transport_impl() {
        fn _f<T: Transport>() {}
        // Компилируется только если TransportKind: Transport.
        let _: fn() = || _f::<TransportKind>();
    }
}
