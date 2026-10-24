//! Automatic pipe and culvert sizing (Calculation Settings > Resize).
//!
//! Round pipe reaches and round culvert outlets marked `auto_size` are set to the smallest
//! standard diameter that keeps the current event below the design level:
//! - a pipe reach: peak flow depth at most `PIPE_FULL_FRACTION` of the diameter (sized for
//!   pipe-full Manning flow, entrance losses ignored as for any reach);
//! - a culvert: pond peak elevation at most the culvert crown (the outlet crown when the barrel
//!   slopes backwards).
//!
//! With the flood-elevation criterion a defined flood level (reach flood depth, pond flood
//! elevation) replaces the crown; items without one still use the crown. Nodes are sized in flow
//! order so every size is chosen with the final sizes upstream. The criteria fall monotonically
//! with the diameter, so each node is sized by bisection over the standard sizes.

use crate::calc::{self, Detail, Message, Results, Severity};
use crate::geometry::{PipeShape, Section};
use crate::outlets::DeviceKind;
use crate::project::{NodeData, NodeId, Project};
use serde::{Deserialize, Serialize};

/// Largest peak depth of a resized pipe reach, as a fraction of its diameter.
pub const PIPE_FULL_FRACTION: f64 = 0.98;

/// What a resized pipe or culvert must stay below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ResizeCriterion {
    /// Pipe reaches flow (nearly) full; culvert headwater reaches the crown.
    #[default]
    Crown,
    /// The node's flood elevation (reach flood depth or pond flood elevation) where defined.
    FloodElevation,
}

impl ResizeCriterion {
    pub const ALL: [ResizeCriterion; 2] = [ResizeCriterion::Crown, ResizeCriterion::FloodElevation];
    pub fn label(self) -> &'static str {
        match self {
            ResizeCriterion::Crown => "Pipe full / headwater at culvert crown",
            ResizeCriterion::FloodElevation => "Flood elevation (crown where none is defined)",
        }
    }
}

/// Resize settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResizeSettings {
    pub criterion: ResizeCriterion,
    /// Available diameters (ft), any order.
    pub sizes: Vec<f64>,
    /// Allow a size below the one entered; otherwise items only grow.
    pub allow_smaller: bool,
}

impl Default for ResizeSettings {
    fn default() -> Self {
        ResizeSettings { criterion: ResizeCriterion::Crown, sizes: STANDARD_INCHES.iter().map(|d| d / 12.0).collect(), allow_smaller: true }
    }
}

/// Common nominal diameters of round storm-drain pipe (inches).
pub const STANDARD_INCHES: [f64; 24] = [12.0, 15.0, 18.0, 21.0, 24.0, 27.0, 30.0, 33.0, 36.0, 42.0, 48.0, 54.0, 60.0, 66.0, 72.0, 78.0, 84.0, 90.0, 96.0, 102.0, 108.0, 114.0, 120.0, 144.0];

/// Whether a reach can be resized (round pipe).
pub fn reach_resizable(section: &Section) -> bool {
    matches!(section, Section::Pipe { shape: PipeShape::Round, .. })
}

/// Whether an outlet device can be resized (round culvert).
pub fn device_resizable(kind: &DeviceKind) -> bool {
    matches!(kind, DeviceKind::Culvert { shape: PipeShape::Round, .. })
}

/// Current diameter of the node's resizable items (the first one), if it has any marked.
fn current_size(project: &Project, id: NodeId) -> Option<f64> {
    match &project.node(id)?.data {
        NodeData::Reach(r) if r.auto_size => match r.section {
            Section::Pipe { shape: PipeShape::Round, width, .. } => Some(width),
            _ => None,
        },
        NodeData::Pond(p) => p.devices.iter().filter(|d| d.auto_size).find_map(|d| match d.kind {
            DeviceKind::Culvert { shape: PipeShape::Round, width, .. } => Some(width),
            _ => None,
        }),
        _ => None,
    }
}

/// Sets the diameter of every resizable item of the node.
fn set_size(project: &mut Project, id: NodeId, d: f64) {
    let Some(node) = project.node_mut(id) else { return };
    match &mut node.data {
        NodeData::Reach(r) if r.auto_size => {
            if let Section::Pipe { shape: PipeShape::Round, width, height, .. } = &mut r.section {
                *width = d;
                *height = d;
            }
        }
        NodeData::Pond(p) => {
            for dev in p.devices.iter_mut().filter(|d| d.auto_size) {
                if let DeviceKind::Culvert { shape: PipeShape::Round, width, height, .. } = &mut dev.kind {
                    *width = d;
                    *height = d;
                }
            }
        }
        _ => {}
    }
}

