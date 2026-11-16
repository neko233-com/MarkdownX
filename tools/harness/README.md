# 脚本驱动（harness）

用 `FDL_SCRIPT` 回放一份逐行脚本来操作窗口，并截图。整个过程不碰用户的文档库：每次运行都先把 `lib_dir` 复制一份到 `work_dir/lib`，再用 `MARKDOWNX_HOME` 指向这份副本。

```sh
cargo build --release --features devtools   # 正式构建不带脚本驱动
python3 tools/harness/run.py tools/harness/examples/scroll.txt            # 默认用 lib_clean，结果在 /tmp/fdl-run
python3 tools/harness/run.py my.txt /path/to/lib /tmp/my-run              # 自定义文档库和输出目录
FDL_BIN=/other/markdownx python3 tools/harness/run.py my.txt
```

- 截图：`<work_dir>/<name>.png`。
- 日志：`<work_dir>/log`。性能数字用 `grep -a bench log` 查看。
- `lib_clean/` 是一个审查用的文档库，含一篇「全部 Block 样式审查」和几张示例图片。
- `winid.swift` 列出屏幕上 markdownx 窗口的 window id（`swift winid.swift`），用来配合系统截图。

## 截图回归

```sh
python3 tools/harness/compare.py tools/harness/examples/menus.txt <旧版二进制> target/release/markdownx
```

