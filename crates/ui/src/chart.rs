//! Report charts: hydrographs and rating curves, in 3-D ribbon or 2-D line style.

use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

/// One plotted curve.
#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub color: Color32,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Show a boxed peak label ("18.56 cfs").
    pub peak_label: Option<String>,
}

/// What to draw.
pub struct ChartSpec<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub x_label: &'a str,
    pub y_label: &'a str,
    pub series: &'a [Series],
    /// Large right-aligned summary lines ("Runoff=18.56 cfs").
    pub summary: &'a [String],
    pub three_d: bool,
    /// Horizontal reference lines (value, label).
    pub marks: &'a [(f64, String)],
}

/// Zoom state kept per chart.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ChartView {
    pub x: Option<(f64, f64)>,
    pub y: Option<(f64, f64)>,
}

fn nice_step(range: f64, target: usize) -> f64 {
    if !(range > 0.0) {
        return 1.0;
    }
    let raw = range / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm < 1.5 { 1.0 } else if norm < 3.5 { 2.0 } else if norm < 7.5 { 5.0 } else { 10.0 };
    step * mag
}

fn fmt_tick(v: f64, step: f64) -> String {
    let dp = if step >= 1.0 { 0 } else { (-step.log10().floor()) as usize };
    format!("{:.*}", dp.min(4), v)
}

fn bold(p: &egui::Painter, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    p.text(at, align, text, font.clone(), color);
    p.text(at + vec2(0.7, 0.0), align, text, font, color);
}

