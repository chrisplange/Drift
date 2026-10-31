//! Sheets: the Sheets tab, the sheet preview on the canvas, PDF / SVG / DXF export, printing
//! and DXF import.

use drift_core::sheets::{ARCHITECTURAL_SCALES, ENGINEERING_SCALES, PREFERRED_SCALES, scale_label};
use drift_core::{
    PaperSize, PlacedView, Revision, Sheet, SheetColours, SheetId, TitleBlock, ViewFrame, ViewId,
    Viewport,
};
#[cfg(not(feature = "demo"))]
use drift_draft::page::Drawing;
use drift_platform::FileType;

use crate::*;

#[cfg(not(feature = "demo"))]
pub(crate) const PDF_FILE: FileType = FileType {
    name: "PDF",
    extensions: &["pdf"],
};
pub(crate) const SVG_FILE: FileType = FileType {
    name: "SVG",
    extensions: &["svg"],
};
#[cfg(not(feature = "demo"))]
pub(crate) const DXF_FILE: FileType = FileType {
    name: "DXF",
    extensions: &["dxf"],
};
#[cfg(not(feature = "demo"))]
pub(crate) const DWG_FILE: FileType = FileType {
    name: "DWG",
    extensions: &["dwg"],
};
#[cfg(not(feature = "demo"))]
pub(crate) const PROFILES_FILE: FileType = FileType {
    name: "Drift profiles for Blender",
    extensions: &["json"],
};
/// What an import brought in and what it couldn't, shown until closed.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImportSummary {
    pub title: String,
    pub summary: String,
    /// Kinds of object left out, with counts.
    pub skipped: Vec<String>,
    /// What was read approximately, in sentences.
    pub notes: Vec<String>,
    /// Geometry that came in far off (see [`drift_core::FAR_MM`]), to pick.
    pub far: Vec<drift_core::EntityId>,
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
    /// Nothing: a view that fills the drawing area stays put (Alt-drag slides its drawing).
    Fixed,
}

/// A drag on a view in progress: the view as it was, and the pointer, when it started.
#[derive(Clone, Debug)]
pub(crate) struct ViewDrag {
    sheet: SheetId,
    index: usize,
    grip: ViewGrip,
    was: PlacedView,
    from: egui::Pos2,
    /// The other picked views moving with it (by index, as they were).
    group: Vec<(usize, PlacedView)>,
}

/// How far `paper` may move views whose frames span `frames` and keep them all inside the
/// sheet's drawing area (a set wider than the area keeps to its lower side).
fn contain_set(frames: drift_core::Bounds, sheet: &Sheet, paper: DVec2) -> DVec2 {
    let area = drift_draft::sheet::drawing_area(sheet);
    let mut paper = paper;
    for k in 0..2 {
        let (lo, hi) = (area.min[k] - frames.min[k], area.max[k] - frames.max[k]);
        paper[k] = if lo <= hi { paper[k].clamp(lo, hi) } else { lo };
    }
    paper
}

/// The smallest a view's frame can be made (paper mm), and how near a handle counts (points).
const MIN_FRAME: f64 = 10.0;
const HANDLE_REACH: f32 = 8.0;
/// What a view's Construction switch does.
const CONSTRUCTION_TIP: &str = "Construction geometry, as thin dashed lines, in this view on \
                                sheets (points and an import's fill edges stay out)";

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
pub(crate) fn dragged_view(was: &PlacedView, grip: ViewGrip, paper: DVec2) -> PlacedView {
    let mut v = was.clone();
    let f = was.frame;
    match grip {
        ViewGrip::Fixed => {}
        ViewGrip::Pan => v.center = was.center - paper * was.scale,
        ViewGrip::Move => v.frame.at = f.at + paper,
        ViewGrip::Resize(dx, dy) => {
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
            v.frame = ViewFrame {
                at,
                size: max - min,
            };
            // The drawing stays where it is on the paper: the frame only crops it.
            v.center = was.center + (at - f.at) * was.scale;
        }
    }
    v
}

/// Keep a moved or resized view's frame inside the sheet's drawing area: a moved one slides
/// back in, a resized one stops at the edge.
fn contain(view: &mut PlacedView, sheet: &Sheet, grip: ViewGrip) {
    let area = drift_draft::sheet::drawing_area(sheet);
    let f = view.frame;
    let b = f.bounds();
    let (min, max) = match grip {
        ViewGrip::Move => {
            let size = f.size.min(area.size());
            let at = f.at.max(area.min + size * 0.5).min(area.max - size * 0.5);
            (at - size * 0.5, at + size * 0.5)
        }
        ViewGrip::Resize(..) => (b.min.max(area.min), b.max.min(area.max)),
        _ => return,
    };
    let at = (min + max) * 0.5;
    // A crop leaves the drawing where it is on the paper; a move takes it along.
    if matches!(grip, ViewGrip::Resize(..)) {
        view.center += (at - f.at) * view.scale;
    }
    view.frame = ViewFrame {
        at,
        size: max - min,
    };
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

/// What the output dialog makes.
#[cfg(not(feature = "demo"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Output {
    Pdf,
    Print,
}

