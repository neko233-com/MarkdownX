//! A normal folder as a workspace, the way Typora opens one.
//!
//! The library still owns pages and `.mdxb` books; this module only reads a
//! directory tree and finds Markdown files. Editing and saving live in
//! `app.rs`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub const DEFAULT_WORKSPACE_DIR: &str = "MarkdownX-Demo";

fn initially_expanded() -> bool { true }

/// A registered folder; its label never changes the underlying directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "initially_expanded")]
    pub expanded: bool,
}

impl WorkspaceSpec {
    pub fn new(path: PathBuf) -> Self { Self { path, label: None, expanded: true } }
    pub fn name(&self) -> String {
        self.label.as_ref().filter(|s| !s.trim().is_empty()).cloned().unwrap_or_else(|| self.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.display().to_string()))
    }
}

/// The platform's Documents folder, including known-folder redirection.
pub fn documents_dir() -> Option<PathBuf> {
    dirs::document_dir().or_else(|| dirs::home_dir().map(|home| home.join("Documents")))
}

/// Restore an explicit folder as-is; only create the default when none was chosen.
pub fn initial_workspace(saved: Option<PathBuf>, documents: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = saved {
        anyhow::ensure!(path.is_absolute(), "workspace path must be absolute");
        return Ok(path);
    }
    let documents = documents.ok_or_else(|| anyhow::anyhow!("system Documents folder is unavailable"))?;
    anyhow::ensure!(documents.is_absolute(), "Documents path must be absolute");
    let root = documents.join(DEFAULT_WORKSPACE_DIR);
    fs::create_dir_all(&root)?;
    Ok(root)
}

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
    /// Inaccessible entries do not hide the rest of the workspace.
    pub warnings: Vec<(PathBuf, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Markdown,
    DocxXml,
    Book,
}

impl DocKind {
    pub fn of(path: &Path) -> Option<Self> {
        match fdl_core::extensions::builtin_editor(path)? {
            fdl_core::extensions::BuiltinEditor::Markdown => Some(DocKind::Markdown),
            fdl_core::extensions::BuiltinEditor::DocxXml => Some(DocKind::DocxXml),
            fdl_core::extensions::BuiltinEditor::Book => Some(DocKind::Book),
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
        let (children, warnings) = scan_dir(&root)?;
        Ok(WorkspaceDir { root, name, children, warnings })
    }

    pub fn files(&self) -> Vec<&FileNode> {
        self.all_files().into_iter().filter(|node| DocKind::of(&node.path).is_some()).collect()
    }

    /// All visible regular files; `files` remains the editable-document set.
    pub fn all_files(&self) -> Vec<&FileNode> {
        let mut out = vec![];
        collect_files(&self.children, &mut out);
        out
    }
}

fn scan_dir(root: &Path) -> anyhow::Result<(Vec<FileNode>, Vec<(PathBuf, String)>)> {
    // Build bottom-up, without an arbitrary depth limit or following symlinks.
    let mut pending = vec![(root.to_path_buf(), None)];
    let mut nodes: Vec<Option<(Option<usize>, FileNode)>> = vec![];
    let mut warnings = vec![];
    while let Some((dir, parent)) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if parent.is_none() => return Err(e.into()),
            Err(e) => { warnings.push((dir, e.to_string())); continue; }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => { warnings.push((dir.clone(), e.to_string())); continue; }
            };
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "__pycache__") {
                continue;
            }
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) => { warnings.push((path, e.to_string())); continue; }
            };
            if meta.file_type().is_symlink() { continue; }
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 { continue; }
            }
            let ty = match entry.file_type() {
                Ok(ty) => ty,
                Err(e) => { warnings.push((path, e.to_string())); continue; }
            };
            if ty.is_dir() {
                let index = nodes.len();
                nodes.push(Some((parent, FileNode { path: path.clone(), name, dir: true, children: vec![] })));
                pending.push((path, Some(index)));
            } else if ty.is_file() {
                nodes.push(Some((parent, FileNode { path, name, dir: false, children: vec![] })));
            }
        }
    }
    let mut children = vec![];
    for i in (0..nodes.len()).rev() {
        let (parent, mut node) = nodes[i].take().unwrap();
        sort_nodes(&mut node.children);
        if let Some(parent) = parent {
            nodes[parent].as_mut().unwrap().1.children.push(node);
        } else {
            children.push(node);
        }
    }
    sort_nodes(&mut children);
    Ok((children, warnings))
}

fn sort_nodes(nodes: &mut [FileNode]) {
    nodes.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
}

#[derive(Debug)]
pub enum SaveFileError {
    Changed,
    Lossy,
    MissingAssets(usize),
    AssetConflict(PathBuf),
    Io(std::io::Error),
}

