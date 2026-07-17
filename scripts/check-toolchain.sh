#!/usr/bin/env bash
#
# scripts/check-toolchain.sh — проверка что rustc version matches rust-toolchain.toml.
#
# Использование: запускается в pre-push hook (.pre-commit-config.yaml).
# Exit code: 0 если OK, 1 если mismatch.
#
# Зачем: если developer использует другую версию rustc, чем указана в
# rust-toolchain.toml, CI может вести себя по-другому. Это ловит несоответствие
# ДО push.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

if [ ! -f "rust-toolchain.toml" ]; then
    echo "⚠ rust-toolchain.toml не найден — пропускаем проверку"
    exit 0
fi

# Парсим канал из rust-toolchain.toml.
EXPECTED_CHANNEL=$(grep 'channel' rust-toolchain.toml | sed -n 's/.*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' | head -1 || true)
if [ -z "$EXPECTED_CHANNEL" ]; then
    echo "⚠ channel не указан в rust-toolchain.toml — пропускаем проверку"
    exit 0
fi

# Source cargo env (если есть).
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

ACTUAL_VERSION=$(rustc --version 2>/dev/null | awk '{print $2}' || echo "unknown")

# Для stable каналов — проверяем major.minor version.
# Для beta/nightly/кастомных — предупреждаем (не блокируем).
case "$EXPECTED_CHANNEL" in
    stable|1.*)
        # Извлекаем major.minor из обоих значений.
        EXPECTED_MM=$(echo "$EXPECTED_CHANNEL" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')
        ACTUAL_MM=$(echo "$ACTUAL_VERSION" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')

        if [ "$EXPECTED_MM" != "$ACTUAL_MM" ]; then
            echo "❌ Toolchain mismatch:"
            echo "   rust-toolchain.toml: channel = \"$EXPECTED_CHANNEL\" (=> Rust $EXPECTED_MM.x)"
            echo "   Current rustc:       $ACTUAL_VERSION"
            echo ""
            echo "Решение: rustup toolchain install $EXPECTED_CHANNEL && rustup default $EXPECTED_CHANNEL"
            exit 1
        fi
        echo "✅ Toolchain OK: rustc $ACTUAL_VERSION (matches stable $EXPECTED_MM)"
        ;;
    beta|nightly)
        echo "⚠ Channel: $EXPECTED_CHANNEL (rolling release) — current rustc: $ACTUAL_VERSION"
        echo "  Убедись что nightly/beta обновлён до вчерашней даты."
        ;;
    *)
        if [ "$EXPECTED_CHANNEL" != "$ACTUAL_VERSION" ]; then
            echo "⚠ Channel mismatch:"
            echo "   rust-toolchain.toml: $EXPECTED_CHANNEL"
            echo "   Current rustc:       $ACTUAL_VERSION"
            exit 1
        fi
        echo "✅ Toolchain OK: $ACTUAL_VERSION"
        ;;
esac