//! The Parameters tab (variables with sliders, options, pinned measurements) and the canvas
//! overlays that go with it (free-direction arrows, option ghosts).

use std::collections::{BTreeSet, HashMap};

use drift_core::units::FRACTIONS;
use drift_core::{
    EntityId, Kind, LengthFormat, Shape, SketchItem, VariableId, VariantId, dimension_kind,
};
use drift_solver::Freedom;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2, vec2};

use crate::DriftApp;
use crate::overlay::ScreenMap;

/// Edit buffers and view state for the Parameters tab.
#[derive(Default)]
pub struct ParamUi {
    new_name: String,
    new_expr: String,
    new_kind: Kind,
    /// Expression / name text being edited, per variable.
    expr_text: HashMap<VariableId, String>,
    name_text: HashMap<VariableId, String>,
    /// A slider's range, fixed while it is dragged.
    slider_range: Option<(VariableId, f64, f64)>,
    option_name: String,
    /// Whether a new option keeps the whole sketch (or only the variable values).
    option_whole: bool,
    /// An option being renamed, and the name typed so far.
    renaming: Option<(VariantId, String)>,
    /// Options drawn faintly as ghosts (for the session, like other view switches).
    pub ghosted: BTreeSet<VariantId>,
    pub show_free: bool,
    /// Each ghost's geometry as of a document revision (`None`: it doesn't solve).
    ghosts: HashMap<VariantId, (u64, Option<Vec<SketchItem>>)>,
    /// How many times a ghost's geometry was worked out (tests count them).
    pub(crate) ghost_computations: usize,
    free: Option<(u64, HashMap<EntityId, Freedom>)>,
}

/// A slider range around a value: from a tenth to double (or symmetric for zero).
fn slider_range(kind: Kind, v: f64) -> (f64, f64) {
    match kind {
        Kind::Angle => (v.min(0.0).min(v - 90.0), v.max(180.0).max(v + 90.0)),
        _ if v > 0.0 => (v * 0.1, v * 2.0),
        _ if v < 0.0 => (v * 2.0, v * 0.1),
        _ => (-10.0, 10.0),
    }
}

