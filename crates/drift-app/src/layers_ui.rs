//! Layers: editor commands (each one undo step), the Layers tab, and the layer / appearance
//! controls for the selection in the Properties tab.

use drift_core::{Appearance, EntityId, Layer, LayerId, LineType, LineWeight, Rgb};
use eframe::egui;

use crate::DriftApp;
use crate::editor::Editor;
use crate::icons::{self, Glyph};

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
pub(crate) fn icon_toggle(ui: &mut egui::Ui, on: &mut bool, glyph: Glyph, strike: bool, tip: &str) {
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
    if r.on_hover_text(tip).clicked() {
        *on = !*on;
    }
}

impl Editor {
    pub fn add_layer(&mut self, name: &str) -> LayerId {
        self.doc.begin();
        let id = self.doc.add_layer(Layer::new(name.trim()));
        self.commit("Add layer");
        id
    }

    /// Change a layer's settings (name, colour, weight, line type, visibility, locking).
    pub fn update_layer(&mut self, id: LayerId, layer: Layer) {
        if self.doc.layer(id) == Some(&layer) {
            return;
        }
        self.doc.begin();
        self.doc.set_layer(id, layer);
        self.commit("Change layer");
    }

    /// Delete a layer; its geometry moves to layer "0".
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), String> {
        self.doc.begin();
        if let Err(e) = self.doc.remove_layer(id) {
            self.doc.rollback();
            return Err(e.to_string());
        }
        self.commit("Delete layer");
        Ok(())
    }

    /// Move a layer to `index` in the draw order (0 at the bottom), one undo step.
    pub fn reorder_layer(&mut self, id: LayerId, index: usize) {
        self.doc.begin();
        self.doc.reorder_layer(id, index);
        self.commit("Reorder layers");
    }

    pub fn set_current_layer(&mut self, id: LayerId) {
        self.doc.begin();
        let _ = self.doc.set_current_layer(id);
        self.commit("Set current layer");
    }

    /// The selected entities with their defining points.
    fn selection_with_points(&self) -> Vec<EntityId> {
        let mut ids = self.doc.defining_points(self.selection.iter().copied());
        ids.extend(self.selection.iter().copied());
        ids.into_iter().collect()
    }

    /// Move the selection (with its points) to a layer.
    pub fn move_selection_to_layer(&mut self, layer: LayerId) {
        let ids = self.selection_with_points();
        self.doc.begin();
        for id in ids {
            let _ = self.doc.set_entity_layer(id, layer);
        }
        self.commit("Move to layer");
    }

    /// Select every curve and standalone point on a layer.
    pub fn select_layer(&mut self, layer: LayerId) {
        let referenced = self.doc.referenced_points();
        self.selection = self
            .doc
            .iter()
            .filter(|(id, e)| e.layer == layer && !referenced.contains(id))
            .map(|(id, _)| id)
            .collect();
    }

    /// Change the appearance overrides of the selected curves with `f`.
    pub fn update_selection_appearance(&mut self, f: impl Fn(&mut Appearance)) {
        let ids: Vec<EntityId> = self.selection.iter().copied().collect();
        self.doc.begin();
        for id in ids {
            if let Some(mut a) = self.doc.get(id).map(|e| e.appearance) {
                f(&mut a);
                let _ = self.doc.set_appearance(id, a);
            }
        }
        self.commit("Change appearance");
    }
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
        });
    changed
}

