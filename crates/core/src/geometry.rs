//! Open-channel and pipe cross-sections, and Manning's equation.
//!
//! All dimensions are in feet. A [`Section`] answers flow area, wetted perimeter and top width
//! for any depth up to its full depth.

use serde::{Deserialize, Serialize};

/// Manning's constant for US customary units (1.486 ft^(1/3)/s).
pub const MANNING_K: f64 = 1.486;
/// Acceleration of gravity, ft/s².
pub const G: f64 = 32.174;

/// Pipe shapes for culverts, pipe reaches, Tc pipe segments and pipe storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PipeShape {
    #[default]
    Round,
    Box,
    /// Horizontal ellipse (width ≥ height) or vertical when height > width.
    Ellipse,
    /// Pipe-arch: flat-ish bottom with a semicircular-ish top (modelled by top/bottom radii).
    Arch,
}

impl PipeShape {
    pub const ALL: [PipeShape; 4] = [PipeShape::Round, PipeShape::Box, PipeShape::Ellipse, PipeShape::Arch];
    pub fn label(self) -> &'static str {
        match self {
            PipeShape::Round => "Round",
            PipeShape::Box => "Box",
            PipeShape::Ellipse => "Ellipse",
            PipeShape::Arch => "Pipe Arch",
        }
    }
}

/// One point of a custom cross-section: horizontal offset and elevation, with the Manning's n
/// of the segment that starts at this point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct XsPoint {
    pub offset: f64,
    pub elev: f64,
    pub n: f64,
}

/// A channel or conduit cross-section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Section {
    /// Rectangular channel: bottom width, bank-full depth.
    Rectangular { width: f64, depth: f64 },
    /// Vee or trapezoidal channel (bottom width zero for a vee); side slopes are run/rise.
    Trapezoidal { bottom: f64, z_left: f64, z_right: f64, depth: f64 },
    /// Parabolic channel given its top width at the bank-full depth.
    Parabolic { width: f64, depth: f64 },
    /// Closed conduit; `fill` is sediment or a raised invert inside the barrel.
    Pipe { shape: PipeShape, width: f64, height: f64, fill: f64 },
    /// Flow area and wetted perimeter entered directly (Tc channel segments only).
    AreaPerimeter { area: f64, perimeter: f64 },
    /// Surveyed cross-section (offset, elevation) points.
    Custom { points: Vec<XsPoint> },
}

impl Section {
    pub fn circular(diameter: f64) -> Section {
        Section::Pipe { shape: PipeShape::Round, width: diameter, height: diameter, fill: 0.0 }
    }

    /// Bank-full (or pipe-full) depth.
    pub fn full_depth(&self) -> f64 {
        match self {
            Section::Rectangular { depth, .. } | Section::Trapezoidal { depth, .. } | Section::Parabolic { depth, .. } => {
                depth.max(0.0)
            }
            Section::Pipe { height, fill, .. } => (height - fill).max(0.0),
            Section::AreaPerimeter { .. } => 1.0,
            Section::Custom { points } => {
                let min = points.iter().map(|p| p.elev).fold(f64::INFINITY, f64::min);
                let left = points.first().map(|p| p.elev).unwrap_or(0.0);
                let right = points.last().map(|p| p.elev).unwrap_or(0.0);
                (left.min(right) - min).max(0.0)
            }
        }
    }

