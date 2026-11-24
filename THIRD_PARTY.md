# 第三方组件与素材

MarkdownX 本体以 MIT 协议发布（见 [LICENSE](LICENSE)）。下面是随桌面二进制一起
编译的 Go 模块，以及仓库里仍保留的第三方浏览器素材。

## 编译进桌面二进制的 Go 模块

由 `go list -deps` 对 `cmd/markdownx`、`cmd/markdownx-server`、
`cmd/markdownx-convert`、`cmd/mdx-enc` 求并集得到。

| 模块 | 版本 | 许可 | 上游 |
|---|---|---|---|
| `github.com/egoist/mygo` | v0.10.0 | MIT | https://github.com/egoist/mygo |
| `github.com/ebitengine/purego` | v0.11.1 | Apache-2.0 | https://github.com/ebitengine/purego |
| `github.com/go-text/typesetting` | v0.3.5 | UNLICENSE OR BSD-3-Clause | https://github.com/go-text/typesetting |
| `golang.org/x/crypto` | v0.59.0 | BSD-3-Clause | https://pkg.go.dev/golang.org/x/crypto |
| `golang.org/x/image` | v0.46.0 | BSD-3-Clause | https://pkg.go.dev/golang.org/x/image |
| `golang.org/x/net` | v0.62.0 | BSD-3-Clause | https://pkg.go.dev/golang.org/x/net |
| `golang.org/x/text` | v0.44.0 | BSD-3-Clause | https://pkg.go.dev/golang.org/x/text |

Go 标准库随 Go 工具链以 BSD-3-Clause 分发。各模块的完整许可文本在模块目录的
`LICENSE` 文件中；构建时由 Go 从 `go.sum` 校验的同一份源码编译。

## 仓库中的浏览器素材（不进入桌面安装包）

`web/`、`assets/` 下的网页阅读器和演示站仍引用以下未修改的上游构建文件：

| 素材 | 许可 |
|---|---|
| `web/lib/three/` Three.js 0.187.0 | MIT，见 `web/lib/three/LICENSE` |
| `assets/docx-viewer/` docx-preview 0.4.1 | Apache-2.0，见 `assets/docx-viewer/DOCX-PREVIEW-LICENSE` |
| `assets/docx-viewer/jszip.min.js` JSZip 3.10.2 | MIT 或 GPL-3.0-or-later，此处选择 MIT，见 `assets/docx-viewer/JSZIP-LICENSE.md` |
| `assets/ofd-viewer/` OFD.js 0.1.1 + fflate | MIT，见 `assets/ofd-viewer/OFDJS-LICENSE` 与 `FFLATE-LICENSE` |
| `assets/fonts/katex/` KaTeX 字体 | MIT，见同目录 `LICENSE` |
| `assets/editor-icons/`、`assets/icons/` 中的 Lucide 图标 | ISC，见同目录 `LICENSE-lucide` |

`assets/themes/`、`assets/covers/`、`assets/illus/` 与 `assets/branding/` 中的
其余素材由本项目绘制或生成，随 MIT 发布。`assets/branding/app-icon.png` 是
MyGo 构建时使用的应用图标。

## 测试数据

`MarkdownX-Demo/` 与 `internal/convert` 的测试样本用于验证导入；其中的第三方
文档（如有）保留其原始许可，不随安装包分发。
