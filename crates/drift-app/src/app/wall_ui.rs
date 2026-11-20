//! The Wall tool's bar (thickness with presets, justification, straight or arc, poché) and
//! the Inspector for picked walls.

use drift_core::{Justify, WallId};

use crate::*;

/// The justifications, as the bar and Inspector show them.
const JUSTIFY: [(Justify, &str); 3] = [
    (Justify::Left, "Left face"),
    (Justify::Centre, "Centre"),
    (Justify::Right, "Right face"),
];

impl DriftApp {
    /// The Wall tool's bar: `Thickness [ … ▾ ] · Left face | Centre | Right face · Straight |
    /// Arc · ☐ Fill walls [Solid ▾ ■]`. What is set here is kept for the next drawing too.
    pub(crate) fn wall_tool_bar(&mut self, ui: &mut egui::Ui) {
        let units = self.drawing.editor.doc.units();
        theme::bar_caption(ui, "Thickness");
        let unit = units.length.mm_per_unit();
        let mut shown = self.drawing.editor.wall_thickness_now() / unit;
        if ui
            .add(
                egui::DragValue::new(&mut shown)
                    .speed(0.05)
                    .range(0.001..=1e6)
                    .max_decimals(4)
                    .suffix(format!(" {}", units.length.symbol())),
            )
            .on_hover_text("How thick new walls are (by the drawing's units until set)")
            .changed()
        {
            self.drawing.editor.prefs.wall_thickness = shown * unit;
            self.drawing.editor.prefs.wall_thickness_expr = None;
        }
        let presets = self.drawing.editor.wall_presets();
        // (A painted chevron: the font has no glyph for one.)
        theme::menu_chevron(ui, "Wall thickness presets", |ui| {
            for p in presets {
                let text = format!("{}  ({})", p.label, units.format_readout(p.mm));
                if ui.button(text).clicked() {
                    self.drawing.editor.prefs.wall_thickness = p.mm;
                    self.drawing.editor.prefs.wall_thickness_expr = None;
                    ui.close();
                }
            }
        })
        .on_hover_text("Common walls, at their actual finished thickness");
        theme::bar_divider(ui);
        if let Some(j) =
            app::context_ui::choice(ui, self.drawing.editor.prefs.wall_justify, &JUSTIFY)
        {
            self.drawing.editor.prefs.wall_justify = j;
        }
        theme::bar_caption(ui, "Space flips");
        theme::bar_divider(ui);
        if let Some(arc) = app::context_ui::choice(
            ui,
            self.drawing.editor.prefs.wall_arc,
            &[(false, "Straight"), (true, "Arc")],
        ) {
            self.drawing.editor.prefs.wall_arc = arc;
            self.tool_modal_key_at(
                if arc {
                    ToolKey::ArcNext
                } else {
                    ToolKey::LineNext
                },
                None,
            );
        }
        theme::bar_caption(ui, "W");
        theme::bar_divider(ui);
        ui.checkbox(&mut self.drawing.editor.prefs.wall_poche.on, "Fill walls")
            .on_hover_text("Fill new walls: joined walls of one style on one layer are one area");
        if self.drawing.editor.prefs.wall_poche.on {
            let mut style = self.drawing.editor.prefs.wall_poche.style.clone();
            self.style_bar(ui, &mut style, "wall_bar");
            self.drawing.editor.prefs.wall_poche.style = style;
        }
    }

