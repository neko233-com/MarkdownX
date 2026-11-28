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

## 批量验收与产物保留

- 主流程使用左侧文件树和文档大纲，右侧原位阅读与编辑当前文档。顶部「文件／大纲」和左下角按钮切换这两页，底栏箭头显隐侧栏。单击文件或按 Enter 原位打开，单击大纲标题定位正文并保持侧栏展开。单文档窗口使用系统标题栏和菜单，底部提供源码开关、编辑状态及字数。
- 每批先列齐目标、差异、实现和用例，再集中验收。主流程覆盖空目录、文件夹展开、同窗切换、两页切换、实时大纲、标题定位、源码往返和无损保存。目录功能的回归保留排序、筛选、搜索、键盘与右键、外部文件变化、拖放和撤销。
- 一次跑完 `python -B scripts/check.py --native`，包含 Go 检查、全量测试、Windows amd64 和 macOS arm64 编译、隔离的原生窗口交互与截图。逐场景审阅默认宽度、窄窗口、高 DPI、浅色和深色布局。集中修复发现的失败后重跑同一验收入口。
- 分别记录编译、无窗口截图、原生窗口和系统拖放的实际验证范围。macOS 交叉编译只证明目标可编译，原生窗口与系统手势需在对应桌面验证。
- 一轮使用一个临时目录，结束时关闭测试进程并清理临时工作区、设置、工具副本、套接字和日志。截图及清单固定保存在 `dist/qa/desktop`，覆盖上一次验收的报告。保留失败现场时只留下定位问题所需的文件。
- 发布包输出到 `dist/release/<version>`。校验并安装当前 Windows 版本后，先用 `python -B scripts/clean_test_artifacts.py --prune-releases --dry-run` 核对范围，再移除超出保留量的本地包。常规保留当前版本、最近两个通过校验的版本及仍安装的版本。只处理该目录内结构及校验和均匹配的产物。

## 构建产物

```sh
# Windows（本机）：安装包 + 便携 exe + 命令行工具
.\scripts\build-windows.cmd

# macOS arm64（在 macOS 上出 .app / .dmg）
./scripts/build-macos-arm64.sh

# 任意平台一次出两个目标
./scripts/cross-build.sh

# Windows 本地完整打包（含 SHA256SUMS）
pwsh -File scripts/package.ps1
```

MyGo 负责应用包本身：Windows 的 per-user NSIS 安装器、图标、文件关联、无控制台窗口；
macOS 的 `.app`、ad-hoc 签名和 `.dmg`。不要再手写 `Info.plist`、`codesign` 或
`hdiutil` 流程。

- Windows 首次安装默认用 `C:\App\MarkdownX`，安装向导支持选择其他有写入权限的目录。覆盖安装沿用已安装路径。换目录时先卸载，再运行安装包选择新目录。
- `scripts/windows-installer.nsh` 通过 MyGo 的安装钩子执行上述路径约定并清理明确的包内文件。卸载只移除程序文件与系统注册，保留同目录中的其他文件以及用户配置。调整 `resources/licenses` 时同步卸载文件表。

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

- 本地开发和打包使用 64 位 PowerShell 7（`pwsh`）。
- 需要 VS 环境变量时，先 `call vcvars64.bat` 或显式设置 `LIB` / `INCLUDE` / `Path`。
