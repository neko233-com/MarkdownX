//! Read-only DOCX layout preview in the existing isolated HTML widget.
//! The original file is never modified; the text preview remains available.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::OnceLock;

const MAX_ARCHIVE: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const MAX_ENTRY: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

fn script_urls() -> &'static (String, String) {
    static SCRIPTS: OnceLock<(String, String)> = OnceLock::new();
    SCRIPTS.get_or_init(|| (
        format!("data:text/javascript;base64,{}", STANDARD.encode(include_bytes!("../assets/docx-viewer/jszip.min.js"))),
        format!("data:text/javascript;base64,{}", STANDARD.encode(include_bytes!("../assets/docx-viewer/docx-preview.min.js"))),
    ))
}

/// Validate and actually decompress the bounded archive before passing bytes to
/// the renderer. Never extract entries to disk or follow relationships outside.
pub fn prepare(path: &Path) -> Result<(String, String)> {
    let bytes = crate::store::read_bounded(path, MAX_ARCHIVE).context("DOCX is too large for the layout preview")?;
    let layout = html(&bytes)?;
    let text = fdl_core::research::preview_docx_bytes(path, &bytes)?;
    Ok((text, layout))
}

fn html(bytes: &[u8]) -> Result<String> {
    let (mut archive, names) = validated_archive(bytes)?;
    ensure!(names.contains("[Content_Types].xml") && names.contains("word/document.xml") && names.contains("_rels/.rels"), "DOCX structure is incomplete");
    validate_main_relationship(&mut archive)?;
    let scripts = script_urls();
    let data = STANDARD.encode(bytes);
    Ok(format!(r#"<style>html,body{{display:block!important;overflow:auto!important;height:auto!important;margin:0!important;padding:0!important;background:#ececec!important}}#docx-preview{{min-width:fit-content}}.docx-wrapper{{padding:12px!important}}</style>
<div id="docx-preview" aria-label="DOCX preview"></div><div id="docx-error" role="status"></div>
<script src="{}"></script><script src="{}"></script>
<script>(()=>{{const out=document.getElementById('docx-preview'), error=document.getElementById('docx-error');try{{const raw=atob('{data}'),bytes=Uint8Array.from(raw,c=>c.charCodeAt(0));docx.renderAsync(bytes,out,null,{{useBase64URL:true,renderAltChunks:false,renderChanges:true,renderComments:true,ignoreLastRenderedPageBreak:false}}).then(()=>{{out.dataset.ready='true';}}).catch(()=>{{error.textContent='无法显示版式；请查看下方提取的文字 / Layout preview failed; see extracted text below.';}});}}catch(_){{error.textContent='无法显示版式；请查看下方提取的文字 / Layout preview failed; see extracted text below.';}}}})();</script>"#, scripts.0, scripts.1))
}

/// Bound and decompress entries before handing a document to a web renderer.
pub(crate) fn validated_archive(bytes: &[u8]) -> Result<(zip::ZipArchive<Cursor<&[u8]>>, HashSet<String>)> {
    let mut archive = fdl_core::research::bounded_archive(Cursor::new(bytes), MAX_ENTRIES).context("invalid document archive")?;
    let mut expanded = 0u64;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).context("unsupported DOCX entry")?;
        ensure!(!entry.encrypted() && entry.enclosed_name().is_some(), "unsafe DOCX entry");
        let name = entry.name();
        ensure!(!name.contains(['\\', ':']) && !name.chars().any(char::is_control)
            && name.trim_end_matches('/').split('/').all(|part| !part.is_empty() && part != "." && part != ".."), "unsafe DOCX entry name");
        ensure!(names.insert(name.to_string()), "duplicate DOCX entry");
        ensure!(entry.size() <= MAX_ENTRY, "DOCX entry is too large");
        expanded = expanded.checked_add(entry.size()).context("DOCX is too large")?;
        ensure!(expanded <= MAX_EXPANDED, "expanded DOCX is too large");
        let expected = entry.size();
        let read = std::io::copy(&mut entry.by_ref().take(expected.saturating_add(1)), &mut std::io::sink())?;
        ensure!(read == expected, "invalid DOCX entry size");
    }
    Ok((archive, names))
}

/// The renderer and the text extractor must refer to the same document part.
fn validate_main_relationship(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<()> {
    use quick_xml::{Reader, events::Event};
    let file = archive.by_name("_rels/.rels")?;
    ensure!(file.size() <= 1024 * 1024, "DOCX relationships are too large");
    let mut xml = String::new();
    file.take(1024 * 1024 + 1).read_to_string(&mut xml)?;
    let mut reader = Reader::from_str(&xml);
    let mut count = 0;
    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element) if element.local_name().as_ref() == b"Relationship" => {
                let (mut kind, mut target, mut mode) = (String::new(), String::new(), String::new());
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    match attribute.key.local_name().as_ref() {
                        b"Type" => kind = attribute.unescape_value()?.into_owned(),
                        b"Target" => target = attribute.unescape_value()?.into_owned(),
                        b"TargetMode" => mode = attribute.unescape_value()?.into_owned(),
                        _ => {}
                    }
                }
                if kind.ends_with("/officeDocument") {
                    count += 1;
                    ensure!((mode.is_empty() || mode.eq_ignore_ascii_case("internal")) && matches!(target.as_str(), "word/document.xml" | "/word/document.xml"), "unsupported DOCX main document relationship");
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(count == 1, "DOCX must contain one main document relationship");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_invalid_and_unsafe_archives_without_touching_them() {
        let root = std::env::temp_dir().join(format!("markdownx-docx-preview-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("paper.docx");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "../escape"] {
            writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"x").unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        std::fs::write(&path, &bytes).unwrap();
        assert!(prepare(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_demo_uses_embedded_scripts_and_keeps_the_source() {
        let root = std::env::temp_dir().join(format!("markdownx-docx-view-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("paper.docx");
        let bytes = include_bytes!("../MarkdownX-Demo/示例合同.docx");
        std::fs::write(&path, bytes).unwrap();
        let (text, preview) = prepare(&path).unwrap();
        assert!(text.contains("分镜制作服务合同示例"));
        assert_eq!(preview.matches("data:text/javascript;base64,").count(), 2);
        assert!(preview.contains("renderAltChunks:false") && preview.contains("renderChanges:true"));
        assert!(!preview.contains(&path.to_string_lossy().to_string()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_a_main_relationship_that_disagrees_with_the_text_extractor() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("[Content_Types].xml", b"x".as_slice()),
            ("word/document.xml", b"x".as_slice()),
            ("_rels/.rels", br#"<Relationships><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/other.xml"/></Relationships>"#.as_slice()),
        ] {
            writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(bytes).unwrap();
        }
        assert!(html(&writer.finish().unwrap().into_inner()).is_err());
    }
}
