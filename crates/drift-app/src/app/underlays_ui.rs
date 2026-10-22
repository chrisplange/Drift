//! Underlays in the UI: what an underlay import reports, and the Underlays section of the
//! Layers panel. Files come in through the import dialog (`import_ui`).

use drift_core::{UnderlayId, underlays::MM_PER_POINT};

use crate::icons::Glyph;
use crate::*;

/// Drawing scales for PDF underlays: a label and n in 1 : n.
pub(crate) const SCALES: &[(&str, f64)] = &[
    ("1 : 1", 1.0),
    ("1 : 5", 5.0),
    ("1 : 10", 10.0),
    ("1 : 20", 20.0),
    ("1 : 25", 25.0),
    ("1 : 50", 50.0),
    ("1 : 100", 100.0),
    ("1 : 200", 200.0),
    ("1 : 500", 500.0),
    ("1 : 1000", 1000.0),
    ("3\" = 1'-0\"", 4.0),
    ("1-1/2\" = 1'-0\"", 8.0),
    ("1\" = 1'-0\"", 12.0),
    ("3/4\" = 1'-0\"", 16.0),
    ("1/2\" = 1'-0\"", 24.0),
    ("3/8\" = 1'-0\"", 32.0),
    ("1/4\" = 1'-0\"", 48.0),
    ("3/16\" = 1'-0\"", 64.0),
    ("1/8\" = 1'-0\"", 96.0),
    ("1/16\" = 1'-0\"", 192.0),
    ("1\" = 10'", 120.0),
    ("1\" = 20'", 240.0),
    ("1\" = 50'", 600.0),
];

fn is_pdf(source: &str) -> bool {
    source.contains(", page")
}

impl DriftApp {
    pub(crate) fn underlay_imported(
        &mut self,
        name: &str,
        result: Result<crate::underlays::UnderlayReport, String>,
        now: f64,
    ) {
        match result {
            Ok(report) => {
                self.abort_operation();
                let page = report
                    .page
                    .filter(|(_, n)| *n > 1)
                    .map_or(String::new(), |(p, n)| format!(" (page {p} of {n})"));
                let mut msg = format!(
                    "Underlay from {name}{page}: {} curves. Calibrate it in the Layers panel",
                    report.curves
                );
                if !report.skipped.is_empty() {
                    msg.push_str(&format!("; skipped {}", report.skipped));
                }
                self.set_status(msg, false, now);
                self.show_panel(crate::dock::Panel::Layers);
                self.fit_target = Some(self.fit_bounds());
            }
            Err(e) => self.set_status(format!("Could not import {name}: {e}"), true, now),
        }
    }

