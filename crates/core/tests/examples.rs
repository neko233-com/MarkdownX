use fdl_core::book::Book;
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
