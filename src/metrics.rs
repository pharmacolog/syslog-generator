
use prometheus::{CounterVec, Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder};

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
    pub generate_duration: Histogram,
    pub send_duration: Histogram,
    pub message_size_bytes: Histogram,
    pub reconnects_total: CounterVec,
    pub cpu_usage: Gauge,
    pub memory_usage: Gauge,
    pub shutdowns_total: IntCounter,
    pub drain_duration: Histogram,
    pub drain_timeouts_total: IntCounter,
    pub messages_drained_total: CounterVec,
}

pub fn create_metrics() -> Metrics {
    let registry = Registry::new();
    let messages_total = CounterVec::new(prometheus::opts!("syslog_messages_total", "Total messages sent"), &["transport", "phase", "target", "result"]).unwrap();
    let messages_generated_total = CounterVec::new(prometheus::opts!("syslog_messages_generated_total", "Total messages generated"), &["phase"]).unwrap();
    let target_rate = Gauge::new("syslog_target_rate_messages_per_second", "Configured target generation rate (messages per second)").unwrap();
    let achieved_rate = Gauge::new("syslog_achieved_rate_messages_per_second", "Achieved generation rate of the last phase (messages per second)").unwrap();
    let active_workers = Gauge::new("syslog_active_workers", "Number of active sender workers across all targets in the current phase").unwrap();
    let bytes_total = CounterVec::new(prometheus::opts!("syslog_bytes_total", "Total bytes sent"), &["transport", "phase", "target"]).unwrap();
    let errors_total = CounterVec::new(prometheus::opts!("syslog_errors_total", "Total send errors"), &["target"]).unwrap();
    let messages_by_sink = CounterVec::new(prometheus::opts!("syslog_messages_by_sink_total", "Messages by sink"), &["sink"]).unwrap();
    let generate_duration = Histogram::with_opts(HistogramOpts::new("syslog_generate_duration_seconds", "Generate duration seconds")).unwrap();
    // Латентность отправки одного сообщения в транспорт (запись в сокет/файл).
    // Buckets покрывают диапазон от 5 мкс до 1 с — достаточно для p50/p95/p99.
    let send_duration = Histogram::with_opts(HistogramOpts::new("syslog_send_duration_seconds", "Per-message send latency into the transport (write/send syscall)").buckets(vec![0.000005,0.00001,0.000025,0.00005,0.0001,0.00025,0.0005,0.001,0.0025,0.005,0.01,0.025,0.05,0.1,0.25,0.5,1.0])).unwrap();
    // Размер отправленного сообщения в байтах (тело SYSLOG-MSG без фрейминга).
    let message_size_bytes = Histogram::with_opts(HistogramOpts::new("syslog_message_size_bytes", "Size of generated syslog message payload in bytes").buckets(vec![16.0,32.0,64.0,128.0,256.0,512.0,1024.0,2048.0,4096.0,8192.0,16384.0,32768.0,65536.0])).unwrap();
    // Число переустановок соединения (TCP/TLS) после ошибки записи.
    let reconnects_total = CounterVec::new(prometheus::opts!("syslog_reconnects_total", "Total transport reconnection attempts after a write failure"), &["transport", "target"]).unwrap();
    let cpu_usage = Gauge::new("syslog_cpu_usage_percent", "CPU usage percent of generator process").unwrap();
    let memory_usage = Gauge::new("syslog_memory_usage_bytes", "Memory usage in bytes of generator process").unwrap();
    let shutdowns_total = IntCounter::new("syslog_shutdowns_total", "Total shutdown events").unwrap();
    let drain_duration = Histogram::with_opts(HistogramOpts::new("syslog_drain_duration_seconds", "Graceful drain duration").buckets(vec![0.1,0.5,1.0,2.0,5.0,10.0,15.0,30.0,60.0])).unwrap();
    let drain_timeouts_total = IntCounter::new("syslog_drain_timeouts_total", "Total drain timeout events").unwrap();
    let messages_drained_total = CounterVec::new(prometheus::opts!("syslog_messages_drained_total", "Messages drained during shutdown"), &["target"]).unwrap();
    for c in [Box::new(messages_total.clone()) as Box<dyn prometheus::core::Collector>, Box::new(messages_generated_total.clone()), Box::new(target_rate.clone()), Box::new(achieved_rate.clone()), Box::new(active_workers.clone()), Box::new(bytes_total.clone()), Box::new(errors_total.clone()), Box::new(messages_by_sink.clone()), Box::new(generate_duration.clone()), Box::new(send_duration.clone()), Box::new(message_size_bytes.clone()), Box::new(reconnects_total.clone()), Box::new(cpu_usage.clone()), Box::new(memory_usage.clone()), Box::new(shutdowns_total.clone()), Box::new(drain_duration.clone()), Box::new(drain_timeouts_total.clone()), Box::new(messages_drained_total.clone())] { registry.register(c).unwrap(); }
    Metrics { registry, messages_total, messages_generated_total, target_rate, achieved_rate, active_workers, bytes_total, errors_total, messages_by_sink, generate_duration, send_duration, message_size_bytes, reconnects_total, cpu_usage, memory_usage, shutdowns_total, drain_duration, drain_timeouts_total, messages_drained_total }
}

pub fn gather_metrics(metrics: &Metrics) -> String {
    let encoder = TextEncoder::new();
    let mf = metrics.registry.gather();
    let mut buf = Vec::new();
    encoder.encode(&mf, &mut buf).unwrap();
    String::from_utf8(buf).unwrap_or_default()
}
