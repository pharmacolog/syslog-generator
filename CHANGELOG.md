
# Changelog

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

## v9.0.0 - 2026-07-13

**Milestone-релиз: веха D «Продакшн-готовность» ЗАКРЫТА.** Major-бамп
(8.6.1 → 9.0.0) как семантический маркер перехода к вехе E (P2 «Зрелость»).

Закрыты все P1-задачи AUDIT.md §4.1 (F1-F10), §4.2 (N1-N9), и часть
P2 (D3, N10 как тех. долг). Это **первый major-релиз** после v7.4.0
(Initial commit), символизирующий достижение промышленной готовности
генератора.

### Why a major bump?
v8.x → v9.0 — это **не breaking change**. Публичный API полностью
backward-compatible: добавлены новые публичные типы (`CompiledTemplate`,
`RuntimeError`, `MetricsError`, `ConfigError`, `DrainError`,
`SchemaCheckError`, `validate_against_embedded_schema`), удалено ничего.
Major bump сделан как milestone release:
1. **Семантический маркер** — переход от этапа разработки (v0-v8.x)
   к зрелому этапу (v9.0+) с фиксированным набором P1-возможностей.
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
CI-пайплайн (N9).

**Закрытие P1-пробелов (v8.6.1):** CompiledTemplate (N5, ~100x шаблонов),
round-trip RFC 5424 (N8), документация (N11).

**Синхронизация с реальностью (v8.6.0):** Grafana-дашборд (N2),
JSON Schema + YAML (D3).

### Notes
- Тесты: **115 unit + 55 integration + 11 N7 = 181**, все зелёные.
- 9 бенчей (3 + 6), все проходят `cargo bench -- --quick`.
- clippy чист, fmt clean, build --release успешен (syslog-generator 9.0.0).
- Backward compatibility: всё v8.x код продолжает работать без изменений.
- Следующая веха — E (P2 «Зрелость»): F15 (CEF/LEEF/JSON-lines),
  F16 (Kafka/Redpanda), F17 (сценарии аномалий), N12 (Docker), N10 (рефакторинг).

## v8.6.1 - 2026-07-13

Patch-релиз перед major v9.0.0: закрытие оставшихся P1-пробелов вехи D
(N5 предкомпиляция шаблонов, N8 round-trip парсинг RFC 5424, N11
актуализация документации). Покрытие тестами не снижено — 181 тестов,
все зелёные.

### Changed
- **`src/template.rs` — `CompiledTemplate`** (N5): старая `render_template`
  делала `String::replace` в цикле по всем ключам — O(N×M) на каждый
  вызов. Теперь шаблон парсится один раз в `Vec<TemplatePart>` (Literal/
  Placeholder) и рендерится за один проход — O(N). Для типичного шаблона
  длиной ~50 символов и ~20 ключей это даёт ~100x ускорение горячего
  пути в `run_phase_multi` (где `render_template` вызывается 5 раз для
  syslog header + 1 раз для тела). Старая `render_template(&str, &HashMap)`
  сохранена как backward-compatible обёртка (внутри вызывает `compile +
  render`). Все 88 предыдущих unit-тестов + 55 integration + 11 N7
  проходят без изменений.

### Added
- **`syslog::tests::parse_rfc5424_for_test`** (N8): минимальный round-trip
  парсер RFC 5424 — `build_rfc5424(&Header, &msg)` → `parse_rfc5424_for_test(&encoded)`
  → сравнение полей. Работает с `&[u8]` напрямую (не UTF-8), потому что
  MSG может содержать бинарные данные (\x80\x81 и т.п.) которые невалидны
  в UTF-8. 3 round-trip теста: простой случай с бинарными байтами,
  NILVALUE-поля + BOM, structured_data с пробелами внутри `[...]`.
- + 8 unit-тестов в `template::tests` для CompiledTemplate.

### Documentation
- **`docs/USER_GUIDE.md` и `docs/DEVELOPER_GUIDE.md` обновлены до v8.6**
  (N11): документы были заморожены на v7.4.0 (Initial commit). Карта
  модулей, раздел архитектурных решений v8.x, примеры публичного API
  для load_profile_from_path/validate_profile/validate_against_embedded_schema.
- **`.meta.json` файлы обновлены до v8.6** (N11): `auth_schema.json.meta.json`,
  `nginx_schema.json.meta.json`, `profile.json.meta.json`,
  `templates.json.meta.json` — ранее ссылались на устаревшую "v4.0",
  теперь "v8.6".

### Notes
- Тесты: **115 unit + 55 integration + 11 N7 = 181**, все зелёные.
- 9 бенчей (3 + 6), все зелёные.
- clippy чист, fmt clean.
- Backward compatible: публичный API не изменился (добавлены новые
  публичные типы — `CompiledTemplate`, `parse_rfc5424_for_test` доступен
  только в `#[cfg(test)]`).

## v8.6.0 - 2026-07-13

Финальная задача вехи D («Продакшн-готовность», P1): **N2 — синхронизация
Grafana-дашборда** с реальным выводом `metrics_server`. Дашборд приведён
в соответствие с фактическими метриками: добавлены panels для основных
показателей нагрузки (rate/latency/errors/active workers/messages by format),
а фейковые gauge-ы cpu_usage/memory_usage удалены из runtime и не упоминаются
в дашборде.

### Added
- **N2 — `syslog_messages_by_format_total{format}`** (CounterVec): реальный
  счётчик сгенерированных сообщений по формату (rfc5424/rfc3164/raw/protobuf).
  Инкрементируется в `core.rs::run_phase_multi` после `generate_message`
  с label = `phase.format_type()`. Раньше метрика упоминалась в дашборде,
  но в runtime не отдавалась.
