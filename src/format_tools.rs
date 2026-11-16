//! On-demand, per-user Windows converters. Downloads are pinned, verified and
//! staged before publication; no system PATH or document library is modified.
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;
const FILE_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Tool { Pandoc, Typst }

impl Tool {
    fn name(self) -> &'static str { match self { Self::Pandoc => "pandoc", Self::Typst => "typst" } }
    fn url(self) -> &'static str { match self {
        Self::Pandoc => "https://github.com/jgm/pandoc/releases/download/3.14/pandoc-3.14-windows-x86_64.zip",
        Self::Typst => "https://github.com/typst/typst/releases/download/v0.15.1/typst-x86_64-pc-windows-msvc.zip",
    } }
    fn digest(self) -> &'static str { match self {
        Self::Pandoc => "0144b96e8c7b023fd382ec5ef5bdc475e873f1019d6fa72502492df2c9a94a3b",
        Self::Typst => "19ce3551153c2fe7ee9fa2f95208310c8f4d3209fedb699e0333faf8913f6736",
    } }
    fn files(self) -> &'static [(&'static str, &'static str)] { match self {
        Self::Pandoc => &[("pandoc-3.14/pandoc.exe", "pandoc.exe"), ("pandoc-3.14/COPYING.rtf", "COPYING.rtf"), ("pandoc-3.14/COPYRIGHT.txt", "COPYRIGHT.txt")],
        Self::Typst => &[("typst-x86_64-pc-windows-msvc/typst.exe", "typst.exe"), ("typst-x86_64-pc-windows-msvc/LICENSE", "LICENSE"), ("typst-x86_64-pc-windows-msvc/NOTICE", "NOTICE")],
    } }
    fn exe(self) -> &'static str { match self { Self::Pandoc => "pandoc.exe", Self::Typst => "typst.exe" } }
}

fn root() -> Option<PathBuf> {
    std::env::var_os("MARKDOWNX_FORMAT_TOOLS_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|p| p.join("markdownx").join("format-tools")))
}

pub(crate) fn cached(tool: Tool) -> Option<PathBuf> {
    let path = root()?.join(tool.name()).join(tool.exe());
    path.is_file().then_some(path)
}

fn verify(path: &Path, expected: &str) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.len() <= ARCHIVE_LIMIT, "converter archive is too large");
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        total += n as u64;
        ensure!(total <= ARCHIVE_LIMIT, "converter archive is too large");
        hash.update(&buf[..n]);
    }
    ensure!(format!("{:x}", hash.finalize()) == expected, "converter archive checksum does not match");
    Ok(())
}

fn extract(tool: Tool, archive: &Path, directory: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).context("invalid converter archive")?;
    let mut total = 0u64;
    for &(source, target) in tool.files() {
        let mut entry = zip.by_name(source).with_context(|| format!("converter archive is missing {source}"))?;
        ensure!(entry.is_file() && entry.size() <= FILE_LIMIT, "invalid converter archive member");
        total = total.checked_add(entry.size()).context("converter archive is too large")?;
        ensure!(total <= FILE_LIMIT, "converter archive is too large");
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(directory.join(target))?;
        let copied = std::io::copy(&mut entry.by_ref().take(FILE_LIMIT + 1), &mut out)?;
        ensure!(copied == entry.size() && copied <= FILE_LIMIT, "invalid converter archive member size");
        out.flush()?;
    }
    Ok(())
}

pub(crate) fn install(tool: Tool) -> Result<PathBuf> {
    if let Some(path) = cached(tool) { return Ok(path); }
    ensure!(std::env::var_os("MARKDOWNX_FORMATS_OFFLINE").is_none_or(|v| v.is_empty() || v == "0"),
        "{} is unavailable and automatic downloads are disabled", tool.name());
    ensure!(cfg!(target_arch = "x86_64"), "automatic converter downloads require Windows x64");
    let root = root().context("cannot determine the converter tools directory")?;
    std::fs::create_dir_all(&root).context("cannot create converter tools directory")?;
    let lock = std::fs::OpenOptions::new().write(true).create(true).truncate(false)
        .open(root.join(format!(".{}.lock", tool.name()))).context("cannot open converter installation lock")?;
    let started = std::time::Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < std::time::Duration::from_secs(420) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(std::fs::TryLockError::WouldBlock) => bail!("timed out waiting for {} installation", tool.name()),
            Err(std::fs::TryLockError::Error(error)) => return Err(error).context("cannot lock converter installation"),
        }
    }
    // A concurrent process may have finished the download while we waited.
    if let Some(path) = cached(tool) { return Ok(path); }
    let destination = root.join(tool.name());
    ensure!(!destination.exists(), "converter directory already exists but has no executable: {}", destination.display());
    let stage = tempfile::Builder::new().prefix("markdownx-format-").tempdir_in(&root)?;
    let archive = stage.path().join("download.zip");
    eprintln!("MarkdownX: downloading {} for document conversion...", tool.name());
    let curl = std::env::var_os("SystemRoot").map(PathBuf::from).map(|p| p.join("System32").join("curl.exe"))
        .filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("curl.exe"));
    let mut command = Command::new(curl);
    let output = std::fs::File::create(&archive)?;
    command.args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "20", "--speed-limit", "1024", "--speed-time", "60", "--max-time", "300", "--max-filesize", "134217728"])
        .arg(tool.url()).stdin(Stdio::null()).stdout(Stdio::from(output));
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    ensure!(command.status().context("could not start curl for converter download")?.success(), "could not download {}", tool.name());
    verify(&archive, tool.digest())?;
    let unpacked = stage.path().join("tool");
    std::fs::create_dir(&unpacked)?;
    extract(tool, &archive, &unpacked)?;
    match std::fs::rename(&unpacked, &destination) {
        Ok(()) => {}
        Err(_) if destination.join(tool.exe()).is_file() => {}
        Err(error) => bail!("cannot install {}: {error}", tool.name()),
    }
    Ok(destination.join(tool.exe()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    #[test]
    fn verifies_and_extracts_only_the_expected_converter_files() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("download.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for &(name, _) in Tool::Pandoc.files() {
            writer.start_file(name, SimpleFileOptions::default()).unwrap();
            writer.write_all(name.as_bytes()).unwrap();
        }
        writer.start_file("../../unexpected", SimpleFileOptions::default()).unwrap();
        writer.write_all(b"not extracted").unwrap();
        writer.finish().unwrap();
        let expected = format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap()));
        verify(&archive, &expected).unwrap();
        assert!(verify(&archive, &"0".repeat(64)).is_err());
        let out = temp.path().join("out");
        std::fs::create_dir(&out).unwrap();
        extract(Tool::Pandoc, &archive, &out).unwrap();
        assert_eq!(std::fs::read(out.join("pandoc.exe")).unwrap(), b"pandoc-3.14/pandoc.exe");
        assert!(!temp.path().join("unexpected").exists());
        assert!(extract(Tool::Typst, &archive, &out).is_err());
    }
}
