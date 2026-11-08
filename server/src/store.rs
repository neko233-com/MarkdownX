use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use uuid::Uuid;

#[derive(Debug)]
pub enum StoreError {
    BadId,
    NotFound,
    TooBig,
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadId => write!(f, "invalid document id"),
            Self::NotFound => write!(f, "document not found"),
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
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("docs"))?;
        Ok(Self { root })
    }

    pub fn path(&self, id: &str) -> Result<PathBuf, StoreError> {
        if !valid_id(id) { return Err(StoreError::BadId); }
        Ok(self.root.join(id))
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

    pub fn write(&self, id: &str, content: &str, max: usize) -> Result<(), StoreError> {
        if content.len() > max { return Err(StoreError::TooBig); }
        atomic_write(&self.path(id)?, content.as_bytes()).map_err(StoreError::Io)
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
        let mut id = format!("{dir}/{base}.md");
        let mut n = 2;
        while self.path(&id)?.exists() {
            id = format!("{dir}/{base}-{n}.md");
            n += 1;
            if n > 999 { return Err(StoreError::BadId); }
        }
        self.write(&id, content, max)?;
        Ok(id)
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

pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.markdownx-{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), Uuid::new_v4()));
    let write = (|| {
        let mut f = fs::File::create(&tmp)?;
        use std::io::Write;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
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
