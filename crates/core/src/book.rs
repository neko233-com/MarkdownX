//! MarkdownX Book (`.mdxb`): a single-file, self-contained interactive book.
//!
//! v1 is plain UTF-8 JSON. Chapters are DocxXML or Markdown, assets are
//! base64, and widgets use the [`BlockKind::Interactive`] block. Keeping the
//! container JSON (rather than ZIP) lets the desktop app and the browser
//! reader open the same file without a decompression dependency.

use crate::model::*;
use crate::paste::{self, Off};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORMAT: &str = "markdownx.book";
/// Version 2 permits a translated chapter to use a different source format.
/// Readers must reject it rather than interpreting Markdown as DocxXML.
pub const VERSION: u32 = 2;
pub const MIME: &str = "application/vnd.markdownx.book+json";
pub const EXTENSION: &str = "mdxb";

const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_ASSET_BYTES: u64 = 64 * 1024 * 1024;

/// Text that exists in Chinese and English. Either side may be empty; readers
/// fall back to the other one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localized {
    #[serde(default)]
    pub zh: String,
    #[serde(default)]
    pub en: String,
}

impl Localized {
    pub fn new(zh: impl Into<String>, en: impl Into<String>) -> Self {
        Localized { zh: zh.into(), en: en.into() }
    }

    pub fn same(s: impl Into<String>) -> Self {
        let s = s.into();
        Localized { zh: s.clone(), en: s }
    }

    pub fn is_empty(&self) -> bool {
        self.zh.trim().is_empty() && self.en.trim().is_empty()
    }

    /// The best text for `lang` (`"zh"`, `"zh-CN"`, `"en"`, ...).
    pub fn get(&self, lang: &str) -> &str {
        let first = if lang.to_ascii_lowercase().starts_with("zh") { &self.zh } else { &self.en };
        if !first.trim().is_empty() {
            return first;
        }
        if lang.to_ascii_lowercase().starts_with("zh") {
            &self.en
        } else {
            &self.zh
        }
    }

    pub fn primary(&self) -> &str {
        if !self.zh.trim().is_empty() { &self.zh } else { &self.en }
    }

