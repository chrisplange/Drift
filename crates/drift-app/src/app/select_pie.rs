//! The Selection filter pie (Shift+Q unless changed): the seven kinds the Selection filter
//! picks, plus All, in a ring around the pointer (#481). It shows as soon as the key goes down.
//! Each kind has a fixed place, so it becomes a flick: hold the key, move towards Edges, let
//! go, and only edges are picked; flick to All for everything back. Let go in the middle (or
//! tap the key) and the ring stays open: each click turns that one kind on or off.
//! Clicks made with the key still held count the same, and letting go then just closes the
//! ring: it never flicks over them. Esc, a right click or the key again closes it. The kinds
//! the filter picks now are lit.
//!
//! The same flick-or-click model as the relations pie (Q, [`super::pie`]), and its geometry.

use super::pie::{RADIUS, place_at, place_pos};
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

/// The Selection filter pie, open.
pub(crate) struct SelectPie {
    pub center: egui::Pos2,
    /// Its key is still down: letting go towards a kind picks only that kind.
    pub held: bool,
    /// A kind has been clicked since it opened: letting go of the key no longer flicks.
    pub clicked: bool,
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

/// The filter after a click on `kind`: that kind on or off, like the Selection filter's own
/// toggles (a flick picks one kind alone). All: everything.
fn toggled(filter: PickFilter, kind: Option<PickKind>) -> PickFilter {
    match kind {
        Some(kind) => filter.with(kind, !filter.has(kind)),
        None => PickFilter::ALL,
    }
}

impl DriftApp {
    /// The Selection filter pie's key: down opens the ring at once. While it is down or the
    /// ring is open the keys are its own. Returns true if the keyboard was used here.
    pub(crate) fn select_pie_keys(&mut self, ctx: &egui::Context) -> bool {
        // The pie's key, the user's (#499): Shift+Q unless changed.
        let (pressed, released, escape, pointer, now) = ctx.input(|i| {
            let (pressed, released) =
                crate::keymap::pressed_released(i, "edit.selection_filter_pie");
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
            // Down and up in one frame, a tap, leaves the ring open to click.
            self.open_select_pie(ctx, pointer, !released);
            return true;
        };
        if escape || (pressed && !pie.held) {
            self.select_pie = None;
        } else if released && pie.held {
            pie.held = false;
            if pie.clicked {
                // Kinds were clicked with the key down: they stand, and the ring is done.
                self.select_pie = None;
            } else if let Some(place) = pointer.and_then(|p| place_at(pie.center, p)) {
                // A flick: let go towards a kind to pick only it. Let go in the middle and
                // the ring stays, to click.
                self.select_pie_flick(place, now);
            }
        }
        true
    }

    /// Open the ring around `at` (the pointer; the canvas's middle without one), kept clear
    /// of the window's edges. `held`: its key is still down.
    pub(crate) fn open_select_pie(
        &mut self,
        ctx: &egui::Context,
        at: Option<egui::Pos2>,
        held: bool,
    ) {
        let at = at.unwrap_or(self.drawing.view.canvas_rect.center());
        // (Room for the labels around it.)
        let screen = ctx.content_rect().shrink2(egui::vec2(170.0, 130.0));
        self.select_pie = Some(SelectPie {
            center: if screen.is_positive() {
                screen.clamp(at)
            } else {
                at
            },
            held,
            clicked: false,
        });
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

    /// A click on `place` with the ring open: see [`toggled`]. The ring stays open, for more.
    pub(crate) fn select_pie_toggle(&mut self, place: usize) {
        if let Some(pie) = &mut self.select_pie {
            pie.clicked = true;
        }
        let filter = toggled(self.drawing.editor.prefs.pick_filter, PLACES[place]);
        self.set_pick_filter(filter);
    }

    /// Draw the Selection filter pie, once it shows, and take its clicks.
    pub(crate) fn selection_filter_pie(&mut self, ctx: &egui::Context) {
        let Some((center, flicks)) = self
            .select_pie
            .as_ref()
            .map(|p| (p.center, p.held && !p.clicked))
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
                paint(ui, center, filter, pointer, towards, flicks);
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
    pointer: Option<egui::Pos2>,
    towards: Option<usize>,
    flicks: bool,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let accent = theme::accent(dark);
    let n = theme::neutrals(dark);
    let radius = theme::radius::MENU;
    super::pie::paint_hub(painter, center, pointer, n, accent);
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
        Some(Some(kind)) if flicks => {
            format!("Pick only {}", kind.label().to_lowercase())
        }
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
