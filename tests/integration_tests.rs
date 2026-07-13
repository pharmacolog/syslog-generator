use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use syslog_generator::{
    apply_overrides, apply_protobuf_schema, create_dispatcher, create_metrics, gather_metrics,
    generate_message, parse_target, render_template, run_profile, serialize_protobuf,
    validate_profile, Overrides, Phase, Profile, ProtobufSchemaFieldMap, ShutdownConfig,
    TargetConfig, ValidationError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// Self-signed сертификат + ключ для TLS-тестов, сгенерированные через
/// системный `openssl req` с явным config-файлом. Кэшируется на процесс —
/// все TLS-тесты переиспользуют один и тот же сертификат (он не привязан
/// к порту).
///
/// Почему `openssl req`, а не `rcgen`:
/// rcgen 0.13 на некоторых окружениях (macOS, OpenSSL 3.x) генерирует
/// PEM-блоки, которые `native_tls::Identity::from_pkcs8` не может
/// распарсить (ошибка "Unknown format in import"). `openssl req -x509`
/// с правильным конфигом даёт стабильный результат, который работает с
/// native-tls на всех поддерживаемых платформах.
///
/// Почему config-файл, а не `-addext`:
/// `openssl req` без конфига создаёт сертификат с `basicConstraints=CA:TRUE`,
/// что не подходит для leaf-сервера (Security.framework на macOS и
/// rustls/Go TLS на Linux отказываются принимать такой сертификат при
/// handshake с ошибкой "connection closed via error"). Нужен leaf-сертификат
/// с `CA:FALSE`, `keyUsage=digitalSignature,keyEncipherment`,
/// `extendedKeyUsage=serverAuth` и SAN (DNS:localhost + IP:127.0.0.1).
///
/// Почему `-days 365`, а не дольше:
/// Security.framework на macOS отклоняет сертификаты с validity period
/// больше 825 дней (типичный лимит для TLS-сертификатов; код ошибки
/// `-67901` — "The validity period in the certificate exceeds the maximum
/// allowed"). 365 дней — с запасом, чтобы тесты не "протухали" между
/// коммитами.
const OPENSSL_SERVER_CNF: &str = r#"
[req]
distinguished_name = req_dn
x509_extensions = v3_server
prompt = no

[req_dn]
CN = localhost

[v3_server]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
IP.1 = 127.0.0.1
"#;

struct TestTlsMaterial {
    cert_pem: Vec<u8>,
    key_pem: Vec<u8>,
    /// Путь к PEM-файлу сертификата, который клиент читает через `tls_ca_file`.
    cert_path: PathBuf,
}

fn openssl_self_signed() -> &'static TestTlsMaterial {
    static CACHE: OnceLock<TestTlsMaterial> = OnceLock::new();
    CACHE.get_or_init(|| {
        // Генерируем в фиксированный путь в target/, чтобы он не зависел
        // от CWD (важно для cargo test, который может запускаться из разных мест).
        let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
        let dir = PathBuf::from(&target_dir).join("test-tls");
        fs::create_dir_all(&dir).expect("create test-tls dir");
        let cnf_path = dir.join("openssl-server.cnf");
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        fs::write(&cnf_path, OPENSSL_SERVER_CNF).expect("write openssl config");
        let status = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-keyout",
                key_path.to_str().unwrap(),
                "-out",
                cert_path.to_str().unwrap(),
                "-days",
                "365",
                "-nodes",
                "-config",
                cnf_path.to_str().unwrap(),
            ])
            .status()
            .expect("не удалось запустить openssl (проверьте, что openssl в PATH)");
        assert!(
            status.success(),
            "openssl req завершился с ошибкой: {status:?}"
        );
        let cert_pem = fs::read(&cert_path).expect("read cert.pem");
        let key_pem = fs::read(&key_path).expect("read key.pem");
        TestTlsMaterial {
            cert_pem,
            key_pem,
            cert_path,
        }
    })
}

#[tokio::test]
async fn test_metrics_presence() {
    // Prometheus omits label-less CounterVec series until at least one label
    // set is observed, so we exercise a minimal file profile first and then
    // assert that the full set of metrics is exported.
    let file_path = "metrics-presence.log";
    let _ = fs::remove_file(file_path);
    let metrics = create_metrics().expect("create_metrics ok in test");

    // Histograms and scalar counters are always exported, even at zero.
    let empty = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(empty.contains("syslog_shutdowns_total"));
    assert!(empty.contains("syslog_drain_duration_seconds"));
    assert!(empty.contains("syslog_generate_duration_seconds"));

    let profile = make_profile(
        vec![TargetConfig {
            address: file_path.into(),
            transport: "file".into(),
            ..Default::default()
        }],
        "round-robin",
        1,
        "metrics",
    );
    run_profile(&profile, metrics.clone()).await.unwrap();

    let s = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(s.contains("syslog_messages_total"));
    assert!(s.contains("syslog_bytes_total"));
    assert!(s.contains("syslog_messages_by_sink_total"));
    let _ = fs::remove_file(file_path);
}

#[test]
fn test_template_render() {
    let mut values = HashMap::new();
    values.insert("user".to_string(), "alice".to_string());
    values.insert("ip".to_string(), "1.2.3.4".to_string());
    let out = render_template("user={{user}} ip={{ip}}", &values);
    assert_eq!(out, "user=alice ip=1.2.3.4");
}

#[test]
fn test_weighted_dispatcher() {
    let targets = vec![
        TargetConfig {
            address: "a".into(),
            transport: "file".into(),
            ..Default::default()
        },
        TargetConfig {
            address: "b".into(),
            transport: "file".into(),
            weight: 3,
            ..Default::default()
        },
    ];
    let seq = create_dispatcher(&targets, "weighted");
    assert_eq!(seq, vec![0, 1, 1, 1]);
}

#[test]
fn test_protobuf_mapping() {
    let mut values = HashMap::new();
    values.insert("real_app".to_string(), "authsvc".to_string());
    let mut fields = HashMap::new();
    fields.insert("app_name".to_string(), "{{real_app}}".to_string());
    let map = ProtobufSchemaFieldMap { fields };
    let out = apply_protobuf_schema(Some(&map), &values);
    assert_eq!(out.get("app_name").unwrap(), "authsvc");
}

#[test]
fn test_generate_message_from_template() {
    let phase = Phase {
        name: "test".into(),
        templates: vec!["app={{real_app}} seq={{sequence}}".into()],
        ..Default::default()
    };
    let msg = generate_message(&phase, 7).unwrap();
    let s = String::from_utf8(msg).unwrap();
    assert!(s.contains("app=test"));
    assert!(s.contains("seq=7"));
}

async fn spawn_tcp_collector(expected: usize) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = tokio::spawn(async move {
        let mut all = Vec::new();
        while all.len() < expected {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let s = String::from_utf8_lossy(&buf[..n]).to_string();
            all.extend(
                s.lines()
                    .map(|x| x.to_string())
                    .filter(|x| !x.trim().is_empty()),
            );
        }
        all
    });
    (addr, handle)
}

async fn spawn_udp_collector(expected: usize) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap().to_string();
    let handle = tokio::spawn(async move {
        let mut all = Vec::new();
        while all.len() < expected {
            let mut buf = vec![0u8; 1024];
            let (n, _) = sock.recv_from(&mut buf).await.unwrap();
            let s = String::from_utf8_lossy(&buf[..n]).to_string();
            all.extend(
                s.lines()
                    .map(|x| x.to_string())
                    .filter(|x| !x.trim().is_empty()),
            );
        }
        all
    });
    (addr, handle)
}

