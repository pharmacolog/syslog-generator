# PLAN: v9.0.0 — Закрытие вехи D «Продакшн-готовность» (все P0 + P1)

> Статус: **откат** с v9.0.0 (был выпущен преждевременно). Все P0 и P1
> задачи AUDIT.md §4 должны быть реализованы и протестированы перед major
> релизом. P2 (веха E) — отдельный этап, начнётся ПОСЛЕ v9.0.0.

Дата: 2026-07-13. Автор: Anton E. Gerasimov.

---

## 1. Полная инвентаризация P0 + P1 (по AUDIT.md §4)

### §4.1 Функциональные характеристики

| # | Задача | Заявлено | Реально | Что нужно |
|---|--------|----------|---------|-----------|
| F1 | rate-limiting + total_messages + duration, убрать min(100) | ✅ v7.5.0 | ✅ | — |
| F2 | connections = пул воркеров | ✅ v7.6.0 | ✅ | — |
| F3 | load_shape | ✅ v7.8.0 | ✅ | — |
| F4 | RNG с seed | ✅ v7.9.0 | ✅ | — |
| F5 | faker/regex/distributions | ✅ v8.0.0 | ✅ | — |
| F6 | корреляции (depends_on) | ✅ v8.0.0 | ✅ | — |
| F7 | RFC 5424 | **❌ галочка** | ✅ реализован | **AUDIT.md: поставить ✅** |
| F8 | RFC 3164 (BSD) | **❌ галочка** | ✅ реализован | **AUDIT.md: поставить ✅** |
| F9 | Framing RFC 6587/5425 | **❌ галочка** | ✅ реализован | **AUDIT.md: поставить ✅** |
| F10 | Protobuf wire-format | ✅ v8.0.0 | ✅ | — |
| F11 | CLI | ✅ v8.1.0 | ✅ | — |
| F12 | HTTP /metrics | ✅ v8.2.0 | ✅ | — |
| F13 | Валидация профиля | ✅ v8.1.0 | ✅ | — |
| F14 | Multi-template | ✅ v7.9.0 | ✅ | — |

### §4.2 Нефункциональные характеристики

| # | Задача | Заявлено | Реально | Что нужно |
|---|--------|----------|---------|-----------|
| N1 | cpu/memory метрики | ✅ v8.6.0 (удалены) | ✅ | — |
| N2 | Дашборд синхронизация | ✅ v8.6.0 | ✅ | — |
| N3 | Метрики нагрузки | ✅ v8.0.0 | ✅ | — |
| N4 | Безопасный TLS по умолчанию | ✅ v8.2.0 | ✅ основное | ⚠️ **mTLS + cipher policy — отложено, нужно сделать** |
| N5 | Бенч-гейт + предкомпиляция | ✅ v8.6.1 | ✅ | — |
| N6 | Zero-copy/буферизация | ❌ | ❌ | **🔴 СДЕЛАТЬ — критично для производительности** |
| N7 | Типизированные ошибки | ✅ v8.3.0 | ✅ | — |
| N8 | Расширение тестов | ⚠️ частично | ⚠️ | **🔴 ДОДЕЛАТЬ: proptest + back-pressure** |
| N9 | CI-пайплайн | ✅ v8.4.0 | ✅ | — |
| N10 | Слои (generator/transport/scheduler/format/observability) | ❌ | ❌ | **🔴 СДЕЛАТЬ — большая архитектурная правка** |
| N11 | Документация как контракт | ✅ v8.6.1 | ✅ | — |
| N12 | Docker/musl/docker-compose | ❌ (P2) | ❌ | Переносится в веху E |

### "Отложено" (внутри F13 и N4)

| Что | Заявлено | Реально |
|-----|----------|---------|
| F13: JSON Schema + YAML | отложено | ✅ в v8.5.0 (D3) — **обновить примечание** |
| N4: mTLS + cipher policy | отложено | ❌ — **сделать в v8.7.2** |

### Итог: что реально нужно сделать перед v9.0.0

