//! Bundled first-launch library; existing libraries are never reseeded.
use std::path::Path;
use std::fs;
use std::io::Write;

include!(concat!(env!("OUT_DIR"), "/guides.rs"));
mod chinese { include!(concat!(env!("OUT_DIR"), "/guides_zh.rs")); }

/// Any entry but the lock file means an existing library, even if all
/// documents were deleted; a pending directory is a first launch to finish.
pub(crate) fn seeded(root: &Path) -> std::io::Result<bool> {
    Ok(!root.join(".guides-pending").exists() && fs::read_dir(root)?.any(|e| e.map_or(true, |e| e.file_name() != ".lock")))
}

/// A guide document as the Windows build seeds it: Ctrl / Alt keys.
pub fn for_windows(xml: &str) -> String {
    crate::shortcuts::windows_keys(xml)
}

pub const WELCOME_ID: &str = "4ea72c5a-82e0-54a4-88aa-3a9914755634";

/// Detect only the exact untouched v0.1.14 starter. User-edited pages are never migrated.
pub fn legacy_welcome_is_untouched(root: &Path, english: bool) -> bool {
    if english { return false; }
    let original = include_str!("../shared-mock/guides/legacy/v0.1.14-welcome.xml").replace("\r\n", "\n");
    let windows = for_windows(&original);
    fs::read_to_string(root.join("docs").join(format!("{WELCOME_ID}.xml"))).ok().is_some_and(|text| {
        let text = text.replace("\r\n", "\n");
        text == original || text == windows
    })
}

/// Returns true only to the process that completed a new library.
pub fn initialize(root: &Path, english: bool) -> anyhow::Result<bool> {
    fs::create_dir_all(root)?;
    if seeded(root)? { return Ok(false); }
    // Copies of the app may share a library: two started together on a new
    // one seed it in turn. Released when `lock` drops.
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join(".lock"))?;
    lock.lock()?;
    if seeded(root)? { return Ok(false); }
    let pending = root.join(".guides-pending");
    fs::create_dir_all(&pending)?;
    // A resumed partial install must finish in the original language.
    let language = pending.join("language");
    let english = match fs::read_to_string(&language) {
        Ok(s) if s == "en" => true,
        Ok(s) if s == "zh" => false,
        Ok(_) => anyhow::bail!("invalid pending guide language"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => { fs::write(&language, if english { "en" } else { "zh" })?; english },
        Err(e) => return Err(e.into()),
    };
    // Stage complete files before publishing. The pending directory lets a
    // failed first launch resume without mistaking it for an existing library.
    for (name, bytes) in if english { FILES } else { chinese::FILES } {
        let path = pending.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        let windows = cfg!(not(target_os = "macos")) && name.starts_with("docs/") && name.ends_with(".xml");
        match std::str::from_utf8(bytes) {
            Ok(xml) if windows => fs::write(path, for_windows(xml))?,
            _ => fs::write(path, bytes)?,
        }
    }
    for name in ["assets", "docs", "index.json", "format.json"] {
        let dest = root.join(name);
        let src = pending.join(name);
        if !src.exists() {
            // An empty assets/ is not embedded, but the library still expects it.
            if name == "assets" { fs::create_dir_all(&dest)?; }
            continue;
        }
        if !dest.exists() { fs::rename(src, dest)?; }
    }
    fs::remove_dir_all(pending)?;
    Ok(true)
}

