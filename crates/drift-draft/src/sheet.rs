//! Sheet furniture: the border, the title block (Drift's modern strip, ISO 7200 or
//! ASME Y14.1 style) and the revision table, drawn into a [`Drawing`] in paper millimetres.

use drift_core::PlacedView;
use drift_core::sheets::scale_label;
use drift_core::{Bounds, DVec2, FieldSet, Project, Rgb, Sheet, TitleBlock};

use crate::page::{Align, Drawing, Pen, Prim, TextStyle, styled_width};

/// A paper-coloured patch, so the view doesn't show through a table.
fn blank(d: &mut Drawing, min: DVec2, max: DVec2) {
    d.push(
        FRAME_LAYER,
        Pen::new(0.0),
        Prim::Fill {
            rings: vec![vec![
                min,
                DVec2::new(max.x, min.y),
                max,
                DVec2::new(min.x, max.y),
            ]],
            triangles: Vec::new(),
            mask: true,
        },
    );
}

/// The layer sheet furniture is drawn on.
pub const FRAME_LAYER: &str = "SHEET";

const BORDER_PEN: f64 = 0.7;
const RULE_PEN: f64 = 0.35;
const THIN_PEN: f64 = 0.18;
const TEXT_PEN: f64 = 0.25;
const LABEL_HEIGHT: f64 = 1.8;
const VALUE_HEIGHT: f64 = 3.0;
const TITLE_HEIGHT: f64 = 5.0;
const REV_ROW: f64 = 6.0;

/// How far in from the paper's edge blank paper's drawing area keeps (what printers leave).
pub const PRINT_MARGIN: f64 = 5.0;

/// The border's inner rectangle: (left, bottom, right, top) margins differ by standard (ISO
/// leaves 20 mm on the left for filing). Blank paper has no border: the paper less the
/// print margin.
pub fn border(sheet: &Sheet) -> Bounds {
    let size = sheet.size();
    let (left, other) = if sheet.title_block == TitleBlock::None {
        (PRINT_MARGIN, PRINT_MARGIN)
    } else if sheet.title_block == TitleBlock::Modern {
        // Even all round.
        (10.0, 10.0)
    } else if sheet.paper.is_ansi() || sheet.title_block == TitleBlock::Ansi {
        (12.7, 12.7)
    } else {
        (20.0, 10.0)
    };
    Bounds {
        min: DVec2::new(left, other),
        max: size - DVec2::splat(other),
    }
}

/// Where the view of the sketch goes: the area inside the border (above the modern strip).
pub fn drawing_area(sheet: &Sheet) -> Bounds {
    let mut b = border(sheet);
    if sheet.title_block == TitleBlock::Modern {
        b.min.y += STRIP;
    }
    b
}

/// The part of the drawing area a view should be centred in: clear of a corner title block.
pub fn view_area(sheet: &Sheet) -> Bounds {
    let mut area = drawing_area(sheet);
    if let (TitleBlock::Iso | TitleBlock::Ansi, Some(tb)) =
        (sheet.title_block, title_block_bounds(sheet))
    {
        area.min.y = tb.max.y;
    }
    area
}

/// Whether a frame fills the sheet's drawing area (as a sheet's one view did before views
/// were cropped in the model): its title then goes inside the area's corner.
pub fn fills_drawing_area(sheet: &Sheet, frame: Bounds) -> bool {
    let area = drawing_area(sheet);
    (frame.min - area.min).abs().max_element() < 1e-6
        && (frame.max - area.max).abs().max_element() < 1e-6
}

/// How far under a view its title reaches (paper mm), for laying views out.
pub const VIEW_TITLE_DEPTH: f64 = 11.0;