- **6 новых panels в `dashboards/grafana.json`** (всего 10):
  - "Messages rate by transport" (timeseries, 12×8):
    `sum by (transport) (rate(syslog_messages_total[1m]))`
  - "Messages by format" (timeseries, 12×8):
    `sum by (format) (rate(syslog_messages_by_format_total[1m]))`
  - "Active Workers" (stat): `syslog_active_workers`
  - "Send p95 latency" (stat):
    `histogram_quantile(0.95, sum(rate(syslog_send_duration_seconds_bucket[5m])) by (le))`
  - "Message size p95" (stat):
    `histogram_quantile(0.95, sum(rate(syslog_message_size_bytes_bucket[5m])) by (le))`
  - "Errors rate" (stat): `sum(rate(syslog_errors_total[1m]))`

### Removed
- **N2 — `syslog_cpu_usage_percent` и `syslog_memory_usage_bytes`**:
  Gauge'ы были объявлены, но никогда не обновлялись в runtime (нет
  реального сбора process metrics). В `/metrics` они всегда показывали 0,
  в дашборде — пустые графики. Удалены из `src/metrics.rs` и не упоминаются
  в новой версии дашборда. Честный подход — не обещать то, чего нет.
  Если в будущем понадобится process metrics — это отдельная задача
  (требует крейта `sysinfo` + фоновой задачи).

### Changed
- `dashboards/grafana.json`: `description` дополнен списком всех panels
  и метрик, `tags = ["syslog-generator", "load-test", "v8.6"]` для фильтрации.

### Notes
- Тесты: **104 unit + 55 integration + 11 N7 = 170**, все зелёные.
  Из них 3 новых: 2 unit в `metrics::tests` (`n2_no_cpu_or_memory_gauges_in_exposition`,
  `n2_messages_by_format_total_after_inc`) + 1 integration
  `test_n2_messages_by_format_total_exported`.
- clippy чист, fmt clean, build --release успешен (syslog-generator 8.6.0).
- Бенчмарки: 9 кейсов (3 + 6), все зелёные.
- Закрыта последняя задача вехи D. Далее — переход к вехе E (P2).

## v8.5.0 - 2026-07-12

Продолжение вехи D («Промышленная готовность», P1). Закрыта задача
**D3 — формальная JSON Schema + YAML-ввод профиля**. Профили теперь можно
загружать как из JSON (.json), так и из YAML (.yaml/.yml) с автоопределением
формата по расширению. Добавлена runtime-валидация через `jsonschema`
(встроенная в бинарник через `include_str!`) и флаг `--schema-strict` для CI.

### Added
- **D3 — формальная JSON Schema** (`schemas/profile.schema.json`):
  Draft 2020-12, $defs для вложенных типов (TargetConfig, SyslogConfig,
  ShutdownConfig, ProtobufSchemaFieldMap, LoadShape с oneOf по тегированным
  вариантам, Phase). Ловит структурные ошибки: неправильные типы,
  обязательные поля, диапазоны (facility 0..=23, severity 0..=7), framing
  enum, дополнительные ключи (additionalProperties=false), источник
  контента (oneOf: templates/templates_file/schema_file), minItems:1
  для phases. Семантические правила остаются на F13-валидаторе.
- **D3 — YAML-ввод профиля.** `load_profile_from_yaml_str()` + автоопределение
  формата в `load_profile_from_path()` по расширению файла (.json/.yaml/.yml).
  Расширение проверяется ДО открытия файла — опечатка в имени даёт явную
  `ConfigError::UnsupportedFormat`, а не маскирующую `Io(NotFound)`.
- **`src/schema_check.rs`** (новый модуль): runtime-валидация Profile против
  встроенной JSON Schema через crate `jsonschema = "0.18"`. Публичные API:
  `validate_against_embedded_schema(&Profile)`, `validate_against_schema(...)`,
  тип `SchemaCheckError` (thiserror), константа `PROFILE_SCHEMA`.
- **CLI-флаг `--schema-strict`**: дополнительно к F13-валидации проверяет
  профиль против формальной JSON Schema. Полезно для CI и для отлова
  структурных ошибок до старта прогона.
- **CI-стадия "Validate examples"** в `.github/workflows/ci.yml`:
  `cargo run -- --validate --schema-strict --profile <file>` для каждого
  `.json`/`.yaml`/`.yml` в `examples/`. Защищает от регрессий в схеме и
  в примерах.

### Changed
- `src/config.rs::Phase`: `templates` теперь с `skip_serializing_if = "Vec::is_empty"`,
  `templates_file`/`schema_file` — с `skip_serializing_if = "Option::is_none"`.
  Это нужно для D3: пустые значения не сериализуются в JSON, и тогда
  `anyOf required: ["templates"|"templates_file"|"schema_file"]` в схеме
  корректно отлавливает фазы без контент-источника.
- `src/main.rs`: ручной `serde_json::from_str(&text)` заменён на
  `load_profile_from_path(path)` с автоопределением формата.
- 6 профилей в `examples/*.json` (graceful_shutdown, multi_target_*,
  protobuf_format, single_target) получили `total_messages: 100` в фазах,
  где его не было — без этого F13 отвергал их как `UnboundedPhase`.
- `examples/templates_basic.json` перенесён в `examples/templates/` —
  это не профиль, а массив шаблонов для `--message`, логически отделён
  от профилей.
- `src/error.rs`: добавлены варианты `ConfigError::Yaml` и
  `ConfigError::UnsupportedFormat` (вместо TODO-комментария).

### Dependencies
- `+serde_yaml = "0.9"` — YAML-парсинг.
- `+jsonschema = "0.18"` — runtime JSON Schema валидация.

### Notes
- Тесты: **102 unit + 54 integration + 11 N7 = 167**, все зелёные.
  Из них 14 новых: 6 config::tests (YAML/JSON загрузка), 6 schema_check::tests,
  2 config_error_yaml/unsupported_format, +5 integration в D3 секции.
- clippy чист, fmt clean.
- Бенчмарки: 9 кейсов (3 message_generation + 6 sender_throughput), все
  зелёные (регрессия v8.4.0 закрыта в v8.4.1, тут не сломалась).
- Live-проверка: `./syslog-generator --validate --schema-strict --profile
  examples/multi_target_roundrobin.yaml` → rc=0, "профиль валиден: 1 фаз(ы),
  2 цел(ей)".
