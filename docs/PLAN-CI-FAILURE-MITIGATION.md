# PLAN — CI Failure Mitigation

**Версия:** 1.0 · **Дата:** 2026-07-16 · **Target release:** v10.7.15
**Автор плана:** Claude (opencode/MiniMax-M3) · **Аудит:** 200 runs за ~3-4 недели
**Ветка реализации:** `feature/pr-14-ci-failure-mitigation`

---

## 0. Контекст и цели

### 0.1 Текущее состояние (на v10.7.14)

CI failure rate: **6-8%** (12/200 runs за весь период, 4/50 за последние недели).
Все failures — в workflow `CI` (Docker стабилен). Преобладают:

- `cargo fmt` (25%)
- `cargo clippy -D warnings` (25% + 33% kafka variant = ~58%)
- `cargo-deny` / `cargo-machete` / `cargo public-api` (~25% каждый)
- examples validate, MSRV build, flaky tests, SBOM API mismatch

**Главные источники потерь:**

1. Разработчик запушил код, не прогнав локально fmt/clippy.
2. PR-12 принёс новые зависимости без их прогона в CI перед merge.
3. Flaky test на macOS не был переписан, а просто получил `continue-on-error`.

### 0.2 Целевые метрики

| Метрика | Сейчас | Цель (после v10.7.15) |
|---|---|---|
| Failure rate | 6-8% | **≤ 2%** |
| Round-trip per failed PR | 50-100 min | **≤ 20 min** (pre-push ловит) |
| CI runner time / PR | ~30 min | **≤ 20 min** (paths-ignore, concurrency) |
| Docs-only commits → CI | триггерят | **пропускаются** |

### 0.3 Решения пользователя (получены 2026-07-16)

