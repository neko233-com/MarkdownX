# MarkdownX

A native Markdown editor for **Windows x64** and **macOS arm64**, written in Go with
[MyGo](https://github.com/egoist/mygo). No web view, no C toolchain, no runtime to install.

[中文说明](README-CN.md)

## Install

Download the latest build from
[GitHub Releases](https://github.com/neko233-com/MarkdownX/releases/latest):

- **Windows x64** — `markdownx-setup-<version>-amd64.exe` (per-user installer) or the
  portable `MarkdownX.exe`.
- **macOS arm64** — Open the DMG or extract `markdownx-<version>-darwin-arm64.tar.gz`, then drag `MarkdownX.app` into Applications.

The builds are not Authenticode-signed or notarized yet; Windows SmartScreen and macOS
Gatekeeper may ask for confirmation on first launch. The command-line tools
(`markdownx-server`, `mdx-convert`, `mdx-enc`) ship as separate archives.
See [docs/INSTALL.md](docs/INSTALL.md) for details and checksums.

## What it does

Open a folder from File to show its tree on the left. Click a Markdown file to read
and edit it on the right. Selecting another file replaces the current document in
place. Use the sidebar tabs or bottom-left button to switch between the file tree
and the document outline. Click a heading to jump to it. The footer can hide the
sidebar or switch to Markdown source.

- **Native rich-text editing.** The single-pane editor renders bold, emphasis, links and
  inline code as you type. `Shift+Enter` makes a hard break. Enter after a pipe-delimited
  header creates a table, and Tab moves through its cells. Drag or hold Shift to select
  across paragraphs.
- **Edit Markdown in place.** Move the caret into formatted text to see its Markdown.
  Click an image or inline equation to edit its source. Press `Cmd/Ctrl+K` to insert or
  edit a link, or hold `Cmd/Ctrl` while clicking to follow it.
- **Contents and footnotes.** Type `[toc]` and press Enter to insert a live table of contents.
  Hover over a footnote to read it, `Cmd/Ctrl`-click to jump to its definition, and click `↩` to return.
  Reference links in the forms `[text][label]` and `[label][]` resolve against document definitions.
- **Rich clipboard.** Copy a multi-paragraph selection as HTML, RTF and plain text at once.
  Copy and paste within MarkdownX preserves headings, inline markup and tables. HTML paste
  retains common structures such as headings, quotes and tables.
- **Markdown source.** Switch between the writing view and editable source with
  `Cmd/Ctrl+/`. Saving preserves untouched Markdown blocks, image references, BOM
  and line endings. If a file changed on disk, Save As can keep your edits in a new file.
- **Native preview.** Split and preview modes render with MyGo's Markdown engine.
  Use the outline to jump to a heading, F8 for focus mode and F9 for typewriter mode.
- **Markdown diagrams.** Enable diagram rendering in preferences to show `mermaid`,
  `flow` and `sequence` fences. Click a diagram to edit it and press Esc to return to
  its preview. The context menu copies or saves the image. HTML export embeds diagrams
  for offline use. Unsupported diagrams keep their editable source.
- **Code fences.** Edit the language below the fence to apply MyGo syntax highlighting.
  `Cmd/Ctrl+A` selects the current code fence while editing its contents.
- **Typeset equations.** Inline and display math draw as native vector images. Enter
  after `$$` to edit a display formula. Unsupported expressions keep their TeX source.
- **Import.** `.docx`, `.xlsx`, `.epub`, `.ofd`, `.pdf`, `.rtf` and `.html` open directly,
  converted to Markdown by pure-Go readers.
- **Export.** Markdown, self-contained offline HTML, PDF and `.mdxb` books. Configure
  paper, orientation, margins, headers, footers and page breaks for PDF. The output keeps
  selectable text and heading links. Print Preview saves and opens the PDF in the system viewer.
- **Books.** Open a `.mdxb` book — the bilingual, multi-chapter format whose chapters are
  DocxXML or Markdown and whose media travel inside the file — and export one back.
- **File tree.** The workspace sidebar shows folders and files in a single tree.
  It renders the visible rows while preserving keyboard navigation and selection across sorting and refreshes.
  When the same file or folder is renamed or moved within the workspace, the tree follows it without losing expansion, selection, or edits to the active Markdown document.
  Drag the divider or focus it and use the arrow keys to resize the sidebar. The width is remembered and double-clicking the divider resets it.
  Use the context menu to create, rename, duplicate, move, trash or undo an operation.
  Click a file or press Enter to open it in place. The context menu can open a file in a new tab or window. When several tabs are open, drag them to change their order. Each tab keeps its unsaved text, selection and undo history.
  Open in Split View places documents side by side in one tab. Drop a file at an editor edge or on a divider to insert a pane there. Drag a divider to resize or a pane header to reorder or detach it. Panes of the same document share edits and undo while keeping their own caret and scroll position.
  Drag a regular or split tab into another window's tab strip, or drop it outside a strip to open a new window near the pointer. Press `Esc` to cancel. The moved tab keeps its draft and undo history.
  Right-click anywhere on a row for its actions or use `Shift+F10` to bring the selected row back into view and open its menu. Deep branches use narrower indents when space is tight.
  Create a sibling directly from a file's context menu. Preferences let new files and templates default to `.md`, `.markdown`, `.text`, or `.txt` when the name has no supported extension.
  Choose New from Template on a file or folder to create a Markdown document in that directory. `typora-template` can supply the suggested filename and description. Title, date, time, clipboard, UUID, and cursor variables expand during creation. Use `Tab` and `Shift+Tab` to fill numbered placeholders such as `{{1:Title}}`. Repeated numbers update together.
  The tree highlights the destination while dragging. Release to move entries within the workspace and copy entries from elsewhere, or hold Alt/Option to copy the entire drop. Drop onto a file for its parent folder or into empty space for the workspace root. Use the tree's context menu to undo the drop as a single operation.
  Drag files from the tree, another MarkdownX window, or the system file manager into an editor. Supported images refer to the original image files. Other files and folders become links, with local file URLs for untitled documents or different volumes. Markdown source shows the insertion caret under the pointer. Code and equation source accept literal paths.
  Click the workspace name to sort, filter, or open the folder in the file manager, a terminal, or Visual Studio Code. This menu stays at the top as the tree scrolls. The footer's plus button creates files and folders.
  Pass a directory to MarkdownX to open its tree. Preferences can restore the last folder with or without its document, or open a new document with an optional default folder.
  Moving a Markdown file between folders updates its relative Markdown image destinations.
  Hover over a document or raster image for a preview. Pin frequently used folders and switch to them from Quick Open. Press `Ctrl/Cmd+K` in Quick Open to reveal a result in the tree without opening it.
  Workspace search groups filename and content matches by folder and jumps to the selected match.
- **Password save.** File → Password save encrypts a page with scrypt + secretbox; the
  standalone `mdx-enc` tool reads the same format.
- **Collaboration server.** `markdownx-server` shares a folder of Markdown over HTTP,
  records every accepted save as a Git commit, serves history, diffs and restore, and
  pushes other people's edits to open editors as they happen.

## Shortcuts

| Keys | Action |
|---|---|
| `Cmd/Ctrl+N` | New document |
| `Cmd/Ctrl+O` | Open a file |
| `Ctrl+P` / `Cmd+Shift+O` | Quick open on Windows / macOS |
| `Ctrl+Shift+1` / `Cmd+Ctrl+1` | Show and focus the document outline on Windows / macOS |
| `Ctrl+Shift+3` / `Cmd+Ctrl+3` | Show and focus the file tree on Windows / macOS |
| `Ctrl/Cmd+Shift+F` | Search workspace filenames or document contents |
| `Cmd/Ctrl+Shift+L` | Show or hide the sidebar |
| `F2` | Rename the focused tree item |
| `Delete` / `Cmd+Backspace` | Confirm moving the tree item to the Recycle Bin or Trash on Windows / macOS |
| `Cmd/Ctrl+Shift+N` | New window |
| `Cmd+T` | New tab on macOS |
| `Cmd/Ctrl+W` | Close tab |
| `Cmd/Ctrl+Shift+T` | Reopen a closed tab or file |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl/Cmd+Alt+\` | Open the current document in a split on the right |
| `Cmd+Ctrl+←` / `Cmd+Ctrl+→` | Focus the split to the left / right on macOS |
| Cmd + backtick | Switch windows on macOS |
| `Cmd/Ctrl+S` | Save |
| `Cmd/Ctrl+Shift+S` | Save as Markdown |
| `Cmd/Ctrl+Shift+E` | Export HTML |
| `Cmd+P` | Print preview on macOS |
| `Cmd/Ctrl+B` / `I` | Bold / italic |
| `Cmd/Ctrl+K` | Insert a link or edit its destination |
| `Cmd/Ctrl+F` / `H` | Find / replace |
| `Cmd/Ctrl+A` | Select the document, or the current fence while editing code |
| `Shift+Arrow` | Extend the selection |
| `Ctrl+Shift+Home/End` / `Cmd+Shift+↑/↓` | Extend to the start or end of the body on Windows / macOS |
| `Cmd/Ctrl+Shift+C` / `V` | Copy as Markdown / paste plain text |
| `Cmd/Ctrl+/` | Toggle Markdown source |
| `F8` / `F9` | Focus / typewriter mode |
| `F11` / `Cmd+Option+F` | Full screen on Windows / macOS |
| `Cmd/Ctrl+1`…`6` | Heading levels |
| `Cmd/Ctrl+0` | Back to paragraph |
| `Cmd/Ctrl+D` / `Cmd/Ctrl+Shift+D` | Select word / delete word |
| `Cmd/Ctrl+[` / `]` | Indent / outdent |
| `Alt+↑` / `↓` | Move paragraph or table row |
| `Cmd/Ctrl+Enter` / `Cmd/Ctrl+Shift+Backspace` | Insert / delete table row |
| `Ctrl+Shift+M` / `Cmd+Option+B` | Formula block on Windows / macOS |
| `Cmd/Ctrl+Z` | Undo |
| `Enter` | Split the block |
| `Tab` | Move to the next table cell or indent a block |
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