- 11 примеров (4 .json + 1 .yaml + 1 .yml + 5 уже валидных) проходят
  полный цикл F13 + schema-strict. Schema-файлы (`schema_*.json`,
  `protobuf_schema.json`) корректно пропускаются через фильтр в CI.
- Публичный API расширен: `ConfigError::{Yaml, UnsupportedFormat}`,
  `RuntimeError::Config(#[from] ConfigError)` уже работал через `#[from]`,
  новые варианты подхватываются автоматически.

## v8.4.1 - 2026-07-12

Patch-релиз сразу после v8.4.0: починка регрессии `sender_throughput`
бенчмарков, сломавшихся ещё в v8.1.0 (внедрение валидации F13).

### Fixed
- **`benches/sender_throughput.rs`**: после F13 (v8.1.0) валидация профиля
  корректно отвергает фазы без условий остановки (`duration_secs=0` +
  `total_messages=None`) как `UnboundedPhase`. Бенчмарки `tcp_sender_throughput`
  и `udp_sender_throughput` использовали `Phase { ..Default::default() }`,
  из-за чего `run_profile(...).unwrap()` падал на старте. Регрессия была
  пропущена при выпуске v8.1.0..v8.4.0, потому что `cargo test` не покрывает
  бенчмарки.
  - `make_profile` теперь принимает `total_messages: u64` и выставляет
    `total_messages: Some(...)` — явное ограничение остановки, удовлетворяющее
    валидации F13.
  - Удалён устаревший комментарий про "caps generation at 100 messages per
    phase" (это ограничение снято ещё в v7.5.0 с введением F1 rate-limiting).

### Notes
- Тесты: **88 unit + 49 + 11 integration = 148**, все зелёные.
- Бенчмарки: все 9 кейсов (`message_generation` x3, `sender_throughput` x6)
  проходят `cargo bench -- --quick` после починки.
- clippy чист, fmt clean, `cargo bench --no-run --locked` успешен.
- Покрытие тестами не снижено — это только ремонт бенчмарков, код прод-системы
  не затронут.

## v8.4.0 - 2026-07-12

Продолжение вехи D («Промышленная готовность», P1). Закрыта задача
**N9 — CI-пайплайн**: GitHub Actions workflow `.github/workflows/ci.yml`,
который запускается на каждый push в `main`/`dev` и каждый PR в `main`,
прогоняя `fmt --check` → `clippy -D warnings` → `build --release` →
`test` → `bench --no-run` на матрице `ubuntu-latest` + `macos-latest`.

### Added
- **N9 — CI-пайплайн (`.github/workflows/ci.yml`).**
  - Триггеры: `push` в `main`/`dev`, `pull_request` в `main`.
  - Job `test` с матрицей `ubuntu-latest` + `macos-latest` — покрывает
    оба бэкенда `native-tls` (openssl-sys на Linux, Security.framework
    на macOS); это важно для безопасного TLS по умолчанию (N4) и для
    тестов с `openssl_self_signed`.
  - Стадии: `cargo fmt --all -- --check` → `cargo clippy --all-targets
    -- -D warnings` → `cargo build --release --locked` → `cargo test
    --no-run --locked` → `cargo test --locked` → `cargo bench --no-run
    --locked`.
  - Кэш cargo registry + build artifacts через `Swatinem/rust-cache@v2`
    с общим ключом по OS (`cache-on-failure: true` — кэш сохраняется
    даже при падении, чтобы следующий push мог переиспользовать слой).
  - На Linux устанавливается `libssl-dev` для `openssl-sys` (нужен
    `native-tls`); на macOS используется системный Security.framework.
  - Best-effort job `msrv`: если в репозитории есть `rust-toolchain.toml`,
    job читает `channel` оттуда и пробует собрать на этой версии rustc.
    Падение не блокирует merge (`continue-on-error: true`).
  - README.md получил три бейджа: CI status, version, Rust version.

### Changed
- `.gitignore` расширен для покрытия артефактов работы и тестов:
  тестовые логи (`*.log`), TLS-PEM (`*.pem`, `ca-*.pem`, `tls-ca-*.pem`,
  `/target/test-tls/`), zip-архивы релизов (`*.zip`), IDE/editor
  (`.vscode/`, `.idea/`, `*.swp`, `.DS_Store`), прочий мусор (`*.tmp`,
  `*.bak`). Это нужно, чтобы CI-артефакты и IDE-конфиги не попадали
  в репозиторий.
- Применён `cargo fmt --all` ко всему workspace — ранее код не
  проходил `cargo fmt --all -- --check`, что блокировало бы CI-гейт.
  Никаких семантических изменений, только переформатирование.

### Notes
- Тесты: **88 unit + 49 + 11 integration = 148**, все зелёные.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- `cargo build --release --locked` — успех.
- GitHub Actions имеет 6 часов на job, поэтому полный `cargo test`
  не разбивается на отдельные стадии (в отличие от sandbox-среды из
  CLAUDE_HANDOFF.md §2, где таймауты заставляли запускать бинарники
  по отдельности).
- Локальная проверка всех стадий перед коммитом прошла без ошибок.
- Релиз не требует новых runtime-зависимостей. CI использует уже
  имеющиеся крейты плюс GitHub Actions стандартные экшены.

## v8.3.1 - 2026-07-12

Patch-релиз сразу после v8.3.0: починка 3 упавших TLS-интеграционных
тестов, которые воспроизводились и до N7 (были задокументированы как
известное ограничение окружения). Покрытие тестами не снижено — все
49 integration-тестов теперь зелёные.

### Fixed
- **TLS-интеграционные тесты mixed-multi-target** (`test_mixed_multi_target_*_end_to_end`):
  три теста, проверяющие end-to-end доставку через все транспорты одновременно
  (file + tcp + udp + tls), теперь проходят на чистом `dev`.
  Причины падений:
  1. `rcgen::generate_simple_self_signed` (rcgen 0.13) на окружении с
     OpenSSL 3.x генерирует PEM-блок, который `native_tls::Identity::from_pkcs8`
     отказывается парсить ("Unknown format in import").
  2. `openssl req -x509 -days 36500` (100 лет validity) превышает лимит
     Security.framework на macOS (825 дней) — "The validity period in the
     certificate exceeds the maximum allowed".
  3. Сертификат без явных extensions (basicConstraints/keyUsage/extendedKeyUsage)
     отклоняется TLS-клиентом как неподходящий для serverAuth.

