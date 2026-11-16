//! Read-only EPUB preview. Untrusted archives are parsed in a bounded worker;
//! only inert, locally embedded content reaches the existing HTML sandbox.
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use rbook::epub::Epub;
use serde::{Deserialize, Serialize};
use url::Url;

const MAX_EPUB: u64 = 32 * 1024 * 1024;
const MAX_OUTPUT: u64 = 64 * 1024 * 1024;
const MAX_HTML: usize = 24 * 1024 * 1024;
const MAX_TEXT: usize = 16 * 1024 * 1024;
const MAX_IMAGE: usize = 6 * 1024 * 1024;
const MAX_NODES: usize = 150_000;
const MAX_CHAPTERS: usize = 512;

#[derive(Serialize, Deserialize)]
struct Preview { text: String, html: String }

/// The archive is not written to disk, and both views use the same snapshot.
pub(crate) fn prepare(path: &Path) -> Result<(String, String), String> {
    let bytes = crate::store::read_bounded(path, MAX_EPUB).map_err(|e| e.to_string())?;
    let output = crate::document_worker::run(bytes, "--internal-epub-preview-worker", MAX_OUTPUT, Duration::from_secs(60), "EPUB")?;
    let preview: Preview = serde_json::from_slice(&output).map_err(|_| "EPUB preview result is invalid")?;
    if preview.text.len() > MAX_TEXT || preview.html.len() > MAX_HTML { return Err("EPUB preview is too large".into()); }
    Ok((preview.text, preview.html))
}

pub(crate) fn worker() -> Result<(), String> {
    let mut bytes = Vec::new();
    io::stdin().lock().take(MAX_EPUB + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_EPUB { return Err("EPUB is too large".into()); }
    let preview = extract(&bytes).map_err(|e| e.to_string())?;
    serde_json::to_writer(io::stdout().lock(), &preview).map_err(|e| e.to_string())
}

fn extract(bytes: &[u8]) -> Result<Preview> {
    let (mut archive, names) = crate::docx_preview::validated_archive(bytes)?;
    ensure!(!names.contains("META-INF/encryption.xml"), "encrypted EPUB resources are unsupported");
    let mut mime = String::new();
    archive.by_name("mimetype").context("EPUB media type is missing")?.take(128).read_to_string(&mut mime)?;
    ensure!(mime == "application/epub+zip", "invalid EPUB media type");
    drop(archive);
    let book = Epub::read(Cursor::new(bytes.to_vec())).map_err(|e| anyhow::anyhow!("invalid EPUB: {e}"))?;
    let mut render = Renderer { book: &book, html: String::new(), text: String::new(), images: HashMap::new(), nodes: 0, omitted: false };
    render.html.push_str(r#"<style>html,body{display:block!important;overflow:auto!important;height:auto!important;margin:0!important;padding:0!important;background:#eee!important}#epub-warning{position:sticky;top:0;z-index:2;padding:8px;background:#fff;color:#713100}#epub-content{max-width:850px;margin:auto;background:white;padding:24px;line-height:1.7}#epub-content img{max-width:100%;height:auto}#epub-content table{border-collapse:collapse;max-width:100%}#epub-content td,#epub-content th{border:1px solid #bbb;padding:4px}#epub-content section+section{border-top:1px solid #ddd;margin-top:24px;padding-top:12px}</style><div id="epub-warning">EPUB 只读预览：原有样式、链接或部分内容可能缺失；不运行脚本，不支持加密书籍。 / Read-only preview: styles, links or content may be omitted; scripts and encrypted books are unsupported.</div><div id="epub-content">"#);
    if let Some(title) = book.metadata().title() {
        render.push_html(&format!("<h1>{}</h1>", escape(title.value())))?;
        render.push_text(&format!("{}\n", title.value()))?;
    }
    for creator in book.metadata().creators().take(64) {
        render.push_html(&format!("<p>{}</p>", escape(creator.value())))?;
        render.push_text(&format!("{}\n", creator.value()))?;
    }
    let mut count = 0usize;
    for spine in book.spine().iter() {
        count += 1;
        ensure!(count <= MAX_CHAPTERS, "EPUB has too many chapters");
        let item = spine.manifest_entry().context("EPUB spine entry is missing")?;
        let base = Url::parse(&format!("https://epub.invalid{}", item.href())).context("invalid EPUB chapter path")?;
        render.push_html("<section>")?;
        render.push_text(&format!("\n[Chapter {count}]\n"))?;
        if matches!(item.media_type(), "application/xhtml+xml" | "text/html") {
            let mut chapter = LimitedBytes::new(MAX_TEXT);
            item.copy_bytes(&mut chapter).map_err(|e| anyhow::anyhow!("cannot read EPUB chapter: {e}"))?;
            let html = decode_chapter(chapter.data)?;
            let dom = html5ever::parse_document(RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()).context("invalid EPUB chapter")?;
            render.walk(&dom.document, &base, 0)?;
        } else if item.media_type() == "image/svg+xml" {
            if let Some(data) = render.image(base.as_str(), &base) {
                render.push_html(&format!("<img src=\"{data}\" alt=\"EPUB page\">"))?;
                render.push_text("[SVG page]\n")?;
            } else { render.omitted = true; }
        } else {
            render.omitted = true;
            render.push_html("<p>此章节格式无法显示 / Unsupported chapter type.</p>")?;
            render.push_text("[Unsupported chapter type]\n")?;
        }
        render.push_html("</section>")?;
    }
    ensure!(count > 0, "EPUB has no chapters");
    if render.omitted {
        render.push_html("<p>部分图片或内容未能显示，请核对原书。 / Some images or content could not be displayed; check the original.</p>")?;
        render.push_text("\n[Some images or content were omitted.]\n")?;
    }
    render.push_html("</div>")?;
    Ok(Preview { text: render.text, html: render.html })
}

fn decode_chapter(bytes: Vec<u8>) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        ensure!((bytes.len() - 2).is_multiple_of(2), "invalid EPUB UTF-16 chapter");
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|&c| if little { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) }).collect();
        return String::from_utf16(&units).context("invalid EPUB UTF-16 chapter");
    }
    String::from_utf8(bytes).context("EPUB chapter is not UTF-8 or UTF-16")
}

