//! Importing DWG, DXF, SVG, PDF and Illustrator drawings: one dialog for how it comes in
//! (as curves to edit or as an underlay to trace, which page or artboard, the scale it was
//! drawn at — found from its dimensions where it can be — whether its pictures come too,
//! and whether edges that only bound a fill print), then the import and its report.
//!
//! An Illustrator file is read through the PDF copy of its artwork that Illustrator saves in
//! it (its own data is private), one page an artboard.

use drift_io::dxf::DxfImport;
use drift_io::vector::{Artwork, ImportOptions, ScaleGuess};

use crate::*;

pub(crate) enum ImportSource {
    Svg(String),
    Pdf(Vec<u8>),
    /// A DXF drawing's text.
    Dxf(String),
    /// A DWG drawing's bytes.
    Dwg(Vec<u8>),
}

/// How the next file opened to import comes in, when that was chosen before it was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportAs {
    /// The dialog, set to editable geometry.
    Geometry,
    /// The dialog, set to an underlay (from the Underlays section).
    Underlay,
    /// As geometry at the defaults, without asking, into the new drawing it was opened as.
    NewDrawing,
}

/// What the drop dialog was answered with.
#[derive(Clone, Copy)]
enum DropChoice {
    Open,
    Import(ImportAs),
    Cancel,
}

/// A file waiting for the import dialog.
pub(crate) struct PendingImport {
    pub name: String,
    pub source: ImportSource,
    pub pages: u32,
    pub page: u32,
    /// As an underlay rather than curves.
    pub underlay: bool,
    /// The scale it was drawn at, 1 : n.
    pub scale: f64,
    pub construction_edges: bool,
    /// Pick two points of known distance on it once it's in, to set its scale.
    pub calibrate: bool,
    /// Bring the pictures of a PDF or SVG in (as an underlay beneath it).
    pub images: bool,
    /// An Illustrator file: its pages are artboards.
    pub illustrator: bool,
    /// The scale has been chosen (so a scale found from the dimensions doesn't replace it).
    pub scale_chosen: bool,
    /// The drawing's unit (mm), for CAD files that don't say theirs.
    unit_mm: f64,
    /// The page read so far, and which page it is.
    art: Option<(u32, Result<Artwork, String>)>,
    /// The scale its dimensions show, and the page that is of.
    guess: Option<(u32, Option<ScaleGuess>)>,
    /// A DXF or DWG drawing as read, at full size.
    cad: Option<Result<DxfImport, String>>,
}

impl PendingImport {
    fn stem(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map_or(self.name.as_str(), |(s, _)| s)
    }

    /// A DXF or DWG drawing, rather than artwork.
    pub fn is_cad(&self) -> bool {
        matches!(self.source, ImportSource::Dxf(_) | ImportSource::Dwg(_))
    }

    /// The artwork of the chosen page, read the first time it is asked for (SVG, PDF and
    /// Illustrator files).
    fn art(&mut self) -> &Result<Artwork, String> {
        if self.art.as_ref().is_none_or(|(page, _)| *page != self.page) {
            let stem = self.stem().to_owned();
            let read = match &self.source {
                ImportSource::Svg(text) => drift_io::svg_import::read(text, &stem),
                ImportSource::Pdf(bytes) => {
                    drift_io::pdf_import::read_artwork(bytes, self.page, &stem)
                }
                ImportSource::Dxf(_) | ImportSource::Dwg(_) => {
                    Err("a CAD drawing has no artwork".into())
                }
            };
            self.art = Some((self.page, read));
        }
        &self.art.as_ref().expect("read").1
    }

    /// A DXF or DWG drawing read at 1 : `drawn_at`.
    fn read_cad(&self, drawn_at: f64) -> Result<DxfImport, String> {
        match &self.source {
            ImportSource::Dxf(text) => drift_io::dxf::read_at(text, self.unit_mm, drawn_at),
            ImportSource::Dwg(bytes) => {
                crate::underlays::read_dwg_at(bytes, self.unit_mm, drawn_at)
            }
            ImportSource::Svg(_) | ImportSource::Pdf(_) => Err("not a CAD drawing".into()),
        }
    }

