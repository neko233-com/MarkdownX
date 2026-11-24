# MarkdownX

**Windows x64** 与 **macOS arm64** 上的原生 Markdown 编辑器，用 Go 和
[MyGo](https://github.com/egoist/mygo) 写成。没有 webview，没有 C 工具链，不需要安装运行时。

[English](README.md)

## 安装

从 [GitHub Releases](https://github.com/neko233-com/MarkdownX/releases/latest) 下载：

- **Windows x64**：`markdownx-setup-<version>-amd64.exe`（当前用户安装）或便携版
  `MarkdownX.exe`。
- **macOS arm64**：`markdownx-<version>-arm64.dmg`，把 `MarkdownX.app` 拖进“应用程序”。

当前构建没有 Authenticode 签名，也没有 Apple 公证；首次运行时 SmartScreen /
Gatekeeper 可能要求确认。命令行工具（`markdownx-server`、`mdx-convert`、
`mdx-enc`）单独发布。校验和与细节见 [docs/INSTALL.md](docs/INSTALL.md)。

## 功能

- **Typora 手感的块编辑器**：连续行合并成一个段落，`Shift+Enter` 产生硬换行，
  保存再打开完全一致。
- **往返无损**：看到的就是 `.md` 文件里的内容——行末空格被当作格式而不是内容，
  列表的续行仍属于该列表项。
- **原生预览**：分栏里用 MyGo 自带的 Markdown 引擎渲染。
- **导入**：`.docx`、`.xlsx`、`.epub`、`.ofd`、`.pdf`、`.rtf`、`.html`
  直接打开，由纯 Go 读取器转成 Markdown。
- **导出**：Markdown、单文件自包含 HTML（离线可用）和 `.mdxb` 书籍。
- **书籍**：打开中英双语、多章节的 `.mdxb`，章节是 DocxXML 或 Markdown，媒体
  内嵌在文件里；也可以导出。
- **工作区侧栏**：打开文件夹，浏览其中的 Markdown，点击切换。
- **密码保存**：`Cmd/Ctrl+Shift+S` 用 scrypt + secretbox 加密当前页；
  独立工具 `mdx-enc` 读写同一格式。
- **协作服务器**：`markdownx-server` 通过 HTTP 共享一个 Markdown 文件夹，每次
  接受的保存都记为 Git 提交，提供历史、diff、restore，并用 SSE 推送其他人的编辑。

## 快捷键

| 按键 | 作用 |
|---|---|
| `Cmd/Ctrl+N` | 新建 |
| `Cmd/Ctrl+O` | 打开文件 |
| `Cmd/Ctrl+Shift+O` | 打开文件夹 |
| `Cmd/Ctrl+S` | 保存 |
| `Cmd/Ctrl+Shift+S` | 密码保存 |
| `Cmd/Ctrl+E` | 导出 HTML |
| `Cmd/Ctrl+B` / `I` | 粗体 / 斜体 |
| `Cmd/Ctrl+F` | 查找替换 |
| `Cmd/Ctrl+1`…`6` | 标题级别 |
| `Cmd/Ctrl+0` | 转回正文 |
| `Cmd/Ctrl+D` | 复制当前块 |
| `Cmd/Ctrl+Z` | 撤销 |
| `Enter` | 拆分块 |
| `Tab` | 缩进到上一块之下 |
| 行首 `Backspace` | 并入上一块 |

## 构建

只需要 Go 1.27.3 或更新版本。构建是 `CGO_ENABLED=0` 的纯 Go，
不需要 Xcode SDK、不需要 Visual Studio C++ 生成工具、不需要交叉编译器。
MyGo 已固定为 `go.mod` 的 tool 依赖，不需要全局安装 CLI。

```sh
# 当前目标
go tool mygo build -platform windows/amd64 -o dist
go tool mygo build -platform darwin/arm64 -o dist

# 任意平台一次出两个目标
./scripts/cross-build.sh
```

MyGo 负责 Windows 当前用户安装器和 macOS `.app`；在 macOS 上还会生成 `.dmg`。
Windows 安装器需要 NSIS（`makensis`），磁盘镜像需要 macOS 的 `hdiutil`。

macOS 上 `scripts/build-macos-arm64.sh` 会产出 `.app` 与 DMG；
Windows 上 `scripts/build-windows.cmd` 先跑检查，再出安装器和命令行工具；
`scripts/package.ps1` 额外生成 SHA-256 清单。

## 只有两个目标

Windows x64 与 macOS arm64。两者都由 Go 自带的交叉编译产出，
Windows 机器可以打 macOS 包，反之亦然。

## 不用界面也能转换

```sh
go run ./cmd/markdownx-convert report.docx > report.md
```

## 共享工作区

```sh
go build -o dist/markdownx-server ./cmd/markdownx-server
MARKDOWNX_SERVER_TOKEN=change-me ./dist/markdownx-server -root ~/notes -addr :8787 -init
```

浏览器打开 `http://localhost:8787/?token=change-me`。页面列出文件夹里的 Markdown，
支持多人实时更新、Git 历史、diff 和一键恢复。每次接受的保存都是一次提交。

| 接口 | 方法 | 用途 |
|---|---|---|
| `/api/docs` | GET | 列出文档 |
| `/api/doc?path=` | GET | 读取一篇及版本哈希 |
| `/api/doc` | PUT | 用 `{path, content, base}` 保存；base 过期返回 `409` |
| `/api/history?path=` | GET | 文档相关的提交 |
| `/api/diff?path=&commit=` | GET | 某次提交的 diff |
| `/api/restore` | POST | 把某次提交的内容写回 |
| `/api/events` | GET | SSE：`{path, base, action}` |

## 目录

```
cmd/markdownx/           桌面应用
cmd/markdownx-server/    协作服务器
cmd/markdownx-convert/   命令行转换器
cmd/mdx-enc/             加密工具
internal/doc/            文档模型、Markdown 读写
internal/editor/         块编辑器内核
internal/book/           .mdxb 书籍：信封、DocxXML、资源
internal/convert/        docx、xlsx、epub、ofd、pdf、rtf、html 读取器
internal/export/         自包含 HTML 导出
internal/server/         HTTP API、Git 历史、SSE、网页编辑器
internal/rage/           scrypt + secretbox 信封
internal/app/            MyGo 原生 UI
mygo.json                应用名、版本、图标、安装包
```

## 测试

```sh
go vet ./...
go test ./...
```

## 文档

| 文档 | 内容 |
|---|---|
| [docs/INSTALL.md](docs/INSTALL.md) | 下载、安装、校验和 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 分层与编辑流程 |
| [docs/FORMAT.md](docs/FORMAT.md) | Markdown 往返、`.mdxb`、加密 |
| [docs/RELEASE.md](docs/RELEASE.md) | 标签与发布流程 |
| [docs/CODEGRAPH233.md](docs/CODEGRAPH233.md) | 贡献者的代码图 MCP |

## 许可

MIT，见 [LICENSE](LICENSE)。
