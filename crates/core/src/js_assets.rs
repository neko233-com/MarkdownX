//! Bundle relative ES module dependencies as data URLs for isolated widgets.
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ress::{Scanner, tokens::{Keyword, Token}};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 16;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_TOKENS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferenceKind { Module, Resource, Fetch, DomResource }

/// Cheap prefilter for classic scripts; the lexer makes the actual decision.
pub fn may_reference_local_assets(source: &str) -> bool {
    source.contains("import") || source.contains("fetch") || source.contains("Audio")
        || (source.contains("src") || source.contains("setAttribute")) && (source.contains("Image") || source.contains("createElement"))
}

/// Locate module imports and literal URLs based on import.meta.url.
fn references(source: &str) -> Result<Vec<(Range<usize>, String, ReferenceKind)>> {
    if !source.contains("import") && !source.contains("export") && !may_reference_local_assets(source) { return Ok(vec![]); }
    let mut tokens = vec![];
    for item in Scanner::new(source) {
        let item = item.map_err(|error| anyhow::anyhow!("invalid JavaScript: {error}"))?;
        if !matches!(item.token, Token::Comment(_) | Token::EoF) {
            ensure!(tokens.len() < MAX_TOKENS, "JavaScript has too many tokens");
            tokens.push(item);
        }
    }
    // Only recognize explicit, immutable DOM element declarations. This avoids
    // treating arbitrary object properties named `src` as file references.
    let mut scopes = Vec::with_capacity(tokens.len());
    let mut stack = vec![0usize];
    let mut next_scope = 0usize;
    for token in &tokens {
        if token.token == "}" && stack.len() > 1 { stack.pop(); }
        scopes.push(*stack.last().unwrap());
        if token.token == "{" { next_scope += 1; stack.push(next_scope); }
    }
    let builtins = ["fetch", "Image", "Audio", "document", "window", "self", "globalThis"];
    let mut shadowed = HashSet::<String>::new();
    let mut binding_steps = 0usize;
    for i in 0..tokens.len() {
        let declaration = matches!(tokens[i].token, Token::Keyword(Keyword::Const(_) | Keyword::Let(_) | Keyword::Var(_) | Keyword::Import(_)));
        if !declaration { continue; }
        let mut start = i + 1;
        if matches!(tokens[i].token, Token::Keyword(Keyword::Import(_)))
            && tokens.get(start).is_some_and(|t| t.token == "*")
            && tokens.get(start + 1).is_some_and(|t| t.token == "as")
            && let Some(name) = tokens.get(start + 2)
            && matches!(name.token, Token::Ident(_))
        {
            let name = &source[name.span.start..name.span.end];
            if builtins.contains(&name) { shadowed.insert(name.to_string()); }
        }
        if matches!(tokens[i].token, Token::Keyword(Keyword::Import(_)))
            && tokens.get(start).is_some_and(|t| matches!(t.token, Token::Ident(_)))
            && tokens.get(start + 1).is_some_and(|t| t.token == ",") { start += 2; }
        let Some(open) = tokens.get(start) else { continue };
        let close = if open.token == "{" { "}" } else if open.token == "[" { "]" } else { continue };
        let mut nesting = 0usize;
        let mut closed = false;
        for token in tokens.iter().skip(start).take(4096) {
            binding_steps += 1;
            if binding_steps > MAX_TOKENS * 2 { break; }
            if token.token == open.token { nesting += 1; }
            if token.token == close {
                nesting = nesting.saturating_sub(1);
                if nesting == 0 { closed = true; break; }
            }
            if matches!(token.token, Token::Ident(_)) {
                let name = &source[token.span.start..token.span.end];
                if builtins.contains(&name) { shadowed.insert(name.to_string()); }
            }
        }
        if !closed { shadowed.extend(builtins.iter().map(|name| name.to_string())); }
        if binding_steps > MAX_TOKENS * 2 { break; }
    }
    for i in 0..tokens.len() {
        if !matches!(tokens[i].token, Token::Ident(_)) { continue; }
        let name = &source[tokens[i].span.start..tokens[i].span.end];
        if !builtins.contains(&name) { continue; }
        let declaration = i > 0 && (matches!(tokens[i - 1].token, Token::Keyword(Keyword::Const(_) | Keyword::Let(_) | Keyword::Var(_) | Keyword::Function(_) | Keyword::Class(_) | Keyword::Import(_)))
            || tokens[i - 1].token == "," && tokens.get(i + 1).is_some_and(|t| t.token == "="));
        // Treat ambiguous function arguments as possible parameter bindings.
        let parameter = tokens.get(i + 1).is_some_and(|t| t.token == "=>")
            || i > 0 && (tokens[i - 1].token == "(" || tokens[i - 1].token == ",")
                && tokens.get(i + 1).is_some_and(|t| t.token == ")" || t.token == "," || t.token == "=");
        if declaration || parameter { shadowed.insert(name.to_string()); }
    }
    let mut parameter_steps = 0usize;
    for i in 0..tokens.len() {
        if !matches!(tokens[i].token, Token::Keyword(Keyword::Function(_) | Keyword::Catch(_))) { continue; }
        let Some(start) = (i + 1..tokens.len().min(i + 32)).find(|&j| tokens[j].token == "(") else { continue };
        let mut nesting = 0usize;
        let mut closed = false;
        for token in tokens.iter().skip(start).take(4096) {
            parameter_steps += 1;
            if parameter_steps > MAX_TOKENS * 2 { break; }
            if token.token == "(" { nesting += 1; }
            if token.token == ")" {
                nesting = nesting.saturating_sub(1);
                if nesting == 0 { closed = true; break; }
            }
            if matches!(token.token, Token::Ident(_)) {
                let name = &source[token.span.start..token.span.end];
                if builtins.contains(&name) { shadowed.insert(name.to_string()); }
            }
        }
        if !closed { shadowed.extend(builtins.iter().map(|name| name.to_string())); }
        if parameter_steps > MAX_TOKENS * 2 { break; }
    }
    let mut elements = HashMap::<String, (usize, usize)>::new();
    let mut declarations = HashMap::<String, usize>::new();
    let mut initializer_steps = 0usize;
    let ends_initializer = |end: usize| {
        tokens.get(end + 1).is_none_or(|next| {
            next.token == ";" || next.token == "," || next.token == ")" || next.token == "}"
                || (matches!(next.token, Token::Ident(_) | Token::Keyword(_))
                    && source[tokens[end].span.end..next.span.start].contains(['\r', '\n']))
        })
    };
    for i in 0..tokens.len() {
        if matches!(tokens[i].token, Token::Keyword(Keyword::Const(_) | Keyword::Let(_) | Keyword::Var(_)))
            && let Some(name) = tokens.get(i + 1)
            && matches!(name.token, Token::Ident(_))
        {
            let name = &source[name.span.start..name.span.end];
            *declarations.entry(name.to_string()).or_default() += 1;
            let constructor = tokens.get(i + 3).is_some_and(|t| matches!(t.token, Token::Keyword(Keyword::New(_))))
                && tokens.get(i + 4).is_some_and(|t| t.token == "Image" && !shadowed.contains("Image") || t.token == "Audio" && !shadowed.contains("Audio"));
            let constructor = constructor && if tokens.get(i + 5).is_some_and(|t| t.token == "(") {
                let mut nesting = 0usize;
                let mut end = None;
                for j in i + 5..tokens.len().min(i + 261) {
                    initializer_steps += 1;
                    if initializer_steps > MAX_TOKENS * 2 { break; }
                    if tokens[j].token == "(" { nesting += 1; }
                    if tokens[j].token == ")" {
                        nesting = nesting.saturating_sub(1);
                        if nesting == 0 { end = Some(j); break; }
                    }
                }
                end.is_some_and(ends_initializer)
            } else { ends_initializer(i + 4) };
            let created = !shadowed.contains("document") && tokens.get(i + 3).is_some_and(|t| t.token == "document")
                        && tokens.get(i + 4).is_some_and(|t| t.token == ".")
                        && tokens.get(i + 5).is_some_and(|t| t.token == "createElement")
                        && tokens.get(i + 6).is_some_and(|t| t.token == "(")
                        && tokens.get(i + 7).is_some_and(|t| matches!(&t.token, Token::String(value) if ["img", "audio", "video", "source"].iter().any(|tag| value.as_ref().eq_ignore_ascii_case(tag))))
                        && tokens.get(i + 8).is_some_and(|t| t.token == ")") && ends_initializer(i + 8);
            let constructed = tokens.get(i + 2).is_some_and(|t| t.token == "=") && (constructor || created);
            if constructed && matches!(tokens[i].token, Token::Keyword(Keyword::Const(_))) { elements.insert(name.to_string(), (i, scopes[i])); }
        }
    }
    elements.retain(|name, _| declarations.get(name) == Some(&1));
    // Expression-bodied arrows do not introduce a brace-delimited scope. If
    // a parameter shadows an element name, leave that name untouched.
    let mut arrow_steps = 0usize;
    for i in 1..tokens.len() {
        if tokens[i].token != "=>" { continue; }
        if matches!(tokens[i - 1].token, Token::Ident(_)) {
            let name = &source[tokens[i - 1].span.start..tokens[i - 1].span.end];
            elements.remove(name);
            if builtins.contains(&name) { shadowed.insert(name.to_string()); }
        } else if tokens[i - 1].token == ")" {
            let mut nesting = 0usize;
            let mut start = None;
            for j in (0..i).rev() {
                arrow_steps += 1;
                if arrow_steps > MAX_TOKENS * 2 { elements.clear(); break; }
                if tokens[j].token == ")" { nesting += 1; }
                if tokens[j].token == "(" {
                    nesting = nesting.saturating_sub(1);
                    if nesting == 0 { start = Some(j); break; }
                }
            }
            let Some(start) = start else { elements.clear(); break; };
            for token in &tokens[start + 1..i - 1] {
                if matches!(token.token, Token::Ident(_)) {
                    let name = &source[token.span.start..token.span.end];
                    elements.remove(name);
                    if builtins.contains(&name) { shadowed.insert(name.to_string()); }
                }
            }
        }
    }
    if shadowed.contains("Image") || shadowed.contains("Audio") || shadowed.contains("document") { elements.clear(); }
    let mut found = vec![];
    for i in 0..tokens.len() {
        if matches!(tokens[i].token, Token::Ident(_))
            && elements.get(&source[tokens[i].span.start..tokens[i].span.end]).is_some_and(|(start, scope)| *start < i && *scope == scopes[i])
            && (i == 0 || !(tokens[i - 1].token == "." || tokens[i - 1].token == "?."))
            && tokens.get(i + 1).is_some_and(|t| t.token == ".")
        {
            let assignment = tokens.get(i + 2).is_some_and(|t| t.token == "src")
                && tokens.get(i + 3).is_some_and(|t| t.token == "=")
                && tokens.get(i + 5).is_none_or(|t| t.token == ";" || t.token == ")" || t.token == "," || t.token == "}");
            let attribute = tokens.get(i + 2).is_some_and(|t| t.token == "setAttribute")
                && tokens.get(i + 3).is_some_and(|t| t.token == "(")
                && tokens.get(i + 4).is_some_and(|t| matches!(&t.token, Token::String(value) if value.as_ref().eq_ignore_ascii_case("src")))
                && tokens.get(i + 5).is_some_and(|t| t.token == ",")
                && tokens.get(i + 7).is_some_and(|t| t.token == ")");
            let item = if assignment { tokens.get(i + 4) } else if attribute { tokens.get(i + 6) } else { None };
            if let Some(item) = item && let Token::String(value) = &item.token {
                let value: &str = value.as_ref();
                if value.contains('\\') { bail!("escaped JavaScript media paths are not supported"); }
                found.push((item.span.start..item.span.end, value.to_string(), ReferenceKind::DomResource));
            }
        }
        if matches!(tokens[i].token, Token::Keyword(Keyword::New(_)))
            && !shadowed.contains("Audio") && tokens.get(i + 1).is_some_and(|t| t.token == "Audio")
            && tokens.get(i + 2).is_some_and(|t| t.token == "(")
            && let Some(item) = tokens.get(i + 3)
            && let Token::String(value) = &item.token
            && tokens.get(i + 4).is_some_and(|t| t.token == ")")
        {
            let value: &str = value.as_ref();
            if value.contains('\\') { bail!("escaped JavaScript media paths are not supported"); }
            found.push((item.span.start..item.span.end, value.to_string(), ReferenceKind::DomResource));
        }
        let global_fetch = if i == 0 || !(tokens[i - 1].token == "." || tokens[i - 1].token == "?.") { !shadowed.contains("fetch") }
            else if i >= 2 && (tokens[i - 2].token == "window" || tokens[i - 2].token == "globalThis" || tokens[i - 2].token == "self") {
                !shadowed.contains(&source[tokens[i - 2].span.start..tokens[i - 2].span.end])
                    && (i == 2 || !(tokens[i - 3].token == "." || tokens[i - 3].token == "?."))
            } else { false };
        if tokens[i].token == "fetch" && global_fetch
            && tokens.get(i + 1).is_some_and(|t| t.token == "(")
            && let Some(item) = tokens.get(i + 2)
            && let Token::String(value) = &item.token
            && tokens.get(i + 3).is_some_and(|t| t.token == ")" || t.token == ",")
        {
            let value: &str = value.as_ref();
            if value.contains('\\') { bail!("escaped JavaScript fetch paths are not supported"); }
            found.push((item.span.start..item.span.end, value.to_string(), ReferenceKind::Fetch));
        }
        if matches!(tokens[i].token, Token::Keyword(Keyword::New(_)))
            && tokens.get(i + 1).is_some_and(|t| t.token == "URL")
            && tokens.get(i + 2).is_some_and(|t| t.token == "(")
            && tokens.get(i + 4).is_some_and(|t| t.token == ",")
            && tokens.get(i + 5).is_some_and(|t| matches!(t.token, Token::Keyword(Keyword::Import(_))))
            && tokens.get(i + 6).is_some_and(|t| t.token == ".")
            && tokens.get(i + 7).is_some_and(|t| t.token == "meta")
            && tokens.get(i + 8).is_some_and(|t| t.token == ".")
            && tokens.get(i + 9).is_some_and(|t| t.token == "url")
            && tokens.get(i + 10).is_some_and(|t| t.token == ")")
            && let Some(item) = tokens.get(i + 3)
            && let Token::String(value) = &item.token
        {
            let value: &str = value.as_ref();
            if value.contains('\\') { bail!("escaped JavaScript resource paths are not supported"); }
            found.push((item.span.start..item.span.end, value.to_string(), ReferenceKind::Resource));
        }
        let import = matches!(tokens[i].token, Token::Keyword(Keyword::Import(_)));
        let export = matches!(tokens[i].token, Token::Keyword(Keyword::Export(_)));
        if !import && !export || i > 0 && (tokens[i - 1].token == "." || tokens[i - 1].token == "?.") { continue; }
        let next = i + 1;
        if next >= tokens.len() { continue; }
        let pos = if import && matches!(tokens[next].token, Token::String(_)) { Some(next) }
            else if import && tokens[next].token == "(" {
                (next + 1 < tokens.len() && matches!(tokens[next + 1].token, Token::String(_))
                    && tokens.get(next + 2).is_some_and(|t| t.token == ")" || t.token == ",")).then_some(next + 1)
            } else if (export || import && !matches!(tokens[next].token, Token::Ident(_))) && !(tokens[next].token == "{" || tokens[next].token == "*") { None }
            else {
                let mut pos = None;
                for j in next..tokens.len().min(next + 4096) {
                    if tokens[j].token == ";" || matches!(tokens[j].token, Token::Keyword(Keyword::Import(_) | Keyword::Export(_))) { break; }
                    if tokens[j].token == "from" && tokens.get(j + 1).is_some_and(|t| matches!(t.token, Token::String(_))) { pos = Some(j + 1); break; }
                }
                pos
            };
        if let Some(pos) = pos && let Token::String(value) = &tokens[pos].token {
            let value: &str = value.as_ref();
            if value.contains('\\') { bail!("escaped JavaScript module paths are not supported"); }
            found.push((tokens[pos].span.start..tokens[pos].span.end, value.to_string(), ReferenceKind::Module));
        }
    }
    Ok(found)
}

