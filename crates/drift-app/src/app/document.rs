//! Opening, saving and restoring documents, and undo/redo.

use crate::*;

impl DriftApp {
    pub(crate) fn is_dirty(&self) -> bool {
        self.file.clean_revision != Some(self.editor.doc.revision())
    }

    pub(crate) fn replace_document(
        &mut self,
        doc: Document,
        name: String,
        path: Option<FilePath>,
        meta: Metadata,
        clean: bool,
    ) {
        self.tool.cancel(&mut self.editor);
        self.tool = ToolKind::Select.create();
        let revision = doc.revision();
        self.editor = Editor::new(doc);
        let bounds = self.editor.bounds();
        self.fit_target = Some(if bounds.is_empty() {
            EMPTY_VIEW
        } else {
            bounds
        });
        self.file = FileState {
            name,
            path,
            meta,
            clean_revision: clean.then_some(revision),
        };
    }

    pub(crate) fn restore_session(&mut self, storage: &dyn eframe::Storage) {
        let Some(text) = storage.get_string(KEY_DOCUMENT) else {
            return;
        };
        match drift_io::load(&text) {
            Ok((doc, meta)) => {
                let name = storage
                    .get_string(KEY_NAME)
                    .unwrap_or_else(|| UNTITLED.into());
                let path = storage
                    .get_string(KEY_PATH)
                    .filter(|p| !p.is_empty() && cfg!(not(target_arch = "wasm32")))
                    .map(FilePath::from);
                let dirty = storage.get_string(KEY_DIRTY).as_deref() == Some("1");
                self.replace_document(doc, name, path, meta, !dirty);
            }
            Err(e) => log::warn!("could not restore the previous session: {e}"),
        }
    }

    /// Run `action` now, or ask first if it would discard unsaved changes.
    pub(crate) fn request(&mut self, action: Pending) {
        if self.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub(crate) fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => {
                self.replace_document(
                    Document::new(),
                    UNTITLED.into(),
                    None,
                    Metadata::default(),
                    true,
                );
            }
            Pending::Open => self.files.open(DRIFT_FILE),
            Pending::Example => {
                self.replace_document(
                    demo::mounting_plate(),
                    "Mounting plate.drift".into(),
                    None,
                    Metadata::default(),
                    false,
                );
            }
        }
    }

    pub(crate) fn file_bytes(&self) -> Vec<u8> {
        let mut meta = self.file.meta.clone();
        meta.generator = Some(format!("Drift {}", env!("CARGO_PKG_VERSION")));
        drift_io::save(&self.editor.doc, &meta).into_bytes()
    }

    pub(crate) fn save(&mut self) {
        match &self.file.path {
            Some(path) => self.files.save_to(path, &self.file_bytes()),
            None => self.save_as(),
        }
    }

    pub(crate) fn save_as(&mut self) {
        self.files
            .save_as(&self.file.name, DRIFT_FILE, self.file_bytes());
    }

    pub(crate) fn handle_file_events(&mut self, now: f64) {
        for event in self.files.take_events() {
            match event {
                FileEvent::Opened {
                    name,
                    path,
                    contents,
                } => {
                    let text = String::from_utf8_lossy(&contents);
                    match drift_io::load(&text) {
                        Ok((doc, meta)) => {
                            self.set_status(format!("Opened {name}"), false, now);
                            self.replace_document(doc, name, path, meta, true);
                        }
                        Err(e) => self.set_status(format!("Could not open {name}: {e}"), true, now),
                    }
                }
                FileEvent::Saved { name, path } => {
                    self.set_status(format!("Saved {name}"), false, now);
                    self.file.name = name;
                    if path.is_some() {
                        self.file.path = path;
                    }
                    self.file.clean_revision = Some(self.editor.doc.revision());
                }
                FileEvent::Failed(message) => self.set_status(message, true, now),
            }
        }
    }

    pub(crate) fn set_status(&mut self, text: String, error: bool, now: f64) {
        let until = now + if error { 12.0 } else { 4.0 };
        if error {
            log::warn!("{text}");
        }
        self.status = Some(Status { text, error, until });
    }

    /// Abort the active tool's operation in progress, keeping the selection.
    /// True if the document shows imperial units (for grid steps).
    pub(crate) fn imperial(&self) -> bool {
        !self.editor.doc.units().length.is_metric()
    }

    pub(crate) fn abort_operation(&mut self) {
        if self.tool.is_busy() {
            self.tool.cancel(&mut self.editor);
        }
    }

    pub(crate) fn set_tool(&mut self, kind: ToolKind) {
        if self.tool.kind() != kind {
            self.abort_operation();
            self.typing = None;
            self.inference_points.clear();
            self.editor.highlight.clear();
            self.tool = kind.create();
            self.editor.hovered = None;
        }
    }

    pub(crate) fn undo(&mut self) {
        self.abort_operation();
        self.editor.undo();
    }

    pub(crate) fn redo(&mut self) {
        self.abort_operation();
        self.editor.redo();
    }

    pub(crate) fn set_selection_style(&mut self, style: LineKind) {
        let ed = &mut self.editor;
        let ids: Vec<_> = ed
            .selection
            .iter()
            .copied()
            .filter(|id| ed.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        ed.doc.begin();
        for id in ids {
            ed.doc.set_style(id, style);
        }
        ed.commit("Change style");
    }
}
