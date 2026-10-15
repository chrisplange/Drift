//! Drift application shell: UI chrome, canvas interaction, files and the GPU canvas callback.
//!
//! The same [`DriftApp`] runs natively and in the browser via eframe. Editing logic lives in
//! [`editor`] and [`tools`], which don't depend on egui.

mod annotations;
mod app;
pub mod blocks;
mod canvas;
pub mod checker;
pub mod clipboard;
pub mod curves;
mod dock;
pub mod editor;
pub mod fills;
pub mod fonts;
mod icons;
mod layers_ui;
pub mod library;
#[cfg(test)]
mod mem_tests;
pub mod modify;
mod notes;
pub mod output;
mod overlay;
mod parameters;
#[cfg(test)]
mod perf_tests;
mod soak;
pub mod temp_dims;
pub mod texts;
mod theme;
pub mod tools;
pub mod transform;
#[cfg(test)]
mod ui_tests;
pub mod underlays;

use std::f64::consts::TAU;

use drift_core::{
    Bounds, Constraint, ConstraintId, DVec2, Document, Kind, LineKind, Shape, Units, demo,
};
use drift_geom::SnapOptions;
use drift_io::Metadata;
use drift_platform::{FileEvent, FilePath, FileType, Files};
use drift_render::{Camera, grid_info};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers as EguiModifiers, PointerButton};

use crate::annotations::{AnnotationColors, AnnotationState, DimLayout, GlyphKind};
use crate::canvas::CanvasPainter;
use crate::editor::{Editor, Guide, GuideKind, Pointer};
use crate::overlay::{OverlayColors, ScreenMap};
use crate::tools::{Axis, Input, Modifiers, Overlay, Tool, ToolKey, ToolKind};

/// The steps angle snapping offers, in degrees.
pub(crate) const ANGLE_STEPS: [f64; 4] = [1.0, 5.0, 15.0, 45.0];

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
pub(crate) const FONT_FILE: FileType = FileType {
    name: "Font",
    extensions: &["ttf", "otf"],
};

const PAT_FILE: FileType = FileType {
    name: "Hatch patterns",
    extensions: &["pat"],
};
const UNTITLED: &str = "Untitled.drift";

// Session-restore keys in eframe's storage (a file natively, localStorage on the web).
const KEY_DOCUMENT: &str = "drift.document";
const KEY_NAME: &str = "drift.name";
const KEY_PATH: &str = "drift.path";
const KEY_DIRTY: &str = "drift.dirty";
/// View preferences (grid, axes, snapping, thin lines, colours, annotations).
const KEY_VIEW: &str = "drift.view";
/// Recently opened or saved sketches, one path per line, newest first.
const KEY_RECENT: &str = "drift.recent";
/// How many recent files are remembered.
const MAX_RECENT: usize = 10;

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
    /// Open the nth recent file.
    Recent(usize),
    /// Close the window.
    Quit,
}

/// A dimension value being edited in place on the canvas.
struct DimensionEdit {
    id: ConstraintId,
    text: String,
    /// Focus (and select all) on the next frame.
    focus: bool,
    /// With the text around the value (before, after, the chips): when a dimension is
    /// opened to edit it, not when it opens just placed (where it would cover the next pick
    /// of a chain).
    extras: bool,
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

    /// Each field's typed text evaluated with the document's variables and units: the value,
    /// and the expression text if it uses variables (so created dimensions can link to it).
    fn evaluate(&self, editor: &Editor) -> [FieldResult; 2] {
        [0, 1].map(|i| {
            let text = self.values[i].trim();
            if text.is_empty() || i >= self.fields.len() {
                return Ok(None);
            }
            editor
                .evaluate_text(text, field_kind(self.fields[i]))
                .map(|e| Some((e.value, (!e.references.is_empty()).then_some(e.text))))
                .map_err(|e| e.to_string())
        })
    }

    fn parsed(&self, editor: &Editor) -> [Option<f64>; 2] {
        self.evaluate(editor)
            .map(|r| r.ok().flatten().map(|(v, _)| v))
    }

    fn expressions(&self, editor: &Editor) -> [Option<String>; 2] {
        self.evaluate(editor)
            .map(|r| r.ok().flatten().and_then(|(_, e)| e))
    }
}

