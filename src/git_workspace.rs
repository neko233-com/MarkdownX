//! Git workspaces: a folder opened by the user is widened to the repository
//! root, and Markdown saves become small commits. This is deliberately thin:
//! MarkdownX does not reimplement Git, it only makes the desktop and the
//! collaboration server produce useful history in the repository the team
//! already uses.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `path` or one of its parents containing `.git`, as reported by Git itself.
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let out = Command::new("git").arg("-C").arg(start).args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if root.is_empty() { None } else { Some(PathBuf::from(root)) }
}

/// Commit only the touched paths. Returns the new hash, or `None` when Git
/// had nothing to commit. A missing `user.name`/`user.email` is filled with a
/// local MarkdownX identity rather than failing the save.
pub fn commit_paths(root: &Path, paths: &[PathBuf], message: &str) -> anyhow::Result<Option<String>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let rels: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| !p.is_empty() && !p.starts_with(".."))
        .collect();
    if rels.is_empty() {
        return Ok(None);
    }
    ensure_identity(root);

    let mut add = Command::new("git");
    add.arg("-C").arg(root).arg("add").arg("--");
    for rel in &rels {
        add.arg(rel);
    }
    let out = add.output()?;
    if !out.status.success() {
        anyhow::bail!("git add failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }

    let mut quiet = Command::new("git");
    quiet.arg("-C").arg(root).args(["diff", "--cached", "--quiet", "--"]);
    for rel in &rels {
        quiet.arg(rel);
    }
    let quiet = quiet.status()?;
    if quiet.success() {
        return Ok(None);
    }
    if !matches!(quiet.code(), Some(1)) {
        anyhow::bail!("git diff --cached failed");
    }

    let mut commit = Command::new("git");
    commit.arg("-C").arg(root).arg("commit").arg("-m").arg(message).arg("--");
    for rel in &rels {
        commit.arg(rel);
    }
    let out = commit.output()?;
    if !out.status.success() {
        anyhow::bail!("git commit failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let hash = Command::new("git").arg("-C").arg(root).args(["rev-parse", "--short", "HEAD"]).output()?;
    let hash = String::from_utf8_lossy(&hash.stdout).trim().to_string();
    Ok(Some(hash))
}

fn ensure_identity(root: &Path) {
    let name = Command::new("git").arg("-C").arg(root).args(["config", "user.name"]).output();
    if !matches!(&name, Ok(o) if o.status.success() && !o.stdout.is_empty()) {
        let _ = Command::new("git").arg("-C").arg(root).args(["config", "user.name", "MarkdownX"]).status();
    }
    let mail = Command::new("git").arg("-C").arg(root).args(["config", "user.email"]).output();
    if !matches!(&mail, Ok(o) if o.status.success() && !o.stdout.is_empty()) {
        let _ = Command::new("git").arg("-C").arg(root).args(["config", "user.email", "markdownx@localhost"]).status();
    }
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("markdownx-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_root_from_a_nested_folder() {
        let dir = temp("root");
        assert!(Command::new("git").arg("-C").arg(&dir).args(["init", "-q", "-b", "main"]).status().unwrap().success());
        let nested = dir.join("design/levels");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(repo_root(&nested).unwrap().canonicalize().unwrap(), dir.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commits_only_the_touched_file() {
        let dir = temp("commit");
        Command::new("git").arg("-C").arg(&dir).args(["init", "-q", "-b", "main"]).status().unwrap();
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        assert!(commit_paths(&dir, std::slice::from_ref(&a), "add a").unwrap().is_some());
        assert!(commit_paths(&dir, std::slice::from_ref(&a), "no change").unwrap().is_none());
        let log = Command::new("git").arg("-C").arg(&dir).args(["log", "--format=%s", "--", "a.md"]).output().unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("add a"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
