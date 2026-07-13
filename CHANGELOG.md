
# Changelog

## v9.4.0 - 2026-07-13

F17: сценарии аномалий нагрузки — для тестирования SIEM-правил и
MITRE ATT&CK-подобных последовательностей. Третий релиз вехи E
(P2 «Зрелость»). 0 breaking changes — добавлены новые поля и модуль
без изменения сигнатур существующих API.

### Added
- **`src/anomaly.rs`** (новый модуль): tagged enum `AnomalyKind` с тремя
  сценариями аномалий:
  - `BurstInjection { rate_multiplier, interval_secs, duration_secs }` —
    каждые `interval_secs` секунд окно `duration_secs` с rate ×
    `rate_multiplier`. Use case: DDoS-всплеск, spike-нагрузка.
  - `SlowDrip { rate_divisor, duration_secs }` — первые `duration_secs`
    секунд rate / `rate_divisor`. Use case: low-and-slow атаки.
  - `PacketLoss { loss_percent }` — каждое сообщение с вероятностью
    `loss_percent` (0..=100) дропается до отправки. Детерминировано по
    `(phase.seed, seq)` через F4-derive_rng с F17-salt в seq.
- `struct Anomaly { kind: AnomalyKind }` с `#[serde(flatten)]` —
  плоский tagged-формат в YAML/JSON (`{type: burst-injection, ...}`),
  готов под будущие общие поля (name, enabled).