/// A typed field's value and its expression text if it uses variables; `None` if empty.
type FieldResult = Result<Option<(f64, Option<String>)>, String>;

/// The kind of value a typed field takes.
fn field_kind(label: &str) -> Kind {
    match label {
        "Angle" | "Sweep" => Kind::Angle,
        _ => Kind::Length,
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
    /// What scrolling does over the canvas, and the gesture in progress.
    wheel: app::view::Wheel,
    pointer: Option<Pointer>,
    grid_snap: bool,
    files: Files,
    file: FileState,
    pending: Option<Pending>,
    status: Option<Status>,
    show_shortcuts: bool,
    /// The computer's font families (found when the font menu is first opened), and what is
    /// typed to narrow them down.
    font_families: Option<Vec<String>>,
    font_filter: String,
    /// Where the screen's text renderer is with the drawing's font.
    screen_font: fonts::ScreenFont,
    /// The relations pie (Q), while it is open.
    pie: Option<app::pie::Pie>,
    /// Text filtering the shortcuts list.
    shortcut_filter: String,
    /// Where the side panels are (docked, floating or hidden).
    layout: dock::Layout,
    dock_view: app::dock_ui::DockView,
    /// Recently opened or saved sketches, newest first.
    recent: Vec<FilePath>,
    /// What to do once the save in progress has finished (Save in the unsaved-changes
    /// prompt), and the document revision it was asked at.
    after_save: Option<(Pending, u64)>,
    /// Closing was confirmed (or nothing was unsaved): let the window close.
    quit_confirmed: bool,
    quit_sent: bool,
    /// "Don't save" on quitting: the session isn't kept for the next start.
    discard_session: bool,
    /// The start panel over an empty sketch.
    show_start: bool,
    show_about: bool,
    show_settings: bool,
    /// What was last copied or cut here (for Edit → Paste).
    clipboard: Option<String>,
    /// The command palette, while open.
    palette: Option<app::palette::Palette>,
    recent_commands: Vec<app::palette::Command>,
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
    /// The Parameters tab's state.
    params: parameters::ParamUi,
    /// The pattern fields in the properties panel.
    patterns: app::patterns::PatternUi,
    /// Draw every curve one pixel wide (Revit's thin lines).
    thin_lines: bool,
    /// Colour geometry by constraint status (under / fully defined) rather than by layer.
    status_colors: bool,
    /// Layer names being edited in the Layers tab.
    layer_names: std::collections::HashMap<drift_core::LayerId, String>,
    /// Layers showing their colours and line settings in the Layers panel.
    expanded_layers: std::collections::HashSet<drift_core::LayerId>,
    /// A layer row being dragged to a new place in the order.
    layer_drag: Option<drift_core::LayerId>,
    /// The layer whose name is being edited.
    renaming_layer: Option<drift_core::LayerId>,
    show_grid: bool,
    show_axes: bool,
    /// Fill tessellations, kept while the document is unchanged.
    fill_meshes: fills::MeshCache,
    /// Built-in and imported hatch patterns.
    hatch_patterns: fills::PatternLibrary,
    /// An annotation label being dragged, and the grab offset (world units).
    note_drag: Option<(drift_core::AnnotationId, DVec2)>,
    note_edit: Option<app::note_ui::NoteEdit>,
    /// A picked hatch's handle being dragged, and what its origin snapped to.
    hatch_drag: Option<(drift_core::FillId, app::hatch_ui::HatchGrip)>,
    hatch_snap: Option<drift_geom::Snap>,
    /// The sheet shown in the Sheets tab and the preview.
    active_sheet: Option<drift_core::SheetId>,
    /// The canvas shows the current sheet as it prints, instead of the model.
    sheet_preview: bool,
    preview: app::sheets_ui::PreviewView,
    /// The previewed page, as of a document revision.
    preview_cache: Option<(u64, drift_core::SheetId, drift_draft::page::Drawing)>,
    view_drag: Option<app::sheets_ui::ViewDrag>,
    /// The lines a view being dragged has snapped to.
    view_guides: Vec<app::view_align::AlignGuide>,
    /// The view the picked one is aligned with (the Views section's "Align with").
    align_with: Option<usize>,
    /// The view picked on the current sheet, and a change to its views to make.
    sheet_view: usize,
    pending_view: Option<app::sheets_ui::ViewAction>,
    /// The report of the last import, until it is closed.
    import_report: Option<app::sheets_ui::ImportSummary>,
    /// Dimensions and annotations hidden on the canvas are drawn (faded) and can be picked.
    show_hidden_annotations: bool,
    /// Temporary dimensions show on a picked curve (a preference), and those showing.
    show_temp_dims: bool,
    temp_dims: app::temp_dims_ui::TempDims,
    /// Dimensions and annotations picked with Shift as well as the picked one.
    annotation_picks: Vec<drift_core::AnnotationRef>,
    /// The Check sketch window, while it is open.
    sketch_check: Option<app::checker_ui::SketchCheck>,
    /// How far apart two ends may be for the sketch checker to take them as meant to meet
    /// (mm).
    gap_tolerance: f64,
    /// The name typed for a new block.
    new_block_name: String,
    /// The Blocks tab's search, over the drawing's blocks and the library.
    block_search: String,
    /// The library category the Blocks tab shows (`None`: all of them).
    library_category: Option<library::Category>,
    /// Curvature combs on picked curves.
    show_combs: bool,
    /// Alt-scaling steps the picked geometry through round sizes (rather than any size).
    round_scale: bool,
    /// The text being typed into (its edits merge into one undo step).
    text_typing: Option<drift_core::TextId>,
    /// A text field had the keyboard at the end of the last frame.
    text_was_focused: bool,
    /// The text whose in-place field has been given the keyboard.
    text_focused: Option<drift_core::TextId>,
    /// The caret in the text being typed on the canvas: the gap it is at, and the gap the
    /// selection runs from (the same when nothing is selected).
    text_cursor: Option<(usize, usize)>,
    /// A caret to give the text's field before it next reads the keyboard.
    text_set_cursor: Option<(usize, usize)>,
    /// The pointer went down in the text being typed and hasn't come up.
    text_grip: bool,
    /// The pointer was used in the text being typed this frame.
    text_touched: bool,
    /// Shift and Ctrl (Cmd) as they are held this frame, for the pointer.
    keys_held: (bool, bool),
    /// When the caret last moved (it shows steadily for a moment, then blinks).
    text_blink: f64,
    /// Show every end point, or only those of hovered or picked curves (as in Illustrator).
    always_endpoints: bool,
    /// Lines are drawn at multiples of this many degrees (angle snapping), if on.
    angle_snap: Option<f64>,
    /// The next opened file becomes an underlay (File → Import underlay).
    import_as_underlay: bool,
    /// A multi-page PDF waiting for its page to be chosen.
    pending_pdf: Option<app::underlays_ui::PendingPdf>,
    /// Memory in use (bytes) as of the last check, when that was, and whether the user has
    /// been warned that it's high.
    memory: (u64, f64, bool),
    /// The self-driving soak test, when `DRIFT_SOAK` asks for one.
    soak: Option<soak::Soak>,
}

/// Start the memory guard: past a hard limit the app ends itself rather than exhaust the
/// machine (see `drift_platform::memory`). Call once from the native entry point.
pub fn start_memory_guard() {
    drift_platform::memory::start_guard();
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
            wheel: Default::default(),
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
            font_families: None,
            font_filter: String::new(),
            screen_font: Default::default(),
            pie: None,
            shortcut_filter: String::new(),
            layout: Default::default(),
            dock_view: Default::default(),
            recent: Vec::new(),
            after_save: None,
            quit_confirmed: false,
            quit_sent: false,
            discard_session: false,
            // Tests drive the bare canvas.
            show_start: !cfg!(test),
            show_about: false,
            show_settings: false,
            clipboard: None,
            palette: None,
            recent_commands: Vec::new(),
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
            params: Default::default(),
            patterns: Default::default(),
            thin_lines: false,
            status_colors: true,
            layer_names: Default::default(),
            expanded_layers: Default::default(),
            layer_drag: None,
            renaming_layer: None,
            show_grid: true,
            show_axes: true,
            fill_meshes: Default::default(),
            hatch_patterns: Default::default(),
            note_drag: None,
            hatch_drag: None,
            hatch_snap: None,
            note_edit: None,
            active_sheet: None,
            sheet_preview: false,
            preview: Default::default(),
            preview_cache: None,
            view_drag: None,
            view_guides: Vec::new(),
            align_with: None,
            sheet_view: 0,
            pending_view: None,
            import_report: None,
            sketch_check: None,
            show_hidden_annotations: false,
            show_temp_dims: true,
            temp_dims: Default::default(),
            annotation_picks: Vec::new(),
            gap_tolerance: crate::checker::DEFAULT_TOLERANCE,
            new_block_name: String::new(),
            block_search: String::new(),
            library_category: Some(library::Category::Doors),
            show_combs: true,
            round_scale: true,
            text_typing: None,
            text_was_focused: false,
            text_focused: None,
            text_cursor: None,
            text_set_cursor: None,
            text_grip: false,
            text_touched: false,
            keys_held: (false, false),
            text_blink: 0.0,
            always_endpoints: false,
            angle_snap: None,
            import_as_underlay: false,
            pending_pdf: None,
            memory: (0, f64::NEG_INFINITY, false),
            soak: soak::Soak::from_env(),
        };
        if let Some(mb) = drift_platform::memory::take_stop_note() {
            app.editor.notice = Some(format!(
                "Drift stopped itself last time: it was using {:.1} GB of memory. Your last autosave has been restored.",
                mb as f64 / 1024.0
            ));
        }
        theme::install_fonts(&cc.egui_ctx);
        fonts::reset_screen();
        theme::apply(&cc.egui_ctx);
        // A soak test starts from an empty sketch.
        if let Some(storage) = cc.storage
            && app.soak.is_none()
        {
            app.restore_session(storage);
        }
        app
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
/// What a set of constraints is, by kind: "3 horizontal, 2 parallel, 1 fix".
fn constraint_summary(doc: &Document, ids: &[ConstraintId]) -> String {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for c in ids.iter().filter_map(|id| doc.constraint(*id)) {
        let name = constraint_label(doc, c);
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, k)) => *k += 1,
            None => counts.push((name, 1)),
        }
    }
    counts
        .iter()
        .map(|(n, k)| format!("{k} {}", n.to_lowercase()))
        .collect::<Vec<_>>()
        .join(", ")
}

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

