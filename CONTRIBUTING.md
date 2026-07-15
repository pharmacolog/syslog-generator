# Contributing to syslog-generator

Спасибо за интерес к проекту! Мы приветствуем любые вклады — bug fixes,
features, документация, benchmarks.

## Code of Conduct

Участвуя в проекте, вы соглашаетесь следовать [Code of Conduct](CODE_OF_CONDUCT.md).
Пожалуйста, сообщайте о неприемлемом поведении команде проекта.

## How to Contribute

### Reporting Bugs

Используйте [GitHub Issues](https://github.com/pharmacolog/syslog-generator/issues)
с шаблоном **Bug Report**. Включите:

- Версию (`syslog-generator --version`)
- ОС и Rust toolchain (`rustc --version`)
- Минимальный воспроизводимый пример (profile + команда)
- Ожидаемое vs фактическое поведение
- Логи (если применимо; **деперсонализируйте** чувствительные данные)

### Suggesting Features

Откройте **Feature Request** issue с:
- Мотивацией (какую проблему решает)
- Предложением API (если есть)
- Альтернативами, которые вы рассматривали
- Готовность реализовать самостоятельно

### Improving Documentation

Документация часто отстаёт от кода. PR с улучшением docs/USER_GUIDE.md,
docs/DEVELOPER_GUIDE.md, rustdoc-комментариев, или исправлением опечаток
**очень приветствуются** и обычно принимаются быстро.

### Submitting Code

#### Workflow

```text
feature/pr-N-* → dev (CI green) → release/v*.*.* (CI green) →
main (release-gate) → tag v*.*.* → push
```

1. **Fork** репозиторий
2. **Clone** ваш fork: `git clone https://github.com/YOUR_USERNAME/syslog-generator.git`
3. **Создайте feature branch** от `dev`: `git checkout -b feature/pr-N-my-feature dev`
4. **Сделайте изменения** (код + тесты + docs)
5. **Запустите Quality Gates** (см. ниже)
6. **Commit** с описательным сообщением
7. **Push** в ваш fork
8. **Откройте PR** в `dev` через [GitHub](https://github.com/pharmacolog/syslog-generator/compare)
9. **Дождитесь CI** на dev (зелёный)
10. **Maintainer** смерджит в release/vX.Y.Z → main → tag

## Quality Gates (ОБЯЗАТЕЛЬНО перед PR)

Каждый PR обязан пройти все gates локально:

```bash
# Format
cargo fmt --all -- --check

# Clippy (strict, no warnings allowed)
cargo clippy --no-default-features --all-targets -- -D warnings
cargo clippy --features kafka --all-targets -- -D warnings
cargo clippy --features kafka,test-helpers --all-targets -- -D warnings

# Tests (339 unit/integration должны быть зелёные)
cargo test --locked --features test-helpers

# Kafka tests
cargo test --locked --features kafka,test-helpers

# Doc (no broken links, no warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Public API (no breaking changes без обоснования)
cargo public-api --features test-helpers 2>/dev/null > /tmp/api.txt
diff -u api-snapshot.txt /tmp/api.txt  # должны быть идентичны

# Build (release)
cargo build --release --locked

# Bench compiles
cargo bench --no-run --locked
```

**Лайфхак:** запустите всё одной командой:

```bash
./scripts/quality-gates.sh   # (см. scripts/)
```

или вручную:

```bash
cargo fmt --all && \
cargo clippy --no-default-features --all-targets -- -D warnings && \
cargo clippy --features kafka --all-targets -- -D warnings && \
cargo clippy --features kafka,test-helpers --all-targets -- -D warnings && \
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps && \
cargo test --locked --features test-helpers && \
cargo bench --no-run --locked
```

## Code Style

### Rust Style

- **Rust 2021 edition**, MSRV 1.95
- **`cargo fmt`** (rustfmt default config, см. `rustfmt.toml` если есть)
- **`cargo clippy -- -D warnings`** (strict mode)
- **Errors**: типизированные через `thiserror` (N7 invariant)
- **No `.unwrap()`/`.expect()`** в runtime коде (только в `#[cfg(test)]`)
- **No `panic!()`/`unreachable!()`/`todo!()`** в non-test коде (только defensive guards с обоснованием)
- **No `unsafe`** (`#![deny(unsafe_code)]` enforced)
- **Doc comments** для всех `pub` items (TODO: enforced via `#![warn(missing_docs)]`)

### Naming Conventions

- **Modules**: `snake_case`
- **Functions/Methods**: `snake_case`
- **Types/Traits**: `UpperCamelCase`
- **Constants**: `UPPER_SNAKE_CASE`
- **Error variants**: `UpperCamelCase`, struct fields `snake_case`
- **Tests**: `#[test] fn <unit>_<scenario>_<expected>() { ... }`

### File Organization

```
src/
├── <layer>/           # format/, transport/, observability/, generator/
│   ├── mod.rs         # public API of the layer
│   └── <file>.rs      # concrete implementations
├── <component>.rs     # cross-cutting (payload, template, schema, ...)
└── lib.rs             # pub use re-exports + backward-compat shims
```

## Testing Requirements

### Unit Tests (inline `#[cfg(test)] mod tests`)

Каждый `pub` item должен иметь хотя бы один unit-тест:

```rust
pub fn my_function(x: u32) -> Result<u32> {
    // ...
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn my_function_returns_ok_for_valid_input() {
        assert_eq!(my_function(42).unwrap(), 42);
    }

    #[test]
    fn my_function_returns_error_for_invalid_input() {
        assert!(my_function(0).is_err());
    }
}
```

### Integration Tests (`tests/`)

Для end-to-end сценариев (multi-target, F12 metrics, N4 TLS handshake):

```rust
// tests/integration_tests.rs
#[test]
fn test_n4_cipher_policy_e2e_tls_handshake() {
    // ...
}
```

### Property-Based Tests (`src/payload_proptests.rs`)

Для invariant-проверок (rng determinism, int ranges, regex validity):

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_int_in_range(min in 0i64..1000, max in 0i64..1000) {
        prop_assume!(min <= max);
        let val = int_in_range(min, max, &mut rng);
        assert!(val >= min && val <= max);
    }
}
```

### Fuzzing (`fuzz/`)

Новые форматы/транспорты должны иметь fuzz-таргет в `fuzz/fuzz_targets/`.

## Adding a New Format

См. [docs/DEVELOPER_GUIDE.md §3.1](docs/DEVELOPER_GUIDE.md#31-добавление-нового-формата) — полный шаблон.

Краткий чек-лист:

- [ ] Создать `src/format/<name>.rs` с `pub fn build(ctx, msg) -> Vec<u8>`
- [ ] Добавить вариант в `FormatKind` enum (`src/format/mod.rs`)
- [ ] Обновить `FormatKind::name()` и `FormatKind::from_str_or_default()`
- [ ] Добавить arm в `impl Format for FormatKind::render`
- [ ] Добавить в `schemas/profile.schema.json` (enum `format`)
- [ ] Тесты: round-trip + edge cases (UTF-8 boundaries, empty msg)
- [ ] Property-based: determinism + invariants
- [ ] Fuzz target в `fuzz/fuzz_targets/format_<name>.rs`
- [ ] Bench в `benches/format/<name>.rs`
- [ ] Пример в `examples/<name>_format.json`
- [ ] Документация в `docs/USER_GUIDE.md`
- [ ] `CHANGELOG.md` — секция нового релиза

## Adding a New Transport

См. [docs/DEVELOPER_GUIDE.md §4.1](docs/DEVELOPER_GUIDE.md#41-добавление-нового-транспорта).

## Commit Messages

Следуйте [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short description>

[optional body]

[optional footer(s)]
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `perf`

**Examples:**
```
feat(format): add CBOR format support (F18)
fix(tls): close_notify before exit (N12)
docs: rewrite USER_GUIDE for v10.7.4
perf(payload): pre-resolve templates per phase (PR-5.1)
chore(deps): bump tokio to 1.43
```

## Release Process (для maintainers)

См. [CLAUDE_HANDOFF.md §12](CLAUDE_HANDOFF.md) и `.github/workflows/ci.yml`.

1. `feature/pr-N-*` → `dev` (CI green)
2. `dev` → `release/v*.*.*` (CI green)
3. `release/v*.*.*` → `main` (release-gate, CI green)
4. Bump `Cargo.toml` version
5. Update `CHANGELOG.md`, `README.md`, `CLAUDE_HANDOFF.md`
6. `cargo build --release`
7. Tag `v*.*.*` и push
8. Архив в `.archived-releases/` (НЕ в git)

## Questions?

- 💬 [GitHub Discussions](https://github.com/pharmacolog/syslog-generator/discussions)
- 📖 [docs/USER_GUIDE.md](docs/USER_GUIDE.md)
- 🔧 [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md)
- 📜 [CHANGELOG.md](CHANGELOG.md)

---

**Спасибо за вклад в syslog-generator! 🚀**