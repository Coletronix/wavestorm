//! Floating windows owned by a document: node reports, edit dialogs, settings and listings.

use crate::dialogs;
use crate::doc::Doc;
use crate::reports;
use stormwaves_core::project::NodeId;

/// A floating window.
pub enum Window {
    Report(reports::ReportState),
    Edit(dialogs::node::EditState),
    Settings(dialogs::settings::SettingsState),
    Units(dialogs::misc::UnitsState),
    Diagram(dialogs::misc::DiagramState),
    Notes(dialogs::misc::NotesState),
    ProjectReport(reports::ProjectReportState),
    Compare(reports::CompareState),
    Messages,
    About,
    Calibrate(dialogs::misc::CalibrateState),
    Underlay(dialogs::misc::UnderlayState),
    Undo(dialogs::misc::UndoState),
    Storms(reports::CurveState),
    Print(dialogs::misc::PrintState),
    StormEdit(dialogs::storm::StormEditor),
    Idf(dialogs::storm::IdfEditor),
    LandUse(dialogs::misc::LandUseState),
    Help(crate::help::HelpState),
    General(dialogs::misc::GeneralState),
}

impl Window {
    /// Node this window is about, if any.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Window::Report(r) => Some(r.node),
            Window::Edit(e) => Some(e.node),
            _ => None,
        }
    }

    pub fn same_kind(&self, other: &Window) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// Things windows ask the app to do.
pub enum WindowAction {
    OpenReport(NodeId),
    OpenEdit(NodeId),
    OpenWindow(Window),
    PickCsvForLink(NodeId),
    Status(String),
    Help(&'static str),
    PrintPdf(stormwaves_core::printing::PrintOptions),
    SetPrefs(crate::prefs::Prefs),
}

/// Shows every window of the document; returns follow-up actions.
pub fn show_all(ctx: &egui::Context, doc: &mut Doc) -> Vec<WindowAction> {
    let mut actions = Vec::new();
    let mut windows = std::mem::take(&mut doc.windows);
    let mut keep = Vec::with_capacity(windows.len());
    for mut w in windows.drain(..) {
        let open = match &mut w {
            Window::Report(s) => reports::show_node_report(ctx, doc, s, &mut actions),
            Window::Edit(s) => dialogs::node::show(ctx, doc, s, &mut actions),
            Window::Settings(s) => dialogs::settings::show(ctx, doc, s, &mut actions),
            Window::Units(s) => dialogs::misc::units(ctx, doc, s),
            Window::Diagram(s) => dialogs::misc::diagram(ctx, doc, s),
            Window::Notes(s) => dialogs::misc::notes(ctx, doc, s),
            Window::ProjectReport(s) => reports::show_project_report(ctx, doc, s),
            Window::Compare(s) => reports::show_compare(ctx, doc, s),
            Window::Messages => reports::show_messages(ctx, doc, &mut actions),
            Window::About => dialogs::misc::about(ctx),
            Window::Calibrate(s) => dialogs::misc::calibrate(ctx, doc, s),
            Window::Underlay(s) => dialogs::misc::underlay(ctx, doc, s, &mut actions),
            Window::Undo(s) => dialogs::misc::undo_history(ctx, doc, s),
            Window::Storms(s) => reports::show_curves(ctx, doc, s),
            Window::StormEdit(s) => dialogs::storm::show(ctx, doc, s),
            Window::Idf(s) => dialogs::storm::idf(ctx, doc, s),
            Window::LandUse(s) => dialogs::misc::land_use(ctx, doc, s),
            Window::Help(s) => crate::help::show(ctx, s),
            Window::General(s) => dialogs::misc::general(ctx, s, &mut actions),
            Window::Print(s) => {
                let (keep, go) = dialogs::misc::print(ctx, s);
                if let Some(o) = go {
                    actions.push(WindowAction::PrintPdf(o));
                }
                keep
            }
        };
        if open {
            keep.push(w);
        }
    }
    // Windows opened while showing (e.g. by a dialog) were pushed onto doc.windows.
    keep.append(&mut doc.windows);
    doc.windows = keep;
    actions
}
