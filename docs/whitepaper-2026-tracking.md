# Whitepaper 2026 — Citations & UTM Stars Tracking

> **Issue:** #200 (citations + UTM stars tracking).
> **Milestone:** v11.6.
> **Назначение:** Quarterly tracking документа citations / backlinks /
> GitHub stars для Whitepaper 2026 (Issues #106, #196, #199).
> **Initial state:** 0 citations, 0 UTM-stars (publication ещё не выполнялся).
> **Acceptance criteria (Issue #106):** ≥5 citations / 3 months after
> Issue #191 (release), ≥50 GitHub stars via UTM.

## Связанные issue

- **#106** — Whitepaper harness (готов, status=`schema_only`).
- **#191** — release marker (`v11.6.0` tag) — стартовая точка для
  3-месячного окна citation counting.
- **#196** — real VM run (поставщик численных результатов).
- **#199** — publication (Habr + dev.to). После #199 permalinks
  становятся известны и сюда вписываются.
- **#200** — этот issue (tracking).

## 1. Backlinks tracking table

Таблица обновляется вручную (PR) при обнаружении citation, а также
автоматически через [`.github/workflows/citation-tracker.yml`](../.github/workflows/citation-tracker.yml)
(quarterly scan). Manual scan делается при:

- просмотре Twitter/X mentions / Reddit posts / Hacker News threads;
- упоминании в release notes других проектов;
- цитировании в RFC draft / IETF document;
- community feedback через GitHub Discussions.

### 1.1 Формат записи

| Колонка | Описание |
|---|---|
| `URL` | Полный URL до цитирующей страницы. |
| `Anchor` | Текст ссылки / цитаты (короткий, ≤80 chars). |
| `Date` | Дата обнаружения (YYYY-MM-DD). |
| `Type` | Один из: `blog_post`, `rfc`, `discussion`, `social`, `release_notes`, `academic`, `video`, `podcast`. |
| `Traffic_Estimate` | Грубая оценка (low / medium / high / N/A). |
| `Notes` | Контекст, кто автор, связь с нашим issue. |

### 1.2 Citations log

| URL | Anchor | Date | Type | Traffic_Estimate | Notes |
|---|---|---|---|---|---|
| _none yet — initial state_ | | | | | |

> **Empty state expected:** до публикации (Issue #199) backlinks
> быть не должно. Если запись появилась — это либо ссылка на
> существующий `docs/whitepaper-2026.md` (in-repo), либо
> pre-publication leak. Проверить legitimacy.

## 2. Initial state — счётчики

| Счётчик | Initial value | Target (acceptance) | Source |
|---|---|---|---|
| **Citations / backlinks** | 0 | ≥5 за 3 мес | `citations_count` (см. §3) |
| **GitHub stars via UTM** | 0 | ≥50 за 3 мес | `utm_stars_count` (см. §4) |
| **Habr permalink views** | 0 | (informational) | manual |
| **dev.to permalink views** | 0 | (informational) | manual |
| **Twitter/X impressions** | 0 | (informational) | manual |
| **Telegram channel forwards** | 0 | (informational) | manual |

**Стартовая дата:** tag `v11.6.0` (после Issue #191).
**End of 3-месячного окна:** start + 90 days.

## 3. Quarterly review schedule

Quarterly cron: `0 0 1 */3 *` (1-е число каждые 3 месяца, 00:00 UTC).

| Quarter | Start (anchor) | End | Status |
|---|---|---|---|
| **Q1 2027** | 2026-08-15 (TBD, after Issue #191) | 2026-11-15 | _pending_ |
| **Q2 2027** | 2026-11-15 | 2027-02-15 | _pending_ |
| **Q3 2027** | 2027-02-15 | 2027-05-15 | _pending_ |
| **Q4 2027** | 2027-05-15 | 2027-08-15 | _pending_ |

**Per-quarter review checklist:**

- [ ] Quarterly scan выполнен (workflow `citation-tracker.yml`
      success).
- [ ] PR с обновлённым `docs/whitepaper-2026-tracking.md` смержен.
- [ ] Counter deltas записаны в `## 4. Quarterly report`.
- [ ] Acceptance metrics (≥5 citations, ≥50 stars) проверены.
- [ ] Если < 50% от target — запланировать boost actions
      (дополнительные публикации, AMA, конференции).

## 4. Quarterly report (заполняется автоматически)

> **Не редактировать вручную.** Этот раздел обновляется
> автоматически через `.github/workflows/citation-tracker.yml`.
> Ручные правки перезатрутся следующим cron run'ом.

### 2026-Q3 (pre-publication baseline)

| Метрика | Value | Delta vs prev | Target progress |
|---|---|---|---|
| Citations | 0 | — | 0% |
| UTM stars | 0 | — | 0% |
| Habr views | 0 | — | — |
| dev.to views | 0 | — | — |

### 2026-Q4 (post-publication, ожидаемый)

| Метрика | Value | Delta vs prev | Target progress |
|---|---|---|---|
| Citations | _pending_ | _pending_ | _pending_ |
| UTM stars | _pending_ | _pending_ | _pending_ |
| Habr views | _pending_ | _pending_ | — |
| dev.to views | _pending_ | _pending_ | — |

### 2027-Q1 (3-month milestone)

| Метрика | Value | Delta vs prev | Target progress |
|---|---|---|---|
| Citations | _pending_ | _pending_ | _pending_ (target ≥5) |
| UTM stars | _pending_ | _pending_ | _pending_ (target ≥50) |
| Habr views | _pending_ | _pending_ | — |
| dev.to views | _pending_ | _pending_ | — |

## 5. Goals & acceptance

### 5.1 Primary goals (Issue #106 acceptance)

- **≥5 citations / 3 months** after Issue #191 (`v11.6.0` tag).
- **≥50 GitHub stars via UTM** / 3 months.

### 5.2 Secondary goals (informational)

- ≥1000 Habr views (informational, not acceptance).
- ≥500 dev.to views (informational).
- ≥1 citation in academic paper / RFC draft.

### 5.3 Failure handling

Если за 3-месячное окно не достигли targets:

1. **Citations < 5:** не критично, но стоит:
   - Review целевые каналы (Telegram, Reddit, HN).
   - Улучшить SEO (alt anchors, cross-references в README).
   - Public AMA в DevOps Moscow / SRE Russia.
2. **UTM stars < 50:** основная метрика. Если < 25 — обсудить
   с maintainer: возможно, whitepaper не привлёк нужную аудиторию,
   нужен re-targeting.

## 6. Process

### 6.1 Manual backlink addition

При обнаружении citation (см. §1):

```bash
# 1. Edit docs/whitepaper-2026-tracking.md (этот файл), §1.2 таблицу.
# 2. Добавить row в формате:
#    | https://example.com/post | "anchor text" | 2026-12-01 | blog_post | medium | Author's note |
#
# 3. PR в dev:
#    git checkout -b docs/whitepaper-citation-<slug>
#    # edit
#    git add docs/whitepaper-2026-tracking.md
#    git commit -m "docs(whitepaper): track citation by <author> in <publication>"
#    gh pr create --base dev --head docs/whitepaper-citation-<slug>
#
# 4. После merge — counter в §2 обновляется вручную (на сейчас —
#    автоматизация только в §3 quarterly).
```

### 6.2 Automated citations scan (quarterly)

См. [`.github/workflows/citation-tracker.yml`](../.github/workflows/citation-tracker.yml).

**Capabilities:**

- Cron `0 0 1 */3 *` (1-е число каждые 3 мес, 00:00 UTC).
- `workflow_dispatch` для manual triggers.
- Использует Google Custom Search API (если `GOOGLE_CSE_API_KEY` /
  `GOOGLE_CSE_CX` secrets доступны).
- Если API недоступен — fallback на GitHub mention search через
  GitHub GraphQL API (`gh api graphql`).
- Если обе опции failed — просто постит comment в Issue #200
  с текущим состоянием счётчиков.

**Output:**

- Автоматический PR в `dev` с обновлённым `§1.2 Citations log`
  и `§4. Quarterly report`.
- Comment в Issue #200 со summary.

### 6.3 Verification cadence

- **Quarterly:** workflow runs.
- **Monthly:** maintainer manual scan (15 минут, см. §1 sources).
- **On-demand:** если кто-то в [Issue #113](https://github.com/pharmacolog/syslog-generator/issues/113)
  сообщает о citation — добавить сразу.

## 7. Anti-patterns

Что **НЕ** считается citation (manual scan должен игнорировать):

- ❌ Self-citation (ссылка в собственном README или docs).
- ❌ Crawler / bot traffic (heuristic: User-Agent содержит `bot`,
  `crawler`, `spider`).
- ❌ Ссылка в issue/PR этом же репозитория (это self-reference).
- ❌ Ссылка в fork'е.
- ❌ Link farms / SEO spam (если обнаружены — пометить как
  `Notes: spam` и **не** учитывать в counter).

## 8. Privacy & ethics

- ❌ **НЕ** пытаться deanonymize авторов citations.
- ❌ **НЕ** агрессивно outreach'ить (никаких "give us a backlink"
  DM'ов или emails).
- ✅ Принимать citations organically.
- ✅ Если authors хотят remove backlink — уважать, удалять из
  таблицы.

## 9. Cross-references

- Методология: [`benchmarks/whitepaper-2026/METHODOLOGY.md`](../benchmarks/whitepaper-2026/METHODOLOGY.md)
- Runbook: [`benchmarks/whitepaper-2026/docs/benchmark-runbook.md`](../benchmarks/whitepaper-2026/docs/benchmark-runbook.md)
- Schema: [`benchmarks/whitepaper-2026/results/EXPECTED.md`](../benchmarks/whitepaper-2026/results/EXPECTED.md)
- RU whitepaper: [`docs/whitepaper-2026.ru.md`](whitepaper-2026.ru.md)
- EN whitepaper: [`docs/whitepaper-2026.en.md`](whitepaper-2026.en.md)
- Workflow: [`.github/workflows/citation-tracker.yml`](../.github/workflows/citation-tracker.yml)
- Installation: [`docs/installation.md` §12](installation.md#12-whitepaper--citations)
- Issue #113 — multi-agent coordination (Standup thread).
- Issue #200 — этот issue.
- AGENTS.md §15 — board sync (обновлять Project V2 cards).
