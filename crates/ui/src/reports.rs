//! Report windows: node reports, project listings, comparisons, messages and curve viewers.

use crate::chart::{self, ChartSpec, ChartView, Series};
use crate::doc::Doc;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{Align2, Color32, FontId, RichText, Sense, Ui, vec2};
use stormwaves_core::calc::{Detail, NodeResult, Severity};
use stormwaves_core::outlets;
use stormwaves_core::project::{Node, NodeData, NodeId, NodeType, Project};
use stormwaves_core::report;
use stormwaves_core::units::group;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Summary,
    Hydrograph,
    Discharge,
    Storage,
    Events,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Summary => "Summary",
            Tab::Hydrograph => "Hydrograph",
            Tab::Discharge => "Discharge",
            Tab::Storage => "Storage",
            Tab::Events => "Events",
        }
    }
}

pub struct ReportState {
    pub node: NodeId,
    pub tab: Tab,
    pub table: bool,
    pub three_d: bool,
    pub shrink: bool,
    pub view: ChartView,
    pub open: bool,
}

impl ReportState {
    pub fn new(node: NodeId) -> Self {
        ReportState { node, tab: Tab::Hydrograph, table: false, three_d: true, shrink: true, view: ChartView::default(), open: true }
    }
}

fn tabs_for(t: NodeType) -> Vec<Tab> {
    match t {
        NodeType::Pond => vec![Tab::Summary, Tab::Hydrograph, Tab::Discharge, Tab::Storage, Tab::Events],
        NodeType::Reach => vec![Tab::Summary, Tab::Hydrograph, Tab::Discharge, Tab::Events],
        _ => vec![Tab::Summary, Tab::Hydrograph, Tab::Events],
    }
}

/// A toggle-style button used for report tabs.
fn tab_button(ui: &mut Ui, label: &str, on: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(13.0);
    let w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x) + 14.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, t.radius as f32, if on { t.light.gamma_multiply(0.9) } else { t.face });
    widgets::bevel(p, r, !on, &t);
    p.text(r.center() + if on { vec2(1.0, 1.0) } else { vec2(0.0, 0.0) }, Align2::CENTER_CENTER, label, font, t.text);
    if on {
        p.rect_stroke(r.shrink(3.0), 0.0, egui::Stroke::new(1.0, t.shadow), egui::StrokeKind::Inside);
    }
    resp.clicked()
}

const COLORS: [Color32; 6] = [
    Color32::from_rgb(40, 200, 60),
    Color32::from_rgb(160, 160, 160),
    Color32::from_rgb(30, 120, 255),
    Color32::from_rgb(240, 60, 60),
    Color32::from_rgb(240, 160, 30),
    Color32::from_rgb(160, 60, 200),
];

fn hydro_series(project: &Project, node: &Node, r: &NodeResult) -> Vec<Series> {
    let u = &project.units;
    let mk = |name: &str, h: &stormwaves_core::hydrograph::Hydrograph, c: Color32| Series {
        name: name.into(),
        color: c,
        x: (0..h.len()).map(|i| h.time(i)).collect(),
        y: h.q.iter().map(|q| u.flow(*q)).collect(),
        peak_label: Some(format!("{} {}", group(u.flow(h.peak().value), u.flow_dp()), u.flow_label())),
    };
    match node.node_type() {
        NodeType::Subcat => vec![mk("Runoff", &r.outflow, Color32::from_rgb(20, 190, 220))],
        _ => {
            let mut v = vec![mk("Inflow", &r.inflow, COLORS[0]), mk("Outflow", &r.outflow, COLORS[1])];
            let names = ["Primary", "Secondary", "Tertiary", "Discarded"];
            let multi = r.routes.iter().filter(|h| h.has_flow()).count() > 1 || r.routes[1].has_flow();
            if multi {
                for k in 0..4 {
                    if r.routes[k].has_flow() {
                        v.push(mk(names[k], &r.routes[k], COLORS[2 + k]));
                    }
                }
            }
            v
        }
    }
}

