//! The relations pie (Q): a ring of relations around the pointer for the current selection.
//! Each relation has a fixed place, so it becomes a flick: hold Q, move towards one, let go.
//! A tap of Q leaves the ring open to click instead. A relation already on the selection is
//! lit, and choosing it takes it off; ones that don't fit the selection are dimmed. While it is
//! open, each relation's letter (shown on it) chooses it outright: Q then H for horizontal.

use super::pie_style;
use crate::annotations::{self, GlyphKind};
use crate::*;
use drift_edit::editor::ConstraintOption;

/// The relations that share each place, counter-clockwise from the right. No selection
/// offers two from the same place.
const PLACES: [&[&str]; 8] = [
    &["Horizontal"],
    &["Parallel", "Tangent"],
    &["Vertical"],
    &["Perpendicular", "Concentric"],
    &["Equal"],
    &["Fix"],
    &["Coincident", "Collinear", "On curve", "Coradial"],
    &["Midpoint", "Symmetric"],
];

/// While the ring is open, a key chooses a relation outright (Q then H: horizontal).
const KEYS: [(Key, &str); 14] = [
    (Key::H, "Horizontal"),
    (Key::V, "Vertical"),
    (Key::L, "Parallel"),
    (Key::P, "Perpendicular"),
    (Key::E, "Equal"),
    (Key::F, "Fix"),
    (Key::M, "Midpoint"),
    (Key::C, "Concentric"),
    (Key::R, "Coradial"),
    (Key::T, "Tangent"),
    (Key::S, "Symmetric"),
    (Key::I, "Coincident"),
    (Key::N, "Collinear"),
    (Key::O, "On curve"),
];

/// The key that chooses a relation while the ring is open.
fn key_for(label: &str) -> Option<Key> {
    KEYS.iter().find(|(_, l)| *l == label).map(|(k, _)| *k)
}

/// The symbol of a relation, by the name it is offered under.
pub(crate) fn glyph(label: &str) -> GlyphKind {
    match label {
        "Horizontal" => GlyphKind::Horizontal,
        "Vertical" => GlyphKind::Vertical,
        "Parallel" => GlyphKind::Parallel,
        "Perpendicular" => GlyphKind::Perpendicular,
        "Equal" => GlyphKind::Equal,
        "Fix" => GlyphKind::Fix,
        "Coincident" => GlyphKind::Coincident,
        "Midpoint" => GlyphKind::Midpoint,
        "Tangent" => GlyphKind::Tangent,
        "Concentric" => GlyphKind::Concentric,
        "Coradial" => GlyphKind::Coradial,
        "Collinear" => GlyphKind::Collinear,
        "Symmetric" => GlyphKind::Symmetric,
        "Corner" => GlyphKind::Corner,
        _ => GlyphKind::OnCurve,
    }
}

/// The open pie.
pub(crate) struct Pie {
    pub center: egui::Pos2,
    /// Where the pointer was when it opened: a flick counts once it has moved from there.
    pub from: egui::Pos2,
    /// Q is still down from opening it: letting go chooses what the pointer is towards.
    pub held: bool,
    /// Whether adding a place's relation would over-define the sketch, as of a revision
    /// (worked out when the pointer first points at it).
    conflict: Option<(usize, u64, bool)>,
}

/// One place on the ring, for the current selection.
#[derive(Clone, Debug)]
pub(crate) struct Slice {
    pub label: &'static str,
    /// The relation, if it fits the selection.
    pub option: Option<ConstraintOption>,
    /// Already on the selection.
    pub on: bool,
}

impl DriftApp {
    /// The ring's eight places for the selection.
    pub(crate) fn pie_slices(&self) -> [Slice; 8] {
        let options = self.drawing.editor.relation_options();
        PLACES.map(|labels| {
            let option = options.iter().find(|o| labels.contains(&o.label)).cloned();
            let on = option
                .as_ref()
                .is_some_and(|o| self.drawing.editor.has_relations(o));
            Slice {
                label: option.as_ref().map_or(labels[0], |o| o.label),
                option,
                on,
            }
        })
    }

