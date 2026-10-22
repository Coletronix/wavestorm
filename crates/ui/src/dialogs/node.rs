//! Node edit dialogs. Each edits a draft copy of the node; OK/Apply commit one undo step.

use super::lookup::{self, CnLookup};
use super::outlet::{self, DeviceEditor};
use super::{Buttons, conv_field, ok_cancel_apply, opt_field};
use crate::doc::Doc;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{RichText, Ui, vec2};
use stormwaves_core::cn::SubArea;
use stormwaves_core::geometry::{PipeShape, RatingPoint, Section, XsPoint};
use stormwaves_core::link::{LinkElevation, LinkSource, ManualHydrograph};
use stormwaves_core::project::{Node, NodeData, NodeId, NodeType, Tailwater};
use stormwaves_core::storage::{CustomKind, StorageVolume, VolumeShape};
use stormwaves_core::tc::{self, TcKind, TcSegment};
use stormwaves_core::units::Units;

pub struct EditState {
    pub node: NodeId,
    pub draft: Node,
    pub tab: usize,
    pub sel_row: Option<usize>,
    pub cn_lookup: CnLookup,
    pub tc_edit: Option<(Option<usize>, TcSegment)>,
    pub volume_edit: Option<(Option<usize>, StorageVolume)>,
    pub device_edit: Option<DeviceEditor>,
    pub manual_text: String,
    /// Text of the Area grid while editing (area, CN, description).
    pub area_rows: Vec<Vec<String>>,
    pub add_kind: usize,
}

impl EditState {
    pub fn new(node: &Node) -> Self {
        let manual_text = match &node.data {
            NodeData::Link(l) => match &l.source {
                LinkSource::Manual(m) => m.flows.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join("\n"),
                _ => String::new(),
            },
            _ => String::new(),
        };
        EditState {
            node: node.id,
            draft: node.clone(),
            tab: if node.node_type() == NodeType::Subcat { 1 } else { 1.min(tabs(node.node_type()).len() - 1) },
            sel_row: None,
            cn_lookup: CnLookup::default(),
            tc_edit: None,
            volume_edit: None,
            device_edit: None,
            manual_text,
            area_rows: Vec::new(),
            add_kind: 0,
        }
    }
}

fn tabs(t: NodeType) -> &'static [&'static str] {
    match t {
        NodeType::Subcat => &["General", "Area", "Tc", "Notes"],
        NodeType::Reach => &["General", "Reach", "Notes"],
        NodeType::Pond => &["General", "Storage", "Outlets", "Tailwater", "Advanced", "Notes"],
        NodeType::Link => &["General", "Manual", "Advanced", "Elevation", "Notes"],
        NodeType::Text => &["Text"],
    }
}

