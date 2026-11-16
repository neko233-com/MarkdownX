//! Clipboard import: HTML (other apps, web pages) and Markdown to DocxXML
//! blocks. Reading the pasteboard itself is `clipboard.rs`.

use crate::model::*;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

pub fn html_to_blocks(html: &str) -> Vec<Block> {
    let Ok(dom) = html5ever::parse_document(RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()) else {
        return vec![];
    };
    let root = find(&dom.document, "body").unwrap_or_else(|| dom.document.clone());
    let mut out = vec![];
    container(&root, &Ctx::default(), &mut out);
    // Markdown typed into another app's rich text still converts (inline).
    if !has_class(&dom.document, "lark-doc") {
        reparse_markdown(&mut out);
    }
    out
}

fn has_class(n: &Handle, class: &str) -> bool {
    has_own_class(n, class) || n.children.borrow().iter().any(|c| has_class(c, class))
}

fn has_own_class(n: &Handle, class: &str) -> bool {
    attr(n, "class").is_some_and(|c| c.split_whitespace().any(|c| c == class))
}

/// A callout: colours snap to the nearest swatch, the emoji id
/// (`data-emoji-id`) to its character.
fn callout(n: &Handle, ctx: &Ctx) -> Block {
    let st = style(n);
    let get = |k: &str| st.iter().find(|(p, _)| p == k).and_then(|(_, v)| parse_color(v));
    let near = |c: (u8, u8, u8, f32), names: &mut dyn Iterator<Item = &str>, f: fn(&str) -> Option<crate::palette::Rgba>| {
        names
            .filter_map(|n| Some((n, f(n)?)))
            .min_by_key(|(_, p)| {
                let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
                d(c.0, p.r) + d(c.1, p.g) + d(c.2, p.b)
            })
            .map(|(n, _)| ColorName(n.into()))
    };
    let bg = get("background-color").and_then(|c| near(c, &mut crate::palette::CALLOUT_BG_COLORS.iter().flatten().copied(), crate::palette::callout_bg));
    let border = get("border-color").and_then(|c| near(c, &mut crate::palette::HUES.iter().copied(), crate::palette::callout_border));
    let emoji = n
        .parent
        .take()
        .and_then(|w| {
            let p = w.upgrade();
            n.parent.set(Some(w));
            p
        })
        .and_then(|p| attr(&p, "data-emoji-id"))
        .and_then(|id| crate::emoji::by_id(&id))
        .unwrap_or("💡");
    let mut kids = vec![];
    container(n, ctx, &mut kids);
    if kids.is_empty() {
        kids.push(Block::text(""));
    }
    Block::new(BlockKind::Callout { emoji: emoji.into(), bg, border, text_color: None }, RichText::default()).with_children(kids)
}

/// Unmarked runs of text blocks are read as inline Markdown.
fn reparse_markdown(blocks: &mut [Block]) {
    for b in blocks {
        reparse_markdown(&mut b.children);
        if let BlockKind::Table(t) = &mut b.kind {
            for cell in t.rows.iter_mut().flatten() {
                reparse_markdown(&mut cell.blocks);
            }
        }
        if !b.kind.has_text() || matches!(b.kind, BlockKind::Code { .. }) {
            continue;
        }
        let mut out = RichText::default();
        for (r, m) in b.text.spans() {
            if *m == Marks::default() {
                out.append(&md_inline(&b.text.text[r]));
            } else {
                out.append(&b.text.slice(r));
            }
        }
        b.text = out;
    }
}

/// Content that never becomes document text.
const SKIP: &[&str] = &[
    "head", "title", "meta", "link", "style", "script", "noscript", "template", "iframe", "object", "embed", "video", "audio",
    "canvas", "svg", "math", "button", "select", "option", "textarea", "input", "map", "area", "source", "track",
];

/// Elements that break the inline flow (their content forms new paragraphs).
const BLOCK: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "main", "aside", "nav", "figure", "figcaption", "center", "address",
    "dl", "dt", "dd", "form", "fieldset", "legend", "details", "summary", "hgroup", "html", "body", "tbody", "thead", "tfoot", "tr",
    "td", "th", "caption",
];

#[derive(Clone, Default)]
struct Ctx {
    marks: Marks,
    align: Option<Align>,
}

fn tag(n: &Handle) -> Option<String> {
    match &n.data {
        NodeData::Element { name, .. } => Some(name.local.to_string().to_ascii_lowercase()),
        _ => None,
    }
}

fn attr(n: &Handle, key: &str) -> Option<String> {
    match &n.data {
        NodeData::Element { attrs, .. } => attrs.borrow().iter().find(|a| &*a.name.local == key).map(|a| a.value.to_string()),
        _ => None,
    }
}

fn find(n: &Handle, name: &str) -> Option<Handle> {
    if tag(n).as_deref() == Some(name) {
        return Some(n.clone());
    }
    n.children.borrow().iter().find_map(|c| find(c, name))
}

/// Declarations of the inline `style` attribute, lowercased.
fn style(n: &Handle) -> Vec<(String, String)> {
    attr(n, "style")
        .unwrap_or_default()
        .split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase()))
        })
        .collect()
}

fn font_px(st: &[(String, String)]) -> Option<f32> {
    let v = &st.iter().find(|(k, _)| k == "font-size")?.1;
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse().ok();
    }
    v.strip_suffix("pt").and_then(|n| n.trim().parse::<f32>().ok()).map(|pt| pt * 4. / 3.)
}

fn hidden(st: &[(String, String)]) -> bool {
    st.iter().any(|(k, v)| (k == "display" && v.starts_with("none")) || (k == "visibility" && v == "hidden"))
}

fn align_of(n: &Handle, st: &[(String, String)]) -> Option<Align> {
    let v = st.iter().find(|(k, _)| k == "text-align").map(|(_, v)| v.clone()).or_else(|| attr(n, "align"))?;
    match v.trim().to_ascii_lowercase().as_str() {
        "center" | "-webkit-center" => Some(Align::Center),
        "right" | "end" | "-webkit-right" => Some(Align::Right),
        "left" | "start" | "justify" => Some(Align::Left),
        _ => None,
    }
}

/// `ctx` with the marks an inline element and its style add.
fn inline_ctx(n: &Handle, t: &str, st: &[(String, String)], ctx: &Ctx) -> Ctx {
    let mut c = ctx.clone();
    let m = &mut c.marks;
    match t {
        "b" | "strong" => m.bold = true,
        "i" | "em" | "cite" | "dfn" | "var" => m.italic = true,
        "u" | "ins" => m.underline = true,
        "s" | "strike" | "del" => m.strike = true,
        "code" | "kbd" | "samp" | "tt" => m.code = true,
        "a" => {
            if let Some(h) = attr(n, "href").map(|h| h.trim().to_string()).filter(|h| link_ok(h)) {
                m.link = Some(h);
            }
        }
        _ => {}
    }
    for (k, v) in st {
        match k.as_str() {
            "font-weight" => {
                m.bold = match v.as_str() {
                    "bold" | "bolder" => true,
                    "normal" | "lighter" => false,
                    n => n.parse::<u32>().map_or(m.bold, |w| w >= 700),
                }
            }
            "font-style" => m.italic = v.starts_with("italic") || v.starts_with("oblique"),
            "text-decoration" | "text-decoration-line" => {
                m.underline |= v.contains("underline");
                m.strike |= v.contains("line-through");
            }
            // Only colors of the palette survive.
            "color" => {
                if let Some(n) = parse_color(v).and_then(palette_text) {
                    m.text_color = n;
                }
            }
            "background-color" | "background" => {
                if let Some(n) = parse_color(v).and_then(palette_bg) {
                    m.bg_color = n;
                }
            }
            _ => {}
        }
    }
    c
}

/// `#rgb`, `#rrggbb`, `rgb(…)` / `rgba(…)` → (r, g, b, alpha).
fn parse_color(v: &str) -> Option<(u8, u8, u8, f32)> {
    let v = v.trim().trim_end_matches("!important").trim();
    if let Some(h) = v.strip_prefix('#') {
        let x = |s: &str| u8::from_str_radix(s, 16).ok();
        return match h.len() {
            3 => {
                let d: Vec<u8> = h.chars().map(|c| x(&c.to_string()).map(|n| n * 17)).collect::<Option<_>>()?;
                Some((d[0], d[1], d[2], 1.))
            }
            6 => Some((x(&h[0..2])?, x(&h[2..4])?, x(&h[4..6])?, 1.)),
            _ => None,
        };
    }
    let inner = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
    let n = |i: usize| parts.get(i)?.trim().parse::<f32>().ok();
    Some((n(0)? as u8, n(1)? as u8, n(2)? as u8, parts.get(3).map_or(Some(1.), |_| n(3))?))
}

fn same(a: (u8, u8, u8, f32), b: (u8, u8, u8, f32)) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && (a.3 - b.3).abs() < 0.02
}

/// Some(None) is the default text color.
fn palette_text(c: (u8, u8, u8, f32)) -> Option<Option<ColorName>> {
    crate::palette::FONT_COLORS
        .iter()
        .find(|(_, v)| same(c, ((v >> 16) as u8, (v >> 8) as u8, *v as u8, 1.)))
        .map(|(n, _)| n.map(|n| ColorName(n.into())))
}

fn palette_bg(c: (u8, u8, u8, f32)) -> Option<Option<ColorName>> {
    if c.3 == 0. {
        return Some(None);
    }
    crate::palette::BG_COLORS
        .iter()
        .flatten()
        .find(|n| crate::palette::bg_color(n).is_some_and(|p| same(c, (p.r, p.g, p.b, p.a))))
        .map(|n| Some(ColorName(n.to_string())))
}

pub fn link_ok(h: &str) -> bool {
    let l = h.to_ascii_lowercase();
    ["http://", "https://", "mailto:", "ftp://"].iter().any(|p| l.starts_with(p))
}

/// A document-relative file path, checked again against the workspace on open.
pub fn relative_link_ok(h: &str) -> bool {
    split_relative_link(h).is_some()
}

/// A relative file link, optionally followed by a heading fragment; an empty
/// file part denotes a heading in the current document.
pub fn split_relative_link(h: &str) -> Option<(&str, Option<&str>)> {
    if h.is_empty() || h.len() > 8192 || h.contains('?') || h.chars().any(char::is_control) { return None; }
    let (path, fragment) = match h.split_once('#') {
        Some((path, fragment)) if !fragment.is_empty() && !fragment.contains('#') => (path, Some(fragment)),
        Some(_) => return None,
        None => (h, None),
    };
    if path.is_empty() { return fragment.map(|fragment| (path, Some(fragment))); }
    relative_path_ok(path).then_some((path, fragment))
}

/// The decoded path may contain percent-encoded `?` or `#` as filename characters.
pub fn relative_path_ok(h: &str) -> bool {
    !h.is_empty() && h.len() <= 8192 && !h.starts_with('/') && !h.starts_with('\\')
        && !h.contains(['\\', ':']) && !h.chars().any(char::is_control)
}

fn container(n: &Handle, ctx: &Ctx, out: &mut Vec<Block>) {
    let mut para = RichText::default();
    for c in n.children.borrow().iter() {
        node(c, ctx, out, &mut para);
    }
    flush(&mut para, ctx, out);
}

fn node(n: &Handle, ctx: &Ctx, out: &mut Vec<Block>, para: &mut RichText) {
    match &n.data {
        NodeData::Text { contents } => push_text(para, &contents.borrow(), &ctx.marks),
        NodeData::Element { .. } => {
            let t = tag(n).unwrap_or_default();
            let st = style(n);
            if SKIP.contains(&t.as_str()) || hidden(&st) {
                return;
            }
            let mut bctx = ctx.clone();
            if let Some(a) = align_of(n, &st) {
                bctx.align = Some(a);
            }
            match t.as_str() {
                "br" => {
                    trim_end_spaces(para);
                    para.push("\n", ctx.marks.clone());
                }
                // Superscript / subscript text is wrapped in parentheses.
                "sup" | "sub" => {
                    let c = inline_ctx(n, &t, &st, ctx);
                    para.push("(", c.marks.clone());
                    for ch in n.children.borrow().iter() {
                        node(ch, &c, out, para);
                    }
                    para.push(")", c.marks.clone());
                }
                "img" => {
                    flush(para, ctx, out);
                    out.extend(image(n));
                }
                "hr" => {
                    flush(para, ctx, out);
                    out.push(Block::new(BlockKind::Divider, RichText::default()));
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    flush(para, ctx, out);
                    let level = t[1..].parse().unwrap_or(1);
                    let mut kids = vec![];
                    container(n, &bctx, &mut kids);
                    let mut text = RichText::default();
                    for (i, k) in kids.iter().filter(|k| k.kind.has_text()).enumerate() {
                        if i > 0 {
                            text.push("\n", Marks::default());
                        }
                        text.append(&k.text);
                    }
                    if !text.is_empty() {
                        let mut b = Block::new(BlockKind::Heading { level, seq_auto: false }, text);
                        b.align = bctx.align.unwrap_or_default();
                        out.push(b);
                    }
                    out.extend(kids.into_iter().filter(|k| !k.kind.has_text()));
                }
                "ul" | "ol" | "menu" => {
                    flush(para, ctx, out);
                    list(n, &bctx, out, t == "ol");
                }
                "li" => {
                    flush(para, ctx, out);
                    out.push(item(n, &bctx, false, None));
                }
                "blockquote" => {
                    flush(para, ctx, out);
                    let mut kids = vec![];
                    container(n, &bctx, &mut kids);
                    if !kids.is_empty() {
                        out.push(Block::new(BlockKind::Quote, RichText::default()).with_children(kids));
                    }
                }
                "pre" if code_lang(n).is_some() || find(n, "code").is_some() => {
                    flush(para, ctx, out);
                    out.push(code(n));
                }
                // A `<pre>` that isn't code is text with its line breaks.
                "pre" => {
                    flush(para, ctx, out);
                    let mut text = String::new();
                    raw_text(n, &mut text);
                    for line in text.replace("\r\n", "\n").trim_end_matches('\n').split('\n') {
                        let mut b = Block::new(BlockKind::Text, RichText::default());
                        b.text.push(&line.replace('\t', " "), ctx.marks.clone());
                        b.align = bctx.align.unwrap_or_default();
                        out.push(b);
                    }
                }
                "table" => {
                    flush(para, ctx, out);
                    out.extend(table(n, &bctx));
                }
                // 高亮块: `.callout-block` carries the colours, the container
                // above it the emoji id.
                _ if has_own_class(n, "callout-block") => {
                    flush(para, ctx, out);
                    out.push(callout(n, &bctx));
                }
                _ if BLOCK.contains(&t.as_str()) => {
                    flush(para, ctx, out);
                    let before = out.len();
                    container(n, &bctx, out);
                    // Big text (a styled title) reads as a heading 1.
                    if font_px(&st).is_some_and(|px| px >= 20.) {
                        for b in &mut out[before..] {
                            if matches!(b.kind, BlockKind::Text) && b.children.is_empty() {
                                b.kind = BlockKind::Heading { level: 1, seq_auto: false };
                            }
                        }
                    }
                    // An empty paragraph is a blank line.
                    if out.len() == before && t == "p" {
                        let mut b = Block::text("");
                        b.align = bctx.align.unwrap_or_default();
                        out.push(b);
                    }
                }
                _ => {
                    let c = inline_ctx(n, &t, &st, ctx);
                    for ch in n.children.borrow().iter() {
                        node(ch, &c, out, para);
                    }
                }
            }
        }
        _ => {
            for ch in n.children.borrow().iter() {
                node(ch, ctx, out, para);
            }
        }
    }
}

