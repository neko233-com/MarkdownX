//! Legacy Word text extraction in a bounded helper process. The worker only
//! receives bytes over stdin and cannot write to the original document.
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

const MAX_DOC: u64 = 32 * 1024 * 1024;
const MAX_TEXT: u64 = 16 * 1024 * 1024;

fn extract(bytes: &[u8]) -> Result<String, String> {
    let doc = undoc::parse_bytes(bytes).map_err(|_| "Legacy Word document is unsupported or damaged")?;
    let options = undoc::render::RenderOptions::default().with_include_headers_footers(true)
        .with_revision_handling(undoc::render::RevisionHandling::ShowMarkup);
    let text = undoc::render::to_markdown(&doc, &options).map_err(|_| "Legacy Word text could not be extracted")?;
    if text.len() as u64 > MAX_TEXT { return Err("Legacy Word text is too large".into()); }
    Ok(text)
}

pub(crate) fn worker() -> Result<(), String> {
    let mut bytes = Vec::new();
    io::stdin().lock().take(MAX_DOC + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DOC { return Err("Legacy Word document is too large".into()); }
    let text = extract(&bytes)?;
    io::stdout().lock().write_all(text.as_bytes()).map_err(|e| e.to_string())
}

pub(crate) fn index(path: &Path) -> Result<String, String> {
    let bytes = crate::store::read_bounded(path, MAX_DOC).map_err(|e| e.to_string())?;
    let output = crate::document_worker::run(bytes, "--internal-doc-text-worker", MAX_TEXT, Duration::from_secs(45), "Legacy Word")?;
    String::from_utf8(output).map_err(|_| "Legacy Word text is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_chinese_notes_and_headers_without_reading_a_user_file() {
        let demo = extract(include_bytes!("../MarkdownX-Demo/旧版合同示例.doc")).unwrap();
        assert!(demo.contains("旧版合同示例") && demo.contains("日本語") && demo.contains('😀'));
        let template = extract(include_bytes!("../crates/core/tests/fixtures/legacy-doc/template.dot")).unwrap();
        assert!(template.contains("旧版合同示例") && template.contains("日本語"));
        let notes = extract(include_bytes!("../crates/core/tests/fixtures/legacy-doc/footnote.doc")).unwrap();
        assert!(notes.contains("TestFootnote") && notes.contains("TestEndnote"));
        let headers = extract(include_bytes!("../crates/core/tests/fixtures/legacy-doc/HeaderFooterUnicode.doc")).unwrap();
        assert!(headers.contains("simple header") && headers.contains("The footer") && headers.contains('€'));
        let revisions = extract(include_bytes!("../crates/core/tests/fixtures/legacy-doc/revisions.doc")).unwrap();
        assert!(revisions.contains("~~") && revisions.contains("simple"), "deleted text was not marked: {revisions}");
        assert!(extract(b"not a Word document").is_err());
    }
}
