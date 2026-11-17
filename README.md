<h1 align="center">MarkdownX</h1>

<p align="center">
  <b>Interactive books, not static pages.</b><br>
  A desktop authoring app and a browser reader for the next generation of learning material.
</p>

<p align="center">
  <a href="https://github.com/neko233-com/MarkdownX/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/neko233-com/MarkdownX/ci.yml?branch=main&label=CI" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT"></a>
  <a href="README-CN.md">简体中文</a>
</p>

![MarkdownX reader](docs/images/reader.png)

## What it is

MarkdownX is a writing and reading environment for **dynamic books**: documents that can contain a draggable parabola, a pendulum you can push, a quiz, a table you can sort, or a whole HTML experiment.

- **Write once, read anywhere.** A book is one `.mdxb` file. Text, images and interactive blocks travel together — no folder of loose assets, no server.
- **Markdown and Feishu/Lark DocxXML.** Edit common Markdown syntax and `.docxxml` files in a workspace, or import an authorized Feishu document. Typora compatibility is still a work in progress.
- **Interactive blocks.** Declarative `canvas` blocks run the built-in quadratic, unit-circle and projectile experiments on desktop and in the web reader. HTML/JS runs in a sandbox on Windows desktop and in the web reader; local data can be embedded while external network connections are blocked.
- **Bilingual by design.** Interface, book metadata and chapter text can carry Chinese and English side by side.
- **A reader that fits the screen.** The web reader is a build-free, responsive page: a single column on a phone, a table of contents and a comfortable measure on a desktop.
- **Local first.** Local editing needs no account; Feishu import uses your optional CLI authorization. Your workspace files stay on your computer.
- **Your folders are your workspace.** Keep multiple folder trees open together and edit their `.md` files in place. Search across them; use each folder’s menu to manage its files and folders. Git-backed saves can create commits for touched files.
- **Built for team workflows.** Game design, product, film and editorial starter templates live in `templates/`; the collaboration server exposes them to the browser. See [docs/WORKFLOWS.md](docs/WORKFLOWS.md).
- **Notes live beside the text.** Temporary comments are stored in `.annotations.json` sidecars, so reviewers can hide, resolve, batch-edit and merge them without touching the prose.
- **Follows your computer.** The first launch follows the system language (Chinese or English) and the setting can pin either one. The app checks the GitHub releases page after launch and every 12 hours, and announces a newer version with its download page. **Settings → About → Check Now** works even when automatic checks are disabled, keeps the result visible, and offers a download-page button when an update is available. Failed checks can be retried; pre-releases are excluded.

## Two ways to use it

| Author | Read |
|---|---|
| The GPUI desktop app: open a folder, edit `.md`, insert Canvas/HTML blocks, annotate, then package a `.mdxb`. | The Chrome reader in `web/`: open a `.mdxb`, browse chapters, resize the window, switch language and theme. |

| Landing | Canvas chapter |
|---|---|
| ![Landing](docs/images/landing.png) | ![Reader](docs/images/reader.png) |

## Install a release

The commands below require the corresponding GitHub Release to be published. The Windows stable release contains an x86_64 MSI. Older macOS test pre-releases contain an ad-hoc signed, unnotarized Apple Silicon DMG. The in-app update checker follows stable releases and does not announce test pre-releases.

Windows x86_64 (PowerShell 7, current user):

```powershell
irm https://raw.githubusercontent.com/neko233-com/MarkdownX/main/install.ps1 | iex
```

macOS arm64 (Apple Silicon):

```sh
curl -fsSL https://raw.githubusercontent.com/neko233-com/MarkdownX/main/install.sh | sh -s -- --allow-unsigned
```

The installer verifies the package checksum; Windows requests normal save/exit and preserves the collaboration server and library. macOS installs to `~/Applications/MarkdownX.app`: close an existing copy first. For the unsigned test build, macOS may block the first launch; review the warning and approve opening it in **System Settings → Privacy & Security** if you trust this build. The installer does not change Gatekeeper settings. For offline installs, download the MSI or DMG and its `.sha256`, then use `pwsh -File ./install.ps1 -MsiPath <file>` or `sh ./install.sh --package <file> --allow-unsigned`. Add `-DryRun` or `--dry-run` to inspect without installing. In Windows Git Bash, `sh ./install.sh` delegates to PowerShell 7.

Releases are verified and packaged locally, then uploaded directly to GitHub Releases. Pushing a tag does not start an automatic publisher. The optional **Actions → Release** workflow is an explicit fallback for cross-platform test builds and requires an existing tag and `PUBLISH vX.Y.Z` confirmation.

## Quick start

**Requirements:** Rust 1.99.1, and on Windows the Visual Studio 2022 C++ build tools plus a Windows SDK. Python 3 is only needed for the examples and a local web server.

```sh
cargo run --release
```

On Windows, run the build from a Visual Studio developer prompt, or call `vcvars64.bat` first.

To install the built app for the current Windows user, with Start Menu/desktop shortcuts and `.mdxb` registration:

```powershell
pwsh -File scripts/install-windows.ps1
```

No administrator rights are needed. The local MSI is preferred. The app goes to `%LOCALAPPDATA%\Programs\MarkdownX`; the server token and workspace remain in `%LOCALAPPDATA%\MarkdownX\server`. The installation is registered in **Apps & features**. The helper refuses to force-close a running installed app or server.

Run `pwsh -File scripts/build-msi.ps1` after the release build to create `dist\MarkdownX-<version>-x64.msi`. The MSI installs the app, optional collaboration server, web UI, templates, shortcuts and `.mdxb` association, and registers MarkdownX as an available app for `.md` files. To make it the default, use **Settings → General → Default app** in MarkdownX and choose MarkdownX for `.md` in Windows Settings, or choose it through a file's **Open with** menu. Windows keeps the user's existing default until they change it. Collaboration starts only when its Start Menu shortcut is selected. The local reader starts on demand and serves only assets embedded in the app. WiX Toolset 3.14 is needed only at build time.

**My documents** shows `MarkdownX-Demo` inside the system Documents folder by default; on Windows this is normally `C:\Users\<user>\Documents\MarkdownX-Demo`. The app creates the default folder if it is missing. Use the **+** beside My documents, **Settings → Documents**, or **File → Open Folder…** to add folders. Their trees stay visible together and are restored on the next launch. Each root’s **…** menu can change its display name or folder, refresh it, or remove it from the list without deleting files. An unavailable folder remains listed without blocking the others. A folder passed on the command line is temporary.

