//! Floating windows with a classic caption bar, kept inside the diagram area.

use crate::theme::Tokens;
use egui::{Align2, Context, FontId, Id, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

const CANVAS_KEY: &str = "stormwaves-canvas-rect";

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
}

impl<'a> Win<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        Win { title: title.into(), id: None, open: None, size: None, width: None, min: None, resizable: false, centered: false }
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
        let frame = egui::Frame::window(&ctx.global_style()).fill(t.face).corner_radius(t.radius).inner_margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 6 }).stroke(Stroke::new(1.0, t.dark));
        let mut w = egui::Window::new(self.title.clone()).id(id).title_bar(false).frame(frame).resizable(self.resizable).constrain_to(area.expand(2.0));
        w = if self.centered { w.anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)) } else { w.default_pos(default_pos) };
        if let Some(s) = self.size {
            w = w.default_size(s);
        }
        if let Some(wd) = self.width {
            w = w.default_width(wd);
        }
        if let Some(m) = self.min {
            w = w.min_size(m);
        }
        let closable = self.open.is_some();
        let title = self.title.clone();
        let mut close = false;
        let r = w.show(ctx, |ui| {
            close = caption(ui, &title, closable);
            ui.add_space(4.0);
            add(ui)
        });
        if close {
            if let Some(o) = self.open {
                *o = false;
            }
        }
        r.and_then(|r| r.inner)
    }
}

/// Draws the caption bar; returns true when the close box was clicked.
fn caption(ui: &mut Ui, title: &str, closable: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(160.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    let p = ui.painter();
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
    if !closable {
        return false;
    }
    let b = Rect::from_center_size(pos2(rect.max.x - 11.0, rect.center().y), vec2(16.0, 14.0));
    let resp = ui.interact(b, ui.id().with("close"), Sense::click());
    let p = ui.painter();
    p.rect_filled(b, 0.0, if !t.bevel && resp.hovered() { egui::Color32::from_rgb(220, 60, 60) } else { t.face });
    if t.bevel {
        crate::widgets::bevel(p, b, !resp.is_pointer_button_down_on(), &t);
    }
    let c = b.center();
    let s = Stroke::new(1.6, t.text);
    p.line_segment([c + vec2(-3.5, -3.0), c + vec2(3.5, 3.0)], s);
    p.line_segment([c + vec2(3.5, -3.0), c + vec2(-3.5, 3.0)], s);
    resp.clicked()
}
