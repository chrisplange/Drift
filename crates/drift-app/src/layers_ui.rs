//! Layers: editor commands (each one undo step), the Layers tab, and the layer / appearance
//! controls for the selection in the Properties tab.

use drift_core::{EntityId, Layer, LayerId, LineType, LineWeight, Rgb};
use eframe::egui;

use crate::DriftApp;
use crate::colour::{Follows, Unset, paint_swatch};
use crate::icons::{self, Glyph};
use drift_edit::editor::LayerFlag;

/// The Layers panel's own state: the name filter, the isolated layer, and the row heights
/// the list is laid out with (only the rows in view are drawn).
#[derive(Default)]
pub(crate) struct LayerList {
    /// Rows show when their name contains this (any case).
    pub filter: String,
    /// The layer shown on its own, and how every layer was before.
    pub isolated: Option<Isolated>,
    /// Layer settings Alt-clicked onto all the other layers (at most one per setting), and
    /// how every layer was before.
    pub solos: Vec<Solo>,
    /// The height of a row, with the space after it, as last drawn.
    row: f32,
    /// Layers picked in the list (Ctrl-click, Shift-click) to change together: a colour,
    /// weight, line type or printing set on one of them goes to all of them.
    pub picked: std::collections::BTreeSet<LayerId>,
    /// Where a Shift-click range starts.
    anchor: Option<LayerId>,
    /// A row to scroll into view.
    reveal: Option<LayerId>,
    /// The picked object's layer, outlined in the list (see [`LayerOnPick`]).
    pub shown: Option<LayerId>,
    /// The pick as last seen, to tell a new one (see [`DriftApp::follow_pick`]).
    seen: Option<PickSignature>,
}

/// What picking an object does in the Layers panel (Preferences → Layers, kept per user,
/// not in the drawing; drift-releases#19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LayerOnPick {
    /// Nothing: the panel only marks the layers the selection is on.
    Nothing,
    /// The panel scrolls to the picked object's layer and outlines it; the current layer
    /// stays.
    #[default]
    Show,
    /// As Show, and the picked object's layer becomes the current layer (not a locked or
    /// hidden one).
    MakeCurrent,
}

impl LayerOnPick {
    pub(crate) const ALL: [LayerOnPick; 3] = [
        LayerOnPick::Nothing,
        LayerOnPick::Show,
        LayerOnPick::MakeCurrent,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            LayerOnPick::Nothing => "Do nothing",
            LayerOnPick::Show => "Show its layer in the Layers panel",
            LayerOnPick::MakeCurrent => "Make its layer current",
        }
    }

    /// What it does, said under the menu.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            LayerOnPick::Nothing => {
                "The Layers panel marks the layers the selection is on, and nothing else \
                 changes."
            }
            LayerOnPick::Show => {
                "The Layers panel scrolls to the picked object's layer and outlines it. The \
                 current layer stays as it is."
            }
            LayerOnPick::MakeCurrent => {
                "The picked object's layer becomes the current layer, so what you draw next \
                 goes on it, and the Layers panel shows it. Not an undo step. A locked or \
                 hidden layer is shown but never made current."
            }
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            LayerOnPick::Nothing => "n",
            LayerOnPick::Show => "s",
            LayerOnPick::MakeCurrent => "c",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// What is picked, cheaply: the active element and the ends and sizes of each picked set.
/// It can miss a pick that swaps one middle member for another of the same count (a box
/// pick of the same number of things, say); that pick's layer is then not followed until
/// the next one, which is fine for revealing and following a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PickSignature {
    active: Option<EntityId>,
    curves: (usize, Option<EntityId>, Option<EntityId>),
    copies: (
        usize,
        Option<drift_core::InstanceId>,
        Option<drift_core::InstanceId>,
    ),
    texts: (
        usize,
        Option<drift_core::TextId>,
        Option<drift_core::TextId>,
    ),
    fills: (
        usize,
        Option<drift_core::FillId>,
        Option<drift_core::FillId>,
    ),
}

impl PickSignature {
    fn of(ed: &drift_edit::editor::Editor) -> Self {
        fn ends<T: Copy + Ord>(
            set: &std::collections::BTreeSet<T>,
        ) -> (usize, Option<T>, Option<T>) {
            (set.len(), set.first().copied(), set.last().copied())
        }
        Self {
            active: ed.active(),
            curves: ends(&ed.selection),
            copies: ends(&ed.selected_instances),
            texts: ends(&ed.selected_texts),
            fills: ends(&ed.selected_fills),
        }
    }

    fn is_empty(&self) -> bool {
        self.curves.0 + self.copies.0 + self.texts.0 + self.fills.0 == 0
    }
}

/// A layer shown on its own; ending the isolate puts `before` back.
pub(crate) struct Isolated {
    pub layer: LayerId,
    pub before: Vec<(LayerId, bool)>,
}

/// A setting Alt-clicked onto every layer but `layer`; ending it puts `before` back.
pub(crate) struct Solo {
    pub flag: LayerFlag,
    pub layer: LayerId,
    pub before: Vec<(LayerId, bool)>,
}

/// What a row asks of the panel, done once the list is drawn.
enum LayerAction {
    Isolate(LayerId),
    EndIsolate,
    ShowAll,
    /// Alt-click on a row's lock or snap toggle.
    Solo(LayerFlag, LayerId),
}

/// The appearance settings picked in a row's controls this frame, even to the value they
/// had: what goes to the other picked rows.
#[derive(Clone, Copy, Debug, Default)]
struct Touched {
    color: bool,
    fill: bool,
    weight: bool,
    line_type: bool,
    printable: bool,
}

impl Touched {
    fn any(&self) -> bool {
        self.color || self.fill || self.weight || self.line_type || self.printable
    }
}

/// The layers and how many curves each holds, worked out again only when the drawing
/// changes (an imported DWG can have a hundred thousand entities).
#[derive(Default)]
pub(crate) struct LayerCounts {
    /// The document revision, entity count and layer count these were taken at.
    key: Option<(u64, usize, usize)>,
    /// The layers, bottom up.
    pub layers: Vec<(LayerId, Layer)>,
    /// Curves on each layer.
    pub curves: std::collections::HashMap<LayerId, usize>,
    /// Curves in all.
    pub total: usize,
}

/// How many of the drawing's colours a picker offers.
const DOCUMENT_COLOURS: usize = 16;

impl LayerCounts {
    /// Bring the counts up to date with `doc`.
    pub fn update(&mut self, doc: &drift_core::Document) {
        let key = (doc.revision(), doc.len(), doc.layers().count());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.layers = doc.layers().map(|(id, l)| (id, l.clone())).collect();
        self.curves.clear();
        self.total = 0;
        for (_, e) in doc.iter() {
            if !e.kind.is_point() {
                *self.curves.entry(e.layer).or_default() += 1;
                self.total += 1;
            }
        }
    }
}

/// The drawing's own colours (its layers', then its objects', fills' and texts'), first in
/// every colour picker. A pass over the whole drawing, so made only while a picker is open,
/// and keyed on the drawing's structure, which a colour can't change without: geometry
/// moving (a drag's every frame) leaves them as they are, picker open or not.
#[derive(Default)]
pub(crate) struct DocumentColours {
    /// The document structure, entity count and layer count these were taken at.
    key: Option<(u64, usize, usize)>,
    colours: Vec<[u8; 3]>,
    /// Passes over the drawing made so far (for the tests).
    #[cfg(test)]
    pub scans: usize,
}

impl DocumentColours {
    fn key(doc: &drift_core::Document) -> (u64, usize, usize) {
        (doc.structure(), doc.len(), doc.layers().count())
    }

    /// Up to date with `doc` (worked out again only if it changed).
    fn update(&mut self, doc: &drift_core::Document) {
        let key = Self::key(doc);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        #[cfg(test)]
        {
            self.scans += 1;
        }
        self.colours.clear();
        let colours = &mut self.colours;
        let mut add = |c: Option<Rgb>| {
            if let Some(c) = c
                && colours.len() < DOCUMENT_COLOURS
                && !colours.contains(&c.0)
            {
                colours.push(c.0);
            }
        };
        for (_, l) in doc.layers() {
            add(l.color);
            add(l.fill_color);
        }
        for (_, e) in doc.iter() {
            add(e.appearance.color);
        }
        for (_, f) in doc.fills() {
            add(f.color);
        }
        for (_, t) in doc.texts() {
            add(t.appearance.color);
        }
    }