On first launch, an empty default folder receives an editable sample in the interface language, and the same file opens in the editor and appears in the folder tree. The library guides use that language too. Existing files are never replaced; opening an explicit file or folder takes priority. Changing the interface language does not translate or overwrite documents you already have. The last opened file in a saved workspace is restored when it is still available.

To create the samples in an existing installation, use **Settings → Documents → Create demo** and choose a location. MarkdownX creates a separate folder with a new name if necessary, without overwriting an existing folder.

Settings → **Extensions** lists built-in formats and installed external extensions. A plugin is a folder with `plugin.json` under the app data `markdownx/plugins` directory (override with `MARKDOWNX_PLUGIN_DIR`); its executable stays inside that folder. A matching file starts the plugin, asks it for Markdown, and shows the result as a read-only preview, so the original format is never rewritten. **Install extension…** copies a selected folder into that directory, and plugins can be switched off individually; a manifest that conflicts with a built-in or another plugin is skipped as a whole. The repository's `MarkdownX-Demo` folder contains Markdown and Excel samples plus a reference plugin.

Images (including animated GIFs), audio, video and 3D models open in a preview: images zoom, audio shows a decoded waveform and duration, video shows the shell thumbnail plus metadata, and OBJ/STL/glTF/GLB models can be rotated and zoomed. Audio and video also offer **Open with system app**. These are built-in providers in the same suffix registry as extensions, so a plugin cannot silently take over their suffixes.

On Windows, PDF files open in a read-only, page-by-page preview using Windows' PDF renderer. Zoom and scroll to inspect details, and use the first, previous, next and last page buttons or Page Up, Page Down, Home and End. Ctrl with `+`, `-` or `0` changes or resets the zoom. Ctrl+F or Find text searches extracted text and navigates matching pages; it does not highlight words on the page. Scans, unusual fonts and pages that fail extraction may not be searchable; the preview reports pages with no text or extraction failures. Ordinary CAJ files and PDFs with a CAJ extension are converted in memory for preview and search; the original is unchanged. Support depends on the CAJ variant; KDH, HN/C8, protected and damaged files require another application. Text selection, OCR and annotation are not yet available. The demo workspace includes a three-page PDF.

On Windows, PDF attachment cards and inline attachments open in the same read-only viewer. DocxXML workspace attachments resolve from their document directory first; `./` keeps that lookup explicit. The demo includes `论文附件示例.docxxml` with both forms.

In workspace Markdown, local image paths are resolved from the Markdown file's own directory. Both `./shots/01.svg` and `shots/01.svg` work; `./` is recommended. Paragraphs and table cells display text mixed with images, including SVG storyboard frames and single-line `<img src="./shots/01.svg" alt="frame">` tags. Standalone HTML image tags may span lines and support pixel dimensions; inline images use the inline layout. Simple text edits preserve original references and HTML attributes, and image event handlers are not executed. Standalone embedded audio, video, models and HTML games also render in the Windows desktop. Media inside a balanced HTML `div`, including nested containers, is supported as an interactive block. Table cells accept complete single-line audio, video or Canvas tags, media containers and their scripts; escape pipes inside a cell as `\|`. Other embedded forms are not yet fully supported.

The desktop also displays Base64 `data:image/...` images (PNG, JPEG, GIF, WebP, BMP and SVG), including images mixed into text and tables; each decoded image is limited to 20 MB. Markdown export preserves the original data, and the built-in HTML/PDF exporter uses the same image validation. External file references inside embedded SVG are not loaded; SVG text requires available system fonts.

Research Markdown supports inline `$...$` and single- or multi-line `$$...$$` display equations. Desktop preview, books and built-in HTML/PDF export retain their TeX source; editing a display equation preserves its block layout. The web reader uses bundled KaTeX to generate native MathML for nested fractions, roots, subscripts, sums and integrals; a browser with MathML support is required. Invalid web equations or those over 16,384 characters show their source, and code examples stay literal. Image, link and footnote syntax inside equations remains literal rather than being collected as a resource.

PDF is static: the built-in exporter shows a standalone video's poster when provided, adds playback notes for video and audio, and attempts to preserve the initial appearance of HTML canvases. Use the HTML export or the MarkdownX document for interaction.

Image destinations can contain balanced parentheses, as in `![frame](./shots/take(1).svg)`. Enclose paths containing spaces in angle brackets, as in `![frame](<./shots/take 1.svg>)`. These forms also package into books and display in the web reader.

SVG URL fragments can select a named `<view>` or `svgView(viewBox(...))`, for example `![view](./shots/frames.svg#closeup)` or `![view](<./shots/frames.svg#svgView(viewBox(50,0,50,50))>)`. A literal `#` in a filename must be written as `%23`. Desktop rendering and the built-in standalone exporter support view boxes, `preserveAspectRatio`, and `:target` in selectors supported by the desktop SVG engine. Other SVG view-spec features and unsupported CSS selectors remain limited. The browser reader uses the browser's SVG implementation.

The command line can export a Markdown file as HTML or PDF:

```sh
markdownx convert ./story.md --to html
markdownx convert ./story.md -o ./story.pdf --strategy builtin
markdownx convert ./story.md -o ./story-pandoc.pdf --strategy pandoc --pdf-engine typst
markdownx convert ./paper.md -o ./paper.html --citeproc
markdownx convert ./paper.md -o ./paper.pdf --citeproc
markdownx convert ./paper.md -o ./paper.docx --citeproc --bibliography ./refs.bib --csl ./journal.csl --reference-doc ./paper-template.docx
```

