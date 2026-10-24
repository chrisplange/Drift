//! Opening, saving and restoring documents, and undo/redo.

use crate::*;

/// The live link to Blender writes the profiles file at most this often (seconds).
pub(crate) const LIVE_PROFILES_S: f64 = 0.1;

/// How many switches the View menu has (see `view_flags`).
pub(crate) const VIEW_FLAGS: usize = 12;

/// What an older file's rules are taken from, said once as it opens.
pub(crate) const OLDER_RULES: &str =
    "Area and fill rules taken from your settings; save to keep them in the file";

/// Said before a save that would leave out things a newer Drift wrote (see `drift_io::Saved`).
#[cfg(not(feature = "demo"))]
pub(crate) const LOSSY_SAVE: &str = "This drawing has things from a newer Drift that this \
     version doesn't know; saving here may lose them.";

/// Read one of the rules new drawings start with from a saved preference (`key=value`).
/// False when `key` isn't one of them.
pub(crate) fn rules_pref(rules: &mut drift_core::DrawingRules, key: &str, value: &str) -> bool {
    let on = value == "1";
    match key {
        "rlayer" => rules.regions_by_layer = on,
        "rfill" => rules.regions_need_fill = on,
        "stack" => rules.stacked_fills = on,
        "dfollow" => rules.follow_divided_fills = on,
        "gaptol" => {
            if let Some(t) = value
                .parse::<f64>()
                .ok()
                .filter(|t| *t > 0.0 && t.is_finite())
            {
                rules.gap_tolerance = t;
            }
        }
        _ => return false,
    }
    true
}

/// The rules new drawings start with, from saved preferences (see `view_prefs`).
#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
pub(crate) fn rules_from_prefs(text: &str) -> drift_core::DrawingRules {
    let mut rules = drift_core::DrawingRules::default();
    for (key, value) in text.split(';').filter_map(|p| p.split_once('=')) {
        rules_pref(&mut rules, key, value);
    }
    rules
}

/// The rules Drift starts new drawings with on this computer, read from its saved settings
/// (for the assistant's server, which runs apart from the app). The defaults if there are
/// none.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
pub fn saved_new_rules() -> drift_core::DrawingRules {
    let text = eframe::storage_dir("Drift")
        .and_then(|dir| std::fs::read_to_string(dir.join("app.ron")).ok())
        .unwrap_or_default();
    // A RON map of strings: `"drift.view": "grid=1;…"` (the preferences hold no quotes).
    let key = format!("\"{KEY_VIEW}\"");
    let prefs = text
        .split_once(&key)
        .and_then(|(_, rest)| rest.trim_start().strip_prefix(':'))
        .and_then(|rest| rest.trim_start().strip_prefix('"'))
        .and_then(|rest| rest.split_once('"'))
        .map_or("", |(prefs, _)| prefs);
    rules_from_prefs(prefs)
}

/// What a file picker was opened for: the file chosen there is used for that, whatever its
/// name (#151: a logo is an SVG, which Import also takes). Files opened otherwise (dropped,
/// File → Open, a recent file) go by their name and content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picking {
    /// The title block logo (Drawing settings → Logo → Choose SVG…).
    Logo,
    /// A font to keep in the drawing (Add a font file… in a font menu).
    Font,
    /// Hatch patterns (Parameters → Hatch patterns → Import .pat…).
    Patterns,
}

