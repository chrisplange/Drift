//! The Inspector for picked offsets (#247): one distance, shown or not, and Detach.

use drift_core::OffsetId;

use crate::*;

impl DriftApp {
    /// The Inspector for picked offsets. False when no offset is picked.
    pub(crate) fn offset_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let offsets: Vec<OffsetId> = self.drawing.editor.selected_offsets().into_iter().collect();
        let Some(first) = offsets
            .first()
            .and_then(|x| self.drawing.editor.doc.offset(*x))
            .cloned()
        else {
            return false;
        };
        // (Only the offsets: with other curves picked too, the mixed Inspector shows.)
        let pieces: std::collections::BTreeSet<drift_core::EntityId> = offsets
            .iter()
            .flat_map(|x| self.drawing.editor.offset_pick_set(*x))
            .collect();
        if self
            .drawing
            .editor
            .selection
            .iter()
            .any(|id| !pieces.contains(id))
        {
            return false;
        }
        let doc = &self.drawing.editor.doc;
        let units = doc.units();
        let source = first.source.iter().filter(|c| doc.contains(**c)).count();
        let layer = first
            .made
            .curves
            .values()
            .next()
            .map(|c| doc.layer_of(*c))
            .map_or(String::new(), |l| format!("On layer {}", l.name));
        let name = match offsets.len() {
            1 => format!("Offset {}", offsets[0]),
            n => format!("{n} offsets"),
        };
        app::panels::title(ui, Some(ToolKind::Offset), &name, &layer);
        theme::section(ui, "Offset");
        // Distance: a value or an expression of variables.
        let same = offsets
            .iter()
            .filter_map(|x| doc.offset(*x))
            .all(|o| o.distance == first.distance && o.distance_expr == first.distance_expr);
        let current = if same {
            first
                .distance_expr
                .clone()
                .unwrap_or_else(|| units.format_readout(first.distance))
        } else {
            String::new()
        };
        let mut text = match &self.drawing.offset_distance_text {
            Some((for_offsets, t)) if *for_offsets == offsets => t.clone(),
            _ => current.clone(),
        };
        let mut commit = None;
        theme::row(ui, "Distance", |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width() - 8.0)
                    .hint_text("various"),
            );
            if r.changed() {
                self.drawing.offset_distance_text = Some((offsets.clone(), text.clone()));
            }
            if r.lost_focus() && text != current {
                commit = Some(text.clone());
            }
        });
        if let Some(t) = commit {
            self.drawing.offset_distance_text = None;
            if let Err(e) = self.drawing.editor.set_offset_distance(&offsets, &t) {
                self.drawing.editor.out.notice = Some(e.to_string());
            }
        }
        let doc = &self.drawing.editor.doc;
        theme::row(ui, "Follows", |ui| {
            ui.label(format!(
                "{source} {} · {} pieces",
                if source == 1 { "curve" } else { "curves" },
                first.made.curves.len()
            ));
        });
        // Layer: the pieces', whichever was picked.
        let layers: Vec<(drift_core::LayerId, String)> =
            doc.layers().map(|(id, l)| (id, l.name.clone())).collect();
        let on = first
            .made
            .curves
            .values()
            .next()
            .and_then(|c| doc.get(*c))
            .map_or(doc.current_layer(), |e| e.layer);
        let mut to = on;
        theme::row(ui, "Layer", |ui| {
            let current = layers
                .iter()
                .find(|(id, _)| *id == on)
                .map_or("?".to_owned(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("offset_layer")
                .selected_text(current)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for (id, n) in &layers {
                        ui.selectable_value(&mut to, *id, n);
                    }
                });
        });
        if to != on {
            self.drawing.editor.move_selection_to_layer(to);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let shown = offsets.iter().all(|x| {
                self.drawing
                    .editor
                    .doc
                    .property_dims()
                    .any(|(_, d)| d.target == drift_core::PropertyRef::OffsetDistance(*x))
            });
            if !shown
                && ui
                    .button("Show distance")
                    .on_hover_text("A dimension of its distance: type into it to change it")
                    .clicked()
            {
                for x in &offsets {
                    self.drawing.editor.show_offset_distance(*x);
                }
            }
            if ui
                .button("Detach")
                .on_hover_text("Plain curves that no longer follow the source")
                .clicked()
            {
                self.drawing.editor.detach_offsets(&offsets);
            }
        });
        true
    }
}
