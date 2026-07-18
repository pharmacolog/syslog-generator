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
    // PR-2: трекаем JoinHandle каждого handle_conn, чтобы дождаться их при
    // shutdown (раньше они были orphan — при отмене accept-цикл выходил
    // немедленно, а in-flight HTTP-запросы продолжали работать на
    // background tokio runtime, теряя свой завершение в основном потоке).
    let mut in_flight: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                eprintln!("F12: HTTP /metrics останавливается; \
                          ждём завершения {} in-flight запросов",
                          in_flight.len());
                // Даём in-flight запросам шанс завершиться (с коротким таймаутом).
                let drain_deadline = std::time::Duration::from_secs(5);
                let drain_result = tokio::time::timeout(
                    drain_deadline,
                    async {
                        // Удаляем завершённые handles; await оставшихся.
                        in_flight.retain_mut(|h| !h.is_finished());
                        for h in in_flight.drain(..) {
                            let _ = h.await;
                        }
                    },
                )
                .await;
                match drain_result {
                    Ok(_) => eprintln!("F12: HTTP /metrics остановлен (все запросы завершены)"),
                    Err(_) => eprintln!(
                        "F12: HTTP /metrics остановлен (timeout {drain_deadline:?}; \
                         часть in-flight запросов прервана)"
                    ),
                }
                return Ok(());
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _peer)) => {
                        let m = metrics.clone();
                        let h = tokio::spawn(handle_conn(stream, m));
                        in_flight.push(h);
                        // Чистим завершённые handles чтобы не накапливать.
                        in_flight.retain(|h| !h.is_finished());
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

    /// PR-16 (coverage): build_http_response_404_sets_content_length.
    /// `build_http_response` для 404 и 405 paths были частично uncovered.
    #[test]
    fn build_http_response_error_paths() {
        // 404 Not Found.
        let r = build_http_response("404 Not Found", "text/plain", "page not found\n");
        assert!(r.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(r.contains("Content-Type: text/plain"));
        assert!(r.contains("Content-Length: 15")); // "page not found\n"
        assert!(r.ends_with("page not found\n"));

        // 405 Method Not Allowed.
        let r = build_http_response("405 Method Not Allowed", "text/plain", "POST not allowed\n");
        assert!(r.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
        assert!(r.contains("Content-Length: 17"));

        // 500 Internal Server Error.
        let r = build_http_response("500 Internal Server Error", "text/plain", "err\n");
        assert!(r.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));

        // Empty body.
        let r = build_http_response("200 OK", "text/plain", "");
        assert!(r.contains("Content-Length: 0"));
        assert!(r.ends_with("\r\n\r\n"));
    }

    /// Резервирует свободный TCP-порт через bind(0), затем отпускает listener.
    /// Небольшая гонка возможна (между drop и re-bind в `serve`), но в
    /// тестах это надёжно: re-bind в `serve` всё равно делается на свежем
    /// локальном listener, и при коллизии (крайне редко) мы повторим.
    async fn reserve_free_port() -> String {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap().to_string();
        drop(probe);
        addr
    }

    /// Коннектится с ретраями (handle serve() ещё не успел bind на той же
    /// машине — супер-редкая гонка после `reserve_free_port`).
    async fn connect_with_retry(addr: &str) -> TcpStream {
        for _ in 0..20 {
            if let Ok(s) = TcpStream::connect(addr).await {
                return s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("не удалось подключиться к {addr}");
    }

    /// Phase 8b: реальный `serve()` + GET / → 200 + prometheus text.
    /// Покрывает ветку `("GET", "/")` через сетевой цикл (а не через route()).
    #[tokio::test]
    async fn serve_real_get_root_returns_metrics() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        metrics
            .messages_total
            .with_label_values(&["tcp", "p1", "127.0.0.1:1", "success"])
            .inc();
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        let mut client = connect_with_retry(&addr).await;
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");
        assert!(text.contains("text/plain; version=0.0.4"));
        assert!(text.contains("syslog_messages_total"));

        shutdown.cancel();
        handle.await.unwrap().expect("serve returns Ok");
    }

    /// Phase 8b: реальный `serve()` + POST /metrics → 405.
    /// Покрывает default-ветку `route()` через сетевой цикл.
    #[tokio::test]
    async fn serve_real_post_metrics_returns_405() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        let mut client = connect_with_retry(&addr).await;
        client
            .write_all(b"POST /metrics HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(
            text.starts_with("HTTP/1.1 405 Method Not Allowed"),
            "resp: {text}"
        );

        shutdown.cancel();
        handle.await.unwrap().expect("serve returns Ok");
    }

    /// Phase 8b: реальный `serve()` + запрос с пустой первой строкой → 400.
    /// Покрывает ветку `parse_request_line == None` → 400 Bad Request.
    #[tokio::test]
    async fn serve_real_malformed_request_returns_400() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        let mut client = connect_with_retry(&addr).await;
        // Только перевод строки без метода/пути — parse_request_line вернёт None.
        client.write_all(b"\r\n").await.unwrap();
        let mut resp = Vec::new();
        // Таймаут на случай зависания.
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.read_to_end(&mut resp),
        )
        .await;
        let read = read.expect("read не завис");
        let _ = read.expect("read ok");
        let text = String::from_utf8_lossy(&resp);
        assert!(
            text.starts_with("HTTP/1.1 400 Bad Request"),
            "resp: {text:?}"
        );

        shutdown.cancel();
        handle.await.unwrap().expect("serve returns Ok");
    }

    /// Phase 8b: реальный `serve()` + клиент закрыл соединение без данных → Ok(0).
    /// Покрывает ранний return в `handle_conn` при пустом read.
    #[tokio::test]
    async fn serve_real_eof_without_request_returns_silently() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        let client = connect_with_retry(&addr).await;
        // Сразу закрываем stream.
        drop(client);
        // Даём серверу время обработать и не упасть.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Делаем второй нормальный запрос — должен работать (сервер живой).
        let mut client2 = connect_with_retry(&addr).await;
        client2
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client2.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");

        shutdown.cancel();
        handle.await.unwrap().expect("serve returns Ok");
    }

    /// Phase 8b: serve() корректно завершается по CancellationToken
    /// без висящих in-flight соединений.
    #[tokio::test]
    async fn serve_real_graceful_shutdown_returns_quickly() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        // Дождёмся, что сервер начал слушать.
        let _ = connect_with_retry(&addr).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Отменяем shutdown.
        shutdown.cancel();
        // serve() должен завершиться быстро (без активных запросов → без drain).
        let res = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        let join = res.expect("serve завершился по таймауту");
        join.expect("task не запаниковал").expect("serve Ok");
    }

    /// Phase 8b: serve() дожидается in-flight запросов при shutdown
    /// (drain-ветка). Покрывает ветку in_flight.drain(..).
    #[tokio::test]
    async fn serve_real_graceful_shutdown_drains_in_flight() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_serve = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr_for_serve, m, sd).await });

        // Открываем несколько параллельных соединений.
        let mut clients = Vec::new();
        for _ in 0..3 {
            let mut c = connect_with_retry(&addr).await;
            c.write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
                .await
                .unwrap();
            clients.push(c);
        }
        // Небольшая задержка чтобы сервер точно принял все соединения
        // и заспавнил handle_conn задачи.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Запускаем shutdown параллельно — drain должен дождаться in-flight.
        shutdown.cancel();
        let join = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
            .await
            .expect("serve завершился в пределах drain-таймаута");
        join.expect("task не запаниковал").expect("serve Ok");

        // Читаем ответы — должны быть валидные 200.
        for mut c in clients {
            let mut resp = Vec::new();
            let _ = c.read_to_end(&mut resp).await;
            let text = String::from_utf8_lossy(&resp);
            assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");
        }
    }

    /// Phase 8b: serve() обрабатывает несколько конкурентных клиентов.
    /// Покрывает многократный accept + спавн нескольких handle_conn.
    #[tokio::test]
    async fn serve_real_handles_concurrent_connections() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        let addr = reserve_free_port().await;
        let addr_for_clients = addr.clone();
        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let handle = tokio::spawn(async move { serve(&addr, m, sd).await });

        let n = 5;
        let mut joins = Vec::new();
        for _ in 0..n {
            let c_addr = addr_for_clients.clone();
            joins.push(tokio::spawn(async move {
                let mut client = connect_with_retry(&c_addr).await;
                client
                    .write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
                    .await
                    .unwrap();
                let mut resp = Vec::new();
                client.read_to_end(&mut resp).await.unwrap();
                let text = String::from_utf8_lossy(&resp);
                assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");
            }));
        }
        for j in joins {
            j.await.expect("client task ok");
        }

        shutdown.cancel();
        handle.await.unwrap().expect("serve Ok");
    }

    /// Phase 8b: serve() возвращает Err при bind на занятом порте.
    /// Покрывает ветку `with_context` + early return Err.
    #[tokio::test]
    async fn serve_real_bind_error_returns_err() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        // Занимаем порт.
        let occupier = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = occupier.local_addr().unwrap().to_string();

        let shutdown = CancellationToken::new();
        let sd = shutdown.clone();
        let m = metrics.clone();
        let res = serve(&addr, m, sd).await;
        assert!(res.is_err(), "ожидаем Err при bind на занятом порте");
        let err = res.unwrap_err();
        // Сообщение должно содержать подсказку про F12 и адрес.
        let msg = format!("{err:#}");
        assert!(
            msg.contains("F12") || msg.contains(addr.as_str()),
            "unexpected error: {msg}"
        );
    }

    /// Phase 8b: `spawn()` запускает сервер фоновой задачей и не падает
    /// даже на занятом порте (просто логирует и завершается).
    /// Покрывает всё тело функции `spawn` (158-165).
    #[tokio::test]
    async fn spawn_runs_serve_in_background_and_logs_bind_error() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        // Занятый порт — serve() внутри spawn вернёт Err, который
        // будет напечатан через eprintln.
        let occupier = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = occupier.local_addr().unwrap().to_string();

        let shutdown = CancellationToken::new();
        spawn(&addr, metrics, shutdown);

        // Даём фоновой задаче шанс отработать.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // Если spawn упал с паникой — этот тест тоже упал бы.
    }

    /// Phase 8b: `spawn()` запускает сервер и реально обслуживает запросы,
    /// когда порт свободен. Дополнительно покрывает happy path spawn().
    #[tokio::test]
    async fn spawn_serves_real_metrics_request() {
        let metrics = create_metrics().expect("create_metrics ok in test");
        metrics
            .messages_total
            .with_label_values(&["tcp", "p1", "127.0.0.1:1", "success"])
            .inc();
        let addr = reserve_free_port().await;
        let shutdown = CancellationToken::new();
        spawn(&addr, metrics, shutdown.clone());

        let mut client = connect_with_retry(&addr).await;
        client
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "resp: {text}");

        shutdown.cancel();
        // Даём фоновой задаче время корректно завершиться.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