/// Keyboard shortcuts by section, for Help → Keyboard shortcuts.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Draw",
        &[
            ("S / Esc", "Select tool"),
            ("L", "Line (polyline)"),
            ("P", "Pen: click for corners, drag for smooth points"),
            ("R", "Rectangle"),
            ("K", "Polygon; X switches inscribed / circumscribed"),
            ("C", "Circle; press again for ellipse, then partial ellipse"),
            (
                "A",
                "Arc: centre-point; press again for 3-point, then tangent arc",
            ),
            (
                "A / L (while drawing)",
                "Switch between line and tangent arc, continuing from the last point",
            ),
            ("U", "Slot; press again for centre and arc slots"),
            ("V", "Point (a vertex)"),
            ("I", "Place block: R turns, X mirrors"),
            (
                "Shift+T",
                "Text: click where it starts and type; Enter finishes",
            ),
            ("X / Y (while drawing)", "Lock to the X or Y axis"),
            (
                "0-9 . - (  (while drawing)",
                "Type exact values; Tab switches field, Enter applies",
            ),
            (
                "=  (while drawing)",
                "Start with a variable name or expression, e.g. =width/2",
            ),
            (
                "Units in typed values",
                "25mm, 2in, 3\", 1'-6 1/2\", 30deg; bare numbers use the display unit",
            ),
            ("Enter / right-click", "Finish a polyline"),
            ("Alt (hold)", "Place without snapping"),
        ],
    ),
    (
        "Annotate",
        &[
            ("D", "Dimension: the kind follows what you click"),
            (
                "Click more points (after a horizontal or vertical dimension)",
                "Chain dimensions along the row; B switches to baseline",
            ),
            (
                "N",
                "Note: click to place, or click geometry first for a leader; Return finishes",
            ),
            ("H", "Fill / hatch: click inside a closed area"),
            ("Double-click a dimension", "Edit its value"),
            ("Drag a dimension", "Move its label"),
        ],
    ),
    (
        "Modify",
        &[
            (
                "T",
                "Trim: click a piece to remove it, or drag across pieces",
            ),
            ("E", "Extend a line or arc end to the next curve"),
            ("O", "Offset: click a curve (and its chain), then the side"),
            ("M", "Mirror the selection about a line you click"),
            ("F", "Fillet; press again for chamfer (type the size)"),
        ],
    ),
    (
        "Selection",
        &[
            (
                "Click / drag",
                "Select, or box-select; Shift adds, Ctrl toggles",
            ),
            (
                "Right-click",
                "Menu for the selection (relations, dimensions, layers) or the view",
            ),
            (
                "Tab",
                "Select the chain of curves joined end to end with the one under the pointer",
            ),
            (
                "Click inside a fill or hatch",
                "Select it (inside an empty outline too, with Preferences → Closed areas → \
                 pick only where filled turned off: area, perimeter and centroid)",
            ),
            ("Click near a line's middle", "Pick its midpoint"),
            (
                "G",
                "Move: X / Y lock an axis, type dX Tab dY, B picks the base point",
            ),
            (
                "R (with a selection)",
                "Rotate: type an angle, B picks the pivot",
            ),
            ("Shift+D", "Duplicate in place and move the copy"),
            (
                "Ctrl+C / Ctrl+X / Ctrl+V",
                "Copy, cut and paste (under the pointer); works between windows",
            ),
            ("Ctrl+Shift+V", "Paste in place"),
            (
                "Arrow keys",
                "Nudge the selection one grid step; ten with Shift",
            ),
            ("Double-click text", "Type into it in place"),
            (
                "Alt+C",
                "Toggle construction (with a drawing tool: draw construction geometry)",
            ),
            (
                "Ctrl+G / Ctrl+Shift+G",
                "Group / ungroup; double-click a group to enter it",
            ),
            ("Ctrl+A", "Select all"),
            (
                "Q",
                "Relations pie for the selection: hold and flick, or tap and click",
            ),
            (
                "Del",
                "Delete the selection, or the picked relation / dimension",
            ),
            (
                "Esc / right-click",
                "Cancel the operation, or clear the selection",
            ),
        ],
    ),
    (
        "View",
        &[
            ("Shift+F / double middle-click", "Zoom to fit"),
            ("Shift+Z", "Zoom to the selection"),
            ("Middle / right drag", "Pan"),
            (
                "Scroll / pinch",
                "Zoom at the cursor (two fingers on a trackpad pan)",
            ),
            ("Alt+D / Alt+R", "Show or hide dimensions / relations"),
            (
                "Alt+H",
                "Show hidden dimensions and notes, faded, to pick and show them again",
            ),
            ("Shift+G", "Snap to grid"),
            (
                "Shift+C",
                "Colour curves by constraint status, or by their own and their layer's colours",
            ),
            ("Ctrl+'", "Show or hide the grid"),
            ("W", "Thin lines: every line one pixel wide"),
        ],
    ),
    (
        "Panels and files",
        &[
            (
                "Ctrl+K / Ctrl+Shift+P",
                "Command palette: any tool, setting, panel or command",
            ),
            ("Ctrl+L", "Show or hide the Layers panel"),
            ("Ctrl+\\", "Hide all panels, and bring them back"),
            (
                "Drag a panel's tab",
                "Onto the canvas to float it, or onto either side to dock it",
            ),
            ("Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z", "Undo / redo"),
            ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
            ("Ctrl+N / Ctrl+O", "New / open"),
            ("Ctrl+,", "Preferences"),
            ("Ctrl+P", "Print (the sheets as a PDF)"),
            ("?", "This list"),
        ],
    ),
];

