//! F16 (v9.3.0): Kafka/Redpanda transport через pure-Rust клиент `rskafka`.
//!
//! Включается фичей `kafka` (opt-in, чтобы не тянуть зависимости тем,
//! кому Kafka не нужен): `cargo build --features kafka`.
//!
//! ## Архитектура
//!
//! - `KafkaConfig` — данные конфигурации (адреса брокеров, топик, compression,
//!   acks, linger). Дешёвая структура (String/enum), живёт в `TargetConfig`.
//! - `target_sender_kafka` — async sender: создаёт `Client` и `BatchProducer`,
//!   читает сообщения из rx, делает `produce`, инкрементирует метрики.
//! - `kafka_compression: String` (serde) → парсится в `rskafka::Compression`.
//! - `kafka_acks` — в rskafka 0.5 нет отдельного параметра acks (он
//!   контролируется на уровне брокера/топика). Поле сохранено для
//!   forward-compat и как метаданные в логах/метриках.
//!
//! ## Метрики
//!
//! - `syslog_kafka_produce_duration_seconds` (histogram) — латентность produce.
//! - `syslog_kafka_produce_batch_bytes` (histogram) — размер payload (record value).
//! - `syslog_kafka_produce_errors_total{target}` (counter) — ошибки produce.
//! - `syslog_kafka_messages_total{topic, result}` — успех/ошибка produce.

use crate::metrics::Metrics;
use anyhow::{anyhow, Result};
use rskafka::client::partition::{Compression as RkCompression, UnknownTopicHandling};
use rskafka::client::producer::{aggregator::RecordAggregator, BatchProducerBuilder};
use rskafka::client::ClientBuilder;
use rskafka::record::Record;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::{next_msg, record_send, record_send_latency, SharedRx};

/// F16: конфигурация Kafka-target'а.
///
/// Живёт в `TargetConfig.kafka_*`. Все поля опциональные с дефолтами —
/// backward-compat для профилей без Kafka.
#[derive(Debug, Clone)]
pub struct KafkaConfig {
    /// Bootstrap brokers (напр. ["broker1:9092", "broker2:9092"]).
    /// Парсится из `address` target'а: разделитель — запятая.
    pub bootstrap_servers: Vec<String>,
    /// Топик для отправки. Обязательное поле (валидатор проверяет).
    pub topic: String,
    /// Идентификатор клиента (для логов и мониторинга брокера).
    pub client_id: String,
    /// Compression: "none" / "gzip" / "snappy" / "lz4" / "zstd".
    pub compression: RkCompression,
    /// acks (forward-compat): "0" / "1" / "all". В rskafka 0.5 это
    /// контролируется на уровне брокера; сохраняем в логах.
    pub acks: Option<String>,
    /// Linger — задержка перед flush'ом батча (для батчинга).
    pub linger: Duration,
    /// Максимальный размер батча (число записей).
    pub max_batch_size: usize,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            bootstrap_servers: Vec::new(),
            topic: String::new(),
            client_id: "syslog-generator".to_string(),
            compression: RkCompression::NoCompression,
            acks: None,
            linger: Duration::from_millis(5),
            max_batch_size: 1024,
        }
    }
}

/// F16: парсит строку compression в `rskafka::Compression`. Возвращает
/// `Err(reason)` для неподдерживаемых значений. Compile-time фичи `rskafka`
/// определяют, какие варианты доступны (gzip/lz4/snappy/zstd).
pub fn parse_kafka_compression(s: &str) -> Result<RkCompression, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "none" | "no" | "" => Ok(RkCompression::NoCompression),
        #[cfg(feature = "kafka")]
        "gzip" => Ok(RkCompression::Gzip),
        #[cfg(feature = "kafka")]
        "lz4" => Ok(RkCompression::Lz4),
        #[cfg(feature = "kafka")]
        "snappy" => Ok(RkCompression::Snappy),
        #[cfg(feature = "kafka")]
        "zstd" => Ok(RkCompression::Zstd),
        other => Err(format!(
            "недопустимый kafka_compression {:?}; ожидается one of: none, gzip, lz4, snappy, zstd",
            other
        )),
    }
}

/// F16: парсит строку acks (forward-compat). Сохраняем в конфиге,
/// в логах; в rskafka 0.5 напрямую не используется.
pub fn parse_kafka_acks(s: &str) -> Result<String, String> {
    match s.trim() {
        "0" | "1" | "all" => Ok(s.trim().to_string()),
        other => Err(format!(
            "недопустимый kafka_acks {:?}; ожидается \"0\", \"1\", \"all\"",
            other
        )),
    }
}

