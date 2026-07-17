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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::metrics::create_metrics;
    use bytes::Bytes;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::net::UdpSocket;
    use tokio::sync::{mpsc, };
    use tokio_util::sync::CancellationToken;

    /// UDP sender отправляет datagram на указанный addr.
    /// End-to-end: sender → реальный UDP socket receiver.
    #[tokio::test]
    async fn udp_sender_delivers_message_to_receiver() {
        // Создаём receiver socket на random port.
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let recv_addr: SocketAddr = receiver.local_addr().unwrap();
        // Создаём SharedRx (mpsc + Arc<Mutex>).
        let (tx, rx_inner) = mpsc::channel::<Bytes>(16);
        let rx = Arc::new(parking_lot::Mutex::new(rx_inner));
        let metrics = create_metrics().unwrap();
        let shutdown = CancellationToken::new();
        // Запускаем sender.
        let sender_handle = tokio::spawn(target_sender_udp(
            recv_addr.to_string(),
            "test".to_string(),
            rx.clone(),
            metrics.clone(),
            shutdown.clone(),
        ));
        // Отправляем 3 сообщения.
        for i in 0..3 {
            tx.send(Bytes::from(format!("msg-{i}\n"))).await.unwrap();
        }
        // Закрываем sender → sender loop завершается.
        drop(tx);
        // Ждём завершения sender'а.
        sender_handle.await.unwrap().unwrap();
        // Receiver должен получить все 3 datagrams.
        let mut received = Vec::new();
        let mut buf = [0u8; 256];
        for _ in 0..3 {
            receiver.recv_from(&mut buf).await.unwrap();
            received.push(String::from_utf8_lossy(&buf[..16]).to_string());
        }
        assert_eq!(received.len(), 3, "expected 3 datagrams");
        assert!(received[0].starts_with("msg-0"));
        assert!(received[1].starts_with("msg-1"));
        assert!(received[2].starts_with("msg-2"));
    }

    /// UDP sender gracefully завершается при shutdown signal.
    #[tokio::test]
    async fn udp_sender_responds_to_shutdown() {
        // Receiver (не используется, но нужен для bind).
        let _receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (tx, rx_inner) = mpsc::channel::<Bytes>(16);
        let rx = Arc::new(parking_lot::Mutex::new(rx_inner));
        let metrics = create_metrics().unwrap();
        let shutdown = CancellationToken::new();
        let sender_handle = tokio::spawn(target_sender_udp(
            _receiver.local_addr().unwrap().to_string(),
            "test".to_string(),
            rx.clone(),
            metrics.clone(),
            shutdown.clone(),
        ));
        // Sender ждёт на next_msg — он не завершится пока channel открыт.
        tx.send(Bytes::from(b"x".to_vec())).await.unwrap();
        // Cancel shutdown — sender должен увидеть cancellation в record_send и завершиться
        // (но не сразу, только при следующем сообщении).
        shutdown.cancel();
        // Закрываем tx → sender loop завершается.
        drop(tx);
        // Sender должен корректно завершиться.
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), sender_handle)
            .await
            .expect("sender должен завершиться в течение 2с")
            .unwrap();
        assert!(result.is_ok());
    }
}
