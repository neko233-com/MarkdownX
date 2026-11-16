//! References in media, script and stylesheet elements in an HTML block. We
//! only rewrite attribute values, preserving scripts, comments and other HTML.
use std::ops::Range;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// Separate an SVG image URL from its view fragment. A `#` in the filename
/// must be URL-encoded before resolution; decoded absolute paths are split at
/// the final `#`. Filesystem callers should first prefer an existing full path.
pub fn svg_fragment(value: &str) -> Option<(&str, &str)> {
    if value.starts_with("data:") || value.starts_with("http://") || value.starts_with("https://") { return None; }
    let (before, fragment) = value.rsplit_once('#')?;
    if fragment.is_empty() { return None; }
    let prefix = if before.starts_with(r"\\?\") { 4 } else { 0 };
    let path = before[prefix..].find('?').map_or(before, |at| &before[..at + prefix]);
    if !path.to_ascii_lowercase().ends_with(".svg") { return None; }
    Some((path, &value[before.len()..]))
}

#[derive(Debug, PartialEq, Eq)]
pub struct Reference { pub range: Range<usize>, pub value: String, pub tag: String, pub attribute: String, pub tag_start: usize, pub module: bool, pub javascript: bool, quoted: bool }

impl Reference {
    /// Split local script URL queries and the encoded identities used in books.
    pub fn script_parts(&self) -> Option<(&str, &str)> {
        if self.tag != "script" || self.attribute != "src" || self.value.starts_with("data:") { return None; }
        let value = self.value.as_str();
        if std::path::Path::new(value).is_absolute() && std::path::Path::new(value).is_file() { return None; }
        let prefix = if value.starts_with(r"\\?\") { 4 } else { 0 };
        if let Some(at) = value[prefix..].find('?').map(|at| at + prefix) {
            return (at > prefix).then(|| (&value[..at], &value[at..]));
        }
        let at = value.rfind('#')?;
        let encoded = &value[at + 1..];
        let (groups, remainder) = encoded.as_bytes().as_chunks::<3>();
        if !encoded.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("%3f")) || !remainder.is_empty()
            || !groups.iter().all(|c| c[0] == b'%' && c[1].is_ascii_hexdigit() && c[2].is_ascii_hexdigit()) { return None; }
        Some((&value[..at], &value[at..]))
    }

    /// Split the supported media-fragment form, keeping the fragment for playback.
    pub fn media_time(&self) -> Option<(&str, &str)> {
        if self.attribute != "src" || !matches!(self.tag.as_str(), "video" | "audio" | "source") { return None; }
        let (path, fragment) = self.value.rsplit_once('#')?;
        let time = fragment.strip_prefix("t=")?;
        let time = time.strip_prefix("npt:").unwrap_or(time);
        if path.is_empty() || time.is_empty() || !time.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b':' | b','))
            || !time.bytes().any(|b| b.is_ascii_digit()) || time.matches(',').count() > 1 { return None; }
        let prefix = if path.starts_with(r"\\?\") { 4 } else { 0 };
        let query = path[prefix..].find('?').map(|position| position + prefix);
        let file = if std::path::Path::new(path).is_absolute() && std::path::Path::new(path).is_file() { path }
            else { query.map_or(path, |position| &path[..position]) };
        Some((file, &self.value[path.len()..]))
    }
}

/// Match the browser's URL serialization before encoding module identity.
pub fn module_identity(suffix: &str) -> String {
    if suffix.is_empty() { return String::new(); }
    const BASE: &str = "https://markdownx.invalid/module.js";
    let full = format!("{BASE}{suffix}");
    let normalized = url::Url::parse(&full).ok().and_then(|url| url.as_str().strip_prefix(BASE).map(str::to_string)).unwrap_or_else(|| suffix.to_string());
    format!("#{}", normalized.bytes().map(|byte| format!("%{byte:02X}")).collect::<String>())
}

