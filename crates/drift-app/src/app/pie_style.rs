//! How every pie looks and is laid out: the relations pie (Q), the Selection filter pie
//! (Shift+Q) and the snap pie (Shift+S) all draw through here, so they read as one thing.
//!
//! - **Layout.** Eight pills on a circle round a hub: each pill's point nearest the middle
//!   (its inner edge, or inner corner for a diagonal one) is on the circle, so the ring reads
//!   round whatever the labels' lengths. The circle is the smallest that keeps every two pills
//!   at least [`GAP`] apart, so a pie of short labels is tighter than one of long ones.
//! - **Pill.** Opaque raised fill; on: tinted with the accent; pointed at: the hover fill.
//!   One thin stroke for all of them ([`STROKE`]): accent when on or pointed at (amber for a
//!   warning), the control border otherwise. The symbol sits at the pill's outer end, so the
//!   labels face the hub: on the left and at the top and bottom, symbol, label, then an
//!   optional key; on the right the same mirrored. Symbols are centred in a square at the
//!   pill's mid-height.
//! - **Hub.** A ring with a mark that rolls round it with the pointer.
//! - **Caption.** What choosing would do, in a box under the ring.
//! - **Choosing.** A flick (let go of the key) goes by direction, as in every pie; a click
//!   counts only on a pill, and a click anywhere else closes the pie.

use crate::*;
use std::sync::Arc;

/// How far from the middle the pointer has to be for a flick to choose.
pub(crate) const DEAD_ZONE: f32 = 22.0;
/// A pill's height.
pub(crate) const PILL_H: f32 = 30.0;
/// The least gap between any two pills.
const GAP: f32 = 14.0;
/// The smallest circle the pills sit on, and the largest it grows to.
const MIN_RADIUS: f32 = 56.0;
const MAX_RADIUS: f32 = 320.0;
/// Every pill's stroke width.
const STROKE: f32 = 1.0;
/// Inside a pill: where the icon's middle is, where the label starts, the end padding.
const ICON_X: f32 = 18.0;
const LABEL_X: f32 = 34.0;
const END_PAD: f32 = 13.0;

/// The room a pie needs round its middle, for keeping it clear of the window's edges.
const ROOM: egui::Vec2 = egui::vec2(300.0, 200.0);

/// Where a pie opened at `at` (the pointer; `fallback` without one) goes: clear of the
/// window's edges where there is room.
pub(crate) fn pie_center(
    ctx: &egui::Context,
    at: Option<egui::Pos2>,
    fallback: egui::Pos2,
) -> egui::Pos2 {
    let at = at.unwrap_or(fallback);
    let screen = ctx.content_rect().shrink2(ROOM);
    if screen.is_positive() {
        screen.clamp(at)
    } else {
        at
    }
}

/// The place the pointer is towards, if it is clear of the middle: what a flick chooses.
pub(crate) fn place_at(center: egui::Pos2, pointer: egui::Pos2) -> Option<usize> {
    let v = pointer - center;
    if v.length() < DEAD_ZONE {
        return None;
    }
    let turns = (-v.y).atan2(v.x) / std::f32::consts::TAU;
    Some((turns * 8.0).round().rem_euclid(8.0) as usize % 8)
}