impl DriftApp {
    /// The Layers panel: a compact row per layer (colour, name, curve count, visibility,
    /// locking; the current one highlighted) that opens to show its colours and line
    /// settings; then underlays.
    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::accent(dark);
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
        let mut status = None;
        // The list reads top down as the layers stack: the top one draws over the rest.
        let mut rows: Vec<(LayerId, egui::Rect)> = Vec::new();
        let output = egui::ScrollArea::vertical()
            .id_salt("layers_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::theme::section(ui, &format!("Layers · {}", layers.len()));
                for (id, layer) in layers.iter().rev() {
                    let id = *id;
                    let mut edited = layer.clone();
                    let is_current = id == current;
                    let open = self.expanded_layers.contains(&id);
                    // The whole row: a click makes the layer current, a drag moves it in
                    // the order (its own buttons and fields take their clicks first).
                    let row = ui.scope_builder(
                        egui::UiBuilder::new()
                            .id_salt(("layer_row", id))
                            .sense(egui::Sense::click_and_drag()),
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
                                    self.layer_row(
                                        ui,
                                        id,
                                        layer,
                                        &mut edited,
                                        is_current,
                                        open,
                                        &counts,
                                        &picked,
                                    );
                                    if open {
                                        self.layer_details(
                                            ui,
                                            id,
                                            &mut edited,
                                            has_selection,
                                            &mut status,
                                        );
                                    }
                                });
                        },
                    );
                    let row = row.response;
                    rows.push((id, row.rect));
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
                    row.on_hover_text("Click to make current · drag to reorder (top draws on top)")
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
                                    has_selection,
                                    egui::Button::new("Move selection to this layer"),
                                )
                                .clicked()
                            {
                                self.editor.move_selection_to_layer(id);
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(
                                    id != LayerId::DEFAULT,
                                    egui::Button::new("Delete layer"),
                                )
                                .clicked()
                            {
                                if let Err(e) = self.editor.remove_layer(id) {
                                    status = Some(e);
                                }
                                ui.close();
                            }
                        });
                    if edited != *layer && self.editor.doc.layer(id).is_some() {
                        self.editor.update_layer(id, edited);
                    }
                    ui.add_space(1.0);
                }
                self.drag_layer(ui, &rows);
                ui.add_space(6.0);
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
                }
                self.underlays_section(ui);
            });
        self.panel_scrolled(ui, crate::dock::Panel::Layers, &output);
        self.layer_counts.curves = counts;
        self.layer_counts.layers = layers;
        if let Some(text) = status {
            self.set_status(text, true, now);
        }
    }

    /// A layer's row: disclosure chevron, colour swatch, name (click: make current;
    /// double-click: rename), the selection marker, curve count, visibility and lock.
    #[allow(clippy::too_many_arguments)]
    fn layer_row(
        &mut self,
        ui: &mut egui::Ui,
        id: LayerId,
        layer: &Layer,
        edited: &mut Layer,
        is_current: bool,
        open: bool,
        counts: &std::collections::HashMap<LayerId, usize>,
        picked: &std::collections::BTreeSet<LayerId>,
    ) {
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::accent(dark);
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
                icon_toggle(
                    ui,
                    &mut locked,
                    glyph,
                    false,
                    "Locked geometry can be snapped to but not edited",
                );
                edited.locked = locked;
                icon_toggle(ui, &mut edited.visible, Glyph::Eye, true, "Show or hide");
                icon_toggle(
                    ui,
                    &mut edited.snappable,
                    Glyph::Snap,
                    true,
                    "Snap to this layer's geometry (off: it can still be picked and edited)",
                );
                ui.add_space(4.0);
                let n = counts.get(&id).copied().unwrap_or(0);
                ui.label(egui::RichText::new(n.to_string()).small().weak())
                    .on_hover_text("Curves on this layer");
                if picked.contains(&id) {
                    // The selection is on this layer: a small selection box (the current
                    // layer has the dot at the left).
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
    }

    /// A layer row being dragged: a line where it would go, and on release, the move.
    fn drag_layer(&mut self, ui: &mut egui::Ui, rows: &[(LayerId, egui::Rect)]) {
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
        let Some(from) = rows.iter().position(|(id, _)| *id == dragged) else {
            return;
        };
        // Where it lands in the list, then in the draw order (the list is top down).
        let at = if from < slot { slot - 1 } else { slot };
        if at != from {
            self.editor.reorder_layer(dragged, rows.len() - 1 - at);
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
        let name = egui::RichText::new(&layer.name);
        let name = if is_current {
            name.family(crate::theme::bold())
        } else if layer.visible {
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
            .on_hover_text(if is_current {
                "The current layer: new geometry goes here. Double-click to rename"
            } else {
                "Click to make this the current layer; double-click to rename"
            });
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
        let dark = ui.visuals().dark_mode;
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

                let a = first.appearance;
                label(ui, "Colour");
                if let Some(c) = color_choice(ui, a.color, "By layer", dark) {
                    self.editor.update_selection_appearance(|x| x.color = c);
                }
                ui.end_row();
                label(ui, "Weight");
                let mut w = a.weight;
                if weight_combo(ui, "selection_weight", &mut w, true) {
                    self.editor.update_selection_appearance(|x| x.weight = w);
                }
                ui.end_row();
                label(ui, "Line type");
                let mut t = a.line_type;
                if line_type_combo(ui, "selection_type", &mut t, true) {
                    self.editor.update_selection_appearance(|x| x.line_type = t);
                }
                ui.end_row();
            });
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{Constraint, DVec2, EntityKind, LineKind, Shape};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Two separate lines; the first on a new layer. Returns (editor, layer, lines).
    fn two_layers() -> (Editor, LayerId, [EntityId; 2]) {
        let mut ed = Editor::default();
        let layer = ed.add_layer("Frame");
        ed.set_current_layer(layer);
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 0.0));
        let l1 = ed.doc.add_line(a, b).unwrap();
        ed.doc.set_current_layer(LayerId::DEFAULT).unwrap();
        let c = ed.doc.add_point(v(0.0, 5.0));
        let d = ed.doc.add_point(v(10.0, 7.0));
        let l2 = ed.doc.add_line(c, d).unwrap();
        ed.commit("Lines");
        (ed, layer, [l1, l2])
    }

    #[test]
    fn layers_reorder_and_keep_their_order_in_files() {
        let (mut ed, frame, lines) = two_layers();
        let notes = ed.add_layer("Notes");
        let order = |ed: &Editor| ed.doc.layers().map(|(id, _)| id).collect::<Vec<_>>();
        // New layers go on top.
        assert_eq!(order(&ed), [LayerId::DEFAULT, frame, notes]);
        ed.reorder_layer(notes, 0);
        assert_eq!(order(&ed), [notes, LayerId::DEFAULT, frame]);
        ed.undo();
        assert_eq!(order(&ed), [LayerId::DEFAULT, frame, notes]);
        ed.redo();
        let text = drift_io::save(&ed.doc, &Default::default());
        let (loaded, _) = drift_io::load(&text).unwrap();
        assert_eq!(
            loaded.layers().map(|(id, _)| id).collect::<Vec<_>>(),
            [notes, LayerId::DEFAULT, frame]
        );
        // Curves draw layer by layer: Frame's line (on top of layer 0 now) comes last.
        let [l1, _] = lines;
        let frame_line = ed.doc.shape(l1);
        let last = ed
            .items()
            .into_iter()
            .rev()
            .find(|i| !matches!(i.shape, drift_core::Shape::Point(_)))
            .map(|i| i.shape);
        assert_eq!(last, frame_line);
    }

    #[test]
    fn non_snappable_layers_are_not_snapped_to() {
        let (mut ed, frame, _) = two_layers();
        let options = drift_geom::SnapOptions {
            tolerance: 1.0,
            grid: None,
            from: None,
        };
        let near = |ed: &Editor, p: DVec2| ed.snap(p, options, |_| false).map(|s| s.at);
        assert_eq!(near(&ed, v(10.2, 0.1)), Some(v(10.0, 0.0)));
        let mut off = ed.doc.layer(frame).cloned().unwrap();
        off.snappable = false;
        ed.update_layer(frame, off);
        assert_ne!(near(&ed, v(10.2, 0.1)), Some(v(10.0, 0.0)));
        // Layer 0 still snaps; Frame's line can still be picked.
        assert_eq!(near(&ed, v(10.2, 6.9)), Some(v(10.0, 7.0)));
    }

    fn ends(ed: &Editor, line: EntityId) -> [EntityId; 2] {
        match ed.doc.get(line).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => [a, b],
            _ => panic!(),
        }
    }

    #[test]
    fn new_geometry_goes_on_the_current_layer() {
        let (ed, layer, [l1, l2]) = two_layers();
        assert_eq!(ed.doc.get(l1).unwrap().layer, layer);
        assert_eq!(ed.doc.get(ends(&ed, l1)[0]).unwrap().layer, layer);
        assert_eq!(ed.doc.get(l2).unwrap().layer, LayerId::DEFAULT);
    }

    #[test]
    fn locked_layers_stay_put() {
        let (mut ed, layer, [l1, l2]) = two_layers();
        let mut locked = ed.doc.layer(layer).unwrap().clone();
        locked.locked = true;
        ed.update_layer(layer, locked);
        // Joining the free line's start to the locked line's end moves only the free line.
        let [_, b] = ends(&ed, l1);
        let [c, _] = ends(&ed, l2);
        ed.add_constraints("Coincident", &[Constraint::Coincident { a: b, b: c }])
            .unwrap();
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 0.0));
        assert!((ed.doc.point(c).unwrap() - v(10.0, 0.0)).length() < 1e-9);
        // Constraints the lock can't satisfy are refused, and the lock itself is never named.
        let [a, _] = ends(&ed, l1);
        let r = ed.add_constraints("Length", &[Constraint::Distance { a, b, value: 3.0 }]);
        assert!(r.is_err());
        assert!(ed.analysis().conflicts.iter().all(|c| c.0 != 0));
        // Locked geometry can't be selected.
        ed.selection = [l1].into();
        ed.sync();
        assert!(ed.selection.is_empty());
    }

    #[test]
    fn hidden_layers_are_not_drawn_or_picked() {
        let (mut ed, layer, [l1, _]) = two_layers();
        let mut hidden = ed.doc.layer(layer).unwrap().clone();
        hidden.visible = false;
        ed.update_layer(layer, hidden);
        assert!(ed.index.pick(v(5.0, 0.0), 0.5, |_| true).is_none());
        assert!(
            ed.items()
                .iter()
                .all(|i| i.shape != ed.doc.shape(l1).unwrap())
        );
    }

    #[test]
    fn appearance_follows_the_layer_unless_overridden() {
        let (mut ed, layer, [l1, l2]) = two_layers();
        let mut look = ed.doc.layer(layer).unwrap().clone();
        look.color = Some(Rgb([220, 40, 40]));
        look.weight = LineWeight(70);
        look.line_type = LineType::Dashed;
        ed.update_layer(layer, look);
        let item = |ed: &Editor, id: EntityId, status: bool| {
            let shape = ed.doc.shape(id).unwrap();
            ed.items_styled(status, true)
                .into_iter()
                .find(|i| i.shape == shape)
                .unwrap()
        };
        let i = item(&ed, l1, false);
        assert_eq!(
            (i.color, i.weight, i.line_type),
            (Some([220, 40, 40]), 70, LineType::Dashed)
        );
        // Constraint-status colours ignore layer colours (weights still apply).
        let i = item(&ed, l1, true);
        assert_eq!((i.color, i.weight), (None, 70));
        // Overrides win.
        ed.selection = [l1].into();
        ed.update_selection_appearance(|a| a.weight = Some(LineWeight(13)));
        assert_eq!(item(&ed, l1, false).weight, 13);
        // Moving to another layer takes its look; one undo step each.
        ed.selection = [l2].into();
        ed.move_selection_to_layer(layer);
        assert_eq!(item(&ed, l2, false).line_type, LineType::Dashed);
        assert!(ed.undo());
        assert_eq!(item(&ed, l2, false).line_type, LineType::Continuous);
    }

    #[test]
    fn modified_copies_keep_their_source_layer() {
        let (mut ed, layer, [l1, _]) = two_layers();
        ed.set_current_layer(LayerId::DEFAULT);
        let m = ed.split_at(l1, v(4.0, 0.0)).unwrap();
        let pieces: Vec<EntityId> = ed.doc.users_of(m).collect();
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces
                .iter()
                .all(|p| ed.doc.get(*p).unwrap().layer == layer)
        );
        let _ = (LineKind::Solid, Shape::Point(v(0.0, 0.0)));
    }

    #[test]
    fn deleting_a_layer_moves_its_geometry_to_layer_zero() {
        let (mut ed, layer, [l1, _]) = two_layers();
        ed.remove_layer(layer).unwrap();
        assert_eq!(ed.doc.get(l1).unwrap().layer, LayerId::DEFAULT);
        assert!(ed.remove_layer(LayerId::DEFAULT).is_err());
        ed.undo();
        assert_eq!(ed.doc.get(l1).unwrap().layer, layer);
    }
}
