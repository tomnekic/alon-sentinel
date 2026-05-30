# ── Dependency planner ───────────────────────────────────────────────────────
FROM rust:1.95-bookworm AS chef
WORKDIR /app

RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Build ────────────────────────────────────────────────────────────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies only — this layer is cached unless Cargo.toml / Cargo.lock change.
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bins

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/target/release/api                         /usr/local/bin/api
COPY --from=builder /app/target/release/worker                      /usr/local/bin/worker
COPY --from=builder /app/target/release/migrate                     /usr/local/bin/migrate
COPY --from=builder /app/target/release/provision_client            /usr/local/bin/provision_client
COPY --from=builder /app/target/release/provision_admin_user        /usr/local/bin/provision_admin_user
COPY --from=builder /app/target/release/seed_admin_role_permissions /usr/local/bin/seed_admin_role_permissions

CMD ["/usr/local/bin/api"]