    /// Leave the colours for this pass's pickers: brought up to date first while a picker is
    /// open (it asks every pass, so a change made during the last pass is caught here, with no
    /// pass run again), and marked stale when `doc` has changed since, so a picker that opens
    /// on them runs the pass again. Checked only here, at the top of the pass: a change made
    /// later in this pass reaches an open picker on the next one.
    pub(crate) fn publish(&mut self, ctx: &egui::Context, doc: &drift_core::Document) {
        if crate::colour::document_colours_wanted(ctx) {
            self.update(doc);
        }
        let fresh = self.key == Some(Self::key(doc));
        crate::colour::offer_document_colours(ctx, fresh, self.colours.clone());
    }
}

/// Side of an [`icon_toggle`]: with a row's margins, a list row's height.
const ICON_TOGGLE: f32 = 22.0;

/// A small icon toggle: full colour when on, faint (and struck through for `strike`) when off.
/// It tells assistive tech whether it is on. True when it was clicked.
pub(crate) fn icon_toggle(
    ui: &mut egui::Ui,
    on: &mut bool,
    glyph: Glyph,
    strike: bool,
    tip: &str,
) -> bool {
    let r = icons::toggle(ui, ICON_TOGGLE, on, tip, |painter, rect, color, on| {
        let color = if on {
            color.gamma_multiply(0.85)
        } else {
            color.gamma_multiply(0.35)
        };
        icons::glyph(painter, rect, glyph, color);
        if strike && !on {
            painter.line_segment(
                [rect.left_bottom(), rect.right_top()],
                egui::Stroke::new(1.5, color),
            );
        }
    });
    r.on_hover_text(tip).clicked()
}

/// `text` shortened in the middle with an ellipsis to fit `width` (CAD layer names share
/// their beginnings and differ at the end, A-WALL-FULL / A-WALL-FULL-PATT).
fn elide_middle(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |keep: usize| {
        let head = keep.div_ceil(2);
        let tail = keep - head;
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.extend(&chars[chars.len() - tail..]);
        s
    };
    // The most characters that still fit.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&cut(mid)) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

pub(crate) fn to_color32(c: Option<Rgb>, dark: bool) -> egui::Color32 {
    match c {
        Some(Rgb([r, g, b])) => egui::Color32::from_rgb(r, g, b),
        None if dark => egui::Color32::from_gray(0xe6),
        None => egui::Color32::from_gray(0x20),
    }
}

/// A colour control (a swatch and its hex; one click opens the picker), `name` saying what
/// it colours (to assistive tech, with its colour as the value) and `auto_label` naming what
/// an unset colour falls back to ("Auto": the theme's ink). `fallback` is the colour that
/// fallback is now (a layer's, the default ink), shown with a small link while unset; None
/// only when it isn't one colour (picks on layers of different colours). It takes the width
/// it is given: a row's value column.
pub(crate) fn color_choice(
    ui: &mut egui::Ui,
    name: &str,
    value: Option<Rgb>,
    auto_label: &str,
    fallback: Option<Follows>,
    dark: bool,
) -> Option<Option<Rgb>> {
    let width = ui.available_width();
    color_choice_of(ui, name, value, auto_label, fallback, dark, width)
}

/// [`color_choice`] at a set width (on a bar, among other controls).
pub(crate) fn color_choice_of(
    ui: &mut egui::Ui,
    name: &str,
    value: Option<Rgb>,
    auto_label: &str,
    fallback: Option<Follows>,
    dark: bool,
    width: f32,
) -> Option<Option<Rgb>> {
    let unset = Unset::from_label(auto_label);
    crate::colour::colour_button(ui, name, value, unset, fallback, dark, width)
}

/// A line weight's menu, as wide as the room it is given (a row's value column).
pub(crate) fn weight_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<LineWeight>,
    by_layer: bool,
) -> bool {
    let text = value.map_or("By layer".into(), LineWeight::label);
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .icon(crate::theme::combo_icon)
        .selected_text(text)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            if by_layer {
                changed |= ui.selectable_value(value, None, "By layer").changed();
            }
            for w in LineWeight::ISO {
                changed |= ui.selectable_value(value, Some(w), w.label()).changed();
            }
        });
    changed
}

fn line_type_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<LineType>,
    by_layer: bool,
    width: f32,
) -> bool {
    let text = value.map_or("By layer", LineType::label);
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .icon(crate::theme::combo_icon)
        .selected_text(text)
        .width(width)
        .show_ui(ui, |ui| {
            if by_layer {
                changed |= ui.selectable_value(value, None, "By layer").changed();
            }
            for t in LineType::ALL {
                changed |= ui
                    .selectable_value(value, Some(t), crate::theme::us(t.label()))
                    .changed();
            }
            // A curve's own (never a layer's): no stroke.
            if by_layer {
                changed |= ui
                    .selectable_value(value, Some(LineType::None), LineType::None.label())
                    .on_hover_text("Not drawn or printed; still bounds areas, snaps and solves")
                    .changed();
            }
        });
    changed
}

/// A layer row's grid: the current layer's dot, the stroke and fill swatches, and the gap
/// between its parts. The name follows; the line cell and the toggles are at the right.
const DOT: f32 = 10.0;
const SWATCH: f32 = 13.0;
const ROW_SPACING: f32 = 5.0;
/// The weight and line type cell: a sample of the line and its weight.
const LINE_CELL: f32 = 48.0;
/// How long the line sample in a layer row is: room for a phantom line's long dash, its two
/// dots and the next long dash, each dot and gap at least [`MIN_MARK`].
const ROW_SAMPLE: f32 = 18.0;
/// The shortest dot or gap a line sample draws, in points: two pixels at one point per
/// pixel, so the edges egui feathers leave a gap clear between two dots.
const MIN_MARK: f32 = 2.0;
// The row's sample (from 3 points in) leaves room in its cell for the weight: "0.25" is
// about 20 points wide, 3 in from the right, and a few points of gap.
const _: () = assert!(3.0 + ROW_SAMPLE + 25.0 <= LINE_CELL);

/// Space between two rows of the list.
const ROW_GAP: f32 = 1.0;

fn pos_of(list: &[LayerId], id: LayerId) -> Option<usize> {
    list.iter().position(|l| *l == id)
}

/// The bar above the list while a layer is isolated; true when "End isolate" is clicked.
fn isolate_bar(ui: &mut egui::Ui, name: &str) -> bool {
    let accent = crate::theme::accent(ui.visuals().dark_mode);
    let mut end = false;
    egui::Frame::new()
        .fill(crate::theme::accent_soft(ui.visuals().dark_mode))
        .stroke(egui::Stroke::new(1.0, accent.gamma_multiply(0.5)))
        .corner_radius(crate::theme::radius::CONTROL)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    end = ui
                        .button("End isolate")
                        .on_hover_text("Show the other layers again, as they were")
                        .clicked();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(format!("Isolating {name}")).truncate())
                            .on_hover_text(format!("Only {name} is shown"));
                    });
                });
            });
        });
    ui.add_space(4.0);
    end
}

/// What every layer row needs to know, worked out once a frame.
struct RowInfo<'a> {
    current: LayerId,
    has_selection: bool,
    counts: &'a std::collections::HashMap<LayerId, usize>,
    picked: &'a std::collections::BTreeSet<LayerId>,
    /// The picked object's layer, outlined (see [`LayerOnPick`]).
    shown: Option<LayerId>,
    isolated: Option<LayerId>,
    any_hidden: bool,
    /// The layer each setting is Alt-clicked onto, if any.
    solo_lock: Option<LayerId>,
    solo_snap: Option<LayerId>,
    /// The rows as listed (top down, filtered): what a Shift-click range runs over.
    list: &'a [LayerId],
}

