# 架构

MarkdownX 是一个单进程的 Go 桌面应用，界面由
[MyGo](https://github.com/egoist/mygo) 的原生 UI 绘制：没有 webview，没有 C 依赖，
两个目标平台（Windows x64、macOS arm64）都由同一条 `CGO_ENABLED=0` 构建链产出。

## 入口与包

| 路径 | 职责 |
|---|---|
| `cmd/markdownx` | 桌面应用入口；接收一个可选的 Markdown 路径 |
| `cmd/markdownx-convert` | 把 docx / xlsx / epub / ofd / pdf / rtf / html 转成 Markdown |
| `cmd/markdownx-server` | 协作服务器：HTTP API、Git 提交、SSE、网页编辑器 |
| `cmd/mdx-enc` | `encrypt` / `decrypt` / `genkey` 加密工具 |
| `internal/doc` | 文档模型、Markdown 读取与导出；格式的唯一真相来源 |
| `internal/editor` | 块编辑器内核：光标、选区、拆分/合并、缩进、undo |
| `internal/diagram` | Markdown 图表围栏的适配与纯 Go SVG 渲染 |
| `internal/convert` | 各格式的纯 Go 读取器 |
| `internal/export` | 单文件、自包含 HTML 导出 |
| `internal/export/pdf` | 纯 Go 纸张排版、字体子集和 PDF 导出 |
| `internal/book` | `.mdxb` 书籍信封、章节与资源 |
| `internal/server` | 协作服务器实现，`web/` 用 `go:embed` 内嵌 |
| `internal/app` | MyGo 窗口、菜单、侧栏、模板、预览、设置 |
| `internal/rage` | scrypt + NaCl secretbox 的密码 / 服务端密钥加密 |

依赖方向只有一条：`cmd` → `internal/app` / `internal/server`；`internal/app` 可以
依赖 `doc`、`editor`、`book`、`convert`、`export`、`rage`；`doc` 不依赖 UI。

## 文档模型与 Markdown

`internal/doc` 把一篇文档表示成块（`Block`）的有序列表。每个块有稳定的
`BlockID`、`BlockKind` 和 `RichText`；`RichText` 是文本 + 样式 run，样式包括粗体、
斜体、下划线、删除线、行内代码、链接、文字/背景色，以及占一个 `U+FFFC` 的行内对象
（公式、图片、按钮、提及等）。

Markdown 是磁盘上的唯一格式。读取时连续普通行合并成一个段落块；写出时再由块还原成
CommonMark/GFM。硬换行、列表续行、表格对齐、有序列表起始编号、嵌套列表、公式分隔符、
转义和本地资源路径都有往返测试（`internal/doc` 与 `internal/editor`）。
工作区解析会记录顶层块在原始 Markdown 中的范围。写回时重用未编辑块的原文，编辑过的
块由文档模型序列化。源码视图可以直接保存文本，切回富文本时以一次可撤销操作应用。
保存前检查磁盘内容是否仍与上次读取或写入一致，并在同一目录通过临时文件替换目标。

书籍 `.mdxb` 是另一种容器：一个 JSON 信封，章节内容可以是 DocxXML 或 Markdown，
资源以 base64 内嵌，支持中英双语章节与阅读主题。版本常量在 `internal/book`。

## 编辑流程

1. 视图收到输入，调用 `Core.Checkpoint(kind)` 记录 undo 快照。
2. 调用 Core 的结构化方法修改文档或选区（拆分、合并、缩进、表格、行内对象等）。
3. `App.ResetBuffers()` 将变更同步到 MyGo 富文本缓冲，保留未改动块的编辑状态。
4. 有改动时标记 dirty，保存时由 `internal/doc` 导出 Markdown 写盘。

结构性修改经过 Core。源码使用 MyGo 的原生代码编辑器。富文本控件负责段内输入和光标移动。
跨块选区由 Core 记录，视图投影到各段绘制，高亮、复制、剪切和替换读取同一选区。
行内源码只在当前光标所在的范围展开。视图在源码和富文本之间换算选区，Core 保存解析后的内容。
`RichText` 记录已解析行内标记的原文，块编辑后仍可保留相邻格式的原有写法。
参考式链接和脚注保留标签，按文档当前定义解析目标。目录和标题跳转共用同一组标题锚点，
HTML 导出在文档末尾生成脚注和回链。
结构化粘贴用 Core 插入块，作为一次撤销操作。Core 的不变量由
`internal/editor` 的测试覆盖：块 id 唯一、run 不越界/不重叠、代码块里没有样式和
行内对象、存盘再读回一致、undo 到底再重做到头回到同一状态。

`CodeKind` 保存围栏的源码和原有标记。正文与 `ui/markdown` 预览共用图表渲染缓存，
由 MyGo 原生绘制 SVG。HTML 导出嵌入同一渲染器的矢量图，解析失败时写出原代码。

PDF 导出在后台排版当前文档的快照，复用文档标题和引用索引。文字以字体子集嵌入 PDF，
本地图片、公式和图表使用静态图像。文件生成完毕后再写入用户选择的路径。

## 应用层

`internal/app` 保存当前文件、工作区根、目录展开状态、最近文件、未保存标记、
查找状态和编辑器 Core。多个窗口各自持有文档与撤销记录。原生菜单和编辑器共用一套按平台
解析的命令，菜单按所属窗口派发。公式由 termtex 生成 SVG，并通过 MyGo 在正文和原生预览中绘制。

设置写到 `os.UserConfigDir()/MarkdownX/settings.json`；用户模板写到同一目录下的
`templates/`。用户文档本身永远是普通 Markdown 文件，不进入应用私有数据库。

## 导入与导出

`internal/convert` 的每个读取器只依赖标准库和纯 Go 包：

- docx / xlsx / epub / ofd 按 ZIP + XML 读取；
- pdf 做有界的文本提取，不执行脚本；
- rtf 解析控制字；
- html 用宽松的标签/实体解析。

`internal/export` 把文档渲染成一个自包含 HTML 文件，本地图片等资源以 data URL
内嵌，离线可打开。

## 协作服务器

`internal/server` 提供：

- `GET /api/docs`、`GET /api/doc?path=`、`PUT /api/doc`、`GET /api/events`；
- 历史、diff、restore 接口；
- 每次接受保存后以当前 Git 分支提交；
- 可选的 token 鉴权和磁盘端到端加密（`internal/rage` 的服务端密钥信封）。

网页编辑器在 `internal/server/web/`，通过 `go:embed` 编进二进制，不需要单独部署
静态文件。

## 构建与发布

`mygo.json` 是应用元数据的唯一来源：名称、版本、图标、文件关联、macOS 最低版本、
Windows 安装目录。`go tool mygo build` 负责：

- 编译桌面应用（链接名称、标识符、版本，Windows 下无控制台窗口）；
- macOS：`.app`、ad-hoc 签名、`.dmg`；
- Windows：per-user NSIS 安装器、图标、文件关联、卸载项。

命令行工具用普通 `go build` 产出，和桌面应用一起进入 GitHub Release。
版本号只改 `mygo.json`；发布标签必须与它一致。
