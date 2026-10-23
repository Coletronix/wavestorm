//! Report windows: node reports, project listings, comparisons, messages and curve viewers.

use crate::chart::{self, ChartSpec, ChartView, Series};
use crate::doc::Doc;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, Ui, vec2};
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
    Sizing,
    Profile,
    /// TR-55 graphical peak discharge worksheet (subcatchments).
    Tr55,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Summary => "Summary",
            Tab::Hydrograph => "Hydrograph",
            Tab::Discharge => "Discharge",
            Tab::Storage => "Storage",
            Tab::Events => "Events",
            Tab::Sizing => "Sizing",
            Tab::Profile => "Profile",
            Tab::Tr55 => "TR-55",
        }
    }
}

pub struct ReportState {
    pub node: NodeId,
    pub tab: Tab,
    pub table: bool,
    pub three_d: bool,
    pub shrink: bool,
    /// Tabular hydrograph shows each inflow hydrograph as its own column.
    pub inflows: bool,
    pub view: ChartView,
    pub open: bool,
}

impl ReportState {
    pub fn new(node: NodeId) -> Self {
        ReportState { node, tab: Tab::Hydrograph, table: false, three_d: true, shrink: true, inflows: false, view: ChartView::default(), open: true }
    }
}

fn tabs_for(t: NodeType) -> Vec<Tab> {
    match t {
        NodeType::Pond => vec![Tab::Summary, Tab::Hydrograph, Tab::Discharge, Tab::Storage, Tab::Events, Tab::Sizing, Tab::Profile],
        NodeType::Reach => vec![Tab::Summary, Tab::Hydrograph, Tab::Discharge, Tab::Events, Tab::Profile],
        NodeType::Subcat => vec![Tab::Summary, Tab::Hydrograph, Tab::Events, Tab::Profile, Tab::Tr55],
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

pub(crate) const COLORS: [Color32; 6] = [
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
    if r.undefined.is_some() {
        out.push("RESULTS UNDEFINED".to_string());
    }
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
            if r.undefined.is_some() {
                out.push("Peak Elev=above defined storage".to_string());
            } else {
                out.push(format!("Peak Elev={}", u.fmt_elev(d.peak_elevation)));
                out.push(format!("Storage={}", u.fmt_volume(d.peak_storage)));
            }
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

/// Draws a node profile (side view) fitted to `size`; the vertical scale is exaggerated
/// independently of the horizontal one.
fn profile_view(ui: &mut Ui, prof: &stormwaves_core::profile::Profile, size: egui::Vec2) {
    use stormwaves_core::profile::{Anchor, Ink};
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let Some([x0, y0, x1, y1]) = prof.bounds() else { return };
    let inner = egui::Rect::from_min_max(rect.min + vec2(16.0, 36.0), rect.max - vec2(16.0, 16.0));
    let (w, h) = ((x1 - x0).max(1e-6), (y1 - y0).max(1e-6));
    let map = |[x, y]: [f64; 2]| egui::pos2(inner.left() + ((x - x0) / w) as f32 * inner.width(), inner.bottom() - ((y - y0) / h) as f32 * inner.height());
    let dark = t.kind == crate::theme::ThemeKind::ModernDark;
    let water = if dark { Color32::from_rgb(0, 150, 170) } else { Color32::from_rgb(0, 235, 245) };
    let painter = ui.painter().with_clip_rect(rect.shrink(2.0));
    for s in &prof.shapes {
        let pts: Vec<egui::Pos2> = s.points.iter().map(|p| map(*p)).collect();
        let color = match s.ink {
            Ink::FlowPath => Color32::from_rgb(30, 60, 220),
            Ink::Structure => t.text,
            Ink::Water | Ink::Outlet => water,
            Ink::Device => Color32::from_rgb(220, 30, 30),
        };
        if s.filled {
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::new(1.0, t.text)));
        } else {
            let width = if s.ink == Ink::Device { 2.5 } else { 2.0 };
            painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
        }
    }
    // Labels that would overlap (e.g. devices at the same invert) are moved up until clear.
    let mut used: Vec<egui::Rect> = Vec::new();
    for l in &prof.labels {
        let (align, offset) = match l.anchor {
            Anchor::Left => (egui::Align2::LEFT_BOTTOM, vec2(2.0, -2.0)),
            Anchor::Right => (egui::Align2::RIGHT_CENTER, vec2(-2.0, 0.0)),
            Anchor::Below => (egui::Align2::LEFT_TOP, vec2(6.0, 4.0)),
        };
        let font = egui::FontId::proportional(if l.title { 13.0 } else { 11.0 });
        let text = if l.title { format!("Profile for {}", l.text) } else { l.text.clone() };
        let galley = painter.layout_no_wrap(text, font, t.text);
        let mut at = map(l.at) + offset;
        let mut r = align.anchor_size(at, galley.size());
        for _ in 0..20 {
            if !used.iter().any(|u| u.intersects(r)) {
                break;
            }
            at.y -= galley.size().y;
            r = align.anchor_size(at, galley.size());
        }
        used.push(r);
        painter.galley(r.min, galley, t.text);
    }
}

/// A sunken, scrolling table of numbers. The returned response takes right-clicks for menus.
pub(crate) fn data_table(ui: &mut Ui, heads: &[String], cols: &[Vec<f64>], height: f32, bold_max: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(4.0)));
    // Plain labels, so a right-click anywhere on the table reaches its context menu.
    child.style_mut().interaction.selectable_labels = false;
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
    response
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
    crate::frame::Win::new(title).id(egui::Id::new(("report", st.node))).open(&mut open).default_size(vec2(640.0, 430.0)).min_size(vec2(420.0, 300.0)).resizable(true).min_max().show(ctx, |ui| {
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
                // The classic report buttons: Table, 2D (graphs) or Shrink (tables), Print, Export,
                // Edit, Help.
                ui.spacing_mut().item_spacing.y = 4.0;
                let charty = matches!(st.tab, Tab::Hydrograph | Tab::Discharge | Tab::Storage | Tab::Sizing);
                if widgets::toggle_button(ui, "Table", 62.0, st.table && charty, charty).clicked() {
                    st.table = !st.table;
                }
                if !charty || st.table {
                    // Text reports (Summary, Events, Profile) show it disabled, as the reference does.
                    if widgets::toggle_button(ui, "Shrink", 62.0, st.shrink && charty, charty).clicked() {
                        st.shrink = !st.shrink;
                    }
                } else if widgets::button(ui, if st.three_d { "2D" } else { "3D" }, 62.0).clicked() {
                    st.three_d = !st.three_d;
                }
                if widgets::button(ui, "Print", 62.0).on_hover_text("Print this node's report (PDF)").clicked() {
                    actions.push(WindowAction::PrintPdf(stormwaves_core::printing::PrintOptions {
                        diagram: false,
                        node_listing: false,
                        area_listing: false,
                        summaries: true,
                        graphs: true,
                        tables: st.table,
                        date: crate::dialogs::misc::today(),
                        only: vec![node.id],
                    }));
                }
                let export = widgets::button(ui, "Export", 62.0);
                egui::Popup::menu(&export).show(|ui| {
                    let summary = || report::node_summary(&doc.project, &doc.results, &node).join("\n");
                    if ui.button("Copy Summary to Clipboard").clicked() {
                        ui.ctx().copy_text(summary());
                        actions.push(WindowAction::Status("Summary copied to the clipboard".into()));
                    }
                    if ui.button("Save Summary as Text...").clicked() {
                        actions.push(WindowAction::SaveFile { name: format!("{}.txt", node.label()), bytes: summary().into_bytes(), ext: "txt" });
                    }
                    if ui.button("Save Graph as EMF...").clicked() {
                        if let Some(bytes) = stormwaves_core::printing::node_graph_emf(&doc.project, &doc.results, node.id) {
                            actions.push(WindowAction::SaveFile { name: format!("{} graph.emf", node.label()), bytes, ext: "emf" });
                        }
                    }
                    if ui.button("Save Summary as EMF...").clicked() {
                        if let Some(bytes) = stormwaves_core::printing::node_summary_emf(&doc.project, &doc.results, node.id) {
                            actions.push(WindowAction::SaveFile { name: format!("{} summary.emf", node.label()), bytes, ext: "emf" });
                        }
                    }
                    if ui.button("Save Hydrograph as CSV...").clicked() {
                        let (heads, cols) = report::hydrograph_table(&doc.project, &doc.results, &node, false, st.inflows);
                        let mut csv = heads.join(",");
                        for i in 0..cols.first().map_or(0, Vec::len) {
                            csv.push('\n');
                            csv.push_str(&cols.iter().map(|c| c.get(i).map_or(String::new(), |v| format!("{v}"))).collect::<Vec<_>>().join(","));
                        }
                        actions.push(WindowAction::SaveFile { name: format!("{} hydrograph.csv", node.label()), bytes: csv.into_bytes(), ext: "csv" });
                    }
                });
                if widgets::button(ui, "Edit", 62.0).clicked() {
                    actions.push(WindowAction::OpenEdit(node.id));
                }
                if widgets::button(ui, "Help", 62.0).clicked() {
                    actions.push(WindowAction::Help(match node.node_type() {
                        NodeType::Subcat => "runoff",
                        NodeType::Reach => "reach",
                        NodeType::Pond => "pond",
                        _ => "link",
                    }));
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
            let section = match (&node.data, &r.detail) {
                (NodeData::Reach(reach), Detail::Reach(d)) if crate::section_view::drawable(&reach.section) => Some((&reach.section, d.peak_depth)),
                _ => None,
            };
            match section {
                Some((sec, depth)) => {
                    let t = Tokens::get(ui.ctx());
                    let sec_h = (size.y / 3.0).min(170.0).max(60.0);
                    text_area(ui, &lines, (size.y - sec_h - 4.0).max(40.0));
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), sec_h), Sense::hover());
                    widgets::sunken(ui.painter(), rect, t.field, &t);
                    let water = if t.kind == crate::theme::ThemeKind::ModernDark { Color32::from_rgb(0, 150, 170) } else { Color32::from_rgb(95, 215, 230) };
                    let painter = ui.painter().with_clip_rect(rect.shrink(2.0));
                    crate::section_view::draw(&painter, rect.shrink(4.0), sec, Some(depth), t.text, water, 1.5);
                }
                None => text_area(ui, &lines, size.y),
            }
        }
        Tab::Hydrograph => {
            if st.table {
                let (heads, cols) = report::hydrograph_table(project, &doc.results, node, st.shrink, st.inflows);
                let has_inflows = node.node_type() != NodeType::Subcat;
                data_table(ui, &heads, &cols, size.y, true).context_menu(|ui| {
                    if has_inflows && ui.checkbox(&mut st.inflows, "Inflows").on_hover_text("Show each inflow hydrograph").changed() {
                        ui.close();
                    }
                    if ui.checkbox(&mut st.shrink, "Shrink").on_hover_text("Show every n-th time step").changed() {
                        ui.close();
                    }
                });
            } else {
                let series = hydro_series(project, node, r);
                let summary = summary_lines(project, node, r, &doc.results.event_heading);
                let y_label = format!("Flow ({})", project.units.flow_label());
                let spec = ChartSpec { title: "Hydrograph", subtitle: &node.title(), x_label: "Time (hours)", y_label: &y_label, series: &series, summary: &summary, three_d: st.three_d, log_x: false, marks: &[] };
                chart::chart(ui, &spec, &mut st.view, size);
            }
        }
        Tab::Discharge | Tab::Storage => rating_tab(ui, project, node, r, st, size),
        Tab::Events => {
            let all = doc.all_results.as_deref().unwrap_or(&[]);
            events_tab(ui, project, node, all, size);
        }
        Tab::Sizing => sizing_tab(ui, project, node, r, st, size),
        Tab::Tr55 => {
            let lines = match &r.detail {
                Detail::Subcat(d) => stormwaves_core::tr55::worksheet_report(d.composite.total_area, d.composite.cn, d.tc_minutes / 60.0, &project.settings.events),
                _ => vec!["Not available.".into()],
            };
            text_area(ui, &lines, size.y);
        }
        Tab::Profile => {
            let prof = stormwaves_core::profile::chain(project, Some(&doc.results), &[node.id]);
            profile_view(ui, &prof, size);
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
                let flows: Vec<outlets::OutletFlows> = elevs.iter().map(|e| outlets::evaluate(&pond.devices, *e, &tw, &pond.storage, &vec![outlets::DeviceState { active: true, since: Some(0.0) }; pond.devices.len()], 1.0e6)).collect();
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
        let _ = data_table(ui, &heads, &cols, size.y, false);
    } else {
        let (title, x_label) = if st.tab == Tab::Discharge {
            ("Stage-Discharge", format!("Discharge ({})", u.flow_label()))
        } else {
            ("Stage-Area-Storage", format!("Storage ({})", u.volume_label()))
        };
        let y_label = format!("Elevation ({})", if u.metric() { "meters" } else { "feet" });
        let spec = ChartSpec { title, subtitle: &node.title(), x_label: &x_label, y_label: &y_label, series: &series, summary: &[], three_d: false, log_x: false, marks: &marks };
        chart::chart(ui, &spec, &mut st.view, size);
    }
}