impl DriftApp {
    /// The Layers panel: the filter, the isolate bar, then a compact row per layer (its
    /// stroke and fill swatches, name, line weight and type, printing,
    /// snapping, visibility and locking; the current one highlighted), each setting changed
    /// in place; then underlays.
    /// Only the rows in view are drawn, so a drawing with a thousand layers costs no more
    /// than one with ten.
    /// After a new pick, what Preferences → Layers says (drift-releases#19): show the picked
    /// object's layer in the Layers panel (scrolled to and outlined), and make it current if
    /// asked, unless it is locked or hidden. A pick on several layers with no active element
    /// does nothing. Run once a frame, after the canvas.
    pub(crate) fn follow_pick(&mut self, now: f64) {
        let pick = PickSignature::of(&self.drawing.editor);
        if self.drawing.layer_list.seen == Some(pick) {
            return;
        }
        self.drawing.layer_list.seen = Some(pick);
        let picked = self.drawing.editor.picked_layer();
        let layer = picked
            .layer
            .filter(|_| !pick.is_empty() && self.layer_on_pick != LayerOnPick::Nothing);
        self.drawing.layer_list.shown = layer;
        let Some(layer) = layer else {
            return;
        };
        self.drawing.layer_list.reveal = Some(layer);
        if self.layer_on_pick != LayerOnPick::MakeCurrent
            || layer == self.drawing.editor.doc.current_layer()
        {
            return;
        }
        let Some(l) = self.drawing.editor.doc.layer(layer).cloned() else {
            return;
        };
        if l.locked || !l.visible {
            let why = if l.locked { "locked" } else { "hidden" };
            self.set_status(
                format!("{} is {why}: the current layer stays", l.name),
                false,
                now,
            );
            return;
        }
        // Not an undo step, nor a change to save (the user's choice, 2026-11-07).
        let _ = self.drawing.editor.doc.set_current_layer_unrecorded(layer);
        if picked.layers > 1 {
            self.set_status(
                format!(
                    "Objects on {} layers picked: current layer set to {}, the last \
                     picked one's",
                    picked.layers, l.name
                ),
                false,
                now,
            );
        }
    }

    /// Make its layer current: the picked object's layer becomes the current one (an undo
    /// step, as a click on its row; a locked or hidden layer too), and the Layers panel
    /// opens to it. Under every Preferences → Layers setting (drift-releases#19).
    pub(crate) fn make_picked_layer_current(&mut self, now: f64) {
        let picked = self.drawing.editor.picked_layer();
        let Some(layer) = picked.layer else {
            let text = if picked.layers > 1 {
                format!(
                    "Objects on {} layers picked: pick one object to make its layer current",
                    picked.layers
                )
            } else {
                "Pick an object to make its layer current".to_owned()
            };
            self.set_status(text, true, now);
            return;
        };
        let Some(name) = self.drawing.editor.doc.layer(layer).map(|l| l.name.clone()) else {
            return;
        };
        self.drawing.editor.set_current_layer(layer);
        self.show_panel(crate::dock::Panel::Layers);
        let list = &mut self.drawing.layer_list;
        if !name
            .to_lowercase()
            .contains(&list.filter.trim().to_lowercase())
        {
            list.filter.clear();
        }
        list.reveal = Some(layer);
        list.shown = Some(layer);
        self.set_status(format!("Current layer: {name}"), false, now);
    }

