# Перенос контекста проекта в Claude — syslog-generator

Дата: 2026-07-12. Текущая версия: **v8.3.1** (compile-verified).

Этот файл — самодостаточный контекст для продолжения работы над проектом в Claude
(Claude Code / Claude.ai). Проект — промышленный генератор нагрузки на syslog на Rust.

---

## 0. Правила общения (ВАЖНО)

- Отвечать всегда **на русском языке**; рассуждения тоже вести на русском.
- Автор: **Антон Герасимов**, CTO ИТ-компании, Москва; уровень Rust/распределённых систем — advanced.
- Предпочтение: **факты, а не домыслы; проверенные источники**.
- **При выпуске версий обязательно обновлять документацию И changelog** (README.md, CHANGELOG.md,
  AUDIT.md, CLAUDE_HANDOFF.md, examples/). Каждая веха завершается compile-verified релизом.
- Прежде чем заявлять результат/метрику — **проверять реальной компиляцией** (`cargo build/test/clippy`).

---

## 1. Что за проект

Промышленный генератор нагрузки на серверы по протоколу **Syslog**, на Rust.

Особенности (ТЗ проекта):
- вариативность шаблонов входных данных;
- поддержка multi-target;
- поддержка профилей нагрузки во времени.

Модульная архитектура, реальный async-runtime на tokio, настоящий TLS client handshake,
Prometheus-метрики с HTTP-эндпоинтом, покрытие интеграционными + юнит-тестами и бенчами Criterion.

---

## 2. Окружение и команды

- **Rust 1.97.0.** Перед любой cargo-командой: `source "$HOME/.cargo/env"`.
- Рабочий каталог проекта: `syslog-generator/` (корень с `Cargo.toml`).
- Типовой паттерн: `cd <проект> && source "$HOME/.cargo/env" && cargo <cmd>`.

### Зависимости (Cargo.toml)
```
anyhow=1, thiserror=1, clap=4(derive), prometheus=0.13,
serde=1(derive), serde_json=1,
tokio=1(macros,rt-multi-thread,signal,sync,time,net,io-util,fs),
tokio-util=0.7(rt), native-tls=0.2, tokio-native-tls=0.3, rcgen=0.13,
governor=0.10.4, chrono=0.4(clock,std), rand=0.9(std,std_rng), regex-syntax=0.8
[dev] criterion=0.5(async_tokio), regex=1
```
> F12 (HTTP /metrics) реализован на голом tokio — **без hyper/axum**. Новых зависимостей для F12/N4 не добавлялось.

### Запуск тестов (в песочнице полный `cargo test` может таймаутить — запускать бинарники отдельно)
```bash
cargo test --no-run
# интеграционные:
BIN=$(ls -t target/debug/deps/integration_tests-* | grep -v '\.d$' | head -1)
timeout 175 "$BIN" --test-threads=1
# юнит (несколько устаревших бинарников; «живой» печатает реальный счёт):
for b in $(ls -t target/debug/deps/syslog_generator-* | grep -v '\.d$'); do
  test -x "$b" && timeout 90 "$b" --test-threads=1
done
```
Текущее зелёное состояние: **60 интеграционных + 88 юнит-тестов**, `cargo clippy --all-targets` чист.
В v8.3.1 починены 3 TLS-target теста (mixed_*_end_to_end), которые до этого
падали из-за несовместимости rcgen 0.13 + OpenSSL. Теперь все тесты зелёные.
Сертификаты для TLS-тестов генерируются через `openssl req -config openssl-server.cnf`
(см. `openssl_self_signed()` в `tests/integration_tests.rs`).

---

## 3. Структура кода (src/, ~3388 строк)