fn sizing_tab(ui: &mut Ui, project: &Project, node: &Node, r: &NodeResult, st: &mut ReportState, size: egui::Vec2) {
    let u = &project.units;
    let curve = stormwaves_core::routing::pond_sizing(&r.inflow);
    if curve.is_empty() {
        ui.label("No inflow: the sizing estimate needs an inflow hydrograph.");
        return;
    }
    let q: Vec<f64> = curve.iter().map(|c| u.flow(c.0)).collect();
    let v: Vec<f64> = curve.iter().map(|c| u.volume(c.1)).collect();
    if st.table {
        let heads = vec![format!("Discharge ({})", u.flow_label()), format!("Storage ({})", u.volume_label())];
        let step = (curve.len() / 40).max(1);
        let cols = vec![q.iter().step_by(step).copied().collect(), v.iter().step_by(step).copied().collect()];
        let _ = data_table(ui, &heads, &cols, size.y, false);
    } else {
        let series = vec![Series { name: "Storage".into(), color: COLORS[2], x: v, y: q, peak_label: None }];
        let summary = vec![format!("Peak Inflow={}", u.fmt_flow(r.inflow.peak().value)), format!("Inflow Volume={}", u.fmt_volume(r.inflow.volume_cf()))];
        let xl = format!("Storage ({})", u.volume_label());
        let yl = format!("Discharge ({})", u.flow_label());
        let spec = ChartSpec { title: "Pond Sizing Estimate", subtitle: &node.title(), x_label: &xl, y_label: &yl, series: &series, summary: &summary, three_d: false, log_x: false, marks: &[] };
        chart::chart(ui, &spec, &mut st.view, size);
    }
}

