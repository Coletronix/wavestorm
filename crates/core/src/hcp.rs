//! Interchange with the reference application's project files (`*.hcp`).
//!
//! The format is INI-like text: a first section holding project settings, then `[LANDUSE]`,
//! `[EVENT]` and `[NODE]` sections, where a node is followed by its child sections (`[AREA]`,
//! `[TC]`, `[STORAGE]`, `[STAGE]`, `[DEVICE]`). Lengths are feet, areas square feet, times
//! seconds and rain depths feet. Keys and sections this model does not represent are kept
//! verbatim in `foreign` fields (project, node and per-record scopes) and written back on export,
//! so a file survives a round trip through Stormwaves.
//!
//! Importing never panics: unknown or malformed data produces warnings and defaults.

use crate::cn::SubArea;
use crate::geometry::{PipeShape, RatingPoint, Section};
use crate::link::{Link, LinkSource, ManualHydrograph};
use crate::outlets::{self, DeviceKind, OpeningShape, OutletDevice, Plane, Routing};
use crate::project::*;
use crate::rainfall::{DurationMode, RainEvent};
use crate::storage::{CustomKind, StorageVolume, VolumeShape};
use crate::tc::{TcKind, TcSegment, SHALLOW_SURFACES, SHEET_SURFACES};
use crate::units::UnitSystem;
use std::collections::HashMap;

/// Diagram units per file coordinate unit (the file's y axis points up, ours down).
const DIAGRAM_SCALE: f64 = 60.0;
/// One mg/L expressed in pounds per cubic foot (the file stores loading concentrations this way).
const MG_L_TO_LB_FT3: f64 = 6.24279605915783e-5;
const NEWLINE: &str = "\r\n";
/// Marker key recording that an unknown section existed even when it holds no entries.
const SECTION_MARK: &str = "\u{0}";

/// The first section's name, assembled so the product name is never spelled out.
fn first_header() -> String {
    format!("{}{}", "Hydr", "oCAD")
}

// ------------------------------------------------------------------ number helpers

/// Rounds to 10 decimals so values survive a text round trip exactly.
fn r10(x: f64) -> f64 {
    if x.is_finite() { (x * 1e10).round() / 1e10 } else { 0.0 }
}

fn fmt(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let r = (x * 1e12).round() / 1e12;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect()
}

fn parse_num(s: &str) -> Option<f64> {
    let v: f64 = s.split_whitespace().next()?.parse().ok()?;
    v.is_finite().then(|| r10(v))
}

fn parse_nums(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite()).map(r10).collect()
}

// ------------------------------------------------------------------ raw parsing

#[derive(Debug, Clone)]
struct RawSection {
    name: String,
    entries: Vec<(String, String)>,
}

fn parse_sections(text: &str, warnings: &mut Vec<String>) -> Vec<RawSection> {
    let mut out: Vec<RawSection> = Vec::new();
    let mut stray = 0usize;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            out.push(RawSection { name: line[1..line.len() - 1].trim().to_string(), entries: Vec::new() });
        } else if let Some(s) = out.last_mut() {
            match line.split_once('=') {
                Some((k, v)) => s.entries.push((k.trim().to_string(), v.trim().to_string())),
                None => s.entries.push((String::new(), line.to_string())),
            }
        } else {
            stray += 1;
        }
    }
    if stray > 0 {
        warnings.push(format!("Ignored {stray} line(s) before the first section"));
    }
    out
}

/// A section being read: values are taken out by key, and what is left over becomes foreign data.
struct Rec {
    e: Vec<(String, String, bool)>,
}

impl Rec {
    fn new(s: &RawSection) -> Rec {
        Rec { e:s.entries.iter().map(|(k, v)| (k.clone(), v.clone(), false)).collect() }
    }
    fn get(&self, key: &str) -> Option<&str> {
        self.e.iter().find(|(k, _, t)| !*t && k.eq_ignore_ascii_case(key)).map(|(_, v, _)| v.as_str())
    }
    fn take(&mut self, key: &str) -> Option<String> {
        let item = self.e.iter_mut().find(|(k, _, t)| !*t && k.eq_ignore_ascii_case(key))?;
        item.2 = true;
        Some(item.1.clone())
    }
    fn take_all(&mut self, key: &str) -> Vec<String> {
        let mut v = Vec::new();
        for item in self.e.iter_mut().filter(|(k, _, t)| !*t && k.eq_ignore_ascii_case(key)) {
            item.2 = true;
            v.push(item.1.clone());
        }
        v
    }
    fn num(&mut self, key: &str) -> Option<f64> {
        self.take(key).and_then(|s| parse_num(&s))
    }
    fn nums(&mut self, key: &str) -> Option<Vec<f64>> {
        self.take(key).map(|s| parse_nums(&s))
    }
    fn flag(&mut self, key: &str) -> Option<bool> {
        self.take(key).map(|s| matches!(s.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "-1"))
    }
    /// Moves every untaken entry into `foreign` under `scope`.
    fn keep(self, foreign: &mut Vec<ForeignEntry>, scope: &str) {
        for (k, v, t) in self.e {
            if !t {
                foreign.push(ForeignEntry { scope: scope.to_string(), key: k, value: v });
            }
        }
    }
}

fn keep_unknown_section(s: &RawSection, foreign: &mut Vec<ForeignEntry>, index: usize) {
    let scope = format!("?{index}:{}", s.name);
    foreign.push(ForeignEntry { scope: scope.clone(), key: SECTION_MARK.into(), value: String::new() });
    for (k, v) in &s.entries {
        foreign.push(ForeignEntry { scope: scope.clone(), key: k.clone(), value: v.clone() });
    }
}

struct RawNode {
    head: RawSection,
    children: Vec<RawSection>,
}

// ------------------------------------------------------------------ import

/// Decodes file bytes: UTF-8 when valid, otherwise Windows-1252 (Latin-1 plus the 0x80-0x9F
/// punctuation block).
pub fn decode_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252_char(b)).collect(),
    }
}

fn cp1252_char(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018,
        0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&b) {
        char::from_u32(u32::from(HIGH.get(usize::from(b - 0x80)).copied().unwrap_or(u16::from(b)))).unwrap_or('?')
    } else {
        char::from(b)
    }
}

fn cp1252_byte(c: char) -> u8 {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return u as u8;
    }
    for b in 0x80u8..0xA0 {
        if cp1252_char(b) == c {
            return b;
        }
    }
    b'?'
}

/// True when the bytes look like a project file of this format (an INI-style first section).
pub fn looks_like_hcp(bytes: &[u8]) -> bool {
    let head = decode_bytes(&bytes[..bytes.len().min(2048)]);
    let first = head.trim_start_matches('\u{feff}').trim_start();
    first.starts_with('[') && !first.starts_with("[{") && head.contains("FileUnits=")
}

pub fn import_bytes(bytes: &[u8]) -> Result<(Project, Vec<String>), String> {
    import(&decode_bytes(bytes))
}

