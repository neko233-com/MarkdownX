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
use std::io::Read;
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
    /// Original root-relative path used to resolve links between chapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// Original path of the optional translated chapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_en_path: Option<String>,
    /// Optional English body of the same chapter. The web reader shows it when
    /// the reader picks English; the desktop library keeps the primary `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_en: Option<String>,
    /// If absent, the translation uses `format`, as in version 1 books.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_en: Option<String>,
}

pub fn chapter_source_path_ok(path: &str) -> bool {
    !path.is_empty() && path.len() <= 8192 && !path.starts_with('/') && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control) && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
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
    /// Directory of this chapter, for relative resources in inline CSS.
    pub asset_base: Option<PathBuf>,
}

impl PackInput {
    pub fn new(document: Document) -> Self {
        let title = Localized::same(document.title.plain_text().trim());
        PackInput { document, title, asset_base: None }
    }

    pub fn titled(document: Document, title: Localized) -> Self {
        PackInput { document, title, asset_base: None }
    }

    pub fn with_asset_base(mut self, base: PathBuf) -> Self { self.asset_base = Some(base); self }
}

pub struct PackOptions {
    /// Library root used to resolve relative `path` image/file sources.
    pub assets_root: Option<PathBuf>,
    pub id: Option<String>,
    pub title: Option<Localized>,
    pub max_asset_bytes: u64,
    /// Embed local PDF links when packaging a book folder or library pages.
    pub embed_linked_pdfs: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        PackOptions { assets_root: None, id: None, title: None, max_asset_bytes: MAX_ASSET_BYTES, embed_linked_pdfs: false }
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
    let mut packer = Packer::new(opts.assets_root.clone(), opts.max_asset_bytes, opts.embed_linked_pdfs, assets);
    let mut chapters = Vec::with_capacity(inputs.len());
    let mut first_title = Localized::default();
    for (n, input) in inputs.into_iter().enumerate() {
        let PackInput { mut document, title, asset_base } = input;
        let title = title.clean();
        let mut title = if title.is_empty() {
            let text = document.title.plain_text();
            let t = text.trim();
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
        collect_document(&mut document, &mut packer, asset_base.as_deref())?;
        chapters.push(Chapter {
            id: format!("ch-{}", n + 1),
            title,
            format: "docxxml".into(),
            content: crate::docxml::serialize(&document),
            source_path: None,
            source_en_path: None,
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
        let mut paths = std::collections::HashMap::new();
        for (n, ch) in self.chapters.iter().enumerate() {
            if ch.id.trim().is_empty() {
                bail!("chapter {} has no id", n + 1);
            }
            if !ids.insert(ch.id.as_str()) {
                bail!("duplicate chapter id {:?}", ch.id);
            }
            for path in [&ch.source_path, &ch.source_en_path].into_iter().flatten() {
                if !chapter_source_path_ok(path) { bail!("unsafe chapter source path {:?}", path); }
                if paths.insert(path.as_str(), n).is_some_and(|previous| previous != n) { bail!("duplicate chapter source path {:?}", path); }
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
            && self.data_uri(cover).is_none()
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
            prepare_asset_paths(&mut document, &self.assets)?;
            out.push(ChapterDocument { id: ch.id.clone(), title: ch.title.clone(), document });
        }
        Ok(out)
    }

    /// Resolve a chapter-relative link using recorded source paths, without filesystem access.
    pub fn resolve_relative_chapter(&self, chapter_id: &str, href: &str, english: bool) -> Option<&Chapter> {
        let (href, _) = crate::paste::split_relative_link(href)?;
        let current = self.chapters.iter().find(|ch| ch.id == chapter_id)?;
        if href.is_empty() { return Some(current); }
        let source = if english { current.source_en_path.as_ref().or(current.source_path.as_ref()) } else { current.source_path.as_ref() }?;
        if !chapter_source_path_ok(source) { return None; }
        let decoded = percent_decode_path(href).ok()?;
        if !crate::paste::relative_path_ok(&decoded) { return None; }
        let mut parts: Vec<&str> = source.split('/').collect();
        parts.pop();
        for part in decoded.split('/') {
            match part {
                "" | "." => {}
                ".." => { parts.pop()?; }
                value => parts.push(value),
            }
        }
        let target = parts.join("/");
        if !chapter_source_path_ok(&target) { return None; }
        let mut matches = self.chapters.iter().filter(|ch| ch.source_path.as_deref() == Some(target.as_str()) || ch.source_en_path.as_deref() == Some(target.as_str()));
        let found = matches.next()?;
        if matches.next().is_some() { None } else { Some(found) }
    }

    pub fn data_uri(&self, key: &str) -> Option<String> {
        let resolved = embedded_key(&self.assets, key)?;
        let (asset, fragment) = if let Some(asset) = self.assets.get(&resolved) { (asset, "") }
            else {
                let (path, fragment) = crate::html_assets::svg_fragment(&resolved)?;
                (self.assets.get(path)?, fragment)
            };
        let mime = if asset.mime.is_empty() { "application/octet-stream" } else { &asset.mime };
        Some(format!("data:{mime};base64,{}{fragment}", asset.data))
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
    seen: BTreeMap<(PathBuf, bool, bool, Option<PathBuf>), String>,
    loading: HashSet<PathBuf>,
    inline_bytes: u64,
    embed_linked_pdfs: bool,
}

impl Packer {
    fn new(root: Option<PathBuf>, max: u64, embed_linked_pdfs: bool, assets: BTreeMap<String, Asset>) -> Self {
        let root_canon = root.as_ref().and_then(|r| r.canonicalize().ok());
        Packer { root, root_canon, max, assets, seen: BTreeMap::new(), loading: HashSet::new(), inline_bytes: 0, embed_linked_pdfs }
    }

    fn embed_pdf_link(&mut self, value: &mut String, base: Option<&Path>) -> Result<()> {
        if !self.embed_linked_pdfs { return Ok(()); }
        let Some((relative, page)) = pdf_link_target(value) else { return Ok(()) };
        let Some(root) = self.root.as_ref() else { return Ok(()) };
        let base = base.unwrap_or(root);
        let mut path = base.join(&relative);
        if !value.starts_with("./") && relative.starts_with("assets") && !path.is_file() { path = root.join(&relative); }
        let mut embedded = path.to_string_lossy().into_owned();
        if !self.embed_src(&mut embedded)? { bail!("linked PDF is missing or outside the book folder: {value}"); }
        if let Some(page) = page { embedded.push_str(&format!("#page={page}")); }
        *value = embedded;
        Ok(())
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
        self.embed_src_mode(src, false, false, false, None)
    }

    fn embed_src_mode(&mut self, src: &mut String, stylesheet: bool, module: bool, javascript: bool, document_base: Option<&Path>) -> Result<bool> {
        let s = src.trim();
        if s.is_empty() || s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:") {
            return Ok(false);
        }
        let Some(root) = self.root.as_ref() else { return Ok(false) };
        let original = if Path::new(s).is_absolute() { PathBuf::from(s) } else { root.join(s) };
        let view = if !original.is_file() && !javascript && !stylesheet { crate::html_assets::svg_fragment(s) } else { None };
        // A decoded filesystem name may itself contain '?'; prefer an actual
        // file before interpreting a remaining query as URL metadata.
        let path = if let Some((path, _)) = view {
            if Path::new(path).is_absolute() { PathBuf::from(path) } else { root.join(path) }
        } else if !javascript && !s.contains('#') && !original.is_file() && let Some((path, _)) = s.split_once('?') {
            if Path::new(path).is_absolute() { PathBuf::from(path) } else { root.join(path) }
        } else { original };
        let canon = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };
        if let Some(root_canon) = &self.root_canon
            && !canon.starts_with(root_canon)
        {
            return Ok(false);
        }
        let cache_base = javascript.then(|| document_base.unwrap_or(root).to_path_buf());
        let cache_key = (canon.clone(), stylesheet, module, cache_base);
        if let Some(key) = self.seen.get(&cache_key) {
            *src = format!("{key}{}", view.map_or("", |(_, fragment)| fragment));
            return Ok(true);
        }
        let file = match std::fs::File::open(&canon) {
            Ok(file) => file,
            Err(_) => return Ok(false),
        };
        if file.metadata().is_ok_and(|meta| meta.len() > self.max) {
            bail!("asset {} is larger than {} MB", canon.display(), self.max / 1024 / 1024);
        }
        let mut bytes = vec![];
        if file.take(self.max.saturating_add(1)).read_to_end(&mut bytes).is_err() { return Ok(false); }
        if bytes.len() as u64 > self.max {
            bail!("asset {} is larger than {} MB", canon.display(), self.max / 1024 / 1024);
        }
        let stem = canon.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
        let ext = canon.extension().and_then(|s| s.to_str()).unwrap_or("bin");
        let model = ext.eq_ignore_ascii_case("gltf") || ext.eq_ignore_ascii_case("glb");
        let bytes = if stylesheet {
            crate::css_assets::bundle_file(&canon, root, self.max)?
        } else if javascript {
            crate::js_assets::bundle_file_for_document(&canon, document_base.unwrap_or(root), root, self.max, module)?
        } else if model {
            if !self.loading.insert(canon.clone()) { bail!("circular model dependency: {}", canon.display()); }
            let result = self.bundle_gltf(&canon, &bytes, ext);
            self.loading.remove(&canon);
            result?
        } else { bytes };
        if bytes.len() as u64 > self.max { bail!("asset {} is larger than {} MB", canon.display(), self.max / 1024 / 1024); }
        let encoded = B64.encode(&bytes);
        let mime = if stylesheet { "text/css" } else if module { "text/javascript" } else { mime_of(ext) };
        if let Some((key, _)) = self.assets.iter().find(|(key, a)| a.data == encoded && (a.mime == mime || canon.ends_with(Path::new(key)))
            && (!model || !key.trim_start_matches("assets/").contains('/'))) {
            let key = key.clone();
            self.seen.insert(cache_key, key.clone());
            *src = format!("{key}{}", view.map_or("", |(_, fragment)| fragment));
            return Ok(true);
        }
        let key = self.unique_key(stem, ext);
        self.assets.insert(
            key.clone(),
            Asset { mime: mime.into(), data: encoded },
        );
        self.seen.insert(cache_key, key.clone());
        *src = format!("{key}{}", view.map_or("", |(_, fragment)| fragment));
        Ok(true)
    }

    /// Rewrite external glTF buffers and textures into the same embedded
    /// assets directory as the model. Original files are never modified.
    fn bundle_gltf(&mut self, path: &Path, bytes: &[u8], ext: &str) -> Result<Vec<u8>> {
        if ext.eq_ignore_ascii_case("gltf") {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).context("invalid glTF JSON")?;
            if !value.is_object() { bail!("invalid glTF root"); }
            return if self.bundle_gltf_json(path, &mut value)? { Ok(serde_json::to_vec(&value)?) } else { Ok(bytes.to_vec()) };
        }
        if bytes.len() < 20 || &bytes[..4] != b"glTF" || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2
            || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len() { bail!("invalid GLB header"); }
        let mut chunks = vec![];
        let mut at = 12usize;
        let mut changed = false;
        let mut saw_json = false;
        while at < bytes.len() {
            if bytes.len() - at < 8 { bail!("invalid GLB chunk header"); }
            let size = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let kind = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
            at += 8;
            if size > bytes.len() - at || !size.is_multiple_of(4) { bail!("invalid GLB chunk length"); }
            let chunk = &bytes[at..at + size];
            if !saw_json && kind != 0x4e4f534a || saw_json && kind == 0x4e4f534a { bail!("invalid GLB JSON chunk order"); }
            let data = if kind == 0x4e4f534a && !saw_json {
                saw_json = true;
                let mut value: serde_json::Value = serde_json::from_slice(chunk).context("invalid GLB JSON")?;
                if !value.is_object() { bail!("invalid GLB root"); }
                if self.bundle_gltf_json(path, &mut value)? {
                    changed = true;
                    let mut json = serde_json::to_vec(&value)?;
                    while !json.len().is_multiple_of(4) { json.push(b' '); }
                    json
                } else { chunk.to_vec() }
            } else { chunk.to_vec() };
            chunks.push((kind, data));
            at += size;
        }
        if !saw_json { bail!("GLB has no JSON chunk"); }
        if !changed { return Ok(bytes.to_vec()); }
        let total = 12usize.checked_add(chunks.iter().map(|(_, data)| 8 + data.len()).sum::<usize>()).context("GLB is too large")?;
        let mut result = Vec::with_capacity(total);
        result.extend_from_slice(b"glTF");
        result.extend_from_slice(&2u32.to_le_bytes());
        result.extend_from_slice(&u32::try_from(total)?.to_le_bytes());
        for (kind, data) in chunks {
            result.extend_from_slice(&u32::try_from(data.len())?.to_le_bytes());
            result.extend_from_slice(&kind.to_le_bytes());
            result.extend_from_slice(&data);
        }
        Ok(result)
    }

    fn bundle_gltf_json(&mut self, path: &Path, value: &mut serde_json::Value) -> Result<bool> {
        let mut changed = false;
        for group in ["buffers", "images"] {
            let Some(group_value) = value.get_mut(group) else { continue };
            let items = group_value.as_array_mut().with_context(|| format!("invalid glTF {group}"))?;
            for item in items {
                if !item.is_object() { bail!("invalid glTF {group} entry"); }
                let Some(uri_value) = item.get("uri") else { continue };
                let uri = uri_value.as_str().context("invalid glTF dependency URI")?;
                if uri.starts_with("data:") { continue; }
                if uri.is_empty() || uri.contains([':', '?', '#', '\\']) { bail!("unsupported glTF dependency {uri}"); }
                let decoded = percent_decode_path(uri)?;
                let relative = Path::new(&decoded);
                if relative.is_absolute() { bail!("absolute glTF dependency is not supported"); }
                let dependency = path.parent().context("glTF has no parent directory")?.join(relative);
                let mut key = dependency.to_string_lossy().to_string();
                if !self.embed_src(&mut key)? { bail!("glTF dependency is missing or outside the book folder: {uri}"); }
                if key.trim_start_matches("assets/").contains('/') {
                    let asset = self.assets.get(&key).cloned().context("embedded glTF dependency is missing")?;
                    let unique = self.unique_key(dependency.file_stem().and_then(|s| s.to_str()).unwrap_or("asset"), dependency.extension().and_then(|s| s.to_str()).unwrap_or("bin"));
                    self.assets.insert(unique.clone(), asset);
                    key = unique;
                }
                item["uri"] = serde_json::Value::String(format!("./{}", key.trim_start_matches("assets/")));
                changed = true;
            }
        }
        Ok(changed)
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

pub(crate) fn percent_decode_path(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() { bail!("invalid percent escape in model path"); }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])?;
            decoded.push(u8::from_str_radix(hex, 16).context("invalid percent escape in model path")?);
            i += 3;
        } else { decoded.push(bytes[i]); i += 1; }
    }
    let value = String::from_utf8(decoded).context("model path is not UTF-8")?;
    if value.contains([':', '\\', '\0']) { bail!("invalid model dependency path"); }
    Ok(value)
}

