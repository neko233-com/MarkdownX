//! Small, theme-aware glyphs for the filesystem tree and search results.

use crate::theme::Theme;
use gpui::{Hsla, Svg};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileKind { Markdown, Book, Text, Code, Data, Image, Pdf, Archive, Audio, Video, Other }

fn file_kind(path: &Path) -> FileKind {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(name.as_str(), "dockerfile" | "makefile" | "justfile") { return FileKind::Code; }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mark" => FileKind::Markdown,
        "mdxb" => FileKind::Book,
        "txt" => FileKind::Text,
        "rs" | "go" | "js" | "jsx" | "ts" | "tsx" | "py" | "sh" | "ps1" | "html" | "css" | "c" | "h" | "cpp" | "cs" | "java" => FileKind::Code,
        "json" | "yaml" | "yml" | "toml" | "xml" | "csv" => FileKind::Data,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "zip" | "7z" | "rar" | "gz" | "tar" => FileKind::Archive,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" => FileKind::Audio,
        "mp4" | "mov" | "webm" | "mkv" => FileKind::Video,
        _ => FileKind::Other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FolderKind { Documents, Assets, Source, Other }

fn folder_kind(name: &str) -> FolderKind {
    match name.to_lowercase().as_str() {
        "docs" | "doc" | "documents" | "notes" | "wiki" | "文档" | "笔记" | "知识库" => FolderKind::Documents,
        "assets" | "images" | "image" | "media" | "resources" | "图片" | "素材" | "资源" => FolderKind::Assets,
        "src" | "source" | "code" | "源码" | "代码" => FolderKind::Source,
        _ => FolderKind::Other,
    }
}

fn folder_color(name: &str) -> Hsla {
    match folder_kind(name) {
        FolderKind::Documents => Theme::workspace_documents(),
        FolderKind::Assets => Theme::workspace_assets(),
        FolderKind::Source => Theme::workspace_source(),
        FolderKind::Other => Theme::workspace_folder(),
    }
}

pub(crate) fn folder(name: &str, expanded: bool, size: f32) -> Svg {
    crate::ui::icon(if expanded { "workspace-folder-open" } else { "workspace-folder" }, size, folder_color(name))
}

pub(crate) fn file(path: &Path, size: f32) -> Svg {
    let (glyph, color) = match file_kind(path) {
        FileKind::Markdown => ("workspace-markdown", Theme::workspace_documents()),
        FileKind::Book => ("workspace-book", Theme::workspace_assets()),
        FileKind::Text => ("file-text", Theme::text_caption()),
        FileKind::Code => ("workspace-code", Theme::workspace_source()),
        FileKind::Data => ("workspace-code", Theme::workspace_data()),
        FileKind::Image => ("workspace-image", Theme::workspace_assets()),
        FileKind::Pdf => ("workspace-pdf", Theme::workspace_pdf()),
        FileKind::Archive => ("workspace-archive", Theme::workspace_source()),
        FileKind::Audio => ("workspace-media", Theme::workspace_data()),
        FileKind::Video => ("workspace-video", Theme::workspace_documents()),
        FileKind::Other => ("file-text", Theme::icon()),
    };
    crate::ui::icon(glyph, size, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_folders_and_common_file_types() {
        assert_eq!(folder_kind("DOCS"), FolderKind::Documents);
        assert_eq!(folder_kind("图片"), FolderKind::Assets);
        assert_eq!(folder_kind("src"), FolderKind::Source);
        assert_eq!(folder_kind("my folder"), FolderKind::Other);
        for (name, kind) in [("README.MD", FileKind::Markdown), ("book.mdxb", FileKind::Book), ("note.txt", FileKind::Text), ("script.ps1", FileKind::Code), ("Dockerfile", FileKind::Code), ("config.json", FileKind::Data), ("image.PNG", FileKind::Image), ("guide.pdf", FileKind::Pdf), ("pack.zip", FileKind::Archive), ("song.mp3", FileKind::Audio), ("clip.mp4", FileKind::Video), ("unknown", FileKind::Other)] {
            assert_eq!(file_kind(Path::new(name)), kind);
        }
        for name in ["workspace-folder", "workspace-folder-open", "workspace-markdown", "workspace-book", "workspace-code", "workspace-image", "workspace-pdf", "workspace-archive", "workspace-media", "workspace-video"] {
            let path = format!("icons/{name}.svg");
            let bytes = crate::assets::FILES.iter().find(|(file, _)| *file == path).expect("bundled icon").1;
            let xml = std::str::from_utf8(bytes).unwrap();
            assert!(xml.contains("currentColor") && xml.contains("viewBox"), "{path}");
            let mut reader = quick_xml::Reader::from_str(xml);
            while !matches!(reader.read_event().unwrap(), quick_xml::events::Event::Eof) {}
        }
    }
}
