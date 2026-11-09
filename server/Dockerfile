# syntax=docker/dockerfile:1

# Build from the repository root:
#   docker build -f server/Dockerfile -t markdownx-server .
FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY src ./src
COPY crates/core ./crates/core
COPY server ./server
COPY templates ./templates
COPY vendor ./vendor
RUN cargo build --release -p markdownx-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates curl tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && adduser --disabled-password --uid 10001 --gecos "" markdownx \
    && mkdir -p /data /app \
    && chown -R markdownx:markdownx /data /app
WORKDIR /app
COPY --from=build /src/target/release/markdownx-server /usr/local/bin/markdownx-server
COPY --from=build /src/server/web ./web
USER markdownx
ENV MARKDOWNX_SERVER_ADDR=:8877 \
    MARKDOWNX_SERVER_ROOT=/data \
    MARKDOWNX_SERVER_WEB=/app/web
EXPOSE 8877
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
    CMD curl -fsS http://127.0.0.1:8877/api/health >/dev/null || exit 1
ENTRYPOINT ["markdownx-server"]
