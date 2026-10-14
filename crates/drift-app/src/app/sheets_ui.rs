//! Sheets: the Sheets tab, the sheet preview on the canvas, PDF / SVG / DXF export, printing
//! and DXF import.

use drift_core::sheets::{PREFERRED_SCALES, scale_label};
use drift_core::{PaperSize, Revision, Sheet, SheetId, SheetView, TitleBlock, ViewFrame};
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

/// A change to the sheet's views asked for from the panel, made once its own edits are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewAction {
    Add,
    Remove,
}

/// What an import brought in and what it couldn't, shown until closed.
#[derive(Clone, Debug)]
pub(crate) struct ImportSummary {
    pub title: String,
    pub summary: String,
    /// Kinds of object left out, with counts.
    pub skipped: Vec<String>,
    /// What was read approximately, in sentences.
    pub notes: Vec<String>,
}

/// How the sheet preview is looked at.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PreviewView {
    /// Paper point at the canvas centre (mm).
    pub center: DVec2,
    /// Screen points per paper mm; 0 means "fit on the next frame".
    pub zoom: f32,
}

/// What a drag on a view in the sheet editor does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ViewGrip {
    /// Slide the drawing inside the view.
    Pan,
    /// Move the view on the paper.
    Move,
    /// Move an edge or corner of its frame: which way each axis' edge goes (-1 the lower,
    /// 1 the upper, 0 neither).
    Resize(i8, i8),
}

/// A drag on a view in progress: the view as it was, and the pointer, when it started.
#[derive(Clone, Debug)]
pub(crate) struct ViewDrag {
    sheet: SheetId,
    index: usize,
    grip: ViewGrip,
    was: SheetView,
    from: egui::Pos2,
}

/// The smallest a view's frame can be made (paper mm), and how near a handle counts (points).
const MIN_FRAME: f64 = 10.0;
const HANDLE_REACH: f32 = 8.0;

/// The eight handles of a frame: which way each resizes (see [`ViewGrip::Resize`]).
const HANDLES: [(i8, i8); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
];

