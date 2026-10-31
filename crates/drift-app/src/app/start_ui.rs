//! The start panel over an empty sketch (new, open, recent files, a few tips), the About
//! window, and Preferences.

use crate::*;

/// The graphics backend as people know it ("DirectX 12" for wgpu's `Dx12`).
fn renderer_name(backend: &str) -> String {
    match backend {
        "Dx12" => "DirectX 12".to_owned(),
        "Vulkan" => "Vulkan".to_owned(),
        "Metal" => "Metal".to_owned(),
        "Gl" => "OpenGL".to_owned(),
        "BrowserWebGpu" => "WebGPU".to_owned(),
        other => other.to_owned(),
    }
}

/// The operating system and processor, as people write them ("Windows · x64").
fn system_name() -> String {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "ARM64",
        "wasm32" => "WebAssembly",
        other => other,
    };
    format!("{os} · {arch}")
}

/// The version shown to users, e.g. "0.1.0-alpha.1".
pub(crate) const VERSION: &str = drift_platform::VERSION;

/// The licences and copyright notices of the fonts and crates built into Drift, made by
/// scripts/notices.py. They also ship as a file beside the program.
const NOTICES: &str = include_str!("../../../../THIRD-PARTY-NOTICES.txt");

impl DriftApp {
    /// Whether the start panel should show: nothing drawn, nothing opened, nothing under way.
    fn start_wanted(&self) -> bool {
        self.show_start
            && self.drawing.editor.doc.is_empty()
            && self.drawing.editor.doc.texts().next().is_none()
            && self.drawing.editor.doc.underlays().next().is_none()
            && self.drawing.file.path.is_none()
            && self.tool.kind() == ToolKind::Select
            && !self.drawing.view.sheet_preview
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
        let canvas = self.drawing.view.canvas_rect;
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
                theme::dialog_frame(&style)
                    .inner_margin(egui::Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(560.0);
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
                                        .size(theme::size::DISPLAY),
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
                                if theme::close_button(ui).clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.horizontal_top(|ui| {
                            // Create: a new sketch (in metric or imperial), or open one.
                            ui.allocate_ui_with_layout(
                                egui::vec2(200.0, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(200.0);
                                    theme::section(ui, "Create");
                                    let dark = ui.visuals().dark_mode;
                                    if ui
                                        .add_sized(
                                            [200.0, 28.0],
                                            theme::primary_button("New sketch", dark),
                                        )
                                        .on_hover_text(
                                            "Start drawing: pick a tool, or press L for a line",
                                        )
                                        .clicked()
                                    {
                                        close = true;
                                    }
                                    ui.add_space(2.0);
                                    let units = self.drawing.editor.doc.units();
                                    let options = [(true, "Metric"), (false, "Imperial")];
                                    if let Some(metric) =
                                        theme::segmented(ui, units.length.is_metric(), &options)
                                        && metric != units.length.is_metric()
                                    {
                                        // A blank sketch stays clean: nothing to save yet.
                                        let clean = !self.is_dirty();
                                        self.drawing.editor.set_units(if metric {
                                            drift_core::Units::default()
                                        } else {
                                            drift_core::Units::architectural()
                                        });
                                        if clean {
                                            self.drawing.file.clean_revision =
                                                Some(self.drawing.editor.doc.revision());
                                        }
                                    }
                                    ui.label(
                                        egui::RichText::new(if units.length.is_metric() {
                                            "Millimetres; more in File → Drawing settings."
                                        } else {
                                            "Feet and inches; more in File → Drawing settings."
                                        })
                                        .small()
                                        .weak(),
                                    );
                                    ui.add_space(6.0);
                                    if ui
                                        .add_sized([200.0, 26.0], egui::Button::new("Open…"))
                                        .clicked()
                                    {
                                        action = Some(Pending::Open);
                                    }
                                    if ui
                                        .add_sized(
                                            [200.0, 26.0],
                                            egui::Button::new("Open the example"),
                                        )
                                        .on_hover_text("A dimensioned mounting plate to explore")
                                        .clicked()
                                    {
                                        action = Some(Pending::Example);
                                    }
                                },
                            );
                            ui.add_space(20.0);
                            // Recent sketches.
                            ui.vertical(|ui| {
                                theme::section(ui, "Recent");
                                if self.recent.is_empty() {
                                    ui.label(
                                        egui::RichText::new(
                                            "Sketches you open or save will be listed here.",
                                        )
                                        .weak(),
                                    );
                                }
                                for (i, path) in self.recent.iter().take(7).enumerate() {
                                    let name = path
                                        .file_stem()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    let folder = path
                                        .parent()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    let (r, response) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 36.0),
                                        egui::Sense::click(),
                                    );
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            &name,
                                        )
                                    });
                                    if response.hovered() {
                                        ui.painter().rect_filled(
                                            r,
                                            theme::radius::CONTROL,
                                            ui.visuals().widgets.hovered.bg_fill,
                                        );
                                    }
                                    // The name over its folder, both cut short to fit.
                                    let painter = ui.painter_at(r);
                                    let width = r.width() - 16.0;
                                    let line = |text: String, size: f32, color| {
                                        let mut job = egui::text::LayoutJob::simple_singleline(
                                            text,
                                            egui::FontId::proportional(size),
                                            color,
                                        );
                                        job.wrap.max_width = width;
                                        job.wrap.max_rows = 1;
                                        painter.layout_job(job)
                                    };
                                    let g = line(
                                        name.clone(),
                                        theme::size::BODY,
                                        ui.visuals().text_color(),
                                    );
                                    painter.galley(
                                        r.left_top() + egui::vec2(8.0, 3.0),
                                        g,
                                        ui.visuals().text_color(),
                                    );
                                    let g = line(
                                        folder,
                                        theme::size::CAPTION,
                                        ui.visuals().weak_text_color(),
                                    );
                                    painter.galley(
                                        r.left_top() + egui::vec2(8.0, 20.0),
                                        g,
                                        ui.visuals().weak_text_color(),
                                    );
                                    if response.on_hover_text(path.to_string_lossy()).clicked() {
                                        action = Some(Pending::Recent(i));
                                    }
                                }
                            });
                        });
                        // A few things worth knowing, on one quiet line at the foot.
                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for (k, (keys, what)) in [
                                (app::panels::shortcut("K", false), "finds any command"),
                                ("?".to_owned(), "lists every shortcut"),
                                ("Right-click".to_owned(), "for the selection's menu"),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                if k > 0 {
                                    ui.add_space(10.0);
                                }
                                app::dialogs::keycaps(ui, &keys);
                                ui.label(egui::RichText::new(what).small().weak());
                            }
                        });
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

    /// Version and system details, for bug reports (About shows them).
    fn details(&self) -> String {
        format!(
            "Drift {VERSION}\nRenderer: {}\nSystem: {}\nMemory: {} MB in use",
            renderer_name(&self.backend),
            system_name(),
            self.memory.0 / (1024 * 1024)
        )
    }

    /// A problem report to paste into an email or message: the details and the end of the log
    /// (which holds file names, never drawing contents).
    fn report_text(&self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        let log = drift_platform::log_file::tail(REPORT_LOG_BYTES);
        #[cfg(target_arch = "wasm32")]
        let log = String::new();
        let mut text = format!(
            "What happened, and what did you expect?\n\n\n--- About ---\n{}\n",
            self.details()
        );
        if !log.is_empty() {
            text.push_str(&format!("\n--- The end of Drift's log ---\n{log}"));
        }
        text
    }

    /// Help → Report a problem: a report to send by email (or paste anywhere), with the details
    /// and the end of the log; or GitHub, for those with an account.
    pub(crate) fn report_window(&mut self, ctx: &egui::Context) {
        if !self.show_report {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut copied = false;
        theme::dialog(ctx, "Report a problem")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                close = theme::dialog_title(ui, "Report a problem", true);
                ui.add(
                    egui::Label::new(
                        "Copy the report and paste it into an email or message to whoever gave \
                         you Drift, with what happened. It has Drift's version, your system and \
                         the end of Drift's log: file names, never what is in your drawings.",
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let dark = ui.visuals().dark_mode;
                    if ui.add(theme::primary_button("Copy report", dark)).clicked() {
                        ui.ctx().copy_text(self.report_text());
                        copied = true;
                    }
                    if ui
                        .button("Email…")
                        .on_hover_text("A new email with the report in it")
                        .clicked()
                    {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(mailto(&self.report_text())));
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui
                        .button("Show logs folder")
                        .on_hover_text("To attach the whole log, or crash.log")
                        .clicked()
                    {
                        show_logs_folder();
                    }
                    ui.hyperlink_to("On GitHub…", crate::links::REPORT)
                        .on_hover_text("Bug reports and feature requests, with a GitHub account");
                });
            });
        if close {
            open = false;
        }
        if copied {
            let now = ctx.input(|i| i.time);
            self.set_status(
                "Copied the report: paste it into an email or message".into(),
                false,
                now,
            );
        }
        self.show_report = open;
    }

    /// Help → About Drift: version and system details, for bug reports.
    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let details = self.details();
        let mut copied = false;
        let mut close = false;
        theme::dialog(ctx, "About Drift")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(340.0);
                close = theme::dialog_title(ui, "About Drift", true);
                ui.horizontal(|ui| {
                    let (logo, _) =
                        ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                    icons::logo(ui.painter(), logo);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Drift")
                                .family(theme::bold())
                                .size(theme::size::DISPLAY),
                        );
                        ui.label(format!("Version {VERSION}"));
                        ui.label(egui::RichText::new("Parametric 2D sketching").weak());
                        ui.label(
                            egui::RichText::new("© 2026 Christopher Lange. All rights reserved.")
                                .small()
                                .weak(),
                        );
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "This is a beta: save often, and expect rough edges. Your sketch is \
                         also kept between sessions automatically.",
                    )
                    .small()
                    .weak(),
                );
                theme::section(ui, "Details");
                let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
                theme::row(ui, "Memory", |ui| {
                    ui.label(format!("{:.2} GB in use", gb(self.memory.0)))
                        .on_hover_text(format!(
                            "Drift warns at {:.0} GB and closes itself at {:.0} GB, so a fault \
                             can't exhaust the computer; the sketch is restored next time.",
                            gb(drift_platform::memory::WARN_BYTES),
                            gb(drift_platform::memory::limit())
                        ));
                });
                for line in details.lines().skip(1) {
                    if let Some((k, v)) = line.split_once(": ") {
                        theme::row(ui, k, |ui| {
                            ui.label(v);
                        });
                    }
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui
                        .button("Copy details")
                        .on_hover_text("For a bug report")
                        .clicked()
                    {
                        ui.ctx().copy_text(details.clone());
                        copied = true;
                    }
                    if ui
                        .button("Licences…")
                        .on_hover_text("The fonts and open-source software Drift is built with")
                        .clicked()
                    {
                        self.show_licences = true;
                    }
                    ui.hyperlink_to("Website", crate::links::WEBSITE);
                    ui.hyperlink_to("User guide", crate::links::GUIDE);
                    ui.hyperlink_to("Licence", crate::links::LICENCE)
                        .on_hover_text("Drift's end-user licence agreement");
                });
            });
        if close {
            open = false;
        }
        if copied {
            let now = ctx.input(|i| i.time);
            self.set_status("Copied the details".into(), false, now);
        }
        self.show_about = open;
    }

    /// About → Licences: the third-party notices, as shipped in THIRD-PARTY-NOTICES.txt.
    pub(crate) fn licences_window(&mut self, ctx: &egui::Context) {
        if !self.show_licences {
            return;
        }
        let mut open = true;
        let mut close = false;
        theme::dialog(ctx, "Licences")
            .open(&mut open)
            .resizable(true)
            .default_size([620.0, 480.0])
            .show(ctx, |ui| {
                close = theme::dialog_title(ui, "Licences", true);
                ui.label(
                    egui::RichText::new(
                        "The licences and copyright notices of the fonts and crates Drift is \
                         built with.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(4.0);
                // Ten thousand lines: lay out only the ones in view.
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let row_height = ui.fonts_mut(|f| f.row_height(&font));
                let lines: Vec<&str> = NOTICES.lines().collect();
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::both().auto_shrink(false).show_rows(
                    ui,
                    row_height,
                    lines.len(),
                    |ui, rows| {
                        for line in &lines[rows] {
                            ui.add(
                                egui::Label::new(egui::RichText::new(*line).monospace()).extend(),
                            );
                        }
                    },
                );
            });
        self.show_licences = open && !close;
    }

    /// Edit → Preferences: how Drift works on this computer (what the canvas shows is in
    /// the View menu, and a drawing's own rules in Drawing settings).
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut close = false;
        // The page shown, kept while the app runs.
        let key = egui::Id::new("preferences_page");
        let mut page = ctx
            .data(|d| d.get_temp::<PrefsPage>(key))
            .unwrap_or_default();
        theme::dialog(ctx, "Preferences")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                close = theme::dialog_title(ui, "Preferences", true);
                let room = (ctx.content_rect().height() - 140.0).clamp(200.0, 520.0);
                ui.horizontal_top(|ui| {
                    // Categories down the left, the page's settings at the right.
                    ui.allocate_ui_with_layout(
                        egui::vec2(140.0, room),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_size(egui::vec2(140.0, room));
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for p in PrefsPage::ALL {
                                if theme::list_row(ui, page == p, p.label()).clicked() {
                                    page = p;
                                }
                            }
                        },
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    // A column of its own: the row's layout would lay the settings out
                    // side by side.
                    let width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, room),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("preferences", page))
                                .max_height(room)
                                .min_scrolled_height(room)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.push_id("preferences", |ui| self.settings(ui, page));
                                });
                        },
                    );
                });
            });
        ctx.data_mut(|d| d.insert_temp(key, page));
        self.show_settings = open && !close;
    }

    /// File → Drawing settings: what is saved with this drawing (its units, how its areas
    /// and fills behave, its drafting style), apart from Preferences, which are the app's.
    pub(crate) fn drawing_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_drawing_settings {
            return;
        }
        let mut open = true;
        let mut close = false;
        theme::dialog(ctx, "Drawing settings")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                close = theme::dialog_title(ui, "Drawing settings", true);
                ui.label(
                    egui::RichText::new(format!(
                        "Saved with {}. A picked dimension or note can set its own style in \
                         the Inspector.",
                        self.drawing.file.name
                    ))
                    .small()
                    .weak(),
                );
                let room = (ctx.content_rect().height() - 160.0).max(200.0);
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .min_scrolled_height(room.min(560.0))
                    .show(ui, |ui| {
                        ui.push_id("drawing_settings", |ui| {
                            ui.set_max_width(ui.available_width() - 10.0);
                            self.project_section(ui);
                            self.units_section(ui);
                            self.rules_section(ui);
                            self.drafting_section(ui);
                        });
                    });
            });
        self.show_drawing_settings = open && !close;
    }

    fn settings(&mut self, ui: &mut egui::Ui, page: PrefsPage) {
        match page {
            PrefsPage::General => self.general_prefs(ui),
            PrefsPage::Drawing => self.drawing_prefs(ui),
            #[cfg(not(target_arch = "wasm32"))]
            PrefsPage::Updates => {
                self.updates_section(ui);
                #[cfg(not(feature = "demo"))]
                self.link_prefs(ui);
            }
        }
        ui.add_space(4.0);
    }

    fn general_prefs(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Appearance");
        theme::appearance(ui);
        theme::section(ui, "Crosshair");
        self.crosshair_prefs(ui);
        self.mouse_prefs(ui);
    }

    fn drawing_prefs(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "While drawing");
        ui.checkbox(
            &mut self.auto_relations,
            "Add relations automatically while drawing",
        )
        .on_hover_text(
            "Horizontal, vertical, coincident, tangent and so on, as you snap · Shift+Alt+R",
        );
        ui.checkbox(
            &mut self.perp_tangent_snap,
            "Snap perpendicular and tangent",
        )
        .on_hover_text(
            "Lines snap square onto lines, circles and arcs, or tangent to circles and arcs",
        );
        ui.horizontal(|ui| {
            ui.label("Angle snap");
            let mut options = vec![(None, "Off".to_owned())];
            options.extend(crate::ANGLE_STEPS.map(|s| (Some(s), format!("{s}°"))));
            let options: Vec<(Option<f64>, &str)> =
                options.iter().map(|(v, l)| (*v, l.as_str())).collect();
            if let Some(step) = theme::segmented(ui, self.angle_snap, &options) {
                self.angle_snap = step;
            }
        });
        ui.checkbox(
            &mut self.round_scale,
            "Round sizes when scaling with dimensions (Alt)",
        )
        .on_hover_text(
            "A scale handle with Alt steps the picked geometry's width and height through \
             round sizes in the document's units, finer as you zoom in. Off: any size",
        );
        theme::section(ui, "Picked geometry");
        ui.checkbox(
            &mut self.show_temp_dims,
            "Show temporary dimensions on the picked geometry",
        )
        .on_hover_text(
            "Pick one line, circle or arc to see its size and how far it is from its \
             neighbours. Click a value to type a new one (the picked geometry moves), the lock \
             to keep it as a dimension, and drag the square at its far end onto other geometry \
             to measure to that",
        );
        ui.label(
            egui::RichText::new(
                "What the canvas shows (grid, end points, dimensions, relations, line \
                 weights, status colours) and snapping to the grid are in the View menu. How \
                 a drawing's areas and fills behave is saved with it, in File → Drawing \
                 settings.",
            )
            .small()
            .weak(),
        );
    }

    fn mouse_prefs(&mut self, ui: &mut egui::Ui) {
        use crate::app::view::WheelMode;
        theme::section(ui, "Mouse and trackpad");
        let options = WheelMode::ALL.map(|m| (m, m.label()));
        if let Some(mode) = theme::segmented(ui, self.wheel.mode, &options) {
            self.wheel.mode = mode;
        }
        ui.label(
            egui::RichText::new(match self.wheel.mode {
                WheelMode::Auto => {
                    "A mouse wheel zooms; two fingers on a trackpad pan, and a pinch zooms."
                }
                WheelMode::Zoom => {
                    "Scrolling always zooms (if a trackpad isn't recognised as one, or you \
                     prefer it)."
                }
                WheelMode::Pan => "Scrolling always pans; Ctrl + scroll zooms.",
            })
            .weak(),
        );
        ui.label(
            egui::RichText::new("The middle or right mouse button drags the view either way.")
                .small()
                .weak(),
        );
    }

    /// Drawing settings → Closed areas and fills: the drawing's rules (saved with it).
    fn rules_section(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Closed areas and fills");
        let mut rules = self.drawing.editor.doc.rules();
        ui.checkbox(
            &mut rules.regions_by_layer,
            "Find closed areas on each layer separately",
        )
        .on_hover_text(
            "On: an area is closed only by curves on one layer, so furniture, notes or \
             hatching on other layers don't cut it up or close it. Its outline must then be \
             all on one layer (a door line on another layer leaves a wall open)",
        );
        ui.checkbox(
            &mut rules.regions_need_fill,
            "Pick an area only where it is filled or hatched",
        )
        .on_hover_text(
            "On: a click inside a solid fill or hatch picks it, and a click inside an empty \
             outline picks nothing (box selection starts there). Off: it picks the area, to \
             measure it",
        );
        ui.checkbox(&mut rules.stacked_fills, "Fills cover what is beneath them")
            .on_hover_text(
                "On: each solid fill or hatch is drawn over everything below it, with its \
                 outline on top, as a drawing program stacks shapes. Off: every fill is drawn \
                 under every curve",
            );
        ui.label("When an edit divides a filled area");
        let options = [
            (false, "Keep whole and flag"),
            (true, "Follow where it was first clicked"),
        ];
        if let Some(follow) = theme::segmented(ui, rules.follow_divided_fills, &options) {
            rules.follow_divided_fills = follow;
        }
        ui.label(
            egui::RichText::new(if rules.follow_divided_fills {
                "When its outline is cut, the fill or hatch takes the area its seed is in \
                 (where it was first clicked); the rest is left empty, and a notice says so. A \
                 curve drawn across it leaves it whole."
            } else {
                "The fill or hatch keeps covering the whole area and is marked \"Covers 2 \
                 areas\". Pick it to fit it to one area, split it into one per area, or keep it \
                 whole."
            })
            .small()
            .weak(),
        );
        self.drawing.editor.change_rules(|r| *r = rules);
        theme::section(ui, "Check sketch");
        self.gap_tolerance_row(ui);
        ui.add_space(4.0);
        let rules = self.drawing.editor.doc.rules();
        let same = rules == self.new_rules;
        if ui
            .add_enabled(
                !same,
                egui::Button::new("Use these area and fill rules for new drawings"),
            )
            .on_hover_text(if same {
                "New drawings start with these rules and this gap tolerance"
            } else {
                "Start new drawings (and older files) with these rules and this gap tolerance, \
                 on this computer"
            })
            .clicked()
        {
            self.new_rules = rules;
        }
    }
}

