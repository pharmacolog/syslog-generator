//! Prometheus-метрики нагрузочного генератора.
//!
//! Все метрики создаются через [`create_metrics`] и собираются через
//! [`gather_metrics`]. До N7 конструкторы метрик (`CounterVec::new`,
//! `Gauge::new`, `Histogram::with_opts`, `IntCounter::new`, `Registry::register`)
//! и сериализация (`TextEncoder::encode`) использовали `.unwrap()`, что при
//! любой ошибке инициализации приводило к панике.
//!
//! С N7 эти пути типизированы через [`crate::error::MetricsError`]:
//! - `create_metrics()` возвращает `Result<Metrics, MetricsError>`;
//! - `gather_metrics()` возвращает `Result<String, MetricsError>`.
//!
//! Все метрики регистрируются ровно один раз в момент создания `Metrics`
//! и далее переиспользуются через `clone()` (Prometheus-метрики внутри
//! обёрнуты в `Arc`, поэтому клонирование дешёвое).

use crate::error::MetricsError;
use prometheus::{
    CounterVec, Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Metrics {
    pub registry: Registry,
    pub messages_total: CounterVec,
    pub messages_generated_total: CounterVec,
    pub target_rate: Gauge,
    pub achieved_rate: Gauge,
    pub active_workers: Gauge,
    pub bytes_total: CounterVec,
    pub errors_total: CounterVec,
    pub messages_by_sink: CounterVec,
    /// N2 (v8.6.0): счётчик сгенерированных сообщений по формату (rfc5424/rfc3164/raw/protobuf).
    /// Полезен для понимания, какой формат преобладает в трафике, и для построения
    /// panel "messages by format" в Grafana. Инкрементируется в `generate_message`.
    pub messages_by_format_total: CounterVec,
    pub generate_duration: Histogram,
    pub send_duration: Histogram,
    pub message_size_bytes: Histogram,
    pub reconnects_total: CounterVec,
    pub shutdowns_total: IntCounter,
    pub drain_duration: Histogram,
    pub drain_timeouts_total: IntCounter,
    pub messages_drained_total: CounterVec,
}

/// Создать набор Prometheus-метрик и зарегистрировать их в собственном registry.
///
/// # Ошибки (N7)
///
/// Раньше каждый из ~10 конструкторов и ~18 регистраций использовал `.unwrap()`,
/// что превращало любой конфликт имён или некорректный формат метрики в
/// панику. Теперь все они проходят через `?` и типизированную ошибку
/// [`MetricsError`]. Возможные причины отказа:
/// - дубликат имени метрики в registry (теоретически невозможно — все имена
///   уникальны по построению, но защита нужна на случай регрессии);
/// - некорректный формат имени/лейблов (`prometheus::Error`).
pub fn create_metrics() -> Result<Metrics, MetricsError> {
    let registry = Registry::new();

    let messages_total = make_counter_vec(
        "syslog_messages_total",
        "Total messages sent",
        &["transport", "phase", "target", "result"],
    )?;
    let messages_generated_total = make_counter_vec(
        "syslog_messages_generated_total",
        "Total messages generated",
        &["phase"],
    )?;
    let target_rate = make_gauge(
        "syslog_target_rate_messages_per_second",
        "Configured target generation rate (messages per second)",
    )?;
    let achieved_rate = make_gauge(
        "syslog_achieved_rate_messages_per_second",
        "Achieved generation rate of the last phase (messages per second)",
    )?;
    let active_workers = make_gauge(
        "syslog_active_workers",
        "Number of active sender workers across all targets in the current phase",
    )?;
    let bytes_total = make_counter_vec(
        "syslog_bytes_total",
        "Total bytes sent",
        &["transport", "phase", "target"],
    )?;
    let errors_total = make_counter_vec("syslog_errors_total", "Total send errors", &["target"])?;
    let messages_by_sink = make_counter_vec(
        "syslog_messages_by_sink_total",
        "Messages by sink",
        &["sink"],
    )?;
    // N2 (v8.6.0): счётчик сгенерированных сообщений по формату.
    // Инкрементируется в `payload::generate_message` после успешной генерации;
    // нужен для дашборда "messages by format".
    let messages_by_format_total = make_counter_vec(
        "syslog_messages_by_format_total",
        "Total messages generated, by format",
        &["format"],
    )?;
    let generate_duration = make_histogram(
        "syslog_generate_duration_seconds",
        "Generate duration seconds",
        None,
    )?;
    // Латентность отправки одного сообщения в транспорт (запись в сокет/файл).
    // Buckets покрывают диапазон от 5 мкс до 1 с — достаточно для p50/p95/p99.
    let send_duration = make_histogram(
        "syslog_send_duration_seconds",
        "Per-message send latency into the transport (write/send syscall)",
        Some(vec![
            0.000005, 0.00001, 0.000025, 0.00005, 0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005,
            0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
        ]),
    )?;
    // Размер отправленного сообщения в байтах (тело SYSLOG-MSG без фрейминга).
    let message_size_bytes = make_histogram(
        "syslog_message_size_bytes",
        "Size of generated syslog message payload in bytes",
        Some(vec![
            16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0,
            32768.0, 65536.0,
        ]),
    )?;
    // Число переустановок соединения (TCP/TLS) после ошибки записи.
    let reconnects_total = make_counter_vec(
        "syslog_reconnects_total",
        "Total transport reconnection attempts after a write failure",
        &["transport", "target"],
    )?;
    // N2 (v8.6.0): cpu_usage/memory_usage удалены — Gauge'ы были объявлены,
    // но никогда не обновлялись (нет реального сбора), поэтому в /metrics
    // всегда показывали 0, а в дашборде — пустые графики. Честный подход —
    // не обещать то, чего нет.
    let shutdowns_total = make_int_counter("syslog_shutdowns_total", "Total shutdown events")?;
    let drain_duration = make_histogram(
        "syslog_drain_duration_seconds",
        "Graceful drain duration",
        Some(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0]),
    )?;
    let drain_timeouts_total =
        make_int_counter("syslog_drain_timeouts_total", "Total drain timeout events")?;
    let messages_drained_total = make_counter_vec(
        "syslog_messages_drained_total",
        "Messages drained during shutdown",
        &["target"],
    )?;

    // Регистрируем все метрики. Имя для каждой позиции хранится рядом, чтобы
    // при ошибке регистрации сообщение содержало его. Это убирает последний
    // `.unwrap()` в кодовой базе (был в цикле `register(...).unwrap()`).
    let pairs: Vec<(&str, Box<dyn prometheus::core::Collector>)> = vec![
        ("syslog_messages_total", Box::new(messages_total.clone())),
        (
            "syslog_messages_generated_total",
            Box::new(messages_generated_total.clone()),
        ),
        (
            "syslog_target_rate_messages_per_second",
            Box::new(target_rate.clone()),
        ),
        (
            "syslog_achieved_rate_messages_per_second",
            Box::new(achieved_rate.clone()),
        ),
        ("syslog_active_workers", Box::new(active_workers.clone())),
        ("syslog_bytes_total", Box::new(bytes_total.clone())),
        ("syslog_errors_total", Box::new(errors_total.clone())),
        (
            "syslog_messages_by_sink_total",
            Box::new(messages_by_sink.clone()),
        ),
        (
            "syslog_messages_by_format_total",
            Box::new(messages_by_format_total.clone()),
        ),
        (
            "syslog_generate_duration_seconds",
            Box::new(generate_duration.clone()),
        ),
        (
            "syslog_send_duration_seconds",
            Box::new(send_duration.clone()),
        ),
        (
            "syslog_message_size_bytes",
            Box::new(message_size_bytes.clone()),
        ),
        (
            "syslog_reconnects_total",
            Box::new(reconnects_total.clone()),
        ),
        ("syslog_shutdowns_total", Box::new(shutdowns_total.clone())),
        (
            "syslog_drain_duration_seconds",
            Box::new(drain_duration.clone()),
        ),
        (
            "syslog_drain_timeouts_total",
            Box::new(drain_timeouts_total.clone()),
        ),
        (
            "syslog_messages_drained_total",
            Box::new(messages_drained_total.clone()),
        ),
    ];
    for (name, c) in pairs {
        registry
            .register(c)
            .map_err(|source| MetricsError::register(name, source))?;
    }

    Ok(Metrics {
        registry,
        messages_total,
        messages_generated_total,
        target_rate,
        achieved_rate,
        active_workers,
        bytes_total,
        errors_total,
        messages_by_sink,
        messages_by_format_total,
        generate_duration,
        send_duration,
        message_size_bytes,
        reconnects_total,
        shutdowns_total,
        drain_duration,
        drain_timeouts_total,
        messages_drained_total,
    })
}

