//! F2: rename the picked block or group in a small dialog.

use crate::blocks::RenameTarget;
use crate::*;

/// The Rename dialog, while open.
pub(crate) struct Renaming {
    target: RenameTarget,
    name: String,
    /// Why the last try was refused.
    error: Option<String>,
    /// The first frame: focus the field with the old name selected.
    fresh: bool,
}

impl DriftApp {
    /// Open the Rename dialog for what F2 renames, or say what can be renamed.
    pub(crate) fn start_rename(&mut self) {
        match self.editor.rename_target() {
            Some(target) => {
                self.renaming = Some(Renaming {
                    target,
                    name: self.editor.name_of(target).unwrap_or_default(),
                    error: None,
                    fresh: true,
                });
            }
            None => {
                self.editor.notice =
                    Some("Pick a group, or copies of one block, to rename (F2)".into());
            }
        }
    }

    pub(crate) fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(r) = &mut self.renaming else {
            return;
        };
        let what = match r.target {
            RenameTarget::Block(_) => "block",
            RenameTarget::Group(_) => "group",
        };
        let field_id = egui::Id::new("rename_field");
        let mut apply = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("rename"))
            .frame(theme::dialog_frame(&ctx.style_of(ctx.theme())))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                theme::dialog_title(ui, &format!("Rename {what}"), false);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut r.name)
                        .id(field_id)
                        .desired_width(f32::INFINITY),
                );
                if r.fresh {
                    r.fresh = false;
                    field.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field_id) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(r.name.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), field_id);
                    }
                }
                if field.changed() {
                    r.error = None;
                }
                if let Some(e) = &r.error {
                    ui.colored_label(egui::Color32::from_rgb(0xff, 0x5d, 0x5d), e);
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    let dark = ui.visuals().dark_mode;
                    if ui.add(theme::primary_button("Rename", dark)).clicked() {
                        apply = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if apply {
            let (target, name) = (r.target, r.name.clone());
            match self.editor.rename(target, &name) {
                Ok(()) => self.renaming = None,
                Err(e) => {
                    if let Some(r) = &mut self.renaming {
                        r.error = Some(e);
                    }
                }
            }
        } else if cancel {
            self.renaming = None;
        }
    }
}
