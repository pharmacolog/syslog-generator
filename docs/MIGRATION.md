# MIGRATION GUIDE

> **Версия:** v10.7.4. Документ описывает breaking changes и шаги миграции.

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

## 4. v10.7.4 — текущая версия

**0 breaking changes** от v10.7.3.

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

## 6. Контакты и помощь

- Issues: https://github.com/pharmacolog/syslog-generator/issues
- Документация: `docs/USER_GUIDE.md`, `docs/DEVELOPER_GUIDE.md`
- CHANGELOG: `CHANGELOG.md` (полная история breaking changes per release)