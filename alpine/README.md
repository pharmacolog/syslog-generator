# Alpine .apk — Maintainer notes (Issue #159)

Этот каталог содержит Alpine-specific файлы для sub-task Issue #159
(Alpine `.apk` packaging для syslog-generator).

## Файлы

- `APKBUILD` — шаблон Alpine package recipe (hand-written, не cargo-install --root).
- `abuild.rsa.pub` (planned) — публичный RSA-ключ для verify подписи `.apk`.

## Build (локально, через abuild)

```bash
# На Alpine 3.21+:
abuild-keygen -an -n 'pharmacolog <noreply@pharmacolog.github.io>'
abuild -r -c
# Output: ~/packages/syslog-generator/x86_64/syslog-generator-10.7.19-r0.apk
```

## Verify

```bash
apk add --allow-untrusted ~/packages/syslog-generator/x86_64/syslog-generator-*.apk
syslog-generator --version
apk info syslog-generator
```

## CI

`.github/workflows/packages.yml` job `build-apk` запускает `cargo build
--target x86_64-unknown-linux-musl` через `docker run alpine:3.21`, затем
`abuild -r -c` для упаковки `.apk`. Output upload'ится в GitHub Release
как `syslog-generator-$VERSION-r0.apk`.

## Out of scope

- **aarch64 musl build** — initial scope только x86_64 (Issue #159 sub-task 6).
- **GPG/PGP signature** — Issue #159 acceptance НЕ требует подписи; используется
  native APK signing через `abuild -k ~/.abuild/<packager>.rsa`. Если maintainer
  хочет signed APK — генерирует `abuild-keygen` локально и добавляет `APK_SIGN_KEY`
  в GitHub Secrets (см. `docs/maintainer-guide.md` для деталей).
- **Alpine aports / community repository** — официальный канал НЕ предоставляется
  (solo-maintainer overhead). Alpine-пользователи устанавливают через GitHub
  Releases с `--allow-untrusted` или через container image.