fn summary_lines(project: &Project, node: &Node, r: &NodeResult, event_heading: &str) -> Vec<String> {
    let u = &project.units;
    let f = |h: &stormwaves_core::hydrograph::Hydrograph| format!("{} {}", group(u.flow(h.peak().value), u.flow_dp()), u.flow_label());
    let mut out = Vec::new();
    match (&node.data, &r.detail) {
        (NodeData::Subcat(_), Detail::Subcat(d)) => {
            out.push(format!("Runoff={}", f(&r.outflow)));
            out.push(event_heading.split(" Rainfall=").next().unwrap_or("").to_string());
            if let Some(rain) = event_heading.split(" Rainfall=").nth(1) {
                out.push(format!("Rainfall={rain}"));
            }
            out.push(format!("Runoff Area={}", u.fmt_area(d.composite.total_area)));
            out.push(format!("Runoff Volume={}", u.fmt_volume(r.outflow.volume_cf())));
            let depth = if d.composite.total_area > 0.0 { r.outflow.volume_cf() / d.composite.total_area * 12.0 } else { 0.0 };
            out.push(format!("Runoff Depth={}", u.fmt_rain(depth)));
            if d.flow_length > 0.0 {
                out.push(format!("Flow Length={}", u.fmt_len(d.flow_length)));
            }
            out.push(format!("Tc={:.1} min", d.tc_minutes));
            out.push(format!("CN={}", if (d.composite.cn - d.composite.cn.round()).abs() < 1e-9 { format!("{:.0}", d.composite.cn) } else { format!("{:.1}", d.composite.cn) }));
        }
        (NodeData::Reach(reach), Detail::Reach(d)) => {
            if r.inflow_area > 0.0 {
                out.push(format!("Inflow Area={}", u.fmt_area(r.inflow_area)));
            }
            out.push(format!("Inflow={}", f(&r.inflow)));
            out.push(format!("Outflow={}", f(&r.outflow)));
            out.push(format!("Depth={:.2}{}", u.len(d.peak_depth), u.len_suffix()));
            out.push(format!("Max Vel={:.1} {}", u.vel(d.max_velocity), u.vel_label()));
            out.push(format!("n={:.3}", reach.n));
            out.push(format!("L={}{}", group(u.len(reach.length), 1), u.len_suffix()));
            out.push(format!("S={:.4} '/'", reach.slope()));
            out.push(format!("Capacity={}", u.fmt_flow(d.capacity)));
        }
        (NodeData::Pond(_), Detail::Pond(d)) => {
            if r.inflow_area > 0.0 {
                out.push(format!("Inflow Area={}", u.fmt_area(r.inflow_area)));
            }
            out.push(format!("Inflow={}", f(&r.inflow)));
            out.push(format!("Outflow={}", f(&r.outflow)));
            let names = ["Primary", "Secondary", "Tertiary", "Discarded"];
            if r.routes.iter().filter(|h| h.has_flow()).count() > 1 {
                for k in 0..4 {
                    if r.routes[k].has_flow() {
                        out.push(format!("{}={}", names[k], f(&r.routes[k])));
                    }
                }
            }
            out.push(format!("Peak Elev={}", u.fmt_elev(d.peak_elevation)));
            out.push(format!("Storage={}", u.fmt_volume(d.peak_storage)));
        }
        (NodeData::Link(_), Detail::Link(desc)) => {
            out.push(format!("Inflow={}", f(&r.inflow)));
            if r.routes[1].has_flow() {
                out.push(format!("Primary={}", f(&r.routes[0])));
                out.push(format!("Secondary={}", f(&r.routes[1])));
            }
            out.push(desc.clone());
        }
        _ => {}
    }
    out
}

fn text_area(ui: &mut Ui, lines: &[String], height: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(4.0)));
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        for l in lines {
            let color = if l.contains("Warning:") || l.contains("Error:") { t.warning } else if l.contains("Hint:") { t.hint } else { t.text };
            ui.add(egui::Label::new(RichText::new(l).monospace().color(color)).wrap_mode(egui::TextWrapMode::Extend));
        }
    });
}