/// Поднимает TLS-коллектор с self-signed сертификатом для "localhost" и
/// возвращает (addr, ca_pem_path, handle). CA-файл нужен тестам, чтобы
/// проверять БЕЗОПАСНЫЙ путь N4 (клиент доверяет именно этому CA), а не
/// отключать проверку. Файл создаётся синхронно до старта сервера.
///
/// Сертификат генерируется через системный `openssl req` (см.
/// [`openssl_self_signed`]) — это совместимо с native-tls на всех
/// поддерживаемых платформах, в отличие от `rcgen`, чей PEM-формат на
/// некоторых окружениях не парсится `Identity::from_pkcs8`.
async fn spawn_tls_collector(
    expected: usize,
) -> (String, String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let tls = openssl_self_signed();
    // Копируем сертификат в target/test-tls/ca-<pid>-<addr>.pem чтобы
    // совпадало с предыдущим контрактом (тесты могут удалить файл после прогона).
    let ca_path = tls.cert_path.with_file_name(format!(
        "ca-{}-{}.pem",
        std::process::id(),
        addr.replace([':', '.'], "_")
    ));
    fs::write(&ca_path, &tls.cert_pem).expect("write ca_path copy");
    let cert_pem = tls.cert_pem.clone();
    let key_pem = tls.key_pem.clone();
    let handle = tokio::spawn(async move {
        use native_tls::Identity;
        use tokio_native_tls::TlsAcceptor;
        let id = Identity::from_pkcs8(&cert_pem, &key_pem)
            .expect("Identity::from_pkcs8 должен принять openssl-generated PEM");
        let acceptor = native_tls::TlsAcceptor::builder(id).build().unwrap();
        let acceptor = TlsAcceptor::from(acceptor);
        let mut all = Vec::new();
        while all.len() < expected {
            let (stream, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(stream).await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = tls.read(&mut buf).await.unwrap();
            let s = String::from_utf8_lossy(&buf[..n]).to_string();
            all.extend(
                s.lines()
                    .map(|x| x.to_string())
                    .filter(|x| !x.trim().is_empty()),
            );
        }
        all
    });
    (addr, ca_path.to_string_lossy().into_owned(), handle)
}

// `count` задаёт точное число сообщений через total_messages (детерминированно).
// messages_per_second = 0 — без ограничения скорости, чтобы тесты не замедлялись rate-limiter’ом.
fn make_profile(targets: Vec<TargetConfig>, distribution: &str, count: u64, name: &str) -> Profile {
    Profile {
        targets,
        distribution: distribution.into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: name.into(),
            messages_per_second: 0,
            total_messages: Some(count),
            templates: vec![format!("{} {{{{sequence}}}}", name)],
            ..Default::default()
        }],
        metrics_addr: None,
    }
}

#[tokio::test]
async fn test_mixed_multi_target_broadcast_end_to_end() {
    let file_path = "mixed-broadcast.log";
    let _ = fs::remove_file(file_path);
    let (tcp_addr, tcp_server) = spawn_tcp_collector(1).await;
    let (udp_addr, udp_server) = spawn_udp_collector(1).await;
    let (tls_addr, tls_ca, tls_server) = spawn_tls_collector(1).await;
    let profile = make_profile(
        vec![
            TargetConfig {
                address: file_path.into(),
                transport: "file".into(),
                ..Default::default()
            },
            TargetConfig {
                address: tcp_addr,
                transport: "tcp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: udp_addr,
                transport: "udp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: tls_addr,
                transport: "tls".into(),
                tls_domain: Some("localhost".into()),
                tls_ca_file: Some(tls_ca.clone()),
                ..Default::default()
            },
        ],
        "broadcast",
        1,
        "broadcast",
    );
    run_profile(
        &profile,
        create_metrics().expect("create_metrics ok in test"),
    )
    .await
    .unwrap();
    let file_content = fs::read_to_string(file_path).unwrap();
    let tcp = tcp_server.await.unwrap();
    let udp = udp_server.await.unwrap();
    let tls = tls_server.await.unwrap();
    assert!(file_content.contains("broadcast 1"));
    assert!(tcp.iter().any(|m| m.contains("broadcast 1")));
    assert!(udp.iter().any(|m| m.contains("broadcast 1")));
    assert!(tls.iter().any(|m| m.contains("broadcast 1")));
    let _ = fs::remove_file(file_path);
    let _ = fs::remove_file(&tls_ca);
}

#[tokio::test]
async fn test_mixed_multi_target_round_robin_end_to_end() {
    let file_path = "mixed-rr.log";
    let _ = fs::remove_file(file_path);
    let (tcp_addr, tcp_server) = spawn_tcp_collector(1).await;
    let (udp_addr, udp_server) = spawn_udp_collector(1).await;
    let (tls_addr, tls_ca, tls_server) = spawn_tls_collector(1).await;
    let profile = make_profile(
        vec![
            TargetConfig {
                address: file_path.into(),
                transport: "file".into(),
                ..Default::default()
            },
            TargetConfig {
                address: tcp_addr,
                transport: "tcp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: udp_addr,
                transport: "udp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: tls_addr,
                transport: "tls".into(),
                tls_domain: Some("localhost".into()),
                tls_ca_file: Some(tls_ca.clone()),
                ..Default::default()
            },
        ],
        "round-robin",
        4,
        "rr",
    );
    run_profile(
        &profile,
        create_metrics().expect("create_metrics ok in test"),
    )
    .await
    .unwrap();
    let file_content = fs::read_to_string(file_path).unwrap();
    let tcp = tcp_server.await.unwrap();
    let udp = udp_server.await.unwrap();
    let tls = tls_server.await.unwrap();
    assert!(file_content.contains("rr 1"));
    assert!(tcp.iter().any(|m| m.contains("rr 2")));
    assert!(udp.iter().any(|m| m.contains("rr 3")));
    assert!(tls.iter().any(|m| m.contains("rr 4")));
    let _ = fs::remove_file(file_path);
    let _ = fs::remove_file(&tls_ca);
}

#[tokio::test]
async fn test_mixed_multi_target_weighted_end_to_end() {
    let file_path = "mixed-weighted.log";
    let _ = fs::remove_file(file_path);
    let (tcp_addr, tcp_server) = spawn_tcp_collector(1).await;
    let (udp_addr, udp_server) = spawn_udp_collector(2).await;
    let (tls_addr, tls_ca, tls_server) = spawn_tls_collector(1).await;
    let profile = make_profile(
        vec![
            TargetConfig {
                address: file_path.into(),
                transport: "file".into(),
                ..Default::default()
            },
            TargetConfig {
                address: tcp_addr,
                transport: "tcp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: udp_addr,
                transport: "udp".into(),
                weight: 2,
                ..Default::default()
            },
            TargetConfig {
                address: tls_addr,
                transport: "tls".into(),
                tls_domain: Some("localhost".into()),
                tls_ca_file: Some(tls_ca.clone()),
                ..Default::default()
            },
        ],
        "weighted",
        5,
        "weighted",
    );
    run_profile(
        &profile,
        create_metrics().expect("create_metrics ok in test"),
    )
    .await
    .unwrap();
    let file_content = fs::read_to_string(file_path).unwrap();
    let tcp = tcp_server.await.unwrap();
    let udp = udp_server.await.unwrap();
    let tls = tls_server.await.unwrap();
    assert!(file_content.contains("weighted 1"));
    assert!(tcp.iter().any(|m| m.contains("weighted 2")));
    assert!(udp.iter().any(|m| m.contains("weighted 3")));
    assert!(udp.iter().any(|m| m.contains("weighted 4")));
    assert!(tls.iter().any(|m| m.contains("weighted 5")));
    let _ = fs::remove_file(file_path);
    let _ = fs::remove_file(&tls_ca);
}

