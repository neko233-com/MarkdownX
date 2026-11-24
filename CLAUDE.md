# 开发约定（给 AI 助手和人）

MarkdownX 现在是一个 **Go + MyGo（github.com/egoist/mygo）** 的桌面应用，目标平台只有
Windows x64 与 macOS arm64。Rust 实现已经完全移除。

## 代码结构

| 路径 | 作用 |
|---|---|
| `cmd/markdownx` | 桌面应用入口 |
| `cmd/markdownx-convert` | 命令行转换器：任意支持格式 → Markdown |
| `cmd/markdownx-server` | 协作服务器（Git 提交 + SSE） |
| `cmd/mdx-enc` | 加密工具（scrypt + secretbox） |
| `internal/doc` | 文档模型、Markdown 解析与导出（唯一的真相来源） |
| `internal/editor` | 块编辑器内核：光标、拆分/合并、缩进、undo |
| `internal/convert` | 导入：docx / xlsx / epub / ofd / pdf / rtf / html |
| `internal/export` | 自包含 HTML 导出 |
| `internal/book` | `.mdxb` 书籍：信封、章节、资源 |
| `internal/server` | HTTP API、Git 历史、SSE、网页编辑器 |
| `internal/app` | MyGo 原生 UI：窗口、侧栏、工具栏、预览 |
| `internal/rage` | 密码 / 服务端密钥加密 |
| `mygo.json` | 应用名、版本、图标、文件关联、安装包配置 |
| `opencode.json` | 本仓库的 codegraph233 MCP 配置 |

## 代码导航（codegraph233 MCP）

仓库提交了 `.codegraph233.json` 和 `opencode.json`，OpenCode 会通过 MCP 启动
`codegraph233 serve --root .`。索引在 `.codegraph233/`，不提交。

- 先 `cg233_status` / `cg233_overview`；单个文件用 `cg233_file_outline`。
- 按名字找声明用 `cg233_find_symbol`；引用/调用用
  `cg233_find_referencing_symbols` / `cg233_call_graph`；改前看
  `cg233_impact`；非符号文本用 `cg233_search_pattern`。
- 大改后运行 `codegraph233 index --root .`（增量，无改动 < 10ms）。
- 详见 [`docs/CODEGRAPH233.md`](docs/CODEGRAPH233.md)。

## 硬性约束

- **纯 Go，`CGO_ENABLED=0`。** 任何新依赖都必须在 `CGO_ENABLED=0` 下编译通过，
  不允许引入需要 C 工具链的库。
- **不引入 webview。** 界面全部用 `github.com/egoist/mygo/ui` 的原生控件；
  预览用 `ui/markdown`，不要写 HTML 页面再套 webview。
- **文档结构只经 `editor.Core` 修改。** 顺序是：`Checkpoint` → Core 方法 → `ResetBuffers`。
- **Markdown 读写必须往返无损。** 改了 `internal/doc` 的解析或导出，
  `internal/doc` 与 `internal/editor` 的测试必须全绿；新增块类型要同时补往返测试。
- **主题颜色用 `c.Theme()` 的语义 token**，不要写死十六进制。
- `go vet ./...` 与 `go test ./...` 每次都必须是干净的。

## 提交前

```sh
go vet ./...
go test ./...
CGO_ENABLED=0 go build ./...
CGO_ENABLED=0 go tool mygo build -platform windows/amd64 -o dist
```

## 构建产物

```sh
# Windows（本机）：安装包 + 便携 exe + 命令行工具
.\scripts\build-windows.cmd

# macOS arm64（在 macOS 上出 .app / .dmg）
./scripts/build-macos-arm64.sh

# 任意平台一次出两个目标
./scripts/cross-build.sh

# Windows 本地完整打包（含 SHA256SUMS）
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

MyGo 负责应用包本身：Windows 的 per-user NSIS 安装器、图标、文件关联、无控制台窗口；
macOS 的 `.app`、ad-hoc 签名和 `.dmg`。不要再手写 `Info.plist`、`codesign` 或
`hdiutil` 流程。

## 发布

- 版本号只有一个来源：`mygo.json` 的 `version`。
- `git tag v<version>` 必须与它一致；`.github/workflows/release.yml` 会校验。
- 标签推送后，Windows 与 macOS 分别用 `go tool mygo build` 构建，CLI 工具单独打包，
  最后合并为 GitHub Release（含 `SHA256SUMS`）。
- 发布前确认 `go vet ./...`、`go test ./...`、`go tool mygo build` 都干净。
- 安装说明见 [`docs/INSTALL.md`](docs/INSTALL.md)。

## 平台

- 只维护 **Windows x64** 和 **macOS arm64**。
- Go 的交叉编译不需要额外工具链：`GOOS=darwin GOARCH=arm64 go build` 在
  Windows 上就能出可用的 macOS 二进制。DMG 仍需要 macOS 生成。

## 用户数据（绝对不要碰）

- 产品名 MarkdownX，二进制 `MarkdownX.exe`（Windows）/ `MarkdownX`（macOS）。
  用户的文档库是真实数据：不要提交进仓库，不要删改。
- 测试一律用临时目录。

## 环境注意（Windows）

- shell 是 PowerShell 5：不支持 `&&`、`||`、三元、`??`、`?.`。
- 需要 VS 环境变量时，先 `call vcvars64.bat` 或显式设置 `LIB` / `INCLUDE` / `Path`。
