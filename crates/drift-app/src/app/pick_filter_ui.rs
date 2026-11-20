//! The Pick filter (#480, #496): which kinds the select tool's clicks and boxes pick. On the
//! bar it is one control that names what it picks ("Pick filter  All"); a click opens the
//! kinds, each with its glyph and its name (never a glyph alone). Narrowed, the control and
//! the kinds that are on take the accent; with everything on there is no accent anywhere.
//! A click or box that hits only kinds the filter leaves out says so, and a click on that
//! message picks everything again.

use crate::annotations::DimLayout;
use crate::icons;
use crate::*;
use drift_edit::editor::{PickFilter, PickKind};

/// The kinds, by name: "Fills", "Points and edges", "Points, edges and fills".
fn kind_list(kinds: &[PickKind]) -> String {
    let names: Vec<String> = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if i == 0 {
                k.label().to_owned()
            } else {
                k.label().to_lowercase()
            }
        })
        .collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The Pick filter in a line, for the bar's tooltip, the rail and the status bar: "Pick
/// filter: Edges only", "Pick filter: Points, Edges and Fills only".
pub(crate) fn describe(filter: PickFilter) -> String {
    let kinds: Vec<&str> = filter.kinds().map(|k| k.label()).collect();
    match kinds.as_slice() {
        _ if filter.is_all() => "Pick filter: everything".to_owned(),
        [] => "Pick filter: nothing can be picked".to_owned(),
        [one] => format!("Pick filter: {one} only"),
        [rest @ .., last] => format!("Pick filter: {} and {last} only", rest.join(", ")),
    }
}

/// What a click or box that hit only `kinds` (all filtered out) says: "Fills are filtered
/// out · Pick filter: Edges only · Click here to pick everything".
pub(crate) fn filtered_out_message(kinds: &[PickKind], filter: PickFilter) -> String {
    let plural = kinds.len() > 1 || kinds.first().is_some_and(|k| k.is_plural());
    format!(
        "{} {} filtered out · {} · Click here to pick everything",
        kind_list(kinds),
        if plural { "are" } else { "is" },
        describe(filter)
    )
}

/// A check mark, centred on `c`, 8 points across.
fn check(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    painter.add(egui::Shape::line(
        vec![
            c + egui::vec2(-4.0, 0.0),
            c + egui::vec2(-1.2, 3.0),
            c + egui::vec2(4.0, -3.5),
        ],
        egui::Stroke::new(1.6, color),
    ));
}

/// The Pick filter on the bar: its caption (left out when `compact`, with something picked)
/// and a button naming what it picks, which opens the kinds. Returns the filter set.
pub(super) fn pick_filter_control(
    ui: &mut egui::Ui,
    filter: PickFilter,
    compact: bool,
) -> Option<PickFilter> {
    let dark = ui.visuals().dark_mode;
    let n = theme::neutrals(dark);
    let accent = theme::accent(dark);
    let narrowed = !filter.is_all();
    let font = egui::FontId::proportional(theme::size::LABEL);
    if !compact {
        // (Painted, so the button stays the one "Pick filter" for assistive tech.)
        let caption = ui.painter().layout_no_wrap(
            "Pick filter".to_owned(),
            font.clone(),
            ui.visuals().weak_text_color(),
        );
        let (r, _) = ui.allocate_exact_size(caption.size(), egui::Sense::hover());
        ui.painter().galley(r.min, caption, egui::Color32::WHITE);
    }
    // One kind: its glyph and its short name. More: how many. All: "All".
    let single = {
        let mut kinds = filter.kinds();
        match (kinds.next(), kinds.next()) {
            (Some(k), None) if narrowed => Some(k),
            _ => None,
        }
    };
    let text = match single {
        Some(k) => k.short_label().to_owned(),
        None => filter.summary(),
    };
    let tone = if narrowed { accent } else { n.text };
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), tone);
    let glyph = if single.is_some() { 14.0 + 5.0 } else { 0.0 };
    let width = 8.0 + glyph + galley.size().x + 8.0 + 8.0 + 8.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Pick filter");
        info.current_text_value = Some(text.clone());
        info
    });
    let fill = if response.is_pointer_button_down_on() {
        n.pressed
    } else if response.hovered() {
        n.hover
    } else {
        n.control
    };
    let edge = if narrowed {
        accent.gamma_multiply(0.7)
    } else {
        n.control_border
    };
    ui.painter().rect(
        rect,
        theme::radius::CONTROL,
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 8.0;
    if let Some(k) = single {
        let r =
            egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 7.0), egui::vec2(14.0, 14.0));
        icons::pick_kind(ui.painter(), r, k, tone);
        x += glyph;
    }
    let text_size = galley.size();
    ui.painter().galley(
        egui::pos2(x, rect.center().y - text_size.y / 2.0),
        galley,
        tone,
    );
    theme::chevron(
        ui.painter(),
        egui::pos2(rect.right() - 12.0, rect.center().y),
        true,
        if narrowed { accent } else { n.secondary },
    );
    let mut out = None;
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| out = pick_filter_list(ui, filter));
    response.on_hover_text(if narrowed {
        format!("{} · click to change", describe(filter))
    } else {
        "Pick filter: what clicks and boxes pick (everything now) · click to narrow it".to_owned()
    });
    out
}

