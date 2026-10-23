//! Painting helpers for the Iridescent Glass theme. egui has no backdrop blur, so glass is
//! emulated: a colourful "aurora" backdrop behind translucent surfaces, a vertical white sheen,
//! a bright top edge, and a thin iridescent rim whose hue runs cyan → blue → violet → rose →
//! amber along the 145° diagonal (brighter at the top-left, where the light comes from).
//! Design notes and sources: `reference_images/research/glass/NOTES.md` (gitignored).

use crate::theme::{Tokens, white};
use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, pos2, vec2};

/// Iridescent stops (cyan, blue, violet, rose, amber).
pub const IRIS: [Color32; 5] = [
    Color32::from_rgb(94, 231, 255),
    Color32::from_rgb(96, 165, 250),
    Color32::from_rgb(167, 139, 250),
    Color32::from_rgb(244, 114, 182),
    Color32::from_rgb(253, 211, 130),
];

/// Colour at `f` (0..1) along the iridescent gradient.
pub fn iris(f: f32) -> Color32 {
    let x = f.clamp(0.0, 1.0) * (IRIS.len() - 1) as f32;
    let i = (x.floor() as usize).min(IRIS.len() - 2);
    let (a, b) = (IRIS[i], IRIS[i + 1]);
    crate::widgets::lerp_color(a, b, x - i as f32)
}

/// `c` at `alpha` opacity (0..1).
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Position of `p` along the 145° diagonal of `r` (0 at the top-left, 1 at the bottom-right).
fn diagonal(r: Rect, p: Pos2) -> f32 {
    let d = vec2(r.width().max(1.0), r.height().max(1.0));
    let q = p - r.min;
    ((q.x / d.x) * 0.55 + (q.y / d.y) * 0.45).clamp(0.0, 1.0)
}

/// A stroke coloured by the iridescent gradient over its own bounding box, `strength` (0..1)
/// scaling the opacity; the top-left (specular) end is the brightest.
pub fn iris_stroke(width: f32, strength: f32) -> PathStroke {
    PathStroke::new_uv(width, move |r, p| {
        let f = diagonal(r, p);
        alpha(iris(f), strength * (1.0 - 0.45 * f))
    })
}

/// Outline points of a rounded rectangle (clockwise from the top-left corner).
pub fn rounded(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let corners = [
        (pos2(rect.min.x + r, rect.min.y + r), std::f32::consts::PI),
        (pos2(rect.max.x - r, rect.min.y + r), 1.5 * std::f32::consts::PI),
        (pos2(rect.max.x - r, rect.max.y - r), 0.0),
        (pos2(rect.min.x + r, rect.max.y - r), 0.5 * std::f32::consts::PI),
    ];
    let n = if r < 0.5 { 1 } else { 6 };
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for (c, start) in corners {
        for k in 0..=n {
            let a = start + 0.5 * std::f32::consts::PI * k as f32 / n as f32;
            pts.push(pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// Fills a convex outline with a vertical gradient from `top` to `bottom` (a fan from the
/// centroid, so `pts` must be convex).
pub fn gradient_fill(p: &Painter, pts: &[Pos2], top: Color32, bottom: Color32) {
    if pts.len() < 3 {
        return;
    }
    let (y0, y1) = pts.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), q| (a.min(q.y), b.max(q.y)));
    let h = (y1 - y0).max(1.0);
    let col = |y: f32| lerp_rgba(top, bottom, (y - y0) / h);
    let c = pts.iter().fold(vec2(0.0, 0.0), |s, q| s + q.to_vec2()) / pts.len() as f32;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c.to_pos2(), col(c.y));
    for q in pts {
        mesh.colored_vertex(*q, col(q.y));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(mesh));
}

/// Linear blend of two (premultiplied) colours including alpha.
pub fn lerp_rgba(a: Color32, b: Color32, f: f32) -> Color32 {
    let f = f.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// How prominent a glass surface is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    /// Rails and bars over the backdrop: translucent.
    Rail,
    /// Floating docks and tiles: translucent with a stronger rim and a drop shadow.
    Float,
    /// A hovered control.
    Hover,
    /// A selected / pressed control: violet-tinted with a bright rim.
    Selected,
}

/// Paints a glass surface: optional shadow, translucent sheen, top highlight and rim.
pub fn surface(p: &Painter, rect: Rect, radius: f32, kind: Surface, t: &Tokens) {
    let (top, bottom, rim, edge) = match kind {
        Surface::Rail => (alpha(t.face, 0.78), alpha(t.face, 0.62), 0.28, 0.10),
        Surface::Float => (alpha(t.light, 0.86), alpha(t.face, 0.80), 0.55, 0.16),
        Surface::Hover => (white(0.12), white(0.04), 0.45, 0.14),
        Surface::Selected => (alpha(IRIS[2], 0.36), alpha(IRIS[1], 0.14), 0.95, 0.22),
    };
    if kind == Surface::Float {
        let shadow = egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(120) };
        p.add(shadow.as_shape(rect, radius));
    }
    let pts = rounded(rect, radius);
    gradient_fill(p, &pts, top, bottom);
    // Sheen: the upper part catches more light.
    let sheen = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + rect.height() * 0.45));
    let sheen_pts: Vec<Pos2> = pts.iter().map(|q| pos2(q.x, q.y.min(sheen.max.y))).collect();
    gradient_fill(p, &sheen_pts, white(edge * 0.5), white(0.0));
    p.add(Shape::Path(PathShape::closed_line(pts, iris_stroke(1.0, rim))));
    // A crisp top edge.
    let r = radius.min(rect.height() / 2.0);
    p.line_segment([pos2(rect.min.x + r, rect.min.y + 0.75), pos2(rect.max.x - r, rect.min.y + 0.75)], egui::Stroke::new(1.0, white(edge)));
}