impl DriftApp {
    /// The file name without its extension, for exports.
    pub(crate) fn file_stem(&self) -> String {
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

    /// A new sheet titled after the file.
    fn fresh_sheet(&self, name: &str) -> Sheet {
        let mut sheet = Sheet::new(name);
        let last = self
            .editor
            .doc
            .sheets()
            .last()
            .map(|(_, s)| s.fields.number.clone());
        sheet.fields.number = crate::app::sheet_editor::next_sheet_number(last.as_deref());
        // The title is the project's; with no project name yet, the file's.
        if self.editor.doc.project().name.trim().is_empty() {
            sheet.fields.title = self.file_stem();
        }
        sheet.fields.date = today();
        sheet
    }

    /// `View n`, for the first `n` no view is called.
    pub(crate) fn next_view_name(&self) -> String {
        drift_edit::tools::view::next_view_name(&self.editor.doc)
    }

    /// A new, empty sheet: views go on it from the Views list or with V (Add view).
    pub(crate) fn add_sheet(&mut self) -> SheetId {
        let n = self.editor.doc.sheets().count() + 1;
        let mut sheet = self.fresh_sheet(&format!("Sheet {n}"));
        self.editor.doc.begin();
        // The first sheet names the project after the file: every sheet's title.
        if !sheet.fields.title.is_empty() {
            let mut project = self.editor.doc.project().clone();
            project.name = std::mem::take(&mut sheet.fields.title);
            self.editor.doc.set_project(project);
        }
        let id = self.editor.doc.add_sheet(sheet);
        self.editor.commit("New sheet");
        self.active_sheet = Some(id);
        self.sheet_view = 0;
        id
    }

    /// A new view of the whole drawing on sheet `id`, filling the space clear of the title
    /// block, picked: one undo step.
    pub(crate) fn add_whole_view(&mut self, id: SheetId) {
        let Some(mut sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let (mut view, mut port) =
            crate::output::sheet_view(self.fit_bounds(), &sheet, self.imperial());
        view.name = self.next_view_name();
        self.editor.doc.begin();
        let added = self.editor.doc.add_view(view);
        port.view = added;
        sheet.viewports.push(port);
        let index = sheet.viewports.len() - 1;
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Add view");
        self.sheet_view = index;
        self.picked_view = Some(added);
        self.view_set = None;
        self.sheet_pick = crate::app::sheet_editor::SheetPick::View;
    }

    /// Change a sheet, and the picked view on it (`view`: its index and its new state).
    /// Typing into their fields merges into one undo step.
    pub(crate) fn update_sheet(
        &mut self,
        id: SheetId,
        sheet: Sheet,
        view: Option<(usize, PlacedView)>,
        typing: bool,
    ) {
        let doc = &self.editor.doc;
        let view = view.filter(|(i, v)| doc.placed_views(&sheet).get(*i) != Some(v));
        if doc.sheet(id) == Some(&sheet) && view.is_none() {
            return;
        }
        let label = if typing {
            "Edit sheet text"
        } else {
            "Edit sheet"
        };
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        if let Some((i, v)) = view {
            self.editor.doc.set_placed_view(id, i, &v);
        }
        let tx = self.editor.doc.commit(label);
        if typing && self.editor.history.undo_label() == Some(label) {
            self.editor.history.merge_into_last(tx);
        } else {
            self.editor.history.push(tx);
        }
        self.editor.sync();
    }

    /// Every sheet as a page (or, with no sheets, a fitted A3 sheet).
    #[cfg(not(feature = "demo"))]
    pub(crate) fn pages(&mut self, painter: &egui::Painter) -> Vec<Drawing> {
        let doc = &self.editor.doc;
        let mut sheets: Vec<(Sheet, Vec<PlacedView>)> = doc
            .sheets()
            .map(|(_, s)| (s.clone(), doc.placed_views(s)))
            .collect();
        if sheets.is_empty() {
            let sheet = self.fresh_sheet("Sheet 1");
            let (view, port) =
                crate::output::sheet_view(self.fit_bounds(), &sheet, self.imperial());
            let placed = PlacedView::new(port.view, &view, &port);
            sheets.push((sheet, vec![placed]));
        }
        let count = sheets.len();
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        sheets
            .iter()
            .enumerate()
            .map(|(i, (s, views))| {
                crate::output::sheet_drawing(
                    &mut self.editor,
                    &self.hatch_patterns,
                    painter,
                    s,
                    views,
                    (i + 1, count),
                )
            })
            .collect()
    }

    #[cfg(not(feature = "demo"))]
    /// The sheets chosen for the PDF and print, as a PDF. Each keeps its number in the whole
    /// set ("2 / 5"), so a reissued sheet matches the set it belongs to.
    pub(crate) fn pdf_bytes(&mut self, painter: &egui::Painter) -> Vec<u8> {
        let chosen: Vec<bool> = self
            .editor
            .doc
            .sheets()
            .map(|(id, _)| !self.pdf_left_out.contains(&id))
            .collect();
        let mut pages = self.pages(painter);
        if chosen.contains(&true) && chosen.len() == pages.len() {
            let mut keep = chosen.into_iter();
            pages.retain(|_| keep.next().unwrap_or(true));
        }
        drift_io::pdf::write(&pages, &self.file_stem())
    }

    #[cfg(not(feature = "demo"))]
    /// Export a PDF: with several sheets, first a dialog to choose which.
    pub(crate) fn export_pdf(&mut self, painter: &egui::Painter) {
        if self.editor.doc.sheets().nth(1).is_some() {
            self.output_dialog = Some(Output::Pdf);
        } else {
            self.write_pdf(painter);
        }
    }

    #[cfg(not(feature = "demo"))]
    fn write_pdf(&mut self, painter: &egui::Painter) {
        let bytes = self.pdf_bytes(painter);
        let name = format!("{}.pdf", self.file_stem());
        self.files.export(&name, PDF_FILE, bytes);
    }

    #[cfg(not(feature = "demo"))]
    /// The current sheet as SVG.
    pub(crate) fn svg_text(&mut self, painter: &egui::Painter) -> String {
        let pages = self.pages(painter);
        let index = self
            .current_sheet()
            .and_then(|id| self.editor.doc.sheets().position(|(s, _)| s == id))
            .unwrap_or(0);
        drift_io::svg::write(&pages[index.min(pages.len() - 1)])
    }

    #[cfg(not(feature = "demo"))]
    pub(crate) fn export_svg(&mut self, painter: &egui::Painter) {
        let text = self.svg_text(painter);
        let name = format!("{}.svg", self.file_stem());
        self.files.export(&name, SVG_FILE, text.into_bytes());
    }

    /// The model at true size on a page fitted to it: the selection, or (with nothing picked)
    /// everything that prints. `None` when there is nothing to draw.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn model_page(&mut self, painter: &egui::Painter) -> Option<Drawing> {
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        let only = self.model_scope();
        let d = crate::output::model_page(
            &mut self.editor,
            &self.hatch_patterns,
            painter,
            only.as_ref(),
            self.model_annotations,
        );
        (!d.items.is_empty()).then_some(d)
    }

    #[cfg(not(feature = "demo"))]
    /// Export the model (or the selection) at 1 : 1 as SVG or PDF.
    pub(crate) fn export_model(&mut self, painter: &egui::Painter, pdf: bool) {
        let Some(d) = self.model_page(painter) else {
            self.status_next = Some("Nothing to export: no geometry that prints".into());
            return;
        };
        let stem = self.file_stem();
        if pdf {
            let bytes = drift_io::pdf::write(&[d], &stem);
            self.files.export(&format!("{stem}.pdf"), PDF_FILE, bytes);
        } else {
            let text = drift_io::svg::write(&d);
            self.files
                .export(&format!("{stem}.svg"), SVG_FILE, text.into_bytes());
        }
    }

    /// The model as DXF R12 text (for tests).
    #[cfg(all(test, not(feature = "demo")))]
    pub(crate) fn dxf_text(&mut self, painter: &egui::Painter) -> String {
        let bytes = self
            .cad_bytes(painter, crate::output::CadFormat::DxfR12)
            .unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
    }

    /// The model at full size as a CAD file (see [`crate::output::cad_file`]).
    #[cfg(not(feature = "demo"))]
    pub(crate) fn cad_bytes(
        &mut self,
        painter: &egui::Painter,
        format: crate::output::CadFormat,
    ) -> Result<Vec<u8>, String> {
        self.hatch_patterns
            .update(self.editor.doc.custom_patterns());
        crate::output::cad_file(&mut self.editor, &self.hatch_patterns, painter, format)
    }

    /// Export DWG / DXF: first the dialog choosing the format.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn export_dxf(&mut self, _: &egui::Painter) {
        self.cad_dialog = true;
    }

    #[cfg(not(feature = "demo"))]
    fn write_cad(&mut self, painter: &egui::Painter, format: crate::output::CadFormat) {
        match self.cad_bytes(painter, format) {
            Ok(bytes) => {
                let name = format!("{}.{}", self.file_stem(), format.extension());
                let kind = match format {
                    crate::output::CadFormat::Dwg => DWG_FILE,
                    _ => DXF_FILE,
                };
                self.files.export(&name, kind, bytes);
            }
            Err(e) => self.status_next = Some(format!("Couldn't export: {e}")),
        }
    }

