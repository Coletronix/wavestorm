//! Links: nodes that sum inflows, inject manual or imported hydrographs, split or adjust a
//! hydrograph, and optionally define a tailwater elevation for upstream nodes.

use crate::hydrograph::{Hydrograph, SEC_PER_HR};
use serde::{Deserialize, Serialize};

/// A manually entered hydrograph, possibly one per rainfall event.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ManualHydrograph {
    /// Start time and step (hours).
    pub t0: f64,
    pub dt: f64,
    /// Flows (cfs) for all events, used when no event-specific series exists.
    pub flows: Vec<f64>,
    /// Event name → flows, for multi-event links.
    #[serde(default)]
    pub per_event: Vec<(String, Vec<f64>)>,
}

impl ManualHydrograph {
    pub fn series_for(&self, event: &str) -> &[f64] {
        self.per_event.iter().find(|(name, _)| name.eq_ignore_ascii_case(event)).map(|(_, v)| v.as_slice()).unwrap_or(&self.flows)
    }
}

/// Where a link's own hydrograph comes from.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum LinkSource {
    /// Only the on-screen inflows.
    #[default]
    Inflows,
    Manual(ManualHydrograph),
    /// Imported time/flow table (e.g. from CSV); times in hours, flows in cfs, multi-event by name.
    Table { name: String, times: Vec<f64>, columns: Vec<(String, Vec<f64>)>, selected: Option<String> },
}

/// Adjustments applied to the link outflow; the remainder becomes the secondary outflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkAdjust {
    /// Primary gets only the flow above this value.
    pub threshold: Option<f64>,
    /// Primary is capped at this value.
    pub limit: Option<f64>,
    /// Primary = multiplier × flow.
    pub multiplier: f64,
    /// Stretch the time axis of the imported/manual hydrograph.
    pub time_multiplier: f64,
    /// Delay (hours).
    pub time_lag: f64,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    /// Keep only this volume (cu ft) from the start of the hydrograph.
    pub slice_volume: Option<f64>,
}

impl Default for LinkAdjust {
    fn default() -> Self {
        LinkAdjust { threshold: None, limit: None, multiplier: 1.0, time_multiplier: 1.0, time_lag: 0.0, start_time: None, end_time: None, slice_volume: None }
    }
}

/// Tailwater elevation the link presents to inflowing nodes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum LinkElevation {
    #[default]
    None,
    Fixed(f64),
    /// (time h, elevation ft) pairs.
    Table(Vec<(f64, f64)>),
    /// Mean, amplitude (half range), period (h), phase (h).
    Tidal { mean: f64, amplitude: f64, period: f64, phase: f64 },
}

impl LinkElevation {
    pub fn at(&self, t: f64) -> Option<f64> {
        match self {
            LinkElevation::None => None,
            LinkElevation::Fixed(e) => Some(*e),
            LinkElevation::Table(pts) => {
                let first = pts.first()?;
                if t <= first.0 {
                    return Some(first.1);
                }
                for w in pts.windows(2) {
                    if t <= w[1].0 {
                        let f = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 0.0 };
                        return Some(w[0].1 + (w[1].1 - w[0].1) * f);
                    }
                }
                pts.last().map(|p| p.1)
            }
            LinkElevation::Tidal { mean, amplitude, period, phase } => {
                if *period <= 0.0 {
                    return Some(*mean);
                }
                Some(mean + amplitude * (2.0 * std::f64::consts::PI * (t - phase) / period).sin())
            }
        }
    }
}

/// The link node's definition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Link {
    pub source: LinkSource,
    pub adjust: LinkAdjust,
    pub elevation: LinkElevation,
    /// Contributing area (sq ft) and impervious fraction for manual hydrographs.
    #[serde(default)]
    pub inflow_area: f64,
    #[serde(default)]
    pub impervious_pct: f64,
}

/// Primary and secondary outflow of a link.
#[derive(Debug, Clone, Default)]
pub struct LinkOutflow {
    pub total_in: Hydrograph,
    pub primary: Hydrograph,
    pub secondary: Hydrograph,
}