fn data_table(ui: &mut Ui, heads: &[String], cols: &[Vec<f64>], height: f32, bold_max: bool) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(4.0)));
    let rows = cols.first().map(|c| c.len()).unwrap_or(0);
    let maxima: Vec<Option<usize>> = cols.iter().map(|c| c.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i)).collect();
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        egui::Grid::new("hydro-table").striped(false).min_col_width(70.0).spacing(vec2(14.0, 1.0)).show(ui, |ui| {
            for h in heads {
                let (a, b) = h.split_once(" (").map(|(a, b)| (a.to_string(), format!("({b}"))).unwrap_or((h.clone(), String::new()));
                ui.vertical(|ui| {
                    ui.label(RichText::new(a).monospace());
                    ui.label(RichText::new(b).monospace());
                });
            }
            ui.end_row();
            for i in 0..rows {
                for (j, c) in cols.iter().enumerate() {
                    let v = c.get(i).copied().unwrap_or(0.0);
                    let s = if j == 0 { format!("{v:>8.2}") } else if v.abs() >= 1000.0 { format!("{:>10}", group(v, 0)) } else { format!("{v:>10.3}") };
                    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
                    let s = if j == 0 { format!("{v:.2}") } else { s };
                    let mut rt = RichText::new(s).monospace();
                    if bold_max && j > 0 && maxima.get(j).copied().flatten() == Some(i) {
                        rt = rt.strong().color(t.select);
                    }
                    ui.label(rt);
                }
                ui.end_row();
            }
        });
    });
}

/// The node report window. Returns false when closed.
pub fn show_node_report(ctx: &egui::Context, doc: &mut Doc, st: &mut ReportState, actions: &mut Vec<WindowAction>) -> bool {
    let Some(node) = doc.project.node(st.node).cloned() else { return false };
    let tabs = tabs_for(node.node_type());
    if !tabs.contains(&st.tab) {
        st.tab = Tab::Summary;
    }
    let title = format!("{} - {}", node.title(), doc.project.title);
    let mut open = st.open;
    let events_needed = st.tab == Tab::Events;
    if events_needed {
        doc.all_results();
    }
    crate::frame::Win::new(title).id(egui::Id::new(("report", st.node))).open(&mut open).default_size(vec2(640.0, 430.0)).min_size(vec2(420.0, 300.0)).resizable(true).collapsible(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for tab in &tabs {
                if tab_button(ui, tab.label(), st.tab == *tab) {
                    st.tab = *tab;
                    st.view = ChartView::default();
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new(&doc.results.event).color(Tokens::get(ui.ctx()).text_dim));
        });
        ui.add_space(4.0);
        let avail = ui.available_size();
        ui.horizontal(|ui| {
            let body_w = (avail.x - 70.0).max(200.0);
            let h = (avail.y - 6.0).max(150.0);
            ui.vertical(|ui| {
                ui.set_width(body_w);
                body(ui, doc, &node, st, vec2(body_w, h));
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let charty = matches!(st.tab, Tab::Hydrograph | Tab::Discharge | Tab::Storage);
                if widgets::button_enabled(ui, if st.table { "Graph" } else { "Table" }, 62.0, charty).clicked() {
                    st.table = !st.table;
                }
                if widgets::button_enabled(ui, if st.three_d { "2D" } else { "3D" }, 62.0, charty && !st.table).clicked() {
                    st.three_d = !st.three_d;
                }
                if widgets::button_enabled(ui, if st.shrink { "Expand" } else { "Shrink" }, 62.0, st.table).clicked() {
                    st.shrink = !st.shrink;
                }
                if widgets::button(ui, "Copy", 62.0).on_hover_text("Copy the summary to the clipboard").clicked() {
                    let lines = report::node_summary(&doc.project, &doc.results, &node);
                    ui.ctx().copy_text(lines.join("\n"));
                    actions.push(WindowAction::Status("Summary copied to the clipboard".into()));
                }
                if widgets::button(ui, "Edit", 62.0).clicked() {
                    actions.push(WindowAction::OpenEdit(node.id));
                }
            });
        });
    });
    st.open = open;
    open
}

