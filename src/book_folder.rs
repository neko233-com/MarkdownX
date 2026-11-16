//! Package a workspace folder, respecting its multilingual book manifest.

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use fdl_core::book::{self, Asset, Localized, PackInput, PackOptions};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Preserve translation source while moving image references into the shared
/// book asset map. Code examples and interactive HTML are handled separately.
fn translation_images(text: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    use crate::model::{Block, BlockKind, Document, RichText};
    let root_canon = root.canonicalize()?;
    let base = file.parent().context("translation has no parent directory")?;
    let mut refs: Vec<(std::ops::Range<usize>, String, PathBuf, String)> = vec![];
    let mut markdown_refs = vec![];
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let (blocks, spans) = crate::paste::markdown_file_with_spans(text);
    let mut protected = vec![false; lines.len()];
    let mut widget_cells = vec![vec![]; lines.len()];
    for (block, span) in blocks.iter().zip(spans) {
        if matches!(block.kind, BlockKind::Code { .. } | BlockKind::Interactive(_)) {
            for index in span { if let Some(value) = protected.get_mut(index) { *value = true; } }
        } else if matches!(block.kind, BlockKind::Table(_)) {
            for index in span {
                if let Some(line) = lines.get(index) { widget_cells[index] = crate::paste::markdown_table_widgets(line).into_iter().map(|(range, _)| range).collect(); }
            }
        }
    }
    let math_ranges: Vec<_> = pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::ENABLE_MATH | pulldown_cmark::Options::ENABLE_TABLES | pulldown_cmark::Options::ENABLE_FOOTNOTES)
        .into_offset_iter().filter_map(|(event, range)| matches!(event, pulldown_cmark::Event::InlineMath(_) | pulldown_cmark::Event::DisplayMath(_)).then_some(range)).collect();
    let mut html_mask = Vec::with_capacity(text.len());
    let mut offset = 0;
    let mut record = |range: std::ops::Range<usize>, raw: &str| -> Result<()> {
        let lower = raw.to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("data:") { return Ok(()); }
        let view = fdl_core::html_assets::svg_fragment(raw);
        ensure!(!raw.contains('#') || view.is_some(), "unsupported local image reference {raw}");
        let (path, fragment) = view.unwrap_or((raw.split_once('?').map_or(raw, |(path, _)| path), ""));
        ensure!(!path.contains(':'), "unsupported local image reference {raw}");
        let decoded = crate::transfer::percent_decode(path);
        let mut path = base.join(&decoded);
        let key = decoded.trim_start_matches("./");
        if !path.is_file() && !decoded.starts_with("./") && key.starts_with("assets/") && assets.contains_key(key) { path = root.join(key); }
        let path = path.canonicalize().with_context(|| format!("translation image is missing: {raw}"))?;
        ensure!(path.starts_with(&root_canon) && path.is_file(), "translation image is outside the book folder: {raw}");
        refs.push((range, decoded, path, fragment.to_string()));
        Ok(())
    };
    for (index, line) in lines.iter().enumerate() {
        let mut masked = line.as_bytes().to_vec();
        if protected[index] || crate::paste::is_markdown_reference_definition(line) {
            for byte in &mut masked { if !matches!(*byte, b'\r' | b'\n') { *byte = b' '; } }
        } else {
            for range in &widget_cells[index] { masked[range.clone()].fill(b' '); }
            mask_inline_code(line, &mut masked);
            for range in crate::paste::markdown_inline_math_spans(line) { masked[range].fill(b' '); }
            let first = math_ranges.partition_point(|range| range.end <= offset);
            for range in math_ranges[first..].iter().take_while(|range| range.start < offset + line.len()) {
                let start = range.start.saturating_sub(offset).min(line.len());
                let end = range.end.saturating_sub(offset).min(line.len());
                for byte in &mut masked[start..end] { if !matches!(*byte, b'\r' | b'\n') { *byte = b' '; } }
            }
            for span in crate::paste::markdown_image_spans(line) {
                let position = offset + span.start;
                if widget_cells[index].iter().any(|range| range.contains(&span.start))
                    || math_ranges.get(math_ranges.partition_point(|range| range.end <= position)).is_some_and(|range| range.contains(&position)) { continue; }
                if let Some(destination) = crate::paste::markdown_image_destination_span(&line[span.clone()]) {
                    let range = offset + span.start + destination.start..offset + span.start + destination.end;
                    let raw = crate::paste::markdown_image_unescape(&text[range.clone()]);
                    markdown_refs.push((range, raw));
                }
                masked[span].fill(b' ');
            }
        }
        html_mask.extend(masked);
        offset += line.len();
    }
    let masked = String::from_utf8(html_mask).expect("masked Markdown remains UTF-8");
    let nontext = html_nontext_ranges(&masked);
    for (range, raw) in markdown_refs {
        if !nontext.iter().any(|region| region.contains(&range.start)) { record(range, &raw)?; }
    }
    for reference in fdl_core::html_assets::references(&masked) {
        let escaped = masked[..reference.tag_start].bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1;
        if !escaped && reference.tag == "img" && reference.attribute == "src" { record(reference.range, &reference.value)?; }
    }
    refs.sort_by_key(|(range, _, _, _)| range.start);
    if refs.is_empty() { return Ok(text.to_string()); }
    let blocks = refs.iter().map(|(_, _, path, fragment)| Block::new(BlockKind::Image {
        src_kind: "path".into(), src: format!("{}{fragment}", path.to_string_lossy()), width: None, height: None, caption: None, crop: None,
    }, RichText::default())).collect();
    let doc = Document { blocks, ..Default::default() };
    let packed = book::pack_with_assets(vec![PackInput::new(doc).with_asset_base(file.parent().context("translation has no parent directory")?.to_path_buf())], &PackOptions { assets_root: Some(root.to_path_buf()), ..Default::default() }, std::mem::take(assets))?;
    let rewritten = crate::docxml::parse(&packed.chapters[0].content)?;
    *assets = packed.assets;
    ensure!(rewritten.blocks.len() == refs.len(), "could not package translation images");
    let mut out = text.to_string();
    for ((range, original, _, _), block) in refs.into_iter().zip(rewritten.blocks).rev() {
        let BlockKind::Image { src, .. } = block.kind else { bail!("could not package translation image") };
        let (key, fragment) = fdl_core::html_assets::svg_fragment(&src).unwrap_or((&src, ""));
        ensure!(assets.contains_key(key), "translation image could not be embedded");
        if original.trim_start_matches("./") != src {
            out.replace_range(range, &format!("./{}{}", markdown_asset_url(key), markdown_fragment_url(fragment)));
        }
    }
    Ok(out)
}

/// Package media attributes in HTML widgets in translated Markdown, leaving
/// other fenced examples and the original translation file untouched.
fn package_translation_html(source: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    use crate::model::{Block, BlockKind, Document, Interactive, RichText};
    let mut doc = Document { blocks: vec![Block::new(BlockKind::Interactive(Interactive::html("", source)), RichText::default())], ..Default::default() };
    crate::transfer::resolve_book_assets(&mut doc.blocks, file.parent().context("translation has no parent directory")?, root);
    let root_canon = root.canonicalize()?;
    for source in doc.asset_sources() {
        if Path::new(&source).is_absolute() {
            ensure!(crate::store::export_path_in(&root_canon, &source).is_some(), "translation HTML media is missing or outside the book folder");
        }
    }
    let packed = book::pack_with_assets(vec![PackInput::new(doc).with_asset_base(file.parent().context("translation has no parent directory")?.to_path_buf())], &PackOptions { assets_root: Some(root.to_path_buf()), ..Default::default() }, std::mem::take(assets))?;
    let rewritten = crate::docxml::parse(&packed.chapters[0].content)?;
    *assets = packed.assets;
    let Some(Block { kind: BlockKind::Interactive(widget), .. }) = rewritten.blocks.first() else { bail!("could not package translation HTML media") };
    Ok(widget.source.clone())
}

