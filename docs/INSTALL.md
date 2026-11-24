# 安装

MarkdownX 只发布 **Windows x64** 与 **macOS arm64** 两个目标。所有安装包都由
[MyGo](https://github.com/egoist/mygo) 的构建流程生成。

## Windows x64

从 [Releases](https://github.com/neko233-com/MarkdownX/releases/latest) 下载：

| 文件 | 用途 |
|---|---|
| `markdownx-setup-<version>-amd64.exe` | 推荐。当前用户安装，创建开始菜单项、文件关联和卸载项 |
| `MarkdownX.exe` | 便携版，不安装，直接运行 |

安装器不需要管理员权限，默认装到 `%LOCALAPPDATA%\Programs\MarkdownX`，并注册
`.md` / `.markdown` / `.mdxb` 文件关联。安装包没有 Authenticode 签名，SmartScreen
可能提示“更多信息 → 仍要运行”。

> **从 0.1.46 或更早的 MSI 升级：** 先在“设置 → 应用”里卸载旧的 MarkdownX，
> 再安装 0.1.47。旧版和 Go 版使用不同的安装器和卸载项，直接覆盖会在同一目录里
> 留下两套文件。旧版的文档库在 `~/Documents/MarkdownX`，卸载不会删除它；升级前
> 请先用旧版导出需要保留的文档。

## macOS arm64

下载 `markdownx-<version>-arm64.dmg`，打开后把 `MarkdownX.app` 拖进
“应用程序”。应用使用 ad-hoc 签名；没有 Apple Developer ID 公证时，首次打开需要
右键 →“打开”，或在“系统设置 → 隐私与安全性”里允许。

## 命令行工具

需要 `markdownx-server`、`mdx-convert` 或 `mdx-enc` 时，下载对应平台的 CLI 包：

| 平台 | 文件 |
|---|---|
| Windows x64 | `markdownx-cli-<version>-windows-amd64.zip` |
| macOS arm64 | `markdownx-cli-<version>-darwin-arm64.tar.gz` |

解压后把目录加入 `PATH` 即可。校验所有下载文件：

```sh
sha256sum -c SHA256SUMS
# macOS
shasum -a 256 -c SHA256SUMS
```

## 从源码构建

只需要 Go 1.27.3+；构建是纯 Go、`CGO_ENABLED=0`，不需要 Xcode 或 Visual Studio
C++ 工具。MyGo 已作为 `go.mod` 的 tool 依赖固定版本。

```sh
go tool mygo build -platform windows/amd64 -o dist
go tool mygo build -platform darwin/arm64 -o dist
```

Windows 安装器需要 NSIS（`makensis`）；macOS 的 `.dmg` 需要 macOS 的 `hdiutil`。
本地打包脚本：

```sh
# Windows
.\scripts\build-windows.cmd
powershell -ExecutionPolicy Bypass -File scripts\package.ps1

# macOS
./scripts/build-macos-arm64.sh

# 两个目标
./scripts/cross-build.sh
```