struct LimitedBytes { data: Vec<u8>, limit: usize }
impl LimitedBytes { fn new(limit: usize) -> Self { Self { data: Vec::new(), limit } } }
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.data.len()) { return Err(io::Error::other("EPUB resource limit exceeded")); }
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

struct Renderer<'a> {
    book: &'a Epub,
    html: String,
    text: String,
    images: HashMap<String, Option<String>>,
    nodes: usize,
    omitted: bool,
}

impl Renderer<'_> {
    fn push_html(&mut self, value: &str) -> Result<()> {
        ensure!(value.len() <= MAX_HTML.saturating_sub(self.html.len()), "EPUB layout is too large");
        self.html.push_str(value); Ok(())
    }
    fn push_text(&mut self, value: &str) -> Result<()> {
        ensure!(value.len() <= MAX_TEXT.saturating_sub(self.text.len()), "EPUB text is too large");
        self.text.push_str(value); Ok(())
    }
    fn image(&mut self, src: &str, base: &Url) -> Option<String> {
        if let Some((header, payload)) = src.split_once(',') && let Some(mime) = header.strip_prefix("data:").and_then(|s| s.strip_suffix(";base64")) {
            if !matches!(mime, "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml" | "image/avif") || payload.len() > MAX_IMAGE * 2 { return None; }
            let bytes = STANDARD.decode(payload).ok()?;
            if bytes.len() > MAX_IMAGE { return None; }
            return Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
        }
        let url = base.join(src).ok()?;
        if url.scheme() != "https" || url.host_str() != Some("epub.invalid") || url.query().is_some() { return None; }
        let key = url.path().to_owned();
        if let Some(cached) = self.images.get(&key) { return cached.clone(); }
        let result = (|| {
            let item = self.book.manifest().by_href(&key)?;
            let mime = item.media_type();
            if !matches!(mime, "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml" | "image/avif") { return None; }
            let mut bytes = LimitedBytes::new(MAX_IMAGE);
            item.copy_bytes(&mut bytes).ok()?;
            Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes.data)))
        })();
        self.images.insert(key, result.clone());
        result
    }
    fn walk(&mut self, node: &Handle, base: &Url, depth: usize) -> Result<()> {
        ensure!(depth <= 128, "EPUB markup is too deeply nested");
        self.nodes += 1;
        ensure!(self.nodes <= MAX_NODES, "EPUB has too many markup nodes");
        match &node.data {
            NodeData::Text { contents } => {
                let value = contents.borrow();
                self.push_html(&escape(&value))?;
                self.push_text(&value)?;
            }
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if matches!(tag, "head" | "script" | "style" | "template" | "noscript" | "iframe" | "object" | "embed" | "annotation" | "annotation-xml") { return Ok(()); }
                if tag == "svg" {
                    let mut svg = String::new();
                    self.svg_fragment(node, base, depth + 1, &mut svg, true)?;
                    if svg.is_empty() || svg.len() > MAX_IMAGE { self.omitted = true; }
                    else {
                        self.push_html(&format!("<img src=\"data:image/svg+xml;base64,{}\" alt=\"EPUB illustration\">", STANDARD.encode(svg)))?;
                        self.push_text(" [SVG 图示] ")?;
                    }
                    return Ok(());
                }
                if matches!(tag, "video" | "audio" | "canvas") { self.omitted = true; return Ok(()); }
                if tag == "img" {
                    let attrs = attrs.borrow();
                    let src = attrs.iter().find(|a| a.name.local.as_ref() == "src").map(|a| a.value.to_string()).unwrap_or_default();
                    let alt = attrs.iter().find(|a| a.name.local.as_ref() == "alt").map(|a| a.value.to_string()).unwrap_or_default();
                    if let Some(data) = self.image(&src, base) {
                        self.push_html(&format!("<img src=\"{data}\" alt=\"{}\">", escape(&alt)))?;
                    } else {
                        self.omitted = true;
                        self.push_html(&format!("<span>[图片不可用: {}]</span>", escape(&alt)))?;
                    }
                    self.push_text(&format!(" [图片: {alt}] "))?;
                    return Ok(());
                }
                if tag == "mfenced" {
                    let attrs = attrs.borrow();
                    let get = |key: &str, fallback: &str| attrs.iter().find(|a| a.name.local.as_ref() == key).map(|a| a.value.to_string()).filter(|v| v.len() <= 32).unwrap_or_else(|| fallback.to_owned());
                    let open = get("open", "("); let close = get("close", ")");
                    let separators: Vec<char> = get("separators", ",").chars().filter(|c| !c.is_whitespace()).collect();
                    self.push_html(&format!("<mrow><mo>{}</mo>", escape(&open)))?;
                    self.push_text(&open)?;
                    let mut index = 0usize;
                    for child in node.children.borrow().iter() {
                        if !matches!(&child.data, NodeData::Element { .. }) { continue; }
                        if index > 0 && !separators.is_empty() {
                            let separator = separators[(index - 1).min(separators.len() - 1)].to_string();
                            self.push_html(&format!("<mo>{}</mo>", escape(&separator)))?;
                            self.push_text(&separator)?;
                        }
                        self.walk(child, base, depth + 1)?;
                        index += 1;
                    }
                    self.push_html(&format!("<mo>{}</mo></mrow>", escape(&close)))?;
                    self.push_text(&close)?;
                    return Ok(());
                }
                let allowed = matches!(tag, "p" | "div" | "span" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "strong" | "em" | "b" | "i" | "u" | "s" | "code" | "pre" | "blockquote" | "ol" | "ul" | "li" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "th" | "td" | "figure" | "figcaption" | "sub" | "sup" | "math" | "mi" | "mo" | "mn" | "mrow" | "msup" | "msub" | "msubsup" | "mfrac" | "msqrt" | "mroot" | "mtext" | "mtable" | "mtr" | "mtd" | "mover" | "munder" | "munderover" | "mmultiscripts" | "mprescripts" | "none" | "mphantom" | "menclose" | "mspace" | "mstyle" | "mpadded" | "merror");
                let block = matches!(tag, "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" | "blockquote" | "pre" | "figcaption");
                if tag == "br" { self.push_html("<br>")?; self.push_text("\n")?; return Ok(()); }
                if tag == "hr" { self.push_html("<hr>")?; self.push_text("\n")?; return Ok(()); }
                if block { self.push_text("\n")?; }
                if allowed {
                    self.push_html(&format!("<{tag}"))?;
                    if tag == "td" || tag == "th" {
                        for attr in attrs.borrow().iter() {
                            let key = attr.name.local.as_ref();
                            if matches!(key, "colspan" | "rowspan") && let Ok(value) = attr.value.parse::<u8>() && (1..=64).contains(&value) {
                                self.push_html(&format!(" {key}=\"{value}\""))?;
                            }
                        }
                    }
                    if tag == "math" || tag.starts_with('m') {
                        for attr in attrs.borrow().iter() {
                            let key = attr.name.local.as_ref();
                            let value = attr.value.as_ref();
                            if matches!(key, "display" | "mathvariant" | "stretchy" | "displaystyle" | "scriptlevel" | "linethickness" | "notation" | "columnalign" | "rowalign" | "alttext")
                                && value.len() <= 256 && !value.chars().any(char::is_control) {
                                self.push_html(&format!(" {key}=\"{}\"", escape(value)))?;
                            }
                        }
                    }
                    self.push_html(">")?;
                }
                for child in node.children.borrow().iter() { self.walk(child, base, depth + 1)?; }
                if allowed { self.push_html(&format!("</{tag}>"))?; }
                if block { self.push_text("\n")?; }
                return Ok(());
            }
            _ => {}
        }
        for child in node.children.borrow().iter() { self.walk(child, base, depth + 1)?; }
        Ok(())
    }

    fn svg_fragment(&mut self, node: &Handle, base: &Url, depth: usize, output: &mut String, root: bool) -> Result<()> {
        ensure!(depth <= 128, "EPUB markup is too deeply nested");
        self.nodes += 1;
        ensure!(self.nodes <= MAX_NODES, "EPUB has too many markup nodes");
        match &node.data {
            NodeData::Text { contents } => output.push_str(&escape(&contents.borrow())),
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if tag == "style" {
                    let mut text = String::new();
                    for child in node.children.borrow().iter() {
                        if let NodeData::Text { contents } = &child.data { text.push_str(&contents.borrow()); }
                    }
                    let css = svg_stylesheet(&text);
                    if css.is_empty() && !text.trim().is_empty() { self.omitted = true; }
                    if !css.is_empty() { output.push_str("<style>"); output.push_str(&css); output.push_str("</style>"); }
                    return Ok(());
                }
                if !matches!(tag, "svg" | "g" | "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "text" | "tspan" | "defs" | "linearGradient" | "radialGradient" | "stop" | "clipPath" | "mask" | "pattern" | "use" | "image" | "title" | "desc") {
                    self.omitted = true; return Ok(());
                }
                output.push('<'); output.push_str(tag);
                if root { output.push_str(" xmlns=\"http://www.w3.org/2000/svg\""); }
                for attr in attrs.borrow().iter() {
                    let key = attr.name.local.as_ref();
                    let value = attr.value.as_ref();
                    if key == "href" {
                        let safe = if tag == "image" { self.image(value, base) } else if value.starts_with('#') && svg_identifier(&value[1..]) { Some(value.to_owned()) } else { None };
                        if let Some(safe) = safe { output.push_str(" href=\""); output.push_str(&escape(&safe)); output.push('"'); }
                        else { self.omitted = true; }
                    } else if key == "style" {
                        let safe = svg_declarations(value);
                        if !safe.is_empty() { output.push_str(" style=\""); output.push_str(&escape(&safe)); output.push('"'); }
                        if safe.is_empty() && !value.trim().is_empty() { self.omitted = true; }
                    } else if matches!(key, "id" | "class" | "viewBox" | "width" | "height" | "x" | "y" | "x1" | "y1" | "x2" | "y2" | "cx" | "cy" | "r" | "rx" | "ry" | "d" | "points" | "transform" | "fill" | "fill-rule" | "stroke" | "stroke-width" | "stroke-linecap" | "stroke-linejoin" | "opacity" | "fill-opacity" | "stroke-opacity" | "font-size" | "font-family" | "font-weight" | "text-anchor" | "dominant-baseline" | "offset" | "stop-color" | "stop-opacity" | "gradientUnits" | "gradientTransform" | "clip-path" | "clip-rule" | "mask" | "preserveAspectRatio") {
                        if svg_safe_value(value) { output.push(' '); output.push_str(key); output.push_str("=\""); output.push_str(&escape(value)); output.push('"'); }
                        else { self.omitted = true; }
                    }
                }
                output.push('>');
                for child in node.children.borrow().iter() { self.svg_fragment(child, base, depth + 1, output, false)?; }
                output.push_str("</"); output.push_str(tag); output.push('>');
            }
            _ => {}
        }
        ensure!(output.len() <= MAX_IMAGE, "EPUB SVG is too large");
        Ok(())
    }
}

