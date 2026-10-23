//! Import of legacy NRCS TR-20 input decks (the public "Computer Program for Project
//! Formulation – Hydrology" card format).
//!
//! Cards are read as whitespace-separated fields. Supported: XSECTN and STRUCT tables, RAINFL and
//! DIMHYD tables, RUNOFF, RESVOR, REACH, ADDHYD and SAVMOV standard control, INCREM and COMPUT
//! executive control, TITLE. TR-20 hydrograph "locations" (registers) are tracked so each
//! operation is connected to the node that produced its input. ADDHYD becomes a link at that
//! cross-section. Anything not converted is listed in the project notes.
//!
//! Units follow TR-20: areas in square miles, Tc in hours, storage in acre-feet, discharge in
//! cfs, elevations and lengths in feet, end areas in square feet.

use crate::cn::SubArea;
use crate::geometry::{RatingPoint, Section};
use crate::link::Link;
use crate::outlets::{DeviceKind, OutletDevice, Routing};
use crate::project::{NodeData, NodeId, NodeType, Pond, Project, Reach, Subcatchment};
use crate::rainfall::{Amc, DurationMode, RainEvent, Storm};
use crate::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use crate::tc::{TcKind, TcSegment};
use crate::uh::UnitHydrograph;
use std::collections::BTreeMap;

const SQFT_PER_SQMI: f64 = 27_878_400.0;
const SQFT_PER_ACRE: f64 = 43_560.0;

/// The converted project and the notes describing what was skipped or assumed.
pub struct Import {
    pub project: Project,
    pub notes: Vec<String>,
}

#[derive(Default)]
struct Table {
    header: Vec<String>,
    rows: Vec<Vec<f64>>,
}

/// Where a TR-20 hydrograph location currently comes from: a node's outflow (route index).
#[derive(Clone, Copy)]
struct Source {
    node: NodeId,
    route: usize,
}

struct Builder {
    project: Project,
    notes: Vec<String>,
    xsectn: BTreeMap<u32, Table>,
    structs: BTreeMap<u32, Table>,
    registers: BTreeMap<u32, Source>,
    /// Nodes in creation order, for the diagram layout.
    order: Vec<NodeId>,
}

/// Input and output hydrograph locations of a REACH or RESVOR card, and the index of the first
/// field after them. The card has three one-column slots two columns apart (input, second
/// input, output), written e.g. "1   2" (second slot blank) or "2 3 4"; the spacing tells
/// them apart from the integer output options further right. Without that pattern the next
/// two fields are taken as input and output.
fn hydrograph_slots(raw: &str, fields: &[String], after_id: usize) -> Option<(u32, Option<u32>, u32, usize)> {
    let spans: Vec<(usize, &str)> = raw.split_whitespace().map(|t| (t.as_ptr() as usize - raw.as_ptr() as usize, t)).collect();
    // Align `fields` (a suffix of the card's tokens) with the spans.
    let offset = spans.len().checked_sub(fields.len())?;
    let at = |k: usize| spans.get(offset + k).copied();
    let one_digit = |t: &str| t.len() == 1 && t.chars().all(|c| c.is_ascii_digit());
    let (s0, t0) = at(after_id)?;
    if !one_digit(t0) {
        return None;
    }
    let slot = |col: usize| (after_id..fields.len()).find_map(|k| at(k).filter(|(p, t)| *p == col && one_digit(t)).map(|(_, t)| (k, t)));
    if let Some((k_out, out)) = slot(s0 + 4) {
        let second = slot(s0 + 2).and_then(|(_, t)| int(t));
        return Some((int(t0)?, second, int(out)?, k_out + 1));
    }
    let out = fields.get(after_id + 1).and_then(|s| int(s))?;
    Some((int(t0)?, None, out, after_id + 2))
}

