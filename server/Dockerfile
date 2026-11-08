# syntax=docker/dockerfile:1

FROM golang:1.23-alpine AS build
WORKDIR /src
RUN apk add --no-cache git ca-certificates
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" \
    -o /out/markdownx-server ./cmd/markdownx-server

FROM alpine:3.20
RUN apk add --no-cache git ca-certificates tzdata \
    && adduser -D -u 10001 markdownx \
    && mkdir -p /data /app \
    && chown -R markdownx:markdownx /data /app
WORKDIR /app
COPY --from=build /out/markdownx-server /usr/local/bin/markdownx-server
COPY --from=build /src/web ./web
USER markdownx
ENV MARKDOWNX_SERVER_ADDR=:8787 \
    MARKDOWNX_SERVER_ROOT=/data
EXPOSE 8787
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
    CMD wget -qO- http://127.0.0.1:8787/api/health >/dev/null || exit 1
ENTRYPOINT ["markdownx-server"]
