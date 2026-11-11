//! Workspace window: two panes — the document list on the left
//! (see `sidebar.rs`) and the editor on the right, with the
//! 64px top bar (breadcrumb title + save status) above the page.

use crate::docinfo::DocInfo;
use crate::docmenu::DocSub;
use crate::editor::view::{Editor, EditorEvent};
use crate::input::{TextField, TextFieldEvent};
use crate::settings::display_title;
use crate::store::Entry;
use crate::shared_store::SharedStore;
use crate::theme::{self, Theme, craft};
use crate::ui::{TipExt, TitleBarExt, ToastKind, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Autosave waits for this pause in editing, but never longer than
/// SAVE_MAX_DELAY after the oldest change not yet on disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);
const SAVE_MAX_DELAY: Duration = Duration::from_secs(2);

actions!(workspace, [NewDoc, ImportDoc, OpenFolder, OpenFile, PackageBook, ToggleSidebar, FocusSearch, GlobalSearch, FindInDoc, ReplaceInDoc, OpenSettings, OpenAbout, CheckForUpdates, GoHome, CloseWindow, Quit,
    HideApp, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

/// The menu bar, in the interface language.
pub fn set_menus(cx: &mut App) {
    use crate::settings::tr;
    cx.set_menus(vec![
        Menu {
            name: "MarkdownX".into(),
            items: vec![
                MenuItem::action(tr("关于 MarkdownX", "About MarkdownX"), OpenAbout),
                MenuItem::action(tr("检查更新…", "Check for Updates…"), CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action(tr("设置…", "Settings…"), OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu(tr("服务", "Services"), SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(tr("隐藏 MarkdownX", "Hide MarkdownX"), HideApp),
                MenuItem::action(tr("隐藏其他", "Hide Others"), HideOthers),
                MenuItem::action(tr("全部显示", "Show All"), ShowAll),
                MenuItem::separator(),
                MenuItem::action(tr("退出 MarkdownX", "Quit MarkdownX"), Quit),
            ],
        },
        Menu {
            name: tr("文件", "File").into(),
            items: vec![
                MenuItem::action(tr("新建文档", "New Document"), NewDoc),
                MenuItem::action(tr("新建笔记窗口", "New Note Window"), crate::quick_note::QuickNew),
                MenuItem::action(tr("导入为文档…", "Import as Document…"), ImportDoc),
                MenuItem::action(tr("打开文件夹…", "Open Folder…"), OpenFolder),
                MenuItem::action(tr("打包成电子书…", "Package as E-book…"), PackageBook),
                MenuItem::action(tr("打开文件…", "Open File…"), OpenFile),
                MenuItem::separator(),
                MenuItem::action(tr("关闭窗口", "Close Window"), CloseWindow),
            ],
        },
        // The standard Edit menu: its items act on the focused editor, and
        // macOS adds 听写 and 表情与符号 to it.
        Menu {
            name: tr("编辑", "Edit").into(),
            items: {
                use crate::editor::keymap as k;
                vec![
                    MenuItem::os_action(tr("撤销", "Undo"), k::Undo, OsAction::Undo),
                    MenuItem::os_action(tr("重做", "Redo"), k::Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action(tr("剪切", "Cut"), k::Cut, OsAction::Cut),
                    MenuItem::os_action(tr("拷贝", "Copy"), k::Copy, OsAction::Copy),
                    MenuItem::os_action(tr("粘贴", "Paste"), k::Paste, OsAction::Paste),
                    MenuItem::os_action(tr("全选", "Select All"), k::SelectAll, OsAction::SelectAll),
                ]
            },
        },
        Menu {
            name: tr("窗口", "Window").into(),
            items: vec![
                MenuItem::action(tr("最小化", "Minimize"), Minimize),
                MenuItem::action(tr("缩放", "Zoom"), Zoom),
                MenuItem::separator(),
                MenuItem::action(tr("进入 / 退出全屏幕", "Enter / Exit Full Screen"), ToggleFullScreen),
            ],
        },
    ]);
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewDoc, None),
        KeyBinding::new("cmd-shift-n", crate::quick_note::QuickNew, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-i", ImportDoc, None),
        KeyBinding::new("cmd-shift-o", OpenFolder, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-shift-e", PackageBook, None),
        KeyBinding::new("cmd-alt-home", GoHome, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-]", ToggleSidebar, None),
        KeyBinding::new("cmd-\\", ToggleSidebar, None),
        KeyBinding::new("cmd-j", FocusSearch, None),
        KeyBinding::new("cmd-k", GlobalSearch, None),
        KeyBinding::new("cmd-f", FindInDoc, None),
        KeyBinding::new("cmd-shift-h", ReplaceInDoc, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // macOS app and window keys. ⌥⌘H (隐藏其他) stays with the editor's
    // 应用上次的颜色, the Feishu shortcut; 隐藏其他 is in the menu. On Windows
    // the system handles these (Win+↓, Alt+F4), and F11 is full screen.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    // Windows: F11 for full screen, and Ctrl+H for 查找和替换 as in Office
    // and Notepad (written cmd-h: `shortcuts::pc_binding` maps cmd to
    // ctrl and drops a literal ctrl-h, Cocoa's backspace).
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None), KeyBinding::new("cmd-h", ReplaceInDoc, None)]);
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(handle) = cx.active_window().and_then(|w| w.downcast::<Workspace>()) {
            // Key dispatch already borrows this window; close after it returns.
            cx.defer(move |cx| {
                if let Err(e) = handle.update(cx, |ws, window, cx| {
                    crate::quick_note::persist(cx);
                    if ws.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
                }) { eprintln!("无法关闭笔记窗口：{e}"); }
            });
        }
    });
    cx.on_action(|_: &Quit, cx| quit(cx));
    crate::modes::bind_keys(cx);
}

/// 退出: the open document is saved first; when that fails its dialog asks
/// (重试 / 放弃修改) and quits afterwards. Call it outside any window update
/// (`cx.defer`): a window on the update stack cannot be asked to save.
pub fn quit(cx: &mut App) {
    let ready = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        .map(|w| w.update(cx, |ws, _, cx| ws.save_before(cx, |_, _, cx| cx.defer(quit))).unwrap_or(false))
        .fold(true, |ready, saved| ready & saved);
    if ready {
        cx.quit();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SaveStatus {
    /// Freshly opened, nothing saved this session: show the modification time.
    Idle,
    Saved,
    Saving,
    Failed(String),
}

/// Right-click / "…" menu on a sidebar row.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub id: String,
    pub pos: Point<Pixels>,
    /// Which corner of the menu sits at `pos`.
    pub corner: Corner,
    /// 下载 submenu open.
    pub sub: bool,
    /// The 批量操作 menu for the checked rows.
    pub batch: bool,
}

/// What the main pane shows: the open document, 主页 or 回收站.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Doc,
    Home,
    Trash,
    Settings,
}

/// A confirmation dialog and the pages it is about.
#[derive(Clone, Debug)]
pub enum Confirm {
    /// 删除 (to the 回收站); `batch` for the 批量操作 selection.
    Delete { ids: Vec<String>, batch: bool },
    /// 回收站 → 恢复.
    Restore(String),
    /// 回收站 → 彻底删除.
    Purge(String),
    /// 回收站 → 清空回收站: every trashed page, permanently.
    EmptyTrash,
    /// 隐藏引用列表 (with 不再提示) / 显示引用列表.
    Refs { show: bool, no_ask: bool },
    /// The open document could not be written before it was to be replaced
    /// (another document, 新建, 退出): 重试 / 放弃修改, then carry on.
    SaveFailed,
}

/// What to do once the open document is safely on disk (or its changes given up).
pub type AfterSave = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// Inline 重命名 of a sidebar row.
pub struct Rename {
    pub id: String,
    pub field: Entity<TextField>,
    _subs: Vec<Subscription>,
}

pub struct Workspace {
    pub quick_note: bool,
    pub shortcut_recording: Option<String>,
    pub shortcut_strokes: Vec<String>,
    /// The failed change: (action, message); an empty action is page-wide.
    pub shortcut_error: Option<(String, String)>,
    pub settings_page: crate::settings::Page,
    /// 外观's theme dropdown while open: the theme its preview shows.
    pub theme_menu: Option<String>,
    /// Where the theme dropdown's button was drawn last frame.
    pub theme_anchor: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// 快捷键's filter field.
    pub settings_filter: Entity<TextField>,
    native_sidebar: crate::native_header::NativeSidebar,
    pub store: SharedStore,
    pub entries: Vec<Entry>,
    pub current: Option<String>,
    /// A file opened from a folder workspace; `current` stays None then.
    pub file: Option<std::path::PathBuf>,
    /// Bytes last read or saved, used to detect changes made by another app.
    file_snapshot: Option<String>,
    file_source: Option<crate::markdown_source::Source>,
    /// File saves run off the UI thread; revisions keep edits made during a save.
    file_saving: bool,
    file_revision: u64,
    window_handle: AnyWindowHandle,
    pub folder: Option<crate::workspace_fs::WorkspaceDir>,
    pub folder_query: Entity<TextField>,
    pub folder_content: bool,
    pub folder_hits: Vec<crate::workspace_fs::Hit>,
    pub folder_searching: bool,
    folder_search_generation: u64,
    folder_search_task: Option<Task<()>>,
    pub folder_expanded: HashSet<std::path::PathBuf>,
    /// `[[wiki links]]` across the open folder, used for the backlink panel.
    pub folder_links: fdl_core::wiki::Index,
    folder_links_generation: u64,
    folder_links_loading: bool,
    /// English chapter bodies from the last imported `.mdxb`, if any.
    pub book_english: Vec<Option<String>>,
    /// Root page id those bodies belong to; prevents leaking into other books.
    pub book_english_id: Option<String>,
    /// Temporary annotations for the open page, stored beside its source.
    pub annotations: fdl_core::annotations::AnnotationStore,
    pub annotations_path: Option<std::path::PathBuf>,
    pub annotations_open: bool,
    pub annotation_draft: Entity<TextField>,
    pub annotation_author: fdl_core::annotations::Author,
    pub annotation_selected: HashSet<String>,
    pub annotation_editing: Option<String>,
    pub editor: Option<Entity<Editor>>,
    pub search: Entity<TextField>,
    /// Global search includes the open folder as well as library documents.
    pub(crate) global_search: bool,
    pub(crate) global_folder_hits: Vec<crate::workspace_fs::Hit>,
    pub(crate) global_folder_searching: bool,
    pub(crate) global_folder_generation: u64,
    pub(crate) global_folder_task: Option<Task<()>>,
    /// Keyboard highlight inside search results.
    pub search_sel: Option<usize>,
    pub sidebar_open: bool,
    /// Dragged width (300 by default, 268 to min(800, window − 540)).
    pub sidebar_width: f32,
    /// The resize handle is being dragged.
    pub sidebar_drag: bool,
    /// The search dialog (command-center style) is open.
    pub search_open: bool,
    pub status: SaveStatus,
    pub row_menu: Option<RowMenu>,
    pub rename: Option<Rename>,
    /// Last-frame bounds of each row's "…" button, to hang its menu from.
    pub more_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// Header "…" menu and its open submenu.
    pub doc_menu: bool,
    pub doc_sub: Option<DocSub>,
    /// Last-frame bounds of the open submenu (clicks there keep the menu open).
    pub doc_sub_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub doc_info: Option<DocInfo>,
    pub list_scroll: ScrollHandle,
    pub search_scroll: ScrollHandle,
    /// Last-frame bounds of the row menu's 下载 submenu.
    pub row_sub_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// 批量操作 mode: the checked docs of 我的文档, in check order.
    pub batch: Option<Vec<String>>,
    /// The open confirmation dialog.
    pub confirm: Option<Confirm>,
    pub view: View,
    pub home: crate::home::Home,
    pub settings_scroll: ScrollHandle,
    /// 回收站 rows, refreshed when the page opens or changes.
    pub trash: Vec<crate::store::TrashEntry>,
    /// A 回收站 row's menu: the page and where it opens.
    pub trash_menu: Option<(String, Point<Pixels>, Corner)>,
    /// The toast and when it appeared.
    pub toast: Option<(ToastKind, SharedString, Instant)>,
    toast_task: Option<Task<()>>,
    /// Folded sidebar sections: 置顶, 我的文档.
    pub folded: [bool; 2],
    /// Expanded pages of the 我的文档 tree.
    pub expanded: HashSet<String>,
    pub move_dlg: Option<crate::tree::MoveDialog>,
    /// 导出 Markdown 设置.
    pub export_md: Option<crate::transfer::ExportMd>,
    /// 编辑 / 阅读 dropdown, and the mode button's last-frame bounds.
    pub mode_menu: bool,
    pub mode_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The header "…" button, which the doc menu hangs from.
    pub more_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The 快捷键 drawer, and the help button's menu (open while hovered).
    pub shortcuts: bool,
    pub(crate) focus: FocusHandle,
    save_task: Option<Task<()>>,
    /// When the oldest edit not yet on disk was made: a save waits for a
    /// pause in typing, but never longer than `SAVE_MAX_DELAY` after it.
    dirty_since: Option<Instant>,
    /// The open document has changes not yet on disk (cleared only by a
    /// successful save or by 放弃修改).
    unsaved: bool,
    /// Held back by a failed save: runs after 重试 succeeds or 放弃修改.
    pub(crate) after_save: Option<AfterSave>,
    editor_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}


/// A macOS notification (only logged when scripted).
fn notify_system(title: &str, body: &str) {
    if crate::devtools::scripted() {
        return eprintln!("notification: {title} / {body}");
    }
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", q(body), q(title));
    std::thread::spawn(move || std::process::Command::new("osascript").arg("-e").arg(script).status());
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_window(false, window, cx)
    }

    pub fn new_window(quick: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build_window(quick, None, window, cx)
    }

    pub fn new_document_window(document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build_window(true, document, window, cx)
    }

    fn build_window(quick: bool, document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::settings::load();
        if !cx.has_global::<SharedStore>() { cx.set_global(SharedStore::open().expect("open document library")); }
        let mut store = cx.global::<SharedStore>().clone();
        let entries = store.list();
        let search = cx.new(|cx| {
            let mut f = TextField::new("搜索", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let settings_filter = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("搜索快捷键", "Search shortcuts"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let folder_query = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("搜索文件…", "Search files…"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![
            cx.subscribe(&settings_filter, |_, _, ev: &TextFieldEvent, cx| if matches!(ev, TextFieldEvent::Changed) { cx.notify(); }),
            cx.subscribe_in(&folder_query, window, |this, _, ev: &TextFieldEvent, _, cx| {
                if matches!(ev, TextFieldEvent::Changed) {
                    this.refresh_folder_hits(cx);
                }
            }),
            cx.observe_window_bounds(window, |this, window, cx| crate::quick_note::remember(window, this.quick_note, cx)),
            // The system switched between light and dark (or the app's own look changed).
            cx.observe_window_appearance(window, |this, _, cx| if crate::settings::sync_dark(cx) { crate::settings::refresh_all(this, cx); }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() { if let Some(id) = &this.current { crate::quick_note::used(id, cx); } this.entries = this.store.list(); this.sort(); this.refresh_folder(cx); crate::update::check_if_stale(cx); cx.notify(); }
                else { this.cancel_shortcut_recording(cx); this.flush(cx); }
            }),
            cx.subscribe_in(&search, window, |this, _, ev: &TextFieldEvent, window, cx| {
                match ev {
                    TextFieldEvent::Changed => {
                        this.refresh_global_folder_hits(cx);
                        this.search_sel = this.default_search_sel(cx);
                        this.search_scroll.set_offset(point(px(0.), px(0.)));
                    }
                    TextFieldEvent::Up => this.move_search_sel(-1, cx),
                    TextFieldEvent::Down => this.move_search_sel(1, cx),
                    TextFieldEvent::Submit => {
                        if let Some(ix) = this.search_sel {
                            this.pick_search(ix, window, cx)
                        }
                    }
                    TextFieldEvent::Cancel => this.close_search(window, cx),
                    _ => {}
                }
                cx.notify();
            }),
            // Last resort (the system logging out): ⌘Q and the close button
            // go through `save_before` and ask when the save fails.
            cx.on_app_quit(|this, cx| {
                crate::quick_note::persist(cx);
                if !this.flush(cx) && let Some(ed) = &this.editor
                    && let Err(error) = this.store.recover_later(&ed.read(cx).core.doc) {
                    crate::logging::write(&format!("Could not preserve unsaved document: {error:#}"));
                }
                async {}
            }),
        ];
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            crate::quick_note::persist(cx);
            this.update(cx, |w, cx| { w.cancel_shortcut_recording(cx); w.save_before(cx, |_, window, _| window.remove_window()) }).unwrap_or(true)
        });
        if !quick { crate::settings::watch_themes(cx); }
        // 日期提醒: post what fell due since the last look (not what was
        // missed while the app was closed).
        cx.spawn(async |this, cx| {
            let mut last = crate::time::now_ms() as i64;
            loop {
                cx.background_executor().timer(Duration::from_secs(15)).await;
                let now = crate::time::now_ms() as i64;
                if this.update(cx, |w, cx| w.check_reminders(last, now, cx)).is_err() {
                    break;
                }
                last = now;
            }
        })
        .detach();
        let sidebar_width = store.settings().sidebar_width.unwrap_or(theme::SIDEBAR_WIDTH);
        let annotation_draft = cx.new(|cx| {
            let mut f = TextField::new("写一条批注…", cx);
            f.multiline = true;
            f.wrap = true;
            f.size = 13.;
            f.line_height = 20.;
            f
        });
        let annotation_name = std::env::var("MARKDOWNX_AUTHOR").or_else(|_| std::env::var("USERNAME")).or_else(|_| std::env::var("USER")).unwrap_or_else(|_| {
            if crate::settings::english() { "Reader".into() } else { "读者".into() }
        });
        let mut ws = Workspace {
            quick_note: quick,
            shortcut_recording: None,
            shortcut_strokes: vec![],
            shortcut_error: None,
            settings_page: Default::default(),
            theme_menu: None,
            theme_anchor: Default::default(),
            settings_filter,
            store,
            entries,
            current: None,
            file: None,
            file_snapshot: None,
            file_source: None,
            file_saving: false,
            file_revision: 0,
            window_handle: window.window_handle(),
            folder: None,
            folder_query,
            folder_content: false,
            folder_hits: vec![],
            folder_searching: false,
            folder_search_generation: 0,
            folder_search_task: None,
            folder_expanded: HashSet::new(),
            folder_links: fdl_core::wiki::Index::default(),
            folder_links_generation: 0,
            folder_links_loading: false,
            book_english: Vec::new(),
            book_english_id: None,
            annotations: fdl_core::annotations::AnnotationStore::new(""),
            annotations_path: None,
            annotations_open: false,
            annotation_draft,
            annotation_author: fdl_core::annotations::Author::new(annotation_name),
            annotation_selected: HashSet::new(),
            annotation_editing: None,
            editor: None,
            search,
            global_search: false,
            global_folder_hits: vec![],
            global_folder_searching: false,
            global_folder_generation: 0,
            global_folder_task: None,
            search_sel: None,
            sidebar_open: !quick,
            sidebar_width,
            sidebar_drag: false,
            search_open: false,
            status: SaveStatus::Idle,
            row_menu: None,
            rename: None,
            more_bounds: Default::default(),
            doc_menu: false,
            doc_sub: None,
            doc_sub_bounds: Rc::new(Cell::new(None)),
            doc_info: None,
            list_scroll: ScrollHandle::new(),
            search_scroll: ScrollHandle::new(),
            row_sub_bounds: Rc::new(Cell::new(None)),
            batch: None,
            confirm: None,
            view: View::Doc,
            home: Default::default(),
            settings_scroll: ScrollHandle::new(),
            trash: vec![],
            trash_menu: None,
            toast: None,
            toast_task: None,
            mode_menu: false,
            mode_btn_bounds: Rc::new(Cell::new(None)),
            more_btn_bounds: Rc::new(Cell::new(None)),
            native_sidebar: Default::default(),
            shortcuts: false,
            folded: [false; 2],
            expanded: HashSet::new(),
            move_dlg: None,
            export_md: None,
            focus: cx.focus_handle(),
            save_task: None,
            dirty_since: None,
            unsaved: false,
            after_save: None,
            editor_sub: None,
            _subs: subs,
        };
        ws.sort();
        ws.load_home();
        // An empty library opens on 主页's empty state; only a quick note
        // window starts a document of its own.
        match document.or_else(|| (!quick).then(|| ws.entries.first().map(|e| e.id.clone())).flatten()) {
            Some(id) => ws.open(&id, window, cx),
            None if quick => ws.new_doc(window, cx),
            None => { ws.view = View::Home; window.focus(&ws.focus); }
        }
        // Kept in the top bar (not just a toast): the user should notice.
        let warnings = ws.store.take_warnings();
        if !warnings.is_empty() {
            let msg = warnings.iter().map(|s| crate::settings::diagnostic(s)).collect::<Vec<_>>().join(crate::settings::tr("；", "; "));
            ws.show_toast_kind(ToastKind::Error, &msg, cx);
            ws.status = SaveStatus::Failed(msg);
        }
        ws
    }

    /// Most recently modified first.
    pub(crate) fn sort(&mut self) {
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.meta.modified));
    }

    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).text.trim().to_lowercase()
    }

    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.try_open(id, window, cx);
    }

    /// Open an existing Markdown file in place, with its folder tree visible.
    pub fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions {
            files: true, directories: false, multiple: false,
            prompt: Some(crate::settings::tr("打开文件", "Open file").into()),
            extensions: Some(["md", "markdown", "mark", "txt", "mdxb"].into_iter().map(SharedString::from).collect()),
        }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt { Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(path) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| w.open_external_path(path, window, cx));
        }).detach();
    }

    pub fn open_external_path(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = if path.is_absolute() { path } else { std::env::current_dir().unwrap_or_default().join(path) };
        if crate::workspace_fs::DocKind::of(&path) == Some(crate::workspace_fs::DocKind::Book) {
            self.open_book_path(path, window, cx);
            return;
        }
        if crate::workspace_fs::DocKind::of(&path) != Some(crate::workspace_fs::DocKind::Markdown) || !path.is_file() {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("请选择 Markdown 或文本文件", "Choose a Markdown or text file"), cx);
            return;
        }
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.open_external_path_now(retry, window, cx)) {
            self.open_external_path_now(path, window, cx);
        }
    }

    fn open_external_path_now(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let canonical = path.canonicalize().ok();
        let existing = self.folder.as_ref().and_then(|folder| folder.files().into_iter().find(|node|
            node.path == path || canonical.as_ref().is_some_and(|p| node.path.canonicalize().ok().as_ref() == Some(p))
        ).map(|node| node.path.clone()));
        let path = if let Some(existing) = existing { existing } else {
            let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else { return };
            let root = crate::git_workspace::repo_root(parent).unwrap_or_else(|| parent.to_path_buf());
            let dir = match crate::workspace_fs::WorkspaceDir::scan(root) {
                Ok(dir) => dir,
                Err(e) => { self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法打开文件夹：{}", "Could not open folder: {}", e), cx); return; }
            };
            let Some(found) = dir.files().into_iter().find(|node|
                node.path == path || canonical.as_ref().is_some_and(|p| node.path.canonicalize().ok().as_ref() == Some(p))
            ).map(|node| node.path.clone()) else {
                self.show_toast_kind(ToastKind::Error, crate::settings::tr("文件不在可显示的目录中", "The file is not in a visible folder"), cx);
                return;
            };
            self.folder_search_generation = self.folder_search_generation.wrapping_add(1);
            self.folder_search_task = None;
            self.folder_searching = false;
            self.folder = Some(dir);
            self.refresh_global_folder_hits(cx);
            self.folder_expanded.clear();
            self.folder_hits.clear();
            self.folder_query.update(cx, |q, cx| q.set_text("", cx));
            self.refresh_folder_links(cx);
            self.sidebar_open = true;
            found
        };
        if let Some(root) = self.folder.as_ref().map(|f| f.root.clone()) {
            let mut parent = path.parent();
            while let Some(dir) = parent {
                if dir == root || !dir.starts_with(&root) { break; }
                self.folder_expanded.insert(dir.to_path_buf());
                parent = dir.parent();
            }
        }
        self.load_file(path, window, cx);
    }

    /// Create a new file without ever replacing an existing one.
    pub fn new_folder_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.folder.as_ref().map(|f| f.root.clone()) else { return };
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&root, Some("Untitled.md")));
        cx.spawn_in(window, async move |this, cx| {
            let path = match prompt { Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(mut path) = path else { return };
            if path.extension().is_none() { path.set_extension("md"); }
            let _ = this.update_in(cx, |w, window, cx| {
                let retry = path.clone();
                if w.save_before(cx, move |w, window, cx| w.new_folder_file_now(retry, window, cx)) {
                    w.new_folder_file_now(path, window, cx);
                }
            });
        }).detach();
    }

    fn new_folder_file_now(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let allowed = self.folder.as_ref().and_then(|f| f.root.canonicalize().ok()).zip(path.parent().and_then(|p| p.canonicalize().ok()))
            .is_some_and(|(root, parent)| parent.starts_with(root));
        if !allowed || crate::workspace_fs::DocKind::of(&path) != Some(crate::workspace_fs::DocKind::Markdown) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("请选择当前文件夹中的 Markdown 文件名", "Choose a Markdown filename inside the current folder"), cx);
            return;
        }
        if let Err(e) = std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法新建文件：{}", "Could not create file: {}", e), cx);
            return;
        }
        self.refresh_folder(cx);
        self.open_external_path_now(path, window, cx);
    }

    /// Pick a normal folder (Typora's 打开文件夹) and show its tree.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::settings::text("打开文件夹").into()),
            extensions: None,
        }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt { Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(path) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| w.open_folder_path(path, window, cx));
        })
        .detach();
    }

    pub fn open_folder_path(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_folder_with_selection(path, true, window, cx);
    }

    /// Startup can show the tree without opening an arbitrary first document.
    pub fn open_folder_tree_path(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_folder_with_selection(path, false, window, cx);
    }

    fn open_folder_with_selection(&mut self, path: std::path::PathBuf, select_first: bool, window: &mut Window, cx: &mut Context<Self>) {
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.open_folder_path_now(retry, select_first, window, cx)) {
            self.open_folder_path_now(path, select_first, window, cx);
        }
    }

    fn open_folder_path_now(&mut self, path: std::path::PathBuf, select_first: bool, window: &mut Window, cx: &mut Context<Self>) {
        // A team's workspace is the Git repository, not whatever nested
        // folder happened to be picked in the dialog.
        let path = crate::git_workspace::repo_root(&path).unwrap_or(path);
        let dir = match crate::workspace_fs::WorkspaceDir::scan(path) {
            Ok(d) => d,
            Err(e) => {
                self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法打开文件夹：{}", "Could not open folder: {}", crate::settings::diagnostic(&e.to_string())), cx);
                return;
            }
        };
        let first = select_first.then(|| dir.files().first().map(|f| f.path.clone())).flatten();
        let mut expanded = HashSet::new();
        let mut parent = first.as_deref().and_then(std::path::Path::parent);
        while let Some(path) = parent {
            if path == dir.root { break; }
            if !path.starts_with(&dir.root) { break; }
            expanded.insert(path.to_path_buf());
            parent = path.parent();
        }
        self.folder_search_generation = self.folder_search_generation.wrapping_add(1);
        self.folder_search_task = None;
        self.folder_searching = false;
        self.folder = Some(dir);
        self.refresh_global_folder_hits(cx);
        self.refresh_folder_links(cx);
        self.folder_expanded = expanded;
        self.folder_hits.clear();
        self.folder_query.update(cx, |q, cx| q.set_text("", cx));
        self.sidebar_open = true;
        self.list_scroll.set_offset(point(px(0.), px(0.)));
        self.current = None;
        self.file = None;
        self.file_snapshot = None;
        self.file_source = None;
        self.editor = None;
        self.view = View::Home;
        window.focus(&self.focus);
        if let Some(path) = first {
            self.load_file(path, window, cx);
        }
        self.show_folder_warnings(cx);
        cx.notify();
    }

    /// Close the folder tree and go back to the document library.
    pub fn close_folder(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.save_before(cx, |w, _, cx| w.close_folder_now(cx)) {
            self.close_folder_now(cx);
        }
    }

    fn close_folder_now(&mut self, cx: &mut Context<Self>) {
        self.folder = None;
        self.refresh_global_folder_hits(cx);
        self.folder_search_generation = self.folder_search_generation.wrapping_add(1);
        self.folder_search_task = None;
        self.folder_searching = false;
        self.folder_links_generation = self.folder_links_generation.wrapping_add(1);
        self.folder_links_loading = false;
        self.folder_hits.clear();
        self.folder_links = fdl_core::wiki::Index::default();
        self.folder_query.update(cx, |q, cx| q.set_text("", cx));
        self.file = None;
        self.file_snapshot = None;
        self.file_source = None;
        self.current = None;
        self.editor = None;
        self.show_view(View::Home, cx);
    }

    pub fn refresh_folder_hits(&mut self, cx: &mut Context<Self>) {
        self.folder_search_generation = self.folder_search_generation.wrapping_add(1);
        let generation = self.folder_search_generation;
        let query = self.folder_query.read(cx).text.clone();
        self.folder_hits.clear();
        self.folder_search_task = None;
        let Some(folder) = self.folder.clone().filter(|_| !query.trim().is_empty()) else {
            self.folder_searching = false;
            cx.notify();
            return;
        };
        let content = self.folder_content;
        self.folder_searching = true;
        self.folder_search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let task = cx.background_executor().spawn(async move { folder.search(&query, content) });
            let hits = task.await;
            let _ = this.update(cx, |w, cx| if w.folder_search_generation == generation {
                w.folder_hits = hits;
                w.folder_searching = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Re-scan without replacing the open editor or losing expanded folders.
    pub fn refresh_folder(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.folder.as_ref().map(|f| f.root.clone()) else { return };
        match crate::workspace_fs::WorkspaceDir::scan(root) {
            Ok(folder) => {
                let mut pending: Vec<_> = folder.children.iter().collect();
                let mut dirs = HashSet::new();
                while let Some(node) = pending.pop() {
                    if node.dir { dirs.insert(node.path.clone()); pending.extend(&node.children); }
                }
                self.folder_expanded.retain(|path| dirs.contains(path));
                self.folder = Some(folder);
                self.refresh_folder_links(cx);
                self.refresh_folder_hits(cx);
                self.refresh_global_folder_hits(cx);
                self.show_folder_warnings(cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法刷新文件夹：{}", "Could not refresh folder: {}", crate::settings::diagnostic(&e.to_string())), cx),
        }
    }

    fn show_folder_warnings(&mut self, cx: &mut Context<Self>) {
        let warning = self.folder.as_ref().and_then(|f| f.warnings.first().map(|(path, error)| (f.warnings.len(), path.display().to_string(), error.clone())));
        if let Some((count, path, error)) = warning {
            self.show_toast_kind(ToastKind::Error, &crate::localized_format!("有 {} 处无法读取：{}（{}）", "Could not read {} location(s): {} ({})", count, path, error), cx);
        }
    }

    /// Rebuild the folder-level `[[wiki]]` index. It reads Markdown only; a
    /// `.mdxb` book is a build artifact and keeps its links inside its chapters.
    pub fn refresh_folder_links(&mut self, cx: &mut Context<Self>) {
        self.folder_links_generation = self.folder_links_generation.wrapping_add(1);
        let generation = self.folder_links_generation;
        let Some(folder) = self.folder.as_ref() else {
            self.folder_links = fdl_core::wiki::Index::default();
            self.folder_links_loading = false;
            return;
        };
        let paths: Vec<_> = folder.files().into_iter().filter(|n| crate::workspace_fs::DocKind::of(&n.path) == Some(crate::workspace_fs::DocKind::Markdown)).map(|n| n.path.clone()).collect();
        self.folder_links_loading = true;
        let task = cx.background_executor().spawn(async move {
            let files = paths.into_iter().filter_map(|p| std::fs::read_to_string(&p).ok().map(|text| (p, text))).collect::<Vec<_>>();
            fdl_core::wiki::Index::build(files)
        });
        cx.spawn(async move |this, cx| {
            let index = task.await;
            let _ = this.update(cx, |w, cx| if w.folder_links_generation == generation {
                w.folder_links = index;
                w.folder_links_loading = false;
                cx.notify();
            });
        }).detach();
    }

    /// Follow a `[[wiki]]` link from the current folder note.
    fn open_wiki(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = match (self.folder.as_ref(), self.file.as_ref()) {
            (Some(_), Some(from)) => self.folder_links.resolve(from, target).map(|p| p.to_path_buf()),
            _ => None,
        };
        if let Some(path) = path {
            self.load_file(path, window, cx);
        } else {
            self.show_toast_kind(ToastKind::Error, if self.folder_links_loading { crate::settings::tr("正在更新双链索引", "Updating link index") } else { crate::settings::text("找不到双链目标") }, cx);
        }
    }

    pub(crate) fn load_file(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.file.as_ref() == Some(&path) && self.editor.is_some() {
            self.view = View::Doc;
            cx.notify();
            return;
        }
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.open_file_path(retry, window, cx)) {
            self.open_file_path(path, window, cx);
        }
    }

    /// Open one file from the folder tree. Markdown is edited in place;
    /// `.mdxb` goes through the library so its chapters keep their page tree.
    pub fn open_file_path(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match crate::workspace_fs::DocKind::of(&path) {
            Some(crate::workspace_fs::DocKind::Book) => {
                if let Ok(id) = self.import_book(&path) {
                    self.file = None;
                    self.file_snapshot = None;
                    self.file_source = None;
                    self.open(&id, window, cx);
                } else {
                    self.show_toast_kind(ToastKind::Error, crate::settings::text("无法打开这本书"), cx);
                }
            }
            Some(crate::workspace_fs::DocKind::Markdown) => {
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法读取文件：{}", "Could not read file: {}", crate::settings::diagnostic(&e.to_string())), cx);
                        return;
                    }
                };
                let (doc, source) = crate::markdown_source::parse(&path, &text);
                let modified = file_modified(&path);
                let editor = cx.new(|cx| {
                    let mut e = Editor::new(doc, modified, crate::store::PageWidth::default(), window, cx);
                    e.cat_pinned = true;
                    e
                });
                self.bind_editor(editor.clone(), window, cx);
                if !self.search_open { editor.read(cx).focus(window); }
                self.editor = Some(editor);
                self.current = None;
                self.file = Some(path);
                self.file_snapshot = Some(text);
                self.file_source = Some(source);
                self.view = View::Doc;
                self.status = SaveStatus::Idle;
                self.doc_menu = false;
                self.close_menus();
                self.reload_annotations(cx);
                cx.notify();
            }
            None => {}
        }
    }

    fn bind_editor(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_sub = Some(cx.subscribe_in(&editor, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => {
                // 阅读模式 put the document back: nothing to save.
                let rejected = this.editor.clone().is_some_and(|ed| ed.update(cx, |e, cx| e.reject_read_only_edit(cx)));
                if !rejected {
                    this.file_revision = this.file_revision.wrapping_add(1);
                    this.sync_document(window, cx);
                    this.schedule_save(window, cx);
                }
            }
            EditorEvent::Mode(reading) => this.set_mode(*reading, window, cx),
            EditorEvent::Info(info) => this.set_info_row(*info),
            EditorEvent::Toast => {
                this.toast = None;
                cx.notify();
            }
            EditorEvent::Save => {
                this.flush(cx);
            }
            EditorEvent::Search(q) => {
                this.open_search(window, cx);
                this.search.update(cx, |s, cx| s.set_text(q, cx));
                this.refresh_global_folder_hits(cx);
                this.search_sel = this.default_search_sel(cx);
            }
            EditorEvent::OpenWiki(target) => this.open_wiki(target, window, cx),
            EditorEvent::OpenDoc(id) => this.open(id, window, cx),
            EditorEvent::OpenDocAt(id, ordinal) => this.open_at(id, *ordinal, window, cx),
            EditorEvent::HideRefs => this.ask_refs(false, cx),
            EditorEvent::Duplicate => {
                if let Some(id) = this.current.clone() {
                    this.duplicate_doc(&id, window, cx);
                }
            }
            EditorEvent::Catalogue(pinned) => {
                if let Some(id) = this.current.clone() {
                    if let Some(e) = this.entries.iter_mut().find(|e| e.id == id) {
                        e.meta.catalogue_closed = !pinned;
                    }
                    let _ = this.store.set_catalogue(&id, *pinned);
                }
            }
        }));
    }

    /// Show `id`; false when it could not be opened (or the open document
    /// could not be saved first, see [`Self::save_before`]).
    pub fn try_open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.view != View::Doc {
            self.view = View::Doc;
            cx.notify();
        }
        if self.current.as_deref() == Some(id) {
            return true;
        }
        let target = id.to_string();
        if !self.save_before(cx, move |w, window, cx| {
            w.open(&target, window, cx);
        }) {
            return false;
        }
        let shared = !self.document_peers(id, window, cx).is_empty();
        let doc = match self.store.load_for_edit(id, shared) {
            Ok(d) => d,
            Err(e) => {
                self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法打开文档：{}", "Could not open document: {}", crate::settings::diagnostic(&e.to_string())), cx);
                cx.notify();
                return false;
            }
        };
        let meta = self.entries.iter().find(|e| e.id == id).map(|e| e.meta.clone());
        let modified = meta.as_ref().map_or_else(crate::time::now, |m| m.modified);
        let width = meta.as_ref().map(|m| m.width).unwrap_or_default();
        let pinned = meta.as_ref().is_some_and(|m| !m.catalogue_closed);
        let editor = cx.new(|cx| {
            let mut e = Editor::new(doc, modified, width, window, cx);
            e.cat_pinned = pinned;
            if let Some(m) = &meta {
                e.created = m.created;
                e.info = m.info;
                e.set_read_only(m.reading, cx);
            }
            e
        });
        // Reuse the live state, including unsaved edits and undo history.
        if let Some(peer) = self.document_peers(id, window, cx).first() {
            let core = peer.read(cx).core.clone();
            editor.update(cx, |e, cx| e.receive_document(&core, cx));
        }
        self.bind_editor(editor.clone(), window, cx);
        // Opening a document puts the caret in it, unless the user is typing a search.
        if !self.search_open {
            editor.read(cx).focus(window);
        }
        if let Some(m) = self.store.touch(id)
            && let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                e.meta.opened = m.opened;
            }
        self.editor = Some(editor);
        self.current = Some(id.to_string());
        self.file = None;
        self.file_snapshot = None;
        self.file_source = None;
        crate::quick_note::used(id, cx);
        self.reveal_in_tree(id);
        self.sync_sub_pages(cx);
        self.status = SaveStatus::Idle;
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.mode_menu = false;
        self.reload_annotations(cx);
        cx.notify();
        true
    }

    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.save_before(cx, |w, window, cx| w.new_doc(window, cx)) {
            return;
        }
        match self.store.create() {
            Ok((id, _)) => {
                self.search_open = false;
                self.doc_info = None;
                self.home.sort_menu = false;
                self.entries = self.store.list();
                self.sort();
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor {
                    ed.read(cx).focus(window);
                }
            }
            Err(e) => self.status = SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        cx.notify();
    }

    /// Ask before 删除: a confirmation, then the 回收站.
    pub fn ask_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.confirm = Some(Confirm::Delete { ids: vec![id.to_string()], batch: false });
        cx.notify();
    }

    /// Move documents to the trash; if the open one goes, open the next.
    pub fn delete_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        // A page goes with its child pages: each topmost page is
        // trashed as one item that carries its subtree.
        let tree = crate::tree::Tree::new(&self.entries);
        let mut all: Vec<(String, Option<String>)> = vec![];
        for id in ids {
            let covered = self.ancestors(id).iter().any(|a| ids.contains(&a.id));
            if covered {
                continue;
            }
            for d in tree.subtree(id) {
                if !all.iter().any(|(x, _)| *x == d) {
                    let root = (d != *id).then(|| id.clone());
                    all.push((d, root));
                }
            }
        }
        let groups = all;
        let all: Vec<String> = groups.iter().map(|(d, _)| d.clone()).collect();
        for handle in cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id()) {
            if let Some(handle) = handle.downcast::<Workspace>()
                && handle.update(cx, |ws, _, _| ws.current.as_ref().is_some_and(|id| all.contains(id))).unwrap_or(false)
            {
                self.show_toast_kind(ToastKind::Error, crate::settings::text("请先关闭正在编辑这些文档的其他窗口，再删除"), cx);
                return;
            }
        }
        if self.current.as_ref().is_some_and(|c| all.contains(c)) {
            let retry = ids.to_vec();
            if !self.save_before(cx, move |w, window, cx| w.delete_docs(&retry, window, cx)) {
                return;
            }
        }
        let mut deleted = vec![];
        let mut errors = vec![];
        for (id, root) in &groups {
            // Do not split a subtree when its root could not be deleted.
            if root.as_ref().is_some_and(|r| !deleted.contains(r)) { continue; }
            match self.store.delete(id, root.as_deref()) {
                Ok(()) => deleted.push(id.clone()),
                Err(e) => errors.push(crate::localized_format!("删除失败：{}", "Could not delete document: {}", crate::settings::diagnostic(&e.to_string()))),
            }
        }
        if self.current.as_ref().is_some_and(|c| deleted.contains(c)) {
            self.save_task = None;
            self.unsaved = false;
            self.current = None;
            self.editor = None;
            self.editor_sub = None;
        }
        self.entries.retain(|e| !deleted.contains(&e.id));
        if let Some(b) = self.batch.as_mut() {
            b.retain(|id| !deleted.contains(id));
        }
        self.row_menu = None;
        if self.current.is_none() {
            // Stay on 主页 / 回收站 if that's where the delete came from.
            // The last document gone leaves the library empty (主页's empty
            // state), not a new 未命名 document.
            let view = self.view;
            match self.entries.first().map(|e| e.id.clone()) {
                Some(next) => { self.open(&next, window, cx); self.view = view; }
                None => self.view = if view == View::Doc { View::Home } else { view },
            }
        }
        self.sync_sub_pages(cx);
        if self.view == View::Trash {
            self.trash = self.store.list_trash();
        }
        errors.extend(self.store.take_warnings().iter().map(|s| crate::settings::diagnostic(s)));
        if errors.is_empty() {
            self.show_toast(crate::settings::text("删除成功"), cx);
        } else {
            let msg = errors.join("；");
            self.show_toast_kind(ToastKind::Error, &msg, cx);
            self.status = SaveStatus::Failed(msg);
        }
        cx.notify();
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut on = false;
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.pinned = !e.meta.pinned;
            on = e.meta.pinned;
            let _ = self.store.set_pinned(id, e.meta.pinned);
        }
        self.sort();
        self.row_menu = None;
        self.show_toast(if on { crate::settings::text("已添加到“置顶”") } else { crate::settings::text("已从“置顶”移除") }, cx);
        cx.notify();
    }

    /// 收藏 / 取消收藏 (listed under 主页 → 收藏).
    pub fn toggle_star(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        let on = e.meta.starred == 0;
        if let Ok(t) = self.store.set_starred(id, on) {
            e.meta.starred = t;
        }
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.show_toast(if on { crate::settings::text("已添加到“收藏”") } else { crate::settings::text("已从“收藏”中移除") }, cx);
        cx.notify();
    }

    /// Switch the main pane to 主页 or 回收站 (or back to the document).
    pub fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        if view == View::Trash {
            self.trash = self.store.list_trash();
        }
        self.view = view;
        self.row_menu = None;
        self.trash_menu = None;
        self.theme_menu = None;
        self.home.sort_menu = false;
        cx.notify();
    }

    /// A success toast (删除成功, 移动成功 …).
    pub fn show_toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.show_toast_kind(ToastKind::Success, msg, cx);
    }

    pub fn show_toast_kind(&mut self, kind: ToastKind, msg: &str, cx: &mut Context<Self>) {
        self.toast = Some((kind, SharedString::from(msg.to_string()), Instant::now()));
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| e.clear_toast(cx));
        }
        self.toast_task = Some(cx.spawn(async |this, cx| {
            cx.background_executor().timer(Duration::from_millis(2700)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
        cx.notify();
    }

    fn render_toast(&self, window: &mut Window) -> Option<AnyElement> {
        let (kind, msg, t0) = self.toast.as_ref()?;
        crate::ui::toast_el(*kind, msg, *t0, window)
    }

    /// 重命名: the row's title turns into an input; Enter or clicking away saves, Esc cancels.
    pub fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.entries.iter().find(|e| e.id == id).map(|e| e.title.trim().to_string()) else { return };
        let field = cx.new(|cx| {
            let mut f = TextField::new(crate::model::UNTITLED, cx);
            f.set_text(&title, cx);
            f.select_all_text(cx);
            f
        });
        let focus = field.focus_handle(cx);
        window.focus(&focus);
        let subs = vec![
            cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
                TextFieldEvent::Submit => w.finish_rename(true, window, cx),
                TextFieldEvent::Cancel => w.finish_rename(false, window, cx),
                _ => {}
            }),
            cx.on_blur(&focus, window, |w, window, cx| w.finish_rename(true, window, cx)),
        ];
        self.rename = Some(Rename { id: id.to_string(), field, _subs: subs });
        // From 主页 the row may be hidden: show it in the sidebar.
        self.sidebar_open = true;
        self.reveal_in_tree(id);
        self.row_menu = None;
        cx.notify();
    }

    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let refocus = r.field.read(cx).is_focused(window);
        let title = r.field.read(cx).text.trim().to_string();
        let old = self.entries.iter().find(|e| e.id == r.id).map(|e| e.title.trim().to_string());
        if commit && old.is_some_and(|o| o != title) {
            if self.current.as_deref() == Some(r.id.as_str()) {
                // The open editor owns the title; its Changed event saves it.
                if let Some(ed) = &self.editor {
                    ed.update(cx, |e, cx| e.set_title(&title, cx));
                }
            } else if let Some(peer) = self.document_peers(&r.id, window, cx).first() {
                peer.update(cx, |e, cx| e.set_title(&title, cx));
            } else {
                match self.store.rename(&r.id, &title) {
                    Ok(meta) => {
                        if let Some(e) = self.entries.iter_mut().find(|e| e.id == r.id) {
                            e.title = title;
                            e.meta = meta;
                        }
                        self.sort();
                    }
                    Err(e) => self.status = SaveStatus::Failed(crate::localized_format!("重命名失败：{}", "Could not rename document: {}", crate::settings::diagnostic(&e.to_string()))),
                }
            }
        }
        if refocus
            && let Some(ed) = &self.editor {
                ed.read(cx).focus(window);
            }
        self.sync_sub_pages(cx);
        cx.notify();
    }

    fn document_peers(&self, id: &str, window: &Window, cx: &mut Context<Self>) -> Vec<Entity<Editor>> {
        cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id())
            .filter_map(|w| w.downcast::<Workspace>())
            .filter_map(|w| w.update(cx, |ws, _, _| {
                (ws.current.as_deref() == Some(id)).then(|| ws.editor.clone()).flatten()
            }).ok().flatten()).collect()
    }

    fn sync_document(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else { return };
        let peers = self.document_peers(&id, window, cx);
        if peers.is_empty() { return; }
        let core = editor.read(cx).core.clone();
        for peer in peers { peer.update(cx, |e, cx| e.receive_document(&core, cx)); }
        for handle in cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id()).filter_map(|w| w.downcast::<Workspace>()) {
            let _ = handle.update(cx, |ws, _, cx| {
                if let Some(entry) = ws.entries.iter_mut().find(|e| e.id == id) {
                    entry.title = core.doc.title.text.clone();
                    entry.icon = core.doc.icon.clone();
                }
                // Every remaining view can save the latest state if its sibling closes.
                if ws.current.as_deref() == Some(&id) { ws.unsaved = true; }
                cx.notify();
            });
        }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.status = SaveStatus::Saving;
        if !self.unsaved || self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
        self.unsaved = true;
        // Each edit replaces the timer (dropping a Task cancels it), so steady
        // typing would put the save off for as long as it lasts: the deadline
        // caps that at SAVE_MAX_DELAY after the oldest unsaved edit.
        let left = SAVE_MAX_DELAY.saturating_sub(self.dirty_since.map_or(Duration::ZERO, |t| t.elapsed()));
        let delay = SAVE_DELAY.min(left);
        // Keep the list title live while typing; the body/snippet updates on save.
        if let (Some(id), Some(ed)) = (&self.current, &self.editor) {
            let title = ed.read(cx).core.doc.title.text.clone();
            if let Some(e) = self.entries.iter_mut().find(|e| &e.id == id) {
                e.title = title;
            }
        }
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |w, cx| w.flush(cx));
        }));
        cx.notify();
    }

    fn check_reminders(&mut self, from: i64, to: i64, cx: &mut Context<Self>) {
        // The open document's reminders are read live (unsaved edits too).
        let open = self.editor.as_ref().map(|e| e.read(cx).core.doc.reminders());
        for e in &self.entries {
            let rs = match &open {
                Some(rs) if self.current.as_deref() == Some(e.id.as_str()) => rs,
                _ => &e.reminders,
            };
            for r in rs.iter().filter(|r| r.should_notify && from < r.notify && r.notify <= to) {
                notify_system(&display_title(&e.title), &crate::editor::reminder::notification(r));
            }
        }
    }

    /// Write the open document to disk if it has unsaved changes. False when
    /// that failed: the changes stay pending (a later save retries them) and
    /// the top bar shows the error.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_task = None;
        if !self.unsaved {
            return true;
        }
        let (Some(id), Some(ed)) = (self.current.clone(), self.editor.clone()) else {
            if let (Some(path), Some(ed)) = (self.file.clone(), self.editor.clone()) {
                if self.file_saving { return false; }
                let Some(snapshot) = self.file_snapshot.clone() else {
                    self.status = SaveStatus::Failed(crate::settings::tr("缺少文件快照，请另存副本", "File snapshot is missing; save a copy").to_string());
                    return false;
                };
                let Some(source) = self.file_source.clone() else {
                    self.status = SaveStatus::Failed(crate::settings::tr("缺少文件来源，请另存副本", "File source is missing; save a copy").to_string());
                    return false;
                };
                let doc = ed.read(cx).core.doc.clone();
                let revision = self.file_revision;
                self.file_saving = true;
                self.status = SaveStatus::Saving;
                let saving_path = path.clone();
                let task = cx.background_executor().spawn(async move { save_folder_document(&saving_path, &snapshot, &doc, &source) });
                let handle = self.window_handle;
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = handle.update(cx, |_, window, cx| {
                        let _ = this.update(cx, |w, cx| w.finish_folder_save(path, revision, result, window, cx));
                    });
                }).detach();
                cx.notify();
                return false;
            }
            self.unsaved = false;
            return true;
        };
        let doc = &ed.read(cx).core.doc;
        let (title, icon, body, reminders, links) = (doc.title.text.clone(), doc.icon.clone(), doc.plain_text(), doc.reminders(), doc.doc_links());
        match self.store.save(&id, doc) {
            Ok(meta) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                    e.title = title;
                    e.icon = icon;
                    e.body = body;
                    e.reminders = reminders;
                    e.links = links;
                    e.meta = meta.clone();
                }
                self.sync_links(cx);
                ed.update(cx, |e, cx| {
                    e.modified = meta.modified;
                    cx.notify();
                });
                self.status = SaveStatus::Saved;
                self.unsaved = false;
                self.sort();
            }
            Err(e) => self.status = SaveStatus::Failed(crate::localized_format!("保存失败：{}", "Could not save: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        // A failed save tries again SAVE_MAX_DELAY after this attempt, not on every keystroke.
        self.dirty_since = self.unsaved.then(Instant::now);
        cx.notify();
        !self.unsaved
    }

    fn finish_folder_save(&mut self, path: std::path::PathBuf, revision: u64, result: Result<FolderSaveOutcome, crate::workspace_fs::SaveFileError>, window: &mut Window, cx: &mut Context<Self>) {
        self.file_saving = false;
        if self.file.as_ref() != Some(&path) { return; }
        match result {
            Ok(outcome) => {
                self.file_snapshot = Some(outcome.text);
                self.file_source = Some(outcome.source);
                if let Some(ed) = &self.editor { ed.update(cx, |e, cx| { e.modified = outcome.modified; cx.notify(); }); }
                self.unsaved = revision != self.file_revision;
                self.status = if self.unsaved { SaveStatus::Saving } else { SaveStatus::Saved };
                self.dirty_since = self.unsaved.then(Instant::now);
                if let Some(error) = outcome.git_warning {
                    self.show_toast_kind(ToastKind::Error, &crate::localized_format!("文件已保存，Git 自动提交未完成：{}", "File saved; Git auto-commit did not complete: {}", error), cx);
                }
                self.refresh_folder_links(cx);
                if self.unsaved {
                    if self.after_save.is_some() { self.flush(cx); } else { self.schedule_save(window, cx); }
                } else if let Some(then) = self.after_save.take() {
                    then(self, window, cx);
                }
            }
            Err(reason) => {
                let retry_io = matches!(&reason, crate::workspace_fs::SaveFileError::Io(_));
                let error = match reason {
                    crate::workspace_fs::SaveFileError::Changed => crate::settings::tr("文件已被其他程序修改，请重新载入或另存副本", "The file changed outside MarkdownX; reload it or save a copy").to_string(),
                    crate::workspace_fs::SaveFileError::Lossy => crate::settings::tr("Markdown 内容或格式无法完整保留，请另存副本", "Markdown content or formatting cannot be preserved; save a copy").to_string(),
                    crate::workspace_fs::SaveFileError::AssetConflict(path) => crate::localized_format!("已有内容不同的同名资源：{}", "An asset with the same name has different content: {}", path.display()),
                    crate::workspace_fs::SaveFileError::MissingAssets(n) => crate::localized_format!("有 {} 个资源无法导出，请另存副本", "{} asset(s) could not be exported; save a copy", n),
                    crate::workspace_fs::SaveFileError::Io(e) => crate::localized_format!("保存失败：{}（{}）", "Could not save: {} ({})", path.display(), e),
                };
                self.status = SaveStatus::Failed(error);
                self.dirty_since = Some(Instant::now());
                if self.after_save.is_some() { self.confirm = Some(Confirm::SaveFailed); }
                else if retry_io {
                    self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(10)).await;
                        let _ = this.update(cx, |w, cx| { if w.unsaved && w.file.as_ref() == Some(&path) && !w.file_saving && w.after_save.is_none() { w.flush(cx); } });
                    }));
                }
            }
        }
        cx.notify();
    }

    /// Save the open document before something replaces it. When the save
    /// fails nothing is replaced: the 保存失败 dialog offers 重试 / 放弃修改
    /// and runs `then` after either.
    pub fn save_before(&mut self, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> bool {
        if self.flush(cx) {
            return true;
        }
        self.after_save = Some(Box::new(then));
        if !self.file_saving { self.confirm = Some(Confirm::SaveFailed); }
        cx.notify();
        false
    }

    /// The 保存失败 dialog's choices: `retry` saves again, otherwise the
    /// changes are given up. Either way the held-back action runs once the
    /// document no longer needs saving.
    pub(crate) fn resolve_save_failed(&mut self, retry: bool, window: &mut Window, cx: &mut Context<Self>) {
        if retry {
            self.confirm = None;
            if !self.flush(cx) {
                if !self.file_saving { self.confirm = Some(Confirm::SaveFailed); }
                return;
            }
        } else {
            if let Some(path) = self.file.clone() {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let (doc, source) = crate::markdown_source::parse(&path, &text);
                        if let Some(ed) = &self.editor {
                            ed.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc), cx));
                        }
                        self.file_snapshot = Some(text);
                        self.file_source = Some(source);
                        self.unsaved = false;
                        self.status = SaveStatus::Idle;
                        self.dirty_since = None;
                    }
                    Err(e) => {
                        self.status = SaveStatus::Failed(crate::localized_format!("无法重新载入：{}（{}）", "Could not reload: {} ({})", path.display(), e));
                        self.confirm = Some(Confirm::SaveFailed);
                        cx.notify();
                        return;
                    }
                }
                if let Some(then) = self.after_save.take() {
                    then(self, window, cx);
                }
                cx.notify();
                return;
            }
            let Some(id) = self.current.clone() else { return };
            match self.store.load_for_edit(&id, false) {
                Ok(doc) => {
                    if let Some(ed) = &self.editor {
                        ed.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc), cx));
                    }
                    self.sync_document(window, cx);
                    self.save_task = None;
                    self.unsaved = false;
                    self.status = SaveStatus::Idle;
                    self.dirty_since = None;
                    self.entries = self.store.list();
                    self.sort();
                }
                Err(e) => {
                    self.status = SaveStatus::Failed(e.to_string());
                    self.confirm = Some(Confirm::SaveFailed);
                    return;
                }
            }
        }
        if let Some(then) = self.after_save.take() {
            then(self, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn save_failed_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = &self.editor else { return };
        let mut doc = ed.read(cx).core.doc.clone();
        doc.title = crate::model::RichText::plain(format!("{}{}", doc.title.text, crate::settings::tr("（恢复副本）", " (recovered copy)")));
        match self.store.create_from(&doc) {
            Ok(_) => {
                self.entries = self.store.list();
                self.sort();
                self.confirm = None;
                if self.file.as_ref().is_some_and(|path| std::fs::read_to_string(path).is_err()) {
                    // The recovery copy is safe even if the original vanished.
                    self.unsaved = false;
                    self.file_snapshot = None;
                    self.file_source = None;
                    self.status = SaveStatus::Idle;
                    self.dirty_since = None;
                    if let Some(then) = self.after_save.take() { then(self, window, cx); }
                } else {
                    self.resolve_save_failed(false, window, cx);
                }
            }
            Err(e) => {
                self.status = SaveStatus::Failed(e.to_string());
                self.confirm = Some(Confirm::SaveFailed);
                cx.notify();
            }
        }
    }

    /// The error of the last failed save, for the 保存失败 dialog.
    pub(crate) fn save_error(&self) -> String {
        match &self.status {
            SaveStatus::Failed(e) => e.clone(),
            _ => crate::settings::text("保存失败").to_string(),
        }
    }

    /// Hand the open document the library's titles and the documents
    /// linking to it (双向链接).
    pub(crate) fn sync_links(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(ed)) = (self.current.clone(), self.editor.clone()) else { return };
        let docs = self
            .entries
            .iter()
            .map(|e| crate::editor::links::DocRef { id: e.id.clone(), title: e.title.clone(), icon: e.icon.clone(), modified: e.meta.modified, opened: e.meta.opened })
            .collect();
        let backlinks = self
            .entries
            .iter()
            .filter(|e| e.id != id)
            .filter_map(|e| {
                let lines: Vec<_> = e.links.iter().filter(|l| l.target == id).cloned().collect();
                (!lines.is_empty()).then(|| crate::editor::links::Backlink { id: e.id.clone(), lines })
            })
            .collect();
        let hidden = self.entries.iter().find(|e| e.id == id).is_some_and(|e| e.meta.refs_hidden);
        ed.update(cx, |e, cx| e.set_links(id, docs, backlinks, hidden, cx));
    }

    /// A 反向引用 line: open that document at its `ordinal`th block.
    fn open_at(&mut self, id: &str, ordinal: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.try_open(id, window, cx) {
            return;
        }
        let Some(ed) = self.editor.clone() else { return };
        ed.update(cx, |e, cx| {
            let mut n = 0;
            let mut target = None;
            e.core.doc.for_each_block(|b| {
                if n == ordinal {
                    target = Some(b.id);
                }
                n += 1;
            });
            if let Some(b) = target {
                e.scroll_to_block(b, cx);
            }
        });
    }

    /// 隐藏引用列表 / 显示引用列表: confirm first (hiding can skip it with 不再提示).
    pub(crate) fn ask_refs(&mut self, show: bool, cx: &mut Context<Self>) {
        if !show && self.store.settings().refs_hide_no_ask {
            return self.set_refs_hidden(true, cx);
        }
        self.confirm = Some(Confirm::Refs { show, no_ask: false });
        cx.notify();
    }

    pub(crate) fn set_refs_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if self.store.update_meta(&id, |m| m.refs_hidden = hidden).is_ok()
            && let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
        {
            e.meta.refs_hidden = hidden;
        }
        self.sync_links(cx);
        cx.notify();
    }

    /// 主页's search box, centred on the pane but never over what sits on
    /// either side of it: the title (and, with the sidebar hidden, the
    /// traffic lights and 展开侧栏) on the left, + on the right. Below a
    /// usable width it is None and the caller shows a search icon instead.
    pub(crate) fn header_search(&self, window: &Window, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let main = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let title = crate::settings::label(crate::settings::text("主页"));
        let title_w = f32::from(window.text_system().shape_line(title.clone().into(), px(17.), &[window.text_style().highlight(HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }).to_run(title.len())], None).width);
        // page_header's left inset (26 with the sidebar, else the traffic
        // lights plus the 4 + 28 + 12 of 展开侧栏), then 16 clear of the title.
        let left = if self.sidebar_open { 26. } else { theme::TRAFFIC_LIGHTS_W + 44. + if cfg!(target_os = "windows") { 32. } else { 0. } } + title_w + 16.;
        // + is 32 wide with an 8pt inset; 16 clear of it.
        let right = HEADER_ACTIONS_H + crate::ui::header_right_inset() + 16.;
        let half = (main / 2. - left).min(main / 2. - right);
        let width = (main - 320.).min(2. * half).min(360.);
        if width < 120. { return None }
        Some(div().id("header-search").absolute().left(px((main - width) / 2.)).top(px(8.))
            .w(px(width)).h(px(32.)).no_drag().flex().items_center().gap(px(8.)).px(px(10.))
            .rounded(px(9.)).bg(Theme::fill_hover())
            .cursor_pointer().hover(|s| s.bg(Theme::fill_hover_strong()))
            .child(icon("search", 15., Theme::text_caption()))
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(Theme::text_caption()).child(crate::settings::tr("全局搜索…", "Search everywhere…")))
            .when(width >= 200., |d| d.child(div().flex_none().text_size(px(11.)).text_color(Theme::text_placeholder()).child(crate::shortcuts::hint("workspace::GlobalSearch", "⌘ + K").replace(" + ", " "))))
            .tip(crate::settings::tr("全局搜索", "Search everywhere"), Some(crate::shortcuts::hint("workspace::GlobalSearch", "⌘ + K")))
            .on_click(cx.listener(|w, _, window, cx| w.open_global_search(window, cx))))
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .editor
            .as_ref()
            .map(|e| display_title(&e.read(cx).core.doc.title.text))
            .unwrap_or_default();
        let save_error = match &self.status {
            SaveStatus::Failed(error) => Some(error.clone()),
            _ => None,
        };
        // Independent icon controls, with equal top and right insets.
        let seg = crate::ui::header_icon;
        let divider = || div().flex_none().w(px(1.)).h(px(18.)).bg(Theme::line_divider());
        // Breadcrumbs: the parent pages, each linking to itself, with the
        // middle ones folded into "…" when the chain is long.
        let chain: Vec<(String, String, Option<String>)> = self
            .current
            .as_ref()
            .map(|id| self.ancestors(id).iter().map(|e| (e.id.clone(), display_title(&e.title), e.icon.clone())).collect())
            .unwrap_or_default();
        let mut crumbs: Vec<AnyElement> = vec![];
        let sep = || div().mx(px(3.)).flex_none().child(icon("chevron-right", 12., theme::c(craft::N500))).into_any_element();
        for (i, (id, name, em)) in chain.iter().enumerate() {
            if chain.len() > 2 && i > 0 && i < chain.len() - 1 {
                if i == 1 {
                    let hidden: Vec<&str> = chain[1..chain.len() - 1].iter().map(|c| c.1.as_str()).collect();
                    crumbs.push(
                        div()
                            .id("crumb-more")
                            .no_drag()
                            .flex_none()
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_color(Theme::text_caption())
                            .tip(&hidden.join(" / "), None)
                            .child("…")
                            .into_any_element(),
                    );
                    crumbs.push(sep());
                }
                continue;
            }
            let id = id.clone();
            crumbs.push(
                div()
                    .id(SharedString::from(format!("crumb-{id}")))
                    .no_drag()
                    .min_w(px(28.))
                    .max_w(px(200.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(Theme::text_caption())
                    .hover(|s| s.text_color(Theme::primary_500()))
                    .when_some(em.clone(), |d, em| {
                        d.child(div().size(px(20.)).mr(px(4.)).flex_none().flex().items_center().justify_center().text_size(px(16.)).child(em))
                    })
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(name.clone())))
                    .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
                    .into_any_element(),
            );
            crumbs.push(sep());
        }
        let record_more = self.more_btn_bounds.clone();
        div()
            .id("top-bar").relative()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(crate::ui::header_right_inset()))
            // Clear of the traffic lights, which sit over the note when the sidebar is closed.
            .pl(if self.sidebar_open { px(18.) } else { px(theme::TRAFFIC_LIGHTS_W) })
            .title_bar()
            .when(!self.sidebar_open, |d| {
                d.child(
                    div()
                        .id("open-sidebar")
                        .no_drag()
                        .flex_none()
                        .ml(px(4.))
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(Theme::fill_hover_strong()))
                        .tip(crate::settings::tr("展开侧栏", "Show sidebar"), Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                        .child(icon("panel-left-open", 17., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| {
                            w.sidebar_open = true;
                            cx.notify();
                        })),
                )
                .child(
                    div().id("header-home").no_drag().flex_none().ml(px(4.)).size(px(28.))
                        .flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                        .hover(|s| s.bg(Theme::fill_hover_strong()))
                        .active(|s| s.bg(Theme::fill_pressed()))
                        .tip(crate::settings::text("返回主页"), None)
                        .child(icon("home", 18., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Home, cx))),
                )
                .child(div().mx(px(12.)).child(divider()))
            })
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .h(px(20.))
                            .min_w_0()
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .children(crumbs)
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(ui_text(title)),
                            ),

                    )
                    .when_some(save_error, |d, error| d.child(
                        div().min_w_0().max_w(px(220.)).text_size(px(12.)).line_height(px(18.))
                            .text_color(theme::c(craft::R400)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(error),
                    )),
            )
            .child(div().flex_1().min_w(px(16.)))
            .child(
                div().gap(px(4.))
                    .h(px(HEADER_ACTIONS_H)).flex_none()
                    .flex()
                    .items_center()
                    .when(self.current.is_some(), |d| {
                        d.child(
                            // No tooltip while its dropdown is open; the menu hangs
                            // from this button's right edge.
                            seg("doc-more", "ellipsis")
                                .relative()
                                .when(self.doc_menu, |d| d.bg(Theme::fill_hover()))
                                .when(!self.doc_menu, |d| d.tip(crate::settings::tr("更多", "More"), None))
                                .child(canvas(move |b, _, _| record_more.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
                                .on_click(cx.listener(|w, _, _, cx| {
                                    w.doc_menu = !w.doc_menu;
                                    w.doc_sub = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        seg("doc-search", "search")
                            .tip(crate::settings::tr("全局搜索", "Search everywhere"), Some(crate::shortcuts::hint("workspace::GlobalSearch", "⌘ + K")))
                            .on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(GlobalSearch), cx))),
                    )
                    .child(
                        seg("doc-annotations", "message-square")
                            .when(self.annotations_open, |d| d.bg(Theme::fill_hover()))
                            .tip(crate::settings::tr("批注", "Notes"), None)
                            .on_click(cx.listener(|w, _, _, cx| w.toggle_annotations(cx))),
                    )
                    .child(
                        // Straight to a new document: no 新建 menu, as 导入 is rare.
                        seg("doc-new", "plus").tip(crate::settings::tr("新建文档", "New document"), None).on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx))),
                    ).into_any_element()
            )
    }
}

