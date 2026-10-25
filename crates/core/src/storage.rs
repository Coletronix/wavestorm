//! Pond storage: stage-storage, surface-area and wetted-area relationships.
//!
//! A pond has any number of storage volumes. Volumes add together, can be embedded in one
//! another (a chamber in a stone bed), and can have a void ratio and a multiplier.

use crate::geometry::{PipeShape, Section};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// How custom stage data is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CustomKind {
    /// Surface areas; storage between stages by the average-end-area (prismatic) formula.
    #[default]
    AreaPrismatic,
    /// Surface areas; storage between stages by the conic (frustum) formula.
    AreaConic,
    /// Incremental storage between successive stages.
    IncrementalStorage,
    /// Cumulative storage at each stage.
    CumulativeStorage,
}

impl CustomKind {
    pub const ALL: [CustomKind; 4] = [
        CustomKind::AreaPrismatic,
        CustomKind::AreaConic,
        CustomKind::IncrementalStorage,
        CustomKind::CumulativeStorage,
    ];
    pub fn label(self) -> &'static str {
        match self {
            CustomKind::AreaPrismatic => "Surface Area (Prismatic)",
            CustomKind::AreaConic => "Surface Area (Conic)",
            CustomKind::IncrementalStorage => "Incremental Storage",
            CustomKind::CumulativeStorage => "Cumulative Storage",
        }
    }
}

/// Shape of one storage volume. Dimensions in feet; custom values in sq ft or cu ft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VolumeShape {
    /// Rectangular bottom with sloping (or vertical, z=0) sides.
    Prismatoid { width: f64, length: f64, height: f64, z: f64 },
    /// Circular bottom with sloping (or vertical) sides: cones and cylinders.
    VerticalCone { diameter: f64, height: f64, z: f64 },
    /// A pipe used as storage, level or sloped (rise over length).
    Pipe { shape: PipeShape, width: f64, height: f64, length: f64, slope: f64 },
    /// Flat-bottomed parabolic arch (generic arched chamber).
    Arch { width: f64, height: f64, length: f64 },
    /// Tabulated stage data: elevations (absolute) with areas or storages.
    Custom { kind: CustomKind, elevations: Vec<f64>, values: Vec<f64> },
}

impl VolumeShape {
    pub fn label(&self) -> &'static str {
        match self {
            VolumeShape::Prismatoid { .. } => "Prismatoid",
            VolumeShape::VerticalCone { .. } => "Vertical Cone/Cylinder",
            VolumeShape::Pipe { .. } => "Pipe Storage",
            VolumeShape::Arch { .. } => "Arch Chamber",
            VolumeShape::Custom { kind, .. } => match kind {
                CustomKind::AreaPrismatic => "Custom Stage Data (Prismatic)",
                CustomKind::AreaConic => "Custom Stage Data (Conic)",
                CustomKind::IncrementalStorage | CustomKind::CumulativeStorage => "Custom Stage Data",
            },
        }
    }
}

/// One storage definition on a pond's Storage tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageVolume {
    pub description: String,
    /// Bottom elevation (ignored for custom data, which carries absolute elevations).
    pub invert: f64,
    pub shape: VolumeShape,
    /// Percent of the gross volume available for water (100 for open storage, ~40 for stone).
    pub voids_pct: f64,
    /// Number of identical volumes.
    pub multiplier: f64,
    /// Index of the volume this one is embedded in.
    #[serde(default)]
    pub embedded_in: Option<usize>,
}

impl StorageVolume {
    pub fn new(description: &str, invert: f64, shape: VolumeShape) -> Self {
        StorageVolume {
            description: description.into(),
            invert,
            shape,
            voids_pct: 100.0,
            multiplier: 1.0,
            embedded_in: None,
        }
    }

    /// Lowest elevation of the volume.
    pub fn bottom(&self) -> f64 {
        match &self.shape {
            VolumeShape::Custom { elevations, .. } => elevations.first().copied().unwrap_or(self.invert),
            _ => self.invert,
        }
    }

