# 发布流程

MarkdownX 的版本号和安装包信息只有一个来源：`mygo.json`。

## 一次性准备

- Go 1.27.3+。
- Windows 安装器需要 [NSIS](https://nsis.sourceforge.io/)（`makensis`）。
  MyGo 在需要时会自动下载并校验 NSIS；CI 里用 `apt-get install nsis` 更快。
- macOS 的 `.dmg` 需要 macOS（`hdiutil`）。其它平台仍可交叉编译出 `.app`。
- 不需要 Apple Developer ID 或 Windows 代码签名证书；当前发布是 ad-hoc /
  未签名构建。要消除 Gatekeeper / SmartScreen 提示，再配置
  `macos.signingIdentity` / `macos.notarize` 和 `windows.certificate`。

## 发版

1. 修改 `mygo.json` 的 `version`（例如 `0.1.47`）。
2. 更新 `CHANGELOG.md` 的对应小节。
3. 本地验证：

   ```sh
   go vet ./...
   go test ./...
   CGO_ENABLED=0 go build ./...
   go tool mygo build -platform windows/amd64 -o dist
   go tool mygo build -platform darwin/arm64 -o dist
   ```

4. 提交并推送 `main`。
5. 打标签并推送：

   ```sh
   git tag v0.1.47
   git push origin v0.1.47
   ```

`.github/workflows/release.yml` 会：

- 校验标签与 `mygo.json` 的版本一致；
- 在 Ubuntu 上用 `go tool mygo build -platform windows/amd64` 生成
  `MarkdownX.exe` 与 `markdownx-setup-<version>-amd64.exe`；
- 在 macOS 上用 `go tool mygo build -platform darwin/arm64` 生成
  `MarkdownX.app` 与 `markdownx-<version>-arm64.dmg`；
- 用普通 `go build` 生成 `markdownx-server`、`mdx-convert`、`mdx-enc`，按平台打成
  CLI 压缩包；
- 合并 `SHA256SUMS`，创建 GitHub Release。

发布后可从 Actions 的 artifact 和 Release 页面下载。若某个平台失败，修复后重新
推送同一个标签（先删除远端标签再推，或使用 `workflow_dispatch` 指定已存在的标签）。

## 本地打包

```sh
# Windows：安装器 + 便携 exe + CLI zip + SHA256SUMS
powershell -ExecutionPolicy Bypass -File scripts/package.ps1

# 两个目标的应用包
./scripts/cross-build.sh

# 只做 macOS
./scripts/build-macos-arm64.sh
```

## 已知限制

- macOS 构建是 ad-hoc 签名，未公证；首次打开需要右键 →“打开”。
- Windows 安装器未做 Authenticode 签名，SmartScreen 会提示。
- 尚未接入 MyGo 的签名自动更新（`updates`）；当前发布不包含 update manifest。
  后续加入时需要 `mygo keygen` 的私钥作为 CI secret，并保持公钥在 `mygo.json`。
