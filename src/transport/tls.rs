//! N10 (v8.8.0): TLS transport (RFC 5425 / STARTTLS over TCP).
//!
//! N4 (v8.2.0): безопасный TLS по умолчанию (проверка сертификата,
//! SNI, hostname). N4.mTLS (v8.7.2): клиентский сертификат + min_protocol.

use crate::metrics::Metrics;
use anyhow::Result;
use bytes::BytesMut;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_native_tls::TlsStream;
use tokio_util::sync::CancellationToken;

use super::{
    drain_as_errors, frame_into, next_msg, record_error, record_reconnect, record_send,
    record_send_latency, Framing, SharedRx,
};

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
pub(crate) async fn tls_connect(
    connector: &tokio_native_tls::TlsConnector,
    addr: &str,
    domain: &str,
) -> Option<TlsStream<TcpStream>> {
    let stream = TcpStream::connect(addr).await.ok()?;
    connector.connect(domain, stream).await.ok()
}
