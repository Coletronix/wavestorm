//! Storm, unit hydrograph and IDF viewers: Depth/Intensity radios, time increment, graph and
//! table tabs, and a tree of curves grouped by name prefix.

use super::{COLORS, data_table};
use crate::chart::{self, ChartSpec, ChartView, Series};
use crate::doc::Doc;
use crate::widgets;
use crate::windows::{Window, WindowAction};
use egui::{RichText, Ui, vec2};
use wavestorm_core::rainfall::Storm;
use wavestorm_core::uh::UnitHydrograph;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    Storm,
    Uh,
    Idf,
}

pub struct CurveState {
    pub kind: CurveKind,
    pub checked: Vec<String>,
    pub view: ChartView,
    /// Intensity (true) or depth (false).
    pub intensity: bool,
    /// Storms: scale by the current event's depth instead of showing fractions.
    pub current_depth: bool,
    /// Storms: one curve per event, each scaled to that event's depth.
    pub event_depths: bool,
    /// Time increment: hours (storms), Tp (unit hydrographs) or minutes (IDF).
    pub inc: f64,
    /// Graph (0) or Table (1).
    pub tab: usize,
    fresh: bool,
}

impl CurveState {
    pub fn storms() -> Self {
        CurveState {
            kind: CurveKind::Storm,
            checked: vec!["Type II 24-hr".into()],
            view: ChartView::default(),
            intensity: false,
            current_depth: false,
            event_depths: false,
            inc: 0.1,
            tab: 0,
            fresh: false,
        }
    }
    pub fn uhs() -> Self {
        CurveState {
            kind: CurveKind::Uh,
            checked: vec!["SCS".into(), "Gamma-284".into()],
            view: ChartView::default(),
            intensity: true,
            current_depth: false,
            event_depths: false,
            inc: 0.1,
            tab: 0,
            fresh: false,
        }
    }
    pub fn idf() -> Self {
        CurveState {
            kind: CurveKind::Idf,
            checked: Vec::new(),
            view: ChartView::default(),
            intensity: true,
            current_depth: false,
            event_depths: false,
            inc: 5.0,
            tab: 0,
            fresh: true,
        }
    }
}

/// Splits a curve name into its tree group: "Type II 24-hr" -> "Type II", "PA-Pittsburgh" -> "PA".
fn group_of(name: &str) -> Option<String> {
    let words: Vec<&str> = name.split(['-', '_', ' ']).filter(|w| !w.is_empty()).collect();
    let first = words.first()?;
    if first.chars().next().is_some_and(|c| c.is_ascii_digit()) || words.len() < 2 {
        return None;
    }
    if first.eq_ignore_ascii_case("type") {
        return Some(format!("{} {}", first, words[1]));
    }
    Some((*first).to_string())
}

/// Groups names by prefix, keeping first-appearance order; a group of one stays a plain entry.
fn grouped(names: &[String]) -> Vec<(Option<String>, Vec<String>)> {
    let mut out: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for n in names {
        let g = group_of(n);
        if let Some(g) = &g
            && let Some(slot) = out.iter_mut().find(|(k, _)| k.as_deref() == Some(g))
        {
            slot.1.push(n.clone());
            continue;
        }
        out.push((g, vec![n.clone()]));
    }
    for (g, members) in out.iter_mut() {
        if members.len() < 2 {
            *g = None;
        }
    }
    out
}

fn storm_value(s: &Storm, t: f64, intensity: bool, h: f64, scale: f64) -> f64 {
    if intensity {
        (s.fraction_at(t + h / 2.0) - s.fraction_at(t - h / 2.0)) / h * scale
    } else {
        s.fraction_at(t) * scale
    }
}

/// Cumulative fraction of the unit hydrograph's area up to `x` (in Tp).
fn uh_cum(u: &UnitHydrograph, x: f64) -> f64 {
    let base = u.base();
    let area = |to: f64| {
        let n = 200;
        let h = to / n as f64;
        (0..n).map(|i| u.ordinate((i as f64 + 0.5) * h) * h).sum::<f64>()
    };
    let total = area(base);
    if total > 0.0 { (area(x.clamp(0.0, base)) / total).clamp(0.0, 1.0) } else { 0.0 }
}

fn idf_names(doc: &Doc) -> Vec<String> {
    match &doc.project.settings.idf {
        Some(c) => c.events.iter().map(|e| e.0.clone()).chain(c.tables.iter().map(|t| t.0.clone())).collect(),
        None => Vec::new(),
    }
}

