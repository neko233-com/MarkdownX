//! 导入 / 导出: 下载为 → PDF / Markdown (with the 导出 Markdown 设置
//! dialog), 打印, and 导入为文档 (主页's 上传, the app menu): local Markdown
//! or DocxXML files picked straight away. The Word / PDF settings and
//! 打印选项 only choose what happens to comments, so they are left out.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::model::{Block, BlockKind, Cover, Document, RichText};
use crate::printing::{self, Job};
use crate::theme::Theme;
use crate::trash::{close_button, dialog, primary_button, scrim, secondary_button};
use crate::ui::ToastKind;
use fdl_core::book;
use fdl_core::docxml;
use gpui::{prelude::FluentBuilder, *};
use std::path::{Path, PathBuf};

/// The open 导出 Markdown 设置 dialog: the page, and 所有内容 picked.
pub struct ExportMd {
    pub id: String,
    pub all: bool,
}

const MARKDOWN_EXTS: [&str; 4] = ["md", "markdown", "mark", "txt"];

fn downloads() -> PathBuf {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_default()
}

/// Where to save `name`: the system save panel (Downloads first), or a
/// scripted answer.
fn save_path(name: &str, cx: &mut App) -> impl std::future::Future<Output = Option<PathBuf>> + use<> {
    let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
    let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&downloads(), Some(name)));
    async move {
        match prompt {
            Some(p) => p.await.ok().and_then(Result::ok).flatten(),
            None => picked,
        }
    }
}

/// Markdown and DocxXML files to import: the system open panel, or a
/// scripted answer.
fn open_paths(cx: &mut App) -> impl std::future::Future<Output = Option<Vec<PathBuf>>> + use<> {
    let picked = crate::devtools::take_pick();
    let extensions = Some(MARKDOWN_EXTS.iter().chain(&["xml", "mdxb"]).map(|e| SharedString::from(*e)).collect());
    let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(crate::settings::text("导入").into()), extensions }));
    async move {
        match prompt {
            Some(p) => p.await.ok().and_then(Result::ok).flatten(),
            None => picked,
        }
    }
}

/// The page title as a file name.
fn file_name(doc: &Document) -> String {
    display_title(&doc.title.text).replace(['/', ':', '\\'], "_")
}

/// A chapter file name that survives every common filesystem.
fn chapter_slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c.is_whitespace() || c == '/' || c == '\\' || c == ':' {
            out.push('-');
        }
        if out.chars().count() >= 32 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "chapter".to_string() } else { out }
}

/// The modal mask (the window dimmed) and its dialog.
fn mask(id: &'static str, dialog: impl IntoElement, close: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static, cx: &mut Context<Workspace>) -> AnyElement {
    deferred(
        div()
            .id(id)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(scrim())
            .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, _, cx| close(w, cx)))
            .child(dialog),
    )
    .with_priority(3)
    .into_any_element()
}

/// 16px radio: the accent with a white dot when checked, else a white disc
/// with the task box's rim.
pub(crate) fn radio(on: bool) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(on, |d| d.bg(Theme::primary()).child(div().size(px(6.)).rounded_full().bg(crate::theme::raised(Theme::bg_body()))))
        .when(!on, |d| d.border_1().border_color(Theme::todo_box()).bg(crate::theme::raised(Theme::bg_body())))
}

/// Percent-escapes in a Markdown link (`%20` and friends).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Relative and `file://` image links of an imported file, made absolute.
fn resolve_images(blocks: &mut [Block], base: &Path) {
    for b in blocks.iter_mut() {
        resolve_images(&mut b.children, base);
        if let BlockKind::Table(t) = &mut b.kind {
            for cell in t.rows.iter_mut().flatten() {
                resolve_images(&mut cell.blocks, base);
            }
        }
        let BlockKind::Image { src, .. } = &mut b.kind else { continue };
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
            continue;
        }
        let path = match src.strip_prefix("file://") {
            Some(p) => PathBuf::from(percent_decode(p)),
            None => base.join(percent_decode(src)),
        };
        *src = path.to_string_lossy().to_string();
    }
}

/// A `.mdxb` asset key (`assets/...`) as a path relative to the library root.
fn book_asset_src(rel: &Path, src: &str) -> String {
    format!("{}/{}", rel.to_string_lossy().replace('\\', "/"), src)
}

