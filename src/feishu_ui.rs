//! Feishu import dialog; authentication stays in the official CLI.

use crate::app::Workspace;
use crate::feishu::{self, ImportError, Imported};
use crate::input::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::trash::{close_button, dialog, primary_button, secondary_button};
use crate::ui::ToastKind;
use gpui::*;

pub struct FeishuDialog {
    field: Entity<TextField>,
    _subs: Vec<Subscription>,
    id: uuid::Uuid,
    busy: bool,
}

impl Workspace {
    pub(crate) fn open_feishu_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus();
        let field = cx.new(|cx| {
            let mut field = TextField::new(crate::settings::tr("粘贴飞书文档或知识库链接", "Paste a Feishu document or wiki link"), cx);
            field.max_len = Some(2048);
            field
        });
        let focus = field.focus_handle(cx);
        let subs = vec![cx.subscribe_in(&field, window, |w, _, event: &TextFieldEvent, window, cx| match event {
            TextFieldEvent::Submit => w.fetch_feishu(window, cx),
            TextFieldEvent::Cancel => { w.feishu_import = None; cx.notify(); }
            _ => {}
        })];
        self.feishu_import = Some(FeishuDialog { field, _subs: subs, id: uuid::Uuid::new_v4(), busy: false });
        window.focus(&focus);
        cx.notify();
    }

    fn fetch_feishu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.feishu_import else { return };
        if dialog.busy { return; }
        let target = match feishu::document_target(&dialog.field.read(cx).text) {
            Ok(target) => target,
            Err(_) => {
                self.show_toast_kind(ToastKind::Error, crate::settings::tr("请输入有效的飞书文档或知识库链接", "Enter a valid Feishu document or wiki link"), cx);
                return;
            }
        };
        dialog.busy = true;
        let id = dialog.id;
        cx.notify();
        let task = cx.background_executor().spawn(async move { feishu::fetch(&target) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let Some(dialog) = &mut w.feishu_import else { return };
                if dialog.id != id { return; }
                dialog.busy = false;
                match result {
                    Ok(imported) => w.accept_feishu(id, imported, window, cx),
                    Err(err) => {
                        let message = match err {
                            ImportError::InvalidLink => crate::settings::tr("飞书链接无效", "Invalid Feishu link"),
                            ImportError::MissingCli => crate::settings::tr("请先安装并登录飞书官方 CLI", "Install and sign in to the official Feishu CLI first"),
                            ImportError::Timeout => crate::settings::tr("获取飞书文档超时，请检查网络和登录状态", "Feishu import timed out; check your network and sign-in"),
                            ImportError::TooLarge => crate::settings::tr("飞书文档过大，未导入", "The Feishu document is too large to import"),
                            ImportError::InvalidResponse => crate::settings::tr("飞书返回的文档格式无法读取", "The Feishu document response could not be read"),
                            ImportError::Failed => crate::settings::tr("无法获取飞书文档，请检查 CLI 登录状态和文档权限", "Could not fetch the document; check CLI sign-in and document permissions"),
                        };
                        w.show_toast_kind(ToastKind::Error, message, cx);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn accept_feishu(&mut self, id: uuid::Uuid, imported: Imported, window: &mut Window, cx: &mut Context<Self>) {
        if !self.feishu_import.as_ref().is_some_and(|dialog| dialog.id == id) { return; }
        let retry = imported.clone();
        if !self.save_before(cx, move |w, window, cx| w.accept_feishu(id, retry, window, cx)) { return; }
        let workspace = self.active_workspace.and_then(|id| self.workspace_roots.iter().find(|r| r.id == id))
            .or_else(|| self.workspace_roots.first());
        let Some(workspace) = workspace.filter(|r| r.tree.is_some() && r.error.is_none()) else {
            self.show_toast_kind(ToastKind::Error, crate::settings::tr("请先打开一个可用的工作目录", "Open an available workspace folder first"), cx);
            return;
        };
        let workspace_id = workspace.id;
        let root = workspace.spec.path.clone();
        let tree = workspace.tree.clone();
        let protected = self.protected_paths(workspace_id);
        let (path, source_saved) = match feishu::save_in_workspace(&root, &protected, &imported) {
            Ok(saved) => saved,
            Err(_) => {
                self.show_toast_kind(ToastKind::Error, crate::settings::tr("无法在工作目录创建飞书文档", "Could not create the Feishu document in this workspace"), cx);
                return;
            }
        };
        self.feishu_import = None;
        self.active_workspace = Some(workspace_id);
        self.folder = tree;
        self.scan_workspace(workspace_id, false, cx);
        self.open_file_path(path, window, cx);
        let message = if !source_saved {
            crate::settings::tr("正文已导入，但无法保存飞书原始响应", "Content imported, but the original Feishu response could not be saved")
        } else if imported.has_references {
            crate::settings::tr("正文已导入；部分附件、评论或嵌入对象需另行处理，原始数据已保留", "Content imported; some attachments, comments or embeds need separate handling. Original data was retained.")
        } else {
            crate::settings::tr("飞书文档已导入", "Feishu document imported")
        };
        self.show_toast_kind(if source_saved { ToastKind::Success } else { ToastKind::Error }, message, cx);
    }

    pub(crate) fn render_feishu_import(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.feishu_import.as_ref()?;
        let busy = state.busy;
        let field = state.field.clone();
        let content = dialog("feishu-import")
            .relative().w(px(560.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().px(px(24.)).pt(px(24.)).pb(px(16.)).text_size(px(16.)).font_weight(FontWeight::SEMIBOLD)
                .child(crate::settings::tr("从飞书导入", "Import from Feishu")))
            .child(close_button("feishu-import-close").absolute().top(px(22.)).right(px(22.)).on_click(cx.listener(|w, _, _, cx| { w.feishu_import = None; cx.notify(); })))
            .child(div().px(px(24.)).pb(px(12.)).text_size(px(13.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("需要先安装并登录飞书官方 CLI，并获得文档读取权限。", "Install and sign in to the official Feishu CLI, and grant document read access.")))
            .child(div().mx(px(24.)).px(px(12.)).py(px(8.)).border_1().border_color(Theme::line_divider()).rounded(px(6.)).child(field))
            .child(div().px(px(24.)).pt(px(12.)).text_size(px(12.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("导入到当前工作目录。附件、评论和嵌入内容可能需要另行处理；原始响应会保存在旁边。", "Imports into the active workspace. Attachments, comments and embeds may need separate handling; the original response is saved alongside.")))
            .child(div().px(px(24.)).py(px(24.)).flex().justify_end().gap(px(12.))
                .child(secondary_button("feishu-import-cancel", crate::settings::tr("取消", "Cancel")).on_click(cx.listener(|w, _, _, cx| { w.feishu_import = None; cx.notify(); })))
                .child(primary_button("feishu-import-confirm", if busy { crate::settings::tr("正在获取…", "Fetching…") } else { crate::settings::tr("导入", "Import") })
                    .on_click(cx.listener(|w, _, window, cx| w.fetch_feishu(window, cx)))));
        Some(crate::transfer::mask("feishu-import-mask", content, |w, cx| { w.feishu_import = None; cx.notify(); }, cx))
    }
}

#[cfg(feature = "devtools")]
pub(crate) fn check_feishu_import(phase: &str, cx: &mut App) {
    let root = std::path::PathBuf::from(std::env::var_os("MARKDOWNX_TEST_WORKSPACE_ROOT").expect("test root"));
    assert!(root.starts_with(std::env::temp_dir()) && root.join(".markdownx-workspace-test").is_file());
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |w, window, cx| {
        #[cfg(target_os = "windows")]
        assert!(window.bounds().origin.x < px(-10000.), "test window must remain off-screen");
        match phase {
            "start" => {
                w.open_feishu_import(window, cx);
                w.feishu_import.as_ref().unwrap().field.update(cx, |field, cx| field.set_text("fakeDocumentToken", cx));
                w.fetch_feishu(window, cx);
            }
            "done" => {
                let path = w.file.as_ref().expect("imported file");
                assert!(path.starts_with(&root) && path.extension().is_some_and(|ext| ext == "docxxml"));
                assert!(crate::workspace_files::feishu_sidecar(path).is_file());
                assert_eq!(w.editor.as_ref().unwrap().read(cx).core.doc.title.text, "Fixture");
                assert!(w.feishu_import.is_none());
            }
            _ => panic!("unknown Feishu import test phase"),
        }
    }).unwrap();
    eprintln!("feishu-import: {phase} PASS");
}

#[cfg(feature = "devtools")]
pub(crate) fn check_native_experiment(kind: &str, cx: &mut App) {
    let handle = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()).unwrap();
    handle.update(cx, |w, window, cx| {
        #[cfg(target_os = "windows")]
        assert!(window.bounds().origin.x < px(-10000.), "test window must remain off-screen");
        let _ = window;
        w.editor.as_ref().expect("editor").read(cx).check_native_experiment(kind);
    }).unwrap();
    eprintln!("native-experiment: {kind} PASS");
}