    /// The DXF or DWG drawing at full size, read the first time it is asked for.
    fn cad(&mut self) -> &Result<DxfImport, String> {
        if self.cad.is_none() {
            self.cad = Some(self.read_cad(1.0));
        }
        self.cad.as_ref().expect("read")
    }

    /// The scale the page's dimensions show (bare figures in `unit_mm`), found once a page.
    /// Until a scale is chosen, it becomes the scale. None for CAD drawings.
    fn guess(&mut self, unit_mm: f64) -> Option<ScaleGuess> {
        if self.is_cad() {
            return None;
        }
        if self.guess.is_none_or(|(page, _)| page != self.page) {
            let found = self
                .art()
                .as_ref()
                .ok()
                .and_then(|art| drift_io::vector::guess_scale(art, unit_mm));
            if let Some(g) = found
                && !self.scale_chosen
            {
                self.scale = g.scale;
            }
            self.guess = Some((self.page, found));
        }
        self.guess.and_then(|(_, g)| g)
    }
}

/// Whether a file name is one Import takes.
pub(crate) fn importable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    crate::app::sheets_ui::IMPORT_FILE
        .extensions
        .iter()
        .any(|e| lower.ends_with(&format!(".{e}")))
}

impl DriftApp {
    /// Open the file picker for an import: File → Import… (`underlay` false), or the
    /// Underlays section's Import… (the dialog then starts on Underlay).
    pub(crate) fn pick_import_file(&mut self, underlay: bool) {
        self.import_as = Some(if underlay {
            ImportAs::Underlay
        } else {
            ImportAs::Geometry
        });
        // (Tests pick their files themselves: no system dialog.)
        #[cfg(not(test))]
        self.files.open(crate::app::sheets_ui::IMPORT_FILE);
    }

    /// A file opened to import: ask how it should come in (or, `as_` NewDrawing, bring it
    /// in at the defaults).
    pub(crate) fn begin_import(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        as_: Option<ImportAs>,
        now: f64,
    ) {
        let lower = name.to_ascii_lowercase();
        let illustrator = lower.ends_with(".ai");
        if illustrator && !bytes.starts_with(b"%PDF") {
            let why = if bytes.starts_with(b"%!PS") {
                "it's from Illustrator 8 or earlier, which saved PostScript: open it in a newer \
                 Illustrator and save it again, or save it as PDF or SVG"
            } else {
                "it isn't an Illustrator file Drift can read"
            };
            self.set_status(format!("Could not import {name}: {why}"), true, now);
            return;
        }
        let (source, pages) = if bytes.starts_with(b"%PDF") {
            match drift_io::pdf_import::page_count(&bytes) {
                Ok(0) => {
                    self.set_status(format!("{name} has no pages"), true, now);
                    return;
                }
                Ok(pages) => (ImportSource::Pdf(bytes), pages),
                Err(e) => {
                    self.set_status(format!("Could not import {name}: {e}"), true, now);
                    return;
                }
            }
        } else if lower.ends_with(".dxf") {
            (ImportSource::Dxf(drift_io::dxf::decode(&bytes)), 1)
        } else if lower.ends_with(".dwg") {
            (ImportSource::Dwg(bytes), 1)
        } else {
            (
                ImportSource::Svg(String::from_utf8_lossy(&bytes).into_owned()),
                1,
            )
        };
        let cad = matches!(source, ImportSource::Dxf(_) | ImportSource::Dwg(_));
        let unit_mm = self.editor.doc.units().length.mm_per_unit();
        let mut pending = PendingImport {
            name: name.to_owned(),
            source,
            pages,
            page: 1,
            underlay: as_ == Some(ImportAs::Underlay),
            scale: 1.0,
            // A CAD drawing's fill edges print, as they did in it.
            construction_edges: !cad,
            calibrate: false,
            images: true,
            illustrator,
            scale_chosen: false,
            unit_mm,
            art: None,
            guess: None,
            cad: None,
        };
        if as_ == Some(ImportAs::NewDrawing) {
            // The defaults: page 1, the scale its dimensions show.
            pending.guess(unit_mm);
            self.finish_import(&mut pending, now);
        } else {
            self.pending_import = Some(pending);
        }
    }