#[tokio::test]
async fn test_total_messages_removes_cap_above_100() {
    // Раньше генерация была жёстко ограничена 100 сообщениями. Проверяем, что потолок снят.
    let file_path = "cap-removed.log";
    let _ = fs::remove_file(file_path);
    let profile = make_profile(
        vec![TargetConfig {
            address: file_path.into(),
            transport: "file".into(),
            ..Default::default()
        }],
        "round-robin",
        250,
        "cap",
    );
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let content = fs::read_to_string(file_path).unwrap();
    let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(lines, 250, "expected 250 messages, got {}", lines);
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(out.contains("syslog_messages_generated_total"));
    let _ = fs::remove_file(file_path);
}

#[tokio::test]
async fn test_rate_limiting_respects_target() {
    // При messages_per_second=50 и duration=1с фактическое число должно быть около 50,
    // а не десятки тысяч (как было бы без rate-limiter). Допускаем запас на burst/планировщик.
    let file_path = "rate-limited.log";
    let _ = fs::remove_file(file_path);
    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.into(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "rate".into(),
            messages_per_second: 50,
            duration_secs: 1,
            templates: vec!["rate {{sequence}}".into()],
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let content = fs::read_to_string(file_path).unwrap();
    let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
    // Токен-бакет governor даёт начальный burst (до ~rate) + пополнение за 1с.
    assert!(
        (40..=110).contains(&lines),
        "rate-limited count out of range: {}",
        lines
    );
    let _ = fs::remove_file(file_path);
}

#[tokio::test]
async fn test_load_shape_linear_ramp_volume() {
    // Linear ramp 20 -> 220 msg/s за 2с. Средняя интенсивность ~120 msg/s,
    // ожидаем ~240 сообщений. Проверяем, что объём соответствует площади под
    // кривой, а не постоянному start_rate (было бы ~40) и не end_rate (~440).
    use syslog_generator::LoadShape;
    let file_path = "loadshape-linear.log";
    let _ = fs::remove_file(file_path);
    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.into(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "ramp".into(),
            duration_secs: 2,
            format: Some("raw".into()),
            templates: vec!["ramp {{sequence}}".into()],
            load_shape: Some(LoadShape::Linear {
                start_rate: 20.0,
                end_rate: 220.0,
            }),
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let content = fs::read_to_string(file_path).unwrap();
    let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
    // Площадь под линейной кривой за 2с = (20+220)/2 * 2 = 240. Широкий допуск
    // на накладные расходы планировщика/sleep.
    assert!(
        (150..=340).contains(&lines),
        "linear ramp volume out of range: {}",
        lines
    );
    let _ = fs::remove_file(file_path);
}

#[tokio::test]
async fn test_load_shape_burst_exceeds_base() {
    // Burst: база 10 msg/s, всплеск 500 msg/s каждую 1с длительностью 0.3с.
    // За 2с всплески дают заметно больше сообщений, чем чистая база (~20).
    use syslog_generator::LoadShape;
    let file_path = "loadshape-burst.log";
    let _ = fs::remove_file(file_path);
    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.into(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "burst".into(),
            duration_secs: 2,
            format: Some("raw".into()),
            templates: vec!["burst {{sequence}}".into()],
            load_shape: Some(LoadShape::Burst {
                base_rate: 10.0,
                burst_rate: 500.0,
                every_secs: 1.0,
                burst_secs: 0.3,
            }),
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let content = fs::read_to_string(file_path).unwrap();
    let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
    // Ожидаемо: 2 окна всплеска по ~0.3с * 500 = ~300 + база ~14 => ~300+.
    // Существенно больше, чем ровная база (~20). Нижнюю границу держим консервативно.
    assert!(lines > 60, "burst volume too low: {}", lines);
    let _ = fs::remove_file(file_path);
}

// Коллектор, считающий число принятых TCP-соединений и собирающий строки,
// пока не наберётся `expected_msgs` сообщений.
async fn spawn_tcp_conn_counter(
    expected_msgs: usize,
) -> (String, tokio::task::JoinHandle<(usize, Vec<String>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = tokio::spawn(async move {
        use std::sync::{Arc, Mutex};
        let conns = Arc::new(Mutex::new(0usize));
        let msgs = Arc::new(Mutex::new(Vec::<String>::new()));
        loop {
            {
                let m = msgs.lock().unwrap();
                if m.len() >= expected_msgs {
                    break;
                }
            }
            let accept = listener.accept();
            let (mut stream, _) =
                match tokio::time::timeout(std::time::Duration::from_secs(5), accept).await {
                    Ok(Ok(s)) => s,
                    _ => break,
                };
            *conns.lock().unwrap() += 1;
            let msgs = msgs.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let s = String::from_utf8_lossy(&buf[..n]).to_string();
                            let mut guard = msgs.lock().unwrap();
                            guard.extend(
                                s.lines()
                                    .map(|x| x.to_string())
                                    .filter(|x| !x.trim().is_empty()),
                            );
                        }
                    }
                }
            });
        }
        let c = *conns.lock().unwrap();
        let m = msgs.lock().unwrap().clone();
        (c, m)
    });
    (addr, handle)
}

