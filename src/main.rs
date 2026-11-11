#![recursion_limit = "512"]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

pub(crate) const APP_ID: &str = "com.neko233.markdownx";

mod app;
mod shared_store;
mod quick_note;
mod assets;
mod changelog;
mod guides;
mod clipboard;
#[cfg(feature = "devtools")]
mod devtools;
/// Without the `devtools` feature (release builds) nothing reads FDL_SCRIPT.
#[cfg(not(feature = "devtools"))]
mod devtools {
    pub static RENDER_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub fn scripted() -> bool { false }
    pub fn start(_: &mut gpui::Window, _: &mut gpui::App) {}
    pub fn take_pick() -> Option<Vec<std::path::PathBuf>> { None }
}
mod docinfo;
mod docmenu;
mod editor;
mod highlight;
mod home;
mod html;
mod input;
mod logging;
mod modes;
mod markdown_source;
#[cfg(target_os = "macos")]
mod native_header;
#[cfg(not(target_os = "macos"))]
#[path = "native_header_fallback.rs"]
mod native_header;
mod printing;
mod search;
mod sidebar;
mod settings;
mod themes;
mod shortcuts;
#[allow(dead_code, unused_imports)]
mod feishu_theme;
#[allow(dead_code, unused_imports)]
mod editor_theme;
#[allow(dead_code, unused_imports)]
mod editor_ui;
mod theme;
mod transfer;
mod trash;
mod tree;
mod ui;
mod workspace_fs;
mod git_workspace;
mod update;

use fdl_core::{docxml, lang, markdown, model, palette, paste, store, time};

use gpui::*;

#[cfg(target_os = "macos")]
fn set_app_icon() {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    let png = include_bytes!("../assets/branding/app-icon.png");
    unsafe {
        let data: *mut Object = msg_send![class!(NSData), dataWithBytes:png.as_ptr() length:png.len()];
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData:data];
        if !image.is_null() {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setApplicationIconImage:image];
            let _: () = msg_send![image, release];
        }
    }
}

/// A folder or supported document argument opens once in the first main window.
fn startup_target_from(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<std::path::PathBuf> {
    args.into_iter().map(std::path::PathBuf::from).find(|p| {
        !p.to_string_lossy().starts_with('-')
            && (p.is_dir() || (workspace_fs::DocKind::of(p).is_some() && p.is_file()))
    })
}

fn startup_target() -> Option<std::path::PathBuf> {
    static TARGET: std::sync::OnceLock<std::sync::Mutex<Option<std::path::PathBuf>>> = std::sync::OnceLock::new();
    TARGET.get_or_init(|| std::sync::Mutex::new(startup_target_from(std::env::args_os().skip(1))))
        .lock()
        .ok()
        .and_then(|mut target| target.take())
}

/// The main window, centred at the remembered size.
fn open_main_window(scripted: bool, cx: &mut App) {
    // Scripted dev runs must not steal keyboard focus from whatever the
    // user is typing into.
    let bounds = Bounds::centered(None, quick_note::main_window_size(cx), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: crate::theme::window_background(),
            app_id: Some(crate::APP_ID.into()),
            window_decorations: if cfg!(target_os = "linux") { Some(WindowDecorations::Client) } else { None },
            titlebar: Some(TitlebarOptions {
                title: Some("MarkdownX".into()),
                appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                traffic_light_position: Some(point(px(20.), px(app::HEADER_H / 2. - 7.))),
            }),
            window_min_size: Some(size(px(480.), px(480.))),
            focus: !scripted,
            ..Default::default()
        },
        |window, cx| {
            devtools::start(window, cx);
            let workspace = cx.new(|cx| app::Workspace::new(window, cx));
            let test_startup = cfg!(feature = "devtools") && scripted && std::env::var_os("MARKDOWNX_TEST_STARTUP_WORKSPACE").is_some();
            if !scripted || test_startup {
                workspace.update(cx, |w, cx| {
                    let documents = if test_startup { std::env::var_os("MARKDOWNX_TEST_DOCUMENTS_DIR").map(std::path::PathBuf::from) } else { workspace_fs::documents_dir() };
                    w.open_initial_workspace(documents, window, cx);
                    if let Some(path) = startup_target() {
                        if path.is_dir() { w.open_folder_tree_path(path, window, cx); }
                        else { w.open_external_path(path, window, cx); }
                    }
                });
            }
            workspace
        },
    )
    .expect("open window");
}

fn main() {
    logging::init();
    let scripted = devtools::scripted();
    // No single-instance guard: copies may share a library (builds tested
    // side by side). `Store` rereads an index another copy rewrote before
    // writing its own, and `guides` seeds a new library only once.
    let root = store::Store::root_dir();
    logging::write(&format!("library {}", root.display()));
    // A library that was here before this launch can show 更新日志.
    let existed = guides::seeded(&root).unwrap_or(false);
    if !scripted && let Err(err) = guides::initialize(&root) {
        logging::fatal(&format!("无法初始化文档库 / Could not set up the library at {}", root.display()), &err);
    }
    let app = Application::new().with_assets(assets::Assets);
    // macOS keeps running after ⌘W closes the last window (quick notes stay
    // available); clicking the Dock icon then brings the main window back.
    app.on_reopen(|cx| if cx.windows().is_empty() { open_main_window(false, cx); });
    app.run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        set_app_icon();
        editor::math::load_fonts(cx);
        input::bind_keys(cx);
        editor::keymap::bind_keys(cx);
        editor::image::bind_keys(cx);
        app::bind_keys(cx);
        settings::load();
        settings::apply_appearance(&settings::prefs().appearance);
        settings::sync_dark(cx);
        themes::load(&settings::prefs().theme_id(), false, || cx.text_system().all_font_names());
        changelog::startup(existed);
        shortcuts::init(cx);
        match shared_store::SharedStore::open() {
            Ok(s) => cx.set_global(s),
            Err(err) => logging::fatal("无法打开文档库 / Could not open the library", &err),
        }
        let quick_enabled = !scripted;
        if quick_enabled { quick_note::install(cx); }
        app::set_menus(cx);
        update::init(cx);
        open_main_window(scripted, cx);
        cx.on_window_closed(move |cx| {
            if (!quick_enabled || !cfg!(target_os = "macos")) && cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if !scripted {
            cx.activate(true);
        }
    });
}
mod annotations_panel;
mod workspaces;
mod workspace_files;

#[cfg(test)]
mod startup_tests {
    use super::startup_target_from;

    #[test]
    fn accepts_existing_folder_or_book_argument() {
        let dir = std::env::temp_dir().join(format!("markdownx-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let book = dir.join("book.mdxb");
        std::fs::write(&book, "{}").unwrap();
        let note = dir.join("note.MD");
        std::fs::write(&note, "note").unwrap();
        assert_eq!(startup_target_from([dir.as_os_str().to_os_string()]), Some(dir.clone()));
        assert_eq!(startup_target_from([book.as_os_str().to_os_string()]), Some(book));
        assert_eq!(startup_target_from([note.as_os_str().to_os_string()]), Some(note));
        assert_eq!(startup_target_from([dir.join("missing").into_os_string()]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
