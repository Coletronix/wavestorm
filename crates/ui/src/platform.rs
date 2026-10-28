//! File access for desktop and browser. Results arrive through an inbox polled each frame, so
//! the asynchronous browser picker and the native dialogs share one code path.

use std::sync::{Arc, Mutex, PoisonError};

/// What a picked file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    OpenProject,
    MergeProject,
    Underlay,
    LinkCsv,
    ImportSubareas,
    ImportTr20,
    NoaaCsv,
    /// A pump or special-outlet rating table (two columns).
    RatingCsv,
    /// Several rainfall library files at once (.hcr, .huh, .hci, *-Rain.txt).
    RainLib,
}

/// A file delivered to the app.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub purpose: Purpose,
    pub name: String,
    /// Native path, when known (desktop).
    pub path: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct Platform {
    inbox: Arc<Mutex<Vec<Incoming>>>,
    /// Native file dialogs may be shown. Off for headless runs (tests, snapshots), where a
    /// dialog would block forever waiting for a user.
    dialogs: bool,
}

impl Default for Platform {
    fn default() -> Self {
        Platform { inbox: Arc::default(), dialogs: true }
    }
}

impl Platform {
    /// A platform that never shows file dialogs: picks are ignored and dialog saves are skipped.
    pub fn headless() -> Self {
        Platform { dialogs: false, ..Platform::default() }
    }

