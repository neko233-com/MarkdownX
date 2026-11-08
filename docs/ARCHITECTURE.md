# 架构

MarkdownX 的桌面端是一个单进程 GPUI 应用（Rust，约 4 万行）；`server/` 是另外的 Go module，负责可选的多人协作。一篇文档在内存里是一棵 block 树（`model.rs`），在磁盘上是一份 DocxXML 文件（`docxml.rs`）。编辑器把这棵树画成一个虚拟化列表。

仓库是一个 Cargo workspace，分两个 crate：

- **`crates/core`（`fdl_core`，约 1 万行）**：文档引擎。包括模型、DocxXML、编辑内核和撤销、磁盘上的文档库、Markdown / HTML 导入和 Markdown 导出，以及 `.mdxb` 书的打包/解包。它不依赖 GPUI，可以单独编译和测试（`cargo test -p fdl_core`），fuzzer 也在这里。
- **根 crate（`markdownx`）**：GPUI 应用，即窗口、视图、渲染、菜单和浮层。`main.rs` 把 `fdl_core` 的模块重新导出到 crate 根，`editor/mod.rs` 重新导出 `editor::core` 和 `editor::table_ops`，所以应用里仍然写 `crate::model`、`super::core`。

依赖只能从应用指向 core。core 里的模块只引用 core 里的模块。

```
main.rs ─ 窗口
  └─ app.rs  Workspace（窗口根视图）
       ├─ store.rs        文档库：docs/*.xml + index.json（见 FORMAT.md）           [core]
       ├─ sidebar / home / search / trash / tree / docmenu / docinfo / modes
       └─ editor::Editor（view.rs）   打开的那篇文档
            └─ editor::Core（core.rs） 文档 + 选区 + 撤销栈                        [core]
```

## 分层

| 层 | 文件 | 职责 | 规则 |
|---|---|---|---|
| 模型 | `model.rs` | `Document` / `Block` / `BlockKind` / `RichText`（文本 + 样式 run）/ 行内对象 `Inline { kind, xml }` | 纯数据。`BlockKind::nests()` 决定哪些 block 能有子级 |
| 格式 | `docxml.rs` | DocxXML ⇄ `Document` | 未知元素原样保留为 `Raw`；行内对象保留原文、读入时解析一次类型；写出的格式稳定，见 FORMAT.md |
| 编辑内核 | `editor/core.rs`、`editor/table_ops.rs` | 所有改文档的操作：输入、删除、合并、缩进、转换、拖动、表格、折叠、行内对象、撤销 | 在 `fdl_core` 里，不碰 GPUI，能直接单测和 fuzz |
| 编辑视图 | `editor/view.rs` + 其余 `editor/*` | 键鼠事件 → Core 调用；浮层、菜单、各 block 的交互 | 改文档只经过 Core（下文） |
| 渲染 | `editor/render.rs`、`editor/text.rs` | block → GPUI 元素；`text.rs` 是自绘富文本元素 | 只读文档 |
| 应用 | `app.rs` 等顶层模块 | 侧栏、主页、搜索、回收站、文档树、导入导出、打印 | 通过 `Store` 读写磁盘 |

## 编辑流程

一次编辑的顺序是固定的：

1. 视图收到事件，调用 `core.checkpoint(kind)`。它记一个撤销快照，把 `generation` 加一，并置 `dirty`。连续输入（同一 block、相邻两次间隔不到 700ms）合成一步。
2. 调用 Core 的操作方法修改 `core.doc` / `core.sel`。
3. 调用 `Editor::changed()`：
   - 先 `core.reveal_caret()`：如果选区（或块选区）两端被折叠藏起来了，就展开把它藏起来的标题或列表项；
   - 有改动时发 `EditorEvent::Changed`，同时刷新查找结果、斜杠菜单和 @ 面板。
4. `Workspace` 收到 `Changed` 后调用 `schedule_save`，500ms 后 `flush()` 写盘。写盘失败时改动保持未保存，顶栏显示错误，切换文档、新建、退出前会询问。保存失败对话框支持重试、重新载入磁盘内容、另存恢复副本；重新载入同时重置编辑器及同文档窗口的状态。退出检查所有窗口，处理单个冲突后重新检查，仍有冲突时不会退出。系统退出阶段仍保存失败的正文存入本地应用数据目录的 `markdownx/recovery/<文档库标识>/`，下次打开同一文档库时恢复为新文档，不覆盖磁盘原版。

