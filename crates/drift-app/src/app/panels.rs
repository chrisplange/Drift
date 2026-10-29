//! Menu bar, toolbar, inspector (properties) and status bar.

use crate::app::palette::{Command, ViewFlag};
use crate::commands::{self, Category};
use crate::icons::{self, Glyph};
use crate::*;

/// A shortcut as the platform writes it: ⌘ / ⇧ on macOS, Ctrl / Shift elsewhere.
pub(crate) fn shortcut(key: &str, shift: bool) -> String {
    commands::cmd_text(key, shift, cfg!(target_os = "macos"))
}

/// An inspector's title: the thing's icon on a soft tile, its name, and a line of detail.
pub(crate) fn title(ui: &mut egui::Ui, icon: Option<ToolKind>, name: &str, detail: &str) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        let (tile, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
        ui.painter().rect_filled(
            tile,
            9.0,
            accent.gamma_multiply(if dark { 0.22 } else { 0.12 }),
        );
        let glyph = tile.shrink(8.0);
        match icon {
            Some(kind) => icons::tool(ui.painter(), glyph, kind, accent),
            None => icons::glyph(ui.painter(), glyph, Glyph::Inspector, accent),
        }
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(
                egui::Label::new(egui::RichText::new(name).family(theme::bold()).size(15.0))
                    .truncate(),
            );
            if !detail.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate());
            }
        });
    });
    ui.add_space(2.0);
}

/// "1 line", "4 lines": a count with the kind's name, lower case and plural past one.
fn plural(n: usize, name: &str) -> String {
    let name = name.to_lowercase();
    if n == 1 {
        format!("1 {name}")
    } else if name.ends_with('s') {
        format!("{n} {name}es")
    } else {
        format!("{n} {name}s")
    }
}

/// The tool that draws an entity kind (for its icon).
fn kind_icon(kind: &drift_core::EntityKind) -> ToolKind {
    use drift_core::EntityKind as K;
    match kind {
        K::Point(_) => ToolKind::Point,
        K::Line { .. } => ToolKind::Line,
        K::Circle { .. } => ToolKind::Circle,
        K::Arc { .. } => ToolKind::Arc,
        K::Ellipse { .. } => ToolKind::Ellipse,
        K::EllipseArc { .. } => ToolKind::EllipseArc,
        K::Bezier { .. } => ToolKind::Pen,
    }
}

/// A value typed into the inspector.
enum SizeEdit {
    /// Set (or add) this dimension to the typed value.
    Dimension(Constraint, String),
    /// Move a point: its id, the axis (0 = X, 1 = Y) and the typed coordinate.
    Point(drift_core::EntityId, usize, String),
    /// Turn a line about its start to the typed angle.
    Angle(drift_core::EntityId, String),
}

/// A value field in the inspector: shows `shown`, and returns what was typed once the field
/// is left (Return, Tab or a click elsewhere) with something different in it. Esc puts the
/// old value back. The text being typed lives in egui's memory.
pub(crate) fn value_field(
    ui: &mut egui::Ui,
    key: egui::Id,
    shown: &str,
    width: f32,
    tip: &str,
) -> Option<String> {
    let mut text = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| shown.to_owned());
    let response = ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .id(key.with("field"))
                .desired_width(width),
        )
        .on_hover_text(tip);
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let typed = text.trim();
        if !cancelled && !typed.is_empty() && typed != shown {
            return Some(typed.to_owned());
        }
    }
    None
}

/// The values of a picked entity that can be typed. Sizes (a line's length, a circle's or
/// arc's radius and diameter) set the dimension that drives them, or add one. Positions (a
/// point, a line's ends, a centre) and a line's angle move the geometry, as far as its
/// relations allow.
fn size_rows(ui: &mut egui::Ui, ed: &Editor, id: drift_core::EntityId) -> Option<SizeEdit> {
    use drift_core::EntityKind as K;
    const UNITS: &str = "units and variables work, e.g. 2in or =width/2";
    let entity = ed.doc.get(id)?;
    let units = ed.doc.units();
    let out = std::cell::RefCell::new(None);
    let dimension = |ui: &mut egui::Ui, label: &str, wanted: Constraint, measured: f64| {
        let existing = ed.size_dimension(&wanted);
        let driven = existing.is_some_and(|c| ed.doc.is_driven(c));
        // A linked dimension shows its expression; otherwise the value as it would be typed.
        let same_kind = existing
            .and_then(|c| ed.doc.constraint(c))
            .is_some_and(|c| std::mem::discriminant(c) == std::mem::discriminant(&wanted));
        let shown = existing
            .filter(|_| same_kind)
            .and_then(|c| ed.dimension_edit_text(c))
            .unwrap_or_else(|| units.format_length(measured));
        theme::row(ui, label, |ui| {
            if driven {
                ui.label(format!("({shown})"))
                    .on_hover_text("A reference dimension: it follows the geometry");
                return;
            }
            let tip = if existing.is_some() {
                format!("Set by a dimension: type a new value ({UNITS})")
            } else {
                format!("Type a value to dimension it ({UNITS})")
            };
            let key = egui::Id::new(("size", id.0, label));
            if let Some(text) = value_field(ui, key, &shown, 118.0, &tip) {
                *out.borrow_mut() = Some(SizeEdit::Dimension(wanted, text));
            }
            if existing.is_some() {
                // A small dimension glyph: the value comes from a dimension.
                let (r, response) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                let color = theme::neutrals(ui.visuals().dark_mode).tertiary;
                icons::tool(ui.painter(), r, ToolKind::Dimension, color);
                response.on_hover_text("Set by a dimension");
            }
        });
    };
    // A point's coordinates, as two fields on one row.
    let position = |ui: &mut egui::Ui, label: &str, point: drift_core::EntityId| {
        let Ok(p) = ed.doc.point(point) else { return };
        theme::row(ui, label, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (axis, name, v) in [(0, "X", p.x), (1, "Y", p.y)] {
                let key = egui::Id::new(("coordinate", point.0, axis));
                let tip = format!("{name}: type a coordinate to move it ({UNITS})");
                if let Some(text) = value_field(ui, key, &units.format_length(v), 84.0, &tip) {
                    *out.borrow_mut() = Some(SizeEdit::Point(point, axis, text));
                }
            }
        });
    };
    match entity.kind {
        K::Line { a, b } => {
            let (pa, pb) = (ed.doc.point(a).ok()?, ed.doc.point(b).ok()?);
            let length = pa.distance(pb);
            let wanted = Constraint::Distance {
                a,
                b,
                value: length,
            };
            dimension(ui, "Length", wanted, length);
            let d = pb - pa;
            let shown = units.format_angle(d.y.atan2(d.x).to_degrees());
            theme::row(ui, "Angle", |ui| {
                let key = egui::Id::new(("angle", id.0));
                let tip = "Type an angle to turn the line about its start (anticlockwise from +X)";
                if let Some(text) = value_field(ui, key, &shown, 118.0, tip) {
                    *out.borrow_mut() = Some(SizeEdit::Angle(id, text));
                }
            });
            position(ui, "Start", a);
            position(ui, "End", b);
        }
        K::Circle { center, .. } | K::Arc { center, .. } => {
            let radius = drift_solver::radius_of(&ed.doc, id)?;
            let wanted = Constraint::Radius {
                curve: id,
                value: radius,
            };
            dimension(ui, "Radius", wanted, radius);
            if matches!(entity.kind, K::Circle { .. }) {
                let wanted = Constraint::Diameter {
                    curve: id,
                    value: 2.0 * radius,
                };
                dimension(ui, "Diameter", wanted, 2.0 * radius);
            }
            position(ui, "Center", center);
        }
        K::Point(_) => position(ui, "Position", id),
        K::Ellipse { center, .. } | K::EllipseArc { center, .. } => position(ui, "Center", center),
        K::Bezier { a, b, .. } => {
            position(ui, "Start", a);
            position(ui, "End", b);
        }
    }
    out.into_inner()
}

