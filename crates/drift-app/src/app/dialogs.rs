//! Modal prompts and windows.

use crate::*;

/// Keys as keycaps ("Ctrl+Z / Ctrl+Y"); descriptions ("Click inside a closed area") as text.
pub(crate) fn keycaps(ui: &mut egui::Ui, keys: &str) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, piece) in keys.split(" / ").enumerate() {
        if i > 0 {
            ui.label(egui::RichText::new("/").weak());
        }
        let is_key = !piece.contains(' ') || piece.len() <= 3;
        if !is_key {
            ui.label(egui::RichText::new(piece).weak());
            continue;
        }
        let text = egui::RichText::new(piece).size(theme::size::CAPTION + 0.5);
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        );
        let size = galley.size() + egui::vec2(12.0, 6.0);
        let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let v = ui.visuals();
        ui.painter().rect(
            r,
            theme::radius::CONTROL,
            v.faint_bg_color,
            egui::Stroke::new(1.0, theme::hairline(ui)),
            egui::StrokeKind::Inside,
        );
        // A deeper bottom edge, like a key.
        ui.painter().line_segment(
            [
                r.left_bottom() + egui::vec2(3.0, -0.5),
                r.right_bottom() + egui::vec2(-3.0, -0.5),
            ],
            egui::Stroke::new(1.5, theme::hairline(ui)),
        );
        ui.painter()
            .galley(r.min + egui::vec2(6.0, 3.0), galley, v.text_color());
    }
}

impl DriftApp {
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        self.underlay_dialog(ctx);
        self.vector_import_dialog(ctx);
        self.start_panel(ctx);
        self.about_window(ctx);
        self.licences_window(ctx);
        self.settings_window(ctx);
        self.drawing_settings_window(ctx);
        self.relation_pie(ctx);
        self.rename_dialog(ctx);
        self.import_report_window(ctx);
        #[cfg(not(feature = "demo"))]
        self.output_window(ctx);
        #[cfg(feature = "demo")]
        self.demo_note(ctx);
        self.sketch_check_window(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.update_notice(ctx);
        if let Some((constraint, placement)) = self.editor.pending_driven {
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("driven"))
                .frame(theme::dialog_frame(&ctx.style_of(ctx.theme())))
                .show(ctx, |ui| {
                    ui.set_width(360.0);
                    theme::dialog_title(ui, "Make this dimension driven?", false);
                    ui.label(format!(
                        "This {} would over-define the sketch. Add it as a driven (reference) \
                     dimension that shows the measurement without constraining it?",
                        constraint.type_name().to_lowercase()
                    ));
                    theme::dialog_buttons(ui, |ui| {
                        if ui.button("Cancel").clicked() {
                            choice = Some(false);
                        }
                        let dark = ui.visuals().dark_mode;
                        if ui
                            .add(theme::primary_button("Add as driven", dark))
                            .clicked()
                        {
                            choice = Some(true);
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
            #[derive(PartialEq)]
            enum Choice {
                Save,
                Discard,
                Cancel,
            }
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("discard"))
                .frame(theme::dialog_frame(&ctx.style_of(ctx.theme())))
                .show(ctx, |ui| {
                ui.set_width(380.0);
                theme::dialog_title(ui, &format!("Save changes to {}?", self.file.name), false);
                ui.label(match action {
                    Pending::Quit => "Your changes will be lost if you close without saving.",
                    #[cfg(not(target_arch = "wasm32"))]
                    Pending::Update => {
                        "Drift closes to install the update. Your changes will be lost if you don't save them."
                    }
                    _ => "Your changes will be lost if you don't save them.",
                });
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(Choice::Cancel);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    let dark = ui.visuals().dark_mode;
                    if ui.add(theme::primary_button("Save", dark)).clicked() {
                        choice = Some(Choice::Save);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(Choice::Cancel);
            }
            if let Some(choice) = choice {
                self.pending = None;
                match choice {
                    Choice::Save => {
                        // Carry on once the file is written (a cancelled save dialog
                        // leaves things as they are).
                        self.after_save = Some((action, self.editor.doc.revision()));
                        self.save();
                    }
                    Choice::Discard => {
                        self.discard_session = action == Pending::Quit;
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.discard_session |= action == Pending::Update;
                        }
                        self.perform(action);
                    }
                    Choice::Cancel => {}
                }
            }
        }

        let mut open = self.benchmark.is_some();
        let mut close = false;
        theme::dialog(ctx, "Solver benchmark")
            .open(&mut open)
            .show(ctx, |ui| {
                close = theme::dialog_title(ui, "Solver benchmark", true);
                ui.label(format!("Running on {}", self.backend));
                for line in self.benchmark.iter().flatten() {
                    ui.monospace(line);
                }
            });
        if !open || close {
            self.benchmark = None;
        }

        let mut open = self.show_shortcuts;
        let mut close = false;
        let filter = &mut self.shortcut_filter;
        theme::dialog(ctx, "Keyboard shortcuts")
            .open(&mut open)
            .resizable(true)
            .default_size([660.0, 560.0])
            .show(ctx, |ui| {
                if theme::dialog_title(ui, "Keyboard shortcuts", true) {
                    close = true;
                }
                let field = ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text("Filter, e.g. \"dimension\" or \"Ctrl\"")
                        .desired_width(f32::INFINITY),
                );
                // Focus the filter as the window opens.
                let seen = egui::Id::new("shortcuts_seen");
                if !ui.data(|d| d.get_temp::<bool>(seen).unwrap_or(false)) {
                    field.request_focus();
                    ui.data_mut(|d| d.insert_temp(seen, true));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                let query = filter.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut any = false;
                        for (section, rows) in SHORTCUTS {
                            let rows: Vec<&(&str, &str)> = rows
                                .iter()
                                .filter(|(k, a)| {
                                    query.is_empty()
                                        || k.to_lowercase().contains(&query)
                                        || a.to_lowercase().contains(&query)
                                        || section.to_lowercase().contains(&query)
                                })
                                .collect();
                            if rows.is_empty() {
                                continue;
                            }
                            any = true;
                            theme::section(ui, section);
                            for (keys, action) in rows {
                                ui.horizontal(|ui| {
                                    // Keys in a column just wide enough for the longer
                                    // ones ("Ctrl+Shift+Z"); the description beside them.
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(150.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center)
                                            .with_main_wrap(true),
                                        |ui| {
                                            ui.set_min_width(150.0);
                                            keycaps(ui, keys);
                                        },
                                    );
                                    ui.add(egui::Label::new(*action).wrap());
                                });
                                ui.add_space(3.0);
                            }
                        }
                        if !any {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("No shortcut matches").weak());
                        }
                    });
            });
        self.show_shortcuts = open && !close;
        if !self.show_shortcuts {
            ctx.data_mut(|d| d.remove::<bool>(egui::Id::new("shortcuts_seen")));
        }
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