#[tokio::test]
async fn test_connection_pool_opens_multiple_connections() {
    // connections=3 на TCP-target должны открыть 3 отдельных соединения (пул воркеров).
    let (tcp_addr, server) = spawn_tcp_conn_counter(30).await;
    let profile = Profile {
        targets: vec![TargetConfig {
            address: tcp_addr,
            transport: "tcp".into(),
            connections: 3,
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "pool".into(),
            messages_per_second: 0,
            total_messages: Some(30),
            templates: vec!["pool {{sequence}}".into()],
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let (conns, msgs) = server.await.unwrap();
    assert_eq!(conns, 3, "expected 3 pooled connections, got {}", conns);
    assert_eq!(
        msgs.len(),
        30,
        "expected 30 messages delivered, got {}",
        msgs.len()
    );
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(out.contains("syslog_active_workers"));
    assert!(
        out.contains("syslog_active_workers 3"),
        "active_workers gauge should equal 3"
    );
}

// ---- Веха B: валидный syslog (RFC 5424 / RFC 3164 + framing) ----

fn phase_with_format(fmt: &str, syslog: syslog_generator::SyslogConfig, template: &str) -> Phase {
    Phase {
        name: "b".into(),
        messages_per_second: 0,
        total_messages: Some(1),
        templates: vec![template.into()],
        format: Some(fmt.into()),
        syslog,
        ..Default::default()
    }
}

#[test]
fn test_rfc5424_message_is_valid() {
    use syslog_generator::SyslogConfig;
    let syslog = SyslogConfig {
        facility: 4,
        severity: 2, // PRI = 34
        hostname: "myhost".into(),
        app_name: "myapp".into(),
        procid: "999".into(),
        msgid: "MSG01".into(),
        structured_data: "[ex@32473 k=\"v\"]".into(),
        bom: false,
    };
    let phase = phase_with_format("rfc5424", syslog, "user login {{sequence}}");
    let bytes = generate_message(&phase, 7).unwrap();
    let msg = String::from_utf8(bytes).unwrap();
    // <PRI>VERSION SP TIMESTAMP SP HOSTNAME SP APP-NAME SP PROCID SP MSGID SP SD SP MSG
    assert!(msg.starts_with("<34>1 "), "bad PRI/VERSION: {}", msg);
    let parts: Vec<&str> = msg.splitn(8, ' ').collect();
    assert_eq!(parts[0], "<34>1");
    // TIMESTAMP RFC3339 с миллисекундами и Z.
    assert!(
        parts[1].ends_with('Z') && parts[1].contains('T') && parts[1].contains('.'),
        "bad TIMESTAMP: {}",
        parts[1]
    );
    assert_eq!(parts[2], "myhost");
    assert_eq!(parts[3], "myapp");
    assert_eq!(parts[4], "999");
    assert_eq!(parts[5], "MSG01");
    assert_eq!(parts[6], "[ex@32473"); // SD разбит по пробелу внутри
    assert!(
        parts[7].starts_with("k=\"v\"] user login 7"),
        "bad SD/MSG: {}",
        parts[7]
    );
}

#[test]
fn test_rfc5424_nilvalues_and_pri_default() {
    use syslog_generator::SyslogConfig;
    // Дефолтный SyslogConfig: facility=1 severity=6 → PRI=14, procid/msgid/sd = NILVALUE.
    let phase = phase_with_format("rfc5424", SyslogConfig::default(), "hello");
    let msg = String::from_utf8(generate_message(&phase, 1).unwrap()).unwrap();
    assert!(msg.starts_with("<14>1 "));
    assert!(
        msg.contains(" - - - hello") || msg.ends_with(" - - - hello"),
        "expected NILVALUEs for procid/msgid/sd: {}",
        msg
    );
}

#[test]
fn test_rfc5424_bom_prefixes_msg() {
    use syslog_generator::SyslogConfig;
    let sc = SyslogConfig {
        bom: true,
        ..Default::default()
    };
    let phase = phase_with_format("rfc5424", sc, "payload");
    let bytes = generate_message(&phase, 1).unwrap();
    // BOM (EF BB BF) должен стоять непосредственно перед MSG.
    let needle = [0xEF, 0xBB, 0xBF, b'p', b'a', b'y'];
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        "BOM not found before MSG"
    );
}

#[test]
fn test_rfc3164_message_is_valid() {
    use syslog_generator::SyslogConfig;
    let sc = SyslogConfig {
        facility: 1,
        severity: 6, // PRI = 14
        hostname: "srv1".into(),
        app_name: "sshd".into(),
        procid: "1234".into(),
        msgid: "-".into(),
        structured_data: "-".into(),
        bom: false,
    };
    let phase = phase_with_format("rfc3164", sc, "session opened");
    let msg = String::from_utf8(generate_message(&phase, 1).unwrap()).unwrap();
    assert!(msg.starts_with("<14>"), "bad PRI: {}", msg);
    // <PRI>Mmm dd hh:mm:ss srv1 sshd[1234]: session opened
    assert!(
        msg.contains(" srv1 sshd[1234]: session opened"),
        "bad RFC3164 body: {}",
        msg
    );
}

#[test]
fn test_raw_format_no_wrapping() {
    use syslog_generator::SyslogConfig;
    // format="raw" — обратная совместимость: без syslog-обёртки.
    let phase = phase_with_format("raw", SyslogConfig::default(), "just text {{sequence}}");
    let msg = String::from_utf8(generate_message(&phase, 5).unwrap()).unwrap();
    assert_eq!(msg, "just text 5");
}

#[tokio::test]
async fn test_octet_counting_framing_over_tcp() {
    // Коллектор, читающий сырые байты и разбирающий octet-counting.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = tokio::spawn(async move {
        let (mut stream, _) =
            tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
        let mut buf = Vec::new();
        let mut tmp = vec![0u8; 4096];
        // Читаем, пока есть данные (до закрытия отправителем).
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(800), stream.read(&mut tmp))
                .await
            {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
                Ok(Err(_)) => break,
            }
        }
        buf
    });
    let profile = Profile {
        targets: vec![TargetConfig {
            address: addr,
            transport: "tcp".into(),
            framing: "octet-counting".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![phase_with_format(
            "rfc5424",
            syslog_generator::SyslogConfig::default(),
            "octet test",
        )],
        metrics_addr: None,
    };
    run_profile(
        &profile,
        create_metrics().expect("create_metrics ok in test"),
    )
    .await
    .unwrap();
    let raw = server.await.unwrap();
    let s = String::from_utf8(raw).unwrap();
    // Формат: `<N> SP SYSLOG-MSG`, где первое поле — длина в октетах.
    let sp = s.find(' ').expect("no MSG-LEN prefix");
    let msg_len: usize = s[..sp].parse().expect("MSG-LEN not a number");
    let payload = &s[sp + 1..];
    assert_eq!(
        payload.len(),
        msg_len,
        "octet count mismatch: declared {}, actual {}",
        msg_len,
        payload.len()
    );
    assert!(
        payload.starts_with("<14>1 "),
        "framed payload is not RFC5424: {}",
        payload
    );
    assert!(
        payload.ends_with("octet test"),
        "unexpected MSG: {}",
        payload
    );
}

#[tokio::test]
async fn test_negative_paths_connection_failures_record_errors() {
    let file_path = "negative-ok.log";
    let _ = fs::remove_file(file_path);
    let profile = make_profile(
        vec![
            TargetConfig {
                address: file_path.into(),
                transport: "file".into(),
                ..Default::default()
            },
            TargetConfig {
                address: "127.0.0.1:9".into(),
                transport: "tcp".into(),
                ..Default::default()
            },
            TargetConfig {
                address: "127.0.0.1:9".into(),
                transport: "tls".into(),
                ..Default::default()
            },
        ],
        "broadcast",
        1,
        "neg",
    );
    let metrics = create_metrics().expect("create_metrics ok in test");
    let _ = run_profile(&profile, metrics.clone()).await;
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(fs::read_to_string(file_path).unwrap().contains("neg 1"));
    assert!(out.contains("syslog_errors_total"));
    let _ = fs::remove_file(file_path);
}

// ===================== Веха C: вариативный пейлоад (F4–F6, F14) =====================

fn write_tmp(name: &str, content: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("sg_{}_{}.json", name, nanos));
    fs::write(&path, content).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn test_f4_seed_reproducibility() {
    // Одинаковый seed → одинаковая последовательность сообщений (детерминизм).
    let phase = Phase {
        name: "det".into(),
        seed: Some(12345),
        format: Some("raw".into()),
        templates: vec!["{{faker.ipv4}}|{{faker.uuid}}|{{faker.username}}".into()],
        ..Default::default()
    };
    let a: Vec<String> = (0..10)
        .map(|i| String::from_utf8(generate_message(&phase, i).unwrap()).unwrap())
        .collect();
    let b: Vec<String> = (0..10)
        .map(|i| String::from_utf8(generate_message(&phase, i).unwrap()).unwrap())
        .collect();
    assert_eq!(a, b, "тот же seed должен давать тот же вывод");
    // Соседние seq различаются.
    assert_ne!(a[0], a[1]);
}

#[test]
fn test_f4_multifield_schema_deterministic_order() {
    // Регрессия: schema.fields — HashMap, обход должен быть детерминирован по имени,
    // иначе RNG потребляется в разном порядке между запусками. Несколько полей
    // повышают вероятность поймать недетерминированный обход.
    let schema = r#"{
        "template": "a={{a}} b={{b}} c={{c}} d={{d}} e={{e}}",
        "fields": {
            "a": {"type":"int","min":0,"max":1000000},
            "b": {"type":"string","len":8},
            "c": {"type":"faker","faker":"uuid"},
            "d": {"type":"int","min":0,"max":1000000},
            "e": {"type":"faker","faker":"ipv4"}
        }
    }"#;
    let path = write_tmp("multifield", schema);
    let phase = Phase {
        name: "mf".into(),
        seed: Some(555),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    // Многократные вызовы для одного seq обязаны совпадать (стабильный порядок полей).
    let ref_msg = String::from_utf8(generate_message(&phase, 3).unwrap()).unwrap();
    for _ in 0..50 {
        let m = String::from_utf8(generate_message(&phase, 3).unwrap()).unwrap();
        assert_eq!(m, ref_msg, "обход полей schema должен быть детерминирован");
    }
    let _ = fs::remove_file(path);
}

#[test]
fn test_f4_different_seed_differs() {
    let mk = |seed: u64| {
        let p = Phase {
            name: "d".into(),
            seed: Some(seed),
            format: Some("raw".into()),
            templates: vec!["{{faker.uuid}}".into()],
            ..Default::default()
        };
        String::from_utf8(generate_message(&p, 1).unwrap()).unwrap()
    };
    assert_ne!(mk(1), mk(2));
}

#[test]
fn test_f5_faker_tokens_in_default_values() {
    let phase = Phase {
        name: "f".into(),
        seed: Some(7),
        format: Some("raw".into()),
        templates: vec![
            "ip={{faker.ipv4}} mac={{faker.mac}} st={{faker.http_status}} ua={{faker.user_agent}}"
                .into(),
        ],
        ..Default::default()
    };
    let s = String::from_utf8(generate_message(&phase, 1).unwrap()).unwrap();
    assert!(!s.contains("{{"), "остались нерендеренные токены: {s}");
    // ipv4 — 4 октета
    let ip = s.split("ip=").nth(1).unwrap().split(' ').next().unwrap();
    assert_eq!(ip.split('.').count(), 4);
    // mac — 6 групп
    let mac = s.split("mac=").nth(1).unwrap().split(' ').next().unwrap();
    assert_eq!(mac.split(':').count(), 6);
}

#[test]
fn test_f5_schema_int_range_and_string_len() {
    let schema = r#"{
        "template": "n={{n}} s={{s}}",
        "fields": {
            "n": {"type": "int", "min": 100, "max": 105},
            "s": {"type": "string", "len": 12}
        }
    }"#;
    let path = write_tmp("intstr", schema);
    let phase = Phase {
        name: "sc".into(),
        seed: Some(3),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    for i in 0..50 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        let n: i64 = s
            .split("n=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((100..=105).contains(&n), "int вне диапазона: {n}");
        let sv = s.split("s=").nth(1).unwrap();
        assert_eq!(sv.len(), 12, "string len != 12: {sv}");
    }
    let _ = fs::remove_file(path);
}

#[test]
fn test_f6_weighted_enum_distribution() {
    // Вес [0,1,0] → всегда второй вариант.
    let schema = r#"{
        "template": "{{k}}",
        "fields": {"k": {"type":"enum","values":["a","b","c"],"distribution":"weighted","weights":[0,1,0]}}
    }"#;
    let path = write_tmp("wenum", schema);
    let phase = Phase {
        name: "w".into(),
        seed: Some(4),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    for i in 0..30 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        assert_eq!(s, "b");
    }
    let _ = fs::remove_file(path);
}

