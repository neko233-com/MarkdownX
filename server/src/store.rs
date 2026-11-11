use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use uuid::Uuid;

#[derive(Debug)]
pub enum StoreError {
    BadId,
    NotFound,
    AlreadyExists,
    Changed,
    TooBig,
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadId => write!(f, "invalid document id"),
            Self::NotFound => write!(f, "document not found"),
            Self::AlreadyExists => write!(f, "document already exists"),
            Self::Changed => write!(f, "document changed outside the collaboration server"),
            Self::TooBig => write!(f, "document is too large"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

#[derive(Clone, Debug, Serialize)]
pub struct Doc {
    pub id: String,
    pub title: String,
    pub path: String,
    pub size: u64,
    pub modified: u64,
}

/// The Git repository is the workspace. Document ids are repository-relative
/// paths (`design/gdd.md`), which keeps the tree, Git history and backlinks
/// pointing at the same thing.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        let store = Self { root };
        let docs = store.root.join("docs");
        store.check_path(&docs).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
        fs::create_dir_all(&docs)?;
        store.check_path(&docs).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
        Ok(store)
    }

    pub fn path(&self, id: &str) -> Result<PathBuf, StoreError> {
        if !valid_id(id) { return Err(StoreError::BadId); }
        let path = self.root.join(id);
        self.check_path(&path)?;
        Ok(path)
    }

    fn check_path(&self, path: &Path) -> Result<(), StoreError> {
        let relative = path.strip_prefix(&self.root).map_err(|_| StoreError::BadId)?;
        let mut current = self.root.clone();
        for part in relative.components() {
            if !matches!(part, std::path::Component::Normal(_)) { return Err(StoreError::BadId); }
            current.push(part.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(meta) if is_link(&meta) => return Err(StoreError::BadId),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(StoreError::Io(e)),
            }
        }
        Ok(())
    }

    pub fn rel(&self, id: &str) -> Result<String, StoreError> {
        self.path(id)?;
        Ok(id.to_string())
    }

    pub fn read(&self, id: &str) -> Result<String, StoreError> {
        match fs::read_to_string(self.path(id)?) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound),
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    #[cfg(test)]
    pub fn write(&self, id: &str, content: &str, max: usize) -> Result<(), StoreError> {
        if content.len() > max { return Err(StoreError::TooBig); }
        atomic_write(&self.path(id)?, content.as_bytes()).map_err(StoreError::Io)
    }

    pub fn write_if_unchanged(&self, id: &str, expected: &str, content: &str, max: usize) -> Result<(), StoreError> {
        if content.len() > max { return Err(StoreError::TooBig); }
        let path = self.path(id)?;
        let check = || match fs::read(self.path(id)?) {
            Ok(current) if current == expected.as_bytes() => Ok(()),
            Ok(_) => Err(StoreError::Changed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound),
            Err(e) => Err(StoreError::Io(e)),
        };
        check()?;
        atomic_write_checked(&path, content.as_bytes(), check)
    }

    /// Publish a completed new file atomically; never replace an existing path.
    pub fn create_at(&self, id: &str, content: &str, max: usize) -> Result<(), StoreError> {
        if content.len() > max { return Err(StoreError::TooBig); }
        let path = self.path(id)?;
        let parent = path.parent().ok_or(StoreError::BadId)?;
        fs::create_dir_all(parent).map_err(StoreError::Io)?;
        self.check_path(&path)?;
        let tmp = parent.join(format!(".markdownx-create-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            use std::io::Write;
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(StoreError::Io)?;
            file.write_all(content.as_bytes()).map_err(StoreError::Io)?;
            file.sync_all().map_err(StoreError::Io)?;
            drop(file);
            self.check_path(&path)?;
            match fs::hard_link(&tmp, &path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(StoreError::AlreadyExists),
                Err(e) => Err(StoreError::Io(e)),
            }
        })();
        let _ = fs::remove_file(&tmp);
        result
    }