fn body(ui: &mut Ui, doc: &mut Doc, node: &Node, st: &mut ReportState, size: egui::Vec2) {
    let project = &doc.project;
    let Some(r) = doc.results.node(node.id) else {
        ui.label("Not calculated (see the message window).");
        return;
    };
    match st.tab {
        Tab::Summary => {
            let lines = report::node_summary(project, &doc.results, node);
            text_area(ui, &lines, size.y);
        }
        Tab::Hydrograph => {
            if st.table {
                let (heads, cols) = report::hydrograph_table(project, &doc.results, node, st.shrink);
                data_table(ui, &heads, &cols, size.y, true);
            } else {
                let series = hydro_series(project, node, r);
                let summary = summary_lines(project, node, r, &doc.results.event_heading);
                let y_label = format!("Flow ({})", project.units.flow_label());
                let spec = ChartSpec { title: "Hydrograph", subtitle: &node.title(), x_label: "Time (hours)", y_label: &y_label, series: &series, summary: &summary, three_d: st.three_d, marks: &[] };
                chart::chart(ui, &spec, &mut st.view, size);
            }
        }
        Tab::Discharge | Tab::Storage => rating_tab(ui, project, node, r, st, size),
        Tab::Events => {
            let all = doc.all_results.as_deref().unwrap_or(&[]);
            events_tab(ui, project, node, all, size);
        }
    }
}

