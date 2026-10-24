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
            let q = pos2(c.x + r * a.cos(), c.y + r * a.sin());
            // Corners meet when the radius is half a side (pills) or zero; a repeated point
            // would make a zero-length segment that tears the stroke join.
            if pts.last().is_none_or(|l: &Pos2| (*l - q).length() > 0.01) {
                pts.push(q);
            }
        }
    }
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (*a - *b).length() <= 0.01) {
        pts.pop();
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
    /// Rails and bars over the backdrop: translucent, thick glass.
    Rail,
    /// Floating docks and tiles: thicker, with a larger drop shadow.
    Float,
    /// An idle key (tool buttons): a thin, faint slab.
    Key,
    /// A hovered control: lifted a little higher.
    Hover,
    /// A selected / pressed control: violet-tinted and pushed down into its edge.
    Selected,
}

/// Paints a raised slab of glass. Light comes from the top-left: there is a soft drop shadow
/// under the slab, its thickness shows as a darker lower edge with a rainbow (dispersion) line
/// along the bottom, and the face has a vertical gradient, a sheen, a bright bevel on its upper
/// inner edge, a shaded lower inner edge, a specular glint and an iridescent rim.
pub fn surface(p: &Painter, rect: Rect, radius: f32, kind: Surface, t: &Tokens) {
    // (face top, face bottom, rim strength, sheen, thickness, shadow alpha)
    let (top, bottom, rim, sheen, depth, shadow) = match kind {
        Surface::Rail => (alpha(t.light, 0.70), alpha(t.face, 0.66), 0.32, 0.14, 3.0, 0.45),
        Surface::Float => (alpha(t.light, 0.90), alpha(t.face, 0.84), 0.60, 0.18, 4.0, 0.55),
        Surface::Key => (white(0.07), white(0.02), 0.18, 0.10, 2.0, 0.30),
        Surface::Hover => (white(0.16), white(0.05), 0.50, 0.18, 3.0, 0.45),
        Surface::Selected => (alpha(IRIS[2], 0.42), alpha(IRIS[1], 0.18), 0.95, 0.22, 1.0, 0.35),
    };
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    // A pressed key sits lower, so its face moves down into the (now thinner) edge.
    let face = if kind == Surface::Selected { rect.translate(vec2(0.0, 1.0)) } else { rect };
    let blur = (6.0 + depth * 4.0) as u8;
    let drop = egui::epaint::Shadow { offset: [0, (depth * 2.0) as i8], blur, spread: 0, color: Color32::from_black_alpha((shadow * 255.0) as u8) };
    p.add(drop.as_shape(face, r));
    // Thickness: the slab's side, seen below the face.
    let side = rounded(face.translate(vec2(0.0, depth)), r);
    let side_color = if kind == Surface::Selected { Color32::from_rgb(52, 36, 120) } else { Color32::from_rgb(6, 8, 18) };
    gradient_fill(p, &side, alpha(side_color, 0.9), alpha(Color32::BLACK, 0.9));
    let lower: Vec<Pos2> = side.iter().copied().filter(|q| q.y > face.center().y + depth).collect();
    p.add(Shape::Path(PathShape::line(lower, iris_stroke(1.0, rim * 0.8))));
    // Face.
    let pts = rounded(face, r);
    gradient_fill(p, &pts, top, bottom);
    let cut = face.min.y + face.height() * 0.48;
    let sheen_pts: Vec<Pos2> = pts.iter().map(|q| pos2(q.x, q.y.min(cut))).collect();
    gradient_fill(p, &sheen_pts, white(sheen), white(0.0));
    bevel(p, face, r, 0.45, 0.4);
    glint(p, face, 0.5);
    p.add(Shape::Path(PathShape::closed_line(pts, iris_stroke(1.0, rim))));
}

/// The inner bevel of a rounded face: a bright line along the upper inner edge fading down the
/// sides, and a shaded line along the lower inner edge.
pub fn bevel(p: &Painter, rect: Rect, radius: f32, light: f32, dark: f32) {
    let inner = rect.shrink(1.0);
    let pts = rounded(inner, (radius - 1.0).max(0.0));
    let (y0, y1) = (inner.min.y, inner.max.y);
    let h = (y1 - y0).max(1.0);
    let upper: Vec<Pos2> = pts.iter().copied().filter(|q| q.y <= inner.center().y).collect();
    let lower: Vec<Pos2> = pts.iter().copied().filter(|q| q.y >= inner.center().y).collect();
    let upper = reorder_open(upper);
    let lower = reorder_open(lower);
    p.add(Shape::Path(PathShape::line(upper, PathStroke::new_uv(1.0, move |_, q| white(light * (1.0 - ((q.y - y0) / (h * 0.5)).clamp(0.0, 1.0)))))));
    p.add(Shape::Path(PathShape::line(
        lower,
        PathStroke::new_uv(1.0, move |_, q| alpha(Color32::BLACK, dark * ((q.y - y0) / h - 0.5).clamp(0.0, 0.5) * 2.0)),
    )));
}

/// Points taken from a closed outline in order may wrap around its start; rotate them so the
/// open polyline runs left to right without a jump across the shape.
fn reorder_open(pts: Vec<Pos2>) -> Vec<Pos2> {
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    // The jump is the longest gap between consecutive points.
    let (mut cut, mut best) = (0, 0.0f32);
    for i in 0..n {
        let d = (pts[(i + 1) % n] - pts[i]).length();
        if d > best {
            best = d;
            cut = (i + 1) % n;
        }
    }
    let mut out = pts[cut..].to_vec();
    out.extend_from_slice(&pts[..cut]);
    out
}