/// A figure for the empty inspector: a number over a caption.
fn stat(ui: &mut egui::Ui, value: usize, caption: &str, width: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(r, 8.0, ui.visuals().faint_bg_color);
    ui.painter().text(
        egui::pos2(r.center().x, r.top() + 20.0),
        egui::Align2::CENTER_CENTER,
        value.to_string(),
        egui::FontId::new(17.0, theme::bold()),
        ui.visuals().strong_text_color(),
    );
    ui.painter().text(
        egui::pos2(r.center().x, r.bottom() - 13.0),
        egui::Align2::CENTER_CENTER,
        caption.to_uppercase(),
        egui::FontId::new(9.5, theme::bold()),
        ui.visuals().weak_text_color(),
    );
}

/// A hairline between groups of status-bar controls.
fn status_divider(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 16.0), egui::Sense::hover());
    ui.painter().vline(
        r.center().x,
        r.y_range(),
        egui::Stroke::new(1.0, theme::hairline(ui)),
    );
}

impl DriftApp {
    /// A menu item for a command of the registry: its label, keys and description; greyed
    /// out (saying why) when it can't run; a check mark for a setting. Returns true if it
    /// ran.
    pub(crate) fn command_item(&mut self, ui: &mut egui::Ui, command: Command) -> bool {
        let Some(spec) = commands::spec(command) else {
            return false;
        };
        if spec.when.hidden() {
            return false;
        }
        let label = self.menu_label(spec);
        let keys = spec.key_text(cfg!(target_os = "macos"));
        let enabled = spec.when.holds(self);
        let response = match self.command_state(command) {
            Some(on) => {
                ui.add_enabled_ui(enabled, |ui| theme::menu_check(ui, on, &label, &keys))
                    .inner
            }
            // (Shortcut text, even empty, becomes part of the button's accessible label.)
            None if keys.is_empty() => ui.add_enabled(enabled, egui::Button::new(label)),
            None => ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)),
        };
        let response = response
            .on_hover_text(spec.description)
            .on_disabled_hover_text(spec.when.reason());
        if !response.clicked() {
            return false;
        }
        ui.close();
        let ctx = ui.ctx().clone();
        self.run_command(&ctx, command);
        true
    }

    /// Menu items for commands, in order; `None` is a separator.
    pub(crate) fn command_items(&mut self, ui: &mut egui::Ui, items: &[Option<Command>]) {
        for item in items {
            match item {
                Some(command) => {
                    self.command_item(ui, *command);
                }
                None => {
                    ui.separator();
                }
            }
        }
    }

    /// A category's commands as a menu, in the registry's order (the rail's), with
    /// separators round the rail's families.
    pub(crate) fn category_menu(&mut self, ui: &mut egui::Ui, category: Category) {
        let breaks = commands::menu_breaks(category);
        for spec in commands::registry()
            .iter()
            .filter(|s| s.category == category)
        {
            if breaks.contains(&spec.command) {
                ui.separator();
            }
            self.command_item(ui, spec.command);
        }
    }

    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        use Command as C;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.command_items(ui, &[Some(C::New), Some(C::Open)]);
                ui.menu_button("Open recent", |ui| {
                    if self.recent.is_empty() {
                        ui.label(egui::RichText::new("No recent sketches").weak());
                    }
                    let mut chosen = None;
                    for (i, path) in self.recent.iter().enumerate() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if ui
                            .button(name)
                            .on_hover_text(path.to_string_lossy())
                            .clicked()
                        {
                            chosen = Some(i);
                        }
                    }
                    if !self.recent.is_empty() {
                        ui.separator();
                        if ui.button("Clear the list").clicked() {
                            self.recent.clear();
                        }
                    }
                    if let Some(i) = chosen {
                        self.request(Pending::Recent(i));
                        ui.close();
                    }
                });
                self.command_items(
                    ui,
                    &[
                        Some(C::OpenExample),
                        Some(C::ShowStart),
                        None,
                        Some(C::Save),
                        Some(C::SaveAs),
                        None,
                        Some(C::DrawingSettings),
                        None,
                        Some(C::Import),
                    ],
                );
                ui.menu_button("Export", |ui| {
                    self.command_items(
                        ui,
                        &[
                            Some(C::ExportPdf),
                            Some(C::ExportSvg),
                            Some(C::ExportDxf),
                            None,
                            Some(C::ExportModelSvg),
                            Some(C::ExportModelPdf),
                        ],
                    );
                    ui.checkbox(&mut self.model_annotations, "Dimensions and notes")
                        .on_hover_text("Model exports include dimensions and notes");
                    ui.separator();
                    self.command_item(ui, C::ExportProfiles);
                });
                self.command_item(ui, C::Print);
            });
            ui.menu_button("Edit", |ui| {
                self.command_items(
                    ui,
                    &[
                        Some(C::Undo),
                        Some(C::Redo),
                        None,
                        Some(C::Cut),
                        Some(C::Copy),
                        Some(C::Paste),
                        Some(C::PasteInPlace),
                        None,
                        Some(C::CheckSketch),
                        None,
                        Some(C::Delete),
                        Some(C::Construction),
                        Some(C::Group),
                        Some(C::Ungroup),
                        Some(C::Duplicate),
                        Some(C::MakeBlock),
                        Some(C::BlockFromGroup),
                        Some(C::Explode),
                        Some(C::ExplodeToGroup),
                        None,
                        Some(C::SelectAll),
                        None,
                        Some(C::Preferences),
                    ],
                );
            });
            ui.menu_button("View", |ui| {
                self.command_item(ui, C::Palette);
                ui.menu_button("Panels", |ui| self.panels_menu(ui));
                ui.separator();
                self.command_items(
                    ui,
                    &[
                        Some(C::SheetPreview),
                        Some(C::ZoomToFit),
                        Some(C::ZoomToSelection),
                        None,
                        Some(C::Toggle(ViewFlag::Grid)),
                        Some(C::Toggle(ViewFlag::Axes)),
                        Some(C::Toggle(ViewFlag::Snap)),
                        None,
                        Some(C::Toggle(ViewFlag::Thin)),
                        Some(C::Toggle(ViewFlag::Status)),
                        Some(C::Toggle(ViewFlag::Endpoints)),
                        None,
                        Some(C::Toggle(ViewFlag::Dimensions)),
                        Some(C::Toggle(ViewFlag::Hidden)),
                        Some(C::Toggle(ViewFlag::Relations)),
                        Some(C::Toggle(ViewFlag::Free)),
                        Some(C::Toggle(ViewFlag::Combs)),
                        Some(C::Toggle(ViewFlag::Crops)),
                    ],
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Appearance").weak());
                    ui.set_max_width(ui.min_rect().width() + 170.0);
                    theme::appearance(ui);
                });
            });
            for category in [Category::Draw, Category::Build] {
                ui.menu_button(category.label(), |ui| self.category_menu(ui, category));
            }
            ui.menu_button("Modify", |ui| {
                self.category_menu(ui, Category::Modify);
                ui.separator();
                ui.menu_button("Arrange", |ui| self.category_menu(ui, Category::Arrange));
            });
            ui.menu_button("Annotate", |ui| self.category_menu(ui, Category::Annotate));
            ui.menu_button("Help", |ui| {
                self.command_items(
                    ui,
                    &[
                        Some(C::UserGuide),
                        Some(C::ReportProblem),
                        Some(C::ShowLogs),
                        None,
                        Some(C::Shortcuts),
                        // For development: it tells users nothing (hidden in release builds).
                        Some(C::Benchmark),
                        None,
                    ],
                );
                #[cfg(not(target_arch = "wasm32"))]
                self.updates_menu(ui);
                self.command_item(ui, C::About);
                ui.label(
                    egui::RichText::new(format!(
                        "Drift {} · {}",
                        drift_platform::VERSION,
                        self.backend
                    ))
                    .small()
                    .weak(),
                );
            });
        });
    }

    /// Select the curves that lie on top of others (the later-made of each pair).
    pub(crate) fn select_overlapping(&mut self, now: f64) {
        let found = self.editor.overlapping();
        if found.is_empty() {
            self.set_status("No curves overlap".into(), false, now);
            return;
        }
        self.abort_operation();
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        self.editor.selection = found.iter().copied().collect();
        self.set_status(
            format!(
                "Selected {} overlapping curve(s). Delete removes them and leaves the ones \
                 underneath",
                found.len()
            ),
            false,
            now,
        );
    }

    /// Zoom so that 100% is life size on a 96 dpi screen, about the middle of the canvas.
    pub(crate) fn zoom_to_percent(&mut self, percent: f64) {
        let target = percent / 100.0 * 96.0 / drift_core::units::MM_PER_INCH;
        self.fit_target = None;
        // The view's origin is the canvas centre.
        self.camera.zoom_at(DVec2::ZERO, target / self.camera.scale);
    }

    /// Fit the view to what is selected. False with nothing selected.
    pub(crate) fn zoom_to_selection(&mut self) -> bool {
        let ed = &self.editor;
        let mut bounds: Bounds = ed
            .selection
            .iter()
            .filter_map(|id| ed.doc.shape(*id))
            .map(|s| s.bounds())
            .collect();
        for id in &ed.selected_instances {
            if let Some(i) = ed.doc.instance(*id) {
                bounds.include(i.at);
            }
        }
        for o in ed.selected_texts.iter().filter_map(|t| ed.text_outline(*t)) {
            bounds.include(o.bounds.min);
            bounds.include(o.bounds.max);
        }
        for (_, a, label) in ed.riders().notes {
            // A section mark's far end too.
            let far = match a {
                drift_core::Annotation::Section { b, .. } => Some(b),
                _ => None,
            };
            for p in label.into_iter().chain(far) {
                bounds.include(p);
            }
        }
        if bounds.is_empty() {
            return false;
        }
        // A single point has no size to fit: show some room around it.
        if bounds.min == bounds.max {
            let pad = DVec2::splat(grid_info(&self.camera, self.imperial()).major);
            bounds.min -= pad;
            bounds.max += pad;
        }
        self.fit_target = Some(bounds);
        true
    }

    /// The status bar's zoom menu.
    fn zoom_menu(&mut self, ui: &mut egui::Ui) {
        self.command_items(
            ui,
            &[
                Some(Command::ZoomToFit),
                Some(Command::ZoomToSelection),
                None,
            ],
        );
        for percent in [25.0, 50.0, 100.0, 200.0, 400.0] {
            let label = if percent == 100.0 {
                "100% (life size)".to_owned()
            } else {
                format!("{percent:.0}%")
            };
            if ui.button(label).clicked() {
                self.zoom_to_percent(percent);
                ui.close();
            }
        }
    }

    /// View → Panels: show or hide each panel; hide them all; reset the layout.
    pub(crate) fn panels_menu(&mut self, ui: &mut egui::Ui) {
        for p in crate::dock::Panel::ALL {
            self.command_item(ui, Command::Panel(p));
        }
        ui.separator();
        self.command_items(
            ui,
            &[Some(Command::HideAllPanels), Some(Command::ResetPanels)],
        );
    }

    /// Several things picked, text or annotations among them: what they are, and where the
    /// annotations show. Returns true if it showed.
    pub(crate) fn mixed_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let ed = &self.editor;
        let annotations = self.picked_annotations();
        let n = ed.selection.len()
            + ed.selected_instances.len()
            + ed.selected_fills.len()
            + ed.selected_texts.len()
            + annotations.len();
        if n < 2 || (ed.selected_texts.is_empty() && annotations.is_empty()) {
            return false;
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        let mut count = |name: &'static str| match counts.iter_mut().find(|c| c.0 == name) {
            Some(c) => c.1 += 1,
            None => counts.push((name, 1)),
        };
        for id in &ed.selection {
            if let Some(e) = ed.doc.get(*id) {
                count(e.kind.type_name());
            }
        }
        for _ in &ed.selected_instances {
            count("Block");
        }
        for _ in &ed.selected_fills {
            count("Fill");
        }
        for r in &annotations {
            match r {
                drift_core::AnnotationRef::Dimension(_) => count("Dimension"),
                drift_core::AnnotationRef::Annotation(id) => {
                    if let Some(a) = ed.doc.annotation(*id) {
                        count(a.type_name());
                    }
                }
            }
        }
        for _ in &ed.selected_texts {
            count("Text");
        }
        let summary: Vec<String> = counts.iter().map(|(name, n)| plural(*n, name)).collect();
        title(
            ui,
            Some(ToolKind::Select),
            &format!("{n} selected"),
            &summary.join(" · "),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Delete, Move (G), Rotate (R), Mirror and Copy act on all of it. Dimensions \
                 follow their geometry.",
            )
            .small()
            .weak(),
        );
        self.visibility_section(ui);
        true
    }

    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        // On a sheet, the Inspector shows what is picked there.
        if self.sheet_preview {
            self.sheet_inspector(ui);
            return;
        }
        self.block_edit_banner(ui);
        self.walls_from_lines_report(ui);
        if self.fill_tool_inspector(ui)
            || self.opening_inspector(ui)
            || self.wall_inspector(ui)
            || self.mixed_inspector(ui)
            || self.annotation_inspector(ui)
            || self.text_inspector(ui)
            || self.instance_inspector(ui)
            || self.shape_inspector(ui)
            || self.fill_inspector(ui)
            || self.dimension_inspector(ui)
        {
            return;
        }
        if let Some(region) = self.editor.region() {
            let units = self.editor.doc.units();
            theme::section(ui, "Region");
            let mono = |ui: &mut egui::Ui, text: String| {
                ui.label(text);
            };
            theme::row(ui, "Area", |ui| mono(ui, units.format_area(region.area)));
            theme::row(ui, "Perimeter", |ui| {
                mono(ui, units.format_readout(region.perimeter))
            });
            theme::row(ui, "Centroid", |ui| {
                mono(
                    ui,
                    format!(
                        "{}, {}",
                        units.format_readout(region.centroid.x),
                        units.format_readout(region.centroid.y)
                    ),
                )
            });
            theme::row(ui, "Holes", |ui| mono(ui, region.holes.len().to_string()));
            self.region_fill_buttons(ui);
        }
        self.group_inspector(ui);
        let ed = &self.editor;
        let selection: Vec<_> = ed.selection.iter().copied().collect();
        for line in &ed.selected_midpoints {
            if let Some(drift_core::Shape::Line { a, b }) = ed.index.shape(*line) {
                let m = (*a + *b) * 0.5;
                let units = ed.doc.units();
                let of = ed.doc.get(*line).map_or(String::new(), |e| {
                    let layer = ed.doc.layer(e.layer).map_or("", |l| l.name.as_str());
                    format!("A line on layer {layer}")
                });
                theme::section(ui, "Midpoint");
                theme::row(ui, "Of", |ui| {
                    ui.label(of);
                });
                theme::row(ui, "At", |ui| {
                    ui.label(format!(
                        "{}, {}",
                        units.format_readout(m.x),
                        units.format_readout(m.y)
                    ));
                });
            }
        }
        let mut size_edit = None;
        match selection.as_slice() {
            [] if !ed.selected_midpoints.is_empty() => {}
            [] if ed.selected_region.is_some() => {}
            [] => {
                // A line saying so, a hint, and the sketch's totals.
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    icons::tool(
                        ui.painter(),
                        r,
                        ToolKind::Select,
                        ui.visuals().weak_text_color(),
                    );
                    ui.label(egui::RichText::new("Nothing selected").weak());
                });
                let hint = if self.editor.regions_need_fill() {
                    "Click geometry, or a fill or hatch, to inspect it."
                } else {
                    "Click geometry to inspect it, or inside a closed area to measure it."
                };
                ui.label(egui::RichText::new(hint).small().weak());
                theme::section(ui, "Sketch");
                self.layer_counts.update(&self.editor.doc);
                let curves = self.layer_counts.total;
                let relations = ed.doc.constraint_count();
                let layers = ed.doc.layers().count();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let w = ((ui.available_width() - 12.0) / 3.0).max(40.0);
                    stat(ui, curves, "Curves", w);
                    stat(ui, relations, "Relations", w);
                    stat(ui, layers, "Layers", w);
                });
            }
            [id] => {
                let (Some(entity), Some(shape)) = (ed.doc.get(*id), ed.index.shape(*id)) else {
                    return;
                };
                let layer = ed
                    .doc
                    .layer(entity.layer)
                    .map_or(String::new(), |l| format!("On layer {}", l.name));
                title(
                    ui,
                    Some(kind_icon(&entity.kind)),
                    entity.kind.type_name(),
                    &layer,
                );
                theme::section(ui, "Geometry");
                size_edit = size_rows(ui, ed, *id);
                property_rows(ui, shape, ed.doc.units());
            }
            many => {
                let mut counts = std::collections::BTreeMap::new();
                for id in many {
                    if let Some(e) = ed.doc.get(*id) {
                        *counts.entry(e.kind.type_name()).or_insert(0) += 1;
                    }
                }
                let summary: Vec<String> =
                    counts.iter().map(|(name, n)| plural(*n, name)).collect();
                title(
                    ui,
                    Some(ToolKind::Select),
                    &format!("{} selected", many.len()),
                    &summary.join(" · "),
                );
            }
        }

        let curves: Vec<_> = selection
            .iter()
            .filter_map(|id| ed.doc.get(*id))
            .filter(|e| !e.kind.is_point())
            .collect();
        if !curves.is_empty() {
            theme::section(ui, "Appearance");
            let current = curves[0].style;
            let uniform = curves.iter().all(|e| e.style == current);
            let shown = if uniform { current } else { LineKind::Solid };
            if let Some(style) = theme::segmented(
                ui,
                shown,
                &[
                    (LineKind::Solid, "Geometry"),
                    (LineKind::Construction, "Construction"),
                    (LineKind::Center, "Centre line"),
                ],
            ) {
                self.set_selection_style(style);
            }
            self.appearance_panel(ui);
            // Filled shapes among the picked curves (a group's, say): their fills too.
            let shapes = self.shapes_in_selection();
            if !shapes.fills.is_empty() {
                let title = match shapes.fills.len() {
                    1 => "Fill".to_owned(),
                    n => format!("Fill · {n} shapes"),
                };
                theme::section(ui, &title);
                self.shapes_fill_section(ui, &shapes);
            }
            theme::section(ui, "Arrange");
            self.arrange_row(ui);
        }

        let now = ui.input(|i| i.time);
        match size_edit {
            Some(SizeEdit::Dimension(wanted, text)) => {
                // A dimension already there is edited. Otherwise the size is added as a
                // dimension, unless automatic relations are off: then it only resizes.
                let keeps =
                    self.editor.auto_relations || self.editor.size_dimension(&wanted).is_some();
                let result = if keeps {
                    self.editor.set_size(wanted, &text).map(|_| ())
                } else {
                    self.editor.resize_once(wanted, &text)
                };
                if let Err(e) = result {
                    self.set_status(format!("Can't use {text}: {e}"), true, now);
                }
            }
            Some(SizeEdit::Point(id, axis, text)) => {
                match self.editor.evaluate_text(&text, Kind::Length) {
                    Ok(v) => {
                        if let Ok(mut to) = self.editor.doc.point(id) {
                            to[axis] = v.value;
                            if !self.editor.move_point_to(id, to) {
                                self.set_status(
                                    "That point is held by its relations".into(),
                                    true,
                                    now,
                                );
                            }
                        }
                    }
                    Err(e) => self.set_status(format!("Can't use {text}: {e}"), true, now),
                }
            }
            Some(SizeEdit::Angle(line, text)) => {
                match self.editor.evaluate_text(&text, Kind::Angle) {
                    Ok(v) => {
                        if !self.editor.set_line_angle(line, v.value) {
                            self.set_status(
                                "That line's direction is held by its relations".into(),
                                true,
                                now,
                            );
                        }
                    }
                    Err(e) => self.set_status(format!("Can't use {text}: {e}"), true, now),
                }
            }
            None => {}
        }
        self.centre_mark_button(ui);
        self.constraint_buttons(ui);
        self.pattern_panel(ui, now);
        self.constraint_list(ui);

        if !self.editor.selection.is_empty() {
            ui.add_space(16.0);
            if theme::danger_button(ui, "Delete selection").clicked() {
                self.editor.delete_selection();
            }
        }
        ui.add_space(10.0);
    }

    /// Add a relation or dimension offered for the selection. Returns a new dimension (to
    /// type its value into).
    pub(crate) fn apply_constraint_option(
        &mut self,
        option: &crate::editor::ConstraintOption,
        is_dimension: bool,
        now: f64,
    ) -> Option<ConstraintId> {
        let added = match option.constraints.as_slice() {
            [c] if is_dimension => self.editor.add_dimension(*c, None).map(|id| vec![id]),
            constraints if is_dimension => self.editor.add_constraints(option.label, constraints),
            // Only the relations that aren't there yet (some may already hold).
            _ => match self.editor.missing_relations(option).as_slice() {
                [] if crate::editor::Editor::is_fix_option(option) => Err(
                    crate::editor::EditError::FullyDefined(self.editor.fixed_name()),
                ),
                [] => Err(crate::editor::EditError::Redundant),
                missing => self.editor.add_constraints(option.label, missing),
            },
        };
        match added {
            Ok(ids) if is_dimension => ids.first().copied(),
            Ok(_) => None,
            Err(e) => {
                self.set_status(format!("{}: {e}", option.label), true, now);
                None
            }
        }
    }

    /// Relations and dimensions that fit the selection, as chips.
    pub(crate) fn constraint_buttons(&mut self, ui: &mut egui::Ui) {
        let relations = self.editor.relation_options();
        let dimensions = self.editor.dimension_options();
        let now = ui.input(|i| i.time);
        let pins = dimensions.clone();
        for (heading, options, is_dimension) in [
            ("Add relation", relations, false),
            ("Add dimension", dimensions, true),
        ] {
            if options.is_empty() {
                continue;
            }
            theme::section(ui, heading);
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for option in &options {
                    let chip = egui::Button::new(option.label).corner_radius(12.0);
                    if ui.add(chip).clicked() {
                        chosen = Some(option.clone());
                    }
                }
            });
            if let Some(option) = chosen
                && let Some(id) = self.apply_constraint_option(&option, is_dimension, now)
            {
                // Let the user type the value straight away.
                self.focus_dimension = Some(id);
            }
        }
        if !pins.is_empty() {
            let mut chosen = None;
            theme::fold(ui, "pin_measurement", "Pin measurement", false, |ui| {
                ui.label(
                    egui::RichText::new("Keep a live measurement in the Parameters tab")
                        .small()
                        .weak(),
                );
                ui.horizontal_wrapped(|ui| {
                    for option in &pins {
                        let chip = egui::Button::new(option.label).corner_radius(12.0);
                        if ui.add(chip).clicked() {
                            chosen = option.constraints.first().copied();
                        }
                    }
                });
            });
            if let Some(c) = chosen {
                match self.editor.pin_measurement(c) {
                    Ok(_) => self.set_status("Pinned (see the Parameters tab)".into(), false, now),
                    Err(e) => self.set_status(format!("Can't pin it: {e}"), true, now),
                }
            }
        }
    }

    /// Constraints on the selection (or all of them when nothing is selected), with editable
    /// dimension values. Each row: the name (click to select its geometry), the value, a
    /// "reference" toggle for dimensions, and a delete button that shows on hover.
    pub(crate) fn constraint_list(&mut self, ui: &mut egui::Ui) {
        let ed = &self.editor;
        let constraints: Vec<(ConstraintId, Constraint)> =
            if ed.selection.is_empty() && ed.selected_midpoints.is_empty() {
                ed.doc.constraints().map(|(id, c)| (id, *c)).collect()
            } else {
                // Include constraints on the selection's points: a line's length is a distance
                // between its end points.
                let mut relevant = ed.doc.defining_points(ed.selection.iter().copied());
                relevant.extend(ed.selection.iter().copied());
                relevant.extend(ed.selected_midpoints.iter().copied());
                ed.doc
                    .constraints()
                    .filter(|(_, c)| c.entities().any(|e| relevant.contains(&e)))
                    .map(|(id, c)| (id, *c))
                    .collect()
            };
        if constraints.is_empty() {
            return;
        }
        let conflicts = ed.analysis().conflicts.clone();
        let everything = ed.selection.is_empty() && ed.selected_midpoints.is_empty();
        theme::section(
            ui,
            &format!("Relations & dimensions · {}", constraints.len()),
        );
        // With nothing selected this lists the whole sketch: folded away unless asked for (or
        // something conflicts).
        if everything && conflicts.is_empty() {
            let key = ui.make_persistent_id("all_constraints_open");
            let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
            if ui
                .add(egui::Button::new(if open { "Hide list" } else { "Show all" }).small())
                .clicked()
            {
                open = !open;
                ui.data_mut(|d| d.insert_temp(key, open));
            }
            if !open {
                return;
            }
        }
        let now = ui.input(|i| i.time);
        let mut remove = None;
        let mut select = None;
        let mut apply = None;
        let mut toggle_driven = None;
        let weak = ui.visuals().weak_text_color();
        // Laying out thousands of rows every frame stalls the UI; the rest are summarised.
        const MAX_ROWS: usize = 100;
        let hidden_rows = constraints.len().saturating_sub(MAX_ROWS);
        for (id, c) in constraints.iter().take(MAX_ROWS) {
            let row = ui.horizontal(|ui| {
                let mut name = egui::RichText::new(constraint_label(&ed.doc, c));
                if conflicts.contains(id) {
                    name = name.color(ui.visuals().error_fg_color);
                }
                let label = ui
                    .allocate_ui_with_layout(
                        egui::vec2(100.0, 22.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(100.0);
                            ui.add(
                                egui::Label::new(name)
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            )
                        },
                    )
                    .inner
                    .on_hover_text(format!("{id}: click to select its geometry"));
                if label.clicked() {
                    select = Some(*c);
                }
                match c.value() {
                    Some(_) if ed.doc.is_driven(*id) => {
                        let value = ed.dimension_value(*id).unwrap_or_default();
                        ui.add_sized(
                            [86.0, 22.0],
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "({})",
                                    ed.doc.units().format(drift_core::dimension_kind(c), value)
                                ))
                                .color(weak),
                            ),
                        )
                        .on_hover_text("Reference (driven): follows the geometry");
                        let mut driven = true;
                        if ui
                            .toggle_value(&mut driven, egui::RichText::new("Ref").small())
                            .on_hover_text("Make it drive the geometry")
                            .changed()
                        {
                            toggle_driven = Some((*id, false));
                        }
                    }
                    Some(_) => {
                        let field = egui::Id::new(("dimension", id.0));
                        let mut text = self
                            .dimension_text
                            .get(id)
                            .cloned()
                            .or_else(|| ed.dimension_edit_text(*id))
                            .unwrap_or_default();
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .id(field)
                                .desired_width(86.0)
                                .hint_text(c.unit()),
                        );
                        if self.focus_dimension == Some(*id) {
                            response.request_focus();
                            self.focus_dimension = None;
                        }
                        if response.has_focus() {
                            self.dimension_text.insert(*id, text.clone());
                        }
                        if response.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                apply = Some((*id, text.clone()));
                            }
                            self.dimension_text.remove(id);
                        }
                        let mut driven = false;
                        if ui
                            .toggle_value(&mut driven, egui::RichText::new("Ref").small())
                            .on_hover_text("Make it a reference that follows the geometry")
                            .changed()
                        {
                            toggle_driven = Some((*id, true));
                        }
                    }
                    None => {}
                }
                // The delete button sits at the row's end, clear of the scroll bar; it is
                // faint until the row is hovered.
                ui.add_space((ui.available_width() - 30.0).max(0.0));
                let hovered = ui.ui_contains_pointer();
                let x = egui::Button::new(egui::RichText::new("×").size(16.0).color(if hovered {
                    ui.visuals().text_color()
                } else {
                    weak.gamma_multiply(0.5)
                }))
                .frame(false)
                .small();
                if ui.add(x).on_hover_text("Delete").clicked() {
                    remove = Some(*id);
                }
            });
            // A hairline between rows.
            let r = row.response.rect;
            ui.painter().line_segment(
                [
                    r.left_bottom() + egui::vec2(0.0, 3.0),
                    r.right_bottom() + egui::vec2(0.0, 3.0),
                ],
                egui::Stroke::new(0.5, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
        }
        if hidden_rows > 0 {
            ui.label(
                egui::RichText::new(format!("… and {hidden_rows} more (pick less to see them)"))
                    .weak(),
            );
        }
        // Clearing all of them at once (on a selection).
        if !everything {
            let (relations, dims) = self.editor.selection_constraints();
            let mut clear = None;
            ui.horizontal_wrapped(|ui| {
                if !relations.is_empty()
                    && ui
                        .button(format!("Clear relations ({})", relations.len()))
                        .on_hover_text(format!(
                            "Remove the relations on the selection, including those holding it \
                             to what isn't picked: {}. Shared corners stay joined; dimensions \
                             stay",
                            crate::constraint_summary(&self.editor.doc, &relations)
                        ))
                        .clicked()
                {
                    clear = Some(false);
                }
                if !dims.is_empty()
                    && ui
                        .button(format!("Clear dimensions ({})", dims.len()))
                        .on_hover_text(format!(
                            "Remove the dimensions on the selection: {}",
                            crate::constraint_summary(&self.editor.doc, &dims)
                        ))
                        .clicked()
                {
                    clear = Some(true);
                }
            });
            if let Some(dimensions) = clear {
                self.clear_selection_constraints(dimensions, now);
                return;
            }
        }
        if let Some(c) = select {
            self.editor.selection = c.entities().collect();
        }
        if let Some(id) = remove {
            self.editor.remove_constraint(id);
        }
        if let Some((id, driven)) = toggle_driven
            && let Err(e) = self.editor.set_driven(id, driven)
        {
            self.set_status(format!("Can't make it driving: it {e}"), true, now);
        }
        if let Some((id, text)) = apply
            && Some(&text) != self.editor.dimension_edit_text(id).as_ref()
            && let Err(e) = self.editor.set_dimension_text(id, &text)
        {
            self.set_status(format!("Can't use {text}: {e}"), true, now);
        }
    }

    /// The status bar: cursor position, the tool's hint, then view toggles, units, the
    /// sketch's definition status and zoom. Messages appear as toasts over the canvas.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, _now: f64) {
        let cluster_left = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let units = self.editor.doc.units();
            let (x, y) = match self.pointer {
                Some(p) => {
                    let at = p.pos();
                    (units.format_readout(at.x), units.format_readout(at.y))
                }
                None => (String::new(), String::new()),
            };
            ui.allocate_ui_with_layout(
                egui::vec2(250.0, 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(250.0);
                    // Each value right-aligned in its own cell, so the digits don't jump.
                    for (axis, value) in [("X", x), ("Y", y)] {
                        ui.label(egui::RichText::new(axis).weak());
                        ui.allocate_ui_with_layout(
                            egui::vec2(96.0, 20.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(96.0);
                                let shown = if value.is_empty() { "–" } else { &value };
                                ui.label(egui::RichText::new(shown).weak());
                            },
                        );
                        ui.add_space(10.0);
                    }
                },
            );
            // Right-hand cluster first, so the hint takes what is left.
            let right =
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // One control height across the bar.
                    ui.spacing_mut().interact_size.y = 22.0;
                    // Zoom as a percentage of real size on a 96 dpi screen.
                    let zoom = self.camera.scale * drift_core::units::MM_PER_INCH / 96.0 * 100.0;
                    let button = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("{zoom:.0}%"))
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .frame_when_inactive(false),
                        )
                        .on_hover_text("Zoom: click for fit, selection and fixed sizes");
                    egui::Popup::menu(&button).show(|ui| self.zoom_menu(ui));
                    #[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
                    self.link_pill(ui);
                    if !self.editor.doc.is_empty() {
                        let analysis = self.editor.analysis();
                        let dark = ui.visuals().dark_mode;
                        let (text, color) = if !analysis.converged {
                            (
                                format!("{} conflicting", analysis.conflicts.len()),
                                ui.visuals().error_fg_color,
                            )
                        } else if analysis.dof == Some(0) {
                            ("Fully defined".to_owned(), theme::ok_color(dark))
                        } else {
                            (
                                format!("{} DOF free", analysis.dof.unwrap_or(0)),
                                theme::under_color(dark),
                            )
                        };
                        theme::pill(ui, &text, color).on_hover_text(
                            "Degrees of freedom: how much the sketch can still move",
                        );
                    }
                    // Curves drawn on top of others: a warning that selects them when clicked.
                    // (Not re-counted in the middle of a drag.)
                    let overlaps = if self.tool.is_busy() {
                        self.editor.overlapping_as_last_found()
                    } else {
                        self.editor.overlapping()
                    };
                    if !overlaps.is_empty() {
                        let amber = egui::Color32::from_rgb(0xff, 0x9f, 0x0a);
                        let pill =
                            theme::pill(ui, &format!("{} overlapping", overlaps.len()), amber);
                        let click = ui
                            .interact(pill.rect, ui.id().with("overlaps"), egui::Sense::click())
                            .on_hover_text(
                                "Curves drawn on top of other curves (the same line twice, or the \
                             touching sides of two shapes). Click to select the ones on top; \
                             Delete then removes them and leaves those underneath.",
                            );
                        if click.clicked() {
                            self.select_overlapping(_now);
                        }
                    }
                    // Dimensions or relations switched off for now: say so, and show them
                    // again on a click.
                    let grey = ui.visuals().weak_text_color();
                    for (shown, text, keys) in [
                        (self.show_relations, "Relations off · Show", "Alt+R"),
                        (self.show_dimensions, "Dims & notes off · Show", "Alt+D"),
                    ] {
                        if shown {
                            continue;
                        }
                        let pill = theme::pill(ui, text, grey);
                        let click =
                            ui.interact(pill.rect, ui.id().with(text), egui::Sense::click());
                        click.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text)
                        });
                        if click
                            .on_hover_text(format!("Click (or {keys}) to show them again"))
                            .clicked()
                        {
                            if text.starts_with("Rel") {
                                self.show_relations = true;
                            } else {
                                self.show_dimensions = true;
                            }
                        }
                    }
                    let hidden = self.hidden_annotation_count();
                    if hidden > 0 {
                        let grey = ui.visuals().weak_text_color();
                        let text = if self.show_hidden_annotations {
                            format!("Showing {hidden} hidden")
                        } else {
                            format!(
                                "{hidden} hidden annotation{}",
                                if hidden == 1 { "" } else { "s" }
                            )
                        };
                        let pill = theme::pill(ui, &text, grey);
                        let click =
                            ui.interact(pill.rect, ui.id().with("hidden"), egui::Sense::click());
                        click.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text)
                        });
                        let click = click.on_hover_text(if self.show_hidden_annotations {
                            "Dimensions and annotations hidden on the canvas, shown faded. Click \
                             (or Alt+H) to leave them out again"
                        } else {
                            "Dimensions and annotations hidden on the canvas. Click (or Alt+H) \
                             to show them faded, so they can be picked and shown again"
                        });
                        if click.clicked() {
                            self.show_hidden_annotations = !self.show_hidden_annotations;
                        }
                    }
                    let mut units = self.editor.doc.units();
                    let short = |f: drift_core::LengthFormat| match f {
                        drift_core::LengthFormat::Architectural => "ft-in",
                        drift_core::LengthFormat::FractionalInch => "in ½",
                        f => f.symbol(),
                    };
                    egui::ComboBox::from_id_salt("status_units")
                        .selected_text(short(units.length))
                        .width(56.0)
                        .show_ui(ui, |ui| {
                            for f in drift_core::LengthFormat::METRIC
                                .into_iter()
                                .chain(drift_core::LengthFormat::IMPERIAL)
                            {
                                ui.selectable_value(&mut units.length, f, f.label());
                            }
                        })
                        .response
                        .on_hover_text("Display units (more in File → Drawing settings)");
                    if units != self.editor.doc.units() {
                        self.editor.set_units(units);
                    }
                    status_divider(ui);
                    let minor = grid_info(&self.camera, self.imperial()).minor;
                    let grid = if units.length.is_metric() {
                        format!("{} mm", format_length(minor))
                    } else {
                        Units {
                            decimals: 4,
                            fraction: 64,
                            ..units
                        }
                        .format_length(minor)
                    };
                    // Two groups, laid out from the right: what the drawing shows, then (at
                    // the left) what snaps and what is added as you draw.
                    let toggle = |ui: &mut egui::Ui,
                                  glyph: Glyph,
                                  name: &str,
                                  value: &mut bool,
                                  tip: &str| {
                        if icons::button(ui, 22.0, *value, name, |p, r, c| {
                            icons::glyph(p, r, glyph, c)
                        })
                        .on_hover_text(tip)
                        .clicked()
                        {
                            *value = !*value;
                        }
                    };
                    toggle(
                        ui,
                        Glyph::Status,
                        "Status colours",
                        &mut self.status_colors,
                        "Constraint status colours (on) or layer and line colours (off) · Shift+C",
                    );
                    toggle(
                        ui,
                        Glyph::Thin,
                        "Thin lines",
                        &mut self.thin_lines,
                        "Thin lines (Shift+Alt+W)",
                    );
                    toggle(
                        ui,
                        Glyph::Endpoints,
                        "End points",
                        &mut self.always_endpoints,
                        "Always show end points (off: only on hovered or picked curves) · Shift+Alt+V",
                    );
                    toggle(
                        ui,
                        Glyph::Axes,
                        "Origin axes",
                        &mut self.show_axes,
                        "Origin axes",
                    );
                    toggle(
                        ui,
                        Glyph::Grid,
                        "Grid",
                        &mut self.show_grid,
                        &format!("Grid: {grid} spacing · {}", shortcut("'", false)),
                    );
                    status_divider(ui);
                    toggle(
                        ui,
                        Glyph::AutoRelations,
                        "Auto relations",
                        &mut self.auto_relations,
                        "Auto relations: add relations as you draw and modify (horizontal, coincident, tangent...). Off: plain geometry · Shift+Alt+R",
                    );
                    // Angle snapping sits with grid snapping: a click opens its steps.
                    let angle_snap = &mut self.angle_snap;
                    let tip = match *angle_snap {
                        Some(step) => {
                            format!("Angle snap: lines are drawn every {step}° (Alt places freely)")
                        }
                        None => "Angle snap: draw lines at every 1°, 5°, 15° or 45°".to_owned(),
                    };
                    let on = angle_snap.is_some();
                    let button = icons::button(ui, 22.0, on, "Angle snap", |p, r, c| {
                        icons::glyph(p, r, Glyph::Angle, c)
                    })
                    .on_hover_text(tip);
                    egui::Popup::menu(&button).show(|ui| {
                        ui.set_min_width(130.0);
                        if ui.radio(angle_snap.is_none(), "Off").clicked() {
                            *angle_snap = None;
                        }
                        for step in crate::ANGLE_STEPS {
                            let picked = *angle_snap == Some(step);
                            if ui.radio(picked, format!("Every {step}°")).clicked() {
                                *angle_snap = Some(step);
                            }
                        }
                    });
                    toggle(
                        ui,
                        Glyph::PerpTangent,
                        "Perpendicular & tangent",
                        &mut self.perp_tangent_snap,
                        "Perpendicular & tangent: lines snap square onto lines, circles and arcs, \
                         or tangent to circles and arcs, and follow guides square to a line or \
                         along an arc from the end you draw on from",
                    );
                    toggle(
                        ui,
                        Glyph::Snap,
                        "Snap to grid",
                        &mut self.grid_snap,
                        &format!("Snap to grid ({grid}) · Shift+G"),
                    );
                    // Where the cluster's leftmost control starts.
                    ui.min_rect().left()
                });
            right.inner
        });
        // The hint, in whatever space is left between the readout and the right cluster (as
        // wide as it actually is, so the hint never lies over its controls).
        let selecting = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        // How to get at a picked shape's fill on its own, until it has been done a few times
        // (#223).
        let tool_hint = if selecting
            && self.editor.fill_alone_picks < crate::FILL_ALONE_HINTS
            && self.editor.picked_shape_fill().is_some()
        {
            format!(
                "Double-click inside to edit its fill on its own · {}",
                self.tool.hint()
            )
        } else {
            self.tool.hint()
        };
        let hint = if self.align_edge.is_some() {
            "Align to edge: click one of the shape's edges (the pattern runs along it from its \
             start) · Esc cancels"
                .to_owned()
        } else if self.add_view_on.is_some() && !self.tool.is_busy() {
            "Drag a rectangle round what the new view shows · Esc cancels".to_owned()
        } else if self.sheet_preview {
            "Drag a view to move it · Alt-drag the drawing inside it · its handles crop it · Esc \
             returns to the model"
                .to_owned()
        } else if let Some(f) = self.editor.hovered_fill.filter(|_| selecting) {
            // What the fill under the pointer is: a shape, or an area fill and why (#202).
            let kind = match self.editor.fill_kind(f) {
                Ok(()) => "Shape".to_owned(),
                Err(why) => format!("Area fill: {}", why.reason()),
            };
            format!("{kind} · {tool_hint}")
        } else {
            tool_hint
        };
        let rect = ui.min_rect();
        let left = rect.left() + 256.0;
        let right = cluster_left.inner - 12.0;
        if right - left > 40.0 {
            let area = egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(right, rect.bottom()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                // The prompt, left-aligned after the readout (as AutoCAD's command line).
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    status_divider(ui);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&hint)
                                .size(theme::size::LABEL)
                                .color(ui.visuals().weak_text_color()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&hint);
                });
            });
        }
    }
}