/// Read-only facts about a shape, beyond the values the inspector offers as fields
/// (lengths, radii, positions and a line's angle).
fn property_rows(ui: &mut egui::Ui, shape: &Shape, units: Units) {
    let row = |ui: &mut egui::Ui, name: &str, value: String| {
        theme::row(ui, name, |ui| {
            ui.monospace(value);
        });
    };
    let len = |v: f64| units.format_readout(v);
    let deg = |a: f64| format!("{:.4}°", a.to_degrees());
    match *shape {
        Shape::Point(_) | Shape::Line { .. } | Shape::Circle { .. } => {}
        Shape::Arc {
            radius,
            start,
            sweep,
            ..
        } => {
            row(ui, "Start angle", deg(start.rem_euclid(TAU)));
            row(ui, "End angle", deg((start + sweep).rem_euclid(TAU)));
            row(ui, "Sweep", deg(sweep));
            row(ui, "Arc length", len(radius * sweep));
        }
        Shape::Ellipse {
            major,
            minor,
            sweep,
            ..
        } => {
            row(
                ui,
                "Semi-axes",
                format!("{} × {}", len(major.length()), len(minor)),
            );
            row(ui, "Rotation", deg(major.y.atan2(major.x).rem_euclid(TAU)));
            if sweep < TAU {
                row(ui, "Sweep", deg(sweep));
            }
        }
        Shape::Bezier { p } => {
            let length: f64 = drift_core::shape::flatten_bezier(&p, 128)
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum();
            row(ui, "Length", len(length));
        }
    }
}

