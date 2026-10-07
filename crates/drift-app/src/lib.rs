//! Drift application shell: UI chrome, canvas interaction, files and the GPU canvas callback.
//!
//! The same [`DriftApp`] runs natively and in the browser via eframe. Editing logic lives in
//! [`editor`] and [`tools`], which don't depend on egui.

mod annotations;
mod canvas;
pub mod editor;
mod overlay;
pub mod tools;
#[cfg(test)]
mod ui_tests;

use std::f64::consts::TAU;

use drift_core::{Bounds, Constraint, ConstraintId, DVec2, Document, LineKind, Shape, demo};
use drift_geom::SnapOptions;
use drift_io::Metadata;
use drift_platform::{FileEvent, FilePath, FileType, Files};
use drift_render::{Camera, grid_info};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers as EguiModifiers, PointerButton};

use crate::annotations::{AnnotationColors, AnnotationState, DimLayout, GlyphKind};
use crate::canvas::CanvasPainter;
use crate::editor::{Editor, Guide, GuideKind, Pointer};
use crate::overlay::{OverlayColors, ScreenMap};
use crate::tools::{Input, Modifiers, Overlay, Tool, ToolKind};

/// Zoom factor per logical point of scroll.
const SCROLL_ZOOM_SPEED: f64 = 0.0015;
/// Margin left around geometry when zooming to fit, in logical points.
const FIT_MARGIN: f64 = 40.0;
/// Pick and snap radius, in logical points.
const PICK_RADIUS: f64 = 8.0;
/// Below this window width the properties panel is hidden and the toolbar is compact.
const WIDE_LAYOUT: f32 = 760.0;
/// What an empty document shows.
const EMPTY_VIEW: Bounds = Bounds {
    min: DVec2::new(-100.0, -60.0),
    max: DVec2::new(100.0, 60.0),
};
const DRIFT_FILE: FileType = FileType {
    name: "Drift sketch",
    extensions: &[drift_io::EXTENSION],
};
const UNTITLED: &str = "Untitled.drift";

// Session-restore keys in eframe's storage (a file natively, localStorage on the web).
const KEY_DOCUMENT: &str = "drift.document";
const KEY_NAME: &str = "drift.name";
const KEY_PATH: &str = "drift.path";
const KEY_DIRTY: &str = "drift.dirty";

/// The open file and whether it has unsaved changes.
struct FileState {
    name: String,
    path: Option<FilePath>,
    meta: Metadata,
    /// Document revision at the last save or load. `None` means never saved.
    clean_revision: Option<u64>,
}

/// An action waiting for the user to confirm discarding unsaved changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    New,
    Open,
    Example,
}

/// A dimension value being edited in place on the canvas.
struct DimensionEdit {
    id: ConstraintId,
    text: String,
    /// Focus (and select all) on the next frame.
    focus: bool,
}

/// Numbers being typed into the active drawing tool.
struct Typing {
    fields: &'static [&'static str],
    values: [String; 2],
    active: usize,
}

impl Typing {
    fn new(fields: &'static [&'static str]) -> Self {
        Self {
            fields,
            values: Default::default(),
            active: 0,
        }
    }

    fn parsed(&self) -> [Option<f64>; 2] {
        [0, 1].map(|i| parse_value(&self.values[i]))
    }
}

struct Status {
    text: String,
    error: bool,
    until: f64,
}

pub struct DriftApp {
    editor: Editor,
    tool: Box<dyn Tool>,
    camera: Camera,
    painter: CanvasPainter,
    /// While set, the view keeps fitting these bounds as the canvas resizes (e.g. while the
    /// browser lays out the page), until the user pans or zooms.
    fit_target: Option<Bounds>,
    /// Last pointer position of an in-progress pan drag.
    pan_anchor: Option<egui::Pos2>,
    pointer: Option<Pointer>,
    grid_snap: bool,
    files: Files,
    file: FileState,
    pending: Option<Pending>,
    status: Option<Status>,
    show_shortcuts: bool,
    /// User override for the properties panel; by default it shows on wide windows only.
    show_properties: Option<bool>,
    backend: String,
    window_title: String,
    /// Where the canvas was laid out last frame.
    canvas_rect: egui::Rect,
    /// Text being typed into dimension value fields.
    dimension_text: std::collections::HashMap<ConstraintId, String>,
    /// A dimension field to focus next frame (e.g. right after creating the dimension).
    focus_dimension: Option<ConstraintId>,
    /// Results of Help → Run solver benchmark.
    benchmark: Option<Vec<String>>,
    show_dimensions: bool,
    show_relations: bool,
    auto_relations: bool,
    /// A relation or dimension picked on the canvas (Delete removes it).
    selected_constraint: Option<ConstraintId>,
    hovered_constraint: Option<ConstraintId>,
    /// A dimension label being dragged, and the grab offset from the label (world units).
    label_drag: Option<(ConstraintId, DVec2)>,
    dimension_edit: Option<DimensionEdit>,
    typing: Option<Typing>,
    /// Points the pointer recently snapped to; new points can align with them.
    inference_points: Vec<DVec2>,
}