fn num(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn int(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// TR-20 standard rainfall table numbers. Table 2 (Type II) is confirmed by published decks;
/// the others follow the historical order and are flagged in the notes for checking.
fn builtin_rain_table(n: u32) -> Option<&'static str> {
    match n {
        1 => Some("Type I 24-hr"),
        2 => Some("Type II 24-hr"),
        3 => Some("Type IA 24-hr"),
        4 => Some("Type III 24-hr"),
        _ => None,
    }
}

/// Parses a TR-20 deck into a new project.
pub fn import(text: &str) -> Result<Import, String> {
    let mut b = Builder {
        project: Project::default(),
        notes: Vec::new(),
        xsectn: BTreeMap::new(),
        structs: BTreeMap::new(),
        registers: BTreeMap::new(),
        order: Vec::new(),
    };
    b.project.title = "TR-20 Import".into();
    b.project.settings.events.clear();
    let mut titles: Vec<String> = Vec::new();
    // Open table: (kind, number, table).
    let mut open: Option<(String, u32, Table)> = None;
    let mut rain_tables: BTreeMap<u32, Storm> = BTreeMap::new();
    let mut dt: Option<f64> = None;
    let mut computs: Vec<Vec<String>> = Vec::new();
    // Standard control statements: name, fields after the name, upper-cased card image.
    let mut controls: Vec<(String, Vec<String>, String)> = Vec::new();
    let mut recognised = false;

    for raw in text.lines() {
        let line = raw.trim_end();
        let tokens: Vec<String> = line.split_whitespace().map(|t| t.to_ascii_uppercase()).collect();
        let Some(first) = tokens.first() else { continue };
        // Data code (a single digit) followed by the statement name, or a bare statement.
        let (code, kw, rest) = if first.len() == 1 && first.chars().all(|c| c.is_ascii_digit()) {
            (first.as_str(), tokens.get(1).map(String::as_str).unwrap_or(""), tokens.get(2..).unwrap_or(&[]))
        } else {
            ("", first.as_str(), tokens.get(1..).unwrap_or(&[]))
        };
        // Statements carry a data code; only these may appear bare. This keeps printed output
        // (e.g. "RUNOFF ABOVE BASEFLOW") pasted around a deck from being read as statements.
        if code.is_empty() && !matches!(kw, "JOB" | "TITLE" | "ENDATA" | "ENDCMP" | "ENDJOB" | "ENDTBL") {
            continue;
        }
        if kw == "ENDJOB" {
            if text.lines().skip_while(|l| l.trim_end() != line).skip(1).any(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case("JOB"))) {
                b.notes.push("Only the first TR-20 job in the file was imported.".into());
            }
            break;
        }
        if code == "8" {
            if let Some((_, _, t)) = open.as_mut() {
                t.rows.push(tokens[1..].iter().filter_map(|s| num(s)).collect());
            }
            continue;
        }
        if kw == "ENDTBL" {
            if let Some((kind, n, t)) = open.take() {
                match kind.as_str() {
                    "XSECTN" => {
                        b.xsectn.insert(n, t);
                    }
                    "STRUCT" => {
                        b.structs.insert(n, t);
                    }
                    "RAINFL" => {
                        if let Some(s) = rain_storm(n, &t) {
                            rain_tables.insert(n, s);
                        } else {
                            b.notes.push(format!("RAINFL {n}: could not read the table; skipped."));
                        }
                    }
                    "DIMHYD" => b.dimhyd(&t),
                    _ => {}
                }
            }
            continue;
        }
        match kw {
            "JOB" => recognised = true,
            "TITLE" => {
                let t = line.trim_start().get(5..).unwrap_or("").trim().to_string();
                if !t.is_empty() {
                    titles.push(t);
                }
            }
            "XSECTN" | "STRUCT" | "RAINFL" | "DIMHYD" => {
                recognised = true;
                let n = rest.first().and_then(|s| int(s)).unwrap_or(0);
                open = Some((kw.to_string(), n, Table { header: rest.to_vec(), rows: Vec::new() }));
            }
            "RUNOFF" | "RESVOR" | "REACH" | "ADDHYD" | "SAVMOV" | "DIVERT" => {
                recognised = true;
                controls.push((kw.to_string(), rest.to_vec(), line.to_ascii_uppercase()));
            }
            "INCREM" => {
                if let Some(v) = rest.iter().skip(1).filter_map(|s| num(s)).next() {
                    dt = Some(dt.map_or(v, |d: f64| d.min(v)));
                }
            }
            "COMPUT" => computs.push(rest.to_vec()),
            "ENDATA" | "ENDCMP" | "LIST" | "" => {}
            "READHD" | "BASFLO" | "INSERT" | "ALTER" | "DELETE" | "UPDATE" | "READDS" => {
                b.notes.push(format!("{kw} statement not converted."));
            }
            _ => {}
        }
    }
    if !recognised && controls.is_empty() {
        return Err("This does not look like a TR-20 input file (no JOB, tables or RUNOFF/REACH/RESVOR statements).".into());
    }
    if let Some(t) = titles.first() {
        b.project.title = t.clone();
    }
    if titles.len() > 1 {
        b.notes.push(format!("TR-20 titles: {}", titles.join(" / ")));
    }
    // TR-20 uses fractional curve numbers.
    b.project.settings.round_cn = false;
    for (kw, f, raw) in &controls {
        b.control(kw, f, raw);
    }
    for s in rain_tables.values() {
        b.project.storms.push(s.clone());
    }
    b.computs(&computs, &rain_tables, dt);
    b.layout();
    let mut project = b.project;
    if !b.notes.is_empty() {
        project.notes = format!("Imported from TR-20.\n{}", b.notes.join("\n"));
    }
    Ok(Import { project, notes: b.notes })
}

