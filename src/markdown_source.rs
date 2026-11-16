//! Preserve unchanged source while editing an ordinary Markdown file.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::model::{Block, BlockId, BlockKind, Document, Marks, RichText};
use crate::workspace_fs::SaveFileError;

#[derive(Clone)]
struct Fragment { block: Block, leading: String, raw: String }

#[derive(Clone)]
struct Header { leading: String, raw: String, title: RichText }

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
    parse_with_root(path, text, None)
}

pub(crate) fn parse_for_book(path: &Path, text: &str, root: &Path) -> (Document, Source) {
    parse_with_root(path, text, Some(root))
}

fn parse_with_root(path: &Path, text: &str, root: Option<&Path>) -> (Document, Source) {
    let bom = text.starts_with('\u{feff}');
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let crlf = body.as_bytes().windows(2).filter(|p| *p == b"\r\n").count();
    let lf = body.bytes().filter(|&b| b == b'\n').count();
    let newline = if crlf > 0 && crlf * 2 >= lf { "\r\n" } else if lf == 0 && body.contains('\r') { "\r" } else { "\n" };
    let raw_lines = lines(body);
    let (mut blocks, mut spans) = crate::paste::markdown_file_with_spans(body);
    if let Some(base) = path.parent() {
        if let Some(root) = root { crate::transfer::resolve_book_assets(&mut blocks, base, root); }
        else { crate::transfer::resolve_images(&mut blocks, base); }
    }
    debug_assert_eq!(blocks.len(), spans.len());
    let mut header = None;
    let mut position = 0;
    let title = if blocks.first().is_some_and(|b| matches!(b.kind, BlockKind::Heading { level: 1, .. })) {
        let block = blocks.remove(0);
        let range = line_span(&raw_lines, &spans.remove(0));
        let title = if block.text.text.trim().is_empty() { RichText::plain(stem(path)) } else { block.text };
        header = Some(Header { leading: body[..range.start].into(), raw: body[range.clone()].into(), title: title.clone() });
        position = range.end;
        title
    } else { RichText::plain(stem(path)) };
    let mut fragments = vec![];
    for (block, span) in blocks.iter().cloned().zip(spans) {
        let range = line_span(&raw_lines, &span);
        fragments.push(Fragment { block, leading: body[position..range.start].into(), raw: body[range.clone()].into() });
        position = range.end;
    }
    // A blank Markdown file still needs a body target for the caret.
    if blocks.is_empty() { blocks.push(Block::text("")); }
    let doc = Document { title, icon: None, cover: None, blocks };
    let source = Source { original: text.into(), document: doc.clone(), bom, newline, header, blocks: fragments, tail: body[position..].into() };
    (doc, source)
}

fn without_ending(s: &str) -> (&str, &str) {
    if let Some(body) = s.strip_suffix("\r\n") { (body, "\r\n") }
    else if let Some(body) = s.strip_suffix('\n') { (body, "\n") }
    else if let Some(body) = s.strip_suffix('\r') { (body, "\r") }
    else { (s, "") }
}