    /// The import dialog, while a file is waiting for it.
    pub(crate) fn import_dialog(&mut self, ctx: &egui::Context) {
        let units = self.editor.doc.units();
        let Some(pending) = &mut self.pending_import else {
            return;
        };
        let mut choice = None;
        let frame = theme::dialog_frame(&ctx.style_of(ctx.theme()));
        let modal = egui::Modal::new(egui::Id::new("vector_import"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                theme::dialog_title(ui, &format!("Import {}", pending.name), false);
                if let Some(underlay) = theme::segmented(
                    ui,
                    pending.underlay,
                    &[(false, "Editable geometry"), (true, "Underlay to trace")],
                ) {
                    pending.underlay = underlay;
                }
                ui.label(
                    egui::RichText::new(if pending.underlay {
                        "A drawing behind the sketch to snap to and trace over."
                    } else {
                        "Curves, fills and text on their layers, to edit."
                    })
                    .weak(),
                );
                ui.add_space(6.0);
                if pending.pages > 1 {
                    ui.horizontal(|ui| {
                        ui.label(if pending.illustrator {
                            "Artboard"
                        } else {
                            "Page"
                        });
                        ui.add(egui::DragValue::new(&mut pending.page).range(1..=pending.pages));
                        ui.label(format!("of {}", pending.pages));
                    });
                }
                // Found before the scale is shown, so it can set it.
                let guess = pending.guess(units.length.mm_per_unit());
                let before = pending.scale;
                ui.horizontal(|ui| {
                    ui.label("Drawn at");
                    let current = SCALES
                        .iter()
                        .find(|(_, n)| ((n - pending.scale) / n).abs() < 1e-9)
                        .map_or_else(
                            || drift_core::sheets::scale_label(pending.scale),
                            |(label, _)| (*label).to_owned(),
                        );
                    egui::ComboBox::from_id_salt("import_scale")
                        .selected_text(current)
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for (label, n) in SCALES {
                                ui.selectable_value(&mut pending.scale, *n, *label);
                            }
                        })
                        .response
                        .on_hover_text(
                            "The scale the drawing is at on its paper: 1:50 makes 1 mm on the \
                         paper 50 mm in the sketch",
                        );
                    ui.label("1 :");
                    ui.add(
                        egui::DragValue::new(&mut pending.scale)
                            .range(0.001..=100_000.0)
                            .speed(0.5)
                            .max_decimals(3),
                    );
                });
                if pending.scale != before {
                    pending.scale_chosen = true;
                }
                if pending.is_cad() {
                    ui.label(
                        egui::RichText::new(
                            "DWG and DXF drawings are usually drawn full size, at 1 : 1.",
                        )
                        .weak(),
                    );
                }
                if let Some(g) = guess {
                    let label = drift_core::sheets::scale_label(g.scale);
                    let sure = g.confidence();
                    let how = if g.written {
                        format!(
                            "written on the drawing ({} of {} scale labels)",
                            g.agreeing, g.measured
                        )
                    } else {
                        format!(
                            "from its dimensions: {} of {} agree{}",
                            g.agreeing,
                            g.measured,
                            if g.confirmed > 0 {
                                ", as its scale label says"
                            } else {
                                ""
                            }
                        )
                    };
                    let confidence = if sure >= 0.7 {
                        "high confidence"
                    } else if sure >= 0.4 {
                        "medium confidence"
                    } else {
                        "low confidence: its views may be at different scales"
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Suggested: {label}, {how} ({confidence})"
                            ))
                            .weak(),
                        );
                        if (pending.scale - g.scale).abs() > 1e-9
                            && ui.small_button("Use").clicked()
                        {
                            pending.scale = g.scale;
                        }
                    });
                    if let Some((other, n)) = g.runner_up {
                        ui.label(
                            egui::RichText::new(format!(
                                "Some views look to be at {} ({n}): check the scale is the one \
                             for the view you want",
                                drift_core::sheets::scale_label(other)
                            ))
                            .weak(),
                        );
                    }
                }
                ui.checkbox(
                    &mut pending.calibrate,
                    "Then calibrate: pick two points on it and type their real distance",
                )
                .on_hover_text("For a drawing whose scale you don't know, or to check it");
                let (ready, pictures) = if pending.is_cad() {
                    (cad_summary(ui, pending), false)
                } else {
                    art_summary(ui, pending, units)
                };
                // A PDF's pictures come in with it either way; an SVG's only as geometry.
                let pdf = matches!(pending.source, ImportSource::Pdf(_));
                if pictures && (pdf || !pending.underlay) {
                    ui.checkbox(&mut pending.images, "Bring in images as pictures")
                        .on_hover_text(
                            "Photos and scans in the file come in as pictures on an underlay \
                             beneath the drawing",
                        );
                }
                if !pending.underlay {
                    ui.add_space(6.0);
                    ui.label("Edges of shapes that are filled but not outlined:");
                    ui.radio_value(
                        &mut pending.construction_edges,
                        true,
                        "Construction: they shape the fill, and don't print",
                    );
                    ui.radio_value(
                        &mut pending.construction_edges,
                        false,
                        "Lines: they print, in the layer's style",
                    );
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                    let dark = ui.visuals().dark_mode;
                    if ui
                        .add_enabled(ready, theme::primary_button("Import", dark))
                        .clicked()
                    {
                        choice = Some(true);
                    }
                });
            });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        if choice == Some(true)
            && let Some(mut pending) = self.pending_import.take()
        {
            let now = ctx.input(|i| i.time);
            self.finish_import(&mut pending, now);
        } else if choice == Some(false) {
            self.pending_import = None;
        }
    }

    /// Bring the chosen page in, as the dialog says.
    pub(crate) fn finish_import(&mut self, pending: &mut PendingImport, now: f64) {
        let name = pending.name.clone();
        let scale = pending.scale;
        if pending.underlay {
            let result = match &pending.source {
                ImportSource::Pdf(bytes) => self.editor.import_pdf_underlay(
                    &name,
                    bytes,
                    pending.page,
                    scale,
                    pending.images,
                ),
                ImportSource::Svg(_) => match pending.art() {
                    Ok(art) => {
                        let art = art.clone();
                        self.editor.import_svg_underlay(&name, &art, scale)
                    }
                    Err(e) => Err(e.clone()),
                },
                ImportSource::Dxf(_) | ImportSource::Dwg(_) => {
                    pending.cad();
                    match pending.cad.take().expect("read") {
                        Ok(import) => self.editor.import_cad_underlay(&name, &import, scale),
                        Err(e) => Err(e),
                    }
                }
            };
            let ok = result.is_ok();
            self.underlay_imported(&name, result, now);
            if ok && pending.calibrate {
                self.set_tool(ToolKind::Calibrate);
            }
            return;
        }
        let label = match pending.source {
            ImportSource::Svg(_) => "Import SVG",
            ImportSource::Pdf(_) => "Import PDF",
            ImportSource::Dxf(_) => "Import DXF",
            ImportSource::Dwg(_) => "Import DWG",
        };
        let read = if pending.is_cad() {
            // Read again at its scale (it was read at full size for the dialog).
            let read = if (scale - 1.0).abs() < 1e-12 {
                pending.cad();
                pending.cad.take().expect("read")
            } else {
                pending.read_cad(scale)
            };
            read.map(|mut import| {
                import.fill_edges_construction = pending.construction_edges;
                (import, Vec::new())
            })
        } else {
            let options = ImportOptions {
                scale,
                construction_edges: pending.construction_edges,
            };
            let images = pending.images;
            pending.art().as_ref().map_err(Clone::clone).map(|art| {
                let pictures = if images {
                    art.images.clone()
                } else {
                    Vec::new()
                };
                (drift_io::vector::to_import(art, &options), pictures)
            })
        };
        let (import, images) = match read {
            Ok(read) => read,
            Err(e) => {
                self.set_status(format!("Could not import {name}: {e}"), true, now);
                return;
            }
        };
        self.abort_operation();
        let before: std::collections::BTreeSet<_> =
            self.editor.doc.iter().map(|(id, _)| id).collect();
        let texts_before: std::collections::BTreeSet<_> =
            self.editor.doc.texts().map(|(id, _)| id).collect();
        self.editor.doc.begin();
        let report = drift_io::dxf::add_to_document(&mut self.editor.doc, &import);
        // Its pictures come in as an underlay beneath it, in the same step.
        let pictures = crate::underlays::picture_underlay(&name, &images, scale, DVec2::ZERO)
            .map(|u| self.editor.doc.add_underlay(u));
        self.editor.commit(label);
        let imported = crate::underlays::Imported {
            underlays: pictures.into_iter().collect(),
            entities: self
                .editor
                .doc
                .iter()
                .map(|(id, _)| id)
                .filter(|id| !before.contains(id))
                .collect(),
            texts: self
                .editor
                .doc
                .texts()
                .map(|(id, _)| id)
                .filter(|id| !texts_before.contains(id))
                .collect(),
        };
        self.report_import(&name, &report, now);
        if pending.calibrate && !imported.entities.is_empty() {
            // What came in stays picked, to see what is being calibrated.
            self.editor.selection = imported
                .entities
                .iter()
                .copied()
                .filter(|id| self.editor.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
                .collect();
            self.set_tool(ToolKind::Calibrate);
            self.tool = Box::new(crate::tools::UnderlayTool::new(
                crate::tools::UnderlayMode::CalibrateImport,
            ));
        }
        self.editor.last_import = Some(imported);
    }

    /// A drawing dropped on the window: open it as a new drawing, import it into this one,
    /// or trace over it. The import dialog follows for the last two.
    pub(crate) fn drop_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.dropped else {
            return;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mut choice = None;
        let frame = theme::dialog_frame(&ctx.style_of(ctx.theme()));
        let modal = egui::Modal::new(egui::Id::new("dropped_file"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                theme::dialog_title(ui, &format!("Open {name}"), false);
                ui.label("How should it come in?");
                ui.add_space(6.0);
                let wide = egui::vec2(ui.available_width(), 0.0);
                let options = [
                    (
                        DropChoice::Open,
                        "Open as new drawing",
                        "A new drawing of its own, with its curves, fills and text to edit",
                    ),
                    (
                        DropChoice::Import(ImportAs::Geometry),
                        "Import as geometry",
                        "Into this drawing, as curves, fills and text to edit",
                    ),
                    (
                        DropChoice::Import(ImportAs::Underlay),
                        "Underlay to trace",
                        "Behind this drawing, to snap to and trace over",
                    ),
                ];
                for (what, label, hint) in options {
                    if ui
                        .add(egui::Button::new(label).min_size(wide))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        choice = Some(what);
                    }
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(DropChoice::Cancel);
                    }
                });
            });
        if modal.should_close() && choice.is_none() {
            choice = Some(DropChoice::Cancel);
        }
        let Some(choice) = choice else {
            return;
        };
        let path = self.dropped.take().expect("dropped");
        match choice {
            DropChoice::Open => {
                // Asks first when the drawing open now has unsaved changes.
                self.open_as_new = Some(path);
                self.request(Pending::OpenDropped);
            }
            DropChoice::Import(as_) => {
                self.import_as = Some(as_);
                self.files.open_path(&path);
            }
            DropChoice::Cancel => {}
        }
    }

    /// Import a file as editable geometry at the defaults, without asking.
    #[cfg(test)]
    pub(crate) fn import_now(&mut self, name: &str, bytes: Vec<u8>, now: f64) {
        self.begin_import(name, bytes, Some(ImportAs::NewDrawing), now);
    }
}

