//! Colour swatches and the colour picker they open, as in creative apps: a swatch is the
//! button, one click opens the picker, and a pick is one click on a swatch. A colour left
//! unset (`None`) is shown as what it falls back to: the Ink swatch (white on the dark
//! canvas, black on paper and in print) or a named one ("By layer", "Match stroke").

use drift_core::Rgb;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, vec2};

/// What an unset colour is, shown as its own swatch first in the picker.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Unset<'a> {
    /// The theme's ink: white on the dark canvas, black on paper and in print.
    Ink,
    /// Something it takes its colour from (by layer, the stroke's, a default), named.
    Named(&'a str),
}

impl Unset<'_> {
    fn name(&self) -> &str {
        match self {
            Unset::Ink => "Ink",
            Unset::Named(n) => n,
        }
    }

    /// What it is, said for the theme in use (`dark`).
    pub(crate) fn tip(&self, dark: bool) -> String {
        match self {
            Unset::Ink => ink_tip(dark).into(),
            Unset::Named(n) => format!("{n}: no color of its own"),
        }
    }

    /// The old labels for an unset colour, as the pickers were called with.
    pub(crate) fn from_label(label: &str) -> Unset<'_> {
        match label {
            "Auto" | "Ink" => Unset::Ink,
            "Same as stroke" => Unset::Named("Match stroke"),
            other => Unset::Named(other),
        }
    }
}

/// What an unset colour follows and so shows as: another colour, or the theme's ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Follows {
    Colour(Rgb),
    Ink,
}

impl Follows {
    /// What a colour that may itself be unset (Ink) shows as.
    pub(crate) fn of(c: Option<Rgb>) -> Follows {
        c.map_or(Follows::Ink, Follows::Colour)
    }

    /// As it shows now, in the theme in use (`dark`).
    fn shown(self, dark: bool) -> Color32 {
        match self {
            Follows::Colour(c) => to_color32(c),
            Follows::Ink => ink(dark),
        }
    }
}

/// A few colours always offered, after the drawing's own and the recent ones.
const PALETTE: [[u8; 3]; 12] = [
    [0xe5, 0x48, 0x4d],
    [0xf7, 0x6b, 0x15],
    [0xff, 0xc5, 0x3d],
    [0x46, 0xa7, 0x58],
    [0x12, 0xa5, 0x94],
    [0x00, 0x90, 0xff],
    [0x3e, 0x63, 0xdd],
    [0x8e, 0x4e, 0xc6],
    [0xd6, 0x40, 0x9f],
    [0xa1, 0x80, 0x72],
    [0x8b, 0x8d, 0x98],
    [0x60, 0x64, 0x6c],
];

/// How many recent picks the picker keeps.
const RECENT: usize = 8;
/// A swatch in the picker's grid, and the gap between two.
const CELL: f32 = 18.0;
const CELL_GAP: f32 = 4.0;
/// Swatches in a row of the grid.
const PER_ROW: usize = 8;

/// Where the open drawing's colours are left for the pickers each pass.
pub(crate) fn document_colours_id() -> egui::Id {
    egui::Id::new("colour_picker_document")
}

/// Set by an open picker every pass (so the next pass brings the colours up to date before
/// it is drawn); taken by the app.
fn document_colours_wanted_id() -> egui::Id {
    egui::Id::new("colour_picker_document_wanted")
}

/// The open drawing's colours as left for the pickers: `fresh` is false when, at the moment
/// they were offered, the drawing had changed since they were worked out.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offered {
    pub fresh: bool,
    pub colours: Vec<[u8; 3]>,
}

/// Leave the drawing's colours for this pass's pickers.
pub(crate) fn offer_document_colours(ctx: &egui::Context, fresh: bool, colours: Vec<[u8; 3]>) {
    ctx.data_mut(|d| d.insert_temp(document_colours_id(), Offered { fresh, colours }));
}

/// True (once) when a picker was open since last asked: the drawing's colours are wanted.
pub(crate) fn document_colours_wanted(ctx: &egui::Context) -> bool {
    take::<bool>(ctx, document_colours_wanted_id()).unwrap_or(false)
}

