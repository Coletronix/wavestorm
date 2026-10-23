//! Node symbols and toolbar glyphs, drawn with the painter (no bitmap assets).

use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use stormwaves_core::project::NodeType;

/// Outline points of a node symbol of diameter `d` centred at `c`.
pub fn node_outline(t: NodeType, c: Pos2, d: f32) -> Vec<Pos2> {
    let r = d / 2.0;
    match t {
        NodeType::Subcat => (0..6)
            .map(|i| {
                let a = std::f32::consts::PI / 3.0 * i as f32;
                pos2(c.x + r * a.cos(), c.y + r * 0.87 * a.sin())
            })
            .collect(),
        NodeType::Reach => {
            let h = r * 0.72;
            vec![pos2(c.x - h, c.y - h), pos2(c.x + h, c.y - h), pos2(c.x + h, c.y + h), pos2(c.x - h, c.y + h)]
        }
        NodeType::Pond => vec![pos2(c.x, c.y - r * 0.95), pos2(c.x + r, c.y + r * 0.72), pos2(c.x - r, c.y + r * 0.72)],
        NodeType::Link => {
            // A banner with wavy top and bottom edges.
            let (w, h) = (r * 0.62, r * 0.82);
            let mut pts = Vec::new();
            let n = 12;
            for i in 0..=n {
                let f = i as f32 / n as f32;
                pts.push(pos2(c.x - w + 2.0 * w * f, c.y - h + (f * std::f32::consts::TAU).sin() * h * 0.12));
            }
            for i in (0..=n).rev() {
                let f = i as f32 / n as f32;
                pts.push(pos2(c.x - w + 2.0 * w * f, c.y + h + (f * std::f32::consts::TAU).sin() * h * 0.12));
            }
            pts
        }
        NodeType::Text => {
            let (w, h) = (r * 0.9, r * 0.45);
            vec![pos2(c.x - w, c.y - h), pos2(c.x + w, c.y - h), pos2(c.x + w, c.y + h), pos2(c.x - w, c.y + h)]
        }
    }
}

pub fn node_colors(t: NodeType, tok: &Tokens) -> (Color32, Color32) {
    match t {
        NodeType::Subcat => tok.subcat,
        NodeType::Reach => tok.reach,
        NodeType::Pond => tok.pond,
        NodeType::Link => tok.link,
        NodeType::Text => (Color32::TRANSPARENT, tok.shadow),
    }
}

/// Draws a node symbol with its number (or palette caption) inside.
pub fn draw_node(p: &Painter, t: NodeType, c: Pos2, d: f32, label: &str, selected: bool, external: bool, tok: &Tokens) {
    draw_node_ex(p, t, c, d, label, selected, external, false, tok);
}

/// Like `draw_node`, plus a red outline when `problem` is set (undefined results or errors).
/// A selected node gets a bold, dark outline. `external` adds the small circle marker under
/// the symbol (manual hydrograph or tailwater elevation).
pub fn draw_node_ex(p: &Painter, t: NodeType, c: Pos2, d: f32, label: &str, selected: bool, external: bool, problem: bool, tok: &Tokens) {
    let (fill, base_stroke) = node_colors(t, tok);
    let stroke = if selected && t != NodeType::Text { dark(base_stroke) } else { base_stroke };
    let pts = node_outline(t, c, d);
    let width = if selected { (d * 0.09).clamp(3.0, 6.0) } else { (d * 0.025).clamp(1.0, 2.0) };
    if problem && t != NodeType::Text {
        let red = Color32::from_rgb(220, 20, 20);
        let big = node_outline(t, c, d * 1.18);
        p.add(Shape::closed_line(big, Stroke::new((d * 0.05).clamp(2.0, 3.5), red)));
    }
    if t == NodeType::Text {
        dashed_polygon(p, &pts, Stroke::new(1.0, stroke));
    } else if t == NodeType::Link {
        // The banner is concave; draw it as a filled mesh of strips.
        let n = pts.len() / 2;
        for i in 0..n.saturating_sub(1) {
            let quad = vec![pts[i], pts[i + 1], pts[pts.len() - 2 - i], pts[pts.len() - 1 - i]];
            p.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
        }
        p.add(Shape::closed_line(pts.clone(), Stroke::new(width, stroke)));
    } else {
        p.add(Shape::convex_polygon(pts, fill, Stroke::new(width, stroke)));
    }
    if external {
        let r = d * 0.11;
        let cc = pos2(c.x, c.y + d * 0.5 + r + 1.0);
        p.circle_filled(cc, r, tok.canvas);
        p.circle_stroke(cc, r, Stroke::new(1.3, base_stroke));
    }
    if !label.is_empty() {
        let size = (d * 0.34).clamp(7.0, 28.0) * if label.chars().count() > 3 { 0.75 } else { 1.0 };
        let dy = if t == NodeType::Pond { d * 0.14 } else { 0.0 };
        p.text(c + vec2(0.0, dy), Align2::CENTER_CENTER, label, FontId::proportional(size), tok.node_text);
    }
}

