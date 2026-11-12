//! Multiple registered filesystem workspaces and their directory trees.

use crate::{app::{Confirm, View, Workspace}, input::{TextField, TextFieldEvent}, ui::ToastKind, workspace_fs::{WorkspaceDir, WorkspaceSpec}};
use gpui::*;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct WorkspaceRoot {
    pub id: u64,
    pub spec: WorkspaceSpec,
    pub tree: Option<WorkspaceDir>,
    pub expanded_paths: HashSet<PathBuf>,
    pub loading: bool,
    pub error: Option<String>,
    pub scan_id: u64,
    pub persistent: bool,
    pub canonical: Option<PathBuf>,
}

#[derive(Clone)]
pub(crate) enum WorkspaceMenuTarget {
    Root(u64),
    Node { root: u64, path: PathBuf, dir: bool },
    New { root: u64, parent: PathBuf, dir: bool },
}

#[derive(Clone)]
pub(crate) struct WorkspaceMenu {
    pub target: WorkspaceMenuTarget,
    pub pos: Point<Pixels>,
}

pub(crate) struct WorkspaceRename {
    pub target: WorkspaceMenuTarget,
    pub field: Entity<crate::input::TextField>,
    pub _subs: Vec<Subscription>,
}

pub(crate) fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().replace('/', "\\").eq_ignore_ascii_case(&b.to_string_lossy().replace('/', "\\")) } else { a == b }
}

fn file_diagnostic(error: &anyhow::Error) -> String {
    let message = error.to_string();
    if !crate::settings::english() {
        let translated = match message.as_str() {
            "invalid name" => Some("名称无效，请使用普通文件名"),
            "invalid workspace path" | "path is outside this workspace" => Some("路径不在工作目录内"),
            "cannot change a workspace root" => Some("不能操作工作区根目录"),
            "path overlaps a protected directory or another workspace" | "parent is protected" => Some("此位置包含文档库或其他工作区，不能操作"),
            "symbolic links and junctions cannot be changed" => Some("不能操作符号链接或目录联接"),
            "choose a Markdown or text filename" => Some("请输入 Markdown 或文本文件名"),
            "the document type cannot be changed by renaming" => Some("不能通过重命名改变文件类型"),
            "destination annotation sidecar already exists" | "an annotation sidecar already exists for this name" => Some("同名批注文件已存在，未覆盖"),
            "cannot identify the annotation sidecar" => Some("无法确认批注文件的归属，已保留原文件"),
            "recycle operation was cancelled or aborted" => Some("回收站操作已取消或中止"),
            _ => None,
        };
        if let Some(translated) = translated { return translated.to_string(); }
    }
    crate::settings::diagnostic(&message)
}

impl Workspace {
    pub(crate) fn workspace_path_busy(&self, path: &std::path::Path) -> bool {
        self.workspace_busy.iter().any(|pending| path.starts_with(pending) || pending.starts_with(path))
    }

    pub(crate) fn workspace_busy_notice(&mut self, cx: &mut Context<Self>) {
        self.show_toast_kind(ToastKind::Info, crate::settings::tr("该目录或文件正在处理中，请稍候", "This folder or file is being updated"), cx);
    }

    fn next_workspace_serial(&mut self) -> u64 {
        self.workspace_serial = self.workspace_serial.wrapping_add(1);
        self.workspace_serial
    }

    pub(crate) fn save_workspaces(&mut self, cx: &mut Context<Self>) {
        let roots = self.workspace_roots.iter().filter(|r| r.persistent).map(|r| r.spec.clone()).collect::<Vec<_>>();
        let active = self.active_workspace.and_then(|id| self.workspace_roots.iter().find(|r| r.id == id && r.persistent)).map(|r| r.spec.path.clone())
            .or_else(|| crate::settings::prefs().workspace_dir.filter(|p| roots.iter().any(|r| same_path(&r.path, p))));
        self.set_pref(|p| {
            if p.last_workspace_file.as_ref().is_some_and(|file| !roots.iter().any(|root| file.starts_with(&root.path))) { p.last_workspace_file = None; }
            p.workspaces = Some(roots); p.workspace_dir = active;
        }, cx);
    }