/// The drawing's colours for an open picker, which asks for them every pass so the next one
/// starts up to date. Whether they are stale is decided once, when the app offers them at the
/// top of the pass: offered stale (the picker just opened on colours from before the last
/// change) makes this pass run again (its output thrown away). A change made later in the same
/// pass, by a widget drawn before or after the picker, is not seen here: the picker shows the
/// colours from the top of the pass for that one frame, and the next pass has the new ones.
fn document_colours(ctx: &egui::Context) -> Vec<[u8; 3]> {
    let Some(offered) = ctx.data(|d| d.get_temp::<Offered>(document_colours_id())) else {
        return Vec::new();
    };
    ctx.data_mut(|d| d.insert_temp(document_colours_wanted_id(), true));
    if !offered.fresh {
        ctx.request_discard("the drawing's colours, for an open colour picker");
        // (If this pass can't be run again, the next frame has them.)
        ctx.request_repaint();
    }
    offered.colours
}

fn recent_id() -> egui::Id {
    egui::Id::new("colour_picker_recent")
}

pub(crate) fn to_color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0[0], c.0[1], c.0[2])
}

pub(crate) fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0[0], c.0[1], c.0[2])
}

/// Ink, as it shows in the theme in use (`dark`) and on paper.
pub(crate) fn ink_tip(dark: bool) -> &'static str {
    if dark {
        "Ink: white on the dark canvas, black on paper and in print"
    } else {
        "Ink: black on the canvas, on paper and in print"
    }
}

fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim().trim_start_matches('#');
    if !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let t = match t.len() {
        // #abc → #aabbcc
        3 => t.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => t.to_owned(),
        _ => return None,
    };
    let byte = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some(Rgb([byte(0)?, byte(2)?, byte(4)?]))
}

/// The theme's ink as shown now.
pub(crate) fn ink(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(0xe6)
    } else {
        Color32::from_gray(0x20)
    }
}

/// Paint a swatch: the colour, or for Ink a square split white / black corner to corner,
/// or for a named fallback what it follows (`fallback`: a colour, or Ink split as above) with
/// a small link in its corner, or, when that isn't known (several picks following different
/// colours), a neutral square crossed by a fine line.
/// A hairline edge keeps white and black swatches visible on either theme.
pub(crate) fn paint_swatch(
    painter: &egui::Painter,
    rect: Rect,
    value: Option<Rgb>,
    unset: Unset,
    fallback: Option<Follows>,
    dark: bool,
) {
    let radius = 3;
    let edge = if dark {
        Color32::from_white_alpha(70)
    } else {
        Color32::from_black_alpha(60)
    };
    let paint_ink = || {
        painter.rect_filled(rect, radius, Color32::from_gray(0x16));
        let r = rect.shrink(0.5);
        painter.add(egui::Shape::convex_polygon(
            vec![r.left_top(), r.right_top(), r.left_bottom()],
            Color32::from_gray(0xf2),
            Stroke::NONE,
        ));
    };
    match (value, unset, fallback) {
        (Some(c), _, _) => {
            painter.rect_filled(rect, radius, to_color32(c));
        }
        (None, Unset::Ink, _) | (None, Unset::Named(_), Some(Follows::Ink)) => paint_ink(),
        (None, Unset::Named(_), Some(Follows::Colour(c))) => {
            painter.rect_filled(rect, radius, to_color32(c));
        }
        (None, Unset::Named(_), None) => {
            let (fill, slash) = if dark {
                (Color32::from_gray(0x3a), Color32::from_gray(0xa0))
            } else {
                (Color32::from_gray(0xd8), Color32::from_gray(0x60))
            };
            painter.rect_filled(rect, radius, fill);
            let r = rect.shrink(2.5);
            painter.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.2, slash));
        }
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    if value.is_none() && matches!(unset, Unset::Named(_)) && fallback.is_some() {
        link_mark(painter, rect, dark);
    }
}