/// A chapter's embedded assets were written under `rel`; point its images,
/// attachments and cover at the files on disk.
fn rewrite_book_assets(doc: &mut Document, rel: &Path) {
    fn blocks(bs: &mut [Block], rel: &Path) {
        for b in bs {
            match &mut b.kind {
                BlockKind::Image { src, .. } if src.starts_with("assets/") => {
                    let next = book_asset_src(rel, src);
                    *src = next;
                }
                BlockKind::File(f) if f.src.starts_with("assets/") => {
                    let next = book_asset_src(rel, &f.src);
                    f.src = next;
                }
                BlockKind::Table(t) => {
                    for row in &mut t.rows {
                        for cell in row {
                            blocks(&mut cell.blocks, rel);
                        }
                    }
                }
                _ => {}
            }
            blocks(&mut b.children, rel);
        }
    }
    if let Some(cover) = &mut doc.cover
        && cover.src.starts_with("assets/")
    {
        cover.src = book_asset_src(rel, &cover.src);
    }
    blocks(&mut doc.blocks, rel);
}

/// A Markdown / TXT file as a page: titled after the file, its leading
/// `# 标题` dropped when it repeats the name.
pub fn markdown_document(path: &Path) -> std::io::Result<Document> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().trim().to_string();
    let off = crate::paste::Off { file: true, ..Default::default() };
    let mut blocks = crate::paste::markdown_to_blocks(text, off);
    if blocks.first().is_some_and(|b| matches!(b.kind, BlockKind::Heading { level: 1, .. }) && b.text.text.trim() == stem) {
        blocks.remove(0);
    }
    if let Some(dir) = path.parent() {
        resolve_images(&mut blocks, dir);
    }
    crate::editor::image::localize_pasted_images(&mut blocks);
    if blocks.is_empty() {
        blocks.push(Block::text(""));
    }
    Ok(Document { title: RichText::plain(stem), icon: None, cover: None, blocks })
}

/// Write a book's chapters into `store` as a page tree, and return its root id.
/// Embedded assets are unpacked under `rel` (relative to the library root), so
/// the same book can be imported repeatedly without overwriting another copy.
fn install_book(store: &mut crate::shared_store::SharedStore, root: &Path, book: book::Book, rel: PathBuf) -> anyhow::Result<String> {
    book.write_assets(root.join(&rel))?;
    let mut chapters = book.chapter_documents()?;
    if chapters.is_empty() {
        anyhow::bail!("empty book");
    }
    for chapter in &mut chapters {
        rewrite_book_assets(&mut chapter.document, &rel);
    }
    let mut root = chapters.remove(0).document;
    if !book.title.is_empty() {
        root.title = RichText::plain(book.title.primary());
    }
    if root.cover.is_none()
        && let Some(src) = &book.cover
    {
        let src = if src.starts_with("assets/") { book_asset_src(&rel, src) } else { src.clone() };
        root.cover = Some(Cover { src, x: 0., y: 0. });
    }
    let id = store.create_from(&root)?;
    for chapter in chapters {
        let child = store.create_from(&chapter.document)?;
        store.set_parent(&child, Some(&id))?;
    }
    Ok(id)
}

impl Workspace {
    pub(crate) fn close_menus(&mut self) {
        self.doc_menu = false;
        self.doc_sub = None;
        self.row_menu = None;
    }

    /// Titles of `id`'s child pages, for its 子页面列表 blocks.
    fn sub_page_titles(&self, id: &str) -> Vec<String> {
        crate::tree::Tree::new(&self.entries).children(Some(id)).iter().map(|e| e.title.clone()).collect()
    }

    /// 下载为 → Markdown: the 导出 Markdown 设置 first.
    pub fn open_export_md(&mut self, id: &str, cx: &mut Context<Self>) {
        self.close_menus();
        self.export_md = Some(ExportMd { id: id.to_string(), all: false });
        cx.notify();
    }

