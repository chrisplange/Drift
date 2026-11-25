//! The snap pie (Shift+S unless changed): the kinds of snap target in a ring around the
//! pointer, each turned on and off like the switches of a snap panel. It shows as soon as the
//! key goes down. Hold the key, move towards a kind and let go to turn that one kind on or off;
//! let go in the middle (or tap the key) and the ring stays open: each click turns one kind on
//! or off. Clicks made with the key still held count the same, and letting go then just closes
//! the ring. While it is open, the letter on a kind turns it on or off too. Esc, a right click
//! or the key again closes it. The kinds that are on are lit.
//!
//! Grid is the same switch as grid snapping. Perpendicular and Tangent are the two halves of
//! the perpendicular & tangent switch (on the status bar): turning one on here while that is
//! off turns it on for that one alone, turning both off turns it off, and turning it on there
//! brings back both.
//!
//! The same flick-or-click model as the Selection filter pie ([`super::select_pie`]), and the
//! relations pie's geometry.

use super::pie_style;
use crate::*;
use drift_geom::{SnapKind, SnapKinds};

/// The kinds each place turns on and off, counter-clockwise from the right, with its name.
/// Points covers ends, standalone points and the origin.
pub(crate) const PLACES: [(&str, &[SnapKind]); 8] = [
    (
        "Points",
        &[SnapKind::Endpoint, SnapKind::Point, SnapKind::Origin],
    ),
    ("Midpoint", &[SnapKind::Midpoint]),
    ("Grid", &[SnapKind::Grid]),
    ("Center", &[SnapKind::Center]),
    ("Quadrant", &[SnapKind::Quadrant]),
    ("On curve", &[SnapKind::OnCurve]),
    ("Perpendicular", &[SnapKind::Perpendicular]),
    ("Tangent", &[SnapKind::Tangent]),
];

/// While the ring is open, the key that turns each place on or off (shown on its pill).
pub(crate) const KEYS: [Key; 8] = [
    Key::E,
    Key::M,
    Key::G,
    Key::C,
    Key::U,
    Key::O,
    Key::P,
    Key::T,
];

/// The snap pie, open.
pub(crate) struct SnapPie {
    pub center: egui::Pos2,
    /// Where the pointer was when it opened: a flick counts once it has moved from there.
    pub from: egui::Pos2,
    /// Its key is still down: letting go towards a kind turns it on or off.
    pub held: bool,
    /// A kind has been clicked since it opened: letting go of the key no longer flicks.
    pub clicked: bool,
}

/// The switches a place stands for, as the app holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Switches {
    kinds: SnapKinds,
    grid: bool,
    perp_tangent: bool,
}

impl Switches {
    /// Whether the place's snapping is on.
    fn lit(self, place: usize) -> bool {
        let kinds = PLACES[place].1;
        match kinds[0] {
            SnapKind::Grid => self.grid,
            SnapKind::Perpendicular | SnapKind::Tangent => {
                self.perp_tangent && self.kinds.has(kinds[0])
            }
            _ => kinds.iter().all(|k| self.kinds.has(*k)),
        }
    }

    /// After turning a place on or off.
    fn toggled(mut self, place: usize) -> Self {
        let on = !self.lit(place);
        let kinds = PLACES[place].1;
        match kinds[0] {
            SnapKind::Grid => self.grid = on,
            kind @ (SnapKind::Perpendicular | SnapKind::Tangent) => {
                let other = if kind == SnapKind::Perpendicular {
                    SnapKind::Tangent
                } else {
                    SnapKind::Perpendicular
                };
                if on && !self.perp_tangent {
                    // The shared switch comes on for this one alone: the other was off, and
                    // stays off.
                    self.perp_tangent = true;
                    self.kinds = self.kinds.with(other, false);
                }
                self.kinds = self.kinds.with(kind, on);
                if !on && !self.kinds.has(other) {
                    // Both off: the shared switch goes off, and coming on again brings both.
                    self.perp_tangent = false;
                    self.kinds = self.kinds.with(kind, true).with(other, true);
                }
            }
            _ => {
                for k in kinds {
                    self.kinds = self.kinds.with(*k, on);
                }
            }
        }
        self
    }

    /// What snaps, in a few words: "everything", "all but Grid", "Points, Midpoint".
    fn summary(self) -> String {
        let (on, off): (Vec<usize>, Vec<usize>) = (0..8).partition(|p| self.lit(*p));
        let names = |places: Vec<usize>| {
            places
                .into_iter()
                .map(|p| PLACES[p].0)
                .collect::<Vec<_>>()
                .join(", ")
        };
        match on.len() {
            8 => "everything".to_owned(),
            0 => "nothing".to_owned(),
            _ if off.len() <= 3 => format!("all but {}", names(off)),
            _ => names(on),
        }
    }
}

impl DriftApp {
    fn snap_switches(&self) -> Switches {
        Switches {
            kinds: self.drawing.editor.prefs.snap_kinds,
            grid: self.grid_snap,
            perp_tangent: self.perp_tangent_snap,
        }
    }

