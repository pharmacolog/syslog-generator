//! F12: лёгкий HTTP-эндпоинт для экспорта метрик в формате Prometheus.
//!
//! Реализован на голом `tokio` (TcpListener + ручной разбор строки запроса),
//! без тяжёлых зависимостей (hyper/axum). Обслуживает только `GET /metrics`
//! (и `GET /` как алиас), остальное — 404. Формат тела — стандартный
//! Prometheus text exposition (из `metrics::gather_metrics`).
//!
//! Сервер запускается фоновой задачей на всё время прогона профиля и
//! останавливается по CancellationToken при завершении/shutdown.

use crate::observability::metrics::{gather_metrics, Metrics};
use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// Content-Type для Prometheus text exposition format (v0.0.4).
const PROM_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Разбирает первую строку HTTP-запроса и возвращает (method, path).
/// Пример: "GET /metrics HTTP/1.1" → ("GET", "/metrics"). None при пустом вводе.
pub fn parse_request_line(buf: &[u8]) -> Option<(String, String)> {
    let text = String::from_utf8_lossy(buf);
    let first = text.lines().next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let raw_path = parts.next()?;
    // Отбрасываем query-string (?foo=bar) — маршрутизация только по пути.
    let path = raw_path.split('?').next().unwrap_or(raw_path).to_string();
    Some((method, path))
}

/// Формирует полный HTTP-ответ (заголовки + тело) для заданных статуса/типа/тела.
pub fn build_http_response(status_line: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Маршрутизация: возвращает готовый HTTP-ответ для (method, path).
/// GET /metrics и GET / → метрики; всё остальное → 404.
///
/// N7: `gather_metrics` теперь возвращает `Result<String, MetricsError>`.
/// При ошибке кодирования (теоретически невозможно, но тип требует обработки)
/// возвращаем 500 с описанием — это лучше, чем паника или пустое тело.
pub fn route(method: &str, path: &str, metrics: &Metrics) -> String {
    match (method, path) {
        ("GET", "/metrics") | ("GET", "/") => match gather_metrics(metrics) {
            Ok(body) => build_http_response("200 OK", PROM_CONTENT_TYPE, &body),
            Err(e) => build_http_response(
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                &format!("500 Internal Server Error: ошибка экспорта метрик: {e}\n"),
            ),
        },
        ("GET", _) => build_http_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            "404 Not Found: используйте GET /metrics\n",
        ),
        _ => build_http_response(
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "405 Method Not Allowed: поддерживается только GET\n",
        ),
    }
}

/// Обрабатывает одно соединение: читает запрос, маршрутизирует, пишет ответ.
async fn handle_conn(mut stream: TcpStream, metrics: Metrics) {
    let mut buf = [0u8; 2048];
    // Достаточно прочитать первую строку запроса; тело GET нам не нужно.
    let n = match stream.read(&mut buf).await {
        Ok(0) => return,
        Ok(n) => n,
        Err(_) => return,
    };
    let response = match parse_request_line(&buf[..n]) {
        Some((method, path)) => route(&method, &path, &metrics),
        None => build_http_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "400 Bad Request\n",
        ),
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Запускает HTTP-сервер метрик на `addr`. Возвращает фактический адрес
/// прослушивания (полезно при порте 0 — ОС выбирает свободный) через
/// возвращаемый `TcpListener`-адрес. Цикл принятия соединений работает до
/// срабатывания `shutdown`.
pub async fn serve(addr: &str, metrics: Metrics, shutdown: CancellationToken) -> Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("F12: не удалось привязать HTTP /metrics к {addr}"))?;
    let local = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| addr.to_string());
    eprintln!("F12: HTTP /metrics слушает на http://{local}/metrics");
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                eprintln!("F12: HTTP /metrics остановлен");
                return Ok(());
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _peer)) => {
                        let m = metrics.clone();
                        tokio::spawn(handle_conn(stream, m));
                    }
                    Err(e) => {
                        eprintln!("F12: ошибка accept на /metrics: {e}");
                    }
                }
            }
        }
    }
}

