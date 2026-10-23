//! Node symbols and toolbar glyphs, drawn with the painter (no bitmap assets).
//!
//! The classic glyphs are small colour pictures in the style of engineering toolbars; on dark
//! themes their dark inks are lifted ([`ink`]) so they stay visible. The glass theme has its own
//! set ([`draw_glass_glyph`]): line icons stroked with the iridescent gradient over translucent
//! glass bodies, after the clear glass icon references in `reference_images/research/glass/`.

use crate::glass::{self, IRIS};
use crate::theme::{Tokens, white};
use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use wavestorm_core::project::NodeType;

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
    if tok.glass && t != NodeType::Text {
        draw_glass_node(p, t, c, d, selected, problem, tok);
    } else {
        draw_flat_node(p, t, c, d, selected, problem, tok);
    }
    let (_, base_stroke) = node_colors(t, tok);
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

fn draw_flat_node(p: &Painter, t: NodeType, c: Pos2, d: f32, selected: bool, problem: bool, tok: &Tokens) {
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
        link_fill(p, &pts, fill);
        p.add(Shape::closed_line(pts.clone(), Stroke::new(width, stroke)));
    } else {
        p.add(Shape::convex_polygon(pts, fill, Stroke::new(width, stroke)));
    }
}

/// The link banner is concave; fill it as a strip of quads.
fn link_fill(p: &Painter, pts: &[Pos2], fill: Color32) {
    let n = pts.len() / 2;
    for i in 0..n.saturating_sub(1) {
        if let (Some(a), Some(b), Some(c), Some(e)) = (pts.get(i), pts.get(i + 1), pts.get(pts.len() - 2 - i), pts.get(pts.len() - 1 - i)) {
            p.add(Shape::convex_polygon(vec![*a, *b, *c, *e], fill, Stroke::NONE));
        }
    }
}

/// A glass node: a tinted translucent body with a vertical gradient, a sheen over its upper
/// part, a rim in the node colour with an iridescent specular edge, and a soft halo when
/// selected (rose when the node has a problem).
fn draw_glass_node(p: &Painter, t: NodeType, c: Pos2, d: f32, selected: bool, problem: bool, tok: &Tokens) {
    let (_, hue) = node_colors(t, tok);
    let pts = node_outline(t, c, d);
    if problem {
        glass::glow(p, c, d * 0.85, glass::alpha(IRIS[3], 0.45));
        p.add(Shape::closed_line(node_outline(t, c, d * 1.16), Stroke::new((d * 0.04).clamp(1.5, 3.0), IRIS[3])));
    } else if selected {
        glass::glow(p, c, d * 0.9, glass::alpha(hue, 0.38));
    }
    let (top, bottom) = (glass::alpha(hue, 0.38), glass::alpha(hue, 0.08));
    if t == NodeType::Link {
        link_fill(p, &pts, glass::alpha(hue, 0.2));
    } else {
        glass::gradient_fill(p, &pts, top, bottom);
        let cut = c.y - d * 0.08;
        let sheen: Vec<Pos2> = pts.iter().map(|q| pos2(q.x, q.y.min(cut))).collect();
        glass::gradient_fill(p, &sheen, white(0.20), white(0.02));
    }
    let width = if selected { (d * 0.05).clamp(1.8, 3.2) } else { (d * 0.025).clamp(1.0, 1.8) };
    p.add(Shape::closed_line(pts.clone(), Stroke::new(width, if selected { lift(hue) } else { hue })));
    p.add(Shape::Path(PathShape::closed_line(pts, glass::iris_stroke(1.0, if selected { 0.9 } else { 0.55 }))));
}