/// HTML whitespace collapsing: runs become one space, none at line start.
fn push_text(para: &mut RichText, s: &str, marks: &Marks) {
    let mut t = String::with_capacity(s.len());
    let mut space = para.text.is_empty() || para.text.ends_with([' ', '\n']);
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !space {
                t.push(' ');
                space = true;
            }
        } else {
            t.push(if c == '\u{a0}' { ' ' } else { c });
            space = false;
        }
    }
    if !t.is_empty() {
        para.push(&t, marks.clone());
    }
}

fn trim_end_spaces(t: &mut RichText) {
    let end = t.text.trim_end_matches(' ').len();
    if end < t.text.len() {
        *t = t.slice(0..end);
    }
}

fn flush(para: &mut RichText, ctx: &Ctx, out: &mut Vec<Block>) {
    let t = std::mem::take(para);
    let start = t.text.len() - t.text.trim_start_matches([' ', '\n']).len();
    let end = t.text.trim_end_matches([' ', '\n']).len();
    if end <= start {
        return;
    }
    // A `<br>` starts a new block.
    let t = t.slice(start..end);
    let mut at = 0;
    for line in t.text.split('\n') {
        let mut b = Block::new(BlockKind::Text, t.slice(at..at + line.len()));
        b.align = ctx.align.unwrap_or_default();
        out.push(b);
        at += line.len() + 1;
    }
}

fn list(n: &Handle, ctx: &Ctx, out: &mut Vec<Block>, ordered: bool) {
    let start: Option<u32> = attr(n, "start").and_then(|s| s.trim().parse().ok()).filter(|&s| s != 1);
    let mut items: Vec<Block> = vec![];
    for c in n.children.borrow().iter() {
        match tag(c).as_deref() {
            Some("li") => {
                let seq = if items.is_empty() { start } else { None };
                items.push(item(c, ctx, ordered, seq));
            }
            // A list nested straight in a list belongs to the item before it.
            Some(t @ ("ul" | "ol")) => {
                let mut kids = vec![];
                list(c, ctx, &mut kids, t == "ol");
                match items.last_mut() {
                    Some(last) => last.children.extend(kids),
                    None => items.extend(kids),
                }
            }
            Some(_) => {
                let mut kids = vec![];
                container(c, ctx, &mut kids);
                items.extend(kids);
            }
            None => {}
        }
    }
    out.extend(items);
}

/// One `<li>`: its first paragraph is the item's text, the rest children.
fn item(li: &Handle, ctx: &Ctx, ordered: bool, seq: Option<u32>) -> Block {
    let kind = match checkbox(li) {
        Some(done) => BlockKind::Todo { done },
        None if ordered => BlockKind::Ordered { seq: attr(li, "value").and_then(|v| v.trim().parse().ok()).or(seq) },
        None => BlockKind::Bullet,
    };
    let mut kids = vec![];
    container(li, ctx, &mut kids);
    let text = match kids.first() {
        Some(b) if matches!(b.kind, BlockKind::Text) && b.children.is_empty() => kids.remove(0).text,
        _ => RichText::default(),
    };
    Block::new(kind, text).with_children(kids)
}

/// `Some(checked)` when the item carries a checkbox of its own (GFM task lists).
fn checkbox(li: &Handle) -> Option<bool> {
    // `<li data-list="done">` (checked) / `"check"` (inferred, unchecked).
    match attr(li, "data-list").as_deref() {
        Some("done") => return Some(true),
        Some("check" | "undone" | "todo") => return Some(false),
        _ => {}
    }
    fn walk(n: &Handle) -> Option<bool> {
        for c in n.children.borrow().iter() {
            match tag(c).as_deref() {
                Some("ul" | "ol") => {}
                Some("input") if attr(c, "type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) => {
                    return Some(attr(c, "checked").is_some());
                }
                Some(_) => {
                    if let Some(v) = walk(c) {
                        return Some(v);
                    }
                }
                None => {}
            }
        }
        None
    }
    walk(li)
}

fn raw_text(n: &Handle, out: &mut String) {
    match &n.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow().replace('\u{a0}', " ")),
        NodeData::Element { .. } => match tag(n).as_deref() {
            Some("br") => out.push('\n'),
            Some(t) if SKIP.contains(&t) => {}
            Some(t) => {
                let block = matches!(t, "div" | "p" | "li" | "tr");
                if block && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                for c in n.children.borrow().iter() {
                    raw_text(c, out);
                }
            }
            None => {}
        },
        _ => {
            for c in n.children.borrow().iter() {
                raw_text(c, out);
            }
        }
    }
}

fn code(pre: &Handle) -> Block {
    let mut text = String::new();
    raw_text(pre, &mut text);
    let text = text.replace("\r\n", "\n");
    let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
    let lang = code_lang(pre).map_or(crate::lang::from_token(""), |l| crate::lang::resolve(&l).to_string());
    Block::new(BlockKind::Code { lang, caption: None, wrap: false }, RichText::plain(text))
}

/// Language from `language-x` / `lang-x` / `highlight-source-x` classes or a
/// `lang` attribute on the `<pre>`, its `<code>` or a wrapping element.
fn code_lang(pre: &Handle) -> Option<String> {
    let mut nodes = vec![pre.clone()];
    nodes.extend(pre.children.borrow().iter().filter(|c| tag(c).as_deref() == Some("code")).cloned());
    if let Some(p) = pre.parent.take() {
        if let Some(p) = p.upgrade() {
            nodes.push(p);
        }
        pre.parent.set(Some(p));
    }
    for n in &nodes {
        for key in ["data-lark-language", "data-lang", "data-language"] {
            if let Some(v) = attr(n, key).filter(|v| !v.trim().is_empty()) {
                return Some(v);
            }
        }
        let classes = attr(n, "class").unwrap_or_default();
        let classes: Vec<&str> = classes.split_whitespace().collect();
        if classes.contains(&"hljs")
            && let Some(l) = classes.iter().find(|c| **c != "hljs" && !c.starts_with("hljs-"))
        {
            return Some(l.to_string());
        }
        for c in classes {
            for p in ["language-", "lang-", "highlight-source-", "data-lark-language-"] {
                if let Some(l) = c.strip_prefix(p).filter(|l| !l.is_empty()) {
                    return Some(l.to_string());
                }
            }
        }
    }
    None
}

fn image(n: &Handle) -> Option<Block> {
    let src = attr(n, "src")?.trim().to_string();
    let l = src.to_ascii_lowercase();
    let src = if l.starts_with("http://") || l.starts_with("https://") || l.starts_with("data:image/") {
        src
    } else {
        let p = src.strip_prefix("file://")?;
        percent_decode(p)
    };
    let mut b = Block::new(
        BlockKind::Image { src_kind: "src".into(), src, width: None, height: None, caption: None, crop: None },
        RichText::default(),
    );
    b.align = Align::Center;
    Some(b)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && s.is_char_boundary(i + 3)
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn width_px(n: &Handle) -> Option<f32> {
    let st = style(n);
    let v = st.iter().find(|(k, _)| k == "width").map(|(_, v)| v.clone()).or_else(|| attr(n, "width"))?;
    let v = v.trim();
    v.strip_suffix("px").unwrap_or(v).trim().parse::<f32>().ok().filter(|w| *w > 0.)
}

/// Column widths from `<col>`s, else the first row's single-column cells.
fn col_widths(table: &Handle, trs: &[(Handle, bool)], ncols: usize) -> Vec<f32> {
    let mut w: Vec<Option<f32>> = vec![None; ncols];
    let mut cols = vec![];
    for c in table.children.borrow().iter() {
        match tag(c).as_deref() {
            Some("colgroup") => cols.extend(c.children.borrow().iter().filter(|x| tag(x).as_deref() == Some("col")).cloned()),
            Some("col") => cols.push(c.clone()),
            _ => {}
        }
    }
    let mut i = 0;
    for col in &cols {
        let span: usize = attr(col, "span").and_then(|s| s.parse().ok()).unwrap_or(1);
        for _ in 0..span {
            if let Some(x) = w.get_mut(i) {
                *x = width_px(col);
            }
            i += 1;
        }
    }
    if let Some((tr, _)) = trs.first() {
        let mut c = 0;
        for td in tr.children.borrow().iter().filter(|x| matches!(tag(x).as_deref(), Some("td" | "th"))) {
            let span: usize = attr(td, "colspan").and_then(|s| s.trim().parse().ok()).unwrap_or(1).max(1);
            if span == 1
                && let Some(x) = w.get_mut(c)
                && x.is_none()
            {
                *x = width_px(td);
            }
            c += span;
        }
    }
    w.into_iter().map(|x| x.map_or(Table::DEFAULT_WIDTH, |x| x.round().max(Table::MIN_WIDTH))).collect()
}

fn table(n: &Handle, ctx: &Ctx) -> Option<Block> {
    fn rows_of(n: &Handle, out: &mut Vec<(Handle, bool)>, head: bool) {
        for c in n.children.borrow().iter() {
            match tag(c).as_deref() {
                Some("thead") => rows_of(c, out, true),
                Some("tbody" | "tfoot") => rows_of(c, out, head),
                Some("tr") => out.push((c.clone(), head)),
                _ => {}
            }
        }
    }
    let mut trs = vec![];
    rows_of(n, &mut trs, false);
    let mut rows: Vec<Vec<TableCell>> = vec![];
    let mut taken: Vec<Vec<bool>> = vec![];
    let mut all_th = vec![];
    for (r, (tr, _)) in trs.iter().enumerate() {
        if rows.len() <= r {
            rows.resize_with(r + 1, Vec::new);
            taken.resize_with(r + 1, Vec::new);
        }
        let mut c = 0;
        let mut th = true;
        for td in tr.children.borrow().iter() {
            let t = tag(td).unwrap_or_default();
            if t != "td" && t != "th" {
                continue;
            }
            th &= t == "th";
            while taken[r].get(c).copied().unwrap_or(false) {
                c += 1;
            }
            let colspan: usize = attr(td, "colspan").and_then(|v| v.trim().parse().ok()).unwrap_or(1).clamp(1, 64);
            let rowspan: usize = attr(td, "rowspan").and_then(|v| v.trim().parse().ok()).unwrap_or(1).clamp(1, trs.len() - r);
            for rr in r..r + rowspan {
                if rows.len() <= rr {
                    rows.resize_with(rr + 1, Vec::new);
                    taken.resize_with(rr + 1, Vec::new);
                }
                if taken[rr].len() < c + colspan {
                    taken[rr].resize(c + colspan, false);
                }
                if rows[rr].len() < c + colspan {
                    rows[rr].resize_with(c + colspan, TableCell::covered);
                }
                for cc in c..c + colspan {
                    taken[rr][cc] = true;
                }
            }
            let mut blocks = vec![];
            container(td, &Ctx { align: align_of(td, &style(td)).or(ctx.align), ..Ctx::default() }, &mut blocks);
            // Cells hold no nested tables.
            blocks.retain(|b| !matches!(b.kind, BlockKind::Table(_)));
            if blocks.is_empty() {
                blocks.push(Block::text(""));
            }
            rows[r][c] = TableCell { blocks, colspan: colspan as u32, rowspan: rowspan as u32, ..TableCell::empty() };
            c += colspan;
        }
        all_th.push(th && c > 0);
    }
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return None;
    }
    for (r, row) in rows.iter_mut().enumerate() {
        row.resize_with(ncols, TableCell::covered);
        for (c, cell) in row.iter_mut().enumerate() {
            if cell.is_covered() && !taken[r].get(c).copied().unwrap_or(false) {
                *cell = TableCell::empty();
            }
        }
    }
    let head = trs.iter().take_while(|(_, h)| *h).count();
    let header_rows = if head > 0 { head } else { usize::from(all_th.first().copied().unwrap_or(false)) };
    let mut t = Table { col_widths: col_widths(n, &trs, ncols), header_rows, header_col: false, rows };
    t.repair();
    Some(Block::new(BlockKind::Table(t), RichText::default()))
}