impl Picking {
    fn file_type(self) -> FileType {
        match self {
            Picking::Logo => crate::app::sheets_ui::SVG_FILE,
            Picking::Font => crate::FONT_FILE,
            Picking::Patterns => crate::PAT_FILE,
        }
    }
}

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
        mut doc: Document,
        name: String,
        path: Option<FilePath>,
        meta: Metadata,
        clean: bool,
    ) {
        self.tool.cancel(&mut self.editor);
        self.tool = ToolKind::Select.create();
        // A file from before drawings kept their rules had this computer's.
        if meta.older_rules {
            doc.set_rules(self.new_rules);
            self.status_next = Some(OLDER_RULES.into());
        }
        let revision = doc.revision();
        // Preferences kept on the editor carry over to the next drawing.
        let mut editor = Editor::new(doc);
        editor.keep_prefs_from(&self.editor);
        self.editor = editor;
        // An isolate belongs to the drawing it was made in (and its undo history).
        self.layer_list.isolated = None;
        self.layer_list.solos.clear();
        self.fit_target = Some(self.fit_bounds());
        self.file = FileState {
            name,
            path,
            meta,
            clean_revision: clean.then_some(revision),
        };
    }

    /// Preferences and view settings as `key=0|1` pairs (unknown keys are ignored when
    /// reading). Hidden dimensions and relations aren't kept: they are back each start.
    pub(crate) fn view_prefs(&self) -> String {
        let rules = self.new_rules;
        let flags = [
            ("grid", self.show_grid),
            ("axes", self.show_axes),
            ("snap", self.grid_snap),
            ("ptsnap", self.perp_tangent_snap),
            ("thin", self.thin_lines),
            ("status", self.status_colors),
            ("auto", self.auto_relations),
            ("ends", self.always_endpoints),
            ("crops", self.show_view_crops),
            ("combs", self.show_combs),
            ("rscale", self.round_scale),
            ("hidden", self.show_hidden_annotations),
            // The rules new drawings start with (they were preferences before format 39).
            ("rlayer", rules.regions_by_layer),
            ("rfill", rules.regions_need_fill),
            ("stack", rules.stacked_fills),
            ("dfollow", rules.follow_divided_fills),
            // The tool bars' Fill switch, as it was last left.
            ("newfill", self.editor.fill_new_shapes),
            ("tdims", self.show_temp_dims),
            ("ai", self.ai_link),
        ];
        let mut prefs: Vec<String> = flags
            .iter()
            .map(|(k, v)| format!("{k}={}", u8::from(*v)))
            .collect();
        prefs.push(format!("wheel={}", self.wheel.mode.key()));
        prefs.push(format!("asnap={}", self.angle_snap.unwrap_or(0.0)));
        prefs.push(format!("gaptol={}", rules.gap_tolerance));
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
            if self.crosshair.apply(key, value) || rules_pref(&mut self.new_rules, key, value) {
                continue;
            }
            match key {
                "asnap" => {
                    self.angle_snap = value
                        .parse::<f64>()
                        .ok()
                        .filter(|step| ANGLE_STEPS.contains(step));
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
                "auto" => self.auto_relations = on,
                "ends" => self.always_endpoints = on,
                "crops" => self.show_view_crops = on,
                "combs" => self.show_combs = on,
                "rscale" => self.round_scale = on,
                "hidden" => self.show_hidden_annotations = on,
                "newfill" => self.editor.fill_new_shapes = on,
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
        self.restore_session_keeping_in(storage, drift_platform::crash::data_dir());
    }

    /// [`Self::restore_session`], keeping a session that can't be opened in `dir`.
    pub(crate) fn restore_session_keeping_in(
        &mut self,
        storage: &dyn eframe::Storage,
        dir: Option<std::path::PathBuf>,
    ) {
        if let Some(prefs) = storage.get_string(KEY_VIEW) {
            self.apply_view_prefs(&prefs);
        }
        self.start_with_new_rules();
        if let Some(recent) = storage.get_string(KEY_RECENT) {
            self.recent = recent
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(FilePath::from)
                .take(MAX_RECENT)
                .collect();
        }
        // The demo starts afresh each time (and never kept a drawing to restore).
        let Some(text) = storage
            .get_string(KEY_DOCUMENT)
            .filter(|t| !t.is_empty() && cfg!(not(feature = "demo")))
        else {
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
            Err(e) => self.hold_session = self.keep_unrestored(&text, &e, dir),
        }
    }

    /// A new, empty drawing, with the rules new drawings start with on this computer. Every
    /// new drawing starts here: File → New, a file opened as a new drawing, and the
    /// assistant's `new_drawing`. (The example is a drawing of its own, opened as it is.)
    pub(crate) fn new_document(&self) -> Document {
        let mut doc = Document::new();
        doc.set_rules(self.new_rules);
        doc
    }

    /// Give the drawing open now (a new, untouched one) the rules new drawings start with.
    pub(crate) fn start_with_new_rules(&mut self) {
        let clean = !self.is_dirty();
        self.editor.doc.set_rules(self.new_rules);
        if clean {
            self.file.clean_revision = Some(self.editor.doc.revision());
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

    /// Open a sketch by its path (the command line, or a file dropped on the window). A
    /// drawing Import takes asks first whether to open it as a new drawing or import it
    /// into this one. A sketch joins the recent files once it has been read.
    pub fn open_file(&mut self, path: FilePath) {
        if is_sketch_file(&path) {
            self.open_path = Some(path);
            self.request(Pending::OpenPath);
        } else if path
            .file_name()
            .is_some_and(|n| app::import_ui::importable(&n.to_string_lossy()))
        {
            self.dropped = Some(path);
        } else {
            // Fonts and hatch patterns go into the open sketch, which stays.
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
                let doc = self.new_document();
                self.replace_document(doc, UNTITLED.into(), None, Metadata::default(), true);
            }
            Pending::Open => self.files.open(app::import_ui::OPEN_FILE),
            Pending::Recent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    self.files.open_path(&path);
                }
            }
            Pending::OpenPath => {
                if let Some(path) = self.open_path.take() {
                    self.files.open_path(&path);
                }
            }
            Pending::ImportNew => {
                // Made in the next frame (see `import_dialog`), which knows the time.
                if let Some((_, go)) = &mut self.import_new {
                    *go = true;
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

    #[cfg(not(feature = "demo"))]
    fn saved(&self) -> drift_io::Saved {
        let mut meta = self.file.meta.clone();
        meta.generator = Some(format!("Drift {}", drift_platform::VERSION));
        drift_io::save_checked(&self.editor.doc, &meta)
    }

    #[cfg(not(feature = "demo"))]
    pub(crate) fn save(&mut self) {
        self.write_file(false, false);
    }

    #[cfg(not(feature = "demo"))]
    pub(crate) fn save_as(&mut self) {
        self.write_file(true, false);
    }

    /// Save, or Save As (`as_new`). When that would leave out things from a newer Drift that
    /// this one doesn't know (a record that held them was deleted), ask first, unless that was
    /// `confirmed`.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn write_file(&mut self, as_new: bool, confirmed: bool) {
        let saved = self.saved();
        if saved.loses_newer_data && !confirmed {
            self.lossy_save = Some(as_new);
            return;
        }
        self.saving_carried = Some(saved.carried);
        let bytes = saved.text.into_bytes();
        // Only ever overwrite a sketch file: never an export or an imported drawing.
        match &self.file.path {
            Some(path) if !as_new && is_sketch_file(path) => self.files.save_to(path, &bytes),
            _ => self.files.save_as(&self.file.name, DRIFT_FILE, bytes),
        }
    }

    #[cfg(feature = "demo")]
    pub(crate) fn save(&mut self) {
        self.not_in_demo("Saving");
    }

    #[cfg(feature = "demo")]
    pub(crate) fn save_as(&mut self) {
        self.not_in_demo("Saving");
    }

    #[cfg(feature = "demo")]
    pub(crate) fn export_profiles(&mut self) {
        self.not_in_demo("Exporting Blender profiles");
    }

    /// The Blender profiles file's text, or `None` when no block is marked as a profile.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn profiles_text(&self) -> Option<String> {
        let blocks = self.editor.blocks_as_drawn();
        drift_io::profiles::write(blocks.iter().map(|(id, b)| (*id, b)), &self.file.name)
    }

    /// File → Export → Blender profiles: the profiles file, wherever it's wanted.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn export_profiles(&mut self) {
        match self.profiles_text() {
            Some(text) => {
                let name = format!("{}.{}", self.file_stem(), drift_io::profiles::EXTENSION);
                self.files.export(
                    &name,
                    crate::app::sheets_ui::PROFILES_FILE,
                    text.into_bytes(),
                );
            }
            None => {
                self.status_next = Some(
                    "No Blender profiles to export: mark a block as one in the Blocks panel or \
                     the Inspector"
                        .into(),
                );
            }
        }
    }

    /// After saving the drawing at `path`: write its Blender profiles beside it, or delete
    /// the ones left from before when no block is a profile now. What to add to "Saved …".
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
    fn update_profiles_beside(&self, path: &std::path::Path) -> Result<String, String> {
        use drift_io::profiles::{Updated, update_beside};
        let blocks = self.editor.blocks_as_drawn();
        match update_beside(path, blocks.iter().map(|(id, b)| (*id, b))) {
            Ok(Updated::Written(n)) => Ok(format!(
                " and {n} Blender profile{}",
                if n == 1 { "" } else { "s" }
            )),
            Ok(Updated::Removed) => Ok(" (no Blender profiles now: their file is removed)".into()),
            Ok(Updated::Nothing) => Ok(String::new()),
            Err(e) => Err(format!(
                ", but the Blender profiles could not be written: {e}"
            )),
        }
    }

    /// The live link to Blender: while a block marked as a Blender profile is open for
    /// editing, rewrite the profiles file beside the drawing as the block changes, at most
    /// every [`LIVE_PROFILES_S`], so Blender's Auto-reload follows the edit before it's
    /// saved. Returns how long to wait before a change still to be written can go out.
    pub(crate) fn live_profiles(&mut self, now: f64) -> Option<f64> {
        let editing = self
            .editor
            .block_edit
            .filter(|e| self.editor.doc.block(e.block).is_some_and(|b| b.profile));
        let (Some(_), Some(path)) = (
            editing,
            self.file.path.as_deref().filter(|p| is_sketch_file(p)),
        ) else {
            self.live_profiles = None;
            return None;
        };
        let revision = self.editor.doc.revision();
        match self.live_profiles {
            Some((written, _)) if written == revision => return None,
            Some((_, at)) if now - at < LIVE_PROFILES_S => {
                return Some(LIVE_PROFILES_S - (now - at));
            }
            _ => {}
        }
        // A failed write is tried again on the next change, and saving reports it. The demo
        // writes no files.
        #[cfg(not(feature = "demo"))]
        {
            let blocks = self.editor.blocks_as_drawn();
            let _ = drift_io::profiles::update_beside(path, blocks.iter().map(|(id, b)| (*id, b)));
        }
        #[cfg(feature = "demo")]
        let _ = path;
        self.live_profiles = Some((revision, now));
        None
    }

    /// Open the file picker for `what`: the file chosen comes back to be used for that.
    pub(crate) fn pick_file(&mut self, what: Picking) {
        self.picking = Some(what);
        // (Tests hand over their files themselves: no system dialog.)
        #[cfg(not(test))]
        self.files.open(what.file_type());
        #[cfg(test)]
        let _ = what.file_type();
    }

    /// Use a file chosen in a picker opened for `what`.
    fn use_picked(&mut self, what: Picking, name: &str, contents: Vec<u8>, now: f64) {
        let done = match what {
            Picking::Logo => self
                .set_logo(&String::from_utf8_lossy(&contents))
                .map(|()| format!("{name} is now the title block logo")),
            Picking::Font => self.editor.add_font_file(contents).map(|family| {
                format!("{family} is now kept in this drawing: pick it in a font menu")
            }),
            Picking::Patterns => self
                .import_patterns(&String::from_utf8_lossy(&contents))
                .map(|n| format!("Imported {n} hatch pattern(s) from {name}")),
        };
        match done {
            Ok(said) => self.set_status(said, false, now),
            Err(e) => self.set_status(format!("Could not use {name}: {e}"), true, now),
        }
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
                    // Chosen in a picker opened for something: used for that.
                    if let Some(what) = self.picking.take() {
                        self.import_as = None;
                        self.use_picked(what, &name, contents, now);
                        continue;
                    }
                    // A drawing to import (a PDF by its content, not its name) asks how it
                    // should come in, starting where that was chosen before it was picked,
                    // or (opened with File → Open) on a new drawing.
                    let import_as = self.import_as.take();
                    if contents.starts_with(b"%PDF") || app::import_ui::importable(&name) {
                        let as_ = import_as.unwrap_or(app::import_ui::ImportAs::NewDrawing);
                        self.begin_import(&name, contents, Some(as_), now);
                        continue;
                    }
                    // Dropped fonts and hatch patterns go into the open sketch, by their name.
                    if lower.ends_with(".ttf") || lower.ends_with(".otf") {
                        self.use_picked(Picking::Font, &name, contents, now);
                        continue;
                    }
                    if lower.ends_with(".pat") {
                        self.use_picked(Picking::Patterns, &name, contents, now);
                        continue;
                    }
                    let text = String::from_utf8_lossy(&contents);
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
                    #[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
                    let profiles = match path.as_deref().filter(|p| is_sketch_file(p)) {
                        Some(p) => self.update_profiles_beside(p),
                        None => Ok(String::new()),
                    };
                    #[cfg(any(target_arch = "wasm32", feature = "demo"))]
                    let profiles: Result<String, String> = Ok(String::new());
                    match profiles {
                        Ok(more) => self.set_status(format!("Saved {name}{more}"), false, now),
                        Err(more) => self.set_status(format!("Saved {name}{more}"), true, now),
                    }
                    self.file.name = name;
                    if let Some(p) = &path {
                        self.remember(p.clone());
                    }
                    if path.is_some() {
                        self.file.path = path;
                    }
                    self.file.clean_revision = Some(self.editor.doc.revision());
                    // The next save compares with this file.
                    #[cfg(not(feature = "demo"))]
                    if let Some(carried) = self.saving_carried.take() {
                        self.file.meta.carried = carried;
                    }
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
                FileEvent::Cancelled => {
                    // What the picked file was to be for goes with it.
                    self.import_as = None;
                    self.picking = None;
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

    /// The View menu's switches, with what the status bar says when each is turned on or
    /// off.
    fn view_flags(&self) -> [(bool, &'static str, &'static str); VIEW_FLAGS] {
        [
            (self.show_grid, "Grid shown", "Grid hidden"),
            (self.show_axes, "Origin axes shown", "Origin axes hidden"),
            (
                self.grid_snap,
                "Snapping to the grid",
                "Not snapping to the grid",
            ),
            (self.thin_lines, "Thin lines", "Lines at their weights"),
            (
                self.status_colors,
                "Curves coloured by constraint status",
                "Curves in their own and their layers' colours",
            ),
            (
                self.always_endpoints,
                "End points always shown",
                "End points shown on hovered and picked curves",
            ),
            (
                self.show_dimensions,
                "Dimensions and notes shown",
                "Dimensions and notes hidden (Alt+D shows them)",
            ),
            (
                self.show_hidden_annotations,
                "Hidden annotations shown, faded",
                "Hidden annotations not shown",
            ),
            (
                self.show_relations,
                "Relations shown",
                "Relations hidden (Alt+R shows them)",
            ),
            (
                self.show_combs,
                "Curvature combs shown",
                "Curvature combs hidden",
            ),
            (
                self.show_view_crops,
                "View crops shown",
                "View crops hidden",
            ),
            (
                self.params.show_free,
                "Free directions shown",
                "Free directions hidden",
            ),
        ]
    }

    /// Say in the status bar which View switch just changed.
    pub(crate) fn announce_view_changes(&mut self, now: f64) {
        let flags = self.view_flags();
        let values = flags.map(|(on, _, _)| on);
        if let Some(seen) = self.view_seen
            && let Some(changed) = (0..VIEW_FLAGS).find(|&i| seen[i] != values[i])
        {
            let (on, shown, hidden) = flags[changed];
            self.set_status(if on { shown } else { hidden }.into(), false, now);
        }
        self.view_seen = Some(values);
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