    /// Flow area, wetted perimeter and top width at depth `y`.
    pub fn props(&self, y: f64) -> SectionProps {
        let y = y.max(0.0);
        match self {
            Section::Rectangular { width, .. } => {
                SectionProps { area: width * y, perimeter: if y > 0.0 { width + 2.0 * y } else { 0.0 }, top_width: *width }
            }
            Section::Trapezoidal { bottom, z_left, z_right, .. } => {
                let area = y * (bottom + 0.5 * (z_left + z_right) * y);
                let perim = if y > 0.0 {
                    bottom + y * ((1.0 + z_left * z_left).sqrt() + (1.0 + z_right * z_right).sqrt())
                } else {
                    0.0
                };
                SectionProps { area, perimeter: perim, top_width: bottom + (z_left + z_right) * y }
            }
            Section::Parabolic { width, depth } => parabolic(*width, *depth, y),
            Section::Pipe { shape, width, height, fill } => pipe_props(*shape, *width, *height, *fill, y),
            Section::AreaPerimeter { area, perimeter } => {
                // Scales linearly with depth up to the entered values at depth 1.
                let f = y.min(1.0);
                SectionProps { area: area * f, perimeter: perimeter * f, top_width: if *area > 0.0 { *area } else { 0.0 } }
            }
            Section::Custom { points } => custom_props(points, y),
        }
    }

    /// Manning discharge (cfs) at depth `y` for roughness `n` and slope `s` (ft/ft).
    pub fn manning_q(&self, y: f64, n: f64, s: f64) -> f64 {
        if let Section::Custom { points } = self {
            return custom_manning_q(points, y, s);
        }
        let p = self.props(y);
        manning(p.area, p.perimeter, n, s)
    }

    /// Velocity (ft/s) at depth `y`.
    pub fn velocity(&self, y: f64, n: f64, s: f64) -> f64 {
        let a = self.props(y).area;
        if a > 0.0 { self.manning_q(y, n, s) / a } else { 0.0 }
    }

    /// Composite Manning's n of a custom section (perimeter-weighted), or `n` otherwise.
    pub fn effective_n(&self, n: f64) -> f64 {
        match self {
            Section::Custom { points } => {
                let (mut num, mut den) = (0.0, 0.0);
                for w in points.windows(2) {
                    let len = ((w[1].offset - w[0].offset).powi(2) + (w[1].elev - w[0].elev).powi(2)).sqrt();
                    num += len * w[0].n;
                    den += len;
                }
                if den > 0.0 { num / den } else { n }
            }
            _ => n,
        }
    }

    /// Depth giving discharge `q` (bisection; capped at the full depth for closed shapes).
    pub fn normal_depth(&self, q: f64, n: f64, s: f64) -> f64 {
        if q <= 0.0 {
            return 0.0;
        }
        let full = self.full_depth();
        let mut hi = if full > 0.0 { full } else { 1.0 };
        if !matches!(self, Section::Pipe { .. }) {
            let mut guard = 0;
            while self.manning_q(hi, n, s) < q && guard < 60 {
                hi *= 2.0;
                guard += 1;
            }
        }
        let mut lo = 0.0;
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if self.manning_q(mid, n, s) < q { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }
}

/// Area, wetted perimeter and top width.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SectionProps {
    pub area: f64,
    pub perimeter: f64,
    pub top_width: f64,
}

impl SectionProps {
    pub fn hydraulic_radius(&self) -> f64 {
        if self.perimeter > 0.0 { self.area / self.perimeter } else { 0.0 }
    }
}

/// Manning's equation, Q = (1.486/n) A R^(2/3) S^(1/2).
pub fn manning(area: f64, perimeter: f64, n: f64, s: f64) -> f64 {
    if area <= 0.0 || perimeter <= 0.0 || n <= 0.0 || s <= 0.0 {
        return 0.0;
    }
    MANNING_K / n * area * (area / perimeter).powf(2.0 / 3.0) * s.sqrt()
}

fn parabolic(width: f64, depth: f64, y: f64) -> SectionProps {
    if width <= 0.0 || depth <= 0.0 || y <= 0.0 {
        return SectionProps::default();
    }
    let t = width * (y / depth).sqrt();
    let area = 2.0 / 3.0 * t * y;
    let x = 4.0 * y / t;
    let r = (1.0 + x * x).sqrt();
    let perimeter = 0.5 * t * (r + (x + r).ln() / x);
    SectionProps { area, perimeter, top_width: t }
}