/// The views' outlines and titles: a title is the view's name in bold over a rule, with its
/// scale beneath, at the frame's lower left. Views are numbered in order when a sheet has
/// more than one.
pub fn draw_views(sheet: &Sheet, views: &[PlacedView], d: &mut Drawing) {
    let several = views.len() > 1;
    for (i, view) in views.iter().enumerate() {
        let frame = view.frame.bounds();
        let fills = fills_drawing_area(sheet, frame);
        if view.border && !fills {
            d.rect(FRAME_LAYER, Pen::new(THIN_PEN), frame.min, frame.max);
        }
        if !view.title {
            continue;
        }
        // Inside the drawing area's corner for a view that fills it.
        let (x, top) = if fills {
            (frame.min.x + 6.0, frame.min.y + 6.0 + VIEW_TITLE_DEPTH)
        } else {
            (frame.min.x, frame.min.y - 2.0)
        };
        let ink = Pen::new(TEXT_PEN).with_color(INK);
        let name = view.title().to_owned();
        let mut left = x;
        if several {
            // The view's number, in a ring.
            let centre = DVec2::new(x + 3.2, top - 3.2);
            d.push(
                FRAME_LAYER,
                Pen::new(RULE_PEN).with_color(INK),
                Prim::Circle {
                    center: centre,
                    radius: 3.2,
                },
            );
            d.styled_text(
                FRAME_LAYER,
                ink,
                centre - DVec2::new(0.0, 1.25),
                2.5,
                Align::Center,
                TextStyle::BOLD,
                (i + 1).to_string(),
            );
            left += 8.4;
        }
        let room = (frame.max.x - left).max(30.0);
        let (text, height) = fitted(&name, 3.2, TextStyle::BOLD, room);
        d.styled_text(
            FRAME_LAYER,
            ink,
            DVec2::new(left, top - 3.6),
            height,
            Align::Left,
            TextStyle::BOLD,
            text.clone(),
        );
        let rule = styled_width(&text, height, TextStyle::BOLD).max(24.0) + 4.0;
        d.line(
            FRAME_LAYER,
            Pen::new(RULE_PEN).with_color(INK),
            DVec2::new(left, top - 5.2),
            DVec2::new(left + rule.min(room), top - 5.2),
        );
        label(
            d,
            DVec2::new(left, top - 8.4),
            &format!("Scale {}", scale_label(view.scale)),
            MUTED,
        );
    }
}

/// The title block's outline, if the sheet has one.
pub fn title_block_bounds(sheet: &Sheet) -> Option<Bounds> {
    let b = border(sheet);
    let (w, h): (f64, f64) = match sheet.title_block {
        TitleBlock::Modern => (b.max.x - b.min.x, STRIP),
        TitleBlock::Iso => (180.0, 36.0),
        TitleBlock::Ansi => (177.8, 38.1),
        TitleBlock::Border | TitleBlock::None => return None,
    };
    let w = w.min(b.max.x - b.min.x);
    Some(Bounds {
        min: DVec2::new(b.max.x - w, b.min.y),
        max: DVec2::new(b.max.x, b.min.y + h),
    })
}

/// A cell: a small label at the top left and a value below it.
fn cell(d: &mut Drawing, min: DVec2, max: DVec2, label: &str, value: &str, value_height: f64) {
    let text = Pen::new(TEXT_PEN);
    d.rect(FRAME_LAYER, Pen::new(THIN_PEN), min, max);
    d.text(
        FRAME_LAYER,
        text,
        DVec2::new(min.x + 1.2, max.y - 1.2 - LABEL_HEIGHT),
        LABEL_HEIGHT,
        Align::Left,
        label,
    );
    d.text(
        FRAME_LAYER,
        text,
        DVec2::new(min.x + 1.5, min.y + 1.6),
        value_height.min(max.y - min.y - LABEL_HEIGHT - 3.5),
        Align::Left,
        value,
    );
}