fn dark(c: Color32) -> Color32 {
    Color32::from_rgb(c.r() / 3, c.g() / 3, (c.b() / 3).max(40))
}

fn dashed_polygon(p: &Painter, pts: &[Pos2], stroke: Stroke) {
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        p.extend(Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
    }
}

/// Toolbar glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomSelected,
    Find,
    Delete,
    Edit,
    Report,
    Compare,
    ProjectReport,
    Print,
    PrintDiagram,
    Calculator,
    Units,
    LandUse,
    DiagramSettings,
    General,
    Idf,
    Notes,
    Help,
    Messages,
    Underlay,
    Calibrate,
    Trace,
    Measure,
    Pointer,
    Undo,
    Redo,
    Open,
    Save,
    New,
}

pub fn draw_glyph(p: &Painter, g: Glyph, r: Rect, t: &Tokens) {
    let ink = t.text;
    let thin = Stroke::new(1.3, ink);
    let c = r.center();
    let s = r.width().min(r.height());
    match g {
        Glyph::ZoomIn | Glyph::ZoomOut => {
            let lc = c + vec2(-s * 0.08, -s * 0.08);
            p.circle_filled(lc, s * 0.33, Color32::from_rgb(230, 240, 255));
            p.circle_stroke(lc, s * 0.33, Stroke::new(1.6, ink));
            p.line_segment([lc + vec2(s * 0.24, s * 0.24), c + vec2(s * 0.48, s * 0.48)], Stroke::new(3.0, ink));
            p.line_segment([lc - vec2(s * 0.17, 0.0), lc + vec2(s * 0.17, 0.0)], Stroke::new(1.8, ink));
            if g == Glyph::ZoomIn {
                p.line_segment([lc - vec2(0.0, s * 0.17), lc + vec2(0.0, s * 0.17)], Stroke::new(1.8, ink));
            }
        }
        Glyph::ZoomFit | Glyph::ZoomSelected => {
            let col = Color32::from_rgb(200, 30, 30);
            p.rect_filled(r.shrink(s * 0.12), 0.0, Color32::from_rgb(255, 235, 235));
            p.rect_stroke(r.shrink(s * 0.12), 0.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                let tip = c + vec2(dx, dy) * s * 0.4;
                p.line_segment([c, tip], Stroke::new(1.6, col));
                let side = vec2(dy, dx) * s * 0.12;
                let back = tip - vec2(dx, dy) * s * 0.15;
                p.add(Shape::convex_polygon(vec![tip, back + side, back - side], col, Stroke::NONE));
            }
            if g == Glyph::ZoomSelected {
                p.circle_filled(c, s * 0.1, ink);
            }
        }
        Glyph::Find => {
            for dx in [-0.2, 0.2] {
                let cc = c + vec2(dx * s, s * 0.12);
                p.circle_filled(cc, s * 0.2, ink);
                p.circle_filled(cc, s * 0.1, Color32::from_rgb(150, 190, 240));
                p.rect_filled(Rect::from_center_size(c + vec2(dx * s, -s * 0.18), vec2(s * 0.18, s * 0.3)), 1.0, ink);
            }
        }
        Glyph::Delete => {
            let x = Color32::from_rgb(30, 30, 30);
            p.line_segment([r.left_top() + vec2(3.0, 3.0), r.right_bottom() - vec2(3.0, 3.0)], Stroke::new(3.0, x));
            p.line_segment([r.right_top() + vec2(-3.0, 3.0), r.left_bottom() + vec2(3.0, -3.0)], Stroke::new(3.0, x));
        }
        Glyph::Edit | Glyph::Notes => {
            let page = Rect::from_min_size(r.min + vec2(s * 0.1, 0.0), vec2(s * 0.62, s * 0.9));
            p.rect_filled(page, 0.0, Color32::WHITE);
            p.rect_stroke(page, 0.0, thin, egui::StrokeKind::Inside);
            for k in 0..4 {
                let y = page.min.y + s * (0.2 + 0.17 * k as f32);
                p.line_segment([pos2(page.min.x + 3.0, y), pos2(page.max.x - 3.0, y)], Stroke::new(1.0, t.shadow));
            }
            if g == Glyph::Edit {
                let a = c + vec2(s * 0.42, -s * 0.38);
                let b = c + vec2(-s * 0.02, s * 0.12);
                p.line_segment([a, b], Stroke::new(3.0, Color32::from_rgb(200, 150, 30)));
                p.line_segment([b, b + vec2(-s * 0.08, s * 0.08)], Stroke::new(2.0, ink));
            }
        }
        Glyph::Report | Glyph::Compare => {
            p.rect_filled(r, 0.0, Color32::from_rgb(255, 255, 190));
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Inside);
            let pts: Vec<Pos2> = (0..12)
                .map(|i| {
                    let f = i as f32 / 11.0;
                    let y = (-(f - 0.3) * (f - 0.3) * 30.0).exp();
                    pos2(r.min.x + 2.0 + f * (r.width() - 4.0), r.max.y - 2.0 - y * (r.height() - 5.0))
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(1.6, Color32::from_rgb(20, 60, 220))));
            if g == Glyph::Compare {
                let pts: Vec<Pos2> = (0..12)
                    .map(|i| {
                        let f = i as f32 / 11.0;
                        let y = 0.55 * (-(f - 0.55) * (f - 0.55) * 12.0).exp();
                        pos2(r.min.x + 2.0 + f * (r.width() - 4.0), r.max.y - 2.0 - y * (r.height() - 5.0))
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(1.6, Color32::from_rgb(220, 40, 40))));
            }
        }
        Glyph::ProjectReport => {
            p.rect_filled(r, 0.0, Color32::WHITE);
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Inside);
            for k in 0..4 {
                let y = r.min.y + 3.0 + k as f32 * (r.height() - 6.0) / 3.0;
                p.line_segment([pos2(r.min.x + 3.0, y), pos2(r.max.x - 3.0, y)], Stroke::new(1.0, t.shadow));
                p.rect_filled(Rect::from_min_size(pos2(r.min.x + 3.0, y - 1.0), vec2(3.0, 2.0)), 0.0, Color32::from_rgb(40, 140, 40));
            }
        }
        Glyph::Print | Glyph::PrintDiagram => {
            let body = Rect::from_min_max(pos2(r.min.x, c.y - s * 0.1), pos2(r.max.x, r.max.y - s * 0.12));
            let paper = Rect::from_min_max(pos2(r.min.x + s * 0.2, r.min.y), pos2(r.max.x - s * 0.2, c.y));
            p.rect_filled(paper, 0.0, Color32::WHITE);
            p.rect_stroke(paper, 0.0, thin, egui::StrokeKind::Inside);
            p.rect_filled(body, 2.0, t.shadow);
            p.rect_stroke(body, 2.0, thin, egui::StrokeKind::Inside);
            if g == Glyph::PrintDiagram {
                p.circle_filled(paper.center(), s * 0.08, Color32::from_rgb(40, 150, 40));
            }
        }
        Glyph::Calculator => {
            let body = r.shrink2(vec2(s * 0.15, 0.0));
            p.rect_filled(body, 1.0, Color32::from_rgb(90, 90, 100));
            p.rect_filled(Rect::from_min_size(body.min + vec2(2.0, 2.0), vec2(body.width() - 4.0, s * 0.22)), 0.0, Color32::from_rgb(190, 230, 190));
            for k in 0..3 {
                for j in 0..3 {
                    let kc = pos2(body.min.x + 3.5 + j as f32 * (body.width() - 7.0) / 2.0, body.min.y + s * 0.42 + k as f32 * s * 0.2);
                    p.rect_filled(Rect::from_center_size(kc, vec2(2.5, 2.5)), 0.0, Color32::WHITE);
                }
            }
        }
        Glyph::LandUse => {
            let top = pos2(c.x, r.min.y);
            p.add(Shape::convex_polygon(vec![top, pos2(r.max.x, c.y + s * 0.1), pos2(r.min.x, c.y + s * 0.1)], Color32::from_rgb(40, 140, 50), Stroke::new(1.0, ink)));
            p.line_segment([pos2(c.x, c.y + s * 0.1), pos2(c.x, r.max.y - 2.0)], Stroke::new(2.0, ink));
            p.line_segment([pos2(r.min.x, r.max.y - 1.0), pos2(r.max.x, r.max.y - 1.0)], Stroke::new(2.0, Color32::from_rgb(40, 220, 60)));
        }
        Glyph::DiagramSettings => {
            p.rect_filled(r.shrink2(vec2(s * 0.1, 0.0)), 0.0, Color32::WHITE);
            p.rect_stroke(r.shrink2(vec2(s * 0.1, 0.0)), 0.0, thin, egui::StrokeKind::Inside);
            p.text(c, Align2::CENTER_CENTER, "A", FontId::proportional(s * 0.8), ink);
        }
        Glyph::General => {
            let grey = Color32::from_rgb(110, 110, 120);
            p.line_segment([pos2(r.min.x + s * 0.25, r.max.y), pos2(r.min.x + s * 0.25, c.y)], Stroke::new(3.0, grey));
            p.circle_stroke(pos2(r.min.x + s * 0.25, c.y - s * 0.15), s * 0.2, Stroke::new(2.0, grey));
            p.line_segment([pos2(r.max.x - s * 0.2, r.max.y), pos2(r.max.x - s * 0.2, r.min.y + 1.0)], Stroke::new(3.0, ink));
        }
        Glyph::Idf => {
            for (k, col) in [Color32::from_rgb(200, 40, 40), Color32::from_rgb(30, 60, 200), Color32::from_rgb(30, 160, 200)].into_iter().enumerate() {
                let x = r.min.x + 1.0 + k as f32 * s * 0.3;
                let b = Rect::from_min_max(pos2(x, r.min.y + s * (0.1 + 0.15 * k as f32)), pos2(x + s * 0.26, r.max.y));
                p.rect_filled(b, 0.0, col);
                p.rect_stroke(b, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            }
        }
        Glyph::Units => {
            p.text(c, Align2::CENTER_CENTER, "12", FontId::proportional(s * 0.75), ink);
            p.line_segment([pos2(r.min.x, r.max.y), pos2(r.max.x, r.max.y)], Stroke::new(1.0, ink));
        }
        Glyph::Help => {
            p.rect_filled(r, 0.0, Color32::from_rgb(255, 230, 40));
            p.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(s * 0.95), Color32::from_rgb(200, 0, 0));
        }
        Glyph::Messages => {
            p.rect_filled(r, 0.0, Color32::from_rgb(255, 255, 220));
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Inside);
            p.text(c, Align2::CENTER_CENTER, "!", FontId::proportional(s * 0.85), Color32::from_rgb(200, 0, 0));
        }
        Glyph::Underlay => {
            p.rect_filled(r, 0.0, Color32::from_rgb(220, 235, 210));
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Inside);
            let pts = vec![pos2(r.min.x + 2.0, r.max.y - 3.0), pos2(c.x - 2.0, c.y), pos2(c.x + 2.0, c.y + 3.0), pos2(r.max.x - 2.0, r.min.y + 4.0)];
            p.add(Shape::line(pts, Stroke::new(1.2, Color32::from_rgb(120, 90, 40))));
        }
        Glyph::Calibrate => {
            p.line_segment([pos2(r.min.x, c.y), pos2(r.max.x, c.y)], Stroke::new(1.5, ink));
            for x in [r.min.x + 1.0, r.max.x - 1.0] {
                p.line_segment([pos2(x, c.y - s * 0.3), pos2(x, c.y + s * 0.3)], Stroke::new(1.5, ink));
            }
            for k in 1..4 {
                let x = r.min.x + r.width() * k as f32 / 4.0;
                p.line_segment([pos2(x, c.y - s * 0.15), pos2(x, c.y)], Stroke::new(1.0, ink));
            }
        }
        Glyph::Trace => {
            let pts = vec![pos2(r.min.x + 1.0, r.max.y - 4.0), pos2(r.min.x + 4.0, r.min.y + 3.0), pos2(r.max.x - 2.0, r.min.y + 6.0), pos2(r.max.x - 4.0, r.max.y - 1.0)];
            p.add(Shape::convex_polygon(pts.clone(), t.subcat.0, Stroke::new(1.5, t.subcat.1)));
            for q in pts {
                p.rect_filled(Rect::from_center_size(q, vec2(3.0, 3.0)), 0.0, ink);
            }
        }
        Glyph::Measure => {
            let bar = Rect::from_center_size(c, vec2(s * 0.95, s * 0.38));
            p.rect_filled(bar, 0.0, Color32::from_rgb(250, 215, 120));
            p.rect_stroke(bar, 0.0, thin, egui::StrokeKind::Inside);
            for k in 0..6 {
                let x = bar.min.x + 2.0 + k as f32 * (bar.width() - 4.0) / 5.0;
                let h = if k % 2 == 0 { s * 0.2 } else { s * 0.12 };
                p.line_segment([pos2(x, bar.min.y), pos2(x, bar.min.y + h)], Stroke::new(1.0, ink));
            }
        }
        Glyph::Pointer => {
            let pts = vec![r.min + vec2(4.0, 1.0), r.min + vec2(4.0, s * 0.85), r.min + vec2(s * 0.42, s * 0.62), r.min + vec2(s * 0.75, s * 0.62)];
            p.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::new(1.2, ink)));
        }
        Glyph::Undo | Glyph::Redo => {
            let dir = if g == Glyph::Undo { -1.0 } else { 1.0 };
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.15 + 0.85 * i as f32 / 10.0);
                    c + vec2(dir * -a.cos() * s * 0.32, -a.sin() * s * 0.32 + s * 0.15)
                })
                .collect();
            let tip = pts[0];
            p.add(Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(30, 90, 200))));
            p.add(Shape::convex_polygon(vec![tip + vec2(0.0, -4.0), tip + vec2(dir * 5.0, 1.0), tip + vec2(-dir * 3.0, 3.0)], Color32::from_rgb(30, 90, 200), Stroke::NONE));
        }
        Glyph::Open => {
            let back = Rect::from_min_max(pos2(r.min.x, r.min.y + s * 0.2), pos2(r.max.x - s * 0.1, r.max.y - s * 0.1));
            p.rect_filled(back, 1.0, Color32::from_rgb(230, 190, 80));
            p.rect_stroke(back, 1.0, thin, egui::StrokeKind::Inside);
            let front = vec![pos2(r.min.x + s * 0.15, c.y), pos2(r.max.x, c.y), pos2(r.max.x - s * 0.15, back.max.y), pos2(r.min.x, back.max.y)];
            p.add(Shape::convex_polygon(front, Color32::from_rgb(250, 215, 120), thin));
        }
        Glyph::Save => {
            p.rect_filled(r, 1.0, Color32::from_rgb(40, 70, 160));
            p.rect_filled(Rect::from_min_size(r.min + vec2(s * 0.2, 1.0), vec2(s * 0.6, s * 0.35)), 0.0, Color32::WHITE);
            p.rect_filled(Rect::from_min_size(pos2(r.min.x + s * 0.25, r.max.y - s * 0.38), vec2(s * 0.5, s * 0.36)), 0.0, Color32::from_rgb(180, 180, 190));
        }
        Glyph::New => {
            let page = vec![r.min + vec2(s * 0.15, 0.0), pos2(r.max.x - s * 0.3, r.min.y), pos2(r.max.x - s * 0.1, r.min.y + s * 0.2), pos2(r.max.x - s * 0.1, r.max.y), pos2(r.min.x + s * 0.15, r.max.y)];
            p.add(Shape::convex_polygon(page, Color32::WHITE, thin));
        }
    }
}
