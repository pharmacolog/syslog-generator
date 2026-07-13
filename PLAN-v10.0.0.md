# PLAN-v10.0.0.md — Веха F (после v9.6.0)

> **Статус: начало вехи F.** Текущая версия: v9.6.0 (веха E «Зрелость» ЗАКРЫТА).
> Цель: довести проект до production-hardened уровня — оптимизация
> производительности, расширенный CI, покрытие ≥ 97%, юзабилити-полировка.
> Версионирование: **v10.0.0** → **v10.7.0** (8 релизов в вехе F).

Дата: 2026-07-13. Цель: v10.7.0.

---

## 0. Контекст перехода

- **Текущая версия**: v9.6.0 (веха E «Зрелость» ЗАКРЫТА).
- **Что перенесено в новый файл**: ничего (новая веха, чистый старт).
- **Что остаётся в `PLAN-веха-E.md`**: детальные планы v9.1.0–v9.6.0
  (стратегические решения D1-D3, раздел 3 — детальные планы релизов E,
  раздел 5 — roadmap после v9.6.0, который перенесён в §6 нового файла).
- **Преемственность**: контракт приёмки (раздел 4) сохранён без изменений
  из `PLAN-v10.0.0.md` → `PLAN-веха-E.md` (21 пункт).

---

## 1. Файловые операции (pre-step для v10.0.0)

- ✅ Переименовать `PLAN-v10.0.0.md` → `PLAN-веха-E.md` (`git mv`).
- ✅ Удалить `docs/docs-developer.md` и `docs/docs-user.md` (redirect-stub'ы,
  заменены на `docs/DEVELOPER_GUIDE.md` и `docs/USER_GUIDE.md`).
- ✅ Создать новый `PLAN-v10.0.0.md` с этим содержимым.

---

## 2. Зафиксированные стратегические решения

| # | Решение | Обоснование |
|---|---|---|
| **D1** | Coverage через `cargo-llvm-cov` (а не tarpaulin) | llvm-cov быстрее (~5-10×), нативный, стабильно работает с `cargo nextest`. Покрытие включает интеграционные тесты (`--workspace --all-targets`). |
| **D2** | Bench-regression gate: Criterion `--baseline` через c5h/bench-regression-action или cargo-benchcmp | Сравнение с последним green-запуском на main. Допуск ±10% (по контракту v9.6.0). Block только при деградации > 10%. |
| **D3** | Fuzzing через `cargo-fuzz` (AFL/libFuzzer backend) | Стандарт в Rust-экосистеме. Таргеты: `profile_parser`, `format_rfc5424`, `format_cef`, `format_leef`, `format_json_lines`. Запускается вручную / по расписанию (не в обычном CI). |
| **D4** | Completions через `clap_complete` (bash/zsh/fish/powershell) | Минимальные зависимости, zero-cost runtime, генерация на лету через subcommand `syslog-generator completions <shell>`. |
| **D5** | Logging через `tracing-subscriber` (а не `env_logger`) | Структурированные логи, совместимость с OpenTelemetry (roadmap §6), фильтрация по level/target. |
| **D6** | v10.0.0 = **major с breaking changes B1-B7** | API cleanup после закрытия вехи E: чистка deprecated re-exports, типизация ошибок, Rust naming convention. Миграция описана в CHANGELOG.md v10.0.0. |

---

## 3. Breaking changes v10.0.0 (B1–B7)