    /// Open the pie to click (from a menu), where the pointer is, in place of any other pie.
    pub(crate) fn open_pie(&mut self, ctx: &egui::Context) {
        let at = ctx.input(|i| i.pointer.latest_pos());
        let fallback = self.drawing.view.canvas_rect.center();
        let center = pie_style::pie_center(ctx, at, fallback);
        self.close_pies();
        self.pie = Some(Pie {
            center,
            from: at.unwrap_or(center),
            held: false,
            conflict: None,
        });
    }

    /// Q (or the key the user gave it) opens the pie at the pointer; while it is open the keys are its own. Returns true if
    /// the keyboard was used here.
    pub(crate) fn pie_keys(&mut self, ctx: &egui::Context) -> bool {
        // Its key is the user's (#499): Q unless changed.
        let (pressed, released, escape, pointer) = ctx.input(|i| {
            let (pressed, released) = crate::keymap::pressed_released(i, "edit.relations_pie");
            (
                pressed,
                released,
                i.key_pressed(Key::Escape),
                i.pointer.latest_pos(),
            )
        });
        let now = ctx.input(|i| i.time);
        // A relation's key, while the ring is open (Q held or not).
        let typed = ctx.input(|i| {
            KEYS.iter()
                .find(|(k, _)| i.key_pressed(*k))
                .map(|(_, label)| *label)
        });
        if self.pie.is_some()
            && let Some(label) = typed
        {
            match self.pie_slices().iter().position(|s| s.label == label) {
                Some(place) => self.pie_choose(place, now),
                None => self.set_status(format!("{label} doesn't fit this selection"), true, now),
            }
            return true;
        }
        let Some(pie) = &mut self.pie else {
            if !pressed {
                return false;
            }
            if self.tool.is_busy() {
                self.pie_waits_for_tool(crate::commands::Command::RelationsPie, now);
                return true;
            }
            if self.drawing.editor.selection.is_empty()
                && self.drawing.editor.selected_midpoints.is_empty()
            {
                let key = crate::commands::spec(crate::commands::Command::RelationsPie)
                    .map(|s| s.key_text(cfg!(target_os = "macos")))
                    .unwrap_or_default();
                self.set_status(
                    format!("Select what to relate first, then press {key}"),
                    false,
                    now,
                );
                return true;
            }
            // (Kept clear of the window's edges, with room for the labels around it.) Down
            // and up in one frame, a tap, leaves the ring open to click.
            self.open_pie(ctx);
            if let Some(pie) = &mut self.pie {
                pie.held = !released;
            }
            return true;
        };
        if escape || (pressed && !pie.held) {
            self.pie = None;
        } else if released && pie.held {
            pie.held = false;
            // A flick: let go towards a relation to choose it. Let go in the middle and the
            // ring stays, to click.
            let rects = self.pie_pills;
            if let Some(place) = pie_style::flick_place(&rects, pie.center, pie.from, pointer) {
                self.pie_choose(place, now);
            }
        }
        true
    }

    /// Add the relation at `place` to the selection, or take it off if it is there.
    pub(crate) fn pie_choose(&mut self, place: usize, now: f64) {
        self.pie = None;
        let slice = self.pie_slices()[place].clone();
        let Some(option) = slice.option else {
            self.set_status(
                format!("{} doesn't fit this selection", slice.label),
                true,
                now,
            );
            return;
        };
        if slice.on {
            let existing = self.drawing.editor.existing_relations(&option);
            self.drawing.editor.remove_relations(&existing);
            return;
        }
        // Only what isn't there yet (some of several lines may already be horizontal).
        let missing = ConstraintOption {
            label: option.label,
            constraints: self.drawing.editor.missing_relations(&option),
        };
        self.apply_constraint_option(&missing, false, now);
    }

    /// Whether adding the relation at `place` would over-define the sketch.
    fn pie_conflict(&mut self, place: usize, slice: &Slice) -> bool {
        let revision = self.drawing.editor.doc.revision();
        if let Some(Pie {
            conflict: Some((p, r, bad)),
            ..
        }) = &self.pie
            && *p == place
            && *r == revision
        {
            return *bad;
        }
        let bad = match &slice.option {
            Some(option) if !slice.on => {
                let missing = self.drawing.editor.missing_relations(option);
                self.drawing.editor.would_conflict(&missing)
            }
            _ => false,
        };
        // (The dry run itself counts as a change to the document, and back.)
        let revision = self.drawing.editor.doc.revision();
        if let Some(pie) = &mut self.pie {
            pie.conflict = Some((place, revision, bad));
        }
        bad
    }

