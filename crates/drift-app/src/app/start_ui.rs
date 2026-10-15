//! The start panel over an empty sketch (new, open, recent files, a few tips), the About
//! window, and Preferences.

use crate::*;

/// The version shown to users, e.g. "0.1.0-alpha.1".
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

impl DriftApp {
    /// Whether the start panel should show: nothing drawn, nothing opened, nothing under way.
    fn start_wanted(&self) -> bool {
        self.show_start
            && self.editor.doc.is_empty()
            && self.editor.doc.texts().next().is_none()
            && self.editor.doc.underlays().next().is_none()
            && self.file.path.is_none()
            && self.tool.kind() == ToolKind::Select
            && !self.sheet_preview
            && self.pending.is_none()
            && self.palette.is_none()
    }

    /// The start panel, centred on the canvas. It goes as soon as there's something to work
    /// on, a tool is picked, or the canvas is clicked.
    pub(crate) fn start_panel(&mut self, ctx: &egui::Context) {
        if !self.show_start {
            return;
        }
        if !self.start_wanted() {
            // Hidden while a prompt or the palette is up; gone for good once work starts.
            if self.pending.is_none() && self.palette.is_none() {
                self.show_start = false;
            }
            return;
        }
        let canvas = self.canvas_rect;
        if !canvas.is_positive() || canvas.width() < 460.0 || canvas.height() < 380.0 {
            return;
        }
        let now = ctx.input(|i| i.time);
        let mut action = None;
        let mut close = false;
        let style = ctx.style_of(ctx.theme());
        let shown = egui::Area::new(egui::Id::new("start_panel"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(canvas.center())
            .show(ctx, |ui| {
                egui::Frame::popup(&style)
                    .corner_radius(14.0)
                    .inner_margin(egui::Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(400.0);
                        ui.horizontal(|ui| {
                            let (logo, _) = ui
                                .allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                            icons::logo(ui.painter(), logo);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(
                                    egui::RichText::new("Drift")
                                        .family(theme::bold())
                                        .size(22.0),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Parametric 2D sketching · {VERSION}"
                                    ))
                                    .small()
                                    .weak(),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("×").size(16.0))
                                            .frame(false),
                                    )
                                    .on_hover_text("Close")
                                    .clicked()
                                {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let accent = theme::accent(ui.visuals().dark_mode);
                            let new = egui::Button::new(
                                egui::RichText::new("New sketch").color(egui::Color32::WHITE),
                            )
                            .fill(accent)
                            .min_size(egui::vec2(0.0, 28.0));
                            if ui
                                .add(new)
                                .on_hover_text("Start drawing: pick a tool, or press L for a line")
                                .clicked()
                            {
                                close = true;
                            }
                            let big = |label: &str| {
                                egui::Button::new(label).min_size(egui::vec2(0.0, 28.0))
                            };
                            if ui.add(big("Open…")).clicked() {
                                action = Some(Pending::Open);
                            }
                            if ui
                                .add(big("Open the example"))
                                .on_hover_text("A dimensioned mounting plate to explore")
                                .clicked()
                            {
                                action = Some(Pending::Example);
                            }
                        });
                        theme::section(ui, "Recent");
                        if self.recent.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "Sketches you open or save will be listed here.",
                                )
                                .small()
                                .weak(),
                            );
                        }
                        for (i, path) in self.recent.iter().take(6).enumerate() {
                            let name = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let folder = path
                                .parent()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let (r, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 26.0),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                            });
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    r,
                                    6.0,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            let painter = ui.painter_at(r);
                            let g = painter.layout_no_wrap(
                                name.clone(),
                                egui::FontId::proportional(13.5),
                                ui.visuals().text_color(),
                            );
                            let x = r.left() + 8.0 + g.size().x + 10.0;
                            painter.galley(
                                egui::pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0),
                                g,
                                ui.visuals().text_color(),
                            );
                            painter.text(
                                egui::pos2(x, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                &folder,
                                egui::FontId::proportional(11.5),
                                ui.visuals().weak_text_color(),
                            );
                            if response.on_hover_text(path.to_string_lossy()).clicked() {
                                action = Some(Pending::Recent(i));
                            }
                        }
                        theme::section(ui, "Good to know");
                        for (keys, what) in [
                            (
                                app::panels::shortcut("K", false),
                                "Find any tool, setting or command",
                            ),
                            ("?".to_owned(), "Every keyboard shortcut"),
                            (
                                "Right-click".to_owned(),
                                "A menu for the selection, with relations and dimensions",
                            ),
                        ] {
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(92.0, 22.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(92.0);
                                        app::dialogs::keycaps(ui, &keys);
                                    },
                                );
                                ui.label(egui::RichText::new(what).weak());
                            });
                        }
                    });
            });
        // A press on the canvas beside the panel starts work.
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| canvas.contains(p) && !shown.response.rect.contains(p))
        });
        if close || pressed_outside {
            self.show_start = false;
        }
        if let Some(action) = action {
            self.request(action);
            let _ = now;
        }
    }

    /// Help → About Drift: version and system details, for bug reports.
    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let details = format!(
            "Drift {VERSION}\nRenderer: {}\nSystem: {} ({})\nMemory in use: {} MB",
            self.backend,
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.memory.0 / (1024 * 1024)
        );
        let mut copied = false;
        egui::Window::new("About Drift")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(340.0);
                ui.horizontal(|ui| {
                    let (logo, _) =
                        ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                    icons::logo(ui.painter(), logo);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Drift")
                                .family(theme::bold())
                                .size(22.0),
                        );
                        ui.label(format!("Version {VERSION}"));
                        ui.label(egui::RichText::new("Parametric 2D sketching").weak());
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "This is an alpha: save often, and expect rough edges. Your sketch is \
                         also kept between sessions automatically.",
                    )
                    .small()
                    .weak(),
                );
                theme::section(ui, "Details");
                for line in details.lines().skip(1) {
                    if let Some((k, v)) = line.split_once(": ") {
                        theme::row(ui, k, |ui| {
                            ui.label(v);
                        });
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Copy details")
                        .on_hover_text("For a bug report")
                        .clicked()
                    {
                        ui.ctx().copy_text(details.clone());
                        copied = true;
                    }
                    ui.hyperlink_to("Project page", env!("CARGO_PKG_REPOSITORY"));
                });
            });
        if copied {
            let now = ctx.input(|i| i.time);
            self.set_status("Copied the details".into(), false, now);
        }
        self.show_about = open;
    }

    /// Edit → Preferences: the settings from the View menu, status bar and Parameters panel
    /// in one place.
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new("Preferences")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(360.0);
                let room = (ctx.content_rect().height() - 140.0).max(200.0);
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .min_scrolled_height(room.min(620.0))
                    .show(ui, |ui| {
                        ui.push_id("preferences", |ui| self.settings(ui));
                    });
            });
        self.show_settings = open;
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        // Clear of the scroll bar.
        ui.set_max_width(ui.available_width() - 10.0);
        theme::section(ui, "Appearance");
        ui.horizontal(|ui| {
            egui::widgets::global_theme_preference_buttons(ui);
        });
        self.units_section(ui);
        theme::section(ui, "Grid and snapping");
        ui.checkbox(&mut self.show_grid, "Show the grid");
        ui.checkbox(&mut self.show_axes, "Show the origin axes");
        ui.checkbox(&mut self.grid_snap, "Snap to the grid")
            .on_hover_text("Arrow keys also move the selection by one grid step");
        ui.checkbox(
            &mut self.auto_relations,
            "Add relations automatically while drawing",
        )
        .on_hover_text("Horizontal, vertical, coincident, tangent and so on, as you snap");
        ui.checkbox(
            &mut self.round_scale,
            "Round sizes when scaling with dimensions (Alt)",
        )
        .on_hover_text(
            "A scale handle with Alt steps the picked geometry's width and height through \
             round sizes in the document's units, finer as you zoom in. Off: any size",
        );
        theme::section(ui, "Dimensions");
        ui.checkbox(
            &mut self.show_temp_dims,
            "Show temporary dimensions on the picked geometry",
        )
        .on_hover_text(
            "Pick one line, circle or arc to see its size and how far it is from its              neighbours. Click a value to type a new one (the picked geometry moves), the lock              to keep it as a dimension, and drag the square at its far end onto other geometry              to measure to that",
        );
        theme::section(ui, "Closed areas");
        ui.checkbox(
            &mut self.editor.regions_by_layer,
            "Find closed areas on each layer separately",
        )
        .on_hover_text(
            "On: an area is closed only by curves on one layer, so furniture, notes or \
             hatching on other layers don't cut it up or close it. Its outline must then be \
             all on one layer (a door line on another layer leaves a wall open)",
        );
        ui.checkbox(
            &mut self.editor.regions_need_fill,
            "Pick an area only where it is filled or hatched",
        )
        .on_hover_text(
            "On: a click inside a solid fill or hatch picks it, and a click inside an empty \
             outline picks nothing (box selection starts there). Off: it picks the area, to \
             measure it",
        );
        theme::section(ui, "Mouse and trackpad");
        theme::row(ui, "Scrolling", |ui| {
            for mode in crate::app::view::WheelMode::ALL {
                ui.selectable_value(&mut self.wheel.mode, mode, mode.label())
                    .on_hover_text(match mode {
                        crate::app::view::WheelMode::Auto => {
                            "A mouse wheel zooms; two fingers on a trackpad pan, and a pinch zooms"
                        }
                        crate::app::view::WheelMode::Zoom => {
                            "Scrolling always zooms (if a trackpad isn't recognised as one, or you prefer it)"
                        }
                        crate::app::view::WheelMode::Pan => {
                            "Scrolling always pans; Ctrl + scroll zooms"
                        }
                    });
            }
        });
        ui.label(
            egui::RichText::new("The middle or right mouse button drags the view either way.")
                .small()
                .weak(),
        );
        theme::section(ui, "Display");
        ui.checkbox(
            &mut self.status_colors,
            "Colour curves by constraint status",
        )
        .on_hover_text("Off: each curve's own colour, or its layer's");
        ui.checkbox(
            &mut self.thin_lines,
            "Thin lines (every line one pixel wide)",
        );
        ui.checkbox(&mut self.always_endpoints, "Always show end points")
            .on_hover_text("Off: only on hovered or picked curves");
        ui.checkbox(&mut self.show_dimensions, "Show dimensions and notes");
        ui.checkbox(&mut self.show_relations, "Show relations");
        ui.checkbox(&mut self.show_combs, "Curvature combs on picked curves");
        theme::section(ui, "Window");
        ui.horizontal(|ui| {
            if ui.button("Reset panel layout").clicked() {
                self.layout = Default::default();
                self.dock_view.hide_all = false;
            }
            if ui
                .button("Show the start panel")
                .on_hover_text("It appears over an empty, unsaved sketch")
                .clicked()
            {
                self.show_start = true;
            }
        });
        theme::section(ui, "Memory");
        let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
        ui.label(
            egui::RichText::new(format!(
                "Using {:.2} GB. Drift warns at {:.0} GB and closes itself at {:.0} GB, so a \
                 fault can't exhaust the computer; the sketch is restored next time.",
                gb(self.memory.0),
                gb(drift_platform::memory::WARN_BYTES),
                gb(drift_platform::memory::limit())
            ))
            .small()
            .weak(),
        );
        ui.add_space(4.0);
    }
}
