//! Opening, saving and restoring documents, and undo/redo.

use crate::*;

/// Whether a path names a sketch (by its extension).
fn is_sketch_file(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(drift_io::EXTENSION))
}

impl DriftApp {
    /// Once a second: note how much memory is in use, and warn (once) when it's far more
    /// than any sketch should need. The guard thread ends the app at the hard limit.
    pub(crate) fn check_memory(&mut self, now: f64) {
        use drift_platform::memory;
        let (_, checked, warned) = self.memory;
        if now - checked < 1.0 {
            return;
        }
        let used = memory::used().unwrap_or(0);
        // Only where the guard runs: a test process holds many app instances at once.
        let high = memory::guarded() && used > memory::WARN_BYTES;
        if high && !warned {
            let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
            self.set_status(
                format!(
                    "Drift is using {:.1} GB of memory, far more than usual. Save your work:                      it stops itself at {:.0} GB to protect the system.",
                    gb(used),
                    gb(memory::limit())
                ),
                true,
                now,
            );
        }
        // Warn again only after it has come back down.
        let warned = if used < memory::WARN_BYTES / 4 * 3 {
            false
        } else {
            warned || high
        };
        self.memory = (used, now, warned);
    }

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
        self.fit_target = Some(self.fit_bounds());
        self.file = FileState {
            name,
            path,
            meta,
            clean_revision: clean.then_some(revision),
        };
    }

    /// View preferences as `key=0|1` pairs (unknown keys are ignored when reading).
    pub(crate) fn view_prefs(&self) -> String {
        let flags = [
            ("grid", self.show_grid),
            ("axes", self.show_axes),
            ("snap", self.grid_snap),
            ("thin", self.thin_lines),
            ("status", self.status_colors),
            ("dims", self.show_dimensions),
            ("rels", self.show_relations),
            ("auto", self.auto_relations),
            ("ends", self.always_endpoints),
        ];
        let mut prefs: Vec<String> = flags
            .iter()
            .map(|(k, v)| format!("{k}={}", u8::from(*v)))
            .collect();
        prefs.extend(self.layout.prefs());
        prefs.join(";")
    }

    pub(crate) fn apply_view_prefs(&mut self, text: &str) {
        let pairs: Vec<(&str, &str)> = text.split(';').filter_map(|p| p.split_once('=')).collect();
        self.layout.apply(&pairs);
        for pair in text.split(';') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let on = value == "1";
            match key {
                "grid" => self.show_grid = on,
                "axes" => self.show_axes = on,
                "snap" => self.grid_snap = on,
                "thin" => self.thin_lines = on,
                "status" => self.status_colors = on,
                "dims" => self.show_dimensions = on,
                "rels" => self.show_relations = on,
                "auto" => self.auto_relations = on,
                "ends" => self.always_endpoints = on,
                _ => {}
            }
        }
    }

    pub(crate) fn restore_session(&mut self, storage: &dyn eframe::Storage) {
        if let Some(prefs) = storage.get_string(KEY_VIEW) {
            self.apply_view_prefs(&prefs);
        }
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
                    .map(FilePath::from)
                    .filter(|p| is_sketch_file(p));
                // An older build could leave an export's name here (e.g. "Plan.pdf").
                let name = if name.to_ascii_lowercase().ends_with(".drift") {
                    name
                } else {
                    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                    format!("{stem}.drift")
                };
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
        // Only ever overwrite a sketch file: never an export or an imported drawing.
        match &self.file.path {
            Some(path) if is_sketch_file(path) => self.files.save_to(path, &self.file_bytes()),
            _ => self.save_as(),
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
                    let lower = name.to_ascii_lowercase();
                    // Real PDFs (by content, not name) are only ever underlays; DXF files are
                    // when asked for.
                    let as_underlay = std::mem::take(&mut self.import_as_underlay);
                    if contents.starts_with(b"%PDF") || (as_underlay && lower.ends_with(".dxf")) {
                        self.import_underlay_file(&name, contents, now);
                        continue;
                    }
                    let text = String::from_utf8_lossy(&contents);
                    if name.to_ascii_lowercase().ends_with(".dxf") {
                        self.import_dxf(&name, &text, now);
                        continue;
                    }
                    if name.to_ascii_lowercase().ends_with(".pat") {
                        match self.import_patterns(&text) {
                            Ok(n) => self.set_status(
                                format!("Imported {n} hatch pattern(s) from {name}"),
                                false,
                                now,
                            ),
                            Err(e) => {
                                self.set_status(format!("Could not import {name}: {e}"), true, now)
                            }
                        }
                        continue;
                    }
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
                FileEvent::Exported { name } => {
                    self.set_status(format!("Exported {name}"), false, now);
                }
                FileEvent::Shown { name } => {
                    self.set_status(format!("Opened {name} in the system viewer"), false, now);
                }
                FileEvent::Failed(message) => self.set_status(message, true, now),
            }
        }
    }

    /// Add the patterns in a `.pat` file to the document (replacing same-named ones).
    pub(crate) fn import_patterns(&mut self, text: &str) -> Result<usize, String> {
        let new = drift_draft::parse_pat(text)?;
        if new.is_empty() {
            return Err("no patterns found".into());
        }
        let mut all = drift_draft::parse_pat(self.editor.doc.custom_patterns()).unwrap_or_default();
        all.retain(|p| !new.iter().any(|n| n.name.eq_ignore_ascii_case(&p.name)));
        let count = new.len();
        all.extend(new);
        self.editor.doc.begin();
        self.editor
            .doc
            .set_custom_patterns(drift_draft::to_pat(&all));
        self.editor.commit("Import hatch patterns");
        Ok(count)
    }

    /// Remove an imported pattern (fills using it fall back to a tint until re-imported).
    pub(crate) fn remove_pattern(&mut self, name: &str) {
        let mut all = drift_draft::parse_pat(self.editor.doc.custom_patterns()).unwrap_or_default();
        all.retain(|p| p.name != name);
        let text = if all.is_empty() {
            String::new()
        } else {
            drift_draft::to_pat(&all)
        };
        self.editor.doc.begin();
        self.editor.doc.set_custom_patterns(text);
        self.editor.commit("Remove hatch pattern");
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
            self.editor.hovered_fill = None;
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