    /// Draw the open pie and take its clicks.
    pub(crate) fn relation_pie(&mut self, ctx: &egui::Context) {
        let Some((center, flick_from)) = self
            .pie
            .as_ref()
            .map(|p| (p.center, p.held.then_some(p.from)))
        else {
            return;
        };
        let slices = self.pie_slices();
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("relation_pie"));
        let fonts = ctx.layer_painter(layer);
        let labels = slices
            .clone()
            .map(|s| pie_style::label_galley(&fonts, s.label));
        let keys = slices
            .clone()
            .map(|s| key_for(s.label).map(|k| pie_style::key_galley(&fonts, k.name())));
        let rects = pie_style::pill_rects(
            center,
            std::array::from_fn(|p| (labels[p].size().x, keys[p].as_ref().map(|k| k.size().x))),
        );
        self.pie_pills = rects;
        let hot = pie_style::hot_place(&rects, center, pointer, flick_from);
        let aim = flick_from.and_then(|from| pie_style::flick_aim(center, from, pointer));
        let conflict = hot.is_some_and(|place| self.pie_conflict(place, &slices[place]));
        let screen = ctx.content_rect();
        let mut clicked = None;
        egui::Area::new(egui::Id::new("relation_pie"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // The whole window is the pie's while it is open: a click on the lit pill
                // chooses it (the one the caption and the conflict warning are for), a click
                // anywhere else closes the pie.
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                if response.clicked() {
                    clicked = Some(hot);
                } else if response.secondary_clicked() {
                    clicked = Some(None);
                }
                let painter = ui.painter();
                let dark = ui.visuals().dark_mode;
                let mark = if conflict {
                    theme::amber(dark)
                } else {
                    theme::accent(dark)
                };
                pie_style::paint_hub(painter, center, aim, mark);
                for (place, (label, key)) in labels.into_iter().zip(keys).enumerate() {
                    let slice = &slices[place];
                    let state = pie_style::PillState {
                        on: slice.on,
                        hot: hot == Some(place),
                        unavailable: slice.option.is_none(),
                        warn: conflict,
                    };
                    pie_style::paint_pill(
                        painter,
                        rects[place],
                        place,
                        label,
                        key,
                        state,
                        |p, r, c| {
                            annotations::paint_glyph_icon(p, r, glyph(slice.label), c);
                        },
                    );
                }
                // What choosing would do, under the ring; with nothing pointed at, the
                // relations the selection has.
                let n = theme::neutrals(dark);
                let (text, color) = if let Some(place) = hot {
                    let slice = &slices[place];
                    if slice.option.is_none() {
                        ("Doesn't fit this selection".to_owned(), n.secondary)
                    } else if slice.on {
                        (
                            format!("Remove {}", slice.label.to_lowercase()),
                            ui.visuals().text_color(),
                        )
                    } else if conflict {
                        (
                            "Would over-define the sketch".to_owned(),
                            theme::amber(dark),
                        )
                    } else {
                        (
                            format!("Add {}", slice.label.to_lowercase()),
                            ui.visuals().text_color(),
                        )
                    }
                } else {
                    (idle_caption(&slices), ui.visuals().text_color())
                };
                pie_style::paint_caption(painter, center, &rects, text, color);
            });
        if let Some(choice) = clicked {
            let now = ctx.input(|i| i.time);
            match choice {
                Some(place) => self.pie_choose(place, now),
                None => self.pie = None,
            }
        }
    }
}

/// The caption with nothing pointed at: the relations already on the selection.
fn idle_caption(slices: &[Slice; 8]) -> String {
    let on: Vec<&str> = slices.iter().filter(|s| s.on).map(|s| s.label).collect();
    if on.is_empty() {
        "Relations: none on the selection".to_owned()
    } else {
        format!("Relations: {}", on.join(", "))
    }
}
