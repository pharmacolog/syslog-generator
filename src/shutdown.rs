
use anyhow::Result;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use crate::metrics::Metrics;

pub async fn shutdown_listener(token: CancellationToken, metrics: Metrics) {
    let _ = tokio::signal::ctrl_c().await;
    metrics.shutdowns_total.inc();
    token.cancel();
}

pub async fn graceful_drain_wait<T>(handles: Vec<tokio::task::JoinHandle<Result<T>>>, timeout_secs: u64, metrics: Metrics) -> Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let started = Instant::now();
    let wait_all = async {
        for handle in handles { let _ = handle.await??; }
        Ok::<(), anyhow::Error>(())
    };
    match tokio::time::timeout(timeout, wait_all).await {
        Ok(res) => { metrics.drain_duration.observe(started.elapsed().as_secs_f64()); res }
        Err(_) => { metrics.drain_duration.observe(started.elapsed().as_secs_f64()); metrics.drain_timeouts_total.inc(); Err(anyhow::anyhow!("drain timeout exceeded after {} seconds", timeout_secs)) }
    }
}