/// Draw the border, title block and revision table. `project` fills in what the sheet
/// leaves to it, and says which fields the title block carries; `scale` is what the title
/// block's scale reads; `sheet_no` is (this sheet, of how many); `logo` goes in the title
/// block.
pub fn draw_frame(
    sheet: &Sheet,
    project: &Project,
    scale: &str,
    sheet_no: (usize, usize),
    logo: Option<&crate::logo::Logo>,
    d: &mut Drawing,
) {
    match sheet.title_block {
        // Blank paper.
        TitleBlock::None => return,
        TitleBlock::Modern => {
            draw_modern(sheet, project, scale, sheet_no, logo, d);
            return;
        }
        TitleBlock::Iso | TitleBlock::Ansi | TitleBlock::Border => {}
    }
    let b = border(sheet);
    d.rect(FRAME_LAYER, Pen::new(BORDER_PEN), b.min, b.max);
    // Centring marks at the middle of each side, ISO style.
    let size = sheet.size();
    let mid = size / 2.0;
    let mark = Pen::new(BORDER_PEN);
    d.line(
        FRAME_LAYER,
        mark,
        DVec2::new(mid.x, 0.0),
        DVec2::new(mid.x, b.min.y),
    );
    d.line(
        FRAME_LAYER,
        mark,
        DVec2::new(mid.x, b.max.y),
        DVec2::new(mid.x, size.y),
    );
    d.line(
        FRAME_LAYER,
        mark,
        DVec2::new(0.0, mid.y),
        DVec2::new(b.min.x, mid.y),
    );
    d.line(
        FRAME_LAYER,
        mark,
        DVec2::new(b.max.x, mid.y),
        DVec2::new(size.x, mid.y),
    );

    let f = &sheet.fields_with(project);
    let arch = project.fields == FieldSet::Architectural;
    let sheet_text = format!("{} / {}", sheet_no.0, sheet_no.1);
    let rev = sheet.revision().unwrap_or("-").to_owned();
    if let Some(tb) = title_block_bounds(sheet) {
        blank(d, tb.min, tb.max);
        d.rect(FRAME_LAYER, Pen::new(RULE_PEN), tb.min, tb.max);
        let (x0, y0, x1, y1) = (tb.min.x, tb.min.y, tb.max.x, tb.max.y);
        let left_w = 60.0_f64.min((x1 - x0) / 3.0);
        let xl = x0 + left_w;
        let row = (y1 - y0) / 4.0;
        let labels = match (sheet.title_block, arch) {
            (TitleBlock::Ansi, false) => ["CONTRACTOR", "DRAWN", "CHECKED", "MATERIAL"],
            (TitleBlock::Ansi, true) => ["CLIENT", "DRAWN", "CHECKED", "STATUS"],
            (_, false) => ["Company", "Drawn by", "Checked by", "Material"],
            (_, true) => ["Client", "Drawn by", "Checked by", "Status"],
        };
        let values = if arch {
            [&project.client, &f.author, &f.checked, &f.status]
        } else {
            [&f.company, &f.author, &f.checked, &f.material]
        };
        // The logo, at the right of the company's cell.
        if let Some(logo) = logo {
            let room = DVec2::new(left_w * 0.45, row - 2.0);
            let k = (room.x / logo.size.x).min(room.y / logo.size.y);
            let w = logo.size.x * k;
            let area = Bounds {
                min: DVec2::new(xl - 1.2 - w, y1 - row + 1.0),
                max: DVec2::new(xl - 1.2, y1 - 1.0),
            };
            logo.draw(d, FRAME_LAYER, area);
        }
        for (k, (label, value)) in labels.iter().zip(values).enumerate() {
            let top = y1 - row * k as f64;
            let label = if k == 1 && !f.date.is_empty() {
                format!("{label}  {}", f.date)
            } else {
                (*label).to_owned()
            };
            cell(
                d,
                DVec2::new(x0, top - row),
                DVec2::new(xl, top),
                &label,
                value,
                VALUE_HEIGHT,
            );
        }
        // Right part: title above, then number / revision / scale / sheet.
        let split = y0 + row * 1.6;
        let title_label = match sheet.title_block {
            TitleBlock::Ansi => "TITLE",
            _ => "Title",
        };
        cell(
            d,
            DVec2::new(xl, split),
            DVec2::new(x1, y1),
            title_label,
            if f.title.is_empty() {
                &sheet.name
            } else {
                &f.title
            },
            TITLE_HEIGHT,
        );
        let w = x1 - xl;
        let cols: Vec<(&str, String, f64)> = match sheet.title_block {
            TitleBlock::Ansi if arch => vec![
                ("PROJECT NO", project.number.clone(), 0.2),
                ("DWG NO", f.number.clone(), 0.35),
                ("REV", rev, 0.12),
                ("SCALE", scale.to_owned(), 0.16),
                ("SHEET", sheet_text, 0.17),
            ],
            _ if arch => vec![
                ("Project no.", project.number.clone(), 0.2),
                ("Drawing number", f.number.clone(), 0.27),
                ("Rev", rev, 0.13),
                ("Scale", scale.to_owned(), 0.2),
                ("Sheet", sheet_text, 0.2),
            ],
            TitleBlock::Ansi => vec![
                ("SIZE", ansi_letter(sheet), 0.12),
                ("DWG NO", f.number.clone(), 0.43),
                ("REV", rev, 0.12),
                ("SCALE", scale.to_owned(), 0.16),
                ("SHEET", sheet_text, 0.17),
            ],
            _ => vec![
                ("Drawing number", f.number.clone(), 0.47),
                ("Rev", rev, 0.13),
                ("Scale", scale.to_owned(), 0.2),
                ("Sheet", sheet_text, 0.2),
            ],
        };
        let mut x = xl;
        for (label, value, frac) in cols {
            let next = x + w * frac;
            cell(
                d,
                DVec2::new(x, y0),
                DVec2::new(next, split),
                label,
                &value,
                VALUE_HEIGHT,
            );
            x = next;
        }
    }
    if !sheet.revisions.is_empty() {
        revision_table(sheet, d);
    }
}

