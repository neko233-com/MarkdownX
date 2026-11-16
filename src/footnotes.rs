//! Footnotes for the built-in standalone HTML/PDF exporter. The editor keeps
//! the original Markdown; only the temporary export source is transformed.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

struct Definition { label: String, range: Range<usize>, body_start: usize }
struct Reference { label: String, range: Range<usize>, owner: Option<usize> }
struct Note { marker: String, backlinks: Vec<String> }
pub(crate) struct Prepared { pub source: String, refs: Vec<(String, String, String)>, notes: Vec<Note>, heading_refs: Vec<(Range<usize>, usize)> }

pub(crate) struct ReadingDocument {
    pub document: crate::model::Document,
    pub return_anchor: String,
    pub heading_aliases: HashMap<String, String>,
}

pub(crate) fn prepare(source: &str) -> Prepared {
    let mut defs = Vec::<Definition>::new();
    let mut refs = Vec::<Reference>::new();
    let mut headings = Vec::new();
    let mut owner = None;
    let mut links = 0usize;
    for (event, range) in Parser::new_ext(source, Options::ENABLE_FOOTNOTES | Options::ENABLE_TABLES | Options::ENABLE_MATH).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => headings.push(range),
            Event::Start(Tag::FootnoteDefinition(label)) => {
                // Restrict conversion to top-level definitions: removing an inner
                // quote/list definition would otherwise change the container.
                let line_start = source[..range.start].rfind('\n').map_or(0, |i| i + 1);
                let prefix = &source[line_start..range.start];
                let body = &source[range.clone()];
                let body_start = body.find("]:").map(|n| range.start + n + 2);
                if prefix.len() <= 3 && prefix.bytes().all(|c| c == b' ') && let Some(body_start) = body_start {
                    owner = Some(defs.len());
                    defs.push(Definition { label: label.into_string(), range, body_start });
                } else { owner = Some(usize::MAX); }
            }
            Event::End(TagEnd::FootnoteDefinition) => owner = None,
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => links += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => links = links.saturating_sub(1),
            Event::FootnoteReference(label) if links == 0 => refs.push(Reference { label: label.into_string(), range, owner }),
            _ => {}
        }
    }
    let mut by_label = HashMap::new();
    let mut duplicates = HashSet::new();
    for (i, def) in defs.iter().enumerate() {
        if by_label.insert(def.label.clone(), i).is_some() { duplicates.insert(def.label.clone()); }
    }
    // Definitions are moved after the main text. Number references in that
    // displayed order, then walk each selected note in order. Source-order
    // scanning incorrectly numbers a reference in an early definition before
    // a later reference in the main text. Grouping also avoids rescanning all
    // references once per level of nested notes.
    let mut by_owner: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (i, reference) in refs.iter().enumerate() { by_owner.entry(reference.owner).or_default().push(i); }
    let mut selected = Vec::new();
    let mut numbers = HashMap::new();
    let mut owners = vec![None];
    let mut ordered_refs = Vec::new();
    let mut at = 0;
    while at < owners.len() {
        if let Some(references) = by_owner.get(&owners[at]) {
            for &i in references {
                let reference = &refs[i];
                if duplicates.contains(&reference.label) { continue; }
                if let Some(&index) = by_label.get(&reference.label) {
                    if let std::collections::hash_map::Entry::Vacant(entry) = numbers.entry(index) {
                        selected.push(index); entry.insert(selected.len()); owners.push(Some(index));
                    }
                    ordered_refs.push(i);
                }
            }
        }
        at += 1;
    }
    if selected.is_empty() { return Prepared { source: source.to_owned(), refs: vec![], notes: vec![], heading_refs: vec![] }; }
    let prefix = loop {
        let p = format!("MDXFOOTNOTE{}X", uuid::Uuid::new_v4().simple());
        if !source.contains(&p) { break p; }
    };
    let mut notes: Vec<Note> = selected.iter().map(|i| Note { marker: format!("{prefix}N{i}X"), backlinks: Vec::new() }).collect();
    let mut replacements = Vec::<(Range<usize>, String)>::new();
    let mut rendered_refs = Vec::new();
    let mut heading_refs = Vec::new();
    for i in ordered_refs {
        let reference = &refs[i];
        let Some(&number) = by_label.get(&reference.label).and_then(|i| numbers.get(i)) else { continue };
        let heading = headings.partition_point(|heading| heading.start <= reference.range.start);
        if heading > 0 && reference.range.end <= headings[heading - 1].end {
            heading_refs.push((reference.range.clone(), number));
        }
        let marker = format!("{prefix}R{i}X");
        let id = format!("{prefix}ref{i}");
        notes[number - 1].backlinks.push(id.clone());
        rendered_refs.push((marker.clone(), format!("<sup class=\"footnote-ref\"><a id=\"{id}\" href=\"#{prefix}note{number}\">{number}</a></sup>"), source[reference.range.clone()].to_string()));
        replacements.push((reference.range.clone(), marker));
    }
    let mut output = source.to_owned();
    let mut edits: Vec<_> = replacements.iter().filter(|(range, _)| !selected.iter().any(|i| defs[*i].range.contains(&range.start))).cloned().collect();
    for &index in &selected { edits.push((defs[index].range.clone(), "\n".to_owned())); }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, value) in edits { output.replace_range(range, &value); }
    for &index in &selected {
        let def = &defs[index];
        let mut body = source[def.body_start..def.range.end].to_owned();
        let mut inner: Vec<_> = replacements.iter().filter(|(range, _)| range.start >= def.body_start && range.end <= def.range.end).collect();
        inner.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, value) in inner { body.replace_range(range.start - def.body_start..range.end - def.body_start, value); }
        let body = body.lines().enumerate().map(|(i, line)| {
            if i == 0 { line.trim_start_matches([' ', '\t']) }
            else if let Some(rest) = line.strip_prefix("    ") { rest }
            else { line.strip_prefix('\t').unwrap_or(line) }
        }).collect::<Vec<_>>().join("\n");
        output.push_str(&format!("\n\n{}\n\n{}\n\n", notes[numbers[&index] - 1].marker, body));
    }
    Prepared { source: output, refs: rendered_refs, notes, heading_refs }
}