    /// 导出: the .md where the user picks; 所有内容 copies local images into
    /// `<name>.assets` beside it.
    fn export_markdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ExportMd { id, all }) = self.export_md.take() else { return };
        cx.notify();
        let Some(doc) = self.doc_of(&id, cx) else { return };
        let subs = self.sub_page_titles(&id);
        let path = save_path(&format!("{}.md", file_name(&doc)), cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = path.await else { return };
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("doc").to_string();
            let assets = format!("{stem}.assets");
            let out = crate::markdown::export(&doc, &crate::markdown::Options { sub_pages: &subs, assets: all.then_some(assets.as_str()), ..Default::default() });
            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let mut missing = out.missing;
            let written = std::fs::write(&path, &out.text).and_then(|_| {
                for (src, rel) in &out.files {
                    let to = dir.join(rel);
                    if let Some(p) = to.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    // A missing image only loses itself, but is reported.
                    if crate::store::export_path(&src.to_string_lossy()).ok_or_else(|| anyhow::anyhow!("Asset outside library"))
                        .and_then(|p| crate::store::read_bounded(&p, crate::store::EXPORT_FILE_LIMIT))
                        .and_then(|bytes| Ok(std::fs::write(&to, bytes)?)).is_err() {
                        missing += 1;
                    }
                }
                Ok(())
            });
            if written.is_err() {
                let _ = this.update(cx, |w, cx| w.show_toast_kind(ToastKind::Error, crate::settings::text("导出失败，请重试"), cx));
            } else if missing > 0 {
                let _ = this.update(cx, |w, cx| w.show_toast_kind(ToastKind::Error, &crate::localized_format!("已导出，但有 {missing} 个图片或附件未能复制", "Exported, but {missing} images or attachments could not be copied"), cx));
            }
        })
        .detach();
    }

    /// 打包成电子书: the current folder or Markdown file, as one `.mdxb`.
    /// Library pages keep their existing 下载为 → MarkdownX 书 path.
    pub fn package_book(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        if !self.flush(cx) {
            return;
        }
        let source = if let Some(folder) = self.folder.clone() {
            let files = folder
                .files()
                .into_iter()
                .filter(|f| {
                    crate::workspace_fs::DocKind::of(&f.path) == Some(crate::workspace_fs::DocKind::Markdown)
                        && !f.path.to_string_lossy().ends_with(".en.md")
                })
                .map(|f| f.path.clone())
                .collect::<Vec<_>>();
            Some((folder.root.clone(), folder.name.clone(), files))
        } else if let Some(file) = self.file.clone() {
            let title = file.file_stem().and_then(|s| s.to_str()).unwrap_or("book").to_string();
            Some((file.parent().map(Path::to_path_buf).unwrap_or_default(), title, vec![file]))
        } else {
            None
        };
        let Some((root, title, files)) = source else {
            if let Some(id) = self.current.clone() {
                self.export_book(&id, window, cx);
            }
            return;
        };
        let mut inputs = Vec::with_capacity(files.len());
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let source = path.with_extension("xml");
            let source_is_newer = source.metadata().ok().and_then(|m| m.modified().ok())
                >= path.metadata().ok().and_then(|m| m.modified().ok());
            let (doc, chapter) = if source_is_newer {
                match std::fs::read_to_string(&source).ok().and_then(|xml| docxml::parse(&xml).ok()) {
                    Some(doc) => {
                        let title = doc.title.text.clone();
                        (doc, title)
                    }
                    None => {
                        let (doc, _) = crate::app::parse_markdown_file(&path, &text);
                        let chapter = doc.title.text.clone();
                        (doc, chapter)
                    }
                }
            } else {
                let (doc, _) = crate::app::parse_markdown_file(&path, &text);
                let chapter = doc.title.text.clone();
                (doc, chapter)
            };
            inputs.push(book::PackInput::titled(doc, book::Localized::same(chapter)));
        }
        if inputs.is_empty() {
            self.show_toast_kind(ToastKind::Error, crate::settings::text("没有可打包的 Markdown"), cx);
            return;
        }
        let path = save_path(&format!("{title}.mdxb"), cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = path.await else { return };
            let opts = book::PackOptions {
                assets_root: Some(root),
                id: None,
                title: Some(book::Localized::same(title)),
                ..Default::default()
            };
            let saved = book::pack(inputs, &opts).and_then(|b| b.save(&path));
            let _ = this.update(cx, |w, cx| match saved {
                Ok(()) => {
                    w.toast = None;
                    cx.notify();
                }
                Err(_) => w.show_toast_kind(ToastKind::Error, crate::settings::text("打包失败，请重试"), cx),
            });
        })
        .detach();
    }

    /// 下载为 → MarkdownX 书: this page and every child page as one `.mdxb`.
    pub fn export_book(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        cx.notify();
        let Some(doc) = self.doc_of(id, cx) else { return };
        let title = self.entries.iter().find(|e| e.id == id).map(|e| e.title.clone()).unwrap_or_default();
        let name = file_name(&doc);
        let ids = crate::tree::Tree::new(&self.entries).subtree(id);
        let mut inputs = Vec::with_capacity(ids.len());
        inputs.push(book::PackInput::titled(doc, book::Localized::same(title.clone())));
        for child in ids.iter().skip(1) {
            let Some(doc) = self.doc_of(child, cx) else { continue };
            let title = self.entries.iter().find(|e| &e.id == child).map(|e| e.title.clone()).unwrap_or_default();
            inputs.push(book::PackInput::titled(doc, book::Localized::same(title)));
        }
        let path = save_path(&format!("{name}.mdxb"), cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = path.await else { return };
            let opts = book::PackOptions {
                assets_root: Some(crate::store::Store::root_dir()),
                id: None,
                title: Some(book::Localized::same(title)),
                ..Default::default()
            };
            let saved = book::pack(inputs, &opts).and_then(|b| b.save(&path));
            let _ = this.update(cx, |w, cx| match saved {
                Ok(()) => {
                    w.toast = None;
                    cx.notify();
                }
                Err(_) => w.show_toast_kind(ToastKind::Error, crate::settings::text("导出失败，请重试"), cx),
            });
        })
        .detach();
    }

    /// 下载为 → PDF: the page laid out on A4 by WebKit, saved where the user picks.
    pub fn export_pdf(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        if !printing::SUPPORTED {
            self.show_toast_kind(ToastKind::Error, crate::settings::text("当前系统不支持"), cx);
            return;
        }
        cx.notify();
        let Some(doc) = self.doc_of(id, cx) else { return };
        let subs = self.sub_page_titles(id);
        let name = file_name(&doc);
        let path = save_path(&format!("{name}.pdf"), cx);
        let view = printing::parent_of(window);
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = path.await else { return };
            let _ = this.update(cx, |w, cx| w.show_toast_kind(ToastKind::Info, crate::settings::text("正在导出文档"), cx));
            let html = crate::html::to_html(&doc, &crate::html::Options { sub_pages: &subs });
            let _ = std::fs::remove_file(&path);
            let res = printing::run(html, name, Job::Pdf(path.clone()), view, cx).await;
            let ok = matches!(res, Ok(true)) && path.is_file();
            let _ = this.update(cx, |w, cx| {
                if ok {
                    w.toast = None;
                    cx.notify();
                } else {
                    w.show_toast_kind(ToastKind::Error, crate::settings::text("导出失败，请重试"), cx);
                }
            });
        })
        .detach();
    }

    /// 打印: the system print panel over the same A4 layout as PDF.
    pub fn print_doc(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        if !printing::SUPPORTED {
            self.show_toast_kind(ToastKind::Error, crate::settings::text("当前系统不支持"), cx);
            return;
        }
        let Some(doc) = self.doc_of(id, cx) else { return };
        let subs = self.sub_page_titles(id);
        let name = file_name(&doc);
        let html = crate::html::to_html(&doc, &crate::html::Options { sub_pages: &subs });
        let view = printing::parent_of(window);
        self.show_toast_kind(ToastKind::Info, crate::settings::text("文档打印中"), cx);
        cx.spawn_in(window, async move |this, cx| {
            let res = printing::run(html, name, Job::Print, view, cx).await;
            let _ = this.update(cx, |w, cx| match res {
                Ok(_) => {
                    w.toast = None;
                    cx.notify();
                }
                Err(e) => w.show_toast_kind(ToastKind::Error, &e, cx),
            });
        })
        .detach();
    }

    /// 上传 / 导入为文档: pick Markdown or DocxXML files, each becoming a
    /// document; the last one opens.
    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        cx.notify();
        let paths = open_paths(cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = paths.await else { return };
            let _ = this.update_in(cx, |w, window, cx| w.import_files(&paths, window, cx));
        })
        .detach();
    }

    /// A `.mdxb`: unpack its chapters into the library as one page tree.
    pub(crate) fn import_book(&mut self, path: &Path) -> anyhow::Result<String> {
        let book = book::Book::load(path)?;
        let english = book.chapters.iter().map(|c| c.content_en.clone()).collect();
        let rel = PathBuf::from("assets").join("books").join(uuid::Uuid::new_v4().to_string());
        let id = install_book(&mut self.store, &crate::store::Store::root_dir(), book, rel)?;
        self.book_english = english;
        self.book_english_id = Some(id.clone());
        Ok(id)
    }

    /// 下载为 → Markdown 文件夹: every page of a book becomes one `.md`
    /// chapter, next to `book.json`. Interactive fences keep their source.
    pub fn export_markdown_folder(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        if !self.flush(cx) {
            return;
        }
        let Some(root_doc) = self.doc_of(id, cx) else { return };
        let title = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.title.clone())
            .unwrap_or_else(|| root_doc.title.text.clone());
        let ids = crate::tree::Tree::new(&self.entries).subtree(id);
        let english = if self.book_english_id.as_deref() == Some(id) {
            self.book_english.clone()
        } else {
            Vec::new()
        };
        let mut chapters = Vec::with_capacity(ids.len());
        for child in ids {
            let Some(doc) = self.doc_of(&child, cx) else { continue };
            let title = self
                .entries
                .iter()
                .find(|e| e.id == child)
                .map(|e| e.title.clone())
                .unwrap_or_else(|| doc.title.text.clone());
            chapters.push((child, title, doc));
        }
        let path = save_path(&format!("{}.md", file_name(&root_doc)), cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let Some(index_path) = path.await else { return };
            let stem = index_path.file_stem().and_then(|s| s.to_str()).unwrap_or("book").to_string();
            let parent = index_path.parent().map(Path::to_path_buf).unwrap_or_default();
            let mut folder = parent.join(&stem);
            let mut n = 2;
            while folder.exists() {
                folder = parent.join(format!("{stem}-{n}"));
                n += 1;
            }
            let library = crate::store::Store::root_dir();
            let result = (|| -> anyhow::Result<()> {
                std::fs::create_dir_all(&folder)?;
                let mut toc = format!("# {}\n\n", title.replace('\n', " "));
                let index_dir = folder
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("book")
                    .to_string();
                let mut metas = Vec::new();
                for (i, (chapter_id, chapter, doc)) in chapters.into_iter().enumerate() {
                    let stem = format!("{:02}-{}", i + 1, chapter_slug(&chapter));
                    let name = format!("{stem}.md");
                    let out = crate::markdown::export(
                        &doc,
                        &crate::markdown::Options {
                            library_root: Some(&library),
                            assets: Some("assets"),
                            ..Default::default()
                        },
                    );
                    let text = format!("# {}\n\n{}", chapter.replace('\n', " "), out.text);
                    crate::workspace_fs::write_atomic(&folder.join(&name), text.as_bytes())?;
                    // Keep the lossless DocxXML next to the Markdown chapter. A later
                    // pack prefers it only while it is at least as fresh as the .md.
                    let source = format!("{stem}.xml");
                    crate::workspace_fs::write_atomic(
                        &folder.join(&source),
                        docxml::serialize(&doc).as_bytes(),
                    )?;
                    let english_name = format!("{stem}.en.md");
                    let has_english = if let Some(Some(body)) = english.get(i) {
                        crate::workspace_fs::write_atomic(
                            &folder.join(&english_name),
                            body.as_bytes(),
                        )?;
                        true
                    } else {
                        false
                    };
                    for (src, rel) in &out.files {
                        let to = folder.join(rel);
                        if let Some(p) = to.parent() {
                            std::fs::create_dir_all(p)?;
                        }
                        let Some(path) = crate::store::export_path(&src.to_string_lossy()) else { continue };
                        let Ok(bytes) = crate::store::read_bounded(&path, crate::store::EXPORT_FILE_LIMIT) else {
                            continue;
                        };
                        std::fs::write(&to, bytes)?;
                    }
                    toc.push_str(&format!("{}. [{}]({}/{})\n", i + 1, chapter, index_dir, name));
                    let mut meta = serde_json::json!({
                        "id": chapter_id,
                        "title": { "zh": chapter, "en": chapter },
                        "file": name,
                        "source": source,
                    });
                    if has_english {
                        meta["content_en_file"] = serde_json::json!(english_name);
                    }
                    metas.push(meta);
                }
                let meta = serde_json::json!({
                    "format": "markdownx.book",
                    "version": 1,
                    "title": { "zh": title, "en": title },
                    "chapters": metas,
                    "assets": "assets",
                });
                crate::workspace_fs::write_atomic(
                    &folder.join("book.json"),
                    serde_json::to_string_pretty(&meta)?.as_bytes(),
                )?;
                crate::workspace_fs::write_atomic(&index_path, toc.as_bytes())?;
                Ok(())
            })();
            let _ = this.update(cx, |w, cx| match result {
                Ok(()) => {
                    w.toast = None;
                    cx.notify();
                }
                Err(_) => w.show_toast_kind(ToastKind::Error, crate::settings::text("导出失败，请重试"), cx),
            });
        })
        .detach();
    }

    /// A book opened by the OS (argv) enters the same path as 导入.
    pub(crate) fn open_book_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.import_files(&[path], window, cx);
    }

    fn import_files(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let again = paths.to_vec();
        if !self.save_before(cx, move |w, window, cx| w.import_files(&again, window, cx)) {
            return;
        }
        let mut last = None;
        let mut failed = false;
        for p in paths {
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
            let made = match ext.as_str() {
                e if MARKDOWN_EXTS.contains(&e) => markdown_document(p).map_err(anyhow::Error::from).and_then(|doc| self.store.create_from(&doc)),
                "xml" => self.store.import(p),
                "mdxb" => self.import_book(p),
                _ => Err(anyhow::anyhow!("文件格式不支持")),
            };
            match made {
                Ok(id) => last = Some(id),
                Err(_) => failed = true,
            }
        }
        self.entries = self.store.list();
        self.sort();
        if let Some(id) = last {
            self.open(&id, window, cx);
        }
        if failed {
            self.show_toast_kind(ToastKind::Error, crate::settings::text("导入失败，请检查文件格式"), cx);
        }
        cx.notify();
    }

    /// 导出 Markdown 设置 as a modal: white, radius 16, a 15pt semibold
    /// title over a hairline, 13pt options, the buttons on the right.
    pub(crate) fn render_export_md(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let all = self.export_md.as_ref()?.all;
        let option = |id: &'static str, on: bool, label: &'static str, value: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .cursor_pointer()
                .child(radio(on))
                .child(div().ml(px(8.)).child(label))
                .on_click(cx.listener(move |w, _, _, cx| {
                    if let Some(e) = w.export_md.as_mut() {
                        e.all = value;
                        cx.notify();
                    }
                }))
        };
        let dialog = dialog("export-md")
            .relative()
            .w(px(720.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .pt(px(24.))
                    .pb(px(24.))
                    .pl(px(24.))
                    .pr(px(56.))
                    .border_b_1()
                    .border_color(Theme::line_divider())
                    .text_size(px(15.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::settings::text("导出 Markdown 设置")),
            )
            .child(close_button("export-md-close").absolute().top(px(27.)).right(px(25.)).on_click(cx.listener(|w, _, _, cx| {
                w.export_md = None;
                cx.notify();
            })))
            .child(
                div().pt(px(24.)).px(px(24.)).pb(px(16.)).mb(px(24.)).child(
                    div()
                        .flex()
                        .items_center()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .child(div().mr(px(40.)).text_color(Theme::text_caption()).child(crate::settings::text("内容范围")))
                        .child(
                            div()
                                .flex()
                                .gap(px(24.))
                                .child(option("export-md-text", !all, crate::settings::text("仅文本（不含图片和附件）"), false))
                                .child(option("export-md-all", all, crate::settings::text("所有内容"), true)),
                        ),
                ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(24.))
                    .flex()
                    .justify_end()
                    .gap(px(12.))
                    .child(secondary_button("export-md-cancel", crate::settings::text("取消")).min_w(px(80.)).on_click(cx.listener(|w, _, _, cx| {
                        w.export_md = None;
                        cx.notify();
                    })))
                    .child(primary_button("export-md-ok", crate::settings::text("导出")).min_w(px(80.)).on_click(cx.listener(|w, _, window, cx| w.export_markdown(window, cx)))),
            );
        Some(mask(
            "export-md-mask",
            dialog,
            |w, cx| {
                w.export_md = None;
                cx.notify();
            },
            cx,
        ))
    }
}

