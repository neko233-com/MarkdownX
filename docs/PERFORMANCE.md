# 性能

MarkdownX 的 Go 重写目前没有发布官方性能数字。仓库里早先的 Rust/GPUI 数据来自
已删除的实现，不能和当前二进制直接比较。

## 可以确定的范围

- 构建是纯 Go、`CGO_ENABLED=0`；桌面应用是一个几 MB 到十几 MB 的单二进制。
- 编辑器把文档解析成块列表，渲染只读文档；没有 webview 和 JavaScript 运行时。
- 导入是纯 Go 解析；docx、xlsx、epub、ofd 按 ZIP + XML 读取所需条目。
- 协作服务器内嵌网页资源，单进程运行；历史操作走本机 Git。

## 自己测量

```sh
go test ./...
go tool mygo build -platform windows/amd64 -o dist
```

需要帧时间或内存数据时，用系统性能工具对 `dist/windows-amd64/MarkdownX.exe`
采样，并在 issue 里附上机器、文档大小和操作步骤。发布性能数字前应先在
Windows x64 与 macOS arm64 上各跑一遍，避免把单机结果当成通用结论。
