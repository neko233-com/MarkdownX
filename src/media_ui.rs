//! Built-in media previews. Each format is one provider in the suffix
//! registry; images and GIFs decode in GPUI, audio shows a decoded waveform,
//! video shows a shell thumbnail, and models use a small software renderer.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use fdl_core::extensions::{BuiltinEditor, builtin_editor};
use fdl_core::media::audio::AudioInfo;
use fdl_core::media::model::{Mesh, Rendered};
use fdl_core::media::video::VideoInfo;
use gpui::*;
use crate::app::Workspace;
#[cfg(target_os = "windows")]
use crate::input::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{ToastKind, icon};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const MODEL_WIDTH: u32 = 900;
const MODEL_HEIGHT: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaKind { Image, Audio, Video, Model, Pdf }

pub(crate) fn media_kind(path: &Path) -> Option<MediaKind> {
    match builtin_editor(path)? {
        BuiltinEditor::Image => Some(MediaKind::Image),
        BuiltinEditor::Audio => Some(MediaKind::Audio),
        BuiltinEditor::Video => Some(MediaKind::Video),
        BuiltinEditor::Model => Some(MediaKind::Model),
        BuiltinEditor::Pdf => Some(MediaKind::Pdf),
        _ => None,
    }
}

pub(crate) fn is_media(path: &Path) -> bool { media_kind(path).is_some() }

/// PDF links use one-based page numbers; reject malformed or overflowing values.
pub(crate) fn pdf_page_fragment(fragment: &str) -> Option<u32> {
    let value = fragment.strip_prefix("page=")?;
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) { return None; }
    value.parse::<u32>().ok()?.checked_sub(1).filter(|page| *page < i32::MAX as u32)
}

pub(crate) struct MediaPreview {
    pub path: PathBuf,
    pub kind: MediaKind,
    zoom: f32,
    yaw: f32,
    pitch: f32,
    drag: Option<(f32, f32)>,
    error: Option<String>,
    audio: Option<AudioInfo>,
    video: Option<VideoInfo>,
    poster: Option<Arc<RenderImage>>,
    mesh: Option<Arc<Mesh>>,
    model_image: Option<Arc<RenderImage>>,
    model_generation: u64,
    model_rendering: bool,
    model_dirty: bool,
    pdf_page: u32,
    pdf_pages: u32,
    pdf_generation: u64,
    pdf_bytes: Option<Arc<Vec<u8>>>,
    pdf_scroll: ScrollHandle,
    #[cfg(target_os = "windows")]
    pdf_search: Option<PdfSearch>,
    #[cfg(target_os = "windows")]
    pdf_stamp: Option<(u64, std::time::SystemTime)>,
    cancel: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
struct PdfSearch {
    field: Entity<TextField>,
    visible: bool,
    loading: bool,
    index: Option<crate::pdf_text::PdfText>,
    error: Option<String>,
    hits: Vec<usize>,
    selected: Option<usize>,
    _subscription: Subscription,
}

#[cfg(target_os = "windows")]
fn pdf_stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

impl Drop for MediaPreview { fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); } }

enum Loaded {
    Image,
    Audio(Option<AudioInfo>),
    Video { info: Option<VideoInfo>, poster: Option<Vec<u8>> },
    Model { mesh: Arc<Mesh>, png: Option<Vec<u8>> },
    Pdf { pages: u32, png: Vec<u8>, bytes: Option<Arc<Vec<u8>>> },
}

fn load_media(kind: MediaKind, path: PathBuf, cancel: Arc<AtomicBool>) -> Result<Loaded, String> {
    match kind {
        MediaKind::Image => Ok(Loaded::Image),
        MediaKind::Audio => Ok(Loaded::Audio(fdl_core::media::audio::inspect(&path).ok())),
        MediaKind::Video => Ok(Loaded::Video {
            info: fdl_core::media::video::inspect(&path).ok(),
            poster: crate::media_thumb::poster(&path),
        }),
        MediaKind::Model => {
            let mesh = Arc::new(fdl_core::media::model::load(&path).map_err(|error| error.to_string())?);
            let png = model_png(&mesh, 0.0, 0.0, 1.0).ok();
            Ok(Loaded::Model { mesh, png })
        }
        MediaKind::Pdf => {
            let bytes = if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("caj")) { Some(Arc::new(convert_caj(&path, cancel)?)) } else { None };
            render_pdf_source(&path, bytes.as_deref().map(Vec::as_slice), 0).map(|(pages, png)| Loaded::Pdf { pages, png, bytes })
        }
    }
}

#[cfg(target_os = "windows")]
fn convert_caj(path: &Path, cancel: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    use caj2pdf_core::{Cancellation, ConversionOptions, InputFormat, Limits, Progress, detect_source, native::SeekableSource};
    struct Cancel(Arc<AtomicBool>);
    impl Cancellation for Cancel { fn is_cancelled(&self) -> bool { self.0.load(Ordering::Relaxed) } }
    impl Progress for Cancel { fn is_cancelled(&self) -> bool { self.0.load(Ordering::Relaxed) } }
    let mut cancel = Cancel(cancel);
    let limits = Limits { max_input_bytes: 256 * 1024 * 1024, max_output_bytes: 256 * 1024 * 1024, max_pages: 10_000, ..Limits::default() };
    let input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if input.metadata().map_err(|e| e.to_string())?.len() > limits.max_input_bytes { return Err("CAJ file exceeds 256 MB".into()); }
    let mut input = SeekableSource::new(input).map_err(|e| e.to_string())?;
    let format = detect_source(&mut input, &limits, &cancel).map_err(|e| e.to_string())?.map(|d| d.format);
    if !matches!(format, Some(InputFormat::Caj | InputFormat::Pdf)) {
        return Err(crate::settings::tr("暂不支持此 CAJ 格式或受保护的文件", "This CAJ variant or protected file is not supported").into());
    }
    let mut bytes = Vec::new();
    caj2pdf_core::convert(&mut input, &mut bytes, ConversionOptions::default(), &limits, &mut cancel).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(not(target_os = "windows"))]