fn rain_storm(n: u32, t: &Table) -> Option<Storm> {
    // Header: table number, time increment (hours).
    let dt = t.header.get(1).and_then(|s| num(s)).filter(|v| *v > 0.0)?;
    let values: Vec<f64> = t.rows.iter().flatten().copied().collect();
    if values.len() < 2 {
        return None;
    }
    let duration = dt * (values.len() - 1) as f64;
    Storm::from_mass_points(&format!("TR-20 Rain Table {n}"), "Imported from a TR-20 RAINFL table", duration, &values, false)
}

impl Builder {
    fn add(&mut self, t: NodeType, name: String, data: NodeData) -> NodeId {
        let id = self.project.add_node(t, [0.0, 0.0]);
        if let Some(n) = self.project.node_mut(id) {
            n.name = name;
            n.data = data;
        }
        self.order.push(id);
        id
    }

    /// Routes the hydrograph in `register` into `to`.
    fn feed(&mut self, register: u32, to: NodeId, op: &str) {
        let Some(src) = self.registers.get(&register).copied() else {
            self.notes.push(format!("{op}: hydrograph location {register} is empty; its inflow was not connected."));
            return;
        };
        let Some(n) = self.project.node_mut(src.node) else { return };
        let label = n.label();
        let connected = match n.outflows.get_mut(src.route) {
            Some(slot) if slot.is_none() => {
                *slot = Some(to);
                true
            }
            _ => false,
        };
        if !connected {
            self.notes.push(format!("{op}: the hydrograph from {label} is used twice; only its first use is connected."));
        }
        self.registers.remove(&register);
    }

    /// Routes the input (and a second input written in the middle slot) into `to`.
    fn feed_all(&mut self, input: u32, second: Option<u32>, to: NodeId, op: &str) {
        self.feed(input, to, op);
        if let Some(s) = second {
            self.feed(s, to, op);
            let label = self.project.node(to).map(|n| n.label()).unwrap_or_default();
            self.notes.push(format!("{op} {label}: hydrograph location {s} in the second input position was added to its inflow; please check."));
        }
    }

