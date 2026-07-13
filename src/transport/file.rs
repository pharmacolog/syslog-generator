//! N10 (v8.8.0): file transport — запись в локальный файл через BufWriter.

use crate::metrics::Metrics;
use anyhow::Result;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;

use super::{next_msg, record_send, record_send_latency, SharedRx};

pub async fn target_sender_file(
    path: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
) -> Result<()> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    // N6 (v8.7.0): `BufWriter` (8 KiB) — мелкие write коалесцируются в один
    // syscall, что уменьшает число системных вызовов в ~N раз для
    // типичной нагрузки (mpsc(1024) → 1024 одиночных write'ов без
    // буфера → 1024 syscall'а, с буфером → 128 syscall'ов при 8 KiB
    // capacity). Flush делается автоматически в Drop при завершении.
    let mut writer = BufWriter::with_capacity(8 * 1024, file);
    while let Some(msg) = next_msg(&rx).await {
        // O_APPEND гарантирует атомарность дозаписи, BufWriter
        // коалесцирует мелкие write в один syscall.
        let t0 = std::time::Instant::now();
        writer.write_all(&msg).await?;
        writer.write_all(b"\n").await?;
        record_send_latency(&metrics, t0.elapsed());
        record_send(
            &metrics,
            "file",
            &phase_name,
            &path,
            msg.len() as u64,
            &shutdown,
        )
        .await;
    }
    // Explicit flush перед выходом — содержимое буфера попадает на диск.
    writer.flush().await?;
    Ok(())
}