/// A lighter version of a hue (selected rims).
fn lift(c: Color32) -> Color32 {
    crate::widgets::lerp_color(c, Color32::WHITE, 0.45)
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

/// Relative luminance (sRGB, 0..1).
fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// A glyph ink colour adapted to the theme: unchanged on light themes; on dark themes
/// near-black becomes the text colour and other dark colours are mixed toward white until they
/// read against the dark face (relative luminance ≥ 0.3).
pub fn ink(t: &Tokens, c: Color32) -> Color32 {
    if !t.is_dark() || luminance(c) >= 0.3 {
        return c;
    }
    if luminance(c) < 0.03 {
        return t.text;
    }
    (1..=20).map(|k| crate::widgets::lerp_color(c, Color32::WHITE, k as f32 * 0.05)).find(|m| luminance(*m) >= 0.3).unwrap_or(Color32::WHITE)
}

pub fn draw_glyph(p: &Painter, g: Glyph, r: Rect, t: &Tokens) {
    if t.glass {
        draw_glass_glyph(p, g, r, t);
        return;
    }
    let k = |c: Color32| ink(t, c);
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
            // The lens is always light, so its sign stays dark.
            let sign = if t.is_dark() { Color32::from_rgb(20, 20, 20) } else { ink };
            p.line_segment([lc - vec2(s * 0.17, 0.0), lc + vec2(s * 0.17, 0.0)], Stroke::new(1.8, sign));
            if g == Glyph::ZoomIn {
                p.line_segment([lc - vec2(0.0, s * 0.17), lc + vec2(0.0, s * 0.17)], Stroke::new(1.8, sign));
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
                p.circle_filled(c, s * 0.1, if t.is_dark() { Color32::from_rgb(20, 20, 20) } else { ink });
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
            let x = k(Color32::from_rgb(30, 30, 30));
            p.line_segment([r.left_top() + vec2(3.0, 3.0), r.right_bottom() - vec2(3.0, 3.0)], Stroke::new(3.0, x));
            p.line_segment([r.right_top() + vec2(-3.0, 3.0), r.left_bottom() + vec2(3.0, -3.0)], Stroke::new(3.0, x));
        }
        Glyph::Edit | Glyph::Notes => {
            let page = Rect::from_min_size(r.min + vec2(s * 0.1, 0.0), vec2(s * 0.62, s * 0.9));
            p.rect_filled(page, 0.0, Color32::WHITE);
            p.rect_stroke(page, 0.0, thin, egui::StrokeKind::Inside);
            for j in 0..4 {
                let y = page.min.y + s * (0.2 + 0.17 * j as f32);
                p.line_segment([pos2(page.min.x + 3.0, y), pos2(page.max.x - 3.0, y)], Stroke::new(1.0, if t.is_dark() { Color32::from_rgb(150, 150, 150) } else { t.shadow }));
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
            for j in 0..4 {
                let y = r.min.y + 3.0 + j as f32 * (r.height() - 6.0) / 3.0;
                p.line_segment([pos2(r.min.x + 3.0, y), pos2(r.max.x - 3.0, y)], Stroke::new(1.0, if t.is_dark() { Color32::from_rgb(150, 150, 150) } else { t.shadow }));
                p.rect_filled(Rect::from_min_size(pos2(r.min.x + 3.0, y - 1.0), vec2(3.0, 2.0)), 0.0, Color32::from_rgb(40, 140, 40));
            }
        }
        Glyph::Print | Glyph::PrintDiagram => {
            let body = Rect::from_min_max(pos2(r.min.x, c.y - s * 0.1), pos2(r.max.x, r.max.y - s * 0.12));
            let paper = Rect::from_min_max(pos2(r.min.x + s * 0.2, r.min.y), pos2(r.max.x - s * 0.2, c.y));
            p.rect_filled(paper, 0.0, Color32::WHITE);
            p.rect_stroke(paper, 0.0, thin, egui::StrokeKind::Inside);
            p.rect_filled(body, 2.0, if t.is_dark() { Color32::from_rgb(150, 154, 164) } else { t.shadow });
            p.rect_stroke(body, 2.0, thin, egui::StrokeKind::Inside);
            if g == Glyph::PrintDiagram {
                p.circle_filled(paper.center(), s * 0.08, Color32::from_rgb(40, 150, 40));
            }
        }
        Glyph::Calculator => {
            let body = r.shrink2(vec2(s * 0.15, 0.0));
            p.rect_filled(body, 1.0, k(Color32::from_rgb(90, 90, 100)));
            p.rect_filled(Rect::from_min_size(body.min + vec2(2.0, 2.0), vec2(body.width() - 4.0, s * 0.22)), 0.0, Color32::from_rgb(190, 230, 190));
            for row in 0..3 {
                for col in 0..3 {
                    let kc = pos2(body.min.x + 3.5 + col as f32 * (body.width() - 7.0) / 2.0, body.min.y + s * 0.42 + row as f32 * s * 0.2);
                    p.rect_filled(Rect::from_center_size(kc, vec2(2.5, 2.5)), 0.0, if t.is_dark() { Color32::from_rgb(40, 40, 46) } else { Color32::WHITE });
                }
            }
        }
        Glyph::LandUse => {
            let top = pos2(c.x, r.min.y);
            p.add(Shape::convex_polygon(vec![top, pos2(r.max.x, c.y + s * 0.1), pos2(r.min.x, c.y + s * 0.1)], k(Color32::from_rgb(40, 140, 50)), Stroke::new(1.0, ink)));
            p.line_segment([pos2(c.x, c.y + s * 0.1), pos2(c.x, r.max.y - 2.0)], Stroke::new(2.0, ink));
            p.line_segment([pos2(r.min.x, r.max.y - 1.0), pos2(r.max.x, r.max.y - 1.0)], Stroke::new(2.0, Color32::from_rgb(40, 220, 60)));
        }
        Glyph::DiagramSettings => {
            p.rect_filled(r.shrink2(vec2(s * 0.1, 0.0)), 0.0, Color32::WHITE);
            p.rect_stroke(r.shrink2(vec2(s * 0.1, 0.0)), 0.0, thin, egui::StrokeKind::Inside);
            p.text(c, Align2::CENTER_CENTER, "A", FontId::proportional(s * 0.8), if t.is_dark() { Color32::from_rgb(20, 20, 20) } else { ink });
        }
        Glyph::General => {
            let grey = k(Color32::from_rgb(110, 110, 120));
            p.line_segment([pos2(r.min.x + s * 0.25, r.max.y), pos2(r.min.x + s * 0.25, c.y)], Stroke::new(3.0, grey));
            p.circle_stroke(pos2(r.min.x + s * 0.25, c.y - s * 0.15), s * 0.2, Stroke::new(2.0, grey));
            p.line_segment([pos2(r.max.x - s * 0.2, r.max.y), pos2(r.max.x - s * 0.2, r.min.y + 1.0)], Stroke::new(3.0, ink));
        }
        Glyph::Idf => {
            for (j, col) in [Color32::from_rgb(200, 40, 40), Color32::from_rgb(30, 60, 200), Color32::from_rgb(30, 160, 200)].into_iter().enumerate() {
                let x = r.min.x + 1.0 + j as f32 * s * 0.3;
                let b = Rect::from_min_max(pos2(x, r.min.y + s * (0.1 + 0.15 * j as f32)), pos2(x + s * 0.26, r.max.y));
                p.rect_filled(b, 0.0, k(col));
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
            for j in 1..4 {
                let x = r.min.x + r.width() * j as f32 / 4.0;
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
            for j in 0..6 {
                let x = bar.min.x + 2.0 + j as f32 * (bar.width() - 4.0) / 5.0;
                let h = if j % 2 == 0 { s * 0.2 } else { s * 0.12 };
                p.line_segment([pos2(x, bar.min.y), pos2(x, bar.min.y + h)], Stroke::new(1.0, if t.is_dark() { Color32::from_rgb(20, 20, 20) } else { ink }));
            }
        }
        Glyph::Pointer => {
            let pts = vec![r.min + vec2(4.0, 1.0), r.min + vec2(4.0, s * 0.85), r.min + vec2(s * 0.42, s * 0.62), r.min + vec2(s * 0.75, s * 0.62)];
            p.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::new(1.2, if t.is_dark() { Color32::from_rgb(20, 20, 20) } else { ink })));
        }
        Glyph::Undo | Glyph::Redo => {
            // The arrowhead sits on the left end for Undo (anticlockwise), the right for Redo.
            let dir = if g == Glyph::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.15 + 0.85 * i as f32 / 10.0);
                    c + vec2(dir * -a.cos() * s * 0.32, -a.sin() * s * 0.32 + s * 0.15)
                })
                .collect();
            let tip = pts[0];
            let blue = k(Color32::from_rgb(30, 90, 200));
            p.add(Shape::line(pts, Stroke::new(2.0, blue)));
            p.add(Shape::convex_polygon(vec![tip + vec2(0.0, -4.0), tip + vec2(dir * 5.0, 1.0), tip + vec2(-dir * 3.0, 3.0)], blue, Stroke::NONE));
        }
        Glyph::Open => {
            let back = Rect::from_min_max(pos2(r.min.x, r.min.y + s * 0.2), pos2(r.max.x - s * 0.1, r.max.y - s * 0.1));
            p.rect_filled(back, 1.0, Color32::from_rgb(230, 190, 80));
            p.rect_stroke(back, 1.0, thin, egui::StrokeKind::Inside);
            let front = vec![pos2(r.min.x + s * 0.15, c.y), pos2(r.max.x, c.y), pos2(r.max.x - s * 0.15, back.max.y), pos2(r.min.x, back.max.y)];
            p.add(Shape::convex_polygon(front, Color32::from_rgb(250, 215, 120), thin));
        }
        Glyph::Save => {
            p.rect_filled(r, 1.0, k(Color32::from_rgb(40, 70, 160)));
            p.rect_filled(Rect::from_min_size(r.min + vec2(s * 0.2, 1.0), vec2(s * 0.6, s * 0.35)), 0.0, Color32::WHITE);
            p.rect_filled(Rect::from_min_size(pos2(r.min.x + s * 0.25, r.max.y - s * 0.38), vec2(s * 0.5, s * 0.36)), 0.0, Color32::from_rgb(180, 180, 190));
        }
        Glyph::New => {
            let page = vec![r.min + vec2(s * 0.15, 0.0), pos2(r.max.x - s * 0.3, r.min.y), pos2(r.max.x - s * 0.1, r.min.y + s * 0.2), pos2(r.max.x - s * 0.1, r.max.y), pos2(r.min.x + s * 0.15, r.max.y)];
            p.add(Shape::convex_polygon(page, Color32::WHITE, thin));
        }
    }
}

