# Contributing

Code style, commit conventions and the hard constraints are in
[AGENTS.md](AGENTS.md) (the same as [CLAUDE.md](CLAUDE.md)).

MarkdownX is a Go desktop app built on [MyGo](https://github.com/egoist/mygo). Only two
targets are maintained: **Windows x64** and **macOS arm64**.

## Building from source

Install Go 1.27.3 or newer. Nothing else is needed: the build is pure Go with
`CGO_ENABLED=0`, so no C toolchain, SDK or cross-compiler is involved. MyGo is pinned as
a `go.mod` tool.

```sh
go tool mygo build -platform windows/amd64 -o dist
go tool mygo build -platform darwin/arm64 -o dist
```

MyGo produces the Windows per-user installer (`markdownx-setup-<version>-amd64.exe`) and
the macOS `MarkdownX.app`; on macOS it also writes the `.dmg`. Cross-compiling both from
any machine:

```sh
./scripts/cross-build.sh
```

Or one at a time:

```sh
# Windows x64
.\scripts\build-windows.cmd

# macOS arm64
./scripts/build-macos-arm64.sh
```

The Windows installer needs NSIS (`makensis`); the macOS disk image needs macOS
(`hdiutil`). `scripts/package.ps1` builds a local Windows package with a SHA-256 manifest.

## Code navigation

The repository ships a project-local [codegraph233](docs/CODEGRAPH233.md) MCP config.
Use `cg233_file_outline`, `cg233_find_symbol`, `cg233_find_referencing_symbols` and
`cg233_impact` before editing, and rebuild the index with `codegraph233 index --root .`
after a large change.

## Converting documents

`cmd/markdownx-convert` turns any supported format into Markdown on stdout:

```sh
go run ./cmd/markdownx-convert report.docx > report.md
```

Supported: `.docx`, `.xlsx`, `.epub`, `.ofd`, `.pdf`, `.rtf`, `.html`, and plain Markdown.

## Testing

```sh
go vet ./...
go test ./...
```

- `internal/doc` covers the document model, the Markdown reader and writer, hard-break
  round trips, tables, equations and list continuation merging.
- `internal/editor` covers splitting, merging, indentation, marks, duplicate and undo.
- `internal/convert` covers every reader; `TestDemoDocuments` also converts the real
  sample documents in `MarkdownX-Demo/` when that folder is present.
- `internal/export` covers the self-contained HTML export.
- `internal/server` covers auth, saves, conflicts, history and restore.

## Releasing

The version lives in `mygo.json`; the tag must match it. See
[docs/RELEASE.md](docs/RELEASE.md) for the full pipeline. The short version:

```sh
git tag v0.1.47
git push origin v0.1.47
```

## Layout

| Path | Contents |
|---|---|
| `cmd/markdownx` | the desktop app |
| `cmd/markdownx-server` | the collaboration server |
| `cmd/markdownx-convert` | the headless converter |
| `cmd/mdx-enc` | the encryption tool |
| `internal/doc` | document model, Markdown read/write |
| `internal/editor` | the block editor core |
| `internal/book` | `.mdxb` books: envelope, DocxXML, assets |
| `internal/convert` | import readers |
| `internal/export` | HTML export |
| `internal/server` | HTTP API, Git history, live updates, web editor |
| `internal/rage` | scrypt + secretbox envelopes |
| `internal/app` | MyGo native UI |

## Documentation

| Document | Contents |
|---|---|
| [docs/INSTALL.md](docs/INSTALL.md) | Downloads and installers |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers and the editing flow |
| [docs/FORMAT.md](docs/FORMAT.md) | Markdown round-trip, `.mdxb`, encryption |
| [docs/RELEASE.md](docs/RELEASE.md) | Release pipeline |
| [docs/CODEGRAPH233.md](docs/CODEGRAPH233.md) | Code graph MCP |