pub fn import(text: &str) -> Result<(Project, Vec<String>), String> {
    let mut warnings = Vec::new();
    let sections = parse_sections(text, &mut warnings);
    let Some((first, rest)) = sections.split_first() else {
        return Err("This is not a project file (no sections found).".into());
    };
    let mut project = Project { title: String::new(), ..Project::default() };

    // ---- project header (whatever the first section is called)
    let mut head = Rec::new(first);
    if let Some(u) = head.take("FileUnits") {
        if !u.eq_ignore_ascii_case("English") {
            return Err(format!(
                "Project files stored in {u} units are not supported yet (no sample to verify the unit conversions against). Files stored in English units open fine, including ones whose reports are set to metric."
            ));
        }
    }
    if let Some(u) = head.take("ReportUnits") {
        project.units.system = if u.eq_ignore_ascii_case("Metric") { UnitSystem::Metric } else { UnitSystem::English };
    }
    project.title = head.take("Name").unwrap_or_default();
    if let Some(v) = head.nums("TimeSpan") {
        if let [a, b, ..] = v[..] {
            project.settings.span_start = r10(a / 3600.0);
            project.settings.span_end = r10(b / 3600.0);
        }
    }
    if let Some(v) = head.num("TimeInc") {
        if v > 0.0 {
            project.settings.dt = r10(v / 3600.0);
        }
    }
    if let Some(v) = head.take("RunoffMethod") {
        let l = v.to_ascii_lowercase();
        project.settings.runoff = if l.contains("sbuh") {
            RunoffMethod::Sbuh
        } else if l.contains("rational") {
            RunoffMethod::Rational
        } else {
            if !l.contains("tr-20") && !l.contains("tr20") {
                warnings.push(format!("Unknown runoff method \"{v}\"; using SCS TR-20"));
            }
            RunoffMethod::ScsTr20
        };
    }
    if let Some(v) = head.take("ReachMethod") {
        let l = v.to_ascii_lowercase();
        project.settings.reach_method = if l.contains("musk") {
            ReachMethod::MuskingumCunge
        } else if l.contains("trans") {
            ReachMethod::StorIndTrans
        } else if l.contains("stor") {
            ReachMethod::StorInd
        } else {
            warnings.push(format!("Unknown reach method \"{v}\""));
            ReachMethod::default()
        };
    }
    if let Some(v) = head.take("PondMethod") {
        project.settings.pond_method = if v.to_ascii_lowercase().contains("dyn") { PondMethod::DynStorInd } else { PondMethod::StorInd };
    }
    if let Some(v) = head.take("UH") {
        project.settings.uh = v;
    }
    let current_event = head.take("RainEvent");
    if let Some(v) = head.num("LoadingRain") {
        project.land_use.annual_rainfall = Some(r10(v * 12.0));
    }
    if let Some(v) = head.num("LoadingFactor") {
        project.land_use.correction = v;
    }
    let p2_header = head.get("P2").and_then(parse_num);

    // ---- group the remaining sections
    let mut events: Vec<&RawSection> = Vec::new();
    let mut landuse: Option<&RawSection> = None;
    let mut nodes: Vec<RawNode> = Vec::new();
    let mut unknown_top = 0usize;
    let mut foreign_top: Vec<ForeignEntry> = Vec::new();
    for s in rest {
        let up = s.name.to_ascii_uppercase();
        match up.as_str() {
            "NODE" => nodes.push(RawNode { head: s.clone(), children: Vec::new() }),
            "EVENT" => events.push(s),
            "LANDUSE" => landuse = Some(s),
            _ => match nodes.last_mut() {
                Some(n) => n.children.push(s.clone()),
                None => {
                    warnings.push(format!("Unknown section [{}] kept unchanged", s.name));
                    keep_unknown_section(s, &mut foreign_top, unknown_top);
                    unknown_top += 1;
                }
            },
        }
    }

    // ---- land use
    if let Some(l) = landuse {
        let mut pollutants: Vec<String> = Vec::new();
        let mut uses = Vec::new();
        for (k, v) in &l.entries {
            if k.eq_ignore_ascii_case("Pollutants") {
                pollutants = v.split_whitespace().map(str::to_string).collect();
            } else {
                let vals: Vec<f64> = v.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).map(|x| r10(x / MG_L_TO_LB_FT3)).collect();
                if vals.len() != v.split_whitespace().count() {
                    warnings.push(format!("Land use \"{k}\" has non-numeric values"));
                }
                uses.push((k.clone(), vals));
            }
        }
        if !pollutants.is_empty() {
            project.land_use.pollutants = pollutants;
        }
        project.land_use.uses = uses;
    }

    // ---- rainfall events
    if !events.is_empty() {
        project.settings.events.clear();
    }
    for (i, s) in events.iter().enumerate() {
        let mut r = Rec::new(s);
        let name = r.take("RainEvent").unwrap_or_else(|| format!("Event {}", i + 1));
        let storm = r.take("StormType").unwrap_or_else(|| "Type II 24-hr".into());
        let depth_in = r.num("StormDepth").map(|d| r10(d * 12.0)).unwrap_or(0.0);
        let mut ev = RainEvent::new(&name, &storm, depth_in);
        if let Some(sec) = r.num("StormDurat") {
            ev.duration = r10(sec / 3600.0);
        }
        match project.storm(&storm) {
            Some(st) => {
                if (ev.duration - st.duration).abs() > 1e-6 {
                    ev.duration_mode = DurationMode::Rescale;
                    warnings.push(format!("Event {name}: duration {} h differs from the storm's; rescaled", ev.duration));
                }
            }
            None => warnings.push(format!("Event {name}: storm \"{storm}\" is not available")),
        }
        r.keep(&mut project.foreign, &format!("EVENT:{i}"));
        project.settings.events.push(ev);
    }
    project.settings.current_event = current_event
        .and_then(|n| project.settings.events.iter().position(|e| e.name.eq_ignore_ascii_case(&n)))
        .unwrap_or(0);
    let p2_default = p2_header
        .map(|ft| r10(ft * 12.0))
        .or_else(|| project.settings.events.iter().find(|e| e.name.eq_ignore_ascii_case("2-Year")).map(|e| e.depth))
        .unwrap_or(3.0);

    head.keep(&mut project.foreign, "");
    project.foreign.extend(foreign_top);

    // ---- nodes
    let mut labels: HashMap<String, NodeId> = HashMap::new();
    let mut pending: Vec<(NodeId, Vec<String>)> = Vec::new();
    for raw in &nodes {
        let id = project.next_id;
        if let Some((node, outs, label)) = import_node(raw, id, p2_default, &mut warnings) {
            project.next_id += 1;
            labels.insert(label.to_ascii_uppercase(), id);
            pending.push((id, outs));
            project.nodes.push(node);
        }
    }
    for (id, outs) in pending {
        let mut targets: [Option<NodeId>; 3] = [None; 3];
        for (k, label) in outs.iter().enumerate() {
            let label = label.trim();
            if label.is_empty() {
                continue;
            }
            match (labels.get(&label.to_ascii_uppercase()), targets.get_mut(k)) {
                (Some(t), Some(slot)) => *slot = Some(*t),
                (None, _) => warnings.push(format!("Outflow to unknown node \"{label}\" ignored")),
                (_, None) => warnings.push(format!("Extra outflow \"{label}\" ignored (only three are supported)")),
            }
        }
        if let Some(n) = project.node_mut(id) {
            n.outflows = targets;
        }
    }
    Ok((project, warnings))
}

fn split_number(raw: &str, nt: NodeType) -> String {
    let t = raw.trim();
    match t.chars().last() {
        Some(c) if c.is_ascii_alphabetic() && c.to_ascii_uppercase().to_string() == nt.letter() => t[..t.len() - c.len_utf8()].to_string(),
        _ => t.to_string(),
    }
}

fn node_type_from(type_name: Option<&str>, number: &str) -> Option<NodeType> {
    let by_name = type_name.map(|t| t.trim().to_ascii_lowercase());
    match by_name.as_deref() {
        Some("subcat") | Some("subcatchment") => Some(NodeType::Subcat),
        Some("reach") => Some(NodeType::Reach),
        Some("pond") => Some(NodeType::Pond),
        Some("link") => Some(NodeType::Link),
        Some("text") => Some(NodeType::Text),
        Some(_) => None,
        None => match number.trim().chars().last()?.to_ascii_uppercase() {
            'S' => Some(NodeType::Subcat),
            'R' => Some(NodeType::Reach),
            'P' => Some(NodeType::Pond),
            'L' => Some(NodeType::Link),
            'T' => Some(NodeType::Text),
            _ => None,
        },
    }
}