impl DriftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("Drift requires the wgpu renderer");
        let painter = CanvasPainter::install(render_state);
        let backend = format!("{:?}", render_state.adapter.get_info().backend);
        log::info!("Drift renderer backend: {backend}");

        let ctx = cc.egui_ctx.clone();
        let files = Files::new(move || ctx.request_repaint());

        let mut app = Self {
            editor: Editor::new(Document::new()),
            tool: ToolKind::Select.create(),
            camera: Camera::default(),
            painter,
            fit_target: Some(EMPTY_VIEW),
            pan_anchor: None,
            pointer: None,
            grid_snap: false,
            files,
            file: FileState {
                name: UNTITLED.into(),
                path: None,
                meta: Metadata::default(),
                clean_revision: Some(0),
            },
            pending: None,
            status: None,
            show_shortcuts: false,
            show_properties: None,
            backend,
            window_title: String::new(),
            canvas_rect: egui::Rect::NOTHING,
            dimension_text: Default::default(),
            focus_dimension: None,
            benchmark: None,
            show_dimensions: true,
            show_relations: true,
            auto_relations: true,
            selected_constraint: None,
            hovered_constraint: None,
            label_drag: None,
            dimension_edit: None,
            typing: None,
            inference_points: Vec::new(),
        };
        if let Some(storage) = cc.storage {
            app.restore_session(storage);
        }
        app
    }

    // ----- Document lifecycle ---------------------------------------------------------------

    fn is_dirty(&self) -> bool {
        self.file.clean_revision != Some(self.editor.doc.revision())
    }

    fn replace_document(
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

    fn restore_session(&mut self, storage: &dyn eframe::Storage) {
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
    fn request(&mut self, action: Pending) {
        if self.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
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

    fn file_bytes(&self) -> Vec<u8> {
        let mut meta = self.file.meta.clone();
        meta.generator = Some(format!("Drift {}", env!("CARGO_PKG_VERSION")));
        drift_io::save(&self.editor.doc, &meta).into_bytes()
    }

    fn save(&mut self) {
        match &self.file.path {
            Some(path) => self.files.save_to(path, &self.file_bytes()),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) {
        self.files
            .save_as(&self.file.name, DRIFT_FILE, self.file_bytes());
    }

    fn handle_file_events(&mut self, now: f64) {
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

    fn set_status(&mut self, text: String, error: bool, now: f64) {
        let until = now + if error { 12.0 } else { 4.0 };
        if error {
            log::warn!("{text}");
        }
        self.status = Some(Status { text, error, until });
    }

    /// Abort the active tool's operation in progress, keeping the selection.
    fn abort_operation(&mut self) {
        if self.tool.is_busy() {
            self.tool.cancel(&mut self.editor);
        }
    }

    fn set_tool(&mut self, kind: ToolKind) {
        if self.tool.kind() != kind {
            self.abort_operation();
            self.typing = None;
            self.inference_points.clear();
            self.editor.highlight.clear();
            self.tool = kind.create();
            self.editor.hovered = None;
        }
    }

    fn undo(&mut self) {
        self.abort_operation();
        self.editor.undo();
    }

    fn redo(&mut self) {
        self.abort_operation();
        self.editor.redo();
    }

    fn set_selection_style(&mut self, style: LineKind) {
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

    // ----- Keyboard -------------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |key| KeyboardShortcut::new(EguiModifiers::COMMAND, key);
        let cmd_shift =
            |key| KeyboardShortcut::new(EguiModifiers::COMMAND | EguiModifiers::SHIFT, key);
        let consume = |shortcut: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&shortcut));

        // Order matters: check the more specific (Shift) variants first.
        if consume(cmd_shift(Key::Z)) || consume(cmd(Key::Y)) {
            self.redo();
        }
        if consume(cmd(Key::Z)) {
            self.undo();
        }
        if consume(cmd_shift(Key::S)) {
            self.save_as();
        }
        if consume(cmd(Key::S)) {
            self.save();
        }
        if consume(cmd(Key::O)) {
            self.request(Pending::Open);
        }

        if ctx.text_edit_focused() {
            return;
        }
        if consume(cmd(Key::A)) {
            self.set_tool(ToolKind::Select);
            self.editor.select_all();
        }
        let plain = ctx.input(|i| i.modifiers.is_none());
        let pressed = |key| plain && ctx.input(|i| i.key_pressed(key));

        if pressed(Key::Escape) {
            if self.selected_constraint.take().is_some() {
                // First Esc just clears a picked relation or dimension.
            } else if !self.tool.cancel(&mut self.editor) {
                self.set_tool(ToolKind::Select);
            }
        }
        if pressed(Key::Enter) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        if (pressed(Key::Delete) || pressed(Key::Backspace)) && !self.tool.is_busy() {
            match self.selected_constraint.take() {
                Some(id) => self.editor.remove_constraint(id),
                None => self.editor.delete_selection(),
            }
        }
        if pressed(Key::X) {
            self.editor.toggle_construction();
        }
        if pressed(Key::F) {
            self.fit_target = Some(self.fit_bounds());
        }
        if pressed(Key::G) {
            self.grid_snap = !self.grid_snap;
        }
        for kind in ToolKind::ALL {
            if let Some(key) = Key::from_name(&kind.shortcut().to_string())
                && pressed(key)
            {
                self.set_tool(kind);
            }
        }
    }

    /// Route number keys to the active tool's typed fields. Runs before other shortcuts so
    /// Enter, Esc, Tab and Backspace go to the typing when it is active.
    fn handle_typing(&mut self, ctx: &egui::Context) {
        let fields = self.tool.fields();
        // egui moves keyboard focus between widgets on Tab before we see the key. While a
        // tool takes typed values, Tab belongs to it, so release focus that landed on a
        // button (otherwise Enter would also click that button).
        if !fields.is_empty()
            && !ctx.text_edit_focused()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        if fields.is_empty() || ctx.text_edit_focused() {
            self.typing = None;
            self.tool.set_typed([None; 2]);
            return;
        }
        if self.typing.as_ref().is_some_and(|t| t.fields != fields) {
            self.typing = None;
        }
        let typed: String = ctx.input_mut(|i| {
            let mut typed = String::new();
            i.events.retain(|e| match e {
                egui::Event::Text(t)
                    if t.chars().all(|c| c.is_ascii_digit() || ".,-".contains(c)) =>
                {
                    typed.push_str(&t.replace(',', "."));
                    false
                }
                _ => true,
            });
            typed
        });
        let consume = |key| ctx.input_mut(|i| i.consume_key(EguiModifiers::NONE, key));
        if !typed.is_empty() {
            let t = self.typing.get_or_insert_with(|| Typing::new(fields));
            t.values[t.active].push_str(&typed);
        } else if self.typing.is_none() && fields.len() > 1 && consume(Key::Tab) {
            // Tab first: start with the second field (e.g. angle).
            let mut t = Typing::new(fields);
            t.active = 1;
            self.typing = Some(t);
        }
        if let Some(t) = &mut self.typing {
            if consume(Key::Tab) {
                t.active = (t.active + 1) % t.fields.len();
            }
            if consume(Key::Backspace) {
                t.values[t.active].pop();
            }
            if consume(Key::Escape) {
                self.typing = None;
            } else if consume(Key::Enter) {
                let values = t.parsed();
                self.typing = None;
                let pointer = self.pointer.unwrap_or_else(|| {
                    Pointer::new(self.camera.center, PICK_RADIUS / self.camera.scale)
                });
                self.tool.set_typed(values);
                self.tool.commit_typed(&pointer, &mut self.editor);
            }
        }
        let values = self.typing.as_ref().map_or([None; 2], Typing::parsed);
        self.tool.set_typed(values);
    }

    fn fit_bounds(&self) -> Bounds {
        let b = self.editor.bounds();
        if b.is_empty() { EMPTY_VIEW } else { b }
    }

    // ----- Panels ---------------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let command = |key: &str| {
            if cfg!(target_os = "macos") {
                format!("Cmd+{key}")
            } else {
                format!("Ctrl+{key}")
            }
        };
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.request(Pending::New);
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text(command("O")))
                    .clicked()
                {
                    self.request(Pending::Open);
                }
                if ui.button("Open example").clicked() {
                    self.request(Pending::Example);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text(command("S")))
                    .clicked()
                {
                    self.save();
                }
                if ui
                    .add(egui::Button::new("Save as…").shortcut_text(command("Shift+S")))
                    .clicked()
                {
                    self.save_as();
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = match self.editor.history.undo_label() {
                    Some(label) => format!("Undo {label}"),
                    None => "Undo".into(),
                };
                let redo = match self.editor.history.redo_label() {
                    Some(label) => format!("Redo {label}"),
                    None => "Redo".into(),
                };
                if ui
                    .add_enabled(
                        self.editor.history.can_undo(),
                        egui::Button::new(undo).shortcut_text(command("Z")),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(
                        self.editor.history.can_redo(),
                        egui::Button::new(redo).shortcut_text(command("Y")),
                    )
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                let has_selection = !self.editor.selection.is_empty();
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new("Delete").shortcut_text("Del"),
                    )
                    .clicked()
                {
                    self.editor.delete_selection();
                }
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new("Toggle construction").shortcut_text("X"),
                    )
                    .clicked()
                {
                    self.editor.toggle_construction();
                }
                if ui
                    .add(egui::Button::new("Select all").shortcut_text(command("A")))
                    .clicked()
                {
                    self.set_tool(ToolKind::Select);
                    self.editor.select_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Zoom to fit").shortcut_text("F"))
                    .clicked()
                {
                    self.fit_target = Some(self.fit_bounds());
                }
                ui.checkbox(&mut self.grid_snap, "Snap to grid (G)");
                ui.checkbox(&mut self.auto_relations, "Automatic relations");
                ui.checkbox(&mut self.show_dimensions, "Show dimensions");
                ui.checkbox(&mut self.show_relations, "Show relations");
                let wide = ui.ctx().content_rect().width() >= WIDE_LAYOUT;
                let mut show = self.show_properties.unwrap_or(wide);
                if ui.checkbox(&mut show, "Properties panel").changed() {
                    self.show_properties = Some(show);
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.show_shortcuts = true;
                }
                if ui.button("Run solver benchmark").clicked() {
                    self.benchmark = Some(run_benchmark());
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, wide: bool) {
        ui.add_space(6.0);
        for kind in ToolKind::ALL {
            let selected = self.tool.kind() == kind;
            let (label, width) = if wide {
                (format!("{}  {}", kind.label(), kind.shortcut()), 96.0)
            } else {
                (kind.label().to_owned(), 68.0)
            };
            if ui
                .add_sized([width, 26.0], egui::Button::selectable(selected, label))
                .on_hover_text(format!("{} ({})", kind.label(), kind.shortcut()))
                .clicked()
            {
                self.set_tool(kind);
            }
        }
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.heading("Properties");
        ui.separator();
        let ed = &self.editor;
        let selection: Vec<_> = ed.selection.iter().copied().collect();
        for line in &ed.selected_midpoints {
            if let Some(drift_core::Shape::Line { a, b }) = ed.index.shape(*line) {
                let m = (*a + *b) * 0.5;
                ui.strong(format!("Midpoint of {line}"));
                // `+ 0.0` turns -0.0 into 0.0 so it doesn't display as "-0.0000".
                ui.monospace(format!("{:.4}, {:.4}", m.x + 0.0, m.y + 0.0));
                ui.add_space(4.0);
            }
        }
        match selection.as_slice() {
            [] if !ed.selected_midpoints.is_empty() => {}
            [] => {
                ui.label(egui::RichText::new("Nothing selected").weak());
                ui.add_space(8.0);
                let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
                ui.label(format!("{curves} curves, {} entities", ed.doc.len()));
            }
            [id] => {
                let (Some(entity), Some(shape)) = (ed.doc.get(*id), ed.index.shape(*id)) else {
                    return;
                };
                ui.strong(format!("{} {id}", entity.kind.type_name()));
                ui.add_space(4.0);
                egui::Grid::new("props")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        property_rows(ui, shape);
                    });
            }
            many => {
                let mut counts = std::collections::BTreeMap::new();
                for id in many {
                    if let Some(e) = ed.doc.get(*id) {
                        *counts.entry(e.kind.type_name()).or_insert(0) += 1;
                    }
                }
                ui.strong(format!("{} selected", many.len()));
                for (name, n) in counts {
                    ui.label(format!("{n} × {name}"));
                }
            }
        }

        let curves: Vec<_> = selection
            .iter()
            .filter_map(|id| ed.doc.get(*id))
            .filter(|e| !e.kind.is_point())
            .collect();
        if !curves.is_empty() {
            ui.add_space(10.0);
            ui.label("Line style");
            let current = curves[0].style;
            let uniform = curves.iter().all(|e| e.style == current);
            let mut chosen = None;
            ui.horizontal(|ui| {
                for (style, label) in [
                    (LineKind::Solid, "Solid"),
                    (LineKind::Construction, "Construction"),
                    (LineKind::Center, "Center"),
                ] {
                    if ui
                        .selectable_label(uniform && current == style, label)
                        .clicked()
                    {
                        chosen = Some(style);
                    }
                }
            });
            if let Some(style) = chosen {
                self.set_selection_style(style);
            }
        }
        if !selection.is_empty() {
            ui.add_space(10.0);
            if ui.button("Delete").clicked() {
                self.editor.delete_selection();
            }
        }

        self.constraint_buttons(ui);
        self.constraint_list(ui);
    }

    /// Buttons for the relations and dimensions that fit the selection.
    fn constraint_buttons(&mut self, ui: &mut egui::Ui) {
        let relations = self.editor.relation_options();
        let dimensions = self.editor.dimension_options();
        let now = ui.input(|i| i.time);
        for (heading, options, is_dimension) in [
            ("Add relation", relations, false),
            ("Add dimension", dimensions, true),
        ] {
            if options.is_empty() {
                continue;
            }
            ui.add_space(10.0);
            ui.label(heading);
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                for option in &options {
                    if ui.button(option.label).clicked() {
                        chosen = Some(option.clone());
                    }
                }
            });
            if let Some(option) = chosen {
                match self
                    .editor
                    .add_constraints(option.label, &option.constraints)
                {
                    Ok(ids) => {
                        if is_dimension {
                            // Let the user type the value straight away.
                            self.focus_dimension = ids.first().copied();
                        }
                    }
                    Err(e) => self.set_status(format!("{}: {e}", option.label), true, now),
                }
            }
        }
    }

    /// Constraints on the selection (or all of them when nothing is selected), with editable
    /// dimension values.
    fn constraint_list(&mut self, ui: &mut egui::Ui) {
        let ed = &self.editor;
        let constraints: Vec<(ConstraintId, Constraint)> =
            if ed.selection.is_empty() && ed.selected_midpoints.is_empty() {
                ed.doc.constraints().map(|(id, c)| (id, *c)).collect()
            } else {
                // Include constraints on the selection's points: a line's length is a distance
                // between its end points.
                let mut relevant = ed.doc.defining_points(ed.selection.iter().copied());
                relevant.extend(ed.selection.iter().copied());
                relevant.extend(ed.selected_midpoints.iter().copied());
                ed.doc
                    .constraints()
                    .filter(|(_, c)| c.entities().any(|e| relevant.contains(&e)))
                    .map(|(id, c)| (id, *c))
                    .collect()
            };
        if constraints.is_empty() {
            return;
        }
        let conflicts = ed.analysis().conflicts.clone();
        ui.add_space(10.0);
        ui.label(format!("Constraints ({})", constraints.len()));
        let now = ui.input(|i| i.time);
        let mut remove = None;
        let mut select = None;
        let mut apply = None;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("constraints")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for (id, c) in &constraints {
                            let mut name = egui::RichText::new(constraint_label(&ed.doc, c));
                            if conflicts.contains(id) {
                                name = name.color(ui.visuals().error_fg_color);
                            }
                            if ui
                                .add(egui::Label::new(name).sense(egui::Sense::click()))
                                .on_hover_text(format!("{id}: click to select its geometry"))
                                .clicked()
                            {
                                select = Some(*c);
                            }
                            match c.value() {
                                Some(value) => {
                                    let field = egui::Id::new(("dimension", id.0));
                                    let mut text = self
                                        .dimension_text
                                        .get(id)
                                        .cloned()
                                        .unwrap_or_else(|| format_value(value));
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut text)
                                            .id(field)
                                            .desired_width(90.0)
                                            .hint_text(c.unit()),
                                    );
                                    if self.focus_dimension == Some(*id) {
                                        response.request_focus();
                                        self.focus_dimension = None;
                                    }
                                    if response.has_focus() {
                                        self.dimension_text.insert(*id, text.clone());
                                    }
                                    if response.lost_focus() {
                                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                            apply = Some((*id, text.clone()));
                                        }
                                        self.dimension_text.remove(id);
                                    }
                                }
                                None => {
                                    ui.label("");
                                }
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(*id);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(c) = select {
            self.editor.selection = c.entities().collect();
        }
        if let Some(id) = remove {
            self.editor.remove_constraint(id);
        }
        if let Some((id, text)) = apply {
            match parse_value(&text) {
                Some(value) => {
                    if let Err(e) = self.editor.set_dimension(id, value) {
                        self.set_status(format!("Can't use {text}: {e}"), true, now);
                    }
                }
                None => self.set_status(format!("\"{text}\" is not a number"), true, now),
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            match self.pointer {
                Some(p) => {
                    let at = p.pos();
                    ui.monospace(format!("X {:>12.4}  Y {:>12.4}", at.x, at.y));
                }
                None => {
                    ui.monospace(format!("{:31}", ""));
                }
            }
            ui.separator();
            ui.monospace(format!(
                "Grid {} mm",
                format_length(grid_info(&self.camera).minor)
            ));
            ui.toggle_value(&mut self.grid_snap, "Snap to grid");
            ui.separator();
            if !self.editor.doc.is_empty() {
                let analysis = self.editor.analysis();
                let (text, color) = if !analysis.converged {
                    (
                        format!("Over defined ({} conflicting)", analysis.conflicts.len()),
                        ui.visuals().error_fg_color,
                    )
                } else if analysis.dof == Some(0) {
                    ("Fully defined".to_owned(), ui.visuals().strong_text_color())
                } else {
                    (
                        format!("Under defined: {} DOF", analysis.dof.unwrap_or(0)),
                        if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(0x5f, 0xa8, 0xf5)
                        } else {
                            egui::Color32::from_rgb(0x15, 0x65, 0xc0)
                        },
                    )
                };
                ui.label(egui::RichText::new(text).color(color));
                ui.separator();
            }
            match &self.status {
                Some(status) if now < status.until => {
                    let text = egui::RichText::new(&status.text);
                    ui.label(if status.error {
                        text.color(ui.visuals().error_fg_color)
                    } else {
                        text
                    });
                }
                _ => {
                    ui.label(egui::RichText::new(self.tool.hint()).weak());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.backend).weak().small());
                ui.separator();
                ui.monospace(format!("{:.4} px/mm", self.camera.scale));
            });
        });
        if self.status.as_ref().is_some_and(|s| now < s.until) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    // ----- Canvas ---------------------------------------------------------------------------

    fn make_pointer(&self, map: &ScreenMap, pos: egui::Pos2, snap: bool) -> Pointer {
        let raw = map.to_world(pos);
        let mut pointer = Pointer::new(raw, PICK_RADIUS / self.camera.scale);
        pointer.relations = self.auto_relations;
        if snap && self.tool.wants_snap() {
            let options = SnapOptions {
                tolerance: pointer.tolerance,
                grid: self.grid_snap.then(|| grid_info(&self.camera).minor),
            };
            let ed = &self.editor;
            let tool = &self.tool;
            pointer.snap = ed.index.snap(raw, options, |id| tool.snap_excludes(ed, id));
            if pointer.snapped_point().is_none() {
                pointer.guide = self.inference(raw, pointer.tolerance);
            }
        }
        if snap {
            self.tool.refine_pointer(&mut pointer);
        }
        pointer
    }

    /// Align `at` horizontally / vertically with recently snapped points (inference lines).
    fn inference(&self, at: DVec2, tol: f64) -> Option<Guide> {
        let mut x_ref: Option<(f64, DVec2)> = None;
        let mut y_ref: Option<(f64, DVec2)> = None;
        for &q in &self.inference_points {
            if (at - q).length() <= tol {
                continue;
            }
            let (dx, dy) = ((at.x - q.x).abs(), (at.y - q.y).abs());
            if dx <= tol && x_ref.is_none_or(|(d, _)| dx < d) {
                x_ref = Some((dx, q));
            }
            if dy <= tol && y_ref.is_none_or(|(d, _)| dy < d) {
                y_ref = Some((dy, q));
            }
        }
        if x_ref.is_none() && y_ref.is_none() {
            return None;
        }
        Some(Guide {
            at: DVec2::new(
                x_ref.map_or(at.x, |(_, q)| q.x),
                y_ref.map_or(at.y, |(_, q)| q.y),
            ),
            kind: GuideKind::Aligned,
            from: [x_ref.map(|r| r.1), y_ref.map(|r| r.1)],
        })
    }

    fn layout_dimensions(&self, painter: &egui::Painter, map: &ScreenMap) -> Vec<DimLayout> {
        let doc = &self.editor.doc;
        doc.constraints()
            .filter(|(_, c)| c.is_dimension())
            .filter_map(|(id, c)| {
                annotations::layout_dimension(painter, doc, Some(id), c, doc.placement(id), map)
            })
            .collect()
    }

    /// Hover, pick, drag and double-click dimensions and relation glyphs. Returns true if the
    /// pointer input was used here (so the tool shouldn't see it).
    fn annotation_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        dims: &[DimLayout],
        glyphs: &[annotations::GlyphLayout],
        pos: Option<egui::Pos2>,
    ) -> bool {
        let idle = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        let dim_at = |p: egui::Pos2| dims.iter().rev().find(|d| d.text_rect.contains(p));
        let glyph_at = |p: egui::Pos2| glyphs.iter().rev().find(|g| g.rect.contains(p));
        let hit = |p: egui::Pos2| {
            dim_at(p)
                .and_then(|d| d.id)
                .or_else(|| glyph_at(p).map(|g| g.id))
        };

        self.hovered_constraint = if idle && self.label_drag.is_none() {
            pos.and_then(hit)
        } else {
            None
        };

        // Dragging a dimension label.
        if idle
            && self.label_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(dim) = dim_at(origin)
            && let Some(id) = dim.id
        {
            let label = dim.anchor
                + self.editor.doc.placement(id).unwrap_or_else(|| {
                    annotations::default_offset(
                        &self.editor.doc,
                        &dim.constraint,
                        self.camera.scale,
                    )
                });
            self.label_drag = Some((id, map.to_world(origin) - label));
            self.selected_constraint = Some(id);
            self.editor.doc.begin();
        }
        if let Some((id, grab)) = self.label_drag {
            if let (Some(p), Some(c)) = (
                response.interact_pointer_pos(),
                self.editor.doc.constraint(id).copied(),
            ) && let Some(anchor) = annotations::dimension_anchor(&self.editor.doc, &c)
            {
                let offset = map.to_world(p) - grab - anchor;
                self.editor.place_dimension(id, offset);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.label_drag = None;
                self.editor.commit("Move dimension");
            }
            return true;
        }
        if !idle {
            return false;
        }
        let at = release_pos(ui, PointerButton::Primary).or(pos);
        if response.double_clicked_by(PointerButton::Primary)
            && let Some(id) = at.and_then(|p| dim_at(p).and_then(|d| d.id))
        {
            self.open_dimension_edit(id);
            return true;
        }
        if response.clicked_by(PointerButton::Primary) {
            if let Some(id) = at.and_then(hit) {
                self.selected_constraint = Some(id);
                self.editor.selection.clear();
                return true;
            }
            self.selected_constraint = None;
        }
        false
    }

    fn open_dimension_edit(&mut self, id: ConstraintId) {
        if let Some(value) = self.editor.doc.constraint(id).and_then(|c| c.value()) {
            self.dimension_edit = Some(DimensionEdit {
                id,
                text: format_value(value),
                focus: true,
            });
        }
    }

    /// The in-place dimension value editor.
    fn dimension_editor(&mut self, ctx: &egui::Context, dims: &[DimLayout], now: f64) {
        let Some(mut edit) = self.dimension_edit.take() else {
            return;
        };
        let Some(at) = dims
            .iter()
            .find(|d| d.id == Some(edit.id))
            .map(|d| d.text_rect.center())
            .or_else(|| self.pointer.map(|_| self.canvas_rect.center()))
        else {
            return;
        };
        let field = egui::Id::new("dimension_edit_field");
        let mut keep = true;
        egui::Area::new(egui::Id::new("dimension_edit"))
            .fixed_pos(at - egui::vec2(45.0, 11.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut edit.text)
                        .id(field)
                        .desired_width(90.0)
                        .horizontal_align(egui::Align::Center),
                );
                if edit.focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, field) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(edit.text.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ctx, field);
                    }
                    edit.focus = false;
                } else if response.lost_focus() {
                    keep = false;
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        match parse_value(&edit.text) {
                            Some(value) => {
                                if let Err(e) = self.editor.set_dimension(edit.id, value) {
                                    self.set_status(
                                        format!("Can't use {}: {e}", edit.text),
                                        true,
                                        now,
                                    );
                                }
                            }
                            None => self.set_status(
                                format!("\"{}\" is not a number", edit.text),
                                true,
                                now,
                            ),
                        }
                    }
                }
            });
        if keep {
            self.dimension_edit = Some(edit);
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.canvas_rect = rect;
        let view_size = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));

        if response.double_clicked_by(PointerButton::Middle) {
            self.fit_target = Some(self.fit_bounds());
        }
        self.pan_and_zoom(ui, &response, rect);
        if let Some(bounds) = self.fit_target {
            self.camera.fit(bounds, view_size, FIT_MARGIN);
        }

        let map = ScreenMap {
            rect,
            camera: self.camera,
        };
        let (mods, alt) = ui.input(|i| {
            (
                Modifiers {
                    shift: i.modifiers.shift,
                    command: i.modifiers.command,
                },
                i.modifiers.alt,
            )
        });
        let snap = !alt; // hold Alt to place freely

        // Annotations are laid out first so they can take pointer input before the tool.
        let annotation_painter = ui.painter_at(rect);
        let dims = if self.show_dimensions {
            self.layout_dimensions(&annotation_painter, &map)
        } else {
            Vec::new()
        };
        let glyphs = if self.show_relations {
            annotations::layout_glyphs(&self.editor.doc, &map)
        } else {
            Vec::new()
        };

        // Feed pointer input to the active tool.
        let dragging = response.dragged_by(PointerButton::Primary);
        let pos = if dragging {
            response.interact_pointer_pos()
        } else {
            response.hover_pos()
        };
        let consumed = self.annotation_input(ui, &response, &map, &dims, &glyphs, pos);
        if consumed {
            self.pointer = pos.map(|pos| self.make_pointer(&map, pos, snap));
        } else if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let p = self.make_pointer(&map, origin, snap);
            self.tool
                .handle(Input::DragStart(p, mods), &mut self.editor);
        }
        if !consumed {
            self.pointer = pos.map(|pos| self.make_pointer(&map, pos, snap));
        }
        if consumed {
            // The annotation took this frame's input.
        } else if let Some(p) = self.pointer {
            if dragging {
                self.tool.handle(Input::Drag(p), &mut self.editor);
            } else {
                self.tool.handle(Input::Hover(p), &mut self.editor);
            }
            // Use the exact release position: if a slow frame batched the release with later
            // pointer motion, the current position is no longer where the user clicked.
            let released = release_pos(ui, PointerButton::Primary)
                .map_or(p, |pos| self.make_pointer(&map, pos, snap));
            if response.drag_stopped_by(PointerButton::Primary) {
                self.tool
                    .handle(Input::DragEnd(released, mods), &mut self.editor);
            }
            if response.clicked_by(PointerButton::Primary) {
                self.tool
                    .handle(Input::Click(released, mods), &mut self.editor);
            }
            if response.double_clicked_by(PointerButton::Primary) {
                self.tool.handle(Input::DoubleClick(p), &mut self.editor);
            }
        } else {
            self.editor.hovered = None;
        }
        if response.clicked_by(PointerButton::Secondary) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        // Tool events may have changed the document (and the pointer's snap targets).
        self.editor.sync();
        if let Some(id) = self.editor.pending_edit.take() {
            self.open_dimension_edit(id);
        }
        // Remember points the pointer snapped to, for inference lines.
        if let Some(snap) = self.pointer.and_then(|p| p.snap)
            && snap.kind.is_point()
            && !self.inference_points.contains(&snap.at)
        {
            self.inference_points.push(snap.at);
            if self.inference_points.len() > 4 {
                self.inference_points.remove(0);
            }
        }
        // Show what a hovered or picked relation/dimension applies to.
        if self.tool.kind() != ToolKind::Dimension {
            self.editor.highlight = self
                .hovered_constraint
                .or(self.selected_constraint)
                .and_then(|id| self.editor.doc.constraint(id))
                .map(|c| c.entities().collect())
                .unwrap_or_default();
        }
        if self.tool.kind() != ToolKind::Select && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // Geometry (GPU).
        let mut items = self.editor.items();
        items.extend(self.tool.preview(self.pointer.as_ref()));
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let callback =
            self.painter
                .callback(rect, ppp, &items, &self.camera, ui.visuals().dark_mode);
        ui.painter().add(callback);

        // Overlays (egui).
        let painter = ui.painter_at(rect);
        let colors = OverlayColors::new(ui.visuals().dark_mode);
        if self.editor.doc.is_empty() && self.tool.kind() == ToolKind::Select {
            overlay::canvas_hint(
                &painter,
                rect,
                "Choose a tool on the left, or open an example from the File menu",
                ui.visuals().weak_text_color(),
            );
        }
        if let Some(Overlay::SelectionBox { rect: r, crossing }) = self.tool.overlay() {
            overlay::selection_box(&painter, &map, &r, crossing, &colors);
        }
        self.paint_annotations(&painter, &map, &dims, &glyphs, ui.visuals().dark_mode);
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.dimension_editor(&ctx, &dims, now);
        if let (Some(p), Some(pos)) = (self.pointer, pos) {
            let mut label = Vec::new();
            if let Some(snap) = p.snap {
                overlay::snap_marker(&painter, &map, &snap, colors.snap);
                label.push(snap.kind.label().to_owned());
            }
            if let Some(m) = self.tool.measurement(&p) {
                label.push(m);
            }
            if let Some(t) = &self.typing {
                for (i, field) in t.fields.iter().enumerate() {
                    let marker = if i == t.active { ">" } else { " " };
                    label.push(format!("{marker} {field}: {}", t.values[i]));
                }
            } else if !self.tool.fields().is_empty() && self.tool.kind() != ToolKind::Select {
                label.push(format!("Type {}", self.tool.fields().join(" / ")));
            }
            overlay::cursor_label(&painter, pos, &label, &colors);
        }
    }

    fn paint_annotations(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
        dims: &[DimLayout],
        glyphs: &[annotations::GlyphLayout],
        dark: bool,
    ) {
        let colors = AnnotationColors::new(dark);
        let conflicts = &self.editor.analysis().conflicts;
        let state = |id: ConstraintId| {
            if self.selected_constraint == Some(id) || self.label_drag.is_some_and(|d| d.0 == id) {
                AnnotationState::Selected
            } else if self.hovered_constraint == Some(id) {
                AnnotationState::Hovered
            } else if conflicts.contains(&id) {
                AnnotationState::Conflict
            } else {
                AnnotationState::Normal
            }
        };
        for dim in dims {
            if let Some(id) = dim.id
                && self.dimension_edit.as_ref().is_none_or(|e| e.id != id)
            {
                annotations::paint_dimension(painter, dim, state(id), &colors);
                // Mark midpoints the dimension measures to (a line in a point slot).
                for (e, slot) in dim.constraint.slots() {
                    if slot == drift_core::Slot::PointOrMidpoint
                        && matches!(
                            self.editor.doc.get(e).map(|x| x.kind),
                            Some(drift_core::EntityKind::Line { .. })
                        )
                        && let Some(m) = self.editor.doc.ref_point(e)
                    {
                        let color = match state(id) {
                            AnnotationState::Normal => colors.normal,
                            AnnotationState::Hovered => colors.hovered,
                            AnnotationState::Selected => colors.selected,
                            AnnotationState::Conflict => colors.conflict,
                            AnnotationState::Preview => colors.preview,
                        };
                        annotations::paint_midpoint_marker(painter, map.to_screen(m), color, false);
                    }
                }
            }
        }
        if let Some((c, offset)) = self
            .tool
            .pending_dimension(&self.editor, self.pointer.as_ref())
            && let Some(dim) = annotations::layout_dimension(
                painter,
                &self.editor.doc,
                None,
                &c,
                Some(offset),
                map,
            )
        {
            annotations::paint_dimension(painter, &dim, AnnotationState::Preview, &colors);
        }
        for glyph in glyphs {
            annotations::paint_glyph(painter, glyph.rect, glyph.kind, state(glyph.id), &colors);
        }
        // Picked and hovered line midpoints.
        let ed = &self.editor;
        let midpoint_at = |line| match ed.index.shape(line) {
            Some(drift_core::Shape::Line { a, b }) => Some(map.to_screen((*a + *b) * 0.5)),
            _ => None,
        };
        for line in &ed.selected_midpoints {
            if let Some(at) = midpoint_at(*line) {
                annotations::paint_midpoint_marker(painter, at, colors.selected, true);
            }
        }
        if let Some(at) = ed.hovered_midpoint.and_then(midpoint_at) {
            annotations::paint_midpoint_marker(painter, at, colors.hovered, false);
        }
        // Inference guide lines and the automatic relation about to be added.
        if let Some(p) = self.pointer
            && let Some(guide) = p.guide
            && p.snapped_point().is_none()
        {
            let at = map.to_screen(guide.at);
            let stroke = egui::Stroke::new(1.0, colors.preview.gamma_multiply(0.8));
            for from in guide.from.iter().flatten() {
                painter.extend(egui::Shape::dashed_line(
                    &[map.to_screen(*from), at],
                    stroke,
                    4.0,
                    3.0,
                ));
            }
            let kind = match guide.kind {
                GuideKind::Horizontal => Some(GlyphKind::Horizontal),
                GuideKind::Vertical => Some(GlyphKind::Vertical),
                GuideKind::Aligned => None,
            };
            if let Some(kind) = kind.filter(|_| self.auto_relations) {
                annotations::paint_auto_relation(
                    painter,
                    at + egui::vec2(-14.0, -16.0),
                    kind,
                    &colors,
                );
            }
        }
    }

    fn pan_and_zoom(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect) {
        let to_view = |pos: egui::Pos2| {
            let v = pos - rect.center();
            DVec2::new(f64::from(v.x), f64::from(v.y))
        };
        // Pan with the middle or right button. Track positions from the press origin rather
        // than per-frame deltas, which include the pointer jump of a touch or pen press.
        let panning = response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary);
        if response.drag_started_by(PointerButton::Middle)
            || response.drag_started_by(PointerButton::Secondary)
        {
            self.pan_anchor = ui.input(|i| i.pointer.press_origin());
        }
        if panning
            && let (Some(from), Some(to)) = (self.pan_anchor, response.interact_pointer_pos())
        {
            let delta = to - from;
            if delta != egui::Vec2::ZERO {
                self.camera
                    .pan(DVec2::new(f64::from(delta.x), f64::from(delta.y)));
                self.fit_target = None;
            }
            self.pan_anchor = Some(to);
        }
        if !panning {
            self.pan_anchor = None;
        }

        if let Some(pos) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (f64::from(scroll) * SCROLL_ZOOM_SPEED).exp() * f64::from(pinch);
            if factor != 1.0 {
                self.camera.zoom_at(to_view(pos), factor);
                self.fit_target = None;
            }
        }
    }

    // ----- Dialogs --------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.pending {
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("discard")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.heading("Discard unsaved changes?");
                ui.add_space(6.0);
                ui.label(format!(
                    "{} has changes that haven't been saved.",
                    self.file.name
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard changes").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(false);
            }
            if let Some(discard) = choice {
                self.pending = None;
                if discard {
                    self.perform(action);
                }
            }
        }

        let mut open = self.benchmark.is_some();
        egui::Window::new("Solver benchmark")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(format!("Running on {}", self.backend));
                for line in self.benchmark.iter().flatten() {
                    ui.monospace(line);
                }
            });
        if !open {
            self.benchmark = None;
        }

        egui::Window::new("Keyboard shortcuts")
            .open(&mut self.show_shortcuts)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (keys, action) in SHORTCUTS {
                            ui.monospace(*keys);
                            ui.label(*action);
                            ui.end_row();
                        }
                    });
            });
    }

    /// A cheap fingerprint of the state the panels display.
    fn ui_signature(&self) -> impl PartialEq + use<> {
        let sel = &self.editor.selection;
        (
            self.tool.kind(),
            self.editor.doc.revision(),
            (sel.len(), sel.first().copied(), sel.last().copied()),
            self.editor.hovered,
            self.grid_snap,
            self.pending,
            self.editor.doc.constraint_count(),
            self.selected_constraint,
            self.hovered_constraint,
            self.editor.hovered_midpoint,
            self.editor.selected_midpoints.len(),
            self.typing.as_ref().map(|t| (t.values.clone(), t.active)),
        )
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let dirty = if self.is_dirty() { "• " } else { "" };
        let title = format!("{dirty}{} — Drift", self.file.name);
        if title != self.window_title {
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            // eframe's web backend doesn't implement the title command; set it directly.
            #[cfg(target_arch = "wasm32")]
            if let Some(document) = eframe::web_sys::window().and_then(|w| w.document()) {
                let _ = ctx;
                document.set_title(&title);
            }
            self.window_title = title;
        }
    }
}

