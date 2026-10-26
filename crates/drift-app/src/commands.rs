//! The command registry: every action Drift offers, in one table.
//!
//! Each command has a stable id, a title and a line of description, palette aliases (AutoCAD's
//! MOVE / M, ROTATE / RO… as well as Drift's own words), a category, its keys, an icon and
//! when it can run. The command palette, the menus (main and right-click), the shortcuts
//! window, the shortcut dispatch and `docs/guide/reference/keyboard-shortcuts.md` are all made
//! from it, so a new tool is added here once and re-binding a key is a one-line change in
//! [`table`].
//!
//! What runs is [`Command`], dispatched by `DriftApp::run_command` (`app/palette.rs`).
//!
//! **The boundary.** The registry dispatches plain commands. Keys that mean something only
//! inside a tool stay with the tools and are listed in [`OTHER_KEYS`] for the shortcuts list:
//! X / Y axis locks, B base point, Tab between typed fields, typed values, G and R while
//! something is picked (a grab or a turn: [`Spec::modal`] shows them), Q's pie (hold and
//! flick), Esc, Enter and the arrows. Ctrl+C / X / V arrive as the system's clipboard events,
//! so they are shown here but handled in `app/edit.rs`.

use std::sync::LazyLock;

use eframe::egui::Key;

use crate::DriftApp;
use crate::dock::Panel;
use crate::editor::{PickFilter, PickKind};
use crate::icons::Glyph;
use crate::stack::Arrange;
use crate::tools::ToolKind;

/// Something Drift can do. The registry describes each; `DriftApp::run_command` does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Tool(ToolKind),
    Arrange(Arrange),
    Toggle(ViewFlag),
    Panel(Panel),
    HideAllPanels,
    ResetPanels,
    New,
    Open,
    OpenExample,
    ShowStart,
    Save,
    SaveAs,
    Import,
    ExportPdf,
    ExportSvg,
    ExportDxf,
    ExportProfiles,
    ExportModelSvg,
    ExportModelPdf,
    Print,
    Undo,
    Redo,
    SelectAll,
    SelectOverlapping,
    CheckSketch,
    HideAnnotations,
    AnnotationsOffSheets,
    AlignDimensions,
    SpaceDimensions,
    ClearRelations,
    ClearDimensions,
    PickFilter(PickFilter),
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    Delete,
    Duplicate,
    Group,
    Rename,
    Ungroup,
    MakeBlock,
    Explode,
    BlockFromGroup,
    ExplodeToGroup,
    /// Walls along the picked curves.
    WallsFromCurves,
    /// Walls found among the picked lines (pairs of faces).
    WallsFromLines,
    /// The picked walls as plain geometry.
    ExplodeWalls,
    /// Two picked walls' nearest ends joined (an L or a T).
    JoinWalls,
    /// The picked walls' ends set not to join.
    UnjoinWalls,
    /// Grab the selection (G).
    Move,
    /// Turn the selection (R with a selection).
    Rotate,
    /// Scale with the selection's handles.
    Scale,
    /// The Inspector's pattern of the selection: linear, or (true) circular.
    Pattern(bool),
    /// Save the drawing as a new option: the whole sketch, or the values only.
    SaveOption(bool),
    ApplyOption(drift_core::VariantId),
    /// Show or hide an option's ghost.
    GhostOption(drift_core::VariantId),
    Construction,
    ZoomToFit,
    ZoomToSelection,
    ZoomLifeSize,
    SheetPreview,
    Palette,
    Shortcuts,
    Preferences,
    DrawingSettings,
    About,
    UserGuide,
    ReportProblem,
    Benchmark,
    Theme(eframe::egui::ThemePreference),
}

/// On/off view settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewFlag {
    Grid,
    Axes,
    Snap,
    PerpTangent,
    Thin,
    Status,
    Endpoints,
    Dimensions,
    Relations,
    Hidden,
    Free,
    Combs,
    Crops,
    AutoRelations,
    /// Walls' centrelines shown.
    Centrelines,
}

/// Where a command belongs: its menu, and its section in the palette and the shortcuts list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    File,
    Edit,
    View,
    Draw,
    Modify,
    Annotate,
    Arrange,
    Layers,
    Help,
    Labs,
}

impl Category {
    pub(crate) const ALL: [Category; 10] = [
        Category::File,
        Category::Edit,
        Category::View,
        Category::Draw,
        Category::Modify,
        Category::Annotate,
        Category::Arrange,
        Category::Layers,
        Category::Help,
        Category::Labs,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::View => "View",
            Category::Draw => "Draw",
            Category::Modify => "Modify",
            Category::Annotate => "Annotate",
            Category::Arrange => "Arrange",
            Category::Layers => "Layers",
            Category::Help => "Help",
            Category::Labs => "Labs",
        }
    }
}

/// A key with modifiers. `command` is Ctrl (⌘ on macOS); `typed` matches the character the
/// key types (? is Shift+/ on most layouts but not all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shortcut {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub typed: bool,
    pub key: Key,
}

const fn keys(command: bool, shift: bool, alt: bool, key: Key) -> Shortcut {
    Shortcut {
        command,
        shift,
        alt,
        typed: false,
        key,
    }
}
const fn plain(key: Key) -> Shortcut {
    keys(false, false, false, key)
}
const fn shift(key: Key) -> Shortcut {
    keys(false, true, false, key)
}
const fn alt(key: Key) -> Shortcut {
    keys(false, false, true, key)
}
const fn shift_alt(key: Key) -> Shortcut {
    keys(false, true, true, key)
}
const fn cmd(key: Key) -> Shortcut {
    keys(true, false, false, key)
}
const fn cmd_shift(key: Key) -> Shortcut {
    keys(true, true, false, key)
}
const fn typed(key: Key) -> Shortcut {
    Shortcut {
        typed: true,
        ..plain(key)
    }
}

impl Shortcut {
    /// How it reads: "Ctrl+Shift+Z" (⇧⌘Z on a Mac), "Shift+G", "Alt+D", "W".
    pub(crate) fn text(self, mac: bool) -> String {
        let name = key_name(self.key, mac);
        if self.typed {
            return name.to_owned();
        }
        if self.command {
            return cmd_text(name, self.shift, mac);
        }
        let mut out = String::new();
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(name);
        out
    }

    /// How many modifiers it takes (the more specific shortcuts are tried first, as egui's
    /// matching ignores extra Shift and Alt).
    fn weight(self) -> u8 {
        self.command as u8 + self.shift as u8 + self.alt as u8
    }

    /// A single letter or key with no modifiers (as typed into the palette).
    pub(crate) fn single(self) -> Option<char> {
        let name = self.key.name();
        (!self.command && !self.shift && !self.alt && !self.typed && name.chars().count() == 1)
            .then(|| name.chars().next())
            .flatten()
    }
}

/// A Ctrl shortcut as the platform writes it: ⌘ / ⇧ on macOS, Ctrl / Shift elsewhere.
pub(crate) fn cmd_text(key: &str, shift: bool, mac: bool) -> String {
    if mac {
        format!("{}⌘{key}", if shift { "⇧" } else { "" })
    } else {
        format!("Ctrl+{}{key}", if shift { "Shift+" } else { "" })
    }
}

fn key_name(key: Key, mac: bool) -> &'static str {
    match key {
        Key::Quote => "'",
        Key::Backslash => "\\",
        Key::Comma => ",",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Questionmark => "?",
        Key::Delete if mac => "⌫",
        Key::Delete => "Del",
        _ => key.name(),
    }
}

/// When a command can run (menus grey it out, the palette dims it and says why).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum When {
    Always,
    /// Geometry, block copies or annotations picked.
    Picked,
    /// Geometry or block copies picked.
    Selection,
    /// Geometry picked.
    Curves,
    /// Geometry picked, or a drawing tool in hand (it then draws construction geometry).
    ConstructionTarget,
    TwoCurves,
    Grouped,
    ConvertibleGroup,
    Copies,
    Clipboard,
    CanUndo,
    CanRedo,
    Arrangeable,
    RelationsOnSelection,
    DimensionsOnSelection,
    AlignableDimensions,
    SpaceableDimensions,
    PickedAnnotations,
    /// Walls picked.
    Walls,
    /// Only in development builds (and hidden elsewhere).
    Debug,
}

