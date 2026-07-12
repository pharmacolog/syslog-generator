
# DEVELOPER GUIDE

Версия документа: `v7.4.0`.

## Модульная архитектура

- `config` — модели профиля (`Profile`, `Phase`, `TargetConfig`, `ShutdownConfig`).
- `core` — генерация сообщений (`generate_message`), диспетчеризация (`create_dispatcher`),
  запуск фаз и профиля (`run_phase_multi`, `run_profile`).
- `sender` — transport runtime: `target_sender_file` / `_tcp` / `_udp` / `_tls`, учёт метрик.
- `schema` — загрузка внешних JSON schema-подобных описаний для генерации данных.
- `template` — подстановка `{{placeholder}}`.
- `protobuf` — protobuf-подобная сериализация по field-map.
- `metrics` — Prometheus registry и метрики.
- `shutdown` — graceful drain и обработка сигналов.

## Test architecture

`tests/integration_tests.rs` включает:

- unit-тесты рендеринга шаблонов, диспетчера, protobuf-маппинга, генерации сообщений;
- broadcast mixed transport e2e (`file + tcp + udp + tls`);
- round-robin mixed transport e2e;
- weighted mixed transport e2e;
- negative-path тест для connection failures;
- проверку экспорта метрик после реальной активности.

Collector helpers поднимают локальные TCP, UDP и TLS endpoint'ы и позволяют проверять
фактическую доставку сообщений по transport-specific runtime path. TLS-коллектор
генерирует self-signed сертификат через `rcgen` 0.13 (`cert.key_pair.serialize_pem()`,
`cert.cert.pem()`) и строит `native_tls::Identity::from_pkcs8(cert_pem, key_pem)` — оба
аргумента в PEM.

## Бенчмарки

Объявлены в `Cargo.toml` как `[[bench]]` с `harness = false` и используют Criterion
(feature `async_tokio` для async-кейсов):

- `benches/message_generation.rs` — `render_template`, `generate_message`,
  `create_dispatcher`.
- `benches/sender_throughput.rs` — throughput отправки через `run_profile` с реальными
  TCP/UDP коллекторами.

## Поведение метрик

Prometheus text-формат не экспортирует label-less `CounterVec` серии, пока не наблюдался
хотя бы один набор меток. Поэтому `syslog_messages_total`, `syslog_bytes_total`,
`syslog_errors_total`, `syslog_messages_by_sink_total`, `syslog_messages_drained_total`
появляются в выводе только после первой соответствующей операции. Histogram'ы
(`syslog_generate_duration_seconds`, `syslog_drain_duration_seconds`) и скалярные счётчики
(`syslog_shutdowns_total`, `syslog_drain_timeouts_total`) экспортируются всегда, даже при
нулевом значении.

## Обработка ошибок транспорта

При неудаче TCP-подключения, TLS-подключения или TLS-handshake соответствующий sender
фиксирует `record_error` (метрика `syslog_errors_total`) и дренирует входную очередь,
помечая каждое сообщение как ошибку, чтобы не блокировать генератор и корректно завершить
фазу.

## Quality state

`v7.4.0` — compile-verified. Проверено реальной компиляцией:
`cargo build --release`, `cargo test` (9/9 зелёных, стабильны на 5 прогонах),
`cargo bench` (реальные измерения), `cargo clippy --all-targets` (без предупреждений).
