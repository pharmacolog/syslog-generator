#!/usr/bin/env bash
#
# .devcontainer/post-create.sh — setup после создания devcontainer.
#
# Устанавливает:
# - Rust toolchain 1.95 (MSRV) уже установлен через devcontainer feature.
# - pre-commit (через pip, т.к. apt может быть медленным в devcontainer).
# - cargo-cyclonedx для SBOM generation.
# - cargo-deny для security audit.
# - cargo-machete для unused deps.
# - cargo-public-api для public-API snapshot gate.

set -euo pipefail

echo "==> syslog-generator devcontainer post-create setup"

# 1. Rust toolchain уже установлен через devcontainer feature (1.95).
# Убедимся что default = 1.95.
if command -v rustup >/dev/null 2>&1; then
    rustup default 1.95
    rustup component add rustfmt clippy llvm-tools-preview
fi

# 2. pre-commit (кросс-платформенная установка через pip).
if ! command -v pre-commit >/dev/null 2>&1; then
    echo "==> Installing pre-commit"
    # Пробуем pip3 (часто предустановлен в rust:bookworm).
    if command -v pip3 >/dev/null 2>&1; then
        pip3 install --break-system-packages --user pre-commit 2>/dev/null || \
            pip3 install --user pre-commit || true
    elif command -v pip >/dev/null 2>&1; then
        pip install --user pre-commit || true
    fi
fi

# 3. cargo tools (для Quality Gates).
echo "==> Installing cargo tools"
cargo install cargo-llvm-cov --locked || true
cargo install cargo-deny --locked || true
cargo install cargo-machete --locked || true
cargo install cargo-cyclonedx --locked || true
cargo install cargo-public-api --locked || true

# 4. pre-commit install (если pre-commit доступен).
if command -v pre-commit >/dev/null 2>&1; then
    echo "==> Installing pre-commit hooks"
    pre-commit install
    pre-commit install --hook-type pre-push
else
    echo "⚠ pre-commit not installed — install manually via 'pip install pre-commit'"
fi

# 5. cargo build для прогрева кэша (ускоряет первый запуск).
echo "==> Pre-building project (warming cargo cache)"
cargo build --locked

echo "==> Setup complete!"
echo ""
echo "Next steps:"
echo "  - Run tests:        cargo test --locked --features test-helpers"
echo "  - Run benchmarks:  cargo bench --bench hot_path -- --quick"
echo "  - Quality gates:    ./scripts/quality-gates.sh"
echo "  - Public API diff:  cargo public-api --features test-helpers > /tmp/api.txt && diff api-snapshot.txt /tmp/api.txt"