#[cfg(feature = "devtools")]
impl Workspace {
    pub(crate) fn check_export_safety(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::model::*;
        let root = crate::store::Store::root_dir();
        assert!(std::env::var_os("FDL_SCRIPT").is_some() && std::env::var_os("MARKDOWNX_HOME").is_some());
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/report.pdf"), b"attachment-content").unwrap();
        let outside = root.parent().unwrap().join("outside-private.txt");
        std::fs::write(&outside, b"PRIVATE-MUST-NOT-EXPORT").unwrap();
        let file = FileBlock { src_kind: "path".into(), src: "assets/report.pdf".into(), name: "Report".into(), view: FileView::Card };
        let mut text = RichText::plain("Attachment: ");
        text.push("\u{fffc}", Marks { inline: Some(crate::docxml::to_inline(InlineKind::File(file))), ..Default::default() });
        let mut doc = Document { title: RichText::plain("Export safety"), blocks: vec![Block::new(BlockKind::Text, text)], ..Default::default() };
        let image = crate::docxml::parse(&format!("<img href=\"{}\"/>", outside.display())).unwrap();
        assert!(image.blocks.iter().any(|b| matches!(b.kind, BlockKind::Image { .. })));
        doc.blocks.extend(image.blocks);
        let html = crate::html::to_html(&doc, &crate::html::Options { sub_pages: &[] });
        assert!(!html.contains("PRIVATE-MUST-NOT-EXPORT"));
        assert!(!html.contains("UFJJVkFURS1NVVNULU5PVC1FWFBPUlQ="));
        assert!(!html.contains(&*outside.to_string_lossy()));
        let id = self.store.create_from(&doc).unwrap();
        self.entries = self.store.list();
        self.open(&id, window, cx);
        self.export_md = Some(ExportMd { id, all: true });
        self.export_markdown(window, cx);
        eprintln!("export-safety: HTML rejected outside file; Markdown export dispatched");
    }
}