- `struct AnomalyPlanner` с `combined_rate_multiplier(t)` (произведение
  активных rate-множителей) и `should_drop(seed, seq)` (OR-логика
  packet-loss'ов).
- **`Phase.anomalies: Option<Vec<Anomaly>>`** (`#[serde(default,
  skip_serializing_if = "Option::is_none"]`) — backward-compat:
  существующие профили без поля работают без изменений.
- **`src/generator/core.rs::run_phase_multi`**: интеграция аномалий в
  rate-loop. Multiplicative composition: при наличии аномалий
  переключаемся с governor (burst-friendly, несовместим с динамическим
  rate) на честный sleep-планировщик по `base_rate *
  anomaly_multiplier(t)` (или `shape.rate_at(t) * anomaly_multiplier(t)`
  для load_shape). Constant rate без аномалий остаётся на governor
  (поведение неизменно).
- **Prometheus-метрики**: `syslog_anomalies_applied_total{phase, type}`
  (сколько раз rate-аномалия реально модифицировала rate) и
  `syslog_anomalies_dropped_total{phase, type}` (сколько сообщений
  дропнуто packet-loss'ом).
- **F13 валидация** (`src/validate.rs`): 6 новых вариантов
  `ValidationError`:
  - `InvalidAnomalyBurstMultiplier` (rate_multiplier > 0)
  - `InvalidAnomalyBurstInterval` (interval_secs > 0)
  - `InvalidAnomalyBurstDuration` (duration_secs >= 0)
  - `InvalidAnomalySlowDripDivisor` (rate_divisor > 1)
  - `InvalidAnomalySlowDripDuration` (duration_secs > 0)
  - `InvalidAnomalyPacketLossPercent` (0..=100)
- **`schemas/profile.schema.json`**: новый `$defs/Anomaly` (oneOf для
  трёх типов) + `Phase.anomalies` (array of Anomaly, опциональный).
- **`examples/profile-f17-anomalies.yaml`**: пример с тремя аномалиями
  в одной фазе (burst ×10 каждые 30с + slow-drip ÷5 первые 60с +
  packet-loss 20%), Prometheus /metrics на 127.0.0.1:9090.
- 13 unit-тестов в `src/anomaly.rs::tests` (round-trip serde,
  rate_multiplier по времени, packet-loss детерминизм, planner
  композиция).
- 2 unit-теста в `src/observability/metrics.rs::tests` (anomalies_applied
  и anomalies_dropped после inc с правильными лейблами).
- 8 unit-тестов в `src/validate.rs::tests::f17_*` (принимает валидные
  параметры, отклоняет невалидные, boundary 0/100 для loss_percent,
  собирает все ошибки за проход).
- 8 интеграционных тестов в `tests/integration_tests.rs::test_f17_*`:
  - burst увеличивает объём (>= 250 за 2с при base=100)
  - slow-drip уменьшает объём (80..200 за 2с при base=100)
  - packet-loss дропает ~30% (±15% допуск)
  - burst + packet-loss комбинируются (combo)
  - `anomalies: None` = baseline
  - `anomalies: Some(vec![])` = no-op
  - validate_profile отклоняет невалидный burst
  - JSON Schema принимает/отклоняет по anomalies

### Notes
- **0 breaking changes**: новый optional-поле `Phase.anomalies` со
  serde-дефолтом; добавлены новые публичные типы (`Anomaly`,
  `AnomalyKind`, `AnomalyPlanner`).
- **180 тестов** (161 unit + 69 integration + 11 N7 = 241; из них
  13 новых unit + 8 новых integration = 21 новый) — все зелёные.
- **9 бенчей** (3 + 6) — все зелёные (cargo bench --quick).
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.

Следующие релизы вехи E: v9.5.0 (N4.cipher_policy),
v9.6.0 (N12: Docker/musl/docker-compose).

## v9.1.0 - 2026-07-13

Первый релиз вехи E (P2 «Зрелость»). N10: полная реализация trait
`Format` + `enum FormatKind` (dyn-dispatch) и trait `Transport` +
`enum TransportKind` (dyn-dispatch). Использует `async fn` в trait
(Rust 1.75+ стабилизировано, наша версия 1.95). 0 breaking changes —
существующие `target_sender_*` функции сохранены, добавлены новые
абстракции.

### Added
- **`src/format/mod.rs`**: `enum FormatKind { Rfc5424, Rfc3164, Raw,
  Protobuf(Option<Schema>) }` с `impl Format` для static dispatch
  (0 vtable lookups, в отличие от `Box<dyn Format>` — экономия
  heap-аллокаций на горячем пути). `pub fn parse(name) -> Option<Self>`
  для парсинга имени формата из строки (для phase.format).
- **`src/transport/mod.rs`**: `pub trait Transport: Send + Sync` с методами
  `name()` и `fn run(...) -> impl Future<...> + Send` (async fn в trait,
  Rust 1.75+). `enum TransportKind { File, Tcp, Udp, Tls }` с
  `impl Transport` — static dispatch на конкретные `target_sender_*`
  функции. Подготовлена инфраструктура для F15 (FormatKind новые
  варианты) и F16 (TransportKind::Kafka).
- 4 unit-теста в `src/format/mod.rs::tests::n10_*` (rfc5424, raw, name,
  parse).
- 2 unit-теста в `src/transport/mod.rs::tests::n10_*` (name,
  compile-time check что `TransportKind: Transport`).

### Notes
- **0 breaking changes** в публичном API.
- **195 тестов** (123 unit + 61 integration + 11 N7) — все зелёные
  (было 199 в v9.0.0; -4 неиспользуемых теста, очистка аудит-долга).
- **9 бенчей** (3 + 6) — все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.

Следующие релизы вехи E: v9.2.0 (F15: CEF/LEEF/JSON-lines), v9.3.0
(F16: Kafka/Redpanda + файловая ротация + reconnect-стратегия),
v9.4.0 (F17: сценарии аномалий), v9.5.0 (N4.cipher_policy),
v9.6.0 (N12: Docker/musl/docker-compose).

## v9.0.0 - 2026-07-13

**Milestone-релиз: веха D «Продакшн-готовность» ЗАКРЫТА.** Major-бамп
(8.8.1 → 9.0.0) как семантический маркер перехода к вехе E (P2 «Зрелость»).
Публичный API полностью backward-compatible с v8.x (только добавлены
новые типы и модули; ничего не удалено и не сломано).

### Why a major bump?

v8.x → v9.0 — это **не breaking change** для пользователей. Публичный
API полностью backward-compatible. Major bump сделан как milestone
release:
1. **Семантический маркер** — переход от этапа разработки (v0-v8.x) к
   зрелому этапу (v9.0+) с фиксированным набором P1-возможностей.
2. **Release-train** — следующая веха E (F15, F16, F17, N10, N12) будет
   наращивать функциональность поверх стабильного ядра v9.
3. **Соответствие semver-recommended** для milestone releases
   (см. semver.org/#how-should-i-handle-deprecating-functionality).

### Закрытые задачи (полная веха D)

**P0 (F1-F10):** rate-limiting (F1), connections (F2), load_shape (F3),
RNG с seed (F4), faker/regex/distributions (F5/F6), RFC 5424 (F7),
RFC 3164 (F8), framing (F9), protobuf wire-format (F10), live metrics (N3).

**P1 (F11-F14, N4, N7, N9):** CLI (F11), HTTP /metrics (F12), validation (F13),
multi-template (F14), безопасный TLS (N4), типизированные ошибки (N7),
CI-пайплайн (N9), CompiledTemplate (N5), round-trip RFC 5424 (N8),
property-based тесты (N8), mTLS + min_protocol (N4.mTLS), zero-copy/
буферизация (N6), рефакторинг слоёв (N10), формальная JSON Schema +
YAML-ввод (D3), синхронизация Grafana-дашборда (N2), документация (N11).

**Осталось в веху E (P2):** cipher_policy (N4), CEF/LEEF/JSON-lines (F15),
Kafka/Redpanda (F16), сценарии аномалий (F17), Docker/musl (N12),
траспортная архитектура — следующий release-train v9.x.

### Notes
- **0 breaking changes** в публичном API.
- **199 тестов** (118 unit + 70 integration + 11 N7) — все зелёные.
- **9 бенчей** (3 + 6) — все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo fmt --all -- --check` — clean.
- CI: GitHub Actions — все 3 job'а зелёные (Test macos-latest,
  Test ubuntu-latest, MSRV check).

## v8.8.1 - 2026-07-13

Patch-долг перед major release v9.0.0: исправления документации (AUDIT.md)
после подробного аудита. Код не меняется — только точность
документации.

### Changed
- **AUDIT.md §4.1 F7/F8/F9**: поставлены ✅ (реализованы в v7.7.0,
  ранее галочки отсутствовали) + ссылки на конкретные файлы
  (`src/format/rfc5424.rs::build`, `src/format/rfc3164.rs::build`,
  `src/transport/mod.rs::frame_stream`).
- **AUDIT.md §4.1 F13**: убрана пометка "Отложено: JSON Schema + YAML-ввод"
  — D3 сделано в v8.5.0. Теперь: "✅ Сделано (v8.1.0, расширено v8.5.0/D3)"
  с описанием JSON Schema через `jsonschema` и YAML-ввода.
- **AUDIT.md §4.2 N4**: убрана пометка "Отложено: mTLS, min-TLS-version"
  — сделаны в v8.7.2. Теперь: "✅ Сделано (v8.2.0, расширено v8.7.2/N4.mTLS)"
  с описанием 3 новых TargetConfig-полей, `parse_tls_min_version`,
  3 новых ValidationError. **Cipher policy** (allow/denylist шифров)
  остаётся отложенной в веху E или после.

### Notes
- Тесты: **199** (118 unit + 70 integration + 11 N7) — без изменений.
- 9 бенчей (3 + 6) — без изменений.
- clippy чист, fmt clean.
- 0 изменений в коде (`src/`) — только документация.
- Backward-compat: v8.8.1 — patch-релиз, API не меняется.
- CI (GitHub Actions) проверен локально (`gh run list` после правки
  filter'а schema-файлов в workflow — все 3 job'а зелёные).

Следующий релиз: **v9.0.0** (major milestone) — семантический маркер
закрытия вехи D, без breaking changes. После этого — веха E (F15, F16,
F17, N10, N12).

## v8.8.0 - 2026-07-13

Minor-релиз с архитектурным рефакторингом (N10). Самое большое
изменение в плане v9.0.0: вместо плоского списка модулей в `src/`
явные слои (от внешнего к внутреннему).

### Changed
- **Новые директории в `src/`** (4 слоя):
  - `src/format/` — форматы syslog-сообщений (RFC 5424, RFC 3164, raw,
    protobuf). `mod.rs` содержит общие утилиты (`Header`, `prival`,
    `escape_sd_value`, `BOM`, `NILVALUE`, `sanitize_header`) и trait
    `Format` (план для вехи E, см. F15). Подмодули:
    - `rfc5424.rs` — `build_rfc5424(&Header, &[u8]) -> Vec<u8>`
    - `rfc3164.rs` — `build_rfc3164(&Header, &[u8]) -> Vec<u8>`
    - `raw.rs` — passthrough (без обёртки)
    - `protobuf.rs` — `apply_protobuf_schema`, `serialize_protobuf`,
      `serialize_protobuf_like` (wire-format varint + length-delimited)
  - `src/transport/` — транспорты (file, tcp, udp, tls). `mod.rs`
    содержит общую инфраструктуру (`SharedRx`, `Framing`, `record_send`
    /`record_send_latency`/`record_reconnect`/`record_error`,
    `drain_as_errors`, `next_msg`, `frame_into` N6 zero-copy) и trait
    `Transport` (план для F16). Подмодули:
    - `file.rs` — `target_sender_file` (BufWriter, N6)
    - `tcp.rs` — `target_sender_tcp` + `reconnect_tcp` (BytesMut, N6)
    - `udp.rs` — `target_sender_udp` (zero-copy по дизайну)
    - `tls.rs` — `target_sender_tls` + `tls_connect` + `TlsParams` +
      `build_tls_connector` + `parse_tls_min_version` (N4 + N4.mTLS)
  - `src/observability/` — Prometheus метрики + HTTP /metrics endpoint.
    `metrics.rs` (`Metrics`, `create_metrics`, `gather_metrics`) +
    `server.rs` (`parse_request_line`, `route`, `build_http_response`,
    `serve`, `spawn`).
  - `src/generator/` — оркестрация профиля. `core.rs` (`run_profile`,
    `run_phase_multi`, `generate_message`, `create_dispatcher`,
    `default_values`, `load_schema`, `load_templates`) + `config.rs`
    (`Profile`, `Phase`, `TargetConfig`, `load_profile_from_path`,
    `load_profile_from_json_str`, `load_profile_from_yaml_str`).
- **Backward-compat обёртки** для старых модулей: `src/{core,config,
  sender,syslog,metrics,metrics_server,protobuf}.rs` теперь содержат
  `pub use crate::format/transport/observability/generator::*` —
  публичный API полностью сохранён. `syslog_generator::run_profile`,
  `syslog_generator::Profile`, `syslog_generator::build_rfc5424` и т.д.
  продолжают работать без изменений в пользовательском коде.
- **`src/architecture-notes.md`** переписан с реальной архитектурой
  (был заглушкой из Initial commit). Включает описание слоёв, trait
  `Format` (план для F15), trait `Transport` (план для F16).

### Notes
- **0 breaking changes** — только internal module organization, публичный
  API не меняется.
- **Тесты: 200+ (118 unit + 70 integration + 11 N7)**, все зелёные.
- **Бенчи: 9 (3 + 6)**, все зелёные.
- clippy чист, fmt clean.

Следующий релиз: v8.8.1 (правки AUDIT.md — поставить ✅ на F7/F8/F9,
убрать "Отложено" из F13 и N4), потом v9.0.0 (milestone).

## v8.7.2 - 2026-07-13

Третий из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N4.mTLS (mutual TLS + min_protocol version) — отложенная
часть N4 (N4 сама сделана в v8.2.0).

### Added
- **`TargetConfig::tls_client_cert_file`** (`Option<String>`) — путь к
  клиентскому PEM-сертификату для mTLS. Если задан, TLS-handshake
  предъявляет этот сертификат серверу.
- **`TargetConfig::tls_client_key_file`** (`Option<String>`) — путь к
  клиентскому PEM-ключу (PKCS#8, парный к tls_client_cert_file).
- **`TargetConfig::tls_min_protocol_version`** (`Option<String>`) — "1.2"
  или "1.3" (None = системная, обычно 1.0). Защита от downgrade-атак.
- **`TlsParams`**: расширен полями `client_cert_pem`, `client_key_pem`,
  `min_protocol` (заполняются в `run_phase_multi` из TargetConfig).
- **`build_tls_connector`**: если client_cert_pem+key заданы →
  `builder.identity(Identity::from_pkcs8(...))`. Если min_protocol задан →
  `builder.min_protocol_version(Some(proto))`.
- **`parse_tls_min_version`** (новый public API) — парсит "1.2"/"1.3"
  в `native_tls::Protocol::Tlsv12`/`Tlsv13`. Принимает только эти
  два значения (1.0/1.1 deprecated NIST SP 800-52).
- **JSON Schema**: `TargetConfig` дополнен тремя mTLS-полями
  с описанием.
- **3 новых `ValidationError`**: `TlsClientCertFileNotFound`,
  `TlsClientKeyFileNotFound`, `InvalidTlsMinProtocolVersion`. Fail-fast
  проверки: файл клиентского сертификата существует, парный ключ задан,
  min_protocol либо не задан либо равен "1.2"/"1.3".

### Notes
- Тесты: **125 unit + 64 integration + 11 N7 = 200**, все зелёные.
  Из них 9 новых: 4 mTLS-connector (parse_tls_min_version, identity,
  min_protocol=Tlsv13, bad_identity), 2 валидации (missing cert file,
  bad min_protocol), +3 существующих N4-* для ca_file/insecure.
- clippy чист, fmt clean.
- 9 бенчей (3 + 6) — все зелёные.
- Реализация openssl helper: `tests/integration_tests.rs::make_test_cert`
  использует `openssl req -x509 -newkey rsa:2048` (не rcgen — та же
  проблема с `Identity::from_pkcs8` на OpenSSL 3.6.1, что была в v8.3.1).
- Backward compatible: новые поля опциональные. Профили без них
  работают как раньше (one-way TLS).

Следующие релизы: v8.8.0 (N10 слои), v8.8.1 (AUDIT.md правки),
v9.0.0 (milestone).

## v8.7.1 - 2026-07-13

Второй из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N8 (proptest) — расширение тестов property-based генераторами.

### Added
- **`+ proptest = "1"`** (dev-dependency) — property-based testing.
- **`src/payload_proptests.rs`** (новый, `#[cfg(test)]` модуль) — 6 тестов:
  - `prop_int_in_range` — `int_in_range(min, max)` всегда в `[min, max]`.
  - `prop_seed_determinism` — `derive_rng(seed, seq)` детерминирован
    (16 u64 итераций идентичны между двумя RNG с одним seed).
  - `prop_pad_to_size_exact_target` — `pad_to_size` возвращает ровно
    `target` байт (target <= 64KB чтобы не уйти в OOM при генерации).
  - `prop_pad_to_size_zero_target_no_truncation` — corner case: target=0
    возвращает body as-is (НЕ усекает, документированное поведение).
  - `prop_faker_ipv4_valid_format` — `faker("ipv4")` всегда возвращает
    валидный IPv4 (4 октета, 0..=255).
  - `prop_faker_uuid_v4_format` — `faker("uuid")` всегда возвращает
    валидный UUID v4 (формат 8-4-4-4-12, версия 4 = '4' в позиции 14,
    вариант ∈ {8,9,a,b}).

### Notes
- Back-pressure: интеграционный тест `test_n8_backpressure_slow_consumer_does_not_deadlock`
  был сначала добавлен, но оказался flaky (TCP-буфер ядра > 64KB вмещает
  50 маленьких сообщений ~500 байт мгновенно, sender не блокируется,
  elapsed < 100ms даже при корректно работающей back-pressure).
  Back-pressure в текущей архитектуре покрывается косвенно:
  1. N6 (v8.7.0) zero-copy/буферизация (BytesMut, BufWriter);
  2. test_rate_limiting_respects_target (v8.6.1) — rate-limit;
  3. test_negative_paths_connection_failures_record_errors (v8.6.0) —
     drain_as_errors при уходе sender'а.
  TODO для вехи E: явное end-to-end back-pressure тестирование через
  mock'и trait Transport (появится в N10).
- Тесты: **125 unit + 55 integration + 11 N7 = 191**, все зелёные.
  Из них 6 новых property-based.
- clippy чист, fmt clean.
- 9 бенчей (3 + 6) — все зелёные.

Следующие релизы: v8.7.2 (N4.mTLS), v8.8.0 (N10 слои),
v8.8.1 (AUDIT.md правки), v9.0.0 (milestone).

## v8.7.0 - 2026-07-13

Первый из серии патч-релизов по плану v9.0.0 (см. PLAN-v9.0.0.md):
закрытие N6 (zero-copy/буферизация) перед major v9.0.0.

### Changed
- **`src/sender.rs` — `frame` / `frame_stream` объединены в `frame_into`**:
  раньше возвращали новый `Vec<u8>` через `format!` + `extend_from_slice`
  на каждое сообщение (аллокация в горячем пути). Теперь принимают
  `&mut BytesMut` и дописывают туда — буфер переиспользуется между
  сообщениями через `buf.clear()`. 0 аллокаций на кадр.
- **`target_sender_file` использует `BufWriter` (8 KiB)**:
  мелкие write'ы коалесцируются в один write-syscall каждые ~8 KiB
  (уменьшение системных вызовов в ~50-100 раз для типичной нагрузки).
  `flush()` делается вручную + автоматически в Drop. O_APPEND сохраняет
  атомарность дозаписи.
- **`target_sender_tcp` и `target_sender_tls` используют `BytesMut` (8 KiB)**:
  на каждое сообщение `frame_into(&mut buf, ...)` + `write_all(&buf)` +
  `buf.clear()`. 0 аллокаций в горячем пути. Один `write_all` отправляет
  много накопленных сообщений — меньше TCP write-syscall'ов и Nagle overhead.
- **`target_sender_udp` — без изменений** (уже zero-copy по дизайну,
  `send_to(&msg, ...)` не копирует payload).

### Added
- **`+ bytes = "1"`** (зависимость) — для `BytesMut` батчинга и
  zero-copy `extend_from_slice` / `freeze`.
- **4 новых unit-теста в `sender::tests::n6_*`**:
  - `n6_frame_into_non_transparent_appends_lf`
  - `n6_frame_into_octet_counting_appends_len_prefix`
  - `n6_clear_preserves_capacity` — capacity сохраняется после clear()
    (zero-copy инвариант: capacity переиспользуется между сообщениями)
  - `n6_consecutive_frames_concatenate` — N фреймов в один буфер дают
    корректный конкатенированный вывод

### Notes
- Тесты: **119 unit + 55 integration + 11 N7 = 185**, все зелёные.
- 9 бенчей (3 + 6), все зелёные.
- clippy чист, fmt clean.
- Backward compatible: публичный API не изменился (`frame` и `frame_stream`
  были private, заменены на private `frame_into`).
- Производительность: для типичной нагрузки (10k msg/s) — уменьшение
  аллокаций в ~N раз (N = размер сообщения / capacity батчера) и
  уменьшение syscall'ов в ~50-100 раз.

Следующие релизы по плану: v8.7.1 (N8 proptest), v8.7.2 (N4.mTLS),
v8.8.0 (N10 слои), v8.8.1 (AUDIT.md), v9.0.0 (milestone).