struct Built {
    series: Vec<Series>,
    title: String,
    x_label: String,
    y_label: String,
    description: Vec<String>,
}

/// Evaluates every checked curve at `xs`, or on a fine grid when `xs` is empty.
fn build(doc: &Doc, st: &CurveState, table_xs: Option<&dyn Fn(f64) -> Vec<f64>>) -> Built {
    let u = &doc.project.units;
    let mut series = Vec::new();
    let mut description = Vec::new();
    let (title, x_label, y_label);
    match st.kind {
        CurveKind::Storm => {
            let events = &doc.project.settings.events;
            let current = events.get(doc.project.settings.current_event).map(|e| u.rain(e.depth)).unwrap_or(1.0);
            for name in st.checked.iter() {
                if let Some(s) = crate::library::find_storm(&doc.project, name) {
                    // (label, depth scale) per curve: one dimensionless or current-depth curve, or
                    // one per event using this storm (every event when none does).
                    let scales: Vec<(String, f64)> = if st.event_depths {
                        let using: Vec<_> = events.iter().filter(|e| e.storm == s.name).collect();
                        let pick = if using.is_empty() { events.iter().collect() } else { using };
                        let single = st.checked.len() == 1;
                        pick.iter()
                            .map(|e| {
                                (if single { e.name.clone() } else { format!("{} {}", name, e.name) }, u.rain(e.depth))
                            })
                            .collect()
                    } else {
                        vec![(name.clone(), if st.current_depth { current } else { 1.0 })]
                    };
                    let xs = match table_xs {
                        Some(f) => f(s.duration),
                        None => (0..=400).map(|k| s.duration * k as f64 / 400.0).collect::<Vec<f64>>(),
                    };
                    let h = if table_xs.is_some() { st.inc.max(1e-3) } else { s.duration / 400.0 };
                    if description.is_empty() {
                        description = vec![
                            s.name.clone(),
                            s.comment.clone(),
                            format!(
                                "{} {} Points",
                                s.fractions.len(),
                                if st.intensity { "Intensity" } else { "Depth" }
                            ),
                        ];
                    }
                    for (label, scale) in scales {
                        let ys = xs.iter().map(|x| storm_value(&s, *x, st.intensity, h, scale)).collect();
                        let color = COLORS[(series.len() + 2) % COLORS.len()];
                        series.push(Series { name: label, color, x: xs.clone(), y: ys, peak_label: None });
                    }
                }
            }
            let what = if st.intensity { "Intensity" } else { "Depth" };
            title = format!("Rainfall {what} vs. Time");
            x_label = "Elapsed Time (hours)".to_string();
            y_label = match (st.intensity, st.current_depth || st.event_depths) {
                (false, false) => "Fractional Rainfall Depth".to_string(),
                (true, false) => "Fraction of Depth per Hour".to_string(),
                (false, true) => format!("Rainfall Depth ({})", u.rain_heading()),
                (true, true) => format!("Rainfall Intensity ({}/hr)", u.rain_heading()),
            };
        }
        CurveKind::Uh => {
            for (i, name) in st.checked.iter().enumerate() {
                if let Some(h) = crate::library::find_uh(&doc.project, name) {
                    let k = h.peak_factor() / wavestorm_core::uh::CFS_PER_SQMI_INCH_PER_HR;
                    let end = h.base().min(50.0);
                    let xs = match table_xs {
                        Some(f) => f(end),
                        None => (0..=200).map(|j| j as f64 * h.base().min(8.0) / 200.0).collect(),
                    };
                    let ys =
                        xs.iter().map(|x| if st.intensity { h.ordinate(*x) * k } else { uh_cum(&h, *x) }).collect();
                    if description.is_empty() {
                        description = vec![h.name.clone(), h.comment.clone()];
                    }
                    series.push(Series {
                        name: name.clone(),
                        color: COLORS[(i + 2) % COLORS.len()],
                        x: xs,
                        y: ys,
                        peak_label: None,
                    });
                }
            }
            title = format!("Unit Hydrograph {} vs. Time", if st.intensity { "Intensity" } else { "Depth" });
            x_label = "Relative Time (Tp)".to_string();
            y_label =
                if st.intensity { "Relative Intensity (K)".to_string() } else { "Cumulative Fraction".to_string() };
        }
        CurveKind::Idf => {
            if let Some(c) = &doc.project.settings.idf {
                description = vec![c.name.clone(), format!("{} events", c.events.len() + c.tables.len())];
                for (i, name) in st.checked.iter().enumerate() {
                    let xs: Vec<f64> = match table_xs {
                        Some(f) => f(180.0),
                        None => (0..=170).map(|j| 5.0 + j as f64).collect(),
                    };
                    let ys: Vec<f64> = xs.iter().map(|x| c.intensity(name, *x).unwrap_or(0.0)).collect();
                    if ys.iter().any(|y| *y > 0.0) {
                        series.push(Series {
                            name: name.clone(),
                            color: COLORS[(i + 3) % COLORS.len()],
                            x: xs,
                            y: ys,
                            peak_label: None,
                        });
                    }
                }
            }
            title = format!(
                "{} Intensity vs. Duration",
                doc.project.settings.idf.as_ref().map(|c| c.name.as_str()).unwrap_or("IDF")
            );
            x_label = "Rainfall Duration (minutes)".to_string();
            y_label = "Rainfall Intensity (in/hr)".to_string();
        }
    }
    Built { series, title, x_label, y_label, description }
}