/// Shows the dialog; returns false when it closes.
pub fn show(ctx: &egui::Context, doc: &mut Doc, st: &mut EditState, actions: &mut Vec<WindowAction>) -> bool {
    if doc.project.node(st.node).is_none() {
        return false;
    }
    let units = doc.project.units;
    let mut keep = true;
    let mut apply = false;
    let title = format!("Edit {} {} - {}", st.draft.node_type().label(), st.draft.label(), doc.project.title);
    crate::frame::Win::new(title).id(egui::Id::new(("edit", st.node))).collapsible(false).resizable(true).default_size(vec2(560.0, 420.0)).show(ctx, |ui| {
        let names = tabs(st.draft.node_type());
        st.tab = st.tab.min(names.len() - 1);
        widgets::tabs(ui, names, &mut st.tab);
        widgets::tab_body(ui, |ui| {
            ui.set_min_size(vec2(520.0, 300.0));
            let tab = names.get(st.tab).copied().unwrap_or("General");
            match tab {
                "General" => general(ui, st),
                "Notes" => {
                    ui.label("Notes (included in reports):");
                    ui.add_sized(vec2(ui.available_width(), 260.0), egui::TextEdit::multiline(&mut st.draft.notes));
                }
                _ => match st.draft.node_type() {
                    NodeType::Subcat => subcat_tab(ui, st, &units, tab, doc),
                    NodeType::Reach => reach_tab(ui, st, &units),
                    NodeType::Pond => pond_tab(ui, st, &units, tab, doc),
                    NodeType::Link => link_tab(ui, st, &units, tab, actions),
                    NodeType::Text => text_tab(ui, st),
                },
            }
        });
        let help = match st.draft.node_type() {
            NodeType::Subcat => if st.tab == 2 { "tc" } else { "cn" },
            NodeType::Reach => "reach",
            NodeType::Pond => "pond",
            NodeType::Link => "link",
            NodeType::Text => "start",
        };
        match ok_cancel_apply(ui, true, help) {
            Buttons::Ok => {
                apply = true;
                keep = false;
            }
            Buttons::Apply => apply = true,
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    // Sub-dialogs.
    if let Some((cn, desc)) = lookup::cn_lookup(ctx, &mut st.cn_lookup) {
        if let NodeData::Subcat(sc) = &mut st.draft.data {
            match st.sel_row {
                Some(i) if i < sc.areas.len() => {
                    sc.areas[i].cn = cn;
                    sc.areas[i].hsg = stormwaves_core::cn::soil_from_description(&desc);
                    sc.areas[i].description = desc;
                }
                _ => sc.areas.push(SubArea::new(0.0, cn, &desc)),
            }
        }
    }
    tc_editor(ctx, st, &units, doc);
    volume_editor(ctx, st, &units);
    if let Some(ed) = &mut st.device_edit {
        if let NodeData::Pond(pond) = &mut st.draft.data {
            match outlet::show(ctx, ed, &units, pond, &st.draft.number) {
                Some(true) => {
                    let dev = ed.device.clone();
                    match ed.index {
                        Some(i) if i < pond.devices.len() => pond.devices[i] = dev,
                        _ => pond.devices.push(dev),
                    }
                    st.device_edit = None;
                }
                Some(false) => st.device_edit = None,
                None => {}
            }
        }
    }
    if apply {
        if let NodeData::Link(l) = &mut st.draft.data {
            if let LinkSource::Manual(m) = &mut l.source {
                m.flows = st.manual_text.split(|c: char| c.is_whitespace() || c == ',').filter_map(|s| s.trim().parse::<f64>().ok()).map(|v| units.flow_in(v)).collect();
            }
        }
        let draft = st.draft.clone();
        doc.edit("Node Edit", |p| {
            // Keep routing edited on the diagram since the dialog opened.
            if let Some(n) = p.node_mut(draft.id) {
                let outflows = n.outflows;
                let pos = n.pos;
                *n = draft.clone();
                n.outflows = outflows;
                n.pos = pos;
            }
        });
    }
    keep
}

fn general(ui: &mut Ui, st: &mut EditState) {
    ui.horizontal(|ui| {
        widgets::with_tip(ui, "Identifies the node on the diagram and in reports, with its type letter (S, R, P, L). Numbers must be unique for each type.", |ui| {
            widgets::text_field(ui, "Node Number:", &mut st.draft.number, 90.0);
        });
        ui.add_space(8.0);
        widgets::with_tip(ui, "A short description shown under the node and in report headings.", |ui| {
            widgets::text_field(ui, "Name:", &mut st.draft.name, 320.0);
        });
    });
    ui.add_space(6.0);
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(format!("Node label: {}", st.draft.label())).color(t.text_dim));
    match &mut st.draft.data {
        NodeData::Subcat(sc) => {
            ui.add_space(8.0);
            widgets::check(ui, &mut sc.large_units, "Large units (acres) for this subcatchment");
            ui.add_space(6.0);
            let mut uh = sc.uh.clone().unwrap_or_default();
            let mut opts = vec![(String::new(), "(project default)".to_string())];
            for u in stormwaves_core::uh::builtin_uhs() {
                opts.push((u.name.clone(), u.name));
            }
            let changed = widgets::with_tip(ui, "Unit hydrograph shape for this subcatchment only. The project default (Settings > Calculation > Unit Hydro) is the standard NRCS unit hydrograph unless changed there.", |ui| {
                widgets::combo_field(ui, "uh", "Unit Hydrograph:", &mut uh, &opts, 220.0)
            });
            if changed {
                sc.uh = if uh.is_empty() { None } else { Some(uh) };
            }
            if st.draft.outline.is_some() {
                ui.add_space(6.0);
                ui.label(RichText::new("This subcatchment has a traced outline on the diagram (right-click it to update the area).").color(t.hint));
            }
        }
        NodeData::Pond(p) => {
            ui.add_space(8.0);
            widgets::check(ui, &mut p.large_units, "Large units (acres, acre-feet) for this pond");
        }
        _ => {}
    }
    ui.add_space(8.0);
    widgets::check(ui, &mut st.draft.locked, "Locked (protect from merge/import)");
}

// ------------------------------------------------------------------ subcatchment

fn subcat_tab(ui: &mut Ui, st: &mut EditState, u: &Units, tab: &str, doc: &Doc) {
    let NodeData::Subcat(sc) = &mut st.draft.data else { return };
    let large = sc.large_units;
    let area_unit = if large { u.area_label() } else if u.metric() { "m²" } else { "sq-ft" };
    let to_area = |v: f64| if large { u.area(v) } else if u.metric() { v / stormwaves_core::units::FT_PER_M.powi(2) } else { v };
    let from_area = |v: f64| if large { u.area_in(v) } else if u.metric() { v * stormwaves_core::units::FT_PER_M.powi(2) } else { v };
    let t = Tokens::get(ui.ctx());
    if tab == "Area" {
        // Refresh the text buffer when it no longer matches the data (first show, lookups).
        let parsed: Vec<(f64, f64, String)> = st.area_rows.iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).map(|r| (widgets::cell_num(&r[0]).map(from_area).unwrap_or(0.0), widgets::cell_num(&r[1]).unwrap_or(0.0), r[2].clone())).collect();
        let same = parsed.len() == sc.areas.len() && parsed.iter().zip(&sc.areas).all(|(p, a)| (p.0 - a.area).abs() < 1e-6 * a.area.abs().max(1.0) && (p.1 - a.cn).abs() < 1e-9 && p.2 == a.description);
        if !same {
            st.area_rows = sc.areas.iter().map(|a| vec![widgets::cell_fmt(to_area(a.area), if large { 3 } else { 0 }), format!("{}", a.cn), a.description.clone(), a.land_use.clone().unwrap_or_default()]).collect();
        }
        let cols = [
            widgets::Column { title: &format!("Area ({area_unit})"), width: 100.0, numeric: true },
            widgets::Column { title: "CN", width: 56.0, numeric: true },
            widgets::Column { title: "Description", width: 260.0, numeric: false },
            widgets::Column { title: "Land Use", width: 100.0, numeric: false },
        ];
        let ev = widgets::grid(ui, "areas", &cols, &mut st.area_rows, &mut st.sel_row, 210.0, true);
        if ev.changed {
            sc.areas = st
                .area_rows
                .iter()
                .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
                .map(|r| {
                    let mut a = SubArea::new(widgets::cell_num(&r[0]).map(from_area).unwrap_or(0.0).max(0.0), widgets::cell_num(&r[1]).unwrap_or(0.0).clamp(0.0, 100.0), r[2].trim());
                    a.land_use = r.get(3).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                    a
                })
                .collect();
        }
        if ev.double_clicked.is_some() {
            st.cn_lookup.open = true;
        }
        ui.horizontal(|ui| {
            if widgets::button(ui, "Lookup CN...", 100.0).on_hover_text("Pick a ground cover for the selected line (double-click a line number also opens it)").clicked() {
                st.cn_lookup.open = true;
            }
            ui.label(RichText::new("Right-click a line number to insert or delete a line.").color(t.text_dim));
        });
        let comp = stormwaves_core::cn::composite(&sc.areas, doc.project.settings.round_cn, 0);
        ui.add_space(6.0);
        ui.label(format!(
            "{} {area_unit}   Weighted Average CN = {:.1} ({})   Impervious = {:.1}%",
            stormwaves_core::units::group(to_area(comp.total_area), if large { 3 } else { 0 }),
            comp.cn_raw,
            if doc.project.settings.round_cn { format!("{:.0} used", comp.cn) } else { "full precision".into() },
            if comp.total_area > 0.0 { comp.impervious_area / comp.total_area * 100.0 } else { 0.0 }
        ));
    } else {
        // Tc tab.
        let cn = stormwaves_core::cn::composite(&sc.areas, true, 0).cn;
        let rows: Vec<Vec<String>> = sc
            .tc
            .iter()
            .map(|s| {
                let r = s.kind.evaluate(cn);
                vec![
                    format!("{:.1}", tc::rounded_minutes(r.minutes, 1)),
                    stormwaves_core::units::group(u.len(r.length), 0),
                    if r.slope > 0.0 { format!("{:.4}", r.slope) } else { String::new() },
                    format!("{:.2}", u.vel(r.velocity)),
                    r.capacity.map(|c| format!("{:.2}", u.flow(c))).unwrap_or_default(),
                    if s.description.is_empty() { s.kind.label().to_string() } else { format!("{}, {}", s.kind.label(), s.description) },
                ]
            })
            .collect();
        let cols = [("Tc (min)", 64.0, true), ("Length", 64.0, true), ("Slope", 60.0, true), ("Velocity", 62.0, true), ("Capacity", 62.0, true), ("Description", 230.0, false)];
        let ev = widgets::list_table(ui, "tc", &cols, &rows, &mut st.sel_row, 180.0);
        let mut edit = ev.double_clicked;
        let sel = st.sel_row.filter(|i| *i < sc.tc.len());
        ui.horizontal(|ui| {
            if widgets::button_enabled(ui, "Edit...", 70.0, sel.is_some()).clicked() {
                edit = sel;
            }
            if widgets::button_enabled(ui, "Delete", 70.0, sel.is_some()).clicked() {
                if let Some(i) = sel {
                    sc.tc.remove(i);
                    st.sel_row = None;
                }
            }
            if widgets::button_enabled(ui, "Move Up", 70.0, sel.is_some_and(|i| i > 0)).clicked() {
                if let Some(i) = sel {
                    sc.tc.swap(i, i - 1);
                    st.sel_row = Some(i - 1);
                }
            }
        });
        if let Some(i) = edit {
            if let Some(s) = sc.tc.get(i) {
                st.tc_edit = Some((Some(i), s.clone()));
            }
        }
        let total = tc::total_minutes(&sc.tc, cn, 1);
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Total Tc = {total:.1} min{}", doc.project.settings.min_tc.map(|m| format!(" (minimum {m:.1} min applies)")).unwrap_or_default())).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Add segment:");
            let p2 = 3.0;
            let kinds = [
                ("Sheet Flow", TcKind::Sheet { length: 100.0, slope: 0.02, n: 0.15, p2, surface: "Grass: Short".into() }),
                ("Shallow Concentrated", TcKind::Shallow { length: 300.0, slope: 0.02, kv: 16.1345, surface: "Unpaved".into() }),
                ("Channel", TcKind::Channel { length: 500.0, slope: 0.01, n: 0.030, section: Section::Trapezoidal { bottom: 2.0, z_left: 2.0, z_right: 2.0, depth: 1.0 } }),
                ("Pipe", TcKind::Channel { length: 200.0, slope: 0.01, n: 0.013, section: Section::circular(1.5) }),
                ("Lag/CN", TcKind::Lag { length: 1000.0, slope: 0.02 }),
                ("Kirpich", TcKind::Kirpich { length: 1000.0, slope: 0.02, factor: 1.0 }),
                ("Kerby", TcKind::Kerby { length: 300.0, slope: 0.02, retardance: 0.4 }),
                ("Lake", TcKind::Lake { length: 500.0, mean_depth: 5.0 }),
                ("Direct Entry", TcKind::Direct { minutes: 6.0, length: 0.0, velocity: 0.0 }),
            ];
            for (label, kind) in kinds {
                if widgets::button(ui, label, 0.0).clicked() {
                    st.tc_edit = Some((None, TcSegment { kind, description: String::new() }));
                }
            }
        });
        let _ = t;
    }
}

