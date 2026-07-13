# Аудит проекта syslog-generator (базис v7.4.0) и план развития до промышленного генератора нагрузки

Дата аудита: 2026-07-11. Базис аудита: реальный компилируемый код v7.4.0 (проверен `cargo build/test/bench/clippy`), документация (`README.md`, `docs/`, `examples/`, `REVIEW.md`), Grafana-дашборд.

> **Статус на v9.3.0 (2026-07-13):** вехи A, B, C, D закрыты полностью
> (v9.0.0). **Веха E (P2 «Зрелость») в процессе.** N10 (v9.1.0) +
> F15 (v9.2.0, CEF/LEEF/JSON-lines) + **F16 (v9.3.0)** сделаны. F16:
> Kafka/Redpanda transport через `rskafka` (opt-in feature `kafka`) +
> файловая ротация (size/time/max_files с LRU cleanup) + exponential
> backoff reconnect с jitter для TCP/TLS. 0 breaking changes.
> Следующие: v9.4.0 (F17: сценарии аномалий), v9.5.0 (N4.cipher_policy
> + rustls), v9.6.0 (N12: Docker/musl/compose).
> Ранее отложенные опциональные задачи A/B/C (F5 regex, F6 корреляции,
> F10 честный protobuf, N3 метрики) закрыты в v8.0.0.
> Разделы 1–2 ниже описывают исходное
> состояние v7.4.0 (исторический снимок); актуальный прогресс — в
> разделах 4 (по-фичево) и 5 (roadmap).

Цель проекта (по формулировке заказчика): промышленный генератор нагрузки для syslog с различными профилями и глубокой кастомизацией вариативного пейлоада.

---

## 1. Резюме для руководителя

Проект компилируется, имеет чистую модульную архитектуру и проходит тесты, но по факту это **функциональный прототип / демонстрация архитектуры**, а не промышленный генератор нагрузки. Между заявленными возможностями и реализацией есть системный разрыв по трём осям, критичным именно для нагрузочного инструмента:

