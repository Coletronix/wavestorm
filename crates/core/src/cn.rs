//! Runoff curve numbers: the TR-55 Table 2-2 lookup, composite weighting, and the
//! unconnected-impervious adjustment (TR-55 Figure 2-4).

use serde::{Deserialize, Serialize};

/// Hydrologic soil group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Hsg {
    A,
    B,
    C,
    D,
}

impl Hsg {
    pub const ALL: [Hsg; 4] = [Hsg::A, Hsg::B, Hsg::C, Hsg::D];
    pub fn letter(self) -> char {
        match self {
            Hsg::A => 'A',
            Hsg::B => 'B',
            Hsg::C => 'C',
            Hsg::D => 'D',
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// One row of the curve-number table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CnRow {
    pub group: &'static str,
    pub cover: &'static str,
    /// Curve numbers for HSG A..D (`None` where TR-55 gives none).
    pub cn: [Option<u8>; 4],
    /// Average percent impervious for pre-weighted urban rows.
    pub impervious_pct: Option<f64>,
}

impl CnRow {
    /// The surface description written to a sub-area, e.g. "Woods, Good, HSG B".
    pub fn description(&self, hsg: Hsg) -> String {
        match self.impervious_pct {
            Some(p) if p > 0.0 && p < 100.0 => format!("{}, {}% imp, HSG {}", self.cover, p, hsg.letter()),
            _ => format!("{}, HSG {}", self.cover, hsg.letter()),
        }
    }
    pub fn cn_for(&self, hsg: Hsg) -> Option<u8> {
        self.cn[hsg.index()]
    }
}

const fn row(group: &'static str, cover: &'static str, a: u8, b: u8, c: u8, d: u8) -> CnRow {
    CnRow { group, cover, cn: [Some(a), Some(b), Some(c), Some(d)], impervious_pct: None }
}
const fn imp(group: &'static str, cover: &'static str, pct: f64, a: u8, b: u8, c: u8, d: u8) -> CnRow {
    CnRow { group, cover, cn: [Some(a), Some(b), Some(c), Some(d)], impervious_pct: Some(pct) }
}
const fn bcd(group: &'static str, cover: &'static str, b: u8, c: u8, d: u8) -> CnRow {
    CnRow { group, cover, cn: [None, Some(b), Some(c), Some(d)], impervious_pct: None }
}

/// TR-55 (1986) Table 2-2a to 2-2d, average runoff condition, Ia = 0.2S.
pub const CN_TABLE: &[CnRow] = &[
    // 2-2a Urban areas
    row("Urban", "Open Space, Poor", 68, 79, 86, 89),
    row("Urban", "Open Space, Fair", 49, 69, 79, 84),
    row("Urban", "Open Space, Good", 39, 61, 74, 80),
    imp("Urban", "Paved parking", 100.0, 98, 98, 98, 98),
    imp("Urban", "Roofs", 100.0, 98, 98, 98, 98),
    imp("Urban", "Paved roads w/curbs & sewers", 100.0, 98, 98, 98, 98),
    imp("Urban", "Unconnected pavement", 100.0, 98, 98, 98, 98),
    imp("Urban", "Unconnected roofs", 100.0, 98, 98, 98, 98),
    row("Urban", "Paved roads w/open ditches", 83, 89, 92, 93),
    row("Urban", "Gravel roads", 76, 85, 89, 91),
    row("Urban", "Dirt roads", 72, 82, 87, 89),
    row("Urban", "Gravel surface (no right-of-way)", 96, 96, 96, 96),
    row("Urban", "Natural desert landscaping", 63, 77, 85, 88),
    row("Urban", "Artificial desert landscaping", 96, 96, 96, 96),
    imp("Urban", "Urban commercial", 85.0, 89, 92, 94, 95),
    imp("Urban", "Urban industrial", 72.0, 81, 88, 91, 93),
    imp("Urban", "1/8 acre lots", 65.0, 77, 85, 90, 92),
    imp("Urban", "1/4 acre lots", 38.0, 61, 75, 83, 87),
    imp("Urban", "1/3 acre lots", 30.0, 57, 72, 81, 86),
    imp("Urban", "1/2 acre lots", 25.0, 54, 70, 80, 85),
    imp("Urban", "1 acre lots", 20.0, 51, 68, 79, 84),
    imp("Urban", "2 acre lots", 12.0, 46, 65, 77, 82),
    row("Urban", "Newly graded area", 77, 86, 91, 94),
    imp("Water", "Water Surface (impervious)", 100.0, 98, 98, 98, 98),
    imp("Water", "Water Surface (pervious)", 0.0, 98, 98, 98, 98),
    // 2-2b Cultivated agricultural lands
    row("Cultivated", "Fallow, bare soil", 77, 86, 91, 94),
    row("Cultivated", "Fallow, crop residue, Poor", 76, 85, 90, 93),
    row("Cultivated", "Fallow, crop residue, Good", 74, 83, 88, 90),
    row("Cultivated", "Row crops, straight row, Poor", 72, 81, 88, 91),
    row("Cultivated", "Row crops, straight row, Good", 67, 78, 85, 89),
    row("Cultivated", "Row crops, SR + crop residue, Poor", 71, 80, 87, 90),
    row("Cultivated", "Row crops, SR + crop residue, Good", 64, 75, 82, 85),
    row("Cultivated", "Row crops, contoured, Poor", 70, 79, 84, 88),
    row("Cultivated", "Row crops, contoured, Good", 65, 75, 82, 86),
    row("Cultivated", "Row crops, C + crop residue, Poor", 69, 78, 83, 87),
    row("Cultivated", "Row crops, C + crop residue, Good", 64, 74, 81, 85),
    row("Cultivated", "Row crops, contoured & terraced, Poor", 66, 74, 80, 82),
    row("Cultivated", "Row crops, contoured & terraced, Good", 62, 71, 78, 81),
    row("Cultivated", "Row crops, C&T + crop residue, Poor", 65, 73, 79, 81),
    row("Cultivated", "Row crops, C&T + crop residue, Good", 61, 70, 77, 80),
    row("Cultivated", "Small grain, straight row, Poor", 65, 76, 84, 88),
    row("Cultivated", "Small grain, straight row, Good", 63, 75, 83, 87),
    row("Cultivated", "Small grain, SR + crop residue, Poor", 64, 75, 83, 86),
    row("Cultivated", "Small grain, SR + crop residue, Good", 60, 72, 80, 84),
    row("Cultivated", "Small grain, contoured, Poor", 63, 74, 82, 85),
    row("Cultivated", "Small grain, contoured, Good", 61, 73, 81, 84),
    row("Cultivated", "Small grain, C + crop residue, Poor", 62, 73, 81, 84),
    row("Cultivated", "Small grain, C + crop residue, Good", 60, 72, 80, 83),
    row("Cultivated", "Small grain, contoured & terraced, Poor", 61, 72, 79, 82),
    row("Cultivated", "Small grain, contoured & terraced, Good", 59, 70, 78, 81),
    row("Cultivated", "Small grain, C&T + crop residue, Poor", 60, 71, 78, 81),
    row("Cultivated", "Small grain, C&T + crop residue, Good", 58, 69, 77, 80),
    row("Cultivated", "Legumes/rotation meadow, straight row, Poor", 66, 77, 85, 89),
    row("Cultivated", "Legumes/rotation meadow, straight row, Good", 58, 72, 81, 85),
    row("Cultivated", "Legumes/rotation meadow, contoured, Poor", 64, 75, 83, 85),
    row("Cultivated", "Legumes/rotation meadow, contoured, Good", 55, 69, 78, 83),
    row("Cultivated", "Legumes/rotation meadow, C&T, Poor", 63, 73, 80, 83),
    row("Cultivated", "Legumes/rotation meadow, C&T, Good", 51, 67, 76, 80),
    // 2-2c Other agricultural lands
    row("Agricultural", "Pasture/grassland/range, Poor", 68, 79, 86, 89),
    row("Agricultural", "Pasture/grassland/range, Fair", 49, 69, 79, 84),
    row("Agricultural", "Pasture/grassland/range, Good", 39, 61, 74, 80),
    row("Agricultural", "Meadow, non-grazed", 30, 58, 71, 78),
    row("Agricultural", "Brush, Poor", 48, 67, 77, 83),
    row("Agricultural", "Brush, Fair", 35, 56, 70, 77),
    row("Agricultural", "Brush, Good", 30, 48, 65, 73),
    row("Agricultural", "Woods/grass comb., Poor", 57, 73, 82, 86),
    row("Agricultural", "Woods/grass comb., Fair", 43, 65, 76, 82),
    row("Agricultural", "Woods/grass comb., Good", 32, 58, 72, 79),
    row("Agricultural", "Woods, Poor", 45, 66, 77, 83),
    row("Agricultural", "Woods, Fair", 36, 60, 73, 79),
    row("Agricultural", "Woods, Good", 30, 55, 70, 77),
    row("Agricultural", "Farmstead", 59, 74, 82, 86),
    // 2-2d Arid and semiarid rangelands
    bcd("Arid Rangeland", "Herbaceous, Poor", 80, 87, 93),
    bcd("Arid Rangeland", "Herbaceous, Fair", 71, 81, 89),
    bcd("Arid Rangeland", "Herbaceous, Good", 62, 74, 85),
    bcd("Arid Rangeland", "Oak-aspen, Poor", 66, 74, 79),
    bcd("Arid Rangeland", "Oak-aspen, Fair", 48, 57, 63),
    bcd("Arid Rangeland", "Oak-aspen, Good", 30, 41, 48),
    bcd("Arid Rangeland", "Pinyon-juniper, Poor", 75, 85, 89),
    bcd("Arid Rangeland", "Pinyon-juniper, Fair", 58, 73, 80),
    bcd("Arid Rangeland", "Pinyon-juniper, Good", 41, 61, 71),
    bcd("Arid Rangeland", "Sagebrush w/grass, Poor", 67, 80, 85),
    bcd("Arid Rangeland", "Sagebrush w/grass, Fair", 51, 63, 70),
    bcd("Arid Rangeland", "Sagebrush w/grass, Good", 35, 47, 55),
    row("Arid Rangeland", "Desert shrub, Poor", 63, 77, 85, 88),
    row("Arid Rangeland", "Desert shrub, Fair", 55, 72, 81, 86),
    row("Arid Rangeland", "Desert shrub, Good", 49, 68, 79, 84),
];

/// How a subcatchment's sub-areas are combined into runoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CnWeighting {
    /// One area-weighted CN for the whole subcatchment.
    #[default]
    WeightedCn,
    /// Separate weighted CNs for the pervious and impervious parts.
    PerviousImpervious,
    /// Runoff computed for each CN separately and summed ("weighted-Q").
    WeightedQ,
}

impl CnWeighting {
    pub const ALL: [CnWeighting; 3] = [CnWeighting::WeightedCn, CnWeighting::PerviousImpervious, CnWeighting::WeightedQ];
    pub fn label(self) -> &'static str {
        match self {
            CnWeighting::WeightedCn => "Weighted CN",
            CnWeighting::PerviousImpervious => "Weighted CN, pervious & impervious separately",
            CnWeighting::WeightedQ => "Weighted Q (runoff for each CN)",
        }
    }
}

