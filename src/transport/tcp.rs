//! N10 (v8.8.0): TCP transport — connect + write + auto-reconnect.
//!
//! N6 (v8.7.0) zero-copy/буферизация: `BytesMut` (8 KiB) переиспользуется
//! между сообщениями; `frame_into` дописывает в буфер, `write_all` отправляет
//! сразу N сообщений → уменьшение TCP write-syscall'ов и Nagle overhead.

use crate::metrics::Metrics;
use anyhow::Result;
use bytes::BytesMut;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::{
    drain_as_errors, frame_into, next_msg, record_error, record_reconnect, record_send,
    record_send_latency, Framing, SharedRx,
};

pub async fn target_sender_tcp(
    addr: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
    framing: Framing,
) -> Result<()> {
    let mut stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(_) => {
            // Connection failure is a real transport error: record it and
            // drain the queue so upstream producers do not block.
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    // N6 (v8.7.0): `BytesMut` (8 KiB) переиспользуется между сообщениями.
    // Раньше `frame_stream()` возвращал новый `Vec<u8>` на каждое сообщение
    // (аллокация + format! префикса длины). Теперь фрейм дописывается в
    // буфер, и единственный `write_all` отправляет сразу N сообщений,
    // что уменьшает число TCP write-syscall'ов (а с ними и Nagle
    // algorithm overhead) пропорционально размеру буфера / сообщения.
    let mut buf = BytesMut::with_capacity(8 * 1024);
    while let Some(msg) = next_msg(&rx).await {
        // 1. Фреймим сообщение в переиспользуемый буфер.
        frame_into(&mut buf, &msg, framing);
        let t0 = std::time::Instant::now();
        if stream.write_all(&buf).await.is_err() {
            record_error(&metrics, &addr).await;
            buf.clear();
            // Попытка переустановить соединение и повторно отправить сообщение.
            match reconnect_tcp(&addr, &metrics, "tcp").await {
                Some(s) => {
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
                None => {
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

/// Переустановить TCP-соединение после ошибки записи (одна попытка).
/// Инкрементирует `syslog_reconnects_total`. Возвращает новый поток или None.
pub(crate) async fn reconnect_tcp(
    addr: &str,
    metrics: &Metrics,
    transport: &str,
) -> Option<TcpStream> {
    record_reconnect(metrics, transport, addr);
    TcpStream::connect(addr).await.ok()
}