// --- The modern sheet -----------------------------------------------------------------------
//
// In the Swiss tradition: a hairline frame, one strong rule, generous space, and type doing
// the work. A full-width strip along the bottom carries the title (the largest thing on the
// sheet), small tracked capitals as labels over plain values, and the sheet number reversed
// out of a black square as the one graphic accent. No boxes round the cells.

/// Height of the modern title strip.
const STRIP: f64 = 24.0;
const INK: Rgb = Rgb([0x11, 0x11, 0x14]);
/// Labels and secondary text.
const MUTED: Rgb = Rgb([0x6e, 0x6e, 0x76]);
/// Hairlines between groups.
const FAINT: Rgb = Rgb([0xc2, 0xc2, 0xc8]);
const WHITE: Rgb = Rgb([0xff, 0xff, 0xff]);
const LABEL: f64 = 1.45;
const VALUE: f64 = 2.5;
/// Tracking for small capitals.
const TRACK: f64 = 0.16;

/// `text` set to fit `room`: shrunk a little if need be, then cut short.
fn fitted(text: &str, height: f64, style: TextStyle, room: f64) -> (String, f64) {
    let width = styled_width(text, height, style);
    if width <= room || room <= 0.0 {
        return (text.to_owned(), height);
    }
    let height = (height * room / width).max(height * 0.62);
    if styled_width(text, height, style) <= room {
        return (text.to_owned(), height);
    }
    let mut cut = text.to_owned();
    while !cut.is_empty() && styled_width(&format!("{}...", cut.trim_end()), height, style) > room {
        cut.pop();
    }
    (format!("{}...", cut.trim_end()), height)
}

/// A small tracked label in capitals.
fn label(d: &mut Drawing, at: DVec2, text: &str, color: Rgb) {
    d.styled_text(
        FRAME_LAYER,
        Pen::new(TEXT_PEN).with_color(color),
        at,
        LABEL,
        Align::Left,
        TextStyle::BOLD.tracked(TRACK),
        text.to_uppercase(),
    );
}

/// A label over a value, the value's baseline at `base`.
fn field(d: &mut Drawing, x: f64, base: f64, name: &str, value: &str, room: f64, bold: bool) {
    field_sized(d, x, base, name, value, room, bold, VALUE);
}

/// A label over a value of cap height `size`.
#[allow(clippy::too_many_arguments)]
fn field_sized(
    d: &mut Drawing,
    x: f64,
    base: f64,
    name: &str,
    value: &str,
    room: f64,
    bold: bool,
    size: f64,
) {
    label(d, DVec2::new(x, base + size + 2.5), name, MUTED);
    let style = if bold {
        TextStyle::BOLD
    } else {
        TextStyle::REGULAR
    };
    let (text, height) = fitted(value, size, style, room);
    d.styled_text(
        FRAME_LAYER,
        Pen::new(TEXT_PEN).with_color(INK),
        DVec2::new(x, base),
        height,
        Align::Left,
        style,
        text,
    );
}