fn tc_editor(ctx: &egui::Context, st: &mut EditState, u: &Units, doc: &Doc) {
    let Some((index, seg)) = &mut st.tc_edit else { return };
    let mut result = Buttons::None;
    let p2_default = doc.project.settings.event().map(|_| 3.0).unwrap_or(3.0);
    let _ = p2_default;
    crate::frame::Win::new(format!("Tc Segment: {}", seg.kind.label())).collapsible(false).resizable(false).show(ctx, |ui| {
        let lf = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| {
            widgets::with_tip(ui, "Horizontal length of this part of the flow path, measured along the path the water takes.", |ui| {
                conv_field(ui, id, label, u.len_label(), v, |x| u.len(x), |x| u.len_in(x), 1, 130.0);
            });
        };
        let sf = |ui: &mut Ui, id: &str, v: &mut f64| {
            widgets::with_tip(ui, "Average land or channel slope along this segment (rise over run, e.g. 0.02 for 2 %).", |ui| {
                conv_field(ui, id, "Slope:", "ft/ft", v, |x| x, |x| x, 4, 130.0);
            });
        };
        ui.horizontal(|ui| {
            widgets::text_field(ui, "Segment ID / Description:", &mut seg.description, 260.0);
        });
        ui.add_space(4.0);
        match &mut seg.kind {
            TcKind::Direct { minutes, length, velocity } => {
                ui.horizontal(|ui| {
                    conv_field(ui, "min", "Tc:", "min", minutes, |x| x, |x| x, 1, 110.0);
                    lf(ui, "len", "Flow Length:", length);
                    conv_field(ui, "vel", "Velocity:", u.vel_label(), velocity, |x| u.vel(x), |x| u.len_in(x), 2, 110.0);
                });
            }
            TcKind::Sheet { length, slope, n, p2, surface } | TcKind::SheetMcCuenSpiess { length, slope, n, p2, surface } => {
                let mut opts: Vec<(String, String)> = tc::SHEET_SURFACES.iter().map(|(s, v)| (s.to_string(), format!("{s}  n={v:.3}"))).collect();
                opts.insert(0, (String::new(), "(custom)".into()));
                let mut sel = surface.clone();
                if widgets::combo_field(ui, "surf", "Surface Description:", &mut sel, &opts, 300.0) {
                    if let Some((_, v)) = tc::SHEET_SURFACES.iter().find(|(s, _)| *s == sel) {
                        *n = *v;
                    }
                    *surface = sel;
                }
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    sf(ui, "slope", slope);
                });
                ui.horizontal(|ui| {
                    widgets::with_tip(ui, "Sheet-flow roughness (TR-55 Table 3-1), much higher than channel n: e.g. 0.011 smooth pavement, 0.15 short grass, 0.40 light woods.", |ui| {
                        conv_field(ui, "n", "Manning's n:", "", n, |x| x, |x| x, 3, 130.0);
                    });
                    widgets::with_tip(ui, "The 2-year, 24-hour rainfall depth at the site (not the design storm). TR-55 uses it in the sheet-flow equation.", |ui| {
                        conv_field(ui, "p2", "2-Year 24-hr Rainfall P2:", u.rain_heading(), p2, |x| u.rain(x), |x| u.rain_in(x), 2, 170.0);
                    });
                });
                if *length > 300.0 {
                    ui.colored_label(Tokens::get(ui.ctx()).warning, "Sheet flow should not exceed 300 feet (TR-55).");
                }
            }
            TcKind::Shallow { length, slope, kv, surface } => {
                let mut opts: Vec<(String, String)> = tc::SHALLOW_SURFACES.iter().map(|(s, v)| (s.to_string(), format!("{s}  Kv={v:.1} fps"))).collect();
                opts.insert(0, (String::new(), "(custom)".into()));
                let mut sel = surface.clone();
                if widgets::combo_field(ui, "surf", "Surface Description:", &mut sel, &opts, 300.0) {
                    if let Some((_, v)) = tc::SHALLOW_SURFACES.iter().find(|(s, _)| *s == sel) {
                        *kv = *v;
                    }
                    *surface = sel;
                }
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    sf(ui, "slope", slope);
                    widgets::with_tip(ui, "Velocity factor in V = Kv × √slope: 20.3 paved, 16.1 unpaved (TR-55), lower for grass and woodland. Choose a surface above to fill it in.", |ui| {
                        conv_field(ui, "kv", "Velocity Factor Kv:", "fps", kv, |x| x, |x| x, 2, 130.0);
                    });
                });
            }
            TcKind::Channel { length, slope, n, section } => {
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    sf(ui, "slope", slope);
                    conv_field(ui, "n", "Manning's n:", "", n, |x| x, |x| x, 3, 100.0);
                    if let Some(v) = lookup::manning_button(ui, "tcn") {
                        *n = v;
                    }
                });
                section_editor(ui, section, u, true);
            }
            TcKind::Lag { length, slope } => {
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    conv_field(ui, "slope", "Average Watershed Slope:", "ft/ft", slope, |x| x, |x| x, 4, 160.0);
                });
                ui.label("Uses the subcatchment's composite CN: Tc = L^0.8 (S+1)^0.7 / (1140 Y^0.5)");
            }
            TcKind::Kirpich { length, slope, factor } => {
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    sf(ui, "slope", slope);
                });
                let opts: Vec<(f64, String)> = tc::KIRPICH_FACTORS.iter().map(|(s, v)| (*v, format!("{v:.1}  {s}"))).collect();
                widgets::combo_field(ui, "kf", "Adjustment Factor:", factor, &opts, 320.0);
            }
            TcKind::Kerby { length, slope, retardance } => {
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    sf(ui, "slope", slope);
                });
                let opts: Vec<(f64, String)> = tc::KERBY_SURFACES.iter().map(|(s, v)| (*v, format!("{v:.2}  {s}"))).collect();
                widgets::combo_field(ui, "kn", "Retardance N:", retardance, &opts, 320.0);
            }
            TcKind::Lake { length, mean_depth } => {
                ui.horizontal(|ui| {
                    lf(ui, "len", "Flow Length:", length);
                    conv_field(ui, "d", "Mean Depth:", u.len_label(), mean_depth, |x| u.len(x), |x| u.len_in(x), 2, 130.0);
                });
            }
        }
        ui.add_space(4.0);
        let r = seg.kind.evaluate(70.0);
        ui.label(RichText::new(format!("Tt = {:.1} min   V = {:.2} {}{}", r.minutes, u.vel(r.velocity), u.vel_label(), r.capacity.map(|c| format!("   Capacity = {}", u.fmt_flow(c))).unwrap_or_default())).strong());
        result = ok_cancel_apply(ui, false, "tc");
    });
    match result {
        Buttons::Ok | Buttons::Apply => {
            let (index, seg) = (index.to_owned(), seg.clone());
            if let NodeData::Subcat(sc) = &mut st.draft.data {
                match index {
                    Some(i) if i < sc.tc.len() => sc.tc[i] = seg,
                    _ => sc.tc.push(seg),
                }
            }
            st.tc_edit = None;
        }
        Buttons::Cancel => st.tc_edit = None,
        Buttons::None => {}
    }
}

