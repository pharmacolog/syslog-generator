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
> - sub-task 4: setup Launchpad PPA для автоматической установки через `apt`.
> - sub-task 5: setup Fedora COPR для автоматической установки через `dnf`.
> - sub-task 6: optional Cosign keyless signing для container images.
>
> **TL;DR:**
> 1. Сгенерировать release signing subkey на macOS (offline backup primary key).
> 2. Залить публичный ключ → `scripts/keys/pharmacolog-release.asc` (commit).
> 3. Залить private signing subkey → GitHub Environment secret `release`.
> 4. Set passphrase → `GPG_PASSPHRASE`.
> 5. Set fingerprint → repository variable `GPG_KEY_FINGERPRINT`.
> 6. Создать GitHub Environment `release` с required reviewers.
> 7. (Optional) Настроить PPA/COPR/Alpine через account setup.

---

## Часть 1: GPG release signing subkey (sub-task 1)

### 1.1 Prerequisites

Установить GnuPG на macOS:

```bash
brew install gnupg pinentry-mac
gpg --version
# Ожидаемый вывод: gpg (GnuPG) 2.5.21 (или новее)
```

Подготовить passphrase через password manager (1Password / `pwgen -s 32 1`):
минимум **24 символа**, включая спецсимволы. Хранить **только** в password
manager (никогда в git, никогда в Slack/email).

### 1.2 Генерация primary key + signing subkey

```bash
# 1. Primary key (capability: Certify — только для подписания subkeys)
# Uid должен точно совпадать с указанным в README.md → Authors.
gpg --quick-generate-key "syslog-generator Release <anton@smg.org.ru>" rsa4096 cert 2y

# 2. Запомнить fingerprint primary key (40 hex chars)
GPG_FPR=$(gpg --list-secret-keys --with-colons 'syslog-generator Release' \
  | awk -F: '/^fpr:/{print $10; exit}')
echo "Main fingerprint: $GPG_FPR"
# Сохранить в password manager (наряду с passphrase).

# 3. Signing subkey (capability: Sign — используется в CI для .deb/.rpm)
gpg --quick-add-key "$GPG_FPR" rsa4096 sign 2y

# 4. Verify структуру ключа
gpg --list-secret-keys "$GPG_FPR"
# Ожидаемый вывод:
#   pub   rsa4096 2026-XX-XX [C]
#   uid              syslog-generator Release <anton@smg.org.ru>
#   ssb   rsa4096 2026-XX-XX [S]   ← signing subkey (то, что экспортируем)
# Encryption subkey (E) НЕ нужен — release signing не шифрует ничего.
```

### 1.3 Генерация revocation certificate (одновременно с primary)

```bash
# Создаётся ОДИН раз, при генерации primary key. Хранить offline.
# Это страховка на случай compromise primary key.
gpg --gen-revoke --armor --output /tmp/gpg-revocation-cert.asc "$GPG_FPR"
# gpg спросит причину:
#   0 = No reason specified (planned rotation)
#   1 = Key has been compromised
#   2 = Key is superseded (planned rotation)
#   Рекомендация: reason = 2 + комментарий "Rotation Q4 2027".

# Verify формат
gpg --list-packets /tmp/gpg-revocation-cert.asc | head -5
```

### 1.4 Export публичного ключа (для репозитория)

```bash
# 1. Export полного public keyring (primary + signing subkey)
gpg --armor --output scripts/keys/pharmacolog-release.asc --export "$GPG_FPR"

# 2. Verify формат
head -3 scripts/keys/pharmacolog-release.asc
# Ожидаемый вывод: -----BEGIN PGP PUBLIC KEY BLOCK-----
tail -3 scripts/keys/pharmacolog-release.asc
# Ожидаемый вывод: -----END PGP PUBLIC KEY BLOCK-----

# 3. Verify fingerprint после export
gpg --show-keys scripts/keys/pharmacolog-release.asc | grep -E "^(pub|uid|ssb)"
# Должно показать тот же fingerprint что и в шаге 1.2.

# 4. Commit в feature branch (sub-task 1 закрывает placeholder)
git checkout docs/issue-155-maintainer-setup-guide
git add scripts/keys/pharmacolog-release.asc
git commit -m "feat(security): replace GPG placeholder with release signing public key (Issue #155 sub-task 1)"
git push origin docs/issue-155-maintainer-setup-guide
# Open PR в main (coordination docs flow per AGENTS.md §4.3).
```