fn rating_tab(ui: &mut Ui, project: &Project, node: &Node, r: &NodeResult, st: &mut ReportState, size: egui::Vec2) {
    let u = &project.units;
    let mut series = Vec::new();
    let mut marks = Vec::new();
    let mut heads = vec![format!("Elevation ({})", if u.metric() { "m" } else { "feet" })];
    let mut cols: Vec<Vec<f64>> = Vec::new();
    match &node.data {
        NodeData::Pond(pond) => {
            let bottom = stormwaves_core::calc::pond_bottom(pond);
            let peak = match &r.detail {
                Detail::Pond(d) => d.peak_elevation,
                _ => bottom,
            };
            let top = pond.storage.top().unwrap_or(bottom).max(peak).max(pond.devices.iter().map(|d| d.invert).fold(bottom, f64::max) + 1.0);
            let n = 80;
            let elevs: Vec<f64> = (0..=n).map(|i| bottom + (top - bottom) * i as f64 / n as f64).collect();
            cols.push(elevs.iter().map(|e| u.len(*e)).collect());
            if st.tab == Tab::Discharge {
                let tw = |rt: outlets::Routing| match rt {
                    outlets::Routing::Primary => fixed_tw(pond.tailwater[0]),
                    outlets::Routing::Secondary => fixed_tw(pond.tailwater[1]),
                    outlets::Routing::Tertiary => fixed_tw(pond.tailwater[2]),
                    _ => None,
                };
                let flows: Vec<outlets::OutletFlows> = elevs.iter().map(|e| outlets::evaluate(&pond.devices, *e, &tw, &pond.storage, &vec![true; pond.devices.len()])).collect();
                let names = ["Primary", "Secondary", "Tertiary", "Discarded"];
                let mut total = Series { name: "Total".into(), color: COLORS[1], x: flows.iter().map(|f| u.flow(f.total())).collect(), y: elevs.iter().map(|e| u.len(*e)).collect(), peak_label: None };
                total.x.iter_mut().for_each(|v| *v = v.max(0.0));
                heads.push(format!("Total ({})", u.flow_label()));
                cols.push(total.x.clone());
                series.push(total);
                for k in 0..4 {
                    let vals: Vec<f64> = flows.iter().map(|f| u.flow([f.primary, f.secondary, f.tertiary, f.discarded][k])).collect();
                    if vals.iter().any(|v| *v > 0.0) {
                        heads.push(format!("{} ({})", names[k], u.flow_label()));
                        cols.push(vals.clone());
                        series.push(Series { name: names[k].into(), color: COLORS[2 + k], x: vals, y: elevs.iter().map(|e| u.len(*e)).collect(), peak_label: None });
                    }
                }
                for (i, d) in pond.devices.iter().enumerate() {
                    let name = if d.description.is_empty() { d.kind.label().to_string() } else { d.description.clone() };
                    marks.push((u.len(d.invert), format!("#{} {}", i + 1, name)));
                }
            } else {
                let stor: Vec<f64> = elevs.iter().map(|e| u.volume(pond.storage.storage(*e))).collect();
                heads.push(format!("Storage ({})", u.volume_label()));
                cols.push(stor.clone());
                series.push(Series { name: "Storage".into(), color: COLORS[2], x: stor, y: elevs.iter().map(|e| u.len(*e)).collect(), peak_label: None });
                if let Some(areas) = elevs.iter().map(|e| pond.storage.surface_area(*e)).collect::<Option<Vec<f64>>>() {
                    heads.push(format!("Surface ({})", u.area_label()));
                    cols.push(areas.iter().map(|a| u.area(*a)).collect());
                }
                if let Some(f) = pond.flood_elevation {
                    marks.push((u.len(f), "Flood Elev".into()));
                }
                marks.push((u.len(peak), "Peak Elev".into()));
            }
        }
        NodeData::Reach(reach) => {
            let full = reach.section.full_depth();
            let n = 60;
            let depths: Vec<f64> = (0..=n).map(|i| full * 1.25 * i as f64 / n as f64).collect();
            cols.push(depths.iter().map(|y| u.len(reach.inlet_invert + y)).collect());
            let q: Vec<f64> = depths.iter().map(|y| u.flow(reach.section.manning_q(y.min(if matches!(reach.section, stormwaves_core::geometry::Section::Pipe { .. }) { full } else { f64::INFINITY }), reach.n, reach.slope()))).collect();
            heads.push(format!("Discharge ({})", u.flow_label()));
            cols.push(q.clone());
            series.push(Series { name: "Discharge".into(), color: COLORS[2], x: q, y: depths.iter().map(|y| u.len(reach.inlet_invert + y)).collect(), peak_label: None });
            marks.push((u.len(reach.inlet_invert + full), "Bank Full".into()));
        }
        _ => {}
    }
    if st.table {
        data_table(ui, &heads, &cols, size.y, false);
    } else {
        let (title, x_label) = if st.tab == Tab::Discharge {
            ("Stage-Discharge", format!("Discharge ({})", u.flow_label()))
        } else {
            ("Stage-Area-Storage", format!("Storage ({})", u.volume_label()))
        };
        let y_label = format!("Elevation ({})", if u.metric() { "meters" } else { "feet" });
        let spec = ChartSpec { title, subtitle: &node.title(), x_label: &x_label, y_label: &y_label, series: &series, summary: &[], three_d: false, marks: &marks };
        chart::chart(ui, &spec, &mut st.view, size);
    }
}

fn fixed_tw(t: stormwaves_core::project::Tailwater) -> Option<f64> {
    match t {
        stormwaves_core::project::Tailwater::Fixed(e) => Some(e),
        _ => None,
    }
}

