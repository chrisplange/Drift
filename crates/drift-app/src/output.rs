//! Output: the page display list ([`Drawing`]) for a sheet (PDF, SVG, printing and the sheet
//! preview) or for model space (DXF), built from the document. Geometry, fills and hatches
//! are exact (true arcs, clipped hatch lines); dimensions and notes are drawn by the same code
//! as on screen, through a [`Recorder`](crate::annotations::Recorder).

use std::collections::BTreeSet;

use drift_core::{
    Bounds, DVec2, EntityId, FillKind, LayerId, LineKind, LineType, PlacedView, Rgb, Shape, Sheet,
    View, Viewport,
};
use drift_draft::Placement;
use drift_draft::page::{Align, CAP_HEIGHT, Drawing, Item, LayerInfo, Pen, Prim};
use drift_render::Camera;
use eframe::egui::{self, Color32, Pos2};

use crate::annotations::{self, AnnotationColors, AnnotationState, PT_PER_MM, Recorded, Recorder};
use crate::editor::Editor;
use crate::fills::PatternLibrary;
use crate::notes;
use crate::overlay::ScreenMap;

/// Layer names for drafting content in exports.
const ANNOTATION_LAYER: &str = "ANNOTATIONS";
/// Pen widths (mm) for thin drafting lines and for unweighted geometry.
const THIN: f64 = 0.18;
const HAIRLINE: f64 = 0.13;
/// Most hatch segments per fill in an export.
const HATCH_LIMIT: usize = 400_000;

/// No layers left out.
static ALL_LAYERS: BTreeSet<LayerId> = BTreeSet::new();

/// How world coordinates map onto the page.
#[derive(Clone, Copy, Debug)]
struct Space<'a> {
    world: DVec2,
    page: DVec2,
    /// Page units per world mm (1 / scale on a sheet; 1 in model space).
    k: f64,
    /// The drawing scale annotations are sized for (1 : n).
    n: f64,
    /// The window the view is seen through (a view's frame on a sheet).
    clip: Option<Bounds>,
    /// Copies of blocks as block definitions and inserts (DXF), rather than their curves.
    inserts: bool,
    /// Whether dimensions, and notes and callouts, are drawn (a sheet can leave them out).
    dimensions: bool,
    notes: bool,
    /// Layers the view leaves out.
    hidden: &'a BTreeSet<LayerId>,
}

impl Space<'_> {
    fn at(&self, w: DVec2) -> DVec2 {
        self.page + (w - self.world) * self.k
    }

    /// Whether `layer` is drawn: shown and printable, and not left out of the view.
    fn draws(&self, id: LayerId, layer: &drift_core::Layer) -> bool {
        layer.visible && layer.printable && !self.hidden.contains(&id)
    }

    /// Whether a dimension or annotation on `entities` is left out with them: it is when every
    /// one is on a layer the view leaves out (one on nothing stays).
    fn leaves_out(&self, doc: &drift_core::Document, entities: &[EntityId]) -> bool {
        !self.hidden.is_empty()
            && !entities.is_empty()
            && entities
                .iter()
                .all(|e| doc.get(*e).is_some_and(|x| self.hidden.contains(&x.layer)))
    }
}

fn item(layer: &str, pen: Pen, prim: Prim, clip: Option<Bounds>) -> Item {
    Item {
        layer: layer.to_owned(),
        pen,
        prim,
        clip,
    }
}

/// A colour faded towards white by `opacity`, for printing translucent fills.
fn on_paper(c: Rgb, opacity: f64) -> Rgb {
    let a = opacity.clamp(0.0, 1.0);
    Rgb(c
        .0
        .map(|v| (255.0 - (255.0 - f64::from(v)) * a).round() as u8))
}

