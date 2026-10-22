//! Subcatchment runoff checked against summaries published in public engineering submittals
//! made with the current reference version (Type II and Type III 24-hour storms, dt 0.01 and
//! 0.05 h, weighted-CN and weighted-Q, Tc from 4.8 to 90.6 minutes).

use stormwaves_core::calc;
use stormwaves_core::cn::{CnWeighting, SubArea};
use stormwaves_core::project::*;
use stormwaves_core::rainfall::RainEvent;
use stormwaves_core::tc::{TcKind, TcSegment};

const AC: f64 = 43_560.0;

struct Case {
    storm: &'static str,
    p: f64,
    dt: f64,
    /// Time span end (hours); spans start at 0. A 24-hour span cuts the recession off, as in
    /// the published run.
    span: f64,
    tc: f64,
    weighting: CnWeighting,
    /// (acres, CN)
    areas: &'static [(f64, f64)],
    /// Published peak (cfs), time of peak (h) and volume (af).
    peak: f64,
    time: f64,
    volume: f64,
}

const II: &str = "Type II 24-hr";
const III: &str = "Type III 24-hr";

#[rustfmt::skip]
const CASES: &[Case] = &[
    // Vermont residential project, Lag/CN Tc, weighted-Q, dt 0.01.
    Case { storm: II, p: 4.22, dt: 0.01, span: 120.0, tc: 10.2, weighting: CnWeighting::WeightedQ, areas: &[(2.265, 41.0)], peak: 0.03, time: 13.36, volume: 0.022 },
    Case { storm: II, p: 5.33, dt: 0.01, span: 120.0, tc: 10.2, weighting: CnWeighting::WeightedQ, areas: &[(2.265, 41.0)], peak: 0.41, time: 12.08, volume: 0.067 },
    Case { storm: II, p: 4.22, dt: 0.01, span: 120.0, tc: 4.8, weighting: CnWeighting::WeightedQ, areas: &[(3.905, 77.0)], peak: 14.52, time: 11.96, volume: 0.646 },
    Case { storm: II, p: 5.33, dt: 0.01, span: 120.0, tc: 4.8, weighting: CnWeighting::WeightedQ, areas: &[(3.905, 77.0)], peak: 21.02, time: 11.96, volume: 0.944 },
    Case { storm: II, p: 4.22, dt: 0.01, span: 120.0, tc: 12.8, weighting: CnWeighting::WeightedQ, areas: &[(3.494, 53.0)], peak: 1.7, time: 12.08, volume: 0.154 },
    Case { storm: II, p: 5.33, dt: 0.01, span: 120.0, tc: 12.8, weighting: CnWeighting::WeightedQ, areas: &[(3.494, 53.0)], peak: 4.18, time: 12.07, volume: 0.296 },
    // Pennsylvania permit submittals, weighted-CN, dt 0.05.
    Case { storm: II, p: 3.36, dt: 0.05, span: 72.0, tc: 27.5, weighting: CnWeighting::WeightedCn, areas: &[(0.891, 60.0), (2.526, 70.0), (0.205, 40.0), (0.395, 40.0)], peak: 1.62, time: 12.27, volume: 0.198 },
    Case { storm: II, p: 3.36, dt: 0.05, span: 72.0, tc: 72.5, weighting: CnWeighting::WeightedCn, areas: &[(22.596, 60.0)], peak: 3.23, time: 12.98, volume: 0.89 },
    Case { storm: II, p: 5.28, dt: 0.05, span: 72.0, tc: 27.5, weighting: CnWeighting::WeightedCn, areas: &[(0.891, 60.0), (2.526, 70.0), (0.205, 40.0), (0.395, 40.0)], peak: 5.78, time: 12.23, volume: 0.565 },
    Case { storm: II, p: 5.28, dt: 0.05, span: 72.0, tc: 72.5, weighting: CnWeighting::WeightedCn, areas: &[(22.596, 60.0)], peak: 13.4, time: 12.85, volume: 2.763 },
    Case { storm: II, p: 7.2, dt: 0.05, span: 72.0, tc: 27.5, weighting: CnWeighting::WeightedCn, areas: &[(0.891, 60.0), (2.526, 70.0), (0.205, 40.0), (0.395, 40.0)], peak: 11.0, time: 12.22, volume: 1.021 },
    Case { storm: II, p: 7.2, dt: 0.05, span: 72.0, tc: 72.5, weighting: CnWeighting::WeightedCn, areas: &[(22.596, 60.0)], peak: 27.32, time: 12.82, volume: 5.171 },
    Case { storm: II, p: 8.4, dt: 0.05, span: 72.0, tc: 27.5, weighting: CnWeighting::WeightedCn, areas: &[(0.891, 60.0), (2.526, 70.0), (0.205, 40.0), (0.395, 40.0)], peak: 14.54, time: 12.22, volume: 1.334 },
    Case { storm: II, p: 8.4, dt: 0.05, span: 72.0, tc: 72.5, weighting: CnWeighting::WeightedCn, areas: &[(22.596, 60.0)], peak: 37.02, time: 12.81, volume: 6.847 },
    Case { storm: II, p: 3.36, dt: 0.05, span: 72.0, tc: 15.4, weighting: CnWeighting::WeightedCn, areas: &[(0.96, 61.0)], peak: 0.45, time: 12.11, volume: 0.041 },
    Case { storm: II, p: 5.28, dt: 0.05, span: 72.0, tc: 15.4, weighting: CnWeighting::WeightedCn, areas: &[(0.96, 61.0)], peak: 1.76, time: 12.09, volume: 0.123 },
    Case { storm: II, p: 7.2, dt: 0.05, span: 72.0, tc: 15.4, weighting: CnWeighting::WeightedCn, areas: &[(0.96, 61.0)], peak: 3.41, time: 12.08, volume: 0.228 },
    Case { storm: II, p: 8.4, dt: 0.05, span: 72.0, tc: 15.4, weighting: CnWeighting::WeightedCn, areas: &[(0.96, 61.0)], peak: 4.54, time: 12.08, volume: 0.3 },
    // Massachusetts site plan: roofs and drives, Type III, Tc 6.0 min (areas in sq ft).
    Case { storm: III, p: 2.64, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4124.0 / AC, 98.0)], peak: 0.24, time: 12.09, volume: 0.019 },
    Case { storm: III, p: 2.64, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(2821.0 / AC, 98.0)], peak: 0.16, time: 12.09, volume: 0.013 },
    Case { storm: III, p: 2.64, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4214.0 / AC, 98.0)], peak: 0.24, time: 12.09, volume: 0.0194 },
    Case { storm: III, p: 3.2, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4124.0 / AC, 98.0)], peak: 0.29, time: 12.09, volume: 0.0234 },
    Case { storm: III, p: 3.2, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(2821.0 / AC, 98.0)], peak: 0.2, time: 12.09, volume: 0.016 },
    Case { storm: III, p: 3.2, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4214.0 / AC, 98.0)], peak: 0.29, time: 12.09, volume: 0.0239 },
    Case { storm: III, p: 4.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4124.0 / AC, 98.0)], peak: 0.43, time: 12.09, volume: 0.036 },
    Case { storm: III, p: 4.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(2821.0 / AC, 98.0)], peak: 0.3, time: 12.09, volume: 0.0246 },
    Case { storm: III, p: 4.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4214.0 / AC, 98.0)], peak: 0.44, time: 12.09, volume: 0.0368 },
    Case { storm: III, p: 8.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4124.0 / AC, 98.0)], peak: 0.8, time: 12.09, volume: 0.0675 },
    Case { storm: III, p: 8.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(2821.0 / AC, 98.0)], peak: 0.55, time: 12.09, volume: 0.0462 },
    Case { storm: III, p: 8.8, dt: 0.05, span: 48.0, tc: 6.0, weighting: CnWeighting::WeightedCn, areas: &[(4214.0 / AC, 98.0)], peak: 0.82, time: 12.09, volume: 0.069 },
    // Connecticut subdivision, 0-24 h span (published depths marked as cut off by the span).
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 30.6, weighting: CnWeighting::WeightedCn, areas: &[(18.25, 77.0), (21.51, 66.0)], peak: 34.44, time: 12.27, volume: 3.614 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 16.0, weighting: CnWeighting::WeightedCn, areas: &[(4.45, 98.0), (3.86, 61.0), (6.2, 66.0), (3.36, 55.0)], peak: 23.31, time: 12.1, volume: 1.633 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 90.6, weighting: CnWeighting::WeightedCn, areas: &[(3.54, 77.0), (11.02, 66.0)], peak: 4.94, time: 13.14, volume: 1.16 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 21.3, weighting: CnWeighting::WeightedCn, areas: &[(3.77, 66.0)], peak: 2.9, time: 12.17, volume: 0.26 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 31.2, weighting: CnWeighting::WeightedCn, areas: &[(3.67, 66.0), (1.9, 83.0)], peak: 5.08, time: 12.28, volume: 0.533 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 38.6, weighting: CnWeighting::WeightedCn, areas: &[(2.39, 80.0), (14.13, 77.0), (6.72, 66.0)], peak: 20.58, time: 12.37, volume: 2.451 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 23.3, weighting: CnWeighting::WeightedCn, areas: &[(3.91, 61.0), (4.45, 98.0), (2.91, 55.0), (4.85, 80.0)], peak: 22.43, time: 12.17, volume: 1.88 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 48.8, weighting: CnWeighting::WeightedCn, areas: &[(10.48, 80.0)], peak: 10.76, time: 12.49, volume: 1.451 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 55.8, weighting: CnWeighting::WeightedCn, areas: &[(14.28, 80.0), (0.23, 98.0)], peak: 13.53, time: 12.59, volume: 2.004 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 9.2, weighting: CnWeighting::WeightedCn, areas: &[(0.69, 98.0), (0.37, 80.0)], peak: 4.25, time: 12.0, volume: 0.238 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 81.0, weighting: CnWeighting::WeightedCn, areas: &[(2.39, 66.0), (3.45, 77.0), (3.88, 80.0)], peak: 5.27, time: 12.94, volume: 1.06 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 25.2, weighting: CnWeighting::WeightedCn, areas: &[(3.03, 80.0)], peak: 4.89, time: 12.19, volume: 0.422 },
    Case { storm: II, p: 3.56, dt: 0.05, span: 24.0, tc: 36.5, weighting: CnWeighting::WeightedCn, areas: &[(1.83, 83.0), (1.14, 66.0), (0.29, 80.0), (0.11, 61.0)], peak: 3.48, time: 12.34, volume: 0.391 },
    // Vermont industrial permit, weighted-Q, dt 0.01, Tc 5.0 min minimum.
    Case { storm: II, p: 3.1, dt: 0.01, span: 360.0, tc: 13.3, weighting: CnWeighting::WeightedQ, areas: &[(1.413, 30.0), (1.456, 78.0)], peak: 2.37, time: 12.06, volume: 0.146 },
    Case { storm: II, p: 2.1, dt: 0.01, span: 360.0, tc: 5.0, weighting: CnWeighting::WeightedQ, areas: &[(0.188, 98.0), (0.546, 80.0), (0.723, 98.0), (0.963, 98.0), (0.152, 98.0), (0.298, 39.0)], peak: 6.74, time: 11.96, volume: 0.345 },
    Case { storm: II, p: 3.1, dt: 0.01, span: 360.0, tc: 5.0, weighting: CnWeighting::WeightedQ, areas: &[(0.188, 98.0), (0.546, 80.0), (0.723, 98.0), (0.963, 98.0), (0.152, 98.0), (0.298, 39.0)], peak: 10.49, time: 11.96, volume: 0.544 },
];

