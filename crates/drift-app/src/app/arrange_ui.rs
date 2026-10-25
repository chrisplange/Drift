//! Arrange: what is picked brought forward or sent backward in its layer's stack.

use crate::stack::Arrange;
use crate::*;

impl DriftApp {
    /// The shortcut an Arrange command has (from the command registry), as drawing programs
    /// have it: Ctrl+] and Ctrl+[ one step, with Shift all the way.
    pub(crate) fn arrange_keys(how: Arrange) -> String {
        crate::commands::spec(app::palette::Command::Arrange(how))
            .map_or_else(String::new, |s| s.key_text(cfg!(target_os = "macos")))
    }

    /// Move the picked fills and shapes in their layers' stacks, or say why nothing moved.
    pub(crate) fn arrange(&mut self, how: Arrange, now: f64) {
        if !self.editor.can_arrange() {
            self.set_status("Pick a fill or a shape to arrange".into(), true, now);
        } else if !self.editor.arrange(how) {
            let place = if matches!(how, Arrange::ToFront | Arrange::Forward) {
                "top"
            } else {
                "bottom"
            };
            self.set_status(format!("Already at the {place} of its layer"), false, now);
        }
    }

    /// The canvas menu's Arrange submenu.
    pub(crate) fn arrange_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Arrange", |ui| {
            self.category_menu(ui, crate::commands::Category::Arrange, &[]);
        });
    }

    /// The Inspector's Arrange buttons, for picked fills and shapes.
    pub(crate) fn arrange_row(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (how, short) in [
                (Arrange::ToFront, "To front"),
                (Arrange::Forward, "Forward"),
                (Arrange::Backward, "Backward"),
                (Arrange::ToBack, "To back"),
            ] {
                if ui
                    .button(short)
                    .on_hover_text(format!(
                        "{} on its layer ({})",
                        how.label(),
                        Self::arrange_keys(how)
                    ))
                    .clicked()
                {
                    self.arrange(how, now);
                }
            }
        });
    }
}
