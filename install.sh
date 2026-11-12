#!/bin/sh
# Install a macOS Apple Silicon disk image, or invoke PowerShell on Windows.
set -eu

fail() { printf 'MarkdownX installation failed: %s\n' "$*" >&2; exit 1; }
script_dir=
case "$0" in
    */install.sh|install.sh)
        if [ -f "$0" ]; then script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P); fi ;;
esac
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        [ -n "$script_dir" ] && [ -f "$script_dir/install.ps1" ] || fail 'Run install.sh beside install.ps1, or use install.ps1 directly.'
        if command -v pwsh.exe >/dev/null 2>&1; then pwsh_cmd=pwsh.exe
        elif command -v pwsh >/dev/null 2>&1; then pwsh_cmd=pwsh
        else fail 'PowerShell 7 (pwsh) is required on Windows.'
        fi
        ps_file="$script_dir/install.ps1"
        if command -v cygpath >/dev/null 2>&1; then ps_file=$(cygpath -w "$ps_file"); fi
        exec "$pwsh_cmd" -NoProfile -File "$ps_file" "$@" ;;
    Darwin) ;;
    *) fail 'This installer supports macOS arm64 and Windows x86_64.' ;;
esac
[ "$(uname -m)" = arm64 ] || fail 'The macOS release currently supports Apple Silicon (arm64).'

package=
expected_hash=
home_dir=$(printenv HOME || :)
[ -n "$home_dir" ] || fail 'HOME is required.'
apps_dir="$home_dir/Applications"
dry_run=0
allow_unsigned=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --package) [ "$#" -ge 2 ] || fail '--package requires a path.'; package=$2; shift 2 ;;
        --sha256) [ "$#" -ge 2 ] || fail '--sha256 requires a hash.'; expected_hash=$2; shift 2 ;;
        --applications-dir) [ "$#" -ge 2 ] || fail '--applications-dir requires a path.'; apps_dir=$2; shift 2 ;;
        --allow-unsigned) allow_unsigned=1; shift ;;
        --dry-run) dry_run=1; shift ;;
        -h|--help)
            printf '%s\n' 'Usage: sh install.sh [--package FILE.dmg] [--sha256 HASH] [--applications-dir DIR] [--allow-unsigned] [--dry-run]'
            exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done
for tool in curl ditto hdiutil shasum codesign spctl awk; do
    command -v "$tool" >/dev/null 2>&1 || fail "Required macOS tool is missing: $tool"
done
if command -v PlistBuddy >/dev/null 2>&1; then plist_tool=$(command -v PlistBuddy)
else plist_tool=/usr/libexec/PlistBuddy
fi
[ -x "$plist_tool" ] || fail 'PlistBuddy is not available.'

temp_base=$(printenv TMPDIR || :)
[ -n "$temp_base" ] || temp_base=/tmp
temp_base=$(printf '%s' "$temp_base" | sed 's:/*$::')
[ -n "$temp_base" ] || temp_base=/
temp=$(mktemp -d "$temp_base/markdownx-install.XXXXXXXX") || fail 'Could not create a temporary directory.'
stage=
mounted=0
preserve_stage=0
cleanup() {
    if [ "$mounted" -eq 1 ]; then
        if ! hdiutil detach "$temp/volume" >/dev/null; then
            printf 'Could not detach the disk image at %s; its temporary directory was preserved.\n' "$temp/volume" >&2
            return
        fi
    fi
    if [ -n "$stage" ] && [ "$preserve_stage" -eq 0 ] && [ -d "$stage" ] && [ ! -L "$stage" ]; then
        case "$stage" in "$apps_dir"/.markdownx-install.*) rm -rf "$stage" ;; esac
    fi
    if [ -d "$temp" ] && [ ! -L "$temp" ]; then
        case "$temp" in "$temp_base"/markdownx-install.*) rm -rf "$temp" ;; esac
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM

workspace_version=
if [ -n "$script_dir" ] && [ -f "$script_dir/Cargo.toml" ] && grep -q '^name = "markdownx"$' "$script_dir/Cargo.toml"; then
    workspace_version=$(sed -n 's/^version = "\([0-9][0-9.]*\)".*/\1/p' "$script_dir/Cargo.toml" | head -n 1)
fi
if [ -z "$package" ] && [ -n "$workspace_version" ]; then
    for candidate in "$script_dir"/dist-release.*/"markdownx-$workspace_version-macos-arm64.dmg"; do
        if [ -f "$candidate" ]; then package=$candidate; break; fi
    done
    if [ -z "$package" ] && [ "$allow_unsigned" -eq 1 ]; then
        for candidate in "$script_dir"/dist-release.*/"markdownx-$workspace_version-macos-arm64-unsigned.dmg"; do
            if [ -f "$candidate" ]; then package=$candidate; break; fi
        done
    fi