    /// Highest defined elevation.
    pub fn top(&self) -> f64 {
        match &self.shape {
            VolumeShape::Prismatoid { height, .. }
            | VolumeShape::VerticalCone { height, .. }
            | VolumeShape::Arch { height, .. } => self.invert + height.max(0.0),
            VolumeShape::Pipe { height, length, slope, .. } => {
                self.invert + height.max(0.0) + (slope * length).max(0.0)
            }
            VolumeShape::Custom { elevations, .. } => elevations.last().copied().unwrap_or(self.invert),
        }
    }

    /// Gross geometric volume (cu ft, one unit, no voids) below elevation `e`.
    pub fn gross(&self, e: f64) -> f64 {
        let h = e - self.invert;
        match &self.shape {
            VolumeShape::Prismatoid { width, length, height, z } => {
                let h = h.clamp(0.0, height.max(0.0));
                width * length * h + (width + length) * z * h * h + 4.0 / 3.0 * z * z * h * h * h
            }
            VolumeShape::VerticalCone { diameter, height, z } => {
                let h = h.clamp(0.0, height.max(0.0));
                let r = diameter / 2.0;
                PI * h * (r * r + r * z * h + z * z * h * h / 3.0)
            }
            VolumeShape::Pipe { shape, width, height, length, slope } => {
                pipe_volume(*shape, *width, *height, *length, *slope, h)
            }
            VolumeShape::Arch { width, height, length } => {
                if *height <= 0.0 || h <= 0.0 {
                    return 0.0;
                }
                let y = h.min(*height);
                length * width * (2.0 * height / 3.0) * (1.0 - (1.0 - y / height).powf(1.5))
            }
            VolumeShape::Custom { kind, elevations, values } => custom_volume(*kind, elevations, values, e),
        }
    }

    /// Water surface area (sq ft, one unit) at elevation `e`, when the shape defines one.
    pub fn surface_area(&self, e: f64) -> Option<f64> {
        let h = e - self.invert;
        match &self.shape {
            VolumeShape::Prismatoid { width, length, height, z } => {
                if h < 0.0 {
                    return Some(0.0);
                }
                let h = h.min(*height);
                Some((width + 2.0 * z * h) * (length + 2.0 * z * h))
            }
            VolumeShape::VerticalCone { diameter, height, z } => {
                if h < 0.0 {
                    return Some(0.0);
                }
                let r = diameter / 2.0 + z * h.min(*height);
                Some(PI * r * r)
            }
            VolumeShape::Pipe { shape, width, height, length, slope } => {
                // Numerical derivative of the volume.
                let _ = (shape, width, height, length, slope);
                let d = 1e-3;
                Some(((self.gross(e + d) - self.gross(e - d)) / (2.0 * d)).max(0.0))
            }
            VolumeShape::Arch { width, height, length } => {
                if h < 0.0 || h > *height || *height <= 0.0 {
                    return Some(0.0);
                }
                Some(length * width * (1.0 - h / height).max(0.0).sqrt())
            }
            VolumeShape::Custom { kind, elevations, values } => match kind {
                CustomKind::AreaPrismatic | CustomKind::AreaConic => Some(interp_clamped(elevations, values, e)),
                _ => None,
            },
        }
    }

