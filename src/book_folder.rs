//! Package a workspace folder, respecting its multilingual book manifest.

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use fdl_core::book::{self, Asset, Localized, PackInput, PackOptions};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

fn inside(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    ensure!(!relative.as_os_str().is_empty() && relative.components().all(|c| matches!(c, Component::Normal(_))), "invalid book manifest path");
    let path = root.join(relative);
    let canonical = path.canonicalize().context("book manifest file is unavailable")?;
    ensure!(canonical.starts_with(root.canonicalize()?), "book manifest path leaves the workspace");
    Ok(path)
}

fn field<T: DeserializeOwned>(value: &Value, name: &str) -> Result<Option<T>> {
    value.get(name).filter(|v| !v.is_null()).map(|v| serde_json::from_value(v.clone()).map_err(Into::into)).transpose()
}

fn collect_assets(root: &Path) -> Result<BTreeMap<String, Asset>> {
    fn is_link(meta: &std::fs::Metadata) -> bool {
        if meta.file_type().is_symlink() { return true; }
        #[cfg(windows)]
        { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return true; } }
        false
    }
    fn visit(root: &Path, dir: &Path, depth: usize, total: &mut u64, out: &mut BTreeMap<String, Asset>) -> Result<()> {
        ensure!(depth <= 64, "book asset folders are too deep");
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;
            ensure!(!is_link(&meta), "book assets cannot contain symbolic links or junctions");
            if meta.is_dir() { visit(root, &path, depth + 1, total, out)?; continue; }
            if !meta.is_file() { continue; }
            ensure!(out.len() < 10_000, "book has too many assets");
            ensure!(meta.len() <= 16 * 1024 * 1024, "book asset is larger than 16 MiB");
            *total = total.saturating_add(meta.len());
            ensure!(*total <= 64 * 1024 * 1024, "book assets are larger than 64 MiB");
            let key = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
            let mime = match ext.as_str() {
                "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "webp" => "image/webp", "svg" => "image/svg+xml",
                "pdf" => "application/pdf", "mp4" => "video/mp4", "mp3" => "audio/mpeg", _ => "application/octet-stream",
            };
            let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(&path)?);
            out.insert(key, Asset { mime: mime.into(), data });
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    let dir = root.join("assets");
    if dir.try_exists()? {
        ensure!(!is_link(&std::fs::symlink_metadata(&dir)?), "book assets cannot be a symbolic link or junction");
        let mut total = 0;
        visit(root, &dir, 0, &mut total, &mut result)?;
    }
    Ok(result)
}

