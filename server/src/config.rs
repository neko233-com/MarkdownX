use std::path::PathBuf;

use anyhow::{bail, Context, Result};

pub const DEFAULT_ADDR: &str = ":8787";
pub const DEFAULT_MAX_BYTES: usize = 2 << 20;

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: String,
    pub root: PathBuf,
    pub token: String,
    pub origin: String,
    pub max_bytes: usize,
    pub commit_name: String,
    pub commit_mail: String,
    pub web_dir: PathBuf,
    pub dev: bool,
}

impl Config {
    pub fn from_args() -> Result<Self> {
        let mut cfg = Self {
            addr: env("MARKDOWNX_SERVER_ADDR", DEFAULT_ADDR),
            root: PathBuf::from(env("MARKDOWNX_SERVER_ROOT", "./data")),
            token: std::env::var("MARKDOWNX_SERVER_TOKEN").unwrap_or_default(),
            origin: std::env::var("MARKDOWNX_SERVER_ORIGIN").unwrap_or_default(),
            max_bytes: DEFAULT_MAX_BYTES,
            commit_name: env("MARKDOWNX_GIT_NAME", "MarkdownX"),
            commit_mail: env("MARKDOWNX_GIT_EMAIL", "server@markdownx.local"),
            web_dir: PathBuf::from(env("MARKDOWNX_SERVER_WEB", "web")),
            dev: false,
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            let (key, inline) = match arg.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (arg.as_str(), None),
            };
            match key {
                "-addr" => cfg.addr = take(&args, &mut i, inline, key)?,
                "-root" => cfg.root = PathBuf::from(take(&args, &mut i, inline, key)?),
                "-token" => cfg.token = take(&args, &mut i, inline, key)?,
                "-origin" => cfg.origin = take(&args, &mut i, inline, key)?,
                "-max-bytes" => cfg.max_bytes = take(&args, &mut i, inline, key)?.parse().context("max-bytes")?,
                "-git-name" => cfg.commit_name = take(&args, &mut i, inline, key)?,
                "-git-email" => cfg.commit_mail = take(&args, &mut i, inline, key)?,
                "-web" => cfg.web_dir = PathBuf::from(take(&args, &mut i, inline, key)?),
                "-dev" => cfg.dev = true,
                "-h" | "--help" => {
                    println!("markdownx-server [-addr ADDR] [-root DIR] [-token TOKEN] [-origin URL] [-max-bytes N] [-git-name NAME] [-git-email MAIL] [-web DIR] [-dev]");
                    std::process::exit(0);
                }
                other => bail!("unknown argument {other}"),
            }
            i += 1;
        }
        cfg.root = std::path::absolute(&cfg.root).context("root")?;
        cfg.web_dir = std::path::absolute(&cfg.web_dir).context("web")?;
        if !(1024..=64 << 20).contains(&cfg.max_bytes) {
            bail!("max-bytes must be between 1 KiB and 64 MiB");
        }
        Ok(cfg)
    }
}

fn env(key: &str, fallback: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback.to_string())
}

fn take(args: &[String], i: &mut usize, inline: Option<String>, key: &str) -> Result<String> {
    if let Some(v) = inline {
        return Ok(v);
    }
    *i += 1;
    args.get(*i).cloned().with_context(|| format!("missing value for {key}"))
}
