# Branch Protection Rules — настройка и обслуживание

**Версия документа:** v10.7.15+ (PR-15, PR-16).
**Назначение:** зафиксировать branch protection rules для `main` и `dev`,
чтобы maintainer'ы могли восстановить их при необходимости
(например, после reset в GitHub UI).

---

## TL;DR — текущая конфигурация

| Branch | Strict checks | Reviews | Linear history | Admin enforce | Force push | Conversation |
|---|---|---|---|---|---|---|
| **`main`** | 7 blocking | 1 approval | ✅ required | ✅ yes | ❌ no | ✅ required |
| **`dev`** | 7 blocking | none | ❌ no | ❌ no | ❌ no | ❌ no |

Обе ветки требуют **обязательные status checks** (strict mode — нужны свежие
checks для merge; устаревшие failed статусы не считаются success).

---

## main (стабильный релизный код)

**Назначение:** хранит только release-готовый код. Никаких прямых push'ей.
Только merge через PR с одобрением maintainer'a.

**Required status checks** (должны быть зелёные для merge):

- `Test (ubuntu-latest)` — primary test run
- `MSRV check (blocking, v10.5.0)` — Rust MSRV enforcement
- `cargo-deny (advisories + licenses, blocking)` — security + license
- `cargo-machete (unused deps, blocking)` — unused dependency detection
- `cargo public-api snapshot (blocking)` — public API stability
- `Coverage (cargo-llvm-cov + codecov upload)` — coverage ≥ 87%
- `Test kafka feature (ubuntu-latest)` — kafka feature integration

**Правила:**

- ✅ **Require pull request reviews** before merging (1 approval)
- ✅ **Dismiss stale pull request approvals** when new commits pushed
- ✅ **Require linear history** (no merge commits from feature branches)
- ✅ **Require conversation resolution** before merging
- ✅ **Enforce admins** (включая repository owner)
- ❌ Disallow force pushes
- ❌ Disallow branch deletions
- ❌ Allow fork syncing — нет (только maintainers)

### API конфигурация (для восстановления)

```bash
TOKEN=<admin-pat-with-administration-write>
curl -s "https://api.github.com/repos/pharmacolog/syslog-generator/branches/main/protection" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Accept: application/vnd.github+json" \
  -H "Content-Type: application/json" \
  -X PUT \
  --data '{
    "restrictions": null,
    "required_status_checks": {
      "strict": true,
      "contexts": [
        "Test (ubuntu-latest)",
        "MSRV check (blocking, v10.5.0)",
        "cargo-deny (advisories + licenses, blocking)",
        "cargo-machete (unused deps, blocking)",
        "cargo public-api snapshot (blocking)",
        "Coverage (cargo-llvm-cov + codecov upload)",
        "Test kafka feature (ubuntu-latest)"
      ]
    },
    "enforce_admins": true,
    "required_pull_request_reviews": {
      "dismiss_stale_reviews": true,
      "required_approving_review_count": 1
    },
    "required_linear_history": true,
    "allow_force_pushes": false,
    "allow_deletions": false,
    "required_conversation_resolution": true
  }'
```

---

## dev (интеграционная ветка — всегда зелёная)

**Назначение:** все feature-ветки мержатся сюда. Должна оставаться "зелёной"
(все CI checks pass) в любой момент. Auto-sync из main через PR workflow.

**Required status checks:** те же 7, что и для main (consistent quality gate).

**Правила:**

- ✅ **Require status checks** (strict mode)
- ❌ **No required reviews** — maintainer может делать self-merge для hotfix
- ❌ **No linear history required** — feature branches могут merge'иться с merge commits
- ❌ **No enforce admins** — maintainers могут push'ить sync commits напрямую
  (но в идеале — через auto-sync PR)
- ❌ Disallow force pushes
- ❌ Disallow branch deletions

### API конфигурация (для восстановления)

```bash
TOKEN=<admin-pat-with-administration-write>
curl -s "https://api.github.com/repos/pharmacolog/syslog-generator/branches/dev/protection" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Accept: application/vnd.github+json" \
  -H "Content-Type: application/json" \
  -X PUT \
  --data '{
    "restrictions": null,
    "required_status_checks": {
      "strict": true,
      "contexts": [
        "Test (ubuntu-latest)",
        "MSRV check (blocking, v10.5.0)",
        "cargo-deny (advisories + licenses, blocking)",
        "cargo-machete (unused deps, blocking)",
        "cargo public-api snapshot (blocking)",
        "Coverage (cargo-llvm-cov + codecov upload)",
        "Test kafka feature (ubuntu-latest)"
      ]
    },
    "enforce_admins": false,
    "required_pull_request_reviews": null,
    "required_linear_history": false,
    "allow_force_pushes": false,
    "allow_deletions": false,
    "required_conversation_resolution": false
  }'
```

---

## Release ветки `release/v*.*.*`

Создаются от `main` для подготовки релиза. **НЕ должны иметь** branch
protection — merge в main идёт через PR `release/v*.*.* → main` с теми же
7 required checks (из CI на `release/v*.*.*` branch).

После успешного merge в main — `release/v*.*.*` ветка остаётся на remote
для истории (см. release/v10.7.3..v10.7.14 — все сохранены).

---

## Что делать если checks изменились

При добавлении/удалении CI jobs в `.github/workflows/ci.yml` нужно
обновить `contexts` в branch protection. Конкретно:

1. Открыть `.github/workflows/ci.yml`, найти новый/изменённый job.
2. Определить его `name:` (например, `cargo-fuzz (smoke)`).
3. Обновить конфигурацию через `gh api`:

```bash
# Добавить новый check в main
TOKEN=<...>
curl -s "https://api.github.com/repos/pharmacolog/syslog-generator/branches/main/protection/required_status_checks" \
  -H "Authorization: Bearer $TOKEN" \
  -X PATCH \
  --data '{
    "strict": true,
    "contexts": [
      "Test (ubuntu-latest)",
      "MSRV check (blocking, v10.5.0)",
      "cargo-deny (advisories + licenses, blocking)",
      "cargo-machete (unused deps, blocking)",
      "cargo public-api snapshot (blocking)",
      "Coverage (cargo-llvm-cov + codecov upload)",
      "Test kafka feature (ubuntu-latest)",
      "NEW_CHECK_NAME_HERE"
    ]
  }'
```

---

## Почему это важно

**Без branch protection:**

- Push в main напрямую → код без CI → регрессия в релизе
- Force push → потеря истории → невозможно bisect
- Hot-fix в main без review → баги в production

**С branch protection (текущая конфигурация):**

- ✅ Любой merge в main проходит через 7 CI gates
- ✅ Требуется review maintainer'a
- ✅ Conversation resolution (комментарии не теряются)
- ✅ dev остаётся "зелёной" — broken commit не пройдёт CI

**Trade-off:**

- dev не требует review → maintainer может сделать hotfix sync без
  задержки. Но strict checks защищают от красного merge.
- main требует review → +1-2 часа на merge, но гарантия качества релиза.