impl When {
    pub(crate) fn holds(self, app: &mut DriftApp) -> bool {
        let ed = &app.editor;
        let curves = !ed.selection.is_empty();
        let selection = curves || !ed.selected_instances.is_empty();
        match self {
            When::Always => true,
            When::Picked => selection || ed.picks_annotations(),
            When::Selection => selection,
            When::Curves => curves,
            When::ConstructionTarget => curves || app.tool.kind().draws_curves(),
            When::TwoCurves => ed.selection.len() >= 2,
            When::Grouped => ed.selection.iter().any(|id| ed.doc.group_of(*id).is_some()),
            When::ConvertibleGroup => ed.convertible_group().is_some(),
            When::Copies => !ed.selected_instances.is_empty(),
            When::Clipboard => app.clipboard.is_some(),
            When::CanUndo => ed.history.can_undo(),
            When::CanRedo => ed.history.can_redo(),
            When::Arrangeable => app.editor.can_arrange(),
            When::RelationsOnSelection => !ed.selection_constraints().0.is_empty(),
            When::DimensionsOnSelection => !ed.selection_constraints().1.is_empty(),
            When::AlignableDimensions => app.can_align_dimensions().0,
            When::SpaceableDimensions => app.can_align_dimensions().1,
            When::PickedAnnotations => !app.picked_annotations().is_empty(),
            When::Walls => !app.editor.selected_walls().is_empty(),
            When::Debug => cfg!(debug_assertions),
        }
    }

    /// Why it can't run, for the palette and repeat-last.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            When::Always | When::Debug => "",
            When::Picked | When::Selection => "Pick something first",
            When::Curves => "Pick some curves first",
            When::ConstructionTarget => "Pick some curves, or take a drawing tool",
            When::TwoCurves => "Pick two or more curves",
            When::Grouped => "Pick something in a group",
            When::ConvertibleGroup => "Pick a group",
            When::Copies => "Pick a block copy",
            When::Clipboard => "Nothing copied yet",
            When::CanUndo => "Nothing to undo",
            When::CanRedo => "Nothing to redo",
            When::Arrangeable => "Pick a fill or a shape",
            When::RelationsOnSelection => "The selection has no relations",
            When::DimensionsOnSelection => "The selection has no dimensions",
            When::AlignableDimensions => "Pick two or more parallel dimensions",
            When::SpaceableDimensions => "Pick three or more stacked parallel dimensions",
            When::PickedAnnotations => "Pick some dimensions or notes",
            When::Walls => "Pick a wall",
        }
    }

    /// Not offered at all in this build.
    pub(crate) fn hidden(self) -> bool {
        self == When::Debug && !cfg!(debug_assertions)
    }
}

/// How a shortcut is guarded when pressed on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Guard {
    Any,
    /// Not while a tool is in the middle of something.
    Idle,
    /// Idle, and no widget has the keyboard.
    IdleUnfocused,
}

/// A small picture for the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Tool(ToolKind),
    Glyph(Glyph),
}

/// One command in the registry.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    /// Stable: for links, tests and (later) user key bindings.
    #[cfg_attr(not(test), allow(dead_code))]
    pub id: &'static str,
    pub command: Command,
    pub category: Category,
    pub title: &'static str,
    /// The menu's shorter label, when it differs ("PDF…" in File → Export).
    pub menu: &'static str,
    pub description: &'static str,
    /// Exact names the palette ranks first: AutoCAD's command names and aliases.
    pub aliases: &'static [&'static str],
    /// More words the palette searches.
    pub words: &'static str,
    /// The keys, the first one shown in menus.
    pub keys: &'static [Shortcut],
    /// Whether the registry dispatches the keys (Ctrl+C / X / V come as clipboard events).
    pub dispatched: bool,
    /// Pressed on the canvas: the Ctrl shortcuts act before a text field is checked for.
    pub global: bool,
    /// Pressed on the canvas: acts during a grab or a turn too.
    pub in_grab: bool,
    /// Once per press, not per key repeat.
    pub once: bool,
    pub guard: Guard,
    /// Keys a tool handles itself, shown with the command ("G", "R with a selection").
    pub modal: &'static str,
    pub icon: Option<Icon>,
    pub when: When,
    /// Enter or Space with nothing in hand does it again.
    pub repeatable: bool,
}

impl Spec {
    fn new(
        id: &'static str,
        category: Category,
        command: Command,
        title: &'static str,
        description: &'static str,
    ) -> Spec {
        let icon = match command {
            Command::Tool(kind) => Some(Icon::Tool(kind)),
            Command::Panel(p) => Some(Icon::Glyph(crate::app::dock_ui::glyph(p))),
            Command::Delete => Some(Icon::Glyph(Glyph::Delete)),
            _ => None,
        };
        Spec {
            id,
            command,
            category,
            title,
            menu: title,
            description,
            aliases: &[],
            words: "",
            keys: &[],
            dispatched: true,
            global: false,
            in_grab: false,
            once: false,
            guard: Guard::Any,
            modal: "",
            icon,
            when: When::Always,
            repeatable: matches!(
                category,
                Category::Draw | Category::Modify | Category::Annotate | Category::Arrange
            ),
        }
    }

    fn keys(mut self, keys: &'static [Shortcut]) -> Self {
        self.keys = keys;
        self
    }
    fn global(mut self) -> Self {
        self.global = true;
        self
    }
    fn in_grab(mut self) -> Self {
        self.in_grab = true;
        self
    }
    fn once(mut self) -> Self {
        self.once = true;
        self
    }
    fn guard(mut self, guard: Guard) -> Self {
        self.guard = guard;
        self
    }
    /// Shown, but the system's clipboard events do it.
    fn clipboard(mut self) -> Self {
        self.dispatched = false;
        self
    }
    fn modal(mut self, keys: &'static str) -> Self {
        self.modal = keys;
        self
    }
    fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }
    fn words(mut self, words: &'static str) -> Self {
        self.words = words;
        self
    }
    fn menu(mut self, label: &'static str) -> Self {
        self.menu = label;
        self
    }
    fn when(mut self, when: When) -> Self {
        self.when = when;
        self
    }
    fn repeat(mut self, on: bool) -> Self {
        self.repeatable = on;
        self
    }

    /// The keys as a menu shows them: the first shortcut, or a tool's modal key. A tool in
    /// a cycle (C: circle, ellipse, partial ellipse) shows the cycle's key.
    pub(crate) fn key_text(&self, mac: bool) -> String {
        if let Some(k) = self.keys.first() {
            return k.text(mac);
        }
        if !self.modal.is_empty() {
            return self.modal.split(' ').next().unwrap_or_default().to_owned();
        }
        match self.command {
            Command::Tool(kind) if kind.cycle_head() != kind => {
                spec(Command::Tool(kind.cycle_head())).map_or_else(String::new, |s| s.key_text(mac))
            }
            _ => String::new(),
        }
    }

    /// Every key, for the shortcuts list: "Ctrl+Shift+Z / Ctrl+Y", or the modal keys.
    pub(crate) fn all_keys(&self, mac: bool) -> String {
        if self.keys.is_empty() {
            return self.modal.to_owned();
        }
        let keys: Vec<String> = self.keys.iter().map(|k| k.text(mac)).collect();
        keys.join(" / ")
    }

    /// What the shortcuts list says it does: the description, and a cycle's other tools.
    pub(crate) fn does(&self) -> String {
        let mut out = self.description.to_owned();
        if let Command::Tool(kind) = self.command
            && let Some(again) = kind.cycle_hint()
            && !self.description.contains(" again")
        {
            out.push_str(". ");
            out.push_str(&again);
        }
        out
    }
}

