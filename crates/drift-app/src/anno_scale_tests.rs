//! Annotations at print size: the canvas shows dimensions and notes exactly as a sheet view
//! at the drawing scale prints them (the same size in the model at any zoom), text greeked
//! when too small to read and set on the raster ladder otherwise; and text prints at the cap
//! height it is set to.

use drift_core::{
    Anchor, Annotation, Constraint, DVec2, Document, DraftStyle, Leader, LeaderEnd, LeaderShape,
    OrdinateAxis,
};
use eframe::egui::{self, Pos2, Rect, vec2};

use crate::annotations::{
    AnnoSpace, AnnotationColors, AnnotationState, GREEK_PT, PT_PER_MM, Recorded, Recorder, ladder,
};
use crate::overlay::ScreenMap;

/// A painter for laying out text, on a context that never draws.
fn painter() -> egui::Painter {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
    egui::Painter::new(
        ctx,
        egui::LayerId::background(),
        Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0)),
    )
}

/// `painter`, clipped to nothing: everything drawn is kept.
fn unclipped(painter: &egui::Painter) -> egui::Painter {
    egui::Painter::new(
        painter.ctx().clone(),
        egui::LayerId::background(),
        Rect::EVERYTHING,
    )
}

/// A plate at 1 : `n` with every kind of dimension, a note with a shouldered leader, an
/// ordinate, a section and an elevation mark.
fn drawing(n: f64) -> Document {
    let mut doc = Document::new();
    doc.set_draft_style(DraftStyle {
        scale: n,
        ..doc.draft_style()
    });
    let p = |doc: &mut Document, x: f64, y: f64| doc.add_point(DVec2::new(x, y));
    let (a, b, c) = (
        p(&mut doc, 0.0, 0.0),
        p(&mut doc, 400.0, 0.0),
        p(&mut doc, 400.0, 300.0),
    );
    let base = doc.add_line(a, b).unwrap();
    let side = doc.add_line(b, c).unwrap();
    let centre = p(&mut doc, 200.0, 150.0);
    let hole = doc.add_circle(centre, 60.0).unwrap();
    let dims = [
        Constraint::Distance { a, b, value: 400.0 },
        Constraint::HorizontalDistance {
            a,
            b: c,
            value: 400.0,
        },
        Constraint::VerticalDistance {
            a: b,
            b: c,
            value: 300.0,
        },
        Constraint::Radius {
            curve: hole,
            value: 60.0,
        },
        Constraint::Diameter {
            curve: hole,
            value: 120.0,
        },
        Constraint::Angle {
            a: base,
            b: side,
            degrees: 90.0,
        },
    ];
    for (k, d) in dims.into_iter().enumerate() {
        let id = doc.add_constraint(d).unwrap();
        // Half of them placed (paper mm), half left where they go by default.
        if k % 2 == 0 {
            doc.set_placement(id, DVec2::new(3.0, -9.0)).unwrap();
        }
    }
    doc.add_annotation(Annotation::Note {
        text: "DEBURR ALL EDGES".into(),
        at: DVec2::new(520.0, 380.0),
        leaders: vec![Leader {
            to: Anchor::Free(DVec2::new(400.0, 300.0)),
            end: LeaderEnd::Arrow,
            shape: LeaderShape::Straight,
            shoulder: true,
            shoulder_len: Leader::SHOULDER,
        }],
        style: Default::default(),
    })
    .unwrap();
    doc.add_annotation(Annotation::Ordinate {
        origin: a,
        point: c,
        axis: OrdinateAxis::X,
        at: -40.0,
    })
    .unwrap();
    doc.add_annotation(Annotation::Section {
        a: DVec2::new(-50.0, 150.0),
        b: DVec2::new(450.0, 150.0),
        label: "A".into(),
        sheet: "A-301".into(),
        flip: false,
        view: None,
    })
    .unwrap();
    doc.add_annotation(Annotation::Elevation {
        at: DVec2::new(200.0, -80.0),
        angle: 90.0,
        label: "1".into(),
        sheet: String::new(),
        view: None,
    })
    .unwrap();
    doc
}

