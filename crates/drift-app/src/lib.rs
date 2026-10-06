//! Drift application shell: UI chrome, canvas interaction, files and the GPU canvas callback.
//!
//! The same [`DriftApp`] runs natively and in the browser via eframe. Editing logic lives in
//! [`editor`] and [`tools`], which don't depend on egui.

mod canvas;
pub mod editor;
mod overlay;
pub mod tools;
#[cfg(test)]
mod ui_tests;

use std::f64::consts::TAU;

use drift_core::{Bounds, DVec2, Document, LineKind, Shape, demo};
use drift_geom::SnapOptions;
use drift_io::Metadata;
use drift_platform::{FileEvent, FilePath, FileType, Files};
use drift_render::{Camera, grid_info};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers as EguiModifiers, PointerButton};

use crate::canvas::CanvasPainter;
use crate::editor::{Editor, Pointer};
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
            editor: Editor::default(),
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

        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if consume(cmd(Key::A)) {
            self.set_tool(ToolKind::Select);
            self.editor.select_all();
        }
        let plain = ctx.input(|i| i.modifiers.is_none());
        let pressed = |key| plain && ctx.input(|i| i.key_pressed(key));

        if pressed(Key::Escape) && !self.tool.cancel(&mut self.editor) {
            self.set_tool(ToolKind::Select);
        }
        if pressed(Key::Enter) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        if (pressed(Key::Delete) || pressed(Key::Backspace)) && !self.tool.is_busy() {
            self.editor.delete_selection();
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
        match selection.as_slice() {
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
        if snap && self.tool.wants_snap() {
            let options = SnapOptions {
                tolerance: pointer.tolerance,
                grid: self.grid_snap.then(|| grid_info(&self.camera).minor),
            };
            let ed = &self.editor;
            let tool = &self.tool;
            pointer.snap = ed.index.snap(raw, options, |id| tool.snap_excludes(ed, id));
        }
        pointer
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

        // Feed pointer input to the active tool.
        let dragging = response.dragged_by(PointerButton::Primary);
        let pos = if dragging {
            response.interact_pointer_pos()
        } else {
            response.hover_pos()
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let p = self.make_pointer(&map, origin, snap);
            self.tool
                .handle(Input::DragStart(p, mods), &mut self.editor);
        }
        self.pointer = pos.map(|pos| self.make_pointer(&map, pos, snap));
        if let Some(p) = self.pointer {
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
        if let (Some(p), Some(pos)) = (self.pointer, pos) {
            let mut label = Vec::new();
            if let Some(snap) = p.snap {
                overlay::snap_marker(&painter, &map, &snap, colors.snap);
                label.push(snap.kind.label().to_owned());
            }
            if let Some(m) = self.tool.measurement(&p) {
                label.push(m);
            }
            overlay::cursor_label(&painter, pos, &label, &colors);
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
    ("Enter / right-click", "Finish polyline"),
    ("Esc", "Cancel operation / clear selection"),
    ("Del", "Delete selection"),
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
    let pt = |p: DVec2| format!("{:.4}, {:.4}", p.x, p.y);
    let len = |v: f64| format!("{v:.4} mm");
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
        if self.pending.is_none() {
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
    fn formats_grid_lengths() {
        assert_eq!(format_length(0.001), "0.001");
        assert_eq!(format_length(1.0), "1");
        assert_eq!(format_length(100.0), "100");
        assert_eq!(format_length(1e6), "1e6");
    }
}