### 1.5 Export private signing subkey (для GitHub Environment)

```bash
# 1. Export ТОЛЬКО subkeys (НЕ primary secret material).
# Primary key остаётся ТОЛЬКО offline.
gpg --armor --export-secret-subkeys "$GPG_FPR" > /tmp/gpg-private-subkeys.asc

# 2. Verify export
head -3 /tmp/gpg-private-subkeys.asc
# Ожидаемый вывод: -----BEGIN PGP PRIVATE KEY BLOCK-----
gpg --list-packets /tmp/gpg-private-subkeys.asc | head -20
# Ожидается: secret-key НЕ присутствует (только secret-subkey пакеты).

# 3. Sanity check на изолированной среде
TMPHOME=$(mktemp -d)
chmod 700 "$TMPHOME"
gpg --homedir "$TMPHOME" --import /tmp/gpg-private-subkeys.asc
gpg --homedir "$TMPHOME" --list-secret-keys
# Должен быть ТОЛЬКО [S] signing subkey. Если есть primary [C] — НЕ использовать
# в GitHub secrets (это значит экспорт слишком широкий).
rm -rf "$TMPHOME"

# 4. Проверить passphrase
echo "Passphrase: stored in password manager"
```

### 1.6 Backup primary key offline

> **Critical:** primary key — ТОЛЬКО offline. Потеря без backup = невозможно
> rotate или подписать новые subkeys. Потеря revocation cert = невозможно
> revoke в emergency.

```bash
# 1. Export primary key (full, including secret material — ТОЛЬКО offline!)
gpg --armor --export-secret-keys "$GPG_FPR" > /tmp/gpg-primary-backup.asc

# 2. Backup set (3 файла + текстовая note):
#   - /tmp/gpg-primary-backup.asc          (primary key, full)
#   - /tmp/gpg-revocation-cert.asc          (revocation certificate)
#   - /tmp/gpg-private-subkeys.asc          (signing subkey, дубликат для GitHub)
#   - scripts/keys/pharmacolog-release.asc  (public key, в git)
#   - текстовый note: fingerprint + creation date + passphrase location

# 3. Записать на encrypted USB drive (LUKS/dm-crypt). НЕ в облако.
#   Хранить 2 копии: домашний safe + банковская ячейка (по желанию).

# 4. Verify backup integrity
gpg --list-packets /tmp/gpg-primary-backup.asc | head -10
```

### 1.7 Загрузка секретов в GitHub

#### Repository variable `GPG_KEY_FINGERPRINT`

```bash
gh variable set GPG_KEY_FINGERPRINT --body "$GPG_FPR" \
  --repo pharmacolog/syslog-generator

# Verify
gh variable list --repo pharmacolog/syslog-generator | grep GPG_KEY_FINGERPRINT
# Ожидаемый вывод:
#   GPG_KEY_FINGERPRINT  <40 hex chars>
```

#### GitHub Environment `release` (с required reviewers)

Через **GitHub UI** (Settings → Environments → New environment):

1. Имя: `release`.
2. **Required reviewers**: добавить `pharmacolog` (себя) для solo-maintainer policy.
3. **Deployment branches**: `Selected branches` → `v*.*.*` (только tags).
4. **Wait timer**: 0 минут (или 5 минут — optional safety margin).
5. **Prevent self-review**: **OFF** (solo-maintainer должен мочь сам approve).

Альтернативный способ через GraphQL (только для дополнительных reviewers):

```bash
# Через GraphQL — требуется "admin:org" scope.
# (CLI не поддерживает reviewer management напрямую.)
```

#### Environment secret `GPG_PRIVATE_KEY`

```bash
# Содержимое файла (ASCII armored)
gh secret set GPG_PRIVATE_KEY \
  --env release \
  --body "$(cat /tmp/gpg-private-subkeys.asc)" \
  --repo pharmacolog/syslog-generator

# Verify (UI): Settings → Environments → release → Secrets → GPG_PRIVATE_KEY
```

#### Environment secret `GPG_PASSPHRASE`

```bash
# Passphrase, использованный при gpg --quick-generate-key.
# Берётся из password manager (НЕ из истории shell).
gh secret set GPG_PASSPHRASE \
  --env release \
  --body "<passphrase-from-password-manager>" \
  --repo pharmacolog/syslog-generator
```

