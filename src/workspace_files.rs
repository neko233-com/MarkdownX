//! Guard filesystem operations originating in the workspace sidebar.

use anyhow::{Context, Result, bail, ensure};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return true; } }
    false
}

fn leaf(name: &str) -> Result<&str> {
    let name = name.trim();
    ensure!(!name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\']), "invalid name");
    let mut parts = Path::new(name).components();
    ensure!(matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none(), "invalid name");
    Ok(name)
}

/// Verify every existing component, and reject paths inside/above protected roots.
fn checked(root: &Path, path: &Path, protected: &[PathBuf], may_be_root: bool) -> Result<()> {
    let relative = path.strip_prefix(root).context("path is outside this workspace")?;
    ensure!(may_be_root || !relative.as_os_str().is_empty(), "cannot change a workspace root");
    ensure!(relative.components().all(|c| matches!(c, Component::Normal(_))), "invalid workspace path");
    let canonical_root = root.canonicalize().context("workspace is unavailable")?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        let meta = fs::symlink_metadata(&cursor).with_context(|| format!("cannot access {}", cursor.display()))?;
        ensure!(!is_link(&meta), "symbolic links and junctions cannot be changed");
    }
    let canonical = path.canonicalize()?;
    ensure!(canonical.starts_with(&canonical_root), "path is outside this workspace");
    for item in protected {
        if let Ok(other) = item.canonicalize() {
            ensure!(!canonical.starts_with(&other) && !other.starts_with(&canonical), "path overlaps a protected directory or another workspace");
        }
    }
    Ok(())
}

/// Recheck an item selected from a possibly stale tree before handing it to
/// another application; links and paths outside the workspace are rejected.
pub(crate) fn validate_external_open(root: &Path, path: &Path) -> Result<()> {
    checked(root, path, &[], false)?;
    ensure!(fs::symlink_metadata(path)?.is_file(), "not a regular file");
    Ok(())
}

pub(crate) fn create(root: &Path, parent: &Path, name: &str, dir: bool, protected: &[PathBuf]) -> Result<PathBuf> {
    create_content(root, parent, name, dir, protected, &[])
}

pub(crate) fn create_content(root: &Path, parent: &Path, name: &str, dir: bool, protected: &[PathBuf], content: &[u8]) -> Result<PathBuf> {
    checked(root, parent, &[], true)?;
    // Creating next to a protected descendant is fine; creating inside it is not.
    let canonical_parent = parent.canonicalize()?;
    for item in protected {
        if let Ok(other) = item.canonicalize() { ensure!(!canonical_parent.starts_with(other), "parent is protected"); }
    }
    let mut name = leaf(name)?.to_string();
    if !dir && Path::new(&name).extension().is_none() { name.push_str(".md"); }
    let path = parent.join(name);
    if dir { fs::create_dir(&path)?; }
    else {
        ensure!(matches!(crate::workspace_fs::DocKind::of(&path), Some(crate::workspace_fs::DocKind::Markdown | crate::workspace_fs::DocKind::DocxXml)), "choose a Markdown, DocxXML or text filename");
        ensure!(!fdl_core::annotations::sidecar(&path).exists(), "an annotation sidecar already exists for this name");
        ensure!(!feishu_sidecar(&path).exists(), "a Feishu source sidecar already exists for this name");
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    Ok(path)
}

pub(crate) fn feishu_sidecar(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.feishu.json"))
}

fn owned_feishu_sidecar(path: &Path) -> Result<Option<PathBuf>> {
    let sidecar = feishu_sidecar(path);
    if !sidecar.exists() { return Ok(None); }
    let meta = fs::symlink_metadata(&sidecar)?;
    ensure!(!is_link(&meta) && meta.is_file(), "Feishu source sidecar is not a regular file");
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&sidecar)?).context("cannot identify the Feishu source sidecar")?;
    ensure!(data.get("format").and_then(|v| v.as_str()) == Some("markdownx.feishu-import"), "unrecognized Feishu source sidecar");
    Ok(Some(sidecar))
}

/// A sidecar can be shared by same-stem extensions; move it only when its key
/// identifies the file. A malformed or ambiguous sidecar is left untouched.
fn owned_sidecar(path: &Path) -> Result<Option<PathBuf>> {
    let sidecar = fdl_core::annotations::sidecar(path);
    if !sidecar.exists() { return Ok(None); }
    let meta = fs::symlink_metadata(&sidecar)?;
    ensure!(!is_link(&meta) && meta.is_file(), "annotation sidecar is not a regular file");
    let store = fdl_core::annotations::AnnotationStore::load(&sidecar).context("cannot identify the annotation sidecar")?;
    if crate::workspaces::same_path(Path::new(&store.doc), path) { return Ok(Some(sidecar)); }
    // A previously renamed folder/file may leave an old doc key. Only infer
    // ownership when that old path is gone and there is one matching document.
    if Path::new(&store.doc).exists() { return Ok(None); }
    let siblings = fs::read_dir(path.parent().context("missing parent")?)?.filter_map(|entry| entry.ok())
        .map(|entry| entry.path()).filter(|p| p.is_file() && matches!(crate::workspace_fs::DocKind::of(p), Some(crate::workspace_fs::DocKind::Markdown | crate::workspace_fs::DocKind::DocxXml))
            && fdl_core::annotations::sidecar(p) == sidecar).count();
    Ok((siblings == 1).then_some(sidecar))
}

