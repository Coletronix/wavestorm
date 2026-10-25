//! Pond routing checked against four pond summaries published in a public engineering
//! submittal made with the current reference version (Stor-Ind, dt 0.01 h, span 0-120 h, Type
//! II 24-hour 4.22" and 5.33"). The ponds use prismatic stage-area storage (pond, forebay and a
//! gravel cell at 40 % voids below), a broad-crested weir, conductivity exfiltration, 12"
//! culverts fed by 1" orifices, and horizontal grates limited by their frame perimeter.
//!
//! The report gives each pond's inflow area and depth (which fix the CN for both storms) but
//! not the inflow subcatchment's Tc; the Tc given here reproduces both published inflow peaks
//! within 0.2 %.

use wavestorm_core::calc::{self, Detail};
use wavestorm_core::cn::SubArea;
use wavestorm_core::geometry::PipeShape;
use wavestorm_core::outlets::{DeviceKind, ExfilMode, OpeningShape, OutletDevice, Plane, Routing};
use wavestorm_core::project::*;
use wavestorm_core::rainfall::RainEvent;
use wavestorm_core::storage::{CustomKind, PondStorage, StorageVolume, VolumeShape};
use wavestorm_core::tc::{TcKind, TcSegment};

const AC: f64 = 43_560.0;

fn prismatic(name: &str, stages: &[(f64, f64)]) -> StorageVolume {
    StorageVolume::new(
        name,
        stages[0].0,
        VolumeShape::Custom {
            kind: CustomKind::AreaPrismatic,
            elevations: stages.iter().map(|s| s.0).collect(),
            values: stages.iter().map(|s| s.1).collect(),
        },
    )
}

fn gravel(stages: &[(f64, f64)]) -> StorageVolume {
    StorageVolume { voids_pct: 40.0, ..prismatic("Gravel cell", stages) }
}

fn culvert(invert: f64, length: f64, outlet_invert: f64) -> OutletDevice {
    OutletDevice::new(
        "12\" culvert",
        invert,
        Routing::Primary,
        DeviceKind::Culvert {
            shape: PipeShape::Round,
            width: 1.0,
            height: 1.0,
            length,
            outlet_invert,
            n: 0.013,
            ke: 0.9,
            cc: 0.9,
            fill: 0.0,
            entrance: String::new(),
        },
    )
}

/// Vertical orifice (diameter in inches) discharging into the culvert (device 0).
fn orifice(invert: f64, inches: f64) -> OutletDevice {
    let kind = DeviceKind::Orifice {
        shape: OpeningShape::Circular { diameter: inches / 12.0 },
        plane: Plane::Vertical,
        cd: 0.6,
        columns: 1,
        rows: 1,
        row_spacing: 0.0,
        weir_at_low_head: true,
        grate: None,
    };
    OutletDevice::new("Orifice", invert, Routing::Device(0), kind)
}

/// 12 x 12 openings of 1.7" x 1.7" in a 30" x 30" horizontal grate, into the culvert.
fn grate(invert: f64) -> OutletDevice {
    let o = 1.7 / 12.0;
    let kind = DeviceKind::Orifice {
        shape: OpeningShape::Rectangular { width: o, height: o },
        plane: Plane::Horizontal,
        cd: 0.6,
        columns: 12,
        rows: 12,
        row_spacing: 0.0,
        weir_at_low_head: true,
        grate: Some((2.5, 2.5)),
    };
    OutletDevice::new("Grate", invert, Routing::Device(0), kind)
}

/// Pond fed by one subcatchment (area ac, CN, Tc min).
fn model(depth: f64, inflow: (f64, f64, f64), pond: Pond) -> (Project, NodeId) {
    let mut p = Project::default();
    p.settings.events = vec![RainEvent::new("Event", "Type II 24-hr", depth)];
    p.settings.current_event = 0;
    p.settings.span_end = 120.0;
    p.settings.dt = 0.01;
    let pid = p.add_node(NodeType::Pond, [0.0, 0.0]);
    let sub = p.add_node(NodeType::Subcat, [0.0, 0.0]);
    if let Some(n) = p.node_mut(sub) {
        n.outflows[0] = Some(pid);
        n.data = NodeData::Subcat(Subcatchment {
            areas: vec![SubArea::new(inflow.0 * AC, inflow.1, "post-development")],
            tc: vec![TcSegment::new(TcKind::Direct { minutes: inflow.2, length: 0.0, velocity: 0.0 }, "")],
            uh: None,
            large_units: true,
        });
    }
    if let Some(n) = p.node_mut(pid) {
        n.data = NodeData::Pond(pond);
    }
    (p, pid)
}

