//! Plain-text reports: node summaries, listings and tabular hydrographs, laid out in the
//! column style engineers expect from routing-diagram stormwater software.

use crate::calc::{Detail, NodeResult, Results, Severity};
use crate::cn::Hsg;
use crate::hydrograph::{Hydrograph, Peak};
use crate::outlets::{Control, DeviceKind, OpeningShape, Plane, Routing, broad_crested_table, final_routing};
use crate::project::{Node, NodeData, NodeId, NodeType, Project, Tailwater};
use crate::storage::VolumeShape;
use crate::tc::TcKind;
use crate::units::{Units, group};

const OUT_NAMES: [&str; 4] = ["Primary", "Secondary", "Tertiary", "Discarded"];

fn peak_line(label: &str, u: &Units, h: &Hydrograph) -> String {
    let p = h.peak();
    format!(
        "{label:<10}= {:>9} {} @ {:>5.2} hrs, Volume= {:>11} {}",
        group(u.flow(p.value), u.flow_dp()),
        u.flow_label(),
        p.time,
        group(u.volume(h.volume_cf()), u.volume_dp()),
        u.volume_label()
    )
}

fn attenuation(inflow: &Peak, outflow: &Peak) -> String {
    if inflow.value > 0.0 {
        let att = (1.0 - outflow.value / inflow.value) * 100.0;
        format!(", Atten= {:.0}%, Lag= {:.1} min", att.max(0.0), (outflow.time - inflow.time) * 60.0)
    } else {
        String::new()
    }
}

/// Depth label: "Depth=" or "Depth>" when the hydrograph is truncated by the time span.
fn depth_word(h: &Hydrograph) -> &'static str {
    let p = h.peak().value;
    if h.q.last().is_some_and(|q| *q > 0.01 * p) { "Depth>" } else { "Depth=" }
}

/// Messages for one node, formatted as "[nn] Hint: text".
pub fn node_messages(results: &Results, id: NodeId) -> Vec<String> {
    results
        .messages
        .iter()
        .filter(|m| m.node == Some(id))
        .map(|m| format!("[{}] {}: {}", m.code, m.severity.label(), m.text))
        .collect()
}

