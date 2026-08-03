# Distribution channels

Документ описывает текущие и планируемые каналы распространения
`syslog-generator`.

Основной принцип проекта: solo maintainer начинает с минимального количества
каналов, которые можно надёжно поддерживать, и добавляет новые каналы только
при подтверждённом спросе.

## GitHub Releases

**Статус: ✅ active**
**Роль: основной канал распространения**

GitHub Releases используется для публикации готовых Linux-пакетов `.deb` и
`.rpm`. Артефакты собираются workflow `packages.yml` и прикрепляются к
соответствующему GitHub Release.

URL для Debian/Ubuntu-пакета:

```text
https://github.com/pharmacolog/syslog-generator/releases/download/v{VERSION}/syslog-generator_{VERSION}_{ARCH}.deb
```

Например:

```text
https://github.com/pharmacolog/syslog-generator/releases/download/v10.7.19/syslog-generator_10.7.19_amd64.deb
```

### Установка `.deb`

Сначала задайте версию и архитектуру пакета:

```bash
VERSION=10.7.19
ARCH=amd64
BASE_URL="https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}"
```

Скачайте пакет:

```bash
curl -fL \
  "${BASE_URL}/syslog-generator_${VERSION}_${ARCH}.deb" \
  -o "syslog-generator_${VERSION}_${ARCH}.deb"
```

Перед установкой необходимо проверить detached GPG-подпись пакета. Имя
подписанного файла публикуется вместе с пакетом:

```bash
curl -fL \
  "${BASE_URL}/syslog-generator_${VERSION}_${ARCH}.deb.asc" \
  -o "syslog-generator_${VERSION}_${ARCH}.deb.asc"
```

Импортируйте OpenPGP-ключ maintainer'а через доверенный канал и проверьте
fingerprint ключа:

```bash
KEY_FINGERPRINT="<MAINTAINER_KEY_FINGERPRINT>"

gpg --keyserver hkps://keys.openpgp.org \
  --recv-keys "$KEY_FINGERPRINT"

gpg --fingerprint "$KEY_FINGERPRINT"
gpg --verify \
  "syslog-generator_${VERSION}_${ARCH}.deb.asc" \
  "syslog-generator_${VERSION}_${ARCH}.deb"
```

Установите пакет только после успешной проверки подписи:

```bash
sudo apt install "./syslog-generator_${VERSION}_${ARCH}.deb"
```

GPG-подпись пакетов и публикация открытого ключа являются частью Issue #155.
До завершения этого workstream следует проверять fingerprint ключа вручную и
не считать один только HTTPS достаточной проверкой происхождения пакета.

### Установка `.rpm`

Для Fedora-подобных систем используется RPM-артефакт из того же GitHub
Release:

```bash
VERSION=10.7.19
RPM_FILE="syslog-generator-${VERSION}-1.x86_64.rpm"
BASE_URL="https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}"

curl -fL "${BASE_URL}/${RPM_FILE}" -o "${RPM_FILE}"
sudo dnf install "./${RPM_FILE}"
```

Перед установкой RPM необходимо выполнить проверку подписи, когда detached
signature и OpenPGP-ключ опубликованы для соответствующего release.

## Launchpad PPA

**Статус: ⚠️ deferred**
**Причина: требуется Ubuntu One account и ручная настройка PPA**

На момент создания Issue #155 у solo maintainer'а не было зарегистрированного
Launchpad PPA. PPA будет добавлен после появления реального спроса на
нативную установку через `apt`.

### Prerequisites

Для публикации в Launchpad PPA нужны:

1. Ubuntu One account.
2. Подтверждённый email.
3. Launchpad account.
4. Созданный PPA для проекта.
5. OpenPGP-ключ, добавленный в Launchpad.
6. Локальная настройка `debuild` и `dput`.
7. Debian source packaging в репозитории.

OpenPGP-ключ должен быть доступен Launchpad, а fingerprint ключа должен
совпадать с ключом, которым подписывается source package.

### План создания PPA

1. Создать или активировать Ubuntu One account.
2. Подтвердить email.
3. Создать Launchpad account.
4. Создать PPA `pharmacolog/syslog-generator`.
5. Сгенерировать OpenPGP-ключ либо выбрать существующий ключ.
6. Загрузить открытый ключ на Launchpad.
7. Проверить fingerprint ключа в локальном GPG keyring и в Launchpad.
8. Подготовить Debian source package.
9. Собрать и подписать source package.
10. Загрузить `.changes` и source artifacts через `dput`.
11. Дождаться обработки build queue.
12. Проверить сборку для поддерживаемых Ubuntu releases.
13. Проверить установку через `apt`.

