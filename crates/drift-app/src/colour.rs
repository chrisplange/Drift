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

    fn tip(&self) -> String {
        match self {
            Unset::Ink => "Ink: white on screen, black on paper and in print".into(),
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

/// Where the drawing's colours (its layers') are left for the picker each frame.
pub(crate) fn document_colours_id() -> egui::Id {
    egui::Id::new("colour_picker_document")
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

fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim().trim_start_matches('#');
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
/// or for a named fallback the colour it falls back to (`fallback`) crossed by a fine line.
/// A hairline edge keeps white and black swatches visible on either theme.
pub(crate) fn paint_swatch(
    painter: &egui::Painter,
    rect: Rect,
    value: Option<Rgb>,
    unset: Unset,
    fallback: Option<Color32>,
    dark: bool,
) {
    let radius = 3;
    let edge = if dark {
        Color32::from_white_alpha(70)
    } else {
        Color32::from_black_alpha(60)
    };
    match (value, unset) {
        (Some(c), _) => {
            painter.rect_filled(rect, radius, to_color32(c));
        }
        (None, Unset::Ink) => {
            painter.rect_filled(rect, radius, Color32::from_gray(0x16));
            let r = rect.shrink(0.5);
            painter.add(egui::Shape::convex_polygon(
                vec![r.left_top(), r.right_top(), r.left_bottom()],
                Color32::from_gray(0xf2),
                Stroke::NONE,
            ));
        }
        (None, Unset::Named(_)) => {
            let fill = fallback.unwrap_or(if dark {
                Color32::from_gray(0x3a)
            } else {
                Color32::from_gray(0xd8)
            });
            painter.rect_filled(rect, radius, fill);
            if fallback.is_none() {
                let slash = if dark {
                    Color32::from_gray(0xa0)
                } else {
                    Color32::from_gray(0x60)
                };
                let r = rect.shrink(2.5);
                painter.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.2, slash));
            }
        }
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
}

/// A colour control filling `width`: its swatch and its hex (or what it falls back to), one
/// click opening the picker. Returns a new colour when one is picked (`Some(None)`: back to
/// unset).
pub(crate) fn colour_button(
    ui: &mut egui::Ui,
    value: Option<Rgb>,
    unset: Unset,
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
    paint_swatch(ui.painter(), swatch, value, unset, None, dark);
    let text = value.map_or_else(|| unset.name().to_owned(), hex);
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
    let tip = value.map_or_else(|| unset.tip(), |c| format!("{} · click to change", hex(c)));
    let response = response.on_hover_text(tip);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, unset.name()));
    colour_popup(ui, &response, value, unset, dark)
}

/// The picker opened by a click on `response` (a swatch): the unset swatch (Ink, By layer…),
/// the drawing's colours, recent picks, a palette, a hex field and, under More, the full
/// colour field. A swatch click picks and closes; the hex field and colour field pick as
/// they change.
pub(crate) fn colour_popup(
    ui: &mut egui::Ui,
    response: &egui::Response,
    value: Option<Rgb>,
    unset: Unset,
    dark: bool,
) -> Option<Option<Rgb>> {
    let mut out = None;
    let id = response.id.with("colour_popup");
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
                paint_swatch(ui.painter(), r, None, unset, None, dark);
                ring(ui, r, picked(None), resp.hovered());
                let resp = resp.on_hover_text(unset.tip());
                let name = unset.name().to_owned();
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, picked(None), &name)
                });
                let label = ui
                    .add(egui::Label::new(unset.name()).sense(egui::Sense::click()))
                    .on_hover_text(unset.tip());
                if resp.clicked() || label.clicked() {
                    out = Some(None);
                    ui.close();
                }
            });
            let document: Vec<[u8; 3]> = ui
                .ctx()
                .data(|d| d.get_temp(document_colours_id()))
                .unwrap_or_default();
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
            // Hex, typed or pasted (with or without the #, three or six digits).
            let text_id = id.with("hex");
            let mut text: String = ui
                .ctx()
                .data(|d| d.get_temp(text_id))
                .unwrap_or_else(|| value.map_or_else(String::new, hex));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Hex").weak());
                let r = crate::theme::field(
                    ui,
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("#RRGGBB")
                        .desired_width(ui.available_width()),
                );
                if r.changed()
                    && let Some(c) = parse_hex(&text)
                {
                    out = Some(Some(c));
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ui.close();
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(text_id, text));
            // The full colour field, behind More.
            let more_id = id.with("more");
            let mut more = ui.ctx().data(|d| d.get_temp(more_id)).unwrap_or(false);
            let label = if more { "Less" } else { "More…" };
            if ui.small_button(label).clicked() {
                more = !more;
            }
            ui.ctx().data_mut(|d| d.insert_temp(more_id, more));
            if more {
                let mut c = value.map_or(ink(dark), to_color32);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    out = Some(Some(Rgb([c.r(), c.g(), c.b()])));
                }
            }
        });
    if !egui::Popup::is_id_open(ui.ctx(), id) {
        // Shown afresh from the colour next time it opens.
        ui.ctx().data_mut(|d| d.remove::<String>(id.with("hex")));
    }
    if let Some(Some(c)) = out {
        remember(ui.ctx(), c);
    }
    out
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
    }
}
