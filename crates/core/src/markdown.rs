//! 下载为 → Markdown: DocxXML document model to CommonMark/GFM text.

use crate::model::*;
use std::path::PathBuf;

/// 导出 Markdown 设置.
#[derive(Default)]
pub struct Options<'a> {
    pub library_root: Option<&'a std::path::Path>,
    /// Titles of the document's child pages (what 子页面列表 shows).
    pub sub_pages: &'a [String],
    /// 所有内容: images are kept, local ones copied into this folder next to
    /// the .md file. None is 仅文本（不含图片和附件）.
    pub assets: Option<&'a str>,
}

pub struct Export {
    pub missing: usize,
    pub text: String,
    /// Local image files to copy: (source, path relative to the .md file).
    pub files: Vec<(PathBuf, String)>,
}

#[cfg(test)]
pub fn to_markdown(doc: &Document) -> String {
    export(doc, &Options { assets: Some("assets"), ..Default::default() }).text
}

pub fn export(doc: &Document, opts: &Options) -> Export {
    let mut w = Writer { opts, files: vec![], missing: 0 };
    let mut out = String::new();
    let title = doc.title.text.trim();
    if !title.is_empty() {
        out.push_str(&format!("# {}\n\n", escape(title)));
    }
    out.push_str(&w.blocks(&doc.blocks));
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('\n');
    Export { text: out, files: w.files, missing: w.missing }
}

/// Render selected top-level blocks with one shared asset map and list state.
/// Unrendered blocks still advance numbering, but don't read or copy assets.
pub struct FragmentWriter<'a> {
    writer: Writer<'a>,
    previous_list: bool,
    sequence: u32,
}

impl<'a> FragmentWriter<'a> {
    pub fn new(opts: &'a Options<'a>) -> Self {
        Self { writer: Writer { opts, files: vec![], missing: 0 }, previous_list: false, sequence: 0 }
    }

    pub fn block(&mut self, block: &Block, render: bool) -> Option<String> {
        self.sequence = match &block.kind {
            BlockKind::Ordered { seq: Some(n) } => *n,
            BlockKind::Ordered { seq: None } => if self.previous_list && self.sequence > 0 { self.sequence + 1 } else { 1 },
            _ => 0,
        };
        self.previous_list = block.kind.is_list();
        render.then(|| self.writer.block(block, self.sequence))
    }

    pub fn finish(self) -> (Vec<(PathBuf, String)>, usize) {
        (self.writer.files, self.writer.missing)
    }
}

struct Writer<'a> {
    missing: usize,
    opts: &'a Options<'a>,
    files: Vec<(PathBuf, String)>,
}