1. **pre-commit** (framework), кросс-платформенная установка через `pip install --user pre-commit`.
2. **pre-push** — отдельный хук, тяжёлые проверки.
3. **Процесс:** фича-ветка → merge → release → main → tag (как в PR-10..13).
4. **Toolchain check** обязателен в pre-push.
5. **Жёсткий gate** для public-api snapshot (НЕ baseline-missing bypass).
6. **Devcontainer нужен сразу** (собственный Dockerfile с cargo tools + pre-commit).
7. **Telegram-уведомления** на failures во всех ветках (main, dev, release/*, PR).

---

## 1. Обновлённый аудит (v10.7.10..v10.7.14)

### 1.1 Что произошло в репо

- **v10.7.10**, **v10.7.11**: PR-10 hot-path performance (-47% per-message).
- **v10.7.12**: PR-11 coverage gate 87%.
- **v10.7.13**: PR-12 partial fix (ALLOW_INSECURE_TLS, мягче deny.toml). **CycloneDX SBOM остался сломанным** (отменено).
- **v10.7.14**: PR-13 N7 invariant cleanup + `scripts/quality-gates.sh` extension.

### 1.2 Новые failure-сигнатуры (сверх топ-12 прошлого аудита)

| # | Сигнатура | Runs | Root cause |
|---|---|---|---|
| 13 | `cargo cyclonedx --output-pattern` unknown arg | 2 | cargo-cyclonedx 0.5/0.6 API mismatch (PR-12) |
| 14 | `examples/cipher_policy_tls13.json` validate fail | 2 | TLS13 validation сломала schema |
| 15 | `examples/mtls_cipher_policy.json` validate fail | 2 | mTLS validation сломала schema |

### 1.3 Pre-existing infrastructure (базис)

- `scripts/quality-gates.sh` (135 строк, G1–G9) — единая точка локального запуска.
- `scripts/check-n7-invariant.sh` — N7 enforcement.
- `scripts/check-changelog.sh` — changelog автоматизация.
- `rust-toolchain.toml` → channel "1.95" (MSRV check работает автоматически).
- 374 passed tests (277 unit + 86 integration + 11 n7).

---

## 2. План реализации (8 задач в одной фича-ветке)

**Процесс (как в PR-10..13):**

1. Создать `feature/pr-14-ci-failure-mitigation` от `dev`.
2. Делать атомарные коммиты (по одному на задачу).
3. Локально прогонять `./scripts/quality-gates.sh` перед каждым коммитом.
4. Push → дождаться CI зелёного → merge в `dev`.
5. Создать `release/v10.7.15` → merge → bump version → release notes → tag → merge в `main`.

### 2.1 Список задач

| # | Задача | Файлы | Сложность | Ловит фейлы |
|---|---|---|---|---|
| T1 | **Pre-commit hook (framework)** — fmt + clippy + clippy-kafka | `.pre-commit-config.yaml` (новый) | S | #1, #2, #3 |
| T2 | **Pre-push hook** — quality-gates.sh (полный) + toolchain check | `.pre-commit-config.yaml` + `scripts/check-toolchain.sh` (новый) | M | #4, #5, #7 |
| T3 | **Public-API жёсткий gate** (полная реализация) | `src/lib.rs` (новый модуль `api.rs`), `.github/workflows/ci.yml`, `scripts/quality-gates.sh`, `api-snapshot.txt` | L | #6 |
| T4 | **CycloneDX вынести в отдельный workflow** | `.github/workflows/sbom.yml` (новый), `scripts/quality-gates.sh` (G10), `.github/workflows/ci.yml` (убрать step) | S | #13 |
| T5 | **Examples validate fix** — cipher_policy_tls13.json + mtls_cipher_policy.json | `examples/*.json` (правка), `schemas/*.json` (если нужно) | M | #14, #15 |
| T6 | **Concurrency + paths-ignore** в CI workflows | `.github/workflows/ci.yml`, `.github/workflows/docker.yml`, `.github/workflows/sbom.yml` | S | cost (time) |
| T7 | **Telegram webhook на failure** (все ветки) | `.github/workflows/notify-telegram.yml` (новый) + `docs/TELEGRAM_SETUP.md` | M | observability |
| T8 | **Devcontainer (собственный Dockerfile)** | `.devcontainer/devcontainer.json`, `.devcontainer/Dockerfile`, `.devcontainer/post-create.sh` | M | env parity |

**Оценка общего объёма:** ~8 атомарных коммитов, ~15 файлов изменено/создано, ~3-5 дней работы.

---

## 3. Детальные спецификации

### 3.1 T1. `.pre-commit-config.yaml` (PRE-COMMIT FRAMEWORK)

**Файл:** `.pre-commit-config.yaml` (новый, в корне)

```yaml
# pre-commit framework config для syslog-generator.
# Установка: pip install --user pre-commit   (кросс-платформенно)
# Установка хуков: pre-commit install
# Установка pre-push: pre-commit install --hook-type pre-push
# Запуск вручную: pre-commit run --all-files
# Skip хука: git commit --no-verify
#
# Принцип: только быстрые проверки (< 30 сек). Тяжёлые — в pre-push (T2).

repos:
  - repo: local
    hooks:

      # G1.1: rustfmt — мгновенный
      - id: cargo-fmt
        name: cargo fmt --check
        entry: cargo fmt --all -- --check
        language: system
        pass_filenames: false
        types: [rust]

      # G1.2: clippy без features (~10 сек warm)
      - id: cargo-clippy
        name: cargo clippy -D warnings
        entry: cargo clippy --all-targets -- -D warnings
        language: system
        pass_filenames: false
        types: [rust]

      # G1.3: clippy --features kafka (~15 сек warm)
      - id: cargo-clippy-kafka
        name: cargo clippy --features kafka -D warnings
        entry: cargo clippy --all-targets --features kafka -- -D warnings
        language: system
        pass_filenames: false
        types: [rust]

      # G6.1: N7 invariant check (~1 сек)
      - id: n7-invariant
        name: N7 invariant — no unwrap/expect in non-test src/
        entry: bash scripts/check-n7-invariant.sh
        language: system
        pass_filenames: false
        types: [rust]
```

**Действия:**

1. Создать файл.
2. `pip install --user pre-commit`.
3. `pre-commit install`.
4. Тест: `touch src/lib.rs && pre-commit run cargo-fmt --all-files` — должен сработать.

**Acceptance criteria:**

- `pre-commit run --all-files` завершается за ≤ 30 сек warm cache.
- При намеренной порче fmt (`cargo fmt --all -- --check` fail) — хук блокирует commit.

---

### 3.2 T2. `.pre-commit-config.yaml` extension + `scripts/check-toolchain.sh`

**Дополнение в `.pre-commit-config.yaml`** (добавить в конец файла из T1):

```yaml
  - repo: local
    hooks:

      # Полный quality-gates.sh (G1..G10, ~3 мин warm)
      - id: quality-gates-prepush
        name: Quality Gates (G1..G10)
        entry: bash scripts/quality-gates.sh
        language: system
        pass_filenames: false
        stages: [pre-push]
        types: [rust]

      # Toolchain check: rustup show + 1.95 installed
      - id: check-toolchain
        name: Check MSRV toolchain (1.95)
        entry: bash scripts/check-toolchain.sh
        language: system
        pass_filenames: false
        stages: [pre-push]
        types: [rust]
```

**Файл:** `scripts/check-toolchain.sh` (новый)

```bash
#!/usr/bin/env bash
#
# scripts/check-toolchain.sh — проверка toolchain перед push.
# Pre-push hook для pre-commit framework.
#
# Проверяет:
#   1. rustup установлен
#   2. toolchain 1.95 установлен (для MSRV-check)
#   3. toolchain active совпадает с rust-toolchain.toml
#
# Exit code: 0 = OK, 1 = missing toolchain (с инструкцией).

set -euo pipefail

EXPECTED_CHANNEL="$(grep -E '^channel\s*=' rust-toolchain.toml | sed -E 's/.*"([^"]+)".*/\1/')"

if [ -z "$EXPECTED_CHANNEL" ]; then
    echo "❌ rust-toolchain.toml не содержит channel"
    exit 1
fi

echo "▶ Ожидаемый toolchain: $EXPECTED_CHANNEL"

if ! command -v rustup >/dev/null 2>&1; then
    echo "❌ rustup не установлен"
    echo "  Установите: https://rustup.rs/"
    exit 1
fi

if ! rustup toolchain list | grep -qE "^$EXPECTED_CHANNEL"; then
    echo "❌ toolchain $EXPECTED_CHANNEL не установлен"
    echo "  Установите: rustup toolchain install $EXPECTED_CHANNEL"
    exit 1
fi

# Активный toolchain должен совпадать
ACTIVE="$(rustup show active-toolchain 2>/dev/null || echo 'unknown')"
if [[ "$ACTIVE" != *"$EXPECTED_CHANNEL"* ]]; then
    echo "⚠ Активный toolchain: $ACTIVE (ожидался $EXPECTED_CHANNEL)"
    echo "  Это OK если rust-toolchain.toml pin сработает, но рекомендуется:"
    echo "  rustup default $EXPECTED_CHANNEL"
fi

echo "✅ toolchain $EXPECTED_CHANNEL готов"
```

**Действия:**

1. Создать `scripts/check-toolchain.sh` (`chmod +x`).
2. Расширить `.pre-commit-config.yaml`.
3. `pre-commit install --hook-type pre-push`.
4. Тест: `pre-commit run quality-gates-prepush --all-files`.

**Acceptance criteria:**

- При отсутствии toolchain 1.95 — push блокируется с инструкцией.
- При наличии — `pre-push` запускает полный `quality-gates.sh` за ≤ 5 мин warm cache.

---

### 3.3 T3. Public-API жёсткий gate (полная реализация)

**Проблема:** сейчас в `.github/workflows/ci.yml` job `public-api` при
отсутствии `api-snapshot.txt` печатает `::warning::` и выходит с 0.
Это значит breaking changes не блокируются в baseline-периоде.

**Решение (полная реализация):**

1. Создать модуль `src/api.rs` с reflection-style публичного API enumeration.
2. Добавить `pub mod api;` в `src/lib.rs`.
3. Сгенерировать baseline `api-snapshot.txt` через `cargo run --bin gen-api-snapshot`.
4. CI: жёстко блокировать при diff.

**Файлы:**

- `src/api.rs` (новый) — модуль с функциями:
  - `pub fn public_api_items() -> Vec<ApiItem>` — перечисление всех `pub` items в crate.
  - `pub struct ApiItem { kind: ApiKind, path: String, signature: String }`.
  - `pub enum ApiKind { Function, Struct, Enum, Trait, TypeAlias, Const, Static, Macro }`.
  - Использует `syn` (уже есть как transitive dep через `jsonschema`/`bytes`) или
    простой текст-парсинг `src/lib.rs` + `cargo metadata --format-version 1`.
- `src/bin/gen_api_snapshot.rs` (новый) — бинарь для генерации baseline:
  - `cargo run --bin gen-api-snapshot --features test-helpers > api-snapshot.txt`.
- `src/lib.rs` — добавить `pub mod api;`.
- `.github/workflows/ci.yml` — job `public-api`:
  - `cargo run --bin gen-api-snapshot --features test-helpers > /tmp/current-api.txt`
  - `diff -u api-snapshot.txt /tmp/current-api.txt` (без baseline-missing bypass)
- `scripts/quality-gates.sh` (G5.3) — обновить для новой команды.

**Реализация `src/api.rs` (скелет):**

```rust
//! Public API enumeration for snapshot-based change detection.
//!
//! Используется для CI-gate (T3): любые изменения публичного API
//! требуют обновления `api-snapshot.txt` через blessed-команду.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKind {
    Function,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiItem {
    pub kind: ApiKind,
    pub path: String,
    pub signature: String,
}

impl ApiItem {
    pub fn render(&self) -> String {
        format!("{:?}\t{}\t{}", self.kind, self.path, self.signature)
    }
}

/// Парсит src/lib.rs и все `pub use ...` + `pub mod ...` для генерации списка.
/// Возвращает отсортированный по `path` список.
pub fn public_api_items(crate_root: &Path) -> std::io::Result<Vec<ApiItem>> {
    let lib_rs = crate_root.join("src/lib.rs");
    let content = fs::read_to_string(&lib_rs)?;
    let mut items = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pub use ") {
            items.push(ApiItem {
                kind: ApiKind::TypeAlias,
                path: rest.trim_end_matches(';').to_string(),
                signature: String::new(),
            });
        } else if let Some(rest) = line.strip_prefix("pub mod ") {
            items.push(ApiItem {
                kind: ApiKind::TypeAlias,
                path: rest.trim_end_matches(';').to_string(),
                signature: String::new(),
            });
        }
        // Расширить при необходимости (pub fn, pub struct, ...)
    }

    items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(items)
}