/// Собрать текущее состояние всех метрик в Prometheus text exposition format (v0.0.4).
///
/// # Ошибки (N7)
///
/// Раньше использовался `.unwrap()` на `TextEncoder::encode` и
/// `String::from_utf8(...).unwrap_or_default()` на результате. Теперь обе
/// операции проходят через `?` и типизированную ошибку [`MetricsError`].
/// На практике ошибка почти невозможна (Prometheus encoder всегда выдаёт
/// валидный UTF-8), но типобезопасность требует явной обработки.
pub fn gather_metrics(metrics: &Metrics) -> Result<String, MetricsError> {
    let encoder = TextEncoder::new();
    let mf = metrics.registry.gather();
    let mut buf = Vec::new();
    encoder.encode(&mf, &mut buf)?;
    let s = String::from_utf8(buf)?;
    Ok(s)
}

// --- внутренние хелперы ----------------------------------------------------
//
// Каждый хелпер оборачивает соответствующий конструктор Prometheus в
// `Result<T, MetricsError>`, добавляя имя метрики в сообщение об ошибке.
// До N7 здесь были `.unwrap()` на каждом вызове.

fn make_counter_vec(name: &str, help: &str, labels: &[&str]) -> Result<CounterVec, MetricsError> {
    CounterVec::new(prometheus::opts!(name, help), labels)
        .map_err(|source| MetricsError::construct("CounterVec", name, source))
}