/// Shape selector plus dimensions for channels and pipes.
pub fn section_editor(ui: &mut Ui, section: &mut Section, u: &Units, allow_area_perimeter: bool) {
    let kind = |s: &Section| match s {
        Section::Rectangular { .. } => 0,
        Section::Trapezoidal { .. } => 1,
        Section::Parabolic { .. } => 2,
        Section::Pipe { .. } => 3,
        Section::AreaPerimeter { .. } => 4,
        Section::Custom { .. } => 5,
        Section::Rating { .. } => 6,
    };
    let mut k = kind(section);
    let mut opts = vec![(0, "Rectangular".to_string()), (1, "Trapezoidal / Vee".to_string()), (2, "Parabolic".to_string()), (3, "Pipe / Conduit".to_string())];
    if allow_area_perimeter {
        opts.push((4, "Area & Perimeter".to_string()));
    }
    opts.push((5, "Custom Cross-Section".to_string()));
    // Rating tables describe reaches (Tc segments use Manning's equation).
    if !allow_area_perimeter {
        opts.push((6, "Rating Table".to_string()));
    }
    if widgets::combo_field(ui, "shape", "Shape:", &mut k, &opts, 200.0) {
        let depth = section.full_depth().max(1.0);
        *section = match k {
            0 => Section::Rectangular { width: 4.0, depth },
            1 => Section::Trapezoidal { bottom: 4.0, z_left: 2.0, z_right: 2.0, depth },
            2 => Section::Parabolic { width: 6.0, depth },
            3 => Section::circular(1.5),
            4 => Section::AreaPerimeter { area: 10.0, perimeter: 12.0 },
            6 => Section::Rating {
                points: vec![
                    RatingPoint { depth: 0.0, flow: 0.0, area: 0.0 },
                    RatingPoint { depth: 1.0, flow: 10.0, area: 5.0 },
                    RatingPoint { depth: 2.0, flow: 40.0, area: 14.0 },
                ],
                bank_full: 2.0,
            },
            _ => Section::Custom {
                points: vec![
                    XsPoint { offset: 0.0, elev: 2.0, n: 0.035 },
                    XsPoint { offset: 6.0, elev: 0.0, n: 0.030 },
                    XsPoint { offset: 10.0, elev: 0.0, n: 0.035 },
                    XsPoint { offset: 16.0, elev: 2.0, n: 0.035 },
                ],
            },
        };
    }
    let lf = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.len_label(), v, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
    let inch = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.small_len_label(), v, |x| u.small_len(x), |x| u.small_len_in(x), 1, 110.0);
    ui.horizontal(|ui| match section {
        Section::Rectangular { width, depth } => {
            lf(ui, "w", "Bottom Width:", width);
            lf(ui, "d", "Depth:", depth);
        }
        Section::Trapezoidal { bottom, z_left, z_right, depth } => {
            lf(ui, "b", "Bottom Width:", bottom);
            lf(ui, "d", "Depth:", depth);
            conv_field(ui, "zl", "Side Slope Z (left):", "h:v", z_left, |x| x, |x| x, 2, 120.0);
            conv_field(ui, "zr", "Side Slope Z (right):", "h:v", z_right, |x| x, |x| x, 2, 120.0);
        }
        Section::Parabolic { width, depth } => {
            lf(ui, "w", "Top Width:", width);
            lf(ui, "d", "Depth:", depth);
        }
        Section::Pipe { shape, width, height, fill } => {
            let opts: Vec<(PipeShape, String)> = PipeShape::ALL.iter().map(|s| (*s, s.label().to_string())).collect();
            if widgets::combo_field(ui, "ps", "Pipe Shape:", shape, &opts, 110.0) && *shape == PipeShape::Round {
                *height = *width;
            }
            if *shape == PipeShape::Round {
                if inch(ui, "dia", "Diameter:", width) {
                    *height = *width;
                }
            } else {
                inch(ui, "w", "Width (span):", width);
                inch(ui, "h", "Height (rise):", height);
            }
            inch(ui, "fill", "Inside Fill:", fill);
        }
        Section::AreaPerimeter { area, perimeter } => {
            conv_field(ui, "a", "Flow Area:", if u.metric() { "m²" } else { "sq-ft" }, area, |x| if u.metric() { x / stormwaves_core::units::FT_PER_M.powi(2) } else { x }, |x| if u.metric() { x * stormwaves_core::units::FT_PER_M.powi(2) } else { x }, 2, 110.0);
            lf(ui, "p", "Wetted Perimeter:", perimeter);
        }
        Section::Custom { points } => {
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("{:>3} {:>10} {:>10} {:>7}", "#", "Offset", "Elev", "n")).monospace());
                let mut del = None;
                for (i, p) in points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:>3}", i + 1)).monospace());
                        let mut o = u.len(p.offset);
                        if widgets::num_box(ui, ui.id().with(("xo", i)), &mut o, 2, 80.0, None) {
                            p.offset = u.len_in(o);
                        }
                        let mut e = u.len(p.elev);
                        if widgets::num_box(ui, ui.id().with(("xe", i)), &mut e, 2, 80.0, None) {
                            p.elev = u.len_in(e);
                        }
                        widgets::num_box(ui, ui.id().with(("xn", i)), &mut p.n, 3, 60.0, None);
                        if widgets::button(ui, "X", 22.0).clicked() {
                            del = Some(i);
                        }
                    });
                }
                if let Some(i) = del {
                    points.remove(i);
                }
                if widgets::button(ui, "Add Point", 80.0).clicked() {
                    let last = points.last().copied().unwrap_or(XsPoint { offset: 0.0, elev: 0.0, n: 0.035 });
                    points.push(XsPoint { offset: last.offset + 5.0, ..last });
                }
            });
        }
        Section::Rating { points, bank_full } => {
            ui.vertical(|ui| {
                lf(ui, "bf", "Bank-Full Depth:", bank_full);
                let area_unit = if u.metric() { "m²" } else { "sf" };
                let area_k = if u.metric() { stormwaves_core::units::FT_PER_M.powi(2) } else { 1.0 };
                ui.label(RichText::new(format!("{:>3} {:>10} {:>10} {:>10}", "#", format!("Depth ({})", u.len_label()), format!("Flow ({})", u.flow_label()), format!("Area ({area_unit})"))).monospace());
                let mut del = None;
                for (i, p) in points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:>3}", i + 1)).monospace());
                        let mut d = u.len(p.depth);
                        if widgets::num_box(ui, ui.id().with(("rd", i)), &mut d, 2, 80.0, None) {
                            p.depth = u.len_in(d);
                        }
                        let mut q = u.flow(p.flow);
                        if widgets::num_box(ui, ui.id().with(("rq", i)), &mut q, 3, 80.0, None) {
                            p.flow = u.flow_in(q);
                        }
                        let mut a = p.area / area_k;
                        if widgets::num_box(ui, ui.id().with(("ra", i)), &mut a, 2, 80.0, None) {
                            p.area = a * area_k;
                        }
                        if widgets::button(ui, "X", 22.0).clicked() {
                            del = Some(i);
                        }
                    });
                }
                if let Some(i) = del {
                    points.remove(i);
                }
                if widgets::button(ui, "Add Row", 80.0).clicked() {
                    let last = points.last().copied().unwrap_or(RatingPoint { depth: 0.0, flow: 0.0, area: 0.0 });
                    points.push(RatingPoint { depth: last.depth + 1.0, ..last });
                }
            });
        }
    });
    // Sketch.
    sketch_section(ui, section);
}

