//! 更新日志: after an update, a quiet card at the foot of the sidebar lists
//! what this version changed, once. The notes are compiled in (no network);
//! a new library (first launch) never sees it.
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::prelude::*;
use gpui::{div, px, AnyElement, ClickEvent, FontWeight, Window, App};

use crate::theme::Theme;
use crate::ui::icon;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RADIUS: f32 = 12.;

/// Each version's notes: (version, [(中文, English)]). Only the running
/// version's are shown; a version without notes shows nothing.
const NOTES: &[(&str, &[(&str, &str)])] = &[
    ("0.1.46", &[
        ("Windows 支持将 MarkdownX 设为 .md 文件的默认应用，并可从设置打开系统选择页面", "Register MarkdownX as a Windows default app for .md and open the system choice page from settings"),
    ]),
    ("0.1.45", &[
        ("统一桌面与网页阅读器的中性灰阶，改善浅色与深色文字层级", "Unify desktop and web neutral colors with clearer light and dark contrast"),
        ("手动检查更新显示持久状态，支持失败重试和打开正式发布下载页", "Show persistent update results, retry failures and open stable release downloads"),
    ]),
    ("0.1.44", &[
        ("模块化互动内容可加载相对图片和媒体，分镜演示已更新", "Load module-relative images and media in interactive content, with an updated storyboard demo"),
    ]),
    ("0.1.43", &[
        ("支持互动内容中模块脚本的相对导入，并增加演示", "Support relative ES module imports in interactive content and add a demo"),
    ]),
    ("0.1.42", &[
        ("在设置中配置本机 PicGo 地址，改进上传连接处理", "Configure the local PicGo address in settings and improve upload response handling"),
    ]),
    ("0.1.41", &[
        ("支持容器包裹的画布和视频，并增加分镜演示", "Support canvas and video in HTML containers with a storyboard demo"),
    ]),
    ("0.1.40", &[
        ("网页阅读的图片、公式标题锚点与桌面保持一致", "Match web heading anchors to desktop for images and equations"),
        ("论文演示增加图片与公式标题跳转", "Add image and equation heading links to the paper demo"),
    ]),
    ("0.1.39", &[
        ("修复网页与 HTML/PDF 导出中带脚注标题的锚点及跳转", "Fix heading anchors and navigation with footnotes in web and HTML/PDF exports"),
        ("跨盘导出保留已校验的本地文件链接", "Preserve verified local file links when exporting across volumes"),
        ("论文演示增加带脚注标题和跳转链接", "Add a heading with a footnote and a navigation link to the paper demo"),
    ]),
    ("0.1.38", &[
        ("阅读模式保留带脚注标题的原始锚点链接", "Preserve original links to headings containing footnotes in reading mode"),
    ]),
    ("0.1.37", &[
        ("Markdown 阅读模式支持论文脚注跳转和返回，保留原文及编辑历史", "Read Markdown footnotes with navigation while preserving source and edit history"),
    ]),
    ("0.1.36", &[
        ("修复论文导出时交错脚注的编号顺序和返回链接", "Fix numbering and backlinks for interleaved footnotes in exported papers"),
    ]),
    ("0.1.35", &[
        ("保留粗体和引用式图片的原文，支持编辑相邻普通文字", "Preserve emphasis and reference-style images when editing adjacent plain text"),
    ]),
    ("0.1.34", &[
        ("编辑嵌套列表中与图片同行的文字时保留原始图片语法", "Preserve original inline image markup when editing nested list text"),
    ]),
    ("0.1.33", &[
        ("修复中文编号、制表符和多空格列表的嵌套层级", "Fix nested lists with Chinese numbering, tabs and multiple spaces"),
        ("编辑嵌套列表文字时保留原始图片引用与缩进", "Preserve original image references and indentation when editing nested list text"),
    ]),
    ("0.1.32", &[
        ("网页阅读保留嵌套列表与起始编号，列表中支持音视频和互动内容", "Preserve nested lists and numbering, with audio, video and interactive content in list items"),
        ("打包列表中的本地媒体和译文资源，演示增加列表示例", "Package local list media and translated resources, with new examples"),
    ]),
    ("0.1.31", &[
        ("缩进代码中的示例图片路径不再被当作真实资源，保留脚注与列表中的图片", "Keep image examples in indented code literal while packaging real footnote and list images"),
    ]),
    ("0.1.30", &[
        ("网页阅读器的 Markdown 章节支持脚注、跳转和返回链接", "Render footnotes with navigation and backlinks in Markdown book chapters"),
        ("论文演示增加脚注示例", "Add a footnote to the sample paper"),
    ]),
    ("0.1.29", &[
        ("命令行内置 HTML 和 PDF 支持脚注编号、引用与返回链接", "Render footnote references and backlinks in built-in HTML and PDF exports"),
    ]),
    ("0.1.28", &[
        ("分镜演示新增表格音视频和嵌套 source 写法，PDF 显示音频播放提示", "Add nested audio/video source examples and show audio playback notes in PDF"),
    ]),
    ("0.1.27", &[
        ("命令行自动处理论文引用并生成 HTML 或 PDF，支持浏览器和 Typst 回退", "Process paper citations in automatic HTML/PDF conversion with browser and Typst fallbacks"),
        ("Pandoc HTML 的本地链接随输出目录调整，缺失引文时保护已有文件", "Rebase local links in Pandoc HTML and protect existing files when citations are missing"),
    ]),
    ("0.1.26", &[
        ("PDF 导出显示视频封面并改善互动内容的打印背景和分页", "Show video posters in PDF and improve interactive content backgrounds and pagination"),
    ]),
    ("0.1.25", &[
        ("修复内嵌 SVG 和分镜图中的中文、镜号在预览及导出时可能丢失的问题", "Preserve labels in embedded and storyboard SVG images during preview and export"),
    ]),
    ("0.1.24", &[
        ("HTML 与 PDF 导出兼容内嵌 SVG 和 BMP，和桌面预览使用相同的图片校验", "Export embedded SVG and BMP to HTML and PDF with the same image validation as desktop preview"),
    ]),
    ("0.1.23", &[
        ("正文和表格中混排的本地图片可上传到 PicGo，保留原文格式与本地文件", "Upload local images mixed into text and tables to PicGo while preserving source formatting and local files"),
        ("支持预览 Markdown 中 Base64 内嵌图片并在导出时保留原始数据", "Preview Base64 embedded images in Markdown and preserve their data on export"),
    ]),
    ("0.1.22", &[
        ("工作区链接支持使用 #page=数字 跳到 PDF 或受支持 CAJ 的指定页面", "Workspace links can open a specified PDF or supported CAJ page with #page=number"),
        ("页码无效或超出文档范围时给出提示，保护原文件与未保存的编辑", "Report invalid or missing pages while preserving source files and unsaved edits"),
    ]),
    ("0.1.21", &[
        ("互动内容的媒体与脚本统一按文档目录查找，并限制在工作区内", "Resolve interactive media and scripts from the document directory and keep them within the workspace"),
        ("缺失或越界的本地资源会显示错误，不再静默加载", "Show an error when local interactive resources are missing or outside the workspace"),
    ]),
    ("0.1.20", &[
        ("分镜表格支持相对路径的视频和 Canvas，打包电子书时保留素材", "Use relative-path video and Canvas in storyboard tables and include their assets in books"),
        ("新增只读 DOC、Office 模板和 EPUB 预览，增强文书脚注、批注与修订提取", "Preview legacy DOC, Office templates and EPUB, with improved notes, comments and revision extraction"),
        ("增强文档压缩包校验，识别重复条目并提示缺失的关联内容", "Validate document archives and report duplicate entries or missing related content"),
    ]),
    ("0.1.19", &[
        ("Markdown 相对路径图片、表格分镜及互动内容预览得到增强，并加入演示文件", "Improved Markdown relative-path images, storyboard tables and interactive previews, with new examples"),
        ("支持只读 PDF、DOC、DOCX、OFD、EPUB 和更多论文及法律文档预览", "Read PDF, DOC, DOCX, OFD, EPUB and more research and legal documents in read-only previews"),
        ("新增命令行转换和本地图片管理，阅读内容时目录树保持清晰", "Convert documents from the command line, manage local images and keep the file tree clear while reading"),
    ]),
    ("0.1.18", &[
        ("工作区可直接打开和编辑 .xlsx、.xlsm、.xlam 表格，支持公式试算、格式、查找替换和工作表管理", "Open and edit .xlsx, .xlsm and .xlam workbooks with formulas, formats, find and replace, and worksheet management"),
        ("新增按后缀匹配的外部扩展，设置 › 扩展可逐个启停，匹配文件以只读 Markdown 预览打开", "Add suffix-matched external extensions with per-plugin switches in Settings › Extensions and read-only Markdown previews"),
        ("图片/GIF、音频、视频和 3D 模型可直接预览，共用后缀注册表", "Preview images/GIFs, audio, video and 3D models through the same suffix registry"),
        ("默认演示目录改为 MarkdownX-Demo，首次打开时附带 Markdown、Excel 和媒体示例", "Rename the default demo folder to MarkdownX-Demo and include Markdown, Excel and media samples on first launch"),
    ]),
    ("0.1.16", &[
        ("首次打开时按界面语言展示可编辑示例和入门指南", "Start with an editable sample and guides in your interface language"),
        ("目录和文件按类型显示彩色图标，搜索结果可打开其他文件", "See color-coded folders and file types, and open other files from search"),
        ("恢复最近使用的工作区文件，同时保留已有文档和旧指南", "Restore recent workspace files while preserving existing documents and guides"),
    ]),
    ("0.1.15", &[
        ("首次打开时按界面语言展示可编辑示例和入门指南", "Start with an editable sample and guides in your interface language"),
        ("示例显示在默认工作目录中，重启时恢复最近打开的工作区文件", "Find the sample in the default workspace and restore your last workspace file"),
        ("保留已有文档和旧指南，空目录可直接新建文件", "Preserve existing documents and create files directly from an empty folder"),
        ("目录和文件按类型显示彩色图标，其他文件可用系统应用打开", "See color-coded folders and file types, and open other files with a system app"),
    ]),
    ("0.1.14", &[
        ("同时打开多个工作目录，管理文件和文件夹，并在所有目录中搜索", "Open multiple workspaces, manage files and folders, and search across them"),
        ("默认打开文档目录下的 MarkdownX-Demo，支持添加和切换目录", "Open MarkdownX-Demo in Documents by default, and add or switch folders"),
        ("修复 Windows 窗口的最小化、最大化、还原和关闭操作", "Fix Windows window minimize, maximize, restore and close controls"),
    ]),
    ("0.1.13", &[
        ("Ctrl+K / ⌘K 全局搜索文档库与当前文件夹，支持路径和正文", "Use Ctrl+K / ⌘K to search the library and open folder by path or text"),
        ("新增 Windows 与 macOS 一键安装入口和发布包校验", "Add Windows and macOS one-command installers with package verification"),
    ]),
    ("0.1.12", &[
        ("空 Markdown 文件打开后可直接输入正文并自动保存", "Type into empty Markdown files immediately and save automatically"),
    ]),
    ("0.1.11", &[
        ("编辑文件时保留未修改的 Markdown 原文、换行和代码围栏", "Keep unchanged Markdown source, line endings and code fences when editing"),
        ("文件夹搜索转到后台，临时写盘失败后自动重试", "Search folders in the background and retry temporary save failures"),
        ("加强协作中新建文件和工作区路径的保护", "Protect concurrent document creation and workspace paths in collaboration"),
    ]),
    ("0.1.10", &[
        ("直接打开 Markdown 和文本文件，也可在目录中新建文件", "Open Markdown and text files directly, and create files in a folder"),
        ("文件保存和双链索引在后台处理，切换前等待最新修改保存", "Save files and index links in the background; finish saving before switching"),
        ("加强外部修改检测与 Git 暂存区保护，保留没有标题的文件", "Strengthen external-change and Git staging protection, and preserve files without a heading"),
        ("协作服务器遇到外部修改时保留两份内容，避免覆盖已有 Git 暂存", "Preserve both sides of external edits and protect existing Git staging in the collaboration server"),
    ]),
    ("0.1.9", &[
        ("文件夹工作区可显示和刷新完整目录树，支持中文路径和内容搜索", "Browse and refresh folder trees with Unicode paths and content search"),
        ("保护 Markdown 保存：检测外部修改、标题或正文丢失及资源冲突", "Protect Markdown saves against external edits, detected text loss and asset conflicts"),
        ("保留段首空格、复合格式、双链和相对链接，修复重复标题", "Preserve leading spaces, combined emphasis, wiki and relative links, and avoid duplicate titles"),
        ("保留有序列表的重新编号和表格列对齐", "Keep ordered-list restarts and table-column alignment"),
    ]),
    ("0.1.8", &[
        ("没有可用显卡时自动使用 WARP 软件渲染，Windows 安装后也能打开界面", "Fall back to WARP software rendering when no GPU is usable, so the Windows app opens after install"),
        ("新增 Windows MSI 安装包，带应用、协作服务器、模板和 .mdxb 关联", "A Windows MSI installs the app, collaboration server, templates and .mdxb association"),
        ("首次启动跟随系统语言，可选手动中文/English；自动检查 GitHub 新版本", "Follow the system language on first launch, pin Chinese/English, and check GitHub releases"),
        ("协作服务器改用 Rust，默认端口 8877，支持 Git 历史、diff、恢复和多人编辑", "The Rust collaboration server uses port 8877 and supports Git history, diff, restore and multiplayer editing"),
    ]),
    ("0.1.7", &[
        ("退出时检查所有窗口的保存状态，保存冲突支持重新载入和另存副本", "Check every window before quitting; reload or save a copy when saving fails"),
        ("退出时无法保存的内容会保留为恢复副本，下次启动恢复", "Preserve unsaved work as recovery copies when quitting and restore it on next launch"),
        ("Markdown 导出保留行内附件的链接和文件", "Markdown exports include inline attachment links and files"),
        ("导出限制本地文件的读取范围和大小，写盘失败时更可靠", "Safer local-file exports and more reliable handling of write failures"),
        ("Windows：修复小狼毫退格多删字符、回车多换行", "Windows: fix extra deletion and newlines when using the Weasel input method"),
    ]),
    ("0.1.6", &[
        ("点击空白或切回窗口后可以继续输入，回到应用时更流畅", "Keep typing after clicking blank space or returning to a window; faster library refresh"),
        ("宽表格可延伸至页边，竖向滚动不再带动表格横移", "Wide tables use the page margins and stay still during vertical scrolling"),
        ("持续输入时也会及时保存，输入时隐藏块手柄", "Save regularly during continuous typing and hide block handles while writing"),
        ("macOS：全局快速笔记默认快捷键改为 ⌥ Space", "macOS: the default global quick-note shortcut is now Option+Space"),
        ("Windows：支持粘贴截图和图片文件、导出 PDF 和打印，修复按词删除", "Windows: paste screenshots and image files, export PDF and print, and delete words correctly"),
    ]),
    ("0.1.5", &[
        ("应用更名为 MarkdownX", "The app is now called MarkdownX"),
        ("首页新增批量管理：创建副本、移动、删除", "Manage several documents at once from Home: duplicate, move, delete"),
        ("回收站可以一键清空", "Empty the Trash in one step"),
        ("插入菜单的图标可以用方向键移动", "Arrow keys move through the insert menu's icons"),
        ("删除全部文档后不再自动新建空文档", "Deleting every document no longer creates an empty one"),
        ("Windows：快捷键显示为 Ctrl / Alt，启动失败时会提示并记录日志", "Windows: shortcuts read Ctrl / Alt, and a failed launch explains itself and leaves a log"),
    ]),
];

