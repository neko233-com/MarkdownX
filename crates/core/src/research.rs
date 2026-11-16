//! Read-only, bounded text previews for common research and document formats.
//! These do not claim to reproduce pagination, illustrations or office layout.
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::collections::{BTreeMap, BTreeSet};
use anyhow::{Context, Result, bail, ensure};
use quick_xml::{Reader, events::{BytesStart, Event}};

const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_TEXT: u64 = 16 * 1024 * 1024;
const MAX_RTF: u64 = 8 * 1024 * 1024;
const MAX_RTF_CONTROLS: usize = 250_000;

pub fn preview_markdown(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    ensure!(meta.is_file() && meta.len() <= MAX_FILE, "file is too large for preview");
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    let text = match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => archive_text(std::fs::File::open(path)?, "docx")?,
        "odt" | "ott" => archive_text(std::fs::File::open(path)?, "odt")?,
        "ofd" => ofd_text(std::fs::File::open(path)?)?,
        "rtf" => rtf_text(path)?,
        "tex" | "ltx" | "sty" | "cls" | "bst" | "bib" | "ris" | "enw" | "nbib" | "csl" | "rmd" | "qmd" | "typ" => {
            ensure!(meta.len() <= MAX_TEXT, "text is too large for preview");
            std::fs::read_to_string(path).context("source is not UTF-8")?
        }
        _ => bail!("unsupported document format"),
    };
    Ok(text_preview(path, &ext, &text))
}

/// Use the same in-memory snapshot for DOCX layout and extracted text.
pub fn preview_docx_bytes(path: &Path, bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() as u64 <= MAX_FILE, "file is too large for preview");
    let text = archive_text(std::io::Cursor::new(bytes), "docx")?;
    Ok(text_preview(path, "docx", &text))
}

/// Format already-extracted legacy Word content without interpreting markup.
pub fn preview_legacy_doc(path: &Path, text: &str) -> Result<String> {
    ensure!(text.len() as u64 <= MAX_TEXT, "document text is too large for preview");
    let mut preview = text_preview(path, "doc", text);
    preview.push_str("\n修订提示：`~~文字~~` 可能表示删除修订或原有删除线，`<ins>文字</ins>` 表示新增；请以原件核对。 / Revision markers: `~~text~~` can indicate a deletion or original strikethrough; `<ins>text</ins>` indicates an insertion. Check the original.\n");
    if text.trim().is_empty() {
        preview.push_str("\n未提取到文字；文档可能为空、仅含图像或包含不受支持的内容，请核对原件。 / No text extracted; the document may be empty, image-only or unsupported. Check the original.\n");
    }
    Ok(preview)
}

/// Format extracted ebook text independently of the original archive.
pub fn preview_epub_text(path: &Path, text: &str) -> Result<String> {
    ensure!(text.len() as u64 <= MAX_TEXT, "ebook text is too large for preview");
    Ok(text_preview(path, "epub", text))
}

/// Keep OFD layout and extracted text tied to the same file snapshot.
pub fn preview_ofd_bytes(path: &Path, bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() as u64 <= MAX_FILE, "file is too large for preview");
    let text = ofd_text(std::io::Cursor::new(bytes))?;
    Ok(text_preview(path, "ofd", &text))
}

fn text_preview(path: &Path, ext: &str, text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let title = path.file_name().unwrap_or_default().to_string_lossy().replace(['\n', '\r'], " ");
    let hidden = if ext == "rtf" { " · 含隐藏文字 / Includes hidden text" } else { "" };
    format!("# {}\n\n{}{hidden}\n\n{fence}text\n{text}\n{fence}\n", title.replace('#', "\\#"),
        "只读文本预览 / Read-only text preview · 文字可能缺失或顺序有误；原版排版、图片、批注和签章可能不显示，未验证签名 / Text may be missing or out of order; layout, images, annotations and seals may be omitted; signatures are not verified")
}

/// Parse RTF as bytes so legacy code pages, escaped characters and Unicode work.
/// Bound both input and control density before the parser builds its token list.
fn rtf_text(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.len() <= MAX_RTF, "RTF is too large for preview");
    let mut bytes = Vec::new();
    file.take(MAX_RTF + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_RTF, "RTF is too large for preview");
    ensure!(bytes.iter().filter(|&&b| matches!(b, b'\\' | b'{' | b'}')).count() <= MAX_RTF_CONTROLS, "RTF is too complex for preview");
    let doc = rtf_parser_tt::RtfDocument::from_bytes(&bytes).context("invalid RTF document")?;
    // Hidden text can matter in legal records; include it in the text preview.
    let text = doc.get_text_with(rtf_parser_tt::TextOptions::default().hidden_text(true));
    ensure!(text.len() as u64 <= MAX_TEXT, "document text is too large for preview");
    Ok(text)
}