/// The summary report for a node.
pub fn node_summary(project: &Project, results: &Results, node: &Node) -> Vec<String> {
    let u = &project.units;
    let s = &project.settings;
    let mut out = vec![node.title(), String::new()];
    out.extend(node_messages(results, node.id));
    if out.len() > 2 {
        out.push(String::new());
    }
    let Some(r) = results.node(node.id) else {
        out.push("(not calculated)".into());
        return out;
    };
    if let Some(why) = &r.undefined {
        out.push(format!("RESULTS UNDEFINED: {why}."));
        out.push("The flows, elevations and volumes below are not physically valid.".into());
        out.push(String::new());
    }
    let span = format!("Time Span= {:.2}-{:.2} hrs, dt= {:.2} hrs", s.span_start, s.span_end, s.dt);
    match (&node.data, &r.detail) {
        (NodeData::Subcat(sc), Detail::Subcat(d)) => {
            let area = d.composite.total_area;
            let depth = if area > 0.0 { r.outflow.volume_cf() / area * 12.0 } else { 0.0 };
            out.push(format!(
                "{}, {} {}",
                peak_line("Runoff", u, &r.outflow),
                depth_word(&r.outflow),
                u.fmt_rain(depth)
            ));
            out.push(String::new());
            out.push(format!("{}, {span}", crate::calc::runoff_note(project)));
            out.push(results.event_heading.clone());
            out.push(String::new());
            out.push(format!(
                "{:>12}  {:>5}  Description",
                format!("Area ({})", u.area_label()),
                if d.rational.is_some() { "C" } else { "CN" }
            ));
            for a in &sc.areas {
                let mark = if a.description.is_empty() { "*" } else { "" };
                out.push(format!(
                    "{:>12}  {:>5}{mark} {}",
                    group(u.area(a.area), u.area_dp()),
                    fmt_cn(a.cn),
                    a.description
                ));
            }
            out.push(format!(
                "{:>12}  {:>5}  Weighted Average",
                group(u.area(area), u.area_dp()),
                fmt_cn(d.composite.cn)
            ));
            if s.report_impervious && area > 0.0 {
                let c = &d.composite;
                out.push(format!(
                    "{:>12}         {:.2}% Pervious Area",
                    group(u.area(c.pervious_area), u.area_dp()),
                    c.pervious_area / area * 100.0
                ));
                if c.impervious_area > 0.0 {
                    out.push(format!(
                        "{:>12}         {:.2}% Impervious Area",
                        group(u.area(c.impervious_area), u.area_dp()),
                        c.impervious_area / area * 100.0
                    ));
                }
                if c.unconnected_area > 0.0 {
                    out.push(format!(
                        "{:>12}         {:.2}% Unconnected",
                        group(u.area(c.unconnected_area), u.area_dp()),
                        c.unconnected_area / c.impervious_area.max(1e-9) * 100.0
                    ));
                }
            }
            // Land use and pollutant loading (Settings > Land-Use), when the areas carry land uses.
            let lu = &project.land_use;
            if (lu.report_land_use || lu.report_loading)
                && sc.areas.iter().any(|a| a.land_use.as_deref().is_some_and(|x| !x.is_empty()))
            {
                let id = node.id;
                let rows = crate::landuse::loading_where(project, results, &|x| x == id);
                out.push(String::new());
                for row in &rows {
                    let mut line = String::new();
                    if lu.report_land_use {
                        line.push_str(&format!(
                            "{:>12}  {:<20} {:.2}% Imp.",
                            group(u.area(row.area), u.area_dp()),
                            row.land_use,
                            row.impervious * 100.0
                        ));
                    } else {
                        line.push_str(&format!("{:>12}  {:<20}", group(u.area(row.area), u.area_dp()), row.land_use));
                    }
                    if lu.report_loading {
                        for (name, load) in lu.pollutants.iter().zip(&row.loads) {
                            line.push_str(&format!("  {name}= {} lb", group(*load, 2)));
                        }
                    }
                    out.push(line);
                }
            }
            out.push(String::new());
            if !sc.tc.is_empty() {
                out.push("    Tc  Length    Slope  Velocity  Capacity  Description".into());
                out.push(if u.metric() {
                    format!("  (min)  (meters)    (m/m)   (m/sec)    ({})", u.flow_label())
                } else {
                    format!("  (min)  (feet)   (ft/ft) (ft/sec)    ({})", u.flow_label())
                });
                for (seg, t) in sc.tc.iter().zip(&d.tc_segments) {
                    let cap = t.capacity.map(|c| format!("{:>9.2}", u.flow(c))).unwrap_or_else(|| " ".repeat(9));
                    let slope = if t.slope > 0.0 { format!("{:.4}", t.slope) } else { String::new() };
                    let desc = if seg.description.is_empty() {
                        seg.kind.label().to_string()
                    } else {
                        format!("{}, {}", seg.kind.label(), seg.description)
                    };
                    out.push(format!(
                        "{:>6.1} {:>7} {:>8} {:>9.1} {cap}  {desc}",
                        crate::tc::rounded_minutes(t.minutes, 1),
                        group(u.len(t.length), 0),
                        slope,
                        u.vel(t.velocity)
                    ));
                    out.push(format!("{:>44}{}", "", tc_detail(&seg.kind, u)));
                }
                out.push(format!("{:>6.1} {:>7}  Total", d.tc_minutes, group(u.len(d.flow_length), 0)));
                if let Some(m) = s.min_tc
                    && d.tc_minutes <= m
                {
                    out.push(format!("        (minimum Tc of {m:.1} min applied)"));
                }
            } else {
                out.push(format!("Tc= {:.1} min", d.tc_minutes));
            }
        }
        (NodeData::Reach(reach), Detail::Reach(d)) => {
            reach_header(&mut out, project, r, &results.event);
            out.push(format!("{}, {span}", format_args!("Routing by {} method", s.reach_method.label())));
            out.push(format!(
                "Max. Velocity= {:.2} {}, Min. Travel Time= {:.1} min",
                u.vel(d.max_velocity),
                u.vel_label(),
                d.min_travel_min
            ));
            out.push(format!(
                "Avg. Velocity = {:.2} {}, Avg. Travel Time= {:.1} min",
                u.vel(d.avg_velocity),
                u.vel_label(),
                d.avg_travel_min
            ));
            out.push(String::new());
            out.push(format!(
                "Peak Storage= {} @ {:.2} hrs",
                u.fmt_volume(d.storage.iter().copied().fold(0.0, f64::max)),
                d.peak_depth_time
            ));
            out.push(format!("Average Depth at Peak Storage= {:.2}{}", u.len(d.peak_depth), u.len_suffix()));
            let area_text = |a: f64| {
                format!(
                    "{:.1} {}",
                    a / if u.metric() { crate::units::FT_PER_M.powi(2) } else { 1.0 },
                    if u.metric() { "m²" } else { "sf" }
                )
            };
            if let Some(fd) = reach.flood_depth.filter(|d| *d > 0.0) {
                let q = reach.section.manning_q(fd, reach.n, reach.slope());
                out.push(format!(
                    "Defined Flood Depth= {:.2}{} Flow Area= {}, Capacity= {}",
                    u.len(fd),
                    u.len_suffix(),
                    area_text(reach.section.props(fd).area),
                    u.fmt_flow(q)
                ));
            }
            out.push(format!(
                "Bank-Full Depth= {:.2}{} Flow Area= {:.1} {}, Capacity= {}",
                u.len(reach.section.full_depth()),
                u.len_suffix(),
                reach.section.props(reach.section.full_depth()).area
                    / if u.metric() { crate::units::FT_PER_M.powi(2) } else { 1.0 },
                if u.metric() { "m²" } else { "sf" },
                u.fmt_flow(d.capacity)
            ));
            out.push(String::new());
            out.push(format!("{}, n= {:.3}", section_text(&reach.section, u), reach.n));
            out.push(format!(
                "Length= {}{} Slope= {:.4} '/'",
                group(u.len(reach.length), 1),
                u.len_suffix(),
                reach.slope()
            ));
            out.push(format!(
                "Inlet Invert= {}, Outlet Invert= {}",
                u.fmt_elev(reach.inlet_invert),
                u.fmt_elev(reach.outlet_invert)
            ));
        }
        (NodeData::Pond(pond), Detail::Pond(d)) => {
            reach_header(&mut out, project, r, &results.event);
            for (k, name) in OUT_NAMES.iter().enumerate() {
                if (r.routes[k].has_flow() || (k == 0 && pond.devices.iter().any(|_| true)))
                    && pond
                        .devices
                        .iter()
                        .enumerate()
                        .any(|(i, _)| final_routing(&pond.devices, i).and_then(route_index) == Some(k))
                {
                    out.push(peak_line(name, u, &r.routes[k]));
                }
            }
            out.push(String::new());
            out.push(format!("Routing by {} method, {span}", s.pond_method.label()));
            if d.start_elevation > crate::calc::pond_bottom(pond) + 1e-9 {
                out.push(format!(
                    "Starting Elev= {} Surf.Area= {} Storage= {}",
                    u.fmt_elev(d.start_elevation),
                    pond.storage.surface_area(d.start_elevation).map(|a| u.fmt_area(a)).unwrap_or_default(),
                    u.fmt_volume(pond.storage.storage(d.start_elevation))
                ));
            }
            let area = d.peak_area.map(|a| format!(" Surf.Area= {}", u.fmt_area(a))).unwrap_or_default();
            if r.undefined.is_some() {
                let top = pond.storage.top().unwrap_or(d.start_elevation);
                out.push(format!(
                    "Peak Elev= UNDEFINED (above the defined storage, top {} Storage= {})",
                    u.fmt_elev(top),
                    u.fmt_volume(pond.storage.storage(top))
                ));
            } else {
                out.push(format!(
                    "Peak Elev= {} @ {:.2} hrs{area} Storage= {}",
                    u.fmt_elev(d.peak_elevation),
                    d.peak_time,
                    u.fmt_volume(d.peak_storage)
                ));
            }
            if let Some(f) = pond.flood_elevation {
                let a =
                    pond.storage.surface_area(f).map(|a| format!(" Surf.Area= {}", u.fmt_area(a))).unwrap_or_default();
                out.push(format!(
                    "Flood Elev= {}{a} Storage= {}",
                    u.fmt_elev(f),
                    u.fmt_volume(pond.storage.storage(f))
                ));
            }
            if let Some(pf) = d.detention.plug_flow {
                let pct = if r.inflow.volume_cf() > 0.0 {
                    d.detention.plug_volume / r.inflow.volume_cf() * 100.0
                } else {
                    0.0
                };
                out.push(format!(
                    "Plug-Flow detention time= {:.1} min calculated for {} ({:.0}% of inflow)",
                    pf,
                    u.fmt_volume(d.detention.plug_volume),
                    pct
                ));
            }
            if let (Some(c), Some(a), Some(b)) = (d.detention.center_of_mass, d.detention.com_out, d.detention.com_in) {
                out.push(format!("Center-of-Mass det. time= {:.1} min ( {} - {} )", c, group(a, 1), group(b, 1)));
            }
            out.push(String::new());
            if !pond.storage.is_empty() {
                out.push("Volume    Invert    Avail.Storage  Storage Description".to_string());
                for (i, v) in pond.storage.volumes.iter().enumerate() {
                    let embed = v.embedded_in.map(|j| format!(" (embedded in #{})", j + 1)).unwrap_or_default();
                    let voids = if v.voids_pct < 100.0 { format!(" {:.1}% Voids", v.voids_pct) } else { String::new() };
                    out.push(format!(
                        "  #{:<4} {:>9} {:>13}  {}{voids}{embed}",
                        i + 1,
                        u.fmt_elev(v.bottom()),
                        u.fmt_volume(pond.storage.available(i)),
                        volume_text(&v.shape, &v.description, u)
                    ));
                }
                out.push(String::new());
                for (i, v) in pond.storage.volumes.iter().enumerate() {
                    if let VolumeShape::Custom { elevations, values, kind } = &v.shape {
                        out.push(format!("Volume #{} stage data ({}):", i + 1, kind.label()));
                        let is_area = matches!(
                            kind,
                            crate::storage::CustomKind::AreaPrismatic | crate::storage::CustomKind::AreaConic
                        );
                        out.push(format!(
                            "{:>12} {:>14} {:>14}",
                            format!("Elevation({})", if u.metric() { "m" } else { "ft" }),
                            if is_area {
                                format!("Surf.Area({})", u.area_label())
                            } else {
                                format!("Value({})", u.volume_label())
                            },
                            format!("Cum.Store({})", u.volume_label())
                        ));
                        for (e, a) in elevations.iter().zip(values) {
                            let cum = pond.storage.storage(*e);
                            let shown = if is_area {
                                group(u.area(*a), u.area_dp())
                            } else {
                                group(u.volume(*a), u.volume_dp())
                            };
                            out.push(format!(
                                "{:>12} {:>14} {:>14}",
                                format!("{:.dp$}", u.len(*e), dp = u.elev_dp()),
                                shown,
                                group(u.volume(cum), u.volume_dp())
                            ));
                        }
                        out.push(String::new());
                    }
                }
            }
            if !pond.devices.is_empty() {
                out.push("Device  Routing     Invert   Outlet Devices".into());
                for (i, dev) in pond.devices.iter().enumerate() {
                    let lines = device_text(dev, u);
                    out.push(format!(
                        "  #{:<4} {:<10} {:>8}   {}",
                        i + 1,
                        dev.routing.label(),
                        u.fmt_elev(dev.invert),
                        lines.first().cloned().unwrap_or_default()
                    ));
                    for l in lines.iter().skip(1) {
                        out.push(format!("{:>33}{l}", ""));
                    }
                }
                out.push(String::new());
                for (k, (name, analysis)) in OUT_NAMES.iter().zip(&d.device_analysis).enumerate() {
                    let Some((step, q, flows)) = analysis else { continue };
                    let t = r.inflow.time(*step);
                    let hw = d.elevation.get(*step).copied().unwrap_or(0.0);
                    let tw = if k < 3 {
                        match pond.tailwater[k] {
                            Tailwater::Fixed(e) => format!(" TW={} (Fixed TW Elev= {})", u.fmt_elev(e), u.fmt_elev(e)),
                            Tailwater::Differential(dd) => format!(" TW={} (Differential TW)", u.fmt_elev(hw - dd)),
                            _ => d.tailwater[k]
                                .map(|e| format!(" TW={} (Dynamic Tailwater)", u.fmt_elev(e)))
                                .unwrap_or_else(|| " (Free Discharge)".into()),
                        }
                    } else {
                        String::new()
                    };
                    out.push(format!(
                        "{} OutFlow  Max={} @ {:.2} hrs HW={}{tw}",
                        name,
                        u.fmt_flow(*q),
                        t,
                        u.fmt_elev(hw)
                    ));
                    let mut tree = Vec::new();
                    for (i, dev) in pond.devices.iter().enumerate() {
                        if route_index(dev.routing) == Some(k) {
                            device_tree(pond, flows, i, 1, u, &mut tree);
                        }
                    }
                    out.extend(tree);
                    out.push(String::new());
                }
            }
        }
        (NodeData::Link(_), Detail::Link(desc)) => {
            reach_header(&mut out, project, r, &results.event);
            if r.routes[1].has_flow() {
                out.push(peak_line("Primary", u, &r.routes[0]));
                out.push(peak_line("Secondary", u, &r.routes[1]));
            }
            out.push(String::new());
            out.push(format!("{desc}, {span}"));
        }
        _ => out.push("(no summary)".into()),
    }
    if !node.notes.trim().is_empty() {
        out.push(String::new());
        out.extend(node.notes.lines().map(|l| l.to_string()));
    }
    out
}