fn draw_modern(
    sheet: &Sheet,
    project: &Project,
    scale: &str,
    sheet_no: (usize, usize),
    logo: Option<&crate::logo::Logo>,
    d: &mut Drawing,
) {
    let b = border(sheet);
    let (x0, y0, x1) = (b.min.x, b.min.y, b.max.x);
    let y1 = y0 + STRIP;
    let width = x1 - x0;
    let f = &sheet.fields_with(project);
    let arch = project.fields == FieldSet::Architectural;
    let ink = |w: f64| Pen::new(w).with_color(INK);
    let faint = Pen::new(THIN_PEN).with_color(FAINT);

    // The frame, and the rule the strip hangs from.
    d.rect(FRAME_LAYER, ink(0.25), b.min, b.max);
    d.line(
        FRAME_LAYER,
        ink(0.6),
        DVec2::new(x0, y1),
        DVec2::new(x1, y1),
    );

    // The sheet number, reversed out of a black square at the right.
    let block = STRIP.min(width / 4.0);
    let bx = x1 - block;
    d.filled_rect(
        FRAME_LAYER,
        ink(0.0),
        DVec2::new(bx, y0),
        DVec2::new(x1, y1),
    );
    label(d, DVec2::new(bx + 3.2, y1 - 4.6), "Sheet", WHITE);
    let white = Pen::new(TEXT_PEN).with_color(WHITE);
    let number = format!("{:02}", sheet_no.0);
    let numeral = 8.4_f64.min(block * 0.36);
    d.styled_text(
        FRAME_LAYER,
        white,
        DVec2::new(bx + 3.0, y0 + 3.6),
        numeral,
        Align::Left,
        TextStyle::BOLD,
        number.clone(),
    );
    d.styled_text(
        FRAME_LAYER,
        white,
        DVec2::new(x1 - 3.4, y0 + 3.6),
        1.9,
        Align::Right,
        TextStyle::REGULAR,
        format!("/ {:02}", sheet_no.1),
    );

    // Narrow paper gets narrower groups; very narrow paper drops the middle one.
    let (num_w, cell_w) = if width >= 260.0 {
        (60.0, 40.0)
    } else {
        (44.0, 28.0)
    };
    let pad = 4.0;
    let top = y1 - 9.6;
    let bottom = y0 + 3.6;
    let rule = |d: &mut Drawing, x: f64| {
        d.line(
            FRAME_LAYER,
            faint,
            DVec2::new(x, y0 + 3.0),
            DVec2::new(x, y1 - 3.0),
        );
    };

    // Drawing number, revision and scale.
    let nx = bx - num_w;
    let half = (num_w - pad) / 2.0;
    // A little larger than the other values, and set lower so the labels stay in line.
    field_sized(
        d,
        nx + pad,
        top - 0.6,
        "Drawing no.",
        &f.number,
        num_w - 2.0 * pad,
        true,
        3.1,
    );
    field(
        d,
        nx + pad,
        bottom,
        "Rev",
        sheet.revision().unwrap_or("-"),
        half - pad,
        true,
    );
    field(
        d,
        nx + pad + half,
        bottom,
        "Scale",
        scale,
        half - pad,
        false,
    );
    rule(d, nx);

    // Who and when, and what it's made of.
    let mut hero_end = nx;
    if width >= 170.0 {
        let mx = nx - 2.0 * cell_w;
        let room = cell_w - pad - 4.0;
        field(d, mx + pad, top, "Drawn", &f.author, room, false);
        field(
            d,
            mx + pad + cell_w,
            top,
            "Checked",
            &f.checked,
            room,
            false,
        );
        field(d, mx + pad, bottom, "Date", &f.date, room, false);
        let (name, value) = if arch {
            ("Status", &f.status)
        } else {
            ("Material", &f.material)
        };
        field(d, mx + pad + cell_w, bottom, name, value, room, false);
        rule(d, mx);
        hero_end = mx;
    }
    // Who it's for and where, on paper wide enough.
    if arch && width >= 300.0 {
        let px = hero_end - 2.0 * cell_w;
        let room = cell_w - pad - 4.0;
        field(d, px + pad, top, "Client", &project.client, room, false);
        field(
            d,
            px + pad + cell_w,
            top,
            "Project no.",
            &project.number,
            room,
            false,
        );
        field(
            d,
            px + pad,
            bottom,
            "Address",
            &project.address,
            2.0 * cell_w - pad - 4.0,
            false,
        );
        rule(d, px);
        hero_end = px;
    }

    // The logo leads the strip, with the title beside it.
    let mut hx = x0 + 5.0;
    if let Some(logo) = logo {
        let area = Bounds {
            min: DVec2::new(hx, y0 + 4.0),
            max: DVec2::new(hx + ((hero_end - hx) * 0.3).min(44.0), y1 - 4.0),
        };
        let used = logo.draw(d, FRAME_LAYER, area);
        if used > 0.0 {
            hx += used + 5.0;
            d.line(
                FRAME_LAYER,
                faint,
                DVec2::new(hx - 0.4, y0 + 3.0),
                DVec2::new(hx - 0.4, y1 - 3.0),
            );
            hx += 4.0;
        }
    }
    // The title: the largest type on the sheet, under whose drawing it is.
    let room = hero_end - hx - 6.0;
    let title = if f.title.trim().is_empty() {
        &sheet.name
    } else {
        &f.title
    };
    let over = if f.company.trim().is_empty() {
        "Title"
    } else {
        f.company.trim()
    };
    let (over, _) = fitted(
        &over.to_uppercase(),
        LABEL,
        TextStyle::BOLD.tracked(TRACK),
        room,
    );
    label(d, DVec2::new(hx, y1 - 4.6), &over, MUTED);
    let (text, height) = fitted(title, 5.4, TextStyle::BOLD, room);
    d.styled_text(
        FRAME_LAYER,
        ink(TEXT_PEN),
        DVec2::new(hx, y0 + 8.6),
        height,
        Align::Left,
        TextStyle::BOLD,
        text,
    );
    // Under it, the sheet's own name when the title is the drawing's.
    if !f.title.trim().is_empty() && f.title.trim() != sheet.name.trim() {
        let (text, height) = fitted(&sheet.name, 2.1, TextStyle::REGULAR, room);
        d.styled_text(
            FRAME_LAYER,
            Pen::new(TEXT_PEN).with_color(MUTED),
            DVec2::new(hx, bottom),
            height,
            Align::Left,
            TextStyle::REGULAR,
            text,
        );
    }

    if !sheet.revisions.is_empty() {
        modern_revisions(sheet, d);
    }
}