/// The sketch's curves, as printed: no points or construction lines, nothing on hidden or
/// non-printing layers, or on layers the view leaves out.
fn geometry(ed: &Editor, s: &Space, d: &mut Drawing) {
    let doc = &ed.doc;
    for (_, layer) in doc.layers() {
        d.layer(LayerInfo {
            name: layer.name.clone(),
            color: layer.color.unwrap_or(Pen::BLACK),
            width: layer.weight.mm(),
            line_type: layer.line_type,
        });
    }
    // Layer by layer, in their draw order.
    let ranks = doc.layer_ranks();
    let mut entities: Vec<_> = doc.iter().collect();
    entities.sort_by_key(|(_, e)| ranks.get(&e.layer).copied().unwrap_or(0));
    for (id, e) in entities {
        if e.kind.is_point() || e.style == LineKind::Construction {
            continue;
        }
        let layer = doc.layer_of(id);
        if !s.draws(e.layer, layer) {
            continue;
        }
        let Some(shape) = doc.shape(id) else { continue };
        if let Some(prim) = shape_prim(s, &shape) {
            let pen = pen_for(e.style, e.appearance, layer);
            d.items.push(item(&layer.name, pen, prim, s.clip));
        }
    }
    // Sketch text: as text, line by line in its own font, each line from its own start (so
    // nothing downstream needs to measure it to align it).
    for (_, t) in doc.texts() {
        let Some(layer) = doc.layer(t.layer) else {
            continue;
        };
        if !s.draws(t.layer, layer) {
            continue;
        }
        ed.ensure_font(&t.font);
        let pen = pen_for(LineKind::Solid, t.appearance, layer);
        let paragraph = drift_draft::outline::Paragraph {
            text: &t.text,
            font: &t.font,
            height: t.height,
            at: t.at,
            angle: t.angle,
            align: t.align,
            spacing: t.spacing,
            wrap: t.wrap,
        };
        for (line, start) in paragraph.lines() {
            let line = line.trim_end().to_owned();
            if line.is_empty() {
                continue;
            }
            let prim = Prim::Text {
                at: s.at(start),
                height: t.height * s.k,
                angle: t.angle.to_radians(),
                align: Align::Left,
                text: line,
                style: Default::default(),
                font: Some(t.font.clone()),
            };
            d.items.push(item(&layer.name, pen, prim, s.clip));
        }
    }
    // Copies of blocks: their curves (sheets), or block definitions and inserts (DXF).
    let mut defined = std::collections::BTreeSet::new();
    for (id, copy) in doc.instances() {
        let Some(layer) = doc.layer(copy.layer) else {
            continue;
        };
        if !s.draws(copy.layer, layer) {
            continue;
        }
        let Some(block) = doc.block(copy.block) else {
            continue;
        };
        if s.inserts {
            if defined.insert(copy.block) {
                let local = Space {
                    world: DVec2::ZERO,
                    page: DVec2::ZERO,
                    k: 1.0,
                    ..*s
                };
                let items = block
                    .items
                    .iter()
                    .filter(|it| it.style != LineKind::Construction)
                    .filter_map(|it| {
                        let prim = shape_prim(&local, &it.shape)?;
                        Some(item(
                            "0",
                            pen_for(it.style, it.appearance, layer),
                            prim,
                            None,
                        ))
                    })
                    .collect();
                d.blocks.push(drift_draft::page::BlockDef {
                    name: block.name.clone(),
                    items,
                });
            }
            d.items.push(item(
                &layer.name,
                Pen::new(HAIRLINE),
                Prim::Insert {
                    block: block.name.clone(),
                    at: s.at(copy.at),
                    angle: copy.angle,
                    scale: copy.scale * s.k,
                    mirror: copy.mirror,
                },
                s.clip,
            ));
        } else {
            for it in doc.instance_items(id) {
                if it.style == LineKind::Construction {
                    continue;
                }
                if let Some(prim) = shape_prim(s, &it.shape) {
                    let pen = pen_for(it.style, it.appearance, layer);
                    d.items.push(item(&layer.name, pen, prim, s.clip));
                }
            }
        }
    }
}

/// How a curve prints: its own or its layer's colour, weight and line type (centre-style
/// curves as thin centre lines).
fn pen_for(style: LineKind, look: drift_core::Appearance, layer: &drift_core::Layer) -> Pen {
    let color = look.color.or(layer.color).unwrap_or(Pen::BLACK);
    let weight = look.weight.unwrap_or(layer.weight).mm();
    let (line_type, width) = if style == LineKind::Center {
        (LineType::Center, THIN)
    } else {
        (
            look.line_type.unwrap_or(layer.line_type),
            if weight > 0.0 { weight } else { HAIRLINE },
        )
    };
    Pen::new(width).with_color(color).with_type(line_type)
}