    /// The Underlays section of the Layers tab.
    pub(crate) fn underlays_section(&mut self, ui: &mut egui::Ui) {
        let underlays: Vec<(UnderlayId, drift_core::Underlay)> = self
            .editor
            .doc
            .underlays()
            .map(|(id, u)| (id, u.clone()))
            .collect();
        ui.add_space(8.0);
        theme::section(ui, &format!("Underlays · {}", underlays.len()));
        if underlays.is_empty() {
            ui.label(
                egui::RichText::new(
                    "A DWG, DXF, SVG, PDF or Illustrator drawing to trace over: snap to it, \
                     calibrate its scale, and draw on top.",
                )
                .weak(),
            );
        }
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        for (id, u) in underlays {
            let picked = self.editor.selected_underlay == Some(id);
            let mut edited = u.clone();
            let frame = egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(6, 4))
                .corner_radius(6.0)
                .stroke(if picked {
                    egui::Stroke::new(1.0, accent)
                } else {
                    egui::Stroke::NONE
                });
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let name = egui::RichText::new(&u.name);
                    let name = if picked {
                        name.family(theme::bold())
                    } else {
                        name
                    };
                    if ui
                        .add(
                            egui::Label::new(name)
                                .sense(egui::Sense::click())
                                .truncate(),
                        )
                        .on_hover_text(&u.source)
                        .clicked()
                    {
                        self.editor.selected_underlay = if picked { None } else { Some(id) };
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("×")
                                        .size(16.0)
                                        .color(ui.visuals().weak_text_color()),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Delete underlay")
                            .clicked()
                        {
                            self.editor.remove_underlay(id);
                        }
                        let lock_glyph = if edited.locked {
                            Glyph::Lock
                        } else {
                            Glyph::Unlock
                        };
                        crate::layers_ui::icon_toggle(
                            ui,
                            &mut edited.locked,
                            lock_glyph,
                            false,
                            "Locked: can't be moved, scaled or calibrated",
                        );
                        crate::layers_ui::icon_toggle(
                            ui,
                            &mut edited.snap,
                            Glyph::Snap,
                            true,
                            "Snap to its end points, midpoints, centres and curves",
                        );
                        crate::layers_ui::icon_toggle(
                            ui,
                            &mut edited.visible,
                            Glyph::Eye,
                            true,
                            "Show or hide",
                        );
                    });
                });
                if picked {
                    self.underlay_details(ui, id, &u, &mut edited);
                }
            });
            if edited != u {
                self.editor
                    .update_underlay(id, "Edit underlay", |x| *x = edited);
            }
        }
        ui.add_space(6.0);
        if ui
            .button("Import…")
            .on_hover_text("Place a DWG, DXF, SVG, PDF or Illustrator drawing under the sketch")
            .clicked()
        {
            self.pick_import_file(true);
        }
    }

    /// Placement, scale and actions for the picked underlay.
    fn underlay_details(
        &mut self,
        ui: &mut egui::Ui,
        id: UnderlayId,
        u: &drift_core::Underlay,
        edited: &mut drift_core::Underlay,
    ) {
        let units = self.editor.doc.units();
        let dark = ui.visuals().dark_mode;
        ui.label(egui::RichText::new(&u.source).small().weak());
        ui.add_space(2.0);
        // Opacity: live while dragging, one undo step.
        theme::row(ui, "Opacity", |ui| {
            let mut percent = u.opacity * 100.0;
            let r = ui.add(egui::Slider::new(&mut percent, 5.0..=100.0).suffix("%"));
            if r.changed() {
                let continuing = r.dragged() && !r.drag_started();
                self.editor
                    .adjust_underlay(id, continuing, |x| x.opacity = percent / 100.0);
                edited.opacity = percent / 100.0;
            }
        });
        theme::row(ui, "Colour", |ui| {
            if let Some(c) = crate::layers_ui::color_choice(ui, u.color, "Grey", dark) {
                edited.color = c;
            }
        });
        // Where its pictures stack: beneath the drawing, or on a layer among its fills (an
        // import's pictures go over the fills that came in with them).
        if !u.images.is_empty() {
            let layers: Vec<(drift_core::LayerId, String)> = self
                .editor
                .doc
                .layers()
                .map(|(id, l)| (id, l.name.clone()))
                .collect();
            let name = |l: Option<drift_core::LayerId>| match l {
                Some(l) => layers
                    .iter()
                    .find(|(id, _)| *id == l)
                    .map_or("Beneath the drawing".to_owned(), |(_, n)| format!("On {n}")),
                None => "Beneath the drawing".to_owned(),
            };
            theme::row(ui, "Pictures", |ui| {
                egui::ComboBox::from_id_salt(("underlay_pictures", id.0))
                    .selected_text(name(u.layer))
                    .show_ui(ui, |ui| {
                        for l in std::iter::once(None).chain(layers.iter().map(|(id, _)| Some(*id)))
                        {
                            if ui.selectable_label(u.layer == l, name(l)).clicked() {
                                edited.layer = l;
                            }
                        }
                    })
                    .response
                    .on_hover_text(
                        "Beneath the drawing, or on a layer: over its fills, under its lines",
                    );
            });
            if u.layer.is_some() {
                self.arrange_row(ui);
            }
        }
        ui.add_enabled_ui(!u.locked, |ui| {
            // 1 : n, as the import dialog has it: a PDF's curves are in points, the others' in
            // millimetres on their paper.
            let per = if is_pdf(&u.source) { MM_PER_POINT } else { 1.0 };
            let n = u.scale / per;
            let current = SCALES
                .iter()
                .find(|(_, s)| ((s - n) / s).abs() < 1e-4)
                .map_or_else(
                    || drift_core::sheets::scale_label(n),
                    |(l, _)| (*l).to_owned(),
                );
            theme::row(ui, "Scale", |ui| {
                egui::ComboBox::from_id_salt(("underlay_scale", id.0))
                    .selected_text(current)
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (label, s) in SCALES {
                            if ui
                                .selectable_label(((s - n) / s).abs() < 1e-4, *label)
                                .clicked()
                            {
                                edited.scale = s * per;
                            }
                        }
                    })
                    .response
                    .on_hover_text("The scale the drawing was drawn at (or calibrate it)");
                ui.label("1 :");
                let mut shown = n;
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .range(1e-6..=1e6)
                            .speed(0.5)
                            .max_decimals(3),
                    )
                    .changed()
                {
                    edited.scale = shown * per;
                }
            });
            let unit = units.length.mm_per_unit();
            theme::row(ui, "Position", |ui| {
                for (axis, value) in [("X", u.at.x), ("Y", u.at.y)] {
                    ui.label(egui::RichText::new(axis).weak());
                    let mut shown = value / unit;
                    if ui
                        .add(
                            egui::DragValue::new(&mut shown)
                                .speed(1.0)
                                .max_decimals(4)
                                .suffix(format!(" {}", units.length.symbol())),
                        )
                        .changed()
                    {
                        if axis == "X" {
                            edited.at.x = shown * unit;
                        } else {
                            edited.at.y = shown * unit;
                        }
                    }
                }
            });
            theme::row(ui, "Angle", |ui| {
                let mut a = u.angle;
                if ui
                    .add(egui::DragValue::new(&mut a).suffix("°").speed(0.5))
                    .changed()
                {
                    edited.angle = a.rem_euclid(360.0);
                }
                for q in [0.0, 90.0, 180.0, 270.0] {
                    if ui.small_button(format!("{q:.0}")).clicked() {
                        // Turn about the underlay's middle, so it stays in view.
                        let middle = u.bounds().center();
                        let mut turned = u.clone();
                        turned.angle = q;
                        turned.at += middle - turned.to_world(u.to_local(middle));
                        *edited = drift_core::Underlay {
                            angle: q,
                            at: turned.at,
                            ..edited.clone()
                        };
                    }
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!u.locked, |ui| {
                if ui
                    .button("Calibrate…")
                    .on_hover_text(
                        "Click two points on the underlay, then type the real distance between \
                         them",
                    )
                    .clicked()
                {
                    self.editor.selected_underlay = Some(id);
                    self.set_tool(ToolKind::Calibrate);
                }
                if ui
                    .button("Move…")
                    .on_hover_text("Click a point on the underlay, then where it goes")
                    .clicked()
                {
                    self.editor.selected_underlay = Some(id);
                    self.set_tool(ToolKind::MoveUnderlay);
                }
            });
            if ui.button("Zoom to").clicked() {
                let b = u.bounds();
                if !b.is_empty() {
                    self.fit_target = Some(b);
                }
            }
            if ui
                .button("Convert to geometry")
                .on_hover_text(
                    "Replace the underlay with lines, arcs and circles you can edit (on the \
                     current layer)",
                )
                .clicked()
            {
                let n = self.editor.convert_underlay(id);
                let now = ui.input(|i| i.time);
                self.set_status(
                    format!("Converted the underlay into {n} curves"),
                    false,
                    now,
                );
            }
        });
    }
}
