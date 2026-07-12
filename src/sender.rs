use crate::metrics::Metrics;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
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
fn record_send_latency(metrics: &Metrics, elapsed: std::time::Duration) {
    metrics.send_duration.observe(elapsed.as_secs_f64());
}

/// Отметить попытку переустановки соединения.
fn record_reconnect(metrics: &Metrics, transport: &str, target: &str) {
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
async fn next_msg(rx: &SharedRx) -> Option<Vec<u8>> {
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

/// Собрать пейлоад с trailer `\n` (non-transparent-framing) в один буфер.
/// Для файла с O_APPEND единый write_all даёт атомарность дозаписи и
/// исключает перемешивание строк между воркерами пула.
fn frame(msg: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(msg.len() + 1);
    buf.extend_from_slice(msg);
    buf.push(b'\n');
    buf
}

/// Обрамить сообщение согласно выбранному способу (RFC 6587).
/// octet-counting: `MSG-LEN SP SYSLOG-MSG`, где MSG-LEN — число октетов SYSLOG-MSG.
/// non-transparent: `SYSLOG-MSG LF`.
fn frame_stream(msg: &[u8], framing: Framing) -> Vec<u8> {
    match framing {
        Framing::NonTransparent => frame(msg),
        Framing::OctetCounting => {
            let prefix = format!("{} ", msg.len());
            let mut buf = Vec::with_capacity(prefix.len() + msg.len());
            buf.extend_from_slice(prefix.as_bytes());
            buf.extend_from_slice(msg);
            buf
        }
    }
}

/// Слить остаток очереди в счётчик ошибок (для нерабочих target'ов),
/// чтобы продюсер не блокировался на переполненном канале.
async fn drain_as_errors(rx: &SharedRx, metrics: &Metrics, addr: &str) {
    while next_msg(rx).await.is_some() {
        record_error(metrics, addr).await;
    }
}

pub async fn target_sender_file(
    path: String,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    while let Some(msg) = next_msg(&rx).await {
        // Единый write_all + O_APPEND — атомарная дозапись без перемешивания
        // между конкурентными воркерами пула.
        let t0 = std::time::Instant::now();
        file.write_all(&frame(&msg)).await?;
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
    Ok(())
}

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
    while let Some(msg) = next_msg(&rx).await {
        let framed = frame_stream(&msg, framing);
        let t0 = std::time::Instant::now();
        if stream.write_all(&framed).await.is_err() {
            record_error(&metrics, &addr).await;
            // Попытка переустановить соединение и повторно отправить сообщение.
            match reconnect_tcp(&addr, &metrics, "tcp").await {
                Some(s) => {
                    stream = s;
                    let t1 = std::time::Instant::now();
                    if stream.write_all(&framed).await.is_ok() {
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
    }
    Ok(())
}

/// Переустановить TCP-соединение после ошибки записи (одна попытка).
/// Инкрементирует `syslog_reconnects_total`. Возвращает новый поток или None.
async fn reconnect_tcp(addr: &str, metrics: &Metrics, transport: &str) -> Option<TcpStream> {
    record_reconnect(metrics, transport, addr);
    TcpStream::connect(addr).await.ok()
}

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

/// N4: параметры TLS-подключения для одного target'а.
#[derive(Clone, Debug, Default)]
pub struct TlsParams {
    /// Имя хоста для SNI и проверки имени в сертификате.
    pub domain: String,
    /// PEM-содержимое доверенного CA (уже прочитано из файла) или None.
    pub ca_pem: Option<Vec<u8>>,
    /// Небезопасный режим: принять любой сертификат.
    pub insecure: bool,
}

/// N4: строит TLS-connector с проверкой сертификатов по умолчанию.
/// Если задан `ca_pem` — добавляет его к корням доверия. Если `insecure`
/// — отключает проверку (явный opt-in).
pub fn build_tls_connector(params: &TlsParams) -> Result<tokio_native_tls::TlsConnector> {
    let mut builder = native_tls::TlsConnector::builder();
    if params.insecure {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    } else if let Some(pem) = &params.ca_pem {
        let cert = native_tls::Certificate::from_pem(pem)?;
        builder.add_root_certificate(cert);
    }
    let connector = builder.build()?;
    Ok(tokio_native_tls::TlsConnector::from(connector))
}

pub async fn target_sender_tls(
    addr: String,
    tls_params: TlsParams,
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
    framing: Framing,
) -> Result<()> {
    let domain = tls_params.domain.clone();
    let connector = match build_tls_connector(&tls_params) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("TLS ({addr}): не удалось построить connector: {e}");
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    let mut tls = match tls_connect(&connector, &addr, &domain).await {
        Some(t) => t,
        None => {
            // TCP/TLS handshake failure: record and drain.
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    while let Some(msg) = next_msg(&rx).await {
        let framed = frame_stream(&msg, framing);
        let t0 = std::time::Instant::now();
        if tls.write_all(&framed).await.is_err() {
            record_error(&metrics, &addr).await;
            // Попытка переустановить TLS-соединение (новый handshake).
            record_reconnect(&metrics, "tls", &addr);
            match tls_connect(&connector, &addr, &domain).await {
                Some(new_tls) => {
                    tls = new_tls;
                    let t1 = std::time::Instant::now();
                    if tls.write_all(&framed).await.is_ok() {
                        record_send_latency(&metrics, t1.elapsed());
                        record_send(
                            &metrics,
                            "tls",
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
                "tls",
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

/// Установить TLS-соединение (TCP connect + handshake). None при любой ошибке.
async fn tls_connect(
    connector: &tokio_native_tls::TlsConnector,
    addr: &str,
    domain: &str,
) -> Option<tokio_native_tls::TlsStream<TcpStream>> {
    let stream = TcpStream::connect(addr).await.ok()?;
    connector.connect(domain, stream).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// N4: безопасный режим по умолчанию — connector строится без ошибок,
    /// системные корни доверия активны, insecure выключен.
    #[test]
    fn test_build_tls_connector_secure_default() {
        let params = TlsParams {
            domain: "example.com".into(),
            ca_pem: None,
            insecure: false,
        };
        assert!(build_tls_connector(&params).is_ok());
    }

    /// N4: небезопасный режим (явный opt-in) тоже строится без ошибок.
    #[test]
    fn test_build_tls_connector_insecure() {
        let params = TlsParams {
            domain: "example.com".into(),
            ca_pem: None,
            insecure: true,
        };
        assert!(build_tls_connector(&params).is_ok());
    }

    /// N4: битый CA-PEM → ошибка построения connector (а не паника).
    #[test]
    fn test_build_tls_connector_bad_ca_errs() {
        let params = TlsParams {
            domain: "example.com".into(),
            ca_pem: Some(b"not a real certificate".to_vec()),
            insecure: false,
        };
        assert!(build_tls_connector(&params).is_err());
    }
}