// ---- Markdown (plain-text paste, behavior-spec §2.5)

/// Parses Markdown into blocks with the same rules as the live shortcuts plus
/// fences, nesting by indentation, pipe tables, images and links.
pub fn markdown_to_blocks(text: &str, off: Off) -> Vec<Block> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = if off.file { expand_reference_images(&text) } else { text };
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    md_blocks(&lines, off)
}

/// Whole-file blocks and the zero-based source line ranges they consumed.
/// Blank separators are left between ranges so callers can preserve them.
pub fn markdown_file_with_spans(text: &str) -> (Vec<Block>, Vec<std::ops::Range<usize>>) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = expand_reference_images(&text);
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let mut spans = vec![];
    let blocks = md_blocks_spanned(&lines, Off { file: true, ..Default::default() }, &mut spans);
    (blocks, spans)
}

/// Resolve image references without changing physical line numbers. This is a
/// parsing view only; the workspace source retains the original definitions.
pub fn expand_reference_images(text: &str) -> String {
    expand_reference_images_with_definitions(text, false)
}

/// Packaging expands image uses but keeps definitions for other Markdown links.
pub fn expand_reference_images_for_pack(text: &str) -> String {
    expand_reference_images_with_definitions(text, true)
}

fn expand_reference_images_with_definitions(text: &str, keep_definitions: bool) -> String {
    use std::collections::{HashMap, HashSet};
    if !text.contains("]:") { return text.to_string(); }
    let lines: Vec<_> = text.split('\n').collect();
    let mut position = 0;
    let offsets: Vec<_> = lines.iter().map(|line| { let start = position; position += line.len() + 1; start }).collect();
    let math_ranges: Vec<_> = pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::ENABLE_MATH | pulldown_cmark::Options::ENABLE_TABLES | pulldown_cmark::Options::ENABLE_FOOTNOTES)
        .into_offset_iter().filter_map(|(event, range)| matches!(event, pulldown_cmark::Event::InlineMath(_) | pulldown_cmark::Event::DisplayMath(_)).then_some(range)).collect();
    let math_end = |line: usize, at: usize| {
        let position = offsets[line] + at;
        math_ranges.get(math_ranges.partition_point(|range| range.end <= position)).filter(|range| range.contains(&position))
            .map(|range| range.end.saturating_sub(offsets[line]).min(lines[line].len()))
    };
    let mut spans = vec![];
    let blocks = md_blocks_spanned(&lines, Off { file: true, ..Default::default() }, &mut spans);
    let mut protected = vec![false; lines.len()];
    for (block, span) in blocks.iter().zip(spans) {
        if matches!(block.kind, BlockKind::Code { .. } | BlockKind::Interactive(_))
            || block.text.runs.iter().any(|run| run.marks.inline.as_ref().is_some_and(Inline::is_display_equation))
        {
            for i in span { if let Some(value) = protected.get_mut(i) { *value = true; } }
        }
    }
    let mut definitions = HashMap::new();
    let mut def_lines = vec![None; lines.len()];
    let mut comment = false;
    let mut raw_tag: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        if protected[i] { continue; }
        if !comment && raw_tag.is_none() && math_end(i, line.len() - line.trim_start().len()).is_none()
            && let Some((label, destination)) = image_reference_definition(line) {
            let key = reference_key(label);
            if !key.is_empty() {
                definitions.entry(key.clone()).or_insert_with(|| destination.to_string());
                def_lines[i] = Some(key);
            }
        }
        let mut at = 0;
        while at < line.len() {
            if let Some(end) = math_end(i, at) { at = end; continue; }
            let rest = &line[at..];
            if comment {
                let end = rest.find("-->").map_or(rest.len(), |n| n + 3);
                at += end;
                if rest[..end].ends_with("-->") { comment = false; }
                continue;
            }
            if let Some(tag) = raw_tag {
                let close = format!("</{tag}>");
                let end = rest.to_ascii_lowercase().find(&close).map_or(rest.len(), |n| n + close.len());
                at += end;
                if rest[..end].to_ascii_lowercase().ends_with(&close) { raw_tag = None; }
                continue;
            }
            if rest.starts_with("<!--") { comment = true; continue; }
            if rest.starts_with('<') {
                if let Some(tag) = ["script", "style", "textarea", "title"].into_iter().find(|tag| {
                    rest.to_ascii_lowercase().strip_prefix(&format!("<{tag}")).is_some_and(|s| s.starts_with('>') || s.starts_with(char::is_whitespace))
                }) { raw_tag = Some(tag); continue; }
                if let Some(end) = inert_html_tag_end(rest) { at += end; continue; }
            }
            let c = rest.chars().next().unwrap();
            if c == '\\' { at += 1 + rest[1..].chars().next().filter(char::is_ascii_punctuation).map_or(0, char::len_utf8); continue; }
            if c == '`' {
                let n = rest.bytes().take_while(|b| *b == b'`').count();
                at += n + find_run(&rest[n..], &rest[..n], b'`', false).map_or(0, |end| end + n);
                continue;
            }
            if c == '$' && let Some((_, end, _)) = markdown_math_at(rest, line[..at].chars().next_back()) { at += end; continue; }
            at += c.len_utf8();
        }
    }
    if definitions.is_empty() { return text.to_string(); }
    let mut output = Vec::with_capacity(lines.len());
    let mut unresolved_links = HashSet::new();
    comment = false;
    raw_tag = None;
    for (i, line) in lines.iter().enumerate() {
        if protected[i] || def_lines[i].is_some() { output.push((*line).to_string()); continue; }
        let mut out = String::new();
        let mut at = 0;
        while at < line.len() {
            if let Some(end) = math_end(i, at) { out.push_str(&line[at..end]); at = end; continue; }
            let rest = &line[at..];
            if comment {
                let end = rest.find("-->").map_or(rest.len(), |n| n + 3);
                out.push_str(&rest[..end]); at += end;
                if rest[..end].ends_with("-->") { comment = false; }
                continue;
            }
            if let Some(tag) = raw_tag {
                let close = format!("</{tag}>");
                let end = rest.to_ascii_lowercase().find(&close).map_or(rest.len(), |n| n + close.len());
                out.push_str(&rest[..end]); at += end;
                if rest[..end].to_ascii_lowercase().ends_with(&close) { raw_tag = None; }
                continue;
            }
            if rest.starts_with("<!--") { comment = true; continue; }
            if rest.starts_with('<') {
                if let Some(tag) = ["script", "style", "textarea", "title"].into_iter().find(|tag| {
                    rest.to_ascii_lowercase().strip_prefix(&format!("<{tag}")).is_some_and(|s| s.starts_with('>') || s.starts_with(char::is_whitespace))
                }) { raw_tag = Some(tag); continue; }
                if let Some(end) = inert_html_tag_end(rest) { out.push_str(&rest[..end]); at += end; continue; }
            }
            let c = rest.chars().next().unwrap();
            if c == '\\' {
                let end = 1 + rest[1..].chars().next().filter(char::is_ascii_punctuation).map_or(0, char::len_utf8);
                out.push_str(&rest[..end]); at += end; continue;
            }
            if c == '`' {
                let n = rest.bytes().take_while(|b| *b == b'`').count();
                let end = n + find_run(&rest[n..], &rest[..n], b'`', false).map_or(0, |end| end + n);
                out.push_str(&rest[..end]); at += end; continue;
            }
            if c == '$' && let Some((_, end, _)) = markdown_math_at(rest, line[..at].chars().next_back()) {
                out.push_str(&rest[..end]); at += end; continue;
            }
            if c == '!' {
                if let Some((_, _, end)) = image_at(rest) { out.push_str(&rest[..end]); at += end; continue; }
                if let Some((alt, label, end)) = image_reference_at(rest) {
                    let key = reference_key(label);
                    if let Some(destination) = definitions.get(&key) {
                        out.push_str("!["); out.push_str(alt); out.push_str("]("); out.push_str(&reference_table_safe(destination)); out.push(')');
                        at += end; continue;
                    }
                }
            }
            if c == '[' && let Some((label, target, end)) = reference_at(rest) {
                let key = reference_key(target);
                if let Some(destination) = definitions.get(&key) {
                    let probe = format!("![]({destination})");
                    if let Some((url, _, _)) = image_at(&probe) {
                        let url = unescape_image(url);
                        if link_ok(&url) || relative_link_ok(&url) {
                            let safe = url.replace('(', "%28").replace(')', "%29").replace(' ', "%20");
                            out.push('['); out.push_str(label); out.push_str("]("); out.push_str(&reference_table_safe(&safe)); out.push(')');
                            at += end; continue;
                        }
                    }
                    unresolved_links.insert(key);
                }
            }
            out.push(c); at += c.len_utf8();
        }
        output.push(out);
    }
    if !keep_definitions { for (i, key) in def_lines.iter().enumerate() { if key.as_ref().is_some_and(|key| !unresolved_links.contains(key)) { output[i].clear(); } } }
    output.join("\n")
}

fn reference_key(label: &str) -> String { unescape_image(label).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase() }

fn reference_table_safe(destination: &str) -> String {
    let mut out = String::with_capacity(destination.len());
    let mut slashes = 0;
    for ch in destination.chars() {
        if ch == '|' && slashes % 2 == 0 { out.push('\\'); }
        out.push(ch);
        slashes = if ch == '\\' { slashes + 1 } else { 0 };
    }
    out
}

fn image_reference_definition(line: &str) -> Option<(&str, &str)> {
    let leading = line.len() - line.trim_start_matches(' ').len();
    if leading > 3 { return None; }
    let s = line[leading..].strip_prefix('[')?;
    let end = find_unescaped(s, "]:")?;
    let label = &s[..end];
    if label.is_empty() || label.starts_with('^') || label.len() > 999 || label.contains('[') { return None; }
    let dest = s[end + 2..].trim();
    let example = format!("![]({dest})");
    let (_, _, used) = image_at(&example)?;
    (used == example.len()).then_some((label, dest))
}

pub fn is_markdown_reference_definition(line: &str) -> bool { image_reference_definition(line.trim_end_matches(['\r', '\n'])).is_some() }

fn image_reference_at(s: &str) -> Option<(&str, &str, usize)> {
    let (alt, label, used) = reference_at(s.strip_prefix('!')?)?;
    Some((alt, label, used + 1))
}

fn reference_at(s: &str) -> Option<(&str, &str, usize)> {
    let bytes = s.as_bytes();
    if !s.starts_with('[') { return None; }
    let mut at = 1;
    let mut depth = 1usize;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if at + 1 < bytes.len() => at += 2,
            b'[' => { depth += 1; at += 1; }
            b']' => { depth -= 1; if depth == 0 { break; } at += 1; }
            _ => at += 1,
        }
    }
    let alt = s.get(1..at)?;
    if bytes.get(at) != Some(&b']') { return None; }
    at += 1;
    if bytes.get(at) == Some(&b'(') { return None; }
    if bytes.get(at) == Some(&b'[') {
        let start = at + 1;
        at = start;
        while at < bytes.len() && bytes[at] != b']' {
            if bytes[at] == b'\\' && at + 1 < bytes.len() { at += 2; } else { at += 1; }
        }
        let label = s.get(start..at)?;
        if bytes.get(at) != Some(&b']') { return None; }
        Some((alt, if label.is_empty() { alt } else { label }, at + 1))
    } else { Some((alt, alt, at)) }
}

pub fn markdown_reference_image_alt(s: &str) -> Option<&str> {
    let (alt, _, end) = image_reference_at(s)?;
    (end == s.len()).then_some(alt)
}

/// Unescaped labels and source ranges for preserving simple Markdown links.
pub fn markdown_preservable_link_spans(s: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = vec![];
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        let c = rest.chars().next().unwrap();
        if c == '\\' {
            at += 1 + rest[1..].chars().next().filter(char::is_ascii_punctuation).map_or(0, char::len_utf8);
        } else if c == '`' {
            let n = rest.bytes().take_while(|b| *b == b'`').count();
            at += n + find_run(&rest[n..], &rest[..n], b'`', false).map_or(0, |end| end + n);
        } else if c == '<' && let Some(end) = inert_html_region_end(rest).or_else(|| inert_html_tag_end(rest)) {
            at += end;
        } else if c == '!' && let Some(end) = image_at(rest).map(|(_, _, end)| end).or_else(|| image_reference_at(rest).map(|(_, _, end)| end)) {
            at += end;
        } else if c == '[' && let Some((label, _, end)) = link_at(rest, true) {
            out.push((at..at + end, unescape_image(label)));
            at += end;
        } else if c == '[' && let Some((label, _, end)) = reference_at(rest) {
            out.push((at..at + end, unescape_image(label)));
            at += end;
        } else { at += c.len_utf8(); }
    }
    out
}

/// Inline rules only (pasting into a non-empty block): one block per line.
pub fn markdown_inline_lines(text: &str) -> Vec<Block> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    text.trim_end_matches('\n').split('\n').map(|l| Block::new(BlockKind::Text, md_inline(&tabs(l)))).collect()
}

/// Runs of tabs become one space.
fn tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c != '\t' {
            out.push(c);
        } else if !out.ends_with('\t') {
            out.push('\t');
        }
    }
    out.replace('\t', " ")
}

/// Rules switched off by the paste target (a list item doesn't re-detect its
/// own kind, a quote no quotes, a callout no code or dividers).
#[derive(Clone, Copy, Default)]
pub struct Off {
    pub bullet: bool,
    pub ordered: bool,
    pub todo: bool,
    pub quote: bool,
    pub code_divider: bool,
    /// Importing a whole file: blank lines only separate blocks, and a line
    /// holding just `![alt](src)` is an image.
    pub file: bool,
}

fn indent_of(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4 - n % 4,
            _ => break,
        }
    }
    n
}