    /// The Inspector for picked walls. False when no wall is picked.
    pub(crate) fn wall_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let walls: Vec<WallId> = self.drawing.editor.selected_walls().into_iter().collect();
        let Some(first) = walls
            .first()
            .and_then(|w| self.drawing.editor.doc.wall(*w))
            .cloned()
        else {
            return false;
        };
        let doc = &self.drawing.editor.doc;
        let units = doc.units();
        let layer = doc
            .layer(first.layer)
            .map_or(String::new(), |l| format!("On layer {}", l.name));
        let name = match walls.len() {
            1 => format!("Wall {}", walls[0]),
            n => format!("{n} walls"),
        };
        app::panels::title(ui, Some(ToolKind::Wall), &name, &layer);
        self.inspector_layer_row(ui);
        let doc = &self.drawing.editor.doc;
        theme::section(ui, "Wall");
        // Thickness: a value or an expression of variables.
        let same = walls
            .iter()
            .filter_map(|w| doc.wall(*w))
            .all(|w| w.thickness == first.thickness && w.thickness_expr == first.thickness_expr);
        let current = if same {
            first
                .thickness_expr
                .clone()
                .unwrap_or_else(|| units.format_readout(first.thickness))
        } else {
            String::new()
        };
        let mut text = match &self.drawing.wall_thickness_text {
            Some((for_walls, t)) if *for_walls == walls => t.clone(),
            _ => current.clone(),
        };
        let mut commit = None;
        theme::row(ui, "Thickness", |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(
                        ui.available_width() - theme::ROW_HEIGHT - ui.spacing().item_spacing.x,
                    )
                    .hint_text("various"),
            );
            if r.changed() {
                self.drawing.wall_thickness_text = Some((walls.clone(), text.clone()));
            }
            if r.lost_focus() && text != current {
                commit = Some(text.clone());
            }
            let presets = self.drawing.editor.wall_presets();
            theme::menu_chevron(ui, "Thickness presets", |ui| {
                for p in presets {
                    let label = format!("{}  ({})", p.label, units.format_readout(p.mm));
                    if ui.button(label).clicked() {
                        commit = Some(units.format_readout(p.mm));
                        ui.close();
                    }
                }
            })
            .on_hover_text("Wall thicknesses to pick from");
        });
        if let Some(t) = commit {
            self.drawing.wall_thickness_text = None;
            if let Err(e) = self.drawing.editor.set_wall_thickness(&walls, &t) {
                self.drawing.editor.out.notice = Some(e.to_string());
            }
        }
        theme::wide_row(ui, "Justify", 180.0, |ui| {
            if let Some(j) = theme::segmented(ui, first.justify, &JUSTIFY) {
                self.drawing.editor.set_wall_justify(&walls, j);
            }
        });
        // Its fill: on or off, and its style.
        let mut on = first.poche.is_some();
        let mut style = first
            .poche
            .clone()
            .unwrap_or_else(|| self.drawing.editor.prefs.wall_poche.style.clone());
        let was = (on, style.clone());
        theme::wide_row(ui, "Wall fill", 160.0, |ui| {
            ui.checkbox(&mut on, "");
            if on {
                self.style_kind(ui, &mut style);
            }
        });
        if on {
            self.style_row(ui, &mut style, "wall_inspector");
        }
        if (on, style.clone()) != was {
            self.drawing
                .editor
                .set_wall_poche(&walls, on.then_some(style.clone()));
        }
        theme::section(ui, "Joins");
        let mut cross = first.cross;
        if ui
            .checkbox(&mut cross, "Joins walls it crosses")
            .on_hover_text("Crossing walls of the same fill and layer clean up as an X")
            .changed()
        {
            self.drawing
                .editor
                .edit_walls(&walls, "Wall joins", |w| w.cross = cross);
        }
        let closed = self
            .drawing
            .editor
            .doc
            .path(first.centre)
            .is_some_and(|p| p.closed);
        if walls.len() == 1 && !closed {
            for (which, label) in [(0, "Joins at its start"), (1, "Joins at its end")] {
                let mut join = first.ends[which].join;
                if ui
                    .checkbox(&mut join, label)
                    .on_hover_text("Off: this end stays capped and overlaps what it meets")
                    .changed()
                {
                    self.drawing
                        .editor
                        .edit_walls(&walls, "Wall joins", |w| w.ends[which].join = join);
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if walls.len() == 2
                && ui
                    .button("Join")
                    .on_hover_text("Join these two walls where they come nearest: an L, or a T")
                    .clicked()
            {
                self.drawing.editor.join_walls(&walls);
            }
            if ui
                .button("Unjoin")
                .on_hover_text("Stop joining where they meet: the ends are capped and overlap")
                .clicked()
            {
                self.drawing.editor.unjoin_walls(&walls);
            }
        });
        if walls.len() == 1 {
            let w = walls[0];
            let segs = self
                .drawing
                .editor
                .doc
                .wall(w)
                .and_then(|wall| {
                    drift_geom::walls::regen::centreline(&self.drawing.editor.doc, wall).ok()
                })
                .map(|(s, ..)| s)
                .unwrap_or_default();
            theme::section(ui, "Segments");
            let mut split = None;
            for (k, s) in segs.iter().enumerate() {
                theme::row(ui, &format!("{}", k + 1), |ui| {
                    let kind = match s {
                        drift_geom::walls::Seg::Line { .. } => "",
                        drift_geom::walls::Seg::Arc { .. } => " (arc)",
                    };
                    ui.label(format!("{}{kind}", units.format_readout(s.length())));
                    if (k + 1 < segs.len() || closed)
                        && ui
                            .small_button("Split after")
                            .on_hover_text("Two walls from here on")
                            .clicked()
                    {
                        split = Some(k);
                    }
                });
            }
            if let Some(k) = split {
                self.drawing.editor.split_wall(w, k);
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Show thickness")
                    .on_hover_text("A dimension of its thickness: type into it to change it")
                    .clicked()
                {
                    self.drawing.editor.show_wall_thickness(w);
                }
                if ui
                    .button("Explode")
                    .on_hover_text("Plain lines, arcs and a fill: no longer a wall")
                    .clicked()
                {
                    self.drawing.editor.explode_walls(&[w]);
                }
            });
        } else {
            ui.add_space(6.0);
            if ui.button("Explode").clicked() {
                self.drawing.editor.explode_walls(&walls);
            }
        }
        true
    }
}
