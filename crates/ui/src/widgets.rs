//! Shared widgets in the application's dialog style: bevelled buttons, labelled numeric fields
//! with spinners, group boxes, tab strips and editable grids. Colours come from [`Tokens`].

use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

/// Draws a Windows-classic 2-pixel bevel (or a flat border in modern themes).
pub fn bevel(p: &egui::Painter, rect: Rect, raised: bool, t: &Tokens) {
    if !t.bevel {
        p.rect_stroke(rect.shrink(0.5), t.radius as f32, Stroke::new(1.0, t.shadow), StrokeKind::Inside);
        return;
    }
    let (o1, o2, i1, i2) = if raised { (t.light, t.dark, t.face, t.shadow) } else { (t.shadow, t.light, t.dark, t.face) };
    let r = rect.shrink(0.5);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, o1));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, o1));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, o2));
    p.line_segment([r.right_bottom(), r.left_bottom() - vec2(1.0, 0.0)], Stroke::new(1.0, o2));
    let r = r.shrink(1.0);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, i1));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, i1));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, i2));
    p.line_segment([r.right_bottom(), r.left_bottom() - vec2(1.0, 0.0)], Stroke::new(1.0, i2));
}

/// A sunken field background (text boxes, lists, the diagram).
pub fn sunken(p: &egui::Painter, rect: Rect, fill: Color32, t: &Tokens) {
    p.rect_filled(rect, t.radius as f32, fill);
    bevel(p, rect, false, t);
}

/// A push button with a classic bevel.
pub fn button(ui: &mut Ui, label: &str, width: f32) -> Response {
    button_enabled(ui, label, width, true)
}

pub fn button_enabled(ui: &mut Ui, label: &str, width: f32, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(13.0);
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x);
    let size = vec2(width.max(text_w + 16.0), 23.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let pressed = resp.is_pointer_button_down_on();
        let fill = if !t.bevel && resp.hovered() && enabled { t.light } else { t.face };
        p.rect_filled(rect, t.radius as f32, fill);
        bevel(p, rect, !pressed, &t);
        let off = if pressed && t.bevel { vec2(1.0, 1.0) } else { Vec2::ZERO };
        let color = if enabled { t.text } else { t.text_disabled };
        if !enabled && t.bevel {
            p.text(rect.center() + off + vec2(1.0, 1.0), Align2::CENTER_CENTER, label, font.clone(), t.light);
        }
        p.text(rect.center() + off, Align2::CENTER_CENTER, label, font, color);
        if resp.has_focus() {
            p.rect_stroke(rect.shrink(4.0), 0.0, Stroke::new(1.0, t.dark), StrokeKind::Inside);
        }
    }
    resp
}

/// A small square icon button for toolbars; `draw` paints the icon inside `rect`.
pub fn tool_button(ui: &mut Ui, tip: &str, selected: bool, draw: impl FnOnce(&egui::Painter, Rect, &Tokens)) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected || resp.is_pointer_button_down_on() {
            p.rect_filled(rect, t.radius as f32, if t.bevel { t.light.gamma_multiply(0.6) } else { t.shadow });
            bevel(p, rect, false, &t);
        } else if resp.hovered() {
            bevel(p, rect, true, &t);
        }
        draw(p, rect.shrink(4.0), &t);
    }
    if resp.hovered() {
        set_status_hint(ui.ctx(), tip);
    }
    resp.on_hover_text(tip)
}

fn status_hint_id() -> egui::Id {
    egui::Id::new("status-hint")
}

/// Describes the control under the pointer in the status bar (for this frame). Panels drawn
/// after the status bar show their hint one frame later, so a changed hint asks for a repaint.
pub fn set_status_hint(ctx: &egui::Context, text: &str) {
    let last_id = egui::Id::new("status-hint-last");
    let changed = ctx.data_mut(|d| {
        d.insert_temp(status_hint_id(), text.to_string());
        let changed = d.get_temp::<String>(last_id).as_deref() != Some(text);
        d.insert_temp(last_id, text.to_string());
        changed
    });
    if changed {
        ctx.request_repaint();
    }
}