/// A curve on the page (points don't print).
fn shape_prim(s: &Space, shape: &Shape) -> Option<Prim> {
    Some(match *shape {
        Shape::Line { a, b } => Prim::Line(s.at(a), s.at(b)),
        Shape::Circle { center, radius } => Prim::Circle {
            center: s.at(center),
            radius: radius * s.k,
        },
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let (start, sweep) = if sweep < 0.0 {
                (start + sweep, -sweep)
            } else {
                (start, sweep)
            };
            Prim::Arc {
                center: s.at(center),
                radius: radius * s.k,
                start,
                sweep,
            }
        }
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => Prim::Ellipse {
            center: s.at(center),
            major: major * s.k,
            minor: minor * s.k,
            start,
            sweep,
        },
        Shape::Bezier { p } => Prim::Bezier {
            p: p.map(|q| s.at(q)),
        },
        Shape::Point(_) => return None,
    })
}

/// Fills and hatches, bottom first.
fn fills(ed: &mut Editor, patterns: &PatternLibrary, s: &Space, d: &mut Drawing) {
    let shapes = ed.fill_shapes();
    let copies = ed.copy_fills();
    let order = ed.fills_in_order();
    let ed = &*ed;
    // The sketch's fills, then copies' fills over them.
    let fills = order
        .iter()
        .filter_map(|id| Some((ed.doc.fill(*id)?, &shapes.get(id)?.region)))
        .chain(copies.iter().map(|c| (&c.fill, &c.region)));
    for (fill, region) in fills {
        let layer = ed.doc.layer(fill.layer).cloned();
        if layer.as_ref().is_some_and(|l| !s.draws(fill.layer, l)) {
            continue;
        }
        let layer_name = layer.as_ref().map_or("0".to_owned(), |l| l.name.clone());
        let own = fill.color.or(layer.as_ref().and_then(|l| l.fill_colour()));
        let rings: Vec<Vec<DVec2>> = std::iter::once(&region.outer)
            .chain(&region.holes)
            .map(|lp| lp.polygon(0.02).into_iter().map(|p| s.at(p)).collect())
            .collect();
        let triangles: Vec<[DVec2; 3]> = crate::fills::tessellate(region, 0.02)
            .map(|(v, idx)| {
                idx.as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| t.map(|i| s.at(v[i as usize])))
                    .collect()
            })
            .unwrap_or_default();
        let solid = |color: Rgb, d: &mut Drawing| {
            d.items.push(item(
                &layer_name,
                Pen::new(0.0).with_color(color),
                Prim::Fill {
                    rings: rings.clone(),
                    triangles: triangles.clone(),
                    mask: false,
                },
                s.clip,
            ));
        };
        match &fill.kind {
            FillKind::Solid => {
                solid(
                    on_paper(own.unwrap_or(Rgb([0x80, 0x88, 0x94])), fill.opacity),
                    d,
                );
            }
            FillKind::Hatch(h) => {
                if let Some(bg) = h.background {
                    solid(bg, d);
                }
                let color = own.unwrap_or(Pen::BLACK);
                let Some(pattern) = patterns.get(&h.pattern) else {
                    solid(on_paper(color, 0.25), d);
                    continue;
                };
                let placement = Placement {
                    scale: h.scale * if h.model { 1.0 } else { s.n },
                    angle: h.angle,
                    origin: h.origin,
                };
                let hatching = drift_draft::hatch(region, pattern, &placement, None, HATCH_LIMIT);
                if hatching.truncated {
                    solid(on_paper(color, 0.25), d);
                    continue;
                }
                let width = fill.weight.map_or(HAIRLINE, |w| w.mm().max(HAIRLINE));
                let pen = Pen::new(width).with_color(color);
                for [a, b] in hatching.segments {
                    d.items
                        .push(item(&layer_name, pen, Prim::Line(s.at(a), s.at(b)), s.clip));
                }
            }
        }
    }
}