/// Time the solver on chained-rectangle sketches (one connected component each).
pub fn run_benchmark() -> Vec<String> {
    use web_time::Instant;
    // Warm up so WASM engines have compiled the solver with their optimising tier.
    for _ in 0..3 {
        let mut doc = drift_solver::bench::chained_rectangles(50);
        drift_solver::solve(&mut doc, &drift_solver::SolveOptions::default());
        drift_solver::analyze(&doc);
    }
    [25, 100, 250]
        .into_iter()
        .map(|n| {
            let mut doc = drift_solver::bench::chained_rectangles(n);
            let start = Instant::now();
            let report = drift_solver::solve(&mut doc, &drift_solver::SolveOptions::default());
            let solve = start.elapsed();
            let start = Instant::now();
            let analysis = drift_solver::analyze(&doc);
            let analyze = start.elapsed();
            format!(
                "{:>5} entities {:>5} constraints  solve {:>7.2} ms  DOF {:>7.2} ms  {}",
                doc.len(),
                doc.constraint_count(),
                solve.as_secs_f64() * 1e3,
                analyze.as_secs_f64() * 1e3,
                if report.converged && analysis.dof == Some(2) {
                    "ok"
                } else {
                    "FAILED"
                },
            )
        })
        .collect()
}

/// A readable name for a constraint: a distance between a line's end points is its length.
fn constraint_label(doc: &Document, c: &Constraint) -> &'static str {
    if let Constraint::Distance { a, b, .. } = c {
        let is_line = doc.iter().any(|(_, e)| {
            matches!(e.kind, drift_core::EntityKind::Line { a: la, b: lb }
                if (la == *a && lb == *b) || (la == *b && lb == *a))
        });
        if is_line {
            return "Length";
        }
    }
    c.type_name()
}

