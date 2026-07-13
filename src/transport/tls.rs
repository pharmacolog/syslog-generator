//! N10 (v8.8.0): TLS transport (RFC 5425 / STARTTLS over TCP).
//!
//! N4 (v8.2.0): безопасный TLS по умолчанию (проверка сертификата,
//! SNI, hostname). N4.mTLS (v8.7.2): клиентский сертификат + min_protocol.
//!
//! N4.cipher_policy (v9.5.0): **миграция native-tls → rustls**. native-tls
//! использует системный TLS-бэкенд (SChannel/SecureTransport/OpenSSL);
//! `set_cipher_list` доступен только в OpenSSL-бэкенде (Linux-only).
//! rustls — pure Rust, кросс-платформенный, поддерживает явный выбор
//! `cipher_suites` через `ClientConfig`. Это **breaking change** в
//! публичном API транспорта (см. CHANGELOG v9.5.0).

use crate::metrics::Metrics;
use anyhow::{anyhow, Result};
use bytes::BytesMut;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{PrivateKeyDer, ServerName};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_util::sync::CancellationToken;

use super::{
    drain_as_errors, frame_into, next_msg, record_error, record_reconnect, record_send,
    record_send_latency, Framing, SharedRx,
};

/// Минимально допустимая версия TLS-протокола (v9.5.0).
///
/// Внутренний enum (вместо `native_tls::Protocol`). Принимаются только
/// TLS 1.2 и TLS 1.3 (1.0/1.1 deprecated NIST SP 800-52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    V1_2,
    V1_3,
}

impl TlsVersion {
    /// Слайс `&'static` для совместимости с rustls API.
    fn as_protocol_versions(&self) -> &'static [&'static rustls::SupportedProtocolVersion] {
        match self {
            TlsVersion::V1_2 => TLS12_AND_13,
            TlsVersion::V1_3 => TLS13_ONLY,
        }
    }
}

/// Дефолтные версии протокола (TLS 1.2 + 1.3) — bound на static,
/// чтобы возвращать `&'static` из `as_protocol_versions`.
static TLS12_AND_13: &[&rustls::SupportedProtocolVersion] =
    &[&rustls::version::TLS12, &rustls::version::TLS13];
static TLS13_ONLY: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

/// Дефолтные версии для построения ClientConfig.
const DEFAULT_PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] = TLS12_AND_13;

/// Поддерживаемые IANA-имена cipher suites (rustls 0.23 + ring).
///
/// Полный список IANA: <https://www.iana.org/assignments/tls-parameters/tls-parameters.xhtml>
/// Реальный список — `rustls::crypto::ring::ALL_CIPHER_SUITES`; здесь мы
/// перечисляем IANA-имена, которые принимает F13-валидация.
const SUPPORTED_CIPHER_SUITES: &[(&str, rustls::SupportedCipherSuite)] = &[
    // TLS 1.3 suites.
    (
        "TLS_AES_256_GCM_SHA384",
        rustls::crypto::ring::cipher_suite::TLS13_AES_256_GCM_SHA384,
    ),
    (
        "TLS_AES_128_GCM_SHA256",
        rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256,
    ),
    (
        "TLS_CHACHA20_POLY1305_SHA256",
        rustls::crypto::ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
    ),
    // TLS 1.2 suites (feature `tls12`).
    (
        "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
        rustls::crypto::ring::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    ),
    (
        "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
        rustls::crypto::ring::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    ),
    (
        "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
        rustls::crypto::ring::cipher_suite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    ),
    (
        "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
        rustls::crypto::ring::cipher_suite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    ),
    (
        "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
        rustls::crypto::ring::cipher_suite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    ),
];

/// Парсинг IANA-имени cipher suite в `rustls::SupportedCipherSuite`.
pub fn parse_cipher_suite(name: &str) -> Result<rustls::SupportedCipherSuite, String> {
    let name = name.trim();
    for (iana, suite) in SUPPORTED_CIPHER_SUITES {
        if *iana == name {
            return Ok(*suite);
        }
    }
    Err(format!(
        "неизвестное cipher suite {:?}; поддерживаемые: {}",
        name,
        SUPPORTED_CIPHER_SUITE_NAMES.join(", ")
    ))
}

/// Публичный список IANA-имён всех поддерживаемых cipher suites.
/// Используется F13 валидацией для формирования сообщения об ошибке.
pub const SUPPORTED_CIPHER_SUITE_NAMES: &[&str] = &[
    "TLS_AES_256_GCM_SHA384",
    "TLS_AES_128_GCM_SHA256",
    "TLS_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
    "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
    "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
];