struct FolderSaveOutcome { text: String, source: crate::markdown_source::Source, modified: u64, git_warning: Option<String> }

fn save_folder_document(path: &std::path::Path, snapshot: &str, doc: &crate::model::Document, source: &crate::markdown_source::Source) -> Result<FolderSaveOutcome, crate::workspace_fs::SaveFileError> {
    #[cfg(feature = "devtools")]
    if let Some(ms) = std::env::var("MARKDOWNX_TEST_SAVE_DELAY_MS").ok().and_then(|s| s.parse::<u64>().ok()) {
        std::thread::sleep(Duration::from_millis(ms.min(10_000)));
    }
    if !source.matches(snapshot) { return Err(crate::workspace_fs::SaveFileError::Changed); }
    let rendered = crate::markdown_source::render(path, source, doc)?;
    let copied = crate::workspace_fs::write_export_if_unchanged(path, snapshot.as_bytes(), rendered.text.as_bytes(), &rendered.files)?;
    let mut touched = vec![path.to_path_buf()];
    touched.extend(copied);
    let git_warning = crate::git_workspace::repo_root(path).and_then(|root| {
        let message = format!("edit: {}", crate::git_workspace::relative(&root, path));
        crate::git_workspace::commit_paths(&root, &touched, &message).err().map(|e| e.to_string())
    });
    Ok(FolderSaveOutcome { text: rendered.text, source: rendered.source, modified: file_modified(path), git_warning })
}