/// The hint set this frame, if any; clears it.
pub fn take_status_hint(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| {
        let hint = d.get_temp::<String>(status_hint_id());
        d.remove::<String>(status_hint_id());
        hint
    })
}

/// The caption row above a field: "Inlet Invert:" left and "(feet)" right.
pub fn caption(ui: &mut Ui, label: &str, unit: &str, width: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 15.0), Sense::hover());
    let p = ui.painter();
    let font = FontId::proportional(12.5);
    let unit_text = if unit.is_empty() { String::new() } else { format!("({unit})") };
    let (lw, uw) = ui.fonts_mut(|f| {
        (f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x, f.layout_no_wrap(unit_text.clone(), font.clone(), t.text).size().x)
    });
    if lw + uw + 6.0 <= width || unit.is_empty() {
        p.with_clip_rect(rect).text(rect.left_center(), Align2::LEFT_CENTER, label, font.clone(), t.text);
        if !unit.is_empty() {
            p.text(rect.right_center(), Align2::RIGHT_CENTER, unit_text, font, t.text);
        }
    } else {
        // Not enough room for both: one run of text, clipped to the field width.
        p.with_clip_rect(rect).text(rect.left_center(), Align2::LEFT_CENTER, format!("{label} {unit_text}"), font, t.text);
    }
}

/// What happened in a [`list_table`] this frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct ListEvents {
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
}

/// A read-only grid: sunken white box, bevelled header cells, a row-number column and
/// navy row selection (the classic data-grid look).
pub fn list_table(ui: &mut Ui, id: &str, cols: &[(&str, f32, bool)], rows: &[Vec<String>], selected: &mut Option<usize>, height: f32) -> ListEvents {
    let t = Tokens::get(ui.ctx());
    let mut ev = ListEvents::default();
    let row_h = 18.0;
    let num_w = 24.0;
    let width = (num_w + cols.iter().map(|c| c.1).sum::<f32>() + 18.0).min(ui.available_width()).max(120.0);
    let (outer, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    sunken(ui.painter(), outer, t.field, &t);
    let inner = outer.shrink(2.0);
    let font = FontId::proportional(12.5);
    let p = ui.painter().with_clip_rect(inner);
    let mut x = inner.min.x;
    for (title, w) in std::iter::once(("#", num_w)).chain(cols.iter().map(|c| (c.0, c.1))) {
        let r = Rect::from_min_size(pos2(x, inner.min.y), vec2(w, row_h + 1.0));
        p.rect_filled(r, 0.0, t.face);
        bevel(&p, r, true, &t);
        p.with_clip_rect(r.shrink(1.0).intersect(inner)).text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, title, font.clone(), t.text);
        x += w;
    }
    let body = Rect::from_min_max(pos2(inner.min.x, inner.min.y + row_h + 1.0), inner.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(id));
    egui::ScrollArea::vertical().id_salt((id, "scroll")).auto_shrink([false, false]).max_height(body.height()).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let total_rows = rows.len().max(((body.height() / row_h) as usize).saturating_sub(0));
        for i in 0..total_rows {
            let (r, resp) = ui.allocate_exact_size(vec2(width - 18.0, row_h), Sense::click());
            let p = ui.painter();
            let num = Rect::from_min_size(r.min, vec2(num_w, row_h));
            p.rect_filled(num, 0.0, t.face);
            bevel(p, num, true, &t);
            p.text(num.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, format!("{}", i + 1), font.clone(), t.text);
            let Some(row) = rows.get(i) else {
                p.line_segment([pos2(num.max.x, r.max.y - 0.5), pos2(r.max.x, r.max.y - 0.5)], Stroke::new(1.0, t.face));
                continue;
            };
            let sel = *selected == Some(i);
            let mut cx = num.max.x;
            for (j, (_, w, numeric)) in cols.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(cx, r.min.y), vec2(*w, row_h));
                cx += w;
                p.rect_filled(cell, 0.0, if sel { t.select } else { t.field });
                p.line_segment([cell.right_top(), cell.right_bottom()], Stroke::new(1.0, t.face));
                p.line_segment([cell.left_bottom(), cell.right_bottom()], Stroke::new(1.0, t.face));
                let text = row.get(j).map(String::as_str).unwrap_or("");
                let (at, align) = if *numeric { (cell.right_center() - vec2(4.0, 0.0), Align2::RIGHT_CENTER) } else { (cell.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER) };
                p.with_clip_rect(cell).text(at, align, text, font.clone(), if sel { t.select_text } else { t.text });
            }
            if resp.clicked() {
                *selected = Some(i);
                ev.clicked = Some(i);
            }
            if resp.double_clicked() {
                *selected = Some(i);
                ev.double_clicked = Some(i);
            }
        }
    });
    ev
}

