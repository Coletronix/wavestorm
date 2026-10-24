//! Recovery of unsaved work. Shortly after every change, the open projects with unsaved changes
//! are copied to local storage (browser `localStorage`, or a file in the configuration folder on
//! the desktop). If the page or window is closed before they are saved or exported, the next
//! session opens them again. Saving a project (or downloading it in the browser) clears its copy.

use crate::doc::Doc;
use crate::platform::store;
use serde::{Deserialize, Serialize};
use wavestorm_core::project::Project;

const KEY: &str = "recovery";

/// Seconds after the last change before unsaved work is copied out.
pub const DELAY: f64 = 1.0;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    projects: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    /// Where the project was saved last, if anywhere.
    path: Option<String>,
    #[serde(default)]
    hcp: bool,
    /// The project as `.waves` JSON.
    project: String,
}

/// The JSON kept for `docs` (only those with unsaved changes), or None when nothing is unsaved.
/// `strip_background` leaves out background images, for when the full copy does not fit.
pub fn snapshot(docs: &[Doc], strip_background: bool) -> Option<String> {
    let projects: Vec<Entry> = docs
        .iter()
        .filter(|d| d.dirty)
        .filter_map(|d| {
            let mut p = d.project.clone();
            if strip_background {
                p.diagram.underlay = None;
            }
            Some(Entry { path: d.path.clone(), hcp: d.hcp, project: p.to_json().ok()? })
        })
        .collect();
    if projects.is_empty() { None } else { serde_json::to_string(&Snapshot { projects }).ok() }
}

/// The projects in a stored snapshot, as documents marked unsaved. Unreadable entries are
/// skipped.
pub fn restore(json: &str) -> Vec<Doc> {
    let Ok(snap) = serde_json::from_str::<Snapshot>(json) else { return Vec::new() };
    snap.projects
        .into_iter()
        .filter_map(|e| {
            let project = Project::from_json(&e.project).ok()?;
            let mut d = Doc::new(project, e.path);
            d.dirty = true;
            d.hcp = e.hcp;
            Some(d)
        })
        .collect()
}

/// Projects left unsaved by the last session (empty when there are none).
pub fn take() -> Vec<Doc> {
    store::read(KEY).map(|t| restore(&t)).unwrap_or_default()
}

/// Watches the open documents and copies unsaved work out [`DELAY`] seconds after it changes.
#[derive(Default)]
pub struct Recovery {
    /// Per document: calculation serial (bumped by every edit) and dirty flag.
    signature: Vec<(u64, bool)>,
    changed_at: Option<f64>,
    /// The last problem reported, so it is shown once.
    reported: Option<String>,
}

impl Recovery {
    /// Call once per frame. Returns a status message when keeping the copy failed or had to
    /// leave something out.
    pub fn update(&mut self, ctx: &egui::Context, docs: &[Doc]) -> Option<String> {
        let now = ctx.input(|i| i.time);
        let sig: Vec<(u64, bool)> = docs.iter().map(|d| (d.calc_serial, d.dirty)).collect();
        if sig != self.signature {
            self.signature = sig;
            self.changed_at = Some(now);
        }
        let t0 = self.changed_at?;
        let wait = DELAY - (now - t0);
        if wait > 0.0 {
            // One wake-up when the delay ends, so the copy is made even if nothing else happens.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            return None;
        }
        self.changed_at = None;
        let problem = write(docs);
        if problem != self.reported {
            self.reported = problem.clone();
            return problem;
        }
        None
    }
}

/// Stores the unsaved work now (or clears the stored copy when nothing is unsaved).
fn write(docs: &[Doc]) -> Option<String> {
    let Some(full) = snapshot(docs, false) else {
        store::remove(KEY);
        return None;
    };
    if store::try_write(KEY, &full).is_ok() {
        return None;
    }
    match snapshot(docs, true).map(|t| store::try_write(KEY, &t)) {
        Some(Ok(())) => Some("Unsaved changes are kept for recovery without their background image (too large for local storage).".into()),
        _ => Some("Could not keep a recovery copy of unsaved changes (local storage is full or unavailable).".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsaved_projects_round_trip() {
        let mut saved = Doc::new(crate::samples::sample_project(), Some("/tmp/a.waves".into()));
        saved.dirty = false;
        let mut changed = Doc::new(crate::samples::sample_project(), Some("/tmp/b.waves".into()));
        changed.dirty = true;
        changed.project.title = "Changed".into();
        assert!(snapshot(std::slice::from_ref(&saved), false).is_none(), "nothing unsaved, nothing kept");
        let json = snapshot(&[saved, changed], false).unwrap();
        let back = restore(&json);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].project.title, "Changed");
        assert_eq!(back[0].path.as_deref(), Some("/tmp/b.waves"));
        assert!(back[0].dirty, "restored work is still unsaved");
        assert!(restore("not json").is_empty());
    }
}
