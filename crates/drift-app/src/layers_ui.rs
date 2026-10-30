//! Layers: editor commands (each one undo step), the Layers tab, and the layer / appearance
//! controls for the selection in the Properties tab.

use drift_core::{EntityId, Layer, LayerId, LineType, LineWeight, Rgb};
use eframe::egui;

use crate::DriftApp;
use crate::icons::{self, Glyph};
use drift_edit::editor::LayerFlag;

/// The filter field shows once a drawing has more layers than this.
const FILTER_FROM: usize = 8;

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
    /// The height of a closed row, with the space after it, as last drawn.
    row: f32,
    /// The same for opened rows, which are taller.
    opened: std::collections::HashMap<LayerId, f32>,
    /// A row to scroll into view.
    reveal: Option<LayerId>,
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

/// A small icon toggle: full colour when on, faint (and struck through for `strike`) when off.
/// True when it was clicked.
pub(crate) fn icon_toggle(
    ui: &mut egui::Ui,
    on: &mut bool,
    glyph: Glyph,
    strike: bool,
    tip: &str,
) -> bool {
    let r = icons::button(ui, 24.0, false, tip, |painter, rect, color| {
        let color = if *on {
            color
        } else {
            color.gamma_multiply(0.4)
        };
        icons::glyph(painter, rect, glyph, color);
        if strike && !*on {
            painter.line_segment(
                [rect.left_bottom(), rect.right_top()],
                egui::Stroke::new(1.5, color),
            );
        }
    });
    let clicked = r.on_hover_text(tip).clicked();
    if clicked {
        *on = !*on;
    }
    clicked
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

/// A colour button: "Auto" (theme foreground / by layer) or a picked colour.
pub(crate) fn color_choice(
    ui: &mut egui::Ui,
    value: Option<Rgb>,
    auto_label: &str,
    dark: bool,
) -> Option<Option<Rgb>> {
    let mut out = None;
    ui.horizontal(|ui| {
        let auto = value.is_none();
        let id = ui.id().with(("colour", auto_label));
        egui::ComboBox::from_id_salt(id)
            .selected_text(if auto { auto_label } else { "Custom" })
            .width(90.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(auto, auto_label).clicked() && !auto {
                    out = Some(None);
                }
                if ui.selectable_label(!auto, "Custom").clicked() && auto {
                    out = Some(Some(Rgb([0xd0, 0x40, 0x40])));
                }
            });
        if !auto {
            let c = to_color32(value, dark);
            let mut rgb = [c.r(), c.g(), c.b()];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                out = Some(Some(Rgb(rgb)));
            }
        }
    });
    out
}