/// Read only the declared page text. This is not a faithful rendering: OFD can
/// place text by glyph, use fonts with non-Unicode mappings, or be image-only.
fn ofd_text<R: Read + Seek>(reader: R) -> Result<String> {
    let mut archive = bounded_archive(reader, 10_000).context("invalid OFD archive")?;
    let mut xml_bytes = 0u64;
    let root = zip_xml(&mut archive, "OFD.xml", &mut xml_bytes)?;
    let documents = ofd_elements(&root, b"DocRoot")?;
    ensure!(!documents.is_empty() && documents.len() <= 128, "invalid OFD document count");
    let mut text = String::new();
    let mut pages = 0usize;
    for doc in documents {
        let doc_path = ofd_path("", &doc)?;
        let xml = zip_xml(&mut archive, &doc_path, &mut xml_bytes)?;
        for page in ofd_pages(&xml)? {
            pages += 1;
            ensure!(pages <= 10000, "too many OFD pages");
            let page_path = ofd_path(doc_path.rsplit_once('/').map_or("", |(dir, _)| dir), &page)?;
            let page_xml = zip_xml(&mut archive, &page_path, &mut xml_bytes)?;
            let fragments = ofd_elements(&page_xml, b"TextCode")?;
            if !text.is_empty() { text.push('\n'); }
            text.push_str(&format!("[Page {pages}]\n"));
            for fragment in fragments {
                ensure!(text.len().saturating_add(fragment.len()).saturating_add(1) as u64 <= MAX_TEXT, "document text is too large for preview");
                text.push_str(&fragment);
                text.push('\n');
            }
        }
    }
    ensure!(pages > 0, "OFD has no pages");
    Ok(text)
}

fn zip_xml<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str, xml_bytes: &mut u64) -> Result<String> {
    let mut file = archive.by_name(name).with_context(|| format!("OFD entry is missing: {name}"))?;
    ensure!(file.size() <= MAX_TEXT, "OFD XML is too large");
    *xml_bytes = xml_bytes.checked_add(file.size()).context("OFD XML is too large")?;
    ensure!(*xml_bytes <= MAX_FILE, "OFD XML is too large");
    let mut xml = String::new();
    file.by_ref().take(MAX_TEXT + 1).read_to_string(&mut xml)?;
    ensure!(xml.len() as u64 <= MAX_TEXT, "OFD XML is too large");
    Ok(xml)
}

fn ofd_path(base: &str, loc: &str) -> Result<String> {
    let loc = loc.trim();
    ensure!(!loc.is_empty() && !loc.contains(['\\', '\0', ':']), "invalid OFD path");
    let mut parts: Vec<&str> = if loc.starts_with('/') || base.is_empty() { vec![] } else { base.split('/').collect() };
    for part in loc.split('/') {
        match part {
            "" | "." => {},
            ".." => { ensure!(parts.pop().is_some(), "OFD path escapes its archive"); },
            _ => parts.push(part),
        }
    }
    ensure!(!parts.is_empty(), "invalid OFD path");
    Ok(parts.join("/"))
}

fn ofd_elements(xml: &str, name: &[u8]) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut values = vec![];
    let mut value: Option<String> = None;
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.local_name().as_ref() == name => { ensure!(value.is_none(), "nested OFD text element"); value = Some(String::new()); }
            Event::End(e) if e.local_name().as_ref() == name => { if let Some(s) = value.take() { values.push(s); ensure!(values.len() <= 200_000, "too many OFD text fragments"); } }
            Event::Text(t) if value.is_some() => value.as_mut().unwrap().push_str(&t.unescape()?),
            Event::CData(t) if value.is_some() => value.as_mut().unwrap().push_str(&String::from_utf8_lossy(&t)),
            Event::Eof => break,
            _ => {},
        }
    }
    Ok(values)
}

fn ofd_pages(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut in_pages = false;
    let mut pages = vec![];
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.local_name().as_ref() == b"Pages" => in_pages = true,
            Event::End(e) if e.local_name().as_ref() == b"Pages" => in_pages = false,
            Event::Start(e) | Event::Empty(e) if in_pages && e.local_name().as_ref() == b"Page" => {
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.local_name().as_ref() == b"BaseLoc" { pages.push(attr.unescape_value()?.into_owned()); break; }
                }
                ensure!(pages.len() <= 10000, "too many OFD pages");
            }
            Event::Eof => break,
            _ => {},
        }
    }
    Ok(pages)
}

fn archive_text<R: Read + Seek>(reader: R, ext: &str) -> Result<String> {
    let mut archive = bounded_archive(reader, 10_000).context("invalid document archive")?;
    let member = if ext == "docx" { "word/document.xml" } else { "content.xml" };
    let mut total = 0u64;
    let xml = document_xml(&mut archive, member, &mut total)?.context("document content is missing")?;
    let mut text = extract_text(&xml, ext)?;
    if ext == "docx" { append_word_related(&mut archive, &xml, &mut text, &mut total)?; }
    Ok(text)
}

