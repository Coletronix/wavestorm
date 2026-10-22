//! Node profiles: side views (horizontal distance in feet, elevation in feet) of subcatchment
//! flow paths, reaches and ponds, for checking elevations of channels, storage and outlets.
//! Several nodes can be chained left to right, in the order given.

use crate::calc::{Detail, Results, pond_bottom};
use crate::outlets::DeviceKind;
use crate::project::{Node, NodeData, NodeId, Project};
use crate::tc::TcKind;

/// What a shape represents (the viewer chooses colours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Overland flow path of a subcatchment.
    FlowPath,
    /// Channel bed and banks, pond walls.
    Structure,
    /// Water at the peak level (filled).
    Water,
    /// Outlet device marker.
    Device,
    /// Outlet pipe or weir overflow leaving a pond (filled).
    Outlet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub ink: Ink,
    pub points: Vec<[f64; 2]>,
    /// Filled polygon (otherwise an open polyline).
    pub filled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Text starts at the point, above it.
    Left,
    /// Text ends at the point, centred on it.
    Right,
    /// Text starts at the point, below it.
    Below,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub at: [f64; 2],
    pub text: String,
    pub anchor: Anchor,
    /// Node title (drawn bold).
    pub title: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    pub shapes: Vec<Shape>,
    pub labels: Vec<Label>,
}

impl Profile {
    /// Bounding box `[min_x, min_y, max_x, max_y]` of all shapes and labels.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let pts = self.shapes.iter().flat_map(|s| s.points.iter().copied()).chain(self.labels.iter().map(|l| l.at));
        pts.fold(None, |b: Option<[f64; 4]>, [x, y]| {
            Some(match b {
                None => [x, y, x, y],
                Some([a, c, d, e]) => [a.min(x), c.min(y), d.max(x), e.max(y)],
            })
        })
    }

    fn line(&mut self, ink: Ink, points: Vec<[f64; 2]>) {
        self.shapes.push(Shape { ink, points, filled: false });
    }

    fn fill(&mut self, ink: Ink, points: Vec<[f64; 2]>) {
        self.shapes.push(Shape { ink, points, filled: true });
    }

    fn label(&mut self, at: [f64; 2], text: impl Into<String>, anchor: Anchor, title: bool) {
        self.labels.push(Label { at, text: text.into(), anchor, title });
    }
}

/// The elevation at which flow enters a node (top of a reach's banks or of a pond).
fn entry_elevation(node: &Node) -> Option<f64> {
    match &node.data {
        NodeData::Reach(r) if r.is_described() => Some(r.inlet_invert + r.section.full_depth()),
        NodeData::Pond(p) if !p.storage.is_empty() || !p.devices.is_empty() => Some(pond_top(p)),
        _ => None,
    }
}

fn pond_top(p: &crate::project::Pond) -> f64 {
    let bottom = pond_bottom(p);
    let top_dev = p.devices.iter().map(|d| d.invert).fold(bottom, f64::max);
    p.storage.top().unwrap_or(top_dev + 2.0).max(top_dev + 0.5)
}

/// Profile of the given nodes side by side (left to right in this order). Subcatchments end at
/// the entry level of the next node (or at elevation 0); ponds are drawn `pond_width` wide.
pub fn chain(project: &Project, results: Option<&Results>, ids: &[NodeId]) -> Profile {
    let nodes: Vec<&Node> = ids.iter().filter_map(|id| project.node(*id)).collect();
    // Ponds have no length of their own; size them to the chain.
    let lengths: f64 = nodes.iter().map(|n| drawn_length(n)).sum();
    let pond_width = (lengths / nodes.len().max(1) as f64).max(60.0);
    let mut out = Profile::default();
    let mut x = 0.0;
    for (i, node) in nodes.iter().enumerate() {
        let next_entry = nodes.get(i + 1).and_then(|n| entry_elevation(n));
        x = add_node(results, node, x, next_entry, pond_width, &mut out);
        x += pond_width * 0.05;
    }
    out
}

fn drawn_length(node: &Node) -> f64 {
    match &node.data {
        NodeData::Subcat(s) => s.tc.iter().map(|t| t.kind.length()).sum(),
        NodeData::Reach(r) => r.length,
        _ => 0.0,
    }
}

