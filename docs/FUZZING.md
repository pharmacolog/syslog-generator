# FUZZING

> **v10.4.0:** введён `cargo-fuzz` для проекта. 5 fuzz-таргетов:
> `profile_parser`, `format_rfc5424`, `format_cef`, `format_leef`, `format_json_lines`.
> Fuzz-тесты запускаются вручную / по расписанию (не в обычном CI).

## Что такое fuzzing

Fuzzing — это автоматическое тестирование с произвольными входными данными.
`cargo-fuzz` использует libFuzzer (часть LLVM) для генерации edge-case
входов, которые находят panics, undefined behavior и infinite loops.

## Установка

```bash
# Требует nightly toolchain (libFuzzer).
rustup install nightly

# Установить cargo-fuzz (один раз):
cargo install cargo-fuzz

# Запуск конкретного таргета (из корня проекта):
cargo +nightly fuzz run profile_parser
cargo +nightly fuzz run format_rfc5424
cargo +nightly fuzz run format_cef
cargo +nightly fuzz run format_leef
cargo +nightly fuzz run format_json_lines
```

## Что покрыто fuzz-таргетами

| Target | Что fuzzит | Что может сломаться |
|---|---|---|
| `profile_parser` | `load_profile_from_yaml_str` (произвольный YAML) | `serde_yaml` panics, некорректные UTF-8 |
| `format_rfc5424` | `build_rfc5424` с произвольными полями Header | escaping, переполнение буфера |
| `format_cef` | `cef::build` с произвольными extensions | CEF escaping (`|`, `=`, `\`) |
| `format_leef` | `leef::build` с произвольными attributes | LEEF escaping |
| `format_json_lines` | `json_lines::build` с произвольными полями | `serde_json` edge cases |

## Запуск в CI

Fuzzing не входит в обычный CI (это долгий процесс, до часов).
Рекомендуется запускать на отдельном schedule (например, weekly):

```yaml
# .github/workflows/fuzz.yml (пример, не добавлено в v10.4.0)
name: Fuzz
on:
  schedule:
    - cron: '0 0 * * 0'  # раз в неделю
jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo +nightly fuzz run profile_parser -- -max_total_time=300
```

## Артефакты

Найденные edge cases сохраняются в `fuzz/corpus/<target>/` и могут быть
воспроизведены через:

```bash
# После нахождения failing input:
cargo +nightly fuzz run profile_parser fuzz/corpus/profile_parser/<id>

# Минимизация (уменьшить вход до минимума, который всё ещё падает):
cargo +nightly fuzz tmin profile_parser fuzz/corpus/profile_parser/<id>
```

## Структура

```
fuzz/
├── Cargo.toml           # fuzz-крейт (зависимости + бинарники)
└── fuzz_targets/
    ├── profile_parser.rs
    ├── format_rfc5424.rs
    ├── format_cef.rs
    ├── format_leef.rs
    └── format_json_lines.rs
```

## Что НЕ покрыто fuzz-таргетами (out of scope)

- **Transport layer** (TCP/TLS/UDP/Kafka/file): требует реального сетевого
  I/O, что в fuzz-окружении недостижимо. Покрывается integration-тестами.
- **Reconnect logic** (`reconnect::reconnect_with_backoff`): уже покрыт
  10 unit-тестами в `src/transport/reconnect.rs::tests`.
- **Metrics layer**: side-effect only, не парсит входы.
- **CLI parsing**: covered by clap's own tests + integration tests.

## Coverage gate (v10.4.0)

Coverage gate ≥ 97% lines запланирован в **v10.4.1** (patch), когда
покрытие действительно дотянет до целевого значения. В v10.4.0 покрытие
улучшено до ~87% (с baseline 86.40% в v10.3.1) — это прогресс, а не
полное достижение цели. Fuzzing — главная ценность v10.4.0.
