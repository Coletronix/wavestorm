//! Chamber wizard: lays out a field of underground storage chambers in a stone bed and builds
//! the pond storage volumes (a stone prismatoid with the chambers embedded in it), plus a
//! layout report and an optional cost estimate.
//!
//! Chamber models are user-defined (no manufacturer data is bundled): a width-vs-height
//! profile of the chamber's storage cross-section, its installed length and recommended
//! installation parameters. A few generic arch-shaped samples are provided.

use crate::storage::{CustomKind, StorageVolume, VolumeShape};
use serde::{Deserialize, Serialize};

/// Cubic feet per cubic yard.
const CF_PER_CY: f64 = 27.0;

/// A chamber model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChamberModel {
    pub name: String,
    pub comment: String,
    /// Storage cross-section: (height above the chamber invert, width), ft.
    pub profile: Vec<(f64, f64)>,
    /// Installed (effective) length per chamber, ft.
    pub length: f64,
    /// Outside width and height (for spacing and stone depth), ft.
    pub outside_width: f64,
    pub outside_height: f64,
    /// Overall length of one unit and the overlap between units, ft (report only).
    pub overall_length: f64,
    pub overlap: f64,
    /// Recommended installation (ft): row spacing, side and end stone, cover, base.
    pub row_spacing: f64,
    pub side_stone: f64,
    pub end_stone: f64,
    pub cover: f64,
    pub base: f64,
}

impl Default for ChamberModel {
    fn default() -> Self {
        ChamberModel::arch("Chamber", 4.0, 2.5, 7.0)
    }
}

impl ChamberModel {
    /// A generic half-ellipse arch chamber `width` x `height` ft, `length` ft per unit.
    pub fn arch(name: &str, width: f64, height: f64, length: f64) -> ChamberModel {
        let n = 10;
        let profile = (0..=n)
            .map(|i| {
                let h = height * i as f64 / n as f64;
                let w = width * (1.0 - (h / height).powi(2)).max(0.0).sqrt();
                (h, w)
            })
            .collect();
        ChamberModel {
            name: name.into(),
            comment: format!("Generic arch chamber, {:.0}\" W x {:.0}\" H", width * 12.0, height * 12.0),
            profile,
            length,
            outside_width: width + 4.0 / 12.0,
            outside_height: height + 0.5 / 12.0,
            overall_length: length + 1.5,
            overlap: 1.5,
            row_spacing: 0.5,
            side_stone: 1.0,
            end_stone: 1.0,
            cover: 0.5,
            base: 0.5,
        }
    }

    /// Generic samples (not any manufacturer's products).
    pub fn samples() -> Vec<ChamberModel> {
        vec![
            ChamberModel::arch("Sample Arch 34x16", 34.0 / 12.0, 16.0 / 12.0, 7.0),
            ChamberModel::arch("Sample Arch 52x30", 52.0 / 12.0, 30.0 / 12.0, 7.0),
            ChamberModel::arch("Sample Arch 60x36", 60.0 / 12.0, 36.0 / 12.0, 7.0),
            ChamberModel::arch("Sample Arch 78x48", 78.0 / 12.0, 48.0 / 12.0, 7.0),
        ]
    }

    /// Storage height of the chamber (top of the profile), ft.
    pub fn height(&self) -> f64 {
        self.profile.iter().map(|p| p.0).fold(0.0, f64::max)
    }

    /// Cross-section area below `h` (sq ft), by the trapezoid rule on the profile.
    pub fn area_below(&self, h: f64) -> f64 {
        let mut a = 0.0;
        for w in self.profile.windows(2) {
            let ((h0, w0), (h1, w1)) = (w[0], w[1]);
            if h1 <= h0 || h <= h0 {
                continue;
            }
            let top = h.min(h1);
            let wt = w0 + (w1 - w0) * (top - h0) / (h1 - h0);
            a += 0.5 * (w0 + wt) * (top - h0);
        }
        a
    }

    /// Storage of one chamber (cu ft).
    pub fn volume(&self) -> f64 {
        self.area_below(self.height()) * self.length.max(0.0)
    }

    /// Effective width: cross-section area / height (ft).
    pub fn effective_width(&self) -> f64 {
        let h = self.height();
        if h > 0.0 { self.area_below(h) / h } else { 0.0 }
    }
}

/// Unit costs and extra line items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Costs {
    pub chamber: f64,
    /// Per cubic yard.
    pub excavation: f64,
    pub stone: f64,
    /// (quantity, description, unit price).
    pub extras: Vec<(f64, String, f64)>,
}

/// A chamber field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    pub model: ChamberModel,
    pub rows: u32,
    pub per_row: u32,
    /// Use the model's recommended spacing, stone and cover.
    pub typical: bool,
    pub row_spacing: f64,
    pub side_stone: f64,
    pub end_stone: f64,
    pub cover: f64,
    pub base: f64,
    /// Side slope of the excavation (run/rise).
    pub side_z: f64,
    pub voids_pct: f64,
    /// Elevation of the bottom of the stone bed.
    pub invert: f64,
    pub costs: Costs,
    pub show_costs: bool,
}