fn fixed_tw(t: stormwaves_core::project::Tailwater) -> Option<f64> {
    match t {
        stormwaves_core::project::Tailwater::Fixed(e) => Some(e),
        _ => None,
    }
}

/// A classic report table in a sunken box: two-line headers (title over units), a rule, text in
/// the first `text_cols` columns left-aligned and numbers right-aligned, and an optional bold row
/// (the largest event, or a total).
fn report_table(ui: &mut Ui, size: egui::Vec2, heads: &[(String, String)], rows: &[Vec<String>], text_cols: usize, bold: Option<usize>) {
    report_table_with(ui, size, heads, rows, &|c| c < text_cols, bold);
}

/// [`report_table`] with any set of text (left-aligned) columns.
fn report_table_with(ui: &mut Ui, size: egui::Vec2, heads: &[(String, String)], rows: &[Vec<String>], is_text: &dyn Fn(usize) -> bool, bold: Option<usize>) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    widgets::sunken(ui.painter(), rect, t.field, &t);
    let font = FontId::proportional(13.0);
    let measure = |ui: &Ui, s: &str| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), t.text).size().x);
    let ncols = heads.len();
    let widths: Vec<f32> = (0..ncols)
        .map(|c| {
            let h = heads.get(c).map_or(0.0f32, |(a, b)| measure(ui, a).max(measure(ui, b)));
            rows.iter().filter_map(|r| r.get(c)).fold(h, |w, s| w.max(measure(ui, s))) + 26.0
        })
        .collect();
    let row_h = 19.0;
    let total = vec2(widths.iter().sum::<f32>() + 16.0, row_h * (rows.len() as f32 + 2.6) + 8.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(3.0)));
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        let (area, _) = ui.allocate_exact_size(total, Sense::hover());
        let p = ui.painter();
        let cell = |row_y: f32, c: usize, text: &str, strong: bool| {
            let x0 = area.left() + 8.0 + widths[..c].iter().sum::<f32>();
            let (pos, align) = if is_text(c) { (egui::pos2(x0 + 4.0, row_y), Align2::LEFT_TOP) } else { (egui::pos2(x0 + widths[c] - 4.0, row_y), Align2::RIGHT_TOP) };
            if strong {
                // Bold by over-striking, as the proportional UI font has no bold face.
                p.text(pos + vec2(0.6, 0.0), align, text, font.clone(), t.text);
            }
            p.text(pos, align, text, font.clone(), t.text);
        };
        let y0 = area.top() + 4.0;
        for (c, (a, b)) in heads.iter().enumerate() {
            cell(y0, c, a, false);
            cell(y0 + row_h, c, b, false);
        }
        let rule_y = y0 + 2.0 * row_h + 3.0;
        p.line_segment([egui::pos2(area.left() + 8.0, rule_y), egui::pos2(area.left() + 8.0 + widths.iter().sum::<f32>(), rule_y)], Stroke::new(1.0, t.text));
        for (i, r) in rows.iter().enumerate() {
            let y = rule_y + 5.0 + i as f32 * row_h;
            for (c, s) in r.iter().enumerate().take(ncols) {
                cell(y, c, s, bold == Some(i));
            }
        }
    });
}