fn svg_identifier(value: &str) -> bool { !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) }

fn svg_safe_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("url(") {
        return lower.strip_prefix("url(#").and_then(|s| s.strip_suffix(')')).is_some_and(svg_identifier);
    }
    !lower.contains("javascript:") && !lower.contains("expression(") && !value.contains(['\\', '@']) && !value.chars().any(char::is_control)
}

fn svg_declarations(value: &str) -> String {
    value.split(';').filter_map(|declaration| {
        let (name, value) = declaration.split_once(':')?;
        let name = name.trim(); let value = value.trim();
        if !matches!(name, "fill" | "fill-rule" | "stroke" | "stroke-width" | "stroke-linecap" | "stroke-linejoin" | "opacity" | "fill-opacity" | "stroke-opacity" | "font-size" | "font-family" | "font-weight" | "text-anchor" | "dominant-baseline" | "clip-path" | "clip-rule" | "shape-rendering" | "text-rendering" | "image-rendering" | "width" | "height") || value.is_empty() || !svg_safe_value(value) || value.contains(['{', '}', '<', '>']) { return None; }
        Some(format!("{name}:{value}"))
    }).collect::<Vec<_>>().join(";")
}

fn svg_stylesheet(value: &str) -> String {
    value.split('}').filter_map(|rule| {
        let (selectors, declarations) = rule.split_once('{')?;
        let selectors = selectors.trim();
        if selectors.is_empty() || !selectors.split(',').all(|s| s.trim().strip_prefix(['.', '#']).is_some_and(svg_identifier)) { return None; }
        let declarations = svg_declarations(declarations);
        (!declarations.is_empty()).then(|| format!("{selectors}{{{declarations}}}"))
    }).collect::<String>()
}