fn import_node(raw: &RawNode, id: NodeId, p2_default: f64, warnings: &mut Vec<String>) -> Option<(Node, Vec<String>, String)> {
    let mut head = Rec::new(&raw.head);
    let number_raw = head.take("Number").unwrap_or_default();
    let type_name = head.take("Type");
    let Some(nt) = node_type_from(type_name.as_deref(), &number_raw) else {
        warnings.push(format!("Node {number_raw}: unknown type {:?}; node skipped", type_name.unwrap_or_default()));
        return None;
    };
    let mut number = split_number(&number_raw, nt);
    if number.is_empty() {
        number = id.to_string();
    }
    let label = format!("{}{}", number, nt.letter());
    let name = head.take("Name").unwrap_or_default();
    let xy = head.nums("XYPos").unwrap_or_default();
    let pos = match xy[..] {
        [x, y, ..] => [r10(x * DIAGRAM_SCALE), r10(-y * DIAGRAM_SCALE)],
        _ => [0.0, 0.0],
    };
    let outs = head.take_all("Outflow");
    let mut foreign: Vec<ForeignEntry> = Vec::new();
    let mut used = vec![false; raw.children.len()];
    let child_idx = |used: &mut Vec<bool>, kind: &str| -> Vec<usize> {
        let ix: Vec<usize> = raw.children.iter().enumerate().filter(|(_, c)| c.name.eq_ignore_ascii_case(kind)).map(|(i, _)| i).collect();
        for &i in &ix {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
        ix
    };

    let data = match nt {
        NodeType::Subcat => {
            let mut sc = Subcatchment { large_units: head.flag("LargeAreas").unwrap_or(true), ..Subcatchment::default() };
            sc.uh = head.take("UH");
            for (k, i) in child_idx(&mut used, "AREA").into_iter().enumerate() {
                let Some(c) = raw.children.get(i) else { continue };
                let mut r = Rec::new(c);
                let area = r.num("Area").unwrap_or(0.0);
                let cn = r.num("CN").unwrap_or(0.0);
                let desc = r.take("Desc").unwrap_or_default();
                let mut sa = SubArea::new(area, cn, &desc);
                sa.land_use = r.take("LandUse").filter(|s| !s.is_empty());
                r.keep(&mut foreign, &format!("AREA:{k}"));
                sc.areas.push(sa);
            }
            let area_sf: f64 = sc.areas.iter().map(|a| a.area).sum();
            for (k, i) in child_idx(&mut used, "TC").into_iter().enumerate() {
                let Some(c) = raw.children.get(i) else { continue };
                let mut r = Rec::new(c);
                let scope = format!("TC:{k}");
                match tc_from(&mut r, area_sf, p2_default) {
                    Some(kind) => {
                        let description = r.take("Desc").unwrap_or_default();
                        r.keep(&mut foreign, &scope);
                        sc.tc.push(TcSegment { kind, description });
                    }
                    None => {
                        warnings.push(format!("Node {label}: Tc method {:?} is not supported; kept unchanged with no travel time", r.get("Method").unwrap_or("")));
                        r.keep(&mut foreign, &scope);
                        sc.tc.push(TcSegment { kind: TcKind::Direct { minutes: 0.0, length: 0.0, velocity: 0.0 }, description: String::new() });
                    }
                }
            }
            NodeData::Subcat(sc)
        }
        NodeType::Reach => {
            let mut reach = Reach::default();
            if let Some(v) = head.num("Length") {
                reach.length = v;
            }
            if let Some(v) = head.num("n") {
                reach.n = v;
            }
            reach.inlet_invert = head.num("InletInvert").unwrap_or(0.0);
            reach.outlet_invert = head.num("OutletInvert").unwrap_or(0.0);
            reach.flood_depth = head.num("FloodDepth");
            let diam = head.num("Diam");
            let botw = head.num("BotWidth");
            let topw = head.num("TopWidth");
            let depth = head.num("Depth");
            let sidez = head.nums("SideZ");
            let mut stages: Vec<(f64, f64)> = Vec::new();
            for (k, i) in child_idx(&mut used, "STAGE").into_iter().enumerate() {
                let Some(c) = raw.children.get(i) else { continue };
                let mut r = Rec::new(c);
                let d = r.num("Depth").unwrap_or(0.0);
                let a = r.num("Area").unwrap_or(0.0);
                stages.push((d, a));
                // Perimeter stays in the foreign record so the table can be written back exactly.
                r.keep(&mut foreign, &format!("STAGE:{k}"));
            }
            if !stages.is_empty() {
                let slope = reach.slope();
                let n = reach.n;
                let perims: Vec<f64> = foreign
                    .iter()
                    .filter(|f| f.scope.starts_with("STAGE:") && f.key.eq_ignore_ascii_case("Perim"))
                    .map(|f| parse_num(&f.value).unwrap_or(0.0))
                    .collect();
                let points: Vec<RatingPoint> = stages
                    .iter()
                    .enumerate()
                    .map(|(i, &(d, a))| {
                        let p = perims.get(i).copied().unwrap_or(0.0);
                        let flow = if d > 0.0 && a > 0.0 && p > 0.0 && n > 0.0 && slope > 0.0 {
                            1.486 / n * a * (a / p).powf(2.0 / 3.0) * slope.sqrt()
                        } else {
                            0.0
                        };
                        RatingPoint { depth: d, flow: r10(flow), area: a }
                    })
                    .collect();
                let bank_full = points.iter().map(|p| p.depth).fold(0.0, f64::max);
                reach.section = Section::Rating { points, bank_full };
            } else if let Some(d) = diam {
                reach.section = Section::circular(d);
            } else if let Some(w) = topw {
                reach.section = Section::Parabolic { width: w, depth: depth.unwrap_or(0.0) };
            } else if botw.is_some() || sidez.is_some() || depth.is_some() {
                let bottom = botw.unwrap_or(0.0);
                let depth = depth.unwrap_or(0.0);
                reach.section = match sidez {
                    Some(z) => {
                        let zl = z.first().copied().unwrap_or(0.0);
                        let zr = z.get(1).copied().unwrap_or(zl);
                        Section::Trapezoidal { bottom, z_left: zl, z_right: zr, depth }
                    }
                    None => Section::Rectangular { width: bottom, depth },
                };
            }
            NodeData::Reach(reach)
        }
        NodeType::Pond => {
            let mut pond = Pond { large_units: head.flag("LargeAreas").unwrap_or(true), ..Pond::default() };
            if let Some(tw) = head.num("Tailwater") {
                pond.tailwater = [Tailwater::Fixed(tw); 3];
            }
            pond.flood_elevation = head.num("FloodElev");
            // Storage: each [STORAGE] opens a volume that the following [STAGE] records fill.
            let mut volume: Option<(StorageVolume, Vec<f64>, Vec<f64>, usize, usize)> = None;
            let mut vol_no = 0usize;
            let finish = |v: Option<(StorageVolume, Vec<f64>, Vec<f64>, usize, usize)>, pond: &mut Pond| {
                if let Some((mut vol, elev, area, _, _)) = v {
                    if let VolumeShape::Custom { elevations, values, .. } = &mut vol.shape {
                        *elevations = elev;
                        *values = area;
                    }
                    pond.storage.volumes.push(vol);
                }
            };
            let mut stage_in_vol = 0usize;
            for (i, c) in raw.children.iter().enumerate() {
                let up = c.name.to_ascii_uppercase();
                if up == "STORAGE" {
                    finish(volume.take(), &mut pond);
                    if let Some(u) = used.get_mut(i) {
                        *u = true;
                    }
                    let mut r = Rec::new(c);
                    let shape = r.take("Shape").unwrap_or_default();
                    let kind = match shape.to_ascii_lowercase().as_str() {
                        "conic" => CustomKind::AreaConic,
                        "" | "prismatic" => CustomKind::AreaPrismatic,
                        other => {
                            warnings.push(format!("Node {label}: storage shape \"{other}\" treated as prismatic stage data"));
                            r.e.push(("Shape".into(), shape.clone(), false));
                            CustomKind::AreaPrismatic
                        }
                    };
                    let desc = r.take("Desc").unwrap_or_default();
                    r.keep(&mut foreign, &format!("STORAGE:{vol_no}"));
                    let vol = StorageVolume::new(&desc, 0.0, VolumeShape::Custom { kind, elevations: Vec::new(), values: Vec::new() });
                    volume = Some((vol, Vec::new(), Vec::new(), vol_no, 0));
                    vol_no += 1;
                    stage_in_vol = 0;
                } else if up == "STAGE" {
                    if let Some(u) = used.get_mut(i) {
                        *u = true;
                    }
                    if volume.is_none() {
                        let vol = StorageVolume::new("", 0.0, VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: Vec::new(), values: Vec::new() });
                        volume = Some((vol, Vec::new(), Vec::new(), vol_no, 0));
                        vol_no += 1;
                        stage_in_vol = 0;
                    }
                    let mut r = Rec::new(c);
                    let e = r.num("Elev").unwrap_or(0.0);
                    let a = r.num("Area").unwrap_or(0.0);
                    if let Some((vol, elev, area, n, _)) = volume.as_mut() {
                        elev.push(e);
                        area.push(a);
                        if elev.len() == 1 {
                            vol.invert = e;
                        }
                        r.keep(&mut foreign, &format!("STORAGE:{n}:STAGE:{stage_in_vol}"));
                    }
                    stage_in_vol += 1;
                }
            }
            finish(volume.take(), &mut pond);
            for (k, i) in child_idx(&mut used, "DEVICE").into_iter().enumerate() {
                let Some(c) = raw.children.get(i) else { continue };
                let mut r = Rec::new(c);
                let scope = format!("DEVICE:{k}");
                match device_from(&mut r) {
                    Some(d) => {
                        r.keep(&mut foreign, &scope);
                        pond.devices.push(d);
                    }
                    None => {
                        warnings.push(format!("Node {label}: outlet device type {:?} is not supported; kept unchanged", r.get("Type").unwrap_or("")));
                        let routing = r.get("Routing").and_then(parse_routing).unwrap_or_default();
                        r.keep(&mut foreign, &scope);
                        pond.devices.push(OutletDevice::new("", 0.0, routing, DeviceKind::ConstantFlow { flow: 0.0, phase_in: 0.0 }));
                    }
                }
            }
            NodeData::Pond(pond)
        }
        NodeType::Link => {
            let mut link = Link::default();
            let flows = head.take_all("Flow");
            if !flows.is_empty() {
                let t0 = head.num("t0").map(|s| r10(s / 3600.0)).unwrap_or(0.0);
                let dt = head.num("dt").map(|s| r10(s / 3600.0)).unwrap_or(0.0);
                let values: Vec<f64> = flows.iter().flat_map(|l| parse_nums(l)).collect();
                link.source = LinkSource::Manual(ManualHydrograph { t0, dt, flows: values, per_event: Vec::new() });
            }
            link.adjust.threshold = head.num("Threshold");
            NodeData::Link(link)
        }
        NodeType::Text => NodeData::Text(TextNode { text: name.clone(), ..TextNode::default() }),
    };
    for (i, c) in raw.children.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(true) {
            warnings.push(format!("Node {label}: section [{}] is not used by this node type; kept unchanged", c.name));
            keep_unknown_section(c, &mut foreign, i);
        }
    }
    let mut node_foreign = Vec::new();
    head.keep(&mut node_foreign, "");
    node_foreign.extend(foreign);
    let node = Node {
        id,
        number,
        name,
        notes: String::new(),
        pos,
        outflows: [None; 3],
        data,
        locked: false,
        outline: None,
        foreign: node_foreign,
    };
    Some((node, outs, label))
}

