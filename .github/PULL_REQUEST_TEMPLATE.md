<!--
Этот шаблон автоматически применяется ко всем PR в syslog-generator.
Заполни ВСЕ секции; пустые секции = PR не будет review'нут.

Обязательно прочитай CONTRIBUTING.md перед открытием PR.
-->

## Что меняется

<!-- Кратко (1-3 предложения) что делает этот PR. -->

## Тип изменений

<!-- Поставь [x] во всех подходящих пунктах. -->

- [ ] Bug fix (non-breaking change, который чинит issue)
- [ ] New feature (non-breaking change, который добавляет функциональность)
- [ ] Breaking change (fix или feature, который ломает backward compatibility)
- [ ] Documentation update
- [ ] CI / tooling change
- [ ] Performance improvement
- [ ] Security fix

## Связанные issues / PRs

<!-- Closes #N, Fixes #N, Related to #N, Depends on #N -->

Refs: <!-- если ссылается на план/документ -->

## Checklist (обязательно к заполнению)

### Pre-PR (локальные проверки)

- [ ] `cargo fmt --all -- --check` — clean
- [ ] `cargo clippy --all-targets --features kafka,test-helpers -- -D warnings` — clean
- [ ] `cargo test --locked --features test-helpers` — все тесты зелёные
- [ ] `bash scripts/check-n7-invariant.sh` — clean (no unwrap/expect в runtime)
- [ ] Если изменены `.github/workflows/*.yml`: `bash -n` для всех `run: |` блоков пройден локально
- [ ] Если менялся публичный API: `cargo public-api` snapshot обновлён в `api-snapshot.txt`

### Code quality

- [ ] Код соответствует существующему стилю (см. DEVELOPER_GUIDE.md)
- [ ] Добавлены юнит-тесты для новой логики
- [ ] Если добавлен формат/транспорт — следует паттерну из DEVELOPER_GUIDE §"Adding a format/transport"
- [ ] N7 invariant: ни одного нового `.unwrap()`/`.expect()` в non-test runtime коде

### Документация

- [ ] CHANGELOG.md — новая секция (если пользовательский-facing change)
- [ ] README.md — обновлены badges / features / CLI examples (если публичный API)
- [ ] AUDIT.md — статус задач обновлён (✅ / 🔄 / ❌)
- [ ] docs/USER_GUIDE.md — обновлены примеры (если поведение изменилось)
- [ ] CLAUDE_HANDOFF.md — версия обновлена (для release PR)

### Quality gates (CI блокирующие)

- [ ] Все CI checks на этом PR зелёные:
  - [ ] `Test (ubuntu-latest)` — success
  - [ ] `Test (macos-latest)` — non-blocking
  - [ ] `MSRV check (blocking, v10.5.0)` — success
  - [ ] `Coverage (cargo-llvm-cov + codecov upload)` — ≥ 87%
  - [ ] `cargo-deny (advisories + licenses, blocking)` — clean
  - [ ] `cargo-machete (unused deps, blocking)` — clean
  - [ ] `cargo public-api snapshot (blocking)` — clean
  - [ ] `Test kafka feature (ubuntu-latest)` — success
  - [ ] `Analyze (actions)` (CodeQL) — success
  - [ ] `Analyze (rust)` (CodeQL) — success

### Branch & flow

- [ ] Branch создан от `dev` (НЕ от `main` — только release/v*.*.* от main)
- [ ] Имя branch: `feature/<short-name>` или `fix/<short-name>` или `release/vX.Y.Z`
- [ ] Merge в `dev` (НЕ в `main` — для main использовать release flow)
- [ ] Локально: `git fetch origin && git rebase origin/dev` перед push (если dev ушёл вперёд)
- [ ] CI на этом PR зелёный → review → merge через GitHub UI (НЕ локальный merge)

## Тестирование

<!-- Как тестировал / как воспроизвести. Скриншоты, если UI. -->

```bash
# Repro / verification команды
```

## Риски и rollback

<!-- Что может сломаться? Как откатить? Нужна ли миграция? -->

## Дополнительные заметки

<!-- Breaking changes, performance impact, security implications -->