/// Table columns at the chosen increment: (headings, columns).
fn table(doc: &Doc, st: &CurveState) -> (Vec<String>, Vec<Vec<f64>>) {
    let inc = if st.inc.is_finite() { st.inc.max(1e-3) } else { 0.1 };
    let steps = move |end: f64| -> Vec<f64> {
        let n = ((end / inc).ceil() as usize).min(5000);
        (0..=n).map(|k| k as f64 * inc).collect()
    };
    let built = build(doc, st, Some(&steps));
    let unit = match st.kind {
        CurveKind::Storm => "hours",
        CurveKind::Uh => "Tp",
        CurveKind::Idf => "min",
    };
    let mut heads = vec![format!("Time ({unit})")];
    let mut cols: Vec<Vec<f64>> = Vec::new();
    // Curves of different lengths share the longest time column; shorter ones pad with their end value.
    let longest = built.series.iter().map(|s| s.x.len()).max().unwrap_or(0);
    if let Some(s) = built.series.iter().find(|s| s.x.len() == longest) {
        cols.push(s.x.clone());
    }
    for s in &built.series {
        heads.push(s.name.clone());
        let mut y = s.y.clone();
        let last = y.last().copied().unwrap_or(0.0);
        y.resize(longest, last);
        cols.push(y);
    }
    (heads, cols)
}

fn table_text(heads: &[String], cols: &[Vec<f64>], sep: &str) -> String {
    let mut out = heads.join(sep);
    let rows = cols.first().map(|c| c.len()).unwrap_or(0);
    for i in 0..rows {
        out.push('\n');
        let row: Vec<String> = cols.iter().map(|c| format!("{:.4}", c.get(i).copied().unwrap_or(0.0))).collect();
        out.push_str(&row.join(sep));
    }
    out
}

fn curve_tree(ui: &mut Ui, st: &mut CurveState, names: &[String]) {
    let mut toggle: Option<(String, bool)> = None;
    let mut leaf = |ui: &mut Ui, st: &CurveState, name: &str, label: &str| {
        let mut on = st.checked.iter().any(|c| c == name);
        if widgets::check(ui, &mut on, label).changed() {
            toggle = Some((name.to_string(), on));
        }
    };
    for (g, members) in grouped(names) {
        match g {
            None => {
                for m in &members {
                    leaf(ui, st, m, m);
                }
            }
            Some(g) => {
                let open = members.iter().any(|m| st.checked.contains(m));
                egui::CollapsingHeader::new(g.clone()).id_salt(("curve-group", &g)).default_open(open).show(ui, |ui| {
                    for m in &members {
                        leaf(ui, st, m, m);
                    }
                });
            }
        }
    }
    if let Some((name, on)) = toggle {
        if on {
            st.checked.push(name);
        } else {
            st.checked.retain(|c| c != &name);
        }
    }
}

