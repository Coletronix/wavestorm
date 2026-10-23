//! The calculation engine: runs every node of a project for one rainfall event.

use crate::cn::{self, CnWeighting, Composite};
use crate::geometry::Section;
use crate::hydrograph::{Hydrograph, Peak};
use crate::link::LinkSource;
use crate::outlets::{self, OutletFlows, Routing};
use crate::project::{Node, NodeData, NodeId, NodeType, Pond, PondMethod, Project, Reach, ReachMethod, RunoffMethod, Tailwater};
use crate::rainfall::{RainEvent, Rainfall};
use crate::routing::{self, DetentionTime, StageTable};
use crate::runoff::{self, CnArea};
use crate::tc::{self, TcResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Severity of a calculation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Note,
    Hint,
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Note => "Note",
            Severity::Hint => "Hint",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
}

/// A message shown in the message window and at the top of node summaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub code: u16,
    pub severity: Severity,
    pub node: Option<NodeId>,
    pub text: String,
}

impl Message {
    pub(crate) fn new(code: u16, severity: Severity, node: Option<NodeId>, text: impl Into<String>) -> Self {
        Message { code, severity, node, text: text.into() }
    }
}

/// Subcatchment calculation details.
#[derive(Debug, Clone, Default)]
pub struct SubcatDetail {
    pub composite: Composite,
    /// CN used for runoff (after rounding and AMC).
    pub cn: f64,
    pub tc_segments: Vec<TcResult>,
    pub tc_minutes: f64,
    pub flow_length: f64,
    pub runoff_depth: f64,
    pub precip: Vec<f64>,
    pub excess: Vec<f64>,
    pub uh: String,
    /// Rational method: weighted C, intensity (in/hr) and duration (h).
    pub rational: Option<(f64, f64, f64)>,
}

/// Reach routing details.
#[derive(Debug, Clone, Default)]
pub struct ReachDetail {
    pub depth: Vec<f64>,
    pub storage: Vec<f64>,
    pub elevation: Vec<f64>,
    pub peak_depth: f64,
    pub peak_depth_time: f64,
    pub max_velocity: f64,
    pub min_travel_min: f64,
    pub avg_velocity: f64,
    pub avg_travel_min: f64,
    pub capacity: f64,
    pub translation_hr: f64,
}

/// Pond routing details.
#[derive(Debug, Clone, Default)]
pub struct PondDetail {
    pub elevation: Vec<f64>,
    pub storage: Vec<f64>,
    pub start_elevation: f64,
    pub peak_elevation: f64,
    pub peak_time: f64,
    pub peak_storage: f64,
    pub peak_area: Option<f64>,
    /// Device flows at the step of maximum total outflow, per outflow.
    pub device_analysis: [Option<(usize, f64, OutletFlows)>; 4],
    pub detention: DetentionTime,
    pub tailwater: [Option<f64>; 3],
}

// Boxing PondDetail would churn every consumer of the public `Detail` data model for no real gain.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    None,
    Subcat(SubcatDetail),
    Reach(ReachDetail),
    Pond(PondDetail),
    Link(String),
}

/// Results for one node.
#[derive(Debug, Clone, Default)]
pub struct NodeResult {
    /// Combined inflow (for a subcatchment, its runoff).
    pub inflow: Hydrograph,
    /// Total outflow (all routes, including discarded).
    pub outflow: Hydrograph,
    /// Primary, secondary, tertiary and discarded outflows.
    pub routes: [Hydrograph; 4],
    /// Contributing area (sq ft) and its impervious part.
    pub inflow_area: f64,
    pub impervious_area: f64,
    pub detail: Detail,
    /// Why this node's results are physically undefined (e.g. a pond that overtops its defined
    /// storage, or anything downstream of one). Reports print "UNDEFINED" instead of numbers.
    pub undefined: Option<String>,
}

impl NodeResult {
    pub fn peak_in(&self) -> Peak {
        self.inflow.peak()
    }
    pub fn peak_out(&self) -> Peak {
        self.outflow.peak()
    }
}

/// Results of a calculation for one event.
#[derive(Debug, Clone, Default)]
pub struct Results {
    pub event: String,
    pub event_heading: String,
    pub t0: f64,
    pub dt: f64,
    pub n: usize,
    pub order: Vec<NodeId>,
    pub nodes: HashMap<NodeId, NodeResult>,
    pub messages: Vec<Message>,
}

impl Results {
    pub fn node(&self, id: NodeId) -> Option<&NodeResult> {
        self.nodes.get(&id)
    }
    pub fn has_errors(&self) -> bool {
        self.messages.iter().any(|m| m.severity == Severity::Error)
    }
}

/// Order nodes so every node comes after the nodes that flow into it. Returns the nodes caught
/// in loops as an error.
pub fn topological_order(project: &Project) -> Result<Vec<NodeId>, Vec<NodeId>> {
    let ids: Vec<NodeId> = project.nodes.iter().filter(|n| n.node_type() != NodeType::Text).map(|n| n.id).collect();
    let mut indeg: HashMap<NodeId, usize> = ids.iter().map(|id| (*id, 0)).collect();
    for n in &project.nodes {
        for t in n.outflows.iter().flatten() {
            if let Some(d) = indeg.get_mut(t) {
                *d += 1;
            }
        }
    }
    let mut ready: Vec<NodeId> = ids.iter().copied().filter(|id| indeg.get(id) == Some(&0)).collect();
    ready.reverse();
    let mut order = Vec::with_capacity(ids.len());
    while let Some(id) = ready.pop() {
        order.push(id);
        if let Some(n) = project.node(id) {
            for t in n.outflows.iter().flatten() {
                if let Some(d) = indeg.get_mut(t) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        ready.push(*t);
                    }
                }
            }
        }
    }
    if order.len() < ids.len() {
        return Err(ids.into_iter().filter(|id| !order.contains(id)).collect());
    }
    Ok(order)
}

/// Calculates the project for its current event.
pub fn calculate(project: &Project) -> Results {
    match project.settings.event() {
        Some(e) => calculate_event(project, e),
        None => {
            let mut r = Results::default();
            r.messages.push(Message::new(595, Severity::Error, None, "Rainfall not specified: define at least one rainfall event"));
            r
        }
    }
}

/// Calculates every event defined in the project.
pub fn calculate_all(project: &Project) -> Vec<Results> {
    project.settings.events.iter().map(|e| calculate_event(project, e)).collect()
}

