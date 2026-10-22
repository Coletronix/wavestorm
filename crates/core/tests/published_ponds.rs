//! Pond routing checked against a pond summary published in a public engineering submittal
//! made with the current reference version: two prismatic stage-area volumes (pond and
//! forebay), a broad-crested overflow weir and exfiltration by hydraulic conductivity to a
//! groundwater elevation. Stor-Ind routing, dt 0.01 h, span 0-120 h, Type II 24-hour storms.
//!
//! The report gives the pond's inflow (area 2.265 ac, depth 0.57" at 4.22" and 1.08" at
//! 5.33", i.e. CN 54) but not the inflow subcatchment's Tc; 6.7 min reproduces both
//! published inflow peaks (1.75 cfs @ 12.01 h and 3.88 cfs @ 12.00 h).

use stormwaves_core::calc::{self, Detail};
use stormwaves_core::cn::SubArea;
use stormwaves_core::outlets::{DeviceKind, ExfilMode, OutletDevice, Routing};
use stormwaves_core::project::*;
use stormwaves_core::rainfall::RainEvent;
use stormwaves_core::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use stormwaves_core::tc::{TcKind, TcSegment};

const AC: f64 = 43_560.0;

fn prismatic(name: &str, stages: &[(f64, f64)]) -> StorageVolume {
    StorageVolume::new(
        name,
        stages[0].0,
        VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: stages.iter().map(|s| s.0).collect(), values: stages.iter().map(|s| s.1).collect() },
    )
}

/// (pond node, project) for one rainfall depth.
fn model(depth: f64) -> (Project, NodeId) {
    let mut p = Project::default();
    p.settings.events = vec![RainEvent::new("Event", "Type II 24-hr", depth)];
    p.settings.current_event = 0;
    p.settings.span_end = 120.0;
    p.settings.dt = 0.01;
    let pond = p.add_node(NodeType::Pond, [0.0, 0.0]);
    let sub = p.add_node(NodeType::Subcat, [0.0, 0.0]);
    if let Some(n) = p.node_mut(sub) {
        n.outflows[0] = Some(pond);
        n.data = NodeData::Subcat(Subcatchment {
            areas: vec![SubArea::new(2.265 * AC, 54.0, "post-development")],
            tc: vec![TcSegment { kind: TcKind::Direct { minutes: 6.7, length: 0.0, velocity: 0.0 }, description: String::new() }],
            uh: None,
            large_units: true,
        });
    }
    let ks = 1.75 / 12.0 / 3600.0; // 1.750 in/hr
    if let Some(n) = p.node_mut(pond) {
        n.data = NodeData::Pond(Pond {
            storage: PondStorage {
                volumes: vec![
                    prismatic("Pond", &[(645.50, 511.0), (646.00, 624.0), (647.00, 867.0), (647.70, 1054.0), (647.75, 1380.0), (648.00, 1515.0)]),
                    prismatic("Forebay", &[(645.50, 88.0), (646.50, 187.0), (647.70, 337.0)]),
                ],
            },
            devices: vec![
                OutletDevice::new("Overflow weir", 647.55, Routing::Primary, DeviceKind::BroadCrestedWeir { length: 4.0, breadth: 3.0, side_z: 0.0, rise: None }),
                OutletDevice::new(
                    "Exfiltration",
                    645.50,
                    Routing::Discarded,
                    DeviceKind::Exfiltration { mode: ExfilMode::Conductivity { ks, groundwater: 640.44, wetted: false }, phase_in: 0.0, max_elevation: None },
                ),
            ],
            flood_elevation: Some(648.0),
            ..Pond::default()
        });
    }
    (p, pond)
}

fn close(label: &str, got: f64, want: f64, abs: f64) {
    assert!((got - want).abs() <= abs, "{label}: got {got:.3}, published {want}");
}

#[test]
fn infiltration_pond_with_overflow_weir() {
    // (depth, inflow, peak elev, storage cf, outflow, discarded, primary, discarded af, primary af)
    for (depth, inflow, elev, storage, outflow, discarded, primary, disc_af, prim_af) in [
        (4.22, 1.75, 647.57, 1981.0, 0.11, 0.07, 0.04, 0.103, 0.005),
        (5.33, 3.88, 647.81, 2296.0, 1.40, 0.09, 1.31, 0.115, 0.089),
    ] {
        let (p, pond) = model(depth);
        let r = calc::calculate(&p);
        let n = r.node(pond).expect("pond result");
        let Detail::Pond(d) = &n.detail else { panic!("pond detail") };
        let tag = |what: &str| format!("{depth}\" {what}");
        eprintln!(
            "{depth}: in {:.2} elev {:.3} stor {:.0} out {:.3} disc {:.3} prim {:.3} disc {:.4} af prim {:.4} af",
            n.inflow.peak().value,
            d.peak_elevation,
            d.peak_storage,
            n.outflow.peak().value,
            n.routes[3].peak().value,
            n.routes[0].peak().value,
            n.routes[3].volume_cf() / AC,
            n.routes[0].volume_cf() / AC
        );
        close(&tag("inflow"), n.inflow.peak().value, inflow, 0.03);
        close(&tag("peak elevation"), d.peak_elevation, elev, 0.02);
        close(&tag("storage"), d.peak_storage, storage, 0.03 * storage);
        close(&tag("outflow"), n.outflow.peak().value, outflow, 0.03 * outflow + 0.01);
        close(&tag("discarded"), n.routes[3].peak().value, discarded, 0.006);
        close(&tag("primary"), n.routes[0].peak().value, primary, 0.04 * primary + 0.01);
        close(&tag("discarded volume"), n.routes[3].volume_cf() / AC, disc_af, 0.002);
        close(&tag("primary volume"), n.routes[0].volume_cf() / AC, prim_af, 0.002);
    }
}