impl Writer<'_> {
    /// Blocks separated by blank lines; consecutive list items stay tight.
    fn blocks(&mut self, bs: &[Block]) -> String {
        let mut out = String::new();
        let mut prev_list = false;
        let mut seq = 0u32;
        for b in bs {
            let list = b.kind.is_list();
            match &b.kind {
                BlockKind::Ordered { seq: Some(n) } => seq = *n,
                BlockKind::Ordered { seq: None } => seq = if prev_list && seq > 0 { seq + 1 } else { 1 },
                _ => seq = 0,
            }
            let s = self.block(b, seq);
            if s.is_empty() && !list {
                continue;
            }
            if !out.is_empty() {
                out.push_str(if list && prev_list && !matches!(b.kind, BlockKind::Ordered { seq: Some(_) }) { "\n" } else { "\n\n" });
            }
            out.push_str(&s);
            prev_list = list;
        }
        out
    }

    fn block(&mut self, b: &Block, seq: u32) -> String {
        let text = inline(&b.text, &mut |f| self.file_link(f));
        let nested = |w: &mut Self, head: String| -> String {
            if b.children.is_empty() {
                return head;
            }
            let prefix = match &b.kind {
                BlockKind::Bullet | BlockKind::Todo { .. } => "  ".to_string(),
                BlockKind::Ordered { .. } => " ".repeat(seq.to_string().len() + 2),
                _ => "    ".to_string(),
            };
            format!("{head}\n{}", indent(&w.blocks(&b.children), &prefix))
        };
        match &b.kind {
            BlockKind::Text => {
                let leading = text.bytes().take_while(|&b| b == b' ').count();
                nested(self, format!("{}{}", "&#32;".repeat(leading), &text[leading..]))
            },
            BlockKind::Heading { level, .. } => nested(self, format!("{} {text}", "#".repeat((*level).clamp(1, 6) as usize))),
            BlockKind::Bullet => nested(self, format!("- {text}")),
            BlockKind::Ordered { .. } => nested(self, format!("{seq}. {text}")),
            BlockKind::Todo { done } => nested(self, format!("- [{}] {text}", if *done { "x" } else { " " })),
            BlockKind::Code { lang, .. } => {
                let lang = if lang.eq_ignore_ascii_case("plain text") { String::new() } else { lang.to_lowercase().replace(' ', "") };
                let mut run = 0;
                let mut longest = 0;
                for ch in b.text.text.chars() {
                    if ch == '`' { run += 1; longest = longest.max(run); } else { run = 0; }
                }
                let fence = "`".repeat((longest + 1).max(3));
                format!("{fence}{lang}\n{}\n{fence}", b.text.text)
            }
            BlockKind::Quote => indent(&self.blocks(&b.children), "> "),
            BlockKind::Callout { emoji, .. } => {
                let body = self.blocks(&b.children);
                indent(&format!("{emoji} {body}"), "> ")
            }
            BlockKind::Divider => "---".into(),
            BlockKind::Image { src, caption, .. } => match self.opts.assets {
                Some(dir) => {
                    let alt = caption.clone().unwrap_or_default();
                    format!("![{}]({})", escape(&alt), self.image(src, dir).replace(' ', "%20"))
                }
                None => String::new(),
            },
            BlockKind::Table(t) => table(t, &mut |f| self.file_link(f)),
            BlockKind::Grid | BlockKind::Column { .. } => self.blocks(&b.children),
            // An add-on block: Markdown has nothing for it.
            BlockKind::Countdown(_) => String::new(),
            BlockKind::Interactive(i) => {
                let payload = serde_json::json!({
                    "title": i.title,
                    "height": i.height(),
                    "source": i.source,
                });
                let longest = i.source.split('`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat((longest + 1).max(3));
                format!("{fence}mdxb-{}\n{}\n{fence}", i.kind.as_str(), payload)
            }
            BlockKind::File(f) => self.file_link(f).map(|url| format!("[{}]({url})", escape(&f.name))).unwrap_or_else(|| escape(&f.name)),
            BlockKind::SubPageList => {
                self.opts.sub_pages.iter().map(|t| format!("- {}", escape(&crate::model::display_title(t)))).collect::<Vec<_>>().join("\n")
            }
            BlockKind::Raw { xml, .. } => match crate::docxml::raw_summary(xml) {
                (text, Some(link)) if crate::paste::link_ok(&link) => format!("[{}]({})", escape(if text.is_empty() { &link } else { &text }), link.replace(' ', "%20")),
                (text, _) => escape(&text).replace('\n', "<br>"),
            },
        }
    }

    fn file_link(&mut self, file: &FileBlock) -> Option<String> {
        let dir = self.opts.assets.filter(|_| file.src_kind == "path")?;
        let link = self.image(&file.src, dir);
        (!link.is_empty()).then(|| link.replace(' ', "%20").replace(')', "%29").replace('(', "%28"))
    }

    /// The link for an image: remote ones stay remote, local files get a
    /// copy in the assets folder (same name, numbered on clashes).
    fn image(&mut self, src: &str, dir: &str) -> String {
        if src.starts_with("http://") || src.starts_with("https://") {
            return src.to_string();
        }
        let Some(path) = crate::store::export_path_in(self.opts.library_root.unwrap_or(&crate::store::Store::root_dir()), src) else { self.missing += 1; return String::new() };
        if let Some((_, rel)) = self.files.iter().find(|(p, _)| *p == path) {
            return rel.clone();
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image").to_string();
        let ext = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
        let mut rel = format!("{dir}/{stem}{ext}");
        let mut n = 1;
        while self.files.iter().any(|(_, r)| *r == rel) {
            n += 1;
            rel = format!("{dir}/{stem}-{n}{ext}");
        }
        self.files.push((path, rel.clone()));
        rel
    }
}

fn table(t: &Table, files: &mut impl FnMut(&FileBlock) -> Option<String>) -> String {
    let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let mut cell = |c: &TableCell| {
        let parts: Vec<String> = c.blocks.iter().map(|b| inline(&b.text, files)).filter(|s| !s.is_empty()).collect();
        parts.join("<br>").replace('|', "\\|")
    };
    let mut row = |r: &Vec<TableCell>| {
        let mut cells: Vec<String> = r.iter().map(&mut cell).collect();
        cells.resize(cols, String::new());
        format!("| {} |", cells.join(" | "))
    };
    let mut lines = vec![];
    let mut rows = t.rows.iter();
    match rows.next() {
        Some(r) => lines.push(row(r)),
        None => lines.push(row(&vec![])),
    }
    let markers = (0..cols).map(|col| {
        match t.rows.iter().filter_map(|row| row.get(col)).filter_map(|cell| cell.blocks.first()).next().map(|block| block.align).unwrap_or_default() {
            Align::Left => "---", Align::Center => ":---:", Align::Right => "---:",
        }
    }).collect::<Vec<_>>().join(" | ");
    lines.push(format!("| {markers} |"));
    lines.extend(rows.map(row));
    lines.join("\n")
}

fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| if l.is_empty() { prefix.trim_end().to_string() } else { format!("{prefix}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape(s: &str) -> String { escape_text(s, false) }

/// Preserve plain wiki and safe relative links from folder Markdown sources.
fn escape_text(s: &str, preserve_links: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        if preserve_links && let Some(len) = plain_link_len(rest) {
            out.push_str(&rest[..len]);
            at += len;
            continue;
        }
        let c = rest.chars().next().unwrap();
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' | '&') || rest.starts_with("&#32;") {
            out.push('\\');
        }
        out.push(c);
        at += c.len_utf8();
    }
    out
}

fn plain_link_len(s: &str) -> Option<usize> {
    if let Some(body) = s.strip_prefix("[[")
        && let Some(end) = body.find("]]")
    {
        let inner = &body[..end];
        if !inner.trim().is_empty() && !inner.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '\\')) {
            return Some(2 + end + 2);
        }
    }
    let body = s.strip_prefix('[')?;
    let end = body.find("](")?;
    let label = &body[..end];
    let target = &body[end + 2..];
    let close = target.find(')')?;
    let url = &target[..close];
    if label.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '\\'))
        || url.is_empty() || url.starts_with('/') || url.starts_with('\\') || url.contains(':')
        || url.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '\\'))
    {
        return None;
    }
    Some(1 + end + 2 + close + 1)
}