/// Published results for one storm.
struct Published {
    depth: f64,
    inflow: f64,
    outflow: f64,
    time: f64,
    elevation: f64,
    storage: f64,
}

fn check(label: &str, inflow: (f64, f64, f64), pond: &Pond, published: &[Published]) {
    for c in published {
        let (p, id) = model(c.depth, inflow, pond.clone());
        let r = calc::calculate(&p);
        let n = r.node(id).expect("pond result");
        let Detail::Pond(d) = &n.detail else { panic!("pond detail") };
        let out = n.outflow.peak();
        eprintln!(
            "{label} {}\": in {:.2} ({}) out {:.3} @ {:.2} ({} @ {}) elev {:.3} ({}) stor {:.0} ({})",
            c.depth,
            n.inflow.peak().value,
            c.inflow,
            out.value,
            out.time,
            c.outflow,
            c.time,
            d.peak_elevation,
            c.elevation,
            d.peak_storage,
            c.storage
        );
        let tag = format!("{label} {}\"", c.depth);
        assert!((n.inflow.peak().value - c.inflow).abs() <= 0.01 * c.inflow + 0.02, "{tag} inflow");
        assert!(
            (out.value - c.outflow).abs() <= 0.03 * c.outflow + 0.01,
            "{tag} outflow {:.3} vs {}",
            out.value,
            c.outflow
        );
        // Late peaks of slowly draining ponds are flat for hours, so their time is loosely defined.
        assert!(
            (out.time - c.time).abs() <= 0.05 + 0.06 * (c.time - 12.0).max(0.0),
            "{tag} time {:.2} vs {}",
            out.time,
            c.time
        );
        assert!(
            (d.peak_elevation - c.elevation).abs() <= 0.02,
            "{tag} elevation {:.3} vs {}",
            d.peak_elevation,
            c.elevation
        );
        assert!(
            (d.peak_storage - c.storage).abs() <= 0.02 * c.storage,
            "{tag} storage {:.0} vs {}",
            d.peak_storage,
            c.storage
        );
    }
}

#[test]
fn infiltration_pond_with_overflow_weir() {
    let ks = 1.75 / 12.0 / 3600.0; // 1.750 in/hr
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![
                prismatic(
                    "Pond",
                    &[
                        (645.50, 511.0),
                        (646.00, 624.0),
                        (647.00, 867.0),
                        (647.70, 1054.0),
                        (647.75, 1380.0),
                        (648.00, 1515.0),
                    ],
                ),
                prismatic("Forebay", &[(645.50, 88.0), (646.50, 187.0), (647.70, 337.0)]),
            ],
        },
        devices: vec![
            OutletDevice::new(
                "Overflow weir",
                647.55,
                Routing::Primary,
                DeviceKind::BroadCrestedWeir { length: 4.0, breadth: 3.0, side_z: 0.0, rise: None },
            ),
            OutletDevice::new(
                "Exfiltration",
                645.50,
                Routing::Discarded,
                DeviceKind::Exfiltration {
                    mode: ExfilMode::Conductivity { ks, groundwater: 640.44, wetted: false },
                    phase_in: 0.0,
                    max_elevation: None,
                },
            ),
        ],
        flood_elevation: Some(648.0),
        ..Pond::default()
    };
    let published = [
        Published { depth: 4.22, inflow: 1.75, outflow: 0.11, time: 14.50, elevation: 647.57, storage: 1981.0 },
        Published { depth: 5.33, inflow: 3.88, outflow: 1.40, time: 12.12, elevation: 647.81, storage: 2296.0 },
    ];
    check("1S pond", (2.265, 54.0, 6.7), &pond, &published);
    // The split between seepage and overflow, and their volumes.
    for (depth, discarded, primary, disc_af, prim_af) in
        [(4.22, 0.07, 0.04, 0.103, 0.005), (5.33, 0.09, 1.31, 0.115, 0.089)]
    {
        let (p, id) = model(depth, (2.265, 54.0, 6.7), pond.clone());
        let r = calc::calculate(&p);
        let n = r.node(id).expect("pond result");
        assert!(
            (n.routes[3].peak().value - discarded).abs() <= 0.006,
            "{depth}\" discarded {:.3}",
            n.routes[3].peak().value
        );
        assert!(
            (n.routes[0].peak().value - primary).abs() <= 0.04 * primary + 0.01,
            "{depth}\" primary {:.3}",
            n.routes[0].peak().value
        );
        assert!((n.routes[3].volume_cf() / AC - disc_af).abs() <= 0.002, "{depth}\" discarded volume");
        assert!((n.routes[0].volume_cf() / AC - prim_af).abs() <= 0.002, "{depth}\" primary volume");
    }
}