fn events_tab(ui: &mut Ui, project: &Project, node: &Node, all: &[stormwaves_core::calc::Results], size: egui::Vec2) {
    let u = &project.units;
    let h = |a: &str, b: String| (a.to_string(), format!("({b})"));
    let elev_unit = if u.metric() { "meters" } else { "feet" }.to_string();
    let is_sub = node.node_type() == NodeType::Subcat;
    let (heads, rows, peaks): (Vec<(String, String)>, Vec<Vec<String>>, Vec<f64>) = if is_sub {
        let heads = vec![("Event".to_string(), String::new()), h("Rainfall", u.rain_heading().into()), h("Runoff", u.flow_label().into()), h("Volume", u.volume_heading().into()), h("Depth", u.rain_heading().into())];
        let mut rows = Vec::new();
        let mut peaks = Vec::new();
        for (r, e) in all.iter().zip(&project.settings.events) {
            let Some(n) = r.node(node.id) else { continue };
            let area = match &n.detail {
                Detail::Subcat(d) => d.composite.total_area,
                _ => 0.0,
            };
            let q = n.outflow.peak().value;
            peaks.push(q);
            rows.push(vec![
                e.name.clone(),
                format!("{:.2}", u.rain(e.depth)),
                group(u.flow(q), u.flow_dp()),
                group(u.volume(n.outflow.volume_cf()), u.volume_dp()),
                format!("{:.2}", if area > 0.0 { u.rain(n.outflow.volume_cf() / area * 12.0) } else { 0.0 }),
            ]);
        }
        (heads, rows, peaks)
    } else {
        let flow = u.flow_label().to_string();
        let heads = vec![
            ("Event".to_string(), String::new()),
            h("Inflow", flow.clone()),
            h("Outflow", flow.clone()),
            h("Primary", flow.clone()),
            h("Secondary", flow),
            h("Elevation", elev_unit),
            h("Storage", u.volume_heading().into()),
        ];
        let rows: Vec<Vec<String>> = report::events_table(project, all, node.id).into_iter().map(|r| r.to_vec()).collect();
        let peaks = all.iter().filter_map(|r| r.node(node.id)).map(|n| n.outflow.peak().value).collect();
        (heads, rows, peaks)
    };
    // The event with the largest peak is shown in bold, as in the reference reports.
    let bold = peaks.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i);
    report_table(ui, size, &heads, &rows, 1, bold);
}