/// A sub-area as entered on a subcatchment's Area tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubArea {
    /// Square feet.
    pub area: f64,
    /// Curve number (SCS/SBUH) — also used as the Rational C when `c` is absent.
    pub cn: f64,
    #[serde(default)]
    pub c: Option<f64>,
    pub description: String,
    /// Soil group, for the soil listing.
    #[serde(default)]
    pub hsg: Option<Hsg>,
    /// Land use name, for pollutant loading.
    #[serde(default)]
    pub land_use: Option<String>,
    /// A sub-area drawn inside its subcatchment's outline (diagram units): its area is measured
    /// from this polygon (see `drawing::sync_subareas`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<[f64; 2]>>,
    /// In a subcatchment with drawn sub-areas, the row holding the rest of the outline: its
    /// area is the outline's area less the drawn sub-areas.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remainder: bool,
}

impl SubArea {
    pub fn new(area: f64, cn: f64, description: &str) -> Self {
        SubArea { area, cn, c: None, description: description.into(), hsg: soil_from_description(description), land_use: None, outline: None, remainder: false }
    }

    /// Fraction of this sub-area that is impervious: 100% for CN ≥ 98, the "x% imp" value in the
    /// description, or zero.
    pub fn impervious_fraction(&self) -> f64 {
        if let Some(p) = percent_impervious(&self.description) {
            return (p / 100.0).clamp(0.0, 1.0);
        }
        if self.description.to_ascii_lowercase().contains("(pervious)") {
            return 0.0;
        }
        if self.cn >= 98.0 { 1.0 } else { 0.0 }
    }