/// N4: параметры TLS-подключения для одного target'а.
///
/// Изменения v9.5.0:
/// - `min_protocol: Option<TlsVersion>` (был `Option<native_tls::Protocol>`).
/// - `cipher_suites: Option<Vec<rustls::SupportedCipherSuite>>` — новое поле.
#[derive(Clone, Debug, Default)]
pub struct TlsParams {
    pub domain: String,
    pub ca_pem: Option<Vec<u8>>,
    pub insecure: bool,
    pub client_cert_pem: Option<Vec<u8>>,
    pub client_key_pem: Option<Vec<u8>>,
    pub min_protocol: Option<TlsVersion>,
    pub cipher_suites: Option<Vec<rustls::SupportedCipherSuite>>,
}

/// Собрать `Arc<rustls::ClientConfig>`. Возвращает конфиг rustls
/// (thread-safe через Arc; коннектор строится per-connect).
pub fn build_tls_connector(params: &TlsParams) -> Result<Arc<rustls::ClientConfig>> {
    // rustls 0.23: убеждаемся, что ring crypto provider установлен (один раз).
    crate::ensure_rustls_provider();

    // 1. Корневые сертификаты.
    let mut root_store = rustls::RootCertStore::empty();
    if let Some(pem) = &params.ca_pem {
        let certs = rustls_pemfile::certs(&mut pem.as_slice())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow!("ошибка парсинга PEM в ca_pem: {e}"))?;
        if certs.is_empty() {
            return Err(anyhow!("ca_pem не содержит валидных сертификатов"));
        }
        for cert in &certs {
            root_store.add(cert.clone())?;
        }
    } else {
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    // 2. Verifier.
    let verifier: Arc<dyn ServerCertVerifier> = if params.insecure {
        Arc::new(NoCertVerifier)
    } else {
        rustls::client::WebPkiServerVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| anyhow!("WebPkiServerVerifier::build: {e}"))?
    };

    // 3. Crypto provider: дефолтный или с кастомными cipher_suites.
    //    В rustls 0.23 cipher_suites идут через `CryptoProvider`, не через
    //    метод builder'а — поэтому пересобираем provider при кастомных suites.
    let provider = Arc::new(match &params.cipher_suites {
        Some(suites) => rustls::crypto::CryptoProvider {
            cipher_suites: suites.clone(),
            ..rustls::crypto::ring::default_provider()
        },
        None => rustls::crypto::ring::default_provider(),
    });

    // 4. ClientConfig: builder_with_provider → with_protocol_versions → verifier → no_client_auth.
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(
            params
                .min_protocol
                .map(|v| v.as_protocol_versions())
                .unwrap_or(DEFAULT_PROTOCOL_VERSIONS),
        )
        .map_err(|e| anyhow!("with_protocol_versions: {e}"))?;

    // State machine rustls 0.23 (client):
    //   builder_with_provider(provider) → WantsVersions
    //   .with_protocol_versions(...) → WantsVerifier
    //   .dangerous().with_custom_certificate_verifier(verifier) → WantsClientCert
    //   .with_client_auth_cert(...) / .with_no_client_auth() → ClientConfig
    //
    // Т.е. verifier ставится ДО client_auth.
    let wants_client_cert = builder
        .dangerous()
        .with_custom_certificate_verifier(verifier);

    let config = match (&params.client_cert_pem, &params.client_key_pem) {
        (Some(cert_pem), Some(key_pem)) => {
            let certs: Vec<_> = rustls_pemfile::certs(&mut cert_pem.as_slice())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow!("ошибка парсинга PEM в client_cert_pem: {e}"))?;
            if certs.is_empty() {
                return Err(anyhow!("client_cert_pem не содержит сертификатов"));
            }
            let mut keys: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut key_pem.as_slice())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow!("ошибка парсинга PKCS#8 в client_key_pem: {e}"))?;
            if keys.is_empty() {
                return Err(anyhow!("client_key_pem не содержит ключей"));
            }
            let key = PrivateKeyDer::Pkcs8(keys.remove(0));
            wants_client_cert.with_client_auth_cert(certs, key)?
        }
        _ => wants_client_cert.with_no_client_auth(),
    };

    Ok(Arc::new(config))
}

/// NoCertVerifier: принимает любой сертификат (для `tls_insecure=true`).
#[derive(Debug)]
struct NoCertVerifier;

impl ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        use rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA1,
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
        ]
    }
}