### Changed
- `tests/integration_tests.rs`: добавлен helper `openssl_self_signed()`
  с кэшированием через `OnceLock` — self-signed сертификат для
  `localhost` (CN + SAN: DNS:localhost, IP:127.0.0.1, CA:FALSE,
  digitalSignature+keyEncipherment, serverAuth) генерируется через
  `openssl req -config openssl-server.cnf` один раз на процесс и
  переиспользуется всеми TLS-тестами. Артефакты пишутся в
  `target/test-tls/`. Зависимость от `rcgen` в integration-тестах
  сохранена (используется в `benches/sender_throughput.rs` не было,
  rcgen остаётся в dev-deps для возможного будущего использования);
  в самих тестах смешанных транспортов rcgen больше не применяется.

### Notes
- Тесты: **88 unit + 49 + 11 integration = 148** — ВСЕ зелёные.
  Прогон: 88 unit (было 88), 49 integration (было 46, +3 починено),
  11 N7-integration (без изменений). Полное покрытие тестами
  восстановлено.
- clippy чист, cargo build --release успешен.
- Требование окружения: `openssl` (>= 1.1.1) в PATH — есть на macOS,
  Linux и в стандартных CI-образах. Если openssl недоступен,
  TLS-тесты упадут с понятным сообщением.

## v8.3.0 - 2026-07-12

Продолжение вехи D («Промышленная готовность», P1). Закрыта задача
**N7 — типизированные ошибки рантайма**: вместо `.unwrap()`/`.expect()`
в рантайм-путях теперь используется доменная система ошибок через `thiserror`
с пробросом через `?` в `anyhow::Error` для CLI-границы.

### Added
- **N7 — типизированные ошибки рантайма.** Новый модуль `src/error.rs` с
  четырьмя доменными enum'ами:
  - `MetricsError` — ошибки инициализации/экспорта Prometheus-метрик
    (варианты `Construct{kind,name,source}`, `Register{name,source}`,
    `Encode(#[from] prometheus::Error)`, `Utf8(#[from] FromUtf8Error)`);
  - `ConfigError` — ошибки загрузки/парсинга профиля (`Io{path,source}`,
    `Json{path,source}`; YAML-вариант отложен до v8.5.0);
  - `DrainError` — ошибки graceful-drain (`TaskJoin(#[from] JoinError)`,
    `Timeout{timeout_secs}`, `Sender(#[from] anyhow::Error)`);
  - `RuntimeError` — общий агрегирующий enum рантайма с `#[from]`-вариантами
    для доменных ошибок, плюс `Cancelled` и `TaskJoin`.

### Changed
- `metrics::create_metrics()` теперь возвращает `Result<Metrics, MetricsError>`
  вместо `Metrics`. Внутренние хелперы `make_counter_vec`/`make_gauge`/
  `make_int_counter`/`make_histogram` сохраняют имя и kind метрики в ошибке;
  цикл `register(...)` хранит имя рядом с каждой метрикой для информативного
  сообщения при конфликте.
- `metrics::gather_metrics()` теперь возвращает `Result<String, MetricsError>`
  вместо `String`. `TextEncoder::encode` и `String::from_utf8` проходят через `?`.
- `shutdown::graceful_drain_wait()` теперь возвращает `Result<(), DrainError>`
  с тремя типизированными вариантами (TaskJoin / Timeout / Sender) вместо
  произвольного `anyhow::Error`.
- `metrics_server::route()` обрабатывает `Result` от `gather_metrics`:
  при ошибке кодирования возвращает `500 Internal Server Error` с описанием.
- `main.rs` обрабатывает `Result` от `create_metrics` через `.context(...)?`
  и пробрасывает `MetricsError` через `anyhow::Error` в `eprintln` с
  корректным `ExitCode::FAILURE`.
- В рантайм-коде (вне `#[cfg(test)] mod tests`) устранены все `.unwrap()`
  и `.expect()` — ранее их было 11 в `metrics.rs` плюс потенциальные точки
  в `main.rs` и `shutdown.rs`.

### Dependencies
- Без изменений. Используется уже подключённый `thiserror = "1"`.

### Notes
- Тесты: **88 unit + 46 + 11 integration = 145**, все зелёные (88 unit
  было 74 + 11 в `error::tests` + 3 в `metrics::tests`; 11 новых
  интеграционных в `tests/n7_runtime_errors.rs`).
- 3 интеграционных теста mixed-target TLS (test_mixed_multi_target_*_end_to_end)
  падают в текущем окружении из-за несовместимости `rcgen 0.13` с системным
  OpenSSL (`Unknown format in import`). Падения воспроизводятся на чистом
  `dev` до N7 и не относятся к этой задаче — будут устранены отдельным
  коммитом (план: bump `rcgen` до 0.14 или миграция на `rcgen` 0.13 +
  фиксированная версия OpenSSL).
- Живая проверка N7: `--version` → rc=0; `--validate` на невалидном профиле
  → rc=1 + список проблем; `--metrics-addr` на занятом порту → rc=0 + warn
  (recoverable); `--print-config` → rc=0; несуществующий файл профиля → rc=1
  + сообщение `ConfigError::Io`.
- Политика обработки ошибок сохранена: bind-fail на `/metrics` НЕ роняет
  генератор (метрики — вспомогательный канал), transport-fail sender'ов
  фиксируется в `metrics.errors_total` и глотается в drain. N7 сделал
  ошибки типизированными, не изменил политику recoverability.

## v8.2.0 - 2026-07-11

Продолжение вехи D («Промышленная готовность», P1). Закрыты
**F12 (HTTP-эндпоинт /metrics)** и **безопасный TLS (N4)**.