`auto` uses the built-in HTML renderer; PDF tries installed Chromium browsers and then Pandoc, trying Typst for the fallback when `pdflatex` is unavailable or fails. DOCX, ODT, RTF and EPUB use Pandoc. On Windows x64, MarkdownX uses programs on `PATH` first and otherwise downloads pinned, SHA-256-verified Pandoc and, when needed for PDF, Typst into the current user’s local application data. The default cache is `%LOCALAPPDATA%/markdownx/format-tools`; `MARKDOWNX_FORMAT_TOOLS_DIR` can override it. Set `MARKDOWNX_FORMATS_OFFLINE=1` to disable downloads; other platforms require separate installations. With `--citeproc`, `auto` uses Pandoc to render citations to HTML and, for PDF, prints that HTML through Chromium when available. `--pdf-engine typst` or another engine selects Pandoc's direct PDF route. `builtin` selects the MarkdownX renderer; `pandoc` explicitly selects Pandoc and needs a PDF engine for PDF output. Use `--browser PATH` to select a browser; it cannot be combined with `--pdf-engine`. For auto PDF, `--no-browser` chooses the Pandoc fallback even when a browser exists. For DOCX/ODT, `--reference-doc` selects a matching style template; `--citeproc` processes citations from document metadata or files given with `--bibliography` and `--csl`, whose paths are relative to the command's working directory. Pandoc warnings during citation processing fail the conversion to prevent silently missing references. The built-in strategy embeds local images and HTML widget resources. The web reader preserves nested list structure and starting numbers, including local audio, video and Canvas inside list items. Desktop parsing preserves Chinese-numbered and tab-indented hierarchy; simple nested text edits, including plain text alongside emphasis and reference-style images, keep original image markup when the source mapping is unambiguous. Image examples in indented or fenced code remain literal, while real images in footnotes and lists are embedded. Its HTML and browser-printed PDF exports render referenced top-level Markdown footnotes with backlinks; the web reader also renders these notes in Markdown-format book chapters. Built-in HTML/PDF and web chapters retain source heading fragments when headings contain footnotes, including duplicate headings. Web Markdown and DocxXML heading anchors use image alt text and equation source, matching desktop navigation. Desktop Markdown reading mode renders top-level footnotes with links and returns, and preserves original heading fragments when headings contain footnotes; desktop editing and DocxXML-format book chapters still display the footnote syntax as text. Open reading mode with the eye button or Ctrl+Alt+V and return with the pencil button or E; changes are saved before switching, annotations are temporarily unavailable, and edit history is retained. Unsupported or unreferenced definitions remain in the output. HTML links to files within the asset root are rebased against the output directory, including links to files that have not been created yet, using relative paths when possible; links outside the asset root or through symlinks fail conversion, and `--root` can widen the boundary; built-in and Pandoc HTML use file URLs pointing to the original local files across volumes. PDF viewers may restrict local file links. PDF is a static print result, and built-in `mdxb-canvas` blocks currently show a placeholder; Pandoc exports do not retain MarkdownX's interactive runtime. Local assets are limited to the Markdown directory by default; `--root DIR` widens that boundary. Pandoc follows its own resource rules and uses `--root` as a secondary search directory. Existing output requires `--force`. On Windows, the installer provides a console entry point; open a new terminal after installation.

Local Markdown images and HTML media can carry a query such as `./frame.svg?v=1`; the query is ignored when reading the local file, and the original Markdown stays unchanged. HTML video and audio sources also support time fragments such as `./clip.mp4?v=1#t=2,5` in desktop preview, packaged books and built-in HTML export; playback depends on the media and viewer. Literal `?` and `#` in filenames should be percent-encoded. Local script URLs also accept queries such as `./game.mjs?v=1`; module queries follow browser URL normalization in desktop preview, packaged books and built-in HTML export, so equivalent Unicode and percent-encoded queries share one module instance. Local SVG fragments are not supported here.

Reference-style images such as `![frame][shot]` with a single-line `[shot]: ./shots/01.svg` definition also work, including forward references, collapsed and shortcut forms. Editing retains definitions; uploading one referenced image changes that use to a direct URL without changing other uses of the definition.

Network reference links also support single-line definitions, for example `[paper][source]` and `[source]: https://example.com/`. In a desktop workspace, direct and reference links can open supported files relative to the current Markdown file, with or without `./`; targets must exist within the workspace. Markdown and DocxXML headings can be reached with `#heading` or `./chapter.md#heading`; repeated headings use `-1`, `-2`, and so on. On Windows, PDF and supported CAJ files accept one-based page links such as `./paper.pdf#page=2`; an out-of-range page reports an error and shows the first page. Books packaged from folders retain chapter source paths so imported desktop books and the web reader can follow links to included chapters and headings. Older books without source paths cannot resolve other chapters automatically. New folder-packaged books also embed local PDF links from chapters and translations, preserving optional `#page=2` fragments; each PDF is limited to 16 MiB and must remain inside the book folder. Imported books open them in the Windows read-only preview, while the web reader opens an embedded copy in the browser PDF viewer. The source PDF is unchanged. Other non-chapter file links, query strings and other PDF fragments are not supported in books yet.

Images inserted, pasted or dropped into workspace Markdown are stored in an `assets` folder beside the document, with new links written as `./assets/...`; existing links are retained. Select a local image and choose **Upload to PicGo** in its toolbar to use the current configuration of a locally running PicGo server. Images mixed with text in paragraphs or table cells can also be clicked and uploaded. A successful upload replaces that image link and keeps the local image; if the image or its surrounding text changes during the upload, the new edit is not overwritten. The default endpoint is `http://127.0.0.1:36677/upload`; change the local address and its configuration parameters in **Settings → General → Image upload**. A nonempty `MARKDOWNX_PICGO_ENDPOINT` overrides this setting. `PICGO_SERVER_SECRET` supplies an optional server token, which is not stored in preferences. Uploads happen only when requested.

`.docx`, `.docm`, `.dotx`, `.dotm`, `.odt`, `.ott` and `.rtf` open as read-only text previews. On Windows, eligible DOCX, DOCM, DOTX and DOTM files also get a sandboxed layout preview with common tables and images, followed by extracted text; archives over 16 MiB, expanded content over 128 MiB, excessive entries or unsupported structure fall back to text. The layout is approximate; macros and embedded external HTML do not run. Extracted text labels recognizable inserted and deleted revisions and includes referenced notes, comments, headers and footers where recognized; missing related content is reported. RTF supports common encodings and includes hidden text, with an 8 MiB input limit and a complexity limit. `.ofd` extracts read-only page text; on Windows, eligible files also get an offline sandboxed layout preview with page navigation, subject to the same archive limits. Complex objects and digital seals may be missing, and diagnostics appear in the preview; `.tex`, `.ltx`, `.sty`, `.cls`, `.bst`, `.bib`, `.ris`, `.enw`, `.nbib`, `.csl`, `.rmd`, `.qmd` and `.typ` open as read-only UTF-8 source. OFD pages may contain only images or use special font mappings; extracted text can be missing or out of order. Previews can omit layout, images, comments and seals and do not verify digital signatures, so check the original when fidelity matters. On Windows, legacy `.doc` and `.dot` open as read-only text previews in a time- and memory-limited helper process, including body text, notes, recognizable revisions and some headers and footers; input is limited to 32 MiB. Protected, damaged or unsupported files require another application. Text, ordering and formatting may be incomplete; consult the original. The demo workspace includes DOC, DOCX, RTF, OFD, Quarto, a Markdown paper with a relative bibliography, and storyboard examples; the paper and citation are fictional.