/// The registry, in menu order within each category.
pub(crate) fn registry() -> &'static [Spec] {
    static REGISTRY: LazyLock<Vec<Spec>> = LazyLock::new(table);
    &REGISTRY
}

/// The registry's entry for a command (`None` for an option's own entries, which come from
/// the drawing).
pub(crate) fn spec(command: Command) -> Option<&'static Spec> {
    registry().iter().find(|s| s.command == command)
}

/// The key that picks a tool, for its tooltip and the toolbar (`None` for a tool in a cycle
/// other than the first, and a tool with no key).
pub(crate) fn tool_key(kind: ToolKind) -> Option<Shortcut> {
    spec(Command::Tool(kind))?.keys.first().copied()
}

/// The registry's table. Re-binding a key is changing its `.keys(...)` here; the tests catch
/// two commands on one key.
fn table() -> Vec<Spec> {
    use Category as C;
    use Command as K;
    let tool = |id, category, kind: ToolKind| {
        Spec::new(
            id,
            category,
            Command::Tool(kind),
            kind.label(),
            kind.description(),
        )
    };
    vec![
        // File.
        Spec::new(
            "file.new",
            C::File,
            K::New,
            "New sketch",
            "Start a new, empty drawing",
        )
        .menu("New")
        .keys(const { &[cmd(Key::N)] })
        .global()
        .aliases(&["NEW", "QNEW"]),
        Spec::new(
            "file.open",
            C::File,
            K::Open,
            "Open…",
            "Open a Drift sketch, or a drawing to import",
        )
        .keys(const { &[cmd(Key::O)] })
        .global()
        .aliases(&["OPEN"]),
        Spec::new(
            "file.open_example",
            C::File,
            K::OpenExample,
            "Open example",
            "A sample drawing to look around in",
        ),
        Spec::new(
            "file.start_panel",
            C::File,
            K::ShowStart,
            "Show the start panel",
            "New, recent and example drawings, over an empty sketch",
        ),
        Spec::new("file.save", C::File, K::Save, "Save", "Save the drawing")
            .keys(const { &[cmd(Key::S)] })
            .global()
            .aliases(&["SAVE", "QSAVE"]),
        Spec::new(
            "file.save_as",
            C::File,
            K::SaveAs,
            "Save as…",
            "Save the drawing under a new name",
        )
        .keys(const { &[cmd_shift(Key::S)] })
        .global()
        .aliases(&["SAVEAS"]),
        Spec::new(
            "file.drawing_settings",
            C::File,
            K::DrawingSettings,
            "Drawing settings…",
            "Units, how areas and fills behave, and drafting style: saved with this drawing",
        )
        .aliases(&["UNITS", "UN"]),
        Spec::new(
            "file.import",
            C::File,
            K::Import,
            "Import…",
            "Bring in a DWG, DXF, SVG, PDF or Illustrator drawing: as curves, fills and text to \
             edit, or as an underlay to trace over",
        )
        .aliases(&["IMPORT", "XREF", "ATTACH"])
        .words("underlay trace dwg dxf svg pdf illustrator ai drawing"),
        Spec::new(
            "file.export_pdf",
            C::File,
            K::ExportPdf,
            "Export PDF…",
            "Every sheet at true scale (or the drawing fitted to A3)",
        )
        .menu("PDF…")
        .aliases(&["EXPORTPDF", "PLOT"]),
        Spec::new(
            "file.export_svg",
            C::File,
            K::ExportSvg,
            "Export SVG…",
            "The current sheet",
        )
        .menu("SVG…"),
        Spec::new(
            "file.export_dxf",
            C::File,
            K::ExportDxf,
            "Export DXF…",
            "The model at full size, with layers and line types",
        )
        .menu("DXF…")
        .aliases(&["DXFOUT", "EXPORT"]),
        Spec::new(
            "file.export_model_svg",
            C::File,
            K::ExportModelSvg,
            "Export model as SVG…",
            "The selection (or everything that prints) at true size on a page fitted to it, a \
             group per layer",
        )
        .menu("Model as SVG…"),
        Spec::new(
            "file.export_model_pdf",
            C::File,
            K::ExportModelPdf,
            "Export model as PDF…",
            "The selection (or everything that prints) at true size on a page fitted to it",
        )
        .menu("Model as PDF…"),
        Spec::new(
            "file.export_profiles",
            C::File,
            K::ExportProfiles,
            "Export Blender profiles…",
            "The blocks marked as Blender profiles, for the Drift Profiles add-on (saving \
             writes them beside the drawing too)",
        )
        .menu("Blender profiles…"),
        Spec::new(
            "file.print",
            C::File,
            K::Print,
            "Print…",
            "Print the sheets (as a PDF)",
        )
        .keys(const { &[cmd(Key::P)] })
        .global()
        .once()
        .aliases(&["PRINT", "PLOT"]),
        // Edit.
        Spec::new(
            "edit.undo",
            C::Edit,
            K::Undo,
            "Undo",
            "Undo the last change",
        )
        .keys(const { &[cmd(Key::Z)] })
        .global()
        .when(When::CanUndo)
        .aliases(&["UNDO", "U"]),
        Spec::new(
            "edit.redo",
            C::Edit,
            K::Redo,
            "Redo",
            "Redo what was undone",
        )
        .keys(const { &[cmd_shift(Key::Z), cmd(Key::Y)] })
        .global()
        .when(When::CanRedo)
        .aliases(&["REDO"]),
        Spec::new(
            "edit.cut",
            C::Edit,
            K::Cut,
            "Cut",
            "Copy the selection and delete it",
        )
        .keys(const { &[cmd(Key::X)] })
        .clipboard()
        .when(When::Picked)
        .aliases(&["CUTCLIP"]),
        Spec::new(
            "edit.copy",
            C::Edit,
            K::Copy,
            "Copy",
            "Copy the selection; works between windows",
        )
        .keys(const { &[cmd(Key::C)] })
        .clipboard()
        .when(When::Picked)
        .aliases(&["COPYCLIP"]),
        Spec::new(
            "edit.paste",
            C::Edit,
            K::Paste,
            "Paste",
            "Under the pointer, or in the middle of the view",
        )
        .keys(const { &[cmd(Key::V)] })
        .clipboard()
        .when(When::Clipboard)
        .aliases(&["PASTECLIP"]),
        Spec::new(
            "edit.paste_in_place",
            C::Edit,
            K::PasteInPlace,
            "Paste in place",
            "Where it was copied from",
        )
        .keys(const { &[cmd_shift(Key::V)] })
        .clipboard()
        .when(When::Clipboard)
        .aliases(&["PASTEORIG"]),
        Spec::new(
            "edit.check_sketch",
            C::Edit,
            K::CheckSketch,
            "Check sketch (gaps, short ends)…",
            "Find ends that nearly meet but don't, ends short of a curve, and curves of no \
             length, and fix them",
        )
        .menu("Check sketch…")
        .aliases(&["AUDIT", "OVERKILL"]),
        Spec::new(
            "edit.delete",
            C::Edit,
            K::Delete,
            "Delete selection",
            "Delete the selection, or the picked relation or dimension",
        )
        .menu("Delete")
        .keys(const { &[plain(Key::Delete), plain(Key::Backspace)] })
        .guard(Guard::Idle)
        .when(When::Picked)
        .aliases(&["ERASE", "E"]),
        Spec::new(
            "edit.construction",
            C::Edit,
            K::Construction,
            "Toggle construction",
            "Make the picked curves construction geometry, or geometry again (with a drawing \
             tool: draw construction geometry)",
        )
        .keys(const { &[alt(Key::C)] })
        .when(When::ConstructionTarget),
        Spec::new(
            "edit.group",
            C::Edit,
            K::Group,
            "Group",
            "Group the picked curves; double-click a group to enter it",
        )
        .keys(const { &[cmd(Key::G)] })
        .global()
        .when(When::TwoCurves)
        .aliases(&["GROUP", "G"]),
        Spec::new(
            "edit.ungroup",
            C::Edit,
            K::Ungroup,
            "Ungroup",
            "Ungroup the picked groups",
        )
        .keys(const { &[cmd_shift(Key::G)] })
        .global()
        .when(When::Grouped)
        .aliases(&["UNGROUP"]),
        Spec::new(
            "edit.duplicate",
            C::Edit,
            K::Duplicate,
            "Duplicate",
            "Copy the selection in place, then move the copy",
        )
        .keys(const { &[shift(Key::D)] })
        .guard(Guard::Idle)
        .when(When::Selection)
        .repeat(true)
        .aliases(&["COPY", "CO", "CP"]),
        Spec::new(
            "edit.rename",
            C::Edit,
            K::Rename,
            "Rename block or group…",
            "Rename the picked group, or the block of the picked copies",
        )
        .keys(const { &[plain(Key::F2)] })
        .guard(Guard::IdleUnfocused)
        .aliases(&["RENAME", "REN"]),
        Spec::new(
            "edit.make_block",
            C::Edit,
            K::MakeBlock,
            "Make block",
            "A block from the selection (name it in the Blocks tab)",
        )
        .when(When::Curves)
        .aliases(&["BLOCK", "B"]),
        Spec::new(
            "edit.block_from_group",
            C::Edit,
            K::BlockFromGroup,
            "Make block from group",
            "The picked group as a block: named after it, with its fills",
        )
        .when(When::ConvertibleGroup),
        Spec::new(
            "edit.explode",
            C::Edit,
            K::Explode,
            "Explode block copies",
            "Turn the picked copies back into plain geometry",
        )
        .menu("Explode")
        .when(When::Copies)
        .aliases(&["EXPLODE", "X"]),
        Spec::new(
            "edit.explode_walls",
            C::Edit,
            K::ExplodeWalls,
            "Explode walls",
            "Turn the picked walls into plain lines, arcs and fills",
        )
        .when(When::Walls)
        .aliases(&["EXPLODEWALL"]),
        Spec::new(
            "edit.join_walls",
            C::Edit,
            K::JoinWalls,
            "Join walls",
            "Join two picked walls where they come nearest: an L, or a T",
        )
        .when(When::Walls)
        .aliases(&["JOINWALL", "WALLJOIN"]),
        Spec::new(
            "edit.unjoin_walls",
            C::Edit,
            K::UnjoinWalls,
            "Unjoin walls",
            "Stop the picked walls joining where they meet: their ends are capped",
        )
        .when(When::Walls)
        .aliases(&["UNJOINWALL", "DISALLOWJOIN"]),
        Spec::new(
            "edit.explode_to_group",
            C::Edit,
            K::ExplodeToGroup,
            "Explode block copies to groups",
            "Turn each picked copy into a group named after its block",
        )
        .menu("Explode to group")
        .when(When::Copies),
        Spec::new(
            "edit.select_all",
            C::Edit,
            K::SelectAll,
            "Select all",
            "Pick everything the selection filter lets through",
        )
        .keys(const { &[cmd(Key::A)] })
        .in_grab()
        .aliases(&["ALL"]),
        Spec::new(
            "edit.select_overlapping",
            C::Edit,
            K::SelectOverlapping,
            "Select overlapping curves",
            "Pick the curves that lie on top of others; Delete removes them",
        )
        .aliases(&["OVERKILL"]),
        Spec::new(
            "edit.clear_relations",
            C::Edit,
            K::ClearRelations,
            "Clear relations on the selection",
            "Remove the relations on the picked curves, as one step",
        )
        .menu("Clear relations")
        .when(When::RelationsOnSelection)
        .aliases(&["DELCON"]),
        Spec::new(
            "edit.clear_dimensions",
            C::Edit,
            K::ClearDimensions,
            "Clear dimensions on the selection",
            "Remove the dimensions on the picked curves, as one step",
        )
        .menu("Clear dimensions")
        .when(When::DimensionsOnSelection),
        Spec::new(
            "edit.pick.points",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Points)),
            "Select points only",
            "A selection filter: clicks and boxes pick points only",
        ),
        Spec::new(
            "edit.pick.edges",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Edges)),
            "Select edges only",
            "A selection filter: clicks and boxes pick curves only",
        ),
        Spec::new(
            "edit.pick.fills",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Fills)),
            "Select fills only",
            "A selection filter: clicks and boxes pick fills and hatches only",
        ),
        Spec::new(
            "edit.pick.areas",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Areas)),
            "Select areas only",
            "A selection filter: clicks pick closed areas only",
        ),
        Spec::new(
            "edit.pick.blocks",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Blocks)),
            "Select blocks & groups only",
            "A selection filter: clicks and boxes pick block copies and groups only",
        ),
        Spec::new(
            "edit.pick.text",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Text)),
            "Select text only",
            "A selection filter: clicks and boxes pick text only",
        ),
        Spec::new(
            "edit.pick.annotations",
            C::Edit,
            K::PickFilter(PickFilter::only(PickKind::Annotations)),
            "Select annotations only",
            "A selection filter: clicks and boxes pick dimensions and notes only",
        ),
        Spec::new(
            "edit.pick.all",
            C::Edit,
            K::PickFilter(PickFilter::ALL),
            "Select everything (no filter)",
            "Turn the selection filter off",
        ),
        Spec::new(
            "edit.save_option_values",
            C::Edit,
            K::SaveOption(false),
            "Save option (values only)",
            "Keep the variables' values as an option, to switch back to",
        )
        .words("variant parameters"),
        Spec::new(
            "edit.save_option_sketch",
            C::Edit,
            K::SaveOption(true),
            "Save option (whole sketch)",
            "Keep the whole sketch as an option, to switch back to",
        )
        .words("variant parameters"),
        tool("edit.select", C::Edit, ToolKind::Select)
            .keys(const { &[plain(Key::S)] })
            .repeat(false)
            .aliases(&["SELECT"]),
        Spec::new(
            "edit.preferences",
            C::Edit,
            K::Preferences,
            "Preferences…",
            "Theme, pointer, drawing aids and updates",
        )
        .keys(const { &[cmd(Key::Comma)] })
        .global()
        .aliases(&["OPTIONS", "OP", "SETTINGS"]),
        // View.
        Spec::new(
            "view.palette",
            C::View,
            K::Palette,
            "Command palette…",
            "Any tool, setting, panel or command, by name",
        )
        .keys(const { &[cmd(Key::K), cmd_shift(Key::P)] })
        .global(),
        Spec::new(
            "view.sheet",
            C::View,
            K::SheetPreview,
            "Sheet preview",
            "Show the drawing on its sheet, as it prints",
        )
        .menu("Sheet")
        .aliases(&["LAYOUT"]),
        Spec::new(
            "view.zoom_fit",
            C::View,
            K::ZoomToFit,
            "Zoom to fit",
            "Fit the view to the drawing (or double middle-click)",
        )
        .keys(const { &[shift(Key::F)] })
        .aliases(&["ZOOM", "Z", "ZE"]),
        Spec::new(
            "view.zoom_selection",
            C::View,
            K::ZoomToSelection,
            "Zoom to selection",
            "Fit the view to what is picked",
        )
        .keys(const { &[shift(Key::Z)] })
        .when(When::Picked)
        .aliases(&["ZO"]),
        Spec::new(
            "view.zoom_life_size",
            C::View,
            K::ZoomLifeSize,
            "Zoom to 100% (life size)",
            "Life size on a 96 dpi screen",
        ),
        Spec::new(
            "view.grid",
            C::View,
            K::Toggle(ViewFlag::Grid),
            "Grid",
            "Show or hide the grid",
        )
        .keys(const { &[cmd(Key::Quote)] })
        .global()
        .aliases(&["GRID"]),
        Spec::new(
            "view.axes",
            C::View,
            K::Toggle(ViewFlag::Axes),
            "Origin axes",
            "Show or hide the X and Y axes through the origin",
        ),
        Spec::new(
            "view.snap",
            C::View,
            K::Toggle(ViewFlag::Snap),
            "Snap to grid",
            "Snap to the grid",
        )
        .keys(const { &[shift(Key::G)] })
        .in_grab()
        .aliases(&["SNAP", "SN"]),
        Spec::new(
            "view.perp_tangent",
            C::View,
            K::Toggle(ViewFlag::PerpTangent),
            "Perpendicular and tangent snaps",
            "Snap perpendicular and tangent to curves while drawing",
        )
        .menu("Perpendicular & tangent")
        .aliases(&["OSNAP"]),
        Spec::new(
            "view.thin",
            C::View,
            K::Toggle(ViewFlag::Thin),
            "Thin lines",
            "Thin lines: every line one pixel wide",
        )
        .keys(const { &[shift_alt(Key::W)] })
        .aliases(&["LWDISPLAY"]),
        Spec::new(
            "view.status_colours",
            C::View,
            K::Toggle(ViewFlag::Status),
            "Constraint status colours",
            "Colour curves by constraint status, or by their own and their layer's colours",
        )
        .keys(const { &[shift(Key::C)] })
        .in_grab(),
        Spec::new(
            "view.endpoints",
            C::View,
            K::Toggle(ViewFlag::Endpoints),
            "Always show end points",
            "Show every end point, or only those of hovered and picked curves",
        )
        .keys(const { &[shift_alt(Key::V)] }),
        Spec::new(
            "view.dimensions",
            C::View,
            K::Toggle(ViewFlag::Dimensions),
            "Dimensions and notes",
            "Show or hide dimensions and notes (shown again at each start)",
        )
        .keys(const { &[alt(Key::D)] }),
        Spec::new(
            "view.hidden_annotations",
            C::View,
            K::Toggle(ViewFlag::Hidden),
            "Hidden annotations (faded)",
            "Show hidden dimensions and notes, faded, to pick and show them again",
        )
        .keys(const { &[alt(Key::H)] }),
        Spec::new(
            "view.relations",
            C::View,
            K::Toggle(ViewFlag::Relations),
            "Relations",
            "Show or hide relation symbols",
        )
        .keys(const { &[alt(Key::R)] }),
        Spec::new(
            "view.free",
            C::View,
            K::Toggle(ViewFlag::Free),
            "Free directions",
            "Arrows for the ways geometry can still move",
        ),
        Spec::new(
            "view.combs",
            C::View,
            K::Toggle(ViewFlag::Combs),
            "Curvature combs",
            "Curvature combs on curves",
        ),
        Spec::new(
            "view.crops",
            C::View,
            K::Toggle(ViewFlag::Crops),
            "View crops",
            "Show or hide views' crops and names on the canvas",
        )
        .keys(const { &[shift_alt(Key::B)] }),
        Spec::new(
            "view.wall_centrelines",
            C::View,
            K::Toggle(ViewFlag::Centrelines),
            "Wall centrelines",
            "Show every wall's centreline (otherwise only a picked or hovered wall's)",
        ),
        Spec::new(
            "view.auto_relations",
            C::View,
            K::Toggle(ViewFlag::AutoRelations),
            "Automatic relations",
            "Add relations automatically while drawing, or not",
        )
        .keys(const { &[shift_alt(Key::R)] }),
        Spec::new(
            "view.theme_dark",
            C::View,
            K::Theme(eframe::egui::ThemePreference::Dark),
            "Dark appearance",
            "Dark colours",
        ),
        Spec::new(
            "view.theme_light",
            C::View,
            K::Theme(eframe::egui::ThemePreference::Light),
            "Light appearance",
            "Light colours",
        ),
        Spec::new(
            "view.theme_system",
            C::View,
            K::Theme(eframe::egui::ThemePreference::System),
            "Appearance follows the system",
            "Dark or light as the system is",
        ),
        Spec::new(
            "view.panel.inspector",
            C::View,
            K::Panel(Panel::Inspector),
            "Inspector panel",
            Panel::Inspector.description(),
        )
        .menu(Panel::Inspector.title())
        .aliases(&["PROPERTIES", "PR"]),
        Spec::new(
            "view.panel.parameters",
            C::View,
            K::Panel(Panel::Parameters),
            "Parameters panel",
            Panel::Parameters.description(),
        )
        .menu(Panel::Parameters.title())
        .aliases(&["PARAMETERS"]),
        Spec::new(
            "view.panel.sheets",
            C::View,
            K::Panel(Panel::Sheets),
            "Sheets panel",
            Panel::Sheets.description(),
        )
        .menu(Panel::Sheets.title()),
        Spec::new(
            "view.panel.blocks",
            C::View,
            K::Panel(Panel::Blocks),
            "Blocks panel",
            Panel::Blocks.description(),
        )
        .menu(Panel::Blocks.title())
        .aliases(&["INSERT"]),
        Spec::new(
            "view.hide_panels",
            C::View,
            K::HideAllPanels,
            "Hide all panels",
            "Hide every panel for a clear view, and bring them back",
        )
        .keys(const { &[cmd(Key::Backslash)] })
        .global(),
        Spec::new(
            "view.reset_panels",
            C::View,
            K::ResetPanels,
            "Reset panel layout",
            "Every panel back in the right dock",
        ),
        // Draw.
        tool("draw.line", C::Draw, ToolKind::Line)
            .keys(const { &[plain(Key::L)] })
            .aliases(&["LINE", "L", "PLINE", "PL"]),
        tool("draw.pen", C::Draw, ToolKind::Pen)
            .keys(const { &[plain(Key::P)] })
            .aliases(&["SPLINE", "SPL"]),
        tool("draw.rectangle", C::Draw, ToolKind::Rectangle)
            .keys(const { &[plain(Key::R)] })
            .aliases(&["RECTANGLE", "REC", "RECTANG"]),
        tool("draw.polygon", C::Draw, ToolKind::Polygon)
            .keys(const { &[plain(Key::K)] })
            .aliases(&["POLYGON", "POL"]),
        tool("draw.circle", C::Draw, ToolKind::Circle)
            .keys(const { &[plain(Key::C)] })
            .aliases(&["CIRCLE", "C"]),
        tool("draw.ellipse", C::Draw, ToolKind::Ellipse).aliases(&["ELLIPSE", "EL"]),
        tool("draw.partial_ellipse", C::Draw, ToolKind::EllipseArc),
        tool("draw.arc", C::Draw, ToolKind::Arc)
            .keys(const { &[plain(Key::A)] })
            .aliases(&["ARC", "A"]),
        tool("draw.three_point_arc", C::Draw, ToolKind::ThreePointArc),
        tool("draw.tangent_arc", C::Draw, ToolKind::TangentArc),
        tool("draw.slot", C::Draw, ToolKind::Slot).keys(const { &[plain(Key::U)] }),
        tool("draw.wall", C::Draw, ToolKind::Wall)
            .keys(const { &[plain(Key::W)] })
            .aliases(&["WALL", "WA"]),
        Spec::new(
            "draw.walls_from_curves",
            C::Draw,
            K::WallsFromCurves,
            "Walls from curves",
            "Walls along the picked lines and arcs, at the Wall tool's settings",
        )
        .when(When::Curves)
        .words("pick lines centreline"),
        Spec::new(
            "draw.walls_from_lines",
            C::Draw,
            K::WallsFromLines,
            "Walls from lines",
            "Walls found among the picked lines: pairs of parallel faces, as imported plans draw them",
        )
        .when(When::Curves)
        .words("convert faces import dwg dxf plan pairs"),
        tool("draw.point", C::Draw, ToolKind::Point)
            .keys(const { &[plain(Key::V)] })
            .aliases(&["POINT", "PO"]),
        tool("draw.text", C::Draw, ToolKind::Text)
            .keys(const { &[shift(Key::T)] })
            .aliases(&["TEXT", "DT", "MTEXT", "MT"]),
        tool("draw.block", C::Draw, ToolKind::Block)
            .keys(const { &[plain(Key::I)] })
            .aliases(&["INSERT", "I"]),
        // Modify.
        Spec::new(
            "modify.move",
            C::Modify,
            K::Move,
            "Move",
            "Move the selection: X / Y lock an axis, type dX Tab dY, B picks the base point",
        )
        .modal("G")
        .when(When::Picked)
        .aliases(&["MOVE", "M"])
        .words("grab"),
        Spec::new(
            "modify.rotate",
            C::Modify,
            K::Rotate,
            "Rotate",
            "Turn the selection: type an angle, B picks the pivot, R again rotates by reference",
        )
        .modal("R (with a selection)")
        .when(When::Selection)
        .aliases(&["ROTATE", "RO"])
        .words("turn spin"),
        Spec::new(
            "modify.scale",
            C::Modify,
            K::Scale,
            "Scale",
            "Drag a handle of the selection's box: Shift keeps the shape, Ctrl scales about \
             the middle",
        )
        .when(When::Selection)
        .aliases(&["SCALE", "SC"])
        .words("resize stretch"),
        tool("modify.mirror", C::Modify, ToolKind::Mirror)
            .keys(const { &[plain(Key::M)] })
            .aliases(&["MIRROR", "MI"]),
        tool("modify.offset", C::Modify, ToolKind::Offset)
            .keys(const { &[plain(Key::O)] })
            .aliases(&["OFFSET", "O"]),
        tool("modify.trim", C::Modify, ToolKind::Trim)
            .keys(const { &[plain(Key::T)] })
            .aliases(&["TRIM", "TR"]),
        tool("modify.extend", C::Modify, ToolKind::Extend)
            .keys(const { &[plain(Key::E)] })
            .aliases(&["EXTEND", "EX"]),
        tool("modify.split", C::Modify, ToolKind::Split).aliases(&["BREAKATPOINT", "BR"]),
        tool("modify.fillet", C::Modify, ToolKind::Fillet)
            .keys(const { &[plain(Key::F)] })
            .aliases(&["FILLET", "F"]),
        tool("modify.chamfer", C::Modify, ToolKind::Chamfer).aliases(&["CHAMFER", "CHA"]),
        Spec::new(
            "modify.linear_pattern",
            C::Modify,
            K::Pattern(false),
            "Linear pattern…",
            "Copies of the selection in a row, a set distance apart (in the Inspector)",
        )
        .when(When::Curves)
        .aliases(&["ARRAY", "AR", "ARRAYRECT"])
        .words("array copies repeat"),
        Spec::new(
            "modify.circular_pattern",
            C::Modify,
            K::Pattern(true),
            "Circular pattern…",
            "Copies of the selection round a centre (in the Inspector)",
        )
        .when(When::Curves)
        .aliases(&["ARRAYPOLAR", "AR"])
        .words("array polar copies repeat"),
        // Annotate.
        tool("annotate.dimension", C::Annotate, ToolKind::Dimension)
            .keys(const { &[plain(Key::D)] })
            .aliases(&["DIMENSION", "DIM", "DLI", "DAL", "DRA", "DDI", "DAN"]),
        tool("annotate.ordinate", C::Annotate, ToolKind::Ordinate).aliases(&["DIMORDINATE", "DOR"]),
        tool("annotate.note", C::Annotate, ToolKind::Note)
            .keys(const { &[plain(Key::N)] })
            .aliases(&["LEADER", "LE", "MLEADER", "MLD"]),
        tool("annotate.callout", C::Annotate, ToolKind::Callout),
        tool("annotate.centre_mark", C::Annotate, ToolKind::CentreMark).aliases(&[
            "DIMCENTER",
            "DCE",
            "CENTERMARK",
        ]),
        tool("annotate.section", C::Annotate, ToolKind::Section),
        tool("annotate.elevation", C::Annotate, ToolKind::Elevation),
        tool("annotate.view", C::Annotate, ToolKind::View).aliases(&["VIEWBASE", "MVIEW", "MV"]),
        tool("annotate.fill", C::Annotate, ToolKind::Fill)
            .keys(const { &[plain(Key::H)] })
            .aliases(&["HATCH", "H", "BHATCH"]),
        Spec::new(
            "annotate.align_dimensions",
            C::Annotate,
            K::AlignDimensions,
            "Align picked dimensions",
            "Put the picked parallel dimensions on the first one's line",
        )
        .menu("Align dimensions")
        .when(When::AlignableDimensions)
        .aliases(&["DIMSPACE"]),
        Spec::new(
            "annotate.space_dimensions",
            C::Annotate,
            K::SpaceDimensions,
            "Space picked dimensions evenly",
            "Space three or more stacked parallel dimensions evenly between the outermost two",
        )
        .menu("Space dimensions evenly")
        .when(When::SpaceableDimensions)
        .aliases(&["DIMSPACE"]),
        Spec::new(
            "annotate.hide",
            C::Annotate,
            K::HideAnnotations,
            "Hide picked dimensions and notes on the canvas",
            "They stay in the drawing (Alt+H shows them faded)",
        )
        .menu("Hide on canvas")
        .when(When::PickedAnnotations),
        Spec::new(
            "annotate.off_sheets",
            C::Annotate,
            K::AnnotationsOffSheets,
            "Leave picked dimensions and notes off sheets",
            "Sheets, PDF, SVG, DXF and print leave them out",
        )
        .menu("Leave off sheets")
        .when(When::PickedAnnotations),
        // Arrange.
        Spec::new(
            "arrange.to_front",
            C::Arrange,
            K::Arrange(Arrange::ToFront),
            Arrange::ToFront.label(),
            "Bring the picked fills and shapes to the front of their layer",
        )
        .keys(const { &[cmd_shift(Key::CloseBracket)] })
        .global()
        .when(When::Arrangeable)
        .aliases(&["DRAWORDER", "DR"]),
        Spec::new(
            "arrange.forward",
            C::Arrange,
            K::Arrange(Arrange::Forward),
            Arrange::Forward.label(),
            "Bring the picked fills and shapes forward one step on their layer",
        )
        .keys(const { &[cmd(Key::CloseBracket)] })
        .global()
        .when(When::Arrangeable),
        Spec::new(
            "arrange.backward",
            C::Arrange,
            K::Arrange(Arrange::Backward),
            Arrange::Backward.label(),
            "Send the picked fills and shapes backward one step on their layer",
        )
        .keys(const { &[cmd(Key::OpenBracket)] })
        .global()
        .when(When::Arrangeable),
        Spec::new(
            "arrange.to_back",
            C::Arrange,
            K::Arrange(Arrange::ToBack),
            Arrange::ToBack.label(),
            "Send the picked fills and shapes to the back of their layer",
        )
        .keys(const { &[cmd_shift(Key::OpenBracket)] })
        .global()
        .when(When::Arrangeable)
        .aliases(&["DRAWORDER", "DR"]),
        // Layers.
        Spec::new(
            "layers.panel",
            C::Layers,
            K::Panel(Panel::Layers),
            "Layers panel",
            Panel::Layers.description(),
        )
        .menu(Panel::Layers.title())
        .keys(const { &[cmd(Key::L)] })
        .global()
        .aliases(&["LAYER", "LA"]),
        // Help.
        Spec::new(
            "help.guide",
            C::Help,
            K::UserGuide,
            "User guide",
            "The user guide, in your browser",
        )
        .aliases(&["HELP"]),
        Spec::new(
            "help.report",
            C::Help,
            K::ReportProblem,
            "Report a problem or ask for a feature",
            "Bug reports and feature requests, on GitHub",
        )
        .menu("Report a problem…"),
        Spec::new(
            "help.shortcuts",
            C::Help,
            K::Shortcuts,
            "Keyboard shortcuts",
            "This list, with a filter",
        )
        .keys(const { &[typed(Key::Questionmark)] })
        .in_grab(),
        Spec::new(
            "help.benchmark",
            C::Help,
            K::Benchmark,
            "Run solver benchmark",
            "For development",
        )
        .when(When::Debug),
        Spec::new(
            "help.about",
            C::Help,
            K::About,
            "About Drift",
            "Version, licences and credits",
        ),
    ]
}

