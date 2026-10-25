//! TR-55 Graphical Peak Discharge method (chapter 4, worksheet 4) and the storage-volume
//! estimate for detention basins (chapter 6, worksheet 6a).
//!
//! Coefficients: NRCS TR-55 (210-VI-TR-55, 2nd ed., June 1986), appendix F, tables F-1 and F-2;
//! pond and swamp factors: table 4-2.

use crate::runoff::scs_runoff_depth as runoff_depth;

/// Rainfall distribution for the graphical method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    I,
    IA,
    II,
    III,
}

impl Distribution {
    /// The distribution of an NRCS storm name ("Type IA 24-hr" → IA), if it is one.
    pub fn from_storm(name: &str) -> Option<Distribution> {
        let n = name.to_ascii_uppercase();
        let n = n.trim_start_matches("TYPE").trim_start();
        let word = n.split_whitespace().next()?;
        match word {
            "I" => Some(Distribution::I),
            "IA" => Some(Distribution::IA),
            "II" => Some(Distribution::II),
            "III" => Some(Distribution::III),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Distribution::I => "I",
            Distribution::IA => "IA",
            Distribution::II => "II",
            Distribution::III => "III",
        }
    }

    /// Table F-1 rows: (Ia/P, C0, C1, C2).
    fn table(self) -> &'static [(f64, f64, f64, f64)] {
        match self {
            Distribution::I => &[
                (0.10, 2.30550, -0.51429, -0.11750),
                (0.20, 2.23537, -0.50387, -0.08929),
                (0.25, 2.18219, -0.48488, -0.06589),
                (0.30, 2.10624, -0.45695, -0.02835),
                (0.35, 2.00303, -0.40769, 0.01983),
                (0.40, 1.87733, -0.32274, 0.05754),
                (0.45, 1.76312, -0.15644, 0.00453),
                (0.50, 1.67889, -0.06930, 0.0),
            ],
            Distribution::IA => &[
                (0.10, 2.03250, -0.31583, -0.13748),
                (0.20, 1.91978, -0.28215, -0.07020),
                (0.25, 1.83842, -0.25543, -0.02597),
                (0.30, 1.72657, -0.19826, 0.02633),
                (0.50, 1.63417, -0.09100, 0.0),
            ],
            Distribution::II => &[
                (0.10, 2.55323, -0.61512, -0.16403),
                (0.30, 2.46532, -0.62257, -0.11657),
                (0.35, 2.41896, -0.61594, -0.08820),
                (0.40, 2.36409, -0.59857, -0.05621),
                (0.45, 2.29238, -0.57005, -0.02281),
                (0.50, 2.20282, -0.51599, -0.01259),
            ],
            Distribution::III => &[
                (0.10, 2.47317, -0.51848, -0.17083),
                (0.30, 2.39628, -0.51202, -0.13245),
                (0.35, 2.35477, -0.49735, -0.11985),
                (0.40, 2.30726, -0.46541, -0.11094),
                (0.45, 2.24876, -0.41314, -0.11508),
                (0.50, 2.17772, -0.36803, -0.09525),
            ],
        }
    }

    /// Table F-2: (C0, C1, C2, C3) of the storage-ratio curve (figure 6-1).
    fn storage_coefs(self) -> (f64, f64, f64, f64) {
        match self {
            Distribution::I | Distribution::IA => (0.660, -1.76, 1.96, -0.730),
            Distribution::II | Distribution::III => (0.682, -1.43, 1.64, -0.804),
        }
    }
}

