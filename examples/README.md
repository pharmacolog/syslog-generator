
# Examples

- `single_target.json` — минимальный профиль.
- `file_sink.json` — запись в файл.
- `multi_target_broadcast.json` — один message fan-out во все target.
- `multi_target_roundrobin.json` — циклическое распределение по target.
- `multi_target_weighted.json` — weighted dispatch между target.
- `graceful_shutdown.json` — пример shutdown/drain поведения.
- `protobuf_format.json` — protobuf-like output format.
- `schema_auth.json`, `schema_nginx.json` — schema-driven generation.
- `load_shape_ramp.json`, `load_shape_sine.json`, `load_shape_burst.json` — профили нагрузки во времени (F3): ramp/синусоида/всплески.
- `variable_payload_seeded.json` — вариативный пейлоад с `seed` и мультишаблонами с весами (F4/F5/F14).
- `variable_payload_schema.json` + `schema_http_access.json` — HTTP access-log через schema с распределениями weighted/zipf и паддингом (F5/F6).
- `cli_quickstart.md` — быстрый старт расширенного CLI и валидации (F11/F13, v8.1.0).
- `metrics_and_tls.md` — HTTP-эндпоинт `/metrics` (F12) и безопасный TLS с CA/SNI/insecure (N4, v8.2.0).
- `profile-f17-anomalies.yaml` — сценарии аномалий нагрузки (F17, v9.4.0): burst-injection (×10 каждые 30с), slow-drip (÷5 первые 60с), packet-loss (20% дропов). Для тестирования SIEM-правил и MITRE ATT&CK-подобных последовательностей.

Начиная с `v7.4.0`, mixed transport matrix покрыта тестами для `broadcast`, `round-robin` и `weighted`, а также есть negative-path сценарии для transport failures.