/// Keys that aren't commands: they act inside a tool or with the mouse. Shown in the
/// shortcuts list and the guide after the commands, by section.
pub(crate) const OTHER_KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "While drawing",
        &[
            (
                "X / Y",
                "Lock the segment to the X or Y axis (press again to unlock)",
            ),
            ("0–9 . - (", "Start typing a value"),
            (
                "=",
                "Start typing with a variable name or expression, such as `=width/2`",
            ),
            ("Tab", "Next field (length → angle, width → height…)"),
            (
                "Enter",
                "Apply the typed values; with nothing typed, finish a polyline",
            ),
            ("Right-click", "Finish a polyline"),
            (
                "A / L",
                "In a polyline: switch to a tangent arc, or back to lines, from the last point; \
                 drawing walls: an arc or a straight segment next",
            ),
            ("W", "Drawing walls: straight or arc segments"),
            ("Space", "Drawing walls: the other face (Left ↔ Right)"),
            ("Backspace", "Drawing walls: take back the last segment"),
            (
                "Alt (hold)",
                "Place without snapping (also no perpendicular or tangent snaps)",
            ),
            (
                "Esc",
                "Stop typing, cancel the step, or (pressed again) go back to Select",
            ),
        ],
    ),
    (
        "Selection",
        &[
            (
                "Click / drag",
                "Select, or box-select (right: window, left: crossing); Shift adds, Ctrl \
                 toggles",
            ),
            (
                "Tab",
                "Highlight the chain under the pointer (click to pick it); with nothing under \
                 the pointer, widen the selection to whole chains",
            ),
            ("Click near a line's middle", "Pick its midpoint"),
            ("Arrow keys", "Nudge one grid step; Shift+arrow, ten"),
            ("Q", "Relations pie: hold and flick, or tap and click"),
            ("Double-click text", "Type into it in place"),
            (
                "Enter or Space",
                "With nothing in hand: the last command or tool again",
            ),
            (
                "Esc",
                "Cancel the operation, or clear the selection; with a dialog or window open, \
                 close it (and nothing else)",
            ),
            (
                "Right-click",
                "In a drawing tool, finish (as Enter does); in Select, the canvas menu; in a \
                 move or rotate, cancel",
            ),
        ],
    ),
    (
        "Dimensions",
        &[
            (
                "Shift+click (after a horizontal or vertical dimension)",
                "Chain dimensions along the row; B switches to baseline",
            ),
            ("Double-click a dimension", "Edit its value"),
            ("Drag a dimension", "Move its label"),
        ],
    ),
    (
        "Mouse",
        &[
            ("Double middle-click", "Zoom to fit"),
            ("Middle or right drag", "Pan"),
            (
                "Scroll / pinch",
                "Zoom at the pointer (two fingers on a trackpad pan)",
            ),
            (
                "Drag a panel's tab",
                "Onto the canvas to float it, or onto either side to dock it",
            ),
        ],
    ),
    (
        "Sheet preview",
        &[
            ("V", "Add a view, by its region in the model"),
            ("Shift+F", "Fit the sheet"),
            (
                "Delete or Backspace",
                "Take the picked view off the sheet (it stays in the drawing)",
            ),
            ("Esc", "Back to the model"),
        ],
    ),
];