fn convert_caj(_: &Path, _: Arc<AtomicBool>) -> Result<Vec<u8>, String> { Err("CAJ preview is not available on this platform".into()) }

#[cfg(test)]
fn render_pdf(path: &Path, index: u32) -> Result<(u32, Vec<u8>), String> { render_pdf_source(path, None, index) }

#[cfg(target_os = "windows")]
fn render_pdf_source(path: &Path, bytes: Option<&[u8]>, index: u32) -> Result<(u32, Vec<u8>), String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::{StorageFile, Streams::{DataReader, DataWriter, InMemoryRandomAccessStream}};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| e.to_string())?; }
    let _apartment = Apartment;
    let read = || -> windows::core::Result<(u32, Vec<u8>)> {
        let document = if let Some(bytes) = bytes {
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
            writer.WriteBytes(bytes)?;
            writer.StoreAsync()?.get()?;
            writer.FlushAsync()?.get()?;
            writer.DetachStream()?;
            stream.Seek(0)?;
            PdfDocument::LoadFromStreamAsync(&stream)?.get()?
        } else {
            let file = StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from(path.as_os_str()))?.get()?;
            PdfDocument::LoadFromFileAsync(&file)?.get()?
        };
        let pages = document.PageCount()?;
        if index >= pages { return Err(windows::core::Error::from_win32()); }
        let page = document.GetPage(index)?;
        let size = page.Size()?;
        let scale = (1800.0 / size.Width.max(size.Height)).min(2.0);
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth((size.Width * scale).max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).max(1.0) as u32)?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?.get()?;
        page.Close()?;
        let length = stream.Size()?;
        if length > 32 * 1024 * 1024 { return Err(windows::core::Error::from_win32()); }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(length as u32)?.get()?;
        let mut png = vec![0; length as usize];
        reader.ReadBytes(&mut png)?;
        Ok((pages, png))
    };
    read().map_err(|error| format!("{}: {error}", crate::settings::tr("无法读取 PDF（文件可能已加密或损坏）", "Could not read PDF (the file may be encrypted or damaged)")))
}

#[cfg(not(target_os = "windows"))]
fn render_pdf_source(_: &Path, _: Option<&[u8]>, _: u32) -> Result<(u32, Vec<u8>), String> {
    Err(crate::settings::tr("此平台暂不支持 PDF 预览", "PDF preview is not available on this platform").into())
}

pub(crate) fn model_png(mesh: &Mesh, yaw: f32, pitch: f32, zoom: f32) -> Result<Vec<u8>, String> {
    let Rendered { width, height, rgba } = fdl_core::media::model::render(mesh, MODEL_WIDTH, MODEL_HEIGHT, yaw, pitch, zoom)
        .map_err(|error| error.to_string())?;
    let mut png = Vec::new();
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    Ok(png)
}

pub(crate) fn decoded_image(png: &[u8]) -> Option<Arc<RenderImage>> {
    let mut data = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.into_rgba8();
    for pixel in data.as_chunks_mut::<4>().0 { pixel.swap(0, 2); }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(data)])))
}

fn kind_label(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => crate::settings::tr("图片", "Image"),
        MediaKind::Audio => crate::settings::tr("音频", "Audio"),
        MediaKind::Video => crate::settings::tr("视频", "Video"),
        MediaKind::Model => crate::settings::tr("3D 模型", "3D model"),
        MediaKind::Pdf => crate::settings::tr("PDF · 只读", "PDF · read-only"),
    }
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() { value /= 1024.0; unit += 1; }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", units[unit]) }
}

fn format_time(seconds: Option<f64>) -> Option<String> {
    let seconds = seconds?;
    if !seconds.is_finite() || seconds < 0.0 { return None; }
    let total = seconds.round() as u64;
    Some(format!("{:02}:{:02}", total / 60, total % 60))
}

