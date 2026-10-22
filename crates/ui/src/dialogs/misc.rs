//! Smaller dialogs: units, diagram, notes, about, background calibration and settings, undo.

use super::{Buttons, conv_field, ok_cancel_apply};
use crate::doc::Doc;
use crate::theme::Tokens;
use crate::widgets;
use crate::windows::WindowAction;
use egui::{Pos2, RichText, vec2};
use stormwaves_core::project::{DiagramSettings, Project};
use stormwaves_core::units::{UnitSystem, Units};

pub struct UnitsState {
    pub draft: Units,
}

impl UnitsState {
    pub fn new(p: &Project) -> Self {
        UnitsState { draft: p.units }
    }
}

pub fn units(ctx: &egui::Context, doc: &mut Doc, st: &mut UnitsState) -> bool {
    let mut keep = true;
    crate::frame::Win::new("Units").collapsible(false).resizable(false).show(ctx, |ui| {
        widgets::group(ui, "Units System", |ui| {
            if widgets::radio(ui, st.draft.system == UnitSystem::English, "English / US customary").clicked() {
                st.draft.system = UnitSystem::English;
            }
            if widgets::radio(ui, st.draft.system == UnitSystem::Metric, "Metric (SI)").clicked() {
                st.draft.system = UnitSystem::Metric;
            }
        });
        widgets::check(ui, &mut st.draft.large, "Large units (acres & acre-feet / hectares & megalitres)");
        widgets::check(ui, &mut st.draft.low_flow, "Low-flow precision (extra decimal places)");
        let draft = st.draft;
        ui.label(RichText::new(format!("Areas in {}, volumes in {}, flows in {}", draft.area_heading(), draft.volume_heading(), draft.flow_label())).color(Tokens::get(ui.ctx()).text_dim));
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                doc.edit("Units", |p| p.units = draft);
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

pub struct DiagramState {
    pub draft: DiagramSettings,
}

impl DiagramState {
    pub fn new(p: &Project) -> Self {
        DiagramState { draft: DiagramSettings { underlay: None, ..p.diagram.clone() } }
    }
}

pub fn diagram(ctx: &egui::Context, doc: &mut Doc, st: &mut DiagramState) -> bool {
    let mut keep = true;
    crate::frame::Win::new("Diagram Settings").collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            conv_field(ui, "sym", "Symbol Size:", "units", &mut st.draft.symbol_size, |x| x, |x| x.clamp(8.0, 400.0), 0, 120.0);
            conv_field(ui, "grid", "Grid Spacing:", "units", &mut st.draft.grid, |x| x, |x| x.clamp(2.0, 1000.0), 0, 120.0);
        });
        widgets::check(ui, &mut st.draft.show_grid, "Show grid");
        widgets::check(ui, &mut st.draft.snap_to_grid, "Snap to grid");
        let mut title = doc.project.title.clone();
        ui.add_space(4.0);
        let changed_title = widgets::text_field(ui, "Project Title:", &mut title, 320.0);
        if changed_title {
            doc.project.title = title;
            doc.dirty = true;
        }
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                let d = st.draft.clone();
                doc.edit("Diagram Settings", |p| {
                    let underlay = p.diagram.underlay.take();
                    p.diagram = DiagramSettings { underlay, ..d.clone() };
                });
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

pub struct NotesState {
    pub title: String,
    pub prepared_by: String,
    pub notes: String,
}

impl NotesState {
    pub fn new(p: &Project) -> Self {
        NotesState { title: p.title.clone(), prepared_by: p.prepared_by.clone(), notes: p.notes.clone() }
    }
}

pub fn notes(ctx: &egui::Context, doc: &mut Doc, st: &mut NotesState) -> bool {
    let mut keep = true;
    crate::frame::Win::new("Project Notes").collapsible(false).resizable(true).default_size(vec2(520.0, 380.0)).show(ctx, |ui| {
        widgets::text_field(ui, "Project Title:", &mut st.title, 380.0);
        widgets::text_field(ui, "Prepared by:", &mut st.prepared_by, 380.0);
        ui.label("Notes:");
        ui.add_sized(vec2(ui.available_width(), 220.0), egui::TextEdit::multiline(&mut st.notes));
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                let (t, pb, n) = (st.title.clone(), st.prepared_by.clone(), st.notes.clone());
                doc.edit("Project Notes", |p| {
                    p.title = t.clone();
                    p.prepared_by = pb.clone();
                    p.notes = n.clone();
                });
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

pub fn about(ctx: &egui::Context) -> bool {
    let mut open = true;
    crate::frame::Win::new("About Stormwaves").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Stormwaves").size(24.0).strong());
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("Stormwater hydrology & hydraulics: TR-55 / TR-20 runoff,");
            ui.label("time of concentration, reach and pond routing, outlet hydraulics.");
            ui.add_space(6.0);
            ui.label("Methods follow USDA NRCS TR-55 (1986) and NEH Part 630.");
            ui.label("Open source (MIT OR Apache-2.0).");
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Mouse: double-click a node for its report, right-click for options, drag from the palette to create nodes, drag an arrowhead to reroute, Ctrl-drag to clone, wheel to zoom, middle-drag to pan.").small());
    });
    open
}

/// Two-point background calibration.
pub struct CalibrateState {
    pub a: Option<Pos2>,
    pub b: Option<Pos2>,
    pub distance: f64,
}

impl CalibrateState {
    pub fn intro() -> Self {
        CalibrateState { a: None, b: None, distance: 100.0 }
    }
    pub fn distance(a: Pos2, b: Pos2) -> Self {
        CalibrateState { a: Some(a), b: Some(b), distance: 100.0 }
    }
}

pub fn calibrate(ctx: &egui::Context, doc: &mut Doc, st: &mut CalibrateState) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    crate::frame::Win::new("Calibrate Background Scale").collapsible(false).resizable(false).show(ctx, |ui| {
        let (Some(a), Some(b)) = (st.a, st.b) else {
            ui.label("The background has been loaded.");
            ui.label("To set its real-world scale, choose Diagram > Background > Calibrate Scale");
            ui.label("(or the ruler button on the right toolbar) and click two points");
            ui.label("on the background that are a known distance apart, such as the");
            ui.label("ends of a scale bar or a dimensioned property line.");
            if ok_cancel_apply(ui, false) != Buttons::None {
                keep = false;
            }
            return;
        };
        let units_dist = ((b - a).length()) as f64;
        ui.label(format!("Distance between the points on the diagram: {units_dist:.1} units"));
        conv_field(ui, "dist", "Real distance between the points:", u.len_label(), &mut st.distance, |x| u.len(x), |x| u.len_in(x), 2, 260.0);
        if units_dist > 0.0 && st.distance > 0.0 {
            let fpu = st.distance / units_dist;
            ui.label(format!("Scale: 1 diagram unit = {}", u.fmt_len(fpu).replace("'", &format!(" {}", if u.metric() { "" } else { "ft" }))));
        }
        match ok_cancel_apply(ui, false) {
            Buttons::Ok | Buttons::Apply => {
                if units_dist > 0.0 && st.distance > 0.0 {
                    let fpu = st.distance / units_dist;
                    doc.edit("Calibrate Background", |p| {
                        if let Some(ul) = &mut p.diagram.underlay {
                            ul.feet_per_unit = Some(fpu);
                        }
                    });
                }
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}

#[derive(Default)]
pub struct UnderlayState {
    pub page_text: String,
}

pub fn underlay(ctx: &egui::Context, doc: &mut Doc, st: &mut UnderlayState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    let Some(ul) = doc.project.diagram.underlay.clone() else { return false };
    crate::frame::Win::new("Background Settings").collapsible(false).resizable(false).show(ctx, |ui| {
        let mut draft = ul.clone();
        ui.label(format!("File: {}", draft.file_name));
        let pages = crate::underlay::page_count(&draft.bytes);
        if pages > 1 {
            let mut page = (draft.page + 1) as f64;
            if conv_field(ui, "page", "PDF Page:", &format!("of {pages}"), &mut page, |x| x, |x| x.clamp(1.0, pages as f64), 0, 120.0) {
                draft.page = (page.round() as usize).clamp(1, pages) - 1;
            }
        }
        ui.horizontal(|ui| {
            let mut op = draft.opacity as f64 * 100.0;
            if conv_field(ui, "op", "Opacity:", "%", &mut op, |x| x, |x| x.clamp(5.0, 100.0), 0, 100.0) {
                draft.opacity = (op / 100.0) as f32;
            }
            conv_field(ui, "sc", "Image Scale:", "units/pixel", &mut draft.scale, |x| x, |x| x.max(1e-4), 4, 140.0);
        });
        widgets::check(ui, &mut draft.visible, "Show background");
        match draft.feet_per_unit {
            Some(f) => ui.label(format!("Calibrated: 1 diagram unit = {:.4} {}", u.len(f), u.len_label())),
            None => ui.colored_label(Tokens::get(ui.ctx()).warning, "Not calibrated: traced areas assume 1 unit = 1 ft."),
        };
        let _ = st;
        if draft != ul {
            // Rescaling the image keeps the calibration consistent: real size per pixel is fixed.
            if (draft.scale - ul.scale).abs() > 1e-12 {
                if let Some(f) = draft.feet_per_unit {
                    draft.feet_per_unit = Some(f * ul.scale / draft.scale);
                }
            }
            doc.edit("Background Settings", |p| p.diagram.underlay = Some(draft.clone()));
            actions.push(WindowAction::Status("Background updated".into()));
        }
        ui.add_space(4.0);
        if widgets::button(ui, "Close", 80.0).clicked() {
            keep = false;
        }
    });
    keep
}

#[derive(Default)]
pub struct UndoState {
    pub selected: Option<usize>,
}

pub fn undo_history(ctx: &egui::Context, doc: &mut Doc, st: &mut UndoState) -> bool {
    let mut open = true;
    crate::frame::Win::new("Undo History").open(&mut open).resizable(true).default_size(vec2(380.0, 340.0)).show(ctx, |ui| {
        egui::Grid::new("undo").striped(true).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
            ui.label(RichText::new("#").strong());
            ui.label(RichText::new("Category").strong());
            ui.label(RichText::new("Activity").strong());
            ui.end_row();
            let mut k = 1;
            for e in doc.undo.iter().rev() {
                if ui.selectable_label(st.selected == Some(k), format!("{k}")).clicked() {
                    st.selected = Some(k);
                }
                ui.label("Undo");
                ui.label(&e.label);
                ui.end_row();
                k += 1;
            }
            for e in doc.redo.iter().rev() {
                ui.label(format!("{k}"));
                ui.label("Redo");
                ui.label(&e.label);
                ui.end_row();
                k += 1;
            }
        });
        ui.horizontal(|ui| {
            if widgets::button_enabled(ui, "Undo", 70.0, !doc.undo.is_empty()).clicked() {
                doc.undo();
            }
            if widgets::button_enabled(ui, "Redo", 70.0, !doc.redo.is_empty()).clicked() {
                doc.redo();
            }
            let mut max = doc.max_undo as f64;
            if conv_field(ui, "max", "Max. Undo Steps:", "", &mut max, |x| x, |x| x.clamp(1.0, 1000.0), 0, 120.0) {
                doc.max_undo = max.round().clamp(1.0, 1000.0) as usize;
            }
        });
    });
    open
}

pub struct PrintState {
    pub opts: stormwaves_core::printing::PrintOptions,
}

impl Default for PrintState {
    fn default() -> Self {
        PrintState { opts: stormwaves_core::printing::PrintOptions { date: today(), ..Default::default() } }
    }
}

/// Today's date as m/d/yyyy.
pub fn today() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new_0();
        format!("{}/{}/{}", d.get_month() + 1, d.get_date(), d.get_full_year())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = (secs / 86_400) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{m}/{d}/{y}")
    }
}

/// Returns Some(options) when the user clicks Print.
pub fn print(ctx: &egui::Context, st: &mut PrintState) -> (bool, Option<stormwaves_core::printing::PrintOptions>) {
    let mut keep = true;
    let mut go = None;
    crate::frame::Win::new("Print Report").collapsible(false).resizable(false).show(ctx, |ui| {
        widgets::group(ui, "Include", |ui| {
            widgets::check(ui, &mut st.opts.diagram, "Routing diagram");
            widgets::check(ui, &mut st.opts.node_listing, "Node listing");
            widgets::check(ui, &mut st.opts.area_listing, "Area and soil listing");
            widgets::check(ui, &mut st.opts.summaries, "Node summaries");
            widgets::check(ui, &mut st.opts.graphs, "Hydrograph graphs");
            widgets::check(ui, &mut st.opts.tables, "Tabular hydrographs");
        });
        widgets::text_field(ui, "Date printed:", &mut st.opts.date, 160.0);
        ui.label(RichText::new("The report is saved as a PDF you can print or share.").color(Tokens::get(ui.ctx()).text_dim));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Print to PDF...", 110.0).clicked() {
                go = Some(st.opts.clone());
                keep = false;
            }
            if widgets::button(ui, "Cancel", 80.0).clicked() {
                keep = false;
            }
        });
    });
    (keep, go)
}