    pub(crate) fn restore_workspaces(&mut self, documents: Option<PathBuf>, cx: &mut Context<Self>) {
        let prefs = crate::settings::prefs();
        let mut create_default = None;
        let specs = match prefs.workspaces {
            Some(specs) => specs,
            None => match prefs.workspace_dir.clone() {
                Some(path) => vec![WorkspaceSpec::new(path)],
                None => match documents.filter(|p| p.is_absolute()) {
                    Some(dir) => {
                        let path = dir.join(crate::workspace_fs::DEFAULT_WORKSPACE_DIR);
                        create_default = Some(path.clone());
                        vec![WorkspaceSpec::new(path)]
                    }
                    None => {
                        self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法确定系统文档目录", "System Documents folder unavailable"), cx);
                        vec![]
                    }
                },
            },
        };
        for spec in specs {
            let create = create_default.as_ref().is_some_and(|path| same_path(path, &spec.path));
            let id = self.register_workspace(spec, true, create, cx);
            if prefs.workspace_dir.as_ref().is_some_and(|path| self.workspace_roots.iter().any(|r| r.id == id && same_path(path, &r.spec.path))) {
                self.active_workspace = Some(id);
            }
        }
        if self.active_workspace.is_none() { self.active_workspace = self.workspace_roots.first().map(|r| r.id); }
    }

    pub(crate) fn register_workspace(&mut self, spec: WorkspaceSpec, persistent: bool, create: bool, cx: &mut Context<Self>) -> u64 {
        if let Some(existing) = self.workspace_roots.iter_mut().find(|r| same_path(&r.spec.path, &spec.path)) {
            let id = existing.id;
            existing.spec.expanded = true;
            existing.persistent |= persistent;
            cx.notify();
            return id;
        }
        let id = self.next_workspace_serial();
        self.workspace_roots.push(WorkspaceRoot { id, spec, tree: None, expanded_paths: HashSet::new(), loading: true, error: None, scan_id: 0, persistent, canonical: None });
        if self.active_workspace.is_none() { self.active_workspace = Some(id); }
        self.scan_workspace(id, create, cx);
        cx.notify();
        id
    }

    pub(crate) fn register_workspace_snapshot(&mut self, tree: WorkspaceDir, persistent: bool, cx: &mut Context<Self>) -> u64 {
        let scan_id = self.next_workspace_serial();
        let id = if let Some(root) = self.workspace_roots.iter_mut().find(|r| same_path(&r.spec.path, &tree.root)) {
            root.tree = Some(tree);
            root.loading = false;
            root.error = None;
            root.scan_id = scan_id;
            root.persistent |= persistent;
            root.id
        } else {
            let id = self.next_workspace_serial();
            self.workspace_roots.push(WorkspaceRoot { id, spec: WorkspaceSpec::new(tree.root.clone()), tree: Some(tree), expanded_paths: HashSet::new(), loading: false, error: None, scan_id, persistent, canonical: None });
            id
        };
        cx.notify();
        id
    }