// ---------------------------------------------------------------- project reports

#[derive(Default)]
pub struct ProjectReportState {
    pub tab: usize,
    /// Distinguishes windows when several Project Reports are open at once.
    pub serial: u64,
    /// List only the selected nodes (otherwise all nodes).
    pub selected_only: bool,
    pub hide_totals: bool,
}

const PROJECT_TABS: [&str; 10] = ["Node Listing", "Area Listing", "Soil Listing", "Land-Use", "Pollutants", "Subcat Loading", "Project Loading", "Pipe Listing", "Events", "Notes"];

enum Listing {
    Text(Vec<String>),
    Table(stormwaves_core::listings::Table),
}

impl Listing {
    fn lines(&self) -> Vec<String> {
        match self {
            Listing::Text(l) => l.clone(),
            Listing::Table(t) => t.text_lines(),
        }
    }
}

pub fn show_project_report(ctx: &egui::Context, doc: &mut Doc, st: &mut ProjectReportState, actions: &mut Vec<WindowAction>) -> bool {
    use stormwaves_core::listings;
    let mut open = true;
    crate::frame::Win::new(format!("Project Reports - {}", doc.project.title)).id(egui::Id::new(("project-report", st.serial))).open(&mut open).default_size(vec2(820.0, 460.0)).resizable(true).min_max().show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, name) in PROJECT_TABS.iter().enumerate() {
                if tab_button(ui, name, st.tab == i) {
                    st.tab = i;
                }
            }
        });
        ui.add_space(4.0);
        let selection = doc.selection.clone();
        let only = st.selected_only.then_some(selection.as_slice());
        let totals = !st.hide_totals;
        let (p, r) = (&doc.project, &doc.results);
        let listing = match st.tab {
            0 => {
                let mut l = vec![doc.results.event_heading.clone(), String::new()];
                l.extend(report::node_listing(&doc.project, &doc.results));
                Listing::Text(l)
            }
            1 => Listing::Table(listings::area_table(p, only, totals)),
            2 => Listing::Table(listings::soil_table(p, only, totals)),
            3 => Listing::Table(listings::land_use_table(p, only, totals)),
            4 => Listing::Table(listings::pollutants_table(p)),
            5 => Listing::Table(listings::subcat_loading_table(p, r, only, totals)),
            6 => Listing::Table(listings::project_loading_table(p, r, only, totals)),
            7 => Listing::Table(listings::pipe_table(p, only)),
            8 => Listing::Text({
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
            }),
            _ => Listing::Text({
                let mut l: Vec<String> = doc.project.notes.lines().map(String::from).collect();
                for n in &doc.project.nodes {
                    if !n.notes.trim().is_empty() {
                        l.push(String::new());
                        l.push(n.title());
                        l.extend(n.notes.lines().map(String::from));
                    }
                }
                l
            }),
        };
        let name = PROJECT_TABS.get(st.tab).copied().unwrap_or("Listing");
        let col_key = ui.id().with("project-report-col");
        let body = vec2(widgets::body_width(ui, col_key, 104.0), (ui.available_height() - 6.0).max(120.0));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(body.x);
                match &listing {
                    Listing::Text(lines) => text_area(ui, lines, body.y),
                    Listing::Table(t) if t.rows.is_empty() => text_area(ui, &t.text_lines(), body.y),
                    Listing::Table(t) => report_table_with(ui, body, &t.heads, &t.rows, &|c| t.is_text(c), t.total),
                }
            });
            let col = ui.vertical(|ui| {
                // The reference's options sit above its usual button column.
                ui.spacing_mut().item_spacing.y = 4.0;
                if widgets::radio(ui, !st.selected_only, "All Nodes").clicked() {
                    st.selected_only = false;
                }
                if widgets::radio(ui, st.selected_only, "Selected Nodes").clicked() {
                    st.selected_only = true;
                }
                let mut show = !st.hide_totals;
                if widgets::check(ui, &mut show, "Show Totals").changed() {
                    st.hide_totals = !show;
                }
                ui.add_space(8.0);
                widgets::toggle_button(ui, "Table", 62.0, false, false);
                widgets::toggle_button(ui, "Shrink", 62.0, false, false);
                if widgets::button(ui, "Print", 62.0).on_hover_text("Print this listing (PDF)").clicked() {
                    let bytes = stormwaves_core::printing::lines_pdf(&doc.project, &doc.results, name, &listing.lines(), &crate::dialogs::misc::today());
                    actions.push(WindowAction::SaveFile { name: format!("{} - {name}.pdf", doc.project.title), bytes, ext: "pdf" });
                }
                let export = widgets::button(ui, "Export", 62.0);
                egui::Popup::menu(&export).show(|ui| {
                    if ui.button("Copy to Clipboard").clicked() {
                        ui.ctx().copy_text(listing.lines().join("\n"));
                        actions.push(WindowAction::Status(format!("{name} copied to the clipboard")));
                    }
                    if ui.button("Save as Text...").clicked() {
                        actions.push(WindowAction::SaveFile { name: format!("{name}.txt"), bytes: listing.lines().join("\n").into_bytes(), ext: "txt" });
                    }
                    if let Listing::Table(t) = &listing {
                        if ui.button("Save as CSV...").clicked() {
                            actions.push(WindowAction::SaveFile { name: format!("{name}.csv"), bytes: t.csv().into_bytes(), ext: "csv" });
                        }
                    }
                });
                widgets::button_enabled(ui, "Edit", 62.0, false);
                if widgets::button(ui, "Help", 62.0).clicked() {
                    actions.push(WindowAction::Help("project-reports"));
                }
            });
            widgets::remember_column(ui, col_key, col.response.rect);
        });
    });
    open
}

