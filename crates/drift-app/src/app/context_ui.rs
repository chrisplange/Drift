//! The context bar under the menu (the active tool and its options, or quick actions for the
//! selection, as in Affinity's context toolbar) and the canvas's right-click menu.

use crate::icons::{self, Glyph};
use crate::tools::SlotMode;
use crate::*;

/// A compact text button for the context bar.
fn bar_button(ui: &mut egui::Ui, label: &str, keys: &str, tip: &str) -> bool {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(12.5),
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    if !keys.is_empty() {
        text.append(
            keys,
            6.0,
            egui::TextFormat {
                font_id: egui::FontId::monospace(10.5),
                color: ui.visuals().weak_text_color(),
                ..Default::default()
            },
        );
    }
    ui.add(
        egui::Button::new(text)
            .frame_when_inactive(false)
            .min_size(egui::vec2(0.0, 24.0)),
    )
    .on_hover_text(tip)
    .clicked()
}

/// A thin vertical divider.
fn divider(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::hairline(ui));
    ui.add_space(2.0);
}

/// A small caption before a control.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).weak());
}

/// A segmented choice sized to its labels (for the bar, where the full width isn't ours).
fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let font = egui::FontId::proportional(12.0);
    let width: f32 = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
                + 22.0
        })
        .sum();
    let mut out = None;
    ui.allocate_ui(egui::vec2(width + 4.0, 24.0), |ui| {
        out = theme::segmented(ui, current, options);
    });
    out
}

