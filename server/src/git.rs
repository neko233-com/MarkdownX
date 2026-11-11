use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug)]
pub enum GitError {
    NoChanges,
    Invalid(String),
    Failed(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoChanges => write!(f, "no changes"),
            Self::Invalid(s) | Self::Failed(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for GitError {}

#[derive(Clone, Debug, Serialize)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub time: String,
    pub subject: String,
}

#[derive(Clone, Debug)]
pub struct Repo {
    root: PathBuf,
    name: String,
    mail: String,
}

impl Repo {
    pub async fn open(root: impl AsRef<Path>, name: &str, mail: &str) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let repo = Self { root, name: name.into(), mail: mail.into() };
        let new_repo = !repo.root.join(".git").exists();
        if new_repo {
            let out = Command::new("git").arg("init").arg("-b").arg("main").current_dir(&repo.root).output().await?;
            if !out.status.success() {
                let out = Command::new("git").arg("init").current_dir(&repo.root).output().await?;
                if !out.status.success() {
                    bail!("git init failed: {}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        let ignore = repo.root.join(".gitignore");
        if new_repo && !ignore.exists() {
            std::fs::write(ignore, "*.tmp\n.markdownx-*\n")?;
        }
        Ok(repo)
    }

    async fn git(&self, args: &[&str]) -> Result<String> {
        let out = self.command()
            .args(args)
            .output()
            .await
            .with_context(|| format!("git {}", args.join(" ")))?;
        if !out.status.success() {
            bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn command(&self) -> Command {
        let mut command = Command::new("git");
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        command.current_dir(&self.root).env("GIT_TERMINAL_PROMPT", "0").env("GIT_LITERAL_PATHSPECS", "1").kill_on_drop(true);
        command
    }

    async fn index_entries(&self, rel: &str) -> Result<Vec<u8>> {
        let out = self.command().args(["ls-files", "--stage", "-z", "--", rel]).output().await?;
        if !out.status.success() { bail!("could not inspect Git staging"); }
        Ok(out.stdout)
    }

    pub async fn commit(&self, rel: &str, message: &str, author: &str) -> Result<String, GitError> {
        safe_rel(rel).map_err(|e| GitError::Invalid(e.to_string()))?;
        let status = self.command().args(["diff", "--cached", "--quiet", "--", rel])
            .status()
            .await
            .map_err(|e| GitError::Failed(e.to_string()))?;
        if !status.success() {
            return Err(GitError::Failed(if status.code() == Some(1) { "the file already has staged changes" } else { "could not inspect staged changes" }.into()));
        }
        let before = self.index_entries(rel).await.map_err(|e| GitError::Failed(e.to_string()))?;
        let index = TemporaryIndex::new().map_err(|e| GitError::Failed(e.to_string()))?;
        let head = self.command().args(["rev-parse", "--verify", "-q", "HEAD"]).output().await.map_err(|e| GitError::Failed(e.to_string()))?;
        let mut read_tree = self.command();
        read_tree.env("GIT_INDEX_FILE", &index.path);
        if head.status.success() { read_tree.args(["read-tree", "HEAD"]); } else { read_tree.args(["read-tree", "--empty"]); }
        let out = read_tree.output().await.map_err(|e| GitError::Failed(e.to_string()))?;
        if !out.status.success() { return Err(GitError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string())); }
        let out = self.command().env("GIT_INDEX_FILE", &index.path).args(["add", "--", rel]).output().await.map_err(|e| GitError::Failed(e.to_string()))?;
        if !out.status.success() { return Err(GitError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string())); }
        let status = self.command().env("GIT_INDEX_FILE", &index.path).args(["diff", "--cached", "--quiet", "--", rel]).status().await.map_err(|e| GitError::Failed(e.to_string()))?;
        if status.success() { return Err(GitError::NoChanges); }
        if status.code() != Some(1) { return Err(GitError::Failed("could not inspect pending changes".into())); }
        let author = sanitize_author(author, &self.name, &self.mail);
        let out = self.command()
            .env("GIT_INDEX_FILE", &index.path)
            .env("GIT_COMMITTER_NAME", &self.name)
            .env("GIT_COMMITTER_EMAIL", &self.mail)
            .args(["commit", "--only", "-m", message, "--author", &author, "--", rel])
            .output()
            .await
            .map_err(|e| GitError::Failed(e.to_string()))?;
        if !out.status.success() {
            return Err(GitError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        if self.index_entries(rel).await.map_err(|e| GitError::Failed(e.to_string()))? == before {
            let out = self.command().args(["reset", "-q", "HEAD", "--", rel]).output().await.map_err(|e| GitError::Failed(e.to_string()))?;
            if !out.status.success() { return Err(GitError::Failed("commit created, but index refresh failed".into())); }
        } else {
            tracing::warn!("Git staging changed during commit; preserving the newer entries");
        }
        self.git(&["rev-parse", "HEAD"]).await.map(|s| s.trim().to_string()).map_err(|e| GitError::Failed(e.to_string()))
    }

    pub async fn log(&self, rel: &str, limit: usize) -> Result<Vec<Commit>, GitError> {
        safe_rel(rel).map_err(|e| GitError::Invalid(e.to_string()))?;
        let limit = limit.clamp(1, 200).to_string();
        let out = self
            .git(&["log", "--follow", "--format=%H%x1f%an%x1f%aI%x1f%s", "--max-count", &limit, "--", rel])
            .await
            .map_err(|e| GitError::Failed(e.to_string()))?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let mut p = line.split('\x1f');
                Some(Commit {
                    hash: p.next()?.to_string(),
                    author: p.next()?.to_string(),
                    time: p.next()?.to_string(),
                    subject: p.next()?.to_string(),
                })
            })
            .collect())
    }

    pub async fn show(&self, rev: &str, rel: &str) -> Result<String, GitError> {
        safe_rev(rev).map_err(|e| GitError::Invalid(e.to_string()))?;
        safe_rel(rel).map_err(|e| GitError::Invalid(e.to_string()))?;
        self.git(&["show", &format!("{rev}:{rel}")]).await.map_err(|e| GitError::Failed(e.to_string()))
    }

    pub async fn diff(&self, from: &str, to: &str, rel: &str) -> Result<String, GitError> {
        safe_rel(rel).map_err(|e| GitError::Invalid(e.to_string()))?;
        let to = if to.trim().is_empty() { "HEAD" } else { to };
        safe_rev(to).map_err(|e| GitError::Invalid(e.to_string()))?;
        if !from.trim().is_empty() {
            safe_rev(from).map_err(|e| GitError::Invalid(e.to_string()))?;
            return self.git(&["diff", "--no-color", from, to, "--", rel]).await.map_err(|e| GitError::Failed(e.to_string()));
        }
        let parent = format!("{to}^");
        if self.git(&["rev-parse", "--verify", &parent]).await.is_ok() {
            self.git(&["diff", "--no-color", &parent, to, "--", rel]).await.map_err(|e| GitError::Failed(e.to_string()))
        } else {
            self.git(&["show", "--no-color", "--format=", to, "--", rel]).await.map_err(|e| GitError::Failed(e.to_string()))
        }
    }

    pub async fn merge(&self, base: &[u8], current: &[u8], incoming: &[u8]) -> Result<(String, bool)> {
        let dir = std::env::temp_dir().join(format!("markdownx-merge-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let (b, c, i) = (dir.join("base"), dir.join("current"), dir.join("incoming"));
        std::fs::write(&b, base)?;
        std::fs::write(&c, current)?;
        std::fs::write(&i, incoming)?;
        let out = Command::new("git")
            .arg("merge-file")
            .arg("-p")
            .arg(&b)
            .arg(&c)
            .arg(&i)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        let _ = std::fs::remove_dir_all(&dir);
        match out.status.code() {
            Some(0) => Ok((String::from_utf8_lossy(&out.stdout).into_owned(), true)),
            Some(1) => Ok((String::from_utf8_lossy(&out.stdout).into_owned(), false)),
            _ => Err(anyhow!("git merge-file: {}", String::from_utf8_lossy(&out.stderr).trim())),
        }
    }
}

struct TemporaryIndex { directory: PathBuf, path: PathBuf }

impl TemporaryIndex {
    fn new() -> std::io::Result<Self> {
        let directory = std::env::temp_dir().join(format!("markdownx-server-index-{}", Uuid::new_v4()));
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

fn sanitize_author(author: &str, name: &str, mail: &str) -> String {
    let author = author.trim().replace(['<', '>', '\n', '\r'], "");
    if author.is_empty() { format!("{name} <{mail}>") } else { format!("{author} <{mail}>") }
}

fn safe_rel(rel: &str) -> Result<()> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') || rel.split('/').any(|p| p == ".." || p.is_empty()) {
        bail!("invalid git path");
    }
    Ok(())
}

fn safe_rev(rev: &str) -> Result<()> {
    if rev.is_empty() || rev.len() > 128 || rev.contains("..") || rev.contains("@{") {
        bail!("invalid revision");
    }
    if !rev.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '^' | '~' | '_' | '.' | '-')) {
        bail!("invalid revision");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escape_paths() {
        assert!(safe_rel("docs/a.md").is_ok());
        assert!(safe_rel("../secret").is_err());
        assert!(safe_rel("docs\\a.md").is_err());
        assert!(safe_rev("HEAD~1").is_ok());
        assert!(safe_rev("x..y").is_err());
    }

    #[tokio::test]
    async fn commits_and_logs_a_file() {
        let root = std::env::temp_dir().join(format!("markdownx-git-{}", Uuid::new_v4()));
        let repo = Repo::open(&root, "Test", "test@example.com").await.unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/a.md"), "# A\n").unwrap();
        let hash = repo.commit("docs/a.md", "create", "Ada").await.unwrap();
        assert_eq!(hash.len(), 40);
        let log = repo.log("docs/a.md", 10).await.unwrap();
        assert_eq!(log[0].subject, "create");
        assert!(repo.commit("docs/a.md", "again", "Ada").await.is_err());
        let text = repo.show(&hash, "docs/a.md").await.unwrap();
        assert!(text.contains("# A"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn preserves_repository_config_and_staged_work() {
        let root = std::env::temp_dir().join(format!("markdownx-server-staged-{}", Uuid::new_v4()));
        let repo = Repo::open(&root, "Test", "test@example.com").await.unwrap();
        std::fs::write(root.join("a.md"), "old a").unwrap();
        std::fs::write(root.join("b.md"), "old b").unwrap();
        repo.commit("a.md", "initial a", "Ada").await.unwrap();
        repo.commit("b.md", "initial b", "Ada").await.unwrap();
        assert!(repo.command().args(["config", "user.name", "Existing User"]).status().await.unwrap().success());
        let config = std::fs::read(root.join(".git/config")).unwrap();
        let repo = Repo::open(&root, "Other", "other@example.com").await.unwrap();
        assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), config);
        std::fs::write(root.join("b.md"), "staged b").unwrap();
        assert!(repo.command().args(["add", "--", "b.md"]).status().await.unwrap().success());
        std::fs::write(root.join("a.md"), "new a").unwrap();
        repo.commit("a.md", "save a", "Ada").await.unwrap();
        assert_eq!(repo.git(&["show", ":b.md"]).await.unwrap(), "staged b");
        std::fs::write(root.join("a.md"), "staged a").unwrap();
        assert!(repo.command().args(["add", "--", "a.md"]).status().await.unwrap().success());
        std::fs::write(root.join("a.md"), "unstaged a").unwrap();
        assert!(matches!(repo.commit("a.md", "blocked", "Ada").await, Err(GitError::Failed(_))));
        assert_eq!(repo.git(&["show", ":a.md"]).await.unwrap(), "staged a");
        assert_eq!(repo.git(&["show", ":b.md"]).await.unwrap(), "staged b");
        let _ = std::fs::remove_dir_all(root);
    }
}
