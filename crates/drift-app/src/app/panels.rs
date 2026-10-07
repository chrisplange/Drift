//! Menu bar, toolbar, properties panel and status bar.

use crate::*;

impl DriftApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let command = |key: &str| {
            if cfg!(target_os = "macos") {
                format!("Cmd+{key}")
            } else {
                format!("Ctrl+{key}")
            }
        };
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.request(Pending::New);
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text(command("O")))
                    .clicked()
                {
                    self.request(Pending::Open);
                }
                if ui.button("Open example").clicked() {
                    self.request(Pending::Example);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text(command("S")))
                    .clicked()
                {
                    self.save();
                }
                if ui
                    .add(egui::Button::new("Save as…").shortcut_text(command("Shift+S")))
                    .clicked()
                {
                    self.save_as();
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
                        egui::Button::new(undo).shortcut_text(command("Z")),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(
                        self.editor.history.can_redo(),
                        egui::Button::new(redo).shortcut_text(command("Y")),
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
                        egui::Button::new("Delete").shortcut_text("Del"),
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
                    .add(egui::Button::new("Select all").shortcut_text(command("A")))
                    .clicked()
                {
                    self.set_tool(ToolKind::Select);
                    self.editor.select_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Zoom to fit").shortcut_text("F"))
                    .clicked()
                {
                    self.fit_target = Some(self.fit_bounds());
                }
                ui.checkbox(&mut self.grid_snap, "Snap to grid (Shift+G)");
                ui.checkbox(&mut self.auto_relations, "Automatic relations");
                ui.checkbox(&mut self.thin_lines, "Thin lines (W)")
                    .on_hover_text("Draw every line one pixel wide, whatever its weight");
                ui.checkbox(&mut self.status_colors, "Constraint status colours")
                    .on_hover_text("Off: show layer colours");
                ui.checkbox(&mut self.show_dimensions, "Show dimensions");
                ui.checkbox(&mut self.show_relations, "Show relations");
                ui.checkbox(&mut self.params.show_free, "Show free directions")
                    .on_hover_text("Arrows on points that can still move, showing how");
                ui.checkbox(&mut self.params.show_ghosts, "Show variants as ghosts");
                let wide = ui.ctx().content_rect().width() >= WIDE_LAYOUT;
                let mut show = self.show_properties.unwrap_or(wide);
                if ui.checkbox(&mut show, "Properties panel").changed() {
                    self.show_properties = Some(show);
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.show_shortcuts = true;
                }
                if ui.button("Run solver benchmark").clicked() {
                    self.benchmark = Some(run_benchmark());
                }
            });
        });
    }

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, wide: bool) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                let mut group = 0;
                for kind in ToolKind::ALL {
                    if kind.group() != group {
                        group = kind.group();
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(if group == 1 { "Annotate" } else { "Modify" })
                                .small()
                                .weak(),
                        );
                    }
                    let selected = self.tool.kind() == kind;
                    let (label, width) = if wide {
                        let label = match kind.shortcut() {
                            Some(c) => format!("{}  {c}", kind.label()),
                            None => kind.label().to_owned(),
                        };
                        (label, 96.0)
                    } else {
                        (kind.label().to_owned(), 68.0)
                    };
                    let hover = match kind.shortcut() {
                        Some(c) => format!("{} ({c})", kind.label()),
                        None => kind.label().to_owned(),
                    };
                    if ui
                        .add_sized([width, 24.0], egui::Button::selectable(selected, label))
                        .on_hover_text(hover)
                        .clicked()
                    {
                        self.switch_tool(kind);
                    }
                }
            });
    }

    pub(crate) fn properties(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            use parameters::RightTab;
            for (tab, label) in [
                (RightTab::Properties, "Properties"),
                (RightTab::Parameters, "Parameters"),
                (RightTab::Layers, "Layers"),
            ] {
                let text = egui::RichText::new(label).strong().size(15.0);
                if ui
                    .add(egui::Button::selectable(self.params.tab == tab, text).frame(false))
                    .clicked()
                {
                    self.params.tab = tab;
                }
            }
        });
        ui.separator();
        if self.params.tab == parameters::RightTab::Parameters {
            let now = ui.input(|i| i.time);
            self.parameters_panel(ui, now);
            return;
        }
        if self.params.tab == parameters::RightTab::Layers {
            let now = ui.input(|i| i.time);
            self.layers_panel(ui, now);
            return;
        }
        if let Some(region) = self.editor.region() {
            let units = self.editor.doc.units();
            ui.strong("Region");
            egui::Grid::new("region").num_columns(2).show(ui, |ui| {
                let mut row = |name: &str, value: String| {
                    ui.label(name);
                    ui.monospace(value);
                    ui.end_row();
                };
                row("Area", units.format_area(region.area));
                row("Perimeter", units.format_readout(region.perimeter));
                row(
                    "Centroid",
                    format!(
                        "{}, {}",
                        units.format_readout(region.centroid.x),
                        units.format_readout(region.centroid.y)
                    ),
                );
                row("Holes", region.holes.len().to_string());
            });
            ui.add_space(8.0);
        }
        let ed = &self.editor;
        let selection: Vec<_> = ed.selection.iter().copied().collect();
        for line in &ed.selected_midpoints {
            if let Some(drift_core::Shape::Line { a, b }) = ed.index.shape(*line) {
                let m = (*a + *b) * 0.5;
                ui.strong(format!("Midpoint of {line}"));
                let units = ed.doc.units();
                ui.monospace(format!(
                    "{}, {}",
                    units.format_readout(m.x),
                    units.format_readout(m.y)
                ));
                ui.add_space(4.0);
            }
        }
        match selection.as_slice() {
            [] if !ed.selected_midpoints.is_empty() => {}
            [] if ed.selected_region.is_some() => {}
            [] => {
                ui.label(egui::RichText::new("Nothing selected").weak());
                ui.add_space(8.0);
                let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
                ui.label(format!("{curves} curves, {} entities", ed.doc.len()));
            }
            [id] => {
                let (Some(entity), Some(shape)) = (ed.doc.get(*id), ed.index.shape(*id)) else {
                    return;
                };
                ui.strong(format!("{} {id}", entity.kind.type_name()));
                ui.add_space(4.0);
                egui::Grid::new("props")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        property_rows(ui, shape, ed.doc.units());
                    });
            }
            many => {
                let mut counts = std::collections::BTreeMap::new();
                for id in many {
                    if let Some(e) = ed.doc.get(*id) {
                        *counts.entry(e.kind.type_name()).or_insert(0) += 1;
                    }
                }
                ui.strong(format!("{} selected", many.len()));
                for (name, n) in counts {
                    ui.label(format!("{n} × {name}"));
                }
            }
        }

        let curves: Vec<_> = selection
            .iter()
            .filter_map(|id| ed.doc.get(*id))
            .filter(|e| !e.kind.is_point())
            .collect();
        if !curves.is_empty() {
            ui.add_space(10.0);
            ui.label("Line style");
            let current = curves[0].style;
            let uniform = curves.iter().all(|e| e.style == current);
            let mut chosen = None;
            ui.horizontal(|ui| {
                for (style, label) in [
                    (LineKind::Solid, "Solid"),
                    (LineKind::Construction, "Construction"),
                    (LineKind::Center, "Center"),
                ] {
                    if ui
                        .selectable_label(uniform && current == style, label)
                        .clicked()
                    {
                        chosen = Some(style);
                    }
                }
            });
            if let Some(style) = chosen {
                self.set_selection_style(style);
            }
            self.appearance_panel(ui);
        }
        if !selection.is_empty() {
            ui.add_space(10.0);
            if ui.button("Delete").clicked() {
                self.editor.delete_selection();
            }
        }

        self.constraint_buttons(ui);
        let now = ui.input(|i| i.time);
        self.pattern_panel(ui, now);
        self.constraint_list(ui);
    }

    /// Buttons for the relations and dimensions that fit the selection.
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
            ui.add_space(10.0);
            ui.label(heading);
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                for option in &options {
                    if ui.button(option.label).clicked() {
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
            ui.add_space(6.0);
            let mut chosen = None;
            egui::CollapsingHeader::new("Pin measurement")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Keep a live measurement in the Parameters tab").weak(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for option in &pins {
                            if ui.button(option.label).clicked() {
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
    /// dimension values.
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
        ui.add_space(10.0);
        ui.label(format!("Constraints ({})", constraints.len()));
        let now = ui.input(|i| i.time);
        let mut remove = None;
        let mut select = None;
        let mut apply = None;
        let mut toggle_driven = None;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("constraints")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for (id, c) in &constraints {
                            let mut name = egui::RichText::new(constraint_label(&ed.doc, c));
                            if conflicts.contains(id) {
                                name = name.color(ui.visuals().error_fg_color);
                            }
                            if ui
                                .add(egui::Label::new(name).sense(egui::Sense::click()))
                                .on_hover_text(format!("{id}: click to select its geometry"))
                                .clicked()
                            {
                                select = Some(*c);
                            }
                            match c.value() {
                                Some(_) if ed.doc.is_driven(*id) => {
                                    ui.horizontal(|ui| {
                                        let value = ed.dimension_value(*id).unwrap_or_default();
                                        ui.add_sized(
                                            [90.0, 18.0],
                                            egui::Label::new(
                                                egui::RichText::new(format!(
                                                    "({})",
                                                    ed.doc.units().format(
                                                        drift_core::dimension_kind(c),
                                                        value
                                                    )
                                                ))
                                                .weak(),
                                            ),
                                        )
                                        .on_hover_text("Driven: follows the geometry");
                                        let mut driven = true;
                                        if ui
                                            .checkbox(&mut driven, "Driven")
                                            .on_hover_text("Untick to make it drive the geometry")
                                            .changed()
                                        {
                                            toggle_driven = Some((*id, false));
                                        }
                                    });
                                }
                                Some(_) => {
                                    ui.horizontal(|ui| {
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
                                                .desired_width(90.0)
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
                                            .checkbox(&mut driven, "Driven")
                                            .on_hover_text(
                                                "Make it a reference that follows the geometry",
                                            )
                                            .changed()
                                        {
                                            toggle_driven = Some((*id, true));
                                        }
                                    })
                                    .inner
                                }
                                None => {
                                    ui.label("");
                                }
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(*id);
                            }
                            ui.end_row();
                        }
                    });
            });
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

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            match self.pointer {
                Some(p) => {
                    let at = p.pos();
                    let units = self.editor.doc.units();
                    ui.monospace(format!(
                        "X {:>14}  Y {:>14}",
                        units.format_readout(at.x),
                        units.format_readout(at.y)
                    ));
                }
                None => {
                    ui.monospace(format!("{:35}", ""));
                }
            }
            ui.separator();
            let units = self.editor.doc.units();
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
            ui.monospace(format!("Grid {grid}"));
            ui.toggle_value(&mut self.grid_snap, "Snap to grid");
            ui.toggle_value(&mut self.thin_lines, "Thin")
                .on_hover_text("Thin lines (W): every line one pixel wide");
            ui.separator();
            let mut units = self.editor.doc.units();
            let short = |f: drift_core::LengthFormat| match f {
                drift_core::LengthFormat::Architectural => "ft-in",
                f => f.symbol(),
            };
            egui::ComboBox::from_id_salt("status_units")
                .selected_text(short(units.length))
                .width(60.0)
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
            ui.separator();
            if !self.editor.doc.is_empty() {
                let analysis = self.editor.analysis();
                let (text, color) = if !analysis.converged {
                    (
                        format!("Over defined ({} conflicting)", analysis.conflicts.len()),
                        ui.visuals().error_fg_color,
                    )
                } else if analysis.dof == Some(0) {
                    ("Fully defined".to_owned(), ui.visuals().strong_text_color())
                } else {
                    (
                        format!("Under defined: {} DOF", analysis.dof.unwrap_or(0)),
                        if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(0x5f, 0xa8, 0xf5)
                        } else {
                            egui::Color32::from_rgb(0x15, 0x65, 0xc0)
                        },
                    )
                };
                ui.label(egui::RichText::new(text).color(color));
                ui.separator();
            }
            match &self.status {
                Some(status) if now < status.until => {
                    let text = egui::RichText::new(&status.text);
                    ui.label(if status.error {
                        text.color(ui.visuals().error_fg_color)
                    } else {
                        text
                    });
                }
                _ => {
                    // Leave room for the readouts on the right; long hints are cut short
                    // (the full text shows on hover).
                    let width = (ui.available_width() - 190.0).max(40.0);
                    let hint = self.tool.hint();
                    ui.allocate_ui(egui::vec2(width, ui.available_height()), |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&hint).weak()).truncate())
                            .on_hover_text(&hint);
                    });
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.backend).weak().small());
                ui.separator();
                ui.monospace(format!("{:.4} px/mm", self.camera.scale));
            });
        });
        if self.status.as_ref().is_some_and(|s| now < s.until) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
}