/// Returns false when closed.
pub fn show_curves(ctx: &egui::Context, doc: &mut Doc, st: &mut CurveState, actions: &mut Vec<WindowAction>) -> bool {
    let mut open = true;
    let mut ok = false;
    let title = match st.kind {
        CurveKind::Storm => "Storm Distribution Report",
        CurveKind::Uh => "Unit Hydrograph Report",
        CurveKind::Idf => "IDF Report",
    };
    let names: Vec<String> = match st.kind {
        CurveKind::Storm => crate::library::storm_names(&doc.project),
        CurveKind::Uh => crate::library::uh_names(&doc.project),
        CurveKind::Idf => idf_names(doc),
    };
    if st.fresh {
        st.fresh = false;
        st.checked = names.iter().take(6).cloned().collect();
    }
    crate::frame::Win::new(title).open(&mut open).default_size(vec2(720.0, 460.0)).resizable(true).min_max().show(
        ctx,
        |ui| {
            // Header: radios, time increment, description of the first checked curve.
            let built_desc = build(doc, st, None).description;
            ui.horizontal_top(|ui| {
                if st.kind != CurveKind::Idf {
                    ui.vertical(|ui| {
                        if widgets::radio(ui, !st.intensity, "Depth").clicked() {
                            st.intensity = false;
                        }
                        if widgets::radio(ui, st.intensity, "Intensity").clicked() {
                            st.intensity = true;
                        }
                    });
                }
                ui.vertical(|ui| {
                    let unit = match st.kind {
                        CurveKind::Storm => "(hours)",
                        CurveKind::Uh => "(Tp)",
                        CurveKind::Idf => "(minutes)",
                    };
                    ui.label(format!("Time Increment {unit}"));
                    widgets::num_box(
                        ui,
                        ui.id().with("inc"),
                        &mut st.inc,
                        2,
                        90.0,
                        Some(if st.kind == CurveKind::Idf { 1.0 } else { 0.05 }),
                    );
                    st.inc = if st.inc.is_finite() { st.inc.max(0.01) } else { 0.1 };
                });
                if st.kind == CurveKind::Storm {
                    ui.vertical(|ui| {
                        if widgets::radio(ui, !st.current_depth && !st.event_depths, "Dimensionless").clicked() {
                            (st.current_depth, st.event_depths) = (false, false);
                        }
                        if widgets::radio(ui, st.current_depth, "Current Depth").clicked() {
                            (st.current_depth, st.event_depths) = (true, false);
                        }
                        if widgets::radio(ui, st.event_depths, "Event Depth(s)").clicked() {
                            (st.current_depth, st.event_depths) = (false, true);
                        }
                    });
                }
                ui.vertical(|ui| {
                    for (i, l) in built_desc.iter().enumerate() {
                        if !l.is_empty() {
                            let rt = RichText::new(l.as_str());
                            ui.label(if i == 0 { rt.strong() } else { rt });
                        }
                    }
                });
            });
            ui.add_space(2.0);
            let avail = ui.available_size();
            let col_key = ui.id().with("curves-col");
            let body_w = widgets::body_width(ui, col_key, 210.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    let size = vec2(body_w, (avail.y - 26.0).max(160.0));
                    ui.set_width(size.x);
                    widgets::tabs(ui, &["Graph", "Table"], &mut st.tab);
                    if st.tab == 1 {
                        let (heads, cols) = table(doc, st);
                        if cols.is_empty() {
                            ui.label("Check one or more curves.");
                        } else {
                            let _ = data_table(ui, &heads, &cols, size.y, false);
                        }
                    } else {
                        let b = build(doc, st, None);
                        let spec = ChartSpec {
                            title: &b.title,
                            subtitle: "",
                            x_label: &b.x_label,
                            y_label: &b.y_label,
                            series: &b.series,
                            summary: &[],
                            three_d: false,
                            log_x: st.kind == CurveKind::Idf,
                            marks: &[],
                        };
                        chart::chart(ui, &spec, &mut st.view, size);
                    }
                });
                let col = ui.vertical(|ui| {
                    ui.set_width(200.0);
                    let h = (avail.y - 66.0).max(100.0);
                    egui::ScrollArea::vertical().max_height(h).min_scrolled_height(h).auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            if names.is_empty() {
                                ui.label("No IDF curve in this project. Use Edit to create one.");
                            }
                            curve_tree(ui, st, &names);
                        },
                    );
                    buttons(ui, doc, st, actions, &mut ok);
                });
                widgets::remember_column(ui, col_key, col.response.rect);
            });
        },
    );
    open && !ok
}

