<h1 align="center">MarkdownX</h1>

<p align="center">
  <b>书不该只是读到这里。</b><br>
  给下一代学习者的可交互书本：桌面创作端 + 浏览器阅读端。
</p>

<p align="center">
  <a href="https://github.com/neko233-com/MarkdownX/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/neko233-com/MarkdownX/ci.yml?branch=main&label=CI" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT"></a>
  <a href="README.md">English</a>
</p>

![MarkdownX 阅读器](docs/images/reader.png)

## 它是什么

MarkdownX 用来写和读**动态书**：一本可以拖动的抛物线、可以推一把的单摆、可以点的测验、可以排序的表格，或者一整段 HTML 小实验。

- **写一次，到处能读。** 一本书就是一个 `.mdxb` 文件，文字、图片和交互块都在里面，不需要服务器，也不怕资源散落。
- **Markdown 与飞书 DocxXML。** 章节可以是任意一种；Markdown 导入即用，DocxXML 保留标题、表格、高亮块和颜色。
- **交互块。** `canvas` 用声明式 JSON（Web 阅读器有二次函数、单位圆、抛体；桌面原生二次函数）；`html` 用沙箱 iframe 放任意 HTML/JS。
- **原生中英双语。** 界面、书名和章节正文都可以同时带中文和英文。
- **为屏幕而生的阅读器。** 手机单列，桌面目录 + 正文；无构建步骤，Chrome 直接打开。
- **本地优先。** 不登录、不上传、不统计；桌面文档库就是你自己的文件夹。
- **文件夹就是工作区。** 打开任意目录，直接编辑里面的 `.md`；同一个目录树也能放 `.mdxb`。侧栏是目录树，支持模糊路径搜索和可开关的内容搜索。
- **批注在正文旁边。** 临时批注保存为 `.annotations.json` sidecar，可以隐藏、解决、批量操作和合并，不必改动正文。
- **跟着你的电脑走。** 首次启动跟随系统语言（中文/English），也可以在设置里固定一种；应用启动后和每 12 小时检查 GitHub 发布页，发现新版本会提示下载地址。

## 两种用法

| 创作 | 阅读 |
|---|---|
| GPUI 桌面端：打开文件夹、编辑 `.md`、插入 Canvas/HTML、写批注，再打包 `.mdxb`。 | `web/` 里的 Chrome 阅读器：打开书、翻章、缩放窗口、切换语言和主题。 |

| 首页 | 可交互章节 |
|---|---|
| ![首页](docs/images/landing.png) | ![阅读器](docs/images/reader.png) |

## 快速开始

需要 Rust 1.99.1；Windows 还需要 Visual Studio 2022 的 C++ 生成工具和 Windows SDK。Python 3 只用于示例和本地静态服务器。

```sh
cargo run --release
```

Windows 请在 Visual Studio 开发者命令行里构建，或先调用 `vcvars64.bat`。

在当前 Windows 用户下安装构建好的版本（开始菜单/桌面快捷方式、`.mdxb` 文件关联、登录时启动协作服务器）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-windows.ps1
```

不需要管理员权限。应用装到 `%LOCALAPPDATA%\Programs\MarkdownX`，服务器令牌和 Git 工作区在 `%LOCALAPPDATA%\MarkdownX\server`；卸载脚本随应用一起安装并登记到「应用和功能」。重新安装会保留已有服务器令牌。

也可以构建 per-user MSI：release 构建后运行 `pwsh -File scripts/build-msi.ps1`，安装 `dist\MarkdownX-0.1.8-x64.msi`。MSI 会装好应用、协作服务器、Web UI、模板、快捷方式、`.mdxb` 关联和登录启动项；构建时只需要 WiX Toolset 3.14。

桌面端默认把文档库放在 `~/Documents/MarkdownX`，可用 `MARKDOWNX_HOME` 改。用 **文件 → 打开文件夹…** 进入 Typora 式目录工作区；用 **文件 → 打包成电子书…**（`⌘⇧E`）把当前目录或当前 Markdown 文件打成一本 `.mdxb`。导出文档库页面及其子页仍用「下载为 → MarkdownX 书 (.mdxb)」。

浏览器阅读：直接用托管版 **https://neko233-com.github.io/MarkdownX/web/**，或在本地运行：

```sh
python -m http.server 8765 --bind 127.0.0.1
# 打开 http://127.0.0.1:8765/web/
```

同一份阅读器也可以用 `vercel.json` 部署到 Vercel：`npx vercel deploy` 只把 `web/` 与 `examples/` 构建进 `.vercel-out`。不登录也可以先做一次可认领的临时部署：`npx vercel deploy --temporary`。

首页自带五个示例，其中包括两本中国初中教材：**力与运动**（物理）和 **物质与变化**（化学）。也可以把任意 `.mdxb` 拖进页面。

## 协作服务器

`server/` 是给团队共用一个工作区的可选 Rust 服务器：文档仍是普通 `.md` 文件，每次接受的保存都会成为一次 Git 提交，浏览器编辑器里能看见谁在线。

```sh
MARKDOWNX_SERVER_TOKEN=change-me cargo run -p markdownx-server -- \
  -root ./data -web server/web
