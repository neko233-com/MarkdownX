//! Interactive blocks in the desktop editor.
//!
//! Canvas blocks carry a small JSON spec. Built-in quadratic, unit-circle and
//! projectile runtimes are native; HTML blocks run in the Web reader.

use super::render::R;
use super::view::Editor;
use crate::editor_theme::Theme;
use crate::model::*;
use gpui::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use std::time::Instant;

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

#[derive(Clone, Debug)]
enum Experiment {
    UnitCircle { angle: f64 },
    Projectile { angle: f64, speed: f64, started: Option<Instant> },
}

impl Experiment {
    fn parse(i: &Interactive) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(&i.source).ok()?;
        let number = |name: &str, default: f64| value[name].as_f64().filter(|v| v.is_finite()).unwrap_or(default);
        match value["kind"].as_str()? {
            "unitcircle" | "unit-circle" => Some(Self::UnitCircle { angle: number("angle", 35.).clamp(0., 360.) }),
            "projectile" => Some(Self::Projectile { angle: number("angle", 45.).clamp(10., 80.), speed: number("speed", 14.).clamp(4., 24.), started: None }),
            _ => None,
        }
    }

    fn range(&self, index: usize) -> (f64, f64) {
        match self { Self::UnitCircle { .. } => (0., 360.), Self::Projectile { .. } if index == 0 => (10., 80.), Self::Projectile { .. } => (4., 24.) }
    }

    #[cfg(test)]
    fn value(&self, index: usize) -> f64 {
        match self { Self::UnitCircle { angle } => *angle, Self::Projectile { angle, .. } if index == 0 => *angle, Self::Projectile { speed, .. } => *speed }
    }

    fn set(&mut self, index: usize, value: f64) {
        match self {
            Self::UnitCircle { angle } => *angle = value.round().clamp(0., 360.),
            Self::Projectile { angle, speed, started } => {
                if index == 0 { *angle = value.round().clamp(10., 80.); } else { *speed = (value * 2.).round().clamp(8., 48.) / 2.; }
                *started = None;
            }
        }
    }

    fn flight(&self) -> Option<(f64, f64, f64)> {
        let Self::Projectile { angle, speed, .. } = self else { return None };
        let radians = angle.to_radians();
        Some((2. * speed * radians.sin() / 9.8, speed * speed * (2. * radians).sin() / 9.8, speed * speed * radians.sin().powi(2) / 19.6))
    }
}

/// Per-window runtime state, keyed by block. A block id is stable while the
/// document is open; the values reset when it is reopened from disk.
#[derive(Clone, Default)]
pub(super) struct IxUi {
    quads: Rc<RefCell<HashMap<BlockId, (String, Rc<RefCell<Quad>>)>>>,
    experiments: Rc<RefCell<HashMap<BlockId, (String, Rc<RefCell<Experiment>>)>>>,
    tracks: Rc<RefCell<HashMap<(BlockId, usize), Bounds<Pixels>>>>,
    drag: Rc<RefCell<Option<(BlockId, usize)>>>,
}

impl IxUi {
    fn quad(&self, id: BlockId, i: &Interactive) -> Option<Rc<RefCell<Quad>>> {
        if let Some((source, q)) = self.quads.borrow().get(&id)
            && source == &i.source {
            return Some(q.clone());
        }
        self.quads.borrow_mut().remove(&id);
        let q = Rc::new(RefCell::new(Quad::parse(i)?));
        self.quads.borrow_mut().insert(id, (i.source.clone(), q.clone()));
        Some(q)
    }

