# MIGRATION GUIDE

> **Статус:** Unreleased / v11.6 (Issue #134). Документ описывает breaking
> changes и шаги миграции. Текущий released tag — v10.7.19; breaking changes
> секции §6 ещё не выпущены.

## 1. v10.0.0 — Breaking cleanup (B1–B7)

### B1: `TlsVersion::V1_2` → `TlsVersion::Tls12` (Rust naming)

```rust
// До v10.0.0:
let v = TlsVersion::V1_2;

// С v10.0.0:
let v = TlsVersion::Tls12;
```

Также `TlsVersion::V1_3` → `TlsVersion::Tls13`.

### B2: Удалены deprecated `pub use` из `src/protobuf.rs`

```rust
// До v10.0.0:
use syslog_generator::protobuf::{apply_protobuf_schema, serialize_protobuf};
//                          ^^^^^^^^^^ deprecated re-export

// С v10.0.0 (используйте прямой путь):
use syslog_generator::protobuf::{apply_protobuf_schema, serialize_protobuf};
//                          ^^^^^^^^^^ теперь thin re-export на format::protobuf
```

API остался тот же, изменилась только реализация (canonical source в `format::protobuf`).

### B3: `MetricsError::AddrBind(String)` → структурный вариант

```rust
// До v10.0.0:
Err(MetricsError::AddrBind("addr parse error".to_string()))

// С v10.0.0:
Err(MetricsError::AddrBind { addr: "0.0.0.0:9090".to_string(), source: io_error })
```

**Примечание:** в реальности `B3` оказался **N/A** — `MetricsError` уже структурный с v8.x.

### B4: `ValidationError` — структурный enum

```rust
// До v10.0.0:
Err(ValidationError::InvalidRegex { source: "...".to_string() })

// С v10.0.0:
Err(ValidationError::InvalidRegex { source: String, expected: Option<String>, actual: Option<String> })
```

**Примечание:** `B4` оказался **N/A** — `ValidationError` уже структурный с v8.x.

### B5: CLI `--target` split (с deprecated alias)

```bash
# До v10.1.0 (deprecated alias, warning в stderr):
syslog-generator --target 127.0.0.1:514:udp

# С v10.1.0 (новый формат):
syslog-generator --target 127.0.0.1:514 --transport udp

# Deprecated alias удалится в v11.0.0.
```

### B6: `Cargo.toml` cleanup

Удалены deprecated зависимости (`rcgen`). Переезд на `openssl req` в тестах.
**0 breaking** для пользователей.

### B7: `Format::name()` → `Display`

```rust
// До v10.0.0:
let name: &'static str = fmt.name();

// С v10.0.0:
let name = fmt.to_string(); // Display impl
```

## 2. v9.5.0 — N4.cipher_policy + rustls миграция (BREAKING)

`native-tls` → `rustls 0.23` — **breaking change** для downstream пользователей,
использовавших `native_tls::Protocol` напрямую.

```rust
// До v9.5.0:
use syslog_generator::{TlsVersion, TlsParams};
let v = parse_tls_min_version("1.2")?; // возвращал native_tls::Protocol

// С v9.5.0:
use syslog_generator::{TlsVersion, TlsParams};
let v = parse_tls_min_version("1.2")?; // возвращает TlsVersion::Tls12 (enum)
```

Если вы использовали `native-tls` API напрямую в своём коде:
- Замените `native_tls::Protocol::Tlsv12` → `TlsVersion::Tls12`.
- Замените `native_tls::Protocol::Tlsv13` → `TlsVersion::Tls13`.

## 3. v8.8.0 — N10 рефакторинг слоёв

**0 breaking changes** для публичного API. Старые имена модулей
(`syslog_generator::core::*`, `::config::*`, `::sender::*`, `::syslog::*`,
`::metrics::*`, `::metrics_server::*`, `::protobuf::*`) сохранены как thin
re-export обёртки. Код, импортирующий через старые пути, продолжает работать.

## 4. v10.7.4 — v10.7.19 — patch-релизы (текущая версия)

**0 breaking changes** от v10.7.3 до v10.7.19 (серия patch-релизов по результатам аудита v10.7.2 + CI улучшения + Coverage expansion + Phase 13 TCP race fix + Phase 14 Step 1/2/3 coverage + release-pgo.yml infra fix + Dependabot maintenance + notify-telegram graceful degradation). Текущая версия — **v10.7.19**.

### 4.2 v10.7.19 (Phase 14 Step 3): Kafka coverage + release-pgo.yml infra fix

**CI: release-pgo.yml infrastructure fix (PR #77).** Final fix PR-Q series
(#70-#77): `dtolnay/rust-toolchain@stable` + download LLVM 20 tarball
(~1.9 GB) с GitHub release. PGO build теперь работает на tag push —
artifact `syslog-generator-pgo-v10.7.19` uploaded автоматически.

PR-Q series history (8 PR'ов):
- #70 (PATH fallback) ✅
- #71-#76 (6 PR'ов) ❌ closed (wrong assumptions)
- #77 (THIS) ✅ — stable rustc + LLVM 20 tarball (правильный URL)

Phase 14 Step 1+2+3: см. v10.7.18 (Tier 2 coverage на tls.rs + kafka.rs).

### 4.3 v10.7.18 (Phase 14): TLS Tier 2 coverage + CI hardening

**Phase 14 Step 1 (PR #63)**: TLS mock infrastructure + 5 integration тестов.
**Phase 14 Step 2 (PR #66)**: 9 unit-тестов + 3 integration-теста.
Coverage `transport/tls.rs`: 58.94% (v10.7.16) → **79.87% lines** (+20.93pp).
Coverage TOTAL: 91.10% → **93.86%** lines.

PR #64: notify-telegram graceful degradation (двойной 'else' → jq syntax error → Telegram 400).

### 4.4 v10.7.17 (Phase 13): TCP reconnect race fix

**Что:** Устранена давняя CI-flake в `phase8a_tcp_*` тестах (`src/transport/tcp.rs`):
3 теста теперь активны (`#[tokio::test(flavor = multi_thread, worker_threads = 2)]`),
вместо `#[ignore]`. Coverage `transport/tcp.rs` восстановлен 84.75% → 98.33%
(+13.58pp).

**Код-пользователи:** без изменений. Production код (`target_sender_tcp`)
не затронут, только test infrastructure.

PR-2 добавил:
- SIGTERM handler (раньше был только SIGINT).
- TLS close_notify перед exit (N12).
- JoinHandle tracking для HTTP server (M7).
- Feature `test-helpers` для `ensure_rustls_provider_for_tests` (N14).

API полностью backward-compatible.

## 5. Будущие breaking changes

### v11.0.0 (major, TBD)

- **Удаление deprecated alias** `--target ADDR:TRANSPORT` (B5).
- **Удаление orphan `pub use`** в `src/lib.rs` (~30 re-exports с 0 external use).
  Будет отдельный PR с deprecation warnings в stderr (минимум за 1 минор до
  удаления).
- **Полный deprecation цикл**: warning в v10.x → removal в v11.0.0.

### v12.0.0 (TBD)

- **`rand 0.10` миграция** (PR-7) — может сломать код, использующий
  `rand 0.9` API в custom `StdRng::from_entropy()`.
- **`rustls 0.23 → 0.27+`** (PR-8) — breaking в rustls API, может сломать
  custom `ClientConfig` extensions.

## 6. `serde_yaml` → `serde_yaml_ng` (Issue #134, breaking в public API)

Upstream `serde_yaml 0.9` (dtolnay) архивирован 2024-03. Мигрируем на активный
drop-in форк `serde_yaml_ng 0.10` (acatton) — тот же `unsafe-libyaml` FFI,
совместимый serde-derive API, та же internal `Value`/`Mapping`/`Sequence`
структура. MSRV крейта `1.64`, MSRV проекта `1.95` — совместимо.

### Что меняется в public API

Меняется конкретный тип одного-единственного публичного поля — `source`
варианта `ConfigError::Yaml`. Это `#[source]` источник через `thiserror`,
поэтому публично наблюдаемое поведение: `source().downcast_ref::<Error>()`
раньше давало `serde_yaml::Error`, теперь даёт `serde_yaml_ng::Error`. Семантика
`Display` (`{source}` форматирует то же «line N, column M: ...») и сигнатура
метода `std::error::Error::source` остаются — это **breaking** только если
downstream явно аннотирует тип `serde_yaml::Error` или делает `downcast`.

### Шаги миграции для downstream

**1. `Cargo.toml` вашего проекта:**

```toml
[dependencies]
# До:
serde_yaml = "0.9"

# После:
serde_yaml_ng = "0.10"
```

**2. Зависимости (прямые `use`/`serde_yaml::` в вашем коде):**

```rust
// До:
use serde_yaml::{Error as YamlError, Value as YamlValue};
fn parse(s: &str) -> Result<YamlValue, YamlError> { serde_yaml::from_str(s) }

// После:
use serde_yaml_ng::{Error as YamlError, Value as YamlValue};
fn parse(s: &str) -> Result<YamlValue, YamlError> { serde_yaml_ng::from_str(s) }
```

Механический sed-replace `serde_yaml::` → `serde_yaml_ng::` покрывает
большинство случаев. Методы API (`from_str`, `from_reader`, `to_string`,
`to_writer`, `Value`, `Mapping`, `Sequence`, `Number`, `Error`) — те же.

**3. Сторона `syslog-generator`:**

```rust
// До: явный downcast или аннотация типа
if let ConfigError::Yaml { source, .. } = &err {
    let yaml_err: &serde_yaml::Error = source; // ← компилировалось на serde_yaml 0.9
    println!("{yaml_err}");
}

// После:
if let ConfigError::Yaml { source, .. } = &err {
    let yaml_err: &serde_yaml_ng::Error = source; // ← нужно обновить аннотацию
    println!("{yaml_err}");
}
```

Опции патчинга:
- **Механическая замена** (предпочтительно): `sed -i 's/serde_yaml::/serde_yaml_ng::/g' $(rg -l 'serde_yaml::' .)`
- **Re-export-обёртка** (если нужен fallback): объявите в своём крейте
  `pub use serde_yaml_ng as serde_yaml;` — API идентичен.

### Что НЕ меняется

- Поведение `load_profile_from_yaml_str` / `load_profile_from_path` (YAML→`Profile`).
- Парсинг YAML-профилей (одинаковая семантика: serde-derive, тот же `Value`).
- CLI, бинарь, схемы JSON Schema, fuzz harness — без изменений контракта.
- Hot-path (генерация syslog-сообщений) — YAML не используется в hot-path,
  см. §7 про честный статус bench rerun.

### Альтернативные кандидаты (не выбраны)

`serde_norway 0.9.42` (cafkafk, MSRV 1.71.1) и `yaml_serde 0.10.4`
(The YAML Organization, MSRV 1.82, libyaml-rs без unsafe C) — оба drop-in.
Не выбраны из-за предпочтения maintainer'а (issue #134) к `serde_yaml_ng`
как ближайшему форку dtolnay.

`serde_yml` (RUSTSEC-2025-0068) и `serde-saphyr` (typed-only, не DOM) —
**не** являются drop-in и не мигрируем на них.

## 7. Performance impact (Issue #134, измеренный)

**A/B microbenchmark YAML-парсинга** (`/tmp/yaml-ab-bench`, criterion 0.8,
100 samples, тот же `Profile` struct, тот же YAML, тот же rustc 1.95.0,
та же машина — `serde_yaml 0.9` НЕ добавлен в project deps):

```
yaml_parse/serde_yaml_0.9     time:   [11.640 µs 11.668 µs 11.699 µs]
                              thrpt:  [56.982 MiB/s 57.133 MiB/s 57.270 MiB/s]

yaml_parse/serde_yaml_ng_0.10 time:   [11.597 µs 11.618 µs 11.640 µs]
                              thrpt:  [57.271 MiB/s 57.380 MiB/s 57.483 MiB/s]
```

Δ ≈ −0.43% (≈ 50 нс) на `serde_yaml_ng` — в пределах noise одного
criterion run. Регрессии на пути YAML→`Profile` не зафиксировано.

**Hot-path bench на `serde_yaml_ng`** (`cargo bench --bench hot_path -- --quick`):

| Bench | Time |
|---|---|
| `hot_path/rfc5424_with_faker` | 1.7373 µs (1.7371–1.7374) |
| `template_render_only` | 104.73 ns (104.52–104.78) |
| `faker_ipv4` | 90.27 ns (89.76–92.33) |
| `faker_uuid` | 33.00 ns (32.81–33.79) |
| `faker_username` | 20.19 ns (20.13–20.43) |

YAML в `b.iter`-теле не вызывается (`load_profile_from_yaml_str` —
один раз до `b.iter`, `benches/hot_path.rs:38`), поэтому эти числа не
зависят от YAML-крейта.

**Fuzz smoke** (`cargo fuzz run profile_parser -- -max_total_time=30 -max_len=4096`):

```
runs:    487138 in 30 seconds
exec/s:  15714
crashes: 0
```

Это 30-секундный smoke, не 2-часовой rerun из acceptance #134.

## 8. Контакты и помощь

- Issues: https://github.com/pharmacolog/syslog-generator/issues
- Документация: `docs/USER_GUIDE.md`, `docs/DEVELOPER_GUIDE.md`
- CHANGELOG: `CHANGELOG.md` (полная история breaking changes per release)