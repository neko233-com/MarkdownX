<h1 align="center">MarkdownX</h1>

<p align="center">
  <b>Interactive books, not static pages.</b><br>
  A desktop authoring app and a browser reader for the next generation of learning material.
</p>

<p align="center">
  <a href="https://github.com/neko233-com/MarkdownX/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/neko233-com/MarkdownX/ci.yml?branch=main&label=CI" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT"></a>
  <a href="#简体中文">简体中文</a>
</p>

![MarkdownX reader](docs/images/reader.png)

## What it is

MarkdownX is a writing and reading environment for **dynamic books**: documents that can contain a draggable parabola, a pendulum you can push, a quiz, a table you can sort, or a whole HTML experiment.

- **Write once, read anywhere.** A book is one `.mdxb` file. Text, images and interactive blocks travel together — no folder of loose assets, no server.
- **Markdown and Feishu/Lark DocxXML.** Chapters can be either format. Existing Markdown imports cleanly; DocxXML keeps its block structure, tables, callouts and colors.
- **Interactive blocks.** `canvas` blocks are declarative JSON (quadratic, unit circle, projectile in the web reader; quadratic natively on the desktop). `html` blocks are sandboxed HTML/JS for everything else.
- **Bilingual by design.** Interface, book metadata and chapter text can carry Chinese and English side by side.
- **A reader that fits the screen.** The web reader is a build-free, responsive page: a single column on a phone, a table of contents and a comfortable measure on a desktop.
- **Local first.** No account, no cloud, no telemetry. The desktop library is a folder you own.

## Two ways to use it

| Author | Read |
|---|---|
| The GPUI desktop app: write chapters, insert Canvas/HTML blocks, export a `.mdxb`. | The Chrome reader in `web/`: open a `.mdxb`, browse chapters, resize the window, switch language and theme. |

| Landing | Canvas chapter |
|---|---|
| ![Landing](docs/images/landing.png) | ![Reader](docs/images/reader.png) |

## Quick start

**Requirements:** Rust 1.99.1, and on Windows the Visual Studio 2022 C++ build tools plus a Windows SDK. Python 3 is only needed for the examples and a local web server.

```sh
cargo run --release
```

On Windows, run the build from a Visual Studio developer prompt, or call `vcvars64.bat` first.

The desktop app creates its library at `~/Documents/MarkdownX` (override with `MARKDOWNX_HOME`). Use **下载为 → MarkdownX 书 (.mdxb)** / **Download as → MarkdownX book (.mdxb)** to export the current page and its sub-pages. On Windows the installer registers `.mdxb`, so double-clicking a book opens and imports it.

To read a book in the browser:

```sh
python -m http.server 8765 --bind 127.0.0.1
# open http://127.0.0.1:8765/web/
```

The landing page ships with three examples. You can also drag any `.mdxb` onto the page.

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

Every chapter is either `docxxml` or `markdown`; `content_en` is optional and lets the reader switch languages without duplicating the book. Assets are embedded as base64, so a book is genuinely one file.

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
examples/           three .mdxb books and the Python generator
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

## Status

This is an early public snapshot. Working today: the desktop editor and library, Markdown/DocxXML interchange, `.mdxb` import/export, the responsive web reader, and Canvas/HTML blocks. The desktop app runs the built-in `quadratic` Canvas explorer natively; HTML stays sandboxed in the reader. macOS/Linux file association and mobile shells are the next milestones. See [issues](https://github.com/neko233-com/MarkdownX/issues).

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
- **交互块。** `canvas` 用声明式 JSON（Web 阅读器有二次函数、单位圆、抛体；桌面原生二次函数）；`html` 用沙箱 iframe 放任意 HTML/JS。
- **原生中英双语。** 界面、书名和章节正文都可以同时带中文和英文。
- **为屏幕而生的阅读器。** 手机单列，桌面目录 + 正文；无构建步骤，Chrome 直接打开。
- **本地优先。** 不登录、不上传、不统计；桌面文档库就是你自己的文件夹。

### 快速开始

需要 Rust 1.99.1；Windows 还需要 Visual Studio 2022 的 C++ 生成工具和 Windows SDK。Python 3 只用于示例和本地静态服务器。

```sh
cargo run --release
```

桌面端默认把文档库放在 `~/Documents/MarkdownX`，可用 `MARKDOWNX_HOME` 改。用「下载为 → MarkdownX 书 (.mdxb)」导出当前页及其子页。

浏览器阅读：

```sh
python -m http.server 8765 --bind 127.0.0.1
# 打开 http://127.0.0.1:8765/web/
```

首页自带三个示例，也可以把任意 `.mdxb` 拖进页面。

### 格式与交互块

`.mdxb` 是 UTF-8 JSON，扩展名 MarkdownX Book，媒体类型 `application/vnd.markdownx.book+json`。每章是 `docxxml` 或 `markdown`，资源以 base64 内嵌，所以一本书真的只有一个文件。完整字段见 [docs/FORMAT.md](docs/FORMAT.md)。

DocxXML 里的交互块：

```xml
<interactive type="canvas" title="拖动参数" height="360"><![CDATA[
  {"kind":"quadratic","a":1,"b":0,"c":0}
]]></interactive>
```

Markdown 里则是 `mdxb-canvas` / `mdxb-html` 围栏代码块，正文是一层 JSON 信封。

### 目录

```
crates/core/        引擎：模型、DocxXML、Markdown、编辑内核、.mdxb、文档库
src/                GPUI 桌面端：编辑、阅读、导入导出、设置
web/                无构建的浏览器阅读器
examples/           三本示例书与生成脚本
shared-mock/        内置指南与源文件
docs/               架构、格式、主题、性能
```

### 构建与测试

```sh
scripts/check.sh
cargo test -p fdl_core
```

### 状态

这是早期公开快照。现在可用：桌面编辑与文档库、Markdown/DocxXML 互转、`.mdxb` 导入导出、响应式 Web 阅读器、Canvas/HTML 交互块。桌面端原生运行内置的 `quadratic` Canvas 实验；HTML 仍在阅读器里沙箱运行。macOS/Linux 文件关联和移动端壳是下一步。Windows 安装器会注册 `.mdxb`。

### 许可

MIT。MarkdownX 源自 s1dashu 的 [Quiet.md](https://github.com/s1dashu/quiet-md)，保留原版权声明；第三方组件、字体与图标见 [THIRD_PARTY.md](THIRD_PARTY.md)。内置界面图标为本项目绘制，随 MIT 发布，不包含 Nucleo 图标。
