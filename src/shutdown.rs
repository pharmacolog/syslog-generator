//! Graceful shutdown: слушатель SIGINT/Ctrl-C и drain воркеров.
//!
//! Поведение `shutdown_listener` не меняется — это fire-and-forget задача,
//! которая перехватывает Ctrl-C и взводит `CancellationToken`. Это приводит
//! к корректному завершению всех send-циклов.
//!
//! Поведение `graceful_drain_wait` (N7): раньше возвращал `anyhow::Result<()>`
//! с произвольным текстом ошибки. Теперь возвращает `Result<(), DrainError>`
//! — типизированный enum, у которого два варианта:
//! - `TaskJoin(JoinError)` — паника/отмена одной из sender-задач;
//! - `Timeout { timeout_secs }` — drain не успел уложиться в отведённое время.

use crate::error::DrainError;
use crate::metrics::Metrics;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub async fn shutdown_listener(token: CancellationToken, metrics: Metrics) {
    let _ = tokio::signal::ctrl_c().await;
    metrics.shutdowns_total.inc();
    token.cancel();
}

/// Дождаться завершения всех sender-задач после остановки продюсера.
///
/// Если не уложились в `timeout_secs` — увеличиваем счётчик
/// `syslog_drain_timeouts_total` и возвращаем `Err(DrainError::Timeout)`,
/// чтобы вызывающий код (`run_profile`) мог завершить процесс с
/// ненулевым кодом возврата.
///
/// Sender-задачи возвращают `anyhow::Result<()>`. Здесь мы фиксируем этот
/// тип — он не generic, потому что внутри `run_phase_multi` мы всегда
/// спавним `target_sender_*` (sender'ы), у которых сигнатура единая.
pub async fn graceful_drain_wait(
    handles: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
    timeout_secs: u64,
    metrics: Metrics,
) -> Result<(), DrainError> {
    let timeout = Duration::from_secs(timeout_secs);
    let started = Instant::now();
    let wait_all = async {
        for handle in handles {
            // Первый `?` пробрасывает JoinError (sender упал) — `From<JoinError>`
            // для `DrainError` через `#[from]`. Второй `?` пробрасывает внутренний
            // `anyhow::Error` от sender'а — `From<anyhow::Error>` тоже на месте.
            handle.await??;
        }
        Ok::<(), DrainError>(())
    };
    match tokio::time::timeout(timeout, wait_all).await {
        Ok(res) => {
            metrics.drain_duration.observe(started.elapsed().as_secs_f64());
            res
        }
        Err(_) => {
            metrics.drain_duration.observe(started.elapsed().as_secs_f64());
            metrics.drain_timeouts_total.inc();
            Err(DrainError::timeout(timeout_secs))
        }
    }
}
