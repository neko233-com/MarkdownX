# 存储格式

MarkdownX 不维护私有文档数据库。用户打开的 `.md` / `.markdown` 文件就是源文件；
应用只在内存里把它解析成块，保存时再写回 Markdown。`.mdxb` 是额外的书籍容器，
密码保存则是独立的加密信封。

## Markdown 文档

- 编码：UTF-8。
- 读取时 `\r\n` 和 `\r` 统一成 `\n`；写出使用 `\n`。
- 空行分隔块；连续普通行合并成一个段落块。
- 段落内的硬换行写成行尾两个空格 + `\n`，读取后是段落内的软换行，往返保持。
- 列表项内部的续行属于该列表项；真正的嵌套块保留为子块。
- 有序列表保留起始编号，不强制从 1 重新编号。
- 表格保留列对齐；单元格里的多行内容按 GFM 规则转义。
- 行内代码允许首尾空格和成组的反引号；代码块内容不做样式解析。
- 公式支持 `$...$` 与 `$$...$$`；未闭合的 `$$` 保持普通段落。
- 链接、图片、wiki 链接和相对路径按原文保留；不认识的 HTML/结构不丢。
- 导出时对 Markdown 标点做必要转义，但不把普通 `&` 变成实体。

往返测试在 `internal/doc/doc_test.go`；修改解析或导出后必须全绿。

## 书籍 `.mdxb`

`.mdxb` 是 UTF-8 JSON，顶层字段：

```json
{
  "format": "markdownx.book",
  "version": 2,
  "title": { "zh": "书名", "en": "Title" },
  "languages": ["zh", "en"],
  "default_language": "zh",
  "cover": "assets/cover.png",
  "theme": { "accent": "#4e83fd", "background": "#ffffff", "text": "#171717" },
  "reading": { "layout": "paged", "max_width": 720, "show_toc": true },
  "chapters": [
    {
      "id": "chapter-1",
      "title": { "zh": "第一章", "en": "Chapter 1" },
      "format": "markdown",
      "content": "# 第一章\n\n...",
      "format_en": "markdown",
      "content_en": "# Chapter 1\n\n..."
    }
  ],
  "assets": {
    "assets/cover.png": { "mime": "image/png", "data": "iVBORw0KGgo..." }
  }
}
```

- `format` 必须是 `markdownx.book`；当前支持 `version` 1 和 2。
- 章节 `format` 可以是 `markdown` 或 DocxXML；中英章节各自独立。
- 所有媒体放在 `assets`，值为 base64；读取时校验 MIME 与路径，不执行内容。
- `title` / `subtitle` 既接受对象，也接受纯字符串（纯字符串按中文处理）。
- 解析时忽略未知字段，重写时不保留它们；新增字段必须同时补往返测试。

## 加密信封

密码保存和协作服务器的磁盘加密都在 `internal/rage`，使用 scrypt（N=2^15, r=8, p=1）
派生 32 字节 key，再用 NaCl `secretbox` 认证加密。

### 密码信封（`mdx-enc`、桌面“密码保存”）

```
"!rage!" | version(2, LE) | salt(16) | nonce(24) | ctLen(4, LE) | ciphertext
```

- `version` 当前为 1。
- 盐和 nonce 每次随机；密文长度必须与 `ctLen` 一致。
- 密码错误或数据损坏返回认证失败，不会输出半个明文。
- 也提供 base64 包装，便于嵌入 Markdown 代码块。

### 服务端密钥信封（协作服务器透明加密）

```
"!sk!!" | nonce(24) | ciphertext
```

- key 由 `mdx-enc genkey` 生成，32 字节，放在服务器本地文件或环境变量中。
- 服务器读取时自动识别并解密；写回时重新加密。
- 密码信封与服务端信封都靠前缀识别，明文文件不会误判。
