#!/usr/bin/env bash
# =============================================================================
# MarkdownX — macOS arm64 (Apple Silicon) build (Go + MyGo native UI)
#
# MyGo is pure Go, zero CGO: it cross-compiles from any machine. On macOS the
# script also signs the app ad hoc and writes the .dmg; elsewhere it leaves a
# complete MarkdownX.app that can be packaged later.
#
#   ./scripts/build-macos-arm64.sh
# =============================================================================
set -euo pipefail
cd "$(dirname "$0")/.."

export CGO_ENABLED=0

echo "==> go vet"
go vet ./...

echo "==> go test"
go test ./...

echo "==> go tool mygo build (darwin/arm64)"
go tool mygo build -platform darwin/arm64 -o dist

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo ""
  echo "✅ 交叉编译完成：dist/darwin-arm64/MarkdownX.app"
  echo "   在 macOS 上运行本脚本会额外生成 .dmg（MyGo 使用 hdiutil 和 ad-hoc 签名）。"
  exit 0
fi

echo ""
echo "✅ macOS arm64 完成"
find dist/darwin-arm64 -maxdepth 2 -type f | sort | sed 's/^/   /'