    fn control(&mut self, kw: &str, f: &[String], raw: &str) {
        let op_code = match kw {
            "RUNOFF" => "1",
            "RESVOR" => "2",
            "REACH" => "3",
            "ADDHYD" => "4",
            "SAVMOV" => "5",
            _ => "6",
        };
        let f: &[String] = if f.first().map(String::as_str) == Some(op_code) { &f[1..] } else { f };
        let ints = |k: usize| f.get(k).and_then(|s| int(s));
        match kw {
            "RUNOFF" => {
                let (Some(xs), Some(out)) = (ints(0), ints(1)) else { return self.bad(kw, f) };
                let nums: Vec<f64> = f[2..].iter().filter_map(|s| num(s)).collect();
                let (Some(&area), Some(&cn), Some(&tc)) = (nums.first(), nums.get(1), nums.get(2)) else { return self.bad(kw, f) };
                let data = NodeData::Subcat(Subcatchment {
                    areas: vec![SubArea::new(area * SQFT_PER_SQMI, cn, &format!("TR-20 RUNOFF, CN {cn}"))],
                    tc: vec![TcSegment { kind: TcKind::Direct { minutes: tc * 60.0, length: 0.0, velocity: 0.0 }, description: "TR-20 Tc".into() }],
                    uh: None,
                    large_units: true,
                });
                let id = self.add(NodeType::Subcat, format!("XSECTN {xs:03}"), data);
                self.registers.insert(out, Source { node: id, route: 0 });
            }
            "REACH" => {
                let (Some(xs), Some((inp, second, out, next))) = (ints(0), hydrograph_slots(raw, f, 1)) else { return self.bad(kw, f) };
                let length = f.get(next).and_then(|s| num(s)).unwrap_or(0.0);
                let reach = match self.xsectn.get(&xs) {
                    Some(t) => rating_reach(t, length),
                    None => {
                        self.notes.push(format!("REACH {xs:03}: no XSECTN {xs:03} table; describe this reach by hand."));
                        Reach { length, ..Reach::default() }
                    }
                };
                let id = self.add(NodeType::Reach, format!("XSECTN {xs:03}"), NodeData::Reach(reach));
                self.feed_all(inp, second, id, kw);
                self.registers.insert(out, Source { node: id, route: 0 });
            }
            "RESVOR" => {
                let (Some(st), Some((inp, second, out, next))) = (ints(0), hydrograph_slots(raw, f, 1)) else { return self.bad(kw, f) };
                // The starting elevation, when given, is the first value with a decimal point
                // (the trailing fields are integer output options).
                let start = f.get(next..).unwrap_or(&[]).iter().find(|s| s.contains('.')).and_then(|s| num(s));
                let pond = match self.structs.get(&st) {
                    Some(t) => struct_pond(t, start, &mut self.notes, st),
                    None => {
                        self.notes.push(format!("RESVOR {st:02}: no STRUCT {st:02} table; describe this pond by hand."));
                        Pond::default()
                    }
                };
                let id = self.add(NodeType::Pond, format!("STRUCT {st:02}"), NodeData::Pond(pond));
                self.feed_all(inp, second, id, kw);
                self.registers.insert(out, Source { node: id, route: 0 });
            }
            "ADDHYD" => {
                let (Some(xs), Some(a), Some(c), Some(out)) = (ints(0), ints(1), ints(2), ints(3)) else { return self.bad(kw, f) };
                let id = self.add(NodeType::Link, format!("XSECTN {xs:03}"), NodeData::Link(Link::default()));
                self.feed(a, id, kw);
                self.feed(c, id, kw);
                self.registers.insert(out, Source { node: id, route: 0 });
            }
            "SAVMOV" => {
                // The last two integers are the source and destination locations.
                let nums: Vec<u32> = f.iter().filter_map(|s| int(s)).collect();
                if let [.., from, to] = nums.as_slice() {
                    if let Some(src) = self.registers.remove(from) {
                        self.registers.insert(*to, src);
                    }
                } else {
                    self.bad(kw, f);
                }
            }
            _ => self.notes.push(format!("{kw} statement not converted ({}).", f.join(" "))),
        }
    }

    fn bad(&mut self, kw: &str, f: &[String]) {
        self.notes.push(format!("{kw}: could not read \"{}\"; skipped.", f.join(" ")));
    }