/// A local PDF link and optional one-based page number, decoded for the filesystem.
pub fn pdf_link_target(value: &str) -> Option<(PathBuf, Option<u32>)> {
    let (path, fragment) = crate::paste::split_relative_link(value)?;
    let decoded = percent_decode_path(path).ok()?;
    let path = PathBuf::from(decoded);
    if path.is_absolute() || !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) { return None; }
    let page = match fragment {
        Some(fragment) => {
            let decoded = percent_decode_path(fragment).ok()?;
            let digits = decoded.strip_prefix("page=")?;
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
            let page = digits.parse::<u32>().ok()?;
            if page == 0 || page > i32::MAX as u32 { return None; }
            Some(page)
        }
        None => None,
    };
    Some((path, page))
}

fn collect_document(doc: &mut Document, packer: &mut Packer, base: Option<&Path>) -> Result<()> {
    if let Some(cover) = &mut doc.cover {
        packer.embed_src(&mut cover.src)?;
    }
    collect_rich(&mut doc.title, packer, base)?;
    collect_blocks(&mut doc.blocks, packer, base)
}

fn collect_rich(text: &mut RichText, packer: &mut Packer, base: Option<&Path>) -> Result<()> {
    for run in &mut text.runs {
            if let Some(link) = &mut run.marks.link { packer.embed_pdf_link(link, base)?; }
            if let Some(inline) = &mut run.marks.inline
                && let InlineKind::File(file) = &inline.kind
            {
                let mut file = file.clone();
                let old = file.clone();
                packer.embed(&mut file.src_kind, &mut file.src)?;
                if file != old { crate::docxml::remap_inline_file(inline, file)?; }
            }
            if let Some(inline) = &mut run.marks.inline
                && let InlineKind::Image(image) = &inline.kind
            {
                let mut image = image.clone();
                let old = image.clone();
                packer.embed(&mut image.src_kind, &mut image.src)?;
                if image != old { crate::docxml::remap_inline_image(inline, image)?; }
            }
        }
    Ok(())
}