    fn set_snap_switches(&mut self, s: Switches) {
        self.drawing.editor.prefs.snap_kinds = s.kinds;
        self.grid_snap = s.grid;
        self.perp_tangent_snap = s.perp_tangent;
    }

    /// The perpendicular & tangent switch (the status bar, the palette, the settings): turned on, it
    /// snaps both, whichever of the two the snap pie had left off.
    pub(crate) fn set_perp_tangent_snap(&mut self, on: bool) {
        self.perp_tangent_snap = on;
        if on {
            let kinds = &mut self.drawing.editor.prefs.snap_kinds;
            *kinds = kinds
                .with(SnapKind::Perpendicular, true)
                .with(SnapKind::Tangent, true);
        }
    }

    /// The snap pie's key: down opens the ring at once. While it is down or the ring is open
    /// the keys are its own. Returns true if the keyboard was used here.
    pub(crate) fn snap_pie_keys(&mut self, ctx: &egui::Context) -> bool {
        let (pressed, released, escape, pointer, now, typed) = ctx.input(|i| {
            let (pressed, released) = crate::keymap::pressed_released(i, "edit.snap_pie");
            (
                pressed,
                released,
                i.key_pressed(Key::Escape),
                i.pointer.latest_pos(),
                i.time,
                KEYS.iter().position(|k| i.key_pressed(*k)),
            )
        });
        let rects = self.pie_pills;
        let Some(pie) = &mut self.snap_pie else {
            if !pressed || ctx.egui_wants_keyboard_input() {
                return false;
            }
            // Down and up in one frame, a tap, leaves the ring open to click.
            self.open_snap_pie(ctx, pointer, !released);
            return true;
        };
        if escape || (pressed && !pie.held) {
            self.snap_pie = None;
        } else if let Some(place) = typed {
            // A kind's letter: on or off, and the ring stays, as for a click.
            self.snap_pie_toggle(place);
            self.snap_status(place, now);
        } else if released && pie.held {
            pie.held = false;
            if pie.clicked {
                self.snap_pie = None;
            } else if let Some(place) =
                pie_style::flick_place(&rects, pie.center, pie.from, pointer)
            {
                // A flick: let go towards a kind to turn it on or off, and the ring closes.
                self.snap_pie = None;
                self.snap_pie_toggle(place);
                self.snap_status(place, now);
            }
        }
        true
    }

    /// Say whether `place` now snaps.
    fn snap_status(&mut self, place: usize, now: f64) {
        let state = if self.snap_switches().lit(place) {
            "on"
        } else {
            "off"
        };
        self.set_status(format!("{} snapping {state}", PLACES[place].0), false, now);
    }

    /// Open the ring around `at` (the pointer; the canvas's middle without one), kept clear
    /// of the window's edges, in place of any other pie. `held`: its key is still down.
    pub(crate) fn open_snap_pie(
        &mut self,
        ctx: &egui::Context,
        at: Option<egui::Pos2>,
        held: bool,
    ) {
        let fallback = self.drawing.view.canvas_rect.center();
        let center = pie_style::pie_center(ctx, at, fallback);
        self.close_pies();
        self.snap_pie = Some(SnapPie {
            center,
            from: at.unwrap_or(center),
            held,
            clicked: false,
        });
    }

    /// Turn `place` on or off. The ring, if open, stays.
    pub(crate) fn snap_pie_toggle(&mut self, place: usize) {
        if let Some(pie) = &mut self.snap_pie {
            pie.clicked = true;
        }
        let s = self.snap_switches().toggled(place);
        self.set_snap_switches(s);
    }