/// Dimension values as shown in fields: up to 6 decimals, without trailing zeros.
fn format_value(value: f64) -> String {
    let s = format!("{value:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

/// Parse a typed dimension value, accepting a trailing unit.
fn parse_value(text: &str) -> Option<f64> {
    let t = text.trim();
    let t = t
        .strip_suffix("mm")
        .or_else(|| t.strip_suffix('°'))
        .unwrap_or(t)
        .trim();
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Where `button` was released this frame, from the raw input events.
fn release_pos(ui: &egui::Ui, button: PointerButton) -> Option<egui::Pos2> {
    ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::PointerButton {
                pos,
                button: b,
                pressed: false,
                ..
            } if *b == button => Some(*pos),
            _ => None,
        })
    })
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("S / Esc", "Select tool"),
    ("L", "Line (polyline)"),
    ("R", "Rectangle"),
    ("C", "Circle"),
    ("A", "Arc (centre, start, end)"),
    ("P", "Point"),
    ("D", "Smart Dimension"),
    (
        "Click near a line's middle",
        "Pick its midpoint (e.g. point + midpoint -> Midpoint)",
    ),
    (
        "0-9 . -  (while drawing)",
        "Type exact values; Tab switches field, Enter applies",
    ),
    ("Double-click a dimension", "Edit its value"),
    ("Drag a dimension", "Move its label"),
    ("Enter / right-click", "Finish polyline"),
    ("Esc", "Cancel operation / clear selection"),
    (
        "Del",
        "Delete selection, or the picked relation / dimension",
    ),
    ("X", "Toggle construction"),
    ("Ctrl+A", "Select all"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
    ("Ctrl+O", "Open"),
    ("F / double middle-click", "Zoom to fit"),
    ("G", "Toggle grid snapping"),
    ("Alt (hold)", "Place without snapping"),
    ("Middle / right drag", "Pan"),
    ("Scroll / pinch", "Zoom at cursor"),
];

