# Contributing to syslog-generator

Спасибо за интерес к проекту! Мы приветствуем любые вклады — bug fixes,
features, документация, benchmarks.

## ⚠️ Обязательный Git Flow (все мержи через PR)

**Все изменения в syslog-generator мерджатся ТОЛЬКО через GitHub Pull Request**
(никаких `git push` напрямую в `main` или `dev`). Это enforced через
[Branch Protection Rules](.github/branch-protection.md) и обязательно для
всех участников (включая maintainers).

### Ветки и их назначение

| Branch | Назначение | Кто может merge | Способ merge |
|---|---|---|---|
| **`main`** | Стабильный релизный код. Только релизы. | Maintainer (с review) | Только PR |
| **`dev`** | Интеграционная ветка. Всегда зелёная. | Maintainer | PR (auto-sync через workflow) |
| `feature/*`, `fix/*` | Новые фичи/фиксы | Maintainer + author | PR → dev |
| `release/vX.Y.Z` | Подготовка релиза | Maintainer | PR → main |

### Flow для нового изменения

```text
feature/pr-N-* → PR → dev → CI green → merge
                            ↓
                (когда готов релиз)
                            ↓
                            → release/vX.Y.Z → PR → main → CI green → merge
                                                                          ↓
                                                          auto-sync main → dev (workflow)
```