/// The visible thickness under an already-drawn rounded face: a strip `depth` deep following
/// its lower outline, with a rainbow line along the bottom. Safe to draw after the face, since
/// it lies entirely below it.
pub fn lip(p: &Painter, rect: Rect, radius: f32, depth: f32) {
    let lower = reorder_open(rounded(rect, radius).into_iter().filter(|q| q.y >= rect.center().y).collect());
    let mut mesh = Mesh::default();
    let (top, bottom) = (alpha(Color32::from_rgb(10, 12, 26), 0.95), alpha(Color32::BLACK, 0.95));
    for (i, q) in lower.iter().enumerate() {
        mesh.colored_vertex(*q, top);
        mesh.colored_vertex(*q + vec2(0.0, depth), bottom);
        if i > 0 {
            let k = (2 * i) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    let base: Vec<Pos2> = lower.iter().map(|q| *q + vec2(0.0, depth)).collect();
    p.add(Shape::Path(PathShape::line(base, iris_stroke(1.0, 0.5))));
}

/// An engraved line: a shadowed groove with a lit edge beside it.
pub fn groove(p: &Painter, a: Pos2, b: Pos2) {
    let d = (b - a).normalized();
    let off = vec2(d.y.abs(), d.x.abs()) * 0.9;
    p.line_segment([a + off, b + off], egui::Stroke::new(1.0, white(0.12)));
    p.line_segment([a, b], egui::Stroke::new(1.0, alpha(Color32::BLACK, 0.6)));
}

/// A specular glint: a soft, flattened white highlight near the top-left of a face.
pub fn glint(p: &Painter, rect: Rect, strength: f32) {
    let rx = (rect.width() * 0.22).min(70.0);
    let ry = (rect.height() * 0.16).clamp(1.5, 7.0);
    let c = pos2(rect.min.x + rect.width() * 0.08 + rx * 0.6, rect.min.y + ry + 1.5);
    glow_ellipse(p, c, vec2(rx, ry), white(0.35 * strength));
}

/// A sunken well (text fields, tables, the canvas): a dark floor, an inner shadow falling from
/// the top edge, and a faint bright lip along the bottom where light catches the far edge.
pub fn well(p: &Painter, rect: Rect, radius: f32, floor: Color32) {
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    let pts = rounded(rect, r);
    gradient_fill(p, &pts, lerp_rgba(floor, Color32::BLACK, 0.35), floor);
    recess(p, rect, r);
}

/// The edges of a recess, drawn over whatever fills it: inner shadow at the top, a soft band
/// below it, a bright lip at the bottom and a faint outline.
pub fn recess(p: &Painter, rect: Rect, radius: f32) {
    let band = (rect.height() * 0.3).min(8.0);
    let pts = rounded(rect, radius);
    let top: Vec<Pos2> = pts.iter().map(|q| pos2(q.x, q.y.min(rect.min.y + band))).collect();
    gradient_fill(p, &top, alpha(Color32::BLACK, 0.35), Color32::TRANSPARENT);
    let inner = rect.shrink(0.5);
    let ip = rounded(inner, radius);
    let (y0, h) = (inner.min.y, inner.height().max(1.0));
    p.add(Shape::Path(PathShape::closed_line(
        ip,
        PathStroke::new_uv(1.0, move |_, q| {
            let f = ((q.y - y0) / h).clamp(0.0, 1.0);
            // Dark along the top (shadowed), light along the bottom (lit lip).
            if f < 0.5 { alpha(Color32::BLACK, 0.55 * (1.0 - f * 2.0) + 0.1) } else { white(0.16 * (f - 0.5) * 2.0 + 0.03) }
        }),
    )));
}

/// A glass bead (checked boxes, radio dots, status lights): a sphere of colour `c` with a
/// darker underside, a drop shadow, a white specular highlight and a rim.
pub fn bead(p: &Painter, c: Pos2, r: f32, color: Color32) {
    glow(p, c + vec2(0.0, r * 0.5), r * 1.6, alpha(Color32::BLACK, 0.45));
    let pts = arc_points(c, r, 28);
    gradient_fill(p, &pts, crate::widgets::lerp_color(color, Color32::WHITE, 0.25), crate::widgets::lerp_color(color, Color32::BLACK, 0.45));
    glow_ellipse(p, c + vec2(-r * 0.25, -r * 0.4), vec2(r * 0.55, r * 0.35), white(0.75));
    p.circle_stroke(c, r, egui::Stroke::new(1.0, lerp_rgba(color, white(1.0), 0.3)));
}

fn arc_points(c: Pos2, r: f32, n: usize) -> Vec<Pos2> {
    (0..n).map(|k| std::f32::consts::TAU * k as f32 / n as f32).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

/// A round sunken socket (unchecked radio buttons).
pub fn socket(p: &Painter, c: Pos2, r: f32, floor: Color32) {
    p.circle_filled(c, r, floor);
    let top = arc_points(c, r - 0.5, 28);
    let (y0, h) = (c.y - r, 2.0 * r);
    p.add(Shape::Path(PathShape::closed_line(
        top,
        PathStroke::new_uv(1.2, move |_, q| {
            let f = ((q.y - y0) / h).clamp(0.0, 1.0);
            if f < 0.5 { alpha(Color32::BLACK, 0.6 * (1.0 - f * 2.0) + 0.1) } else { white(0.22 * (f - 0.5) * 2.0 + 0.04) }
        }),
    )));
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
        // A pill: the corner arcs meet on the short sides, and no point is repeated.
        assert_eq!(pts.len(), 26);
        for (i, q) in pts.iter().enumerate() {
            assert!((pts[(i + 1) % pts.len()] - *q).length() > 0.01, "repeated point at {i}");
        }
    }
}
