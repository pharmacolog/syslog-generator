# `.archived-releases/`

Этот каталог содержит **локально-собранные архивы** `syslog-generator-vX.Y.Z-verified.zip`,
сохранённые **вне репозитория** (`.gitignore` запись `/.archived-releases/`).

## Назначение

- Архив каждого релиза нужен для **повторной проверки** или **smoke-test**
  конкретной версии (например, регрешн-анализ).
- Эти архивы — **не часть публичного API**, не привязаны к git-тегам
  (теги — это `git tag -a vX.Y.Z`, архивы — это просто локально собранные
  бинарники).

## Правила

1. **Не коммитить** содержимое этого каталога (`.gitignore` запрещает).
2. **Не удалять** этот каталог (`.gitkeep`).
3. **Пополнять** после каждого release-коммита:

   ```bash
   source "$HOME/.cargo/env" && cargo clean
   cd /Users/anton/svn/github
   zip -rq .archived-releases/syslog-generator-vX.Y.Z-verified.zip \
     syslog-generator -x '*/target/*' -x '*/.git/*' -x '*.zip' \
     -x '.archived-releases/*'
   unzip -p .archived-releases/syslog-generator-vX.Y.Z-verified.zip \
     syslog-generator/Cargo.toml | grep '^version'  # → version = "X.Y.Z"
   ```

4. **Feature-бранчи НЕ удалять** после merge — они остаются как `feature/vX.Y.Z-*`
   в remote refs для аудита/отката. Локально удалять можно после merge,
   remote ref сохраняется.

## Связь с git-тегами

| git tag | Архив |
|---------|-------|
| `git tag -a vX.Y.Z` (в release-коммите) | `.archived-releases/syslog-generator-vX.Y.Z-verified.zip` (локально) |

Если git tag push-ится в origin, архив остаётся локальным. Это намеренное
разделение: теги — публичные (для пользователей), архивы — локальные
(для разработки/QA).

## Итого

По состоянию на v8.8.0 (2026-07-13) каталог пуст — все архивы были
собраны и использованы для smoke-test, после чего удалены в ходе
финальной уборки релиза. В следующих релизах (v8.8.1, v9.0.0, v9.x.y)
архивы будут сохраняться здесь.