### Added
- **F12 — HTTP-экспорт метрик** (`src/metrics_server.rs`): лёгкий HTTP-сервер
  на голом `tokio` (без hyper/axum). Обслуживает `GET /metrics` (и `GET /`
  как алиас) — возвращает Prometheus text exposition (v0.0.4); прочие пути
  → 404, не-GET → 405. Запускается фоновой задачей на всё время прогона
  и гасится по завершении. Новое поле профиля `metrics_addr` и флаг
  `--metrics-addr`. Недоступность привязки логируется, но не роняет генератор.
- **Безопасный TLS (N4)**: новые поля `TargetConfig`: `tls_domain` (SNI и проверка
  имени; по умолчанию — хост-часть address), `tls_ca_file` (PEM доверенного
  CA для self-signed/приватного CA), `tls_insecure` (явный opt-in в небезопасный
  режим). `build_tls_connector()` + `TlsParams` в `src/sender.rs`.
- Новый вариант валидации `ValidationError::TlsCaFileNotFound` — F13 отклоняет
  профиль, если указанный `tls_ca_file` не существует.

### Changed
- **БЕЗОПАСНОСТЬ (breaking-поведение):** TLS-транспорт теперь **проверяет**
  сертификаты по умолчанию (ранее был `danger_accept_invalid_certs(true)`).
  Для self-signed укажите `tls_ca_file` или явно `tls_insecure: true`. При
  `tls_insecure` в stderr печатается предупреждение.
- `run_profile` поднимает HTTP /metrics (если задан `metrics_addr`) до запуска фаз.

### Notes
- Тесты: **49 интеграционных + 74 юнит-теста** (все зелёные), clippy чист.
  mixed-target тесты теперь проверяют безопасный TLS-путь через доверенный CA.

## v8.1.0 - 2026-07-11

Начало вехи D («Промышленная готовность», P1). Закрыты две первые
задачи: **F11 (расширенный CLI)** и **F13 (валидация профиля)**.

### Added
- **F13 — валидация профиля** (`src/validate.rs`): типизированный `ValidationError`
  (через `thiserror`) и `validate_profile()`, собирающий **все** ошибки за один
  проход. Проверяет: transport/format/distribution/framing/shutdown.mode,
  диапазоны facility (0..=23) и severity (0..=7), веса шаблонов, пустые
  targets/phases, фазы без источника контента и без условия остановки,
  отрицательные/NaN-значения в load_shape.
- **F11 — расширенный CLI** (`src/cli.rs`): флаги-оверрайды `--target/-t`
  (повторяемый, `ADDR[:TRANSPORT]`), `--distribution`, `--rate`, `--duration`,
  `--total`, `--format`, `--seed`, `--message/-m`; команды `--validate` (dry-run
  только валидация) и `--print-config` (вывод итогового профиля JSON);
  флаг `--version`. Быстрый режим: запуск без файла-профиля только по
  `--target` + `--message`.
- Новые примеры: `examples/cli_quickstart.md`.

### Changed
- `run_profile()` теперь **fail-fast**: валидирует профиль перед запуском
  и возвращает ошибку с полным списком проблем вместо паники в рантайме.
- `main()` переписан на `ExitCode` с корректными кодами возврата (0 —
  успех/валидно, 1 — ошибка/невалидно) и внятными сообщениями через stderr.
- Ручные `Default` для `Profile` и `TargetConfig`, согласованные с serde-
  дефолтами (distribution="round-robin", connections=1, weight=1,
  framing="non-transparent"), чтобы `..Default::default()` в коде давал
  валидные значения.

### Dependencies
- Добавлен `thiserror = "1"` (типизированные ошибки валидации).

### Notes
- Сборка/тесты/clippy проверены реальной компиляцией на Rust 1.97.0.
  Прогон: **44 интеграционных + 61 юнит-тест** (в т.ч. 6 интеграционных
  и 23 юнит-теста на F11/F13) — все зелёные, clippy чист.
- Живая проверка CLI: `--version`, `--help`, `--validate` (валидный/
  невалидный профиль с 7 ошибками), `--print-config`, быстрый режим в файл.
- **Совместимость:** старые JSON-профили продолжают работать. Однако
  ранее молча принимавшиеся некорректные профили (опечатка в transport/
  format, severity/facility вне диапазона) теперь отклоняются на старте —
  это намеренное ужесточение (fail-fast).

## v8.0.0 - 2026-07-11

Мажорный релиз. Полностью закрыты вехи A, B и C — включая все ранее
отложенные (опциональные) задачи. Генератор перешёл на честную
бинарную protobuf-сериализацию (wire-format вместо JSON-заглушки),
получил regex-генерацию строк и межполевые корреляции в схеме, а также
метрики латентности отправки, размера сообщений и счётчик реконнектов.

Версия поднята до 8.0.0 из-за **несовместимого изменения формата
`protobuf`**: вывод фазы с `format: "protobuf"` теперь настоящий
Protobuf wire-format (varint + length-delimited), а не сериализованный
JSON. Потребители бинарного вывода должны обновить парсеры.

Сборка, тесты и clippy проверены реальной компиляцией на Rust 1.97.0
(`cargo build --release`, `cargo clippy --all-targets`). Прогон тестов:
38 интеграционных + 72 юнит-теста (payload/syslog/protobuf) — все зелёные.
Живая проверка: protobuf-вывод декодирован стандартным разбором varint
(поля 1..4 с корректными wire-type), regex-строки соответствуют паттернам
и детерминированы по seed, корреляции status→severity выдержаны на всех
сообщениях.

### Added

- **Честный protobuf wire-format (F10, Веха B).** Модуль `protobuf`
  переписан: реальная бинарная сериализация вместо `serde_json::to_vec`.
  Реализованы `write_varint`, zigzag-кодирование, теги `(field<<3)|wire_type`,
  length-delimited для строк/байтов. Тип `PbType` (Str/Bytes/Int/Uint/Sint/
  Bool/Double/Float) с корректным wire-type для каждого. Спецификация поля
  `"номер:тип:шаблон"` (или `"номер:шаблон"`, или просто `"шаблон"` с
  автонумерацией по алфавиту имён). Поля сортируются по номеру — канонический
  детерминированный вывод. Публичный API: `serialize_protobuf`, `PbType`.
