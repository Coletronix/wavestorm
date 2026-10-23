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
            Purpose::OpenProject | Purpose::MergeProject => ("Stormwaves project", &["waves", "json"]),
            Purpose::ImportSubareas => ("Sub-area table", &["csv", "txt"]),
            Purpose::Underlay => ("Image or PDF", &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "pdf"]),
            Purpose::LinkCsv => ("Hydrograph CSV", &["csv", "txt"]),
            Purpose::ImportTr20 => ("TR-20 input file", &["inp", "dat", "tr2", "txt"]),
            Purpose::NoaaCsv => ("NOAA Atlas 14 table", &["csv", "txt"]),
            Purpose::RatingCsv => ("Rating table", &["csv", "txt"]),
        }
    }

    /// Asks the user for a file.
    pub fn pick(&self, purpose: Purpose) {
        if !self.dialogs {
            return;
        }
        let (name, ext) = Self::filters(purpose);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new().add_filter(name, ext).pick_file() {
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
                if let Some(file) = dialog.pick_file().await {
                    let bytes = file.read().await;
                    inbox.lock().unwrap_or_else(PoisonError::into_inner).push(Incoming { purpose, name: file.file_name(), path: None, bytes });
                }
            });
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
    let a = document.create_element("a").map_err(err)?.dyn_into::<web_sys::HtmlAnchorElement>().map_err(|_| "anchor".to_string())?;
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
        Some(base.join("stormwaves").join(name))
    }

    pub fn read(key: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read_to_string(config_file(&format!("{key}.json"))?).ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()?.local_storage().ok()??.get_item(&format!("stormwaves.{key}")).ok()?
        }
    }

    pub fn write(key: &str, text: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = config_file(&format!("{key}.json")) {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, text);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(Ok(Some(s))) = web_sys::window().map(|w| w.local_storage()) {
            let _ = s.set_item(&format!("stormwaves.{key}"), text);
        }
    }
}

/// Recently opened project paths (desktop only), most recent first.
#[cfg(not(target_arch = "wasm32"))]
pub mod recent {
    fn file() -> Option<std::path::PathBuf> {
        super::store::config_file("recent.json")
    }

    pub fn list() -> Vec<String> {
        file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default()
    }

    pub fn push(path: &str) {
        let mut v = list();
        v.retain(|p| p != path);
        v.insert(0, path.to_string());
        v.truncate(8);
        if let Some(f) = file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(t) = serde_json::to_string_pretty(&v) {
                let _ = std::fs::write(f, t);
            }
        }
    }
}
