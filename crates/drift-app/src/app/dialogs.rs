//! Modal prompts and windows.

use crate::*;

impl DriftApp {
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        self.underlay_dialog(ctx);
        if let Some((constraint, placement)) = self.editor.pending_driven {
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("driven")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading("Make this dimension driven?");
                ui.add_space(6.0);
                ui.label(format!(
                    "This {} would over-define the sketch. Add it as a driven (reference) \
                     dimension that shows the measurement without constraining it?",
                    constraint.type_name().to_lowercase()
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Add as driven").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(false);
            }
            if let Some(add) = choice {
                self.editor.pending_driven = None;
                if add && let Err(e) = self.editor.add_driven_dimension(constraint, placement) {
                    self.editor.notice = Some(format!("{}: {e}", constraint.type_name()));
                }
            }
        }

        if let Some(action) = self.pending {
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("discard")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.heading("Discard unsaved changes?");
                ui.add_space(6.0);
                ui.label(format!(
                    "{} has changes that haven't been saved.",
                    self.file.name
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard changes").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(false);
            }
            if let Some(discard) = choice {
                self.pending = None;
                if discard {
                    self.perform(action);
                }
            }
        }

        let mut open = self.benchmark.is_some();
        egui::Window::new("Solver benchmark")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(format!("Running on {}", self.backend));
                for line in self.benchmark.iter().flatten() {
                    ui.monospace(line);
                }
            });
        if !open {
            self.benchmark = None;
        }

        egui::Window::new("Keyboard shortcuts")
            .open(&mut self.show_shortcuts)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        for (keys, action) in SHORTCUTS {
                            ui.label(
                                egui::RichText::new(*keys)
                                    .monospace()
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.label(*action);
                            ui.end_row();
                        }
                    });
            });
    }

    /// A cheap fingerprint of the state the panels display.
    pub(crate) fn ui_signature(&self) -> impl PartialEq + use<> {
        let sel = &self.editor.selection;
        (
            self.tool.kind(),
            self.editor.doc.revision(),
            (sel.len(), sel.first().copied(), sel.last().copied()),
            self.editor.hovered,
            self.grid_snap,
            (self.pending, self.editor.pending_driven.is_some()),
            self.editor.doc.constraint_count(),
            self.selected_constraint,
            self.hovered_constraint,
            self.editor.hovered_midpoint,
            self.editor.selected_midpoints.len(),
            self.typing.as_ref().map(|t| (t.values.clone(), t.active)),
        )
    }

    pub(crate) fn update_title(&mut self, ctx: &egui::Context) {
        let dirty = if self.is_dirty() { "• " } else { "" };
        let title = format!("{dirty}{} — Drift", self.file.name);
        if title != self.window_title {
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            // eframe's web backend doesn't implement the title command; set it directly.
            #[cfg(target_arch = "wasm32")]
            if let Some(document) = eframe::web_sys::window().and_then(|w| w.document()) {
                let _ = ctx;
                document.set_title(&title);
            }
            self.window_title = title;
        }
    }
}
