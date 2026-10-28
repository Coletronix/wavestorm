//! The project model: nodes on the routing diagram, their data, and calculation settings.
//!
//! Projects are saved as JSON (`*.waves`). All stored values are in internal units.

use crate::cn::{CnWeighting, SubArea};
use crate::geometry::Section;
use crate::link::Link;
use crate::outlets::OutletDevice;
use crate::rainfall::{RainEvent, Storm};
use crate::storage::PondStorage;
use crate::tc::TcSegment;
use crate::uh::UnitHydrograph;
use crate::units::Units;
use serde::{Deserialize, Serialize};

/// Stable node identifier (never reused within a project).
pub type NodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    Subcat,
    Reach,
    Pond,
    Link,
    Text,
}

impl NodeType {
    pub fn letter(self) -> &'static str {
        match self {
            NodeType::Subcat => "S",
            NodeType::Reach => "R",
            NodeType::Pond => "P",
            NodeType::Link => "L",
            NodeType::Text => "T",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            NodeType::Subcat => "Subcatchment",
            NodeType::Reach => "Reach",
            NodeType::Pond => "Pond",
            NodeType::Link => "Link",
            NodeType::Text => "Text",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            NodeType::Subcat => "Subcat",
            NodeType::Reach => "Reach",
            NodeType::Pond => "Pond",
            NodeType::Link => "Link",
            NodeType::Text => "Text",
        }
    }
    /// Which outflows the node type offers.
    pub fn outflow_count(self) -> usize {
        match self {
            NodeType::Subcat | NodeType::Reach => 1,
            NodeType::Pond => 3,
            NodeType::Link => 2,
            NodeType::Text => 0,
        }
    }
    pub fn accepts_inflow(self) -> bool {
        matches!(self, NodeType::Reach | NodeType::Pond | NodeType::Link)
    }
}

/// Subcatchment data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Subcatchment {
    pub areas: Vec<SubArea>,
    pub tc: Vec<TcSegment>,
    /// Unit hydrograph override (name); `None` uses the project setting.
    #[serde(default)]
    pub uh: Option<String>,
    /// Report this subcatchment's areas in large units (acres) or small (sq ft).
    #[serde(default = "yes")]
    pub large_units: bool,
}

fn yes() -> bool {
    true
}

/// Reach routing method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReachMethod {
    StorInd,
    #[default]
    StorIndTrans,
    MuskingumCunge,
}

impl ReachMethod {
    pub const ALL: [ReachMethod; 3] = [ReachMethod::StorInd, ReachMethod::StorIndTrans, ReachMethod::MuskingumCunge];
    pub fn label(self) -> &'static str {
        match self {
            ReachMethod::StorInd => "Stor-Ind",
            ReachMethod::StorIndTrans => "Stor-Ind+Trans",
            ReachMethod::MuskingumCunge => "Muskingum-Cunge",
        }
    }
}

/// Pond routing method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PondMethod {
    #[default]
    StorInd,
    /// Tailwater from the downstream node's previous step.
    DynStorInd,
}

impl PondMethod {
    pub const ALL: [PondMethod; 2] = [PondMethod::StorInd, PondMethod::DynStorInd];
    pub fn label(self) -> &'static str {
        match self {
            PondMethod::StorInd => "Stor-Ind",
            PondMethod::DynStorInd => "Dyn-Stor-Ind",
        }
    }
}

/// Reach (open channel or pipe) data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reach {
    pub section: Section,
    pub n: f64,
    pub length: f64,
    pub inlet_invert: f64,
    pub outlet_invert: f64,
    /// Constant exfiltration loss (cfs), discarded.
    #[serde(default)]
    pub exfiltration: f64,
    /// Defined flood depth above the invert (reported with its flow area and capacity).
    #[serde(default)]
    pub flood_depth: Option<f64>,
    /// Sized automatically (round pipes only; see `resize`).
    #[serde(default)]
    pub auto_size: bool,
}