/// What the artwork of an SVG, PDF or Illustrator file holds, and what won't come across.
/// Whether it can be imported, and whether it has pictures.
fn art_summary(
    ui: &mut egui::Ui,
    pending: &mut PendingImport,
    units: drift_core::Units,
) -> (bool, bool) {
    let scale = pending.scale;
    let underlay = pending.underlay;
    match pending.art() {
        Ok(art) => {
            let paths = art.paths.len();
            let texts = art.texts.len();
            let size = art.size;
            let mut about = if size.x > 0.0 && size.y > 0.0 {
                format!(
                    "Paper {} × {} mm, {} in the sketch.",
                    fmt_mm(size.x),
                    fmt_mm(size.y),
                    if (scale - 1.0).abs() < 1e-12 {
                        "the same".to_owned()
                    } else {
                        // Metric lengths are written without their unit.
                        let unit = if units.length.is_metric() {
                            format!(" {}", units.length.symbol())
                        } else {
                            String::new()
                        };
                        format!(
                            "{} × {}{unit}",
                            units.format_length(size.x * scale),
                            units.format_length(size.y * scale)
                        )
                    }
                )
            } else {
                String::new()
            };
            about.push_str(&format!(
                " {paths} path{}{}, on {} layer{}.",
                if paths == 1 { "" } else { "s" },
                if texts > 0 {
                    format!(" and {texts} text{}", if texts == 1 { "" } else { "s" })
                } else {
                    String::new()
                },
                art.layers.len().max(1),
                if art.layers.len() > 1 { "s" } else { "" }
            ));
            ui.label(egui::RichText::new(about.trim()).weak());
            // What won't come across as it looks, said before it's imported.
            let mut heads_up: Vec<String> = art
                .notes
                .iter()
                .filter(|n| !n.starts_with("Page ") && !n.starts_with("Sized as"))
                .cloned()
                .collect();
            heads_up.extend(
                art.skipped
                    .iter()
                    .map(|(what, n)| format!("{n} × {what} left out")),
            );
            let fits = curve_limits(ui, &mut heads_up, art.curve_count(), underlay);
            (paths + texts > 0 && fits, !art.images.is_empty())
        }
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e.as_str());
            (false, false)
        }
    }
}

