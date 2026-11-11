//! Git workspaces: a folder opened by the user is widened to the repository
//! root, and Markdown saves become small commits. This is deliberately thin:
//! MarkdownX does not reimplement Git, it only makes the desktop and the
//! collaboration server produce useful history in the repository the team
//! already uses.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git_at(path: &Path) -> Command {
    let mut command = Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command.env("GIT_LITERAL_PATHSPECS", "1").env("GIT_TERMINAL_PROMPT", "0").arg("-C").arg(path);
    command
}

/// `path` or one of its parents containing `.git`, as reported by Git itself.
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let out = git_at(start).args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if root.is_empty() { None } else { Some(PathBuf::from(root)) }
}

/// Commit only touched paths without replacing any existing staged work.
/// A missing identity is supplied to this invocation, not written to Git config.
pub fn commit_paths(root: &Path, paths: &[PathBuf], message: &str) -> anyhow::Result<Option<String>> {
    commit_paths_after(root, paths, message, || {})
}

fn commit_paths_after(root: &Path, paths: &[PathBuf], message: &str, before_commit: impl FnOnce()) -> anyhow::Result<Option<String>> {
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
    let mut staged = git_at(root);
    staged.args(["diff", "--cached", "--quiet", "--"]);
    for rel in &rels { staged.arg(rel); }
    let staged = staged.status()?;
    if !staged.success() {
        if matches!(staged.code(), Some(1)) { anyhow::bail!("the file already has staged changes"); }
        anyhow::bail!("could not inspect staged changes");
    }

    let before = index_entries(root, &rels)?;
    let index = TemporaryIndex::new()?;
    let head = git_at(root).args(["rev-parse", "--verify", "-q", "HEAD"]).output()?;
    let mut read_tree = git_at(root);
    read_tree.env("GIT_INDEX_FILE", &index.path);
    if head.status.success() { read_tree.args(["read-tree", "HEAD"]); }
    else { read_tree.args(["read-tree", "--empty"]); }
    let out = read_tree.output()?;
    if !out.status.success() { anyhow::bail!("git read-tree failed: {}", String::from_utf8_lossy(&out.stderr).trim()); }

    let mut add = git_at(root);
    add.env("GIT_INDEX_FILE", &index.path);
    add.arg("add").arg("--");
    for rel in &rels {
        add.arg(rel);
    }
    let out = add.output()?;
    if !out.status.success() {
        anyhow::bail!("git add failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }

    let mut quiet = git_at(root);
    quiet.env("GIT_INDEX_FILE", &index.path);
    quiet.args(["diff", "--cached", "--quiet", "--"]);
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

    before_commit();
    let mut commit = git_at(root);
    commit.env("GIT_INDEX_FILE", &index.path);
    if !git_configured(root, "user.name") { commit.args(["-c", "user.name=MarkdownX"]); }
    if !git_configured(root, "user.email") { commit.args(["-c", "user.email=markdownx@localhost"]); }
    // Git rebuilds its commit index from the current HEAD for --only. If
    // another process committed meanwhile, unrelated changes are not reverted.
    commit.args(["commit", "--only", "-m"]).arg(message).arg("--");
    for rel in &rels { commit.arg(rel); }
    let out = commit.output()?;
    if !out.status.success() {
        anyhow::bail!("git commit failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    // Only synchronize paths whose real index entries did not change while
    // Git was running. Other staged paths are never touched.
    if index_entries(root, &rels)? == before {
        let mut reset = git_at(root);
        reset.args(["reset", "-q", "HEAD", "--"]);
        for rel in &rels { reset.arg(rel); }
        let out = reset.output()?;
        if !out.status.success() { anyhow::bail!("commit created, but index refresh failed: {}", String::from_utf8_lossy(&out.stderr).trim()); }
    } else {
        log::warn!("Git index changed during automatic commit; preserving the newer staged entries");
    }
    let hash = git_at(root).args(["rev-parse", "--short", "HEAD"]).output()?;
    let hash = String::from_utf8_lossy(&hash.stdout).trim().to_string();
    Ok(Some(hash))
}

struct TemporaryIndex { directory: PathBuf, path: PathBuf }

impl TemporaryIndex {
    fn new() -> std::io::Result<Self> {
        let directory = std::env::temp_dir().join(format!("markdownx-git-index-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        Ok(Self { path: directory.join("index"), directory })
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.directory.join("index.lock"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn index_entries(root: &Path, rels: &[String]) -> anyhow::Result<Vec<u8>> {
    let mut command = git_at(root);
    command.args(["ls-files", "--stage", "-z", "--"]);
    for rel in rels { command.arg(rel); }
    let out = command.output()?;
    if !out.status.success() { anyhow::bail!("could not inspect the Git index"); }
    Ok(out.stdout)
}

fn git_configured(root: &Path, key: &str) -> bool {
    git_at(root).args(["config", "--get", key]).output().is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
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

    #[test]
    fn concurrent_commit_does_not_revert_other_paths() {
        let dir = temp("concurrent-head");
        assert!(git_at(&dir).args(["init", "-q", "-b", "main"]).status().unwrap().success());
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "old a").unwrap();
        std::fs::write(&b, "old b").unwrap();
        commit_paths(&dir, &[a.clone(), b.clone()], "initial").unwrap();
        std::fs::write(&a, "new a").unwrap();
        commit_paths_after(&dir, std::slice::from_ref(&a), "save a", || {
            std::fs::write(&b, "new b").unwrap();
            assert!(git_at(&dir).args(["add", "--", "b.md"]).status().unwrap().success());
            assert!(git_at(&dir).args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-q", "-m", "concurrent"]).status().unwrap().success());
        }).unwrap();
        let a_head = git_at(&dir).args(["show", "HEAD:a.md"]).output().unwrap();
        let b_head = git_at(&dir).args(["show", "HEAD:b.md"]).output().unwrap();
        assert_eq!(a_head.stdout, b"new a");
        assert_eq!(b_head.stdout, b"new b");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn automatic_commit_preserves_existing_staged_changes() {
        let dir = temp("staged");
        assert!(Command::new("git").arg("-C").arg(&dir).args(["init", "-q", "-b", "main"]).status().unwrap().success());
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "original a").unwrap();
        std::fs::write(&b, "original b").unwrap();
        let config = std::fs::read(dir.join(".git/config")).unwrap();
        commit_paths(&dir, &[a.clone(), b.clone()], "initial").unwrap();
        assert_eq!(std::fs::read(dir.join(".git/config")).unwrap(), config);

        std::fs::write(&b, "staged b").unwrap();
        assert!(git_at(&dir).args(["add", "--", "b.md"]).status().unwrap().success());
        std::fs::write(&a, "new a").unwrap();
        assert!(commit_paths(&dir, std::slice::from_ref(&a), "edit a").unwrap().is_some());
        assert_eq!(git_at(&dir).args(["diff", "--cached", "--name-only"]).output().unwrap().stdout, b"b.md\n");

        std::fs::write(&a, "staged a").unwrap();
        assert!(git_at(&dir).args(["add", "--", "a.md"]).status().unwrap().success());
        std::fs::write(&a, "unstaged a").unwrap();
        let before = git_at(&dir).args(["diff", "--cached", "--binary"]).output().unwrap().stdout;
        let head = git_at(&dir).args(["rev-parse", "HEAD"]).output().unwrap().stdout;
        assert!(commit_paths(&dir, std::slice::from_ref(&a), "must not commit").is_err());
        assert_eq!(git_at(&dir).args(["diff", "--cached", "--binary"]).output().unwrap().stdout, before);
        assert_eq!(git_at(&dir).args(["rev-parse", "HEAD"]).output().unwrap().stdout, head);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_commit_does_not_stage_the_file() {
        let dir = temp("hook-failure");
        assert!(Command::new("git").arg("-C").arg(&dir).args(["init", "-q", "-b", "main"]).status().unwrap().success());
        let file = dir.join("note.md");
        std::fs::write(&file, "before").unwrap();
        commit_paths(&dir, std::slice::from_ref(&file), "initial").unwrap();
        let hook = dir.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(&file, "after").unwrap();
        let before = index_entries(&dir, &["note.md".into()]).unwrap();
        assert!(commit_paths(&dir, std::slice::from_ref(&file), "blocked").is_err());
        assert_eq!(index_entries(&dir, &["note.md".into()]).unwrap(), before);
        let _ = std::fs::remove_dir_all(dir);
    }
}