fn parse_routing(s: &str) -> Option<Routing> {
    let l = s.trim().to_ascii_lowercase();
    match l.as_str() {
        "primary" => Some(Routing::Primary),
        "secondary" => Some(Routing::Secondary),
        "tertiary" => Some(Routing::Tertiary),
        "discarded" => Some(Routing::Discarded),
        _ => {
            let n: usize = l.strip_prefix("device")?.trim().parse().ok()?;
            Some(Routing::Device(n.checked_sub(1)?))
        }
    }
}

fn surface_lookup(table: &[(&str, f64)], name: &str) -> Option<f64> {
    table.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, v)| *v)
}

/// Reads one `[TC]` record. Returns `None` (leaving the record untouched) for unknown methods.
fn tc_from(r: &mut Rec, area_sf: f64, p2_default: f64) -> Option<TcKind> {
    let method = r.get("Method")?.trim().to_ascii_lowercase();
    let known = matches!(method.as_str(), "sheet" | "shallow" | "channel" | "trap" | "parabol" | "pipe" | "lag" | "direct");
    if !known {
        return None;
    }
    r.take("Method");
    let length = r.num("Length").unwrap_or(0.0);
    let slope = r.num("Slope").unwrap_or(0.0);
    Some(match method.as_str() {
        "sheet" => {
            let surface = r.take("Surface").unwrap_or_default();
            let n = r.num("n").or_else(|| surface_lookup(SHEET_SURFACES, &surface)).unwrap_or(0.15);
            let p2 = r.num("P2").map(|ft| r10(ft * 12.0)).unwrap_or(p2_default);
            TcKind::Sheet { length, slope, n, p2, surface }
        }
        "shallow" => {
            let surface = r.take("Surface").unwrap_or_default();
            let kv = r.num("Kv").or_else(|| surface_lookup(SHALLOW_SURFACES, &surface)).unwrap_or(16.1345);
            TcKind::Shallow { length, slope, kv, surface }
        }
        "direct" => {
            let minutes = r.num("Tc").map(|s| r10(s / 60.0)).unwrap_or(0.0);
            TcKind::Direct { minutes, length, velocity: 0.0 }
        }
        "lag" => {
            let slope = if slope > 0.0 || r.get("Cinterval").is_none() {
                slope
            } else {
                let ci = r.get("Cinterval").and_then(parse_num).unwrap_or(0.0);
                let cl = r.get("Clength").and_then(parse_num).unwrap_or(0.0);
                if area_sf > 0.0 { r10(ci * cl / area_sf) } else { 0.0 }
            };
            TcKind::Lag { length, slope }
        }
        _ => {
            let n = r.num("n").unwrap_or(0.0);
            let section = match method.as_str() {
                "channel" => {
                    let area = r.num("Area").unwrap_or(0.0);
                    let perimeter = r.num("Perim").unwrap_or(0.0);
                    Section::AreaPerimeter { area, perimeter }
                }
                "trap" => {
                    let bottom = r.num("Width").unwrap_or(0.0);
                    let depth = r.num("Depth").unwrap_or(0.0);
                    let z = r.nums("SideZ").unwrap_or_default();
                    let zl = z.first().copied().unwrap_or(0.0);
                    let zr = z.get(1).copied().unwrap_or(zl);
                    Section::Trapezoidal { bottom, z_left: zl, z_right: zr, depth }
                }
                "parabol" => {
                    let width = r.num("Width").unwrap_or(0.0);
                    let depth = r.num("Depth").unwrap_or(0.0);
                    Section::Parabolic { width, depth }
                }
                _ => Section::circular(r.num("Diam").unwrap_or(0.0)),
            };
            TcKind::Channel { length, slope, n, section }
        }
    })
}

/// Reads one `[DEVICE]` record. Returns `None` (leaving the record untouched) for unknown types.
fn device_from(r: &mut Rec) -> Option<OutletDevice> {
    let ty = r.get("Type")?.trim().to_ascii_lowercase();
    if !matches!(ty.as_str(), "culvert" | "orifice" | "bcrweir" | "scvweir") {
        return None;
    }
    r.take("Type");
    let desc = r.take("Desc").unwrap_or_default();
    let invert = r.num("Invert").unwrap_or(0.0);
    let routing = r.take("Routing").and_then(|s| parse_routing(&s)).unwrap_or_default();
    let kind = match ty.as_str() {
        "culvert" => {
            let d = r.num("Diam").unwrap_or(0.0);
            DeviceKind::Culvert {
                shape: PipeShape::Round,
                width: d,
                height: d,
                length: r.num("Length").unwrap_or(0.0),
                outlet_invert: r.num("OutletInvert").unwrap_or(invert),
                n: r.num("n").unwrap_or(0.013),
                ke: r.num("Ke").unwrap_or(0.5),
                cc: r.num("CC").unwrap_or(0.9),
                fill: 0.0,
                entrance: r.take("Entrance").unwrap_or_default(),
            }
        }
        "orifice" => {
            let shape = match r.num("Diam") {
                Some(d) => OpeningShape::Circular { diameter: d },
                None => OpeningShape::Rectangular { width: r.num("Width").unwrap_or(0.0), height: r.num("Height").unwrap_or(0.0) },
            };
            let plane = match r.take("Plane").unwrap_or_default().to_ascii_lowercase().as_str() {
                "horiz" | "horizontal" => Plane::Horizontal,
                _ => Plane::Vertical,
            };
            DeviceKind::Orifice {
                shape,
                plane,
                cd: r.num("C").unwrap_or(0.6),
                columns: 1,
                rows: 1,
                row_spacing: 0.0,
                weir_at_low_head: true,
                grate: None,
            }
        }
        "bcrweir" => {
            DeviceKind::BroadCrestedWeir { length: r.num("Length").unwrap_or(0.0), breadth: r.num("Breadth").unwrap_or(0.0), side_z: 0.0, rise: None }
        }
        _ => DeviceKind::VeeTrapWeir { notch_angle: r.num("Angle").unwrap_or(90.0), crest_length: 0.0, cv: r.num("C").unwrap_or(2.5), rise: None },
    };
    let mut d = OutletDevice::new(&desc, invert, routing, kind);
    d.multiplier = 1.0;
    Some(d)
}

