//! Preserve unchanged source while editing an ordinary Markdown file.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::model::{Block, BlockId, BlockKind, Document, Marks, RichText};
use crate::workspace_fs::SaveFileError;

#[derive(Clone)]
struct Fragment { block: Block, leading: String, raw: String }

#[derive(Clone)]
struct Header { leading: String, raw: String, title: String }

#[derive(Clone)]
pub struct Source {
    original: String,
    document: Document,
    bom: bool,
    newline: &'static str,
    header: Option<Header>,
    blocks: Vec<Fragment>,
    tail: String,
}

pub struct Rendered {
    pub text: String,
    pub source: Source,
    pub files: Vec<(PathBuf, String)>,
}

fn stem(path: &Path) -> &str {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or(crate::model::UNTITLED)
}

/// Byte ranges of physical lines, including each line's original ending.
fn lines(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    let mut result = vec![];
    while i < bytes.len() {
        if matches!(bytes[i], b'\r' | b'\n') {
            let end = if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') { i + 2 } else { i + 1 };
            result.push(start..end);
            start = end;
            i = end;
        } else { i += 1; }
    }
    if start < bytes.len() { result.push(start..bytes.len()); }
    if result.is_empty() { result.push(0..0); }
    result
}

fn line_span(lines: &[Range<usize>], range: &Range<usize>) -> Range<usize> {
    lines[range.start].start..lines[range.end - 1].end
}

pub fn parse(path: &Path, text: &str) -> (Document, Source) {
    let bom = text.starts_with('\u{feff}');
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let crlf = body.as_bytes().windows(2).filter(|p| *p == b"\r\n").count();
    let lf = body.bytes().filter(|&b| b == b'\n').count();
    let newline = if crlf > 0 && crlf * 2 >= lf { "\r\n" } else if lf == 0 && body.contains('\r') { "\r" } else { "\n" };
    let raw_lines = lines(body);
    let (mut blocks, mut spans) = crate::paste::markdown_file_with_spans(body);
    debug_assert_eq!(blocks.len(), spans.len());
    let mut header = None;
    let mut position = 0;
    let title = if blocks.first().is_some_and(|b| matches!(b.kind, BlockKind::Heading { level: 1, .. })) {
        let block = blocks.remove(0);
        let range = line_span(&raw_lines, &spans.remove(0));
        let title = if block.text.text.trim().is_empty() { stem(path).to_string() } else { block.text.text };
        header = Some(Header { leading: body[..range.start].into(), raw: body[range.clone()].into(), title: title.clone() });
        position = range.end;
        title
    } else { stem(path).to_string() };
    let mut fragments = vec![];
    for (block, span) in blocks.iter().cloned().zip(spans) {
        let range = line_span(&raw_lines, &span);
        fragments.push(Fragment { block, leading: body[position..range.start].into(), raw: body[range.clone()].into() });
        position = range.end;
    }
    // A blank Markdown file still needs a body target for the caret.
    if blocks.is_empty() { blocks.push(Block::text("")); }
    let doc = Document { title: RichText::plain(title), icon: None, cover: None, blocks };
    let source = Source { original: text.into(), document: doc.clone(), bom, newline, header, blocks: fragments, tail: body[position..].into() };
    (doc, source)
}

fn without_ending(s: &str) -> (&str, &str) {
    if let Some(body) = s.strip_suffix("\r\n") { (body, "\r\n") }
    else if let Some(body) = s.strip_suffix('\n') { (body, "\n") }
    else if let Some(body) = s.strip_suffix('\r') { (body, "\r") }
    else { (s, "") }
}