/// Put one editable example into an empty default workspace, never into an
/// existing workspace containing files and never through a link/junction.
pub fn create_workspace_demo(root: &Path, english: bool) -> anyhow::Result<Option<std::path::PathBuf>> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() { return Ok(None); }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; if metadata.file_attributes() & 0x400 != 0 { return Ok(None); } }
    if fs::read_dir(root)?.next().is_some() { return Ok(None); }
    let (name, text) = if english {
        ("Getting started.md", include_str!("../shared-mock/guides/demos/en.md"))
    } else {
        ("开始使用.md", include_str!("../shared-mock/guides/demos/zh.md"))
    };
    let path = root.join(name);
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let text = if cfg!(not(target_os = "macos")) { for_windows(text) } else { text.to_string() };
    if let Err(error) = file.write_all(text.as_bytes()).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error.into());
    }
    // The same empty default folder also gets a workbook, so the spreadsheet
    // editor can be tried without finding a file first.
    let workbook = if english { "Sales data.xlsx" } else { "销售数据.xlsx" };
    fs::OpenOptions::new().write(true).create_new(true).open(root.join(workbook))?
        .write_all(include_bytes!("../MarkdownX-Demo/销售数据.xlsx"))?;
    // Seed the storyboard and its relative resources only in a new workspace.
    fs::create_dir(root.join("storyboard-shots"))?;
    for (name, bytes) in [
        ("分镜与互动演示.md", include_bytes!("../MarkdownX-Demo/分镜与互动演示.md").as_slice()),
        ("分镜资料.md", include_bytes!("../MarkdownX-Demo/分镜资料.md").as_slice()),
        ("storyboard-shots/01.svg", include_bytes!("../MarkdownX-Demo/storyboard-shots/01.svg").as_slice()),
        ("storyboard-shots/02.svg", include_bytes!("../MarkdownX-Demo/storyboard-shots/02.svg").as_slice()),
        ("storyboard-shots/03.svg", include_bytes!("../MarkdownX-Demo/storyboard-shots/03.svg").as_slice()),
        ("storyboard-shots/04(近景).svg", include_bytes!("../MarkdownX-Demo/storyboard-shots/04(近景).svg").as_slice()),
        ("示例视频.mp4", include_bytes!("../MarkdownX-Demo/示例视频.mp4").as_slice()),
        ("示例音频.wav", include_bytes!("../MarkdownX-Demo/示例音频.wav").as_slice()),
        ("示例模型.obj", include_bytes!("../MarkdownX-Demo/示例模型.obj").as_slice()),
        ("示例合同.rtf", include_bytes!("../MarkdownX-Demo/示例合同.rtf").as_slice()),
        ("示例合同.docx", include_bytes!("../MarkdownX-Demo/示例合同.docx").as_slice()),
        ("旧版合同示例.doc", include_bytes!("../MarkdownX-Demo/旧版合同示例.doc").as_slice()),
        ("版式文书示例.ofd", include_bytes!("../MarkdownX-Demo/版式文书示例.ofd").as_slice()),
        ("论文阅读示例.pdf", include_bytes!("../MarkdownX-Demo/论文阅读示例.pdf").as_slice()),
        ("论文附件示例.docxxml", include_bytes!("../MarkdownX-Demo/论文附件示例.docxxml").as_slice()),
        ("电子书论文示例.epub", include_bytes!("../MarkdownX-Demo/电子书论文示例.epub").as_slice()),
        ("论文源文件示例.qmd", include_bytes!("../MarkdownX-Demo/论文源文件示例.qmd").as_slice()),
        ("论文引用示例.md", include_bytes!("../MarkdownX-Demo/论文引用示例.md").as_slice()),
        ("参考文献.bib", include_bytes!("../MarkdownX-Demo/参考文献.bib").as_slice()),
        ("module-game.mjs", include_bytes!("../MarkdownX-Demo/module-game.mjs").as_slice()),
        ("module-helper.mjs", include_bytes!("../MarkdownX-Demo/module-helper.mjs").as_slice()),
        ("module-label.mjs", include_bytes!("../MarkdownX-Demo/module-label.mjs").as_slice()),
        ("storyboard-data.json", include_bytes!("../MarkdownX-Demo/storyboard-data.json").as_slice()),
        ("storyboard-game.js", include_bytes!("../MarkdownX-Demo/storyboard-game.js").as_slice()),
        ("storyboard-game.css", include_bytes!("../MarkdownX-Demo/storyboard-game.css").as_slice()),
        ("storyboard-palette.css", include_bytes!("../MarkdownX-Demo/storyboard-palette.css").as_slice()),
    ] {
        fs::OpenOptions::new().write(true).create_new(true).open(root.join(name))?.write_all(bytes)?;
    }
    Ok(Some(path))
}