/// `was` after a drag of `paper` millimetres by `grip`.
pub(crate) fn dragged_view(was: &SheetView, grip: ViewGrip, paper: DVec2) -> SheetView {
    let mut v = was.clone();
    match (grip, was.frame) {
        (ViewGrip::Pan, _) | (ViewGrip::Move, None) => v.center = was.center - paper * was.scale,
        (ViewGrip::Move, Some(f)) => {
            v.frame = Some(ViewFrame {
                at: f.at + paper,
                ..f
            });
        }
        (ViewGrip::Resize(..), None) => {}
        (ViewGrip::Resize(dx, dy), Some(f)) => {
            let b = f.bounds();
            let (mut min, mut max) = (b.min, b.max);
            // The dragged edges follow the pointer, never past the opposite ones.
            match dx {
                -1 => min.x = (min.x + paper.x).min(max.x - MIN_FRAME),
                1 => max.x = (max.x + paper.x).max(min.x + MIN_FRAME),
                _ => {}
            }
            match dy {
                -1 => min.y = (min.y + paper.y).min(max.y - MIN_FRAME),
                1 => max.y = (max.y + paper.y).max(min.y + MIN_FRAME),
                _ => {}
            }
            let at = (min + max) * 0.5;
            v.frame = Some(ViewFrame {
                at,
                size: max - min,
            });
            // The drawing stays where it is on the paper: the frame only crops it.
            v.center = was.center + (at - f.at) * was.scale;
        }
    }
    v
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
    pub(crate) fn update_sheet(&mut self, id: SheetId, sheet: Sheet, typing: bool) {
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
        self.files.export(&name, PDF_FILE, bytes);
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
        self.files.export(&name, SVG_FILE, text.into_bytes());
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
        self.files.export(&name, DXF_FILE, text.into_bytes());
    }

    /// Open the sheets as a PDF in the system viewer (a browser tab on the web) to print.
    pub(crate) fn print(&mut self, painter: &egui::Painter) {
        let bytes = self.pdf_bytes(painter);
        let name = format!("{}.pdf", self.file_stem());
        self.files.open_externally(&name, &bytes);
    }

    /// Add a DXF file to the document, as one undo step: its curves, text, hatches, blocks
    /// and layouts. What came in, and what couldn't, is listed in a report that stays up
    /// until it is closed.
    pub(crate) fn import_dxf(&mut self, name: &str, text: &str, now: f64) {
        let unit = self.editor.doc.units().length.mm_per_unit();
        match drift_io::dxf::read(text, unit) {
            Ok(import) => {
                self.abort_operation();
                self.editor.doc.begin();
                let report = drift_io::dxf::add_to_document(&mut self.editor.doc, &import);
                self.editor.commit("Import DXF");
                let mut came_in = Vec::new();
                let mut count = |n: usize, one: &str, many: &str| {
                    if n > 0 {
                        came_in.push(format!("{n} {}", if n == 1 { one } else { many }));
                    }
                };
                count(report.curves, "curve", "curves");
                count(report.texts, "line of text", "lines of text");
                count(report.fills, "hatch or fill", "hatches and fills");
                count(report.copies, "block copy", "block copies");
                count(report.blocks, "block", "blocks");
                count(report.dimensions, "dimension", "dimensions");
                count(report.layers, "new layer", "new layers");
                count(report.sheets, "sheet", "sheets");
                let summary = if came_in.is_empty() {
                    format!("Nothing to import in {name}")
                } else {
                    format!("Imported {} from {name}", came_in.join(", "))
                };
                let skipped: Vec<String> = report
                    .skipped
                    .iter()
                    .map(|(kind, n)| format!("{n} × {kind}"))
                    .collect();
                // Anything left out or read approximately is worth more than a passing
                // message.
                if !skipped.is_empty() || !report.notes.is_empty() {
                    self.import_report = Some(ImportSummary {
                        title: name.to_owned(),
                        summary: summary.clone(),
                        skipped,
                        notes: report.notes.clone(),
                    });
                }
                self.set_status(summary, false, now);
                self.fit_target = Some(self.fit_bounds());
            }
            Err(e) => self.set_status(format!("Could not import {name}: {e}"), true, now),
        }
    }

    /// The report of the last import, until it is closed.
    pub(crate) fn import_report_window(&mut self, ctx: &egui::Context) {
        let Some(report) = &self.import_report else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let mut check = false;
        egui::Window::new(format!("Import: {}", report.title))
            .id(egui::Id::new("import_report"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(&report.summary);
                if !report.skipped.is_empty() {
                    theme::section(ui, "Left out");
                    ui.label(
                        egui::RichText::new("Kinds of object Drift doesn't read yet:")
                            .small()
                            .weak(),
                    );
                    for line in &report.skipped {
                        ui.label(line);
                    }
                }
                if !report.notes.is_empty() {
                    theme::section(ui, "Read with changes");
                    for note in &report.notes {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("•");
                            ui.label(note);
                        });
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if ui
                        .button("Check sketch…")
                        .on_hover_text(
                            "Drawings from other programs often have ends that nearly meet: \
                             find and fix them",
                        )
                        .clicked()
                    {
                        check = true;
                        close = true;
                    }
                    if ui
                        .button("Copy")
                        .on_hover_text("Copy this report")
                        .clicked()
                    {
                        let mut text = report.summary.clone();
                        for line in report.skipped.iter().chain(&report.notes) {
                            text.push('\n');
                            text.push_str(line);
                        }
                        ui.ctx().copy_text(text);
                    }
                });
            });
        if check {
            self.open_sketch_check();
        }
        if close || !open {
            self.import_report = None;
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
        typing |= self.views_section(ui, &mut sheet);
        theme::section(ui, "Title block");
        egui::ComboBox::from_id_salt("title_block")
            .selected_text(sheet.title_block.label())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for t in TitleBlock::ALL {
                    ui.selectable_value(&mut sheet.title_block, t, t.label());
                }
            });
        theme::row(ui, "Logo", |ui| {
            let has = !self.editor.doc.logo().is_empty();
            if ui
                .button(if has { "Replace…" } else { "Choose SVG…" })
                .on_hover_text(
                    "A logo for every sheet's title block, from an SVG file (with its text \
                     converted to outlines). It is kept in the drawing's file.",
                )
                .clicked()
            {
                self.files.open(SVG_FILE);
            }
            if has && ui.button("Remove").clicked() {
                self.editor.doc.begin();
                self.editor.doc.set_logo(String::new());
                self.editor.commit("Remove logo");
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
        match self.pending_view.take() {
            Some(ViewAction::Add) => {
                self.add_view(id);
                self.sheet_preview = true;
            }
            Some(ViewAction::Remove) => self.remove_view(id),
            None => {}
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
            let (pan, factor) = self.wheel.read(ui);
            let factor = factor as f32;
            self.preview.center -=
                DVec2::new(f64::from(pan.x), -f64::from(pan.y)) / f64::from(self.preview.zoom);
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
        self.view_input(ui, &response, rect, id, &sheet);

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
        self.paint_view_frames(&painter, rect, &sheet, ui.visuals().dark_mode);
        // What this is, and how to leave.
        let info = format!(
            "{} · {} {} · {}  —  drag a view to move it (Alt: the drawing inside it) · Esc \
             returns to the model",
            sheet.name,
            sheet.paper.label(),
            if sheet.portrait {
                "portrait"
            } else {
                "landscape"
            },
            sheet.scale_text()
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

    /// The view of `sheet` picked in the sheet editor and the Sheets panel.
    pub(crate) fn current_view(&self, sheet: &Sheet) -> usize {
        self.sheet_view.min(sheet.views.len().saturating_sub(1))
    }

    /// A view's frame on screen.
    fn view_rect(&self, rect: egui::Rect, sheet: &Sheet, view: &SheetView) -> egui::Rect {
        let b = drift_draft::sheet::view_frame(sheet, view);
        egui::Rect::from_two_pos(
            self.paper_to_preview(rect, b.min),
            self.paper_to_preview(rect, b.max),
        )
    }

    /// Where a frame's handles are on screen, with what each one resizes.
    fn view_handles(frame: egui::Rect) -> [(egui::Pos2, (i8, i8)); 8] {
        HANDLES.map(|(dx, dy)| {
            let x = match dx {
                -1 => frame.left(),
                1 => frame.right(),
                _ => frame.center().x,
            };
            // Paper y is up: the lower edge is at the bottom of the screen.
            let y = match dy {
                -1 => frame.bottom(),
                1 => frame.top(),
                _ => frame.center().y,
            };
            (egui::pos2(x, y), (dx, dy))
        })
    }

    /// What a press at `pos` would take hold of: a handle of the picked view, else the view
    /// under the pointer (one with a frame before one that fills the sheet).
    pub(crate) fn view_grip_at(
        &self,
        rect: egui::Rect,
        sheet: &Sheet,
        pos: egui::Pos2,
        alt: bool,
    ) -> Option<(usize, ViewGrip)> {
        let picked = self.current_view(sheet);
        if let Some(v) = sheet.views.get(picked).filter(|v| v.frame.is_some()) {
            let frame = self.view_rect(rect, sheet, v);
            if let Some((_, (dx, dy))) = Self::view_handles(frame)
                .into_iter()
                .find(|(at, _)| at.distance(pos) <= HANDLE_REACH)
            {
                return Some((picked, ViewGrip::Resize(dx, dy)));
            }
        }
        let under = |framed: bool| {
            sheet.views.iter().enumerate().rev().find(|(_, v)| {
                v.frame.is_some() == framed && self.view_rect(rect, sheet, v).contains(pos)
            })
        };
        let (index, view) = under(true).or_else(|| under(false))?;
        let grip = if alt || view.frame.is_none() {
            ViewGrip::Pan
        } else {
            ViewGrip::Move
        };
        Some((index, grip))
    }

    /// Pick, move, resize and pan the sheet's views with the primary button.
    fn view_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        id: SheetId,
        sheet: &Sheet,
    ) {
        let alt = ui.input(|i| i.modifiers.alt);
        if response.drag_started_by(PointerButton::Primary)
            && let Some(from) = ui.input(|i| i.pointer.press_origin())
            && let Some((index, grip)) = self.view_grip_at(rect, sheet, from, alt)
        {
            self.sheet_view = index;
            self.view_drag = Some(ViewDrag {
                sheet: id,
                index,
                grip,
                was: sheet.views[index].clone(),
                from,
            });
            self.editor.doc.begin();
        }
        if let Some(drag) = self.view_drag.clone() {
            if let Some(pos) = response.interact_pointer_pos()
                && drag.index < sheet.views.len()
            {
                let d = pos - drag.from;
                let paper =
                    DVec2::new(f64::from(d.x), -f64::from(d.y)) / f64::from(self.preview.zoom);
                let mut moved = sheet.clone();
                moved.views[drag.index] = dragged_view(&drag.was, drag.grip, paper);
                self.editor.doc.set_sheet(drag.sheet, moved);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.view_drag = None;
                self.editor.commit(match drag.grip {
                    ViewGrip::Pan => "Move sheet view",
                    ViewGrip::Move => "Move view on sheet",
                    ViewGrip::Resize(..) => "Resize view",
                });
            }
        } else if response.clicked_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((index, _)) = self.view_grip_at(rect, sheet, pos, false)
        {
            self.sheet_view = index;
        }
        if response.hovered() {
            let grip = self.view_drag.as_ref().map(|d| d.grip).or_else(|| {
                response
                    .hover_pos()
                    .and_then(|p| self.view_grip_at(rect, sheet, p, alt))
                    .map(|(_, g)| g)
            });
            let dragging = self.view_drag.is_some();
            ui.ctx().set_cursor_icon(match grip {
                Some(ViewGrip::Resize(0, _)) => egui::CursorIcon::ResizeVertical,
                Some(ViewGrip::Resize(_, 0)) => egui::CursorIcon::ResizeHorizontal,
                Some(ViewGrip::Resize(dx, dy)) if dx == dy => egui::CursorIcon::ResizeNeSw,
                Some(ViewGrip::Resize(..)) => egui::CursorIcon::ResizeNwSe,
                Some(ViewGrip::Move) => egui::CursorIcon::Move,
                Some(ViewGrip::Pan) if dragging => egui::CursorIcon::Grabbing,
                Some(ViewGrip::Pan) => egui::CursorIcon::Grab,
                None => egui::CursorIcon::Default,
            });
        }
    }

    /// The editor's marks over the page (they don't print): the picked view's frame and
    /// handles, and the frame of the one under the pointer.
    fn paint_view_frames(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        sheet: &Sheet,
        dark: bool,
    ) {
        let accent = theme::accent(dark);
        let picked = self.current_view(sheet);
        let hover = painter
            .ctx()
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .and_then(|p| self.view_grip_at(rect, sheet, p, false))
            .map(|(i, _)| i);
        for (i, view) in sheet.views.iter().enumerate() {
            let frame = self.view_rect(rect, sheet, view);
            // With one view filling the sheet there is nothing to tell apart.
            if sheet.views.len() == 1 && view.frame.is_none() {
                continue;
            }
            if i == picked {
                painter.rect_stroke(
                    frame,
                    0.0,
                    egui::Stroke::new(1.5, accent),
                    egui::StrokeKind::Outside,
                );
                if view.frame.is_some() {
                    for (at, _) in Self::view_handles(frame) {
                        let knob = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
                        painter.rect_filled(knob, 1.0, egui::Color32::WHITE);
                        painter.rect_stroke(
                            knob,
                            1.0,
                            egui::Stroke::new(1.5, accent),
                            egui::StrokeKind::Middle,
                        );
                    }
                }
            } else {
                let alpha = if hover == Some(i) { 0.9 } else { 0.35 };
                painter.rect_stroke(
                    frame,
                    0.0,
                    egui::Stroke::new(1.0, accent.gamma_multiply(alpha)),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }

    /// Add a view to a sheet: of the selection if there is one, else of the whole sketch, at
    /// the scale of the views already there. Returns its index.
    pub(crate) fn add_view(&mut self, id: SheetId) -> Option<usize> {
        let mut sheet = self.editor.doc.sheet(id)?.clone();
        let ed = &self.editor;
        let picked: Bounds = ed
            .selection
            .iter()
            .filter_map(|e| ed.doc.shape(*e))
            .map(|s| s.bounds())
            .collect();
        let bounds = if picked.is_empty() || picked.min == picked.max {
            self.fit_bounds()
        } else {
            picked
        };
        if bounds.is_empty() {
            return None;
        }
        let scale = sheet.views.first().map_or(1.0, |v| v.scale);
        let mut view = crate::output::framed_view(bounds, &sheet, scale);
        view.name = format!("View {}", sheet.views.len() + 1);
        sheet.views.push(view);
        let index = sheet.views.len() - 1;
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Add view");
        self.sheet_view = index;
        Some(index)
    }

    /// Remove the picked view (a sheet keeps at least one).
    pub(crate) fn remove_view(&mut self, id: SheetId) {
        let Some(mut sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        if sheet.views.len() < 2 {
            return;
        }
        let index = self.current_view(&sheet);
        sheet.views.remove(index);
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Remove view");
        self.sheet_view = index.saturating_sub(1);
    }

    /// The Sheets panel's Views section: the sheet's views, and the picked one's settings.
    /// Returns true if text was typed (so the edits merge into one undo step).
    fn views_section(&mut self, ui: &mut egui::Ui, sheet: &mut Sheet) -> bool {
        let mut typing = false;
        theme::section(ui, "Views");
        let picked = self.current_view(sheet);
        let several = sheet.views.len() > 1;
        if several {
            for (i, v) in sheet.views.iter().enumerate() {
                let name = if v.name.trim().is_empty() {
                    "View"
                } else {
                    v.name.trim()
                };
                let text = format!("{}  {name}  ·  {}", i + 1, scale_label(v.scale));
                if ui
                    .add_sized(
                        [ui.available_width(), 22.0],
                        egui::Button::selectable(picked == i, text),
                    )
                    .clicked()
                {
                    self.sheet_view = i;
                }
            }
        }
        let picked = self.current_view(sheet);
        let bounds = self.fit_bounds();
        let area = drift_draft::sheet::view_area(sheet);
        let Some(view) = sheet.views.get_mut(picked) else {
            return false;
        };
        let mut fit = false;
        theme::row(ui, "Title", |ui| {
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut view.name)
                        .hint_text("Plan, Detail A…")
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
        });
        theme::row(ui, "Scale", |ui| {
            egui::ComboBox::from_id_salt("sheet_scale")
                .selected_text(scale_label(view.scale))
                .width(90.0)
                .show_ui(ui, |ui| {
                    for n in PREFERRED_SCALES {
                        ui.selectable_value(&mut view.scale, n, scale_label(n));
                    }
                });
            fit = ui
                .button("Fit")
                .on_hover_text("Centre the drawing in the view at the largest scale that fits")
                .clicked();
        });
        theme::row(ui, "Frame", |ui| {
            let framed = view.frame.is_some();
            if let Some(framed) =
                theme::segmented(ui, framed, &[(false, "Fills sheet"), (true, "Framed")])
            {
                view.frame = framed.then(|| {
                    // A frame over the middle of the sheet, to drag and crop from there.
                    let size = area.size() * 0.6;
                    ViewFrame {
                        at: area.center(),
                        size,
                    }
                });
            }
        });
        theme::row(ui, "Show", |ui| {
            ui.checkbox(&mut view.dimensions, "Dimensions")
                .on_hover_text("Dimensions and ordinates, in this view");
            ui.checkbox(&mut view.notes, "Notes")
                .on_hover_text("Notes and callouts, in this view");
        });
        theme::row(ui, "", |ui| {
            ui.checkbox(&mut view.title, "Title")
                .on_hover_text("Its title and scale, written under it");
            ui.add_enabled(
                view.frame.is_some(),
                egui::Checkbox::new(&mut view.border, "Border"),
            )
            .on_hover_text("Print the frame's outline");
        });
        if fit {
            crate::output::fit_view(bounds, sheet, picked);
        }
        ui.label(
            egui::RichText::new(
                "On the sheet: drag a view to move it, its handles to crop it, and Alt-drag to \
                 slide the drawing inside it.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            if ui
                .button("Add view")
                .on_hover_text(
                    "A new view in its own frame: of the selection in the model if there is \
                     one, else of the whole drawing",
                )
                .clicked()
            {
                self.pending_view = Some(ViewAction::Add);
            }
            if several && ui.button("Remove view").clicked() {
                self.pending_view = Some(ViewAction::Remove);
            }
        });
        typing
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