/// A row of the shortcuts list: the keys, the command (empty for a tool's own keys) and
/// what it does.
pub(crate) type ShortcutRow = (String, String, String);

/// The shortcuts list, by section: the categories' commands with keys, then [`OTHER_KEYS`].
/// Rows are (keys, command, what it does).
pub(crate) fn shortcut_sections(mac: bool) -> Vec<(&'static str, Vec<ShortcutRow>)> {
    let mut out = Vec::new();
    for category in Category::ALL {
        let rows: Vec<_> = registry()
            .iter()
            .filter(|s| s.category == category && !s.when.hidden())
            .filter(|s| !s.keys.is_empty() || !s.modal.is_empty())
            .map(|s| (s.all_keys(mac), s.title.to_owned(), s.does()))
            .collect();
        if !rows.is_empty() {
            out.push((category.label(), rows));
        }
    }
    for (section, rows) in OTHER_KEYS {
        let rows = rows
            .iter()
            .map(|(k, does)| (k.to_string(), String::new(), does.to_string()))
            .collect();
        out.push((*section, rows));
    }
    out
}

/// The tables in `docs/guide/reference/keyboard-shortcuts.md`, between its markers.
#[cfg(test)]
pub(crate) fn docs_tables() -> String {
    let cell = |s: &str| s.replace('|', "\\|");
    let mut out = String::new();
    for (section, rows) in shortcut_sections(false) {
        // The commands' sections have a column for the command; the others don't.
        let commands = rows.iter().any(|(_, title, _)| !title.is_empty());
        out.push_str(&format!("\n## {section}\n\n"));
        out.push_str(if commands {
            "| Key | Command | Does |\n|---|---|---|\n"
        } else {
            "| Key | Does |\n|---|---|\n"
        });
        for (keys, title, does) in rows {
            let (keys, does) = (cell(&keys), cell(&does));
            if commands {
                let title = cell(&title);
                out.push_str(&format!("| {keys} | **{title}** | {does} |\n"));
            } else {
                out.push_str(&format!("| {keys} | {does} |\n"));
            }
        }
        // The tools and commands with no key of their own, in the menus and the palette.
        let keyless: Vec<String> = registry()
            .iter()
            .filter(|s| s.category.label() == section && !s.when.hidden())
            .filter(|s| {
                matches!(
                    s.category,
                    Category::Draw | Category::Modify | Category::Annotate
                )
            })
            .filter(|s| s.key_text(false).is_empty())
            .map(|s| format!("**{}**", s.title.trim_end_matches('…')))
            .collect();
        if !keyless.is_empty() {
            out.push_str(&format!(
                "\nWith no key (use the {section} menu or the command palette): {}.\n",
                keyless.join(", ")
            ));
        }
    }
    out
}

