//! Import a Feishu document through the user's separately authorized official CLI.

use crate::model::Document;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_OUTPUT: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct Imported {
    pub document: Document,
    pub xml: String,
    pub response: Vec<u8>,
    pub has_references: bool,
}

#[derive(Debug)]
pub enum ImportError { InvalidLink, MissingCli, Failed, Timeout, TooLarge, InvalidResponse }

/// Never send arbitrary URLs to the CLI, and always import the whole document.
pub fn document_target(input: &str) -> Result<String, ImportError> {
    let input = input.trim();
    if !input.is_empty() && input.len() <= 128 && input.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return Ok(input.to_string());
    }
    let mut url = url::Url::parse(input).map_err(|_| ImportError::InvalidLink)?;
    let host = url.host_str().ok_or(ImportError::InvalidLink)?;
    let trusted = host == "feishu.cn" || host.ends_with(".feishu.cn") || host == "larksuite.com" || host.ends_with(".larksuite.com");
    let mut parts = url.path_segments().ok_or(ImportError::InvalidLink)?;
    let valid_path = matches!(parts.next(), Some("docx" | "wiki"))
        && parts.next().is_some_and(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
    if !trusted || url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() || !valid_path {
        return Err(ImportError::InvalidLink);
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

fn cli_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        let candidates = [dir.join("lark-cli.exe"), dir.join("node_modules/@larksuite/cli/bin/lark-cli.exe")];
        #[cfg(not(windows))]
        let candidates = [dir.join("lark-cli"), dir.join("node_modules/@larksuite/cli/bin/lark-cli")];
        if let Some(found) = candidates.into_iter().find(|path| path.is_file()) { return Some(found); }
    }
    None
}

fn read_limited(mut input: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
    let mut result = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut overflow = false;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 { break; }
        let remaining = MAX_OUTPUT.saturating_sub(result.len());
        result.extend_from_slice(&buffer[..n.min(remaining)]);
        overflow |= n > remaining;
    }
    Ok((result, overflow))
}

pub fn fetch(target: &str) -> Result<Imported, ImportError> {
    let target = document_target(target)?;
    let cli = cli_path().ok_or(ImportError::MissingCli)?;
    let mut command = Command::new(cli);
    command.args(["docs", "+fetch", "--api-version", "v2", "--as", "user", "--doc-format", "xml", "--detail", "full", "--doc", &target, "--format", "json"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|_| ImportError::Failed)?;
    let stdout = child.stdout.take().ok_or(ImportError::Failed)?;
    let stderr = child.stderr.take().ok_or(ImportError::Failed)?;
    let out = std::thread::spawn(move || read_limited(stdout));
    let err = std::thread::spawn(move || read_limited(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < Duration::from_secs(90) => std::thread::sleep(Duration::from_millis(50)),
            _ => { let _ = child.kill(); let _ = child.wait(); break None; }
        }
    };
    let (bytes, too_large) = out.join().ok().and_then(Result::ok).ok_or(ImportError::Failed)?;
    let _ = err.join(); // Never persist or display CLI diagnostics; they may contain private data.
    let status = status.ok_or(ImportError::Timeout)?;
    if too_large { return Err(ImportError::TooLarge); }
    if !status.success() { return Err(ImportError::Failed); }
    parse_response(bytes)
}

fn parse_response(bytes: Vec<u8>) -> Result<Imported, ImportError> {
    if bytes.len() > MAX_OUTPUT { return Err(ImportError::TooLarge); }
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ImportError::InvalidResponse)?;
    if json.get("ok").and_then(|v| v.as_bool()) != Some(true) { return Err(ImportError::Failed); }
    let content = json.pointer("/data/document/content").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).ok_or(ImportError::InvalidResponse)?;
    let document = fdl_core::docxml::parse(content).map_err(|_| ImportError::InvalidResponse)?;
    let mut unsupported = false;
    document.for_each_block(|b| unsupported |= matches!(&b.kind, crate::model::BlockKind::Raw { .. }));
    let has_references = unsupported || json.pointer("/data/document/reference_map").and_then(|v| v.as_object()).is_some_and(|m| !m.is_empty())
        || content.contains("token=") || content.contains("ref=");
    let xml = content.to_string();
    Ok(Imported { document, xml, response: bytes, has_references })
}

