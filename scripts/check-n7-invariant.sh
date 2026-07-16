#!/usr/bin/env bash
#
# scripts/check-n7-invariant.sh — проверка N7 инварианта.
#
# N7: в runtime коде (вне #[cfg(test)]) нет .unwrap()/.expect()/panic!()
# /unreachable!()/todo!()/unimplemented!().
#
# Это проверяется через grep. Используется как Quality Gate в CI.
#
# Игнорирует:
# - Строки внутри #[cfg(test)] модулей обычных файлов (например, mod tests в конце).
# - Целые test-only файлы (имена *_tests.rs, *_proptests.rs).
# - Однострочные комментарии с этими паттернами (heuristic).

set -euo pipefail

cd "$(dirname "$0")/.."

PATTERNS='\.unwrap\(\)|\.expect\(|panic!\(|unreachable!\(|todo!\(|unimplemented!\('

VIOLATIONS=0
VIOLATION_FILES=""

for file in $(find src -name "*.rs"); do
    # Skip целые test-only файлы (по naming convention).
    base=$(basename "$file")
    case "$base" in
        *_tests.rs|*_proptests.rs)
            continue
            ;;
    esac

    # Найти позицию первой #[cfg(test)] модуля (если есть).
    TEST_LINE=$(grep -n "#\[cfg(test)\]" "$file" | head -1 | cut -d: -f1 || echo 0)

    if [ "$TEST_LINE" -gt 0 ]; then
        # Проверяем только строки до TEST_LINE (выше #[cfg(test)] mod).
        MATCHES=$(head -n "$((TEST_LINE - 1))" "$file" | grep -nE "$PATTERNS" || true)
    else
        # Нет #[cfg(test)] — проверяем весь файл.
        MATCHES=$(grep -nE "$PATTERNS" "$file" || true)
    fi

    if [ -n "$MATCHES" ]; then
        # Исключаем однострочные комментарии (//) с этими словами.
        REAL=$(echo "$MATCHES" | grep -v ':[[:space:]]*//' || true)
        if [ -n "$REAL" ]; then
            VIOLATION_FILES="$VIOLATION_FILES\n$file:\n$REAL\n"
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    fi
done

if [ "$VIOLATIONS" -gt 0 ]; then
    echo "❌ N7 invariant violated in $VIOLATIONS file(s):"
    echo -e "$VIOLATION_FILES"
    echo ""
    echo "N7 invariant: no .unwrap()/.expect() in non-test runtime code."
    echo "Use ?-operator and typed errors (thiserror) instead."
    exit 1
fi

echo "✅ N7 invariant holds: no unwrap/expect/panic in non-test code"
exit 0