impl Default for Reach {
    fn default() -> Self {
        Reach {
            section: Section::Trapezoidal { bottom: 4.0, z_left: 2.0, z_right: 2.0, depth: 2.0 },
            n: 0.030,
            length: 0.0,
            inlet_invert: 0.0,
            outlet_invert: 0.0,
            exfiltration: 0.0,
            flood_depth: None,
            auto_size: false,
        }
    }
}

impl Reach {
    pub fn slope(&self) -> f64 {
        if self.length > 0.0 { (self.inlet_invert - self.outlet_invert) / self.length } else { 0.0 }
    }
    pub fn is_described(&self) -> bool {
        // A rating table supplies discharge directly, so it needs no slope or roughness.
        let hydraulics = matches!(self.section, Section::Rating { .. }) || (self.slope() > 0.0 && self.n > 0.0);
        self.length > 0.0 && hydraulics && self.section.full_depth() > 0.0
    }
}

/// Tailwater applied to an outflow's devices.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Tailwater {
    #[default]
    Free,
    Fixed(f64),
    /// Tailwater stays this far below the headwater.
    Differential(f64),
    /// Taken from the downstream node (dynamic routing only).
    Dynamic,
}

/// Pond data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pond {
    pub storage: PondStorage,
    pub devices: Vec<OutletDevice>,
    /// Starting water-surface elevation; `None` = bottom of storage.
    #[serde(default)]
    pub start_elevation: Option<f64>,
    #[serde(default)]
    pub flood_elevation: Option<f64>,
    /// Tailwater for primary, secondary, tertiary outflows.
    #[serde(default)]
    pub tailwater: [Tailwater; 3],
    #[serde(default = "yes")]
    pub large_units: bool,
    /// The chamber wizard's field, when the storage was laid out with it.
    #[serde(default)]
    pub chamber: Option<crate::chamber::Field>,
    /// The pond estimator's inputs, when the storage was estimated from a drawn footprint.
    #[serde(default)]
    pub estimate: Option<crate::drawing::PondEstimate>,
}

/// A free-text (and optional image) annotation on the diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextNode {
    pub text: String,
    pub size: f32,
    pub border: bool,
}

impl Default for TextNode {
    fn default() -> Self {
        TextNode { text: "Text".into(), size: 14.0, border: false }
    }
}

/// Node-type specific data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeData {
    Subcat(Subcatchment),
    Reach(Reach),
    Pond(Pond),
    Link(Link),
    Text(TextNode),
}

impl NodeData {
    pub fn node_type(&self) -> NodeType {
        match self {
            NodeData::Subcat(_) => NodeType::Subcat,
            NodeData::Reach(_) => NodeType::Reach,
            NodeData::Pond(_) => NodeType::Pond,
            NodeData::Link(_) => NodeType::Link,
            NodeData::Text(_) => NodeType::Text,
        }
    }
    pub fn empty(t: NodeType) -> NodeData {
        match t {
            NodeType::Subcat => NodeData::Subcat(Subcatchment { large_units: true, ..Default::default() }),
            NodeType::Reach => NodeData::Reach(Reach::default()),
            NodeType::Pond => NodeData::Pond(Pond { large_units: true, ..Default::default() }),
            NodeType::Link => NodeData::Link(Link::default()),
            NodeType::Text => NodeData::Text(TextNode::default()),
        }
    }
}

/// A key/value pair from an interchange file that the model does not represent, kept so that
/// saving the project writes it back unchanged. `scope` names the section it came from
/// ("" = the node or project header, "AREA:0" = the first area record, ...).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ForeignEntry {
    pub scope: String,
    pub key: String,
    pub value: String,
}