/// ZIP readers can collapse duplicate names, making two parsers select different
/// document parts. Check the declared count before allocation, then compare it
/// to both the central directory and the reader's visible entries.
pub fn bounded_archive<R: Read + Seek>(mut reader: R, limit: usize) -> Result<zip::ZipArchive<R>> {
    let end = reader.seek(SeekFrom::End(0))?;
    ensure!(end >= 22, "ZIP directory is missing");
    let tail_len = end.min(65_557) as usize;
    reader.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = vec![0; tail_len];
    reader.read_exact(&mut tail)?;
    let eocd = (0..=tail.len() - 22).rev().find(|&at| {
        tail[at..at + 4] == [0x50, 0x4b, 0x05, 0x06]
            && at + 22 + u16::from_le_bytes([tail[at + 20], tail[at + 21]]) as usize == tail.len()
    }).context("ZIP end record is missing")?;
    let value = |offset: usize| u16::from_le_bytes([tail[eocd + offset], tail[eocd + offset + 1]]);
    ensure!(value(4) == 0 && value(6) == 0 && value(8) == value(10), "multi-disk ZIP is unsupported");
    let declared = if value(10) == u16::MAX {
        let eocd_position = end - tail_len as u64 + eocd as u64;
        reader.seek(SeekFrom::Start(eocd_position.checked_sub(20).context("ZIP64 locator is missing")?))?;
        let mut locator = [0; 20]; reader.read_exact(&mut locator)?;
        ensure!(locator[..4] == [0x50, 0x4b, 0x06, 0x07], "ZIP64 locator is missing");
        ensure!(u32::from_le_bytes(locator[4..8].try_into()?) == 0 && u32::from_le_bytes(locator[16..20].try_into()?) == 1, "multi-disk ZIP64 is unsupported");
        let position = u64::from_le_bytes(locator[8..16].try_into()?);
        reader.seek(SeekFrom::Start(position))?;
        let mut zip64 = [0; 56]; reader.read_exact(&mut zip64)?;
        ensure!(zip64[..4] == [0x50, 0x4b, 0x06, 0x06], "ZIP64 directory is missing");
        let size = u64::from_le_bytes(zip64[4..12].try_into()?);
        ensure!(size >= 44 && position.checked_add(12).and_then(|p| p.checked_add(size)) == eocd_position.checked_sub(20), "invalid ZIP64 directory");
        ensure!(u32::from_le_bytes(zip64[16..20].try_into()?) == 0 && u32::from_le_bytes(zip64[20..24].try_into()?) == 0, "multi-disk ZIP64 is unsupported");
        let disk = u64::from_le_bytes(zip64[24..32].try_into()?);
        let total = u64::from_le_bytes(zip64[32..40].try_into()?);
        ensure!(disk == total, "multi-disk ZIP64 is unsupported");
        usize::try_from(total).context("ZIP64 entry count is too large")?
    } else { value(10) as usize };
    ensure!(declared <= limit, "ZIP has too many entries");
    let archive = zip::ZipArchive::new(reader)?;
    let count = archive.len();
    let start = archive.central_directory_start();
    ensure!(count <= limit && count == declared, "ZIP contains duplicate or inconsistent entries");
    let mut reader = archive.into_inner();
    reader.seek(SeekFrom::Start(start))?;
    let mut scanned = 0usize;
    loop {
        let mut signature = [0; 4];
        reader.read_exact(&mut signature)?;
        if signature != [0x50, 0x4b, 0x01, 0x02] { break; }
        let mut header = [0; 42];
        reader.read_exact(&mut header)?;
        let length = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]) as i64;
        reader.seek(SeekFrom::Current(length(24) + length(26) + length(28)))?;
        scanned += 1;
        ensure!(scanned <= limit, "ZIP has too many entries");
    }
    ensure!(scanned == declared, "ZIP central directory is inconsistent");
    let archive = zip::ZipArchive::new(reader)?;
    let mut names = BTreeSet::new();
    for name in archive.file_names() { ensure!(names.insert(name.to_string()), "ZIP has duplicate decoded names"); }
    Ok(archive)
}

fn document_xml<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str, total: &mut u64) -> Result<Option<String>> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(file.size() <= MAX_TEXT, "document part is too large for preview");
    *total = total.checked_add(file.size()).context("document parts are too large")?;
    ensure!(*total <= MAX_FILE, "document parts are too large");
    let mut xml = String::new();
    file.by_ref().take(MAX_TEXT + 1).read_to_string(&mut xml)?;
    ensure!(xml.len() as u64 <= MAX_TEXT, "document part is too large for preview");
    Ok(Some(xml))
}

fn word_attribute(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in element.attributes() {
        let attribute = attribute?;
        if attribute.key.local_name().as_ref() == name {
            let value = attribute.unescape_value()?.into_owned();
            ensure!(value.len() <= 512 && !value.chars().any(char::is_control), "invalid Word reference");
            return Ok(Some(value));
        }
    }
    Ok(None)
}

