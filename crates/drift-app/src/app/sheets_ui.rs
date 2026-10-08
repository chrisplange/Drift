//! Sheets: the Sheets tab, the sheet preview on the canvas, PDF / SVG / DXF export, printing
//! and DXF import.

use drift_core::sheets::{PREFERRED_SCALES, scale_label};
use drift_core::{PaperSize, Revision, Sheet, SheetId, TitleBlock};
use drift_draft::page::Drawing;
use drift_platform::FileType;

use crate::*;

pub(crate) const PDF_FILE: FileType = FileType {
    name: "PDF",
    extensions: &["pdf"],
};
pub(crate) const SVG_FILE: FileType = FileType {
    name: "SVG",
    extensions: &["svg"],
};
pub(crate) const DXF_FILE: FileType = FileType {
    name: "DXF",
    extensions: &["dxf"],
};

/// How the sheet preview is looked at.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PreviewView {
    /// Paper point at the canvas centre (mm).
    pub center: DVec2,
    /// Screen points per paper mm; 0 means "fit on the next frame".
    pub zoom: f32,
}

/// A view move in progress: the sheet's view centre and the pointer when it started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewDrag {
    sheet: SheetId,
    center: DVec2,
    from: egui::Pos2,
}

/// Today's date as `YYYY-MM-DD` (UTC).
pub(crate) fn today() -> String {
    let secs = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The revision after `rev`: A → B … Z → AA; numbers count up.
fn next_revision(rev: Option<&str>) -> String {
    match rev {
        None => "A".into(),
        Some(r) if r.chars().all(|c| c.is_ascii_digit()) && !r.is_empty() => {
            (r.parse::<u64>().unwrap_or(0) + 1).to_string()
        }
        Some(r) => {
            let mut chars: Vec<u8> = r.to_ascii_uppercase().into_bytes();
            let mut i = chars.len();
            loop {
                if i == 0 {
                    chars.insert(0, b'A');
                    break;
                }
                i -= 1;
                if chars[i] == b'Z' {
                    chars[i] = b'A';
                } else if chars[i].is_ascii_uppercase() {
                    chars[i] += 1;
                    break;
                } else {
                    chars.push(b'A');
                    break;
                }
            }
            String::from_utf8(chars).unwrap_or_else(|_| "A".into())
        }
    }
}

impl DriftApp {
    /// The file name without its extension, for exports.
    fn file_stem(&self) -> String {
        let name = &self.file.name;
        name.rsplit_once('.')
            .map_or(name.as_str(), |(s, _)| s)
            .to_owned()
    }

    /// The sheet shown in the Sheets tab and the preview (the first, if none is picked).
    pub(crate) fn current_sheet(&self) -> Option<SheetId> {
        self.active_sheet
            .filter(|id| self.editor.doc.sheet(*id).is_some())
            .or_else(|| self.editor.doc.sheets().next().map(|(id, _)| id))
    }

    /// A new sheet titled after the file, its view fitted to the sketch.
    fn fresh_sheet(&self, name: &str) -> Sheet {
        let mut sheet = Sheet::new(name);
        sheet.fields.title = self.file_stem();
        sheet.fields.date = today();
        crate::output::fit_sheet(self.fit_bounds(), &mut sheet);
        sheet
    }

    pub(crate) fn add_sheet(&mut self) -> SheetId {
        let n = self.editor.doc.sheets().count() + 1;
        let sheet = self.fresh_sheet(&format!("Sheet {n}"));
        self.editor.doc.begin();
        let id = self.editor.doc.add_sheet(sheet);
        self.editor.commit("New sheet");
        self.active_sheet = Some(id);
        id
    }

    /// Change a sheet. Typing into its fields merges into one undo step.
    fn update_sheet(&mut self, id: SheetId, sheet: Sheet, typing: bool) {
        if self.editor.doc.sheet(id) == Some(&sheet) {
            return;
        }
        let label = if typing {
            "Edit sheet text"
        } else {
            "Edit sheet"
        };
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        let tx = self.editor.doc.commit(label);
        if typing && self.editor.history.undo_label() == Some(label) {
            self.editor.history.merge_into_last(tx);
        } else {
            self.editor.history.push(tx);
        }
        self.editor.sync();
    }

    /// Every sheet as a page (or, with no sheets, a fitted A3 sheet).
    fn pages(&mut self, painter: &egui::Painter) -> Vec<Drawing> {
        let sheets: Vec<Sheet> = self.editor.doc.sheets().map(|(_, s)| s.clone()).collect();
        let sheets = if sheets.is_empty() {
            vec![self.fresh_sheet("Sheet 1")]
        } else {
            sheets
        };
        let count = sheets.len();
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        sheets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                crate::output::sheet_drawing(
                    &mut self.editor,
                    &self.hatch_patterns,
                    painter,
                    s,
                    (i + 1, count),
                )
            })
            .collect()
    }

    pub(crate) fn pdf_bytes(&mut self, painter: &egui::Painter) -> Vec<u8> {
        let pages = self.pages(painter);
        drift_io::pdf::write(&pages, &self.file_stem())
    }

    pub(crate) fn export_pdf(&mut self, painter: &egui::Painter) {
        let bytes = self.pdf_bytes(painter);
        let name = format!("{}.pdf", self.file_stem());
        self.files.save_as(&name, PDF_FILE, bytes);
    }

    /// The current sheet as SVG.
    pub(crate) fn svg_text(&mut self, painter: &egui::Painter) -> String {
        let pages = self.pages(painter);
        let index = self
            .current_sheet()
            .and_then(|id| self.editor.doc.sheets().position(|(s, _)| s == id))
            .unwrap_or(0);
        drift_io::svg::write(&pages[index.min(pages.len() - 1)])
    }

    pub(crate) fn export_svg(&mut self, painter: &egui::Painter) {
        let text = self.svg_text(painter);
        let name = format!("{}.svg", self.file_stem());
        self.files.save_as(&name, SVG_FILE, text.into_bytes());
    }

    pub(crate) fn dxf_text(&mut self, painter: &egui::Painter) -> String {
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let d = crate::output::model_drawing(&mut self.editor, &self.hatch_patterns, painter);
        drift_io::dxf::write(&d, drift_io::dxf::DxfUnits::of(&self.editor.doc.units()))
    }

    pub(crate) fn export_dxf(&mut self, painter: &egui::Painter) {
        let text = self.dxf_text(painter);
        let name = format!("{}.dxf", self.file_stem());
        self.files.save_as(&name, DXF_FILE, text.into_bytes());
    }

    /// Open the sheets as a PDF in the system viewer (a browser tab on the web) to print.
    pub(crate) fn print(&mut self, painter: &egui::Painter) {
        let bytes = self.pdf_bytes(painter);
        let name = format!("{}.pdf", self.file_stem());
        self.files.open_externally(&name, &bytes);
    }

    /// Add a DXF file's curves to the document, as one undo step.
    pub(crate) fn import_dxf(&mut self, name: &str, text: &str, now: f64) {
        let unit = self.editor.doc.units().length.mm_per_unit();
        match drift_io::dxf::read(text, unit) {
            Ok(import) => {
                self.abort_operation();
                self.editor.doc.begin();
                let report = drift_io::dxf::add_to_document(&mut self.editor.doc, &import);
                self.editor.commit("Import DXF");
                let skipped: usize = report.skipped.values().sum();
                let mut msg = format!(
                    "Imported {} curve(s) from {name}{}",
                    report.curves,
                    if report.layers > 0 {
                        format!(" on {} new layer(s)", report.layers)
                    } else {
                        String::new()
                    }
                );
                if report.copies > 0 {
                    msg.push_str(&format!(
                        ", {} block copies ({} new blocks)",
                        report.copies, report.blocks
                    ));
                }
                if skipped > 0 {
                    let kinds: Vec<String> = report
                        .skipped
                        .iter()
                        .map(|(k, n)| format!("{n} {k}"))
                        .collect();
                    msg.push_str(&format!("; skipped {}", kinds.join(", ")));
                }
                self.set_status(msg, false, now);
                self.fit_target = Some(self.fit_bounds());
            }
            Err(e) => self.set_status(format!("Could not import {name}: {e}"), true, now),
        }
    }

    // ----- The Sheets tab --------------------------------------------------------------------

    pub(crate) fn sheets_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.sheets_contents(ui));
    }

    fn sheets_contents(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Sheets");
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .doc
            .sheets()
            .map(|(id, s)| (id, s.name.clone()))
            .collect();
        let current = self.current_sheet();
        if sheets.is_empty() {
            ui.label(
                egui::RichText::new(
                    "A sheet lays the drawing out on paper at a scale, with a border, title \
                     block and revision table, for printing and PDF.",
                )
                .weak(),
            );
        }
        for (id, name) in &sheets {
            if ui
                .add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::selectable(current == Some(*id), name),
                )
                .clicked()
            {
                self.active_sheet = Some(*id);
            }
        }
        ui.horizontal(|ui| {
            if ui.button("New sheet").clicked() {
                self.add_sheet();
                self.sheet_preview = true;
                self.preview.zoom = 0.0;
            }
            if current.is_some() {
                let label = if self.sheet_preview {
                    "Back to model"
                } else {
                    "Show sheet"
                };
                if ui.button(label).clicked() {
                    self.sheet_preview = !self.sheet_preview;
                    self.preview.zoom = 0.0;
                }
            }
        });
        let Some(id) = current else {
            theme::section(ui, "Output");
            self.output_buttons(ui);
            return;
        };
        let Some(original) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let mut sheet = original.clone();
        let mut typing = false;
        theme::section(ui, "Paper");
        theme::row(ui, "Name", |ui| {
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut sheet.name)
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
        });
        theme::row(ui, "Size", |ui| {
            egui::ComboBox::from_id_salt("paper_size")
                .selected_text(sheet.paper.label())
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for p in PaperSize::STANDARD {
                        ui.selectable_value(&mut sheet.paper, p, p.label());
                    }
                });
        });
        if let Some(portrait) = theme::segmented(
            ui,
            sheet.portrait,
            &[(false, "Landscape"), (true, "Portrait")],
        ) {
            sheet.portrait = portrait;
        }
        theme::section(ui, "View");
        theme::row(ui, "Scale", |ui| {
            egui::ComboBox::from_id_salt("sheet_scale")
                .selected_text(scale_label(sheet.scale))
                .width(90.0)
                .show_ui(ui, |ui| {
                    for n in PREFERRED_SCALES {
                        ui.selectable_value(&mut sheet.scale, n, scale_label(n));
                    }
                });
            if ui
                .button("Fit")
                .on_hover_text("Centre the drawing on the sheet at the largest scale that fits")
                .clicked()
            {
                crate::output::fit_sheet(self.fit_bounds(), &mut sheet);
            }
        });
        ui.label(
            egui::RichText::new("Drag on the sheet to move the drawing within it.")
                .small()
                .weak(),
        );
        theme::section(ui, "Title block");
        egui::ComboBox::from_id_salt("title_block")
            .selected_text(sheet.title_block.label())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for t in TitleBlock::ALL {
                    ui.selectable_value(&mut sheet.title_block, t, t.label());
                }
            });
        let f = &mut sheet.fields;
        for (label, value, hint) in [
            ("Title", &mut f.title, "Drawing title"),
            ("Number", &mut f.number, "DR-001"),
            ("Drawn by", &mut f.author, ""),
            ("Checked", &mut f.checked, ""),
            ("Company", &mut f.company, ""),
            ("Material", &mut f.material, ""),
        ] {
            theme::row(ui, label, |ui| {
                typing |= ui
                    .add(
                        egui::TextEdit::singleline(value)
                            .hint_text(hint)
                            .desired_width(ui.available_width() - 8.0),
                    )
                    .changed();
            });
        }
        theme::row(ui, "Date", |ui| {
            typing |= ui
                .add(egui::TextEdit::singleline(&mut f.date).desired_width(96.0))
                .changed();
            if ui.small_button("Today").clicked() {
                f.date = today();
            }
        });
        theme::section(ui, "Revisions");
        let mut remove = None;
        for (k, r) in sheet.revisions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                typing |= ui
                    .add(egui::TextEdit::singleline(&mut r.rev).desired_width(26.0))
                    .changed();
                typing |= ui
                    .add(egui::TextEdit::singleline(&mut r.date).desired_width(78.0))
                    .changed();
                typing |= ui
                    .add(
                        egui::TextEdit::singleline(&mut r.by)
                            .hint_text("by")
                            .desired_width(30.0),
                    )
                    .changed();
                if ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text("Remove this revision")
                    .clicked()
                {
                    remove = Some(k);
                }
            });
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut r.description)
                        .hint_text("Description")
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
            ui.add_space(4.0);
        }
        if let Some(k) = remove {
            sheet.revisions.remove(k);
        }
        if ui.button("Add revision").clicked() {
            let rev = next_revision(sheet.revisions.last().map(|r| r.rev.as_str()));
            let first = sheet.revisions.is_empty();
            sheet.revisions.push(Revision {
                rev,
                date: today(),
                description: if first {
                    "First issue".into()
                } else {
                    String::new()
                },
                by: sheet.fields.author.clone(),
            });
        }
        if sheet != original {
            self.update_sheet(id, sheet, typing);
        }
        theme::section(ui, "Output");
        self.output_buttons(ui);
        ui.add_space(10.0);
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::new(
                    egui::RichText::new("Delete sheet")
                        .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                ),
            )
            .clicked()
        {
            self.editor.doc.begin();
            self.editor.doc.remove_sheet(id);
            self.editor.commit("Delete sheet");
            self.active_sheet = None;
            if self.editor.doc.sheets().next().is_none() {
                self.sheet_preview = false;
            }
        }
    }

    fn output_buttons(&mut self, ui: &mut egui::Ui) {
        let painter = ui.painter().clone();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("PDF…")
                .on_hover_text("Every sheet, at true scale")
                .clicked()
            {
                self.export_pdf(&painter);
            }
            if ui
                .button("SVG…")
                .on_hover_text("This sheet, at true scale")
                .clicked()
            {
                self.export_svg(&painter);
            }
            if ui
                .button("DXF…")
                .on_hover_text("The model at full size, with its layers and line types")
                .clicked()
            {
                self.export_dxf(&painter);
            }
            if ui.button("Print…").clicked() {
                self.print(&painter);
            }
        });
    }

    // ----- The sheet preview -----------------------------------------------------------------

    /// The canvas in sheet mode: the page as it prints. Scroll zooms, middle / right drag
    /// pans, and dragging with the primary button moves the drawing within the sheet.
    pub(crate) fn sheet_canvas(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.current_sheet() else {
            self.sheet_preview = false;
            return;
        };
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.canvas_rect = rect;
        self.pointer = None;
        let painter = ui.painter_at(rect);
        let dark = ui.visuals().dark_mode;
        painter.rect_filled(
            rect,
            0.0,
            if dark {
                egui::Color32::from_rgb(0x2a, 0x2c, 0x31)
            } else {
                egui::Color32::from_rgb(0xd5, 0xd8, 0xde)
            },
        );
        let Some(sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let size = sheet.size();
        // Build the page (cached while the document doesn't change).
        let revision = self.editor.doc.revision();
        let stale = !matches!(&self.preview_cache, Some((r, s, _)) if *r == revision && *s == id);
        if stale {
            let number = self
                .editor
                .doc
                .sheets()
                .position(|(s, _)| s == id)
                .map_or(1, |i| i + 1);
            let count = self.editor.doc.sheets().count();
            self.hatch_patterns
                .update(self.editor.doc.custom_patterns());
            let d = crate::output::sheet_drawing(
                &mut self.editor,
                &self.hatch_patterns,
                &painter,
                &sheet,
                (number, count),
            );
            self.preview_cache = Some((revision, id, d));
        }
        if self.preview.zoom <= 0.0 || response.double_clicked_by(PointerButton::Middle) {
            let fit =
                ((rect.width() - 48.0) / size.x as f32).min((rect.height() - 48.0) / size.y as f32);
            self.preview.zoom = fit.max(0.05);
            self.preview.center = size * 0.5;
        }
        // Zoom at the pointer, pan with middle / right drag.
        if let Some(pos) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.002).exp() * pinch;
            if factor != 1.0 {
                let before = self.preview_to_paper(rect, pos);
                self.preview.zoom = (self.preview.zoom * factor).clamp(0.05, 200.0);
                let after = self.preview_to_paper(rect, pos);
                self.preview.center += before - after;
            }
        }
        if response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary)
        {
            let d = response.drag_delta();
            self.preview.center -=
                DVec2::new(f64::from(d.x), -f64::from(d.y)) / f64::from(self.preview.zoom);
        }
        // Primary drag moves the view of the sketch within the sheet.
        if response.drag_started_by(PointerButton::Primary)
            && let Some(from) = ui.input(|i| i.pointer.press_origin())
        {
            self.view_drag = Some(ViewDrag {
                sheet: id,
                center: sheet.center,
                from,
            });
            self.editor.doc.begin();
        }
        if let Some(drag) = self.view_drag {
            if let Some(pos) = response.interact_pointer_pos() {
                let d = pos - drag.from;
                let paper =
                    DVec2::new(f64::from(d.x), -f64::from(d.y)) / f64::from(self.preview.zoom);
                let mut moved = sheet.clone();
                moved.center = drag.center - paper * sheet.scale;
                self.editor.doc.set_sheet(drag.sheet, moved);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.view_drag = None;
                self.editor.commit("Move sheet view");
            }
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if self.view_drag.is_some() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }

        let zoom = self.preview.zoom;
        let to_screen = |p: DVec2| self.paper_to_preview(rect, p);
        let (a, b) = (to_screen(DVec2::ZERO), to_screen(size));
        let page = egui::Rect::from_two_pos(a, b);
        painter.rect_filled(
            page.translate(egui::vec2(0.0, 3.0)).expand(1.0),
            2.0,
            egui::Color32::from_black_alpha(60),
        );
        painter.rect_filled(page, 0.0, egui::Color32::WHITE);
        if let Some((_, _, d)) = &self.preview_cache {
            crate::output::paint_drawing(
                &painter.with_clip_rect(page.intersect(rect)),
                d,
                to_screen,
                zoom,
            );
        }
        // What this is, and how to leave.
        let info = format!(
            "{} · {} {} · {}  —  drag to move the drawing · Esc returns to the model",
            sheet.name,
            sheet.paper.label(),
            if sheet.portrait {
                "portrait"
            } else {
                "landscape"
            },
            scale_label(sheet.scale)
        );
        let galley = painter.layout_no_wrap(
            info,
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
        let at = rect.left_top() + egui::vec2(12.0, 10.0);
        painter.rect_filled(
            egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(8.0, 4.0)),
            8.0,
            ui.visuals().window_fill.gamma_multiply(0.92),
        );
        painter.galley(at, galley, ui.visuals().text_color());
    }

    fn paper_to_preview(&self, rect: egui::Rect, p: DVec2) -> egui::Pos2 {
        let d = (p - self.preview.center) * f64::from(self.preview.zoom);
        rect.center() + egui::vec2(d.x as f32, -d.y as f32)
    }

    fn preview_to_paper(&self, rect: egui::Rect, pos: egui::Pos2) -> DVec2 {
        let v = pos - rect.center();
        self.preview.center
            + DVec2::new(f64::from(v.x), -f64::from(v.y)) / f64::from(self.preview.zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_and_dates() {
        assert_eq!(next_revision(None), "A");
        assert_eq!(next_revision(Some("A")), "B");
        assert_eq!(next_revision(Some("Z")), "AA");
        assert_eq!(next_revision(Some("AZ")), "BA");
        assert_eq!(next_revision(Some("3")), "4");
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
        assert_eq!(&t[4..5], "-");
    }
}