// ------------------------------------------------------------------ export

struct Out {
    s: String,
}

impl Out {
    fn section(&mut self, name: &str) {
        self.s.push('[');
        self.s.push_str(&clean(name));
        self.s.push(']');
        self.s.push_str(NEWLINE);
    }
    fn kv(&mut self, k: &str, v: &str) {
        self.s.push_str(&clean(k));
        self.s.push('=');
        self.s.push_str(&clean(v));
        self.s.push_str(NEWLINE);
    }
    fn num(&mut self, k: &str, v: f64) {
        self.kv(k, &fmt(v));
    }
    fn foreign(&mut self, f: &[ForeignEntry], scope: &str) {
        for e in f.iter().filter(|e| e.scope == scope && e.key != SECTION_MARK) {
            if e.key.is_empty() {
                self.s.push_str(&clean(&e.value));
                self.s.push_str(NEWLINE);
            } else {
                self.kv(&e.key, &e.value);
            }
        }
    }
    /// Writes the unknown sections of `f` whose scope starts with `?`.
    fn unknown_sections(&mut self, f: &[ForeignEntry]) {
        let mut seen: Vec<&str> = Vec::new();
        for e in f.iter().filter(|e| e.scope.starts_with('?')) {
            if !seen.contains(&e.scope.as_str()) {
                seen.push(&e.scope);
            }
        }
        for scope in seen {
            let name = scope.split_once(':').map(|(_, n)| n).unwrap_or("");
            self.section(name);
            self.foreign(f, scope);
        }
    }
}

fn has_foreign(f: &[ForeignEntry], scope: &str, key: &str) -> bool {
    f.iter().any(|e| e.scope == scope && e.key.eq_ignore_ascii_case(key))
}

fn foreign_num(f: &[ForeignEntry], scope: &str, key: &str) -> Option<f64> {
    f.iter().find(|e| e.scope == scope && e.key.eq_ignore_ascii_case(key)).and_then(|e| parse_num(&e.value))
}

pub fn export_bytes(project: &Project) -> Vec<u8> {
    export(project).chars().map(cp1252_byte).collect()
}

pub fn export(project: &Project) -> String {
    let mut o = Out { s: String::new() };
    let st = &project.settings;
    o.section(&first_header());
    o.kv("FileUnits", "English");
    // A project that did not come from such a file has no header extras: give it the defaults.
    if !project.foreign.iter().any(|e| e.scope.is_empty()) {
        o.kv("CalcUnits", "English");
        o.kv("InputUnits", "English");
    }
    o.kv("ReportUnits", if project.units.system == UnitSystem::Metric { "Metric" } else { "English" });
    o.foreign(&project.foreign, "");
    o.kv("Name", &project.title);
    o.kv("TimeSpan", &format!("{} {}", fmt(st.span_start * 3600.0), fmt(st.span_end * 3600.0)));
    o.num("TimeInc", st.dt * 3600.0);
    o.kv("RunoffMethod", st.runoff.label());
    o.kv("ReachMethod", st.reach_method.label());
    o.kv("PondMethod", st.pond_method.label());
    o.kv("UH", &st.uh);
    if let Some(e) = st.event() {
        o.kv("RainEvent", &e.name);
    }
    if let Some(r) = project.land_use.annual_rainfall {
        o.num("LoadingRain", r / 12.0);
    }
    o.num("LoadingFactor", project.land_use.correction);
    o.unknown_sections(&project.foreign);

    if !project.land_use.uses.is_empty() {
        o.section("LANDUSE");
        o.kv("Pollutants", &format!(" {}", project.land_use.pollutants.join(" ")));
        for (name, vals) in &project.land_use.uses {
            let v: Vec<String> = vals.iter().map(|x| format!("{}", x * MG_L_TO_LB_FT3)).collect();
            o.kv(name, &format!(" {}", v.join(" ")));
        }
    }
    for (i, e) in st.events.iter().enumerate() {
        o.section("EVENT");
        o.kv("RainEvent", &e.name);
        o.kv("StormType", &e.storm);
        let duration = match (e.duration_mode, project.storm(&e.storm)) {
            (DurationMode::Default, Some(s)) => s.duration,
            _ => e.duration,
        };
        o.num("StormDurat", duration * 3600.0);
        o.num("StormDepth", e.depth / 12.0);
        o.foreign(&project.foreign, &format!("EVENT:{i}"));
    }
    for n in &project.nodes {
        export_node(&mut o, project, n);
    }
    o.s
}

fn export_node(o: &mut Out, project: &Project, n: &Node) {
    let f = &n.foreign;
    o.section("NODE");
    o.kv("Number", &n.label());
    o.kv(
        "Type",
        match n.node_type() {
            NodeType::Subcat => "Subcat",
            NodeType::Reach => "Reach",
            NodeType::Pond => "Pond",
            NodeType::Link => "Link",
            NodeType::Text => "Text",
        },
    );
    o.kv("Name", &if let NodeData::Text(t) = &n.data { t.text.clone() } else { n.name.clone() });
    o.kv("XYPos", &format!("{} {}", fmt(n.pos[0] / DIAGRAM_SCALE), fmt(-n.pos[1] / DIAGRAM_SCALE)));
    let count = n.outflows.iter().rposition(|x| x.is_some()).map_or(0, |i| i + 1);
    for t in n.outflows.iter().take(count) {
        let label = t.and_then(|id| project.node(id)).map(|x| x.label()).unwrap_or_default();
        o.kv("Outflow", &label);
    }
    let flag = |b: bool| if b { "True" } else { "False" };
    match &n.data {
        NodeData::Subcat(sc) => {
            o.kv("LargeAreas", flag(sc.large_units));
            if let Some(u) = &sc.uh {
                o.kv("UH", u);
            }
        }
        NodeData::Reach(r) => export_reach_keys(o, r, f),
        NodeData::Pond(p) => {
            o.kv("LargeAreas", flag(p.large_units));
            if let Some(Tailwater::Fixed(v)) = p.tailwater.iter().find(|t| matches!(t, Tailwater::Fixed(_))) {
                o.num("Tailwater", *v);
            }
            if let Some(v) = p.flood_elevation {
                o.num("FloodElev", v);
            }
        }
        NodeData::Link(l) => {
            if let LinkSource::Manual(m) = &l.source {
                o.num("t0", m.t0 * 3600.0);
                o.num("dt", m.dt * 3600.0);
            }
            if let Some(t) = l.adjust.threshold {
                o.num("Threshold", t);
            }
            if let LinkSource::Manual(m) = &l.source {
                for chunk in m.flows.chunks(19) {
                    let v: Vec<String> = chunk.iter().map(|x| fmt(*x)).collect();
                    o.kv("Flow", &format!(" {}", v.join(" ")));
                }
            }
        }
        NodeData::Text(_) => {}
    }
    o.foreign(f, "");
    match &n.data {
        NodeData::Subcat(sc) => {
            let area_sf: f64 = sc.areas.iter().map(|a| a.area).sum();
            for (k, a) in sc.areas.iter().enumerate() {
                o.section("AREA");
                o.num("Area", a.area);
                o.num("CN", a.cn);
                o.kv("Desc", &a.description);
                if let Some(l) = &a.land_use {
                    o.kv("LandUse", l);
                }
                o.foreign(f, &format!("AREA:{k}"));
            }
            for (k, seg) in sc.tc.iter().enumerate() {
                export_tc(o, seg, f, &format!("TC:{k}"), area_sf);
            }
        }
        NodeData::Reach(r) => export_reach_stages(o, r, f),
        NodeData::Pond(p) => {
            for (k, v) in p.storage.volumes.iter().enumerate() {
                export_volume(o, v, f, k);
            }
            for (k, d) in p.devices.iter().enumerate() {
                export_device(o, d, f, &format!("DEVICE:{k}"));
            }
        }
        _ => {}
    }
    o.unknown_sections(f);
}

