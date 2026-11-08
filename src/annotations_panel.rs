//! 批注: a side panel over the page. Annotations live in a sidecar next to a
//! Markdown file, or under `annotations/` for library pages, so the document
//! format stays clean and two readers can merge their notes.

use fdl_core::annotations::AnnotationStore;
use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::app::{Workspace, HEADER_H};
use crate::model::{Block, BlockId};
use crate::theme::Theme;
use crate::ui::icon;

/// Annotation bodies highlight `@name` spans. The names are local display
/// handles (the same identity used by the sidecar author), not accounts.
fn mention_text(body: &str) -> StyledText {
    let ranges = fdl_core::annotations::mentions(body);
    if ranges.is_empty() {
        return StyledText::new(body.to_string());
    }
    let font = Font {
        family: crate::theme::UI_FONT.into(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight(400.),
        style: FontStyle::Normal,
    };
    let mut runs = vec![];
    let mut at = 0;
    for r in ranges {
        if at < r.start {
            runs.push(TextRun { len: r.start - at, font: font.clone(), color: Theme::text(), background_color: None, underline: None, strikethrough: None });
        }
        runs.push(TextRun {
            len: r.end - r.start,
            font: font.clone(),
            color: Theme::primary_500(),
            background_color: Some(Theme::primary_500().opacity(0.1)),
            underline: None,
            strikethrough: None,
        });
        at = r.end;
    }
    if at < body.len() {
        runs.push(TextRun { len: body.len() - at, font, color: Theme::text(), background_color: None, underline: None, strikethrough: None });
    }
    StyledText::new(body.to_string()).with_runs(runs)
}

impl Workspace {
    pub(crate) fn reload_annotations(&mut self, cx: &mut Context<Self>) {
        let (key, path) = match (&self.file, &self.current) {
            (Some(file), _) => (file.display().to_string(), fdl_core::annotations::sidecar(file)),
            (None, Some(id)) => (id.clone(), fdl_core::annotations::library_sidecar(&crate::store::Store::root_dir(), id)),
            _ => {
                self.annotations = AnnotationStore::new("");
                self.annotations_path = None;
                self.annotations_open = false;
                self.annotation_selected.clear();
                self.annotation_editing = None;
                self.annotation_draft.update(cx, |f, cx| f.set_text("", cx));
                return;
            }
        };
        if self.annotations.doc != key {
            self.annotations = AnnotationStore::load_or_new(&path, key);
            self.annotations_open = false;
            self.annotation_selected.clear();
            self.annotation_editing = None;
            self.annotation_draft.update(cx, |f, cx| f.set_text("", cx));
        }
        self.annotations_path = Some(path);
        cx.notify();
    }

    fn save_annotations(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.annotations_path.clone() else { return };
        if let Err(e) = self.annotations.save(&path) {
            self.show_toast_kind(crate::ui::ToastKind::Error, &crate::settings::diagnostic(&e.to_string()), cx);
        }
    }

    pub(crate) fn toggle_annotations(&mut self, cx: &mut Context<Self>) {
        self.annotations_open = !self.annotations_open;
        if self.annotations_open {
            self.reload_annotations(cx);
        }
        cx.notify();
    }

    pub(crate) fn close_annotations(&mut self, cx: &mut Context<Self>) {
        self.annotations_open = false;
        self.annotation_editing = None;
        self.annotation_draft.update(cx, |f, cx| f.set_text("", cx));
        cx.notify();
    }

    pub(crate) fn begin_annotation_edit(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(a) = self.annotations.annotations.iter().find(|a| a.id == id) else { return };
        let body = a.body.clone();
        self.annotation_editing = Some(id.to_string());
        self.annotation_draft.update(cx, |f, cx| f.set_text(&body, cx));
        cx.notify();
    }

    pub(crate) fn cancel_annotation_edit(&mut self, cx: &mut Context<Self>) {
        self.annotation_editing = None;
        self.annotation_draft.update(cx, |f, cx| f.set_text("", cx));
        cx.notify();
    }

    pub(crate) fn submit_annotation(&mut self, cx: &mut Context<Self>) {
        let body = self.annotation_draft.read(cx).text.trim().to_string();
        if body.is_empty() { return; }
        if let Some(id) = self.annotation_editing.clone() {
            if !self.annotations.update(&id, &body) { return; }
        } else {
            let (quote, anchor) = self.annotation_anchor(cx);
            self.annotations.add(self.annotation_author.clone(), body, quote, anchor);
        }
        self.save_annotations(cx);
        self.annotation_editing = None;
        self.annotation_draft.update(cx, |f, cx| f.set_text("", cx));
        cx.notify();
    }

    pub(crate) fn delete_annotation(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.annotations.remove(id) {
            self.annotation_selected.remove(id);
            self.save_annotations(cx);
            cx.notify();
        }
    }

    pub(crate) fn set_annotation_resolved(&mut self, id: &str, resolved: bool, cx: &mut Context<Self>) {
        if self.annotations.set_resolved(id, resolved) {
            self.save_annotations(cx);
            cx.notify();
        }
    }

    pub(crate) fn toggle_annotation_selected(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.annotation_selected.remove(id) {
            self.annotation_selected.insert(id.to_string());
        }
        cx.notify();
    }

    pub(crate) fn resolve_selected_annotations(&mut self, resolved: bool, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.annotation_selected.iter().cloned().collect();
        if ids.is_empty() { return; }
        self.annotations.batch_resolve(&ids, resolved);
        self.annotation_selected.clear();
        self.save_annotations(cx);
        cx.notify();
    }

    pub(crate) fn delete_selected_annotations(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.annotation_selected.iter().cloned().collect();
        if ids.is_empty() { return; }
        self.annotations.batch_remove(&ids);
        self.annotation_selected.clear();
        self.save_annotations(cx);
        cx.notify();
    }

    fn annotation_anchor(&self, cx: &App) -> (String, Vec<usize>) {
        let Some(ed) = &self.editor else { return (String::new(), vec![]) };
        let core = &ed.read(cx).core;
        let head = core.sel.head;
        let path = block_path(&core.doc.blocks, head.block).unwrap_or_default();
        let text = core.text_of(head.block).map(|t| t.text.as_str()).unwrap_or("");
        let (a, b) = if !core.sel.is_collapsed() && core.sel.anchor.block == head.block {
            (core.sel.anchor.offset.min(text.len()), head.offset.min(text.len()))
        } else {
            (0, text.len().min(120))
        };
        let quote = text.get(a.min(b)..b.max(a)).unwrap_or("").trim().to_string();
        (quote, path)
    }

    pub(crate) fn merge_annotations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::settings::text("合并批注").into()),
            extensions: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = prompt.await.ok().and_then(Result::ok).flatten().and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, _, cx| {
                match AnnotationStore::load(&path) {
                    Ok(other) => {
                        w.annotations.merge(other);
                        w.save_annotations(cx);
                        w.show_toast_kind(crate::ui::ToastKind::Success, crate::settings::text("已合并批注"), cx);
                        cx.notify();
                    }
                    Err(e) => w.show_toast_kind(crate::ui::ToastKind::Error, &crate::settings::diagnostic(&e.to_string()), cx),
                }
            });
        })
        .detach();
    }

    pub(crate) fn render_annotations(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.annotations_open || (self.current.is_none() && self.file.is_none()) { return None; }
        let count = self.annotations.unresolved();
        let rows = self.annotations.annotations.clone();
        let selected = self.annotation_selected.clone();
        let editing = self.annotation_editing.clone();

        let mut list = div().id("ann-list").flex_1().min_h_0().overflow_y_scroll().py(px(6.));
        if rows.is_empty() {
            list = list.child(div().px(px(16.)).py(px(24.)).text_size(px(12.)).text_color(Theme::text_caption()).child(crate::settings::tr("还没有批注。选中一段文字，写下你的想法。", "No notes yet. Select some text and write what you notice.")));
        }
        for (i, a) in rows.iter().enumerate() {
            let id = a.id.clone();
            let id_select = id.clone();
            let id_edit = id.clone();
            let id_delete = id.clone();
            let id_resolve = id.clone();
            let resolved = a.resolved;
            let on = selected.contains(&id);
            let row = div()
                .id(("ann-row", i))
                .mx(px(10.))
                .my(px(4.))
                .p(px(10.))
                .rounded(px(7.))
                .when(on, |d| d.bg(Theme::fill_hover()))
                .when(a.resolved, |d| d.opacity(0.55))
                .hover(|d| d.bg(Theme::fill_hover()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .id(("ann-check", i))
                                .size(px(14.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(Theme::line_divider())
                                .when(on, |d| d.bg(Theme::primary_500()))
                                .cursor_pointer()
                                .on_click(cx.listener(move |w, _, _, cx| w.toggle_annotation_selected(&id_select, cx))),
                        )
                        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(Theme::text()).child(a.author.name.clone()))
                        .child(div().text_size(px(10.)).text_color(Theme::text_caption()).child(time_text(a.created)))
                        .child(div().flex_1())
                        .when(a.resolved, |d| d.child(div().text_size(px(10.)).text_color(Theme::text_caption()).child(crate::settings::tr("已解决", "Resolved")))),
                )
                .when(!a.quote.trim().is_empty(), |d| {
                    d.child(div().mt(px(6.)).pl(px(3.)).border_l_2().border_color(Theme::primary_500()).pl(px(8.)).text_size(px(11.)).text_color(Theme::text_caption()).child(format!("“{}”", a.quote)))
                })
                .child(div().mt(px(6.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(mention_text(&a.body)))
                .child(
                    div()
                        .mt(px(6.))
                        .flex()
                        .gap(px(10.))
                        .child(link_action(crate::settings::tr(if resolved { "重新打开" } else { "解决" }, if resolved { "Reopen" } else { "Resolve" }), ("ann-resolve", i), cx, move |w, cx| w.set_annotation_resolved(&id_resolve, !resolved, cx)))
                        .child(link_action(crate::settings::tr("编辑", "Edit"), ("ann-edit", i), cx, move |w, cx| w.begin_annotation_edit(&id_edit, cx)))
                        .child(link_action(crate::settings::tr("删除", "Delete"), ("ann-delete", i), cx, move |w, cx| w.delete_annotation(&id_delete, cx))),
                );
            list = list.child(row);
        }

        let composer = div()
            .p(px(12.))
            .border_b_1()
            .border_color(Theme::line_divider())
            .child(div().mb(px(6.)).text_size(px(10.)).text_color(Theme::text_caption()).child(format!("@{} · {}", self.annotation_author.name, crate::settings::tr("支持 @ 提及", "mention with @"))))
            .child(div().h(px(72.)).rounded(px(6.)).border_1().border_color(Theme::line_divider()).p(px(6.)).child(self.annotation_draft.clone()))
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1())
                    .when(editing.is_some(), |d| d.child(link_action(crate::settings::tr("取消", "Cancel"), ("ann-cancel", 0), cx, |w, cx| w.cancel_annotation_edit(cx))))
                    .child(
                        div()
                            .id("ann-submit")
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(5.))
                            .bg(Theme::primary_500())
                            .text_size(px(12.))
                            .text_color(Theme::bg_body())
                            .cursor_pointer()
                            .hover(|d| d.opacity(0.88))
                            .on_click(cx.listener(|w, _, _, cx| w.submit_annotation(cx)))
                            .child(crate::settings::text(if editing.is_some() { "保存" } else { "添加批注" })),
                    ),
            );

        let batch = (!selected.is_empty()).then(|| {
            div()
                .px(px(12.))
                .py(px(6.))
                .border_t_1()
                .border_color(Theme::line_divider())
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().text_size(px(10.)).text_color(Theme::text_caption()).child(format!("{} 条选中", selected.len())))
                .child(div().flex_1())
                .child(link_action(crate::settings::tr("解决选中", "Resolve selected"), ("ann-resolve-all", 0), cx, |w, cx| w.resolve_selected_annotations(true, cx)))
                .child(link_action(crate::settings::tr("删除选中", "Delete selected"), ("ann-delete-all", 0), cx, |w, cx| w.delete_selected_annotations(cx)))
        });

        Some(
            div()
                .id("annotations-panel")
                .absolute()
                .top(px(HEADER_H))
                .right(px(0.))
                .bottom(px(0.))
                .w(px(336.))
                .bg(Theme::bg_overlay())
                .border_l_1()
                .border_color(Theme::line_divider())
                .shadow_lg()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(46.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .border_b_1()
                        .border_color(Theme::line_divider())
                        .child(icon("message-square", 16., Theme::icon()))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(crate::settings::text("批注")))
                        .child(div().px(px(6.)).py(px(1.)).rounded(px(8.)).bg(Theme::fill_hover()).text_size(px(10.)).text_color(Theme::text_caption()).child(count.to_string()))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("ann-merge")
                                .p(px(5.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|d| d.bg(Theme::fill_hover()))
                                .on_click(cx.listener(|w, _, window, cx| w.merge_annotations(window, cx)))
                                .child(icon("batch", 14., Theme::icon())),
                        )
                        .child(
                            div()
                                .id("ann-close")
                                .p(px(5.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|d| d.bg(Theme::fill_hover()))
                                .on_click(cx.listener(|w, _, _, cx| w.close_annotations(cx)))
                                .child(icon("x", 14., Theme::icon())),
                        ),
                )
                .child(composer)
                .child(list)
                .children(batch)
                .into_any_element(),
        )
    }
}

fn block_path(blocks: &[Block], id: BlockId) -> Option<Vec<usize>> {
    for (i, b) in blocks.iter().enumerate() {
        if b.id == id { return Some(vec![i]); }
        if let Some(mut p) = block_path(&b.children, id) {
            p.insert(0, i);
            return Some(p);
        }
    }
    None
}

fn time_text(secs: u64) -> String {
    let (_, m, d, h, mi) = crate::time::civil(secs);
    format!("{:02}-{:02} {:02}:{:02}", m, d, h, mi)
}

fn link_action<F>(text: &'static str, id: (&'static str, usize), cx: &mut Context<Workspace>, f: F) -> Stateful<Div>
where
    F: Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
{
    div()
        .id(id)
        .text_size(px(11.))
        .text_color(Theme::primary_500())
        .cursor_pointer()
        .hover(|d| d.opacity(0.75))
        .child(text)
        .on_click(cx.listener(move |w, _, _, cx| f(w, cx)))
}