    /// Draw the snap pie, once it shows, and take its clicks.
    pub(crate) fn snap_pie(&mut self, ctx: &egui::Context) {
        let Some((center, flick_from)) = self
            .snap_pie
            .as_ref()
            .map(|p| (p.center, (p.held && !p.clicked).then_some(p.from)))
        else {
            return;
        };
        let switches = self.snap_switches();
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let screen = ctx.content_rect();
        let mut clicked = None;
        let mut shown = None;
        egui::Area::new(egui::Id::new("snap_pie"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // A click on a pill turns it on or off; anywhere else closes the pie.
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                let painter = ui.painter();
                let labels = PLACES.map(|(l, _)| pie_style::label_galley(painter, l));
                let keys = KEYS.map(|k| pie_style::key_galley(painter, k.name()));
                let rects = pie_style::pill_rects(
                    center,
                    std::array::from_fn(|p| (labels[p].size().x, Some(keys[p].size().x))),
                );
                shown = Some(rects);
                if response.clicked() {
                    clicked = Some(pie_style::pill_under(&rects, pointer));
                } else if response.secondary_clicked() {
                    clicked = Some(None);
                }
                let hot = pie_style::hot_place(&rects, center, pointer, flick_from);
                let aim = pointer.filter(|_| flick_from.is_some() && hot.is_some());
                paint(painter, center, switches, aim, &rects, labels, keys, hot);
            });
        if let Some(rects) = shown {
            self.pie_pills = rects;
        }
        match clicked {
            Some(Some(place)) => self.snap_pie_toggle(place),
            Some(None) => self.snap_pie = None,
            None => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    painter: &egui::Painter,
    center: egui::Pos2,
    switches: Switches,
    aim: Option<egui::Pos2>,
    rects: &[egui::Rect; 8],
    labels: [std::sync::Arc<egui::Galley>; 8],
    keys: [std::sync::Arc<egui::Galley>; 8],
    hot: Option<usize>,
) {
    let dark = painter.ctx().theme() == egui::Theme::Dark;
    pie_style::paint_hub(painter, center, aim, theme::accent(dark));
    for (place, (label, key)) in labels.into_iter().zip(keys).enumerate() {
        let kind = PLACES[place].1[0];
        let state = pie_style::PillState {
            on: switches.lit(place),
            hot: hot == Some(place),
            ..Default::default()
        };
        pie_style::paint_pill(
            painter,
            rects[place],
            place,
            label,
            Some(key),
            state,
            |p, icon, color| {
                // (Those two glyphs stand on their point: a ⊥ rises above it, a tangent's
                // circle hangs below it. Centre them in the icon's square.)
                let at = icon.center()
                    + match kind {
                        SnapKind::Perpendicular => egui::vec2(0.0, 5.0),
                        SnapKind::Tangent => egui::vec2(0.0, -5.0),
                        _ => egui::Vec2::ZERO,
                    };
                crate::overlay::snap_glyph(p, at, kind, color);
            },
        );
    }
    let text = caption(switches, hot);
    let color = painter
        .ctx()
        .style_of(painter.ctx().theme())
        .visuals
        .text_color();
    pie_style::paint_caption(painter, center, rects, text, color);
}

/// What choosing the hot place would do; with nothing pointed at, what snaps now.
fn caption(switches: Switches, hot: Option<usize>) -> String {
    match hot {
        Some(place) if switches.lit(place) => format!("Stop snapping to {}", hint(place)),
        Some(place) => format!("Snap to {}", hint(place)),
        None => format!("Snapping: {}", switches.summary()),
    }
}

/// A place's kinds in a sentence.
fn hint(place: usize) -> &'static str {
    match place {
        0 => "ends, points and the origin",
        1 => "midpoints",
        2 => "the grid",
        3 => "centers",
        4 => "quadrants",
        5 => "the nearest point on curves",
        6 => "perpendicular points",
        _ => "tangent points",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERP: usize = 6;
    const TANGENT: usize = 7;

    fn defaults() -> Switches {
        Switches {
            kinds: SnapKinds::ALL,
            grid: true,
            perp_tangent: true,
        }
    }

    #[test]
    fn points_covers_ends_points_and_the_origin() {
        let s = defaults().toggled(0);
        assert!(!s.lit(0));
        for k in [SnapKind::Endpoint, SnapKind::Point, SnapKind::Origin] {
            assert!(!s.kinds.has(k), "{k:?}");
        }
        assert!(s.kinds.has(SnapKind::Midpoint));
        assert_eq!(s.toggled(0), defaults());
    }

    #[test]
    fn grid_is_the_grid_snap_switch() {
        let s = defaults().toggled(2);
        assert!(!s.grid && !s.lit(2));
        assert_eq!(s.kinds, SnapKinds::ALL, "no kind bit changes");
    }

    #[test]
    fn perpendicular_and_tangent_both_off_turn_the_shared_switch_off() {
        let s = defaults().toggled(PERP);
        assert!(s.perp_tangent && !s.lit(PERP) && s.lit(TANGENT));
        let s = s.toggled(TANGENT);
        assert!(
            !s.perp_tangent,
            "nothing of it is left on, so the guides go too"
        );
        assert!(!s.lit(PERP) && !s.lit(TANGENT));
        // The shared switch turned on again (the bar) brings back both.
        let mut back = s;
        back.perp_tangent = true;
        assert!(back.lit(PERP) && back.lit(TANGENT));
    }

    #[test]
    fn one_turned_on_with_the_shared_switch_off_comes_on_alone() {
        let off = Switches {
            perp_tangent: false,
            ..defaults()
        };
        let s = off.toggled(TANGENT);
        assert!(s.perp_tangent && s.lit(TANGENT) && !s.lit(PERP));
        // And turning it off again leaves the shared switch off, with both ready for it.
        assert_eq!(s.toggled(TANGENT), off);
    }

    #[test]
    fn the_summary_names_what_is_off_or_on() {
        assert_eq!(defaults().summary(), "everything");
        assert_eq!(defaults().toggled(1).summary(), "all but Midpoint");
        let mut none = defaults();
        for place in 0..8 {
            none = none.toggled(place);
        }
        assert_eq!(none.summary(), "nothing");
        assert_eq!(none.toggled(1).toggled(2).summary(), "Midpoint, Grid");
    }

    #[test]
    fn the_keys_are_distinct_and_leave_the_pie_keys_alone() {
        for (i, k) in KEYS.iter().enumerate() {
            assert!(!KEYS[i + 1..].contains(k), "{k:?} twice");
            assert!(![Key::Q, Key::S].contains(k), "{k:?} opens a pie");
        }
    }
}
