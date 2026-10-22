//! Time of concentration: a sum of flow-segment travel times (TR-55 Chapter 3 and others).

use crate::geometry::{G, Section};
use serde::{Deserialize, Serialize};

/// TR-55 Table 3-1: Manning's n for sheet flow.
pub const SHEET_SURFACES: &[(&str, f64)] = &[
    ("Smooth surfaces", 0.011),
    ("Fallow", 0.05),
    ("Cultivated: Residue<=20%", 0.06),
    ("Cultivated: Residue>20%", 0.17),
    ("Grass: Short", 0.15),
    ("Grass: Dense", 0.24),
    ("Grass: Bermuda", 0.41),
    ("Range", 0.13),
    ("Woods: Light underbrush", 0.40),
    ("Woods: Dense underbrush", 0.80),
];

/// Velocity factors Kv (ft/s) for V = Kv·√s (TR-55 Figure 3-1; NEH 630 Ch. 15 upland method).
pub const SHALLOW_SURFACES: &[(&str, f64)] = &[
    ("Paved", 20.3282),
    ("Unpaved", 16.1345),
    ("Grassed Waterway", 15.0),
    ("Nearly Bare & Untilled", 10.0),
    ("Cultivated Straight Rows", 9.0),
    ("Short Grass Pasture", 7.0),
    ("Woodland", 5.0),
    ("Forest w/Heavy Litter", 2.5),
];

/// Kerby retardance coefficients N.
pub const KERBY_SURFACES: &[(&str, f64)] = &[
    ("Smooth impervious surface", 0.02),
    ("Smooth bare packed soil", 0.10),
    ("Poor grass, cultivated row crops", 0.20),
    ("Pasture, average grass", 0.40),
    ("Deciduous timberland", 0.60),
    ("Conifer timberland, dense grass", 0.80),
];

/// Kirpich adjustment factors.
pub const KIRPICH_FACTORS: &[(&str, f64)] = &[
    ("Natural basin, well-defined channel", 1.0),
    ("Overland flow on grassed surfaces", 2.0),
    ("Overland flow on concrete or asphalt", 0.4),
    ("Concrete channels", 0.2),
];

/// One leg of the flow path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TcKind {
    /// Tc entered directly (minutes); length and velocity are optional, for reporting.
    Direct { minutes: f64, length: f64, velocity: f64 },
    /// TR-55 sheet flow (Manning's kinematic solution). `p2` is the 2-year 24-hour depth (in).
    Sheet { length: f64, slope: f64, n: f64, p2: f64, surface: String },
    /// Sheet flow whose length is limited by the McCuen-Spiess criterion (L ≤ 100·√s / n).
    SheetMcCuenSpiess { length: f64, slope: f64, n: f64, p2: f64, surface: String },
    /// Shallow concentrated (or upland) flow, V = Kv·√s.
    Shallow { length: f64, slope: f64, kv: f64, surface: String },
    /// Channel flow by Manning's equation at the bank-full depth.
    Channel { length: f64, slope: f64, n: f64, section: Section },
    /// NRCS lag (curve-number) method; uses the subcatchment's CN.
    Lag { length: f64, slope: f64 },
    /// Kirpich formula with an adjustment factor.
    Kirpich { length: f64, slope: f64, factor: f64 },
    /// Kerby overland-flow formula with retardance N.
    Kerby { length: f64, slope: f64, retardance: f64 },
    /// Travel across a lake or reservoir at the wave celerity √(g·D).
    Lake { length: f64, mean_depth: f64 },
}

/// A Tc flow segment with an optional description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TcSegment {
    pub kind: TcKind,
    #[serde(default)]
    pub description: String,
}

/// Computed travel time and supporting values for one segment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TcResult {
    pub minutes: f64,
    pub length: f64,
    pub slope: f64,
    pub velocity: f64,
    /// Channel capacity at bank-full depth (channel segments only).
    pub capacity: Option<f64>,
    /// For McCuen-Spiess, the length actually used.
    pub effective_length: f64,
}