fn slope(kind: &TcKind) -> f64 {
    match kind {
        TcKind::Sheet { slope, .. }
        | TcKind::SheetMcCuenSpiess { slope, .. }
        | TcKind::Shallow { slope, .. }
        | TcKind::Channel { slope, .. }
        | TcKind::Lag { slope, .. }
        | TcKind::Kirpich { slope, .. }
        | TcKind::Kerby { slope, .. } => slope.max(0.0),
        TcKind::Direct { .. } | TcKind::Lake { .. } => 0.0,
    }
}

/// Adds one node starting at `x0`; returns the x where it ends.
fn add_node(results: Option<&Results>, node: &Node, x0: f64, next_entry: Option<f64>, pond_width: f64, out: &mut Profile) -> f64 {
    let res = results.and_then(|r| r.node(node.id));
    match &node.data {
        NodeData::Subcat(s) => {
            let end = next_entry.unwrap_or(0.0);
            let segs: Vec<(f64, f64, String)> = s
                .tc
                .iter()
                .map(|t| (t.kind.length().max(0.0), slope(&t.kind), if t.description.is_empty() { t.kind.label().to_string() } else { t.description.clone() }))
                .collect();
            let total: f64 = segs.iter().map(|s| s.0).sum();
            if total <= 0.0 {
                // No lengths: a short flat line so the node still shows.
                out.line(Ink::FlowPath, vec![[x0, end], [x0 + pond_width * 0.5, end]]);
                out.label([x0, end], node.title(), Anchor::Left, true);
                return x0 + pond_width * 0.5;
            }
            let drop: f64 = segs.iter().map(|s| s.0 * s.1).sum();
            let (mut x, mut y) = (x0, end + drop);
            out.label([x0, y], node.title(), Anchor::Left, true);
            let mut path = vec![[x, y]];
            for (len, s, text) in &segs {
                out.label([x, y], text.clone(), Anchor::Below, false);
                out.line(Ink::Structure, vec![[x, y], [x, y - drop.max(1.0) * 0.04]]);
                x += len;
                y -= len * s;
                path.push([x, y]);
            }
            out.line(Ink::FlowPath, path);
            x
        }
        NodeData::Reach(r) => {
            let len = r.length.max(1.0);
            let full = r.section.full_depth();
            let (a, b) = ([x0, r.inlet_invert], [x0 + len, r.outlet_invert]);
            if let Some(Detail::Reach(d)) = res.map(|r| &r.detail) {
                let depth = d.peak_depth.max(0.0);
                if depth > 0.0 {
                    out.fill(Ink::Water, vec![a, b, [b[0], b[1] + depth], [a[0], a[1] + depth]]);
                }
            }
            out.line(Ink::Structure, vec![[a[0], a[1] + full], a, b, [b[0], b[1] + full]]);
            out.line(Ink::Structure, vec![[a[0], a[1] + full], [b[0], b[1] + full]]);
            out.label([x0, r.inlet_invert + full], node.title(), Anchor::Left, true);
            x0 + len
        }
        NodeData::Pond(p) => {
            let bottom = pond_bottom(p);
            let top = pond_top(p);
            let w = pond_width;
            let xr = x0 + w;
            if let Some(Detail::Pond(d)) = res.map(|r| &r.detail) {
                let peak = d.peak_elevation.clamp(bottom, top.max(d.peak_elevation));
                if peak > bottom {
                    out.fill(Ink::Water, vec![[x0, bottom], [xr, bottom], [xr, peak], [x0, peak]]);
                }
            }
            out.line(Ink::Structure, vec![[x0, top], [x0, bottom], [xr, bottom], [xr, top]]);
            out.label([x0, top], node.title(), Anchor::Left, true);
            let tick = w * 0.04;
            let mut right = xr;
            for d in &p.devices {
                let name = if d.description.is_empty() { d.kind.label().to_string() } else { d.description.clone() };
                out.line(Ink::Device, vec![[xr - tick, d.invert], [xr + tick, d.invert]]);
                out.label([xr - tick * 1.5, d.invert], name, Anchor::Right, false);
                match &d.kind {
                    DeviceKind::Culvert { height, outlet_invert, length, fill, .. } => {
                        let run = length.max(w * 0.6);
                        let h = (height - fill).max(0.0);
                        out.fill(Ink::Outlet, vec![[xr, d.invert], [xr + run, *outlet_invert], [xr + run, outlet_invert + h], [xr, d.invert + h]]);
                        right = right.max(xr + run);
                    }
                    DeviceKind::SharpCrestedWeir { .. } | DeviceKind::BroadCrestedWeir { .. } | DeviceKind::VeeTrapWeir { .. } | DeviceKind::CustomWeir { .. } => {
                        let run = w * 0.3;
                        let t = ((top - bottom) * 0.04).max(0.05);
                        out.fill(Ink::Outlet, vec![[xr, d.invert - t], [xr + run, d.invert - t], [xr + run, d.invert], [xr, d.invert]]);
                        right = right.max(xr + run);
                    }
                    _ => {}
                }
            }
            right
        }
        _ => x0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::calculate;
    use crate::cn::SubArea;
    use crate::project::{NodeType, Pond, Reach, Subcatchment};
    use crate::storage::{PondStorage, StorageVolume, VolumeShape};
    use crate::tc::TcSegment;

    #[test]
    fn chain_of_subcat_reach_pond() {
        let mut p = Project::default();
        let s = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        let r = p.add_node(NodeType::Reach, [0.0, 0.0]);
        let pd = p.add_node(NodeType::Pond, [0.0, 0.0]);
        if let Some(n) = p.node_mut(s) {
            n.outflows[0] = Some(r);
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![SubArea::new(5.0 * 43_560.0, 75.0, "area")],
                tc: vec![
                    TcSegment { kind: TcKind::Sheet { length: 100.0, slope: 0.02, n: 0.24, p2: 3.0, surface: "Grass".into() }, description: "AB".into() },
                    TcSegment { kind: TcKind::Shallow { length: 400.0, slope: 0.01, kv: 16.13, surface: "Unpaved".into() }, description: String::new() },
                ],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(r) {
            n.outflows[0] = Some(pd);
            n.data = NodeData::Reach(Reach { length: 500.0, inlet_invert: 110.0, outlet_invert: 105.0, ..Reach::default() });
        }
        if let Some(n) = p.node_mut(pd) {
            n.data = NodeData::Pond(Pond {
                storage: PondStorage { volumes: vec![StorageVolume::new("Basin", 100.0, VolumeShape::Prismatoid { length: 100.0, width: 50.0, height: 6.0, z: 3.0 })] },
                ..Pond::default()
            });
        }
        let res = calculate(&p);
        let prof = chain(&p, Some(&res), &[s, r, pd]);
        // The subcatchment path ends at the reach's bank top (110 + 2 ft deep default section)
        // after falling 100*0.02 + 400*0.01 = 6 ft.
        let path = prof.shapes.iter().find(|sh| sh.ink == Ink::FlowPath).unwrap();
        assert_eq!(path.points.first().unwrap()[1], 118.0);
        assert_eq!(path.points.last().unwrap(), &[500.0, 112.0]);
        assert!(prof.labels.iter().any(|l| l.text == "AB") && prof.labels.iter().any(|l| l.text == "Shallow Concentrated Flow"));
        // Reach and pond both carry water at their peaks.
        assert_eq!(prof.shapes.iter().filter(|sh| sh.ink == Ink::Water).count(), 2);
        let [x0, y0, x1, y1] = prof.bounds().unwrap();
        assert!(x0 == 0.0 && x1 > 1000.0 && y0 == 100.0 && y1 >= 118.0);
    }

    #[test]
    fn undescribed_nodes_do_not_panic() {
        let mut p = Project::default();
        let ids: Vec<NodeId> = [NodeType::Subcat, NodeType::Reach, NodeType::Pond, NodeType::Link].iter().map(|t| p.add_node(*t, [0.0, 0.0])).collect();
        let prof = chain(&p, None, &ids);
        assert!(prof.bounds().is_some());
    }
}
