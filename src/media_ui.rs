//! Built-in media previews. Each format is one provider in the suffix
//! registry; images and GIFs decode in GPUI, audio shows a decoded waveform,
//! video shows a shell thumbnail, and models use a small software renderer.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use fdl_core::extensions::{BuiltinEditor, builtin_editor};
use fdl_core::media::audio::AudioInfo;
use fdl_core::media::model::{Mesh, Rendered};
use fdl_core::media::video::VideoInfo;
use gpui::*;
use crate::app::Workspace;
use crate::theme::Theme;
use crate::ui::{ToastKind, icon};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const MODEL_WIDTH: u32 = 900;
const MODEL_HEIGHT: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaKind { Image, Audio, Video, Model }

pub(crate) fn media_kind(path: &Path) -> Option<MediaKind> {
    match builtin_editor(path)? {
        BuiltinEditor::Image => Some(MediaKind::Image),
        BuiltinEditor::Audio => Some(MediaKind::Audio),
        BuiltinEditor::Video => Some(MediaKind::Video),
        BuiltinEditor::Model => Some(MediaKind::Model),
        _ => None,
    }
}

pub(crate) fn is_media(path: &Path) -> bool { media_kind(path).is_some() }

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
}

enum Loaded {
    Image,
    Audio(Option<AudioInfo>),
    Video { info: Option<VideoInfo>, poster: Option<Vec<u8>> },
    Model { mesh: Arc<Mesh>, png: Option<Vec<u8>> },
}

fn load_media(kind: MediaKind, path: PathBuf) -> Result<Loaded, String> {
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
    }
}

fn model_png(mesh: &Mesh, yaw: f32, pitch: f32, zoom: f32) -> Result<Vec<u8>, String> {
    let Rendered { width, height, rgba } = fdl_core::media::model::render(mesh, MODEL_WIDTH, MODEL_HEIGHT, yaw, pitch, zoom)
        .map_err(|error| error.to_string())?;
    let mut png = Vec::new();
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    Ok(png)
}

fn decoded_image(png: &[u8]) -> Option<Arc<RenderImage>> {
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
        let Some(kind) = media_kind(&path) else { return };
        if !path.is_file() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("文件不存在", "The file does not exist"), cx);
            return;
        }
        self.media_serial = self.media_serial.wrapping_add(1);
        let generation = self.media_serial;
        self.media_preview = Some(MediaPreview {
            path: path.clone(), kind, zoom: 1.0, yaw: 0.0, pitch: 0.0, drag: None,
            error: None, audio: None, video: None, poster: None, mesh: None,
            model_image: None, model_generation: 0, model_rendering: false, model_dirty: false,
        });
        let task = cx.background_executor().spawn(async move { load_media(kind, path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |workspace, _, cx| {
                if workspace.media_serial != generation { return; }
                let Some(preview) = &mut workspace.media_preview else { return };
                match result {
                    Ok(Loaded::Image) => {}
                    Ok(Loaded::Audio(info)) => { if info.is_none() { preview.error = Some(crate::settings::tr("无法读取音频信息，可用系统应用播放", "Audio details are unavailable; play it with your system app").into()); } preview.audio = info; }
                    Ok(Loaded::Video { info, poster }) => { preview.video = info; preview.poster = poster.as_deref().and_then(decoded_image); }
                    Ok(Loaded::Model { mesh, png }) => { preview.mesh = Some(mesh); preview.model_image = png.as_deref().and_then(decoded_image); }
                    Err(error) => preview.error = Some(crate::localized_format!("无法预览此文件：{}", "Could not preview this file: {}", crate::settings::diagnostic(&error))),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(crate) fn close_media_preview(&mut self, cx: &mut Context<Self>) {
        self.media_preview = None;
        cx.notify();
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
        let width = (f32::from(window.viewport_size().width) - 48.).clamp(320., 1100.);
        let height = (f32::from(window.viewport_size().height) - 48.).clamp(320., 780.);
        let title = state.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut body = div().flex_1().min_h_0().min_w_0().flex().items_center().justify_center().p(px(18.));
        match state.kind {
            MediaKind::Image => {
                let view_w = (width - 48.) * state.zoom;
                let view_h = (height - 150.) * state.zoom;
                body = body.child(div().id("media-image-box").w(px(width - 48.)).h(px(height - 150.)).overflow_hidden().flex().items_center().justify_center().cursor_pointer()
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
        let mut content = crate::trash::dialog("media-preview").relative().w(px(width)).h(px(height)).flex().flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|w, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" { w.close_media_preview(cx); cx.stop_propagation(); }
            }))
            .child(div().px(px(24.)).pt(px(20.)).pb(px(6.)).flex().items_center().gap(px(10.))
                .child(div().flex_1().min_w_0().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().flex_none().text_size(px(11.)).text_color(Theme::text_caption()).child(kind_label(state.kind))))
            .child(div().px(px(24.)).pb(px(8.)).text_size(px(12.)).text_color(Theme::text_caption())
                .child(format_bytes(std::fs::metadata(&state.path).map(|meta| meta.len()).unwrap_or(0))))
            .child(body);
        content = content.child(crate::trash::close_button("media-close").absolute().top(px(18.)).right(px(20.))
            .on_click(cx.listener(|w, _, _, cx| w.close_media_preview(cx))));
        let mut footer = div().px(px(24.)).pb(px(16.)).flex().items_center().gap(px(10.))
            .child(crate::settings::button("media-open-external", crate::settings::tr("使用系统应用打开", "Open with system app"))
                .on_click(cx.listener(|w, _, _, cx| w.open_media_externally(cx))));
        match state.kind {
            MediaKind::Image | MediaKind::Model => {
                footer = footer
                    .child(zoom_button("media-zoom-out", "−").on_click(cx.listener(|w, _, _, cx| {
                        if let Some(preview) = &mut w.media_preview { preview.zoom = (preview.zoom / 1.25).max(MIN_ZOOM); if preview.kind == MediaKind::Model { w.rerender_media_model(cx); } }
                        cx.notify();
                    })))
                    .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(format!("{:.0}%", state.zoom * 100.)))
                    .child(zoom_button("media-zoom-in", "+").on_click(cx.listener(|w, _, _, cx| {
                        if let Some(preview) = &mut w.media_preview { preview.zoom = (preview.zoom * 1.25).min(MAX_ZOOM); if preview.kind == MediaKind::Model { w.rerender_media_model(cx); } }
                        cx.notify();
                    })));
            }
            MediaKind::Audio | MediaKind::Video => {}
        }
        content = content.child(footer);
        Some(crate::transfer::mask("media-mask", content, |w, cx| w.close_media_preview(cx), cx))
    }
}

fn zoom_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).w(px(26.)).h(px(24.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
        .text_size(px(14.)).text_color(Theme::text()).hover(|style| style.bg(Theme::fill_hover())).child(label)
}

#[cfg(test)]
mod tests {
    use super::{media_kind, MediaKind};
    use std::path::Path;

    fn kind(name: &str) -> Option<MediaKind> { media_kind(Path::new(name)) }

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
        assert_eq!(kind("笔记.md"), None);
        assert_eq!(kind("表格.xlsx"), None);
        assert_eq!(kind("书.mdxb"), None);
        assert_eq!(kind("数据.game"), None);
        assert_eq!(kind("README"), None);
    }
}