pub fn bundle_file(path: &Path, root: &Path, max_asset: u64) -> Result<Vec<u8>> {
    bundle_file_for_document(path, path.parent().context("script has no parent directory")?, root, max_asset, true)
}

pub fn bundle_file_for_document(path: &Path, base: &Path, root: &Path, max_asset: u64, module: bool) -> Result<Vec<u8>> {
    let root = root.canonicalize().context("script root is unavailable")?;
    let path = path.canonicalize().context("script is unavailable")?;
    let document_base = base.canonicalize().context("script base is unavailable")?;
    ensure!(document_base.starts_with(&root), "script base is outside the workspace");
    let mut bundler = Bundler { root, document_base, max_asset, total: 0, loading: HashSet::new(), cached: HashMap::new(), resources: HashMap::new() };
    bundler.file(&path, 0, module)
}

pub fn bundle_inline(source: &str, base: &Path, root: &Path, max_asset: u64) -> Result<String> {
    let root = root.canonicalize().context("script root is unavailable")?;
    let base = base.canonicalize().context("script base is unavailable")?;
    ensure!(base.starts_with(&root), "script base is outside the workspace");
    ensure!(source.len() as u64 <= max_asset, "inline script is too large");
    let mut bundler = Bundler { root, document_base: base.clone(), max_asset, total: 0, loading: HashSet::new(), cached: HashMap::new(), resources: HashMap::new() };
    bundler.rewrite(source, &base, 0)
}