#[derive(Default)]
struct WordReferences { footnotes: BTreeSet<String>, endnotes: BTreeSet<String>, comments: BTreeSet<String>, headers: BTreeSet<String> }

fn word_references(xml: &str) -> Result<WordReferences> {
    let mut refs = WordReferences::default();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element) => {
                let target = match element.name().as_ref() {
                    b"w:footnoteReference" => Some(&mut refs.footnotes),
                    b"w:endnoteReference" => Some(&mut refs.endnotes),
                    b"w:commentReference" | b"w:commentRangeStart" => Some(&mut refs.comments),
                    b"w:headerReference" | b"w:footerReference" => Some(&mut refs.headers),
                    _ => None,
                };
                if let Some(target) = target && let Some(id) = word_attribute(&element, b"id")? {
                    target.insert(id);
                    ensure!(target.len() <= 10_000, "too many Word references");
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(refs)
}

fn word_relationships(xml: &str) -> Result<BTreeMap<String, (String, String)>> {
    let mut reader = Reader::from_str(xml);
    let mut relationships = BTreeMap::new();
    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element) if element.local_name().as_ref() == b"Relationship" => {
                let Some(id) = word_attribute(&element, b"Id")? else { continue };
                let Some(kind) = word_attribute(&element, b"Type")? else { continue };
                let Some(target) = word_attribute(&element, b"Target")? else { continue };
                let mode = word_attribute(&element, b"TargetMode")?.unwrap_or_default();
                if !mode.is_empty() && !mode.eq_ignore_ascii_case("internal") { continue; }
                if !(kind.starts_with("http://schemas.openxmlformats.org/officeDocument/2006/relationships/") || kind.starts_with("http://purl.oclc.org/ooxml/officeDocument/relationships/")) { continue; }
                let kind = kind.rsplit('/').next().unwrap_or("");
                if !matches!(kind, "footnotes" | "endnotes" | "comments" | "header" | "footer") { continue; }
                let Ok(path) = word_part_path(&target) else { continue };
                ensure!(relationships.insert(id, (kind.to_string(), path)).is_none(), "duplicate Word relationship");
                ensure!(relationships.len() <= 4096, "too many Word relationships");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

fn word_part_path(target: &str) -> Result<String> {
    ensure!(!target.contains(['?', '#', '\\', ':']) && !target.chars().any(char::is_control), "invalid Word part path");
    let mut bytes = Vec::with_capacity(target.len());
    let raw = target.as_bytes(); let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%' {
            ensure!(at + 2 < raw.len(), "invalid Word part escape");
            let hex = std::str::from_utf8(&raw[at + 1..at + 3])?;
            bytes.push(u8::from_str_radix(hex, 16).context("invalid Word part escape")?);
            at += 3;
        } else { bytes.push(raw[at]); at += 1; }
    }
    let decoded = String::from_utf8(bytes)?;
    ensure!(!decoded.contains(['?', '#', '\\', ':']) && !decoded.chars().any(char::is_control), "invalid Word part path");
    ofd_path("word", &decoded)
}