/// The pages of Preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum PrefsPage {
    #[default]
    General,
    Drawing,
    #[cfg(not(target_arch = "wasm32"))]
    Updates,
}

impl PrefsPage {
    #[cfg(not(target_arch = "wasm32"))]
    const ALL: [PrefsPage; 3] = [PrefsPage::General, PrefsPage::Drawing, PrefsPage::Updates];
    #[cfg(target_arch = "wasm32")]
    const ALL: [PrefsPage; 2] = [PrefsPage::General, PrefsPage::Drawing];

    fn label(self) -> &'static str {
        match self {
            PrefsPage::General => "General",
            PrefsPage::Drawing => "Drawing aids",
            #[cfg(not(target_arch = "wasm32"))]
            PrefsPage::Updates => "Updates & Labs",
        }
    }
}

/// How much of the log's end goes in a problem report.
#[cfg(not(target_arch = "wasm32"))]
const REPORT_LOG_BYTES: usize = 6 * 1024;

/// A `mailto:` link for a new email holding `body` (cut short to what mail programs take, at a
/// line), to whoever the tester sends it to.
fn mailto(body: &str) -> String {
    const MAX: usize = 1800;
    let body = if body.len() > MAX {
        let mut end = MAX;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        let cut = &body[..end];
        &cut[..cut.rfind('\n').unwrap_or(cut.len())]
    } else {
        body
    };
    let encode = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    format!(
        "mailto:{}?subject={}&body={}",
        crate::links::REPORT_EMAIL,
        encode(&format!("Drift {VERSION}: a problem")),
        encode(body)
    )
}

/// Show Drift's data folder, where `drift.log` and `crash.log` are, in Explorer or the Finder.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn show_logs_folder() {
    let Some(log) = drift_platform::log_file::path() else {
        return;
    };
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if log.exists() {
        crate::app::recovery::reveal(&log);
    } else if let Some(dir) = log.parent() {
        crate::app::recovery::reveal(dir);
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn the_report_email_is_a_short_encoded_link() {
        let long = format!("line one & two\n{}", "x ".repeat(2000));
        let link = mailto(&long);
        assert!(link.starts_with("mailto:"), "{link}");
        // Cut short at a line: the long one goes.
        assert!(link.ends_with("body=line%20one%20%26%20two"), "{link}");
        assert!(!link.contains(' ') && !link.contains('\n'));
        assert!(link.len() < 6000);
    }
}