/// One node of the routing diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// The node "number" shown in the symbol (may contain letters, e.g. "1" or "A2").
    pub number: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    /// Diagram position of the symbol's centre (diagram units).
    pub pos: [f64; 2],
    /// Primary, secondary and tertiary outflow targets.
    #[serde(default)]
    pub outflows: [Option<NodeId>; 3],
    pub data: NodeData,
    /// Protects the node from merge/import updates.
    #[serde(default)]
    pub locked: bool,
    /// Traced outline on the diagram (diagram units), for subcatchments drawn over a background.
    #[serde(default)]
    pub outline: Option<Vec<[f64; 2]>>,
    /// The Tc flow path drawn on the diagram (diagram units, upstream end first), for
    /// subcatchments whose Tc segment lengths were measured from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tc_path: Option<Vec<[f64; 2]>>,
    /// The `tc_path` points lettered A, B, C… on the diagram (segment ends), in order; empty
    /// letters every point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tc_path_breaks: Vec<usize>,
    /// Interchange-file data this model does not represent (see `hcp`).
    #[serde(default)]
    pub foreign: Vec<ForeignEntry>,
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        self.data.node_type()
    }
    /// "1S", "2P", ...
    pub fn label(&self) -> String {
        format!("{}{}", self.number, self.node_type().letter())
    }
    /// The display units for this node's own values: the project's units, with large areas and
    /// volumes (acres, acre-feet) only when the node's "Large units" box is also checked.
    pub fn units(&self, project: &Units) -> Units {
        let large = match &self.data {
            NodeData::Subcat(sc) => sc.large_units,
            NodeData::Pond(p) => p.large_units,
            _ => true,
        };
        Units { large: project.large && large, ..*project }
    }

    /// "Subcatchment 1S: Name"
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            format!("{} {}", self.node_type().label(), self.label())
        } else {
            format!("{} {}: {}", self.node_type().label(), self.label(), self.name)
        }
    }
}

/// Runoff method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RunoffMethod {
    #[default]
    ScsTr20,
    Sbuh,
    Rational,
}

impl RunoffMethod {
    pub const ALL: [RunoffMethod; 3] = [RunoffMethod::ScsTr20, RunoffMethod::Sbuh, RunoffMethod::Rational];
    pub fn label(self) -> &'static str {
        match self {
            RunoffMethod::ScsTr20 => "SCS TR-20",
            RunoffMethod::Sbuh => "SBUH",
            RunoffMethod::Rational => "Rational",
        }
    }
}

/// Intensity-duration-frequency coefficients i = a / (b + t)^c (t in minutes, i in in/hr), one
/// set per event name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdfCurve {
    pub name: String,
    pub events: Vec<(String, f64, f64, f64)>,
    /// Events given as discrete (duration minutes, intensity in/hr) points instead of
    /// coefficients (from IDF files and NOAA tables).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<(String, Vec<(f64, f64)>)>,
}

impl IdfCurve {
    pub fn intensity(&self, event: &str, minutes: f64) -> Option<f64> {
        if let Some((_, a, b, c)) = self.events.iter().find(|e| e.0.eq_ignore_ascii_case(event)) {
            let d = (b + minutes).powf(*c);
            return (d > 0.0).then(|| a / d);
        }
        let (_, pts) = self.tables.iter().find(|e| e.0.eq_ignore_ascii_case(event))?;
        interpolate_loglog(pts, minutes)
    }
}

/// Interpolates (duration, intensity) points linearly in log-log space, holding the end values
/// beyond the first and last point. `pts` must be sorted by duration.
pub fn interpolate_loglog(pts: &[(f64, f64)], x: f64) -> Option<f64> {
    let (first, last) = (pts.first()?, pts.last()?);
    if !(x.is_finite() && x > 0.0) {
        return None;
    }
    if x <= first.0 {
        return Some(first.1);
    }
    if x >= last.0 {
        return Some(last.1);
    }
    let i = pts.partition_point(|p| p.0 <= x).saturating_sub(1);
    let (a, b) = (pts.get(i)?, pts.get(i + 1)?);
    if !(a.0 > 0.0 && b.0 > a.0 && a.1 > 0.0 && b.1 > 0.0) {
        return Some(a.1);
    }
    let s = (x / a.0).ln() / (b.0 / a.0).ln();
    Some((a.1.ln() + s * (b.1 / a.1).ln()).exp())
}

