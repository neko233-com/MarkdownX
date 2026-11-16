//! Apply SVG view fragments before passing images to the desktop SVG renderer.
//! The renderer does not implement `<view>` or fragment selection itself.

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let digits = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(digits, 16).ok()?);
            i += 3;
        } else { out.push(bytes[i]); i += 1; }
    }
    String::from_utf8(out).ok()
}

fn view_box(value: &str) -> Option<String> {
    let numbers: Vec<f64> = value.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|part| !part.is_empty()).map(str::parse).collect::<Result<_, _>>().ok()?;
    if numbers.len() != 4 || numbers.iter().any(|n| !n.is_finite()) || numbers[2] <= 0.0 || numbers[3] <= 0.0 { return None; }
    Some(numbers.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))
}

fn aspect(value: &str) -> Option<String> {
    let parts: Vec<_> = value.split_ascii_whitespace().collect();
    let alignment = *parts.first()?;
    if !matches!(alignment, "none" | "xMinYMin" | "xMidYMin" | "xMaxYMin" | "xMinYMid" | "xMidYMid" | "xMaxYMid" | "xMinYMax" | "xMidYMax" | "xMaxYMax")
        || parts.len() > 2 || parts.get(1).is_some_and(|v| !matches!(*v, "meet" | "slice")) { return None; }
    Some(parts.join(" "))
}

/// Replace a target pseudo-class only in selector preludes, keeping quoted
/// strings, comments, attribute values and declaration values untouched.
fn target_selectors(css: &str, marker: &str) -> Option<String> {
    let bytes = css.as_bytes();
    let mut at = 0;
    let mut start = 0;
    let mut rules = vec![true];
    let mut brackets = 0usize;
    let mut parens = 0usize;
    let mut changes = Vec::new();
    while at < bytes.len() {
        if bytes[at..].starts_with(b"/*") {
            at += 2;
            while at + 1 < bytes.len() && !bytes[at..].starts_with(b"*/") { at += 1; }
            at = (at + 2).min(bytes.len());
            continue;
        }
        if matches!(bytes[at], b'\'' | b'"') {
            let quote = bytes[at]; at += 1;
            while at < bytes.len() {
                if bytes[at] == b'\\' { at = (at + 2).min(bytes.len()); }
                else if bytes[at] == quote { at += 1; break; }
                else { at += 1; }
            }
            continue;
        }
        if bytes[at] == b'\\' { at = (at + 2).min(bytes.len()); continue; }
        match bytes[at] {
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'(' if brackets == 0 => parens += 1,
            b')' if brackets == 0 => parens = parens.saturating_sub(1),
            b'{' if brackets == 0 && parens == 0 => {
                let prelude = css[start..at].trim_start();
                let nested = *rules.last().unwrap_or(&false) && ["@media", "@supports", "@layer", "@container"].iter().any(|name| prelude.starts_with(name));
                rules.push(nested); start = at + 1;
            }
            b'}' if brackets == 0 && parens == 0 => { if rules.len() > 1 { rules.pop(); } start = at + 1; }
            b';' if brackets == 0 && parens == 0 => start = at + 1,
            b':' if brackets == 0 && *rules.last().unwrap_or(&false) && !css[start..at].trim_start().starts_with('@')
                && (at == 0 || bytes[at - 1] != b':') && bytes.get(at..at + 7).is_some_and(|part| part.eq_ignore_ascii_case(b":target"))
                && !bytes.get(at + 7).is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'\\') || !byte.is_ascii()) => {
                changes.push(at..at + 7); at += 7; continue;
            }
            _ => {}
        }
        at += 1;
    }
    if changes.is_empty() { return None; }
    let mut result = css.to_string();
    for range in changes.into_iter().rev() { result.replace_range(range, &format!("[{marker}]")); }
    Some(result)
}

fn start_tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, byte) in source.as_bytes().get(start..)?.iter().copied().enumerate() {
        match (quote, byte) {
            (None, b'\'' | b'"') => quote = Some(byte),
            (Some(q), b) if q == b => quote = None,
            (None, b'>') => {
                let mut end = start + offset;
                if source.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') { end -= 1; }
                return Some(end);
            }
            _ => {}
        }
    }
    None
}