#[test]
fn gravel_cell_pond_with_culvert_and_orifice() {
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![
                gravel(&[(627.75, 720.0), (631.25, 720.0)]),
                prismatic(
                    "Pond",
                    &[
                        (631.25, 1895.0),
                        (632.0, 2235.0),
                        (633.0, 2708.0),
                        (633.2, 2805.0),
                        (633.25, 3056.0),
                        (634.0, 3536.0),
                        (635.0, 4106.0),
                        (635.75, 4549.0),
                    ],
                ),
                prismatic("Forebay", &[(631.25, 66.0), (632.0, 124.0), (633.0, 223.0), (633.2, 245.0)]),
            ],
        },
        devices: vec![
            culvert(629.0, 44.0, 628.0),
            orifice(631.25, 1.0),
            orifice(632.0, 1.7),
            grate(633.55),
            OutletDevice::new(
                "Spillway",
                635.15,
                Routing::Primary,
                DeviceKind::BroadCrestedWeir { length: 10.0, breadth: 2.0, side_z: 0.0, rise: None },
            ),
        ],
        start_elevation: Some(631.25),
        flood_elevation: Some(635.75),
        ..Pond::default()
    };
    let published = [
        Published { depth: 4.22, inflow: 17.95, outflow: 7.07, time: 12.03, elevation: 635.11, storage: 12769.0 },
        Published { depth: 5.33, inflow: 24.92, outflow: 17.94, time: 12.00, elevation: 635.69, storage: 15305.0 },
    ];
    check("2S pond", (3.905, 82.0, 3.5), &pond, &published);
}

#[test]
fn gravel_cell_pond_with_grate() {
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![
                gravel(&[(632.5, 390.0), (636.0, 390.0)]),
                prismatic("Pond", &[(636.0, 2079.0), (637.0, 2508.0), (637.45, 2709.0), (637.5, 2909.0)]),
                prismatic("Forebay", &[(636.0, 50.0), (637.0, 123.0), (637.45, 164.0)]),
            ],
        },
        devices: vec![culvert(634.5, 32.0, 633.0), orifice(636.0, 1.0), grate(637.0)],
        start_elevation: Some(636.0),
        flood_elevation: Some(637.5),
        ..Pond::default()
    };
    let published = [
        Published { depth: 4.22, inflow: 1.55, outflow: 0.05, time: 17.88, elevation: 637.00, storage: 2934.0 },
        Published { depth: 5.33, inflow: 3.05, outflow: 0.41, time: 12.41, elevation: 637.05, storage: 3062.0 },
    ];
    check("3S pond", (1.423, 57.0, 6.3), &pond, &published);
}