pub fn pack(root: &Path, fallback_title: &str, files: &[PathBuf]) -> Result<book::Book> {
    let manifest_path = root.join("book.json");
    let manifest = if manifest_path.try_exists()? {
        ensure!(std::fs::metadata(&manifest_path)?.len() <= 8 * 1024 * 1024, "book manifest is too large");
        let value: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
        (value.get("format").and_then(Value::as_str) == Some(book::FORMAT)).then_some(value)
    } else { None };
    let metas = manifest.as_ref().and_then(|m| m.get("chapters")).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut inputs = Vec::new();
    let mut translations = Vec::new();
    for path in files {
        let relative = path.strip_prefix(root).context("chapter is outside the workspace")?.to_string_lossy().replace('\\', "/");
        inside(root, &relative)?;
        let meta = metas.iter().find(|m| m.get("file").and_then(Value::as_str) == Some(relative.as_str())
            || m.get("source").and_then(Value::as_str) == Some(relative.as_str()));
        let text = std::fs::read_to_string(path)?;
        let xml_path = if crate::workspace_fs::DocKind::of(path) == Some(crate::workspace_fs::DocKind::DocxXml) {
            Some(path.clone())
        } else if let Some(name) = meta.and_then(|m| m.get("source")).and_then(Value::as_str) {
            let candidate = root.join(name);
            if candidate.try_exists()? { Some(inside(root, name)?) } else { None }
        } else {
            let candidate = path.with_extension("xml");
            candidate.try_exists()?.then_some(candidate)
        };
        let xml_is_newer = xml_path.as_ref().and_then(|p| p.metadata().ok()?.modified().ok()).zip(path.metadata().ok().and_then(|m| m.modified().ok()))
            .is_some_and(|(source, markdown)| source >= markdown);
        let doc = if let Some(xml_path) = xml_path.filter(|_| xml_is_newer) {
            crate::docxml::parse(&std::fs::read_to_string(xml_path)?)?
        } else {
            let (mut doc, _) = crate::app::parse_markdown_file(path, &text);
            if let Some(parent) = path.parent() { crate::transfer::resolve_images(&mut doc.blocks, parent); }
            doc
        };
        for source in doc.asset_sources() {
            if source.starts_with("assets/") || source.starts_with("content-assets/") || Path::new(&source).is_absolute() {
                let path = if Path::new(&source).is_absolute() { PathBuf::from(&source) } else { root.join(&source) };
                ensure!(path.is_file() && path.canonicalize()?.starts_with(root.canonicalize()?), "chapter asset is missing or outside the workspace");
            }
        }
        let title = meta.and_then(|m| field::<Localized>(m, "title").transpose()).transpose()?.unwrap_or_else(|| Localized::same(doc.title.text.trim()));
        let english_name = meta.and_then(|m| m.get("content_en_file")).and_then(Value::as_str);
        let english = if let Some(name) = english_name {
            let file = inside(root, name)?;
            let format = meta.and_then(|m| m.get("format_en")).and_then(Value::as_str).unwrap_or_else(|| {
                if file.extension().and_then(|s| s.to_str()) == Some("xml") { "docxxml" } else { "markdown" }
            });
            Some((std::fs::read_to_string(file)?, format.to_string()))
        } else {
            let mut file = path.to_path_buf();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            file.set_file_name(format!("{stem}.en.md"));
            if file.try_exists()? {
                let name = file.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                Some((std::fs::read_to_string(inside(root, &name)?)?, "markdown".to_string()))
            } else { None }
        };
        let original_id = meta.and_then(|m| m.get("id")).and_then(Value::as_str).map(str::to_string);
        inputs.push(PackInput::titled(doc, title));
        translations.push((original_id, english));
    }
    if inputs.is_empty() { bail!("book has no chapters"); }
    let mut assets = if manifest.is_some() { collect_assets(root)? } else { BTreeMap::new() };
    if let Some(mimes) = manifest.as_ref().and_then(|m| m.get("asset_mimes")).and_then(Value::as_object) {
        for (key, mime) in mimes {
            if let (Some(asset), Some(mime)) = (assets.get_mut(key), mime.as_str()) { asset.mime = mime.to_string(); }
        }
    }
    let title = manifest.as_ref().and_then(|m| field::<Localized>(m, "title").transpose()).transpose()?.unwrap_or_else(|| Localized::same(fallback_title));
    let mut result = book::pack_with_assets(inputs, &PackOptions { assets_root: Some(root.to_path_buf()), title: Some(title), ..Default::default() }, assets)?;
    if let Some(meta) = &manifest {
        if let Some(id) = field(meta, "id")? { result.id = id; }
        if let Some(subtitle) = field(meta, "subtitle")? { result.subtitle = subtitle; }
        if let Some(authors) = field(meta, "authors")? { result.authors = authors; }
        if let Some(languages) = field(meta, "languages")? { result.languages = languages; }
        if let Some(default_language) = field(meta, "default_language")? { result.default_language = default_language; }
        if let Some(cover) = field(meta, "cover")? { result.cover = Some(cover); }
        if let Some(theme) = field(meta, "theme")? { result.theme = theme; }
        if let Some(reading) = field(meta, "reading")? { result.reading = reading; }
        if let Some(created) = field(meta, "created")? { result.created = created; }
    }
    for (chapter, (id, english)) in result.chapters.iter_mut().zip(translations) {
        if let Some(id) = id { chapter.id = id; }
        if let Some((body, format)) = english {
            if !matches!(format.as_str(), "markdown" | "docxxml") { bail!("unknown translation format"); }
            chapter.content_en = Some(body);
            if format != chapter.format { chapter.format_en = Some(format); result.version = 2; }
        }
    }
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_manifest_translations_and_assets_without_losing_metadata() {
        let root = std::env::temp_dir().join(format!("markdownx-book-folder-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/picture.png"), b"picture").unwrap();
        let chapter = root.join("01-start.md");
        std::fs::write(&chapter, "# 中文\n\n正文\n").unwrap();
        std::fs::write(root.join("01-start.xml"), "<title>中文</title><img path=\"assets/picture.png\"/>").unwrap();
        std::fs::write(root.join("01-start.en.md"), "# English\n\n![picture](assets/picture.png)\n").unwrap();
        let meta = serde_json::json!({"format":"markdownx.book", "version":1, "id":"original-book",
            "title":{"zh":"中文书名","en":"English book"}, "subtitle":{"zh":"副标题","en":"Subtitle"},
            "authors":["Author"], "languages":["zh","en"], "default_language":"en", "asset_mimes":{"assets/picture.png":"image/custom"}, "chapters":[{
                "id":"chapter-original", "title":{"zh":"中文","en":"English"}, "file":"01-start.md",
                "source":"01-start.xml", "content_en_file":"01-start.en.md", "format_en":"markdown"}]
        });
        std::fs::write(root.join("book.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        let result = pack(&root, "fallback", std::slice::from_ref(&chapter)).unwrap();
        assert_eq!(result.version, 2);
        assert_eq!(result.id, "original-book");
        assert_eq!(result.title, Localized::new("中文书名", "English book"));
        assert_eq!(result.default_language, "en");
        assert_eq!(result.chapters[0].id, "chapter-original");
        assert_eq!(result.chapters[0].format_en.as_deref(), Some("markdown"));
        assert!(result.chapters[0].content_en.as_deref().unwrap().contains("English"));
        assert!(result.chapters[0].content.contains("assets/picture.png"));
        assert_eq!(result.assets.len(), 1);
        assert_eq!(result.assets["assets/picture.png"].mime, "image/custom");
        let mut bad = meta;
        bad["chapters"][0]["content_en_file"] = serde_json::json!("../outside.md");
        std::fs::write(root.join("book.json"), serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(pack(&root, "fallback", &[chapter]).is_err());
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