/// A swatch that follows another colour: a small chain link in its corner.
fn link_mark(painter: &egui::Painter, r: Rect, dark: bool) {
    let c = r.right_bottom() - vec2(3.0, 3.0);
    let (bg, fg) = if dark {
        (Color32::from_gray(0x1e), Color32::from_gray(0xd0))
    } else {
        (Color32::from_gray(0xf4), Color32::from_gray(0x40))
    };
    painter.circle_filled(c, 3.6, bg);
    for dx in [-1.2, 1.2] {
        painter.circle_stroke(c + vec2(dx, 0.0), 1.5, Stroke::new(0.9, fg));
    }
}

/// A colour control filling `width`: its swatch and its hex (or what it falls back to), one
/// click opening the picker. `name` says what it colours, read out with its colour as the
/// value (not the row's own label, which would name two controls alike). `fallback` is the
/// colour an unset value shows as, when known. Returns a new colour when one is picked
/// (`Some(None)`: back to unset).
pub(crate) fn colour_button(
    ui: &mut egui::Ui,
    name: &str,
    value: Option<Rgb>,
    unset: Unset,
    fallback: Option<Follows>,
    dark: bool,
    width: f32,
) -> Option<Option<Rgb>> {
    let height = ui.spacing().interact_size.y;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width.max(60.0), height), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect(
        rect,
        crate::theme::radius::CONTROL,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let swatch = Rect::from_center_size(
        Pos2::new(rect.left() + 4.0 + 7.0, rect.center().y),
        vec2(14.0, 14.0),
    );
    paint_swatch(ui.painter(), swatch, value, unset, fallback, dark);
    let text = value.map_or_else(|| unset.name().to_owned(), hex);
    let spoken = text.clone();
    let color = if value.is_some() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    ui.painter().text(
        Pos2::new(swatch.right() + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        color,
    );
    let tip = value.map_or_else(
        || unset.tip(dark),
        |c| format!("{} · click to change", hex(c)),
    );
    let response = response.on_hover_text(tip);
    response.widget_info(|| labeled_with_value(egui::WidgetType::ColorButton, name, &spoken));
    colour_popup(ui, &response, value, unset, fallback, dark)
}

/// A control's accessible name (`label`, what it is for) and the value it holds (`value`),
/// read out together.
pub(crate) fn labeled_with_value(
    typ: egui::WidgetType,
    label: &str,
    value: &str,
) -> egui::WidgetInfo {
    let mut info = egui::WidgetInfo::labeled(typ, true, label);
    info.current_text_value = Some(value.to_owned());
    info
}

/// The picker opened by a click on `response` (a swatch): the unset swatch (Ink, By layer…,
/// shown as `fallback` when that is known), the drawing's colours, recent picks, a palette,
/// a hex field and, under More, the full colour field. A swatch click picks and closes; six
/// hex digits pick as they are typed, three when the field is left; a drag in the colour
/// field picks once, when the button is let go. Only the colour the picker closes on is
/// kept among the recent ones.
pub(crate) fn colour_popup(
    ui: &mut egui::Ui,
    response: &egui::Response,
    value: Option<Rgb>,
    unset: Unset,
    fallback: Option<Follows>,
    dark: bool,
) -> Option<Option<Rgb>> {
    let mut out = None;
    let id = response.id.with("colour_popup");
    let field_id = id.with("field");
    let last_id = id.with("last");
    egui::Popup::from_toggle_button_response(response)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(PER_ROW as f32 * (CELL + CELL_GAP) - CELL_GAP)
        .show(|ui| {
            ui.spacing_mut().item_spacing = vec2(CELL_GAP, 6.0);
            let picked = |c: Option<Rgb>| value == c;
            // The unset swatch, named.
            ui.horizontal(|ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(CELL, CELL), egui::Sense::click());
                paint_swatch(ui.painter(), r, None, unset, fallback, dark);
                ring(ui, r, picked(None), resp.hovered());
                let resp = resp.on_hover_text(unset.tip(dark));
                let name = unset.name().to_owned();
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, picked(None), &name)
                });
                let label = ui
                    .add(egui::Label::new(unset.name()).sense(egui::Sense::click()))
                    .on_hover_text(unset.tip(dark));
                if resp.clicked() || label.clicked() {
                    out = Some(None);
                    ui.close();
                }
            });
            let document = document_colours(ui.ctx());
            let recent: Vec<[u8; 3]> = ui
                .ctx()
                .data(|d| d.get_temp(recent_id()))
                .unwrap_or_default();
            for (title, colours) in [
                ("In this drawing", document.as_slice()),
                ("Recent", recent.as_slice()),
                ("Colors", PALETTE.as_slice()),
            ] {
                if colours.is_empty() {
                    continue;
                }
                ui.label(egui::RichText::new(title).small().weak());
                for row in colours.chunks(PER_ROW) {
                    ui.horizontal(|ui| {
                        for &c in row {
                            let c = Rgb(c);
                            let (r, resp) =
                                ui.allocate_exact_size(vec2(CELL, CELL), egui::Sense::click());
                            paint_swatch(ui.painter(), r, Some(c), unset, None, dark);
                            ring(ui, r, picked(Some(c)), resp.hovered());
                            let on = picked(Some(c));
                            resp.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Button,
                                    true,
                                    on,
                                    hex(c),
                                )
                            });
                            if resp.on_hover_text(hex(c)).clicked() {
                                out = Some(Some(c));
                                ui.close();
                            }
                        }
                    });
                }
            }
            ui.separator();
            // Hex, typed or pasted (with or without the #, three or six digits). The text is
            // kept with the colour it was made from, and made again from the colour in use
            // when that changes some other way (the colour field, an undo) while the field
            // isn't being typed in.
            let text_id = id.with("hex");
            let edit_id = id.with("hex_field");
            let typing = ui.memory(|m| m.has_focus(edit_id));
            let (mut from, mut text): (Option<Rgb>, String) = ui
                .ctx()
                .data(|d| d.get_temp(text_id))
                .unwrap_or_else(|| (value, value.map_or_else(String::new, hex)));
            if from != value && !typing {
                from = value;
                text = value.map_or_else(String::new, hex);
            }
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Hex").weak());
                let r = crate::theme::field(
                    ui,
                    egui::TextEdit::singleline(&mut text)
                        .id(edit_id)
                        .hint_text("#RRGGBB")
                        .desired_width(ui.available_width()),
                );
                let parsed = parse_hex(&text);
                // Six digits as typed; three (#abc) only once the field is left, so typing
                // #abcdef doesn't pass through #AABBCC.
                let six = text.trim().trim_start_matches('#').len() == 6;
                // Esc lets go of the field too, but takes back what was typed, as egui's own
                // fields do.
                let escaped = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
                let done = r.lost_focus() && out.is_none() && !escaped;
                if let Some(c) = parsed
                    && ((r.changed() && six) || done)
                {
                    if Some(c) != value {
                        out = Some(Some(c));
                    }
                    from = Some(c);
                }
                if done || escaped {
                    // Left as the colour it gave, or as the colour in use if it gave none.
                    text = from.map_or_else(String::new, hex);
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ui.close();
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(text_id, (from, text)));
            // The full colour field, behind More.
            let more_id = id.with("more");
            let mut more = ui.ctx().data(|d| d.get_temp(more_id)).unwrap_or(false);
            let label = if more { "Less" } else { "More…" };
            if ui.small_button(label).clicked() {
                more = !more;
            }
            ui.ctx().data_mut(|d| d.insert_temp(more_id, more));
            if more {
                // A drag in the field is held here and picked once, when the button is let
                // go: one change (one undo step), not one a frame.
                let held: Option<Color32> = ui.ctx().data(|d| d.get_temp(field_id));
                let mut c = held.unwrap_or_else(|| {
                    value.map_or_else(
                        || match (unset, fallback) {
                            (Unset::Named(_), Some(f)) => f.shown(dark),
                            _ => ink(dark),
                        },
                        to_color32,
                    )
                });
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    ui.ctx().data_mut(|d| d.insert_temp(field_id, c));
                }
                if !ui.input(|i| i.pointer.any_down())
                    && let Some(c) = take::<Color32>(ui.ctx(), field_id)
                {
                    out = Some(Some(rgb_of(c)));
                }
            }
        });
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    if !open {
        // A colour still held from the field is picked as the picker closes.
        if let Some(c) = take::<Color32>(ui.ctx(), field_id) {
            out = Some(Some(rgb_of(c)));
        }
        // Shown afresh from the colour next time it opens.
        ui.ctx()
            .data_mut(|d| d.remove::<(Option<Rgb>, String)>(id.with("hex")));
    }
    match out {
        Some(Some(c)) => ui.ctx().data_mut(|d| {
            d.insert_temp(last_id, c);
        }),
        Some(None) => ui.ctx().data_mut(|d| d.remove::<Rgb>(last_id)),
        None => {}
    }
    // The colour the picker closed on goes first among the recent ones (not each one
    // passed through on the way).
    if !open && let Some(c) = take::<Rgb>(ui.ctx(), last_id) {
        remember(ui.ctx(), c);
    }
    out
}