/// A soft round glow (radial gradient from `c` to transparent).
pub fn glow(p: &Painter, center: Pos2, radius: f32, c: Color32) {
    let n = 40;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, c);
    for k in 0..n {
        let a = std::f32::consts::TAU * k as f32 / n as f32;
        mesh.colored_vertex(center + vec2(a.cos(), a.sin()) * radius, Color32::TRANSPARENT);
    }
    for k in 0..n {
        mesh.add_triangle(0, 1 + k, 1 + (k + 1) % n);
    }
    p.add(Shape::mesh(mesh));
}

/// An elliptical glow (for the backdrop's aurora bands).
fn glow_ellipse(p: &Painter, center: Pos2, radii: egui::Vec2, c: Color32) {
    let n = 48;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, c);
    // A mid ring keeps the falloff soft instead of linear.
    for k in 0..n {
        let a = std::f32::consts::TAU * k as f32 / n as f32;
        mesh.colored_vertex(center + vec2(a.cos() * radii.x, a.sin() * radii.y) * 0.5, lerp_rgba(c, Color32::TRANSPARENT, 0.6));
    }
    for k in 0..n {
        let a = std::f32::consts::TAU * k as f32 / n as f32;
        mesh.colored_vertex(center + vec2(a.cos() * radii.x, a.sin() * radii.y), Color32::TRANSPARENT);
    }
    for k in 0..n {
        let (a, b) = (1 + k, 1 + (k + 1) % n);
        mesh.add_triangle(0, a, b);
        mesh.add_triangle(a, b, a + n);
        mesh.add_triangle(b, b + n, a + n);
    }
    p.add(Shape::mesh(mesh));
}

/// The application backdrop: deep navy with violet, cyan and rose aurora glows, so the
/// translucent rails and docks above it read as glass.
pub fn backdrop(p: &Painter, rect: Rect) {
    p.rect_filled(rect, 0.0, Color32::from_rgb(5, 7, 16));
    let (w, h) = (rect.width(), rect.height());
    let at = |fx: f32, fy: f32| pos2(rect.min.x + w * fx, rect.min.y + h * fy);
    glow_ellipse(p, at(0.08, 0.05), vec2(w * 0.45, h * 0.55), alpha(IRIS[2], 0.30));
    glow_ellipse(p, at(0.95, 0.12), vec2(w * 0.35, h * 0.45), alpha(IRIS[0], 0.20));
    glow_ellipse(p, at(0.80, 1.00), vec2(w * 0.50, h * 0.45), alpha(IRIS[3], 0.18));
    glow_ellipse(p, at(0.15, 0.95), vec2(w * 0.40, h * 0.35), alpha(IRIS[1], 0.18));
}

/// The diagram canvas in the glass theme: the backdrop seen through dark smoked glass.
pub fn canvas(p: &Painter, rect: Rect, t: &Tokens) {
    gradient_fill(p, &rounded(rect, 16.0), lerp_rgba(t.canvas, Color32::from_rgb(20, 24, 48), 0.5), t.canvas);
    let (w, h) = (rect.width(), rect.height());
    glow_ellipse(p, pos2(rect.min.x + w * 0.15, rect.min.y), vec2(w * 0.5, h * 0.6), alpha(IRIS[2], 0.10));
    glow_ellipse(p, pos2(rect.max.x, rect.max.y), vec2(w * 0.5, h * 0.6), alpha(IRIS[0], 0.07));
}

/// A thin horizontal divider that fades out at both ends.
pub fn divider(p: &Painter, a: Pos2, b: Pos2) {
    let mid = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let stroke = |x0: Pos2, x1: Pos2, c0: Color32, c1: Color32| {
        let pts = vec![x0, x1];
        let len = (x1 - x0).length().max(1.0);
        p.add(Shape::Path(PathShape::line(
            pts,
            PathStroke::new_uv(1.0, move |_, q| lerp_rgba(c0, c1, ((q - x0).length() / len).clamp(0.0, 1.0))),
        )));
    };
    stroke(a, mid, white(0.0), white(0.16));
    stroke(mid, b, white(0.16), white(0.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iris_runs_through_its_stops() {
        assert_eq!(iris(0.0), IRIS[0]);
        assert_eq!(iris(1.0), IRIS[4]);
        assert_eq!(iris(-3.0), IRIS[0]);
        assert_eq!(iris(0.5), IRIS[2]);
    }

    #[test]
    fn rounded_rect_stays_inside() {
        let r = Rect::from_min_size(pos2(10.0, 20.0), vec2(100.0, 30.0));
        let pts = rounded(r, 40.0);
        assert!(pts.iter().all(|q| r.expand(0.01).contains(*q)));
        assert_eq!(pts.len(), 28);
    }
}