impl Workspace {
    pub(crate) fn open_media_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_media_preview_at(path, None, window, cx);
    }

    pub(crate) fn open_media_preview_at(&mut self, path: PathBuf, page: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
        if self.spreadsheet_has_pending(cx) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("工作簿仍有未保存的修改，请先保存或在工作簿窗口中处理", "The workbook has unsaved changes; save or resolve them before continuing"), cx);
            return;
        }
        let Some(kind) = media_kind(&path) else { return };
        if !path.is_file() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("文件不存在", "The file does not exist"), cx);
            return;
        }
        self.spreadsheet_preview = None;
        window.focus(&self.focus);
        self.media_serial = self.media_serial.wrapping_add(1);
        let generation = self.media_serial;
        let cancel = Arc::new(AtomicBool::new(false));
        self.media_preview = Some(MediaPreview {
            path: path.clone(), kind, zoom: 1.0, yaw: 0.0, pitch: 0.0, drag: None,
            error: None, audio: None, video: None, poster: None, mesh: None,
            model_image: None, model_generation: 0, model_rendering: false, model_dirty: false,
            pdf_page: 0, pdf_pages: 0, pdf_generation: 0, pdf_bytes: None, pdf_scroll: ScrollHandle::new(),
            #[cfg(target_os = "windows")]
            pdf_search: None,
            #[cfg(target_os = "windows")]
            pdf_stamp: pdf_stamp(&path),
            cancel: cancel.clone(),
        });
        let task = cx.background_executor().spawn(async move { load_media(kind, path, cancel) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |workspace, window, cx| {
                if workspace.media_serial != generation { return; }
                let Some(preview) = &mut workspace.media_preview else { return };
                let mut page_to_open = None;
                let mut page_missing = false;
                match result {
                    Ok(Loaded::Image) => {}
                    Ok(Loaded::Audio(info)) => { if info.is_none() { preview.error = Some(crate::settings::tr("无法读取音频信息，可用系统应用播放", "Audio details are unavailable; play it with your system app").into()); } preview.audio = info; }
                    Ok(Loaded::Video { info, poster }) => { preview.video = info; preview.poster = poster.as_deref().and_then(decoded_image); }
                    Ok(Loaded::Model { mesh, png }) => { preview.mesh = Some(mesh); preview.model_image = png.as_deref().and_then(decoded_image); }
                    Ok(Loaded::Pdf { pages, png, bytes }) => {
                        preview.pdf_pages = pages; preview.poster = decoded_image(&png); preview.pdf_bytes = bytes;
                        if let Some(page) = page { if page < pages { page_to_open = Some(page); } else { page_missing = true; } }
                    }
                    Err(error) => preview.error = Some(crate::localized_format!("无法预览此文件：{}", "Could not preview this file: {}", crate::settings::diagnostic(&error))),
                }
                if let Some(page) = page_to_open { workspace.turn_pdf_page(page as i32, window, cx); }
                if page_missing { workspace.show_toast_kind(ToastKind::Error, crate::settings::tr("PDF 链接指向的页码不存在", "The linked PDF page does not exist"), cx); }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(crate) fn close_media_preview(&mut self, cx: &mut Context<Self>) {
        self.media_preview = None;
        cx.notify();
    }

    fn turn_pdf_page(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &mut self.media_preview else { return };
        if preview.kind != MediaKind::Pdf || preview.pdf_pages == 0 { return; }
        let next = (preview.pdf_page as i64 + i64::from(delta)).clamp(0, i64::from(preview.pdf_pages - 1)) as u32;
        if next == preview.pdf_page { return; }
        preview.pdf_page = next;
        preview.pdf_scroll.set_offset(Point::default());
        preview.pdf_generation = preview.pdf_generation.wrapping_add(1);
        let generation = preview.pdf_generation;
        let serial = self.media_serial;
        let path = preview.path.clone();
        let bytes = preview.pdf_bytes.clone();
        preview.poster = None;
        preview.error = None;
        let task = cx.background_executor().spawn(async move { render_pdf_source(&path, bytes.as_deref().map(Vec::as_slice), next) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |workspace, _, cx| {
                if workspace.media_serial != serial { return; }
                let Some(preview) = &mut workspace.media_preview else { return };
                if preview.pdf_generation != generation { return; }
                match result {
                    Ok((pages, png)) => { preview.pdf_pages = pages; preview.poster = decoded_image(&png); }
                    Err(error) => preview.error = Some(error),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    fn adjust_media_zoom(&mut self, factor: f32, cx: &mut Context<Self>) {
        if let Some(preview) = &mut self.media_preview {
            preview.zoom = if factor == 0. { 1. } else { (preview.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM) };
            if preview.kind == MediaKind::Model { self.rerender_media_model(cx); }
        }
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_pdf_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(preview) = self.media_preview.as_mut() else { return false };
        if preview.kind != MediaKind::Pdf { return false; }
        if preview.pdf_pages == 0 { return true; }
        if let Some(search) = &mut preview.pdf_search {
            search.visible = true;
            window.focus(&search.field.focus_handle(cx));
            cx.notify();
            return true;
        }
        let path = preview.path.clone();
        let bytes = preview.pdf_bytes.clone();
        let stamp = preview.pdf_stamp;
        let cancel = preview.cancel.clone();
        let serial = self.media_serial;
        let field = cx.new(|cx| {
            let mut input = TextField::new(crate::settings::tr("搜索 PDF 文字…", "Search PDF text…"), cx);
            input.max_len = Some(200);
            input
        });
        let subscription = cx.subscribe_in(&field, window, |this, _, event: &TextFieldEvent, window, cx| {
            match event {
                TextFieldEvent::Changed => this.refresh_pdf_matches(cx),
                TextFieldEvent::Submit => this.move_pdf_match(1, window, cx),
                TextFieldEvent::ShiftSubmit => this.move_pdf_match(-1, window, cx),
                TextFieldEvent::Cancel => { this.close_pdf_search(window, cx); },
                _ => {}
            }
        });
        self.media_preview.as_mut().unwrap().pdf_search = Some(PdfSearch {
            field: field.clone(), visible: true, loading: true, index: None, error: None, hits: vec![], selected: None, _subscription: subscription,
        });
        window.focus(&field.focus_handle(cx));
        let task = cx.background_executor().spawn(async move {
            if stamp.is_none() || pdf_stamp(&path) != stamp { return Err("PDF changed; reopen the file".to_string()); }
            let mut result = crate::pdf_text::index(&path, bytes, &cancel)?;
            if pdf_stamp(&path) != stamp { return Err("PDF changed; reopen the file".to_string()); }
            for text in result.pages.iter_mut().flatten() { *text = text.to_lowercase(); }
            Ok(result)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |workspace, _, cx| {
                if workspace.media_serial != serial { return; }
                let Some(preview) = &mut workspace.media_preview else { return };
                let Some(search) = &mut preview.pdf_search else { return };
                search.loading = false;
                match result {
                    Ok(index) if index.pages.len() == preview.pdf_pages as usize => search.index = Some(index),
                    Ok(_) => search.error = Some(crate::settings::tr("页数已变化，请重新打开 PDF", "Page count changed; reopen the PDF").into()),
                    Err(_) => search.error = Some(crate::settings::tr("无法提取文字，请重新打开或使用系统应用", "Could not extract text; reopen or use a system app").into()),
                }
                workspace.refresh_pdf_matches(cx);
            });
        }).detach();
        cx.notify();
        true
    }

    #[cfg(target_os = "windows")]
    fn refresh_pdf_matches(&mut self, cx: &mut Context<Self>) {
        let Some(search) = self.media_preview.as_mut().and_then(|preview| preview.pdf_search.as_mut()) else { return };
        let query = search.field.read(cx).text.trim().to_lowercase();
        search.hits = if query.is_empty() { vec![] } else {
            search.index.as_ref().map(|index| index.pages.iter().enumerate().filter_map(|(i, text)| text.as_ref().is_some_and(|text| text.contains(&query)).then_some(i)).collect()).unwrap_or_default()
        };
        search.selected = None;
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    fn move_pdf_match(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &mut self.media_preview else { return };
        let Some(search) = &mut preview.pdf_search else { return };
        if search.hits.is_empty() { return; }
        let count = search.hits.len();
        let next = match search.selected { Some(i) if delta < 0 => (i + count - 1) % count, Some(i) => (i + 1) % count, None if delta < 0 => count - 1, None => 0 };
        search.selected = Some(next);
        let page = search.hits[next] as i32;
        let current = preview.pdf_page as i32;
        self.turn_pdf_page(page - current, window, cx);
        cx.notify();
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn close_pdf_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(search) = self.media_preview.as_mut().and_then(|preview| preview.pdf_search.as_mut()) else { return false };
        if !search.visible { return false; }
        search.visible = false;
        window.focus(&self.focus);
        cx.notify();
        true
    }

    pub(crate) fn handle_pdf_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.media_preview.as_ref().is_some_and(|preview| preview.kind == MediaKind::Pdf) { return false; }
        #[cfg(target_os = "windows")]
        let search_focused = self.media_preview.as_ref().and_then(|preview| preview.pdf_search.as_ref()).is_some_and(|search| search.visible && search.field.focus_handle(cx).is_focused(window));
        #[cfg(not(target_os = "windows"))]
        let search_focused = false;
        if !self.focus.is_focused(window) && !search_focused { return false; }
        let modifiers = event.keystroke.modifiers;
        let primary = if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control };
        #[cfg(target_os = "windows")]
        if primary && event.keystroke.key == "f" { return self.open_pdf_search(window, cx); }
        if search_focused { return false; }
        if primary {
            let factor = match event.keystroke.key.as_str() { "+" | "=" => Some(1.25), "-" => Some(1. / 1.25), "0" => Some(0.), _ => None };
            if let Some(factor) = factor { self.adjust_media_zoom(factor, cx); return true; }
        }
        let delta = match event.keystroke.key.as_str() { "pageup" => Some(-1), "pagedown" => Some(1), "home" => Some(i32::MIN), "end" => Some(i32::MAX), _ => None };
        if let Some(delta) = delta { self.turn_pdf_page(delta, window, cx); return true; }
        false
    }

    /// Scripted verification that asynchronous media loading reached the UI.
    #[cfg(feature = "devtools")]
    pub(crate) fn check_media_preview(&self, expected: &str) {
        let preview = self.media_preview.as_ref().expect("media preview is not open");
        match expected {
            "image" => assert_eq!(preview.kind, MediaKind::Image),
            "audio" => {
                assert_eq!(preview.kind, MediaKind::Audio);
                let audio = preview.audio.as_ref().expect("audio waveform is missing");
                assert!(!audio.peaks.is_empty() && audio.duration_seconds.is_some(), "audio metadata is incomplete");
            }
            "video" => {
                assert_eq!(preview.kind, MediaKind::Video);
                let info = preview.video.as_ref().expect("video metadata is missing");
                assert!(info.bytes > 0 && info.width.is_some() && info.height.is_some(), "video metadata is incomplete");
            }
            "model" => {
                assert_eq!(preview.kind, MediaKind::Model);
                assert!(preview.mesh.is_some() && preview.model_image.is_some(), "model was not rendered");
            }
            "pdf" | "caj" => {
                assert_eq!(preview.kind, MediaKind::Pdf);
                assert!(preview.pdf_pages > 0 && preview.poster.is_some(), "document was not rendered");
                assert_eq!(preview.pdf_bytes.is_some(), expected == "caj");
            }
            "pdf-attachment" => {
                assert_eq!(preview.kind, MediaKind::Pdf);
                assert!(preview.pdf_pages == 3 && preview.pdf_page == 0 && preview.poster.is_some(), "PDF attachment did not render: {:?}", preview.error);
            }
            "pdf-zoomed" | "pdf-scrolled" | "pdf-page2" | "pdf-last" | "pdf-first" | "pdf-reset" | "pdf-page-missing" => {
                assert_eq!(preview.kind, MediaKind::Pdf);
                assert_eq!(preview.pdf_pages, 3);
                assert!(preview.poster.is_some(), "PDF page is not rendered");
                match expected {
                    "pdf-zoomed" => assert!(preview.zoom > 1. && preview.pdf_scroll.max_offset().height > px(0.), "zoom {} scroll {:?}", preview.zoom, preview.pdf_scroll.max_offset()),
                    "pdf-scrolled" => assert!(preview.pdf_scroll.offset().y < px(0.)),
                    "pdf-page2" => assert_eq!(preview.pdf_page, 1),
                    "pdf-last" => assert_eq!(preview.pdf_page, 2),
                    "pdf-first" => assert_eq!(preview.pdf_page, 0),
                    "pdf-page-missing" => {
                        assert_eq!(preview.pdf_page, 0);
                        let (kind, message, _) = self.toast.as_ref().expect("missing page warning was not shown");
                        assert!(*kind == ToastKind::Error && (message.contains("页码不存在") || message.contains("page does not exist")), "wrong missing page warning");
                    }
                    "pdf-reset" => assert_eq!(preview.zoom, 1.),
                    _ => unreachable!(),
                }
            }
            #[cfg(target_os = "windows")]
            "pdf-indexed" | "pdf-caj-indexed" | "pdf-match2" | "pdf-match-all" | "pdf-no-match" | "pdf-search-closed" | "pdf-scan" => {
                let search = preview.pdf_search.as_ref().expect("PDF search not created");
                match expected {
                    "pdf-indexed" | "pdf-caj-indexed" => {
                        assert!(!search.loading && search.error.is_none() && search.index.as_ref().is_some_and(|index| index.pages.len() == 3 && index.pages.iter().all(Option::is_some)), "PDF index did not finish: {:?}", search.error);
                        assert_eq!(preview.pdf_bytes.is_some(), expected == "pdf-caj-indexed");
                    }
                    "pdf-match2" => assert_eq!(search.hits, vec![1]),
                    "pdf-match-all" => assert_eq!(search.hits, vec![0, 1, 2]),
                    "pdf-no-match" => assert!(search.hits.is_empty()),
                    "pdf-search-closed" => assert!(!search.visible),
                    "pdf-scan" => assert!(!search.loading && search.error.is_none() && search.index.as_ref().is_some_and(|index| index.pages.len() == 1 && index.pages[0].as_ref().is_none_or(|text| text.trim().is_empty())) && search.hits.is_empty(), "blank page must not be treated as searchable text"),
                    _ => unreachable!(),
                }
            }
            _ => panic!("unknown media expectation"),
        }
        crate::logging::write(&format!("media-preview: {expected} PASS"));
    }

    fn rerender_media_model(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = &mut self.media_preview else { return };
        if preview.mesh.is_none() { return; }
        if preview.model_rendering { preview.model_dirty = true; return; }
        preview.model_rendering = true;
        preview.model_generation = preview.model_generation.wrapping_add(1);
        let generation = preview.model_generation;
        let mesh = preview.mesh.clone().unwrap();
        let (yaw, pitch, zoom) = (preview.yaw, preview.pitch, preview.zoom);
        let task = cx.background_executor().spawn(async move { model_png(&mesh, yaw, pitch, zoom) });
        cx.spawn(async move |this, cx| {
            let png = task.await.ok();
            let _ = this.update(cx, |workspace, cx| {
                let Some(preview) = &mut workspace.media_preview else { return };
                if preview.model_generation != generation { return; }
                preview.model_rendering = false;
                if let Some(png) = png.as_deref() { preview.model_image = decoded_image(png); }
                if preview.model_dirty { preview.model_dirty = false; workspace.rerender_media_model(cx); }
                cx.notify();
            });
        }).detach();
    }

    fn open_media_externally(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = &self.media_preview else { return };
        let Ok(url) = url::Url::from_file_path(&preview.path) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法识别文件路径", "Could not identify the file path"), cx);
            return;
        };
        cx.open_url(url.as_str());
    }

    pub(crate) fn render_media_preview(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.media_preview.as_ref()?;
        let left = if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let width = (f32::from(window.viewport_size().width) - left).max(1.);
        let height = f32::from(window.viewport_size().height).max(1.);
        let title = state.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut body = div().flex_1().min_h_0().min_w_0().flex().items_center().justify_center().p(px(18.));
        match state.kind {
            MediaKind::Pdf => {
                let view_w = (width - 36.).max(1.);
                let view_h = (height - 155.).max(1.);
                let page = state.poster.as_ref().map(|image| {
                    let natural = image.size(0);
                    let natural_w = natural.width.0.max(1) as f32;
                    let natural_h = natural.height.0.max(1) as f32;
                    let scale = (view_w / natural_w).min(view_h / natural_h) * state.zoom;
                    let page_w = natural_w * scale;
                    let page_h = natural_h * scale;
                    div().w(px(view_w.max(page_w))).h(px(view_h.max(page_h))).flex().items_center().justify_center()
                        .child(img(image.clone()).w(px(page_w)).h(px(page_h)).flex_none().object_fit(ObjectFit::Contain)).into_any_element()
                });
                body = body.child(div().id("pdf-scroll").w_full().h_full().overflow_scroll().track_scroll(&state.pdf_scroll)
                    .children(page.or_else(|| state.error.as_ref().map(|error| div().text_color(Theme::text_caption()).child(error.clone()).into_any_element()))));
            }
            MediaKind::Image => {
                let view_w = (width - 48.).max(1.) * state.zoom;
                let view_h = (height - 150.).max(1.) * state.zoom;
                body = body.child(div().id("media-image-box").w(px((width - 48.).max(1.))).h(px((height - 150.).max(1.))).overflow_hidden().flex().items_center().justify_center().cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, event: &MouseDownEvent, _, cx| {
                        if event.click_count == 2 && let Some(preview) = &mut w.media_preview { preview.zoom = 1.0; cx.notify(); }
                    }))
                    .on_scroll_wheel(cx.listener(|w, event: &ScrollWheelEvent, _, cx| {
                        let dy = match event.delta { ScrollDelta::Pixels(p) => f32::from(p.y), ScrollDelta::Lines(l) => l.y * 40. };
                        if dy != 0.0 && let Some(preview) = &mut w.media_preview {
                            preview.zoom = (preview.zoom * if dy > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(MIN_ZOOM, MAX_ZOOM);
                            cx.notify();
                        }
                    }))
                    .child(img(state.path.clone()).id("media-image").w(px(view_w)).h(px(view_h)).object_fit(ObjectFit::Contain)));
            }
            MediaKind::Audio => {
                let (duration, rate, channels) = state.audio.as_ref().map(|info| (info.duration_seconds, info.sample_rate, info.channels)).unwrap_or_default();
                let peaks = state.audio.as_ref().map(|info| info.peaks.as_slice()).unwrap_or(&[]);
                let mut waveform = div().h(px(110.)).w_full().flex().items_end().gap(px(1.)).px(px(4.));
                for peak in peaks {
                    waveform = waveform.child(div().flex_1().min_w(px(1.)).h(px((peak * 104.).max(2.))).rounded(px(1.)).bg(Theme::primary()));
                }
                body = body.child(div().w(px(width - 80.)).max_h_full().flex().flex_col().gap(px(16.))
                    .child(div().w_full().h(px(130.)).rounded(px(10.)).bg(Theme::fill_hover()).children((!peaks.is_empty()).then_some(waveform)))
                    .child(div().flex().flex_wrap().gap(px(18.)).text_size(px(12.)).text_color(Theme::text_caption())
                        .child(if let Some(duration) = format_time(duration) { format!("{}{}", crate::settings::tr("时长 ", "Duration "), duration) } else { crate::settings::tr("时长未知", "Duration unknown").to_string() })
                        .children(rate.map(|rate| format!("{rate} Hz")))
                        .children(channels.map(|channels| format!("{} {}", channels, crate::settings::tr("声道", "channels")))))
                    .children(state.error.as_ref().map(|error| div().text_size(px(12.)).text_color(Theme::text_caption()).child(error.clone()))));
            }
            MediaKind::Video => {
                let info = state.video.as_ref();
                let dims = info.and_then(|info| Some((info.width?, info.height?))).map(|(w, h)| format!("{w} × {h}"));
                let duration = format_time(info.and_then(|info| info.duration_seconds));
                let bytes = info.map(|info| format_bytes(info.bytes));
                let poster = state.poster.as_ref().map(|image| img(image.clone()).object_fit(ObjectFit::Contain).into_any_element());
                body = body.child(div().w(px(width - 80.)).max_h_full().flex().flex_col().gap(px(16.))
                    .child(div().w_full().h(px(height - 230.)).max_h(px(440.)).min_h(px(180.)).rounded(px(10.)).overflow_hidden().flex().items_center().justify_center().bg(Theme::fill_hover())
                        .children(poster.or_else(|| Some(icon("file-g-video", 64., Theme::icon()).into_any_element()))))
                    .child(div().flex().flex_wrap().gap(px(18.)).text_size(px(12.)).text_color(Theme::text_caption())
                        .children(dims).children(duration.map(|duration| format!("{} {}", crate::settings::tr("时长", "Duration"), duration))).children(bytes)));
            }
            MediaKind::Model => {
                let image = state.model_image.as_ref().map(|image| img(image.clone()).w_full().h_full().object_fit(ObjectFit::Contain).into_any_element());
                body = body.child(div().w_full().h_full().flex().flex_col().gap(px(10.))
                    .child(div().id("media-model").flex_1().min_h_0().w_full().rounded(px(10.)).overflow_hidden().bg(Theme::fill_hover())
                        .flex().items_center().justify_center().cursor_pointer()
                        .on_mouse_down(MouseButton::Left, cx.listener(|w, event: &MouseDownEvent, _, cx| {
                            let reset = if let Some(preview) = &mut w.media_preview {
                                if event.click_count == 2 { preview.yaw = 0.0; preview.pitch = 0.0; preview.zoom = 1.0; true }
                                else { preview.drag = Some((f32::from(event.position.x), f32::from(event.position.y))); false }
                            } else { false };
                            if reset { w.rerender_media_model(cx); }
                            cx.notify();
                        }))
                        .on_scroll_wheel(cx.listener(|w, event: &ScrollWheelEvent, _, cx| {
                            let dy = match event.delta { ScrollDelta::Pixels(p) => f32::from(p.y), ScrollDelta::Lines(l) => l.y * 40. };
                            if dy != 0.0 {
                                if let Some(preview) = &mut w.media_preview {
                                    preview.zoom = (preview.zoom * if dy > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(MIN_ZOOM, MAX_ZOOM);
                                }
                                w.rerender_media_model(cx);
                            }
                        }))
                        .on_mouse_move(cx.listener(|w, event: &MouseMoveEvent, _, cx| {
                            let moved = if let Some(preview) = &mut w.media_preview {
                                if let Some((last_x, last_y)) = preview.drag {
                                    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                                    preview.yaw += (x - last_x) * 0.012;
                                    preview.pitch = (preview.pitch + (y - last_y) * 0.012).clamp(-1.45, 1.45);
                                    preview.drag = Some((x, y));
                                    true
                                } else { false }
                            } else { false };
                            if moved { w.rerender_media_model(cx); }
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(|w, _: &MouseUpEvent, _, cx| {
                            if let Some(preview) = &mut w.media_preview { preview.drag = None; }
                            cx.notify();
                        }))
                        .children(image.or_else(|| state.error.as_ref().map(|error| div().px(px(24.)).text_size(px(12.)).text_color(Theme::text_caption()).child(error.clone()).into_any_element()))))
                    .child(div().text_size(px(11.)).text_color(Theme::text_caption())
                        .child(crate::settings::tr("拖动旋转 · 双击重置 · 滚轮缩放", "Drag to rotate · double-click to reset · scroll to zoom"))));
            }
        }
        let mut content = div().id("media-preview").relative().w_full().h_full().bg(Theme::bg_body()).flex().flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|w, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" { w.close_media_preview(cx); cx.stop_propagation(); }
            }))
            .child(div().px(px(24.)).pt(px(20.)).pb(px(6.)).flex().items_center().gap(px(10.))
                .child(div().flex_1().min_w_0().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().flex_none().text_size(px(11.)).text_color(Theme::text_caption()).child(if state.kind == MediaKind::Pdf && state.path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("caj")) { crate::settings::tr("CAJ · 只读", "CAJ · read-only") } else { kind_label(state.kind) })))
            .child(div().px(px(24.)).pb(px(8.)).text_size(px(12.)).text_color(Theme::text_caption())
                .child(format_bytes(std::fs::metadata(&state.path).map(|meta| meta.len()).unwrap_or(0))));
        #[cfg(target_os = "windows")]
        if let Some(search) = state.pdf_search.as_ref().filter(|search| search.visible) {
            let status = if search.loading { crate::settings::tr("正在提取文字…", "Extracting text…").to_string() }
                else if let Some(error) = &search.error { error.clone() }
                else if let Some(selected) = search.selected { format!("{} / {} {}", selected + 1, search.hits.len(), crate::settings::tr("页", "pages")) }
                else { format!("{} {}", search.hits.len(), crate::settings::tr("个匹配页", "matching pages")) };
            let failed = search.index.as_ref().map(|index| index.pages.iter().filter(|page| page.is_none()).count()).unwrap_or(0);
            let empty = search.index.as_ref().map(|index| index.pages.iter().filter(|page| page.as_ref().is_some_and(|text| text.trim().is_empty())).count()).unwrap_or(0);
            content = content.child(div().px(px(24.)).pb(px(6.)).flex().flex_col().gap(px(5.))
                .child(div().flex().items_center().gap(px(8.))
                    .child(div().flex_1().min_w_0().px(px(8.)).py(px(4.)).border_1().border_color(Theme::line_border()).rounded(px(6.)).child(search.field.clone()))
                    .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(status))
                    .child(zoom_button("pdf-find-prev", "↑").on_click(cx.listener(|w, _, window, cx| w.move_pdf_match(-1, window, cx))))
                    .child(zoom_button("pdf-find-next", "↓").on_click(cx.listener(|w, _, window, cx| w.move_pdf_match(1, window, cx))))
                    .child(zoom_button("pdf-find-close", "×").on_click(cx.listener(|w, _, window, cx| { w.close_pdf_search(window, cx); }))))
                .child(div().text_size(px(11.)).text_color(Theme::text_caption()).child(if failed > 0 || empty > 0 {
                    format!("{} {} · {} {} · {}", failed, crate::settings::tr("页提取失败", "pages failed"), empty, crate::settings::tr("页没有文字", "pages have no text"), crate::settings::tr("扫描件或特殊字体可能无法检索", "Scans or unusual fonts may not be searchable"))
                } else { crate::settings::tr("扫描件或特殊字体可能无法检索", "Scans or unusual fonts may not be searchable").to_string() })));
        }
        content = content.child(body);
        content = content.child(crate::trash::close_button("media-close").absolute().top(px(18.)).right(px(20.))
            .on_click(cx.listener(|w, _, _, cx| w.close_media_preview(cx))));
        let mut footer = div().px(px(24.)).pb(px(16.)).flex().flex_wrap().items_center().gap(px(10.))
            .child(crate::settings::button("media-open-external", crate::settings::tr("使用系统应用打开", "Open with system app"))
                .on_click(cx.listener(|w, _, _, cx| w.open_media_externally(cx))));
        match state.kind {
            MediaKind::Image | MediaKind::Model | MediaKind::Pdf => {
                footer = footer
                    .child(zoom_button("media-zoom-out", "−").on_click(cx.listener(|w, _, _, cx| w.adjust_media_zoom(1. / 1.25, cx))))
                    .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(format!("{:.0}%", state.zoom * 100.)))
                    .child(zoom_button("media-zoom-in", "+").on_click(cx.listener(|w, _, _, cx| w.adjust_media_zoom(1.25, cx))));
            }
            MediaKind::Audio | MediaKind::Video => {}
        }
        if state.kind == MediaKind::Pdf {
            #[cfg(target_os = "windows")]
            { footer = footer.child(crate::settings::button("pdf-find", crate::settings::tr("查找文字", "Find text"))
                .on_click(cx.listener(|w, _, window, cx| { w.open_pdf_search(window, cx); }))); }
            footer = footer
                .child(zoom_button("pdf-first", "«").on_click(cx.listener(|w, _, window, cx| w.turn_pdf_page(i32::MIN, window, cx))))
                .child(zoom_button("pdf-prev", "‹").on_click(cx.listener(|w, _, window, cx| w.turn_pdf_page(-1, window, cx))))
                .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(format!("{} / {}", if state.pdf_pages == 0 { 0 } else { state.pdf_page + 1 }, state.pdf_pages)))
                .child(zoom_button("pdf-next", "›").on_click(cx.listener(|w, _, window, cx| w.turn_pdf_page(1, window, cx))))
                .child(zoom_button("pdf-last", "»").on_click(cx.listener(|w, _, window, cx| w.turn_pdf_page(i32::MAX, window, cx))));
        }
        content = content.child(footer);
        Some(crate::transfer::preview_pane("media-pane", content, left))
    }
}

