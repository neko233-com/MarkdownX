//! Resolve stylesheet dependencies into data URLs for isolated widgets.
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cssparser::{Parser, Token};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 16;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Reference { span: Range<usize>, value: String, import: bool, function: bool }

/// Extract URL tokens, quoted `url()` and string-form `@import` without
/// mistaking comments or ordinary strings for resource references.
fn references(css: &str) -> Result<Vec<Reference>> {
    fn walk<'i>(parser: &mut Parser<'i>, out: &mut Vec<Reference>, depth: usize, failed: &mut bool) {
        if depth > MAX_DEPTH { *failed = true; return; }
        let mut import = false;
        loop {
            let start = parser.position().byte_index();
            let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else { break };
            let end = parser.position().byte_index();
            match token {
                Token::WhiteSpace(_) | Token::Comment(_) => continue,
                Token::AtKeyword(name) => { import = name.eq_ignore_ascii_case("import"); continue; }
                Token::QuotedString(value) if import => out.push(Reference { span: start..end, value: value.to_string(), import: true, function: false }),
                Token::UnquotedUrl(value) => out.push(Reference { span: start..end, value: value.to_string(), import, function: true }),
                Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                    let value = parser.parse_nested_block(|input| {
                        let value = input.expect_string()?.to_string();
                        input.expect_exhausted()?;
                        Ok::<_, cssparser::ParseError<()>>(value)
                    });
                    if let Ok(value) = value { out.push(Reference { span: start..parser.position().byte_index(), value, import, function: true }); }
                }
                Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                    let _ = parser.parse_nested_block(|input| {
                        walk(input, out, depth + 1, failed);
                        Ok::<_, cssparser::ParseError<()>>(())
                    });
                }
                _ => {}
            }
            import = false;
        }
    }
    let mut parser = Parser::new(css);
    let mut out = vec![];
    let mut failed = false;
    walk(&mut parser, &mut out, 0, &mut failed);
    ensure!(!failed, "stylesheet nesting is too deep");
    Ok(out)
}

/// Produce a self-contained UTF-8 stylesheet. Only files under `root` are
/// read; remote URLs remain unchanged for the widget CSP to block.
pub fn bundle_file(path: &Path, root: &Path, max_asset: u64) -> Result<Vec<u8>> {
    let root = root.canonicalize().context("stylesheet root is unavailable")?;
    let mut bundler = Bundler { root, max_asset, total: 0, loading: HashSet::new(), cached: HashMap::new() };
    let path = path.canonicalize().context("stylesheet is unavailable")?;
    bundler.css(&path, 0)
}

/// Resolve an HTML style attribute or `<style>` body against its document
/// directory, while keeping the original document source unchanged.
pub fn bundle_inline(css: &str, base: &Path, root: &Path, max_asset: u64) -> Result<String> {
    let root = root.canonicalize().context("stylesheet root is unavailable")?;
    let base = base.canonicalize().context("stylesheet base is unavailable")?;
    ensure!(base.starts_with(&root), "stylesheet base is outside the workspace");
    ensure!(css.len() as u64 <= max_asset, "inline stylesheet is too large");
    let mut bundler = Bundler { root, max_asset, total: 0, loading: HashSet::new(), cached: HashMap::new() };
    bundler.rewrite(css.to_string(), &base, 0)
}

struct Bundler { root: PathBuf, max_asset: u64, total: u64, loading: HashSet<PathBuf>, cached: HashMap<(PathBuf, bool), String> }