/// The canvas: 1200 × 800 points about (600, 400), `zoom` points per mm.
fn canvas(zoom: f64) -> ScreenMap {
    ScreenMap {
        rect: Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0)),
        camera: drift_render::Camera {
            center: DVec2::new(180.0, 120.0),
            scale: zoom,
        },
    }
}

/// A 1 : `n` sheet's page about the same model point, as `output::annotations` lays out.
fn sheet(n: f64) -> ScreenMap {
    ScreenMap {
        rect: Rect::from_center_size(Pos2::ZERO, vec2(1.0, 1.0)),
        camera: drift_render::Camera {
            center: DVec2::new(180.0, 120.0),
            scale: f64::from(PT_PER_MM) / n,
        },
    }
}

/// Everything drawn for the drawing's dimensions and annotations through `space`, on `pen`.
fn record(painter: &egui::Painter, doc: &Document, space: &AnnoSpace) -> Vec<Recorded> {
    let recorder = Recorder::new(painter);
    let colors = AnnotationColors::new(false);
    for (id, c) in doc.constraints().filter(|(_, c)| c.is_dimension()) {
        let l = space
            .layout_dimension(painter, doc, Some(id), c, space.placement(doc, id))
            .expect("laid out");
        space.paint_dimension(&recorder, &l, AnnotationState::Normal, &colors);
    }
    for l in space.layout_annotations(painter, doc) {
        let style = doc.draft_style_for(drift_core::AnnotationRef::Annotation(l.id));
        space.paint_annotation(&recorder, &l, AnnotationState::Normal, &colors, &style);
    }
    recorder.take()
}

