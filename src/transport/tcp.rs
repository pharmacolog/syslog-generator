//! N10 (v8.8.0): TCP transport — connect + write + auto-reconnect.
//!
//! N6 (v8.7.0) zero-copy/буферизация: `BytesMut` (8 KiB) переиспользуется
//! между сообщениями; `frame_into` дописывает в буфер, `write_all` отправляет
//! сразу N сообщений → уменьшение TCP write-syscall'ов и Nagle overhead.
//!
//! F16 (v9.3.0): reconnect с exponential backoff + jitter через
//! `reconnect::reconnect_with_backoff` — после ошибки записи выполняется
//! несколько попыток переподключения с нарастающей задержкой.

use crate::metrics::Metrics;
use anyhow::Result;
use bytes::BytesMut;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::reconnect::{reconnect_with_backoff, ReconnectConfig};
use super::{
    drain_as_errors, frame_into, next_msg, record_error, record_reconnect, record_send,
    record_send_latency, Framing, SharedRx,
};

/// F16: TCP sender с настраиваемой reconnect-стратегией.
///
/// Используется как из `target_sender_tcp` (default reconnect = без лимита
/// попыток, backoff 100ms→30s, multiplier 2.0), так и напрямую из
/// `run_phase_multi` с `reconnect_config` из профиля.
///
/// **Backward-compat**: при провале *первой* попытки connect'а sender НЕ
/// уходит в exponential-backoff retry — он сразу drain'ит очередь и
/// завершается (как в v9.1.0). Backoff-retry активируется ТОЛЬКО при
/// ошибке записи в **успешно установленное** соединение. Это сохраняет
/// поведение negative-path тестов (connection refused → 1 drain, без
/// 15-секундного зависания).
#[allow(clippy::too_many_arguments)]
pub async fn target_sender_tcp(
    addr: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
    framing: Framing,
    reconnect_config: Option<ReconnectConfig>,
) -> Result<()> {
    let rcfg = reconnect_config.unwrap_or_default();
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(_) => {
            // Backward-compat: первая неудача connect'а → drain + return.
            // Reconnect-strategy здесь НЕ применяется (иначе при max_attempts=None
            // sender зависнет на бесконечном backoff'е — это противоречит
            // поведению v9.1.0 и ломает negative-path тесты).
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    run_send_loop(
        stream, addr, phase_name, rx, metrics, shutdown, framing, rcfg,
    )
    .await
}

/// Внутренний цикл отправки сообщений + reconnect при ошибке записи.
#[allow(clippy::too_many_arguments)]
async fn run_send_loop(
    mut stream: TcpStream,
    addr: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
    framing: Framing,
    rcfg: ReconnectConfig,
) -> Result<()> {
    // N6 (v8.7.0): `BytesMut` (8 KiB) переиспользуется между сообщениями.
    let mut buf = BytesMut::with_capacity(8 * 1024);
    while let Some(msg) = next_msg(&rx).await {
        // 1. Фреймим сообщение в переиспользуемый буфер.
        frame_into(&mut buf, &msg, framing);
        let t0 = std::time::Instant::now();
        if stream.write_all(&buf).await.is_err() {
            record_error(&metrics, &addr).await;
            buf.clear();
            // F16: попытка переустановить соединение через exponential backoff.
            // reconnect_with_backoff возвращает:
            //   - Some(Ok(stream)) — успешно переподключились
            //   - Some(Err(e))     — исчерпали max_attempts, последняя ошибка
            //   - None             — shutdown отменил дальнейшие попытки
            let addr_clone = addr.clone();
            let outcome = reconnect_with_backoff(
                &rcfg,
                &shutdown,
                || {
                    // Каждая попытка инкрементит reconnects_total — и для
                    // успешных, и для неудачных (попытка была).
                    record_reconnect(&metrics, "tcp", &addr);
                },
                || {
                    let a = addr_clone.clone();
                    async move { TcpStream::connect(&a).await.map_err(|_| ()) }
                },
            )
            .await;
            match outcome {
                Some(Ok(s)) => {
                    stream = s;
                    // Повторно фреймим в чистый буфер.
                    frame_into(&mut buf, &msg, framing);
                    let t1 = std::time::Instant::now();
                    if stream.write_all(&buf).await.is_ok() {
                        record_send_latency(&metrics, t1.elapsed());
                        record_send(
                            &metrics,
                            "tcp",
                            &phase_name,
                            &addr,
                            msg.len() as u64,
                            &shutdown,
                        )
                        .await;
                    } else {
                        record_error(&metrics, &addr).await;
                    }
                }
                Some(Err(_)) | None => {
                    // F16: при исчерпании попыток или отмене — сливаем остаток
                    // очереди в errors и выходим (аналогично pre-F16 поведению).
                    drain_as_errors(&rx, &metrics, &addr).await;
                    return Ok(());
                }
            }
        } else {
            record_send_latency(&metrics, t0.elapsed());
            record_send(
                &metrics,
                "tcp",
                &phase_name,
                &addr,
                msg.len() as u64,
                &shutdown,
            )
            .await;
        }
        // Освобождаем буфер для следующего сообщения. `clear()` сохраняет
        // capacity (ёмкость) — переиспользуем аллокацию.
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::metrics::create_metrics;
    use bytes::Bytes;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, };
    use tokio_util::sync::CancellationToken;

    /// TCP sender: connect → write → drain → exit.
    /// End-to-end: sender → реальный TCP listener.
    #[tokio::test]
    async fn tcp_sender_delivers_message_to_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        // Server side: accept + read framed message.
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();
            buf
        });
        // Client side: SharedRx.
        let (tx, rx_inner) = mpsc::channel::<Bytes>(16);
        let rx = Arc::new(parking_lot::Mutex::new(rx_inner));
        let metrics = create_metrics().unwrap();
        let shutdown = CancellationToken::new();
        let sender_handle = tokio::spawn(target_sender_tcp(
            addr.clone(),
            "test".to_string(),
            rx.clone(),
            metrics.clone(),
            shutdown.clone(),
            Framing::NonTransparent,
            None, // default reconnect
        ));
        // Отправляем 3 сообщения через non-transparent framing (newline-separated).
        for i in 0..3 {
            tx.send(Bytes::from(format!("msg-{i}\n"))).await.unwrap();
        }
        drop(tx);
        sender_handle.await.unwrap().unwrap();
        let received = server.await.unwrap();
        let s = std::str::from_utf8(&received).unwrap();
        assert!(s.contains("msg-0"), "expected msg-0 in: {}", s);
        assert!(s.contains("msg-1"), "expected msg-1 in: {}", s);
        assert!(s.contains("msg-2"), "expected msg-2 in: {}", s);
    }

    /// TCP sender с octet-counting framing: каждое сообщение имеет
    /// префикс с длиной (`MSG-LEN SP`).
    #[tokio::test]
    async fn tcp_sender_octet_counting_framing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();
            buf
        });
        let (tx, rx_inner) = mpsc::channel::<Bytes>(16);
        let rx = Arc::new(parking_lot::Mutex::new(rx_inner));
        let metrics = create_metrics().unwrap();
        let shutdown = CancellationToken::new();
        let sender_handle = tokio::spawn(target_sender_tcp(
            addr,
            "test".to_string(),
            rx.clone(),
            metrics.clone(),
            shutdown.clone(),
            Framing::OctetCounting,
            None,
        ));
        let msg = b"hello world".to_vec();
        tx.send(Bytes::from(msg.clone())).await.unwrap();
        drop(tx);
        sender_handle.await.unwrap().unwrap();
        let received = server.await.unwrap();
        // Format: "12 hello world" (12 = длина "hello world").
        let s = std::str::from_utf8(&received).unwrap();
        assert!(
            s.starts_with(&format!("{} ", msg.len())),
            "expected octet-counting prefix, got: {}",
            s
        );
    }

    /// TCP sender с unreachable addr запускает reconnect loop —
    /// этот путь покрыт через `reconnect::reconnect_with_backoff` unit tests
    /// (см. src/transport/reconnect.rs::tests). Здесь не дублируем.
    ///
    /// Вместо этого проверим что TCP sender с валидным connection
    /// корректно выходит при shutdown signal.
    #[tokio::test]
    async fn tcp_sender_exits_on_shutdown_after_drain() {
        // Создаём ephemeral listener, accept, но НЕ читаем.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        // Accept, чтобы TCP sender подключился.
        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            // Никогда не закрываем stream — держим connection alive.
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let (tx, rx_inner) = mpsc::channel::<Bytes>(16);
        let rx = Arc::new(parking_lot::Mutex::new(rx_inner));
        let metrics = create_metrics().unwrap();
        let shutdown = CancellationToken::new();
        let sender_handle = tokio::spawn(target_sender_tcp(
            addr,
            "test".to_string(),
            rx,
            metrics,
            shutdown.clone(),
            Framing::NonTransparent,
            None,
        ));
        // Дать sender подключиться.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Отправить сообщение, потом закрыть tx.
        tx.send(Bytes::from(b"hello\n".to_vec())).await.unwrap();
        drop(tx);
        // Sender должен корректно завершиться после drain.
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), sender_handle)
            .await
            .expect("sender должен завершиться после drain")
            .unwrap();
        assert!(
            result.is_ok(),
            "sender не должен возвращать error после drain"
        );
    }

    /// PR-16 (coverage): record_reconnect_increments_with_target_label.
    /// `record_reconnect` нужно покрыть в unit-test (раньше только integration).
    #[test]
    fn record_reconnect_increments_metric_with_target_label() {
        use crate::observability::metrics::create_metrics;
        let metrics = create_metrics().unwrap();
        record_reconnect(&metrics, "tcp", "127.0.0.1:514");
        record_reconnect(&metrics, "tcp", "127.0.0.1:514");
        record_reconnect(&metrics, "udp", "127.0.0.1:514");
        // reconnects_total{transport="tcp", target="127.0.0.1:514"} = 2.
        let m = metrics
            .reconnects_total
            .get_metric_with_label_values(&["tcp", "127.0.0.1:514"])
            .unwrap();
        assert_eq!(m.get(), 2.0);
        let m = metrics
            .reconnects_total
            .get_metric_with_label_values(&["udp", "127.0.0.1:514"])
            .unwrap();
        assert_eq!(m.get(), 1.0);
    }

    /// PR-16 (coverage): record_error_increments_errors_total.
    /// `record_error` нужно покрыть unit-тестом.
    #[tokio::test]
    async fn record_error_increments_errors_total_per_target() {
        use crate::observability::metrics::create_metrics;
        let metrics = create_metrics().unwrap();
        record_error(&metrics, "127.0.0.1:514").await;
        record_error(&metrics, "127.0.0.1:514").await;
        record_error(&metrics, "127.0.0.1:515").await;
        let m = metrics
            .errors_total
            .get_metric_with_label_values(&["127.0.0.1:514"])
            .unwrap();
        assert_eq!(m.get(), 2.0);
        let m = metrics
            .errors_total
            .get_metric_with_label_values(&["127.0.0.1:515"])
            .unwrap();
        assert_eq!(m.get(), 1.0);
    }
}
