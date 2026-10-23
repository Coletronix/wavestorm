//! The rainfall library: storms, unit hydrographs, IDF files and county lookup tables that come
//! from a folder chosen in Settings > General (desktop) or from files imported by the user
//! (desktop and browser). Selecting a library storm or unit hydrograph for a project copies it
//! into the project (see [`adopt`]) so the project file stays self-contained.

use crate::platform::store;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use stormwaves_core::project::{NodeData, Project};
use stormwaves_core::rainfall::{Storm, builtin_storms};
use stormwaves_core::rainlib::RainLibrary;
use stormwaves_core::uh::{UnitHydrograph, builtin_uhs};

const IMPORT_KEY: &str = "rainfall_imports";
/// Imported files are kept for later sessions only while they stay below this many bytes.
const IMPORT_LIMIT: usize = 2_500_000;

#[derive(Default)]
struct State {
    folder: String,
    scanned: RainLibrary,
    /// Imported files (name, text), in the order imported.
    imports: Vec<(String, String)>,
    merged: Arc<RainLibrary>,
    persist: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sets up the library at startup: scans the preference folder and restores earlier imports.
pub fn init(folder: &str, persist: bool) {
    let mut st = lock();
    st.persist = persist;
    st.folder = folder.trim().to_string();
    st.scanned = scan(&st.folder);
    st.imports = if persist { store::read(IMPORT_KEY).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() } else { Vec::new() };
    rebuild(&mut st);
}

/// Changes the library folder and scans it again.
pub fn set_folder(folder: &str) {
    let mut st = lock();
    st.folder = folder.trim().to_string();
    st.scanned = scan(&st.folder);
    rebuild(&mut st);
}

/// Like [`set_folder`], but does nothing when the folder is unchanged.
pub fn set_folder_if_changed(folder: &str) {
    if lock().folder != folder.trim() {
        set_folder(folder);
    }
}

pub fn folder() -> String {
    lock().folder.clone()
}

/// Scans the library folder again (new files appear without restarting).
pub fn rescan() {
    let mut st = lock();
    st.scanned = scan(&st.folder);
    rebuild(&mut st);
}

/// Adds one imported file. Returns a status message.
pub fn import(name: &str, bytes: &[u8]) -> String {
    let mut probe = RainLibrary::default();
    if !probe.add_file(name, bytes) {
        return match probe.problems.first() {
            Some(p) => format!("Cannot read {p}"),
            None => format!("{name} is not a rainfall file (.hcr, .huh, .idf or *-Rain.txt)"),
        };
    }
    let mut st = lock();
    st.imports.retain(|(n, _)| n != name);
    st.imports.push((name.to_string(), String::from_utf8_lossy(bytes).into_owned()));
    rebuild(&mut st);
    if st.persist {
        let total: usize = st.imports.iter().map(|(_, t)| t.len()).sum();
        if total <= IMPORT_LIMIT {
            if let Ok(json) = serde_json::to_string(&st.imports) {
                store::write(IMPORT_KEY, &json);
            }
        }
    }
    format!("Imported {name}")
}

/// Forgets the imported files.
pub fn clear_imports() {
    let mut st = lock();
    st.imports.clear();
    rebuild(&mut st);
    if st.persist {
        store::write(IMPORT_KEY, "[]");
    }
}

pub fn import_count() -> usize {
    lock().imports.len()
}

fn rebuild(st: &mut State) {
    let mut merged = st.scanned.clone();
    for (name, text) in &st.imports {
        let mut one = RainLibrary::default();
        if one.add_file(name, text.as_bytes()) {
            for s in one.storms {
                if !merged.storms.iter().any(|x| x.name == s.name) {
                    merged.storms.push(s);
                }
            }
            for u in one.uhs {
                if !merged.uhs.iter().any(|x| x.name == u.name) {
                    merged.uhs.push(u);
                }
            }
            for i in one.idfs {
                merged.idfs.retain(|x| x.name != i.name);
                merged.idfs.push(i);
            }
            for t in one.tables {
                merged.tables.retain(|x| x.name != t.name);
                merged.tables.push(t);
            }
        }
    }
    merged.storms.sort_by_key(|s| s.name.to_ascii_lowercase());
    merged.uhs.sort_by_key(|u| u.name.to_ascii_lowercase());
    merged.idfs.sort_by_key(|i| i.name.to_ascii_lowercase());
    merged.tables.sort_by_key(|t| t.name.to_ascii_lowercase());
    st.merged = Arc::new(merged);
}

/// The current library (cheap to clone).
pub fn current() -> Arc<RainLibrary> {
    lock().merged.clone()
}

#[cfg(not(target_arch = "wasm32"))]
fn scan(folder: &str) -> RainLibrary {
    let mut lib = RainLibrary::default();
    if folder.is_empty() {
        return lib;
    }
    let mut files = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from(folder)];
    // The folder itself and its immediate subfolders.
    let mut depth = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            if depth == 0 {
                lib.problems.push(format!("Cannot read the folder {folder}"));
            }
            depth += 1;
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth == 0 {
                    dirs.push(path);
                }
            } else {
                files.push(path);
            }
        }
        depth += 1;
    }
    files.sort();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".hcr") || lower.ends_with(".huh") || lower.ends_with(".idf") || lower.ends_with("-rain.txt")) {
            continue;
        }
        if std::fs::metadata(&path).map_or(true, |m| m.len() > 40_000_000) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            lib.add_file(&name, &bytes);
        }
    }
    lib
}

