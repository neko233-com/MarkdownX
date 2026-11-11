# 存储格式

## 文档库目录

默认位置：`~/Documents/MarkdownX`（「文稿」里的独立文件夹，用户能直接看到和备份）。可以用环境变量 `MARKDOWNX_HOME` 指定别处。

```
MarkdownX/
├── format.json          {"format": 1}：目录布局版本（store::FORMAT）
├── index.json           文档 id → Meta
├── index.json.bak       上一版 index.json
├── settings.json        窗口级 UI 设置（侧栏宽度等）
├── .lock                写锁（flock）：改写索引时短暂独占，不是单实例标记
├── docs/<id>.xml        每篇文档一份 DocxXML
├── assets/              粘贴或插入的图片、附件；assets/thumbs 是文件预览缩略图
└── trash/
    ├── index.json       id → Trashed { meta, deleted, root }
    └── <id>.xml
```

### Meta 字段

| 字段 | 含义 | 省略时 |
|---|---|---|
| `created` / `modified` / `opened` | unix 秒 | 必填；`opened` 默认 0 |
| `pinned` | 置顶 | false |
| `starred` | 收藏时间，0 表示未收藏 | 0 |
| `parent` | 父文档 id（文档树） | 顶层 |
| `order` | 同级排序键（最近一次移动的毫秒数） | 按 `created` 排 |
| `width` | 页宽：`standard`（820）/ `wide`（1020）/ `full` | standard |
| `catalogue_closed` | 目录已收起 | false |
| `reading` | 阅读模式 | false |
| `info` | 文档信息行：`{hidden, created, modified}` | 只显示修改时间 |
| `refs_hidden` | 隐藏引用列表 | false |

新字段一律带 `#[serde(default)]`，值为默认时不写出。这样旧版程序读新库会忽略它不认识的字段，新版程序读旧库会用默认值。

### 可靠性约定

- **原子写入。** 所有文件先写到 `<name>.tmp`，再 `rename` 覆盖原文件。崩溃时，磁盘上要么是旧内容，要么是新内容。
- **索引损坏后恢复。** 每次保存 `index.json` 前，先把旧文件复制成 `.bak`。读取时如果解析失败：
  - 坏文件改名为 `<name>.corrupt-<秒>` 保留，不会被覆盖；
  - 改用 `.bak`，并提示用户一次；
  - 没有可用的 `.bak` 时，用空索引。文档本身还在 `docs/` 里，只是置顶、层级这些信息会丢。
- **版本标记。** 打开库时，如果 `format.json` 里的版本比本程序新，会提示「请升级后再编辑」。改目录布局、会让旧版本误读的时候，要把 `FORMAT` 加一。
- **手放的文件也会出现。** 直接放进 `docs/` 的 `.xml` 文件（比如从飞书导出的 DocxXML）会出现在列表里。
- **保存失败不静默。**

## DocxXML

DocxXML 是飞书 / Lark 的文档 XML 格式。`crates/core/tests/fixtures/all-blocks.xml` 收录了每种 block 的规范写法，读入再写出必须逐字节不变（测试 `fixture_round_trips`）。

### 写出规则

- 顶层 block 每个占一行。子级换行，每层缩进两个空格。
- **表格单元格里的 block 写在一行内，不缩进。** 内容里的换行写成 `&#10;`（比如代码块的行），以免和排版用的换行混淆。
- 空的列表项如果有子段落，写成 `<li><p></p>…`。不加这个空 `<p>`，读回时子段落会被并进列表项。
- 折叠写成 `folded="true"`。图标和封面写在 `<title icon=… cover=… cover-x=… cover-y=…>` 上。

### 读入规则