    pub fn is_unconnected(&self) -> bool {
        self.description.to_ascii_lowercase().contains("unconnected")
    }
}

/// Parses "38% imp" from a description.
pub fn percent_impervious(description: &str) -> Option<f64> {
    let lower = description.to_ascii_lowercase();
    let idx = lower.find("% imp")?;
    let head = &lower[..idx];
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + 1).unwrap_or(0);
    head.get(start..)?.parse().ok()
}

/// Reads "HSG B" from a description.
pub fn soil_from_description(description: &str) -> Option<Hsg> {
    let upper = description.to_ascii_uppercase();
    let idx = upper.rfind("HSG ")?;
    match upper.get(idx + 4..idx + 5)? {
        "A" => Some(Hsg::A),
        "B" => Some(Hsg::B),
        "C" => Some(Hsg::C),
        "D" => Some(Hsg::D),
        _ => None,
    }
}

/// Composite CN data for one subcatchment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Composite {
    pub total_area: f64,
    /// Area-weighted CN before rounding, with any unconnected-impervious adjustment applied.
    pub cn_raw: f64,
    pub cn: f64,
    pub pervious_area: f64,
    pub impervious_area: f64,
    pub pervious_cn: f64,
    pub impervious_cn: f64,
    pub unconnected_area: f64,
}