impl Prepared {
    /// Anchors must use the original footnote syntax, not random render markers.
    pub(crate) fn export_anchors(&self, doc: &crate::model::Document) -> fdl_core::anchors::Targets {
        let Some((first, _, _)) = self.refs.first() else { return fdl_core::anchors::targets(doc) };
        let Some((prefix, _)) = first.rsplit_once('R') else { return fdl_core::anchors::targets(doc) };
        let markers: HashMap<&str, &str> = self.refs.iter().map(|(marker, _, original)| (marker.as_str(), original.as_str())).collect();
        fdl_core::anchors::targets_with_title_transform(doc, true, |text| {
            let mut output = String::with_capacity(text.len());
            let mut rest = text;
            while let Some(at) = rest.find(prefix) {
                output.push_str(&rest[..at]);
                rest = &rest[at..];
                let suffix = &rest[prefix.len()..];
                let Some(end) = suffix.find('X') else { output.push_str(rest); rest = ""; break };
                let length = prefix.len() + end + 1;
                output.push_str(markers.get(&rest[..length]).copied().unwrap_or(&rest[..length]));
                rest = &rest[length..];
            }
            output.push_str(rest);
            output
        })
    }

    /// Build a temporary desktop reading document. Its generated text is never
    /// used as the editable source or written back to the Markdown file.
    pub(crate) fn reading_document(&self, path: &std::path::Path, original: &str, original_doc: &crate::model::Document) -> Option<ReadingDocument> {
        if self.notes.is_empty() { return None; }
        let number = |html: &str| -> Option<usize> {
            html.split("</a>").next()?.rsplit('>').next()?.parse().ok()
        };
        let mut sample = self.source.clone();
        for (marker, html, _) in &self.refs { sample = sample.replace(marker, &number(html)?.to_string()); }
        let (sample_doc, _) = crate::markdown_source::parse(path, &sample);
        let targets = fdl_core::anchors::targets(&sample_doc);
        let mut used: HashSet<String> = targets.title.into_iter().chain(targets.headings.into_iter().map(|(_, name)| name)).collect();
        let original_targets = fdl_core::anchors::targets(original_doc);
        used.extend(original_targets.title.iter().cloned());
        used.extend(original_targets.headings.iter().map(|(_, name)| name.clone()));
        let mut titles = Vec::new();
        let mut anchors = Vec::new();
        for i in 0..self.notes.len() {
            let base = format!("{} {}", crate::settings::tr("脚注", "Footnote"), i + 1);
            let mut title = base.clone();
            let mut suffix = 2;
            while !used.insert(fdl_core::anchors::slug(&title)) { title = format!("{base} {suffix}"); suffix += 1; }
            anchors.push(fdl_core::anchors::slug(&title));
            titles.push(title);
        }
        let mut source = self.source.clone();
        for (marker, html, _) in &self.refs {
            let n = number(html)?;
            source = source.replace(marker, &format!("[{n}](#{})", anchors.get(n.checked_sub(1)?)?));
        }
        let back = format!("mdx-back-{}", uuid::Uuid::new_v4().simple());
        for (note, title) in self.notes.iter().zip(titles) {
            source = source.replace(&note.marker, &format!("### {title}\n\n[{}](#{back})", crate::settings::tr("返回引用", "Back to reference")));
        }
        let document = crate::markdown_source::parse(path, &source).0;
        let mut heading_aliases = HashMap::new();
        if !self.heading_refs.is_empty() {
            let mut changed = original.to_owned();
            let mut edits = self.heading_refs.clone();
            edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
            for (range, number) in edits { changed.replace_range(range, &number.to_string()); }
            let changed_doc = crate::markdown_source::parse(path, &changed).0;
            let changed_targets = fdl_core::anchors::targets(&changed_doc);
            let preview_targets = fdl_core::anchors::targets(&document);
            let existing: HashSet<_> = preview_targets.title.into_iter().chain(preview_targets.headings.into_iter().map(|(_, name)| name)).collect();
            if original_targets.headings.len() == changed_targets.headings.len() && original_targets.title.is_some() == changed_targets.title.is_some() {
                let old = original_targets.title.into_iter().chain(original_targets.headings.into_iter().map(|(_, name)| name));
                let new = changed_targets.title.into_iter().chain(changed_targets.headings.into_iter().map(|(_, name)| name));
                for (before, after) in old.zip(new) {
                    if before != after && existing.contains(&after) { heading_aliases.insert(before, after); }
                }
            }
        }
        Some(ReadingDocument { document, return_anchor: back, heading_aliases })
    }

