//! Stable heading fragments shared by document navigation and HTML export.
use crate::model::{BlockId, BlockKind, Document};
use std::collections::HashSet;

/// Lowercase Unicode letters and digits, keep `_` / `-`, and turn spaces into
/// hyphens. Punctuation is omitted, as in common Markdown heading fragments.
pub fn slug(text: &str) -> String {
    text.trim().chars().flat_map(char::to_lowercase).filter_map(|ch| {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_') { Some(ch) }
        else if ch.is_whitespace() { Some('-') }
        else { None }
    }).collect()
}

pub struct Targets { pub title: Option<String>, pub headings: Vec<(BlockId, String)> }

/// Assign distinct fragments in document order, including the document title.
pub fn targets(doc: &Document) -> Targets {
    targets_with_title(doc, true)
}

/// Imported Markdown chapters have a synthetic page title in addition to the
/// original Markdown headings. It must not consume a source heading fragment.
pub fn targets_with_title(doc: &Document, include_title: bool) -> Targets {
    targets_with_title_transform(doc, include_title, str::to_owned)
}

/// Assign anchors after a caller restores source text used by temporary render markers.
pub fn targets_with_title_transform(doc: &Document, include_title: bool, transform: impl Fn(&str) -> String) -> Targets {
    let mut used = HashSet::new();
    let mut make = |text: &str| {
        let base = slug(text);
        if base.is_empty() { return None; }
        let mut candidate = base.clone();
        let mut n = 1;
        while used.contains(&candidate) { candidate = format!("{base}-{n}"); n += 1; }
        used.insert(candidate.clone());
        Some(candidate)
    };
    let title = if include_title { make(&transform(&doc.title.plain_text())) } else { None };
    let mut headings = vec![];
    doc.for_each_block(|block| if matches!(block.kind, BlockKind::Heading { .. })
        && let Some(anchor) = make(&transform(&block.text.plain_text())) { headings.push((block.id, anchor)); });
    Targets { title, headings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_slug_and_duplicates() {
        assert_eq!(slug(" 合同：付款 条款! "), "合同付款-条款");
        assert_eq!(slug("ΟΣ"), "οσ");
        let blocks = crate::paste::markdown_to_blocks("## Résumé\n## Résumé\n## Résumé-1\n", crate::paste::Off { file: true, ..Default::default() });
        let doc = Document { blocks, ..Default::default() };
        let names: Vec<_> = targets(&doc).headings.into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["résumé", "résumé-1", "résumé-1-1"]);
        let blocks = crate::paste::markdown_to_blocks("# 章节\n## 章节\n", crate::paste::Off { file: true, ..Default::default() });
        let doc = Document { title: crate::model::RichText::plain("章节"), blocks, ..Default::default() };
        let names: Vec<_> = targets_with_title(&doc, false).headings.into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["章节", "章节-1"]);
    }

    #[test]
    fn media_and_equations_use_alt_text_and_tex_source() {
        let md = "## 图解 ![镜头](./shot.svg)\n\n## 公式 $\\frac{1}{2}$\n";
        let blocks = crate::paste::markdown_to_blocks(md, crate::paste::Off { file: true, ..Default::default() });
        let doc = Document { blocks, ..Default::default() };
        let names: Vec<_> = targets(&doc).headings.into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["图解-镜头", "公式-frac12"]);
        let xml = r#"<title>Legal</title><h2>章节 <inline-image src="./shot.svg" alt="镜头"/></h2><h2>公式 <latex>\alpha</latex></h2>"#;
        let doc = crate::docxml::parse(xml).unwrap();
        let names: Vec<_> = targets(&doc).headings.into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["章节-镜头", "公式-alpha"]);
    }
}