fn property_rows(ui: &mut egui::Ui, shape: &Shape) {
    let row = |ui: &mut egui::Ui, name: &str, value: String| {
        ui.label(name);
        ui.monospace(value);
        ui.end_row();
    };
    let pt = |p: DVec2| format!("{:.4}, {:.4}", p.x + 0.0, p.y + 0.0);
    let len = |v: f64| format!("{:.4} mm", v + 0.0);
    let deg = |a: f64| format!("{:.4}°", a.to_degrees());
    match *shape {
        Shape::Point(p) => {
            row(ui, "X", len(p.x));
            row(ui, "Y", len(p.y));
        }
        Shape::Line { a, b } => {
            let d = b - a;
            row(ui, "Start", pt(a));
            row(ui, "End", pt(b));
            row(ui, "Length", len(d.length()));
            row(ui, "Angle", deg(d.y.atan2(d.x)));
        }
        Shape::Circle { center, radius } => {
            row(ui, "Center", pt(center));
            row(ui, "Radius", len(radius));
            row(ui, "Diameter", len(2.0 * radius));
        }
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            row(ui, "Center", pt(center));
            row(ui, "Radius", len(radius));
            row(ui, "Start angle", deg(start.rem_euclid(TAU)));
            row(ui, "End angle", deg((start + sweep).rem_euclid(TAU)));
            row(ui, "Sweep", deg(sweep));
            row(ui, "Arc length", len(radius * sweep));
        }
    }
}

