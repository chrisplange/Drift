//! The inspector for sketch text.

use drift_core::TextFont;

use crate::*;

impl DriftApp {
    /// Edit the picked text: its words, size, angle and font; convert it to curves.
    pub(crate) fn text_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(id) = self.editor.selected_text else {
            return false;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            self.editor.selected_text = None;
            return false;
        };
        let units = self.editor.doc.units();
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Text").family(theme::bold()).size(15.0));
        ui.add_space(4.0);
        let mut words = t.text.clone();
        let edit = ui.add(
            egui::TextEdit::singleline(&mut words)
                .hint_text("Type the text")
                .desired_width(f32::INFINITY),
        );
        // Just placed with the Text tool: type straight away.
        if self.editor.editing_text == Some(id) {
            edit.request_focus();
            self.editor.editing_text = None;
        }
        if edit.changed() {
            // Typing is one undo step per visit to the field.
            let continuing = self.text_typing == Some(id);
            self.editor.type_text(id, &words, continuing);
            self.text_typing = Some(id);
        }
        if edit.lost_focus() {
            self.text_typing = None;
        }
        let unit = units.length.mm_per_unit();
        let mut edited = t.clone();
        theme::row(ui, "Height", |ui| {
            let mut shown = t.height / unit;
            if ui
                .add(
                    egui::DragValue::new(&mut shown)
                        .speed(0.1)
                        .range(0.001..=1e6)
                        .max_decimals(4)
                        .suffix(format!(" {}", units.length.symbol())),
                )
                .on_hover_text("Height of capital letters")
                .changed()
            {
                edited.height = shown * unit;
            }
        });
        theme::row(ui, "Angle", |ui| {
            let mut a = t.angle;
            if ui
                .add(egui::DragValue::new(&mut a).suffix("°").speed(0.5))
                .changed()
            {
                edited.angle = a.rem_euclid(360.0);
            }
            for q in [0.0, 90.0, 180.0, 270.0] {
                if ui.small_button(format!("{q:.0}")).clicked() {
                    edited.angle = q;
                }
            }
        });
        theme::row(ui, "Font", |ui| {
            egui::ComboBox::from_id_salt(("text_font", id.0))
                .selected_text(t.font.label())
                .show_ui(ui, |ui| {
                    for f in TextFont::ALL {
                        ui.selectable_value(&mut edited.font, f, f.label());
                    }
                });
        });
        if edited != t {
            self.editor.text_height = edited.height;
            self.editor.update_text(id, |x| *x = edited);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Convert to curves")
                .on_hover_text("Replace the text with its outlines as curves and lines (grouped)")
                .clicked()
            {
                let n = self.editor.convert_text(id);
                let now = ui.input(|i| i.time);
                self.set_status(format!("Converted the text into {n} curves"), false, now);
            }
            if ui.button("Delete").clicked() {
                self.editor.remove_text(id);
            }
        });
        true
    }
}
