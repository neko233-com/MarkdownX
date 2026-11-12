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
- **Markdown 与飞书 DocxXML。** 工作目录可直接编辑常用 Markdown 和 `.docxxml`，也可以导入已授权的飞书文档。Typora 格式兼容仍在完善。
- **交互块。** 二次函数、单位圆、抛体 Canvas 可在桌面和 Web 阅读器运行；HTML/JS 在阅读器的沙箱中运行，外部资源和脚本网络连接受到限制。
- **原生中英双语。** 界面、书名和章节正文都可以同时带中文和英文。
- **为屏幕而生的阅读器。** 手机单列，桌面目录 + 正文；无构建步骤，Chrome 直接打开。
- **本地优先。** 本地编辑无需登录；飞书导入使用可选的 CLI 授权，工作目录保存在自己的电脑上。
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

## 安装发布版

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

```sh
cargo run --release
```

Windows 请在 Visual Studio 开发者命令行里构建，或先调用 `vcvars64.bat`。

在当前 Windows 用户下安装构建好的版本（开始菜单/桌面快捷方式、`.mdxb` 文件关联、登录时启动协作服务器）：

```powershell
pwsh -File scripts/install-windows.ps1
```

不需要管理员权限。优先使用当前构建的 MSI；应用装到 `%LOCALAPPDATA%\Programs\MarkdownX`，服务器令牌和工作区保留在 `%LOCALAPPDATA%\MarkdownX\server`，并在「应用和功能」中管理。安装脚本不会强制结束运行中的安装版应用或服务器。

release 构建后运行 `pwsh -File scripts/build-msi.ps1`，生成 `dist\MarkdownX-<version>-x64.msi`。MSI 会装好应用、协作服务器、Web UI、模板、快捷方式、`.mdxb` 关联和登录启动项；构建时只需要 WiX Toolset 3.14。

「我的文档」默认显示系统文档目录下的 `MarkdownX-Demo`；Windows 通常为 `C:\Users\<用户名>\Documents\MarkdownX-Demo`。目录不存在时自动创建。使用「我的文档」旁的加号、设置或 **文件 → 打开文件夹…** 添加多个目录，各目录树同时显示并在重启后恢复。目录旁的「…」可更改显示名称或目录、刷新、移除工作区；移除只影响列表，不删除磁盘文件。不可用的目录会保留提示，不影响其他目录。启动参数指定的文件夹仅本次生效。

首次启动时，空的默认目录会生成与界面语言一致、可直接编辑的示例，并同时在编辑区和目录树中显示；文档库指南也使用相同语言。已有文件不会被覆盖，启动参数指定的文件或目录优先打开。切换界面语言不会翻译或覆盖已有文档；已保存工作区中最后打开的文件仍然可用时会在重启后恢复。

图片（含动态 GIF）、音频、视频和 3D 模型可直接预览：图片可缩放，音频显示解码后的波形与时长，视频显示系统缩略图和元数据，OBJ/STL/glTF/GLB 可拖动旋转和缩放。音频和视频还可选择「使用系统应用打开」。它们和外部扩展共用同一套后缀注册表，插件不能静默占用这些后缀。

设置 › **扩展** 会列出内置格式和已安装的外部扩展。扩展是应用数据目录 `markdownx/plugins` 下带 `plugin.json` 的文件夹（可用 `MARKDOWNX_PLUGIN_DIR` 覆盖），程序必须位于该文件夹内。打开匹配后缀的文件时，应用启动扩展并索取 Markdown，以只读预览显示，不会改写原格式。「安装扩展…」会把选中的文件夹复制到该目录，可逐个关闭扩展；与内置格式或其他扩展冲突的清单会整体跳过。仓库中的 `MarkdownX-Demo` 目录包含 Markdown、Excel 示例和一个参考扩展。

目录树显示普通文件，并用适配深浅主题的图标区分文件夹、Markdown、文本、书籍、代码和图片等类型；其他类型可从文件菜单或全局搜索结果选择「使用系统应用打开」。文件名搜索覆盖可见文件，正文搜索只读取支持的文档；隐藏项、符号链接及构建依赖目录不显示。

