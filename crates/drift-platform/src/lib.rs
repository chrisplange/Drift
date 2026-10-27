//! Platform services for Drift: opening and saving files on native and web.
//!
//! Dialogs are asynchronous everywhere (the browser has no blocking dialogs), so results arrive
//! later as [`FileEvent`]s. Call [`Files::take_events`] each frame; the `wake` callback passed to
//! [`Files::new`] is invoked when an event arrives so the UI can repaint.

pub mod crash;
pub mod fonts;
pub mod memory;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Drift's version: the crate version, or the one a build was given in `DRIFT_VERSION`.
/// `scripts/package-windows.ps1` labels local test builds that way, e.g.
/// `0.1.0-beta.1.5+test.9490d05`; semver puts it just after the release it was built from,
/// so the updater offers the next release but not that one.
pub const VERSION: &str = match option_env!("DRIFT_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// Where a document was opened from or saved to. Only native builds have paths; the browser
/// has no persistent file handles yet, so every web save is a download.
pub type FilePath = PathBuf;

#[derive(Debug)]
pub enum FileEvent {
    Opened {
        name: String,
        path: Option<FilePath>,
        contents: Vec<u8>,
    },
    Saved {
        name: String,
        path: Option<FilePath>,
    },
    /// A copy in another format (PDF, SVG, DXF) was written; the document's own file is
    /// unchanged.
    Exported {
        name: String,
    },
    /// A file was handed to the system (or browser) viewer, e.g. to print it.
    Shown {
        name: String,
    },
    Failed(String),
    /// The user closed the file picker without choosing a file.
    Cancelled,
}

/// A file type filter for dialogs, e.g. `("Drift sketch", &["drift"])`.
#[derive(Clone, Copy, Debug)]
pub struct FileType {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct Files {
    events: Arc<Mutex<Vec<FileEvent>>>,
    wake: Waker,
    /// How many file pickers are open.
    pickers: Arc<AtomicUsize>,
}

/// An open file picker: counted by [`Files::picker_open`] until it is dropped.
pub struct Picker(Arc<AtomicUsize>);

impl Drop for Picker {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Files {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            events: Arc::default(),
            wake: Arc::new(wake),
            pickers: Arc::default(),
        }
    }

    /// Whether a file picker is open. Its answer replaces or saves the document, so the app
    /// treats it as a modal dialog: the drawing can't change underneath it.
    pub fn picker_open(&self) -> bool {
        self.pickers.load(Ordering::Relaxed) > 0
    }

    /// Count a file picker as open until the returned [`Picker`] is dropped. The dialogs here
    /// do this themselves; tests use it to stand in for one.
    pub fn picker(&self) -> Picker {
        self.pickers.fetch_add(1, Ordering::Relaxed);
        Picker(self.pickers.clone())
    }

    /// Hand the app `event` as if a dialog had sent it (tests).
    #[doc(hidden)]
    pub fn deliver(&self, event: FileEvent) {
        self.post(event);
    }

    /// Events that arrived since the last call.
    pub fn take_events(&self) -> Vec<FileEvent> {
        std::mem::take(&mut *self.events.lock().expect("file event lock poisoned"))
    }

    fn post(&self, event: FileEvent) {
        self.events
            .lock()
            .expect("file event lock poisoned")
            .push(event);
        (self.wake)();
    }

    /// Ask the user for a file to open.
    #[cfg(not(all(target_arch = "wasm32", feature = "demo")))]
    pub fn open(&self, file_type: FileType) {
        let this = self.clone();
        let picker = self.picker();
        spawn(async move {
            let _picker = picker;
            let dialog =
                rfd::AsyncFileDialog::new().add_filter(file_type.name, file_type.extensions);
            let Some(handle) = dialog.pick_file().await else {
                this.post(FileEvent::Cancelled);
                return;
            };
            let contents = handle.read().await;
            #[cfg(not(target_arch = "wasm32"))]
            let path = Some(handle.path().to_owned());
            #[cfg(target_arch = "wasm32")]
            let path = None;
            this.post(FileEvent::Opened {
                name: handle.file_name(),
                path,
                contents,
            });
        });
    }

    /// Ask the user for a file to open, with the browser's own file input. The browser demo
    /// leaves rfd out: its web dialog also holds the code that downloads a saved file.
    #[cfg(all(target_arch = "wasm32", feature = "demo"))]
    pub fn open(&self, file_type: FileType) {
        let this = self.clone();
        let picker = self.picker();
        spawn(async move {
            let _picker = picker;
            match web_pick::pick(file_type.extensions).await {
                Some((name, contents)) => this.post(FileEvent::Opened {
                    name,
                    path: None,
                    contents,
                }),
                None => this.post(FileEvent::Cancelled),
            }
        });
    }

    /// Open a file by its path (a recent file, or one passed on the command line). Native
    /// only.
    pub fn open_path(&self, path: &FilePath) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.post(match std::fs::read(path) {
                Ok(contents) => FileEvent::Opened {
                    name,
                    path: Some(path.clone()),
                    contents,
                },
                Err(e) => FileEvent::Failed(format!("could not open {}: {e}", path.display())),
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            self.post(FileEvent::Failed(
                "opening a path is not supported on the web".into(),
            ));
        }
    }

    /// Ask the user where to save, then write `contents` there. On the web this downloads the
    /// file under `suggested_name`.
    #[cfg(not(feature = "demo"))]
    pub fn save_as(&self, suggested_name: &str, file_type: FileType, contents: Vec<u8>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let this = self.clone();
            let name = suggested_name.to_owned();
            let picker = self.picker();
            spawn(async move {
                let _picker = picker;
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter(file_type.name, file_type.extensions)
                    .set_file_name(&name);
                if let Some(handle) = dialog.save_file().await {
                    let path = handle.path().to_owned();
                    this.post(write_native(path, &contents, Backup::Keep));
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = file_type;
            self.post(web::download(suggested_name, &contents));
        }
    }

    /// Ask the user where to write an export (a PDF, SVG or DXF copy), then write it. Unlike
    /// [`Files::save_as`], this reports [`FileEvent::Exported`]: the export does not become
    /// the document's file.
    #[cfg(not(feature = "demo"))]
    pub fn export(&self, suggested_name: &str, file_type: FileType, contents: Vec<u8>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let this = self.clone();
            let name = suggested_name.to_owned();
            let picker = self.picker();
            spawn(async move {
                let _picker = picker;
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter(file_type.name, file_type.extensions)
                    .set_file_name(&name);
                if let Some(handle) = dialog.save_file().await {
                    let path = handle.path().to_owned();
                    this.post(match write_native(path, &contents, Backup::None) {
                        FileEvent::Saved { name, .. } => FileEvent::Exported { name },
                        other => other,
                    });
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = file_type;
            self.post(match web::download(suggested_name, &contents) {
                FileEvent::Saved { name, .. } => FileEvent::Exported { name },
                other => other,
            });
        }
    }

    /// Open `contents` in the system's viewer for its type (a new browser tab on the web), e.g. a
    /// PDF to print. Native builds write it to a temporary folder first.
    #[cfg(not(feature = "demo"))]
    pub fn open_externally(&self, name: &str, contents: &[u8]) {
        #[cfg(not(target_arch = "wasm32"))]
        self.post(open_native(name, contents));
        #[cfg(target_arch = "wasm32")]
        self.post(web::open(name, contents));
    }

    /// Overwrite a file that was previously opened or saved (native only).
    #[cfg(not(feature = "demo"))]
    pub fn save_to(&self, path: &FilePath, contents: &[u8]) {
        #[cfg(not(target_arch = "wasm32"))]
        self.post(write_native(path.clone(), contents, Backup::Keep));
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, contents);
            self.post(FileEvent::Failed(
                "saving to a path is not supported on the web".into(),
            ));
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
fn write_native(path: PathBuf, contents: &[u8], backup: Backup) -> FileEvent {
    let name = file_name(&path);
    match write_atomic(&path, contents, backup) {
        Ok(()) => FileEvent::Saved {
            name,
            path: Some(path),
        },
        Err(e) => FileEvent::Failed(format!(
            "could not save {}: {e}. The file on disk is unchanged.",
            path.display()
        )),
    }
}

/// Whether [`write_atomic`] keeps the file it replaces as `<name>.bak`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backup {
    /// Keep the previous version beside it (drawings).
    Keep,
    /// Don't (exports and other files that can be made again).
    None,
}

/// Write `contents` to `path` so that a crash, power cut, full disk or locked file can never
/// leave a cut-off file there: the bytes go to a temporary file in the same folder, which is
/// flushed to disk and then renamed over `path`. If anything fails, `path` is untouched. With
/// [`Backup::Keep`] the version it replaces is copied to `<name>.bak` first.
#[cfg(not(feature = "demo"))]
pub fn write_atomic(
    path: &std::path::Path,
    contents: &[u8],
    backup: Backup,
) -> std::io::Result<()> {
    replace_with(path, backup, |file| {
        use std::io::Write as _;
        file.write_all(contents)
    })
}

/// [`write_atomic`] with the writing step given, so tests can make it fail partway.
#[cfg(not(feature = "demo"))]
fn replace_with(
    path: &std::path::Path,
    backup: Backup,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temp = sibling(path, &format!(".{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&temp)?;
        write(&mut file)?;
        file.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    if backup == Backup::Keep && path.is_file() {
        // A copy, not a rename: `path` must exist at every moment. A backup that can't be made
        // doesn't stop the save.
        if let Err(e) = std::fs::copy(path, sibling(path, ".bak")) {
            log::warn!("could not keep a backup of {}: {e}", file_name(path));
        }
    }
    // Something holding the old file open (a virus scanner, a file indexer) can block the
    // rename on Windows for a moment.
    let mut tries = 0;
    loop {
        match std::fs::rename(&temp, path) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(e);
            }
        }
    }
}

/// `path` with `suffix` added to its file name: `plan.drift` + `.bak` = `plan.drift.bak`.
#[cfg(not(feature = "demo"))]
fn sibling(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// A path's file name, for messages and logs (never the folders above it).
pub fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
fn open_native(name: &str, contents: &[u8]) -> FileEvent {
    let dir = std::env::temp_dir().join("drift");
    let path = dir.join(name);
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, contents)) {
        return FileEvent::Failed(format!("could not write {}: {e}", path.display()));
    }
    #[cfg(target_os = "windows")]
    let launched = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(&path)
        .spawn();
    #[cfg(target_os = "macos")]
    let launched = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let launched = std::process::Command::new("xdg-open").arg(&path).spawn();
    match launched {
        Ok(_) => FileEvent::Shown {
            name: name.to_owned(),
        },
        Err(e) => FileEvent::Failed(format!("could not open {}: {e}", path.display())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(future));
}

#[cfg(target_arch = "wasm32")]
fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
}

#[cfg(all(target_arch = "wasm32", feature = "demo"))]
mod web_pick {
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_futures::JsFuture;

    /// A file the user picks (its name and contents), or `None` if they cancel.
    pub async fn pick(extensions: &[&str]) -> Option<(String, Vec<u8>)> {
        let document = web_sys::window()?.document()?;
        let input: web_sys::HtmlInputElement =
            document.create_element("input").ok()?.dyn_into().ok()?;
        input.set_type("file");
        let accept: Vec<String> = extensions.iter().map(|e| format!(".{e}")).collect();
        input.set_accept(&accept.join(","));
        // Settled by a pick or a cancel, whichever comes.
        let chosen = js_sys::Promise::new(&mut |resolve, _| {
            let done = Closure::once_into_js(move || {
                let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
            });
            for event in ["change", "cancel"] {
                let _ = input.add_event_listener_with_callback(event, done.unchecked_ref());
            }
        });
        input.click();
        JsFuture::from(chosen).await.ok()?;
        let file = input.files()?.get(0)?;
        let buffer = JsFuture::from(file.array_buffer()).await.ok()?;
        Some((file.name(), js_sys::Uint8Array::new(&buffer).to_vec()))
    }
}

// Downloads and blob URLs: the ways a file leaves the browser build, so not in the demo.
#[cfg(all(target_arch = "wasm32", not(feature = "demo")))]
mod web {
    use wasm_bindgen::JsCast as _;

    use super::FileEvent;

    /// Trigger a browser download of `contents` named `name`.
    pub fn download(name: &str, contents: &[u8]) -> FileEvent {
        match try_download(name, contents) {
            Ok(()) => FileEvent::Saved {
                name: name.to_owned(),
                path: None,
            },
            Err(e) => FileEvent::Failed(format!("download failed: {e:?}")),
        }
    }

    /// The MIME type for a file name's extension.
    fn mime(name: &str) -> &'static str {
        match name
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("pdf") => "application/pdf",
            Some("svg") => "image/svg+xml",
            Some("dxf") => "application/dxf",
            Some("dwg") => "image/vnd.dwg",
            _ => "application/json",
        }
    }

    fn blob_url(name: &str, contents: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
        let bytes = js_sys::Uint8Array::from(contents);
        let parts = js_sys::Array::of1(&bytes);
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime(name));
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
        web_sys::Url::create_object_url_with_blob(&blob)
    }

    /// Open `contents` in a new tab (the browser's viewer, which can print it).
    pub fn open(name: &str, contents: &[u8]) -> FileEvent {
        let result = (|| -> Result<(), wasm_bindgen::JsValue> {
            let window = web_sys::window().ok_or("no window")?;
            let url = blob_url(name, contents)?;
            window.open_with_url_and_target(&url, "_blank")?;
            Ok(())
        })();
        match result {
            Ok(()) => FileEvent::Shown {
                name: name.to_owned(),
            },
            Err(e) => FileEvent::Failed(format!("could not open {name}: {e:?}")),
        }
    }

    fn try_download(name: &str, contents: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let url = blob_url(name, contents)?;
        let anchor: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        web_sys::Url::revoke_object_url(&url)?;
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "demo")))]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("drift-atomic-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_failed_save_leaves_the_original_untouched() {
        let dir = folder("fail");
        let path = dir.join("plan.drift");
        std::fs::write(&path, b"the good version").unwrap();
        let failed = replace_with(&path, Backup::Keep, |file| {
            use std::io::Write as _;
            file.write_all(b"half of the new")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"the good version");
        // No temporary file is left behind, and no backup was made for a save that failed.
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["plan.drift"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_keeps_the_previous_version_as_bak() {
        let dir = folder("bak");
        let path = dir.join("plan.drift");
        write_atomic(&path, b"first", Backup::Keep).unwrap();
        assert!(
            !dir.join("plan.drift.bak").exists(),
            "nothing to back up yet"
        );
        write_atomic(&path, b"second", Backup::Keep).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read(dir.join("plan.drift.bak")).unwrap(), b"first");
        write_atomic(&path, b"third", Backup::Keep).unwrap();
        assert_eq!(
            std::fs::read(dir.join("plan.drift.bak")).unwrap(),
            b"second"
        );
        let export = dir.join("plan.pdf");
        write_atomic(&export, b"a", Backup::None).unwrap();
        write_atomic(&export, b"b", Backup::None).unwrap();
        assert!(!dir.join("plan.pdf.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