On Windows, EPUB files have a read-only preview of spine-ordered text, local images, common tables, MathML and sanitized static SVG; input is limited to 32 MiB. Scripts do not run, and styles, links, animation or other content may be omitted. EPUB files containing `META-INF/encryption.xml`, including those with obfuscated fonts, currently require another reader. The original is never modified; the demo workspace includes `电子书论文示例.epub`.

Inside an HTML widget, relative image candidates in `img` and `source` `srcset` attributes are resolved from the chapter directory in desktop, packaged books and built-in HTML/PDF export. Descriptors are preserved; both `./frame.svg` (recommended) and `frame.svg` work.

Standalone Markdown HTML blocks such as `<video src="./clip.mp4" controls></video>`, `<audio>` and `<canvas>`, with immediately following scripts and styles, run in the Windows desktop and web reader sandboxes. Local `<script src="./game.js"></script>` and `<link rel="stylesheet" href="./game.css">` references resolve relative to the Markdown file. Local `url()` images and fonts and nested `@import` files in external CSS resolve relative to their stylesheet and are bundled with it; references in HTML `style` attributes and `<style>` blocks resolve relative to the chapter. Missing, circular or out-of-workspace dependencies are rejected. Inline and external `<script type="module">` blocks bundle static relative imports, re-exports and literal `import()` dependencies recursively, relative to the importing module. Literal dynamic `import()` in classic scripts is also bundled relative to the script; ES module specifiers require `./` or `../`. Literal `new URL("./frame.svg", import.meta.url)` references also embed module-relative local assets for images and media; both `frame.svg` and `./frame.svg` work. Asset query parameters do not change embedded bytes, and SVG fragments are retained. Literal `fetch("./data.json")` and `fetch("data.json")` in inline or external JavaScript embed local data relative to the Markdown document; `fetch(new URL("./data.json", import.meta.url))` uses the module directory. Common Canvas scripts can also embed literal local media for `const image = new Image(); image.src = "./frame.svg"`, equivalent `document.createElement("img")`/`setAttribute("src", ...)` assignments in the same block, and `new Audio("./sound.mp3")`. These paths are relative to the Markdown document; computed paths and assignments across blocks are not resolved, and potentially shadowed built-in names are conservatively skipped. Only embedded data URLs are available to script requests; external network connections remain blocked. Computed fetch paths, circular imports, escaped paths and computed dynamic imports are not supported. Windows desktop uses WebView2; HTML on macOS and Linux still requires the web reader. The Windows MSI includes a local web reader at `http://127.0.0.1:8877/reader/` when the collaboration server is running. Package a document as `.mdxb` and select it in the reader.

Standalone `<model-viewer src="./model.glb"></model-viewer>` blocks support OBJ, STL, glTF and GLB. Models can be rotated and zoomed inside desktop documents and in the web reader. Local glTF/GLB buffers and textures are embedded alongside the model when packaging. Desktop uses a simplified geometry preview without materials, textures or animation; OBJ material libraries and formats requiring extra Draco/KTX2 decoders are not yet supported in the web reader.

When packaging folder chapters, recognized Markdown images in translations are resolved beside the translation and embedded into the book. The agent CLI also embeds recognized chapter-relative images and rejects missing or out-of-folder local images; code examples remain literal. In the web reader, embedded Markdown images display in paragraphs and table cells. DocxXML chapter and translation images, covers and attachments are resolved relative to their own source files and embedded when they are inside the book folder.

Workspace `.xlsx`, `.xlsm`, and `.xlam` files open in the spreadsheet editor; create a new `.xlsx` from a workspace folder. You can edit cells, use local formula calculation where supported, undo and redo, copy and fill ranges, manage worksheets, and search the current sheet or replace text, formula source, and values separately. Replacement can match case or the entire cell. Value replacement uses the underlying number (for example, 50% is `0.5`); dates use `YYYY-MM-DD`. For selection arithmetic, enter a number or percentage and choose +, −, ×, or ÷; text, booleans, and calendar dates are skipped. Formulas are optional: when included, selected formulas and their selected input cells are both changed. The grid displays common workbook colors, font emphasis, alignment and borders, plus supported conditional colors and number formats. Large conditional-format ranges are skipped with a notice; dashed and double borders are approximated. Selection number formats include integer, decimal, grouped, percentage, scientific and date formats, with undo and redo. Protected worksheets allow cells known to be unlocked; locked or ambiguous cells cannot be saved. Protected hidden formulas are omitted from the editor and formula search; copy their calculated results using Copy values. Worksheet protection does not encrypt the file. This support is still in development: some structures cannot be edited safely, and local formula calculation is unavailable for workbooks using the 1904 date system.

The separate MarkdownX library remains at `~/Documents/MarkdownX` (override with `MARKDOWNX_HOME`); its documents remain available from Home and search, while the sidebar focuses on workspace folders. Changing folders does not move or overwrite the library. Use **File → Package as E-book…** (`⌘⇧E`) to turn the current folder, or the open Markdown file, into one `.mdxb`. **下载为 → MarkdownX 书 (.mdxb)** exports a library page and its sub-pages. On Windows the installer registers `.mdxb`, so double-clicking a book opens and imports it.