impl Link {
    /// The link's own hydrograph on the calculation grid.
    pub fn own_hydrograph(&self, event: &str, t0: f64, dt: f64, n: usize) -> Hydrograph {
        let k = if self.adjust.time_multiplier > 0.0 { self.adjust.time_multiplier } else { 1.0 };
        match &self.source {
            LinkSource::Inflows => Hydrograph::zeros(t0, dt, n),
            LinkSource::Manual(m) => {
                let series = Hydrograph { t0: m.t0 * k, dt: m.dt * k, q: m.series_for(event).to_vec() };
                if series.dt <= 0.0 { Hydrograph::zeros(t0, dt, n) } else { series.resample(t0, dt, n) }
            }
            LinkSource::Table { times, columns, selected, .. } => {
                let col = match selected {
                    Some(name) if name != "(MultiEvent)" => columns.iter().find(|(c, _)| c == name),
                    _ => columns.iter().find(|(c, _)| c.eq_ignore_ascii_case(event)).or(columns.first()),
                };
                let Some((_, flows)) = col else { return Hydrograph::zeros(t0, dt, n) };
                let pts: Vec<(f64, f64)> = times.iter().zip(flows).map(|(t, q)| (t * k, *q)).collect();
                Hydrograph { t0, dt, q: (0..n).map(|i| table_interp(&pts, t0 + i as f64 * dt)).collect() }
            }
        }
    }

    /// Applies the link adjustments to the combined inflow.
    pub fn route(&self, total_in: &Hydrograph) -> LinkOutflow {
        let a = &self.adjust;
        let lagged = crate::routing::translate(total_in, a.time_lag.max(0.0));
        let mut primary = lagged.clone();
        let mult = if a.multiplier.is_finite() { a.multiplier.max(0.0) } else { 1.0 };
        let mut sliced = 0.0;
        for (i, q) in primary.q.iter_mut().enumerate() {
            let t = lagged.time(i);
            let mut v = *q * mult;
            if let Some(th) = a.threshold {
                v = (v - th).max(0.0);
            }
            if let Some(lim) = a.limit {
                v = v.min(lim.max(0.0));
            }
            if a.start_time.is_some_and(|s| t < s) || a.end_time.is_some_and(|e| t > e) {
                v = 0.0;
            }
            if let Some(cap) = a.slice_volume {
                let room = (cap - sliced).max(0.0) / (lagged.dt * SEC_PER_HR);
                v = v.min(room);
                sliced += v * lagged.dt * SEC_PER_HR;
            }
            *q = v.min(lagged.q[i]).max(0.0);
        }
        let secondary = Hydrograph {
            t0: lagged.t0,
            dt: lagged.dt,
            q: lagged.q.iter().zip(&primary.q).map(|(a, b)| (a - b).max(0.0)).collect(),
        };
        LinkOutflow { total_in: total_in.clone(), primary, secondary }
    }

    /// Short description of the adjustment for summaries, e.g. "Inflow above 1.20 cfs".
    pub fn describe(&self) -> String {
        let a = &self.adjust;
        let mut parts = Vec::new();
        if let Some(t) = a.threshold {
            parts.push(format!("Inflow above {t:.2} cfs"));
        }
        if let Some(l) = a.limit {
            parts.push(format!("Inflow limited to {l:.2} cfs"));
        }
        if (a.multiplier - 1.0).abs() > 1e-9 {
            parts.push(format!("Inflow x {:.3}", a.multiplier));
        }
        if a.time_lag > 0.0 {
            parts.push(format!("delayed {:.2} hrs", a.time_lag));
        }
        if parts.is_empty() { "Outflow = Inflow".into() } else { format!("Primary outflow = {}", parts.join(", ")) }
    }
}

fn table_interp(pts: &[(f64, f64)], t: f64) -> f64 {
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return 0.0 };
    if t < first.0 || t > last.0 {
        return 0.0;
    }
    for w in pts.windows(2) {
        if t <= w[1].0 {
            let f = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 0.0 };
            return w[0].1 + (w[1].1 - w[0].1) * f;
        }
    }
    last.1
}

/// Times (hours) and named discharge columns (cfs) read from a hydrograph CSV.
pub type CsvColumns = (Vec<f64>, Vec<(String, Vec<f64>)>);