/// Calculates the project for one rainfall event.
// `!(x > 0.0)` is deliberately NaN-safe (true for NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn calculate_event(project: &Project, event: &RainEvent) -> Results {
    let s = &project.settings;
    let mut r = Results { event: event.name.clone(), event_heading: event.heading(), t0: s.span_start, dt: s.dt, ..Default::default() };
    let n = s.steps();
    if n < 2 || !(s.dt > 0.0) {
        r.messages.push(Message::new(500, Severity::Error, None, "Invalid Time Span or dt"));
        return r;
    }
    r.n = n;
    let order = match topological_order(project) {
        Ok(o) => o,
        Err(looped) => {
            let names: Vec<String> = looped.iter().filter_map(|id| project.node(*id)).map(|n| n.label()).collect();
            r.messages.push(Message::new(520, Severity::Error, looped.first().copied(), format!("Illegal loop involving {}", names.join(", "))));
            return r;
        }
    };
    let storm = project.storm(&event.storm);
    let rain = storm.as_ref().map(|st| Rainfall::new(st, event));
    if s.runoff != RunoffMethod::Rational {
        if storm.is_none() {
            r.messages.push(Message::new(595, Severity::Error, None, format!("Rainfall distribution \"{}\" not found", event.storm)));
            return r;
        }
        if event.depth <= 0.0 {
            r.messages.push(Message::new(560, Severity::Error, None, "Storm depth is zero"));
        }
    }
    r.messages.push(Message::new(13, Severity::Note, None, format!("Time span={:.2}-{:.2} hrs, dt={:.2} hrs, {} points", s.span_start, s.span_end, s.dt, n)));
    r.messages.push(Message::new(16, Severity::Note, None, runoff_note(project)));
    r.messages.push(Message::new(19, Severity::Note, None, event.heading()));
    r.messages.push(Message::new(22, Severity::Note, None, format!("Reach routing by {} method - Pond routing by {} method", s.reach_method.label(), s.pond_method.label())));

    let base_messages = r.messages.clone();
    if s.pond_method == PondMethod::DynStorInd {
        // Dynamic storage-indication: repeat the network pass, taking each pond's tailwater from
        // the downstream node's previous-step elevation in the last pass, until it settles.
        let mut prev: Option<Results> = None;
        for pass in 0..12 {
            let mut cur = Results { messages: base_messages.clone(), ..r.clone() };
            run_nodes(project, event, &order, rain.as_ref(), n, prev.as_ref(), &mut cur);
            let change = prev.as_ref().map(|p| max_elevation_change(p, &cur)).unwrap_or(f64::INFINITY);
            prev = Some(cur);
            if change < 1e-4 {
                break;
            }
            if pass == 11
                && let Some(p) = &mut prev
            {
                p.messages.push(Message::new(85, Severity::Warning, None, "Dynamic tailwater did not converge; Oscillations may require smaller dt"));
            }
        }
        if let Some(p) = prev {
            r = p;
        }
    } else {
        run_nodes(project, event, &order, rain.as_ref(), n, None, &mut r);
    }
    tailwater_checks(project, &order, &mut r);
    r.order = order;
    r.messages.push(Message::new(31, Severity::Note, None, "Calculations complete"));
    r
}

/// Largest change in any pond elevation between two passes.
fn max_elevation_change(a: &Results, b: &Results) -> f64 {
    let mut worst: f64 = 0.0;
    for (id, rb) in &b.nodes {
        if let (Some(Detail::Pond(pa)), Detail::Pond(pb)) = (a.node(*id).map(|r| &r.detail), &rb.detail) {
            for (x, y) in pa.elevation.iter().zip(&pb.elevation) {
                worst = worst.max((x - y).abs());
            }
        }
    }
    worst
}

/// Runs every node once in flow order. `prev` supplies downstream elevations for dynamic
/// tailwater (the previous pass); otherwise results computed so far are used.
#[allow(clippy::too_many_arguments)]
fn run_nodes(project: &Project, event: &RainEvent, order: &[NodeId], rain: Option<&Rainfall>, n: usize, prev: Option<&Results>, r: &mut Results) {
    let s = &project.settings;
    for id in order.iter().copied() {
        let Some(node) = project.node(id) else { continue };
        // Gather inflows from upstream nodes.
        let mut inflow = Hydrograph::zeros(s.span_start, s.dt, n);
        let (mut area, mut imp) = (0.0, 0.0);
        for (up, k) in project.inflows(id) {
            if let Some(res) = r.nodes.get(&up) {
                inflow.add(&res.routes[k]);
                if k == 0 {
                    area += res.inflow_area;
                    imp += res.impervious_area;
                }
            }
        }
        let has_upstream = !project.inflows(id).is_empty();
        let result = match &node.data {
            NodeData::Subcat(sc) => {
                if has_upstream {
                    r.messages.push(Message::new(67, Severity::Warning, Some(id), format!("{} Inflow not allowed - Ignored", node.label())));
                }
                subcat(project, node, sc, event, rain, n, &mut r.messages)
            }
            NodeData::Reach(reach) => {
                let mut res = route_reach(project, node, reach, &inflow, &mut r.messages);
                res.inflow_area = area;
                res.impervious_area = imp;
                res
            }
            NodeData::Pond(pond) => {
                let mut msgs = Vec::new();
                let mut res = route_pond(project, node, pond, &inflow, prev.unwrap_or(r), &mut msgs);
                r.messages.extend(msgs);
                res.inflow_area = area;
                res.impervious_area = imp;
                if !has_upstream {
                    r.messages.push(Message::new(43, Severity::Hint, Some(id), format!("{} Has no inflow (Outflow=Zero)", node.label())));
                }
                res
            }
            NodeData::Link(link) => {
                let own = link.own_hydrograph(&event.name, s.span_start, s.dt, n);
                let mut total = inflow.clone();
                total.add(&own);
                let out = link.route(&total);
                let mut res = NodeResult {
                    inflow: total,
                    outflow: {
                        let mut o = out.primary.clone();
                        o.add(&out.secondary);
                        o
                    },
                    routes: [out.primary, out.secondary, Hydrograph::zeros(s.span_start, s.dt, n), Hydrograph::zeros(s.span_start, s.dt, n)],
                    inflow_area: area,
                    impervious_area: imp,
                    detail: Detail::Link(link.describe()),
                    undefined: None,
                };
                if !matches!(link.source, LinkSource::Inflows) {
                    res.inflow_area += link.inflow_area;
                    res.impervious_area += link.inflow_area * link.impervious_pct / 100.0;
                }
                res
            }
            NodeData::Text(_) => NodeResult::default(),
        };
        let mut result = result;
        // Anything fed by an undefined node is undefined too.
        if result.undefined.is_none()
            && let Some(up) = project.inflows(id).into_iter().find(|(up, _)| r.nodes.get(up).is_some_and(|u| u.undefined.is_some()))
        {
            let label = project.node(up.0).map(|n| n.label()).unwrap_or_default();
            r.messages.push(Message::new(95, Severity::Error, Some(id), format!("{} Inflow from {label} is undefined", node.label())));
            result.undefined = Some(format!("inflow from {label} is undefined"));
        }
        r.nodes.insert(id, result);
    }
}

pub fn runoff_note(project: &Project) -> String {
    let s = &project.settings;
    match s.runoff {
        RunoffMethod::ScsTr20 => format!("Runoff by SCS TR-20 method, UH={}", s.uh),
        RunoffMethod::Sbuh => "Runoff by SBUH method".into(),
        RunoffMethod::Rational => "Runoff by Rational method".into(),
    }
}

/// Composite CN and Tc for a subcatchment, independent of the event (AMC aside).
pub fn subcat_composite(project: &Project, sc: &crate::project::Subcatchment) -> (Composite, f64) {
    let comp = cn::composite(&sc.areas, project.settings.round_cn, 0);
    (comp, comp.cn)
}