fi

downloaded=0
if [ -z "$package" ]; then
    api='https://api.github.com/repos/neko233-com/MarkdownX/releases'
    if curl --fail --silent --location --proto '=https' --proto-redir '=https' "$api" -o "$temp/releases.json"; then
        command -v osascript >/dev/null 2>&1 || fail 'macOS JavaScript for Automation is required to discover releases.'
        if ! osascript -l JavaScript - "$temp/releases.json" "$allow_unsigned" > "$temp/release.txt" <<'JXA'
function run(argv) {
    ObjC.import('Foundation');
    const raw = $.NSString.stringWithContentsOfFileEncodingError($(argv[0]), $.NSUTF8StringEncoding, null);
    if (!raw) throw Error('Cannot read release list');
    const releases = JSON.parse(ObjC.unwrap(raw));
    if (!Array.isArray(releases)) throw Error('Invalid release list');
    const sorted = releases.filter(r => !r.draft);
    for (const release of sorted) {
        if (!/^v[0-9]+\.[0-9]+\.[0-9]+$/.test(release.tag_name)) continue;
        const names = (release.assets || []).map(a => a.name);
        const signed = 'markdownx-macos-arm64.dmg';
        const unsigned = 'markdownx-macos-arm64-unsigned.dmg';
        const name = names.includes(signed) ? signed : (argv[1] === '1' && names.includes(unsigned) ? unsigned : '');
        if (name && names.includes(name + '.sha256')) return [release.tag_name, name, release.prerelease ? 'yes' : 'no'].join('\n');
    }
    throw Error('No suitable DMG with checksum; unsigned test builds need --allow-unsigned');
}
JXA
        then fail 'No suitable release was found. Unsigned test builds require --allow-unsigned.'; fi
    else
        # GitHub's unauthenticated API is shared and can be rate-limited even
        # for public repositories. The public release feed has no API quota;
        # probe both the package and checksum before selecting a release.
        feed='https://github.com/neko233-com/MarkdownX/releases.atom'
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' "$feed" -o "$temp/releases.atom" || fail 'Could not retrieve GitHub releases; supply --package for an offline install.'
        sed -n 's#.*<id>tag:github.com,2008:Repository/[0-9][0-9]*/\(v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)</id>.*#\1#p' "$temp/releases.atom" > "$temp/tags.txt"
        found=0
        while IFS= read -r candidate; do
            printf '%s\n' "$candidate" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || continue
            for name in markdownx-macos-arm64.dmg markdownx-macos-arm64-unsigned.dmg; do
                [ "$name" != markdownx-macos-arm64-unsigned.dmg ] || [ "$allow_unsigned" -eq 1 ] || continue
                base="https://github.com/neko233-com/MarkdownX/releases/download/$candidate"
                if curl --head --fail --silent --location --proto '=https' --proto-redir '=https' "$base/$name" >/dev/null \
                    && curl --head --fail --silent --location --proto '=https' --proto-redir '=https' "$base/$name.sha256" >/dev/null; then
                    printf '%s\n%s\nunknown\n' "$candidate" "$name" > "$temp/release.txt"
                    found=1
                    break 2
                fi
            done
        done < "$temp/tags.txt"
        [ "$found" -eq 1 ] || fail 'No suitable release was found. Unsigned test builds require --allow-unsigned.'
    fi
    tag=$(sed -n '1p' "$temp/release.txt")
    asset=$(sed -n '2p' "$temp/release.txt")
    prerelease=$(sed -n '3p' "$temp/release.txt")
    printf '%s\n' "$tag" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Invalid release tag.'
    case "$asset" in markdownx-macos-arm64.dmg|markdownx-macos-arm64-unsigned.dmg) ;; *) fail 'Invalid release asset.' ;; esac
    [ "$prerelease" != yes ] || printf '%s\n' 'Using a GitHub pre-release.'
    base="https://github.com/neko233-com/MarkdownX/releases/download/$tag"
    package="$temp/$asset"
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' "$base/$asset.sha256" -o "$temp/package.sha256"
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' "$base/$asset" -o "$package"
    downloaded=1
fi
[ -f "$package" ] && [ ! -L "$package" ] || fail 'The package is not a regular file.'

actual_hash=$(shasum -a 256 "$package" | awk '{print tolower($1)}')
printf '%s\n' "$actual_hash" | grep -Eq '^[0-9a-f]{64}$' || fail 'Could not calculate SHA-256.'
if [ -n "$expected_hash" ]; then
    expected_hash=$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')
    [ "$actual_hash" = "$expected_hash" ] || fail 'The supplied SHA-256 does not match the package.'