#[test]
fn test_f6_zipf_enum_hot_key() {
    let schema = r#"{
        "template": "{{k}}",
        "fields": {"k": {"type":"enum","values":["r1","r2","r3","r4","r5"],"distribution":"zipf","zipf_exponent":1.3}}
    }"#;
    let path = write_tmp("zenum", schema);
    let phase = Phase {
        name: "z".into(),
        seed: Some(5),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    let mut c_r1 = 0;
    let mut c_r5 = 0;
    for i in 0..3000 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        if s == "r1" {
            c_r1 += 1;
        }
        if s == "r5" {
            c_r5 += 1;
        }
    }
    assert!(
        c_r1 > c_r5,
        "zipf: горячий ключ r1({c_r1}) должен доминировать над r5({c_r5})"
    );
    let _ = fs::remove_file(path);
}

#[test]
fn test_f6_padding_reaches_size() {
    let phase = Phase {
        name: "p".into(),
        seed: Some(6),
        format: Some("raw".into()),
        templates: vec!["x".into()],
        pad_to_bytes: Some(64),
        ..Default::default()
    };
    let s = generate_message(&phase, 1).unwrap();
    assert_eq!(s.len(), 64, "паддинг должен добить до 64 байт");
    assert!(s.starts_with(b"x "));
}

#[test]
fn test_f14_multi_template_uses_more_than_first() {
    // Три разных шаблона; при вариативном выборе должны встретиться минимум два.
    let phase = Phase {
        name: "mt".into(),
        seed: Some(99),
        format: Some("raw".into()),
        templates: vec!["AAA".into(), "BBB".into(), "CCC".into()],
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    for i in 0..60 {
        seen.insert(String::from_utf8(generate_message(&phase, i).unwrap()).unwrap());
    }
    assert!(
        seen.len() >= 2,
        "мультишаблон должен использовать не только первый: {seen:?}"
    );
}

#[test]
fn test_f14_weighted_template_selection() {
    // Вес [0,1] → всегда второй шаблон.
    let phase = Phase {
        name: "wt".into(),
        seed: Some(11),
        format: Some("raw".into()),
        templates: vec!["FIRST".into(), "SECOND".into()],
        template_weights: Some(vec![0.0, 1.0]),
        ..Default::default()
    };
    for i in 0..30 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        assert_eq!(s, "SECOND");
    }
}

// ---- Веха C, опциональные задачи: regex (F5) и корреляции (F6) ----

#[test]
fn test_f5_regex_field_matches_pattern() {
    // Поле type="regex" должно порождать строку, соответствующую паттерну.
    let schema = r#"{
        "template": "id={{id}}",
        "fields": {
            "id": {"type": "regex", "regex": "[A-Z]{2}[0-9]{6}"}
        }
    }"#;
    let path = write_tmp("regexfield", schema);
    let phase = Phase {
        name: "rx".into(),
        seed: Some(4242),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    let re = regex::Regex::new(r"^id=[A-Z]{2}[0-9]{6}$").unwrap();
    for i in 0..50 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        assert!(re.is_match(&s), "regex-поле не соответствует паттерну: {s}");
    }
    let _ = fs::remove_file(path);
}

#[test]
fn test_f5_regex_deterministic_by_seed() {
    let schema = r#"{
        "template": "{{tok}}",
        "fields": { "tok": {"type": "regex", "regex": "sess-[a-f0-9]{8}"} }
    }"#;
    let path = write_tmp("regexdet", schema);
    let phase = Phase {
        name: "rxd".into(),
        seed: Some(777),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    let a: Vec<_> = (0..20)
        .map(|i| String::from_utf8(generate_message(&phase, i).unwrap()).unwrap())
        .collect();
    let b: Vec<_> = (0..20)
        .map(|i| String::from_utf8(generate_message(&phase, i).unwrap()).unwrap())
        .collect();
    assert_eq!(a, b, "regex должен быть воспроизводим по seed");
    let _ = fs::remove_file(path);
}

#[test]
fn test_f6_cross_field_correlation() {
    // level зависит от status: 200/301 → INFO, 404 → WARN, 500 → ERROR.
    let schema = r#"{
        "template": "status={{status}} level={{level}}",
        "fields": {
            "status": {"type": "enum", "values": ["200","301","404","500"]},
            "level":  {"type": "string", "len": 3, "depends_on": "status",
                        "mapping": {"200":"INFO","301":"INFO","404":"WARN","500":"ERROR"},
                        "mapping_default": "UNKNOWN"}
        }
    }"#;
    let path = write_tmp("corr", schema);
    let phase = Phase {
        name: "cr".into(),
        seed: Some(2024),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    for i in 0..200 {
        let s = String::from_utf8(generate_message(&phase, i).unwrap()).unwrap();
        let status = s
            .split("status=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        let level = s.split("level=").nth(1).unwrap().trim();
        let expected = match status {
            "200" | "301" => "INFO",
            "404" => "WARN",
            "500" => "ERROR",
            _ => "UNKNOWN",
        };
        assert_eq!(
            level, expected,
            "корреляция status={status} → level нарушена: {s}"
        );
    }
    let _ = fs::remove_file(path);
}

#[test]
fn test_f6_correlation_default_when_no_mapping_hit() {
    // Если значения родителя нет в mapping — берётся mapping_default.
    let schema = r#"{
        "template": "a={{a}} b={{b}}",
        "fields": {
            "a": {"type": "enum", "values": ["x"]},
            "b": {"type": "string", "len": 2, "depends_on": "a",
                   "mapping": {"y": "MATCH"}, "mapping_default": "DEFV"}
        }
    }"#;
    let path = write_tmp("corrdef", schema);
    let phase = Phase {
        name: "crd".into(),
        seed: Some(5),
        format: Some("raw".into()),
        schema_file: Some(path.clone()),
        ..Default::default()
    };
    let s = String::from_utf8(generate_message(&phase, 1).unwrap()).unwrap();
    assert!(s.contains("b=DEFV"), "ожидался mapping_default: {s}");
    let _ = fs::remove_file(path);
}