    fn dimhyd(&mut self, t: &Table) {
        let step = t.header.first().and_then(|s| num(s)).filter(|v| *v > 0.0).unwrap_or(0.02);
        let mut q: Vec<f64> = t.rows.iter().flatten().copied().collect();
        let peak = q.iter().copied().fold(0.0, f64::max);
        if q.len() < 3 || peak <= 0.0 {
            self.notes.push("DIMHYD: could not read the table; the standard unit hydrograph is used.".into());
            return;
        }
        q.iter_mut().for_each(|v| *v /= peak);
        if q.last().is_some_and(|v| *v > 0.0) {
            q.push(0.0);
        }
        let name = "TR-20 DIMHYD".to_string();
        let t: Vec<f64> = (0..q.len()).map(|i| i as f64 * step).collect();
        self.project.uhs.push(UnitHydrograph { name: name.clone(), comment: "Imported from a TR-20 DIMHYD table".into(), t, q });
        self.project.settings.uh = name;
        self.notes.push("DIMHYD: imported as the project's unit hydrograph \"TR-20 DIMHYD\" (time step taken as a fraction of Tp; please check).".into());
    }

    fn computs(&mut self, computs: &[Vec<String>], rain_tables: &BTreeMap<u32, Storm>, dt: Option<f64>) {
        let dt = dt.filter(|v| *v > 0.0).unwrap_or(0.1);
        self.project.settings.dt = dt;
        let mut start = f64::INFINITY;
        let mut end: f64 = 0.0;
        for (i, f) in computs.iter().enumerate() {
            // Fields: [op code 7] from-xsec, to-xsec, start time, depth, duration, table, ARC,
            // alternate, storm. In card images the one-digit rain table often touches the
            // duration ("1.05" = 1.0 and table 5); split it when a field is missing.
            let f: Vec<String> = if f.first().map(String::as_str) == Some("7") { f[1..].to_vec() } else { f.clone() };
            let mut v: Vec<String> = f.get(2..).unwrap_or(&[]).to_vec();
            if v.len() == 6 {
                if let Some((a, b)) = v[2].split_once('.') {
                    if b.len() >= 2 {
                        let (dur, table) = (format!("{a}.{}", &b[..1]), b[1..].to_string());
                        v.splice(2..3, [dur, table]);
                    }
                }
            }
            let start_t = v.first().and_then(|s| num(s)).unwrap_or(0.0);
            let Some(depth) = v.get(1).and_then(|s| num(s)) else {
                self.notes.push(format!("COMPUT: could not read \"{}\"; skipped.", f.join(" ")));
                continue;
            };
            let duration = v.get(2).and_then(|s| num(s)).filter(|d| *d > 0.0).unwrap_or(1.0);
            let table = v.get(3).and_then(|s| int(s)).unwrap_or(2);
            let arc = v.get(4).and_then(|s| int(s)).unwrap_or(2);
            let storm_no = v.get(6).and_then(|s| int(s)).unwrap_or(i as u32 + 1);
            let storm = match (rain_tables.get(&table), builtin_rain_table(table)) {
                (Some(s), _) => s.name.clone(),
                (None, Some(name)) => {
                    if table != 2 {
                        self.notes.push(format!("COMPUT storm {storm_no}: TR-20 rain table {table} taken as {name}; please verify."));
                    }
                    name.to_string()
                }
                (None, None) => {
                    self.notes.push(format!("COMPUT storm {storm_no}: rain table {table} is not defined; Type II 24-hr used."));
                    "Type II 24-hr".to_string()
                }
            };
            let mut ev = RainEvent::new(&format!("Storm {storm_no}"), &storm, depth);
            ev.amc = Amc::from_number(arc.clamp(1, 3) as u8);
            let storm_len = rain_tables.get(&table).map_or(24.0, |s| s.duration);
            if (duration - 1.0).abs() > 1e-9 {
                ev.duration_mode = DurationMode::Rescale;
                ev.duration = storm_len * duration;
            }
            start = start.min(start_t);
            end = end.max(start_t + storm_len * duration);
            if !self.project.settings.events.iter().any(|e| e.name == ev.name) {
                self.project.settings.events.push(ev);
            }
        }
        if self.project.settings.events.is_empty() {
            self.notes.push("No COMPUT statements; a default 2-Year Type II event was added.".into());
            self.project.settings.events.push(RainEvent::new("2-Year", "Type II 24-hr", 3.0));
            start = 0.0;
            end = 24.0;
        }
        self.project.settings.current_event = 0;
        self.project.settings.span_start = if start.is_finite() { start } else { 0.0 };
        // TR-20 runs a fixed number of steps; allow at least 400 steps and the storm plus 6 h.
        self.project.settings.span_end = self.project.settings.span_start + (400.0 * dt).max(end - self.project.settings.span_start + 6.0);
    }

