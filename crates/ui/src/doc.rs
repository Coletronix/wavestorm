//! An open project: data, undo history, results and view state.

use crate::windows::Window;
use egui::{TextureHandle, Vec2};
use stormwaves_core::calc::{self, Results};
use stormwaves_core::project::{NodeId, Project};

/// Diagram view: world → screen is `canvas.min + offset + world * zoom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        View { offset: egui::vec2(40.0, 40.0), zoom: 1.0 }
    }
}

/// One undo step.
#[derive(Clone)]
pub struct UndoEntry {
    pub label: String,
    pub project: Project,
}

pub struct Doc {
    pub project: Project,
    pub path: Option<String>,
    pub dirty: bool,
    pub undo: Vec<UndoEntry>,
    pub redo: Vec<UndoEntry>,
    pub max_undo: usize,
    pub results: Results,
    /// Results for every event (Events tabs), computed on demand.
    pub all_results: Option<Vec<Results>>,
    pub view: View,
    pub selection: Vec<NodeId>,
    pub windows: Vec<Window>,
    pub underlay_tex: Option<TextureHandle>,
    /// Bytes length + page the texture was made from (to detect changes).
    pub underlay_key: Option<(usize, usize)>,
    pub fit_pending: bool,
}

impl Doc {
    pub fn new(project: Project, path: Option<String>) -> Doc {
        let results = calc::calculate(&project);
        Doc {
            project,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            max_undo: 50,
            results,
            all_results: None,
            view: View::default(),
            selection: Vec::new(),
            windows: Vec::new(),
            underlay_tex: None,
            underlay_key: None,
            fit_pending: true,
        }
    }

    pub fn title(&self) -> String {
        self.project.title.clone()
    }

    /// Applies an edit as one undo step and recalculates.
    pub fn edit(&mut self, label: &str, f: impl FnOnce(&mut Project)) {
        let before = self.project.clone();
        f(&mut self.project);
        if self.project == before {
            return;
        }
        self.undo.push(UndoEntry { label: label.into(), project: before });
        if self.undo.len() > self.max_undo {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.changed();
    }

    /// Records an undo step for a change already made (e.g. during a drag).
    pub fn commit_from(&mut self, label: &str, before: Project) {
        if self.project == before {
            return;
        }
        self.undo.push(UndoEntry { label: label.into(), project: before });
        if self.undo.len() > self.max_undo {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.changed();
    }

    /// Marks the project changed: dirty, recalculated.
    pub fn changed(&mut self) {
        self.dirty = true;
        self.recalc();
    }

    pub fn recalc(&mut self) {
        self.results = calc::calculate(&self.project);
        self.all_results = None;
    }

    pub fn all_results(&mut self) -> &[Results] {
        if self.all_results.is_none() {
            self.all_results = Some(calc::calculate_all(&self.project));
        }
        self.all_results.as_deref().unwrap_or(&[])
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else { return false };
        let now = std::mem::replace(&mut self.project, e.project);
        self.redo.push(UndoEntry { label: e.label, project: now });
        self.selection.retain(|id| self.project.node(*id).is_some());
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        let now = std::mem::replace(&mut self.project, e.project);
        self.undo.push(UndoEntry { label: e.label, project: now });
        self.selection.retain(|id| self.project.node(*id).is_some());
        self.changed();
        true
    }

    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selection.contains(&id)
    }
}