1. **Создайте feature branch от `dev`**: `git checkout -b feature/pr-N-my-feature dev`
2. **Сделайте изменения** (код + тесты + docs)
3. **Пройдите Quality Gates** локально (см. ниже)
4. **Commit** с Conventional Commits message
5. **Push** в fork или в origin (если у вас есть права на feature-ветки)
6. **Откройте PR** `feature/*` → `dev` через [GitHub UI](https://github.com/pharmacolog/syslog-generator/compare)
7. **Дождитесь CI** на этом PR (все 7 blocking jobs должны быть зелёные)
8. **Maintainer review + merge** через GitHub UI
9. **Никаких локальных `git merge` или `git push` в `main`/`dev`** — только PR

### Sync main → dev

После каждого merge в `main` GitHub Actions workflow
`.github/workflows/sync-main-to-dev.yml` **автоматически** создаёт PR
`main → dev` для синхронизации. Этот PR требует тех же 7 CI checks
(что и любой другой PR в `dev`). После зелёного CI — merge.

### Почему это важно

- **Auditability:** каждый merge имеет PR, review, CI logs, conversation history
- **Quality:** ни один merge не проходит без 7 CI checks (coverage ≥ 87%, cargo-deny, cargo-machete, public-api snapshot, etc.)
- **Rollback:** через `git revert -m 1 <merge-sha>` или GitHub UI "Revert"
- **CodeQL scanning:** PR триггерит CodeQL analysis, который ловит security issues (см. alert #7 — code injection в notify-telegram.yml, пофикшен через PR)

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

#### Workflow (ОБЯЗАТЕЛЬНО через PR)

> ⚠️ **Все мержи — через GitHub Pull Request.** Никаких прямых push'ей в
> `main` или `dev`. Это enforced через [Branch Protection Rules](.github/branch-protection.md).

```text
feature/pr-N-* → PR → dev → CI green → merge
                            ↓
                (когда готов релиз)
                            ↓
                dev → release/vX.Y.Z → PR → main → CI green → merge
                                                                      ↓
                                                      auto-sync main → dev (workflow)
```

**Пошагово:**

1. **Создайте feature branch от `dev`**: `git checkout -b feature/pr-N-my-feature dev`
2. **Сделайте изменения** (код + тесты + docs)
3. **Запустите Quality Gates** локально (см. ниже) — все должны пройти
4. **Commit** с [Conventional Commits](#commit-messages) message
5. **Push** в origin (или в ваш fork): `git push -u origin feature/pr-N-my-feature`
6. **Откройте PR** `feature/*` → `dev` через [GitHub UI](https://github.com/pharmacolog/syslog-generator/compare)
7. **Заполните PR template** (`.github/PULL_REQUEST_TEMPLATE.md`) — checklist обязателен
8. **Дождитесь CI** на PR (все 7 blocking jobs должны быть зелёные)
9. **Maintainer review + merge** через GitHub UI (НЕ локальный merge)
10. **Auto-sync** из main в dev запустится автоматически (через `.github/workflows/sync-main-to-dev.yml`)

**Что НЕЛЬЗЯ делать:**

- ❌ `git push origin main` — прямой push в main заблокирован branch protection
- ❌ `git push origin dev` — для maintainer'ов допустимо только для hotfix через PR
- ❌ `git checkout main && git merge dev && git push` — прямой merge в main
- ❌ Force push в любую защищённую ветку
- ❌ Merge PR с красными CI checks (enforced через `strict: true` в required_status_checks)

## Quality Gates (ОБЯЗАТЕЛЬНО перед PR)

Каждый PR обязан пройти все gates локально (они же дублируются в CI и
проверяются через [Branch Protection](.github/branch-protection.md)):

```bash
# Format
cargo fmt --all -- --check

# Clippy (strict, no warnings allowed)
cargo clippy --no-default-features --all-targets -- -D warnings
cargo clippy --features kafka --all-targets -- -D warnings
cargo clippy --features kafka,test-helpers --all-targets -- -D warnings

# Tests (399 unit/integration должны быть зелёные)
cargo test --locked --features test-helpers

# Kafka tests
cargo test --locked --features kafka,test-helpers

# Doc (no broken links, no warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Public API (no breaking changes без обоснования)
cargo public-api --features test-helpers 2>/dev/null > /tmp/api.txt
diff -u api-snapshot.txt /tmp/api.txt  # должны быть идентичны
# Если изменился API — обновите api-snapshot.txt и обоснуйте в PR

# Build (release)
cargo build --release --locked

# Bench compiles
cargo bench --no-run --locked

# N7 invariant (no unwrap/expect/panic в runtime коде)
bash scripts/check-n7-invariant.sh

# Если меняли .github/workflows/*.yml: bash syntax check для embedded scripts
for f in .github/workflows/*.yml; do
  python3 -c "import yaml; yaml.safe_load(open('$f'))"  # yaml syntax
  awk '/^      - name:/,/^      - name:|^jobs:/' "$f" | \
    sed 's/^        //' | bash -n  # shell syntax в run: | блоках
done
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

> ⚠️ **Все шаги — через PR.** Никаких прямых push'ей в `main` или `dev`.

Полная документация — в [CLAUDE_HANDOFF.md](CLAUDE_HANDOFF.md) и
[`.github/branch-protection.md`](.github/branch-protection.md).
Конфигурация защиты веток — в [`.github/branch-protection.md`](.github/branch-protection.md).

**Шаги для release PR (через GitHub UI):**

1. **Убедиться, что dev зелёный** — все CI checks pass на последнем commit
2. **Создать `release/vX.Y.Z` от main** (через GitHub UI или `git checkout -b release/vX.Y.Z origin/main`)
3. **Открыть PR `dev` → `release/vX.Y.Z`** — merge после CI green
4. **Bump `Cargo.toml`** версию (например `10.7.14` → `10.7.15`)
5. **Обновить документацию** (в том же коммите или отдельным коммитом):
   - `CHANGELOG.md` (новая секция vX.Y.Z)
   - `README.md` (badge + Docker tags + docs table)
   - `AUDIT.md` (статусы вех и задач)
   - `CLAUDE_HANDOFF.md` (текущая версия + история)
   - `examples/` (если нужно)
6. **Push** `release/vX.Y.Z` в origin (через PR review, не напрямую)
7. **Открыть PR `release/vX.Y.Z` → `main`** — требует 1 approval + 7 CI checks
8. **Дождаться CI** на этом PR (все blocking jobs зелёные)
9. **Merge** через GitHub UI (squash или merge commit — оба ОК)
10. **Push tag `vX.Y.Z`**: `git push origin vX.Y.Z` (этот push НЕ идёт через PR — теги
    обрабатываются отдельно; trigger'ит Docker + SBOM workflows)
11. **Создать GitHub Release** через `gh release create vX.Y.Z --target vX.Y.Z --title ... --notes ...`
    с архивом (`syslog-generator-vX.Y.Z-verified.zip`) и SBOM (`sbom-vX.Y.Z.cdx.json`)
12. **Auto-sync main → dev** запустится автоматически (через `.github/workflows/sync-main-to-dev.yml`).
    Maintainer merge'ит этот sync PR после CI green.

**Что НЕЛЬЗЯ делать в release:**

- ❌ `git push origin main` — заблокировано branch protection
- ❌ Force push в `release/v*.*.*` после создания
- ❌ Merge release PR с красными CI (даже если они non-blocking — strict mode требует fresh green)
- ❌ Забыть bump `Cargo.toml` (это приведёт к несоответствию tag и версии в бинарнике)

**Чек-лист перед merge release PR:**

- [ ] Все 7 CI blocking checks SUCCESS
- [ ] Все CodeQL analyses (Analyze (actions), Analyze (rust)) SUCCESS
- [ ] `Cargo.toml` version совпадает с tag name
- [ ] `CHANGELOG.md` содержит секцию vX.Y.Z
- [ ] `git diff vX.Y.Z-1 vX.Y.Z -- Cargo.toml` показывает корректный bump
- [ ] Локальный `cargo build --release` показывает новую версию через `--version`

## Questions?

- 💬 [GitHub Discussions](https://github.com/pharmacolog/syslog-generator/discussions)
- 📖 [docs/USER_GUIDE.md](docs/USER_GUIDE.md)
- 🔧 [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md)
- 📜 [CHANGELOG.md](CHANGELOG.md)

---

**Спасибо за вклад в syslog-generator! 🚀**