    fn experiment(&self, id: BlockId, i: &Interactive) -> Option<Rc<RefCell<Experiment>>> {
        if let Some((source, state)) = self.experiments.borrow().get(&id)
            && source == &i.source { return Some(state.clone()); }
        self.experiments.borrow_mut().remove(&id);
        let state = Rc::new(RefCell::new(Experiment::parse(i)?));
        self.experiments.borrow_mut().insert(id, (i.source.clone(), state.clone()));
        Some(state)
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
        if let Some((_, q)) = self.quads.borrow().get(&id) {
            let (lo, hi) = range(i);
            q.borrow_mut().set(i, lo + (hi - lo) * f);
        } else if let Some((_, state)) = self.experiments.borrow().get(&id) {
            let (lo, hi) = state.borrow().range(i);
            state.borrow_mut().set(i, lo + (hi - lo) * f);
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
                    .child(format!("{} ({vx:.2}, {vy:.2})  ·  Δ = {d:.2}", crate::settings::tr("顶点", "Vertex"))),
            );
            for (idx, label) in [crate::settings::tr("二次项 a", "Quadratic a"), crate::settings::tr("一次项 b", "Linear b"), crate::settings::tr("常数项 c", "Constant c")].iter().enumerate() {
                let value = q.borrow().value(idx);
                controls = controls.child(self.ix_slider(id, idx, label, value, range(idx), format!("{value:.1}"), &ui, cx));
            }
            div().flex().flex_col().child(graph).child(controls).into_any_element()
        } else if i.kind == InteractiveKind::Canvas && self.ed.ix.experiment(id, i).is_some() {
            let state = self.ed.ix.experiment(id, i).unwrap();
            let snapshot = state.borrow().clone();
            let paint = state.clone();
            let graph = div().h(px(GRAPH_H)).w_full().relative().overflow_hidden().child(canvas(
                |b, _, _| b,
                move |b, _, window, _| paint_experiment(&paint.borrow(), b, window),
            ).absolute().inset_0());
            let ui = self.ed.ix.clone();
            let mut controls = div().px(px(14.)).pt(px(10.)).pb(px(12.)).flex().flex_col().gap(px(8.));
            match snapshot {
                Experiment::UnitCircle { angle } => {
                    let rad = angle.to_radians();
                    controls = controls.child(div().text_size(px(11.)).text_color(Theme::text_caption())
                        .child(format!("θ = {angle:.0}° = {rad:.3} rad   ·   sin θ = {:.3}   ·   cos θ = {:.3}", rad.sin(), rad.cos())));
                    controls = controls.child(self.ix_slider(id, 0, crate::settings::tr("角度 θ", "Angle θ"), angle, (0., 360.), format!("{angle:.0}°"), &ui, cx));
                }
                Experiment::Projectile { angle, speed, .. } => {
                    let (flight, distance, height) = snapshot.flight().unwrap();
                    controls = controls.child(div().text_size(px(11.)).text_color(Theme::text_caption()).child(format!(
                        "{} {distance:.1} m   ·   {} {height:.1} m   ·   {} {flight:.2} s",
                        crate::settings::tr("射程", "Range"), crate::settings::tr("最大高度", "Apex"), crate::settings::tr("飞行时间", "Flight"))));
                    controls = controls.child(self.ix_slider(id, 0, crate::settings::tr("发射角", "Angle"), angle, (10., 80.), format!("{angle:.0}°"), &ui, cx));
                    controls = controls.child(self.ix_slider(id, 1, crate::settings::tr("初速度", "Speed"), speed, (4., 24.), format!("{speed:.1} m/s"), &ui, cx));
                    let launch = state.clone();
                    controls = controls.child(div().id(("projectile-launch", id.0)).cursor_pointer().w(px(90.)).text_center().px(px(12.)).py(px(5.))
                        .rounded(px(5.)).bg(Theme::primary_500()).text_color(gpui::white()).text_size(px(12.))
                        .child(crate::settings::tr("发射", "Launch"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if let Experiment::Projectile { started, .. } = &mut *launch.borrow_mut() { *started = Some(Instant::now()); }
                            cx.notify();
                        })));
                }
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
        v: f64,
        limits: (f64, f64),
        display: String,
        ui: &IxUi,
        cx: &mut Context<Editor>,
    ) -> AnyElement {
        let (lo, hi) = limits;
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
            .child(div().w(px(78.)).text_size(px(11.)).text_color(Theme::text_caption()).child(label.to_string()))
            .child(track)
            .child(div().w(px(68.)).text_right().text_size(px(11.)).text_color(Theme::text_caption()).child(display))
            .into_any_element()
    }
}