    /// Create a new note under `folder`, returning its repository-relative id.
    /// `folder` is optional and may itself be nested; the default is `docs`.
    pub fn create(&self, title: &str, folder: &str, content: &str, max: usize) -> Result<String, StoreError> {
        if content.len() > max { return Err(StoreError::TooBig); }
        let folder = folder.trim().trim_matches('/').replace('\\', "/");
        if !folder.is_empty() && !valid_folder(&folder) { return Err(StoreError::BadId); }
        let dir = if folder.is_empty() { "docs".to_string() } else { folder };
        let base = slug(title);
        // A title made only of CJK/punctuation leaves a number like "2" after
        // slugging. Keep the path meaningful instead of shipping "2.md".
        let base = if base.chars().all(|c| c.is_ascii_digit() || c == '-') {
            format!("doc-{}", &Uuid::new_v4().simple().to_string()[..8])
        } else {
            base
        };
        for n in 1..=999 {
            let id = if n == 1 { format!("{dir}/{base}.md") } else { format!("{dir}/{base}-{n}.md") };
            match self.create_at(&id, content, max) {
                Ok(()) => return Ok(id),
                Err(StoreError::AlreadyExists) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(StoreError::AlreadyExists)
    }

    pub fn stat(&self, id: &str) -> Result<Doc, StoreError> {
        let path = self.path(id)?;
        let content = self.read(id)?;
        let meta = fs::metadata(&path)?;
        Ok(Doc {
            id: id.to_string(),
            title: title(&content, fallback_title(id).as_str()),
            path: path.display().to_string(),
            size: meta.len(),
            modified: meta.modified().ok().and_then(since_epoch).unwrap_or(0),
        })
    }

    /// List every Markdown file in the repository. Build output, dependencies
    /// and hidden folders are skipped; nested paths stay nested.
    pub fn list(&self) -> std::io::Result<Vec<Doc>> {
        let mut out = vec![];
        walk(&self.root, &self.root, 0, &mut out)?;
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}

fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<Doc>) -> std::io::Result<()> {
    if depth > 16 { return Ok(()); }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "__pycache__" | "dist" | "build") {
            continue;
        }
        // Workflow starters are served by /api/templates, not as ordinary docs.
        if depth == 0 && name == "templates" {
            continue;
        }
        if is_link(&fs::symlink_metadata(&path)?) { continue; }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(&path, root, depth + 1, out)?;
        } else if ty.is_file() && is_markdown(&path) {
            let id = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if !valid_id(&id) { continue; }
            let Ok(content) = fs::read_to_string(&path) else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            out.push(Doc {
                id: id.clone(),
                title: title(&content, fallback_title(&id).as_str()),
                path: path.display().to_string(),
                size: meta.len(),
                modified: meta.modified().ok().and_then(since_epoch).unwrap_or(0),
            });
        }
    }
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("md" | "markdown"))
}

fn fallback_title(id: &str) -> String {
    Path::new(id).file_stem().unwrap_or_default().to_string_lossy().replace(['-', '_'], " ")
}

fn since_epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// A repository-relative Markdown path: `/`-separated segments, no `..`,
/// no absolute paths, no backslashes, and always a Markdown extension.
pub fn valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 240 || id.starts_with('/') || id.contains("..") || id.contains('\\') {
        return false;
    }
    if !id.ends_with(".md") && !id.ends_with(".markdown") { return false; }
    id.split('/').all(valid_segment)
}

fn valid_folder(folder: &str) -> bool {
    !folder.is_empty() && folder.len() <= 160 && folder.split('/').all(valid_segment)
}

/// Chinese, Japanese and Korean filenames are normal in a team repository;
/// reject only control characters, path separators and Windows-reserved
/// characters. Leading dots are hidden files and stay out of the editor.
fn valid_segment(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 96
        && !part.starts_with('.')
        && !part.ends_with(' ')
        && !part.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
}

pub fn title(content: &str, fallback: &str) -> String {
    for line in content.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if let Some(t) = line.strip_prefix("# ") {
            let t = t.trim();
            if !t.is_empty() {
                return t.chars().take(160).collect();
            }
        }
    }
    fallback.to_string()
}

pub fn slug(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        format!("doc-{}", &Uuid::new_v4().simple().to_string()[..8])
    } else {
        out.chars().take(64).collect()
    }
}

#[cfg(test)]
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match atomic_write_checked(path, bytes, || Ok(())) {
        Ok(()) => Ok(()),
        Err(StoreError::Io(error)) => Err(error),
        Err(_) => unreachable!("an unconditional write cannot report a conflict"),
    }
}