/// Парсит "1.2" или "1.3" в `TlsVersion`. Иные значения → Err.
pub fn parse_tls_min_version(s: &str) -> Result<TlsVersion, String> {
    match s.trim() {
        "1.2" => Ok(TlsVersion::V1_2),
        "1.3" => Ok(TlsVersion::V1_3),
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
    let config = match build_tls_connector(&tls_params) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("TLS ({addr}): не удалось построить connector: {e}");
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    let connector = TlsConnector::from(config);
    let mut tls = match tls_connect(&connector, &addr, &domain).await {
        Some(t) => t,
        None => {
            record_error(&metrics, &addr).await;
            drain_as_errors(&rx, &metrics, &addr).await;
            return Ok(());
        }
    };
    let mut buf = BytesMut::with_capacity(8 * 1024);
    while let Some(msg) = next_msg(&rx).await {
        frame_into(&mut buf, &msg, framing);
        let t0 = std::time::Instant::now();
        if tls.write_all(&buf).await.is_err() {
            record_error(&metrics, &addr).await;
            buf.clear();
            record_reconnect(&metrics, "tls", &addr);
            match tls_connect(&connector, &addr, &domain).await {
                Some(new_tls) => {
                    tls = new_tls;
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
        buf.clear();
    }
    Ok(())
}

pub(crate) async fn tls_connect(
    connector: &TlsConnector,
    addr: &str,
    domain: &str,
) -> Option<TlsStream<TcpStream>> {
    let stream = TcpStream::connect(addr).await.ok()?;
    let server_name = ServerName::try_from(domain.to_string()).ok()?;
    connector.connect(server_name, stream).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tls_version_accepts_valid() {
        assert_eq!(parse_tls_min_version("1.2").unwrap(), TlsVersion::V1_2);
        assert_eq!(parse_tls_min_version("1.3").unwrap(), TlsVersion::V1_3);
        assert_eq!(parse_tls_min_version("  1.3  ").unwrap(), TlsVersion::V1_3);
    }

    #[test]
    fn parse_tls_version_rejects_invalid() {
        assert!(parse_tls_min_version("1.0").is_err());
        assert!(parse_tls_min_version("1.1").is_err());
        assert!(parse_tls_min_version("2.0").is_err());
        assert!(parse_tls_min_version("").is_err());
        assert!(parse_tls_min_version("tlsv12").is_err());
    }

    #[test]
    fn parse_cipher_suite_accepts_known() {
        assert_eq!(
            parse_cipher_suite("TLS_AES_256_GCM_SHA384").unwrap(),
            rustls::crypto::ring::cipher_suite::TLS13_AES_256_GCM_SHA384,
        );
        assert!(parse_cipher_suite("TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384").is_ok());
    }

    #[test]
    fn parse_cipher_suite_rejects_unknown() {
        let err = parse_cipher_suite("TLS_BOGUS_SUITE").unwrap_err();
        assert!(err.contains("TLS_BOGUS_SUITE"));
        assert!(err.contains("поддерживаемые:"));
    }

    #[test]
    fn tls_params_default_has_no_cipher_suites() {
        let p = TlsParams::default();
        assert!(p.cipher_suites.is_none());
        assert!(p.min_protocol.is_none());
        assert!(!p.insecure);
    }

    #[test]
    fn build_connector_with_default_roots_succeeds() {
        let p = TlsParams {
            domain: "example.com".into(),
            ..Default::default()
        };
        let cfg = build_tls_connector(&p).expect("default connector");
        let _cfg2 = Arc::clone(&cfg);
    }

    #[test]
    fn build_connector_with_empty_ca_pem_fails() {
        let p = TlsParams {
            domain: "example.com".into(),
            ca_pem: Some(b"NOT A PEM".to_vec()),
            ..Default::default()
        };
        assert!(build_tls_connector(&p).is_err());
    }

    #[test]
    fn build_connector_with_insecure_succeeds() {
        let p = TlsParams {
            domain: "example.com".into(),
            insecure: true,
            ..Default::default()
        };
        let _cfg = build_tls_connector(&p).expect("insecure");
    }

    #[test]
    fn build_connector_with_custom_cipher_suites_succeeds() {
        let p = TlsParams {
            domain: "example.com".into(),
            cipher_suites: Some(vec![
                rustls::crypto::ring::cipher_suite::TLS13_AES_256_GCM_SHA384,
            ]),
            ..Default::default()
        };
        let _cfg = build_tls_connector(&p).expect("custom suites");
    }

    #[test]
    fn build_connector_with_mtls_bad_cert_fails() {
        let p = TlsParams {
            domain: "example.com".into(),
            client_cert_pem: Some(b"NOT A CERT".to_vec()),
            client_key_pem: Some(b"NOT A KEY".to_vec()),
            ..Default::default()
        };
        assert!(build_tls_connector(&p).is_err());
    }
}
