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
                    "Drift is using {:.1} GB of memory, far more than usual. Save your work: it stops itself at {:.0} GB to protect the system.",
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
        // Preferences kept on the editor carry over to the next drawing.
        let mut editor = Editor::new(doc);
        editor.keep_prefs_from(&self.editor);
        self.editor = editor;
        // An isolate belongs to the drawing it was made in (and its undo history).
        self.layer_list.isolated = None;
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
            ("ptsnap", self.perp_tangent_snap),
            ("thin", self.thin_lines),
            ("status", self.status_colors),
            ("dims", self.show_dimensions),
            ("rels", self.show_relations),
            ("auto", self.auto_relations),
            ("ends", self.always_endpoints),
            ("crops", self.show_view_crops),
            ("rscale", self.round_scale),
            ("hidden", self.show_hidden_annotations),
            ("rlayer", self.editor.regions_by_layer),
            ("rfill", self.editor.regions_need_fill),
            ("tdims", self.show_temp_dims),
            ("ai", self.ai_link),
        ];
        let mut prefs: Vec<String> = flags
            .iter()
            .map(|(k, v)| format!("{k}={}", u8::from(*v)))
            .collect();
        prefs.push(format!("wheel={}", self.wheel.mode.key()));
        prefs.push(format!("asnap={}", self.angle_snap.unwrap_or(0.0)));
        prefs.push(format!("gaptol={}", self.gap_tolerance));
        prefs.extend(self.crosshair.prefs());
        #[cfg(not(target_arch = "wasm32"))]
        {
            prefs.push(format!("upchk={}", u8::from(self.updates.auto_check)));
            if let Some(skipped) = &self.updates.skipped {
                prefs.push(format!("upskip={skipped}"));
            }
        }
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
            if self.crosshair.apply(key, value) {
                continue;
            }
            match key {
                "asnap" => {
                    self.angle_snap = value
                        .parse::<f64>()
                        .ok()
                        .filter(|step| ANGLE_STEPS.contains(step));
                }
                "gaptol" => {
                    if let Some(t) = value
                        .parse::<f64>()
                        .ok()
                        .filter(|t| *t > 0.0 && t.is_finite())
                    {
                        self.gap_tolerance = t;
                    }
                }
                "wheel" => {
                    if let Some(mode) = crate::app::view::WheelMode::from_key(value) {
                        self.wheel.mode = mode;
                    }
                }
                "grid" => self.show_grid = on,
                "axes" => self.show_axes = on,
                "snap" => self.grid_snap = on,
                "ptsnap" => self.perp_tangent_snap = on,
                "thin" => self.thin_lines = on,
                "status" => self.status_colors = on,
                "dims" => self.show_dimensions = on,
                "rels" => self.show_relations = on,
                "auto" => self.auto_relations = on,
                "ends" => self.always_endpoints = on,
                "crops" => self.show_view_crops = on,
                "rscale" => self.round_scale = on,
                "hidden" => self.show_hidden_annotations = on,
                "rlayer" => self.editor.regions_by_layer = on,
                "rfill" => self.editor.regions_need_fill = on,
                "tdims" => self.show_temp_dims = on,
                "ai" => self.ai_link = on,
                #[cfg(not(target_arch = "wasm32"))]
                "upchk" => self.updates.auto_check = on,
                #[cfg(not(target_arch = "wasm32"))]
                "upskip" => self.updates.skipped = semver::Version::parse(value).ok(),
                _ => {}
            }
        }
    }

    pub(crate) fn restore_session(&mut self, storage: &dyn eframe::Storage) {
        if let Some(prefs) = storage.get_string(KEY_VIEW) {
            self.apply_view_prefs(&prefs);
        }
        if let Some(recent) = storage.get_string(KEY_RECENT) {
            self.recent = recent
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(FilePath::from)
                .take(MAX_RECENT)
                .collect();
        }
        let Some(text) = storage.get_string(KEY_DOCUMENT).filter(|t| !t.is_empty()) else {
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

    /// Put a sketch at the top of the recent files.
    pub(crate) fn remember(&mut self, path: FilePath) {
        if !is_sketch_file(&path) {
            return;
        }
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }

    /// Open a sketch by its path (the command line, or a file dropped on the window).
    pub fn open_file(&mut self, path: FilePath) {
        if is_sketch_file(&path) {
            self.remember(path);
            self.request(Pending::Recent(0));
        } else {
            // A drawing in another format is brought into the open sketch, which stays.
            self.files.open_path(&path);
        }
    }

    /// The window was asked to close: with unsaved changes, hold it and ask first.
    pub(crate) fn handle_close(&mut self, ctx: &egui::Context) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && !self.quit_confirmed {
            if self.is_dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                // Bring the window up so the question is seen.
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
                self.pending = Some(Pending::Quit);
            } else {
                self.quit_confirmed = true;
            }
        }
        if self.quit_confirmed && !asked && !self.quit_sent {
            self.quit_sent = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                self.show_start = !cfg!(test);
                self.replace_document(
                    Document::new(),
                    UNTITLED.into(),
                    None,
                    Metadata::default(),
                    true,
                );
            }
            Pending::Open => self.files.open(DRIFT_FILE),
            Pending::Recent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    self.files.open_path(&path);
                }
            }
            Pending::Quit => self.quit_confirmed = true,
            #[cfg(not(target_arch = "wasm32"))]
            Pending::Update => self.install_update_and_quit(),
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
                    // From Import underlay: an underlay. Otherwise a PDF (by its content, not
                    // its name) or an SVG asks how it should come in.
                    let as_underlay = std::mem::take(&mut self.import_as_underlay);
                    let pdf = contents.starts_with(b"%PDF");
                    let cad = lower.ends_with(".dxf") || lower.ends_with(".dwg");
                    if as_underlay && (pdf || cad || lower.ends_with(".svg")) {
                        self.import_underlay_file(&name, contents, now);
                        continue;
                    }
                    if pdf || lower.ends_with(".svg") || lower.ends_with(".ai") {
                        self.begin_vector_import(&name, contents, now);
                        continue;
                    }
                    let text = String::from_utf8_lossy(&contents);
                    if lower.ends_with(".dxf") {
                        self.import_dxf(&name, &drift_io::dxf::decode(&contents), now);
                        continue;
                    }
                    if lower.ends_with(".dwg") {
                        self.import_dwg(&name, &contents, now);
                        continue;
                    }
                    if lower.ends_with(".ttf") || lower.ends_with(".otf") {
                        match self.editor.add_font_file(contents) {
                            Ok(family) => self.set_status(
                                format!(
                                    "{family} is now kept in this drawing: pick it in a font menu"
                                ),
                                false,
                                now,
                            ),
                            Err(e) => {
                                self.set_status(format!("Could not add {name}: {e}"), true, now)
                            }
                        }
                        continue;
                    }
                    if lower.ends_with(".svg") {
                        match self.set_logo(&text) {
                            Ok(()) => self.set_status(
                                format!("{name} is now the title block logo"),
                                false,
                                now,
                            ),
                            Err(e) => {
                                self.set_status(format!("Could not use {name}: {e}"), true, now)
                            }
                        }
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
                    match drift_io::load_with_notes(&text) {
                        Ok((doc, meta, dropped)) => {
                            let status = if dropped.is_empty() {
                                format!("Opened {name}")
                            } else {
                                format!(
                                    "Opened {name}, leaving out {} relation(s) that had become \
                                     meaningless: {}. Save to keep it that way",
                                    dropped.len(),
                                    dropped.join(", ")
                                )
                            };
                            self.set_status(status, !dropped.is_empty(), now);
                            if let Some(p) = &path {
                                self.remember(p.clone());
                            }
                            self.replace_document(doc, name, path, meta, true);
                        }
                        Err(e) => self.set_status(format!("Could not open {name}: {e}"), true, now),
                    }
                }
                FileEvent::Saved { name, path } => {
                    self.set_status(format!("Saved {name}"), false, now);
                    self.file.name = name;
                    if let Some(p) = &path {
                        self.remember(p.clone());
                    }
                    if path.is_some() {
                        self.file.path = path;
                    }
                    self.file.clean_revision = Some(self.editor.doc.revision());
                    // Saved from the unsaved-changes prompt: carry on with what was asked.
                    if let Some((action, revision)) = self.after_save.take()
                        && revision == self.editor.doc.revision()
                    {
                        self.perform(action);
                    }
                }
                FileEvent::Exported { name } => {
                    self.set_status(format!("Exported {name}"), false, now);
                }
                FileEvent::Shown { name } => {
                    self.set_status(format!("Opened {name} in the system viewer"), false, now);
                }
                FileEvent::Failed(message) => {
                    // A recent file that has gone is dropped from the list.
                    self.recent.retain(|p| p.exists());
                    self.set_status(message, true, now);
                }
            }
        }
    }

    /// Add the patterns in a `.pat` file to the document (replacing same-named ones).
    /// Make an SVG the title blocks' logo (one undo step), if it can be read.
    pub(crate) fn set_logo(&mut self, svg: &str) -> Result<(), String> {
        drift_draft::logo::parse(svg)?;
        self.editor.doc.begin();
        self.editor.doc.set_logo(svg.to_owned());
        self.editor.commit("Title block logo");
        Ok(())
    }

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
        // Another tool gives up a view being drawn for a sheet.
        if kind != ToolKind::View {
            self.add_view_on = None;
        }
        if self.sheet_preview && kind != ToolKind::Select {
            self.show_sheet(None);
            self.status_next = Some("Back in the model to draw".to_owned());
        }
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