impl DriftApp {
    /// The context bar: the tool's icon and name, then its options (or, with the select tool,
    /// actions for the selection), and a search field for the command palette at the right.
    pub(crate) fn context_bar(&mut self, ui: &mut egui::Ui) {
        let kind = self.tool.kind();
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            ui.set_height(28.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            // The palette's search field, from the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.search_field(ui) {
                    self.open_palette();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let accent = theme::accent(ui.visuals().dark_mode);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    icons::tool(ui.painter(), r.shrink(1.0), kind, accent);
                    // Painted rather than a label, so the toolbar keeps the only "Select"
                    // (etc.) button for assistive tech.
                    let name = ui.painter().layout_no_wrap(
                        kind.label().to_owned(),
                        egui::FontId::new(13.0, theme::bold()),
                        ui.visuals().strong_text_color(),
                    );
                    let (r, _) = ui.allocate_exact_size(name.size(), egui::Sense::hover());
                    ui.painter().galley(r.min, name, egui::Color32::WHITE);
                    divider(ui);
                    self.tool_options(ui, kind, now);
                });
            });
        });
    }

    /// A field-like button that opens the command palette.
    fn search_field(&mut self, ui: &mut egui::Ui) -> bool {
        let (r, response) = ui.allocate_exact_size(egui::vec2(220.0, 24.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Search commands")
        });
        let visuals = ui.visuals();
        let fill = if response.hovered() {
            visuals.widgets.hovered.bg_fill
        } else {
            visuals.extreme_bg_color
        };
        ui.painter().rect(
            r,
            7.0,
            fill,
            egui::Stroke::new(1.0, theme::hairline(ui)),
            egui::StrokeKind::Inside,
        );
        let weak = ui.visuals().weak_text_color();
        icons::glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(r.left() + 14.0, r.center().y),
                egui::Vec2::splat(13.0),
            ),
            Glyph::Search,
            weak,
        );
        ui.painter().text(
            egui::pos2(r.left() + 26.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            "Search commands",
            egui::FontId::proportional(12.5),
            weak,
        );
        ui.painter().text(
            egui::pos2(r.right() - 8.0, r.center().y),
            egui::Align2::RIGHT_CENTER,
            app::panels::shortcut("K", false),
            egui::FontId::monospace(10.5),
            weak.gamma_multiply(0.8),
        );
        response
            .on_hover_text("Find any tool, setting, panel or command")
            .clicked()
    }

    /// Options for the active tool.
    fn tool_options(&mut self, ui: &mut egui::Ui, kind: ToolKind, now: f64) {
        let units = self.editor.doc.units();
        match kind {
            ToolKind::Select => self.selection_actions(ui, now),
            ToolKind::Circle | ToolKind::Ellipse | ToolKind::EllipseArc => {
                if let Some(k) = choice(
                    ui,
                    kind,
                    &[
                        (ToolKind::Circle, "Circle"),
                        (ToolKind::Ellipse, "Ellipse"),
                        (ToolKind::EllipseArc, "Partial ellipse"),
                    ],
                ) {
                    self.switch_tool(k);
                }
                caption(ui, "C cycles");
            }
            ToolKind::Arc | ToolKind::ThreePointArc | ToolKind::TangentArc => {
                if let Some(k) = choice(
                    ui,
                    kind,
                    &[
                        (ToolKind::Arc, "Centre"),
                        (ToolKind::ThreePointArc, "3-point"),
                        (ToolKind::TangentArc, "Tangent"),
                    ],
                ) {
                    self.switch_tool(k);
                }
                caption(ui, "A cycles");
            }
            ToolKind::Polygon => {
                caption(ui, "Sides");
                let mut sides = if self.editor.polygon_sides == 0 {
                    6
                } else {
                    self.editor.polygon_sides
                };
                if ui
                    .add(egui::DragValue::new(&mut sides).range(3..=64).speed(0.1))
                    .changed()
                {
                    self.editor.polygon_sides = sides;
                }
                if let Some(c) = choice(
                    ui,
                    self.editor.polygon_circumscribed,
                    &[(false, "Inscribed"), (true, "Circumscribed")],
                ) {
                    self.editor.polygon_circumscribed = c;
                }
                caption(ui, "X switches");
            }
            ToolKind::Slot => {
                if let Some(m) = choice(
                    ui,
                    self.editor.slot_mode,
                    &[
                        (SlotMode::Straight, "Straight"),
                        (SlotMode::Centre, "Centre"),
                        (SlotMode::Arc, "Arc"),
                    ],
                ) {
                    self.editor.slot_mode = m;
                    self.tool.cancel(&mut self.editor);
                }
                caption(ui, "U cycles");
            }
            ToolKind::Fillet | ToolKind::Chamfer => {
                if let Some(k) = choice(
                    ui,
                    kind,
                    &[(ToolKind::Fillet, "Fillet"), (ToolKind::Chamfer, "Chamfer")],
                ) {
                    self.switch_tool(k);
                }
                caption(ui, "Type the size · F switches");
            }
            ToolKind::Text => {
                caption(ui, "Height");
                let unit = units.length.mm_per_unit();
                let mut shown = if self.editor.text_height > 0.0 {
                    self.editor.text_height
                } else {
                    10.0
                } / unit;
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .speed(0.1)
                            .range(0.001..=1e6)
                            .max_decimals(3)
                            .suffix(format!(" {}", units.length.symbol())),
                    )
                    .on_hover_text("Cap height of new text")
                    .changed()
                {
                    self.editor.text_height = shown * unit;
                }
            }
            ToolKind::Pen => {
                ui.checkbox(&mut self.show_combs, "Curvature combs");
                caption(
                    ui,
                    "Click for corners, drag for smooth points · Enter finishes",
                );
            }
            ToolKind::Dimension | ToolKind::Ordinate | ToolKind::Note | ToolKind::Callout => {
                ui.checkbox(&mut self.show_dimensions, "Show dimensions")
                    .on_hover_text("Alt+D");
                ui.checkbox(&mut self.show_relations, "Show relations")
                    .on_hover_text("Alt+R");
            }
            _ => {
                ui.add(
                    egui::Label::new(egui::RichText::new(kind.description()).size(12.0).weak())
                        .truncate(),
                );
            }
        }
    }

    /// Quick actions for the selection (the select tool's options).
    fn selection_actions(&mut self, ui: &mut egui::Ui, now: f64) {
        let ed = &self.editor;
        let n = ed.selection.len() + ed.selected_instances.len();
        if n == 0 {
            caption(
                ui,
                "Click or drag to select · Tab picks a chain · double-click a group to enter it",
            );
            return;
        }
        let grouped = ed.selection.iter().any(|id| ed.doc.group_of(*id).is_some());
        let curves = !ed.selection.is_empty();
        caption(ui, &format!("{n} selected"));
        divider(ui);
        if bar_button(
            ui,
            "Duplicate",
            "Shift+D",
            "Copy in place, then move the copy",
        ) {
            self.duplicate_and_grab();
        }
        if bar_button(ui, "Move", "G", "Move the selection (type a distance)") {
            self.tool
                .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
        }
        if bar_button(ui, "Rotate", "R", "Rotate the selection (type an angle)") {
            self.tool
                .key(ToolKey::R, self.pointer.as_ref(), &mut self.editor);
        }
        if curves && bar_button(ui, "Mirror", "M", "Mirror about a line you click") {
            self.switch_tool(ToolKind::Mirror);
        }
        divider(ui);
        if curves
            && self.editor.selection.len() >= 2
            && bar_button(ui, "Group", "", "Group (Ctrl+G)")
        {
            self.editor.make_group();
        }
        if grouped && bar_button(ui, "Ungroup", "", "Ungroup (Ctrl+Shift+G)") {
            self.editor.ungroup();
        }
        if curves
            && bar_button(
                ui,
                "Construction",
                "X",
                "Toggle construction geometry (guides that don't print)",
            )
        {
            self.editor.toggle_construction();
        }
        if curves
            && bar_button(ui, "Make block", "", "A block from the selection")
            && let Err(e) = self.editor.make_block("")
        {
            self.set_status(e, true, now);
        }
        divider(ui);
        let red = ui.visuals().error_fg_color;
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Delete").size(12.5).color(red))
                    .frame_when_inactive(false)
                    .min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("Delete the selection (Del)")
            .clicked()
        {
            self.editor.delete_selection();
        }
    }

    /// Duplicate the selection in place and grab the copy (Shift+D).
    pub(crate) fn duplicate_and_grab(&mut self) {
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        if self.editor.duplicate_selection() > 0 {
            self.tool
                .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
        }
    }

    /// The canvas's right-click menu (with the select tool): actions for the selection, or
    /// for the view when nothing is picked.
    pub(crate) fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.set_min_width(210.0);
        let item = |ui: &mut egui::Ui, label: &str, keys: &str| {
            ui.add(egui::Button::new(label).shortcut_text(keys))
                .clicked()
        };
        let n = self.editor.selection.len() + self.editor.selected_instances.len();
        if n == 0 {
            if item(ui, "Select all", &app::panels::shortcut("A", false)) {
                self.editor.select_all();
                ui.close();
            }
            if item(ui, "Zoom to fit", "Shift+F") {
                self.fit_target = Some(self.fit_bounds());
                ui.close();
            }
            ui.separator();
            let mut grid = self.show_grid;
            if ui.checkbox(&mut grid, "Grid").changed() {
                self.show_grid = grid;
            }
            let mut snap = self.grid_snap;
            if ui.checkbox(&mut snap, "Snap to grid").changed() {
                self.grid_snap = snap;
            }
            ui.separator();
            if item(ui, "Command palette…", &app::panels::shortcut("K", false)) {
                self.open_palette();
                ui.close();
            }
            return;
        }
        ui.label(egui::RichText::new(format!("{n} selected")).small().weak());
        if item(ui, "Duplicate", "Shift+D") {
            ui.close();
            self.duplicate_and_grab();
        }
        if item(ui, "Move", "G") {
            ui.close();
            self.tool
                .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
        }
        if item(ui, "Rotate", "R") {
            ui.close();
            self.tool
                .key(ToolKey::R, self.pointer.as_ref(), &mut self.editor);
        }
        let curves = !self.editor.selection.is_empty();
        if curves && item(ui, "Mirror…", "M") {
            ui.close();
            self.switch_tool(ToolKind::Mirror);
        }
        // Relations and dimensions that fit the selection.
        let relations = self.editor.relation_options();
        let dimensions = self.editor.dimension_options();
        if !relations.is_empty() || !dimensions.is_empty() {
            ui.separator();
        }
        for (title, options, is_dimension) in [
            ("Add relation", relations, false),
            ("Add dimension", dimensions, true),
        ] {
            if options.is_empty() {
                continue;
            }
            ui.menu_button(title, |ui| {
                for option in &options {
                    if ui.button(option.label).clicked() {
                        if let Some(id) = self.apply_constraint_option(option, is_dimension, now) {
                            self.open_dimension_edit(id);
                        }
                        ui.close();
                    }
                }
            });
        }
        if curves {
            ui.separator();
            let grouped = self
                .editor
                .selection
                .iter()
                .any(|id| self.editor.doc.group_of(*id).is_some());
            if self.editor.selection.len() >= 2
                && item(ui, "Group", &app::panels::shortcut("G", false))
            {
                self.editor.make_group();
                ui.close();
            }
            if grouped && item(ui, "Ungroup", &app::panels::shortcut("G", true)) {
                self.editor.ungroup();
                ui.close();
            }
            if item(ui, "Toggle construction", "X") {
                self.editor.toggle_construction();
                ui.close();
            }
            if item(ui, "Select chain", "Tab") {
                self.editor.select_chain();
                ui.close();
            }
            let layers: Vec<(drift_core::LayerId, String)> = self
                .editor
                .doc
                .layers()
                .map(|(id, l)| (id, l.name.clone()))
                .collect();
            ui.menu_button("Move to layer", |ui| {
                for (id, name) in layers {
                    if ui.button(name).clicked() {
                        self.editor.move_selection_to_layer(id);
                        ui.close();
                    }
                }
            });
            if item(ui, "Make block", "") {
                if let Err(e) = self.editor.make_block("") {
                    self.set_status(e, true, now);
                }
                ui.close();
            }
        }
        if !self.editor.selected_instances.is_empty() && item(ui, "Explode block", "") {
            self.editor.explode_selected();
            ui.close();
        }
        ui.separator();
        let red = ui.visuals().error_fg_color;
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Delete").color(red)).shortcut_text(
                    if cfg!(target_os = "macos") {
                        "⌫"
                    } else {
                        "Del"
                    },
                ),
            )
            .clicked()
        {
            self.editor.delete_selection();
            ui.close();
        }
    }
}