// ---------------------------------------------------------------- comparison

pub struct CompareState {
    pub nodes: Vec<NodeId>,
    /// Hydrograph shown: 0 inflow, 1 outflow, 2 primary, 3 secondary.
    pub what: usize,
    /// 0 Summary, 1 Hydrograph, 2 Profile.
    pub tab: usize,
    pub three_d: bool,
    pub table: bool,
    pub view: ChartView,
}

impl CompareState {
    pub fn new(nodes: Vec<NodeId>) -> Self {
        CompareState { nodes, what: 1, tab: 1, three_d: true, table: false, view: ChartView::default() }
    }
}

pub fn show_compare(ctx: &egui::Context, doc: &mut Doc, st: &mut CompareState, actions: &mut Vec<WindowAction>) -> bool {
    let mut open = true;
    if st.nodes.is_empty() {
        st.nodes = doc.selection.clone();
    }
    crate::frame::Win::new("Comparison Report").open(&mut open).default_size(vec2(680.0, 430.0)).resizable(true).min_max().show(ctx, |ui| {
        // Reference layout: report tabs at the left, the hydrograph selector at the right.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, name) in ["Summary", "Hydrograph", "Profile"].iter().enumerate() {
                if tab_button(ui, name, st.tab == i) {
                    st.tab = i;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let opts = vec![(0usize, "Inflow".to_string()), (1, "Outflow".to_string()), (2, "Primary".to_string()), (3, "Secondary".to_string())];
                widgets::combo(ui, "cmp-what", &mut st.what, &opts, 120.0);
            });
        });
        ui.add_space(4.0);
        let col_key = ui.id().with("compare-col");
        let body = vec2(widgets::body_width(ui, col_key, 62.0), (ui.available_height() - 6.0).max(150.0));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(body.x);
                compare_body(ui, doc, st, body);
            });
            let col = ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let graph = st.tab == 1;
                if widgets::toggle_button(ui, "Table", 62.0, st.table && graph, graph).clicked() {
                    st.table = !st.table;
                }
                if widgets::button_enabled(ui, if st.three_d { "2D" } else { "3D" }, 62.0, graph && !st.table).clicked() {
                    st.three_d = !st.three_d;
                }
                if widgets::button(ui, "Print", 62.0).clicked() {
                    let o = stormwaves_core::printing::PrintOptions {
                        diagram: false,
                        node_listing: false,
                        area_listing: false,
                        summaries: true,
                        graphs: true,
                        tables: false,
                        date: crate::dialogs::misc::today(),
                        only: st.nodes.clone(),
                    };
                    actions.push(WindowAction::PrintPdf(o));
                }
                let export = widgets::button(ui, "Export", 62.0);
                egui::Popup::menu(&export).show(|ui| {
                    if ui.button("Copy Summary to Clipboard").clicked() {
                        let rows = compare_rows(doc, st);
                        ui.ctx().copy_text(rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n"));
                    }
                });
                if widgets::button(ui, "Selection", 62.0).on_hover_text("Compare the nodes now selected on the diagram").clicked() {
                    st.nodes = doc.selection.clone();
                }
                if widgets::button(ui, "Help", 62.0).clicked() {
                    widgets::request_help(ui.ctx(), "print");
                }
            });
            widgets::remember_column(ui, col_key, col.response.rect);
        });
    });
    open
}