/// Draws glass line icons inside one icon box: every stroke shares the iridescent gradient over
/// the box (so an icon reads as one piece of glass), closed shapes get a translucent body.
struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    w: f32,
}

impl Pen<'_> {
    fn stroke(&self) -> PathStroke {
        let r = self.r;
        PathStroke::new_uv(self.w, move |_, q| {
            let d = vec2(r.width().max(1.0), r.height().max(1.0));
            let f = (((q.x - r.min.x) / d.x) * 0.55 + ((q.y - r.min.y) / d.y) * 0.45).clamp(0.0, 1.0);
            glass::iris(0.05 + f * 0.8)
        })
    }
    fn line(&self, pts: Vec<Pos2>) {
        self.p.add(Shape::Path(PathShape::line(pts, self.stroke())));
    }
    fn seg(&self, a: Pos2, b: Pos2) {
        self.line(vec![a, b]);
    }
    /// A closed outline with a glass body (`pts` convex).
    fn shape(&self, pts: Vec<Pos2>) {
        glass::gradient_fill(self.p, &pts, white(0.22), white(0.04));
        self.p.add(Shape::Path(PathShape::closed_line(pts, self.stroke())));
    }
    fn outline(&self, pts: Vec<Pos2>) {
        self.p.add(Shape::Path(PathShape::closed_line(pts, self.stroke())));
    }
    fn circle(&self, c: Pos2, rad: f32, body: bool) {
        let pts = arc(c, rad, 0.0, std::f32::consts::TAU, 24);
        if body { self.shape(pts) } else { self.outline(pts) }
    }
    fn rrect(&self, rect: Rect, rad: f32, body: bool) {
        let pts = glass::rounded(rect, rad);
        if body { self.shape(pts) } else { self.outline(pts) }
    }
    fn dot(&self, c: Pos2, rad: f32) {
        self.p.circle_filled(c, rad, glass::iris(((c.x - self.r.min.x) / self.r.width().max(1.0)).clamp(0.0, 1.0)));
    }
    /// A white specular glint.
    fn glint(&self, c: Pos2, rad: f32) {
        self.p.circle_filled(c, rad, white(0.75));
    }
}