/// What was drawn, in the model: points (mm), and sizes (mm, for dots and text).
#[derive(Debug)]
enum Drawn {
    Points(&'static str, Vec<DVec2>),
    Dot(DVec2, f64),
    Text { at: DVec2, cap: f64, text: String },
}

fn in_model(items: Vec<Recorded>, map: &ScreenMap, em_to_cap: f64) -> Vec<Drawn> {
    let w = |p: Pos2| map.to_world(p);
    let mm = |l: f32| f64::from(l) / map.camera.scale;
    items
        .into_iter()
        .map(|r| match r {
            Recorded::Line(p, _) => Drawn::Points("line", p.into_iter().map(w).collect()),
            Recorded::Polygon(p) => Drawn::Points("polygon", p.into_iter().map(w).collect()),
            Recorded::Mask(p) => Drawn::Points("mask", p.into_iter().map(w).collect()),
            Recorded::Dot(at, r) => Drawn::Dot(w(at), mm(r)),
            Recorded::Text {
                baseline,
                size,
                text,
                ..
            } => Drawn::Text {
                at: w(baseline),
                cap: mm(size) * em_to_cap,
                text,
            },
        })
        .collect()
}

/// The canvas at print size draws what the sheet prints, the same size in the model, at any
/// zoom and drawing scale: every line, arrowhead, mask and dot exactly, and every text at the
/// cap height it prints at (in the screen's font, corrected to the drawing font's capitals),
/// within 2 %.
#[test]
fn the_canvas_at_print_size_draws_what_the_sheet_prints() {
    canvas_draws_what_the_sheet_prints(&painter());
}

/// The same with the drawing's own font, which the screen shows too.
#[test]
fn with_a_drawing_font_the_canvas_draws_what_the_sheet_prints() {
    let painter = with_drawing_font();
    assert!((crate::fonts::screen_cap_correction() - 1.0).abs() < 1e-6);
    canvas_draws_what_the_sheet_prints(&painter);
    drawn_capitals_stand_as_high_as_they_print(&painter);
    crate::fonts::reset_screen();
}

/// A capital H drawn on the canvas at print size stands as high as it prints (its glyph's
/// mesh on screen, against the page's cap height times the zoom), within 2 %.
#[test]
fn drawn_capitals_stand_as_high_as_printed() {
    drawn_capitals_stand_as_high_as_they_print(&painter());
}

fn drawn_capitals_stand_as_high_as_they_print(painter: &egui::Painter) {
    use crate::annotations::{ShownText, Zoom, ZoomPen};
    // At zooms that set the text right on a raster ladder step (64 and 90.5 points: no
    // scaling the rest of the way), the H's glyph on screen is no more than ¾ of a pixel
    // smaller than it prints (the rasteriser rounds its edges, mostly out, by up to a few
    // pixels): tight enough that leaving out the screen font's cap correction (2.5 % with
    // Windows' interface font: a pixel at 64 points) fails it. The exact guard of the
    // correction is the canvas-vs-sheet digest, which compares cap heights without pixels.
    for step in [
        crate::annotations::ladder(64.0),
        crate::annotations::ladder(90.0),
    ] {
        let em = crate::annotations::paper_font(3.5);
        let paper = painter.layout_no_wrap(
            "H".into(),
            crate::fonts::drafting_font(em, false),
            egui::Color32::WHITE,
        );
        let k = step / (em * crate::fonts::screen_cap_correction());
        let pen = ZoomPen {
            inner: painter,
            zoom: Zoom { c: Pos2::ZERO, k },
        };
        let ShownText::Set(shown) = pen.shown_text(Pos2::ZERO, &paper, 0.0) else {
            panic!("set, not greeked");
        };
        let (lo, hi) = shown.rows[0]
            .row
            .visuals
            .mesh
            .vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                (lo.min(v.pos.y), hi.max(v.pos.y))
            });
        let over = (hi - lo) - 3.5 * PT_PER_MM * k;
        assert!(
            (-0.75..=3.0).contains(&over),
            "at the {step}-point step: an H {over} points off its printed height"
        );
    }
    let em = crate::annotations::paper_font(3.5);
    let paper = painter.layout_no_wrap(
        "H".into(),
        crate::fonts::drafting_font(em, false),
        egui::Color32::WHITE,
    );
    for k in [20.0f32, 37.0, 60.0] {
        let pen = ZoomPen {
            inner: painter,
            zoom: Zoom { c: Pos2::ZERO, k },
        };
        let ShownText::Set(shown) = pen.shown_text(Pos2::ZERO, &paper, 0.0) else {
            panic!("set, not greeked");
        };
        let row = &shown.rows[0].row;
        let (lo, hi) = row
            .visuals
            .mesh
            .vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                (lo.min(v.pos.y), hi.max(v.pos.y))
            });
        let drawn = hi - lo;
        let printed = 3.5 * PT_PER_MM * k;
        // The rasteriser rounds a glyph's bounds out to whole pixels of the raster size, which
        // the scaling the rest of the way enlarges: up to two of those on top of the 2 %
        // (macOS and Linux fonts round further out than Windows').
        let size = shown.job.sections[0].format.font_id.size;
        let raster_px = size / crate::annotations::ladder(size);
        assert!(
            (drawn - printed).abs() <= printed * 0.02 + 2.0 * raster_px,
            "×{k}: an H {drawn} points high on screen, {printed} printed (raster pixel {raster_px})"
        );
    }
}

/// A painter whose screen font is a drawing font the drawing keeps (egui's own Ubuntu), the
/// drawing font set for printing too.
fn with_drawing_font() -> egui::Painter {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let data = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
        .font
        .to_vec();
    let mut doc = Document::new();
    let mut fonts = doc.fonts().clone();
    fonts.embedded.insert(
        "Ubuntu".into(),
        drift_core::EmbeddedFont {
            regular: drift_core::FontFile::new(data, 0),
            bold: None,
        },
    );
    fonts.drawing = Some("Ubuntu".into());
    doc.set_fonts(fonts);
    let editor = drift_edit::editor::Editor::new(doc);
    let mut state = crate::fonts::ScreenFont::default();
    for _ in 0..2 {
        crate::fonts::sync_screen(&ctx, &editor, &mut state);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
    }
    crate::fonts::sync_screen(&ctx, &editor, &mut state);
    assert!(
        matches!(state, crate::fonts::ScreenFont::Ready(_)),
        "{state:?}"
    );
    egui::Painter::new(
        ctx,
        egui::LayerId::background(),
        Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0)),
    )
}