fn collect_blocks(blocks: &mut [Block], packer: &mut Packer, base: Option<&Path>) -> Result<()> {
    for b in blocks {
        collect_rich(&mut b.text, packer, base)?;
        match &mut b.kind {
            BlockKind::Image { src_kind, src, .. } => packer.embed(src_kind, src)?,
            BlockKind::File(f) => packer.embed(&mut f.src_kind, &mut f.src)?,
            BlockKind::Interactive(i) if i.kind == InteractiveKind::Html => {
                i.source = crate::html_assets::rewrite_references(&i.source, |reference| {
                    let script = reference.script_parts();
                    let (src, fragment) = reference.media_time().or(script).unwrap_or((&reference.value, ""));
                    let fragment = if script.is_some() { if reference.module { crate::html_assets::script_identity(fragment) } else { String::new() } } else { fragment.to_string() };
                    let mut next = src.to_string();
                    Ok::<_, anyhow::Error>(packer.embed_src_mode(&mut next, reference.tag == "link", reference.module, reference.javascript, base)?.then(|| format!("{next}{fragment}")))
                })?;
                if let Some(root) = packer.root.clone() {
                    let base = base.unwrap_or(&root);
                    i.source = crate::html_assets::rewrite_inline_styles(&i.source, |css| {
                        let bundled = crate::css_assets::bundle_inline(css, base, &root, packer.max)?;
                        packer.inline_bytes = packer.inline_bytes.saturating_add(bundled.len().saturating_sub(css.len()) as u64);
                        if packer.inline_bytes > MAX_TOTAL_ASSET_BYTES { bail!("inline stylesheet dependencies are too large"); }
                        Ok::<_, anyhow::Error>((bundled != css).then_some(bundled))
                    })?;
                    i.source = crate::html_assets::rewrite_inline_scripts(&i.source, |script, module| {
                        if !module && !crate::js_assets::may_reference_local_assets(script) { return Ok::<_, anyhow::Error>(None); }
                        let bundled = crate::js_assets::bundle_inline(script, base, &root, packer.max)?;
                        packer.inline_bytes = packer.inline_bytes.saturating_add(bundled.len().saturating_sub(script.len()) as u64);
                        if packer.inline_bytes > MAX_TOTAL_ASSET_BYTES { bail!("inline module dependencies are too large"); }
                        Ok::<_, anyhow::Error>((bundled != script).then_some(bundled))
                    })?;
                }
            }
            BlockKind::Table(t) => {
                for row in &mut t.rows {
                    for cell in row {
                        collect_blocks(&mut cell.blocks, packer, base)?;
                    }
                }
            }
            _ => {}
        }
        collect_blocks(&mut b.children, packer, base)?;
    }
    Ok(())
}