#[cfg(test)]
mod tests {
    use super::{BlockKind, book, install_book, markdown_document, percent_decode};
    use crate::model::{Block, Document, RichText};
    use crate::shared_store::SharedStore;
    use std::path::PathBuf;

    #[test]
    fn chapter_slugs_are_filesystem_safe() {
        assert_eq!(super::chapter_slug("机械 运动/1"), "机械-运动-1");
        assert_eq!(super::chapter_slug("///"), "chapter");
        assert!(super::chapter_slug(&"长".repeat(80)).chars().count() <= 32);
    }

    #[test]
    fn installs_book_as_page_tree() {
        let dir = std::env::temp_dir().join(format!("fdl-book-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/pic.png"), b"png").unwrap();
        let mut store = SharedStore::open_at(dir.clone()).unwrap();
        let image = BlockKind::Image {
            src_kind: "path".into(),
            src: "assets/pic.png".into(),
            width: None,
            height: None,
            caption: None,
            crop: None,
        };
        let first = Document {
            title: RichText::plain("Chapter 1"),
            blocks: vec![Block::new(image, RichText::default())],
            ..Default::default()
        };
        let second = Document { title: RichText::plain("Chapter 2"), ..Default::default() };
        let book = book::pack(
            vec![book::PackInput::new(first), book::PackInput::new(second)],
            &book::PackOptions { assets_root: Some(dir.clone()), ..Default::default() },
        )
        .unwrap();
        let root = install_book(&mut store, &dir, book, PathBuf::from("assets/books/test")).unwrap();
        let entries = store.list();
        assert_eq!(entries.len(), 2);
        let child = entries.iter().find(|e| e.id != root).unwrap();
        assert_eq!(child.meta.parent.as_deref(), Some(root.as_str()));
        let doc = store.load(&root).unwrap();
        match &doc.blocks[0].kind {
            BlockKind::Image { src, .. } => {
                assert_eq!(src, "assets/books/test/assets/pic.png");
                assert!(dir.join(src).is_file());
            }
            other => panic!("expected image, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_markdown_file() {
        let dir = std::env::temp_dir().join(format!("fdl-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("周报.md");
        std::fs::write(&path, "\u{feff}# 周报\n\n第一段\n\n- a\n- b\n\n![图](https://e.com/a.png)\n\n## 小结\n").unwrap();
        let doc = markdown_document(&path).unwrap();
        assert_eq!(doc.title.text, "周报");
        let kinds: Vec<_> = doc.blocks.iter().map(|b| std::mem::discriminant(&b.kind)).collect();
        assert_eq!(kinds.len(), 5);
        assert_eq!(doc.blocks[0].text.text, "第一段");
        assert!(matches!(doc.blocks[1].kind, BlockKind::Bullet));
        assert!(matches!(&doc.blocks[3].kind, BlockKind::Image { src, .. } if src == "https://e.com/a.png"));
        assert!(matches!(doc.blocks[4].kind, BlockKind::Heading { level: 2, .. }));
        assert_eq!(percent_decode("a%20b%E5%9B%BE.png"), "a b图.png");
        let _ = std::fs::remove_dir_all(dir);
    }
}
