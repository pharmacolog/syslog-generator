
# USER GUIDE

Версия документа: `v7.4.0`.

## Возможности

- Profile/phases execution.
- Multi-target dispatch: `broadcast`, `round-robin`, `weighted`.
- Runtime sender для `file`, `tcp`, `udp`, `tls`.
- Реальный TLS client handshake/runtime.
- Генерация сообщений из `templates`, `templates_file`, `schema_file`.
- Protobuf-like serialization через `format: protobuf`.
- Graceful shutdown с drain.
- Prometheus metrics.

## Test coverage

Проект теперь покрывает mixed multi-target end-to-end сценарии для:

- `broadcast`
- `round-robin`
- `weighted`

Также есть negative-path тесты для transport-level connection failures, чтобы проверять наблюдаемость ошибок и частичную деградацию mixed profile execution.