/// Rewrite HTML media at the start of list paragraphs in translated Markdown.
/// Parser ranges exclude fenced and indented code and preserve surrounding
/// list markers and indentation when the temporary packaged source is patched.
fn translation_list_html_media(text: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut items = Vec::<bool>::new();
    let mut ranges = Vec::new();
    for (event, range) in Parser::new_ext(text, Options::ENABLE_FOOTNOTES | Options::ENABLE_TABLES | Options::ENABLE_MATH).into_offset_iter() {
        match event {
            Event::Start(Tag::Item) => items.push(false),
            Event::End(TagEnd::Item) => { items.pop(); },
            Event::Start(Tag::Paragraph | Tag::HtmlBlock) if !items.is_empty() => {
                let lower = text[range.clone()].trim_start().to_ascii_lowercase();
                let starts = |tag: &str| lower.strip_prefix(&format!("<{tag}")).is_some_and(|rest| rest.starts_with([' ', '\t', '\r', '\n', '>', '/']));
                let media = ["video", "audio", "canvas", "model-viewer"].into_iter().any(starts);
                let companion = items.last().copied().unwrap_or(false) && ["script", "style", "link"].into_iter().any(starts);
                if media || companion {
                    if media && let Some(active) = items.last_mut() { *active = true; }
                    ranges.push(range);
                }
            }
            _ => {}
        }
    }
    let mut output = text.to_string();
    for range in ranges.into_iter().rev() {
        let source = &text[range.clone()];
        if fdl_core::html_assets::references(source).is_empty() && !fdl_core::html_assets::has_inline_styles(source) { continue; }
        let updated = package_translation_html(source, file, root, assets)?;
        if updated != source { output.replace_range(range, &updated); }
    }
    Ok(output)
}

fn translation_html_media(text: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    use crate::model::{BlockKind, InteractiveKind};
    if text.as_bytes().iter().enumerate().any(|(i, c)| *c == b'\r' && text.as_bytes().get(i + 1) != Some(&b'\n')) {
        return translation_html_media(&text.replace("\r\n", "\n").replace('\r', "\n"), file, root, assets);
    }
    let nested = translation_list_html_media(text, file, root, assets)?;
    let text = nested.as_str();
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let (blocks, spans) = crate::paste::markdown_file_with_spans(text);
    let mut tables = std::collections::HashSet::new();
    for (block, span) in blocks.iter().zip(&spans) {
        if matches!(block.kind, BlockKind::Table(_)) { tables.extend(span.clone()); }
    }
    let raw: std::collections::HashMap<_, _> = blocks.into_iter().zip(spans).filter_map(|(block, span)| {
        let BlockKind::Interactive(widget) = block.kind else { return None };
        if widget.kind != InteractiveKind::Html || matches!(lines.get(span.start)?.trim_start().chars().next(), Some('`' | '~')) { return None; }
        Some((span.start, (span.end, widget.source)))
    }).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if tables.contains(&i) {
            let mut line = lines[i].to_string();
            for (range, mut widget) in crate::paste::markdown_table_widgets(lines[i]).into_iter().rev() {
                if widget.kind != InteractiveKind::Html { continue; }
                let updated = package_translation_html(&widget.source, file, root, assets)?;
                if updated == widget.source { continue; }
                let encoded = lines[i][range.clone()].starts_with("<markdownx-interactive ");
                widget.source = updated;
                let replacement = if encoded { crate::paste::markdown_table_widget(&widget) } else { widget.source.replace('|', "\\|") };
                line.replace_range(range, &replacement);
            }
            out.push_str(&line);
            i += 1;
            continue;
        }
        if let Some((end, source)) = raw.get(&i) {
            if fdl_core::html_assets::references(source).is_empty() && !fdl_core::html_assets::has_inline_styles(source) { out.push_str(&lines[i..*end].concat()); }
            else {
                let rewritten = package_translation_html(source, file, root, assets)?;
                if rewritten == *source { out.push_str(&lines[i..*end].concat()); }
                else {
                    out.push_str(&rewritten.replace('\n', if lines[i].ends_with("\r\n") { "\r\n" } else { "\n" }));
                    if lines[*end - 1].ends_with('\n') { out.push_str(if lines[*end - 1].ends_with("\r\n") { "\r\n" } else { "\n" }); }
                }
            }
            i = *end;
            continue;
        }
        let trimmed = lines[i].trim_start_matches([' ', '\t']);
        let first = trimmed.as_bytes().first().copied();
        let count = first.filter(|c| *c == b'`' || *c == b'~').map_or(0, |ch| trimmed.bytes().take_while(|c| *c == ch).count());
        if count < 3 { out.push_str(lines[i]); i += 1; continue; }
        let mut end = i + 1;
        while end < lines.len() {
            let line = lines[end].trim_start_matches([' ', '\t']);
            let run = line.bytes().take_while(|c| Some(*c) == first).count();
            if run >= count && line[run..].trim().is_empty() { break; }
            end += 1;
        }
        let raw = lines[i + 1..end].concat();
        out.push_str(lines[i]);
        let is_html = trimmed[count..].trim().eq_ignore_ascii_case("mdxb-html");
        let mut payload = if is_html { serde_json::from_str::<Value>(&raw).ok() } else { None };
        if let Some(source) = payload.as_ref().and_then(|v| v.get("source")).and_then(Value::as_str)
            && (!fdl_core::html_assets::references(source).is_empty() || fdl_core::html_assets::has_inline_styles(source))
        {
            let rewritten = package_translation_html(source, file, root, assets)?;
            if rewritten == source { out.push_str(&raw); }
            else {
                payload.as_mut().unwrap()["source"] = Value::String(rewritten);
                out.push_str(&serde_json::to_string(payload.as_ref().unwrap())?);
                out.push_str(if lines[i].ends_with("\r\n") { "\r\n" } else { "\n" });
            }
        } else { out.push_str(&raw); }
        if end < lines.len() { out.push_str(lines[end]); }
        i = end + 1;
    }
    Ok(out)
}

pub(crate) fn markdown_asset_url(key: &str) -> String {
    let mut out = String::new();
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') { out.push(byte as char); }
        else { use std::fmt::Write; let _ = write!(out, "%{byte:02X}"); }
    }
    out
}

fn markdown_fragment_url(fragment: &str) -> String {
    let mut out = String::new();
    for byte in fragment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'#' | b'%' | b'/' | b'-' | b'_' | b'.' | b'~' | b'(' | b')' | b',' | b';' | b'=' | b':') { out.push(byte as char); }
        else { use std::fmt::Write; let _ = write!(out, "%{byte:02X}"); }
    }
    out
}

fn mask_inline_code(line: &str, masked: &mut [u8]) {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\' { at = (at + 2).min(bytes.len()); continue; }
        if bytes[at] != b'`' { at += 1; continue; }
        let len = bytes[at..].iter().take_while(|b| **b == b'`').count();
        let mut end = at + len;
        let mut closed = None;
        while end < bytes.len() {
            if bytes[end] != b'`' { end += 1; continue; }
            let run = bytes[end..].iter().take_while(|b| **b == b'`').count();
            if run == len { closed = Some(end); break; }
            end += run;
        }
        if let Some(end) = closed { masked[at..end + len].fill(b' '); at = end + len; }
        else { at += len; }
    }
}

