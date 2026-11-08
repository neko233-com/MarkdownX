//! Temporary annotations, stored beside a Markdown file or in the library.
//!
//! The document format stays clean: annotations never enter DocxXML or
//! `.mdxb`. A sidecar can be edited by a person and merged when two readers
//! annotate the same book.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::time;

pub const FORMAT: &str = "markdownx.annotations";
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Author {
    pub id: String,
    pub name: String,
    /// CSS colour, e.g. `#b05226`.
    #[serde(default)]
    pub color: String,
}

impl Author {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let id = slug(&name);
        Author { id, name, color: "#b05226".into() }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Reply {
    pub id: String,
    pub author: Author,
    pub body: String,
    pub created: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Annotation {
    pub id: String,
    pub author: Author,
    pub body: String,
    /// Text the annotation was written against, kept for display and search.
    #[serde(default)]
    pub quote: String,
    /// Child-index path from the document root; empty means the whole page.
    #[serde(default)]
    pub anchor: Vec<usize>,
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub replies: Vec<Reply>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AnnotationStore {
    pub format: String,
    pub version: u32,
    /// Stable document key: a page id or the file path used at creation.
    pub doc: String,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
}

impl AnnotationStore {
    pub fn new(doc: impl Into<String>) -> Self {
        AnnotationStore { format: FORMAT.into(), version: VERSION, doc: doc.into(), annotations: vec![] }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let s: AnnotationStore = serde_json::from_slice(&bytes)?;
        s.check()?;
        Ok(s)
    }

    pub fn load_or_new(path: &Path, doc: impl Into<String>) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::new(doc))
    }

    fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.format == FORMAT, "not a MarkdownX annotation file");
        anyhow::ensure!(self.version == VERSION, "unsupported annotation version {}", self.version);
        let mut ids = std::collections::HashSet::new();
        for a in &self.annotations {
            anyhow::ensure!(ids.insert(a.id.as_str()), "duplicate annotation id");
            anyhow::ensure!(!a.body.trim().is_empty(), "empty annotation body");
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn add(&mut self, author: Author, body: impl Into<String>, quote: impl Into<String>, anchor: Vec<usize>) -> String {
        let id = format!("a-{}", uuid::Uuid::new_v4().simple());
        let now = time::now();
        self.annotations.push(Annotation {
            id: id.clone(), author, body: body.into().trim().into(), quote: quote.into(), anchor,
            created: now, updated: now, resolved: false, replies: vec![],
        });
        id
    }

    pub fn update(&mut self, id: &str, body: &str) -> bool {
        let Some(a) = self.annotations.iter_mut().find(|a| a.id == id) else { return false };
        let body = body.trim();
        if body.is_empty() { return false; }
        a.body = body.into();
        a.updated = time::now();
        true
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.annotations.len();
        self.annotations.retain(|a| a.id != id);
        self.annotations.len() != before
    }

    pub fn set_resolved(&mut self, id: &str, resolved: bool) -> bool {
        let Some(a) = self.annotations.iter_mut().find(|a| a.id == id) else { return false };
        a.resolved = resolved;
        a.updated = time::now();
        true
    }

    pub fn batch_resolve(&mut self, ids: &[String], resolved: bool) -> usize {
        let mut n = 0;
        for a in &mut self.annotations {
            if ids.iter().any(|id| id == &a.id) && a.resolved != resolved {
                a.resolved = resolved;
                a.updated = time::now();
                n += 1;
            }
        }
        n
    }

    pub fn batch_remove(&mut self, ids: &[String]) -> usize {
        let before = self.annotations.len();
        self.annotations.retain(|a| !ids.iter().any(|id| id == &a.id));
        before - self.annotations.len()
    }

    pub fn reply(&mut self, id: &str, author: Author, body: impl Into<String>) -> bool {
        let Some(a) = self.annotations.iter_mut().find(|a| a.id == id) else { return false };
        let body = body.into().trim().to_string();
        if body.is_empty() { return false; }
        a.replies.push(Reply { id: format!("r-{}", uuid::Uuid::new_v4().simple()), author, body, created: time::now() });
        a.updated = time::now();
        true
    }

    /// Merge another copy. Newer copies win; replies are unioned by id.
    pub fn merge(&mut self, other: AnnotationStore) {
        for mut a in other.annotations {
            match self.annotations.iter_mut().find(|b| b.id == a.id) {
                Some(b) => {
                    if a.updated >= b.updated {
                        let replies = std::mem::take(&mut b.replies);
                        for r in replies {
                            if !a.replies.iter().any(|x| x.id == r.id) {
                                a.replies.push(r);
                            }
                        }
                        *b = a;
                    } else {
                        for r in a.replies {
                            if !b.replies.iter().any(|x| x.id == r.id) {
                                b.replies.push(r);
                            }
                        }
                    }
                }
                None => self.annotations.push(a),
            }
        }
        self.annotations.sort_by_key(|a| a.created);
    }

    pub fn unresolved(&self) -> usize {
        self.annotations.iter().filter(|a| !a.resolved).count()
    }
}

/// Sidecar for a Markdown file: `lesson.md.annotations.json`.
pub fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("md.annotations.json")
}

/// Sidecar for a library page: `<root>/annotations/<id>.json`.
pub fn library_sidecar(root: &Path, id: &str) -> PathBuf {
    root.join("annotations").join(format!("{}.json", safe_id(id)))
}

fn safe_id(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c.is_whitespace() || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() { "reader".into() } else { out.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a() -> Author {
        Author::new("Neko")
    }

    #[test]
    fn crud_and_batch() {
        let mut s = AnnotationStore::new("doc");
        let id = s.add(a(), "look here", "quote", vec![1]);
        assert_eq!(s.unresolved(), 1);
        assert!(s.update(&id, "changed"));
        assert!(s.set_resolved(&id, true));
        assert_eq!(s.unresolved(), 0);
        assert_eq!(s.batch_resolve(std::slice::from_ref(&id), false), 1);
        assert!(s.reply(&id, a(), "reply"));
        assert_eq!(s.annotations[0].replies.len(), 1);
        assert!(s.remove(&id));
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn merges_newer_and_unions_replies() {
        let mut left = AnnotationStore::new("doc");
        let id = left.add(a(), "one", "", vec![]);
        let mut right = left.clone();
        left.reply(&id, a(), "left");
        right.update(&id, "two");
        right.reply(&id, a(), "right");
        left.merge(right);
        assert_eq!(left.annotations[0].body, "two");
        assert_eq!(left.annotations[0].replies.len(), 2);
    }

    #[test]
    fn rejects_other_formats_and_duplicate_ids() {
        let mut s = AnnotationStore::new("doc");
        s.add(a(), "x", "", vec![]);
        let mut bad = s.clone();
        bad.format = "other".into();
        assert!(bad.check().is_err());
        let mut dup = s.clone();
        let first = dup.annotations[0].clone();
        dup.annotations.push(first);
        assert!(dup.check().is_err());
    }

    #[test]
    fn sidecars_stay_next_to_the_source() {
        assert_eq!(sidecar(Path::new("a/b/lesson.md")), PathBuf::from("a/b/lesson.md.annotations.json"));
        assert_eq!(library_sidecar(Path::new("/lib"), "a b/c"), PathBuf::from("/lib/annotations/a-b-c.json"));
    }
}