    /// Wetted (bottom plus side) area below elevation `e`, for exfiltration over wetted area.
    pub fn wetted_area(&self, e: f64) -> Option<f64> {
        let h = e - self.invert;
        match &self.shape {
            VolumeShape::Prismatoid { width, length, height, z } => {
                if h <= 0.0 {
                    return Some(0.0);
                }
                let h = h.min(*height);
                let slant = h * (1.0 + z * z).sqrt();
                // Sides are trapezoids: (bottom edge + top edge)/2 × slant length.
                let side_w = (width + width + 2.0 * z * h) / 2.0 * slant;
                let side_l = (length + length + 2.0 * z * h) / 2.0 * slant;
                Some(width * length + 2.0 * side_w + 2.0 * side_l)
            }
            VolumeShape::VerticalCone { diameter, height, z } => {
                if h <= 0.0 {
                    return Some(0.0);
                }
                let h = h.min(*height);
                let (r0, r1) = (diameter / 2.0, diameter / 2.0 + z * h);
                let slant = (h * h + (r1 - r0).powi(2)).sqrt();
                Some(PI * r0 * r0 + PI * (r0 + r1) * slant)
            }
            VolumeShape::Pipe { shape, width, height, length, slope } if slope.abs() < 1e-12 => {
                let s = Section::Pipe { shape: *shape, width: *width, height: *height, fill: 0.0 };
                Some(s.props(h).perimeter * length)
            }
            VolumeShape::Custom { kind: CustomKind::AreaConic, elevations, values } => {
                Some(conic_wetted(elevations, values, e))
            }
            _ => None,
        }
    }
}

fn interp_clamped(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return 0.0;
    }
    if x <= xs[0] {
        return if x < xs[0] { 0.0 } else { ys[0] };
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = xs[..n].partition_point(|v| *v <= x).saturating_sub(1).min(n - 2);
    let (x0, x1) = (xs[i], xs[i + 1]);
    if x1 <= x0 { ys[i] } else { ys[i] + (ys[i + 1] - ys[i]) * (x - x0) / (x1 - x0) }
}

/// Cumulative storage at each custom stage; storage between stages is interpolated linearly,
/// as stage-storage tables are reported.
fn custom_volume(kind: CustomKind, elev: &[f64], vals: &[f64], e: f64) -> f64 {
    let n = elev.len().min(vals.len());
    if n == 0 || e <= elev[0] {
        return 0.0;
    }
    let mut cum = Vec::with_capacity(n);
    cum.push(match kind {
        CustomKind::CumulativeStorage => vals[0].max(0.0),
        _ => 0.0,
    });
    for i in 1..n {
        let dh = (elev[i] - elev[i - 1]).max(0.0);
        let (a0, a1) = (vals[i - 1].max(0.0), vals[i].max(0.0));
        let prev = cum[i - 1];
        cum.push(match kind {
            CustomKind::AreaPrismatic => prev + dh * (a0 + a1) / 2.0,
            CustomKind::AreaConic => prev + dh / 3.0 * (a0 + a1 + (a0 * a1).sqrt()),
            CustomKind::IncrementalStorage => prev + a1,
            CustomKind::CumulativeStorage => a1.max(prev),
        });
    }
    interp_clamped(&elev[..n], &cum, e)
}

/// Wetted area of a conic frustum stack: bottom area plus lateral surface of each frustum.
fn conic_wetted(elev: &[f64], areas: &[f64], e: f64) -> f64 {
    let n = elev.len().min(areas.len());
    if n == 0 || e <= elev[0] {
        return 0.0;
    }
    let mut total = areas[0].max(0.0);
    for i in 0..n.saturating_sub(1) {
        let (e0, e1) = (elev[i], elev[i + 1]);
        if e1 <= e0 || e <= e0 {
            break;
        }
        let top = e.min(e1);
        let a_top = areas[i] + (areas[i + 1] - areas[i]) * (top - e0) / (e1 - e0);
        let (r0, r1) = ((areas[i].max(0.0) / PI).sqrt(), (a_top.max(0.0) / PI).sqrt());
        let slant = ((top - e0).powi(2) + (r1 - r0).powi(2)).sqrt();
        total += PI * (r0 + r1) * slant;
    }
    total
}

fn pipe_volume(shape: PipeShape, w: f64, h: f64, length: f64, slope: f64, depth_at_low_end: f64) -> f64 {
    if w <= 0.0 || h <= 0.0 || length <= 0.0 || depth_at_low_end <= 0.0 {
        return 0.0;
    }
    let s = Section::Pipe { shape, width: w, height: h, fill: 0.0 };
    let rise = slope * length;
    if rise.abs() < 1e-9 {
        return s.props(depth_at_low_end.min(h)).area * length;
    }
    // Sloped: integrate the section area along the length (invert rises from the low end).
    let n = 100;
    let dx = length / n as f64;
    (0..n)
        .map(|i| {
            let x = (i as f64 + 0.5) * dx;
            let y = depth_at_low_end - rise.abs() * x / length;
            if y > 0.0 { s.props(y.min(h)).area * dx } else { 0.0 }
        })
        .sum()
}