fn subcat(
    project: &Project,
    node: &Node,
    sc: &crate::project::Subcatchment,
    event: &RainEvent,
    rain: Option<&Rainfall>,
    n: usize,
    msgs: &mut Vec<Message>,
) -> NodeResult {
    let s = &project.settings;
    let (comp, base_cn) = subcat_composite(project, sc);
    let cn_used = if event.amc == crate::rainfall::Amc::II { base_cn } else { cn::round_to(event.amc.adjust_cn(base_cn), if s.round_cn { 0 } else { 2 }) };
    let tc_segments: Vec<TcResult> = sc.tc.iter().map(|t| t.kind.evaluate(cn_used)).collect();
    let mut tc_min: f64 = tc_segments.iter().map(|t| tc::rounded_minutes(t.minutes, 1)).sum();
    if let Some(m) = s.min_tc {
        tc_min = tc_min.max(m);
    }
    let flow_length = tc_segments.iter().map(|t| t.length).sum();
    let area: f64 = comp.total_area;
    let mut detail = SubcatDetail {
        composite: comp,
        cn: cn_used,
        tc_segments,
        tc_minutes: tc_min,
        flow_length,
        uh: sc.uh.clone().unwrap_or_else(|| s.uh.clone()),
        ..Default::default()
    };
    if area <= 0.0 {
        msgs.push(Message::new(40, Severity::Hint, Some(node.id), format!("{} Not described", node.label())));
        let z = Hydrograph::zeros(s.span_start, s.dt, n);
        return NodeResult { inflow: z.clone(), outflow: z.clone(), routes: [z.clone(), z.clone(), z.clone(), z], detail: Detail::Subcat(detail), ..Default::default() };
    }
    // Calculation grid starting at or before the storm start so no excess is lumped.
    let k = if s.span_start > 0.0 { (s.span_start / s.dt).ceil() as usize } else { 0 };
    let calc_t0 = s.span_start - k as f64 * s.dt;
    let calc_n = n + k;
    let tc_hr = tc_min / 60.0;
    let amc = |cn: f64| if event.amc == crate::rainfall::Amc::II { cn } else { event.amc.adjust_cn(cn) };
    let parts: Vec<CnArea> = match s.cn_weighting {
        CnWeighting::WeightedCn => vec![CnArea { area_sf: area, cn: cn_used }],
        CnWeighting::PerviousImpervious => {
            let round = |v: f64| if s.round_cn { v.round() } else { v };
            let mut v = Vec::new();
            if comp.pervious_area > 0.0 {
                v.push(CnArea { area_sf: comp.pervious_area, cn: amc(round(comp.pervious_cn)) });
            }
            if comp.impervious_area > 0.0 {
                v.push(CnArea { area_sf: comp.impervious_area, cn: amc(round(comp.impervious_cn)) });
            }
            v
        }
        CnWeighting::WeightedQ => sc.areas.iter().filter(|a| a.area > 0.0).map(|a| CnArea { area_sf: a.area, cn: amc(a.cn) }).collect(),
    };
    let series = match (s.runoff, rain) {
        (RunoffMethod::ScsTr20, Some(rain)) => {
            let uh_name = detail.uh.clone();
            let uh = project.unit_hydrograph(&uh_name).unwrap_or_else(crate::uh::UnitHydrograph::nrcs);
            Some(runoff::scs_uh_runoff(rain, &parts, s.ia_ratio, tc_hr, &uh, calc_t0, s.dt, calc_n))
        }
        (RunoffMethod::Sbuh, Some(rain)) => Some(runoff::sbuh_runoff(rain, &parts, s.ia_ratio, tc_hr, calc_t0, s.dt, calc_n)),
        _ => None,
    };
    let hydro = match series {
        Some(series) => {
            let take = |v: &[f64]| v.get(k..).map(|x| x.to_vec()).unwrap_or_default();
            detail.precip = take(&series.precip);
            detail.excess = take(&series.excess);
            detail.runoff_depth = rain.map(|r| parts.iter().map(|p| p.area_sf / area * runoff::scs_runoff_depth(r.total_depth(), p.cn, s.ia_ratio)).sum()).unwrap_or(0.0);
            Hydrograph { t0: s.span_start, dt: s.dt, q: take(&series.hydrograph.q) }
        }
        None => {
            // Rational method.
            let c_weighted = {
                let mut sum = 0.0;
                for a in &sc.areas {
                    let c = a.c.unwrap_or(if a.cn <= 1.0 { a.cn } else { 0.0 });
                    sum += a.area * c;
                }
                (sum / area * event.frequency_factor).min(1.0)
            };
            let duration = event.rational_duration.unwrap_or(tc_hr.max(s.dt));
            let intensity = event
                .rational_intensity
                .or_else(|| s.idf.as_ref().and_then(|idf| idf.intensity(&event.name, duration * 60.0)))
                .unwrap_or(0.0);
            if intensity <= 0.0 {
                msgs.push(Message::new(550, Severity::Error, Some(node.id), "Rational intensity or duration missing"));
            }
            detail.rational = Some((c_weighted, intensity, duration));
            detail.runoff_depth = c_weighted * intensity * duration;
            runoff::rational_runoff(c_weighted, area, intensity, duration, tc_hr, s.rational_rise, s.rational_fall, s.span_start, s.dt, n)
        }
    };
    if s.runoff != RunoffMethod::Rational && tc_min <= 0.0 {
        msgs.push(Message::new(46, Severity::Hint, Some(node.id), format!("{} Tc=0 (Instant runoff peak depends on dt)", node.label())));
    }
    if s.runoff == RunoffMethod::Rational && tc_hr < 8.0 * s.dt {
        msgs.push(Message::new(70, Severity::Warning, Some(node.id), format!("{} Tc<8dt requires smaller dt", node.label())));
    }
    if !hydro.has_flow() {
        msgs.push(Message::new(45,Severity::Hint, Some(node.id), format!("{} Runoff=Zero", node.label())));
    } else {
        if tc_hr > 0.0 && tc_hr < 2.0 * s.dt {
            msgs.push(Message::new(49, Severity::Hint, Some(node.id), format!("{} Tc<2dt may require smaller dt", node.label())));
        }
        let p = hydro.peak();
        let span = s.span_end - s.span_start;
        if p.time < s.span_start + 0.05 * span || p.time > s.span_end - 0.05 * span {
            msgs.push(Message::new(73, Severity::Warning, Some(node.id), format!("{} Peak may fall outside time span", node.label())));
        }
        if hydro.q.last().is_some_and(|q| *q > 0.01 * p.value) {
            msgs.push(Message::new(37, Severity::Hint, Some(node.id), format!("{} Longer time span advised for full volumes", node.label())));
        }
        if let Some(q0) = hydro.q.first()
            && *q0 > 0.01 * p.value
        {
            msgs.push(Message::new(82, Severity::Warning, Some(node.id), format!("{} Early inflow requires earlier time span", node.label())));
        }
        for (i, seg) in detail.tc_segments.iter().enumerate() {
            if let Some(cap) = seg.capacity
                && cap > 0.0
                && p.value > cap
            {
                msgs.push(Message::new(
                    47,
                    Severity::Hint,
                    Some(node.id),
                    format!("{} Peak is {:.0}% of capacity of segment #{}", node.label(), p.value / cap * 100.0, i + 1),
                ));
            }
        }
    }
    let z = Hydrograph::zeros(s.span_start, s.dt, n);
    NodeResult {
        inflow: hydro.clone(),
        outflow: hydro.clone(),
        routes: [hydro, z.clone(), z.clone(), z],
        inflow_area: area,
        impervious_area: comp.impervious_area,
        detail: Detail::Subcat(detail),
        undefined: None,
    }
}