/// Whether the node meets the design criterion in these results.
fn meets(project: &Project, id: NodeId, r: &Results) -> bool {
    let (Some(node), Some(res)) = (project.node(id), r.node(id)) else { return false };
    if res.undefined.is_some() {
        return false;
    }
    let flood = project.settings.resize.criterion == ResizeCriterion::FloodElevation;
    match (&node.data, &res.detail) {
        (NodeData::Reach(reach), Detail::Reach(d)) => {
            let limit = match reach.flood_depth {
                Some(f) if flood => f,
                _ => PIPE_FULL_FRACTION * reach.section.full_depth(),
            };
            d.peak_depth <= limit + 1e-9
        }
        (NodeData::Pond(pond), Detail::Pond(d)) => {
            let crown = pond
                .devices
                .iter()
                .filter(|dev| dev.auto_size)
                .filter_map(|dev| match dev.kind {
                    DeviceKind::Culvert { shape: PipeShape::Round, height, outlet_invert, .. } => Some(dev.invert.max(outlet_invert) + height),
                    _ => None,
                })
                .reduce(f64::min);
            let limit = match (pond.flood_elevation, crown) {
                (Some(f), _) if flood => f,
                (_, Some(c)) => c,
                _ => return true,
            };
            d.peak_elevation <= limit + 1e-9
        }
        _ => true,
    }
}

fn size_text(project: &Project, d: f64) -> String {
    let u = &project.units;
    if u.metric() { format!("{:.0} cm", u.small_len(d)) } else { format!("{:.0}\"", u.small_len(d)) }
}

/// Resizes the project's marked pipes and culverts for its current event and returns the
/// messages (59) describing what changed. Does nothing when no item is marked.
pub fn resize(project: &mut Project) -> Vec<Message> {
    let mut msgs = Vec::new();
    let Ok(order) = calc::topological_order(project) else { return msgs };
    let targets: Vec<NodeId> = order.iter().copied().filter(|id| current_size(project, *id).is_some()).collect();
    if targets.is_empty() {
        return msgs;
    }
    let Some(event) = project.settings.event().cloned() else { return msgs };
    let mut sizes: Vec<f64> = project.settings.resize.sizes.iter().copied().filter(|d| d.is_finite() && *d > 0.0).collect();
    sizes.sort_by(f64::total_cmp);
    sizes.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    for id in targets {
        let Some(d0) = current_size(project, id) else { continue };
        let candidates: Vec<f64> = sizes.iter().copied().filter(|d| project.settings.resize.allow_smaller || *d >= d0 - 1e-9).collect();
        let Some(&largest) = candidates.last() else { continue };
        let mut trial = project.clone();
        let mut passes = |d: f64| {
            set_size(&mut trial, id, d);
            let r = calc::calculate_event(&trial, &event);
            meets(&trial, id, &r)
        };
        // Smallest passing candidate by bisection: candidates[lo..] pass, [..lo] fail.
        let (mut lo, mut hi) = (0, candidates.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if candidates.get(mid).is_some_and(|d| passes(*d)) { hi = mid } else { lo = mid + 1 }
        }
        let label = project.node(id).map(|n| n.label()).unwrap_or_default();
        let what = if matches!(project.node(id).map(|n| &n.data), Some(NodeData::Reach(_))) { "Pipe reach" } else { "Culvert outlet" };
        let (d, ok) = match candidates.get(lo) {
            Some(d) => (*d, true),
            None => (largest, false),
        };
        set_size(project, id, d);
        if !ok {
            msgs.push(Message::new(59, Severity::Warning, Some(id), format!("{label} {what} resized to {} (largest size, still too small)", size_text(project, d))));
        } else if (d - d0).abs() > 1e-9 {
            msgs.push(Message::new(59, Severity::Hint, Some(id), format!("{label} {what} resized to {}", size_text(project, d))));
        }
    }
    msgs
}