/// A wet basin from a second public permit (dt 0.01 h, span 0-360 h, weighted-Q runoff): forebay
/// and micropool below the basin, a 15" pipe fed by a 2.5" orifice and a 24" x 24" overflow grate
/// with 36 openings. Its inflow subcatchment is fully published (Tc 5.0 min). The report routes
/// with Dyn-Stor-Ind and finer routing; with free discharge on the primary outlet and no flow
/// over the secondary spillway (crest 322.80' is above both peaks, so it is left out) plain
/// Stor-Ind is equivalent.
#[test]
fn wet_basin_with_orifice_and_overflow_grate() {
    let well = [(315.0, 4.0), (316.0, 56.0), (317.0, 165.0), (318.0, 330.0), (319.0, 552.0)];
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![
                prismatic("Forebay", &well),
                prismatic("Micropool", &well),
                prismatic(
                    "Basin",
                    &[
                        (319.0, 5221.0),
                        (320.0, 6088.0),
                        (321.0, 7012.0),
                        (322.0, 7993.0),
                        (323.0, 9030.0),
                        (324.0, 10123.0),
                    ],
                ),
            ],
        },
        devices: vec![
            OutletDevice::new(
                "15\" pipe",
                319.50,
                Routing::Primary,
                DeviceKind::Culvert {
                    shape: PipeShape::Round,
                    width: 1.25,
                    height: 1.25,
                    length: 35.0,
                    outlet_invert: 319.25,
                    n: 0.013,
                    ke: 0.5,
                    cc: 0.9,
                    fill: 0.0,
                    entrance: String::new(),
                },
            ),
            orifice(319.86, 2.5),
            OutletDevice::new(
                "Overflow grate",
                321.60,
                Routing::Device(0),
                DeviceKind::Orifice {
                    shape: OpeningShape::Rectangular { width: 2.2 / 12.0, height: 2.2 / 12.0 },
                    plane: Plane::Horizontal,
                    cd: 0.6,
                    columns: 36,
                    rows: 1,
                    row_spacing: 0.0,
                    weir_at_low_head: true,
                    grate: Some((2.0, 2.0)),
                },
            ),
        ],
        start_elevation: Some(319.86),
        ..Pond::default()
    };
    let areas = [(0.188, 98.0), (0.546, 80.0), (0.723, 98.0), (0.963, 98.0), (0.152, 98.0), (0.298, 39.0)];
    for (depth, inflow, outflow, time, elevation, storage) in
        [(2.10, 6.74, 0.19, 13.98, 321.30, 16004.0), (3.10, 10.49, 1.59, 12.14, 321.74, 19322.0)]
    {
        let mut p = Project::default();
        p.settings.events = vec![RainEvent::new("Event", "Type II 24-hr", depth)];
        p.settings.current_event = 0;
        p.settings.span_end = 360.0;
        p.settings.dt = 0.01;
        p.settings.cn_weighting = wavestorm_core::cn::CnWeighting::WeightedQ;
        let pid = p.add_node(NodeType::Pond, [0.0, 0.0]);
        let sub = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        if let Some(n) = p.node_mut(sub) {
            n.outflows[0] = Some(pid);
            n.data = NodeData::Subcat(Subcatchment {
                areas: areas.iter().map(|&(a, cn)| SubArea::new(a * AC, cn, "area")).collect(),
                tc: vec![TcSegment::new(TcKind::Direct { minutes: 5.0, length: 0.0, velocity: 0.0 }, "")],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(pid) {
            n.data = NodeData::Pond(pond.clone());
        }
        let r = calc::calculate(&p);
        let n = r.node(pid).expect("pond result");
        let Detail::Pond(d) = &n.detail else { panic!("pond detail") };
        let out = n.outflow.peak();
        eprintln!(
            "3P basin {depth}\": in {:.2} ({inflow}) out {:.3} @ {:.2} ({outflow} @ {time}) elev {:.3} ({elevation}) stor {:.0} ({storage})",
            n.inflow.peak().value,
            out.value,
            out.time,
            d.peak_elevation,
            d.peak_storage
        );
        assert!((n.inflow.peak().value - inflow).abs() <= 0.01 * inflow + 0.02, "{depth}\" inflow");
        assert!((out.value - outflow).abs() <= 0.03 * outflow + 0.01, "{depth}\" outflow {:.3}", out.value);
        assert!((out.time - time).abs() <= 0.05 + 0.06 * (time - 12.0).max(0.0), "{depth}\" time {:.2}", out.time);
        assert!((d.peak_elevation - elevation).abs() <= 0.02, "{depth}\" elevation {:.3}", d.peak_elevation);
        assert!((d.peak_storage - storage).abs() <= 0.02 * storage, "{depth}\" storage {:.0}", d.peak_storage);
    }
}

/// A detention basin from a third public report (Type IA, span 1-28 h, dt 0.01 h) with primary
/// and secondary culverts, each fed by an 18" horizontal grate (the primary also by four 2" x 6"
/// orifices). Its inflow subcatchment is fully published; nothing is fitted. At the larger storms
/// the water passes the secondary grate (835.00') and both outlets run.
#[test]
fn basin_with_primary_and_secondary_culverts() {
    let culvert = |d_in: f64, invert: f64, length: f64, routing: Routing| {
        let d = d_in / 12.0;
        let kind = DeviceKind::Culvert {
            shape: PipeShape::Round,
            width: d,
            height: d,
            length,
            outlet_invert: 826.10,
            n: 0.010,
            ke: 0.9,
            cc: 0.9,
            fill: 0.0,
            entrance: String::new(),
        };
        OutletDevice::new("Culvert", invert, routing, kind)
    };
    let opening = |shape: OpeningShape, plane: Plane, columns: u32, invert: f64, into: usize| {
        let kind = DeviceKind::Orifice {
            shape,
            plane,
            cd: 0.6,
            columns,
            rows: 1,
            row_spacing: 0.0,
            weir_at_low_head: true,
            grate: None,
        };
        OutletDevice::new("Orifice/Grate", invert, Routing::Device(into), kind)
    };
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![prismatic(
                "Basin",
                &[(831.0, 483.0), (832.0, 722.0), (833.0, 986.0), (834.0, 1276.0), (835.0, 1590.0), (836.0, 1930.0)],
            )],
        },
        devices: vec![
            culvert(6.0, 831.50, 37.0, Routing::Primary),
            opening(OpeningShape::Circular { diameter: 1.5 }, Plane::Horizontal, 1, 833.00, 0),
            culvert(8.0, 833.00, 32.0, Routing::Secondary),
            opening(OpeningShape::Circular { diameter: 1.5 }, Plane::Horizontal, 1, 835.00, 2),
            opening(OpeningShape::Rectangular { width: 2.0 / 12.0, height: 6.0 / 12.0 }, Plane::Vertical, 4, 831.75, 0),
        ],
        ..Pond::default()
    };
    // (depth, inflow, outflow, time, elevation, storage)
    for (depth, inflow, outflow, time, elevation, storage) in [
        (4.44, 0.85, 0.64, 8.17, 832.49, 986.0),
        (6.5, 1.78, 1.05, 8.28, 833.72, 2240.0),
        (8.52, 2.78, 1.46, 8.34, 835.03, 4071.0),
        (9.36, 3.21, 2.56, 8.13, 835.18, 4311.0),
    ] {
        let mut p = Project::default();
        p.settings.events = vec![RainEvent::new("Event", "Type IA 24-hr", depth)];
        p.settings.current_event = 0;
        p.settings.span_start = 1.0;
        p.settings.span_end = 28.0;
        p.settings.dt = 0.01;
        let pid = p.add_node(NodeType::Pond, [0.0, 0.0]);
        let sub = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        if let Some(n) = p.node_mut(sub) {
            n.outflows[0] = Some(pid);
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![SubArea::new(1.086 * AC, 75.0, "a"), SubArea::new(1.01 * AC, 70.0, "b")],
                tc: vec![TcSegment::new(TcKind::Direct { minutes: 8.9, length: 0.0, velocity: 0.0 }, "")],
                uh: None,
                large_units: true,
            });
        }
        if let Some(n) = p.node_mut(pid) {
            n.data = NodeData::Pond(pond.clone());
        }
        let r = calc::calculate(&p);
        let n = r.node(pid).expect("pond result");
        let Detail::Pond(d) = &n.detail else { panic!("pond detail") };
        let out = n.outflow.peak();
        eprintln!(
            "1P basin {depth}\": in {:.2} ({inflow}) out {:.3} @ {:.2} ({outflow} @ {time}) elev {:.3} ({elevation}) stor {:.0} ({storage}) sec {:.2}",
            n.inflow.peak().value,
            out.value,
            out.time,
            d.peak_elevation,
            d.peak_storage,
            n.routes[1].peak().value
        );
        assert!((n.inflow.peak().value - inflow).abs() <= 0.01 * inflow + 0.02, "{depth}\" inflow");
        assert!((out.value - outflow).abs() <= 0.03 * outflow + 0.01, "{depth}\" outflow {:.3}", out.value);
        assert!((out.time - time).abs() <= 0.05, "{depth}\" time {:.2}", out.time);
        assert!((d.peak_elevation - elevation).abs() <= 0.02, "{depth}\" elevation {:.3}", d.peak_elevation);
        assert!((d.peak_storage - storage).abs() <= 0.02 * storage, "{depth}\" storage {:.0}", d.peak_storage);
    }
}