pub(crate) fn rename(root: &Path, path: &Path, name: &str, dir: bool, protected: &[PathBuf]) -> Result<PathBuf> {
    checked(root, path, protected, false)?;
    let meta = fs::symlink_metadata(path)?;
    ensure!(if dir { meta.is_dir() } else { meta.is_file() }, "file type changed");
    let dest = path.parent().context("missing parent")?.join(leaf(name)?);
    if path == dest { return Ok(dest); }
    if !dir { ensure!(crate::workspace_fs::DocKind::of(&dest) == crate::workspace_fs::DocKind::of(path), "the document type cannot be changed by renaming"); }
    let editable = !dir && matches!(crate::workspace_fs::DocKind::of(path), Some(crate::workspace_fs::DocKind::Markdown | crate::workspace_fs::DocKind::DocxXml));
    let sidecar = if editable { owned_sidecar(path)? } else { None };
    let dest_sidecar = editable.then(|| fdl_core::annotations::sidecar(&dest));
    let feishu = if editable { owned_feishu_sidecar(path)? } else { None };
    let dest_feishu = editable.then(|| feishu_sidecar(&dest));
    if let Some(target) = &dest_sidecar { ensure!(!target.exists() || fdl_core::annotations::sidecar(path) == *target, "destination annotation sidecar already exists"); }
    if let Some(target) = &dest_feishu { ensure!(!target.exists() || feishu_sidecar(path) == *target, "destination Feishu source sidecar already exists"); }
    renamore::rename_exclusive(path, &dest)?;
    if let (Some(from), Some(to)) = (&sidecar, &dest_sidecar) && from != to
        && let Err(error) = renamore::rename_exclusive(from, to) {
        if let Err(rollback) = renamore::rename_exclusive(&dest, path) { bail!("annotation move failed: {error}; document rollback failed: {rollback}; document is at {}", dest.display()); }
        bail!("annotation move failed; document restored: {error}");
    }
    if let (Some(from), Some(to)) = (&feishu, &dest_feishu) && from != to
        && let Err(error) = renamore::rename_exclusive(from, to) {
        let annotation_restored = match (&sidecar, &dest_sidecar) { (Some(old), Some(new)) if old != new => renamore::rename_exclusive(new, old).is_ok(), _ => true };
        let document_restored = renamore::rename_exclusive(&dest, path).is_ok();
        ensure!(annotation_restored && document_restored, "Feishu source move failed: {error}; rollback incomplete");
        bail!("Feishu source move failed; document restored: {error}");
    }
    Ok(dest)
}

pub(crate) fn trash(root: &Path, path: &Path, dir: bool, protected: &[PathBuf]) -> Result<()> {
    checked(root, path, protected, false)?;
    let meta = fs::symlink_metadata(path)?;
    ensure!(if dir { meta.is_dir() } else { meta.is_file() }, "file type changed");
    let mut paths = vec![path.to_path_buf()];
    if !dir && matches!(crate::workspace_fs::DocKind::of(path), Some(crate::workspace_fs::DocKind::Markdown | crate::workspace_fs::DocKind::DocxXml)) {
        if let Some(sidecar) = owned_sidecar(path)? { paths.push(sidecar); }
        if let Some(sidecar) = owned_feishu_sidecar(path)? { paths.push(sidecar); }
    }
    // Windows COM must start in a fresh STA, not in GPUI's executor thread.
    std::thread::spawn(move || send_to_trash(paths).map_err(|e| e.to_string())).join()
        .map_err(|_| anyhow::anyhow!("recycle bin worker failed"))?.map_err(anyhow::Error::msg)
}

#[cfg(not(windows))]
fn send_to_trash(paths: Vec<PathBuf>) -> Result<()> { trash::delete_all(paths)?; Ok(()) }