/// The combined storage of all volumes in a pond.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PondStorage {
    pub volumes: Vec<StorageVolume>,
}

impl PondStorage {
    pub fn is_empty(&self) -> bool {
        self.volumes.is_empty()
    }

    pub fn bottom(&self) -> Option<f64> {
        self.volumes.iter().map(|v| v.bottom()).reduce(f64::min)
    }

    pub fn top(&self) -> Option<f64> {
        self.volumes.iter().map(|v| v.top()).reduce(f64::max)
    }

    /// Water stored (cu ft) below elevation `e`, with voids, multipliers and embedding.
    pub fn storage(&self, e: f64) -> f64 {
        let gross: Vec<f64> = self.volumes.iter().map(|v| v.gross(e) * v.multiplier.max(0.0)).collect();
        let mut total = 0.0;
        for (i, v) in self.volumes.iter().enumerate() {
            let displaced: f64 = self
                .volumes
                .iter()
                .enumerate()
                .filter(|(j, w)| *j != i && w.embedded_in == Some(i))
                .map(|(j, _)| gross[j])
                .sum();
            total += (gross[i] - displaced).max(0.0) * (v.voids_pct / 100.0).clamp(0.0, 1.0);
        }
        total
    }

    /// Surface area of the outermost volumes at `e` (sq ft), if every one of them defines it.
    pub fn surface_area(&self, e: f64) -> Option<f64> {
        let mut sum = 0.0;
        for v in self.volumes.iter().filter(|v| v.embedded_in.is_none()) {
            sum += v.surface_area(e)? * v.multiplier.max(0.0);
        }
        Some(sum)
    }

    /// Wetted area of the outermost volumes below `e` (sq ft), if every one defines it.
    pub fn wetted_area(&self, e: f64) -> Option<f64> {
        let mut sum = 0.0;
        for v in self.volumes.iter().filter(|v| v.embedded_in.is_none()) {
            sum += v.wetted_area(e)? * v.multiplier.max(0.0);
        }
        Some(sum)
    }