fn canvas_draws_what_the_sheet_prints(painter: &egui::Painter) {
    // (A clip that takes everything in: the canvas leaves out text off screen.)
    let painter = unclipped(painter);
    let draw_cap = drift_draft::font::cap_ratio();
    let screen_cap = crate::fonts::screen_cap_ratio();
    for n in [1.0, 50.0, 0.5] {
        let doc = drawing(n);
        // Zoomed so the text stands from 6 to 300 points: set, never greeked.
        let print_text = f64::from(crate::annotations::paper_font(3.5)) / PT_PER_MM as f64;
        for text_pt in [6.0, 24.0, 300.0] {
            let zoom = text_pt / (print_text * n);
            let map = canvas(zoom);
            let on_canvas = in_model(
                record(&painter, &doc, &AnnoSpace::print(&map, n)),
                &map,
                screen_cap,
            );
            let page = sheet(n);
            let on_sheet = in_model(
                record(&painter, &doc, &AnnoSpace::sheet(&page, n)),
                &page,
                draw_cap,
            );
            let at = format!("1:{n} at {text_pt} pt");
            assert_eq!(on_canvas.len(), on_sheet.len(), "{at}: what's drawn");
            // A point's place to within a hundredth of a printed millimetre.
            let tol = 0.01 * n;
            let near = |a: DVec2, b: DVec2| (a - b).length() <= tol;
            for (c, s) in on_canvas.iter().zip(&on_sheet) {
                match (c, s) {
                    (Drawn::Points(kc, pc), Drawn::Points(ks, ps)) => {
                        assert_eq!(kc, ks, "{at}");
                        assert_eq!(pc.len(), ps.len(), "{at}: {kc}");
                        for (a, b) in pc.iter().zip(ps) {
                            assert!(near(*a, *b), "{at}: {kc} {a} vs {b}");
                        }
                    }
                    (Drawn::Dot(a, ra), Drawn::Dot(b, rb)) => {
                        assert!(near(*a, *b) && (ra - rb).abs() <= tol, "{at}: dot");
                    }
                    (
                        Drawn::Text {
                            at: a,
                            cap: ca,
                            text: ta,
                        },
                        Drawn::Text {
                            at: b,
                            cap: cb,
                            text: tb,
                        },
                    ) => {
                        assert_eq!(ta, tb, "{at}");
                        assert!(
                            (ca / cb - 1.0).abs() <= 0.02,
                            "{at}: {ta:?} cap {ca} on the canvas, {cb} printed"
                        );
                        // Its baseline where it prints (the canvas sets its text on it), to within
                        // 2 % of its height.
                        assert!(
                            (*a - *b).length() <= cb * 0.02 + tol,
                            "{at}: {ta:?} {a} {b}"
                        );
                    }
                    _ => panic!("{at}: {c:?} drawn where the sheet has {s:?}"),
                }
            }
        }
    }
}

/// Dimension text printed at a "3.5 mm" text height has capitals 3.5 mm high (and the
/// drawing scale doesn't change it).
#[test]
fn dimension_text_prints_at_its_cap_height() {
    let painter = painter();
    for n in [1.0, 50.0] {
        let doc = drawing(n);
        let page = sheet(n);
        let caps: Vec<f64> = record(&painter, &doc, &AnnoSpace::sheet(&page, n))
            .into_iter()
            .filter_map(|r| match r {
                Recorded::Text { size, text, .. } if !text.trim().is_empty() => Some(
                    // The page's text height (cap) in mm, as output.rs writes it.
                    f64::from(size) / f64::from(PT_PER_MM) * drift_draft::font::cap_ratio(),
                ),
                _ => None,
            })
            .collect();
        // Dimensions and the note at the style's 3.5; the marks' smaller lettering apart.
        assert!(
            caps.iter().filter(|c| (*c - 3.5).abs() < 1e-6).count() >= 6,
            "{caps:?}"
        );
    }
}