static PENDING: AtomicBool = AtomicBool::new(false);

fn notes() -> Option<&'static [(&'static str, &'static str)]> { NOTES.iter().find(|(v, _)| *v == VERSION).map(|(_, n)| *n) }

/// At launch, once preferences and themes are loaded: a library that existed
/// before this launch and was last opened by an older version gets the card.
/// Records this version either way, so it shows only once.
pub fn startup(existed: bool) {
    let mut p = crate::settings::prefs();
    if p.seen_version == VERSION { return }
    let upgraded = existed && (p.seen_version.is_empty() || crate::update::newer(VERSION, &p.seen_version));
    PENDING.store(upgraded && notes().is_some(), Ordering::Relaxed);
    p.seen_version = VERSION.into();
    if let Err(e) = crate::settings::save(&p) { log::warn!("changelog: {e:#}"); }
}

/// The card, while it has not been closed.
pub fn card(on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Option<AnyElement> {
    if !PENDING.load(Ordering::Relaxed) { return None }
    let notes = notes()?;
    let title = crate::localized_format!("已更新到 {}", "Updated to {}", VERSION);
    let mut list = div().mt(px(6.)).w_full().min_w_0().flex().flex_col().gap(px(4.));
    for &(zh, en) in notes {
        list = list.child(div().w_full().min_w_0().flex().gap(px(6.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption())
            .child(div().flex_none().child("·")).child(div().flex_1().min_w_0().child(crate::settings::tr(zh, en))));
    }
    // A continuous-corner white surface; the faint shadow follows its radius.
    Some(crate::ui::continuous_surface(RADIUS, Theme::bg_body()).id("changelog").flex_none().min_w_0().overflow_hidden().mx(px(crate::sidebar::SB_X - 4.)).mb(px(2.)).pl(px(12.)).pr(px(6.)).pt(px(8.)).pb(px(10.))
        .rounded(px(RADIUS)).shadow(crate::theme::shadow_card())
        .child(div().w_full().min_w_0().flex().items_center().justify_between()
            .child(div().text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().id("changelog-close").size(px(20.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
                .hover(|s| s.bg(Theme::fill_hover())).child(icon("x", 12., Theme::text_caption()))
                .on_click(move |e, w, cx| { PENDING.store(false, Ordering::Relaxed); on_close(e, w, cx); })))
        .child(list)
        .into_any_element())
}
