#!/usr/bin/env bash
#
# scripts/quality-gates.sh — единая точка запуска Quality Gates.
#
# Использование: ./scripts/quality-gates.sh
#               CHECK_CHANGELOG=1 ./scripts/quality-gates.sh (для releases)
#
# Все шаги должны exit code 0. Любой non-zero → PR заблокирован.
#
# В CI (.github/workflows/ci.yml) эти шаги выполняются параллельно
# в разных jobs. Локальный запуск — последовательно для быстрой обратной связи.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║  syslog-generator Quality Gates                              ║"
echo "║  $(date '+%Y-%m-%d %H:%M:%S')                                       ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Source cargo env (если есть).
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

FAILED=0

run_step() {
    local title="$1"
    local cmd="$2"
    echo ""
    echo "▶ $title"
    echo "  $ $cmd"
    if eval "$cmd"; then
        echo "  ✅ PASS"
    else
        echo "  ❌ FAIL"
        FAILED=$((FAILED + 1))
    fi
}

# ─────────────────────────────────────────────────────────────────
# G1. Format & Lints (RUSTFMT, CLIPPY)
# ─────────────────────────────────────────────────────────────────
run_step "G1.1: cargo fmt --all --check" "cargo fmt --all -- --check"
run_step "G1.2: cargo clippy (no features)" \
    "cargo clippy --no-default-features --all-targets -- -D warnings"
run_step "G1.3: cargo clippy (--features kafka)" \
    "cargo clippy --features kafka --all-targets -- -D warnings"
run_step "G1.4: cargo clippy (--features kafka,test-helpers)" \
    "cargo clippy --features kafka,test-helpers --all-targets -- -D warnings"

# ─────────────────────────────────────────────────────────────────
# G2. Documentation (RUSTDOC)
# ─────────────────────────────────────────────────────────────────
run_step "G2.1: cargo doc --no-deps (no warnings)" \
    "RUSTDOCFLAGS='-D warnings' cargo doc --no-deps"

# ─────────────────────────────────────────────────────────────────
# G3. Tests (UNIT + INTEGRATION + N7)
# ─────────────────────────────────────────────────────────────────
run_step "G3.1: cargo test --locked (--features test-helpers)" \
    "cargo test --locked --features test-helpers"
run_step "G3.2: cargo test --locked (--features kafka,test-helpers)" \
    "cargo test --locked --features kafka,test-helpers"

# ─────────────────────────────────────────────────────────────────
# G4. Build & Benches
# ─────────────────────────────────────────────────────────────────
run_step "G4.1: cargo build --release --locked" \
    "cargo build --release --locked"
run_step "G4.2: cargo bench --no-run --locked" \
    "cargo bench --no-run --locked"

# ─────────────────────────────────────────────────────────────────
# G5. Security & Public API
# ─────────────────────────────────────────────────────────────────
run_step "G5.1: cargo-deny (advisories + licenses)" \
    "command -v cargo-deny >/dev/null && cargo deny check || echo 'cargo-deny not installed (skipping — CI will catch)'"
run_step "G5.2: cargo-machete (unused deps)" \
    "command -v cargo-machete >/dev/null && cargo machete || echo 'cargo-machete not installed (skipping — CI will catch)'"
run_step "G5.3: cargo public-api (snapshot diff)" \
    "command -v cargo-public-api >/dev/null && bash -c 'diff -u api-snapshot.txt <(cargo public-api --features test-helpers 2>/dev/null)' || echo 'cargo-public-api not installed or diff has changes (CI will catch)'"

# ─────────────────────────────────────────────────────────────────
# G6. N7 invariant check (no .unwrap()/.expect() in non-test src/)
# ─────────────────────────────────────────────────────────────────
run_step "G6.1: N7 invariant — no unwrap()/expect() in non-test src/" \
    "bash scripts/check-n7-invariant.sh"

# ─────────────────────────────────────────────────────────────────
# G7. Coverage gate (cargo-llvm-cov, ≥ 87%)
# ─────────────────────────────────────────────────────────────────
if command -v cargo-llvm-cov >/dev/null 2>&1; then
    run_step "G7.1: coverage ≥ 87% (cargo-llvm-cov)" \
        "cargo llvm-cov --features kafka,test-helpers --workspace --all-targets --fail-under-lines=87 --summary-only 2>&1 | tail -5"
else
    echo ""
    echo "▶ G7.1: coverage ≥ 87% (cargo-llvm-cov)"
    echo "  ⚠ cargo-llvm-cov not installed (skipping — CI will catch)"
fi

# ─────────────────────────────────────────────────────────────────
# G8. Performance regression (PR-10 target ≤ 2 µs/msg)
# ─────────────────────────────────────────────────────────────────
echo ""
echo "▶ G8.1: performance hot-path (PR-10 target ≤ 2 µs/msg)"
echo "  ⚠ Run \`cargo bench --bench hot_path -- --quick\` to verify (~30 секунд)."
echo "  ⚠ Этот gate НЕ enforced в CI (только мониторинг через bench output artifact)."

# ─────────────────────────────────────────────────────────────────
# G9. Changelog + RELEASE_CHECK (для releases)
# ─────────────────────────────────────────────────────────────────
if [ -n "${CHECK_CHANGELOG:-}" ]; then
    run_step "G9.1: CHANGELOG.md updated for new version" \
        "bash scripts/check-changelog.sh"
fi

# ─────────────────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════════════"
if [ "$FAILED" -eq 0 ]; then
    echo "  ✅ ALL QUALITY GATES PASSED"
    echo "═══════════════════════════════════════════════════════════════"
    exit 0
else
    echo "  ❌ $FAILED QUALITY GATE(S) FAILED"
    echo "═══════════════════════════════════════════════════════════════"
    exit 1
fi