### 1.8 Тестирование

Локально протестировать подпись:

```bash
# Установить release build artifact (из последнего GitHub Release)
VERSION="11.4.0"
curl -fsSL -O "https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}/syslog-generator_${VERSION}_amd64.deb"

# Verify подпись
dpkg-sig --verify syslog-generator_${VERSION}_amd64.deb
# Ожидаемый вывод:
#   Processing syslog-generator_11.4.0_amd64.deb...
#   GOODSIG ... 0x<GPG_FPR_last_16_chars>

# Verify .rpm
curl -fsSL -O "https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}/syslog-generator-${VERSION}-1.x86_64.rpm"
rpm -Kv syslog-generator-${VERSION}-1.x86_64.rpm
# Ожидаемый вывод:
#   syslog-generator-11.4.0-1.x86_64.rpm:
#     Header V4 RSA/SHA256 Signature, key ID 0x<GPG_FPR_last_16_chars>: OK
```

Триггерить новый tag push для проверки CI signing (после setup):

```bash
# Tag push — триггерит packages.yml: build → sign → verify-install → release.
git tag v11.4.0-test && git push origin v11.4.0-test
# Проверить CI run:
gh run list --workflow Packages --limit 1
# Убедиться что:
#   - sign job SUCCESS (GPG_PRIVATE_KEY + GPG_PASSPHRASE загружены корректно)
#   - verify-install job SUCCESS (dpkg-sig --verify OK)
#   - release job upload: SHA256SUMS, SHA256SUMS.asc, scripts/keys/pharmacolog-release.asc
# После верификации удалить тестовый tag:
git tag -d v11.4.0-test && git push origin :refs/tags/v11.4.0-test
```

### 1.9 Rotation / Revocation runbook

#### Плановая ротация (каждые ~2 года, до истечения subkey)

```bash
# 1. Создать новый signing subkey на ТОМ ЖЕ primary
gpg --quick-add-key "$GPG_FPR" rsa4096 sign 2y
# Старый subkey оставить для overlap (dual-signing period).

# 2. Export нового subkey
gpg --armor --export-secret-subkeys "$GPG_FPR" > /tmp/gpg-private-subkeys-v2.asc
gh secret set GPG_PRIVATE_KEY \
  --env release \
  --body "$(cat /tmp/gpg-private-subkeys-v2.asc)" \
  --repo pharmacolog/syslog-generator

# 3. Update scripts/keys/pharmacolog-release.asc в репо
gpg --armor --output scripts/keys/pharmacolog-release.asc --export "$GPG_FPR"
git add scripts/keys/pharmacolog-release.asc
git commit -m "feat(security): rotate GPG release signing subkey"

# 4. Через 1-2 минорных релиза — revoke старый subkey
gpg --quick-revoke-subkey "$GPG_FPR" "<OLD_SUBKEY_FPR>"
gpg --armor --output scripts/keys/pharmacolog-release.asc --export "$GPG_FPR"
git add scripts/keys/pharmacolog-release.asc
git commit -m "feat(security): revoke old GPG signing subkey"
```

#### Emergency revocation (key compromised)

```bash
# 1. Import revocation cert (с offline backup)
gpg --import /tmp/gpg-revocation-cert.asc
# 2. Push в public keyserver (Ubuntu OpenPGP keyserver)
gpg --keyserver hkps://keys.openpgp.org --send-keys "$GPG_FPR"
# 3. Удалить secrets из GitHub Environment
gh secret delete GPG_PRIVATE_KEY --env release --repo pharmacolog/syslog-generator
gh secret delete GPG_PASSPHRASE --env release --repo pharmacolog/syslog-generator
# 4. Создать новый primary key (см. §1.2)
# 5. Security advisory (если compromise public) — см. SECURITY.md
```

---

## Часть 2: Launchpad PPA setup (sub-task 4)

### 2.1 Prerequisites

- Ubuntu One account: <https://login.ubuntu.com/>
- Подтверждённый email (через Ubuntu One dashboard).
- Launchpad account: <https://launchpad.net/> (создаётся автоматически из Ubuntu One).
- Созданный PPA: <https://launchpad.net/~/pharmacolog/+activate-ppa> → имя `syslog-generator`.
- OpenPGP-ключ, загруженный в Launchpad: <https://launchpad.net/~/pharmacolog/+editpgpkeys>.
- Локально: `sudo apt install devscripts dput`.