impl Bundler {
    fn read(&mut self, path: &Path) -> Result<Vec<u8>> {
        ensure!(path.starts_with(&self.root), "stylesheet dependency is outside the workspace");
        let file = std::fs::File::open(path).context("stylesheet dependency is missing")?;
        ensure!(file.metadata()?.is_file(), "stylesheet dependency is not a file");
        let mut bytes = vec![];
        file.take(self.max_asset.saturating_add(1)).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= self.max_asset, "stylesheet dependency is too large");
        self.total = self.total.checked_add(bytes.len() as u64).context("stylesheet dependencies are too large")?;
        ensure!(self.total <= MAX_TOTAL, "stylesheet dependencies are too large");
        Ok(bytes)
    }

    fn css(&mut self, path: &Path, depth: usize) -> Result<Vec<u8>> {
        ensure!(depth <= MAX_DEPTH, "stylesheet imports are too deep");
        ensure!(self.loading.insert(path.to_path_buf()), "circular stylesheet import");
        let result = self.css_inner(path, depth);
        self.loading.remove(path);
        result
    }

    fn css_inner(&mut self, path: &Path, depth: usize) -> Result<Vec<u8>> {
        let css = String::from_utf8(self.read(path)?).context("stylesheet is not UTF-8")?;
        Ok(self.rewrite(css, path.parent().context("stylesheet has no parent")?, depth)?.into_bytes())
    }

    fn rewrite(&mut self, css: String, base: &Path, depth: usize) -> Result<String> {
        let mut changes = vec![];
        let mut output_len = css.len();
        for reference in references(&css)? {
            let value = reference.value.trim();
            let lower = value.to_ascii_lowercase();
            if value.is_empty() || value.starts_with('#') || value.starts_with("//") || lower.starts_with("data:") || lower.starts_with("blob:") || lower.starts_with("http:") || lower.starts_with("https:") { continue; }
            let (without_fragment, fragment) = value.split_once('#').map_or((value, ""), |(a, b)| (a, b));
            let path_part = without_fragment.split_once('?').map_or(without_fragment, |(a, _)| a);
            if path_part.is_empty() { continue; }
            let decoded = crate::book::percent_decode_path(path_part).context("invalid stylesheet dependency")?;
            let relative = Path::new(&decoded);
            ensure!(!relative.is_absolute(), "absolute stylesheet dependency is not supported");
            let dependency = base.join(relative).canonicalize().context("stylesheet dependency is missing")?;
            ensure!(dependency.starts_with(&self.root), "stylesheet dependency is outside the workspace");
            let cache_key = (dependency.clone(), reference.import);
            let data = if let Some(data) = self.cached.get(&cache_key) { data.clone() } else {
                let bytes = if reference.import { self.css(&dependency, depth + 1)? } else { self.read(&dependency)? };
                let mime = if reference.import { "text/css" } else { crate::book::mime_of(dependency.extension().and_then(|e| e.to_str()).unwrap_or("")) };
                let data = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
                ensure!(data.len() as u64 <= self.max_asset, "embedded stylesheet dependency is too large");
                self.cached.insert(cache_key, data.clone());
                data
            };
            let value = if fragment.is_empty() { data } else { format!("{data}#{fragment}") };
            let mut quoted = String::new();
            cssparser::serialize_string(&value, &mut quoted).expect("writing to a string cannot fail");
            // A generated URL inside an HTML <style> must not create </style>.
            quoted = quoted.replace('<', "\\3c ");
            let replacement = if reference.function { format!("url({quoted})") } else { quoted };
            output_len = output_len.checked_sub(reference.span.len()).and_then(|n| n.checked_add(replacement.len())).context("stylesheet is too large")?;
            ensure!(output_len as u64 <= self.max_asset, "stylesheet is too large after embedding dependencies");
            changes.push((reference.span, replacement));
        }
        let mut output = css;
        for (span, value) in changes.into_iter().rev() { output.replace_range(span, &value); }
        if output.len() as u64 > self.max_asset { bail!("stylesheet is too large after embedding dependencies"); }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_css_urls_and_imports_without_touching_comments_or_strings() {
        let css = "/* url(fake.png) */ @import './theme.css' screen; .a { content: 'url(no.png)'; background: url(\"img/a b.png\"); mask:URL(../x.svg#icon) }";
        let values = references(css).unwrap();
        assert_eq!(values.iter().map(|r| (&*r.value, r.import)).collect::<Vec<_>>(), [("./theme.css", true), ("img/a b.png", false), ("../x.svg#icon", false)]);
        let escaped = references(r#"@import url("theme.css"); a { background: url("a\20 b.png") }"#).unwrap();
        assert_eq!(escaped.iter().map(|r| (&*r.value, r.import)).collect::<Vec<_>>(), [("theme.css", true), ("a b.png", false)]);
    }

    #[test]
    fn bundles_nested_css_images_and_fonts_without_leaving_workspace() {
        let root = std::env::temp_dir().join(format!("markdownx-css-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("styles")).unwrap();
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::write(root.join("images/a b.svg"), b"<svg/>").unwrap();
        std::fs::write(root.join("styles/font.woff2"), b"font data").unwrap();
        std::fs::write(root.join("styles/theme.css"), "@font-face { src: url(font.woff2) }").unwrap();
        let main = root.join("styles/game.css");
        std::fs::write(&main, "@import './theme.css'; canvas { background: url('../images/a%20b.svg#icon') }").unwrap();
        let bundled = String::from_utf8(bundle_file(&main, &root, 1024 * 1024).unwrap()).unwrap();
        assert!(bundled.contains("data:text/css;base64,"));
        assert!(bundled.contains("data:image/svg+xml;base64,PHN2Zy8+#icon"));
        let imported = bundled.split("data:text/css;base64,").nth(1).unwrap().split('"').next().unwrap();
        let imported = String::from_utf8(STANDARD.decode(imported).unwrap()).unwrap();
        assert!(imported.contains("data:font/woff2;base64,"));
        std::fs::write(root.join("styles/theme.css"), "@import 'game.css';").unwrap();
        assert!(bundle_file(&main, &root, 1024 * 1024).is_err());
        let outside = root.with_extension("private.svg");
        std::fs::write(&outside, b"private").unwrap();
        let outside_name = outside.file_name().unwrap().to_string_lossy();
        std::fs::write(&main, format!("a {{ background: url('../../{outside_name}') }}")).unwrap();
        assert!(bundle_file(&main, &root, 1024 * 1024).unwrap_err().to_string().contains("outside"));
        assert_eq!(std::fs::read(&outside).unwrap(), b"private");
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_css_uses_document_directory_and_never_reads_outside_root() {
        let root = std::env::temp_dir().join(format!("markdownx-inline-css-{}", uuid::Uuid::new_v4()));
        let base = root.join("chapter");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("frame.svg"), b"wrong").unwrap();
        std::fs::write(base.join("frame.svg"), b"<svg/>").unwrap();
        std::fs::write(base.join("palette.css"), "a { color: red }").unwrap();
        let original = "@import 'palette.css'; canvas { background: url(./frame.svg) }";
        let output = bundle_inline(original, &base, &root, 1024 * 1024).unwrap();
        assert!(output.contains("data:text/css;base64,") && output.contains("data:image/svg+xml;base64,PHN2Zy8+"));
        assert_eq!(std::fs::read(base.join("frame.svg")).unwrap(), b"<svg/>");
        assert!(bundle_inline("a { background: url(./missing.png) }", &base, &root, 1024 * 1024).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