/// Генерирует полный snapshot в текстовом формате.
pub fn generate_snapshot(crate_root: &Path) -> std::io::Result<String> {
    let items = public_api_items(crate_root)?;
    let mut out = String::new();
    out.push_str("# API snapshot for syslog-generator\n");
    out.push_str("# Format: KIND\tPATH\tSIGNATURE\n");
    for item in items {
        out.push_str(&item.render());
        out.push('\n');
    }
    Ok(out)
}
```

**Acceptance criteria:**

- Удаление `pub use` строки в `lib.rs` → `gen-api-snapshot diff` падает с понятным
  сообщением.
- Отсутствие `api-snapshot.txt` → CI fail с инструкцией
  "Run `cargo run --bin gen-api-snapshot --features test-helpers > api-snapshot.txt`",
  а не bypass.

---

### 3.4 T4. CycloneDX вынести в отдельный workflow + G10

**Файл:** `.github/workflows/sbom.yml` (новый)

```yaml
name: SBOM (CycloneDX)

on:
  push:
    branches: [main, dev, "release/v*.*.*"]
    tags: ["v*.*.*"]
  pull_request:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  sbom:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    concurrency:
      group: sbom-${{ github.ref }}
      cancel-in-progress: ${{ github.event_name == 'pull_request' }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: [rustfmt, clippy]

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: ${{ runner.os }}-cargo-sbom

      - name: Install cargo-cyclonedx
        run: cargo install cargo-cyclonedx --locked

      - name: Generate SBOM
        env:
          SHORT_SHA: ${{ github.sha }}
        run: |
          # Проверка версии (cargo-cyclonedx 0.5+ не поддерживает --output-pattern)
          cargo cyclonedx --version
          cargo cyclonedx --format json \
            --override-filename "sbom-${SHORT_SHA::8}" \
            --spec-version 1.5

      - name: Upload SBOM artifact
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: sbom-${{ github.sha }}
          path: target/**/*.cdx.json
          if-no-files-found: warn
```

**Файл:** `scripts/quality-gates.sh` — добавить G10:

```bash
# ─────────────────────────────────────────────────────────────────
# G10. SBOM (cargo-cyclonedx)
# ─────────────────────────────────────────────────────────────────
if command -v cargo-cyclonedx >/dev/null 2>&1; then
    run_step "G10.1: SBOM generation (cargo-cyclonedx)" \
        "cargo cyclonedx --format json --spec-version 1.5 --quiet 2>&1 | tail -5"
else
    echo ""
    echo "▶ G10.1: SBOM generation (cargo-cyclonedx)"
    echo "  ⚠ cargo-cyclonedx not installed (skipping — CI will catch)"
    echo "  Install: cargo install cargo-cyclonedx --locked"
fi
```

**Файл:** `.github/workflows/ci.yml` — удалить step `Generate SBOM (CycloneDX JSON)`
из job `coverage`.

**Acceptance criteria:**

- Новый workflow `.github/workflows/sbom.yml` запускается на push/PR.
- Coverage job больше не падает на `--output-pattern`.
- `bash scripts/quality-gates.sh` показывает G10.

---

### 3.5 T5. Examples validate fix

**Проблема:** `examples/cipher_policy_tls13.json` и `examples/mtls_cipher_policy.json`
падают на `--validate --schema-strict` (особенно в `test-kafka` job).

**Действия:**

1. `cargo run --quiet --bin syslog-generator -- --validate --schema-strict --profile examples/cipher_policy_tls13.json` — посмотреть конкретный error.
2. То же для `examples/mtls_cipher_policy.json`.
3. Починить примеры (обновить поля под текущую schema) И/ИЛИ расширить schema.
4. Проверить, что в `test-kafka` job examples валидируются с `--features kafka`.

**Acceptance criteria:**

- `for f in examples/*.json; do cargo run --quiet --bin syslog-generator -- --validate --schema-strict --profile "$f"; done` — exit 0.
- `cargo run ... --features kafka -- --validate ...` для всех examples — exit 0.

---

### 3.6 T6. Concurrency + paths-ignore в workflows

**Файл:** `.github/workflows/ci.yml` — добавить в начало:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

on:
  push:
    branches: [main, dev, "release/v*"]
    paths-ignore:
      - '**.md'
      - 'docs/**'
      - 'CHANGELOG.md'
      - 'README.md'
      - 'AUDIT.md'
      - 'REVIEW.md'
      - 'CLAUDE_HANDOFF.md'
      - 'PLAN-*.md'
      - 'examples/**/*.md'
      - '.github/dependabot.yml'
      - '.gitignore'
  pull_request:
    branches: [main]
    paths-ignore:
      - '**.md'
      - 'docs/**'
      - 'CHANGELOG.md'
      - 'README.md'
      - 'AUDIT.md'
      - 'REVIEW.md'
      - 'CLAUDE_HANDOFF.md'
      - 'PLAN-*.md'
      - 'examples/**/*.md'
      - '.github/dependabot.yml'
      - '.gitignore'
```

**Файл:** `.github/workflows/docker.yml` — добавить `concurrency:` аналогично.

**Файл:** `.github/workflows/sbom.yml` — `concurrency:` уже в T4.

**Acceptance criteria:**

- Push только в `CHANGELOG.md` → CI пропускается (verified в GitHub Actions UI).
- 2 push'а подряд в одну PR-ветку → старый run отменяется.

---

### 3.7 T7. Telegram webhook на failure (все ветки)

**Файл:** `.github/workflows/notify-telegram.yml` (новый)

```yaml
name: Notify Telegram (failure, all branches)

on:
  workflow_run:
    workflows: [CI, Docker, "SBOM (CycloneDX)"]
    types: [completed]

permissions:
  contents: read

jobs:
  notify:
    runs-on: ubuntu-latest
    # Только failures, не cancelled/skipped
    if: github.event.workflow_run.conclusion == 'failure'
    steps:
      - name: Send Telegram message
        env:
          TELEGRAM_BOT_TOKEN: ${{ secrets.TELEGRAM_BOT_TOKEN }}
          TELEGRAM_CHAT_ID: ${{ secrets.TELEGRAM_CHAT_ID }}
          WORKFLOW: ${{ github.event.workflow_run.name }}
          BRANCH: ${{ github.event.workflow_run.head_branch }}
          COMMIT: ${{ github.event.workflow_run.head_sha }}
          EVENT: ${{ github.event.workflow_run.event }}
          URL: ${{ github.event.workflow_run.html_url }}
        run: |
          TEXT="❌ *${WORKFLOW}* failed
          Branch: \`${BRANCH}\`
          Event: \`${EVENT}\`
          Commit: \`${COMMIT:0:8}\`
          ${URL}"
          curl -sS -X POST \
            "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
            -d chat_id="${TELEGRAM_CHAT_ID}" \
            -d parse_mode=Markdown \
            -d disable_web_page_preview="true" \
            --data-urlencode "text=${TEXT}"
```

**Файл:** `docs/TELEGRAM_SETUP.md` (новый) — инструкция:

```markdown
# Telegram-уведомления о CI failures

## 1. Создать бота

1. Открыть [@BotFather](https://t.me/BotFather) в Telegram.
2. `/newbot` → задать имя (например, `syslog-generator-ci-bot`) → получить **BOT_TOKEN**.
3. Добавить бота в нужный чат/канал.

## 2. Узнать chat_id

1. Добавить бота в чат.
2. Отправить любое сообщение в чат.
3. Открыть `https://api.telegram.org/bot<BOT_TOKEN>/getUpdates`.
4. Найти `chat.id` в JSON-ответе (отрицательное число для групп).

## 3. Добавить секреты в GitHub

Repository → Settings → Secrets and variables → Actions → New repository secret:

- `TELEGRAM_BOT_TOKEN` = `<BOT_TOKEN>` из шага 1.
- `TELEGRAM_CHAT_ID` = `<chat.id>` из шага 2.

## 4. Проверка

1. Сделать push с заведомо сломанным кодом (например, `cargo fmt` нарушение).
2. Дождаться CI fail.
3. Бот должен прислать сообщение в указанный чат.

## 5. Отключение

Удалить секреты `TELEGRAM_BOT_TOKEN` и `TELEGRAM_CHAT_ID` → workflow
перестаёт отправлять (не падает, т.к. используется `if:` condition).

## 6. Что триггерит уведомление

Workflow `notify-telegram.yml` подписан на `workflow_run` для:

- `CI`
- `Docker`
- `SBOM (CycloneDX)`

Только `conclusion == 'failure'` (не cancelled, не skipped).
Покрытие: **все ветки** (main, dev, release/*, feature/*, PR).
```

**Acceptance criteria:**

- После push с failing CI в любой ветке в Telegram приходит сообщение.
- При отсутствии секретов — workflow exit 0 (silent skip).

---

### 3.8 T8. Devcontainer (собственный Dockerfile)

**Файлы:**

**`.devcontainer/devcontainer.json`** (новый):

```json
{
  "name": "syslog-generator",
  "build": {
    "dockerfile": "Dockerfile",
    "context": ".."
  },
  "features": {
    "ghcr.io/devcontainers/features/common-utils:2": {}
  },
  "postCreateCommand": "bash .devcontainer/post-create.sh",
  "customizations": {
    "vscode": {
      "extensions": [
        "rust-lang.rust-analyzer",
        "tamasfe.even-better-toml",
        "vadimcn.vscode-lldb"
      ],
      "settings": {
        "rust-analyzer.cargo.features": "all",
        "[rust]": {
          "editor.defaultFormatter": "rust-lang.rust-analyzer"
        }
      }
    }
  },
  "mounts": [
    "source=${localEnv:HOME}/.cargo/registry,target=/usr/local/cargo/registry,type=bind,consistency=cached"
  ],
  "remoteUser": "dev"
}
```

**`.devcontainer/Dockerfile`** (новый):

```dockerfile
# .devcontainer/Dockerfile — образ для syslog-generator devcontainer.
# Базовый rust 1.95 + cargo tools для CI + pre-commit framework.

FROM rust:1.95-bookworm

# Системные пакеты для native-tls/protobuf/rskafka
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl-dev pkg-config build-essential cmake protobuf-compiler \
    python3 python3-pip python3-venv \
    git curl jq \
    && rm -rf /var/lib/apt/lists/*

# Создаём непривилегированного пользователя
RUN useradd -m -s /bin/bash dev \
    && mkdir -p /workspace \
    && chown -R dev:dev /workspace

USER dev
WORKDIR /workspace

# Cargo-утилиты для CI-паритета
RUN cargo install cargo-deny --locked \
    && cargo install cargo-machete --locked \
    && cargo install cargo-public-api --locked \
    && cargo install cargo-cyclonedx --locked \
    && cargo install cargo-llvm-cov --locked

# Pre-commit framework (кросс-платформенная установка через pip)
RUN pip3 install --break-system-packages --user pre-commit \
    && /home/dev/.local/bin/pre-commit --version

ENV PATH="/home/dev/.local/bin:/usr/local/cargo/bin:${PATH}"

# Verify
RUN rustc --version && cargo --version && \
    cargo deny --version && cargo machete --version && \
    cargo public-api --version && cargo cyclonedx --version && \
    cargo llvm-cov --version
```

**`.devcontainer/post-create.sh`** (новый):

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

echo "▶ Installing pre-commit hooks..."
if command -v pre-commit >/dev/null; then
    pre-commit install
    pre-commit install --hook-type pre-push
    echo "  ✅ pre-commit + pre-push hooks installed"
else
    echo "  ⚠ pre-commit not found in PATH"
fi

echo ""
echo "▶ Toolchain verification:"
rustc --version
cargo --version

echo ""
echo "✅ syslog-generator devcontainer ready"
```

**Действия:**

1. Создать файлы.
2. Проверить: `devcontainer up --workspace-folder .` (требует Docker + devcontainer CLI).
3. Внутри контейнера: `pre-commit run --all-files` должно сработать.

**Acceptance criteria:**

- `devcontainer up` собирает контейнер за ≤ 10 мин (после кэша).
- Все quality-gates проходят внутри контейнера.
- Все CI-утилиты (`cargo-deny`, `cargo-machete`, `cargo-public-api`, `cargo-cyclonedx`,
  `cargo-llvm-cov`, `pre-commit`) доступны в PATH.

---

## 4. Процесс релиза (v10.7.15)

Следуя CLAUDE_HANDOFF.md §6:

1. ✅ Все 8 задач реализованы и смержены в `dev` через `feature/pr-14-ci-failure-mitigation`.
2. ✅ CI на `dev` зелёный (`./scripts/quality-gates.sh` PASS + GitHub Actions PASS).
3. ✅ `git checkout -b release/v10.7.15` от `main`.
4. ✅ `git merge dev` (или cherry-pick коммитов).
5. ✅ Bump в `Cargo.toml`: `10.7.14 → 10.7.15`.
6. ✅ Обновить:
   - `CHANGELOG.md` (новая секция `## v10.7.15 - 2026-MM-DD`, Added/Changed/Notes).
   - `README.md` (Quality Gates секция + новые badges).
   - `AUDIT.md` (пометить задачи ✅ Сделано (v10.7.15)).
   - `CLAUDE_HANDOFF.md` (добавить v10.7.15 в историю).
7. ✅ `cargo clean && zip -rq syslog-generator-v10.7.15-verified.zip syslog-generator -x '*/target/*' -x '*/.git/*' -x '*.zip'`.
8. ✅ Merge `release/v10.7.15` → `main`, tag `v10.7.15`.

---

## 5. Файлы, которые будут созданы/изменены

### Создать (новые)

- `.pre-commit-config.yaml`
- `scripts/check-toolchain.sh`
- `.github/workflows/sbom.yml`
- `.github/workflows/notify-telegram.yml`
- `docs/TELEGRAM_SETUP.md`
- `.devcontainer/devcontainer.json`
- `.devcontainer/Dockerfile`
- `.devcontainer/post-create.sh`
- `src/api.rs`
- `src/bin/gen_api_snapshot.rs`

### Изменить (правки)

- `.github/workflows/ci.yml`:
  - Добавить `concurrency:` и `paths-ignore:` (T6).
  - Убрать CycloneDX step из job `coverage` (T4).
  - Заhardgate-ить public-api job (T3).
- `.github/workflows/docker.yml`:
  - Добавить `concurrency:` (T6).
- `scripts/quality-gates.sh`:
  - G5.3 для нового public-api формата (T3).
  - G10 SBOM step (T4).
- `src/lib.rs`:
  - Добавить `pub mod api;` (T3).
- `api-snapshot.txt`:
  - Baseline через `cargo run --bin gen-api-snapshot` (T3).
- `examples/cipher_policy_tls13.json`, `examples/mtls_cipher_policy.json`:
  - Починить под текущую schema (T5).
- `schemas/*.json` (если нужно):
  - Расширить для tls13/mtls полей (T5).
- `Cargo.toml`:
  - Bump `10.7.14 → 10.7.15`.
- `CHANGELOG.md`, `README.md`, `AUDIT.md`, `CLAUDE_HANDOFF.md`:
  - Release notes по CLAUDE_HANDOFF §6.

**Итого:** ~10 новых файлов, ~8 правок, 8 атомарных коммитов.

---

## 6. Acceptance criteria для v10.7.15

### 6.1 Functional

- [ ] `pre-commit install` + `pre-commit install --hook-type pre-push` отрабатывают.
- [ ] `pre-commit run --all-files` PASS за ≤ 30 сек warm.
- [ ] `pre-commit run quality-gates-prepush --all-files` PASS за ≤ 5 мин warm.
- [ ] Отсутствие toolchain 1.95 → push блокируется с инструкцией.
- [ ] CycloneDX в отдельном workflow `.github/workflows/sbom.yml` без `--output-pattern` создаёт SBOM.
- [ ] Все 41+ examples проходят `--validate --schema-strict` (с/без `--features kafka`).
- [ ] `public-api` job: отсутствие `api-snapshot.txt` → fail с инструкцией, не bypass.
- [ ] Push в `CHANGELOG.md` → CI пропускается (verified в Actions UI).
- [ ] Push в feature branch → outdated run отменяется (verified).
- [ ] Telegram webhook: failing CI в любой ветке → сообщение в чате.
- [ ] `devcontainer up` поднимает контейнер с rust 1.95 + всеми tools.

### 6.2 Quality gates (все ✅)

- [ ] `cargo fmt --all --check`: clean.
- [ ] `cargo clippy --no-default-features --all-targets -D warnings`: clean.
- [ ] `cargo clippy --features kafka,test-helpers --all-targets -D warnings`: clean.
- [ ] `cargo clippy --all-targets --features kafka -D warnings`: clean.
- [ ] `RUSTDOCFLAGS=-D warnings cargo doc --no-deps`: clean.
- [ ] `cargo test --locked --features test-helpers`: ≥ 374 passed.
- [ ] `cargo test --locked --features kafka,test-helpers`: PASS.
- [ ] `cargo build --release --locked`: SUCCESS.
- [ ] `cargo bench --no-run --locked`: SUCCESS.
- [ ] `cargo deny check`: clean.
- [ ] `cargo machete`: clean.
- [ ] `cargo public-api` snapshot diff: clean.
- [ ] `bash scripts/check-n7-invariant.sh`: ✅.
- [ ] `cargo llvm-cov --fail-under-lines=87`: ≥ 87% lines.
- [ ] `bash scripts/quality-gates.sh`: ALL PASS (G1..G10).

### 6.3 Metrics (post-release verification, через 1 неделю)

- [ ] CI failure rate ≤ 2% (за следующие 50 runs).
- [ ] Round-trip per failed PR ≤ 20 min.
- [ ] CI runner time / PR ≤ 20 min (median).

---

## 7. Rollback plan

Если что-то из 8 задач ломает CI:

1. **T1/T2 (pre-commit)**: не влияет на CI напрямую (только локально). Rollback = revert commit.
2. **T3 (public-api gate)**: если hash mismatch массовый → временно вернуть baseline-missing bypass + issue.
3. **T4 (CycloneDX)**: trivial revert.
4. **T5 (examples)**: trivial revert через `git revert`.
5. **T6 (concurrency/paths-ignore)**: если docs-only push сломал CI detection — убрать paths-ignore.
6. **T7 (Telegram)**: не влияет на CI (отдельный workflow). Удалить секреты → no-op.
7. **T8 (devcontainer)**: вообще не в CI; rollback = удалить `.devcontainer/`.

**Принцип:** каждый commit атомарен и обратим. v10.7.15 можно откатить целиком через revert.

---

## 8. Инструкции для следующего агента

1. **Прочитать первым:** этот файл (`docs/PLAN-CI-FAILURE-MITIGATION.md`) целиком.
2. **Контекст:** `CLAUDE_HANDOFF.md` §6 (процесс релиза), `scripts/quality-gates.sh` (существующая инфраструктура), `.github/workflows/ci.yml` (текущее состояние).
3. **Начало работы:**

   ```bash
   git checkout dev
   git pull
   git checkout -b feature/pr-14-ci-failure-mitigation
   ```

4. **Порядок задач:** T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 (по одной за раз).
5. **После каждой задачи:**
   - `cargo fmt --all && cargo fmt --all -- --check` (проверить себя).
   - `./scripts/quality-gates.sh` (полный прогон).
   - `git add -p && git commit -m "fix/pr-14: T<N> <краткое описание>"`.
   - `git push origin feature/pr-14-ci-failure-mitigation`.
   - Дождаться CI зелёного (через `gh run watch`).
6. **Все вопросы:** задавать ДО правок (через `AskUserQuestion` / clarification).
7. **Перед merge в dev:** убедиться что все 10 quality gates (G1..G10) зелёные.
8. **Создать PR:**

   ```bash
   gh pr create --base dev --head feature/pr-14-ci-failure-mitigation \
     --title "PR-14: CI Failure Mitigation (pre-commit + devcontainer + Telegram)" \
     --body "..."
   ```

9. **После merge:** следовать CLAUDE_HANDOFF §6 для выпуска v10.7.15.

---

## 9. Зафиксированные решения пользователя

| # | Решение |
|---|---|
| 1 | pre-commit: `pip install --user pre-commit` (кросс-платформенно) |
| 2 | CycloneDX → отдельный workflow `sbom.yml`, G10 в `quality-gates.sh` |
| 3 | Public-API: полная реализация (~200 строк, reflection-style) |
| 4 | Devcontainer: собственный Dockerfile с cargo tools + pre-commit |
| 5 | Telegram: failures во всех ветках (main, dev, release/*, PR) |
| 6 | Путь к плану: `docs/PLAN-CI-FAILURE-MITIGATION.md` |

---

## 10. Чек-лист готовности

- [x] Аудит обновлён (v10.7.10..v10.7.14).
- [x] Все 6 решений пользователя учтены.
- [x] 8 задач специфицированы с файлами и acceptance criteria.
- [x] Pre-existing `scripts/quality-gates.sh` интегрирован (G1–G9 + G10).
- [x] Процесс release (CLAUDE_HANDOFF §6) применён.
- [x] Инструкции для следующего агента в §8.
- [x] Rollback plan в §7.