/// Draws a chart filling `size`; returns nothing but handles drag-zoom on `view`.
pub fn chart(ui: &mut Ui, spec: &ChartSpec, view: &mut ChartView, size: Vec2) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.chart_bg);
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, t.shadow), egui::StrokeKind::Inside);
    let mut top = rect.min.y + 6.0;
    if !spec.subtitle.is_empty() {
        bold(&p, pos2(rect.center().x, top + 8.0), Align2::CENTER_CENTER, spec.subtitle, FontId::proportional(14.0), t.text);
        top += 18.0;
    }
    bold(&p, pos2(rect.center().x, top + 8.0), Align2::CENTER_CENTER, spec.title, FontId::proportional(14.0), t.chart_title);
    top += 20.0;

    // Data ranges.
    let mut xr = (f64::INFINITY, f64::NEG_INFINITY);
    let mut yr = (0.0f64, f64::NEG_INFINITY);
    for s in spec.series {
        for (x, y) in s.x.iter().zip(&s.y) {
            if x.is_finite() && y.is_finite() {
                xr = (xr.0.min(*x), xr.1.max(*x));
                yr = (yr.0.min(*y), yr.1.max(*y));
            }
        }
    }
    for (v, _) in spec.marks {
        yr = (yr.0.min(*v), yr.1.max(*v));
    }
    if !xr.0.is_finite() || xr.1 <= xr.0 {
        xr = (0.0, 1.0);
    }
    if !yr.1.is_finite() || yr.1 <= yr.0 {
        yr = (yr.0, yr.0 + 1.0);
    }
    // Rating curves (elevation on y) shouldn't start at zero.
    let y_from_zero = spec.marks.is_empty() && yr.0 >= 0.0;
    if !y_from_zero {
        let min_y = spec.series.iter().flat_map(|s| s.y.iter().copied()).filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min);
        let min_y = spec.marks.iter().map(|m| m.0).fold(min_y, f64::min);
        if min_y.is_finite() {
            yr.0 = min_y;
        }
    }
    let (x0, x1) = view.x.unwrap_or(xr);
    let ystep_auto = nice_step(yr.1 - yr.0, 8);
    let (y0, y1) = view.y.unwrap_or(((yr.0 / ystep_auto).floor() * ystep_auto, (yr.1 * 1.08 / ystep_auto).ceil() * ystep_auto));

    let summary_w = if spec.summary.is_empty() { 0.0 } else { (rect.width() * 0.34).min(260.0) };
    let legend_w: f32 = if spec.series.len() > 1 || !spec.summary.is_empty() { 96.0 } else { 0.0 };
    let depth = if spec.three_d { vec2(10.0, -7.0) * (spec.series.len().max(1) as f32).sqrt() } else { Vec2::ZERO };
    let plot = Rect::from_min_max(pos2(rect.min.x + 58.0, top + 6.0 - depth.y), pos2(rect.max.x - legend_w.max(16.0) - depth.x, rect.max.y - 40.0));
    if plot.width() < 40.0 || plot.height() < 40.0 {
        return;
    }
    let sx = |x: f64| plot.min.x + ((x - x0) / (x1 - x0)) as f32 * plot.width();
    let sy = |y: f64| plot.max.y - ((y - y0) / (y1 - y0)) as f32 * plot.height();

    // Back wall and floor (3-D) or plain frame.
    if spec.three_d {
        let back = plot.translate(depth);
        p.rect_filled(back, 0.0, t.chart_bg);
        p.add(Shape::convex_polygon(vec![plot.left_bottom(), plot.right_bottom(), back.right_bottom(), back.left_bottom()], t.chart_bg.gamma_multiply(0.92), Stroke::new(1.0, t.chart_grid)));
        p.add(Shape::convex_polygon(vec![plot.left_top(), plot.left_bottom(), back.left_bottom(), back.left_top()], t.chart_bg.gamma_multiply(0.96), Stroke::new(1.0, t.chart_grid)));
    }
    let grid = |a: Pos2, b: Pos2| {
        p.extend(Shape::dashed_line(&[a, b], Stroke::new(1.0, t.chart_grid), 3.0, 3.0));
    };
    let xstep = nice_step(x1 - x0, ((plot.width() / 28.0) as usize).clamp(2, 40));
    let ystep = nice_step(y1 - y0, ((plot.height() / 22.0) as usize).clamp(2, 20));
    let font = FontId::proportional(11.5);
    let mut x = (x0 / xstep).ceil() * xstep;
    while x <= x1 + 1e-9 {
        let px = sx(x);
        grid(pos2(px, plot.max.y) + depth, pos2(px, plot.min.y) + depth);
        if spec.three_d {
            p.line_segment([pos2(px, plot.max.y), pos2(px, plot.max.y) + depth], Stroke::new(1.0, t.chart_grid));
        }
        p.text(pos2(px, plot.max.y + 4.0), Align2::CENTER_TOP, fmt_tick(x, xstep), font.clone(), t.text);
        x += xstep;
    }
    let mut y = (y0 / ystep).ceil() * ystep;
    while y <= y1 + 1e-9 {
        let py = sy(y);
        grid(pos2(plot.min.x, py) + depth, pos2(plot.max.x, py) + depth);
        if spec.three_d {
            p.line_segment([pos2(plot.min.x, py), pos2(plot.min.x, py) + depth], Stroke::new(1.0, t.chart_grid));
        }
        p.text(pos2(plot.min.x - 5.0, py), Align2::RIGHT_CENTER, fmt_tick(y, ystep), font.clone(), t.text);
        y += ystep;
    }
    bold(&p, pos2((plot.min.x + plot.max.x) / 2.0, rect.max.y - 12.0), Align2::CENTER_CENTER, spec.x_label, FontId::proportional(12.5), t.text);
    let galley = p.layout_no_wrap(spec.y_label.to_string(), FontId::proportional(12.5), t.text);
    let gpos = pos2(rect.min.x + 6.0, (plot.min.y + plot.max.y) / 2.0 + galley.size().x / 2.0);
    p.add(egui::epaint::TextShape::new(gpos, galley, t.text).with_angle(-std::f32::consts::FRAC_PI_2));

    // Horizontal marks (flood elevation etc.).
    for (v, label) in spec.marks {
        if *v >= y0 && *v <= y1 {
            let py = sy(*v);
            p.line_segment([pos2(plot.min.x, py), pos2(plot.max.x, py)], Stroke::new(1.5, Color32::from_rgb(200, 60, 60)));
            p.text(pos2(plot.min.x + 4.0, py - 2.0), Align2::LEFT_BOTTOM, label, font.clone(), Color32::from_rgb(160, 30, 30));
        }
    }

    // Series: back to front.
    let clip = p.with_clip_rect(plot.expand2(vec2(depth.x.abs() + 2.0, depth.y.abs() + 2.0)).translate(vec2(depth.x / 2.0, depth.y / 2.0)).intersect(rect));
    let n = spec.series.len();
    let mut peak_boxes: Vec<(Pos2, String, Color32)> = Vec::new();
    for (i, s) in spec.series.iter().enumerate() {
        let k = if spec.three_d && n > 1 { (n - 1 - i) as f32 / (n - 1) as f32 } else { 0.0 };
        let off = depth * k;
        let pts: Vec<Pos2> = s.x.iter().zip(&s.y).filter(|(x, y)| x.is_finite() && y.is_finite()).map(|(x, y)| pos2(sx(*x), sy(*y)) + off).collect();
        if pts.len() < 2 {
            continue;
        }
        if spec.three_d {
            let base = sy(y0.max(0.0).min(y1)) ;
            let light = Color32::from_rgba_unmultiplied(s.color.r(), s.color.g(), s.color.b(), 70);
            // Area under the curve in thin vertical strips (works for any shape).
            // Filled area and ribbon as plain triangle meshes (no feathering, so no spikes on
            // degenerate quads).
            let mut mesh = egui::Mesh::default();
            let mut quad = |m: &mut egui::Mesh, q: [Pos2; 4], c: Color32| {
                let i = m.vertices.len() as u32;
                for v in q {
                    m.colored_vertex(v, c);
                }
                m.add_triangle(i, i + 1, i + 2);
                m.add_triangle(i, i + 2, i + 3);
            };
            let rib = depth / (n.max(1) as f32) * 0.8;
            for w in pts.windows(2) {
                quad(&mut mesh, [w[0], w[1], pos2(w[1].x, base + off.y), pos2(w[0].x, base + off.y)], light);
            }
            for w in pts.windows(2) {
                quad(&mut mesh, [w[0], w[1], w[1] + rib, w[0] + rib], s.color);
            }
            clip.add(Shape::mesh(mesh));
            clip.add(Shape::line(pts.clone(), Stroke::new(1.0, s.color.gamma_multiply(0.6))));
            // Diagonal hatch under the curve.
            let step = 7.0;
            let mut hx = plot.min.x;
            while hx < plot.max.x {
                let xv = x0 + ((hx - plot.min.x) / plot.width()) as f64 * (x1 - x0);
                let yv = interp(&s.x, &s.y, xv);
                if let Some(yv) = yv {
                    let top = sy(yv) + off.y;
                    let bottom = base + off.y;
                    if bottom - top > 4.0 {
                        let a = pos2(hx + off.x, bottom);
                        let b = pos2(hx + off.x + (bottom - top).min(step * 1.2), (bottom - (bottom - top).min(step * 1.2)).max(top));
                        clip.line_segment([a, b], Stroke::new(1.0, s.color.gamma_multiply(0.7)));
                    }
                }
                hx += step;
            }
        } else {
            clip.add(Shape::line(pts.clone(), Stroke::new(2.0, s.color)));
        }
        if let Some(label) = &s.peak_label {
            if let Some((j, _)) = s.y.iter().enumerate().filter(|(_, v)| v.is_finite()).max_by(|a, b| a.1.total_cmp(b.1)) {
                if let (Some(x), Some(y)) = (s.x.get(j), s.y.get(j)) {
                    if *y > 0.0 {
                        peak_boxes.push((pos2(sx(*x), sy(*y)) + off, label.clone(), s.color));
                    }
                }
            }
        }
    }
    // Summary text (right-aligned, large, bold).
    if !spec.summary.is_empty() {
        let size = (plot.height() / (spec.summary.len() as f32 + 1.0)).clamp(11.0, 20.0);
        let mut sy_ = plot.min.y + 4.0;
        let x = plot.max.x - 6.0;
        for line in spec.summary {
            let f = FontId::proportional(size);
            p.text(pos2(x + 1.5, sy_ + 1.5), Align2::RIGHT_TOP, line, f.clone(), t.chart_bg.gamma_multiply(0.6));
            bold(&p, pos2(x, sy_), Align2::RIGHT_TOP, line, f, t.text);
            sy_ += size * 1.25;
        }
        let _ = summary_w;
    }

    // Peak labels, nudged apart vertically.
    let mut placed: Vec<Rect> = Vec::new();
    for (at, label, color) in peak_boxes {
        let g = p.layout_no_wrap(label.clone(), FontId::proportional(11.5), Color32::BLACK);
        let mut r = Rect::from_min_size(at + vec2(4.0, -g.size().y - 8.0), g.size() + vec2(8.0, 4.0));
        if r.max.x > plot.max.x {
            r = r.translate(vec2(plot.max.x - r.max.x, 0.0));
        }
        if r.min.x < plot.min.x + 2.0 {
            r = r.translate(vec2(plot.min.x + 2.0 - r.min.x, 0.0));
        }
        if r.min.y < plot.min.y {
            r = r.translate(vec2(0.0, plot.min.y - r.min.y));
        }
        for _ in 0..10 {
            if placed.iter().any(|o| o.intersects(r)) {
                r = r.translate(vec2(0.0, r.height() + 2.0));
            }
        }
        placed.push(r);
        p.rect_filled(r, 0.0, Color32::from_rgb(255, 255, 220));
        p.rect_stroke(r, 0.0, Stroke::new(1.0, color.gamma_multiply(0.8)), egui::StrokeKind::Inside);
        p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(11.5), Color32::BLACK);
    }

    // Legend.
    if legend_w > 0.0 {
        let lx = rect.max.x - legend_w + 4.0;
        let mut ly = top + 4.0;
        let lr = Rect::from_min_size(pos2(lx, ly), vec2(legend_w - 10.0, 4.0 + 15.0 * spec.series.len() as f32));
        p.rect_filled(lr, 0.0, Color32::WHITE);
        p.rect_stroke(lr, 0.0, Stroke::new(1.0, t.dark), egui::StrokeKind::Inside);
        for s in spec.series {
            ly += 2.0;
            let sw = Rect::from_min_size(pos2(lx + 4.0, ly + 2.0), vec2(10.0, 10.0));
            p.rect_filled(sw, 0.0, s.color);
            p.rect_stroke(sw, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
            p.text(pos2(sw.max.x + 4.0, sw.center().y), Align2::LEFT_CENTER, &s.name, FontId::proportional(11.0), Color32::BLACK);
            ly += 13.0;
        }
    }
    // Drag-zoom: drag down-right to zoom, up-left to reset.
    if resp.dragged() {
        if let (Some(a), Some(b)) = (resp.interact_pointer_pos().map(|q| q - resp.drag_delta()), resp.interact_pointer_pos()) {
            let _ = a;
            let origin = ui.ctx().data(|d| d.get_temp::<Pos2>(resp.id.with("origin"))).unwrap_or(b);
            ui.painter().rect_stroke(Rect::from_two_pos(origin, b), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
        }
    }
    if resp.drag_started() {
        if let Some(p0) = resp.interact_pointer_pos() {
            ui.ctx().data_mut(|d| d.insert_temp(resp.id.with("origin"), p0));
        }
    }
    if resp.drag_stopped() {
        let origin = ui.ctx().data(|d| d.get_temp::<Pos2>(resp.id.with("origin")));
        if let (Some(a), Some(b)) = (origin, resp.interact_pointer_pos()) {
            if b.x < a.x || b.y < a.y {
                *view = ChartView::default();
            } else if (b.x - a.x) > 6.0 && (b.y - a.y) > 6.0 {
                let wx = |px: f32| x0 + ((px - plot.min.x) / plot.width()) as f64 * (x1 - x0);
                let wy = |py: f32| y0 + ((plot.max.y - py) / plot.height()) as f64 * (y1 - y0);
                view.x = Some((wx(a.x), wx(b.x)));
                view.y = Some((wy(b.y), wy(a.y)));
            }
        }
    }
}

fn interp(xs: &[f64], ys: &[f64], x: f64) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let i = xs.partition_point(|v| *v <= x);
    if i == 0 || i >= xs.len() {
        return None;
    }
    let (a, b) = (xs[i - 1], xs[i]);
    let f = if b > a { (x - a) / (b - a) } else { 0.0 };
    Some(ys.get(i - 1)? + (ys.get(i)? - ys.get(i - 1)?) * f)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ticks() {
        assert_eq!(super::nice_step(30.0, 15), 2.0);
        assert_eq!(super::fmt_tick(0.25, 0.05), "0.25");
        assert_eq!(super::interp(&[0.0, 1.0], &[0.0, 2.0], 0.5), Some(1.0));
    }
}