| Модуль | Назначение |
|--------|-----------|
| `main.rs` | Точка входа; `ExitCode` (0/1), парсинг CLI, вызов `run_profile`. |
| `lib.rs` | Реэкспорты публичного API (в т.ч. `build_tls_connector`, `TlsParams`, `build_http_response`, `parse_request_line`, `route`, `serve as serve_metrics`). |
| `cli.rs` | `Args` (clap derive), `Overrides`, `to_overrides()`, `apply_overrides()`, `parse_target()`. Флаги: `--profile/-p`, `--target/-t` (повторяемый `ADDR[:TRANSPORT]`), `--distribution`, `--rate`, `--duration`, `--total`, `--format`, `--seed`, `--message/-m`, `--validate`, `--print-config`, `--metrics-addr`, `--version`, `--help`. |
| `config.rs` | `Profile`, `Phase`, `TargetConfig`, load_shape-конфиг, `Default`-имплы. |
| `core.rs` | `run_profile` / `run_phase_multi`: оркестрация фаз, диспетчеризация (broadcast/round-robin/weighted), спавн sender-задач по транспортам, подъём HTTP /metrics. |
| `sender.rs` | `target_sender_file/tcp/udp/tls`; `TlsParams` + `build_tls_connector`; реконнект TCP/TLS. |
| `metrics.rs` | Prometheus registry, `create_metrics`, `gather_metrics`. |
| `metrics_server.rs` | **F12:** лёгкий HTTP-сервер на tokio (`parse_request_line`, `build_http_response`, `route`, `handle_conn`, `serve`, `spawn`). |
| `error.rs` | **N7:** `MetricsError`, `ConfigError`, `DrainError`, `RuntimeError` (thiserror, `#[from]`-варианты для проброса через `?` в anyhow). В рантайм-коде нет `.unwrap()`/`.expect()`. |
| `validate.rs` | **F13:** `ValidationError` (thiserror), `validate_profile` (собирает ВСЕ ошибки за проход). |
| `payload.rs` | Генерация пейлоада: faker-токены, schema-поля, regex-строки (F5), корреляции (F6), распределения, паддинг. |
| `syslog.rs` | RFC 5424 / RFC 3164 форматирование (PRI/severity/facility/version/SD/timestamp). |
| `protobuf.rs` | **F10:** реальный protobuf wire-format (varint, zigzag, length-delimited). |
| `load_shape.rs` | **F3:** профили нагрузки во времени (constant/linear/sine/burst). |
| `schema.rs`, `template.rs`, `shutdown.rs` | Загрузка схемы/шаблонов; graceful shutdown/drain. |

Тесты: `tests/integration_tests.rs` (mixed-target e2e: file+tcp+udp+tls по всем режимам dispatch, negative-path, F12/N4).
Бенчи: `benches/message_generation.rs`, `benches/sender_throughput.rs` (Criterion).
Документация: `README.md`, `CHANGELOG.md`, `AUDIT.md`, `REVIEW.md`, `docs/USER_GUIDE.md`, `docs/DEVELOPER_GUIDE.md`, `examples/` (профили + `cli_quickstart.md`, `metrics_and_tls.md`).

---

## 4. Ключевые механики (для понимания при доработке)

### F12 — HTTP /metrics (v8.2.0)
- `Profile.metrics_addr: Option<String>` (`#[serde(default)]`) + CLI `--metrics-addr` (переопределяет профиль).
- `run_profile`: если `metrics_addr` задан → `metrics_server::spawn(addr, metrics.clone(), token)` до запуска фаз;
  после всех фаз `token.cancel()` гасит сервер.
- `route(method, path, &Metrics)`: `GET /metrics` и `GET /` (алиас) → 200, `Content-Type: text/plain; version=0.0.4; charset=utf-8`,
  тело = `gather_metrics`; прочий GET → 404; не-GET → 405.
- Недоступность привязки — логируется в stderr, генерацию не роняет (метрики — вспомогательный канал).
- ⚠️ Нюанс для тестов: `syslog_messages_total` — CounterVec, не экспортируется до первого `inc()`;
  скалярные метрики (напр. `syslog_shutdowns_total`) присутствуют всегда.

### N4 — безопасный TLS по умолчанию (v8.2.0)
- **Breaking-поведение:** раньше был жёсткий `danger_accept_invalid_certs(true)`. Теперь TLS проверяет сертификат/имя по умолчанию.
- `TargetConfig` (`#[serde(default)]`): `tls_domain: Option<String>` (SNI/проверка имени, по умолчанию — хост-часть `address`),
  `tls_ca_file: Option<String>` (PEM доверенного CA), `tls_insecure: bool` (явный opt-in в небезопасный режим, default false).