/// Парсит `address` (CSV-список host:port) в `Vec<String>`.
pub fn parse_bootstrap_servers(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// F16: основной sender — читает из `rx`, шлёт в Kafka через `rskafka`.
/// Метрики инкрементируются per-message (latency) и per-batch (bytes).
pub async fn target_sender_kafka(
    config: KafkaConfig,
    addr: String, // bootstrap_servers CSV (для метрик; в config уже распарсен)
    phase_name: String,
    rx: SharedRx,
    metrics: Metrics,
    shutdown: CancellationToken,
) -> Result<()> {
    if config.topic.is_empty() {
        metrics.errors_total.with_label_values(&[&addr]).inc();
        eprintln!("Kafka ({addr}): kafka_topic не задан — sender не запускается");
        super::drain_as_errors(&rx, &metrics, &addr).await;
        return Ok(());
    }
    if config.bootstrap_servers.is_empty() {
        metrics.errors_total.with_label_values(&[&addr]).inc();
        eprintln!("Kafka ({addr}): bootstrap_servers пустые — sender не запускается");
        super::drain_as_errors(&rx, &metrics, &addr).await;
        return Ok(());
    }

    // Создаём Kafka-клиент. rskafka сам делает leader-detection и
    // переподключение внутри PartitionClient; внешний retry не нужен.
    let client = Arc::new(
        ClientBuilder::new(config.bootstrap_servers.clone())
            .client_id(config.client_id.clone())
            .build()
            .await
            .map_err(|e| anyhow!("Kafka ({addr}): не удалось создать Client: {e}"))?,
    );

    // Берём partition 0 (типичный default для single-partition топиков).
    // Для multi-partition — пользователь может расширить в будущем
    // (сейчас API rskafka не позволяет указать partition strategy на
    // уровне Client без явной round-robin логики в нашем коде).
    let partition_client = Arc::new(
        client
            .partition_client(config.topic.clone(), 0, UnknownTopicHandling::Retry)
            .await
            .map_err(|e| anyhow!("Kafka ({addr}): partition_client({}): {e}", config.topic))?,
    );

    // BatchProducer с linger=5ms и настраиваемой compression. По дефолту
    // rskafka батчит записи внутри linger-окна — это снижает число
    // produce-requests и улучшает throughput на порядок.
    let producer = BatchProducerBuilder::new(partition_client.clone())
        .with_linger(config.linger)
        .with_compression(config.compression)
        .build(RecordAggregator::new(config.max_batch_size));

    // Метрики латентности produce и размера payload. rskafka сам
    // инкрементит internal-счётчики, но для Prometheus-экспорта нужны
    // наши.
    while let Some(msg) = next_msg(&rx).await {
        if shutdown.is_cancelled() {
            break;
        }
        let bytes = msg.len() as f64;
        // PR-17e: msg is Bytes, rskafka expects Vec<u8> for Record.value.
        // Конвертация через Bytes::into() — zero-copy если Bytes не shared.
        let record = Record {
            key: None,
            value: Some(msg.into()),
            headers: BTreeMap::new(),
            timestamp: chrono::Utc::now(),
        };
        let t0 = std::time::Instant::now();
        let res = producer.produce(record).await;
        let elapsed = t0.elapsed();
        match res {
            Ok(_) => {
                record_send_latency(&metrics, elapsed);
                // ВАЖНО: histogram размера здесь — обновляем до record_send
                // (record_send тоже обновляет message_size_bytes, но это
                // общий histogram, который мониторит payload-size — оба
                // обновления согласуются).
                metrics.kafka_produce_batch_bytes.observe(bytes);
                record_send(
                    &metrics,
                    "kafka",
                    &phase_name,
                    &addr,
                    bytes as u64,
                    &shutdown,
                )
                .await;
            }
            Err(e) => {
                metrics.errors_total.with_label_values(&[&addr]).inc();
                metrics
                    .kafka_produce_errors_total
                    .with_label_values(&[&addr, "produce"])
                    .inc();
                eprintln!("Kafka ({addr}): produce error: {e}");
                // Продолжаем работу — rskafka сам сделает leader-recovery
                // для следующего produce. Полностью сливать очередь в
                // errors не стоит: если broker временно недоступен, его
                // восстановление не должно приводить к потере всех
                // последующих сообщений.
            }
        }
    }
    // Flush остатков батча перед выходом (graceful drain).
    if let Err(e) = producer.flush().await {
        eprintln!("Kafka ({addr}): flush error: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kafka_compression_known_values() {
        assert_eq!(
            parse_kafka_compression("none").unwrap(),
            RkCompression::NoCompression
        );
        assert_eq!(
            parse_kafka_compression("").unwrap(),
            RkCompression::NoCompression
        );
        assert_eq!(
            parse_kafka_compression("None").unwrap(),
            RkCompression::NoCompression
        );
        assert!(parse_kafka_compression("unknown").is_err());
        assert!(parse_kafka_compression("brotli").is_err());
    }

    #[test]
    fn parse_kafka_acks_known_values() {
        assert_eq!(parse_kafka_acks("0").unwrap(), "0");
        assert_eq!(parse_kafka_acks("1").unwrap(), "1");
        assert_eq!(parse_kafka_acks("all").unwrap(), "all");
        assert_eq!(parse_kafka_acks("  0  ").unwrap(), "0");
        assert!(parse_kafka_acks("2").is_err());
        assert!(parse_kafka_acks("none").is_err());
    }

    #[test]
    fn parse_bootstrap_servers_csv() {
        let v = parse_bootstrap_servers("broker1:9092,broker2:9092");
        assert_eq!(v, vec!["broker1:9092", "broker2:9092"]);
        let v = parse_bootstrap_servers("  a:1 , b:2 , ,c:3 ");
        assert_eq!(v, vec!["a:1", "b:2", "c:3"]);
        let v = parse_bootstrap_servers("");
        assert!(v.is_empty());
        let v = parse_bootstrap_servers("single:9092");
        assert_eq!(v, vec!["single:9092"]);
    }

    #[test]
    fn kafka_config_default() {
        let c = KafkaConfig::default();
        assert_eq!(c.client_id, "syslog-generator");
        assert_eq!(c.compression, RkCompression::NoCompression);
        assert!(c.topic.is_empty());
        assert_eq!(c.linger, Duration::from_millis(5));
        assert_eq!(c.max_batch_size, 1024);
    }
}
