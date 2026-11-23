//! The Selection filter pie (hold S): the seven kinds the Selection filter picks, plus All, in
//! a ring around the pointer (#481). Each has a fixed place, so it becomes a flick: hold S,
//! move towards Edges, let go, and only edges are picked; flick to All for everything back.
//! A quick tap of S still just takes the Select tool. Held still until the ring shows, then
//! let go in the middle, and the ring stays open: clicks there turn kinds on and off, several
//! at once. Esc, a right click or S closes it. The kinds the filter picks now are lit.
//!
//! The same tap/hold model as the relations pie (Q, [`super::pie`]), and its geometry.

use super::pie::{DEAD_ZONE, RADIUS, place_at, place_pos};
use crate::*;
use drift_edit::editor::{PickFilter, PickKind};

/// The kind at each place, counter-clockwise from the right; `None` is All, at the top.
pub(crate) const PLACES: [Option<PickKind>; 8] = [
    Some(PickKind::Edges),
    Some(PickKind::Fills),
    None,
    Some(PickKind::Points),
    Some(PickKind::Areas),
    Some(PickKind::Blocks),
    Some(PickKind::Text),
    Some(PickKind::Annotations),
];

/// How long S has to be held, still, before the ring shows (a shorter press is a tap).
const HOLD: f64 = 0.3;

/// The Selection filter pie, from S going down.
pub(crate) struct SelectPie {
    pub center: egui::Pos2,
    /// S is still down: letting go chooses what the pointer is towards.
    pub held: bool,
    /// The ring is showing (S has been held long enough, or the pointer moved off).
    pub shown: bool,
    pressed_at: f64,
}

/// Its name on the ring.
fn place_label(kind: Option<PickKind>) -> &'static str {
    kind.map_or("All", PickKind::short_label)
}

/// Whether a place is lit for the filter.
fn lit(filter: PickFilter, kind: Option<PickKind>) -> bool {
    match kind {
        Some(kind) => filter.has(kind),
        None => filter.is_all(),
    }
}

impl DriftApp {
    /// S: a tap takes the Select tool, a hold opens the Selection filter pie. While it is down
    /// or the ring is open the keys are its own. Returns true if the keyboard was used here.
    pub(crate) fn select_pie_keys(&mut self, ctx: &egui::Context) -> bool {
        // The Select tool's key, the user's (#499): S unless changed.
        let (pressed, released, escape, pointer, now) = ctx.input(|i| {
            let (pressed, released) = crate::keymap::pressed_released(i, "edit.select");
            (
                pressed,
                released,
                i.key_pressed(Key::Escape),
                i.pointer.latest_pos(),
                i.time,
            )
        });
        let Some(pie) = &mut self.select_pie else {
            if !pressed || self.tool.is_busy() || ctx.egui_wants_keyboard_input() {
                return false;
            }
            let at = pointer.unwrap_or(self.drawing.view.canvas_rect.center());
            // (Kept clear of the window's edges, with room for the labels around it.)
            let screen = ctx.content_rect().shrink2(egui::vec2(170.0, 130.0));
            self.select_pie = Some(SelectPie {
                center: if screen.is_positive() {
                    screen.clamp(at)
                } else {
                    at
                },
                held: true,
                shown: false,
                pressed_at: now,
            });
            if released {
                // Down and up in one frame: a tap.
                self.select_pie = None;
                self.tool_key(ctx, ToolKind::Select);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOLD));
            }
            return true;
        };
        if pie.held && !pie.shown {
            let moved = pointer.is_some_and(|p| (p - pie.center).length() >= DEAD_ZONE);
            if moved || now - pie.pressed_at >= HOLD {
                pie.shown = true;
            } else if !released {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    (HOLD - (now - pie.pressed_at)).max(0.01),
                ));
            }
        }
        if escape || (pressed && !pie.held) {
            self.select_pie = None;
        } else if released && pie.held {
            if !pie.shown {
                // Let go before the ring showed: a tap.
                self.select_pie = None;
                self.tool_key(ctx, ToolKind::Select);
                return true;
            }
            pie.held = false;
            // A flick: let go towards a kind to pick only it. Let go in the middle and the
            // ring stays, to click.
            if let Some(place) = pointer.and_then(|p| place_at(pie.center, p)) {
                self.select_pie_flick(place, now);
            }
        }
        true
    }

    /// Let go towards `place`: pick only that kind, or everything for All.
    pub(crate) fn select_pie_flick(&mut self, place: usize, now: f64) {
        self.select_pie = None;
        let filter = PLACES[place].map_or(PickFilter::ALL, PickFilter::only);
        self.set_pick_filter(filter);
        self.set_status(
            format!("Selection filter: {}", filter.summary()),
            false,
            now,
        );
    }

    /// A click on `place` with the ring open: turn that kind on or off (All: everything on).
    /// The ring stays open, for more.
    pub(crate) fn select_pie_toggle(&mut self, place: usize) {
        let filter = self.drawing.editor.prefs.pick_filter;
        let filter = match PLACES[place] {
            Some(kind) => filter.with(kind, !filter.has(kind)),
            None => PickFilter::ALL,
        };
        self.set_pick_filter(filter);
    }

    /// Draw the Selection filter pie, once it shows, and take its clicks.
    pub(crate) fn selection_filter_pie(&mut self, ctx: &egui::Context) {
        let Some((center, held)) = self
            .select_pie
            .as_ref()
            .filter(|p| p.shown)
            .map(|p| (p.center, p.held))
        else {
            return;
        };
        let filter = self.drawing.editor.prefs.pick_filter;
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let towards = pointer.and_then(|p| place_at(center, p));
        let screen = ctx.content_rect();
        let mut clicked = None;
        egui::Area::new(egui::Id::new("selection_filter_pie"))
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
                paint(ui, center, filter, towards, held);
            });
        match clicked {
            Some(Some(place)) => self.select_pie_toggle(place),
            Some(None) => self.select_pie = None,
            None => {}
        }
    }
}