- 用同一份脚本分别跑两个版本，逐张逐像素比较截图。有差异时输出差异像素数和范围，并把差异图写到 `/tmp/fdl-compare/diff-<名字>.png`。
- 只移动代码、不改外观的重构（拆模块、换 token、共用组件），应当每张都是 `identical`。
- `examples/menus.txt` 依次打开「…」菜单和页宽子菜单、编辑模式菜单、斜杠菜单、块菜单、侧栏右键菜单、帮助菜单。
- `python3 tools/harness/table_resize.py` 在临时文档库中验证调整表格列宽时页面位置不跳动，覆盖屏幕外光标、单元格内光标、无变化拖拽、保存和撤销重做；需要 Pillow。
- 比较时光标不闪烁（`FDL_NOBLINK`）。脚本开头要先把指针移到空白处，否则真实鼠标的位置会以 hover 的形式出现在截图里。
- 旧版二进制可以这样编：`git worktree add /tmp/old <提交>`，在那里用 `CARGO_TARGET_DIR=<本仓库>/target cargo build --release --features devtools`，再把二进制复制出来。
- `python3 tools/harness/global_search.py` 用临时文档库和目录，验证全局搜索的键盘打开、正文匹配、结果选择以及链接和旧搜索快捷键。
- `python3 tools/harness/workspace_startup.py` 用隔离目录验证多目录并存、重启恢复、跨目录搜索、保存和文件管理，以及不可用目录的保护；不会读取真实文档目录。
- `python3 tools/harness/onboarding.py` 在隔离目录验证中英文首次启动、示例文件与目录树、重启和切换语言、已有文件保护、旧版未改/已改指南及启动参数。
- `python3 tools/harness/feishu_interactive.py` 使用模拟飞书 CLI、临时工作目录和屏幕外窗口验证在线导入及单位圆、抛体画布控件；不会连接真实飞书账号。
- `python tools/harness/pdf_preview.py -Quiet` 在隔离目录验证 PDF 的只读预览、缩放、滚动、键盘翻页、中英文按页查找、无文字页面和 CAJ 内存转换路径，并检查源文件不变。
- `python tools/harness/pdf_attachment.py -Quiet` 在隔离工作区点击 DocxXML 卡片和行内 PDF 附件，验证相对路径及只读预览。
- `python3 tools/harness/spreadsheet_number_format.py` 使用隔离工作区和屏幕外窗口验证数字格式应用、撤销、重做、隐藏公式、单元格样式与 openpyxl 兼容性；需先构建 `devtools` 版本并安装 openpyxl。
- `python3 tools/harness/spreadsheet_conditional.py` 在隔离桌面窗口中验证条件格式、公式重算、撤销、大区域保护和原文件兼容性；需先构建 `devtools` 版本并安装 openpyxl。
- `python3 tools/harness/book_languages.py` 在隔离库中验证双语书导入、重启、`.mdxb` 导出和 Markdown 文件夹重打包。
- `python tools/harness/book_xml_resources.py -Quiet` 在隔离工作区验证通过桌面界面打包 DocxXML 正文、译文、封面和相对附件。
- `python tools/harness/inline_model.py -Quiet` 在隔离工作区和屏幕外窗口验证 Markdown 内模型加载、拖动旋转、缩放、重置及原文件不被修改。
- `python tools/harness/indented_code.py -Quiet` 在隔离工作区验证缩进代码示例不读取图片，真实相对图片可打开且源文件不变。
- `python tools/harness/nested_list_save.py -Quiet` 在隔离工作区编辑中文编号子项，验证自动保存保留缩进、原始图片引用及换行。
- `python tools/harness/list_media.py -Quiet` 在隔离窗口中播放列表内音视频和 Canvas，验证相对路径与源文件保护。
- `python tools/harness/media_sources.py -Quiet` 在隔离窗口验证表格内嵌套音视频资源及两种相对路径的实际播放，并检查源文件不变。
- `python tools/harness/picgo_settings.py -Quiet` 在隔离窗口验证图床地址设置、重启后上传和原图保护。
- `python tools/harness/image_host.py -Quiet` 在隔离工作区和屏幕外窗口验证本地图片存储、相对链接以及向本机模拟 PicGo 服务的主动上传。
- `python tools/harness/html_images.py -Quiet` 在隔离工作区验证 Markdown 中独立及表格混排的 HTML 图片实际加载，并检查源文件不变。
- `python tools/harness/rich_title.py -Quiet` 验证 Markdown 一级标题中的图片和公式加载，并保护原始文件。
- `python tools/harness/embedded_images.py -Quiet` 验证内嵌 PNG、SVG 和表格图片的桌面显示、SVG 外部引用隔离及临时缓存清理。
- `python tools/harness/relative_links.py -Quiet` 在隔离工作区验证相对文件导航、切换前保存、PDF 预览和工作区边界。
- `python tools/harness/math_resources.py -Quiet` 验证公式中的图片和引用写法不会被误打包，真实相对图片仍能嵌入。
- `python tools/harness/display_math.py -Quiet` 在隔离窗口验证独立公式布局、编辑自动保存和 HTML 导出。
- `python tools/harness/cli_conversion.py -Quiet` 用真实 CLI 和本机浏览器验证 HTML/PDF 导出、相对资源及失败时的输出保护。
- `python tools/harness/cli_office.py -Quiet` 在本机 Pandoc 可用时验证 DOCX/ODT/RTF/EPUB、参考文献、图片和样式模板。
- `python tools/harness/create_demo.py -Quiet` 在隔离目录验证手动创建演示文件夹、打开示例和保护已有同名目录。
- `python tools/harness/caj_preview.py <CAJ 样本> -Quiet` 用单独提供的文件验证只读预览与原文件完整性；测试副本位于临时目录。
- `python tools/harness/caj_page_link.py -Quiet` 验证相对页码链接可打开 PDF 数据包装的 CAJ 并跳到指定页，不修改原件。
- `python tools/harness/inline_html.py -Quiet` 在 Windows 屏幕外窗口验证 Canvas 点击、视频播放、路径与网络隔离、菜单遮挡、滚动释放和控件定位。
- `python tools/harness/inline_image_host.py -Quiet` 在隔离窗口验证混排图片的工具栏上传、失败重试、表格与引用格式保留及并发编辑保护。
- `python tools/harness/inline_html_boundary.py -Quiet` 验证互动资源越过工作区或缺失时显示错误，不运行控件或改动源文件。
- `python tools/harness/module_widget.py -Quiet` 验证内联及外置 ES 模块的相对依赖、点击和源文件保护。
- `python tools/harness/wrapped_widget.py -Quiet` 在隔离窗口验证容器内 Canvas、相对脚本与样式及源文件保护。
- `python tools/harness/inline_css.py -Quiet` 在屏幕外窗口验证外置 CSS 的相对图片、嵌套样式及 Canvas 点击。
- `python tools/harness/research_preview.py -Quiet` 验证 OFD、DOCX、RTF 与 Quarto 源文件的只读文本预览不会修改正文或原文件。
- `python tools/harness/docx_layout.py -Quiet` 在隔离桌面窗口验证 DOCX 只读版式、表格和图片，不修改源文件。
- `python tools/harness/docx_revisions.py -Quiet` 检查 DOCX 插入和删除修订在版式及文本预览中均保留。
- `python tools/harness/ofd_layout.py -Quiet` 在隔离桌面窗口验证 OFD 双页绘制、翻页、签名提示和源文件只读保护。
- `python tools/harness/legacy_doc.py -Quiet` 验证旧版 Word 文件通过受限子进程只读预览，且不改动原件。
- `python tools/harness/office_templates.py -Quiet` 验证 Office 宏文档及模板的只读预览，使用本地合成样本，不执行宏。
- `python tools/harness/epub_preview.py -Quiet` 在隔离桌面窗口验证 EPUB 章节、图片、公式及原文件只读保护。
- Windows 上运行 `python tools/harness/window_controls.py`，在隔离窗口和临时文档库中验证标题栏命中、最小化、最大化/还原、全屏控件、关闭时保存和外部修改冲突保护。

## 脚本命令

每行一条命令；`#` 开头的是注释。完整说明见 `src/devtools.rs` 顶部。