/// The commands whose keys the canvas dispatches, the more specific shortcuts first.
pub(crate) fn dispatched() -> &'static [(Shortcut, &'static Spec)] {
    static KEYS: LazyLock<Vec<(Shortcut, &'static Spec)>> = LazyLock::new(|| {
        let mut keys: Vec<_> = registry()
            .iter()
            .filter(|s| s.dispatched)
            .flat_map(|s| s.keys.iter().map(move |k| (*k, s)))
            .collect();
        keys.sort_by_key(|(k, _)| std::cmp::Reverse(k.weight()));
        keys
    });
    &KEYS
}

/// How well a palette query matches a command. Exact names first: a command's own key ("M"
/// is Mirror), then an alias (AutoCAD's "RO"), then its title; then prefixes and words.
pub(crate) fn rank(query: &str, spec: &Spec) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    // (G is Drift's move, though AutoCAD's G groups.)
    let own_key = spec
        .keys
        .iter()
        .filter_map(|k| k.single())
        .any(|c| c.to_lowercase().eq(q.chars()));
    if own_key || (spec.modal.len() == 1 && spec.modal.eq_ignore_ascii_case(&q)) {
        return Some(2100);
    }
    if spec.aliases.iter().any(|a| a.to_lowercase() == q) {
        return Some(2000);
    }
    let title = spec.title.trim_end_matches('…').to_lowercase();
    if title == q {
        return Some(1900);
    }
    let alias = spec
        .aliases
        .iter()
        .filter(|a| a.len() > 2)
        .filter_map(|a| score(&q, a).filter(|s| *s >= 800).map(|s| s - 100))
        .max();
    let title = score(&q, spec.title);
    let group = score(&q, spec.category.label()).map(|s| s / 3);
    let words = (q.len() >= 3
        && (spec.words.contains(q.as_str()) || spec.description.to_lowercase().contains(&q)))
    .then_some(150);
    title.max(alias).max(group).max(words)
}