/// Удобный вход: спавнит `serve` фоновой задачей и сразу возвращает управление.
/// Ошибку привязки логирует, но не прерывает прогон (метрики — вспомогательный
/// канал, их недоступность не должна ронять генератор).
pub fn spawn(addr: &str, metrics: Metrics, shutdown: CancellationToken) {
    let addr = addr.to_string();
    tokio::spawn(async move {
        if let Err(e) = serve(&addr, metrics, shutdown).await {
            eprintln!("F12: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::metrics::create_metrics;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[test]
    fn test_parse_request_line_ok() {
        let (m, p) = parse_request_line(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(m, "GET");
        assert_eq!(p, "/metrics");
    }

    #[test]
    fn test_parse_request_line_strips_query() {
        let (m, p) = parse_request_line(b"GET /metrics?foo=bar HTTP/1.1\r\n").unwrap();
        assert_eq!(m, "GET");
        assert_eq!(p, "/metrics");
    }

    #[test]
    fn test_parse_request_line_empty() {
        assert!(parse_request_line(b"").is_none());
    }

    #[test]
    fn test_route_metrics_200() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        // CounterVec без наблюдённых меток не экспортируется, поэтому
        // сначала инкрементируем одну серию.
        metrics
            .messages_total
            .with_label_values(&["tcp", "p1", "127.0.0.1:1", "success"])
            .inc();
        let resp = route("GET", "/metrics", &metrics);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("text/plain; version=0.0.4"));
        // Prometheus-текст содержит имена наших метрик.
        assert!(resp.contains("syslog_messages_total"));
        // Всегда-присутствующая скалярная метрика.
        assert!(resp.contains("syslog_shutdowns_total"));
    }

    #[test]
    fn test_route_root_alias_200() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let resp = route("GET", "/", &metrics);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
    }

    #[test]
    fn test_route_unknown_404() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let resp = route("GET", "/healthz", &metrics);
        assert!(resp.starts_with("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn test_route_non_get_405() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let resp = route("POST", "/metrics", &metrics);
        assert!(resp.starts_with("HTTP/1.1 405 Method Not Allowed"));
    }

    #[test]
    fn test_build_http_response_content_length() {
        let resp = build_http_response("200 OK", "text/plain", "hello");
        assert!(resp.contains("Content-Length: 5"));
        assert!(resp.ends_with("hello"));
    }

    /// Полный сетевой цикл: поднимаем сервер на 127.0.0.1:0, делаем реальный
    /// GET /metrics и проверяем, что получили prometheus-текст.
    #[tokio::test]
    async fn test_serve_real_get_metrics() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        // Увеличим счётчик, чтобы вывод был непустым.
        metrics
            .messages_total
            .with_label_values(&["tcp", "p1", "127.0.0.1:1", "success"])
            .inc();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        // Запускаем accept-цикл вручную (используем уже привязанный listener).
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sd.cancelled() => return,
                    accepted = listener.accept() => {
                        if let Ok((stream, _)) = accepted {
                            tokio::spawn(handle_conn(stream, m.clone()));
                        }
                    }
                }
            }
        });

        let mut client = TcpStream::connect(&addr).await.unwrap();
        client
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");
        assert!(text.contains("syslog_messages_total"));
        shutdown.cancel();
    }

    /// Реальный GET на неизвестный путь → 404.
    #[tokio::test]
    async fn test_serve_real_get_404() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sd.cancelled() => return,
                    accepted = listener.accept() => {
                        if let Ok((stream, _)) = accepted {
                            tokio::spawn(handle_conn(stream, m.clone()));
                        }
                    }
                }
            }
        });
        let mut client = TcpStream::connect(&addr).await.unwrap();
        client
            .write_all(b"GET /nope HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 404 Not Found"), "resp: {text}");
        shutdown.cancel();
    }
}
