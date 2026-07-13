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

/// Trait `Transport` (планируется в вехе E для динамического выбора транспорта).
/// Сейчас транспорты — это просто функции `target_sender_*`.
pub trait Transport: Send + Sync {
    /// Имя транспорта для метрик ("file", "tcp", "udp", "tls").
    fn name(&self) -> &'static str;
    /// Запустить цикл отправки: читать из `rx`, отправлять через транспорт.
    fn run(
        &self,
        rx: SharedRx,
        metrics: Metrics,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()>;
}

// Подмодули реализации конкретных транспортов.
pub mod file;
pub mod tcp;
pub mod tls;
pub mod udp;

// Re-exports для API, экспортируемого из `pub use` в `lib.rs`.
// (`reconnect_tcp` и `tls_connect` остаются pub(crate) — это внутренние
// helpers sender'ов, не часть публичного API.)

// Обёртки для backward-compat: `syslog_generator::target_sender_file` и т.д.
pub use file::target_sender_file;
pub use tcp::target_sender_tcp;
pub use tls::{build_tls_connector, parse_tls_min_version, target_sender_tls, TlsParams};
pub use udp::target_sender_udp;