/// `line` without its first `n` columns of indentation.
fn dedent(line: &str, n: usize) -> &str {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        if col >= n {
            return &line[i..];
        }
        match c {
            ' ' => col += 1,
            '\t' => col += 4 - col % 4,
            _ => return &line[i..],
        }
    }
    ""
}

enum Marker {
    Bullet,
    Ordered(u32),
}

/// A list item line: (marker, content column, content).
fn list_item(line: &str) -> Option<(Marker, usize, &str)> {
    let ind = indent_of(line);
    let rest = line.trim_start_matches([' ', '\t']);
    let (marker, mlen) = if let Some(c) = rest.chars().next().filter(|c| matches!(c, '-' | '*' | '+')) {
        (Marker::Bullet, c.len_utf8())
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let tail = &rest[digits..];
        let d = tail.chars().next().filter(|c| matches!(c, '.' | ')' | '。'))?;
        (Marker::Ordered(rest[..digits].parse().ok()?), digits + d.len_utf8())
    };
    let after = &rest[mlen..];
    let first = after.chars().next().filter(|c| matches!(c, ' ' | '\t'))?;
    let marker_end = ind + rest[..mlen].chars().count();
    let first_width = if first == '\t' { 4 - marker_end % 4 } else { 1 };
    let mut columns = marker_end;
    let mut padding = 0;
    for c in after.chars().take_while(|c| matches!(c, ' ' | '\t')) {
        columns += if c == '\t' { 4 - columns % 4 } else { 1 };
        padding += c.len_utf8();
    }
    let (padding, col) = if columns - marker_end <= 4 && padding < after.len() {
        (padding, columns)
    } else { (first.len_utf8(), marker_end + first_width) };
    let content = &after[padding..];
    // `***` / `- - -` style rules are dividers, not items.
    if divider(rest) {
        return None;
    }
    Some((marker, col, content))
}

fn divider(line: &str) -> bool {
    let t: String = line.trim().chars().filter(|c| *c != ' ').collect();
    matches!(t.as_str(), "---" | "***" | "___") || (t.len() > 3 && t.len() < 6 && t.chars().all(|c| c == '-'))
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then(|| (c, n, t[n..].trim()))
}

fn todo_prefix(s: &str) -> Option<(bool, &str)> {
    for (p, done) in [("[ ] ", false), ("[] ", false), ("【】 ", false), ("[x] ", true), ("[X] ", true), ("【x】 ", true)] {
        if let Some(r) = s.strip_prefix(p) {
            return Some((done, r));
        }
    }
    None
}

/// Byte range of a list item's inline content, retaining its original marker.
pub fn markdown_list_content_span(line: &str) -> Option<std::ops::Range<usize>> {
    let (_, _, content) = list_item(line)?;
    let content = todo_prefix(content).map_or(content, |(_, rest)| rest).trim();
    let start = (content.as_ptr() as usize).checked_sub(line.as_ptr() as usize)?;
    Some(start..start + content.len())
}

/// Parse inline content using the same rules as a Markdown file import.
pub fn markdown_file_inline(s: &str) -> RichText { md_inline_mode(s, true) }

/// Locate CommonMark indented code without mistaking footnote continuations
/// or nested list content for code. The parser provides already-dedented text.
fn indented_code_blocks(lines: &[&str]) -> std::collections::HashMap<usize, (usize, String)> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    if !lines.iter().any(|line| indent_of(line) >= 4) { return Default::default(); }
    let source = lines.join("\n");
    let mut offsets = Vec::with_capacity(lines.len());
    let mut position = 0;
    for line in lines { offsets.push(position); position += line.len() + 1; }
    let mut found = std::collections::HashMap::new();
    let mut active: Option<(usize, usize, String)> = None;
    for (event, range) in Parser::new_ext(&source, Options::ENABLE_FOOTNOTES | Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => {
                let start = offsets.partition_point(|offset| *offset <= range.start).saturating_sub(1);
                let end = offsets.partition_point(|offset| *offset < range.end);
                active = Some((start, end, String::new()));
            }
            Event::Text(text) => if let Some((_, _, body)) = &mut active { body.push_str(&text); },
            Event::End(TagEnd::CodeBlock) => if let Some((start, end, mut body)) = active.take() {
                if body.ends_with('\n') { body.pop(); }
                found.insert(start, (end, body));
            },
            _ => {}
        }
    }
    found
}

fn md_blocks(lines: &[&str], off: Off) -> Vec<Block> {
    md_blocks_spanned(lines, off, &mut vec![])
}

fn md_blocks_spanned(lines: &[&str], off: Off, spans: &mut Vec<std::ops::Range<usize>>) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    let mut i = 0;
    let indented_code = if off.file && !off.code_divider { indented_code_blocks(lines) } else { Default::default() };
    // Ordered items continue the previous item's numbering.
    let mut last_list = false;
    while i < lines.len() {
        let line = lines[i];
        // Blank lines are empty paragraphs (none before the first block).
        if line.trim().is_empty() {
            if !out.is_empty() && !off.file {
                out.push(Block::text(""));
                spans.push(i..i + 1);
            }
            last_list = false;
            i += 1;
            continue;
        }
        if indent_of(line) >= 4 && let Some((end, body)) = indented_code.get(&i) {
            out.push(Block::new(BlockKind::Code { lang: String::new(), caption: None, wrap: false }, RichText::plain(body.clone())));
            spans.push(i..*end);
            i = *end;
            last_list = false;
            continue;
        }
        let ind = indent_of(line);
        let body = line.trim();
        // Fenced code (closed or running to the end).
        if let Some((fc, n, info)) = fence(line).filter(|_| !off.code_divider) {
            let mut code = vec![];
            let mut j = i + 1;
            while j < lines.len() {
                if let Some((c2, n2, rest)) = fence(lines[j])
                    && c2 == fc
                    && n2 >= n
                    && rest.is_empty()
                {
                    break;
                }
                code.push(dedent(lines[j], ind));
                j += 1;
            }
            let lang = crate::lang::resolve(info.split_whitespace().next().unwrap_or("")).to_string();
            let body = code.join("\n");
            if let Some(kind) = info.strip_prefix("mdxb-").and_then(|k| match k {
                "canvas" => Some(InteractiveKind::Canvas),
                "html" => Some(InteractiveKind::Html),
                _ => None,
            }) && let Ok(v) = serde_json::from_str::<serde_json::Value>(&body)
            {
                let source = v.get("source").and_then(|s| s.as_str()).unwrap_or_default();
                if !source.is_empty() {
                    out.push(Block::new(
                        BlockKind::Interactive(Interactive {
                            kind,
                            title: v.get("title").and_then(|s| s.as_str()).unwrap_or_default().to_string(),
                            height: v.get("height").and_then(|h| h.as_u64()).unwrap_or(360) as u32,
                            source: source.to_string(),
                        }),
                        RichText::default(),
                    ));
                    spans.push(i..(j + 1).min(lines.len()));
                    i = j + 1;
                    last_list = false;
                    continue;
                }
            }
            out.push(Block::new(BlockKind::Code { lang, caption: None, wrap: false }, RichText::plain(body)));
            spans.push(i..(j + 1).min(lines.len()));
            i = j + 1;
            last_list = false;
            continue;
        }
        // A complete display formula is a single equation object, including
        // its physical source span; unclosed delimiters remain plain text.
        if ind <= 3 && !off.code_divider && body.starts_with("$$") {
            let display = if body == "$$" {
                let mut end = i + 1;
                while end < lines.len() && lines[end].trim() != "$$" { end += 1; }
                (end < lines.len()).then(|| (lines[i + 1..end].join("\n"), end + 1))
            } else { body.strip_prefix("$$").and_then(|s| s.strip_suffix("$$")).map(|src| (src.trim().to_string(), i + 1)) };
            if let Some((src, end)) = display && !src.trim().is_empty() {
                let mut text = RichText::default();
                text.push(crate::editor::core::EQUATION, Marks { inline: Some(crate::docxml::to_display_equation(&src)), ..Marks::default() });
                out.push(Block::new(BlockKind::Text, text));
                spans.push(i..end);
                i = end;
                last_list = false;
                continue;
            }
        }
        if let Some((marker, col, content)) = list_item(line).filter(|(m, _, c)| {
            let todo = todo_prefix(c).is_some();
            !(todo && off.todo || !todo && matches!(m, Marker::Bullet) && off.bullet || !todo && matches!(m, Marker::Ordered(_)) && off.ordered)
        }) {
            // The item's own lines: anything indented past its marker.
            let mut j = i + 1;
            let mut sub: Vec<&str> = vec![];
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    // Preserve blank lines in nested HTML and code, while a
                    // following sibling still ends this item.
                    let mut next = j + 1;
                    while next < lines.len() && lines[next].trim().is_empty() { next += 1; }
                    if lines.get(next).is_some_and(|n| indent_of(n) > ind) {
                        sub.extend(std::iter::repeat_n("", next - j));
                        j = next;
                        continue;
                    }
                    break;
                }
                if indent_of(l) <= ind {
                    break;
                }
                sub.push(dedent(l, col.min(indent_of(l))));
                j += 1;
            }
            let (kind, text) = match todo_prefix(content) {
                Some((done, rest)) => (BlockKind::Todo { done }, rest),
                None => match marker {
                    Marker::Bullet => (BlockKind::Bullet, content),
                    Marker::Ordered(n) => {
                        let cont = last_list && matches!(out.last().map(|b| &b.kind), Some(BlockKind::Ordered { .. }));
                        (BlockKind::Ordered { seq: (!cont && n != 1).then_some(n) }, content)
                    }
                },
            };
            let mut content = vec![text];
            content.extend(sub.iter().copied());
            if off.file && indent_of(text) >= 4 {
                out.push(Block::new(kind, RichText::default()).with_children(md_blocks(&content, off)));
            } else if let Some((source, used)) = off.file.then(|| html_media_block(&content)).flatten() {
                let mut children = vec![Block::new(BlockKind::Interactive(Interactive::html("", source)), RichText::default())];
                children.extend(md_blocks(&content[used..], off));
                out.push(Block::new(kind, RichText::default()).with_children(children));
            } else {
                out.push(Block::new(kind, md_inline_mode(text.trim(), off.file)).with_children(md_blocks(&sub, off)));
            }
            spans.push(i..j);
            last_list = true;
            i = j;
            continue;
        }
        last_list = false;
        if let Some((done, rest)) = todo_prefix(body).filter(|_| !off.todo) {
            out.push(Block::new(BlockKind::Todo { done }, md_inline_mode(rest.trim(), off.file)));
            spans.push(i..i + 1);
            i += 1;
            continue;
        }
        let hashes = body.bytes().take_while(|&b| b == b'#').count();
        if (1..=9).contains(&hashes) && body[hashes..].starts_with([' ', '\t']) {
            let t = body[hashes..].trim().trim_end_matches('#').trim_end();
            out.push(Block::new(BlockKind::Heading { level: hashes as u8, seq_auto: false }, md_inline_mode(t, off.file)));
            spans.push(i..i + 1);
            i += 1;
            continue;
        }
        if divider(body) && !off.code_divider {
            out.push(Block::new(BlockKind::Divider, RichText::default()));
            spans.push(i..i + 1);
            i += 1;
            continue;
        }
        if quote_line(body).is_some() && !off.quote {
            let start = i;
            let mut sub = vec![];
            while i < lines.len() {
                let Some(r) = quote_line(lines[i].trim()) else { break };
                sub.push(r);
                i += 1;
            }
            let kids = md_blocks(&sub, Off { quote: true, ..off });
            if !kids.is_empty() {
                out.push(Block::new(BlockKind::Quote, RichText::default()).with_children(kids));
                spans.push(start..i);
            }
            continue;
        }
        if let Some((t, n)) = pipe_table(&lines[i..], off.file) {
            out.push(t);
            spans.push(i..i + n);
            i += n;
            continue;
        }
        if off.file && let Some((source, used)) = html_media_block(&lines[i..]) {
            out.push(Block::new(BlockKind::Interactive(Interactive::html("", source)), RichText::default()));
            spans.push(i..i + used);
            i += used;
            continue;
        }
        if off.file && let Some((image, width, height, used)) = html_image_block(&lines[i..]) {
            let mut b = Block::new(BlockKind::Image { src_kind: image.src_kind, src: image.src, width, height, caption: (!image.alt.is_empty()).then_some(image.alt), crop: None }, RichText::default());
            b.align = crate::model::Align::Center;
            out.push(b);
            spans.push(i..i + used);
            i += used;
            continue;
        }
        if let Some((src, alt)) = image_line(body).filter(|_| off.file) {
            let mut b = Block::new(
                BlockKind::Image { src_kind: "src".into(), src: unescape_image(src), width: None, height: None, caption: (!alt.is_empty()).then(|| unescape_image(alt)), crop: None },
                RichText::default(),
            );
            b.align = crate::model::Align::Center;
            out.push(b);
            spans.push(i..i + 1);
            i += 1;
            continue;
        }
        // Leading spaces stay literal.
        out.push(Block::new(BlockKind::Text, md_inline_mode(&tabs(line.trim_end()), off.file)));
        spans.push(i..i + 1);
        i += 1;
    }
    out
}