Passing a folder path as the first argument opens its tree without selecting a file. The tree shows regular files and uses distinct, theme-aware icons for folders, Markdown, text, books, code, images and other common types. Markdown, text and `.mdxb` open in MarkdownX; use **Open with system app** in its menu or the global search result to open another type elsewhere. Filename search includes visible files; content search reads supported documents. Use **Refresh** after external changes; returning focus to the window also refreshes the tree. Dot-prefixed entries, symlinks, `target`, `node_modules` and `__pycache__` are excluded. Unchanged Markdown source is preserved where possible; unsupported edited constructs may still change; UTF-8 BOM and predominant CRLF line endings are retained. If another program changed the file, or an export would lose detected text, references a missing asset, or conflicts with a different existing asset, MarkdownX keeps the pending edit instead of replacing the source.

**File → Open File…** (`Ctrl+O` on Windows) and a file path argument open Markdown or text files in place, with their folder tree. Use **+** beside a folder to create a Markdown file; its **…** menu also creates folders. The **…** menu on a file or folder renames it or asks before moving it to the system recycle bin. Existing paths are never overwritten by these actions. Folder saves and link indexing run in the background; switching or closing waits for pending edits to finish. Existing staged changes to the same file prevent Git auto-commit and are reported without undoing the file save.

Press `Ctrl+K` (`⌘K` on macOS) to search library documents and all open folders by name, path and text. Use ↑/↓ and Enter to open a result, or Esc to close search. The previous `Ctrl+J` / `⌘J` shortcut remains available; insert a link with `Ctrl+Shift+K` / `⌘⇧K`.

To read a book in the browser, use the hosted reader at **https://neko233-com.github.io/MarkdownX/web/**, or run it locally:

```sh
python -m http.server 8765 --bind 127.0.0.1
# open http://127.0.0.1:8765/web/
```

The same reader deploys to Vercel with `vercel.json`; `npx vercel deploy` builds `.vercel-out` from `web/` and `examples/` only. A temporary, claimable deployment can be made without an account: `npx vercel deploy --temporary`.

The landing page ships with five examples, including two Chinese middle-school books: **力与运动** (physics) and **物质与变化** (chemistry). You can also drag any `.mdxb` onto the page.

## Import from Feishu