fn events_tab(ui: &mut Ui, project: &Project, node: &Node, all: &[stormwaves_core::calc::Results], size: egui::Vec2) {
    let u = &project.units;
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)));
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        egui::Grid::new("events").spacing(vec2(22.0, 4.0)).show(ui, |ui| {
            let is_sub = node.node_type() == NodeType::Subcat;
            let heads: Vec<String> = if is_sub {
                vec!["Event".into(), format!("Rainfall ({})", u.rain_heading()), format!("Runoff ({})", u.flow_label()), format!("Volume ({})", u.volume_heading()), format!("Depth ({})", u.rain_heading())]
            } else {
                vec![
                    "Event".into(),
                    format!("Inflow ({})", u.flow_label()),
                    format!("Outflow ({})", u.flow_label()),
                    format!("Primary ({})", u.flow_label()),
                    format!("Secondary ({})", u.flow_label()),
                    format!("Elevation ({})", if u.metric() { "m" } else { "feet" }),
                    format!("Storage ({})", u.volume_heading()),
                ]
            };
            for h in &heads {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            if is_sub {
                for (r, e) in all.iter().zip(&project.settings.events) {
                    let Some(n) = r.node(node.id) else { continue };
                    let area = match &n.detail {
                        Detail::Subcat(d) => d.composite.total_area,
                        _ => 0.0,
                    };
                    ui.label(&e.name);
                    ui.label(format!("{:.2}", u.rain(e.depth)));
                    ui.label(group(u.flow(n.outflow.peak().value), u.flow_dp()));
                    ui.label(group(u.volume(n.outflow.volume_cf()), u.volume_dp()));
                    ui.label(format!("{:.2}", if area > 0.0 { u.rain(n.outflow.volume_cf() / area * 12.0) } else { 0.0 }));
                    ui.end_row();
                }
            } else {
                for row in report::events_table(project, all, node.id) {
                    for c in row.iter() {
                        ui.label(c);
                    }
                    ui.end_row();
                }
            }
        });
    });
}

// ---------------------------------------------------------------- project reports

#[derive(Default)]
pub struct ProjectReportState {
    pub tab: usize,
}

pub fn show_project_report(ctx: &egui::Context, doc: &mut Doc, st: &mut ProjectReportState) -> bool {
    let mut open = true;
    crate::frame::Win::new(format!("Project Reports - {}", doc.project.title)).open(&mut open).default_size(vec2(760.0, 460.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, name) in ["Node Listing", "Area Listing", "Soil Listing", "Events", "Notes"].iter().enumerate() {
                if tab_button(ui, name, st.tab == i) {
                    st.tab = i;
                }
            }
        });
        ui.add_space(4.0);
        let lines = match st.tab {
            0 => {
                let mut l = vec![doc.results.event_heading.clone(), String::new()];
                l.extend(report::node_listing(&doc.project, &doc.results));
                l
            }
            1 => report::area_listing(&doc.project),
            2 => report::soil_listing(&doc.project),
            3 => {
                let u = doc.project.units;
                let mut l = vec![format!("{:<16} {:<28} {:>10} {:>10} {:>6} {:>4}", "Event", "Storm Type", "Duration", "Depth", "B/B", "AMC")];
                for e in &doc.project.settings.events {
                    l.push(format!(
                        "{:<16} {:<28} {:>10} {:>10} {:>6} {:>4}",
                        e.name,
                        e.storm,
                        match e.duration_mode {
                            stormwaves_core::rainfall::DurationMode::Default => "Default".to_string(),
                            _ => format!("{:.2} hrs", e.duration),
                        },
                        u.fmt_rain(e.depth),
                        e.back_to_back,
                        e.amc.number()
                    ));
                }
                l
            }
            _ => {
                let mut l: Vec<String> = doc.project.notes.lines().map(String::from).collect();
                for n in &doc.project.nodes {
                    if !n.notes.trim().is_empty() {
                        l.push(String::new());
                        l.push(n.title());
                        l.extend(n.notes.lines().map(String::from));
                    }
                }
                l
            }
        };
        let h = ui.available_height().max(120.0);
        text_area(ui, &lines, h);
    });
    open
}

// ---------------------------------------------------------------- comparison

pub struct CompareState {
    pub nodes: Vec<NodeId>,
    pub what: usize,
    pub three_d: bool,
    pub view: ChartView,
}

impl CompareState {
    pub fn new(nodes: Vec<NodeId>) -> Self {
        CompareState { nodes, what: 1, three_d: true, view: ChartView::default() }
    }
}

