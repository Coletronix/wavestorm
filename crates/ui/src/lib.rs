//! Stormwaves user interface (egui). The same code runs on the desktop and in the browser.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod chart;
pub mod diagram;
pub mod dialogs;
pub mod doc;
pub mod frame;
pub mod icons;
pub mod platform;
pub mod reports;
pub mod samples;
pub mod theme;
pub mod underlay;
pub mod widgets;
pub mod windows;

use diagram::{CanvasAction, Interaction, Tool};
use doc::Doc;
use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use icons::Glyph;
use platform::{Incoming, Platform, Purpose};
use stormwaves_core::project::{Node, NodeData, NodeId, NodeType, Project};
use theme::{ThemeKind, Tokens};
use windows::{Window, WindowAction};

/// The application.
pub struct StormwavesApp {
    pub docs: Vec<Doc>,
    pub active: usize,
    pub theme: ThemeKind,
    pub platform: Platform,
    pub tool: Tool,
    pub interaction: Interaction,
    palette_drag: Option<NodeType>,
    status: String,
    clipboard: Vec<Node>,
    context_node: Option<(NodeId, Pos2)>,
    error: Option<String>,
    styled: bool,
    untitled: usize,
    canvas_rect: Rect,
}

impl Default for StormwavesApp {
    fn default() -> Self {
        Self::new()
    }
}

impl StormwavesApp {
    pub fn new() -> Self {
        let mut doc = Doc::new(samples::sample_project(), None);
        doc.dirty = false;
        StormwavesApp {
            docs: vec![doc],
            active: 0,
            theme: ThemeKind::Classic,
            platform: Platform::default(),
            tool: Tool::Select,
            interaction: Interaction::default(),
            palette_drag: None,
            status: "Ready".into(),
            clipboard: Vec::new(),
            context_node: None,
            error: None,
            styled: false,
            untitled: 1,
            canvas_rect: Rect::NOTHING,
        }
    }

    pub fn doc(&mut self) -> &mut Doc {
        if self.docs.is_empty() {
            self.docs.push(Doc::new(Project::default(), None));
        }
        self.active = self.active.min(self.docs.len() - 1);
        &mut self.docs[self.active]
    }

    pub fn new_project(&mut self) {
        let mut p = Project::default();
        p.title = format!("Untitled {}", self.untitled);
        self.untitled += 1;
        self.docs.push(Doc::new(p, None));
        self.active = self.docs.len() - 1;
    }

    fn receive(&mut self, ctx: &egui::Context) {
        for item in self.platform.take() {
            if let Err(e) = self.handle_incoming(ctx, item) {
                self.error = Some(e);
            }
        }
    }

    fn handle_incoming(&mut self, _ctx: &egui::Context, item: Incoming) -> Result<(), String> {
        match item.purpose {
            Purpose::OpenProject => {
                let text = String::from_utf8(item.bytes).map_err(|_| "The file is not a Stormwaves project (not text).".to_string())?;
                let project = Project::from_json(&text)?;
                // Replace an untouched empty first project.
                if self.docs.len() == 1 && !self.docs[0].dirty && self.docs[0].project.nodes.is_empty() {
                    self.docs.clear();
                }
                self.docs.push(Doc::new(project, item.path));
                self.active = self.docs.len() - 1;
                self.status = format!("Opened {}", item.name);
            }
            Purpose::Underlay => {
                let pages = underlay::page_count(&item.bytes);
                if pages == 0 {
                    return Err(format!("{} could not be read as an image or PDF.", item.name));
                }
                let raster = underlay::rasterize(&item.bytes, 0, 150.0)?;
                let doc = self.doc();
                let width = raster.width.max(1) as f64;
                let scale = 1200.0 / width;
                doc.edit("Load Background", |p| {
                    p.diagram.underlay = Some(stormwaves_core::project::Underlay {
                        file_name: item.name.clone(),
                        bytes: item.bytes.clone(),
                        page: 0,
                        origin: [0.0, 0.0],
                        scale,
                        rotation: 0.0,
                        feet_per_unit: None,
                        opacity: 0.85,
                        visible: true,
                        locked: false,
                    });
                });
                doc.underlay_key = None;
                doc.fit_pending = true;
                doc.windows.push(Window::Calibrate(dialogs::misc::CalibrateState::intro()));
                self.status = format!("Background loaded: {} ({} page{}). Use Diagram > Background > Calibrate Scale.", item.name, pages, if pages == 1 { "" } else { "s" });
            }
            Purpose::LinkCsv => {
                let text = String::from_utf8_lossy(&item.bytes).into_owned();
                let (times, columns) = stormwaves_core::link::parse_csv(&text)?;
                let sel = self.doc().selection.first().copied();
                let doc = self.doc();
                if let Some(id) = sel {
                    doc.edit("Link Import", |p| {
                        if let Some(Node { data: NodeData::Link(l), .. }) = p.node_mut(id) {
                            l.source = stormwaves_core::link::LinkSource::Table { name: item.name.clone(), times, columns, selected: None };
                        }
                    });
                }
            }
        }
        Ok(())
    }

