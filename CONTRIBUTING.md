# Contributing

Code style and commit conventions are in [AGENTS.md](AGENTS.md) (the same as CLAUDE.md). Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before you start.

## Building from source

macOS needs the Xcode Command Line Tools; Windows needs the Visual Studio 2022 C++ Build Tools and the Windows SDK. The Rust version is pinned in `rust-toolchain.toml`, and rustup picks it up automatically.

```sh
cargo build --release --locked
./target/release/markdownx                          # uses ~/Documents/MarkdownX
MARKDOWNX_HOME=/tmp/lib ./target/release/markdownx   # uses another library
```

While developing, point `MARKDOWNX_HOME` at a scratch directory; don't test against your real library.

The desktop project is a Cargo workspace: `crates/core` is the document engine (`fdl_core`), which does not depend on GPUI, and the root crate is the app. `server/` is a workspace member that builds `markdownx-server`. GPUI, taffy and xattr are locally patched copies in `vendor/`; the changes are listed in [vendor/PATCHES.md](vendor/PATCHES.md). `target/debug` can grow past 10 GB; if you only build release, `cargo clean --profile dev` clears it.

## Testing

```sh
scripts/check.sh                                        # tests + clippy with zero warnings; run before committing
cargo test --release -p fdl_core                        # the document engine only, without building GPUI
FDL_FUZZ_SEEDS=2000 cargo test --release random_edits   # more random edits, for the editing core
cargo test -p markdownx-server                          # Rust collaboration server
cargo clippy -p markdownx-server --all-targets -- -D warnings
```

- The editing core has a random test: starting from a document with every block type, it makes random selections, typing, structural, style, table and undo edits, checks the model's invariants after each step, and saves and reloads every 25 steps.
- The document format has a regression test: `crates/core/tests/fixtures/all-blocks.xml` must read and write back byte for byte.
- UI screenshots and scripted runs are covered in [tools/harness/README.md](tools/harness/README.md).

To run the checks on every commit: `git config core.hooksPath scripts/hooks`.

## Documentation

| Document | Contents |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers, the editing flow, invariants, undo, rendering, the module map |
| [docs/FORMAT.md](docs/FORMAT.md) | Library layout, index recovery, DocxXML read/write rules, `.mdxb` books and interactive blocks |
| [docs/THEMES.md](docs/THEMES.md) | Writing document themes in CSS: selectors, properties, named colors |
| [AGENTS.md](AGENTS.md) | Code style, pre-commit checks, testing conventions |

Most of these, AGENTS.md included, are written in Chinese for now.

## Building release packages

Before a release, `scripts/check.sh` must pass and CI must be green on that commit. After packaging, verify the signatures, notarization (macOS) and the SHA-256 of the downloads. The Windows CI build does not replace installing, uninstalling and editing on a real machine.

The declared macOS minimum is 13, not yet verified on real hardware. Intel packages can be cross-compiled on Apple Silicon; real Intel hardware validation is still needed.

### macOS

`scripts/release.sh` builds for the local architecture, generates the app icon, signs with a Developer ID Application certificate, submits for Apple notarization, staples the ticket and runs a Gatekeeper check, then writes a ZIP and its SHA-256. It does not upload to GitHub or launch the app.

Install the signing certificate, with its private key, in the login keychain and set up a notarytool keychain profile, then run:

```sh
SIGNING_IDENTITY="Developer ID Application: …" NOTARY_PROFILE="markdownx" scripts/release.sh
```

The script stops if signing or notarization fails; whatever is left in the failed run's directory must not be shipped.

To build both architectures sequentially on one Apple Silicon Mac, install the Intel Rust target once, then run the same signing and notarization flow for each architecture:

```sh
rustup target add x86_64-apple-darwin
export SIGNING_IDENTITY="Developer ID Application: …" NOTARY_PROFILE="markdownx"
scripts/release.sh
MACOS_TARGET=x86_64-apple-darwin scripts/release.sh
```

Each run keeps its output in a separate `dist-release.*` directory. Intel ZIPs use `macos-x86_64`; Apple Silicon ZIPs keep `macos-arm64`. CI builds both on its existing ARM runner and verifies the binary architectures and signatures. `SIGNING_IDENTITY=-` skips notarization and creates ad-hoc signed `-unsigned.zip` validation packages, which must not be distributed and are rejected by `publish-downloads.sh`.

### Windows

Uses Inno Setup 7. In a Visual Studio x64 Developer PowerShell:

```powershell
# An unsigned installer for local testing; nothing is uploaded or published
.\scripts\package-windows.ps1 -Iscc 'C:\path\to\ISCC.exe'

# Once the certificate is issued and connected through a hardware token or a cloud signing client,
# pick the current user's certificate by thumbprint
.\scripts\package-windows.ps1 -Iscc 'C:\path\to\ISCC.exe' `
  -CertificateThumbprint '<40-character certificate thumbprint>' `
  -TimestampUrl 'http://time.certum.pl'
```

Signing uses `signtool.exe` from the Windows SDK; `-SignTool` sets its path. The timestamp URL should be the certificate authority's RFC 3161 service (Certum in the example). The script signs the app, the installer and the uninstaller separately, checks the publisher, the trusted signature and the timestamp, and stops on any failure. The final installer and its SHA-256 go to `dist/windows/signed/` or `dist/windows/unsigned/`; the intermediate directories are not for release.

The installer lets you choose the location and an optional desktop shortcut, and by default installs for the current user only. Uninstalling keeps your books. On Windows, the library lives in `MarkdownX` inside the system Documents folder.

On every platform, never commit credentials, private keys or certificate files.

### Releases and in-app updates

There is no public binary release yet, and the in-app updater is compiled out (`src/update.rs`, `ENABLED = false`). A source build must not fetch an update it cannot verify against a published key. The menu item explains this instead of contacting GitHub.

When releases start, publish signed packages plus `latest/update-<platform>.json` (`macos-arm64`, `macos-x86_64`, `windows-x64`), turn `ENABLED` on together with the release key, and document the minisign verification path there. The update code still checks the package signature, the trusted comment (the package file name) and, on macOS, the signing team before swapping the app.

For a new version, bump the version in both `Cargo.toml` files together, and write the version's highlights in `src/changelog.rs`: after updating, existing users see them once at the bottom of the sidebar.

### Linux (experimental)

Install Rust through rustup, then run `scripts/linux-deps.sh` on Ubuntu/Debian or Arch-based systems. Run `scripts/preview-linux.sh` for an isolated temporary library, or set `MARKDOWNX_HOME` to reuse a test library. `scripts/package-linux.sh` produces a tarball and SHA-256 in `dist/linux`; it includes a desktop entry and a 512×512 icon generated with ImageMagick. The binary is dynamically linked; build on the oldest distribution you intend to support and check `runtime-libraries.txt` on the target system.

The Linux build and full checks run on Ubuntu 24.04. Interactive testing has been done on Omarchy 4.5 with Wayland, floating windows and 250% scaling, including Chinese title input. GNOME Wayland, X11 and systems without Noto fonts still need interactive verification; this is not a claim of support for every desktop distribution. Software Vulkan (`vulkan-swrast` on Arch) is useful in VMs or when no hardware Vulkan driver is available. Global quick-note shortcuts and automatic updates are not supported on Linux yet.

Linux support started in [PR #7](https://github.com/neko233-com/MarkdownX/pull/7) by [VJ-2303](https://github.com/VJ-2303); maintainer follow-ups add desktop integration and runtime fixes.