独立的 MarkdownX 文档库仍放在 `~/Documents/MarkdownX`（可用 `MARKDOWNX_HOME` 改）；侧栏以工作目录为主，已有库文档可从主页和搜索访问，切换目录不会移动或覆盖它。用 **文件 → 打包成电子书…**（`⌘⇧E`）把当前目录或 Markdown 文件打成 `.mdxb`；导出文档库页面及其子页仍用「下载为 → MarkdownX 书 (.mdxb)」。

「文件 → 打开文件…」（Windows 为 `Ctrl+O`）或启动参数可直接编辑 Markdown 和文本文件，并显示所在目录树；侧栏目录名旁的加号可新建 Markdown 文件；目录或文件旁的「…」可新建文件夹、重命名，或在确认后移入系统回收站。文件操作不会覆盖同名文件。文件保存与双链索引在后台进行，切换或关闭前会等待最新修改保存。若同一文件已有 Git 暂存内容，自动提交会跳过并提示，文件保存结果仍保留。

按 `Ctrl+K`（macOS 为 `⌘K`）可全局搜索文档库和全部工作目录的名称、路径与正文；用 ↑/↓ 和 Enter 打开结果，Esc 关闭搜索。原来的 `Ctrl+J` / `⌘J` 仍可使用；插入链接改为 `Ctrl+Shift+K` / `⌘⇧K`。

浏览器阅读：直接用托管版 **https://neko233-com.github.io/MarkdownX/web/**，或在本地运行：

```sh
python -m http.server 8765 --bind 127.0.0.1
# 打开 http://127.0.0.1:8765/web/
```

同一份阅读器也可以用 `vercel.json` 部署到 Vercel：`npx vercel deploy` 只把 `web/` 与 `examples/` 构建进 `.vercel-out`。不登录也可以先做一次可认领的临时部署：`npx vercel deploy --temporary`。

首页自带五个示例，其中包括两本中国初中教材：**力与运动**（物理）和 **物质与变化**（化学）。也可以把任意 `.mdxb` 拖进页面。

## 导入飞书文档

在主页或文件菜单选择「从飞书导入」，粘贴文档或知识库链接。请先安装[飞书官方 CLI](https://github.com/larksuite/cli)，运行 `lark-cli config init` 和 `lark-cli auth login`，并授予目标文档的读取权限。MarkdownX 使用该授权将 DocxXML 正文保存到当前工作目录，不覆盖已有文件；完整响应保存在旁边的隐藏来源文件中。

目前支持正文和已实现的 DocxXML 块，图片、附件、评论和嵌入应用不会自动下载或转换，原始引用保留在来源文件中。也可以离线打开飞书导出的 Markdown。`.docxxml` 是 XML 文本，不是 Word `.docx` 文件。

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

每章是 `docxxml` 或 `markdown`；`content_en` 可选。英文与主正文格式不同时使用 v2 的 `format_en`，旧版阅读器会拒绝 v2，避免误读。导入书的翻译和资源按页面 ID 保留，重启或章节重排后仍可导出；编辑一种语言不会自动翻译另一种。完整字段见 [docs/FORMAT.md](docs/FORMAT.md)。

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

这是早期公开快照。现在可用：桌面编辑与文档库、Typora 式文件夹工作区与目录树、模糊/内容搜索、直接编辑 `.md`、Markdown/DocxXML 互转、`.mdxb` 导入导出与打包、Markdown 文件夹导出、响应式 Web 阅读器、Canvas/HTML 交互块、`[[双链]]`、批注 sidecar（批量解决/删除/合并），以及带 Git 历史/diff/恢复的 Rust 协作服务器。桌面端原生运行 `quadratic`、`unitcircle` 和 `projectile` Canvas 实验；HTML 仍在阅读器里沙箱运行。Windows 在硬件 D3D11 不可用时会自动回退 WARP，VM/RDP 里也能打开桌面 UI，并已用窗口截图冒烟验证。Windows MSI 会注册 `.mdxb` 并在登录时启动后台 server；macOS/Linux 文件关联和移动端壳是下一步。见 [issues](https://github.com/neko233-com/MarkdownX/issues)。

## 许可

MIT。MarkdownX 源自 s1dashu 的 [Quiet.md](https://github.com/s1dashu/quiet-md)，保留原版权声明；第三方组件、字体与图标见 [THIRD_PARTY.md](THIRD_PARTY.md)。内置界面图标为本项目绘制，随 MIT 发布，不包含 Nucleo 图标。
