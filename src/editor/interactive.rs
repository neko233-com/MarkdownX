//! Interactive blocks in the desktop editor.
//!
//! Canvas blocks carry a small JSON spec. The built-in quadratic runtime is
//! native so a book stays explorable without a browser; the same spec runs in
//! `web/js/widgets.js`. HTML blocks remain sandboxed in the Web reader.

use super::render::R;
use super::view::Editor;
use crate::editor_theme::Theme;
use crate::model::*;
use gpui::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const GRAPH_H: f32 = 240.;
const A_RANGE: (f64, f64) = (-3., 3.);
const B_RANGE: (f64, f64) = (-8., 8.);
const C_RANGE: (f64, f64) = (-8., 8.);

/// The quadratic runtime's state. `source` is the starting point; dragging a
/// slider changes this copy only, so exploring never dirties the document.
#[derive(Clone, Debug)]
pub(super) struct Quad {
    a: f64,
    b: f64,
    c: f64,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
}

impl Quad {
    fn parse(i: &Interactive) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(&i.source).ok()?;
        if v["kind"].as_str() != Some("quadratic") {
            return None;
        }
        let n = |key: &str, fallback: f64| v[key].as_f64().filter(|n| n.is_finite()).unwrap_or(fallback);
        let (x_min, x_max) = (n("xMin", -4.).clamp(-100., 100.), n("xMax", 4.).clamp(-100., 100.));
        let (y_min, y_max) = (n("yMin", -4.).clamp(-100., 100.), n("yMax", 8.).clamp(-100., 100.));
        // One grid line per unit; keep a hostile or mistaken spec from asking
        // the paint pass for millions of them.
        (x_min < x_max && y_min < y_max && x_max - x_min <= 200. && y_max - y_min <= 200.).then(|| Self {
            a: n("a", 1.).clamp(A_RANGE.0, A_RANGE.1),
            b: n("b", 0.).clamp(B_RANGE.0, B_RANGE.1),
            c: n("c", 0.).clamp(C_RANGE.0, C_RANGE.1),
            x_min,
            x_max,
            y_min,
            y_max,
        })
    }

    fn value(&self, i: usize) -> f64 {
        [self.a, self.b, self.c][i]
    }

    fn set(&mut self, i: usize, v: f64) {
        match i {
            0 => self.a = v,
            1 => self.b = v,
            _ => self.c = v,
        }
    }

    fn vertex(&self) -> (f64, f64) {
        let x = if self.a == 0. { 0. } else { -self.b / (2. * self.a) };
        (x, self.a * x * x + self.b * x + self.c)
    }

    fn disc(&self) -> f64 {
        self.b * self.b - 4. * self.a * self.c
    }
}

fn range(i: usize) -> (f64, f64) {
    [A_RANGE, B_RANGE, C_RANGE][i]
}

/// Per-window runtime state, keyed by block. A block id is stable while the
/// document is open; the values reset when it is reopened from disk.
#[derive(Clone, Default)]
pub(super) struct IxUi {
    quads: Rc<RefCell<HashMap<BlockId, Rc<RefCell<Quad>>>>>,
    tracks: Rc<RefCell<HashMap<(BlockId, usize), Bounds<Pixels>>>>,
    drag: Rc<RefCell<Option<(BlockId, usize)>>>,
}

impl IxUi {
    fn quad(&self, id: BlockId, i: &Interactive) -> Option<Rc<RefCell<Quad>>> {
        if let Some(q) = self.quads.borrow().get(&id) {
            return Some(q.clone());
        }
        let q = Rc::new(RefCell::new(Quad::parse(i)?));
        self.quads.borrow_mut().insert(id, q.clone());
        Some(q)
    }

    fn begin(&self, id: BlockId, i: usize) {
        *self.drag.borrow_mut() = Some((id, i));
    }

    fn end(&self) {
        *self.drag.borrow_mut() = None;
    }

    fn set_from_x(&self, id: BlockId, i: usize, x: f32) {
        let Some(b) = self.tracks.borrow().get(&(id, i)).copied() else { return };
        let w = f32::from(b.size.width).max(1.);
        let f = ((x - f32::from(b.left())) / w).clamp(0., 1.) as f64;
        let (lo, hi) = range(i);
        if let Some(q) = self.quads.borrow().get(&id) {
            q.borrow_mut().set(i, lo + (hi - lo) * f);
        }
    }
}