/// Normalize book identities from older versions as well as new script queries.
pub fn script_identity(suffix: &str) -> String {
    if !suffix.starts_with('#') { return module_identity(suffix); }
    let encoded = &suffix.as_bytes()[1..];
    let (groups, remainder) = encoded.as_chunks::<3>();
    if !remainder.is_empty() || !groups.iter().all(|c| c[0] == b'%' && c[1].is_ascii_hexdigit() && c[2].is_ascii_hexdigit()) { return suffix.to_string(); }
    let bytes: Vec<u8> = groups.iter().map(|c| u8::from_str_radix(std::str::from_utf8(&c[1..]).unwrap(), 16).unwrap()).collect();
    match String::from_utf8(bytes) { Ok(raw) if raw.starts_with('?') => module_identity(&raw), _ => suffix.to_string() }
}

struct InlineStyle { range: Range<usize>, value: String, attribute: bool, quoted: bool }
struct InlineScript { range: Range<usize>, value: String, module: bool }

fn name_byte(c: u8) -> bool { c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':') }
fn skip_space(bytes: &[u8], mut p: usize) -> usize { while bytes.get(p).is_some_and(u8::is_ascii_whitespace) { p += 1; } p }

// HTML srcset URLs run to whitespace, not to the first comma: a data URL can
// itself contain commas. Trailing commas delimit a candidate without a size.
fn srcset_urls(value: &str) -> Vec<Range<usize>> {
    let bytes = value.as_bytes();
    let mut ranges = vec![];
    let mut p = 0;
    while p < bytes.len() {
        while bytes.get(p).is_some_and(|b| b.is_ascii_whitespace() || *b == b',') { p += 1; }
        let start = p;
        while bytes.get(p).is_some_and(|b| !b.is_ascii_whitespace()) { p += 1; }
        let mut end = p;
        while end > start && bytes[end - 1] == b',' { end -= 1; }
        if start < end { ranges.push(start..end); }
        if end != p { continue; }
        let mut parentheses = 0usize;
        while p < bytes.len() {
            match bytes[p] {
                b'(' => parentheses += 1,
                b')' => parentheses = parentheses.saturating_sub(1),
                b',' if parentheses == 0 => { p += 1; break; }
                _ => {}
            }
            p += 1;
        }
    }
    ranges
}

fn decode_attribute(encoded: &str) -> Option<String> {
    if let Ok(value) = quick_xml::escape::unescape(encoded) { return Some(value.into_owned()); }
    let html = format!("<img src=\"{}\">", encoded.replace('"', "&quot;"));
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()).ok()?;
    fn find(node: &Handle) -> Option<String> {
        if let NodeData::Element { name, attrs, .. } = &node.data && name.local.as_ref() == "img" {
            return attrs.borrow().iter().find(|attr| attr.name.local.as_ref() == "src").map(|attr| attr.value.to_string());
        }
        node.children.borrow().iter().find_map(find)
    }
    find(&dom.document)
}

/// Find src/srcset/poster attributes on media elements, excluding markup-looking
/// text in scripts, styles, comments and quoted attribute values.
pub fn references(source: &str) -> Vec<Reference> {
    scan(source, false).0
}