Core 的不变量由 `crates/core/src/editor/fuzz_tests.rs` 在每一步随机编辑后检查：

- 选区落在可见的光标停靠点上，两端在同一作用域（同一单元格或正文）；
- block id 唯一，run 不越界、不重叠；行内对象的 run 里只有 U+FFFC；
- 分栏比例和为 1，表格的合并格不重叠；
- 只有 `nests()` 的 block 和容器（引用、高亮块、分栏、列）有子级；
- 代码块里没有样式和行内对象；
- 存盘再读回结果不变；
- 撤销到底再重做到头，回到同一状态。

### 直接修改的例外

结构性修改（增删、移动 block，合并、拆分文本）全部在 Core 里。少数**只改单个 block 属性**的地方仍在视图模块里通过 `core.block_mut(id)` 直接写，前后同样是 `checkpoint` / `changed`：

- 图片尺寸、裁剪、描述（`image.rs`）
- 倒计时设置（`countdown.rs`）
- 代码块名称（`caption.rs`）
- 图标、封面（`cover.rs`）
- 标题重命名（`view.rs`）
- 若干勾选、折叠切换（`view.rs`、`render.rs`）

它们不改树结构，所以不会破坏上面的不变量。新增的结构性操作一律写进 Core，并在 fuzzer 的 `step` 里加一种随机操作。

## 撤销

- 每一步是整篇 `Document` 的快照（`Snapshot`），存在 `VecDeque` 里。上限是 500 步，并且约 256MB；文档再长也至少保留 20 步。
- 文档图标和折叠状态不进历史：撤销和重做时，`keep_outside_history` 保留它们当前的值。
- `generation` 在每次改动（包括折叠）时递增。渲染缓存、目录、字数统计都靠它判断是否需要重算。

快照方案简单可靠，代价是长文档每一步要复制整篇。百万字文档的历史实测中，输入同步帧处理 p50 为 5.06ms；这不包含屏幕呈现延迟，测试版本和口径见 README。

## 渲染与虚拟化

- 页面是一个 GPUI `List`。条目依次是：标题区（`Item::Header`）、每个可见的顶层 block、`Item::Below`（引用列表）。
- 文本、标题、列表这类 block 的子级各自成一行（`flattens`）。容器类 block 的子级画在容器那一行里面。
- `sync_items` 给每一行算一个 key，即 `hash_block`。它和上一帧的 key 比较，只 `splice` 前后缀之外变化的区间，所以只有变化的行重新测量高度。
- `hash_block` 对文本只取长度。原因：列表每帧都会重新测量可见行，同长度的修改（比如对整段 ⌘B）最多让屏幕外某一行的缓存高度暂时过期。对文本本身做哈希，在百万字文档上每次按键要多花约 3ms。实测记录在 `render.rs` 的注释里。
- 滚动条用 vendor GPUI 新增的 `ListState::estimated_scroll_metrics`：没量过的行按平均行高估算（见 vendor/PATCHES.md）。

## 模块地图

文档引擎（`crates/core/src/`）：

| 模块 | 内容 |
|---|---|
| `model.rs` | 文档模型，包括行内对象类型、`display_title` |
| `docxml.rs` | DocxXML 解析与序列化；`to_inline` 构造行内对象 |
| `store.rs` | 文档库读写、索引恢复、回收站、设置；`image_path` |
| `paste.rs` | HTML / Markdown → block（粘贴和导入共用） |
| `markdown.rs` | 导出 Markdown |
| `book.rs` | `.mdxb` 单文件书：`Book`/`Chapter`/`Localized`、打包、解包、资源校验 |
| `annotations.rs` | 批注 sidecar：作者、回复、锚点、CRUD/批量、合并、`@name` 扫描 |
| `wiki.rs` | 文件夹 `[[双链]]` 解析、文件索引与反向引用（不依赖 GPUI） |
| `collab.rs` | 预留实时协作协议：session/presence/op log/apply；无网络和 UI |
| `lang.rs` | 代码语言表 |
| `palette.rs` | 文字色、背景色、高亮块色板（RGBA，不依赖 GPUI；`theme.rs` 转成 GPUI 颜色） |
| `time.rs` | 日期换算、本地时区、当前时间 |
| `editor/core.rs` | 编辑内核 |
| `editor/table_ops.rs` | 表格操作和单元格选区（`CellSel`） |
| `editor/fuzz_tests.rs` | 随机编辑 fuzzer |

应用顶层（`src/`）：