### Source packaging

PPA требует Debian source package. Минимальный набор packaging-файлов:

```text
debian/control
debian/rules
debian/changelog
```

В source package также должны быть корректно определены:

- имя binary package;
- версия пакета;
- архитектуры сборки;
- build dependencies;
- runtime dependencies;
- описание пакета;
- правила сборки Rust-проекта;
- changelog entry для конкретного Ubuntu release.

### Сборка и upload

После подготовки `debian/` source package создаётся и подписывается командой:

```bash
debuild -S -sa -k$FINGERPRINT
```

Затем source package загружается в PPA:

```bash
dput ppa:pharmacolog/syslog-generator ../syslog-generator_<VERSION>-1_source.changes
```

После upload Launchpad помещает source package в build queue. Ожидание
обычно составляет около 30 минут, но фактическое время зависит от нагрузки
Launchpad и количества целевых Ubuntu series.

### Ограничения PPA

Launchpad PPA принимает **только source packages**.

Готовый `.deb`, созданный `cargo-deb`, нельзя напрямую загрузить в PPA как
замену Debian source package. Launchpad должен сам пересобрать пакет из
исходников с использованием Debian packaging metadata.

Таким образом, публикация в PPA требует отдельного source packaging и
отдельного цикла проверки сборки. GitHub Release `.deb` и Launchpad PPA —
разные артефакты с разными процессами публикации.

## Fedora COPR

**Статус: ⚠️ deferred**
**Причина: требуется Fedora account, Fedora Account System и ручная настройка**

На момент создания Issue #155 у solo maintainer'а не было настроенного Fedora
COPR account. COPR будет добавлен после появления спроса на native RPM
distribution.

### Prerequisites

Для публикации в COPR нужны:

1. Fedora account.
2. Fedora Account System (FAS) identity.
3. Доступ к COPR.
4. Установленный и настроенный `copr-cli`.
5. RPM spec file.
6. Возможность собрать source RPM локально.
7. OpenPGP signing setup, если RPM signing будет включён в release process.

Настройка `copr-cli` выполняется через Fedora credentials:

```bash
copr-cli configure
```

### План создания COPR project

1. Создать Fedora account.
2. Завершить регистрацию в Fedora Account System.
3. Настроить `copr-cli`.
4. Создать COPR project `syslog-generator`.
5. Подготовить RPM spec file.
6. Собрать source RPM.
7. Запустить COPR build.
8. Включить требуемые chroots.
9. Дождаться пересборки пакета COPR.
10. Проверить установку через `dnf`.
11. Проверить обновление пакета между версиями.

### Source packaging

COPR должен пересобрать пакет из source RPM. Source RPM создаётся через
`rpmbuild -bs`:

```bash
rpmbuild -bs packaging/syslog-generator.spec
```

Результатом является файл вида:

```text
~/rpmbuild/SRPMS/syslog-generator-<VERSION>-<RELEASE>.src.rpm
```

RPM spec file должен описывать:

- исходный архив или source checkout;
- build dependencies;
- runtime dependencies;
- Rust toolchain requirements;
- build и install steps;
- license;
- binary files;
- system integration;
- changelog.

### Создание project и build

Пример создания COPR project:

```bash
copr-cli create syslog-generator
```

После создания source RPM запускается build:

```bash
copr-cli build \
  syslog-generator \
  ~/rpmbuild/SRPMS/syslog-generator-${VERSION}-1.src.rpm
```

COPR пересобирает binary RPM на своей build infrastructure. Готовый RPM не
загружается в COPR как единственный источник истины: COPR использует source RPM
для повторяемой сборки.

### Chroots

Планируемые chroots:

```text
Fedora 39+
EPEL 9
Rocky Linux 9
AlmaLinux 9
```

Точный список версий Fedora следует поддерживать актуальным в COPR project,
поскольку Fedora releases имеют ограниченный lifecycle.

### Ограничения COPR

COPR rebuilds from source. Это означает:

- локальный binary RPM не заменяет source RPM;
- build dependencies должны быть доступны в выбранном chroot;
- packaging должен быть воспроизводимым;
- несовпадение toolchain или системных библиотек может проявиться только во
  время COPR build;
