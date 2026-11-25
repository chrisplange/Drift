//! The relations pie (Q): a ring of relations around the pointer for the current selection.
//! Each relation has a fixed place, so it becomes a flick: hold Q, move towards one, let go.
//! A tap of Q leaves the ring open to click instead. A relation already on the selection is
//! lit, and choosing it takes it off; ones that don't fit the selection are dimmed. While it is
//! open, each relation's letter (shown on it) chooses it outright: Q then H for horizontal.

use crate::annotations::{self, AnnotationColors, GlyphKind};
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

/// How far from the middle the pointer has to be to choose, and where the labels sit.
pub(crate) const DEAD_ZONE: f32 = 22.0;
pub(crate) const RADIUS: f32 = 96.0;

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

/// The place the pointer is towards, if it is clear of the middle.
pub(crate) fn place_at(center: egui::Pos2, pointer: egui::Pos2) -> Option<usize> {
    let v = pointer - center;
    if v.length() < DEAD_ZONE {
        return None;
    }
    let turns = (-v.y).atan2(v.x) / std::f32::consts::TAU;
    Some((turns * 8.0).round().rem_euclid(8.0) as usize % 8)
}

/// A pie's middle: a ring, with a mark that rolls round it with the pointer (rather than
/// stepping between the eight places), once the pointer is far enough out to point anywhere.
pub(crate) fn paint_hub(
    painter: &egui::Painter,
    center: egui::Pos2,
    pointer: Option<egui::Pos2>,
    n: theme::Neutrals,
    mark: egui::Color32,
) {
    let r = DEAD_ZONE - 6.0;
    painter.circle_filled(center, r, n.raised);
    painter.circle_stroke(center, r, egui::Stroke::new(2.0, n.control_border));
    if let Some(v) = pointer.map(|p| p - center).filter(|v| v.length() > 4.0) {
        painter.circle_filled(center + v.normalized() * r, 3.5, mark);
    }
}