/// Resizes the project (see [`resize`]) and calculates it, with the resize messages included.
pub fn calculate(project: &mut Project) -> Results {
    let msgs = resize(project);
    let mut r = calc::calculate(project);
    let at = r.messages.iter().position(|m| m.code == 31).unwrap_or(r.messages.len());
    r.messages.splice(at..at, msgs);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn::SubArea;
    use crate::outlets::{OutletDevice, Routing};
    use crate::project::{NodeType, Pond, Reach, Subcatchment};
    use crate::rainfall::RainEvent;
    use crate::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
    use crate::tc::{TcKind, TcSegment};

    const AC: f64 = 43_560.0;

    fn find(p: &Project, label: &str) -> NodeId {
        p.by_label(label).map(|n| n.id).unwrap()
    }

    /// A 25-acre subcatchment draining to a node of type `t` with the given data.
    fn project_with(t: NodeType, data: NodeData) -> Project {
        let mut p = Project::default();
        p.settings.events = vec![RainEvent::new("25-Year", "Type II 24-hr", 4.3)];
        p.settings.current_event = 0;
        p.settings.span_start = 10.0;
        p.settings.span_end = 40.0;
        p.settings.dt = 0.1;
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let id = p.add_node(t, [100.0, 0.0]);
        if let Some(n) = p.node_mut(s) {
            n.outflows[0] = Some(id);
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![SubArea::new(25.0 * AC, 75.0, "Mixed cover, HSG C")],
                tc: vec![TcSegment::new(TcKind::Direct { minutes: 15.0, length: 0.0, velocity: 0.0 }, "")],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(id) {
            n.data = data;
        }
        p
    }

    /// A subcatchment draining to a pipe reach marked for sizing.
    fn pipe_project() -> Project {
        let reach = Reach { section: Section::circular(1.0), n: 0.013, length: 300.0, inlet_invert: 103.0, outlet_invert: 100.0, auto_size: true, ..Reach::default() };
        project_with(NodeType::Reach, NodeData::Reach(reach))
    }

    #[test]
    fn pipe_reach_gets_smallest_size_that_flows_below_full() {
        let mut p = pipe_project();
        let msgs = resize(&mut p);
        let id = find(&p, "1R");
        let d = current_size(&p, id).unwrap();
        assert!(msgs.iter().any(|m| m.code == 59), "{msgs:?}");
        let r = crate::calc::calculate(&p);
        assert!(meets(&p, id, &r));
        // The next smaller standard size would run fuller than the limit.
        let smaller = STANDARD_INCHES.iter().map(|x| x / 12.0).rfind(|x| *x < d - 1e-9).unwrap();
        let mut q = p.clone();
        set_size(&mut q, id, smaller);
        assert!(!meets(&q, id, &crate::calc::calculate(&q)));
        // Sizing again is stable and silent.
        assert!(resize(&mut p).is_empty());
    }

    #[test]
    fn grow_only_keeps_a_larger_pipe() {
        let mut p = pipe_project();
        let id = find(&p, "1R");
        set_size(&mut p, id, 10.0);
        p.settings.resize.allow_smaller = false;
        assert!(resize(&mut p).is_empty());
        assert_eq!(current_size(&p, id), Some(10.0));
    }

    #[test]
    fn culvert_keeps_pond_below_crown_or_flood_level() {
        let culvert = DeviceKind::Culvert { shape: PipeShape::Round, width: 1.0, height: 1.0, length: 50.0, outlet_invert: 99.5, n: 0.013, ke: 0.5, cc: 0.9, fill: 0.0, entrance: String::new() };
        let mut dev = OutletDevice::new("Culvert", 100.0, Routing::Primary, culvert);
        dev.auto_size = true;
        let volume = StorageVolume::new("Basin", 100.0, VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: vec![100.0, 112.0], values: vec![0.3 * AC, 0.6 * AC] });
        let pond = Pond { storage: PondStorage { volumes: vec![volume] }, devices: vec![dev], ..Pond::default() };
        let mut p = project_with(NodeType::Pond, NodeData::Pond(pond));
        let id = find(&p, "1P");
        resize(&mut p);
        let crown_d = current_size(&p, id).unwrap();
        let r = crate::calc::calculate(&p);
        let Some(Detail::Pond(d)) = r.node(id).map(|n| &n.detail) else { panic!() };
        assert!(d.peak_elevation <= 100.0 + crown_d);
        // A flood level above the crown allows a smaller culvert.
        if let Some(NodeData::Pond(pd)) = p.node_mut(id).map(|n| &mut n.data) {
            pd.flood_elevation = Some(100.0 + crown_d + 3.0);
        }
        p.settings.resize.criterion = ResizeCriterion::FloodElevation;
        resize(&mut p);
        assert!(current_size(&p, id).unwrap() < crown_d);
    }

    #[test]
    fn unmarked_projects_are_untouched() {
        let mut p = pipe_project();
        if let Some(NodeData::Reach(r)) = p.node_mut(find(&p, "1R")).map(|n| &mut n.data) {
            r.auto_size = false;
        }
        let before = p.clone();
        assert!(resize(&mut p).is_empty());
        assert_eq!(p, before);
    }
}