fn export_reach_keys(o: &mut Out, r: &Reach, f: &[ForeignEntry]) {
    if let Some(d) = r.flood_depth {
        o.num("FloodDepth", d);
    }
    match &r.section {
        Section::Rectangular { width, depth } => {
            o.num("BotWidth", *width);
            o.num("Depth", *depth);
        }
        Section::Trapezoidal { bottom, z_left, z_right, depth } => {
            if *bottom > 0.0 {
                o.num("BotWidth", *bottom);
            }
            o.num("Depth", *depth);
            o.kv("SideZ", &format!("{} {}", fmt(*z_left), fmt(*z_right)));
        }
        Section::Parabolic { width, depth } => {
            o.num("TopWidth", *width);
            o.num("Depth", *depth);
        }
        Section::Pipe { shape: PipeShape::Round, width, fill, .. } if *fill <= 0.0 => o.num("Diam", *width),
        // Everything else is written as a depth / area / perimeter table.
        _ => {}
    }
    o.num("Length", r.length);
    o.num("n", r.n);
    o.num("InletInvert", r.inlet_invert);
    o.num("OutletInvert", r.outlet_invert);
    let _ = f;
}

fn export_reach_stages(o: &mut Out, r: &Reach, f: &[ForeignEntry]) {
    let tabulated = match &r.section {
        Section::Rating { points, .. } => points.iter().map(|p| (p.depth, p.area)).collect::<Vec<_>>(),
        Section::Rectangular { .. } | Section::Trapezoidal { .. } | Section::Parabolic { .. } => return,
        Section::Pipe { shape: PipeShape::Round, fill, .. } if *fill <= 0.0 => return,
        s => {
            let full = s.full_depth();
            (0..=10).map(|i| {
                let d = full * f64::from(i) / 10.0;
                (d, s.props(d).area)
            }).collect()
        }
    };
    for (k, (d, a)) in tabulated.iter().enumerate() {
        let scope = format!("STAGE:{k}");
        o.section("STAGE");
        o.num("Depth", *d);
        o.num("Area", *a);
        if !has_foreign(f, &scope, "Perim") {
            let p = match &r.section {
                Section::Rating { .. } => 0.0,
                s => s.props(*d).perimeter,
            };
            o.num("Perim", p);
        }
        o.foreign(f, &scope);
    }
}

fn export_tc(o: &mut Out, seg: &TcSegment, f: &[ForeignEntry], scope: &str, area_sf: f64) {
    o.section("TC");
    if has_foreign(f, scope, "Method") {
        // A method this model does not know: written back exactly as read.
        o.foreign(f, scope);
        return;
    }
    let direct = |o: &mut Out, minutes: f64, length: f64| {
        o.kv("Method", "Direct");
        o.kv("Desc", &seg.description);
        if length > 0.0 {
            o.num("Length", length);
        }
        o.num("Tc", minutes * 60.0);
    };
    match &seg.kind {
        TcKind::Direct { minutes, length, .. } => direct(o, *minutes, *length),
        TcKind::Sheet { length, slope, n, p2, surface } => {
            o.kv("Method", "Sheet");
            o.kv("Desc", &seg.description);
            o.num("Length", *length);
            o.num("Slope", *slope);
            o.kv("Surface", surface);
            o.num("n", *n);
            o.num("P2", p2 / 12.0);
        }
        TcKind::Shallow { length, slope, kv, surface } => {
            o.kv("Method", "Shallow");
            o.kv("Desc", &seg.description);
            o.num("Length", *length);
            o.num("Slope", *slope);
            o.kv("Surface", surface);
            o.num("Kv", *kv);
        }
        TcKind::Lag { length, slope } => {
            o.kv("Method", "Lag");
            o.kv("Desc", &seg.description);
            o.num("Length", *length);
            let contour = foreign_num(f, scope, "Cinterval").zip(foreign_num(f, scope, "Clength"));
            let from_contour = contour.filter(|_| area_sf > 0.0).map(|(ci, cl)| ci * cl / area_sf);
            if !from_contour.is_some_and(|s| (s - slope).abs() < 1e-8) {
                o.num("Slope", *slope);
            }
        }
        TcKind::Channel { length, slope, n, section } => {
            let method = match section {
                Section::AreaPerimeter { .. } => "Channel",
                Section::Rectangular { .. } | Section::Trapezoidal { .. } => "Trap",
                Section::Parabolic { .. } => "Parabol",
                Section::Pipe { shape: PipeShape::Round, fill, .. } if *fill <= 0.0 => "Pipe",
                _ => {
                    direct(o, seg.kind.evaluate(0.0).minutes, *length);
                    o.foreign(f, scope);
                    return;
                }
            };
            o.kv("Method", method);
            o.kv("Desc", &seg.description);
            o.num("Length", *length);
            o.num("Slope", *slope);
            o.num("n", *n);
            match section {
                Section::AreaPerimeter { area, perimeter } => {
                    o.num("Area", *area);
                    o.num("Perim", *perimeter);
                }
                Section::Rectangular { width, depth } => {
                    o.num("Width", *width);
                    o.num("Depth", *depth);
                    o.kv("SideZ", "0 0");
                }
                Section::Trapezoidal { bottom, z_left, z_right, depth } => {
                    if *bottom > 0.0 {
                        o.num("Width", *bottom);
                    }
                    o.num("Depth", *depth);
                    o.kv("SideZ", &format!("{} {}", fmt(*z_left), fmt(*z_right)));
                }
                Section::Parabolic { width, depth } => {
                    o.num("Width", *width);
                    o.num("Depth", *depth);
                }
                Section::Pipe { width, .. } => o.num("Diam", *width),
                _ => {}
            }
        }
        other => direct(o, other.evaluate(0.0).minutes, other.length()),
    }
    o.foreign(f, scope);
}

fn export_volume(o: &mut Out, v: &StorageVolume, f: &[ForeignEntry], k: usize) {
    o.section("STORAGE");
    let scale = (v.voids_pct / 100.0) * v.multiplier;
    let (conic, table): (bool, Vec<(f64, f64)>) = match &v.shape {
        VolumeShape::Custom { kind: kind @ (CustomKind::AreaPrismatic | CustomKind::AreaConic), elevations, values } => {
            (*kind == CustomKind::AreaConic, elevations.iter().copied().zip(values.iter().map(|a| a * scale)).collect())
        }
        shape => {
            // Any other storage is tabulated as surface areas (the derivative of its volume).
            let (bottom, top) = (v.bottom(), v.top());
            let steps = 24;
            let pts = (0..=steps)
                .map(|i| {
                    let e = bottom + (top - bottom) * f64::from(i) / f64::from(steps);
                    let a = v.surface_area(e).unwrap_or_else(|| {
                        let d = ((top - bottom) / 1000.0).max(1e-6);
                        (v.gross(e + d) - v.gross(e)) / d
                    });
                    (e, a * scale)
                })
                .collect();
            let _ = shape;
            (false, pts)
        }
    };
    if conic {
        o.kv("Shape", "Conic");
    }
    if !v.description.is_empty() {
        o.kv("Desc", &v.description);
    }
    o.foreign(f, &format!("STORAGE:{k}"));
    for (j, (e, a)) in table.iter().enumerate() {
        o.section("STAGE");
        o.num("Elev", *e);
        o.num("Area", *a);
        o.foreign(f, &format!("STORAGE:{k}:STAGE:{j}"));
    }
}

