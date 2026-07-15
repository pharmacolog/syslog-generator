# syntax=docker/dockerfile:1.6
#
# N12 (v9.6.0): Dockerfile для syslog-generator.
#
# Multi-stage сборка:
# 1) builder — rust:1.95-bookworm с cmake/pkg-config/libssl-dev.
#    Зафиксировано на MSRV = 1.95 (Cargo.toml rust-version = "1.95"),
#    иначе Docker-сборка использует более новый toolchain, чем заявлено
#    в MSRV-check CI job (v10.5.0). Если перейти на `cargo build --bin`
#    можно убрать apt-зависимости (бинарь syslog-generator pure Rust:
#    rskafka + rustls+ring + tokio + chrono).
# 2) runtime — gcr.io/distroless/cc-debian12 (Debian 12 + libc, без shell).
#    Минимальная поверхность атаки, ≈25 MB.
#
# Использование:
#   docker build -t syslog-generator:dev .
#   docker run --rm syslog-generator:dev --version
#   docker run --rm -v $PWD/examples:/examples:ro syslog-generator:dev \
#     --profile /examples/single_target.json

# ============ Stage 1: builder ============
FROM rust:1.95-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Кэшируем зависимости отдельно от исходников — Docker слои позволяют
# переиспользовать ~2 GB зависимостей пока src/ меняется.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY tests ./tests
COPY schemas ./schemas
COPY examples ./examples
COPY auth_schema.json auth_schema.json.meta.json \
     nginx_schema.json nginx_schema.json.meta.json \
     profile.json profile.json.meta.json \
     templates.json templates.json.meta.json ./

# Продакшн-сборка. --locked гарантирует совпадение с Cargo.lock.
RUN cargo build --release --locked --bin syslog-generator

# Стрипаем debug-символы (~30% экономии).
RUN strip /app/target/release/syslog-generator

# ============ Stage 2: runtime ============
# distroless/cc-debian12: Debian 12 + glibc + ca-certificates.
# Без shell/без apt. Если все deps pure-Rust (а у нас rskafka + rustls+ring),
# можно перейти на distroless/static-debian12 (≈2 MB), но cross-arch сложнее.
FROM gcr.io/distroless/cc-debian12 AS runtime

# Mozilla CA bundle для TLS-handshake (rustls-webpki-roots дублирует
# в памяти, но файловая копия нужна для openssl-style fallback и
# совместимости с системными утилитами).
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Бинарь (после strip ≈ 8-10 MB).
COPY --from=builder /app/target/release/syslog-generator /usr/local/bin/syslog-generator

# Примеры профилей (read-only mount в compose).
COPY --from=builder /app/examples /examples

# distroless cc-debian12 содержит non-root пользователя с UID 65532.
USER 65532:65532

# Не задаём HEALTHCHECK — syslog-generator CLI не имеет long-running daemon
# режима, это burst-генератор (запускается → шлёт → выходит).

ENTRYPOINT ["/usr/local/bin/syslog-generator"]
CMD ["--help"]