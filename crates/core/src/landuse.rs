//! Land-use reporting and pollutant loading (Schueler's Simple Method or event runoff).

use crate::calc::{Detail, Results};
use crate::project::{NodeData, Project};
use crate::units::group;
use serde::{Deserialize, Serialize};

/// Converts (cu ft) × (mg/L) to pounds.
const LB_PER_CF_MG_L: f64 = 62.428e-6;

/// Land-use settings for a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LandUseSettings {
    /// Pollutant names, e.g. TSS, TP, TN.
    pub pollutants: Vec<String>,
    /// Land uses with an event-mean concentration (mg/L) per pollutant.
    pub uses: Vec<(String, Vec<f64>)>,
    /// Annual rainfall (inches) for the Simple Method; `None` uses each event's runoff.
    pub annual_rainfall: Option<f64>,
    /// Fraction of annual rainfall events that produce runoff (Pj).
    pub correction: f64,
}

impl Default for LandUseSettings {
    fn default() -> Self {
        LandUseSettings { pollutants: vec!["TSS".into(), "TP".into(), "TN".into()], uses: Vec::new(), annual_rainfall: Some(42.0), correction: 0.9 }
    }
}

/// One row of the land-use / loading report.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadRow {
    pub land_use: String,
    pub area: f64,
    pub impervious: f64,
    /// Runoff coefficient Rv (Simple Method).
    pub rv: f64,
    /// Runoff depth (inches): annual (Simple Method) or this event's.
    pub runoff: f64,
    /// Load per pollutant (pounds).
    pub loads: Vec<f64>,
}

/// Aggregates every sub-area by land use and computes loads.
pub fn loading(project: &Project, results: &Results) -> Vec<LoadRow> {
    loading_where(project, results, &|_| true)
}

/// As [`loading`], for the subcatchments `keep` accepts.
pub fn loading_where(project: &Project, results: &Results, keep: &dyn Fn(crate::project::NodeId) -> bool) -> Vec<LoadRow> {
    let s = &project.land_use;
    let mut rows: Vec<LoadRow> = Vec::new();
    for n in project.nodes.iter().filter(|n| keep(n.id)) {
        let NodeData::Subcat(sc) = &n.data else { continue };
        let event_depth = match results.node(n.id).map(|r| (&r.detail, r.outflow.volume_cf())) {
            Some((Detail::Subcat(d), vol)) if d.composite.total_area > 0.0 => vol / d.composite.total_area * 12.0,
            _ => 0.0,
        };
        for a in &sc.areas {
            let name = a.land_use.clone().filter(|u| !u.is_empty()).unwrap_or_else(|| "(unassigned)".into());
            let row = match rows.iter_mut().position(|r| r.land_use == name) {
                Some(i) => &mut rows[i],
                None => {
                    rows.push(LoadRow { land_use: name.clone(), area: 0.0, impervious: 0.0, rv: 0.0, runoff: 0.0, loads: vec![0.0; s.pollutants.len()] });
                    let last = rows.len() - 1;
                    &mut rows[last]
                }
            };
            let imp = a.impervious_fraction();
            // Runoff depth weighted by area.
            let rv = 0.05 + 0.9 * imp;
            let depth = match s.annual_rainfall {
                Some(p) => p * s.correction * rv,
                None => event_depth,
            };
            let old = row.area;
            row.area += a.area;
            if row.area > 0.0 {
                row.impervious = (row.impervious * old + imp * a.area) / row.area;
                row.rv = (row.rv * old + rv * a.area) / row.area;
                row.runoff = (row.runoff * old + depth * a.area) / row.area;
            }
            let conc = s.uses.iter().find(|(u, _)| *u == name).map(|(_, c)| c.clone()).unwrap_or_default();
            let volume_cf = depth / 12.0 * a.area;
            for (k, load) in row.loads.iter_mut().enumerate() {
                *load += volume_cf * conc.get(k).copied().unwrap_or(0.0) * LB_PER_CF_MG_L;
            }
        }
    }
    rows.sort_by(|a, b| a.land_use.cmp(&b.land_use));
    rows
}

/// Text table of the loading report.
pub fn loading_report(project: &Project, results: &Results) -> Vec<String> {
    let u = &project.units;
    let s = &project.land_use;
    let rows = loading(project, results);
    let mut head = format!("{:>12} {:<20} {:>7} {:>7} {:>9}", format!("Area({})", u.area_label()), "Land Use", "Imp.%", "Rv", "Runoff");
    for p in &s.pollutants {
        head.push_str(&format!(" {:>11}", format!("{p}(lb)")));
    }
    let basis = match s.annual_rainfall {
        Some(p) => format!("Annual loading by the Simple Method: P = {p:.2} in, Pj = {:.2}", s.correction),
        None => format!("Single-event loading from runoff of the {} event", results.event),
    };
    let mut out = vec![basis, String::new(), head, String::new()];
    let mut totals = vec![0.0; s.pollutants.len()];
    let mut area = 0.0;
    for r in &rows {
        area += r.area;
        let mut line = format!("{:>12} {:<20} {:>7.2} {:>7.3} {:>9.2}", group(u.area(r.area), u.area_dp()), r.land_use, r.impervious * 100.0, r.rv, r.runoff);
        for (k, l) in r.loads.iter().enumerate() {
            line.push_str(&format!(" {:>11}", group(*l, 2)));
            if let Some(t) = totals.get_mut(k) {
                *t += l;
            }
        }
        out.push(line);
    }
    let mut total = format!("{:>12} {:<20} {:>7} {:>7} {:>9}", group(u.area(area), u.area_dp()), "TOTAL", "", "", "");
    for t in totals {
        total.push_str(&format!(" {:>11}", group(t, 2)));
    }
    out.push(total);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn::SubArea;
    use crate::project::{NodeType, Subcatchment};

    #[test]
    fn simple_method_load() {
        let mut p = Project::default();
        p.land_use.uses = vec![("Parking".into(), vec![100.0, 0.5, 2.0])];
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let mut a = SubArea::new(43_560.0, 98.0, "Paved parking, HSG B");
        a.land_use = Some("Parking".into());
        p.node_mut(id).unwrap().data = NodeData::Subcat(Subcatchment { areas: vec![a], tc: vec![], uh: None, large_units: true });
        let r = crate::calc::calculate(&p);
        let rows = loading(&p, &r);
        // L = 0.226 R C A with R = 42 × 0.9 × 0.95 inches, A = 1 ac, C = 100 mg/L (0.226 is the
        // rounded form of the exact 0.2266 used here).
        let expect = 0.226 * 42.0 * 0.9 * 0.95 * 100.0;
        assert!((rows[0].loads[0] - expect).abs() / expect < 0.005, "{} vs {expect}", rows[0].loads[0]);
        assert!(loading_report(&p, &r).iter().any(|l| l.contains("TOTAL")));
    }
}