/// For a plain one-line block, edit only the changed characters. In
/// particular, tabs, hard-break spaces and literal HTML remain untouched.
fn patch_plain(old: &Block, new: &Block, raw: &str) -> Option<String> {
    if !matches!(old.kind, BlockKind::Text) || !matches!(new.kind, BlockKind::Text)
        || old.align != new.align || old.bg != new.bg || old.folded != new.folded
        || !old.children.is_empty() || !new.children.is_empty()
        || old.text.runs.iter().chain(&new.text.runs).any(|r| r.marks != Marks::default())
        || new.text.text.contains(['\r', '\n']) { return None; }
    let (body, _) = without_ending(raw);
    if body.contains(['\r', '\n']) { return None; }
    let trimmed = body.trim_end();
    let mut transformed = String::new();
    let mut map = vec![];
    let mut chars = trimmed.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        let mut end = start + ch.len_utf8();
        if ch == '\t' {
            while chars.peek().is_some_and(|(_, c)| *c == '\t') {
                let (at, c) = chars.next().unwrap();
                end = at + c.len_utf8();
            }
            transformed.push(' ');
        } else { transformed.push(ch); }
        map.push(start..end);
    }
    if transformed != old.text.text { return None; }
    let before: Vec<char> = old.text.text.chars().collect();
    let after: Vec<char> = new.text.text.chars().collect();
    let prefix = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
    let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    let from = map.get(prefix).map_or(trimmed.len(), |r| r.start);
    let to = map.get(before.len() - suffix).map_or(trimmed.len(), |r| r.start);
    let new_offsets: Vec<usize> = new.text.text.char_indices().map(|(i, _)| i).chain(std::iter::once(new.text.text.len())).collect();
    let inserted = &new.text.text[new_offsets[prefix]..new_offsets[after.len() - suffix]];
    let mut escaped = String::new();
    for c in inserted.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') { escaped.push('\\'); }
        escaped.push(c);
    }
    let patched = format!("{}{}{}", &raw[..from], escaped, &raw[to..]);
    let matches_plain = |s: &str| {
        let (blocks, _) = crate::paste::markdown_file_with_spans(s);
        blocks.len() == 1 && matches!(blocks[0].kind, BlockKind::Text) && blocks[0].text == new.text
    };
    if matches_plain(&patched) { return Some(patched); }
    // Typing after a literal `#`, list marker or fence can otherwise turn
    // the existing text into Markdown structure. Escape only that marker.
    let line = patched.trim_start_matches([' ', '\t']);
    let indent = patched.len() - line.len();
    for (at, c) in line.char_indices().take(12) {
        if !c.is_ascii_punctuation() { continue; }
        let mut trial = patched.clone();
        trial.insert(indent + at, '\\');
        if matches_plain(&trial) { return Some(trial); }
    }
    None
}

/// Keep the original marker and nested source when editing a simple list or heading.
fn patch_marked(old: &Block, new: &Block, raw: &str) -> Option<String> {
    if old.kind != new.kind || old.align != new.align || old.bg != new.bg || old.folded != new.folded || old.children != new.children
        || !matches!(old.kind, BlockKind::Heading { .. } | BlockKind::Bullet | BlockKind::Ordered { .. } | BlockKind::Todo { .. })
        || old.text.text.is_empty() || new.text.text.contains(['\r', '\n'])
        || old.text.runs.iter().chain(&new.text.runs).any(|r| r.marks != Marks::default()) { return None; }
    let first_end = raw.find(['\r', '\n']).unwrap_or(raw.len());
    let first = &raw[..first_end];
    let mut positions = first.match_indices(&old.text.text);
    let (pos, _) = positions.next()?;
    if positions.next().is_some() { return None; }
    let before: Vec<char> = old.text.text.chars().collect();
    let after: Vec<char> = new.text.text.chars().collect();
    let prefix = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
    let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    let old_offsets: Vec<_> = old.text.text.char_indices().map(|(i, _)| i).chain(std::iter::once(old.text.text.len())).collect();
    let new_offsets: Vec<_> = new.text.text.char_indices().map(|(i, _)| i).chain(std::iter::once(new.text.text.len())).collect();
    let mut escaped = String::new();
    for c in new.text.text[new_offsets[prefix]..new_offsets[after.len() - suffix]].chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') { escaped.push('\\'); }
        escaped.push(c);
    }
    Some(format!("{}{}{}", &raw[..pos + old_offsets[prefix]], escaped, &raw[pos + old_offsets[before.len() - suffix]..]))
}

/// Toggle a task without rewriting its nested items or line endings.
fn patch_task(old: &Block, new: &Block, raw: &str) -> Option<String> {
    let (BlockKind::Todo { done: before }, BlockKind::Todo { done: after }) = (&old.kind, &new.kind) else { return None };
    if before == after || old.text != new.text || old.align != new.align || old.bg != new.bg || old.folded != new.folded || old.children != new.children { return None; }
    let end = raw.find(['\r', '\n']).unwrap_or(raw.len());
    let line = &raw[..end];
    let (at, _) = line.match_indices("[ ]").chain(line.match_indices("[x]")).chain(line.match_indices("[X]"))
        .filter(|(_, marker)| (*marker != "[ ]") == *before).min_by_key(|(at, _)| *at)?;
    let mut result = raw.to_string();
    result.replace_range(at + 1..at + 2, if *after { "x" } else { " " });
    Some(result)
}