| 模块 | 内容 |
|---|---|
| `main.rs` | 入口；`FDL_SCRIPT` 脚本模式（仅 `devtools` 特性）；重新导出 `fdl_core` 的模块 |
| `guides.rs` | 文档库是空的时放入内置的「开始指南」（只放一次） |
| `app.rs` | `Workspace`：两栏窗口、打开 / 新建 / 保存 / 删除、保存状态、提醒调度、文件夹工作区 |
| `workspace_fs.rs` | 文件夹扫描、目录树、模糊/内容搜索（不依赖文档库） |
| `annotations_panel.rs` | 右侧批注面板：作者、输入、列表、解决/编辑/删除、批量操作 |
| `html.rs` / `printing.rs` / `transfer.rs` | 导出 HTML / PDF、打印、导入导出（含 `.mdxb` 的树打包、Markdown 文件夹导出、文件夹打包与导入） |
| `clipboard.rs` | 系统剪贴板的 HTML 读取，以及脚本模式的模拟剪贴板 |
| `sidebar.rs` / `tree.rs` | 左栏与文档树（子文档、移动到） |
| `home.rs` / `trash.rs` | 主页、回收站 |
| `search.rs` | 搜索弹窗、高级搜索 |
| `docmenu.rs` / `docinfo.rs` / `modes.rs` | 「…」菜单、文档信息、编辑 / 阅读切换和快捷键面板 |
| `theme.rs` | 设计 token：`craft` 色阶、语义颜色 `Theme::*`、命名颜色的显示色、阴影 `shadow_*`、字阶、笔记面板尺寸 |
| `ui.rs` / `input.rs` | 共用小部件（菜单外壳、菜单行、白色药丸按钮、提示、Toast）、单行输入框 |
| `highlight.rs` | 代码高亮 |
| `assets.rs` | 编进二进制的图标、字体、封面图库 |
| `devtools.rs` | 脚本驱动，只在 `--features devtools` 构建里编入（见 tools/harness/README.md） |

编辑器（`src/editor/`）：

| 模块 | 内容 |
|---|---|
| `view.rs` | `Editor` 实体：状态、键鼠处理、`changed()` |
| `keymap.rs` / `menus.rs` | 快捷键绑定、斜杠菜单和转换菜单的数据 |
| `render.rs` / `text.rs` | 页面渲染、富文本元素 |
| `overlays.rs` | 手柄、斜杠 / + 菜单、块菜单、浮动工具栏 |
| `drag.rs` / `gap.rs` | 拖动 block、点击 block 间隙 |
| `table.rs` / `grid.rs` | 表格（界面部分；操作在 core 的 `table_ops.rs`）、分栏 |
| `image.rs` / `file.rs` / `cover.rs` | 图片、视频或文件、图标与封面 |
| `math.rs` | 公式（katex-rs 生成 HTML，再自行排版） |
| `link.rs` / `links.rs` | 链接面板与卡片；@文档 与引用列表 |
| `button.rs` / `countdown.rs` / `reminder.rs` / `subpages.rs` | 按钮、倒计时、日期提醒、子页面列表 |
| `interactive.rs` | Canvas / HTML 交互卡片；桌面原生 `quadratic` 运行时，HTML 仍由 Web 阅读器执行 |
| `home.rs` | 主页：`书架`（根文档）/ `所有页面` / `收藏`，新建与导入卡片，列表/宫格 |
| `emoji.rs` / `emoji_picker.rs` | 表情数据与面板 |
| `catalogue.rs` / `find.rs` / `reading.rs` / `scrollbar.rs` / `caption.rs` | 目录、查找替换、阅读模式与信息行、滚动条、代码块名和图片描述 |

## 书与浏览器阅读器

`.mdxb` 是 JSON 单文件（格式见 [FORMAT.md](FORMAT.md)）。桌面端 `transfer.rs` 的 `export_book` 把当前文档和 `Tree::subtree` 收集到的子页按顺序交给 `book::pack`，`import_book` 把 `Book::load` 的章节写成文档树，并把资源落到 `assets/books/<uuid>/`、改写正文里的 `src`。Windows 安装器注册 `.mdxb`；启动时 `main.rs` 从 argv 取第一本书，交给 `open_book_path` 走同一条导入并打开路径。

`web/` 是独立的无构建阅读器：`mdxb.js` 校验并归一化书，`docxml.js` / `markdown.js` 把章节转成同一套块对象，`app.js` 负责目录、章节、语言、主题和进度，`widgets.js` 挂载 Canvas / HTML 交互块。它不依赖 Rust 构建产物，也不嵌入桌面应用；两边共享的契约是 `.mdxb` 和 DocxXML，而不是代码。

