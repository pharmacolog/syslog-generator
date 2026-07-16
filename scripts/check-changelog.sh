#!/usr/bin/env bash
#
# scripts/check-changelog.sh — проверка обновления CHANGELOG.md перед release.
#
# Использование: CHECK_CHANGELOG=1 ./scripts/check-changelog.sh
#
# Проверяет что:
# 1. CHANGELOG.md содержит секцию для текущей версии (Cargo.toml).
# 2. CHANGELOG.md содержит секцию для предыдущего релиза (для сравнения).

set -euo pipefail

cd "$(dirname "$0")/.."

CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Current version (Cargo.toml): $CURRENT_VERSION"

# Ищем "## v${CURRENT_VERSION}" в CHANGELOG.md (без учёта даты и деталей).
if grep -q "## v${CURRENT_VERSION}" CHANGELOG.md; then
    echo "✅ CHANGELOG.md contains section for v${CURRENT_VERSION}"
else
    echo "❌ CHANGELOG.md missing section for v${CURRENT_VERSION}"
    echo ""
    echo "Добавьте секцию:"
    echo "    ## v${CURRENT_VERSION} - $(date '+%Y-%m-%d')"
    echo ""
    echo "с описанием изменений."
    exit 1
fi

# Проверяем что README.md badge обновлён.
if grep -q "version-v${CURRENT_VERSION}" README.md; then
    echo "✅ README.md badge updated to v${CURRENT_VERSION}"
else
    echo "❌ README.md badge still shows old version (expected v${CURRENT_VERSION})"
    exit 1
fi

# Проверяем что CLAUDE_HANDOFF.md содержит запись для текущей версии.
if grep -q "v${CURRENT_VERSION}" CLAUDE_HANDOFF.md; then
    echo "✅ CLAUDE_HANDOFF.md contains v${CURRENT_VERSION}"
else
    echo "❌ CLAUNCHHANDOFF.md missing v${CURRENT_VERSION} history entry"
    exit 1
fi

echo ""
echo "All CHANGELOG/README/HANDOFF checks passed for v${CURRENT_VERSION}."
exit 0