/// Unit peak discharge qu (csm/in) for `tc` hours and the ratio `ia_p`, both clamped to the
/// method's range (Tc 0.1-10 h, Ia/P 0.1-0.5). log qu is interpolated linearly in Ia/P
/// between the table F-1 rows.
pub fn unit_peak(dist: Distribution, tc: f64, ia_p: f64) -> f64 {
    let tc = if tc.is_finite() { tc.clamp(0.1, 10.0) } else { 0.1 };
    let r = if ia_p.is_finite() { ia_p.clamp(0.1, 0.5) } else { 0.1 };
    let lt = tc.log10();
    let log_qu = |row: &(f64, f64, f64, f64)| row.1 + row.2 * lt + row.3 * lt * lt;
    let t = dist.table();
    let i = t.partition_point(|row| row.0 < r).clamp(1, t.len() - 1);
    let (a, b) = (&t[i - 1], &t[i]);
    let k = if b.0 > a.0 { ((r - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
    10f64.powf(log_qu(a) + k * (log_qu(b) - log_qu(a)))
}

/// Pond and swamp adjustment factor Fp (table 4-2), interpolated in the percentage.
pub fn pond_factor(pct: f64) -> f64 {
    const T: [(f64, f64); 5] = [(0.0, 1.00), (0.2, 0.97), (1.0, 0.87), (3.0, 0.75), (5.0, 0.72)];
    let p = if pct.is_finite() { pct.clamp(0.0, 5.0) } else { 0.0 };
    for w in T.windows(2) {
        if p <= w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (p - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    0.72
}

/// Worksheet 4 for one storm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakWorksheet {
    pub area_sq_mi: f64,
    pub cn: f64,
    pub tc: f64,
    pub dist: Distribution,
    pub pond_pct: f64,
    pub rainfall: f64,
    pub ia: f64,
    pub ia_p: f64,
    pub qu: f64,
    pub runoff: f64,
    pub fp: f64,
    pub qp: f64,
    /// Ia/P or Tc fell outside the method's range and was clamped.
    pub out_of_range: bool,
}

/// TR-55 graphical peak discharge: qp = qu·Am·Q·Fp. `area_sf` in sq ft, `tc` in hours,
/// `rainfall` (24-hour) in inches.
pub fn graphical_peak(
    area_sf: f64,
    cn: f64,
    tc: f64,
    dist: Distribution,
    rainfall: f64,
    pond_pct: f64,
) -> PeakWorksheet {
    let area_sq_mi = area_sf / 27_878_400.0;
    let s = if cn > 0.0 { 1000.0 / cn - 10.0 } else { f64::INFINITY };
    let ia = 0.2 * s;
    let ia_p = if rainfall > 0.0 { ia / rainfall } else { f64::INFINITY };
    let qu = unit_peak(dist, tc, ia_p);
    let runoff = runoff_depth(rainfall, cn, 0.2);
    let fp = pond_factor(pond_pct);
    let out_of_range = !(0.1..=10.0).contains(&tc) || !(0.1..=0.5).contains(&ia_p);
    PeakWorksheet {
        area_sq_mi,
        cn,
        tc,
        dist,
        pond_pct,
        rainfall,
        ia,
        ia_p,
        qu,
        runoff,
        fp,
        qp: qu * area_sq_mi * runoff * fp,
        out_of_range,
    }
}

impl PeakWorksheet {
    /// Worksheet 4 as report lines.
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![
            format!("Drainage area, Am = {:.4} sq mi ({:.2} ac)", self.area_sq_mi, self.area_sq_mi * 640.0),
            format!("Runoff curve number, CN = {:.0}", self.cn),
            format!("Time of concentration, Tc = {:.2} hr", self.tc),
            format!("Rainfall distribution = Type {}", self.dist.label()),
            format!("Pond and swamp areas = {:.1} % of Am", self.pond_pct),
            String::new(),
            format!("Rainfall, P (24-hour) = {:.2} in", self.rainfall),
            format!("Initial abstraction, Ia = {:.3} in", self.ia),
            format!("Ia/P = {:.2}", self.ia_p),
            format!("Unit peak discharge, qu = {:.0} csm/in", self.qu),
            format!("Runoff, Q = {:.2} in", self.runoff),
            format!("Pond and swamp adjustment factor, Fp = {:.2}", self.fp),
            format!("Peak discharge, qp = qu Am Q Fp = {:.2} cfs", self.qp),
        ];
        if self.out_of_range {
            v.push(
                "Note: Tc or Ia/P is outside the method's range (Tc 0.1-10 hr, Ia/P 0.1-0.5) and was limited to it."
                    .into(),
            );
        }
        v
    }
}

/// Worksheet 4 for every event of a subcatchment (`area_sf`, composite `cn`, `tc` hours), one
/// column per event as on the printed worksheet. Events whose storm is not an NRCS type I, IA,
/// II or III distribution are listed as not applicable.
pub fn worksheet_report(area_sf: f64, cn: f64, tc: f64, events: &[crate::rainfall::RainEvent]) -> Vec<String> {
    let mut out = vec!["TR-55 Graphical Peak Discharge (worksheet 4)".to_string(), String::new()];
    out.push(format!("Drainage area, Am = {:.4} sq mi ({:.2} ac)", area_sf / 27_878_400.0, area_sf / 43_560.0));
    out.push(format!("Runoff curve number, CN = {cn:.0}"));
    out.push(format!("Time of concentration, Tc = {tc:.2} hr"));
    out.push("Pond and swamp areas = 0.0 % of Am (Fp = 1.00)".into());
    out.push(String::new());
    let sheets: Vec<(String, Option<PeakWorksheet>)> = events
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                Distribution::from_storm(&e.storm).map(|d| graphical_peak(area_sf, cn, tc, d, e.depth, 0.0)),
            )
        })
        .collect();
    let row = |label: &str, f: &dyn Fn(&PeakWorksheet) -> String| {
        let mut s = format!("{label:<34}");
        for (_, w) in &sheets {
            s.push_str(&format!("{:>12}", w.as_ref().map_or("n/a".to_string(), f)));
        }
        s
    };
    let mut head = format!("{:<34}", "");
    for (name, _) in &sheets {
        head.push_str(&format!("{name:>12}"));
    }
    out.push(head);
    out.push(row("Rainfall distribution", &|w| format!("Type {}", w.dist.label())));
    out.push(row("Rainfall, P (24-hour), in", &|w| format!("{:.2}", w.rainfall)));
    out.push(row("Initial abstraction, Ia, in", &|w| format!("{:.3}", w.ia)));
    out.push(row("Ia/P", &|w| format!("{:.2}", w.ia_p)));
    out.push(row("Unit peak discharge, qu, csm/in", &|w| format!("{:.0}", w.qu)));
    out.push(row("Runoff, Q, in", &|w| format!("{:.2}", w.runoff)));
    out.push(row("Pond and swamp factor, Fp", &|w| format!("{:.2}", w.fp)));
    out.push(row("Peak discharge, qp, cfs", &|w| format!("{:.2}", w.qp)));
    out.push(String::new());
    if sheets.iter().any(|(_, w)| w.is_some_and(|w| w.out_of_range)) {
        out.push(
            "Note: Tc or Ia/P is outside the method's range (Tc 0.1-10 hr, Ia/P 0.1-0.5) and was limited to it.".into(),
        );
    }
    if sheets.iter().any(|(_, w)| w.is_none()) {
        out.push("n/a: the graphical method applies only to the NRCS 24-hour type I, IA, II and III storms.".into());
    }
    out.push(
        "The graphical method is a check on peak discharge only; the hydrographs use the unit-hydrograph method."
            .into(),
    );
    out
}

