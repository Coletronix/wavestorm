//! The session: the projects open when the app last ran, so the next start reopens them, and
//! unsaved work is never lost. Shortly after every change the open projects are recorded in
//! local storage (browser `localStorage`, or a file in the configuration folder on the
//! desktop): a project saved to a file by its path, one with unsaved changes (or no file, as
//! always in the browser) with its contents. At the next start they are reopened; files that
//! have since gone missing are skipped, and with nothing to reopen the app starts with no
//! project open.

use crate::doc::Doc;
use crate::platform::store;
use serde::{Deserialize, Serialize};
use wavestorm_core::project::Project;

const KEY: &str = "session";
/// The key used by 0.3.0, which kept only projects with unsaved changes.
const OLD_KEY: &str = "recovery";

/// Seconds after the last change before the session is recorded.
pub const DELAY: f64 = 1.0;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    projects: Vec<Entry>,
    /// The project tab that was in front.
    #[serde(default)]
    active: usize,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    /// Where the project was saved last, if anywhere.
    path: Option<String>,
    #[serde(default)]
    hcp: bool,
    #[serde(default = "yes")]
    dirty: bool,
    /// The project as `.waves` JSON, when it has unsaved changes or no file.
    #[serde(default)]
    project: Option<String>,
}

fn yes() -> bool {
    true
}

/// The JSON recording `docs` (with tab `active` in front), or None when nothing is open.
/// `strip_background` leaves background images out of kept contents, for when the full record
/// does not fit.
pub fn snapshot(docs: &[Doc], active: usize, strip_background: bool) -> Option<String> {
    let projects: Vec<Entry> = docs
        .iter()
        .map(|d| {
            let keep = d.dirty || d.path.is_none();
            let project = keep
                .then(|| {
                    let mut p = d.project.clone();
                    if strip_background {
                        p.diagram.underlay = None;
                    }
                    p.to_json().ok()
                })
                .flatten();
            Entry { path: d.path.clone(), hcp: d.hcp, dirty: d.dirty, project }
        })
        .collect();
    if projects.is_empty() { None } else { serde_json::to_string(&Snapshot { projects, active }).ok() }
}

/// The projects of a recorded session and the tab to bring to the front. `read` loads a file
/// for projects recorded by path; unreadable entries and missing files are skipped.
pub fn restore(json: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> (Vec<Doc>, usize) {
    let Ok(snap) = serde_json::from_str::<Snapshot>(json) else { return (Vec::new(), 0) };
    let mut docs = Vec::new();
    let mut active = 0;
    for (i, e) in snap.projects.into_iter().enumerate() {
        let doc = match (e.project, &e.path) {
            (Some(text), _) => Project::from_json(&text).ok().map(|p| {
                let mut d = Doc::new(p, e.path.clone());
                d.dirty = e.dirty;
                d.hcp = e.hcp;
                d
            }),
            (None, Some(path)) => read(path).and_then(|bytes| crate::project_from_bytes(path, bytes).ok()).map(|(p, hcp, _)| {
                let mut d = Doc::new(p, Some(path.clone()));
                d.dirty = false;
                d.hcp = hcp;
                d
            }),
            (None, None) => None,
        };
        if let Some(d) = doc {
            if i == snap.active {
                active = docs.len();
            }
            docs.push(d);
        }
    }
    (docs, active)
}

/// The projects open at the end of the last session (empty when none could be reopened), and
/// the tab to bring to the front.
pub fn take() -> (Vec<Doc>, usize) {
    let Some(text) = store::read(KEY).or_else(|| store::read(OLD_KEY)) else { return (Vec::new(), 0) };
    restore(&text, |path| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read(path).ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            None
        }
    })
}

/// Watches the open documents and records the session [`DELAY`] seconds after it changes.
#[derive(Default)]
pub struct Recovery {
    /// Per document: calculation serial (bumped by every edit), dirty flag and path; then the
    /// tab in front.
    signature: Vec<(u64, bool, Option<String>)>,
    active: usize,
    changed_at: Option<f64>,
    /// The last problem reported, so it is shown once.
    reported: Option<String>,
}

impl Recovery {
    /// Call once per frame. Returns a status message when recording the session failed or had
    /// to leave something out.
    pub fn update(&mut self, ctx: &egui::Context, docs: &[Doc], active: usize) -> Option<String> {
        let now = ctx.input(|i| i.time);
        let sig: Vec<(u64, bool, Option<String>)> = docs.iter().map(|d| (d.calc_serial, d.dirty, d.path.clone())).collect();
        if sig != self.signature || active != self.active {
            self.signature = sig;
            self.active = active;
            self.changed_at = Some(now);
        }
        let t0 = self.changed_at?;
        let wait = DELAY - (now - t0);
        if wait > 0.0 {
            // One wake-up when the delay ends, so the record is made even if nothing else happens.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            return None;
        }
        self.changed_at = None;
        let problem = write(docs, active);
        if problem != self.reported {
            self.reported = problem.clone();
            return problem;
        }
        None
    }
}

/// Records the session now (or clears the record when nothing is open).
fn write(docs: &[Doc], active: usize) -> Option<String> {
    store::remove(OLD_KEY);
    let Some(full) = snapshot(docs, active, false) else {
        store::remove(KEY);
        return None;
    };
    if store::try_write(KEY, &full).is_ok() {
        return None;
    }
    match snapshot(docs, active, true).map(|t| store::try_write(KEY, &t)) {
        Some(Ok(())) => Some("Unsaved changes are kept for recovery without their background image (too large for local storage).".into()),
        _ => Some("Could not keep a recovery copy of unsaved changes (local storage is full or unavailable).".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(path: &str) -> Option<Vec<u8>> {
        (path == "/p/a.waves").then(|| crate::samples::sample_project().to_json().unwrap().into_bytes())
    }

    #[test]
    fn session_round_trip() {
        let mut saved = Doc::new(crate::samples::sample_project(), Some("/p/a.waves".into()));
        saved.dirty = false;
        let mut changed = Doc::new(crate::samples::sample_project(), Some("/p/b.waves".into()));
        changed.dirty = true;
        changed.project.title = "Changed".into();
        let mut gone = Doc::new(crate::samples::sample_project(), Some("/p/gone.waves".into()));
        gone.dirty = false;
        let mut untitled = Doc::new(crate::samples::sample_project(), None);
        untitled.dirty = false;
        untitled.project.title = "Untitled".into();
        assert!(snapshot(&[], 0, false).is_none(), "nothing open, nothing recorded");
        let json = snapshot(&[gone, saved, changed, untitled], 2, false).unwrap();
        let (back, active) = restore(&json, files);
        // The missing file is skipped; the saved file is read from disk (clean); the changed and
        // untitled projects come from their kept contents.
        let titles: Vec<(&str, bool, Option<&str>)> = back.iter().map(|d| (d.project.title.as_str(), d.dirty, d.path.as_deref())).collect();
        assert_eq!(titles, [("Brookside Commons", false, Some("/p/a.waves")), ("Changed", true, Some("/p/b.waves")), ("Untitled", false, None)]);
        assert_eq!(active, 1, "the front tab follows its project");
        assert_eq!(restore("not json", files).0.len(), 0);
    }

    #[test]
    fn reads_the_old_recovery_record() {
        let old = r#"{"projects":[{"path":null,"hcp":false,"project":"#.to_string() + &serde_json::to_string(&crate::samples::sample_project().to_json().unwrap()).unwrap() + "}]}";
        let (docs, _) = restore(&old, |_| None);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].dirty, "0.3.0 kept only unsaved work");
    }
}
