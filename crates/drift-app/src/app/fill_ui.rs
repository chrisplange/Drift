//! The inspector for fills and hatches, and the Fill tool's settings.

use drift_core::{FillId, FillKind, Hatch};
use drift_draft::{Pattern, Placement};

use crate::fills::{FillStyle, PatternLibrary};
use crate::layers_ui::{color_choice, weight_combo};
use crate::*;

/// A small swatch of a hatch pattern, drawn with the painter.
fn swatch(ui: &mut egui::Ui, pattern: Option<&Pattern>, size: egui::Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 3.0, visuals.extreme_bg_color);
    match pattern {
        None => {
            painter.rect_filled(
                rect.shrink(1.0),
                3.0,
                visuals.text_color().gamma_multiply(0.45),
            );
        }
        Some(pattern) => {
            let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
            // (The swatch itself, as a region: no need to find it among curves each paint.)
            let region = drift_geom::regions::Region::rectangle(DVec2::ZERO, DVec2::new(w, h));
            {
                // Show model patterns (real-world size) shrunk to a few courses.
                let spacing = pattern.min_spacing();
                let scale = if spacing.is_finite() && spacing > 0.0 {
                    (3.5 / spacing).clamp(0.01, 4.0)
                } else {
                    1.0
                };
                let placement = Placement {
                    scale,
                    ..Placement::default()
                };
                let hatching = drift_draft::hatch(&region, pattern, &placement, None, 2000);
                let stroke = egui::Stroke::new(1.0, visuals.text_color());
                let to =
                    |p: DVec2| egui::pos2(rect.left() + p.x as f32, rect.bottom() - p.y as f32);
                for [a, b] in hatching.segments {
                    if a == b {
                        painter.circle_filled(to(a), 0.8, stroke.color);
                    } else {
                        painter.line_segment([to(a), to(b)], stroke);
                    }
                }
            }
        }
    }
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    response
}

/// A menu of the hatch patterns, each with its swatch.
fn pattern_combo(
    ui: &mut egui::Ui,
    h: &mut Hatch,
    patterns: &PatternLibrary,
    id: &str,
    width: f32,
) {
    egui::ComboBox::from_id_salt(("hatch_pattern", id))
        .selected_text(&h.pattern)
        .width(width)
        .height(360.0)
        .show_ui(ui, |ui| {
            for p in patterns.all() {
                ui.horizontal(|ui| {
                    swatch(ui, Some(p), egui::vec2(34.0, 18.0));
                    let label = if p.description.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}  ·  {}", p.name, p.description)
                    };
                    if ui
                        .selectable_label(h.pattern.eq_ignore_ascii_case(&p.name), label)
                        .clicked()
                    {
                        h.pattern = p.name.clone();
                        h.model = p.model;
                    }
                });
            }
        });
}