    /// Leave the open drawing's colours for the colour pickers drawn this pass, wherever
    /// they are (worked out again only when the drawing has changed and a picker is open).
    /// Run every pass, before the panels.
    pub(crate) fn publish_document_colours(&mut self, ctx: &egui::Context) {
        self.drawing
            .document_colours
            .publish(ctx, &self.drawing.editor.doc);
    }

    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        self.end_stale_isolate();
        self.drawing.layer_counts.update(&self.drawing.editor.doc);
        let counts = std::mem::take(&mut self.drawing.layer_counts.curves);
        let layers = std::mem::take(&mut self.drawing.layer_counts.layers);
        let current = self.drawing.editor.doc.current_layer();
        let has_selection = !self.drawing.editor.selection.is_empty();
        // The layers the selection is on (its curves and points, block copies, texts and
        // fills).
        let ed = &self.drawing.editor;
        let mut picked: std::collections::BTreeSet<LayerId> = ed
            .selection
            .iter()
            .filter_map(|id| ed.doc.get(*id).map(|e| e.layer))
            .collect();
        picked.extend(
            ed.selected_instances
                .iter()
                .filter_map(|id| ed.doc.instance(*id).map(|i| i.layer)),
        );
        picked.extend(
            ed.selected_texts
                .iter()
                .filter_map(|id| ed.doc.text(*id).map(|t| t.layer)),
        );
        picked.extend(
            ed.selected_fills
                .iter()
                .filter_map(|id| ed.doc.fill(*id).map(|f| f.layer)),
        );
        // The rows, top down as the layers stack (the top one draws over the rest), that
        // match the filter.
        let needle = self.drawing.layer_list.filter.trim().to_lowercase();
        let mut total = 0;
        let mut any_hidden = false;
        let mut list: Vec<LayerId> = Vec::new();
        for (id, layer) in layers.iter().rev() {
            let id = *id;
            total += 1;
            any_hidden |= !layer.visible;
            if needle.is_empty() || layer.name.to_lowercase().contains(&needle) {
                list.push(id);
            }
        }
        let info = RowInfo {
            current,
            has_selection,
            counts: &counts,
            picked: &picked,
            shown: self.drawing.layer_list.shown,
            isolated: self.drawing.layer_list.isolated.as_ref().map(|i| i.layer),
            any_hidden,
            solo_lock: self.solo_of(LayerFlag::Locked),
            solo_snap: self.solo_of(LayerFlag::Snappable),
            list: &list,
        };
        // Rows the filter hides aren't picked, so a change made on a picked row goes only to
        // rows in sight (the current layer is always shown).
        if !self.drawing.layer_list.picked.is_empty() {
            let listed: std::collections::HashSet<LayerId> = list.iter().copied().collect();
            self.drawing
                .layer_list
                .picked
                .retain(|l| *l == current || listed.contains(l));
        }
        let mut status = None;
        let mut action = None;
        crate::theme::section(ui, &format!("Layers · {total}"));
        self.layer_filter(ui, list.len(), total);
        if let Some(iso) = &self.drawing.layer_list.isolated {
            let name = self
                .drawing
                .editor
                .doc
                .layer(iso.layer)
                .map_or(String::new(), |l| l.name.clone());
            if isolate_bar(ui, &name) {
                action = Some(LayerAction::EndIsolate);
            }
        }
        // The current layer stays in sight when the filter leaves it out.
        if !list.contains(&current) {
            ui.label(
                egui::RichText::new("Current layer (not matching the filter)")
                    .small()
                    .weak(),
            );
            self.layer_list_row(ui, current, &info, false, &mut status, &mut action);
            ui.separator();
        }
        let mut rows: Vec<(LayerId, egui::Rect)> = Vec::new();
        let area = egui::ScrollArea::vertical()
            .id_salt("layers_scroll")
            .auto_shrink([false, false]);
        let output = crate::theme::pane_scroll_viewport(ui, area, |ui, viewport| {
            // Each row's top, from the heights the rows had when last drawn.
            let closed = if self.drawing.layer_list.row > 0.0 {
                self.drawing.layer_list.row
            } else {
                crate::theme::LIST_ROW + ROW_GAP
            };
            let heights: Vec<f32> = vec![closed; list.len()];
            let mut tops = Vec::with_capacity(list.len() + 1);
            let mut y = 0.0;
            for h in &heights {
                tops.push(y);
                y += h;
            }
            tops.push(y);
            if let Some(i) = self
                .drawing
                .layer_list
                .reveal
                .take()
                .and_then(|r| pos_of(&list, r))
            {
                let top = ui.max_rect().top();
                let rect = egui::Rect::from_x_y_ranges(
                    ui.max_rect().x_range(),
                    top + tops[i]..=top + tops[i + 1],
                );
                ui.scroll_to_rect(rect, None);
            }
            // The rows in view, and one more each way.
            let first = tops
                .partition_point(|t| *t <= viewport.min.y)
                .saturating_sub(2);
            let last = tops
                .partition_point(|t| *t < viewport.max.y)
                .min(list.len());
            let first = first.min(last);
            ui.add_space(tops[first]);
            for (i, &id) in list.iter().enumerate().take(last).skip(first) {
                let top = ui.cursor().top();
                let rect = self.layer_list_row(ui, id, &info, true, &mut status, &mut action);
                ui.add_space(ROW_GAP);
                rows.push((id, rect));
                let drawn = ui.cursor().top() - top;
                let guess = heights[i];
                self.drawing.layer_list.row = drawn;
                if (drawn - guess).abs() > 0.5 {
                    // Laid out with a wrong height: again, with the right one.
                    ui.ctx().request_repaint();
                }
            }
            ui.add_space(tops[list.len()] - tops[last]);
            if list.is_empty() {
                ui.label(egui::RichText::new("No layer matches the filter").weak());
            }
            // Room to scroll above and below, for a row dragged to the list's ends.
            let room = (viewport.min.y > 0.0, viewport.max.y < tops[list.len()]);
            self.drag_layer(ui, &rows, room);
            // Below the rows, ids that don't depend on how many rows are drawn.
            let footer = egui::UiBuilder::new().id(egui::Id::new("layers_footer"));
            ui.scope_builder(footer, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("New layer")
                        .on_hover_text("A new layer, made current; type its name")
                        .clicked()
                    {
                        let n = self.drawing.editor.doc.layers().count();
                        let name = format!("Layer {n}");
                        let id = self.drawing.editor.add_layer(&name);
                        self.drawing.editor.set_current_layer(id);
                        self.drawing.renaming_layer = Some(id);
                        self.drawing.layer_names.insert(id, name);
                        // The new layer would hide behind a filter it doesn't match.
                        self.drawing.layer_list.filter.clear();
                        self.drawing.layer_list.reveal = Some(id);
                    }
                });
                self.underlays_section(ui);
            });
        });
        self.panel_scrolled(ui, crate::dock::Panel::Layers, &output);
        self.drawing.layer_counts.curves = counts;
        self.drawing.layer_counts.layers = layers;
        // Picked rows of layers that are gone go.
        let doc = &self.drawing.editor.doc;
        self.drawing
            .layer_list
            .picked
            .retain(|id| doc.layer(*id).is_some());
        match action {
            Some(LayerAction::Isolate(id)) => {
                let before = self.drawing.editor.isolate_layer(id);
                // Isolating another layer keeps the first isolate's "before".
                let before = match self.drawing.layer_list.isolated.take() {
                    Some(iso) => iso.before,
                    None => before,
                };
                self.drawing.layer_list.isolated = Some(Isolated { layer: id, before });
            }
            Some(LayerAction::EndIsolate) => {
                if let Some(iso) = self.drawing.layer_list.isolated.take() {
                    self.drawing
                        .editor
                        .set_layers_visible(&iso.before, "End isolate");
                }
            }
            Some(LayerAction::ShowAll) => {
                self.drawing.layer_list.isolated = None;
                let all: Vec<(LayerId, bool)> = self
                    .drawing
                    .editor
                    .doc
                    .layers()
                    .map(|(id, _)| (id, true))
                    .collect();
                self.drawing
                    .editor
                    .set_layers_visible(&all, "Show all layers");
            }
            Some(LayerAction::Solo(flag, id)) => self.toggle_solo(flag, id),
            None => {}
        }
        if let Some(text) = status {
            self.set_status(text, true, now);
        }
    }

    /// The layer a setting is Alt-clicked onto, if any.
    fn solo_of(&self, flag: LayerFlag) -> Option<LayerId> {
        self.drawing
            .layer_list
            .solos
            .iter()
            .find(|s| s.flag == flag)
            .map(|s| s.layer)
    }

    /// Alt-click on a layer's lock or snap toggle: set the other layers the other way, or,
    /// on the layer it was done on, put them back (one undo step each).
    pub(crate) fn toggle_solo(&mut self, flag: LayerFlag, id: LayerId) {
        let was = self
            .drawing
            .layer_list
            .solos
            .iter()
            .position(|s| s.flag == flag)
            .map(|i| self.drawing.layer_list.solos.remove(i));
        match was {
            Some(solo) if solo.layer == id => {
                self.drawing
                    .editor
                    .set_layers_flag(flag, &solo.before, flag.labels().1);
            }
            was => {
                let before = self.drawing.editor.solo_layer_flag(flag, id);
                // Done on another layer meanwhile: the first one's "before" stays.
                let before = was.map_or(before, |s| s.before);
                self.drawing.layer_list.solos.push(Solo {
                    flag,
                    layer: id,
                    before,
                });
            }
        }
    }

    /// Forget an isolate that is over: its layer deleted, or undone back to how the layers
    /// were before it. The same for lock and snap set on all but one layer.
    fn end_stale_isolate(&mut self) {
        let doc = &self.drawing.editor.doc;
        self.drawing.layer_list.solos.retain(|s| {
            let gone = doc.layer(s.layer).is_none();
            let undone = s
                .before
                .iter()
                .all(|(id, on)| doc.layer(*id).is_none_or(|l| s.flag.get(l) == *on));
            !gone && !undone
        });
        let Some(iso) = &self.drawing.layer_list.isolated else {
            return;
        };
        let doc = &self.drawing.editor.doc;
        let gone = doc.layer(iso.layer).is_none();
        let undone = iso
            .before
            .iter()
            .all(|(id, on)| doc.layer(*id).is_none_or(|l| l.visible == *on));
        if gone || undone {
            self.drawing.layer_list.isolated = None;
        }
    }

    /// The filter field: names containing the text (any case), how many match, and a
    /// button (or Escape) to clear it.
    fn layer_filter(&mut self, ui: &mut egui::Ui, matched: usize, total: usize) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let filter = &mut self.drawing.layer_list.filter;
                if !filter.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("{matched} of {total}"))
                            .small()
                            .weak(),
                    )
                    .on_hover_text("Layers matching the filter");
                    if crate::theme::delete_button(ui, "×", "Clear the filter").clicked() {
                        filter.clear();
                    }
                }
                let r = crate::theme::field(
                    ui,
                    egui::TextEdit::singleline(filter)
                        .id_salt("layer_filter")
                        .hint_text("Filter layers")
                        .desired_width(ui.available_width()),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    filter.clear();
                }
            });
        });
        ui.add_space(2.0);
    }

    /// A layer's row in its frame, with what a click, a drag and the context menu do.
    /// Returns its rect.
    fn layer_list_row(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        info: &RowInfo,
        draggable: bool,
        status: &mut Option<String>,
        action: &mut Option<LayerAction>,
    ) -> egui::Rect {
        let Some(layer) = self.drawing.editor.doc.layer(id).cloned() else {
            return egui::Rect::NOTHING;
        };
        let accent = crate::theme::accent(ui.visuals().dark_mode);
        let mut edited = layer.clone();
        let mut touched = Touched::default();
        let is_current = id == info.current;
        let picked_row = self.drawing.layer_list.picked.contains(&id);
        // The whole row: a click makes the layer current, a drag moves it in the order (its
        // own buttons and fields take their clicks first).
        let sense = if draggable {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        // (Filled in once it is known whether the pointer is on the row.)
        let backdrop = ui.painter().add(egui::Shape::Noop);
        let row = ui.scope_builder(
            // An id of its own: which rows are drawn changes as the list scrolls.
            egui::UiBuilder::new()
                .id(egui::Id::new(("layer_row", id, draggable)))
                .sense(sense),
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        if let Some(a) =
                            self.layer_row(ui, id, &layer, &mut edited, &mut touched, info)
                        {
                            *action = Some(a);
                        }
                    });
            },
        );
        let row = row.response;
        if row.clicked() {
            let mods = ui.input(|i| i.modifiers);
            self.layer_row_clicked(id, mods, info.list);
        }
        if row.drag_started() {
            self.drawing.layer_drag = Some(id);
        }
        // The current layer on the accent's tint; an opened one, and the one under the
        // pointer, on a quiet fill.
        let radius = crate::theme::radius::CONTROL;
        let dark = ui.visuals().dark_mode;
        let hovered = self.drawing.layer_drag.is_none() && row.hovered();
        let header = egui::Rect::from_min_size(
            row.rect.min,
            egui::vec2(row.rect.width(), crate::theme::LIST_ROW),
        );
        let quiet = crate::theme::row_fill(dark, false, hovered);
        let tint = crate::theme::row_fill(dark, is_current, false);
        let mut fills = Vec::new();
        if picked_row {
            // Picked among several: a quiet fill and the accent down its left edge (the
            // current layer keeps the accent's tint, the picked object's an outline).
            let fill =
                crate::theme::row_fill(dark, false, true).unwrap_or(egui::Color32::TRANSPARENT);
            fills.push(egui::Shape::rect_filled(row.rect, radius, fill));
            let bar = egui::Rect::from_min_size(row.rect.min, egui::vec2(3.0, row.rect.height()));
            fills.push(egui::Shape::rect_filled(bar, 1.5, accent));
        } else if let Some(fill) = quiet {
            fills.push(egui::Shape::rect_filled(row.rect, radius, fill));
        }
        if let Some(fill) = tint {
            fills.push(egui::Shape::rect_filled(header, radius, fill));
        }
        ui.painter().set(backdrop, egui::Shape::Vec(fills));
        if info.shown == Some(id) {
            ui.painter().rect_stroke(
                row.rect,
                radius,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Inside,
            );
        }
        let rect = row.rect;
        // (No tooltip of the row's own: its name, swatches, line cell and switches each have
        // theirs, and a second one would sit over them.)
        row.context_menu(|ui| {
            if ui.button("Make current").clicked() {
                self.drawing.editor.set_current_layer(id);
                ui.close();
            }
            if ui
                .add_enabled(can_rename(id, &layer), egui::Button::new("Rename"))
                .clicked()
            {
                self.drawing.renaming_layer = Some(id);
                ui.close();
            }
            if ui.button("Select all on this layer").clicked() {
                self.drawing.editor.select_layer(id);
                ui.close();
            }
            if ui
                .add_enabled(
                    info.has_selection,
                    egui::Button::new("Move selection to this layer"),
                )
                .clicked()
            {
                self.drawing.editor.move_selection_to_layer(id);
                ui.close();
            }
            ui.separator();
            if info.isolated == Some(id) {
                if ui
                    .button("End isolate")
                    .on_hover_text("Show the other layers again, as they were")
                    .clicked()
                {
                    *action = Some(LayerAction::EndIsolate);
                    ui.close();
                }
            } else if ui
                .button("Isolate")
                .on_hover_text("Show this layer alone (Alt-click its eye)")
                .clicked()
            {
                *action = Some(LayerAction::Isolate(id));
                ui.close();
            }
            if ui
                .add_enabled(info.any_hidden, egui::Button::new("Show all layers"))
                .clicked()
            {
                *action = Some(LayerAction::ShowAll);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(id != LayerId::DEFAULT, egui::Button::new("Delete layer"))
                .clicked()
            {
                if let Err(e) = self.drawing.editor.remove_layer(id) {
                    *status = Some(e);
                }
                ui.close();
            }
        });
        if (edited != layer || touched.any()) && self.drawing.editor.doc.layer(id).is_some() {
            self.change_layers(id, edited, touched);
        }
        rect
    }

    /// Row `id` is now `now`, its `touched` settings picked in its controls: with it among
    /// several picked rows, those settings (colours, weight, line type, printing) go to all
    /// of them, one undo step, even where a pick left this row as it was; otherwise the
    /// change goes to it alone.
    fn change_layers(&mut self, id: LayerId, now: Layer, touched: Touched) {
        let picked = &self.drawing.layer_list.picked;
        if !(picked.len() > 1 && picked.contains(&id)) {
            self.drawing.editor.update_layer(id, now);
            return;
        }
        let doc = &self.drawing.editor.doc;
        let changes: Vec<(LayerId, Layer)> = picked
            .iter()
            .filter_map(|&other| {
                if other == id {
                    return Some((id, now.clone()));
                }
                let mut l = doc.layer(other)?.clone();
                if touched.color {
                    l.color = now.color;
                }
                if touched.fill {
                    l.fill_color = now.fill_color;
                }
                if touched.weight {
                    l.weight = now.weight;
                }
                if touched.line_type {
                    l.line_type = now.line_type;
                }
                if touched.printable {
                    l.printable = now.printable;
                }
                Some((other, l))
            })
            .collect();
        self.drawing.editor.update_layers(&changes, "Change layers");
    }

    /// A click on a layer's row or name: Ctrl (Cmd) picks it or lets it go among several,
    /// Shift picks the rows from the last one clicked to it; a plain click lets the picked
    /// rows go and makes it the current layer.
    fn layer_row_clicked(&mut self, id: LayerId, mods: egui::Modifiers, list: &[LayerId]) {
        let current = self.drawing.editor.doc.current_layer();
        let rows = &mut self.drawing.layer_list;
        if mods.command {
            // The current layer counts as picked when the first other one is added.
            if rows.picked.is_empty() && id != current {
                rows.picked.insert(current);
            }
            if !rows.picked.remove(&id) {
                rows.picked.insert(id);
            }
            rows.anchor = Some(id);
        } else if mods.shift {
            let from = rows.anchor.unwrap_or(current);
            if let (Some(a), Some(b)) = (pos_of(list, from), pos_of(list, id)) {
                rows.picked = list[a.min(b)..=a.max(b)].iter().copied().collect();
            } else {
                // From a row the filter hides: the run starts at the row clicked.
                rows.picked = std::iter::once(id).collect();
                rows.anchor = Some(id);
            }
        } else {
            rows.picked.clear();
            rows.anchor = Some(id);
            if id != current {
                self.drawing.editor.set_current_layer(id);
            }
        }
    }

    /// A layer's row (see [`DOT`], [`SWATCH`] and [`ROW_SPACING`] for its grid): the
    /// current layer's dot, the stroke and fill swatches (a click opens the colour picker),
    /// the name (click: make current; double-click: rename), the selection marker, curve
    /// count, the line cell (weight and line type; a click opens their menu), and the
    /// printing, snapping, visibility and lock toggles. Alt-click on the eye isolates the
    /// layer (or ends the isolate).
    fn layer_row(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        layer: &Layer,
        edited: &mut Layer,
        touched: &mut Touched,
        info: &RowInfo,
    ) -> Option<LayerAction> {
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::accent(dark);
        let is_current = id == info.current;
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = ROW_SPACING;
            // The active (current) layer's dot; a faint ring on the others.
            let (dot, dot_response) =
                ui.allocate_exact_size(egui::vec2(DOT, ICON_TOGGLE), egui::Sense::hover());
            if is_current {
                ui.painter().circle_filled(dot.center(), 4.0, accent);
                dot_response.on_hover_text("The current layer: what you draw goes on it");
            } else {
                ui.painter().circle_stroke(
                    dot.center(),
                    3.5,
                    egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.6)),
                );
            }
            // Stroke and fill, each a swatch that opens the picker.
            ui.spacing_mut().item_spacing.x = 3.0;
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::click());
            paint_swatch(ui.painter(), r, edited.color, Unset::Ink, None, dark);
            hover_ring(ui, r, resp.hovered());
            let stroke_value = edited.color.map_or("Ink".to_owned(), crate::colour::hex);
            let tip = match edited.color {
                Some(c) => format!("Stroke {} · click to change", crate::colour::hex(c)),
                None => format!("Stroke: {} · click to change", crate::colour::ink_tip(dark)),
            };
            let resp = resp.on_hover_text(tip);
            let label = format!("{} stroke color", layer.name);
            resp.widget_info(|| {
                crate::colour::labeled_with_value(
                    egui::WidgetType::ColorButton,
                    &label,
                    &stroke_value,
                )
            });
            if let Some(c) =
                crate::colour::colour_popup(ui, &resp, edited.color, Unset::Ink, None, dark)
            {
                edited.color = c;
                touched.color = true;
            }
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::click());
            let stroke_shown = crate::colour::Follows::of(edited.color);
            let unset = Unset::Named("Match stroke");
            paint_swatch(
                ui.painter(),
                r,
                edited.fill_color,
                unset,
                Some(stroke_shown),
                dark,
            );
            hover_ring(ui, r, resp.hovered());
            let fill_value = edited
                .fill_color
                .map_or("Match stroke".to_owned(), crate::colour::hex);
            let tip = match edited.fill_color {
                Some(c) => format!(
                    "Fill {} (fills and hatches on this layer) · click to change",
                    crate::colour::hex(c)
                ),
                None => "Fill: matches the stroke (fills and hatches on this layer) · click to \
                         change"
                    .to_owned(),
            };
            let resp = resp.on_hover_text(tip);
            let label = format!("{} fill color", layer.name);
            resp.widget_info(|| {
                crate::colour::labeled_with_value(
                    egui::WidgetType::ColorButton,
                    &label,
                    &fill_value,
                )
            });
            if let Some(c) = crate::colour::colour_popup(
                ui,
                &resp,
                edited.fill_color,
                unset,
                Some(stroke_shown),
                dark,
            ) {
                edited.fill_color = c;
                touched.fill = true;
            }
            ui.spacing_mut().item_spacing.x = ROW_SPACING;
            // Controls from the right, so the name takes the space left.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut locked = edited.locked;
                let glyph = if locked { Glyph::Lock } else { Glyph::Unlock };
                let tip = if info.solo_lock == Some(id) {
                    "Lock or unlock (locked geometry can be snapped to but not edited) · \
                     Alt-click: put the other layers' locks back"
                } else {
                    "Lock or unlock (locked geometry can be snapped to but not edited) · \
                     Alt-click: lock every other layer"
                };
                if icon_toggle(ui, &mut locked, glyph, false, tip) && ui.input(|i| i.modifiers.alt)
                {
                    locked = edited.locked;
                    action = Some(LayerAction::Solo(LayerFlag::Locked, id));
                }
                edited.locked = locked;
                let visible = edited.visible;
                let tip = "Show or hide · Alt-click: show this layer alone";
                if icon_toggle(ui, &mut edited.visible, Glyph::Eye, true, tip)
                    && ui.input(|i| i.modifiers.alt)
                {
                    edited.visible = visible;
                    action = Some(if info.isolated == Some(id) {
                        LayerAction::EndIsolate
                    } else {
                        LayerAction::Isolate(id)
                    });
                }
                let snappable = edited.snappable;
                let tip = if info.solo_snap == Some(id) {
                    "Snap to this layer's geometry (off: it can still be picked and edited) · \
                     Alt-click: put the other layers' snapping back"
                } else {
                    "Snap to this layer's geometry (off: it can still be picked and edited) · \
                     Alt-click: snap to this layer only"
                };
                if icon_toggle(ui, &mut edited.snappable, Glyph::Snap, true, tip)
                    && ui.input(|i| i.modifiers.alt)
                {
                    edited.snappable = snappable;
                    action = Some(LayerAction::Solo(LayerFlag::Snappable, id));
                }
                let tip = "Printed and exported (off: shown here, left out of prints and exports)";
                touched.printable |=
                    icon_toggle(ui, &mut edited.printable, Glyph::Print, true, tip);
                ui.add_space(4.0);
                line_cell(ui, id, &layer.name, edited, touched);
                ui.add_space(4.0);
                if info.picked.contains(&id) {
                    ui.add_space(5.0);
                    let (mark, response) =
                        ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    let r = mark.shrink(0.5);
                    ui.painter().rect(
                        r,
                        1.5,
                        accent.gamma_multiply(0.25),
                        egui::Stroke::new(1.2, accent),
                        egui::StrokeKind::Inside,
                    );
                    response.on_hover_text("Some of the selection is on this layer");
                }
                ui.add_space(4.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.layer_name(ui, id, layer, edited, info);
                });
            });
        });
        action
    }

    /// A layer row being dragged: a line where it would go, and on release, the move. Only
    /// the rows in view are in `rows` (top down, perhaps filtered); dragging near the top or
    /// bottom of the list scrolls it.
    fn drag_layer(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[(LayerId, egui::Rect)],
        (up, down_room): (bool, bool),
    ) {
        let Some(dragged) = self.drawing.layer_drag else {
            return;
        };
        let (Some(pointer), down) =
            ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()))
        else {
            self.drawing.layer_drag = None;
            return;
        };
        // The gap the pointer is nearest: before row `slot` (rows top down).
        let slot = rows
            .iter()
            .position(|(_, r)| pointer.y < r.center().y)
            .unwrap_or(rows.len());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let view = ui.clip_rect();
        if down {
            let edge = 24.0;
            let step = if up && pointer.y < view.top() + edge {
                8.0
            } else if down_room && pointer.y > view.bottom() - edge {
                -8.0
            } else {
                0.0
            };
            if step != 0.0 {
                ui.scroll_with_delta(egui::vec2(0.0, step));
                ui.ctx().request_repaint();
            }
        }
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            let y = rows
                .get(slot)
                .map_or(last.1.bottom() + 1.0, |(_, r)| r.top() - 1.0);
            let accent = crate::theme::accent(ui.visuals().dark_mode);
            ui.painter().line_segment(
                [
                    egui::pos2(first.1.left(), y),
                    egui::pos2(first.1.right(), y),
                ],
                egui::Stroke::new(2.0, accent),
            );
        }
        if down {
            return;
        }
        self.drawing.layer_drag = None;
        // The dragged layer goes just above the row after the gap, or just below the row
        // before it, in the whole list (top down) without the dragged layer.
        let mut order: Vec<LayerId> = self
            .drawing
            .editor
            .doc
            .layers()
            .map(|(id, _)| id)
            .filter(|id| *id != dragged)
            .collect();
        order.reverse();
        let pos = |order: &[LayerId], id: LayerId| order.iter().position(|l| *l == id);
        let after = rows[slot..].iter().map(|r| r.0).find(|id| *id != dragged);
        let before = rows[..slot]
            .iter()
            .rev()
            .map(|r| r.0)
            .find(|id| *id != dragged);
        let at = match (after, before) {
            (Some(a), _) => pos(&order, a),
            (None, Some(b)) => pos(&order, b).map(|i| i + 1),
            (None, None) => None,
        };
        let Some(at) = at else {
            return;
        };
        order.insert(at, dragged);
        let index = order.len() - 1 - at;
        let now = self
            .drawing
            .editor
            .doc
            .layers()
            .position(|(id, _)| id == dragged);
        if now != Some(index) {
            self.drawing.editor.reorder_layer(dragged, index);
        }
    }

    /// A layer's name, or the field renaming it.
    fn layer_name(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        layer: &Layer,
        edited: &mut Layer,
        info: &RowInfo,
    ) {
        let is_current = id == info.current;
        let curves = info.counts.get(&id).copied().unwrap_or(0);
        if self.drawing.renaming_layer == Some(id) && can_rename(id, layer) {
            let name = self
                .drawing
                .layer_names
                .entry(id)
                .or_insert_with(|| layer.name.clone());
            let r = crate::theme::field(
                ui,
                egui::TextEdit::singleline(name).desired_width(ui.available_width()),
            );
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if !cancel && !name.trim().is_empty() {
                    edited.name = name.trim().to_owned();
                }
                self.drawing.renaming_layer = None;
                self.drawing.layer_names.remove(&id);
            }
            return;
        }
        // Long names lose their middle, not their distinguishing end; the tooltip has it all.
        let mut font = egui::TextStyle::Body.resolve(ui.style());
        if is_current {
            font.family = crate::theme::bold();
        }
        let shown = elide_middle(ui, &layer.name, &font, ui.available_width());
        let name = egui::RichText::new(shown).font(font);
        let name = if is_current || layer.visible {
            name
        } else {
            name.weak()
        };
        let r = ui
            .add(
                egui::Label::new(name)
                    .sense(egui::Sense::click())
                    .truncate(),
            )
            .on_hover_text(format!(
                "{} · {curves} {}\n{}",
                layer.name,
                if curves == 1 { "curve" } else { "curves" },
                if is_current {
                    "The current layer: new geometry goes here. Double-click to rename · \
                     Ctrl- or Shift-click to pick several and change them together · drag \
                     to reorder (top draws on top)"
                } else {
                    "Click to make this the current layer; double-click to rename · Ctrl- or \
                     Shift-click to pick several and change them together · drag to reorder \
                     (top draws on top)"
                }
            ));
        if r.clicked() {
            let mods = ui.input(|i| i.modifiers);
            self.layer_row_clicked(id, mods, info.list);
        }
        if r.double_clicked() && can_rename(id, layer) {
            self.drawing.renaming_layer = Some(id);
        }
    }

    /// Appearance controls for the selected curves (in the Properties tab; their layer is
    /// the Inspector's first row, `inspector_layer_row`).
    pub(crate) fn appearance_panel(&mut self, ui: &mut egui::Ui) {
        let curves: Vec<EntityId> = self
            .drawing
            .editor
            .selection
            .iter()
            .copied()
            .filter(|id| {
                self.drawing
                    .editor
                    .doc
                    .get(*id)
                    .is_some_and(|e| !e.kind.is_point())
            })
            .collect();
        if curves.is_empty() {
            return;
        }
        ui.add_space(2.0);
        self.stroke_rows(ui, &curves, "selection");
    }

    /// Colour, weight and line type rows for `curves`' own
    /// appearance, shown as the first's; `salt` keeps the combos' ids apart.
    pub(crate) fn stroke_rows(&mut self, ui: &mut egui::Ui, curves: &[EntityId], salt: &str) {
        let Some(first) = curves
            .first()
            .and_then(|id| self.drawing.editor.doc.get(*id))
            .copied()
        else {
            return;
        };
        let dark = ui.visuals().dark_mode;
        let a = first.appearance;
        let by_layer = by_layer_follows(&self.drawing.editor.doc, curves);
        crate::theme::row(ui, "Color", |ui| {
            ui.push_id((salt, "colour"), |ui| {
                let width = ui.available_width();
                let unset = Unset::Named("By layer");
                if let Some(c) = crate::colour::colour_button(
                    ui,
                    "Stroke color",
                    a.color,
                    unset,
                    by_layer,
                    dark,
                    width,
                ) {
                    self.drawing
                        .editor
                        .update_appearance(curves, |x| x.color = c);
                }
            });
        });
        crate::theme::row(ui, "Weight", |ui| {
            let mut w = a.weight;
            if weight_combo(ui, (salt, "weight"), &mut w, true) {
                self.drawing
                    .editor
                    .update_appearance(curves, |x| x.weight = w);
            }
        });
        crate::theme::row(ui, "Line type", |ui| {
            let mut t = a.line_type;
            let width = ui.available_width();
            if line_type_combo(ui, (salt, "type"), &mut t, true, width) {
                self.drawing
                    .editor
                    .update_appearance(curves, |x| x.line_type = t);
            }
        });
    }
}