/// HTML tags, comments and raw-text elements in the masked Markdown; image
/// syntax within those regions is literal rather than a Markdown reference.
fn html_nontext_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let mut ranges = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' { i += 1; continue; }
        if bytes[i..].starts_with(b"<!--") {
            let end = text[i + 4..].find("-->").map_or(bytes.len(), |end| i + 4 + end + 3);
            ranges.push(i..end); i = end; continue;
        }
        let mut p = i + 1;
        let closing = bytes.get(p) == Some(&b'/');
        if closing { p += 1; }
        let name = p;
        while bytes.get(p).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':')) { p += 1; }
        if p == name || !bytes[name].is_ascii_alphabetic() { i += 1; continue; }
        let tag = &lower[name..p];
        let mut quote = None;
        while p < bytes.len() {
            match (quote, bytes[p]) {
                (Some(q), c) if c == q => quote = None,
                (None, b'\'' | b'"') => quote = Some(bytes[p]),
                (None, b'>') => { p += 1; break; }
                _ => {}
            }
            p += 1;
        }
        if !closing && matches!(tag, "script" | "style" | "textarea" | "title") {
            let close = format!("</{tag}>");
            p = lower[p..].find(&close).map_or(bytes.len(), |end| p + end + close.len());
        }
        ranges.push(i..p); i = p;
    }
    ranges
}

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
                "pdf" => "application/pdf", "mp4" | "m4v" => "video/mp4", "mov" => "video/quicktime", "webm" => "video/webm", "ogv" => "video/ogg",
                "mp3" => "audio/mpeg", "wav" => "audio/wav", "ogg" | "opus" => "audio/ogg", "m4a" => "audio/mp4", "aac" => "audio/aac", "flac" => "audio/flac", "vtt" => "text/vtt",
                "js" | "mjs" => "text/javascript", "css" => "text/css",
                "obj" => "model/obj", "stl" => "model/stl", "gltf" => "model/gltf+json", "glb" => "model/gltf-binary", _ => "application/octet-stream",
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

fn validate_document_assets(doc: &crate::model::Document, root: &Path) -> Result<()> {
    let mut sources = doc.asset_sources();
    if let Some(cover) = &doc.cover { sources.push(cover.src.clone()); }
    for source in sources {
        if source.starts_with("assets/") || source.starts_with("content-assets/") || Path::new(&source).is_absolute() {
            ensure!(crate::store::export_path_in(root, &source).is_some(), "chapter asset is missing or outside the workspace: {source}");
        }
    }
    Ok(())
}

fn package_translation_xml(body: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    let base = file.parent().context("translation has no parent directory")?;
    let mut doc = crate::docxml::parse(body)?;
    crate::transfer::resolve_book_document_assets(&mut doc, base, root);
    validate_document_assets(&doc, root)?;
    let packed = book::pack_with_assets(vec![PackInput::new(doc).with_asset_base(base.to_path_buf())],
        &PackOptions { assets_root: Some(root.to_path_buf()), embed_linked_pdfs: true, ..Default::default() }, std::mem::take(assets))?;
    *assets = packed.assets;
    Ok(packed.chapters[0].content.clone())
}

fn translation_pdf_links(text: &str, file: &Path, root: &Path, assets: &mut BTreeMap<String, Asset>) -> Result<String> {
    use crate::model::{Block, BlockKind, Document, RichText};
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
    let mut refs = Vec::new();
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_MATH).into_offset_iter() {
        if let Event::Start(Tag::Link { link_type, dest_url, title, .. }) = event
            && book::pdf_link_target(&dest_url).is_some()
        { refs.push((range, dest_url.to_string(), title.to_string(), link_type)); }
    }
    if refs.is_empty() { return Ok(text.to_string()); }
    let blocks = refs.iter().map(|(_, target, _, _)| {
        let mut rich = RichText::plain("link"); rich.runs[0].marks.link = Some(target.clone());
        Block::new(BlockKind::Text, rich)
    }).collect();
    let base = file.parent().context("translation has no parent directory")?;
    let packed = book::pack_with_assets(vec![PackInput::new(Document { blocks, ..Default::default() }).with_asset_base(base.to_path_buf())],
        &PackOptions { assets_root: Some(root.to_path_buf()), embed_linked_pdfs: true, ..Default::default() }, std::mem::take(assets))?;
    let doc = crate::docxml::parse(&packed.chapters[0].content)?;
    *assets = packed.assets;
    ensure!(doc.blocks.len() == refs.len(), "could not package translated PDF links");
    let mut output = text.to_string();
    for ((range, _, title, kind), block) in refs.into_iter().zip(doc.blocks).rev() {
        let target = block.text.runs.iter().find_map(|run| run.marks.link.as_deref()).context("packaged PDF link is missing")?;
        let (path, fragment) = target.split_once('#').map_or((target, ""), |(path, fragment)| (path, fragment));
        let dest = format!("./{}{}", markdown_asset_url(path), if fragment.is_empty() { String::new() } else { format!("#{fragment}") });
        let raw = &text[range.clone()];
        let replacement = if matches!(kind, LinkType::Inline) {
            let probe = format!("!{raw}");
            let dest_range = crate::paste::markdown_image_destination_span(&probe).context("could not locate PDF link destination")?;
            ensure!(dest_range.start > 0, "invalid PDF link range");
            let mut replacement = raw.to_string();
            replacement.replace_range(dest_range.start - 1..dest_range.end - 1, &dest);
            replacement
        } else {
            let mut depth = 0usize;
            let mut escaped = false;
            let mut end = None;
            for (at, byte) in raw.bytes().enumerate() {
                if escaped { escaped = false; continue; }
                if byte == b'\\' { escaped = true; continue; }
                if byte == b'[' { depth += 1; }
                if byte == b']' { depth = depth.saturating_sub(1); if depth == 0 { end = Some(at + 1); break; } }
            }
            let label = raw.get(..end.context("invalid PDF reference label")?).context("invalid PDF reference range")?;
            let title = if title.is_empty() { String::new() } else { format!(" \"{}\"", title.replace('\\', "\\\\").replace('"', "\\\"")) };
            format!("{label}({dest}{title})")
        };
        output.replace_range(range, &replacement);
    }
    Ok(output)
}

fn package_manifest_cover(result: &mut book::Book, root: &Path) -> Result<()> {
    let Some(src) = result.cover.as_deref() else { return Ok(()) };
    if src.is_empty() || src.starts_with("http://") || src.starts_with("https://") || result.data_uri(src).is_some() { return Ok(()); }
    let mut doc = crate::model::Document { cover: Some(crate::model::Cover { src: src.to_string(), x: 0., y: 0. }), ..Default::default() };
    crate::transfer::resolve_book_document_assets(&mut doc, root, root);
    validate_document_assets(&doc, root)?;
    let packed = book::pack_with_assets(vec![PackInput::new(doc)], &PackOptions { assets_root: Some(root.to_path_buf()), ..Default::default() }, std::mem::take(&mut result.assets))?;
    let doc = crate::docxml::parse(&packed.chapters[0].content)?;
    let cover = doc.cover.context("book cover could not be packaged")?.src;
    ensure!(packed.data_uri(&cover).is_some(), "book cover could not be embedded");
    result.assets = packed.assets;
    result.cover = Some(cover);
    Ok(())
}