/// Points along a circular arc from angle `a0` to `a1` (radians, y down).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

/// The glass theme's icon set (see the module docs).
pub fn draw_glass_glyph(p: &Painter, g: Glyph, r: Rect, t: &Tokens) {
    let s = r.width().min(r.height());
    let r = Rect::from_center_size(r.center(), vec2(s, s));
    let pen = Pen { p, r, w: (s * 0.085).clamp(1.3, 2.2) };
    let c = r.center();
    let at = |fx: f32, fy: f32| pos2(r.min.x + s * fx, r.min.y + s * fy);
    match g {
        Glyph::ZoomIn | Glyph::ZoomOut | Glyph::Find => {
            let lc = at(0.42, 0.42);
            pen.circle(lc, s * 0.3, true);
            pen.seg(at(0.64, 0.64), at(0.92, 0.92));
            match g {
                Glyph::ZoomIn => {
                    pen.seg(lc - vec2(s * 0.14, 0.0), lc + vec2(s * 0.14, 0.0));
                    pen.seg(lc - vec2(0.0, s * 0.14), lc + vec2(0.0, s * 0.14));
                }
                Glyph::ZoomOut => pen.seg(lc - vec2(s * 0.14, 0.0), lc + vec2(s * 0.14, 0.0)),
                _ => pen.dot(lc, s * 0.08),
            }
            pen.glint(lc + vec2(-s * 0.12, -s * 0.12), s * 0.04);
        }
        Glyph::ZoomFit | Glyph::ZoomSelected => {
            // Four frame corners around the view.
            let (a, b, l) = (0.1, 0.9, 0.28);
            for (x, y, dx, dy) in [(a, a, 1.0, 1.0), (b, a, -1.0, 1.0), (b, b, -1.0, -1.0), (a, b, 1.0, -1.0)] {
                pen.line(vec![at(x + dx * l, y), at(x, y), at(x, y + dy * l)]);
            }
            if g == Glyph::ZoomSelected {
                pen.circle(c, s * 0.16, true);
            } else {
                pen.rrect(Rect::from_center_size(c, vec2(s * 0.36, s * 0.36)), 2.5, true);
            }
        }
        Glyph::Delete => {
            pen.seg(at(0.14, 0.24), at(0.86, 0.24));
            pen.line(vec![at(0.38, 0.24), at(0.4, 0.1), at(0.6, 0.1), at(0.62, 0.24)]);
            pen.shape(vec![at(0.22, 0.32), at(0.78, 0.32), at(0.72, 0.92), at(0.28, 0.92)]);
            pen.seg(at(0.42, 0.44), at(0.43, 0.8));
            pen.seg(at(0.58, 0.44), at(0.57, 0.8));
        }
        Glyph::Edit => {
            pen.shape(vec![at(0.64, 0.12), at(0.88, 0.36), at(0.36, 0.88), at(0.12, 0.64)]);
            pen.line(vec![at(0.12, 0.64), at(0.08, 0.92), at(0.36, 0.88)]);
            pen.seg(at(0.56, 0.2), at(0.8, 0.44));
        }
        Glyph::Notes | Glyph::New => {
            pen.shape(vec![at(0.2, 0.06), at(0.62, 0.06), at(0.82, 0.26), at(0.82, 0.94), at(0.2, 0.94)]);
            pen.line(vec![at(0.62, 0.06), at(0.62, 0.26), at(0.82, 0.26)]);
            if g == Glyph::Notes {
                for y in [0.46, 0.62, 0.78] {
                    pen.seg(at(0.32, y), at(0.7, y));
                }
            } else {
                pen.seg(at(0.51, 0.44), at(0.51, 0.8));
                pen.seg(at(0.33, 0.62), at(0.69, 0.62));
            }
        }
        Glyph::Report | Glyph::Compare => {
            pen.rrect(Rect::from_min_max(at(0.06, 0.12), at(0.94, 0.88)), s * 0.14, true);
            let curve = |peak: f32, h: f32, w: f32| -> Vec<Pos2> {
                (0..=14)
                    .map(|i| {
                        let f = i as f32 / 14.0;
                        let y = h * (-(f - peak) * (f - peak) * w).exp();
                        at(0.16 + f * 0.68, 0.76 - y * 0.48)
                    })
                    .collect()
            };
            pen.line(curve(0.35, 1.0, 26.0));
            if g == Glyph::Compare {
                pen.line(curve(0.6, 0.6, 12.0));
            }
        }
        Glyph::ProjectReport => {
            pen.rrect(Rect::from_min_max(at(0.1, 0.06), at(0.9, 0.94)), s * 0.12, true);
            for y in [0.3, 0.5, 0.7] {
                pen.dot(at(0.28, y), s * 0.05);
                pen.seg(at(0.4, y), at(0.74, y));
            }
        }
        Glyph::Print | Glyph::PrintDiagram => {
            pen.line(vec![at(0.26, 0.36), at(0.26, 0.08), at(0.74, 0.08), at(0.74, 0.36)]);
            pen.rrect(Rect::from_min_max(at(0.06, 0.36), at(0.94, 0.74)), s * 0.1, true);
            pen.shape(vec![at(0.26, 0.6), at(0.74, 0.6), at(0.74, 0.94), at(0.26, 0.94)]);
            if g == Glyph::PrintDiagram {
                pen.dot(at(0.4, 0.77), s * 0.05);
                pen.dot(at(0.6, 0.83), s * 0.05);
            } else {
                pen.seg(at(0.36, 0.76), at(0.64, 0.76));
            }
            pen.glint(at(0.8, 0.48), s * 0.035);
        }
        Glyph::Calculator => {
            pen.rrect(Rect::from_min_max(at(0.18, 0.06), at(0.82, 0.94)), s * 0.12, true);
            pen.rrect(Rect::from_min_max(at(0.3, 0.18), at(0.7, 0.34)), 1.5, false);
            for row in 0..3 {
                for col in 0..3 {
                    pen.dot(at(0.33 + col as f32 * 0.17, 0.5 + row as f32 * 0.14), s * 0.04);
                }
            }
        }
        Glyph::Units | Glyph::Measure => {
            // A ruler lying on the diagonal.
            let (a, b, w) = (at(0.08, 0.72), at(0.72, 0.08), s * 0.2);
            let n = (b - a).normalized();
            let side = vec2(n.y, -n.x) * -w;
            pen.shape(vec![a, b, b + side, a + side]);
            for j in 1..6 {
                let q = a + (b - a) * (j as f32 / 6.0);
                let len = if j % 2 == 0 { 0.55 } else { 0.32 };
                pen.seg(q + side, q + side * (1.0 - len));
            }
            if g == Glyph::Measure {
                pen.dot(at(0.18, 0.92), s * 0.05);
            }
        }
        Glyph::LandUse => {
            // A leaf with its midrib and stem.
            let pi = std::f32::consts::PI;
            let mut leaf = arc(at(0.88, 0.8), s * 0.68, pi, 1.5 * pi, 10);
            leaf.extend(arc(at(0.2, 0.12), s * 0.68, 0.0, 0.5 * pi, 10).into_iter().skip(1));
            glass::gradient_fill(p, &leaf, glass::alpha(t.subcat.1, 0.35), white(0.04));
            pen.outline(leaf);
            pen.seg(at(0.2, 0.8), at(0.7, 0.3));
            pen.seg(at(0.2, 0.8), at(0.08, 0.92));
        }
        Glyph::DiagramSettings => {
            let (a, b, d) = (at(0.22, 0.26), at(0.78, 0.36), at(0.46, 0.8));
            pen.seg(a, b);
            pen.seg(b, d);
            pen.seg(a, d);
            for q in [a, b, d] {
                pen.circle(q, s * 0.13, true);
            }
        }
        Glyph::General => {
            // Two sliders.
            for (y, x) in [(0.32, 0.66), (0.68, 0.34)] {
                pen.seg(at(0.08, y), at(0.92, y));
                pen.circle(at(x, y), s * 0.13, true);
            }
        }
        Glyph::Idf => {
            pen.line(vec![at(0.1, 0.06), at(0.1, 0.9), at(0.94, 0.9)]);
            for (k, h) in [(0.0, 0.0), (0.12, 0.0), (0.24, 0.0)] {
                let pts: Vec<Pos2> = (0..=10)
                    .map(|i| {
                        let f = i as f32 / 10.0;
                        at(0.2 + f * 0.7, 0.8 - (0.12 + 0.55 / (1.0 + 6.0 * f)) - k + h)
                    })
                    .collect();
                pen.line(pts);
            }
        }
        Glyph::Help => {
            pen.circle(c, s * 0.42, true);
            let q = arc(at(0.5, 0.4), s * 0.14, std::f32::consts::PI, 2.4 * std::f32::consts::PI, 10);
            let mut path = q;
            path.push(at(0.5, 0.6));
            pen.line(path);
            pen.dot(at(0.5, 0.74), s * 0.05);
        }
        Glyph::Messages => {
            // The speech bubble from the glass references, with an exclamation mark.
            let body = glass::rounded(Rect::from_min_max(at(0.06, 0.1), at(0.94, 0.7)), s * 0.16);
            glass::gradient_fill(p, &body, white(0.22), white(0.04));
            pen.outline(body);
            pen.shape(vec![at(0.28, 0.68), at(0.46, 0.68), at(0.24, 0.92)]);
            pen.seg(at(0.5, 0.22), at(0.5, 0.46));
            pen.dot(at(0.5, 0.58), s * 0.045);
        }
        Glyph::Underlay => {
            pen.rrect(Rect::from_min_max(at(0.06, 0.14), at(0.94, 0.86)), s * 0.12, true);
            pen.line(vec![at(0.12, 0.8), at(0.38, 0.5), at(0.56, 0.68), at(0.68, 0.56), at(0.88, 0.8)]);
            pen.circle(at(0.7, 0.34), s * 0.08, false);
        }
        Glyph::Calibrate => {
            pen.seg(at(0.12, 0.5), at(0.88, 0.5));
            pen.seg(at(0.08, 0.28), at(0.08, 0.72));
            pen.seg(at(0.92, 0.28), at(0.92, 0.72));
            pen.line(vec![at(0.24, 0.38), at(0.12, 0.5), at(0.24, 0.62)]);
            pen.line(vec![at(0.76, 0.38), at(0.88, 0.5), at(0.76, 0.62)]);
        }
        Glyph::Trace => {
            let pts = vec![at(0.12, 0.7), at(0.24, 0.16), at(0.84, 0.3), at(0.74, 0.88)];
            glass::gradient_fill(p, &pts, glass::alpha(t.subcat.1, 0.3), white(0.03));
            pen.outline(pts.clone());
            for q in pts {
                p.rect_filled(Rect::from_center_size(q, vec2(s * 0.14, s * 0.14)), 1.5, t.face);
                p.rect_stroke(Rect::from_center_size(q, vec2(s * 0.14, s * 0.14)), 1.5, Stroke::new(1.0, IRIS[0]), egui::StrokeKind::Inside);
            }
        }
        Glyph::Pointer => {
            // The cursor arrow from the glass references.
            let pts = vec![at(0.2, 0.06), at(0.84, 0.56), at(0.52, 0.6), at(0.34, 0.92)];
            glass::gradient_fill(p, &[pts[0], pts[1], pts[2]], white(0.3), white(0.06));
            glass::gradient_fill(p, &[pts[0], pts[2], pts[3]], white(0.3), white(0.06));
            pen.outline(pts);
            pen.glint(at(0.3, 0.24), s * 0.04);
        }
        Glyph::Undo | Glyph::Redo => {
            // An arc over the top with the arrowhead on its left end (Undo) or right (Redo).
            let dir = if g == Glyph::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = arc(at(0.5, 0.62), s * 0.34, std::f32::consts::PI * 1.05, std::f32::consts::PI * 2.0, 12)
                .into_iter()
                .map(|q| pos2(c.x + (q.x - c.x) * dir, q.y))
                .collect();
            let start = pts.first().copied().unwrap_or(c);
            let end = pts.last().copied().unwrap_or(c);
            pen.line(pts);
            let tip = start + vec2(0.0, s * 0.06);
            pen.line(vec![tip + vec2(-dir * s * 0.16, -s * 0.12), tip, tip + vec2(dir * s * 0.14, -s * 0.16)]);
            pen.dot(end, s * 0.04);
        }
        Glyph::Open => {
            pen.shape(vec![at(0.06, 0.2), at(0.38, 0.2), at(0.48, 0.32), at(0.9, 0.32), at(0.9, 0.84), at(0.06, 0.84)]);
            pen.seg(at(0.06, 0.44), at(0.9, 0.44));
        }
        Glyph::Save => {
            pen.shape(vec![at(0.1, 0.08), at(0.72, 0.08), at(0.92, 0.28), at(0.92, 0.92), at(0.1, 0.92)]);
            pen.rrect(Rect::from_min_max(at(0.28, 0.08), at(0.66, 0.32)), 1.0, false);
            pen.rrect(Rect::from_min_max(at(0.26, 0.56), at(0.76, 0.92)), 1.5, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    #[test]
    fn dark_ink_is_lifted_only_on_dark_themes() {
        let classic = Tokens::for_kind(ThemeKind::Classic);
        let dark = Tokens::for_kind(ThemeKind::ModernDark);
        let navy = Color32::from_rgb(30, 90, 200);
        assert_eq!(ink(&classic, navy), navy);
        assert!(luminance(ink(&dark, navy)) >= 0.3);
        assert_eq!(ink(&dark, Color32::from_rgb(30, 30, 30)), dark.text);
        assert_eq!(ink(&dark, Color32::from_rgb(255, 230, 40)), Color32::from_rgb(255, 230, 40));
    }
}