    /// Rebuilds the underlay texture when the background changes.
    fn sync_underlay(&mut self, ctx: &egui::Context) {
        let doc = self.doc();
        let key = doc.project.diagram.underlay.as_ref().map(|u| (u.bytes.len(), u.page));
        if key == doc.underlay_key {
            return;
        }
        doc.underlay_key = key;
        doc.underlay_tex = None;
        if let Some(u) = &doc.project.diagram.underlay {
            match underlay::rasterize(&u.bytes, u.page, 150.0) {
                Ok(r) => doc.underlay_tex = Some(underlay::texture(ctx, "underlay", &r)),
                Err(e) => self.error = Some(format!("[470] Warning: Background image failed: {e}")),
            }
        }
    }

    pub fn save(&mut self, save_as: bool) {
        let platform = self.platform.clone();
        let doc = self.doc();
        let json = match doc.project.to_json() {
            Ok(j) => j,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let name = format!("{}.waves", sanitize(&doc.project.title));
        let path = if save_as { None } else { doc.path.clone() };
        match platform.save(&name, path.as_deref(), json.as_bytes(), "waves") {
            Ok(Some(p)) => {
                doc.path = Some(p.clone());
                doc.dirty = false;
                self.status = format!("Saved {p}");
            }
            Ok(None) => {
                if cfg!(target_arch = "wasm32") {
                    doc.dirty = false;
                    self.status = format!("Downloaded {name}");
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn open_window(&mut self, w: Window) {
        let doc = self.doc();
        // One window per node report/edit, and one of each settings window.
        let existing = doc.windows.iter().position(|o| o.same_kind(&w) && o.node() == w.node());
        if let Some(i) = existing {
            let old = doc.windows.remove(i);
            // Keep an existing report's tab; replace dialogs.
            if matches!(old, Window::Report(_)) {
                doc.windows.push(old);
                return;
            }
        }
        doc.windows.push(w);
    }

    fn open_report(&mut self, id: NodeId) {
        let Some(n) = self.doc().project.node(id).cloned() else { return };
        if n.node_type() == NodeType::Text {
            self.open_window(Window::Edit(dialogs::node::EditState::new(&n)));
            return;
        }
        self.open_window(Window::Report(reports::ReportState::new(id)));
    }

    fn open_edit(&mut self, id: NodeId) {
        if let Some(n) = self.doc().project.node(id).cloned() {
            self.open_window(Window::Edit(dialogs::node::EditState::new(&n)));
        }
    }

    fn delete_selection(&mut self) {
        let doc = self.doc();
        let sel = doc.selection.clone();
        if sel.is_empty() {
            return;
        }
        doc.edit("Node Delete", |p| {
            for id in &sel {
                p.remove_node(*id);
            }
        });
        doc.windows.retain(|w| w.node().is_none_or(|n| !sel.contains(&n)));
        doc.selection.clear();
    }

    fn copy_selection(&mut self) {
        let doc = self.doc();
        self.clipboard = doc.selection.iter().filter_map(|id| doc.project.node(*id).cloned()).collect();
        self.status = format!("Copied {} node(s)", self.clipboard.len());
    }

    fn paste(&mut self) {
        let nodes = self.clipboard.clone();
        if nodes.is_empty() {
            return;
        }
        let doc = self.doc();
        let mut ids = Vec::new();
        doc.edit("Node Paste", |p| {
            let offset = 30.0;
            let mut map = std::collections::HashMap::new();
            for n in &nodes {
                let id = p.add_node(n.node_type(), [n.pos[0] + offset, n.pos[1] + offset]);
                map.insert(n.id, id);
                if let Some(m) = p.node_mut(id) {
                    m.data = n.data.clone();
                    m.name = n.name.clone();
                    m.notes = n.notes.clone();
                    m.outline = n.outline.as_ref().map(|o| o.iter().map(|q| [q[0] + offset, q[1] + offset]).collect());
                }
                ids.push(id);
            }
            // Keep routing between pasted nodes.
            for n in &nodes {
                if let Some(&id) = map.get(&n.id) {
                    let outs: Vec<Option<NodeId>> = n.outflows.iter().map(|o| o.and_then(|t| map.get(&t).copied())).collect();
                    if let Some(m) = p.node_mut(id) {
                        for (k, o) in outs.into_iter().enumerate() {
                            if let Some(slot) = m.outflows.get_mut(k) {
                                *slot = o;
                            }
                        }
                    }
                }
            }
        });
        doc.selection = ids;
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if cmd && pressed(Key::Z) {
            if shift { self.doc().redo() } else { self.doc().undo() };
        } else if cmd && pressed(Key::Y) {
            self.doc().redo();
        } else if cmd && pressed(Key::S) {
            self.save(shift);
        } else if cmd && pressed(Key::O) {
            self.platform.pick(Purpose::OpenProject);
        } else if cmd && pressed(Key::N) {
            self.new_project();
        } else if cmd && pressed(Key::C) {
            self.copy_selection();
        } else if cmd && pressed(Key::X) {
            self.copy_selection();
            self.delete_selection();
        } else if cmd && pressed(Key::V) {
            self.paste();
        } else if cmd && pressed(Key::A) {
            let doc = self.doc();
            doc.selection = doc.project.nodes.iter().map(|n| n.id).collect();
        }
    }

    // ---------------------------------------------------------------- menus

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Project", |ui| {
                if ui.button("New").clicked() {
                    self.new_project();
                }
                if ui.button("Open...").clicked() {
                    self.platform.pick(Purpose::OpenProject);
                }
                if ui.button("Open Sample Project").clicked() {
                    self.docs.push(Doc::new(samples::sample_project(), None));
                    self.active = self.docs.len() - 1;
                }
                if ui.button("Close").clicked() {
                    self.close_active();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    self.save(false);
                }
                if ui.button("Save As...").clicked() {
                    self.save(true);
                }
                ui.separator();
                ui.menu_button("Export", |ui| {
                    if ui.button("Node Listing (text)...").clicked() {
                        self.export_text();
                        ui.close();
                    }
                    if ui.button("All Hydrographs (CSV)...").clicked() {
                        self.export_csv();
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Notes...").clicked() {
                    let st = dialogs::misc::NotesState::new(&self.doc().project);
                    self.open_window(Window::Notes(st));
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });
            ui.menu_button("Diagram", |ui| {
                if ui.button("Zoom In").clicked() {
                    self.zoom(1.25);
                }
                if ui.button("Zoom Out").clicked() {
                    self.zoom(0.8);
                }
                if ui.button("Zoom All").clicked() {
                    self.zoom_all(false);
                }
                if ui.button("Zoom Selected").clicked() {
                    self.zoom_all(true);
                }
                ui.separator();
                if ui.button("Select All").clicked() {
                    let doc = self.doc();
                    doc.selection = doc.project.nodes.iter().map(|n| n.id).collect();
                }
                let doc = self.doc();
                let mut grid = doc.project.diagram.show_grid;
                if ui.checkbox(&mut grid, "Show Grid").changed() {
                    doc.edit("Diagram Settings", |p| p.diagram.show_grid = grid);
                }
                let mut snap = doc.project.diagram.snap_to_grid;
                if ui.checkbox(&mut snap, "Snap to Grid").changed() {
                    doc.edit("Diagram Settings", |p| p.diagram.snap_to_grid = snap);
                }
                ui.separator();
                ui.menu_button("Background", |ui| {
                    if ui.button("Load Image or PDF...").clicked() {
                        self.platform.pick(Purpose::Underlay);
                        ui.close();
                    }
                    let has = self.doc().project.diagram.underlay.is_some();
                    if ui.add_enabled(has, egui::Button::new("Calibrate Scale")).clicked() {
                        self.tool = Tool::Calibrate { points: Vec::new() };
                        self.status = "Click two points a known distance apart on the background".into();
                        ui.close();
                    }
                    if ui.add_enabled(has, egui::Button::new("Move Background")).clicked() {
                        self.tool = Tool::MoveUnderlay;
                        ui.close();
                    }
                    if ui.add_enabled(has, egui::Button::new("Background Settings...")).clicked() {
                        self.open_window(Window::Underlay(dialogs::misc::UnderlayState::default()));
                        ui.close();
                    }
                    if ui.add_enabled(has, egui::Button::new("Remove Background")).clicked() {
                        self.doc().edit("Remove Background", |p| p.diagram.underlay = None);
                        ui.close();
                    }
                });
                if ui.button("Trace Subcatchment").clicked() {
                    self.start_trace();
                }
                if ui.button("Measure Length").clicked() {
                    self.tool = Tool::Measure { points: Vec::new() };
                    self.status = "Click points along the path; double-click or Enter to finish".into();
                }
                ui.separator();
                if ui.button("Diagram Settings...").clicked() {
                    let st = dialogs::misc::DiagramState::new(&self.doc().project);
                    self.open_window(Window::Diagram(st));
                }
            });
            ui.menu_button("Node", |ui| {
                ui.menu_button("Create", |ui| {
                    for t in [NodeType::Subcat, NodeType::Reach, NodeType::Pond, NodeType::Link, NodeType::Text] {
                        if ui.button(t.label()).clicked() {
                            self.create_node(t);
                            ui.close();
                        }
                    }
                });
                let sel = self.doc().selection.first().copied();
                if ui.add_enabled(sel.is_some(), egui::Button::new("Edit...")).clicked() {
                    if let Some(id) = sel {
                        self.open_edit(id);
                    }
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new("Report...")).clicked() {
                    if let Some(id) = sel {
                        self.open_report(id);
                    }
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new("Delete")).clicked() {
                    self.delete_selection();
                }
                ui.separator();
                if ui.add_enabled(sel.is_some(), egui::Button::new("Cut")).clicked() {
                    self.copy_selection();
                    self.delete_selection();
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new("Copy")).clicked() {
                    self.copy_selection();
                }
                if ui.add_enabled(!self.clipboard.is_empty(), egui::Button::new("Paste")).clicked() {
                    self.paste();
                }
                ui.separator();
                let doc = self.doc();
                let undo_label = doc.undo.last().map(|e| format!("Undo {}", e.label)).unwrap_or_else(|| "Undo".into());
                let redo_label = doc.redo.last().map(|e| format!("Redo {}", e.label)).unwrap_or_else(|| "Redo".into());
                let (can_undo, can_redo) = (!doc.undo.is_empty(), !doc.redo.is_empty());
                if ui.add_enabled(can_undo, egui::Button::new(undo_label)).clicked() {
                    self.doc().undo();
                }
                if ui.add_enabled(can_redo, egui::Button::new(redo_label)).clicked() {
                    self.doc().redo();
                }
                if ui.button("Undo History...").clicked() {
                    self.open_window(Window::Undo(dialogs::misc::UndoState::default()));
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Messages").clicked() {
                    self.open_window(Window::Messages);
                }
                if ui.button("Project Reports").clicked() {
                    self.open_window(Window::ProjectReport(reports::ProjectReportState::default()));
                }
                if ui.button("Comparison Report").clicked() {
                    let sel = self.doc().selection.clone();
                    self.open_window(Window::Compare(reports::CompareState::new(sel)));
                }
                ui.separator();
                if ui.button("Storm Distributions...").clicked() {
                    self.open_window(Window::Storms(reports::CurveState::storms()));
                }
                if ui.button("Unit Hydrographs...").clicked() {
                    self.open_window(Window::Storms(reports::CurveState::uhs()));
                }
                ui.separator();
                ui.menu_button("Theme", |ui| {
                    for k in ThemeKind::ALL {
                        if ui.radio(self.theme == k, k.label()).clicked() {
                            self.theme = k;
                            self.styled = false;
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button("Print", |ui| {
                if ui.button("Report to Text File...").clicked() {
                    self.export_text();
                    ui.close();
                }
                if ui.button("Hydrographs to CSV...").clicked() {
                    self.export_csv();
                    ui.close();
                }
            });
            ui.menu_button("Settings", |ui| {
                if ui.button("Calculation...").clicked() {
                    let st = dialogs::settings::SettingsState::new(&self.doc().project);
                    self.open_window(Window::Settings(st));
                }
                if ui.button("Units...").clicked() {
                    let st = dialogs::misc::UnitsState::new(&self.doc().project);
                    self.open_window(Window::Units(st));
                }
                if ui.button("Diagram...").clicked() {
                    let st = dialogs::misc::DiagramState::new(&self.doc().project);
                    self.open_window(Window::Diagram(st));
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About Stormwaves").clicked() {
                    self.open_window(Window::About);
                }
            });
        });
    }

    fn close_active(&mut self) {
        if self.docs.is_empty() {
            return;
        }
        self.docs.remove(self.active);
        if self.docs.is_empty() {
            self.new_project();
        }
        self.active = self.active.min(self.docs.len() - 1);
    }

    fn create_node(&mut self, t: NodeType) {
        let canvas = self.canvas_rect;
        let doc = self.doc();
        let c = if canvas.is_finite() { diagram::to_world(&doc.view, canvas, canvas.center()) } else { pos2(200.0, 200.0) };
        let mut id = 0;
        doc.edit("Node Create", |p| id = p.add_node(t, [c.x as f64, c.y as f64]));
        doc.selection = vec![id];
    }

    fn start_trace(&mut self) {
        let has_scale = self.doc().project.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit).is_some();
        self.tool = Tool::Trace { points: Vec::new() };
        self.status = if has_scale {
            "Click the outline corners; click the first point, double-click or press Enter to close (Backspace undoes a point)".into()
        } else {
            "No calibrated background: areas will use 1 diagram unit = 1 ft. Calibrate the background first for real areas.".into()
        };
    }

    /// Screen position of a diagram point (for tests and automation).
    pub fn world_to_screen(&mut self, w: Pos2) -> Pos2 {
        let canvas = self.canvas_rect;
        diagram::to_screen(&self.doc().view, canvas, w)
    }

    fn zoom(&mut self, f: f32) {
        let canvas = self.canvas_rect;
        if canvas.is_finite() {
            diagram::zoom_about(self.doc(), canvas, canvas.center(), f);
        }
    }

    fn zoom_all(&mut self, selected: bool) {
        let canvas = self.canvas_rect;
        if !canvas.is_finite() {
            return;
        }
        let doc = self.doc();
        let sel = doc.selection.clone();
        diagram::fit_view(doc, canvas, if selected && !sel.is_empty() { Some(&sel) } else { None });
    }

    fn export_text(&mut self) {
        let doc = self.doc();
        let mut lines = vec![doc.project.title.clone(), doc.results.event_heading.clone(), String::new()];
        lines.extend(stormwaves_core::report::node_listing(&doc.project, &doc.results));
        let ids: Vec<NodeId> = doc.project.nodes.iter().map(|n| n.id).collect();
        for id in ids {
            if let Some(n) = doc.project.node(id) {
                if n.node_type() == NodeType::Text {
                    continue;
                }
                lines.push(String::new());
                lines.push("-".repeat(100));
                lines.extend(stormwaves_core::report::node_summary(&doc.project, &doc.results, n));
            }
        }
        let name = format!("{} report.txt", sanitize(&doc.project.title));
        let text = lines.join("\n");
        if let Err(e) = self.platform.save(&name, None, text.as_bytes(), "txt") {
            self.error = Some(e);
        }
    }

    fn export_csv(&mut self) {
        let doc = self.doc();
        let r = &doc.results;
        let mut cols: Vec<(String, Vec<f64>)> = Vec::new();
        for n in &doc.project.nodes {
            if let Some(res) = r.node(n.id) {
                if n.node_type() != NodeType::Text {
                    cols.push((format!("{} Inflow (cfs)", n.label()), res.inflow.q.clone()));
                    cols.push((format!("{} Outflow (cfs)", n.label()), res.outflow.q.clone()));
                }
            }
        }
        let mut out = String::from("Time (hours)");
        for (h, _) in &cols {
            out.push(',');
            out.push_str(h);
        }
        out.push('\n');
        for i in 0..r.n {
            out.push_str(&format!("{:.4}", r.t0 + i as f64 * r.dt));
            for (_, v) in &cols {
                out.push_str(&format!(",{:.4}", v.get(i).copied().unwrap_or(0.0)));
            }
            out.push('\n');
        }
        let name = format!("{} hydrographs.csv", sanitize(&doc.project.title));
        if let Err(e) = self.platform.save(&name, None, out.as_bytes(), "csv") {
            self.error = Some(e);
        }
    }

    // ---------------------------------------------------------------- toolbars

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let tb = |ui: &mut Ui, g: Glyph, tip: &str| widgets::tool_button(ui, tip, false, |p, r, t| icons::draw_glyph(p, g, r, t)).clicked();
            if tb(ui, Glyph::ZoomIn, "Zoom In") {
                self.zoom(1.25);
            }
            if tb(ui, Glyph::ZoomOut, "Zoom Out") {
                self.zoom(0.8);
            }
            if tb(ui, Glyph::ZoomFit, "Zoom All") {
                self.zoom_all(false);
            }
            if tb(ui, Glyph::ZoomSelected, "Zoom Selected") {
                self.zoom_all(true);
            }
            separator(ui);
            if tb(ui, Glyph::Find, "Find node (select and zoom)") {
                self.zoom_all(true);
            }
            if tb(ui, Glyph::Delete, "Delete selected node(s)") {
                self.delete_selection();
            }
            if tb(ui, Glyph::Edit, "Edit selected node") {
                if let Some(id) = self.doc().selection.first().copied() {
                    self.open_edit(id);
                }
            }
            separator(ui);
            if tb(ui, Glyph::Report, "Node Report") {
                if let Some(id) = self.doc().selection.first().copied() {
                    self.open_report(id);
                }
            }
            if tb(ui, Glyph::Compare, "Comparison Report") {
                let sel = self.doc().selection.clone();
                self.open_window(Window::Compare(reports::CompareState::new(sel)));
            }
            if tb(ui, Glyph::ProjectReport, "Project Reports") {
                self.open_window(Window::ProjectReport(reports::ProjectReportState::default()));
            }
            separator(ui);
            if tb(ui, Glyph::Print, "Report to text file") {
                self.export_text();
            }
            if tb(ui, Glyph::PrintDiagram, "Hydrographs to CSV") {
                self.export_csv();
            }
            separator(ui);
            if tb(ui, Glyph::Notes, "Project Notes") {
                let st = dialogs::misc::NotesState::new(&self.doc().project);
                self.open_window(Window::Notes(st));
            }
            if tb(ui, Glyph::Calculator, "Calculation Settings") {
                let st = dialogs::settings::SettingsState::new(&self.doc().project);
                self.open_window(Window::Settings(st));
            }
            ui.add_space(6.0);
            // Rainfall event selector.
            let doc = self.doc();
            let events: Vec<(usize, String)> = doc.project.settings.events.iter().enumerate().map(|(i, e)| (i, e.name.clone())).collect();
            let mut cur = doc.project.settings.current_event;
            if widgets::combo(ui, "event", &mut cur, &events, 120.0) {
                doc.edit("Select Event", |p| p.settings.current_event = cur);
            }
            ui.add_space(6.0);
            if tb(ui, Glyph::Help, "About / Help") {
                self.open_window(Window::About);
            }
        });
    }

    fn right_toolbar(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let tool = self.tool.clone();
            let tb = |ui: &mut Ui, g: Glyph, tip: &str, on: bool| widgets::tool_button(ui, tip, on, |p, r, t| icons::draw_glyph(p, g, r, t)).clicked();
            if tb(ui, Glyph::Pointer, "Select", tool == Tool::Select) {
                self.tool = Tool::Select;
            }
            if tb(ui, Glyph::Units, "Units", false) {
                let st = dialogs::misc::UnitsState::new(&self.doc().project);
                self.open_window(Window::Units(st));
            }
            if tb(ui, Glyph::Calculator, "Calculation Settings", false) {
                let st = dialogs::settings::SettingsState::new(&self.doc().project);
                self.open_window(Window::Settings(st));
            }
            if tb(ui, Glyph::Messages, "Messages", false) {
                self.open_window(Window::Messages);
            }
            ui.add_space(6.0);
            if tb(ui, Glyph::Underlay, "Load background image or PDF", false) {
                self.platform.pick(Purpose::Underlay);
            }
            if tb(ui, Glyph::Calibrate, "Calibrate background scale", matches!(tool, Tool::Calibrate { .. })) {
                self.tool = Tool::Calibrate { points: Vec::new() };
                self.status = "Click two points a known distance apart on the background".into();
            }
            if tb(ui, Glyph::Trace, "Trace a subcatchment outline", matches!(tool, Tool::Trace { .. })) {
                self.start_trace();
            }
            if tb(ui, Glyph::ZoomFit, "Measure length", matches!(tool, Tool::Measure { .. })) {
                self.tool = Tool::Measure { points: Vec::new() };
                self.status = "Click points along the path; double-click or Enter to finish".into();
            }
            ui.add_space(6.0);
            if tb(ui, Glyph::Undo, "Undo", false) {
                self.doc().undo();
            }
            if tb(ui, Glyph::Redo, "Redo", false) {
                self.doc().redo();
            }
        });
    }

    fn palette(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for nt in [NodeType::Subcat, NodeType::Reach, NodeType::Pond, NodeType::Link, NodeType::Text] {
                let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 46.0), Sense::click_and_drag());
                let p = ui.painter();
                if resp.hovered() {
                    widgets::bevel(p, rect, true, &t);
                }
                let caption = match nt {
                    NodeType::Subcat => "Subcat",
                    NodeType::Reach => "Reach",
                    NodeType::Pond => "Pond",
                    NodeType::Link => "Link",
                    NodeType::Text => "Text",
                };
                icons::draw_node(p, nt, rect.center(), 44.0, "", false, false, &t);
                p.text(rect.center() + vec2(0.0, if nt == NodeType::Pond { 6.0 } else { 0.0 }), Align2::CENTER_CENTER, caption, FontId::proportional(11.0), t.node_text);
                if resp.drag_started() {
                    self.palette_drag = Some(nt);
                }
                if resp.clicked() {
                    self.create_node(nt);
                }
                let resp = resp.on_hover_text(format!("Drag onto the diagram to create a {}", nt.label().to_lowercase()));
                let _ = resp;
            }
        });
        if let (Some(nt), Some(pos)) = (self.palette_drag, ui.ctx().pointer_hover_pos()) {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("palette-drag"));
            let p = ui.ctx().layer_painter(layer);
            let d = (self.doc().project.diagram.symbol_size as f32 * self.doc().view.zoom).clamp(20.0, 120.0);
            icons::draw_node(&p, nt, pos, d, "", false, false, &t);
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let doc = self.doc();
        let count = doc.project.nodes.iter().filter(|n| n.node_type() != NodeType::Text).count();
        let world = self.interaction.hover_world;
        let doc = &self.docs[self.active.min(self.docs.len() - 1)];
        let scale = doc.project.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit);
        let (x, y) = match (world, scale) {
            (Some(w), Some(s)) => (format!("X={:.1}'", w.x as f64 * s), format!("Y={:.1}'", w.y as f64 * s)),
            (Some(w), None) => (format!("X={:.2}", w.x / 100.0), format!("Y={:.2}", w.y / 100.0)),
            _ => (String::new(), String::new()),
        };
        let errors = doc.results.messages.iter().filter(|m| m.severity >= stormwaves_core::calc::Severity::Warning).count();
        let tool = self.tool.name();
        ui.horizontal(|ui| {
            let cell = |ui: &mut Ui, text: &str, w: f32| {
                let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
                widgets::bevel(ui.painter(), r, false, &t);
                ui.painter().with_clip_rect(r.shrink(2.0)).text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(12.0), t.text);
            };
            let w = ui.available_width() - 2.0 * 96.0 - 150.0 - 120.0 - 16.0;
            cell(ui, &self.status, w.max(100.0));
            cell(ui, tool, 120.0);
            cell(ui, &format!("{count} nodes, {errors} warnings"), 150.0);
            cell(ui, &x, 96.0);
            cell(ui, &y, 96.0);
        });
    }

    fn project_tabs(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut switch = None;
            for (i, d) in self.docs.iter().enumerate() {
                let label = format!("{}{}", d.title(), if d.dirty { " *" } else { "" });
                let w = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), FontId::proportional(12.5), t.text).size().x) + 18.0;
                let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
                let p = ui.painter();
                let active = i == self.active;
                p.rect_filled(r, 0.0, if active { t.face } else { t.face.gamma_multiply(0.94) });
                widgets::bevel(p, r, !active, &t);
                p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.5), t.text);
                if resp.clicked() {
                    switch = Some(i);
                }
            }
            if let Some(i) = switch {
                self.active = i;
            }
        });
    }

    // ---------------------------------------------------------------- canvas actions

    fn canvas_actions(&mut self, actions: Vec<CanvasAction>, ctx: &egui::Context) {
        for a in actions {
            match a {
                CanvasAction::OpenReport(id) => {
                    let described = self.doc().results.node(id).is_some_and(|r| r.inflow.has_flow() || r.outflow.has_flow());
                    if described { self.open_report(id) } else { self.open_edit(id) }
                }
                CanvasAction::OpenEdit(id) => self.open_edit(id),
                CanvasAction::Delete => self.delete_selection(),
                CanvasAction::ContextNode(id) => self.context_node = ctx.pointer_hover_pos().map(|p| (id, p)),
                CanvasAction::Calibrated(a, b) => {
                    let doc = self.doc();
                    doc.windows.push(Window::Calibrate(dialogs::misc::CalibrateState::distance(a, b)));
                }
                CanvasAction::Traced(points) => self.finish_trace(points),
                CanvasAction::Measured(points) => {
                    let len = diagram::polyline_length(&points) as f64;
                    let doc = self.doc();
                    let fpu = doc.project.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit).unwrap_or(1.0);
                    let u = doc.project.units;
                    self.status = format!("Measured length = {}", u.fmt_len(len * fpu));
                }
                CanvasAction::Convert(id, t) => self.convert(id, t),
                CanvasAction::ClearOutflows(id) => self.doc().edit("Node Reroute", |p| {
                    if let Some(n) = p.node_mut(id) {
                        n.outflows = [None; 3];
                    }
                }),
            }
        }
    }

    fn finish_trace(&mut self, points: Vec<Pos2>) {
        let doc = self.doc();
        let fpu = doc.project.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit).unwrap_or(1.0);
        let area_sf = diagram::polygon_area(&points) as f64 * fpu * fpu;
        let c = diagram::polygon_centroid(&points);
        let outline: Vec<[f64; 2]> = points.iter().map(|p| [p.x as f64, p.y as f64]).collect();
        let mut id = 0;
        doc.edit("Trace Subcatchment", |p| {
            id = p.add_node(NodeType::Subcat, [c.x as f64, c.y as f64]);
            if let Some(n) = p.node_mut(id) {
                n.outline = Some(outline.clone());
                if let NodeData::Subcat(sc) = &mut n.data {
                    sc.areas.push(stormwaves_core::cn::SubArea::new(area_sf, 61.0, "Open Space, Good, HSG B"));
                }
            }
        });
        doc.selection = vec![id];
        let u = doc.project.units;
        self.status = format!("Created subcatchment with area {} (traced). Edit it to set ground cover and Tc.", u.fmt_area(area_sf));
        self.open_edit(id);
    }

    fn convert(&mut self, id: NodeId, t: NodeType) {
        self.doc().edit("Node Convert", |p| {
            let number = p.next_number(t);
            if let Some(n) = p.node_mut(id) {
                n.data = NodeData::empty(t);
                n.number = number;
                if t.outflow_count() < 3 {
                    for k in t.outflow_count()..3 {
                        n.outflows[k] = None;
                    }
                }
            }
        });
        self.doc().windows.retain(|w| w.node() != Some(id));
    }

    fn context_menu(&mut self, ctx: &egui::Context) {
        let Some((id, pos)) = self.context_node else { return };
        let Some(n) = self.doc().project.node(id).cloned() else {
            self.context_node = None;
            return;
        };
        let t = Tokens::get(ctx);
        let mut close = false;
        let resp = egui::Area::new(egui::Id::new("node-context")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.face).show(ui, |ui| {
                ui.set_min_width(150.0);
                ui.label(egui::RichText::new(n.title()).strong());
                ui.separator();
                if ui.button("Edit...").clicked() {
                    self.open_edit(id);
                    close = true;
                }
                if ui.button("Report...").clicked() {
                    self.open_report(id);
                    close = true;
                }
                if ui.button("Delete").clicked() {
                    self.delete_selection();
                    close = true;
                }
                if ui.button("Copy").clicked() {
                    self.copy_selection();
                    close = true;
                }
                if ui.button("Remove Outflow Routing").clicked() {
                    self.canvas_actions(vec![CanvasAction::ClearOutflows(id)], ctx);
                    close = true;
                }
                if matches!(n.node_type(), NodeType::Reach | NodeType::Pond | NodeType::Link) {
                    ui.menu_button("Convert to", |ui| {
                        for to in [NodeType::Reach, NodeType::Pond, NodeType::Link] {
                            if to != n.node_type() && ui.button(to.label()).clicked() {
                                self.convert(id, to);
                                close = true;
                            }
                        }
                    });
                }
                if n.node_type() == NodeType::Subcat && n.outline.is_some() {
                    if ui.button("Update Area from Outline").clicked() {
                        self.area_from_outline(id);
                        close = true;
                    }
                }
            });
        });
        let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer();
        if close || clicked_elsewhere || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.context_node = None;
        }
    }

    fn area_from_outline(&mut self, id: NodeId) {
        let doc = self.doc();
        let fpu = doc.project.diagram.underlay.as_ref().and_then(|u| u.feet_per_unit).unwrap_or(1.0);
        doc.edit("Area from Outline", |p| {
            if let Some(n) = p.node_mut(id) {
                let Some(o) = &n.outline else { return };
                let pts: Vec<Pos2> = o.iter().map(|q| pos2(q[0] as f32, q[1] as f32)).collect();
                let area = diagram::polygon_area(&pts) as f64 * fpu * fpu;
                if let NodeData::Subcat(sc) = &mut n.data {
                    let current: f64 = sc.areas.iter().map(|a| a.area).sum();
                    if sc.areas.is_empty() {
                        sc.areas.push(stormwaves_core::cn::SubArea::new(area, 61.0, "Open Space, Good, HSG B"));
                    } else if current > 0.0 {
                        // Scale existing sub-areas proportionally to the outline area.
                        for a in &mut sc.areas {
                            a.area *= area / current;
                        }
                    }
                }
            }
        });
    }

    fn window_actions(&mut self, actions: Vec<WindowAction>) {
        for a in actions {
            match a {
                WindowAction::OpenReport(id) => self.open_report(id),
                WindowAction::OpenEdit(id) => self.open_edit(id),
                WindowAction::OpenWindow(w) => self.open_window(w),
                WindowAction::PickCsvForLink(id) => {
                    self.doc().selection = vec![id];
                    self.platform.pick(Purpose::LinkCsv);
                }
                WindowAction::Status(s) => self.status = s,
            }
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        let Some(msg) = self.error.clone() else { return };
        let mut open = true;
        crate::frame::Win::new("Stormwaves").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(&msg);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if widgets::button(ui, "OK", 80.0).clicked() {
                    open = false;
                }
            });
        });
        if !open {
            self.error = None;
        }
    }

    /// Draws the whole application.
    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.styled {
            Tokens::for_kind(self.theme).apply(&ctx);
            self.styled = true;
        }
        let t = Tokens::get(&ctx);
        self.receive(&ctx);
        self.sync_underlay(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("menu").frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::symmetric(2, 1))).show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::symmetric(4, 3))).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::symmetric(2, 2))).show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("tabs").frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin { left: 64, right: 2, top: 0, bottom: 2 })).show(ui, |ui| self.project_tabs(ui));
        egui::Panel::left("palette").exact_size(64.0).resizable(false).frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| self.palette(ui));
        egui::Panel::right("settings-bar").exact_size(34.0).resizable(false).frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| self.right_toolbar(ui));
        let mut actions = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.face).inner_margin(egui::Margin::same(2))).show(ui, |ui| {
            let outer = ui.available_rect_before_wrap();
            widgets::bevel(ui.painter(), outer, false, &t);
            let inner = outer.shrink(2.0);
            self.canvas_rect = inner;
            frame::set_canvas(&ctx, inner);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            let idx = self.active.min(self.docs.len().saturating_sub(1));
            if let Some(doc) = self.docs.get_mut(idx) {
                actions = diagram::show(&mut child, doc, &mut self.tool, &mut self.interaction, &mut self.palette_drag);
            }
            if self.palette_drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
                self.palette_drag = None;
            }
        });
        self.canvas_actions(actions, &ctx);
        self.context_menu(&ctx);
        let idx = self.active.min(self.docs.len().saturating_sub(1));
        let wactions = match self.docs.get_mut(idx) {
            Some(doc) => windows::show_all(&ctx, doc),
            None => Vec::new(),
        };
        self.window_actions(wactions);
        self.error_dialog(&ctx);
        let title = format!("{} - Stormwaves", self.doc().title());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        let _ = Stroke::NONE;
        let _ = Color32::BLACK;
    }
}

fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(8.0, 24.0), Sense::hover());
    let x = r.center().x;
    ui.painter().line_segment([pos2(x, r.min.y + 2.0), pos2(x, r.max.y - 2.0)], Stroke::new(1.0, t.shadow));
    if t.bevel {
        ui.painter().line_segment([pos2(x + 1.0, r.min.y + 2.0), pos2(x + 1.0, r.max.y - 2.0)], Stroke::new(1.0, t.light));
    }
}

fn sanitize(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.trim().is_empty() { "project".into() } else { s.trim().to_string() }
}

impl eframe::App for StormwavesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

/// Drives the app from a short command (used by the offscreen snapshot tool and tests):
/// `report:1P`, `edit:1S`, `tab:<n>`, `select:1R`, `settings`, `settings-tab:<n>`, `messages`,
/// `project-report`, `storms`, `uhs`, `units`, `theme:<classic|modern|dark>`, `compare`, `new`.
pub fn script(app: &mut StormwavesApp, cmd: &str) {
    let (verb, arg) = cmd.split_once(':').unwrap_or((cmd, ""));
    let find = |app: &mut StormwavesApp, label: &str| app.doc().project.by_label(label).map(|n| n.id);
    match verb.trim() {
        "report" => {
            if let Some(id) = find(app, arg) {
                app.open_report(id);
            }
        }
        "edit" => {
            if let Some(id) = find(app, arg) {
                app.open_edit(id);
            }
        }
        "select" => {
            if let Some(id) = find(app, arg) {
                app.doc().selection = vec![id];
            }
        }
        "tab" => {
            let n: usize = arg.parse().unwrap_or(0);
            if let Some(w) = app.doc().windows.last_mut() {
                match w {
                    Window::Report(r) => {
                        r.tab = [reports::Tab::Summary, reports::Tab::Hydrograph, reports::Tab::Discharge, reports::Tab::Storage, reports::Tab::Events, reports::Tab::Sizing][n.min(5)];
                    }
                    Window::Edit(e) => e.tab = n,
                    Window::Settings(s) => s.tab = n,
                    Window::ProjectReport(s) => s.tab = n,
                    _ => {}
                }
            }
        }
        "table" => {
            if let Some(Window::Report(r)) = app.doc().windows.last_mut() {
                r.table = true;
            }
        }
        "settings" => {
            let st = dialogs::settings::SettingsState::new(&app.doc().project);
            app.open_window(Window::Settings(st));
        }
        "units" => {
            let st = dialogs::misc::UnitsState::new(&app.doc().project);
            app.open_window(Window::Units(st));
        }
        "messages" => app.open_window(Window::Messages),
        "project-report" => app.open_window(Window::ProjectReport(reports::ProjectReportState::default())),
        "storms" => app.open_window(Window::Storms(reports::CurveState::storms())),
        "uhs" => app.open_window(Window::Storms(reports::CurveState::uhs())),
        "compare" => {
            let sel: Vec<NodeId> = app.doc().project.nodes.iter().filter(|n| n.node_type() != NodeType::Text).map(|n| n.id).take(3).collect();
            app.open_window(Window::Compare(reports::CompareState::new(sel)));
        }
        "theme" => {
            app.theme = match arg {
                "modern" => ThemeKind::Modern,
                "dark" => ThemeKind::ModernDark,
                _ => ThemeKind::Classic,
            };
            app.styled = false;
        }
        "new" => app.new_project(),
        "trace" => app.start_trace(),
        "calibrate" => app.tool = Tool::Calibrate { points: Vec::new() },
        "measure" => app.tool = Tool::Measure { points: Vec::new() },
        _ => {}
    }
}