fn route_index(r: Routing) -> Option<usize> {
    match r {
        Routing::Primary => Some(0),
        Routing::Secondary => Some(1),
        Routing::Tertiary => Some(2),
        Routing::Discarded => Some(3),
        Routing::Device(_) => None,
    }
}

fn device_tree(
    pond: &crate::project::Pond,
    flows: &crate::outlets::OutletFlows,
    i: usize,
    depth: usize,
    u: &Units,
    out: &mut Vec<String>,
) {
    if depth > pond.devices.len() + 1 {
        return;
    }
    let Some(dev) = pond.devices.get(i) else { return };
    let q = flows.devices.get(i).copied().unwrap_or(0.0);
    let pot = flows.potential.get(i).copied().unwrap_or(0.0);
    let vel = flows.velocities.get(i).copied().unwrap_or(0.0);
    let feeders: Vec<usize> =
        pond.devices.iter().enumerate().filter(|(_, d)| d.routing == Routing::Device(i)).map(|(j, _)| j).collect();
    let name = if dev.description.is_empty() { dev.kind.label().to_string() } else { dev.description.clone() };
    let control = match flows.controls.get(i).copied().flatten() {
        _ if !feeders.is_empty() && q + 1e-9 < pot => {
            format!("(Passes {} of {} potential flow)", u.fmt_flow(q), u.fmt_flow(pot))
        }
        Some(Control::None) | None => format!("( Controls {})", u.fmt_flow(0.0)),
        Some(c) => {
            let v = if vel > 0.0 { format!(" @ {:.1} {}", u.vel(vel), u.vel_label()) } else { String::new() };
            format!("({} {}{v})", c.label(), u.fmt_flow(q))
        }
    };
    out.push(format!("{}{}={} {control}", "  ".repeat(depth), i + 1, name));
    for j in feeders {
        device_tree(pond, flows, j, depth + 1, u, out);
    }
}