impl eframe::App for DriftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let before = self.ui_signature();
        self.handle_file_events(now);
        if let Some(notice) = self.editor.notice.take() {
            self.set_status(notice, true, now);
        }
        self.editor.auto_relations = self.auto_relations;
        if self.pending.is_none() {
            self.handle_typing(&ctx);
            self.shortcuts(&ctx);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, now));
        let wide = ctx.content_rect().width() >= WIDE_LAYOUT;
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(if wide { 112.0 } else { 84.0 })
            .show(ui, |ui| self.toolbar(ui, wide));
        if self.show_properties.unwrap_or(wide) {
            egui::Panel::right("properties")
                .resizable(true)
                .default_size(240.0)
                .show(ui, |ui| self.properties(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.dialogs(&ctx);
        self.update_title(&ctx);
        // Panels drawn earlier in the frame may show stale state if a later panel (e.g. the
        // canvas) changed it. egui only repaints on input, so ask for one more frame.
        if self.ui_signature() != before {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let mut meta = self.file.meta.clone();
        meta.generator = Some(format!("Drift {}", env!("CARGO_PKG_VERSION")));
        storage.set_string(KEY_DOCUMENT, drift_io::save(&self.editor.doc, &meta));
        storage.set_string(KEY_NAME, self.file.name.clone());
        let path = self
            .file
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        storage.set_string(KEY_PATH, path);
        storage.set_string(KEY_DIRTY, if self.is_dirty() { "1" } else { "0" }.into());
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(10)
    }
}

/// Format a power-of-ten length compactly, e.g. `0.001`, `10`, `1e6`.
fn format_length(value: f64) -> String {
    if (1e-4..1e5).contains(&value) {
        let decimals = (-value.log10().floor()).max(0.0) as usize;
        format!("{value:.decimals$}")
    } else {
        format!("{value:e}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_length;

    #[test]
    fn parses_and_formats_dimension_values() {
        use super::{format_value, parse_value};
        assert_eq!(parse_value(" 12.5 mm"), Some(12.5));
        assert_eq!(parse_value("30°"), Some(30.0));
        assert_eq!(parse_value("abc"), None);
        assert_eq!(format_value(10.0), "10");
        assert_eq!(format_value(0.1 + 0.2), "0.3");
        assert_eq!(format_value(12.3456789), "12.345679");
    }

    #[test]
    fn benchmark_sketches_solve() {
        let lines = super::run_benchmark();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.ends_with("ok")), "{lines:#?}");
    }

    #[test]
    fn formats_grid_lengths() {
        assert_eq!(format_length(0.001), "0.001");
        assert_eq!(format_length(1.0), "1");
        assert_eq!(format_length(100.0), "100");
        assert_eq!(format_length(1e6), "1e6");
    }
}
