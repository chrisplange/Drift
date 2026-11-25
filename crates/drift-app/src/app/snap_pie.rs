//! The snap pie (Shift+S unless changed): the kinds of snap target in a ring around the
//! pointer, each turned on and off like the switches of a snap panel. It shows as soon as the
//! key goes down. Hold the key, move towards a kind and let go to turn that one kind on or off;
//! let go in the middle (or tap the key) and the ring stays open: each click turns one kind on
//! or off. Clicks made with the key still held count the same, and letting go then just closes
//! the ring. Esc, a right click or the key again closes it. The kinds that are on are lit.
//!
//! Grid is the same switch as grid snapping, and Perpendicular and Tangent need the
//! perpendicular/tangent switch: turning one of them on here turns that on too.
//!
//! The same flick-or-click model as the Selection filter pie ([`super::select_pie`]), and the
//! relations pie's geometry.

use super::pie_style::{self, place_at};
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

/// The snap pie, open.
pub(crate) struct SnapPie {
    pub center: egui::Pos2,
    /// Its key is still down: letting go towards a kind turns it on or off.
    pub held: bool,
    /// A kind has been clicked since it opened: letting go of the key no longer flicks.
    pub clicked: bool,
}

/// The switches a place stands for, as the app holds them.
#[derive(Clone, Copy)]
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
                if on && !self.perp_tangent {
                    // The shared switch comes on for this one alone.
                    let other = if kind == SnapKind::Perpendicular {
                        SnapKind::Tangent
                    } else {
                        SnapKind::Perpendicular
                    };
                    self.perp_tangent = true;
                    self.kinds = self.kinds.with(other, false);
                }
                self.kinds = self.kinds.with(kind, on);
            }
            _ => {
                for k in kinds {
                    self.kinds = self.kinds.with(*k, on);
                }
            }
        }
        self
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

    /// The snap pie's key: down opens the ring at once. While it is down or the ring is open
    /// the keys are its own. Returns true if the keyboard was used here.
    pub(crate) fn snap_pie_keys(&mut self, ctx: &egui::Context) -> bool {
        let (pressed, released, escape, pointer, now) = ctx.input(|i| {
            let (pressed, released) = crate::keymap::pressed_released(i, "edit.snap_pie");
            (
                pressed,
                released,
                i.key_pressed(Key::Escape),
                i.pointer.latest_pos(),
                i.time,
            )
        });
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
        } else if released && pie.held {
            pie.held = false;
            if pie.clicked {
                self.snap_pie = None;
            } else if let Some(place) = pointer.and_then(|p| place_at(pie.center, p)) {
                // A flick: let go towards a kind to turn it on or off, and the ring closes.
                self.snap_pie = None;
                self.snap_pie_toggle(place);
                let state = if self.snap_switches().lit(place) {
                    "on"
                } else {
                    "off"
                };
                self.set_status(format!("{} snapping {state}", PLACES[place].0), false, now);
            }
        }
        true
    }

    /// Open the ring around `at` (the pointer; the canvas's middle without one), kept clear
    /// of the window's edges. `held`: its key is still down.
    pub(crate) fn open_snap_pie(
        &mut self,
        ctx: &egui::Context,
        at: Option<egui::Pos2>,
        held: bool,
    ) {
        let fallback = self.drawing.view.canvas_rect.center();
        self.snap_pie = Some(SnapPie {
            center: pie_style::pie_center(ctx, at, fallback),
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
        let Some((center, flicks)) = self
            .snap_pie
            .as_ref()
            .map(|p| (p.center, p.held && !p.clicked))
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
                let rects = pie_style::pill_rects(
                    center,
                    std::array::from_fn(|p| (labels[p].size().x, None)),
                );
                shown = Some(rects);
                if response.clicked() {
                    clicked = Some(pie_style::pill_under(&rects, pointer));
                } else if response.secondary_clicked() {
                    clicked = Some(None);
                }
                let hot = pie_style::hot_place(&rects, center, pointer, flicks);
                paint(painter, center, switches, pointer, &rects, labels, hot);
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

fn paint(
    painter: &egui::Painter,
    center: egui::Pos2,
    switches: Switches,
    pointer: Option<egui::Pos2>,
    rects: &[egui::Rect; 8],
    labels: [std::sync::Arc<egui::Galley>; 8],
    hot: Option<usize>,
) {
    let dark = painter.ctx().theme() == egui::Theme::Dark;
    pie_style::paint_hub(painter, center, pointer, theme::accent(dark));
    for (place, label) in labels.into_iter().enumerate() {
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
            None,
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
    let text = match hot {
        Some(place) if switches.lit(place) => format!("Stop snapping to {}", hint(place)),
        Some(place) => format!("Snap to {}", hint(place)),
        None => "Snapping: click kinds on and off".to_owned(),
    };
    let color = painter
        .ctx()
        .style_of(painter.ctx().theme())
        .visuals
        .text_color();
    pie_style::paint_caption(painter, center, rects, text, color);
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