fn zoom_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).w(px(26.)).h(px(24.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
        .text_size(px(14.)).text_color(Theme::text()).hover(|style| style.bg(Theme::fill_hover())).child(label)
}

#[cfg(test)]
mod tests {
    use super::{media_kind, pdf_page_fragment, MediaKind};
    use std::path::Path;

    fn kind(name: &str) -> Option<MediaKind> { media_kind(Path::new(name)) }

    #[test]
    fn pdf_fragments_are_one_based_and_reject_invalid_values() {
        assert_eq!(pdf_page_fragment("page=1"), Some(0));
        assert_eq!(pdf_page_fragment("page=02"), Some(1));
        for invalid in ["", "page=", "page=0", "page=-1", "page=2&zoom=100", "Page=2", "page=2147483649", "page=999999999999999999"] {
            assert_eq!(pdf_page_fragment(invalid), None, "{invalid}");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn pdf_pages_render_without_modifying_the_source() {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 160] /Resources << >> >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 120] /Resources << >> >>",
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
        for offset in &offsets[1..] { pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
        pdf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len()).as_bytes());
        let path = std::env::temp_dir().join(format!("markdownx-pdf-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, &pdf).unwrap();
        for page in 0..2 {
            let (count, png) = super::render_pdf(&path, page).unwrap();
            assert_eq!(count, 2);
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
        let caj = path.with_extension("caj");
        std::fs::write(&caj, &pdf).unwrap();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let converted = super::convert_caj(&caj, cancel.clone()).unwrap();
        for page in 0..2 {
            let (count, png) = super::render_pdf_source(&caj, Some(&converted), page).unwrap();
            assert_eq!(count, 2);
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
        assert_eq!(std::fs::read(&caj).unwrap(), pdf);
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(super::convert_caj(&caj, cancel).is_err());
        std::fs::write(&caj, b"KDH protected or unsupported").unwrap();
        assert!(super::convert_caj(&caj, Default::default()).is_err());
        std::fs::remove_file(caj).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), pdf);
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires a separately supplied real CAJ sample"]
    fn real_caj_renders_in_memory_without_modifying_source() {
        let path = std::path::PathBuf::from(std::env::var_os("MARKDOWNX_CAJ_SAMPLE").expect("MARKDOWNX_CAJ_SAMPLE"));
        let before = std::fs::read(&path).unwrap();
        let pdf = super::convert_caj(&path, Default::default()).unwrap();
        let (pages, png) = super::render_pdf_source(&path, Some(&pdf), 0).unwrap();
        assert!(pages > 0 && png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn suffixes_route_to_the_matching_media_provider() {
        assert_eq!(kind("照片.PNG"), Some(MediaKind::Image));
        assert_eq!(kind("动画.gif"), Some(MediaKind::Image));
        assert_eq!(kind("图标.svg"), Some(MediaKind::Image));
        assert_eq!(kind("旁白.mp3"), Some(MediaKind::Audio));
        assert_eq!(kind("录音.wav"), Some(MediaKind::Audio));
        assert_eq!(kind("音乐.flac"), Some(MediaKind::Audio));
        assert_eq!(kind("演示.mp4"), Some(MediaKind::Video));
        assert_eq!(kind("录屏.webm"), Some(MediaKind::Video));
        assert_eq!(kind("影片.mov"), Some(MediaKind::Video));
        assert_eq!(kind("角色.glb"), Some(MediaKind::Model));
        assert_eq!(kind("场景.gltf"), Some(MediaKind::Model));
        assert_eq!(kind("方块.obj"), Some(MediaKind::Model));
        assert_eq!(kind("零件.stl"), Some(MediaKind::Model));
        #[cfg(target_os = "windows")]
        assert_eq!(kind("论文.PDF"), Some(MediaKind::Pdf));
        #[cfg(target_os = "windows")]
        assert_eq!(kind("论文.CAJ"), Some(MediaKind::Pdf));
        assert_eq!(kind("笔记.md"), None);
        assert_eq!(kind("表格.xlsx"), None);
        assert_eq!(kind("书.mdxb"), None);
        assert_eq!(kind("数据.game"), None);
        assert_eq!(kind("README"), None);
    }
}