fn subcat_project(storm: &str, p_in: f64, dt: f64, span: f64, weighting: CnWeighting) -> Project {
    let mut p = Project::default();
    p.settings.events = vec![RainEvent::new("Event", storm, p_in)];
    p.settings.current_event = 0;
    p.settings.span_start = 0.0;
    p.settings.span_end = span;
    p.settings.dt = dt;
    p.settings.cn_weighting = weighting;
    p
}

fn add_subcat(p: &mut Project, areas: &[(f64, f64)], tc: f64) -> NodeId {
    let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
    if let Some(n) = p.node_mut(id) {
        n.data = NodeData::Subcat(Subcatchment {
            areas: areas.iter().map(|&(a, cn)| SubArea::new(a * AC, cn, "area")).collect(),
            tc: vec![TcSegment { kind: TcKind::Direct { minutes: tc, length: 0.0, velocity: 0.0 }, description: String::new() }],
            uh: None,
            large_units: true,
        });
    }
    id
}

fn run(c: &Case) -> (f64, f64, f64) {
    let mut p = subcat_project(c.storm, c.p, c.dt, c.span, c.weighting);
    let id = add_subcat(&mut p, c.areas, c.tc);
    let r = calc::calculate(&p);
    let n = r.node(id).expect("subcatchment result");
    let pk = n.outflow.peak();
    (pk.value, pk.time, n.outflow.volume_cf() / AC)
}