fn word_notes(xml: &str, kind: &str, wanted: &BTreeSet<String>) -> Result<String> {
    let tag = match kind { "footnotes" => b"w:footnote".as_slice(), "endnotes" => b"w:endnote".as_slice(), _ => b"w:comment".as_slice() };
    let label = match kind { "footnotes" => "Footnote", "endnotes" => "Endnote", _ => "Comment" };
    let mut reader = Reader::from_str(xml);
    let mut current: Option<(String, usize, Option<String>)> = None;
    let mut output = String::new();
    let mut remaining = wanted.clone();
    loop {
        let start = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Start(element) if element.name().as_ref() == tag => {
                ensure!(current.is_none(), "nested Word note");
                if let Some(id) = word_attribute(&element, b"id")? && wanted.contains(&id) {
                    current = Some((id, start, word_attribute(&element, b"author")?));
                }
            }
            Event::End(element) if element.name().as_ref() == tag => {
                if let Some((id, start, author)) = current.take() {
                    remaining.remove(&id);
                    let end = reader.buffer_position() as usize;
                    let fragment = xml.get(start..end).context("invalid Word note position")?;
                    let body = extract_text(fragment, "docx")?;
                    output.push_str(&format!("\n[{label} {id}{}]\n{body}", author.map(|a| format!(" · {a}")).unwrap_or_default()));
                    ensure!(output.len() as u64 <= MAX_TEXT, "Word notes are too large");
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(current.is_none(), "unterminated Word note");
    if !remaining.is_empty() { output.push_str(&format!("\n[Some {label} references could not be read.]\n")); }
    Ok(output)
}

fn append_word_related<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, xml: &str, text: &mut String, total: &mut u64) -> Result<()> {
    let refs = word_references(xml)?;
    if refs.footnotes.is_empty() && refs.endnotes.is_empty() && refs.comments.is_empty() && refs.headers.is_empty() { return Ok(()); }
    let relations = match document_xml(archive, "word/_rels/document.xml.rels", total).and_then(|xml| xml.context("Word relationships are missing")).and_then(|xml| word_relationships(&xml)) {
        Ok(relations) => relations,
        Err(_) => { text.push_str("\n[Referenced notes, comments or headers may be unavailable.]\n"); ensure!(text.len() as u64 <= MAX_TEXT, "document text is too large for preview"); return Ok(()); }
    };
    let mut seen = BTreeSet::new();
    let mut found = BTreeSet::new();
    for (id, (kind, path)) in relations {
        let wanted = match kind.as_str() {
            "footnotes" if !refs.footnotes.is_empty() => Some(&refs.footnotes),
            "endnotes" if !refs.endnotes.is_empty() => Some(&refs.endnotes),
            "comments" if !refs.comments.is_empty() => Some(&refs.comments),
            "header" | "footer" if refs.headers.contains(&id) => Some(&refs.headers),
            _ => None,
        };
        let Some(wanted) = wanted else { continue };
        if !seen.insert(path.clone()) { continue; }
        let part = document_xml(archive, &path, total).and_then(|part| part.context("Word part is missing"));
        let section = part.and_then(|part| if matches!(kind.as_str(), "header" | "footer") {
            Ok(format!("\n[{}]\n{}", if kind == "header" { "Header" } else { "Footer" }, extract_text(&part, "docx")?))
        } else { word_notes(&part, &kind, wanted) });
        match section {
            Ok(section) => { ensure!(text.len().saturating_add(section.len()) as u64 <= MAX_TEXT, "document text is too large for preview"); text.push_str(&section); found.insert(if matches!(kind.as_str(), "header" | "footer") { id } else { kind }); }
            Err(_) => text.push_str("\n[Referenced Word content could not be read.]\n"),
        }
    }
    if (!refs.footnotes.is_empty() && !found.contains("footnotes")) || (!refs.endnotes.is_empty() && !found.contains("endnotes"))
        || (!refs.comments.is_empty() && !found.contains("comments")) || refs.headers.iter().any(|id| !found.contains(id)) {
        text.push_str("\n[Some referenced notes, comments or headers may be unavailable.]\n");
    }
    ensure!(text.len() as u64 <= MAX_TEXT, "document text is too large for preview");
    Ok(())
}

fn extract_text(xml: &str, ext: &str) -> Result<String> {
    fn mark_revisions(text: &mut String, revisions: &mut [(&'static str, bool)]) {
        for (label, started) in revisions {
            if !*started { text.push_str(label); *started = true; }
        }
    }
    fn mark_reference(element: &BytesStart<'_>, text: &mut String, revisions: &mut [(&'static str, bool)]) -> Result<()> {
        let label = match element.name().as_ref() {
            b"w:footnoteReference" => "Footnote", b"w:endnoteReference" => "Endnote", _ => "Comment",
        };
        if let Some(id) = word_attribute(element, b"id")? {
            mark_revisions(text, revisions);
            text.push_str(&format!("[{label} {id}]"));
        }
        Ok(())
    }
    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    let mut in_word_text = false;
    let mut standalone_deleted = false;
    let mut revisions: Vec<(&'static str, bool)> = vec![];
    let mut odt_paragraph = 0usize;
    loop {
        match reader.read_event()? {
            Event::Start(e) => match e.name().as_ref() {
                b"w:t" if ext == "docx" => in_word_text = true,
                b"w:delText" if ext == "docx" => {
                    in_word_text = true;
                    if revisions.is_empty() { text.push_str("[Deleted: "); standalone_deleted = true; }
                }
                b"w:del" | b"w:moveFrom" if ext == "docx" => { ensure!(revisions.len() < 64, "Word revisions are too deeply nested"); revisions.push(("[Deleted: ", false)); }
                b"w:ins" | b"w:moveTo" if ext == "docx" => { ensure!(revisions.len() < 64, "Word revisions are too deeply nested"); revisions.push(("[Inserted: ", false)); }
                b"w:footnoteReference" | b"w:endnoteReference" | b"w:commentReference" if ext == "docx" => mark_reference(&e, &mut text, &mut revisions)?,
                b"text:p" | b"text:h" if ext == "odt" => odt_paragraph += 1,
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"w:t" if ext == "docx" => in_word_text = false,
                b"w:delText" if ext == "docx" => {
                    in_word_text = false;
                    if standalone_deleted { text.push(']'); standalone_deleted = false; }
                }
                b"w:del" | b"w:moveFrom" | b"w:ins" | b"w:moveTo" if ext == "docx" => {
                    if revisions.pop().is_some_and(|(_, started)| started) { text.push(']'); }
                }
                b"w:p" if ext == "docx" => text.push('\n'),
                b"text:p" | b"text:h" if ext == "odt" => { odt_paragraph = odt_paragraph.saturating_sub(1); text.push('\n'); }
                _ => {}
            },
            Event::Empty(e) => match e.name().as_ref() {
                b"w:tab" | b"text:tab" => { mark_revisions(&mut text, &mut revisions); text.push('\t'); }
                b"w:br" | b"w:cr" | b"text:line-break" => { mark_revisions(&mut text, &mut revisions); text.push('\n'); }
                b"w:footnoteReference" | b"w:endnoteReference" | b"w:commentReference" if ext == "docx" => mark_reference(&e, &mut text, &mut revisions)?,
                b"text:s" if odt_paragraph > 0 => text.push(' '),
                _ => {}
            },
            Event::Text(t) if in_word_text || odt_paragraph > 0 => { mark_revisions(&mut text, &mut revisions); text.push_str(&t.unescape()?); }
            Event::CData(t) if in_word_text || odt_paragraph > 0 => { mark_revisions(&mut text, &mut revisions); text.push_str(&String::from_utf8_lossy(&t)); }
            Event::Eof => break,
            _ => {}
        }
        ensure!(text.len() as u64 <= MAX_TEXT, "document text is too large for preview");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_preview_reads_chinese_unicode_and_hidden_text_without_modifying_original() {
        let path = std::env::temp_dir().join(format!("mdx-legal-{}.rtf", uuid::Uuid::new_v4()));
        let rtf = br"{\rtf1\ansi\ansicpg936 \'d6\'d0\'ce\'c4 \u-10179?\u-8704?{\v  hidden}\par Second line}";
        std::fs::write(&path, rtf).unwrap();
        let text = preview_markdown(&path).unwrap();
        assert!(text.contains("中文"), "{text}");
        assert!(text.contains('😀'), "{text}");
        assert!(text.contains("hidden") && text.contains("Second line"), "{text}");
        assert!(text.contains("含隐藏文字"));
        assert_eq!(std::fs::read(&path).unwrap(), rtf);
        let legacy = b"{\\rtf1\\ansi\\ansicpg936 \xd6\xd0\xce\xc4}";
        std::fs::write(&path, legacy).unwrap();
        assert!(preview_markdown(&path).unwrap().contains("中文"));
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        std::fs::write(&path, b"{\\rtf1 bad \\'zz}").unwrap();
        assert!(preview_markdown(&path).is_err());
        std::fs::write(&path, vec![b'\\'; MAX_RTF_CONTROLS + 1]).unwrap();
        assert!(preview_markdown(&path).is_err());
        std::fs::write(&path, include_bytes!("../../../MarkdownX-Demo/示例合同.rtf")).unwrap();
        assert!(preview_markdown(&path).unwrap().contains("合同示例"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn research_source_extensions_open_as_read_only_text() {
        for ext in ["ltx", "sty", "cls", "bst", "nbib", "csl", "rmd", "qmd", "typ"] {
            let path = std::env::temp_dir().join(format!("mdx-paper-{}.{ext}", uuid::Uuid::new_v4()));
            std::fs::write(&path, "paper `source`\n").unwrap();
            assert!(preview_markdown(&path).unwrap().contains("paper `source`"), "{ext}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "paper `source`\n");
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn extracts_word_and_open_document_text_without_markup() {
        assert_eq!(extract_text("<w:document><w:p><w:r><w:t>A &amp; B</w:t><w:tab/><w:t>C</w:t></w:r></w:p></w:document>", "docx").unwrap(), "A & B\tC\n");
        assert_eq!(extract_text("<office:document><text:p>A<text:s/>B<text:line-break/>C</text:p></office:document>", "odt").unwrap(), "A B\nC\n");
    }

    #[test]
    fn word_text_preview_retains_and_labels_tracked_changes() {
        let xml = "<w:document><w:p><w:r><w:t>Before </w:t></w:r><w:del><w:r><w:delText>old clause</w:delText></w:r></w:del><w:ins><w:r><w:t>new clause</w:t></w:r></w:ins><w:moveFrom><w:r><w:delText>previous</w:delText></w:r></w:moveFrom><w:moveTo><w:r><w:t>moved</w:t></w:r></w:moveTo></w:p></w:document>";
        let result = extract_text(xml, "docx").unwrap();
        assert_eq!(result, "Before [Deleted: old clause][Inserted: new clause][Deleted: previous][Inserted: moved]\n");
        assert_eq!(extract_text("<w:p><w:delText>orphan</w:delText></w:p>", "docx").unwrap(), "[Deleted: orphan]\n");
        let nested = format!("{}x{}", "<w:ins>".repeat(65), "</w:ins>".repeat(65));
        assert!(extract_text(&nested, "docx").is_err());
    }

    #[test]
    fn word_preview_includes_referenced_notes_comments_and_headers_only() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("mdx-word-notes-{}.docx", uuid::Uuid::new_v4()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let doc = r#"<w:document><w:body><w:p><w:r><w:t>Contract</w:t><w:footnoteReference w:id="2"/><w:endnoteReference w:id="4"/><w:commentReference w:id="9"/></w:r></w:p><w:sectPr><w:headerReference r:id="h1"/><w:footerReference r:id="f1"/></w:sectPr></w:body></w:document>"#;
        let rels = r#"<Relationships><Relationship Id="n1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="n2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="n3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="h1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header%201.xml"/><Relationship Id="f1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer.xml"/></Relationships>"#;
        for (name, content) in [
            ("word/document.xml", doc), ("word/_rels/document.xml.rels", rels),
            ("word/footnotes.xml", r#"<w:footnotes><w:footnote w:id="2"><w:p><w:r><w:t>Referenced footnote</w:t></w:r></w:p></w:footnote><w:footnote w:id="5"><w:p><w:r><w:t>UNREFERENCED</w:t></w:r></w:p></w:footnote></w:footnotes>"#),
            ("word/endnotes.xml", r#"<w:endnotes><w:endnote w:id="4"><w:p><w:r><w:t>Referenced endnote</w:t></w:r></w:p></w:endnote></w:endnotes>"#),
            ("word/comments.xml", r#"<w:comments><w:comment w:id="9" w:author="Reviewer"><w:p><w:r><w:t>Review comment</w:t></w:r></w:p></w:comment></w:comments>"#),
            ("word/header 1.xml", "<w:hdr><w:p><w:r><w:t>Page header</w:t></w:r></w:p></w:hdr>"),
            ("word/footer.xml", "<w:ftr><w:p><w:r><w:t>Page footer</w:t></w:r></w:p></w:ftr>"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let original = std::fs::read(&path).unwrap();
        let preview = preview_markdown(&path).unwrap();
        for needle in ["Contract[Footnote 2][Endnote 4][Comment 9]", "Referenced footnote", "Referenced endnote", "[Comment 9 · Reviewer]", "Review comment", "Page header", "Page footer"] {
            assert!(preview.contains(needle), "missing {needle}: {preview}");
        }
        assert!(!preview.contains("UNREFERENCED"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        assert!(word_part_path("%2e%2e/%2e%2e/escape.xml").is_err());
        assert!(word_part_path("file:%2f%2fexample").is_err());
    }

    #[test]
    fn word_preview_reports_unavailable_related_content() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("mdx-word-missing-notes-{}.docx", uuid::Uuid::new_v4()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"<w:document><w:p><w:r><w:t>Body</w:t><w:footnoteReference w:id="3"/></w:r></w:p></w:document>"#).unwrap();
        zip.finish().unwrap();
        let preview = preview_markdown(&path).unwrap();
        assert!(preview.contains("Body[Footnote 3]") && preview.contains("may be unavailable"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn zip_preview_rejects_duplicate_names_and_entry_overflow() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["word/document.xml", "word/document.xmL"] {
            writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"content").unwrap();
        }
        let original = writer.finish().unwrap().into_inner();
        assert!(bounded_archive(std::io::Cursor::new(original.clone()), 1).is_err());
        assert!(bounded_archive(std::io::Cursor::new(original.clone()), 2).is_ok());
        let mut duplicate = original;
        let name = b"word/document.xmL";
        let positions: Vec<_> = duplicate.windows(name.len()).enumerate().filter_map(|(i, value)| (value == name).then_some(i)).collect();
        assert_eq!(positions.len(), 2);
        for position in positions { duplicate[position + name.len() - 1] = b'l'; }
        assert!(bounded_archive(std::io::Cursor::new(duplicate), 2).is_err());
    }

    #[test]
    fn zip64_directory_with_a_small_entry_count_is_supported() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"sample").unwrap();
        let original = writer.finish().unwrap().into_inner();
        let end = original.len() - 22;
        assert_eq!(&original[end..end + 4], b"PK\x05\x06");
        let central_size = u32::from_le_bytes(original[end + 12..end + 16].try_into().unwrap()) as u64;
        let central_offset = u32::from_le_bytes(original[end + 16..end + 20].try_into().unwrap()) as u64;
        let mut encoded = original[..end].to_vec();
        encoded.extend_from_slice(b"PK\x06\x06");
        encoded.extend_from_slice(&44u64.to_le_bytes());
        encoded.extend_from_slice(&45u16.to_le_bytes()); encoded.extend_from_slice(&45u16.to_le_bytes());
        encoded.extend_from_slice(&0u32.to_le_bytes()); encoded.extend_from_slice(&0u32.to_le_bytes());
        encoded.extend_from_slice(&1u64.to_le_bytes()); encoded.extend_from_slice(&1u64.to_le_bytes());
        encoded.extend_from_slice(&central_size.to_le_bytes()); encoded.extend_from_slice(&central_offset.to_le_bytes());
        encoded.extend_from_slice(b"PK\x06\x07"); encoded.extend_from_slice(&0u32.to_le_bytes());
        encoded.extend_from_slice(&(end as u64).to_le_bytes()); encoded.extend_from_slice(&1u32.to_le_bytes());
        let mut footer = original[end..].to_vec();
        footer[8..12].copy_from_slice(&[0xff; 4]); footer[12..20].copy_from_slice(&[0xff; 8]);
        encoded.extend_from_slice(&footer);
        assert!(bounded_archive(std::io::Cursor::new(encoded), 10).is_ok());
    }

    #[test]
    #[ignore = "set MARKDOWNX_DOCX_SAMPLES to a directory of Apache POI test fixtures"]
    fn real_docx_references_open_without_modification() {
        let root = std::path::PathBuf::from(std::env::var_os("MARKDOWNX_DOCX_SAMPLES").expect("missing sample directory"));
        for (name, expected) in [
            ("footnotes.docx", "snoska"), ("endnotes.docx", "XXX"), ("comment.docx", "This is the first line"),
            ("HeaderFooterUnicode.docx", "This is a simple header"), ("HeaderFooterUnicode.docx", "The footer, with Molière"),
        ] {
            let path = root.join(name);
            let original = std::fs::read(&path).unwrap();
            let preview = preview_markdown(&path).unwrap();
            assert!(preview.contains(expected), "{name} is missing {expected}: {preview}");
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn source_preview_keeps_backticks_inside_the_fence() {
        let path = std::env::temp_dir().join(format!("mdx-research-{}.bib", uuid::Uuid::new_v4()));
        std::fs::write(&path, "@article{a, title={```}}\n").unwrap();
        let preview = preview_markdown(&path).unwrap();
        assert!(preview.contains("````text\n@article{a, title={```}}"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn docx_preview_reads_archive_without_modifying_original() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("mdx-doc-{}.docx", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"<w:document><w:p><w:r><w:t>Legal &amp; research</w:t></w:r></w:p></w:document>").unwrap();
        zip.finish().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(preview_markdown(&path).unwrap().contains("Legal & research"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn office_templates_and_macro_documents_are_read_only() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("mdx-office-formats-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let docx = include_bytes!("../../../MarkdownX-Demo/示例合同.docx");
        for ext in ["DOCM", "dotx", "dotm"] {
            let path = root.join(format!("sample.{ext}"));
            std::fs::write(&path, docx).unwrap();
            assert!(preview_markdown(&path).unwrap().contains("分镜制作服务合同示例"));
            assert_eq!(std::fs::read(&path).unwrap(), docx);
        }
        let path = root.join("sample.ott");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"<office:document><text:p>Template &amp; contract</text:p></office:document>").unwrap();
        zip.finish().unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(preview_markdown(&path).unwrap().contains("Template & contract"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ofd_preview_follows_page_order_and_preserves_original() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("mdx-legal-{}.ofd", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, contents) in [
            ("OFD.xml", "<ofd:OFD><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"),
            ("Doc_0/Document.xml", "<o:Document><o:Pages><o:Page BaseLoc='Pages/second.xml'/><o:Page BaseLoc='./Pages/first.xml'/></o:Pages></o:Document>"),
            ("Doc_0/Pages/second.xml", "<o:Page><o:TextObject><o:TextCode>合同 &amp; 论文</o:TextCode></o:TextObject></o:Page>"),
            ("Doc_0/Pages/first.xml", "<o:Page><o:TextObject><o:TextCode><![CDATA[第二页]]></o:TextCode></o:TextObject></o:Page>"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let before = std::fs::read(&path).unwrap();
        let preview = preview_markdown(&path).unwrap();
        assert!(preview.contains("[Page 1]\n合同 & 论文\n\n[Page 2]\n第二页"));
        assert!(preview.contains("未验证签名"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ofd_paths_cannot_escape_the_archive() {
        assert_eq!(ofd_path("Doc_0", "Pages/../Page.xml").unwrap(), "Doc_0/Page.xml");
        assert_eq!(ofd_path("Doc_0", "/Other/Page.xml").unwrap(), "Other/Page.xml");
        assert!(ofd_path("Doc_0", "../../secret").is_err());
        assert!(ofd_path("Doc_0", "C:/secret").is_err());
        assert!(ofd_path("Doc_0", "..\\secret").is_err());
    }

    #[test]
    #[ignore = "set MARKDOWNX_OFD_SAMPLES to a directory of public OFD fixtures"]
    fn real_ofd_samples_open_without_modification() {
        let dir = std::env::var_os("MARKDOWNX_OFD_SAMPLES").expect("missing sample directory");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("ofd")) { continue }
            let original = std::fs::read(&path).unwrap();
            let text = preview_markdown(&path).unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
            assert!(text.contains("[Page 1]"), "{}", path.display());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            count += 1;
        }
        assert!(count > 0, "no OFD samples found");
    }
}