/// The revision table in the top-right corner, newest at the bottom: labels, a rule, and
/// rows parted by hairlines.
fn modern_revisions(sheet: &Sheet, d: &mut Drawing) {
    let b = border(sheet);
    let widths = [11.0, 64.0, 23.0, 14.0];
    let total: f64 = widths.iter().sum::<f64>().min(b.max.x - b.min.x);
    let x0 = b.max.x - total;
    let row = 5.6;
    let head = 6.4;
    let bottom = b.max.y - head - row * sheet.revisions.len() as f64;
    blank(d, DVec2::new(x0, bottom), b.max);
    let ink = |w: f64| Pen::new(w).with_color(INK);
    let faint = Pen::new(THIN_PEN).with_color(FAINT);
    let mut x = x0 + 3.0;
    for (w, name) in widths.iter().zip(["Rev", "Description", "Date", "By"]) {
        label(d, DVec2::new(x, b.max.y - head + 2.3), name, MUTED);
        x += w;
    }
    d.line(
        FRAME_LAYER,
        ink(0.35),
        DVec2::new(x0, b.max.y - head),
        DVec2::new(b.max.x, b.max.y - head),
    );
    for (k, r) in sheet.revisions.iter().enumerate() {
        let base = b.max.y - head - row * (k + 1) as f64;
        let cells = [&r.rev, &r.description, &r.date, &r.by];
        let mut x = x0 + 3.0;
        for (i, (w, value)) in widths.iter().zip(cells).enumerate() {
            let style = if i == 0 {
                TextStyle::BOLD
            } else {
                TextStyle::REGULAR
            };
            let (text, height) = fitted(value, 2.2, style, w - 4.0);
            d.styled_text(
                FRAME_LAYER,
                ink(TEXT_PEN),
                DVec2::new(x, base + 1.8),
                height,
                Align::Left,
                style,
                text,
            );
            x += w;
        }
        if k + 1 < sheet.revisions.len() {
            d.line(
                FRAME_LAYER,
                faint,
                DVec2::new(x0 + 3.0, base),
                DVec2::new(b.max.x - 3.0, base),
            );
        }
    }
    // Closed off below and to the left.
    d.line(
        FRAME_LAYER,
        ink(0.35),
        DVec2::new(x0, bottom),
        DVec2::new(b.max.x, bottom),
    );
    d.line(
        FRAME_LAYER,
        ink(0.25),
        DVec2::new(x0, bottom),
        DVec2::new(x0, b.max.y),
    );
}

