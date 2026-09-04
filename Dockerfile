FROM node:24-bookworm-slim AS web-builder
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.89-bookworm AS rust-builder
RUN apt-get update \
    && apt-get install --yes --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home watcher
WORKDIR /app
COPY --from=rust-builder /build/target/release/pubky-watcher-canvas /usr/local/bin/pubky-watcher-canvas
COPY --from=web-builder /build/web/dist ./web/dist
ENV BIND_ADDR=0.0.0.0:3001
ENV STATIC_DIR=/app/web/dist
EXPOSE 3001
USER watcher
CMD ["pubky-watcher-canvas"]