pub fn pack(root: &Path, fallback_title: &str, files: &[PathBuf]) -> Result<book::Book> {
    let manifest_path = root.join("book.json");
    let manifest = if manifest_path.try_exists()? {
        ensure!(std::fs::metadata(&manifest_path)?.len() <= 8 * 1024 * 1024, "book manifest is too large");
        let value: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
        (value.get("format").and_then(Value::as_str) == Some(book::FORMAT)).then_some(value)
    } else { None };
    let metas = manifest.as_ref().and_then(|m| m.get("chapters")).and_then(Value::as_array).cloned().unwrap_or_default();
    let selected: std::collections::HashSet<String> = files.iter().filter_map(|path| path.strip_prefix(root).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/")).collect();
    let mut secondary = std::collections::HashSet::new();
    let primary: std::collections::HashSet<&str> = metas.iter().filter_map(|meta| meta.get("file").and_then(Value::as_str)).collect();
    for meta in &metas {
        if let Some(name) = meta.get("content_en_file").and_then(Value::as_str) { secondary.insert(name.to_string()); }
        if let (Some(file), Some(source)) = (meta.get("file").and_then(Value::as_str), meta.get("source").and_then(Value::as_str))
            && file != source && selected.contains(file) { secondary.insert(source.to_string()); }
    }
    let mut inputs = Vec::new();
    let mut translations = Vec::new();
    for path in files {
        let relative = path.strip_prefix(root).context("chapter is outside the workspace")?.to_string_lossy().replace('\\', "/");
        if secondary.contains(&relative) && !primary.contains(relative.as_str()) { continue; }
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
        let (doc, base) = if let Some(xml_path) = xml_path.filter(|_| xml_is_newer) {
            let mut doc = crate::docxml::parse(&std::fs::read_to_string(&xml_path)?)?;
            if let Some(parent) = xml_path.parent() { crate::transfer::resolve_book_document_assets(&mut doc, parent, root); }
            let base = xml_path.parent().context("chapter has no parent directory")?.to_path_buf();
            (doc, base)
        } else {
            (crate::markdown_source::parse_for_book(path, &text, root).0, path.parent().context("chapter has no parent directory")?.to_path_buf())
        };
        validate_document_assets(&doc, root)?;
        let title_override = meta.and_then(|m| field::<Localized>(m, "title").transpose()).transpose()?;
        let explicit_title = title_override.as_ref().is_some_and(|title| !title.en.trim().is_empty());
        let title = title_override.unwrap_or_else(|| Localized::same(doc.title.plain_text().trim()));
        let english_name = meta.and_then(|m| m.get("content_en_file")).and_then(Value::as_str);
        let english = if let Some(name) = english_name {
            let file = inside(root, name)?;
            let format = meta.and_then(|m| m.get("format_en")).and_then(Value::as_str).unwrap_or_else(|| {
                if file.extension().and_then(|s| s.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("docxxml")) { "docxxml" } else { "markdown" }
            });
            Some((std::fs::read_to_string(&file)?, format.to_string(), file))
        } else {
            let mut file = path.to_path_buf();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            file.set_file_name(format!("{stem}.en.md"));
            if file.try_exists()? {
                let name = file.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                let file = inside(root, &name)?;
                Some((std::fs::read_to_string(&file)?, "markdown".to_string(), file))
            } else { None }
        };
        let original_id = meta.and_then(|m| m.get("id")).and_then(Value::as_str).map(str::to_string);
        let source_path = meta.and_then(|m| m.get("source_path").or_else(|| m.get("file"))).and_then(Value::as_str).unwrap_or(&relative).to_string();
        let source_en_path = english.as_ref().map(|(_, _, file)| {
            meta.and_then(|m| m.get("source_en_path")).and_then(Value::as_str).map(str::to_string)
                .unwrap_or_else(|| file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/"))
        });
        inputs.push(PackInput::titled(doc, title).with_asset_base(base));
        translations.push((original_id, english, source_path, source_en_path, explicit_title));
    }
    if inputs.is_empty() { bail!("book has no chapters"); }
    let mut assets = if manifest.is_some() { collect_assets(root)? } else { BTreeMap::new() };
    if let Some(mimes) = manifest.as_ref().and_then(|m| m.get("asset_mimes")).and_then(Value::as_object) {
        for (key, mime) in mimes {
            if let (Some(asset), Some(mime)) = (assets.get_mut(key), mime.as_str()) { asset.mime = mime.to_string(); }
        }
    }
    let title = manifest.as_ref().and_then(|m| field::<Localized>(m, "title").transpose()).transpose()?.unwrap_or_else(|| Localized::same(fallback_title));
    let mut result = book::pack_with_assets(inputs, &PackOptions { assets_root: Some(root.to_path_buf()), title: Some(title), embed_linked_pdfs: true, ..Default::default() }, assets)?;
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
    for (chapter, (id, english, source_path, source_en_path, explicit_title)) in result.chapters.iter_mut().zip(translations) {
        if let Some(id) = id { chapter.id = id; }
        chapter.source_path = Some(source_path);
        chapter.source_en_path = source_en_path;
        if let Some((body, format, file)) = english {
            if !matches!(format.as_str(), "markdown" | "docxxml") { bail!("unknown translation format"); }
            if !explicit_title {
                let title = if format == "docxxml" { crate::docxml::parse(&body)?.title.plain_text() }
                    else {
                        let (blocks, _) = crate::paste::markdown_file_with_spans(&body);
                        blocks.first().filter(|block| matches!(block.kind, crate::model::BlockKind::Heading { level: 1, .. }))
                            .map(|block| block.text.plain_text()).unwrap_or_default()
                    };
                if !title.trim().is_empty() { chapter.title.en = title.trim().to_string(); }
            }
            chapter.content_en = Some(if format == "markdown" {
                let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
                let expanded = crate::paste::expand_reference_images_for_pack(&normalized);
                let links = translation_pdf_links(&expanded, &file, root, &mut result.assets)?;
                let images = translation_images(&links, &file, root, &mut result.assets)?;
                translation_html_media(&images, &file, root, &mut result.assets)?
            } else { package_translation_xml(&body, &file, root, &mut result.assets)? });
            if format != chapter.format { chapter.format_en = Some(format); result.version = 2; }
        }
    }
    package_manifest_cover(&mut result, root)?;
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_indented_code_without_loading_example_images() {
        let root = std::env::temp_dir().join(format!("markdownx-book-indented-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("paper.md");
        let source = "# Paper\n\n    ![example](missing.png)\n\n![real](./real.svg)\n\n[^n]: note\n\n    ![footnote](real.svg)\n";
        std::fs::write(&file, source).unwrap();
        std::fs::write(root.join("real.svg"), b"<svg/>").unwrap();
        let book = pack(&root, "Paper", std::slice::from_ref(&file)).unwrap();
        let doc = crate::docxml::parse(&book.chapters[0].content).unwrap();
        assert!(doc.blocks.iter().any(|b| matches!(b.kind, crate::model::BlockKind::Code { .. }) && b.text.text == "![example](missing.png)"));
        assert_eq!(doc.blocks.iter().filter(|b| matches!(b.kind, crate::model::BlockKind::Image { .. })).count(), 2);
        assert_eq!(book.assets.len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stylesheet_imports_and_images_are_packaged_without_changing_sources() {
        let root = std::env::temp_dir().join(format!("mdx-css-dependencies-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("chapter/styles")).unwrap();
        std::fs::create_dir_all(root.join("chapter/images")).unwrap();
        std::fs::write(root.join("chapter/images/frame.svg"), b"<svg/>").unwrap();
        std::fs::write(root.join("chapter/styles/theme.css"), "canvas { color: red }").unwrap();
        let style = root.join("chapter/styles/game");
        let content = "@import './theme.css'; canvas { background: url('../images/frame.svg') }";
        std::fs::write(&style, content).unwrap();
        let chapter = root.join("chapter/game.md");
        std::fs::write(&chapter, "# Game\n\n<canvas></canvas>\n<link rel='stylesheet' href='./styles/game'>\n").unwrap();
        let result = pack(&root, "Game", std::slice::from_ref(&chapter)).unwrap();
        let css = result.assets.values().find(|asset| asset.mime == "text/css").unwrap();
        let css = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&css.data).unwrap()).unwrap();
        assert!(css.contains("data:text/css;base64,") && css.contains("data:image/svg+xml;base64,PHN2Zy8+"));
        assert_eq!(std::fs::read_to_string(&style).unwrap(), content);
        std::fs::remove_file(root.join("chapter/images/frame.svg")).unwrap();
        assert!(pack(&root, "Game", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_css_uses_each_chapter_and_translation_directory() {
        let root = std::env::temp_dir().join(format!("mdx-inline-css-book-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("zh")).unwrap();
        std::fs::create_dir_all(root.join("en")).unwrap();
        std::fs::write(root.join("frame.svg"), b"wrong root").unwrap();
        std::fs::write(root.join("zh/frame.svg"), b"zh image").unwrap();
        std::fs::write(root.join("en/frame.svg"), b"en image").unwrap();
        let chapter = root.join("zh/story.md");
        let source = "# Story\n\n<canvas style=\"background: url('./frame.svg')\"></canvas>\n<style>canvas { mask-image: url(frame.svg) }</style>\n";
        std::fs::write(&chapter, source).unwrap();
        let english = root.join("en/story.md");
        let translation = "# Story\n\n```mdxb-html\n{\"source\":\"<canvas style=\\\"background:url('./frame.svg')\\\"></canvas>\"}\n```\n";
        std::fs::write(&english, translation).unwrap();
        std::fs::write(root.join("book.json"), r#"{"format":"markdownx.book","chapters":[{"file":"zh/story.md","content_en_file":"en/story.md","format_en":"markdown"}]}"#).unwrap();
        let result = pack(&root, "Story", std::slice::from_ref(&chapter)).unwrap();
        assert!(result.chapters[0].content.contains("data:image/svg+xml;base64,emggaW1hZ2U="));
        assert!(result.chapters[0].content_en.as_deref().unwrap().contains("data:image/svg+xml;base64,ZW4gaW1hZ2U="));
        assert_eq!(std::fs::read_to_string(&chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(&english).unwrap(), translation);
        std::fs::remove_file(root.join("en/frame.svg")).unwrap();
        assert!(pack(&root, "Story", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundled_storyboard_demo_packages_its_real_video_and_images() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("MarkdownX-Demo");
        let chapter = root.join("分镜与互动演示.md");
        let before = std::fs::read(&chapter).unwrap();
        let result = pack(&root, "Demo", std::slice::from_ref(&chapter)).unwrap();
        assert!(result.assets.values().any(|asset| asset.mime == "text/javascript"
            && String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap()).is_ok_and(|script| script.contains("mountSlate") && script.contains("data:text/javascript;base64,"))));
        assert!(result.assets.values().any(|asset| asset.mime == "video/mp4" && base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap() == std::fs::read(root.join("示例视频.mp4")).unwrap()));
        assert!(result.assets.values().any(|asset| asset.mime == "audio/wav" && base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap() == std::fs::read(root.join("示例音频.wav")).unwrap()));
        assert!(result.assets.values().any(|asset| asset.mime == "model/obj" && base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap() == std::fs::read(root.join("示例模型.obj")).unwrap()));
        assert!(result.assets.values().any(|asset| asset.mime == "text/javascript" && base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap() == std::fs::read(root.join("storyboard-game.js")).unwrap()));
        assert!(result.assets.values().filter(|asset| asset.mime == "text/css").any(|asset| {
            let css = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap()).unwrap();
            css.contains("data:text/css;base64,") && css.contains("data:image/svg+xml;base64,")
        }));
        assert_eq!(std::fs::read(chapter).unwrap(), before);
    }

    #[test]
    fn chinese_numbered_lists_keep_nested_images_when_packaged() {
        let root = std::env::temp_dir().join(format!("markdownx-legal-list-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("case.md");
        let source = "# Case\n\n3。 Parent\n   - Child\n     ![frame](./shot.svg)\n\n4。 Next\n";
        std::fs::write(&file, source).unwrap();
        std::fs::write(root.join("shot.svg"), b"<svg/>").unwrap();
        let book = pack(&root, "Case", std::slice::from_ref(&file)).unwrap();
        let doc = crate::docxml::parse(&book.chapters[0].content).unwrap();
        assert!(matches!(doc.blocks[0].kind, crate::model::BlockKind::Ordered { seq: Some(3) }));
        let child = &doc.blocks[0].children[0];
        assert!(matches!(child.kind, crate::model::BlockKind::Bullet));
        assert!(matches!(&child.children[0].kind, crate::model::BlockKind::Image { src, .. } if src.starts_with("assets/")));
        assert_eq!(book.assets.len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_media_and_translations_embed_local_assets_without_touching_sources() {
        let root = std::env::temp_dir().join(format!("markdownx-list-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("part")).unwrap();
        let part = root.join("part");
        std::fs::write(part.join("clip.mp4"), b"local video").unwrap();
        std::fs::write(part.join("sound.wav"), b"local audio").unwrap();
        std::fs::write(part.join("game.js"), "document.body.dataset.ready='yes';").unwrap();
        let source = "# Story\n\n- <video controls>\n  <source src=\"./clip.mp4\">\n  </video>\n";
        let translated = "# English\n\n- <audio controls>\n  <source src=\"sound.wav\">\n  </audio>\n\n- <canvas></canvas>\n  <script src=\"./game.js\"></script>\n\n      - <video src=\"missing.mp4\"></video>\n";
        let chapter = part.join("story.md");
        let english = part.join("story.en.md");
        std::fs::write(&chapter, source).unwrap();
        std::fs::write(&english, translated).unwrap();
        let book = pack(&root, "Story", std::slice::from_ref(&chapter)).unwrap();
        let doc = crate::docxml::parse(&book.chapters[0].content).unwrap();
        assert!(doc.blocks.iter().flat_map(|b| &b.children).any(|b| matches!(&b.kind, crate::model::BlockKind::Interactive(widget) if widget.source.contains("assets/clip.mp4"))));
        let body = book.chapters[0].content_en.as_deref().unwrap();
        assert!(body.contains("assets/sound.wav") && body.contains("assets/game.js"), "{body}");
        assert!(body.contains("missing.mp4"));
        assert_eq!(book.assets.len(), 3);
        assert_eq!(std::fs::read_to_string(chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(english).unwrap(), translated);
        std::fs::remove_file(part.join("sound.wav")).unwrap();
        assert!(pack(&root, "Story", &[part.join("story.md")]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chapter_html_media_is_local_first_and_rejects_missing_files() {
        let root = std::env::temp_dir().join(format!("mdx-html-media-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(&part).unwrap();
        std::fs::write(root.join("clip.mp4"), b"wrong file").unwrap();
        std::fs::write(part.join("clip.mp4"), b"local video").unwrap();
        let chapter = part.join("story.md");
        let source = "# Story\n\n```mdxb-html\n{\"source\":\"<video src='clip.mp4' controls></video>\"}\n```\n";
        std::fs::write(&chapter, source).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        assert_eq!(result.assets.len(), 1);
        let asset = result.assets.values().next().unwrap();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap(), b"local video");
        assert!(result.chapters[0].content.contains("assets/clip.mp4"));
        assert_eq!(std::fs::read_to_string(&chapter).unwrap(), source);
        std::fs::remove_file(part.join("clip.mp4")).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn implicit_asset_paths_prefer_chapter_then_legacy_root() {
        let root = std::env::temp_dir().join(format!("mdx-model-paths-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(part.join("assets")).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(part.join("assets/model.obj"), b"chapter model").unwrap();
        std::fs::write(root.join("assets/model.obj"), b"root model").unwrap();
        let chapter = part.join("chapter.md");
        std::fs::write(&chapter, "<model-viewer src='assets/model.obj'></model-viewer>\n").unwrap();
        let data = |book: &book::Book| base64::engine::general_purpose::STANDARD.decode(&book.assets.values().next().unwrap().data).unwrap();
        assert_eq!(data(&pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap()), b"chapter model");
        std::fs::remove_file(part.join("assets/model.obj")).unwrap();
        assert_eq!(data(&pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap()), b"root model");
        std::fs::write(&chapter, "<model-viewer src='./assets/model.obj'></model-viewer>\n").unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn translated_html_media_uses_translation_directory_and_preserves_other_fences() {
        let root = std::env::temp_dir().join(format!("mdx-html-translation-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(&part).unwrap();
        std::fs::write(part.join("clip.mp4"), b"translated clip").unwrap();
        let chapter = part.join("story.md");
        std::fs::write(&chapter, "# 中文\n\n正文\n").unwrap();
        let translated = "# English\n\n```mdxb-html\n{\"source\":\"<video src='./clip.mp4?v=1#t=1,2'></video>\"}\n```\n\n<video controls src='clip.mp4#t=2,3'></video>\n\n```html\n<video src='missing.mp4'>\n```\n";
        let english = part.join("story.en.md");
        std::fs::write(&english, translated).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        let body = result.chapters[0].content_en.as_deref().unwrap();
        assert!(body.contains("assets/clip.mp4"));
        assert!(body.contains("#t=1,2"));
        assert!(body.contains("<video controls src='assets/clip.mp4#t=2,3'></video>"));
        assert!(body.contains("<video src='missing.mp4'>"));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&result.assets.values().next().unwrap().data).unwrap(), b"translated clip");
        assert_eq!(std::fs::read_to_string(&english).unwrap(), translated);
        std::fs::remove_file(part.join("clip.mp4")).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn table_media_packages_in_chapters_and_translations() {
        let root = std::env::temp_dir().join(format!("mdx-table-media-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(&part).unwrap();
        std::fs::write(part.join("clip.mp4"), b"local video").unwrap();
        std::fs::write(root.join("clip.mp4"), b"wrong video").unwrap();
        let chapter = part.join("story.md");
        let raw = "| Shot | Media |\n| --- | --- |\n| 1 | <video src='./clip.mp4'></video> |\n";
        std::fs::write(&chapter, format!("# Story\n\n{raw}")).unwrap();
        let widget = crate::model::Interactive::html("game", "<video src='clip.mp4'></video>\n<script>let s='中文 | x';</script>");
        let encoded = crate::paste::markdown_table_widget(&widget);
        let translated = format!("# English\n\n{raw}| 2 | before<br>{encoded} |\n");
        let english = part.join("story.en.md");
        std::fs::write(&english, &translated).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        let body = result.chapters[0].content_en.as_deref().unwrap();
        assert!(body.contains("<video src='assets/clip.mp4'></video>"), "{body}");
        let cells: Vec<_> = body.lines().flat_map(crate::paste::markdown_table_widgets).collect();
        assert_eq!(cells.len(), 2);
        assert!(cells[1].1.source.contains("assets/clip.mp4") && cells[1].1.source.contains("中文 | x"));
        assert!(result.assets.values().any(|asset| base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap() == b"local video"));
        assert_eq!(std::fs::read_to_string(&english).unwrap(), translated);
        std::fs::remove_file(part.join("clip.mp4")).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chapter_and_translation_math_do_not_package_literal_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("real.svg"), b"real-image").unwrap();
        let chapter = root.join("story.md");
        let translation = root.join("story.en.md");
        let source = "# Story\n\n$\\text{![fake](missing.png)}$\n\n$\\text{![ref][shot]}$\n\n$$\n\\text{![block][shot]}\n$$\n\n> $$\n> \\text{![quote][shot]}\n> $$\n\n![real](./real.svg)\n\n[shot]: missing.png\n";
        let english = "# English\n\n$\\text{![fake](missing.png)}$\n\n$\\text{<img src='missing.png'>}$\n\n$\\text{[pdf](missing.pdf#page=2)}$\n\n$$\n\\text{![block][shot]}\n$$\n\n> $$\n> \\text{![quote][shot]}\n> $$\n\n![real](./real.svg)\n\n[shot]: missing.png\n";
        std::fs::write(&chapter, source).unwrap();
        std::fs::write(&translation, english).unwrap();
        let result = pack(root, "Book", std::slice::from_ref(&chapter)).unwrap();
        let doc = crate::docxml::parse(&result.chapters[0].content).unwrap();
        let equations: Vec<_> = doc.blocks.iter().flat_map(|b| &b.text.runs).filter_map(|r| r.marks.inline.as_ref()?.equation()).collect();
        assert_eq!(equations, ["\\text{![fake](missing.png)}", "\\text{![ref][shot]}", "\\text{![block][shot]}"]);
        let text = result.chapters[0].content_en.as_deref().unwrap();
        assert!(text.contains("$\\text{![fake](missing.png)}$"), "{text}");
        assert!(text.contains("$\\text{<img src='missing.png'>}$"), "{text}");
        assert!(text.contains("$\\text{[pdf](missing.pdf#page=2)}$"), "{text}");
        assert!(text.contains("\\text{![block][shot]}"), "{text}");
        assert!(text.contains("\\text{![quote][shot]}"), "{text}");
        assert_eq!(result.assets.len(), 1);
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&result.assets.values().next().unwrap().data).unwrap(), b"real-image");
        assert_eq!(std::fs::read_to_string(chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(translation).unwrap(), english);
    }

    #[test]
    fn translation_images_follow_the_translation_directory_and_keep_code_literal() {
        let root = std::env::temp_dir().join(format!("markdownx-book-translation-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(part.join("shots")).unwrap();
        std::fs::create_dir_all(root.join("shots")).unwrap();
        std::fs::write(part.join("shots/图 1.svg"), b"local-image").unwrap();
        std::fs::write(root.join("shots/图 1.svg"), b"wrong-image").unwrap();
        let chapter = part.join("01.md");
        std::fs::write(&chapter, "# 中文\n\n正文\n").unwrap();
        let english = "# English\n\n![A](./shots/%E5%9B%BE%201.svg?v=1) and ![B](shots/%E5%9B%BE%201.svg?cache=2)\n\n`![literal](missing.png)`\n\n```md\n![example](missing.png)\n```\n";
        std::fs::write(part.join("01.en.md"), english).unwrap();
        let result = pack(&root, "book", std::slice::from_ref(&chapter)).unwrap();
        let translation = result.chapters[0].content_en.as_deref().unwrap();
        assert_eq!(translation.matches("./assets/").count(), 2);
        assert!(translation.contains("`![literal](missing.png)`"));
        assert!(translation.contains("![example](missing.png)"));
        assert_eq!(result.assets.len(), 1);
        let asset = result.assets.values().next().unwrap();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap(), b"local-image");
        assert_eq!(std::fs::read_to_string(part.join("01.en.md")).unwrap(), english);
        std::fs::write(part.join("01.en.md"), "![missing](./missing.png)").unwrap();
        assert!(pack(&root, "book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn svg_views_package_chapters_and_translations_without_changing_sources() {
        let root = std::env::temp_dir().join(format!("markdownx-book-svg-view-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(&part).unwrap();
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><view id='right' viewBox='50 0 50 50'/></svg>";
        std::fs::write(part.join("frame#take.svg"), svg).unwrap();
        let chapter = part.join("story.md");
        let source = "# Story\n\n![frame](./frame%23take.svg?v=1#right)\n\n| A | B |\n|---|---|\n| 1 | ![frame](frame%23take.svg#right) |\n";
        let translation = "# English\n\n![frame](frame%23take.svg#right)\n\n<img src='./frame%23take.svg#right'>\n";
        std::fs::write(&chapter, source).unwrap();
        std::fs::write(part.join("story.en.md"), translation).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        assert_eq!(result.assets.len(), 1);
        assert_eq!(result.chapters[0].content.matches("#right").count(), 2);
        let translated = result.chapters[0].content_en.as_ref().unwrap();
        assert_eq!(translated.matches("#right").count(), 2, "{translated}");
        assert_eq!(std::fs::read_to_string(chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(part.join("story.en.md")).unwrap(), translation);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn html_images_package_from_each_chapter_directory_without_touching_examples() {
        let root = std::env::temp_dir().join(format!("markdownx-html-image-book-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(part.join("shots")).unwrap();
        std::fs::create_dir_all(root.join("shots")).unwrap();
        std::fs::write(part.join("shots/a&b.svg"), b"chapter-image").unwrap();
        std::fs::write(root.join("shots/a&b.svg"), b"wrong-image").unwrap();
        let chapter = part.join("story.md");
        let original = "# 中文\n\n<img\n src='./shots/a&amp;b.svg' width='120' alt='封面'>\n";
        std::fs::write(&chapter, original).unwrap();
        let english = "# English\n\n<img data-src='missing.png'\n src=shots/a&amp;b.svg alt='A'>\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Left <img src='./shots/a&amp;b.svg' alt='B'> right |\n\n`<img src='missing1.png'>`\n\n<!--\n<img src='missing2.png'>\n![fake](missing3.png)\n-->\n\n```html\n<img src='missing4.png'>\n```\n\n\\<img src='missing5.png'>\n\n<a title=\"![fake](missing6.png)\">text</a>\n";
        let translation = part.join("story.en.md");
        std::fs::write(&translation, english).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        assert!(result.chapters[0].content.contains("assets/"));
        let translated = result.chapters[0].content_en.as_deref().unwrap();
        assert_eq!(translated.matches("src=./assets/").count(), 1);
        assert_eq!(translated.matches("src='./assets/").count(), 1);
        assert!(translated.contains("data-src='missing.png'"));
        assert!(translated.contains("<img src='missing2.png'>") && translated.contains("![fake](missing3.png)"));
        assert!(translated.contains("\\<img src='missing5.png'>") && translated.contains("![fake](missing6.png)"));
        assert_eq!(result.assets.len(), 1);
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&result.assets.values().next().unwrap().data).unwrap(), b"chapter-image");
        assert_eq!(std::fs::read_to_string(&chapter).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&translation).unwrap(), english);
        std::fs::remove_file(part.join("shots/a&b.svg")).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reference_images_package_in_chapters_and_translations() {
        let root = std::env::temp_dir().join(format!("markdownx-reference-images-{}", uuid::Uuid::new_v4()));
        let part = root.join("part");
        std::fs::create_dir_all(&part).unwrap();
        std::fs::write(part.join("shot.svg"), b"local-image").unwrap();
        std::fs::write(root.join("shot.svg"), b"wrong-image").unwrap();
        let chapter = part.join("story.md");
        let source = "# 中文\n\n![封面][shot]\n\nSee [paper][source].\n\n[shot]: ./shot.svg\n[source]: https://example.test/paper\n";
        let english = "# English\n\nBefore ![frame][shot] after\n\nSee [paper][source].\n\n[shot]: shot.svg\n[source]: https://example.test/paper\n[spare]: ./shot.svg \"![example](missing.png)\"\n\n`![literal][shot]`\n";
        std::fs::write(&chapter, source).unwrap();
        std::fs::write(part.join("story.en.md"), english).unwrap();
        let result = pack(&root, "Book", std::slice::from_ref(&chapter)).unwrap();
        assert!(result.chapters[0].content.contains("assets/"));
        assert!(result.chapters[0].content.contains("https://example.test/paper"));
        let translated = result.chapters[0].content_en.as_deref().unwrap();
        assert!(translated.contains("![frame](./assets/"));
        assert!(translated.contains("[paper](https://example.test/paper)"));
        assert!(translated.contains("[shot]: shot.svg"));
        assert!(translated.contains("[spare]: ./shot.svg \"![example](missing.png)\""));
        assert!(translated.contains("`![literal][shot]`"));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&result.assets.values().next().unwrap().data).unwrap(), b"local-image");
        assert_eq!(std::fs::read_to_string(&chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(part.join("story.en.md")).unwrap(), english);
        std::fs::remove_file(part.join("shot.svg")).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relative_chapter_links_keep_original_paths_and_translation_bases() {
        let root = std::env::temp_dir().join(format!("markdownx-book-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("part")).unwrap();
        std::fs::create_dir_all(root.join("en")).unwrap();
        let first = root.join("part/first.md");
        let second = root.join("part/第二 章.md");
        std::fs::write(&first, "# First\n\n[Next](./第二%20章.md)\n").unwrap();
        std::fs::write(&second, "# Second\n\n[Back](first.md)\n").unwrap();
        std::fs::write(root.join("en/first.md"), "# English\n\n[Next](../part/第二%20章.md)\n").unwrap();
        std::fs::write(root.join("book.json"), serde_json::json!({"format":"markdownx.book","version":1,"chapters":[{"id":"a","file":"part/first.md","content_en_file":"en/first.md"},{"id":"b","file":"part/第二 章.md"}]}).to_string()).unwrap();
        let book = pack(&root, "Book", &[first.clone(), second.clone()]).unwrap();
        assert_eq!(book.chapters[0].source_path.as_deref(), Some("part/first.md"));
        assert_eq!(book.chapters[0].source_en_path.as_deref(), Some("en/first.md"));
        assert_eq!(book.resolve_relative_chapter("a", "./第二%20章.md", false).unwrap().id, "b");
        assert_eq!(book.resolve_relative_chapter("a", "./第二%20章.md#目标", false).unwrap().id, "b");
        assert_eq!(book.resolve_relative_chapter("a", "#目标", false).unwrap().id, "a");
        assert_eq!(book.resolve_relative_chapter("a", "../part/第二%20章.md", true).unwrap().id, "b");
        assert_eq!(book.resolve_relative_chapter("b", "first.md", false).unwrap().id, "a");
        assert!(book.resolve_relative_chapter("a", "../../outside.md", false).is_none());
        assert!(book.resolve_relative_chapter("a", "https://example.test/", false).is_none());
        let mut duplicate = book.clone();
        duplicate.chapters[1].source_path = duplicate.chapters[0].source_path.clone();
        assert!(duplicate.validate().is_err());
        let mut unsafe_path = book;
        unsafe_path.chapters[0].source_path = Some("../outside.md".into());
        assert!(unsafe_path.validate().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pdf_links_in_chapters_and_markdown_translations_are_packaged() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let zh = root.join("zh"); let en = root.join("en");
        std::fs::create_dir(&zh).unwrap(); std::fs::create_dir(&en).unwrap();
        std::fs::write(zh.join("paper one.pdf"), b"%PDF-1.4\nprimary").unwrap();
        std::fs::write(en.join("paper one.pdf"), b"%PDF-1.4\ntranslation").unwrap();
        let chapter = zh.join("story.md"); let translation = en.join("story.md");
        let source = "# Story\n\n[Paper](./paper%20one.pdf#page=2) and [Ref][paper].\n\n[paper]: paper%20one.pdf#page=3 \"reference\"\n\n`[code](missing.pdf)`\n";
        std::fs::write(&chapter, source).unwrap(); std::fs::write(&translation, source).unwrap();
        let manifest = serde_json::json!({"format":"markdownx.book", "version":1, "chapters":[{"file":"zh/story.md", "content_en_file":"en/story.md"}]});
        std::fs::write(root.join("book.json"), manifest.to_string()).unwrap();
        let book = pack(root, "Book", &[chapter.clone(), translation.clone()]).unwrap();
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.assets.len(), 2);
        assert!(book.chapters[0].content.contains("#page=2") && book.chapters[0].content.contains("#page=3"));
        let translated = book.chapters[0].content_en.as_ref().unwrap();
        assert!(translated.contains("#page=2") && translated.contains("#page=3"), "{translated}");
        assert!(translated.contains("`[code](missing.pdf)`"));
        assert_eq!(std::fs::read_to_string(chapter).unwrap(), source);
        assert_eq!(std::fs::read_to_string(translation).unwrap(), source);
    }

    #[test]
    fn manifest_source_and_translation_are_not_extra_chapters() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let primary = root.join("story.md");
        let xml = root.join("story.docxxml");
        let english = root.join("translation.md");
        std::fs::write(&primary, "# Story\n\nMarkdown\n").unwrap();
        std::fs::write(&xml, "<title>Story</title><p>XML</p>").unwrap();
        std::fs::write(&english, "# Story\n\nTranslation\n").unwrap();
        let manifest = serde_json::json!({"format":"markdownx.book", "version":1, "chapters":[{"file":"story.md", "source":"story.docxxml", "content_en_file":"translation.md"}]});
        std::fs::write(root.join("book.json"), manifest.to_string()).unwrap();
        let result = pack(root, "Book", &[xml, english, primary]).unwrap();
        assert_eq!(result.chapters.len(), 1);
        assert!(result.chapters[0].content.contains("XML"));
        assert!(result.chapters[0].content_en.as_ref().unwrap().contains("Translation"));
        assert_eq!(result.chapters[0].title.en, "Story");
    }

    #[test]
    fn xml_chapters_package_relative_title_cover_attachments_and_translation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("book");
        let primary = root.join("zh");
        let english = root.join("en");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&english).unwrap();
        std::fs::write(root.join("shot.svg"), b"wrong root image").unwrap();
        std::fs::write(root.join("book.svg"), b"book cover").unwrap();
        std::fs::write(primary.join("shot.svg"), b"chinese image").unwrap();
        std::fs::write(primary.join("cover.svg"), b"chinese cover").unwrap();
        std::fs::write(primary.join("contract.pdf"), b"chinese contract").unwrap();
        std::fs::write(english.join("shot.svg"), b"english image").unwrap();
        std::fs::write(english.join("contract.pdf"), b"english contract").unwrap();
        let chinese = "<title cover=\"./cover.svg\">Title <inline-image src=\"./shot.svg\" alt=\"shot\"/></title><figure view-type=\"Card\"><source path=\"contract.pdf\" name=\"contract\"/></figure><p>File <source path=\"./contract.pdf\" name=\"contract\" custom=\"keep\"/></p>";
        let translated = "<title>English <inline-image src=\"shot.svg\" alt=\"shot\"/></title><p>File <source path=\"./contract.pdf\" name=\"contract\" custom=\"keep\"/></p>";
        let chapter = primary.join("chapter.docxxml");
        let translation = english.join("chapter.docxxml");
        std::fs::write(&chapter, chinese).unwrap();
        std::fs::write(&translation, translated).unwrap();
        let manifest = serde_json::json!({"format":"markdownx.book", "version":1, "cover":"./book.svg#front", "chapters":[{"file":"zh/chapter.docxxml", "content_en_file":"en/chapter.docxxml"}]});
        std::fs::write(root.join("book.json"), manifest.to_string()).unwrap();
        let result = pack(&root, "Book", &[chapter.clone(), translation.clone()]).unwrap();
        assert_eq!(result.chapters.len(), 1);
        assert_eq!(result.chapters[0].title.en, "English shot");
        let data = result.assets.values().map(|asset| base64::engine::general_purpose::STANDARD.decode(&asset.data).unwrap()).collect::<Vec<_>>();
        for expected in [b"book cover".as_slice(), b"chinese image", b"chinese cover", b"chinese contract", b"english image", b"english contract"] { assert!(data.iter().any(|bytes| bytes == expected)); }
        assert!(!data.iter().any(|bytes| bytes == b"wrong root image"));
        assert!(result.cover.as_deref().unwrap().ends_with("#front"));
        assert!(result.data_uri(result.cover.as_deref().unwrap()).is_some());
        let zh = crate::docxml::parse(&result.chapters[0].content).unwrap();
        let en = crate::docxml::parse(result.chapters[0].content_en.as_deref().unwrap()).unwrap();
        assert!(zh.asset_sources().iter().all(|src| src.starts_with("assets/")));
        assert!(en.asset_sources().iter().all(|src| src.starts_with("assets/")));
        assert!(zh.cover.as_ref().unwrap().src.starts_with("assets/"));
        assert!(result.chapters[0].content.contains("custom=\"keep\"") && result.chapters[0].content_en.as_ref().unwrap().contains("custom=\"keep\""));
        assert_eq!(std::fs::read_to_string(&chapter).unwrap(), chinese);
        assert_eq!(std::fs::read_to_string(&translation).unwrap(), translated);
        std::fs::write(temp.path().join("outside.pdf"), b"outside").unwrap();
        std::fs::write(&translation, "<title>English</title><p><source path=\"../../outside.pdf\"/></p>").unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
        std::fs::write(&translation, translated).unwrap();
        std::fs::write(temp.path().join("outside.svg"), b"outside cover").unwrap();
        let mut unsafe_manifest = manifest;
        unsafe_manifest["cover"] = serde_json::json!("../outside.svg#front");
        std::fs::write(root.join("book.json"), unsafe_manifest.to_string()).unwrap();
        assert!(pack(&root, "Book", std::slice::from_ref(&chapter)).is_err());
    }

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