Open **Import from Feishu** on Home or in the File menu and paste a document or wiki link. Install the [official Feishu CLI](https://github.com/larksuite/cli), run `lark-cli config init` and `lark-cli auth login`, and grant read access to the document first. MarkdownX uses that authorization to fetch the full DocxXML into the active workspace, without overwriting files; the original response is kept in an adjacent hidden sidecar.

The current import covers document text and supported DocxXML blocks. Images, attachments, comments and embedded applications are not downloaded or converted automatically; their source references are retained. Markdown exported from Feishu can also be opened offline. `.docxxml` is an XML document, not a Word `.docx` file.

## For agents

A book can be maintained as a folder of `NN-topic.md` chapters. Coding agents can validate and package it without opening the GUI:

```sh
python tools/agent/mdxb.py check examples
python tools/agent/mdxb.py pack books/physics --title "力与运动" -o physics.mdxb
python tools/agent/mdxb.py unpack physics.mdxb books/physics
python tools/agent/mdxb.py search books/physics 速度 --content
```

See [agent/README.md](agent/README.md) for the chapter dialect, annotation sidecar contract and review loop, and [agent/AGENTS.md](agent/AGENTS.md) for rules to give an agent.

## Collaboration server

`server/` is an optional Rust server for teams that want one shared workspace instead of passing `.mdxb` files around. The repository root is the workspace: nested `.md` paths, Chinese filenames and folders are supported, every accepted save is a Git commit, and the browser editor has live presence, history, diff and restore. Point it at a team repo and put it behind TLS on the intranet.

```sh
MARKDOWNX_SERVER_TOKEN=change-me cargo run -p markdownx-server -- \
  -root ./data -web server/web
# open http://127.0.0.1:8877/
```

The editor shows who else is in the room, saves over WebSocket, and resolves stale writes with a three-way merge. Overlapping edits are reported as a conflict with **load latest**, **use merged draft** and **overwrite remote** choices instead of silently dropping text. The history panel reads Git commits, shows a unified diff, and can restore any revision as a new commit. A multi-stage `Dockerfile` is included.

The server is single-tenant and protected by one shared token; put it behind TLS. See [server/README.md](server/README.md) for flags, the HTTP/WebSocket protocol, the conflict model, workspace paths, templates and intranet deployment notes. Workflow conventions are in [docs/WORKFLOWS.md](docs/WORKFLOWS.md).

## The `.mdxb` format

`.mdxb` is a UTF-8 JSON document. The extension is MarkdownX Book; the media type is `application/vnd.markdownx.book+json`.

```json
{
  "format": "markdownx.book",
  "version": 1,
  "id": "quadratics",
  "title": { "zh": "二次函数", "en": "Quadratics" },
  "languages": ["zh", "en"],
  "default_language": "zh",
  "chapters": [
    {
      "id": "chapter-1",
      "title": { "zh": "从一条曲线开始", "en": "Start with a curve" },
      "format": "docxxml",
      "content": "<title>…</title><h1>…</h1>",
      "content_en": "<title>…</title><h1>…</h1>"
    }
  ],
  "assets": {
    "assets/cover.png": { "mime": "image/png", "data": "iVBORw0KGgo…" }
  }
}
```

Every chapter is either `docxxml` or `markdown`; `content_en` is optional. When its format differs from the main chapter, v2 adds `format_en`; v1 readers reject v2 instead of misreading it. Imported translations and their assets survive restarts and chapter reordering, but editing one language does not translate the other.

### Markdown ↔ book

`文件 → 打包成电子书…` turns a folder of Markdown chapters into one `.mdxb`. `下载为 → Markdown 文件夹 (.md)` goes the other way: it writes `NN-title.md` chapters, a same-stem `.xml` DocxXML source per chapter, `book.json` and copied assets beside a small index file. Tables, headings, chapter order and interactive `mdxb-canvas` / `mdxb-html` sources survive; callout tints, grids and other editor-only presentation may degrade to plain Markdown. The XML source keeps the full structure, so packing the folder again recreates the book; if you edit a `.md`, the newer Markdown wins over its XML. XML chapters and translations resolve title images, covers and attachments relative to their own files; a local manifest cover resolves from the book root. Local assets must stay within that root. PDF links from the library are copied into the exported folder, including page fragments; both the Markdown and DocxXML sources remain portable.

The browser reader displays embedded DocxXML attachments as downloads; attachments not embedded in the book show their names only.

### Interactive blocks in DocxXML

```xml
<interactive type="canvas" title="拖动参数" height="360"><![CDATA[
  {"kind":"quadratic","a":1,"b":0,"c":0,"xMin":-4,"xMax":4,"yMin":-4,"yMax":8}
]]></interactive>
```

```xml
<interactive type="html" title="单摆" height="320"><![CDATA[
  <canvas id="c"></canvas><script>/* sandboxed */</script>
]]></interactive>
```

In Markdown, the same block is a fenced code block with the info string `mdxb-canvas` or `mdxb-html` and a JSON envelope:

````markdown
```mdxb-canvas
{"title":"拖动参数","height":360,"source":"{\"kind\":\"quadratic\",\"a\":1}"}
```
````

The full schema and the round-trip rules are in [docs/FORMAT.md](docs/FORMAT.md).

## Project layout

```
crates/core/        fdl_core: model, DocxXML, Markdown, editor core, .mdxb, library
src/                GPUI desktop app: editor, reader UI, import/export, settings
web/                build-free browser reader (HTML/CSS/ES modules)
server/             Rust collaboration server + browser editor (Git history, diff)
templates/          game, product, film and editorial workflow starters
examples/           five .mdxb books and the Python generators
agent/              book-authoring rules for coding agents
tools/agent/        check, pack, unpack, search, merge-annotations CLI
shared-mock/        built-in guide library and its sources
docs/               architecture, format, themes, performance
```

The document engine does not depend on GPUI, so it can be tested and reused by other front ends. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build and test

```sh
scripts/check.sh                         # release tests + clippy
cargo test -p fdl_core                   # engine only
FDL_FUZZ_SEEDS=2000 cargo test --release random_edits
```

The web reader has no build step. Run `node --check` on each file under `web/js/` after editing, then open the page through a local server.

The collaboration server is a workspace member:

```sh
cargo test -p markdownx-server
cargo clippy -p markdownx-server --all-targets -- -D warnings
```

## Status

This is an early public snapshot. Working today: the desktop editor and library, multiple folder workspaces with directory trees and fuzzy/full-text search, direct `.md` editing, Markdown/DocxXML interchange, `.mdxb` import/export and packaging, Markdown-folder export with retained interactive sources, the responsive web reader, Canvas/HTML blocks, `[[wiki links]]`, annotation sidecars with batch resolve/delete, and a self-hosted Rust collaboration server with live presence, three-way merge conflict handling, and Git history/diff/restore. The desktop app runs the built-in `quadratic`, `unitcircle` and `projectile` Canvas experiments natively; Windows desktop and the web reader run HTML in a sandbox. On Windows it falls back to WARP when a hardware D3D11 device is unavailable, so the installed app opens even on VMs and RDP sessions; the fallback is covered by a captured window smoke test. The Windows MSI registers `.mdxb` and starts collaboration only on request; macOS Info.plist and the Linux MIME package register `.mdxb`, while double-click behavior on macOS/Linux still needs a real desktop check. Mobile shells are a later milestone. See [issues](https://github.com/neko233-com/MarkdownX/issues).

## License

MIT. MarkdownX is a fork of [Quiet.md](https://github.com/s1dashu/quiet-md) by s1dashu; the original copyright notice is kept in [LICENSE](LICENSE). Third-party components, fonts and icons are listed in [THIRD_PARTY.md](THIRD_PARTY.md). Most bundled UI icons are drawn for this project; the Lucide-derived ones are ISC. No Nucleo icons are included.

---

## 简体中文

<h3 align="center">MarkdownX</h3>

<p align="center"><b>书不该只是读到这里。</b><br>给下一代学习者的可交互书本：桌面创作端 + 浏览器阅读端。</p>

### 它是什么

MarkdownX 用来写和读**动态书**：一本可以拖动的抛物线、可以推一把的单摆、可以点的测验、可以排序的表格，或者一整段 HTML 小实验。

- **写一次，到处能读。** 一本书就是一个 `.mdxb` 文件，文字、图片和交互块都在里面，不需要服务器。
- **Markdown 与飞书 DocxXML。** 章节可以是任意一种；Markdown 导入即用，DocxXML 保留标题、表格、高亮块和颜色。
- **交互块。** 二次函数、单位圆、抛体 Canvas 可在桌面和 Web 阅读器运行；HTML/JS 在 Windows 桌面和阅读器的沙箱中运行，外部资源和脚本网络连接受到限制。
- **原生中英双语。** 界面、书名和章节正文都可以同时带中文和英文。
- **为屏幕而生的阅读器。** 手机单列，桌面目录 + 正文；无构建步骤，Chrome 直接打开。
- **本地优先。** 本地编辑无需登录；飞书导入使用可选的 CLI 授权，工作目录保存在自己的电脑上。
- **选择自己的文件夹。** 目录树直接编辑所选目录里的 `.md`；文件属于 Git 仓库时，保存可为改动的文件生成提交。同一个树里也能放 `.mdxb`，支持模糊路径搜索、内容搜索、`[[双链]]` 和行内批注。
- **团队工作流开箱即用。** `templates/` 里有游戏策划、产品 PRD、影视 treatment、内容选题等模板；浏览器协作端会直接列出这些模板。见 [docs/WORKFLOWS.md](docs/WORKFLOWS.md)。
- **跟着你的电脑走。** 首次启动跟随系统语言（中文/English），也可以在设置里固定一种；应用启动后和每 12 小时检查 GitHub 发布页，发现新版本会提示下载地址。

### 安装发布版

以下命令需要对应 GitHub Release 已发布。当前版本是**测试预发布版**：macOS Apple Silicon DMG 为临时签名，尚未经过 Apple 公证。应用内更新检查只提示正式版，不提示测试预发布版。

Windows x86_64（PowerShell 7，安装到当前用户）：

```powershell
irm https://raw.githubusercontent.com/neko233-com/MarkdownX/main/install.ps1 | iex
```

macOS arm64（Apple Silicon）：

```sh
curl -fsSL https://raw.githubusercontent.com/neko233-com/MarkdownX/main/install.sh | sh -s -- --allow-unsigned
```

安装脚本会校验安装包；Windows 会请求应用正常保存并退出，保留文档库及协作服务器数据。macOS 安装到 `~/Applications/MarkdownX.app`，请先关闭正在运行的旧版。未公证测试版首次启动时，macOS 可能阻止打开；确认信任后，可在「系统设置 → 隐私与安全性」中检查提示并允许打开。脚本不会修改 Gatekeeper 设置。离线安装时下载 MSI 或 DMG 与其 `.sha256`，运行 `pwsh -File ./install.ps1 -MsiPath <文件>` 或 `sh ./install.sh --package <文件> --allow-unsigned`；添加 `-DryRun` 或 `--dry-run` 可只检查。Windows Git Bash 可通过 `sh ./install.sh` 调用 PowerShell 7。

推送版本标签后会自动触发发布：Windows 和 macOS 两个平台构建与校验均通过后发布测试版，并验证公开安装入口。工作流不会创建或推送标签；如需手动重试，可在 **Actions → Release** 中选择已有标签并输入 `PUBLISH vX.Y.Z`。

## 快速开始

需要 Rust 1.99.1；Windows 还需要 Visual Studio 2022 的 C++ 生成工具和 Windows SDK。Python 3 只用于示例和本地静态服务器。

在当前 Windows 用户下安装构建好的版本（开始菜单/桌面快捷方式、`.mdxb` 文件关联）：

```powershell
pwsh -File scripts/install-windows.ps1
```

不需要管理员权限。优先使用当前构建的 MSI；应用装到 `%LOCALAPPDATA%\Programs\MarkdownX`，服务器令牌和工作区保留在 `%LOCALAPPDATA%\MarkdownX\server`，并在「应用和功能」中管理。安装脚本不会强制结束运行中的安装版应用或服务器。

release 构建后运行 `pwsh -File scripts/build-msi.ps1`，生成 `dist\MarkdownX-<version>-x64.msi`。MSI 会装好应用、可选协作服务器、Web UI、模板、快捷方式和 `.mdxb` 关联；协作服务仅在主动打开「MarkdownX Collaboration」时启动，不随登录启动。本机阅读器按需启动，仅提供应用内置的静态资源。构建时只需要 WiX Toolset 3.14。

```sh
cargo run --release
```

「我的文档」默认显示系统文档目录下的 `MarkdownX-Demo`；目录不存在时自动创建。使用「我的文档」旁的加号、设置或「文件 → 打开文件夹…」添加多个目录，各目录树同时显示并在重启后恢复。目录旁的「…」可更改显示名称或目录、刷新、移除工作区；移除只影响列表，不删除磁盘文件。不可用的目录会保留提示，不影响其他目录。启动参数指定的文件夹仅本次生效。

设置 › **扩展** 会列出内置格式和已安装的外部扩展。扩展是应用数据目录 `markdownx/plugins` 下带 `plugin.json` 的文件夹（可用 `MARKDOWNX_PLUGIN_DIR` 覆盖），程序必须位于该文件夹内。打开匹配后缀的文件时，应用启动扩展并索取 Markdown，以只读预览显示，不会改写原格式。「安装扩展…」会把选中的文件夹复制到该目录，可逐个关闭扩展；与内置格式或其他扩展冲突的清单会整体跳过。仓库中的 `MarkdownX-Demo` 目录包含 Markdown、Excel 示例和一个参考扩展。

图片（含动态 GIF）、音频、视频和 3D 模型可直接预览：图片可缩放，音频显示解码后的波形与时长，视频显示系统缩略图和元数据，OBJ/STL/glTF/GLB 可拖动旋转和缩放。音频和视频还可选择「使用系统应用打开」。它们和外部扩展共用同一套后缀注册表，插件不能静默占用这些后缀。

工作目录中的 `.xlsx`、`.xlsm` 和 `.xlam` 可在表格编辑器中打开，也可从工作目录新建 `.xlsx`。目前支持单元格编辑、可用公式的本地试算、撤销重做、区域复制填充、工作表管理，以及在当前工作表中查找，并分别替换文本、公式原文和数值；替换可选择区分大小写或匹配整个单元格。数值替换使用原值（例如 50% 输入 `0.5`），日期使用 `YYYY-MM-DD`。选区运算可输入数值或百分比后选择加、减、乘、除；文本、布尔值和日历日期会跳过。公式可选择参与：若公式和它引用的单元格同时位于选区，两者都会改变。表格可显示常见的单元格颜色、字体强调、对齐和边框，以及部分条件颜色和数字格式。过大的条件格式区域会提示并跳过；虚线和双线边框以近似实线显示。选区可设置整数、小数、千位分隔、百分比、科学计数及日期格式，并支持撤销重做。受保护工作表可编辑明确未锁定的单元格，锁定或状态不明确的单元格不能保存；隐藏公式不会显示在编辑栏或公式搜索中，可使用“复制值”复制其计算结果。工作表保护不会加密文件。表格支持仍在完善：部分结构暂不能安全编辑，使用 1904 日期系统的工作簿暂不提供本地公式计算。

独立的 MarkdownX 文档库仍放在 `~/Documents/MarkdownX`（可用 `MARKDOWNX_HOME` 改）；侧栏以工作目录为主，已有库文档可从主页和搜索访问，切换文件夹不会移动或覆盖它。

首次启动时，空的默认目录会生成与界面语言一致、可直接编辑的示例，并同时在编辑区和目录树中显示；文档库指南也使用相同语言。已有文件不会被覆盖，启动参数指定的文件或目录优先打开。切换界面语言不会翻译或覆盖已有文档；已保存工作区中最后打开的文件仍然可用时会在重启后恢复。

已有工作区的用户可在 **设置 → 文档 → 创建演示** 选择保存位置。应用会创建一个独立的 `MarkdownX-Demo` 文件夹；如果同名文件夹已存在，会使用新名称，不覆盖现有内容。

启动 MarkdownX 时传入文件夹路径会直接显示目录树，不会自动打开文件。目录树显示普通文件，并用适配深浅主题的图标区分文件夹、Markdown、文本、书籍、代码和图片等类型；其他类型可从文件菜单或全局搜索结果选择「使用系统应用打开」。文件名搜索覆盖可见文件，正文搜索只读取支持的文档。外部增删文件后可点「刷新」，窗口重新获得焦点时也会刷新。名称以点开头的项、符号链接、`target`、`node_modules` 和 `__pycache__` 不显示。保存时 Markdown 会重新导出，原有排版和未支持的语法可能变化；UTF-8 BOM 和主要的 CRLF 换行会保留。检测到文件已被其他程序修改、导出会丢失标题或正文、资源无法导出或同名资源内容不同时，未保存内容会保留，源文件不会被替换。

「文件 → 打开文件…」（Windows 为 `Ctrl+O`）或启动参数可直接编辑 Markdown 和文本文件，并显示所在目录树；侧栏目录名旁的加号可新建 Markdown 文件，目录和文件旁的「…」提供其他文件管理操作。文件保存与双链索引在后台进行，切换或关闭前会等待最新修改保存。若同一文件已有 Git 暂存内容，自动提交会跳过并提示，文件保存结果仍保留。

按 `Ctrl+K`（macOS 为 `⌘K`）可全局搜索文档库和全部工作目录的名称、路径与正文；用 ↑/↓ 和 Enter 打开结果，Esc 关闭搜索。原来的 `Ctrl+J` / `⌘J` 仍可使用；插入链接改为 `Ctrl+Shift+K` / `⌘⇧K`。

浏览器阅读：

```sh
python -m http.server 8765 --bind 127.0.0.1
# 打开 http://127.0.0.1:8765/web/
```

首页自带五个示例，也可以把任意 `.mdxb` 拖进页面。

SVG 片段可选择命名 `<view>` 或 `svgView(viewBox(...))`，例如 `![视图](./shots/frames.svg#closeup)` 或 `![视图](<./shots/frames.svg#svgView(viewBox(50,0,50,50))>)`。文件名中的 `#` 要写成 `%23`。桌面渲染与内置独立导出支持视口、`preserveAspectRatio`，以及桌面 SVG 引擎支持的选择器中的 `:target`；其他视图规格和未支持的 CSS 选择器仍有限制。浏览器阅读器使用浏览器自己的 SVG 实现。

### 格式与交互块

`.mdxb` 是 UTF-8 JSON，扩展名 MarkdownX Book，媒体类型 `application/vnd.markdownx.book+json`。每章是 `docxxml` 或 `markdown`，资源以 base64 内嵌，所以一本书真的只有一个文件。完整字段见 [docs/FORMAT.md](docs/FORMAT.md)。

`文件 → 打包成电子书…` 把 Markdown 章节目录打成一本 `.mdxb`；`下载为 → Markdown 文件夹 (.md)` 反向写出 `NN-title.md`、同名 `.xml` DocxXML 源、`book.json` 和资源。表格、标题、章节顺序、交互块的 `mdxb-canvas` / `mdxb-html` 源码会保留；高亮块颜色、分栏等纯展示细节可能退化成普通 Markdown，但 XML 源保留完整结构，重新打包即可回到书中；如果改过 `.md`，较新的 Markdown 会覆盖较旧的 XML。

DocxXML 里的交互块：

```xml
<interactive type="canvas" title="拖动参数" height="360"><![CDATA[
  {"kind":"quadratic","a":1,"b":0,"c":0}
]]></interactive>
```

Markdown 里则是 `mdxb-canvas` / `mdxb-html` 围栏代码块，正文是一层 JSON 信封。

### 协作服务器

`server/` 是给团队共用一个仓库的可选 Rust 服务器：`-root` 指向 `.git` 所在目录，文档 ID 是仓库相对路径（支持中文文件名和嵌套目录），每次接受的保存都会成为一次 Git 提交，浏览器编辑器里能看见谁在线，并带历史/diff/恢复。

```sh
MARKDOWNX_SERVER_TOKEN=change-me cargo run -p markdownx-server -- \
  -root ./data -web server/web
# 打开 http://127.0.0.1:8877/
```

它支持在线光标/在线成员、WebSocket 保存、三方合并与冲突提示（载入最新、采用合并稿、覆盖远端），版本面板可以看 Git 历史、diff 并把任意版本恢复成一次新提交。附多阶段 `Dockerfile`。服务器是单租户、一个共享令牌，请放在 TLS 反向代理之后；参数、HTTP/WebSocket 协议和部署说明见 [server/README.md](server/README.md)。

### 目录

```
crates/core/        引擎：模型、DocxXML、Markdown、编辑内核、.mdxb、文档库
src/                GPUI 桌面端：编辑、阅读、导入导出、设置
web/                无构建的浏览器阅读器
server/             Rust 协作服务器与浏览器编辑器（Git 历史、diff）
templates/          游戏、产品、影视与内容工作流模板
examples/           五本示例书与生成脚本
agent/              给编码 agent 的写书规则
tools/agent/        check、pack、unpack、search、merge-annotations CLI
shared-mock/        内置指南与源文件
docs/               架构、格式、主题、性能
```

### 构建与测试

```sh
scripts/check.sh
cargo test -p fdl_core
```

协作服务器是 workspace 成员：

```sh
cargo test -p markdownx-server
cargo clippy -p markdownx-server --all-targets -- -D warnings
```

### 状态

这是早期公开快照。现在可用：桌面编辑与文档库、文件夹工作区、Markdown/DocxXML 互转、`.mdxb` 导入导出、Markdown 文件夹导出、批注 sidecar、`[[双链]]`、响应式 Web 阅读器、Canvas/HTML 交互块，以及带 Git 历史和 diff 的 Rust 协作服务器。桌面端原生运行 `quadratic`、`unitcircle` 和 `projectile` Canvas 实验；HTML 在 Windows 桌面和 Web 阅读器里沙箱运行。Windows 在硬件 D3D11 不可用时会自动回退 WARP，VM/RDP 里也能打开桌面 UI，并已用窗口截图冒烟验证。Windows MSI 会注册 `.mdxb`，协作服务按需启动；macOS Info.plist 和 Linux MIME 包也会注册 `.mdxb`，但双击行为仍待真机验证，移动端壳是下一步。

### 许可

MIT。MarkdownX 源自 s1dashu 的 [Quiet.md](https://github.com/s1dashu/quiet-md)，保留原版权声明；第三方组件、字体与图标见 [THIRD_PARTY.md](THIRD_PARTY.md)。内置界面图标为本项目绘制，随 MIT 发布，不包含 Nucleo 图标。