fn reach_header(out: &mut Vec<String>, project: &Project, r: &NodeResult, event: &str) {
    let u = &project.units;
    if r.inflow_area > 0.0 {
        let depth = r.inflow.volume_cf() / r.inflow_area * 12.0;
        let imp = if project.settings.report_impervious && r.impervious_area > 0.0 {
            format!(", {:.2}% Impervious", r.impervious_area / r.inflow_area * 100.0)
        } else {
            String::new()
        };
        out.push(format!(
            "Inflow Area = {}{imp}, Inflow {} {} for {} event",
            u.fmt_area(r.inflow_area),
            depth_word(&r.inflow),
            u.fmt_rain(depth),
            event
        ));
    }
    out.push(peak_line("Inflow", u, &r.inflow));
    out.push(format!("{}{}", peak_line("Outflow", u, &r.outflow), attenuation(&r.inflow.peak(), &r.outflow.peak())));
}

fn fmt_cn(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{:.0}", v)
    } else if v <= 1.0 {
        format!("{:.2}", v)
    } else {
        format!("{:.1}", v)
    }
}

fn tc_detail(kind: &TcKind, u: &Units) -> String {
    match kind {
        TcKind::Sheet { n, p2, surface, .. } | TcKind::SheetMcCuenSpiess { n, p2, surface, .. } => {
            format!("{surface} n= {n:.3} P2= {}", u.fmt_rain(*p2))
        }
        TcKind::Shallow { kv, surface, .. } => format!("{surface} Kv= {:.1} fps", kv),
        TcKind::Channel { section, n, .. } => format!("{} n= {n:.3}", section_text(section, u)),
        TcKind::Lag { .. } => "Lag/CN method".into(),
        TcKind::Kirpich { factor, .. } => format!("Kirpich adjustment= {factor:.2}"),
        TcKind::Kerby { retardance, .. } => format!("Kerby N= {retardance:.2}"),
        TcKind::Lake { mean_depth, .. } => format!("Mean depth= {:.2}{}", u.len(*mean_depth), u.len_suffix()),
        TcKind::Direct { .. } => "Direct Entry".into(),
    }
}

/// One-line description of a channel or pipe section.
pub fn section_text(section: &crate::geometry::Section, u: &Units) -> String {
    use crate::geometry::Section as S;
    let l = |v: f64| format!("{:.2}{}", u.len(v), u.len_suffix());
    let p = section.props(section.full_depth());
    let ap = format!(
        "Area={:.1} {} Perim={:.1}{}",
        if u.metric() { p.area / crate::units::FT_PER_M.powi(2) } else { p.area },
        if u.metric() { "m²" } else { "sf" },
        u.len(p.perimeter),
        u.len_suffix()
    );
    match section {
        S::Rectangular { width, depth } => format!("{} x {} deep Rectangular Channel {ap}", l(*width), l(*depth)),
        S::Trapezoidal { bottom, z_left, z_right, depth } => {
            if *bottom <= 0.0 {
                format!("{} deep Vee Channel Z= {:.1}/{:.1} {ap}", l(*depth), z_left, z_right)
            } else {
                format!("{} x {} deep Trapezoidal Channel Z= {:.1}/{:.1} {ap}", l(*bottom), l(*depth), z_left, z_right)
            }
        }
        S::Parabolic { width, depth } => format!("W={} D={} {ap}", l(*width), l(*depth)),
        S::Pipe { shape, width, height, fill } => {
            let fill = if *fill > 0.0 { format!(" Inside fill= {:.1}\"", u.small_len(*fill)) } else { String::new() };
            match shape {
                crate::geometry::PipeShape::Round => format!(
                    "Diam= {:.1}\"{fill} {ap} r= {:.2}{}",
                    u.small_len(*width),
                    u.len(p.hydraulic_radius()),
                    u.len_suffix()
                ),
                other => {
                    format!("{} {:.1}\" x {:.1}\"{fill} {ap}", other.label(), u.small_len(*width), u.small_len(*height))
                }
            }
        }
        S::AreaPerimeter { area, perimeter } => format!(
            "Area={:.1} sf Perim={:.1}{} r={:.2}{}",
            area,
            u.len(*perimeter),
            u.len_suffix(),
            u.len(area / perimeter.max(1e-9)),
            u.len_suffix()
        ),
        S::Custom { points } => format!("Custom cross-section, {} points {ap}", points.len()),
        S::Rating { points, bank_full } => {
            format!("Rating table, {} rows, bank-full depth {}", points.len(), l(*bank_full))
        }
    }
}

fn volume_text(shape: &VolumeShape, desc: &str, u: &Units) -> String {
    let l = |v: f64| format!("{:.2}{}", u.len(v), u.len_suffix());
    let base = match shape {
        VolumeShape::Prismatoid { width, length, height, z } => format!(
            "{}W x {}L x {}H Prismatoid{}",
            l(*width),
            l(*length),
            l(*height),
            if *z > 0.0 { format!(" Z={z:.1}") } else { String::new() }
        ),
        VolumeShape::VerticalCone { diameter, height, z } => format!(
            "{}D x {}H Vertical Cone/Cylinder{}",
            l(*diameter),
            l(*height),
            if *z > 0.0 { format!(" Z={z:.1}") } else { String::new() }
        ),
        VolumeShape::Pipe { shape, width, height, length, .. } => {
            format!("{:.1}\" {} Pipe Storage x {}L", u.small_len(width.max(*height)), shape.label(), l(*length))
        }
        VolumeShape::Arch { width, height, length } => {
            format!("{}W x {}H x {}L Arch Chamber", l(*width), l(*height), l(*length))
        }
        VolumeShape::Custom { .. } => format!("{} Listed below", shape.label()),
    };
    if desc.is_empty() || desc == shape.label() { base } else { format!("{desc}: {base}") }
}

