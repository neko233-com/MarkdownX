//! Wiki-style links between Markdown files.
//!
//! A link is written `[[Target]]` or `[[Target|Label]]`. Targets are resolved
//! against the file stem, the relative path without extension, or the first
//! `# heading` of a file. The index is transport-free and only reads text, so
//! the same code serves the folder workspace, the CLI and future Web builds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One `[[...]]` occurrence in a Markdown source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub label: String,
    /// Zero-based source line.
    pub line: usize,
    /// Byte range of the whole `[[...]]` token.
    pub start: usize,
    pub end: usize,
}

impl WikiLink {
    pub fn display(&self) -> &str {
        if self.label.is_empty() { &self.target } else { &self.label }
    }
}

/// Links found in one file.
#[derive(Clone, Debug, Default)]
pub struct FileLinks {
    pub path: PathBuf,
    pub title: String,
    pub links: Vec<WikiLink>,
}

impl FileLinks {
    pub fn new(path: PathBuf, text: &str) -> Self {
        Self { title: title_of(&path, text), path, links: links(text) }
    }
}

/// All files in a folder, with backlink lookup.
#[derive(Clone, Debug, Default)]
pub struct Index {
    files: Vec<FileLinks>,
}

impl Index {
    pub fn build(files: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        let mut out: Vec<FileLinks> = files.into_iter().map(|(p, t)| FileLinks::new(p, &t)).collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Self { files: out }
    }

    pub fn files(&self) -> &[FileLinks] {
        &self.files
    }

    pub fn get(&self, path: &Path) -> Option<&FileLinks> {
        self.files.iter().find(|f| same_path(&f.path, path))
    }

    /// Files whose `[[...]]` resolves to `path`.
    pub fn backlinks(&self, path: &Path) -> Vec<(&FileLinks, &WikiLink)> {
        let mut out = vec![];
        for file in &self.files {
            if same_path(&file.path, path) {
                continue;
            }
            for link in &file.links {
                if self.resolve(&file.path, &link.target).is_some_and(|p| same_path(p, path)) {
                    out.push((file, link));
                }
            }
        }
        out
    }

    /// Resolve a target from `from`. Ambiguity resolves to the first path in
    /// sorted order; the caller can show all candidates if it wants to.
    pub fn resolve(&self, from: &Path, target: &str) -> Option<&Path> {
        let key = normalize(target);
        if key.is_empty() {
            return None;
        }
        let from_dir = from.parent();
        let mut fallback: Option<&Path> = None;
        for file in &self.files {
            let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            let rel = from_dir.and_then(|d| file.path.strip_prefix(d).ok()).unwrap_or(&file.path);
            let rel_no_ext = rel.with_extension("");
            let rel_key = normalize(&rel_no_ext.to_string_lossy().replace('\\', "/"));
            let title_key = normalize(&file.title);
            let stem_key = normalize(stem);
            if key == rel_key || key == stem_key || key == title_key {
                return Some(&file.path);
            }
            if fallback.is_none() && (stem_key.ends_with(&key) || title_key.contains(&key)) {
                fallback = Some(&file.path);
            }
        }
        fallback
    }

    pub fn unresolved_count(&self, from: &Path) -> usize {
        self.get(from).map_or(0, |f| f.links.iter().filter(|l| self.resolve(from, &l.target).is_none()).count())
    }
}

/// Parse `[[Target]]` / `[[Target|Label]]`, skipping fenced code blocks.
pub fn links(text: &str) -> Vec<WikiLink> {
    let mut out = vec![];
    let mut offset = 0;
    let mut fenced = false;
    for (line_no, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            offset += raw.len();
            continue;
        }
        if fenced {
            offset += raw.len();
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i + 3 < bytes.len() {
            if bytes[i] == b'['
                && bytes[i + 1] == b'['
                && let Some(rel_end) = line[i + 2..].find("]]")
            {
                let end = i + 2 + rel_end;
                let inner = &line[i + 2..end];
                if valid_inner(inner) {
                    let (target, label) = split_inner(inner);
                    out.push(WikiLink {
                        target,
                        label,
                        line: line_no,
                        start: offset + i,
                        end: offset + end + 2,
                    });
                    i = end + 2;
                    continue;
                }
            }
            i += 1;
        }
        offset += raw.len();
    }
    out
}

fn valid_inner(inner: &str) -> bool {
    let inner = inner.trim();
    !inner.is_empty() && inner.len() <= 240 && !inner.contains('\n') && !inner.contains("[[")
}

fn split_inner(inner: &str) -> (String, String) {
    let inner = inner.trim();
    match inner.split_once('|') {
        Some((t, l)) => (t.trim().to_string(), l.trim().to_string()),
        None => (inner.to_string(), String::new()),
    }
}

/// Target form used for comparison: case, `.md`, spaces and backslashes folded.
pub fn normalize(target: &str) -> String {
    let mut s = target.trim().replace('\\', "/").to_lowercase();
    if let Some(stripped) = s.strip_suffix(".md") {
        s = stripped.to_string();
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim_matches('/').to_string()
}

/// First ATX heading, falling back to the file stem.
pub fn title_of(path: &Path, text: &str) -> String {
    for line in text.lines().take(200) {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# ")
            && !rest.trim().is_empty()
        {
            return rest.trim().to_string();
        }
    }
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string()
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

/// A folder-level view used by the desktop sidebar and the agent CLI.
pub fn build(files: impl IntoIterator<Item = (PathBuf, String)>) -> Index {
    Index::build(files)
}

/// Count links by target for a compact “most linked” summary.
pub fn target_counts(index: &Index) -> Vec<(String, usize)> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for f in index.files() {
        for l in &f.links {
            *m.entry(l.target.clone()).or_default() += 1;
        }
    }
    let mut out: Vec<_> = m.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn parses_links_aliases_and_lines() {
        let text = "intro\n\nsee [[Lesson]] and [[other|The Other]]\n";
        let l = links(text);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].target, "Lesson");
        assert_eq!(l[0].line, 2);
        assert_eq!(l[1].label, "The Other");
    }

    #[test]
    fn skips_fenced_and_incomplete_links() {
        let text = "```\n[[No]]\n```\n[[Yes]]\n[[broken]\n[[ ]]\n";
        let l = links(text);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].target, "Yes");
    }

    #[test]
    fn resolves_stem_path_and_title() {
        let idx = Index::build([
            (p("a/one.md"), "# First Lesson\nsee [[two]]".into()),
            (p("a/two.md"), "# Second\n".into()),
        ]);
        assert_eq!(idx.resolve(&p("a/one.md"), "two"), Some(p("a/two.md").as_path()));
        assert_eq!(idx.resolve(&p("a/one.md"), "Second"), Some(p("a/two.md").as_path()));
        assert_eq!(idx.resolve(&p("a/one.md"), "missing"), None);
    }

    #[test]
    fn backlinks_are_file_level() {
        let idx = Index::build([
            (p("one.md"), "# One\n\n[[Two]]\n".into()),
            (p("two.md"), "# Two\n\n[[One]]\n".into()),
            (p("three.md"), "# Three\n".into()),
        ]);
        let b = idx.backlinks(&p("two.md"));
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].0.path, p("one.md"));
        assert_eq!(b[0].1.target, "Two");
    }

    #[test]
    fn title_and_normalize() {
        assert_eq!(title_of(&p("x.md"), "no heading"), "x");
        assert_eq!(title_of(&p("x.md"), "# Real\nbody"), "Real");
        assert_eq!(normalize(" A/B.MD "), "a/b");
        assert_eq!(normalize("hello   world"), "hello world");
    }
}
