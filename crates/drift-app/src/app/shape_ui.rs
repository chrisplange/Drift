//! A filled shape in the Inspector as one object, as a drawing program shows it: its Stroke
//! (the outline's colour, weight and line type, or none) and its Fill (none, solid or
//! hatch). Area fills (bounded by several shapes' lines) keep the fill's own inspector.

use std::collections::BTreeSet;

use drift_core::{EntityId, LineType};

use crate::app::fill_ui::{FillEdit, fill_style_editor_or_none};
use crate::*;
use drift_edit::editor::PickedShapes;
use drift_edit::fills::FillStyle;

impl DriftApp {
    /// The picked filled shapes when they are all that is picked (and nothing but shapes'
    /// fills and outlines): the Inspector shows them as objects.
    pub(crate) fn picked_shapes(&self) -> Option<PickedShapes> {
        let ed = &self.editor;
        if ed.selected_fills.is_empty() && ed.selection.is_empty() {
            return None;
        }
        // Another fill picked (an area fill) keeps the fill's inspector.
        let shapes = self.editor.shapes_in_selection();
        if shapes.fills.is_empty()
            || ed.selected_fills.iter().any(|f| !shapes.fills.contains(f))
            || !ed.selected_instances.is_empty()
            || !ed.selected_texts.is_empty()
        {
            return None;
        }
        let outline: BTreeSet<EntityId> = shapes.curves.iter().copied().collect();
        let only_outlines = ed
            .selection
            .iter()
            .filter(|id| ed.doc.get(**id).is_some_and(|e| !e.kind.is_point()))
            .all(|id| outline.contains(id));
        only_outlines.then_some(shapes)
    }

    /// The Inspector for picked filled shapes. Returns whether it showed.
    pub(crate) fn shape_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(shapes) = self.picked_shapes() else {
            return false;
        };
        let first = shapes.fills[0];
        let Some(fill) = self.editor.doc.fill(first).cloned() else {
            return false;
        };
        let (name, detail) = match shapes.fills.len() {
            1 => {
                // A lone circle or ellipse goes by its name; anything else is a shape.
                let name = match shapes.curves.as_slice() {
                    [one] => self
                        .editor
                        .doc
                        .get(*one)
                        .map_or("Shape", |e| e.kind.type_name()),
                    _ => "Shape",
                };
                let layer = self
                    .editor
                    .doc
                    .layer(fill.layer)
                    .map_or(String::new(), |l| format!("On layer {}", l.name));
                (name.to_owned(), layer)
            }
            n => (format!("{n} shapes"), String::new()),
        };
        app::panels::title(ui, Some(ToolKind::Rectangle), &name, &detail);
        self.group_inspector(ui);
        self.fill_notices(ui, first);

        theme::section(ui, "Stroke");
        self.stroke_section(ui, &shapes.curves);

        theme::section(ui, "Fill");
        if self.shapes_fill_section(ui, &shapes) {
            return true;
        }