/// Stage a unique sibling file, then validate again before replacing the source.
fn write_atomic_checked(path: &Path, bytes: &[u8], validate: impl FnOnce() -> Result<(), SaveFileError>) -> Result<(), SaveFileError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document.md");
    let tmp = parent.join(format!(".{}.markdownx-{}.tmp", name, uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(SaveFileError::Io)?;
        f.write_all(bytes).map_err(SaveFileError::Io)?;
        f.sync_all().map_err(SaveFileError::Io)?;
        drop(f);
        validate()?;
        fs::rename(&tmp, path).map_err(SaveFileError::Io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match write_atomic_checked(path, bytes, || Ok(())) {
        Ok(()) => Ok(()),
        Err(SaveFileError::Io(error)) => Err(error),
        Err(_) => unreachable!("an unconditional write cannot report a conflict"),
    }
}

/// Refuse to replace a file that changed since it was opened or last saved.
pub fn write_if_unchanged(path: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), SaveFileError> {
    let current = fs::read(path).map_err(SaveFileError::Io)?;
    if current != expected { return Err(SaveFileError::Changed); }
    write_atomic_checked(path, bytes, || {
        if fs::read(path).map_err(SaveFileError::Io)? == expected { Ok(()) } else { Err(SaveFileError::Changed) }
    })
}

/// Spreadsheet saves refuse links and read-only sources in addition to checking
/// the byte snapshot. Both checks are repeated immediately before replacement.
pub fn write_regular_if_unchanged(path: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), SaveFileError> {
    fn check(path: &Path, expected: &[u8]) -> Result<(), SaveFileError> {
        let meta = fs::symlink_metadata(path).map_err(SaveFileError::Io)?;
        #[cfg(windows)]
        let reparse = { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 };
        #[cfg(not(windows))]
        let reparse = false;
        if !meta.is_file() || meta.file_type().is_symlink() || reparse || meta.permissions().readonly() {
            return Err(SaveFileError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "source is not a writable regular file")));
        }
        if fs::read(path).map_err(SaveFileError::Io)? != expected { return Err(SaveFileError::Changed); }
        Ok(())
    }
    check(path, expected)?;
    write_atomic_checked(path, bytes, || check(path, expected))
}

/// Copy assets before publishing the Markdown, without replacing an existing file.
pub fn write_export_if_unchanged(path: &Path, expected: &[u8], bytes: &[u8], assets: &[(PathBuf, String)]) -> Result<Vec<PathBuf>, SaveFileError> {
    if assets.is_empty() { write_if_unchanged(path, expected, bytes)?; return Ok(vec![]); }
    if fs::read(path).map_err(SaveFileError::Io)? != expected { return Err(SaveFileError::Changed); }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let root = parent.canonicalize().map_err(SaveFileError::Io)?;
    let mut destinations = vec![];
    for (from, rel) in assets {
        fs::File::open(from).map_err(SaveFileError::Io)?;
        let relative = Path::new(rel);
        if relative.as_os_str().is_empty() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(SaveFileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid asset path")));
        }
        let to = parent.join(relative);
        let dir = to.parent().unwrap_or(parent);
        fs::create_dir_all(dir).map_err(SaveFileError::Io)?;
        if !dir.canonicalize().map_err(SaveFileError::Io)?.starts_with(&root) {
            return Err(SaveFileError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "asset folder is outside the document folder")));
        }
        if to.try_exists().map_err(SaveFileError::Io)?
            && (!to.canonicalize().map_err(SaveFileError::Io)?.starts_with(&root) || !same_contents(from, &to).map_err(SaveFileError::Io)?)
        {
            return Err(SaveFileError::AssetConflict(to));
        }
        destinations.push((from, to));
    }
    let mut copied = vec![];
    for (from, to) in destinations {
        if to.try_exists().map_err(SaveFileError::Io)? {
            if !to.canonicalize().map_err(SaveFileError::Io)?.starts_with(&root) || !same_contents(from, &to).map_err(SaveFileError::Io)? {
                return Err(SaveFileError::AssetConflict(to));
            }
            continue;
        }
        let mut source = fs::File::open(from).map_err(SaveFileError::Io)?;
        let mut target = fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(SaveFileError::Io)?;
        if let Err(error) = std::io::copy(&mut source, &mut target).and_then(|_| target.sync_all()) {
            drop(target);
            let _ = fs::remove_file(&to);
            return Err(SaveFileError::Io(error));
        }
        copied.push(to);
    }
    write_if_unchanged(path, expected, bytes)?;
    Ok(copied)
}

pub(crate) fn same_contents(a: &Path, b: &Path) -> std::io::Result<bool> {
    if a.canonicalize()? == b.canonicalize()? { return Ok(true); }
    let mut a = fs::File::open(a)?;
    let mut b = fs::File::open(b)?;
    if a.metadata()?.len() != b.metadata()?.len() { return Ok(false); }
    let mut left = [0u8; 8192];
    let mut right = [0u8; 8192];
    loop {
        let count = a.read(&mut left)?;
        if count == 0 { return Ok(true); }
        b.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] { return Ok(false); }
    }
}