    pub(crate) fn scan_workspace(&mut self, id: u64, create: bool, cx: &mut Context<Self>) {
        let scan_id = self.next_workspace_serial();
        let Some(root) = self.workspace_roots.iter_mut().find(|r| r.id == id) else { return };
        root.scan_id = scan_id;
        root.loading = true;
        root.error = None;
        let path = root.spec.path.clone();
        let task = cx.background_executor().spawn(async move {
            anyhow::ensure!(path.is_absolute(), "workspace path must be absolute");
            if create { crate::workspace_fs::initial_workspace(None, path.parent().map(std::path::Path::to_path_buf))?; }
            let canonical = path.canonicalize()?;
            let tree = WorkspaceDir::scan(path)?;
            Ok::<_, anyhow::Error>((canonical, tree))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |w, cx| {
                let Some(index) = w.workspace_roots.iter().position(|r| r.id == id && r.scan_id == scan_id) else { return };
                match result {
                    Ok((canonical, tree)) => {
                        if let Some(other) = w.workspace_roots.iter().find(|r| r.id != id && r.canonical.as_ref() == Some(&canonical)).map(|r| r.id) {
                            let persistent = w.workspace_roots[index].persistent;
                            w.workspace_roots.remove(index);
                            if let Some(root) = w.workspace_roots.iter_mut().find(|r| r.id == other) { root.persistent |= persistent; root.spec.expanded = true; }
                            if w.active_workspace == Some(id) {
                                w.active_workspace = Some(other);
                                w.folder = w.workspace_roots.iter().find(|r| r.id == other).and_then(|r| r.tree.clone());
                            }
                            if persistent { w.save_workspaces(cx); }
                            w.show_toast_kind(ToastKind::Info, crate::settings::tr("该目录已添加", "This folder is already added"), cx);
                        } else {
                            let root = &mut w.workspace_roots[index];
                            root.canonical = Some(canonical);
                            root.tree = Some(tree.clone());
                            root.loading = false;
                            root.error = None;
                            if w.active_workspace == Some(id) {
                                w.folder = Some(tree);
                                w.refresh_folder_links(cx);
                            }
                        }
                    }
                    Err(error) => {
                        let root = &mut w.workspace_roots[index];
                        root.loading = false;
                        root.error = Some(crate::settings::diagnostic(&error.to_string()));
                    }
                }
                w.refresh_folder_hits(cx);
                w.refresh_global_folder_hits(cx);
                cx.notify();
            });
        }).detach();
    }

    pub(crate) fn add_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions {
            files: false, directories: true, multiple: true,
            prompt: Some(crate::settings::tr("添加工作目录", "Add workspace folders").into()), extensions: None,
        }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt { Some(prompt) => prompt.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(paths) = paths else { return };
            let _ = this.update_in(cx, |w, _, cx| {
                for path in paths {
                    let path = if path.is_absolute() { path } else { match std::env::current_dir() { Ok(dir) => dir.join(path), Err(error) => { w.show_toast_kind(ToastKind::Error, &error.to_string(), cx); continue; } } };
                    w.register_workspace(WorkspaceSpec::new(path), true, false, cx);
                }
                w.save_workspaces(cx);
            });
        }).detach();
    }

    pub(crate) fn refresh_workspaces(&mut self, cx: &mut Context<Self>) {
        let ids = self.workspace_roots.iter().filter(|r| !r.loading).map(|r| r.id).collect::<Vec<_>>();
        for id in ids { self.scan_workspace(id, false, cx); }
    }

    pub(crate) fn open_workspace_file(&mut self, id: u64, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_path_busy(&path) { self.workspace_busy_notice(cx); return; }
        if crate::workspace_fs::DocKind::of(&path).is_none() {
            self.show_toast_kind(ToastKind::Info, crate::settings::tr("请从文件菜单选择「使用系统应用打开」", "Use the file menu to open this with your system app"), cx);
            return;
        }
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.open_workspace_file_now(id, retry, window, cx)) {
            self.open_workspace_file_now(id, path, window, cx);
        }
    }

    pub(crate) fn open_workspace_file_now(&mut self, id: u64, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_path_busy(&path) { self.workspace_busy_notice(cx); return; }
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id) else { return };
        let Some(tree) = root.tree.clone() else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("目录尚未加载", "Folder is not loaded"), cx);
            return;
        };
        if !tree.all_files().iter().any(|file| same_path(&file.path, &path)) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("文件不在此工作目录中", "The file is not in this workspace"), cx);
            return;
        }
        let changed = self.active_workspace != Some(id);
        let persistent = root.persistent;
        self.active_workspace = Some(id);
        self.folder = Some(tree.clone());
        if let Some(root) = self.workspace_roots.iter_mut().find(|r| r.id == id) {
            root.spec.expanded = true;
            let mut parent = path.parent();
            while let Some(dir) = parent {
                if dir == tree.root || !dir.starts_with(&tree.root) { break; }
                root.expanded_paths.insert(dir.to_path_buf());
                parent = dir.parent();
            }
            self.folder_expanded = root.expanded_paths.clone();
        }
        if changed && persistent { self.save_workspaces(cx); }
        self.refresh_folder_links(cx);
        if self.file.as_ref().is_some_and(|file| same_path(file, &path)) && self.editor.is_some() { self.show_view(View::Doc, cx); }
        else { self.open_file_path(path, window, cx); }
    }

    pub(crate) fn open_workspace_external(&mut self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace_menu = None;
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id) else { return };
        if !root.tree.as_ref().is_some_and(|tree| tree.all_files().iter().any(|node| same_path(&node.path, &path))) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("文件不在此工作目录中", "The file is not in this workspace"), cx); return;
        }
        if let Err(error) = crate::workspace_files::validate_external_open(&root.spec.path, &path) {
            self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法打开文件：{}", "Could not open file: {}", file_diagnostic(&error)), cx); return;
        }
        let Ok(url) = url::Url::from_file_path(&path) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法识别文件路径", "Could not identify the file path"), cx); return;
        };
        if crate::devtools::scripted() { eprintln!("devtools: external workspace file"); return; }
        cx.open_url(url.as_str());
    }

    pub(crate) fn remove_workspace(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id) else { return };
        if self.workspace_path_busy(&root.spec.path) { self.workspace_busy_notice(cx); return; }
        let closes_file = self.active_workspace == Some(id) && self.file.as_ref().is_some_and(|p| p.starts_with(&root.spec.path));
        if closes_file && !self.save_before(cx, move |w, window, cx| w.remove_workspace_now(id, window, cx)) { return }
        self.remove_workspace_now(id, window, cx);
    }

    fn remove_workspace_now(&mut self, id: u64, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.workspace_roots.iter().position(|r| r.id == id) else { return };
        if self.workspace_path_busy(&self.workspace_roots[index].spec.path) { self.workspace_busy_notice(cx); return; }
        let removed = self.workspace_roots.remove(index);
        let closes_file = self.active_workspace == Some(id) && self.file.as_ref().is_some_and(|p| p.starts_with(&removed.spec.path));
        if closes_file { self.close_folder_now(cx); }
        if self.active_workspace == Some(id) {
            self.active_workspace = self.workspace_roots.first().map(|r| r.id);
            self.folder = self.workspace_roots.first().and_then(|r| r.tree.clone());
            self.refresh_folder_links(cx);
        }
        self.workspace_menu = None;
        self.workspace_rename = None;
        self.save_workspaces(cx);
        self.refresh_folder_hits(cx);
        self.refresh_global_folder_hits(cx);
        self.show_toast_kind(ToastKind::Info, crate::settings::tr("已移除工作区，磁盘文件未删除", "Workspace removed; its files remain on disk"), cx);
        cx.notify();
    }

    fn protected_paths(&self, id: u64) -> Vec<PathBuf> {
        let root = self.workspace_roots.iter().find(|r| r.id == id);
        let mut paths = vec![crate::store::Store::root_dir()];
        paths.extend(self.workspace_roots.iter().filter(|r| r.id != id && root.is_some_and(|root| {
            match (&root.canonical, &r.canonical) {
                (Some(parent), Some(child)) => child.starts_with(parent),
                _ => r.spec.path.starts_with(&root.spec.path),
            }
        })).map(|r| r.spec.path.clone()));
        paths
    }

    pub(crate) fn prompt_workspace_new(&mut self, id: u64, parent: Option<PathBuf>, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace_roots.iter_mut().find(|r| r.id == id && r.tree.is_some() && r.error.is_none()) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("目录尚未加载或无法读取", "Folder is unavailable or still loading"), cx); return;
        };
        let parent = parent.unwrap_or_else(|| root.spec.path.clone());
        root.spec.expanded = true;
        if parent != root.spec.path { root.expanded_paths.insert(parent.clone()); }
        self.start_workspace_rename(WorkspaceMenuTarget::New { root: id, parent, dir }, window, cx);
    }

    pub(crate) fn start_workspace_rename(&mut self, target: WorkspaceMenuTarget, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match &target {
            WorkspaceMenuTarget::Root(id) => self.workspace_roots.iter().find(|r| r.id == *id).map(|r| r.spec.name()),
            WorkspaceMenuTarget::Node { path, .. } => path.file_name().map(|s| s.to_string_lossy().into_owned()),
            WorkspaceMenuTarget::New { .. } => Some(String::new()),
        };
        let Some(initial) = initial else { return };
        let field = cx.new(|cx| { let mut f = TextField::new(crate::settings::tr("输入名称", "Enter a name"), cx); f.set_text(&initial, cx); f.select_all_text(cx); f });
        let focus = field.focus_handle(cx);
        let subs = vec![
            cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
                TextFieldEvent::Submit => w.finish_workspace_rename(true, window, cx),
                TextFieldEvent::Cancel => w.finish_workspace_rename(false, window, cx), _ => {}
            }),
            cx.on_blur(&focus, window, |w, window, cx| w.finish_workspace_rename(true, window, cx)),
        ];
        self.workspace_menu = None;
        self.workspace_rename = Some(WorkspaceRename { target, field, _subs: subs });
        window.focus(&focus);
        cx.notify();
    }

    fn finish_workspace_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.workspace_rename.take() else { return };
        let name = rename.field.read(cx).text.trim().to_string();
        if !commit || name.is_empty() { cx.notify(); return; }
        match rename.target {
            WorkspaceMenuTarget::Root(id) => {
                if let Some(root) = self.workspace_roots.iter_mut().find(|r| r.id == id) { root.spec.label = Some(name); }
                self.save_workspaces(cx);
            }
            WorkspaceMenuTarget::New { root, parent, dir } => {
                let Some(path) = self.workspace_roots.iter().find(|r| r.id == root).map(|r| r.spec.path.clone()) else { return };
                let protected = self.protected_paths(root);
                let editing = (self.file.clone(), self.current.clone(), self.file_revision);
                let mut pending = parent.join(&name);
                if !dir && pending.extension().is_none() { pending.set_extension("md"); }
                if self.workspace_path_busy(&pending) { self.workspace_busy_notice(cx); return; }
                self.workspace_busy.insert(pending.clone());
                let task = cx.background_executor().spawn(async move { crate::workspace_files::create(&path, &parent, &name, dir, &protected) });
                cx.spawn_in(window, async move |this, cx| {
                    let result = task.await;
                    let _ = this.update_in(cx, |w, window, cx| {
                        w.workspace_busy.remove(&pending);
                        match result {
                            Ok(path) => {
                                w.scan_workspace(root, false, cx);
                                w.show_toast_kind(ToastKind::Success, crate::settings::tr("已创建", "Created"), cx);
                                if !dir && (w.file.clone(), w.current.clone(), w.file_revision) == editing {
                                    let retry = path.clone();
                                    if w.save_before(cx, move |w, window, cx| w.open_created_workspace_file(root, retry, window, cx)) {
                                        w.open_created_workspace_file(root, path, window, cx);
                                    }
                                }
                            }
                            Err(e) => w.show_toast_kind(ToastKind::Error, &crate::localized_format!("创建失败：{}", "Could not create: {}", file_diagnostic(&e)), cx),
                        }
                    });
                }).detach();
            }
            WorkspaceMenuTarget::Node { root, path, dir } => {
                if path.file_name().is_some_and(|n| n == name.as_str()) { return }
                let retry_path = path.clone(); let retry_name = name.clone();
                if self.save_before(cx, move |w, window, cx| w.rename_workspace_node_now(root, retry_path, retry_name, dir, window, cx)) {
                    self.rename_workspace_node_now(root, path, name, dir, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn open_created_workspace_file(&mut self, id: u64, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id) else { return };
        let Some(tree) = root.tree.clone() else { return };
        self.active_workspace = Some(id);
        self.folder = Some(tree);
        self.save_workspaces(cx);
        self.refresh_folder_links(cx);
        self.open_file_path(path, window, cx);
    }

    fn rename_workspace_node_now(&mut self, id: u64, path: PathBuf, name: String, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_path_busy(&path) { self.workspace_busy_notice(cx); return; }
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id).map(|r| r.spec.path.clone()) else { return };
        let protected = self.protected_paths(id);
        let affected = self.file.clone().filter(|file| file.starts_with(&path));
        let destination = path.parent().map(|parent| parent.join(&name));
        if destination.as_ref().is_some_and(|dest| self.workspace_path_busy(dest)) { self.workspace_busy_notice(cx); return; }
        self.workspace_busy.insert(path.clone());
        if let Some(dest) = &destination { self.workspace_busy.insert(dest.clone()); }
        if affected.is_some() { self.close_folder_now(cx); }
        let source = path.clone();
        let task = cx.background_executor().spawn(async move { crate::workspace_files::rename(&root, &source, &name, dir, &protected) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                w.workspace_busy.remove(&path);
                if let Some(dest) = &destination { w.workspace_busy.remove(dest); }
                let reopen = match &result {
                    Ok(dest) => affected.as_ref().map(|file| if file == &path { dest.clone() } else { dest.join(file.strip_prefix(&path).expect("affected file is inside renamed folder")) }),
                    Err(_) => affected.clone(),
                };
                if let Err(e) = result { w.show_toast_kind(ToastKind::Error, &crate::localized_format!("重命名失败：{}", "Could not rename: {}", file_diagnostic(&e)), cx); }
                else { w.show_toast_kind(ToastKind::Success, crate::settings::tr("已重命名", "Renamed"), cx); }
                w.scan_workspace(id, false, cx);
                if w.file.is_none() && w.current.is_none() && w.view == View::Home && let Some(file) = reopen && file.is_file() {
                    w.active_workspace = Some(id);
                    w.folder = w.workspace_roots.iter().find(|r| r.id == id).and_then(|r| r.tree.clone());
                    w.open_file_path(file, window, cx);
                }
            });
        }).detach();
    }

    pub(crate) fn ask_trash_workspace_node(&mut self, id: u64, path: PathBuf, dir: bool, cx: &mut Context<Self>) {
        self.workspace_menu = None;
        self.confirm = Some(Confirm::TrashWorkspaceNode { root: id, path, dir });
        cx.notify();
    }

    pub(crate) fn trash_workspace_node(&mut self, id: u64, path: PathBuf, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let retry = path.clone();
        if self.save_before(cx, move |w, window, cx| w.trash_workspace_node_now(id, retry, dir, window, cx)) {
            self.trash_workspace_node_now(id, path, dir, window, cx);
        }
    }

    fn trash_workspace_node_now(&mut self, id: u64, path: PathBuf, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_path_busy(&path) { self.workspace_busy_notice(cx); return; }
        let Some(root) = self.workspace_roots.iter().find(|r| r.id == id).map(|r| r.spec.path.clone()) else { return };
        let protected = self.protected_paths(id);
        let affected = self.file.clone().filter(|file| file.starts_with(&path));
        self.workspace_busy.insert(path.clone());
        let pending = path.clone();
        if affected.is_some() { self.close_folder_now(cx); }
        let task = cx.background_executor().spawn(async move { crate::workspace_files::trash(&root, &path, dir, &protected) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                w.workspace_busy.remove(&pending);
                match result {
                    Ok(()) => w.show_toast_kind(ToastKind::Success, crate::settings::tr("已移入系统回收站", "Moved to the system recycle bin"), cx),
                    Err(e) => {
                        w.show_toast_kind(ToastKind::Error, &crate::localized_format!("移入回收站失败：{}", "Could not move to the recycle bin: {}", file_diagnostic(&e)), cx);
                        if w.file.is_none() && w.current.is_none() && w.view == View::Home && let Some(file) = affected && file.is_file() { w.open_file_path(file, window, cx); }
                    }
                }
                w.scan_workspace(id, false, cx);
            });
        }).detach();
    }

    pub(crate) fn replace_workspace(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_roots.iter().find(|r| r.id == id).is_some_and(|r| self.workspace_path_busy(&r.spec.path)) { self.workspace_busy_notice(cx); return; }
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(crate::settings::tr("更换工作目录", "Change workspace folder").into()), extensions: None }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(path) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| {
                let retry = path.clone();
                if w.save_before(cx, move |w, _, cx| w.replace_workspace_now(id, retry, cx)) { w.replace_workspace_now(id, path, cx); }
                let _ = window;
            });
        }).detach();
    }

    fn replace_workspace_now(&mut self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        if self.workspace_roots.iter().find(|r| r.id == id).is_some_and(|r| self.workspace_path_busy(&r.spec.path)) { self.workspace_busy_notice(cx); return; }
        if !path.is_absolute() || self.workspace_roots.iter().any(|r| r.id != id && same_path(&r.spec.path, &path)) {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("目录无效或已经添加", "Invalid or already added folder"), cx); return;
        }
        if self.active_workspace == Some(id) && self.file.is_some() { self.close_folder_now(cx); }
        if let Some(root) = self.workspace_roots.iter_mut().find(|r| r.id == id) {
            root.spec.path = path; root.spec.label = None; root.spec.expanded = true;
            root.tree = None; root.error = None; root.canonical = None; root.expanded_paths.clear(); root.persistent = true;
        } else { return }
        self.save_workspaces(cx);
        self.scan_workspace(id, false, cx);
    }
}