fn atomic_write_checked(path: &Path, bytes: &[u8], validate: impl FnOnce() -> Result<(), StoreError>) -> Result<(), StoreError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(StoreError::Io)?;
    let tmp = dir.join(format!(".{}.markdownx-{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), Uuid::new_v4()));
    let write = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(StoreError::Io)?;
        use std::io::Write;
        f.write_all(bytes).map_err(StoreError::Io)?;
        f.sync_all().map_err(StoreError::Io)?;
        drop(f);
        validate()?;
        fs::rename(&tmp, path).map_err(StoreError::Io)
    })();
    if write.is_err() { let _ = fs::remove_file(&tmp); }
    write
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("markdownx-store-{}", Uuid::new_v4()))
    }

    #[test]
    fn concurrent_creation_never_overwrites_existing_files() {
        let root = temp();
        let store = Store::open(&root).unwrap();
        store.create_at("docs/existing.md", "kept", 100).unwrap();
        assert!(matches!(store.create_at("docs/existing.md", "replacement", 100), Err(StoreError::AlreadyExists)));
        assert_eq!(store.read("docs/existing.md").unwrap(), "kept");
        let gate = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8).map(|i| {
            let store = store.clone();
            let gate = gate.clone();
            std::thread::spawn(move || { gate.wait(); (store.create("Same", "docs", &i.to_string(), 100).unwrap(), i) })
        }).collect();
        let mut ids = std::collections::HashSet::new();
        for t in threads {
            let (id, i) = t.join().unwrap();
            assert!(ids.insert(id.clone()));
            assert_eq!(store.read(&id).unwrap(), i.to_string());
        }
        assert_eq!(ids.len(), 8);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_outside_the_workspace() {
        let root = temp();
        let outside = temp();
        fs::create_dir_all(&outside).unwrap();
        let store = Store::open(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("docs/link")).unwrap();
        assert!(matches!(store.create_at("docs/link/escaped.md", "bad", 100), Err(StoreError::BadId)));
        assert!(!outside.join("escaped.md").exists());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn ids_are_conservative() {
        assert!(valid_id("docs/lesson-01.md"));
        assert!(valid_id("design/levels/level-1.markdown"));
        assert!(valid_id("设计/关卡 1.md"));
        assert!(!valid_id("../x.md"));
        assert!(!valid_id("docs/a:b.md"));
        assert!(!valid_id(""));
        assert!(!valid_id("docs/a.txt"));
    }

    #[test]
    fn staged_save_keeps_external_changes() {
        let root = temp();
        let store = Store::open(&root).unwrap();
        store.write("docs/a.md", "original", 100).unwrap();
        let path = root.join("docs/a.md");
        let result = atomic_write_checked(&path, b"local", || {
            fs::write(&path, "external").unwrap();
            if fs::read(&path).unwrap() == b"original" { Ok(()) } else { Err(StoreError::Changed) }
        });
        assert!(matches!(result, Err(StoreError::Changed)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        assert!(matches!(store.write_if_unchanged("docs/a.md", "original", "local", 100), Err(StoreError::Changed)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn title_prefers_the_first_h1() {
        assert_eq!(title("# Hello\n\nbody", "x"), "Hello");
        assert_eq!(title("body", "fallback"), "fallback");
    }

    #[test]
    fn slugs_are_ascii_ids() {
        assert_eq!(slug("Hello World"), "hello-world");
        let zh = slug("中文标题");
        assert!(zh.starts_with("doc-"), "{zh}");
    }

    #[test]
    fn creates_nested_docs_and_lists_them() {
        let root = temp();
        let store = Store::open(&root).unwrap();
        let id = store.create("Game Design", "design", "# Game Design\n", 1 << 20).unwrap();
        assert_eq!(id, "design/game-design.md");
        let zh = store.create("关卡 2", "design", "# 关卡 2\n", 1 << 20).unwrap();
        assert!(zh.starts_with("design/doc-"), "{zh}");
        assert_eq!(store.read(&id).unwrap(), "# Game Design\n");
        let docs = store.list().unwrap();
        assert_eq!(docs.len(), 2);
        assert!(docs.iter().any(|d| d.id == id));
        assert!(docs.iter().any(|d| d.id == zh));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_replaces_cleanly() {
        let root = temp();
        let p = root.join("docs/a.md");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "two");
        assert!(!fs::read_dir(root.join("docs")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains(".tmp")));
        let _ = fs::remove_dir_all(root);
    }
}