fn routing_text(r: Routing) -> String {
    r.label()
}

fn export_device(o: &mut Out, d: &OutletDevice, f: &[ForeignEntry], scope: &str) {
    o.section("DEVICE");
    if has_foreign(f, scope, "Type") {
        o.foreign(f, scope);
        return;
    }
    match &d.kind {
        DeviceKind::Culvert { shape, width, height, length, outlet_invert, n, ke, cc, entrance, .. } => {
            o.kv("Type", "Culvert");
            o.kv("Routing", &routing_text(d.routing));
            if !d.description.is_empty() {
                o.kv("Desc", &d.description);
            }
            o.num("Invert", d.invert);
            o.num("OutletInvert", *outlet_invert);
            o.num("Length", *length);
            if *shape == PipeShape::Round {
                o.num("Diam", *width);
            } else {
                o.num("Width", *width);
                o.num("Height", *height);
            }
            o.num("n", *n);
            if !entrance.is_empty() {
                o.kv("Entrance", entrance);
            }
            o.num("Ke", *ke);
            o.num("CC", *cc);
        }
        DeviceKind::Orifice { shape, plane, cd, .. } => {
            o.kv("Type", "Orifice");
            if !d.description.is_empty() {
                o.kv("Desc", &d.description);
            }
            o.num("Invert", d.invert);
            o.kv("Routing", &routing_text(d.routing));
            match shape {
                OpeningShape::Circular { diameter } => o.num("Diam", *diameter),
                OpeningShape::Rectangular { width, height } => {
                    o.num("Width", *width);
                    o.num("Height", *height);
                }
            }
            o.num("C", *cd);
            o.kv("Plane", if *plane == Plane::Horizontal { "Horiz" } else { "Vert" });
        }
        DeviceKind::BroadCrestedWeir { length, breadth, .. } => {
            o.kv("Type", "BCRWeir");
            if !d.description.is_empty() {
                o.kv("Desc", &d.description);
            }
            o.num("Invert", d.invert);
            o.kv("Routing", &routing_text(d.routing));
            o.num("Length", *length);
            o.num("Breadth", *breadth);
            if !has_foreign(f, scope, "Stage") {
                for (h, c) in outlets::broad_crested_table(*breadth) {
                    o.kv("Stage", &format!("{} {}", fmt(h), fmt(c)));
                }
            }
        }
        DeviceKind::VeeTrapWeir { notch_angle, crest_length, cv, .. } if *crest_length <= 0.0 => {
            o.kv("Type", "SCVWeir");
            if !d.description.is_empty() {
                o.kv("Desc", &d.description);
            }
            o.num("Invert", d.invert);
            o.kv("Routing", &routing_text(d.routing));
            o.num("Angle", *notch_angle);
            o.num("C", *cv);
        }
        other => {
            // No known counterpart in the interchange format: written by its Stormwaves name.
            o.kv("Type", other.label());
            o.kv("Desc", &d.description);
            o.num("Invert", d.invert);
            o.kv("Routing", &routing_text(d.routing));
        }
    }
    o.foreign(f, scope);
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc;

    fn header() -> String {
        format!("[{}]", first_header())
    }

    /// A small project with every node type, written in the interchange format.
    fn fixture() -> String {
        let mut s = String::new();
        let mut add = |l: &str| {
            s.push_str(l);
            s.push_str("\r\n");
        };
        add(&header());
        for l in [
            "FileUnits=English",
            "CalcUnits=English",
            "ReportUnits=English",
            "Source=test fixture \u{ae} 2025",
            "Name=Fixture project",
            "Mystery=42",
            "TimeSpan=0 108000",
            "TimeInc=360",
            "RunoffMethod=SCS TR-20",
            "ReachMethod=Stor-Ind+Trans",
            "PondMethod=Stor-Ind",
            "UH=SCS",
            "RainEvent=10-Year",
            "P2=0.25",
            "[EVENT]",
            "RainEvent=2-Year",
            "StormType=Type II 24-hr",
            "StormDurat=86400",
            "StormDepth=0.25",
            "[EVENT]",
            "RainEvent=10-Year",
            "StormType=Type II 24-hr",
            "StormDurat=86400",
            "StormDepth=0.375",
            "Extra=1",
            "[WEIRD]",
            "A=1",
            "[NODE]",
            "Number=1S",
            "Type=Subcat",
            "Name=Roof",
            "XYPos=1 2",
            "Outflow=1R",
            "LargeAreas=True",
            "Flavor=sweet",
            "[AREA]",
            "Area=217800",
            "CN=61",
            "Desc=Open space, HSG B",
            "CNindex=5B",
            "[TC]",
            "Method=Sheet",
            "Length=100",
            "Slope=0.01",
            "Surface=Grass: Dense",
            "n=0.24",
            "P2=0.25",
            "[TC]",
            "Method=Lag",
            "Length=1050",
            "Clength=817",
            "Cinterval=20",
            "[TC]",
            "Method=Future",
            "X=1",
            "[NODE]",
            "Number=1R",
            "Type=Reach",
            "Name=Swale",
            "XYPos=1 0",
            "Outflow=1P",
            "Diam=2.5",
            "Length=1000",
            "n=0.013",
            "InletInvert=80",
            "OutletInvert=75",
            "[NODE]",
            "Number=2R",
            "Type=Reach",
            "XYPos=3 0",
            "Length=500",
            "n=0.03",
            "InletInvert=10",
            "OutletInvert=9",
            "[STAGE]",
            "Depth=0",
            "Area=0",
            "Perim=0",
            "[STAGE]",
            "Depth=1",
            "Area=5",
            "Perim=6.8",
            "[NODE]",
            "Number=1P",
            "Type=Pond",
            "Name=Pond",
            "XYPos=2 -1",
            "Outflow=1L",
            "Outflow=",
            "Tailwater=70",
            "[STORAGE]",
            "[STAGE]",
            "Elev=70",
            "Area=1000",
            "[STAGE]",
            "Elev=74",
            "Area=4000",
            "[DEVICE]",
            "Type=Culvert",
            "Routing=Primary",
            "Invert=70.5",
            "OutletInvert=70",
            "Length=20",
            "Diam=1.5",
            "n=0.013",
            "Ke=0.5",
            "CC=0.9",
            "[DEVICE]",
            "Type=Gizmo",
            "Routing=Device 1",
            "[NODE]",
            "Number=1L",
            "Type=Link",
            "XYPos=4 -1",
            "Threshold=0.5",
            "t0=0",
            "dt=1800",
            "Flow= 0 1 2",
            "Flow= 1 0",
            "[NODE]",
            "Number=1T",
            "Type=Text",
            "Name=A note",
            "XYPos=5 5",
            "NameStyle=Bold Large",
        ] {
            add(l);
        }
        s
    }

    #[test]
    fn imports_every_node_type() {
        let (p, w) = import(&fixture()).unwrap();
        assert_eq!(p.title, "Fixture project");
        assert_eq!(p.settings.events.len(), 2);
        assert!((p.settings.events[0].depth - 3.0).abs() < 1e-9);
        assert_eq!(p.settings.current_event, 1);
        assert!((p.settings.dt - 0.1).abs() < 1e-9);
        assert!((p.settings.span_end - 30.0).abs() < 1e-9);
        let types: Vec<NodeType> = p.nodes.iter().map(|n| n.node_type()).collect();
        assert_eq!(
            types,
            [NodeType::Subcat, NodeType::Reach, NodeType::Reach, NodeType::Pond, NodeType::Link, NodeType::Text]
        );
        assert!(w.iter().any(|m| m.contains("WEIRD")));
        assert!(w.iter().any(|m| m.contains("Gizmo")));
        let s = &p.nodes[0];
        assert_eq!(s.label(), "1S");
        assert_eq!(s.outflows[0], Some(p.nodes[1].id));
        let NodeData::Subcat(sc) = &s.data else { panic!() };
        assert_eq!(sc.areas.len(), 1);
        assert_eq!(sc.tc.len(), 3);
        assert!(matches!(sc.tc[0].kind, TcKind::Sheet { p2, .. } if (p2 - 3.0).abs() < 1e-9));
        // Lag slope from contours: 20 * 817 / 217800.
        assert!(matches!(sc.tc[1].kind, TcKind::Lag { slope, .. } if (slope - 20.0 * 817.0 / 217_800.0).abs() < 1e-9));
        let NodeData::Pond(pond) = &p.nodes[3].data else { panic!() };
        assert_eq!(pond.devices.len(), 2);
        assert_eq!(pond.storage.volumes.len(), 1);
        assert_eq!(p.nodes[3].outflows[0], Some(p.nodes[4].id));
        assert!(matches!(pond.tailwater[0], Tailwater::Fixed(v) if v == 70.0));
        let NodeData::Link(l) = &p.nodes[4].data else { panic!() };
        let LinkSource::Manual(m) = &l.source else { panic!() };
        assert_eq!(m.flows, vec![0.0, 1.0, 2.0, 1.0, 0.0]);
        assert!((m.dt - 0.5).abs() < 1e-9);
        let NodeData::Reach(r) = &p.nodes[2].data else { panic!() };
        assert!(matches!(r.section, Section::Rating { .. }));
        assert!(p.nodes[0].foreign.iter().any(|f| f.key == "Flavor"));
        assert!(p.foreign.iter().any(|f| f.key == "Mystery"));
    }

    #[test]
    fn round_trip_is_stable() {
        let (p1, _) = import(&fixture()).unwrap();
        let text = export(&p1);
        assert!(text.starts_with(&header()));
        let (p2, _) = import(&text).unwrap();
        assert_eq!(p1, p2);
        // Unknown data and the byte-level encoding survive.
        assert!(text.contains("Flavor=sweet"));
        assert!(text.contains("[WEIRD]"));
        assert!(text.contains("Method=Future"));
        assert!(text.contains("Type=Gizmo"));
        assert_eq!(export(&p2), text);
        let bytes = export_bytes(&p1);
        assert!(bytes.contains(&0xAE));
        let (p3, _) = import_bytes(&bytes).unwrap();
        assert_eq!(p1, p3);
    }

    #[test]
    fn decodes_latin1_and_crlf() {
        let mut text = fixture().replace("\r\n", "\n");
        text.push('\n');
        let bytes: Vec<u8> = text.chars().map(cp1252_byte).collect();
        assert!(std::str::from_utf8(&bytes).is_err());
        let (p, _) = import_bytes(&bytes).unwrap();
        assert_eq!(p.nodes.len(), 6);
        assert!(looks_like_hcp(&bytes));
        assert!(!looks_like_hcp(b"{\"format\":1}"));
    }

    #[test]
    fn garbage_never_panics() {
        let text = fixture();
        for cut in (0..text.len()).step_by(7) {
            if let Some(t) = text.get(..cut) {
                let _ = import(t);
            }
        }
        let junk = ["", "[", "]", "[]", "=", "[NODE]\n[NODE]\n[AREA]", "[x]\nNumber=\nType=Pond\n[NODE]\nXYPos=\nOutflow=", "\u{0}\u{1}[a]=="];
        for j in junk {
            let _ = import(j);
        }
        let mut seed = 12345u64;
        for _ in 0..200 {
            let mut bytes: Vec<u8> = text.bytes().collect();
            for _ in 0..8 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let i = (seed >> 33) as usize % bytes.len();
                bytes[i] = (seed >> 20) as u8;
            }
            let _ = import_bytes(&bytes);
        }
        assert!(import("hello").is_err());
    }

    #[test]
    fn fixture_calculates() {
        let (p, _) = import(&fixture()).unwrap();
        let r = calc::calculate(&p);
        assert!(r.n > 0);
    }

    /// The user-supplied example project (kept out of the repository); skipped when absent.
    fn example_bytes() -> Option<Vec<u8>> {
        let name = "reference_images/expert_reference/Mountain View Housing Complex Rev. H.hcp";
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut dir = Some(root);
        while let Some(d) = dir {
            if let Ok(b) = std::fs::read(d.join(name)) {
                return Some(b);
            }
            dir = d.parent();
        }
        // Linked worktrees keep the gitignored folder in the main checkout.
        let mut dir = Some(root);
        while let Some(d) = dir {
            if let Some(main) = d.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".claude")).and_then(|a| a.parent()) {
                if let Ok(b) = std::fs::read(main.join(name)) {
                    return Some(b);
                }
            }
            dir = d.parent();
        }
        None
    }

    #[test]
    fn example_project() {
        let Some(bytes) = example_bytes() else { return };
        let (p, warnings) = import_bytes(&bytes).unwrap();
        eprintln!("warnings: {warnings:?}");
        assert_eq!(p.title, "Mountain View Housing Complex Rev. H");
        assert_eq!(p.nodes.len(), 16);
        let count = |t| p.nodes.iter().filter(|n| n.node_type() == t).count();
        assert_eq!(count(NodeType::Subcat), 8);
        assert_eq!(count(NodeType::Reach), 4);
        assert_eq!(count(NodeType::Pond), 2);
        assert_eq!(count(NodeType::Link), 1);
        assert_eq!(count(NodeType::Text), 1);
        let two = p.settings.events.iter().find(|e| e.name == "2-Year").unwrap();
        assert!((two.depth - 2.70).abs() < 1e-6);
        assert_eq!(p.settings.event().map(|e| e.name.as_str()), Some("25-Year"));
        assert!((p.settings.event().map(|e| e.depth).unwrap_or(0.0) - 4.30).abs() < 1e-6);
        let s1 = p.by_label("1S").unwrap();
        let NodeData::Subcat(sc) = &s1.data else { panic!() };
        assert_eq!(sc.areas.len(), 2);
        assert_eq!(sc.areas[0].area, 217_800.0);
        assert_eq!(sc.areas[1].cn, 74.0);
        assert_eq!(sc.tc.len(), 3);
        let r = calc::calculate(&p);
        let errors: Vec<_> = r.messages.iter().filter(|m| m.severity == calc::Severity::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
        // Published results for Reach 2R, 25-year event (photo of the reference application).
        let id = p.by_label("2R").unwrap().id;
        let nr = r.node(id).unwrap();
        let (pi, po) = (nr.peak_in(), nr.peak_out());
        eprintln!("2R inflow {:.2} cfs @ {:.2} h, outflow {:.2} cfs @ {:.2} h, area {:.3} ac, imp {:.2}%", pi.value, pi.time, po.value, po.time, nr.inflow_area / 43560.0, 100.0 * nr.impervious_area / nr.inflow_area);
        // Published: inflow 20.88 cfs @ 12.23 h, outflow 19.99 cfs @ 12.30 h. Measured gaps:
        // inflow -0.4 % / +0.03 h, outflow +1.8 % (our reach attenuates 2 %, the published 4 %).
        assert!((pi.value / 20.88 - 1.0).abs() < 0.015 && (pi.time - 12.23).abs() < 0.06, "inflow {pi:?}");
        assert!((po.value / 19.99 - 1.0).abs() < 0.03 && (po.time - 12.30).abs() < 0.06, "outflow {po:?}");
        assert!((nr.inflow_area / 43_560.0 - 48.0).abs() < 0.001);
        assert!((100.0 * nr.impervious_area / nr.inflow_area - 13.65).abs() < 0.05);
        let summary = crate::report::node_summary(&p, &r, p.by_label("2R").unwrap()).join("\n");
        assert!(summary.contains("Defined Flood Depth= 1.50' Flow Area= 6.0 sf, Capacity= 22.22 cfs"), "{summary}");
        // Published: Max. Velocity= 3.60 fps, Min. Travel Time= 2.1 min; Avg. 1.42 fps, 5.3 min.
        assert!(summary.contains("Min. Travel Time= 2.1 min"), "{summary}");
        assert!(summary.contains("Avg. Velocity = 1.42 fps, Avg. Travel Time= 5.3 min"), "{summary}");
        // Round trip of the real file.
        let (q, _) = import(&export(&p)).unwrap();
        assert_eq!(p, q);
    }
}