fn scan(source: &str, include_links: bool) -> (Vec<Reference>, Vec<InlineStyle>, Vec<Reference>, Vec<InlineScript>) {
    let bytes = source.as_bytes();
    let mut out = vec![];
    let mut styles = vec![];
    let mut links = vec![];
    let mut scripts = vec![];
    let mut i = 0;
    let mut raw: Option<(String, usize, bool, bool)> = None;
    while i < bytes.len() {
        if bytes[i] != b'<' { i += 1; continue; }
        if bytes[i..].starts_with(b"<!--") {
            if raw.is_some() { i += 1; continue; }
            i = source[i + 4..].find("-->").map_or(bytes.len(), |end| i + 4 + end + 3);
            continue;
        }
        let mut p = i + 1;
        let closing = bytes.get(p) == Some(&b'/');
        if closing { p += 1; }
        let start = p;
        while bytes.get(p).is_some_and(|c| name_byte(*c)) { p += 1; }
        if start == p { i += 1; continue; }
        let tag = source[start..p].to_ascii_lowercase();
        if let Some((in_raw, content_start, module, javascript)) = &raw {
            if closing && tag == *in_raw {
                if !include_links && *content_start <= i {
                    if tag == "style" { styles.push(InlineStyle { range: *content_start..i, value: source[*content_start..i].into(), attribute: false, quoted: false }); }
                    if tag == "script" && *javascript { scripts.push(InlineScript { range: *content_start..i, value: source[*content_start..i].into(), module: *module }); }
                }
                raw = None;
            } else { i += 1; continue; }
        }
        if closing { i = p; continue; }
        let media = matches!(tag.as_str(), "img" | "video" | "audio" | "source" | "track" | "model-viewer" | "script");
        let mut link = None;
        let mut stylesheet = false;
        let mut module = false;
        let mut javascript = tag == "script";
        let mut closed = false;
        while p < bytes.len() {
            p = skip_space(bytes, p);
            if p >= bytes.len() { break; }
            if bytes[p] == b'>' { p += 1; closed = true; break; }
            if bytes[p] == b'/' && bytes.get(p + 1) == Some(&b'>') { p += 2; closed = true; break; }
            let name_start = p;
            while bytes.get(p).is_some_and(|c| name_byte(*c)) { p += 1; }
            if p == name_start { p += 1; continue; }
            let attr = &source[name_start..p];
            p = skip_space(bytes, p);
            if bytes.get(p) != Some(&b'=') { continue; }
            p = skip_space(bytes, p + 1);
            let quote = bytes.get(p).copied().filter(|c| *c == b'\'' || *c == b'"');
            if quote.is_some() { p += 1; }
            let value_start = p;
            while p < bytes.len() && match quote { Some(q) => bytes[p] != q, None => !bytes[p].is_ascii_whitespace() && !matches!(bytes[p], b'>' | b'<') } { p += 1; }
            let value_end = p;
            if quote.is_some() && p == bytes.len() { break; }
            if !include_links && attr.eq_ignore_ascii_case("style") && value_start < value_end
                && let Some(value) = decode_attribute(&source[value_start..value_end])
            { styles.push(InlineStyle { range: value_start..value_end, value, attribute: true, quoted: quote.is_some() }); }
            if !include_links && tag == "script" && attr.eq_ignore_ascii_case("type") {
                let kind = decode_attribute(&source[value_start..value_end]).unwrap_or_default().trim().to_ascii_lowercase();
                module = kind == "module";
                let mime = kind.split(';').next().unwrap_or("").trim();
                javascript = module || matches!(mime, "" | "text/javascript" | "application/javascript" | "text/ecmascript" | "application/ecmascript" | "text/jscript" | "text/livescript");
            }
            if !include_links && tag == "link" && attr.eq_ignore_ascii_case("rel") {
                stylesheet = source[value_start..value_end].split_ascii_whitespace().any(|v| v.eq_ignore_ascii_case("stylesheet"));
            }
            if !include_links && tag == "link" && attr.eq_ignore_ascii_case("href") && value_start < value_end
                && let Some(value) = decode_attribute(&source[value_start..value_end])
            { link = Some(Reference { range: value_start..value_end, value, tag: tag.clone(), attribute: "href".into(), tag_start: i, module: false, javascript: false, quoted: quote.is_some() }); }
            if include_links && tag == "a" && attr.eq_ignore_ascii_case("href") && value_start < value_end
                && let Some(value) = decode_attribute(&source[value_start..value_end])
            { links.push(Reference { range: value_start..value_end, value, tag: tag.clone(), attribute: "href".into(), tag_start: i, module: false, javascript: false, quoted: quote.is_some() }); }
            if !include_links && media && (attr.eq_ignore_ascii_case("src") || tag == "video" && attr.eq_ignore_ascii_case("poster")) && value_start < value_end {
                let encoded = &source[value_start..value_end];
                if let Some(value) = decode_attribute(encoded) { out.push(Reference { range: value_start..value_end, value, tag: tag.clone(), attribute: attr.to_ascii_lowercase(), tag_start: i, module: false, javascript: false, quoted: quote.is_some() }); }
            }
            if !include_links && matches!(tag.as_str(), "img" | "source") && attr.eq_ignore_ascii_case("srcset") {
                let encoded = &source[value_start..value_end];
                for range in srcset_urls(encoded) {
                    if let Some(value) = decode_attribute(&encoded[range.clone()]) {
                        out.push(Reference { range: value_start + range.start..value_start + range.end, value, tag: tag.clone(), attribute: "srcset".into(), tag_start: i, module: false, javascript: false, quoted: quote.is_some() });
                    }
                }
            }
            if quote.is_some() { p += 1; }
        }
        if stylesheet && let Some(link) = link { out.push(link); }
        if javascript { for reference in out.iter_mut().filter(|reference| reference.tag_start == i) { reference.javascript = true; reference.module = module; } }
        if closed && matches!(tag.as_str(), "script" | "style" | "textarea" | "title") { raw = Some((tag, p, module, javascript)); }
        i = p.max(i + 1);
    }
    (out, styles, links, scripts)
}

