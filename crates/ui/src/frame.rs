//! Floating windows with a classic caption bar, kept inside the diagram area.

use crate::theme::Tokens;
use egui::{Align2, Context, FontId, Id, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

const CANVAS_KEY: &str = "wavestorm-canvas-rect";

/// Records the diagram rectangle that windows are kept inside.
pub fn set_canvas(ctx: &Context, rect: Rect) {
    ctx.data_mut(|d| d.insert_temp(Id::new(CANVAS_KEY), rect));
}

fn canvas(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(Id::new(CANVAS_KEY)))
}

/// Builder mirroring the parts of `egui::Window` the app uses.
pub struct Win<'a> {
    title: String,
    id: Option<Id>,
    open: Option<&'a mut bool>,
    size: Option<Vec2>,
    width: Option<f32>,
    min: Option<Vec2>,
    resizable: bool,
    centered: bool,
    min_max: bool,
}

/// Minimize/maximize state of a window with [`Win::min_max`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Normal,
    Minimized,
    /// Filling the diagram area; `prev` is the rectangle to restore.
    Maximized { prev: Rect },
    /// Restoring to `prev` this frame.
    Restore { prev: Rect },
}

impl<'a> Win<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        Win { title: title.into(), id: None, open: None, size: None, width: None, min: None, resizable: false, centered: false, min_max: false }
    }
    /// Report-style window: minimize and maximize boxes beside the close box.
    pub fn min_max(mut self) -> Self {
        self.min_max = true;
        self
    }
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }
    pub fn default_size(mut self, s: Vec2) -> Self {
        self.size = Some(s);
        self
    }
    pub fn default_width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }
    pub fn min_size(mut self, s: Vec2) -> Self {
        self.min = Some(s);
        self
    }
    pub fn resizable(mut self, r: bool) -> Self {
        self.resizable = r;
        self
    }
    pub fn collapsible(self, _c: bool) -> Self {
        self
    }
    pub fn anchor(mut self, _a: Align2, _o: Vec2) -> Self {
        self.centered = true;
        self
    }

    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        if self.open.as_deref() == Some(&false) {
            return None;
        }
        let t = Tokens::get(ctx);
        let id = self.id.unwrap_or_else(|| Id::new(("win", self.title.clone())));
        let area = canvas(ctx).unwrap_or_else(|| ctx.content_rect());
        // Stagger new windows so they don't open exactly on top of each other.
        let k = (id.value() % 7) as f32;
        let default_pos = area.min + vec2(30.0 + 26.0 * k, 20.0 + 22.0 * k);
        let frame = if t.glass {
            egui::Frame::window(&ctx.global_style()).fill(t.face).corner_radius(14).inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 8 }).stroke(Stroke::NONE)
        } else {
            egui::Frame::window(&ctx.global_style()).fill(t.face).corner_radius(t.radius).inner_margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 6 }).stroke(Stroke::new(1.0, t.dark))
        };
        let shape_id = id.with("shape");
        let shape = if self.min_max { ctx.data(|d| d.get_temp::<Shape>(shape_id)).unwrap_or(Shape::Normal) } else { Shape::Normal };
        let minimized = shape == Shape::Minimized;
        let mut w = egui::Window::new(self.title.clone()).id(id).title_bar(false).frame(frame).resizable(self.resizable && !minimized).constrain_to(area.expand(2.0));
        w = if self.centered { w.anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)) } else { w.default_pos(default_pos) };
        if let Some(s) = self.size {
            w = w.default_size(s);
        }
        if let Some(wd) = self.width {
            w = w.default_width(wd);
        }
        if let Some(m) = self.min.filter(|_| !minimized) {
            w = w.min_size(m);
        }
        match shape {
            Shape::Maximized { .. } => w = w.fixed_rect(area),
            Shape::Restore { prev } => w = w.fixed_rect(prev),
            Shape::Minimized => w = w.fixed_size(vec2(220.0, 20.0)),
            Shape::Normal => {}
        }
        let closable = self.open.is_some();
        let title = self.title.clone();
        let mut buttons = CaptionButtons::default();
        let r = w.show(ctx, |ui| {
            buttons = caption(ui, &title, closable, self.min_max.then_some(shape));
            if minimized {
                return None;
            }
            ui.add_space(4.0);
            Some(add(ui))
        });
        if t.glass
            && let Some(r) = &r
        {
            // Rim and sheen over the finished window (its own layer, so it stays on top).
            let p = ctx.layer_painter(r.response.layer_id);
            let rect = r.response.rect;
            let sheen = Rect::from_min_size(rect.min, vec2(rect.width(), 40.0_f32.min(rect.height())));
            let pts: Vec<egui::Pos2> = crate::glass::rounded(rect, 14.0).into_iter().map(|q| pos2(q.x, q.y.min(sheen.max.y))).collect();
            crate::glass::gradient_fill(&p, &pts, crate::theme::white(0.05), crate::theme::white(0.0));
            p.add(egui::Shape::Path(egui::epaint::PathShape::closed_line(crate::glass::rounded(rect, 14.0), crate::glass::iris_stroke(1.0, 0.6))));
        }
        if self.min_max {
            let rect = r.as_ref().map(|r| r.response.rect);
            let next = match (shape, buttons.minimize, buttons.maximize) {
                (Shape::Minimized, true, _) => Shape::Normal,
                (_, true, _) => Shape::Minimized,
                (Shape::Maximized { prev }, _, true) => Shape::Restore { prev },
                (_, _, true) => Shape::Maximized { prev: rect.unwrap_or(area) },
                // A restore holds its rectangle for one frame, then the window is free again.
                (Shape::Restore { .. }, _, _) => Shape::Normal,
                (s, _, _) => s,
            };
            if next != shape {
                ctx.data_mut(|d| d.insert_temp(shape_id, next));
                ctx.request_repaint();
            }
        }
        if buttons.close
            && let Some(o) = self.open
        {
            *o = false;
        }
        r.and_then(|r| r.inner).flatten()
    }
}