impl Default for Field {
    fn default() -> Self {
        Field {
            model: ChamberModel::default(),
            rows: 4,
            per_row: 6,
            typical: true,
            row_spacing: 0.5,
            side_stone: 1.0,
            end_stone: 1.0,
            cover: 0.5,
            base: 0.5,
            side_z: 0.0,
            voids_pct: 40.0,
            invert: 100.0,
            costs: Costs::default(),
            show_costs: false,
        }
    }
}

/// Field dimensions and volumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub count: f64,
    pub base_length: f64,
    pub base_width: f64,
    pub height: f64,
    /// Gross volume of the stone bed (cu ft, including the chambers).
    pub field_volume: f64,
    pub chamber_storage: f64,
    pub stone_storage: f64,
}

impl Field {
    /// Installation parameters in effect (the model's when `typical`).
    fn params(&self) -> (f64, f64, f64, f64, f64) {
        if self.typical {
            let m = &self.model;
            (m.row_spacing, m.side_stone, m.end_stone, m.cover, m.base)
        } else {
            (self.row_spacing, self.side_stone, self.end_stone, self.cover, self.base)
        }
    }

    pub fn layout(&self) -> Layout {
        let m = &self.model;
        let (spacing, side, end, cover, base) = self.params();
        let (rows, per_row) = (self.rows.max(1) as f64, self.per_row.max(1) as f64);
        let base_length = per_row * m.length.max(0.0) + 2.0 * end.max(0.0);
        let base_width = rows * m.outside_width.max(0.0) + (rows - 1.0) * spacing.max(0.0) + 2.0 * side.max(0.0);
        let height = base.max(0.0) + m.outside_height.max(0.0) + cover.max(0.0);
        let z = self.side_z.max(0.0);
        let field_volume = prismatoid(base_width, base_length, height, z);
        let count = rows * per_row;
        let chamber_storage = count * m.volume();
        let stone_storage = (field_volume - chamber_storage).max(0.0) * self.voids_pct.clamp(0.0, 100.0) / 100.0;
        Layout { count, base_length, base_width, height, field_volume, chamber_storage, stone_storage }
    }

    /// Pond storage volumes: the stone bed and the chambers embedded in it.
    pub fn volumes(&self) -> Vec<StorageVolume> {
        let l = self.layout();
        let (_, _, _, _, base) = self.params();
        let mut stone = StorageVolume::new(
            &format!("{:.2}'W x {:.2}'L x {:.2}'H Field", l.base_width, l.base_length, l.height),
            self.invert,
            VolumeShape::Prismatoid { width: l.base_width, length: l.base_length, height: l.height, z: self.side_z.max(0.0) },
        );
        stone.voids_pct = self.voids_pct.clamp(0.0, 100.0);
        let m = &self.model;
        let inv = self.invert + base.max(0.0);
        let (elevations, values): (Vec<f64>, Vec<f64>) = m.profile.iter().map(|(h, w)| (inv + h, w * m.length.max(0.0))).unzip();
        let mut chambers = StorageVolume::new(&format!("{} x {} Inside #1", m.name, l.count), inv, VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations, values });
        chambers.multiplier = l.count;
        chambers.embedded_in = Some(0);
        vec![stone, chambers]
    }

    /// The wizard's text report: layout, storage and (optionally) costs.
    pub fn report(&self) -> Vec<String> {
        let m = &self.model;
        let l = self.layout();
        let (spacing, side, end, cover, base) = self.params();
        let inch = |ft: f64| format!("{:.1}\"", ft * 12.0);
        let mut out = vec![
            format!("{}: {}", m.name, m.comment),
            format!(
                "Effective Size= {:.1}\"W x {:.1}\"H => {:.2} sf x {:.2}'L = {:.1} cf",
                m.effective_width() * 12.0,
                m.height() * 12.0,
                m.area_below(m.height()),
                m.length,
                m.volume()
            ),
            format!("Overall Size= {:.1}\"W x {:.1}\"H x {:.2}'L with {:.2}' Overlap", m.outside_width * 12.0, m.outside_height * 12.0, m.overall_length, m.overlap),
            String::new(),
            format!("{} Wide + {} Spacing = {} C-C", inch(m.outside_width), inch(spacing), inch(m.outside_width + spacing)),
            format!("{} Chambers/Row x {:.2}' Long + {} End Stone x 2 = {:.2}' Base Length", self.per_row.max(1), m.length, inch(end), l.base_length),
            format!(
                "{} Rows x {} Wide + {} Spacing x {} + {} Side Stone x 2 = {:.2}' Base Width",
                self.rows.max(1),
                inch(m.outside_width),
                inch(spacing),
                self.rows.max(1) - 1,
                inch(side),
                l.base_width
            ),
            format!("{} Base + {} Chamber Height + {} Cover = {:.2}' Field Height", inch(base), inch(m.outside_height), inch(cover), l.height),
            String::new(),
            format!("{} Chambers x {:.1} cf = {:.1} cf Chamber Storage", l.count, m.volume(), l.chamber_storage),
            format!(
                "{:.1} cf Field - {:.1} cf Chambers = {:.1} cf Stone x {:.1}% Voids = {:.1} cf Stone Storage",
                l.field_volume,
                l.chamber_storage,
                l.field_volume - l.chamber_storage,
                self.voids_pct,
                l.stone_storage
            ),
            String::new(),
            format!("Stone + Chamber Storage = {:.1} cf = {:.3} af", l.stone_storage + l.chamber_storage, (l.stone_storage + l.chamber_storage) / 43_560.0),
        ];
        if self.show_costs {
            let c = &self.costs;
            let exc_cy = l.field_volume / CF_PER_CY;
            let stone_cy = (l.field_volume - l.chamber_storage).max(0.0) / CF_PER_CY;
            let mut total = l.count * c.chamber + exc_cy * c.excavation + stone_cy * c.stone;
            out.push(String::new());
            out.push(format!("{} Chambers @ $ {:.2} /ea = $ {:.2}", l.count, c.chamber, l.count * c.chamber));
            out.push(format!("{:.1} cy Field Excavation @ $ {:.2} /cy = $ {:.2}", exc_cy, c.excavation, exc_cy * c.excavation));
            out.push(format!("{:.1} cy Stone @ $ {:.2} /cy = $ {:.2}", stone_cy, c.stone, stone_cy * c.stone));
            for (q, d, p) in &c.extras {
                out.push(format!("{q} {d} @ $ {p:.2} = $ {:.2}", q * p));
                total += q * p;
            }
            out.push(format!("Total Cost = $ {total:.2}"));
        }
        out
    }
}