/// Parses a hydrograph CSV: first column time (hours/minutes/seconds in the heading), further
/// columns are named hydrographs in cfs or cms. Returns (times in hours, named columns in cfs).
pub fn parse_csv(text: &str) -> Result<CsvColumns, String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.chars().all(|c| c == ','));
    let header = lines.next().ok_or("empty file")?;
    let heads: Vec<String> = header.split(',').map(|s| s.trim().trim_matches('"').to_string()).collect();
    if heads.len() < 2 {
        return Err("expected a time column and at least one flow column".into());
    }
    let time_scale = {
        let h = heads[0].to_ascii_lowercase();
        if h.contains("min") { 1.0 / 60.0 } else if h.contains("sec") { 1.0 / 3600.0 } else { 1.0 }
    };
    let flow_scale: Vec<f64> = heads[1..].iter().map(|h| if h.to_ascii_lowercase().contains("cms") { 35.314_666_7 } else { 1.0 }).collect();
    let names: Vec<String> = heads[1..]
        .iter()
        .map(|h| h.split('(').next().unwrap_or(h).trim().to_string())
        .collect();
    let mut times = Vec::new();
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    for line in lines {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let Some(t) = cells.first().and_then(|c| c.trim_matches('"').parse::<f64>().ok()) else {
            // A second heading line (units) is allowed.
            continue;
        };
        times.push(t * time_scale);
        for (j, col) in cols.iter_mut().enumerate() {
            let v = cells.get(j + 1).and_then(|c| c.trim_matches('"').parse::<f64>().ok()).unwrap_or(0.0);
            col.push(v * flow_scale.get(j).copied().unwrap_or(1.0));
        }
    }
    if times.len() < 2 {
        return Err("fewer than two data rows".into());
    }
    if times.windows(2).any(|w| w[1] < w[0]) {
        return Err("time values must increase".into());
    }
    Ok((times, names.into_iter().zip(cols).collect()))
}

/// Fills blank entries that lie between two entered values by linear interpolation by index.
/// Blanks before the first or after the last entered value stay blank.
pub fn interpolate_blanks(values: &[Option<f64>]) -> Vec<Option<f64>> {
    let mut out = values.to_vec();
    let known: Vec<usize> = (0..values.len()).filter(|&i| values[i].is_some_and(|v| v.is_finite())).collect();
    for w in known.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (va, vb) = (values[a].unwrap_or(0.0), values[b].unwrap_or(0.0));
        for (i, slot) in out.iter_mut().enumerate().take(b).skip(a + 1) {
            *slot = Some(va + (vb - va) * (i - a) as f64 / (b - a) as f64);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_fills_interior_blanks_only() {
        let v = [None, Some(1.0), None, None, Some(4.0), None];
        let r = interpolate_blanks(&v);
        assert_eq!(r, vec![None, Some(1.0), Some(2.0), Some(3.0), Some(4.0), None]);
        assert!(interpolate_blanks(&[]).is_empty());
    }

    fn reference_link() -> Link {
        Link {
            source: LinkSource::Manual(ManualHydrograph {
                t0: 10.0,
                dt: 0.5,
                flows: vec![0.0, 0.0, 0.2, 0.5, 0.9, 1.4, 1.8, 1.9, 1.8, 1.6, 1.4, 1.2, 0.9, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1, 0.0],
                per_event: vec![],
            }),
            adjust: LinkAdjust { threshold: Some(1.2), ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn reference_manual_link_with_threshold() {
        // Inflow 1.90 cfs, primary 0.70 cfs @ 13.50 (above 1.20), secondary 1.20 cfs.
        let link = reference_link();
        let own = link.own_hydrograph("25-Year", 10.0, 0.1, 301);
        let out = link.route(&own);
        assert!((own.max().value - 1.9).abs() < 1e-9);
        assert!((out.primary.max().value - 0.7).abs() < 1e-9);
        assert!((out.secondary.max().value - 1.2).abs() < 1e-9);
        assert!((out.primary.max().time - 13.5).abs() < 1e-9);
        // Volume: 0.678 af in, 0.109 af primary.
        let af = |h: &Hydrograph| h.volume_cf() / 43_560.0;
        assert!((af(&own) - 0.678).abs() < 0.002, "{}", af(&own));
        assert!((af(&out.primary) - 0.109).abs() < 0.003, "{}", af(&out.primary));
    }

    #[test]
    fn csv_parsing() {
        let csv = "Time (minutes),2-year,100-year (cms)\n0,0,0\n30,1.5,1\n60,0,0\n";
        let (t, cols) = parse_csv(csv).unwrap();
        assert_eq!(t, vec![0.0, 0.5, 1.0]);
        assert_eq!(cols[0].0, "2-year");
        assert!((cols[1].1[1] - 35.3146667).abs() < 1e-6);
        assert!(parse_csv("").is_err());
        assert!(parse_csv("t,q\n1,2\n0,3\n").is_err());
    }

    #[test]
    fn tidal_and_table_elevations() {
        let tide = LinkElevation::Tidal { mean: 5.0, amplitude: 2.0, period: 12.42, phase: 0.0 };
        assert!((tide.at(12.42 / 4.0).unwrap() - 7.0).abs() < 1e-9);
        let table = LinkElevation::Table(vec![(0.0, 1.0), (10.0, 3.0)]);
        assert_eq!(table.at(5.0), Some(2.0));
        assert_eq!(LinkElevation::Table(vec![]).at(1.0), None);
    }
}
