//! Sheet furniture: the border, the title block (ISO 7200 or ASME Y14.1 style) and the
//! revision table, drawn into a [`Drawing`] in paper millimetres.

use drift_core::sheets::scale_label;
use drift_core::{Bounds, DVec2, Sheet, TitleBlock};

use crate::page::{Align, Drawing, Pen, Prim};

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

/// The border's inner rectangle: (left, bottom, right, top) margins differ by standard (ISO
/// leaves 20 mm on the left for filing).
pub fn border(sheet: &Sheet) -> Bounds {
    let size = sheet.size();
    let (left, other) = if sheet.paper.is_ansi() || sheet.title_block == TitleBlock::Ansi {
        (12.7, 12.7)
    } else {
        (20.0, 10.0)
    };
    Bounds {
        min: DVec2::new(left, other),
        max: size - DVec2::splat(other),
    }
}

/// Where the view of the sketch goes: the area inside the border.
pub fn drawing_area(sheet: &Sheet) -> Bounds {
    border(sheet)
}

/// The title block's outline, if the sheet has one.
pub fn title_block_bounds(sheet: &Sheet) -> Option<Bounds> {
    let b = border(sheet);
    let (w, h): (f64, f64) = match sheet.title_block {
        TitleBlock::Iso => (180.0, 36.0),
        TitleBlock::Ansi => (177.8, 38.1),
        TitleBlock::None => return None,
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

/// Draw the border, title block and revision table. `sheet_no` is (this sheet, of how many).
pub fn draw_frame(sheet: &Sheet, sheet_no: (usize, usize), d: &mut Drawing) {
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

    let f = &sheet.fields;
    let scale = scale_label(sheet.scale);
    let sheet_text = format!("{} / {}", sheet_no.0, sheet_no.1);
    let rev = sheet.revision().unwrap_or("-").to_owned();
    if let Some(tb) = title_block_bounds(sheet) {
        blank(d, tb.min, tb.max);
        d.rect(FRAME_LAYER, Pen::new(RULE_PEN), tb.min, tb.max);
        let (x0, y0, x1, y1) = (tb.min.x, tb.min.y, tb.max.x, tb.max.y);
        let left_w = 60.0_f64.min((x1 - x0) / 3.0);
        let xl = x0 + left_w;
        let row = (y1 - y0) / 4.0;
        let labels = match sheet.title_block {
            TitleBlock::Ansi => ["CONTRACTOR", "DRAWN", "CHECKED", "MATERIAL"],
            _ => ["Company", "Drawn by", "Checked by", "Material"],
        };
        let values = [&f.company, &f.author, &f.checked, &f.material];
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
            TitleBlock::Ansi => vec![
                ("SIZE", ansi_letter(sheet), 0.12),
                ("DWG NO", f.number.clone(), 0.43),
                ("REV", rev, 0.12),
                ("SCALE", scale, 0.16),
                ("SHEET", sheet_text, 0.17),
            ],
            _ => vec![
                ("Drawing number", f.number.clone(), 0.47),
                ("Rev", rev, 0.13),
                ("Scale", scale, 0.2),
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
        sheet.scale = 2.0;
        sheet.fields.number = "DR-001".into();
        sheet.fields.author = "CP".into();
        sheet.revisions.push(Revision {
            rev: "A".into(),
            date: "2026-10-08".into(),
            description: "First issue".into(),
            by: "CP".into(),
        });
        let mut d = Drawing::new(sheet.size());
        draw_frame(&sheet, (1, 2), &mut d);
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
        sheet.title_block = TitleBlock::None;
        assert!(title_block_bounds(&sheet).is_none());
    }
}