fn fmt_value(v: f64, decimals: usize) -> String {
    if v.is_finite() { format!("{v:.decimals$}") } else { String::new() }
}

/// A single-line text box with a sunken frame.
pub fn text_box(ui: &mut Ui, text: &mut String, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 21.0), Sense::hover());
    sunken(ui.painter(), rect, t.field, &t);
    let inner = rect.shrink2(vec2(4.0, 2.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.add_sized(inner.size(), egui::TextEdit::singleline(text).frame(egui::Frame::NONE).text_color(t.text).margin(egui::Margin::ZERO))
}

/// A numeric text box with optional spinner arrows. Returns true when the value changed.
/// The text is kept while editing and parsed on every change; invalid text leaves the value.
pub fn num_box(ui: &mut Ui, id: Id, value: &mut f64, decimals: usize, width: f32, spin: Option<f64>) -> bool {
    let t = Tokens::get(ui.ctx());
    let h = 21.0;
    let spin_w = if spin.is_some() { 15.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let field = Rect::from_min_max(rect.min, pos2(rect.max.x - spin_w, rect.max.y));
    sunken(ui.painter(), field, t.field, &t);
    let key = id.with("text");
    let mut text: String = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| fmt_value(*value, decimals));
    let inner = field.shrink2(vec2(4.0, 2.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(id));
    let resp = child.add_sized(
        inner.size(),
        egui::TextEdit::singleline(&mut text).id(id).frame(egui::Frame::NONE).text_color(t.text).margin(egui::Margin::ZERO),
    );
    let mut changed = false;
    if resp.changed() {
        let trimmed = text.trim().replace(',', "");
        if trimmed.is_empty() {
            if value.is_finite() {
                *value = f64::NAN;
                changed = true;
            }
        } else if let Ok(v) = trimmed.parse::<f64>() {
            if (v - *value).abs() > 0.0 || !value.is_finite() {
                *value = v;
                changed = true;
            }
        }
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    if let Some(step) = spin {
        let sr = Rect::from_min_max(pos2(field.max.x, rect.min.y), rect.max);
        let up = Rect::from_min_max(sr.min, pos2(sr.max.x, sr.center().y));
        let down = Rect::from_min_max(pos2(sr.min.x, sr.center().y), sr.max);
        for (r, dir) in [(up, 1.0), (down, -1.0)] {
            let resp = ui.interact(r, id.with(dir as i32), Sense::click());
            let p = ui.painter();
            p.rect_filled(r, 0.0, t.face);
            bevel(p, r, !resp.is_pointer_button_down_on(), &t);
            let c = r.center();
            let tri = if dir > 0.0 {
                vec![pos2(c.x - 3.0, c.y + 1.5), pos2(c.x + 3.0, c.y + 1.5), pos2(c.x, c.y - 1.5)]
            } else {
                vec![pos2(c.x - 3.0, c.y - 1.5), pos2(c.x + 3.0, c.y - 1.5), pos2(c.x, c.y + 1.5)]
            };
            p.add(egui::Shape::convex_polygon(tri, t.text, Stroke::NONE));
            if resp.clicked() {
                let base = if value.is_finite() { *value } else { 0.0 };
                let k = 10f64.powi(decimals as i32);
                *value = ((base + dir * step) * k).round() / k;
                ui.data_mut(|d| d.remove::<String>(key));
                changed = true;
            }
        }
    }
    changed
}

/// Caption plus numeric box, stacked (the reference dialog layout).
pub fn num_field(ui: &mut Ui, id: &str, label: &str, unit: &str, value: &mut f64, decimals: usize, width: f32) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        caption(ui, label, unit, width);
        let step = 10f64.powi(-(decimals.min(3) as i32)).max(0.01);
        changed = num_box(ui, ui.id().with(id), value, decimals, width, Some(if decimals == 0 { 1.0 } else { step * 10.0 }));
    });
    changed
}