fn file_modified(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or_else(crate::time::now)
}

/// Read a Markdown file as the document model; its original source is retained for safe saves.
pub(crate) fn parse_markdown_file(path: &std::path::Path, text: &str) -> (crate::model::Document, u64) {
    (crate::markdown_source::parse(path, text).0, file_modified(path))
}

/// Header and action group share an 8px top/right inset.
pub const HEADER_H: f32 = 48.;
pub(crate) const HEADER_ACTIONS_H: f32 = 32.;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Clicks no longer take focus from the editor (see the root's mouse
        // down), so leaving the document hands it back to the window's keys.
        if self.view != View::Doc && self.editor.as_ref().is_some_and(|e| e.read(cx).focus.is_focused(window)) { window.focus(&self.focus); }
        self.native_sidebar.sync(if self.sidebar_open { self.sidebar_w(window) } else { 0. }, window);
        let sidebar = if self.sidebar_open { Some(if matches!(self.view, View::Settings | View::Trash) { self.render_settings_sidebar(window, cx) } else { self.render_sidebar(window, cx) }) } else { None };
        let row_menu = self.render_row_menu(window, cx);
        let resize = self.sidebar_open.then(|| self.render_resize_handle(window, cx));
        let resize_overlay = self.render_resize_overlay(cx);
        let search = self.render_search(window, cx);
        let doc_menu = self.render_doc_menu(window, cx);
        let doc_info = self.render_doc_info(cx);
        let confirm = self.render_confirm(cx);
        let trash_menu = self.render_trash_menu(cx);
        let home_menus = self.render_home_menus(cx);
        let theme_menu = self.render_theme_menu(cx);
        let toast = self.render_toast(window);
        let mode_menu = self.render_mode_menu(cx);
        let annotations = self.render_annotations(window, cx);
        let main = match self.view {
            View::Doc => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(self.top_bar(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .children(self.editor.clone()),
                        )
                        .children(self.render_shortcuts(cx)),
                )
                .into_any_element(),
            View::Home => self.render_home(window, cx),
            View::Trash => self.render_trash(cx),
            View::Settings => self.render_settings(cx),
        };
        let move_dlg = self.render_move_dialog(cx);
        let export_md = self.render_export_md(cx);
        // Only the note stands out: a white plane flush with the window's top,
        // right and bottom edges. The sidebar sits sunken on the window
        // surface beside it; the change of value is the only line between
        // them (no gutter, radius, border or shadow).
        // A document sits on its theme's page colour.
        let page = if self.view == View::Doc { crate::editor_theme::Theme::bg_body() } else { Theme::bg_body() };
        let note = div().id("note").relative().flex_1().min_w_0().h_full().flex().bg(page).child(main);
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            // A click on what takes no input (title bar, page margins, the
            // sidebar's blank space) leaves the caret in the document, also
            // the click that brings the window back (#59). Runs before the
            // root takes focus; anything inside that takes focus did so first.
            .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| {
                if w.view == View::Doc && w.editor.as_ref().is_some_and(|e| e.read(cx).focus.is_focused(window)) { window.prevent_default(); }
            }))
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(theme::UI_FONT)
            .text_color(Theme::text())
            // Window keys act on the window they were pressed in, not on
            // whichever window macOS considers active.
            .on_action(cx.listener(|w, _: &CloseWindow, window, cx| {
                crate::quick_note::persist(cx);
                if w.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
            }))
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|w, _: &OpenSettings, _, cx| w.show_view(View::Settings, cx)))
            .on_action(cx.listener(|w, _: &OpenAbout, _, cx| w.open_settings(crate::settings::Page::About, cx)))
            .on_action(cx.listener(|w, _: &CheckForUpdates, _, cx| { w.open_settings(crate::settings::Page::About, cx); crate::update::check(true, cx); }))
            .on_action(cx.listener(|w, _: &GoHome, _, cx| w.show_view(View::Home, cx)))
            .on_action(cx.listener(|w, _: &NewDoc, window, cx| w.new_doc(window, cx)))
            .on_action(cx.listener(|w, _: &ImportDoc, window, cx| w.open_import(window, cx)))
            .on_action(cx.listener(|w, _: &OpenFolder, window, cx| w.open_folder(window, cx)))
            .on_action(cx.listener(|w, _: &OpenFile, window, cx| w.open_file(window, cx)))
            .on_action(cx.listener(|w, _: &PackageBook, window, cx| w.package_book(window, cx)))
            .on_action(cx.listener(|w, _: &ToggleSidebar, _, cx| {
                w.sidebar_open = !w.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|w, _: &FindInDoc, window, cx| w.find_in_doc(false, window, cx)))
            .on_action(cx.listener(|w, _: &ReplaceInDoc, window, cx| {
                let replace = !w.reading();
                w.find_in_doc(replace, window, cx)
            }))
            .on_action(cx.listener(|w, _: &crate::modes::EditMode, window, cx| w.set_mode(false, window, cx)))
            .on_action(cx.listener(|w, _: &crate::modes::ReadMode, window, cx| w.set_mode(true, window, cx)))
            .on_action(cx.listener(|w, _: &crate::modes::ToggleShortcuts, _, cx| {
                w.shortcuts = !w.shortcuts;
                cx.notify();
            }))
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| {
                // The theme dropdown: ↑ ↓ preview, ↩ applies, Esc closes it (not 设置).
                if w.theme_menu.is_some() && w.theme_menu_key(&ev.keystroke.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                // Esc leaves 设置 (unless its filter field has the caret).
                if ev.keystroke.key == "escape" && w.view == View::Settings && !w.settings_filter.read(cx).is_focused(window) {
                    w.close_settings(cx);
                    cx.stop_propagation();
                    return;
                }
                if ev.keystroke.key == "escape" && (w.doc_info.is_some() || w.doc_menu || w.mode_menu) {
                    w.doc_info = None;
                    w.mode_menu = false;
                    w.doc_menu = false;
                    w.doc_sub = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // Esc leaves 批量操作 (closing its menu or confirmation first), even from the editor.
            .capture_key_down(cx.listener(|w, ev: &KeyDownEvent, _, cx| {
                if w.record_shortcut(ev, cx) { return; }
                // Esc cancels a confirmation dialog.
                if ev.keystroke.key == "escape" && (w.confirm.is_some() || w.export_md.is_some()) {
                    w.confirm = None;
                    w.after_save = None;
                    w.export_md = None;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if ev.keystroke.key != "escape" || w.batch.is_none() || w.search_open || w.rename.is_some() {
                    return;
                }
                if w.row_menu.is_some() {
                    w.row_menu = None;
                } else {
                    w.batch = None;
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_action(cx.listener(|w, _: &FocusSearch, window, cx| {
                if w.folder.is_some() {
                    w.sidebar_open = true;
                    let fh = w.folder_query.read(cx).focus_handle(cx);
                    window.focus(&fh);
                    cx.notify();
                } else if w.search_open {
                    w.close_search(window, cx);
                } else {
                    w.open_search(window, cx);
                }
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|w, _: &GlobalSearch, window, cx| {
                w.open_global_search(window, cx);
                cx.stop_propagation();
            }))
            .children(sidebar)
            .child(note)
            .children(annotations)
            .children(resize)
            .children(crate::ui::window_controls(window))
            .children(row_menu)
            .children(doc_menu)
            .children(mode_menu)
            .children(doc_info)
            .children(trash_menu)
            .children(home_menus)
            .children(theme_menu)
            .children(confirm)
            .children(move_dlg)
            .children(export_md)
            .children(search)
            .children(resize_overlay)
            .children(toast)
    }
}

/// Fault injection against real window/save code, available only in the harness build.
#[cfg(feature = "devtools")]
#[derive(Clone)]
struct SaveSafetyQa(Vec<(WindowHandle<Workspace>, String)>);
#[cfg(feature = "devtools")]
impl Global for SaveSafetyQa {}

#[cfg(feature = "devtools")]
pub(crate) fn check_save_safety(phase: &str, cx: &mut App) {
    use crate::model::{Document, RichText};
    let doc = |title: &str| Document { title: RichText::plain(title), ..Default::default() };
    if phase == "setup" {
        let mut store = cx.global::<SharedStore>().clone();
        let mut windows = vec![];
        for name in ["A", "B", "C"] {
            let id = store.create_from(&doc(name)).unwrap();
            let open_id = id.clone();
            let handle = cx.open_window(WindowOptions { focus: false, ..Default::default() }, |window, cx| {
                cx.new(|cx| Workspace::new_document_window(Some(open_id), window, cx))
            }).unwrap();
            handle.update(cx, |ws, _, cx| {
                ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc(&format!("local-{name}"))), cx));
                ws.unsaved = true;
            }).unwrap();
            if name != "C" {
                std::fs::write(crate::store::Store::root_dir().join("docs").join(format!("{id}.xml")), crate::docxml::serialize(&doc(&format!("disk-{name}")))).unwrap();
            }
            windows.push((handle, id));
        }
        quit(cx);
        for (handle, _) in &windows[..2] {
            handle.update(cx, |ws, _, _| assert!(ws.unsaved && matches!(ws.confirm, Some(Confirm::SaveFailed)))).unwrap();
        }
        assert_eq!(store.load(&windows[2].1).unwrap().title.text, "local-C");
        windows[0].0.update(cx, |ws, window, cx| {
            ws.resolve_save_failed(true, window, cx);
            assert!(ws.unsaved);
            ws.confirm = None;
            ws.resolve_save_failed(false, window, cx);
            assert!(!ws.unsaved);
            assert_eq!(ws.editor.as_ref().unwrap().read(cx).core.doc.title.text, "disk-A");
        }).unwrap();
        cx.set_global(SaveSafetyQa(windows));
        eprintln!("save-safety: all windows checked; successful sibling saved; retry and reload checked");
    } else {
        let windows = cx.global::<SaveSafetyQa>().0.clone();
        windows[1].0.update(cx, |ws, window, cx| {
            assert!(ws.unsaved && matches!(ws.confirm, Some(Confirm::SaveFailed)));
            ws.after_save = None;
            ws.save_failed_copy(window, cx);
            assert!(!ws.unsaved);
            assert_eq!(ws.editor.as_ref().unwrap().read(cx).core.doc.title.text, "disk-B");
            assert!(ws.store.list().iter().any(|e| e.title.starts_with("local-B")));
        }).unwrap();
        windows[0].0.update(cx, |ws, _, cx| {
            ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("after-reload")), cx));
            ws.unsaved = true;
            assert!(ws.flush(cx));
            assert_eq!(ws.store.load(ws.current.as_ref().unwrap()).unwrap().title.text, "after-reload");
        }).unwrap();
        eprintln!("save-safety: PASS; remaining conflict blocked quit; copy preserved; edit after reload saved cleanly");
        for (handle, _) in windows { handle.update(cx, |_, window, _| window.remove_window()).unwrap(); }
    }
}

/// Check the live folder view without writing to the opened workspace.
#[cfg(feature = "devtools")]
pub(crate) fn check_folder_state(root: &str, cx: &mut App) {
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |ws, _, _| {
        let folder = ws.folder.as_ref().expect("folder was not opened");
        assert_eq!(folder.root.canonicalize().unwrap(), std::path::Path::new(root).canonicalize().unwrap());
        assert!(ws.sidebar_open, "the directory tree is hidden");
        assert!(!folder.children.is_empty());
        assert!(ws.file.as_ref().is_some_and(|file| folder.files().iter().any(|node| node.path == *file)));
        assert!(folder.warnings.is_empty(), "workspace scan had warnings");
        eprintln!("folder-state: PASS; {} root entries, {} supported files", folder.children.len(), folder.files().len());
    }).unwrap();
}

/// Check global search through real key dispatch in a marked temporary folder.
#[cfg(feature = "devtools")]
pub(crate) fn check_global_search(args: &str, cx: &mut App) {
    let (phase, root) = args.split_once(' ').expect("phase and temporary folder");
    let root = std::path::PathBuf::from(root);
    assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root.join(".markdownx-global-search-test").is_file());
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |ws, window, cx| match phase {
        "ready" => {
            assert!(ws.editor.as_ref().is_some_and(|editor| editor.read(cx).focus.is_focused(window)), "opening a folder file must focus its editor");
        }
        "empty" => {
            assert!(ws.editor.is_none() && ws.focus.is_focused(window), "an empty folder must leave workspace shortcuts available");
        }
        "closed" => {
            assert!(!ws.search_open && ws.focus.is_focused(window), "closing search must restore workspace focus");
        }
        "open" => {
            assert!(ws.search_open, "global search did not open");
            assert!(ws.global_search, "search is not in global mode");
            assert!(ws.search.read(cx).is_focused(window), "global search input is not focused");
            assert_eq!(ws.folder.as_ref().unwrap().root.canonicalize().unwrap(), root.canonicalize().unwrap());
        }
        "results" => {
            assert!(ws.search_open && ws.global_search && !ws.global_folder_searching);
            assert_eq!(ws.search.read(cx).text, "globalsearchuniquexyz");
            assert_eq!(ws.global_folder_hits.len(), 2);
            assert!(ws.global_folder_hits.iter().all(|h| h.snippet.as_ref().is_some_and(|s| s.contains("globalsearchuniquexyz"))));
            assert_eq!(ws.search_results(cx).len(), 1, "library and folder results must both be searchable");
            assert_eq!(ws.search_sel, Some(2), "the previously selected library result must not move when folder results arrive");
        }
        "moved" => assert_eq!(ws.search_sel, Some(1)),
        "picked" => {
            assert!(!ws.search_open);
            assert_eq!(ws.file.as_deref(), Some(root.join("notes/b.md").as_path()));
        }
        "link" => assert!(matches!(ws.editor.as_ref().unwrap().read(cx).menu.as_ref(), Some(crate::editor::view::Menu::Link { .. }))),
        "legacy" => assert!(!ws.search_open && ws.folder_query.read(cx).is_focused(window)),
        _ => panic!("unknown global search check"),
    }).unwrap();
    eprintln!("global-search: {phase} PASS");
}

