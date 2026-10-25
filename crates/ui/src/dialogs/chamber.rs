//! Chamber Wizard: lays out an underground chamber field for a pond and replaces the pond's
//! storage with the stone bed and the embedded chambers.

use super::{Buttons, conv_field, ok_cancel_apply};
use crate::theme::Tokens;
use crate::widgets;
use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use wavestorm_core::chamber::{ChamberModel, Field};
use wavestorm_core::units::Units;

pub struct ChamberWizard {
    pub field: Field,
    /// Index into the model list; the last entry is the user's own chamber.
    pub model: usize,
    pub models: Vec<ChamberModel>,
}

impl ChamberWizard {
    pub fn new(existing: Option<&Field>, invert: f64) -> Self {
        let mut models = ChamberModel::samples();
        let field =
            existing.cloned().unwrap_or_else(|| Field { invert, model: models[1].clone(), ..Default::default() });
        let model = match models.iter().position(|m| *m == field.model) {
            Some(i) => i,
            None => {
                models.push(field.model.clone());
                models.len() - 1
            }
        };
        ChamberWizard { field, model, models }
    }
}

/// Shows the wizard; returns OK (apply the field), Cancel, or None while open.
pub fn show(ctx: &egui::Context, title: &str, w: &mut ChamberWizard, u: &Units) -> Buttons {
    let mut result = Buttons::None;
    let small = |x: f64| u.small_len(x);
    let small_in = |x: f64| u.small_len_in(x);
    let sl = u.small_len_label();
    crate::frame::Win::new(format!("{title}: Chamber Wizard"))
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(860.0, 540.0))
        .show(ctx, |ui| {
            let f = &mut w.field;
            ui.horizontal(|ui| {
                let options: Vec<(usize, String)> =
                    w.models.iter().enumerate().map(|(i, m)| (i, m.name.clone())).collect();
                if widgets::combo_field(ui, "model", "Model:", &mut w.model, &options, 200.0)
                    && let Some(m) = w.models.get(w.model)
                {
                    f.model = m.clone();
                }
                ui.add_space(12.0);
                // The user's own arch chamber: outside width, height and length per unit.
                let m = &mut f.model;
                let mut width = m.outside_width;
                let mut height = m.height();
                let mut length = m.length;
                let a = conv_field(ui, "cw", "Width:", sl, &mut width, small, small_in, 1, 120.0);
                let b = conv_field(ui, "ch", "Height:", sl, &mut height, small, small_in, 1, 120.0);
                let c = conv_field(
                    ui,
                    "cl",
                    "Length each:",
                    u.len_label(),
                    &mut length,
                    |x| u.len(x),
                    |x| u.len_in(x),
                    2,
                    130.0,
                );
                if (a || b || c) && width > 0.0 && height > 0.0 && length > 0.0 {
                    *m = ChamberModel::arch("Custom chamber", width, height, length);
                    if w.models.last().is_none_or(|l| l.name != "Custom chamber") {
                        w.models.push(m.clone());
                    }
                    w.model = w.models.len() - 1;
                    if let Some(l) = w.models.last_mut() {
                        *l = m.clone();
                    }
                }
            });
            ui.label(RichText::new(&f.model.comment).color(Tokens::get(ui.ctx()).text_dim));
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    widgets::check(ui, &mut f.typical, "Use typical spacing");
                    let mut rows = f.rows as f64;
                    if conv_field(ui, "rows", "Number of Rows:", "", &mut rows, |x| x, |x| x, 0, 150.0)
                        && rows.is_finite()
                    {
                        f.rows = rows.clamp(1.0, 500.0) as u32;
                    }
                    let mut per = f.per_row as f64;
                    if conv_field(ui, "per", "Chambers per Row:", "", &mut per, |x| x, |x| x, 0, 150.0)
                        && per.is_finite()
                    {
                        f.per_row = per.clamp(1.0, 500.0) as u32;
                    }
                    ui.add_enabled_ui(!f.typical, |ui| {
                        conv_field(ui, "rs", "Row Spacing:", sl, &mut f.row_spacing, small, small_in, 1, 150.0);
                        conv_field(ui, "ss", "Side Stone:", sl, &mut f.side_stone, small, small_in, 1, 150.0);
                        conv_field(ui, "es", "End Stone:", sl, &mut f.end_stone, small, small_in, 1, 150.0);
                        conv_field(ui, "sc", "Stone Cover:", sl, &mut f.cover, small, small_in, 1, 150.0);
                        conv_field(ui, "sb", "Stone Base:", sl, &mut f.base, small, small_in, 1, 150.0);
                    });
                    conv_field(ui, "z", "Side-Z:", "run/rise", &mut f.side_z, |x| x, |x| x.max(0.0), 1, 150.0);
                    conv_field(
                        ui,
                        "v",
                        "Stone Voids:",
                        "%",
                        &mut f.voids_pct,
                        |x| x,
                        |x| x.clamp(0.0, 100.0),
                        1,
                        150.0,
                    );
                    conv_field(
                        ui,
                        "inv",
                        "Stone Invert:",
                        u.len_label(),
                        &mut f.invert,
                        |x| u.len(x),
                        |x| u.len_in(x),
                        2,
                        150.0,
                    );
                });
                ui.vertical(|ui| {
                    layout_sketch(ui, f, vec2(260.0, 330.0));
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        widgets::check(ui, &mut f.show_costs, "Show Costs");
                    });
                    if f.show_costs {
                        ui.horizontal(|ui| {
                            conv_field(
                                ui,
                                "cc",
                                "Chamber Cost:",
                                "$/ea",
                                &mut f.costs.chamber,
                                |x| x,
                                |x| x.max(0.0),
                                2,
                                110.0,
                            );
                            conv_field(
                                ui,
                                "ce",
                                "Excavation:",
                                "$/cy",
                                &mut f.costs.excavation,
                                |x| x,
                                |x| x.max(0.0),
                                2,
                                110.0,
                            );
                            conv_field(ui, "cs", "Stone:", "$/cy", &mut f.costs.stone, |x| x, |x| x.max(0.0), 2, 110.0);
                        });
                    }
                    let t = Tokens::get(ui.ctx());
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(ui.available_width().max(300.0), if f.show_costs { 290.0 } else { 330.0 }),
                        Sense::hover(),
                    );
                    widgets::sunken(ui.painter(), rect, t.field, &t);
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(4.0)));
                    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
                        for l in f.report() {
                            ui.add(
                                egui::Label::new(RichText::new(l).size(12.0).color(t.text))
                                    .wrap_mode(egui::TextWrapMode::Wrap),
                            );
                        }
                    });
                });
            });
            ui.label(
                RichText::new("OK replaces this pond's storage with the stone field and the chambers embedded in it.")
                    .color(Tokens::get(ui.ctx()).text_dim),
            );
            result = ok_cancel_apply(ui, false, "pond");
        });
    result
}

