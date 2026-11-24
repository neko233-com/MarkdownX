#!/usr/bin/env bash
# =============================================================================
# MarkdownX — cross-compile both maintained targets from any machine.
#
# MyGo is pure Go with CGO_ENABLED=0: no mingw, zig or Xcode is needed.
# The macOS disk image needs macOS; other machines still get MarkdownX.app.
#
#   ./scripts/cross-build.sh
#   → dist/windows-amd64/MarkdownX.exe + markdownx-setup-<version>-amd64.exe
#   → dist/darwin-arm64/MarkdownX.app (+ .dmg on macOS)
#   → dist/cli/<platform>/{markdownx-server,mdx-convert,mdx-enc}
# =============================================================================
set -euo pipefail
cd "$(dirname "$0")/.."

export CGO_ENABLED=0

echo "==> go vet"
go vet ./...

echo "==> go test"
go test ./...

echo "==> go tool mygo build"
go tool mygo build -platform windows/amd64,darwin/arm64 -o dist

build_cli() {
  local goos=$1 goarch=$2 suffix=$3 out="dist/cli/${1}-${2}"
  mkdir -p "$out"
  echo "==> ${goos}/${goarch} command-line tools"
  GOOS=$goos GOARCH=$goarch go build -trimpath -ldflags "-s -w" \
    -o "$out/markdownx-server${suffix}" ./cmd/markdownx-server
  GOOS=$goos GOARCH=$goarch go build -trimpath -ldflags "-s -w" \
    -o "$out/mdx-convert${suffix}" ./cmd/markdownx-convert
  GOOS=$goos GOARCH=$goarch go build -trimpath -ldflags "-s -w" \
    -o "$out/mdx-enc${suffix}" ./cmd/mdx-enc
}

build_cli windows amd64 ".exe"
build_cli darwin arm64 ""

echo ""
find dist -maxdepth 3 -type f | sort
echo ""
echo "✅ 完成（零 CGO、零第三方运行时）"