/// Description lines for an outlet device.
pub fn device_text(dev: &crate::outlets::OutletDevice, u: &Units) -> Vec<String> {
    let inch = |v: f64| format!("{:.1}\"", u.small_len(v));
    let ft = |v: f64| format!("{:.1}{}", u.len(v), u.len_suffix());
    let name = |fallback: &str| if dev.description.is_empty() { fallback.to_string() } else { dev.description.clone() };
    let mult = if (dev.multiplier - 1.0).abs() > 1e-9 { format!(" X {} ", fmt_cn(dev.multiplier)) } else { " ".into() };
    match &dev.kind {
        DeviceKind::Culvert { shape, width, height, length, outlet_invert, n, ke, cc, entrance, .. } => {
            let size = if *shape == crate::geometry::PipeShape::Round {
                inch(*width)
            } else {
                format!("{} x {}", inch(*width), inch(*height))
            };
            let slope = if *length > 0.0 { (dev.invert - outlet_invert) / length } else { 0.0 };
            vec![
                format!("{size}{mult}x {} long {} {}, Ke= {:.3}", ft(*length), name("Culvert"), entrance, ke),
                format!(
                    "Inlet / Outlet Invert= {} / {}  S= {:.4} '/'  Cc= {:.3}",
                    u.fmt_elev(dev.invert),
                    u.fmt_elev(*outlet_invert),
                    slope,
                    cc
                ),
                format!("n= {n:.3}"),
            ]
        }
        DeviceKind::Orifice { shape, plane, cd, columns, rows, .. } => {
            let dir = match plane {
                Plane::Horizontal => "Horiz.",
                Plane::Vertical => "Vert.",
                Plane::Angle(_) => "Angled",
            };
            let size = match shape {
                OpeningShape::Circular { diameter } => inch(*diameter),
                OpeningShape::Rectangular { width, height } => format!("{} W x {} H", inch(*width), inch(*height)),
            };
            let grid = if columns * rows > 1 { format!(" X {columns} cols X {rows} rows") } else { String::new() };
            vec![format!("{size}{grid}{mult}{dir} {} C= {cd:.3}", name("Orifice/Grate"))]
        }
        DeviceKind::SharpCrestedWeir { length, end_contractions, .. } => {
            vec![format!(
                "{} long{mult}Sharp-Crested Rectangular Weir {} End Contraction(s) {}",
                ft(*length),
                end_contractions,
                name("")
            )]
        }
        DeviceKind::BroadCrestedWeir { length, breadth, .. } => {
            let t = broad_crested_table(*breadth);
            vec![
                format!(
                    "{} long x {} breadth{mult}{}",
                    ft(*length),
                    ft(*breadth),
                    name("Broad-Crested Rectangular Weir")
                ),
                format!(
                    "Head ({}) {}",
                    if u.metric() { "meters" } else { "feet" },
                    t.iter().map(|p| format!("{:.dp$}", u.len(p.0), dp = u.elev_dp())).collect::<Vec<_>>().join(" ")
                ),
                format!("Coef. (English) {}", t.iter().map(|p| format!("{:.2}", p.1)).collect::<Vec<_>>().join(" ")),
            ]
        }
        DeviceKind::VeeTrapWeir { notch_angle, crest_length, cv, .. } => {
            vec![format!(
                "{:.1} deg x {} long{mult}{} Cv= {cv:.2}",
                notch_angle,
                ft(*crest_length),
                name(dev.kind.label())
            )]
        }
        DeviceKind::CustomWeir { points, coefficient } => {
            vec![format!("{} ({} points) C= {coefficient:.2}", name("Custom Weir/Orifice"), points.len())]
        }
        DeviceKind::Exfiltration { mode, .. } => {
            let what = match mode {
                crate::outlets::ExfilMode::ConstantFlow { flow } => format!("{} Constant", u.fmt_flow(*flow)),
                crate::outlets::ExfilMode::SurfaceVelocity { velocity } => {
                    format!("{:.3} in/hr Exfiltration over Surface area", velocity * 12.0 * 3600.0)
                }
                crate::outlets::ExfilMode::WettedVelocity { velocity } => {
                    format!("{:.3} in/hr Exfiltration over Wetted area", velocity * 12.0 * 3600.0)
                }
                crate::outlets::ExfilMode::Conductivity { ks, groundwater, .. } => format!(
                    "{:.3} in/hr Conductivity to Groundwater Elev= {}",
                    ks * 12.0 * 3600.0,
                    u.fmt_elev(*groundwater)
                ),
            };
            vec![format!("{what}{mult}{}", name(""))]
        }
        DeviceKind::Special { table, .. } => {
            vec![format!("{} ({} points)", name("Special & User-Defined"), table.len())]
        }
        DeviceKind::ConstantFlow { flow, .. } => vec![format!("{}{mult}{}", u.fmt_flow(*flow), name("Constant Flow"))],
        DeviceKind::Pump { on_elevation, off_elevation, max_elevation, priority_group, .. } => {
            let max = max_elevation.map(|m| format!(" Max= {}", u.fmt_elev(m))).unwrap_or_default();
            let group = if *priority_group > 0 { format!(" Priority Group {priority_group}") } else { String::new() };
            vec![format!(
                "{} On= {} Off= {}{max}{group}",
                name("Pump"),
                u.fmt_elev(*on_elevation),
                u.fmt_elev(*off_elevation)
            )]
        }
        DeviceKind::Siphon { diameter, length, n, ke, outlet_elevation, prime_elevation, break_elevation } => vec![
            format!(
                "{}{mult}x {} long {}, Ke= {:.3} n= {:.3}",
                inch(*diameter),
                ft(*length),
                name("Tube/Siphon"),
                ke,
                n
            ),
            format!(
                "Outlet= {}  Prime= {}  Break= {}",
                u.fmt_elev(*outlet_elevation),
                u.fmt_elev(*prime_elevation),
                u.fmt_elev(*break_elevation)
            ),
        ],
        DeviceKind::DamBreach { start_time, start_elevation, formation, final_bottom, final_width, side_z } => {
            let start = match (start_time, start_elevation) {
                (Some(t), _) => format!("starts at {t:.2} hrs"),
                (None, Some(e)) => format!("starts at WSE {}", u.fmt_elev(*e)),
                _ => "never starts".into(),
            };
            vec![
                format!("{} {start}, forms in {:.2} hrs", name("Dam Breach"), formation),
                format!("Final bottom= {}  width= {}  Z= {:.1}", u.fmt_elev(*final_bottom), ft(*final_width), side_z),
            ]
        }
        DeviceKind::DelayedGate {
            trigger_elevation,
            delay,
            opening,
            width,
            height,
            cd,
            step,
            vertical_range,
            rotary,
        } => vec![
            format!(
                "{} W x {} H{mult}{} C= {cd:.3}",
                inch(*width),
                inch(*height),
                name(if *rotary { "Delayed Rotary Weir" } else { "Delayed Gate" })
            ),
            format!(
                "Trigger= {}  Delay= {:.2} hrs  Opening time= {:.2} hrs{}{}",
                u.fmt_elev(*trigger_elevation),
                delay,
                opening,
                if *step > 0.0 { format!("  Opening step= {step:.2} hrs") } else { String::new() },
                if *vertical_range > 0.0 {
                    format!("  Vertical range= {}", ft(*vertical_range))
                } else {
                    String::new()
                }
            ),
        ],
        DeviceKind::Channel { section, n, slope } => {
            vec![format!("{} {}, n= {n:.3} S= {slope:.4} '/'", name("Channel Outlet"), section_text(section, u))]
        }
    }
}