// ---- Веха B, F10: честный protobuf wire-format ----

#[test]
fn test_f10_protobuf_is_valid_wire_format() {
    // Проверяем, что вывод — настоящий protobuf, а не JSON.
    let mut m = ProtobufSchemaFieldMap::default();
    m.fields.insert("1_name".into(), "1:hello".into());
    m.fields.insert("2_count".into(), "2:int:150".into());
    let vals: HashMap<String, String> = HashMap::new();
    let bytes = serialize_protobuf(Some(&m), &vals);
    // Не JSON: не начинается с '{' и не содержит кавычек JSON.
    assert_ne!(bytes.first(), Some(&b'{'));
    // Ручной разбор wire-format: field1 (LEN "hello"), field2 (VARINT 150).
    // tag1 = (1<<3)|2 = 0x0A, len=5, "hello", tag2 = (2<<3)|0 = 0x10, 0x96 0x01.
    assert_eq!(
        bytes,
        vec![0x0A, 0x05, b'h', b'e', b'l', b'l', b'o', 0x10, 0x96, 0x01]
    );
}

// ---- Веха A, N3: метрики латентности/размера/реконнектов ----

#[tokio::test]
async fn test_n3_metrics_size_and_latency_exported() {
    let out_path = std::env::temp_dir().join(format!(
        "sg_n3_{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let profile = Profile {
        targets: vec![TargetConfig {
            address: out_path.to_string_lossy().to_string(),
            transport: "file".into(),
            ..Default::default()
        }],
        phases: vec![Phase {
            name: "n3".into(),
            total_messages: Some(20),
            messages_per_second: 500,
            seed: Some(1),
            format: Some("raw".into()),
            templates: vec!["payload {{sequence}}".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(
        out.contains("syslog_message_size_bytes"),
        "нет histogram размера сообщений"
    );
    assert!(
        out.contains("syslog_send_duration_seconds"),
        "нет histogram латентности отправки"
    );
    // Гистограммы дают _bucket/_sum/_count — основа для p50/p95/p99.
    assert!(
        out.contains("syslog_send_duration_seconds_bucket"),
        "нет корзин латентности"
    );
    assert!(
        out.contains("syslog_message_size_bytes_count"),
        "нет счётчика размера"
    );
    let _ = fs::remove_file(out_path);
}

/// N2 (v8.6.0): `syslog_messages_by_format_total` инкрементируется в
/// `run_phase_multi` для каждой фазы по её `format_type()`. Проверяем что
/// после прогона mixed-target профиля (raw + rfc5424) обе серии присутствуют.
#[tokio::test]
async fn test_n2_messages_by_format_total_exported() {
    let dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let file_path = dir.join(format!("sg_n2_fmt_{nanos}.log"));
    let _ = fs::remove_file(&file_path);

    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.to_string_lossy().to_string(),
            transport: "file".into(),
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: Default::default(),
        phases: vec![
            Phase {
                name: "raw_phase".into(),
                total_messages: Some(3),
                messages_per_second: 1000,
                seed: Some(1),
                format: Some("raw".into()),
                templates: vec!["raw {{sequence}}".into()],
                ..Default::default()
            },
            Phase {
                name: "rfc_phase".into(),
                total_messages: Some(2),
                messages_per_second: 1000,
                seed: Some(2),
                format: Some("rfc5424".into()),
                templates: vec!["rfc {{sequence}}".into()],
                ..Default::default()
            },
        ],
        metrics_addr: None,
    };

    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics.clone()).await.unwrap();
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");

    // CounterVec без наблюдённых меток не экспортируется до первого inc —
    // здесь инкременты были, значит серии должны быть.
    assert!(
        out.contains("syslog_messages_by_format_total"),
        "нет syslog_messages_by_format_total в выводе, got:\n{out}"
    );
    // raw_phase дал 3 инкремента с label format="raw".
    assert!(
        out.lines().any(|l| {
            l.starts_with("syslog_messages_by_format_total{format=\"raw\"}")
                && l.trim_end().ends_with(" 3")
        }),
        "ожидалась серия format=raw со значением 3, got:\n{out}"
    );
    // rfc_phase дал 2 инкремента с label format="rfc5424".
    assert!(
        out.lines().any(|l| {
            l.starts_with("syslog_messages_by_format_total{format=\"rfc5424\"}")
                && l.trim_end().ends_with(" 2")
        }),
        "ожидалась серия format=rfc5424 со значением 2, got:\n{out}"
    );

    // N2: cpu_usage_percent и memory_usage_bytes удалены.
    assert!(
        !out.contains("syslog_cpu_usage_percent"),
        "cpu_usage_percent должен быть удалён в N2"
    );
    assert!(
        !out.contains("syslog_memory_usage_bytes"),
        "memory_usage_bytes должен быть удалён в N2"
    );

    let _ = fs::remove_file(&file_path);
}

#[tokio::test]
async fn test_n3_reconnect_metric_registered_and_scrapeable() {
    // Счётчик syslog_reconnects_total — это CounterVec с метками; Prometheus
    // не выводит его в текст, пока не наблюдалась хотя бы одна серия
    // (это корректное поведение). Проверяем, что метрика зарегистрирована
    // в реестре и корректно экспортируется после наблюдения серии.
    let metrics = create_metrics().expect("create_metrics ok in test");
    // До наблюдения — серий нет.
    let before = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(
        !before
            .lines()
            .any(|l| l.starts_with("syslog_reconnects_total{")),
        "до наблюдения не должно быть серий reconnects"
    );
    // Наблюдаем серию через публичное поле (так же, как в боевом
    // коде реконнекта через record_reconnect).
    metrics
        .reconnects_total
        .with_label_values(&["tcp", "127.0.0.1:514"])
        .inc();
    let after = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(
        after.contains("syslog_reconnects_total"),
        "метрика reconnects не экспортируется"
    );
    let series = after
        .lines()
        .find(|l| l.starts_with("syslog_reconnects_total{"))
        .expect("ожидалась серия reconnects после инкремента");
    assert!(
        series.contains("transport=\"tcp\"") && series.contains("127.0.0.1:514"),
        "метки reconnects некорректны: {series}"
    );
    assert!(
        series.trim_end().ends_with("1"),
        "значение счётчика должно быть 1: {series}"
    );
}

#[tokio::test]
async fn test_n3_dead_tcp_target_records_errors() {
    // Мёртвый TCP-таргет (никто не слушает): подключение провальное,
    // сообщения должны учтёны как ошибки отправки (очередь дренируется).
    let metrics = create_metrics().expect("create_metrics ok in test");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener); // порт свободен, никто не слушает
    let profile = Profile {
        targets: vec![TargetConfig {
            address: addr,
            transport: "tcp".into(),
            ..Default::default()
        }],
        phases: vec![Phase {
            name: "rc".into(),
            total_messages: Some(5),
            messages_per_second: 100,
            seed: Some(1),
            format: Some("raw".into()),
            templates: vec!["x {{sequence}}".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    run_profile(&profile, metrics.clone()).await.unwrap();
    let out = gather_metrics(&metrics).expect("gather_metrics ok in test");
    assert!(
        out.lines().any(|l| l.starts_with("syslog_errors_total{")),
        "ожидались ошибки отправки на мёртвый TCP-таргет"
    );
}

// ===================== F11 CLI-оверрайды + F13 валидация (веха D) =====================

#[tokio::test]
async fn test_f13_run_profile_rejects_invalid_profile() {
    // run_profile должен fail-fast на невалидном профиле, а не пытаться запуститься.
    let profile = Profile {
        targets: vec![TargetConfig {
            address: "127.0.0.1:514".into(),
            transport: "sctp".into(), // невалидно
            ..Default::default()
        }],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "p".into(),
            total_messages: Some(1),
            templates: vec!["x".into()],
            format: Some("raw".into()),
            ..Default::default()
        }],
        metrics_addr: None,
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    let res = run_profile(&profile, metrics).await;
    assert!(
        res.is_err(),
        "невалидный профиль должен быть отклонён run_profile"
    );
    let msg = format!("{}", res.unwrap_err());
    assert!(
        msg.contains("transport"),
        "сообщение об ошибке должно указывать на transport: {msg}"
    );
}

#[tokio::test]
async fn test_f13_valid_profile_passes_validation() {
    let profile = Profile {
        targets: vec![parse_target("127.0.0.1:9998:tcp").unwrap()],
        distribution: "round-robin".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![Phase {
            name: "ok".into(),
            total_messages: Some(1),
            templates: vec!["x".into()],
            format: Some("raw".into()),
            ..Default::default()
        }],
        metrics_addr: None,
    };
    assert!(validate_profile(&profile).is_empty());
}

#[tokio::test]
async fn test_f13_collects_all_errors() {
    let profile = Profile {
        targets: vec![TargetConfig {
            address: "".into(),
            transport: "bad".into(),
            connections: 0,
            framing: "junk".into(),
            ..Default::default()
        }],
        distribution: "nope".into(),
        shutdown: ShutdownConfig::default(),
        phases: vec![],
        metrics_addr: None,
    };
    let errs = validate_profile(&profile);
    // пустой address, bad transport, junk framing, zero connections, bad distribution, no phases
    assert!(
        errs.len() >= 6,
        "ожидалось >=6 ошибок, получено {}: {:?}",
        errs.len(),
        errs
    );
    assert!(errs.contains(&ValidationError::NoPhases));
}

#[tokio::test]
async fn test_f11_apply_overrides_then_run_to_file() {
    // Полный путь: пустой профиль -> CLI-оверрайды (target=file, message, total, seed)
    // -> валидация -> реальный запуск.
    let out = write_tmp("cli-override-run.log", "");
    let _ = fs::remove_file(&out);

    let mut profile = Profile::default();
    let overrides = Overrides {
        targets: vec![parse_target(&format!("{out}:file")).unwrap()],
        messages: vec!["cli-evt {{sequence}}".into()],
        total: Some(4),
        seed: Some(99),
        format: Some("raw".into()),
        ..Default::default()
    };
    apply_overrides(&mut profile, &overrides);

    // валидация должна пройти
    assert!(
        validate_profile(&profile).is_empty(),
        "профиль после оверрайдов должен быть валиден"
    );
    // distribution должен быть непустым (Default::default даёт round-robin)
    assert_eq!(profile.distribution, "round-robin");

    let metrics = create_metrics().expect("create_metrics ok in test");
    run_profile(&profile, metrics).await.unwrap();

    let content = fs::read_to_string(&out).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "ожидалось 4 сообщения, получено {}: {content:?}",
        lines.len()
    );
    assert_eq!(lines[0], "cli-evt 1");
    assert_eq!(lines[3], "cli-evt 4");
    let _ = fs::remove_file(&out);
}

#[tokio::test]
async fn test_f11_target_parsing_variants() {
    assert_eq!(parse_target("1.2.3.4:514").unwrap().transport, "tcp");
    assert_eq!(parse_target("1.2.3.4:514:udp").unwrap().transport, "udp");
    assert_eq!(parse_target("1.2.3.4:6514:tls").unwrap().transport, "tls");
    let f = parse_target("/var/log/out.log:file").unwrap();
    assert_eq!(f.transport, "file");
    assert_eq!(f.address, "/var/log/out.log");
}

#[tokio::test]
async fn test_f11_scalar_overrides_apply_to_all_phases() {
    let mut profile = Profile {
        phases: vec![
            Phase {
                name: "a".into(),
                templates: vec!["x".into()],
                ..Default::default()
            },
            Phase {
                name: "b".into(),
                templates: vec!["y".into()],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let overrides = Overrides {
        rate: Some(250),
        duration: Some(45),
        ..Default::default()
    };
    apply_overrides(&mut profile, &overrides);
    for p in &profile.phases {
        assert_eq!(p.messages_per_second, 250);
        assert_eq!(p.duration_secs, 45);
    }
}

// ===================== F12: HTTP /metrics =====================

/// F12: полный сквозной прогон — run_profile с metrics_addr на 127.0.0.1:0
/// поднимает HTTP-сервер; делаем реальный GET /metrics и проверяем
/// prometheus-текст, затем GET на неизвестный путь → 404.
#[tokio::test]
async fn test_f12_http_metrics_endpoint_via_run_profile() {
    // Свободный порт получаем через временную привязку, затем освобождаем.
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let addr_str = addr.to_string();

    let file_path = format!("f12-metrics-{}.log", std::process::id());
    let _ = fs::remove_file(&file_path);

    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.clone(),
            transport: "file".into(),
            ..Default::default()
        }],
        metrics_addr: Some(addr_str.clone()),
        phases: vec![Phase {
            // Длительная фаза (~2 с с ограниченным rate) — чтобы HTTP /metrics
            // был жив во время опроса (без этого прогон завершается мгновенно).
            name: "f12".into(),
            duration_secs: 2,
            messages_per_second: 50,
            seed: Some(7),
            format: Some("raw".into()),
            templates: vec!["msg {{sequence}}".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let metrics = create_metrics().expect("create_metrics ok in test");

    // Запускаем прогон в фоне; сервер /metrics живёт, пока идёт прогон.
    let run = tokio::spawn(async move { run_profile(&profile, metrics).await });

    // Дадим серверу время привязаться и опрашиваем /metrics с ретраями.
    let mut body = String::new();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        if let Ok(mut c) = TcpStream::connect(&addr_str).await {
            c.write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut buf = Vec::new();
            let _ = c.read_to_end(&mut buf).await;
            body = String::from_utf8_lossy(&buf).to_string();
            if body.contains("HTTP/1.1 200") {
                break;
            }
        }
    }
    assert!(body.contains("HTTP/1.1 200 OK"), "нет 200 OK: {body}");
    assert!(
        body.contains("text/plain; version=0.0.4"),
        "нет prometheus content-type: {body}"
    );
    assert!(
        body.contains("syslog_messages_total"),
        "нет метрик в теле: {body}"
    );

    // 404 на неизвестный путь.
    let mut c404 = TcpStream::connect(&addr_str).await.unwrap();
    c404.write_all(b"GET /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf404 = Vec::new();
    let _ = c404.read_to_end(&mut buf404).await;
    let body404 = String::from_utf8_lossy(&buf404);
    assert!(
        body404.contains("HTTP/1.1 404 Not Found"),
        "ожидался 404: {body404}"
    );

    let _ = run.await.unwrap();
    let _ = fs::remove_file(&file_path);
}

/// F12: если metrics_addr не задан — сервер не поднимается (порт свободен),
/// а прогон завершается штатно.
#[tokio::test]
async fn test_f12_no_metrics_addr_no_server() {
    let file_path = format!("f12-nometrics-{}.log", std::process::id());
    let _ = fs::remove_file(&file_path);
    let profile = Profile {
        targets: vec![TargetConfig {
            address: file_path.clone(),
            transport: "file".into(),
            ..Default::default()
        }],
        metrics_addr: None,
        phases: vec![Phase {
            name: "f12b".into(),
            total_messages: Some(3),
            messages_per_second: 1000,
            seed: Some(1),
            format: Some("raw".into()),
            templates: vec!["x {{sequence}}".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let metrics = create_metrics().expect("create_metrics ok in test");
    let res = run_profile(&profile, metrics).await;
    assert!(res.is_ok());
    let _ = fs::remove_file(&file_path);
}

// ===================== N4: безопасный TLS (валидация) =====================

/// N4: несуществующий tls_ca_file отклоняется валидацией профиля.
#[tokio::test]
async fn test_n4_tls_ca_file_missing_rejected() {
    let profile = Profile {
        targets: vec![TargetConfig {
            address: "127.0.0.1:6514".into(),
            transport: "tls".into(),
            tls_ca_file: Some("/nonexistent/path/ca.pem".into()),
            ..Default::default()
        }],
        phases: vec![Phase {
            name: "n4".into(),
            total_messages: Some(1),
            templates: vec!["x".into()],
            format: Some("raw".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let errs = validate_profile(&profile);
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::TlsCaFileNotFound { .. })),
        "ожидалась ошибка TlsCaFileNotFound, получено: {errs:?}"
    );
}

/// N4: существующий tls_ca_file принимается (профиль валиден по этому полю).
#[tokio::test]
async fn test_n4_tls_ca_file_present_ok() {
    // Создаём временный PEM-подобный файл (валидация проверяет только наличие).
    let ca_path = format!("n4-ca-{}.pem", std::process::id());
    fs::write(
        &ca_path,
        b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    let profile = Profile {
        targets: vec![TargetConfig {
            address: "127.0.0.1:6514".into(),
            transport: "tls".into(),
            tls_domain: Some("syslog.example.com".into()),
            tls_ca_file: Some(ca_path.clone()),
            ..Default::default()
        }],
        phases: vec![Phase {
            name: "n4b".into(),
            total_messages: Some(1),
            templates: vec!["x".into()],
            format: Some("raw".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let errs = validate_profile(&profile);
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, ValidationError::TlsCaFileNotFound { .. })),
        "неожиданная ошибка CA: {errs:?}"
    );
    let _ = fs::remove_file(&ca_path);
}

/// N4: tls_insecure по умолчанию false (безопасный режим — проверка сертификатов).
#[test]
fn test_n4_tls_insecure_defaults_false() {
    let t = TargetConfig::default();
    assert!(
        !t.tls_insecure,
        "по умолчанию TLS должен проверять сертификаты"
    );
    assert!(t.tls_domain.is_none());
    assert!(t.tls_ca_file.is_none());
}

// ===================== D3: JSON Schema + YAML-ввод (веха D) =====================
//
// D3 (v8.5.0): профили можно загружать как из JSON (.json), так и из YAML
// (.yaml/.yml). Формат определяется по расширению файла в load_profile_from_path.
// Дополнительно через флаг --schema-strict (CLI) профиль валидируется
// против формальной JSON Schema (schemas/profile.schema.json).

use syslog_generator::{load_profile_from_path, validate_against_embedded_schema};

/// D3: YAML-профиль из examples/ загружается корректно (round-robin).
#[test]
fn test_d3_yaml_profile_loads() {
    let path = "examples/multi_target_roundrobin.yaml";
    let p = load_profile_from_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("не удалось загрузить {path}: {e:?}"));
    assert_eq!(p.distribution, "round-robin");
    assert_eq!(p.targets.len(), 2);
    assert_eq!(p.targets[0].address, "./yaml-rr-a.log");
    assert_eq!(p.targets[1].address, "./yaml-rr-b.log");
    assert_eq!(p.phases.len(), 1);
    assert_eq!(p.phases[0].name, "yaml-rr");
    assert_eq!(p.phases[0].messages_per_second, 6);
    assert_eq!(p.phases[0].templates, vec!["yaml rr seq={{sequence}}"]);
}

/// D3: расширение .yml обрабатывается так же, как .yaml.
#[test]
fn test_d3_yml_profile_loads() {
    let path = "examples/multi_target_roundrobin.yml";
    let p = load_profile_from_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("не удалось загрузить {path}: {e:?}"));
    assert_eq!(p.phases[0].name, "yml-rr");
    assert_eq!(p.targets.len(), 2);
}

/// D3: YAML-профиль с load_shape.burst сериализуется без потерь.
#[test]
fn test_d3_yaml_load_shape_burst() {
    let path = "examples/load_shape_burst.yaml";
    let p = load_profile_from_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("не удалось загрузить {path}: {e:?}"));
    use syslog_generator::LoadShape;
    let shape = p.phases[0]
        .load_shape
        .as_ref()
        .expect("load_shape должна быть");
    match shape {
        LoadShape::Burst {
            base_rate,
            burst_rate,
            every_secs,
            burst_secs,
        } => {
            assert_eq!(*base_rate, 100.0);
            assert_eq!(*burst_rate, 8000.0);
            assert_eq!(*every_secs, 10.0);
            assert_eq!(*burst_secs, 2.0);
        }
        other => panic!("ожидался Burst, got: {other:?}"),
    }
}

/// D3: неподдерживаемое расширение → ConfigError::UnsupportedFormat.
#[test]
fn test_d3_unsupported_extension_returns_error() {
    use syslog_generator::ConfigError;
    let bad_path = std::env::temp_dir().join("sg_test_d3_unknown.toml");
    let e = load_profile_from_path(&bad_path).unwrap_err();
    match e {
        ConfigError::UnsupportedFormat { extension, .. } => {
            assert_eq!(extension, "toml");
        }
        other => panic!("ожидался UnsupportedFormat, got: {other:?}"),
    }
}

/// D3: все examples/*.json и examples/*.yaml проходят формальную JSON Schema.
/// Это защищает от регрессий: если кто-то изменит схему, тест сразу
/// покажет что примеры перестали соответствовать.
///
/// Файлы, которые не парсятся как Profile (например, schema-файлы для
/// schema_file, мета-файлы), пропускаются — мы не можем отличить их
/// по имени, только попыткой десериализации.
#[test]
fn test_d3_all_examples_pass_schema() {
    let entries: Vec<_> = std::fs::read_dir("examples")
        .expect("examples/ должен существовать")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("json") | Some("yaml") | Some("yml")
            )
        })
        .collect();

    let mut count = 0;
    for entry in entries {
        let path = entry.path();
        // Пробуем загрузить как Profile. Если не парсится — это не профиль
        // (например, schema-файл для schema_file), пропускаем молча.
        let p = match load_profile_from_path(&path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Схема-файлы (без phases) парсятся как Profile с пустым phases —
        // отфильтровываем по этому признаку.
        if p.phases.is_empty() {
            continue;
        }
        validate_against_embedded_schema(&p)
            .unwrap_or_else(|e| panic!("schema {}: {e}", path.display()));
        count += 1;
    }
    // Гарантируем, что примеры YAML реально добавились (не только JSON).
    assert!(count >= 5, "ожидалось >= 5 примеров-профилей, got: {count}");
}

// ====================== N8 (v8.7.1): back-pressure ======================