### 2.2 План создания PPA

1. Создать или активировать Ubuntu One account.
2. Подтвердить email.
3. Создать Launchpad account (auto через Ubuntu One).
4. Создать PPA `pharmacolog/syslog-generator`:
   - <https://launchpad.net/~/pharmacolog/+new-ppa>
   - Display name: `syslog-generator`
   - Description: `Industrial syslog load generator`
   - Default values for everything else (PPA активен для всех Ubuntu series).
5. Загрузить OpenPGP-ключ в Launchpad:
   - <https://launchpad.net/~/pharmacolog/+editpgpkeys>
   - Paste содержимое `scripts/keys/pharmacolog-release.asc`.
   - Launchpad вышлет verification email → подтвердить.
   - Fingerprint должен совпадать с `$GPG_KEY_FINGERPRINT`.
6. Проверить, что Launchpad видит ключ:
   ```bash
   gpg --keyserver keyserver.ubuntu.com --recv-keys "$GPG_FPR"
   # Verify отображается в Launchpad profile
   ```
7. Подготовить Debian source package в репозитории (см. §2.3).
8. Собрать и подписать source package:
   ```bash
   debuild -S -sa -k"$GPG_FPR"
   ```
9. Загрузить `.changes` и source artifacts через `dput`:
   ```bash
   dput ppa:pharmacolog/syslog-generator ../syslog-generator_<VERSION>-1_source.changes
   ```
10. Дождаться обработки build queue (~30 мин).
11. Проверить сборку для поддерживаемых Ubuntu releases.

### 2.3 Debian source packaging

PPA принимает **только source packages**, не pre-built `.deb` от `cargo-deb`.

