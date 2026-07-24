# AGENTS.md — единый источник правил для AI-агентов

> **Single source of truth** для операционных правил всех AI-агентов (Claude Code,
> opencode, sub-agents) в `pharmacolog/syslog-generator`. Этот файл заменяет
> `docs/COORDINATION.md` (устарел) и дополняет `CLAUDE_HANDOFF.md` (только project
> context и release process).
>
> **Версия документа**: 2.0 (2026-07-24). Все операционные правила, lock state
> machine, file ownership, quality gates — здесь. При любых расхождениях
> с `CLAUDE_HANDOFF.md` или `docs/COORDINATION.md` — следовать этому файлу.
>
> **Cross-references:**
> - [CLAUDE_HANDOFF.md](CLAUDE_HANDOFF.md) — project context, release process, Git Flow §0.5.
> - [docs/COORDINATION.md](docs/COORDINATION.md) — **deprecated since 2026-07-24**, исторический changelog.
> - Issue #113 (`[META] Multi-agent coordination + standups`) — hub для standup-комментариев.

## Оглавление

1. [Язык и стиль общения](#1-язык-и-стиль-общения)
2. [GitHub auth bootstrap](#2-github-auth-bootstrap)
3. [Глоссарий и терминология](#3-глоссарий-и-терминология)
4. [Git Flow: иерархия веток и merge policy](#4-git-flow-иерархия-веток-и-merge-policy)
5. [Lock state machine (однозначная)](#5-lock-state-machine-однозначная)
6. [Pre-claim checks](#6-pre-claim-checks)
7. [Issue Status Transitions и определение `Done`](#7-issue-status-transitions-и-определение-done)
8. [Test Coverage Requirement (S11/P9)](#8-test-coverage-requirement-s11p9)
9. [File Ownership matrix](#9-file-ownership-matrix)
10. [Pre-PR Gate-check (S6)](#10-pre-pr-gate-check-s6)
11. [Worktree-based isolation (S1)](#11-worktree-based-isolation-s1)
12. [Sub-agent Dispatch Protocol (S7)](#12-sub-agent-dispatch-protocol-s7)
13. [Conflict Resolution Protocol (S8)](#13-conflict-resolution-protocol-s8)
14. [Deferral Tracking (S10)](#14-deferral-tracking-s10)
15. [Синхронизация состояния с GitHub Projects (ОБЯЗАТЕЛЬНО)](#15-синхронизация-состояния-с-github-projects-обязательно)
16. [Communication Protocol (S5)](#16-communication-protocol-s5)
17. [Coordination Cadence](#17-coordination-cadence)
18. [Анти-паттерны (запрещено)](#18-анти-паттерны-запрещено)

---

## 1. Язык и стиль общения

- **Все ответы и рассуждения — на русском языке.**
- Факты, не домыслы: проверять реальной компиляцией (`cargo build/test/clippy`)
  перед заявлением результата.
- При выпуске версий обязательно обновлять: `README.md`, `CHANGELOG.md`,
  `AUDIT.md`, `CLAUDE_HANDOFF.md`, `examples/`. Каждая веха завершается
  compile-verified релизом.

## 2. GitHub auth bootstrap

**Перед любым действием с GitHub** (issue, PR, project, workflow) — проверить аутентификацию.

### 2.1 Проверка текущего состояния

```bash
gh auth status
# Ожидаемый вывод:
#   ✓ Logged in to github.com account <login>
#   ✓ Git operations for https://github.com/<owner>/<repo>
#   ✓ Token: gho_***
#   - Active account: true
```

Если `Active account: false` или `Not logged in` — нужен bootstrap.

### 2.2 Bootstrap новой сессии

```bash
# Вариант A: GitHub MCP (если сессия поддерживает)
#   1. Активировать GitHub MCP tool в начале сессии.
#   2. Вызвать `get_me` для verify подключения.
#   3. Если `get_me` возвращает ошибку `Bad credentials` — переключиться на PAT-логин.

# Вариант B: gh CLI с device flow (если MCP недоступен)
gh auth login --device --scopes "gist,project,read:org,repo,workflow"
# Скопировать код из вывода, перейти по URL https://github.com/login/device,
# ввести код, подтвердить scopes.

# Вариант C: gh CLI с PAT (если есть)
echo "<github_pat>" | gh auth login --with-token
# PAT должен иметь scopes: gist, project, read:org, repo, workflow.
```

### 2.3 Verify после bootstrap

```bash
gh auth status && gh api user --jq '.login' && gh project list --owner <org> --limit 1
```

Если любой из этих шагов fails — **СТОП**. Не делать вид, что auth работает;
документировать блокер в issue #113 и попросить maintainer help.

### 2.4 Graceful degradation

Если `gh` или GitHub MCP недоступны, агент **не**:
- Не создаёт PR, не делает commit с lock-заявкой, не claim'ит issue.
- Не комментирует issue.
- Объясняет пользователю, что для coordination требуется auth,
  и предлагает выполнить actions вручную (paste команды).

## 3. Глоссарий и терминология

Следующие термины — **синонимы** для обсуждения roadmap:

| Термин | Значение | GitHub label | Пример |
|---|---|---|---|
| **milestone** | GitHub milestone vX.Y.Z | `milestone-vX.Y` | `milestone-v11.6` |
| **sprint** | Период работы над issues в milestone | (= milestone) | "sprint v11.6" |
| **release** | Конкретный patch/minor vX.Y.Z | (= milestone) | "release v11.6.0" |
| **release-train** | Серия release'ов (например, v10.7.0→v10.7.19) | (= milestone series) | "release-train v10.7" |
| **веха** | Высокоуровневая цель (A, B, C, D, E, F, G, H) | `track-X` | "веха G" |

**Правила:**
- Issue → `milestone: vX.Y.Z` (всегда конкретная версия, не "TBD" или "Sprint 1").
- В комментариях и standup используется "sprint v11.6" (естественнее).
- В `api-snapshot.txt` / `Cargo.toml` / `CHANGELOG.md` — `vX.Y.Z` (semver).
- В `Project V2` поле `Release Confidence` отражает состояние milestone.

## 4. Git Flow: иерархия веток и merge policy

### 4.1 Иерархия

| Branch | Назначение | Кто мерджит | Способ | Защита |
|---|---|---|---|---|
| **`main`** | Стабильный релизный код | Maintainer (с review) | Только PR | 7 checks + 1 review + linear |
| **`dev`** | Интеграционная ветка. Всегда зелёная. | Maintainer | PR | 7 checks, no review |
| `feature/*`, `fix/*` | Новые фичи/фиксы | Author + Maintainer | PR → dev | (нет protection) |
| `release/vX.Y.Z` | Подготовка релиза | Maintainer | PR → main | (нет protection) |
| `docs/*`, `chore/*` | Координация, docs | Maintainer | PR → main (см. §4.3) | (нет protection) |

### 4.2 Merge strategy

**`gh pr merge <N> --squash --admin` — единственный корректный способ merge.**

- `--squash` обязателен (для traceability: один commit per issue в `dev`,
  один commit per release в `main`).
- `--admin` — bypass branch protection для maintainer'ов (solo-maintainer
  policy: PR-19/22, см. `CLAUDE_HANDOFF.md §0`).
- `--delete-branch` — после успешного merge feature-ветка удаляется автоматически.

**Устаревшее правило (❌ больше НЕ действует):**
> "Squash merge для dev запрещён, оставляем merge commits для traceability"
> (`CLAUDE_HANDOFF.md §0.5` первоначально, v2026-07-23).

Это правило отменено: накопление merge-коммитов в `dev` ухудшает readability
и затрудняет bisect. Squash-merge в `dev` сохраняет информацию через PR body
и closing commits — это достаточный audit trail.

### 4.3 Исключения: координационные документы

Следующие файлы **могут мержиться напрямую в `main`** через PR, минуя `dev`:

- `AGENTS.md`
- `CLAUDE_HANDOFF.md`
- `docs/COORDINATION.md` (deprecated, см. §18)
- `CHANGELOG.md` (только entry-style updates)
- `.github/CODEOWNERS`
- `LICENSE`, `README.md` (только структурные изменения)

**Почему исключение:** координационные docs не запускают CI (нет `cargo test`,
нет `cargo build`); они не требуют интеграционного периода в `dev`. Их merge в
`main` сразу делает их доступными всем агентам через `git pull`.

**Правило:** даже для координационных docs создаётся PR, проходит review,
и merge делается с `--squash` (как и для feature-веток).

### 4.4 Запрещено (без исключений)

- ❌ `git push origin main` — branch protection блокирует.
- ❌ `git push origin dev` напрямую (sync через auto-sync workflow `sync-main-to-dev.yml`).
- ❌ Force push в `main`/`dev`/`release/*`.
- ❌ Merge PR с красными CI (strict mode enforced).
- ❌ Merge без review для `main` (1 approval required).
- ❌ Локальный `git merge origin/main && git push origin dev` (используется auto-sync workflow).

## 5. Lock state machine (однозначная)

Agent Operations (#4) карточки имеют поле **`Lock State`** с 7 значениями.
Это **state machine** с явными переходами:

```
                          ┌─── review ────┐
                          │   (PR open)   │
                          ▼               │
   free ──claim──▶ claimed ──push──▶ active ──pr create──▶ review ──merge──▶ released
                          │           │                       │              │
                          │           └───Blocker──▶ blocked  │              │
                          │               set         │       │              │
                          │                          │       │              │
                          │                          ▼       │              │
                          │                        stale ──heartbeat──▶ active
                          │                          │                  │
                          │                          │                  ▼
                          │                          └───24h no contact──▶ archive
                          │                                                (off-board)
                          │
                          └───Owner change / agent stop──▶ released
```

### 5.1 Состояния и определения

| Состояние | Owner поля карточки | Определение | Когда устанавливается |
|---|---|---|---|
| `free` | `Unassigned` / `Maintainer` | Никто не работает, agent свободно может claim | Default. После archive agent-card. |
| `claimed` | `Agent-X` (новый) | Агент заявил intent, но ещё нет local changes | Сразу после `gh issue comment "...starting work"` + `gh issue edit --add-assignee`. До первого `git commit`. |
| `active` | `Agent-X` | Есть локальные изменения / push в feature-ветку | При первом `git commit` ИЛИ `git push` в feature-ветку этой сессии. |
| `review` | `Agent-X` (PR open) | Открыт PR, ожидается merge | Сразу после `gh pr create` (любой state). |
| `blocked` | `Agent-X` | Внешняя блокировка (другой PR, ОК другого maintainer'а, etc.) | Когда `Blocked By` поле непустое. `Heartbeat` обновляется ежедневно. |
| `stale` | `Agent-X` | Heartbeat > 24h без push/commit/PR-comment | Auto-detected cron'ом (TBD) ИЛИ вручную при обнаружении. |
| `released` | `Agent-X → Maintainer` | Работа завершена: branch deleted, handoff published | После branch delete и standup в #113. |

### 5.2 Transition rules (явные)

| From | To | Триггер | Кто выполняет | Когда обновлять |
|---|---|---|---|---|
| `free` | `claimed` | `gh issue comment "...starting work"` + `--add-assignee` | Агент | В момент claim. |
| `claimed` | `active` | Первый `git commit` ИЛИ `git push` в feature-ветке | Агент | Сразу после commit/push. |
| `claimed` | `released` | Агент решает не работать (например, не прошёл pre-claim check) | Агент | Сразу. |
| `active` | `review` | `gh pr create` | Агент | Сразу. |
| `active` | `blocked` | Установка `Blocked By` (текст) | Агент | В момент обнаружения блокировки. |
| `active` | `released` | `git push --delete` feature-ветки, branch merged | Агент | Сразу после merge. |
| `active` | `stale` | Heartbeat > 24h | Auto-detected ИЛИ вручную | При обнаружении. |
| `review` | `active` | PR закрыт БЕЗ merge (closed, not merged) — возврат к работе | Агент | При re-claim. |
| `review` | `released` | PR merged (squash) | Агент ИЛИ auto-detected | После merge. |
| `blocked` | `active` | Блокировка снята (external PR merged, maintainer responded) | Агент | Сразу. |
| `stale` | `active` | Heartbeat обновлён (push, comment, push force, etc.) | Агент | При возобновлении работы. |
| `stale` | `released` | Heartbeat > 7d без активности, OR agent явно передаёт работу | Auto-detected ИЛИ вручную | При передаче. |
| `released` | (gone) | `archiveProjectV2Item` после merge всех связанных PR | Агент | После branch delete + handoff. |

### 5.3 Heartbeat и stale detection

**Heartbeat** — поле `date` в Project #4, обновляется при любом push, commit, comment, sync.
- Формат: `YYYY-MM-DD` (date, не datetime).
- Обновляется **автоматически** (через `gh api graphql updateProjectV2ItemFieldValue`) при каждом из событий: `git commit`, `git push`, `gh issue comment`, `gh pr create`, `gh pr comment`.
- `stale` определяется как: `now - Heartbeat > 24h` (без учёта часового пояса; сравнение по date).

**Auto-detection `stale`:** планируется добавить cron workflow
`.github/workflows/agent-ops-stale-detector.yml` (post-AGENTS.md v2.0). Пока
`stale` детектируется вручную при board-reconciliation.

## 6. Pre-claim checks

**Перед началом работы над issue** (обязательно):

```bash
# 1. Прочитать issue и проверить assignees/labels/lockState
gh issue view <N> --repo pharmacolog/syslog-generator --json number,title,state,assignees,labels,body

# 2. Проверить Project #2 Scrum карточку
gh project item-list 2 --owner pharmacolog --limit 100 --format json | \
  jq '.items[] | select(.content.number == <N>) | {status, owner, lockState}'

# 3. Проверить Agent Operations #4 — есть ли уже активная карточка на этот issue
gh project item-list 4 --owner pharmacolog --limit 100 --format json | \
  jq '.items[] | select(.title | test("<issue# or keyword>")) | {status, lockState, agent}'

# 4. Проверить file scope через CODEOWNERS + recent commits
git fetch origin && git log --oneline --all -10 -- <paths/under/scope>

# 5. Синхронизировать worktree
git fetch origin && git switch -c "feature/<branch>" origin/dev
```

**Если на любом шаге обнаружено расхождение** (assignee, lockState=active, file
conflict) — **не брать задачу**. Создать comment в #113:

> "🤖 Agent-X: issue #N занят (assignee=Y, lockState=active, file scope
>  conflict with PR #M). Ищу другую задачу."

## 7. Issue Status Transitions и определение `Done`

### 7.1 Status transitions (Issue ↔ Project V2)

| Issue status (GitHub) | Project V2 status (scrum #2) | Триггер |
|---|---|---|
| open (только что создан) | `Backlog` | issue created |
| open + assignee | `Ready` | owner assigned + branch created |
| open + in progress | `In Progress` | первый commit/push |
| open + PR open | `In Review` | PR opened |
| open + blocked | `Blocked` | `Blocked By` set |
| **closed** | **`Done`** | issue closed (см. §7.2 для условий) |

### 7.2 Определение `Done` для issue

**Hard rule: issue переходит в `Done` (`Status: Done` в Project V2) ТОЛЬКО когда выполнены ВСЕ условия:**

1. ✅ PR merged в `dev` (squash, через `gh pr merge --squash`).
2. ✅ CI green на `dev` (все 7 blocking required checks + CodeQL + N7 + public-api + advisory-expiry).
3. ✅ `main` обновлён через merge из `dev` (auto-sync workflow `sync-main-to-dev.yml`).
4. ✅ CI green на `main` (повторный полный run).
5. ✅ Документация обновлена (если были doc-изменения: `README.md`, `CHANGELOG.md`, `CLAUDE_HANDOFF.md`, `AUDIT.md`).
6. ✅ Закрывающий комментарий опубликован (с status + limitations + mitigations + links на merged PR).

**Issue #117 исключение:** `#[ignore]` для known-flaky тестов с отдельным
nightly workflow. Issue остаётся open до 3 consecutive nightly green runs,
потом закрывается.

### 7.3 Определение `Done` для milestone (sprint/release)

**Hard rule: milestone закрывается (`Milestone: closed`) ТОЛЬКО когда выполнены ВСЕ условия:**

1. ✅ **Все** issues milestone в state `closed` (т.е. каждая issue прошла §7.2).
2. ✅ **Все** issues `Done` синхронизированы в Project V2.
3. ✅ `release/vX.Y.Z` branch создан от последнего merge commit в `main`.
4. ✅ `Cargo.toml` `version` bumped до `X.Y.Z` в release-branch.
5. ✅ `CHANGELOG.md` содержит полный release entry.
6. ✅ `git tag vX.Y.Z` создан и push'нут.
7. ✅ GitHub Release создан через `gh release create vX.Y.Z --generate-notes`.
8. ✅ Auto-sync `main → dev` сработал (workflow `sync-main-to-dev.yml`).

**После milestone closure:** `tag → push → GitHub Release` — это завершающая
операция, которая происходит **один раз** на milestone и **только** когда
все issues закрыты.

## 8. Test Coverage Requirement (S11/P9)

> **Полная версия**: [`docs/COORDINATION.md` §S11/P9](docs/COORDINATION.md#s11-test-coverage-requirement-p9--принцип-9).
> Скопирована сюда для single source of truth; см. COORDINATION.md для исторического changelog.

**Обязательство:** каждый code change в рамках выполняемой задачи ДОЛЖЕН
сопровождаться тестами в том же PR.

### 8.1 Правила по типу изменения

| Тип изменения | Минимальные требования к тестам |
|---|---|
| **New feature** | Positive tests (happy path) + negative tests (error paths) + edge cases |
| **Bug fix** | Regression test, который воспроизводит баг и проверяет фикс |
| **Refactor** | Existing tests must continue to pass; refactor should NOT decrease coverage |
| **Optimization** | Benchmark (если применимо) + correctness tests (что оптимизация не сломала behavior) |
| **Documentation** | N/A (только если меняется doc-tests или example code) |

### 8.2 Конкретные требования

1. **Unit tests**: добавлять в `#[cfg(test)] mod tests` в конце файла, который меняется.
2. **Integration tests**: для cross-cutting changes — добавлять в `tests/integration_tests.rs`.
3. **Snapshot tests**: для output форматирования (форматы: rfc5424, rfc3164, cef, leef, json_lines, protobuf).
4. **Property-based tests (proptest)**: для сложных invariants.
5. **Test naming**: `fn test_<feature>_<scenario>` (e.g., `sanitize_header_ascii_fast_path`).
6. **Coverage gate**: `cargo test --release --lib` должен проходить; coverage не должна падать ниже текущего уровня (~94.03%).
7. **TDD preference**: для новых фич — писать тест СНАЧАЛА, потом реализацию.

### 8.3 Что запрещено

- ❌ PR без тестов для нового кода.
- ❌ "TODO: add tests later" в PR body.
- ❌ Удаление существующих тестов без обоснования в PR body.
- ❌ Пропуск flaky тестов через `#[ignore]` без создания linked issue (см. §14).

### 8.4 Acceptance для PR

PR блокируется merge если:
- Новый public API без unit тестов.
- Bug fix без regression test.
- Coverage delta < 0% (coverage упала).

## 9. File Ownership matrix

| Файл / модуль | Owner / режим | Issue / Branch | Lock |
|---|---|---|---|
| `src/plan/*` | **Agent-1** (Issue #88 lock) | #88 — `feature/compiled-plan` | active until A2.6 merged |
| `src/generator/core.rs` | **shared** (#85, #88, #86 — требует sync) | `feature/perf-*` | pre-claim обязателен |
| `src/transport/tls.rs` | **shared** (#85, #82, F16) | `feature/tls-*` | pre-claim обязателен |
| `src/transport/tcp.rs` | **shared** (#82) | `feature/tcp-*` | pre-claim обязателен |
| `src/format/*` | **shared** (Agent-2 read-only в #85) | `feature/format-*` | pre-claim обязателен |
| `src/cli.rs` | **Agent-1** (последовательно) | #93, #92, #83 — `feature/cli-*` | released after each |
| `src/payload.rs` | **Agent-2** (в #85 sub-task 6) | `feature/payload-*` | released |
| `src/observer/*` | **shared** | TBD | pre-claim |
| `src/anomaly.rs` | **shared** | TBD | pre-claim |
| `Cargo.toml` | **serialized** (только один merge за раз) | — | merge order обязателен |
| `.github/workflows/*.yml` | **shared** | `feature/ci-*` | pre-claim |
| `docs/COORDINATION.md` | **deprecated since 2026-07-24** (см. AGENTS.md) | — | frozen |
| `CLAUDE_HANDOFF.md` | **maintainer-controlled** (cross-refs в AGENTS.md) | — | maintainer only |
| `AGENTS.md` | **single source of truth** (AI-agents controlled) | — | free (any agent) |

**Shared файлы** требуют координации: кто открыл PR первым, тот и ведёт merge
(см. §13 Conflict Resolution).

## 10. Pre-PR Gate-check (S6)

**Перед `git push` (или `gh pr create`):**

```bash
bash scripts/pre-pr-check.sh [--skip-build] [--skip-kafka]
```

Полный набор gates (11 проверок):

1. Format (`cargo fmt --all --check`)
2. Clippy default (`cargo clippy --all-targets -- -D warnings`)
3. Clippy + kafka (`cargo clippy --all-targets --features kafka -- -D warnings`)
4. Tests (`cargo test --release --lib`)
5. Build (`cargo build --release --locked`)
6. Doc (`RUSTDOCFLAGS=-D warnings cargo doc --no-deps`)
7. Public API (`cargo public-api --features test-helpers` diff against `api-snapshot.txt`)
8. N7 invariant (`scripts/check-n7-invariant.sh` — no `.unwrap()/.expect()` в non-test runtime)
9. Cargo-deny (`cargo deny check`)
10. Advisory ignore expiry (`scripts/check-advisory-expiry.sh` — structured reason + future expiry)
11. Cargo-machete (`cargo machete` — no unused dependencies)

**Дополнительно (перед commit):**

```bash
actionlint .github/workflows/*.yml  # YAML lint (если меняли workflow)
python3 -c "yaml.safe_load(...)"   # quick YAML parse check
bash -n scripts/*.sh               # shell syntax check
```

**Предупреждения vs ошибки:**

- `cargo-deny` показывает warnings (`duplicate entries`, `no-license-field`,
  `advisory-not-detected`) — это **нормально** при exit code 0. Warnings не
  блокируют merge. Только errors (exit code != 0) блокируют.
- `cargo-machete` может показывать false-positive — сверяйся с Cargo.toml
  (binary crates, build-dependencies не всегда детектируются).

**Coverage gate** (в CI, не в pre-pr-check): `cargo llvm-cov --fail-under-lines=97`
для Tier 1 модулей (см. `docs/COVERAGE.md`).

## 11. Worktree-based isolation (S1)

**Каждый агент работает в отдельном worktree** от своего base-ветки:

```bash
# Создание worktree
git worktree add -b "feature/<name>" "/path/to/wt" origin/<base>
# Пример: git worktree add -b "feature/ci-hardening" "/path/to/wt" origin/dev
```

**Правила:**

- Worktree создаётся от `origin/main` или `origin/dev` через `git worktree add -b feature/<name> /path/to/wt origin/<base>`.
- Base branch — отдельная feature-ветка (НЕ main, чтобы не было конфликтов на master).
- Каждый worktree имеет свой `Cargo.lock`, `target/`, `.git/index` — независимые.
- **`git checkout` между worktrees запрещён** (переключение через `cd`). Потеря unstaged work (P4) — это известная проблема, избегать.
- Worktree удаляется **только после** branch merge и `git fetch --prune` для cleanup.

## 12. Sub-agent Dispatch Protocol (S7)

При работе над sub-task, который можно делегировать:

1. **Скоупинг**: явно указать файлы, sub-task номер, expected output.
2. **Изоляция**: создать отдельный worktree ИЛИ новую ветку.
3. **Контракт**: sub-agent возвращает список изменённых файлов + commit hash + ready-to-push branch.
4. **Verification**: проверить через pre-PR gate-check перед push.

**Пример диспатча:**

```
Task: Implement Issue #85 sub-task 9 (Arc<ClientConfig> for TLS)
Worktree: /Users/anton/svn/github/syslog-generator-agent2 (already exists)
Branch: feature/a1-remaining
Files to modify:
  - src/transport/tls.rs (only)
Tests:
  - src/transport/tls.rs::tests::...
Expected: pre-PR gate-check passes
```

## 13. Conflict Resolution Protocol (S8)

**При merge conflict (двух PR в одни файлы):**

1. **First-to-PR wins**: кто открыл PR раньше → тот merge первым.
2. **Assignee priority**: если оба assignee → приоритет у того, чей sub-task стартует раньше.
3. **Rebase + retry**: второй агент делает `git fetch origin/dev && git rebase origin/dev && git push --force-with-lease`.
4. **Last resort — maintainer arbitration**: maintainer принимает решение через issue comment.

**Hard rule:** НИКОГДА не force-push в `main`/`dev`/`release/*`. Только в
feature-ветки (с `--force-with-lease`).

## 14. Deferral Tracking (S10)

**Если sub-task не может быть сделан в текущем PR:**

1. **Создать linked issue** с labels `deferred-from:<parent-issue>`.
2. **Reference в closing comment** parent issue.
3. **Reference в roadmap** (`docs/ROADMAP.md`) или в Project V2 description.

**Запрещено**: писать "TODO: implement later" без отдельного tracking issue.

**`#[ignore]` для тестов**: требует linked issue (пример: Issue #117 для
TLS stress tests). Без linked issue — `#[ignore]` не допустим.

## 15. Синхронизация состояния с GitHub Projects (ОБЯЗАТЕЛЬНО)

**Принцип: при любом изменении состояния задачи немедленно актуализируй её на досках.**

GitHub Projects v2 — единый source of truth для статусов, owner'ов, locks и handoff.

### 15.1 Когда обновлять

| Событие | Что обновлять |
|---|---|
| `gh issue edit`, `gh pr create/close/merge` | статус карточек в Project #1, #2 |
| Claim/lock в `Agent Operations` (#4) | создать карточку со `Sync State: Current` + `Lock State: claimed/active` |
| Sync ветки с `origin/main`/`origin/dev` | `Sync State: Current` (если up-to-date) или `Behind` |
| Push в feature-ветку | `Branch` в карточке Project #2; `Heartbeat` в Project #4 |
| Heartbeat / blocker | `Heartbeat` (date), `Blocked By` (текст) |
| PR open | в Project #2 добавить карточку `status=In Review`, `CI State=Running` |
| PR merged | `status=Done`, `owner=Maintainer`, `Lock State=released`, `CI State=Green` |
| Issue closed | `status=Done`, освободить `Lock State`, снять `lock:claimed` |
| Issue transfer / superseded | `status=Done` или оставить, добавить link в handoff |
| Stale карточка (agent-сессия ended) | `archiveProjectV2Item` для карточки |
| Branch deleted / worktree removed | очистить `Branch` поле карточки; `Sync State: Current` |
| Stale detection (Heartbeat > 24h) | `Lock State: stale`, archive if > 7d |

### 15.2 Как обновлять

- **Быстрые правки** — `gh api graphql` с `updateProjectV2ItemFieldValue`.
- **Массовые правки** — для серии однотипных карточек формируй GraphQL `mutation`
  с переменными `$item`, `$f`, `$o` (для `singleSelectOptionId`) / `$t` (для `text`).
- **archiveProjectV2Item** — для stale карточек; карточка исчезает из `item-list`.

### 15.3 Поля проектов

- **Project #1 (Roadmap)**: `Status`, `Critical Path`, `Risk Gate`, `Release Confidence`,
  `Priority`, `Track`, `Effort`, `Risk`, `File Scope`, `Dependency Chain`.
- **Project #2 (Scrum Delivery)**: `Status`, `Owner`, `Lock State`, `CI State`,
  `Review SLA`, `Sub-agent State`, `Branch`, `Worktree`, `Started`, `Heartbeat`,
  `Blocked By`, `Handoff Summary`, `Sprint`, `Sprint Goal`.
- **Agent Operations (#4)**: `Status` (Backlog/Claimed/Active/Handoff/In Review/Blocked/Released),
  `Agent`, `Issue or PR`, `Branch`, `Worktree`, `File Scope`, `Lock State`
  (free/claimed/active/review/stale/released), `Sync State`, `Blocked By`,
  `Heartbeat`, `Handoff Summary`.

### 15.4 Перед завершением сессии — reconciliation

1. Сверить карточку `Agent Operations` со своим фактическим состоянием:
   - `Sync State = Current`?
   - `Lock State = released` или `active` (если передаёшь другому агенту)?
   - `Heartbeat = сегодня`?
   - `Blocked By` пуст или содержит актуальную блокировку?
2. Если ты закрыл issue / merge'нул PR — обновить `Project #1`/`Project #2`:
   - `Status=Done`, `Critical Path=Completed` (если был на critical path),
     `Risk Gate=Green`, `Release Confidence=Complete`.
3. Если карточка твоей сессии в `Agent Operations` стала неактуальной —
   `archiveProjectV2Item` или `status=Released`.
4. Опубликовать standup в issue #113 с финальным состоянием.

## 16. Communication Protocol (S5)

### 16.1 Daily standup (через issue)

Каждый день агент публикует standup comment в `coordination` issue (#113):

```text
🤖 Agent-X standup [YYYY-MM-DD]
✅ Yesterday: PR #N merged (Issue #M)
🔄 Today: working on Issue #K sub-tasks 1-5
📋 Blockers: file conflict with Agent-Y on src/cli.rs
🚦 Status updates: Project V2 #M → Done
```

### 16.2 Claim comment

Перед началом работы:

```bash
gh issue comment <N> --body "🤖 Agent-X starting work on this issue"
gh issue edit <N> --add-assignee <login>
```

### 16.3 Conflict notification

При обнаружении blocker:

```bash
gh issue comment <N> --body "🤖 Agent-X: blocked on Agent-Y's PR #M. Waiting for merge before proceeding."
```

## 17. Coordination Cadence

- **Daily**: standup comment в #113.
- **Weekly**: retro — что блокировало, что улучшить.
- **Per-PR**: pre-PR gate-check + post-merge status comment.
- **Per-release**: maintainer review всех open issues → re-prioritize → milestone closure (§7.3).

## 18. Анти-паттерны (запрещено)

### 18.1 Board sync

- ❌ Закрыть issue в GitHub, не обновив `Status` в Project boards.
- ❌ Merge'нуть PR, оставив `Lock State=active` или `owner=Agent-X` без передачи.
- ❌ Удалить worktree/ветку, оставив в карточке `Branch=<deleted>` или `Sync State=Behind`.
- ❌ Игнорировать `lock:claimed` label после закрытия issue.
- ❌ Держать `Status=In Progress` для карточки, чей `Lock State=released`.
- ❌ Оставлять `Status=Ready/Backlog` для closed issues.

### 18.2 Git Flow

- ❌ `git push origin main` — branch protection блокирует.
- ❌ `git push origin dev` напрямую (sync через auto-sync workflow).
- ❌ Force push в `main`/`dev`/`release/*`.
- ❌ Merge PR с красными CI (strict mode enforced).
- ❌ Merge без review для `main` (1 approval required).
- ❌ Локальный `git merge origin/main && git push origin dev`.

### 18.3 Worktree

- ❌ `git checkout` для переключения между worktrees (использовать `cd`).
- ❌ Потеря unstaged work (P4): всегда commit перед `cd` или `git stash`.

### 18.4 Code quality

- ❌ Force push в любую защищённую ветку.
- ❌ Push без локального pre-PR gate-check.
- ❌ PR без тестов для нового кода (см. §8).
- ❌ "TODO: implement later" без linked issue (см. §14).
- ❌ "TODO: add tests later" в PR body.
- ❌ Удаление существующих тестов без обоснования в PR body.
- ❌ `#[ignore]` на flaky тестах без linked tracking issue.

### 18.5 Coordination

- ❌ Браться за issue, который уже assigned другому агенту без согласования.
- ❌ Писать "TODO: implement later" без linked issue.
- ❌ Закрывать issue без verification по коду.
- ❌ Закрывать issue без обновления документации.
- ❌ Менять Project V2 напрямую через UI (только через `gh api graphql`).
- ❌ Создавать branch без Issue/PR связи.

### 18.6 Auth

- ❌ Делать вид, что GitHub auth работает, если `gh auth status` fails.
- ❌ Пропускать board updates из-за "проблем с auth" — лучше документировать блокер.

---

## История изменений

### v2.0 (2026-07-24) — Single source of truth
- Объявлен **single source of truth** для AI-агентов.
- **Lock state machine** расписана с явными transition rules (13 переходов).
- **Issue Status Transitions** с явным `Done` определением.
- **Milestone closure flow** (tag → push → GitHub Release) добавлен.
- **Test Coverage Requirement (S11/P9)** скопирован из `docs/COORDINATION.md`.
- **GitHub auth bootstrap** секция добавлена (graceful degradation).
- **Merge strategy** unified: `--squash` обязателен, устаревшее правило отменено.
- **Coordination docs flow**: могут мержиться в `main` напрямую через PR.
- **Terminology**: milestone/sprint/release/release-train — синонимы.
- **Cargo-deny warnings** explicitly OK (норма при exit code 0).
- **`scripts/coordinate.sh`**: TODO помечен, описание сохранено для reference.
- Cross-references в `CLAUDE_HANDOFF.md §0` и `docs/COORDINATION.md`.

### v1.0 (2026-07-23) — initial
- Базовый AGENTS.md с board sync principle, lock state, file ownership.
- См. [PR #146](https://github.com/pharmacolog/syslog-generator/pull/146).