/// Caption plus text box.
pub fn text_field(ui: &mut Ui, label: &str, text: &mut String, width: f32) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        caption(ui, label, "", width);
        changed = text_box(ui, text, width).changed();
    });
    changed
}

/// A drop-down list with a classic sunken box. Returns true when the selection changed.
pub fn combo<T: PartialEq + Clone>(ui: &mut Ui, id: &str, current: &mut T, options: &[(T, String)], width: f32) -> bool {
    let shown = options.iter().find(|(v, _)| v == current).map(|(_, l)| l.clone()).unwrap_or_default();
    let mut changed = false;
    egui::ComboBox::from_id_salt(ui.id().with(id)).width(width - 8.0).selected_text(shown).show_ui(ui, |ui| {
        for (v, label) in options {
            if ui.selectable_label(v == current, label).clicked() && v != current {
                *current = v.clone();
                changed = true;
            }
        }
    });
    changed
}

/// Caption plus combo.
pub fn combo_field<T: PartialEq + Clone>(ui: &mut Ui, id: &str, label: &str, current: &mut T, options: &[(T, String)], width: f32) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        caption(ui, label, "", width);
        changed = combo(ui, id, current, options, width);
    });
    changed
}

/// A check box drawn as a classic sunken square.
pub fn check(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(13.0);
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(18.0 + tw, 18.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let b = Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(13.0, 13.0));
    sunken(p, b, t.field, &t);
    if *on {
        let c = b.center();
        p.line_segment([pos2(c.x - 3.5, c.y), pos2(c.x - 1.0, c.y + 2.5)], Stroke::new(2.0, t.text));
        p.line_segment([pos2(c.x - 1.0, c.y + 2.5), pos2(c.x + 3.5, c.y - 3.0)], Stroke::new(2.0, t.text));
    }
    p.text(pos2(b.max.x + 5.0, rect.center().y), Align2::LEFT_CENTER, label, font, t.text);
    resp
}

/// A radio button.
pub fn radio(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(13.0);
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x);
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0 + tw, 18.0), Sense::click());
    let p = ui.painter();
    let c = rect.left_center() + vec2(6.5, 0.0);
    p.circle_filled(c, 6.0, t.field);
    p.circle_stroke(c, 6.0, Stroke::new(1.0, t.shadow));
    if selected {
        p.circle_filled(c, 2.5, t.text);
    }
    p.text(pos2(c.x + 11.0, c.y), Align2::LEFT_CENTER, label, font, t.text);
    resp
}