/// Edit a fill style. Returns true if it changed.
pub(crate) fn fill_style_editor(
    ui: &mut egui::Ui,
    style: &mut FillStyle,
    patterns: &PatternLibrary,
    dark: bool,
    id: &str,
) -> bool {
    let before = style.clone();
    let solid = matches!(style.kind, FillKind::Solid);
    if let Some(choice) = theme::segmented(ui, solid, &[(true, "Solid"), (false, "Hatch")]) {
        if choice && !solid {
            style.kind = FillKind::Solid;
        } else if !choice && solid {
            style.kind = FillKind::Hatch(Hatch::new("ANSI31"));
        }
    }
    ui.add_space(4.0);
    match &mut style.kind {
        FillKind::Solid => {
            theme::row(ui, "Colour", |ui| {
                if let Some(c) = color_choice(ui, style.color, "By layer", dark) {
                    style.color = c;
                }
            });
            theme::row(ui, "Opacity", |ui| {
                let mut percent = (style.opacity * 100.0).round();
                if ui
                    .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
                    .changed()
                {
                    style.opacity = percent / 100.0;
                }
            });
        }
        FillKind::Hatch(h) => {
            theme::row(ui, "Pattern", |ui| {
                ui.horizontal(|ui| {
                    swatch(ui, patterns.get(&h.pattern), egui::vec2(34.0, 20.0));
                    let width = ui.available_width() - 6.0;
                    pattern_combo(ui, h, patterns, id, width);
                });
            });
            theme::row(ui, "Scale", |ui| {
                ui.add(
                    egui::DragValue::new(&mut h.scale)
                        .speed(0.02)
                        .range(0.001..=10_000.0)
                        .prefix("× "),
                );
            });
            theme::row(ui, "Angle", |ui| {
                ui.add(
                    egui::DragValue::new(&mut h.angle)
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                );
            });
            // Drafting patterns keep their size on paper (scaled by the drawing scale); model
            // patterns have a real-world size and scale with the geometry.
            theme::row(ui, "Size", |ui| {
                if let Some(model) =
                    theme::segmented(ui, h.model, &[(false, "Drafting"), (true, "Model")])
                {
                    h.model = model;
                }
            });
            theme::row(ui, "Lines", |ui| {
                if let Some(c) = color_choice(ui, style.color, "By layer", dark) {
                    style.color = c;
                }
            });
            theme::row(ui, "Weight", |ui| {
                let mut w = style.weight;
                if weight_combo(ui, ("hatch_weight", id), &mut w, false) {
                    style.weight = w;
                }
            });
            theme::row(ui, "Background", |ui| {
                let mut on = h.background.is_some();
                if ui.checkbox(&mut on, "").changed() {
                    h.background = on.then_some(drift_core::Rgb([0xff, 0xf4, 0xd6]));
                }
                if let Some(bg) = &mut h.background {
                    let mut rgb = bg.0;
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                        *bg = drift_core::Rgb(rgb);
                    }
                }
            });
        }
    }
    *style != before
}