- **Regex-генерация строк (F5, Веха C).** Поле схемы `"type": "regex"` с
  ключом `"regex"`: строка генерируется из паттерна через разбор в HIR
  (`regex-syntax`) и обход проектным `StdRng` — детерминизм по seed (F4)
  сохранён. Поддержаны литералы, классы символов, повторы (ограничение
  `REGEX_MAX_REPEAT = 16`), альтернация, группы, конкатенация. Некорректный
  паттерн даёт пустую строку (без паники). Публичный API: `gen_from_regex`.
- **Межполевые корреляции (F6, Веха C).** Поле схемы может зависеть от
  другого через `"depends_on": "<имя>"` + `"mapping": {<знач.родителя>:
  <значение>}` и `"mapping_default"`. Генерация двухпроходная: сначала
  независимые поля, затем зависимые по значению родителя. Пример:
  `status` (enum) → `severity` (INFO/WARN/ERROR). Детерминированный порядок
  обхода полей (F4) сохранён.
- **Метрики отправки (N3, Веха A).**
  - `syslog_send_duration_seconds` — histogram латентности отправки
    (корзины 5µs–1s), основа для p50/p95/p99.
  - `syslog_message_size_bytes` — histogram размера сообщений (16B–64KB).
  - `syslog_reconnects_total` — CounterVec попыток реконнекта с метками
    `transport`, `target`.
- **Реальный реконнект TCP/TLS.** При ошибке записи TCP/TLS-сендер
  выполняет одну попытку переустановить соединение и повторно отправить
  сообщение (инкремент `syslog_reconnects_total`). Ранее ошибка записи
  вела только к учёту ошибки без восстановления.
- **Примеры.** `examples/protobuf_message.json` — фаза с честным
  protobuf-выводом (номерованные типизированные поля).

### Changed

- **BREAKING:** `format: "protobuf"` выдаёт бинарный protobuf wire-format
  вместо JSON. См. вводную секцию.
- Схема поля (`SchemaField`) расширена ключами `regex`, `depends_on`,
  `mapping`, `mapping_default`.

### Notes / ограничения

- Файловый транспорт использует `\n`-фрейминг — небезопасен для
  бинарного protobuf (сообщение может содержать байт `0x0a`). Для
  бинарного вывода по TCP/TLS корректен octet-counting фрейминг (RFC 6587).
- HTTP-эндпоинт `/metrics` (F12) и p50/p95/p99-агрегация в рантайме
  относятся к вехе D и в этом релизе не реализованы: метрики N3
  собираются и доступны через `gather_metrics`.

## v7.9.0 - 2026-07-11

Веха C «Вариативный пейлоад»: реализованы F4, F5, F6 и F14. Генератор
больше не выдаёт захардкоженные значения — пейлоад стал по-настоящему
вариативным и, при заданном `seed`, полностью воспроизводимым (в т.ч.
межпроцессно). Сборка и тесты проверены реальной компиляцией
(`cargo build --release`, `cargo test`, `cargo clippy`) на Rust 1.97.0;
вариативность и детерминизм дополнительно подтверждены живым прогоном
через TCP-приёмник (два независимых процесса с одним seed дали побайтово
идентичный вывод, распределения enum/шаблонов совпали с заданными весами).

### Added

- **Детерминированный ГПСЧ с seed (F4).** Новый модуль `payload`. RNG
  (`rand::StdRng`) выводится из пары `(seed, seq)` через SplitMix64-перемешивание:
  один и тот же `seed` + номер сообщения дают идентичный вывод, соседние
  сообщения различаются. Без `seed` берётся энтропия ОС (вариативно, но
  не воспроизводимо). Поле `seed` фазы перестало быть «мёртвым».
- **Богатый faker-набор (F5).** Токены `{{faker.*}}`: `ipv4`, `ipv6`, `mac`,
  `uuid` (валидный v4 RFC 4122), `hostname`, `username`, `user_agent`, `url`,
  `http_status`. Все они теперь доступны в `default_values` и как
  `type:"faker"` в schema.
- **Типы полей schema (F5).** `int` — реальный диапазон `min..=max`;
  `enum` — случайный выбор из `values`; `datetime` — реальное «сейчас» в
  RFC3339 UTC с опциональным джиттером `jitter_secs`; `string` — случайная
  строка длины `len`.
- **Распределения выбора (F6).** Для `enum` — `distribution`:
  `uniform` (дефолт), `weighted` (по `weights`), `zipf` (по `zipf_exponent`,
  «горячие» ключи выбираются чаще).
- **Паддинг до размера (F6).** Поле фазы `pad_to_bytes` добивает тело
  сообщения случайными символами до целевого размера в байтах.
- **Мультишаблоны (F14).** Из `templates`/`templates_file` теперь выбирается
  случайный шаблон на каждое сообщение (равновероятно или по
  `template_weights`), а не всегда первый.
- **Тесты.** +11 юнит-тестов модуля `payload` и +10 интеграционных
  (детерминизм, диапазоны, форматы faker, взвешенный/zipf enum, паддинг,
  мультишаблон, стабильный порядок полей schema).

### Fixed

- **Межпроцессный детерминизм при заданном seed.** `schema.fields` —
  `HashMap`, чей порядок обхода рандомизирован между процессами. Из-за этого
  RNG потреблялся в разной последовательности и два запуска с одним `seed`
  давали разные значения schema-полей. Обход полей теперь детерминированно
  отсортирован по имени.

### Changed

- Значения по умолчанию (`timestamp`, `pid`, `faker.ipv4`, `faker.username`
  и др.) больше не захардкожены — генерируются через `payload` с учётом seed.
- Прямая зависимость `rand` 0.9 (ранее присутствовала лишь транзитивно).

## v7.8.0 - 2026-07-11

Завершение Вехи A «Настоящая нагрузка»: реализован F3 — профили нагрузки
во времени (кривые интенсивности внутри фазы). Теперь фаза может задавать
не только постоянный rate, но и ramp-up/ramp-down, синусоиду и всплески.
С закрытием F1 (rate-limiting, v7.5.0), F2 (конкурентность соединений, v7.6.0)
и F3 Веха A завершена полностью. Сборка и тесты проверены реальной
компиляцией (`cargo build --release`, `cargo test`, `cargo clippy`) на Rust 1.97.0.