/// Width of a closed conduit at height `z` above its invert.
fn pipe_width_at(shape: PipeShape, w: f64, h: f64, z: f64) -> f64 {
    if z <= 0.0 || z >= h || w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    match shape {
        PipeShape::Box => w,
        PipeShape::Round | PipeShape::Ellipse => {
            let u = 2.0 * z / h - 1.0;
            w * (1.0 - u * u).max(0.0).sqrt()
        }
        PipeShape::Arch => {
            // Lower part: shallow elliptical belly up to 40% of the rise; upper part: half-ellipse crown.
            let zb = 0.4 * h;
            if z <= zb {
                let u = 1.0 - z / zb;
                w * (1.0 - u * u).max(0.0).sqrt()
            } else {
                let u = (z - zb) / (h - zb);
                w * (1.0 - u * u).max(0.0).sqrt()
            }
        }
    }
}

fn pipe_props(shape: PipeShape, w: f64, h: f64, fill: f64, y: f64) -> SectionProps {
    if w <= 0.0 || h <= 0.0 {
        return SectionProps::default();
    }
    let fill = fill.clamp(0.0, h);
    let top = (fill + y).min(h);
    if top <= fill {
        return SectionProps::default();
    }
    match shape {
        PipeShape::Box => {
            let d = top - fill;
            let mut perim = w + 2.0 * d;
            if top >= h - 1e-9 {
                perim += w;
            }
            let top_w = if top >= h - 1e-9 { 0.0 } else { w };
            SectionProps { area: w * d, perimeter: perim, top_width: top_w }
        }
        PipeShape::Round if (w - h).abs() < 1e-9 && fill <= 0.0 => {
            let d = w;
            let r = d / 2.0;
            let yy = top.min(d);
            let theta = 2.0 * (1.0 - yy / r).clamp(-1.0, 1.0).acos();
            let area = r * r / 2.0 * (theta - theta.sin());
            let perimeter = r * theta;
            let top_width = 2.0 * r * (theta / 2.0).sin();
            SectionProps { area, perimeter, top_width: if yy >= d - 1e-9 { 0.0 } else { top_width } }
        }
        _ => {
            // Numerical integration of width and of the boundary length.
            let n = 200;
            let (mut area, mut perim) = (0.0, 0.0);
            let dz = (top - fill) / n as f64;
            let mut last_w = pipe_width_at(shape, w, h, fill);
            for i in 1..=n {
                let z = fill + i as f64 * dz;
                let wz = pipe_width_at(shape, w, h, z.min(h - 1e-12));
                area += 0.5 * (last_w + wz) * dz;
                let dw = 0.5 * (wz - last_w);
                perim += 2.0 * (dz * dz + dw * dw).sqrt();
                last_w = wz;
            }
            perim += pipe_width_at(shape, w, h, fill + 1e-9);
            if top >= h - 1e-9 {
                perim += last_w;
            }
            SectionProps { area, perimeter: perim, top_width: if top >= h - 1e-9 { 0.0 } else { last_w } }
        }
    }
}

fn custom_props(points: &[XsPoint], y: f64) -> SectionProps {
    let Some(min) = points.iter().map(|p| p.elev).reduce(f64::min) else { return SectionProps::default() };
    let wse = min + y;
    let mut out = SectionProps::default();
    for w in points.windows(2) {
        let s = segment_wet(w[0], w[1], wse);
        out.area += s.area;
        out.perimeter += s.perimeter;
        out.top_width += s.top_width;
    }
    out
}

/// Manning's subdivision: each segment conveys with its own n (summed conveyance).
fn custom_manning_q(points: &[XsPoint], y: f64, s: f64) -> f64 {
    let Some(min) = points.iter().map(|p| p.elev).reduce(f64::min) else { return 0.0 };
    let wse = min + y;
    points.windows(2).map(|w| {
        let p = segment_wet(w[0], w[1], wse);
        manning(p.area, p.perimeter, w[0].n, s)
    }).sum()
}

