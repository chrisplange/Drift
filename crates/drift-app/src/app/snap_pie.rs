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

use super::pie::{RADIUS, place_at, place_pos};
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
        let at = at.unwrap_or(self.drawing.view.canvas_rect.center());
        let screen = ctx.content_rect().shrink2(egui::vec2(190.0, 130.0));
        self.snap_pie = Some(SnapPie {
            center: if screen.is_positive() {
                screen.clamp(at)
            } else {
                at
            },
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
        let Some(center) = self.snap_pie.as_ref().map(|p| p.center) else {
            return;
        };
        let switches = self.snap_switches();
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let towards = pointer.and_then(|p| place_at(center, p));
        let screen = ctx.content_rect();
        let mut clicked = None;
        egui::Area::new(egui::Id::new("snap_pie"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                if response.clicked() {
                    clicked = Some(towards);
                } else if response.secondary_clicked() {
                    clicked = Some(None);
                }
                paint(ui, center, switches, pointer, towards);
            });
        match clicked {
            Some(Some(place)) => self.snap_pie_toggle(place),
            Some(None) => self.snap_pie = None,
            None => {}
        }
    }
}

fn paint(
    ui: &egui::Ui,
    center: egui::Pos2,
    switches: Switches,
    pointer: Option<egui::Pos2>,
    towards: Option<usize>,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let accent = theme::accent(dark);
    let n = theme::neutrals(dark);
    let radius = theme::radius::MENU;
    super::pie::paint_hub(painter, center, pointer, n, accent);
    for (place, (label, kinds)) in PLACES.into_iter().enumerate() {
        let on = switches.lit(place);
        let hot = towards == Some(place);
        let text_color = if on {
            visuals.strong_text_color()
        } else {
            n.text
        };
        let galley = painter.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(theme::size::BODY),
            text_color,
        );
        let size = egui::vec2(galley.size().x + 50.0, 30.0);
        let at = place_pos(center, place);
        let lean = (at.x - center.x) / RADIUS;
        let rect = egui::Rect::from_center_size(at + egui::vec2(lean * size.x * 0.5, 0.0), size);
        painter.rect_filled(rect, radius, if hot { n.hover } else { n.raised });
        if on {
            painter.rect_filled(rect, radius, theme::accent_soft(dark));
        }
        let stroke = if hot {
            egui::Stroke::new(2.5, accent)
        } else if on {
            egui::Stroke::new(1.5, accent)
        } else {
            egui::Stroke::new(1.0, n.control_border)
        };
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        let icon_color = if on { accent } else { n.text };
        let mut icon_at = egui::pos2(rect.left() + 19.0, rect.center().y);
        if matches!(kinds[0], SnapKind::Perpendicular | SnapKind::Tangent) {
            // (Those glyphs stand on their point: centre them.)
            icon_at.y += 4.0;
        }
        crate::overlay::snap_glyph(painter, icon_at, kinds[0], icon_color);
        painter.galley(
            egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
    }
    let text = match towards {
        Some(place) if switches.lit(place) => format!("Stop snapping to {}", hint(place)),
        Some(place) => format!("Snap to {}", hint(place)),
        None => "Snapping: click kinds on and off".to_owned(),
    };
    let color = visuals.text_color();
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