/// Create a separate demo on request without changing an existing folder or library.
pub fn create_demo_folder(parent: &Path, protected: &Path, english: bool) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let parent = parent.canonicalize()?;
    anyhow::ensure!(parent.is_dir(), "demo location is not a folder");
    if let Ok(protected) = protected.canonicalize() { anyhow::ensure!(!parent.starts_with(protected), "demo location is inside the document library"); }
    for n in 1..=10000 {
        let name = if n == 1 { "MarkdownX-Demo".to_string() } else { format!("MarkdownX-Demo-{n}") };
        let root = parent.join(name);
        match fs::create_dir(&root) {
            Ok(()) => {
                let starter = create_workspace_demo(&root, english)
                    .map_err(|e| anyhow::anyhow!("could not populate {}: {e}", root.display()))?
                    .ok_or_else(|| anyhow::anyhow!("demo folder was changed while being created: {}", root.display()))?;
                return Ok((root, starter));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("could not find an unused demo folder name")
}

#[cfg(test)]
mod tests {
    #[test]
    fn requested_demo_uses_a_new_folder_and_avoids_the_library() {
        let parent = std::env::temp_dir().join(format!("markdownx-request-demo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(parent.join("MarkdownX-Demo")).unwrap();
        std::fs::write(parent.join("MarkdownX-Demo/keep.txt"), "keep").unwrap();
        std::fs::create_dir(parent.join("library")).unwrap();
        assert!(super::create_demo_folder(&parent.join("library"), &parent.join("library"), false).is_err());
        let (root, starter) = super::create_demo_folder(&parent, &parent.join("library"), false).unwrap();
        assert_eq!(root.file_name().unwrap(), "MarkdownX-Demo-2");
        assert!(starter.is_file() && root.join("分镜与互动演示.md").is_file());
        assert_eq!(std::fs::read(root.join("论文附件示例.docxxml")).unwrap(), include_bytes!("../MarkdownX-Demo/论文附件示例.docxxml"));
        assert!(root.join("论文阅读示例.pdf").is_file());
        assert_eq!(std::fs::read_to_string(parent.join("MarkdownX-Demo/keep.txt")).unwrap(), "keep");
        std::fs::remove_dir_all(parent).unwrap();
    }
    /// Guides go only into an empty folder: an existing library, even one whose
    /// documents were all deleted, is never refilled or overwritten.
    #[test]
    fn existing_library_is_never_reseeded() {
        let root = std::env::temp_dir().join(format!("qmd-guides-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(super::initialize(&root, true).unwrap());
        let docs = root.join("docs");
        let first = std::fs::read_dir(&docs).unwrap().next().unwrap().unwrap().path();
        std::fs::write(&first, "<title>mine</title>").unwrap();
        assert!(!super::initialize(&root, false).unwrap());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "<title>mine</title>");
        for e in std::fs::read_dir(&docs).unwrap() { std::fs::remove_file(e.unwrap().path()).unwrap(); }
        assert!(!super::initialize(&root, false).unwrap());
        assert_eq!(std::fs::read_dir(&docs).unwrap().count(), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Windows guides use Windows keys without retaining Mac-only passages.
    #[test]
    fn windows_guides_use_windows_keys() {
        let docs: Vec<&str> = super::FILES.iter().chain(super::chinese::FILES).filter(|(n, _)| n.starts_with("docs/") && n.ends_with(".xml")).map(|(_, b)| std::str::from_utf8(b).unwrap()).collect();
        let all: String = docs.iter().map(|d| super::for_windows(d)).collect();
        assert!(all.contains("<code>Ctrl+B</code>") && all.contains("<code>Ctrl+K</code>") && all.contains("<code>Ctrl+Alt+1</code>") && all.contains("<code>Ctrl+Shift+C</code>"));
        assert!(!all.contains("on your Mac") && !all.contains("Time Machine"));
        let mac = |l: &&str| l.contains(['⌘', '⌥', '⇧', '⌃']);
        assert!(!all.lines().any(|l| mac(&l)), "{}", all.lines().find(mac).unwrap_or_default());
    }

    /// Copies launched together on a new library: one seeds it, none fails.
    #[test]
    fn simultaneous_first_launches() {
        let root = std::env::temp_dir().join(format!("fdl-guides-{}", uuid::Uuid::new_v4()));
        let runs: Vec<_> = (0..4).map(|_| { let root = root.clone(); std::thread::spawn(move || super::initialize(&root, true)) }).collect();
        assert_eq!(runs.into_iter().map(|r| r.join().unwrap().unwrap()).filter(|seeded| *seeded).count(), 1);
        let docs = super::FILES.iter().filter(|(n, _)| n.starts_with("docs/")).count();
        assert!(docs > 0);
        assert_eq!(std::fs::read_dir(root.join("docs")).unwrap().count(), docs);
        assert!(!root.join(".guides-pending").exists());
        // A library with content is never reseeded.
        std::fs::remove_dir_all(root.join("docs")).unwrap();
        super::initialize(&root, false).unwrap();
        assert!(!root.join("docs").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn localized_first_launch_and_resume_keep_the_same_language() {
        for english in [false, true] {
            let root = std::env::temp_dir().join(format!("markdownx-guides-lang-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(root.join(".guides-pending")).unwrap();
            std::fs::write(root.join(".guides-pending/language"), if english { "en" } else { "zh" }).unwrap();
            assert!(super::initialize(&root, !english).unwrap());
            let text = std::fs::read_to_string(root.join("docs/4ea72c5a-82e0-54a4-88aa-3a9914755634.xml")).unwrap();
            assert!(text.contains(if english { "Welcome to MarkdownX" } else { "欢迎使用 MarkdownX" }));
            assert!(!text.contains(if english { "欢迎使用 MarkdownX" } else { "Welcome to MarkdownX" }));
            assert!(!super::initialize(&root, !english).unwrap());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn workspace_demo_is_localized_and_never_replaces_existing_files() {
        for english in [false, true] {
            let root = std::env::temp_dir().join(format!("markdownx-demo-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let demo = super::create_workspace_demo(&root, english).unwrap().unwrap();
            let text = std::fs::read_to_string(&demo).unwrap();
            assert!(text.contains(if english { "Welcome to MarkdownX" } else { "欢迎使用 MarkdownX" }));
            let workbook = root.join(if english { "Sales data.xlsx" } else { "销售数据.xlsx" });
            let book = fdl_core::spreadsheet::read(&workbook).unwrap();
            assert!(book.sheets.iter().any(|sheet| sheet.name == "销售数据"));
            assert!(root.join("分镜与互动演示.md").is_file());
            for name in ["module-game.mjs", "module-helper.mjs", "module-label.mjs", "storyboard-data.json"] { assert!(root.join(name).is_file(), "missing {name}"); }
            assert_eq!(std::fs::read(root.join("storyboard-game.js")).unwrap(), include_bytes!("../MarkdownX-Demo/storyboard-game.js"));
            assert_eq!(std::fs::read(root.join("storyboard-game.css")).unwrap(), include_bytes!("../MarkdownX-Demo/storyboard-game.css"));
            assert_eq!(std::fs::read(root.join("storyboard-palette.css")).unwrap(), include_bytes!("../MarkdownX-Demo/storyboard-palette.css"));
            assert!(root.join("storyboard-shots/02.svg").is_file());
            assert!(root.join("示例视频.mp4").is_file() && root.join("示例模型.obj").is_file());
            assert_eq!(std::fs::read(root.join("示例音频.wav")).unwrap(), include_bytes!("../MarkdownX-Demo/示例音频.wav"));
            assert!(root.join("示例合同.rtf").is_file() && root.join("示例合同.docx").is_file() && root.join("旧版合同示例.doc").is_file() && root.join("版式文书示例.ofd").is_file() && root.join("电子书论文示例.epub").is_file() && root.join("论文源文件示例.qmd").is_file() && root.join("论文阅读示例.pdf").is_file());
            assert!(root.join("论文引用示例.md").is_file() && root.join("参考文献.bib").is_file());
            std::fs::write(&demo, "My own document").unwrap();
            assert!(super::create_workspace_demo(&root, !english).unwrap().is_none());
            assert_eq!(std::fs::read_to_string(&demo).unwrap(), "My own document");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn legacy_detection_rejects_any_user_edit() {
        let root = std::env::temp_dir().join(format!("markdownx-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let path = root.join("docs").join(format!("{}.xml", super::WELCOME_ID));
        let legacy = include_str!("../shared-mock/guides/legacy/v0.1.14-welcome.xml");
        for text in [legacy.to_string(), super::for_windows(legacy)] {
            std::fs::write(&path, text).unwrap();
            assert!(super::legacy_welcome_is_untouched(&root, false));
            assert!(!super::legacy_welcome_is_untouched(&root, true));
        }
        std::fs::write(&path, format!("{legacy}\nMy edit")).unwrap();
        assert!(!super::legacy_welcome_is_untouched(&root, false));
        std::fs::remove_dir_all(root).unwrap();
    }
}