/// Project-wide node listing for the current event.
pub fn node_listing(project: &Project, results: &Results) -> Vec<String> {
    let u = &project.units;
    let s = &project.settings;
    let mut out = vec![
        format!("Time span={:.2}-{:.2} hrs, dt={:.2} hrs, {} points", s.span_start, s.span_end, s.dt, results.n),
        crate::calc::runoff_note(project),
        format!(
            "Reach routing by {} method - Pond routing by {} method",
            s.reach_method.label(),
            s.pond_method.label()
        ),
        String::new(),
    ];
    let mut nodes: Vec<&Node> = project.nodes.iter().filter(|n| n.node_type() != NodeType::Text).collect();
    nodes.sort_by_key(|n| (type_rank(n.node_type()), n.number.parse::<u64>().unwrap_or(u64::MAX), n.number.clone()));
    let (mut tot_area, mut tot_vol) = (0.0, 0.0);
    for n in nodes {
        let Some(r) = results.node(n.id) else { continue };
        let pk = |h: &Hydrograph| format!("{} {}", u.fmt_flow(h.peak().value), u.fmt_volume(h.volume_cf()));
        let (a, b) = match (&n.data, &r.detail) {
            (NodeData::Subcat(_), Detail::Subcat(d)) => {
                let area = d.composite.total_area;
                tot_area += area;
                tot_vol += r.outflow.volume_cf();
                let depth = if area > 0.0 { r.outflow.volume_cf() / area * 12.0 } else { 0.0 };
                let len = if d.flow_length > 0.0 {
                    format!("Flow Length={} ", u.fmt_len(d.flow_length))
                } else {
                    String::new()
                };
                (
                    format!("Runoff Area={} Runoff {}{}", u.fmt_area(area), depth_word(&r.outflow), u.fmt_rain(depth)),
                    format!(
                        "{len}Tc={:.1} min {}={} Runoff={}",
                        d.tc_minutes,
                        if d.rational.is_some() { "C" } else { "CN" },
                        fmt_cn(d.composite.cn),
                        pk(&r.outflow)
                    ),
                )
            }
            (NodeData::Reach(reach), Detail::Reach(d)) => (
                format!(
                    "Avg. Flow Depth={:.2}{} Max Vel={:.1} {} Inflow={}",
                    u.len(d.peak_depth),
                    u.len_suffix(),
                    u.vel(d.max_velocity),
                    u.vel_label(),
                    pk(&r.inflow)
                ),
                format!(
                    "n={:.3} L={}{} S={:.4} '/' Capacity={} Outflow={}",
                    reach.n,
                    group(u.len(reach.length), 1),
                    u.len_suffix(),
                    reach.slope(),
                    u.fmt_flow(d.capacity),
                    pk(&r.outflow)
                ),
            ),
            (NodeData::Pond(_), Detail::Pond(d)) => {
                let mut parts = Vec::new();
                for (k, name) in OUT_NAMES.iter().enumerate() {
                    if r.routes[k].has_flow() && r.routes.iter().filter(|h| h.has_flow()).count() > 1 {
                        parts.push(format!("{}={}", name, pk(&r.routes[k])));
                    }
                }
                parts.push(format!("Outflow={}", pk(&r.outflow)));
                (
                    format!(
                        "Peak Elev={} Storage={} Inflow={}",
                        u.fmt_elev(d.peak_elevation),
                        u.fmt_volume(d.peak_storage),
                        pk(&r.inflow)
                    ),
                    parts.join(" "),
                )
            }
            (NodeData::Link(_), Detail::Link(desc)) => {
                let tail = if r.routes[1].has_flow() {
                    format!("Primary={} Secondary={}", pk(&r.routes[0]), pk(&r.routes[1]))
                } else {
                    format!("Outflow={}", pk(&r.outflow))
                };
                (format!("{desc} Inflow={}", pk(&r.inflow)), tail)
            }
            _ => continue,
        };
        let (a, b) = if r.undefined.is_some() {
            ("RESULTS UNDEFINED (see messages)".to_string(), String::new())
        } else {
            (a, b)
        };
        out.push(format!("{:<46}{:>70}", n.title(), a));
        out.push(format!("{:>116}", b));
        out.push(String::new());
    }
    if tot_area > 0.0 {
        out.push(format!(
            "Total Runoff Area = {} Runoff Volume = {} Average Runoff Depth = {}",
            u.fmt_area(tot_area),
            u.fmt_volume(tot_vol),
            u.fmt_rain(tot_vol / tot_area * 12.0)
        ));
    }
    out
}

fn type_rank(t: NodeType) -> u8 {
    match t {
        NodeType::Subcat => 0,
        NodeType::Reach => 1,
        NodeType::Pond => 2,
        NodeType::Link => 3,
        NodeType::Text => 4,
    }
}