    pub(crate) fn render(self, mut html: String) -> anyhow::Result<String> {
        // The document title is emitted both in <title> and as the visible H1.
        // Keep the browser-tab title plain while retaining the live reference in H1.
        if let Some(start) = html.find("<title>").map(|at| at + "<title>".len())
            && let Some(end) = html[start..].find("</title>").map(|at| start + at) {
            let mut title = html[start..end].to_owned();
            for (marker, replacement, _) in &self.refs {
                if title.contains(marker) {
                    let number = replacement.split("</a>").next().and_then(|part| part.rsplit('>').next())
                        .ok_or_else(|| anyhow::anyhow!("could not render a footnote in the document title"))?;
                    title = title.replace(marker, number);
                }
            }
            html.replace_range(start..end, &title);
        }
        for (marker, replacement, _) in self.refs {
            anyhow::ensure!(html.matches(&marker).count() == 1, "could not place a footnote reference in the exported document");
            html = html.replace(&marker, &replacement);
        }
        for (i, note) in self.notes.iter().enumerate() {
            let marker = format!("<div class=\"b\">{}</div>", note.marker);
            let prefix = note.marker.rsplit_once('N').map_or("", |(prefix, _)| prefix);
            let backs = note.backlinks.iter().map(|id| format!(" <a href=\"#{id}\" aria-label=\"返回引用\">↩</a>")).collect::<String>();
            let separator = if i == 0 { "<hr><h2>注释</h2>" } else { "" };
            let replacement = format!("{separator}<div class=\"b footnote\" id=\"{prefix}note{}\"><sup>{}</sup>{backs}</div>", i + 1, i + 1);
            anyhow::ensure!(html.matches(&marker).count() == 1, "could not place a footnote definition in the exported document");
            html = html.replace(&marker, &replacement);
        }
        Ok(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_literals_and_unreferenced_definitions_unchanged() {
        let source = "No note [^missing]. `[^a]` and \\[^a].\n\n```md\n[^a]\n```\n\n[^a]: unreferenced\n";
        let prepared = prepare(source);
        assert_eq!(prepared.source, source);
        assert!(prepared.notes.is_empty());
    }

    #[test]
    fn keeps_repeated_nested_and_table_references_distinct() {
        let source = "First[^a] again[^a].\n\n| A | B |\n|---|---|\n| x | [^b] |\n\n[^a]: parent[^b]\n\n[^b]: child\n";
        let prepared = prepare(source);
        assert_eq!(prepared.notes.len(), 2);
        assert_eq!(prepared.refs.len(), 4);
        assert_eq!(prepared.notes[0].backlinks.len(), 2);
        assert_eq!(prepared.notes[1].backlinks.len(), 2);
    }

    #[test]
    fn numbers_main_text_before_references_inside_early_definitions() {
        let source = "First[^a].\n\n[^a]: nested[^c] and repeat[^b]\n\nLater[^b] and again[^a].\n\n[^b]: B refers back[^a]\n\n[^c]: C\n";
        let prepared = prepare(source);
        assert_eq!(prepared.notes.len(), 3);
        assert!(prepared.notes[0].marker.ends_with("N0X"));
        assert!(prepared.notes[1].marker.ends_with("N1X"));
        assert!(prepared.notes[2].marker.ends_with("N2X"));
        let numbers: Vec<_> = prepared.refs.iter().map(|(_, html, _)| {
            html.split("</a>").next().unwrap().rsplit_once('>').unwrap().1.to_string()
        }).collect();
        assert_eq!(numbers, ["1", "2", "1", "3", "2", "1"]);
        assert_eq!(prepared.notes.iter().map(|n| n.backlinks.len()).collect::<Vec<_>>(), [3, 2, 1]);
    }

    #[test]
    fn desktop_reading_document_contains_links_and_resolves_heading_collisions() {
        let label = crate::settings::tr("脚注", "Footnote");
        let source = format!("# Paper\n\n## {label} 1\n\nText[^a] and again[^a].\n\n[^a]: **Important** note.\n");
        let path = std::path::Path::new("paper.md");
        let (original, _) = crate::markdown_source::parse(path, &source);
        let result = prepare(&source).reading_document(path, &source, &original).unwrap();
        let preview = result.document;
        let back = result.return_anchor;
        let anchor = format!("{}-1-2", fdl_core::anchors::slug(label));
        let targets = fdl_core::anchors::targets(&preview);
        assert!(targets.headings.iter().any(|(_, name)| name == &anchor));
        let mut links = vec![];
        preview.for_each_block(|block| for (_, marks) in block.text.spans() {
            if let Some(link) = &marks.link { links.push(link.clone()); }
        });
        assert_eq!(links, [format!("#{anchor}"), format!("#{anchor}"), format!("#{back}")]);
        assert!(preview.plain_text().contains("Important note."));
        assert!(!preview.plain_text().contains("MDXFOOTNOTE"));
        assert!(prepare("No notes here").reading_document(path, "No notes here", &original).is_none());
    }

    #[test]
    fn formula_contents_do_not_create_footnotes() {
        let source = "Formula $\\text{[^n]}$.\n\n$$\n\\text{[^n]}\n$$\n\n[^n]: note\n";
        let prepared = super::prepare(source);
        assert_eq!(prepared.source, source);
    }

    #[test]
    fn desktop_reading_preserves_original_heading_fragments_with_footnotes() {
        let path = std::path::Path::new("paper.md");
        let source = "# Paper\n\n[Go](#sectiona)\n\n## Section[^a]\n\n## Section1\n\n## Literal `[^a]`\n\n[^a]: Important.\n";
        let (original, _) = crate::markdown_source::parse(path, source);
        let result = prepare(source).reading_document(path, source, &original).unwrap();
        assert_eq!(result.heading_aliases.get("sectiona").map(String::as_str), Some("section1"));
        assert_eq!(result.heading_aliases.get("section1").map(String::as_str), Some("section1-1"));
        assert!(!result.heading_aliases.contains_key("literala"));
        assert!(fdl_core::anchors::targets(&result.document).headings.iter().any(|(_, name)| name == "section1"));
    }

    #[test]
    fn does_not_drop_duplicate_definitions() {
        let source = "Text[^a].\n\n[^a]: one\n\n[^a]: two\n";
        let prepared = prepare(source);
        assert_eq!(prepared.source, source);
    }
}
