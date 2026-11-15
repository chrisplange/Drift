//! A sheet's own annotations (#432) laid out and drawn: notes and leaders in paper space,
//! dimensions through the viewport they measure in. The layout and paint code is the model's
//! ([`crate::annotations::layout_dimension_own`], [`crate::notes::layout_note`]); what is
//! new here is where the points come from.

use drift_core::{
    AnnotationId, Constraint, DVec2, Document, DraftStyle, LeaderShape, PlacedView, Sheet,
    SheetAnnotation, SheetAnnotationId,
};
use eframe::egui::{self, Pos2};

use crate::annotations::{
    self, AnnoSpace, AnnotationColors, AnnotationState, DimLayout, DimOwn, Pen,
};
use crate::notes::{self, NoteLayout};
use crate::overlay::ScreenMap;

/// A sheet annotation laid out on a map of the paper.
#[derive(Clone, Debug)]
pub enum SheetShape {
    Note(NoteLayout),
    Dimension(DimLayout),
}

/// A sheet annotation laid out where it is drawn, with what painting it needs.
#[derive(Clone, Debug)]
pub struct SheetLayout {
    pub id: SheetAnnotationId,
    pub shape: SheetShape,
    /// The space it was laid out in (the paper's for a note, its viewport's for a dimension).
    pub space: AnnoSpace,
    /// The drafting style it is drawn in.
    pub style: DraftStyle,
}