/// Centre marks and centre lines.
fn centre_lines(ed: &Editor, s: &Space, d: &mut Drawing) {
    let pen = Pen::new(THIN).with_type(LineType::Center);
    let on_sheets = |id| {
        ed.doc
            .shown(drift_core::AnnotationRef::Annotation(id))
            .sheets
    };
    for (_, a) in ed.doc.annotations().filter(|(id, _)| on_sheets(*id)) {
        if s.leaves_out(&ed.doc, &a.entities()) {
            continue;
        }
        for (p, q) in notes::centre_segments(&ed.doc, a) {
            d.items.push(item(
                ANNOTATION_LAYER,
                pen,
                Prim::Line(s.at(p), s.at(q)),
                s.clip,
            ));
        }
    }
}

fn print_colors() -> AnnotationColors {
    let black = Color32::BLACK;
    AnnotationColors {
        normal: black,
        hovered: black,
        selected: black,
        conflict: black,
        preview: black,
        driven: black,
        text_background: Color32::WHITE,
        glyph_background: Color32::WHITE,
    }
}

/// Dimensions and notes, drawn through a recorder at the drawing scale and mapped onto the
/// page.
fn annotations(ed: &Editor, painter: &egui::Painter, s: &Space, d: &mut Drawing) {
    let doc = &ed.doc;
    // Virtual screen: points sized for paper at 1 : n, centred on the space's world origin.
    let pt_per_world = f64::from(PT_PER_MM) / s.n;
    let map = ScreenMap {
        rect: egui::Rect::from_center_size(Pos2::ZERO, egui::vec2(1.0, 1.0)),
        camera: Camera {
            center: s.world,
            scale: pt_per_world,
        },
    };
    let recorder = Recorder::new(painter);
    let colors = print_colors();
    for (id, c) in doc.constraints() {
        if !c.is_dimension()
            || !s.dimensions
            || !doc.shown(drift_core::AnnotationRef::Dimension(id)).sheets
            || s.leaves_out(doc, &c.entities().collect::<Vec<_>>())
        {
            continue;
        }
        if let Some(layout) =
            annotations::layout_dimension(painter, doc, Some(id), c, doc.placement(id), &map)
        {
            annotations::paint_dimension(&recorder, &layout, AnnotationState::Normal, &colors);
        }
    }
    for layout in notes::layout_annotations(painter, doc, &map) {
        let style = annotations::DimStyle::of(
            &doc.draft_style_for(drift_core::AnnotationRef::Annotation(layout.id)),
        );
        // Ordinates are dimensions; centre marks are linework, and always drawn.
        let shown = match doc.annotation(layout.id) {
            Some(drift_core::Annotation::Ordinate { .. }) => s.dimensions,
            Some(
                drift_core::Annotation::Note { .. }
                | drift_core::Annotation::HoleCallout { .. }
                | drift_core::Annotation::ChamferCallout { .. }
                | drift_core::Annotation::Section { .. }
                | drift_core::Annotation::Elevation { .. },
            ) => s.notes,
            _ => true,
        };
        if !shown
            || !doc
                .shown(drift_core::AnnotationRef::Annotation(layout.id))
                .sheets
            || doc
                .annotation(layout.id)
                .is_some_and(|a| s.leaves_out(doc, &a.entities()))
        {
            continue;
        }
        notes::paint_annotation(&recorder, &layout, AnnotationState::Normal, &colors, &style);
    }
    let to_page = |p: Pos2| s.at(map.to_world(p));
    let len = |v: f32| f64::from(v) / pt_per_world * s.k;
    let pen = Pen::new(THIN);
    for r in recorder.take() {
        let prim = match r {
            Recorded::Line(points, _) => {
                let points: Vec<DVec2> = points.into_iter().map(to_page).collect();
                if points.len() == 2 {
                    Prim::Line(points[0], points[1])
                } else {
                    Prim::Polyline {
                        points,
                        closed: false,
                    }
                }
            }
            Recorded::Polygon(points) => {
                let ring: Vec<DVec2> = points.into_iter().map(to_page).collect();
                let triangles = (1..ring.len().saturating_sub(1))
                    .map(|k| [ring[0], ring[k], ring[k + 1]])
                    .collect();
                Prim::Fill {
                    rings: vec![ring],
                    triangles,
                    mask: false,
                }
            }
            Recorded::Mask(points) => Prim::Fill {
                rings: vec![points.into_iter().map(to_page).collect()],
                triangles: Vec::new(),
                mask: true,
            },
            Recorded::Dot(at, r) => {
                let c = to_page(at);
                let r = len(r);
                let ring: Vec<DVec2> = (0..12)
                    .map(|k| c + DVec2::from_angle(f64::from(k) * std::f64::consts::TAU / 12.0) * r)
                    .collect();
                let triangles = (1..ring.len() - 1)
                    .map(|k| [ring[0], ring[k], ring[k + 1]])
                    .collect();
                Prim::Fill {
                    rings: vec![ring],
                    triangles,
                    mask: false,
                }
            }
            Recorded::Text {
                baseline,
                angle,
                size,
                text,
                bold,
                italic,
            } => Prim::Text {
                at: to_page(baseline),
                height: len(size) * drift_draft::font::cap_ratio(),
                // Screen angles turn clockwise (y down); page angles counter-clockwise.
                angle: -f64::from(angle),
                align: Align::Left,
                text,
                style: drift_draft::page::TextStyle {
                    bold,
                    italic,
                    tracking: 0.0,
                },
                font: None,
            },
        };
        d.items.push(item(ANNOTATION_LAYER, pen, prim, s.clip));
    }
    d.layer(LayerInfo {
        name: ANNOTATION_LAYER.into(),
        color: Pen::BLACK,
        width: THIN,
        line_type: LineType::Continuous,
    });
}