/// An etched group box with a title; `add` fills it.
pub fn group<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().min;
    let frame = egui::Frame::NONE.inner_margin(egui::Margin { left: 8, right: 8, top: 14, bottom: 6 });
    let inner = frame.show(ui, |ui| ui.vertical(add).inner);
    let rect = inner.response.rect;
    let p = ui.painter();
    let r = Rect::from_min_max(pos2(rect.min.x, top.y + 6.0), rect.max);
    let font = FontId::proportional(12.5);
    let tw = ui.fonts_mut(|f| f.layout_no_wrap(title.to_string(), font.clone(), t.text).size().x);
    let gap = (r.min.x + 6.0, r.min.x + 10.0 + tw);
    let etch = |p: &egui::Painter, off: f32, c: Color32| {
        let r = r.translate(vec2(off, off));
        p.line_segment([pos2(r.min.x, r.min.y), pos2(gap.0, r.min.y)], Stroke::new(1.0, c));
        p.line_segment([pos2(gap.1, r.min.y), pos2(r.max.x - 1.0, r.min.y)], Stroke::new(1.0, c));
        p.line_segment([pos2(r.max.x - 1.0, r.min.y), pos2(r.max.x - 1.0, r.max.y - 1.0)], Stroke::new(1.0, c));
        p.line_segment([pos2(r.max.x - 1.0, r.max.y - 1.0), pos2(r.min.x, r.max.y - 1.0)], Stroke::new(1.0, c));
        p.line_segment([pos2(r.min.x, r.max.y - 1.0), pos2(r.min.x, r.min.y)], Stroke::new(1.0, c));
    };
    if t.bevel {
        etch(p, 1.0, t.light);
    }
    etch(p, 0.0, t.shadow);
    p.text(pos2(gap.0 + 2.0, r.min.y), Align2::LEFT_CENTER, title, font, t.text);
    inner.inner
}

/// A classic tab strip. Returns true when the selection changed.
pub fn tabs(ui: &mut Ui, names: &[&str], selected: &mut usize) -> bool {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(13.0);
    let mut changed = false;
    let start = ui.cursor().min;
    let mut x = start.x + 2.0;
    let h = 21.0;
    let mut sel_rect = None;
    let mut rects = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let w = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), t.text).size().x) + 14.0;
        let r = Rect::from_min_size(pos2(x, start.y + 2.0), vec2(w, h));
        rects.push(r);
        if i == *selected {
            sel_rect = Some(r.expand2(vec2(2.0, 2.0)));
        }
        x += w;
    }
    let width = ui.available_width();
    let (strip, _) = ui.allocate_exact_size(vec2(width, h + 2.0), Sense::hover());
    let p = ui.painter().clone();
    let base_y = strip.max.y - 0.5;
    p.line_segment([pos2(strip.min.x, base_y), pos2(strip.max.x, base_y)], Stroke::new(1.0, if t.bevel { t.light } else { t.shadow }));
    for (i, (r, name)) in rects.iter().zip(names).enumerate() {
        let resp = ui.interact(*r, ui.id().with(("tab", i)), Sense::click());
        if resp.clicked() && *selected != i {
            *selected = i;
            changed = true;
        }
        if i != *selected {
            draw_tab(&p, *r, false, &t);
            p.text(r.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, *name, font.clone(), t.text);
        }
    }
    if let (Some(r), Some(name)) = (sel_rect, names.get(*selected)) {
        p.rect_filled(r, 0.0, t.face);
        draw_tab(&p, r, true, &t);
        p.text(r.center(), Align2::CENTER_CENTER, *name, font, t.text);
    }
    changed
}

fn draw_tab(p: &egui::Painter, r: Rect, selected: bool, t: &Tokens) {
    let l = if t.bevel { t.light } else { t.shadow };
    let r = r.shrink(0.5);
    p.line_segment([r.left_bottom(), pos2(r.min.x, r.min.y + 2.0)], Stroke::new(1.0, l));
    p.line_segment([pos2(r.min.x + 2.0, r.min.y), pos2(r.max.x - 2.0, r.min.y)], Stroke::new(1.0, l));
    p.line_segment([pos2(r.max.x - 1.0, r.min.y + 1.0), pos2(r.max.x - 1.0, r.max.y)], Stroke::new(1.0, t.dark));
    if selected {
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(2.0, t.face));
    }
}

