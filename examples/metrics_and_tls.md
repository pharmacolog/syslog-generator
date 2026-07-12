# F12 (/metrics) и безопасный TLS (N4) — примеры (v8.2.0)

## HTTP-эндпоинт /metrics (F12)

Поднять экспортер Prometheus на всё время прогона можно двумя способами.

### Через CLI-флаг

```bash
# запускаем генератор с /metrics на 127.0.0.1:9090
syslog-generator -p examples/single_target.json --metrics-addr 127.0.0.1:9090

# в другом терминале — снимаем метрики
curl -s http://127.0.0.1:9090/metrics    # 200, Prometheus text (v0.0.4)
curl -s http://127.0.0.1:9090/           # алиас /metrics
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:9090/nope   # 404
```

### Через профиль

```json
{
  "metrics_addr": "127.0.0.1:9090",
  "targets": [{ "address": "/tmp/out.log", "transport": "file" }],
  "phases": [
    { "name": "load", "duration_secs": 30, "messages_per_second": 500,
      "format": "raw", "templates": ["msg {{sequence}}"] }
  ]
}
```

Флаг `--metrics-addr` переопределяет поле профиля `metrics_addr`.
Сервер гасится автоматически по завершении всех фаз. Если порт занят или
адрес некорректен — это логируется в stderr, но генерация не прерывается
(метрики — вспомогательный канал).

Пример scrape-конфига Prometheus:

```yaml
scrape_configs:
  - job_name: syslog-generator
    static_configs:
      - targets: ['127.0.0.1:9090']
```

## Безопасный TLS (N4)

По умолчанию TLS-транспорт **проверяет** сертификат сервера и его имя.

### Публичный/корпоративный CA (проверка «из коробки»)

```json
{ "address": "syslog.example.com:6514", "transport": "tls" }
```

`tls_domain` не указан → имя для проверки берётся из хост-части `address`
(`syslog.example.com`).

### Self-signed или приватный CA

```json
{
  "address": "10.0.0.5:6514",
  "transport": "tls",
  "tls_domain": "syslog.internal",
  "tls_ca_file": "/etc/ssl/my-ca.pem"
}
```

CA из `tls_ca_file` добавляется к системным доверенным корням; имя
проверяется против `tls_domain`. Несуществующий файл CA отклоняется
валидацией (`--validate` → код возврата `1`).

### Небезопасный режим (только тестовые стенды)

```json
{ "address": "127.0.0.1:6514", "transport": "tls", "tls_insecure": true }
```

`tls_insecure: true` полностью отключает проверку сертификата и имени
(danger). При запуске в stderr печатается предупреждение:

```
⚠ TLS (127.0.0.1:6514): tls_insecure=true — проверка сертификата ОТКЛЮЧЕНА (небезопасно)
```

| Поле | По умолчанию | Назначение |
|------|--------------|-----------|
| `tls_domain` | хост-часть `address` | SNI и проверка имени сервера |
| `tls_ca_file` | — | PEM доверенного CA (self-signed/приватный CA) |
| `tls_insecure` | `false` | `true` — ОТКЛЮЧИТЬ проверку (небезопасно) |