impl R<'_> {
    pub(super) fn interactive(&self, b: &Block, i: &Interactive, cx: &mut Context<Editor>) -> AnyElement {
        let id = b.id;
        let kind = match i.kind {
            InteractiveKind::Canvas => "Canvas",
            InteractiveKind::Html => "HTML",
        };
        let title = if i.title.trim().is_empty() { crate::settings::text("交互内容") } else { i.title.as_str() };
        let header = div()
            .h(px(38.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(Theme::line_divider())
            .child(div().size(px(7.)).rounded_full().bg(Theme::primary_500()))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(Theme::text()).child(title.to_string()))
            .child(
                div()
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(Theme::fill_hover())
                    .text_size(px(10.))
                    .text_color(Theme::text_caption())
                    .child(kind),
            );

        let body = if i.kind == InteractiveKind::Canvas && self.ed.ix.quad(id, i).is_some() {
            let q = self.ed.ix.quad(id, i).unwrap();
            let ui = self.ed.ix.clone();
            let q2 = q.clone();
            let graph = div()
                .h(px(GRAPH_H))
                .w_full()
                .relative()
                .overflow_hidden()
                .child(canvas(
                    |b, _, _| b,
                    move |b, _, window, _| paint_quad(&q2.borrow(), b, window),
                ).absolute().inset_0());
            let mut controls = div().px(px(14.)).pt(px(10.)).pb(px(12.)).flex().flex_col().gap(px(8.));
            let (vx, vy) = q.borrow().vertex();
            let d = q.borrow().disc();
            controls = controls.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(Theme::text_caption())
                    .child(format!("y = {:.2}x² + {:.2}x + {:.2}", q.borrow().a, q.borrow().b, q.borrow().c))
                    .child(format!("顶点 ({vx:.2}, {vy:.2})  ·  Δ = {d:.2}")),
            );
            for (idx, label) in ["二次项 a", "一次项 b", "常数项 c"].iter().enumerate() {
                controls = controls.child(self.ix_slider(id, idx, label, &q, &ui, cx));
            }
            div().flex().flex_col().child(graph).child(controls).into_any_element()
        } else {
            let note = match i.kind {
                InteractiveKind::Canvas => crate::settings::text("这个 Canvas 需要 Web 阅读器"),
                InteractiveKind::Html => crate::settings::text("沙箱 HTML · 在 MarkdownX 阅读器中运行"),
            };
            div()
                .h(px((i.height().min(520)) as f32))
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(crate::editor_ui::icon("circle-play", 28., Theme::text_caption()))
                .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(note))
                .into_any_element()
        };

        div()
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(Theme::line_divider())
            .bg(Theme::bg_overlay())
            .overflow_hidden()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn ix_slider(
        &self,
        id: BlockId,
        idx: usize,
        label: &str,
        q: &Rc<RefCell<Quad>>,
        ui: &IxUi,
        cx: &mut Context<Editor>,
    ) -> AnyElement {
        let (lo, hi) = range(idx);
        let v = q.borrow().value(idx);
        let frac = ((v - lo) / (hi - lo)) as f32;
        let tracks = ui.tracks.clone();
        let ui2 = ui.clone();
        let track = div()
            .relative()
            .h(px(20.))
            .flex_1()
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left_0()
                    .right_0()
                    .h(px(4.))
                    .rounded_full()
                    .bg(Theme::line_divider()),
            )
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left_0()
                    .w(relative(frac))
                    .h(px(4.))
                    .rounded_full()
                    .bg(Theme::primary_500()),
            )
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(relative(frac))
                    .ml(px(-6.))
                    .size(px(12.))
                    .rounded_full()
                    .bg(Theme::primary_500()),
            )
            .child(canvas(move |b, _, _| { tracks.borrow_mut().insert((id, idx), b); }, |_, _, _, _| {}).absolute().inset_0())
            .on_mouse_down(MouseButton::Left, cx.listener(move |e, ev: &MouseDownEvent, _, cx| {
                e.ix.begin(id, idx);
                e.ix.set_from_x(id, idx, f32::from(ev.position.x));
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |e, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) && *e.ix.drag.borrow() == Some((id, idx)) {
                    e.ix.set_from_x(id, idx, f32::from(ev.position.x));
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |e, _: &MouseUpEvent, _, cx| {
                if *e.ix.drag.borrow() == Some((id, idx)) {
                    ui2.end();
                    cx.notify();
                }
            }));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(54.)).text_size(px(11.)).text_color(Theme::text_caption()).child(label.to_string()))
            .child(track)
            .child(div().w(px(42.)).text_right().text_size(px(11.)).text_color(Theme::text_caption()).child(format!("{v:.1}")))
            .into_any_element()
    }
}