/// Volume of a prismatoid with base `w` x `l`, height `h` and side slope `z`.
fn prismatoid(w: f64, l: f64, h: f64, z: f64) -> f64 {
    w * l * h + (w + l) * z * h * h + 4.0 / 3.0 * z * z * h * h * h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::PondStorage;

    /// The layout arithmetic of the reference's published wizard example: 10 x 10 chambers,
    /// 52" wide at 6" spacing, 7.00' long, 12" side/end stone, 6" base and cover, 30" high,
    /// 51.6 cf each, 40 % voids.
    #[test]
    fn published_layout_example() {
        let mut m = ChamberModel::arch("Test", 4.0, 2.5, 7.0);
        m.outside_width = 52.0 / 12.0;
        m.outside_height = 2.5;
        // Rescale the profile so one chamber holds 51.6 cf (7.38 sf x 7.00').
        let k = 51.6 / m.volume();
        m.profile.iter_mut().for_each(|p| p.1 *= k);
        let f = Field { model: m, rows: 10, per_row: 10, typical: false, row_spacing: 0.5, side_stone: 1.0, end_stone: 1.0, cover: 0.5, base: 0.5, ..Default::default() };
        let l = f.layout();
        assert!((l.base_length - 72.0).abs() < 1e-9);
        assert!((l.base_width - 49.83).abs() < 0.01, "{}", l.base_width);
        assert!((l.height - 3.5).abs() < 1e-9);
        assert!((l.field_volume - 12_558.0).abs() < 5.0, "{}", l.field_volume);
        assert!((l.chamber_storage - 5_160.0).abs() < 1.0);
        assert!((l.stone_storage - 2_959.0).abs() < 3.0, "{}", l.stone_storage);
        // The generated pond storage holds the same total at the top of the field.
        let s = PondStorage { volumes: f.volumes() };
        let total = s.storage(f.invert + l.height);
        assert!((total - (l.chamber_storage + l.stone_storage)).abs() < 1.0, "{total}");
        assert!(f.report().iter().any(|r| r.contains("Stone + Chamber Storage")));
    }

    #[test]
    fn arch_profile_area() {
        let m = ChamberModel::arch("A", 4.0, 2.0, 1.0);
        // Half ellipse: pi/4 * w * h.
        let exact = std::f64::consts::PI / 4.0 * 4.0 * 2.0;
        assert!((m.area_below(2.0) - exact).abs() / exact < 0.03);
        assert_eq!(m.area_below(-1.0), 0.0);
    }

    #[test]
    fn costs_total() {
        let f = Field { show_costs: true, costs: Costs { chamber: 100.0, excavation: 5.0, stone: 15.0, extras: vec![(2.0, "Header".into(), 50.0)] }, ..Default::default() };
        let r = f.report();
        assert!(r.last().is_some_and(|l| l.starts_with("Total Cost")));
    }
}