- 只丢掉**源码排版产生的空白**：一个换行加缩进，以及嵌套 block 前面的空格和 Tab。其余空白都是内容，例如 `<p> </p>` 保留那个空格。
- 不认识的元素原样保留，写出时原样输出：block 级的是 `BlockKind::Raw`。
- 行内对象（公式、文件、按钮、日期提醒、@文档，以及不认识的行内元素）在文本里各占一个 U+FFFC，样式里存一个 `Inline { kind, xml }`：
  - `xml` 是读入时的原文，写出时原样输出，所以不认识的属性也不会丢；
  - `kind` 是读入时解析出来的类型，视图和导出只看它。
  - 编辑器新建或修改对象时，经 `docxml::to_inline` 先拼出规范 XML 再读回一遍，两者始终一致，存盘再读回也不变。

### 本地扩展

DocxXML 里没有、本应用自己加的写法。别的工具不认识这些写法，按上一节的规则它们只会被当作未知元素或属性：

- `<countdown duration start-time notify color timing/>`：倒计时。云端的倒计时是小组件，不在 DocxXML 里。
- `<time … is-whole-day="true">`：全天提醒。模型里有 `isWholeDay`，但 XML 里没有这个属性。没有此属性、且时间是本地 23:59 的提醒，也按全天处理。
- 图片和附件的本地文件写成 `path="…"`（云端用 `token` / `href`）。`<figure><source path=… name=…/></figure>` 表示视频或文件。

### 已知限制

- **单元格内的尾随空格。** 如果单元格里某个 block 有嵌套子级，它文本末尾的空格或 Tab 读回时会丢失。原因：旧版本写单元格时带缩进，读入要能兼容，所以必须丢掉嵌套 block 前的空白。fuzzer 的 `round_trip` 对这一点做了相同的预处理（`trim_before_nested`）。
- 代码块、分割线、图片等不能有子级的 block 如果在内存里带了子级，写出时这些子级会被丢掉。编辑器保证不会产生这种结构（见 ARCHITECTURE.md 的不变量）。

## 保存失败与恢复

写入使用同目录的唯一临时文件，文件同步后再重命名；Unix 平台还同步父目录。文档库写锁获取失败时拒绝修改，仍可只读列出笔记。检测到其他实例修改正文时不会覆盖磁盘内容，可以重新载入或另存副本。

系统退出阶段保存失败时，正文另存至系统本地应用数据目录的 `markdownx/recovery/<文档库标识>/`。下次打开同一文档库时创建恢复副本，成功写入后移除恢复文件。它是退出失败时的恢复措施，不是完整版本历史。

导出本地资源时检查规范化路径，拒绝文档库外的文件和越界符号链接。HTML 内嵌图片读取上限为 20 MiB，Markdown 复制的单个资源上限为 256 MiB；缺失、被拒绝或超限的 Markdown 资源会计入导出警告。行内附件与块附件均导出为链接并复制文件。

## MarkdownX Book（`.mdxb`）

一本动态书是一个 UTF-8 JSON 文件，扩展名 `.mdxb`，媒体类型 `application/vnd.markdownx.book+json`。v1 刻意不用 ZIP：桌面端和浏览器阅读器都能零依赖读取同一份文件；资源以 base64 内嵌，代价是体积增大约三分之一。

```json
{
  "format": "markdownx.book",
  "version": 1,
  "id": "quadratics",
  "title": { "zh": "二次函数", "en": "Quadratics" },
  "subtitle": { "zh": "把抛物线握在手里", "en": "Hold a parabola" },
  "authors": ["MarkdownX"],
  "languages": ["zh", "en"],
  "default_language": "zh",
  "cover": "assets/cover.png",
  "theme": { "accent": "#B4532A", "font": "serif" },
  "reading": { "layout": "scroll", "max_width": 760, "show_toc": true },
  "chapters": [
    { "id": "curves", "title": { "zh": "从一条曲线开始", "en": "Start with a curve" },
      "format": "docxxml", "content": "<title>…</title>", "content_en": "<title>…</title>" }
  ],
  "assets": { "assets/cover.png": { "mime": "image/png", "data": "iVBORw0KGgo…" } },
  "created": 1760000000,
  "modified": 1760000000,
  "generator": "MarkdownX 0.1.7"
}
```

### 字段