/// Keep a code fence's original language/options when only its body changes.
fn patch_fence(old: &Block, new: &Block, raw: &str, newline: &str) -> Option<String> {
    let (BlockKind::Code { lang: a, caption: ac, wrap: aw }, BlockKind::Code { lang: b, caption: bc, wrap: bw }) = (&old.kind, &new.kind) else { return None };
    if a != b || ac != bc || aw != bw || old.align != new.align || old.bg != new.bg || old.folded != new.folded || old.children != new.children { return None; }
    let ranges = lines(raw);
    let (opening, ending) = without_ending(&raw[ranges.first()?.clone()]);
    let indent = opening.len() - opening.trim_start_matches([' ', '\t']).len();
    let rest = &opening[indent..];
    let marker = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let count = rest.chars().take_while(|c| *c == marker).count();
    if count < 3 { return None; }
    let info = &rest[count..];
    let mut longest = 0;
    let mut run = 0;
    for c in new.text.text.chars() {
        if c == marker { run += 1; longest = longest.max(run); } else { run = 0; }
    }
    let fence = marker.to_string().repeat(count.max(longest + 1));
    let last = raw[ranges.last()?.clone()].trim();
    let closed = ranges.len() > 1 && last.chars().count() >= count && last.chars().all(|c| c == marker);
    let mut output = format!("{}{}{}{}", &opening[..indent], fence, info, if ending.is_empty() { newline } else { ending });
    for (i, line) in new.text.text.split('\n').enumerate() {
        if i > 0 { output.push_str(newline); }
        output.push_str(&opening[..indent]);
        output.push_str(line);
    }
    if closed {
        let (_, final_ending) = without_ending(&raw[ranges.last()?.clone()]);
        output.push_str(newline);
        output.push_str(&opening[..indent]);
        output.push_str(&fence);
        output.push_str(final_ending);
    }
    Some(output)
}

fn ended(text: &str) -> bool { text.ends_with(['\r', '\n']) }

fn separate(out: &mut String, previous: Option<&Block>, next: &Block, newline: &str) {
    if !ended(out) { out.push_str(newline); }
    let tight = previous.is_some_and(|p| p.kind.is_list() && next.kind.is_list()) && !matches!(next.kind, BlockKind::Ordered { seq: Some(_) });
    if !tight { out.push_str(newline); }
}

fn normalize(block: &mut Block, root: &Path, files: &[(PathBuf, String)]) {
    block.id = BlockId(0);
    if let BlockKind::Image { src_kind, src, .. } = &mut block.kind {
        *src_kind = "src".into();
        let full = root.join(src.as_str()).canonicalize().ok();
        if let Some((_, rel)) = files.iter().find(|(path, _)| full.as_ref().is_some_and(|p| path.canonicalize().ok().as_ref() == Some(p))) {
            *src = rel.replace(' ', "%20");
        }
    }
    if let BlockKind::Table(table) = &mut block.kind {
        for cell in table.rows.iter_mut().flatten() {
            for child in &mut cell.blocks { normalize(child, root, files); }
        }
    }
    for child in &mut block.children { normalize(child, root, files); }
}

fn empty_paragraph(block: &Block) -> bool {
    matches!(block.kind, BlockKind::Text) && block.text.text.is_empty() && block.text.runs.is_empty()
        && block.align == crate::model::Align::Left && block.bg.is_none() && !block.folded && block.children.is_empty()
}

fn equivalent(path: &Path, expected: &Document, actual: &Document, files: &[(PathBuf, String)]) -> bool {
    let mut expected = expected.clone();
    let mut actual = actual.clone();
    // Blank paragraphs are represented by separators in Markdown and do not
    // produce file-mode blocks when the source is parsed again.
    expected.blocks.retain(|b| !empty_paragraph(b));
    actual.blocks.retain(|b| !empty_paragraph(b));
    if expected.title.text.trim().is_empty() { expected.title = RichText::plain(stem(path)); }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    for block in &mut expected.blocks { normalize(block, root, files); }
    for block in &mut actual.blocks { normalize(block, root, &[]); }
    expected == actual
}

impl Source { pub fn matches(&self, text: &str) -> bool { self.original == text } }