/// Keep display delimiters and line endings when the formula editor changes its source.
fn patch_display_equation(old: &Block, new: &Block, raw: &str) -> Option<String> {
    if old.text.text != crate::editor::core::EQUATION || new.text.text != crate::editor::core::EQUATION { return None; }
    let ([old_run], [new_run]) = (old.text.runs.as_slice(), new.text.runs.as_slice()) else { return None };
    let (old_obj, new_obj) = (old_run.marks.inline.as_ref()?, new_run.marks.inline.as_ref()?);
    if !old_obj.is_display_equation() || !new_obj.is_display_equation() { return None; }
    let mut old_block = old.clone(); let mut new_block = new.clone();
    old_block.text = RichText::default(); new_block.text = RichText::default();
    if old_block != new_block { return None; }
    let mut old_marks = old_run.marks.clone(); let mut new_marks = new_run.marks.clone();
    old_marks.inline = None; new_marks.inline = None;
    if old_marks != new_marks { return None; }
    let src = new_obj.equation()?.replace("\r\n", "\n").replace('\r', "\n");
    if src.trim().is_empty() || src.lines().any(|line| line.trim() == "$$") { return None; }
    let ranges = lines(raw);
    let first = &raw[ranges.first()?.clone()];
    let last = &raw[ranges.last()?.clone()];
    let (first_body, first_end) = without_ending(first);
    let (last_body, _) = without_ending(last);
    if ranges.len() >= 3 && first_body.trim() == "$$" && last_body.trim() == "$$" && !first_end.is_empty() {
        let body = src.replace('\n', first_end);
        return Some(format!("{first}{body}{first_end}{last}"));
    }
    if ranges.len() == 1 {
        let opening = first_body.find("$$")?;
        let closing = first_body.rfind("$$")?;
        if opening == closing || !first_body[..opening].trim().is_empty() || !first_body[closing + 2..].trim().is_empty() { return None; }
        if !src.contains('\n') { return Some(format!("{}{src}{}", &first_body[..opening + 2], &first[closing..])); }
        let newline = if first_end.is_empty() { "\n" } else { first_end };
        return Some(format!("{}{newline}{}{newline}{}", &first_body[..opening + 2], src.replace('\n', newline), &first[closing..]));
    }
    None
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

/// Preserve one styled title run when its text changes and its marks do not.
fn patch_styled_title(old: &RichText, new: &RichText, raw: &str) -> Option<String> {
    if old.runs.len() != 1 || new.runs.len() != 1 || old.runs[0].marks != new.runs[0].marks
        || old.runs[0].marks.inline.is_some() || old.text.is_empty() || new.text.contains(['\r', '\n']) { return None; }
    let mut positions = raw.match_indices(&old.text);
    let (at, _) = positions.next()?;
    if positions.next().is_some() { return None; }
    let mut text = String::new();
    for ch in new.text.chars() {
        if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') { text.push('\\'); }
        text.push(ch);
    }
    Some(format!("{}{}{}", &raw[..at], text, &raw[at + old.text.len()..]))
}

/// Edit a uniquely identifiable plain run while retaining surrounding rich markup.
/// The full-document equivalence check remains the final authority for saving.
fn patch_rich_plain_run(old: &RichText, new: &RichText, raw: &str, cell: bool) -> Option<(std::ops::Range<usize>, String)> {
    if old.text == new.text || old.text.contains(['\r', '\n']) || new.text.contains(['\r', '\n'])
        || old.runs.iter().all(|run| run.marks == Marks::default()) { return None; }
    let before: Vec<char> = old.text.chars().collect();
    let after: Vec<char> = new.text.chars().collect();
    let prefix = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
    let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    let offsets = |text: &str| text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect::<Vec<_>>();
    let a = offsets(&old.text);
    let b = offsets(&new.text);
    let (start, end) = (a[prefix], a[before.len() - suffix]);
    let mut candidate = None;
    for (run, marks) in old.spans() {
        if *marks != Marks::default() || run.start > start || run.end < end || run.is_empty() { continue; }
        let literal = &old.text[run.clone()];
        let mut positions = raw.match_indices(literal);
        let (at, _) = positions.next()?;
        if positions.next().is_some() || candidate.is_some() { return None; }
        candidate = Some((at + start - run.start)..(at + end - run.start));
    }
    let mut inserted = String::new();
    for ch in new.text[b[prefix]..b[after.len() - suffix]].chars() {
        if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') || cell && ch == '|' { inserted.push('\\'); }
        inserted.push(ch);
    }
    Some((candidate?, inserted))
}

fn patch_rich_plain(old: &Block, new: &Block, raw: &str, cell: bool) -> Option<String> {
    let mut comparable = new.clone(); comparable.text = old.text.clone();
    if comparable != *old { return None; }
    let (range, inserted) = patch_rich_plain_run(&old.text, &new.text, raw, cell)?;
    let mut result = raw.to_string(); result.replace_range(range, &inserted); Some(result)
}

/// Patch plain text inside an existing list tree without regenerating siblings.
/// Ambiguous source occurrences are rejected; the caller also reparses and
/// compares the complete document before permitting a save.
fn patch_nested_text(old: &Block, new: &Block, raw: &str, path: &Path, source: &str) -> Option<String> {
    if !old.kind.is_list() || !new.kind.is_list() { return None; }
    fn collect(old: &Block, new: &Block, raw: &str, path: &Path, source: &str, edits: &mut Vec<(std::ops::Range<usize>, String)>) -> Option<()> {
        if old.id != new.id || old.kind != new.kind || old.align != new.align || old.bg != new.bg || old.folded != new.folded
            || old.children.len() != new.children.len() { return None; }
        if old.text != new.text {
            if let Some(edit) = patch_rich_plain_run(&old.text, &new.text, raw, false) {
                edits.push(edit);
            } else if old.kind.is_list() && old.text.spans().any(|(_, marks)| marks.inline.as_ref().is_some_and(|inline| inline.image().is_some())) {
                let mut candidate = None;
                let mut inspected = 0;
                for range in lines(raw) {
                    let (line, _) = without_ending(&raw[range.clone()]);
                    let Some(span) = crate::paste::markdown_list_content_span(line) else { continue };
                    let content = &line[span.clone()];
                    if !content.contains("![") && !content.to_ascii_lowercase().contains("<img") { continue; }
                    inspected += 1;
                    if inspected > 4096 { return None; }
                    let mut probe = old.clone();
                    probe.text = crate::paste::markdown_file_inline(content);
                    probe.children.clear();
                    crate::transfer::resolve_images(std::slice::from_mut(&mut probe), path.parent()?);
                    if probe.text != old.text { continue; }
                    if candidate.is_some() { return None; }
                    let mut before = old.clone(); before.kind = BlockKind::Text; before.children.clear();
                    let mut after = new.clone(); after.kind = BlockKind::Text; after.children.clear();
                    let replacement = patch_inline_images(&before, &after, content, source, false)
                        .or_else(|| patch_inline_image_sources(&before, &after, content, source, false))?;
                    candidate = Some((range.start + span.start..range.start + span.end, replacement));
                }
                edits.push(candidate?);
            } else {
            if !matches!(old.kind, BlockKind::Text | BlockKind::Heading { .. } | BlockKind::Bullet | BlockKind::Ordered { .. } | BlockKind::Todo { .. })
                || old.text.text.is_empty() || old.text.text.contains(['\r', '\n']) || new.text.text.contains(['\r', '\n'])
                || old.text.runs.iter().chain(&new.text.runs).any(|run| run.marks != Marks::default()) { return None; }
            let mut matches = raw.match_indices(&old.text.text);
            let (at, _) = matches.next()?;
            if matches.next().is_some() { return None; }
            let before: Vec<char> = old.text.text.chars().collect();
            let after: Vec<char> = new.text.text.chars().collect();
            let prefix = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
            let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
            let a: Vec<_> = old.text.text.char_indices().map(|(i, _)| i).chain(std::iter::once(old.text.text.len())).collect();
            let b: Vec<_> = new.text.text.char_indices().map(|(i, _)| i).chain(std::iter::once(new.text.text.len())).collect();
            let mut inserted = String::new();
            for ch in new.text.text[b[prefix]..b[after.len() - suffix]].chars() {
                if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') { inserted.push('\\'); }
                inserted.push(ch);
            }
            edits.push((at + a[prefix]..at + a[before.len() - suffix], inserted));
            }
        }
        for (before, after) in old.children.iter().zip(&new.children) { collect(before, after, raw, path, source, edits)?; }
        Some(())
    }
    let mut edits = Vec::new();
    collect(old, new, raw, path, source, &mut edits)?;
    if edits.is_empty() { return None; }
    edits.sort_by_key(|(range, _)| range.start);
    if edits.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) { return None; }
    let mut output = raw.to_string();
    for (range, value) in edits.into_iter().rev() { output.replace_range(range, &value); }
    Some(output)
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

/// Preserve the original indentation and line endings when editing an
/// indented Markdown code block. Refuse ambiguous mappings instead of guessing.
fn patch_indented_code(old: &Block, new: &Block, raw: &str, newline: &str) -> Option<String> {
    let (BlockKind::Code { lang: a, caption: ac, wrap: aw }, BlockKind::Code { lang: b, caption: bc, wrap: bw }) = (&old.kind, &new.kind) else { return None };
    if a != b || ac != bc || aw != bw || old.align != new.align || old.bg != new.bg || old.folded != new.folded || old.children != new.children { return None; }
    let ranges = lines(raw);
    let old_lines: Vec<_> = old.text.text.split('\n').collect();
    if ranges.len() != old_lines.len() { return None; }
    let mut prefixes = Vec::with_capacity(ranges.len());
    for (range, code) in ranges.iter().zip(&old_lines) {
        let (line, _) = without_ending(&raw[range.clone()]);
        let prefix = line.strip_suffix(code)?;
        if !prefix.bytes().all(|c| matches!(c, b' ' | b'\t')) { return None; }
        let columns = prefix.bytes().fold(0, |n, byte| if byte == b'\t' { n + 4 - n % 4 } else { n + 1 });
        if columns < 4 && !code.is_empty() { return None; }
        prefixes.push(prefix);
    }
    let first = prefixes.iter().copied().find(|p| !p.is_empty())?;
    let final_ending = without_ending(&raw[ranges.last()?.clone()]).1;
    let lines: Vec<_> = new.text.text.split('\n').collect();
    let mut output = String::new();
    for (i, text) in lines.iter().enumerate() {
        let prefix = prefixes.get(i).copied().filter(|p| !p.is_empty()).unwrap_or(first);
        output.push_str(prefix); output.push_str(text);
        if i + 1 < lines.len() { output.push_str(newline); } else { output.push_str(final_ending); }
    }
    Some(output)
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

/// Change one image destination while retaining HTML attributes or Markdown title.
fn patch_image_destination(raw: &str, next: &str, source: &str, cell: bool) -> Option<String> {
    let html = fdl_core::html_assets::references(raw);
    if html.len() == 1 && html[0].tag == "img" && html[0].attribute == "src" && raw.trim_start().get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("<img")) {
        let value = if cell { next.replace('|', "\\|") } else { next.to_string() };
        return fdl_core::html_assets::rewrite_references::<()>(raw, |reference| Ok((reference.tag == "img" && reference.attribute == "src").then(|| value.clone()))).ok();
    }
    let mut image = raw.to_string();
    if crate::paste::markdown_image_destination_span(&image).is_none() {
        crate::paste::markdown_reference_image_alt(&image)?;
        let expanded = crate::paste::expand_reference_images_for_pack(&format!("{image}\n\n{source}"));
        image = expanded.lines().next()?.to_string();
    }
    let destination = crate::paste::markdown_image_destination_span(&image)?;
    let escaped = next.replace(' ', "%20").replace('(', "%28").replace(')', "%29");
    image.replace_range(destination, &if cell { escaped.replace('|', "\\|") } else { escaped });
    Some(image)
}

/// Change just an image destination, retaining the author's alt text and title.
fn patch_image_source(old: &Block, new: &Block, raw: &str, source: &str, cell: bool) -> Option<String> {
    let (BlockKind::Image { src: old_src, .. }, BlockKind::Image { src: new_src, .. }) = (&old.kind, &new.kind) else { return None };
    if old_src == new_src { return None; }
    let mut comparable = new.clone();
    if let BlockKind::Image { src, src_kind, .. } = &mut comparable.kind {
        *src = old_src.clone();
        if let BlockKind::Image { src_kind: old_kind, .. } = &old.kind { *src_kind = old_kind.clone(); }
    }
    if comparable != *old { return None; }
    let spans = crate::paste::markdown_image_spans_with_html(raw);
    if spans.len() != 1 { return None; }
    let span = spans.first()?;
    let mut output = raw.to_string();
    output.replace_range(span.clone(), &patch_image_destination(&raw[span.clone()], new_src, source, cell)?);
    Some(output)
}

fn same_inline_image_except_source(old: &crate::model::Inline, new: &crate::model::Inline) -> bool {
    let normalize = |inline: &crate::model::Inline| {
        let mut result = inline.clone();
        let mut image = result.image()?.clone();
        image.src_kind = "src".into(); image.src = "normalized".into();
        crate::docxml::remap_inline_image(&mut result, image).ok()?;
        Some(result)
    };
    normalize(old).zip(normalize(new)).is_some_and(|(a, b)| a == b)
}

fn patch_inline_images(old: &Block, new: &Block, raw: &str, source: &str, cell: bool) -> Option<String> {
    if !matches!(old.kind, BlockKind::Text) || old.kind != new.kind || old.align != new.align || old.bg != new.bg
        || old.folded != new.folded || !old.children.is_empty() || !new.children.is_empty() { return None; }
    let images = |text: &RichText| -> Option<Vec<crate::model::Inline>> {
        let mut images = vec![];
        for (range, marks) in text.spans() {
            let mut rest = marks.clone();
            let object = rest.inline.take();
            if rest != Marks::default() { return None; }
            if let Some(object) = object {
                object.image()?;
                for _ in text.text[range].chars() { images.push(object.clone()); }
            }
        }
        Some(images)
    };
    let old_images = images(&old.text)?;
    let new_images = images(&new.text)?;
    if old_images.is_empty() || old_images.len() != new_images.len() || new.text.text.contains(['\r', '\n'])
        || !old_images.iter().zip(&new_images).all(|(a, b)| same_inline_image_except_source(a, b)) { return None; }
    let (body, ending) = without_ending(raw);
    if body.contains(['\r', '\n']) { return None; }
    let spans = crate::paste::markdown_image_spans_with_html(body);
    if spans.len() != old_images.len() { return None; }
    let before: Vec<_> = old.text.text.split('\u{fffc}').collect();
    let after: Vec<_> = new.text.text.split('\u{fffc}').collect();
    if before.len() != spans.len() + 1 || after.len() != before.len() { return None; }
    let mut result = String::new();
    let mut at = 0;
    for i in 0..before.len() {
        let end = spans.get(i).map_or(body.len(), |span| span.start);
        let original = &body[at..end];
        if before[i] == after[i] { result.push_str(original); }
        else {
            if original != before[i] { return None; }
            let a: Vec<char> = before[i].chars().collect();
            let b: Vec<char> = after[i].chars().collect();
            let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
            let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
            let old_offsets: Vec<_> = before[i].char_indices().map(|(at, _)| at).chain(std::iter::once(before[i].len())).collect();
            let new_offsets: Vec<_> = after[i].char_indices().map(|(at, _)| at).chain(std::iter::once(after[i].len())).collect();
            result.push_str(&original[..old_offsets[prefix]]);
            for ch in after[i][new_offsets[prefix]..new_offsets[b.len() - suffix]].chars() {
                if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') || cell && ch == '|' { result.push('\\'); }
                result.push(ch);
            }
            result.push_str(&original[old_offsets[a.len() - suffix]..]);
        }
        if let Some(span) = spans.get(i) {
            let original = &body[span.clone()];
            if old_images[i] == new_images[i] { result.push_str(original); }
            else { result.push_str(&patch_image_destination(original, &new_images[i].image()?.src, source, cell)?); }
            at = span.end;
        }
    }
    result.push_str(ending);
    Some(result)
}

/// Preserve arbitrary unchanged marks when only inline image destinations changed.
fn patch_inline_image_sources(old: &Block, new: &Block, raw: &str, source: &str, cell: bool) -> Option<String> {
    let images = |text: &RichText| -> Vec<(usize, crate::model::Inline)> {
        text.spans().flat_map(|(range, marks)| {
            let inline = marks.inline.as_ref().filter(|inline| inline.image().is_some()).cloned();
            text.text[range.clone()].char_indices().filter_map(move |(offset, ch)| if ch == '\u{fffc}' { inline.clone().map(|inline| (range.start + offset, inline)) } else { None }).collect::<Vec<_>>()
        }).collect()
    };
    let before = images(&old.text);
    let after = images(&new.text);
    if before.is_empty() || before.len() != after.len() { return None; }
    let mut comparable = new.clone();
    for ((offset, original), (new_offset, updated)) in before.iter().zip(&after) {
        if offset != new_offset || !same_inline_image_except_source(original, updated) { return None; }
        comparable.text.update_marks(*offset..*offset + '\u{fffc}'.len_utf8(), |marks| marks.inline = Some(original.clone()));
    }
    if comparable != *old { return None; }
    let spans = crate::paste::markdown_image_spans_with_html(raw);
    if spans.len() != before.len() { return None; }
    let mut result = raw.to_string();
    for (span, ((_, original), (_, updated))) in spans.into_iter().zip(before.iter().zip(&after)).rev() {
        if original != updated { result.replace_range(span.clone(), &patch_image_destination(&raw[span], &updated.image()?.src, source, cell)?); }
    }
    Some(result)
}

/// Keep reference-link syntax when plain text around unchanged links changes.
fn patch_reference_links(old: &Block, new: &Block, raw: &str, cell: bool) -> Option<String> {
    if !matches!(old.kind, BlockKind::Text) || old.kind != new.kind || old.align != new.align || old.bg != new.bg
        || old.folded != new.folded || !old.children.is_empty() || !new.children.is_empty() { return None; }
    let parts = |text: &RichText| -> Option<(Vec<String>, Vec<(String, String)>)> {
        let mut plain = vec![String::new()];
        let mut links: Vec<(String, String)> = vec![];
        let mut in_link = false;
        for (range, marks) in text.spans() {
            let mut rest = marks.clone();
            let url = rest.link.take();
            if rest != Marks::default() { return None; }
            let value = &text.text[range];
            if let Some(url) = url {
                if in_link && links.last().is_some_and(|(old, _)| old == &url) { links.last_mut()?.1.push_str(value); }
                else { links.push((url, value.to_string())); plain.push(String::new()); }
                in_link = true;
            } else {
                plain.last_mut()?.push_str(value);
                in_link = false;
            }
        }
        Some((plain, links))
    };
    let (before, old_links) = parts(&old.text)?;
    let (after, new_links) = parts(&new.text)?;
    if old_links.is_empty() || old_links != new_links || before.len() != after.len() || new.text.text.contains(['\r', '\n']) { return None; }
    let (body, ending) = without_ending(raw);
    if body.contains(['\r', '\n']) { return None; }
    let spans = crate::paste::markdown_preservable_link_spans(body);
    if spans.len() != old_links.len() || spans.iter().zip(&old_links).any(|((_, label), (_, text))| label != text) { return None; }
    let mut result = String::new();
    let mut at = 0;
    for i in 0..before.len() {
        let end = spans.get(i).map_or(body.len(), |(span, _)| span.start);
        let original = &body[at..end];
        if before[i] == after[i] { result.push_str(original); }
        else {
            if original != before[i] { return None; }
            let a: Vec<char> = before[i].chars().collect();
            let b: Vec<char> = after[i].chars().collect();
            let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
            let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
            let a_offsets: Vec<_> = before[i].char_indices().map(|(at, _)| at).chain(std::iter::once(before[i].len())).collect();
            let b_offsets: Vec<_> = after[i].char_indices().map(|(at, _)| at).chain(std::iter::once(after[i].len())).collect();
            result.push_str(&original[..a_offsets[prefix]]);
            for ch in after[i][b_offsets[prefix]..b_offsets[b.len() - suffix]].chars() {
                if matches!(ch, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') || cell && ch == '|' { result.push('\\'); }
                result.push(ch);
            }
            result.push_str(&original[a_offsets[a.len() - suffix]..]);
        }
        if let Some((span, _)) = spans.get(i) { result.push_str(&body[span.clone()]); at = span.end; }
    }
    result.push_str(ending);
    Some(result)
}

/// Patch plain table cells in place, keeping the rest of each row verbatim.
/// Structural changes and cells with unsupported markup remain protected by
/// the normal lossy-save check.
fn patch_table(old: &Block, new: &Block, raw: &str, source: &str) -> Option<String> {
    let (BlockKind::Table(before), BlockKind::Table(after)) = (&old.kind, &new.kind) else { return None };
    let mut comparable = new.clone();
    comparable.kind = old.kind.clone();
    if comparable != *old || before.col_widths != after.col_widths || before.header_rows != after.header_rows
        || before.header_col != after.header_col || before.rows.len() != after.rows.len() { return None; }
    let source_lines = lines(raw);
    if source_lines.len() != before.rows.len() + 1 { return None; }
    let mut edits = vec![];
    for (r, (before_row, after_row)) in before.rows.iter().zip(&after.rows).enumerate() {
        if before_row.len() != after_row.len() { return None; }
        let range = &source_lines[if r == 0 { 0 } else { r + 1 }];
        let (line, _) = without_ending(&raw[range.clone()]);
        let start = line.len() - line.trim_start().len();
        let end = line.trim_end().len();
        let inner_start = start + usize::from(line[start..end].starts_with('|'));
        let inner_end = if line[inner_start..end].ends_with('|') && !line[..end - 1].ends_with('\\') { end - 1 } else { end };
        let mut cells = vec![];
        let mut from = inner_start;
        let mut escaped = false;
        for (at, ch) in line[inner_start..inner_end].char_indices() {
            if ch == '|' && !escaped { cells.push(from..inner_start + at); from = inner_start + at + 1; }
            escaped = ch == '\\' && !escaped;
        }
        cells.push(from..inner_end);
        if cells.len() != before_row.len() { return None; }
        for ((old_cell, new_cell), cell) in before_row.iter().zip(after_row).zip(cells) {
            if old_cell == new_cell { continue; }
            let mut comparable_cell = new_cell.clone();
            comparable_cell.blocks = old_cell.blocks.clone();
            if comparable_cell != *old_cell || old_cell.blocks.len() != 1 || new_cell.blocks.len() != 1
                || new_cell.blocks[0].text.text.contains(['\r', '\n']) { return None; }
            let part = &line[cell.clone()];
            let content = part.trim();
            let leading = part.len() - part.trim_start().len();
            let replacement = patch_plain(&old_cell.blocks[0], &new_cell.blocks[0], content)
                .or_else(|| patch_image_source(&old_cell.blocks[0], &new_cell.blocks[0], content, source, true))
                .or_else(|| patch_inline_image_sources(&old_cell.blocks[0], &new_cell.blocks[0], content, source, true))
                .or_else(|| patch_inline_images(&old_cell.blocks[0], &new_cell.blocks[0], content, source, true))
                .or_else(|| patch_reference_links(&old_cell.blocks[0], &new_cell.blocks[0], content, true))
                .or_else(|| patch_rich_plain(&old_cell.blocks[0], &new_cell.blocks[0], content, true))?;
            edits.push((range.start + cell.start + leading..range.start + cell.start + leading + content.len(), replacement));
        }
    }
    if edits.is_empty() { return None; }
    let mut result = raw.to_string();
    for (range, replacement) in edits.into_iter().rev() { result.replace_range(range, &replacement); }
    Some(result)
}

fn separate(out: &mut String, previous: Option<&Block>, next: &Block, newline: &str) {
    if !ended(out) { out.push_str(newline); }
    let tight = previous.is_some_and(|p| p.kind.is_list() && next.kind.is_list()) && !matches!(next.kind, BlockKind::Ordered { seq: Some(_) });
    if !tight { out.push_str(newline); }
}

fn normalize(block: &mut Block, root: &Path, files: &[(PathBuf, String)]) {
    block.id = BlockId(0);
    if let BlockKind::Interactive(i) = &mut block.kind
        && i.kind == crate::model::InteractiveKind::Html
    {
        i.source = fdl_core::html_assets::rewrite(&i.source, |src| {
            let full = root.join(src).canonicalize().ok();
            Ok::<_, std::convert::Infallible>(files.iter().find(|(path, _)| full.as_ref().is_some_and(|p| path.canonicalize().ok().as_ref() == Some(p)))
                .map(|(_, rel)| root.join(rel).to_string_lossy().to_string()))
        }).unwrap();
    }
    if let BlockKind::Image { src_kind, src, .. } = &mut block.kind {
        *src_kind = "src".into();
        let full = root.join(src.as_str()).canonicalize().ok();
        if let Some((_, rel)) = files.iter().find(|(path, _)| full.as_ref().is_some_and(|p| path.canonicalize().ok().as_ref() == Some(p))) {
            *src = root.join(rel).to_string_lossy().to_string();
        }
        if Path::new(src).is_absolute() { *src = Path::new(src).components().collect::<PathBuf>().to_string_lossy().to_string(); }
    }
    for run in &mut block.text.runs {
        if let Some(inline) = &mut run.marks.inline
            && let Some(image) = inline.image()
        {
            let mut image = image.clone();
            image.src_kind = "src".into();
            let full = root.join(&image.src).canonicalize().ok();
            if let Some((_, rel)) = files.iter().find(|(path, _)| full.as_ref().is_some_and(|p| path.canonicalize().ok().as_ref() == Some(p))) {
                image.src = root.join(rel).to_string_lossy().to_string();
            }
            if Path::new(&image.src).is_absolute() { image.src = Path::new(&image.src).components().collect::<PathBuf>().to_string_lossy().to_string(); }
            let _ = crate::docxml::remap_inline_image(inline, image);
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
    let mut title = Block::new(BlockKind::Text, expected.title.clone());
    normalize(&mut title, root, files);
    expected.title = title.text;
    let mut title = Block::new(BlockKind::Text, actual.title.clone());
    normalize(&mut title, root, &[]);
    actual.title = title.text;
    for block in &mut expected.blocks { normalize(block, root, files); }
    for block in &mut actual.blocks { normalize(block, root, &[]); }
    expected == actual
}

impl Source {
    pub fn matches(&self, text: &str) -> bool { self.original == text }
    pub(crate) fn matches_document(&self, doc: &Document) -> bool { self.document == *doc }
}

pub fn render(path: &Path, source: &Source, doc: &Document) -> Result<Rendered, SaveFileError> {
    if doc == &source.document {
        return Ok(Rendered { text: source.original.clone(), source: source.clone(), files: vec![] });
    }
    let opts = crate::markdown::Options { library_root: path.parent(), assets: Some("assets"), dot_relative: true, ..Default::default() };
    let mut writer = crate::markdown::FragmentWriter::new(&opts);
    let mut out = if source.bom { "\u{feff}".to_string() } else { String::new() };
    let has_header = !doc.title.text.trim().is_empty() && (source.header.is_some() || doc.title.text != stem(path));
    let same_header_position = has_header == source.header.is_some();
    if has_header {
        if let Some(header) = &source.header {
            out.push_str(&header.leading);
            if header.title == doc.title { out.push_str(&header.raw); }
            else {
                let old = Block::new(BlockKind::Text, header.title.clone());
                let mut new = old.clone();
                new.text = doc.title.clone();
                if let Some(patched) = patch_inline_image_sources(&old, &new, &header.raw, &source.original, false)
                    .or_else(|| patch_rich_plain(&old, &new, &header.raw, false))
                    .or_else(|| patch_styled_title(&old.text, &new.text, &header.raw))
                    .or_else(|| {
                        let mut before = old.clone();
                        let mut after = new.clone();
                        before.kind = BlockKind::Heading { level: 1, seq_auto: false };
                        after.kind = before.kind.clone();
                        patch_marked(&before, &after, &header.raw)
                    }) {
                    out.push_str(&patched);
                } else { return Err(SaveFileError::Lossy); }
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
        let patched = old.filter(|_| !unchanged).and_then(|(_, f)| patch_display_equation(&f.block, block, &f.raw).or_else(|| patch_plain(&f.block, block, &f.raw)).or_else(|| patch_image_source(&f.block, block, &f.raw, &source.original, false)).or_else(|| patch_inline_image_sources(&f.block, block, &f.raw, &source.original, false)).or_else(|| patch_inline_images(&f.block, block, &f.raw, &source.original, false)).or_else(|| patch_reference_links(&f.block, block, &f.raw, false)).or_else(|| patch_marked(&f.block, block, &f.raw)).or_else(|| patch_rich_plain(&f.block, block, &f.raw, false)).or_else(|| patch_nested_text(&f.block, block, &f.raw, path, &source.original)).or_else(|| patch_task(&f.block, block, &f.raw)).or_else(|| patch_fence(&f.block, block, &f.raw, source.newline)).or_else(|| patch_indented_code(&f.block, block, &f.raw, source.newline)).or_else(|| patch_table(&f.block, block, &f.raw, &source.original)));
        let generated = writer.block(block, !unchanged && patched.is_none());
        let raw = if let Some((_, fragment)) = old.filter(|_| unchanged) { fragment.raw.clone() }
            else if let Some(patched) = patched { patched }
            else {
                if old.is_some_and(|(_, f)| f.raw.contains('<') || f.raw.contains("](") || f.raw.contains("![") || f.raw.contains("][")
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
    if let Some(header) = &mut next_source.header { header.title = doc.title.clone(); }
    Ok(Rendered { text: out, source: next_source, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_title_preserves_images_equations_and_source_on_edit() {
        let dir = std::env::temp_dir().join(format!("markdownx-title-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("shot one.svg"), "<svg/>").unwrap();
        let path = dir.join("story.md");
        let original = "# Story ![frame](<./shot%20one.svg> \"title\") $\\alpha$\r\n\r\nBody\r\n";
        let (mut doc, source) = parse(&path, original);
        assert_eq!(doc.title.plain_text(), "Story frame \\alpha");
        assert!(doc.asset_sources().iter().any(|src| src.ends_with("shot one.svg")));
        assert_eq!(render(&path, &source, &doc).unwrap().text, original);
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(&path, &source, &doc).unwrap();
        assert_eq!(saved.text, original.replace("Body", "New Body"));
        doc.title.insert_plain(0, "New ", Marks::default());
        let saved = render(&path, &saved.source, &doc).unwrap();
        assert_eq!(saved.text, original.replace("# Story", "# New Story").replace("Body", "New Body"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn display_math_survives_unrelated_edits_and_formula_updates() {
        let path = Path::new("paper.md");
        let input = "# Paper\r\n\r\nBefore\r\n\r\n$$\r\n\\frac{a}{b}\r\n$$\r\n\r\nAfter\r\n";
        let (mut doc, source) = parse(path, input);
        assert!(doc.blocks[1].text.runs[0].marks.inline.as_ref().unwrap().is_display_equation());
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert_eq!(saved.text, input.replace("Before", "New Before"));
        let id = doc.blocks[1].id;
        let mut core = crate::editor::core::Core::new(doc);
        core.set_equation_source(crate::editor::core::Pos::new(id, 0), "\\frac{x}{y}");
        let saved = render(path, &saved.source, &core.doc).unwrap();
        assert!(saved.text.contains("$$\r\n\\frac{x}{y}\r\n$$"), "{}", saved.text);
        let (roundtrip, _) = parse(path, &saved.text);
        assert!(roundtrip.blocks[1].text.runs[0].marks.inline.as_ref().unwrap().is_display_equation());
    }

    #[test]
    fn rich_plain_edits_preserve_styles_links_and_images_in_paragraphs_tables_and_lists() {
        let path = Path::new("story.md");
        let input = "# Story\r\n\r\nNote **bold** and ![frame][shot] end\r\n\r\n3。 Parent\r\n   - View **detail** and ![frame][shot] end\r\n\r\n| Shot | Frame |\r\n| --- | --- |\r\n| 1 | See **detail** and ![frame][shot] end |\r\n\r\n[shot]: ./shots/01.svg \"title\"\r\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        doc.blocks[1].children[0].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[2].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        let expected = input.replace("Note **", "New Note **").replace("- View **", "- New View **").replace("| See **", "| New See **");
        assert_eq!(saved.text, expected);
    }

    #[test]
    fn editing_text_preserves_original_image_query_syntax() {
        let dir = std::env::temp_dir().join(format!("markdownx-query-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("frame.svg"), "<svg/>").unwrap();
        let path = dir.join("story.md");
        let input = "# Story\r\n\r\nBefore ![frame](./frame.svg?v=1) after\r\n\r\n| Shot | Image |\r\n| --- | --- |\r\n| 1 | View ![frame](frame.svg?cache=2) end |\r\n";
        let (mut doc, source) = parse(&path, input);
        let assets = doc.asset_sources();
        assert_eq!(assets.len(), 2);
        assert!(assets.iter().all(|src| Path::new(src).is_file()));
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[1].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let rendered = render(&path, &source, &doc).unwrap();
        assert_eq!(rendered.text, input.replace("Before !", "New Before !").replace("| View !", "| New View !"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rich_plain_edits_refuse_ambiguous_source() {
        let path = Path::new("story.md");
        let input = "# Story\n\n3。 Parent\n   - Same **bold**\n   - Same **other**\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].children[0].text.insert_plain(0, "New ", Marks::default());
        assert!(matches!(render(path, &source, &doc), Err(SaveFileError::Lossy)));
    }

    #[test]
    fn nested_list_inline_images_preserve_markup_when_text_changes() {
        let path = Path::new("story.md");
        let input = "# Story\r\n\r\n3。 Parent\r\n   - Before ![frame](<./shot%20one.svg> \"title\") and <img src=\"other.svg\" alt=\"Detail\" width=\"40\"> after\r\n";
        let (mut doc, source) = parse(path, input);
        let text = &mut doc.blocks[0].children[0].text;
        text.insert_plain(0, "New ", Marks::default());
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, input.replace("- Before", "- New Before"));
    }

    #[test]
    fn ambiguous_nested_inline_image_edits_do_not_overwrite_source() {
        let path = Path::new("story.md");
        let input = "# Story\n\n3。 Parent\n   - Same ![frame](./shot.svg) after\n   - Same ![frame](./shot.svg) after\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].children[1].text.insert_plain(0, "New ", Marks::default());
        assert!(matches!(render(path, &source, &doc), Err(SaveFileError::Lossy)));
    }

    #[test]
    fn nested_list_text_edits_escape_markup_and_reject_ambiguous_source() {
        let path = Path::new("case.md");
        let input = "# Case\n\n3。 Parent\n   - Child\n     ![frame](./shot.svg)\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].text = RichText::plain("Parent [A]");
        doc.blocks[0].children[0].text = RichText::plain("Child*");
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, "# Case\n\n3。 Parent \\[A\\]\n   - Child\\*\n     ![frame](./shot.svg)\n");
        let ambiguous = "# Case\n\n3。 Parent\n   - Child\n     ![Child](./shot.svg)\n";
        let (mut doc, source) = parse(path, ambiguous);
        doc.blocks[0].children[0].text = RichText::plain("Changed");
        assert!(matches!(render(path, &source, &doc), Err(SaveFileError::Lossy)));
    }

    #[test]
    fn editing_nested_list_text_preserves_sibling_image_markup() {
        let path = Path::new("case.md");
        let input = "# Case\r\n\r\n3。 Parent\r\n   - Child\r\n     ![frame](<./shot%20one.svg> \"title\")\r\n\r\nAfter\r\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].children[0].text = RichText::plain("Child edited");
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, input.replace("- Child\r\n", "- Child edited\r\n"));
    }

    #[test]
    fn editing_near_chinese_lists_preserves_indentation_and_image_source() {
        let path = Path::new("case.md");
        let input = "# Case\n\n3。 Parent\n   - Child\n     ![frame](./shot.svg)\n\nAfter\n";
        let (mut doc, source) = parse(path, input);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Ordered { .. }));
        assert!(matches!(doc.blocks[0].children[0].children[0].kind, BlockKind::Image { .. }));
        doc.blocks[1].text = RichText::plain("After editing");
        assert_eq!(render(path, &source, &doc).unwrap().text, input.replace("After\n", "After editing\n"));
    }

    #[test]
    fn editing_next_to_list_media_preserves_original_markup_and_paths() {
        let path = Path::new("story.md");
        let input = "# Story\n\n- <video controls>\n  <source src=\"./clip.mp4\">\n  </video>\n\nAfter\n";
        let (mut doc, source) = parse(path, input);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Bullet));
        assert!(matches!(doc.blocks[0].children[0].kind, BlockKind::Interactive(_)));
        doc.blocks[1].text = RichText::plain("After editing");
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, input.replace("After\n", "After editing\n"));
    }

    #[test]
    fn editing_indented_code_preserves_source_and_never_reads_example_images() {
        let path = Path::new("paper.md");
        let input = "# Paper\r\n\r\n    ![literal](missing.png)\r\n\tsecond\r\n\r\nAfter\r\n";
        let (mut doc, source) = parse(path, input);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Code { .. }));
        assert_eq!(render(path, &source, &doc).unwrap().text, input);
        doc.blocks[0].text = RichText::plain("![changed](still-missing.png)\nsecond\nthird");
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, "# Paper\r\n\r\n    ![changed](still-missing.png)\r\n\tsecond\r\n    third\r\n\r\nAfter\r\n");
        let (reopened, _) = parse(path, &output.text);
        assert_eq!(reopened.blocks[0].text.text, doc.blocks[0].text.text);
    }

    #[test]
    fn editing_other_storyboard_cells_keeps_video_and_canvas_source() {
        let path = Path::new("story.md");
        let input = "# Story\n\n| Shot | Media |\n| --- | --- |\n| 01 | <video controls src=\"./clip.mp4\"></video> |\n| 02 | <canvas id=\"g\"></canvas><script>let n=0;</script> |\n";
        let (mut doc, source) = parse(path, input);
        let BlockKind::Table(table) = &mut doc.blocks[0].kind else { panic!("expected table") };
        table.rows[1][0].blocks[0].text = RichText::plain("01A");
        let output = render(path, &source, &doc).unwrap();
        assert_eq!(output.text, input.replace("| 01 |", "| 01A |"));
    }

    #[test]
    fn inserted_image_stays_in_workspace_and_saves_with_dot_relative_link() {
        let root = std::env::temp_dir().join(format!("markdownx-image-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("story.md");
        let original = "# Story\n\nNotes\n";
        std::fs::write(&path, original).unwrap();
        let mut png = Vec::new();
        image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        let block = crate::editor::image::image_block_from_in(&png, "png", Some(&root)).unwrap();
        let BlockKind::Image { src, .. } = &block.kind else { panic!("expected image") };
        let stored = PathBuf::from(src);
        assert_eq!(stored.parent(), Some(root.join("assets").as_path()));
        assert_eq!(std::fs::read(&stored).unwrap(), png);
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" height=\"10\"/></svg>";
        let vector = crate::editor::image::image_block_from_in(svg, "svg", Some(&root)).unwrap();
        let BlockKind::Image { src, .. } = vector.kind else { panic!("expected SVG image") };
        assert_eq!(std::fs::read(src).unwrap(), svg);
        assert!(crate::editor::image::image_block_from_in(b"not an SVG", "svg", Some(&root)).is_err());
        let (mut doc, source) = parse(&path, original);
        doc.blocks.push(block);
        let rendered = render(&path, &source, &doc).unwrap();
        assert!(rendered.text.contains("![](./assets/"), "{}", rendered.text);
        assert!(crate::workspace_fs::write_export_if_unchanged(&path, original.as_bytes(), rendered.text.as_bytes(), &rendered.files).unwrap().is_empty());
        let (reopened, _) = parse(&path, &rendered.text);
        assert!(reopened.blocks.iter().any(|b| matches!(&b.kind, BlockKind::Image { src, .. } if Path::new(src) == stored)));
        assert_eq!(std::fs::read(&stored).unwrap(), png);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uploaded_image_links_preserve_alt_title_and_table() {
        let path = Path::new("story.md");
        let input = "# Story\n\n![cover](<./cover.png> \"title\")\n\n| Shot | Frame |\n| --- | --- |\n| 1 | ![frame](./frame.png) |\n";
        let (mut doc, source) = parse(path, input);
        let BlockKind::Image { src, .. } = &mut doc.blocks[0].kind else { panic!("expected cover") };
        *src = "https://example.test/cover.png".into();
        let BlockKind::Table(table) = &mut doc.blocks[1].kind else { panic!("expected table") };
        let BlockKind::Image { src, .. } = &mut table.rows[1][1].blocks[0].kind else { panic!("expected frame") };
        *src = "https://example.test/frame.png".into();
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("![cover](<https://example.test/cover.png> \"title\")"));
        assert!(saved.text.contains("| 1 | ![frame](https://example.test/frame.png) |"));
    }

    #[test]
    fn storyboard_images_resolve_beside_the_markdown_and_keep_source() {
        let root = std::env::temp_dir().join(format!("markdownx-storyboard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("shots")).unwrap();
        let path = root.join("storyboard.md");
        let input = "# 分镜\n\n| 镜号 | 画面 |\n| --- | --- |\n| 01 | ![草图](./shots/01.svg) |\n| 02 | ![草图](shots/02.svg) |\n\n说明\n";
        let (mut doc, source) = parse(&path, input);
        let BlockKind::Table(table) = &doc.blocks[0].kind else { panic!("expected table") };
        for (row, name) in [(1, "01.svg"), (2, "02.svg")] {
            let BlockKind::Image { src, .. } = &table.rows[row][1].blocks[0].kind else { panic!("expected image") };
            assert_eq!(Path::new(src), root.join("shots").join(name));
        }
        let BlockKind::Table(table) = &mut doc.blocks[0].kind else { unreachable!() };
        table.rows[1][0].blocks[0].text = RichText::plain("01A");
        doc.blocks[1].text = RichText::plain("修改说明");
        let saved = render(&path, &source, &doc).unwrap();
        assert!(saved.text.contains("![草图](./shots/01.svg)"));
        assert!(saved.text.contains("![草图](shots/02.svg)"));
        assert!(saved.text.contains("| 01A | ![草图](./shots/01.svg) |"));
        assert!(saved.text.contains("修改说明"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_storyboard_images_survive_text_edits() {
        let path = Path::new("storyboard.md");
        let input = "# Story\n\nBefore ![one](./shots/one.svg) after\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Left ![two](shots/two.svg) right |\n";
        let (mut doc, source) = parse(path, input);
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[1].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("New Before ![one](./shots/one.svg) after"), "{}", saved.text);
        assert!(saved.text.contains("New Left ![two](shots/two.svg) right"), "{}", saved.text);
    }

    #[test]
    fn uploaded_inline_images_preserve_formatting_references_titles_and_html() {
        fn replace(text: &mut RichText, index: usize, url: &str) {
            let (range, mut inline) = text.spans().filter_map(|(range, marks)| marks.inline.as_ref().filter(|inline| inline.image().is_some()).map(|inline| (range, inline.clone()))).nth(index).unwrap();
            let mut image = inline.image().unwrap().clone();
            image.src = url.into(); image.src_kind = "src".into();
            crate::docxml::remap_inline_image(&mut inline, image).unwrap();
            text.update_marks(range, |marks| marks.inline = Some(inline.clone()));
        }
        let path = Path::new("storyboard.md");
        let input = "# Story\n\n**Bold ![one](./a.png \"A title\")** and ![two][ref]\n\nBefore ![three](b.png) after\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Left <img src='./c.png' alt='frame' data-note='x\\|y'> right |\n\n[ref]: ./b.png \"Reference title\"\n";
        let (mut doc, source) = parse(path, input);
        replace(&mut doc.blocks[0].text, 0, "https://example.test/one.png");
        replace(&mut doc.blocks[0].text, 1, "https://example.test/two.png");
        replace(&mut doc.blocks[1].text, 0, "https://example.test/three.png");
        doc.blocks[1].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[2].kind else { panic!("expected table") };
        replace(&mut table.rows[1][1].blocks[0].text, 0, "https://example.test/frame.png");
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("**Bold ![one](https://example.test/one.png \"A title\")** and ![two](https://example.test/two.png \"Reference title\")"), "{}", saved.text);
        assert!(saved.text.contains("New Before ![three](https://example.test/three.png) after"), "{}", saved.text);
        assert!(saved.text.contains("Left <img src='https://example.test/frame.png' alt='frame' data-note='x\\|y'> right"), "{}", saved.text);
        assert!(saved.text.contains("[ref]: ./b.png \"Reference title\""));
    }

    #[test]
    fn reference_images_preserve_definitions_during_edits_and_uploads() {
        let path = Path::new("storyboard.md");
        let input = "# Story\n\n![cover][shot]\n\nBefore ![detail][shot] after\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Left ![frame][shot] right |\n\n[shot]: ./shots/one.svg \"title\"\n";
        let (mut doc, source) = parse(path, input);
        assert_eq!(render(path, &source, &doc).unwrap().text, input);
        let BlockKind::Image { src, .. } = &mut doc.blocks[0].kind else { panic!("expected image") };
        *src = "https://example.test/uploaded.png".into();
        doc.blocks[1].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[2].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("![cover](https://example.test/uploaded.png \"title\")"));
        assert!(saved.text.contains("New Before ![detail][shot] after"));
        assert!(saved.text.contains("New Left ![frame][shot] right"));
        assert!(saved.text.contains("[shot]: ./shots/one.svg \"title\""));
    }

    #[test]
    fn reference_links_preserve_source_when_editing_adjacent_text() {
        let path = Path::new("paper.md");
        let input = "# Paper\n\nBefore [paper][ref] after\n\n| No | Source |\n| --- | --- |\n| 1 | See [article][ref] below |\n\n[ref]: https://example.test/paper(1) \"A title\"\n";
        let (mut doc, source) = parse(path, input);
        assert_eq!(render(path, &source, &doc).unwrap().text, input);
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[1].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("New Before [paper][ref] after"), "{}", saved.text);
        assert!(saved.text.contains("New See [article][ref] below"), "{}", saved.text);
        assert!(saved.text.contains("[ref]: https://example.test/paper(1) \"A title\""));
    }

    #[test]
    fn relative_links_preserve_direct_and_reference_source_when_editing() {
        let path = Path::new("paper.md");
        let input = "# Paper\n\nBefore [chapter](<./chapter (1).md> \"Title\") after\n\nSee [contract][legal] below\n\n[legal]: 合同.pdf\n";
        let (mut doc, source) = parse(path, input);
        assert_eq!(render(path, &source, &doc).unwrap().text, input);
        doc.blocks[0].text.insert_plain(0, "New ", Marks::default());
        doc.blocks[1].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("New Before [chapter](<./chapter (1).md> \"Title\") after"));
        assert!(saved.text.contains("New See [contract][legal] below"));
        assert!(saved.text.contains("[legal]: 合同.pdf"));
    }

    #[test]
    fn html_images_preserve_original_attributes_when_editing_and_uploading() {
        let path = Path::new("storyboard.md");
        let input = "# Story\n\n<img alt='cover'\n width='120' src='./shots/a&amp;b.svg' data-note='keep'>\n\nBefore <img src=shots/one.svg alt='one'> after\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Left <img src='./shots/two.svg' alt='two'> right |\n";
        let (mut doc, source) = parse(path, input);
        let BlockKind::Image { src, .. } = &mut doc.blocks[0].kind else { panic!("expected image") };
        *src = "https://example.test/a?x=1&y=2".into();
        doc.blocks[1].text.insert_plain(0, "New ", Marks::default());
        let BlockKind::Table(table) = &mut doc.blocks[2].kind else { panic!("expected table") };
        table.rows[1][1].blocks[0].text.insert_plain(0, "New ", Marks::default());
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("<img alt='cover'\n width='120' src='https://example.test/a?x=1&amp;y=2' data-note='keep'>"), "{}", saved.text);
        assert!(saved.text.contains("New Before <img src=shots/one.svg alt='one'> after"));
        assert!(saved.text.contains("New Left <img src='./shots/two.svg' alt='two'> right"));
    }

    #[test]
    fn editing_near_inline_css_keeps_original_relative_source() {
        let root = std::env::temp_dir().join(format!("mdx-inline-css-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("story.md");
        let input = "# Story\n\n<canvas style=\"background:url('./shot.svg')\"></canvas>\n<style>canvas { mask-image: url(shot.svg) }</style>\n\nNotes\n";
        let (mut doc, source) = parse(&path, input);
        doc.blocks.last_mut().unwrap().text = RichText::plain("Changed notes");
        let rendered = render(&path, &source, &doc).unwrap();
        assert!(rendered.text.contains("<canvas style=\"background:url('./shot.svg')\"></canvas>\n<style>canvas { mask-image: url(shot.svg) }</style>"));
        assert!(rendered.text.contains("Changed notes"));
        assert!(!rendered.text.contains("data:image/"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn editing_around_html_video_preserves_the_relative_source() {
        let root = std::env::temp_dir().join(format!("mdx-html-source-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("story.md");
        let input = "# Story\n\n```mdxb-html\n{\"source\":\"<video src='./clip.mp4' controls></video>\"}\n```\n\nNotes\n";
        let (mut doc, source) = parse(&path, input);
        let BlockKind::Interactive(widget) = &doc.blocks[0].kind else { panic!("expected widget") };
        let refs = fdl_core::html_assets::references(&widget.source);
        assert_eq!(Path::new(&refs[0].value), root.join("clip.mp4"));
        doc.blocks[1].text = RichText::plain("New notes");
        let saved = render(&path, &source, &doc).unwrap();
        assert!(saved.text.contains("src='./clip.mp4'"));
        assert!(saved.text.contains("New notes"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn editing_around_raw_html_video_keeps_original_markup() {
        let path = Path::new("story.md");
        let input = "# Story\n\n<video controls src='./clip.mp4'></video>\n\nNotes\n";
        let (mut doc, source) = parse(path, input);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Interactive(_)));
        doc.blocks[1].text = RichText::plain("New notes");
        let saved = render(path, &source, &doc).unwrap();
        assert!(saved.text.contains("<video controls src='./clip.mp4'></video>"));
        assert!(saved.text.contains("New notes"));
    }

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
        let (updated, _) = parse(path, "# **New**\n\nbody\n");
        doc.title = updated.title;
        assert_eq!(render(path, &source, &doc).unwrap().text, "# **New**\n\nbody\n");
    }
}
