//! Platform services for Drift: opening and saving files on native and web.
//!
//! Dialogs are asynchronous everywhere (the browser has no blocking dialogs), so results arrive
//! later as [`FileEvent`]s. Call [`Files::take_events`] each frame; the `wake` callback passed to
//! [`Files::new`] is invoked when an event arrives so the UI can repaint.

pub mod crash;
pub mod fonts;
pub mod memory;

use std::path::PathBuf;
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
}

impl Files {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            events: Arc::default(),
            wake: Arc::new(wake),
        }
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
        spawn(async move {
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
        spawn(async move {
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
            spawn(async move {
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter(file_type.name, file_type.extensions)
                    .set_file_name(&name);
                if let Some(handle) = dialog.save_file().await {
                    let path = handle.path().to_owned();
                    this.post(write_native(path, &contents));
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
            spawn(async move {
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter(file_type.name, file_type.extensions)
                    .set_file_name(&name);
                if let Some(handle) = dialog.save_file().await {
                    let path = handle.path().to_owned();
                    this.post(match write_native(path, &contents) {
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
        self.post(write_native(path.clone(), contents));
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
fn write_native(path: PathBuf, contents: &[u8]) -> FileEvent {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match std::fs::write(&path, contents) {
        Ok(()) => FileEvent::Saved {
            name,
            path: Some(path),
        },
        Err(e) => FileEvent::Failed(format!("could not save {}: {e}", path.display())),
    }
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
