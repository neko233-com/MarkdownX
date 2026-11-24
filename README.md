# MarkdownX

A native Markdown editor for **Windows x64** and **macOS arm64**, written in Go with
[MyGo](https://github.com/egoist/mygo). No web view, no C toolchain, no runtime to install.

[中文说明](README-CN.md)

## Install

Download the latest build from
[GitHub Releases](https://github.com/neko233-com/MarkdownX/releases/latest):

- **Windows x64** — `markdownx-setup-<version>-amd64.exe` (per-user installer) or the
  portable `MarkdownX.exe`.
- **macOS arm64** — `markdownx-<version>-arm64.dmg`, drag `MarkdownX.app` into
  Applications.

The builds are not Authenticode-signed or notarized yet; Windows SmartScreen and macOS
Gatekeeper may ask for confirmation on first launch. The command-line tools
(`markdownx-server`, `mdx-convert`, `mdx-enc`) ship as separate archives.
See [docs/INSTALL.md](docs/INSTALL.md) for details and checksums.

## What it does

- **Block editor with a Typora feel.** Consecutive lines form one paragraph, `Shift+Enter`
  makes a hard break, and both survive a save/load round trip unchanged.
- **Round-trip accuracy.** What you see is what lands in the `.md` file: paragraphs merge,
  trailing spaces are treated as formatting rather than content, and list continuation
  lines stay part of the item.
- **Native preview.** A split pane renders the document with MyGo's own Markdown engine.
- **Import.** `.docx`, `.xlsx`, `.epub`, `.ofd`, `.pdf`, `.rtf` and `.html` open directly,
  converted to Markdown by pure-Go readers.
- **Export.** Markdown, a single self-contained HTML file that works offline, and
  `.mdxb` books.
- **Books.** Open a `.mdxb` book — the bilingual, multi-chapter format whose chapters are
  DocxXML or Markdown and whose media travel inside the file — and export one back.
- **Workspace sidebar.** Open a folder, browse its Markdown files, click to switch.
- **Password save.** `Cmd/Ctrl+Shift+S` encrypts a page with scrypt + secretbox; the
  standalone `mdx-enc` tool reads the same format.
- **Collaboration server.** `markdownx-server` shares a folder of Markdown over HTTP,
  records every accepted save as a Git commit, serves history, diffs and restore, and
  pushes other people's edits to open editors as they happen.

## Shortcuts

| Keys | Action |
|---|---|
| `Cmd/Ctrl+N` | New document |
| `Cmd/Ctrl+O` | Open a file |
| `Cmd/Ctrl+Shift+O` | Open a folder |
| `Cmd/Ctrl+S` | Save |
| `Cmd/Ctrl+Shift+S` | Password save |
| `Cmd/Ctrl+E` | Export HTML |
| `Cmd/Ctrl+B` / `I` | Bold / italic |
| `Cmd/Ctrl+F` | Find and replace |
| `Cmd/Ctrl+1`…`6` | Heading levels |
| `Cmd/Ctrl+0` | Back to paragraph |
| `Cmd/Ctrl+D` | Duplicate block |
| `Cmd/Ctrl+Z` | Undo |
| `Enter` | Split the block |
| `Tab` | Nest under the previous block |
| `Backspace` at line start | Merge into the previous block |

## Build

Install Go 1.27.3 or newer. That is the only requirement: the build is pure Go with
`CGO_ENABLED=0`, so no Xcode SDK, no Visual Studio C++ tools, and no cross-compiler.
MyGo is pinned as a `go.mod` tool, so no global CLI install is needed.

```sh
# The machine you are on
go tool mygo build -platform windows/amd64 -o dist
go tool mygo build -platform darwin/arm64 -o dist

# Both targets, from any machine
./scripts/cross-build.sh
```

MyGo produces the Windows per-user installer and the macOS `.app`; on macOS it also
writes the `.dmg`. The Windows installer needs NSIS (`makensis`), and the disk image
needs macOS (`hdiutil`).

On macOS, `scripts/build-macos-arm64.sh` builds the app and DMG.
On Windows, `scripts/build-windows.cmd` runs the checks, builds the installer and the
command-line tools. `scripts/package.ps1` adds a SHA-256 manifest.

## Only two targets

Windows x64 and macOS arm64. Both are produced by Go's own cross-compilation, so a
Windows machine can build the macOS app and the reverse.

## Convert without the UI

```sh
go run ./cmd/markdownx-convert report.docx > report.md
```

## Share a workspace

```sh
go build -o dist/markdownx-server ./cmd/markdownx-server
MARKDOWNX_SERVER_TOKEN=change-me ./dist/markdownx-server -root ~/notes -addr :8787 -init
```

Open `http://localhost:8787/?token=change-me`. The page lists the folder's Markdown,
edits with live updates from other people, and shows each document's Git history with
diffs and one-click restore. Every accepted save is a commit.

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/docs` | GET | list documents |
| `/api/doc?path=` | GET | read one with its version hash |
| `/api/doc` | PUT | save with `{path, content, base}`; `409` on a stale base |
| `/api/history?path=` | GET | commits that touched the document |
| `/api/diff?path=&commit=` | GET | one commit's diff |
| `/api/restore` | POST | write a commit's content back |
| `/api/events` | GET | server-sent events: `{path, base, action}` |

## Layout

```
cmd/markdownx/           the desktop app
cmd/markdownx-server/    the collaboration server
cmd/markdownx-convert/   the headless converter
cmd/mdx-enc/             the encryption tool
internal/doc/            document model, Markdown reader and writer
internal/editor/         the block editor core
internal/book/           .mdxb books: envelope, DocxXML, assets
internal/convert/        docx, xlsx, epub, ofd, pdf, rtf, html readers
internal/export/         self-contained HTML export
internal/server/         HTTP API, Git history, live updates, web editor
internal/rage/           scrypt + secretbox envelopes
internal/app/            MyGo native UI
mygo.json                app name, version, icon, installers
```

## Tests

```sh
go vet ./...
go test ./...
```

## Documentation

| Document | Contents |
|---|---|
| [docs/INSTALL.md](docs/INSTALL.md) | Downloads, installers, checksums |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers and the editing flow |
| [docs/FORMAT.md](docs/FORMAT.md) | Markdown round-trip, `.mdxb`, encryption |
| [docs/RELEASE.md](docs/RELEASE.md) | Tagging and the release pipeline |
| [docs/CODEGRAPH233.md](docs/CODEGRAPH233.md) | Code graph MCP for contributors |

## License

MIT. See [LICENSE](LICENSE).