pub fn has_inline_styles(source: &str) -> bool { !scan(source, false).1.is_empty() }

/// A widget consisting of a single declarative model viewer, without scripts.
pub fn standalone_model(source: &str) -> Option<String> {
    let trimmed = source.trim();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("<model-viewer")?;
    if !rest.starts_with('>') && !rest.chars().next().is_some_and(char::is_whitespace) { return None; }
    if !lower.ends_with("</model-viewer>") || lower.contains("<script") || lower.matches("<model-viewer").count() != 1 { return None; }
    references(trimmed).into_iter().find(|r| r.tag == "model-viewer" && r.attribute == "src").map(|r| r.value)
}

/// Rewrite recognized references without touching the surrounding source.
pub fn rewrite<E>(source: &str, mut map: impl FnMut(&str) -> Result<Option<String>, E>) -> Result<String, E> {
    rewrite_references(source, |reference| map(&reference.value))
}

/// Rewrite anchor destinations without touching scripts, comments or media.
pub fn rewrite_links<E>(source: &str, mut map: impl FnMut(&str) -> Result<Option<String>, E>) -> Result<String, E> {
    rewrite_list(source, scan(source, true).2, |reference| map(&reference.value))
}

/// As `rewrite`, with the element and attribute available to the mapper.
pub fn rewrite_references<E>(source: &str, mut map: impl FnMut(&Reference) -> Result<Option<String>, E>) -> Result<String, E> {
    rewrite_list(source, references(source), &mut map)
}

fn rewrite_list<E>(source: &str, refs: Vec<Reference>, mut map: impl FnMut(&Reference) -> Result<Option<String>, E>) -> Result<String, E> {
    let mut changes = vec![];
    for r in refs {
        if let Some(value) = map(&r)? {
            let mut escaped = value.replace('&', "&amp;").replace('"', "&quot;").replace('\'', "&#39;").replace('<', "&lt;").replace('>', "&gt;");
            let mut range = r.range;
            if !r.quoted {
                if r.attribute == "srcset" {
                    let commas = source[range.end..].bytes().take_while(|b| *b == b',').count();
                    escaped.push_str(&source[range.end..range.end + commas]);
                    range.end += commas;
                }
                escaped = format!("\"{escaped}\"");
            }
            changes.push((range, escaped));
        }
    }
    let mut result = source.to_string();
    for (range, value) in changes.into_iter().rev() { result.replace_range(range, &value); }
    Ok(result)
}