/// Which way a place is from the middle, by sign: (1, 0) right, (0, -1) up, and so on
/// (places count counter-clockwise from the right).
fn signs(place: usize) -> egui::Vec2 {
    const S: [(f32, f32); 8] = [
        (1.0, 0.0),
        (1.0, -1.0),
        (0.0, -1.0),
        (-1.0, -1.0),
        (-1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ];
    let (x, y) = S[place % 8];
    egui::vec2(x, y)
}

/// A pill's size for a label of width `label_w` and an optional key of width `key_w`.
fn pill_size(label_w: f32, key_w: Option<f32>) -> egui::Vec2 {
    egui::vec2(
        LABEL_X + label_w + END_PAD + key_w.map_or(0.0, |w| w + 12.0),
        PILL_H,
    )
}

/// The pill of `size` at `place` on a circle of `radius`: its point nearest the middle on
/// the circle, the pill growing away from the middle from there.
fn pill_on(center: egui::Pos2, place: usize, size: egui::Vec2, radius: f32) -> egui::Rect {
    let a = place as f32 * std::f32::consts::FRAC_PI_4;
    let anchor = center + egui::vec2(a.cos(), -a.sin()) * radius;
    let s = signs(place);
    let mid = anchor + egui::vec2(s.x * size.x / 2.0, s.y * size.y / 2.0);
    egui::Rect::from_min_size(
        egui::pos2(
            (mid.x - size.x / 2.0).round(),
            (mid.y - size.y / 2.0).round(),
        ),
        size,
    )
}

/// The gap between two rectangles (0 if they touch or overlap).
fn gap(a: egui::Rect, b: egui::Rect) -> f32 {
    let dx = (a.min.x - b.max.x).max(b.min.x - a.max.x).max(0.0);
    let dy = (a.min.y - b.max.y).max(b.min.y - a.max.y).max(0.0);
    dx.hypot(dy)
}

/// A pie's eight pills, for labels of the widths given (and optional keys): on the
/// smallest circle that keeps every two of them [`GAP`] apart.
pub(crate) fn pill_rects(center: egui::Pos2, widths: [(f32, Option<f32>); 8]) -> [egui::Rect; 8] {
    let sizes = widths.map(|(label, key)| pill_size(label, key));
    let mut radius = MIN_RADIUS;
    loop {
        let rects: [egui::Rect; 8] =
            std::array::from_fn(|place| pill_on(center, place, sizes[place], radius));
        let clear = (0..8).all(|i| (i + 1..8).all(|j| gap(rects[i], rects[j]) >= GAP));
        if clear || radius >= MAX_RADIUS {
            return rects;
        }
        radius += 1.0;
    }
}

/// The pill under the pointer.
pub(crate) fn pill_under(rects: &[egui::Rect; 8], pointer: Option<egui::Pos2>) -> Option<usize> {
    let p = pointer?;
    rects.iter().position(|r| r.contains(p))
}

/// The pill that would be chosen: while the key is held (a flick), the one the pointer is
/// towards; otherwise the one under it.
pub(crate) fn hot_place(
    rects: &[egui::Rect; 8],
    center: egui::Pos2,
    pointer: Option<egui::Pos2>,
    flicks: bool,
) -> Option<usize> {
    pill_under(rects, pointer).or_else(|| {
        flicks
            .then(|| pointer.and_then(|p| place_at(center, p)))
            .flatten()
    })
}

/// The label's text, laid out as a pill shows it.
pub(crate) fn label_galley(painter: &egui::Painter, text: &str) -> Arc<egui::Galley> {
    painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        egui::Color32::PLACEHOLDER,
    )
}

/// A key's name, laid out as a pill shows it at its end.
pub(crate) fn key_galley(painter: &egui::Painter, text: &str) -> Arc<egui::Galley> {
    painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(theme::size::CAPTION),
        egui::Color32::PLACEHOLDER,
    )
}

/// A pill's state.
#[derive(Clone, Copy, Default)]
pub(crate) struct PillState {
    /// What it stands for is on (lit).
    pub on: bool,
    /// It is what would be chosen.
    pub hot: bool,
    /// It can't be chosen now (dimmed).
    pub unavailable: bool,
    /// Choosing it would be a problem (amber when hot).
    pub warn: bool,
}

