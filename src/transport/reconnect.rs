//! F16 (v9.3.0): утилита reconnect-стратегии с exponential backoff + jitter.
//!
//! Используется в TCP и TLS транспортах: после ошибки записи выполняется
//! повторный handshake с нарастающей задержкой и джиттером, чтобы не
//! «долбить» упавший endpoint без пауз и не синхронизироваться с другими
//! клиентами (thundering herd).
//!
//! ## Алгоритм
//!
//! ```text
//! attempt = 1..=max_attempts (или ∞ если None)
//! backoff = initial_backoff_ms
//! loop {
//!     if shutdown.cancelled() -> return None
//!     match connect().await {
//!         Ok(v) => return Some(v),
//!         Err(_) => {}
//!     }
//!     jitter ∈ [0.5, 1.5)
//!     sleep_ms = min(backoff * jitter, max_backoff_ms)
//!     tokio::select! { sleep; shutdown.cancelled() => return None }
//!     backoff = min(backoff * multiplier, max_backoff_ms)
//! }
//! ```
//!
//! ## Почему jitter
//!
//! Без jitter все клиенты (наш + коллеги в том же ЦОД) реконнектятся
//! синхронно → пиковый шторм запросов ровно в момент recovery упавшего
//! endpoint'а. Равномерный jitter в [0.5, 1.5) разносит попытки по
//! времени — см. AWS Architecture Blog «Exponential Backoff And Jitter».
//!
//! ## Shutdown
//!
//! `tokio::select!` на `shutdown.cancelled()` внутри backoff-loop критичен:
//! иначе graceful shutdown будет ждать полного backoff-таймаута.

use std::future::Future;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Параметры reconnect-стратегии.
///
/// Поля `Option<...>` отражают serde-дефолты (`#[serde(default)]` в
/// `TargetConfig`): None в большинстве случаев означает «использовать
/// встроенный разумный дефолт», см. [`ReconnectConfig::resolve`].
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Максимум попыток. `None` = бесконечно (до отмены shutdown'ом).
    pub max_attempts: Option<u32>,
    /// Начальная задержка перед первой повторной попыткой (после первой
    /// неудачи). Типичное значение: 100 мс.
    pub initial_backoff_ms: u64,
    /// Верхняя граница задержки (после многократного умножения).
    /// Типичное значение: 30 000 мс (30 с).
    pub max_backoff_ms: u64,
    /// Множитель: `backoff = backoff * multiplier` после каждой неудачи.
    /// Типичное значение: 2.0 (×2 каждый раз).
    pub multiplier: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: None,
            initial_backoff_ms: 100,
            max_backoff_ms: 30_000,
            multiplier: 2.0,
        }
    }
}

impl ReconnectConfig {
    /// Создать конфиг из опциональных полей `TargetConfig`, заполняя
    /// дефолтами отсутствующие. Используется в `core.rs::run_phase_multi`.
    pub fn resolve(
        max_attempts: Option<u32>,
        initial_backoff_ms: Option<u64>,
        max_backoff_ms: Option<u64>,
        multiplier: Option<f64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            max_attempts: max_attempts.or(defaults.max_attempts),
            initial_backoff_ms: initial_backoff_ms.unwrap_or(defaults.initial_backoff_ms),
            max_backoff_ms: max_backoff_ms.unwrap_or(defaults.max_backoff_ms),
            multiplier: multiplier.unwrap_or(defaults.multiplier),
        }
    }

    /// Валидация параметров (вызывается из validate.rs). Возвращает
    /// `Err(reason)` если параметры вне диапазона.
    pub fn validate(&self) -> Result<(), String> {
        if self.initial_backoff_ms == 0 {
            return Err("reconnect_initial_backoff_ms должен быть > 0".to_string());
        }
        if self.max_backoff_ms < self.initial_backoff_ms {
            return Err(format!(
                "reconnect_max_backoff_ms ({}) должен быть >= reconnect_initial_backoff_ms ({})",
                self.max_backoff_ms, self.initial_backoff_ms
            ));
        }
        if !self.multiplier.is_finite() || self.multiplier < 1.0 {
            return Err(format!(
                "reconnect_multiplier ({}) должен быть >= 1.0 и конечным",
                self.multiplier
            ));
        }
        Ok(())
    }
}

