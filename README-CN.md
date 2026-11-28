# MarkdownX

**Windows x64** 与 **macOS arm64** 上的原生 Markdown 编辑器，用 Go 和
[MyGo](https://github.com/egoist/mygo) 写成。没有 webview，没有 C 工具链，不需要安装运行时。

[English](README.md)

## 安装

从 [GitHub Releases](https://github.com/neko233-com/MarkdownX/releases/latest) 下载：

- **Windows x64**：`markdownx-setup-<version>-amd64.exe`（当前用户安装）或便携版
  `MarkdownX.exe`。
- **macOS arm64** 打开 DMG 或解压 `markdownx-<version>-darwin-arm64.tar.gz`，把 `MarkdownX.app` 拖进“应用程序”。

当前构建没有 Authenticode 签名，也没有 Apple 公证；首次运行时 SmartScreen /
Gatekeeper 可能要求确认。命令行工具（`markdownx-server`、`mdx-convert`、
`mdx-enc`）单独发布。校验和与细节见 [docs/INSTALL.md](docs/INSTALL.md)。

## 功能

用「文件 → 打开文件夹」显示左侧目录树。单击 Markdown 文件，在右侧阅读和直接编辑；
再次点选文件时原位切换。用侧栏顶部「文件／大纲」或左下角按钮切换，点大纲里的标题可定位到正文。
底栏箭头收起或恢复侧栏，旁边的「源码」切换到 Markdown 文本。

- **原生富文本编辑**。单栏正文即时显示粗体、斜体、链接和行内代码。用 `Shift+Enter`
  插入硬换行，键入表头后回车建表，`Tab` 切换单元格。可拖选或按住 `Shift` 跨段选中正文。
- **原位编辑 Markdown**。光标进入行内格式时展开原有标记。点击图片或行内公式编辑源码。
  用 `Cmd/Ctrl+K` 插入或编辑链接，按住 `Cmd/Ctrl` 点击链接跳转。
- **目录与脚注**。输入 `[toc]` 后回车生成可点击目录。脚注 `[^标识]` 支持悬停查看，
  `Cmd/Ctrl+点击` 跳转到定义，点击 `↩` 返回引用。参考式链接 `[文字][标识]` 和 `[标识][]` 使用文档内的定义。
- **富文本剪贴板**。跨段复制同时提供 HTML、RTF 和纯文本。从 MarkdownX 复制后粘贴可保留
  标题、行内标记和表格结构。粘贴 HTML 时可保留常见的标题、引用和表格。
- **Markdown 源码**。用 `Cmd/Ctrl+/` 切换编辑视图和可编辑源码。保存保留未改动的
  Markdown 区段、图片引用、BOM 和换行风格。磁盘文件已更新时，可用「另存为」保存手头修改。
- **原生预览**。分栏和预览由 MyGo Markdown 引擎渲染。可用大纲跳转标题，按 F8 进入专注模式，
  按 F9 进入打字机模式。
- **Markdown 图表**。在偏好设置中打开「渲染 Markdown 图表」，正文和预览会显示 `mermaid`、
  `flow`、`sequence` 围栏。点图表编辑，按 `Esc` 回到预览。右键可复制或导出 SVG、PNG、JPEG，
  HTML 导出包含离线图表。暂未支持的图表保留可编辑源码。
- **代码围栏**。在围栏下方修改语言，已有语法由 MyGo 高亮。光标在代码中时，`Cmd/Ctrl+A` 选中当前围栏。
- **公式排版**。行内和独立公式原生矢量渲染。输入 `$$` 后回车编辑公式块，超出渲染范围的
  表达式保留 TeX 源码。
- **导入**：`.docx`、`.xlsx`、`.epub`、`.ofd`、`.pdf`、`.rtf`、`.html`
  直接打开，由纯 Go 读取器转成 Markdown。
- **导出**：Markdown、单文件自包含 HTML（离线可用）、PDF 和 `.mdxb` 书籍。
  PDF 支持纸张、方向、页边距、页眉页脚和分页，保留可复制的正文及目录链接。打印预览保存并打开 PDF。
- **书籍**：打开中英双语、多章节的 `.mdxb`，章节是 DocxXML 或 Markdown，媒体
  内嵌在文件里；也可以导出。
- **文件目录树**。侧栏按文件夹层级显示工作区，支持方向键导航、回车打开和定位当前文件。
  大目录仅构建视口附近的条目，排序和刷新会保留选中路径。
  在工作区内从外部重命名或移动同一条目时，目录树会跟随新路径，保留展开状态、选中项和正在编辑的 Markdown 内容。
  拖动侧栏右边缘，或聚焦分隔线后用方向键调整宽度。宽度会保存，双击分隔线可恢复默认。
  右键可新建、重命名、复制、移动、移到回收站或废纸篓，以及撤销上次文件操作。
  单击文件或按 Enter 会原位打开。右键的「打开方式」可另开标签页或窗口。有多个标签页时可以拖动排序，每个标签页保留未保存的内容、选区和撤销记录。
  「打开方式 → 在分栏中打开」及拖到编辑区边缘或分隔线，都可在同一标签页并排打开文档。拖动分隔线调整宽度，拖动栏标题排序或拖到标题栏分离。同一文档可在多栏编辑，修改和撤销同步，光标及滚动位置各自保留。
  可将普通或分栏标签页拖到另一个窗口的标签栏，或拖离后松开，在落点附近另开窗口。拖动时按 `Esc` 取消。移动后可继续编辑原草稿和撤销上一次修改。
  整行都可右键和拖放。选中项滚出视口时，按 `Shift+F10` 会定位后打开菜单。狭窄侧栏会压缩深层缩进，留出显示名称的空间。
  从文件行可直接新建同级文件或文件夹。文件名无受支持的扩展名时，新建文件和从模板新建沿用偏好设置的默认类型，可选 `.md`、`.markdown`、`.text` 和 `.txt`。
  在文件或文件夹上选择「从模板新建」，可在对应目录创建并打开 Markdown。支持 `typora-template` 的默认文件名和说明，以及标题、日期、时间、剪贴板、UUID 和光标变量。用 `Tab` 和 `Shift+Tab` 填写 `{{1:标题}}` 等数字占位符，同编号内容会同步更新。
  拖动会高亮目标并提示操作。松手即移动工作区内的条目、复制工作区外的条目，按住 Alt／Option 可复制整批。拖到文件行会放入其父文件夹，拖到空白处会放入工作区根目录。目录树右键菜单可一次撤销整个拖放操作。拖到系统文件管理器可复制。
  从目录树、其它 MarkdownX 窗口或系统文件管理器拖入正文时，支持的图片引用原文件，其它条目生成链接。无标题文档和跨盘条目使用本机文件链接。源码模式会在指针落点显示插入光标，代码或公式源码会插入原始路径。
  点根目录名称可切换工作区、刷新、排序、筛选，或在文件管理器、终端和 Visual Studio Code 中打开文件夹。滚动后菜单仍在侧栏顶端。底部的加号用于新建文件或文件夹。跨目录移动 Markdown 时会同步调整相对图片引用。
  向 MarkdownX 传入目录路径即可打开文件树。偏好设置可选择启动时恢复文件夹、同时恢复文档，或用指定的默认文件夹新建文档。
  悬停可预览 Markdown 摘录或常见位图。文件夹可以固定并从「快速打开」切换。按 `Ctrl/Cmd+K` 可在目录树中显示搜索结果，保留当前文档。
  工作区搜索按目录分组，点击结果定位到对应文档和匹配行。
- **密码保存**。通过「文件 → 密码保存」用 scrypt + secretbox 加密当前页，
  独立工具 `mdx-enc` 读写同一格式。
- **协作服务器**：`markdownx-server` 通过 HTTP 共享一个 Markdown 文件夹，每次
  接受的保存都记为 Git 提交，提供历史、diff、restore，并用 SSE 推送其他人的编辑。

## 快捷键

| 按键 | 作用 |
|---|---|
| `Cmd/Ctrl+N` | 新建 |
| `Cmd/Ctrl+O` | 打开文件 |
| `Ctrl+P` / `Cmd+Shift+O` | 快速打开（Windows / macOS） |
| `Ctrl+Shift+1` / `Cmd+Ctrl+1` | 显示并聚焦文档大纲（Windows / macOS） |
| `Ctrl+Shift+3` / `Cmd+Ctrl+3` | 显示并聚焦文件目录树（Windows / macOS） |
| `Ctrl/Cmd+Shift+F` | 搜索工作区文件名或正文 |
| `Cmd/Ctrl+Shift+L` | 显示或隐藏侧栏 |
| `F2` | 重命名文件树中获得焦点的条目 |
| `Delete` / `Cmd+Backspace` | 移到回收站或废纸篓前确认（Windows / macOS） |
| `Cmd/Ctrl+Shift+N` | 新建窗口 |
| `Cmd+T` | 新建标签页（macOS） |
| `Cmd/Ctrl+W` | 关闭标签页 |
| `Cmd/Ctrl+Shift+T` | 重新打开已关闭的标签页或文件 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 下一个 / 上一个标签页 |
| `Ctrl/Cmd+Alt+\` | 将当前文档向右分栏 |
| `Cmd+Ctrl+←` / `Cmd+Ctrl+→` | 聚焦左侧 / 右侧分栏（macOS） |
| Cmd+反引号 | 切换窗口（macOS） |
| `Cmd/Ctrl+S` | 保存 |
| `Cmd/Ctrl+Shift+S` | 另存为 Markdown |
| `Cmd/Ctrl+Shift+E` | 导出 HTML |
| `Cmd+P` | 打印预览（macOS） |
| `Cmd/Ctrl+B` / `I` | 粗体 / 斜体 |
| `Cmd/Ctrl+K` | 插入链接或编辑链接地址 |
| `Cmd/Ctrl+F` / `H` | 查找 / 替换 |
| `Cmd/Ctrl+A` | 选中整篇正文，或选中光标所在的代码围栏 |
| `Shift+方向键` | 扩展正文选区 |
| `Ctrl+Shift+Home/End` / `Cmd+Shift+↑/↓` | 扩展选区到正文开头或末尾（Windows / macOS） |
| `Cmd/Ctrl+Shift+C` / `V` | 复制为 Markdown / 粘贴纯文本 |
| `Cmd/Ctrl+/` | 切换 Markdown 源码 |
| `F8` / `F9` | 专注 / 打字机模式 |
| `F11` / `Cmd+Option+F` | 全屏（Windows / macOS） |
| `Cmd/Ctrl+1`…`6` | 标题级别 |
| `Cmd/Ctrl+0` | 转回正文 |
| `Cmd/Ctrl+D` / `Cmd/Ctrl+Shift+D` | 选词 / 删除词语 |
| `Cmd/Ctrl+[` / `]` | 缩进 / 取消缩进 |
| `Alt+↑` / `↓` | 移动段落或表格行 |
| `Cmd/Ctrl+Enter` / `Cmd/Ctrl+Shift+Backspace` | 插入 / 删除表格行 |
| `Ctrl+Shift+M` / `Cmd+Option+B` | 公式块（Windows / macOS） |
| `Cmd/Ctrl+Z` | 撤销 |
| `Enter` | 拆分块 |
| `Tab` | 移到下一个表格单元格或缩进块 |
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
