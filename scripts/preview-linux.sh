#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -s) == Linux ]] || { echo 'Run this preview inside Linux.' >&2; exit 1; }
cargo build --release --locked
export MARKDOWNX_HOME="${MARKDOWNX_HOME:-$(mktemp -d "${TMPDIR:-/tmp}/markdownx-linux-preview.XXXXXX")}"
printf 'Preview library: %s\n' "$MARKDOWNX_HOME"
exec "${CARGO_TARGET_DIR:-target}/release/markdownx"