/// Locate a balanced HTML div containing actual media, ignoring comments and
/// script/style text. A closing tag in a string or comment is not a boundary.
fn html_media_container(lines: &[&str]) -> Option<usize> {
    let mut source = String::new();
    for line in lines.iter().take(1024) {
        if source.len().saturating_add(line.len() + 1) > 256 * 1024 { break; }
        if !source.is_empty() { source.push('\n'); }
        source.push_str(line);
    }
    let lower = source.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let (mut at, mut depth, mut media) = (0, 0usize, false);
    let mut raw: Option<&str> = None;
    while at < bytes.len() {
        if raw.is_none() && at > 0 && bytes[at - 1] == b'\n' {
            let end = lower[at..].find('\n').map_or(bytes.len(), |n| at + n);
            if fence(&lower[at..end]).is_some() { return None; }
        }
        if let Some(tag) = raw {
            let close = format!("</{tag}");
            loop {
                at += lower[at..].find(&close)?;
                let after = at + close.len();
                if bytes.get(after).is_some_and(|c| *c == b'>' || c.is_ascii_whitespace()) { break; }
                at = after;
            }
            raw = None;
        }
        if bytes[at] != b'<' { at += 1; continue; }
        if lower[at..].starts_with("<!--") {
            at += 4 + lower[at + 4..].find("-->")? + 3;
            continue;
        }
        let mut p = at + 1;
        let closing = bytes.get(p) == Some(&b'/');
        if closing { p += 1; }
        let start = p;
        while bytes.get(p).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':')) { p += 1; }
        if start == p { at += 1; continue; }
        let tag = &lower[start..p];
        let mut quote = None;
        while p < bytes.len() {
            match (quote, bytes[p]) {
                (Some(q), c) if q == c => quote = None,
                (None, b'\'' | b'"') => quote = Some(bytes[p]),
                (None, b'>') => break,
                _ => {}
            }
            p += 1;
        }
        if p == bytes.len() { return None; }
        let self_closing = lower[at..p].trim_end().ends_with('/');
        if tag == "div" {
            if closing {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let end = p + 1;
                    let line_end = lower[end..].find('\n').map_or(bytes.len(), |n| end + n);
                    let tail = lower[end..line_end].trim();
                    if !media || !(tail.is_empty() || tail.starts_with("<script") || tail.starts_with("<style")) { return None; }
                    return Some(lower[..end].bytes().filter(|&c| c == b'\n').count() + 1);
                }
            } else if !self_closing { depth += 1; }
        } else if !closing && depth > 0 {
            if matches!(tag, "video" | "audio" | "canvas" | "model-viewer") { media = true; }
            if matches!(tag, "script" | "style" | "textarea" | "title") && !self_closing { raw = Some(match tag { "script" => "script", "style" => "style", "textarea" => "textarea", _ => "title" }); }
        }
        at = p + 1;
    }
    None
}

/// A standalone HTML video, audio, canvas or a media-containing div and its
/// immediately following scripts. Incomplete tags remain ordinary Markdown text.
fn html_media_block(lines: &[&str]) -> Option<(String, usize)> {
    fn starts_tag(line: &str, tag: &str) -> bool {
        let lower = line.trim_start().to_ascii_lowercase();
        lower.strip_prefix(&format!("<{tag}")).is_some_and(|rest| rest.starts_with('>') || rest.starts_with('/') || rest.chars().next().is_some_and(char::is_whitespace))
    }
    fn through_close(lines: &[&str], start: usize, tag: &str) -> Option<usize> {
        let close = format!("</{tag}>");
        for (i, line) in lines.iter().enumerate().skip(start) {
            if !matches!(tag, "script" | "style") && i > start && fence(line).is_some() { return None; }
            if line.to_ascii_lowercase().contains(&close) { return Some(i + 1); }
        }
        None
    }
    fn through_tag(lines: &[&str], start: usize) -> Option<usize> {
        let mut quote = None;
        for (i, line) in lines.iter().enumerate().skip(start) {
            for byte in line.bytes() {
                match (quote, byte) {
                    (Some(q), b) if q == b => quote = None,
                    (None, b'\'' | b'"') => quote = Some(byte),
                    (None, b'>') => return Some(i + 1),
                    _ => {},
                }
            }
        }
        None
    }
    let first = *lines.first()?;
    let tag = ["video", "audio", "canvas", "model-viewer", "div"].into_iter().find(|tag| starts_tag(first, tag))?;
    let mut end = if tag == "div" { html_media_container(lines)? } else { through_close(lines, 0, tag)? };
    // A script beginning after a closing tag on the same line also belongs to
    // the widget and may run across following lines.
    let tail = lines[end - 1].to_ascii_lowercase();
    let after = tail.find(&format!("</{tag}>")).map(|pos| &tail[pos + tag.len() + 3..]).unwrap_or("");
    if after.contains("<script") && !after.contains("</script>") { end = through_close(lines, end, "script")?; }
    else if after.contains("<style") && !after.contains("</style>") { end = through_close(lines, end, "style")?; }
    loop {
        let mut next = end;
        while next < lines.len() && lines[next].trim().is_empty() { next += 1; }
        if next == lines.len() { break; }
        end = if starts_tag(lines[next], "script") { through_close(lines, next, "script")? }
            else if starts_tag(lines[next], "style") { through_close(lines, next, "style")? }
            else if starts_tag(lines[next], "link") { through_tag(lines, next)? }
            else { break };
    }
    Some((lines[..end].join("\n"), end))
}

/// `![alt](src "title")` filling the whole line: its src.
fn image_line(s: &str) -> Option<(&str, &str)> {
    let (src, alt, used) = image_at(s)?;
    s[used..].trim().is_empty().then_some((src, alt))
}

/// Read an HTML image tag as inert Markdown content. HTML parsing supplies
/// proper attribute and entity handling; event handlers are never executed.
fn html_image_at(s: &str) -> Option<(InlineImage, Option<f32>, Option<f32>, usize)> {
    let prefix = s.get(..4)?;
    if !prefix.eq_ignore_ascii_case("<img") || !s.as_bytes().get(4).is_some_and(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>')) { return None; }
    let mut quote = None;
    let mut end = None;
    for (i, byte) in s.bytes().enumerate().skip(4) {
        match (quote, byte) {
            (Some(q), c) if q == c => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => { end = Some(i + 1); break; }
            _ => {}
        }
    }
    let end = end?;
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).from_utf8().read_from(&mut &s.as_bytes()[..end]).ok()?;
    let node = find(&dom.document, "img")?;
    let src = attr(&node, "src")?.trim().to_string();
    if src.is_empty() { return None; }
    let dimension = |name: &str| {
        let css = style(&node).into_iter().find(|(key, _)| key == name).map(|(_, value)| value);
        let value = css.or_else(|| attr(&node, name))?;
        let value = value.trim();
        value.strip_suffix("px").unwrap_or(value).trim().parse::<f32>().ok().filter(|value| value.is_finite() && *value > 0.)
    };
    let image = InlineImage { src_kind: "src".into(), src, alt: attr(&node, "alt").unwrap_or_default() };
    Some((image, dimension("width"), dimension("height"), end))
}

fn html_image_line(s: &str) -> Option<(InlineImage, Option<f32>, Option<f32>)> {
    let (image, width, height, end) = html_image_at(s)?;
    s[end..].trim().is_empty().then_some((image, width, height))
}

fn html_image_block(lines: &[&str]) -> Option<(InlineImage, Option<f32>, Option<f32>, usize)> {
    let first = lines.first()?.trim_start();
    if !first.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("<img")) { return None; }
    let mut source = String::new();
    for (i, line) in lines.iter().take(64).enumerate() {
        if source.len() + line.len() > 64 * 1024 { return None; }
        if i > 0 { source.push('\n'); }
        source.push_str(if i == 0 { first } else { line });
        if inert_html_tag_end(&source).is_some() {
            let (image, width, height, end) = html_image_at(&source)?;
            return source[end..].trim().is_empty().then_some((image, width, height, i + 1));
        }
    }
    None
}

/// Markdown image at the start of a string. Parentheses may be balanced in
/// bare destinations; angle destinations may contain spaces.
fn image_at(s: &str) -> Option<(&str, &str, usize)> {
    let bytes = s.as_bytes();
    if !s.starts_with("![") { return None; }
    let mut at = 2;
    let mut depth = 1usize;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if at + 1 < bytes.len() => at += 2,
            b'[' => { depth += 1; at += 1; }
            b']' => { depth -= 1; if depth == 0 { break; } at += 1; }
            _ => at += 1,
        }
    }
    let alt = s.get(2..at)?;
    if bytes.get(at..at + 2)? != b"](" { return None; }
    at += 2;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) { at += 1; }
    let src;
    if bytes.get(at) == Some(&b'<') {
        at += 1;
        let start = at;
        while at < bytes.len() && bytes[at] != b'>' {
            if bytes[at] == b'\\' && at + 1 < bytes.len() { at += 2; } else { at += 1; }
        }
        src = s.get(start..at)?;
        if bytes.get(at) != Some(&b'>') { return None; }
        at += 1;
    } else {
        let start = at;
        let mut parens = 0usize;
        while let Some(&ch) = bytes.get(at) {
            match ch {
                b'\\' if at + 1 < bytes.len() => at += 2,
                b'(' => { parens += 1; at += 1; }
                b')' if parens > 0 => { parens -= 1; at += 1; }
                b')' => break,
                c if c.is_ascii_whitespace() && parens == 0 => break,
                _ => at += 1,
            }
        }
        if parens != 0 { return None; }
        src = s.get(start..at)?;
    }
    if src.is_empty() { return None; }
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) { at += 1; }
    if let Some(quote @ (b'"' | b'\'')) = bytes.get(at).copied() {
        at += 1;
        while let Some(&ch) = bytes.get(at) {
            if ch == quote { break; }
            if ch == b'\\' && at + 1 < bytes.len() { at += 2; } else { at += 1; }
        }
        if bytes.get(at) != Some(&quote) { return None; }
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) { at += 1; }
    }
    (bytes.get(at) == Some(&b')')).then_some((src, alt, at + 1))
}

fn unescape_image(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) { out.push(chars.next().unwrap()); }
        else { out.push(ch); }
    }
    out
}

/// Source spans of inline Markdown images outside escapes and code spans.
pub fn markdown_image_spans(s: &str) -> Vec<std::ops::Range<usize>> { markdown_image_spans_inner(s, false) }

/// Image spans used for preserving surrounding Markdown edits, including
/// inert HTML image tags as well as Markdown image syntax.
pub fn markdown_image_spans_with_html(s: &str) -> Vec<std::ops::Range<usize>> { markdown_image_spans_inner(s, true) }

fn markdown_image_spans_inner(s: &str, html: bool) -> Vec<std::ops::Range<usize>> {
    let mut result = vec![];
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        let ch = rest.chars().next().unwrap();
        if ch == '\\' {
            at += ch.len_utf8();
            if let Some(next) = s[at..].chars().next().filter(char::is_ascii_punctuation) { at += next.len_utf8(); }
        } else if ch == '`' {
            let n = rest.bytes().take_while(|b| *b == b'`').count();
            let delim = &rest[..n];
            at += n + find_run(&rest[n..], delim, b'`', false).map_or(0, |end| end + n);
        } else if ch == '$' && let Some((_, used, _)) = markdown_math_at(rest, s[..at].chars().next_back()) {
            at += used;
        } else if ch == '<' && let Some(used) = inert_html_region_end(rest) {
            at += used;
        } else if ch == '<' {
            if html && let Some((_, _, _, used)) = html_image_at(rest) { result.push(at..at + used); at += used; }
            else if let Some(used) = inert_html_tag_end(rest) { at += used; }
            else { at += 1; }
        } else if ch == '!' {
            if let Some((_, _, used)) = image_at(rest) { result.push(at..at + used); at += used; }
            else if html && let Some((_, _, used)) = image_reference_at(rest) { result.push(at..at + used); at += used; }
            else { at += 1; }
        } else { at += ch.len_utf8(); }
    }
    result
}

pub fn markdown_image_unescape(s: &str) -> String { unescape_image(s) }