pub(crate) fn weight_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<LineWeight>,
    by_layer: bool,
) -> bool {
    let text = value.map_or("By layer".into(), LineWeight::label);
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(90.0)
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
) -> bool {
    let text = value.map_or("By layer", LineType::label);
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(90.0)
        .show_ui(ui, |ui| {
            if by_layer {
                changed |= ui.selectable_value(value, None, "By layer").changed();
            }
            for t in LineType::ALL {
                changed |= ui.selectable_value(value, Some(t), t.label()).changed();
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

fn pos_of(list: &[LayerId], id: LayerId) -> Option<usize> {
    list.iter().position(|l| *l == id)
}

/// The bar above the list while a layer is isolated; true when "End isolate" is clicked.
fn isolate_bar(ui: &mut egui::Ui, name: &str) -> bool {
    let accent = crate::theme::accent(ui.visuals().dark_mode);
    let mut end = false;
    egui::Frame::new()
        .fill(accent.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, accent.gamma_multiply(0.5)))
        .corner_radius(7.0)
        .inner_margin(egui::Margin::symmetric(6, 3))
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
    isolated: Option<LayerId>,
    any_hidden: bool,
    /// The layer each setting is Alt-clicked onto, if any.
    solo_lock: Option<LayerId>,
    solo_snap: Option<LayerId>,
}

impl DriftApp {
    /// The Layers panel: a filter once there are many layers, the isolate bar, then a
    /// compact row per layer (colour, name, curve count, visibility, locking; the current
    /// one highlighted) that opens to show its colours and line settings; then underlays.
    /// Only the rows in view are drawn, so a drawing with a thousand layers costs no more
    /// than one with ten.
    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        self.end_stale_isolate();
        self.layer_counts.update(&self.editor.doc);
        let counts = std::mem::take(&mut self.layer_counts.curves);
        let layers = std::mem::take(&mut self.layer_counts.layers);
        let current = self.editor.doc.current_layer();
        let has_selection = !self.editor.selection.is_empty();
        // The layers the selection is on.
        let picked: std::collections::BTreeSet<LayerId> = self
            .editor
            .selection
            .iter()
            .filter_map(|id| self.editor.doc.get(*id).map(|e| e.layer))
            .collect();
        // The rows, top down as the layers stack (the top one draws over the rest), that
        // match the filter.
        let needle = self.layer_list.filter.trim().to_lowercase();
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
            isolated: self.layer_list.isolated.as_ref().map(|i| i.layer),
            any_hidden,
            solo_lock: self.solo_of(LayerFlag::Locked),
            solo_snap: self.solo_of(LayerFlag::Snappable),
        };
        let mut status = None;
        let mut action = None;
        crate::theme::section(ui, &format!("Layers · {total}"));
        if total > FILTER_FROM || !self.layer_list.filter.is_empty() {
            self.layer_filter(ui, list.len(), total);
        }
        if let Some(iso) = &self.layer_list.isolated {
            let name = self
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
        let output = egui::ScrollArea::vertical()
            .id_salt("layers_scroll")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                // Each row's top, from the heights the rows had when last drawn.
                let closed = if self.layer_list.row > 0.0 {
                    self.layer_list.row
                } else {
                    29.0
                };
                let heights: Vec<f32> = list
                    .iter()
                    .map(|id| {
                        if self.expanded_layers.contains(id) {
                            let opened = self.layer_list.opened.get(id).copied();
                            opened.unwrap_or(closed + 140.0)
                        } else {
                            closed
                        }
                    })
                    .collect();
                let mut tops = Vec::with_capacity(list.len() + 1);
                let mut y = 0.0;
                for h in &heights {
                    tops.push(y);
                    y += h;
                }
                tops.push(y);
                if let Some(i) = self.layer_list.reveal.take().and_then(|r| pos_of(&list, r)) {
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
                    ui.add_space(1.0);
                    rows.push((id, rect));
                    let drawn = ui.cursor().top() - top;
                    let open = self.expanded_layers.contains(&id);
                    let guess = heights[i];
                    if open {
                        self.layer_list.opened.insert(id, drawn);
                    } else {
                        self.layer_list.row = drawn;
                    }
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
                            let n = self.editor.doc.layers().count();
                            let name = format!("Layer {n}");
                            let id = self.editor.add_layer(&name);
                            self.editor.set_current_layer(id);
                            self.renaming_layer = Some(id);
                            self.layer_names.insert(id, name);
                            // The new layer would hide behind a filter it doesn't match.
                            self.layer_list.filter.clear();
                            self.layer_list.reveal = Some(id);
                        }
                    });
                    self.underlays_section(ui);
                });
            });
        self.panel_scrolled(ui, crate::dock::Panel::Layers, &output);
        self.layer_counts.curves = counts;
        self.layer_counts.layers = layers;
        self.layer_list
            .opened
            .retain(|id, _| self.expanded_layers.contains(id));
        match action {
            Some(LayerAction::Isolate(id)) => {
                let before = self.editor.isolate_layer(id);
                // Isolating another layer keeps the first isolate's "before".
                let before = match self.layer_list.isolated.take() {
                    Some(iso) => iso.before,
                    None => before,
                };
                self.layer_list.isolated = Some(Isolated { layer: id, before });
            }
            Some(LayerAction::EndIsolate) => {
                if let Some(iso) = self.layer_list.isolated.take() {
                    self.editor.set_layers_visible(&iso.before, "End isolate");
                }
            }
            Some(LayerAction::ShowAll) => {
                self.layer_list.isolated = None;
                let all: Vec<(LayerId, bool)> =
                    self.editor.doc.layers().map(|(id, _)| (id, true)).collect();
                self.editor.set_layers_visible(&all, "Show all layers");
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
        self.layer_list
            .solos
            .iter()
            .find(|s| s.flag == flag)
            .map(|s| s.layer)
    }

    /// Alt-click on a layer's lock or snap toggle: set the other layers the other way, or,
    /// on the layer it was done on, put them back (one undo step each).
    pub(crate) fn toggle_solo(&mut self, flag: LayerFlag, id: LayerId) {
        let was = self
            .layer_list
            .solos
            .iter()
            .position(|s| s.flag == flag)
            .map(|i| self.layer_list.solos.remove(i));
        match was {
            Some(solo) if solo.layer == id => {
                self.editor
                    .set_layers_flag(flag, &solo.before, flag.labels().1);
            }
            was => {
                let before = self.editor.solo_layer_flag(flag, id);
                // Done on another layer meanwhile: the first one's "before" stays.
                let before = was.map_or(before, |s| s.before);
                self.layer_list.solos.push(Solo {
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
        let doc = &self.editor.doc;
        self.layer_list.solos.retain(|s| {
            let gone = doc.layer(s.layer).is_none();
            let undone = s
                .before
                .iter()
                .all(|(id, on)| doc.layer(*id).is_none_or(|l| s.flag.get(l) == *on));
            !gone && !undone
        });
        let Some(iso) = &self.layer_list.isolated else {
            return;
        };
        let doc = &self.editor.doc;
        let gone = doc.layer(iso.layer).is_none();
        let undone = iso
            .before
            .iter()
            .all(|(id, on)| doc.layer(*id).is_none_or(|l| l.visible == *on));
        if gone || undone {
            self.layer_list.isolated = None;
        }
    }

    /// The filter field: names containing the text (any case), how many match, and a
    /// button (or Escape) to clear it.
    fn layer_filter(&mut self, ui: &mut egui::Ui, matched: usize, total: usize) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let filter = &mut self.layer_list.filter;
                if !filter.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("{matched} of {total}"))
                            .small()
                            .weak(),
                    )
                    .on_hover_text("Layers matching the filter");
                    if ui
                        .small_button("×")
                        .on_hover_text("Clear the filter")
                        .clicked()
                    {
                        filter.clear();
                    }
                }
                let r = ui.add(
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
        let Some(layer) = self.editor.doc.layer(id).cloned() else {
            return egui::Rect::NOTHING;
        };
        let accent = crate::theme::accent(ui.visuals().dark_mode);
        let mut edited = layer.clone();
        let is_current = id == info.current;
        let open = self.expanded_layers.contains(&id);
        // The whole row: a click makes the layer current, a drag moves it in the order (its
        // own buttons and fields take their clicks first).
        let sense = if draggable {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        let row = ui.scope_builder(
            // An id of its own: which rows are drawn changes as the list scrolls.
            egui::UiBuilder::new()
                .id(egui::Id::new(("layer_row", id, draggable)))
                .sense(sense),
            |ui| {
                egui::Frame::new()
                    .fill(if is_current {
                        accent.gamma_multiply(0.16)
                    } else if open {
                        ui.visuals().faint_bg_color
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(7.0)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        if let Some(a) = self.layer_row(ui, id, &layer, &mut edited, info) {
                            *action = Some(a);
                        }
                        if open {
                            self.layer_details(ui, id, &mut edited, info.has_selection, status);
                        }
                    });
            },
        );
        let row = row.response;
        if row.clicked() && !is_current {
            self.editor.set_current_layer(id);
        }
        if row.drag_started() {
            self.layer_drag = Some(id);
        }
        if self.layer_drag.is_none() && row.hovered() && !is_current {
            ui.painter().rect_stroke(
                row.rect,
                7.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.hovered.bg_stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        let rect = row.rect;
        let tip = if draggable {
            "Click to make current · drag to reorder (top draws on top)"
        } else {
            "Click to make current"
        };
        row.on_hover_text(format!("{}\n{tip}", layer.name))
            .context_menu(|ui| {
                if ui.button("Make current").clicked() {
                    self.editor.set_current_layer(id);
                    ui.close();
                }
                if ui
                    .add_enabled(id != LayerId::DEFAULT, egui::Button::new("Rename"))
                    .clicked()
                {
                    self.renaming_layer = Some(id);
                    ui.close();
                }
                if ui.button("Select all on this layer").clicked() {
                    self.editor.select_layer(id);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        info.has_selection,
                        egui::Button::new("Move selection to this layer"),
                    )
                    .clicked()
                {
                    self.editor.move_selection_to_layer(id);
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
                    if let Err(e) = self.editor.remove_layer(id) {
                        *status = Some(e);
                    }
                    ui.close();
                }
            });
        if edited != layer && self.editor.doc.layer(id).is_some() {
            self.editor.update_layer(id, edited);
        }
        rect
    }

    /// A layer's row: disclosure chevron, colour swatch, name (click: make current;
    /// double-click: rename), the selection marker, curve count, visibility and lock.
    /// Alt-click on the eye isolates the layer (or ends the isolate).
    fn layer_row(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        layer: &Layer,
        edited: &mut Layer,
        info: &RowInfo,
    ) -> Option<LayerAction> {
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::accent(dark);
        let is_current = id == info.current;
        let open = self.expanded_layers.contains(&id);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            // The active (current) layer's dot; a faint ring on the others.
            let (dot, dot_response) =
                ui.allocate_exact_size(egui::vec2(10.0, 22.0), egui::Sense::hover());
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
            let (chev, r) = ui.allocate_exact_size(egui::vec2(14.0, 22.0), egui::Sense::click());
            let c = chev.center();
            let color = if r.hovered() {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            let pts = if open {
                vec![
                    c + egui::vec2(-3.5, -1.8),
                    c + egui::vec2(0.0, 1.8),
                    c + egui::vec2(3.5, -1.8),
                ]
            } else {
                vec![
                    c + egui::vec2(-1.8, -3.5),
                    c + egui::vec2(1.8, 0.0),
                    c + egui::vec2(-1.8, 3.5),
                ]
            };
            ui.painter()
                .add(egui::Shape::line(pts, egui::Stroke::new(1.4, color)));
            if r.on_hover_text("Colours and line settings").clicked() {
                if open {
                    self.expanded_layers.remove(&id);
                } else {
                    self.expanded_layers.insert(id);
                }
            }
            // Stroke colour, with the fill colour in the lower right when it differs.
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(swatch, 3.0, to_color32(layer.color, dark));
            if layer.fill_color.is_some_and(|c| Some(c) != layer.color) {
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![
                        swatch.right_top(),
                        swatch.right_bottom(),
                        swatch.left_bottom(),
                    ],
                    to_color32(layer.fill_colour(), dark),
                    egui::Stroke::NONE,
                ));
            }
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
                ui.add_space(4.0);
                let n = info.counts.get(&id).copied().unwrap_or(0);
                ui.label(egui::RichText::new(n.to_string()).small().weak())
                    .on_hover_text("Curves on this layer");
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
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.layer_name(ui, id, layer, edited, is_current);
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
        let Some(dragged) = self.layer_drag else {
            return;
        };
        let (Some(pointer), down) =
            ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()))
        else {
            self.layer_drag = None;
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
        self.layer_drag = None;
        // The dragged layer goes just above the row after the gap, or just below the row
        // before it, in the whole list (top down) without the dragged layer.
        let mut order: Vec<LayerId> = self
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
        let now = self.editor.doc.layers().position(|(id, _)| id == dragged);
        if now != Some(index) {
            self.editor.reorder_layer(dragged, index);
        }
    }

    /// A layer's name, or the field renaming it.
    fn layer_name(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        layer: &Layer,
        edited: &mut Layer,
        is_current: bool,
    ) {
        if self.renaming_layer == Some(id) && id != LayerId::DEFAULT {
            let name = self
                .layer_names
                .entry(id)
                .or_insert_with(|| layer.name.clone());
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(ui.available_width()));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if !cancel && !name.trim().is_empty() {
                    edited.name = name.trim().to_owned();
                }
                self.renaming_layer = None;
                self.layer_names.remove(&id);
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
                "{}\n{}",
                layer.name,
                if is_current {
                    "The current layer: new geometry goes here. Double-click to rename"
                } else {
                    "Click to make this the current layer; double-click to rename"
                }
            ));
        if r.clicked() && !is_current {
            self.editor.set_current_layer(id);
        }
        if r.double_clicked() && id != LayerId::DEFAULT {
            self.renaming_layer = Some(id);
        }
    }

    /// An opened layer row: stroke and fill colours, weight and line type, printing, and
    /// actions.
    fn layer_details(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        edited: &mut Layer,
        has_selection: bool,
        status: &mut Option<String>,
    ) {
        let dark = ui.visuals().dark_mode;
        let indent = 19.0;
        let label = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(indent);
            ui.add_sized(
                [44.0, 20.0],
                egui::Label::new(egui::RichText::new(text).weak()),
            );
        };
        ui.horizontal(|ui| {
            label(ui, "Stroke");
            if let Some(c) = color_choice(ui, edited.color, "Auto", dark) {
                edited.color = c;
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Fill");
            if let Some(c) = color_choice(ui, edited.fill_color, "Same as stroke", dark) {
                edited.fill_color = c;
            }
        })
        .response
        .on_hover_text("Colour of fills and hatches on this layer");
        ui.horizontal(|ui| {
            label(ui, "Line");
            let mut w = Some(edited.weight);
            if weight_combo(ui, ("layer_weight", id.0), &mut w, false) {
                edited.weight = w.unwrap_or_default();
            }
            let mut t = Some(edited.line_type);
            if line_type_combo(ui, ("layer_type", id.0), &mut t, false) {
                edited.line_type = t.unwrap_or_default();
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Output");
            ui.checkbox(&mut edited.printable, "Printed and exported");
        });
        ui.horizontal(|ui| {
            ui.add_space(indent);
            if ui.small_button("Select all").clicked() {
                self.editor.select_layer(id);
            }
            if ui
                .add_enabled(
                    has_selection,
                    egui::Button::new("Move selection here").small(),
                )
                .on_hover_text("Move the selected geometry onto this layer")
                .clicked()
            {
                self.editor.move_selection_to_layer(id);
            }
            if id != LayerId::DEFAULT
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Delete").color(ui.visuals().error_fg_color),
                        )
                        .small(),
                    )
                    .on_hover_text("Delete the layer (its geometry moves to 0)")
                    .clicked()
            {
                if let Err(e) = self.editor.remove_layer(id) {
                    *status = Some(e);
                }
                self.expanded_layers.remove(&id);
            }
        });
        ui.add_space(3.0);
    }

    /// Layer and appearance controls for the selected curves (in the Properties tab).
    pub(crate) fn appearance_panel(&mut self, ui: &mut egui::Ui) {
        let curves: Vec<EntityId> = self
            .editor
            .selection
            .iter()
            .copied()
            .filter(|id| self.editor.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        if curves.is_empty() {
            return;
        }
        let first = self.editor.doc.get(curves[0]).copied().expect("exists");
        let uniform_layer = curves.iter().all(|id| {
            self.editor
                .doc
                .get(*id)
                .is_some_and(|e| e.layer == first.layer)
        });
        ui.add_space(10.0);
        let label = |ui: &mut egui::Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
        };
        egui::Grid::new("appearance")
            .num_columns(2)
            .min_col_width(crate::theme::LABEL_WIDTH)
            .show(ui, |ui| {
                label(ui, "Layer");
                let text = if uniform_layer {
                    self.editor
                        .doc
                        .layer(first.layer)
                        .map_or("?".into(), |l| l.name.clone())
                } else {
                    "(mixed)".into()
                };
                let layers: Vec<(LayerId, String)> = self
                    .editor
                    .doc
                    .layers()
                    .map(|(id, l)| (id, l.name.clone()))
                    .collect();
                let mut target = None;
                egui::ComboBox::from_id_salt("selection_layer")
                    .selected_text(text)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (id, name) in layers {
                            if ui
                                .selectable_label(uniform_layer && id == first.layer, name)
                                .clicked()
                            {
                                target = Some(id);
                            }
                        }
                    });
                if let Some(id) = target {
                    self.editor.move_selection_to_layer(id);
                }
                ui.end_row();
                self.stroke_rows(ui, &curves, "selection");
            });
    }

    /// Colour, weight and line type rows (in a two-column grid) for `curves`' own
    /// appearance, shown as the first's; `salt` keeps the combos' ids apart.
    pub(crate) fn stroke_rows(&mut self, ui: &mut egui::Ui, curves: &[EntityId], salt: &str) {
        let Some(first) = curves
            .first()
            .and_then(|id| self.editor.doc.get(*id))
            .copied()
        else {
            return;
        };
        let dark = ui.visuals().dark_mode;
        let label = |ui: &mut egui::Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
        };
        let a = first.appearance;
        label(ui, "Colour");
        if let Some(c) = color_choice(ui, a.color, "By layer", dark) {
            self.editor.update_appearance(curves, |x| x.color = c);
        }
        ui.end_row();
        label(ui, "Weight");
        let mut w = a.weight;
        if weight_combo(ui, (salt, "weight"), &mut w, true) {
            self.editor.update_appearance(curves, |x| x.weight = w);
        }
        ui.end_row();
        label(ui, "Line type");
        let mut t = a.line_type;
        if line_type_combo(ui, (salt, "type"), &mut t, true) {
            self.editor.update_appearance(curves, |x| x.line_type = t);
        }
        ui.end_row();
    }
}
