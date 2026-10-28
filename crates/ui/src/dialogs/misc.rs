//! Smaller dialogs: units, diagram, notes, about, background calibration and settings, undo.

use super::{Buttons, conv_field, ok_cancel_apply};
use crate::doc::Doc;
use crate::prefs::Prefs;
use crate::theme::{ThemeKind, Tokens};
use crate::widgets;
use crate::windows::WindowAction;
use egui::{Pos2, RichText, vec2};
use wavestorm_core::drawing::{self, PondEstimate};
use wavestorm_core::landuse::ImperviousBasis;
use wavestorm_core::project::{DiagramSettings, NodeData, NodeType, Project};
use wavestorm_core::units::{UnitSystem, Units};

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
        widgets::group(ui, "Custom Decimal Places", |ui| {
            let opts: Vec<(Option<u8>, String)> = std::iter::once((None, "Auto".to_string()))
                .chain((0..=5u8).map(|d| (Some(d), d.to_string())))
                .collect();
            let d = &mut st.draft.decimals;
            egui::Grid::new("units-dp").num_columns(4).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                for (i, (label, v)) in [
                    ("Area", &mut d.area),
                    ("Volume", &mut d.volume),
                    ("Flow", &mut d.flow),
                    ("Rainfall", &mut d.rain),
                    ("Elevation", &mut d.elevation),
                    ("Length", &mut d.length),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.label(label);
                    widgets::combo(ui, &format!("dp-{label}"), v, &opts, 70.0);
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });
        let draft = st.draft;
        ui.label(
            RichText::new(format!(
                "Areas in {}, volumes in {}, flows in {}",
                draft.area_heading(),
                draft.volume_heading(),
                draft.flow_label()
            ))
            .color(Tokens::get(ui.ctx()).text_dim),
        );
        match ok_cancel_apply(ui, false, "start") {
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
            conv_field(
                ui,
                "sym",
                "Symbol Size:",
                "units",
                &mut st.draft.symbol_size,
                |x| x,
                |x| x.clamp(8.0, 400.0),
                0,
                120.0,
            );
            conv_field(
                ui,
                "grid",
                "Grid Spacing:",
                "units",
                &mut st.draft.grid,
                |x| x,
                |x| x.clamp(2.0, 1000.0),
                0,
                120.0,
            );
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
        match ok_cancel_apply(ui, false, "mouse") {
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

pub struct GeneralState {
    pub draft: Prefs,
}

/// Settings > General: per-user preferences, saved for later sessions.
pub fn general(ctx: &egui::Context, st: &mut GeneralState, actions: &mut Vec<WindowAction>) -> bool {
    let mut keep = true;
    crate::frame::Win::new("General Settings").collapsible(false).resizable(false).show(ctx, |ui| {
        widgets::group(ui, "Appearance", |ui| {
            for k in ThemeKind::ALL {
                if widgets::radio(ui, st.draft.theme == k, k.label()).clicked() {
                    st.draft.theme = k;
                }
            }
        });
        widgets::group(ui, "Automatic Backup", |ui| {
            let dim = Tokens::get(ui.ctx()).text_dim;
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.horizontal(|ui| {
                    widgets::check(ui, &mut st.draft.autosave, "Autosave projects every").on_hover_text("Save changed projects to their own files automatically. Projects never saved yet are kept for recovery instead.");
                    if widgets::num_box(ui, ui.id().with("autosave-minutes"), &mut st.draft.autosave_minutes, 1, 60.0, Some(1.0)) {
                        st.draft.autosave_minutes = st.draft.autosave_minutes.clamp(0.25, 240.0);
                    }
                    ui.label("minutes");
                });
                conv_field(ui, "backup", "Backup Interval", "minutes", &mut st.draft.backup_minutes, |x| x, |x| x.clamp(0.0, 1440.0), 0, 200.0);
                ui.label(RichText::new("Changed projects are also copied to <project>.bak; 0 turns backups off.").color(dim));
            }
            ui.label(RichText::new(if cfg!(target_arch = "wasm32") {
                "Unsaved changes are kept in this browser's storage and reopened if the page is closed before you export the project."
            } else {
                "Unsaved changes, including new projects never saved, are kept and reopened if the program closes unexpectedly."
            })
            .color(dim));
        });
        widgets::group(ui, "Rainfall Library", |ui| {
            let lib = crate::library::current();
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.label("Folder with rainfall files (.hcr storms, .huh unit hydrographs, .idf IDF curves, *-Rain.txt county tables):");
                ui.horizontal(|ui| {
                    widgets::text_box(ui, &mut st.draft.rainfall_folder, 360.0);
                    if widgets::button(ui, "Browse...", 78.0).clicked()
                        && let Some(p) = crate::platform::Platform::default().pick_folder()
                    {
                        st.draft.rainfall_folder = p;
                    }
                });
            }
            ui.horizontal(|ui| {
                if widgets::button(ui, "Import Rainfall Files...", 170.0).on_hover_text("Choose several rainfall files at once").clicked() {
                    actions.push(WindowAction::PickFile(crate::platform::Purpose::RainLib));
                }
                #[cfg(not(target_arch = "wasm32"))]
                if widgets::button(ui, "Rescan", 70.0).on_hover_text("Read the folder again").clicked() {
                    crate::library::set_folder(&st.draft.rainfall_folder);
                }
                if widgets::button_enabled(ui, "Forget Imports", 110.0, crate::library::import_count() > 0).clicked() {
                    crate::library::clear_imports();
                }
            });
            let t = Tokens::get(ui.ctx());
            ui.label(RichText::new(format!("{} storms, {} unit hydrographs, {} IDF files (provisional format), {} lookup tables loaded", lib.storms.len(), lib.uhs.len(), lib.idfs.len(), lib.tables.len())).color(t.text_dim));
            for p in lib.problems.iter().take(3) {
                ui.colored_label(t.warning, p);
            }
        });
        widgets::check(ui, &mut st.draft.multiple_project_reports, "Allow multiple Project Reports windows");
        widgets::check(ui, &mut st.draft.show_tips, "Show pop-up tips on node editing screens");
        widgets::check(ui, &mut st.draft.show_messages, "Show new calculation messages automatically");
        match ok_cancel_apply(ui, true, "start") {
            Buttons::Ok => {
                crate::library::set_folder_if_changed(&st.draft.rainfall_folder);
                actions.push(WindowAction::SetPrefs(st.draft.clone()));
                keep = false;
            }
            Buttons::Apply => {
                crate::library::set_folder_if_changed(&st.draft.rainfall_folder);
                actions.push(WindowAction::SetPrefs(st.draft.clone()));
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
    crate::frame::Win::new("Project Notes").collapsible(false).resizable(true).default_size(vec2(520.0, 380.0)).show(
        ctx,
        |ui| {
            widgets::text_field(ui, "Project Title:", &mut st.title, 380.0);
            widgets::text_field(ui, "Prepared by:", &mut st.prepared_by, 380.0);
            ui.label("Notes:");
            ui.add_sized(vec2(ui.available_width(), 220.0), egui::TextEdit::multiline(&mut st.notes));
            match ok_cancel_apply(ui, false, "start") {
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
        },
    );
    keep
}

pub fn about(ctx: &egui::Context) -> bool {
    let mut open = true;
    crate::frame::Win::new("About WAVEStorm").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        // Wide enough for the description lines on one line each.
        ui.set_min_width(400.0);
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            crate::logo::show(ui, 360.0);
            ui.add_space(4.0);
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
            if ok_cancel_apply(ui, false, "background") != Buttons::None {
                keep = false;
            }
            return;
        };
        let units_dist = ((b - a).length()) as f64;
        ui.label(format!("Distance between the points on the diagram: {units_dist:.1} units"));
        conv_field(
            ui,
            "dist",
            "Real distance between the points:",
            u.len_label(),
            &mut st.distance,
            |x| u.len(x),
            |x| u.len_in(x),
            2,
            260.0,
        );
        if units_dist > 0.0 && st.distance > 0.0 {
            let fpu = st.distance / units_dist;
            ui.label(format!(
                "Scale: 1 diagram unit = {}",
                u.fmt_len(fpu).replace("'", &format!(" {}", if u.metric() { "" } else { "ft" }))
            ));
        }
        match ok_cancel_apply(ui, false, "background") {
            Buttons::Ok | Buttons::Apply => {
                if units_dist > 0.0 && st.distance > 0.0 {
                    let fpu = st.distance / units_dist;
                    doc.edit("Calibrate Background", |p| {
                        if let Some(ul) = &mut p.diagram.underlay {
                            ul.feet_per_unit = Some(fpu);
                        }
                        // Drawn sub-areas and Tc paths are re-measured at the new scale.
                        wavestorm_core::drawing::sync_all(p);
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

/// Pond estimator: asks for the depth and side slope of a pond whose top was just drawn.
pub struct PondEstimateState {
    /// The drawn top of the pond (diagram units).
    pub points: Vec<[f64; 2]>,
    pub est: PondEstimate,
}

impl PondEstimateState {
    pub fn new(points: &[Pos2]) -> Self {
        PondEstimateState {
            points: points.iter().map(|p| [p.x as f64, p.y as f64]).collect(),
            est: PondEstimate::default(),
        }
    }
}

/// On OK, creates a pond at the centre of the drawn outline whose storage is the estimated
/// stage-area data (prismatic), and opens its Storage tab.
pub fn pond_estimate(
    ctx: &egui::Context,
    doc: &mut Doc,
    st: &mut PondEstimateState,
    actions: &mut Vec<WindowAction>,
) -> bool {
    let mut keep = true;
    let u = doc.project.units;
    let fpu = drawing::feet_per_unit(&doc.project);
    crate::frame::Win::new("Pond Estimator").collapsible(false).resizable(false).show(ctx, |ui| {
        let top_area = drawing::polygon_area(&st.points) * fpu * fpu;
        ui.label(format!("Area at the top of the pond (drawn): {}", u.fmt_area(top_area)));
        if doc.project.diagram.underlay.as_ref().and_then(|ul| ul.feet_per_unit).is_none() {
            ui.colored_label(Tokens::get(ui.ctx()).warning, "Not calibrated: 1 diagram unit = 1 ft.");
        }
        ui.add_space(4.0);
        let len = u.len_label();
        conv_field(
            ui,
            "pe-top",
            "Top Elevation:",
            len,
            &mut st.est.top,
            |x| u.len(x),
            |x| u.len_in(x),
            u.elev_dp(),
            260.0,
        );
        conv_field(
            ui,
            "pe-depth",
            "Approximate Depth:",
            len,
            &mut st.est.depth,
            |x| u.len(x),
            |x| u.len_in(x),
            2,
            260.0,
        );
        conv_field(ui, "pe-z", "Side Slope:", "H:1V", &mut st.est.z, |x| x, |x| x.max(0.0), 2, 260.0);
        ui.add_space(4.0);
        let volume = drawing::pond_storage(&st.points, fpu, &st.est);
        match &volume {
            Ok(v) => {
                let (top, bottom) = (st.est.top, st.est.bottom());
                ui.label(format!(
                    "Bottom elevation: {}    Bottom area: {}",
                    u.fmt_elev(bottom),
                    u.fmt_area(v.surface_area(bottom).unwrap_or(0.0))
                ));
                ui.label(format!(
                    "Storage: {} from {} to {} (prismatic stage-area data)",
                    u.fmt_volume(v.gross(top)),
                    u.fmt_elev(bottom),
                    u.fmt_elev(top)
                ));
            }
            Err(e) => {
                ui.colored_label(Tokens::get(ui.ctx()).warning, e.as_str());
            }
        }
        if let Some(max) = drawing::pond_max_depth(&st.points, fpu, st.est.z) {
            ui.label(RichText::new(format!("The side slopes meet {} below the top.", u.fmt_len(max))).small());
        }
        match ok_cancel_apply(ui, false, "background") {
            Buttons::Ok | Buttons::Apply => {
                if let Ok(v) = volume {
                    let c = drawing::centroid(&st.points);
                    let (outline, est) = (st.points.clone(), st.est);
                    let mut id = 0;
                    doc.edit("Pond Estimator", |p| {
                        id = p.add_node(NodeType::Pond, c);
                        if let Some(n) = p.node_mut(id) {
                            n.outline = Some(outline);
                            if let NodeData::Pond(pond) = &mut n.data {
                                pond.storage.volumes.push(v);
                                pond.estimate = Some(est);
                            }
                        }
                    });
                    doc.selection = vec![id];
                    actions.push(WindowAction::OpenEdit(id));
                    actions.push(WindowAction::Status(
                        "Created a pond with estimated storage. Add its outlets on the Outlets tab.".into(),
                    ));
                    keep = false;
                }
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
            if conv_field(
                ui,
                "page",
                "PDF Page:",
                &format!("of {pages}"),
                &mut page,
                |x| x,
                |x| x.clamp(1.0, pages as f64),
                0,
                120.0,
            ) {
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
        widgets::check(ui, &mut draft.locked, "Lock position and size");
        match draft.feet_per_unit {
            Some(f) => ui.label(format!("Calibrated: 1 diagram unit = {:.4} {}", u.len(f), u.len_label())),
            None => {
                ui.colored_label(Tokens::get(ui.ctx()).warning, "Not calibrated: traced areas assume 1 unit = 1 ft.")
            }
        };
        let _ = st;
        if draft != ul {
            // Rescaling the image keeps the calibration consistent (real size per pixel is
            // fixed), and the drawings scale with it about its corner so they stay on it.
            let k = draft.scale / ul.scale;
            let rescaled = (k - 1.0).abs() > 1e-12 && k.is_finite() && k > 0.0;
            if rescaled && let Some(f) = draft.feet_per_unit {
                draft.feet_per_unit = Some(f / k);
            }
            let o = ul.origin;
            doc.edit("Background Settings", |p| {
                p.diagram.underlay = Some(draft.clone());
                if rescaled {
                    wavestorm_core::drawing::transform(p, &|q| [o[0] + (q[0] - o[0]) * k, o[1] + (q[1] - o[1]) * k]);
                }
            });
            actions.push(WindowAction::Status("Background updated".into()));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Move / Resize...", 120.0)
                .on_hover_text("Drag the background on the diagram to move it, or a corner to resize it")
                .clicked()
            {
                actions.push(WindowAction::PlaceUnderlay);
                keep = false;
            }
            if widgets::button(ui, "Close", 80.0).clicked() {
                keep = false;
            }
        });
    });
    keep
}

#[derive(Default)]
pub struct UndoState {
    pub selected: Option<usize>,
}

pub fn undo_history(ctx: &egui::Context, doc: &mut Doc, st: &mut UndoState) -> bool {
    let mut open = true;
    crate::frame::Win::new("Undo History").open(&mut open).resizable(true).default_size(vec2(380.0, 340.0)).show(
        ctx,
        |ui| {
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
        },
    );
    open
}

pub struct PrintState {
    pub opts: wavestorm_core::printing::PrintOptions,
}

impl Default for PrintState {
    fn default() -> Self {
        PrintState { opts: wavestorm_core::printing::PrintOptions { date: today(), ..Default::default() } }
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
pub fn print(ctx: &egui::Context, st: &mut PrintState) -> (bool, Option<wavestorm_core::printing::PrintOptions>) {
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
        ui.label(
            RichText::new("The report is saved as a PDF you can print or share.").color(Tokens::get(ui.ctx()).text_dim),
        );
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

pub struct LandUseState {
    pub draft: wavestorm_core::landuse::LandUseSettings,
    pub tab: usize,
    pub new_pollutant: String,
    /// Rainfall depth remembered while "Use actual runoff" is selected.
    pub depth: f64,
}

impl LandUseState {
    pub fn new(p: &Project) -> Self {
        LandUseState {
            draft: p.land_use.clone(),
            tab: 1,
            new_pollutant: String::new(),
            depth: p.land_use.annual_rainfall.unwrap_or(42.0),
        }
    }
}

pub fn land_use(ctx: &egui::Context, doc: &mut Doc, st: &mut LandUseState) -> bool {
    let mut keep = true;
    crate::frame::Win::new("Land-Use Settings").resizable(true).default_size(vec2(560.0, 420.0)).show(ctx, |ui| {
        widgets::tabs(ui, &["General", "Land-Uses"], &mut st.tab);
        widgets::tab_body(ui, |ui| {
            ui.set_min_size(vec2(520.0, 260.0));
            let d = &mut st.draft;
            if st.tab == 0 {
                widgets::check(ui, &mut d.report_land_use, "Report Land-Use");
                widgets::check(ui, &mut d.report_loading, "Report Pollutant Loading");
                ui.add_space(4.0);
                widgets::group(ui, "Calculate Imperviousness for:", |ui| {
                    for (basis, label) in [
                        (ImperviousBasis::EntireProject, "Entire Project"),
                        (ImperviousBasis::ProjectLandUse, "Project Land-Use"),
                        (ImperviousBasis::EachSubcatchment, "Each Subcatchment"),
                        (ImperviousBasis::SubcatchmentLandUse, "Subcatchment Land-Use"),
                        (ImperviousBasis::ActualRunoff, "Use actual runoff"),
                    ] {
                        if widgets::radio(ui, d.imperviousness == basis, label).clicked() {
                            d.imperviousness = basis;
                            // Actual runoff replaces the Simple Method's annual rainfall.
                            if basis == ImperviousBasis::ActualRunoff {
                                if let Some(p) = d.annual_rainfall.take() {
                                    st.depth = p;
                                }
                            } else if d.annual_rainfall.is_none() {
                                d.annual_rainfall = Some(st.depth);
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                if let Some(p) = &mut d.annual_rainfall {
                    conv_field(ui, "p", "Rainfall Depth:", "inches", p, |x| x, |x| x.max(0.0), 2, 140.0);
                    conv_field(
                        ui,
                        "pj",
                        "Correction Factor:",
                        "",
                        &mut d.correction,
                        |x| x,
                        |x| x.clamp(0.0, 1.0),
                        3,
                        140.0,
                    );
                } else {
                    ui.label(
                        RichText::new("Loads use the runoff of the calculated event.")
                            .color(Tokens::get(ui.ctx()).text_dim),
                    );
                }
                ui.add_space(4.0);
                if widgets::button_enabled(ui, "Remove Land-Use data...", 160.0, !d.uses.is_empty()).clicked() {
                    d.uses.clear();
                }
            } else {
                egui::ScrollArea::both().max_height(230.0).show(ui, |ui| {
                    egui::Grid::new("lu").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                        ui.label(RichText::new("Line").strong());
                        ui.add_sized(vec2(140.0, 16.0), egui::Label::new(RichText::new("Land-Use").strong()));
                        for p in &d.pollutants {
                            ui.add_sized(vec2(70.0, 16.0), egui::Label::new(RichText::new(p).strong()));
                        }
                        ui.end_row();
                        ui.label("");
                        ui.label("");
                        for _ in &d.pollutants {
                            ui.add_sized(vec2(70.0, 16.0), egui::Label::new("(mg/l)"));
                        }
                        ui.end_row();
                        let mut del = None;
                        let n = d.pollutants.len();
                        for (i, (name, conc)) in d.uses.iter_mut().enumerate() {
                            ui.label(format!("{}", i + 1));
                            widgets::text_box(ui, name, 140.0);
                            conc.resize(n, 0.0);
                            for (k, c) in conc.iter_mut().enumerate() {
                                widgets::num_box(ui, ui.id().with(("c", i, k)), c, 2, 70.0, None);
                            }
                            if widgets::button(ui, "X", 22.0).clicked() {
                                del = Some(i);
                            }
                            ui.end_row();
                        }
                        if let Some(i) = del {
                            d.uses.remove(i);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if widgets::button(ui, "Add Land Use", 100.0).clicked() {
                        d.uses.push((format!("Land use {}", d.uses.len() + 1), vec![0.0; d.pollutants.len()]));
                    }
                    widgets::text_box(ui, &mut st.new_pollutant, 70.0);
                    if widgets::button(ui, "Add Pollutant", 100.0).clicked() && !st.new_pollutant.trim().is_empty() {
                        d.pollutants.push(st.new_pollutant.trim().to_string());
                        st.new_pollutant.clear();
                    }
                    widgets::button_enabled(ui, "Import Pollutants...", 130.0, false);
                });
            }
        });
        match ok_cancel_apply(ui, true, "landuse") {
            Buttons::Ok | Buttons::Apply => {
                let d = st.draft.clone();
                doc.edit("Land-Use Settings", |p| p.land_use = d.clone());
                keep = false;
            }
            Buttons::Cancel => keep = false,
            Buttons::None => {}
        }
    });
    keep
}