impl TcKind {
    pub fn label(&self) -> &'static str {
        match self {
            TcKind::Direct { .. } => "Direct Entry",
            TcKind::Sheet { .. } => "Sheet Flow",
            TcKind::SheetMcCuenSpiess { .. } => "Sheet Flow (McCuen-Spiess)",
            TcKind::Shallow { .. } => "Shallow Concentrated Flow",
            TcKind::Channel { section, .. } => match section {
                Section::Rectangular { .. } => "Rectangular Channel",
                Section::Trapezoidal { bottom, .. } if *bottom <= 0.0 => "Vee Channel",
                Section::Trapezoidal { .. } => "Trapezoidal Channel",
                Section::Parabolic { .. } => "Parabolic Channel",
                Section::Pipe { .. } => "Circular Channel (pipe)",
                Section::AreaPerimeter { .. } => "Channel Flow",
                Section::Custom { .. } => "Custom Channel",
                Section::Rating { .. } => "Rated Channel",
            },
            TcKind::Lag { .. } => "Lag/CN Method",
            TcKind::Kirpich { .. } => "Kirpich Method",
            TcKind::Kerby { .. } => "Kerby Method",
            TcKind::Lake { .. } => "Lake or Reservoir",
        }
    }

    pub fn length(&self) -> f64 {
        match self {
            TcKind::Direct { length, .. }
            | TcKind::Sheet { length, .. }
            | TcKind::SheetMcCuenSpiess { length, .. }
            | TcKind::Shallow { length, .. }
            | TcKind::Channel { length, .. }
            | TcKind::Lag { length, .. }
            | TcKind::Kirpich { length, .. }
            | TcKind::Kerby { length, .. }
            | TcKind::Lake { length, .. } => *length,
        }
    }

    /// Travel time for this segment. `cn` is the subcatchment's composite CN (lag method).
    pub fn evaluate(&self, cn: f64) -> TcResult {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        let from_velocity = |length: f64, slope: f64, v: f64| TcResult {
            minutes: if ok(v) && length > 0.0 { length / v / 60.0 } else { 0.0 },
            length,
            slope,
            velocity: v,
            capacity: None,
            effective_length: length,
        };
        match self {
            TcKind::Direct { minutes, length, velocity } => TcResult {
                minutes: minutes.max(0.0),
                length: *length,
                slope: 0.0,
                velocity: *velocity,
                capacity: None,
                effective_length: *length,
            },
            TcKind::Sheet { length, slope, n, p2, .. } => sheet(*length, *slope, *n, *p2),
            TcKind::SheetMcCuenSpiess { length, slope, n, p2, .. } => {
                let max = if *n > 0.0 { 100.0 * slope.max(0.0).sqrt() / n } else { *length };
                let mut r = sheet(length.min(max), *slope, *n, *p2);
                r.length = *length;
                r
            }
            TcKind::Shallow { length, slope, kv, .. } => from_velocity(*length, *slope, kv * slope.max(0.0).sqrt()),
            TcKind::Channel { length, slope, n, section } => {
                let depth = section.full_depth();
                let q = section.manning_q(depth, *n, *slope);
                let a = section.props(depth).area;
                let v = if a > 0.0 { q / a } else { 0.0 };
                TcResult { capacity: Some(q), ..from_velocity(*length, *slope, v) }
            }
            TcKind::Lag { length, slope } => {
                // Tc = L^0.8 (S+1)^0.7 / (1140 Y^0.5) hours with Y in percent (lag = 0.6 Tc).
                let s = if cn > 0.0 { 1000.0 / cn - 10.0 } else { 0.0 };
                let y = slope * 100.0;
                let hours = if ok(y) && *length > 0.0 && cn > 0.0 {
                    length.powf(0.8) * (s + 1.0).powf(0.7) / (1140.0 * y.sqrt())
                } else {
                    0.0
                };
                let minutes = hours * 60.0;
                let v = if minutes > 0.0 { length / (minutes * 60.0) } else { 0.0 };
                TcResult { minutes, length: *length, slope: *slope, velocity: v, capacity: None, effective_length: *length }
            }
            TcKind::Kirpich { length, slope, factor } => {
                let minutes = if ok(*slope) && *length > 0.0 {
                    0.0078 * length.powf(0.77) * slope.powf(-0.385) * factor
                } else {
                    0.0
                };
                let v = if minutes > 0.0 { length / (minutes * 60.0) } else { 0.0 };
                TcResult { minutes, length: *length, slope: *slope, velocity: v, capacity: None, effective_length: *length }
            }
            TcKind::Kerby { length, slope, retardance } => {
                let minutes = if ok(*slope) && *length > 0.0 && *retardance > 0.0 {
                    1.44 * (length * retardance).powf(0.467) * slope.powf(-0.235)
                } else {
                    0.0
                };
                let v = if minutes > 0.0 { length / (minutes * 60.0) } else { 0.0 };
                TcResult { minutes, length: *length, slope: *slope, velocity: v, capacity: None, effective_length: *length }
            }
            TcKind::Lake { length, mean_depth } => from_velocity(*length, 0.0, (G * mean_depth.max(0.0)).sqrt()),
        }
    }
}