/// Peak values of each compared node (label, inflow, outflow, primary, secondary).
fn compare_rows(doc: &Doc, st: &CompareState) -> Vec<Vec<String>> {
    let u = doc.project.units;
    let f = |h: &stormwaves_core::hydrograph::Hydrograph| group(u.flow(h.peak().value), u.flow_dp());
    st.nodes
        .iter()
        .filter_map(|id| Some((doc.project.node(*id)?, doc.results.node(*id)?)))
        .map(|(n, r)| vec![n.title(), f(&r.inflow), f(&r.outflow), f(&r.routes[0]), f(&r.routes[1])])
        .collect()
}

fn compare_body(ui: &mut Ui, doc: &mut Doc, st: &mut CompareState, size: egui::Vec2) {
    if st.tab == 0 {
        let flow = doc.project.units.flow_label().to_string();
        let heads: Vec<(String, String)> = ["Node", "Inflow", "Outflow", "Primary", "Secondary"]
            .iter()
            .enumerate()
            .map(|(i, h)| (h.to_string(), if i == 0 { String::new() } else { format!("({flow})") }))
            .collect();
        let rows = compare_rows(doc, st);
        report_table(ui, size, &heads, &rows, 1, None);
        return;
    }
    if st.tab == 2 {
        // Side-by-side profile in the order the nodes were selected.
        let prof = stormwaves_core::profile::chain(&doc.project, Some(&doc.results), &st.nodes);
        if prof.shapes.is_empty() {
            ui.label("Select subcatchments, reaches or ponds in flow order, then click Selection.");
        } else {
            profile_view(ui, &prof, size);
        }
        return;
    }
    {
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
            ui.label("Select two or more nodes on the diagram, then click Selection.");
            return;
        }
        if st.table {
            let mut heads = vec!["Time (hours)".to_string()];
            heads.extend(series.iter().map(|s| format!("{} ({})", s.name, u.flow_label())));
            let mut cols = vec![series[0].x.clone()];
            cols.extend(series.iter().map(|s| s.y.clone()));
            let _ = data_table(ui, &heads, &cols, size.y, true);
            return;
        }
        let title = ["Inflow", "Outflow", "Primary", "Secondary"][st.what.min(3)];
        let y = format!("Flow ({})", u.flow_label());
        let spec = ChartSpec { title: &format!("{title} Hydrograph Comparison"), subtitle: "", x_label: "Time (hours)", y_label: &y, series: &series, summary: &[], three_d: st.three_d, log_x: false, marks: &[] };
        chart::chart(ui, &spec, &mut st.view, size);
    }
}

// ---------------------------------------------------------------- messages

pub fn show_messages(ctx: &egui::Context, doc: &mut Doc, actions: &mut Vec<WindowAction>) -> bool {
    let mut open = true;
    let t = Tokens::get(ctx);
    crate::frame::Win::new(format!("Messages - {}", doc.project.title)).open(&mut open).default_size(vec2(560.0, 240.0)).resizable(true).min_max().show(ctx, |ui| {
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

mod curves;
pub use curves::{CurveKind, CurveState, show_curves};
