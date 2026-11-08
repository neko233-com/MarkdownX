use fdl_core::book::Book;
use fdl_core::markdown;
use std::path::Path;

#[test]
fn example_books_parse_into_readable_chapters() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("examples directory") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdxb") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let book = Book::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        assert!(!book.chapters.is_empty(), "{} has no chapters", path.display());
        let docs = book.chapter_documents().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(docs.len(), book.chapters.len());
        for doc in docs {
            assert!(!doc.document.title.text.trim().is_empty(), "{}: empty title", path.display());
        }
        seen += 1;
    }
    assert!(seen >= 5, "expected the demo books, found {seen}");
}

/// `.mdxb` → `.md` is allowed to lose presentation, but not structure:
/// a chapter must stay a heading, tables stay pipe tables, and interactive
/// sources stay fenced so they can be packed again.
#[test]
fn books_convert_to_structured_markdown() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut md = String::new();
    for entry in std::fs::read_dir(&dir).expect("examples directory") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdxb") {
            continue;
        }
        let book = Book::parse(&std::fs::read(&path).unwrap()).unwrap();
        for chapter in book.chapter_documents().unwrap() {
            let out = markdown::export(
                &chapter.document,
                &markdown::Options { assets: Some("assets"), ..Default::default() },
            );
            assert!(!out.text.trim().is_empty(), "{} converted to empty Markdown", path.display());
            md.push_str(&out.text);
            md.push('\n');
        }
    }
    assert!(md.contains("mdxb-canvas"), "Canvas source was lost");
    assert!(md.contains("mdxb-html"), "HTML source was lost");
    assert!(md.contains('|'), "table structure was lost");
}