| 命令 | 作用 |
|---|---|
| `wait <ms>` | 等待，期间自己绘制帧（锁屏时也能出图） |
| `key <按键>` | 发送按键，如 `key cmd-b`、`key escape` |
| `type <文字>` | 输入文字（行尾空格有效） |
| `move x y` / `click x y` / `dclick x y` / `rclick x y` / `drag x1 y1 x2 y2` | 鼠标操作（窗口坐标） |
| `wheel x y dy [dx]` | 一次滚轮事件；可选的 `dx` 是横向分量（模拟触控板），正数露出右边的内容 |
| `shot <名字>` / `burst <名字> n` | 截图；连拍 n 张，间隔约 8ms |
| `mark <文字>` / `commit <文字>` | 输入法组合文字 / 提交 |
| `clip html|text <文件>` | 设置模拟剪贴板的内容，之后的 `key cmd-v` 粘贴它 |
| `pick <路径>[|<路径>…]` | 预先回答下一次打开 / 保存对话框，不弹出系统面板 |
| `resize w h` | 调整窗口大小 |
| `place x y` | 把窗口左上角放到主屏幕的 (x, y)，浮在其它窗口上（配合 `FDL_NOPARK` 拍桌面截图） |
| `deskshot <名字> [边距]` | 连同窗口下面的内容截图，四周多留「边距」pt（默认 48）。窗口下面先用 `shared-mock/guides/backdrop.swift` 铺一张壁纸，见 `shared-mock/guides/shots.sh` |
| `bench <n> <命令>` | 把一条命令跑 n 次，每次同步画一帧，输出耗时 |
| `record <文件.mp4>` / `record stop` / `glide` / `scroll` / `caption` | 录制演示视频 |
| `open-folder` | 触发打开文件夹操作，配合 `pick` 使用 |
| `create-demo` | 创建演示文件夹，配合 `pick` 指定其父目录 |
| `open-file` / `new-folder-file` | 触发打开文件 / 在目录中新建文件，配合 `pick` 使用 |
| `check-folder-state <目录>` | 只读检查已打开的目录、侧栏和首个文件 |
| `check-workspace-startup <阶段> <目录>` | 检查临时目录中的默认、选择、恢复和启动参数覆盖行为 |
| `check-onboarding <阶段>` | 在带标记的临时目录验证首开示例、目录树和已有内容 |
| `check-multi-workspace <阶段>` | 在带标记的临时目录中检查多目录、保存、搜索和文件管理 |
| `window-native <动作>` / `window-client <动作>` / `check-window-close <阶段>` | Windows 隔离窗口专用：验证原生及全屏标题栏消息和关闭时的保存保护 |
| `check-global-search <阶段> <临时目录>` | 在带 `.markdownx-global-search-test` 标记的临时目录中检查全局搜索、键盘选择、链接快捷键和原有搜索 |
| `check-folder-safety <阶段> <临时目录>` | 依次检查后台保存、切换、外部冲突及恢复；包含 `begin` / `conflict` / `recover` / `finish`、`empty-begin` / `empty-done`、`picker` / `duplicate` / `picked` 和 `search-begin` / `search-done`；各阶段间等待异步操作完成，临时目录须有 `.markdownx-folder-test` 标记 |
| `activate` | 让窗口成为 key window。**会抢焦点，一般不要用** |
| `quit` | 退出（`run.py` 会自动在末尾追加） |

环境变量：

- `FDL_SHOTS`：截图目录（`run.py` 已设好）；
- `FDL_TRACE`：输出事件跟踪；
- `FDL_NOPARK`：不把窗口挪到屏幕外；
- `FDL_WINDOW_CAPTURE`：改用窗口服务器截图，截图里带系统画的红绿灯；
- `FDL_NOBLINK`：光标不闪烁（`compare.py` 已设好）。
- `FDL_RECORD_FPS`：录屏帧率，默认 30。

窗口内截图命令目前只支持 macOS；Windows 可用原生窗口截图工具检查实际渲染。
Windows 的脚本窗口默认放在屏幕之外，避免与用户操作相互干扰；需要人工观察时才使用 `FDL_NOPARK`。

## 使用规则

脚本跑在用户正在使用的电脑上，必须遵守以下规则：

- 不用真实的 ⌘C / ⌘V 系统剪贴板；`key cmd-v` 只在 `clip` 之后使用。
- 不抢焦点：不用 `activate`。窗口默认被挪到屏幕外渲染。
- 不点：
  - 设置 › 文档库的「在访达中显示」按钮；
  - 链接、「网页搜索」、打开链接类按钮（会启动浏览器）；
  - 打印（会弹出系统打印面板）；
  - 快速查看 / `qlmanage -p`。
- 不用 `open` 命令。
- 需要选文件时，用 `pick a|b` 代替系统对话框。
- 不要拿用户的真实文档库当 `lib_dir`。确实需要时只读复制一份，产物里也不要提交用户的内容。

- `python tools/harness/update_checks.py -Quiet` 在隔离桌面库中通过回环 HTTP 验证手动检查、重复请求保护、关闭自动检查后的手动操作、正式版本、网络失败重试及浅深色切换；正式构建不包含测试服务入口。