struct Bundler { root: PathBuf, document_base: PathBuf, max_asset: u64, total: u64, loading: HashSet<PathBuf>, cached: HashMap<PathBuf, String>, resources: HashMap<PathBuf, String> }
impl Bundler {
    fn file(&mut self, path: &Path, depth: usize, module: bool) -> Result<Vec<u8>> {
        ensure!(depth <= MAX_DEPTH, "JavaScript imports are too deep");
        ensure!(path.starts_with(&self.root), "JavaScript dependency is outside the workspace");
        ensure!(self.loading.insert(path.to_path_buf()), "circular JavaScript imports are not supported");
        let result = self.file_inner(path, depth, module);
        self.loading.remove(path);
        result
    }
    fn file_inner(&mut self, path: &Path, depth: usize, module: bool) -> Result<Vec<u8>> {
        let file = std::fs::File::open(path).context("JavaScript dependency is missing")?;
        ensure!(file.metadata()?.is_file(), "JavaScript dependency is not a file");
        let mut bytes = vec![];
        file.take(self.max_asset.saturating_add(1)).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= self.max_asset, "JavaScript dependency is too large");
        self.total = self.total.checked_add(bytes.len() as u64).context("JavaScript dependencies are too large")?;
        ensure!(self.total <= MAX_TOTAL, "JavaScript dependencies are too large");
        if !module && !bytes.windows(5).any(|part| part == b"fetch" || part == b"Image" || part == b"Audio")
            && !bytes.windows(6).any(|part| part == b"import") && !bytes.windows(13).any(|part| part == b"createElement") { return Ok(bytes); }
        let text = String::from_utf8(bytes).context("JavaScript dependency is not UTF-8")?;
        let mut output = self.rewrite(&text, path.parent().context("script has no parent directory")?, depth)?;
        // Identical source in different files represents distinct ES modules.
        if module {
            let relative = path.strip_prefix(&self.root)?.to_str().context("JavaScript path is not Unicode")?.replace('\\', "/");
            let identity = relative.bytes().map(|byte| format!("%{byte:02X}")).collect::<String>();
            output.push_str(&format!("\n/* markdownx-module:{identity} */\n"));
        }
        ensure!(output.len() as u64 <= self.max_asset, "JavaScript is too large after embedding dependencies");
        Ok(output.into_bytes())
    }
    fn rewrite(&mut self, source: &str, base: &Path, depth: usize) -> Result<String> {
        let mut edits = vec![];
        let mut output_len = source.len();
        for (range, value, kind) in references(source)? {
            if kind == ReferenceKind::Module && !(value.starts_with("./") || value.starts_with("../")) { continue; }
            if kind != ReferenceKind::Module && (value.is_empty() || value.starts_with(['/', '#', '?']) || value.contains(':')) { continue; }
            let (path, suffix) = value.find(['?', '#']).map_or((value.as_str(), ""), |at| (&value[..at], &value[at..]));
            let decoded = crate::book::percent_decode_path(path).context("invalid JavaScript dependency")?;
            let dependency = if matches!(kind, ReferenceKind::Fetch | ReferenceKind::DomResource) { &self.document_base } else { base }.join(decoded).canonicalize().context("JavaScript dependency is missing")?;
            ensure!(dependency.starts_with(&self.root), "JavaScript dependency is outside the workspace");
            let data = if kind == ReferenceKind::Module {
                if let Some(data) = self.cached.get(&dependency) { data.clone() } else {
                    let bytes = self.file(&dependency, depth + 1, true)?;
                    let data = format!("data:text/javascript;base64,{}", STANDARD.encode(bytes));
                    self.cached.insert(dependency, data.clone());
                    data
                }
            } else if let Some(data) = self.resources.get(&dependency) { data.clone() } else {
                let file = std::fs::File::open(&dependency).context("JavaScript resource is missing")?;
                ensure!(file.metadata()?.is_file(), "JavaScript resource is not a file");
                let mut bytes = vec![];
                file.take(self.max_asset.saturating_add(1)).read_to_end(&mut bytes)?;
                ensure!(bytes.len() as u64 <= self.max_asset, "JavaScript resource is too large");
                self.total = self.total.checked_add(bytes.len() as u64).context("JavaScript dependencies are too large")?;
                ensure!(self.total <= MAX_TOTAL, "JavaScript dependencies are too large");
                let ext = dependency.extension().and_then(|e| e.to_str()).unwrap_or("");
                let data = format!("data:{};base64,{}", crate::book::mime_of(ext), STANDARD.encode(bytes));
                self.resources.insert(dependency, data.clone());
                data
            };
            // Data URL queries corrupt base64. Modules keep a unique identity
            // in the fragment; assets retain SVG fragments and ignore queries.
            let suffix = if kind != ReferenceKind::Module {
                suffix.find('#').map_or_else(String::new, |at| suffix[at..].to_string())
            } else { crate::html_assets::module_identity(suffix) };
            let replacement = serde_json::to_string(&format!("{data}{suffix}"))?;
            output_len = output_len.checked_sub(range.len()).and_then(|n| n.checked_add(replacement.len())).context("JavaScript is too large")?;
            ensure!(output_len as u64 <= self.max_asset, "JavaScript is too large after embedding dependencies");
            edits.push((range, replacement));
        }
        let mut output = source.to_owned();
        for (range, replacement) in edits.into_iter().rev() { output.replace_range(range, &replacement); }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_ignore_literals_comments_and_property_names() {
        let source = r#"// import './fake.js'
const text = "import './fake.js'"; const regex = /import\s+['\"]fake/;
if (true) /import\s+['\"]fake/.test(text);
obj.import('./ignored.js'); const object = {import: 1, from: './ignored.js'}; import './a.js';
import { n } from '../b.js'; export * from './c.js';
async function f() { return import('./d.js'); }
const t = `literal import './fake.js' ${import('./e.js')}`;
const art = new URL('frames/01.svg', import.meta.url);
const art2 = new URL('./frames/02.svg', import.meta.url);
const other = new URL('./untouched.svg', document.baseURI);"#;
        let values = references(source).unwrap().into_iter().map(|(_, s, k)| (s, k)).collect::<Vec<_>>();
        assert_eq!(values, [("./a.js".into(), ReferenceKind::Module), ("../b.js".into(), ReferenceKind::Module), ("./c.js".into(), ReferenceKind::Module), ("./d.js".into(), ReferenceKind::Module), ("./e.js".into(), ReferenceKind::Module), ("frames/01.svg".into(), ReferenceKind::Resource), ("./frames/02.svg".into(), ReferenceKind::Resource)]);
    }
    #[test]
    fn embeds_module_relative_resources_without_leaving_root() {
        let root = std::env::temp_dir().join(format!("markdownx-js-resource-{}", uuid::Uuid::new_v4()));
        let base = root.join("chapter");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("frame.svg"), "wrong-root").unwrap();
        std::fs::write(base.join("frame.svg"), "<svg/>").unwrap();
        let source = "const a=new URL('frame.svg',import.meta.url); const b=new URL('./frame.svg?v=1#icon',import.meta.url);";
        let output = bundle_inline(source, &base, &root, 1024 * 1024).unwrap();
        assert_eq!(output.matches("data:image/svg+xml;base64,PHN2Zy8+").count(), 2, "{output}");
        assert!(output.contains("#icon") && !output.contains("?v=1"));
        let outside = root.with_extension("private.svg");
        std::fs::write(&outside, "private").unwrap();
        let name = outside.file_name().unwrap().to_string_lossy();
        let source = format!("new URL('../../{name}', import.meta.url)");
        assert!(bundle_inline(&source, &base, &root, 1024 * 1024).unwrap_err().to_string().contains("outside"));
        assert!(bundle_inline("new URL('./missing.svg', import.meta.url)", &base, &root, 1024 * 1024).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fetches_use_document_directory_and_keep_network_and_inert_text() {
        let root = std::env::temp_dir().join(format!("markdownx-js-fetch-{}", uuid::Uuid::new_v4()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(root.join("data.json"), "{\"place\":\"document\"}").unwrap();
        std::fs::write(scripts.join("data.json"), "wrong").unwrap();
        let source = "// fetch('missing.json')\nconst text=\"fetch('missing.json')\"; obj.fetch('missing.json'); fetch('./data.json').then(r=>r.json()); window.fetch('data.json'); fetch('https://example.invalid/a');";
        let file = scripts.join("game.js");
        std::fs::write(&file, source).unwrap();
        let result = String::from_utf8(bundle_file_for_document(&file, &root, &root, 1024 * 1024, false).unwrap()).unwrap();
        let encoded = STANDARD.encode("{\"place\":\"document\"}");
        assert_eq!(result.matches(&encoded).count(), 2, "{result}");
        assert!(result.contains("obj.fetch('missing.json')") && result.contains("fetch('https://example.invalid/a')"));
        assert_eq!(std::fs::read_to_string(file).unwrap(), source);
        let outside = root.with_extension("private.json");
        std::fs::write(&outside, "private").unwrap();
        let attempt = format!("fetch('../{}')", outside.file_name().unwrap().to_string_lossy());
        assert!(bundle_inline(&attempt, &root, &root, 1024 * 1024).unwrap_err().to_string().contains("outside"));
        assert!(bundle_inline("fetch('./missing.json')", &root, &root, 1024 * 1024).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundles_literal_dom_media_from_document_not_script_directory() {
        let root = std::env::temp_dir().join(format!("markdownx-js-dom-{}", uuid::Uuid::new_v4()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(root.join("frame.svg"), "<svg/>").unwrap();
        std::fs::write(scripts.join("frame.svg"), "wrong").unwrap();
        std::fs::write(root.join("sound.mp3"), "sound").unwrap();
        let source = r#"const img = new Image(); img.src = './frame.svg';
const second = document.createElement('img'); second.setAttribute('src', 'frame.svg');
const sound = new Audio('sound.mp3'); const ignored = {}; ignored.src = './missing.svg';
const text = "img.src = './missing.svg'"; // img.src = './missing.svg'
function shadow(img) { img.src = './unrelated.svg'; }
img.src = './' + name;"#;
        let file = scripts.join("game.js");
        std::fs::write(&file, source).unwrap();
        let output = String::from_utf8(bundle_file_for_document(&file, &root, &root, 1024 * 1024, false).unwrap()).unwrap();
        assert_eq!(output.matches("data:image/svg+xml;base64,PHN2Zy8+").count(), 2, "{output}");
        assert!(output.contains("data:audio/mpeg;base64,c291bmQ="));
        assert!(output.contains("ignored.src = './missing.svg'") && output.contains("img.src = './' + name") && output.contains("img.src = './unrelated.svg'"));
        assert!(references("const img=new Image(); img.src='frame.svg'; [1].map((img, i)=>img.src='unrelated.svg');").unwrap().is_empty());
        let local = "const fetch=x=>x; fetch('missing.json'); window.fetch('data.json'); class Image {} const img=new Image(); img.src='missing.svg';";
        assert_eq!(references(local).unwrap().into_iter().map(|(_, value, kind)| (value, kind)).collect::<Vec<_>>(), [("data.json".into(), ReferenceKind::Fetch)]);
        let semicolon_free = "import {value} from './helper.mjs'\nfetch('./data.json')";
        assert_eq!(references(semicolon_free).unwrap().iter().map(|(_, value, kind)| (value.as_str(), *kind)).collect::<Vec<_>>(), [("./helper.mjs", ReferenceKind::Module), ("./data.json", ReferenceKind::Fetch)]);
        let parameters = "function f({fetch}) { fetch('missing.json'); } window.fetch('data.json'); const run=({Image})=>{const img=new Image();img.src='missing.svg';};";
        assert_eq!(references(parameters).unwrap().into_iter().map(|(_, value, kind)| (value, kind)).collect::<Vec<_>>(), [("data.json".into(), ReferenceKind::Fetch)]);
        let imported = "import Image from './image.mjs'; const img=new Image(); img.src='missing.svg';";
        assert_eq!(references(imported).unwrap().into_iter().map(|(_, value, kind)| (value, kind)).collect::<Vec<_>>(), [("./image.mjs".into(), ReferenceKind::Module)]);
        assert_eq!(references("const img=document.createElement('IMG');img.setAttribute('SRC','frame.svg');").unwrap().into_iter().map(|(_, value, kind)| (value, kind)).collect::<Vec<_>>(), [("frame.svg".into(), ReferenceKind::DomResource)]);
        assert!(references("const img=new Image().other; img.src='missing.svg';").unwrap().is_empty());
        assert_eq!(references("const img=new Image()\nimg.src='frame.svg'").unwrap().len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
        let outside = root.with_extension("svg");
        std::fs::write(&outside, "private").unwrap();
        let attempt = format!("const img=new Image(); img.src='../{}';", outside.file_name().unwrap().to_string_lossy());
        assert!(bundle_inline(&attempt, &root, &root, 1024 * 1024).unwrap_err().to_string().contains("outside"));
        assert!(bundle_inline("const img=new Image(); img.src='missing.svg';", &root, &root, 1024 * 1024).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn classic_script_literal_dynamic_import_is_bundled_beside_script() {
        let root = std::env::temp_dir().join(format!("markdownx-classic-import-{}", uuid::Uuid::new_v4()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("helper.mjs"), "export const n=42;").unwrap();
        let script = scripts.join("game.js");
        std::fs::write(&script, "import('./helper.mjs').then(m=>console.log(m.n));").unwrap();
        let result = String::from_utf8(bundle_file_for_document(&script, &root, &root, 1024 * 1024, false).unwrap()).unwrap();
        assert!(result.contains("data:text/javascript;base64,"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundles_nested_imports_and_rejects_cycles_and_escape() {
        let root = std::env::temp_dir().join(format!("markdownx-js-{}", uuid::Uuid::new_v4()));
        let base = root.join("chapter");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("helper.js"), "export const n = 42;").unwrap();
        std::fs::write(base.join("game.js"), "import {n} from '../helper.js?v=1'; console.log(n);").unwrap();
        let output = String::from_utf8(bundle_file(&base.join("game.js"), &root, 1024 * 1024).unwrap()).unwrap();
        assert!(output.contains("data:text/javascript;base64,") && output.contains("#%3F%76%3D%31"));
        std::fs::write(root.join("helper.js"), "import './chapter/game.js';").unwrap();
        assert!(bundle_file(&base.join("game.js"), &root, 1024 * 1024).unwrap_err().to_string().contains("circular"));
        let outside = root.with_extension("private.js");
        std::fs::write(&outside, "export const secret = 1;").unwrap();
        let name = outside.file_name().unwrap().to_string_lossy();
        let inline = format!("import '../../{name}';");
        assert!(bundle_inline(&inline, &base, &root, 1024 * 1024).unwrap_err().to_string().contains("outside"));
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
