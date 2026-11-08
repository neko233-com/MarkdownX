//! A normal folder as a workspace, the way Typora opens one.
//!
//! The library still owns pages and `.mdxb` books; this module only reads a
//! directory tree and finds Markdown files. Editing and saving live in
//! `app.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    /// Directories sort before files and never open in the editor.
    pub dir: bool,
    pub children: Vec<FileNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDir {
    pub root: PathBuf,
    pub name: String,
    pub children: Vec<FileNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Markdown,
    Book,
}

impl DocKind {
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "md" | "markdown" | "mark" | "txt" => Some(DocKind::Markdown),
            "mdxb" => Some(DocKind::Book),
            _ => None,
        }
    }
}

impl WorkspaceDir {
    pub fn scan(root: PathBuf) -> anyhow::Result<Self> {
        if !root.is_dir() {
            anyhow::bail!("not a folder: {}", root.display());
        }
        let name = root.file_name().and_then(|s| s.to_str()).unwrap_or("Workspace").to_string();
        let children = scan_dir(&root, 0)?;
        Ok(WorkspaceDir { root, name, children })
    }

    pub fn files(&self) -> Vec<&FileNode> {
        let mut out = vec![];
        collect_files(&self.children, &mut out);
        out
    }
}

fn scan_dir(dir: &Path, depth: usize) -> anyhow::Result<Vec<FileNode>> {
    if depth > 12 {
        return Ok(vec![]);
    }
    let mut dirs = vec![];
    let mut files = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "__pycache__") {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            let children = scan_dir(&path, depth + 1)?;
            // Keep a folder only when something inside can be opened.
            if !children.is_empty() {
                dirs.push(FileNode { path, name, dir: true, children });
            }
        } else if ty.is_file() && DocKind::of(&path).is_some() {
            files.push(FileNode { path, name, dir: false, children: vec![] });
        }
    }
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
    dirs.extend(files);
    Ok(dirs)
}

/// Write a file without truncating the old one until the new bytes are on disk.
/// A crash leaves either the previous file or the new one, never a half file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document.md");
    let tmp = parent.join(format!(".{}.markdownx-{}.tmp", name, std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn collect_files<'a>(nodes: &'a [FileNode], out: &mut Vec<&'a FileNode>) {
    for n in nodes {
        if n.dir {
            collect_files(&n.children, out);
        } else {
            out.push(n);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub rel: String,
    pub score: i64,
    /// A line around a content match, already trimmed to a readable length.
    pub snippet: Option<String>,
}

impl WorkspaceDir {
    /// Fuzzy path search, optionally reading file bodies. Folders are not
    /// results; a file's path is part of the needle.
    pub fn search(&self, query: &str, content: bool) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let mut hits = vec![];
        for file in self.files() {
            let rel = file.path.strip_prefix(&self.root).unwrap_or(&file.path).to_string_lossy().replace('\\', "/");
            let path_score = fuzzy_score(&q, &rel.to_lowercase());
            let mut body_match = None;
            if content
                && let Ok(bytes) = fs::read(&file.path)
            {
                let text = String::from_utf8_lossy(&bytes);
                if let Some(at) = text.to_lowercase().find(&q) {
                    body_match = Some(snippet(&text, at, q.len()));
                }
            }
            if let Some(s) = path_score {
                hits.push(Hit { path: file.path.clone(), rel: rel.clone(), score: s + 20, snippet: body_match });
            } else if let Some(sn) = body_match {
                hits.push(Hit { path: file.path.clone(), rel, score: 1, snippet: Some(sn) });
            }
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.rel.cmp(&b.rel)));
        hits.truncate(200);
        hits
    }
}

fn snippet(text: &str, at: usize, len: usize) -> String {
    let start = at.saturating_sub(36);
    let end = (at + len + 80).min(text.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&text[start..end].replace(['\n', '\r'], " "));
    out.push('…');
    out
}

/// A small subsequence scorer: every needle character must appear in order.
/// Matches at a word boundary or next to the previous character score higher.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<i64> {
    let mut score = 0i64;
    let mut from = 0usize;
    let mut prev: Option<usize> = None;
    for (i, ch) in needle.chars().enumerate() {
        let rest = haystack.get(from..)?;
        let at = rest.find(ch)?;
        let pos = from + at;
        score += 10;
        if pos == 0 || matches!(haystack.as_bytes().get(pos.wrapping_sub(1)), Some(b'/' | b'\\' | b' ' | b'-' | b'_' | b'.')) {
            score += 40;
        }
        if prev == Some(pos.saturating_sub(1)) {
            score += 20;
        }
        if i == 0 && pos == 0 {
            score += 30;
        }
        prev = Some(pos);
        from = pos + ch.len_utf8();
    }
    // Prefer shorter paths and earlier first matches.
    score -= haystack.chars().count() as i64 / 4;
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("markdownx-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_markdown_and_books_but_skips_other_files() {
        let dir = temp("scan");
        fs::create_dir_all(dir.join("chapters")).unwrap();
        fs::write(dir.join("readme.md"), "hello").unwrap();
        fs::write(dir.join("chapters/one.mdxb"), "{}").unwrap();
        fs::write(dir.join("cover.png"), "not a doc").unwrap();
        let ws = WorkspaceDir::scan(dir.clone()).unwrap();
        let names: Vec<&str> = ws.files().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["one.mdxb", "readme.md"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fuzzy_finds_path_and_content() {
        let dir = temp("search");
        fs::create_dir_all(dir.join("physics")).unwrap();
        fs::write(dir.join("physics/forces.md"), "牛顿第一定律与惯性").unwrap();
        let ws = WorkspaceDir::scan(dir.clone()).unwrap();
        assert_eq!(ws.search("forces", false)[0].rel, "physics/forces.md");
        let hit = &ws.search("惯性", true)[0];
        assert!(hit.snippet.as_deref().unwrap_or("").contains("惯性"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scorer_prefers_boundaries() {
        let a = fuzzy_score("for", "physics/forces.md").unwrap();
        let b = fuzzy_score("for", "a/effort.md").unwrap();
        assert!(a > b);
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = temp("atomic");
        let path = dir.join("note.md");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, "new".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".tmp")).collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