#[cfg(windows)]
fn send_to_trash(paths: Vec<PathBuf>) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{core::PCWSTR, Win32::{System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED}, UI::Shell::{FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE}}};
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        struct ComGuard;
        impl Drop for ComGuard { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
        let _com = ComGuard;
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        // Never suppress the system's warning if a location cannot recycle.
        operation.SetOperationFlags(FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI | FOFX_EARLYFAILURE)?;
        for path in &paths {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
            operation.DeleteItem(&item, None)?;
        }
        operation.PerformOperations()?;
        ensure!(!operation.GetAnyOperationsAborted()?.as_bool(), "recycle operation was cancelled or aborted");
        for path in paths { ensure!(!path.try_exists()?, "recycle operation did not move {}", path.display()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_open_checks_workspace_membership_and_regular_files() {
        let base = std::env::temp_dir().join(format!("markdownx-open-{}", uuid::Uuid::new_v4()));
        let root = base.join("workspace");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("图片 #1.png");
        fs::write(&file, b"sample").unwrap();
        let outside = base.join("outside.png");
        fs::write(&outside, b"keep").unwrap();
        assert!(validate_external_open(&root, &file).is_ok());
        assert!(validate_external_open(&root, &outside).is_err());
        assert!(validate_external_open(&root, &root).is_err());
        let url = url::Url::from_file_path(&file).unwrap().to_string();
        assert!(url.contains("%20") && url.contains("%23"));
        #[cfg(unix)]
        {
            let link = root.join("link.png");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(validate_external_open(&root, &link).is_err());
        }
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn creates_and_renames_without_overwriting_or_escaping() {
        let root = std::env::temp_dir().join(format!("markdownx-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let a = create(&root, &root, "中文", false, &[]).unwrap();
        fs::write(&a, "original").unwrap();
        let b = create(&root, &root, "other.md", false, &[]).unwrap();
        fs::write(&b, "keep").unwrap();
        assert!(create(&root, &root, "other.md", false, &[]).is_err());
        assert!(create(&root, &root, "../escape", false, &[]).is_err());
        assert!(rename(&root, &a, "other.md", false, &[]).is_err());
        assert_eq!(fs::read_to_string(&b).unwrap(), "keep");
        let renamed = rename(&root, &a, "新名称.md", false, &[]).unwrap();
        assert_eq!(fs::read_to_string(renamed).unwrap(), "original");
        assert!(rename(&root, &root, "bad", true, &[]).is_err());
        let sub = create(&root, &root, "nested", true, &[]).unwrap();
        assert!(rename(&root, &sub, "bad", true, std::slice::from_ref(&sub)).is_err());
        fs::write(sub.join("child.md"), "keep nested content").unwrap();
        let moved = rename(&root, &sub, "renamed-folder", true, &[]).unwrap();
        assert_eq!(fs::read_to_string(moved.join("child.md")).unwrap(), "keep nested content");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_sidecars_with_their_documents_and_refuses_collisions() {
        let root = std::env::temp_dir().join(format!("markdownx-annotations-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let first = create(&root, &root, "one.md", false, &[]).unwrap();
        let side = fdl_core::annotations::sidecar(&first);
        fdl_core::annotations::AnnotationStore::new(first.display().to_string()).save(&side).unwrap();
        let second = rename(&root, &first, "two.md", false, &[]).unwrap();
        assert!(!side.exists());
        assert!(fdl_core::annotations::sidecar(&second).exists());
        let third = rename(&root, &second, "three.md", false, &[]).unwrap();
        assert!(fdl_core::annotations::sidecar(&third).exists());
        let occupied = root.join("occupied.md.annotations.json");
        fs::write(&occupied, "keep").unwrap();
        assert!(rename(&root, &third, "occupied.md", false, &[]).is_err());
        assert_eq!(fs::read_to_string(occupied).unwrap(), "keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "moves and restores uniquely named temporary files in the Windows recycle bin"]
    fn windows_recycle_bin_round_trip() {
        let root = std::env::temp_dir().join(format!("markdownx-recycle-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let file = create(&root, &root, "recycle.md", false, &[]).unwrap();
        fs::write(&file, "temporary test document").unwrap();
        let sidecar = fdl_core::annotations::sidecar(&file);
        fdl_core::annotations::AnnotationStore::new(file.display().to_string()).save(&sidecar).unwrap();
        let folder = create(&root, &root, "temporary-folder", true, &[]).unwrap();
        fs::write(folder.join(".hidden"), "hidden content").unwrap();
        trash(&root, &file, false, &[]).unwrap();
        trash(&root, &folder, true, &[]).unwrap();
        assert!(!file.exists() && !sidecar.exists() && !folder.exists());
        let expected = [file.clone(), sidecar.clone(), folder.clone()];
        std::thread::spawn(move || {
            let found: Vec<_> = trash::os_limited::list().unwrap().into_iter().filter(|item| expected.iter().any(|p| crate::workspaces::same_path(&item.original_path(), p))).collect();
            assert_eq!(found.len(), 3, "all temporary items must be in the recycle bin");
            trash::os_limited::restore_all(found).unwrap();
        }).join().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "temporary test document");
        assert!(sidecar.is_file());
        assert_eq!(fs::read_to_string(folder.join(".hidden")).unwrap(), "hidden content");
        fs::remove_dir_all(root).unwrap();
    }
}