fn collect_files<'a>(nodes: &'a [FileNode], out: &mut Vec<&'a FileNode>) {
    let mut pending: Vec<_> = nodes.iter().rev().collect();
    while let Some(n) = pending.pop() {
        if n.dir {
            pending.extend(n.children.iter().rev());
        } else {
            out.push(n);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub root: PathBuf,
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
        let q: String = query.trim().chars().flat_map(char::to_lowercase).collect();
        if q.is_empty() {
            return vec![];
        }
        let mut hits = vec![];
        for file in self.all_files() {
            let rel = file.path.strip_prefix(&self.root).unwrap_or(&file.path).to_string_lossy().replace('\\', "/");
            let path_score = fuzzy_score(&q, &rel.to_lowercase());
            let mut body_match = None;
            if content && DocKind::of(&file.path).is_some()
                && let Ok(bytes) = fs::read(&file.path)
            {
                let text = String::from_utf8_lossy(&bytes);
                if let Some((start, end)) = find_case_insensitive(&text, &q) {
                    body_match = Some(snippet(&text, start, end));
                }
            }
            if let Some(s) = path_score {
                hits.push(Hit { root: self.root.clone(), path: file.path.clone(), rel: rel.clone(), score: s + 20, snippet: body_match });
            } else if let Some(sn) = body_match {
                hits.push(Hit { root: self.root.clone(), path: file.path.clone(), rel, score: 1, snippet: Some(sn) });
            }
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.rel.cmp(&b.rel)));
        hits.truncate(200);
        hits
    }
}

fn find_case_insensitive(text: &str, needle: &str) -> Option<(usize, usize)> {
    let lower: String = text.chars().flat_map(char::to_lowercase).collect();
    let at = lower.find(needle)?;
    let end = at + needle.len();
    let mut offset = 0;
    let mut start = None;
    for (i, ch) in text.char_indices() {
        let next = offset + ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        if start.is_none() && next > at { start = Some(i); }
        if next >= end { return Some((start?, i + ch.len_utf8())); }
        offset = next;
    }
    None
}

fn snippet(text: &str, at: usize, end: usize) -> String {
    let mut start = at.saturating_sub(36);
    while !text.is_char_boundary(start) { start += 1; }
    let mut end = (end + 80).min(text.len());
    while !text.is_char_boundary(end) { end -= 1; }
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&text[start..end].replace(['\n', '\r'], " "));
    if end < text.len() { out.push('…'); }
    out
}

