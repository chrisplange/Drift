//! Platform services for Drift: opening and saving files on native and web.
//!
//! Dialogs are asynchronous everywhere (the browser has no blocking dialogs), so results arrive
//! later as [`FileEvent`]s. Call [`Files::take_events`] each frame; the `wake` callback passed to
//! [`Files::new`] is invoked when an event arrives so the UI can repaint.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    Failed(String),
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
    pub fn open(&self, file_type: FileType) {
        let this = self.clone();
        spawn(async move {
            let dialog =
                rfd::AsyncFileDialog::new().add_filter(file_type.name, file_type.extensions);
            let Some(handle) = dialog.pick_file().await else {
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

    /// Ask the user where to save, then write `contents` there. On the web this downloads the
    /// file under `suggested_name`.
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

    /// Overwrite a file that was previously opened or saved (native only).
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(future));
}

#[cfg(target_arch = "wasm32")]
fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
}

#[cfg(target_arch = "wasm32")]
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

    fn try_download(name: &str, contents: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let bytes = js_sys::Uint8Array::from(contents);
        let parts = js_sys::Array::of1(&bytes);
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let anchor: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        web_sys::Url::revoke_object_url(&url)?;
        Ok(())
    }
}
