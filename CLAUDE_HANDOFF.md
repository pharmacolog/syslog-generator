# Перенос контекста проекта в Claude — syslog-generator

Дата: 2026-07-22 (выпущен v10.7.19, planning v10.7.21). Текущая версия: **v10.7.19** на main (TBD после merge release PR). Release train: dev → release/v10.7.19 → main → tag. Phase 14 Step 1+2+3 (PRs #63, #66, #69) — TLS mock + Kafka refactor + Tier 2 coverage на tls.rs (79.87%) + kafka.rs (77.80%). PR-Q series #70-#77 — release-pgo.yml infra fix (LLVM version mismatch, final fix в PR #77: stable rustc + LLVM 20 tarball download). PR #64 (notify-telegram jq syntax fix), PR #62+#48 (Dependabot maintenance). Coverage cumulative v10.7.16→v10.7.19: tls.rs 58.94%→79.87% (+20.93pp), kafka.rs 51.68%→77.80% (+26.12pp). TOTAL: 91.10%→**94.03%** lines (+2.93pp). 400 unit + 96 integration + 11 proptest tests pass; clippy clean. **Solo-maintainer policy (PR-19/22)**: все мержи через PR, PR-only flow, sync-main-to-dev workflow активен. **Protocol для будущих releases** (см. CLAUDE_HANDOFF §6): feature → dev → release/vX.Y.Z → main → tag.

**v10.7.18 (2026-07-21, patch) — Phase 14 Step 1+2 + CI hardening.** См. §7 history для details.

**v10.7.17 (2026-07-21, patch) — Phase 13: TCP reconnect race fix.**
- PR #58 (`9c55f55`) — merge commit `e28b461`, tag `v10.7.17`, release published.
- Решение в `src/transport/tcp.rs`: `#[tokio::test(flavor = multi_thread, worker_threads = 2)]` + `server_started_tx/Rx` oneshot sync (server сигналит BEFORE `accept()`) + accept loop с timeout + `Option<Sender>` для cancel test + tolerance ranges (errors_total ∈ [1..=3], reconnects_total ∈ [0..=10]) + `stream.read(1 byte)` перед RST.
- Без `#[ignore]`, без `tokio::time::sleep` (no Sleepy Test pattern).
- Coverage `transport/tcp.rs`: 84.75% → 98.33% (+13.58pp).
- 391 unit + integration tests, 0 ignored.
- PR-only flow: PR #60 (release/v10.7.17 → main), reviewed через subagent, merge --admin (solo-maintainer policy — PR-Q для собственных PR).
- Sync main → dev через auto-sync workflow, PR #59 «🔄 Sync main → dev after 9c55f55» MERGED.

### Git Traceability для v10.7.21 (следующий minor release)

Не создаю `release/v10.7.21` branch заранее — это было бы misleading (branch показывает "линейную историю разработки v10.7.21 release-train", который физически не существует).

**Использован механизм `git notes`:**
- `git notes --ref=v10.7.16 add -m "..." <v10.7.16^{}>`
- `git push origin refs/notes/v10.7.16`
- Просмотр: `git notes --ref=v10.7.16 show v10.7.16`

**Note содержит:**
- План трассировки: v10.7.21 будет cut как release/v10.7.21 branch от v10.7.16 commit (FF)
- Release-train v10.7.21: 5 шагов когда наступит (feature → dev → release → main → tag)
- Trigger: после существенных изменений (PR-17f+, coverage 97%, новые PR-quality)

**Преимущества подхода:**
1. ✅ Не загрязняет git namespace (нет ложной ветки)
2. ✅ Searchable через `git log`, `git notes list`
3. ✅ Pushable через `refs/notes/*` (постоянный commit-time artifact)
4. ✅ Не меняет `git tag --list` или `git branch --list`
5. ✅ Когда release-train v10.7.21 реально стартует — branch будет создан из этого места

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

## 0.5 Обязательный Git Flow (с v10.7.15)

**Все мержи в syslog-generator — ТОЛЬКО через GitHub Pull Request.** Это
enforced через [Branch Protection Rules](.github/branch-protection.md)
и обязательно для всех участников (включая maintainers).

### Иерархия веток

| Branch | Назначение | Кто мерджит | Способ | Защита |
|---|---|---|---|---|
| **`main`** | Стабильный релизный код | Maintainer (с review) | Только PR | 7 checks + 1 review + linear |
| **`dev`** | Интеграционная ветка. Всегда зелёная. | Maintainer | PR (auto-sync через workflow) | 7 checks, no review |
| `feature/*`, `fix/*` | Новые фичи/фиксы | Author + Maintainer | PR → dev | (нет protection) |
| `release/vX.Y.Z` | Подготовка релиза | Maintainer | PR → main | (нет protection) |

### Поток изменений

```text
feature/pr-N-* → PR → dev → CI green (7 checks) → merge
                                              ↓
                              (когда готов релиз ↓
                                              ↓
                                              → release/vX.Y.Z → PR → main → CI green → merge
                                                                                              ↓
                                                                          auto-sync main → dev (workflow)
                                                                                              ↓
                                                                          PR merge → CI green → merge
```

### Required status checks (7 blocking jobs для main и dev)

1. `Test (ubuntu-latest)` — primary test run
2. `MSRV check (blocking, v10.5.0)` — Rust MSRV enforcement
3. `cargo-deny (advisories + licenses, blocking)` — security + license
4. `cargo-machete (unused deps, blocking)` — unused dependency detection
5. `cargo public-api snapshot (blocking)` — public API stability
6. `Coverage (cargo-llvm-cov + codecov upload)` — coverage ≥ 87%
7. `Test kafka feature (ubuntu-latest)` — kafka feature integration

Плюс non-blocking: `Test (macos-latest)`, `Build & push (Docker)`, `Generate CycloneDX SBOM`, `Analyze (actions/rust)` (CodeQL).

### Запрещено

- ❌ `git push origin main` — branch protection блокирует
- ❌ `git push origin dev` (для maintainers допустимо только через PR — для sync используется workflow)
- ❌ Force push в любую защищённую ветку
- ❌ Merge PR с красными CI (strict mode enforced)
- ❌ Merge без review для main (1 approval required)
- ❌ Squash merge для dev (для traceability оставляем merge commits)
- ❌ Локальный `git merge origin/main && git push origin dev` (используется auto-sync workflow)

### Автоматизация

- `.github/workflows/sync-main-to-dev.yml` — auto-sync PR после merge в main
- `.github/PULL_REQUEST_TEMPLATE.md` — стандартный checklist
- `.github/branch-protection.md` — конфигурация protection rules
- `scripts/quality-gates.sh` — локальные gates (G1..G10)

### Lessons learned

- **v10.7.15 (PR-18):** Code injection в `notify-telegram.yml` через `${{ github.event.workflow_run.head_branch }}`. Исправлено через PR #18 — перенос user inputs в `env:` блок. PR-only flow поймал это через CodeQL analyze.
- **v10.7.15 (PR-17):** Bash syntax error в `ci.yml` (лишний `fi` после PR-15 merge). CI упал с exit code 2. Branch protection для main требует CI green — но я поторопился с merge до зелёного CI. Теперь защита от повторения: strict required_status_checks + workflow `sync-main-to-dev.yml` (нельзя merge dev → main напрямую, только через release flow).

**Полная конфигурация:** [.github/branch-protection.md](.github/branch-protection.md)

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

- **Rust 1.95.0 (MSRV).** Перед любой cargo-командой: `source "$HOME/.cargo/env"`.
  Канал зафиксирован в `rust-toolchain.toml` (`channel = "1.95"`); реальный
  rustc проверяется скриптом `scripts/check-toolchain.sh` (pre-push hook).
- Рабочий каталог проекта: `syslog-generator/` (корень с `Cargo.toml`).
- Типовой паттерн: `cd <проект> && source "$HOME/.cargo/env" && cargo <cmd>`.

### Зависимости (Cargo.toml)
```
anyhow=1, thiserror=1, clap=4(derive), prometheus=0.13,
serde=1(derive), serde_json=1, serde_yaml=0.9,
tokio=1(macros,rt-multi-thread,signal,sync,time,net,io-util,fs),
tokio-util=0.7(rt), native-tls=0.2, tokio-native-tls=0.3, rcgen=0.13,
governor=0.10.4, chrono=0.4(clock,std), rand=0.9(std,std_rng),
regex-syntax=0.8, jsonschema=0.18, bytes=1,
rskafka=0.5 (optional, features="compression-{gzip,lz4,snappy,zstd}", за feature flag `kafka`)
[dev] criterion=0.5(async_tokio), regex=1, proptest=1, socket2=0.5
```
> F12 (HTTP /metrics) реализован на голом tokio — **без hyper/axum**. Новых зависимостей для F12/N4 не добавлялось.

### CI (N9, v8.4.0)
GitHub Actions workflow `.github/workflows/ci.yml` запускается на каждый push
в `main`/`dev` и PR в `main`. Матрица: `ubuntu-latest` + `macos-latest`.
Стадии: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` →
`cargo build --release --locked` → `cargo test --no-run --locked` →
`cargo test --locked` → `cargo bench --no-run --locked`. Кэш cargo через
`Swatinem/rust-cache@v2`. На Linux устанавливается `libssl-dev` для
`openssl-sys` (нужен native-tls). Дополнительно — best-effort job `msrv`,
читающий канал из `rust-toolchain.toml` (если файл есть).

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
Текущее зелёное состояние: **70 интеграционных + 142 юнит-теста + 11 N7 + 9 бенчей (3 + 6) = 223 теста**, `cargo clippy --all-targets --features kafka -- -D warnings` чист. Без feature `kafka`: 68 integration + 142 unit = 210 тестов.
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
Сделано: F11 (v8.1.0), F13 (v8.1.0), F12 (v8.2.0), N4 (v8.2.0), N7 (v8.3.0),
v8.3.1 (починка TLS-тестов), N9 (v8.4.0, CI-пайплайн),
v8.4.1 (починка sender_thoughput бенчмарков после регрессии F13),
D3 (v8.5.0, формальная JSON Schema + YAML-ввод),
**N2 (v8.6.0, синхронизация Grafana-дашборда)**.

**Веха D закрыта полностью (v9.0.0).** Все P1-задачи (F11, F12, F13, N4, N7, N9,
D3, N2) сделаны. См. CHANGELOG.md и AUDIT.md §5.
- **Веха E в процессе**: v9.1.0 (N10), v9.2.0 (F15), **v9.3.0 (F16)** сделаны.
  Следующие: v9.4.0 (F17), v9.5.0 (N4.cipher_policy), v9.6.0 (N12).
- **N7 — типизированные ошибки рантайма (v8.3.0):** в рантайм-коде (вне `#[cfg(test)]`)
  устранены все `.unwrap()`/`.expect()`. Введён `src/error.rs` с `MetricsError`,
  `ConfigError`, `DrainError` и общим `RuntimeError` (через `thiserror`).
  `create_metrics()`/`gather_metrics()` возвращают `Result<_, MetricsError>`;
  `graceful_drain_wait` — `Result<(), DrainError>`. Ошибки пробрасываются через `?`
  в `anyhow::Error` на границе CLI и уходят в `eprintln` с `ExitCode::FAILURE`.
  Политика recoverability (bind-fail на `/metrics`, transport-fail sender'ов) сохранена.
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
- **v8.3.1** — починка 3 упавших TLS-интеграционных тестов (rcgen + OpenSSL).
- **v8.4.0** — веха D: N9 (CI-пайплайн на GitHub Actions).
- **v8.4.1** — починка sender_throughput бенчмарков (F13 regression).
- **v8.5.0** — веха D: D3 (формальная JSON Schema + YAML-ввод профиля).
- **v8.6.0** — веха D закрыта: N2 (синхронизация Grafana-дашборда).
- **v8.6.1** — закрытие P1-пробелов: N5 (CompiledTemplate, ~100x шаблонов), N8 (round-trip RFC 5424), N11 (docs/.meta.json).
- **v8.7.0** — N6 zero-copy/буферизация (BytesMut, BufWriter, уменьшение syscall'ов в ~50-100 раз).
- **v8.7.1** — N8 proptest: 6 property-based тестов для payload (int/seed/pad/faker IPv4/faker UUID).
- **v8.7.2** — N4.mTLS: 3 новых TargetConfig-поля (tls_client_cert_file, tls_client_key_file, tls_min_protocol_version), 9 новых тестов.
- **v8.8.0** — N10 рефакторинг слоёв: `src/format/` (RFC 5424/3164/raw/protobuf), `src/transport/` (file/tcp/udp/tls), `src/observability/` (metrics + HTTP), `src/generator/` (orchestration). 0 breaking changes.
- **v8.8.1** — patch-долг: правки `AUDIT.md` (поставлены ✅ на F7/F8/F9, убраны устаревшие пометки «Отложено» из F13 и N4). Код без изменений. ← текущая.
- **v9.0.0** — milestone-релиз: веха D «Продакшн-готовность» ЗАКРЫТА. Major-бамп без breaking changes (0 изменений в API). Публичный API полностью backward-compatible.
- **v9.1.0** — N10 trait Format + TransportKind (dyn-dispatch, async fn в trait). 0 breaking changes. Подготовка к F15 (CEF/LEEF/JSON-lines) и F16 (Kafka/Redpanda).
- **v9.5.1** — F17 сценарии аномалий: tagged enum `AnomalyKind` (BurstInjection, SlowDrip, PacketLoss), `Phase.anomalies`, `AnomalyPlanner`, метрики `syslog_anomalies_applied_total`/`syslog_anomalies_dropped_total`, валидация F13 (6 новых `ValidationError`), JSON Schema `Anomaly`. 0 breaking changes относительно v9.5.0 (patch поверх N4.cipher_policy + rustls миграции). 21 новый тест.
- **v9.6.0** — N12 Docker/musl/docker-compose: multi-stage `Dockerfile` (distroless/cc-debian12, ~25 MB), `.dockerignore`, `docker-compose.yml` (syslog-generator + syslog-ng + prometheus + grafana), `docker/syslog-ng.conf`, `docker/prometheus.yml`, `examples/profile-docker.yaml`, `.github/workflows/docker.yml` (multi-arch linux/amd64 + linux/arm64, push в ghcr.io). В составе release-train: N4.cipher_policy + rustls миграция (BREAKING), F17 (anomalies), F16 (Kafka через `rskafka` opt-in feature + файловая ротация + exponential backoff reconnect), hotfix `mtls_cipher_policy.json`, CI-фиксы (flaky tests). 314 тестов (215 unit + 88 integration + 11 n7) — все зелёные. **Веха E «Зрелость» ЗАКРЫТА.**
- **v10.0.0** — Веха F «Production-hardened» старт: breaking B1 (`TlsVersion::V1_2 → Tls12`), B2 (удалён `pub use self::protobuf::*`), B6 (`rust-version = "1.95"`, удалён `rcgen`), B7 (`Format::name()` → `Display`). B3 (MetricsError структурный), B4 (ValidationError структурный), B5 (CLI `--target split`) перенесены в v10.1.0. PLAN split: `PLAN-v10.0.0.md` (веха F, 8 релизов) + `PLAN-веха-E.md` (история вехи E). Cleanup docs: удалены redirect-stub'ы `docs/docs-{developer,user}.md`. 314 тестов — все зелёные.
- **v10.1.0** — Performance ч.1: `lto = "fat"` + `codegen-units = 1` в Cargo.toml release profile (5-15% throughput gain). Bench-regression monitoring в CI (non-blocking, `cargo bench -- --quick` с выводом в артефакт `bench-output-${{ matrix.os }}`). Breaking B5 (CLI `--target split`): deprecated alias `ADDR:TRANSPORT` (warning в stderr), новый формат `-t ADDR --transport TRANSPORT`. Полный deprecation в v11.0.0. **B3+B4 — N/A**: `MetricsError`/`ValidationError` уже структурные с v8.x. 317 тестов (218 unit + 88 integration + 11 n7) — все зелёные.
- **v10.2.0** — Performance ч.2: hot-path оптимизация faker-генераторов. Все `format!()` с многоэтапными аллокациями заменены на `String::with_capacity(N)` + `write!()` через `std::fmt::Write`. Затронуты `faker.ipv4`, `faker.ipv6`, `faker.mac`, `faker.hostname`, `faker.url`, `faker.uuid`, `random_string`. Bench: `generate_message_from_template` 6.96µs → **5.17µs** (-26%), `template_render_realistic` 758ns → 720ns (-5%), `create_dispatcher_weighted` 60ns → 52ns (-13%). 311 тестов — все зелёные. Lock-free atomics и BytesMut pre-alloc — N/A (prometheus AtomicU64 + N6 уже оптимизировано).
- **v10.3.0** — Coverage ч.1: `cargo-llvm-cov` baseline **86.40% lines / 88.36% functions / 86.49% regions**. Новый non-blocking CI job `coverage` (ubuntu-latest) устанавливает `cargo-llvm-cov` через `taiki-e/install-action@v2`, запускает `cargo llvm-cov --features kafka --workspace --lcov`, загружает артефакты `lcov.info` + `coverage-summary.txt`. `docs/COVERAGE.md` — документация по coverage + план v10.4.0 (покрыть непокрытые модули до ≥ 97% lines). Топ непокрытых: `transport/tcp.rs` 46.72%, `transport/kafka.rs` 51.68%, `transport/mod.rs` 63.33%, `shutdown.rs` 67.44%, `transport/tls.rs` 68.44%. 317 тестов — все зелёные.
- **v10.3.1** — **Patch-fix CI**: `cargo fmt` фикс для `src/payload.rs:97` (`write!(...).expect(...)` в одну строку). CI был сломан на `cargo fmt --all -- --check` после v10.2.0; v10.3.1 восстанавливает зелёный CI. 🚨 **CI green обязателен перед merge в main** — добавлено явное правило в `PLAN-v10.0.0.md` §4 п.15 и § Release-gate workflow. Удалены локальные release-ветки `release/v10.{0,1,2,3}.0`. 317 тестов — все зелёные.
- **v10.4.0** — Coverage ч.2: покрытие **87.07% lines / 89.38% functions / 87.20% regions** (baseline v10.3.0 = 86.40%, +0.67pp). Cargo-fuzz: 5 таргетов (`profile_parser`, `format_rfc5424`, `format_cef`, `format_leef`, `format_json_lines`), `fuzz/Cargo.toml`, `docs/FUZZING.md`. Coverage gate запланирован на v10.4.1+.
- **v10.4.1** — patch-fix flaky: 3 time-sensitive теста (расширены допуски, добавлены `sleep` в setup для стабилизации тайминга на shared CI runners).
- **v10.4.2** — patch-fix flaky: 2 TLS mTLS теста через `OnceLock`-кэширование `make_test_cert` (раньше каждый вызов генерил новый сертификат — в редких случаях давал разные fingerprints). Release-gate возвращён после восстановления CI: `cargo-llvm-cov` через `taiki-e/install-action@v2` работает.
- **v10.4.3** — CI fix: `cargo install` вместо `taiki-e/install-action` для `cargo-llvm-cov`/`cargo-deny`/`cargo-machete`. Зависимости Dependabot обновлены.
- **v10.4.4** — CI fix: правильный multi-arch Docker build через два job'а (linux/amd64 + linux/arm64 отдельно, чтобы избежать QEMU emulation на build time). Docker workflow теперь trigger'ится на `release/v*.*.*` для arm64.
- **v10.5.0** — CI расширение (cargo-deny, cargo-machete, MSRV-blocking, Dependabot). `rust-toolchain.toml` для blocking MSRV check.
- **v10.5.1** — hotfix: dependabot `dependency-type: 'direct'/'indirect'` — убрать groups (Dependabot schema validation).
- **v10.5.2** — hotfix: Docker Smoke test skip on PR (build без push) + Dependabot groups optimization.
- **v10.5.3** — Dependabot batch updates (GH Actions major bumps + webpki-roots 1.0 + prometheus 0.14).
- **v10.6.0** — Usability ч.1: shell completions (bash, zsh, fish, powershell, elvish) через `clap_complete` 4; man page generation через `clap_mangen` 0.2 (subcommand `syslog-generator man`); colored error output через `owo-colors` 4 (auto-detect NO_COLOR env).
- **v10.7.0** — Usability ч.2: structured logging через `tracing` + `tracing-subscriber` (env-filter, RUST_LOG поддержка); progress bar `indicatif` (только при `duration_secs > 30` И TTY); `--dry-run` (печатает план без отправки).
- **v10.7.1** — **Закрытие вехи F**: breaking deps cleanup (`indicatif 0.18`, `criterion 0.8`); double Ctrl-C = hard shutdown через `AtomicUsize` counter; `--config` (auto-detect JSON/YAML по расширению, alias `--profile`); `completions <shell>` и `man` subcommands.
- **v10.7.2** — Dependabot maintenance: clap_mangen 0.2 → 0.3, indicatif 0.17 → 0.18, rand 0.10 ОТКАЧЕНО до 0.9 (breaking API требует hot-path rewrite → перенесено в v10.7.3+).
- **v10.7.3** — **Patch-release (PR-1): critical fixes по результатам аудита v10.7.2 + CI hardening**. C1: duplicate `src/protobuf.rs` (354 строки) → thin re-export. M2: dead code `reconnect_tcp` удалён. M3: `tls_connect` mis-annotation исправлена. M4: `KafkaFeatureDisabled` теперь реально emit'ится через `cfg!(feature = "kafka")` (раньше declared but never pushed → silent fail). N2: broken placeholders в `examples/templates/templates_basic.json` (random_int, real_action) → sequence/real_command. N3: cipher_suites parsing bug (`out.clear()` отбрасывал валидные suites) — теперь сохраняет. N10: 2 rustdoc warnings исправлены. **CI improvements**: test-kafka job валидирует ВСЕ примеры (не только kafka_redpanda.yaml), Test job пропускает `kafka_*`. 337 unit + 86 integration + 11 n7 = 434 теста. Все Quality Gates зелёные.
- **v10.7.4** — **Patch-release (PR-2): safety & correctness по результатам аудита v10.7.2**. H5: SIGTERM handler (для Docker/K8d). N6: hoist CancellationToken в run_profile (single shared listener вместо per-phase). N12: TLS `close_notify` перед exit. M7: JoinHandle tracking для HTTP server. N5: reconnect_config + tls_params параметры через Transport trait (использование в PR-4). N19: Dockerfile MSRV fix (1.95 вместо 1.97). N14: `ensure_rustls_provider_for_tests` под feature `test-helpers`. 339 passed (242 unit + 86 integration + 11 n7). Все Quality Gates зелёные.
- **v10.7.5** — **Patch-release (PR-3): README overhaul + SSDLC baseline**. README.md полностью переписан по best practices (tagline, 11 бейджей, quick start, features, installation, CLI, profile format, architecture, performance, security). SECURITY.md (vulnerability disclosure policy, supported versions matrix, threat model, response timeline Google Project Zero 90-day, cryptographic inventory, dependency policy). CONTRIBUTING.md (workflow, Quality Gates, code style, testing requirements, format/transport добавление, Conventional Commits, release process). CODE_OF_CONDUCT.md (Contributor Covenant v2.1). scripts/quality-gates.sh + check-n7-invariant.sh + check-changelog.sh. codecov.yml (coverage badge в README). benches/hot_path.rs для per-msg perf baseline (PR-10).
- **v10.7.6** — **Patch-release (PR-4): minimal architecture cleanup**. `OnceLock` вместо `Once` в rustls provider init. `default-features = false` на serde/prometheus (compile time + binary size reduction). Crate-level lints: `#![deny(unsafe_code)]` + `#![warn(clippy::all)]`. (Минимальные изменения — большие рефакторинги отложены.)
- **v10.7.7** — **Patch-release (PR-10): hot-path performance optimizations**. `generate_message_with_format` 3.79 µs/msg → **2.01 µs/msg** (-47%, target ≤ 2 µs ✅). Throughput 264 → 498 Kelem/s (+89%). Оптимизации: `PhaseContext` pre-compile templates (`CompiledTemplate`), cached syslog header (5× re-render per message eliminated для static syslog fields), faker scan + skip unreferenced fakers, pre-built faker keys (avoid 9× `format!` per message), `pick_template_compiled` для borrowed templates. -47% baseline reduction.
- **v10.7.8** — **Patch-release (PR-6): extended bench coverage**. Иерархическая `benches/format/` (CEF/LEEF/JSON-lines) + `benches/transport/` (TLS/file_rotation/reconnect). 9 bench binaries (было 2). Каждый компилируется отдельно.
- **v10.7.9** — **Patch-release (PR-11): Test coverage + gate**. 87.94% lines coverage. Coverage gate ≥ 87% blocking в CI через `cargo llvm-cov --fail-under-lines=87`. 41+ новых тестов (validate.rs kafka/cipher policy, TCP/UDP/raw/rfc3164 sender loops, generator/core helpers). codecov badge через shields.io. Allow list для main.rs/tls.rs/kafka.rs (непокрываемые unit-тестами).
- **v10.7.10** — **Patch-release (PR-9): README overhaul + SSDLC docs**. Полностью переписан README (11 бейджей, key features, installation, CLI, profile format, architecture overview). SECURITY.md (vulnerability disclosure, threat model). CONTRIBUTING.md (workflow, Quality Gates, Conventional Commits). CODE_OF_CONDUCT.md (Contributor Covenant v2.1). scripts/quality-gates.sh + check-n7-invariant.sh + check-changelog.sh. codecov.yml + coverage upload в CI.
- **v10.7.11** — **Patch-release (PR-10): hot-path performance**. `generate_message_with_format` 3.79 µs/msg → 2.01 µs/msg (-47%, target ≤ 2 µs ✅). Throughput 264 → 498 Kelem/s (+89%). Optimizations: `PhaseContext` pre-compile templates + cached syslog header + faker scan + skip unreferenced fakers.
- **v10.7.12** — **Patch-release (PR-11): Test coverage + gate**. 87.94% lines. Coverage gate ≥ 87% blocking. 41+ новых тестов (validate.rs kafka/cipher, TCP/UDP/raw/rfc3164 sender loops, generator/core helpers).
- **v10.7.13** — **Patch-release (PR-12): Security hardening + SSDLC**. F13 gate для `tls_insecure=true` (MITM-trivial fix). `Zeroizing<Vec<u8>>` для TLS private keys. Drop `RSA_PKCS1_SHA1` из `NoCertVerifier`. `tracing::warn!` для SIEM-indexed security warnings. `yanked = "deny"` в deny.toml. SBOM generation (cargo-cyclonedx). Docker SLSA Build L1 (provenance + sbom). Threat model в SECURITY.md. License policy drift исправлен.
- **v10.7.14** — **Patch-release (PR-13): N7 invariant cleanup + Quality Gates extension**. После PR-10/12 осталось несколько `.expect()` и `unreachable!()` в runtime коде — все заменены на graceful fallbacks (5 в `src/format/json_lines.rs`, 4 в `src/validate.rs`, 1 в `src/generator/config.rs`, 1 в `src/generator/core.rs`, 5 в `src/payload.rs`). `scripts/quality-gates.sh` расширен до 9 gates G1..G9 (cargo-deny, cargo-machete, public-api, N7 invariant, coverage ≥ 87% blocking, perf regression hint, changelog check). 374 теста (277 unit + 86 integration + 11 n7), все зелёные.
- **v10.7.15** — **Patch-release (PR-15+PR-16): CI Failure Mitigation T1-T8 + Coverage expansion +1.77%**. PR-15: 8 задач по снижению CI failure rate с 6-8% до target 2% — `.pre-commit-config.yaml` (T1), `scripts/check-toolchain.sh` + pre-push (T2), public-API strict gate через `cargo public-api` + `api-snapshot.txt` (T3), отдельный `.github/workflows/sbom.yml` с CycloneDX 1.5 SBOM (T4), examples validate (T5), concurrency + paths-ignore в CI/Docker workflows (T6), опциональный `.github/workflows/notify-telegram.yml` + `docs/TELEGRAM_SETUP.md` (T7), `.devcontainer/` (T8). PR-16: **25 новых тестов**, coverage **89.65% lines / 90.42% functions / 89.53% regions** (baseline 87.88% → +1.77%). `validate.rs` 87.39% → **94.53%** (+7.14%), `transport/mod.rs` 63% → 89.53% (+26%), `transport/tcp.rs` 46.72% → 84.50% (+37.78%). Patch-fix: `test_connection_pool_opens_multiple_connections` (rate 0→100 для coverage stability 3/3), `scripts/quality-gates.sh` dedup G8/G9/G10 (было два блока с номером G8). 399 тестов (302 unit + 86 integration + 11 n7). G8 perf regression 2.18 µs (в пределах ±10% от PR-10 baseline 2.01 µs). Quality Gates все ✅.
- **v10.7.15 (PR-18)** — **Security hotfix: Code injection в `notify-telegram.yml`** (CodeQL alert #7, severity: critical, CWE-94/95/116). `github.event.workflow_run.head_branch` (и другие user-controlled inputs) интерполировались напрямую в bash `run:` блок через `${{ }}`. Fix: перенос всех user inputs в `env:` блок + bash native `${VAR}` syntax. Закрыто через PR #18 `dev → main`. Также: Dependabot alert #1 (CVE-2025-53605 protobuf 2.28.0) dismissed с reason `not_used` — protobuf НЕ runtime зависимость (самописный encoder ~496 строк в `src/format/protobuf.rs`), только cargo-fuzz dev-toolchain.
- **v10.7.15 (Mandatory Git Flow)** — **Все мержи через PR.** Branch Protection Rules настроены: main (7 required checks + 1 review + linear + admin enforce), dev (7 required checks, no review). Auto-sync workflow `.github/workflows/sync-main-to-dev.yml` создаёт PR `main → dev` после каждого merge в main. PR template `.github/PULL_REQUEST_TEMPLATE.md` со стандартным checklist. Документация: `.github/branch-protection.md`, обновлённые `CONTRIBUTING.md` + `CLAUDE_HANDOFF.md` раздел 0.5. Покрытие проверками не снижено — все 7 blocking jobs требуются на каждом PR.
- **v10.7.17 (PR-Q: Solo-maintainer policy)** — **Review собственных PR через subagent.** Solo-maintainer не может approve собственный PR через gh CLI (`GraphQL: Review Can not approve your own pull request`). Решение для таких случаев: запустить `task` tool (general subagent) с детальным review checklist → если APPROVE → `gh pr merge --merge --admin --delete-branch=true`. Subagent оценивает diff, файлы, линтеры, тесты, CI статус, semantic correctness. Используется в PR #60 (v10.7.17 release) — subagent одобрил, merge --admin выполнен. Это deviation от strict "1 approval required" rule, но явно авторизовано пользователем для solo-maintainer workflow. В дальнейшем — **применять ту же политику** ко всем собственным PR.
- **v10.7.18** — **Patch-release: Phase 14 Step 1+2 + CI hardening.** dev = `ddd3b30`. PR #63 (Phase 14 Step 1, TLS mock + 5 tests), PR #66 (Phase 14 Step 2, 9 unit + 3 integration tests), PR #64 (notify-telegram jq syntax fix), PR #62+#48 (Dependabot maintenance). Coverage tls.rs 58.94% → 79.87% lines (+20.93pp cumulatively, Step 1+2 combined). 400 unit + 96 integration + 11 proptest tests pass. Sync main → dev через PR #59, #61, #65, #67.

**v10.7.18** — **Patch-release: Phase 14 Step 1+2 + CI hardening.** PR #63 (Phase 14 Step 1, TLS mock + 5 tests), PR #66 (Phase 14 Step 2, 9 unit + 3 integration), PR #64 (notify-telegram jq syntax fix). Coverage tls.rs 58.94% → 79.87% (+20.93pp cumulatively). Release commit `efbf9aca` через PR #67 → main → tag `v10.7.18` → GitHub Release.

**v10.7.19** — **Patch-release: Phase 14 Step 3 + release-pgo.yml infra fix.** PR #69 (Phase 14 Step 3, refactor extract validate_kafka_target_config + 8 unit-тестов), PR #77 (release-pgo.yml infra fix — stable rustc + LLVM 20 tarball download). PR-Q series #70-#77 (8 PR'ов — большинство closed как wrong assumption, #70 + #77 merged). Coverage cumulative v10.7.16 → v10.7.19: tls.rs 58.94% → 79.87% (+20.93pp), kafka.rs 51.68% → 77.80% (+26.12pp), TOTAL 91.10% → **94.03%** (+2.93pp). PGO build работает на tag push (LLVM 20 tarball download). Release commit через PR → main → tag `v10.7.19` → PGO build автоматически → GitHub Release published.

**v10.7.17** — **Patch-release: Phase 13 TCP reconnect race fix.** См. секцию «1. Что за проект»/release history. PR #58 (`9c55f55`) → release/v10.7.17 → PR #60 → main (`e28b461`) → tag v10.7.17 → GitHub Release. Coverage `transport/tcp.rs` 84.75% → 98.33% (+13.58pp), 5/5 `phase8a_*` tests теперь активны (без `#[ignore]` и без `tokio::time::sleep`), 391 tests pass. Sync main → dev через auto-sync PR #59.

---

## 8. Как продолжить в Claude

1. Распаковать актуальный архив `syslog-generator-vX.Y.Z-verified.zip` (в нём весь исходник без `target/`).
2. Прочитать `AUDIT.md` (полный план и статусы), `CHANGELOG.md`, этот файл.
3. **Веха D закрыта полностью (v8.6.0).** Следующая веха — **E «Зрелость» (P2)**. Рекомендуемая первая задача — F15 (дополнительные форматы: CEF/LEEF/JSON-lines) — см. AUDIT.md §4.1 P2.
4. Перед началом каждой задачи прогонять `cargo bench --no-run --locked` и `cargo bench --bench {name} -- --quick` (как минимум message_generation + sender_throughput) — регрессия v8.4.0 показала, что `cargo test` не покрывает бенчмарки.
4. Соблюдать процесс релиза из раздела 6 и правила общения из раздела 0.

---

## 9. Пример стартового промпта для Claude

Скопируй текст ниже в первое сообщение новой сессии Claude, приложив архив
`syslog-generator-v9.1.0-verified.zip` (или распакованный проект). При работе в Claude Code
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
   (перед cargo всегда: source "$HOME/.cargo/env"). Rust 1.95.0 (MSRV).
3. Если полный `cargo test` таймаутит — запускай тестовые бинарники отдельно
   (способ описан в разделе 2 CLAUDE_HANDOFF.md).
4. При выпуске версии ОБЯЗАТЕЛЬНО обновляй документацию И changelog:
   Cargo.toml (bump), CHANGELOG.md, README.md, AUDIT.md (пометь задачи «✅ Сделано (vX.Y.Z)»
   и статус вехи), CLAUDE_HANDOFF.md, examples/. Следуй чек-листу релиза из раздела 6.
5. Compile-verified релиз: код собирается, clippy чист, все тесты зелёные.

Текущее состояние: версия v10.4.0. **Веха F («Production-hardened») в процессе.** Сделанные релизы: вехи A/B/C/D (v8.0.0, v9.0.0) + вехи E (v9.1.0-v9.6.0) + **v10.0.0 + v10.1.0 + v10.2.0 + v10.3.0 + v10.3.1 + v10.4.0** (старт вехи F: breaking cleanup + Performance ч.1+ч.2 + CLI split + Coverage baseline + CI patch-fix + Coverage progress + cargo-fuzz). Следующие релизы вехи F: v10.4.1 (Coverage patch: ≥ 97% gate), v10.5.0 (CI расширение), v10.6.0 (usability ч.1), v10.7.0 (usability ч.2 + закрытие вехи F). План в `PLAN-v10.0.0.md` (8 релизов). История вехи E в `PLAN-веха-E.md`.
Все P1-задачи (F11/F12/F13/N4/N7/N9/D3/N2/N5/N8/N11) выполнены. Все P2-задачи вехи E (N10/F15/F16/F17/N4.cipher_policy/N12) выполнены. Сделанная часть вехи F: v10.0.0 (breaking cleanup), v10.1.0 (Performance ч.1 + B5), v10.2.0 (Performance ч.2: faker hot-path), v10.3.0 (Coverage baseline). Оставшиеся: v10.4.0-v10.7.0.

Задача на эту сессию: возьми задачу вехи E — F15 (дополнительные форматы для SIEM)
реализовать форматы CEF (ArcSight Common Event Format), LEEF (IBM QRadar)
и JSON-lines. Архитектурно — выделить trait `Format` в `src/format/`,
перенести туда существующие rfc5424/rfc3164/raw/protobuf; новые форматы
реализуют trait. Валидация (F13) расширяется — допустимые значения
`format` зависят от транспорта. Сначала предложи короткий план (структура
trait, как хранить format-specific данные — через Header/struct или через
JSON-config), дождись моего «ок», затем реализуй с тестами и выпусти
v8.7.0 по чек-листу. Перед стартом — прогнать cargo bench --no-run
и cargo bench --quick, чтобы убедиться что бенчмарки не сломаны (v8.4.0
показал что cargo test их не покрывает).

Начни с чтения CLAUDE_HANDOFF.md, AUDIT.md, и краткого плана по F15. Вопросы задавай
до начала правок, если что-то неоднозначно.
```

> Архив — `syslog-generator-v8.6.0-verified.zip` (текущая версия). Если хочешь начать не с F15, а с другой
> задачи (например, вехи E — F15 форматы или F17 аномалии) — замени абзаг «Задача на эту сессию».
