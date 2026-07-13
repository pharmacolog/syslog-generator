//! N10 (v8.8.0): UDP transport — zero-copy по дизайну (send_to(&msg, ...)).
//!
//! Каждый datagram = один syscall (UDP не поддерживает batching на уровне
//! payload). `send_to` принимает &[u8] и не копирует payload — это самый
//! zero-friendly транспорт.

use crate::metrics::Metrics;
use anyhow::Result;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use super::{next_msg, record_error, record_send, record_send_latency, SharedRx};

pub async fn target_sender_udp(
    addr: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
) -> Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    while let Some(msg) = next_msg(&rx).await {
        let t0 = std::time::Instant::now();
        if socket.send_to(&msg, &addr).await.is_err() {
            record_error(&metrics, &addr).await;
        } else {
            record_send_latency(&metrics, t0.elapsed());
            record_send(
                &metrics,
                "udp",
                &phase_name,
                &addr,
                msg.len() as u64,
                &shutdown,
            )
            .await;
        }
    }
    Ok(())
}