1. **N6** — Zero-copy/буферизация (file + net)
2. **N8** — proptest + back-pressure тест
3. **N4.mTLS + cipher policy** — клиентский сертификат + min-TLS-version
4. **N10** — Рефакторинг слоёв (большое изменение, выделение `src/format/`, `src/transport/`, `src/observability/`)
5. **AUDIT.md** — поставить ✅ на F7/F8/F9 + убрать "Отложено" из F13
6. **CLAUDE_HANDOFF.md** — обновить историю
7. **CHANGELOG.md** — секция v9.0.0

---

## 2. План релизов (последовательность patch'ей → major)

| Релиз | Тип | Что | Зависит от |
|-------|-----|-----|------------|
| **v8.7.0** | patch | **N6**: zero-copy/буферизация (BufWriter для file, BytesMut для net, батчированная запись) | — |
| **v8.7.1** | patch | **N8**: proptest (property-based для payload генераторов) + отдельный тест на back-pressure (mpsc overflow) | — |
| **v8.7.2** | patch | **N4.mTLS**: клиентский сертификат (tls_client_cert_file + tls_client_key_file) + min_tls_version (1.2/1.3) + cipher policy (allow/denylist) | — |
| **v8.8.0** | minor | **N10**: рефакторинг слоёв — выделение `src/format/` (RFC 5424/3164/raw/protobuf → trait Format), `src/transport/` (sender'ы), `src/observability/` (metrics + metrics_server) | — |
| **v8.8.1** | patch | **AUDIT.md/CLAUDE_HANDOFF.md**: поставить ✅ на F7/F8/F9, убрать "Отложено" из F13 | требует v8.7.x для корректных отсылок |
| **v9.0.0** | major | Milestone release: веха D ЗАКРЫТА полностью. Публичный API может сломаться из-за N10 (новые модули), но семантика backward-compatible. Переход к вехе E. | требует v8.7.x + v8.8.x |

**Все патчи v8.7.x и v8.8.x — backward-compatible** (только новые фичи, никаких breaking changes). v9.0.0 может содержать breaking changes от рефакторинга слоёв.

---

## 3. Детальный план каждого релиза

### 3.1 v8.7.0 — N6 (zero-copy/буферизация)

**Цель:** Устранить лишние копирования и аллокации в горячем пути.

**Изменения:**
- `src/sender.rs::target_sender_file` — заменить `Vec<u8>` + `write_all` на `BufWriter<File>` для уменьшения системных вызовов.
- `src/sender.rs::target_sender_tcp/tls` — перейти на `BytesMut` для accumulating buffer, `write_all` → `write_all` через `BytesMut::freeze()`.
- `src/sender.rs::target_sender_udp` — `send_to` уже принимает `&[u8]`, проверить что нет лишних `.to_vec()`.

**Тесты:**
- Unit: `sender::tests::buf_writer_accumulates_small_writes` — серия из N мелких сообщений даёт 1 системный вызов write.
- Unit: `sender::tests::bytes_mut_no_copy_on_send` — `BytesMut` шарится между write и ack.
- Benchmark: добавить `benches/sender_allocation.rs` — измеряет аллокации на сообщение через dhat или rough counters.

**Критерии приёмки:**
- 0 регрессий в существующих 181 тестах
- 3+ новых теста на zero-copy поведение
- cargo bench показывает меньше аллокаций или хотя бы не больше
- clippy + fmt чисто

### 3.2 v8.7.1 — N8 (proptest + back-pressure)

**Цель:** Property-based тесты для payload генераторов + явный тест на back-pressure.

**Изменения:**
- `Cargo.toml`: + `proptest = "1"` в dev-dependencies.
- `src/payload.rs`: добавить `#[cfg(test)] mod proptests` с property-based тестами:
  - `prop_int_in_range` — сгенерированный int всегда в [min, max].
  - `prop_enum_in_values` — сгенерированный enum всегда из `values`.
  - `prop_faker_ipv4_format` — IPv4 всегда валидный формат (4 октета, 0..=255).
  - `prop_seed_determinism` — тот же seed даёт ту же последовательность.
- `tests/integration_tests.rs`: добавить `test_backpressure_mpsc_overflow`:
  - target с `connections=1`, генерация быстрее чем TCP может отправить → mpsc(1024) заполнится → проверить что продюсер блокируется, drain в конце освобождает канал.

**Критерии приёмки:**
- 5+ property-based тестов
- 1 явный тест на back-pressure
- 0 регрессий
- clippy + fmt чисто

### 3.3 v8.7.2 — N4.mTLS + cipher policy

**Цель:** Поддержка клиентских сертификатов и настройка TLS.

**Изменения:**
- `src/config.rs::TargetConfig`:
  - + `tls_client_cert_file: Option<String>` — путь к клиентскому PEM-сертификату
  - + `tls_client_key_file: Option<String>` — путь к клиентскому PEM-ключу
  - + `tls_min_protocol_version: Option<String>` — "1.2" или "1.3"
- `src/sender.rs::TlsParams`:
  - + поля `client_cert: Option<Vec<u8>>`, `client_key: Option<Vec<u8>>`
  - + поле `min_protocol: Option<TlsVersion>`
- `src/sender.rs::build_tls_connector`: загрузить identity если client_cert+key заданы, установить min_protocol.
- `src/validate.rs`: новые `ValidationError` для не-валидных путей к сертификатам.

**Тесты:**
- `tls_mtls_handshake_success` — поднимаем TLS-сервер с rcgen, клиент с client_cert+key → handshake успешен.
- `tls_mtls_handshake_rejected_without_cert` — клиент без сертификата отвергается сервером с verify_client.
- `tls_min_protocol_1_3_rejects_1_2` — клиент с min=1.3 не подключается к серверу с 1.2.

**Критерии приёмки:**
- 3+ новых теста на mTLS и cipher policy
- Валидация на отсутствующие файлы клиентских сертификатов
- 0 регрессий
- clippy + fmt чисто

### 3.4 v8.8.0 — N10 (рефакторинг слоёв)

**Цель:** Чёткая архитектура с явными слоями вместо плоского списка модулей.

**Изменения:**
- `src/format/` (новый модуль):
  - `mod.rs` с `pub trait Format { fn render(&self, h: &Header, msg: &[u8]) -> Vec<u8>; }`
  - `rfc5424.rs`, `rfc3164.rs`, `raw.rs`, `protobuf.rs` — каждый реализует trait
  - Реэкспорт через `syslog::build_rfc5424/build_rfc3164` для backward-compat
- `src/transport/` (новый модуль):
  - `mod.rs` с `pub trait Transport` (open, write, close)
  - `file.rs`, `tcp.rs`, `udp.rs`, `tls.rs` — каждый реализует trait
  - Текущие `target_sender_*` функции переезжают сюда
- `src/observability/` (новый модуль):
  - `mod.rs` реэкспортирует `metrics::*` и `metrics_server::*`
  - Или просто переименование через `pub use metrics::*;` в `src/observability/mod.rs`
- `src/generator/` (новый модуль):
  - `mod.rs` реэкспортирует `core::*` (или перенос `run_profile`, `run_phase_multi`, `generate_message`)
- `src/lib.rs`: обновить `pub use` и `pub mod` для новой структуры.
- Backward-compat: оставить `pub use crate::core::*` (старые пути).
- `src/architecture-notes.md` — заменить заглушку реальным описанием архитектуры.

**Тесты:**
- Существующие тесты должны работать без изменений (backward-compat через реэкспорты).
- 1-2 новых теста на trait Format (например, динамический dispatch).

**Критерии приёмки:**
- 0 регрессий в 181+ тестах
- Новые модули не ломают публичный API (всё реэкспортируется)
- `architecture-notes.md` обновлён
- clippy + fmt чисто

### 3.5 v8.8.1 — AUDIT.md синхронизация

**Цель:** Исправить забытые галочки и устаревшие "Отложено" примечания.

**Изменения:**
- `AUDIT.md`:
  - F7: добавить ✅ (был реализован в v7.7.0)
  - F8: добавить ✅
  - F9: добавить ✅
  - F13: убрать "Отложено: JSON Schema и YAML" (сделано в v8.5.0/D3)
  - N4: убрать "Отложено: mTLS, cipher policy" (сделано в v8.7.2)
  - N5: добавить ✅ (сделано в v8.6.1)
  - N6: добавить ✅ (сделано в v8.7.0)
  - N8: добавить ✅ (расширено в v8.7.1)
  - N10: добавить ✅ (сделано в v8.8.0)
- `CLAUDE_HANDOFF.md`: обновить историю v8.7.0 → v8.8.1.

### 3.6 v9.0.0 — major milestone

**Цель:** Семантический маркер "Все P0+P1 закрыты, веха D ЗАКРЫТА, переход к вехе E".

**Изменения:**
- `Cargo.toml`: 8.8.1 → 9.0.0
- Все файлы: обновить ссылки на "v8.x" → "v9.0" где релевантно.
- `CHANGELOG.md`: большая секция v9.0.0 с полным списком закрытых задач и обоснованием major bump.
- `CLAUDE_HANDOFF.md`: roadmap переключён на веху E.

**Это NOT breaking change** (как и прошлый раз): публичный API полностью backward-compatible. Major bump = milestone marker, как обсуждалось.

---

## 4. Критерии приёмки (для каждого релиза и финально)

Каждый релиз (v8.7.0, v8.7.1, v8.7.2, v8.8.0, v8.8.1) должен:

1. ✅ **Все ранее зелёные тесты остаются зелёными** (никаких регрессий):
   - 181 теста на момент v8.6.1 (115 unit + 55 integration + 11 N7) — и растёт с каждым релизом.
2. ✅ **Новые тесты добавлены** для каждой новой функциональности.
3. ✅ `cargo fmt --all -- --check` — clean.
4. ✅ `cargo clippy --all-targets -- -D warnings` — clean.
5. ✅ `cargo build --release` — успех.
6. ✅ `cargo test --locked` — все зелёные.
7. ✅ `cargo bench --no-run --locked` — успех.
8. ✅ `cargo bench --bench sender_throughput -- --quick` — все 6 кейсов Success.
9. ✅ `cargo bench --bench message_generation -- --quick` — все 3 кейса Success.
10. ✅ Live-проверка бинарника: `./target/release/syslog-generator --version`,
    `--validate --schema-strict --profile examples/*.json|yaml|yml` — rc=0.
11. ✅ Уборка: `target/` удалён после `cargo clean`, zip-архив удалён после сборки,
    локальные feature/release ветки удалены.
12. ✅ Gitflow: feature → dev → release → main → tag → push.
13. ✅ Документация (CHANGELOG.md, README.md, AUDIT.md, CLAUDE_HANDOFF.md)
    обновлена с подробным подробным сообщением коммита.

Финально для v9.0.0 дополнительно:
- ✅ Все задачи AUDIT.md §4.1 (P0+P1) и §4.2 (P0+P1) либо имеют ✅ в коде,
  либо явно отмечены как "Перенесено в веху E" с обоснованием.
- ✅ Все "Отложено" примечания из F13 и N4 удалены (соответствующие задачи сделаны).
- ✅ Реальное состояние кода = то, что написано в AUDIT.md (нет расхождений).

---

## 5. Стратегия выполнения

Сейчас я в build mode. Начинаю с **v8.7.0** (N6 — zero-copy).

После каждого релиза:
1. Проверяю критерии приёмки
2. Создаю release/ветку, бамплю версию
3. Делаю release-коммит (только метаданные: CHANGELOG, README, AUDIT, CLAUDE_HANDOFF)
4. Мержу в main, тегаю, пушу
5. Архив + уборка
6. Возвращаюсь в dev, начинаю следующий релиз

Перед началом **каждого** релиза я:
- Прогоняю cargo test --locked + cargo bench --quick (per CLAUDE_HANDOFF §0.4)
- Проверяю, что предыдущая работа не сломана

---

## 6. Roadmap: после v9.0.0

P2 (веха E) — отдельный этап:
- F15: CEF / LEEF / JSON-lines (после N10, когда есть trait Format)
- F16: Kafka/Redpanda + файловая ротация (после N10, когда есть trait Transport)
- F17: сценарии атак/аномалий
- N12: Docker/musl/docker-compose (отдельный этап)

Эти задачи начнутся после v9.0.0 как v9.1.0, v9.2.0 и т.д.