#[derive(PartialEq, Clone, Default)]
struct Md {
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    code: bool,
    link: Option<String>,
}

fn inline(t: &RichText, files: &mut impl FnMut(&FileBlock) -> Option<String>) -> String {
    // Merge runs that only differ in marks Markdown can't express (colors).
    let mut runs: Vec<(Md, String)> = vec![];
    for (r, m) in t.spans() {
        let s = match m.inline.as_ref().and_then(Inline::equation) {
            Some(src) => t.text[r].chars().map(|_| format!("${src}$")).collect(),
            None => match &m.inline {
                Some(Inline { kind: InlineKind::File(f), .. }) => t.text[r].chars().map(|_| f.name.clone()).collect(),
                Some(obj) => {
                    // An 打开超链接 button exports as a link.
                    let (text, link) = crate::docxml::raw_summary(&obj.xml);
                    let s = match link.filter(|l| matches!(obj.kind, InlineKind::Button(_)) && crate::paste::link_ok(l)) {
                        Some(l) => format!("[{text}]({l})"),
                        None => text,
                    };
                    t.text[r].chars().map(|_| s.clone()).collect()
                }
                None => t.text[r].to_string(),
            },
        };
        let file_link = m.inline.as_ref().and_then(|obj| match &obj.kind { InlineKind::File(f) => files(f), _ => None });
        let md = Md {
            bold: m.bold,
            italic: m.italic,
            strike: m.strike,
            underline: m.underline,
            code: m.code,
            link: file_link.or_else(|| m.link.clone().filter(|url| crate::paste::link_ok(url))),
        };
        match runs.last_mut() {
            Some((last, text)) if *last == md => text.push_str(&s),
            _ => runs.push((md, s)),
        }
    }
    let adjacent = |left: &(Md, String), right: &(Md, String)| {
        let styled = |m: &Md| m.bold || m.italic || m.strike;
        styled(&left.0) && styled(&right.0)
            && left.1.chars().next_back().is_some_and(|c| !c.is_whitespace())
            && right.1.chars().next().is_some_and(|c| !c.is_whitespace())
    };
    let html: Vec<bool> = (0..runs.len()).map(|i| {
        (i > 0 && adjacent(&runs[i - 1], &runs[i])) || (i + 1 < runs.len() && adjacent(&runs[i], &runs[i + 1]))
    }).collect();
    let mut out = String::new();
    for ((m, s), html) in runs.into_iter().zip(html) {
        if s.is_empty() {
            continue;
        }
        // Code spans keep their spaces; emphasis markers must hug the text.
        let body = if m.code { s.as_str() } else { s.trim() };
        let lead = if m.code { "" } else { &s[..s.len() - s.trim_start().len()] };
        let trail = if m.code { "" } else { &s[s.trim_end().len()..] };
        if body.is_empty() {
            out.push_str(&s);
            continue;
        }
        let mut x = if m.code {
            let mut run = 0;
            let mut longest = 0;
            for c in body.chars() {
                if c == '`' { run += 1; longest = longest.max(run); } else { run = 0; }
            }
            let tick = "`".repeat(longest + 1);
            let pad = body.starts_with('`') || body.ends_with('`')
                || (body.starts_with(' ') && body.ends_with(' ') && !body.trim_matches(' ').is_empty());
            if pad { format!("{tick} {body} {tick}") } else { format!("{tick}{body}{tick}") }
        } else {
            escape_text(body, m.link.is_none())
        };
        if m.underline {
            x = format!("<u>{x}</u>");
        }
        if m.strike {
            x = if html { format!("<del>{x}</del>") } else { format!("~~{x}~~") };
        }
        if m.italic {
            x = if html { format!("<em>{x}</em>") } else { format!("*{x}*") };
        }
        if m.bold {
            x = if html { format!("<strong>{x}</strong>") } else { format!("**{x}**") };
        }
        if let Some(url) = &m.link {
            x = format!("[{x}]({})", url.replace(' ', "%20").replace(')', "%29"));
        }
        out.push_str(lead);
        out.push_str(&x);
        out.push_str(trail);
    }
    out.replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_attachments_export_links_and_copies_including_tables() {
        let root = std::env::temp_dir().join(format!("markdownx-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/report (1).pdf"), b"pdf").unwrap();
        let file = FileBlock { src_kind: "path".into(), src: "assets/report (1).pdf".into(), name: "Report [1]".into(), view: FileView::Card };
        let mut text = RichText::plain("before ");
        text.push("\u{fffc}", Marks { inline: Some(crate::docxml::to_inline(InlineKind::File(file))), ..Default::default() });
        text.push(" after", Marks::default());
        let mut table = Table { col_widths: vec![], header_rows: 0, header_col: false, rows: vec![vec![TableCell::empty()]] };
        table.rows[0][0].blocks = vec![Block::new(BlockKind::Text, text.clone())];
        let doc = Document { blocks: vec![Block::new(BlockKind::Text, text), Block::new(BlockKind::Table(table), RichText::default())], ..Default::default() };
        let out = export(&doc, &Options { assets: Some("out.assets"), library_root: Some(&root), ..Default::default() });
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.missing, 0);
        assert_eq!(out.text.matches("[Report \\[1\\]](out.assets/report%20%281%29.pdf)").count(), 2);
        let plain = export(&doc, &Options { library_root: Some(&root), ..Default::default() });
        assert!(plain.files.is_empty());
        assert!(!plain.text.contains("out.assets"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordered_list_restarts_keep_their_explicit_numbers() {
        let input = "3. alpha\n4. beta\n\n8. gamma\n9. delta\n";
        let doc = Document { blocks: crate::paste::markdown_to_blocks(input, crate::paste::Off { file: true, ..Default::default() }), ..Default::default() };
        let output = export(&doc, &Options::default());
        let parsed = crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() });
        assert!(output.text.contains("4. beta\n\n8. gamma"));
        assert_eq!(doc.blocks.iter().map(|b| &b.kind).collect::<Vec<_>>(), parsed.iter().map(|b| &b.kind).collect::<Vec<_>>());
    }

    #[test]
    fn table_column_alignment_round_trips() {
        let input = "| left | center | right |\n| --- | :---: | ---: |\n| a | b | c |\n";
        let doc = Document { blocks: crate::paste::markdown_to_blocks(input, crate::paste::Off { file: true, ..Default::default() }), ..Default::default() };
        let output = export(&doc, &Options::default());
        assert!(output.text.contains("| --- | :---: | ---: |"));
        let parsed = crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() });
        let (BlockKind::Table(a), BlockKind::Table(b)) = (&doc.blocks[0].kind, &parsed[0].kind) else { panic!("expected tables") };
        for (left, right) in a.rows.iter().zip(&b.rows) {
            for (left, right) in left.iter().zip(right) { assert_eq!(left.blocks[0].align, right.blocks[0].align); }
        }
    }

    #[test]
    fn leading_spaces_remain_text_after_list_items() {
        let doc = Document { blocks: vec![
            Block::new(BlockKind::Bullet, RichText::plain("item")),
            Block::new(BlockKind::Text, RichText::plain("  indented sibling")),
            Block::new(BlockKind::Bullet, RichText::plain("next")),
            Block::new(BlockKind::Text, RichText::plain(" &#32; literally")),
        ], ..Default::default() };
        let output = export(&doc, &Options::default());
        let parsed = Document { blocks: crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() }), ..Default::default() };
        assert!(output.text.contains("&#32;&#32;indented sibling"));
        assert!(output.text.contains("\\&#32; literally"));
        assert_eq!(parsed.plain_text(), doc.plain_text());
    }

    #[test]
    fn adjacent_emphasis_runs_keep_text_and_marks() {
        let mut text = RichText::plain("plain ");
        text.push("bold", Marks { bold: true, ..Default::default() });
        text.push("<", Marks { bold: true, italic: true, ..Default::default() });
        text.push(">", Marks { bold: true, ..Default::default() });
        text.push("plain", Marks::default());
        text.push("italic", Marks { italic: true, ..Default::default() });
        text.push("both", Marks { bold: true, italic: true, ..Default::default() });
        let doc = Document { blocks: vec![Block::new(BlockKind::Text, text.clone())], ..Default::default() };
        let output = export(&doc, &Options::default());
        let parsed = crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() });
        assert_eq!(parsed[0].text, text);
    }

    #[test]
    fn inline_code_keeps_edge_spaces_and_backtick_runs() {
        let mut text = RichText::plain("prefix ");
        text.push("  code``  ", Marks { code: true, ..Default::default() });
        let cell = TableCell { blocks: vec![Block::new(BlockKind::Text, text.clone())], ..TableCell::empty() };
        let table = Table { col_widths: vec![], header_rows: 0, header_col: false, rows: vec![vec![cell]] };
        let doc = Document { blocks: vec![Block::new(BlockKind::Text, text), Block::new(BlockKind::Table(table), RichText::default())], ..Default::default() };
        let output = export(&doc, &Options::default());
        let parsed = Document { blocks: crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() }), ..Default::default() };
        assert_eq!(parsed.plain_text(), doc.plain_text());
    }

    #[test]
    fn nested_lists_keep_text_and_code_indentation() {
        let input = "- parent\n  paragraph\n  - child\n    nested text\n10. ordered\n    line\n- [x] todo\n  ```text\n  code  \n  ```\n";
        let blocks = crate::paste::markdown_to_blocks(input, crate::paste::Off { file: true, ..Default::default() });
        let doc = Document { blocks, ..Default::default() };
        let output = export(&doc, &Options::default());
        let again = Document { blocks: crate::paste::markdown_to_blocks(&output.text, crate::paste::Off { file: true, ..Default::default() }), ..Default::default() };
        assert_eq!(again.plain_text(), doc.plain_text());
    }

    #[test]
    fn preserves_plain_wiki_and_safe_relative_links() {
        let doc = Document {
            blocks: vec![Block::text("See [[Target|Label]] and [Local](../note.md) and [Bad](javascript:alert)")],
            ..Default::default()
        };
        let output = export(&doc, &Options::default());
        assert!(output.text.contains("[[Target|Label]]"));
        assert!(output.text.contains("[Local](../note.md)"));
        assert!(!output.text.contains("[Bad](javascript:alert)"));
    }

    #[test]
    fn plain_ampersand_does_not_become_an_html_entity() {
        let doc = Document { title: RichText::default(), blocks: vec![Block::text("literal &amp;")], ..Default::default() };
        let text = export(&doc, &Options::default()).text;
        assert!(text.contains(r"\&amp;"));
        let blocks = crate::paste::markdown_to_blocks(&text, crate::paste::Off { file: true, ..Default::default() });
        assert_eq!(blocks[0].text.text, "literal &amp;");
    }

    #[test]
    fn exports_common_blocks() {
        let mut bold = RichText::plain("a ");
        bold.push("bold ", Marks { bold: true, ..Default::default() });
        bold.push("link", Marks { link: Some("https://x.y".into()), ..Default::default() });
        let doc = Document {
            title: RichText::plain("标题"),
            blocks: vec![
                Block::new(BlockKind::Heading { level: 2, seq_auto: false }, RichText::plain("小节")),
                Block::new(BlockKind::Text, bold),
                Block::new(BlockKind::Ordered { seq: None }, RichText::plain("one"))
                    .with_children(vec![Block::new(BlockKind::Bullet, RichText::plain("sub"))]),
                Block::new(BlockKind::Ordered { seq: None }, RichText::plain("two")),
                Block::new(BlockKind::Todo { done: true }, RichText::plain("done")),
                Block::new(BlockKind::Code { lang: "Rust".into(), caption: None, wrap: false }, RichText::plain("fn main() {}")),
                Block::new(BlockKind::Quote, RichText::default()).with_children(vec![Block::text("q")]),
                Block::new(BlockKind::Divider, RichText::default()),
            ],
            ..Default::default()
        };
        let md = to_markdown(&doc);
        assert_eq!(
            md,
            "# 标题\n\n## 小节\n\na **bold** [link](https://x.y)\n\n1. one\n   - sub\n2. two\n- [x] done\n\n```rust\nfn main() {}\n```\n\n> q\n\n---\n"
        );
    }

    #[test]
    fn exports_tables() {
        let cell = |s: &str| TableCell { blocks: vec![Block::text(s)], ..TableCell::empty() };
        let t = Table { col_widths: vec![], header_rows: 1, header_col: false, rows: vec![vec![cell("a"), cell("b|c")], vec![cell("1")]] };
        assert_eq!(table(&t, &mut |_| None), "| a | b\\|c |\n| --- | --- |\n| 1 |  |");
    }
}