#[cfg(target_arch = "wasm32")]
fn scan(_folder: &str) -> RainLibrary {
    RainLibrary::default()
}

/// Storm names to offer: built-in, then the project's own, then the library's.
pub fn storm_names(p: &Project) -> Vec<String> {
    let mut v: Vec<String> = builtin_storms().into_iter().map(|s| s.name).collect();
    for n in p.storms.iter().map(|s| s.name.clone()).chain(current().storms.iter().map(|s| s.name.clone())) {
        if !v.contains(&n) {
            v.push(n);
        }
    }
    v
}

/// Unit hydrograph names to offer.
pub fn uh_names(p: &Project) -> Vec<String> {
    let mut v: Vec<String> = builtin_uhs().into_iter().map(|u| u.name).collect();
    for n in p.uhs.iter().map(|u| u.name.clone()).chain(current().uhs.iter().map(|u| u.name.clone())) {
        if !v.contains(&n) {
            v.push(n);
        }
    }
    v
}

/// A storm from the project, the built-in list or the library.
pub fn find_storm(p: &Project, name: &str) -> Option<Storm> {
    p.storm(name).or_else(|| current().storms.iter().find(|s| s.name == name).cloned())
}

/// A unit hydrograph from the project, the built-in list or the library.
pub fn find_uh(p: &Project, name: &str) -> Option<UnitHydrograph> {
    p.unit_hydrograph(name).or_else(|| current().uhs.iter().find(|u| u.name == name).cloned())
}

/// Copies every library storm and unit hydrograph the project refers to (events, default and
/// per-subcatchment unit hydrographs) into the project. Returns how many were copied.
pub fn adopt(p: &mut Project) -> usize {
    let lib = current();
    if lib.is_empty() {
        return 0;
    }
    let mut copied = 0;
    let storms: Vec<String> = p.settings.events.iter().map(|e| e.storm.clone()).collect();
    for name in storms {
        if p.storm(&name).is_none() {
            if let Some(s) = lib.storms.iter().find(|s| s.name == name) {
                p.storms.push(s.clone());
                copied += 1;
            }
        }
    }
    let mut uhs: Vec<String> = vec![p.settings.uh.clone()];
    for n in &p.nodes {
        if let NodeData::Subcat(sc) = &n.data {
            uhs.extend(sc.uh.clone());
        }
    }
    for name in uhs {
        if p.unit_hydrograph(&name).is_none() {
            if let Some(u) = lib.uhs.iter().find(|u| u.name == name) {
                p.uhs.push(u.clone());
                copied += 1;
            }
        }
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use stormwaves_core::rainfall::RainEvent;

    #[test]
    fn imported_storms_are_offered_and_adopted() {
        init("", false);
        let msg = import("Mine 3-hr.hcr", b"name=Mine 3-hr\nduration=3\ndepth= 0 .1 .5 1\n");
        assert!(msg.starts_with("Imported"), "{msg}");
        assert!(import("x.hcr", b"name=x\n").starts_with("Cannot read"));
        assert!(import("notes.md", b"hello").contains("not a rainfall file"));
        let mut p = Project::default();
        assert!(storm_names(&p).contains(&"Mine 3-hr".to_string()));
        assert!(find_storm(&p, "Mine 3-hr").is_some());
        p.settings.events = vec![RainEvent::new("2-Year", "Mine 3-hr", 2.0)];
        assert_eq!(adopt(&mut p), 1);
        assert_eq!(p.storms.len(), 1);
        assert_eq!(adopt(&mut p), 0, "already in the project");
        assert!(p.storm("Mine 3-hr").is_some());
        clear_imports();
        assert!(!storm_names(&Project::default()).contains(&"Mine 3-hr".to_string()));
    }
}