# 打开 http://127.0.0.1:8877/
```

它支持在线光标/在线成员、WebSocket 保存、三方合并与冲突提示（载入最新、采用合并稿、覆盖远端），版本面板可以看 Git 历史、diff 并把任意版本恢复成一次新提交。附多阶段 `Dockerfile`。服务器是单租户、一个共享令牌，请放在 TLS 反向代理之后；参数、HTTP/WebSocket 协议和部署说明见 [server/README.md](server/README.md)。

## 给 agent

一本书可以维护成 `NN-topic.md` 章节文件夹。编码 agent 不用打开 GUI 就能校验和打包：

```sh
python tools/agent/mdxb.py check examples
python tools/agent/mdxb.py pack books/physics --title "力与运动" -o physics.mdxb
python tools/agent/mdxb.py unpack physics.mdxb books/physics
python tools/agent/mdxb.py search books/physics 速度 --content
```

章节语法、批注 sidecar 契约和审阅循环见 [agent/README.md](agent/README.md)；给 agent 的规则见 [agent/AGENTS.md](agent/AGENTS.md)。

## `.mdxb` 格式

`.mdxb` 是 UTF-8 JSON，扩展名 MarkdownX Book，媒体类型 `application/vnd.markdownx.book+json`。

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

每章是 `docxxml` 或 `markdown`；`content_en` 可选，让阅读器在不复制整本书的情况下切换语言。资源以 base64 内嵌，所以一本书真的只有一个文件。完整字段见 [docs/FORMAT.md](docs/FORMAT.md)。

### DocxXML 里的交互块

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

Markdown 里则是 `mdxb-canvas` / `mdxb-html` 围栏代码块，正文是一层 JSON 信封：

````markdown
```mdxb-canvas
{"title":"拖动参数","height":360,"source":"{\"kind\":\"quadratic\",\"a\":1}"}
```
````

## 目录

```
crates/core/        引擎：模型、DocxXML、Markdown、编辑内核、.mdxb、文档库
src/                GPUI 桌面端：编辑、阅读、导入导出、设置
web/                无构建的浏览器阅读器（HTML/CSS/ES modules）
examples/           五本示例书与生成脚本
agent/              给编码 agent 的写书规则
tools/agent/        check、pack、unpack、search、merge-annotations CLI
shared-mock/        内置指南与源文件
docs/               架构、格式、主题、性能
```

文档引擎不依赖 GPUI，所以可以单独测试，也可以给别的前端复用。见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 构建与测试

```sh
scripts/check.sh                         # release 测试 + clippy
cargo test -p fdl_core                   # 只测引擎
FDL_FUZZ_SEEDS=2000 cargo test --release random_edits
```

Web 阅读器没有构建步骤。改完 `web/js/` 后对每个文件跑 `node --check`，再用本地服务器打开页面。

## 状态

这是早期公开快照。现在可用：桌面编辑与文档库、Typora 式文件夹工作区与目录树、模糊/内容搜索、直接编辑 `.md`、Markdown/DocxXML 互转、`.mdxb` 导入导出与打包、Markdown 文件夹导出、响应式 Web 阅读器、Canvas/HTML 交互块、`[[双链]]`、批注 sidecar（批量解决/删除/合并），以及带 Git 历史/diff/恢复的 Rust 协作服务器。桌面端原生运行内置的 `quadratic` Canvas 实验；HTML 仍在阅读器里沙箱运行。Windows 在硬件 D3D11 不可用时会自动回退 WARP，VM/RDP 里也能打开桌面 UI，并已用窗口截图冒烟验证。Windows MSI 会注册 `.mdxb` 并在登录时启动后台 server；macOS/Linux 文件关联和移动端壳是下一步。见 [issues](https://github.com/neko233-com/MarkdownX/issues)。

## 许可

MIT。MarkdownX 源自 s1dashu 的 [Quiet.md](https://github.com/s1dashu/quiet-md)，保留原版权声明；第三方组件、字体与图标见 [THIRD_PARTY.md](THIRD_PARTY.md)。内置界面图标为本项目绘制，随 MIT 发布，不包含 Nucleo 图标。