impl eframe::App for DriftApp {
    /// Runs every frame, including while the window is covered or minimised (when `ui`
    /// doesn't): a close request must be answered here or it goes through unasked.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_close(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let before = self.ui_signature();
        self.handle_file_events(now);
        fonts::sync_screen(&ctx, &self.editor, &mut self.screen_font);
        if let Some(notice) = self.editor.notice.take() {
            self.set_status(notice, true, now);
        }
        self.check_memory(now);
        // A sketch dropped on the window opens.
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()));
        if let Some(path) = dropped
            && cfg!(not(target_arch = "wasm32"))
        {
            self.open_file(path);
        }
        self.editor.auto_relations = self.auto_relations;
        self.editor.always_endpoints = self.always_endpoints;
        // Text placed but left empty goes once it's no longer picked.
        let keep = self.editor.selected_text;
        self.editor.drop_empty_texts(keep);
        if self.pending.is_none() && self.editor.pending_driven.is_none() {
            if self.palette.is_none() {
                self.handle_typing(&ctx);
                self.shortcuts(&ctx);
            }
            // Before the panels, so it takes the arrow keys, Return and Esc first.
            self.palette_ui(&ctx);
        }

        let style = ui.style().clone();
        let frame = |x: i8, y: i8| {
            egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(x, y))
        };
        egui::Panel::top("menu")
            .frame(frame(8, 3))
            .show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("context")
            .frame(frame(10, 3))
            .show(ui, |ui| self.context_bar(ui));
        egui::Panel::bottom("status")
            .frame(frame(12, 4))
            .show(ui, |ui| self.status_bar(ui, now));
        let wide = ctx.content_rect().width() >= WIDE_LAYOUT;
        egui::Panel::left("tools")
            .resizable(false)
            .frame(frame(8, 6))
            .exact_size(if wide { 168.0 } else { 56.0 })
            .show(ui, |ui| self.toolbar(ui, wide));
        self.begin_docks(wide);
        self.dock(ui, dock::Side::Left);
        self.dock(ui, dock::Side::Right);
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.sheet_preview {
                self.sheet_canvas(ui);
            } else {
                self.canvas(ui);
            }
        });
        self.floating_panels(&ctx);
        self.dock_drag(&ctx);

        self.dialogs(&ctx);
        self.update_title(&ctx);
        self.text_was_focused = ctx.text_edit_focused();
        // Panels drawn earlier in the frame may show stale state if a later panel (e.g. the
        // canvas) changed it. egui only repaints on input, so ask for one more frame.
        if self.ui_signature() != before {
            ctx.request_repaint();
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.soak_input(ctx, raw_input);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(KEY_VIEW, self.view_prefs());
        let recent: Vec<String> = self
            .recent
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        storage.set_string(KEY_RECENT, recent.join("\n"));
        if self.discard_session {
            // "Don't save" on quitting: start fresh next time.
            storage.set_string(KEY_DOCUMENT, String::new());
            return;
        }
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