| # | Breaking change | Файлы | Сложность |
|---|---|---|---|
| **B1** | `TlsVersion::V1_2` → `TlsVersion::Tls12` (Rust naming convention) | `src/transport/tls.rs`, `src/validate.rs`, `src/transport/mod.rs` | низкая |
| **B2** | Удалить `pub use` deprecated re-exports из `lib.rs` (старые имена модулей `src/protobuf.rs`, `src/syslog.rs`, `src/payload.rs` — они перенесены в `src/format/`, `src/transport/`, `src/generator/` в v8.8.0). Оставить только новые пути. | `src/lib.rs` | низкая |
| **B3** | `MetricsError::AddrBind(String)` → `MetricsError::AddrBind { addr: String, source: std::io::Error }` (структурный вариант вместо `String`-based) | `src/error.rs`, `src/observability/server.rs`, `src/observability/mod.rs` | средняя |
| **B4** | `ValidationError` → структурный enum с полями `source: String`, `expected: Option<String>`, `actual: Option<String>` где применимо (69 мест использования; поэтапно, начиная с самых частых: `InvalidRegex`, `InvalidSchemaField`, `InvalidTlsMinProtocolVersion`, `InvalidCipherSuite`) | `src/validate.rs`, тесты | высокая |
| **B5** | CLI: `--target ADDR:TRANSPORT` → `--target ADDR` + `--transport TRANSPORT` (раздельные флаги). Старый формат ADDR:TRANSPORT остаётся как deprecated alias на v10.0.0, удаляется в v11.0.0. | `src/cli.rs`, `src/main.rs` | средняя |
| **B6** | `Cargo.toml`: удалить deprecated `[features.kafka.dependencies]` (нет такого сейчас — verify и почистить, если есть) | `Cargo.toml` | низкая |
| **B7** | `Format::name() -> &'static str` → удалить, заменить на `impl Display for FormatKind` (с `&'static str` через `write!`) | `src/format/mod.rs`, `src/format/{rfc5424,rfc3164,raw,protobuf,cef,leef,json_lines}.rs`, `src/generator/core.rs` | средняя |

**Migration guide для B1-B7** — в `CHANGELOG.md` секция v10.0.0.

---

## 4. Контракт (критерии приёмки) — копия без изменений

> **🚨 КРИТИЧНО: перед выпуском релиза в `main` обязательно дождаться
> зелёного CI run на ветке release/vX.Y.Z. Локальные проверки
> (`cargo fmt` / `clippy` / `build` / `test`) НЕ заменяют CI.
> Добавлено 2026-07-13 после инцидента с v10.2.0/v10.3.0, когда CI упал
> на `cargo fmt --all -- --check`, но релизы были выпущены.**

1. ✅ Все ранее зелёные тесты остаются зелёными (никаких регрессий)
2. ✅ Новые тесты добавлены для новой функциональности
3. ✅ **Backward-compat прогон**: `load_profile_from_path` на все `examples/*.json` + `examples/*.yaml` без изменений (NB: для v10.0.0 backward-compat прогон делается на `--target ADDR:TRANSPORT` legacy-формате; deprecated alias должен работать)
4. ✅ `cargo public-api` diff не показывает breaking changes (или breaking changes явно документированы в CHANGELOG с migration guide)
5. ✅ `cargo fmt --all -- --check` clean
6. ✅ `cargo clippy --all-targets -- -D warnings` clean (с `--features kafka` если применимо)
7. ✅ `cargo build --release` успех (для фичей с feature flags — `cargo build --release --features kafka`)
8. ✅ `cargo test --locked` все зелёные (включая `--features kafka` если есть)
9. ✅ `cargo bench --no-run --locked` успех
10. ✅ `cargo bench --quick` 9/9 Success
11. ✅ **Bench regression check**: throughput message_generation и sender_throughput не просел > 10% относительно предыдущего релиза
12. ✅ Live-проверка бинарника: `./target/release/syslog-generator --version` показывает корректную версию
13. ✅ Уборка: `target/` удалён, zip удалён
14. ✅ Gitflow: feature → dev → release → main → tag → push
15. ✅ **CI green перед merge в main**: на ветке `release/vX.Y.Z` все job'ы GitHub Actions
    (`.github/workflows/ci.yml`) должны быть зелёными. Используй `gh run watch <run-id>`
    или `gh pr checks <pr>` для ожидания. Только после зелёного CI — merge в main.