pub fn render(path: &Path, source: &Source, doc: &Document) -> Result<Rendered, SaveFileError> {
    if doc == &source.document {
        return Ok(Rendered { text: source.original.clone(), source: source.clone(), files: vec![] });
    }
    let opts = crate::markdown::Options { library_root: path.parent(), assets: Some("assets"), ..Default::default() };
    let mut writer = crate::markdown::FragmentWriter::new(&opts);
    let mut out = if source.bom { "\u{feff}".to_string() } else { String::new() };
    let has_header = !doc.title.text.trim().is_empty() && (source.header.is_some() || doc.title.text != stem(path));
    let same_header_position = has_header == source.header.is_some();
    if has_header {
        if let Some(header) = &source.header {
            out.push_str(&header.leading);
            if header.title == doc.title.text { out.push_str(&header.raw); }
            else {
                let title_doc = Document { title: doc.title.clone(), icon: None, cover: None, blocks: vec![] };
                let title = crate::markdown::export(&title_doc, &opts).text;
                let escaped = title.strip_prefix("# ").unwrap_or("").trim_end_matches('\n');
                if header.title.is_empty() || header.raw.matches(&header.title).count() != 1 { return Err(SaveFileError::Lossy); }
                out.push_str(&header.raw.replacen(&header.title, escaped, 1));
            }
        } else {
            let title_doc = Document { title: doc.title.clone(), icon: None, cover: None, blocks: vec![] };
            out.push_str(&crate::markdown::export(&title_doc, &opts).text.replace('\n', source.newline));
        }
    } else if source.header.as_ref().is_some_and(|h| !h.leading.trim().is_empty()) {
        return Err(SaveFileError::Lossy);
    }
    let indexed: HashMap<_, _> = source.blocks.iter().enumerate().map(|(i, f)| (f.block.id, (i, f))).collect();
    let mut used = HashSet::new();
    let mut previous_index = None;
    let mut previous_block: Option<&Block> = None;
    let mut have_content = has_header;
    for (i, block) in doc.blocks.iter().enumerate() {
        let old = indexed.get(&block.id).copied();
        let adjacent = old.is_some_and(|(index, _)| if i == 0 { index == 0 && same_header_position } else { previous_index == Some(index.wrapping_sub(1)) && index > 0 });
        if adjacent {
            let (index, fragment) = old.unwrap();
            out.push_str(&fragment.leading);
            used.insert(index);
        } else if have_content {
            separate(&mut out, previous_block, block, source.newline);
        }
        let unchanged = old.is_some_and(|(_, f)| f.block == *block);
        let patched = old.filter(|_| !unchanged).and_then(|(_, f)| patch_plain(&f.block, block, &f.raw).or_else(|| patch_marked(&f.block, block, &f.raw)).or_else(|| patch_task(&f.block, block, &f.raw)).or_else(|| patch_fence(&f.block, block, &f.raw, source.newline)));
        let generated = writer.block(block, !unchanged && patched.is_none());
        let raw = if let Some((_, fragment)) = old.filter(|_| unchanged) { fragment.raw.clone() }
            else if let Some(patched) = patched { patched }
            else {
                if old.is_some_and(|(_, f)| f.raw.contains('<') || f.raw.contains("](")
                    || f.raw.lines().any(|line| line.ends_with("  ") || line.ends_with('\\'))
                    || (f.raw.lines().count() > 1 && !matches!(f.block.kind, BlockKind::Code { .. }))) { return Err(SaveFileError::Lossy); }
                let mut rendered = generated.unwrap_or_default().replace('\n', source.newline);
                let needs_ending = old.map_or(i + 1 < doc.blocks.len() || ended(&source.original), |(_, f)| ended(&f.raw));
                if needs_ending { rendered.push_str(source.newline); }
                rendered
            };
        out.push_str(&raw);
        previous_index = old.map(|(index, _)| index);
        previous_block = Some(block);
        have_content = true;
    }
    for (index, fragment) in source.blocks.iter().enumerate() {
        if !used.contains(&index) && !fragment.leading.trim().is_empty() { return Err(SaveFileError::Lossy); }
    }
    out.push_str(&source.tail);
    let (files, missing) = writer.finish();
    if missing > 0 { return Err(SaveFileError::MissingAssets(missing)); }
    let (reparsed, mut next_source) = parse(path, &out);
    if !equivalent(path, doc, &reparsed, &files) { return Err(SaveFileError::Lossy); }
    next_source.document = doc.clone();
    for (fragment, block) in next_source.blocks.iter_mut().zip(doc.blocks.iter().filter(|b| !empty_paragraph(b))) { fragment.block = block.clone(); }
    if let Some(header) = &mut next_source.header { header.title = doc.title.text.clone(); }
    Ok(Rendered { text: out, source: next_source, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads a user-selected folder without modifying it"]
    fn selected_folder_preserves_original_and_simple_edits() {
        let root = std::env::var_os("MARKDOWNX_WORKSPACE_TEST_ROOT").expect("selected folder is required");
        let folder = crate::workspace_fs::WorkspaceDir::scan(root.into()).unwrap();
        let mut checked = 0;
        let mut edited = 0;
        let mut rejected = 0;
        let mut empty_edited = 0;
        for item in folder.files() {
            if crate::workspace_fs::DocKind::of(&item.path) != Some(crate::workspace_fs::DocKind::Markdown) { continue; }
            let text = std::fs::read_to_string(&item.path).unwrap();
            let (mut doc, source) = parse(&item.path, &text);
            assert_eq!(render(&item.path, &source, &doc).unwrap().text, text);
            checked += 1;
            if source.blocks.is_empty() && doc.blocks.len() == 1 && empty_paragraph(&doc.blocks[0]) {
                doc.blocks[0].text.push("probe", Marks::default());
                assert!(render(&item.path, &source, &doc).unwrap().text.contains("probe"));
                empty_edited += 1;
                continue;
            }
            if let Some((index, replacement)) = source.blocks.iter().enumerate().find_map(|(index, fragment)| {
                if text.matches(&fragment.raw).count() != 1 { return None; }
                let mut next = fragment.block.clone();
                if !matches!(next.kind, BlockKind::Text) || next.text.text.is_empty() { return None; }
                next.text.push(" test", Marks::default());
                patch_plain(&fragment.block, &next, &fragment.raw).map(|raw| (index, raw))
            }) {
                let original = &source.blocks[index].raw;
                doc.blocks[index].text.push(" test", Marks::default());
                match render(&item.path, &source, &doc) {
                    Ok(result) => { assert_eq!(result.text, text.replacen(original, &replacement, 1)); edited += 1; }
                    Err(SaveFileError::Lossy) => rejected += 1,
                    Err(_) => panic!("unexpected source-preservation failure"),
                }
            }
        }
        if let Ok(expected) = std::env::var("MARKDOWNX_EXPECTED_WORKSPACE_FILES") { assert_eq!(checked, expected.parse::<usize>().unwrap()); }
        println!("Read-only selected-folder source check: {checked} exact, {edited} text edits and {empty_edited} empty-file edits in memory, {rejected} protected rejections");
        assert!(edited > 0);
    }

    #[test]
    fn edits_preserve_unmodified_source_and_hardbreaks() {
        let path = Path::new("example.md");
        let original = "\u{feff}# **Title**\r\n\r\nfirst line\r\nsecond line  \r\n\r\n<!-- note -->\r\n\r\n```rust title=demo\r\nlet x = 1;\r\n```\r\n";
        let (mut doc, source) = parse(path, original);
        assert_eq!(render(path, &source, &doc).unwrap().text, original);
        doc.blocks[0].text = RichText::plain("first line edited");
        let result = render(path, &source, &doc).unwrap();
        assert_eq!(result.text, original.replacen("first line", "first line edited", 1));
        doc.blocks[1].text = RichText::plain("second line edited");
        let result = render(path, &result.source, &doc).unwrap();
        assert!(result.text.contains("second line edited  \r\n"));
        assert!(result.text.contains("<!-- note -->"));
        assert!(result.text.contains("```rust title=demo\r\n"));
    }

    #[test]
    fn editing_code_preserves_fence_options_and_uses_a_safe_fence() {
        let path = Path::new("example.md");
        let original = "```rust title=demo\nlet x = 1;\n```\n\nnext\n";
        let (mut doc, source) = parse(path, original);
        doc.blocks[0].text = RichText::plain("let x = 2;\n````\nend");
        let result = render(path, &source, &doc).unwrap();
        assert!(result.text.starts_with("`````rust title=demo\n"));
        assert!(result.text.contains("\n`````\n\nnext\n"));
    }

    #[test]
    fn editing_plain_text_retains_tabs_and_literal_html() {
        let path = Path::new("example.md");
        let original = "a\t\tb  \n<span>before</span>\n";
        let (mut doc, source) = parse(path, original);
        doc.blocks[0].text = RichText::plain("a b changed");
        let result = render(path, &source, &doc).unwrap();
        assert!(result.text.starts_with("a\t\tb changed  \n"));
        doc.blocks[1].text = RichText::plain("<span>after</span>");
        let result = render(path, &result.source, &doc).unwrap();
        assert!(result.text.contains("<span>after</span>"));
    }

    #[test]
    fn plain_edits_escape_new_markdown_and_literal_block_markers() {
        let path = Path::new("example.md");
        let (mut doc, source) = parse(path, "#\n");
        doc.blocks[0].text.push(" title", Marks::default());
        assert_eq!(render(path, &source, &doc).unwrap().text, "\\# title\n");
        let (mut doc, source) = parse(path, "body\n");
        doc.blocks[0].text.push(" *stars* <tag> &amp;", Marks::default());
        assert_eq!(render(path, &source, &doc).unwrap().text, "body \\*stars\\* \\<tag> \\&amp;\n");
    }

    #[test]
    fn list_edits_keep_children_and_source_conventions() {
        let path = Path::new("example.md");
        let original = "+ parent  \n  - nested\n\nnext\n";
        let (mut doc, source) = parse(path, original);
        doc.blocks[0].text.push(" edited", Marks::default());
        assert_eq!(render(path, &source, &doc).unwrap().text, "+ parent edited  \n  - nested\n\nnext\n");
    }

    #[test]
    fn toggling_a_nested_task_keeps_the_original_source() {
        let path = Path::new("example.md");
        let original = "- [ ] parent  \n  - child\n";
        let (mut doc, source) = parse(path, original);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Todo { done: false }));
        doc.blocks[0].kind = BlockKind::Todo { done: true };
        assert_eq!(render(path, &source, &doc).unwrap().text, "- [x] parent  \n  - child\n");
    }

    #[test]
    fn empty_paragraphs_can_be_saved_and_edited_later() {
        let path = Path::new("example.md");
        let (mut doc, source) = parse(path, "# Old\n\nbody\n");
        doc.title = RichText::plain("New");
        doc.blocks = vec![Block::text("")];
        let first = render(path, &source, &doc).unwrap();
        assert!(first.text.starts_with("# New\n"));
        doc.blocks[0].text = RichText::plain("new body");
        let second = render(path, &first.source, &doc).unwrap();
        assert!(second.text.contains("new body"));

        let (mut doc, source) = parse(path, "# Title\n\nbody\n");
        doc.blocks.insert(0, Block::text(""));
        let first = render(path, &source, &doc).unwrap();
        doc.blocks[1].text = RichText::plain("body edited");
        let second = render(path, &first.source, &doc).unwrap();
        assert!(second.text.contains("body edited"));
    }

    #[test]
    fn blank_files_open_with_a_body_caret_and_save_new_text() {
        let path = Path::new("note.md");
        for original in ["", "\u{feff}\r\n", "# Heading\n"] {
            let (doc, source) = parse(path, original);
            assert_eq!(doc.blocks.len(), 1);
            assert!(empty_paragraph(&doc.blocks[0]));
            assert_eq!(render(path, &source, &doc).unwrap().text, original);
            let old_title = doc.title.text.clone();
            let mut core = crate::editor::core::Core::new(doc);
            core.insert_text("first text");
            assert_eq!(core.doc.title.text, old_title);
            let saved = render(path, &source, &core.doc).unwrap();
            assert!(saved.text.contains("first text"));
            assert_eq!(parse(path, &saved.text).0.blocks[0].text.text, "first text");
        }
    }

    #[test]
    fn unsupported_formatting_is_not_silently_discarded() {
        let path = Path::new("example.md");
        let (mut doc, source) = parse(path, "plain\n");
        doc.blocks[0].bg = Some(crate::model::ColorName("red".into()));
        assert!(matches!(render(path, &source, &doc), Err(SaveFileError::Lossy)));
    }

    #[test]
    fn heading_edits_keep_existing_markup() {
        let path = Path::new("example.md");
        let (mut doc, source) = parse(path, "# **Old**\n\nbody\n");
        doc.title = RichText::plain("New");
        assert_eq!(render(path, &source, &doc).unwrap().text, "# **New**\n\nbody\n");
    }
}
