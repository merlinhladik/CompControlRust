# SPDX-License-Identifier: GPL-3.0-or-later
# Multi-stage build for the CompControlRust web server: compiles the Leptos WASM
# frontend (Trunk) + the axum backend, then ships a slim runtime image that
# serves both same-origin on :5001. Schema migrations run on startup.

# ── Stage 1: build frontend (wasm) + server (host) ──────────────────────────
FROM rust:1-bookworm AS builder
WORKDIR /app

# Frontend toolchain: wasm target + Trunk (+ wasm-bindgen/wasm-opt fetched by Trunk).
RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

# Cache dependencies: copy manifests first, then sources.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY config ./config

# Build the Leptos frontend → crates/ccr-frontend/dist (served by the backend).
RUN cd crates/ccr-frontend && trunk build --release
# Build the backend (default-members excludes the wasm frontend from the host build).
RUN cargo build --release -p ccr-server

# ── Stage 2: runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ccr-server /usr/local/bin/ccr-server
COPY --from=builder /app/crates/ccr-frontend/dist /app/frontend
COPY --from=builder /app/config /app/config

ENV CCR_FRONTEND_DIR=/app/frontend \
    CCR_BRACKET_CONFIG=/app/config/bracket_config.xlsx \
    CCR_HTTP_ADDR=0.0.0.0:5001 \
    RUST_LOG=ccr_server=info,tower_http=info

EXPOSE 5001
CMD ["ccr-server"]