    /// The Export DWG / DXF dialog: which format, then the file.
    #[cfg(not(feature = "demo"))]
    pub(crate) fn cad_window(&mut self, ctx: &egui::Context) {
        use crate::output::CadFormat;
        if !self.cad_dialog {
            return;
        }
        let title = "Export DWG / DXF";
        let dark = ctx.global_style().visuals.dark_mode;
        let (mut close, mut go) = (false, false);
        theme::dialog(ctx, title)
            .id(egui::Id::new("cad_export"))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                if theme::dialog_title(ui, title, true) {
                    close = true;
                }
                ui.label(
                    egui::RichText::new(
                        "The model at full size, in the drawing's units, with its layers.",
                    )
                    .weak(),
                );
                ui.add_space(6.0);
                for format in CadFormat::ALL {
                    #[cfg(not(feature = "dwg"))]
                    if format != CadFormat::DxfR12 {
                        continue;
                    }
                    ui.radio_value(&mut self.cad_format, format, format.label());
                    ui.indent(format.extension(), |ui| {
                        ui.label(egui::RichText::new(format.hint()).small().weak());
                    });
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui.add(theme::primary_button("Export…", dark)).clicked() {
                        go = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if go {
            self.cad_dialog = false;
            let painter = ctx.layer_painter(egui::LayerId::background());
            self.write_cad(&painter, self.cad_format);
        } else if close {
            self.cad_dialog = false;
        }
    }

    #[cfg(not(feature = "demo"))]
    /// Print: with several sheets, first a dialog to choose which.
    pub(crate) fn print(&mut self, painter: &egui::Painter) {
        if self.editor.doc.sheets().nth(1).is_some() {
            self.output_dialog = Some(Output::Print);
        } else {
            self.open_to_print(painter);
        }
    }

    #[cfg(not(feature = "demo"))]
    /// Choose the sheets for a PDF or print, then make it.
    pub(crate) fn output_window(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.output_dialog else {
            return;
        };
        let title = match kind {
            Output::Pdf => "Export PDF",
            Output::Print => "Print",
        };
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .doc
            .sheets()
            .map(|(id, s)| (id, crate::app::sheet_editor::sheet_label(s)))
            .collect();
        let current = self.current_sheet();
        let dark = ctx.global_style().visuals.dark_mode;
        let (mut close, mut go) = (false, false);
        theme::dialog(ctx, title)
            .id(egui::Id::new("output_sheets"))
            .show(ctx, |ui| {
                ui.set_width(340.0);
                if theme::dialog_title(ui, title, true) {
                    close = true;
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Sheets").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(c) = current
                            && ui.link("This sheet").clicked()
                        {
                            self.pdf_left_out = sheets
                                .iter()
                                .map(|(id, _)| *id)
                                .filter(|id| *id != c)
                                .collect();
                        }
                        if ui.link("All").clicked() {
                            self.pdf_left_out.clear();
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (id, label) in &sheets {
                            let mut on = !self.pdf_left_out.contains(id);
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    self.pdf_left_out.remove(id);
                                } else {
                                    self.pdf_left_out.insert(*id);
                                }
                            }
                        }
                    });
                let chosen = sheets
                    .iter()
                    .filter(|(id, _)| !self.pdf_left_out.contains(id))
                    .count();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{chosen} of {} sheets. Each keeps its number in the set.",
                        sheets.len()
                    ))
                    .small()
                    .weak(),
                );
                theme::dialog_buttons(ui, |ui| {
                    let label = match kind {
                        Output::Pdf => "Export…",
                        Output::Print => "Print…",
                    };
                    if ui
                        .add_enabled(chosen > 0, theme::primary_button(label, dark))
                        .clicked()
                    {
                        go = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if go {
            self.output_dialog = None;
            let painter = ctx.layer_painter(egui::LayerId::background());
            match kind {
                Output::Pdf => self.write_pdf(&painter),
                Output::Print => self.open_to_print(&painter),
            }
        } else if close {
            self.output_dialog = None;
        }
    }

    #[cfg(not(feature = "demo"))]
    /// Open the chosen sheets as a PDF in the system viewer (a browser tab on the web) to
    /// print.
    fn open_to_print(&mut self, painter: &egui::Painter) {
        let bytes = self.pdf_bytes(painter);
        let name = format!("{}.pdf", self.file_stem());
        self.files.open_externally(&name, &bytes);
    }

    // The demo's stand-ins: a note where the desktop app writes a file.
    #[cfg(feature = "demo")]
    pub(crate) fn export_pdf(&mut self, _: &egui::Painter) {
        self.not_in_demo("Exporting a PDF");
    }

    #[cfg(feature = "demo")]
    pub(crate) fn export_svg(&mut self, _: &egui::Painter) {
        self.not_in_demo("Exporting an SVG");
    }

    #[cfg(feature = "demo")]
    pub(crate) fn export_dxf(&mut self, _: &egui::Painter) {
        self.not_in_demo("Exporting DWG or DXF");
    }

    #[cfg(feature = "demo")]
    pub(crate) fn export_model(&mut self, _: &egui::Painter, pdf: bool) {
        self.not_in_demo(if pdf {
            "Exporting a PDF"
        } else {
            "Exporting an SVG"
        });
    }

    #[cfg(feature = "demo")]
    pub(crate) fn print(&mut self, _: &egui::Painter) {
        self.not_in_demo("Printing");
    }

    /// Say what an import brought in; what it left out or changed stays up in a report.
    pub(crate) fn report_import(
        &mut self,
        name: &str,
        report: &drift_io::dxf::ImportReport,
        now: f64,
    ) {
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
        // What the import's commit put far off: listed here, not in the status bar.
        let far = std::mem::take(&mut self.editor.out.far_off);
        // Anything left out, read approximately or far off is worth more than a passing
        // message.
        if !skipped.is_empty() || !report.notes.is_empty() || !far.is_empty() {
            self.import_report = Some(ImportSummary {
                title: name.to_owned(),
                summary: summary.clone(),
                skipped,
                notes: report.notes.clone(),
                far,
            });
        }
        self.set_status(summary, false, now);
        self.fit_target = Some(self.fit_bounds());
    }

    /// The report of the last import, until it is closed.
    pub(crate) fn import_report_window(&mut self, ctx: &egui::Context) {
        let Some(report) = &self.import_report else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let mut check = false;
        let mut pick_far = false;
        let far_count =
            (!report.far.is_empty()).then(|| checker::far_off_count(&self.editor.doc, &report.far));
        let title = format!("Imported {}", report.title);
        theme::dialog(ctx, &title)
            .id(egui::Id::new("import_report"))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                if theme::dialog_title(ui, &title, true) {
                    close = true;
                }
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
                if let Some(what) = &far_count {
                    theme::section(ui, "Far off");
                    ui.label(format!(
                        "More than 100,000 km from the origin: {what}. Usually stray objects \
                         in the file; they make fitting the view show the drawing as a dot."
                    ));
                    if ui
                        .button("Select them")
                        .on_hover_text("Pick them, to look at or delete")
                        .clicked()
                    {
                        pick_far = true;
                        close = true;
                    }
                }
                theme::dialog_buttons(ui, |ui| {
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
        if pick_far {
            self.editor.selection = report.far.iter().copied().collect();
        }
        if check {
            self.open_sketch_check();
        }
        if close || !open {
            self.import_report = None;
        }
    }

    // ----- The Sheets tab --------------------------------------------------------------------

    pub(crate) fn sheets_panel(&mut self, ui: &mut egui::Ui) {
        let output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.sheets_contents(ui));
        self.panel_scrolled(ui, crate::dock::Panel::Sheets, &output);
    }

    fn sheets_contents(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Sheets");
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .doc
            .sheets()
            .map(|(id, s)| (id, crate::app::sheet_editor::sheet_label(s)))
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
        // Rows drag to reorder (a bar shows where one would go); right-click for more.
        use crate::app::sheet_editor::{SheetMenu, drop_index, gap_at, sheet_menu};
        let mut spots = Vec::new();
        let (mut dragging, mut dropped, mut chose) = (None, None, None);
        for (k, (id, label)) in sheets.iter().enumerate() {
            let shown = self.sheet_preview && current == Some(*id);
            let row = theme::list_row(ui, shown, label).interact(egui::Sense::drag());
            spots.push((*id, row.rect.top(), row.rect.bottom()));
            if row.dragged() {
                dragging = Some(*id);
            }
            if row.drag_stopped() {
                dropped = Some(*id);
            }
            if row.clicked() {
                self.show_sheet(Some(*id));
                self.show_panel(crate::dock::Panel::Inspector);
            }
            row.on_hover_text("Drag to reorder; right-click for more")
                .context_menu(|ui| {
                    if let Some(m) = sheet_menu(ui, k, sheets.len()) {
                        chose = Some((*id, m));
                    }
                });
        }
        if let (Some(id), Some(p)) = (dragging.or(dropped), ui.ctx().pointer_interact_pos()) {
            let to = drop_index(&spots, id, p.y);
            if dragging.is_some() {
                let y = gap_at(&spots, id, to);
                let r = ui.min_rect();
                ui.painter().hline(
                    r.x_range(),
                    y,
                    egui::Stroke::new(2.0, theme::accent(ui.visuals().dark_mode)),
                );
            }
            if dropped.is_some() {
                chose = Some((id, SheetMenu::Move(to)));
            }
        }
        match chose {
            Some((id, SheetMenu::Duplicate(views))) => self.duplicate_sheet(id, views),
            Some((id, SheetMenu::Move(to))) => self.move_sheet(id, to),
            Some((id, SheetMenu::Delete)) => self.delete_sheet(id),
            None => {}
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("New sheet").clicked() {
                self.new_sheet();
            }
            if let Some(id) = current
                && ui
                    .button("Add view")
                    .on_hover_text(
                        "Drag a rectangle round what the view shows, in the model; it goes on \
                         the sheet",
                    )
                    .clicked()
            {
                self.start_add_view(id);
            }
        });
        ui.label(
            egui::RichText::new(
                "The tabs over the canvas switch between the model and each sheet. Pick a view \
                 or the paper there to set it up in the Inspector.",
            )
            .small()
            .weak(),
        );
        self.model_views_section(ui, current);
        theme::section(ui, "Output");
        self.output_buttons(ui);
    }

    /// Drawing settings → Project: what every sheet's title block shares. Typing merges into
    /// one undo step.
    pub(crate) fn project_section(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Project");
        ui.label(
            egui::RichText::new(
                "Shared by every sheet's title block. A sheet's own title or company, set in \
                 its Inspector, wins.",
            )
            .small()
            .weak(),
        );
        ui.add_space(2.0);
        let before = self.editor.doc.project().clone();
        let mut p = before.clone();
        let mut typing = false;
        for (label, value, hint) in [
            ("Name", &mut p.name, "Riverside house"),
            ("Number", &mut p.number, "2026-014"),
            ("Client", &mut p.client, ""),
            ("Address", &mut p.address, ""),
            ("Company", &mut p.company, "Your practice"),
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
        ui.add_space(4.0);
        ui.label("Title block fields");
        let sets = drift_core::FieldSet::ALL.map(|f| (f, f.label()));
        if let Some(f) = theme::segmented(ui, p.fields, &sets) {
            p.fields = f;
        }
        ui.label(
            egui::RichText::new(match p.fields {
                drift_core::FieldSet::Mechanical => "For parts: each sheet's material.",
                drift_core::FieldSet::Architectural => {
                    "For buildings: client, address, project number, and each sheet's status."
                }
            })
            .small()
            .weak(),
        );
        if p != before {
            let label = if typing {
                "Edit project text"
            } else {
                "Edit project"
            };
            self.editor.doc.begin();
            self.editor.doc.set_project(p);
            let tx = self.editor.doc.commit(label);
            if typing && self.editor.history.undo_label() == Some(label) {
                self.editor.history.merge_into_last(tx);
            } else {
                self.editor.history.push(tx);
            }
            self.editor.sync();
        }
    }

    /// The Inspector's settings for a sheet itself: its number and name, paper, title block
    /// and revisions. Returns true if text was typed (edits then merge into one undo step).
    pub(crate) fn paper_settings(&mut self, ui: &mut egui::Ui, sheet: &mut Sheet) -> bool {
        let mut typing = false;
        theme::section(ui, "Sheet");
        theme::row(ui, "Number", |ui| {
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut sheet.fields.number)
                        .hint_text("A-101")
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
        });
        theme::row(ui, "Name", |ui| {
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut sheet.name)
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
        });
        theme::row(ui, "Status", |ui| {
            // A field of its own, with the common statuses in a menu beside it.
            let room = ui.available_width() - 8.0;
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut sheet.fields.status)
                        .hint_text("For permit")
                        .desired_width(room - 44.0),
                )
                .on_hover_text("What the sheet is issued for")
                .changed();
            egui::ComboBox::from_id_salt("sheet_status")
                .selected_text("")
                .width(20.0)
                .show_ui(ui, |ui| {
                    for s in drift_core::sheets::STATUSES {
                        if ui.selectable_label(sheet.fields.status == s, s).clicked() {
                            sheet.fields.status = s.to_owned();
                        }
                    }
                })
                .response
                .on_hover_text("Common statuses");
        });
        theme::section(ui, "Paper");
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
        // How it prints: in colour, in greys, or black lines over grey fills.
        let colours = SheetColours::ALL.map(|c| (c, c.label()));
        if let Some(c) = theme::segmented(ui, sheet.colours, &colours) {
            sheet.colours = c;
        }
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
                self.pick_file(crate::app::document::Picking::Logo);
            }
            if has && ui.button("Remove").clicked() {
                self.editor.doc.begin();
                self.editor.doc.set_logo(String::new());
                self.editor.commit("Remove logo");
            }
        });
        // The project's title and company show as hints: typing one here is this sheet's own.
        let project = self.editor.doc.project().clone();
        let or = |own: &'static str, shared: &str| -> String {
            if shared.trim().is_empty() {
                own.to_owned()
            } else {
                format!("{}  · from project", shared.trim())
            }
        };
        let (title_hint, company_hint) =
            (or("Drawing title", &project.name), or("", &project.company));
        let f = &mut sheet.fields;
        let mut rows = vec![
            ("Title", &mut f.title, title_hint.as_str()),
            ("Drawn by", &mut f.author, ""),
            ("Checked", &mut f.checked, ""),
            ("Company", &mut f.company, company_hint.as_str()),
        ];
        if project.fields == drift_core::FieldSet::Mechanical {
            rows.push(("Material", &mut f.material, ""));
        }
        for (label, value, hint) in rows {
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
        theme::row(ui, "Project", |ui| {
            if ui
                .button("Project settings…")
                .on_hover_text(
                    "The project's name, number, client, address and company, shared by every \
                     sheet (File → Drawing settings)",
                )
                .clicked()
            {
                self.show_drawing_settings = true;
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
        typing
    }

    fn output_buttons(&mut self, ui: &mut egui::Ui) {
        let painter = ui.painter().clone();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("PDF…")
                .on_hover_text("The sheets you choose, at true scale")
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
                .button("DWG / DXF…")
                .on_hover_text("The model at full size, with its layers, for CAD")
                .clicked()
            {
                self.export_dxf(&painter);
            }
            if ui.button("Print…").clicked() {
                self.print(&painter);
            }
        });
        ui.label(egui::RichText::new("Model at 1 : 1").weak());
        ui.horizontal_wrapped(|ui| self.model_export_buttons(ui, &painter));
    }

    /// Model as SVG and PDF, and whether they carry dimensions and notes.
    pub(crate) fn model_export_buttons(&mut self, ui: &mut egui::Ui, painter: &egui::Painter) {
        let scope = if self.model_scope().is_some() {
            "The selection"
        } else {
            "Everything that prints"
        };
        if ui
            .button("Model as SVG…")
            .on_hover_text(format!(
                "{scope}, at true size on a page fitted to it, a group per layer"
            ))
            .clicked()
        {
            self.export_model(painter, false);
        }
        if ui
            .button("Model as PDF…")
            .on_hover_text(format!("{scope}, at true size on a page fitted to it"))
            .clicked()
        {
            self.export_model(painter, true);
        }
        ui.checkbox(&mut self.model_annotations, "Dimensions and notes")
            .on_hover_text("Model exports include dimensions and notes");
    }

    /// What a model export is limited to: the pick, if anything is picked.
    fn model_scope(&self) -> Option<crate::output::Scope> {
        let dimension = self.selected_constraint.filter(|id| {
            self.editor
                .doc
                .constraint(*id)
                .is_some_and(|c| c.is_dimension())
        });
        crate::output::Scope::of_selection(&self.editor, dimension)
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
            let views = self.editor.doc.placed_views(&sheet);
            let d = crate::output::sheet_drawing(
                &mut self.editor,
                &self.hatch_patterns,
                &painter,
                &sheet,
                &views,
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
        let views = self.editor.doc.placed_views(&sheet);
        self.view_input(ui, &response, rect, id, &sheet, &views);
        self.view_drop(ui, &response, rect, id);
        // Right-click on a view: what can be done with it.
        if self.sheet_pick == crate::app::sheet_editor::SheetPick::View {
            response.context_menu(|ui| {
                ui.set_min_width(210.0);
                self.command_items(
                    ui,
                    &[
                        Some(crate::commands::Command::RemoveFromSheet),
                        Some(crate::commands::Command::DeleteView),
                    ],
                );
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
        self.paint_view_frames(&painter, rect, &sheet, &views, ui.visuals().dark_mode);
        self.paint_drop_ghost(ui, &response, rect, &views);
        // What this is, and how to leave.
        // The paper and scale (the tab above names the sheet).
        let info = format!(
            "{} {} · {}",
            sheet.paper.label(),
            if sheet.portrait {
                "portrait"
            } else {
                "landscape"
            },
            self.editor.doc.sheet_scale_text(&sheet)
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
        self.sheet_view.min(sheet.viewports.len().saturating_sub(1))
    }

    /// The views picked on the sheet shown, by index: several after Shift/Ctrl-click, a box
    /// on the paper or Select all, else the current one alone, or none with the paper picked.
    pub(crate) fn picked_views(&self, sheet: &Sheet) -> std::collections::BTreeSet<usize> {
        let n = sheet.viewports.len();
        if self.sheet_pick != crate::app::sheet_editor::SheetPick::View || n == 0 {
            return Default::default();
        }
        let current = self.current_view(sheet);
        match &self.view_set {
            Some((id, set)) if Some(*id) == self.current_sheet() && set.contains(&current) => {
                set.iter().copied().filter(|i| *i < n).collect()
            }
            _ => [current].into(),
        }
    }

    /// Pick `set` (indices of views on sheet `id`), `current` the one the Inspector and
    /// handles follow; an empty set picks the paper.
    pub(crate) fn pick_views(
        &mut self,
        id: SheetId,
        views: &[PlacedView],
        set: std::collections::BTreeSet<usize>,
        current: usize,
    ) {
        use crate::app::sheet_editor::SheetPick;
        self.confirm_delete_set = false;
        let Some(&first) = set.first() else {
            self.view_set = None;
            self.sheet_pick = SheetPick::Paper;
            return;
        };
        let current = if set.contains(&current) {
            current
        } else {
            first
        };
        self.sheet_view = current;
        self.picked_view = views.get(current).map(|v| v.id);
        self.sheet_pick = SheetPick::View;
        self.view_set = (set.len() > 1).then_some((id, set));
    }

    /// Pick every view on the current sheet (Select all on a sheet).
    pub(crate) fn pick_all_views(&mut self) {
        if let Some(id) = self.current_sheet()
            && let Some(sheet) = self.editor.doc.sheet(id)
        {
            let views = self.editor.doc.placed_views(sheet);
            let current = self.sheet_view;
            self.pick_views(id, &views, (0..views.len()).collect(), current);
        }
    }

    /// Move the picked views on the current sheet by `paper` millimetres (arrow keys), kept
    /// inside the drawing area: one undo step.
    pub(crate) fn nudge_views(&mut self, paper: DVec2) {
        let Some(id) = self.current_sheet() else {
            return;
        };
        let Some(sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let picked = self.picked_views(&sheet);
        if picked.is_empty() {
            return;
        }
        let views = self.editor.doc.placed_views(&sheet);
        let frames: drift_core::Bounds = picked.iter().map(|i| views[*i].frame.bounds()).collect();
        let paper = contain_set(frames, &sheet, paper);
        if paper == DVec2::ZERO {
            return;
        }
        self.editor.doc.begin();
        for &i in &picked {
            let moved = dragged_view(&views[i], ViewGrip::Move, paper);
            self.editor.doc.set_placed_view(id, i, &moved);
        }
        self.editor.commit(if picked.len() == 1 {
            "Move view on sheet"
        } else {
            "Move views on sheet"
        });
    }

    /// Take the picked views off sheet `id` (they stay in the model): one undo step.
    pub(crate) fn remove_picked_views(&mut self, id: SheetId) {
        let Some(mut sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        let picked = self.picked_views(&sheet);
        if picked.len() <= 1 {
            self.remove_viewport(id);
            return;
        }
        for i in picked.iter().rev() {
            sheet.viewports.remove(*i);
        }
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Remove views from sheet");
        self.sheet_view = picked.first().map_or(0, |i| i.saturating_sub(1));
        self.view_set = None;
        self.sheet_pick = crate::app::sheet_editor::SheetPick::Paper;
    }

    /// Delete the picked views from the model (and so from every sheet): one undo step.
    pub(crate) fn delete_picked_views(&mut self) {
        let Some(sheet) = self
            .current_sheet()
            .and_then(|id| self.editor.doc.sheet(id))
        else {
            return;
        };
        let views = self.editor.doc.placed_views(sheet);
        let gone: Vec<ViewId> = self
            .picked_views(sheet)
            .iter()
            .map(|i| views[*i].id)
            .collect();
        self.editor.doc.begin();
        for v in &gone {
            self.editor.doc.remove_view(*v);
        }
        self.editor.commit("Delete views");
        self.picked_view = None;
        self.view_set = None;
        self.confirm_delete_set = false;
        self.sheet_pick = crate::app::sheet_editor::SheetPick::Paper;
    }

    /// A view's frame on screen.
    fn view_rect(&self, rect: egui::Rect, view: &PlacedView) -> egui::Rect {
        let b = view.frame.bounds();
        egui::Rect::from_two_pos(
            self.paper_to_preview(rect, b.min),
            self.paper_to_preview(rect, b.max),
        )
    }

    /// Where a frame's handles are on screen, with what each one resizes.
    pub(crate) fn view_handles(frame: egui::Rect) -> [(egui::Pos2, (i8, i8)); 8] {
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
    /// under the pointer (one in a frame of its own before one that fills the drawing area,
    /// which a plain drag slides the drawing in, having no room to move).
    pub(crate) fn view_grip_at(
        &self,
        rect: egui::Rect,
        sheet: &Sheet,
        views: &[PlacedView],
        pos: egui::Pos2,
        alt: bool,
    ) -> Option<(usize, ViewGrip)> {
        let picked = self.current_view(sheet);
        if let Some(v) = views.get(picked)
            && self.picked_views(sheet).len() <= 1
        {
            let frame = self.view_rect(rect, v);
            if let Some((_, (dx, dy))) = Self::view_handles(frame)
                .into_iter()
                .find(|(at, _)| at.distance(pos) <= HANDLE_REACH)
            {
                return Some((picked, ViewGrip::Resize(dx, dy)));
            }
        }
        let fills =
            |v: &PlacedView| drift_draft::sheet::fills_drawing_area(sheet, v.frame.bounds());
        let under = |filling: bool| {
            views
                .iter()
                .enumerate()
                .rev()
                .find(|(_, v)| fills(v) == filling && self.view_rect(rect, v).contains(pos))
        };
        let (index, view) = under(false).or_else(|| under(true))?;
        let grip = if alt {
            ViewGrip::Pan
        } else if fills(view) {
            ViewGrip::Fixed
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
        views: &[PlacedView],
    ) {
        let alt = ui.input(|i| i.modifiers.alt);
        // Shift or Ctrl/⌘ adds to the views picked (or takes one out).
        let adding = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if response.drag_started_by(PointerButton::Primary)
            && let Some(from) = ui.input(|i| i.pointer.press_origin())
        {
            match self.view_grip_at(rect, sheet, views, from, alt) {
                Some((index, grip)) if grip != ViewGrip::Fixed => {
                    // A move of one of several picked views takes them all along.
                    let picked = self.picked_views(sheet);
                    let group: Vec<(usize, PlacedView)> =
                        if grip == ViewGrip::Move && picked.len() > 1 && picked.contains(&index) {
                            picked
                                .iter()
                                .filter(|i| **i != index)
                                .map(|i| (*i, views[*i].clone()))
                                .collect()
                        } else {
                            Vec::new()
                        };
                    let set = if group.is_empty() {
                        [index].into()
                    } else {
                        picked
                    };
                    self.pick_views(id, views, set, index);
                    self.view_drag = Some(ViewDrag {
                        sheet: id,
                        index,
                        grip,
                        was: views[index].clone(),
                        from,
                        group,
                    });
                    self.editor.doc.begin();
                }
                // On the paper (or a view filling the drawing area): a box to pick views by.
                _ => self.view_box = Some((from, from)),
            }
        }
        if let Some((start, _)) = self.view_box {
            let end = ui.input(|i| i.pointer.latest_pos()).unwrap_or(start);
            self.view_box = Some((start, end));
            if !response.dragged_by(PointerButton::Primary) {
                self.view_box = None;
                // Left to right: views wholly inside; right to left: views it touches too.
                let area = egui::Rect::from_two_pos(start, end);
                let crossing = end.x < start.x;
                let hits = views.iter().enumerate().filter(|(_, v)| {
                    let frame = self.view_rect(rect, v);
                    if crossing {
                        area.intersects(frame)
                    } else {
                        area.contains_rect(frame)
                    }
                });
                let mut set = if adding {
                    self.picked_views(sheet)
                } else {
                    Default::default()
                };
                set.extend(hits.map(|(i, _)| i));
                let current = self.sheet_view;
                self.pick_views(id, views, set, current);
                self.show_panel(crate::dock::Panel::Inspector);
            }
        }
        if let Some(drag) = self.view_drag.clone() {
            if let Some(pos) = response.interact_pointer_pos()
                && drag.index < views.len()
                && drag.group.iter().all(|(i, _)| *i < views.len())
            {
                let d = pos - drag.from;
                let mut paper =
                    DVec2::new(f64::from(d.x), -f64::from(d.y)) / f64::from(self.preview.zoom);
                let free = ui.input(|i| i.modifiers.command);
                // Edges, middles and drawings line up with the other views' (Ctrl: freely);
                // the views moving along aren't lined up with.
                self.view_guides.clear();
                if !free {
                    let tol = 6.0 / f64::from(self.preview.zoom);
                    let moving = |i: usize| drag.group.iter().any(|(g, _)| *g == i);
                    let others: Vec<PlacedView> = views
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !moving(*i))
                        .map(|(_, v)| v.clone())
                        .collect();
                    let index = (0..drag.index).filter(|i| !moving(*i)).count();
                    let (snapped, guides) = crate::app::view_align::snap_drag(
                        sheet, &others, index, &drag.was, drag.grip, paper, tol,
                    );
                    paper = snapped;
                    self.view_guides = guides;
                }
                if !drag.group.is_empty() {
                    // Several views: they keep their places relative to each other, and all
                    // stay inside the border (Ctrl: anywhere).
                    if !free {
                        let frames: drift_core::Bounds = std::iter::once(&drag.was)
                            .chain(drag.group.iter().map(|(_, v)| v))
                            .map(|v| v.frame.bounds())
                            .collect();
                        paper = contain_set(frames, sheet, paper);
                    }
                    for (i, was) in std::iter::once((drag.index, &drag.was))
                        .chain(drag.group.iter().map(|(i, v)| (*i, v)))
                    {
                        let moved = dragged_view(was, ViewGrip::Move, paper);
                        self.editor.doc.set_placed_view(drag.sheet, i, &moved);
                    }
                } else {
                    let mut moved = dragged_view(&drag.was, drag.grip, paper);
                    // Views stay on the paper, inside the border (Ctrl: anywhere).
                    if !free {
                        contain(&mut moved, sheet, drag.grip);
                    }
                    self.editor
                        .doc
                        .set_placed_view(drag.sheet, drag.index, &moved);
                }
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.view_drag = None;
                self.view_guides.clear();
                self.editor.commit(match drag.grip {
                    ViewGrip::Pan => "Move sheet view",
                    ViewGrip::Move if !drag.group.is_empty() => "Move views on sheet",
                    ViewGrip::Move | ViewGrip::Fixed => "Move view on sheet",
                    ViewGrip::Resize(..) => "Resize view",
                });
            }
        } else if (response.clicked_by(PointerButton::Primary)
            || response.clicked_by(PointerButton::Secondary))
            && let Some(pos) = response.interact_pointer_pos()
        {
            // A view, or the paper around the views: picked, and shown in the Inspector. A
            // view filling the drawing area counts only where its drawing is.
            let model = self.fit_bounds();
            let at = self.preview_to_paper(rect, pos);
            let hit = self
                .view_grip_at(rect, sheet, views, pos, false)
                .filter(|(i, _)| {
                    let v = &views[*i];
                    !drift_draft::sheet::fills_drawing_area(sheet, v.frame.bounds())
                        || crate::app::sheet_editor::on_drawing(v, model, at)
                });
            let primary = response.clicked_by(PointerButton::Primary);
            match hit {
                // Shift or Ctrl/⌘: in or out of the views picked.
                Some((index, _)) if primary && adding => {
                    let mut set = self.picked_views(sheet);
                    if !set.remove(&index) {
                        set.insert(index);
                    }
                    self.pick_views(id, views, set, index);
                }
                // A right-click on one of several picked views keeps them all.
                Some((index, _)) if !primary && self.picked_views(sheet).contains(&index) => {
                    let set = self.picked_views(sheet);
                    self.pick_views(id, views, set, index);
                }
                Some((index, _)) => self.pick_views(id, views, [index].into(), index),
                None if primary && adding => {}
                None => self.pick_views(id, views, Default::default(), 0),
            }
            if response.clicked_by(PointerButton::Primary) {
                self.show_panel(crate::dock::Panel::Inspector);
            }
        }
        if response.hovered() {
            let grip = self.view_drag.as_ref().map(|d| d.grip).or_else(|| {
                response
                    .hover_pos()
                    .and_then(|p| self.view_grip_at(rect, sheet, views, p, alt))
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
                Some(ViewGrip::Fixed) | None => egui::CursorIcon::Default,
            });
        }
    }

    /// A view dragged from the Views list and let go over the sheet goes on it there.
    fn view_drop(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        id: SheetId,
    ) {
        let Some(pos) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
        else {
            return;
        };
        let at = self.preview_to_paper(rect, pos);
        if let Some(view) = response.dnd_release_payload::<ViewId>() {
            self.place_view(id, *view, Some(at));
        }
    }

    /// While a view from the Views list is held over the sheet, its frame where it would go.
    fn paint_drop_ghost(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        views: &[PlacedView],
    ) {
        let Some(pos) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
        else {
            return;
        };
        let Some(held) = response.dnd_hover_payload::<ViewId>() else {
            return;
        };
        let Some(view) = self.editor.doc.view(*held) else {
            return;
        };
        let size = view.paper_size();
        let ghost = egui::Rect::from_center_size(
            pos,
            egui::vec2(size.x as f32, size.y as f32) * self.preview.zoom,
        );
        let accent = theme::accent(ui.visuals().dark_mode);
        let painter = ui.painter_at(rect);
        painter.rect_filled(ghost, 0.0, accent.gamma_multiply(0.12));
        painter.rect_stroke(
            ghost,
            0.0,
            egui::Stroke::new(1.5, accent),
            egui::StrokeKind::Outside,
        );
        let already = views.iter().any(|v| v.id == *held);
        let text = if already {
            format!("{} (again)", view.title())
        } else {
            view.title().to_owned()
        };
        painter.text(
            ghost.left_bottom() + egui::vec2(0.0, 4.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(12.0),
            accent,
        );
    }

    /// The editor's marks over the page (they don't print): the picked view's frame and
    /// handles, and the frame of the one under the pointer.
    fn paint_view_frames(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        sheet: &Sheet,
        views: &[PlacedView],
        dark: bool,
    ) {
        let accent = theme::accent(dark);
        let picked = self.picked_views(sheet);
        if let Some((a, b)) = self.view_box {
            let area = egui::Rect::from_two_pos(a, b);
            painter.rect_filled(area, 0.0, accent.gamma_multiply(0.08));
            painter.rect_stroke(
                area,
                0.0,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Middle,
            );
        }
        let hover = painter
            .ctx()
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .and_then(|p| self.view_grip_at(rect, sheet, views, p, false))
            .map(|(i, _)| i);
        // The lines a dragged view snapped to, across the sheet: frames in the accent, drawings
        // lining up in magenta.
        let paper = sheet.size();
        for g in &self.view_guides {
            let (a, b) = if g.vertical {
                (DVec2::new(g.at, 0.0), DVec2::new(g.at, paper.y))
            } else {
                (DVec2::new(0.0, g.at), DVec2::new(paper.x, g.at))
            };
            let color = if g.drawings {
                egui::Color32::from_rgb(0xe0, 0x3c, 0xc8)
            } else {
                accent
            };
            crate::overlay::dashed(
                painter,
                &[
                    self.paper_to_preview(rect, a),
                    self.paper_to_preview(rect, b),
                ],
                egui::Stroke::new(1.0, color),
                6.0,
                4.0,
            );
        }
        for (i, view) in views.iter().enumerate() {
            let frame = self.view_rect(rect, view);
            if picked.contains(&i) {
                painter.rect_stroke(
                    frame,
                    0.0,
                    egui::Stroke::new(1.5, accent),
                    egui::StrokeKind::Outside,
                );
                // Resize handles for one picked view only.
                let handles = if picked.len() == 1 {
                    Self::view_handles(frame).to_vec()
                } else {
                    Vec::new()
                };
                for (at, _) in handles {
                    let knob = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
                    painter.rect_filled(knob, 1.0, egui::Color32::WHITE);
                    painter.rect_stroke(
                        knob,
                        1.0,
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Middle,
                    );
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

    /// Put a model view on a sheet: its middle `at` a paper point, or, at the scale it has,
    /// on free paper clear of the views there (failing that, where `place_frame` puts it).
    /// Returns its index on the sheet.
    pub(crate) fn place_view(
        &mut self,
        id: SheetId,
        view: ViewId,
        at: Option<DVec2>,
    ) -> Option<usize> {
        let mut sheet = self.editor.doc.sheet(id)?.clone();
        let size = self.editor.doc.view(view)?.paper_size();
        let placed = self.editor.doc.placed_views(&sheet);
        let model = self.fit_bounds();
        let at = at
            .or_else(|| crate::app::sheet_editor::free_spot_on_sheet(&sheet, &placed, model, size))
            .unwrap_or_else(|| crate::output::place_frame(&sheet, &placed, size));
        let mut port = Viewport::new(view, at);
        port.title = true;
        sheet.viewports.push(port);
        let index = sheet.viewports.len() - 1;
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Place view on sheet");
        self.active_sheet = Some(id);
        self.sheet_view = index;
        self.picked_view = Some(view);
        Some(index)
    }

    /// Take the picked view off a sheet. The view stays in the model, for placing again.
    pub(crate) fn remove_viewport(&mut self, id: SheetId) {
        let Some(mut sheet) = self.editor.doc.sheet(id).cloned() else {
            return;
        };
        if sheet.viewports.is_empty() {
            return;
        }
        let index = self.current_view(&sheet);
        sheet.viewports.remove(index);
        self.editor.doc.begin();
        self.editor.doc.set_sheet(id, sheet);
        self.editor.commit("Remove view from sheet");
        self.sheet_view = index.saturating_sub(1);
    }

    /// Delete a view from the model, and so from every sheet it is on.
    pub(crate) fn delete_view(&mut self, view: ViewId) {
        self.editor.doc.begin();
        self.editor.doc.remove_view(view);
        self.editor.commit("Delete view");
        if self.picked_view == Some(view) {
            self.picked_view = None;
        }
        self.confirm_delete_view = None;
        self.view_set = None;
    }

    /// Show a view's crop on the canvas, in the model.
    pub(crate) fn zoom_to_view(&mut self, view: ViewId) {
        if let Some(v) = self.editor.doc.view(view) {
            let crop = v.crop();
            // A little air round it, so its name and grips show.
            let air = crop.size().max_element() * 0.08;
            self.fit_target = Some(Bounds {
                min: crop.min - DVec2::splat(air),
                max: crop.max + DVec2::splat(air),
            });
            self.sheet_preview = false;
            self.picked_view = Some(view);
        }
    }

    /// The Sheets panel's Views section: every view of the model, with the sheets it is on.
    /// Click picks one (its crop shows its grips on the canvas), double-click zooms to it, and
    /// dragging one onto the sheet preview places it there.
    fn model_views_section(&mut self, ui: &mut egui::Ui, current: Option<SheetId>) {
        theme::section(ui, "Views");
        let doc = &self.editor.doc;
        let rows: Vec<(ViewId, String, String)> = doc
            .views()
            .map(|(id, v)| {
                let on: Vec<String> = doc
                    .sheets_showing(id)
                    .into_iter()
                    .filter_map(|s| doc.sheet(s).map(crate::app::sheet_editor::sheet_label))
                    .collect();
                let on = if on.is_empty() {
                    "not placed".to_owned()
                } else {
                    format!("on {}", on.join(", "))
                };
                (
                    id,
                    v.title().to_owned(),
                    format!("{} · {on}", scale_label(v.scale)),
                )
            })
            .collect();
        if rows.is_empty() {
            ui.label(
                egui::RichText::new(
                    "A view is a named crop of the model at a scale. Draw one with the View \
                     tool, or add one to a sheet.",
                )
                .weak(),
            );
        }
        let mut zoom = None;
        for (id, name, detail) in &rows {
            let picked = self.picked_view == Some(*id);
            let text = egui::RichText::new(format!("{name}\n{detail}"));
            // (The space after the text grows: it sits at the left, as in the other lists.)
            let response = ui
                .add_sized(
                    [ui.available_width(), 34.0],
                    egui::Button::selectable(picked, (text, egui::Atom::grow()))
                        .sense(egui::Sense::click_and_drag()),
                )
                .on_hover_text(
                    "Double-click to zoom to it in the model; drag it onto the sheet to place it",
                );
            response.dnd_set_drag_payload(*id);
            if response.clicked() || response.drag_started() {
                self.picked_view = Some(*id);
            }
            if response.double_clicked() {
                zoom = Some(*id);
            }
        }
        if let Some(id) = zoom {
            self.zoom_to_view(id);
        }
        let Some(view_id) = self
            .picked_view
            .filter(|id| self.editor.doc.view(*id).is_some())
        else {
            return;
        };
        let Some(original) = self.editor.doc.view(view_id).cloned() else {
            return;
        };
        let mut view = original.clone();
        let mut typing = false;
        theme::row(ui, "Name", |ui| {
            typing |= ui
                .add(
                    egui::TextEdit::singleline(&mut view.name)
                        .hint_text("Plan, Detail A…")
                        .desired_width(ui.available_width() - 8.0),
                )
                .changed();
        });
        theme::row(ui, "Scale", |ui| {
            self.scale_menu(ui, "model_view_scale", &mut view.scale);
        });
        // Four switches in two even columns.
        theme::row(ui, "Show", |ui| {
            egui::Grid::new("model_view_show")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.checkbox(&mut view.dimensions, "Dimensions")
                        .on_hover_text("Dimensions and ordinates, in this view");
                    ui.checkbox(&mut view.notes, "Notes")
                        .on_hover_text("Notes and callouts, in this view");
                    ui.end_row();
                    ui.checkbox(&mut view.construction, "Construction")
                        .on_hover_text(CONSTRUCTION_TIP);
                    ui.checkbox(&mut view.crop_visible, "Crop")
                        .on_hover_text("Its crop and name on the canvas (they never print)");
                    ui.end_row();
                });
        });
        self.update_view(view_id, view, typing);
        self.referenced_by(ui, view_id);
        let placed = self.editor.doc.sheets_showing(view_id);
        ui.horizontal_wrapped(|ui| {
            if let Some(sheet) = current {
                let here = placed.contains(&sheet);
                if ui
                    .button(if here {
                        "Place again"
                    } else {
                        "Place on sheet"
                    })
                    .on_hover_text("Put it on the current sheet, where there is room")
                    .clicked()
                {
                    self.place_view(sheet, view_id, None);
                    self.sheet_preview = true;
                }
            }
            if ui
                .button("Zoom to view")
                .on_hover_text("Show its crop in the model")
                .clicked()
            {
                self.zoom_to_view(view_id);
            }
            if ui.button("Delete view").clicked() {
                if placed.is_empty() {
                    self.delete_view(view_id);
                } else {
                    self.confirm_delete_view = Some(view_id);
                }
            }
        });
        if self.confirm_delete_view == Some(view_id) {
            let n = placed.len();
            ui.label(
                egui::RichText::new(format!(
                    "It is on {n} sheet{}: deleting it takes it off {}.",
                    if n == 1 { "" } else { "s" },
                    if n == 1 { "that sheet" } else { "them all" }
                ))
                .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
            );
            ui.horizontal(|ui| {
                if ui.button("Delete it").clicked() {
                    self.delete_view(view_id);
                }
                if ui.button("Keep it").clicked() {
                    self.confirm_delete_view = None;
                }
            });
        }
    }

    /// A scale menu: the scales for the drawing's units first.
    pub(crate) fn scale_menu(&self, ui: &mut egui::Ui, id: &str, scale: &mut f64) {
        let imperial = self.imperial();
        egui::ComboBox::from_id_salt(id)
            .selected_text(scale_label(*scale))
            .width(90.0)
            .height(420.0)
            .show_ui(ui, |ui| {
                ui.set_min_width(150.0);
                let metric: Vec<f64> = PREFERRED_SCALES.to_vec();
                let arch: Vec<f64> = ARCHITECTURAL_SCALES.iter().map(|(n, _)| *n).collect();
                let eng: Vec<f64> = ENGINEERING_SCALES.iter().map(|(n, _)| *n).collect();
                let groups = if imperial {
                    [
                        ("Architectural", arch),
                        ("Engineering", eng),
                        ("Metric", metric),
                    ]
                } else {
                    [
                        ("Metric", metric),
                        ("Architectural", arch),
                        ("Engineering", eng),
                    ]
                };
                for (k, (name, scales)) in groups.into_iter().enumerate() {
                    if k > 0 {
                        ui.separator();
                    }
                    ui.label(egui::RichText::new(name).small().weak());
                    for n in scales {
                        ui.selectable_value(scale, n, scale_label(n));
                    }
                }
            });
    }

    /// The Sheets panel's section for the views on the current sheet, and the picked one's
    /// settings. Its edits come back in `edited` (its index and new state). Returns true if
    /// text was typed (so the edits merge into one undo step).
    pub(crate) fn views_section(
        &mut self,
        ui: &mut egui::Ui,
        sheet: &Sheet,
        edited: &mut Option<(usize, PlacedView)>,
    ) -> bool {
        let mut typing = false;
        let imperial = self.imperial();
        theme::section(ui, "Views on this sheet");
        let views = self.editor.doc.placed_views(sheet);
        let picked = self.current_view(sheet);
        for (i, v) in views.iter().enumerate() {
            let text = format!("{}  {}  ·  {}", i + 1, v.title(), scale_label(v.scale));
            if theme::list_row(ui, picked == i, &text).clicked() {
                self.sheet_view = i;
                self.picked_view = Some(v.id);
            }
        }
        let picked = self.current_view(sheet);
        let bounds = self.fit_bounds();
        let area = drift_draft::sheet::drawing_area(sheet);
        if let Some(original) = views.get(picked) {
            let mut view = original.clone();
            let mut fit = false;
            let mut fill = false;
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
                self.scale_menu(ui, "sheet_scale", &mut view.scale);
                fit = ui
                    .button("Fit")
                    .on_hover_text("Centre the drawing in the view at the largest scale that fits")
                    .clicked();
            });
            // A new scale keeps the crop in the model: the frame on the paper grows or shrinks.
            if view.scale != original.scale {
                view.frame.size = original.frame.size * original.scale / view.scale;
            }
            theme::row(ui, "Frame", |ui| {
                fill = ui
                    .button("Fill drawing area")
                    .on_hover_text(
                        "Frame it to the sheet's whole drawing area (after changing the paper \
                         size, say), cropping more or less of the model",
                    )
                    .clicked();
            });
            // What the view shows, then how it is framed, in two even columns.
            theme::row(ui, "Show", |ui| {
                egui::Grid::new("view_show")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.checkbox(&mut view.dimensions, "Dimensions")
                            .on_hover_text("Dimensions and ordinates, in this view");
                        ui.checkbox(&mut view.notes, "Notes")
                            .on_hover_text("Notes and callouts, in this view");
                        ui.end_row();
                        ui.checkbox(&mut view.construction, "Construction")
                            .on_hover_text(CONSTRUCTION_TIP);
                        ui.end_row();
                        ui.checkbox(&mut view.title, "Title")
                            .on_hover_text("Its title and scale, written under it");
                        ui.checkbox(&mut view.border, "Border")
                            .on_hover_text("Print the frame's outline");
                        ui.end_row();
                    });
            });
            // The layers drawn in this view; one hidden in the model is off everywhere.
            let layers: Vec<(drift_core::LayerId, String, bool)> = self
                .editor
                .doc
                .layers()
                .map(|(id, l)| (id, l.name.clone(), l.visible && l.printable))
                .collect();
            theme::row(ui, "Layers", |ui| {
                ui.vertical(|ui| {
                    for (id, name, drawn) in layers {
                        let mut on = drawn && !view.hidden_layers.contains(&id);
                        let r = ui.add_enabled(drawn, egui::Checkbox::new(&mut on, name));
                        let r = if drawn {
                            r.on_hover_text("Draw this layer in this view")
                        } else {
                            r.on_disabled_hover_text(
                                "Hidden or not printed in the model (Layers panel)",
                            )
                        };
                        if r.changed() {
                            if on {
                                view.hidden_layers.remove(&id);
                            } else {
                                view.hidden_layers.insert(id);
                            }
                        }
                    }
                });
            });
            self.referenced_by(ui, original.id);
            if fit {
                crate::output::fit_view(bounds, &mut view, imperial);
            }
            if fill {
                view.frame = ViewFrame {
                    at: area.center(),
                    size: area.size(),
                };
            }
            if views.len() > 1 {
                let mut all = views.clone();
                all[picked] = view.clone();
                if self.align_row(ui, &mut all, picked) {
                    view = all[picked].clone();
                }
            }
            if view != *original {
                *edited = Some((picked, view));
            }
        } else {
            ui.label(
                egui::RichText::new(
                    "No views yet: Add view in the rail, or drag one from the Views list in \
                     the Sheets panel onto the sheet.",
                )
                .weak(),
            );
        }
        typing
    }

    /// Align the picked view with another: frame edges and middles, or lining the drawings
    /// up (same scale). True if it moved.
    fn align_row(&mut self, ui: &mut egui::Ui, views: &mut [PlacedView], picked: usize) -> bool {
        use crate::app::view_align::{AlignTo, align_view, same_scale};
        let others: Vec<(usize, String)> = views
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != picked)
            .map(|(i, v)| (i, format!("{} {}", i + 1, v.title())))
            .collect();
        let mut with = self
            .align_with
            .filter(|w| others.iter().any(|(i, _)| i == w))
            .unwrap_or(others[0].0);
        theme::row(ui, "Align with", |ui| {
            let shown = others
                .iter()
                .find(|(i, _)| *i == with)
                .map_or(String::new(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("align_with")
                .selected_text(shown)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for (i, n) in &others {
                        ui.selectable_value(&mut with, *i, n);
                    }
                });
        });
        self.align_with = Some(with);
        let same_scale = same_scale(&views[picked], &views[with]);
        let mut how = None;
        ui.horizontal_wrapped(|ui| {
            for (a, label, hint) in AlignTo::FRAMES {
                if ui
                    .add(egui::Button::new(label).small())
                    .on_hover_text(hint)
                    .clicked()
                {
                    how = Some(a);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (a, label, hint) in [
                (
                    AlignTo::DrawingsAcross,
                    "Drawings level",
                    "Side by side: the same height of the model at the same height on the \
                     paper (floors meet)",
                ),
                (
                    AlignTo::DrawingsDown,
                    "Drawings in line",
                    "One above the other: the same x of the model at the same x on the paper",
                ),
            ] {
                if ui
                    .add_enabled(same_scale, egui::Button::new(label).small())
                    .on_hover_text(hint)
                    .on_disabled_hover_text("Only drawings at the same scale line up")
                    .clicked()
                {
                    how = Some(a);
                }
            }
        });
        how.is_some_and(|a| align_view(views, picked, with, a))
    }

    pub(crate) fn paper_to_preview(&self, rect: egui::Rect, p: DVec2) -> egui::Pos2 {
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
