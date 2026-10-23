//! A small original demonstration project shown at start-up.

use stormwaves_core::cn::SubArea;
use stormwaves_core::geometry::{PipeShape, Section};
use stormwaves_core::outlets::{DeviceKind, OpeningShape, OutletDevice, Plane, Routing};
use stormwaves_core::project::*;
use stormwaves_core::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use stormwaves_core::tc::{TcKind, TcSegment};

const AC: f64 = 43_560.0;

fn seg(kind: TcKind, desc: &str) -> TcSegment {
    TcSegment { kind, description: desc.into() }
}

fn subcat(p: &mut Project, number: &str, name: &str, pos: [f64; 2], areas: Vec<SubArea>, tc: Vec<TcSegment>, to: NodeId) -> NodeId {
    let id = p.add_node(NodeType::Subcat, pos);
    if let Some(n) = p.node_mut(id) {
        n.number = number.into();
        n.name = name.into();
        n.outflows[0] = Some(to);
        n.data = NodeData::Subcat(Subcatchment { areas, tc, uh: None, large_units: true });
    }
    id
}

/// "Brookside Commons": three subcatchments, a grass swale, a detention basin with a riser and
/// spillway, and an off-site inflow link.
pub fn sample_project() -> Project {
    let mut p = Project::default();
    p.title = "Brookside Commons".into();
    p.prepared_by = "Stormwaves sample".into();
    p.notes = "Demonstration project: post-development runoff routed through a swale and a detention basin.".into();
    p.settings.current_event = 2;

    let basin = p.add_node(NodeType::Pond, [520.0, 220.0]);
    let swale = p.add_node(NodeType::Reach, [330.0, 220.0]);
    let outfall = p.add_node(NodeType::Link, [700.0, 220.0]);

    if let Some(n) = p.node_mut(outfall) {
        n.name = "Brook Outfall".into();
    }
    if let Some(n) = p.node_mut(swale) {
        n.name = "Grass Swale".into();
        n.outflows[0] = Some(basin);
        n.data = NodeData::Reach(Reach {
            section: Section::Trapezoidal { bottom: 3.0, z_left: 3.0, z_right: 3.0, depth: 1.5 },
            n: 0.030,
            length: 420.0,
            inlet_invert: 108.0,
            outlet_invert: 105.5,
            exfiltration: 0.0,
        });
    }
    if let Some(n) = p.node_mut(basin) {
        n.name = "Detention Basin".into();
        n.outflows[0] = Some(outfall);
        n.outflows[1] = Some(outfall);
        n.data = NodeData::Pond(Pond {
            storage: PondStorage {
                volumes: vec![StorageVolume::new(
                    "Basin",
                    100.0,
                    VolumeShape::Custom {
                        kind: CustomKind::AreaConic,
                        elevations: vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0],
                        values: vec![6_000.0, 8_200.0, 10_600.0, 13_200.0, 16_000.0, 19_000.0],
                    },
                )],
            },
            devices: vec![
                OutletDevice::new(
                    "Outlet Pipe",
                    100.0,
                    Routing::Primary,
                    DeviceKind::Culvert {
                        shape: PipeShape::Round,
                        width: 1.5,
                        height: 1.5,
                        length: 60.0,
                        outlet_invert: 99.4,
                        n: 0.013,
                        ke: 0.5,
                        cc: 0.9,
                        fill: 0.0,
                        entrance: "RCP, square edge headwall".into(),
                    },
                ),
                OutletDevice::new(
                    "Low-Flow Orifice",
                    100.0,
                    Routing::Device(0),
                    DeviceKind::Orifice {
                        shape: OpeningShape::Circular { diameter: 0.5 },
                        plane: Plane::Vertical,
                        cd: 0.6,
                        columns: 1,
                        rows: 1,
                        row_spacing: 0.0,
                        weir_at_low_head: true,
                        grate: None,
                    },
                ),
                OutletDevice::new(
                    "Riser Grate",
                    102.8,
                    Routing::Device(0),
                    DeviceKind::Orifice {
                        shape: OpeningShape::Rectangular { width: 3.0, height: 3.0 },
                        plane: Plane::Horizontal,
                        cd: 0.6,
                        columns: 1,
                        rows: 1,
                        row_spacing: 0.0,
                        weir_at_low_head: true,
                        grate: None,
                    },
                ),
                OutletDevice::new("Emergency Spillway", 104.0, Routing::Secondary, DeviceKind::BroadCrestedWeir { length: 12.0, breadth: 8.0, side_z: 3.0, rise: None }),
            ],
            start_elevation: None,
            flood_elevation: Some(105.0),
            tailwater: [Tailwater::Free; 3],
            large_units: false,
            chamber: None,
        });
    }

    subcat(
        &mut p,
        "1",
        "North Lots",
        [120.0, 90.0],
        vec![
            SubArea::new(3.2 * AC, 75.0, "1/4 acre lots, 38% imp, HSG B"),
            SubArea::new(0.9 * AC, 98.0, "Paved roads w/curbs & sewers, HSG B"),
        ],
        vec![
            seg(TcKind::Sheet { length: 100.0, slope: 0.02, n: 0.24, p2: 3.1, surface: "Grass: Dense".into() }, "A-B"),
            seg(TcKind::Shallow { length: 420.0, slope: 0.025, kv: 20.3282, surface: "Paved".into() }, "B-C"),
        ],
        swale,
    );
    subcat(
        &mut p,
        "2",
        "Hillside Woods",
        [120.0, 340.0],
        vec![SubArea::new(5.5 * AC, 55.0, "Woods, Good, HSG B"), SubArea::new(1.1 * AC, 70.0, "Woods, Good, HSG C")],
        vec![
            seg(TcKind::Sheet { length: 100.0, slope: 0.06, n: 0.4, p2: 3.1, surface: "Woods: Light underbrush".into() }, "D-E"),
            seg(TcKind::Shallow { length: 650.0, slope: 0.08, kv: 5.0, surface: "Woodland".into() }, "E-F"),
        ],
        swale,
    );
    subcat(
        &mut p,
        "3",
        "Commons Parking",
        [520.0, 60.0],
        vec![SubArea::new(1.6 * AC, 98.0, "Paved parking, HSG B"), SubArea::new(0.5 * AC, 61.0, "Open Space, Good, HSG B")],
        vec![seg(TcKind::Direct { minutes: 6.0, length: 0.0, velocity: 0.0 }, "")],
        basin,
    );
    let text = p.add_node(NodeType::Text, [330.0, 400.0]);
    if let Some(n) = p.node_mut(text) {
        n.data = NodeData::Text(TextNode { text: "Post-Development".into(), size: 18.0, border: true });
    }
    p
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_calculates_cleanly() {
        let p = super::sample_project();
        let r = stormwaves_core::calc::calculate(&p);
        assert!(!r.has_errors(), "{:?}", r.messages);
        assert!(r.messages.iter().all(|m| m.severity < stormwaves_core::calc::Severity::Warning), "{:?}", r.messages);
    }
}