fn segment_wet(a: XsPoint, b: XsPoint, wse: f64) -> SectionProps {
    let (da, db) = (wse - a.elev, wse - b.elev);
    let dx = (b.offset - a.offset).abs();
    if da <= 0.0 && db <= 0.0 {
        return SectionProps::default();
    }
    let full_len = (dx * dx + (b.elev - a.elev).powi(2)).sqrt();
    if da >= 0.0 && db >= 0.0 {
        return SectionProps { area: 0.5 * (da + db) * dx, perimeter: full_len, top_width: dx };
    }
    // Partially wet: the wet fraction along the segment.
    let wet_depth = da.max(db);
    let frac = wet_depth / (da - db).abs();
    SectionProps { area: 0.5 * wet_depth * dx * frac, perimeter: full_len * frac, top_width: dx * frac }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parabolic_matches_reference_tc_segment() {
        // W=4.00' D=1.50' Area=4.0 sf Perim=5.2' n=0.025 s=0.0125 -> 22.32 cfs, 5.6 fps.
        let s = Section::Parabolic { width: 4.0, depth: 1.5 };
        let p = s.props(1.5);
        assert!((p.area - 4.0).abs() < 1e-9);
        assert!((p.perimeter - 5.199).abs() < 0.002);
        let q = s.manning_q(1.5, 0.025, 0.0125);
        assert!((q - 22.32).abs() < 0.01, "{q}");
    }

    #[test]
    fn round_pipe_matches_reference_tc_segment() {
        // Diam=24" n=0.013 s=0.01 -> 22.62 cfs, 7.2 fps.
        let s = Section::circular(2.0);
        let q = s.manning_q(2.0, 0.013, 0.01);
        assert!((q - 22.62).abs() < 0.01, "{q}");
        assert!((s.velocity(2.0, 0.013, 0.01) - 7.2).abs() < 0.05);
        // Half full: area is half of full.
        assert!((s.props(1.0).area - std::f64::consts::PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn integrated_shapes_are_consistent() {
        let ellipse = Section::Pipe { shape: PipeShape::Ellipse, width: 2.0, height: 2.0, fill: 0.0 };
        let round = Section::circular(2.0);
        // An ellipse with equal axes is a circle (integrated vs closed form).
        assert!((ellipse.props(1.3).area - round.props(1.3).area).abs() < 2e-3);
        let filled = Section::Pipe { shape: PipeShape::Ellipse, width: 2.0, height: 2.0, fill: 0.5 };
        assert!(filled.props(10.0).area < round.props(10.0).area);
    }

    #[test]
    fn normal_depth_inverts_manning() {
        let s = Section::Trapezoidal { bottom: 4.0, z_left: 2.0, z_right: 2.0, depth: 3.0 };
        let y = s.normal_depth(50.0, 0.03, 0.005);
        assert!((s.manning_q(y, 0.03, 0.005) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn custom_section_subdivision() {
        let pts = vec![
            XsPoint { offset: 0.0, elev: 10.0, n: 0.035 },
            XsPoint { offset: 10.0, elev: 8.0, n: 0.025 },
            XsPoint { offset: 14.0, elev: 8.0, n: 0.035 },
            XsPoint { offset: 24.0, elev: 10.0, n: 0.035 },
        ];
        let s = Section::Custom { points: pts };
        assert!((s.full_depth() - 2.0).abs() < 1e-12);
        let p = s.props(2.0);
        assert!((p.area - (4.0 * 2.0 + 2.0 * 10.0)).abs() < 1e-9);
        assert!(s.manning_q(2.0, 0.03, 0.01) > 0.0);
        assert_eq!(Section::Custom { points: vec![] }.manning_q(1.0, 0.03, 0.01), 0.0);
    }
}
