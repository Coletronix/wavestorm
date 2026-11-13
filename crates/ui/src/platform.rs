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

/// Named text values that persist between sessions: files in the user's configuration folder on
/// the desktop; in the browser, IndexedDB (see [`store::load`]), which holds far more than
/// `localStorage` (about 5 MB per site), so recovery copies keep their background images.
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
            web::read(key)
        }
    }

    pub fn write(key: &str, text: &str) {
        let _ = try_write(key, text);
    }

    /// Like [`write`], reporting failure. In the browser a write to IndexedDB finishes later:
    /// its failure (for example a full storage quota) comes from [`take_problem`].
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
            web::write(key, Some(text))
        }
    }

    /// Deletes a stored value.
    pub fn remove(key: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = config_file(&format!("{key}.json")) {
            let _ = std::fs::remove_file(f);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = web::write(key, None);
        }
    }

    /// A write that failed since the last call, described for the user (browser only).
    pub fn take_problem() -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            web::take_problem()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// Opens the browser's IndexedDB store and reads every value into memory, moving values
    /// kept in `localStorage` by earlier versions across. Call once before the app starts;
    /// until then (or if IndexedDB is unavailable) values live in `localStorage` as before.
    #[cfg(target_arch = "wasm32")]
    pub async fn load() {
        web::load().await
    }

    /// What a failed write of `key` loses, for the message.
    #[cfg(target_arch = "wasm32")]
    fn what(key: &str) -> &'static str {
        match key {
            "session" => "the copy of your open projects (reopened next time)",
            "recent" => "the Recent Projects list",
            _ => "your settings",
        }
    }

    #[cfg(target_arch = "wasm32")]
    mod web {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

        const DB: &str = "wavestorm";
        const STORE: &str = "store";
        /// The prefix of values in `localStorage`.
        const PREFIX: &str = "wavestorm.";

        thread_local! {
            /// The database and every value in it, once [`load`] has run.
            static OPEN: RefCell<Option<(IdbDatabase, HashMap<String, String>)>> = const { RefCell::new(None) };
            static PROBLEMS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }

        fn local() -> Option<web_sys::Storage> {
            web_sys::window()?.local_storage().ok()?
        }

        fn js(e: JsValue) -> String {
            e.dyn_ref::<web_sys::DomException>()
                .map(|d| format!("{}: {}", d.name(), d.message()))
                .or_else(|| e.as_string())
                .unwrap_or_else(|| format!("{e:?}"))
        }

        pub fn read(key: &str) -> Option<String> {
            let cached = OPEN.with(|o| o.borrow().as_ref().map(|(_, m)| m.get(key).cloned()));
            match cached {
                Some(v) => v,
                None => local()?.get_item(&format!("{PREFIX}{key}")).ok()?,
            }
        }

        pub fn write(key: &str, value: Option<&str>) -> Result<(), String> {
            let db = OPEN.with(|o| {
                o.borrow_mut().as_mut().map(|(db, m)| {
                    match value {
                        Some(v) => m.insert(key.to_string(), v.to_string()),
                        None => m.remove(key),
                    };
                    db.clone()
                })
            });
            let Some(db) = db else {
                // No IndexedDB: localStorage, as earlier versions did.
                let s = local().ok_or("no browser storage")?;
                let k = format!("{PREFIX}{key}");
                return match value {
                    Some(v) => s.set_item(&k, v).map_err(js),
                    None => s.remove_item(&k).map_err(js),
                };
            };
            let (key, value) = (key.to_string(), value.map(str::to_string));
            // Transactions run in the order they are created, so the last write wins.
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = put(&db, &key, value.as_deref()).await {
                    PROBLEMS.with(|p| {
                        p.borrow_mut().push(format!(
                            "This browser would not store {} ({e}). Your work is not lost while this tab \
                             stays open: save (download) your projects to keep them.",
                            super::what(&key)
                        ))
                    });
                }
            });
            Ok(())
        }

        pub fn take_problem() -> Option<String> {
            PROBLEMS.with(|p| p.borrow_mut().pop())
        }

        /// A promise settled by `req`: its result, or its error.
        fn request(req: &IdbRequest) -> js_sys::Promise {
            let req = req.clone();
            js_sys::Promise::new(&mut |resolve, reject| {
                let r = req.clone();
                let ok = Closure::once_into_js(move || {
                    let _ = resolve.call1(&JsValue::NULL, &r.result().unwrap_or(JsValue::UNDEFINED));
                });
                let r = req.clone();
                let fail = Closure::once_into_js(move || {
                    let e = r.error().ok().flatten().map(JsValue::from).unwrap_or(JsValue::UNDEFINED);
                    let _ = reject.call1(&JsValue::NULL, &e);
                });
                req.set_onsuccess(Some(ok.unchecked_ref()));
                req.set_onerror(Some(fail.unchecked_ref()));
            })
        }

        /// A promise settled when `tx` commits (a write is only safe then) or aborts.
        fn committed(tx: &IdbTransaction) -> js_sys::Promise {
            let tx = tx.clone();
            js_sys::Promise::new(&mut |resolve, reject| {
                let ok = Closure::once_into_js(move || {
                    let _ = resolve.call0(&JsValue::NULL);
                });
                let t = tx.clone();
                let fail = Closure::once_into_js(move || {
                    let e = t.error().map(JsValue::from).unwrap_or_else(|| JsValue::from_str("aborted"));
                    let _ = reject.call1(&JsValue::NULL, &e);
                });
                tx.set_oncomplete(Some(ok.unchecked_ref()));
                tx.set_onabort(Some(fail.unchecked_ref()));
            })
        }

        async fn put(db: &IdbDatabase, key: &str, value: Option<&str>) -> Result<(), String> {
            let tx = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite).map_err(js)?;
            let store = tx.object_store(STORE).map_err(js)?;
            let k = JsValue::from_str(key);
            match value {
                Some(v) => store.put_with_key(&JsValue::from_str(v), &k).map(drop),
                None => store.delete(&k).map(drop),
            }
            .map_err(js)?;
            JsFuture::from(committed(&tx)).await.map(drop).map_err(js)
        }

        async fn open() -> Result<(IdbDatabase, HashMap<String, String>), String> {
            let factory = web_sys::window().ok_or("no window")?.indexed_db().map_err(js)?.ok_or("no IndexedDB")?;
            let req = factory.open_with_u32(DB, 1).map_err(js)?;
            let r = req.clone();
            let upgrade = Closure::once_into_js(move || {
                if let Ok(db) = r.result() {
                    let _ = db.unchecked_into::<IdbDatabase>().create_object_store(STORE);
                }
            });
            req.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
            let db: IdbDatabase = JsFuture::from(request(&req)).await.map_err(js)?.unchecked_into();
            let tx = db.transaction_with_str(STORE).map_err(js)?;
            let store = tx.object_store(STORE).map_err(js)?;
            let keys = JsFuture::from(request(&store.get_all_keys().map_err(js)?)).await.map_err(js)?;
            let values = JsFuture::from(request(&store.get_all().map_err(js)?)).await.map_err(js)?;
            let (keys, values): (js_sys::Array, js_sys::Array) = (keys.unchecked_into(), values.unchecked_into());
            let mut map = HashMap::new();
            for (k, v) in keys.iter().zip(values.iter()) {
                if let (Some(k), Some(v)) = (k.as_string(), v.as_string()) {
                    map.insert(k, v);
                }
            }
            Ok((db, map))
        }

        pub async fn load() {
            let (db, mut map) = match open().await {
                Ok(x) => x,
                Err(e) => {
                    log::warn!("IndexedDB unavailable, using localStorage: {e}");
                    return;
                }
            };
            // Ask the browser not to clear this site's storage when space runs low.
            if let Some(w) = web_sys::window() {
                let _ = w.navigator().storage().persist();
            }
            // Move what earlier versions kept in localStorage; it is removed once stored here.
            if let Some(s) = local() {
                let n = s.length().unwrap_or(0);
                let keys: Vec<String> = (0..n).filter_map(|i| s.key(i).ok().flatten()).collect();
                for k in keys.iter().filter_map(|k| k.strip_prefix(PREFIX)) {
                    let Some(v) = s.get_item(&format!("{PREFIX}{k}")).ok().flatten() else { continue };
                    if !map.contains_key(k) {
                        if let Err(e) = put(&db, k, Some(&v)).await {
                            log::warn!("Could not move {k} to IndexedDB: {e}");
                            continue;
                        }
                        map.insert(k.to_string(), v);
                    }
                    let _ = s.remove_item(&format!("{PREFIX}{k}"));
                }
            }
            OPEN.with(|o| *o.borrow_mut() = Some((db, map)));
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
