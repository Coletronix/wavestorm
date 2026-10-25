//! Cross-section drawing shared by the reach edit dialog and the reach report: channel or pipe
//! outline in `ink`, with the water surface filled up to a given depth.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use wavestorm_core::geometry::Section;

/// Whether `section` has geometry that can be drawn (rating tables and area/perimeter
/// sections have none).
pub fn drawable(section: &Section) -> bool {
    match section {
        Section::Rating { .. } | Section::AreaPerimeter { .. } => false,
        Section::Custom { points } => points.len() >= 2,
        _ => section.full_depth() > 1e-9,
    }
}

/// Draws `section` fitted into `rect` (aspect ratio kept). `depth` is the water depth above the
/// invert (None or zero draws a dry section). `stroke_w` is the outline width.
pub fn draw(
    painter: &Painter,
    rect: Rect,
    section: &Section,
    depth: Option<f64>,
    ink: Color32,
    water: Color32,
    stroke_w: f32,
) {
    if !drawable(section) {
        return;
    }
    let full = section.full_depth().max(0.01);
    let is_pipe = matches!(section, Section::Pipe { .. });
    let is_custom = matches!(section, Section::Custom { .. });
    let depth = depth.filter(|d| d.is_finite() && *d > 0.0).unwrap_or(0.0);
    // Open channels may overflow their banks; pipes and custom sections clip at the top.
    let wl = if is_pipe || is_custom { depth.min(full) } else { depth.min(full * 1.25) };

    // Outline in (offset, elevation above the invert) coordinates.
    let outline: Vec<(f64, f64)> = match section {
        Section::Custom { points } => {
            let min = points.iter().map(|p| p.elev).fold(f64::INFINITY, f64::min);
            points.iter().map(|p| (p.offset, p.elev - min)).collect()
        }
        _ => {
            let n = 48;
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for i in 0..=n {
                let y = full * i as f64 / n as f64;
                let w = if i == n {
                    section.props(y * 0.999).top_width.max(section.props(y).top_width)
                } else {
                    section.props(y).top_width
                };
                left.push((-w / 2.0, y));
                right.push((w / 2.0, y));
            }
            left.reverse();
            left.into_iter().chain(right).collect()
        }
    };
    if outline.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
        return;
    }
    let (minx, maxx) = outline.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let top = outline.iter().map(|p| p.1).fold(0.0, f64::max).max(wl);
    let pad = 6.0;
    let sx = (rect.width() as f64 - 2.0 * pad) / (maxx - minx).max(1e-6);
    let sy = (rect.height() as f64 - 2.0 * pad) / top.max(1e-6);
    let s = sx.min(sy);
    if !(s.is_finite() && s > 0.0) {
        return;
    }
    let cx = rect.center().x as f64 - (minx + maxx) / 2.0 * s;
    let base = rect.center().y as f64 + top * s / 2.0;
    let to = |p: (f64, f64)| Pos2::new((cx + p.0 * s) as f32, (base - p.1 * s) as f32);

    if wl > 0.0 {
        let fill = |pts: Vec<Pos2>, painter: &Painter| {
            if pts.len() >= 3 {
                painter.add(Shape::convex_polygon(pts, water, Stroke::NONE));
            }
        };
        match section {
            Section::Custom { .. } => {
                // One convex piece per bed segment, between the segment and the water level.
                for w in outline.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if a.1 >= wl && b.1 >= wl {
                        continue;
                    }
                    let mut poly = vec![to((a.0, wl)), to((b.0, wl))];
                    if (a.1 < wl) != (b.1 < wl) {
                        // The segment crosses the water level.
                        let t = (wl - a.1) / (b.1 - a.1);
                        let xc = a.0 + t * (b.0 - a.0);
                        poly = if a.1 < wl {
                            vec![to((a.0, wl)), to((xc, wl)), to(a)]
                        } else {
                            vec![to((xc, wl)), to((b.0, wl)), to(b)]
                        };
                    } else {
                        poly.push(to(b));
                        poly.push(to(a));
                    }
                    fill(poly, painter);
                }
            }
            _ => {
                let ys = 40;
                let (mut l, mut r) = (Vec::new(), Vec::new());
                for i in 0..=ys {
                    let y = wl * i as f64 / ys as f64;
                    let hw = section.props(y.min(full * 0.999)).top_width / 2.0;
                    l.push(to((-hw, y)));
                    r.push(to((hw, y)));
                }
                r.reverse();
                l.extend(r);
                fill(l, painter);
            }
        }
    }
    let line: Vec<Pos2> = outline.iter().map(|p| to(*p)).collect();
    if is_pipe {
        painter.add(Shape::closed_line(line, Stroke::new(stroke_w, ink)));
    } else {
        painter.add(Shape::line(line, Stroke::new(stroke_w, ink)));
        // Water surface line above the banks of a flooded channel is part of the fill; nothing more.
    }
}