1. **Нагрузка не создаётся.** Генерация жёстко ограничена ≤100 сообщениями на фазу, нет rate-limiting, нет длительности прогона, нет параллелизма соединений. Это исключает нагрузочный сценарий как класс.
2. **Пейлоад не вариативен.** «faker», «enum», «int», «datetime» возвращают фиксированные константы; нет ГПСЧ, нет seed-детерминизма, нет распределений. Заявленная «глубокая кастомизация вариативного пейлоада» отсутствует.
3. **Формат не соответствует syslog.** Реальный RFC 5424 / RFC 3164 не реализованы (PRI/severity/facility/version/structured-data/timestamp игнорируются), TCP/TLS не используют framing по [RFC 6587](https://datatracker.ietf.org/doc/html/rfc6587)/[RFC 5425](https://www.rfc-editor.org/rfc/rfc5425.txt). Приёмник-syslog не распарсит вывод как syslog.

Ниже — детальный разбор «заявлено vs реализовано» и приоритизированный план (функциональные и нефункциональные характеристики).

---

## 2. Матрица «Заявлено vs Реализовано»

| Возможность (заявлено в доках/примерах/дашборде) | Реализация в коде | Статус |
|---|---|---|
| Profile/phases execution | `run_profile` → `run_phase_multi` последовательно по фазам | ✅ Есть |
| Multi-target dispatch `broadcast` / `round-robin` / `weighted` | `create_dispatcher` + ветвление в `run_phase_multi` | ✅ Есть (но round-robin/weighted завязаны на порядок в `dispatch`) |
| Sender `file` / `tcp` / `udp` / `tls` | 4 функции в `sender.rs`, реальный async I/O | ✅ Есть |
| Реальный TLS client handshake | `native-tls` / `tokio-native-tls`, проверка сертификата включена по умолчанию, `tls_ca_file`/`tls_domain`/`tls_insecure` | ✅ Безопасно по умолчанию (N4, v8.2.0) |
| Генерация из `templates` / `templates_file` / `schema_file` | `load_templates`, `load_schema`, `render_template` | ⚠️ Частично: используется только **первый** шаблон (`templates.first()`), остальные игнорируются |
| Protobuf-like serialization (`format: protobuf`) | `serialize_protobuf_like` → это `serde_json::to_vec` map | ❌ Это JSON, не protobuf; вводит в заблуждение |
| Graceful shutdown + drain | `shutdown_listener` (ctrl_c), `graceful_drain_wait` с таймаутом | ✅ Есть |
| Prometheus metrics | `metrics.rs`, registry, `gather_metrics`, HTTP `/metrics` (`metrics_server.rs`) | ✅ HTTP-экспорт (F12, v8.2.0) |
| `messages_per_second` (rate) | `total = messages_per_second.min(100)` — трактуется как **количество**, не как rate | ❌ Не rate; жёсткий потолок 100 |
| `duration_secs` (длительность фазы) | Объявлено в `Phase`, **нигде не используется** | ❌ Мёртвое поле |
| `seed` (детерминизм) | Объявлено в `Phase`, **нигде не используется** | ❌ Мёртвое поле |
| `connections` (пул соединений на target) | Объявлено в `TargetConfig`, **нигде не используется** (всегда 1 канал) | ❌ Мёртвое поле |
| `faker.ipv4/username/uuid`, `enum`, `int`, `datetime` | Возвращают статические константы («192.0.2.10», «alice», первый элемент, `min`, фикс. дата) | ❌ Нет вариативности |
| Формат RFC 5424 (`format_type()` по умолчанию) | Не реализован: нет PRI/VERSION/TIMESTAMP/STRUCTURED-DATA. `format` кроме `protobuf` игнорируется | ❌ Отсутствует |
| Формат RFC 3164 (BSD) | Не реализован | ❌ Отсутствует |
| Grafana: `syslog_cpu_usage_percent`, `syslog_memory_usage_bytes` | Gauge объявлены, но никогда не обновляются (всегда 0) | ❌ Мёртвые метрики |
| Grafana: `syslog_message_size_bytes_bucket` | Метрики нет в коде | ❌ Отсутствует |
| Grafana: `syslog_messages_by_format_total` | Метрики нет в коде | ❌ Отсутствует |
| Grafana: `syslog_active_workers` | Метрики нет в коде | ❌ Отсутствует |

---

## 3. Детальный разбор ключевых разрывов

### 3.1. Нагрузка (критично для цели проекта)

Фрагмент `core.rs`:

```rust
let total = if phase.messages_per_second == 0 { 1 } else { phase.messages_per_second.min(100) as usize };
for seq in 1..=total { ... }
```

Последствия:
- Максимум 100 сообщений за фазу вне зависимости от конфигурации — нагрузочный тест невозможен.
- `messages_per_second` семантически неверно: это не «в секунду», а «всего», причём с потолком.
- Нет rate-limiting: сообщения отправляются в цикле максимально быстро, «per second» не соблюдается.
- `duration_secs` не влияет ни на что — фаза не длится по времени.
- `connections` не создаёт параллельных соединений/воркеров — на каждый target ровно один канал `mpsc(1024)` и один sender-таск.

Для промышленного генератора это фундаментальные пробелы: нет управления интенсивностью, длительностью, конкурентностью и общим объёмом.

### 3.2. Вариативность пейлоада (критично для цели)

`default_values` и `generate_message` подставляют фиксированные значения; `schema` типы возвращают константы:

```rust
"enum" => field.values.and_then(|v| v.first().cloned())...  // всегда первый элемент
"int"  => field.min.unwrap_or(0).to_string()                // всегда min
"datetime" => "2026-07-11T12:00:00Z"                          // константа
"faker" => "ipv4" => "192.0.2.20" ...                         // константа
```

Нет ГПСЧ, нет диапазонов, нет распределений (uniform/zipf/normal), нет реального faker-набора (IP, hostnames, UUID, user-agent, коды ответов). `seed` не подключён, значит нет воспроизводимости. Только **первый** шаблон из массива используется — нельзя чередовать разнотипные строки. Это прямо противоречит «глубокой кастомизации вариативного пейлоада».

### 3.3. Соответствие формату syslog (критично для корректности)

- `format_type()` возвращает `"rfc5424"` по умолчанию, но никакой RFC-сборки нет: на выходе — сырой рендер шаблона без PRI (`<PRIVAL>`), VERSION, RFC3339-времени, HOSTNAME/APP-NAME/PROCID/MSGID и STRUCTURED-DATA. Приёмник (rsyslog, syslog-ng, Graylog, SIEM) не распознает это как валидный syslog ([RFC 5424](https://www.rfc-editor.org/rfc/rfc5424.html)).
- TCP/TLS отправка добавляет `\n` вручную, но это неполный вариант non-transparent-framing; **octet-counting framing** по [RFC 6587](https://www.rfc-editor.org/rfc/rfc6587.txt) и [RFC 5425](https://www.rfc-editor.org/rfc/rfc5425.txt) (обязателен для syslog-over-TLS, порт 6514, `SYSLOG-FRAME = MSG-LEN SP SYSLOG-MSG`) отсутствует.
- `format: protobuf` фактически сериализует JSON (`serde_json::to_vec`), а не protobuf — терминологически неверно и не совместимо ни с одним protobuf-приёмником.

Для сравнения, промышленные генераторы поддерживают оба формата и framing: `loggen` из [syslog-ng](https://github.com/syslog-ng/syslog-ng/blob/master/tests/loggen/loggen.md) (флаг `-P` для RFC5424, framing-опции), [`flog`](https://github.com/mingrammer/flog) (rfc3164/rfc5424/json).

### 3.4. Наблюдаемость (важно для нефункциональных требований)

- `syslog_cpu_usage_percent`, `syslog_memory_usage_bytes` объявлены, но не заполняются — на дашборде всегда 0.
- Дашборд ссылается на несуществующие метрики: `syslog_message_size_bytes`, `syslog_messages_by_format_total`, `syslog_active_workers`. Панели будут пустыми/сломанными.
- Prometheus не экспортирует label-less `CounterVec` до первой записи (учтено в тестах, но не задокументировано для операторов как ожидаемое поведение — стоит вынести в USER_GUIDE).

### 3.5. Безопасность (важно для промышленного применения)

- TLS клиент жёстко использует `danger_accept_invalid_certs(true)` — MITM-уязвимость, нет опции строгой валидации/CA/SNI/mTLS.
- Нет конфигурации минимальной версии TLS и cipher policy.

### 3.6. Прочие функциональные ограничения

- Нет CLI-управления, кроме `--profile <file>`: нельзя переопределить rate/duration/target из командной строки.
- `main.rs` использует `.expect(...)` — падение с паникой вместо кода возврата и человекочитаемой ошибки; нет `--help` с примерами, нет `--version`.
- Prometheus-эндпоинт (`/metrics`) не поднимается — `gather_metrics` есть, но HTTP-экспортер отсутствует; собирать метрики в реальном прогоне нечем.
- Нет обработки back-pressure/переполнения канала (при медленном приёмнике `mpsc(1024)` блокирует продюсера — это может быть желаемо, но не настраивается и не измеряется).

---

## 4. План улучшений

Приоритеты: **P0** — без этого цель (промышленный нагрузочный генератор) недостижима; **P1** — необходимо для продакшн-готовности; **P2** — зрелость и удобство.

### 4.1. Функциональные характеристики

#### P0 — Ядро нагрузки
- **F1. Настоящий rate-limiting и объём.** Ввести токен-бакет/leaky-bucket (например, `governor`) с `messages_per_second` как истинной интенсивностью; поддержать `duration_secs` и/или `total_messages` как условие остановки. Убрать потолок `min(100)`.
- **F2. Конкурентность соединений.** Реализовать `connections` как пул воркеров/соединений на target (для TCP/TLS — несколько потоков; для UDP — несколько сокетов), с балансировкой генерации между ними.
- **F3. Профили нагрузки во времени. ✅ Сделано (v7.8.0).** Поле фазы `load_shape` задаёт кривую интенсивности: `constant`, `linear` (ramp-up/ramp-down), `sine`, `burst` (spike). Переходы между фазами — через последовательность фаз с разными `load_shape`.

#### P0 — Вариативность пейлоада
- **F4. Реальный ГПСЧ с seed. ✅ Сделано (v7.9.0).** Модуль `payload`: RNG (`rand::StdRng`) выводится из пары `(seed, seq)` через SplitMix64-перемешивание — один и тот же `seed` + номер сообщения дают идентичный вывод (в т.ч. межпроцессно), соседние сообщения различаются; без `seed` — энтропия ОС. Поле `seed` фазы больше не «мёртвое».
- **F5. Богатый набор генераторов данных. ✅ Сделано полностью (v8.0.0).** `faker`: `ipv4`/`ipv6`/`mac`/`uuid`(v4)/`hostname`/`username`/`user_agent`/`url`/`http_status`; `int` с `min..=max`; `enum` со случайным выбором; `datetime` с реальным «сейчас» (RFC3339 UTC) и джиттером `jitter_secs`; `string(len)`. **`regex`-генерация строк реализована** (v8.0.0): поле `"type":"regex"` с `"regex"`, разбор паттерна в HIR (`regex-syntax`) и генерация проектным `StdRng` с сохранением детерминизма по seed; ограничение повторов `REGEX_MAX_REPEAT = 16`, некорректный паттерн → пустая строка. Веса enum перенесены в F6.
- **F6. Распределения и корреляции. ✅ Сделано полностью (v8.0.0).** Распределения выбора для `enum`: `uniform`/`weighted`(по `weights`)/`zipf`(по `zipf_exponent`); паддинг тела до размера через `pad_to_bytes`; мультишаблонность вынесена в F14. **Межполевые корреляции реализованы** (v8.0.0): поле зависит от другого через `depends_on` + `mapping` (значение родителя → значение) и `mapping_default`; двухпроходная генерация (независимые → зависимые), детерминизм по seed сохранён.

#### P0 — Соответствие формату
- **F7. Полноценный RFC 5424. ✅ Сделано (v7.7.0).** Сборка `<PRI>VERSION TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG` с корректным PRI = facility*8+severity, RFC3339-временем, NILVALUE (`-`) для пустых полей, экранированием в STRUCTURED-DATA ([RFC 5424](https://www.rfc-editor.org/rfc/rfc5424.html)). Реализация в `src/format/rfc5424.rs::build()`.
- **F8. RFC 3164 (BSD). ✅ Сделано (v7.7.0).** Классический `<PRI>Mmm dd hh:mm:ssHOSTNAME TAG: MSG` для legacy-приёмников ([Graylog reference](https://graylog.org/post/syslog-protocol-a-reference-guide/)). Реализация в `src/format/rfc3164.rs::build()`.
- **F9. Framing для потоковых транспортов. ✅ Сделано (v7.7.0).** Octet-counting (`MSG-LEN SP SYSLOG-MSG`) и non-transparent-framing (TRAILER = LF/NUL) по [RFC 6587](https://datatracker.ietf.org/doc/html/rfc6587); для TLS — по [RFC 5425](https://www.rfc-editor.org/rfc/rfc5425.txt), дефолтный порт 6514, поддержка сообщений ≥2048 (реком. ≥8192) октет. Реализация в `src/transport/mod.rs::frame_stream()`.
- **F10. Честный protobuf. ✅ Сделано (v8.0.0).** Модуль `protobuf` переписан на реальный wire-format (varint + length-delimited) вместо `serde_json::to_vec`. Теги `(field<<3)|wire_type`, zigzag для sint, length-delimited для строк/байтов. Тип `PbType` (Str/Bytes/Int/Uint/Sint/Bool/Double/Float). Спецификация поля `"номер:тип:шаблон"` (с автонумерацией по алфавиту имён, если номер опущен). Поля сортируются по номеру — канонический детерминированный вывод. Живая проверка: вывод декодируется стандартным разбором varint. Ограничение: файловый `\n`-фрейминг небезопасен для бинарного вывода — для TCP/TLS корректен octet-counting фрейминг.

#### P1 — Управление и интеграция
- **F11. Полноценный CLI. ✅ Сделано (v8.1.0).** Модуль `cli` (`src/cli.rs`): флаги-оверрайды `--target/-t` (повторяемый, `ADDR[:TRANSPORT]`), `--distribution`, `--rate`, `--duration`, `--total`, `--format`, `--seed`, `--message/-m`; команды `--validate` (dry-run) и `--print-config`; `--version` и богатый `--help` с примерами. Быстрый режим без файла-профиля. `main()` переписан на `ExitCode` (коды возврата вместо паник). Оверрайды (`apply_overrides`) — чистая тестируемая функция. Живая проверка: `--version`/`--help`/`--validate`/`--print-config`/быстрый запуск в файл.
- **F12. HTTP-эндпоинт `/metrics`. ✅ Сделано (v8.2.0).** Модуль `metrics_server` (`src/metrics_server.rs`): лёгкий HTTP-сервер на голом `tokio` (без hyper/axum). `GET /metrics` (и `GET /` как алиас) → 200 Prometheus text exposition (v0.0.4); прочие пути → 404, не-GET → 405. Порт настраивается полем профиля `metrics_addr` и флагом `--metrics-addr`. Сервер поднимается фоновой задачей на всё время прогона и гасится по завершении (через `CancellationToken`). Недоступность привязки логируется, но не роняет генератор. Живая проверка: `curl /metrics` → prometheus-текст, `/nope` → 404.
- **F13. Валидация профиля. ✅ Сделано (v8.1.0, расширено v8.5.0/D3).** Модуль `validate` (`src/validate.rs`): типизированный `ValidationError` (`thiserror`) и `validate_profile()`, собирающий **все** ошибки за один проход (не падает на первой). Проверяет: transport/format/distribution/framing/shutdown.mode, диапазоны facility(0..=23)/severity(0..=7) для rfc5424/3164, веса шаблонов, непустые targets/phases, `connections>=1`, источник контента, условие остановки фазы, отрицательные/NaN в load_shape. `run_profile()` делает fail-fast. **D3 (v8.5.0)**: дополнительно структурная JSON Schema (`schemas/profile.schema.json`) через `jsonschema` — `--schema-strict` ловит неправильные типы, неизвестные ключи, значения вне диапазонов 0..=23/0..=7. **YAML-ввод** (D3) — `load_profile_from_yaml_str`, автоопределение по расширению `.yaml`/`.yml`.
- **F14. Multi-template и schema per-phase. ✅ Сделано (v7.9.0).** Из `templates`/`templates_file` на каждое сообщение выбирается случайный шаблон — равновероятно или по `template_weights`. Schema и templates по-прежнему сосуществуют на фазе (schema.template имеет приоритет, иначе выбирается шаблон из списка).

#### P2 — Расширения
- **F15. Дополнительные форматы:** CEF, LEEF, JSON-lines, Apache/Nginx access — для SIEM-сценариев.
- **F16. Дополнительные транспорты:** TCP с keep-alive/reconnect, Kafka/Redpanda sink (совпадает с экспертизой заказчика), файловая ротация.
- **F17. Сценарии «атак»/аномалий. ✅ Сделано (v9.5.1, patch поверх v9.5.0).** Tagged enum `AnomalyKind` с тремя сценариями: `BurstInjection { rate_multiplier, interval_secs, duration_secs }` (всплеск интенсивности каждые `interval_secs` в течение `duration_secs`), `SlowDrip { rate_divisor, duration_secs }` (пониженная интенсивность первые `duration_secs` — low-and-slow), `PacketLoss { loss_percent }` (детерминированный по `(seed, seq)` drop до отправки через F4-derive_rng с F17-salt). `Phase.anomalies: Option<Vec<Anomaly>>` (`#[serde(default)]`). В `run_phase_multi` аномалии композиционны: multiplicative для rate (burst × M + slow-drip ÷ D = × (M/D)), OR для packet-loss. При наличии аномалий переключаемся с governor (burst-friendly) на честный sleep-планировщик. Prometheus-метрики `syslog_anomalies_applied_total{phase,type}` + `syslog_anomalies_dropped_total{phase,type}`. 6 новых `ValidationError` (F13) + `schemas/profile.schema.json::Anomaly` (oneOf для трёх типов). 13 unit + 2 metrics + 8 validate + 8 integration = 31 новый тест. 0 breaking changes относительно v9.5.0.

### 4.2. Нефункциональные характеристики

#### P0 — Корректность и наблюдаемость
- **N1. Живые ресурсные метрики.** Реально заполнять `cpu`/`memory` (например, через `sysinfo`), либо удалить эти gauge и панели.
- **N2. Синхронизация дашборда с метриками.** Добавить недостающие метрики (`syslog_message_size_bytes` histogram, `syslog_messages_by_format_total`, `syslog_active_workers`) или удалить их из `grafana.json`. Дашборд не должен ссылаться на несуществующее.
- **N3. Метрики нагрузки. ✅ Сделано (v8.0.0).** Фактический achieved-rate и target-vs-actual (`syslog_achieved_rate`/`syslog_target_rate`), **histogram латентности отправки** `syslog_send_duration_seconds` (корзины 5µs–1s — основа для p50/p95/p99), **histogram размера сообщений** `syslog_message_size_bytes` (16B–64KB), **счётчик реконнектов** `syslog_reconnects_total{transport,target}` (с реальным восстановлением TCP/TLS). p50/p95/p99-агрегация в рантайме и HTTP-экспорт (F12) — веха D; сейчас метрики доступны через `gather_metrics`.

#### P1 — Безопасность
- **N4. Безопасный TLS по умолчанию. ✅ Сделано (v8.2.0, расширено v8.7.2/N4.mTLS).** Валидация сертификата включена по умолчанию (`build_tls_connector` + `TlsParams` в `src/transport/tls.rs`). Новые поля `TargetConfig`: `tls_domain` (SNI/проверка имени; по умолчанию — хост-часть `address`), `tls_ca_file` (PEM доверенного CA для self-signed/приватного CA), `tls_insecure` (явный opt-in в небезопасный режим, по умолчанию `false`; при включении — предупреждение в stderr), **tls_client_cert_file** + **tls_client_key_file** (v8.7.2, mTLS — клиент предъявляет сертификат серверу), **tls_min_protocol_version** (v8.7.2, "1.2" или "1.3", защита от downgrade-атак). `parse_tls_min_version` парсит строку в `native_tls::Protocol`. 3 новых `ValidationError`: `TlsClientCertFileNotFound`, `TlsClientKeyFileNotFound`, `InvalidTlsMinProtocolVersion`. Валидация (F13) отклоняет несуществующий `tls_ca_file`/`tls_client_cert_file`/`tls_client_key_file`. Живая проверка: insecure-warn, отклонение битого CA (rc=1), mixed-тесты + mTLS-тесты проходят. **Отложено** (в веху E или после): **cipher policy** (allow/denylist шифров).

#### P1 — Производительность
- **N5. Бенчмарки как регрессионный гейт.** Расширить Criterion-бенчи на реальные нагрузочные показатели (msg/s на транспорт, аллокации на сообщение); фиксировать базлайны. Профилирование горячего пути генерации (сейчас `render_template` делает `String::replace` в цикле по всем ключам — O(templ*keys), стоит перейти на однопроходный парсер/предкомпиляцию шаблонов).
- **N6. Zero-copy/буферизация.** Переиспользование буферов, батчирование записи в сокет/файл, `BufWriter`, векторизованная запись.

#### P1 — Надёжность и качество
- **N7. Обработка ошибок. ✅ Сделано (v8.3.0).** В рантайм-коде (вне `#[cfg(test)]`) устранены все `.unwrap()`/`.expect()`. Введён `src/error.rs` с доменными enum'ами `MetricsError`/`ConfigError`/`DrainError` и общим `RuntimeError` (через `thiserror`). `create_metrics()`/`gather_metrics()` теперь возвращают `Result<_, MetricsError>`; `graceful_drain_wait` — `Result<(), DrainError>`. Ошибки пробрасываются через `?` в `anyhow::Error` на границе CLI и уходят в `eprintln` с `ExitCode::FAILURE`. Политика recoverability (bind-fail на `/metrics`, transport-fail sender'ов) сохранена. 11 новых интеграционных тестов в `tests/n7_runtime_errors.rs`; +14 unit-тестов в `error::tests`/`metrics::tests`.
- **N8. Расширение тестов.** Тесты корректности RFC-форматов (парсинг обратно валидатором), rate-точности, framing, reconnect, back-pressure; property-based тесты (`proptest`) для генераторов; тесты детерминизма по seed.
- **N9. CI-пайплайн. ✅ Сделано (v8.4.0).** GitHub Actions workflow `.github/workflows/ci.yml` запускается на каждый push в `main`/`dev` и PR в `main`. Матрица: `ubuntu-latest` + `macos-latest`. Стадии: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo build --release --locked` → `cargo test --no-run --locked` → `cargo test --locked` → `cargo bench --no-run --locked`. Кэш cargo через `Swatinem/rust-cache@v2`. На Linux устанавливается `libssl-dev` для `openssl-sys`. Best-effort `msrv` job читает канал из `rust-toolchain.toml`. Актуализирован `.gitignore` (тестовые логи/TLS-PEM/zip-архивы/IDE) и применён `cargo fmt --all` для прохождения CI-гейта. README получил бейджи CI.
- **Регрессия бенчмарков `sender_throughput` после F13 (v8.4.1):** в v8.1.0 (F13 — валидация профиля) `run_profile()` стал отвергать фазы без условий остановки (`duration_secs=0` + `total_messages=None`). `benches/sender_throughput.rs::make_profile` использовал `Phase { ..Default::default() }`, что давало такую фазу — бенчмарк падал на старте с `ValidationError::UnboundedPhase`. Регрессия была пропущена в v8.1.0..v8.4.0, потому что `cargo test` не покрывает бенчмарки (требуется `cargo bench`). Починено в v8.4.1: `make_profile` теперь принимает `total_messages: u64` и выставляет `total_messages: Some(...)` — явное ограничение остановки, удовлетворяющее валидации F13. Все 9 бенчей (3 + 6) теперь проходят `cargo bench -- --quick`.
- **D3 — формальная JSON Schema + YAML-ввод профиля (v8.5.0):** профили теперь можно загружать как из JSON (`.json`), так и из YAML (`.yaml`/`.yml`) с автоопределением формата по расширению в `load_profile_from_path`. Добавлена runtime-валидация через `jsonschema = "0.18"` (встроенная схема через `include_str!` в `schemas/profile.schema.json`) и CLI-флаг `--schema-strict`. Schema ловит структурные ошибки (типы, диапазоны, обязательные поля, unknown keys), семантика остаётся на F13. Новые варианты `ConfigError::Yaml` и `ConfigError::UnsupportedFormat`. CI-стадия "Validate examples" прогоняет `--validate --schema-strict` на всех `.json`/`.yaml`/`.yml` в `examples/` — регрессионный тест на изменения схемы. 6 профилей-примеров получили `total_messages: 100` (без этого F13 отвергал их как `UnboundedPhase`); `templates_basic.json` перенесён в `examples/templates/` (это не профиль, а массив шаблонов для `--message`). Все 167 тестов (102 unit + 54 integration + 11 N7) и 9 бенчей зелёные.
- **N2 — синхронизация Grafana-дашборда (v8.6.0):** добавлена реальная `syslog_messages_by_format_total{format}` (CounterVec, инкремент в core.rs::run_phase_multi) и 6 panels в `dashboards/grafana.json`: messages rate by transport, messages by format, active workers, send p95 latency, message size p95, errors rate. Удалены фейковые gauge-ы `cpu_usage_percent`/`memory_usage_bytes` — они были объявлены, но никогда не обновлялись в runtime (нет реального сбора process metrics), в `/metrics` всегда показывали 0, в дашборде — пустые графики. Честный подход — не обещать то, чего нет. Если process metrics понадобятся в будущем — это отдельная задача (требует крейта `sysinfo` + фоновой задачи). Дашборд теперь покрывает все ключевые метрики нагрузки. +3 теста: `n2_no_cpu_or_memory_gauges_in_exposition`, `n2_messages_by_format_total_after_inc`, `test_n2_messages_by_format_total_exported`. Все 170 тестов (104 unit + 55 integration + 11 N7) и 9 бенчей зелёные.
- **N6 (v8.7.0):** zero-copy/буферизация — в горячем пути send-loop каждое сообщение
  вызывало `frame()` / `frame_stream()`, возвращавшие новый `Vec<u8>` (аллокация
  + format! на каждом сообщении). Теперь `frame_into(&mut BytesMut, &msg, framing)`
  пишет в переиспользуемый буфер (0 аллокаций на кадр); `target_sender_file` использует
  `BufWriter` (8 KiB) для коалесцирования мелких write-syscall'ов; `target_sender_tcp/tls`
  используют `BytesMut` (8 KiB) для батчинга TCP-write. Уменьшение syscall'ов в
  ~50-100 раз для типичной нагрузки (10k msg/s). + `bytes = "1"` зависимость;
  + 4 unit-теста на zero-copy инварианты (capacity сохраняется, N фреймов в
  один буфер дают корректный конкатенированный вывод).
- **N10 (v8.8.0):** рефакторинг слоёв. До N10 в `src/` лежало ~17 модулей
  вперемешку (`format`, `transport`, `observability`, `generator`
  были разбросаны по `src/syslog.rs`, `src/sender.rs`, `src/metrics*.rs`,
  `src/protobuf.rs`, `src/core.rs`, `src/config.rs`). После N10:
  4 новые директории (`src/format/`, `src/transport/`, `src/observability/`,
  `src/generator/`) с явным разделением слоёв. Старые модули
  `src/{core,config,sender,syslog,metrics,metrics_server,protobuf}.rs`
  заменены на thin re-export обёртки — публичный API полностью
  сохранён, 0 breaking changes. `src/architecture-notes.md` переписан
  с реальной архитектурой (был заглушкой v7.4.0). Trait `Format` и
  trait `Transport` объявлены как план для вехи E (F15, F16).
- **N4.mTLS (v8.7.2):** mutual TLS (клиент предъявляет сертификат серверу) +
  минимальная версия TLS-протокола. Добавлены 3 поля в TargetConfig:
  `tls_client_cert_file`, `tls_client_key_file`, `tls_min_protocol_version`
  (все опциональные, backward-compatible). `TlsParams` расширен
  соответствующими полями; `build_tls_connector` загружает client identity
  через `Identity::from_pkcs8` (если задан) и устанавливает min_protocol
  через `builder.min_protocol_version`. `parse_tls_min_version` парсит
  "1.2"/"1.3" в `native_tls::Protocol`. 3 новых `ValidationError`:
  `TlsClientCertFileNotFound`, `TlsClientKeyFileNotFound`,
  `InvalidTlsMinProtocolVersion` (fail-fast). 9 новых тестов (4
  connector, 2 валидация, 3 парсинга). Реализация openssl helper
  (не rcgen — та же проблема с `Identity::from_pkcs8` на OpenSSL 3.6.1).
- **N8 (v8.7.1):** property-based тесты через `proptest = "1"` в
  `src/payload_proptests.rs` (`#[cfg(test)]` модуль). 6 тестов покрывают
  инварианты которые трудно выразить конкретными примерами: int_in_range
  в диапазоне, seed-детерминизм (16 итераций u64), pad_to_size ровно target
  (с лимитом 64KB чтобы не уйти в OOM), faker IPv4 валидный формат, faker
  UUID v4 валидный формат. Каждый тест прогоняет 256+ случайных комбинаций.
  Явное end-to-end back-pressure integration тестирование (TCP receiver
  с задержкой → проверка mpsc overflow) отложено — TCP-буфер ядра > 64KB
  вмещает маленькие сообщения мгновенно, sender не блокируется, тест
  flaky. Back-pressure покрывается косвенно: N6 (батчинг), rate-limit,
  drain_as_errors. TODO для вехи E через mock'и trait Transport.
- **N5 + N8 + N11 (v8.6.1):** финальные P1-пробелы перед major v9.0. Закрыты:
  - **N5** `src/template.rs` — `CompiledTemplate` с one-pass парсингом
    `{{placeholder}}` (вместо O(N×M) `String::replace`-цикла). Для типичного
    шаблона ~50 символов × 20 ключей это ~100x ускорение горячего пути
    в `run_phase_multi`. Старая `render_template` сохранена как
    backward-compatible обёртка.
  - **N8** `src/syslog.rs::tests` — round-trip парсер RFC 5424
    (`parse_rfc5424_for_test`) с 3 тестами (простой случай с бинарными
    байтами \x80\x81, NILVALUE + BOM, structured_data с пробелами внутри
    `[...]`). Гарантирует что наш вывод соответствует RFC и парсится
    стандартными приёмниками (rsyslog, syslog-ng) корректно.
  - **N11** — `docs/USER_GUIDE.md` и `docs/DEVELOPER_GUIDE.md` обновлены
    до v8.6 (были заморожены на v7.4.0). `.meta.json` файлы
    актуализированы (v4.0 → v8.6). После этого **все P1-задачи вехи D
    закрыты**.
  Все 181 тестов (115 unit + 55 integration + 11 N7) и 9 бенчей зелёные.

#### P2 — Сопровождаемость и поставка
- **N10. Вынести ядро из `lib.rs`-реэкспортов в чёткие слои** (`generator`, `transport`, `scheduler`, `format`, `observability`), убрать `architecture-notes.md`-заглушку, описать реальную архитектуру.
- **N11. Документация как контракт.** Привести USER/DEVELOPER guide в соответствие реализации, добавить раздел «ограничения» и «поведение метрик Prometheus»; синхронизировать `.meta.json` (сейчас ссылаются на устаревшую «v4.0»).
- **N12. Поставка:** Docker-образ, статически слинкованный бинарник (musl), пример docker-compose со стеком «генератор + rsyslog/syslog-ng + Prometheus + Grafana» для приёмочного тестирования.

---

## 5. Рекомендованный порядок работ (roadmap)

1. **Веха A — «Настоящая нагрузка» (P0 F1–F3, N3): ✅ ЗАВЕРШЕНА ПОЛНОСТЬЮ (v8.0.0).** rate-limiting (F1, v7.5.0), connections (F2, v7.6.0), профили нагрузки во времени (F3, v7.8.0 — constant/linear/sine/burst), метрики нагрузки с латентностью/размером/реконнектами (N3, v8.0.0). Без отложенных задач.
2. **Веха B — «Валидный syslog» (P0 F7–F10): ✅ ЗАВЕРШЕНА ПОЛНОСТЬЮ (v8.0.0).** RFC 5424/3164 + framing (v7.7.0); честный protobuf wire-format (F10, v8.0.0). Делает вывод пригодным для реальных приёмников. Без отложенных задач.
3. **Веха C — «Вариативный пейлоад» (P0 F4–F6, F14): ✅ ЗАВЕРШЕНА ПОЛНОСТЬЮ (v8.0.0).** ГПСЧ+seed (F4, v7.9.0), богатый faker-набор + типы полей + **regex** (F5), распределения uniform/weighted/zipf + паддинг + **межполевые корреляции** (F6), мультишаблоны с весами (F14). Ранее отложенные `regex` и корреляции реализованы в v8.0.0. Закрывает «глубокую кастомизацию» пейлоада без остаточных задач.
4. **Веха D — «Продакшн-готовность» (P1): ✅ ЗАКРЫТА (v8.8.0, последний patch — v8.8.1 с правками AUDIT.md).** Все P1-задачи выполнены: CLI (F11), валидация профиля (F13), типизированные ошибки валидации (`ValidationError` через `thiserror`), **HTTP-эндпоинт /metrics (F12)**, **безопасный TLS по умолчанию (N4)**, **типизированные ошибки рантайма (N7)**, починка TLS-тестов (v8.3.1), **CI-пайплайн GitHub Actions (N9)**, починка регрессии бенчмарков (v8.4.1), **JSON Schema + YAML-ввод (D3)**, **синхронизация Grafana-дашборда (N2)**, **CompiledTemplate/N5 (v8.6.1)**, **round-trip RFC 5424/N8 (v8.6.1)**, **документация/N11 (v8.6.1)**, **zero-copy/буферизация (N6, v8.7.0)**, **property-based тесты (N8, v8.7.1)**, **mTLS + min_protocol (N4.mTLS, v8.7.2)**, **рефакторинг слоёв format/transport/observability/generator (N10, v8.8.0)**. v8.8.1 — правки AUDIT.md (поставить ✅ на F7/F8/F9, убрать "Отложено" из F13/N4). v9.0.0 — milestone release без breaking changes (семантический маркер закрытия вехи D). Потом веха E (P2).
5. **Веха E — «Зрелость» (P2):** доп. форматы/транспорты, сценарии аномалий, Docker/compose, рефакторинг слоёв.

Каждая веха завершается compile-verified релизом с обновлением `CHANGELOG.md` и документации (как в текущем процессе v7.4.0).

---

## 6. Источники ориентиров

- Формат: [RFC 5424 — The Syslog Protocol](https://www.rfc-editor.org/rfc/rfc5424.html); [RFC 3164 / RFC 5424 обзор — Graylog](https://graylog.org/post/syslog-protocol-a-reference-guide/); [structured data — LogCentral](https://logcentral.io/blog/structured-data-syslog-rfc-5424-overview).
- Framing/TLS: [RFC 6587 — TCP framing](https://datatracker.ietf.org/doc/html/rfc6587); [RFC 5425 — TLS transport](https://www.rfc-editor.org/rfc/rfc5425.txt); [NXLog — syslog over TLS](https://nxlog.co/news-and-blog/posts/syslog-forwarding-over-tls).
- Инструменты-ориентиры: [syslog-ng loggen](https://github.com/syslog-ng/syslog-ng/blob/master/tests/loggen/loggen.md) (`--size`, `-P`, framing); [flog](https://github.com/mingrammer/flog) (rfc3164/rfc5424/json); [синтетическая генерация логов — EvidenceForge](https://github.com/Cisco-Talos/EvidenceForge/blob/main/docs/design/synthetic-log-generation-research.md).
- Масштабирование: [Axoflow — syslog scaling](https://axoflow.com/blog/syslog-scaling-and-performance-considerations).