fn inert_html_tag_end(s: &str) -> Option<usize> {
    if !s.starts_with('<') || !s.as_bytes().get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/') { return None; }
    let mut quote = None;
    for (i, byte) in s.bytes().enumerate().skip(1) {
        match (quote, byte) {
            (Some(q), c) if q == c => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn inert_html_region_end(s: &str) -> Option<usize> {
    if s.starts_with("<!--") { return Some(s.find("-->").map_or(s.len(), |end| end + 3)); }
    let lower = s.to_ascii_lowercase();
    for tag in ["script", "style", "textarea", "title"] {
        let open = format!("<{tag}");
        if lower.strip_prefix(&open).is_some_and(|rest| rest.starts_with('>') || rest.chars().next().is_some_and(char::is_whitespace)) {
            let close = format!("</{tag}>");
            return Some(lower.find(&close).map_or(s.len(), |end| end + close.len()));
        }
    }
    None
}

/// Destination inside a single Markdown image expression, excluding angle brackets.
pub fn markdown_image_destination_span(s: &str) -> Option<std::ops::Range<usize>> {
    let (src, _, used) = image_at(s)?;
    if used != s.len() { return None; }
    let start = (src.as_ptr() as usize).checked_sub(s.as_ptr() as usize)?;
    Some(start..start.checked_add(src.len())?)
}

fn quote_line(s: &str) -> Option<&str> {
    if s == ">" {
        return Some("");
    }
    if let Some(r) = s.strip_prefix('>') {
        return Some(r.strip_prefix(' ').unwrap_or(r));
    }
    s.strip_prefix("》 ")
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').filter(|x| !x.ends_with('\\')).unwrap_or(t);
    let mut cells = vec![];
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

// GFM cells cannot contain literal newlines. This inert wrapper preserves a
// widget's source and metadata when a table is exported back to Markdown.
fn table_widget(value: &str) -> Option<Interactive> {
    use base64::Engine as _;
    let encoded = value.strip_prefix("<markdownx-interactive data-base64=\"")?
        .strip_suffix("\"></markdownx-interactive>")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let kind = match payload.get("kind")?.as_str()? {
        "html" => InteractiveKind::Html,
        "canvas" => InteractiveKind::Canvas,
        _ => return None,
    };
    let source = payload.get("source")?.as_str()?;
    if source.is_empty() { return None; }
    Some(Interactive {
        kind,
        title: payload.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        height: payload.get("height").and_then(|v| v.as_u64()).unwrap_or(360).min(u32::MAX as u64) as u32,
        source: source.to_string(),
    })
}

/// Complete interactive cells and their byte ranges in one GFM table row.
pub fn markdown_table_widgets(line: &str) -> Vec<(std::ops::Range<usize>, Interactive)> {
    let mut result = vec![];
    let mut start = usize::from(line.starts_with('|'));
    let bytes = line.as_bytes();
    let mut end = start;
    while end <= bytes.len() {
        if end < bytes.len() && bytes[end] == b'\\' && bytes.get(end + 1) == Some(&b'|') { end += 2; continue; }
        if end == bytes.len() || bytes[end] == b'|' {
            let raw = &line[start..end];
            let pieces: Vec<_> = if raw.contains("<markdownx-interactive data-base64=\"") { raw.split("<br>").collect() } else { vec![raw] };
            let mut offset = 0;
            for piece in pieces {
                let leading = piece.len() - piece.trim_start().len();
                let trailing = piece.trim_end().len();
                if leading <= trailing {
                    let value = piece[leading..trailing].replace("\\|", "|");
                    let widget = table_widget(&value).or_else(|| html_media_block(&[&value]).and_then(|(source, used)| (used == 1).then(|| Interactive::html("", source))));
                    if let Some(widget) = widget { result.push((start + offset + leading..start + offset + trailing, widget)); }
                }
                offset += piece.len() + "<br>".len();
            }
            start = end + 1;
        }
        end += 1;
    }
    result
}

/// Encode an interactive table cell without literal newlines or GFM pipes.
pub fn markdown_table_widget(widget: &Interactive) -> String {
    use base64::Engine as _;
    let payload = serde_json::json!({ "kind": widget.kind.as_str(), "title": widget.title, "height": widget.height(), "source": widget.source });
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
    format!("<markdownx-interactive data-base64=\"{encoded}\"></markdownx-interactive>")
}

/// GFM pipe table at the start of `lines`: (table, lines used).
fn pipe_table(lines: &[&str], file: bool) -> Option<(Block, usize)> {
    let head = lines.first()?;
    let sep = lines.get(1)?;
    if !head.contains('|') {
        return None;
    }
    let seps = table_cells(sep);
    let ok = |c: &String| {
        let c = c.trim_matches(':');
        !c.is_empty() && c.chars().all(|x| x == '-')
    };
    if !seps.iter().all(ok) || !sep.contains('-') {
        return None;
    }
    let header = table_cells(head);
    let ncols = header.len();
    if ncols != seps.len() {
        return None;
    }
    let aligns: Vec<Align> = seps
        .iter()
        .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        })
        .collect();
    let mut n = 2;
    let mut rows = vec![header];
    while let Some(l) = lines.get(n) {
        if l.trim().is_empty() || !l.contains('|') {
            break;
        }
        rows.push(table_cells(l));
        n += 1;
    }
    let rows = rows
        .into_iter()
        .map(|r| {
            (0..ncols)
                .map(|c| {
                    let value = r.get(c).map_or("", String::as_str);
                    let pieces: Vec<&str> = if file && value.contains("<markdownx-interactive data-base64=\"") { value.split("<br>").collect() } else { vec![value] };
                    let blocks = pieces.into_iter().map(|value| {
                        let mut b = if file {
                            if let Some(widget) = table_widget(value) {
                                Block::new(BlockKind::Interactive(widget), RichText::default())
                            } else if let Some((source, 1)) = html_media_block(&[value]) {
                                Block::new(BlockKind::Interactive(Interactive::html("", source)), RichText::default())
                            } else if let Some((image, width, height)) = html_image_line(value) {
                                Block::new(BlockKind::Image { src_kind: image.src_kind, src: image.src, width, height, caption: (!image.alt.is_empty()).then_some(image.alt), crop: None }, RichText::default())
                            } else if let Some((src, alt)) = image_line(value) {
                                Block::new(BlockKind::Image { src_kind: "src".into(), src: unescape_image(src), width: None, height: None, caption: (!alt.is_empty()).then(|| unescape_image(alt)), crop: None }, RichText::default())
                            } else { Block::new(BlockKind::Text, md_inline_mode(value, file)) }
                        } else { Block::new(BlockKind::Text, md_inline_mode(value, file)) };
                        b.align = aligns[c];
                        b
                    }).collect();
                    TableCell { blocks, ..TableCell::empty() }
                })
                .collect()
        })
        .collect();
    let mut t = Table { col_widths: vec![200.; ncols], header_rows: 1, header_col: false, rows };
    t.repair();
    Some((Block::new(BlockKind::Table(t), RichText::default()), n))
}

fn is_boundary(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace())
}

/// Inline Markdown: code spans, equations, links, bare URLs and the emphasis
/// rules of the live shortcuts (`~x~` is underline). Paste turns escapes,
/// images and `<…>` autolinks off.
pub fn md_inline(s: &str) -> RichText {
    md_inline_mode(s, false)
}

/// Whole-file imports must understand the escapes and tags emitted by export.
fn md_inline_mode(s: &str, file: bool) -> RichText {
    md_inline_with(s, file, true)
}

fn md_inline_with(s: &str, file: bool, autolink: bool) -> RichText {
    let mut out = RichText::default();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut RichText| {
        if !plain.is_empty() {
            out.push(plain, Marks::default());
            plain.clear();
        }
    };
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        let prev = s[..i].chars().next_back();
        if file && c == '\\' && let Some(next) = rest[1..].chars().next().filter(char::is_ascii_punctuation) {
            plain.push(next);
            i += 1 + next.len_utf8();
            continue;
        }
        if file && c == '!' && let Some((src, alt, used)) = image_at(rest) {
            flush(&mut plain, &mut out);
            let image = InlineImage { src_kind: "src".into(), src: unescape_image(src), alt: unescape_image(alt) };
            out.push(crate::editor::core::EQUATION, Marks { inline: Some(crate::docxml::to_inline(InlineKind::Image(image))), ..Marks::default() });
            i += used;
            continue;
        }
        if file && c == '<' && let Some((image, _, _, used)) = html_image_at(rest) {
            flush(&mut plain, &mut out);
            out.push(crate::editor::core::EQUATION, Marks { inline: Some(crate::docxml::to_inline(InlineKind::Image(image))), ..Marks::default() });
            i += used;
            continue;
        }
        if file && c == '<' && let Some(used) = inert_html_region_end(rest) {
            plain.push_str(&rest[..used]);
            i += used;
            continue;
        }
        if file {
            if rest.starts_with("&#32;") {
                plain.push(' ');
                i += 5;
                continue;
            }
            if let Some(tag) = ["<br>", "<br/>", "<br />"].iter().find(|tag| rest.starts_with(**tag)) {
                plain.push('\n');
                i += tag.len();
                continue;
            }
            let mut tagged = false;
            for (open, close, style) in [("<u>", "</u>", 0), ("<strong>", "</strong>", 1), ("<em>", "</em>", 2), ("<del>", "</del>", 3)] {
                if let Some(body) = rest.strip_prefix(open) && let Some(end) = find_unescaped(body, close) {
                    flush(&mut plain, &mut out);
                    let mut t = md_inline_with(&body[..end], file, autolink);
                    let len = t.len();
                    t.update_marks(0..len, |m| match style { 0 => m.underline = true, 1 => m.bold = true, 2 => m.italic = true, _ => m.strike = true });
                    out.append(&t);
                    i += open.len() + end + close.len();
                    tagged = true;
                    break;
                }
            }
            if tagged { continue; }
            if let Some(used) = inert_html_tag_end(rest) && rest[..used].to_ascii_lowercase().contains("<img") {
                plain.push_str(&rest[..used]);
                i += used;
                continue;
            }
        }
        // Code span.
        if c == '`' {
            let n = rest.bytes().take_while(|&b| b == b'`').count();
            let delim = &rest[..n];
            if let Some(end) = find_run(&rest[n..], delim, b'`', false) {
                let inner = &rest[n..n + end];
                let inner = if inner.len() > 2 && inner.starts_with(' ') && inner.ends_with(' ') && (!file || !inner.trim_matches(' ').is_empty()) { &inner[1..inner.len() - 1] } else { inner };
                if !inner.is_empty() {
                    flush(&mut plain, &mut out);
                    out.push(inner, Marks { code: true, ..Marks::default() });
                    i += n + end + n;
                    continue;
                }
            }
            plain.push_str(delim);
            i += n;
            continue;
        }
        // Equations accept surrounding punctuation; currency-like pairs stay text.
        if c == '$' && let Some((inner, used, _)) = markdown_math_at(rest, prev) {
            flush(&mut plain, &mut out);
            out.push(crate::editor::core::EQUATION, Marks { inline: Some(crate::docxml::to_inline(InlineKind::Equation(inner.to_string()))), ..Marks::default() });
            i += used;
            continue;
        }
        // Links `[text](url)`.
        if c == '[' {
            let open = 0;
            if let Some((label, url, used)) = link_at(&rest[open..], file) {
                flush(&mut plain, &mut out);
                let mut t = md_inline_with(label, file, false);
                if t.is_empty() {
                    t = RichText::plain(url.as_str());
                }
                let len = t.len();
                t.update_marks(0..len, |m| m.link = Some(url.to_string()));
                out.append(&t);
                i += open + used;
                continue;
            }
        }
        // Bare URLs (they get links when drawn).
        if autolink && (c == 'h' || c == 'w') && !prev.is_some_and(|p| p.is_alphanumeric()) {
            let l = rest.get(..8).unwrap_or(rest).to_ascii_lowercase();
            if l.starts_with("https://") || l.starts_with("http://") || l.starts_with("www.") {
                let end = rest.find(|c: char| c.is_whitespace() || "<>\"'`".contains(c) || !c.is_ascii()).unwrap_or(rest.len());
                let url = rest[..end].trim_end_matches(|c: char| ".,;:!?)]}*_~".contains(c));
                let host = url.splitn(2, "://").last().unwrap_or("");
                if host.len() > 3 && host.contains('.') {
                    flush(&mut plain, &mut out);
                    let href = if l.starts_with("www.") { format!("https://{url}") } else { url.to_string() };
                    out.push(url, Marks { link: Some(href), ..Marks::default() });
                    i += url.len();
                    continue;
                }
            }
        }
        // Emphasis.
        if matches!(c, '*' | '_' | '~') {
            let n = rest.bytes().take_while(|&b| b == c as u8).count();
            let run = &rest[..n];
            let rule: Option<fn(&mut Marks)> = match (c, n) {
                ('*' | '_', 3) => Some(|m| {
                    m.bold = true;
                    m.italic = true
                }),
                ('*' | '_', 2) => Some(|m| m.bold = true),
                ('*' | '_', 1) => Some(|m| m.italic = true),
                ('~', 2) => Some(|m| m.strike = true),
                ('~', 1) => Some(|m| m.underline = true),
                _ => None,
            };
            let bounded = c == '_' || (c == '~' && n == 1);
            if let Some(set) = rule
                && (!bounded || is_boundary(prev))
                && let Some(end) = find_run(&rest[n..], run, c as u8, file)
            {
                let inner = &rest[n..n + end];
                let after = rest[n + end + n..].chars().next();
                if !inner.starts_with(char::is_whitespace) && !inner.ends_with(char::is_whitespace) && (!bounded || is_boundary(after)) {
                    flush(&mut plain, &mut out);
                    let mut t = md_inline_with(inner, file, autolink);
                    let len = t.len();
                    t.update_marks(0..len, set);
                    out.append(&t);
                    i += n + end + n;
                    continue;
                }
            }
            plain.push_str(run);
            i += n;
            continue;
        }
        plain.push(c);
        i += c.len_utf8();
    }
    flush(&mut plain, &mut out);
    out
}

/// An inline math span, shared by parsing and resource/reference scanners.
fn markdown_math_at(s: &str, prev: Option<char>) -> Option<(&str, usize, bool)> {
    if !s.starts_with('$') || prev == Some('$') { return None; }
    for delim in ["$$", "$"] {
        if !s.starts_with(delim) || s[delim.len()..].starts_with('$') { continue; }
        let body = &s[delim.len()..];
        let Some(end) = find_unescaped(body, delim) else { continue };
        let inner = &body[..end];
        let after = body[end + delim.len()..].chars().next();
        if inner.trim().is_empty() || inner.contains('\n') || inner.starts_with(char::is_whitespace) || inner.ends_with(char::is_whitespace)
            || after == Some('$') || (delim == "$" && after.is_some_and(|c| c.is_ascii_digit())) { continue; }
        return Some((inner, delim.len() * 2 + end, delim == "$$"));
    }
    None
}

/// Byte ranges of inline math outside code and HTML markup on a physical line.
pub fn markdown_inline_math_spans(s: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = vec![];
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        let ch = rest.chars().next().unwrap();
        if ch == '\\' {
            at += 1 + rest[1..].chars().next().filter(char::is_ascii_punctuation).map_or(0, char::len_utf8);
        } else if ch == '`' {
            let n = rest.bytes().take_while(|b| *b == b'`').count();
            at += n + find_run(&rest[n..], &rest[..n], b'`', false).map_or(0, |end| end + n);
        } else if ch == '<' && let Some(end) = inert_html_region_end(rest).or_else(|| inert_html_tag_end(rest)) { at += end; }
        else if ch == '$' && let Some((_, used, _)) = markdown_math_at(rest, s[..at].chars().next_back()) {
            spans.push(at..at + used); at += used;
        } else { at += ch.len_utf8(); }
    }
    spans
}

/// Find a tag that is not preceded by an odd run of escape slashes.
fn find_unescaped(s: &str, needle: &str) -> Option<usize> {
    s.match_indices(needle).map(|(at, _)| at).find(|&at| s[..at].bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 0)
}

