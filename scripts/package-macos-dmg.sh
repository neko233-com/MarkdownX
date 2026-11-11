#!/bin/bash
# Package a prebuilt app for direct distribution; this does not use the App Store.
set -euo pipefail
if [[ $# != 2 ]]; then echo 'Usage: package-macos-dmg.sh APP OUTPUT.dmg' >&2; exit 2; fi
app=$1
output=$2
[[ -d "$app" && ! -L "$app" && "$output" == *.dmg && ! -e "$output" ]] || { echo 'Invalid app or DMG destination.' >&2; exit 1; }
codesign --verify --deep --strict "$app"
stage=$(mktemp -d "${TMPDIR:-/tmp}/markdownx-dmg.XXXXXXXX")
cleanup() { [[ -d "$stage" && ! -L "$stage" ]] && rm -rf "$stage"; }
trap cleanup EXIT
ditto "$app" "$stage/MarkdownX.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname MarkdownX -srcfolder "$stage" -format UDZO "$output"
hdiutil verify "$output" >/dev/null
(cd "$(dirname "$output")" && shasum -a 256 "$(basename "$output")" > "$(basename "$output").sha256")
printf 'Disk image: %s\n' "$output"