/// The middle of a place's label.
pub(crate) fn place_pos(center: egui::Pos2, place: usize) -> egui::Pos2 {
    let a = place as f32 * std::f32::consts::FRAC_PI_4;
    center + egui::vec2(a.cos(), -a.sin()) * RADIUS
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

    /// Open the pie to click (from a menu), where the pointer is.
    pub(crate) fn open_pie(&mut self, ctx: &egui::Context) {
        let at = ctx
            .input(|i| i.pointer.latest_pos())
            .unwrap_or(self.drawing.view.canvas_rect.center());
        let screen = ctx.content_rect().shrink2(egui::vec2(170.0, 130.0));
        self.pie = Some(Pie {
            center: if screen.is_positive() {
                screen.clamp(at)
            } else {
                at
            },
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
            if !pressed || self.tool.is_busy() {
                return false;
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
            // (Kept clear of the window's edges, with room for the labels around it.)
            self.open_pie(ctx);
            if let Some(pie) = &mut self.pie {
                pie.held = true;
            }
            return true;
        };
        if escape || (pressed && !pie.held) {
            self.pie = None;
        } else if released && pie.held {
            pie.held = false;
            // A flick: let go towards a relation to choose it. Let go in the middle and the
            // ring stays, to click.
            if let Some(place) = pointer.and_then(|p| place_at(pie.center, p)) {
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
        let Some(center) = self.pie.as_ref().map(|p| p.center) else {
            return;
        };
        let slices = self.pie_slices();
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let towards = pointer.and_then(|p| place_at(center, p));
        let conflict = towards.is_some_and(|place| self.pie_conflict(place, &slices[place]));
        let screen = ctx.content_rect();
        let mut clicked = None;
        egui::Area::new(egui::Id::new("relation_pie"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // The whole window is the pie's while it is open.
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                if response.clicked() {
                    clicked = Some(towards);
                } else if response.secondary_clicked() {
                    clicked = Some(None);
                }
                paint(ui, center, &slices, pointer, towards, conflict);
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

fn paint(
    ui: &egui::Ui,
    center: egui::Pos2,
    slices: &[Slice; 8],
    pointer: Option<egui::Pos2>,
    towards: Option<usize>,
    conflict: bool,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let accent = theme::accent(dark);
    let amber = crate::theme::amber(dark);
    let n = theme::neutrals(dark);
    // (The symbols on the pills' own fill, not on a tile of the canvas's.)
    let glyph_colors = AnnotationColors::new(dark);
    let radius = theme::radius::MENU;
    let mark = if conflict && towards.is_some() {
        amber
    } else {
        accent
    };
    paint_hub(painter, center, pointer, n, mark);
    for (place, slice) in slices.iter().enumerate() {
        let available = slice.option.is_some();
        let hot = towards == Some(place);
        let text_color = if !available {
            n.tertiary.gamma_multiply(0.75)
        } else if slice.on {
            visuals.strong_text_color()
        } else {
            n.text
        };
        let galley = painter.layout_no_wrap(
            slice.label.to_owned(),
            egui::FontId::proportional(theme::size::BODY),
            text_color,
        );
        let key = key_for(slice.label).map(|k| {
            painter.layout_no_wrap(
                k.name().to_owned(),
                egui::FontId::proportional(theme::size::CAPTION),
                n.tertiary,
            )
        });
        let size = egui::vec2(
            galley.size().x + 46.0 + key.as_ref().map_or(0.0, |k| k.size().x + 14.0),
            30.0,
        );
        // Labels to the sides start at the ring rather than straddling it.
        let at = place_pos(center, place);
        let lean = (at.x - center.x) / RADIUS;
        let rect = egui::Rect::from_center_size(at + egui::vec2(lean * size.x * 0.5, 0.0), size);
        // Opaque, like everything that floats: what is on is tinted with the accent over
        // the raised fill, what doesn't fit sinks back to the panel's.
        let fill = if !available {
            n.panel
        } else if hot {
            n.hover
        } else {
            n.raised
        };
        painter.rect_filled(rect, radius, fill);
        if slice.on {
            painter.rect_filled(rect, radius, theme::accent_soft(dark));
        }
        let stroke = if hot && conflict {
            egui::Stroke::new(1.5, amber)
        } else if hot && available || slice.on {
            egui::Stroke::new(1.5, accent)
        } else if available {
            egui::Stroke::new(1.0, n.control_border)
        } else {
            egui::Stroke::new(1.0, n.border)
        };
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 19.0, rect.center().y),
            egui::Vec2::splat(18.0),
        );
        annotations::paint_glyph_icon(
            painter,
            icon,
            glyph(slice.label),
            if !available {
                n.tertiary.gamma_multiply(0.75)
            } else if slice.on {
                accent
            } else {
                glyph_colors.normal
            },
        );
        painter.galley(
            egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
        // Its key, at the end.
        if let Some(key) = key {
            painter.galley(
                egui::pos2(
                    rect.right() - 14.0 - key.size().x,
                    rect.center().y - key.size().y / 2.0,
                ),
                key,
                n.tertiary,
            );
        }
    }
    // What choosing would do, under the ring.
    let note = towards.map(|place| {
        let slice = &slices[place];
        if slice.option.is_none() {
            ("Doesn't fit this selection".to_owned(), n.secondary)
        } else if slice.on {
            (
                format!("Remove {}", slice.label.to_lowercase()),
                visuals.text_color(),
            )
        } else if conflict {
            ("Would over-define the sketch".to_owned(), amber)
        } else {
            (
                format!("Add {}", slice.label.to_lowercase()),
                visuals.text_color(),
            )
        }
    });
    if let Some((text, color)) = note {
        let galley =
            painter.layout_no_wrap(text, egui::FontId::proportional(theme::size::LABEL), color);
        let at = egui::pos2(center.x - galley.size().x / 2.0, center.y + RADIUS + 26.0);
        let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(9.0, 5.0));
        painter.rect(
            back,
            theme::radius::CONTROL,
            n.raised,
            egui::Stroke::new(1.0, n.control_border),
            egui::StrokeKind::Outside,
        );
        painter.galley(at, galley, color);
    }
}
