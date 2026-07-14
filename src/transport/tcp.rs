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