fi
sum_file=
if [ "$downloaded" -eq 1 ]; then sum_file="$temp/package.sha256"
elif [ -f "$package.sha256" ]; then sum_file="$package.sha256"
fi
if [ -n "$sum_file" ]; then
    [ "$(wc -l < "$sum_file" | tr -d ' ')" -eq 1 ] || fail 'The checksum file must contain exactly one line.'
    listed_hash=$(awk 'NR == 1 {print tolower($1)}' "$sum_file")
    listed_name=$(awk 'NR == 1 {$1=""; sub(/^[[:space:]]+[*]?/, ""); print}' "$sum_file")
    [ "$listed_hash" = "$actual_hash" ] || fail 'The package failed its SHA-256 check.'
    [ -z "$listed_name" ] || [ "$listed_name" = "$(basename "$package")" ] || fail 'The checksum names another package.'
elif [ -z "$expected_hash" ]; then
    printf '%s\n' 'Local package: SHA-256 calculated; no reference was supplied.'
fi

hdiutil verify "$package" >/dev/null || fail 'The disk image failed verification.'
mkdir "$temp/volume"
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$temp/volume" "$package" >/dev/null || fail 'Could not mount the disk image.'
mounted=1
app="$temp/volume/MarkdownX.app"
info="$app/Contents/Info.plist"
[ -d "$app" ] && [ ! -L "$app" ] && [ -f "$info" ] && [ -f "$app/Contents/MacOS/markdownx" ] || fail 'The macOS app bundle is incomplete.'
bundle_id=$("$plist_tool" -c 'Print :CFBundleIdentifier' "$info") || fail 'Could not read the app identifier.'
version=$("$plist_tool" -c 'Print :CFBundleShortVersionString' "$info") || fail 'Could not read the app version.'
[ "$bundle_id" = com.neko233.markdownx ] || fail 'The package is not a MarkdownX app.'
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Invalid app version.'
codesign --verify --deep --strict "$app" || fail 'The app signature is invalid.'
codesign -dv --verbose=4 "$app" > "$temp/signature.txt" 2>&1 || fail 'Could not inspect the app signature.'
if grep -Fx 'Signature=adhoc' "$temp/signature.txt" >/dev/null; then
    [ "$allow_unsigned" -eq 1 ] || fail 'This unsigned test build requires --allow-unsigned.'
    printf '%s\n' 'Warning: this test build is not notarized; macOS may ask for explicit approval before opening it.' >&2
else
    spctl --assess --type execute "$app" || fail 'Gatekeeper did not accept this app signature or notarization.'
fi

destination="$apps_dir/MarkdownX.app"
if [ -L "$destination" ] || { [ -e "$destination" ] && [ ! -d "$destination" ]; }; then fail 'The app destination is occupied.'; fi
if [ -d "$destination" ]; then
    old_id=$("$plist_tool" -c 'Print :CFBundleIdentifier' "$destination/Contents/Info.plist") || fail 'The existing app is not a recognized MarkdownX bundle.'
    [ "$old_id" = com.neko233.markdownx ] || fail 'The existing app is not a recognized MarkdownX bundle.'
    # Compare the complete executable path, so other applications are not matched.
    # shellcheck disable=SC2009
    if ps -axo comm= | grep -Fx "$destination/Contents/MacOS/markdownx" >/dev/null; then
        fail 'Close the running MarkdownX app and retry; it was not forced to stop.'
    fi
fi
if [ "$dry_run" -eq 1 ]; then
    printf 'Ready: MarkdownX %s; install to %s\n' "$version" "$destination"
    exit 0
fi
mkdir -p "$apps_dir"
apps_dir=$(CDPATH='' cd -- "$apps_dir" && pwd -P)
destination="$apps_dir/MarkdownX.app"
[ ! -L "$destination" ] || fail 'The app destination changed before installation.'
stage=$(mktemp -d "$apps_dir/.markdownx-install.XXXXXXXX") || fail 'Could not stage the application.'
ditto "$app" "$stage/MarkdownX.app"
codesign --verify --deep --strict "$stage/MarkdownX.app" || fail 'The copied app signature is invalid.'
if [ -d "$destination" ]; then mv "$destination" "$stage/previous.app"; fi
if ! mv "$stage/MarkdownX.app" "$destination"; then
    if [ -d "$stage/previous.app" ] && ! mv "$stage/previous.app" "$destination"; then
        preserve_stage=1
        fail "Could not restore the previous app. It remains at $stage/previous.app"
    fi
    fail 'Could not install the app; the previous version was restored.'
fi
if [ -d "$stage/previous.app" ]; then rm -rf "$stage/previous.app"; fi
printf 'MarkdownX %s installed for the current user: %s\n' "$version" "$destination"