fn sketch_section(ui: &mut Ui, section: &Section) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(220.0, 90.0), egui::Sense::hover());
    let full = section.full_depth().max(0.01);
    let pts: Vec<(f64, f64)> = match section {
        Section::Custom { points } => points.iter().map(|p| (p.offset, p.elev)).collect(),
        _ => {
            let n = 40;
            let mut left = Vec::new();
            let mut right = Vec::new();
            for i in 0..=n {
                let y = full * i as f64 / n as f64;
                let w = section.props(y).top_width.max(if i == n { section.props(y * 0.999).top_width } else { 0.0 });
                left.push((-w / 2.0, y));
                right.push((w / 2.0, y));
            }
            left.reverse();
            left.into_iter().chain(right).collect()
        }
    };
    if pts.len() < 2 {
        return;
    }
    let (minx, maxx) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (miny, maxy) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let sx = (rect.width() as f64 - 10.0) / (maxx - minx).max(1e-6);
    let sy = (rect.height() as f64 - 10.0) / (maxy - miny).max(1e-6);
    let s = sx.min(sy);
    let cx = rect.center().x as f64 - (minx + maxx) / 2.0 * s;
    let to = |p: &(f64, f64)| egui::pos2((cx + p.0 * s) as f32, (rect.max.y as f64 - 5.0 - (p.1 - miny) * s) as f32);
    let screen: Vec<egui::Pos2> = pts.iter().map(to).collect();
    let water = matches!(section, Section::Pipe { .. });
    if water {
        ui.painter().add(egui::Shape::convex_polygon(screen.clone(), egui::Color32::from_rgb(0, 230, 240), egui::Stroke::new(1.5, t.text)));
    } else {
        ui.painter().add(egui::Shape::line(screen, egui::Stroke::new(2.0, t.text)));
    }
}

// ------------------------------------------------------------------ reach

fn reach_tab(ui: &mut Ui, st: &mut EditState, u: &Units) {
    let NodeData::Reach(r) = &mut st.draft.data else { return };
    section_editor(ui, &mut r.section, u, false);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        widgets::with_tip(ui, "Manning's roughness of the channel or pipe. Use the lookup button for typical values (e.g. 0.013 concrete pipe, 0.030-0.040 grassed channel). Not used for rating tables.", |ui| {
            conv_field(ui, "n", "Manning's n:", "", &mut r.n, |x| x, |x| x, 3, 100.0);
            if let Some(v) = lookup::manning_button(ui, "rn") {
                r.n = v;
            }
        });
        widgets::with_tip(ui, "Length of the reach along the flow path. Storage is the flow area times this length; Stor-Ind+Trans also delays the outflow by length / peak velocity.", |ui| {
            conv_field(ui, "len", "Length:", u.len_label(), &mut r.length, |x| u.len(x), |x| u.len_in(x), 1, 110.0);
        });
    });
    ui.horizontal(|ui| {
        widgets::with_tip(ui, "Channel bottom (or pipe invert) elevation at the upstream end. Depths and water surface elevations are reported from here.", |ui| {
            conv_field(ui, "in", "Inlet Invert:", u.len_label(), &mut r.inlet_invert, |x| u.len(x), |x| u.len_in(x), 2, 120.0);
        });
        widgets::with_tip(ui, "Channel bottom (or pipe invert) elevation at the downstream end. Together with the length this sets the slope.", |ui| {
            conv_field(ui, "out", "Outlet Invert:", u.len_label(), &mut r.outlet_invert, |x| u.len(x), |x| u.len_in(x), 2, 120.0);
        });
        widgets::with_tip(ui, "Average bed slope. Editing it moves the outlet invert so that slope = (inlet - outlet) / length.", |ui| {
            let mut slope = r.slope();
            if conv_field(ui, "s", "Slope:", "ft/ft", &mut slope, |x| x, |x| x, 4, 110.0) && slope.is_finite() {
                r.outlet_invert = r.inlet_invert - slope * r.length;
            }
        });
    });
    ui.horizontal(|ui| {
        widgets::with_tip(ui, "A constant loss through the channel bed (e.g. infiltration in a dry swale). It is taken from the inflow first and reported as discarded.", |ui| {
            conv_field(ui, "exf", "Constant Exfiltration:", u.flow_label(), &mut r.exfiltration, |x| u.flow(x), |x| u.flow_in(x), 2, 150.0);
        });
    });
    let cap = r.section.manning_q(r.section.full_depth(), r.n, r.slope());
    ui.label(RichText::new(format!("Capacity at bank full = {}", u.fmt_flow(cap))).strong());
}

// ------------------------------------------------------------------ pond