pub fn show_compare(ctx: &egui::Context, doc: &mut Doc, st: &mut CompareState) -> bool {
    let mut open = true;
    if st.nodes.is_empty() {
        st.nodes = doc.selection.clone();
    }
    crate::frame::Win::new("Comparison Report").open(&mut open).default_size(vec2(620.0, 420.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let opts = vec![(0usize, "Inflow".to_string()), (1, "Outflow".to_string()), (2, "Primary".to_string()), (3, "Secondary".to_string())];
            widgets::combo(ui, "cmp-what", &mut st.what, &opts, 120.0);
            if widgets::button(ui, if st.three_d { "2D" } else { "3D" }, 50.0).clicked() {
                st.three_d = !st.three_d;
            }
            if widgets::button(ui, "Use Selection", 100.0).clicked() {
                st.nodes = doc.selection.clone();
            }
        });
        let u = doc.project.units;
        let mut series = Vec::new();
        for (i, id) in st.nodes.iter().enumerate() {
            let (Some(n), Some(r)) = (doc.project.node(*id), doc.results.node(*id)) else { continue };
            let h = match st.what {
                0 => &r.inflow,
                1 => &r.outflow,
                2 => &r.routes[0],
                _ => &r.routes[1],
            };
            series.push(Series {
                name: n.label(),
                color: COLORS[i % COLORS.len()],
                x: (0..h.len()).map(|k| h.time(k)).collect(),
                y: h.q.iter().map(|q| u.flow(*q)).collect(),
                peak_label: Some(format!("{} {}", group(u.flow(h.peak().value), u.flow_dp()), u.flow_label())),
            });
        }
        if series.is_empty() {
            ui.label("Select two or more nodes on the diagram, then click Use Selection.");
            return;
        }
        let title = ["Inflow", "Outflow", "Primary", "Secondary"][st.what.min(3)];
        let y = format!("Flow ({})", u.flow_label());
        let spec = ChartSpec { title: &format!("{title} Hydrograph Comparison"), subtitle: "", x_label: "Time (hours)", y_label: &y, series: &series, summary: &[], three_d: st.three_d, marks: &[] };
        let size = ui.available_size();
        chart::chart(ui, &spec, &mut st.view, size);
    });
    open
}

// ---------------------------------------------------------------- messages

pub fn show_messages(ctx: &egui::Context, doc: &mut Doc, actions: &mut Vec<WindowAction>) -> bool {
    let mut open = true;
    let t = Tokens::get(ctx);
    crate::frame::Win::new(format!("Messages - {}", doc.project.title)).open(&mut open).default_size(vec2(560.0, 240.0)).resizable(true).show(ctx, |ui| {
        let lines = report::message_lines(&doc.project, &doc.results);
        let h = (ui.available_height() - 30.0).max(80.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        widgets::sunken(ui.painter(), rect, Color32::from_rgb(255, 255, 255), &t);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(4.0)));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            for (m, (sev, line)) in doc.results.messages.iter().zip(lines) {
                let color = match sev {
                    Severity::Note => t.note,
                    Severity::Hint => t.hint,
                    Severity::Warning => t.warning,
                    Severity::Error => t.error,
                };
                let resp = ui.add(egui::Label::new(RichText::new(line).monospace().color(color)).sense(Sense::click()));
                if resp.clicked() {
                    if let Some(id) = m.node {
                        doc.selection = vec![id];
                    }
                }
                if resp.double_clicked() {
                    if let Some(id) = m.node {
                        actions.push(WindowAction::OpenReport(id));
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            if widgets::button(ui, "Copy", 70.0).clicked() {
                let text: Vec<String> = report::message_lines(&doc.project, &doc.results).into_iter().map(|(_, l)| l).collect();
                ui.ctx().copy_text(text.join("\n"));
            }
        });
    });
    open
}

// ---------------------------------------------------------------- storms and unit hydrographs

pub struct CurveState {
    pub storms: bool,
    pub checked: Vec<String>,
    pub view: ChartView,
    pub intensity: bool,
}

