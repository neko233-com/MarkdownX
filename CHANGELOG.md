# Changelog

## 0.1.47 - 2026-11-24

**Go 重写，MyGo 原生 UI，正式安装包。**

### 桌面应用

- 用 Go + MyGo 重写，纯 Go、`CGO_ENABLED=0`，没有 webview、Node 或 C 工具链。
- 块编辑器：段落、标题、列表、待办、引用、代码块、表格、分割线、公式、图片、
  链接、行内代码、粗体/斜体/下划线/删除线、查找替换、撤销重做。
- Markdown 往返无损：硬换行、列表续行、有序列表起始编号、表格对齐、嵌套列表、
  公式分隔符和本地资源路径都有测试。
- 工作区侧栏、最近文件、内置/用户模板、自动保存、未保存关闭确认。
- 导入 `.docx`、`.xlsx`、`.epub`、`.ofd`、`.pdf`、`.rtf`、`.html`；导出 Markdown、
  单文件 HTML 和 `.mdxb` 书籍。

### 安装与发布

- 使用 MyGo 的正式分发流程：Windows per-user NSIS 安装器（图标、文件关联、
  卸载项、无控制台窗口），macOS `.app` + `.dmg`（ad-hoc 签名）。
- 命令行工具单独发布：`markdownx-server`、`mdx-convert`、`mdx-enc`。
- Release 附带每个文件的 SHA-256 和合并后的 `SHA256SUMS`。
- CI 在 Windows 与 macOS 上分别验证 MyGo 构建。

### 开发

- 接入 codegraph233 MCP：`.codegraph233.json`、`opencode.json`、
  `docs/CODEGRAPH233.md`，以及 AGENTS/CLAUDE 导航约定。
- `mygo.json` 成为版本和打包元数据的唯一来源；标签必须与它一致。
- 重写架构、格式、安装、发布、性能和工作流文档；删除 Rust 时代的主题与
  UI 更新文档。
- `go.mod` 把 MyGo CLI 固定为 tool 依赖，`go tool mygo build` 可在任意平台复现。

### 升级说明

0.1.46 及更早版本把文档存在 `~/Documents/MarkdownX` 的私有文档库里，并用 MSI
安装到 `%LOCALAPPDATA%\Programs\MarkdownX`。0.1.47 直接打开 Markdown 文件和
文件夹，不读取旧库，也不会修改旧库。

升级时请先在“设置 → 应用”里卸载旧的 MSI，再安装 0.1.47，避免两套安装器在同一
目录里留下文件。旧库中的文件仍在原处，不会被删除；需要保留的内容请先用旧版导出。

### 已知限制

- Windows 安装器未做 Authenticode 签名；macOS 构建是 ad-hoc 签名、未公证。
  首次运行时系统可能要求确认。
- 尚未接入 MyGo 的签名自动更新；当前版本通过 GitHub Releases 手动升级。