/// Text too small to read on screen is greeked (bars, no letters); text on screen is set on
/// the ladder and scaled the rest of the way, so it is the size it should be.
#[test]
fn small_text_is_greeked_and_the_rest_set_on_the_ladder() {
    let painter = painter();
    let doc = drawing(1.0);
    let map = canvas(0.05);
    let items = record(&painter, &doc, &AnnoSpace::print(&map, 1.0));
    assert!(
        !items.iter().any(|r| matches!(r, Recorded::Text { .. })),
        "no letters when they'd be under {GREEK_PT} pt"
    );
    assert!(items.iter().any(|r| matches!(r, Recorded::Polygon(_))));
    // Zoomed in, the labels off the canvas aren't set at all; those on it are.
    let texts = |painter: &egui::Painter| {
        record(painter, &doc, &AnnoSpace::print(&canvas(20.0), 1.0))
            .into_iter()
            .filter(|r| matches!(r, Recorded::Text { .. }))
            .count()
    };
    let (all, on_canvas) = (texts(&unclipped(&painter)), texts(&painter));
    assert!(0 < on_canvas && on_canvas < all, "{on_canvas} of {all} set");
    // The ladder: steps of 2^¼ from 4 to 96 points.
    assert_eq!(ladder(1.0), GREEK_PT);
    assert_eq!(ladder(1000.0), 96.0);
    let step = ladder(13.0) / ladder(11.0);
    assert!((step - 2f32.powf(0.25)).abs() < 1e-4, "{step}");
    // Zooming within a step reuses the raster size.
    assert_eq!(ladder(12.6), ladder(14.0));
}