#[test]
fn published_subcatchment_runoff() {
    let mut failures = Vec::new();
    for (i, c) in CASES.iter().enumerate() {
        let (q, t, v) = run(c);
        println!("case {i:2}: {} Tc {:4.1}  peak {q:7.2} ({:7.2}) @ {t:5.2} ({:5.2})  vol {v:7.4} ({:7.4})", c.storm, c.tc, c.peak, c.time, c.volume);
        // Published values are rounded to 0.01 cfs, 0.01 h and 0.001 af (or whole cubic feet).
        let peak_ok = (q - c.peak).abs() <= 0.03 * c.peak + 0.01;
        let time_ok = (t - c.time).abs() <= 0.025;
        let volume_ok = (v - c.volume).abs() <= 0.01 * c.volume + 0.001;
        if !(peak_ok && time_ok && volume_ok) {
            failures.push(i);
        }
    }
    assert!(failures.is_empty(), "cases off: {failures:?}");
}

/// Two published subcatchments (Pennsylvania cases 6/7, 8/9, 10/11, 12/13) joined at a link:
/// the combined peak depends on their relative timing. Published (cfs, h) per event.
#[test]
fn published_link_sums() {
    for (pair, peak, time) in [((6, 7), 3.69, 12.93), ((8, 9), 14.93, 12.80), ((10, 11), 30.07, 12.78), ((12, 13), 40.54, 12.77)] {
        let (a, b) = (&CASES[pair.0], &CASES[pair.1]);
        let mut p = subcat_project(a.storm, a.p, a.dt, a.span, a.weighting);
        let link = p.add_node(NodeType::Link, [0.0, 0.0]);
        for c in [a, b] {
            let id = add_subcat(&mut p, c.areas, c.tc);
            if let Some(n) = p.node_mut(id) {
                n.outflows[0] = Some(link);
            }
        }
        let r = calc::calculate(&p);
        let pk = r.node(link).expect("link result").outflow.peak();
        assert!((pk.value - peak).abs() <= 0.01 * peak + 0.01, "link {pair:?}: {:.2} vs {peak}", pk.value);
        assert!((pk.time - time).abs() <= 0.025, "link {pair:?}: {:.2} h vs {time}", pk.time);
    }
}