16. ✅ Документация: README, CHANGELOG, CLAUDE_HANDOFF, AUDIT обновлены
17. ✅ PLAN-v10.0.0.md обновлён (отметка ✅ для закрытых задач)
18. ✅ Schema: `schemas/profile.schema.json` синхронизирован с новыми полями
19. ✅ Архив в `.archived-releases/` сохранён (НЕ в git)
20. ✅ feature/release ветки НЕ удаляются (по требованию)
21. ✅ **Cargo.toml version bumped** в первом коммите feature-ветки

### Release-gate workflow (v10.4.0+)

```
feature/vX.Y.Z-name
   ↓ cargo fmt/clippy/test локально
   ↓ merge feature/vX.Y.Z-name → dev
release/vX.Y.Z (от dev)
   ↓ CI: gh pr checks (или push в release/vX.Y.Z + gh run watch)
   ↓ ждём зелёного CI (все jobs в ci.yml)
   ↓ только после зелёного CI:
   ↓ cargo build --release, smoke, zip, .archived-releases/
main (merge --no-ff + tag)
```

Команды для проверки CI:

```bash
# После push в release/vX.Y.Z:
gh run list --limit 5 --workflow=CI --branch=release/vX.Y.Z

# Дождаться зелёного CI:
gh run watch <run-id>

# Или через PR (если есть):
gh pr checks <pr-number> --watch
```

---

## 5. План релизов вехи F (v10.0.0 → v10.7.0)

| Релиз | Тип | Что | Зависит от |
|-------|-----|-----|------------|
| **v10.0.0** | major (breaking B1-B7) | Pre-step: PLAN-веха-E.md (rename) + новый PLAN-v10.0.0.md. Удалить `docs/docs-{developer,user}.md` (заменены на `docs/{DEVELOPER,USER}_GUIDE.md`). Breaking: B1-B7 (cleanup + типизация ошибок). USER_GUIDE.md до v10.0.0. README/AUDIT/CLAUDE_HANDOFF — статус вехи F → «в процессе», версия → 10.0.0. CHANGELOG — секция v10.0.0 с migration guide для B1-B7. | — |
| **v10.1.0** ✅ | minor | **Performance (часть 1)**: LTO + codegen-units=1 в Cargo.toml release profile. Bench-regression monitoring в CI (non-blocking, вывод как артефакт). **B5 (CLI `--target split`)**: deprecated alias `ADDR:TRANSPORT` (warning в stderr), новый формат `ADDR` + `--transport TRANSPORT`. **B3+B4 — N/A** (уже структурные с v8.x). | v10.0.0 |
| **v10.2.0** ✅ | minor | **Performance (часть 2)**: hot-path оптимизация faker-генераторов. Все `format!()` с многоэтапными аллокациями заменены на `String::with_capacity(N)` + `write!()` через `std::fmt::Write`. Затронуты `faker.ipv4`, `faker.ipv6`, `faker.mac`, `faker.hostname`, `faker.url`, `faker.uuid`, `random_string`. Bench: `generate_message_from_template` 6.96µs → **5.17µs** (-26%). Lock-free atomics и BytesMut pre-alloc — N/A (prometheus AtomicU64 + N6 уже оптимизировано). | v10.1.0 |
| **v10.3.0** ✅ | minor | **Coverage (часть 1)**: `cargo-llvm-cov` baseline **86.40% lines / 88.36% functions / 86.49% regions**. Non-blocking CI coverage job (ubuntu-latest, `taiki-e/install-action@v2`, артефакты `lcov.info` + `coverage-summary.txt`). `docs/COVERAGE.md` с baseline-таблицей и планом v10.4.0 (покрыть непокрытые модули). 317 тестов — все зелёные. | v10.2.0 |
| **v10.3.1** ✅ | patch | **🚨 CI fix**: `cargo fmt` для `src/payload.rs:97` (`write!(...).expect(...)` в одну строку). CI был сломан после v10.2.0 на `cargo fmt --all -- --check`. v10.3.1 восстанавливает зелёный CI. **Добавлен release-gate workflow**: CI green обязателен перед merge в main (см. §4 п.15 и § Release-gate workflow). Удалены локальные release-ветки `release/v10.{0,1,2,3}.0`. | v10.3.0 |
| **v10.4.0** | minor | **Coverage (часть 2)**: покрытие ≥ 97% — добавить тесты для непокрытых модулей (по отчёту v10.3.0). **Coverage gate** в CI (blocking: fail если < 97%). **Fuzzing**: `cargo-fuzz` — 5 таргетов (profile_parser, format_rfc5424, format_cef, format_leef, format_json_lines). Fuzz-корпус в `fuzz/corpus/`, инструкция в `docs/FUZZING.md`. | v10.3.0 |
| **v10.5.0** | minor | **CI расширение**: `cargo-deny` (security + license blocking), `cargo-machete` (unused deps blocking), MSRV-check (best-effort → blocking, requires `rust-toolchain.toml`). Dependabot `.github/dependabot.yml` (еженедельные PR для dependencies + actions). | v10.4.0 |
| **v10.6.0** | minor | **Usability (часть 1)**: `clap_complete` (bash/zsh/fish/powershell completions через subcommand `completions <shell>`). `clap_mangen` (man page через subcommand `man`). Цветной вывод ошибок (`owo-colors` + auto-detection `NO_COLOR` env). | v10.5.0 |
| **v10.7.0** | minor | **Usability (часть 2) + закрытие вехи F**: `tracing-subscriber` (замена `println!`/`eprintln!` на `tracing::{info,warn,error}!`). Прогресс-бар `indicatif` (только при `duration_secs > 30s` И TTY). `--dry-run` (печатает план, не отправляет). Двойной `Ctrl-C` = hard shutdown (counter). RUST_LOG поддержка. `--config` (auto-detect JSON/YAML по расширению, алиас `--profile`). Документация: `docs/PERFORMANCE.md`, `docs/COVERAGE.md`, `docs/FUZZING.md`, обновлённые `USER_GUIDE.md`/`DEVELOPER_GUIDE.md`. **Закрытие вехи F.** | v10.6.0 |