- каждый новый chroot увеличивает время проверки и maintenance overhead.

## Alpine `.apk`

**Статус: ✅ active** (Issue #159, milestone v11.4)

Native `.apk` публикуется в GitHub Releases. Сборка через `cargo build
--target x86_64-unknown-linux-musl` + `abuild -r` в Alpine-builder environment
(подробно в `docs/installation.md` §4). Alpine `.apk` покрывает тот же use-case,
что планировался для PPA/COPR — без overhead внешних account setup.

Подробная процедура установки — в `docs/installation.md` §4 (Alpine).

Поддержка Alpine `x86_64-unknown-linux-musl` в Alpine-builder environment
подтверждена локально (мусл-билд syslog-generator собирается за ~4-5 минут,
~14.8 MB ELF static-pie бинарь; `abuild -r` упаковывает в `.apk`). GitHub Actions
через `.github/workflows/packages.yml:build-apk` job собирает `.apk` в Alpine
container, upload'ит в GitHub Release как `syslog-generator-$VERSION-r$PKGREL.apk`.

См. также `alpine/README.md` для maintainer notes по локальной сборке
и `alpine/APKBUILD` для шаблона.

**Out of scope (отложено):**
- aarch64 musl build — initial scope только x86_64. Issue #159 явно out-of-scope
  aarch64. Расширение через cross-compilation (QEMU или cross) — follow-up issue.

## Container registries

**Статус: ✅ active**

Контейнерный образ публикуется в GitHub Container Registry:

```text
ghcr.io/pharmacolog/syslog-generator
```

Для release images поддерживается multi-arch публикация:

```text
linux/amd64
linux/arm64
```

Пример установки:

```bash
docker pull ghcr.io/pharmacolog/syslog-generator:v10.7.19
```

Пример запуска:

```bash
docker run --rm \
  ghcr.io/pharmacolog/syslog-generator:v10.7.19 \
  --version
```

Подробное описание контейнерного запуска приведено в
[`README.md`](../README.md) и в
[`docs/installation.md` §5.6](installation.md#56-container-registries).

Container registry не заменяет Linux package channels:

- контейнер требует Docker или совместимый runtime;
- package manager integration отсутствует;
- обновление управляется тегами образов;
- системные сервисы и host-level integration не устанавливаются автоматически.

## Comparison table

| Channel | Status | Auto-built | Signing | User install command |
|---|---|---:|---|---|
| GitHub Releases `.deb` | ✅ active, primary | Да, через `packages.yml` | GPG signature, Issue #155 | `sudo apt install ./syslog-generator_<VERSION>_<ARCH>.deb` |
| GitHub Releases `.rpm` | ✅ active, primary | Да, через `packages.yml` | RPM/GPG verification, Issue #155 | `sudo dnf install ./syslog-generator-<VERSION>-1.x86_64.rpm` |
| Launchpad PPA | ⚠️ deferred | Да, после source upload и Launchpad build | OpenPGP-signed source package | `sudo add-apt-repository ppa:pharmacolog/syslog-generator && sudo apt install syslog-generator` |
| Fedora COPR | ⚠️ deferred | Да, после source RPM upload и COPR build | RPM/OpenPGP signing по настроенному policy | `sudo dnf copr enable pharmacolog/syslog-generator && sudo dnf install syslog-generator` |
| Alpine `.apk` | ❌ not supported | Нет | Не определено | Не поддерживается |
| GHCR container | ✅ active | Да | Registry provenance/SBOM; package GPG не применяется | `docker pull ghcr.io/pharmacolog/syslog-generator:<TAG>` |

Статусы `✅ active` для GitHub Releases и GHCR означают наличие основного
release workflow. Переход от плановой GPG-схемы к обязательной проверке
подписей должен быть завершён в рамках Issue #155.

## Maintenance overhead

Каждый distribution channel требует времени maintainer'а на:

- первоначальную регистрацию и настройку account;
- управление signing keys и их rotation;
- поддержку packaging metadata;
- проверку сборки на целевых дистрибутивах;
- мониторинг build queue;
- обработку failed builds;
- проверку install и upgrade paths;
- реагирование на изменения toolchain и системных зависимостей;
- обновление документации;
- расследование проблем пользователей.

Solo maintainer начинает с GitHub Releases и container registry. Launchpad PPA,
Fedora COPR и Alpine packaging добавляются только при достаточном user demand,
когда maintenance overhead оправдывает дополнительный канал.
