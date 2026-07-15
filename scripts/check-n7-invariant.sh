#!/usr/bin/env bash
#
# scripts/check-n7-invariant.sh — проверка N7 инварианта.
#
# N7: в runtime коде (вне #[cfg(test)]) нет .unwrap()/.expect()/panic!()
# /unreachable!()/todo!()/unimplemented!().
#
# Это проверяется через grep. Используется как Quality Gate в CI.
# Игнорирует строки с #[cfg(test)] preceding и комментарии.

set -euo pipefail

cd "$(dirname "$0")/.."

PATTERNS='\.unwrap\(\)|\.expect\(|panic!\(|unreachable!\(|todo!\(|unimplemented!\('
FOUND=$(grep -rn -E "$PATTERNS" src/ --include="*.rs" 2>/dev/null || true)

# Фильтруем строки внутри #[cfg(test)] модулей и однострочные комментарии
# с этими паттернами (редко, но возможно — "this function never panics").
# Через awk: считаем строки в файле которые идут после #[cfg(test)] и до конца файла.

VIOLATIONS=0
VIOLATION_FILES=""
for file in $(find src -name "*.rs"); do
    # Найти позицию первой #[cfg(test)] модуля (если есть).
    TEST_LINE=$(grep -n "#\[cfg(test)\]" "$file" | head -1 | cut -d: -f1 || echo 0)
    if [ "$TEST_LINE" -gt 0 ]; then
        # Проверяем только строки до TEST_LINE.
        MATCHES=$(head -n "$((TEST_LINE - 1))" "$file" | grep -nE "$PATTERNS" || true)
    else
        MATCHES=$(grep -nE "$PATTERNS" "$file" || true)
    fi

    if [ -n "$MATCHES" ]; then
        # Исключаем однострочные комментарии (//) с этими словами.
        # Это эвристика — false positives возможны но безопасны.
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