fn paint_quad(q: &Quad, b: Bounds<Pixels>, window: &mut Window) {
    let (x0, y0) = (f32::from(b.left()), f32::from(b.top()));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    window.paint_quad(fill(b, Theme::bg_body()));
    let px_x = |x: f64| x0 + ((x - q.x_min) / (q.x_max - q.x_min)) as f32 * w;
    let px_y = |y: f64| y0 + (1. - (y - q.y_min) / (q.y_max - q.y_min)) as f32 * h;
    let grid = Theme::line_divider().opacity(0.55);
    let mut x = q.x_min.ceil() as i64;
    while (x as f64) <= q.x_max {
        window.paint_quad(fill(Bounds::new(point(px(px_x(x as f64)), b.top()), size(px(1.), b.size.height)), grid));
        x += 1;
    }
    let mut y = q.y_min.ceil() as i64;
    while (y as f64) <= q.y_max {
        window.paint_quad(fill(Bounds::new(point(b.left(), px(px_y(y as f64))), size(b.size.width, px(1.))), grid));
        y += 1;
    }
    let axis = Theme::text_caption().opacity(0.55);
    if 0. >= q.x_min && 0. <= q.x_max {
        window.paint_quad(fill(Bounds::new(point(px(px_x(0.)), b.top()), size(px(1.), b.size.height)), axis));
    }
    if 0. >= q.y_min && 0. <= q.y_max {
        window.paint_quad(fill(Bounds::new(point(b.left(), px(px_y(0.))), size(b.size.width, px(1.))), axis));
    }
    let mut path = PathBuilder::stroke(px(2.));
    let mut started = false;
    for i in 0..=160 {
        let x = q.x_min + (q.x_max - q.x_min) * i as f64 / 160.;
        let y = q.a * x * x + q.b * x + q.c;
        if y < q.y_min || y > q.y_max {
            started = false;
            continue;
        }
        let p = point(px(px_x(x)), px(px_y(y)));
        if started {
            path.line_to(p);
        } else {
            path.move_to(p);
            started = true;
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, Theme::primary_500());
    }
    let (vx, vy) = q.vertex();
    if vy >= q.y_min && vy <= q.y_max {
        window.paint_quad(
            fill(Bounds::new(point(px(px_x(vx) - 3.), px(px_y(vy) - 3.)), size(px(6.), px(6.))), Theme::primary_500())
                .corner_radii(px(3.)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Quad;
    use crate::model::Interactive;

    fn canvas(source: &str) -> Interactive {
        Interactive::canvas("", source.to_string())
    }

    #[test]
    fn parses_quadratic_spec_and_clamps_controls() {
        let q = Quad::parse(&canvas(r#"{"kind":"quadratic","a":9,"b":-99,"c":2}"#)).unwrap();
        assert_eq!((q.a, q.b, q.c), (3., -8., 2.));
        assert_eq!((q.x_min, q.x_max, q.y_min, q.y_max), (-4., 4., -4., 8.));
    }

    #[test]
    fn rejects_wrong_or_oversized_ranges() {
        assert!(Quad::parse(&canvas(r#"{"kind":"projectile"}"#)).is_none());
        let q = Quad::parse(&canvas(r#"{"kind":"quadratic","xMin":-400,"xMax":400}"#)).unwrap();
        assert_eq!((q.x_min, q.x_max), (-100., 100.));
        assert!(Quad::parse(&canvas(r#"{"kind":"quadratic","yMin":10,"yMax":-10}"#)).is_none());
    }
}
