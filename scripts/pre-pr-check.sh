#!/usr/bin/env bash
# Pre-PR gate-check: обязательные проверки перед `git push`.
#
# Использование:
#   bash scripts/pre-pr-check.sh                  # полная проверка
#   bash scripts/pre-pr-check.sh --skip-build      # пропустить cargo build (для скорости)
#
# Exit codes:
#   0 — все проверки прошли
#   1 — минимум одна проверка failed
#
# Контракт: ДОЛЖЕН запускаться перед каждым `git push` в feature-ветку.
# См. docs/COORDINATION.md §S6.

set -uo pipefail

SKIP_BUILD=0
SKIP_KAFKA=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        --skip-kafka) SKIP_KAFKA=1 ;;
        --help)
            echo "Usage: $0 [--skip-build] [--skip-kafka]"
            exit 0
            ;;
    esac
done

FAILED=0
TOTAL=0

run_check() {
    local name="$1"
    shift
    TOTAL=$((TOTAL + 1))
    echo ""
    echo "=== [$TOTAL] $name ==="
    if "$@"; then
        echo "✅ $name: PASS"
    else
        echo "❌ $name: FAIL"
        FAILED=$((FAILED + 1))
    fi
}

# 1. Format
run_check "cargo fmt --all --check" \
    bash -c 'cargo fmt --all -- --check'

# 2. Clippy (default features)
run_check "cargo clippy --all-targets" \
    bash -c 'cargo clippy --all-targets -- -D warnings'

# 3. Clippy + kafka feature
if [ "$SKIP_KAFKA" -eq 0 ]; then
    run_check "cargo clippy --features kafka" \
        bash -c 'cargo clippy --all-targets --features kafka -- -D warnings'
fi

# 4. Tests
run_check "cargo test --release --lib" \
    bash -c 'cargo test --release --lib'

# 5. Build
if [ "$SKIP_BUILD" -eq 0 ]; then
    run_check "cargo build --release" \
        bash -c 'cargo build --release --locked'
fi

# 6. Doc
run_check "cargo doc -D warnings" \
    bash -c 'RUSTDOCFLAGS="-D warnings" cargo doc --no-deps'

# 7. Public API
run_check "cargo public-api matches api-snapshot.txt" \
    bash -c 'diff <(cargo public-api --features test-helpers 2>/dev/null) api-snapshot.txt'

# 8. N7 invariant
run_check "N7 invariant" \
    bash -c 'bash scripts/check-n7-invariant.sh'

# 9. cargo deny
run_check "cargo deny" \
    bash -c 'cargo deny check'

# 10. cargo machete
run_check "cargo machete" \
    bash -c 'cargo machete'

echo ""
echo "========================================"
if [ "$FAILED" -eq 0 ]; then
    echo "✅ ALL $TOTAL CHECKS PASSED"
    echo "Ready for git push."
    exit 0
else
    echo "❌ $FAILED of $TOTAL CHECKS FAILED"
    echo "Fix errors before git push!"
    exit 1
fi
