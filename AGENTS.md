# AGENTS.md — правила для AI-агентов в репозитории

Этот файл — основной контекст поведения для AI-агентов (Claude Code, opencode и т.п.),
работающих над `pharmacolog/syslog-generator`. Дополняет `CLAUDE_HANDOFF.md`, который
описывает проект и процесс выпуска; здесь — операционные правила.

## Синхронизация состояния с GitHub Projects (ОБЯЗАТЕЛЬНО)

**Принцип: при любом изменении состояния задачи немедленно актуализируй её на досках.**

GitHub Projects v2 — единый source of truth для статусов, owner'ов, locks и handoff.
Расхождения между досками и фактическим состоянием issues/PRs/worktrees
порождают race conditions между параллельными агентами и ломают coordination через issue #113.

### Когда обновлять

Триггеры — любое из следующих событий (даже если ты не заканчиваешь задачу):

| Событие | Что обновлять |
|---|---|
| `gh issue edit`, `gh pr create/close/merge` | статус карточек в Project #1, #2 |
| Claim/lock в `Agent Operations` (`#4`) | создать карточку со `Sync State: Current` + `Lock State: active` |
| Sync ветки с `origin/main`/`origin/dev` | `Sync State: Current`, обновить `Behind N commits` |
| Push в feature-ветку | `Branch` в карточке Project #2 |
| Heartbeat / blocker | `Heartbeat` (date), `Blocked By` (текст) |
| PR open | в Project #2 добавить карточку `status=In Review`, `CI State=Running/Green/Red` |
| PR merged | `status=Done`, `owner=Maintainer`, `Lock State=released`, `CI State=Green` |
| Issue closed | `status=Done`, освободить `Lock State`, снять `lock:claimed` |
| Issue transfer / superseded | `status=Done` или оставить, добавить link в handoff |
| Stale карточка (agent-сессия ended) | archiveProjectV2Item для карточки или обновить `status=Released`, `Lock State=released` |
| Branch deleted / worktree removed | очистить `Branch` поле карточки |

### Как обновлять

- **Быстрые правки** — `gh api graphql` с `updateProjectV2ItemFieldValue`.
- **Массовые правки** — для серии однотипных карточек формируй GraphQL `mutation`
  с переменными `$item`, `$f`, `$o` (для `singleSelectOptionId`) / `$t` (для `text`).