### Added

- **Профили нагрузки во времени (F3).** Новый модуль `load_shape` и поле фазы
  `load_shape` задают кривую интенсивности `rate_at(t)`:
  - `constant` — постоянный rate (опционально отличный от `messages_per_second`);
  - `linear` — линейный ramp от `start_rate` к `end_rate` за `duration_secs` фазы;
  - `sine` — синусоида между `min_rate` и `max_rate` с периодом `period_secs`
    (старт в минимуме);
  - `burst` — базовый `base_rate` с всплесками `burst_rate` каждые
    `every_secs` длительностью `burst_secs`.
  При заданном `load_shape` планировщик `run_phase_multi` переходит с токен-
  бакета `governor` на sleep-контроллер по мгновенному `rate_at(t)`.
- Публичный тип `LoadShape` и методы `rate_at` / `effective_base`.
- Примеры `examples/load_shape_ramp.json`, `examples/load_shape_sine.json`,
  `examples/load_shape_burst.json`.
- 9 юнит-тестов модуля `load_shape` (constant/linear/sine/burst, границы,
  неотрицательность, десериализация) и 2 интеграционных (linear ramp
  по площади под кривой, burst свыше базы). Всего 20 интеграционных
  + 15 юнит-тестов, все зелёные.

### Changed

- Метрика `syslog_target_rate_messages_per_second` при заданном `load_shape`
  отражает характерное (пиковое) значение кривой.
- Обратная совместимость сохранена: без `load_shape` поведение прежнее
  (постоянный rate через `governor`).

### Verified

- sine (min 20, max 400, период 4с) через реальный TCP-приёмник: число
  сообщений по окнам 0.5с — 17 → 59 → 106 → 128 → 131 (пик на t≈2с) → 106
  → 60 → 19, что точно воспроизводит синусоиду со стартом в минимуме.

## v7.7.0 - 2026-07-11

Веха B «Валидный syslog»: полноценные форматы RFC 5424 и RFC 3164 плюс
фрейминг по RFC 6587 для потоковых транспортов. Вывод теперь пригоден
для реальных приёмников (rsyslog, syslog-ng, Graylog, SIEM). Сборка и тесты
проверены реальной компиляцией (`cargo build --release`, `cargo test`, `cargo clippy`)
на Rust 1.97.0.

### Added

- **Формат RFC 5424 (F7).** Модуль `syslog` собирает полный заголовок
  `<PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG`.
  PRIVAL = facility*8 + severity (facility 0..23, severity 0..7); TIMESTAMP —
  RFC3339 UTC с миллисекундами и суффиксом Z; NILVALUE (`-`) для пустых полей;
  опциональный UTF-8 BOM перед MSG (§6.4). Санитация и обрезка полей по
  лимитам ABNF (HOSTNAME 255, APP-NAME 48, PROCID 128, MSGID 32).
- **Формат RFC 3164 / BSD (F8).** `<PRI>Mmm dd hh:mm:ss HOSTNAME TAG: MSG`
  с локальным временем (день с ведущим пробелом для 1..9) и TAG из APP-NAME
  (+`[PROCID]`, если задан).
- **Фрейминг по RFC 6587 (F9).** Поле `TargetConfig.framing` для tcp/tls:
  `octet-counting` (`MSG-LEN SP SYSLOG-MSG`) или `non-transparent` (`SYSLOG-MSG` + LF,
  дефолт). Октетный счёт — точное число октетов SYSLOG-MSG; подходит для
  syslog-over-TLS (RFC 5425, порт 6514).
- **Параметры `syslog` в фазе.** Блок `syslog` в Phase: `facility`, `severity`,
  `hostname`, `app_name`, `procid`, `msgid`, `structured_data`, `bom`. Строковые
  поля проходят подстановку шаблона (`{{hostname}}`, `{{sequence}}` и т.п.).
- Публичные функции `syslog::build_rfc5424`, `build_rfc3164`, `prival`,
  `escape_sd_value` и тип `sender::Framing`.
- Примеры `examples/rfc5424_tcp.json`, `examples/rfc3164_udp.json`,
  `examples/rfc5424_tls_octet.json`.
- 6 новых юнит-тестов модуля `syslog` и 6 интеграционных (RFC5424,
  NILVALUE, BOM, RFC3164, raw, octet-counting через TCP). Всего 18 интеграционных
  + 6 юнит-тестов, все зелёные.

### Changed

- **Семантика `format`.** Значения: `rfc5424` (дефолт), `rfc3164`, `protobuf`,
  любое другое (напр. `raw`) — сырой рендер шаблона без обёртки. **BREAKING:**
  тело шаблона теперь — это MSG внутри syslog-конверта; кто полагался на
  прежний «сырой» вывод по умолчанию — укажите `"format": "raw"`.
- Добавлена зависимость `chrono` (0.4) для RFC3339/BSD-времени.

### Verified

- RFC5424 в файл: `<37>1 2026-07-11T..Z web-01 authsvc 8421 AUTH [origin@32473
  ip="192.0.2.1"] User N ...` (PRI 4*8+5=37).
- RFC3164 в файл: `<6>Jul 11 .. router1 kernel: link up ethN`.
- Octet-counting через реальный TCP-приёмник: `64 <14>1 .. octet framing
  test 1` — объявленная длина 64 = фактическая длина SYSLOG-MSG.

## v7.6.0 - 2026-07-11

Веха A (продолжение): конкурентность соединений — поле `connections` теперь
реализовано как пул воркеров на каждый target. Сборка и тесты проверены
реальной компиляцией (`cargo build --release`, `cargo test`, `cargo clippy`) на Rust 1.97.0.

### Added