/// Storage ratio Vs/Vr for an outflow/inflow peak ratio (figure 6-1, valid 0.1-0.8).
pub fn storage_ratio(dist: Distribution, qo_qi: f64) -> f64 {
    let (c0, c1, c2, c3) = dist.storage_coefs();
    let r = if qo_qi.is_finite() { qo_qi.clamp(0.1, 0.8) } else { 0.1 };
    c0 + c1 * r + c2 * r * r + c3 * r * r * r
}

/// Storage required (cu ft) to cut `qi` to `qo` given runoff `runoff_in` inches over `area_sf`.
// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn storage_estimate(dist: Distribution, qi: f64, qo: f64, runoff_in: f64, area_sf: f64) -> f64 {
    if !(qi > 0.0) {
        return 0.0;
    }
    storage_ratio(dist, qo / qi) * runoff_in / 12.0 * area_sf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TR-55 example 4-1 (worksheet 4, figure 4-2): 250 ac, CN 75, Tc 1.53 h, type II,
    /// P = 6.0 in: Ia = 0.667 in, Ia/P = 0.11, qu ≈ 270 csm/in, Q = 3.28 in, qp = 345 cfs.
    #[test]
    fn example_4_1() {
        let w = graphical_peak(250.0 * 43_560.0, 75.0, 1.53, Distribution::II, 6.0, 0.0);
        assert!((w.ia - 0.667).abs() < 0.001);
        assert!((w.ia_p - 0.11).abs() < 0.005);
        assert!((w.qu - 270.0).abs() < 4.0, "qu {}", w.qu);
        assert!((w.runoff - 3.28).abs() < 0.01);
        assert!((w.qp - 345.0).abs() / 345.0 < 0.015, "qp {}", w.qp);
        assert!(!w.out_of_range);
    }

    /// TR-55 example 6-1: qo/qi = 180/360, Q = 3.4 in over 75 ac (type II) → Vs = 5.9 af.
    #[test]
    fn example_6_1() {
        let vs = storage_estimate(Distribution::II, 360.0, 180.0, 3.4, 75.0 * 43_560.0) / 43_560.0;
        assert!((vs - 5.9).abs() < 0.05, "{vs}");
    }

    #[test]
    fn helpers() {
        assert_eq!(Distribution::from_storm("Type IA 24-hr"), Some(Distribution::IA));
        assert_eq!(Distribution::from_storm("Type III 24-hr"), Some(Distribution::III));
        assert_eq!(Distribution::from_storm("Huff 0-10sm"), None);
        assert!((pond_factor(0.6) - 0.92).abs() < 1e-9);
        assert_eq!(pond_factor(9.0), 0.72);
        // Clamped, never NaN.
        assert!(unit_peak(Distribution::IA, f64::NAN, 2.0).is_finite());
    }
}