/// How well `query` matches `text`: whole-word and prefix matches first, then substrings,
/// then letters in order. `None` when it doesn't match. Letters in order may cross words for
/// a short query only ("pta" finds "3-pt arc"); five or more letters stay in one word, so
/// "rotate" doesn't find "Report a problem".
pub(crate) fn score(query: &str, text: &str) -> Option<i32> {
    let q = query.to_lowercase();
    let t = text.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if t.starts_with(&q) {
        return Some(1000 - t.len() as i32);
    }
    if let Some(at) = t.find(&q) {
        let word_start = at == 0 || !t.as_bytes()[at - 1].is_ascii_alphanumeric();
        return Some(if word_start { 800 } else { 500 } - at as i32);
    }
    if q.chars().count() >= 5 {
        return t
            .split(|c: char| !c.is_alphanumeric())
            .filter_map(|word| scattered(&q, word))
            .max()
            .map(|s| s - t.len() as i32 / 4);
    }
    scattered(&q, &t).map(|s| s - t.len() as i32 / 4)
}

/// Letters of `q` in order in `t`, preferring those that start words.
fn scattered(q: &str, t: &str) -> Option<i32> {
    let mut chars = t.chars();
    let mut total = 0;
    let mut prev = ' ';
    for qc in q.chars() {
        loop {
            let c = chars.next()?;
            let starts = !prev.is_alphanumeric();
            prev = c;
            if c == qc {
                total += if starts { 12 } else { 2 };
                break;
            }
        }
    }
    Some(100 + total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_beat_substrings_beat_scattered_letters() {
        let prefix = score("rec", "Rectangle").unwrap();
        let word = score("arc", "3-pt arc").unwrap();
        let inner = score("ang", "Rectangle").unwrap();
        let scattered = score("rct", "Rectangle").unwrap();
        assert!(prefix > word && word > inner && inner > scattered);
        assert!(score("xyz", "Rectangle").is_none());
        assert_eq!(score("", "Anything"), Some(0));
    }

    #[test]
    fn long_queries_dont_match_across_words() {
        assert!(score("rotate", "Report a problem or ask for a feature").is_none());
        // Within a word they still do, and short ones may cross words.
        assert!(score("rctngl", "Rectangle").is_some());
        assert!(score("pta", "3-pt arc").is_some());
    }

    #[test]
    fn ids_are_unique_and_every_command_is_registered_once() {
        let mut ids = std::collections::HashSet::new();
        let mut commands = Vec::new();
        for s in registry() {
            assert!(ids.insert(s.id), "two commands are {}", s.id);
            assert!(
                !commands.contains(&s.command),
                "{:?} is registered twice",
                s.command
            );
            commands.push(s.command);
            assert!(!s.title.is_empty() && !s.description.is_empty(), "{}", s.id);
        }
        // Every tool on the toolbar is a command.
        for (_, tools) in ToolKind::SECTIONS {
            for kind in tools.iter() {
                assert!(spec(Command::Tool(*kind)).is_some(), "{kind:?}");
            }
        }
    }

    /// No two commands share a key (walls wanting W would fail here while W is thin lines),
    /// and no command takes a key a tool uses while it is in hand.
    #[test]
    fn no_two_commands_share_a_shortcut() {
        let mut seen: Vec<(Shortcut, &str)> = Vec::new();
        for s in registry() {
            for k in s.keys {
                if let Some((_, other)) = seen.iter().find(|(o, _)| o == k) {
                    panic!("{} and {} are both {}", other, s.id, k.text(false));
                }
                seen.push((*k, s.id));
            }
        }
        // W is the Wall tool; thin lines are Shift+Alt+W.
        assert_eq!(
            seen.iter()
                .find(|(k, _)| *k == plain(Key::W))
                .map(|(_, id)| *id),
            Some("draw.wall"),
            "W is the Wall tool"
        );
        assert_eq!(
            spec(Command::Toggle(ViewFlag::Thin)).unwrap().keys,
            &[shift_alt(Key::W)]
        );
        // The tools' own keys: X / Y lock, G grabs, B picks a base point, Q's pie, Tab,
        // Enter / Space (finish, repeat), Esc and the arrows.
        for reserved in [
            Key::X,
            Key::Y,
            Key::G,
            Key::B,
            Key::Q,
            Key::Tab,
            Key::Enter,
            Key::Space,
            Key::Escape,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
        ] {
            if let Some((_, id)) = seen.iter().find(|(k, _)| *k == plain(reserved)) {
                panic!("{id} takes {}, which tools use", reserved.name());
            }
        }
    }

    #[test]
    fn exact_names_rank_first_and_drifts_own_keys_win() {
        let best = |q: &str| {
            registry()
                .iter()
                .filter_map(|s| rank(q, s).map(|r| (r, s.id)))
                .enumerate()
                .max_by_key(|(i, (r, _))| (*r, std::cmp::Reverse(*i)))
                .map(|(_, x)| x)
                .map(|(_, id)| id)
        };
        assert_eq!(best("RO"), Some("modify.rotate"));
        assert_eq!(best("rotate"), Some("modify.rotate"));
        assert_eq!(best("SC"), Some("modify.scale"));
        assert_eq!(best("TR"), Some("modify.trim"));
        assert_eq!(best("EX"), Some("modify.extend"));
        assert_eq!(best("CHA"), Some("modify.chamfer"));
        assert_eq!(best("REC"), Some("draw.rectangle"));
        assert_eq!(best("DIM"), Some("annotate.dimension"));
        assert_eq!(best("AR"), Some("modify.linear_pattern"));
        // M is Drift's Mirror, though AutoCAD's M moves; MOVE finds Move.
        assert_eq!(best("m"), Some("modify.mirror"));
        assert_eq!(best("move"), Some("modify.move"));
        // G is Drift's move too, though AutoCAD's G groups.
        assert_eq!(best("g"), Some("modify.move"));
        // Every single-key shortcut finds its own command.
        for s in registry() {
            for c in s.keys.iter().filter_map(|k| k.single()) {
                assert_eq!(best(&c.to_string()), Some(s.id), "{c}");
            }
        }
        // "rotate" no longer finds the problem report at all.
        let report = spec(Command::ReportProblem).unwrap();
        assert!(rank("rotate", report).is_none());
    }

    #[test]
    fn modify_commands_are_registered() {
        for id in [
            "modify.move",
            "modify.rotate",
            "modify.scale",
            "modify.mirror",
            "modify.linear_pattern",
            "modify.circular_pattern",
            "modify.offset",
            "modify.trim",
            "modify.extend",
            "modify.fillet",
            "modify.chamfer",
        ] {
            let s = registry().iter().find(|s| s.id == id).expect(id);
            assert_eq!(s.category, Category::Modify, "{id}");
        }
    }

    /// The guide's shortcut tables are made from the registry. After changing a key or a
    /// description, run with `DRIFT_BLESS_DOCS=1` to rewrite them.
    #[test]
    fn docs_shortcuts_match_the_registry() {
        const BEGIN: &str = "<!-- BEGIN commands: made from crates/drift-app/src/commands.rs -->";
        const END: &str = "<!-- END commands -->";
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/guide/reference/keyboard-shortcuts.md"
        );
        let text = std::fs::read_to_string(path).expect("the shortcuts page");
        let start = text.find(BEGIN).expect("the BEGIN marker") + BEGIN.len();
        let end = text.find(END).expect("the END marker");
        let made = format!("\n{}\n", docs_tables());
        if text[start..end] != made {
            if std::env::var_os("DRIFT_BLESS_DOCS").is_some() {
                let text = format!("{}{made}{}", &text[..start], &text[end..]);
                std::fs::write(path, text).expect("write the shortcuts page");
            } else {
                panic!(
                    "docs/guide/reference/keyboard-shortcuts.md is out of date with the command \
                     registry: run `DRIFT_BLESS_DOCS=1 cargo test -p drift-app docs_shortcuts` \
                     and check the diff"
                );
            }
        }
    }
}