fn buttons(ui: &mut Ui, doc: &Doc, st: &mut CurveState, actions: &mut Vec<WindowAction>, ok: &mut bool) {
    let first = st.checked.first().cloned();
    ui.horizontal(|ui| {
        let more = widgets::button(ui, if st.kind == CurveKind::Storm { "More Storms" } else { "More..." }, 96.0);
        egui::Popup::menu(&more).show(|ui| {
            if ui
                .button("Import Files...")
                .on_hover_text("Add rainfall files (.hcr, .huh, .idf, *-Rain.txt) to the rainfall library")
                .clicked()
            {
                actions.push(WindowAction::PickFile(crate::platform::Purpose::RainLib));
                ui.close();
            }
            if st.kind == CurveKind::Storm && ui.button("New Custom Storm...").clicked() {
                actions
                    .push(WindowAction::OpenWindow(Window::StormEdit(crate::dialogs::storm::StormEditor::default())));
                ui.close();
            }
        });
        if widgets::button(ui, "OK", 78.0).clicked() {
            *ok = true;
        }
    });
    ui.horizontal(|ui| {
        if widgets::button(ui, "Print", 46.0).on_hover_text("Print the table (PDF)").clicked() {
            let (heads, cols) = table(doc, st);
            let lines: Vec<String> = table_text(&heads, &cols, "  ").lines().map(str::to_string).collect();
            let name = first.clone().unwrap_or_else(|| "Curves".into());
            let bytes = wavestorm_core::printing::lines_pdf(&doc.project, &doc.results, &name, &lines, &crate::dialogs::misc::today());
            actions.push(WindowAction::SaveFile { name: format!("{name}.pdf"), bytes, ext: "pdf" });
        }
        let export = widgets::button(ui, "Export", 52.0);
        egui::Popup::menu(&export).show(|ui| {
            if ui.button("Copy Table to Clipboard").clicked() {
                let (heads, cols) = table(doc, st);
                ui.ctx().copy_text(table_text(&heads, &cols, "\t"));
                actions.push(WindowAction::Status("Table copied to the clipboard".into()));
                ui.close();
            }
            if ui.button("Save Table as CSV...").clicked() {
                let (heads, cols) = table(doc, st);
                actions.push(WindowAction::SaveFile { name: "curves.csv".into(), bytes: table_text(&heads, &cols, ",").into_bytes(), ext: "csv" });
                ui.close();
            }
            let tip = match st.kind {
                CurveKind::Storm => "Save the first checked storm as a rainfall (.hcr) file",
                CurveKind::Uh => "Save the first checked unit hydrograph as a (.huh) file",
                CurveKind::Idf => "Save the IDF curve as an (.idf) file. PROVISIONAL: the .idf layout is our own and not yet verified against real .idf files.",
            };
            if ui.button("Save Curve File...").on_hover_text(tip).clicked() {
                match st.kind {
                    CurveKind::Storm => {
                        if let Some(s) = first.as_ref().and_then(|n| crate::library::find_storm(&doc.project, n)) {
                            actions.push(WindowAction::SaveFile { name: format!("{}.hcr", s.name), bytes: wavestorm_core::rainlib::write_hcr(&s).into_bytes(), ext: "hcr" });
                        }
                    }
                    CurveKind::Uh => {
                        if let Some(u) = first.as_ref().and_then(|n| crate::library::find_uh(&doc.project, n)) {
                            actions.push(WindowAction::SaveFile { name: format!("{}.huh", u.name), bytes: wavestorm_core::rainlib::write_huh(&u).into_bytes(), ext: "huh" });
                        }
                    }
                    CurveKind::Idf => {
                        if let Some(c) = &doc.project.settings.idf {
                            let file = wavestorm_core::rainlib::IdfFile::from_curve(c);
                            actions.push(WindowAction::SaveFile { name: format!("{}.idf", c.name), bytes: wavestorm_core::rainlib::write_idf(&file).into_bytes(), ext: "idf" });
                        }
                    }
                }
                ui.close();
            }
        });
        let can_edit = st.kind != CurveKind::Uh;
        if widgets::button_enabled(ui, "Edit", 46.0, can_edit).on_hover_text("Edit a copy of the first checked storm, or the project IDF curve").clicked() {
            match st.kind {
                CurveKind::Storm => {
                    if let Some(s) = first.as_ref().and_then(|n| crate::library::find_storm(&doc.project, n)) {
                        let mut ed = crate::dialogs::storm::StormEditor::from_storm(&s);
                        ed.name = format!("{} (custom)", s.name);
                        actions.push(WindowAction::OpenWindow(Window::StormEdit(ed)));
                    }
                }
                CurveKind::Idf => actions.push(WindowAction::OpenWindow(Window::Idf(crate::dialogs::storm::IdfEditor::new(doc)))),
                CurveKind::Uh => {}
            }
        }
        if widgets::button(ui, "Help", 46.0).clicked() {
            widgets::request_help(ui.ctx(), "rainfall");
        }
    });
}