impl DriftApp {
    pub(crate) fn parameters_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // The drawing's units and drafting standard are in Preferences: this tab is
                // for what drives the sketch.
                self.variables_section(ui, now);
                self.options_section(ui, now);
                self.measurements_section(ui);
                self.patterns_section(ui);
            });
        self.panel_scrolled(ui, crate::dock::Panel::Parameters, &output);
    }

    fn patterns_section(&mut self, ui: &mut egui::Ui) {
        crate::theme::section(ui, "Hatch patterns");
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let custom: Vec<(String, String)> = self
            .hatch_patterns
            .custom()
            .iter()
            .map(|p| (p.name.clone(), p.description.clone()))
            .collect();
        if custom.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "{} built-in patterns. Import AutoCAD or Revit .pat files to add your own; \
                     they are saved in the drawing.",
                    drift_draft::builtin().len()
                ))
                .weak(),
            );
        }
        let mut remove = None;
        for (name, description) in &custom {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).family(crate::theme::bold()));
                ui.label(egui::RichText::new(description).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("×").frame(false))
                        .on_hover_text("Remove this pattern from the drawing")
                        .clicked()
                    {
                        remove = Some(name.clone());
                    }
                });
            });
        }
        if let Some(name) = remove {
            self.remove_pattern(&name);
        }
        if ui.button("Import .pat…").clicked() {
            self.pick_file(crate::app::document::Picking::Patterns);
        }
    }

    pub(crate) fn units_section(&mut self, ui: &mut egui::Ui) {
        crate::theme::section(ui, "Units");
        let mut units = self.editor.doc.units();
        let before = units;
        let metric = units.length.is_metric();
        match crate::theme::segmented(ui, metric, &[(true, "Metric"), (false, "Imperial")]) {
            Some(true) if !metric => units.length = LengthFormat::Millimetre,
            Some(false) if metric => units.length = LengthFormat::Inch,
            _ => {}
        }
        ui.add_space(4.0);
        let formats: &[LengthFormat] = if units.length.is_metric() {
            &LengthFormat::METRIC
        } else {
            &LengthFormat::IMPERIAL
        };
        crate::theme::row(ui, "Format", |ui| {
            egui::ComboBox::from_id_salt("length_format")
                .selected_text(units.length.label())
                .width(ui.available_width() - 4.0)
                .show_ui(ui, |ui| {
                    for &f in formats {
                        ui.selectable_value(&mut units.length, f, f.label());
                    }
                });
        });
        if units.length.is_fractional() {
            crate::theme::row(ui, "Round to", |ui| {
                egui::ComboBox::from_id_salt("fraction")
                    .selected_text(format!("1/{}\"", units.fraction))
                    .show_ui(ui, |ui| {
                        for f in FRACTIONS {
                            ui.selectable_value(&mut units.fraction, f, format!("1/{f}\""));
                        }
                    });
                ui.checkbox(&mut units.zero_feet, "Show 0'");
            });
        } else {
            crate::theme::row(ui, "Decimals", |ui| {
                ui.add(egui::DragValue::new(&mut units.decimals).range(0..=8));
            });
        }
        crate::theme::row(ui, "Angles", |ui| {
            ui.add(egui::DragValue::new(&mut units.angle_decimals).range(0..=8));
            ui.label(egui::RichText::new("decimals").weak());
        });
        if units != before {
            self.editor.set_units(units);
        }
    }

    fn variables_section(&mut self, ui: &mut egui::Ui, now: f64) {
        crate::theme::section(ui, "Variables");
        let units = self.editor.doc.units();
        let vars: Vec<_> = self
            .editor
            .doc
            .variables()
            .map(|(id, v)| (id, v.clone()))
            .collect();
        if vars.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Name values (width = 120) and use them in dimensions: type \
                     =width or width/2 into a dimension.",
                )
                .weak(),
            );
        }
        let mut status = None;
        let mut remove = None;
        for (id, var) in &vars {
            let value = self.editor.evaluation().value(&self.editor.doc, *id);
            ui.horizontal(|ui| {
                // Name (renamed on Enter or when focus leaves).
                let name = self
                    .params
                    .name_text
                    .entry(*id)
                    .or_insert_with(|| var.name.clone());
                let r = ui
                    .add(egui::TextEdit::singleline(name).desired_width(70.0))
                    .on_hover_text(format!("A {}", var.kind.label().to_lowercase()));
                if r.lost_focus() {
                    let text = std::mem::take(name);
                    self.params.name_text.remove(id);
                    if text.trim() != var.name
                        && let Err(e) = self.editor.rename_variable(*id, &text)
                    {
                        status = Some(format!("Can't rename {}: {e}", var.name));
                    }
                } else if !r.has_focus() {
                    *name = var.name.clone();
                }
                ui.label("=");
                let expr = self
                    .params
                    .expr_text
                    .entry(*id)
                    .or_insert_with(|| var.expr.clone());
                let r = ui.add(
                    egui::TextEdit::singleline(expr)
                        .desired_width(100.0)
                        .id(egui::Id::new(("var_expr", id.0))),
                );
                if r.lost_focus() {
                    let text = std::mem::take(expr);
                    self.params.expr_text.remove(id);
                    if ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && text.trim() != var.expr
                        && let Err(e) = self.editor.set_variable(*id, &text)
                    {
                        status = Some(format!("Can't set {}: {e}", var.name));
                    }
                } else if !r.has_focus() {
                    *expr = var.expr.clone();
                }
                if crate::icons::button(ui, 22.0, false, "Delete variable", |p, r, c| {
                    crate::icons::glyph(p, r, crate::icons::Glyph::Delete, c)
                })
                .on_hover_text("Delete variable")
                .clicked()
                {
                    remove = Some(*id);
                }
            });
            // A plain value gets a slider to scrub it; an expression shows what it comes to.
            match value {
                Some(Ok(v)) if self.editor.is_scrubbable(*id) => {
                    self.slider(ui, *id, var.kind, v);
                }
                Some(Ok(v)) => {
                    ui.horizontal(|ui| {
                        ui.add_space(84.0);
                        ui.label(
                            egui::RichText::new(format!("= {}", units.format(var.kind, v))).weak(),
                        );
                    });
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                }
                None => {}
            }
            ui.add_space(2.0);
        }
        if let Some(id) = remove
            && let Err(e) = self.editor.remove_variable(id)
        {
            status = Some(format!("Can't delete it: {e}"));
        }
        // Add a variable.
        ui.add_space(4.0);
        let mut enter = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.params.new_name)
                    .hint_text("name")
                    .desired_width(80.0),
            );
            ui.label("=");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.params.new_expr)
                    .hint_text("value or expression")
                    .desired_width(ui.available_width() - 6.0),
            );
            enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("new_kind")
                .selected_text(self.params.new_kind.label())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut self.params.new_kind, k, k.label());
                    }
                });
            if ui.button("Add").clicked() || enter {
                match self.editor.add_variable(
                    &self.params.new_name,
                    &self.params.new_expr,
                    self.params.new_kind,
                ) {
                    Ok(_) => {
                        self.params.new_name.clear();
                        self.params.new_expr.clear();
                    }
                    Err(e) => status = Some(format!("Can't add it: {e}")),
                }
            }
        });
        if let Some(text) = status {
            self.set_status(text, true, now);
        }
    }

    fn slider(&mut self, ui: &mut egui::Ui, id: VariableId, kind: Kind, value: f64) {
        let (lo, hi) = match self.params.slider_range {
            Some((v, lo, hi)) if v == id => (lo, hi),
            _ => slider_range(kind, value),
        };
        let mut v = value;
        let r = ui.add(
            egui::Slider::new(&mut v, lo..=hi)
                .show_value(false)
                .clamping(egui::SliderClamping::Always),
        );
        if r.drag_started() {
            self.params.slider_range = Some((id, lo, hi));
        }
        if r.changed() && v != value {
            self.editor.scrub_variable(id, v);
        }
        if !r.dragged() && self.editor.is_scrubbing() {
            self.editor.end_scrub();
            self.params.slider_range = None;
        }
    }

    fn options_section(&mut self, ui: &mut egui::Ui, now: f64) {
        crate::theme::section(ui, "Options");
        let active = self.editor.active_variant();
        let options: Vec<_> = self
            .editor
            .doc
            .variants()
            .map(|(id, v)| (id, v.name.clone(), v.is_sketch()))
            .collect();
        if options.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Save the drawing as it is to come back to it in one step: its variable \
                     values, or the whole sketch.",
                )
                .weak(),
            );
        }
        let mut status = None;
        for (id, name, whole) in options {
            ui.horizontal(|ui| {
                let kind = if whole { "Whole sketch" } else { "Values only" };
                if let Some((rid, text)) = &mut self.params.renaming
                    && *rid == id
                {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                    if r.lost_focus() {
                        let text = std::mem::take(text);
                        self.params.renaming = None;
                        if !text.trim().is_empty() && text.trim() != name {
                            self.editor.rename_variant(id, &text);
                        }
                    } else {
                        r.request_focus();
                    }
                } else {
                    let r = ui
                        .radio(active == Some(id), &name)
                        .on_hover_text(format!("{kind}. Click to apply; double-click to rename"));
                    if r.double_clicked() {
                        self.params.renaming = Some((id, name.clone()));
                    } else if r.clicked()
                        && active != Some(id)
                        && let Err(e) = self.editor.apply_variant(id)
                    {
                        status = Some(format!("Can't apply {name}: {e}"));
                    }
                }
                ui.label(
                    egui::RichText::new(if whole { "sketch" } else { "values" })
                        .small()
                        .weak(),
                );
                let mut shown = self.params.ghosted.contains(&id);
                let tip = if active == Some(id) {
                    "Ghost: the drawing is in this option now"
                } else {
                    "Show this option faintly behind the drawing"
                };
                if crate::layers_ui::icon_toggle(
                    ui,
                    &mut shown,
                    crate::icons::Glyph::Eye,
                    true,
                    tip,
                ) {
                    if shown {
                        self.params.ghosted.insert(id);
                    } else {
                        self.params.ghosted.remove(&id);
                    }
                }
                let update_tip = if whole {
                    "Store the sketch as it is now in this option"
                } else {
                    "Store the current values in this option"
                };
                if ui
                    .small_button("Update")
                    .on_hover_text(update_tip)
                    .clicked()
                {
                    self.editor.update_variant(id);
                }
                if ui.small_button("×").on_hover_text("Delete").clicked() {
                    self.editor.remove_variant(id);
                    self.params.ghosted.remove(&id);
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.params.option_name)
                    .hint_text("name")
                    .desired_width(110.0),
            );
            egui::ComboBox::from_id_salt("option_kind")
                .selected_text(if self.params.option_whole {
                    "Whole sketch"
                } else {
                    "Values only"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.params.option_whole, false, "Values only")
                        .on_hover_text("The variables' values, applied over the sketch as it is");
                    ui.selectable_value(&mut self.params.option_whole, true, "Whole sketch")
                        .on_hover_text("Everything in the drawing, to return to as it was");
                });
        });
        if ui
            .button("Save current")
            .on_hover_text("Save the drawing as it is now as an option (saved in the file)")
            .clicked()
        {
            let n = self.editor.doc.variants().count() + 1;
            let name = if self.params.option_name.trim().is_empty() {
                format!("Option {n}")
            } else {
                std::mem::take(&mut self.params.option_name)
            };
            self.editor.save_option(&name, self.params.option_whole);
        }
        if let Some(text) = status {
            self.set_status(text, true, now);
        }
    }

    fn measurements_section(&mut self, ui: &mut egui::Ui) {
        crate::theme::section(ui, "Pinned measurements");
        let units = self.editor.doc.units();
        let pinned: Vec<_> = self
            .editor
            .doc
            .measurements()
            .map(|(id, c)| (id, *c))
            .collect();
        if pinned.is_empty() {
            ui.label(
                egui::RichText::new("Select geometry and use Pin measurement in the Inspector.")
                    .weak(),
            );
        }
        let mut unpin = None;
        for (id, c) in pinned {
            ui.horizontal(|ui| {
                ui.label(crate::constraint_label(&self.editor.doc, &c));
                let value = drift_solver::measure(&self.editor.doc, &c)
                    .map_or("—".into(), |v| units.format(dimension_kind(&c), v));
                ui.label(value);
                if ui.small_button("Unpin").clicked() {
                    unpin = Some(id);
                }
            });
        }
        if let Some(id) = unpin {
            self.editor.unpin_measurement(id);
        }
    }

    /// Free-direction arrows on under-defined points.
    pub(crate) fn paint_free_directions(&mut self, painter: &egui::Painter, map: &ScreenMap) {
        // Hidden mid-drag (and mid-scrub): they would be stale, and recomputing them every
        // frame would slow the drag on large sketches.
        if !self.params.show_free || self.editor.doc.in_transaction() {
            return;
        }
        let revision = self.editor.doc.revision();
        if self
            .params
            .free
            .as_ref()
            .is_none_or(|(r, _)| *r != revision)
        {
            self.params.free = Some((revision, drift_solver::free_directions(&self.editor.doc)));
        }
        let Some((_, free)) = &self.params.free else {
            return;
        };
        let color = Color32::from_rgb(0x3d, 0xa5, 0xf5);
        let stroke = Stroke::new(1.5, color);
        let arrow = |from: Pos2, dir: Vec2| {
            let tip = from + dir * 13.0;
            painter.line_segment([from + dir * 4.0, tip], stroke);
            let n = dir.rot90() * 3.0;
            painter.line_segment([tip, tip - dir * 4.0 + n], stroke);
            painter.line_segment([tip, tip - dir * 4.0 - n], stroke);
        };
        for (id, freedom) in free {
            let Ok(p) = self.editor.doc.point(*id) else {
                continue;
            };
            let at = map.to_screen(p);
            if !map.rect.contains(at) {
                continue;
            }
            match freedom {
                Freedom::Line(d) => {
                    // World y-up → screen y-down.
                    let dir = vec2(d.x as f32, -(d.y as f32));
                    arrow(at, dir);
                    arrow(at, -dir);
                }
                Freedom::Plane => {
                    for dir in [
                        vec2(1.0, 0.0),
                        vec2(-1.0, 0.0),
                        vec2(0.0, 1.0),
                        vec2(0.0, -1.0),
                    ] {
                        arrow(at, dir * 0.8);
                    }
                }
            }
        }
    }

    /// The ghosts' geometry, worked out again after each committed edit (never during a
    /// drag or any edit under way, which would cost a solve per option per frame).
    pub(crate) fn refresh_ghosts(&mut self) {
        let doc = &self.editor.doc;
        self.params.ghosted.retain(|id| doc.variant(*id).is_some());
        let ghosted = &self.params.ghosted;
        self.params.ghosts.retain(|id, _| ghosted.contains(id));
        if doc.in_transaction() {
            return;
        }
        let revision = doc.revision();
        for &id in &self.params.ghosted {
            if self
                .params
                .ghosts
                .get(&id)
                .is_none_or(|(r, _)| *r != revision)
            {
                self.params.ghost_computations += 1;
                let items = self.editor.variant_geometry(id);
                self.params.ghosts.insert(id, (revision, items));
            }
        }
    }

    /// The options picked as ghosts, drawn faintly behind the sketch (not the one the
    /// drawing is in: it would cover the drawing exactly).
    pub(crate) fn paint_ghosts(&mut self, painter: &egui::Painter, map: &ScreenMap) {
        if self.params.ghosted.is_empty() {
            return;
        }
        self.refresh_ghosts();
        let active = self.editor.active_variant();
        let palette = [
            Color32::from_rgb(0xb3, 0x88, 0xff),
            Color32::from_rgb(0xff, 0x9e, 0x5e),
            Color32::from_rgb(0x4d, 0xd0, 0xc4),
            Color32::from_rgb(0xf0, 0x6e, 0xaa),
        ];
        let order: Vec<VariantId> = self.editor.doc.variants().map(|(id, _)| id).collect();
        for (i, id) in order.iter().enumerate() {
            if Some(*id) == active {
                continue;
            }
            let Some((_, Some(items))) = self.params.ghosts.get(id) else {
                continue;
            };
            let stroke = Stroke::new(1.2, palette[i % palette.len()].gamma_multiply(0.55));
            for item in items {
                paint_shape(painter, map, &item.shape, stroke);
            }
        }
    }
}