/// Exercise switching, refresh and conflict recovery in a marked temporary folder.
#[cfg(feature = "devtools")]
pub(crate) fn check_folder_safety(args: &str, cx: &mut App) {
    use crate::model::{Document, RichText};
    let (phase, root) = args.split_once(' ').expect("phase and temporary folder");
    let root = std::path::PathBuf::from(root);
    assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root.join(".markdownx-folder-test").is_file());
    assert!(crate::git_workspace::repo_root(&root).is_none(), "the test fixture must not be inside a Git repository");
    let first = root.join("A/first.md");
    let second = root.join("B/second.md");
    let doc = |title: &str| Document { title: RichText::plain(title), ..Default::default() };
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |ws, window, cx| {
        match phase {
            "begin" => {
                std::fs::create_dir_all(first.parent().unwrap()).unwrap();
                std::fs::create_dir_all(second.parent().unwrap()).unwrap();
                std::fs::create_dir_all(root.join("empty")).unwrap();
                std::fs::write(&first, "# First\r\n\r\nfirst\r\n").unwrap();
                std::fs::write(&second, "# Second\r\n\r\nsecond\r\n").unwrap();
                ws.open_folder_path(root.clone(), window, cx);
                assert!(ws.sidebar_open && ws.folder.is_some());
                assert_eq!(ws.file.as_deref(), Some(first.as_path()));
                ws.open_external_path(second.clone(), window, cx);
                assert_eq!(ws.file.as_deref(), Some(second.as_path()));
                let editor = ws.editor.as_ref().unwrap().clone();
                ws.load_file(second.clone(), window, cx);
                assert_eq!(ws.editor.as_ref().unwrap().entity_id(), editor.entity_id());
                editor.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("Saved")), cx));
                ws.file_revision += 1;
                ws.unsaved = true;
                assert!(!ws.flush(cx) && ws.file_saving);
                editor.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("Latest")), cx));
                ws.file_revision += 1;
                ws.load_file(first.clone(), window, cx);
                assert!(ws.file_saving && ws.after_save.is_some() && ws.confirm.is_none());
                assert_eq!(ws.file.as_deref(), Some(second.as_path()));
            }
            "conflict" => {
                assert!(!ws.unsaved && !ws.file_saving);
                assert_eq!(ws.file.as_deref(), Some(first.as_path()));
                assert!(std::fs::read(&second).unwrap().starts_with(b"# Latest\r\n"));
                ws.load_file(second.clone(), window, cx);
                ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("Local")), cx));
                ws.file_revision += 1;
                ws.unsaved = true;
                std::fs::write(&second, "# External\r\n").unwrap();
                ws.load_file(first.clone(), window, cx);
                assert!(ws.file_saving && ws.confirm.is_none());
            }
            "recover" => {
                assert!(ws.unsaved && !ws.file_saving && matches!(ws.confirm, Some(Confirm::SaveFailed)));
                assert_eq!(std::fs::read_to_string(&second).unwrap(), "# External\r\n");
                ws.confirm = None;
                ws.resolve_save_failed(false, window, cx);
                assert_eq!(ws.file.as_deref(), Some(first.as_path()));
                ws.load_file(second.clone(), window, cx);
                ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("Recovery")), cx));
                ws.file_revision += 1;
                ws.unsaved = true;
                std::fs::remove_file(&second).unwrap();
                ws.load_file(first.clone(), window, cx);
            }
            "finish" => {
                assert!(ws.unsaved && !ws.file_saving && matches!(ws.confirm, Some(Confirm::SaveFailed)));
                ws.confirm = None;
                ws.resolve_save_failed(false, window, cx);
                assert!(ws.unsaved && matches!(ws.confirm, Some(Confirm::SaveFailed)));
                ws.save_failed_copy(window, cx);
                assert!(!ws.unsaved && ws.store.list().iter().any(|entry| entry.title.starts_with("Recovery")));
                assert_eq!(ws.file.as_deref(), Some(first.as_path()));
                let new_file = root.join("C/new.md");
                std::fs::create_dir_all(new_file.parent().unwrap()).unwrap();
                std::fs::write(&new_file, "# New\n").unwrap();
                ws.refresh_folder(cx);
                assert!(ws.folder.as_ref().unwrap().files().iter().any(|node| node.path == new_file));
                ws.close_folder(window, cx);
                ws.open_folder_tree_path(root.clone(), window, cx);
                assert!(ws.sidebar_open && ws.folder.is_some() && ws.file.is_none() && ws.editor.is_none());
                ws.close_folder(window, cx);
                eprintln!("folder-safety: background save, concurrent edit, switching, conflict and recovery checked");
            }
            "empty-begin" => {
                let editor = ws.editor.as_ref().expect("new empty file has an editor").clone();
                editor.update(cx, |e, cx| {
                    assert_eq!(e.core.doc.blocks.len(), 1);
                    e.core.insert_text("draft");
                    assert_eq!(e.core.doc.title.text, "from-dialog");
                    e.changed(window, cx);
                });
            }
            "empty-done" => {
                let created = root.join("C/from-dialog.md");
                assert!(!ws.unsaved && !ws.file_saving);
                assert_eq!(std::fs::read(&created).unwrap(), b"draft");
                eprintln!("empty-file: PASS; body input and automatic save");
            }
            "picker" => {
                let created = root.join("C/from-dialog.md");
                assert_eq!(ws.file.as_deref(), Some(created.as_path()));
                assert!(ws.folder_expanded.contains(&root.join("C")));
                assert_eq!(std::fs::read(&created).unwrap(), b"draft");
                std::fs::write(&created, "external marker").unwrap();
            }
            "search-begin" => {
                ws.folder_content = true;
                ws.folder_query.update(cx, |q, cx| q.set_text("not-the-result", cx));
                ws.refresh_folder_hits(cx);
                ws.folder_query.update(cx, |q, cx| q.set_text("external marker", cx));
                ws.refresh_folder_hits(cx);
                assert!(ws.folder_searching);
            }
            "search-done" => {
                assert!(!ws.folder_searching);
                assert!(ws.folder_hits.iter().any(|hit| hit.path == root.join("C/from-dialog.md")));
                ws.folder_query.update(cx, |q, cx| q.set_text("", cx));
                ws.refresh_folder_hits(cx);
                assert!(ws.folder_hits.is_empty());
                eprintln!("folder-search: PASS; background content search and stale-query protection");
            }
            "duplicate" => {
                let created = root.join("C/from-dialog.md");
                assert_eq!(std::fs::read(&created).unwrap(), b"external marker");
                assert_eq!(ws.file.as_deref(), Some(created.as_path()));
            }
            "picked" => {
                assert_eq!(ws.file.as_deref(), Some(first.as_path()));
                assert!(ws.folder_expanded.contains(&root.join("A")));
                ws.close_folder(window, cx);
                eprintln!("folder-safety: PASS; background save, concurrent edit, switching, conflicts, recovery, file picker and safe creation");
            }
            _ => panic!("unknown folder safety phase"),
        }
    }).unwrap();
}