#[derive(Default)]
struct CaptionButtons {
    close: bool,
    minimize: bool,
    maximize: bool,
}

/// A caption box (close, minimize or maximize) at `center`; returns true when clicked.
fn caption_box(ui: &mut Ui, center: egui::Pos2, key: &str, glyph: impl FnOnce(&egui::Painter, egui::Pos2, Stroke)) -> bool {
    let t = Tokens::get(ui.ctx());
    let b = Rect::from_center_size(center, vec2(16.0, 14.0));
    let resp = ui.interact(b, ui.id().with(key), Sense::click());
    let p = ui.painter();
    if t.glass {
        // Round, borderless boxes that light up on hover (close turns rose).
        if resp.hovered() {
            let c = if key == "close" { crate::glass::alpha(crate::glass::IRIS[3], 0.85) } else { crate::theme::white(0.14) };
            p.circle_filled(b.center(), 9.0, c);
        }
        glyph(p, b.center(), Stroke::new(1.4, if resp.hovered() { t.text } else { t.text_dim }));
        return resp.clicked();
    }
    let hot = !t.bevel && resp.hovered();
    p.rect_filled(b, 0.0, if hot && key == "close" { egui::Color32::from_rgb(220, 60, 60) } else if hot { t.light } else { t.face });
    if t.bevel {
        crate::widgets::bevel(p, b, !resp.is_pointer_button_down_on(), &t);
    }
    glyph(p, b.center(), Stroke::new(1.6, t.text));
    resp.clicked()
}

/// Draws the caption bar and its boxes (`shape` is set for report windows with minimize and
/// maximize boxes).
fn caption(ui: &mut Ui, title: &str, closable: bool, shape: Option<Shape>) -> CaptionButtons {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(160.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, if t.glass { 24.0 } else { 20.0 }), Sense::hover());
    let p = ui.painter();
    if t.glass {
        // A glowing iridescent bead instead of the water drop, and quiet title text.
        let c = rect.left_center() + vec2(8.0, 0.0);
        crate::glass::glow(p, c, 9.0, crate::glass::alpha(crate::glass::IRIS[2], 0.5));
        p.circle_filled(c, 3.5, crate::glass::IRIS[0]);
        p.circle_stroke(c, 3.5, Stroke::new(1.0, crate::theme::white(0.6)));
        p.with_clip_rect(rect.shrink2(vec2(22.0, 0.0))).text(pos2(rect.min.x + 22.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.5), t.text);
        crate::glass::divider(p, rect.left_bottom() + vec2(0.0, 2.0), rect.right_bottom() + vec2(0.0, 2.0));
    } else {
        if t.bevel {
            crate::widgets::caption_bar(p, rect, "", &t);
        } else {
            p.rect_filled(rect, t.radius as f32, t.face);
            p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.shadow));
        }
        // A small water-drop emblem.
        let c = rect.left_center() + vec2(10.0, 0.0);
        p.circle_filled(c + vec2(0.0, 2.0), 4.5, egui::Color32::from_rgb(90, 170, 240));
        p.add(egui::Shape::convex_polygon(vec![c + vec2(0.0, -6.0), c + vec2(4.2, 0.5), c + vec2(-4.2, 0.5)], egui::Color32::from_rgb(90, 170, 240), Stroke::NONE));
        let text_color = if t.bevel { t.caption_text } else { t.text };
        p.with_clip_rect(rect.shrink2(vec2(22.0, 0.0))).text(pos2(rect.min.x + 22.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.0), text_color);
    }
    let mut out = CaptionButtons::default();
    if !closable {
        return out;
    }
    let y = rect.center().y;
    out.close = caption_box(ui, pos2(rect.max.x - 11.0, y), "close", |p, c, s| {
        p.line_segment([c + vec2(-3.5, -3.0), c + vec2(3.5, 3.0)], s);
        p.line_segment([c + vec2(3.5, -3.0), c + vec2(-3.5, 3.0)], s);
    });
    if let Some(shape) = shape {
        let maximized = matches!(shape, Shape::Maximized { .. });
        out.maximize = caption_box(ui, pos2(rect.max.x - 31.0, y), "max", |p, c, s| {
            let thin = Stroke::new(1.0, s.color);
            if maximized {
                // Restore glyph: two overlapping windows.
                let back = Rect::from_min_size(c + vec2(-2.0, -5.0), vec2(7.0, 6.0));
                let front = Rect::from_min_size(c + vec2(-5.0, -2.0), vec2(7.0, 6.0));
                p.rect_stroke(back, 0.0, thin, egui::StrokeKind::Inside);
                p.rect_filled(front, 0.0, Tokens::get(p.ctx()).face);
                p.rect_stroke(front, 0.0, thin, egui::StrokeKind::Inside);
                p.line_segment([front.left_top() + vec2(0.0, 1.0), front.right_top() + vec2(0.0, 1.0)], thin);
            } else {
                let r = Rect::from_center_size(c, vec2(9.0, 8.0));
                p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Inside);
                p.line_segment([r.left_top() + vec2(0.0, 1.0), r.right_top() + vec2(0.0, 1.0)], thin);
            }
        });
        out.minimize = caption_box(ui, pos2(rect.max.x - 47.0, y), "min", |p, c, s| {
            p.line_segment([c + vec2(-3.5, 3.0), c + vec2(3.0, 3.0)], s);
        });
    }
    out
}