/// Выполнить reconnect с exponential backoff + jitter.
///
/// `connect` — замыкание, создающее Future подключения. Вызывается
/// последовательно до тех пор, пока не вернёт `Ok(_)` (Some возвращается)
/// или пока не закончатся `max_attempts` (None возвращается).
/// `on_attempt` (опционально) вызывается перед каждой попыткой — обычно
/// используется для инкремента `syslog_reconnects_total` метрики.
///
/// ## Backoff schedule
///
/// С `initial=100ms, max=30s, multiplier=2.0`:
/// - попытка 1: 0 ms (сразу)
/// - попытка 2: ~100 ms
/// - попытка 3: ~200 ms
/// - попытка 4: ~400 ms
/// - попытка 5: ~800 ms
/// - попытка 6: ~1.6 s
/// - попытка 7: ~3.2 s
/// - попытка 8: ~6.4 s
/// - попытка 9: ~12.8 s
/// - попытка 10: ~25.6 s
/// - попытка 11+: ~30 s (cap)
pub async fn reconnect_with_backoff<F, Fut, T, E>(
    config: &ReconnectConfig,
    shutdown: &CancellationToken,
    mut on_attempt: impl FnMut(),
    mut connect: F,
) -> Option<Result<T, E>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut backoff_ms = config.initial_backoff_ms as f64;
    let max_backoff_ms = config.max_backoff_ms as f64;
    let multiplier = config.multiplier;

    // Стратегия итераций: `attempts` (u64, чтобы избежать переполнения при
    // max_attempts=None и долгом цикле). При Some(max) — обрываем по счётчику.
    let mut attempts: u64 = 0;
    loop {
        if shutdown.is_cancelled() {
            return None;
        }
        // Проверяем max_attempts ДО попытки, чтобы первый успешный
        // connect не был отброшен из-за превышения счётчика.
        if let Some(max) = config.max_attempts {
            if attempts >= max as u64 {
                return None;
            }
        }
        attempts += 1;
        on_attempt();
        match connect().await {
            Ok(v) => return Some(Ok(v)),
            Err(e) => {
                if let Some(max) = config.max_attempts {
                    if attempts >= max as u64 {
                        return Some(Err(e));
                    }
                }
                // Равномерный jitter в [0.5, 1.5) — типичная рекомендация
                // AWS Architecture Blog для разнесения пиков при recovery.
                let jitter: f64 = rand::random::<f64>() + 0.5;
                let sleep_ms = (backoff_ms * jitter).min(max_backoff_ms) as u64;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
                    _ = shutdown.cancelled() => { return None; }
                }
                backoff_ms = (backoff_ms * multiplier).min(max_backoff_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn success_on_first_attempt() {
        let cfg = ReconnectConfig {
            max_attempts: Some(3),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok::<i32, &'static str>(42)
                }
            },
        )
        .await;
        assert!(matches!(result, Some(Ok(42))));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_until_success() {
        let cfg = ReconnectConfig {
            max_attempts: Some(5),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Err("transient")
                    } else {
                        Ok::<i32, &'static str>(99)
                    }
                }
            },
        )
        .await;
        assert!(matches!(result, Some(Ok(99))));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn returns_some_err_when_max_attempts_exhausted() {
        // Семантика: max_attempts = N → ровно N попыток. После N-й неудачи
        // возвращается Some(Err(последняя_ошибка)) — вызывающий код может
        // различить "исчерпали попытки, получили ошибку" (Some(Err)) от
        // "shutdown отменил до попытки" (None).
        let cfg = ReconnectConfig {
            max_attempts: Some(3),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, &'static str>("always fails")
                }
            },
        )
        .await;
        assert!(matches!(result, Some(Err("always fails"))));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn returns_some_err_on_last_attempt_with_max_2() {
        // Граничный случай: max_attempts=2 → 2 попытки → Some(Err).
        let cfg = ReconnectConfig {
            max_attempts: Some(2),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, &'static str>("boom")
                }
            },
        )
        .await;
        assert!(matches!(result, Some(Err("boom"))));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn returns_none_when_max_attempts_is_zero() {
        // Edge case: max_attempts=0 → ни одной попытки → None.
        // Полезно для override'а из профиля (отключить auto-reconnect).
        let cfg = ReconnectConfig {
            max_attempts: Some(0),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, &'static str>("x")
                }
            },
        )
        .await;
        assert!(result.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn returns_none_when_shutdown_cancelled() {
        let cfg = ReconnectConfig {
            max_attempts: Some(10),
            initial_backoff_ms: 10,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        // Отдельный клон для использования внутри FnMut-замыкания (вызывается
        // много раз). CancellationToken.cancel() принимает &self, но
        // перемещать `shutdown` в `async move` нельзя — FnMut требует
        // возможности вызвать замыкание повторно.
        let shutdown_signal = Arc::new(shutdown.clone());
        let cancel_after = Arc::new(AtomicU32::new(0));
        let cancel_after_cl = cancel_after.clone();
        let shutdown_signal_cl = shutdown_signal.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let ca = cancel_after_cl.clone();
                let sig = shutdown_signal_cl.clone();
                async move {
                    let n = ca.fetch_add(1, Ordering::SeqCst);
                    if n == 1 {
                        // Через одну неудачу отменяем shutdown.
                        sig.cancel();
                    }
                    Err::<i32, &'static str>("boom")
                }
            },
        )
        .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn infinite_retries_when_max_attempts_none() {
        let cfg = ReconnectConfig {
            max_attempts: None,
            initial_backoff_ms: 1,
            max_backoff_ms: 10,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        shutdown.cancel(); // сразу отменён — должен сразу выйти
        let calls = Arc::new(AtomicU32::new(0));
        let calls_cl = calls.clone();
        let result = reconnect_with_backoff(
            &cfg,
            &shutdown,
            || {},
            move || {
                let c = calls_cl.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, &'static str>("x")
                }
            },
        )
        .await;
        assert!(result.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn on_attempt_callback_invoked_per_try() {
        let cfg = ReconnectConfig {
            max_attempts: Some(3),
            initial_backoff_ms: 5,
            max_backoff_ms: 100,
            multiplier: 2.0,
        };
        let shutdown = CancellationToken::new();
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_cl = attempts.clone();
        let _ = reconnect_with_backoff(
            &cfg,
            &shutdown,
            move || {
                attempts_cl.fetch_add(1, Ordering::SeqCst);
            },
            || async { Err::<i32, &'static str>("x") },
        )
        .await;
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn validate_catches_bad_params() {
        // initial = 0 — бессмысленно (нет задержки).
        assert!(ReconnectConfig {
            initial_backoff_ms: 0,
            ..ReconnectConfig::default()
        }
        .validate()
        .is_err());

        // max < initial — нарушение инварианта.
        assert!(ReconnectConfig {
            initial_backoff_ms: 100,
            max_backoff_ms: 50,
            ..ReconnectConfig::default()
        }
        .validate()
        .is_err());

        // multiplier < 1 — backoff не растёт, а уменьшается.
        assert!(ReconnectConfig {
            multiplier: 0.5,
            ..ReconnectConfig::default()
        }
        .validate()
        .is_err());

        // NaN multiplier — невалидно.
        assert!(ReconnectConfig {
            multiplier: f64::NAN,
            ..ReconnectConfig::default()
        }
        .validate()
        .is_err());

        // Валидный дефолт.
        assert!(ReconnectConfig::default().validate().is_ok());
    }

    #[test]
    fn resolve_fills_defaults() {
        let r = ReconnectConfig::resolve(None, None, None, None);
        assert_eq!(r.initial_backoff_ms, 100);
        assert_eq!(r.max_backoff_ms, 30_000);
        assert_eq!(r.multiplier, 2.0);
        assert_eq!(r.max_attempts, None);

        let r = ReconnectConfig::resolve(Some(5), Some(50), Some(1000), Some(3.0));
        assert_eq!(r.max_attempts, Some(5));
        assert_eq!(r.initial_backoff_ms, 50);
        assert_eq!(r.max_backoff_ms, 1000);
        assert_eq!(r.multiplier, 3.0);
    }
}