/// Calculation settings (the calculation-settings dialog).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalcSettings {
    pub runoff: RunoffMethod,
    pub uh: String,
    pub events: Vec<RainEvent>,
    pub current_event: usize,
    /// Calculation time span (hours) and step.
    pub span_start: f64,
    pub span_end: f64,
    pub dt: f64,
    pub reach_method: ReachMethod,
    pub pond_method: PondMethod,
    /// Initial abstraction ratio Ia/S.
    pub ia_ratio: f64,
    pub cn_weighting: CnWeighting,
    /// Round composite CN to whole numbers.
    pub round_cn: bool,
    /// Minimum Tc (minutes), applied after calculating.
    #[serde(default)]
    pub min_tc: Option<f64>,
    /// Rational method rise and fall factors.
    pub rational_rise: f64,
    pub rational_fall: f64,
    #[serde(default)]
    pub idf: Option<IdfCurve>,
    /// Reporting step for tabular hydrographs (0 = automatic "shrink").
    #[serde(default)]
    pub table_step: f64,
    /// Report impervious/pervious areas separately.
    #[serde(default = "yes")]
    pub report_impervious: bool,
    /// Automatic pipe and culvert sizing.
    #[serde(default)]
    pub resize: crate::resize::ResizeSettings,
}

impl Default for CalcSettings {
    fn default() -> Self {
        let storm = "Type II 24-hr";
        CalcSettings {
            runoff: RunoffMethod::ScsTr20,
            uh: "SCS".into(),
            events: vec![
                RainEvent::new("2-Year", storm, 3.10),
                RainEvent::new("10-Year", storm, 4.50),
                RainEvent::new("25-Year", storm, 5.40),
                RainEvent::new("100-Year", storm, 7.00),
            ],
            current_event: 1,
            span_start: 0.0,
            span_end: 30.0,
            dt: 0.05,
            reach_method: ReachMethod::StorIndTrans,
            pond_method: PondMethod::StorInd,
            ia_ratio: 0.2,
            cn_weighting: CnWeighting::WeightedCn,
            round_cn: true,
            min_tc: None,
            rational_rise: 1.0,
            rational_fall: 1.0,
            idf: None,
            table_step: 0.0,
            report_impervious: true,
            resize: crate::resize::ResizeSettings::default(),
        }
    }
}

impl CalcSettings {
    pub fn event(&self) -> Option<&RainEvent> {
        self.events.get(self.current_event).or(self.events.first())
    }
    pub fn steps(&self) -> usize {
        if self.dt > 0.0 && self.span_end > self.span_start {
            (((self.span_end - self.span_start) / self.dt).round() as usize + 1).min(2_000_001)
        } else {
            0
        }
    }
}

/// A calibrated background image or PDF page behind the routing diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
    pub file_name: String,
    /// Original file bytes (image or PDF), base64 in the saved project.
    #[serde(with = "crate::base64::serde_bytes")]
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub page: usize,
    /// Diagram position of the image's top-left corner.
    pub origin: [f64; 2],
    /// Diagram units per image pixel.
    pub scale: f64,
    /// Rotation (radians) about the origin.
    #[serde(default)]
    pub rotation: f64,
    /// Real-world feet per diagram unit, set by two-point calibration.
    pub feet_per_unit: Option<f64>,
    pub opacity: f32,
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

/// Diagram display settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramSettings {
    /// Node symbol diameter (diagram units).
    pub symbol_size: f64,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub grid: f64,
    #[serde(default)]
    pub underlay: Option<Underlay>,
}

impl Default for DiagramSettings {
    fn default() -> Self {
        DiagramSettings { symbol_size: 48.0, show_grid: false, snap_to_grid: false, grid: 24.0, underlay: None }
    }
}

/// A complete project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub title: String,
    #[serde(default)]
    pub prepared_by: String,
    #[serde(default)]
    pub notes: String,
    pub units: Units,
    pub settings: CalcSettings,
    pub diagram: DiagramSettings,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub storms: Vec<Storm>,
    #[serde(default)]
    pub uhs: Vec<UnitHydrograph>,
    #[serde(default)]
    pub land_use: crate::landuse::LandUseSettings,
    pub next_id: NodeId,
    /// Interchange-file data this model does not represent (see `hcp`).
    #[serde(default)]
    pub foreign: Vec<ForeignEntry>,
}