    /// Lays the network out as a tree drawn from its outlet(s) on the right: each node one column
    /// left of the node it drains to, upstream leaves on consecutive rows in depth-first order
    /// (so tributaries sit beside their receiving node) and other nodes centred on their inflows.
    fn layout(&mut self) {
        const DX: f64 = 150.0;
        const DY: f64 = 110.0;
        let outlets: Vec<NodeId> = self.order.iter().copied().filter(|id| self.project.node(*id).is_some_and(|n| n.outflows.iter().all(Option::is_none))).collect();
        let mut placed: BTreeMap<NodeId, (usize, f64)> = BTreeMap::new();
        let mut next_row = 0.0;
        // Iterative post-order walk: (node, depth, children expanded?).
        for root in outlets.iter().rev() {
            let mut stack = vec![(*root, 0usize, false)];
            while let Some((id, depth, expanded)) = stack.pop() {
                if placed.contains_key(&id) {
                    continue;
                }
                let ups: Vec<NodeId> = self.project.inflows(id).into_iter().map(|(u, _)| u).filter(|u| !placed.contains_key(u)).collect();
                if !expanded && !ups.is_empty() && depth < 10_000 {
                    stack.push((id, depth, true));
                    stack.extend(ups.into_iter().rev().map(|u| (u, depth + 1, false)));
                    continue;
                }
                let rows: Vec<f64> = self.project.inflows(id).iter().filter_map(|(u, _)| placed.get(u)).map(|p| p.1).collect();
                let row = if rows.is_empty() {
                    next_row += 1.0;
                    next_row - 1.0
                } else {
                    rows.iter().sum::<f64>() / rows.len() as f64
                };
                placed.insert(id, (depth, row));
            }
        }
        let max_depth = placed.values().map(|p| p.0).max().unwrap_or(0);
        for id in &self.order {
            // Nodes not reached from an outlet (e.g. in a loop) go on rows of their own.
            let (depth, row) = placed.get(id).copied().unwrap_or_else(|| {
                next_row += 1.0;
                (0, next_row - 1.0)
            });
            if let Some(n) = self.project.node_mut(*id) {
                n.pos = [100.0 + DX * (max_depth - depth.min(max_depth)) as f64, 100.0 + DY * row];
            }
        }
    }
}

fn rating_reach(t: &Table, length: f64) -> Reach {
    let rows: Vec<(f64, f64, f64)> = t.rows.iter().filter_map(|r| Some((*r.first()?, *r.get(1)?, *r.get(2)?))).collect();
    let invert = rows.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
    let invert = if invert.is_finite() { invert } else { 0.0 };
    let points: Vec<RatingPoint> = rows.iter().map(|&(e, q, a)| RatingPoint { depth: e - invert, flow: q, area: a }).collect();
    // Header: number, drainage area, bank-full elevation.
    let bank_full = t.header.get(2).and_then(|s| num(s)).map_or(0.0, |e| (e - invert).max(0.0));
    Reach { section: Section::Rating { points, bank_full }, n: 0.0, length, inlet_invert: invert, outlet_invert: invert, exfiltration: 0.0, flood_depth: None, auto_size: false }
}