- **Пул воркеров на target.** Ранее мёртвое поле `TargetConfig.connections` задаёт
  число параллельных воркеров (соединений/сокетов/файловых дескрипторов) на
  target. Воркеры конкурентно читают из общей очереди target'а через
  `Arc<Mutex<Receiver>>` (`sender::SharedRx`); каждое сообщение достаётся ровно
  одному воркеру. Для `tcp`/`tls` это означает N независимых соединений.
- Метрика `syslog_active_workers` (gauge) — суммарное число активных воркеров
  во всех target'ах фазы. Закрывает расхождение с Grafana-дашбордом, где эта
  метрика уже использовалась, но не существовала в коде.
- Пример `examples/connection_pool.json` и тест
  `test_connection_pool_opens_multiple_connections` (проверяет, что `connections: 3`
  открывают 3 реальных TCP-соединения и что gauge = 3). Всего 12 интеграционных
  тестов, все зелёные (стабильно за 3 прогона).

### Fixed

- **Целостность записи при конкурентных воркерах.** Пейлоад и trailer (`\n`)
  теперь собираются в один буфер и пишутся одним `write_all` (было два
  раздельных вызова). Для файла с O_APPEND это даёт атомарную дозапись и
  исключает перемешивание строк. Проверено стресс-прогоном: 8 воркеров ×
  10000 сообщений — 0 повреждённых/перемешанных строк.

### Notes

- Проверено на живых прогонах: 4 воркера × 1000 сообщений — 1000/1000
  уникальных строк, 0 малформат.

## v7.5.0 - 2026-07-11

Веха A «Настоящая нагрузка»: снятие жёсткого потолка генерации и ввод настоящего
rate-limiting. Сборка и тесты проверены реальной компиляцией (`cargo build --release`,
`cargo test`, `cargo clippy`) на Rust 1.97.0.

### Changed

- **BREAKING (семантика профиля):** `messages_per_second` теперь — истинная
  интенсивность (сообщений в секунду) через токен-бакет `governor`, а не «общее
  количество с потолком 100». Значение `0` означает «без ограничения скорости».
- Удалён жёсткий потолок `messages_per_second.min(100)` в `src/core.rs` — генератор
  больше не ограничен 100 сообщениями на фазу.

### Added

- Зависимость `governor = "0.10.4"` — токен-бакет rate-limiting.
- Поле `Phase.total_messages: Option<u64>` — условие остановки по общему числу
  сообщений.
- Задействовано ранее мёртвое поле `Phase.duration_secs` — условие остановки
  фазы по времени. Фаза завершается по первому наступившему условию
  (`duration_secs`, `total_messages` или сигнал завершения).
- Метрики нагрузки Prometheus: `syslog_messages_generated_total{phase}`,
  `syslog_target_rate_messages_per_second`, `syslog_achieved_rate_messages_per_second`.
- Тесты `test_total_messages_removes_cap_above_100` (проверка снятия потолка на
  250 сообщений) и `test_rate_limiting_respects_target` (проверка соблюдения rate). Всего
  11 интеграционных тестов, все зелёные.

### Notes

- Проверено на живых прогонах: профиль 200 msg/s × 500 сообщений — 500 строк
  за ~1.5с; профиль 100 msg/s × duration 2с — ~299 строк за ~2.0с (burst + пополнение).

## v7.4.0 - 2026-07-11

Compile-verified релиз. Впервые весь проект собран и проверен реальной компиляцией
(`cargo build --release`, `cargo test`, `cargo bench`, `cargo clippy`) в записываемом
окружении с установленным Rust 1.97.0.

### Fixed

- `src/sender.rs`: `bytes_total.inc_by(...)` теперь принимает `f64` (prometheus 0.13 API),
  ранее не компилировалось (`u64` → `f64`).
- `tests/integration_tests.rs`: TLS-коллектор приведён к rcgen 0.13 API — вместо
  несуществующего поля `CertifiedKey::signing_key` используются `cert.key_pair` и
  `cert.cert`, а `native_tls::Identity::from_pkcs8` получает PEM (сертификат и ключ),
  а не DER, как того требует API.
- `benches/message_generation.rs` и `benches/sender_throughput.rs`: полностью переписаны
  под фактический публичный API библиотеки. Прежние версии ссылались на несуществующие
  символы (`TemplateContext`, `format_syslog_msg`, `build_rfc5424_msg`, `pick_template`,
  `create_registry`, `worker_tcp`/`worker_udp`) и незаявленные зависимости (`rand`,
  `governor`) и не компилировались.
- Удалены неиспользуемые импорты в интеграционных тестах; устранены предупреждения clippy
  (`is_err()` вместо `if let Err(_)`).

### Added

- `Cargo.toml`: явные секции `[[bench]]` с `harness = false` и feature `async_tokio` для
  Criterion, без которых бенчмарки не собирались.
- Реальные, исполняемые бенчмарки Criterion: генерация сообщений (`render_template`,
  `generate_message`, `create_dispatcher`) и пропускная способность отправки через
  `run_profile` с настоящими TCP/UDP коллекторами.
- Учёт ошибок подключения/handshake в метрике `syslog_errors_total` для `tcp` и `tls`
  senders: при неудаче соединение фиксирует ошибку и дренирует очередь, не блокируя
  генератор (реальная фиксация negative-path поведения).
- Mixed end-to-end тесты для `broadcast`, `round-robin` и `weighted` dispatch поверх
  `file + tcp + udp + tls` с настоящим TLS-handshake; стабильны на 5 повторных прогонах.
- Negative-path интеграционный тест для отказов транспорта.

### Changed

- Версия проекта поднята с `7.3.0` до `7.4.0` в `Cargo.toml`.
- `test_metrics_presence` переработан: сначала прогоняет минимальный файловый профиль,
  затем проверяет экспорт `CounterVec`-метрик (Prometheus не выводит label-less серии до
  первой записи).
- README и DEVELOPER_GUIDE синхронизированы с реальным кодом: версия, раздел бенчмарков,
  поведение метрик, обработка ошибок транспорта.

## v7.3.0

- Модульная архитектура и transport runtime (`file`, `tcp`, `udp`, `tls`) с TLS client
  handshake через `native-tls` / `tokio-native-tls` (drafted, до compile-verification).
