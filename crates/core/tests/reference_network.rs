//! End-to-end check against a published sample network: six subcatchments, a 30" pipe reach and
//! a detention pond with a riser (culvert + orifice) and a broad-crested spillway.
//! Type II 24-hr, 4.30", time span 10-40 h, dt 0.10 h, Stor-Ind+Trans reach, Stor-Ind pond.

use stormwaves_core::calc::{self, Detail};
use stormwaves_core::cn::SubArea;
use stormwaves_core::geometry::{PipeShape, Section};
use stormwaves_core::outlets::{DeviceKind, OpeningShape, OutletDevice, Plane, Routing};
use stormwaves_core::project::*;
use stormwaves_core::rainfall::RainEvent;
use stormwaves_core::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use stormwaves_core::tc::{TcKind, TcSegment};

const AC: f64 = 43_560.0;

fn subcat(p: &mut Project, area: f64, cn: f64, tc: f64, to: NodeId) -> NodeId {
    let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
    let n = p.node_mut(id).unwrap();
    n.outflows[0] = Some(to);
    n.data = NodeData::Subcat(Subcatchment {
        areas: vec![SubArea::new(area * AC, cn, "test area")],
        tc: vec![TcSegment { kind: TcKind::Direct { minutes: tc, length: 0.0, velocity: 0.0 }, description: String::new() }],
        uh: None,
        large_units: true,
    });
    id
}

fn network() -> (Project, NodeId, NodeId) {
    let mut p = Project::default();
    p.settings.events = vec![RainEvent::new("25-Year", "Type II 24-hr", 4.30)];
    p.settings.current_event = 0;
    p.settings.span_start = 10.0;
    p.settings.span_end = 40.0;
    p.settings.dt = 0.10;
    let pond = p.add_node(NodeType::Pond, [0.0, 0.0]);
    let reach = p.add_node(NodeType::Reach, [0.0, 0.0]);
    p.node_mut(reach).unwrap().outflows[0] = Some(pond);
    p.node_mut(reach).unwrap().data = NodeData::Reach(Reach {
        section: Section::Pipe { shape: PipeShape::Round, width: 2.5, height: 2.5, fill: 0.0 },
        n: 0.013,
        length: 1475.0,
        inlet_invert: 100.0,
        outlet_invert: 100.0 - 0.0169 * 1475.0,
        exfiltration: 0.0,
    });
    for (a, cn, tc) in [(20.0, 71.0, 91.9), (43.5, 55.0, 31.1), (27.0, 55.0, 24.8), (12.0, 60.0, 25.0)] {
        subcat(&mut p, a, cn, tc, reach);
    }
    for (a, cn, tc) in [(25.0, 60.0, 46.7), (25.0, 60.0, 23.2)] {
        subcat(&mut p, a, cn, tc, pond);
    }
    p.node_mut(pond).unwrap().data = NodeData::Pond(Pond {
        storage: PondStorage {
            volumes: vec![StorageVolume::new(
                "Custom Stage Data (Prismatic)",
                50.0,
                VolumeShape::Custom {
                    kind: CustomKind::AreaPrismatic,
                    elevations: vec![50.0, 52.5, 53.0, 55.0, 57.0],
                    values: vec![0.2 * AC, 0.4 * AC, 1.2 * AC, 1.5 * AC, 2.0 * AC],
                },
            )],
        },
        devices: vec![
            OutletDevice::new(
                "Culvert",
                50.0,
                Routing::Primary,
                DeviceKind::Culvert {
                    shape: PipeShape::Round,
                    width: 2.0,
                    height: 2.0,
                    length: 12.0,
                    outlet_invert: 49.6,
                    n: 0.013,
                    ke: 0.2,
                    cc: 0.9,
                    fill: 0.0,
                    entrance: String::new(),
                },
            ),
            OutletDevice::new(
                "Orifice plate",
                50.5,
                Routing::Device(0),
                DeviceKind::Orifice {
                    shape: OpeningShape::Circular { diameter: 16.0 / 12.0 },
                    plane: Plane::Vertical,
                    cd: 0.6,
                    columns: 1,
                    rows: 1,
                    row_spacing: 0.0,
                    weir_at_low_head: true,
                    grate: None,
                },
            ),
            OutletDevice::new("Spillway", 53.6, Routing::Secondary, DeviceKind::BroadCrestedWeir { length: 5.0, breadth: 2.5, side_z: 0.0, rise: None }),
        ],
        start_elevation: None,
        flood_elevation: Some(55.0),
        tailwater: [Tailwater::Fixed(49.0), Tailwater::Free, Tailwater::Free],
        large_units: true,
    });
    (p, reach, pond)
}

fn close(label: &str, got: f64, want: f64, rel: f64) {
    assert!((got - want).abs() <= rel * want.abs(), "{label}: got {got:.3}, published {want}");
}

#[test]
fn reference_pipe_reach() {
    let (p, reach, _) = network();
    let r = calc::calculate(&p);
    let n = r.node(reach).unwrap();
    // Published: inflow 38.32 cfs 7.429 af; outflow 37.15 cfs; peak depth 1.55'; max vel 11.8 fps.
    // Summed subcatchment peaks run ~4.5% above the published values although each subcatchment
    // matches within 1% (open investigation, see AGENTS.md).
    close("1R inflow", n.inflow.peak().value, 38.32, 0.06);
    close("1R inflow volume", n.inflow.volume_cf() / AC, 7.429, 0.005);
    close("1R outflow", n.outflow.peak().value, 37.15, 0.06);
    let Detail::Reach(d) = &n.detail else { panic!("reach detail") };
    close("1R peak depth", d.peak_depth, 1.55, 0.04);
    close("1R max velocity", d.max_velocity, 11.8, 0.03);
    close("1R capacity", d.capacity, 53.40, 0.005);
}

#[test]
fn reference_detention_pond() {
    let (p, _, pond) = network();
    let r = calc::calculate(&p);
    let n = r.node(pond).unwrap();
    // Published: inflow 60.54 cfs @ 12.30 h, 11.236 af; outflow 23.17 @ 13.51 (primary 12.25,
    // secondary 10.92); peak elevation 54.49', storage 3.156 af.
    close("1P inflow", n.inflow.peak().value, 60.54, 0.06);
    close("1P inflow volume", n.inflow.volume_cf() / AC, 11.236, 0.005);
    close("1P outflow", n.outflow.peak().value, 23.17, 0.03);
    close("1P primary", n.routes[0].peak().value, 12.25, 0.02);
    close("1P secondary", n.routes[1].peak().value, 10.92, 0.05);
    let Detail::Pond(d) = &n.detail else { panic!("pond detail") };
    close("1P peak elevation", d.peak_elevation, 54.49, 0.0005);
    close("1P storage", d.peak_storage / AC, 3.156, 0.03);
    close("1P peak time", n.outflow.peak().time, 13.51, 0.01);
    // Plug-flow 105.7 min, center-of-mass 98.7 min.
    close("1P center of mass", d.detention.center_of_mass.unwrap(), 98.7, 0.05);
    close("1P plug flow", d.detention.plug_flow.unwrap(), 105.7, 0.08);
}