fn ansi_letter(sheet: &Sheet) -> String {
    use drift_core::PaperSize::*;
    match sheet.paper {
        AnsiA => "A",
        AnsiB => "B",
        AnsiC => "C",
        AnsiD => "D",
        AnsiE => "E",
        A4 => "A4",
        A3 => "A3",
        A2 => "A2",
        A1 => "A1",
        A0 => "A0",
        Custom(..) => "-",
    }
    .to_owned()
}

/// The revision table in the top-right corner: newest at the bottom.
fn revision_table(sheet: &Sheet, d: &mut Drawing) {
    let b = border(sheet);
    let widths = [12.0, 70.0, 24.0, 14.0];
    let total: f64 = widths.iter().sum();
    let x0 = b.max.x - total.min(b.max.x - b.min.x);
    let header = match sheet.title_block {
        TitleBlock::Ansi => ["REV", "DESCRIPTION", "DATE", "BY"],
        _ => ["Rev", "Description", "Date", "By"],
    };
    let text = Pen::new(TEXT_PEN);
    blank(
        d,
        DVec2::new(x0, b.max.y - REV_ROW * (sheet.revisions.len() + 1) as f64),
        DVec2::new(x0 + total, b.max.y),
    );
    let rows = std::iter::once(header.map(str::to_owned)).chain(sheet.revisions.iter().map(|r| {
        [
            r.rev.clone(),
            r.description.clone(),
            r.date.clone(),
            r.by.clone(),
        ]
    }));
    for (k, cells) in rows.enumerate() {
        let top = b.max.y - REV_ROW * k as f64;
        let mut x = x0;
        for (w, value) in widths.iter().zip(cells) {
            d.rect(
                FRAME_LAYER,
                Pen::new(if k == 0 { RULE_PEN } else { THIN_PEN }),
                DVec2::new(x, top - REV_ROW),
                DVec2::new(x + w, top),
            );
            let h = if k == 0 { LABEL_HEIGHT + 0.2 } else { 2.5 };
            d.text(
                FRAME_LAYER,
                text,
                DVec2::new(x + 1.2, top - REV_ROW / 2.0 - h / 2.0),
                h,
                Align::Left,
                value,
            );
            x += w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Prim;
    use drift_core::Revision;

    fn texts(d: &Drawing) -> Vec<String> {
        d.items
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn frame_fits_the_paper_and_fills_the_block() {
        let mut sheet = Sheet::new("Plate");
        sheet.title_block = TitleBlock::Iso;
        sheet.fields.number = "DR-001".into();
        sheet.fields.author = "CP".into();
        sheet.revisions.push(Revision {
            rev: "A".into(),
            date: "2026-10-08".into(),
            description: "First issue".into(),
            by: "CP".into(),
        });
        let mut d = Drawing::new(sheet.size());
        draw_frame(&sheet, &Project::default(), "1:2", (1, 2), None, &mut d);
        let b = d.bounds();
        assert!(b.min.x >= -1e-9 && b.min.y >= -1e-9);
        assert!(b.max.x <= 420.0 + 1e-9 && b.max.y <= 297.0 + 1e-9);
        let t = texts(&d);
        for want in ["Plate", "DR-001", "1:2", "1 / 2", "A", "First issue", "CP"] {
            assert!(t.iter().any(|x| x == want), "{want} in {t:?}");
        }
        // The title block sits in the bottom-right corner of the border.
        let tb = title_block_bounds(&sheet).unwrap();
        assert!((tb.max.x - 410.0).abs() < 1e-9 && (tb.min.y - 10.0).abs() < 1e-9);
        sheet.title_block = TitleBlock::Border;
        assert!(title_block_bounds(&sheet).is_none());
    }

    #[test]
    fn border_only_draws_the_frame_and_none_draws_nothing() {
        let mut sheet = Sheet::new("Board");
        sheet.revisions.push(Revision {
            rev: "A".into(),
            date: "2026-10-20".into(),
            description: "First issue".into(),
            by: "CL".into(),
        });
        let frame = |sheet: &Sheet| {
            let mut d = Drawing::new(sheet.size());
            draw_frame(sheet, &Project::default(), "1:1", (1, 1), None, &mut d);
            d
        };
        // Border only: the ISO border and four centring marks, with the revision table.
        sheet.title_block = TitleBlock::Border;
        let d = frame(&sheet);
        let b = border(&sheet);
        assert!(d.items.iter().any(|i| matches!(
            &i.prim,
            Prim::Polyline { points, closed: true } if points.contains(&b.min)
        )));
        let marks = d
            .items
            .iter()
            .filter(|i| {
                matches!(i.prim, Prim::Line(a, _) if a.x == 0.0 || a.y == 0.0
                || a.x == 210.0 || a.y == 148.5)
            })
            .count();
        assert!(marks >= 2, "{:?}", d.items);
        assert!(texts(&d).iter().any(|t| t == "First issue"));
        assert_eq!(view_area(&sheet), drawing_area(&sheet));
        // None: blank paper, and the whole of it less a print margin to draw on.
        sheet.title_block = TitleBlock::None;
        assert!(frame(&sheet).items.is_empty());
        assert!(title_block_bounds(&sheet).is_none());
        assert_eq!(
            view_area(&sheet),
            Bounds {
                min: DVec2::splat(PRINT_MARGIN),
                max: DVec2::new(420.0, 297.0) - DVec2::splat(PRINT_MARGIN),
            }
        );
        // Keys: an old "none" is Border only, so old files print as before.
        assert_eq!(TitleBlock::parse("none"), Some(TitleBlock::Border));
        assert_eq!(TitleBlock::parse("blank"), Some(TitleBlock::None));
    }

    #[test]
    fn the_modern_strip_fits_every_paper() {
        use drift_core::PaperSize;
        for (paper, portrait) in [
            (PaperSize::A3, false),
            (PaperSize::A4, true),
            (PaperSize::A4, false),
            (PaperSize::A0, false),
            (PaperSize::AnsiA, true),
        ] {
            let mut sheet = Sheet::new("General arrangement");
            assert_eq!(sheet.title_block, TitleBlock::Modern);
            sheet.paper = paper;
            sheet.portrait = portrait;
            sheet.fields.title = "A title long enough that it must be shrunk or cut to fit".into();
            sheet.fields.number = "DR-0142-REV-LONG-NUMBER".into();
            sheet.fields.company = "Northlight Works".into();
            sheet.fields.author = "A very long name indeed".into();
            sheet.fields.material = "6061-T6, clear anodised, 25 µm".into();
            sheet.revisions.push(Revision {
                rev: "A".into(),
                date: "2026-10-11".into(),
                description: "First issue".into(),
                by: "CL".into(),
            });
            let mut d = Drawing::new(sheet.size());
            draw_frame(&sheet, &Project::default(), "1:1", (3, 12), None, &mut d);
            let b = border(&sheet);
            // The view sits above the strip; the strip spans the frame.
            let area = drawing_area(&sheet);
            let strip = title_block_bounds(&sheet).unwrap();
            assert!((area.min.y - strip.max.y).abs() < 1e-9);
            assert!((strip.max.x - strip.min.x - (b.max.x - b.min.x)).abs() < 1e-9);
            assert_eq!(view_area(&sheet), area);
            // Every line of text stays inside the frame, whatever the paper.
            for item in &d.items {
                if let Prim::Text {
                    at,
                    height,
                    align,
                    text,
                    style,
                    ..
                } = &item.prim
                {
                    let w = styled_width(text, *height, *style);
                    let (left, right) = match align {
                        Align::Left => (at.x, at.x + w),
                        Align::Right => (at.x - w, at.x),
                        Align::Center => (at.x - w / 2.0, at.x + w / 2.0),
                    };
                    assert!(
                        left >= b.min.x - 1e-9 && right <= b.max.x + 1e-9,
                        "{text:?} on {paper:?}: {left}..{right}"
                    );
                    assert!(at.y >= b.min.y && at.y + height <= b.max.y);
                }
            }
            let t = texts(&d);
            assert!(
                t.iter().any(|x| x == "03") && t.iter().any(|x| x == "/ 12"),
                "{t:?}"
            );
            assert!(t.iter().any(|x| x == "NORTHLIGHT WORKS"));
        }
    }
}