| 字段 | 必填 | 说明 |
|---|---|---|
| `format` | 是 | 固定 `markdownx.book`；不匹配直接拒绝 |
| `version` | 是 | 目前是 `1`；未来不兼容的布局要加一 |
| `id` | 否 | 书标识；导入时用于资源目录名，空则生成 |
| `title` / `subtitle` | 否 | `{zh, en}`；缺一边时回退到另一边 |
| `authors` / `languages` | 否 | 作者列表、书里出现的语言标签 |
| `default_language` | 否 | `zh` 或 `en`；阅读器初始语言 |
| `cover` | 否 | 资源 key（`assets/...`）或 https URL |
| `theme` | 否 | `accent`、`background`、`text`、`font`（`serif`/`sans`） |
| `reading` | 否 | `layout`（`scroll`）、`max_width`、`show_toc` |
| `chapters` | 是 | 至少一章，`id` 不能重复 |
| `assets` | 否 | key → `{mime, data}`，`data` 是标准 base64，不带 data URI 前缀 |
| `created` / `modified` | 否 | unix 秒 |
| `generator` | 否 | 生成器标识，只用于诊断 |

`chapters[]` 的 `format` 是 `docxxml` 或 `markdown`。DocxXML 章节的 `content` 是完整的 DocxXML，可以带 `<title>`；Markdown 章节在导入时走 `paste::markdown_to_blocks`。`content_en` 可选，是同一章的英文正文，阅读器切英文时优先用它；桌面文档库只保留主 `content`，所以导出再导入会丢掉 `content_en`（已知限制，后续会把双语正文并入文档模型）。

### 资源 key 与安全

- key 必须以 `assets/` 开头，只能是相对路径；拒绝 `..`、`\`、`:` 和绝对路径。
- 打包时只读文档库根目录内的文件，跳过 `http:`、`https:` 和 `data:`，单文件上限 16 MiB、总资源上限 64 MiB。
- 导入时资源写到 `<文档库>/assets/books/<uuid>/assets/...`，并把正文里的 `src` 改写成这个相对库根的路径。这样文档库整体移动后图片仍然有效，多次导入也不会互相覆盖。
- 封面同样按资源 key 处理；不是资源 key 又不是 https 的封面会被 `validate` 拒绝。空字符串表示没有封面，合法。

### 交互块

DocxXML 的 block 级写法：

```xml
<interactive type="canvas" title="拖动参数" height="360"><![CDATA[
  {"kind":"quadratic","a":1,"b":0,"c":0,"xMin":-4,"xMax":4,"yMin":-4,"yMax":8}
]]></interactive>
```

- `type`：`canvas`（默认）或 `html`，大小写不敏感。
- `title`：卡片标题，可空。
- `height`：120–1600 像素，越界会被夹住；缺省 360。
- 正文是 CDATA 里的源码。`canvas` 是 JSON spec，`html` 是完整 HTML/JS。
- 在表格单元格里不能写 CDATA（单元格会把换行压成 `&#10;`），改写转义文本。
- 写出时 `]]>` 会被拆成 `]]]]><![CDATA[>`。

Markdown 用围栏代码块，info 是 `mdxb-canvas` 或 `mdxb-html`，正文是一层 JSON 信封：

````markdown
```mdxb-canvas
{"title":"拖动参数","height":360,"source":"{\"kind\":\"quadratic\",\"a\":1}"}
```
````

围栏长度按 `source` 里最长的反引号串加一，至少三个；读入时 JSON 解析失败或 `source` 为空会退回普通代码块。`canvas` 的 Web 运行时在 `web/js/widgets.js`，实现 `quadratic`、`unitcircle`、`projectile`；桌面端原生实现 `quadratic`（`src/editor/interactive.rs`），其他 `kind` 在桌面显示提示、在 Web 阅读器运行。未知 `kind` 在阅读器里显示“没有运行时”。`html` 在阅读器里放进 `iframe sandbox="allow-scripts"`，不能读宿主页面或本地文件；桌面端暂不执行 HTML。

### 导入导出

桌面端「下载为 → MarkdownX 书 (.mdxb)」把当前页和它的子页按文档树顺序打包：当前页是第一章，子页依次成章。导入时第一章建为根文档，其余章用 `Meta.parent` 挂到根文档下；资源写到独立目录，正文 `src` 改写为库内相对路径。