fn paint(
    ui: &egui::Ui,
    center: egui::Pos2,
    filter: PickFilter,
    towards: Option<usize>,
    held: bool,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let accent = theme::accent(dark);
    let n = theme::neutrals(dark);
    let radius = theme::radius::MENU;
    // The middle: a ring, with a mark towards what would be chosen.
    painter.circle_filled(center, DEAD_ZONE - 6.0, n.raised);
    painter.circle_stroke(
        center,
        DEAD_ZONE - 6.0,
        egui::Stroke::new(2.0, n.control_border),
    );
    if let Some(place) = towards {
        let dir = (place_pos(center, place) - center).normalized();
        painter.circle_filled(center + dir * (DEAD_ZONE - 6.0), 3.5, accent);
    }
    for (place, kind) in PLACES.into_iter().enumerate() {
        let on = lit(filter, kind);
        let hot = towards == Some(place);
        let text_color = if on {
            visuals.strong_text_color()
        } else {
            n.text
        };
        let galley = painter.layout_no_wrap(
            place_label(kind).to_owned(),
            egui::FontId::proportional(theme::size::BODY),
            text_color,
        );
        let size = egui::vec2(galley.size().x + 50.0, 30.0);
        // Labels to the sides start at the ring rather than straddling it.
        let at = place_pos(center, place);
        let lean = (at.x - center.x) / RADIUS;
        let rect = egui::Rect::from_center_size(at + egui::vec2(lean * size.x * 0.5, 0.0), size);
        // Opaque, like everything that floats: what the filter picks is tinted with the accent.
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
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 19.0, rect.center().y),
            egui::Vec2::splat(18.0),
        );
        let icon_color = if on { accent } else { n.text };
        match kind {
            Some(kind) => crate::icons::pick_kind(painter, icon, kind, icon_color),
            None => crate::icons::tool(painter, icon, ToolKind::Select, icon_color),
        }
        painter.galley(
            egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
    }
    // What choosing would do, under the ring; and the filter now, while nothing is pointed at.
    let text = match towards.map(|p| PLACES[p]) {
        Some(None) => "Pick everything".to_owned(),
        Some(Some(kind)) if held => format!("Pick only {}", kind.label().to_lowercase()),
        Some(Some(kind)) if filter.has(kind) => {
            format!("Stop picking {}", kind.label().to_lowercase())
        }
        Some(Some(kind)) => format!("Pick {} too", kind.label().to_lowercase()),
        None => format!("Selection filter: {}", filter.summary()),
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
