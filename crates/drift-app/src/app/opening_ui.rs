//! The Door / window tool's bar (size set, family, type, width, the way it opens) and the
//! Inspector for picked openings.

use drift_core::{Hand, OpeningAt, OpeningId, Symbol, WallFace};

use crate::*;
use drift_edit::library::{hosted, sizes};
use drift_edit::openings::Flip;

/// The four ways an opening opens, as the bar shows them (Space steps through them).
const SWINGS: [((Hand, WallFace), &str); 4] = [
    ((Hand::Right, WallFace::Right), "Right, in"),
    ((Hand::Left, WallFace::Right), "Left, in"),
    ((Hand::Left, WallFace::Left), "Left, out"),
    ((Hand::Right, WallFace::Left), "Right, out"),
];

impl DriftApp {
    /// The drawing's size set picker: by its units, or a set it names (a drawing setting,
    /// one undo step).
    fn size_set_picker(&mut self, ui: &mut egui::Ui, salt: &str) {
        let rules = self.editor.doc.rules();
        let named = rules.size_set.map(|s| s.to_string());
        let shown = match &named {
            Some(id) => sizes::get(id).map_or(id.clone(), |s| s.name.clone()),
            None => format!("{} (by units)", self.editor.size_set().name),
        };
        let mut pick: Option<Option<&'static str>> = None;
        egui::ComboBox::from_id_salt(salt)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(named.is_none(), "By the drawing's units")
                    .clicked()
                {
                    pick = Some(None);
                }
                for s in sizes::all() {
                    if ui
                        .selectable_label(named.as_deref() == Some(s.id), &s.name)
                        .clicked()
                    {
                        pick = Some(Some(s.id));
                    }
                }
            })
            .response
            .on_hover_text("Which door and window sizes (and wall presets) this drawing uses");
        if let Some(p) = pick {
            let mut r = rules;
            r.size_set = sizes::id_for(p);
            if r != rules {
                self.editor.doc.begin();
                self.editor.doc.set_rules(r);
                self.editor.commit("Size set");
                // The bar's type follows the set.
                let family = self.editor.prefs.opening.family.clone();
                self.editor.set_opening_family(&family);
            }
        }
    }

    /// The Door / window tool's bar: `[US (imperial) ▾] · [Door – Single Swing ▾] [3′0″ ×
    /// 6′8″ ▾] · Width [ … ] · Right, in | Left, in | Left, out | Right, out`. What is set here
    /// is kept for the next drawing too.
    pub(crate) fn opening_tool_bar(&mut self, ui: &mut egui::Ui) {
        self.size_set_picker(ui, "opening_size_set");
        bar_divider(ui);
        let set = self.editor.size_set();
        let now = self.editor.opening_type_now();
        let family_name = hosted::family(now.family).map_or(now.family, |f| f.name);
        let mut family: Option<&'static str> = None;
        egui::ComboBox::from_id_salt("opening_family")
            .selected_text(family_name)
            .show_ui(ui, |ui| {
                for f in set.families() {
                    if ui.selectable_label(f.id == now.family, f.name).clicked() {
                        family = Some(f.id);
                    }
                }
            });
        if let Some(f) = family {
            self.editor.set_opening_family(f);
        }
        let now = self.editor.opening_type_now();
        let mut row: Option<sizes::SizeRow> = None;
        egui::ComboBox::from_id_salt("opening_type")
            .selected_text(if now.name.is_empty() {
                "Custom".to_owned()
            } else {
                now.name.clone()
            })
            .show_ui(ui, |ui| {
                for r in set.rows_of(now.family) {
                    if ui.selectable_label(r.name == now.name, &r.name).clicked() {
                        row = Some(r.clone());
                    }
                }
            })
            .response
            .on_hover_text("The type: named by its leaf; the wall is cut to its opening");
        if let Some(r) = row {
            self.editor.set_opening_type(&r);
        }
        bar_divider(ui);
        let units = self.editor.doc.units();
        let unit = units.length.mm_per_unit();
        ui.label(egui::RichText::new("Opening").size(12.0).weak());
        let now = self.editor.opening_type_now();
        let mut shown = now.opening.0 / unit;
        if ui
            .add(
                egui::DragValue::new(&mut shown)
                    .speed(0.05)
                    .range(0.001..=1e6)
                    .max_decimals(4)
                    .suffix(format!(" {}", units.length.symbol())),
            )
            .on_hover_text("The cut in the wall (a door's rough opening); any width will do")
            .changed()
        {
            let w = shown * unit;
            self.editor.prefs.opening.leaf = (w - (now.opening.0 - now.leaf.0)).clamp(0.0, w);
            self.editor.prefs.opening.width = w;
            self.editor.prefs.opening.height = now.opening.1;
            self.editor.prefs.opening.family = now.family.to_owned();
            self.editor.prefs.opening.type_name.clear();
        }
        bar_divider(ui);
        let swing = (
            self.editor.prefs.opening.hand,
            self.editor.prefs.opening.facing,
        );
        if let Some((h, f)) = app::context_ui::choice(ui, swing, &SWINGS) {
            self.editor.prefs.opening.hand = h;
            self.editor.prefs.opening.facing = f;
        }
        ui.label(egui::RichText::new("Space").size(12.0).weak());
    }

    /// The Inspector for picked openings. False when no opening is picked.
    pub(crate) fn opening_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let openings: Vec<OpeningId> = self.editor.selected_openings().into_iter().collect();
        let Some(&first) = openings.first() else {
            return false;
        };
        let Some(op) = self.editor.doc.opening(first).cloned() else {
            return false;
        };
        let units = self.editor.doc.units();
        let name = match (&op.symbol, openings.len()) {
            (Symbol::Library { family, type_name }, 1) => {
                let f = hosted::family(family).map_or(family.as_str(), |f| f.name);
                if type_name.is_empty() {
                    f.to_owned()
                } else {
                    format!("{f} – {type_name}")
                }
            }
            (Symbol::Void, 1) => "Opening".to_owned(),
            (_, n) => format!("{n} openings"),
        };
        app::panels::title(
            ui,
            Some(ToolKind::Opening),
            &name,
            &format!("In wall {}", op.wall),
        );
        theme::section(ui, "Opening");
        if openings.len() == 1 {
            let offset_now = self
                .editor
                .opening_offsets(first)
                .zip(self.editor.opening_span(first))
                .map(|((a, b, start), (u0, u1))| if start { u0 - a } else { b - u1 });
            let width_now = op
                .width_expr
                .clone()
                .unwrap_or_else(|| units.format_readout(op.width));
            for (field, label, current) in [
                (0u8, "Offset", offset_now.map(|v| units.format_readout(v))),
                (1, "Width", Some(width_now)),
            ] {
                let Some(current) = current else {
                    continue;
                };
                let mut text = match &self.opening_text {
                    Some((o, f, t)) if *o == first && *f == field => t.clone(),
                    _ => current.clone(),
                };
                let mut commit = None;
                theme::row(ui, label, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(ui.available_width() - 8.0),
                    );
                    if r.changed() {
                        self.opening_text = Some((first, field, text.clone()));
                    }
                    if r.lost_focus() && text != current {
                        commit = Some(text.clone());
                    }
                });
                if let Some(t) = commit {
                    self.opening_text = None;
                    let done = if field == 0 {
                        self.editor.set_opening_offset(first, &t)
                    } else {
                        self.editor.set_opening_width(first, &t)
                    };
                    if let Err(e) = done {
                        self.editor.out.notice = Some(e.to_string());
                    }
                }
            }
            if op.height > 0.0 {
                theme::row(ui, "Height", |ui| {
                    ui.label(units.format_readout(op.height));
                });
            }
            if op.leaf != op.width && matches!(op.symbol, Symbol::Library { .. }) {
                theme::row(ui, "Leaf", |ui| {
                    ui.label(units.format_readout(op.leaf));
                });
            }
        }
        let swing = (op.hand, op.facing);
        theme::row(ui, "Opens", |ui| {
            egui::ComboBox::from_id_salt("opening_swing")
                .selected_text(
                    SWINGS
                        .iter()
                        .find(|(s, _)| *s == swing)
                        .map_or("", |(_, l)| l),
                )
                .show_ui(ui, |ui| {
                    for ((h, f), label) in SWINGS {
                        if ui.selectable_label((h, f) == swing, label).clicked() {
                            self.editor.edit_openings(&openings, "Opening swing", |o| {
                                o.hand = h;
                                o.facing = f;
                            });
                        }
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Flip hinge")
                .on_hover_text("Hinged at the other jamb")
                .clicked()
            {
                self.editor.flip_openings(&openings, Flip::Hand);
            }
            if ui
                .button("Flip side")
                .on_hover_text("Opens to the wall's other face")
                .clicked()
            {
                self.editor.flip_openings(&openings, Flip::Facing);
            }
            let centred = openings.iter().all(|o| {
                matches!(
                    self.editor.doc.opening(*o).map(|x| &x.at),
                    Some(OpeningAt::Centred)
                )
            });
            if !centred
                && ui
                    .button("Centre")
                    .on_hover_text("In the middle of its wall segment")
                    .clicked()
            {
                self.editor.centre_openings(&openings);
            }
            if ui
                .button("Delete")
                .on_hover_text("The wall closes up")
                .clicked()
            {
                self.editor.delete_openings(&openings);
            }
        });
        theme::section(ui, "Size set");
        self.size_set_picker(ui, "inspector_size_set");
        true
    }
}

impl DriftApp {
    /// A picked opening's two flip arrows on the canvas: across the wall (the side it opens
    /// to) and along it (its hinge). A click on one flips it.
    pub(crate) fn paint_opening_arrows(
        &self,
        painter: &egui::Painter,
        map: &crate::overlay::ScreenMap,
        colors: &crate::overlay::OverlayColors,
    ) {
        let picked = self.editor.selected_openings();
        if picked.len() != 1 {
            return;
        }
        let hover = self
            .pointer
            .and_then(|p| self.editor.opening_arrow_at(p.raw, p.tolerance));
        for o in picked {
            for (at, dir, flip) in self.editor.opening_flip_arrows(o) {
                let c = map.to_screen(at);
                // Screen y is down.
                let d = egui::vec2(dir.x as f32, -dir.y as f32).normalized();
                let n = egui::vec2(-d.y, d.x);
                let lit = hover == Some((o, flip));
                let color = if lit {
                    colors.window
                } else {
                    colors.window.gamma_multiply(0.75)
                };
                let stroke = egui::Stroke::new(if lit { 2.0 } else { 1.5 }, color);
                let half = 9.0;
                let (a, b) = (c - d * half, c + d * half);
                painter.line_segment([a, b], stroke);
                for (tip, back) in [(b, -d), (a, d)] {
                    painter.line_segment([tip, tip + back * 4.5 + n * 4.0], stroke);
                    painter.line_segment([tip, tip + back * 4.5 - n * 4.0], stroke);
                }
            }
        }
    }
}

/// A thin vertical divider on the bar.
fn bar_divider(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::hairline(ui));
    ui.add_space(2.0);
}
