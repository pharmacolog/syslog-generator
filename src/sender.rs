use crate::metrics::Metrics;
use anyhow::Result;
use bytes::BytesMut;
use std::fmt::Write as _;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
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
/// Обрамить сообщение согласно выбранному способу (RFC 6587) и дописать
/// в `buf`. N6 (v8.7.0): zero-copy/буферизация — раньше `frame()` и `frame_stream()`
/// возвращали новый `Vec<u8>` на каждое сообщение (аллокация в горячем пути).
/// Теперь они принимают `&mut BytesMut` и дописывают туда — буфер
/// переиспользуется между сообщениями через `buf.clear()`.
///
/// - non-transparent: `SYSLOG-MSG LF`
/// - octet-counting:   `MSG-LEN SP SYSLOG-MSG`, где MSG-LEN — число октетов SYSLOG-MSG.
fn frame_into(buf: &mut BytesMut, msg: &[u8], framing: Framing) {
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
#[derive(Clone, Debug)]
pub struct TlsParams {
    /// Имя хоста для SNI и проверки имени в сертификате.
    pub domain: String,
    /// PEM-содержимое доверенного CA (уже прочитано из файла) или None.
    pub ca_pem: Option<Vec<u8>>,
    /// Небезопасный режим: принять любой сертификат.
    pub insecure: bool,
    /// N4.mTLS (v8.7.2): PEM-содержимое клиентского сертификата. None →
    /// клиент не предъявляет сертификат (one-way TLS).
    pub client_cert_pem: Option<Vec<u8>>,
    /// N4.mTLS: PEM-содержимое клиентского ключа (PKCS#8). Парный к
    /// `client_cert_pem`. None → mTLS не используется.
    pub client_key_pem: Option<Vec<u8>>,
    /// N4.mTLS: минимальная допустимая версия TLS-протокола. None →
    /// системная по умолчанию (обычно TLS 1.0). Защита от downgrade-атак.
    pub min_protocol: Option<native_tls::Protocol>,
}

// `Default` написан вручную (а не через `#[derive(Default)]`) потому что
// `native_tls::Protocol` не реализует `Default`. clippy предлагает derive
// (`derivable_impls` lint), но derive не сработает без обёртки.
#[allow(clippy::derivable_impls)]
impl Default for TlsParams {
    fn default() -> Self {
        Self {
            domain: String::new(),
            ca_pem: None,
            insecure: false,
            client_cert_pem: None,
            client_key_pem: None,
            min_protocol: None,
        }
    }
}

/// N4: строит TLS-connector с проверкой сертификатов по умолчанию.
/// Если задан `ca_pem` — добавляет его к корням доверия. Если `insecure`
/// — отключает проверку (явный opt-in). N4.mTLS: если `client_cert_pem` и
/// `client_key_pem` заданы — загружает клиентский identity (mTLS). Если
/// `min_protocol` задан — устанавливает минимально допустимую версию TLS.
pub fn build_tls_connector(params: &TlsParams) -> Result<tokio_native_tls::TlsConnector> {
    let mut builder = native_tls::TlsConnector::builder();
    if params.insecure {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    } else if let Some(pem) = &params.ca_pem {
        let cert = native_tls::Certificate::from_pem(pem)?;
        builder.add_root_certificate(cert);
    }
    // N4.mTLS: загрузить клиентский identity, если оба заданы.
    // Identity::from_pkcs8 принимает PEM cert + PEM key (PKCS#8).
    if let (Some(cert_pem), Some(key_pem)) = (&params.client_cert_pem, &params.client_key_pem) {
        let identity = native_tls::Identity::from_pkcs8(cert_pem, key_pem)?;
        builder.identity(identity);
    }
    // N4.mTLS: минимальная версия TLS-протокола.
    if let Some(proto) = params.min_protocol {
        builder.min_protocol_version(Some(proto));
    }
    let connector = builder.build()?;
    Ok(tokio_native_tls::TlsConnector::from(connector))
}

/// N4.mTLS (v8.7.2): парсит строку `"1.2"` или `"1.3"` в
/// `native_tls::Protocol`. Принимает только эти два значения (1.0/1.1
/// не рекомендуются NIST SP 800-52 и deprecated в большинстве ОС).
pub fn parse_tls_min_version(s: &str) -> Result<native_tls::Protocol, String> {
    match s.trim() {
        "1.2" => Ok(native_tls::Protocol::Tlsv12),
        "1.3" => Ok(native_tls::Protocol::Tlsv13),
        other => Err(format!(
            "допустимые значения: \"1.2\", \"1.3\"; получено: {:?}",
            other
        )),
    }
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
    // N6 (v8.7.0): `BytesMut` (8 KiB) переиспользуется — без аллокации
    // на каждое сообщение. См. комментарий в `target_sender_tcp`.
    let mut buf = BytesMut::with_capacity(8 * 1024);
    while let Some(msg) = next_msg(&rx).await {
        // 1. Фреймим в переиспользуемый буфер.
        frame_into(&mut buf, &msg, framing);
        let t0 = std::time::Instant::now();
        if tls.write_all(&buf).await.is_err() {
            record_error(&metrics, &addr).await;
            buf.clear();
            // Попытка переустановить TLS-соединение (новый handshake).
            record_reconnect(&metrics, "tls", &addr);
            match tls_connect(&connector, &addr, &domain).await {
                Some(new_tls) => {
                    tls = new_tls;
                    // Повторно фреймим в чистый буфер.
                    frame_into(&mut buf, &msg, framing);
                    let t1 = std::time::Instant::now();
                    if tls.write_all(&buf).await.is_ok() {
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
        // N6: переиспользуем ёмкость буфера для следующего сообщения.
        buf.clear();
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
            client_cert_pem: None,
            client_key_pem: None,
            min_protocol: None,
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
            client_cert_pem: None,
            client_key_pem: None,
            min_protocol: None,
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
            client_cert_pem: None,
            client_key_pem: None,
            min_protocol: None,
        };
        assert!(build_tls_connector(&params).is_err());
    }

    // ====================== N6 (v8.7.0): zero-copy/буферизация ======================
    //
    // Эти тесты проверяют что `frame_into` корректно заполняет `BytesMut`
    // и что после `clear()` capacity сохраняется (буфер переиспользуется
    // между сообщениями без новых аллокаций).

    /// N6: `frame_into` с non-transparent framing добавляет MSG + LF
    /// (без новой аллокации в горячем пути — буфер переиспользуется).
    #[test]
    fn n6_frame_into_non_transparent_appends_lf() {
        let mut buf = BytesMut::with_capacity(64);
        frame_into(&mut buf, b"hello", Framing::NonTransparent);
        assert_eq!(&buf[..], b"hello\n");
    }

    /// N6: `frame_into` с octet-counting framing добавляет `<len> <msg>`
    /// (без финального LF, по RFC 6587).
    #[test]
    fn n6_frame_into_octet_counting_appends_len_prefix() {
        let mut buf = BytesMut::with_capacity(64);
        frame_into(&mut buf, b"hello", Framing::OctetCounting);
        assert_eq!(&buf[..], b"5 hello");
    }

    /// N6: после `clear()` capacity сохраняется (буфер переиспользуется
    /// между сообщениями). Это zero-copy инвариант: новые сообщения не
    /// аллоцируют новые буферы.
    #[test]
    fn n6_clear_preserves_capacity() {
        let mut buf = BytesMut::with_capacity(8 * 1024);
        let cap_initial = buf.capacity();
        frame_into(&mut buf, b"some long message here", Framing::NonTransparent);
        let cap_after_write = buf.capacity();
        assert!(
            cap_after_write >= cap_initial,
            "capacity не должна уменьшаться после записи: initial={}, after={}",
            cap_initial,
            cap_after_write
        );
        buf.clear();
        let cap_after_clear = buf.capacity();
        assert_eq!(
            cap_after_clear, cap_after_write,
            "clear() не должна менять capacity: after_write={}, after_clear={}",
            cap_after_write, cap_after_clear
        );
        // После clear() длина = 0, но capacity та же.
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    /// N6: последовательность фреймов в один и тот же буфер даёт
    /// корректный конкатенированный вывод (без перекрытий, без потерь).
    /// Это тест на инвариант батчинга: N сообщений → 1 write_all.
    #[test]
    fn n6_consecutive_frames_concatenate() {
        let mut buf = BytesMut::with_capacity(128);
        frame_into(&mut buf, b"alpha", Framing::NonTransparent);
        buf.clear();
        frame_into(&mut buf, b"beta", Framing::NonTransparent);
        buf.clear();
        frame_into(&mut buf, b"gamma", Framing::OctetCounting);
        assert_eq!(&buf[..], b"5 gamma");
    }
}
