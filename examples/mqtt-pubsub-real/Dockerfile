# Multi-stage build of the mqtt-pubsub-real example.
#
# The build context is this directory (examples/mqtt-pubsub-real/), so the
# image builds against the PUBLISHED crates.io taktora-* deps exactly as the
# committed Cargo.toml pins them — the `[patch.crates-io]` local-dep block
# stays commented out (patch OFF). Nothing outside this directory is needed.

# ---- build stage -----------------------------------------------------------
FROM rust:1-slim-bookworm AS build
WORKDIR /app

# iceoryx2 (pulled transitively via taktora-connector-transport-iox) runs
# `bindgen` in its build script, which needs libclang; `cc` covers any C in
# the dependency graph. `rust:slim` ships neither.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential clang libclang-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy the manifest + lockfile first for layer caching, then the sources.
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# --locked: build the exact registry-sourced graph committed in Cargo.lock.
RUN cargo build --release --locked

# ---- runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Plain-TCP MQTT (no TLS feature) needs no extra runtime libraries beyond the
# glibc already present in debian-slim.
COPY --from=build /app/target/release/mqtt-pubsub-real /usr/local/bin/mqtt-pubsub-real

# Broker address is supplied via env by docker-compose (MQTT_BROKER=broker).
ENV MQTT_BROKER=127.0.0.1 \
    MQTT_PORT=1883

ENTRYPOINT ["/usr/local/bin/mqtt-pubsub-real"]