/// Returns a transformed SVG, or `None` when the requested fragment cannot be
/// represented faithfully by the renderer. The input is never changed on disk.
pub fn apply(bytes: &[u8], fragment: &str) -> Option<Vec<u8>> {
    let source = std::str::from_utf8(bytes).ok()?;
    let fragment = decode(fragment.strip_prefix('#')?)?;
    let document = roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg") { return None; }
    let target = if fragment.starts_with("svgView(") { None } else { document.descendants().find(|node| node.is_element() && node.attribute("id") == Some(fragment.as_str())) };
    let mut bounds = None;
    let mut ratio = None;
    if let Some(spec) = fragment.strip_prefix("svgView(").and_then(|v| v.strip_suffix(')')) {
        for item in spec.split(';') {
            if let Some(value) = item.strip_prefix("viewBox(").and_then(|v| v.strip_suffix(')')) {
                if bounds.is_some() { return None; }
                bounds = Some(view_box(value)?);
            } else {
                let value = item.strip_prefix("preserveAspectRatio(").and_then(|v| v.strip_suffix(')'))?;
                if ratio.is_some() { return None; }
                ratio = Some(aspect(value)?);
            }
        }
        if bounds.is_none() && ratio.is_none() { return None; }
    } else {
        if let Some(target) = target && target.tag_name().name() == "view" && target.tag_name().namespace() == root.tag_name().namespace() {
            bounds = target.attribute("viewBox").map(view_box).transpose_option()?;
            ratio = target.attribute("preserveAspectRatio").map(aspect).transpose_option()?;
        }
    }
    let replacements = [("viewBox", bounds), ("preserveAspectRatio", ratio)];
    let mut changes = Vec::new();
    let mut added = String::new();
    let marker = (0..1000).map(|i| format!("data-markdownx-target-{i}")).find(|name| !source.contains(name))?;
    let mut styled_target = false;
    for style in document.descendants().filter(|node| node.is_element() && node.tag_name().name() == "style" && node.tag_name().namespace() == root.tag_name().namespace()) {
        if style.children().any(|node| node.is_element()) { continue; }
        let css = style.children().filter(|node| node.is_text()).filter_map(|node| node.text()).collect::<String>();
        if let Some(updated) = target_selectors(&css, &marker) {
            let end = style.range().end;
            let body = start_tag_end(source, style.range().start)? + 1;
            let close = source.get(body..end)?.rfind("</")? + body;
            changes.push((body..close, quick_xml::escape::escape(&updated).into_owned()));
            styled_target = true;
        }
    }
    if styled_target && let Some(target) = target {
        let end = start_tag_end(source, target.range().start)?;
        changes.push((end..end, format!(" {marker}=\"\"")));
    }
    for (name, value) in replacements {
        if let Some(value) = value {
            if let Some(attr) = root.attributes().find(|attr| attr.name() == name && attr.namespace().is_none()) {
                changes.push((attr.range_value(), value));
            } else { added.push_str(&format!(" {name}=\"{value}\"")); }
        }
    }
    if !added.is_empty() {
        let end = start_tag_end(source, root.range().start)?;
        changes.push((end..end, added));
    }
    changes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = source.to_string();
    for (range, value) in changes { result.replace_range(range, &value); }
    Some(result.into_bytes())
}

trait TransposeOption<T> { fn transpose_option(self) -> Option<Option<T>>; }
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> { match self { Some(Some(value)) => Some(Some(value)), Some(None) => None, None => Some(None) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><view id="right" viewBox="50 0 50 50"/><rect id="red" width="50" height="50" fill="red"/><rect x="50" width="50" height="50" fill="blue"/></svg>"#;
    #[test]
    fn selected_view_changes_viewbox_without_changing_source() {
        for fragment in ["#right", "#svgView(viewBox(50,0,50,50))", "#%72ight"] {
            let result = String::from_utf8(apply(SVG.as_bytes(), fragment).unwrap()).unwrap();
            let doc = roxmltree::Document::parse(&result).unwrap();
            assert_eq!(doc.root_element().attribute("viewBox"), Some("50 0 50 50"));
            let tree = usvg::Tree::from_data(result.as_bytes(), &usvg::Options::default()).unwrap();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(100, 50).unwrap();
            resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
            assert_eq!(pixmap.pixel(10, 25).unwrap().red(), 255);
            assert_eq!(pixmap.pixel(50, 25).unwrap().blue(), 255);
            assert_eq!(pixmap.pixel(90, 25).unwrap().alpha(), 0);
        }
        assert_eq!(apply(SVG.as_bytes(), "#red").unwrap(), SVG.as_bytes());
        assert_eq!(apply(SVG.as_bytes(), "#missing").unwrap(), SVG.as_bytes());
        assert!(apply(SVG.as_bytes(), "#svgView(viewBox(1,2,0,3))").is_none());
    }

    #[test]
    fn target_selectors_match_the_fragment_and_keep_specificity() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><style>/* :target */ rect {fill:red} :target {fill:blue} .shape {fill:green}</style><rect id="frame" width="50" height="50"/></svg>"#;
        let pixel = |source: &str, fragment: &str| {
            let viewed = apply(source.as_bytes(), fragment).unwrap();
            let tree = usvg::Tree::from_data(&viewed, &usvg::Options::default()).unwrap();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(50, 50).unwrap();
            resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
            pixmap.pixel(25, 25).unwrap()
        };
        assert_eq!(pixel(svg, "#frame").blue(), 255);
        assert_eq!(pixel(svg, "#missing").red(), 255);
        let classed = svg.replace("id=\"frame\"", "id=\"frame\" class=\"shape\"");
        assert_eq!(pixel(&classed, "#frame").green(), 128);
        let svg = svg.replace("/* :target */", "/* :target */ rect {content: ':target'}");
        let result = String::from_utf8(apply(svg.as_bytes(), "#frame").unwrap()).unwrap();
        let parsed = roxmltree::Document::parse(&result).unwrap();
        let css = parsed.descendants().find(|node| node.has_tag_name("style")).unwrap().text().unwrap();
        assert!(css.contains("/* :target */") && css.contains("':target'"));
        assert_eq!(pixel(&svg, "#frame").blue(), 255);
    }
}