- **archiveProjectV2Item** — для stale карточек; карточка исчезает из `item-list` и не мешает.
- **Поля проектов:**
  - Project #1 (Roadmap): `Status`, `Critical Path`, `Risk Gate`, `Release Confidence`,
    `Priority`, `Track`, `Effort`, `Risk`, `File Scope`, `Dependency Chain`.
  - Project #2 (Scrum Delivery): `Status`, `Owner`, `Lock State`, `CI State`, `Review SLA`,
    `Sub-agent State`, `Branch`, `Worktree`, `Started`, `Heartbeat`, `Blocked By`,
    `Handoff Summary`, `Sprint`, `Sprint Goal`.
  - Agent Operations (#4): `Status` (Backlog/Claimed/Active/Handoff/In Review/Blocked/Released),
    `Agent`, `Issue or PR`, `Branch`, `Worktree`, `File Scope`, `Lock State`
    (free/claimed/active/review/stale/released), `Sync State`, `Blocked By`, `Heartbeat`,
    `Handoff Summary`.

### Перед завершением сессии — reconciliation

Перед закрытием своей сессии (handoff) выполни:
1. Сверить карточку `Agent Operations` со своим фактическим состоянием:
   - `Sync State = Current`?
   - `Lock State = released` или `active` (если передаёшь другому агенту)?
   - `Heartbeat = сегодня`?
   - `Blocked By` пуст или содержит актуальную блокировку?
2. Если ты закрыл issue / merge'нул PR — обновить `Project #1`/`Project #2`:
   - `Status=Done`, `Critical Path=Completed` (если был на critical path),
     `Risk Gate=Green`, `Release Confidence=Complete`.
3. Если карточка твоей сессии в `Agent Operations` стала неактуальной (ветка
   удалена, PR merged, work transferred) — `archiveProjectV2Item` или `status=Released`.
4. Опубликовать standup в issue #113 с финальным состоянием: что сделано, что
   осталось, blocker/handoff для следующего агента.

### Анти-паттерны (запрещено)

- ❌ Закрыть issue в GitHub, не обновив `Status` в Project boards.
- ❌ Merge'нуть PR, оставив `Lock State=active` или `owner=Agent-X` без передачи.
- ❌ Удалить worktree/ветку, оставив в карточке `Branch=<deleted>` или `Sync State=Behind`.
- ❌ Игнорировать `lock:claimed` label после закрытия issue.
- ❌ Держать `Status=In Progress` для карточки, чей `Lock State=released`
  (только если не передаёшь другому агенту с явным blockedBy).
- ❌ Оставлять `Status=Ready/Backlog` для closed issues — это вводит в заблуждение.

### Автоматизация

- `Agent Operations Sync` — карточка, которая фиксирует `Sync State: Current`,
  `Heartbeat: today`, `Lock State: released` после каждого push/sync.
- `project-v2-sync.yml` workflow синхронизирует `Status` issue из GitHub events
  (closed → Done), но `Lock State`, `Owner`, `Branch`, `Worktree`,
  `Handoff Summary` обновляет только агент через `gh api graphql`.

## Coordination

- **Issue #113** (`[META] Multi-agent coordination + standups`) — обязательный
  hub для всех standup-комментариев. Каждый агент комментирует standup при
  старте работы, изменении scope, завершении и handoff.
- **Pre-claim checks** перед началом работы над issue:
  1. `gh issue view <N> --json assignees,state,labels` — убедиться, что нет
     активного owner'а.
  2. `gh project item-list 2` — проверить `Lock State` карточки.
  3. Если owner назначен / Lock State=active — **не** брать задачу, обсудить
     в #113.
  4. `git fetch origin && git log --left-right --count HEAD...origin/main` —
     синхронизировать свою worktree.
- **Lock state machine:** `free → claimed → active → (review | blocked | stale | released)`.
  - `free` — никто не работает.
  - `claimed` — placeholder для заявки, до первого push.
  - `active` — есть локальные изменения / push в feature-ветку.
  - `review` — открыт PR, ожидается merge.
  - `blocked` — внешняя блокировка (например, ожидание другого PR).
  - `stale` — Heartbeat > 24h без прогресса.
  - `released` — работа завершена, branch deleted, handoff опубликован.

## File Ownership (см. `docs/COORDINATION.md` §S3 для деталей)

- `src/plan/*` — Agent-1 (Issue #88 lock).
- `src/generator/core.rs` — shared (требует sync).
- `src/transport/tls.rs`, `src/transport/tcp.rs` — относительно shared,
  требуют pre-claim объявления.
- `src/format/*` — относительно shared, требуют pre-claim.
- `Cargo.toml` — serialized, merge order обязателен.
- `.github/workflows/*.yml` — относительно shared.
- `docs/COORDINATION.md`, `CLAUDE_HANDOFF.md`, `AGENTS.md` — coordination docs,
  изменения согласовываются в #113.

## Quality Gates (см. `scripts/pre-pr-check.sh`)

Перед `git push` (или `gh pr create`) выполни:
```bash
bash scripts/pre-pr-check.sh --skip-build --skip-kafka
```
Если есть WIP commit, который не должен проходить полный build — добавь
`--skip-build` для pre-push validation только на изменённых gates.

Финальная проверка перед merge (в CI):
- `actionlint .github/workflows/*.yml` — clean.
- `python3 -c "yaml.safe_load(...)"` — clean.
- `bash -n scripts/*.sh` — clean.
- `bash scripts/check-advisory-expiry.sh` — PASS.
- `cargo test --locked --features test-helpers` (на CI).

## Версионирование

Версионирование файла: `AGENTS.md` — кумулятивный документ. Изменения
вносятся через PR с явным changelog в commit message. Cross-reference
в `CLAUDE_HANDOFF.md §0` для агентов, которые ищут контекст.