fn make_gauge(name: &str, help: &str) -> Result<Gauge, MetricsError> {
    Gauge::new(name, help).map_err(|source| MetricsError::construct("Gauge", name, source))
}

fn make_int_counter(name: &str, help: &str) -> Result<IntCounter, MetricsError> {
    IntCounter::new(name, help)
        .map_err(|source| MetricsError::construct("IntCounter", name, source))
}

fn make_histogram(
    name: &str,
    help: &str,
    buckets: Option<Vec<f64>>,
) -> Result<Histogram, MetricsError> {
    let opts = HistogramOpts::new(name, help);
    let opts = match buckets {
        Some(b) => opts.buckets(b),
        None => opts,
    };
    Histogram::with_opts(opts).map_err(|source| MetricsError::construct("Histogram", name, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// N7: `create_metrics()` возвращает Ok и готовый registry.
    #[test]
    fn create_metrics_ok() {
        let m = create_metrics().expect("create_metrics должен возвращать Ok в штатном режиме");
        // Скалярная метрика присутствует всегда (IntCounter без лейблов).
        assert!(m.shutdowns_total.get() == 0);
    }

    /// N7: `gather_metrics()` возвращает валидный Prometheus text exposition.
    /// Проверяем, что CounterVec без наблюдённых меток не ломает экспорт —
    /// они просто не попадают в вывод до первого `inc()`.
    #[test]
    fn gather_metrics_ok_contains_known_scalars() {
        let m = create_metrics().expect("create_metrics ok");
        let s = gather_metrics(&m).expect("gather_metrics ok");
        assert!(s.contains("syslog_shutdowns_total"));
        assert!(s.contains("syslog_drain_duration_seconds"));
        assert!(s.contains("syslog_generate_duration_seconds"));
    }

    /// N7: после `inc()` на CounterVec соответствующая серия появляется в выводе.
    #[test]
    fn gather_metrics_includes_labeled_counter_after_inc() {
        let m = create_metrics().expect("create_metrics ok");
        m.messages_total
            .with_label_values(&["tcp", "p1", "127.0.0.1:1", "success"])
            .inc();
        let s = gather_metrics(&m).expect("gather_metrics ok");
        assert!(s.contains("syslog_messages_total"), "got:\n{s}");
    }

    /// N2 (v8.6.0): cpu_usage/memory_usage удалены — больше не должны
    /// экспортироваться. Это регрессионный тест: если кто-то добавит их
    /// обратно без реальной реализации сбора, тест напомнит что это
    /// давало пустые графики в дашборде.
    #[test]
    fn n2_no_cpu_or_memory_gauges_in_exposition() {
        let m = create_metrics().expect("create_metrics ok");
        let s = gather_metrics(&m).expect("gather_metrics ok");
        assert!(
            !s.contains("syslog_cpu_usage_percent"),
            "cpu_usage_percent Gauge удалён в N2 — в выводе его быть не должно"
        );
        assert!(
            !s.contains("syslog_memory_usage_bytes"),
            "memory_usage_bytes Gauge удалён в N2 — в выводе его быть не должно"
        );
    }

    /// N2 (v8.6.0): `messages_by_format_total` присутствует в /metrics после
    /// инкремента с явным форматом.
    #[test]
    fn n2_messages_by_format_total_after_inc() {
        let m = create_metrics().expect("create_metrics ok");
        m.messages_by_format_total
            .with_label_values(&["rfc5424"])
            .inc();
        m.messages_by_format_total
            .with_label_values(&["raw"])
            .inc_by(3.0);
        let s = gather_metrics(&m).expect("gather_metrics ok");
        assert!(s.contains("syslog_messages_by_format_total"), "got:\n{s}");
        assert!(s.contains("format=\"rfc5424\""), "got:\n{s}");
        assert!(s.contains("format=\"raw\""), "got:\n{s}");
        // Проверяем значения в экспорте. Prometheus exposition format
        // использует ПРОБЕЛ между метрикой и значением (`metric{labels} 42`),
        // не запятую.
        assert!(
            s.lines().any(|l| {
                l.starts_with("syslog_messages_by_format_total{format=\"rfc5424\"}")
                    && l.trim_end().ends_with(" 1")
            }),
            "rfc5424 должно быть = 1, got:\n{s}"
        );
        assert!(
            s.lines().any(|l| {
                l.starts_with("syslog_messages_by_format_total{format=\"raw\"}")
                    && l.trim_end().ends_with(" 3")
            }),
            "raw должно быть = 3, got:\n{s}"
        );
    }
}