/// Everything in the view: fills under curves, then centre lines and annotations.
fn view(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    s: &Space,
    d: &mut Drawing,
) {
    fills(ed, patterns, s, d);
    geometry(ed, s, d);
    centre_lines(ed, s, d);
    annotations(ed, painter, s, d);
}

/// A sheet as printed, with its `views` (from `Document::placed_views`): each view of the
/// sketch at its scale, seen through its frame, under the views' titles, the border, title
/// block and revision table, in the sheet's colours.
pub fn sheet_drawing(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    sheet: &Sheet,
    views: &[PlacedView],
    number: (usize, usize),
) -> Drawing {
    let mut d = Drawing::new(sheet.size());
    // The drawing's own font, for everything measured and written from here on.
    d.font = ed.drawing_font();
    drift_draft::font::set_drawing_font(d.font.clone());
    for v in views {
        let frame = v.frame.bounds();
        if !drift_draft::sheet::fills_drawing_area(sheet, frame) {
            // A framed view is a piece of paper of its own: it covers what is under it.
            let c = [
                frame.min,
                DVec2::new(frame.max.x, frame.min.y),
                frame.max,
                DVec2::new(frame.min.x, frame.max.y),
            ];
            d.push(
                drift_draft::sheet::FRAME_LAYER,
                Pen::new(0.0),
                Prim::Fill {
                    rings: vec![c.to_vec()],
                    triangles: vec![[c[0], c[1], c[2]], [c[0], c[2], c[3]]],
                    mask: true,
                },
            );
        }
        let space = Space {
            world: v.center,
            page: frame.center(),
            k: 1.0 / v.scale,
            n: v.scale,
            clip: Some(frame),
            inserts: false,
            dimensions: v.dimensions,
            notes: v.notes,
            hidden: &v.hidden_layers,
        };
        view(ed, patterns, painter, &space, &mut d);
    }
    drift_draft::sheet::draw_views(sheet, views, &mut d);
    // (An unreadable logo was refused when it was chosen; one from a file is skipped.)
    let logo = Some(ed.doc.logo())
        .filter(|svg| !svg.is_empty())
        .and_then(|svg| drift_draft::logo::parse(svg).ok());
    let scale = drift_core::sheets::scale_text(views.iter().map(|v| v.scale));
    drift_draft::sheet::draw_frame(
        sheet,
        ed.doc.project(),
        &scale,
        number,
        logo.as_ref(),
        &mut d,
    );
    // In the sheet's colours: what the preview shows is what prints.
    d.set_colours(sheet.colours);
    d
}