impl SheetLayout {
    /// Whether `p` (on the map it was laid out for) picks it.
    pub fn hit(&self, p: Pos2) -> bool {
        match &self.shape {
            SheetShape::Note(n) => n.hit(p),
            SheetShape::Dimension(d) => {
                d.text_rect.expand(3.0).contains(p)
                    || dimension_segments(d).iter().any(|[a, b]| {
                        let ab = *b - *a;
                        let t = if ab.length_sq() > 0.0 {
                            ((p - *a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        (*a + ab * t - p).length() <= 4.0
                    })
            }
        }
    }

    /// Paint it with `pen` (the screen's painter, or a recorder).
    pub fn paint(&self, pen: &dyn Pen, state: AnnotationState, colors: &AnnotationColors) {
        match &self.shape {
            SheetShape::Note(n) => {
                self.space
                    .paint_annotation(pen, n, state, colors, &self.style);
            }
            SheetShape::Dimension(d) => self.space.paint_dimension(pen, d, state, colors),
        }
    }
}

/// A dimension's lines, for picking: its dimension line and extension lines.
fn dimension_segments(d: &DimLayout) -> Vec<[Pos2; 2]> {
    use crate::annotations::DimShape;
    match &d.shape {
        DimShape::Linear { p, d: line, .. } => {
            vec![[line[0], line[1]], [p[0], line[0]], [p[1], line[1]]]
        }
        DimShape::Radius { tip, from, .. } => vec![[*tip, *from], [*from, d.text_pos]],
        DimShape::Diameter { ends } => vec![[ends[0], ends[1]]],
        DimShape::Angular { .. } => Vec::new(),
    }
}

/// The map of the model as viewport `view` shows it on a map of the paper (`paper`: world
/// is paper mm): a model point lands where the paper map puts its place on the paper.
pub fn port_map(paper: &ScreenMap, view: &PlacedView) -> ScreenMap {
    let n = positive(view.scale);
    ScreenMap {
        rect: paper.rect,
        camera: drift_render::Camera {
            center: view.center - (view.frame.at - paper.camera.center) * n,
            scale: paper.camera.scale / n,
        },
    }
}

fn positive(n: f64) -> f64 {
    if n.is_finite() && n > 0.0 { n } else { 1.0 }
}

/// What a sheet dimension measures now, with the measurement as its value (`None` while its
/// geometry can't be measured).
pub fn measured(doc: &Document, measure: &Constraint) -> Option<Constraint> {
    drift_solver::measure(doc, measure).map(|v| measure.with_value(v))
}

/// The space annotations at 1 : `n` are laid out in on `map`: as a sheet prints them. A map
/// already at the page's own size (a point per paper mm at 96 dpi: what output records on)
/// is laid out on directly, as the views' own annotations are; any other (the sheet on
/// screen, at its zoom) is laid out at the page's size and scaled onto it.
fn space_on(map: &ScreenMap, n: f64) -> AnnoSpace {
    let page = f64::from(annotations::PT_PER_MM) / n;
    if (map.camera.scale - page).abs() <= 1e-9 * page {
        AnnoSpace::sheet(map, n)
    } else {
        AnnoSpace::print(map, n)
    }
}

/// The space a dimension through `view` is laid out in on `paper`: as a sheet prints it at
/// the view's scale, on the paper map (whatever its zoom).
pub fn port_space(paper: &ScreenMap, view: &PlacedView) -> AnnoSpace {
    space_on(&port_map(paper, view), positive(view.scale))
}

/// The space notes are laid out in on `paper`: the paper at full size, on the map.
pub fn paper_space(paper: &ScreenMap) -> AnnoSpace {
    space_on(paper, 1.0)
}

/// A sheet dimension laid out through `view` on `paper` (`None` while what it measures
/// can't be).
pub fn layout_sheet_dimension(
    painter: &egui::Painter,
    doc: &Document,
    paper: &ScreenMap,
    view: &PlacedView,
    measure: &Constraint,
    label: Option<DVec2>,
    own: &DimOwn,
) -> Option<(DimLayout, AnnoSpace)> {
    let space = port_space(paper, view);
    let c = measured(doc, measure)?;
    // Its label offset is kept in paper mm: world mm at this view's scale.
    let placement = label.map(|l| space.label_world(l));
    let layout =
        annotations::layout_dimension_own(painter, doc, None, &c, placement, &space, Some(own))?;
    Some((space.dim_to_screen(layout), space))
}

/// One annotation of `sheet` laid out on `paper` (`None` for one that can't be drawn: its
/// view or geometry is missing, which a document never keeps).
pub fn layout_sheet_annotation(
    painter: &egui::Painter,
    doc: &Document,
    sheet: &Sheet,
    paper: &ScreenMap,
    id: SheetAnnotationId,
    a: &SheetAnnotation,
) -> Option<SheetLayout> {
    let style = a.look().apply(&doc.draft_style());
    match a {
        SheetAnnotation::Dimension {
            port,
            measure,
            label,
            note,
            ..
        } => {
            let view = doc.port_view(sheet, *port)?;
            let own = DimOwn {
                style,
                note: note.clone(),
            };
            let (layout, space) =
                layout_sheet_dimension(painter, doc, paper, &view, measure, *label, &own)?;
            Some(SheetLayout {
                id,
                shape: SheetShape::Dimension(layout),
                space,
                style,
            })
        }
        SheetAnnotation::Note {
            text,
            at,
            leaders,
            style: note_style,
            ..
        } => {
            let space = paper_space(paper);
            let s = |p: DVec2| space.map.to_screen(p);
            let tips: Vec<_> = leaders
                .iter()
                .filter_map(|l| {
                    let tip = doc.sheet_leader_tip(sheet, l)?;
                    let bend = match l.leader.shape {
                        LeaderShape::Bent(b) => Some(s(tip + b)),
                        _ => None,
                    };
                    Some((l.leader, s(tip), bend))
                })
                .collect();
            let font = space.style(&style).text;
            let layout = notes::layout_note(
                painter,
                AnnotationId(id.0),
                text,
                s(*at),
                &tips,
                note_style,
                font,
                space.sizing,
            );
            Some(SheetLayout {
                id,
                shape: SheetShape::Note(space.note_to_screen(layout)),
                space,
                style,
            })
        }
    }
}

/// Every annotation of `sheet` laid out on `paper`, in drawing order.
pub fn layout_sheet_annotations(
    painter: &egui::Painter,
    doc: &Document,
    sheet: &Sheet,
    paper: &ScreenMap,
) -> Vec<SheetLayout> {
    sheet
        .annotations
        .iter()
        .filter_map(|(id, a)| layout_sheet_annotation(painter, doc, sheet, paper, *id, a))
        .collect()
}

/// The viewport of `sheet` under paper point `at`: the topmost whose frame holds it, as
/// `(index, view)`.
pub fn port_at(doc: &Document, sheet: &Sheet, at: DVec2) -> Option<(usize, PlacedView)> {
    sheet
        .viewports
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(i, p)| Some((i, PlacedView::new(p.view, doc.view(p.view)?, p))))
        .find(|(_, v)| {
            let b = v.frame.bounds();
            at.cmpge(b.min).all() && at.cmple(b.max).all()
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use drift_core::{
        Anchor, Bounds, EntityId, NoteStyle, PortId, SheetId, SheetLeader, StyleOverride, View,
        ViewId, Viewport,
    };
    use drift_draft::page::{Drawing, Prim};
    use drift_edit::editor::Editor;
    use drift_edit::fills::PatternLibrary;

    use super::*;

    const V: fn(f64, f64) -> DVec2 = DVec2::new;

    /// A painter for laying out text, on a context that never draws.
    pub(crate) fn painter() -> egui::Painter {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        egui::Painter::new(
            ctx,
            egui::LayerId::background(),
            egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
        )
    }

    /// A 1000 mm line from the origin along x, in a view of it at 1:10 placed with its middle
    /// at (200, 150) on a sheet (so the line runs from 150 to 250 across the paper at
    /// y = 150), and a second empty sheet.
    pub(crate) struct Plan {
        pub ed: Editor,
        pub a: EntityId,
        pub b: EntityId,
        pub line: EntityId,
        pub view: ViewId,
        pub sheet: SheetId,
        pub other: SheetId,
        pub port: PortId,
    }

    pub(crate) fn plan() -> Plan {
        let mut doc = Document::new();
        let a = doc.add_point(V(0.0, 0.0));
        let b = doc.add_point(V(1000.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let view = doc.add_view(View::new(
            "Plan",
            Bounds {
                min: V(-500.0, -500.0),
                max: V(1500.0, 500.0),
            },
            10.0,
        ));
        let mut s = Sheet::new("A-101");
        s.title_block = drift_core::TitleBlock::None;
        s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
        let sheet = doc.add_sheet(s);
        let mut blank = Sheet::new("A-102");
        blank.title_block = drift_core::TitleBlock::None;
        let other = doc.add_sheet(blank);
        let port = doc.tie_viewport(sheet, 0).unwrap();
        Plan {
            ed: Editor::new(doc),
            a,
            b,
            line,
            view,
            sheet,
            other,
            port,
        }
    }

    impl Plan {
        /// The line's length, its label 10 mm below it on the paper.
        pub fn dimension(&mut self) -> SheetAnnotationId {
            let c = Constraint::Distance {
                a: self.a,
                b: self.b,
                value: 0.0,
            };
            self.ed
                .doc
                .add_sheet_annotation(
                    self.sheet,
                    SheetAnnotation::dimension(self.port, c, Some(V(0.0, -10.0))),
                )
                .unwrap()
        }

        /// "DOOR" at (300, 220) with a leader to the line's middle through the view.
        pub fn note(&mut self) -> SheetAnnotationId {
            let note = SheetAnnotation::Note {
                text: "DOOR".into(),
                at: V(300.0, 220.0),
                leaders: vec![SheetLeader::through(
                    self.port,
                    Anchor::Curve {
                        curve: self.line,
                        t: 0.5,
                    },
                )],
                style: NoteStyle::default(),
                look: StyleOverride::default(),
            };
            self.ed.doc.add_sheet_annotation(self.sheet, note).unwrap()
        }

        pub fn page(&mut self, sheet: SheetId) -> Drawing {
            let s = self.ed.doc.sheet(sheet).unwrap().clone();
            let views = self.ed.doc.placed_views(&s);
            crate::output::sheet_drawing(
                &mut self.ed,
                &PatternLibrary::default(),
                &painter(),
                &s,
                &views,
                (1, 2),
            )
        }
    }

    /// The annotation layer's text, with where each piece starts.
    pub(crate) fn texts(d: &Drawing) -> Vec<(String, DVec2, f64)> {
        d.items
            .iter()
            .filter(|i| i.layer == "ANNOTATIONS")
            .filter_map(|i| match &i.prim {
                Prim::Text {
                    text, at, height, ..
                } => Some((text.clone(), *at, *height)),
                _ => None,
            })
            .collect()
    }

    /// The annotation layer's straight lines.
    fn lines(d: &Drawing) -> Vec<(DVec2, DVec2)> {
        d.items
            .iter()
            .filter(|i| i.layer == "ANNOTATIONS")
            .filter_map(|i| match &i.prim {
                Prim::Line(a, b) => Some((*a, *b)),
                _ => None,
            })
            .collect()
    }

    /// The dimension line: the horizontal line of the annotation layer `len` mm long.
    fn dimension_line(d: &Drawing, len: f64) -> Option<(DVec2, DVec2)> {
        lines(d)
            .into_iter()
            .find(|(a, b)| (a.y - b.y).abs() < 1e-6 && ((a.x - b.x).abs() - len).abs() < 0.05)
    }

    #[test]
    fn a_sheet_dimension_prints_true_size_at_any_scale_and_follows_its_view() {
        let mut p = plan();
        p.dimension();
        let d = p.page(p.sheet);
        // It reads the model's 1000 mm, at the drafting style's 3.5 mm on paper.
        let t = texts(&d);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].0, "1000");
        assert!((t[0].2 - 3.5).abs() < 0.01, "{t:?}");
        // Its line is 100 mm on the paper (1000 at 1:10), 10 mm under the line at y = 150.
        let (a, b) = dimension_line(&d, 100.0).expect("the dimension line");
        assert!((a.y - 140.0).abs() < 0.05, "{a} {b}");
        assert!((a.x.min(b.x) - 150.0).abs() < 0.05, "{a} {b}");
        // Nothing of it is clipped to the view's frame, and nothing is on the other sheet.
        assert!(
            d.items
                .iter()
                .filter(|i| i.layer == "ANNOTATIONS")
                .all(|i| i.clip.is_none())
        );
        assert!(texts(&p.page(p.other)).is_empty());

        // The view at 1:20: still 1000, the same size text, a line half as long, still
        // 10 mm under the geometry.
        let mut v = p.ed.doc.view(p.view).unwrap().clone();
        v.scale = 20.0;
        p.ed.doc.set_view(p.view, v);
        let d = p.page(p.sheet);
        let t = texts(&d);
        assert_eq!(t[0].0, "1000");
        assert!((t[0].2 - 3.5).abs() < 0.01, "{t:?}");
        let (a, b) = dimension_line(&d, 50.0).expect("the dimension line at 1:20");
        assert!((a.y - 140.0).abs() < 0.05, "{a} {b}");
        assert!((a.x.min(b.x) - 175.0).abs() < 0.05, "{a} {b}");

        // The view moved 30 right and 20 up on the sheet: the dimension goes with it.
        let mut s = p.ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports[0].at = V(230.0, 170.0);
        p.ed.doc.set_sheet(p.sheet, s);
        let d = p.page(p.sheet);
        let (a, b) = dimension_line(&d, 50.0).expect("the dimension line, moved");
        assert!((a.y - 160.0).abs() < 0.05, "{a} {b}");
        assert!((a.x.min(b.x) - 205.0).abs() < 0.05, "{a} {b}");

        // The geometry changes: the value follows.
        p.ed.doc.set_point(p.b, V(750.0, 0.0)).unwrap();
        let d = p.page(p.sheet);
        assert_eq!(texts(&d)[0].0, "750");
        assert!(dimension_line(&d, 37.5).is_some());
    }

    #[test]
    fn a_sheet_note_keeps_its_place_and_its_leader_follows() {
        let mut p = plan();
        p.note();
        let tip_of = |d: &Drawing, tip: DVec2| {
            lines(d)
                .iter()
                .any(|(a, b)| a.distance(tip) < 0.05 || b.distance(tip) < 0.05)
                || d.items.iter().any(|i| match &i.prim {
                    Prim::Polyline { points, .. } => points.iter().any(|q| q.distance(tip) < 0.05),
                    _ => false,
                })
        };
        let d = p.page(p.sheet);
        let t = texts(&d);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].0, "DOOR");
        let text_at = t[0].1;
        assert!(text_at.distance(V(300.0, 220.0)) < 12.0, "{text_at}");
        // Its leader's tip is the line's middle (500, 0) through the view: (200, 150).
        assert!(tip_of(&d, V(200.0, 150.0)), "{:?}", lines(&d));
        // The view is rescaled and moved: the tip follows, the text does not move at all.
        let mut v = p.ed.doc.view(p.view).unwrap().clone();
        v.scale = 5.0;
        v.center = V(0.0, 0.0);
        p.ed.doc.set_view(p.view, v);
        let d = p.page(p.sheet);
        assert_eq!(texts(&d)[0].1, text_at);
        assert!(tip_of(&d, V(300.0, 150.0)), "{:?}", lines(&d));
    }

    #[test]
    fn sheet_annotations_reach_the_pdf_and_stay_out_of_the_model() {
        let mut p = plan();
        p.dimension();
        p.note();
        let pages = [p.page(p.sheet), p.page(p.other)];
        let pdf = drift_io::pdf::write(&pages, "Test");
        let words = crate::ui_tests::pdf_words(&pdf);
        assert!(words.contains("1000") && words.contains("DOOR"), "{words}");
        // Once each: on its own sheet's page only.
        assert_eq!(words.matches("DOOR").count(), 1, "{words}");
        assert_eq!(words.matches("1000").count(), 1, "{words}");
        // The 100 mm dimension line is in the PDF at that length.
        let (_, lengths) = crate::output::pdf_page_and_lines(&pdf);
        assert!(
            lengths.iter().any(|l| (l - 100.0).abs() < 0.1),
            "{lengths:?}"
        );
        // The model's own export has neither: they are the sheet's.
        let model = crate::output::model_drawing(&mut p.ed, &PatternLibrary::default(), &painter());
        assert!(texts(&model).is_empty());
        assert_eq!(p.ed.doc.annotations().count(), 0);
        assert_eq!(p.ed.doc.constraints().count(), 0);
    }

    #[test]
    fn the_screen_layout_matches_the_page_at_any_zoom() {
        let mut p = plan();
        let dim = p.dimension();
        let sheet = p.ed.doc.sheet(p.sheet).unwrap().clone();
        // The sheet shown at 3 screen points to the paper mm, panned.
        let paper = ScreenMap {
            rect: egui::Rect::from_min_size(Pos2::new(40.0, 20.0), egui::vec2(800.0, 600.0)),
            camera: drift_render::Camera {
                center: V(210.0, 148.0),
                scale: 3.0,
            },
        };
        let layouts = layout_sheet_annotations(&painter(), &p.ed.doc, &sheet, &paper);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].id, dim);
        let SheetShape::Dimension(d) = &layouts[0].shape else {
            panic!("a dimension");
        };
        // Its dimension line's ends on screen are the page's (150, 140) and (250, 140).
        let crate::annotations::DimShape::Linear { d: ends, .. } = &d.shape else {
            panic!("linear");
        };
        for (end, page) in ends.iter().zip([V(150.0, 140.0), V(250.0, 140.0)]) {
            let want = paper.to_screen(page);
            assert!((*end - want).length() < 0.2, "{end:?} vs {want:?}");
        }
        assert_eq!(d.label.plain(), "1000");
        // Picked on its text and on its line, not off it.
        assert!(layouts[0].hit(d.text_rect.center()));
        assert!(layouts[0].hit(paper.to_screen(V(160.0, 140.0))));
        assert!(!layouts[0].hit(paper.to_screen(V(160.0, 200.0))));
    }
}