/// Area-weighted composite curve number, rounded to `decimals` places when `round` is set.
pub fn composite(areas: &[SubArea], round: bool, decimals: u32) -> Composite {
    let mut c = Composite::default();
    let (mut sum_p, mut sum_i) = (0.0, 0.0);
    for a in areas {
        let area = a.area.max(0.0);
        c.total_area += area;
        // A pre-weighted CN splits into 98 impervious and an implied pervious CN.
        let fi = a.impervious_fraction();
        let ai = area * fi;
        let ap = area - ai;
        let (cn_i, cn_p) = if fi > 0.0 && fi < 1.0 { (98.0, ((a.cn - 98.0 * fi) / (1.0 - fi)).clamp(0.0, 100.0)) } else { (a.cn, a.cn) };
        c.impervious_area += ai;
        c.pervious_area += ap;
        sum_i += ai * cn_i;
        sum_p += ap * cn_p;
        if a.is_unconnected() {
            c.unconnected_area += ai;
        }
    }
    if c.total_area <= 0.0 {
        return c;
    }
    c.pervious_cn = if c.pervious_area > 0.0 { sum_p / c.pervious_area } else { 0.0 };
    c.impervious_cn = if c.impervious_area > 0.0 { sum_i / c.impervious_area } else { 0.0 };
    let mut raw = (sum_p + sum_i) / c.total_area;
    let imp_frac = c.impervious_area / c.total_area;
    if c.unconnected_area > 0.0 && imp_frac < 0.30 && c.pervious_area > 0.0 {
        // TR-55 Figure 2-4: CNc = CNp + Pimp (98 - CNp)(1 - 0.5 R).
        let r = c.unconnected_area / c.impervious_area;
        raw = c.pervious_cn + imp_frac * (c.impervious_cn - c.pervious_cn) * (1.0 - 0.5 * r);
    }
    c.cn_raw = raw;
    c.cn = if round { round_to(raw, decimals) } else { raw };
    c
}

pub fn round_to(v: f64, decimals: u32) -> f64 {
    let k = 10f64.powi(decimals as i32);
    (v * k).round() / k
}

#[cfg(test)]
mod tests {
    use super::*;

    const AC: f64 = 43_560.0;

    #[test]
    fn reference_weighted_cn_is_rounded() {
        // 12.5 ac @58, 7.4 @65, 5.1 @60 -> 60 (59.98 before rounding).
        let areas = vec![
            SubArea::new(12.5 * AC, 58.0, "Woods/grass comb., Good, HSG B"),
            SubArea::new(7.4 * AC, 65.0, "Woods/grass comb., Fair, HSG B"),
            SubArea::new(5.1 * AC, 60.0, "Woods, Fair, HSG B"),
        ];
        let c = composite(&areas, true, 0);
        assert_eq!(c.cn, 60.0);
        assert!((c.total_area - 25.0 * AC).abs() < 1e-6);
        assert_eq!(c.impervious_area, 0.0);
        assert_eq!(areas[0].hsg, Some(Hsg::B));
    }

    #[test]
    fn unconnected_impervious_matches_tr55_example() {
        // TR-55: 1/2-acre lot, 20% impervious (75% unconnected), pervious CN 61 -> composite 66.
        let areas = vec![
            SubArea::new(0.8 * AC, 61.0, "Open Space, Good, HSG B"),
            SubArea::new(0.05 * AC, 98.0, "Paved parking, HSG B"),
            SubArea::new(0.15 * AC, 98.0, "Unconnected roofs, HSG B"),
        ];
        let c = composite(&areas, true, 0);
        assert_eq!(c.cn, 66.0);
        // All connected: figure 2-3 gives 68.
        let connected = vec![SubArea::new(0.8 * AC, 61.0, "Open Space, Good, HSG B"), SubArea::new(0.2 * AC, 98.0, "Roofs, HSG B")];
        assert_eq!(composite(&connected, true, 0).cn, 68.0);
    }

    #[test]
    fn preweighted_rows_split_impervious() {
        let a = SubArea::new(AC, 75.0, "1/4 acre lots, 38% imp, HSG B");
        assert!((a.impervious_fraction() - 0.38).abs() < 1e-12);
        let c = composite(&[a], true, 0);
        assert!((c.impervious_area - 0.38 * AC).abs() < 1e-6);
        assert_eq!(c.cn, 75.0);
    }

    #[test]
    fn table_descriptions() {
        let woods = CN_TABLE.iter().find(|r| r.cover == "Woods, Fair").unwrap();
        assert_eq!(woods.cn_for(Hsg::B), Some(60));
        assert_eq!(woods.description(Hsg::B), "Woods, Fair, HSG B");
        let lots = CN_TABLE.iter().find(|r| r.cover == "1/4 acre lots").unwrap();
        assert_eq!(lots.description(Hsg::C), "1/4 acre lots, 38% imp, HSG C");
        assert_eq!(composite(&[], true, 0).cn, 0.0);
    }
}