/// Model space at true size (world millimetres), for DXF. Annotations are sized for the
/// document's drawing scale, and line type patterns scaled by it.
pub fn model_drawing(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
) -> Drawing {
    let n = ed.doc.draft_style().scale;
    let mut d = Drawing::new(DVec2::ZERO);
    d.line_type_scale = n;
    let space = Space {
        world: DVec2::ZERO,
        page: DVec2::ZERO,
        k: 1.0,
        n,
        clip: None,
        inserts: true,
        dimensions: true,
        notes: true,
        hidden: &ALL_LAYERS,
    };
    view(ed, patterns, painter, &space, &mut d);
    let b = d.bounds();
    if !b.is_empty() {
        d.size = b.max - b.min;
    }
    d
}

/// A window onto the sketch as a page of `size` mm: world point `center` at the page's
/// centre, at 1 : `scale`, with annotations sized for that scale (for pictures of the
/// drawing, such as the assistant's renders).
pub fn view_drawing(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    center: DVec2,
    scale: f64,
    size: DVec2,
    dimensions: bool,
) -> Drawing {
    let mut d = Drawing::new(size);
    d.font = ed.drawing_font();
    drift_draft::font::set_drawing_font(d.font.clone());
    let space = Space {
        world: center,
        page: size / 2.0,
        k: 1.0 / scale,
        n: scale,
        clip: None,
        inserts: false,
        dimensions,
        notes: true,
        hidden: &ALL_LAYERS,
    };
    view(ed, patterns, painter, &space, &mut d);
    d
}

/// Every sheet of the document as a page; with no sheets, one A3 sheet titled `title` with
/// the drawing fitted on it (as the app prints).
pub fn sheet_pages(
    ed: &mut Editor,
    patterns: &mut PatternLibrary,
    painter: &egui::Painter,
    title: &str,
) -> Vec<Drawing> {
    let doc = &ed.doc;
    let mut sheets: Vec<(Sheet, Vec<PlacedView>)> = doc
        .sheets()
        .map(|(_, s)| (s.clone(), doc.placed_views(s)))
        .collect();
    if sheets.is_empty() {
        let mut sheet = Sheet::new("Sheet 1");
        if doc.project().name.trim().is_empty() {
            sheet.fields.title = title.to_owned();
        }
        let imperial = !doc.units().length.is_metric();
        let (view, port) = sheet_view(ed.bounds(), &sheet, imperial);
        let placed = PlacedView::new(port.view, &view, &port);
        sheets.push((sheet, vec![placed]));
    }
    patterns.update(ed.doc.custom_patterns());
    let count = sheets.len();
    sheets
        .iter()
        .enumerate()
        .map(|(i, (s, views))| sheet_drawing(ed, patterns, painter, s, views, (i + 1, count)))
        .collect()
}

/// Centre a view on `b` (the sketch with its dimension labels, in world mm) at the largest
/// preferred scale that fits its frame, which stays as it is on the paper.
pub fn fit_view(b: Bounds, view: &mut PlacedView, imperial: bool) {
    if b.is_empty() {
        return;
    }
    // A good margin of air around the drawing.
    let extent = b.size() * 1.1;
    view.scale = drift_core::sheets::fit_scale(extent, view.frame.size * 0.9, imperial);
    view.center = b.center();
}

/// A sheet's first view of `b` (world mm): filling the drawing area clear of the title block,
/// at the largest preferred scale that fits, centred on it.
pub fn sheet_view(b: Bounds, sheet: &Sheet, imperial: bool) -> (View, Viewport) {
    let clear = drift_draft::sheet::view_area(sheet);
    let extent = if b.is_empty() {
        clear.size()
    } else {
        b.size() * 1.1
    };
    let scale = drift_core::sheets::fit_scale(extent, clear.size() * 0.9, imperial);
    let center = if b.is_empty() {
        DVec2::ZERO
    } else {
        b.center()
    };
    let half = clear.size() * scale * 0.5;
    let crop = Bounds {
        min: center - half,
        max: center + half,
    };
    (
        View::new("", crop, scale),
        Viewport::new(drift_core::ViewId(0), clear.center()),
    )
}

