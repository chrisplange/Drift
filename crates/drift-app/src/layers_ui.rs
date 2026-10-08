//! Layers: editor commands (each one undo step), the Layers tab, and the layer / appearance
//! controls for the selection in the Properties tab.

use drift_core::{Appearance, EntityId, Layer, LayerId, LineType, LineWeight, Rgb};
use eframe::egui;

use crate::DriftApp;
use crate::editor::Editor;
use crate::icons::{self, Glyph};

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
        let mut auto = value.is_none();
        if ui.checkbox(&mut auto, auto_label).changed() {
            out = Some(if auto {
                None
            } else {
                Some(Rgb([0xd0, 0x40, 0x40]))
            });
        }
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
    /// The Layers tab.
    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let dark = ui.visuals().dark_mode;
        let layers: Vec<(LayerId, Layer)> = self
            .editor
            .doc
            .layers()
            .map(|(id, l)| (id, l.clone()))
            .collect();
        let current = self.editor.doc.current_layer();
        let has_selection = !self.editor.selection.is_empty();
        let mut status = None;
        let accent = crate::theme::accent(dark);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::theme::section(ui, &format!("Layers · {}", layers.len()));
                });
                if self.layers_dock == crate::app::layers_hud::LayersDock::Hidden
                    && ui
                        .button("Show as a panel")
                        .on_hover_text(
                            "A compact layer list to keep at hand while drawing (Ctrl+L)",
                        )
                        .clicked()
                {
                    self.toggle_layers_hud();
                }
                for (id, layer) in layers {
                    let mut edited = layer.clone();
                    let is_current = current == id;
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(8.0)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if is_current {
                                accent
                            } else {
                                egui::Color32::TRANSPARENT
                            },
                        ))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                        if ui
                            .radio(current == id, "")
                            .on_hover_text("Current layer: new geometry goes here")
                            .clicked()
                        {
                            self.editor.set_current_layer(id);
                        }
                        let name = self
                            .layer_names
                            .entry(id)
                            .or_insert_with(|| layer.name.clone());
                        let r = ui.add_enabled(
                            id != LayerId::DEFAULT,
                            egui::TextEdit::singleline(name)
                                .desired_width(ui.available_width() - 112.0),
                        );
                        if r.lost_focus() && !name.trim().is_empty() {
                            edited.name = name.trim().to_owned();
                        }
                        if !r.has_focus() && !r.lost_focus() {
                            *name = layer.name.clone();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if id != LayerId::DEFAULT
                                && ui
                                    .add(egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(16.0)
                                            .color(ui.visuals().weak_text_color()),
                                    )
                                    .frame(false))
                                    .on_hover_text("Delete layer (its geometry moves to 0)")
                                    .clicked()
                            {
                                if let Err(e) = self.editor.remove_layer(id) {
                                    status = Some(e);
                                }
                                self.layer_names.remove(&id);
                            }
                            icon_toggle(
                                ui,
                                &mut edited.printable,
                                Glyph::Print,
                                true,
                                "Printable: included when printing or exporting",
                            );
                            let mut locked = edited.locked;
                            let lock_glyph = if locked { Glyph::Lock } else { Glyph::Unlock };
                            icon_toggle(
                                ui,
                                &mut locked,
                                lock_glyph,
                                false,
                                "Locked geometry can be snapped to but not edited, and stays fixed",
                            );
                            edited.locked = locked;
                            icon_toggle(ui, &mut edited.visible, Glyph::Eye, true, "Show or hide");
                        });
                    });
                            ui.horizontal(|ui| {
                                ui.add_space(22.0);
                                if let Some(c) = color_choice(ui, edited.color, "Auto colour", dark)
                                {
                                    edited.color = c;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(22.0);
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
                                ui.add_space(22.0);
                                if ui.small_button("Select all").clicked() {
                                    self.editor.select_layer(id);
                                }
                                if ui
                                    .add_enabled(
                                        has_selection,
                                        egui::Button::new("Move here").small(),
                                    )
                                    .on_hover_text("Move the selected geometry onto this layer")
                                    .clicked()
                                {
                                    self.editor.move_selection_to_layer(id);
                                }
                            });
                        });
                    if edited != layer {
                        self.editor.update_layer(id, edited);
                    }
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_layer_name)
                            .hint_text("new layer")
                            .desired_width(110.0),
                    );
                    if ui.button("Add layer").clicked() {
                        let n = self.editor.doc.layers().count();
                        let name = if self.new_layer_name.trim().is_empty() {
                            format!("Layer {n}")
                        } else {
                            std::mem::take(&mut self.new_layer_name)
                        };
                        let id = self.editor.add_layer(&name);
                        self.editor.set_current_layer(id);
                    }
                });
            });
        if let Some(text) = status {
            self.set_status(text, true, now);
        }
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
        egui::Grid::new("appearance").num_columns(2).show(ui, |ui| {
            ui.label("Layer");
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
            ui.label("Colour");
            if let Some(c) = color_choice(ui, a.color, "By layer", dark) {
                self.editor.update_selection_appearance(|x| x.color = c);
            }
            ui.end_row();
            ui.label("Weight");
            let mut w = a.weight;
            if weight_combo(ui, "selection_weight", &mut w, true) {
                self.editor.update_selection_appearance(|x| x.weight = w);
            }
            ui.end_row();
            ui.label("Line type");
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
