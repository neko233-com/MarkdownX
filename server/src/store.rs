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

#[derive(Clone, Debug)]
pub struct Store {
    docs: PathBuf,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let docs = root.join("docs");
        fs::create_dir_all(&docs)?;
        Ok(Self { docs })
    }

    pub fn path(&self, id: &str) -> Result<PathBuf, StoreError> {
        if !valid_id(id) { return Err(StoreError::BadId); }
        Ok(self.docs.join(format!("{id}.md")))
    }

    pub fn rel(&self, id: &str) -> Result<String, StoreError> {
        self.path(id)?;
        Ok(format!("docs/{id}.md"))
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

    pub fn stat(&self, id: &str) -> Result<Doc, StoreError> {
        let path = self.path(id)?;
        let content = self.read(id)?;
        let meta = fs::metadata(&path)?;
        Ok(Doc {
            id: id.to_string(),
            title: title(&content, id),
            path: path.display().to_string(),
            size: meta.len(),
            modified: meta.modified().ok().and_then(since_epoch).unwrap_or(0),
        })
    }

    pub fn list(&self) -> std::io::Result<Vec<Doc>> {
        let mut out = vec![];
        for e in fs::read_dir(&self.docs)? {
            let e = e?;
            if !e.file_type()?.is_file() { continue; }
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".md") else { continue };
            if !valid_id(id) { continue; }
            let Ok(content) = fs::read_to_string(e.path()) else { continue };
            let meta = e.metadata()?;
            out.push(Doc {
                id: id.to_string(),
                title: title(&content, id),
                path: e.path().display().to_string(),
                size: meta.len(),
                modified: meta.modified().ok().and_then(since_epoch).unwrap_or(0),
            });
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }
}

fn since_epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_alphanumeric() { return false; }
    if id.len() > 97 || id.contains("..") { return false; }
    id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
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
        assert!(valid_id("lesson-01"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("a b"));
        assert!(!valid_id(""));
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
        assert!(valid_id(&zh), "{zh}");
        assert!(zh.starts_with("doc-"));
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
