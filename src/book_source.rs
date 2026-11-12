//! Preserve imported book metadata and translations by stable library page ID.
//! Source snapshots are immutable, so a crash cannot partially replace them.

use anyhow::{Context, Result, bail};
use fdl_core::book::{self, Book, Localized, PackInput, PackOptions};
use crate::model::Document;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub imported_title: String,
}

#[derive(Serialize, Deserialize)]
pub struct Source {
    version: u32,
    pub pages: Vec<Page>,
    pub book: Book,
    #[serde(skip)]
    index: OnceLock<HashMap<String, usize>>,
}

fn safe_id(id: &str) -> Result<&str> {
    anyhow::ensure!(!id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "invalid library page id");
    Ok(id)
}

fn source_path(root: &Path, id: &str) -> Result<PathBuf> {
    Ok(root.join(".book-sources").join("sources").join(format!("{}.json", safe_id(id)?)))
}

fn link_path(root: &Path, id: &str) -> Result<PathBuf> {
    Ok(root.join(".book-sources").join("pages").join(format!("{}.json", safe_id(id)?)))
}

impl Source {
    pub fn write(root: &Path, pages: Vec<Page>, book: Book) -> Result<()> {
        anyhow::ensure!(!pages.is_empty() && pages.len() == book.chapters.len(), "book page mapping is incomplete");
        let source_id = safe_id(&pages[0].id)?.to_string();
        let source = Source { version: 1, pages, book, index: OnceLock::new() };
        let path = source_path(root, &source_id)?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::create_dir_all(root.join(".book-sources/pages"))?;
        crate::workspace_fs::write_atomic(&path, &serde_json::to_vec(&source)?)?;
        for page in &source.pages {
            crate::workspace_fs::write_atomic(&link_path(root, &page.id)?, source_id.as_bytes())?;
        }
        Ok(())
    }

    /// Mixed origins must not silently merge colliding asset names.
    pub fn for_pages(root: &Path, ids: &[String]) -> Result<Option<Self>> {
        let mut origins = BTreeSet::new();
        let mut linked = Vec::new();
        for id in ids {
            match std::fs::read_to_string(link_path(root, id)?) {
                Ok(source) => { origins.insert(safe_id(source.trim())?.to_string()); linked.push(id); }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if source_path(root, id)?.try_exists()? { origins.insert(id.clone()); linked.push(id); }
                }
                Err(e) => return Err(e.into()),
            }
        }
        if origins.len() > 1 { bail!("cannot safely combine pages from different imported books"); }
        let Some(origin) = origins.into_iter().next() else { return Ok(None) };
        let bytes = std::fs::read(source_path(root, &origin)?).context("book translation source is unavailable")?;
        let source: Source = serde_json::from_slice(&bytes).context("book translation source is damaged")?;
        anyhow::ensure!(source.version == 1 && source.pages.len() == source.book.chapters.len(), "book translation source is invalid");
        let index = source.page_index();
        anyhow::ensure!(source.pages.first().is_some_and(|p| p.id == origin) && index.len() == source.pages.len() && linked.iter().all(|id| index.contains_key(*id)), "book translation mapping is invalid");
        source.book.validate()?;
        Ok(Some(source))
    }

    pub fn translation(&self, id: &str) -> Option<(&book::Chapter, &Page)> {
        let i = *self.page_index().get(id)?;
        Some((&self.book.chapters[i], &self.pages[i]))
    }

    fn page_index(&self) -> &HashMap<String, usize> {
        self.index.get_or_init(|| self.pages.iter().enumerate().map(|(i, page)| (page.id.clone(), i)).collect())
    }

    pub fn pack(&self, pages: Vec<(String, String, Document)>, opts: &PackOptions) -> Result<Book> {
        if let Some(root) = &opts.assets_root {
            for (_, _, doc) in &pages {
                let mut sources = doc.asset_sources();
                if let Some(cover) = &doc.cover { sources.push(cover.src.clone()); }
                for source in sources.into_iter().filter(|s| s.starts_with("assets/books/")) {
                    let path = root.join(&source);
                    anyhow::ensure!(path.is_file() && path.canonicalize()?.starts_with(root.canonicalize()?), "an imported book asset is unavailable: {source}");
                }
            }
        }
        let ids: Vec<_> = pages.iter().map(|p| p.0.clone()).collect();
        let inputs = pages.iter().map(|(_, title, doc)| PackInput::titled(doc.clone(), Localized::same(title))).collect();
        let packed = book::pack_with_assets(inputs, opts, self.book.assets.clone())?;
        let is_root = ids.first() == self.pages.first().map(|p| &p.id);
        let mut result = if is_root { self.book.clone() } else { packed.clone() };
        result.assets = packed.assets;
        result.chapters = packed.chapters;
        result.modified = packed.modified;
        result.generator = packed.generator;
        let mut used = HashSet::new();
        for (index, (id, title, _)) in pages.iter().enumerate() {
            let chapter = &mut result.chapters[index];
            if let Some((original, page)) = self.translation(id) {
                chapter.id = original.id.clone();
                chapter.title = changed_title(&original.title, &page.imported_title, title);
                chapter.content_en = original.content_en.clone();
                let en_format = original.format_en.as_deref().unwrap_or(&original.format);
                if chapter.content_en.is_some() && en_format != chapter.format {
                    chapter.format_en = Some(en_format.to_string());
                    result.version = 2;
                }
            } else {
                chapter.id = format!("page-{id}");
            }
            if !used.insert(chapter.id.clone()) { bail!("duplicate chapter id after combining pages"); }
        }
        if is_root {
            result.title = changed_title(&self.book.title, &self.pages[0].imported_title, &pages[0].1);
        }
        result.validate()?;
        Ok(result)
    }
}

pub fn changed_title(original: &Localized, imported: &str, current: &str) -> Localized {
    let mut title = original.clone();
    if current != imported {
        if title.zh.trim().is_empty() { title.en = current.to_string(); }
        else { title.zh = current.to_string(); }
    }
    title
}