/// A quiet ring round a swatch under the pointer.
fn hover_ring(ui: &egui::Ui, r: egui::Rect, hovered: bool) {
    if hovered {
        ui.painter().rect_stroke(
            r.expand(1.5),
            4,
            egui::Stroke::new(1.0, ui.visuals().strong_text_color().gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
}

/// Draw a short sample of a line from `a` to `b`: as thick as `weight` reads on screen, and
/// dashed as `line_type` is. A dot as long as the line is thick is drawn round.
fn paint_line_sample(
    painter: &egui::Painter,
    a: egui::Pos2,
    b: egui::Pos2,
    weight: LineWeight,
    line_type: LineType,
    color: egui::Color32,
) {
    let length = (b - a).length();
    let dir = (b - a) / length.max(1e-3);
    let sample = dash_runs(line_type, length, sample_width(weight));
    let stroke = egui::Stroke::new(sample.width, color);
    for (from, to) in sample.runs {
        if sample.round_dots && to - from <= sample.width + 1e-3 {
            painter.circle_filled(a + dir * (from + to) / 2.0, sample.width / 2.0, color);
        } else {
            painter.line_segment([a + dir * from, a + dir * to], stroke);
        }
    }
}

/// What "By layer" shows as for `curves`: their layers' stroke (Ink too) when they all have
/// the same one; unknown (`None`) when they differ.
fn by_layer_follows(
    doc: &drift_core::Document,
    curves: &[EntityId],
) -> Option<crate::colour::Follows> {
    let mut follows = curves
        .iter()
        .filter_map(|id| doc.get(*id))
        .filter_map(|e| doc.layer(e.layer))
        .map(|l| crate::colour::Follows::of(l.color));
    let first = follows.next()?;
    follows.all(|f| f == first).then_some(first)
}

/// What a fill's or hatch's "By layer" shows as on `layers`: their fill colour (which
/// matches the stroke, or Ink, until given its own) when they all have the same one;
/// unknown (`None`) when they differ.
pub(crate) fn fill_by_layer_follows(
    doc: &drift_core::Document,
    layers: impl IntoIterator<Item = LayerId>,
) -> Option<Follows> {
    let mut follows = layers
        .into_iter()
        .filter_map(|id| doc.layer(id))
        .map(|l| Follows::of(l.fill_colour()));
    let first = follows.next()?;
    follows.all(|f| f == first).then_some(first)
}

/// How thick a line of `weight` is drawn in a sample: from a hairline at 0.13 mm to 5 points
/// at 2.00 mm, every ISO weight a different width.
fn sample_width(weight: LineWeight) -> f32 {
    let mm = (weight.mm() as f32).max(0.13);
    (0.8 * (mm / 0.13).powf(0.67)).clamp(0.8, 5.0)
}

/// A line sample: the runs drawn and how thick.
#[derive(Debug, PartialEq)]
struct Sample {
    /// (start, end) of each run, in points along the sample.
    runs: Vec<(f32, f32)>,
    /// How thick it is drawn: as asked, or thinner when its dots wouldn't fit otherwise.
    width: f32,
    /// Its dots are as long as it is thick, and drawn round.
    round_dots: bool,
}

/// The drawn runs of a `line_type` sample `length` points long and `width` thick, flush with
/// both ends: its pattern at a size that reads in a row (a dash of 4 paper units as 8
/// points). A pattern of a long dash and dots (centre, phantom) shows as the end of one long
/// dash, its dots, and the start of the next: every dot and gap at least [`MIN_MARK`] and at
/// least as long as the line is thick, so a dot reads as a dot, not a bar, and each long
/// dash at least twice a dot. Where that can't fit, the dots shrink, then the line is drawn
/// thinner (its weight is shown beside it in numbers).
fn dash_runs(line_type: LineType, length: f32, width: f32) -> Sample {
    let dashes = line_type.paper_dashes();
    let mut sample = Sample {
        runs: Vec::new(),
        width,
        round_dots: false,
    };
    if dashes.is_empty() {
        sample.runs.push((0.0, length));
        return sample;
    }
    let scale = 2.0;
    let runs = &mut sample.runs;
    if dashes.len() > 2 {
        let dots = &dashes[1..];
        let group = dots.iter().sum::<f64>() as f32;
        let shortest = dots.iter().copied().fold(f64::INFINITY, f64::min) as f32;
        // The shortest dot or gap: the pattern's own size, or as long as the line is thick,
        // but no longer than leaves each long dash twice as long.
        let fit = length / (group / shortest + 4.0);
        let mark = (shortest * scale).max(width).min(fit).max(MIN_MARK);
        sample.width = width.min(mark);
        sample.round_dots = sample.width >= mark - 1e-3;
        let scale = mark / shortest;
        let stub = ((length - group * scale) / 2.0).max(0.0);
        runs.push((0.0, stub));
        let mut t = stub;
        // Gap, dot, gap, … after the long dash.
        for (k, d) in dots.iter().enumerate() {
            let end = t + *d as f32 * scale;
            if k % 2 == 1 {
                runs.push((t, end));
            }
            t = end;
        }
        runs.push((t.min(length), length));
        return sample;
    }
    // A dash and a gap, repeated: as many whole dashes as come nearest to fitting, stretched
    // or squeezed a little to end flush on a dash, never cut short to a sliver.
    let (dash, gap) = (dashes[0] as f32 * scale, dashes[1] as f32 * scale);
    let count = ((length + gap) / (dash + gap)).round().max(1.0);
    let scale = length / (count * dash + (count - 1.0) * gap);
    let (dash, gap) = (dash * scale, gap * scale);
    for k in 0..count as usize {
        let from = k as f32 * (dash + gap);
        runs.push((from, (from + dash).min(length)));
    }
    sample
}

/// A layer's weight and line type cell: a sample of its line and its weight; a click opens
/// a menu of weights and line types, each shown as it draws. A pick there is marked in
/// `touched`.
fn line_cell(
    ui: &mut egui::Ui,
    id: LayerId,
    name: &str,
    edited: &mut Layer,
    touched: &mut Touched,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(LINE_CELL, ICON_TOGGLE), egui::Sense::click());
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color().gamma_multiply(0.8)
    };
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(0.0, 2.0)),
            crate::theme::radius::CONTROL,
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    let y = rect.center().y;
    paint_line_sample(
        ui.painter(),
        egui::pos2(rect.left() + 3.0, y),
        egui::pos2(rect.left() + 3.0 + ROW_SAMPLE, y),
        edited.weight,
        edited.line_type,
        color,
    );
    let weight = if edited.weight.0 == 0 {
        "Hair".to_owned()
    } else {
        format!("{:.2}", edited.weight.mm())
    };
    ui.painter().text(
        egui::pos2(rect.right() - 3.0, y),
        egui::Align2::RIGHT_CENTER,
        weight,
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
    let value = format!(
        "{} · {}",
        edited.weight.label(),
        crate::theme::us(edited.line_type.label())
    );
    let response = response.on_hover_text(format!("{value} · click to change"));
    let label = format!("{name} line weight and type");
    response.widget_info(|| {
        crate::colour::labeled_with_value(egui::WidgetType::Button, &label, &value)
    });
    let text = ui.visuals().text_color();
    egui::Popup::from_toggle_button_response(&response)
        .id(egui::Id::new(("layer_line_menu", id.0)))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(170.0)
        .show(|ui| {
            let row = |ui: &mut egui::Ui, on: bool, w: LineWeight, t: LineType, label: String| {
                let (r, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 20.0),
                    egui::Sense::click(),
                );
                if let Some(fill) =
                    crate::theme::row_fill(ui.visuals().dark_mode, on, resp.hovered())
                {
                    ui.painter()
                        .rect_filled(r, crate::theme::radius::CONTROL, fill);
                }
                let y = r.center().y;
                paint_line_sample(
                    ui.painter(),
                    egui::pos2(r.left() + 6.0, y),
                    egui::pos2(r.left() + 46.0, y),
                    w,
                    t,
                    text,
                );
                ui.painter().text(
                    egui::pos2(r.left() + 56.0, y),
                    egui::Align2::LEFT_CENTER,
                    &label,
                    egui::TextStyle::Body.resolve(ui.style()),
                    text,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label)
                });
                resp.clicked()
            };
            ui.label(egui::RichText::new("Weight").small().weak());
            for w in LineWeight::ISO {
                if row(ui, edited.weight == w, w, LineType::Continuous, w.label()) {
                    edited.weight = w;
                    touched.weight = true;
                    ui.close();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Line type").small().weak());
            for t in LineType::ALL {
                let label = crate::theme::us(t.label()).to_string();
                if row(ui, edited.line_type == t, LineWeight(25), t, label) {
                    edited.line_type = t;
                    touched.line_type = true;
                    ui.close();
                }
            }
        });
}