fn route_reach(project: &Project, node: &Node, reach: &Reach, inflow: &Hydrograph, msgs: &mut Vec<Message>) -> NodeResult {
    let s = &project.settings;
    let n = inflow.len();
    let z = Hydrograph::zeros(inflow.t0, inflow.dt, n);
    if !reach.is_described() {
        if inflow.has_flow() {
            msgs.push(Message::new(40, Severity::Hint, Some(node.id), format!("{} Not described (Outflow=Inflow)", node.label())));
        }
        return NodeResult { inflow: inflow.clone(), outflow: inflow.clone(), routes: [inflow.clone(), z.clone(), z.clone(), z], ..Default::default() };
    }
    let slope = reach.slope();
    let full = reach.section.full_depth();
    let capacity = reach.section.manning_q(full, reach.n, slope);
    let is_pipe = matches!(reach.section, Section::Pipe { .. });
    let len = reach.length;
    // Exfiltration loss.
    let mut net_in = inflow.clone();
    let mut lost = z.clone();
    if reach.exfiltration > 0.0 {
        for (q, l) in net_in.q.iter_mut().zip(lost.q.iter_mut()) {
            *l = q.min(reach.exfiltration);
            *q -= *l;
        }
    }
    let peak_in = net_in.peak().value;
    let mut detail = ReachDetail { capacity, ..Default::default() };
    // Muskingum-Cunge needs a bed slope; a rating-table reach without one is routed by Stor-Ind+Trans.
    let method = if s.reach_method == ReachMethod::MuskingumCunge && slope <= 0.0 {
        msgs.push(Message::new(98, Severity::Hint, Some(node.id), format!("{} No slope for Muskingum-Cunge - Stor-Ind+Trans used", node.label())));
        ReachMethod::StorIndTrans
    } else {
        s.reach_method
    };
    let outflow = match method {
        ReachMethod::StorInd | ReachMethod::StorIndTrans => {
            let top_depth = if is_pipe { full } else { (full * 4.0).max(reach.section.normal_depth(peak_in * 1.5, reach.n, slope)) };
            let table = StageTable::build(
                0.0,
                top_depth.max(0.01),
                4000,
                |y| reach.section.props(y).area * len,
                |y| if is_pipe && y >= full { capacity } else { reach.section.manning_q(y, reach.n, slope) },
            );
            let res = routing::level_pool(&table, &net_in, 0.0, &mut |y, _, _| table.outflow_at(y).min(if is_pipe { capacity } else { f64::INFINITY }));
            if let Some(ex) = res.exceeded_by {
                if is_pipe {
                    let detained = res.storage.iter().copied().fold(0.0, f64::max) - table.storage_at(full);
                    msgs.push(Message::new(
                        76,
                        Severity::Warning,
                        Some(node.id),
                        format!("{} Detained {} (Pond w/culvert advised)", node.label(), project.units.fmt_volume(detained.max(0.0))),
                    ));
                } else {
                    msgs.push(Message::new(91, Severity::Warning, Some(node.id), format!("{} Storage range exceeded by {:.2}'", node.label(), ex)));
                }
            }
            detail.depth = res.elevation.clone();
            detail.storage = res.storage.clone();
            let (ip, peak_depth) = res.elevation.iter().copied().enumerate().fold((0, 0.0), |b, (i, v)| if v > b.1 { (i, v) } else { b });
            detail.peak_depth = peak_depth;
            detail.peak_depth_time = res.outflow.time(ip);
            let v_at = |y: f64| reach.section.velocity(y.min(if is_pipe { full } else { f64::INFINITY }), reach.n, slope);
            detail.max_velocity = v_at(peak_depth);
            detail.min_travel_min = if detail.max_velocity > 0.0 { len / detail.max_velocity / 60.0 } else { 0.0 };
            let out = if method == ReachMethod::StorIndTrans && detail.max_velocity > 0.0 {
                detail.translation_hr = len / detail.max_velocity / 3600.0;
                routing::translate(&res.outflow, detail.translation_hr)
            } else {
                res.outflow
            };
            // Average velocity: the velocity at the mean outflow of the steps that carry flow
            // (matches the published 1.42 fps / 5.3 min of the user's example reach exactly and
            // three published reaches within ~2 %). "Carrying flow" means above 1e-6 cfs: the
            // unit-hydrograph tail has many vanishing values that would otherwise dilute the mean.
            let flowing: Vec<f64> = out.q.iter().copied().filter(|q| *q > 1e-6).collect();
            let q_avg = if flowing.is_empty() { 0.0 } else { flowing.iter().sum::<f64>() / flowing.len() as f64 };
            detail.avg_velocity = if q_avg > 0.0 { v_at(reach.section.normal_depth(q_avg, reach.n, slope)) } else { 0.0 };
            detail.avg_travel_min = if detail.avg_velocity > 0.0 { len / detail.avg_velocity / 60.0 } else { 0.0 };
            out
        }
        ReachMethod::MuskingumCunge => {
            let q_ref = peak_in.max(1e-6);
            let y = reach.section.normal_depth(q_ref, reach.n, slope);
            let dy = (y * 0.01).max(1e-4);
            let (qa, qb) = (reach.section.manning_q(y + dy, reach.n, slope), reach.section.manning_q((y - dy).max(0.0), reach.n, slope));
            let (aa, ab) = (reach.section.props(y + dy).area, reach.section.props((y - dy).max(0.0)).area);
            let celerity = if aa > ab { (qa - qb) / (aa - ab) } else { 0.0 };
            let top = reach.section.props(y).top_width;
            let (out, x) = routing::muskingum_cunge(&net_in, len, slope, q_ref, celerity, top);
            if !(0.0..=0.5).contains(&x) {
                msgs.push(Message::new(97, Severity::Warning, Some(node.id), format!("{} Factor X Out Of Range", node.label())));
            }
            detail.depth = out.q.iter().map(|q| reach.section.normal_depth(*q, reach.n, slope)).collect();
            let ip = out.max_index().unwrap_or(0);
            detail.peak_depth = detail.depth.get(ip).copied().unwrap_or(0.0);
            detail.peak_depth_time = out.time(ip);
            detail.max_velocity = reach.section.velocity(detail.peak_depth, reach.n, slope);
            detail.min_travel_min = if detail.max_velocity > 0.0 { len / detail.max_velocity / 60.0 } else { 0.0 };
            detail.avg_velocity = detail.max_velocity;
            detail.avg_travel_min = detail.min_travel_min;
            detail.storage = detail.depth.iter().map(|y| reach.section.props(*y).area * len).collect();
            out
        }
    };
    detail.elevation = detail.depth.iter().map(|y| reach.inlet_invert + y).collect();
    if peak_in > capacity && capacity > 0.0 {
        msgs.push(Message::new(55, Severity::Hint, Some(node.id), format!("{} Peak inflow is {:.0}% of Manning's capacity", node.label(), peak_in / capacity * 100.0)));
    }
    let mut total = outflow.clone();
    total.add(&lost);
    flow_checks(node, inflow, &total, true, msgs);
    NodeResult { inflow: inflow.clone(), outflow: total, routes: [outflow, z.clone(), z, lost], detail: Detail::Reach(detail), ..Default::default() }
}

/// Outflow index (0 primary, 1 secondary, 2 tertiary) a device routes to, if it routes directly to one.
fn route_index(r: Routing) -> Option<usize> {
    match r {
        Routing::Primary => Some(0),
        Routing::Secondary => Some(1),
        Routing::Tertiary => Some(2),
        _ => None,
    }
}

/// Elevation at which a device discharges (a culvert's outlet invert, otherwise its invert).
fn device_outlet_level(d: &outlets::OutletDevice) -> f64 {
    match &d.kind {
        outlets::DeviceKind::Culvert { outlet_invert, .. } => *outlet_invert,
        _ => d.invert,
    }
}

/// Messages 89 (Qout>Qin) and 86 (oscillating outflow) for a routed pond or reach.
fn flow_checks(node: &Node, inflow: &Hydrograph, outflow: &Hydrograph, check_qout: bool, msgs: &mut Vec<Message>) {
    let (pi, po) = (inflow.peak().value, outflow.peak().value);
    if check_qout && po > pi * 1.005 && po - pi > 0.001 {
        msgs.push(Message::new(89, Severity::Warning, Some(node.id), format!("{} Qout>Qin may require smaller dt", node.label())));
    }
    let tol = 1e-6 + 0.001 * outflow.q.iter().copied().fold(0.0, f64::max);
    let mut last = 0i8;
    let mut changes: Vec<usize> = Vec::new();
    for (i, w) in outflow.q.windows(2).enumerate() {
        let d = w[1] - w[0];
        if !(d.abs() >= tol) {
            continue;
        }
        let sign = if d > 0.0 { 1 } else { -1 };
        if last != 0 && sign != last {
            changes.push(i);
        }
        last = sign;
    }
    if changes.windows(9).any(|w| matches!((w.first(), w.last()), (Some(a), Some(b)) if b - a <= 19)) {
        msgs.push(Message::new(86, Severity::Warning, Some(node.id), format!("{} Oscillations may require smaller dt", node.label())));
    }
}