        theme::section(ui, "Layer");
        self.shape_layer_row(ui, &shapes);
        theme::section(ui, "Arrange");
        self.arrange_row(ui);
        if let ([_], Some(shape)) = (
            shapes.fills.as_slice(),
            self.editor.fill_shapes().get(&first).cloned(),
        ) {
            let units = self.editor.doc.units();
            let r = &shape.region;
            theme::section(ui, "Size");
            theme::row(ui, "Area", |ui| {
                ui.label(units.format_area(r.area));
            });
            theme::row(ui, "Perimeter", |ui| {
                ui.label(units.format_readout(r.perimeter));
            });
        }
        ui.add_space(10.0);
        if theme::danger_button(ui, "Delete").clicked() {
            // The whole shape: its outline, and with it its fill.
            self.editor.selection.extend(shapes.curves.iter().copied());
            self.editor.delete_selection();
        }
        true
    }

    /// A Fill section for picked shapes (none, solid or hatch). Returns whether their fills
    /// were removed (the shapes stay, picked by their outlines).
    pub(crate) fn shapes_fill_section(&mut self, ui: &mut egui::Ui, shapes: &PickedShapes) -> bool {
        let Some(fill) = shapes
            .fills
            .first()
            .and_then(|f| self.editor.doc.fill(*f))
            .cloned()
        else {
            return false;
        };
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let dark = ui.visuals().dark_mode;
        let mut style = FillStyle::of(&fill);
        // One hatched shape: its pattern's alignment, with the angle among it.
        let aligned =
            shapes.fills.len() == 1 && matches!(fill.kind, drift_core::FillKind::Hatch(_));
        let edit = fill_style_editor_or_none(
            ui,
            &mut style,
            &self.hatch_patterns,
            dark,
            "shape",
            !aligned,
        );
        if aligned && edit != FillEdit::Removed {
            self.pattern_alignment(ui, shapes.fills[0]);
        }
        match edit {
            FillEdit::Unchanged => false,
            FillEdit::Changed => {
                self.editor.doc.begin();
                for id in &shapes.fills {
                    if let Some(mut f) = self.editor.doc.fill(*id).cloned() {
                        style.apply(&mut f);
                        self.editor.doc.set_fill(*id, f);
                    }
                }
                self.editor.commit("Edit fill");
                false
            }
            FillEdit::Removed => {
                self.editor.doc.begin();
                for id in &shapes.fills {
                    self.editor.doc.remove_fill(*id);
                }
                if self.editor.commit("Remove fill") {
                    self.editor.selected_fills.clear();
                    self.editor.selection.extend(shapes.curves.iter().copied());
                }
                true
            }
        }
    }

    /// A Stroke section for `curves`: a line or none, then (with a line) its colour, weight
    /// and line type.
    pub(crate) fn stroke_section(&mut self, ui: &mut egui::Ui, curves: &[EntityId]) {
        let ed = &self.editor;
        let stroked = |id: &EntityId| {
            ed.doc.get(*id).is_some_and(|e| {
                let layer = ed.doc.layer_of(*id);
                e.appearance
                    .line_type
                    .unwrap_or(layer.line_type)
                    .is_stroked()
            })
        };
        let on = curves.iter().any(stroked);
        if let Some(line) = theme::segmented(ui, on, &[(true, "Line"), (false, "None")])
            && line != on
        {
            // None: not drawn or printed. A line again: as its layer draws it.
            self.editor.update_appearance(curves, |a| {
                a.line_type = (!line).then_some(LineType::None);
            });
        }
        if on {
            ui.add_space(4.0);
            egui::Grid::new(("stroke", curves.first()))
                .num_columns(2)
                .min_col_width(theme::LABEL_WIDTH)
                .show(ui, |ui| self.stroke_rows(ui, curves, "shape"));
        } else {
            ui.label(
                egui::RichText::new(
                    "No stroke: the outline isn't drawn or printed, but still bounds the fill, \
                     snaps and holds its relations.",
                )
                .small()
                .weak(),
            );
        }
    }

    /// The shapes' layer: moving them moves their outlines (with their points) and fills.
    fn shape_layer_row(&mut self, ui: &mut egui::Ui, shapes: &PickedShapes) {
        let ed = &self.editor;
        let layers: Vec<(drift_core::LayerId, String)> = ed
            .doc
            .layers()
            .map(|(id, l)| (id, l.name.clone()))
            .collect();
        let current = shapes
            .fills
            .first()
            .and_then(|f| ed.doc.fill(*f))
            .map(|f| f.layer);
        let uniform = shapes
            .fills
            .iter()
            .all(|f| ed.doc.fill(*f).map(|f| f.layer) == current);
        let text = match current.filter(|_| uniform) {
            Some(l) => ed.doc.layer(l).map_or("?".into(), |l| l.name.clone()),
            None => "(mixed)".into(),
        };
        let mut target = None;
        egui::ComboBox::from_id_salt("shape_layer")
            .selected_text(text)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for (id, name) in layers {
                    if ui
                        .selectable_label(uniform && Some(id) == current, name)
                        .clicked()
                    {
                        target = Some(id);
                    }
                }
            });
        // What is picked, as the right-click menu moves it (#266): a shape picked whole
        // takes its fill, a fill picked on its own goes alone.
        if let Some(layer) = target {
            self.editor.move_selection_to_layer(layer);
        }
    }
}