pub const FORMAT_VERSION: u32 = 1;

impl Default for Project {
    fn default() -> Self {
        Project {
            format: FORMAT_VERSION,
            title: "Untitled".into(),
            prepared_by: String::new(),
            notes: String::new(),
            units: Units::default(),
            settings: CalcSettings::default(),
            diagram: DiagramSettings::default(),
            nodes: Vec::new(),
            storms: Vec::new(),
            uhs: Vec::new(),
            land_use: crate::landuse::LandUseSettings::default(),
            next_id: 1,
            foreign: Vec::new(),
        }
    }
}

impl Project {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn by_label(&self, label: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.label().eq_ignore_ascii_case(label))
    }

    /// Lowest unused node number for a type ("1", "2", ...).
    pub fn next_number(&self, t: NodeType) -> String {
        let used: Vec<u64> =
            self.nodes.iter().filter(|n| n.node_type() == t).filter_map(|n| n.number.parse::<u64>().ok()).collect();
        let mut k = 1;
        while used.contains(&k) {
            k += 1;
        }
        k.to_string()
    }

    /// Adds an empty node and returns its id.
    pub fn add_node(&mut self, t: NodeType, pos: [f64; 2]) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let number = self.next_number(t);
        self.nodes.push(Node {
            id,
            number,
            name: String::new(),
            notes: String::new(),
            pos,
            outflows: [None; 3],
            data: NodeData::empty(t),
            locked: false,
            outline: None,
            tc_path: None,
            tc_path_breaks: Vec::new(),
            foreign: Vec::new(),
        });
        id
    }

    /// Removes a node and any arrows pointing at it.
    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        let i = self.nodes.iter().position(|n| n.id == id)?;
        let node = self.nodes.remove(i);
        for n in &mut self.nodes {
            for o in &mut n.outflows {
                if *o == Some(id) {
                    *o = None;
                }
            }
        }
        Some(node)
    }

    /// Nodes whose outflow `k` goes to `id`.
    pub fn inflows(&self, id: NodeId) -> Vec<(NodeId, usize)> {
        let mut v = Vec::new();
        for n in &self.nodes {
            for (k, o) in n.outflows.iter().enumerate() {
                if *o == Some(id) {
                    v.push((n.id, k));
                }
            }
        }
        v
    }

    pub fn storm(&self, name: &str) -> Option<Storm> {
        self.storms
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .or_else(|| crate::rainfall::builtin_storms().into_iter().find(|s| s.name == name))
    }

    pub fn unit_hydrograph(&self, name: &str) -> Option<UnitHydrograph> {
        self.uhs
            .iter()
            .find(|u| u.name == name)
            .cloned()
            .or_else(|| crate::uh::builtin_uhs().into_iter().find(|u| u.name == name))
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(text: &str) -> Result<Project, String> {
        let p: Project = serde_json::from_str(text).map_err(|e| format!("Cannot read project: {e}"))?;
        if p.format > FORMAT_VERSION {
            return Err(format!("This project was saved by a newer version (format {}).", p.format));
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_labels() {
        let mut p = Project::default();
        let a = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let b = p.add_node(NodeType::Pond, [100.0, 0.0]);
        let c = p.add_node(NodeType::Subcat, [0.0, 100.0]);
        assert_eq!(p.node(c).unwrap().label(), "2S");
        p.node_mut(a).unwrap().outflows[0] = Some(b);
        assert_eq!(p.inflows(b), vec![(a, 0)]);
        p.remove_node(b);
        assert_eq!(p.node(a).unwrap().outflows[0], None);
        p.remove_node(a);
        assert_eq!(p.next_number(NodeType::Subcat), "1");
    }

    #[test]
    fn json_round_trip() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Reach, [10.0, 20.0]);
        p.node_mut(id).unwrap().name = "Swale".into();
        let text = p.to_json().unwrap();
        let q = Project::from_json(&text).unwrap();
        assert_eq!(p, q);
        assert!(Project::from_json("{").is_err());
    }
}