/// Messages 78 and 80: a downstream node fed by a pond whose tailwater was not modelled (free
/// tailwater, non-dynamic pond routing) but which rose above the pond's devices or water surface.
fn tailwater_checks(project: &Project, order: &[NodeId], r: &mut Results) {
    if project.settings.pond_method == PondMethod::DynStorInd {
        return;
    }
    let mut out = Vec::new();
    for id in order {
        let Some(node) = project.node(*id) else { continue };
        let NodeData::Pond(pond) = &node.data else { continue };
        let Some(pres) = r.node(*id) else { continue };
        let Detail::Pond(pd) = &pres.detail else { continue };
        for k in 0..3 {
            if pond.tailwater.get(k) != Some(&Tailwater::Free) {
                continue;
            }
            let Some(target) = node.outflows.get(k).copied().flatten() else { continue };
            let (Some(down), Some(dres)) = (project.node(target), r.node(target)) else { continue };
            if pres.undefined.is_some() || dres.undefined.is_some() {
                continue;
            }
            let down_elev: &[f64] = match &dres.detail {
                Detail::Pond(p) => &p.elevation,
                Detail::Reach(rd) => &rd.elevation,
                _ => continue,
            };
            let peak = down_elev.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mut any_device = false;
            for (i, d) in pond.devices.iter().enumerate() {
                if route_index(d.routing) != Some(k) {
                    continue;
                }
                any_device = true;
                let x = peak - device_outlet_level(d);
                if x > 0.005 {
                    out.push(Message::new(78, Severity::Warning, Some(target), format!("{} Submerged Pond {} device #{} by {:.2}'", down.label(), node.label(), i + 1, x)));
                }
            }
            if !any_device {
                continue;
            }
            let flow = pres.routes.get(k);
            let mut worst: Option<(f64, usize)> = None;
            for (i, (dn, up)) in down_elev.iter().zip(&pd.elevation).enumerate() {
                if !flow.is_some_and(|h| h.q.get(i).is_some_and(|q| *q > 1e-6)) {
                    continue;
                }
                let x = dn - up;
                if x > 0.005 && worst.is_none_or(|w| x > w.0) {
                    worst = Some((x, i));
                }
            }
            if let Some((x, i)) = worst {
                out.push(Message::new(80, Severity::Warning, Some(target), format!("{} Exceeded Pond {} by {:.2}' @ {:.2} hrs", down.label(), node.label(), x, pres.inflow.time(i))));
            }
        }
    }
    r.messages.extend(out);
}

/// Bottom of a pond's model: storage bottom, or the lowest device invert for zero-storage ponds.
pub fn pond_bottom(pond: &Pond) -> f64 {
    pond.storage.bottom().or_else(|| pond.devices.iter().map(|d| d.invert).reduce(f64::min)).unwrap_or(0.0)
}