#[cfg(test)]
mod markdown_file_tests {
    use super::{parse_markdown_file, save_folder_document};

    #[test]
    fn saves_without_inserting_an_implicit_title() {
        let dir = std::env::temp_dir().join(format!("markdownx-no-heading-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("note.txt");
        let text = "\u{feff}First line\r\n\r\nSecond line\r\n";
        std::fs::write(&path, text).unwrap();
        let (doc, source) = crate::markdown_source::parse(&path, text);
        let saved = save_folder_document(&path, text, &doc, &source).unwrap();
        assert!(saved.text.starts_with("\u{feff}First line\r\n"));
        assert!(!saved.text.contains("# note"));
        let mut renamed = doc.clone();
        renamed.title = crate::model::RichText::plain("New title");
        let saved = save_folder_document(&path, &saved.text, &renamed, &saved.source).unwrap();
        assert!(saved.text.starts_with("\u{feff}# New title\r\n"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn parses_title_and_keeps_tables() {
        let dir = std::env::temp_dir().join(format!("markdownx-file-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lesson.md");
        let text = "# 力与运动\n\n| 量 | 单位 |\n|---|---|\n| 速度 | m/s |\n";
        std::fs::write(&path, text).unwrap();
        let (doc, _) = parse_markdown_file(&path, text);
        assert_eq!(doc.title.text, "力与运动");
        assert!(matches!(doc.blocks.first().map(|b| &b.kind), Some(crate::model::BlockKind::Table(_))));
        let opts = crate::markdown::Options { library_root: Some(dir.as_path()), ..Default::default() };
        let back = crate::markdown::export(&doc, &opts);
        assert!(back.text.contains("| 量 | 单位 |"));
        assert_eq!(back.text.matches("# 力与运动").count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_a_bom_before_the_title() {
        let (doc, _) = parse_markdown_file(std::path::Path::new("note.md"), "\u{feff}# 标题\r\n\r\n正文\r\n");
        assert_eq!(doc.title.text, "标题");
    }

    #[test]
    #[ignore = "reads the workspace specified by MARKDOWNX_WORKSPACE_TEST_ROOT"]
    fn real_workspace_is_readable() {
        let root = std::path::PathBuf::from(std::env::var_os("MARKDOWNX_WORKSPACE_TEST_ROOT").expect("set MARKDOWNX_WORKSPACE_TEST_ROOT"));
        let folder = crate::workspace_fs::WorkspaceDir::scan(root).unwrap();
        assert!(folder.warnings.is_empty());
        let files = folder.files();
        if let Ok(expected) = std::env::var("MARKDOWNX_EXPECTED_WORKSPACE_FILES") {
            assert_eq!(files.len(), expected.parse::<usize>().unwrap());
        }
        fn unescape(text: &str) -> String {
            let mut chars = text.chars().peekable();
            let mut out = String::new();
            while let Some(ch) = chars.next() {
                if ch == '\\' && chars.peek().is_some_and(|c| c.is_ascii_punctuation()) { out.push(chars.next().unwrap()); }
                else { out.push(ch); }
            }
            out
        }
        fn same_kind(a: &crate::model::BlockKind, b: &crate::model::BlockKind) -> bool {
            match (a, b) {
                (crate::model::BlockKind::Table(a), crate::model::BlockKind::Table(b)) => {
                    a.col_widths == b.col_widths && a.header_rows == b.header_rows && a.header_col == b.header_col
                        && a.rows.len() == b.rows.len() && a.rows.iter().zip(&b.rows).all(|(left, right)| {
                            left.len() == right.len() && left.iter().zip(right).all(|(x, y)| {
                                x.colspan == y.colspan && x.rowspan == y.rowspan && x.bg == y.bg && x.valign == y.valign && x.blocks.len() == y.blocks.len()
                            })
                        })
                }
                (a, b) => a == b,
            }
        }
        let mut checked = 0;
        let mut changed_text = 0;
        let mut changed_titles = 0;
        let mut missing_assets = 0;
        let mut asset_conflicts = 0;
        let mut changed_structure = 0;
        let mut changed_styles = 0;
        let mut changed_properties = 0;
        let mut guarded = 0;
        let mut causes = [0usize; 4];
        for file in files {
            if crate::workspace_fs::DocKind::of(&file.path) != Some(crate::workspace_fs::DocKind::Markdown) { continue; }
            let text = std::fs::read_to_string(&file.path).unwrap();
            let (doc, _) = parse_markdown_file(&file.path, &text);
            let opts = crate::markdown::Options { library_root: file.path.parent(), assets: Some("assets"), ..Default::default() };
            let exported = crate::markdown::export(&doc, &opts);
            let (reparsed, _) = parse_markdown_file(&file.path, &exported.text);
            let mut before_blocks = vec![];
            let mut after_blocks = vec![];
            doc.for_each_block(|b| before_blocks.push((b.kind.clone(), b.text.clone(), b.align, b.bg.clone(), b.folded, b.children.len())));
            reparsed.for_each_block(|b| after_blocks.push((b.kind.clone(), b.text.clone(), b.align, b.bg.clone(), b.folded, b.children.len())));
            let same_structure = before_blocks.len() == after_blocks.len() && before_blocks.iter().zip(&after_blocks)
                .all(|(x, y)| std::mem::discriminant(&x.0) == std::mem::discriminant(&y.0));
            changed_structure += usize::from(!same_structure);
            if same_structure {
                changed_styles += usize::from(before_blocks.iter().zip(&after_blocks).any(|(x, y)| x.1 != y.1));
                changed_properties += usize::from(before_blocks.iter().zip(&after_blocks).any(|(x, y)| {
                    !same_kind(&x.0, &y.0) || x.2 != y.2 || x.3 != y.3 || x.4 != y.4 || x.5 != y.5
                }));
            }
            let original = doc.plain_text();
            let after = reparsed.plain_text();
            let changed_title = doc.title.text != reparsed.title.text;
            let has_missing = exported.missing > 0;
            let parent = file.path.parent().unwrap();
            let canonical_root = parent.canonicalize().unwrap();
            let has_conflict = exported.files.iter().any(|(source, relative)| {
                let destination = parent.join(relative);
                let dir = destination.parent().unwrap();
                if dir.exists() && !dir.canonicalize().is_ok_and(|p| p.starts_with(&canonical_root)) { return true; }
                match std::fs::symlink_metadata(&destination) {
                    Ok(_) => !destination.canonicalize().is_ok_and(|p| p.starts_with(&canonical_root))
                        || !crate::workspace_fs::same_contents(source, &destination).unwrap_or(false),
                    Err(error) => error.kind() != std::io::ErrorKind::NotFound,
                }
            });
            changed_titles += usize::from(changed_title);
            missing_assets += usize::from(has_missing);
            asset_conflicts += usize::from(has_conflict);
            guarded += usize::from(changed_title || original != after || has_missing || has_conflict);
            if original != after {
                changed_text += 1;
                let kind = if unescape(&after) == original { 0 }
                    else if after.replace("<br>", "\n") == original { 1 }
                    else if after.chars().filter(|c| !c.is_whitespace()).eq(original.chars().filter(|c| !c.is_whitespace())) { 2 }
                    else { 3 };
                causes[kind] += 1;
                if std::env::var_os("MARKDOWNX_WORKSPACE_DIAGNOSTICS").is_some() {
                    let class = |ch: Option<char>| match ch {
                        None => "end",
                        Some('\n') => "newline",
                        Some('\\') => "backslash",
                        Some(' ') => "space",
                        Some(c) if c.is_whitespace() => "whitespace",
                        Some('*') => "star",
                        Some('_') => "underscore",
                        Some('~') => "tilde",
                        Some('`') => "backtick",
                        Some('[' | ']') => "bracket",
                        Some('<' | '>') => "angle",
                        Some(c) if c.is_alphanumeric() => "text",
                        Some(_) => "punctuation",
                    };
                    let prefix = original.chars().zip(after.chars()).take_while(|(a, b)| a == b).count();
                    let code = doc.blocks.iter().filter(|b| matches!(b.kind, crate::model::BlockKind::Code { .. })).count();
                    let tables = doc.blocks.iter().filter(|b| matches!(b.kind, crate::model::BlockKind::Table(_))).count();
                    let shape = |text: &str| text.chars().skip(prefix).take(24).map(|c| match c {
                        '\n' => 'N', '\\' => 'B', ' ' => 'S',
                        '*' | '_' | '~' | '`' => 'M', '[' | ']' => 'K', '<' | '>' => 'A',
                        c if c.is_whitespace() => 'S',
                        c if c.is_alphanumeric() => 'X',
                        _ => 'P',
                    }).collect::<String>();
                    eprintln!("difference: kind={kind}, before={}, after={}, lengths={}/{}, code={code}, tables={tables}, shape={}/{}",
                        class(original.chars().nth(prefix)), class(after.chars().nth(prefix)), original.chars().count(), after.chars().count(), shape(&original), shape(&after));
                }
            }
            checked += 1;
        }
        eprintln!("workspace-read: {checked} files; {guarded} would be protected; title differences={changed_titles}, text differences={changed_text}, structure differences={changed_structure}, inline style differences={changed_styles}, block property differences={changed_properties}, missing assets={missing_assets}, asset conflicts={asset_conflicts}; text causes: escapes={}, breaks={}, whitespace={}, other={}", causes[0], causes[1], causes[2], causes[3]);
        assert!(checked > 0);
    }

    #[test]
    fn recognizes_books_by_suffix() {
        assert_eq!(crate::workspace_fs::DocKind::of(std::path::Path::new("a.md")), Some(crate::workspace_fs::DocKind::Markdown));
        assert_eq!(crate::workspace_fs::DocKind::of(std::path::Path::new("a.mdxb")), Some(crate::workspace_fs::DocKind::Book));
        assert_eq!(crate::workspace_fs::DocKind::of(std::path::Path::new("a.png")), None);
    }
}