/// Plan view of the field (rows of chambers in the stone bed) and an end view strip below.
fn layout_sketch(ui: &mut Ui, f: &Field, size: egui::Vec2) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, t.radius as f32, t.field);
    p.rect_stroke(rect, t.radius as f32, Stroke::new(1.0, t.shadow), egui::StrokeKind::Inside);
    let l = f.layout();
    let m = &f.model;
    let (spacing, side, end) =
        if f.typical { (m.row_spacing, m.side_stone, m.end_stone) } else { (f.row_spacing, f.side_stone, f.end_stone) };
    let plan_area = Rect::from_min_max(rect.min + vec2(10.0, 10.0), pos2(rect.max.x - 10.0, rect.max.y - 50.0));
    let s =
        (plan_area.width() as f64 / l.base_width.max(1e-6)).min(plan_area.height() as f64 / l.base_length.max(1e-6));
    if !(s.is_finite() && s > 0.0) {
        return;
    }
    let ink = crate::icons::ink(&t, Color32::from_rgb(20, 20, 160));
    let water = if t.is_dark() { Color32::from_rgb(0, 130, 150) } else { Color32::from_rgb(0, 230, 255) };
    let w = (l.base_width * s) as f32;
    let h = (l.base_length * s) as f32;
    let bed = Rect::from_center_size(plan_area.center(), vec2(w, h));
    p.rect_stroke(bed, 0.0, Stroke::new(1.5, ink), egui::StrokeKind::Inside);
    let rows = f.rows.max(1);
    let per = f.per_row.max(1);
    // Draw at most a readable number of chambers; larger fields keep their outline.
    let detailed = rows * per <= 4000;
    for r in 0..rows {
        let x = bed.min.x + ((side + r as f64 * (m.outside_width + spacing)) * s) as f32;
        let cw = (m.outside_width * s) as f32;
        let run =
            Rect::from_min_size(pos2(x, bed.min.y + (end * s) as f32), vec2(cw, (per as f64 * m.length * s) as f32));
        p.rect_filled(run, 0.0, water);
        if detailed {
            for c in 1..per {
                let y = run.min.y + (c as f64 * m.length * s) as f32;
                p.line_segment([pos2(run.min.x, y), pos2(run.max.x, y)], Stroke::new(1.0, ink));
            }
        }
        p.rect_stroke(run, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    }
    // End view: a row of arches on the stone base.
    let strip = Rect::from_min_max(pos2(bed.min.x, rect.max.y - 40.0), pos2(bed.max.x, rect.max.y - 10.0));
    p.rect_stroke(strip, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    let sx = strip.width() / l.base_width.max(1e-6) as f32;
    let ch = strip.height() * (m.outside_height / l.height.max(1e-6)) as f32;
    for r in 0..rows {
        let x0 = strip.min.x + ((side + r as f64 * (m.outside_width + spacing)) as f32) * sx;
        let cw = m.outside_width as f32 * sx;
        let base_y = strip.max.y - 2.0;
        let pts: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 12.0;
                pos2(x0 + cw / 2.0 - a.cos() * cw / 2.0, base_y - a.sin() * ch)
            })
            .collect();
        p.add(egui::Shape::convex_polygon(pts.clone(), water, Stroke::new(1.0, ink)));
    }
}