/// The crop of a new view of `b` (world mm) at 1 : `scale`: `b` with a margin, no bigger
/// than will fit on `sheet`.
pub fn view_crop(b: Bounds, sheet: &Sheet, scale: f64) -> Bounds {
    let area = drift_draft::sheet::view_area(sheet);
    let size = (b.size() / scale + DVec2::splat(10.0))
        .max(DVec2::splat(20.0))
        .min(area.size())
        * scale;
    Bounds {
        min: b.center() - size * 0.5,
        max: b.center() + size * 0.5,
    }
}

/// Where on `sheet` a frame of `size` (paper mm) goes, among the views already there
/// (`placed`): to the right of them, top-aligned with the drawing area, if it fits, else in
/// the middle.
pub fn place_frame(sheet: &Sheet, placed: &[PlacedView], size: DVec2) -> DVec2 {
    let area = drift_draft::sheet::view_area(sheet);
    let size = size.min(area.size());
    // To the right of the frames already placed (a frame filling the area leaves no room).
    let taken = placed
        .iter()
        .map(|v| v.frame.bounds().max.x)
        .fold(area.min.x, f64::max);
    let gap = 12.0;
    let at = if taken + gap + size.x <= area.max.x {
        DVec2::new(taken + gap + size.x * 0.5, area.max.y - gap - size.y * 0.5)
    } else {
        area.center()
    };
    at.max(area.min + size * 0.5).min(area.max - size * 0.5)
}

// ---------------------------------------------------------------------------------------------
// Painting a drawing on screen (the sheet preview)