/// A raised panel frame (the area below a tab strip).
pub fn tab_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get(ui.ctx());
    let inner = egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, add);
    let r = inner.response.rect;
    let p = ui.painter();
    let l = if t.bevel { t.light } else { t.shadow };
    p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, l));
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.dark));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.dark));
    inner.inner
}

/// Column description for [`grid`].
pub struct Column<'a> {
    pub title: &'a str,
    pub width: f32,
    /// Right-align (numbers).
    pub numeric: bool,
}

/// What the user did in a grid this frame.
#[derive(Debug, Default)]
pub struct GridEvents {
    pub changed: bool,
    pub selected: Option<usize>,
    pub double_clicked: Option<usize>,
    pub delete_row: Option<usize>,
    pub insert_row: Option<usize>,
}

/// An editable table with a row-number column, one blank row at the end for new entries, and a
/// right-click menu (insert/delete row). Cells are strings; callers convert.
pub fn grid(ui: &mut Ui, id: &str, cols: &[Column], rows: &mut Vec<Vec<String>>, selected: &mut Option<usize>, height: f32, editable: bool) -> GridEvents {
    let t = Tokens::get(ui.ctx());
    let mut ev = GridEvents::default();
    let row_h = 18.0;
    let num_w = 26.0;
    let total_w: f32 = num_w + cols.iter().map(|c| c.width).sum::<f32>() + 16.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total_w, height), Sense::hover());
    sunken(ui.painter(), outer, t.field, &t);
    let inner = outer.shrink(2.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(id));
    let gid = child.id().with(id);
    let font = FontId::proportional(12.5);
    // Header.
    let head = Rect::from_min_size(inner.min, vec2(inner.width(), row_h + 2.0));
    child.painter().rect_filled(head, 0.0, t.face);
    let mut x = inner.min.x;
    for (title, w) in std::iter::once(("#", num_w)).chain(cols.iter().map(|c| (c.title, c.width))) {
        let r = Rect::from_min_size(pos2(x, head.min.y), vec2(w, head.height()));
        bevel(child.painter(), r, true, &t);
        child.painter().with_clip_rect(r.shrink(1.0)).text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, title, font.clone(), t.text);
        x += w;
    }
    if editable && rows.last().is_none_or(|r| r.iter().any(|c| !c.trim().is_empty())) {
        rows.push(vec![String::new(); cols.len()]);
    }
    let body = Rect::from_min_max(pos2(inner.min.x, head.max.y), inner.max);
    let mut body_ui = child.new_child(egui::UiBuilder::new().max_rect(body));
    egui::ScrollArea::vertical().id_salt(gid.with("scroll")).max_height(body.height()).auto_shrink([false, false]).show(&mut body_ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let n = rows.len();
        for i in 0..n {
            let (row_rect, _) = ui.allocate_exact_size(vec2(inner.width() - 14.0, row_h), Sense::hover());
            let p = ui.painter().clone();
            let num_rect = Rect::from_min_size(row_rect.min, vec2(num_w, row_h));
            p.rect_filled(num_rect, 0.0, t.face);
            bevel(&p, num_rect, true, &t);
            let is_sel = *selected == Some(i);
            p.text(num_rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, format!("{}", i + 1), font.clone(), t.text);
            let num_resp = ui.interact(num_rect, gid.with(("num", i)), Sense::click());
            if num_resp.clicked() {
                *selected = Some(i);
                ev.selected = Some(i);
            }
            if num_resp.double_clicked() {
                ev.double_clicked = Some(i);
            }
            if editable {
                num_resp.context_menu(|ui| {
                    if ui.button("Insert row").clicked() {
                        ev.insert_row = Some(i);
                        ui.close();
                    }
                    if ui.button("Delete row").clicked() {
                        ev.delete_row = Some(i);
                        ui.close();
                    }
                });
            }
            let mut cx = row_rect.min.x + num_w;
            for (j, c) in cols.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(cx, row_rect.min.y), vec2(c.width, row_h));
                cx += c.width;
                let bg = if is_sel { t.select } else { t.field };
                let fg = if is_sel { t.select_text } else { t.text };
                p.rect_filled(cell, 0.0, bg);
                p.line_segment([cell.right_top(), cell.right_bottom()], Stroke::new(1.0, t.face));
                p.line_segment([cell.left_bottom(), cell.right_bottom()], Stroke::new(1.0, t.face));
                let Some(value) = rows.get_mut(i).and_then(|r| r.get_mut(j)) else { continue };
                let cid = gid.with(("cell", i, j));
                let editing = ui.memory(|m| m.has_focus(cid));
                if editable && (editing || is_sel) {
                    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(cell.shrink2(vec2(3.0, 1.0))));
                    let r = cui.add_sized(
                        cell.shrink2(vec2(3.0, 1.0)).size(),
                        egui::TextEdit::singleline(value).id(cid).frame(egui::Frame::NONE).text_color(fg).margin(egui::Margin::ZERO).horizontal_align(if c.numeric { egui::Align::RIGHT } else { egui::Align::LEFT }),
                    );
                    if r.changed() {
                        ev.changed = true;
                    }
                    if r.gained_focus() {
                        *selected = Some(i);
                        ev.selected = Some(i);
                    }
                } else {
                    let align = if c.numeric { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
                    let at = if c.numeric { cell.right_center() - vec2(4.0, 0.0) } else { cell.left_center() + vec2(4.0, 0.0) };
                    p.with_clip_rect(cell).text(at, align, value.as_str(), font.clone(), fg);
                    let resp = ui.interact(cell, gid.with(("hit", i, j)), Sense::click());
                    if resp.clicked() {
                        *selected = Some(i);
                        ev.selected = Some(i);
                        if editable {
                            ui.memory_mut(|m| m.request_focus(cid));
                        }
                    }
                    if resp.double_clicked() {
                        ev.double_clicked = Some(i);
                    }
                }
            }
        }
    });
    if let Some(i) = ev.delete_row {
        if i < rows.len() {
            rows.remove(i);
            ev.changed = true;
            *selected = None;
        }
    }
    if let Some(i) = ev.insert_row {
        if i <= rows.len() {
            rows.insert(i, vec![String::new(); cols.len()]);
            ev.changed = true;
        }
    }
    ev
}