/// A small subsequence scorer: every needle character must appear in order.
/// Matches at a word boundary or next to the previous character score higher.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<i64> {
    let mut score = 0i64;
    let mut from = 0usize;
    let mut prev_end: Option<usize> = None;
    for (i, ch) in needle.chars().enumerate() {
        let rest = haystack.get(from..)?;
        let at = rest.find(ch)?;
        let pos = from + at;
        score += 10;
        if pos == 0 || matches!(haystack.as_bytes().get(pos.wrapping_sub(1)), Some(b'/' | b'\\' | b' ' | b'-' | b'_' | b'.')) {
            score += 40;
        }
        if prev_end == Some(pos) {
            score += 20;
        }
        if i == 0 && pos == 0 {
            score += 30;
        }
        from = pos + ch.len_utf8();
        prev_end = Some(from);
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
    fn initial_workspace_creates_only_the_default_and_preserves_custom_paths() {
        let dir = temp("initial");
        let documents = dir.join("文档");
        fs::create_dir_all(&documents).unwrap();
        let default = documents.join(DEFAULT_WORKSPACE_DIR);
        assert!(!default.exists());
        assert_eq!(initial_workspace(None, Some(documents.clone())).unwrap(), default);
        assert!(default.is_dir());
        let custom = dir.join("其他目录");
        assert_eq!(initial_workspace(Some(custom.clone()), Some(documents)).unwrap(), custom);
        assert!(!custom.exists(), "an unavailable chosen folder must not be recreated");
        assert!(initial_workspace(None, None).is_err());
        assert!(initial_workspace(Some(PathBuf::from("relative")), None).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shows_regular_files_but_keeps_document_operations_limited() {
        let dir = temp("scan");
        fs::create_dir_all(dir.join("chapters")).unwrap();
        fs::create_dir_all(dir.join("empty")).unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(dir.join("readme.md"), "hello").unwrap();
        fs::write(dir.join("chapters/one.mdxb"), "{}").unwrap();
        fs::write(dir.join("cover.png"), "not a doc").unwrap();
        fs::write(dir.join(".hidden/ignored.md"), "hidden").unwrap();
        let ws = WorkspaceDir::scan(dir.clone()).unwrap();
        assert_eq!(ws.children.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["chapters", "empty", "cover.png", "readme.md"]);
        assert!(ws.warnings.is_empty());
        let names: Vec<&str> = ws.files().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["one.mdxb", "readme.md"]);
        assert_eq!(ws.all_files().len(), 3);
        assert_eq!(ws.search("cover.png", false)[0].path, dir.join("cover.png"));
        assert!(ws.search("not a doc", true).is_empty(), "binary or unsupported file bodies must not be searched");
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
    fn content_search_handles_unicode_boundaries_and_case_expansion() {
        let dir = temp("unicode-search");
        let text = format!("{}İstanbul 中华测试{}", "汉".repeat(25), "字".repeat(50));
        fs::write(dir.join("中文.md"), &text).unwrap();
        let ws = WorkspaceDir::scan(dir.clone()).unwrap();
        assert!(ws.search("中华", true)[0].snippet.as_ref().unwrap().contains("中华"));
        assert!(ws.search("istanbul", true).is_empty()); // İ lowercases to i + combining dot.
        assert!(ws.search("i\u{307}stanbul", true)[0].snippet.as_ref().unwrap().contains("İstanbul"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scans_beyond_the_old_depth_limit() {
        let dir = temp("deep");
        let mut nested = dir.clone();
        for _ in 0..16 { nested.push("d"); }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("deep.md"), "deep").unwrap();
        let ws = WorkspaceDir::scan(dir.clone()).unwrap();
        assert_eq!(ws.files().len(), 1);
        assert_eq!(ws.files()[0].path, nested.join("deep.md"));
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

    #[test]
    fn refuses_to_overwrite_external_changes_or_deleted_files() {
        let dir = temp("conflict");
        let path = dir.join("note.md");
        fs::write(&path, "original").unwrap();
        fs::write(&path, "external").unwrap();
        assert!(matches!(write_if_unchanged(&path, b"original", b"local"), Err(SaveFileError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert!(write_if_unchanged(&path, b"external", b"local").is_ok());
        fs::remove_file(&path).unwrap();
        assert!(matches!(write_if_unchanged(&path, b"local", b"new"), Err(SaveFileError::Io(_))));
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn staged_write_rechecks_before_replacing_the_source() {
        let dir = temp("late-conflict");
        let path = dir.join("note.md");
        fs::write(&path, "original").unwrap();
        let result = write_atomic_checked(&path, b"local", || {
            fs::write(&path, "external").map_err(SaveFileError::Io)?;
            if fs::read(&path).map_err(SaveFileError::Io)? == b"original" { Ok(()) } else { Err(SaveFileError::Changed) }
        });
        assert!(matches!(result, Err(SaveFileError::Changed)));
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn simultaneous_atomic_writes_do_not_share_a_temporary_file() {
        let dir = temp("parallel");
        let path = dir.join("note.md");
        fs::write(&path, "initial").unwrap();
        let threads: Vec<_> = (0..4).map(|n| {
            let path = path.clone();
            std::thread::spawn(move || write_atomic(&path, format!("value-{n}").as_bytes()))
        }).collect();
        for thread in threads { thread.join().unwrap().unwrap(); }
        assert!(fs::read_to_string(&path).unwrap().starts_with("value-"));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn export_assets_never_overwrite_a_different_file() {
        let dir = temp("asset-conflict");
        let path = dir.join("note.md");
        let source = dir.join("source.png");
        let target = dir.join("assets/picture.png");
        let assets = vec![(source.clone(), "assets/picture.png".to_string())];
        fs::write(&path, b"original").unwrap();
        fs::write(&source, b"first image").unwrap();
        assert_eq!(write_export_if_unchanged(&path, b"original", b"first edit", &assets).unwrap(), vec![target.clone()]);
        assert_eq!(fs::read(&target).unwrap(), b"first image");
        assert!(write_export_if_unchanged(&path, b"first edit", b"second edit", &assets).unwrap().is_empty());
        fs::write(&source, b"different image").unwrap();
        assert!(matches!(write_export_if_unchanged(&path, b"second edit", b"unsafe", &assets), Err(SaveFileError::AssetConflict(_))));
        assert_eq!(fs::read(&path).unwrap(), b"second edit");
        assert_eq!(fs::read(&target).unwrap(), b"first image");
        fs::remove_file(&target).unwrap();
        fs::remove_file(&source).unwrap();
        assert!(matches!(write_export_if_unchanged(&path, b"second edit", b"unsafe", &assets), Err(SaveFileError::Io(_))));
        assert_eq!(fs::read(&path).unwrap(), b"second edit");
        fs::remove_dir_all(dir).unwrap();
    }

}