/// Exported to PDF, a sheet's dimension text has capitals as high as its text height says:
/// "3.5 mm" text at 3.5 mm (its font size in the PDF, times the font's cap height).
#[test]
fn a_pdf_prints_dimension_text_at_its_cap_height() {
    use drift_core::sheets::{Sheet, View, Viewport};
    let mut doc = drawing(10.0);
    let view = doc.add_view(View::new(
        "Plan",
        drift_core::Bounds {
            min: DVec2::new(-200.0, -200.0),
            max: DVec2::new(700.0, 500.0),
        },
        10.0,
    ));
    let mut sheet = Sheet::new("A-101");
    sheet
        .viewports
        .push(Viewport::new(view, DVec2::new(200.0, 150.0)));
    doc.add_sheet(sheet.clone());
    let views = doc.placed_views(&sheet);
    let mut ed = drift_edit::editor::Editor::new(doc);
    let page = crate::output::sheet_drawing(
        &mut ed,
        &drift_edit::fills::PatternLibrary::default(),
        &painter(),
        &sheet,
        &views,
        (1, 1),
    );
    let pdf = drift_io::pdf::write(&[page], "Caps");
    // Every stream; every font size the pages set text in (points).
    let mut sizes = Vec::new();
    let mut rest = &pdf[..];
    while let Some(at) = find(
        rest, b"stream
",
    ) {
        let body = &rest[at + 7..];
        let end = find(
            body,
            b"
endstream",
        )
        .expect("a stream's end");
        // (Page contents are written as they are; fonts and pictures packed.)
        let raw = &body[..end];
        let unpacked = miniz_oxide::inflate::decompress_to_vec_zlib(raw);
        {
            let text = String::from_utf8_lossy(unpacked.as_deref().unwrap_or(raw));
            let words: Vec<&str> = text.split_whitespace().collect();
            for w in words.windows(2) {
                if w[1] == "Tf" {
                    sizes.push(w[0].parse::<f64>().expect("a size"));
                }
            }
        }
        // (Past its `endstream`, whose tail would read as another `stream`.)
        rest = &body[end
            + b"
endstream"
                .len()..];
    }
    // Helvetica (no drawing font): capitals 0.718 of the size.
    let caps: Vec<f64> = sizes
        .iter()
        .map(|s| s * drift_draft::page::CAP_HEIGHT * 25.4 / 72.0)
        .collect();
    let at_3_5 = caps.iter().filter(|c| (*c - 3.5).abs() < 0.01).count();
    assert!(at_3_5 >= 6, "caps {caps:?}");
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A placed label is the same distance from what it measures on every sheet, whatever the
/// view's scale: 12 paper mm on a 1:20 detail, a 1:100 plan and a 2:1 enlargement.
#[test]
fn a_placed_label_keeps_its_paper_distance_at_every_view_scale() {
    let painter = painter();
    let mut doc = Document::new();
    let a = doc.add_point(DVec2::new(0.0, 0.0));
    let b = doc.add_point(DVec2::new(400.0, 0.0));
    doc.add_line(a, b).unwrap();
    let id = doc
        .add_constraint(Constraint::Distance { a, b, value: 400.0 })
        .unwrap();
    doc.set_placement(id, DVec2::new(0.0, 12.0)).unwrap();
    let c = *doc.constraint(id).unwrap();
    for n in [20.0, 100.0, 0.5] {
        let page = sheet(n);
        let space = AnnoSpace::sheet(&page, n);
        let l = crate::annotations::layout_dimension(
            &painter,
            &doc,
            Some(id),
            &c,
            space.placement(&doc, id),
            &space,
        )
        .expect("laid out");
        // The dimension line (where its label's handle is) above the measured line, on the
        // page.
        let measured = page.to_screen(DVec2::new(200.0, 0.0));
        let paper_mm = f64::from(measured.y - l.handle.y) / f64::from(PT_PER_MM);
        assert!((paper_mm - 12.0).abs() < 1e-3, "1:{n}: {paper_mm} mm");
    }
}

/// A new sheet's view is at the drawing scale when the drawing fits at it (so the sheet
/// prints what the canvas shows), else at the largest standard scale that fits.
#[test]
fn a_new_sheet_view_is_at_the_drawing_scale_when_it_fits() {
    let sheet = drift_core::Sheet::new("A-101");
    let plate = drift_core::Bounds {
        min: DVec2::ZERO,
        max: DVec2::new(600.0, 400.0),
    };
    // On A3 it fits at 1:10 (kept), not at 1:1; 1:2 is the largest standard scale that fits.
    let (view, _) = crate::output::sheet_view(plate, &sheet, false, 10.0);
    assert_eq!(view.scale, 10.0);
    let (view, _) = crate::output::sheet_view(plate, &sheet, false, 1.0);
    assert_eq!(view.scale, 2.0);
}

/// A drawing without sheets prints on a page of its own, through a view the drawing doesn't
/// keep: its dimensions are on it (the rule sheet views print by applies to the drawing's own
/// views only).
#[test]
fn a_drawing_without_sheets_prints_its_dimensions() {
    let mut doc = Document::new();
    let a = doc.add_point(DVec2::new(0.0, 0.0));
    let b = doc.add_point(DVec2::new(1234.0, 0.0));
    doc.add_line(a, b).unwrap();
    doc.add_constraint(Constraint::Distance {
        a,
        b,
        value: 1234.0,
    })
    .unwrap();
    let mut ed = drift_edit::editor::Editor::new(doc);
    let mut patterns = drift_edit::fills::PatternLibrary::default();
    let pages = crate::output::sheet_pages(&mut ed, &mut patterns, &painter(), "Plan");
    let printed = pages[0].items.iter().any(
        |i| matches!(&i.prim, drift_draft::page::Prim::Text { text, .. } if text.contains("1234")),
    );
    assert!(printed, "the dimension's 1234 on the page");
}