---

## 6. Roadmap после вехи F (после v10.7.0)

Кандидаты на следующие вехи:

- **Hot-reload профиля без остановки генератора** (через SIGHUP или watch on file).
- **Distributed mode** (multi-node coordinator через gRPC или Raft).
- **gRPC/syslog-over-HTTP2 транспорты** (для service-mesh окружений).
- **OpenTelemetry exporter** (помимо Prometheus; OTLP через tonic).
- **Плагинная система** для кастомных форматов/транспортов (через WASM или dynamic loading).
- **Web UI** для real-time мониторинга (Grafana всё ещё рекомендуется, но встроенная панель полезна для отладки).

---

## 7. Полная инвентаризация вехи F

| Категория | Задача | Где | Релиз |
|---|---|---|---|
| **Breaking** | B1: TlsVersion rename | `src/transport/tls.rs` | v10.0.0 |
| **Breaking** | B2: lib.rs cleanup | `src/lib.rs` | v10.0.0 |
| **Breaking** | ~~B3: MetricsError::AddrBind~~ | `src/error.rs` | **N/A** (уже структурный) |
| **Breaking** | ~~B4: ValidationError структурный~~ | `src/validate.rs` (69 мест) | **N/A** (уже структурный) |
| **Breaking** | B5: CLI --target split | `src/cli.rs` | **v10.1.0** ✅ (deprecated alias) |
| **Breaking** | B6: Cargo.toml cleanup | `Cargo.toml` | v10.0.0 |
| **Breaking** | B7: Format::name → Display | `src/format/*` | v10.0.0 |
| **Performance** | LTO + codegen-units=1 | `Cargo.toml` | v10.1.0 |
| **Performance** | Bench-regression gate | `.github/workflows/ci.yml` | v10.1.0 |
| **Performance** | ~~Lock-free atomic counters~~ | `src/observability/metrics.rs` | **N/A** (prometheus crate уже использует AtomicU64) |
| **Performance** | ~~BytesMut pre-alloc verify~~ | `src/transport/tcp.rs`, `src/transport/tls.rs` | **N/A** (N6, v8.7.0) |
| **Performance** | faker hot-path оптимизация | `src/payload.rs` | **v10.2.0** ✅ (-26% на generate_message_from_template) |
| **Coverage** | cargo-llvm-cov baseline | `.github/workflows/ci.yml` | v10.3.0 |
| **Coverage** | Coverage report analysis | (process, no code) | v10.3.0 |
| **Coverage** | Tests for uncovered modules | `src/**/tests.rs`, `tests/*.rs` | v10.4.0 |
| **Coverage** | Coverage gate ≥ 97% (blocking) | `.github/workflows/ci.yml` | v10.4.0 |
| **Coverage** | cargo-fuzz setup | `fuzz/` | v10.4.0 |
| **Coverage** | 5 fuzz targets | `fuzz/fuzz_targets/*.rs` | v10.4.0 |
| **Coverage** | Fuzzing docs | `docs/FUZZING.md` | v10.4.0 |
| **CI** | cargo-deny (security + license) | `.github/workflows/ci.yml` | v10.5.0 |
| **CI** | cargo-machete (unused deps) | `.github/workflows/ci.yml` | v10.5.0 |
| **CI** | MSRV-check (blocking) | `.github/workflows/ci.yml` | v10.5.0 |
| **CI** | Dependabot | `.github/dependabot.yml` | v10.5.0 |
| **Usability** | clap_complete (bash/zsh/fish/powershell) | `src/cli.rs` | v10.6.0 |
| **Usability** | clap_mangen (man page) | `src/cli.rs` | v10.6.0 |
| **Usability** | owo-colors (colored errors) | `src/error.rs` | v10.6.0 |
| **Usability** | tracing-subscriber | `src/main.rs`, `src/**/*.rs` | v10.7.0 |
| **Usability** | indicatif progress bar | `src/generator/core.rs` | v10.7.0 |
| **Usability** | --dry-run | `src/cli.rs`, `src/generator/core.rs` | v10.7.0 |
| **Usability** | Double Ctrl-C = hard shutdown | `src/shutdown.rs` | v10.7.0 |
| **Usability** | RUST_LOG support | `src/main.rs` | v10.7.0 |
| **Usability** | --config (auto-detect JSON/YAML) | `src/cli.rs` | v10.7.0 |
| **Docs** | USER_GUIDE.md update | `docs/USER_GUIDE.md` | v10.0.0 |
| **Docs** | PERFORMANCE.md | `docs/PERFORMANCE.md` | v10.7.0 |
| **Docs** | COVERAGE.md | `docs/COVERAGE.md` | v10.7.0 |
| **Docs** | DEVELOPER_GUIDE cleanup | `docs/DEVELOPER_GUIDE.md` | v10.0.0 |
| **Plan** | PLAN-веха-E.md (rename) | `PLAN-веха-E.md` | v10.0.0 |
| **Plan** | PLAN-v10.0.0.md (new) | `PLAN-v10.0.0.md` | v10.0.0 |

---

## 8. Открытые вопросы / TODO

- [x] ~~B4 (ValidationError структурный)~~ — N/A: уже структурный с v8.x.
- [x] ~~B3 (MetricsError::AddrBind)~~ — N/A: уже структурный с v8.x.
- [ ] D2 (bench-regression gate) — выбрать конкретный инструмент: `cargo-benchcmp`, `c5h/bench-regression-action`, или собственный скрипт
- [ ] Покрытие ≥ 97% — реальный baseline (сейчас ~X%, нужно измерить)
- [ ] `--target ADDR:TRANSPORT` deprecated alias — нужен period deprecation (v10.0.0 = alias, v11.0.0 = removal)