/// What a DXF or DWG drawing holds, and what won't come across. Whether it can be imported.
fn cad_summary(ui: &mut egui::Ui, pending: &mut PendingImport) -> bool {
    let underlay = pending.underlay;
    match pending.cad() {
        Ok(import) => {
            let curves = import.entities.len();
            let texts = import.texts.len();
            let copies = import.inserts.len();
            let mut about = format!("{curves} curve{}", if curves == 1 { "" } else { "s" });
            if texts > 0 {
                about.push_str(&format!(
                    ", {texts} text{}",
                    if texts == 1 { "" } else { "s" }
                ));
            }
            if copies > 0 {
                about.push_str(&format!(
                    ", {copies} block cop{}",
                    if copies == 1 { "y" } else { "ies" }
                ));
            }
            let layers = import.layers.len().max(1);
            about.push_str(&format!(
                ", on {layers} layer{}.",
                if layers > 1 { "s" } else { "" }
            ));
            ui.label(egui::RichText::new(about).weak());
            let mut heads_up = import.notes.clone();
            heads_up.extend(
                import
                    .skipped
                    .iter()
                    .map(|(what, n)| format!("{n} × {what} left out")),
            );
            let fits = curve_limits(ui, &mut heads_up, curves, underlay);
            let any = curves + texts + copies + import.hatches.len() > 0;
            any && fits
        }
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e.as_str());
            false
        }
    }
}