「文件 → 打包成电子书…」是另一条入口：在文件夹工作区里把该目录下所有 `.md` 按文件名顺序装成章节，在单文件模式下只装当前 `.md`。它不依赖文档库，适合 agent 或脚本生成的书籍目录。

「下载为 → Markdown 文件夹 (.md)」是反向入口：选定一个 `.md` 路径后，在它旁边建同名目录，写出 `01-title.md`…、`book.json`、`assets/`，再写一个带章节链接的索引 `.md`。章节正文来自 `markdown::export`；标题、表格、章节顺序和 `mdxb-canvas` / `mdxb-html` 源码都会保留，callout 颜色、分栏等纯展示可能退化为普通 Markdown。每章旁边还会写一份同名的 `.xml` DocxXML 源文件（`book.json` 的 `source` 字段），因此重新打包时只要 `.xml` 不比 `.md` 旧，就会优先用 XML 还原完整结构；编辑过 `.md` 后则以 Markdown 为准。源书带 `content_en` 时再写 `NN-title.en.md`，`book.json` 用 `content_en_file` 指过去，重新打包会把它装回 `content_en`。把该目录再交给「打包成电子书…」即可回到 `.mdxb`。`book.json` 记录章节 id/标题/文件名/XML 源，供 agent 与后续工具使用。

## 文件夹工作区

`文件 → 打开文件夹…` 将所选目录添加到侧栏，多个工作目录可同时显示并在重启后恢复；默认目录是系统文档目录下的 `MarkDown-Demo`。目录树直接来自文件系统，普通子目录（包括没有 Markdown 的目录）及 `.md`、`.markdown`、`.mark`、`.txt`、`.mdxb` 文件可见；名称以点开头的项、符号链接、`target`、`node_modules` 和 `__pycache__` 会跳过。选择 Git 仓库内的目录时保持所选路径，不自动扩大到仓库根目录，也不自动打开首个文件。每个目录可独立刷新、更换或移除；移除工作区不会删除磁盘文件。不可读取的目录显示错误而不阻止其他目录；侧栏和全局搜索可跨目录匹配路径与正文。文件或文件夹可在侧栏新建和重命名，删除经确认后交给系统回收站。

`.md` 等 UTF-8 文本文件直接在原位置编辑和回写，`.mdxb` 仍按书导入并打开。保存时尽量保留源文件的标题形式，并沿用 Markdown 导出选项：图片相对 `assets/` 复制，正文重新序列化，因此源文件中的原始排版或未支持的语法可能变化；UTF-8 BOM 和主要的 CRLF 换行会保留。写回前先比较最近读取或保存的原文，发现外部修改、文件消失、导出后的正文不能往返或资源无法导出时保留本地未保存内容，供重新载入或另存副本。该检查不保证所有 Markdown 语义或原始格式都能保留。写回使用同目录临时文件 + `fsync` + rename；读取比较与替换之间仍有并发窗口，不是跨进程锁。

## 批注（annotations）

批注是独立 sidecar，正文不被改写：

- 文件夹模式：`lesson.md.annotations.json`，与源文件同目录。
- 文档库模式：`<文档库>/annotations/<page-id>.json`。

```json
{
  "format": "markdownx.annotations",
  "version": 1,
  "doc": "lesson.md",
  "annotations": [
    {
      "id": "a1",
      "author": { "id": "alice", "name": "Alice", "color": "#b05226" },
      "body": "先说明参照物。",
      "quote": "一辆小车从站台出发。",
      "anchor": [2, 0],
      "created": 1760000000000,
      "updated": 1760000000000,
      "resolved": false,
      "replies": []
    }
  ]
}
```

`anchor` 是从文档根开始的 block 路径；`quote` 是给未变化的正文留的人类可读快照。多用户离线批注用 `python tools/agent/mdxb.py merge-annotations out.json a.json b.json` 合并，同一个 id 取 `updated` 较新的版本，回复按 id 取并集。