    fn clean(mut self) -> Self {
        self.zh = self.zh.trim().to_string();
        self.en = self.en.trim().to_string();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Empty: use the reader's light/dark default.
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub text: String,
    /// `serif` or `sans`.
    #[serde(default = "default_font")]
    pub font: String,
}

fn default_accent() -> String {
    "#3B82F6".into()
}

fn default_font() -> String {
    "serif".into()
}

impl Default for Theme {
    fn default() -> Self {
        Theme { accent: default_accent(), background: String::new(), text: String::new(), font: default_font() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    /// `scroll` for now; `page` is reserved.
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default = "default_width")]
    pub max_width: u32,
    #[serde(default = "default_true")]
    pub show_toc: bool,
}

fn default_layout() -> String {
    "scroll".into()
}

fn default_width() -> u32 {
    760
}

fn default_true() -> bool {
    true
}

impl Default for Reading {
    fn default() -> Self {
        Reading { layout: default_layout(), max_width: default_width(), show_toc: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// e.g. `image/png`.
    #[serde(default)]
    pub mime: String,
    /// Standard base64, no data-URI prefix.
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    #[serde(default)]
    pub title: Localized,
    /// `docxxml` or `markdown`.
    pub format: String,
    pub content: String,
    /// Optional English body of the same chapter. The web reader shows it when
    /// the reader picks English; the desktop library keeps the primary `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_en: Option<String>,
    /// If absent, the translation uses `format`, as in version 1 books.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_en: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: Localized,
    #[serde(default)]
    pub subtitle: Localized,
    #[serde(default)]
    pub authors: Vec<String>,
    /// BCP-47-ish tags the book carries, e.g. `["zh", "en"]`.
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub default_language: String,
    /// An asset key (`assets/cover.jpg`) or an https URL.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub reading: Reading,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub modified: u64,
    #[serde(default)]
    pub generator: String,
}

/// One document to put into a book, plus its display title.
pub struct PackInput {
    pub document: Document,
    pub title: Localized,
}

impl PackInput {
    pub fn new(document: Document) -> Self {
        let title = Localized::same(document.title.text.trim());
        PackInput { document, title }
    }

    pub fn titled(document: Document, title: Localized) -> Self {
        PackInput { document, title }
    }
}

pub struct PackOptions {
    /// Library root used to resolve relative `path` image/file sources.
    pub assets_root: Option<PathBuf>,
    pub id: Option<String>,
    pub title: Option<Localized>,
    pub max_asset_bytes: u64,
}

impl Default for PackOptions {
    fn default() -> Self {
        PackOptions { assets_root: None, id: None, title: None, max_asset_bytes: MAX_ASSET_BYTES }
    }
}

/// Pack documents into one book. Local assets are embedded and their sources
/// rewritten to `assets/...` keys with `src_kind = "src"`.
pub fn pack(inputs: Vec<PackInput>, opts: &PackOptions) -> Result<Book> {
    pack_with_assets(inputs, opts, BTreeMap::new())
}

/// Pack edited chapters while retaining assets referenced by translations.
pub fn pack_with_assets(inputs: Vec<PackInput>, opts: &PackOptions, assets: BTreeMap<String, Asset>) -> Result<Book> {
    if inputs.is_empty() {
        bail!("a book needs at least one chapter");
    }
    let mut packer = Packer::new(opts.assets_root.clone(), opts.max_asset_bytes, assets);
    let mut chapters = Vec::with_capacity(inputs.len());
    let mut first_title = Localized::default();
    for (n, input) in inputs.into_iter().enumerate() {
        let PackInput { mut document, title } = input;
        let title = title.clean();
        let mut title = if title.is_empty() {
            let t = document.title.text.trim();
            if t.is_empty() { Localized::new("未命名", "Untitled") } else { Localized::same(t) }
        } else {
            title
        };
        if title.is_empty() {
            title = Localized::new(format!("第 {} 章", n + 1), format!("Chapter {}", n + 1));
        }
        if first_title.is_empty() {
            first_title = title.clone();
        }
        collect_document(&mut document, &mut packer)?;
        chapters.push(Chapter {
            id: format!("ch-{}", n + 1),
            title,
            format: "docxxml".into(),
            content: crate::docxml::serialize(&document),
            content_en: None,
            format_en: None,
        });
    }
    let title = opts.title.clone().unwrap_or(first_title).clean();
    let title = if title.is_empty() { Localized::new("未命名", "Untitled") } else { title };
    let now = now_ms();
    let book = Book {
        format: FORMAT.into(),
        version: 1,
        id: opts.id.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        title,
        subtitle: Localized::default(),
        authors: vec![],
        languages: vec!["zh".into(), "en".into()],
        default_language: "zh".into(),
        cover: None,
        theme: Theme::default(),
        reading: Reading::default(),
        chapters,
        assets: packer.assets,
        created: now,
        modified: now,
        generator: format!("MarkdownX {}", env!("CARGO_PKG_VERSION")),
    };
    book.validate()?;
    Ok(book)
}

/// A chapter parsed back into the desktop document model.
pub struct ChapterDocument {
    pub id: String,
    pub title: Localized,
    pub document: Document,
}

impl Book {
    pub fn parse(bytes: &[u8]) -> Result<Book> {
        let book: Book = serde_json::from_slice(bytes).context("not a MarkdownX book (invalid JSON)")?;
        book.validate()?;
        Ok(book)
    }

    pub fn from_json(s: &str) -> Result<Book> {
        Self::parse(s.as_bytes())
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Book> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        Book::parse(&bytes).with_context(|| format!("open {}", path.display()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let json = self.to_json()?;
        std::fs::write(path, json).with_context(|| format!("write {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!("not a MarkdownX book (format is {:?})", self.format);
        }
        if self.version == 0 || self.version > VERSION {
            bail!("unsupported MarkdownX book version {}", self.version);
        }
        if self.chapters.is_empty() {
            bail!("a MarkdownX book needs at least one chapter");
        }
        let mut ids = HashSet::new();
        for (n, ch) in self.chapters.iter().enumerate() {
            if ch.id.trim().is_empty() {
                bail!("chapter {} has no id", n + 1);
            }
            if !ids.insert(ch.id.as_str()) {
                bail!("duplicate chapter id {:?}", ch.id);
            }
            if ch.content.trim().is_empty() {
                bail!("chapter {:?} is empty", ch.id);
            }
            match ch.format.as_str() {
                "docxxml" => {
                    crate::docxml::parse(&ch.content).with_context(|| format!("chapter {:?} is not valid DocxXML", ch.id))?;
                }
                "markdown" => {}
                other => bail!("chapter {:?} has unknown format {:?}", ch.id, other),
            }
            if let Some(format) = &ch.format_en {
                if self.version < 2 { bail!("chapter {:?} needs book version 2 for format_en", ch.id); }
                if !matches!(format.as_str(), "docxxml" | "markdown") {
                    bail!("chapter {:?} has unknown translation format {:?}", ch.id, format);
                }
            }
            if let Some(content) = &ch.content_en
                && ch.format_en.as_deref().unwrap_or(&ch.format) == "docxxml"
                && !content.trim().is_empty()
            {
                crate::docxml::parse(content).with_context(|| format!("English chapter {:?} is not valid DocxXML", ch.id))?;
            }
        }
        let mut total = 0u64;
        for (key, asset) in &self.assets {
            if !asset_key_ok(key) {
                bail!("unsafe asset key {:?}", key);
            }
            let bytes = B64.decode(&asset.data).with_context(|| format!("asset {:?} is not base64", key))?;
            total = total.saturating_add(bytes.len() as u64);
        }
        if total > MAX_TOTAL_ASSET_BYTES {
            bail!("book assets are larger than {} MB", MAX_TOTAL_ASSET_BYTES / 1024 / 1024);
        }
        if let Some(cover) = &self.cover
            && !cover.is_empty()
            && !cover.starts_with("http://")
            && !cover.starts_with("https://")
            && !self.assets.contains_key(cover)
        {
            bail!("cover {:?} is not an embedded asset", cover);
        }
        Ok(())
    }

    /// Write every embedded asset under `root`, keeping its `assets/...` key.
    /// Returns the number of files written.
    pub fn write_assets(&self, root: impl AsRef<Path>) -> Result<usize> {
        write_assets(root, &self.assets)
    }

    /// Parse each chapter into a [`Document`]. Asset sources are rewritten to
    /// `path` + the same `assets/...` key, so call [`Book::write_assets`] with
    /// the same root before opening them.
    pub fn chapter_documents(&self) -> Result<Vec<ChapterDocument>> {
        let mut out = Vec::with_capacity(self.chapters.len());
        for ch in &self.chapters {
            let mut document = match ch.format.as_str() {
                "docxxml" => crate::docxml::parse(&ch.content)?,
                "markdown" => {
                    let blocks = paste::markdown_to_blocks(&ch.content, Off { file: true, ..Default::default() });
                    let title = if ch.title.is_empty() { RichText::default() } else { RichText::plain(ch.title.primary()) };
                    Document { title, blocks, ..Default::default() }
                }
                other => bail!("chapter {:?} has unknown format {:?}", ch.id, other),
            };
            if document.title.text.trim().is_empty() && !ch.title.is_empty() {
                document.title = RichText::plain(ch.title.primary());
            }
            prepare_asset_paths(&mut document, &self.assets);
            out.push(ChapterDocument { id: ch.id.clone(), title: ch.title.clone(), document });
        }
        Ok(out)
    }

    pub fn data_uri(&self, key: &str) -> Option<String> {
        let asset = self.assets.get(key)?;
        let mime = if asset.mime.is_empty() { "application/octet-stream" } else { &asset.mime };
        Some(format!("data:{mime};base64,{}", asset.data))
    }
}

pub fn write_assets(root: impl AsRef<Path>, assets: &BTreeMap<String, Asset>) -> Result<usize> {
    let root = root.as_ref();
    for (key, asset) in assets {
        if !asset_key_ok(key) { bail!("unsafe asset key {:?}", key); }
        let path = root.join(key);
        if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?; }
        let bytes = B64.decode(&asset.data).with_context(|| format!("asset {:?} is not base64", key))?;
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(assets.len())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Only relative keys under `assets/`, with no traversal or drive prefix.
fn asset_key_ok(key: &str) -> bool {
    key.starts_with("assets/")
        && !key.contains("..")
        && !key.contains('\\')
        && !key.contains(':')
        && !key.chars().any(char::is_control)
        && Path::new(key).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

struct Packer {
    root: Option<PathBuf>,
    root_canon: Option<PathBuf>,
    max: u64,
    assets: BTreeMap<String, Asset>,
    seen: BTreeMap<PathBuf, String>,
}

impl Packer {
    fn new(root: Option<PathBuf>, max: u64, assets: BTreeMap<String, Asset>) -> Self {
        let root_canon = root.as_ref().and_then(|r| r.canonicalize().ok());
        Packer { root, root_canon, max, assets, seen: BTreeMap::new() }
    }

    fn embed(&mut self, src_kind: &mut String, src: &mut String) -> Result<()> {
        if src_kind == "token" || src.trim().is_empty() {
            return Ok(());
        }
        if self.embed_src(src)? {
            *src_kind = "src".into();
        }
        Ok(())
    }

    fn embed_src(&mut self, src: &mut String) -> Result<bool> {
        let s = src.trim();
        if s.is_empty() || s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:") {
            return Ok(false);
        }
        let Some(root) = self.root.as_ref() else { return Ok(false) };
        let path = if Path::new(s).is_absolute() { PathBuf::from(s) } else { root.join(s) };
        let canon = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };
        if let Some(root_canon) = &self.root_canon
            && !canon.starts_with(root_canon)
        {
            return Ok(false);
        }
        if let Some(key) = self.seen.get(&canon) {
            *src = key.clone();
            return Ok(true);
        }
        let bytes = match std::fs::read(&canon) {
            Ok(b) => b,
            Err(_) => return Ok(false),
        };
        if bytes.len() as u64 > self.max {
            bail!("asset {} is larger than {} MB", canon.display(), self.max / 1024 / 1024);
        }
        let encoded = B64.encode(&bytes);
        let stem = canon.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
        let ext = canon.extension().and_then(|s| s.to_str()).unwrap_or("bin");
        let mime = mime_of(ext);
        if let Some((key, _)) = self.assets.iter().find(|(key, a)| a.data == encoded && (a.mime == mime || canon.ends_with(Path::new(key)))) {
            let key = key.clone();
            self.seen.insert(canon, key.clone());
            *src = key;
            return Ok(true);
        }
        let key = self.unique_key(stem, ext);
        self.assets.insert(
            key.clone(),
            Asset { mime: mime.into(), data: encoded },
        );
        self.seen.insert(canon, key.clone());
        *src = key;
        Ok(true)
    }

    fn unique_key(&self, stem: &str, ext: &str) -> String {
        let base = slug(stem);
        let ext = safe_ext(ext);
        let mut key = format!("assets/{base}.{ext}");
        let mut n = 1;
        while self.assets.contains_key(&key) {
            n += 1;
            key = format!("assets/{base}-{n}.{ext}");
        }
        key
    }
}

fn collect_document(doc: &mut Document, packer: &mut Packer) -> Result<()> {
    if let Some(cover) = &mut doc.cover {
        packer.embed_src(&mut cover.src)?;
    }
    collect_blocks(&mut doc.blocks, packer)
}

fn collect_blocks(blocks: &mut [Block], packer: &mut Packer) -> Result<()> {
    for b in blocks {
        for run in &mut b.text.runs {
            if let Some(inline) = &mut run.marks.inline
                && let InlineKind::File(file) = &inline.kind
            {
                let mut file = file.clone();
                let old = file.clone();
                packer.embed(&mut file.src_kind, &mut file.src)?;
                if file != old { crate::docxml::remap_inline_file(inline, file)?; }
            }
        }
        match &mut b.kind {
            BlockKind::Image { src_kind, src, .. } => packer.embed(src_kind, src)?,
            BlockKind::File(f) => packer.embed(&mut f.src_kind, &mut f.src)?,
            BlockKind::Table(t) => {
                for row in &mut t.rows {
                    for cell in row {
                        collect_blocks(&mut cell.blocks, packer)?;
                    }
                }
            }
            _ => {}
        }
        collect_blocks(&mut b.children, packer)?;
    }
    Ok(())
}

fn prepare_asset_paths(doc: &mut Document, assets: &BTreeMap<String, Asset>) {
    fn blocks(bs: &mut [Block], assets: &BTreeMap<String, Asset>) {
        for b in bs {
            match &mut b.kind {
                BlockKind::Image { src_kind, src, .. } if assets.contains_key(src.as_str()) => {
                    *src_kind = "path".into();
                }
                BlockKind::File(f) if assets.contains_key(f.src.as_str()) => {
                    f.src_kind = "path".into();
                }
                BlockKind::Table(t) => {
                    for row in &mut t.rows {
                        for cell in row {
                            blocks(&mut cell.blocks, assets);
                        }
                    }
                }
                _ => {}
            }
            blocks(&mut b.children, assets);
        }
    }
    blocks(&mut doc.blocks, assets);
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "asset".into() } else { out }
}

fn safe_ext(ext: &str) -> String {
    let e: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_lowercase();
    if e.is_empty() { "bin".into() } else { e }
}

fn mime_of(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" | "md" | "xml" | "json" => "text/plain; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdxb-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn image(src: &str) -> Block {
        Block::new(
            BlockKind::Image { src_kind: "path".into(), src: src.into(), width: None, height: None, caption: None, crop: None },
            RichText::default(),
        )
    }

    #[test]
    fn packs_and_unpacks_assets() {
        let dir = temp_dir();
        std::fs::write(dir.join("pic.png"), b"\x89PNG\r\n\x1a\nfake").unwrap();
        let mut doc = Document { title: RichText::plain("A & B"), ..Default::default() };
        doc.blocks = vec![Block::text("hello"), image("pic.png")];
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert_eq!(book.format, FORMAT);
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.assets.len(), 1);
        let key = book.assets.keys().next().unwrap().clone();
        assert!(key.starts_with("assets/"));
        assert!(book.chapters[0].content.contains("src=\"assets/"));

        let out = temp_dir();
        book.write_assets(&out).unwrap();
        assert_eq!(std::fs::read(out.join(&key)).unwrap(), b"\x89PNG\r\n\x1a\nfake");
        let docs = book.chapter_documents().unwrap();
        assert_eq!(docs[0].document.blocks.len(), 2);
        match &docs[0].document.blocks[1].kind {
            BlockKind::Image { src_kind, src, .. } => {
                assert_eq!(src_kind, "path");
                assert_eq!(src, &key);
            }
            other => panic!("expected image, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn preserves_unicode_asset_names_and_inline_attachment_attributes() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/报告.pdf"), b"pdf fixture").unwrap();
        let doc = crate::docxml::parse("<title>报告</title><p>See <source path=\"assets/报告.pdf\" name=\"报告\" custom=\"keep\"/></p>").unwrap();
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert!(book.assets.contains_key("assets/报告.pdf"));
        assert!(book.chapters[0].content.contains("custom=\"keep\""));
        assert!(book.chapters[0].content.contains("path=\"assets/报告.pdf\""));
        let unpacked = book.chapter_documents().unwrap();
        assert!(unpacked[0].document.asset_sources().contains(&"assets/报告.pdf".to_string()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn identical_bytes_with_different_media_types_keep_separate_assets() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/note.txt"), b"same bytes").unwrap();
        let document = crate::docxml::parse("<title>Text</title><p><source path=\"assets/note.txt\" name=\"note\"/></p>").unwrap();
        let mut original = BTreeMap::new();
        original.insert("assets/image.png".into(), Asset { mime: "image/png".into(), data: B64.encode(b"same bytes") });
        let result = pack_with_assets(vec![PackInput::new(document)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }, original).unwrap();
        assert_eq!(result.assets.len(), 2);
        assert_eq!(result.assets["assets/image.png"].mime, "image/png");
        assert_eq!(result.assets["assets/note.txt"].mime, "text/plain; charset=utf-8");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn json_round_trip_and_validation() {
        let doc = Document { title: RichText::plain("Title"), ..Default::default() };
        let book = pack(vec![PackInput::new(doc)], &PackOptions::default()).unwrap();
        let text = book.to_json().unwrap();
        let back = Book::from_json(&text).unwrap();
        assert_eq!(back.title, book.title);
        assert_eq!(back.chapters[0].content, book.chapters[0].content);
        let mut bad = back;
        bad.format = "other".into();
        assert!(bad.validate().is_err());
        bad.format = FORMAT.into();
        bad.version = 99;
        assert!(bad.validate().is_err());
        bad.version = VERSION;
        bad.cover = Some(String::new());
        assert!(bad.validate().is_ok(), "an empty cover means no cover");
    }

    #[test]
    fn rejects_unsafe_asset_keys() {
        assert!(asset_key_ok("assets/a.png"));
        assert!(!asset_key_ok("assets/../a.png"));
        assert!(!asset_key_ok("/etc/passwd"));
        assert!(!asset_key_ok("assets\\a.png"));
        assert!(!asset_key_ok("C:/a.png"));
    }

    #[test]
    fn localized_falls_back() {
        let l = Localized::new("中文", "");
        assert_eq!(l.get("en"), "中文");
        let l = Localized::new("", "English");
        assert_eq!(l.get("zh-CN"), "English");
    }
}