/// Offset in `s` of a delimiter run exactly equal to `delim` (not part of a
/// longer run of `ch`), skipping a non-empty match only.
fn find_run(s: &str, delim: &str, ch: u8, escapes: bool) -> Option<usize> {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(k) = s[from..].find(delim) {
        let at = from + k;
        let before_ok = at == 0 || b[at - 1] != ch;
        let after_ok = b.get(at + delim.len()) != Some(&ch);
        let escaped = escapes && s[..at].bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1;
        if at > 0 && before_ok && after_ok && !escaped {
            return Some(at);
        }
        from = at + delim.len().max(1);
        // Skip the rest of a longer run.
        while b.get(from) == Some(&ch) {
            from += 1;
        }
    }
    None
}

/// `[label](url "title")` at the start of `s`: (label, url, bytes used).
fn link_at(s: &str, file: bool) -> Option<(&str, String, usize)> {
    if file {
        let probe = format!("!{s}");
        let (url, label, used) = image_at(&probe)?;
        let start = (label.as_ptr() as usize).checked_sub(probe.as_ptr() as usize)?.checked_sub(1)?;
        let label = s.get(start..start + label.len())?;
        let url = unescape_image(url);
        if !(link_ok(&url) || url.starts_with("www.") || relative_link_ok(&url)) { return None; }
        return Some((label, url, used - 1));
    }
    let b = s.as_bytes();
    if b.first() != Some(&b'[') {
        return None;
    }
    let mut depth = 0;
    let mut close = None;
    let mut escaped = false;
    for (k, &c) in b.iter().enumerate() {
        if file && escaped { escaped = false; continue; }
        if file && c == b'\\' { escaped = true; continue; }
        match c {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let rest = &s[close + 1..];
    let inner = rest.strip_prefix('(')?;
    let end = inner.find(')')?;
    let target = inner[..end].trim();
    let url = target.split_whitespace().next().unwrap_or("").trim_matches(['<', '>']);
    if url.is_empty() || !(link_ok(url) || url.starts_with("www.")) {
        return None;
    }
    Some((&s[1..close], url.to_string(), close + 1 + 1 + end + 1))
}

#[cfg(test)]
mod tests {
    use super::html_to_blocks;
    use crate::docxml;
    use crate::model::{Document, RichText};

    fn xml(html: &str) -> String {
        docxml::serialize(&Document { blocks: html_to_blocks(html), ..Default::default() })
    }

    #[test]
    fn unicode_and_tab_list_markers_keep_nested_content() {
        use crate::model::BlockKind;
        for input in ["1。 Parent\n   - Child\n     ![frame](./shot.svg)\n", "-\tParent\n    - Child\n      ![frame](./shot.svg)\n", "1.   Parent\n     - Child\n       ![frame](./shot.svg)\n"] {
            let (blocks, _) = super::markdown_file_with_spans(input);
            assert_eq!(blocks.len(), 1, "{input}");
            let parent = &blocks[0];
            assert_eq!(parent.children.len(), 1, "{input}");
            let child = &parent.children[0];
            assert!(matches!(child.kind, BlockKind::Bullet), "{input}");
            assert_eq!(child.text.text, "Child");
            assert_eq!(child.children.len(), 1, "{input}");
            assert!(matches!(&child.children[0].kind, BlockKind::Image { src, .. } if src == "./shot.svg"), "{input}");
        }
    }

    #[test]
    fn indented_code_started_on_a_list_marker_is_not_an_image() {
        use crate::model::BlockKind;
        let input = "-     ![literal](missing.png)\n      second\n";
        let (blocks, _) = super::markdown_file_with_spans(input);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Bullet) && blocks[0].text.text.is_empty());
        assert!(matches!(blocks[0].children[0].kind, BlockKind::Code { .. }));
        assert_eq!(blocks[0].children[0].text.text, "![literal](missing.png)\nsecond");
    }

    #[test]
    fn list_item_media_keeps_its_interactive_child_and_relative_source() {
        let input = "- <video controls>\n  <source src=\"./clip.mp4\">\n  </video>\n\n- <canvas></canvas><script>let n = 1;\n\n  let s = `a\n\n  b`; </script>\n";
        let (blocks, _) = super::markdown_file_with_spans(input);
        assert_eq!(blocks.len(), 2);
        for block in &blocks { assert!(matches!(block.kind, crate::model::BlockKind::Bullet) && block.text.text.is_empty()); }
        let crate::model::BlockKind::Interactive(video) = &blocks[0].children[0].kind else { panic!("video child missing") };
        assert!(video.source.contains("./clip.mp4"));
        let crate::model::BlockKind::Interactive(canvas) = &blocks[1].children[0].kind else { panic!("canvas child missing") };
        assert!(canvas.source.contains("let n = 1;\n\nlet s = `a\n\nb`"));
    }

    #[test]
    fn indented_code_is_literal_but_footnote_images_remain_images() {
        use crate::model::BlockKind;
        let input = "A[^n].\n\n    ![literal](missing.png)\n    code two\n\n[^n]: note\n\n    ![real](real.svg)\n\n        ![literal](nested.png)\n\nAfter\n";
        let (blocks, spans) = super::markdown_file_with_spans(input);
        assert!(matches!(&blocks[1].kind, BlockKind::Code { .. }));
        assert_eq!(blocks[1].text.text, "![literal](missing.png)\ncode two");
        assert_eq!(spans[1], 2..4);
        assert!(blocks.iter().any(|b| matches!(&b.kind, BlockKind::Image { src, .. } if src == "real.svg")));
        assert!(blocks.iter().any(|b| matches!(&b.kind, BlockKind::Code { .. }) && b.text.text == "![literal](nested.png)"));
        let refs = "    ![literal][ref]\n\n[ref]: real.svg\n\n![real][ref]\n";
        assert!(super::expand_reference_images(refs).starts_with("    ![literal][ref]"));
        let nested = super::markdown_file_with_spans("> Quote\n>\n>     code in quote\n\n- Item\n\n      code in list\n").0;
        assert!(matches!(nested[0].kind, BlockKind::Quote));
        assert!(nested[0].children.iter().any(|b| matches!(b.kind, BlockKind::Code { .. }) && b.text.text == "code in quote"));
        assert!(matches!(nested[1].kind, BlockKind::Bullet));
        assert!(nested[1].children.iter().any(|b| matches!(b.kind, BlockKind::Code { .. }) && b.text.text == "code in list"));
    }

    #[test]
    fn math_is_inert_to_image_and_reference_expansion() {
        let source = "# Paper\n\n$\\text{![inline](missing.png)}$\n\n$\\text{![ref][shot]}$\n\n$$\n\\text{![block][shot]}\n$$\n\n> $$\n> \\text{![quote][shot]}\n> $$\n\n![real][shot]\n\n[shot]: ./real.svg\n";
        let expanded = super::expand_reference_images(source);
        assert!(expanded.contains("$\\text{![ref][shot]}$"), "{expanded}");
        assert!(expanded.contains("\\text{![block][shot]}"), "{expanded}");
        assert!(expanded.contains("\\text{![quote][shot]}"), "{expanded}");
        assert!(expanded.contains("![real](./real.svg)"), "{expanded}");
        let line = "$\\text{![fake](missing.png)}$ and ![real](./real.svg)";
        let spans = super::markdown_image_spans_with_html(line);
        assert_eq!(spans.len(), 1);
        assert_eq!(&line[spans[0].clone()], "![real](./real.svg)");
        let blocks = super::markdown_file_with_spans(source).0;
        assert_eq!(blocks[1].text.runs[0].marks.inline.as_ref().unwrap().equation(), Some("\\text{![inline](missing.png)}"));
        assert_eq!(blocks[2].text.runs[0].marks.inline.as_ref().unwrap().equation(), Some("\\text{![ref][shot]}"));
        assert_eq!(blocks[3].text.runs[0].marks.inline.as_ref().unwrap().equation(), Some("\\text{![block][shot]}"));
    }

    #[test]
    fn display_math_consumes_its_full_span_and_leaves_code_literal() {
        let input = "# Paper\n\n$$\n\\frac{1}{\\frac{a}{b}}\n$$\n\n$$x^2$$\n\n```tex\n$$\nliteral\n$$\n```\n\n$$\nunclosed\n";
        let (blocks, spans) = super::markdown_file_with_spans(input);
        assert_eq!(spans[1], 2..5);
        assert_eq!(spans[2], 6..7);
        for block in &blocks[1..3] {
            let obj = block.text.runs[0].marks.inline.as_ref().unwrap();
            assert!(obj.is_display_equation());
        }
        assert_eq!(blocks[1].text.runs[0].marks.inline.as_ref().unwrap().equation(), Some("\\frac{1}{\\frac{a}{b}}"));
        assert!(matches!(blocks[3].kind, crate::model::BlockKind::Code { .. }));
        assert_eq!(blocks[3].text.text, "$$\nliteral\n$$");
        assert_eq!(blocks[4].text.text, "$$");
        let doc = crate::model::Document { blocks, ..Default::default() };
        let reread = crate::docxml::parse(&crate::docxml::serialize(&doc)).unwrap();
        assert!(reread.blocks[1].text.runs[0].marks.inline.as_ref().unwrap().is_display_equation());
    }

    #[test]
    fn markdown_source_spans_cover_complete_blocks() {
        let text = "# Title\r\n\r\nfirst\r\nsecond\r\n\r\n- item\r\n  nested\r\n\r\n```rust\r\na\r\n```\r\n";
        let (blocks, spans) = super::markdown_file_with_spans(text);
        assert_eq!(spans, vec![0..1, 2..3, 3..4, 5..7, 8..11]);
        let plain = super::markdown_to_blocks(text, super::Off { file: true, ..Default::default() });
        let doc = |blocks| Document { blocks, ..Default::default() };
        assert_eq!(docxml::serialize(&doc(blocks)), docxml::serialize(&doc(plain)));
    }

    #[test]
    fn markdown_html_images_render_without_interpreting_code_or_scripts() {
        use crate::model::BlockKind;
        let input = "<IMG\n src='./shots/a&amp;b.svg'\n alt='封面' width='120' height='60'>\n\n正文 <img src=shots/b.svg alt='图 &gt; 样'> 后\n\n| 镜号 | 画面 |\n| --- | --- |\n| 01 | 左<img src='./shots/c.svg' alt='局部'>右 |\n\n`<img src='missing1.png'>`\n\n<!-- <img src='missing2.png'> -->\n\n<script>let x=\"<img src='missing3.png'>\"</script>\n";
        let blocks = super::markdown_to_blocks(input, super::Off { file: true, ..Default::default() });
        assert!(matches!(&blocks[0].kind, BlockKind::Image { src, caption, width: Some(w), height: Some(h), .. } if src == "./shots/a&b.svg" && caption.as_deref() == Some("封面") && *w == 120. && *h == 60.));
        let inline = blocks[1].text.runs.iter().find_map(|run| run.marks.inline.as_ref()?.image()).unwrap();
        assert_eq!((&*inline.src, &*inline.alt), ("shots/b.svg", "图 > 样"));
        let BlockKind::Table(table) = &blocks[2].kind else { panic!("expected table") };
        let inline = table.rows[1][1].blocks[0].text.runs.iter().find_map(|run| run.marks.inline.as_ref()?.image()).unwrap();
        assert_eq!(inline.src, "./shots/c.svg");
        assert!(blocks[3..].iter().all(|block| block.text.runs.iter().all(|run| run.marks.inline.as_ref().is_none_or(|i| i.image().is_none()))));
        assert_eq!(super::markdown_image_spans_with_html("`<img src='fake'>` x <img src='real'> <!-- <img src='fake'> -->").len(), 1);
        let (_, spans) = super::markdown_file_with_spans(input);
        assert_eq!(spans[0], 0..3);
    }

    #[test]
    fn storyboard_table_images_are_blocks() {
        let input = "| 镜号 | 分镜 |\n| --- | --- |\n| 01 | ![草图](./shots/01.svg) |\n| 02 | ![草图](shots/02.png) |\n";
        let blocks = super::markdown_to_blocks(input, super::Off { file: true, ..Default::default() });
        let crate::model::BlockKind::Table(table) = &blocks[0].kind else { panic!("expected table") };
        assert!(matches!(&table.rows[1][1].blocks[0].kind, crate::model::BlockKind::Image { src, .. } if src == "./shots/01.svg"));
        assert!(matches!(&table.rows[2][1].blocks[0].kind, crate::model::BlockKind::Image { src, .. } if src == "shots/02.png"));
    }

    #[test]
    fn reference_images_resolve_forward_and_keep_examples_inert() {
        use crate::model::BlockKind;
        let input = "![Cover][ SHOT ]\n\n| Scene | Frame |\n| --- | --- |\n| 1 | before ![Detail][] after |\n| 2 | ![Shortcut] |\n\n[shot]: ./shots/one.svg \"title\"\n[DETAIL]: shots/two.svg \"left|right\"\n[shortcut]: <./shots/three 3.svg>\n\n`![code][shot]`\n\n```md\n![example][shot]\n[fake]: missing.svg\n```\n\n<!-- ![comment][shot] -->\n";
        let (blocks, spans) = super::markdown_file_with_spans(input);
        assert!(matches!(&blocks[0].kind, BlockKind::Image { src, .. } if src == "./shots/one.svg"));
        let BlockKind::Table(table) = &blocks[1].kind else { panic!("expected table") };
        let inline = table.rows[1][1].blocks[0].text.runs.iter().find_map(|r| r.marks.inline.as_ref()?.image()).unwrap();
        assert_eq!(inline.src, "shots/two.svg");
        assert!(matches!(&table.rows[2][1].blocks[0].kind, BlockKind::Image { src, .. } if src == "./shots/three 3.svg"));
        assert_eq!(spans[0], 0..1);
        assert!(blocks.iter().any(|b| matches!(&b.kind, BlockKind::Code { .. }) && b.text.text.contains("![example][shot]")));
        assert!(super::expand_reference_images(input).contains("<!-- ![comment][shot] -->"));
        let hidden = "text <!--\n[hidden]: missing.svg\n-->\n\n![x][hidden]\n\n[valid]: ./ok.svg\n\n![ok][valid]\n";
        let expanded = super::expand_reference_images(hidden);
        assert!(expanded.contains("![x][hidden]"));
        assert!(expanded.contains("![ok](./ok.svg)"));
    }

    #[test]
    fn reference_links_resolve_and_footnotes_remain_literal() {
        let input = "Source [paper][REF] and [site][] and [other].\n\n[ref]: https://example.test/paper(1) \"Paper\"\n[site]: https://example.test/\n[other]: mailto:author@example.test\n\n[local file][file]\n\n[file]: ./paper.pdf\n\n`[example][ref]`\n";
        let blocks = super::markdown_to_blocks(input, super::Off { file: true, ..Default::default() });
        let links: Vec<_> = blocks[0].text.runs.iter().filter_map(|r| r.marks.link.as_deref()).collect();
        assert_eq!(links, vec!["https://example.test/paper%281%29", "https://example.test/", "mailto:author@example.test"]);
        assert!(blocks.iter().any(|b| b.text.runs.iter().any(|r| r.marks.link.as_deref() == Some("./paper.pdf"))));
        assert!(blocks.iter().any(|b| b.text.text.contains("[example][ref]")));
        let footnote = "Sentence[^note]\n\n[^note]: https://example.test/reference\n";
        assert_eq!(super::expand_reference_images(footnote), footnote);
        let mixed = "![cover][same]\n\n[download][same]\n\n[same]: ./shot.svg\n";
        let expanded = super::expand_reference_images(mixed);
        assert!(expanded.contains("![cover](./shot.svg)"));
        assert!(expanded.contains("[download](./shot.svg)"));
        assert!(!expanded.contains("[same]: ./shot.svg"));
    }

    #[test]
    fn relative_markdown_links_support_dot_paths_balanced_names_and_titles() {
        let input = "[one](./chapter.md) [two](chapter.md) [name](<./file (1).md> \"title\") [ref][paper]\n\n[paper]: ../legal/合同.pdf\n";
        let blocks = super::markdown_to_blocks(input, super::Off { file: true, ..Default::default() });
        let links: Vec<_> = blocks[0].text.runs.iter().filter_map(|r| r.marks.link.as_deref()).collect();
        assert_eq!(links, vec!["./chapter.md", "chapter.md", "./file (1).md", "../legal/合同.pdf"]);
        let output = crate::markdown::export(&Document { blocks, ..Default::default() }, &crate::markdown::Options::default()).text;
        assert!(output.contains("./file%20%281%29.md"));
        assert!(super::relative_link_ok("../legal/合同.pdf"));
        assert!(super::relative_link_ok("foo#heading"));
        assert!(super::relative_link_ok("#标题"));
        for invalid in ["", "/absolute", "C:/path", "https://x", "foo#", "foo#bar#baz", "file?query", "a\\b"] {
            assert!(!super::relative_link_ok(invalid), "{invalid}");
        }
    }

    #[test]
    fn inline_images_keep_surrounding_text_and_ignore_code() {
        let input = "前 ![镜头](<./shots/第 1.svg>) 后 `![code](x.png)`\n\n| Shot | Frame |\n| --- | --- |\n| 1 | 左 ![画面](shots/a(1).svg) 右 |\n";
        let blocks = super::markdown_to_blocks(input, super::Off { file: true, ..Default::default() });
        assert_eq!(blocks[0].text.plain_text(), "前 镜头 后 ![code](x.png)");
        let crate::model::BlockKind::Table(table) = &blocks[1].kind else { panic!("expected table") };
        assert_eq!(table.rows[1][1].blocks[0].text.plain_text(), "左 画面 右");
        let xml = docxml::serialize(&Document { blocks, ..Default::default() });
        assert!(xml.contains("src=\"./shots/第 1.svg\""), "{xml}");
        assert!(xml.contains("src=\"shots/a(1).svg\""), "{xml}");
    }

    #[test]
    fn standalone_html_media_and_canvas_become_interactive_without_consuming_code() {
        let input = "<video controls\n src='./clip.mp4'></video>\n\n<canvas id='game'></canvas>\n<script>\nlet n=1;\n</script>\n\n```html\n<video src='example.mp4'></video>\n```\n";
        let (blocks, spans) = super::markdown_file_with_spans(input);
        assert!(matches!(&blocks[0].kind, crate::model::BlockKind::Interactive(i) if i.source.contains("./clip.mp4")));
        assert!(matches!(&blocks[1].kind, crate::model::BlockKind::Interactive(i) if i.source.contains("let n=1")));
        assert!(matches!(&blocks[2].kind, crate::model::BlockKind::Code { .. }));
        assert_eq!(spans, vec![0..2, 3..7, 8..11]);
        let incomplete = super::markdown_to_blocks("<video>\n\n```html\n</video>\n```", super::Off { file: true, ..Default::default() });
        assert!(matches!(&incomplete[0].kind, crate::model::BlockKind::Text));
        let model = super::markdown_to_blocks("<model-viewer src='./model.obj'></model-viewer>", super::Off { file: true, ..Default::default() });
        assert!(matches!(&model[0].kind, crate::model::BlockKind::Interactive(i) if crate::html_assets::standalone_model(&i.source).as_deref() == Some("./model.obj")));
        let source = "<canvas id='game'></canvas>\n<link rel='stylesheet'\n href='./game.css'>\n<script src='game.js'></script>\n\nAfter";
        let (blocks, spans) = super::markdown_file_with_spans(source);
        assert!(matches!(&blocks[0].kind, crate::model::BlockKind::Interactive(i) if i.source.contains("game.css") && i.source.contains("game.js")));
        assert_eq!(spans[0], 0..4);
        assert_eq!(blocks[1].text.plain_text(), "After");
    }

    #[test]
    fn wrapped_media_preserves_nested_divs_and_inert_examples() {
        let source = "<div class='game'>\n<!-- </div><video src='fake.mp4'></video> -->\n<div><canvas id='game'></canvas></div>\n<script>const sample='</div>'; </script>\n</div>\n<script src='./game.js'></script>\n\nAfter\n\n| Shot | Game |\n| --- | --- |\n| 1 | <div><video src='clip.mp4'></video></div> |\n";
        let (blocks, spans) = super::markdown_file_with_spans(source);
        assert!(matches!(&blocks[0].kind, crate::model::BlockKind::Interactive(i) if i.source.contains("sample='</div>'") && i.source.contains("./game.js")));
        assert_eq!(spans[0], 0..6);
        assert_eq!(blocks[1].text.plain_text(), "After");
        let crate::model::BlockKind::Table(table) = &blocks[2].kind else { panic!("expected table") };
        assert!(matches!(&table.rows[1][1].blocks[0].kind, crate::model::BlockKind::Interactive(_)));
        for inert in ["<div>plain</div>", "<div><!-- <canvas></canvas> --></div>", "<div><script>let s='<canvas>';</script></div>", "<div><script>let s='</scripture><canvas></canvas>';</script></div>", "<div><canvas></canvas></div> trailing"] {
            let blocks = super::markdown_to_blocks(inert, super::Off { file: true, ..Default::default() });
            assert!(!matches!(blocks[0].kind, crate::model::BlockKind::Interactive(_)), "{inert}");
        }
    }

    #[test]
    fn blocks_and_marks() {
        let x = xml(
            "<meta charset='utf-8'><h2>Title</h2><p>a <b>bold</b> and <a href=\"https://x.y/\">link</a></p>\
             <ul><li>one<ul><li>sub</li></ul></li><li><input type=checkbox checked> done</li></ul>\
             <ol start=3><li>three</li></ol><blockquote><p>q</p></blockquote><hr><pre><code class=\"language-rust\">fn x() {}\n</code></pre>",
        );
        assert!(x.contains("<h2>Title</h2>"), "{x}");
        assert!(x.contains("<p>a <b>bold</b> and <a href=\"https://x.y/\">link</a></p>"), "{x}");
        assert!(x.contains("<li>one"), "{x}");
        assert!(x.contains("sub"), "{x}");
        assert!(x.contains("<checkbox done=\"true\">done</checkbox>"), "{x}");
        assert!(x.contains("<li seq=\"3\">three</li>"), "{x}");
        assert!(x.contains("<blockquote>"), "{x}");
        assert!(x.contains("<hr/>"), "{x}");
        assert!(x.contains("lang=\"rust\""), "{x}");
        assert!(x.contains("<code>fn x() {}</code></pre>"), "{x}");
    }

    #[test]
    fn feishu_todo_and_callout() {
        let x = xml("<ul class=\"list-done1\"><li data-list=\"done\"><div>t</div></li></ul>\
             <div class=\"zoneType-calloutBlock\"><div class=\"callout-container\" data-emoji-id=\"chestnut\">\
             <div class=\"callout-block\" style=\"background-color:rgb(255,245,235);border-color:rgb(254,212,164);\"><div>c</div></div></div></div>");
        assert!(x.contains("<checkbox done=\"true\">t</checkbox>"), "{x}");
        assert!(x.contains("<callout emoji=\"🌰\" background-color=\"light-orange\" border-color=\"orange\">"), "{x}");
    }

    #[test]
    fn google_docs_wrapper_is_not_bold() {
        let x = xml("<b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\"><p dir=\"ltr\"><span style=\"font-weight:700\">B</span><span> plain</span></p></b>");
        assert!(x.contains("<p><b>B</b> plain</p>"), "{x}");
    }

    #[test]
    fn whitespace_and_breaks() {
        let x = xml("<div>\n  line one <br>\n  line   two\n</div><p></p><div>x</div>");
        assert!(x.contains("<p>line one</p>\n<p>line two</p>\n<p></p>\n<p>x</p>"), "{x}");
    }

    #[test]
    fn markdown() {
        let x = docxml::serialize(&Document {
            title: RichText::default(),
            blocks: super::markdown_to_blocks(
                "# H1\n\nSome **bold** and *it* and ~u~ and `c` [l](https://a.b/c)\n- a\n  - b\n- [x] t\n3. three\n4. four\n> q\n\n---\n```rust\nfn main() {}\n```\n| A | B |\n|---|:-:|\n| 1 | 2 |\n",
                Default::default(),
            ),
            ..Default::default()
        });
        assert!(x.contains("<h1>H1</h1>"), "{x}");
        assert!(x.contains("<b>bold</b>"), "{x}");
        assert!(x.contains("<em>it</em>"), "{x}");
        assert!(x.contains("<u>u</u>"), "{x}");
        assert!(x.contains("<code>c</code>"), "{x}");
        assert!(x.contains("<a href=\"https://a.b/c\">l</a>"), "{x}");
        assert!(x.contains("<li>a"), "{x}");
        assert!(x.contains("<checkbox done=\"true\">t</checkbox>"), "{x}");
        assert!(x.contains("<li seq=\"3\">three</li>"), "{x}");
        assert!(x.contains("<li>four</li>"), "{x}");
        assert!(x.contains("<blockquote>"), "{x}");
        assert!(x.contains("<hr/>"), "{x}");
        assert!(x.contains("fn main() {}"), "{x}");
        assert!(x.contains("<table"), "{x}");
    }

    #[test]
    fn markdown_files_roundtrip_escaped_text_and_inline_markup() {
        use crate::model::{Block, BlockKind, Marks};
        let mut text = RichText::plain(r"literal *stars* _underscores_ [brackets] <angle> C:\notes\a_b ");
        text.push("underlined", Marks { underline: true, ..Default::default() });
        text.push("\nsecond line", Marks::default());
        let doc = Document {
            title: RichText::plain(r"Title_[brackets] \"),
            blocks: vec![Block::new(BlockKind::Text, text.clone())],
            ..Default::default()
        };
        let output = crate::markdown::export(&doc, &crate::markdown::Options::default());
        let blocks = super::markdown_to_blocks(&output.text, super::Off { file: true, ..Default::default() });
        assert_eq!(blocks[0].text.text, doc.title.text);
        assert_eq!(blocks[1].text.text, text.text);
        assert!(blocks[1].text.runs.iter().any(|r| r.marks.underline));
        let url = "https://example.com/a_b";
        let input = format!("# Title\n\n{url}\n");
        let first = super::markdown_to_blocks(&input, super::Off { file: true, ..Default::default() });
        let doc = Document { title: first[0].text.clone(), blocks: first[1..].to_vec(), ..Default::default() };
        let output = crate::markdown::export(&doc, &crate::markdown::Options::default());
        let second = super::markdown_to_blocks(&output.text, super::Off { file: true, ..Default::default() });
        assert_eq!(second[1].text.text, url);
        assert_eq!(super::md_inline(r"a\_b").text, r"a\_b");
    }

    #[test]
    fn markdown_literals() {
        let t = super::md_inline("a_b_c snake_case_name ** x** costs $5 and $6");
        assert!(t.runs.iter().all(|r| !r.marks.bold && !r.marks.italic), "{:?}", t);
        assert!(t.runs.iter().all(|r| r.marks.inline.is_none()));
        let math = super::md_inline("公式($1+2$)，$x_i^2$。 price $5-$10");
        let equations: Vec<_> = math.runs.iter().filter_map(|r| r.marks.inline.as_ref()?.equation()).collect();
        assert_eq!(equations, ["1+2", "x_i^2"]);
        assert!(math.text.contains("$5-$10"));
    }

    #[test]
    fn tables() {
        let x = xml("<table><tr><th>a</th><th>b</th></tr><tr><td colspan=2>c</td></tr></table>");
        assert!(x.contains("<table"), "{x}");
        assert!(x.contains("colspan=\"2\""), "{x}");
    }
}