/// The kinds, one row each (a check while it is on, its glyph and its name), then Pick
/// everything. Alt-click or double-click a kind for it alone. Returns the filter set.
fn pick_filter_list(ui: &mut egui::Ui, filter: PickFilter) -> Option<PickFilter> {
    let dark = ui.visuals().dark_mode;
    let n = theme::neutrals(dark);
    let accent = theme::accent(dark);
    let narrowed = !filter.is_all();
    let font = egui::FontId::proportional(theme::size::LABEL);
    ui.set_width(232.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut out = None;
    for kind in PickKind::ALL {
        let on = filter.has(kind);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::ROW_HEIGHT),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                on,
                format!("Pick {}", kind.label()),
            )
        });
        theme::paint_row(ui, rect, false, response.hovered());
        // On: a check, and the glyph and name in full; the accent while the filter is
        // narrowed. Off: no check, both quieter.
        let mark = if narrowed { accent } else { n.text };
        let y = rect.center().y;
        if on {
            check(ui.painter(), egui::pos2(rect.left() + 14.0, y), mark);
        }
        let glyph = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 28.0, y - 7.0),
            egui::vec2(14.0, 14.0),
        );
        icons::pick_kind(
            ui.painter(),
            glyph,
            kind,
            if on { mark } else { n.tertiary },
        );
        ui.painter().text(
            egui::pos2(rect.left() + 50.0, y),
            egui::Align2::LEFT_CENTER,
            kind.label(),
            font.clone(),
            if on { n.text } else { n.secondary },
        );
        let alone =
            response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.alt));
        if alone {
            out = Some(PickFilter::only(kind));
        } else if response.clicked() {
            out = Some(filter.with(kind, !on));
        }
        response.on_hover_text(format!("{}: picks {}", kind.label(), kind.picks()));
    }
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::ROW_HEIGHT),
        if narrowed {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, narrowed, "Pick everything")
    });
    theme::paint_row(ui, rect, false, narrowed && response.hovered());
    ui.painter().text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Pick everything",
        font,
        if narrowed { n.text } else { n.tertiary },
    );
    if narrowed && response.clicked() {
        out = Some(PickFilter::ALL);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        theme::hint(ui, "Alt-click or double-click a kind: only that kind");
    });
    out
}

impl DriftApp {
    /// A select click or box that hit only kinds the Pick filter leaves out (the tool's
    /// report, with the annotations there, which only the app lays out): say which, and the
    /// filter, in a message a click on which picks everything again.
    pub(super) fn take_filtered_out(
        &mut self,
        map: &crate::overlay::ScreenMap,
        dims: &[DimLayout],
        notes: &[crate::notes::NoteLayout],
    ) {
        let Some(report) = self.drawing.editor.out.filtered_out.take() else {
            return;
        };
        let filter = self.drawing.editor.prefs.pick_filter;
        if filter.is_all() {
            return;
        }
        let mut kinds = report.kinds;
        let r = egui::Rect::from_two_pos(
            map.to_screen(report.rect.min),
            map.to_screen(report.rect.max),
        );
        let annotations = if report.boxed {
            dims.iter()
                .any(|d| crate::picks::dimension_in_box(d, r, report.crossing))
                || notes
                    .iter()
                    .any(|l| crate::picks::note_in_box(l, r, report.crossing))
        } else {
            let at = r.center();
            dims.iter().any(|d| {
                d.text_rect
                    .expand(crate::annotations::TEXT_REACH)
                    .contains(at)
            }) || notes.iter().any(|l| l.hit(at))
        };
        if annotations {
            if filter.annotations() {
                // The box picked them: something was picked after all.
                return;
            }
            kinds.push(PickKind::Annotations);
        }
        if kinds.is_empty() {
            return;
        }
        let text = filtered_out_message(&kinds, filter);
        self.drawing.pick_all_offer = Some(text.clone());
        self.status_next = Some(text);
    }
}