/// Draw `d` with egui: `to_screen` maps page mm to screen points, `px` is points per mm.
pub fn paint_drawing(
    painter: &egui::Painter,
    d: &Drawing,
    to_screen: impl Fn(DVec2) -> Pos2,
    px: f32,
) {
    let d = &d.exploded();
    // A painter per window the items are seen through (kept while the window is the same).
    let mut window: Option<(Bounds, egui::Painter)> = None;
    for it in &d.items {
        if let Some(c) = it.clip
            && window.as_ref().is_none_or(|(w, _)| *w != c)
        {
            let (a, b) = (to_screen(c.min), to_screen(c.max));
            let rect = egui::Rect::from_two_pos(a, b).intersect(painter.clip_rect());
            window = Some((c, painter.with_clip_rect(rect)));
        }
        let p = match (&it.clip, &window) {
            (Some(_), Some((_, clipped))) => clipped,
            _ => painter,
        };
        let [r, g, b] = it.pen.color.0;
        let color = Color32::from_rgb(r, g, b);
        let stroke = egui::Stroke::new((it.pen.width as f32 * px).max(0.6), color);
        let dashes: Vec<f32> = drift_draft::page::dash_pattern(it.pen.line_type)
            .iter()
            .map(|v| (v * d.line_type_scale) as f32 * px)
            .collect();
        // Strokes end and turn round, as in the PDF: a dot at each end (and, for `corners`,
        // at each corner), so lines that meet look joined.
        let round = |pts: &[Pos2], corners: bool| {
            if stroke.width < 1.5 {
                return;
            }
            let r = stroke.width / 2.0;
            match pts {
                [] => {}
                [first, .., last] if !corners => {
                    p.circle_filled(*first, r, color);
                    p.circle_filled(*last, r, color);
                }
                _ => {
                    for q in pts {
                        p.circle_filled(*q, r, color);
                    }
                }
            }
        };
        let line = |pts: Vec<Pos2>, corners: bool| {
            if dashes.len() >= 2 && dashes[0] > 1.0 {
                p.extend(egui::Shape::dashed_line(&pts, stroke, dashes[0], dashes[1]));
            } else {
                round(&pts, corners);
                p.add(egui::Shape::line(pts, stroke));
            }
        };
        let arc_points = |c: DVec2, r: f64, start: f64, sweep: f64| -> Vec<Pos2> {
            // Fine enough that the chords stay within a tenth of a pixel of the arc.
            let n =
                ((sweep.abs() * (r * f64::from(px)).sqrt() * 1.2).ceil() as usize).clamp(12, 512);
            (0..=n)
                .map(|k| to_screen(c + DVec2::from_angle(start + sweep * k as f64 / n as f64) * r))
                .collect()
        };
        match &it.prim {
            Prim::Line(a, b) => line(vec![to_screen(*a), to_screen(*b)], false),
            Prim::Polyline { points, closed } => {
                let mut pts: Vec<Pos2> = points.iter().map(|q| to_screen(*q)).collect();
                if *closed && let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                line(pts, true);
            }
            Prim::Circle { center, radius } => line(
                arc_points(*center, *radius, 0.0, std::f64::consts::TAU),
                false,
            ),
            Prim::Arc {
                center,
                radius,
                start,
                sweep,
            } => line(arc_points(*center, *radius, *start, *sweep), false),
            Prim::Ellipse { .. } | Prim::Bezier { .. } => line(
                drift_draft::page::prim_points(&it.prim)
                    .into_iter()
                    .map(&to_screen)
                    .collect(),
                false,
            ),
            Prim::Fill {
                rings,
                triangles,
                mask,
            } => {
                let fill = if *mask { Color32::WHITE } else { color };
                if triangles.is_empty() {
                    if let Some(ring) = rings.first() {
                        let pts = ring.iter().map(|q| to_screen(*q)).collect();
                        p.add(egui::Shape::convex_polygon(pts, fill, egui::Stroke::NONE));
                    }
                } else {
                    let mut mesh = egui::Mesh::default();
                    for t in triangles {
                        let base = mesh.vertices.len() as u32;
                        for q in t {
                            mesh.colored_vertex(to_screen(*q), fill);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    p.add(egui::Shape::mesh(mesh));
                }
            }
            // Sketch text in its own font: its glyphs, filled.
            Prim::Text {
                at,
                height,
                angle,
                text,
                font: Some(font),
                ..
            } => {
                let contours = drift_draft::outline::text_contours(
                    text,
                    font,
                    *height,
                    *at,
                    angle.to_degrees(),
                );
                let rings = drift_draft::outline::contour_rings(&contours);
                let mut mesh = egui::Mesh::default();
                for t in drift_draft::logo::triangulate(&rings, false) {
                    let base = mesh.vertices.len() as u32;
                    for q in t {
                        mesh.colored_vertex(to_screen(q), color);
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                }
                p.add(egui::Shape::mesh(mesh));
            }
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
                style,
                font: None,
            } => {
                let cap = d
                    .font
                    .as_ref()
                    .map_or(CAP_HEIGHT, |f| f.face(style.bold).cap_ratio());
                let size = (*height / cap) as f32 * px;
                if size < 1.5 {
                    continue;
                }
                let family = crate::fonts::drafting_font(size, style.bold).family;
                let job = egui::text::LayoutJob::single_section(
                    text.clone(),
                    egui::TextFormat {
                        font_id: egui::FontId::new(size, family),
                        color,
                        extra_letter_spacing: style.tracking as f32 * size,
                        ..Default::default()
                    },
                );
                let galley = p.layout_job(job);
                let Some(first) = galley.rows.first().and_then(|r| r.row.glyphs.first()) else {
                    continue;
                };
                let ascent = galley.rows[0].pos.y + first.pos.y;
                // Tracking trails the last letter too; it isn't part of the measure.
                let width = galley.size().x - style.tracking as f32 * size;
                let shift = match align {
                    Align::Left => 0.0,
                    Align::Center => width / 2.0,
                    Align::Right => width,
                };
                let screen_angle = -(*angle as f32);
                let rot = egui::emath::Rot2::from_angle(screen_angle);
                let pos = to_screen(*at) - rot * egui::vec2(shift, ascent);
                p.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(screen_angle));
            }
            Prim::Insert { .. } => {}
        }
    }
}