/// Parses a grid cell as a number (blank or invalid → None).
pub fn cell_num(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', "");
    if s.is_empty() { None } else { s.parse().ok().filter(|v: &f64| v.is_finite()) }
}

/// Formats a number for a grid cell.
pub fn cell_fmt(v: f64, decimals: usize) -> String {
    if v.is_finite() { format!("{v:.decimals$}") } else { String::new() }
}

/// Paints a window-caption style title bar (used in report headers).
pub fn caption_bar(p: &egui::Painter, rect: Rect, title: &str, t: &Tokens) {
    let steps = 16;
    for i in 0..steps {
        let f = i as f32 / steps as f32;
        let c = lerp_color(t.caption, t.caption_end, f);
        let x0 = rect.min.x + rect.width() * f;
        let x1 = rect.min.x + rect.width() * (i + 1) as f32 / steps as f32;
        p.rect_filled(Rect::from_min_max(pos2(x0, rect.min.y), pos2(x1 + 0.5, rect.max.y)), 0.0, c);
    }
    p.text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(13.0), t.caption_text);
}

pub fn lerp_color(a: Color32, b: Color32, f: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A point helper.
pub fn p2(x: f32, y: f32) -> Pos2 {
    pos2(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_hint_is_taken_once() {
        let ctx = egui::Context::default();
        assert_eq!(take_status_hint(&ctx), None);
        set_status_hint(&ctx, "Zoom in");
        assert_eq!(take_status_hint(&ctx).as_deref(), Some("Zoom in"));
        assert_eq!(take_status_hint(&ctx), None, "cleared until something is hovered again");
    }
}
