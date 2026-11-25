//! The Selection filter pie (Shift+Q unless changed): the seven kinds the Selection filter
//! picks, plus All, in a ring around the pointer (#481). It shows as soon as the key goes down.
//! Each kind has a fixed place, so it becomes a flick: hold the key, move towards Edges, let
//! go, and only edges are picked; flick to All for everything back. Let go in the middle (or
//! tap the key) and the ring stays open: each click turns that one kind on or off.
//! Clicks made with the key still held count the same, and letting go then just closes the
//! ring: it never flicks over them. While it is open, the letter on a kind turns it on or off
//! too. Esc, a right click or the key again closes it. The kinds the filter picks now are lit.
//!
//! The same flick-or-click model as the relations pie (Q, [`super::pie`]), and its geometry.

use super::pie_style;
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

/// While the ring is open, the key that turns each place on or off (shown on its pill); All's
/// picks everything.
pub(crate) const KEYS: [Key; 8] = [
    Key::E,
    Key::F,
    Key::A,
    Key::P,
    Key::R,
    Key::B,
    Key::T,
    Key::N,
];

/// The Selection filter pie, open.
pub(crate) struct SelectPie {
    pub center: egui::Pos2,
    /// Where the pointer was when it opened: a flick counts once it has moved from there.
    pub from: egui::Pos2,
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
        let (pressed, released, escape, pointer, now, typed) = ctx.input(|i| {
            let (pressed, released) =
                crate::keymap::pressed_released(i, "edit.selection_filter_pie");
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
        let Some(pie) = &mut self.select_pie else {
            if !pressed || ctx.egui_wants_keyboard_input() {
                return false;
            }
            if self.tool.is_busy() {
                self.pie_waits_for_tool(crate::commands::Command::SelectionFilterPie, now);
                return true;
            }
            // Down and up in one frame, a tap, leaves the ring open to click.
            self.open_select_pie(ctx, pointer, !released);
            return true;
        };
        if escape || (pressed && !pie.held) {
            self.select_pie = None;
        } else if let Some(place) = typed {
            // A kind's letter: as a click on it, and the ring stays.
            self.select_pie_toggle(place);
            let filter = self.drawing.editor.prefs.pick_filter;
            self.set_status(
                format!("Selection filter: {}", filter.summary()),
                false,
                now,
            );
        } else if released && pie.held {
            pie.held = false;
            if pie.clicked {
                // Kinds were clicked with the key down: they stand, and the ring is done.
                self.select_pie = None;
            } else if let Some(place) = pie_style::flick_place(&rects, pie.from, pointer) {
                // A flick: let go towards a kind to pick only it. Let go in the middle and
                // the ring stays, to click.
                self.select_pie_flick(place, now);
            }
        }
        true
    }

    /// Open the ring around `at` (the pointer; the canvas's middle without one), kept clear
    /// of the window's edges, in place of any other pie. `held`: its key is still down.
    pub(crate) fn open_select_pie(
        &mut self,
        ctx: &egui::Context,
        at: Option<egui::Pos2>,
        held: bool,
    ) {
        let fallback = self.drawing.view.canvas_rect.center();
        let center = pie_style::pie_center(ctx, at, fallback);
        self.close_pies();
        self.select_pie = Some(SelectPie {
            center,
            from: at.unwrap_or(center),
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
        let Some((center, flick_from)) = self
            .select_pie
            .as_ref()
            .map(|p| (p.center, (p.held && !p.clicked).then_some(p.from)))
        else {
            return;
        };
        let filter = self.drawing.editor.prefs.pick_filter;
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let screen = ctx.content_rect();
        let mut clicked = None;
        let mut shown = None;
        egui::Area::new(egui::Id::new("selection_filter_pie"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // The whole window is the pie's while it is open: a click on a pill turns it
                // on or off, a click anywhere else closes the pie.
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                let painter = ui.painter();
                let labels = PLACES.map(|k| pie_style::label_galley(painter, place_label(k)));
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
                let hot = pie_style::hot_place(&rects, pointer, flick_from);
                let flicks = flick_from.is_some();
                let aim =
                    flick_from.and_then(|from| pie_style::flick_aim(&rects, center, from, pointer));
                paint(
                    painter, center, filter, aim, &rects, labels, keys, hot, flicks,
                );
            });
        if let Some(rects) = shown {
            self.pie_pills = rects;
        }
        match clicked {
            Some(Some(place)) => self.select_pie_toggle(place),
            Some(None) => self.select_pie = None,
            None => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    painter: &egui::Painter,
    center: egui::Pos2,
    filter: PickFilter,
    aim: Option<egui::Vec2>,
    rects: &[egui::Rect; 8],
    labels: [std::sync::Arc<egui::Galley>; 8],
    keys: [std::sync::Arc<egui::Galley>; 8],
    hot: Option<usize>,
    flicks: bool,
) {
    let dark = painter.ctx().theme() == egui::Theme::Dark;
    pie_style::paint_hub(painter, center, aim, theme::accent(dark));
    for (place, (label, key)) in labels.into_iter().zip(keys).enumerate() {
        let kind = PLACES[place];
        let state = pie_style::PillState {
            on: lit(filter, kind),
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
            |p, icon, color| match kind {
                Some(kind) => crate::icons::pick_kind(p, icon, kind, color),
                None => crate::icons::tool(p, icon, ToolKind::Select, color),
            },
        );
    }
    // What choosing would do, under the ring; and the filter now, while nothing is pointed at.
    let text = match hot.map(|p| PLACES[p]) {
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
    let color = painter
        .ctx()
        .style_of(painter.ctx().theme())
        .visuals
        .text_color();
    pie_style::paint_caption(painter, center, rects, text, color);
}