/// Area listing: area by curve number and description, with the subcatchments using each.
pub fn area_listing(project: &Project) -> Vec<String> {
    let u = &project.units;
    let mut rows: Vec<(String, f64, f64, Vec<String>)> = Vec::new();
    for n in &project.nodes {
        if let NodeData::Subcat(sc) = &n.data {
            for a in &sc.areas {
                match rows.iter_mut().find(|r| r.0 == a.description && (r.1 - a.cn).abs() < 1e-9) {
                    Some(r) => {
                        r.2 += a.area;
                        if !r.3.contains(&n.label()) {
                            r.3.push(n.label());
                        }
                    }
                    None => rows.push((a.description.clone(), a.cn, a.area, vec![n.label()])),
                }
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = vec![
        format!("{:>12}  {:>4}  {:<44} Subcatchment", format!("Area ({})", u.area_label()), "CN", "Description"),
        String::new(),
    ];
    let (mut tot, mut wsum) = (0.0, 0.0);
    for (d, cn, a, nodes) in &rows {
        tot += a;
        wsum += a * cn;
        out.push(format!("{:>12}  {:>4}  {:<44} {}", group(u.area(*a), u.area_dp()), fmt_cn(*cn), d, nodes.join(", ")));
    }
    if tot > 0.0 {
        out.push(format!(
            "{:>12}  {:>4}  TOTAL AREA",
            group(u.area(tot), u.area_dp()),
            fmt_cn((wsum / tot * 10.0).round() / 10.0)
        ));
    }
    out
}

/// Soil listing: area by hydrologic soil group.
pub fn soil_listing(project: &Project) -> Vec<String> {
    let u = &project.units;
    let mut groups: Vec<(Option<Hsg>, f64, Vec<String>)> =
        Hsg::ALL.iter().map(|h| (Some(*h), 0.0, Vec::new())).collect();
    groups.push((None, 0.0, Vec::new()));
    for n in &project.nodes {
        if let NodeData::Subcat(sc) = &n.data {
            for a in &sc.areas {
                let idx = a.hsg.map(|h| h as usize).unwrap_or(4);
                if let Some(g) = groups.get_mut(idx) {
                    g.1 += a.area;
                    if !g.2.contains(&n.label()) {
                        g.2.push(n.label());
                    }
                }
            }
        }
    }
    let mut out =
        vec![format!("{:>12}  {:<6}  Subcatchment", format!("Area ({})", u.area_label()), "Soil"), String::new()];
    let mut tot = 0.0;
    for (h, a, nodes) in &groups {
        tot += a;
        let name = h.map(|h| format!("HSG {}", h.letter())).unwrap_or_else(|| "Other".into());
        out.push(format!("{:>12}  {:<6}  {}", group(u.area(*a), u.area_dp()), name, nodes.join(", ")));
    }
    out.push(format!("{:>12}  TOTAL AREA", group(u.area(tot), u.area_dp())));
    out
}

/// Report step: the calculation step, or a multiple of it chosen to fit about `rows` rows.
pub fn table_step(dt: f64, n: usize, requested: f64, rows: usize) -> usize {
    if requested > 0.0 && dt > 0.0 {
        return ((requested / dt).round() as usize).max(1);
    }
    let mut k = 1;
    let nice = [1, 2, 5, 10, 20, 50, 100, 200, 500];
    for m in nice {
        k = m;
        if n / m <= rows.max(1) {
            break;
        }
    }
    k
}

/// Tabular hydrograph for a node: time plus inflow/outflow (and elevation/storage for ponds and
/// reaches, precipitation/excess for subcatchments).
/// Tabular hydrograph columns for a node. With `inflows`, each upstream hydrograph that makes up
/// the node's inflow gets its own column after the Inflow column (e.g. "2S", "1P Secondary").
pub fn hydrograph_table(
    project: &Project,
    results: &Results,
    node: &Node,
    shrink: bool,
    inflows: bool,
) -> (Vec<String>, Vec<Vec<f64>>) {
    let u = &project.units;
    let Some(r) = results.node(node.id) else { return (Vec::new(), Vec::new()) };
    let n = r.outflow.len();
    let step = if shrink { table_step(results.dt, n, project.settings.table_step, 60) } else { 1 };
    let mut heads = vec!["Time (hours)".to_string()];
    let mut cols: Vec<Vec<f64>> = vec![(0..n).step_by(step).map(|i| r.outflow.time(i)).collect()];
    let pick = |v: &[f64], f: &dyn Fn(f64) -> f64| -> Vec<f64> {
        (0..n).step_by(step).map(|i| v.get(i).copied().map(f).unwrap_or(0.0)).collect()
    };
    let flow = |v: f64| u.flow(v);
    match &r.detail {
        Detail::Subcat(d) => {
            heads.push(format!("Precip. ({})", u.rain_heading()));
            cols.push(pick(&d.precip, &|v| u.rain(v)));
            heads.push(format!("Excess ({})", u.rain_heading()));
            cols.push(pick(&d.excess, &|v| u.rain(v)));
            heads.push(format!("Runoff ({})", u.flow_label()));
            cols.push(pick(&r.outflow.q, &flow));
        }
        Detail::Reach(d) => {
            heads.push(format!("Inflow ({})", u.flow_label()));
            cols.push(pick(&r.inflow.q, &flow));
            heads.push(format!("Storage ({})", u.volume_label()));
            cols.push(pick(&d.storage, &|v| u.volume(v)));
            heads.push(format!("Elevation ({})", if u.metric() { "m" } else { "feet" }));
            cols.push(pick(&d.elevation, &|v| u.len(v)));
            heads.push(format!("Outflow ({})", u.flow_label()));
            cols.push(pick(&r.outflow.q, &flow));
        }
        Detail::Pond(d) => {
            heads.push(format!("Inflow ({})", u.flow_label()));
            cols.push(pick(&r.inflow.q, &flow));
            heads.push(format!("Storage ({})", u.volume_label()));
            cols.push(pick(&d.storage, &|v| u.volume(v)));
            heads.push(format!("Elevation ({})", if u.metric() { "m" } else { "feet" }));
            cols.push(pick(&d.elevation, &|v| u.len(v)));
            heads.push(format!("Outflow ({})", u.flow_label()));
            cols.push(pick(&r.outflow.q, &flow));
            for (k, name) in OUT_NAMES.iter().enumerate() {
                if r.routes[k].has_flow() {
                    heads.push(format!("{} ({})", name, u.flow_label()));
                    cols.push(pick(&r.routes[k].q, &flow));
                }
            }
        }
        _ => {
            heads.push(format!("Inflow ({})", u.flow_label()));
            cols.push(pick(&r.inflow.q, &flow));
            heads.push(format!("Primary ({})", u.flow_label()));
            cols.push(pick(&r.routes[0].q, &flow));
            if r.routes[1].has_flow() {
                heads.push(format!("Secondary ({})", u.flow_label()));
                cols.push(pick(&r.routes[1].q, &flow));
            }
        }
    }
    if inflows && !matches!(r.detail, Detail::Subcat(_)) {
        let at = heads.iter().position(|h| h.starts_with("Inflow")).map_or(heads.len(), |i| i + 1);
        let mut extra = Vec::new();
        for (up, k) in project.inflows(node.id) {
            let (Some(un), Some(ur)) = (project.node(up), results.node(up)) else { continue };
            let name =
                if k == 0 { un.label() } else { format!("{} {}", un.label(), OUT_NAMES.get(k).copied().unwrap_or("")) };
            extra.push((format!("{name} ({})", u.flow_label()), pick(&ur.routes[k].q, &flow)));
        }
        for (j, (h, c)) in extra.into_iter().enumerate() {
            heads.insert(at + j, h);
            cols.insert(at + j, c);
        }
    }
    (heads, cols)
}

/// Multi-event summary rows for a node: (event, inflow, outflow, primary, secondary, elevation, storage).
pub fn events_table(project: &Project, all: &[Results], id: NodeId) -> Vec<[String; 7]> {
    let u = &project.units;
    let mut rows = Vec::new();
    for r in all {
        let Some(n) = r.node(id) else { continue };
        let (elev, stor) = match &n.detail {
            Detail::Pond(d) => (
                format!("{:.dp$}", u.len(d.peak_elevation), dp = u.elev_dp()),
                group(u.volume(d.peak_storage), u.volume_dp()),
            ),
            Detail::Reach(d) => (
                format!(
                    "{:.dp$}",
                    u.len(d.elevation.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                    dp = u.elev_dp()
                ),
                group(u.volume(d.storage.iter().copied().fold(0.0, f64::max)), u.volume_dp()),
            ),
            _ => (String::new(), String::new()),
        };
        let f = |h: &Hydrograph| group(u.flow(h.peak().value), u.flow_dp());
        if n.undefined.is_some() {
            let x = || "UNDEFINED".to_string();
            rows.push([
                r.event.clone(),
                if n.undefined.as_deref().is_some_and(|w| !w.starts_with("inflow")) { f(&n.inflow) } else { x() },
                x(),
                x(),
                x(),
                x(),
                x(),
            ]);
            continue;
        }
        rows.push([r.event.clone(), f(&n.inflow), f(&n.outflow), f(&n.routes[0]), f(&n.routes[1]), elev, stor]);
    }
    rows
}

/// Messages as shown in the message window.
pub fn message_lines(project: &Project, results: &Results) -> Vec<(Severity, String)> {
    results
        .messages
        .iter()
        .map(|m| {
            let node = m.node.and_then(|id| project.node(id)).map(|n| format!("{} ", n.label())).unwrap_or_default();
            let text = if !node.is_empty() && m.text.starts_with(node.trim()) {
                m.text.clone()
            } else {
                format!("{node}{}", m.text)
            };
            (m.severity, format!("[{}] {}: {}", m.code, m.severity.label(), text))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::calculate;
    use crate::cn::SubArea;
    use crate::project::Subcatchment;

    #[test]
    fn summary_and_listing_render() {
        let mut p = Project::default();
        let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
        if let Some(n) = p.node_mut(id) {
            n.name = "Test Area".into();
            n.data = NodeData::Subcat(Subcatchment {
                areas: vec![SubArea::new(10.0 * 43_560.0, 70.0, "Woods, Good, HSG C")],
                tc: vec![crate::tc::TcSegment::new(
                    TcKind::Sheet {
                        length: 100.0,
                        slope: 0.02,
                        n: 0.4,
                        p2: 3.0,
                        surface: "Woods: Light underbrush".into(),
                    },
                    "",
                )],
                uh: None,
                large_units: true,
            });
        }
        let r = calculate(&p);
        let node = p.node(id).unwrap();
        let s = node_summary(&p, &r, node);
        assert!(s[0].starts_with("Subcatchment 1S: Test Area"));
        assert!(s.iter().any(|l| l.contains("Weighted Average")));
        assert!(s.iter().any(|l| l.contains("Sheet Flow")));
        let listing = node_listing(&p, &r);
        assert!(listing.iter().any(|l| l.contains("Total Runoff Area")));
        let (heads, cols) = hydrograph_table(&p, &r, node, true, true);
        assert_eq!(heads.len(), cols.len());
        assert!(!area_listing(&p).is_empty());
        assert!(soil_listing(&p).iter().any(|l| l.contains("HSG C")));
    }

    #[test]
    fn inflow_columns_add_up_to_inflow() {
        let mut p = Project::default();
        let link = p.add_node(NodeType::Link, [0.0, 0.0]);
        for (area, cn) in [(10.0, 70.0), (4.0, 98.0)] {
            let id = p.add_node(NodeType::Subcat, [0.0, 0.0]);
            if let Some(n) = p.node_mut(id) {
                n.outflows[0] = Some(link);
                n.data = NodeData::Subcat(Subcatchment {
                    areas: vec![SubArea::new(area * 43_560.0, cn, "area")],
                    tc: vec![crate::tc::TcSegment::new(
                        TcKind::Direct { minutes: 10.0, length: 0.0, velocity: 0.0 },
                        "",
                    )],
                    uh: None,
                    large_units: true,
                });
            }
        }
        let r = calculate(&p);
        let node = p.node(link).unwrap();
        let (plain, _) = hydrograph_table(&p, &r, node, false, false);
        let (heads, cols) = hydrograph_table(&p, &r, node, false, true);
        assert_eq!(heads.len(), plain.len() + 2);
        assert!(
            heads[1].starts_with("Inflow") && heads[2].starts_with("1S") && heads[3].starts_with("2S"),
            "{heads:?}"
        );
        for (i, _) in cols[0].iter().enumerate() {
            assert!((cols[2][i] + cols[3][i] - cols[1][i]).abs() < 1e-9);
        }
    }
}