- `sender::TlsParams { domain, ca_pem, insecure }` + `build_tls_connector(&TlsParams)`:
  insecure → `danger_accept_invalid_certs(true)` + `danger_accept_invalid_hostnames(true)`;
  иначе если `ca_pem` → `native_tls::Certificate::from_pem` + `add_root_certificate`.
- Валидация (F13): несуществующий `tls_ca_file` → `ValidationError::TlsCaFileNotFound` (rc=1).
- `tls_insecure=true` печатает предупреждение в stderr.

### Прочее уже сделанное
- F1 rate-limiting (governor), F2 пул соединений (`connections`), F3 load_shape.
- F7–F10 валидный syslog (RFC 5424/3164 + framing RFC 6587/5425) + честный protobuf.
- F4–F6, F14 вариативный пейлоад (seed, faker, regex, корреляции, мультишаблоны с весами).
- F11 расширенный CLI, F13 валидация профиля.
- N3 метрики нагрузки (achieved-rate, histogram латентности/размера, реконнекты).

---

## 5. Дорожная карта (веха D в работе, дальше — E)

**Веха D — «Продакшн-готовность» (P1), 🔄 в работе.**
Сделано: F11 (v8.1.0), F13 (v8.1.0), F12 (v8.2.0), N4 (v8.2.0).
**Осталось в вехе D:**
- **N7 — типизированные ошибки рантайма (v8.3.0):** в рантайм-коде (вне `#[cfg(test)]`)
  устранены все `.unwrap()`/`.expect()`. Введён `src/error.rs` с `MetricsError`,
  `ConfigError`, `DrainError` и общим `RuntimeError` (через `thiserror`).
  `create_metrics()`/`gather_metrics()` возвращают `Result<_, MetricsError>`;
  `graceful_drain_wait` — `Result<(), DrainError>`. Ошибки пробрасываются через `?`
  в `anyhow::Error` на границе CLI и уходят в `eprintln` с `ExitCode::FAILURE`.
  Политика recoverability (bind-fail на `/metrics`, transport-fail sender'ов) сохранена.
- **CI:** пайплайн (build/test/clippy/fmt), возможно бенч-гейт.
- **Формальная JSON Schema / YAML-ввод** профиля.
- Синхронизация Grafana-дашборда с реальными метриками (N2).

**Веха E — «Зрелость» (P2), после D:**
- Доп. форматы: CEF / LEEF / JSON-lines.
- Sink: Kafka / Redpanda.
- Сценарии аномалий.
- Docker / docker-compose, рефакторинг слоёв.
- Опционально по TLS: mTLS (клиентский сертификат), настройка min-TLS-version / cipher policy.

---

## 6. Процесс релиза (чек-лист, применялся для v8.1.0, v8.2.0, v8.3.0)

1. Реализовать задачу(и) вехи, добавить юнит- + интеграционные тесты.
2. `cargo build` + `cargo clippy --all-targets` — чисто; тесты зелёные (см. раздел 2).
3. Живые проверки поведения бинарником (напр. `curl /metrics` → 200/404; TLS insecure-warn; `--validate` rc=1).
4. Bump версии в `Cargo.toml`.
5. `cargo build --release`; `--version` показывает новую версию.
6. Обновить `CHANGELOG.md` (новая секция `## vX.Y.Z - ГГГГ-ММ-ДД`, Added/Changed/Notes),
   `README.md` (версия + новые разделы), `AUDIT.md` (пометить задачи «✅ Сделано (vX.Y.Z)», статус вехи),
   `examples/` (новые примеры/README).
7. `cargo clean` в проекте, затем упаковать:
   `zip -rq syslog-generator-vX.Y.Z-verified.zip syslog-generator -x '*/target/*' -x '*/.git/*' -x '*.zip'`;
   проверить версию в Cargo.toml внутри архива.

---

## 7. История версий (кратко)

- **v7.4.0** — базис аудита (исторический снимок), от него составлен AUDIT.md.
- **v7.5.0–v7.9.0** — F1 rate, F2 connections, RFC-форматы + framing, F3 load_shape, F4/F14 seed+мультишаблоны.
- **v8.0.0** — закрыты вехи A/B/C полностью (F5 regex, F6 корреляции, F10 честный protobuf, N3 метрики).
- **v8.1.0** — начало вехи D: F11 (расширенный CLI), F13 (валидация профиля с типизированными ошибками).
- **v8.2.0** — веха D: F12 (HTTP /metrics), N4 (безопасный TLS по умолчанию).
- **v8.3.0** — веха D: N7 (типизированные ошибки рантайма).
- **v8.3.1** — починка 3 упавших TLS-интеграционных тестов (rcgen + OpenSSL). ← текущая.

---

## 8. Как продолжить в Claude

1. Распаковать актуальный архив `syslog-generator-vX.Y.Z-verified.zip` (в нём весь исходник без `target/`).
2. Прочитать `AUDIT.md` (полный план и статусы), `CHANGELOG.md`, этот файл.
3. Взять следующую задачу вехи D (рекомендуется **N9 — CI-пайплайн**, затем D3 — JSON Schema + YAML).
4. Соблюдать процесс релиза из раздела 6 и правила общения из раздела 0.

---

## 9. Пример стартового промпта для Claude

Скопируй текст ниже в первое сообщение новой сессии Claude, приложив архив
`syslog-generator-v8.3.1-verified.zip` (или распакованный проект). При работе в Claude Code
достаточно открыть каталог проекта — файл `CLAUDE_HANDOFF.md` уже лежит в корне.

```text
Ты — старший инженер Rust, продолжаешь мой проект «syslog-generator»: промышленный
генератор нагрузки на серверы по протоколу Syslog. К сообщению приложен архив проекта
(исходники без target/). В корне лежит файл CLAUDE_HANDOFF.md — это полный контекст
переноса: прочитай его ПЕРВЫМ, затем AUDIT.md и CHANGELOG.md.

Правила работы (обязательны):
1. Отвечай и рассуждай на русском языке.
2. Я предпочитаю факты домыслам и проверенные утверждения — не выдумывай результаты
   и метрики. Любой заявленный результат (тесты, поведение, счётчики) подтверждай
   реальной компиляцией и запуском: cargo build, cargo clippy --all-targets, cargo test
   (перед cargo всегда: source "$HOME/.cargo/env"). Rust 1.97.0.
3. Если полный `cargo test` таймаутит — запускай тестовые бинарники отдельно
   (способ описан в разделе 2 CLAUDE_HANDOFF.md).
4. При выпуске версии ОБЯЗАТЕЛЬНО обновляй документацию И changelog:
   Cargo.toml (bump), CHANGELOG.md, README.md, AUDIT.md (пометь задачи «✅ Сделано (vX.Y.Z)»
   и статус вехи), CLAUDE_HANDOFF.md, examples/. Следуй чек-листу релиза из раздела 6.
5. Compile-verified релиз: код собирается, clippy чист, все тесты зелёные.

Текущее состояние: версия v8.3.1. Веха D («Продакшн-готовность») в работе; закрыты
F11, F13, F12 (HTTP /metrics), N4 (безопасный TLS по умолчанию), N7 (типизированные
ошибки рантайма), починены все упавшие TLS-тесты (v8.3.1).

Задача на эту сессию: возьми следующую задачу вехи D — N9 (CI-пайплайн): GitHub
Actions workflow .github/workflows/ci.yml с шагами fmt / clippy -D warnings /
build / test (отдельные стадии по CLAUDE_HANDOFF.md §2 из-за таймаутов sandbox)
/ bench --no-run / опционально audit+deny. README — бейджи CI. Сначала предложи
короткий план (какие шаги, в каком порядке, какие dev-зависимости), дождись моего
«ок», затем реализуй с тестами и выпусти v8.4.0 по чек-листу.

Начни с чтения CLAUDE_HANDOFF.md, AUDIT.md, и краткого плана по N9. Вопросы задавай
до начала правок, если что-то неоднозначно.
```

> Архив — `syslog-generator-v8.3.1-verified.zip` (текущая версия). Если хочешь начать не с N9, а с другой
> задачи вехи D (D3 JSON Schema + YAML, N2 синхронизация дашборда) — замени абзац «Задача на эту сессию».