    /// Available storage of each volume at its own top (for the "Avail.Storage" column).
    pub fn available(&self, i: usize) -> f64 {
        let Some(v) = self.volumes.get(i) else { return 0.0 };
        let top = v.top();
        let single = PondStorage { volumes: vec![StorageVolume { embedded_in: None, ..v.clone() }] };
        let mut s = single.storage(top);
        // Subtract what embedded volumes displace.
        for (j, w) in self.volumes.iter().enumerate() {
            if j != i && w.embedded_in == Some(i) {
                s -= w.gross(w.top().min(top)) * w.multiplier.max(0.0) * (v.voids_pct / 100.0).clamp(0.0, 1.0);
            }
        }
        s.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AC: f64 = 43_560.0;

    fn reference_pond() -> PondStorage {
        PondStorage {
            volumes: vec![StorageVolume::new(
                "Custom Stage Data (Prismatic)",
                50.0,
                VolumeShape::Custom {
                    kind: CustomKind::AreaPrismatic,
                    elevations: vec![50.0, 52.5, 53.0, 55.0, 57.0],
                    values: vec![0.2 * AC, 0.4 * AC, 1.2 * AC, 1.5 * AC, 2.0 * AC],
                },
            )],
        }
    }

    #[test]
    fn reference_custom_stage_storage() {
        // Cum.Store 0.000, 0.750, 1.150, 3.850, 7.350 acre-feet.
        let p = reference_pond();
        for (e, af) in [(50.0, 0.0), (52.5, 0.75), (53.0, 1.15), (55.0, 3.85), (57.0, 7.35)] {
            assert!((p.storage(e) / AC - af).abs() < 1e-9, "{e}");
        }
        // Peak elev 54.49 (54.486 unrounded) -> storage 3.156 af, surface area 1.423 ac.
        assert!((p.storage(54.486) / AC - 3.156).abs() < 0.0005);
        assert!((p.surface_area(54.486).unwrap() / AC - 1.423).abs() < 0.0005);
        assert!((p.available(0) / AC - 7.35).abs() < 1e-9);
    }

    #[test]
    fn prismatoid_matches_closed_form() {
        // 10' x 20' x 10' with vertical sides = 2000 cf.
        let v = StorageVolume::new(
            "vault",
            100.0,
            VolumeShape::Prismatoid { width: 10.0, length: 20.0, height: 10.0, z: 0.0 },
        );
        assert!((v.gross(110.0) - 2000.0).abs() < 1e-9);
        assert!((v.gross(120.0) - 2000.0).abs() < 1e-9);
        // Sloped sides: compare with numerical integration of the surface area.
        let s =
            StorageVolume::new("pond", 0.0, VolumeShape::Prismatoid { width: 10.0, length: 20.0, height: 4.0, z: 3.0 });
        let mut integ = 0.0;
        let n = 4000;
        for i in 0..n {
            let e = (i as f64 + 0.5) * 4.0 / n as f64;
            integ += s.surface_area(e).unwrap() * 4.0 / n as f64;
        }
        assert!((s.gross(4.0) - integ).abs() / integ < 1e-6);
    }

    #[test]
    fn embedded_chamber_in_stone() {
        // 1000 cf stone bed at 40% voids with a 200 cf chamber inside: 0.4*(1000-200) + 200 = 520.
        let stone = StorageVolume {
            voids_pct: 40.0,
            ..StorageVolume::new(
                "stone",
                0.0,
                VolumeShape::Prismatoid { width: 10.0, length: 10.0, height: 10.0, z: 0.0 },
            )
        };
        let chamber = StorageVolume {
            embedded_in: Some(0),
            ..StorageVolume::new(
                "chamber",
                0.0,
                VolumeShape::Prismatoid { width: 2.0, length: 10.0, height: 10.0, z: 0.0 },
            )
        };
        let p = PondStorage { volumes: vec![stone, chamber] };
        assert!((p.storage(10.0) - 520.0).abs() < 1e-9);
    }

    #[test]
    fn horizontal_pipe_and_cylinder() {
        let pipe = StorageVolume::new(
            "pipe",
            102.0,
            VolumeShape::Pipe { shape: PipeShape::Round, width: 4.0, height: 4.0, length: 100.0, slope: 0.0 },
        );
        assert!((pipe.gross(106.0) - PI * 4.0 * 100.0).abs() < 1e-6);
        let cyl =
            StorageVolume::new("drywell", 100.0, VolumeShape::VerticalCone { diameter: 10.0, height: 8.0, z: 0.0 });
        assert!((cyl.gross(108.0) - PI * 25.0 * 8.0).abs() < 1e-9);
        let sloped = StorageVolume::new(
            "pipe",
            0.0,
            VolumeShape::Pipe { shape: PipeShape::Round, width: 2.0, height: 2.0, length: 100.0, slope: 0.01 },
        );
        assert!((sloped.gross(10.0) - PI * 100.0).abs() < 0.05);
        assert!(sloped.gross(1.0) < PI * 100.0 / 2.0);
    }

    #[test]
    fn malformed_custom_data_is_safe() {
        let v = StorageVolume::new(
            "x",
            0.0,
            VolumeShape::Custom { kind: CustomKind::AreaConic, elevations: vec![5.0], values: vec![] },
        );
        assert_eq!(v.gross(10.0), 0.0);
        let v = StorageVolume::new(
            "x",
            0.0,
            VolumeShape::Custom {
                kind: CustomKind::CumulativeStorage,
                elevations: vec![1.0, 0.0],
                values: vec![1.0, 2.0],
            },
        );
        let _ = v.gross(0.5);
    }
}