fn embedded_key(assets: &BTreeMap<String, Asset>, src: &str) -> Option<String> {
    let src = src.trim_start_matches("./");
    if assets.contains_key(src) { return Some(src.to_string()); }
    if let Some((path, fragment)) = crate::html_assets::svg_fragment(src)
        && let Some(key) = embedded_key(assets, path)
        && assets.get(&key).is_some_and(|asset| asset.mime.eq_ignore_ascii_case("image/svg+xml"))
    { return Some(format!("{key}{fragment}")); }
    let src = if src.contains('#') { src } else { src.split_once('?').map_or(src, |(path, _)| path) };
    let mut bytes = vec![];
    let raw = src.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len()
            && let Some(value) = std::str::from_utf8(&raw[i + 1..i + 3]).ok().and_then(|s| u8::from_str_radix(s, 16).ok())
        { bytes.push(value); i += 3; }
        else { bytes.push(raw[i]); i += 1; }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    assets.contains_key(&decoded).then_some(decoded)
}

fn prepare_asset_paths(doc: &mut Document, assets: &BTreeMap<String, Asset>) -> Result<()> {
    fn rich(text: &mut RichText, assets: &BTreeMap<String, Asset>) -> Result<()> {
        for run in &mut text.runs {
                if let Some(inline) = &mut run.marks.inline
                    && let InlineKind::Image(image) = &inline.kind
                    && let Some(key) = embedded_key(assets, &image.src)
                {
                    let mut image = image.clone();
                    image.src_kind = "path".into();
                    image.src = key;
                    crate::docxml::remap_inline_image(inline, image)?;
                }
            }
        Ok(())
    }
    fn blocks(bs: &mut [Block], assets: &BTreeMap<String, Asset>) -> Result<()> {
        for b in bs {
            rich(&mut b.text, assets)?;
            match &mut b.kind {
                BlockKind::Interactive(i) if i.kind == InteractiveKind::Html => {
                    i.source = crate::html_assets::rewrite_references(&i.source, |reference| {
                        let script = reference.script_parts();
                        let (src, fragment) = reference.media_time().or(script).unwrap_or((&reference.value, ""));
                        let fragment = if script.is_some() { if reference.module { crate::html_assets::script_identity(fragment) } else { String::new() } } else { fragment.to_string() };
                        Ok::<_, anyhow::Error>(embedded_key(assets, src).map(|key| format!("{key}{fragment}")))
                    })?;
                }
                BlockKind::Image { src_kind, src, .. } if embedded_key(assets, src).is_some() => {
                    *src = embedded_key(assets, src).unwrap();
                    *src_kind = "path".into();
                }
                BlockKind::File(f) if assets.contains_key(f.src.as_str()) => {
                    f.src_kind = "path".into();
                }
                BlockKind::Table(t) => {
                    for row in &mut t.rows {
                        for cell in row {
                            blocks(&mut cell.blocks, assets)?;
                        }
                    }
                }
                _ => {}
            }
            blocks(&mut b.children, assets)?;
        }
        Ok(())
    }
    rich(&mut doc.title, assets)?;
    blocks(&mut doc.blocks, assets)
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

pub(crate) fn mime_of(ext: &str) -> &'static str {
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
        "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "obj" => "model/obj",
        "stl" => "model/stl",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "vtt" => "text/vtt",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "txt" | "md" | "xml" | "json" => "text/plain; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_packaging_does_not_change_the_same_file_as_an_attachment() {
        let dir = temp_dir();
        std::fs::write(dir.join("frame.svg"), b"<svg/>").unwrap();
        let css = "canvas { background: url('./frame.svg') }";
        std::fs::write(dir.join("game.css"), css).unwrap();
        let widget = Block::new(BlockKind::Interactive(Interactive::html("", "<canvas></canvas><link rel='stylesheet' href='game.css'>")), RichText::default());
        let attachment = Block::new(BlockKind::File(FileBlock { src_kind: "path".into(), src: "game.css".into(), name: "game.css".into(), view: FileView::Card }), RichText::default());
        let doc = Document { blocks: vec![widget, attachment], ..Default::default() };
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let styles = book.assets.values().filter(|asset| asset.mime == "text/css").map(|asset| String::from_utf8(B64.decode(&asset.data).unwrap()).unwrap()).collect::<Vec<_>>();
        assert!(styles.iter().any(|style| style == css));
        assert!(styles.iter().any(|style| style.contains("data:image/svg+xml;base64,PHN2Zy8+")));
        assert_eq!(std::fs::read_to_string(dir.join("game.css")).unwrap(), css);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
    fn inline_images_are_embedded_and_restored() {
        let dir = temp_dir();
        std::fs::write(dir.join("frame.svg"), b"<svg/>").unwrap();
        let doc = crate::docxml::parse("<title>Story</title><p>Before <inline-image src=\"./frame.svg\" alt=\"frame\"/> after</p>").unwrap();
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert_eq!(book.assets.len(), 1);
        let key = book.assets.keys().next().unwrap();
        let restored = book.chapter_documents().unwrap();
        assert!(restored[0].document.asset_sources().contains(key));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_image_query_uses_the_file_and_embedded_lookup() {
        let dir = temp_dir();
        std::fs::write(dir.join("frame.svg"), "<svg/>").unwrap();
        let doc = crate::docxml::parse("<title>Story</title><p><inline-image src=\"./frame.svg?v=1\" alt=\"frame\"/></p>").unwrap();
        let mut book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let key = book.assets.keys().next().unwrap().clone();
        assert_eq!(book.data_uri(&key), book.data_uri(&format!("{key}?v=2")));
        assert!(book.data_uri(&format!("{key}#icon")).unwrap().ends_with("#icon"));
        book.assets.insert("assets/other.png".into(), Asset { mime: "image/png".into(), data: B64.encode(b"image") });
        assert!(book.data_uri("assets/other.png#icon").is_none());
        book.chapters[0].content = format!("<title>Story</title><p><inline-image src=\"{key}?v=2\" alt=\"frame\"/></p>");
        let restored = book.chapter_documents().unwrap();
        assert!(restored[0].document.asset_sources().contains(&key));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn media_time_fragment_is_preserved_when_packing_and_unpacking() {
        let dir = temp_dir();
        std::fs::write(dir.join("clip.mp4"), b"video").unwrap();
        let doc = crate::docxml::parse("<title>Story</title><interactive type=\"html\"><![CDATA[<video src='./clip.mp4?v=1#t=2,5'></video>]]></interactive>").unwrap();
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let key = book.assets.keys().next().unwrap();
        assert!(book.chapters[0].content.contains(&format!("{key}#t=2,5")));
        let docs = book.chapter_documents().unwrap();
        assert!(docs[0].document.asset_sources().contains(key));
        let BlockKind::Interactive(widget) = &docs[0].document.blocks[0].kind else { panic!("expected interactive media") };
        assert!(widget.source.contains("#t=2,5"));
        let absolute = dir.join("clip.mp4").canonicalize().unwrap();
        let html = format!("<video src='{}#t=2,5'></video>", absolute.display());
        let reference = crate::html_assets::references(&html).pop().unwrap();
        assert_eq!(reference.media_time().unwrap().0, absolute.to_str().unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_pdf_links_are_embedded_with_page_and_confined_to_the_root() {
        let dir = temp_dir();
        let chapter = dir.join("chapter");
        std::fs::create_dir_all(&chapter).unwrap();
        std::fs::write(chapter.join("paper one.pdf"), b"%PDF-1.4\nchapter").unwrap();
        std::fs::write(dir.join("paper one.pdf"), b"%PDF-1.4\nwrong").unwrap();
        let doc = crate::docxml::parse("<title>T</title><p><a href=\"./paper%20one.pdf#page=2\">Paper</a></p>").unwrap();
        let options = PackOptions { assets_root: Some(dir.clone()), embed_linked_pdfs: true, ..Default::default() };
        let book = pack(vec![PackInput::new(doc.clone()).with_asset_base(chapter.clone())], &options).unwrap();
        let key = book.assets.keys().next().unwrap();
        assert_eq!(B64.decode(&book.assets[key].data).unwrap(), b"%PDF-1.4\nchapter");
        assert!(book.chapters[0].content.contains(&format!("{key}#page=2")));
        assert!(crate::docxml::serialize(&doc).contains("./paper%20one.pdf#page=2"));
        assert_eq!(pdf_link_target("a.pdf#page=002").unwrap().1, Some(2));
        assert!(pdf_link_target("a.pdf#page=0").is_none());
        assert!(pdf_link_target("https://example.test/a.pdf").is_none());
        let outside = dir.with_extension("pdf");
        std::fs::write(&outside, b"%PDF-1.4\noutside").unwrap();
        let escape = format!("../../{}", outside.file_name().unwrap().to_string_lossy());
        let unsafe_doc = crate::docxml::parse(&format!("<title>T</title><p><a href=\"{escape}\">outside</a></p>")).unwrap();
        assert!(pack(vec![PackInput::new(unsafe_doc).with_asset_base(chapter)], &options).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn svg_views_share_the_asset_and_survive_book_roundtrip() {
        let dir = temp_dir();
        let path = dir.join("frame#take.svg");
        std::fs::write(&path, b"<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        let one = format!("{}#right", path.display());
        let two = format!("{}#svgView(viewBox(50,0,50,50))", path.display());
        let image = |src| Block::new(BlockKind::Image { src_kind: "path".into(), src, width: None, height: None, caption: None, crop: None }, RichText::default());
        let doc = Document { blocks: vec![image(one), image(two)], ..Default::default() };
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert_eq!(book.assets.len(), 1);
        let key = book.assets.keys().next().unwrap();
        let right = format!("{key}#right");
        assert!(book.data_uri(&right).unwrap().ends_with("#right"));
        assert!(book.chapters[0].content.contains(&right));
        assert!(book.chapter_documents().unwrap()[0].document.asset_sources().contains(&right));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn script_queries_preserve_module_identity_through_book_roundtrip() {
        let dir = temp_dir();
        std::fs::write(dir.join("game.mjs"), "globalThis.count=(globalThis.count||0)+1;").unwrap();
        let source = "<canvas></canvas><script type='module' src='./game.mjs?v=1'></script><script type='module' src='./game.mjs?v=1'></script><script type='module' src='./game.mjs?v=2'></script><script type='module' src='./game.mjs?v=中'></script><script type='module' src='./game.mjs?v=%E4%B8%AD'></script><script type='module' src='./game.mjs#%3F%76%3D%E4%B8%AD'></script>";
        let doc = crate::docxml::parse(&format!("<title>Game</title><interactive type=\"html\"><![CDATA[{source}]]></interactive>")).unwrap();
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let key = book.assets.keys().next().unwrap();
        assert_eq!(book.assets.len(), 1);
        assert_eq!(book.chapters[0].content.matches("#%3F%76%3D%31").count(), 2);
        assert_eq!(book.chapters[0].content.matches("#%3F%76%3D%32").count(), 1);
        assert_eq!(book.chapters[0].content.matches("#%3F%76%3D%25%45%34%25%42%38%25%41%44").count(), 3);
        let docs = book.chapter_documents().unwrap();
        assert!(docs[0].document.asset_sources().iter().all(|value| value == key));
        let BlockKind::Interactive(widget) = &docs[0].document.blocks[0].kind else { panic!("expected interactive widget") };
        assert_eq!(widget.source.matches("#%3F%76%3D%31").count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inline_title_images_are_embedded_and_restored() {
        let dir = temp_dir();
        std::fs::write(dir.join("frame.svg"), b"<svg/>").unwrap();
        let doc = crate::docxml::parse("<title>Story <inline-image src=\"./frame.svg\" alt=\"frame\"/> <latex>\\alpha</latex></title><p>Body</p>").unwrap();
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert_eq!(book.assets.len(), 1);
        let key = book.assets.keys().next().unwrap();
        let restored = book.chapter_documents().unwrap();
        assert!(restored[0].document.asset_sources().contains(key));
        assert_eq!(restored[0].document.title.plain_text(), "Story frame \\alpha");
        assert_eq!(book.chapters[0].title.zh, "Story frame \\alpha");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn module_scripts_embed_relative_dependencies_and_keep_classic_scripts() {
        let dir = temp_dir();
        std::fs::write(dir.join("helper.mjs"), "export const value = 42;").unwrap();
        std::fs::write(dir.join("frame.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("entry.mjs"), "import {value} from './helper.mjs'; const frame=new URL('frame.svg',import.meta.url); document.body.dataset.value = value;").unwrap();
        let html = r#"<canvas></canvas><script type='module' src='./entry.mjs'></script><script type='module'>import './helper.mjs';</script><script>const example="import './missing.js'";</script>"#;
        let doc = Document { blocks: vec![Block::new(BlockKind::Interactive(Interactive::html("", html)), RichText::default())], ..Default::default() };
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let asset = book.assets.values().find(|asset| asset.mime == "text/javascript").unwrap();
        let entry = String::from_utf8(B64.decode(&asset.data).unwrap()).unwrap();
        assert!(entry.contains("data:text/javascript;base64,") && entry.contains("data:image/svg+xml;base64,PHN2Zy8+"), "{entry}");
        let chapter = crate::docxml::parse(&book.chapters[0].content).unwrap();
        let BlockKind::Interactive(widget) = &chapter.blocks[0].kind else { panic!("expected widget") };
        assert!(widget.source.contains("data:text/javascript;base64,") && widget.source.contains("missing.js"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn html_media_is_embedded_without_rewriting_script_text() {
        let dir = temp_dir();
        std::fs::write(dir.join("clip.mp4"), b"video fixture").unwrap();
        std::fs::write(dir.join("poster.svg"), b"<svg/>").unwrap();
        std::fs::write(dir.join("sound.wav"), b"audio fixture").unwrap();
        let html = "<video src='./clip.mp4' poster=\"poster.svg\" controls><source src='clip.mp4'></video><audio controls><source src='./sound.wav'></audio><picture><source srcset='./poster.svg 1x, poster.svg 2x'></picture><script>let example=\"<video src='keep.mp4'>\"</script>";
        let block = Block::new(BlockKind::Interactive(Interactive::html("Video", html)), RichText::default());
        let doc = Document { blocks: vec![block], ..Default::default() };
        let book = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        assert_eq!(book.assets.len(), 3);
        let restored = book.chapter_documents().unwrap();
        let BlockKind::Interactive(widget) = &restored[0].document.blocks[0].kind else { panic!("expected widget") };
        assert!(widget.source.contains("<video src='assets/clip.mp4'"), "{}", widget.source);
        assert!(widget.source.contains("poster=\"assets/poster.svg\""));
        assert!(widget.source.contains("<audio controls><source src='assets/sound.wav'>"));
        assert!(widget.source.contains("srcset='assets/poster.svg 1x, assets/poster.svg 2x'"));
        assert!(widget.source.contains("<video src='keep.mp4'>"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn html_scripts_embed_local_fetch_from_each_document_directory() {
        let dir = temp_dir();
        let chapter = dir.join("chapter");
        let scripts = chapter.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(chapter.join("data.json"), "{\"chapter\":true}").unwrap();
        std::fs::write(scripts.join("data.json"), "wrong").unwrap();
        let game = scripts.join("game.js");
        std::fs::write(&game, "fetch('data.json').then(r=>r.json());").unwrap();
        let html = format!("<canvas></canvas><script src='{}'></script><script>fetch('./data.json')</script><script type='application/json'>{{\"fetch\":\"keep\"}}</script>", game.display());
        let block = Block::new(BlockKind::Interactive(Interactive::html("", &html)), RichText::default());
        let doc = Document { blocks: vec![block], ..Default::default() };
        let book = pack(vec![PackInput::new(doc).with_asset_base(chapter)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
        let script = book.assets.values().find(|a| a.mime.contains("javascript")).unwrap();
        let script = String::from_utf8(B64.decode(&script.data).unwrap()).unwrap();
        let encoded = B64.encode("{\"chapter\":true}");
        assert!(script.contains(&encoded));
        let restored = book.chapter_documents().unwrap();
        let BlockKind::Interactive(widget) = &restored[0].document.blocks[0].kind else { panic!("expected widget") };
        assert!(widget.source.contains(&encoded) && widget.source.contains("{\"fetch\":\"keep\"}"));
        assert_eq!(std::fs::read_to_string(game).unwrap(), "fetch('data.json').then(r=>r.json());");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn html_media_over_the_asset_limit_is_rejected() {
        let dir = temp_dir();
        std::fs::write(dir.join("clip.mp4"), b"12345").unwrap();
        let block = Block::new(BlockKind::Interactive(Interactive::html("", "<video src='clip.mp4'></video>")), RichText::default());
        let doc = Document { blocks: vec![block], ..Default::default() };
        let result = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), max_asset_bytes: 4, ..Default::default() });
        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gltf_and_glb_dependencies_are_relocated_and_remain_loadable() {
        let dir = temp_dir();
        let mut binary = vec![];
        for value in [0.0f32, 0., 0., 1., 0., 0., 0., 1., 0.] { binary.extend_from_slice(&value.to_le_bytes()); }
        std::fs::write(dir.join("mesh data.bin"), &binary).unwrap();
        let json = serde_json::json!({"asset":{"version":"2.0"}, "buffers":[{"uri":"./mesh%20data.bin","byteLength":36}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0});
        let raw_json = serde_json::to_vec(&json).unwrap();
        std::fs::write(dir.join("model.gltf"), &raw_json).unwrap();
        let mut chunk = raw_json.clone();
        while !chunk.len().is_multiple_of(4) { chunk.push(b' '); }
        let mut glb = b"glTF".to_vec();
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&u32::try_from(20 + chunk.len()).unwrap().to_le_bytes());
        glb.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_le_bytes());
        glb.extend_from_slice(&0x4e4f534au32.to_le_bytes());
        glb.extend_from_slice(&chunk);
        std::fs::write(dir.join("model.glb"), &glb).unwrap();
        for name in ["model.gltf", "model.glb"] {
            let html = format!("<model-viewer src='./{name}'></model-viewer>");
            let doc = Document { blocks: vec![Block::new(BlockKind::Interactive(Interactive::html("", html)), RichText::default())], ..Default::default() };
            let packed = pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).unwrap();
            assert_eq!(packed.assets.len(), 2);
            let model_key = packed.assets.keys().find(|key| key.ends_with(name.rsplit('.').next().unwrap())).unwrap();
            let output = temp_dir();
            packed.write_assets(&output).unwrap();
            let mesh = crate::media::model::load(&output.join(model_key)).unwrap();
            assert_eq!(mesh.triangles.len(), 1);
            std::fs::remove_dir_all(output).unwrap();
        }
        assert_eq!(std::fs::read(dir.join("model.gltf")).unwrap(), raw_json);
        assert_eq!(std::fs::read(dir.join("model.glb")).unwrap(), glb);
        std::fs::remove_file(dir.join("mesh data.bin")).unwrap();
        let doc = Document { blocks: vec![Block::new(BlockKind::Interactive(Interactive::html("", "<model-viewer src='model.gltf'></model-viewer>")), RichText::default())], ..Default::default() };
        assert!(pack(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(dir.clone()), ..Default::default() }).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gltf_dependencies_cannot_leave_the_book_or_recurse() {
        let dir = temp_dir();
        let outside = temp_dir();
        std::fs::write(outside.join("data.bin"), b"private").unwrap();
        let escape = format!("../{}/data.bin", outside.file_name().unwrap().to_string_lossy());
        let path = dir.join("model.gltf");
        let doc = || Document { blocks: vec![Block::new(BlockKind::Interactive(Interactive::html("", "<model-viewer src='model.gltf'></model-viewer>")), RichText::default())], ..Default::default() };
        let opts = PackOptions { assets_root: Some(dir.clone()), ..Default::default() };
        std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"buffers":[{"uri":escape}]})).unwrap()).unwrap();
        assert!(pack(vec![PackInput::new(doc())], &opts).unwrap_err().to_string().contains("outside"));
        std::fs::write(&path, br#"{"buffers":[{"uri":"./model.gltf"}]}"#).unwrap();
        assert!(pack(vec![PackInput::new(doc())], &opts).unwrap_err().to_string().contains("circular"));
        assert_eq!(std::fs::read(outside.join("data.bin")).unwrap(), b"private");
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
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
