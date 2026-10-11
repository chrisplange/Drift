//! The inspector for sketch text.

use drift_core::TextFont;

use crate::*;

impl DriftApp {
    /// Typing a text in place: a field at its start on the canvas while the outlines update
    /// live above it. Return, Esc or a click elsewhere finishes.
    pub(crate) fn text_editor(&mut self, ctx: &egui::Context, map: &ScreenMap) {
        let Some(id) = self.editor.editing_text else {
            self.text_focused = None;
            return;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            self.editor.editing_text = None;
            return;
        };
        let at = map.to_screen(t.at);
        let field = egui::Id::new("text_edit_field");
        let mut done = false;
        let style = ctx.style_of(ctx.theme());
        egui::Area::new(egui::Id::new("text_edit"))
            .fixed_pos(at + egui::vec2(-6.0, 8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(&style)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        let mut words = t.text.clone();
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut words)
                                .id(field)
                                .hint_text("Type the text, then Enter")
                                .char_limit(crate::texts::MAX_TEXT_CHARS)
                                .desired_width(240.0),
                        );
                        if self.text_focused != Some(id) {
                            response.request_focus();
                            self.text_focused = Some(id);
                        } else if response.lost_focus() {
                            done = true;
                        }
                        if response.changed() {
                            // Typing is one undo step per visit to the field.
                            let continuing = self.text_typing == Some(id);
                            self.editor.type_text(id, &words, continuing);
                            self.text_typing = Some(id);
                        }
                    });
            });
        if done {
            self.editor.editing_text = None;
            self.text_typing = None;
            self.text_focused = None;
        }
    }

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
        let layer = self.editor.doc.layer(t.layer).map_or(String::new(), |l| {
            format!("{} · on layer {}", t.font.label(), l.name)
        });
        app::panels::title(ui, Some(ToolKind::Text), "Text", &layer);
        ui.add_space(4.0);
        let mut words = t.text.clone();
        let edit = ui.add(
            egui::TextEdit::singleline(&mut words)
                .hint_text("Type the text")
                .char_limit(crate::texts::MAX_TEXT_CHARS)
                .desired_width(f32::INFINITY),
        );
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
        });
        // On its own row, so a narrow dock isn't pushed wider.
        theme::row(ui, "", |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
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