## 协作服务器

`server/` 是独立的 Go module，不属于 Cargo workspace，也不嵌入桌面进程。它面向“一个团队共用一个文件夹”的场景，文档仍是 `<root>/docs/*.md`，每个被接受的保存都在 `<root>` 的 Git 仓库里形成一次提交。

| 包 | 职责 |
|---|---|
| `internal/store` | 文档 ID、读写、标题提取、临时文件 + `fsync` + `rename` 的原子保存 |
| `internal/gitstore` | 初始化仓库、提交单个文件、`log` / `show` / `diff` / `merge-file`；不把提交写进父仓库 |
| `internal/live` | 内存房间：当前内容 + revision、最近 256 个快照、presence、WebSocket 广播、三方合并与冲突 |
| `internal/api` | HTTP JSON API、Bearer token、`/ws/docs/{id}` 升级 |
| `cmd/markdownx-server` | 配置、静态编辑器、优雅退出 |
| `web` | 浏览器编辑器（纯文本编辑、版本面板、diff、冲突提示） |

一致性模型是乐观并发，不是 CRDT：客户端带 `base` revision 保存；`base` 落后时，服务器用 Git 的 `merge-file` 对 base/current/incoming 做三方合并，干净就提交，重叠就返回冲突且不写文件。revision 只在房间内存中有效，服务重启后从磁盘重新开始；持久历史以 Git 提交为准。这个边界让服务器可以独立部署，也避免把实时协议塞进 `fdl_core` 的文档模型。

`server/README.md` 记录启动参数、HTTP/WebSocket 消息、安全边界和 Docker 用法。

## 文件夹工作区与批注

`workspace_fs.rs` 只读文件系统，输出 `WorkspaceDir` / `FileNode` / `Hit`。`Workspace` 用 `file: Option<PathBuf>` 表示“正在直接编辑的 Markdown”，`folder: Option<WorkspaceDir>` 表示目录模式；`flush` 在文件模式下把文档重新导出为 Markdown 并写回源文件，在文档库模式下仍走 `store.save`。`.mdxb` 在文件夹里被点击时走 `import_book`，但不清空目录树。

批注不写进正文：core 的 `annotations.rs` 管理 JSON sidecar，`annotations_panel.rs` 渲染右侧面板。锚点是从根开始的 block 索引路径，不是 `BlockId`（DocxXML 不保存 BlockId）。多用户离线修改通过 `tools/agent/mdxb.py merge-annotations` 按 `updated` 合并。

双链与协作：

- `wiki.rs` 只认 `[[Target]]` / `[[Target|Label]]`，跳过代码围栏；`Index::build` 接收 `(path, text)`，桌面在打开文件夹、保存文件后重建。侧栏显示当前文件的出链和反链，解析按文件名 stem、相对路径或 H1 标题。
- `collab.rs` 是预留层：`Session` 维护 `revision`、`epoch`、peer presence 和 op log；`Operation` 带 `protocol`/`seq`/`base`，`apply` 返回 `Applied` / `Duplicate` / `Stale` / `Future` / `WrongProtocol`。目前没有传输层、持久化或 UI，后续接 WebSocket/CRDT 时不改文档模型。
- `annotations.rs` 的 `mentions` 扫描 `@name`（跳过邮箱），批注面板只负责高亮；正文块本身还没有 @ 自动补全。

`agent/` 与 `tools/agent/` 是给编码 agent 的入口：章节保持 `NN-topic.md`，`mdxb.py` 负责 check / pack / unpack / search / merge-annotations；它和桌面端读写同一份 `.mdxb` 契约。

## 界面语言

界面支持简体中文与英文，偏好保存在 `Preferences::english`。静态文案使用 `settings::text` / `label` 的统一映射，成对文案使用 `settings::tr`，带参数的文案使用 `localized_format!`。菜单表保留稳定的原始标签，在显示时翻译；搜索同时匹配英文标签与原有关键词。输入框保留原始 placeholder，在渲染时翻译，切换语言时刷新所有窗口。

用户标题、正文、附件名和已保存的按钮文字不作为翻译键；只有空标题等界面默认值随语言变化。Core 的存储诊断在应用层用已知模板翻译，保留路径、文档 ID 和系统错误原文，文档引擎不依赖界面语言。语言回归检查覆盖界面翻译键、菜单表、日期提醒，以及用户内容和诊断参数的保留。