fn sheet(length: f64, slope: f64, n: f64, p2: f64) -> TcResult {
    let hours = if length > 0.0 && slope > 0.0 && n > 0.0 && p2 > 0.0 {
        0.007 * (n * length).powf(0.8) / (p2.sqrt() * slope.powf(0.4))
    } else {
        0.0
    };
    let minutes = hours * 60.0;
    TcResult {
        minutes,
        length,
        slope,
        velocity: if hours > 0.0 { length / (hours * 3600.0) } else { 0.0 },
        capacity: None,
        effective_length: length,
    }
}

/// Travel time of one segment rounded to the reporting precision (0.1 min by default); the
/// subcatchment's Tc is the sum of the rounded segment times, as they appear in the report.
pub fn rounded_minutes(minutes: f64, decimals: u32) -> f64 {
    crate::cn::round_to(minutes, decimals)
}

/// Total Tc in minutes for a list of segments (each rounded to `decimals` places).
pub fn total_minutes(segments: &[TcSegment], cn: f64, decimals: u32) -> f64 {
    segments.iter().map(|s| rounded_minutes(s.kind.evaluate(cn).minutes, decimals)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(kind: TcKind) -> TcSegment {
        TcSegment { kind, description: String::new() }
    }

    #[test]
    fn reference_four_segment_tc() {
        // 19.1 + 3.0 + 1.0 + 0.1 = 23.2 min, 831 ft.
        let segs = vec![
            seg(TcKind::Sheet { length: 100.0, slope: 0.025, n: 0.40, p2: 3.40, surface: "Woods: Light underbrush".into() }),
            seg(TcKind::Shallow { length: 350.0, slope: 0.015, kv: 16.1345, surface: "Unpaved".into() }),
            seg(TcKind::Channel { length: 321.0, slope: 0.0125, n: 0.025, section: Section::Parabolic { width: 4.0, depth: 1.5 } }),
            seg(TcKind::Channel { length: 60.0, slope: 0.01, n: 0.013, section: Section::circular(2.0) }),
        ];
        let mins: Vec<f64> = segs.iter().map(|s| s.kind.evaluate(60.0).minutes).collect();
        let expect = [19.1, 3.0, 1.0, 0.1];
        for (m, e) in mins.iter().zip(expect) {
            assert!((m - e).abs() < 0.051, "{m} vs {e}");
        }
        assert!((total_minutes(&segs, 60.0, 1) - 23.2).abs() < 1e-9);
    }

    #[test]
    fn tr55_example_3_1() {
        // Sheet 0.30 h + shallow 0.24 h + channel 0.99 h = 1.53 h (91.9 min in the reference).
        let segs = vec![
            seg(TcKind::Sheet { length: 100.0, slope: 0.01, n: 0.24, p2: 3.6, surface: "Grass: Dense".into() }),
            seg(TcKind::Shallow { length: 1400.0, slope: 0.01, kv: 16.1345, surface: "Unpaved".into() }),
            seg(TcKind::Channel {
                length: 7300.0,
                slope: 0.005,
                n: 0.05,
                section: Section::AreaPerimeter { area: 27.0, perimeter: 28.2 },
            }),
        ];
        let t = total_minutes(&segs, 71.0, 1);
        assert!((t - 91.9).abs() < 1e-9, "{t}");
    }

    #[test]
    fn degenerate_segments_are_zero() {
        assert_eq!(TcKind::Sheet { length: 100.0, slope: 0.0, n: 0.1, p2: 3.0, surface: String::new() }.evaluate(70.0).minutes, 0.0);
        assert_eq!(TcKind::Lag { length: 100.0, slope: 0.01 }.evaluate(0.0).minutes, 0.0);
        assert_eq!(TcKind::Kirpich { length: 100.0, slope: -1.0, factor: 1.0 }.evaluate(70.0).minutes, 0.0);
    }

    #[test]
    fn mccuen_spiess_limits_length() {
        let full = TcKind::Sheet { length: 300.0, slope: 0.01, n: 0.4, p2: 3.0, surface: String::new() }.evaluate(70.0);
        let ms = TcKind::SheetMcCuenSpiess { length: 300.0, slope: 0.01, n: 0.4, p2: 3.0, surface: String::new() }.evaluate(70.0);
        assert!(ms.minutes < full.minutes);
    }
}