fn escape(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for c in value.chars() {
        match c { '&' => result.push_str("&amp;"), '<' => result.push_str("&lt;"), '>' => result.push_str("&gt;"), '"' => result.push_str("&quot;"), '\'' => result.push_str("&#39;"), _ => result.push(c) }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite_demo(name: &str, replace: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
        let bytes = include_bytes!("../MarkdownX-Demo/电子书论文示例.epub");
        let mut source = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut replace = Some(replace);
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let entry_name = entry.name().to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).unwrap();
            if entry_name == name { content = replace.take().unwrap()(content); }
            target.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
            target.write_all(&content).unwrap();
        }
        assert!(replace.is_none());
        target.finish().unwrap().into_inner()
    }

    #[test]
    fn ebook_keeps_chapter_order_images_and_math_without_active_content() {
        let bytes = include_bytes!("../MarkdownX-Demo/电子书论文示例.epub");
        let preview = extract(bytes).unwrap();
        assert!(preview.text.find("引言与方法").unwrap() < preview.text.find("结果与参考文献").unwrap());
        assert!(preview.html.contains("data:image/svg+xml;base64,") && preview.html.contains("<math>"));
        assert!(preview.html.contains("不运行脚本") && !preview.html.contains("<script"));
        assert!(extract(b"not an EPUB").is_err());
    }

    #[test]
    fn encrypted_archives_are_refused() {
        let bytes = include_bytes!("../MarkdownX-Demo/电子书论文示例.epub");
        let mut source = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            target.start_file(entry.name(), zip::write::SimpleFileOptions::default()).unwrap();
            std::io::copy(&mut entry, &mut target).unwrap();
        }
        target.start_file("META-INF/encryption.xml", zip::write::SimpleFileOptions::default()).unwrap();
        target.write_all(b"<encryption/>").unwrap();
        let encoded = target.finish().unwrap().into_inner();
        assert!(extract(&encoded).is_err());
    }

    #[test]
    fn scripts_external_images_and_event_handlers_are_not_rendered() {
        let bytes = rewrite_demo("EPUB/Text/01.xhtml", |content| {
            let original = String::from_utf8(content).unwrap();
            original.replace("</body>", r#"<script>window.epubProbe='BAD_SCRIPT';</script><img src="https://tracker.invalid/secret" onerror="BAD_HANDLER"/><svg xmlns="http://www.w3.org/2000/svg"><image href="../Images/%E7%A4%BA%E6%84%8F%20%E5%9B%BE.svg"/></svg><table><tr><td colspan="2" rowspan="3">Merged</td></tr></table></body>"#).into_bytes()
        });
        let preview = extract(&bytes).unwrap();
        assert!(!preview.html.contains("<script") && !preview.html.contains("BAD_HANDLER") && !preview.html.contains("tracker.invalid"));
        assert!(!preview.text.contains("BAD_SCRIPT") && preview.html.contains("colspan=\"2\" rowspan=\"3\""));
        assert!(preview.html.matches("data:image/svg+xml;base64,").count() >= 2 && preview.html.contains("部分图片或内容未能显示"));
    }

    #[test]
    fn utf16_chapters_preserve_supplementary_characters() {
        let bytes = rewrite_demo("EPUB/Text/02.xhtml", |content| {
            let text = String::from_utf8(content).unwrap().replace("虚构的分镜研究", "虚构的分镜研究 😀");
            let mut bytes = vec![0xff, 0xfe];
            for unit in text.encode_utf16() { bytes.extend_from_slice(&unit.to_le_bytes()); }
            bytes
        });
        assert!(extract(&bytes).unwrap().text.contains("😀"));
        assert!(decode_chapter(vec![0xff, 0xfe, 1]).is_err());
    }

    #[test]
    fn embedded_svg_keeps_safe_shapes_and_drops_active_content() {
        let bytes = rewrite_demo("EPUB/Text/01.xhtml", |content| {
            let original = String::from_utf8(content).unwrap();
            original.replace("</body>", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><style>.ink {fill:red} @import url(https://bad.invalid/x);</style><script>BAD_SVG</script><rect class="ink" x="1" y="1" width="18" height="18" onload="BAD_HANDLER"/></svg></body>"#).into_bytes()
        });
        let preview = extract(&bytes).unwrap();
        let encoded = preview.html.split("data:image/svg+xml;base64,").nth(2).unwrap().split('"').next().unwrap();
        let svg = String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
        assert!(svg.contains("<rect") && svg.contains("fill:red") && svg.contains("viewBox"));
        assert!(!svg.contains("BAD_SVG") && !svg.contains("BAD_HANDLER") && !svg.contains("bad.invalid"));
        assert!(!svg_safe_value(r"u\72l(https://bad.invalid)"));
    }

    #[test]
    fn legacy_mathml_fences_keep_parentheses_and_suppress_annotations() {
        let bytes = rewrite_demo("EPUB/Text/01.xhtml", |content| {
            let original = String::from_utf8(content).unwrap();
            original.replace("</body>", r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><msup><mfenced><mi>a</mi><mi>b</mi></mfenced><mn>2</mn></msup><annotation encoding="application/x-tex">DO_NOT_DUPLICATE</annotation></semantics></math></body>"#).into_bytes()
        });
        let preview = extract(&bytes).unwrap();
        assert!(preview.html.contains("<mrow><mo>(</mo><mi>a</mi><mo>,</mo><mi>b</mi><mo>)</mo></mrow>"));
        assert!(!preview.html.contains("DO_NOT_DUPLICATE") && !preview.text.contains("DO_NOT_DUPLICATE"));
    }
}