Создать `debian/` директорию в репо (уже обсуждалось в Issue #107, deferred для v11.5):

```text
debian/control
debian/rules
debian/changelog
debian/source/format
```

Содержимое файлов (пример для `debian/control`):

```
Source: syslog-generator
Section: net
Priority: optional
Maintainer: Anton E. Gerasimov <anton@smg.org.ru>
Build-Depends: debhelper (>= 11), cargo (>= 1.70), rustc (>= 1.70), libssl-dev, pkg-config, dpkg-dev (>= 1.21)
Standards-Version: 4.5.1
Homepage: https://github.com/pharmacolog/syslog-generator

Package: syslog-generator
Architecture: any
Depends: ${misc:Depends}, ${shlibs:Depends}, libc6 (>= 2.31)
Description: Industrial syslog load generator
 syslog-generator — промышленный генератор нагрузки для серверов по
 протоколу Syslog на Rust.
```

Содержимое `debian/rules` (пример):

```makefile
#!/usr/bin/make -f
%:
	dh $@

override_dh_auto_build:
	cargo build --release --locked

override_dh_auto_install:
	mkdir -p debian/syslog-generator/usr/bin
	cp target/release/syslog-generator debian/syslog-generator/usr/bin/
	mkdir -p debian/syslog-generator/usr/share/man/man1
	cargo run --quiet --release --bin syslog-generator -- man | \
		gzip -9 > debian/syslog-generator/usr/share/man/man1/syslog-generator.1.gz
```

Содержимое `debian/changelog` (пример):

```
syslog-generator (11.5.0-1) unstable; urgency=medium

  * Initial PPA release (Issue #155 sub-task 4).

 -- Anton E. Gerasimov <anton@smg.org.ru>  Mon, 27 Jul 2026 12:00:00 +0300
```

Это deferred scope для отдельного PR (Issue #107 acceptance включал только
`.deb`/`.rpm` build infrastructure, не source packaging).

### 2.4 Тестирование

После успешной загрузки PPA и build queue:

```bash
# Добавить PPA
sudo add-apt-repository ppa:pharmacolog/syslog-generator
sudo apt update

# Установить из PPA
sudo apt install syslog-generator

# Verify
syslog-generator --version
syslog-generator completions bash | head -3
dpkg -s syslog-generator
```

### 2.5 Документирование

После успешного PPA setup, обновить `docs/installation.md` §2.2:

```diff
- 2.2 Через Launchpad PPA (deferred)
+ 2.2 Через Launchpad PPA (✅ active)
+
+ ```bash
+ sudo add-apt-repository ppa:pharmacolog/syslog-generator
+ sudo apt update
+ sudo apt install syslog-generator
+ ```
+
+ GPG signature verification работает автоматически (импортированный ключ
+ в `/etc/apt/keyrings/` и Launchpad).
```

---

## Часть 3: Fedora COPR setup (sub-task 5)

### 3.1 Prerequisites

- Fedora account: <https://accounts.fedoraproject.org/>
- Fedora Account System (FAS) identity.
- Доступ к COPR: <https://copr.fedorainfracloud.org/> (auto от FAS).
- Установленный `copr-cli`:
  ```bash
  sudo dnf install copr-cli
  ```
- Настроить `copr-cli` credentials:
  ```bash
  copr-cli configure
  # Ввести FAS username + password.
  # Token сохранится в ~/.config/copr.
  ```

### 3.2 План создания COPR project

1. Создать Fedora account.
2. Подтвердить email.
3. Настроить `copr-cli` (см. §3.1).
4. Создать COPR project `syslog-generator`:
   ```bash
   copr-cli create syslog-generator \
     --description "Industrial syslog load generator" \
     --homepage "https://github.com/pharmacolog/syslog-generator" \
     --contact "anton@smg.org.ru" \
     --disable_createrepo false
   ```
5. Подготовить RPM spec file (`.copr/syslog-generator.spec` или в корне).
6. Собрать source RPM локально:
   ```bash
   rpmbuild -bs packaging/syslog-generator.spec
   # Результат: ~/rpmbuild/SRPMS/syslog-generator-<VERSION>-1.src.rpm
   ```
7. Запустить COPR build:
   ```bash
   copr-cli build syslog-generator \
     ~/rpmbuild/SRPMS/syslog-generator-${VERSION}-1.src.rpm
   ```
8. Включить требуемые chroots:
   ```bash
   copr-cli edit-package-system syslog-generator \
     --packagetype rpm \
     --chroot fedora-rawhide-x86_64 \
     --chroot fedora-40-x86_64 \
     --chroot fedora-39-x86_64 \
     --chroot epel-9-x86_64 \
     --chrock epel-9-aarch64
   ```
9. Дождаться пересборки пакета COPR.
10. Проверить установку через `dnf`.

### 3.3 RPM spec file

COPR rebuilds from source — spec file обязателен.

Пример `packaging/rpm/syslog-generator.spec`:

```spec
Name:           syslog-generator
Version:        11.5.0
Release:        1%{?dist}
Summary:        Industrial syslog load generator written in Rust
License:        ASL 2.0
URL:            https://github.com/pharmacolog/syslog-generator
Source0:        https://github.com/pharmacolog/syslog-generator/archive/v%{version}.tar.gz

BuildRequires:  cargo, rust, openssl-devel, pkgconfig, systemd
Requires:       openssl-libs

%description
Industrial syslog load generator written in Rust.

%prep
%autosetup -n syslog-generator-%{version}

%build
cargo build --release --locked

%install
install -m755 -D target/release/syslog-generator %{buildroot}%{_bindir}/syslog-generator
install -m644 -D packaging/rpm/syslog-generator.1 %{buildroot}%{_mandir}/man1/syslog-generator.1
gzip -9 %{buildroot}%{_mandir}/man1/syslog-generator.1

%files
%license LICENSE
%doc README.md docs/installation.md
%{_bindir}/syslog-generator
%{_mandir}/man1/syslog-generator.1.gz
```

Это deferred scope для отдельного PR (Issue #107 acceptance не покрывал RPM
spec generation для COPR).

### 3.4 Тестирование

```bash
# Включить COPR repo
sudo dnf copr enable pharmacolog/syslog-generator
sudo dnf update

# Установить из COPR
sudo dnf install syslog-generator

# Verify
syslog-generator --version
rpm -K $(rpm -q syslog-generator)
rpm -qi syslog-generator
```

### 3.5 Документирование

Обновить `docs/installation.md` §3.2:

```diff
- 3.2 Через Fedora COPR (deferred)
+ 3.2 Через Fedora COPR (✅ active)
+
+ ```bash
+ sudo dnf copr enable pharmacolog/syslog-generator
+ sudo dnf install syslog-generator
+ ```
+
+ GPG signature verification работает автоматически (RPM metadata подписана
+ ключом COPR — chain of trust).
```

---

## Часть 4: Alpine .apk (sub-task 6 — optional)

### 4.1 Prerequisites

- Alpine Linux build environment (musl libc).
- `apk-tools` (abuild, apk).
- Rust target: `rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`.

### 4.2 Реализация

```bash
# 1. Добавить Rust target для musl
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl

# 2. Cross-compile
cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target aarch64-unknown-linux-musl

# 3. Создать APKBUILD в корне репо (alpine/)
# Содержимое APKBUILD — пример:
```

`alpine/APKBUILD`:

```apkb
# Contributor: Anton E. Gerasimov <anton@smg.org.ru>
# Maintainer: Anton E. Gerasimov <anton@smg.org.ru>
pkgname=syslog-generator
pkgver=11.5.0
pkgrel=0
pkgdesc="Industrial syslog load generator"
url="https://github.com/pharmacolog/syslog-generator"
arch="x86_64 aarch64"
license="ASL-2.0"
depends=""
makedepends="cargo rust openssl-dev pkgconfig abuild"
source="$pkgname-$pkgver.tar.gz::https://github.com/pharmacolog/syslog-generator/archive/v$pkgver.tar.gz"
builddir="$srcdir/syslog-generator-$pkgver"

build() {
    cd "$builddir"
    case "$CARCH" in
        x86_64) RUST_TARGET=x86_64-unknown-linux-musl ;;
        aarch64) RUST_TARGET=aarch64-unknown-linux-musl ;;
    esac
    cargo build --release --locked --target $RUST_TARGET
}

package() {
    install -m755 -D "$builddir/target/$RUST_TARGET/release/syslog-generator" \
        "$pkgdir/usr/bin/syslog-generator"
    install -m644 -D "$builddir/packaging/alpine/syslog-generator.1" \
        "$pkgdir/usr/share/man/man1/syslog-generator.1"
    gzip -9 "$pkgdir/usr/share/man/man1/syslog-generator.1"
}

check() {
    "$builddir/target/$RUST_TARGET/release/syslog-generator" --version
}
```

### 4.3 Тестирование

```bash
# Сборка на Alpine
docker run --rm -v "$(pwd):/src" -w /src alpine:latest sh -c "
    apk add abuild openssl-dev cargo rust git
    abuild-keygen -an -n packager
    abuild -r -c
"

# Проверка пакета
apk info syslog-generator
apk add --allow-untrusted /src/alpine/syslog-generator-*.apk
syslog-generator --version
```

### 4.4 Decision: deferred

Alpine .apk оставлен **deferred** — musl build + APKBUILD + Alpine build environment
требуют значительного maintenance overhead без подтверждённого user demand.
Для solo maintainer рекомендуется начинать с Debian/RHEL PPA/COPR.

---

## Acceptance checklist (overall)

| Sub-task | Action | Verification | Status |
|---|---|---|---|
| **1** | Generate GPG release signing subkey + replace placeholder | `dpkg-sig --verify` OK, `rpm -K` OK | ⬜ Pending (PR #156 closed it as scaffold) |
| **2** | sign step в packages.yml | CI signing job success | ✅ Scaffold in PR #156 |
| **3** | verify-install hardening | CI verify-install OK | ✅ PR #156 |
| **4** | Launchpad PPA setup | `apt install` from PPA works | ⬜ Manual setup required |
| **5** | Fedora COPR setup | `dnf install` from COPR works | ⬜ Manual setup required |
| **6** | Alpine .apk | `apk add` works | ⬜ Deferred (no user demand) |

После завершения sub-tasks 4-5 — milestone v11.4 можно считать полностью закрытым.

---

## Cross-references

- [Issue #155](https://github.com/pharmacolog/syslog-generator/issues/155) — tracking issue.
- [Issue #107](https://github.com/pharmacolog/syslog-generator/issues/107) — closed via PR #154 (basic packages).
- [PR #156](https://github.com/pharmacolog/syslog-generator/pull/156) — scaffold merge.
- [AGENTS.md §2](AGENTS.md) — GitHub auth bootstrap.
- [AGENTS.md §10](AGENTS.md) — pre-PR gate-check.
- [AGENTS.md §15](AGENTS.md) — board sync принцип (обновлять после каждого действия).
- `docs/installation.md` — user-facing install docs.
- `docs/distribution-channels.md` — distribution channels overview.