fn struct_pond(t: &Table, start: Option<f64>, notes: &mut Vec<String>, st: u32) -> Pond {
    let mut rows: Vec<(f64, f64, f64)> = t.rows.iter().filter_map(|r| Some((*r.first()?, *r.get(1)?, *r.get(2)?))).collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut start = start;
    // Storage must start from zero: insert a zero-storage stage one foot lower and start there.
    if let Some(&(e0, _, s0)) = rows.first() {
        if s0 > 0.0 {
            rows.insert(0, (e0 - 1.0, 0.0, 0.0));
            start = start.or(Some(e0));
            notes.push(format!("STRUCT {st:02}: first storage is not zero; a zero-storage stage was added at {:.2} ft and the pond starts at {e0:.2} ft.", e0 - 1.0));
        }
    }
    let bottom = rows.first().map_or(0.0, |r| r.0);
    let volume = StorageVolume::new(
        "TR-20 stage-storage",
        bottom,
        VolumeShape::Custom { kind: CustomKind::CumulativeStorage, elevations: rows.iter().map(|r| r.0).collect(), values: rows.iter().map(|r| r.2 * SQFT_PER_ACRE).collect() },
    );
    let device = OutletDevice::new(
        "TR-20 stage-discharge",
        bottom,
        Routing::Primary,
        DeviceKind::Special { table: rows.iter().map(|r| (r.0, r.1)).collect(), head_based: false, extrapolate: false },
    );
    Pond { storage: PondStorage { volumes: vec![volume] }, devices: vec![device], start_elevation: start, ..Pond::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::calculate;
    use crate::project::Node;

    /// Five-subwatershed example from a public TR-20 training manual (24.87 sq mi, Type II,
    /// 4.25 in). The manual's REACH cards name sections 8 and 6 for the tables it defines as
    /// 003 and 005; they are corrected here. Published outlet: 6818.4 cfs @ 13.30 h,
    /// 1.85 in, 2458.2 af (TR-20 routes reaches by Att-Kin, so peaks are compared loosely).
    const TRAINING_DECK: &str = "\
JOB TR-20                        FULLPRINT                                      NOPLOTS
TITLE          OUTLET OF BASIN AT WATERSHED #5
TITLE
 2 XSECTN      003                         1.0         193.29
 8                                        188.          0.000           0.000
 8                                       190.7         20.071           8.203
 8                                      193.29         85.823          24.671
 8                                      195.78        247.063          70.625
 8                                      199.28        923.957         297.058
 9 ENDTBL
 2 XSECTN      005                         1.0          187.39
 8                                        180.          0.000           0.000
 8                                      183.69         95.543          21.501
 8                                      187.39        333.701          55.523
 8                                      194.86       1730.803         242.722
 8                                      204.79       7160.793         935.737
 9 ENDTBL
 6 RUNOFF 1   1          1           5.5599             76.048         1.311
 6 RUNOFF 1   2          2           5.5599             72.541         1.089
 6 ADDHYD 4   9      1 2 3
 6 RUNOFF 1   3          1           2.0411             75.444         1.314
 6 REACH 3    3      3   2            7985.
 6 ADDHYD 4   9      1 2 3
 6 RUNOFF 1   4          4           6.1100             74.710         1.470
 6 ADDHYD 4   7      4 3 2
 6 REACH 3    5      2   1           10424.
 6 RUNOFF 1   5          5           5.6000             76.350         1.047
 6 ADDHYD 4 10       1 5 4                                                     1
   ENDATA
 7 INCREM 6                              0.1
 7 COMPUT 7 001      010                 0.0              4.25            1.02 2       1    1
   ENDCMP 1
   ENDJOB 2
";

    #[test]
    fn training_deck_builds_the_network() {
        let imp = import(TRAINING_DECK).unwrap();
        let p = &imp.project;
        assert_eq!(p.title, "OUTLET OF BASIN AT WATERSHED #5");
        let count = |t: NodeType| p.nodes.iter().filter(|n| n.node_type() == t).count();
        assert_eq!((count(NodeType::Subcat), count(NodeType::Reach), count(NodeType::Link)), (5, 2, 4));
        let ev = &p.settings.events[0];
        assert_eq!((ev.storm.as_str(), ev.depth, ev.amc), ("Type II 24-hr", 4.25, Amc::II));
        assert_eq!(p.settings.dt, 0.1);
        // Every node except the outlet link drains somewhere.
        let outlets: Vec<&Node> = p.nodes.iter().filter(|n| n.outflows[0].is_none()).collect();
        assert_eq!(outlets.len(), 1, "{:?}", imp.notes);
        assert_eq!(outlets[0].name, "XSECTN 010");
        assert!(p.nodes.iter().filter(|n| n.node_type() == NodeType::Reach).all(|n| matches!(&n.data, NodeData::Reach(r) if r.is_described())));
    }

    #[test]
    fn training_deck_matches_published_outlet_volume() {
        let imp = import(TRAINING_DECK).unwrap();
        let r = calculate(&imp.project);
        let outlet = imp.project.nodes.iter().find(|n| n.name == "XSECTN 010").unwrap();
        let res = r.node(outlet.id).unwrap();
        let af = res.outflow.volume_cf() / SQFT_PER_ACRE;
        assert!((af - 2458.2).abs() / 2458.2 < 0.01, "volume {af:.1} af");
        // The deck's rating tables stop near 924 and 7161 cfs while ~5,800 cfs reaches the first
        // reach, so routing runs far beyond the tables. TR-20's Att-Kin and our storage routing
        // extrapolate differently there (ours ~12 % lower, ~1 h later); only a loose check.
        let pk = res.outflow.peak();
        assert!((pk.value - 6818.4).abs() / 6818.4 < 0.15, "peak {:.1}", pk.value);
    }

    #[test]
    fn fused_duration_and_rain_table_and_struct_offset() {
        let deck = "\
JOB TR-20
 3 STRUCT       01
 8          100.0       0.0      0.5
 8          102.0      10.0      2.0
 8          104.0      60.0      4.0
 9 ENDTBL
 6 RUNOFF 1 001        1 0.5                 80.0           0.5      1           1
 6 RESVOR 2     01 1    2                                               1
 7 INCREM 6                              0.05
 7 COMPUT 7 001    002                    0.0            3.19            1.05 2     1   2
 7 COMPUT 7 001    002                    0.0            4.91            1.03 2     1   10
";
        let imp = import(deck).unwrap();
        let p = &imp.project;
        let evs: Vec<(&str, &str, f64)> = p.settings.events.iter().map(|e| (e.name.as_str(), e.storm.as_str(), e.depth)).collect();
        // Table 5 is undefined (falls back to Type II with a note); table 3 maps to Type IA.
        assert_eq!(evs, vec![("Storm 2", "Type II 24-hr", 3.19), ("Storm 10", "Type IA 24-hr", 4.91)]);
        assert!(imp.notes.iter().any(|n| n.contains("rain table 5")));
        let pond = p.nodes.iter().find(|n| n.node_type() == NodeType::Pond).unwrap();
        let NodeData::Pond(pd) = &pond.data else { panic!() };
        // First storage 0.5 af > 0: zero stage inserted at 99 ft, start at 100 ft.
        assert_eq!(pd.start_elevation, Some(100.0));
        assert_eq!(pd.storage.bottom(), Some(99.0));
        let sub = p.nodes.iter().find(|n| n.node_type() == NodeType::Subcat).unwrap();
        assert_eq!(sub.outflows[0], Some(pond.id));
        // 0.5 sq mi of CN 80 runoff overtops the 4 af structure table: results are undefined.
        let r = calculate(p);
        assert!(r.messages.iter().filter(|m| m.severity == crate::calc::Severity::Error).all(|m| m.code == 93));
        assert!(r.node(pond.id).is_some_and(|n| n.undefined.is_some()));
    }

    #[test]
    fn rejects_unrelated_text() {
        assert!(import("hello world\n1 2 3").is_err());
    }
}