impl CurveState {
    pub fn storms() -> Self {
        CurveState { storms: true, checked: vec!["Type II 24-hr".into()], view: ChartView::default(), intensity: false }
    }
    pub fn uhs() -> Self {
        CurveState { storms: false, checked: vec!["SCS".into(), "Gamma-284".into()], view: ChartView::default(), intensity: true }
    }
}

pub fn show_curves(ctx: &egui::Context, doc: &mut Doc, st: &mut CurveState) -> bool {
    let mut open = true;
    let title = if st.storms { "Storm Distribution Report" } else { "Unit Hydrograph Report" };
    crate::frame::Win::new(title).open(&mut open).default_size(vec2(720.0, 440.0)).resizable(true).show(ctx, |ui| {
        let names: Vec<String> = if st.storms {
            let mut v: Vec<String> = stormwaves_core::rainfall::builtin_storms().into_iter().map(|s| s.name).collect();
            v.extend(doc.project.storms.iter().map(|s| s.name.clone()));
            v
        } else {
            let mut v: Vec<String> = stormwaves_core::uh::builtin_uhs().into_iter().map(|u| u.name).collect();
            v.extend(doc.project.uhs.iter().map(|u| u.name.clone()));
            v
        };
        ui.horizontal(|ui| {
            if st.storms {
                if widgets::radio(ui, !st.intensity, "Depth").clicked() {
                    st.intensity = false;
                }
                if widgets::radio(ui, st.intensity, "Intensity").clicked() {
                    st.intensity = true;
                }
            }
        });
        let avail = ui.available_size();
        ui.horizontal(|ui| {
            let mut series = Vec::new();
            for (i, name) in st.checked.iter().enumerate() {
                let color = COLORS[(i + 2) % COLORS.len()];
                if st.storms {
                    if let Some(s) = doc.project.storm(name) {
                        let n = 400;
                        let xs: Vec<f64> = (0..=n).map(|k| s.duration * k as f64 / n as f64).collect();
                        let ys: Vec<f64> = if st.intensity {
                            let h = s.duration / n as f64;
                            xs.iter().map(|x| (s.fraction_at(x + h / 2.0) - s.fraction_at(x - h / 2.0)) / h).collect()
                        } else {
                            xs.iter().map(|x| s.fraction_at(*x)).collect()
                        };
                        series.push(Series { name: name.clone(), color, x: xs, y: ys, peak_label: None });
                    }
                } else if let Some(u) = doc.project.unit_hydrograph(name) {
                    let k = u.peak_factor() / stormwaves_core::uh::CFS_PER_SQMI_INCH_PER_HR;
                    let xs: Vec<f64> = (0..=200).map(|j| j as f64 * u.base().min(8.0) / 200.0).collect();
                    let ys = xs.iter().map(|x| u.ordinate(*x) * k).collect();
                    series.push(Series { name: name.clone(), color, x: xs, y: ys, peak_label: None });
                }
            }
            let (title, xl, yl) = if st.storms {
                if st.intensity { ("Rainfall Intensity vs. Time", "Elapsed Time (hours)", "Fraction of Depth per Hour") } else { ("Rainfall Depth vs. Time", "Elapsed Time (hours)", "Fractional Rainfall Depth") }
            } else {
                ("Unit Hydrograph Intensity vs. Time", "Relative Time (Tp)", "Relative Intensity (K)")
            };
            let spec = ChartSpec { title, subtitle: "", x_label: xl, y_label: yl, series: &series, summary: &[], three_d: false, marks: &[] };
            chart::chart(ui, &spec, &mut st.view, vec2((avail.x - 210.0).max(200.0), avail.y.max(200.0)));
            ui.vertical(|ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().max_height(avail.y).show(ui, |ui| {
                    for name in names {
                        let mut on = st.checked.contains(&name);
                        if widgets::check(ui, &mut on, &name).changed() {
                            if on {
                                st.checked.push(name.clone());
                            } else {
                                st.checked.retain(|c| c != &name);
                            }
                        }
                    }
                });
            });
        });
    });
    open
}