/// Rewrite only actual `style` attributes and `<style>` bodies, leaving
/// scripts, comments and ordinary quoted HTML attributes untouched.
pub fn rewrite_inline_styles<E>(source: &str, mut map: impl FnMut(&str) -> Result<Option<String>, E>) -> Result<String, E> {
    let mut changes = vec![];
    for style in scan(source, false).1 {
        if let Some(mut value) = map(&style.value)? {
            if style.attribute {
                value = value.replace('&', "&amp;").replace('"', "&quot;").replace('\'', "&#39;").replace('<', "&lt;").replace('>', "&gt;");
                if !style.quoted { value = format!("\"{value}\""); }
            }
            changes.push((style.range, value));
        }
    }
    let mut result = source.to_string();
    for (range, value) in changes.into_iter().rev() { result.replace_range(range, &value); }
    Ok(result)
}

/// Rewrite inline module script bodies while preserving HTML attributes and comments.
pub fn rewrite_inline_scripts<E>(source: &str, mut map: impl FnMut(&str, bool) -> Result<Option<String>, E>) -> Result<String, E> {
    let mut changes = vec![];
    for script in scan(source, false).3 {
        if let Some(value) = map(&script.value, script.module)? { changes.push((script.range, value)); }
    }
    let mut result = source.to_string();
    for (range, value) in changes.into_iter().rev() { result.replace_range(range, &value); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_queries_follow_browser_url_serialization() {
        let expected = "#%3F%76%3D%25%45%34%25%42%38%25%41%44";
        assert_eq!(module_identity("?v=中"), expected);
        assert_eq!(module_identity("?v=%E4%B8%AD"), expected);
        assert_eq!(script_identity("#%3F%76%3D%E4%B8%AD"), expected);
        assert_eq!(module_identity("?q=a b#图"), module_identity("?q=a%20b#%E5%9B%BE"));
        assert_ne!(module_identity("?q=%3f"), module_identity("?q=%3F"));
        assert_eq!(module_identity("?"), "#%3F");
        assert_eq!(module_identity("#"), "#%23");
    }
    #[test]
    fn recognizes_only_actual_media_attributes() {
        let html = "<!-- <img src='fake.png'> --><script>let a=\"<img src='fake.png'>\";</script><VIDEO controls src='./clip%201.mp4' poster=\"./a&amp;b.svg\"><source src=other.webm></VIDEO><p title=\"<img src='fake'>\">text</p>";
        let values = references(html).into_iter().map(|r| r.value).collect::<Vec<_>>();
        assert_eq!(values, ["./clip%201.mp4", "./a&b.svg", "other.webm"]);
        let result = rewrite::<()>(html, |src| Ok(Some(format!("new/{src}")))).unwrap();
        assert!(result.contains("src='new/./clip%201.mp4'"));
        assert!(result.contains("poster=\"new/./a&amp;b.svg\""));
        assert!(result.contains("let a=\"<img src='fake.png'>\""));
        let unquoted = rewrite::<()>("<video src=clip.mp4>", |_| Ok(Some("folder/my clip.mp4".into()))).unwrap();
        assert_eq!(unquoted, "<video src=\"folder/my clip.mp4\">");
        assert_eq!(references("<script><!-- intentionally unfinished </script><img src='ok.png'>").len(), 1);
        let html = "<link href='./game.css' rel='alternate stylesheet'><link rel='canonical' href='ignore.html'><script src='./game.js'>let s=\"<img src='fake'>\"</script>";
        let refs = references(html);
        assert_eq!(refs.iter().map(|r| (r.tag.as_str(), r.attribute.as_str(), r.value.as_str())).collect::<Vec<_>>(),
            [("link", "href", "./game.css"), ("script", "src", "./game.js")]);
        assert_eq!(standalone_model(" <model-viewer src='./mesh.glb'></model-viewer> ").as_deref(), Some("./mesh.glb"));
        assert!(standalone_model("<model-viewer src='a.obj'></model-viewer><script>1</script>").is_none());
        let links = "<!-- <a href='hidden'> --><script>let s=\"<a href='fake'>\"</script><a href='./a&amp;b.md#x'>one</a><a href=#local>two</a><img src='keep.png'>";
        let edited = rewrite_links::<()>(links, |href| Ok((href.starts_with("./")).then(|| "../a&b.md#x".to_string()))).unwrap();
        assert!(edited.contains("href='../a&amp;b.md#x'"));
        assert!(edited.contains("<a href=#local>two</a>"));
        assert!(edited.contains("let s=\"<a href='fake'>\""));
        assert!(edited.contains("<img src='keep.png'>"));
    }

    #[test]
    fn rewrites_srcset_candidates_and_keeps_descriptors_and_data_urls() {
        let html = "<picture><source srcset='./small.svg 1x, big&amp;wide.svg 2x'><img srcset=one.svg,></picture><img srcset='data:image/png;base64,AAAA 1x, ./next.svg 2x'>";
        let values = references(html).into_iter().map(|r| r.value).collect::<Vec<_>>();
        assert_eq!(values, ["./small.svg", "big&wide.svg", "one.svg", "data:image/png;base64,AAAA", "./next.svg"]);
        let result = rewrite::<()>(html, |src| Ok((!src.starts_with("data:")).then(|| format!("assets/{src}")))).unwrap();
        assert!(result.contains("srcset='assets/./small.svg 1x, assets/big&amp;wide.svg 2x'"));
        assert!(result.contains("srcset=\"assets/one.svg,\""));
        assert!(result.contains("data:image/png;base64,AAAA 1x, assets/./next.svg 2x"));
    }

    #[test]
    fn script_types_control_executable_rewriting() {
        let html = "<script type='application/json'>{\"fetch\":\"./keep.json\"}</script><script type='module'>fetch('./module.json')</script><script>fetch('./classic.json')</script><script src='plain.js'></script><script src='data.json' type='application/json'></script>";
        let scripts = references(html);
        assert!(scripts[0].javascript && !scripts[0].module);
        assert!(!scripts[1].javascript);
        let mut kinds = vec![];
        let rewritten = rewrite_inline_scripts::<()>(html, |script, module| { kinds.push(module); Ok(Some(script.replace("fetch", "LOAD"))) }).unwrap();
        assert_eq!(kinds, [true, false, false]);
        assert!(rewritten.contains("{\"fetch\":\"./keep.json\"}") && rewritten.contains("LOAD('./classic.json')"));
    }

    #[test]
    fn rewrites_real_inline_styles_without_touching_script_or_comments() {
        let html = "<!-- <p style='fake'> --><canvas style=\"background:url(&quot;a.png&quot;)\"></canvas><script>const x=\"<p style='fake'>\";</script><style>/* <i style='fake'> */ a { background: url('b.svg') }</style>";
        let result = rewrite_inline_styles::<()>(html, |css| Ok(Some(format!("{css}/*done*/")))).unwrap();
        assert!(result.contains("style=\"background:url(&quot;a.png&quot;)/*done*/\""));
        assert!(result.contains("a { background: url('b.svg') }/*done*/</style>"));
        assert!(result.contains("<script>const x=\"<p style='fake'>\";</script>"));
        assert!(result.contains("<!-- <p style='fake'> -->"));
    }

    #[test]
    fn decodes_html_named_entities_in_image_attributes() {
        let refs = references("<img src='shots/a&copy;.svg' alt='x'>");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].value, "shots/a©.svg");
        let output = rewrite::<()>("<img src='shots/a&copy;.svg'>", |_| Ok(Some("./assets/a&b.svg".into()))).unwrap();
        assert_eq!(output, "<img src='./assets/a&amp;b.svg'>");
    }
}