/// Whether a layer's name can be changed: any but a first layer named "0" (the layer DWG and
/// DXF files always have, which keeps its name as it does there). A new drawing's first
/// layer, "Layer 1", is renamed like any other (#423).
fn can_rename(id: LayerId, layer: &Layer) -> bool {
    id != LayerId::DEFAULT || layer.name != Layer::CAD_ZERO
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colour::Follows;

    #[test]
    fn by_layer_shows_as_the_layers_stroke_ink_too_or_unknown_when_they_differ() {
        let mut doc = drift_core::Document::new();
        let mut layer = |c: Option<Rgb>| {
            let mut l = Layer::new("L");
            l.color = c;
            doc.add_layer(l)
        };
        let (ink, red, blue) = (
            layer(None),
            layer(Some(Rgb([0xff, 0, 0]))),
            layer(Some(Rgb([0, 0, 0xff]))),
        );
        let mut curve_on = |layer: LayerId| {
            let a = doc.add_point(glam::DVec2::ZERO);
            let b = doc.add_point(glam::DVec2::X);
            let l = doc.add_line(a, b).unwrap();
            doc.set_entity_layer(l, layer).unwrap();
            l
        };
        let (on_ink, on_red, on_red2, on_blue) =
            (curve_on(ink), curve_on(red), curve_on(red), curve_on(blue));
        assert_eq!(by_layer_follows(&doc, &[on_ink]), Some(Follows::Ink));
        assert_eq!(
            by_layer_follows(&doc, &[on_red, on_red2]),
            Some(Follows::Colour(Rgb([0xff, 0, 0])))
        );
        assert_eq!(by_layer_follows(&doc, &[on_red, on_blue]), None);
        assert_eq!(by_layer_follows(&doc, &[on_ink, on_red]), None);
    }

    /// Accessibility events of one frame.
    fn clicked_events(h: &egui_kittest::Harness<'_, bool>) -> Vec<egui::WidgetInfo> {
        h.output()
            .platform_output
            .events
            .iter()
            .filter_map(|e| match e {
                egui::output::OutputEvent::Clicked(info) => Some(info.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_toggle_says_it_was_clicked_once_with_its_new_state() {
        use egui_kittest::kittest::Queryable as _;
        let mut h = egui_kittest::Harness::builder()
            .with_size([200.0, 100.0])
            .build_ui_state(
                |ui, on: &mut bool| {
                    icon_toggle(ui, on, Glyph::Eye, true, "Show or hide");
                },
                false,
            );
        h.run();
        let at = h.get_by_label("Show or hide").rect().center();
        h.hover_at(at);
        h.run();
        let mut events = Vec::new();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
            events.extend(clicked_events(&h));
        }
        assert!(*h.state(), "switched on");
        assert_eq!(events.len(), 1, "one click said: {events:?}");
        assert_eq!(events[0].selected, Some(true), "said as it is now");
    }

    #[test]
    fn every_line_type_and_weight_looks_its_own_in_a_row() {
        let widths: Vec<f32> = LineWeight::ISO.iter().map(|w| sample_width(*w)).collect();
        // In a row, every line type draws its own, at every weight.
        for w in &widths {
            let runs: Vec<Vec<(f32, f32)>> = LineType::ALL
                .iter()
                .map(|t| dash_runs(*t, ROW_SAMPLE, *w).runs)
                .collect();
            for (i, a) in runs.iter().enumerate() {
                for (j, b) in runs.iter().enumerate().skip(i + 1) {
                    assert_ne!(
                        a,
                        b,
                        "{:?} and {:?} draw alike at {w}",
                        LineType::ALL[i],
                        LineType::ALL[j]
                    );
                }
            }
        }
        let len = |(a, b): (f32, f32)| b - a;
        for t in LineType::ALL {
            for length in [ROW_SAMPLE, 40.0] {
                for w in &widths {
                    let sample = dash_runs(t, length, *w);
                    let r = &sample.runs;
                    // Starts and ends flush.
                    assert_eq!(r[0].0, 0.0, "{t:?} at {length} starts flush");
                    let end = r.last().unwrap().1;
                    assert!((end - length).abs() < 1e-3, "{t:?} at {length} ends flush");
                    // Every gap stays clear of egui's feathered edges.
                    for pair in r.windows(2) {
                        let gap = pair[1].0 - pair[0].1;
                        assert!(gap >= MIN_MARK - 1e-3, "{t:?} at {length}, {w}: {r:?}");
                    }
                    assert!(sample.width <= *w);
                }
            }
        }
        // Centre and phantom read as a long dash, their dots (one, two), and a long dash:
        // each dot at least two pixels and as long as the line is thick (a dot, not a bar),
        // the long dashes at least twice a dot.
        for (t, dots) in [(LineType::Center, 1), (LineType::Phantom, 2)] {
            for length in [ROW_SAMPLE, 40.0] {
                for w in &widths {
                    let sample = dash_runs(t, length, *w);
                    let r = &sample.runs;
                    assert_eq!(r.len(), dots + 2, "{t:?} at {length}: {r:?}");
                    let (first, last) = (len(r[0]), len(r[r.len() - 1]));
                    for dot in &r[1..r.len() - 1] {
                        let dot = len(*dot);
                        assert!(dot >= MIN_MARK - 1e-3, "{t:?} at {length}, {w}: {r:?}");
                        assert!(dot >= sample.width - 1e-3, "{t:?}: a dot, not a bar");
                        assert!(first >= 2.0 * dot - 1e-3 && last >= 2.0 * dot - 1e-3);
                    }
                }
                // The usual weights (up to 0.50 mm) keep their width.
                let usual = sample_width(LineWeight::ISO[5]);
                for w in widths.iter().filter(|w| **w <= usual) {
                    assert_eq!(dash_runs(t, length, *w).width, *w, "{t:?} at {length}");
                }
            }
        }
        // (Hair draws as the thinnest, 0.13.)
        let widths: Vec<f32> = LineWeight::ISO[1..]
            .iter()
            .map(|w| sample_width(*w))
            .collect();
        assert!(
            widths.windows(2).all(|w| w[1] > w[0] + 0.15),
            "each heavier weight draws thicker: {widths:?}"
        );
    }
}