    pub fn take(&self) -> Vec<Incoming> {
        std::mem::take(&mut *self.inbox.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn push(&self, item: Incoming) {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner).push(item);
    }

    fn filters(purpose: Purpose) -> (&'static str, &'static [&'static str]) {
        match purpose {
            Purpose::OpenProject => ("Project (.waves or .hcp)", &["waves", "json", "hcp"]),
            Purpose::MergeProject => ("WAVEStorm project", &["waves", "json"]),
            Purpose::ImportSubareas => ("Sub-area table", &["csv", "txt"]),
            Purpose::Underlay => ("Image or PDF", &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "pdf"]),
            Purpose::LinkCsv => ("Hydrograph CSV", &["csv", "txt"]),
            Purpose::ImportTr20 => ("TR-20 input file", &["inp", "dat", "tr2", "txt"]),
            Purpose::RatingCsv => ("Rating table", &["csv", "txt"]),
            Purpose::NoaaCsv => ("Rainfall table, IDF file or NOAA Atlas 14 CSV", &["txt", "hci", "idf", "csv"]),
            Purpose::RainLib => ("Rainfall files", &["hcr", "huh", "hci", "idf", "txt"]),
        }
    }

    /// Asks the user for a file (several for [`Purpose::RainLib`]).
    pub fn pick(&self, purpose: Purpose) {
        if !self.dialogs {
            return;
        }
        let (name, ext) = Self::filters(purpose);
        let many = purpose == Purpose::RainLib;
        #[cfg(not(target_arch = "wasm32"))]
        {
            if many {
                for path in rfd::FileDialog::new().add_filter(name, ext).pick_files().unwrap_or_default() {
                    if let Ok(bytes) = std::fs::read(&path) {
                        self.push(Incoming {
                            purpose,
                            name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            path: Some(path.to_string_lossy().into_owned()),
                            bytes,
                        });
                    }
                }
            } else if let Some(path) = rfd::FileDialog::new().add_filter(name, ext).pick_file() {
                match std::fs::read(&path) {
                    Ok(bytes) => self.push(Incoming {
                        purpose,
                        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        path: Some(path.to_string_lossy().into_owned()),
                        bytes,
                    }),
                    Err(e) => log::error!("cannot read {}: {e}", path.display()),
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let inbox = self.inbox.clone();
            let dialog = rfd::AsyncFileDialog::new().add_filter(name, ext);
            wasm_bindgen_futures::spawn_local(async move {
                if many {
                    for file in dialog.pick_files().await.unwrap_or_default() {
                        let bytes = file.read().await;
                        inbox.lock().unwrap_or_else(PoisonError::into_inner).push(Incoming {
                            purpose,
                            name: file.file_name(),
                            path: None,
                            bytes,
                        });
                    }
                } else if let Some(file) = dialog.pick_file().await {
                    let bytes = file.read().await;
                    inbox.lock().unwrap_or_else(PoisonError::into_inner).push(Incoming {
                        purpose,
                        name: file.file_name(),
                        path: None,
                        bytes,
                    });
                }
            });
        }
    }

    /// Asks the user for a folder (desktop only; the browser has no folders).
    pub fn pick_folder(&self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !self.dialogs {
                return None;
            }
            rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().into_owned())
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Opens a file directly by path (desktop command line).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_path(&self, path: &str) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot open {path}: {e}"))?;
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.push(Incoming { purpose: Purpose::OpenProject, name, path: Some(path.into()), bytes });
        Ok(())
    }

    /// Saves bytes: to `path` if given, else asks (desktop) or downloads (browser).
    /// Returns the path written, if any.
    pub fn save(&self, suggested: &str, path: Option<&str>, bytes: &[u8], ext: &str) -> Result<Option<String>, String> {
        if !self.dialogs && path.is_none() {
            return Ok(None);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = match path {
                Some(p) => std::path::PathBuf::from(p),
                None => match rfd::FileDialog::new().set_file_name(suggested).add_filter(ext, &[ext]).save_file() {
                    Some(p) => p,
                    None => return Ok(None),
                },
            };
            // Write to a temporary file first so a failed save never truncates the project.
            let tmp = target.with_extension(format!("{ext}.tmp"));
            std::fs::write(&tmp, bytes).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
            std::fs::rename(&tmp, &target).map_err(|e| format!("Cannot save {}: {e}", target.display()))?;
            Ok(Some(target.to_string_lossy().into_owned()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, ext);
            download(suggested, bytes)?;
            Ok(None)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    let err = |e: wasm_bindgen::JsValue| format!("download failed: {e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let blob = web_sys::Blob::new_with_buffer_source_sequence(&parts).map_err(err)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(err)?;
    let a = document
        .create_element("a")
        .map_err(err)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "anchor".to_string())?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    web_sys::Url::revoke_object_url(&url).map_err(err)?;
    Ok(())
}

/// Small named text values that persist between sessions: files in the user's configuration
/// folder on the desktop, `localStorage` in the browser. Failures are ignored (best effort).
pub mod store {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn config_file(name: &str) -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
        Some(base.join("wavestorm").join(name))
    }

    pub fn read(key: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read_to_string(config_file(&format!("{key}.json"))?).ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()?.local_storage().ok()??.get_item(&format!("wavestorm.{key}")).ok()?
        }
    }

    pub fn write(key: &str, text: &str) {
        let _ = try_write(key, text);
    }

    /// Like [`write`], reporting failure (for example a full browser storage quota).
    pub fn try_write(key: &str, text: &str) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let f = config_file(&format!("{key}.json")).ok_or("no configuration folder")?;
            if let Some(dir) = f.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(f, text).map_err(|e| e.to_string())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let s = web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or("no local storage")?;
            s.set_item(&format!("wavestorm.{key}"), text).map_err(|e| format!("{e:?}"))
        }
    }

    /// Deletes a stored value.
    pub fn remove(key: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = config_file(&format!("{key}.json")) {
            let _ = std::fs::remove_file(f);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(Ok(Some(s))) = web_sys::window().map(|w| w.local_storage()) {
            let _ = s.remove_item(&format!("wavestorm.{key}"));
        }
    }
}

/// Recently opened or saved projects, most recent first: file paths on the desktop; in the
/// browser (no file paths) copies of the projects themselves, kept in browser storage.
pub mod recent {
    use serde::{Deserialize, Serialize};
    use wavestorm_core::project::Project;

    const KEY: &str = "recent";
    const MAX_DESKTOP: usize = 8;
    const MAX_WEB: usize = 6;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Entry {
        pub name: String,
        #[serde(default)]
        pub path: Option<String>,
        /// The project (`.waves` JSON), for entries without a file (browser).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub data: Option<String>,
    }

    impl Entry {
        /// What the menu shows: the path on the desktop, the name in the browser.
        pub fn label(&self) -> &str {
            self.path.as_deref().unwrap_or(&self.name)
        }

        /// The project file's bytes, or None when the file is gone.
        pub fn bytes(&self) -> Option<Vec<u8>> {
            if let Some(d) = &self.data {
                return Some(d.clone().into_bytes());
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(p) = &self.path {
                return std::fs::read(p).ok();
            }
            None
        }
    }

    fn file_name(p: &str) -> String {
        std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
    }

    pub fn list() -> Vec<Entry> {
        let Some(text) = super::store::read(KEY) else { return Vec::new() };
        if let Ok(v) = serde_json::from_str::<Vec<Entry>>(&text) {
            return v;
        }
        // Earlier desktop lists were plain paths.
        serde_json::from_str::<Vec<String>>(&text)
            .map(|v| v.into_iter().map(|p| Entry { name: file_name(&p), path: Some(p), data: None }).collect())
            .unwrap_or_default()
    }

    /// Records an opened or saved project: by its path when it has one (desktop); in the
    /// browser, as a copy of the project under its file name.
    pub fn push(name: &str, path: Option<&str>, project: &Project) {
        let entry = match path {
            Some(p) => Entry { name: file_name(p), path: Some(p.to_string()), data: None },
            None if cfg!(target_arch = "wasm32") => match project.to_json() {
                Ok(j) => Entry { name: name.to_string(), path: None, data: Some(j) },
                Err(_) => return,
            },
            None => return,
        };
        let mut v = list();
        v.retain(
            |e| if entry.path.is_some() { e.path != entry.path } else { e.path.is_some() || e.name != entry.name },
        );
        v.insert(0, entry);
        v.truncate(if cfg!(target_arch = "wasm32") { MAX_WEB } else { MAX_DESKTOP });
        save(v);
    }

    /// Drops an entry whose file is gone.
    pub fn forget(e: &Entry) {
        let mut v = list();
        v.retain(|x| x != e);
        save(v);
    }

    fn save(mut v: Vec<Entry>) {
        // Browser storage is limited: drop the oldest copies until the list fits.
        loop {
            let Ok(t) = serde_json::to_string_pretty(&v) else { return };
            if super::store::try_write(KEY, &t).is_ok() || v.is_empty() {
                return;
            }
            v.pop();
        }
    }
}