/// Create a new editable file in the chosen workspace without replacing data.
pub fn save_in_workspace(root: &std::path::Path, protected: &[PathBuf], imported: &Imported) -> anyhow::Result<(PathBuf, bool)> {
    use std::io::Write;
    let title: String = imported.document.title.text.chars().take(48).map(|c| {
        if c.is_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' }
    }).collect();
    let title = title.trim_matches('-');
    let stem = if title.is_empty() { "Feishu".to_string() } else { format!("Feishu-{title}") };
    for index in 1..=1000 {
        let suffix = if index == 1 { String::new() } else { format!("-{index}") };
        let name = format!("{stem}{suffix}.docxxml");
        let expected = root.join(&name);
        if expected.try_exists()? || crate::workspace_files::feishu_sidecar(&expected).try_exists()? { continue; }
        let file = crate::workspace_files::create_content(root, root, &name, false, protected, imported.xml.as_bytes())?;
        let source = crate::workspace_files::feishu_sidecar(&file);
        let payload = serde_json::json!({ "format": "markdownx.feishu-import", "version": 1, "response": String::from_utf8_lossy(&imported.response) });
        let saved = (|| -> anyhow::Result<()> {
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(source)?;
            output.write_all(&serde_json::to_vec(&payload)?)?;
            output.sync_all()?;
            Ok(())
        })().is_ok();
        return Ok((file, saved));
    }
    anyhow::bail!("could not find a free filename")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_feishu_document_urls_and_imports_whole_document() {
        assert_eq!(document_target("https://a.feishu.cn/docx/abc123?from=share#share-xyz").unwrap(), "https://a.feishu.cn/docx/abc123?from=share");
        assert_eq!(document_target("https://a.larksuite.com/wiki/ABC123").unwrap(), "https://a.larksuite.com/wiki/ABC123");
        assert_eq!(document_target("ABC123").unwrap(), "ABC123");
        for bad in ["http://a.feishu.cn/docx/abc", "https://evil-feishu.cn/docx/abc", "https://a.feishu.cn.evil.test/docx/abc", "https://user@a.feishu.cn/docx/abc", "https://a.feishu.cn/sheets/abc"] {
            assert!(document_target(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn retains_complete_response_and_rejects_invalid_content() {
        let input = r#"{"ok":true,"data":{"document":{"content":"<title>中文</title><p>正文</p>","reference_map":{"image":{"r1":{"token":"abc"}}}}}}"#.as_bytes().to_vec();
        let imported = parse_response(input.clone()).unwrap();
        assert_eq!(imported.response, input);
        assert_eq!(imported.document.title.text, "中文");
        assert!(imported.has_references);
        assert!(parse_response(br#"{"ok":true,"data":{"document":{"content":"<p>"}}}"#.to_vec()).is_err());
        assert!(parse_response(br#"{"ok":false}"#.to_vec()).is_err());
        let unknown = br#"{"ok":true,"data":{"document":{"content":"<title>Test</title><future-block>Keep</future-block>"}}}"#.to_vec();
        assert!(parse_response(unknown).unwrap().has_references);
    }

    #[test]
    fn saves_to_workspace_without_overwriting_and_moves_the_source_on_rename() {
        let root = std::env::temp_dir().join(format!("markdownx-feishu-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let input = r#"{"ok":true,"data":{"document":{"content":"<title>中文</title><p>正文</p>","reference_map":{"image":{"r1":{"token":"abc"}}}}}}"#;
        let imported = parse_response(input.as_bytes().to_vec()).unwrap();
        let existing = root.join("Feishu-中文.docxxml");
        std::fs::write(&existing, "do not overwrite").unwrap();
        let (path, source_saved) = save_in_workspace(&root, &[], &imported).unwrap();
        assert!(source_saved);
        assert_ne!(path, existing);
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "do not overwrite");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), imported.xml);
        let source = crate::workspace_files::feishu_sidecar(&path);
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        assert_eq!(data["response"].as_str().unwrap(), input);
        let renamed = crate::workspace_files::rename(&root, &path, "renamed.docxxml", false, &[]).unwrap();
        assert!(!source.exists());
        assert!(crate::workspace_files::feishu_sidecar(&renamed).is_file());
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
