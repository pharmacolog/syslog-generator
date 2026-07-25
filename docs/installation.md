# Installation

> **Версия документа**: pinned to **milestone v11.4** (Issue #107, label `track-gtm`).
> Подходит для релизов ветки `v11.4.x`. Для более ранних версий используйте
> [README.md → Установка из исходников](../README.md#-установка).

`syslog-generator` распространяется в виде нативных Linux-пакетов
(`.deb` / `.rpm`), Docker-образа (multi-arch `linux/amd64` + `linux/arm64`)
и crate на [crates.io](https://crates.io/crates/syslog-generator). Этот документ
описывает установку и проверку на production-серверах под управлением apt и dnf.

**Трек:** Issue
[#107](https://github.com/pharmacolog/syslog-generator/issues/107) —
[GTM-2] Linux-пакеты .deb/.rpm через `cargo-deb` / `cargo-rpm`.
PR: [#146](https://github.com/pharmacolog/syslog-generator/pull/146)
(coordination docs / AGENTS.md v2.0; см. историю изменений внизу).

**Связанные документы:**
[README.md](../README.md) ·
[docs/USER_GUIDE.md](USER_GUIDE.md) ·
[docs/CLI_REFERENCE.md](CLI_REFERENCE.md) ·
[docs/PERFORMANCE.md](PERFORMANCE.md) ·
[docs/MIGRATION.md](MIGRATION.md) ·
[AGENTS.md](../AGENTS.md) (single source of truth; `docs/COORDINATION.md` —
**deprecated since 2026-07-24**).

---

## Оглавление

1. [Поддерживаемые дистрибутивы](#1-поддерживаемые-дистрибутивы)
2. [Ubuntu / Debian (.deb)](#2-ubuntu--debian-deb)
3. [Fedora / RHEL / Rocky (.rpm)](#3-fedora--rhel--rocky-rpm)
4. [Alpine (.apk)](#4-alpine-apk)
5. [Сборка из исходников / `cargo install`](#5-сборка-из-источников--cargo-install)
6. [Автодополнение shell](#6-автодополнение-shell)
7. [Проверка установки (verification)](#7-проверка-установки-verification)
8. [Удаление (uninstall)](#8-удаление-uninstall)
9. [Troubleshooting](#9-troubleshooting)
10. [Кросс-ссылки и история](#10-кросс-ссылки-и-история)

---

## 1. Поддерживаемые дистрибутивы

| Дистрибутив | Пакет | Статус | Канал |
|---|---|---|---|
| Ubuntu 22.04 LTS (jammy) | `.deb` | ✅ supported | GitHub Releases |
| Ubuntu 24.04 LTS (noble) | `.deb` | ✅ supported | GitHub Releases + PPA (planned) |
| Debian 12 (bookworm) | `.deb` | ✅ supported | GitHub Releases |
| Debian 13 (trixie) | `.deb` | ✅ supported | GitHub Releases |
| Fedora 39 / 40 / 41 | `.rpm` | ✅ supported | GitHub Releases + COPR (planned) |
| RHEL 9 / Rocky Linux 9 | `.rpm` | ✅ supported | GitHub Releases |
| AlmaLinux 9 | `.rpm` | ✅ supported | GitHub Releases |
| openSUSE Leap / Tumbleweed | `.rpm` | ⚠️ experimental | GitHub Releases |
| Alpine 3.19+ | `.apk` | ❌ **не поддерживается** | — (см. §4) |
| Arch Linux | AUR | ❌ не поддерживается официально | — |
| macOS / Windows | — | ⚠️ только из исходников | [README.md](../README.md#-установка) |

Архитектуры: `x86_64` (amd64) и `aarch64` (arm64).
Динамическая линковка с `glibc` ≥ 2.31 (Ubuntu 20.04+, RHEL 9+, Debian 11+).
Musl-билды для Alpine — **не предоставляются** (см. §4).

CI matrix (см. `docs/ROADMAP.md` → Веха G → v11.4):
`debian: [bookworm, noble] × arch: [amd64, arm64]` и
`fedora: [39, 40] × arch: [x86_64, aarch64]`.

---

## 2. Ubuntu / Debian (.deb)

### 2.1 Установка из GitHub Releases (рекомендуется)

```bash
# 1. Скачать .deb для нужной архитектуры (подставить версию milestone v11.4)
VERSION="11.4.0"
ARCH="amd64"        # или "arm64"
curl -fsSL -O "https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}/syslog-generator_${VERSION}_${ARCH}.deb"

# 2. Локальная установка через apt (разрешает зависимости, регистрирует в dpkg)
sudo apt install "./syslog-generator_${VERSION}_${ARCH}.deb"

# 3. Verify
syslog-generator --version
```

`apt install ./file.deb` (а не `dpkg -i`) подтягивает зависимости автоматически
(текущая сборка требует только `libc6`, остальное — `$auto` из
`Cargo.toml → [package.metadata.deb]`).

### 2.2 Через Launchpad PPA (deferred)

> **⚠️ Deferred:** канал `ppa:pharmacolog/syslog-generator` ещё не создан.
> Отслеживание: Issue
> [#155](https://github.com/pharmacolog/syslog-generator/issues/155)
> (placeholder, связан с Issue #107). После публикации PPA этот раздел
> будет дополнен командами:
>
> ```bash
> sudo add-apt-repository ppa:pharmacolog/syslog-generator
> sudo apt update
> sudo apt install syslog-generator
> ```

Подписаться на обновление: GitHub Watch → `Releases only` для репозитория
[pharmacolog/syslog-generator](https://github.com/pharmacolog/syslog-generator).

### 2.3 Обновление

```bash
# При использовании GitHub Releases: повторить §2.1 с новой VERSION
# При использовании PPA (когда будет): обычный apt upgrade
sudo apt update && sudo apt upgrade syslog-generator
```

---

## 3. Fedora / RHEL / Rocky (.rpm)

### 3.1 Установка из GitHub Releases (рекомендуется)

```bash
# 1. Скачать .rpm для нужной архитектуры
VERSION="11.4.0"
ARCH="x86_64"       # или "aarch64"
curl -fsSL -O "https://github.com/pharmacolog/syslog-generator/releases/download/v${VERSION}/syslog-generator-${VERSION}-1.${ARCH}.rpm"

# 2. Локальная установка через dnf (разрешает зависимости)
sudo dnf install "./syslog-generator-${VERSION}-1.${ARCH}.rpm"

# 3. Verify
syslog-generator --version
```

`dnf install ./file.rpm` (а не `rpm -i`) подтягивает зависимости и обновляет
базу rpm. Для RHEL 9 / Rocky 9 / AlmaLinux 9 синтаксис идентичен (dnf есть
из коробки).

### 3.2 Через Fedora COPR (deferred)

> **⚠️ Deferred:** канал
> `copr.fedoraproject.org/coprs/pharmacolog/syslog-generator` ещё не создан.
> Отслеживание: Issue
> [#155](https://github.com/pharmacolog/syslog-generator/issues/155)
> (placeholder, общий с PPA). Команды после публикации COPR:
>
> ```bash
> sudo dnf copr enable pharmacolog/syslog-generator
> sudo dnf install syslog-generator
> ```

Альтернативный путь для RHEL-семейства — EPEL + ручной `.rpm` (см. §3.1).

### 3.3 Обновление

```bash
# При использовании GitHub Releases: повторить §3.1 с новой VERSION
# При использовании COPR (когда будет): обычный dnf upgrade
sudo dnf upgrade syslog-generator
```

---

## 4. Alpine (.apk)

> **❌ Alpine-пакет официально не поддерживается** в milestone v11.4.
>
> Причина: основная сборка линкуется динамически с `glibc` (через
> `cargo-deb` / `cargo-rpm`). Alpine использует `musl`, что требует
> отдельного `--target x86_64-unknown-linux-musl` билда + проверки всех
> нативных зависимостей (rustls → ring, kafka-lz4/zstd, file rotation).
>
> **Tracking**: Issue
> [#155](https://github.com/pharmacolog/syslog-generator/issues/155)
> (placeholder; deferred до получения ≥ 10 запросов от пользователей).
>
> **Workaround**: запускать под Alpine через Docker-образ
> `ghcr.io/pharmacolog/syslog-generator:v11.4.0` (multi-arch: linux/amd64 +
> linux/arm64; см. [README.md → Docker](../README.md#docker)):
>
> ```bash
> docker run --rm ghcr.io/pharmacolog/syslog-generator:v11.4.0 --version
> ```

Если вам нужен нативный Alpine-пакет — откройте issue с label `track-gtm`,
опишите use-case (load-testing на Alpine-based routers / WAF), и мы
приоритизируем для milestone v11.5+.

---

## 5. Сборка из исходников / `cargo install`

Подходит для разработчиков, непрерывной интеграции, или дистрибутивов без
готовых пакетов.

### 5.1 Зависимости

| Компонент | Минимум | Проверка |
|---|---|---|
| Rust toolchain (stable) | ≥ 1.95 (MSRV из `Cargo.toml`) | `rustc --version` |
| `cargo` | bundled с rustup | `cargo --version` |
| `pkg-config` | любой | `pkg-config --version` |
| OpenSSL headers (`libssl-dev`) | только если включаете TLS-vendor | `pkg-config --modversion openssl` |
| `build-essential` / `gcc` | для FFI (rustls-ring) | `gcc --version` |
| `cmake` | ≥ 3.16 (для некоторых нативных крейтов) | `cmake --version` |

### 5.2 Установка Rust

```bash
# Через rustup (рекомендуется)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup install stable
```

### 5.3 Сборка release

```bash
# Клонирование
git clone https://github.com/pharmacolog/syslog-generator.git
cd syslog-generator
git checkout v11.4.0      # или "main" для latest

# Release с LTO (рекомендуется; ~3-5 мин компиляции)
cargo build --release

# Бинарь: ./target/release/syslog-generator
./target/release/syslog-generator --version
```

### 5.4 `cargo install` (глобальная установка)

```bash
# Из crates.io (после публикации milestone v11.4)
cargo install syslog-generator

# С дополнительными features
cargo install syslog-generator --features kafka

# Из Git (latest main)
cargo install --git https://github.com/pharmacolog/syslog-generator --tag v11.4.0

# Из локального клона (для разработки)
git clone https://github.com/pharmacolog/syslog-generator.git
cd syslog-generator
cargo install --path .

# Бинарь: ~/.cargo/bin/syslog-generator (добавить в PATH)
export PATH="$HOME/.cargo/bin:$PATH"
syslog-generator --version
```

### 5.5 Opt-in фичи

| Фича | Флаг | Размер билда | Use-case |
|---|---|---|---|
| Kafka / Redpanda transport | `--features kafka` | +~8 MiB | Нагрузка на Kafka-брокеры |
| Test-helpers | `--features test-helpers` | +~2 MiB | Только для integration-тестов |

```bash
cargo build --release --features kafka
cargo install syslog-generator --features kafka
```

Без фич — бинарь содержит `file` / `tcp` / `udp` / `tls` транспорты и
все 7 форматов (RFC 5424 / RFC 3164 / raw / protobuf / CEF / LEEF / NDJSON).
См. [docs/USER_GUIDE.md](USER_GUIDE.md) §3.

### 5.6 Docker (multi-arch)

```bash
docker pull ghcr.io/pharmacolog/syslog-generator:v11.4.0
docker run --rm ghcr.io/pharmacolog/syslog-generator:v11.4.0 --version
```

Полный стек (syslog-generator + syslog-ng + Prometheus + Grafana):
см. [README.md → Docker](../README.md#docker).

---

## 6. Автодополнение shell

Подкоманда `completions` встроена в CLI начиная с v10.6.0 (см.
[docs/CLI_REFERENCE.md](CLI_REFERENCE.md) §3). Поддерживаются `bash`, `zsh`,
`fish`, `powershell`, `elvish`.

### 6.1 bash

```bash
# Системный путь (рекомендуется при установке через .deb/.rpm)
sudo syslog-generator completions bash > /usr/share/bash-completion/completions/syslog-generator

# Пользовательский путь (без sudo)
syslog-generator completions bash > ~/.local/share/bash-completion/completions/syslog-generator

# Активация в текущей сессии
source <(syslog-generator completions bash)
```

### 6.2 zsh

```bash
# В директорию из $fpath (обычно /usr/share/zsh/site-functions/)
sudo syslog-generator completions zsh > "${fpath[1]}/_syslog-generator"

# Или через oh-my-zsh custom
syslog-generator completions zsh > "$HOME/.oh-my-zsh/custom/completions/_syslog-generator"

# Перезагрузить compinit
autoload -Uz compinit && compinit
```

### 6.3 fish

```bash
# Системный
sudo syslog-generator completions fish > /usr/share/fish/vendor_completions.d/syslog-generator.fish

# Пользовательский
syslog-generator completions fish > ~/.config/fish/completions/syslog-generator.fish
```

### 6.4 Автоустановка после `apt install ./syslog-generator.deb`

`postinst`-скрипт пакета (см. `debian-scripts/postinst`) регистрирует placeholder
для bash-completions. Если директория `/usr/share/bash-completion/completions`
существует, выполните вручную:

```bash
sudo syslog-generator completions bash | sudo tee /usr/share/bash-completion/completions/syslog-generator > /dev/null
```

Аналогично для zsh и fish (см. §6.2 / §6.3). Для RPM-пакета auto-registration
добавлен в milestone v11.5 (см. Issue #107, sub-task 2).

---

## 7. Проверка установки (verification)

### 7.1 Базовая проверка версии

```bash
syslog-generator --version
# Ожидаемый вывод:
# syslog-generator 11.4.0

syslog-generator --help
# Должен показать секции: Usage, Options, Commands (с completions/man).
```

### 7.2 Smoke test (UDP localhost)

```bash
# Терминал 1: слушать UDP на 127.0.0.1:5514
nc -ul 127.0.0.1 5514

# Терминал 2: отправить 10 сообщений
syslog-generator \
  -t 127.0.0.1:5514:udp \
  -m '<165>1 {{timestamp}} smoke-{{sequence}} syslog-generator[1]: verify install' \
  --rate 10 --total 10 --seed 42
# Ожидаемый exit code: 0
```

### 7.3 Smoke test (файл с rotation)

```bash
# Записать 1000 сообщений в /tmp/sg-smoke.log
syslog-generator \
  -t /tmp/sg-smoke.log:file \
  -m 'smoke line {{sequence}}' \
  --rate 100 --total 1000 --format raw
wc -l /tmp/sg-smoke.log
# Ожидаемый вывод: 1000 /tmp/sg-smoke.log
```

### 7.4 JSON-профиль (validation через --validate)

```bash
syslog-generator --validate --schema-strict --profile examples/multi_target_roundrobin.json
# Ожидаемый exit code: 0 (без ошибок JSON Schema)
```

### 7.5 Метрики Prometheus

```bash
# Запуск на 30 секунд с экспортом метрик
syslog-generator \
  --profile examples/multi_target_roundrobin.json \
  --metrics-addr 127.0.0.1:9090 --duration 30 &

# Через 5 секунд: проверить endpoint
curl -s http://127.0.0.1:9090/metrics | head -20
# Ожидаемые метрики: sg_messages_sent_total, sg_bytes_sent_total, ...
```

### 7.6 Авто-дополнение

```bash
# bash
source <(syslog-generator completions bash) && type _syslog-generator
# zsh — аналогично
```

Если любой из шагов падает — см. §9 (Troubleshooting) или откройте issue с
полным выводом (`syslog-generator --version`, `uname -a`, логи).

### 7.7 Проверка GPG-подписи (начиная с v11.5+)

> **Доступно с milestone v11.5+** (Issue #155, sub-tasks 1–3). До публикации
> GPG-ключа артефакты **не подписаны** — `dpkg-sig --verify` вернёт warning
> вместо success. Это нормально для milestone v11.4.x.

#### 7.7.1 Импорт публичного ключа

Один раз на машине (или в проде-инфраструктуре):

```bash
# Вариант A: прямой download с GitHub (main branch — обновляется при
# sub-task 1 завершении)
sudo mkdir -p /etc/apt/keyrings /etc/pki/rpm-gpg
sudo curl -fsSL -o /etc/apt/keyrings/pharmacolog-release.asc \
  https://raw.githubusercontent.com/pharmacolog/syslog-generator/main/scripts/keys/pharmacolog-release.asc
sudo curl -fsSL -o /etc/pki/rpm-gpg/RPM-GPG-KEY-pharmacolog \
  https://raw.githubusercontent.com/pharmacolog/syslog-generator/main/scripts/keys/pharmacolog-release.asc

# Вариант B: из локального клона
git clone https://github.com/pharmacolog/syslog-generator.git
sudo cp syslog-generator/scripts/keys/pharmacolog-release.asc \
  /etc/apt/keyrings/pharmacolog-release.asc
```

Для Fedora/RHEL (после sub-task 1):

```bash
sudo rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-pharmacolog
```

#### 7.7.2 Verify .deb (после скачивания, до установки)

```bash
# Установить утилиту (один раз)
sudo apt install dpkg-sig

# Verify
dpkg-sig --verify syslog-generator_11.5.0_amd64.deb
# Ожидаемый вывод (с подписью):
#   Processing syslog-generator_11.5.0_amd64.deb...
#   GOODSIG ... 0xPHARMACOLOG_RELEASE_FINGERPRINT
```

#### 7.7.3 Verify .rpm (после скачивания, до установки)

```bash
rpm -K syslog-generator-11.5.0-1.x86_64.rpm
# Ожидаемый вывод:
#   syslog-generator-11.5.0-1.x86_64.rpm: digests signatures OK

# Verbose (покажет key ID и подписанта)
rpm -Kv syslog-generator-11.5.0-1.x86_64.rpm
```

#### 7.7.4 Полная цепочка verify (PPA/COPR с подписью)

После sub-task 4 (Launchpad PPA) / sub-task 5 (Fedora COPR) — `apt install`
и `dnf install` через эти каналы автоматически проверяют подпись по ключу
из `/etc/apt/keyrings/` или `/etc/pki/rpm-gpg/`. Никаких дополнительных
действий не требуется.

### 7.8 SHA256SUMS manifest

Начиная с v11.5+ каждый GitHub Release содержит `SHA256SUMS` + detached
`.sig` (см. §10.5). Проверка:

```bash
# Скачать всё
curl -fsSL -O https://github.com/pharmacolog/syslog-generator/releases/download/v11.5.0/{SHA256SUMS,SHA256SUMS.sig,syslog-generator_11.5.0_amd64.deb}

# Verify manifest (GPG signature)
gpg --verify SHA256SUMS.sig SHA256SUMS

# Verify checksums
sha256sum -c SHA256SUMS --ignore-missing
```

---

## 8. Удаление (uninstall)

### 8.1 Ubuntu / Debian (.deb)

```bash
# Сохранить конфиги (если есть в /etc/syslog-generator)
sudo apt remove syslog-generator

# Полное удаление вместе с конфигами
sudo apt purge syslog-generator
sudo apt autoremove
```

### 8.2 Fedora / RHEL (.rpm)

```bash
sudo dnf remove syslog-generator
# Или
sudo rpm -e syslog-generator
```

### 8.3 `cargo install`

```bash
cargo uninstall syslog-generator
```

### 8.4 Сборка из исходников

```bash
# Удалить бинарь и клон
rm -rf ~/.cargo/bin/syslog-generator
rm -rf /path/to/syslog-generator
```

### 8.5 Docker

```bash
docker rmi ghcr.io/pharmacolog/syslog-generator:v11.4.0
```

### 8.6 Очистка shell completions (опционально)

```bash
sudo rm -f /usr/share/bash-completion/completions/syslog-generator
sudo rm -f /usr/share/zsh/site-functions/_syslog-generator
sudo rm -f /usr/share/fish/vendor_completions.d/syslog-generator.fish
```

---

## 9. Troubleshooting

### 9.1 `error while loading shared libraries: libssl.so.3` / `libcrypto.so.3`

**Причина:** пакет собран с динамической линковкой OpenSSL 3.x, а в системе
только OpenSSL 1.1 (RHEL 9 имеет OpenSSL 3, но RHEL 8 / Ubuntu 20.04 — нет).

**Решение:**

```bash
# RHEL 9 / Rocky 9 / Fedora 39+ — должно работать из коробки
sudo dnf install openssl

# Ubuntu 22.04+ — OpenSSL 3 по умолчанию
sudo apt install libssl3

# Ubuntu 20.04 — обновиться до 22.04 LTS, или собрать из исходников (§5)
```

Если пакет был собран с rustls (pure Rust) — зависимости от OpenSSL нет.
Проверка:

```bash
ldd $(which syslog-generator) | grep -E 'ssl|crypto'
# Если пусто — rustls, проблема в другом.
```

### 9.2 GPG signature warning при `dnf install ./file.rpm`

**Симптом:**

```
The downloaded packages were not signed or the signature could not be verified.
```

**Причина:** GitHub Release артефакт подписан (GPG-ключ проекта), но локальный
rpm-db не знает публичный ключ.

**Решение (временное, для тестирования):**

```bash
sudo dnf install ./syslog-generator-11.4.0-1.x86_64.rpm --nogpgcheck
```

**Решение (постоянное, для production):**

```bash
# 1. Импортировать GPG-ключ проекта (URL — см. README.md → Security)
sudo rpm --import https://github.com/pharmacolog/syslog-generator/raw/main/scripts/keys/pharmacolog-release.asc

# 2. Verify подпись вручную
rpm -K syslog-generator-11.4.0-1.x86_64.rpm
# Ожидаемый вывод: ... digests signatures OK

# 3. После импорта — обычный dnf install
sudo dnf install ./syslog-generator-11.4.0-1.x86_64.rpm
```

### 9.3 `apt` ругается на unsigned `.deb`

**Симптом:**

```
WARNING: The following packages cannot be authenticated!
```

**Решение:**

```bash
# 1. Проверить подпись через dpkg-sig (если установлен)
sudo apt install dpkg-sig
dpkg-sig --verify syslog-generator_11.4.0_amd64.deb

# 2. Если подпись корректна — установить с подтверждением
sudo apt install -y ./syslog-generator_11.4.0_amd64.deb
```

### 9.4 `bash: syslog-generator: command not found` после установки пакета

**Причина:** shell hash не обновлён, или пакет установился в `/usr/local/bin`,
которого нет в `$PATH` для текущего пользователя.

**Решение:**

```bash
# Проверить, куда установился бинарь
dpkg -L syslog-generator | grep bin        # Debian/Ubuntu
rpm -ql syslog-generator | grep bin         # Fedora/RHEL

# Перезагрузить PATH
hash -r
exec $SHELL -l

# Если путь нестандартный — добавить в ~/.bashrc
echo 'export PATH="/usr/local/bin:$PATH"' >> ~/.bashrc
```

### 9.5 `cannot find -lssl` / `pkg-config` errors при сборке

**Решение (Ubuntu/Debian):**

```bash
sudo apt install build-essential pkg-config libssl-dev cmake
```

**Решение (Fedora/RHEL):**

```bash
sudo dnf groupinstall "Development Tools"
sudo dnf install pkg-config openssl-devel cmake
```

### 9.6 Segmentation fault при старте

**Симптом:** `syslog-generator --version` падает с `Segmentation fault`
сразу после установки.

**Решение:**

```bash
# 1. Проверить архитектуру
uname -m
# Ожидается: x86_64 или aarch64
# Если i386 — скачать правильный .deb/.rpm

# 2. Проверить glibc version
ldd --version | head -1
# Должно быть ≥ 2.31 (glibc 2.31 = Debian 11, Ubuntu 20.04)

# 3. Если всё ОК — собрать debug-версию для crash-репорта
RUST_BACKTRACE=full syslog-generator --version > /tmp/sg-crash.log 2>&1
# И приложить /tmp/sg-crash.log к issue.
```

### 9.7 Auto-completion не работает в bash

**Решение:**

```bash
# 1. Проверить, что файл создан
ls -l /usr/share/bash-completion/completions/syslog-generator

# 2. Проверить, что bash-completion загружен
type _syslog-generator

# 3. Если нет — перезагрузить
source /etc/profile
exec bash

# 4. Для пользовательского пути
mkdir -p ~/.local/share/bash-completion/completions
syslog-generator completions bash > ~/.local/share/bash-completion/completions/syslog-generator
source ~/.local/share/bash-completion/completions/syslog-generator
```

### 9.8 `Permission denied` при записи в `/var/log/...`

**Симптом:** profile с `address: /var/log/syslog-load.log` падает с permission error.

**Решение:**

```bash
# Вариант A: запустить под отдельным пользователем
sudo useradd -r -s /usr/sbin/nologin syslog-gen
sudo -u syslog-gen syslog-generator --profile load-to-varlog.json

# Вариант B: изменить путь на пользовательский
# В профиле: address: /tmp/syslog-load.log

# Вариант C: capabilities (для CAP_DAC_OVERRIDE)
sudo setcap cap_dac_override+ep $(which syslog-generator)
```

### 9.9 Конфликт с ранее установленной версией

**Решение:**

```bash
# Ubuntu/Debian
sudo apt remove syslog-generator
sudo apt install ./syslog-generator_11.4.0_amd64.deb

# Fedora/RHEL
sudo dnf remove syslog-generator
sudo dnf install ./syslog-generator-11.4.0-1.x86_64.rpm
```

### 9.10 Прочие проблемы

Если ни один из сценариев не помог:

1. Собрать диагностический bundle:
   ```bash
   syslog-generator --version > /tmp/sg-diag.txt 2>&1
   uname -a >> /tmp/sg-diag.txt
   cat /etc/os-release >> /tmp/sg-diag.txt
   ldd $(which syslog-generator) >> /tmp/sg-diag.txt
   RUST_LOG=debug syslog-generator --profile your.json --rate 1 --total 1 >> /tmp/sg-diag.txt 2>&1
   ```
2. Открыть issue на
   [github.com/pharmacolog/syslog-generator/issues/new](https://github.com/pharmacolog/syslog-generator/issues/new)
   с label `bug` и приложить `/tmp/sg-diag.txt`.

См. также [SECURITY.md](../SECURITY.md) для ответственного раскрытия уязвимостей.

### 9.11 `apt`/`dnf` ругается на signature verification failed

**Симптом (apt):**

```
The following signatures couldn't be verified because the public key is not available: NO_PUBKEY 0xPHARMACOLOG_RELEASE_FINGERPRINT
```

**Симптом (dnf):**

```
The GPG keys listed for the package ... are not configured: ... NO_PUBKEY 0xPHARMACOLOG_RELEASE_FINGERPRINT
Public key for ... is not installed
```

**Причина:** публичный GPG-ключ проекта не импортирован в системе.

**Решение:**

```bash
# 1. Скачать и импортировать ключ (один раз)
sudo curl -fsSL -o /etc/apt/keyrings/pharmacolog-release.asc \
  https://raw.githubusercontent.com/pharmacolog/syslog-generator/main/scripts/keys/pharmacolog-release.asc

# Ubuntu/Debian (.deb):
sudo apt-key add /etc/apt/keyrings/pharmacolog-release.asc 2>/dev/null || \
  sudo gpg --dearmor < /etc/apt/keyrings/pharmacolog-release.asc \
    | sudo tee /etc/apt/keyrings/pharmacolog-release.gpg > /dev/null

# Fedora/RHEL (.rpm):
sudo rpm --import /etc/apt/keyrings/pharmacolog-release.asc

# 2. Verify вручную (до установки)
dpkg-sig --verify syslog-generator_11.5.0_amd64.deb    # apt
rpm -K syslog-generator-11.5.0-1.x86_64.rpm            # dnf

# 3. После импорта — обычная установка
sudo apt install ./syslog-generator_11.5.0_amd64.deb
sudo dnf install ./syslog-generator-11.5.0-1.x86_64.rpm
```

**Workaround (НЕ рекомендуется для production):**

```bash
# Только для разовых тестов:
sudo apt install --allow-unauthenticated ./syslog-generator_11.5.0_amd64.deb
sudo dnf install --nogpgcheck ./syslog-generator-11.5.0-1.x86_64.rpm
```

**Где взять fingerprint ключа:** см. README.md → Security, либо
[`scripts/keys/pharmacolog-release.asc`](../scripts/keys/pharmacolog-release.asc)
после публикации sub-task 1.

---

## 10. Кросс-ссылки и история

### 10.1 Связанные документы

| Документ | Назначение |
|---|---|
| [README.md](../README.md) | Top-level обзор + install badge (планируется в milestone v11.5) |
| [docs/USER_GUIDE.md](USER_GUIDE.md) | Полное руководство пользователя |
| [docs/CLI_REFERENCE.md](CLI_REFERENCE.md) | Все флаги и subcommands (`completions`, `man`) |
| [docs/PERFORMANCE.md](PERFORMANCE.md) | Бенчмарки, профилирование (LTO, PGO, fat/thin) |
| [docs/DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) | Архитектура + как собрать кастомный пакет |
| [docs/MIGRATION.md](MIGRATION.md) | Breaking changes между версиями |
| [docs/ROADMAP.md](ROADMAP.md) | План релизов v11.x → v13.x (Вехи G, H, I) |
| [AGENTS.md](../AGENTS.md) | Single source of truth для AI-агентов |
| [docs/COORDINATION.md](COORDINATION.md) | **Deprecated since 2026-07-24** → см. AGENTS.md |

### 10.2 История изменений

| Версия | Дата | Изменения |
|---|---|---|
| **v11.4.0** (Issue #107) | TBD | Первая публикация `.deb` / `.rpm` через `cargo-deb` / `cargo-rpm`. CI matrix: `debian×2×arch×2` + `fedora×2×arch×2`. GPG-подпись deferred (Issue #155). Documentation: этот файл. |
| PR #146 | 2026-07-24 | Coordination docs / AGENTS.md v2.0 (контекст для этого документа; см. AGENTS.md §10 — coordination docs flow). |

### 10.3 Связанные issues

- **Issue #107** —
  [GTM-2] Linux-пакеты .deb/.rpm через `cargo-deb` / `cargo-rpm`
  (milestone v11.4, track `track-gtm`, priority `p2`).
- **Issue #155** (placeholder) — PPA / COPR / Alpine `.apk` distribution
  channels (deferred; см. §2.2, §3.2, §4).
- **Issue #92** — B3 Presets (зависимость Issue #107: default config в пакете).

### 10.5 GPG-подпись и supply-chain integrity

> **Roadmap:** полная подпись артефактов — milestone v11.5+ (Issue #155).
> В v11.4.x артефакты публикуются **без подписи** (deferred scope PR #154).

#### Что подписывается

| Артефакт | Подпись | Где проверять |
|---|---|---|
| `syslog-generator_*.deb` | `dpkg-sig --sign builder` (origin + builder роль) | `dpkg-sig --verify <pkg>` |
| `syslog-generator-*.rpm` | `rpmsign --addsign` (SHA256 digest + GPG signature) | `rpm -K <pkg>` |
| `SHA256SUMS` | detached `.sig` (GPG) | `gpg --verify SHA256SUMS.sig SHA256SUMS` |
| Контейнеры `ghcr.io/*` | Cosign keyless (GitHub OIDC) | `cosign verify --certificate-identity ...` (опционально, sub-task 6) |

#### Где хранится публичный ключ

- **Public key:** [`scripts/keys/pharmacolog-release.asc`](../scripts/keys/pharmacolog-release.asc)
  (sub-task 1, добавляется в milestone v11.5+).
- **Fingerprint:** в README.md → Security, после публикации ключа.
- **Key server:** publish на `keyserver.ubuntu.com` и `keys.openpgp.org`
  (best practice — обеспечивает discovery).

#### Как это защищает пользователя

1. **Authenticity** — публичный ключ fingerprint'а совпадает с тем, что
   на сайте → артефакт подписан maintainer'ом, не подменён MITM'ом.
2. **Integrity** — `sha256sum -c SHA256SUMS` подтверждает, что файл не
   повреждён при download.
3. **Non-repudiation** — GPG-подпись привязана к ключу maintainer'а;
   отозвать можно через keyserver revocation certificate.

#### Связанные sub-tasks (Issue #155)

- sub-task 1: создание release signing subkey + публикация `pharmacolog-release.asc`.
- sub-task 2: `sign` step в `packages.yml` (между `build-deb` и `verify-install`).
- sub-task 3: hardened `verify-install` (этот документ, §7.7).
- sub-task 4: Launchpad PPA bootstrap (auto-verify через `add-apt-repository`).
- sub-task 5: Fedora COPR setup (auto-verify через `dnf copr enable`).
- sub-task 6: optional Cosign keyless signing для container images.

### 10.4 Метаданные документа

- **Pinned to:** milestone `v11.4` (Issue #107).
- **Поддерживается:** релиз-капитаном Issue #107 (см. AGENTS.md §9 — file
  ownership matrix).
- **Review SLA:** при изменении API установки или CI matrix — обновить этот
  документ в том же PR (см. AGENTS.md §8 — Test Coverage + docs sync).
- **Язык:** русский (по AGENTS.md §1).
- **Cross-references обновляются через:**
  `docs/USER_GUIDE.md` §1, `README.md` секция «Установка».

---

<p align="center">
Установка описана для milestone v11.4. Для следующих релизов см.
<a href="../CHANGELOG.md">CHANGELOG.md</a> и
<a href="ROADMAP.md">docs/ROADMAP.md</a>.
</p>
