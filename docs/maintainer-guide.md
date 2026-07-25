# Maintainer Setup Guide — Issue #155 follow-up

> **Цель:** детальный пошаговый мануал для завершения Issue #155 (GPG-подпись и
> distribution channels для syslog-generator) после того, как PR #156 установил
> всю инфраструктуру.
>
> **Когда выполнять:** после merge PR #156 в `dev` (commit `0866191`,
> maintainer-side финализация).
>
> **Где применять:**
> - sub-task 1: заменить placeholder GPG-ключ на production release signing
>   public key.
>
> **Note:** sub-tasks 4 (PPA), 5 (COPR), 6 (Alpine) — **Not Planned** (решение
> maintainer'а 2026-07-25, см. `docs/distribution-channels.md` §Политика).
> Этот документ покрывает только sub-task 1.
>
> **TL;DR:**
> 1. Сгенерировать release signing subkey на macOS (offline backup primary key).
> 2. Залить публичный ключ → `scripts/keys/pharmacolog-release.asc` (commit).
> 3. Залить private signing subkey → GitHub Environment secret `release`.
> 4. Set passphrase → `GPG_PASSPHRASE`.
> 5. Set fingerprint → repository variable `GPG_KEY_FINGERPRINT`.
> 6. Создать GitHub Environment `release` с required reviewers.

---

## Часть 1: GPG release signing subkey (sub-task 1)

## Acceptance checklist (overall)

| Sub-task | Action | Verification | Status |
|---|---|---|---|
| **1** | Generate GPG release signing subkey + replace placeholder | `dpkg-sig --verify` OK, `rpm -K` OK | ⬜ Pending (PR #156 closed it as scaffold) |
| **2** | sign step в packages.yml | CI signing job success | ✅ Scaffold in PR #156 |
| **3** | verify-install hardening | CI verify-install OK | ✅ PR #156 |

После завершения sub-task 1 — milestone v11.4 полностью функционален для
production deployment через GitHub Releases.

## Cross-references

- [Issue #155](https://github.com/pharmacolog/syslog-generator/issues/155) — tracking issue.
- [Issue #107](https://github.com/pharmacolog/syslog-generator/issues/107) — closed via PR #154 (basic packages).
- [PR #156](https://github.com/pharmacolog/syslog-generator/pull/156) — scaffold merge.
- [PR #157](https://github.com/pharmacolog/syslog-generator/pull/157) — coordination docs (этот PR).
- [AGENTS.md §2](AGENTS.md) — GitHub auth bootstrap.
- [AGENTS.md §10](AGENTS.md) — pre-PR gate-check.
- [AGENTS.md §15](AGENTS.md) — board sync принцип (обновлять после каждого действия).
- `docs/installation.md` — user-facing install docs.
- `docs/distribution-channels.md` — distribution channels overview + policy.