fn route_pond(
    project: &Project,
    node: &Node,
    pond: &Pond,
    inflow: &Hydrograph,
    results: &Results,
    msgs: &mut Vec<Message>,
) -> NodeResult {
    let s = &project.settings;
    let n = inflow.len();
    let z = Hydrograph::zeros(inflow.t0, inflow.dt, n);
    let bottom = pond_bottom(pond);
    let top_store = pond.storage.top().unwrap_or(bottom);
    let top_dev = pond.devices.iter().map(|d| d.invert).fold(bottom, f64::max);
    let top = top_store.max(top_dev) + 10.0;
    let mut vols: Vec<(usize, f64, f64)> = pond.storage.volumes.iter().enumerate().filter(|(_, v)| v.embedded_in.is_none()).map(|(j, v)| (j, v.bottom(), v.top())).collect();
    vols.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut highest: Option<(usize, f64)> = None;
    for (j, b, t) in vols {
        if let Some((lj, lt)) = highest {
            if b - lt > 0.005 {
                msgs.push(Message::new(42, Severity::Hint, Some(node.id), format!("{} Gap in defined storage above volume #{} at {:.2}'", node.label(), lj + 1, lt)));
            }
        }
        if highest.is_none_or(|(_, lt)| t > lt) {
            highest = Some((j, t));
        }
    }
    for (k, tw) in pond.tailwater.iter().enumerate() {
        if let Tailwater::Fixed(e) = tw {
            for (i, d) in pond.devices.iter().enumerate() {
                let level = device_outlet_level(d);
                if route_index(d.routing) == Some(k) && level < *e {
                    msgs.push(Message::new(77, Severity::Warning, Some(node.id), format!("{} Manual tailwater submerged device #{} by {:.2}'", node.label(), i + 1, e - level)));
                }
            }
        }
    }
    for (i, d) in pond.devices.iter().enumerate() {
        if !pond.storage.is_empty() && d.invert > top_store {
            msgs.push(Message::new(92, Severity::Warning, Some(node.id), format!("{} Outlet Device {} is above defined storage", node.label(), i + 1)));
        }
        if let Some(b) = pond.storage.bottom()
            && d.invert < b
            && !matches!(d.kind, outlets::DeviceKind::Exfiltration { .. })
        {
            msgs.push(Message::new(44, Severity::Hint, Some(node.id), format!("{} Outlet Device {} is below defined storage", node.label(), i + 1)));
        }
    }
    // Downstream elevation for dynamic tailwater: previous-step elevation of the target node.
    let downstream_elev = |k: usize, step: usize| -> Option<f64> {
        let target = node.outflows.get(k).copied().flatten()?;
        match results.node(target).map(|r| &r.detail) {
            Some(Detail::Pond(p)) => p.elevation.get(step.saturating_sub(1)).copied(),
            _ => {
                if let Some(Node { data: NodeData::Link(l), .. }) = project.node(target) {
                    l.elevation.at(inflow.time(step))
                } else {
                    None
                }
            }
        }
    };
    let tw_of = |route: Routing, hw: f64, step: usize| -> Option<f64> {
        let k = match route {
            Routing::Primary => 0,
            Routing::Secondary => 1,
            Routing::Tertiary => 2,
            _ => return None,
        };
        match pond.tailwater[k] {
            Tailwater::Free => {
                if s.pond_method == PondMethod::DynStorInd { downstream_elev(k, step) } else { None }
            }
            Tailwater::Fixed(e) => Some(e),
            Tailwater::Differential(d) => Some(hw - d.max(0.0)),
            Tailwater::Dynamic => downstream_elev(k, step),
        }
    };
    let nd = pond.devices.len();
    let initial = vec![outlets::DeviceState::default(); nd];
    let start = pond.start_elevation.unwrap_or(bottom).max(bottom);
    let mut pump_states: Vec<Vec<outlets::DeviceState>> = Vec::with_capacity(n);
    let mut undefined = None;
    let elevation: Vec<f64>;
    let storage: Vec<f64>;
    if pond.storage.is_empty() {
        // Zero-storage pond: outflow equals inflow; find the headwater that passes it.
        let mut el = Vec::with_capacity(n);
        for (i, q) in inflow.q.iter().enumerate() {
            let t = inflow.time(i);
            let flows = |e: f64| outlets::evaluate(&pond.devices, e, &|r| tw_of(r, e, i), &pond.storage, &initial, t).total();
            let (mut lo, mut hi) = (bottom, bottom + 1.0);
            let mut guard = 0;
            while flows(hi) < *q && guard < 40 {
                hi += (hi - bottom) * 2.0;
                guard += 1;
            }
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if flows(mid) < *q { lo = mid } else { hi = mid }
            }
            el.push(if *q > 0.0 { 0.5 * (lo + hi) } else { bottom });
            pump_states.push(initial.clone());
        }
        elevation = el;
        storage = vec![0.0; n];
    } else {
        let table = StageTable::build(bottom, top, 3000, |e| pond.storage.storage(e), |_| 0.0);
        let mut current = initial.clone();
        let res = routing::level_pool(&table, inflow, start, &mut |e, step, commit| {
            let t = inflow.time(step);
            let q = outlets::evaluate(&pond.devices, e, &|r| tw_of(r, e, step), &pond.storage, &current, t).total();
            if commit {
                // The flow for this step used the state at its start; update for the next step.
                pump_states.push(current.clone());
                outlets::update_states(&pond.devices, e, t, &mut current);
            }
            q
        });
        // Above the top of the defined storage the pond has no volume to hold water, so the
        // routed elevation and outflow are not physically meaningful.
        let peak = res.elevation.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ex = res.exceeded_by.map_or(peak - top_store, |v| v + (top - top_store));
        if ex > 0.005 {
            msgs.push(Message::new(
                93,
                Severity::Error,
                Some(node.id),
                format!("{} Storage range exceeded by {:.2}' - results undefined (extend the storage or add outlet capacity)", node.label(), ex),
            ));
            undefined = Some(format!("peak elevation exceeds the defined storage (top {:.2}') by {:.2}'", top_store, ex));
        }
        elevation = res.elevation;
        storage = res.storage;
    }
    // Split the outflow by route from the device flows at each elevation.
    let mut routes = [z.clone(), z.clone(), z.clone(), z.clone()];
    let mut analysis: [Option<(usize, f64, OutletFlows)>; 4] = [None, None, None, None];
    for (i, e) in elevation.iter().enumerate() {
        let st = pump_states.get(i).cloned().unwrap_or_else(|| vec![outlets::DeviceState::default(); nd]);
        let f = outlets::evaluate(&pond.devices, *e, &|r| tw_of(r, *e, i), &pond.storage, &st, inflow.time(i));
        let vals = [f.primary, f.secondary, f.tertiary, f.discarded];
        for k in 0..4 {
            routes[k].q[i] = vals[k];
            if vals[k] > 0.0 && analysis[k].as_ref().is_none_or(|a| vals[k] > a.1) {
                analysis[k] = Some((i, vals[k], f.clone()));
            }
        }
    }
    // Zero-storage ponds pass the inflow exactly; scale route flows to match it.
    if pond.storage.is_empty() {
        for i in 0..n {
            let tot: f64 = routes.iter().map(|r| r.q[i]).sum();
            if tot > 0.0 {
                let k = inflow.q[i] / tot;
                for r in routes.iter_mut() {
                    r.q[i] *= k;
                }
            }
        }
    }
    let mut outflow = z.clone();
    for r in &routes {
        outflow.add(r);
    }
    let (ipk, peak_e) = elevation.iter().copied().enumerate().fold((0, f64::NEG_INFINITY), |b, (i, v)| if v > b.1 { (i, v) } else { b });
    let peak_storage = storage.get(ipk).copied().unwrap_or(0.0);
    let detention = routing::detention_time(inflow, &outflow, pond.storage.storage(start));
    if let Some(flood) = pond.flood_elevation
        && peak_e > flood
    {
        msgs.push(Message::new(58, Severity::Hint, Some(node.id), format!("{} Peaked {:.2}' above defined flood level", node.label(), peak_e - flood)));
    }
    for (i, d) in pond.devices.iter().enumerate() {
        if let outlets::DeviceKind::Channel { section, .. } = &d.kind {
            let full = section.full_depth();
            if peak_e.is_finite() && full.is_finite() && peak_e - d.invert > full {
                msgs.push(Message::new(96, Severity::Warning, Some(node.id), format!("{} Channel Outlet {} depth exceeded by {:.2}'", node.label(), i + 1, peak_e - d.invert - full)));
            }
        }
    }
    flow_checks(node, inflow, &outflow, start <= bottom, msgs);
    let tw = [tw_of(Routing::Primary, peak_e, ipk), tw_of(Routing::Secondary, peak_e, ipk), tw_of(Routing::Tertiary, peak_e, ipk)];
    let detail = PondDetail {
        start_elevation: start,
        peak_elevation: if peak_e.is_finite() { peak_e } else { start },
        peak_time: inflow.time(ipk),
        peak_storage,
        peak_area: pond.storage.surface_area(peak_e),
        device_analysis: analysis,
        detention,
        tailwater: tw,
        elevation,
        storage,
    };
    NodeResult { inflow: inflow.clone(), outflow, routes, detail: Detail::Pond(detail), undefined, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn::SubArea;
    use crate::outlets::{DeviceKind, OpeningShape, OutletDevice, Plane};
    use crate::project::{NodeData, Subcatchment};
    use crate::storage::{CustomKind, StorageVolume, VolumeShape};
    use crate::tc::{TcKind, TcSegment};

    const AC: f64 = 43_560.0;

    fn project_with_pond() -> (Project, NodeId, NodeId) {
        let mut p = Project::default();
        p.settings.events = vec![RainEvent::new("25-Year", "Type II 24-hr", 4.3)];
        p.settings.current_event = 0;
        p.settings.span_start = 10.0;
        p.settings.span_end = 40.0;
        p.settings.dt = 0.1;
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let pond = p.add_node(NodeType::Pond, [100.0, 0.0]);
        if let Some(n) = p.node_mut(s) {
            n.outflows[0] = Some(pond);
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![SubArea::new(25.0 * AC, 60.0, "Woods, Fair, HSG B")],
                tc: vec![TcSegment { kind: TcKind::Direct { minutes: 23.2, length: 0.0, velocity: 0.0 }, description: String::new() }],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(pond) {
            n.data = NodeData::Pond(Pond {
                storage: crate::storage::PondStorage {
                    volumes: vec![StorageVolume::new(
                        "Custom",
                        50.0,
                        VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: vec![50.0, 55.0], values: vec![0.5 * AC, 1.0 * AC] },
                    )],
                },
                devices: vec![OutletDevice::new(
                    "Orifice",
                    50.0,
                    Routing::Primary,
                    DeviceKind::Orifice {
                        shape: OpeningShape::Circular { diameter: 1.0 },
                        plane: Plane::Vertical,
                        cd: 0.6,
                        columns: 1,
                        rows: 1,
                        row_spacing: 0.0,
                        weir_at_low_head: true,
                        grate: None,
                    },
                )],
                ..Default::default()
            });
        }
        (p, s, pond)
    }

    #[test]
    fn overtopping_the_defined_storage_is_undefined() {
        let (mut p, _, pond) = project_with_pond();
        let link = p.add_node(NodeType::Link, [200.0, 0.0]);
        if let Some(n) = p.node_mut(pond) {
            n.outflows[0] = Some(link);
            if let NodeData::Pond(pd) = &mut n.data {
                // A tiny basin, 1 ft deep, cannot hold 25 acres of runoff behind a 1 ft orifice.
                pd.storage.volumes[0].shape = VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: vec![50.0, 51.0], values: vec![1000.0, 1000.0] };
            }
        }
        let r = calculate(&p);
        let pr = r.node(pond).expect("pond result");
        assert!(pr.undefined.is_some());
        assert!(r.messages.iter().any(|m| m.code == 93 && m.severity == Severity::Error && m.node == Some(pond)));
        assert!(r.node(link).is_some_and(|l| l.undefined.is_some()));
        assert!(r.messages.iter().any(|m| m.code == 95 && m.node == Some(link)));
        let node = p.node(pond).expect("pond");
        let text = crate::report::node_summary(&p, &r, node).join("\n");
        assert!(text.contains("UNDEFINED"), "{text}");
        // The original pond stays well inside its storage.
        let (p, _, pond) = project_with_pond();
        assert!(calculate(&p).node(pond).is_some_and(|r| r.undefined.is_none()));
    }

    #[test]
    fn subcat_into_pond() {
        let (p, s, pond) = project_with_pond();
        let r = calculate(&p);
        assert!(!r.has_errors(), "{:?}", r.messages);
        let sub = r.node(s).unwrap();
        assert!((sub.outflow.peak().value - 18.56).abs() < 0.6, "{}", sub.outflow.peak().value);
        let pr = r.node(pond).unwrap();
        assert!(pr.outflow.peak().value < sub.outflow.peak().value);
        assert!((pr.inflow_area - 25.0 * AC).abs() < 1e-6);
        // Mass balance: inflow = outflow + change in storage.
        let Detail::Pond(d) = &pr.detail else { panic!() };
        let stored = d.storage.last().unwrap() - d.storage.first().unwrap();
        let err = pr.inflow.volume_cf() - pr.outflow.volume_cf() - stored;
        assert!(err.abs() / pr.inflow.volume_cf() < 0.01, "mass balance {err}");
        assert!(d.detention.center_of_mass.unwrap() > 0.0);
    }

    #[test]
    fn loops_are_reported() {
        let (mut p, _, pond) = project_with_pond();
        let reach = p.add_node(NodeType::Reach, [0.0, 0.0]);
        p.node_mut(pond).unwrap().outflows[0] = Some(reach);
        p.node_mut(reach).unwrap().outflows[0] = Some(pond);
        let r = calculate(&p);
        assert!(r.messages.iter().any(|m| m.code == 520));
    }

    #[test]
    fn empty_and_undescribed_nodes_are_safe() {
        let mut p = Project::default();
        for t in [NodeType::Subcat, NodeType::Reach, NodeType::Pond, NodeType::Link, NodeType::Text] {
            p.add_node(t, [0.0, 0.0]);
        }
        let ids: Vec<NodeId> = p.nodes.iter().map(|n| n.id).collect();
        p.node_mut(ids[0]).unwrap().outflows[0] = Some(ids[1]);
        p.node_mut(ids[1]).unwrap().outflows[0] = Some(ids[2]);
        p.node_mut(ids[2]).unwrap().outflows[0] = Some(ids[3]);
        let r = calculate(&p);
        assert!(!r.has_errors(), "{:?}", r.messages);
        p.settings.dt = 0.0;
        assert!(calculate(&p).has_errors());
    }

    fn has_msg(r: &Results, code: u16, node: NodeId, needle: &str) -> bool {
        r.messages.iter().any(|m| m.code == code && m.node == Some(node) && m.text.contains(needle))
    }

    fn set_tc(p: &mut Project, s: NodeId, minutes: f64) {
        if let Some(Node { data: NodeData::Subcat(sc), .. }) = p.node_mut(s) {
            sc.tc = vec![TcSegment { kind: TcKind::Direct { minutes, length: 0.0, velocity: 0.0 }, description: String::new() }];
        }
    }

    fn pond_mut(p: &mut Project, id: NodeId) -> &mut Pond {
        match p.node_mut(id) {
            Some(Node { data: NodeData::Pond(pd), .. }) => pd,
            _ => panic!("not a pond"),
        }
    }

    #[test]
    fn gap_in_defined_storage_is_hinted() {
        let (mut p, _, pond) = project_with_pond();
        assert!(!calculate(&p).messages.iter().any(|m| m.code == 42));
        let shape = VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: vec![60.0, 62.0], values: vec![0.5 * AC, 1.0 * AC] };
        let pd = pond_mut(&mut p, pond);
        pd.storage.volumes.push(StorageVolume::new("Upper", 60.0, shape.clone()));
        let mut embedded = StorageVolume::new("Inside", 57.0, shape);
        embedded.embedded_in = Some(0);
        pd.storage.volumes.push(embedded);
        let r = calculate(&p);
        let n42: Vec<_> = r.messages.iter().filter(|m| m.code == 42).collect();
        assert_eq!(n42.len(), 1, "{n42:?}");
        assert!(has_msg(&r, 42, pond, "Gap in defined storage above volume #1 at 55.00'"), "{n42:?}");
    }

    #[test]
    fn tc_zero_hint_and_rational_tc_warning() {
        let (mut p, s, _) = project_with_pond();
        set_tc(&mut p, s, 0.0);
        let r = calculate(&p);
        assert!(has_msg(&r, 46, s, "Tc=0 (Instant runoff peak depends on dt)"));
        assert!(!r.messages.iter().any(|m| m.code == 70));
        set_tc(&mut p, s, 23.2);
        assert!(!calculate(&p).messages.iter().any(|m| m.code == 46));
        // Rational: Tc of 23.2 min is under 8 dt for dt = 0.1 h.
        p.settings.runoff = RunoffMethod::Rational;
        p.settings.events[0].rational_intensity = Some(3.0);
        let r = calculate(&p);
        assert!(has_msg(&r, 70, s, "Tc<8dt requires smaller dt"), "{:?}", r.messages);
        p.settings.dt = 0.04;
        assert!(!calculate(&p).messages.iter().any(|m| m.code == 70));
    }

    #[test]
    fn peak_over_tc_channel_capacity_is_hinted() {
        use crate::geometry::Section;
        let (mut p, s, _) = project_with_pond();
        if let Some(Node { data: NodeData::Subcat(sc), .. }) = p.node_mut(s) {
            sc.tc.push(TcSegment { kind: TcKind::Channel { length: 100.0, slope: 0.01, n: 0.03, section: Section::Rectangular { width: 1.0, depth: 0.5 } }, description: String::new() });
        }
        let r = calculate(&p);
        assert!(has_msg(&r, 47, s, "of capacity of segment #2"), "{:?}", r.messages);
    }

    #[test]
    fn manual_tailwater_above_a_device_is_flagged() {
        let (mut p, _, pond) = project_with_pond();
        pond_mut(&mut p, pond).tailwater[0] = Tailwater::Fixed(52.0);
        let r = calculate(&p);
        assert!(has_msg(&r, 77, pond, "Manual tailwater submerged device #1 by 2.00'"), "{:?}", r.messages);
        pond_mut(&mut p, pond).tailwater[0] = Tailwater::Fixed(49.0);
        assert!(!calculate(&p).messages.iter().any(|m| m.code == 77));
    }

    #[test]
    fn channel_outlet_depth_exceeded_is_flagged() {
        use crate::geometry::Section;
        let (mut p, _, pond) = project_with_pond();
        pond_mut(&mut p, pond).devices = vec![OutletDevice::new("Channel", 50.0, Routing::Primary, DeviceKind::Channel { section: Section::Rectangular { width: 4.0, depth: 0.2 }, n: 0.03, slope: 0.01 })];
        let r = calculate(&p);
        assert!(has_msg(&r, 96, pond, "Channel Outlet 1 depth exceeded by"), "{:?}", r.messages);
        pond_mut(&mut p, pond).devices = vec![OutletDevice::new("Channel", 50.0, Routing::Primary, DeviceKind::Channel { section: Section::Rectangular { width: 4.0, depth: 20.0 }, n: 0.03, slope: 0.01 })];
        assert!(!calculate(&p).messages.iter().any(|m| m.code == 96));
    }

    fn two_ponds() -> (Project, NodeId, NodeId) {
        use crate::geometry::PipeShape;
        let (mut p, _s, upper) = project_with_pond();
        let lower = p.add_node(NodeType::Pond, [200.0, 0.0]);
        if let Some(n) = p.node_mut(upper) {
            n.outflows[0] = Some(lower);
        }
        pond_mut(&mut p, upper).devices = vec![OutletDevice::new(
            "Culvert",
            50.0,
            Routing::Primary,
            DeviceKind::Culvert { shape: PipeShape::Round, width: 1.5, height: 1.5, length: 40.0, outlet_invert: 49.8, n: 0.013, ke: 0.5, cc: 0.9, fill: 0.0, entrance: String::new() },
        )];
        if let Some(n) = p.node_mut(lower) {
            n.data = NodeData::Pond(Pond {
                storage: crate::storage::PondStorage { volumes: vec![StorageVolume::new("Lower", 49.5, VolumeShape::Prismatoid { width: 60.0, length: 60.0, height: 8.0, z: 0.0 })] },
                devices: vec![OutletDevice::new(
                    "Orifice",
                    49.5,
                    Routing::Primary,
                    DeviceKind::Orifice { shape: OpeningShape::Circular { diameter: 0.4 }, plane: Plane::Vertical, cd: 0.6, columns: 1, rows: 1, row_spacing: 0.0, weir_at_low_head: true, grate: None },
                )],
                ..Default::default()
            });
        }
        (p, upper, lower)
    }

    #[test]
    fn unmodelled_tailwater_submergence_is_reported_once() {
        let (mut p, upper, lower) = two_ponds();
        let r = calculate(&p);
        assert!(!r.has_errors(), "{:?}", r.messages);
        assert!(has_msg(&r, 78, lower, "Submerged Pond"), "{:?}", r.messages);
        assert!(r.messages.iter().filter(|m| m.code == 78).all(|m| m.text.contains("device #1 by")));
        assert!(r.messages.iter().filter(|m| m.code == 78).count() == 1);
        assert!(has_msg(&r, 80, lower, "Exceeded Pond"), "{:?}", r.messages);
        assert!(r.messages.iter().filter(|m| m.code == 80).count() == 1);
        let done = r.messages.iter().position(|m| m.code == 31).unwrap_or(0);
        assert!(r.messages.iter().position(|m| m.code == 78).unwrap_or(usize::MAX) < done);
        // Manual or dynamic tailwater means it was modelled: no message.
        pond_mut(&mut p, upper).tailwater[0] = Tailwater::Fixed(40.0);
        let r = calculate(&p);
        assert!(!r.messages.iter().any(|m| m.code == 78 || m.code == 80));
        p.settings.pond_method = PondMethod::DynStorInd;
        pond_mut(&mut p, upper).tailwater[0] = Tailwater::Free;
        let r = calculate(&p);
        assert!(!r.messages.iter().any(|m| m.code == 78 || m.code == 80));
    }

    #[test]
    fn qout_over_qin_and_oscillation_checks() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Pond, [0.0, 0.0]);
        let node = p.node(id).unwrap().clone();
        let h = |q: Vec<f64>| Hydrograph { t0: 0.0, dt: 0.1, q };
        let smooth = h((0..40).map(|i| (-((i as f64 - 20.0) / 6.0).powi(2)).exp() * 10.0).collect());
        let mut msgs = Vec::new();
        flow_checks(&node, &smooth, &smooth, true, &mut msgs);
        assert!(msgs.is_empty(), "{msgs:?}");
        let bigger = h(smooth.q.iter().map(|q| q * 1.02).collect());
        flow_checks(&node, &smooth, &bigger, true, &mut msgs);
        assert_eq!(msgs.iter().map(|m| m.code).collect::<Vec<_>>(), vec![89]);
        msgs.clear();
        flow_checks(&node, &smooth, &bigger, false, &mut msgs);
        assert!(msgs.is_empty());
        let zigzag = h((0..40).map(|i| 5.0 + if i % 2 == 0 { 1.0 } else { -1.0 }).collect());
        flow_checks(&node, &zigzag, &zigzag, true, &mut msgs);
        assert_eq!(msgs.iter().map(|m| m.code).collect::<Vec<_>>(), vec![86]);
        // A few isolated wiggles are fine.
        let mut q = smooth.q.clone();
        q.iter_mut().skip(2).step_by(7).for_each(|v| *v += 0.5);
        msgs.clear();
        flow_checks(&node, &h(q.clone()), &h(q), true, &mut msgs);
        assert!(!msgs.iter().any(|m| m.code == 86), "{msgs:?}");
    }

    #[test]
    fn ordinary_ponds_do_not_oscillate() {
        let (p, _, _) = project_with_pond();
        let r = calculate(&p);
        assert!(!r.messages.iter().any(|m| matches!(m.code, 86 | 89)), "{:?}", r.messages);
        let (p, _, _) = two_ponds();
        assert!(!calculate(&p).messages.iter().any(|m| matches!(m.code, 86 | 89)));
    }

    #[test]
    fn dynamic_tailwater_couples_ponds() {
        use crate::geometry::PipeShape;
        let (mut p, _s, upper) = project_with_pond();
        // Upper pond drains by a culvert into a small lower pond with a restrictive orifice.
        let lower = p.add_node(NodeType::Pond, [200.0, 0.0]);
        if let Some(n) = p.node_mut(upper) {
            n.outflows[0] = Some(lower);
            if let NodeData::Pond(pd) = &mut n.data {
                pd.devices = vec![OutletDevice::new(
                    "Culvert",
                    50.0,
                    Routing::Primary,
                    DeviceKind::Culvert { shape: PipeShape::Round, width: 1.5, height: 1.5, length: 40.0, outlet_invert: 49.8, n: 0.013, ke: 0.5, cc: 0.9, fill: 0.0, entrance: String::new() },
                )];
            }
        }
        if let Some(n) = p.node_mut(lower) {
            n.data = NodeData::Pond(Pond {
                storage: crate::storage::PondStorage {
                    volumes: vec![StorageVolume::new("Lower", 49.5, VolumeShape::Prismatoid { width: 60.0, length: 60.0, height: 8.0, z: 0.0 })],
                },
                devices: vec![OutletDevice::new(
                    "Orifice",
                    49.5,
                    Routing::Primary,
                    DeviceKind::Orifice { shape: OpeningShape::Circular { diameter: 0.75 }, plane: Plane::Vertical, cd: 0.6, columns: 1, rows: 1, row_spacing: 0.0, weir_at_low_head: true, grate: None },
                )],
                ..Default::default()
            });
        }
        let peak = |p: &Project, id: NodeId| -> (f64, f64, f64) {
            let r = calculate(p);
            assert!(!r.has_errors(), "{:?}", r.messages);
            let res = r.node(id).unwrap();
            let Detail::Pond(d) = &res.detail else { panic!() };
            let stored = d.storage.last().unwrap() - d.storage.first().unwrap();
            (d.peak_elevation, res.inflow.volume_cf(), res.outflow.volume_cf() + stored)
        };
        let (si, in1, out1) = peak(&p, upper);
        p.settings.pond_method = PondMethod::DynStorInd;
        let (dsi, in2, out2) = peak(&p, upper);
        assert!(dsi > si + 0.05, "tailwater should raise the upper pond: SI {si:.3} DSI {dsi:.3}");
        assert!((in1 - out1).abs() / in1 < 0.01 && (in2 - out2).abs() / in2 < 0.01, "mass balance");
    }
}