impl DriftApp {
    /// The inspector's fill sections: selected fills, the Fill tool's settings, or buttons to
    /// fill the selected region. Returns true if it showed something that replaces the usual
    /// "No selection" content.
    pub(crate) fn fill_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let dark = ui.visuals().dark_mode;
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let fills: Vec<FillId> = self
            .editor
            .selected_fills
            .iter()
            .copied()
            .filter(|id| self.editor.doc.fill(*id).is_some())
            .collect();
        if let Some(&first) = fills.first() {
            let fill = self.editor.doc.fill(first).cloned().expect("exists");
            let shapes = self.editor.fill_shapes();
            let shape = shapes.get(&first);
            let title = match (&fill.kind, fills.len()) {
                (FillKind::Solid, 1) => "Fill".to_owned(),
                (FillKind::Hatch(_), 1) => "Hatch".to_owned(),
                (_, n) => format!("{n} fills"),
            };
            app::panels::title(ui, Some(ToolKind::Fill), &title, "");
            if shape.is_none_or(|s| s.lost) {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Boundary lost: its curves no longer enclose an area. It keeps its last \
                         shape until you re-pick it.",
                    )
                    .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d))
                    .small(),
                );
                if ui.button("Re-pick boundary").clicked() {
                    self.editor.repick_fill = Some(first);
                    self.set_tool(ToolKind::Fill);
                }
            }
            if let Some(parts) = shape.filter(|_| fill.divided).map(|s| s.parts) {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Covers {parts} areas: an edit divided its area, and it was kept whole."
                    ))
                    .color(theme::warning_color(ui.visuals().dark_mode))
                    .small(),
                );
                ui.horizontal_wrapped(|ui| {
                    let result = if ui
                        .button("Fit to its seed's area")
                        .on_hover_text("Cover only the area where it was first clicked")
                        .clicked()
                    {
                        Some(self.editor.fit_fill_to_seed(first))
                    } else if ui
                        .button(format!("Split into {parts}"))
                        .on_hover_text("One fill per area, each in this style")
                        .clicked()
                    {
                        Some(self.editor.split_fill(first).map(|_| ()))
                    } else {
                        if ui
                            .button("Keep whole")
                            .on_hover_text("Clear the mark and keep covering every area")
                            .clicked()
                        {
                            self.editor.keep_fill_whole(first);
                        }
                        None
                    };
                    if let Some(Err(e)) = result {
                        self.editor.notice = Some(e);
                    }
                });
            }
            theme::section(ui, "Style");
            let mut style = FillStyle::of(&fill);
            if fills.len() == 1 && matches!(fill.kind, FillKind::Hatch(_)) {
                ui.label(
                    egui::RichText::new(
                        "On the canvas, drag the square handle to move the pattern (it snaps to points) and the round one to turn it.",
                    )
                    .small()
                    .weak(),
                );
            }
            if fill_style_editor(ui, &mut style, &self.hatch_patterns, dark, "selected") {
                self.editor.doc.begin();
                for id in &fills {
                    if let Some(mut f) = self.editor.doc.fill(*id).cloned() {
                        style.apply(&mut f);
                        self.editor.doc.set_fill(*id, f);
                    }
                }
                self.editor.commit("Edit fill");
            }
            theme::section(ui, "Layer");
            let layers: Vec<(drift_core::LayerId, String)> = self
                .editor
                .doc
                .layers()
                .map(|(id, l)| (id, l.name.clone()))
                .collect();
            let current = self
                .editor
                .doc
                .layer(fill.layer)
                .map_or("?".into(), |l| l.name.clone());
            let mut target = None;
            egui::ComboBox::from_id_salt("fill_layer")
                .selected_text(current)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for (id, name) in layers {
                        if ui.selectable_label(id == fill.layer, name).clicked() {
                            target = Some(id);
                        }
                    }
                });
            if let Some(layer) = target {
                self.editor.doc.begin();
                for id in &fills {
                    if let Some(mut f) = self.editor.doc.fill(*id).cloned() {
                        f.layer = layer;
                        self.editor.doc.set_fill(*id, f);
                    }
                }
                self.editor.commit("Move fill to layer");
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Bring to front")
                    .on_hover_text("Draw above the other fills on its layer")
                    .clicked()
                {
                    self.editor.reorder_fill(first, true);
                }
                if ui
                    .button("Send to back")
                    .on_hover_text("Draw below the other fills on its layer")
                    .clicked()
                {
                    self.editor.reorder_fill(first, false);
                }
            });
            if let Some(shape) = shape {
                let units = self.editor.doc.units();
                let r = &shape.region;
                theme::section(ui, "Area");
                theme::row(ui, "Area", |ui| {
                    ui.label(units.format_area(r.area));
                });
                theme::row(ui, "Perimeter", |ui| {
                    ui.label(units.format_readout(r.perimeter));
                });
                theme::row(ui, "Centroid", |ui| {
                    ui.label(format!(
                        "{}, {}",
                        units.format_readout(r.centroid.x),
                        units.format_readout(r.centroid.y)
                    ));
                });
            }
            ui.add_space(10.0);
            if theme::danger_button(ui, "Delete").clicked() {
                self.editor.delete_selection();
            }
            return true;
        }
        false
    }

    /// The Fill tool's inspector: the style the next fill gets, whatever is selected (a
    /// fill just made is selected; clicking it again gives it this style).
    pub(crate) fn fill_tool_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        if self.tool.kind() != ToolKind::Fill {
            return false;
        }
        let dark = ui.visuals().dark_mode;
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        app::panels::title(ui, Some(ToolKind::Fill), "Fill / hatch", "New fills");
        ui.label(
            egui::RichText::new(if self.editor.repick_fill.is_some() {
                "Click inside the area that should be the fill's new boundary."
            } else if self.editor.fill_eyedropper {
                "Click a fill or hatch to pick up its style."
            } else {
                "Click inside a closed area to fill it with this style; click a fill again to \
                 give it this style. Alt+click a fill to pick up its style."
            })
            .small()
            .weak(),
        );
        theme::section(ui, "New fills");
        let mut style = self.editor.fill_style.clone();
        if fill_style_editor(ui, &mut style, &self.hatch_patterns, dark, "tool") {
            self.editor.fill_style = style;
        }
        ui.add_space(6.0);
        self.eyedropper_button(ui);
        true
    }

    fn eyedropper_button(&mut self, ui: &mut egui::Ui) {
        let on = self.editor.fill_eyedropper;
        if ui
            .add(egui::Button::new("Pick up a style").selected(on))
            .on_hover_text("Click a fill or hatch to use its style for new fills (or Alt+click it)")
            .clicked()
        {
            self.editor.fill_eyedropper = !on;
        }
    }

    /// The Fill tool's bar: solid or hatch, the colour or pattern, and the eyedropper.
    pub(crate) fn fill_tool_bar(&mut self, ui: &mut egui::Ui) {
        self.fill_style_bar(ui);
        self.eyedropper_button(ui);
    }

    /// The drawing tools' Fill control: whether closed shapes come out filled as they are
    /// drawn, and in what style (the Fill tool's).
    pub(crate) fn new_shape_fill_bar(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.editor.fill_new_shapes, "Fill")
            .on_hover_text(
                "Closed shapes are filled or hatched as they are drawn, in this style (the \
                 Fill tool's); the fill is part of the shape's undo step",
            );
        if self.editor.fill_new_shapes {
            self.fill_style_bar(ui);
        }
    }

    /// Solid or hatch, and the colour or pattern, of the current fill style.
    fn fill_style_bar(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let mut style = self.editor.fill_style.clone();
        let solid = matches!(style.kind, FillKind::Solid);
        if let Some(choice) =
            app::context_ui::choice(ui, solid, &[(true, "Solid"), (false, "Hatch")])
        {
            style.kind = if choice {
                FillKind::Solid
            } else {
                FillKind::Hatch(Hatch::new("ANSI31"))
            };
        }
        match &mut style.kind {
            FillKind::Solid => {
                if let Some(c) = color_choice(ui, style.color, "By layer", dark) {
                    style.color = c;
                }
            }
            FillKind::Hatch(h) => {
                swatch(
                    ui,
                    self.hatch_patterns.get(&h.pattern),
                    egui::vec2(34.0, 20.0),
                );
                pattern_combo(ui, h, &self.hatch_patterns, "bar", 120.0);
            }
        }
        if style != self.editor.fill_style {
            self.editor.fill_style = style;
        }
    }

    /// Buttons under a selected region's measurements: fill or hatch it.
    pub(crate) fn region_fill_buttons(&mut self, ui: &mut egui::Ui) {
        let Some(seed) = self.editor.selected_region else {
            return;
        };
        ui.add_space(6.0);
        let mut make = None;
        ui.horizontal(|ui| {
            if ui
                .button("Fill")
                .on_hover_text("Fill this area with a solid colour")
                .clicked()
            {
                make = Some(FillKind::Solid);
            }
            if ui
                .button("Hatch")
                .on_hover_text("Hatch this area with the Fill tool's pattern")
                .clicked()
            {
                make = Some(match &self.editor.fill_style.kind {
                    FillKind::Hatch(h) => FillKind::Hatch(h.clone()),
                    FillKind::Solid => FillKind::Hatch(Hatch::new("ANSI31")),
                });
            }
        });
        if let Some(kind) = make {
            let saved = self.editor.fill_style.clone();
            self.editor.fill_style.kind = kind;
            let result = self.editor.add_fill_at(seed);
            self.editor.fill_style = saved;
            if let Err(e) = result {
                let now = ui.input(|i| i.time);
                self.set_status(e, true, now);
            }
        }
    }
}