fn paint_experiment(state: &Experiment, b: Bounds<Pixels>, window: &mut Window) {
    window.paint_quad(fill(b, Theme::bg_body()));
    let x0 = f32::from(b.left());
    let y0 = f32::from(b.top());
    let w = f32::from(b.size.width).max(1.);
    let h = f32::from(b.size.height).max(1.);
    let mut axes = PathBuilder::stroke(px(1.));
    let mut curve = PathBuilder::stroke(px(2.));
    let (dot_x, dot_y) = match state {
        Experiment::UnitCircle { angle } => {
            let (cx, cy) = (x0 + w / 2., y0 + h / 2.);
            let radius = w.min(h) * 0.37;
            axes.move_to(point(px(cx - radius * 1.15), px(cy)));
            axes.line_to(point(px(cx + radius * 1.15), px(cy)));
            axes.move_to(point(px(cx), px(cy - radius * 1.15)));
            axes.line_to(point(px(cx), px(cy + radius * 1.15)));
            for n in 0..=128 {
                let a = std::f32::consts::TAU * n as f32 / 128.;
                let point = point(px(cx + radius * a.cos()), px(cy - radius * a.sin()));
                if n == 0 { curve.move_to(point); } else { curve.line_to(point); }
            }
            let radians = angle.to_radians();
            let dot = (cx + radius * radians.cos() as f32, cy - radius * radians.sin() as f32);
            curve.move_to(point(px(cx), px(cy)));
            curve.line_to(point(px(dot.0), px(dot.1)));
            axes.move_to(point(px(dot.0), px(dot.1)));
            axes.line_to(point(px(dot.0), px(cy)));
            axes.move_to(point(px(dot.0), px(dot.1)));
            axes.line_to(point(px(cx), px(dot.1)));
            dot
        }
        Experiment::Projectile { angle, speed, started } => {
            let (duration, distance, height) = state.flight().unwrap();
            let xmax = (distance.max(4.) * 1.12).max(5.);
            let ymax = (height.max(1.) * 1.25).max(2.);
            let pad = 24f32.min(w / 8.).min(h / 8.);
            let sx = |x: f64| x0 + pad + (x / xmax) as f32 * (w - 2. * pad);
            let sy = |y: f64| y0 + h - pad - (y / ymax) as f32 * (h - 2. * pad);
            axes.move_to(point(px(sx(0.)), px(sy(ymax))));
            axes.line_to(point(px(sx(0.)), px(sy(0.))));
            axes.line_to(point(px(sx(xmax)), px(sy(0.))));
            let radians = angle.to_radians();
            let position = |t: f64| (speed * radians.cos() * t, (speed * radians.sin() * t - 4.9 * t * t).max(0.));
            for n in 0..=120 {
                let (x, y) = position(duration * n as f64 / 120.);
                let point = point(px(sx(x)), px(sy(y)));
                if n == 0 { curve.move_to(point); } else { curve.line_to(point); }
            }
            let elapsed = started.map(|start| start.elapsed().as_secs_f64()).unwrap_or(0.).min(duration);
            if started.is_some() && elapsed < duration { window.request_animation_frame(); }
            let (x, y) = position(elapsed);
            (sx(x), sy(y))
        }
    };
    if let Ok(path) = axes.build() { window.paint_path(path, Theme::line_divider()); }
    if let Ok(path) = curve.build() { window.paint_path(path, Theme::primary_500()); }
    window.paint_quad(fill(Bounds::new(point(px(dot_x - 4.), px(dot_y - 4.)), size(px(8.), px(8.))), Theme::primary_500()).corner_radii(px(4.)));
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

#[cfg(feature = "devtools")]
impl Editor {
    pub(crate) fn check_native_experiment(&self, kind: &str) {
        let block = self.core.doc.blocks.iter().find(|b| matches!(&b.kind, BlockKind::Interactive(i) if i.source.contains(kind))).expect("experiment block");
        let states = self.ix.experiments.borrow();
        let (_, state) = states.get(&block.id).expect("experiment was rendered");
        let before = match &*state.borrow() { Experiment::UnitCircle { angle } | Experiment::Projectile { angle, .. } => *angle };
        let bounds = *self.ix.tracks.borrow().get(&(block.id, 0)).expect("experiment slider was laid out");
        drop(states);
        let x = f32::from(bounds.left()) + f32::from(bounds.size.width) * 0.75;
        self.ix.set_from_x(block.id, 0, x);
        let states = self.ix.experiments.borrow();
        let after = match &*states.get(&block.id).unwrap().1.borrow() { Experiment::UnitCircle { angle } | Experiment::Projectile { angle, .. } => *angle };
        assert_ne!(before, after, "slider must update the experiment");
    }
}

#[cfg(test)]
mod tests {
    use super::{Experiment, IxUi, Quad};
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

    #[test]
    fn experiments_support_circle_and_projectile_controls() {
        let mut circle = Experiment::parse(&canvas(r#"{"kind":"unitcircle","angle":45}"#)).unwrap();
        assert_eq!(circle.value(0), 45.);
        circle.set(0, 400.);
        assert_eq!(circle.value(0), 360.);
        let mut projectile = Experiment::parse(&canvas(r#"{"kind":"projectile","angle":45,"speed":14}"#)).unwrap();
        let (time, range, height) = projectile.flight().unwrap();
        assert!((time - 2.02).abs() < 0.02);
        assert!((range - 20.).abs() < 0.01);
        assert!((height - 5.).abs() < 0.01);
        projectile.set(1, 7.3);
        assert_eq!(projectile.value(1), 7.5);
        assert!(Experiment::parse(&canvas(r#"{"kind":"unknown"}"#)).is_none());
    }

    #[test]
    fn changing_widget_source_discards_stale_runtime() {
        let ui = IxUi::default();
        let id = crate::model::BlockId(1);
        assert!(ui.quad(id, &canvas(r#"{"kind":"quadratic"}"#)).is_some());
        let projectile = canvas(r#"{"kind":"projectile"}"#);
        assert!(ui.quad(id, &projectile).is_none());
        assert!(ui.experiment(id, &projectile).is_some());
        let circle = canvas(r#"{"kind":"unit-circle"}"#);
        assert!(matches!(*ui.experiment(id, &circle).unwrap().borrow(), Experiment::UnitCircle { .. }));
    }
}
