# Координация нескольких агентов в `syslog-generator`

> **⚠️ DEPRECATED since 2026-07-24 (AGENTS.md v2.0)**
>
> Этот документ **заменён** [`AGENTS.md`](../AGENTS.md) — single source of truth
> для AI-агентов. Все правила из §S1–S11 ниже были перенесены и унифицированы
> в AGENTS.md. Содержимое ниже сохранено **исключительно как исторический
> changelog**. **Не редактируйте** §S1–S11 напрямую — любые изменения
> координации вносятся в `AGENTS.md`, а в этот файл попадают только
> исторические ссылки.
>
> **Cross-references:**
> - [`AGENTS.md`](../AGENTS.md) — single source of truth (lock state, file ownership, gates).
> - [Issue #113](https://github.com/pharmacolog/syslog-generator/issues/113) — coordination hub.
>
> ---
>
> **Original headers (pre-AGENTS.md v2.0):**
>
> > **Дата создания**: 2026-07-23
> > **Автор**: AI-агент (pharmacolog/syslog-generator-agent2)
> > **Статус**: Рекомендации (draft v1)

## 1. Контекст

В репозитории `pharmacolog/syslog-generator` параллельно работают несколько AI-агентов:

- **Maintainer-agent** (`feature/perf-a0-baseline`) — Issue #87 Performance baseline
- **Auxiliary-agent** (`feature/a1-remaining`) — Issue #85 quick-wins + roadmap
- **Sub-agents** (временные) — диспатчатся на sub-tasks через `task` tool

Без формализованной координации возникают:
- Конфликты merge в одинаковых файлах (`src/cli.rs`, `src/generator/core.rs`, `Cargo.toml`)
- Дублирование работы (два агента делают один sub-task)
- Race conditions на Project V2 (оба обновляют статус одного issue)
- Потеря контекста между сессиями

## 2. Обнаруженные проблемы (из реальной работы)

### P1. Конкуренция за файлы
**Симптом**: merge conflicts в `scripts/perf-baseline.sh`, `src/generator/core.rs`.
**Пример**: PR #110 (мой, Issue #85) и `feature/perf-a0-baseline` (#87 baseline) оба трогали `scripts/perf-baseline.sh`.

### P2. Не-явное владение issue
**Симптом**: parallel agent работал над #87 без явного assignment; я начал #85 без согласования.
**Последствие**: параллельные коммиты в одни файлы (например, `anomaly_proptests.rs`/`load_shape_proptests.rs` references в `src/lib.rs`).

### P3. Race conditions на Project V2
**Симптом**: workflow `project-v2-sync.yml` срабатывает на закрытие issue, но PR-merge может прилететь до закрытия issue → статус не обновляется автоматически.
**Пример**: Issue #85 merged через PR #110, но workflow не нашёл "Closes #N" в body (использовалось "Issue #85") → статус остался "In Review", пришлось обновлять вручную.

### P4. `git checkout` теряет unstaged work
**Симптом**: `git checkout feature/perf-a0-baseline` → `git checkout feature/perf-a1-quick-wins` → unstaged edit (clippy fix) потерян.
**Последствие**: пришлось переделать edit; CI red.

### P5. Нет pre-PR gate-check
**Симптом**: я пушил PR без запуска `cargo clippy --all-targets -- -D warnings` локально. CI поймал clippy error `manual_range_contains`. PR получил красный CI, пришлось делать fix-commit.
**Урок**: локальный CI gate ОБЯЗАТЕЛЕН перед push.

## 3. Решения

### S1. Worktree-based isolation
Каждый агент работает в **отдельном worktree** от своего base-ветки:

```
/Users/anton/svn/github/syslog-generator         ← Agent-1 (feature/perf-a0-baseline)
/Users/anton/svn/github/syslog-generator-agent2  ← Agent-2 (feature/a1-remaining)
```

Правила:
- Worktree создаётся от `origin/main` (или `origin/dev`) через `git worktree add -b feature/<name> /path/to/wt origin/<base>`
- Base branch для каждой worktree — отдельная feature-ветка (НЕ main, чтобы не было конфликтов на master)
- Каждый worktree имеет свой `Cargo.lock`, `target/` (build кэш), `.git/index` — независимые
- `git checkout` между worktrees запрещён (переключение через `cd`)

### S2. Issue Lock Protocol (Issue Assignment + Comment)
Перед началом работы над issue:

```bash
# 1. Прочитать issue
gh issue view 85 --repo pharmacolog/syslog-generator

# 2. Проверить assignees
gh issue view 85 --json assignees --repo pharmacolog/syslog-generator

# 3. Если никто не назначен:
gh issue comment 85 --body "🤖 Agent-X starting work on this issue" --repo pharmacolog/syslog-generator
gh issue edit 85 --add-assignee pharmacolog --repo pharmacolog/syslog-generator

# 4. Если уже назначен Agent-Y:
gh issue comment 85 --body "🤖 Agent-X acknowledging ownership to Agent-Y" --repo pharmacolog/syslog-generator
# Не трогать issue, искать другую задачу
```

**Обязательство**: после назначения — обновить Project V2 `Owner` поле (если есть).

### S3. File Ownership Matrix
| Файл / модуль | Owner | Issue / Branch |
|---|---|---|
| `src/cli.rs` | Agent-1 (последовательно) | #93, #92, #83 — `feature/cli-*` |
| `src/format/*.rs` | Agent-2 (Agent-1 read-only) | #85, F15 — `feature/format-*` |
| `src/transport/tls.rs` | Agent-2 (сейчас) | #85, #82, F16 — `feature/tls-*` |
| `src/transport/tcp.rs` | Agent-2 (сейчас) | #82 — `feature/tcp-*` |
| `src/generator/core.rs` | **⚠️ shared** | #85, #88, #86 — требуется merge order |
| `src/plan/` | Agent-1 (новый модуль) | #88 — `feature/compiled-plan` |
| `src/payload.rs` | Agent-2 | #85 sub-task 6 — `feature/payload-*` |
| `Cargo.toml` | **⚠️ serialized** | Только один PR одновременно |
| `.github/workflows/*.yml` | Agent-2 | CI/CD — `feature/ci-*` |
| `docs/**.md` | Agent-1 (DRY) | #91, MIGRATION — `feature/docs-*` |

**Shared файлы** требуют координации: кто открыл PR первым, тот и ведёт merge.

### S4. Project V2 как Single Source of Truth

#### Кастомные поля:
- **Owner** (single-select): `Agent-1`, `Agent-2`, `Maintainer`, `Unassigned`
- **Branch** (text): `feature/<name>`
- **PR** (number): `<#>`

#### Workflow:
1. Каждый агент обновляет **Owner** на себя при начале работы
2. Workflow `project-v2-sync.yml` (v1 уже есть) автоматически обновляет **Status** на lifecycle events
3. При merge PR — workflow ставит `Done`
4. Никто не редактирует Project V2 напрямую — только через gh CLI

### S5. Communication Protocol

#### Daily standup (через issue)
Каждый день агент публикует standup comment в `coordination` issue:

```
🤖 Agent-X standup [YYYY-MM-DD]
✅ Yesterday: PR #N merged (Issue #M)
🔄 Today: working on Issue #K sub-tasks 1-5
📋 Blockers: file conflict with Agent-Y on src/cli.rs
🚦 Status updates: Project V2 #M → "Done"
```

#### Ворк-flow для нового issue:
1. Создать issue (если нет) с labels (`track-*`, `priority-*`, `milestone-*`)
2. Закоммитить себя как owner
3. Начать работу в worktree
4. По завершении: PR + close issue + update docs

### S6. Pre-PR Gate-check (ОБЯЗАТЕЛЬНО)

Перед `git push`:

```bash
# 1. Format
cargo fmt --all -- --check

# 2. Clippy (default + kafka feature)
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features kafka -- -D warnings

# 3. Tests
cargo test --release --lib

# 4. Build
cargo build --release --locked

# 5. Doc
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# 6. Public API
cargo public-api --features test-helpers 2>/dev/null > /tmp/api.txt
diff /tmp/api.txt api-snapshot.txt  # должен быть 0 строк diff

# 7. N7 invariant
bash scripts/check-n7-invariant.sh

# 8. Deny + Machete
cargo deny check
cargo machete
```

**Все 8 проверок должны пройти локально ДО push**. CI — только финальная проверка.

### S7. Sub-agent Dispatch Protocol

При работе над sub-task, который можно делегировать:

1. **Скоупинг**: явно указать файлы, sub-task номер, expected output
2. **Изоляция**: создать отдельный worktree ИЛИ новую ветку
3. **Контракт**: sub-agent возвращает список изменённых файлов + commit hash + ready-to-push branch
4. **Verification**: проверить через pre-PR gate-check перед push

Пример диспатча:
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

### S8. Conflict Resolution Protocol

При merge conflict (двух PR в одни файлы):

1. **First-to-PR wins**: кто открыл PR раньше → тот merge первым
2. **Assignee priority**: если оба assignee → приоритет у того, чей sub-task стартует раньше
3. **Rebase + retry**: второй агент делает `git fetch origin/dev && git rebase origin/dev && git push --force-with-lease`
4. **Last resort — maintainer arbitration**: maintainer принимает решение через issue comment

**Hard rule**: НИКОГДА не force-push в `main`/`dev`/`release/*`. Только в feature-ветки.

### S9. Issue Status Transitions

| Состояние | Триггер | Действие |
|---|---|---|
| `Backlog` | issue создан | ничего |
| `Ready` | pre-conditions resolved | owner assigned, branch created |
| `In Progress` | первый commit | workflow project-v2-sync |
| `In Review` | PR opened | workflow project-v2-sync (regex matched) |
| `Done` | PR merged + dev green + main green | workflow project-v2-sync + comment |
| `Blocked` | external dependency | comment with reason |

**Hard rule**: Issue переходит в `Done` ТОЛЬКО когда:
1. PR merged в dev
2. CI green на dev
3. main обновлён через merge из dev
4. CI green на main
5. Документация обновлена
6. Закрывающий комментарий с status + limitations + mitigations

### S10. "Deferral" Tracking (принцип 6)

Если sub-task не может быть сделан в текущем PR:

1. **Создать linked issue** с labels `deferred-from:<parent-issue>`
2. **Reference в closing comment** parent issue
3. **Reference в roadmap** (`docs/ROADMAP.md`) или в Project V2 description

**Запрещено**: писать "TODO: implement later" без отдельного tracking issue.

### S11. Test Coverage Requirement (P9 — принцип 9)

**Обязательство**: каждый code change в рамках выполняемой задачи ДОЛЖЕН сопровождаться тестами в том же PR.

#### Правила

| Тип изменения | Минимальные требования к тестам |
|---|---|
| **New feature** | Positive tests (happy path) + negative tests (error paths) + edge cases |
| **Bug fix** | Regression test, который воспроизводит баг и проверяет фикс |
| **Refactor** | Existing tests must continue to pass; refactor should NOT decrease coverage |
| **Optimization** | Benchmark (если применимо) + correctness tests (что оптимизация не сломала behavior) |
| **Documentation** | N/A (только если меняется doc-tests или example code) |

#### Конкретные требования

1. **Unit tests**: добавлять в `#[cfg(test)] mod tests` в конце файла, который меняется
2. **Integration tests**: для cross-cutting changes — добавлять в `tests/integration_tests.rs`
3. **Snapshot tests**: для output форматирования (форматы: rfc5424, rfc3164, cef, leef, json_lines, protobuf)
4. **Property-based tests (proptest)**: для сложных invariants
5. **Test naming**: `fn test_<feature>_<scenario>` (e.g., `sanitize_header_ascii_fast_path`)
6. **Coverage gate**: `cargo test --release --lib` должен проходить; coverage не должна падать ниже текущего уровня (~93.86%)
7. **TDD preference**: для новых фич — писать тест СНАЧАЛА, потом реализацию

#### Что запрещено

- ❌ PR без тестов для нового кода
- ❌ "TODO: add tests later" в PR body
- ❌ Удаление существующих тестов без обоснования в PR body
- ❌ Пропуск flaky тестов через `#[ignore]` без создания linked issue

#### Acceptance для PR

PR блокируется merge если:
- Новый public API без unit тестов
- Bug fix без regression test
- Coverage delta < 0% (coverage упала)

#### Примеры для Issue #85 sub-tasks

| Sub-task | Обязательные тесты |
|---|---|
| #85 sub-task 10 (BytesMut capacity) | unit test для нового `calculate_capacity()` функции + integration test с большим `max_message_bytes` |
| #85 sub-task 7 (protobuf pre-resolve) | snapshot test что wire-format НЕ изменился + benchmark test |
| #85 sub-task 6 (regex HIR cache) | unit test на cache hit/miss + integration test на детерминизм |
| #85 sub-task 9 (TLS Arc<ClientConfig>) | unit test на lazy init + integration test mTLS round-trip |

## 4. Action Plan

### Phase 1 (немедленно)
- [x] Создать `docs/COORDINATION.md` (этот документ)
- [ ] Добавить "Owner" поле в Project V2 (через workflow)
- [ ] Настроить `.github/CODEOWNERS` (если файл отсутствует)
- [ ] Создать `coordination` issue для standup'ов

### Phase 2 (на этой неделе)
- [ ] Issue #85: завершить оставшиеся sub-tasks (1-7, 9-10, 13)
- [ ] Issue #85 → `Done` после merge в main
- [ ] Веха H: создать issues #94-#108 (уже созданы)
- [ ] Drive Issue #95 (distributed mode) до `Done`

### Phase 3 (на этой + следующей неделе)
- [ ] Issue #98 (Web UI) — пробный sub-agent dispatch
- [ ] Issue #106 (Whitepaper) — owner: GTM track
- [ ] Issue #107 (.deb/.rpm) — owner: GTM track

## 5. Coordination Cadence

- **Daily**: standup comment в `coordination` issue
- **Weekly**: retro — что блокировало, что улучшить
- **Per-PR**: pre-PR gate-check + post-merge status comment
- **Per-release**: maintainer review всех open issues → re-prioritize

## 6. Tools & Scripts

### Helper: `scripts/coordinate.sh` (TODO)
```bash
#!/bin/bash
# Helper для multi-agent coordination.

# Проверить assignments в open issues:
gh issue list --label "track-a" --json number,title,assignees

# Standup comment:
gh issue comment <coordination-issue> --body "🤖 Agent-X standup: ..."

# Lock issue (comment + assign):
gh issue comment <issue> --body "🤖 Agent-X starting work"
gh issue edit <issue> --add-assignee pharmacolog

# Project V2 update:
gh project item-edit --project-id <project-id> --id <item-id> \
  --field-id <owner-field-id> --single-select-option-id <agent-option-id>
```

### Pre-PR gate-check: `scripts/pre-pr-check.sh` (TODO)
```bash
#!/bin/bash
set -euo pipefail
echo "=== fmt ===" && cargo fmt --all -- --check
echo "=== clippy ===" && cargo clippy --all-targets -- -D warnings
echo "=== clippy+kafka ===" && cargo clippy --all-targets --features kafka -- -D warnings
echo "=== test ===" && cargo test --release --lib
echo "=== build ===" && cargo build --release --locked
echo "=== doc ===" && RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
echo "=== public-api ===" && diff <(cargo public-api --features test-helpers 2>/dev/null) api-snapshot.txt
echo "=== n7 ===" && bash scripts/check-n7-invariant.sh
echo "=== deny ===" && cargo deny check
echo "=== machete ===" && cargo machete
echo "ALL CHECKS PASSED ✅"
```

## 7. Anti-Patterns (чего НЕ делать)

- ❌ Force-push в `main`/`dev`/`release/*`
- ❌ Push без локального pre-PR gate-check
- ❌ Менять Project V2 напрямую (только через gh CLI / workflow)
- ❌ Писать "TODO: implement later" без linked issue
- ❌ Использовать `git checkout` для переключения между worktrees (использовать `cd`)
- ❌ Закрывать issue без verification по коду
- ❌ Закрывать issue без обновления документации
- ❌ Браться за issue, который уже assigned другому агенту без согласования
- ❌ **PR без тестов** для нового кода (S11/P9)
- ❌ **Удалять существующие тесты** без обоснования
- ❌ **`#[ignore]` на flaky tests** без linked tracking issue

---

**Следующий шаг**: показать пользователю, применить к текущей работе, начать Issue #85 remaining sub-tasks.

---

## История изменений

### v1.2 (2026-07-24) — DEPRECATED, перенесено в AGENTS.md v2.0
- Документ помечен как deprecated. Все правила из §S1–S11 унифицированы и
  перенесены в [`AGENTS.md`](../AGENTS.md) — single source of truth.
- **Изменения в унификации:**
  - Lock state machine: добавлены transition rules и Heartbeat semantics.
  - Issue Status: явное определение `Done` (6 условий) + Milestone closure.
  - Merge strategy: `--squash` теперь обязателен (отменено старое правило "запрет squash").
  - Test Coverage (S11/P9): скопирован в AGENTS.md §8 как часть SoT.
  - Coordination docs (CLAUDE_HANDOFF, AGENTS, COORDINATION) могут мержиться в `main` напрямую через PR.
  - Terminology: milestone/sprint/release — синонимы.
  - cargo-deny warnings: явно OK при exit code 0.
  - `scripts/coordinate.sh` помечен как TODO, описание сохранено.
  - GitHub auth bootstrap секция добавлена.
- Любые будущие изменения координации — в `AGENTS.md`, не здесь.

### v1.1 (2026-07-23 13:50) — добавили S11/P9: Test Coverage Requirement

По запросу пользователя: каждый code change в рамках задачи ДОЛЖЕН сопровождаться тестами. Правила:
- New feature → positive + negative + edge cases
- Bug fix → regression test
- Refactor → existing tests pass, coverage не падает
- Optimization → correctness tests + benchmark

Запрет: PR без тестов для нового кода блокируется merge.

Применяется к:
- Issue #85 remaining sub-tasks (1-7, 9-10, 13)
- Все будущие issue в track-*