fn pond_tab(ui: &mut Ui, st: &mut EditState, u: &Units, tab: &str, _doc: &Doc) {
    let NodeData::Pond(p) = &mut st.draft.data else { return };
    let large = p.large_units;
    let vol = |v: f64| if large { u.fmt_volume(v) } else { format!("{} {}", stormwaves_core::units::group(if u.metric() { v / stormwaves_core::units::CF_PER_M3 } else { v }, 0), if u.metric() { "m³" } else { "cf" }) };
    match tab {
        "Storage" => {
            let rows: Vec<Vec<String>> = p
                .storage
                .volumes
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let emb = v.embedded_in.map(|j| format!(" (in #{})", j + 1)).unwrap_or_default();
                    vec![u.fmt_elev(v.bottom()), vol(p.storage.available(i)), format!("{}: {}{emb}", v.description, v.shape.label())]
                })
                .collect();
            let ev = widgets::list_table(ui, "storage", &[("Invert", 80.0, true), ("Avail.Storage", 100.0, true), ("Storage Description", 300.0, false)], &rows, &mut st.sel_row, 150.0);
            let mut edit = ev.double_clicked;
            let mut del = None;
            let sel = st.sel_row.filter(|i| *i < p.storage.volumes.len());
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, "Edit...", 70.0, sel.is_some()).clicked() {
                    edit = sel;
                }
                if widgets::button_enabled(ui, "Delete", 70.0, sel.is_some()).clicked() {
                    del = sel;
                    st.sel_row = None;
                }
            });
            if let Some(i) = del {
                p.storage.volumes.remove(i);
                for v in &mut p.storage.volumes {
                    v.embedded_in = match v.embedded_in {
                        Some(j) if j == i => None,
                        Some(j) if j > i => Some(j - 1),
                        other => other,
                    };
                }
            }
            if let Some(i) = edit {
                if let Some(v) = p.storage.volumes.get(i) {
                    st.volume_edit = Some((Some(i), v.clone()));
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Add storage:");
                let base = p.storage.bottom().unwrap_or(100.0);
                let shapes = [
                    ("Custom Stage Data", VolumeShape::Custom { kind: CustomKind::AreaPrismatic, elevations: vec![base, base + 1.0, base + 2.0], values: vec![1000.0, 2000.0, 3000.0] }),
                    ("Prismatoid", VolumeShape::Prismatoid { width: 20.0, length: 40.0, height: 4.0, z: 3.0 }),
                    ("Cone/Cylinder", VolumeShape::VerticalCone { diameter: 8.0, height: 6.0, z: 0.0 }),
                    ("Pipe Storage", VolumeShape::Pipe { shape: PipeShape::Round, width: 4.0, height: 4.0, length: 100.0, slope: 0.0 }),
                    ("Arch Chamber", VolumeShape::Arch { width: 4.0, height: 2.5, length: 50.0 }),
                ];
                for (label, shape) in shapes {
                    if widgets::button(ui, label, 0.0).clicked() {
                        st.volume_edit = Some((None, StorageVolume::new(label, base, shape)));
                    }
                }
            });
            if let (Some(b), Some(top)) = (p.storage.bottom(), p.storage.top()) {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("Storage {} to {}: {}", u.fmt_elev(b), u.fmt_elev(top), vol(p.storage.storage(top)))).strong());
            } else {
                ui.label("No storage: the pond is modelled as a zero-storage junction (outflow = inflow).");
            }
        }
        "Outlets" => {
            let rows: Vec<Vec<String>> = p
                .devices
                .iter()
                .map(|d| vec![u.fmt_elev(d.invert), if d.description.is_empty() { d.kind.label().to_string() } else { d.description.clone() }, d.routing.label()])
                .collect();
            let ev = widgets::list_table(ui, "outlets", &[("Invert", 80.0, true), ("Description", 260.0, false), ("Routing", 100.0, false)], &rows, &mut st.sel_row, 150.0);
            let mut edit = ev.double_clicked;
            let mut del = None;
            let sel = st.sel_row.filter(|i| *i < p.devices.len());
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, "Edit Outlet...", 100.0, sel.is_some()).clicked() {
                    edit = sel;
                }
                if widgets::button_enabled(ui, "Delete", 70.0, sel.is_some()).clicked() {
                    del = sel;
                    st.sel_row = None;
                }
            });
            if let Some(i) = del {
                p.devices.remove(i);
                for d in &mut p.devices {
                    d.routing = match d.routing {
                        stormwaves_core::outlets::Routing::Device(j) if j == i => stormwaves_core::outlets::Routing::Primary,
                        stormwaves_core::outlets::Routing::Device(j) if j > i => stormwaves_core::outlets::Routing::Device(j - 1),
                        other => other,
                    };
                }
            }
            if let Some(i) = edit {
                if let Some(d) = p.devices.get(i) {
                    st.device_edit = Some(DeviceEditor::new(Some(i), d.clone()));
                }
            }
            ui.add_space(4.0);
            ui.label("Add outlet device:");
            ui.horizontal_wrapped(|ui| {
                let base = p.storage.bottom().unwrap_or(100.0);
                for (label, kind) in outlet::templates() {
                    if widgets::button(ui, label, 0.0).clicked() {
                        st.device_edit = Some(DeviceEditor::new(None, stormwaves_core::outlets::OutletDevice::new(label, base, stormwaves_core::outlets::Routing::Primary, kind)));
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Tip: for a riser, enter the outlet barrel (culvert) first, then route the openings to it (Routing = Device 1).").color(Tokens::get(ui.ctx()).text_dim));
        }
        "Tailwater" => {
            for (k, name) in ["Primary", "Secondary", "Tertiary"].iter().enumerate() {
                widgets::group(ui, &format!("{name} Tailwater"), |ui| {
                    ui.horizontal(|ui| {
                        let cur = p.tailwater[k];
                        if widgets::radio(ui, matches!(cur, Tailwater::Free), "Free discharge").clicked() {
                            p.tailwater[k] = Tailwater::Free;
                        }
                        if widgets::radio(ui, matches!(cur, Tailwater::Fixed(_)), "Fixed elevation").clicked() {
                            p.tailwater[k] = Tailwater::Fixed(p.storage.bottom().unwrap_or(0.0));
                        }
                        if widgets::radio(ui, matches!(cur, Tailwater::Differential(_)), "Differential").clicked() {
                            p.tailwater[k] = Tailwater::Differential(1.0);
                        }
                        if widgets::radio(ui, matches!(cur, Tailwater::Dynamic), "Dynamic (downstream node)").clicked() {
                            p.tailwater[k] = Tailwater::Dynamic;
                        }
                        match &mut p.tailwater[k] {
                            Tailwater::Fixed(e) => {
                                conv_field(ui, &format!("tw{k}"), "Elevation:", u.len_label(), e, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                            }
                            Tailwater::Differential(d) => {
                                conv_field(ui, &format!("tw{k}"), "Below HW by:", u.len_label(), d, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                            }
                            _ => {}
                        }
                    });
                });
            }
            ui.label(RichText::new("Dynamic tailwater uses the downstream pond's (or link's) water surface; select Dyn-Stor-Ind routing on the calculation settings.").color(Tokens::get(ui.ctx()).text_dim));
        }
        "Advanced" => {
            ui.horizontal(|ui| {
                widgets::with_tip(ui, "Water level at the start of the calculation, e.g. the permanent pool of a wet pond. Blank starts the pond empty.", |ui| {
                    opt_field(ui, "start", "Starting Elevation:", u.len_label(), &mut p.start_elevation, |x| u.len(x), |x| u.len_in(x), 2, 150.0);
                });
                widgets::with_tip(ui, "The level at which the pond floods (e.g. top of berm). It is shown in reports for comparison with the peak elevation; it does not change the routing.", |ui| {
                    opt_field(ui, "flood", "Flood Elevation:", u.len_label(), &mut p.flood_elevation, |x| u.len(x), |x| u.len_in(x), 2, 150.0);
                });
            });
            ui.label("Leave the starting elevation blank to start at the bottom of storage.");
        }
        _ => {}
    }
}

fn volume_editor(ctx: &egui::Context, st: &mut EditState, u: &Units) {
    let Some((index, vol)) = &mut st.volume_edit else { return };
    let count = match &st.draft.data {
        NodeData::Pond(p) => p.storage.volumes.len(),
        _ => 0,
    };
    let mut result = Buttons::None;
    crate::frame::Win::new(format!("Storage Volume: {}", vol.shape.label())).collapsible(false).resizable(true).show(ctx, |ui| {
        let lf = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.len_label(), v, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
        ui.horizontal(|ui| {
            widgets::text_field(ui, "Description:", &mut vol.description, 220.0);
            if !matches!(vol.shape, VolumeShape::Custom { .. }) {
                lf(ui, "inv", "Invert:", &mut vol.invert);
            }
        });
        ui.horizontal(|ui| match &mut vol.shape {
            VolumeShape::Prismatoid { width, length, height, z } => {
                lf(ui, "w", "Width:", width);
                lf(ui, "l", "Length:", length);
                lf(ui, "h", "Height:", height);
                conv_field(ui, "z", "Side Slope Z:", "h:v", z, |x| x, |x| x, 2, 100.0);
            }
            VolumeShape::VerticalCone { diameter, height, z } => {
                lf(ui, "d", "Diameter:", diameter);
                lf(ui, "h", "Height:", height);
                conv_field(ui, "z", "Side Slope Z:", "h:v", z, |x| x, |x| x, 2, 100.0);
            }
            VolumeShape::Pipe { shape, width, height, length, slope } => {
                let opts: Vec<(PipeShape, String)> = PipeShape::ALL.iter().map(|s| (*s, s.label().to_string())).collect();
                if widgets::combo_field(ui, "ps", "Shape:", shape, &opts, 110.0) && *shape == PipeShape::Round {
                    *height = *width;
                }
                let inch = |ui: &mut Ui, id: &str, label: &str, v: &mut f64| conv_field(ui, id, label, u.small_len_label(), v, |x| u.small_len(x), |x| u.small_len_in(x), 1, 100.0);
                if inch(ui, "w", if *shape == PipeShape::Round { "Diameter:" } else { "Width:" }, width) && *shape == PipeShape::Round {
                    *height = *width;
                }
                if *shape != PipeShape::Round {
                    inch(ui, "h", "Height:", height);
                }
                lf(ui, "l", "Length:", length);
                conv_field(ui, "s", "Slope:", "ft/ft", slope, |x| x, |x| x, 4, 100.0);
            }
            VolumeShape::Arch { width, height, length } => {
                lf(ui, "w", "Width:", width);
                lf(ui, "h", "Height:", height);
                lf(ui, "l", "Length:", length);
            }
            VolumeShape::Custom { kind, elevations, values } => {
                ui.vertical(|ui| {
                    let opts: Vec<(CustomKind, String)> = CustomKind::ALL.iter().map(|k| (*k, k.label().to_string())).collect();
                    widgets::combo_field(ui, "ck", "Data Type:", kind, &opts, 220.0);
                    let is_area = matches!(kind, CustomKind::AreaPrismatic | CustomKind::AreaConic);
                    let (val_label, to, from): (String, Box<dyn Fn(f64) -> f64>, Box<dyn Fn(f64) -> f64>) = if is_area {
                        (format!("Surf.Area ({})", if u.metric() { "m²" } else { "sq-ft" }), Box::new(|v| if u.metric() { v / stormwaves_core::units::FT_PER_M.powi(2) } else { v }), Box::new(|v| if u.metric() { v * stormwaves_core::units::FT_PER_M.powi(2) } else { v }))
                    } else {
                        (format!("Storage ({})", if u.metric() { "m³" } else { "cu-ft" }), Box::new(|v| if u.metric() { v / stormwaves_core::units::CF_PER_M3 } else { v }), Box::new(|v| if u.metric() { v * stormwaves_core::units::CF_PER_M3 } else { v }))
                    };
                    ui.label(RichText::new(format!("{:>3} {:>12} {:>16}", "#", format!("Elev ({})", if u.metric() { "m" } else { "ft" }), val_label)).monospace());
                    let mut del = None;
                    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        for i in 0..elevations.len().min(values.len()) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{:>3}", i + 1)).monospace());
                                let mut e = u.len(elevations[i]);
                                if widgets::num_box(ui, ui.id().with(("ce", i)), &mut e, 2, 90.0, None) {
                                    elevations[i] = u.len_in(e);
                                }
                                let mut v = to(values[i]);
                                if widgets::num_box(ui, ui.id().with(("cv", i)), &mut v, 0, 110.0, None) {
                                    values[i] = from(v).max(0.0);
                                }
                                if widgets::button(ui, "X", 22.0).clicked() {
                                    del = Some(i);
                                }
                            });
                        }
                    });
                    if let Some(i) = del {
                        elevations.remove(i);
                        values.remove(i);
                    }
                    if widgets::button(ui, "Add Stage", 80.0).clicked() {
                        let e = elevations.last().copied().unwrap_or(100.0) + 1.0;
                        let v = values.last().copied().unwrap_or(0.0);
                        elevations.push(e);
                        values.push(v);
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            conv_field(ui, "voids", "Voids:", "%", &mut vol.voids_pct, |x| x, |x| x.clamp(0.0, 100.0), 1, 90.0);
            conv_field(ui, "mult", "Multiplier:", "", &mut vol.multiplier, |x| x, |x| x.max(0.0), 0, 90.0);
            let mut opts: Vec<(Option<usize>, String)> = vec![(None, "(not embedded)".into())];
            for i in 0..count {
                if Some(i) != *index {
                    opts.push((Some(i), format!("Volume #{}", i + 1)));
                }
            }
            widgets::combo_field(ui, "emb", "Embedded in:", &mut vol.embedded_in, &opts, 140.0);
        });
        result = ok_cancel_apply(ui, false, "pond");
    });
    match result {
        Buttons::Ok | Buttons::Apply => {
            let (index, vol) = (*index, vol.clone());
            if let NodeData::Pond(p) = &mut st.draft.data {
                match index {
                    Some(i) if i < p.storage.volumes.len() => p.storage.volumes[i] = vol,
                    _ => p.storage.volumes.push(vol),
                }
            }
            st.volume_edit = None;
        }
        Buttons::Cancel => st.volume_edit = None,
        Buttons::None => {}
    }
}

// ------------------------------------------------------------------ link

fn link_tab(ui: &mut Ui, st: &mut EditState, u: &Units, tab: &str, actions: &mut Vec<WindowAction>) {
    let id = st.node;
    let NodeData::Link(l) = &mut st.draft.data else { return };
    match tab {
        "Manual" => {
            ui.horizontal(|ui| {
                let manual = matches!(l.source, LinkSource::Manual(_));
                let inflows = matches!(l.source, LinkSource::Inflows);
                let file = matches!(l.source, LinkSource::Table { .. });
                if widgets::radio(ui, inflows, "Sum of inflows only").clicked() {
                    l.source = LinkSource::Inflows;
                }
                if widgets::radio(ui, manual, "Enter hydrograph manually").clicked() && !manual {
                    l.source = LinkSource::Manual(ManualHydrograph { t0: 0.0, dt: 0.5, flows: vec![0.0], per_event: vec![] });
                }
                if widgets::radio(ui, file, "Import from CSV file").clicked() {
                    actions.push(WindowAction::PickCsvForLink(id));
                }
            });
            ui.add_space(6.0);
            match &mut l.source {
                LinkSource::Manual(m) => {
                    ui.horizontal(|ui| {
                        conv_field(ui, "t0", "Starting Time (to):", "hours", &mut m.t0, |x| x, |x| x, 2, 130.0);
                        conv_field(ui, "dt", "Time Step (dt):", "hours", &mut m.dt, |x| x, |x| x.max(1e-4), 3, 130.0);
                        conv_field(ui, "area", "Inflow Area:", u.area_heading(), &mut l.inflow_area, |x| u.area(x), |x| u.area_in(x), 3, 130.0);
                    });
                    ui.label(format!("Flows ({}) one per time step, separated by spaces or new lines:", u.flow_label()));
                    ui.add_sized(vec2(ui.available_width(), 160.0), egui::TextEdit::multiline(&mut st.manual_text).font(egui::TextStyle::Monospace));
                    let n = st.manual_text.split(|c: char| c.is_whitespace() || c == ',').filter(|s| s.trim().parse::<f64>().is_ok()).count();
                    ui.label(format!("{n} flow values present"));
                }
                LinkSource::Table { name, columns, selected, times } => {
                    ui.label(format!("File: {name} ({} rows, {} hydrographs)", times.len(), columns.len()));
                    let mut opts: Vec<(Option<String>, String)> = vec![(None, "(MultiEvent: match event names)".into())];
                    for (c, _) in columns.iter() {
                        opts.push((Some(c.clone()), c.clone()));
                    }
                    widgets::combo_field(ui, "col", "Hydrograph:", selected, &opts, 260.0);
                    conv_field(ui, "area", "Inflow Area:", u.area_heading(), &mut l.inflow_area, |x| u.area(x), |x| u.area_in(x), 3, 130.0);
                }
                LinkSource::Inflows => {
                    ui.label("The link outflow is the sum of the nodes routed into it.");
                }
            }
        }
        "Advanced" => {
            let a = &mut l.adjust;
            ui.horizontal(|ui| {
                opt_field(ui, "th", "Flow Threshold (primary gets flow above):", u.flow_label(), &mut a.threshold, |x| u.flow(x), |x| u.flow_in(x), 2, 250.0);
                opt_field(ui, "lim", "Flow Limit (primary capped at):", u.flow_label(), &mut a.limit, |x| u.flow(x), |x| u.flow_in(x), 2, 200.0);
            });
            ui.horizontal(|ui| {
                conv_field(ui, "mult", "Discharge Multiplier:", "", &mut a.multiplier, |x| x, |x| x.max(0.0), 3, 140.0);
                conv_field(ui, "tm", "Time Multiplier:", "", &mut a.time_multiplier, |x| x, |x| x.max(0.0), 3, 120.0);
                conv_field(ui, "lag", "Time Lag:", "hours", &mut a.time_lag, |x| x, |x| x.max(0.0), 2, 110.0);
            });
            ui.horizontal(|ui| {
                opt_field(ui, "st", "Start Time:", "hours", &mut a.start_time, |x| x, |x| x, 2, 110.0);
                opt_field(ui, "et", "End Time:", "hours", &mut a.end_time, |x| x, |x| x, 2, 110.0);
                opt_field(ui, "sv", "Slice Volume:", u.volume_heading(), &mut a.slice_volume, |x| u.volume(x), |x| u.volume_in(x), 3, 140.0);
            });
            ui.label(RichText::new("Any flow removed from the primary outflow appears as the secondary outflow.").color(Tokens::get(ui.ctx()).text_dim));
        }
        "Elevation" => {
            let e = &mut l.elevation;
            ui.horizontal(|ui| {
                if widgets::radio(ui, matches!(e, LinkElevation::None), "None").clicked() {
                    *e = LinkElevation::None;
                }
                if widgets::radio(ui, matches!(e, LinkElevation::Fixed(_)), "Fixed").clicked() {
                    *e = LinkElevation::Fixed(100.0);
                }
                if widgets::radio(ui, matches!(e, LinkElevation::Tidal { .. }), "Tidal").clicked() {
                    *e = LinkElevation::Tidal { mean: 100.0, amplitude: 2.0, period: 12.42, phase: 0.0 };
                }
                if widgets::radio(ui, matches!(e, LinkElevation::Table(_)), "Table").clicked() {
                    *e = LinkElevation::Table(vec![(0.0, 100.0), (24.0, 100.0)]);
                }
            });
            match e {
                LinkElevation::Fixed(v) => {
                    conv_field(ui, "fe", "Tailwater Elevation:", u.len_label(), v, |x| u.len(x), |x| u.len_in(x), 2, 150.0);
                }
                LinkElevation::Tidal { mean, amplitude, period, phase } => {
                    ui.horizontal(|ui| {
                        conv_field(ui, "m", "Mean Elevation:", u.len_label(), mean, |x| u.len(x), |x| u.len_in(x), 2, 120.0);
                        conv_field(ui, "a", "Amplitude:", u.len_label(), amplitude, |x| u.len(x), |x| u.len_in(x), 2, 110.0);
                        conv_field(ui, "p", "Period:", "hours", period, |x| x, |x| x, 2, 100.0);
                        conv_field(ui, "ph", "Phase:", "hours", phase, |x| x, |x| x, 2, 100.0);
                    });
                }
                LinkElevation::Table(pts) => {
                    let mut del = None;
                    for (i, p) in pts.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            widgets::num_box(ui, ui.id().with(("lt", i)), &mut p.0, 2, 80.0, None);
                            let mut e = u.len(p.1);
                            if widgets::num_box(ui, ui.id().with(("le", i)), &mut e, 2, 90.0, None) {
                                p.1 = u.len_in(e);
                            }
                            if widgets::button(ui, "X", 22.0).clicked() {
                                del = Some(i);
                            }
                        });
                    }
                    if let Some(i) = del {
                        pts.remove(i);
                    }
                    if widgets::button(ui, "Add Row", 80.0).clicked() {
                        let last = pts.last().copied().unwrap_or((0.0, 100.0));
                        pts.push((last.0 + 1.0, last.1));
                    }
                }
                LinkElevation::None => {
                    ui.label("No tailwater is defined by this link.");
                }
            }
        }
        _ => {}
    }
}

fn text_tab(ui: &mut Ui, st: &mut EditState) {
    let NodeData::Text(t) = &mut st.draft.data else { return };
    ui.label("Text:");
    ui.add_sized(vec2(ui.available_width(), 140.0), egui::TextEdit::multiline(&mut t.text));
    ui.horizontal(|ui| {
        let mut size = t.size as f64;
        if conv_field(ui, "size", "Font Size:", "pt", &mut size, |x| x, |x| x.clamp(4.0, 120.0), 0, 100.0) {
            t.size = size as f32;
        }
        widgets::check(ui, &mut t.border, "Border");
    });
}
