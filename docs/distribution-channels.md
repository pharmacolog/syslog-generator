# Distribution channels

Документ описывает текущие и планируемые каналы распространения
`syslog-generator`.

Основной принцип проекта: solo maintainer начинает с минимального количества
каналов, которые можно надёжно поддерживать, и добавляет новые каналы только
при подтверждённом спросе.

## Политика по distribution channels (принято 2026-07-25)

> **Решение maintainer'а (issue #155 closeout): PPA и COPR НЕ будут
> реализованы для этого проекта.**
>
> **Обоснование:**
> 1. **Solo-maintainer overhead:** PPA и COPR требуют external account setup
>    (Ubuntu One, Fedora Account System), регулярного rotation signing keys
>    между каналами, поддержки source packaging layer (отдельного от
>    `cargo-deb`/`.rpm` build infrastructure), и monitoring build queue.
>    Для одного человека это критический maintenance burden.
> 2. **Source packaging требует отдельной инфраструктуры:** PPA и COPR
>    принимают ТОЛЬКО source packages (`.dsc`/`.src.rpm`), не pre-built
>    бинарные `.deb`/`.rpm` от `cargo-deb`/`cargo-rpm`. Это означает
>    дублирование packaging pipeline (`debian/control`, `packaging/rpm/*.spec`),
>    которое само по себе значительный объём работы и потенциальный
>    источник расхождений версий.
> 3. **GitHub Releases + ghcr.io достаточно для целевой аудитории:**
>    production-серверы с apt/dnf получают artifact + `apt install ./file.deb`/
>    `dnf install ./file.rpm` workflow (1 команда download + 1 команда install),
>    что минимально отличается от PPA/COPR UX. Enterprise-grade подпись через
>    GPG (sub-task 1) закрывает trust gap.
> 4. **Container image покрывает Alpine и Arch use-case** (см. §Alpine).
>
> **Это решение финальное для проекта.** Любое будущее пересмотрение
> должно быть обосновано (не "может пригодиться", а "есть X пользователей
> с Y use-case, которые не могут использовать существующие каналы").

## Статус каналов

| Канал | Статус | Описание |
|---|---|---|
| **GitHub Releases** | ✅ active (primary) | `.deb` + `.rpm` артефакты с GPG-подписью через `packages.yml` |
| **ghcr.io container image** | ✅ active | multi-arch Docker image (`linux/amd64`, `linux/arm64`) |
| Launchpad PPA | ❌ **Not Planned** | Решение maintainer'а 2026-07-25 (см. §Политика выше) |
| Fedora COPR | ❌ **Not Planned** | Решение maintainer'а 2026-07-25 (см. §Политика выше) |
| Alpine `.apk` | ❌ **Not Planned** | musl build + APKBUILD не в scope (покрыто через container image) |
| Arch Linux AUR | ❌ **Not Planned** | Arch user community поддерживает AUR самостоятельно |

## GitHub Releases (✅ active, primary)

GitHub Releases — основной канал распространения для production deployments.

URL для Debian/Ubuntu-пакета:

```text
https://github.com/pharmacolog/syslog-generator/releases/download/v{VERSION}/syslog-generator_{VERSION}_{ARCH}.deb
```

Пример:

```text
https://github.com/pharmacolog/syslog-generator/releases/download/v10.7.19/syslog-generator_10.7.19_amd64.deb
```

### Установка `.deb`

```bash
VERSION=10.7.19
ARCH=amd64
BASE_URL="https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}"

curl -fL "${BASE_URL}/syslog-generator_${VERSION}_${ARCH}.deb" \
  -o "syslog-generator_${VERSION}_${ARCH}.deb"

# Перед установкой — проверяем GPG-подпись (после sub-task 1 завершения)
gpg --keyserver hkps://keys.openpgp.org --recv-keys "<MAINTAINER_KEY_FINGERPRINT>"
curl -fL "${BASE_URL}/syslog-generator_${VERSION}_${ARCH}.deb.asc" \
  -o "syslog-generator_${VERSION}_${ARCH}.deb.asc"
gpg --verify "syslog-generator_${VERSION}_${ARCH}.deb.asc" \
  "syslog-generator_${VERSION}_${ARCH}.deb"

sudo apt install "./syslog-generator_${VERSION}_${ARCH}.deb"
```

См. `docs/installation.md` §7.7 для подробной процедуры verify (после
sub-task 1 завершения).

### Установка `.rpm`

```bash
VERSION=10.7.19
RPM_FILE="syslog-generator-${VERSION}-1.x86_64.rpm"
BASE_URL="https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}"

curl -fL "${BASE_URL}/${RPM_FILE}" -o "${RPM_FILE}"
# Verify GPG signature
rpm -K "${RPM_FILE}"
sudo dnf install "./${RPM_FILE}"
```

## Container registries (✅ active)

Контейнерный образ публикуется в GitHub Container Registry:

```text
ghcr.io/pharmacolog/syslog-generator
```

Multi-arch: `linux/amd64` + `linux/arm64`.

```bash
docker pull ghcr.io/pharmacolog/syslog-generator:v10.7.19
docker run --rm ghcr.io/pharmacolog/syslog-generator:v10.7.19 --version
```

### Container image покрывает Alpine use-case

Alpine Linux пользователи могут запускать syslog-generator через
container image без нативного Alpine-пакета:

```bash
docker run --rm --platform linux/amd64 \
  ghcr.io/pharmacolog/syslog-generator:v10.7.19 --version
# Или multi-arch
docker run --rm \
  ghcr.io/pharmacolog/syslog-generator:v10.7.19 --version
```

Container image использует `glibc` из Debian-slim base — работает на любом
Docker runtime (включая Alpine через `docker run` или `podman`).

## Launchpad PPA (❌ Not Planned)

**Не реализуется.** См. §Политика выше для обоснования.

Для Ubuntu/Debian-пользователей рекомендуется `apt install ./file.deb` через
GitHub Releases (workflow 1-2 минуты) или container image.

## Fedora COPR (❌ Not Planned)

**Не реализуется.** См. §Политика выше для обоснования.

Для Fedora/RHEL/Rocky/AlmaLinux-пользователей рекомендуется
`dnf install ./file.rpm` через GitHub Releases или container image.

## Alpine `.apk` (❌ Not Planned)

**Не реализуется.** musl build + APKBUILD + Alpine build environment не
в scope. Покрывается через container image (см. §Container registries).

Для Alpine-пользователей: `docker run --rm ghcr.io/pharmacolog/syslog-generator:TAG`
работает на Alpine через Docker (или `podman --runtime crun`).

## Arch Linux (❌ Not Planned)

Arch User Repository (AUR) поддерживается community-maintainer'ами Arch
ecosystem. Официальный пакет не предоставляется — пользователи могут
создать AUR package самостоятельно по примеру `cargo-deb` workflow.

Если будет ≥10 запросов на официальный AUR пакет — откройте issue с label
`track-gtm` и описанием use-case.

## macOS / Windows

Container image через `docker` (Docker Desktop на macOS, WSL2 на Windows).

## Comparison table

| Channel | Status | Auto-built | Signing | User install command |
|---|---|---:|---|---|
| GitHub Releases `.deb` | ✅ active, primary | Да, через `packages.yml` | GPG signature, Issue #155 sub-task 1 | `sudo apt install ./syslog-generator_<VERSION>_<ARCH>.deb` |
| GitHub Releases `.rpm` | ✅ active, primary | Да, через `packages.yml` | RPM/GPG verification, Issue #155 sub-task 1 | `sudo dnf install ./syslog-generator-<VERSION>-1.x86_64.rpm` |
| Launchpad PPA | ❌ **Not Planned** | — | — | (use GitHub Releases) |
| Fedora COPR | ❌ **Not Planned** | — | — | (use GitHub Releases) |
| Alpine `.apk` | ❌ **Not Planned** | — | — | `docker run ghcr.io/pharmacolog/syslog-generator:TAG` |
| GHCR container | ✅ active | Да | Registry provenance/SBOM; package GPG не применяется | `docker pull ghcr.io/pharmacolog/syslog-generator:<TAG>` |
| Arch AUR | ❌ Not Planned | — | — | (community-maintained) |

## Maintenance overhead (с учётом Not Planned)

Для текущего набора каналов (GitHub Releases + GHCR) maintainer тратит
время на:

- Поддержку `packages.yml` workflow (trigger на tag push, build/sign/verify/release).
- Rotation GPG signing key (раз в 2 года, через `docs/maintainer-guide.md` §1.9).
- Обновление `Cargo.toml` метаданных для `cargo-deb`/`cargo-rpm` при обновлении версий зависимостей.
- Поддержку Dockerfile и container build infrastructure.

Без PPA/COPR/Alpine overhead значительно снижается (нет external account
management, нет source packaging layer, нет musl build matrix).