/// Paint one pill in `rect`; `icon` draws its symbol in the square it is given, in the
/// colour given.
pub(crate) fn paint_pill(
    painter: &egui::Painter,
    rect: egui::Rect,
    place: usize,
    label: Arc<egui::Galley>,
    key: Option<Arc<egui::Galley>>,
    state: PillState,
    icon: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) {
    let dark = painter.ctx().theme() == egui::Theme::Dark;
    let visuals = painter
        .ctx()
        .style_of(painter.ctx().theme())
        .visuals
        .clone();
    let accent = theme::accent(dark);
    let n = theme::neutrals(dark);
    let radius = theme::radius::MENU;
    let fill = if state.unavailable {
        n.panel
    } else if state.hot {
        n.hover
    } else {
        n.raised
    };
    painter.rect_filled(rect, radius, fill);
    if state.on {
        painter.rect_filled(rect, radius, theme::accent_soft(dark));
    }
    let stroke_color = if state.hot && state.warn {
        theme::amber(dark)
    } else if state.on || (state.hot && !state.unavailable) {
        accent
    } else if state.unavailable {
        n.border
    } else {
        n.control_border
    };
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(STROKE, stroke_color),
        egui::StrokeKind::Inside,
    );
    let dim = n.tertiary.gamma_multiply(0.75);
    let (text_color, icon_color) = if state.unavailable {
        (dim, dim)
    } else if state.on {
        (visuals.strong_text_color(), accent)
    } else {
        (n.text, n.text)
    };
    // The symbol at the outer end: on the right side of the ring, the row is mirrored.
    let mirrored = signs(place).x > 0.0;
    let y = rect.center().y;
    let (icon_x, label_x, key_x) = if mirrored {
        (
            rect.right() - ICON_X,
            rect.right() - LABEL_X - label.size().x,
            key.as_ref().map(|_| rect.left() + END_PAD),
        )
    } else {
        (
            rect.left() + ICON_X,
            rect.left() + LABEL_X,
            key.as_ref().map(|k| rect.right() - END_PAD - k.size().x),
        )
    };
    icon(
        painter,
        egui::Rect::from_center_size(egui::pos2(icon_x, y), egui::Vec2::splat(18.0)),
        icon_color,
    );
    painter.galley(
        egui::pos2(label_x, y - label.size().y / 2.0),
        label,
        text_color,
    );
    if let (Some(key), Some(x)) = (key, key_x) {
        painter.galley(egui::pos2(x, y - key.size().y / 2.0), key, n.tertiary);
    }
}

/// A pie's middle: a ring, with a mark that rolls round it with the pointer (rather than
/// stepping between the eight places), once the pointer is far enough out to point anywhere.
pub(crate) fn paint_hub(
    painter: &egui::Painter,
    center: egui::Pos2,
    pointer: Option<egui::Pos2>,
    mark: egui::Color32,
) {
    let n = theme::neutrals(painter.ctx().theme() == egui::Theme::Dark);
    let r = DEAD_ZONE - 6.0;
    painter.circle_filled(center, r, n.raised);
    painter.circle_stroke(center, r, egui::Stroke::new(2.0, n.control_border));
    if let Some(v) = pointer.map(|p| p - center).filter(|v| v.length() > 4.0) {
        painter.circle_filled(center + v.normalized() * r, 3.5, mark);
    }
}

/// What choosing would do (or what the pie is for), in a box under the ring.
pub(crate) fn paint_caption(
    painter: &egui::Painter,
    center: egui::Pos2,
    rects: &[egui::Rect; 8],
    text: String,
    color: egui::Color32,
) {
    let n = theme::neutrals(painter.ctx().theme() == egui::Theme::Dark);
    let galley =
        painter.layout_no_wrap(text, egui::FontId::proportional(theme::size::LABEL), color);
    let bottom = rects.iter().map(|r| r.bottom()).fold(center.y, f32::max);
    let at = egui::pos2(center.x - galley.size().x / 2.0, bottom + 16.0);
    let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(9.0, 5.0));
    painter.rect(
        back,
        theme::radius::CONTROL,
        n.raised,
        egui::Stroke::new(STROKE, n.control_border),
        egui::StrokeKind::Outside,
    );
    painter.galley(at, galley, color);
}

impl DriftApp {
    /// One pie at a time: a pie's key pressed while another pie is open closes that one, so
    /// the pie asked for opens in its place instead of on top of it.
    pub(crate) fn switch_pies(&mut self, ctx: &egui::Context) {
        let pressed = |id: &str| ctx.input(|i| crate::keymap::pressed_released(i, id).0);
        if pressed("edit.relations_pie") && self.pie.is_none() {
            self.select_pie = None;
            self.snap_pie = None;
        }
        if pressed("edit.selection_filter_pie") && self.select_pie.is_none() {
            self.pie = None;
            self.snap_pie = None;
        }
        if pressed("edit.snap_pie") && self.snap_pie.is_none() {
            self.pie = None;
            self.select_pie = None;
        }
    }
}