#[test]
fn gravel_cell_pond_with_two_orifices_and_grate() {
    let pond = Pond {
        storage: PondStorage {
            volumes: vec![
                gravel(&[(637.25, 360.0), (640.75, 360.0)]),
                prismatic("Forebay", &[(640.75, 32.0), (641.0, 45.0), (642.0, 112.0), (642.7, 174.0)]),
                prismatic(
                    "Pond",
                    &[
                        (640.75, 1955.0),
                        (641.0, 2047.0),
                        (642.0, 2432.0),
                        (642.7, 2717.0),
                        (642.75, 2926.0),
                        (643.0, 3059.0),
                        (643.25, 3176.0),
                    ],
                ),
            ],
        },
        devices: vec![culvert(637.5, 54.0, 636.0), orifice(640.75, 1.0), orifice(641.15, 1.0), grate(642.12)],
        start_elevation: Some(640.75),
        flood_elevation: Some(643.25),
        ..Pond::default()
    };
    let published = [
        Published { depth: 4.22, inflow: 5.70, outflow: 3.13, time: 12.04, elevation: 642.33, storage: 4186.0 },
        Published { depth: 5.33, inflow: 8.85, outflow: 6.41, time: 12.02, elevation: 642.61, storage: 4965.0 },
    ];
    check("4S pond", (2.071, 70.0, 4.7), &pond, &published);
}
