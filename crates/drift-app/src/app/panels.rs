//! Menu bar, toolbar, inspector (properties) and status bar.

use crate::icons::{self, Glyph};
use crate::*;

/// A shortcut as the platform writes it: ⌘ / ⇧ on macOS, Ctrl / Shift elsewhere.
pub(crate) fn shortcut(key: &str, shift: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("{}⌘{key}", if shift { "⇧" } else { "" })
    } else {
        format!("Ctrl+{}{key}", if shift { "Shift+" } else { "" })
    }
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
fn value_field(
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
                .font(egui::TextStyle::Monospace)
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
                ui.monospace(format!("({shown})"))
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
                ui.label(egui::RichText::new("dim").small().weak())
                    .on_hover_text("Set by a dimension");
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

/// A checkbox-style menu item with a shortcut hint.
fn toggle_item(ui: &mut egui::Ui, value: &mut bool, label: &str, keys: &str) {
    if ui
        .add(egui::Button::selectable(*value, label).shortcut_text(keys))
        .clicked()
    {
        *value = !*value;
    }
}

impl DriftApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("New").shortcut_text(shortcut("N", false)))
                    .clicked()
                {
                    self.request(Pending::New);
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text(shortcut("O", false)))
                    .clicked()
                {
                    self.request(Pending::Open);
                }
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
                if ui.button("Open example").clicked() {
                    self.request(Pending::Example);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text(shortcut("S", false)))
                    .clicked()
                {
                    self.save();
                }
                if ui
                    .add(egui::Button::new("Save as…").shortcut_text(shortcut("S", true)))
                    .clicked()
                {
                    self.save_as();
                }
                ui.separator();
                if ui
                    .button("Import DXF…")
                    .on_hover_text("Add a DXF file's curves to the sketch as geometry")
                    .clicked()
                {
                    self.files.open(app::sheets_ui::DXF_FILE);
                }
                if ui
                    .button("Import underlay…")
                    .on_hover_text("A PDF or DXF drawing to trace over (Layers tab)")
                    .clicked()
                {
                    self.pick_underlay_file();
                }
                let painter = ui.painter().clone();
                ui.menu_button("Export", |ui| {
                    if ui
                        .button("PDF…")
                        .on_hover_text("Every sheet at true scale (or the drawing fitted to A3)")
                        .clicked()
                    {
                        self.export_pdf(&painter);
                    }
                    if ui
                        .button("SVG…")
                        .on_hover_text("The current sheet")
                        .clicked()
                    {
                        self.export_svg(&painter);
                    }
                    if ui
                        .button("DXF…")
                        .on_hover_text("The model at full size, with layers and line types")
                        .clicked()
                    {
                        self.export_dxf(&painter);
                    }
                });
                if ui
                    .add(egui::Button::new("Print…").shortcut_text(shortcut("P", false)))
                    .clicked()
                {
                    self.print(&painter);
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = match self.editor.history.undo_label() {
                    Some(label) => format!("Undo {label}"),
                    None => "Undo".into(),
                };
                let redo = match self.editor.history.redo_label() {
                    Some(label) => format!("Redo {label}"),
                    None => "Redo".into(),
                };
                if ui
                    .add_enabled(
                        self.editor.history.can_undo(),
                        egui::Button::new(undo).shortcut_text(shortcut("Z", false)),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(
                        self.editor.history.can_redo(),
                        egui::Button::new(redo).shortcut_text(shortcut("Z", true)),
                    )
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                let ctx = ui.ctx().clone();
                let now = ui.input(|i| i.time);
                let can_copy = !self.editor.selection.is_empty()
                    || !self.editor.selected_instances.is_empty()
                    || self.editor.selected_text.is_some();
                if ui
                    .add_enabled(
                        can_copy,
                        egui::Button::new("Cut").shortcut_text(shortcut("X", false)),
                    )
                    .clicked()
                {
                    self.cut(&ctx);
                }
                if ui
                    .add_enabled(
                        can_copy,
                        egui::Button::new("Copy").shortcut_text(shortcut("C", false)),
                    )
                    .clicked()
                {
                    self.copy(&ctx);
                }
                if ui
                    .add_enabled(
                        self.clipboard.is_some(),
                        egui::Button::new("Paste").shortcut_text(shortcut("V", false)),
                    )
                    .on_hover_text("Under the pointer, or in the middle of the view")
                    .clicked()
                {
                    self.paste_copied(false, now);
                }
                if ui
                    .add_enabled(
                        self.clipboard.is_some(),
                        egui::Button::new("Paste in place").shortcut_text(shortcut("V", true)),
                    )
                    .on_hover_text("Where it was copied from")
                    .clicked()
                {
                    self.paste_copied(true, now);
                }
                ui.separator();
                if ui
                    .button("Check sketch…")
                    .on_hover_text(
                        "Find ends that nearly meet but don't, ends short of a curve, and \
                         curves of no length, and fix them",
                    )
                    .clicked()
                {
                    self.open_sketch_check();
                    ui.close();
                }
                ui.separator();
                let has_selection = !self.editor.selection.is_empty();
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new("Delete").shortcut_text(if cfg!(target_os = "macos") {
                            "⌫"
                        } else {
                            "Del"
                        }),
                    )
                    .clicked()
                {
                    self.editor.delete_selection();
                }
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new("Toggle construction").shortcut_text("X"),
                    )
                    .clicked()
                {
                    self.editor.toggle_construction();
                }
                if ui
                    .add_enabled(
                        self.editor.selection.len() >= 2,
                        egui::Button::new("Group").shortcut_text(shortcut("G", false)),
                    )
                    .clicked()
                {
                    self.editor.make_group();
                }
                let grouped = self
                    .editor
                    .selection
                    .iter()
                    .any(|id| self.editor.doc.group_of(*id).is_some());
                if ui
                    .add_enabled(
                        grouped,
                        egui::Button::new("Ungroup").shortcut_text(shortcut("G", true)),
                    )
                    .clicked()
                {
                    self.editor.ungroup();
                }
                if ui
                    .add_enabled(
                        !self.editor.selection.is_empty()
                            || !self.editor.selected_instances.is_empty(),
                        egui::Button::new("Duplicate").shortcut_text("Shift+D"),
                    )
                    .on_hover_text("Copy the selection in place, then move the copy")
                    .clicked()
                {
                    self.set_tool(ToolKind::Select);
                    if self.editor.duplicate_selection() > 0 {
                        self.tool
                            .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
                    }
                }
                if ui
                    .add_enabled(
                        !self.editor.selection.is_empty(),
                        egui::Button::new("Make block"),
                    )
                    .on_hover_text("A block from the selection (name it in the Blocks tab)")
                    .clicked()
                {
                    let now = ui.input(|i| i.time);
                    if let Err(e) = self.editor.make_block("") {
                        self.set_status(e, true, now);
                    }
                }
                if ui
                    .add_enabled(
                        !self.editor.selected_instances.is_empty(),
                        egui::Button::new("Explode"),
                    )
                    .clicked()
                {
                    self.editor.explode_selected();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Select all").shortcut_text(shortcut("A", false)))
                    .clicked()
                {
                    self.set_tool(ToolKind::Select);
                    self.editor.select_all();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Preferences…").shortcut_text(shortcut(",", false)))
                    .clicked()
                {
                    self.show_settings = true;
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Command palette…").shortcut_text(shortcut("K", false)))
                    .clicked()
                {
                    self.open_palette();
                    ui.close();
                }
                ui.menu_button("Panels", |ui| self.panels_menu(ui));
                ui.separator();
                if ui
                    .add(egui::Button::selectable(self.sheet_preview, "Sheet"))
                    .on_hover_text("Show the drawing on its sheet, as it prints")
                    .clicked()
                {
                    if self.current_sheet().is_none() {
                        self.add_sheet();
                    }
                    self.sheet_preview = !self.sheet_preview;
                    self.preview.zoom = 0.0;
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Zoom to fit").shortcut_text("Shift+F"))
                    .clicked()
                {
                    self.fit_target = Some(self.fit_bounds());
                    ui.close();
                }
                ui.separator();
                toggle_item(ui, &mut self.show_grid, "Grid", &shortcut("'", false));
                toggle_item(ui, &mut self.show_axes, "Origin axes", "");
                toggle_item(ui, &mut self.grid_snap, "Snap to grid", "Shift+G");
                ui.separator();
                toggle_item(ui, &mut self.thin_lines, "Thin lines", "W");
                toggle_item(
                    ui,
                    &mut self.status_colors,
                    "Constraint status colours",
                    "Shift+C",
                );
                toggle_item(ui, &mut self.always_endpoints, "Always show end points", "");
                ui.separator();
                toggle_item(
                    ui,
                    &mut self.show_dimensions,
                    "Dimensions and notes",
                    "Alt+D",
                );
                toggle_item(
                    ui,
                    &mut self.show_hidden_annotations,
                    "Hidden annotations (faded)",
                    "Alt+H",
                );
                toggle_item(ui, &mut self.show_relations, "Relations", "Alt+R");
                toggle_item(ui, &mut self.params.show_free, "Free directions", "");
                toggle_item(ui, &mut self.params.show_ghosts, "Variant ghosts", "");
                toggle_item(ui, &mut self.show_combs, "Curvature combs", "");
                ui.separator();
                toggle_item(ui, &mut self.auto_relations, "Automatic relations", "");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Appearance").weak());
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.show_shortcuts = true;
                }
                if ui.button("Run solver benchmark").clicked() {
                    self.benchmark = Some(run_benchmark());
                }
                ui.separator();
                if ui.button("About Drift").clicked() {
                    self.show_about = true;
                }
                let memory = match self.memory.0 {
                    0 => String::new(),
                    bytes => format!(" · {} MB", bytes / (1024 * 1024)),
                };
                ui.label(
                    egui::RichText::new(format!(
                        "Drift {} · {}{memory}",
                        env!("CARGO_PKG_VERSION"),
                        self.backend
                    ))
                    .small()
                    .weak(),
                );
            });
        });
    }

    /// The tool palette: icon buttons in groups (draw, annotate, modify), with tooltips that
    /// name the tool, its shortcut and what it does.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, wide: bool) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if wide {
                    ui.spacing_mut().item_spacing.y = 1.0;
                }
                ui.add_space(4.0);
                for (i, (title, group)) in ToolKind::SECTIONS.iter().enumerate() {
                    if wide {
                        if !title.is_empty() {
                            ui.add_space(if i > 0 { 7.0 } else { 2.0 });
                            theme::caps_header(ui, title);
                            ui.add_space(2.0);
                        }
                    } else if i > 0 {
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);
                    }
                    for &kind in *group {
                        let selected = self.tool.kind() == kind;
                        let draw = |p: &egui::Painter, r: egui::Rect, c: egui::Color32| {
                            icons::tool(p, r, kind, c)
                        };
                        let response = if wide {
                            let key = kind.shortcut_label();
                            icons::tool_row(ui, selected, kind.label(), key.as_deref(), draw)
                        } else {
                            icons::button(ui, 38.0, selected, kind.label(), draw)
                        };
                        let response = response.on_hover_ui(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(kind.label()).family(theme::bold()));
                                if let Some(c) = kind.shortcut_label() {
                                    ui.label(egui::RichText::new(c.to_string()).monospace().weak());
                                }
                            });
                            ui.label(egui::RichText::new(kind.description()).weak());
                        });
                        if response.clicked() {
                            self.switch_tool(kind);
                        }
                    }
                }
                ui.add_space(6.0);
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
        let target = percent / 100.0 * 96.0 / 25.4;
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
        if let Some(o) = ed.selected_text.and_then(|t| ed.text_outline(t)) {
            bounds.include(o.bounds.min);
            bounds.include(o.bounds.max);
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
        if ui
            .add(egui::Button::new("Zoom to fit").shortcut_text("Shift+F"))
            .clicked()
        {
            self.fit_target = Some(self.fit_bounds());
            ui.close();
        }
        let picked = !self.editor.selection.is_empty()
            || !self.editor.selected_instances.is_empty()
            || self.editor.selected_text.is_some();
        if ui
            .add_enabled(
                picked,
                egui::Button::new("Zoom to selection").shortcut_text("Shift+Z"),
            )
            .clicked()
        {
            self.zoom_to_selection();
            ui.close();
        }
        ui.separator();
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
        use crate::dock::Panel;
        for p in Panel::ALL {
            let shown =
                !self.dock_view.hide_all && self.layout.place(p) != crate::dock::Place::Hidden;
            let keys = if p == Panel::Layers {
                shortcut("L", false)
            } else {
                String::new()
            };
            if ui
                .add(egui::Button::selectable(shown, p.title()).shortcut_text(keys))
                .on_hover_text(p.description())
                .clicked()
            {
                if shown {
                    self.layout.move_to(p, crate::dock::Place::Hidden, None);
                } else {
                    self.show_panel(p);
                }
                ui.close();
            }
        }
        ui.separator();
        if ui
            .add(
                egui::Button::selectable(self.dock_view.hide_all, "Hide all panels")
                    .shortcut_text(shortcut("\\", false)),
            )
            .clicked()
        {
            self.dock_view.hide_all = !self.dock_view.hide_all;
            ui.close();
        }
        if ui
            .button("Reset panel layout")
            .on_hover_text("Every panel back in the right dock")
            .clicked()
        {
            self.layout = Default::default();
            self.dock_view.hide_all = false;
            ui.close();
        }
    }

    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        self.block_edit_banner(ui);
        if self.annotation_inspector(ui)
            || self.text_inspector(ui)
            || self.instance_inspector(ui)
            || self.fill_inspector(ui)
            || self.dimension_inspector(ui)
        {
            return;
        }
        if let Some(region) = self.editor.region() {
            let units = self.editor.doc.units();
            theme::section(ui, "Region");
            let mono = |ui: &mut egui::Ui, text: String| {
                ui.monospace(text);
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
        self.centre_mark_button(ui);
        let ed = &self.editor;
        let selection: Vec<_> = ed.selection.iter().copied().collect();
        for line in &ed.selected_midpoints {
            if let Some(drift_core::Shape::Line { a, b }) = ed.index.shape(*line) {
                let m = (*a + *b) * 0.5;
                let units = ed.doc.units();
                theme::section(ui, "Midpoint");
                theme::row(ui, "Of", |ui| {
                    ui.label(format!("{line}"));
                });
                theme::row(ui, "At", |ui| {
                    ui.monospace(format!(
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
                ui.add_space(22.0);
                ui.vertical_centered(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                    icons::tool(
                        ui.painter(),
                        r.shrink(6.0),
                        ToolKind::Select,
                        ui.visuals().weak_text_color().gamma_multiply(0.7),
                    );
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Nothing selected").size(14.0).weak());
                    ui.add_space(2.0);
                    let hint = if self.editor.regions_need_fill {
                        "Click geometry, or a fill or hatch, to inspect it."
                    } else {
                        "Click geometry to inspect it, or inside a closed area to measure it."
                    };
                    ui.label(egui::RichText::new(hint).small().weak());
                });
                theme::section(ui, "Sketch");
                let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
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
                    .map_or(String::new(), |l| format!("on layer {}", l.name));
                title(
                    ui,
                    Some(kind_icon(&entity.kind)),
                    entity.kind.type_name(),
                    &format!("{id} · {layer}"),
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
                let summary: Vec<String> = counts
                    .iter()
                    .map(|(name, n)| format!("{n} {name}"))
                    .collect();
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
            constraints => self.editor.add_constraints(option.label, constraints),
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
        ui.horizontal(|ui| {
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
                    ui.label(
                        egui::RichText::new(format!("X {x:>13}   Y {y:>13}"))
                            .monospace()
                            .color(ui.visuals().weak_text_color()),
                    );
                },
            );
            // Right-hand cluster first, so the hint takes what is left.
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // Zoom as a percentage of real size on a 96 dpi screen.
                let zoom = self.camera.scale * 25.4 / 96.0 * 100.0;
                let button = ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{zoom:.0}%"))
                                .monospace()
                                .color(ui.visuals().weak_text_color()),
                        )
                        .frame_when_inactive(false),
                    )
                    .on_hover_text("Zoom: click for fit, selection and fixed sizes");
                egui::Popup::menu(&button).show(|ui| self.zoom_menu(ui));
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
                    theme::pill(ui, &text, color)
                        .on_hover_text("Degrees of freedom: how much the sketch can still move");
                }
                // Curves drawn on top of others: a warning that selects them when clicked.
                // (Not re-counted in the middle of a drag.)
                let overlaps = if self.tool.is_busy() {
                    self.editor
                        .overlap_cache
                        .borrow()
                        .as_ref()
                        .map(|(_, found)| found.clone())
                        .unwrap_or_default()
                } else {
                    self.editor.overlapping()
                };
                if !overlaps.is_empty() {
                    let amber = egui::Color32::from_rgb(0xff, 0x9f, 0x0a);
                    let pill = theme::pill(ui, &format!("{} overlapping", overlaps.len()), amber);
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
                let hidden = self.hidden_annotation_count();
                if hidden > 0 {
                    let grey = ui.visuals().weak_text_color();
                    let text = if self.show_hidden_annotations {
                        format!("{hidden} hidden, shown")
                    } else {
                        format!("{hidden} hidden")
                    };
                    let pill = theme::pill(ui, &text, grey);
                    let click =
                        ui.interact(pill.rect, ui.id().with("hidden"), egui::Sense::click());
                    click.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text)
                    });
                    let click = click.on_hover_text(
                        "Dimensions and annotations hidden on the canvas. Click (or Alt+H) \
                             to show them faded, so they can be picked and shown again",
                    );
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
                    .on_hover_text("Display units (more in the Parameters tab)");
                if units != self.editor.doc.units() {
                    self.editor.set_units(units);
                }
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
                let angle_snap = &mut self.angle_snap;
                for (glyph, value, tip) in [
                    (
                        Glyph::Thin,
                        &mut self.thin_lines,
                        "Thin lines (W)".to_owned(),
                    ),
                    (
                        Glyph::Snap,
                        &mut self.grid_snap,
                        format!("Snap to grid ({grid}) · Shift+G"),
                    ),
                    (
                        Glyph::Status,
                        &mut self.status_colors,
                        "Constraint status colours (on) or layer and line colours (off) · Shift+C"
                            .to_owned(),
                    ),
                    (
                        Glyph::Endpoints,
                        &mut self.always_endpoints,
                        "Always show end points (off: only on hovered or picked curves)".to_owned(),
                    ),
                    (Glyph::Axes, &mut self.show_axes, "Origin axes".to_owned()),
                    (
                        Glyph::Grid,
                        &mut self.show_grid,
                        format!("Grid: {grid} spacing · {}", shortcut("'", false)),
                    ),
                ] {
                    if icons::button(ui, 22.0, *value, &tip, |p, r, c| {
                        icons::glyph(p, r, glyph, c)
                    })
                    .on_hover_text(tip)
                    .clicked()
                    {
                        *value = !*value;
                    }
                    // Angle snapping sits with grid snapping: a click opens its steps.
                    if glyph == Glyph::Snap {
                        let tip = match *angle_snap {
                            Some(step) => format!(
                                "Angle snap: lines are drawn every {step}° (Alt places freely)"
                            ),
                            None => "Angle snap: draw lines at every 5°, 15° or 45°".to_owned(),
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
                    }
                }
            });
            let _ = right;
        });
        // The hint, in whatever space is left between the readout and the right cluster.
        let hint = self.tool.hint();
        let rect = ui.min_rect();
        let left = rect.left() + 262.0;
        let right = rect.right() - 446.0;
        if right - left > 40.0 {
            let area = egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(right, rect.bottom()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&hint).weak().small()).truncate())
                        .on_hover_text(&hint);
                });
            });
        }
    }
}