fn paint_shape(painter: &egui::Painter, map: &ScreenMap, shape: &Shape, stroke: Stroke) {
    match *shape {
        Shape::Line { a, b } => {
            painter.line_segment([map.to_screen(a), map.to_screen(b)], stroke);
        }
        Shape::Circle { center, radius } => {
            painter.circle_stroke(
                map.to_screen(center),
                (radius * map.camera.scale) as f32,
                stroke,
            );
        }
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let r_px = radius * map.camera.scale;
            let steps = ((sweep.abs() * r_px.sqrt()).ceil() as usize).clamp(8, 256);
            let points: Vec<Pos2> = (0..=steps)
                .map(|k| {
                    let a = start + sweep * k as f64 / steps as f64;
                    map.to_screen(center + radius * drift_core::DVec2::from_angle(a))
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            let points = drift_core::shape::flatten_ellipse(center, major, minor, start, sweep, 64)
                .into_iter()
                .map(|p| map.to_screen(p))
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        Shape::Bezier { p } => {
            let points = drift_core::shape::flatten_bezier(&p, 32)
                .into_iter()
                .map(|q| map.to_screen(q))
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        Shape::Point(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_ranges_bracket_the_value() {
        assert_eq!(slider_range(Kind::Length, 20.0), (2.0, 40.0));
        assert_eq!(slider_range(Kind::Number, 0.0), (-10.0, 10.0));
        let (lo, hi) = slider_range(Kind::Angle, 30.0);
        assert!(lo <= 0.0 && hi >= 120.0);
    }
}