/// Say what won't come across, and warn of (or refuse) more curves than a sketch can take.
/// Whether it fits.
fn curve_limits(
    ui: &mut egui::Ui,
    heads_up: &mut Vec<String>,
    curves: usize,
    underlay: bool,
) -> bool {
    let too_many = !underlay && curves > TOO_MANY_CURVES;
    if !underlay && curves > MANY_CURVES && !too_many {
        heads_up.push(format!(
            "About {curves} curves: the import may take a while, and the sketch be slow to \
             work in (an underlay is lighter)"
        ));
    }
    if !heads_up.is_empty() {
        let amber = egui::Color32::from_rgb(0xff, 0x9f, 0x0a);
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Before you import").strong());
        for line in heads_up.iter() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(amber, "⚠");
                ui.label(line);
            });
        }
    }
    if too_many {
        ui.add_space(4.0);
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!(
                "About {curves} curves is more than a sketch can work with (over \
                 {TOO_MANY_CURVES}): import it as an underlay to trace, or split the drawing \
                 first"
            ),
        );
    }
    !too_many
}

use crate::app::underlays_ui::SCALES;

/// Above this many curves an import as geometry is warned about, and above the second it is
/// refused (an underlay takes it).
const MANY_CURVES: usize = 50_000;
const TOO_MANY_CURVES: usize = 400_000;

/// Millimetres on paper, to a tenth.
fn fmt_mm(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_owned)
}