fn rgb_of(c: Color32) -> Rgb {
    Rgb([c.r(), c.g(), c.b()])
}

/// Remove and return what is kept under `id`.
fn take<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: egui::Id) -> Option<T> {
    ctx.data_mut(|d| {
        let v = d.get_temp::<T>(id);
        d.remove::<T>(id);
        v
    })
}

/// A swatch's ring: the accent round the colour in use, a quiet one under the pointer.
fn ring(ui: &egui::Ui, r: Rect, picked: bool, hovered: bool) {
    let dark = ui.visuals().dark_mode;
    if picked {
        ui.painter().rect_stroke(
            r.expand(2.0),
            4,
            Stroke::new(1.5, crate::theme::accent(dark)),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        ui.painter().rect_stroke(
            r.expand(1.5),
            4,
            Stroke::new(1.0, ui.visuals().strong_text_color().gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
}

/// Keep `c` first among the recent picks.
fn remember(ctx: &egui::Context, c: Rgb) {
    ctx.data_mut(|d| {
        let list: &mut Vec<[u8; 3]> = d.get_temp_mut_or_default(recent_id());
        list.retain(|x| *x != c.0);
        list.insert(0, c.0);
        list.truncate(RECENT);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_takes_what_people_paste() {
        assert_eq!(parse_hex("#D04040"), Some(Rgb([0xd0, 0x40, 0x40])));
        assert_eq!(parse_hex("d04040"), Some(Rgb([0xd0, 0x40, 0x40])));
        assert_eq!(parse_hex(" #abc "), Some(Rgb([0xaa, 0xbb, 0xcc])));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
        assert_eq!(hex(Rgb([0x0a, 0xb0, 0xff])), "#0AB0FF");
        // Six bytes that aren't six characters: no colour, and no panic.
        assert_eq!(parse_hex("1€23"), None);
        assert_eq!(parse_hex("aéabc"), None);
        assert_eq!(parse_hex("#ééé"), None);
    }

    #[test]
    fn ink_is_said_as_it_shows_in_each_theme() {
        let dark = Unset::Ink.tip(true);
        let light = Unset::Ink.tip(false);
        assert!(dark.contains("white"), "{dark}");
        assert!(!light.contains("white"), "{light}");
        assert!(light.contains("black on the canvas"), "{light}");
    }

    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable as _};

    /// A colour control and every colour it gave.
    struct Picker {
        value: Option<Rgb>,
        fallback: Option<Follows>,
        picks: Vec<Option<Rgb>>,
    }

    fn picker(value: Option<Rgb>, fallback: Option<Follows>) -> Harness<'static, Picker> {
        let mut h = Harness::builder().with_size([420.0, 640.0]).build_ui_state(
            |ui, p: &mut Picker| {
                let unset = Unset::Named("By layer");
                if let Some(c) =
                    colour_button(ui, "Stroke color", p.value, unset, p.fallback, true, 120.0)
                {
                    p.value = c;
                    p.picks.push(c);
                }
            },
            Picker {
                value,
                fallback,
                picks: Vec::new(),
            },
        );
        h.run();
        h
    }

    fn open(h: &mut Harness<'static, Picker>) {
        h.get_by_role(egui::accesskit::Role::ColorWell).click();
        h.run();
        h.run();
    }

    fn open_more(h: &mut Harness<'static, Picker>) {
        open(h);
        h.get_by_label("More…").click();
        h.run();
        h.run();
    }

    /// The colour field's R value.
    fn red_box(h: &Harness<'static, Picker>) -> (Pos2, f64) {
        let r = h
            .get_all_by_role(egui::accesskit::Role::SpinButton)
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the R value");
        let value = r.accesskit_node().numeric_value().unwrap_or_default();
        (r.rect().center(), value)
    }

    fn recent(h: &Harness<'static, Picker>) -> Vec<[u8; 3]> {
        h.ctx.data(|d| d.get_temp(recent_id())).unwrap_or_default()
    }

    fn press(h: &mut Harness<'static, Picker>, at: Pos2, pressed: bool) {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn a_drag_in_the_colour_field_is_one_pick_and_one_recent_colour() {
        let mut h = picker(Some(Rgb([0x40, 0x40, 0x40])), None);
        open_more(&mut h);
        let (at, before) = red_box(&h);
        h.hover_at(at);
        h.run();
        press(&mut h, at, true);
        for i in 1..=12 {
            h.event(egui::Event::PointerMoved(
                at + egui::vec2(6.0 * i as f32, 0.0),
            ));
            h.run();
        }
        assert!(
            h.state().picks.is_empty(),
            "nothing picked while the drag goes on"
        );
        press(&mut h, at + egui::vec2(72.0, 0.0), false);
        h.run();
        let picks = &h.state().picks;
        assert_eq!(picks.len(), 1, "one pick for the drag: {picks:?}");
        let (_, after) = red_box(&h);
        assert!(
            after > before,
            "the drag changed the colour ({before} → {after})"
        );
        // Closed: the colour it closed on is the one recent colour.
        press(&mut h, egui::pos2(400.0, 620.0), true);
        press(&mut h, egui::pos2(400.0, 620.0), false);
        h.run();
        assert_eq!(recent(&h), vec![h.state().value.unwrap().0]);
    }

    #[test]
    fn hex_six_digits_pick_as_typed_three_when_entered() {
        let mut h = picker(None, None);
        open(&mut h);
        h.get_by_role(egui::accesskit::Role::TextInput).click();
        h.run();
        for c in "#abcdef".chars() {
            h.event(egui::Event::Text(c.to_string()));
            h.run();
        }
        assert_eq!(
            h.state().picks,
            vec![Some(Rgb([0xab, 0xcd, 0xef]))],
            "not #AABBCC on the way"
        );
        for _ in 0..7 {
            h.key_press(egui::Key::Backspace);
            h.run();
        }
        for c in "#fff".chars() {
            h.event(egui::Event::Text(c.to_string()));
            h.run();
        }
        assert_eq!(h.state().picks.len(), 1, "three digits wait for Enter");
        h.key_press(egui::Key::Enter);
        h.run();
        h.run();
        assert_eq!(h.state().picks.last(), Some(&Some(Rgb([0xff; 3]))));
        assert_eq!(h.state().picks.len(), 2);
        // Closed on white: only it is kept among the recent colours.
        assert_eq!(recent(&h), vec![[0xff; 3]]);
    }

    #[test]
    fn escape_takes_back_three_hex_digits_not_yet_entered() {
        let mut h = picker(Some(Rgb([0xd0, 0x40, 0x40])), None);
        open(&mut h);
        let field = h.get_by_role(egui::accesskit::Role::TextInput);
        field.click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run();
        for c in "#abc".chars() {
            h.event(egui::Event::Text(c.to_string()));
            h.run();
        }
        h.key_press(egui::Key::Escape);
        h.run();
        h.run();
        assert!(h.state().picks.is_empty(), "{:?}", h.state().picks);
        assert_eq!(h.state().value, Some(Rgb([0xd0, 0x40, 0x40])));
    }

    #[test]
    fn a_colour_well_says_what_it_colours_and_its_colour() {
        let h = picker(Some(Rgb([0xff, 0, 0])), None);
        let well = h.get_by_role(egui::accesskit::Role::ColorWell);
        assert_eq!(
            well.accesskit_node().label().as_deref(),
            Some("Stroke color")
        );
        assert_eq!(well.accesskit_node().value().as_deref(), Some("#FF0000"));
        let h = picker(None, Some(Follows::Ink));
        let well = h.get_by_role(egui::accesskit::Role::ColorWell);
        assert_eq!(well.accesskit_node().value().as_deref(), Some("By layer"));
    }

    #[test]
    fn hex_follows_the_colour_when_it_changes_elsewhere() {
        let mut h = picker(Some(Rgb([0xd0, 0x40, 0x40])), None);
        open(&mut h);
        let shown = |h: &Harness<'static, Picker>| {
            h.get_by_role(egui::accesskit::Role::TextInput)
                .accesskit_node()
                .value()
                .unwrap_or_default()
        };
        assert_eq!(shown(&h), "#D04040");
        // An undo (or the colour field) changes the colour while the picker is open.
        h.state_mut().value = Some(Rgb([0x00, 0x00, 0xff]));
        h.run();
        h.run();
        assert_eq!(shown(&h), "#0000FF");
    }

    /// A drawing's colours, kept as the app keeps them: worked out only when a picker asks.
    struct Drawing {
        colours: Vec<[u8; 3]>,
        revision: u32,
        made_at: Option<u32>,
        made: Vec<[u8; 3]>,
    }

    #[test]
    fn an_open_picker_shows_the_drawings_colours_as_they_are_this_frame() {
        let mut h = Harness::builder().with_size([420.0, 640.0]).build_ui_state(
            |ui, d: &mut Drawing| {
                if document_colours_wanted(ui.ctx()) {
                    d.made_at = Some(d.revision);
                    d.made = d.colours.clone();
                }
                offer_document_colours(ui.ctx(), d.made_at == Some(d.revision), d.made.clone());
                let _ = colour_button(ui, "Stroke color", None, Unset::Ink, None, true, 120.0);
            },
            Drawing {
                colours: vec![[0x12, 0x34, 0x56]],
                revision: 0,
                made_at: None,
                made: Vec::new(),
            },
        );
        h.run();
        assert_eq!(
            h.state().made_at,
            None,
            "nothing worked out with no picker open"
        );
        h.get_by_role(egui::accesskit::Role::ColorWell).click();
        h.run();
        assert!(h.query_by_label("#123456").is_some());
        // The drawing changes (an undo) while the picker is open: the very next frame shows
        // its colours, not the last ones, worked out before the picker is drawn, so the pass
        // isn't run twice (a drag's every frame would otherwise run the whole app twice).
        let reruns =
            |h: &Harness<'_, Drawing>| h.ctx.cumulative_pass_nr() - h.ctx.cumulative_frame_nr();
        let before = reruns(&h);
        h.state_mut().colours = vec![[0x65, 0x43, 0x21]];
        h.state_mut().revision += 1;
        h.step();
        assert!(
            h.query_by_label("#654321").is_some(),
            "this frame's colours"
        );
        assert!(h.query_by_label("#123456").is_none());
        assert_eq!(reruns(&h), before, "no pass run again");
    }

    #[test]
    fn an_unset_colour_opens_the_colour_field_at_what_it_shows() {
        let red = Color32::from_rgb(0xc8, 0x20, 0x20);
        let mut h = picker(None, Some(Follows::Colour(Rgb([0xc8, 0x20, 0x20]))));
        open_more(&mut h);
        let (_, r) = red_box(&h);
        assert_eq!(
            r,
            f64::from(red.r()),
            "the field starts at the fallback colour"
        );
    }
}
