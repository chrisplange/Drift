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
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(
                        egui::Button::selectable(
                            self.layers_dock != app::layers_hud::LayersDock::Hidden,
                            "Layers panel",
                        )
                        .shortcut_text(shortcut("L", false)),
                    )
                    .clicked()
                {
                    self.toggle_layers_hud();
                    ui.close();
                }
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
                    .add(egui::Button::new("Zoom to fit").shortcut_text("F"))
                    .clicked()
                {
                    self.fit_target = Some(self.fit_bounds());
                    ui.close();
                }
                ui.separator();
                toggle_item(ui, &mut self.show_grid, "Grid", &shortcut("'", false));
                toggle_item(ui, &mut self.show_axes, "Origin axes", "");
                toggle_item(ui, &mut self.grid_snap, "Snap to grid", "⇧G");
                ui.separator();
                toggle_item(ui, &mut self.thin_lines, "Thin lines", "W");
                toggle_item(
                    ui,
                    &mut self.status_colors,
                    "Constraint status colours",
                    "⇧C",
                );
                ui.separator();
                toggle_item(ui, &mut self.show_dimensions, "Dimensions and notes", "");
                toggle_item(ui, &mut self.show_relations, "Relations", "");
                toggle_item(ui, &mut self.params.show_free, "Free directions", "");
                toggle_item(ui, &mut self.params.show_ghosts, "Variant ghosts", "");
                ui.separator();
                toggle_item(ui, &mut self.auto_relations, "Automatic relations", "");
                let wide = ui.ctx().content_rect().width() >= WIDE_LAYOUT;
                let mut show = self.show_properties.unwrap_or(wide);
                let before = show;
                toggle_item(ui, &mut show, "Inspector", "");
                if show != before {
                    self.show_properties = Some(show);
                }
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
                ui.label(
                    egui::RichText::new(format!(
                        "Drift {} · {}",
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
                            ui.add_space(if i > 0 { 10.0 } else { 2.0 });
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
                            let key = kind.shortcut().map(|c| c.to_string());
                            icons::tool_row(ui, selected, kind.label(), key.as_deref(), draw)
                        } else {
                            icons::button(ui, 38.0, selected, kind.label(), draw)
                        };
                        let response = response.on_hover_ui(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(kind.label()).family(theme::bold()));
                                if let Some(c) = kind.shortcut() {
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

    pub(crate) fn properties(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        use parameters::RightTab;
        if let Some(tab) = theme::segmented(
            ui,
            self.params.tab,
            &[
                (RightTab::Properties, "Inspector"),
                (RightTab::Parameters, "Parameters"),
                (RightTab::Layers, "Layers"),
                (RightTab::Sheets, "Sheets"),
                (RightTab::Blocks, "Blocks"),
            ],
        ) {
            self.params.tab = tab;
        }
        ui.add_space(4.0);
        if self.params.tab == RightTab::Parameters {
            let now = ui.input(|i| i.time);
            self.parameters_panel(ui, now);
            return;
        }
        if self.params.tab == RightTab::Layers {
            let now = ui.input(|i| i.time);
            self.layers_panel(ui, now);
            return;
        }
        if self.params.tab == RightTab::Sheets {
            self.sheets_panel(ui);
            return;
        }
        if self.params.tab == RightTab::Blocks {
            self.blocks_panel(ui);
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.inspector(ui));
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        self.block_edit_banner(ui);
        if self.annotation_inspector(ui)
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
        match selection.as_slice() {
            [] if !ed.selected_midpoints.is_empty() => {}
            [] if ed.selected_region.is_some() => {}
            [] => {
                ui.add_space(18.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No selection").size(15.0).weak());
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Click geometry to inspect it, or inside a closed area to measure it.",
                        )
                        .small()
                        .weak(),
                    );
                });
                theme::section(ui, "Document");
                let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
                theme::row(ui, "Curves", |ui| {
                    ui.label(curves.to_string());
                });
                theme::row(ui, "Relations", |ui| {
                    ui.label(ed.doc.constraint_count().to_string());
                });
            }
            [id] => {
                let (Some(entity), Some(shape)) = (ed.doc.get(*id), ed.index.shape(*id)) else {
                    return;
                };
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(entity.kind.type_name())
                            .family(theme::bold())
                            .size(15.0),
                    );
                    ui.label(egui::RichText::new(id.to_string()).weak());
                });
                theme::section(ui, "Geometry");
                property_rows(ui, shape, ed.doc.units());
            }
            many => {
                let mut counts = std::collections::BTreeMap::new();
                for id in many {
                    if let Some(e) = ed.doc.get(*id) {
                        *counts.entry(e.kind.type_name()).or_insert(0) += 1;
                    }
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{} selected", many.len()))
                        .family(theme::bold())
                        .size(15.0),
                );
                let summary: Vec<String> = counts
                    .iter()
                    .map(|(name, n)| format!("{n} {name}"))
                    .collect();
                ui.label(egui::RichText::new(summary.join(" · ")).weak());
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

        self.constraint_buttons(ui);
        let now = ui.input(|i| i.time);
        self.pattern_panel(ui, now);
        self.constraint_list(ui);

        if !self.editor.selection.is_empty() {
            ui.add_space(14.0);
            let delete = egui::Button::new(
                egui::RichText::new("Delete selection").color(ui.visuals().error_fg_color),
            )
            .min_size(egui::vec2(ui.available_width(), 26.0));
            if ui.add(delete).clicked() {
                self.editor.delete_selection();
            }
        }
        ui.add_space(10.0);
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
            if let Some(option) = chosen {
                let added = match option.constraints.as_slice() {
                    [c] if is_dimension => self.editor.add_dimension(*c, None).map(|id| vec![id]),
                    constraints => self.editor.add_constraints(option.label, constraints),
                };
                match added {
                    Ok(ids) => {
                        if is_dimension {
                            // Let the user type the value straight away.
                            self.focus_dimension = ids.first().copied();
                        }
                    }
                    Err(e) => self.set_status(format!("{}: {e}", option.label), true, now),
                }
            }
        }
        if !pins.is_empty() {
            ui.add_space(4.0);
            let mut chosen = None;
            egui::CollapsingHeader::new(egui::RichText::new("Pin measurement").weak())
                .default_open(false)
                .show(ui, |ui| {
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
        for (id, c) in &constraints {
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
                ui.label(
                    egui::RichText::new(format!("{zoom:.0}%"))
                        .monospace()
                        .color(ui.visuals().weak_text_color()),
                )
                .on_hover_text(format!("{:.4} px/mm", self.camera.scale));
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
                let mut units = self.editor.doc.units();
                let short = |f: drift_core::LengthFormat| match f {
                    drift_core::LengthFormat::Architectural => "ft-in",
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
                }
            });
            let _ = right;
        });
        // The hint, in whatever space is left between the readout and the right cluster.
        let hint = self.tool.hint();
        let rect = ui.min_rect();
        let left = rect.left() + 262.0;
        let right = rect.right() - 420.0;
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
