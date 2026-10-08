//! Output: the page display list ([`Drawing`]) for a sheet (PDF, SVG, printing and the sheet
//! preview) or for model space (DXF), built from the document. Geometry, fills and hatches
//! are exact (true arcs, clipped hatch lines); dimensions and notes are drawn by the same code
//! as on screen, through a [`Recorder`](crate::annotations::Recorder).

use drift_core::{Bounds, DVec2, FillKind, LineKind, LineType, Rgb, Shape, Sheet};
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

/// How world coordinates map onto the page.
#[derive(Clone, Copy, Debug)]
struct Space {
    world: DVec2,
    page: DVec2,
    /// Page units per world mm (1 / scale on a sheet; 1 in model space).
    k: f64,
    /// The drawing scale annotations are sized for (1 : n).
    n: f64,
    clipped: bool,
}

impl Space {
    fn at(&self, w: DVec2) -> DVec2 {
        self.page + (w - self.world) * self.k
    }
}

fn item(layer: &str, pen: Pen, prim: Prim, clipped: bool) -> Item {
    Item {
        layer: layer.to_owned(),
        pen,
        prim,
        clipped,
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
/// non-printing layers.
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
    for (id, e) in doc.iter() {
        if e.kind.is_point() || e.style == LineKind::Construction {
            continue;
        }
        let layer = doc.layer_of(id);
        if !layer.visible || !layer.printable {
            continue;
        }
        let Some(shape) = doc.shape(id) else { continue };
        let color = e.appearance.color.or(layer.color).unwrap_or(Pen::BLACK);
        let weight = e.appearance.weight.unwrap_or(layer.weight).mm();
        let (line_type, width) = if e.style == LineKind::Center {
            (LineType::Center, THIN)
        } else {
            (
                e.appearance.line_type.unwrap_or(layer.line_type),
                if weight > 0.0 { weight } else { HAIRLINE },
            )
        };
        let pen = Pen::new(width).with_color(color).with_type(line_type);
        let prim = match shape {
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
            Shape::Point(_) => continue,
        };
        d.items.push(item(&layer.name, pen, prim, s.clipped));
    }
}

/// Fills and hatches, bottom first.
fn fills(ed: &mut Editor, patterns: &PatternLibrary, s: &Space, d: &mut Drawing) {
    let shapes = ed.fill_shapes();
    for id in ed.fills_in_order() {
        let (Some(fill), Some(shape)) = (ed.doc.fill(id).cloned(), shapes.get(&id)) else {
            continue;
        };
        let layer = ed.doc.layer(fill.layer).cloned();
        if layer.as_ref().is_some_and(|l| !l.visible || !l.printable) {
            continue;
        }
        let layer_name = layer.as_ref().map_or("0".to_owned(), |l| l.name.clone());
        let own = fill.color.or(layer.as_ref().and_then(|l| l.color));
        let region = &shape.region;
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
                s.clipped,
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
                    d.items.push(item(
                        &layer_name,
                        pen,
                        Prim::Line(s.at(a), s.at(b)),
                        s.clipped,
                    ));
                }
            }
        }
    }
}

/// Centre marks and centre lines.
fn centre_lines(ed: &Editor, s: &Space, d: &mut Drawing) {
    let pen = Pen::new(THIN).with_type(LineType::Center);
    for (_, a) in ed.doc.annotations() {
        for (p, q) in notes::centre_segments(&ed.doc, a) {
            d.items.push(item(
                ANNOTATION_LAYER,
                pen,
                Prim::Line(s.at(p), s.at(q)),
                s.clipped,
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
        if !c.is_dimension() {
            continue;
        }
        if let Some(layout) =
            annotations::layout_dimension(painter, doc, Some(id), c, doc.placement(id), &map)
        {
            annotations::paint_dimension(&recorder, &layout, AnnotationState::Normal, &colors);
        }
    }
    let style = annotations::DimStyle::of(&doc.draft_style());
    for layout in notes::layout_annotations(painter, doc, &map) {
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
            } => Prim::Text {
                at: to_page(baseline),
                height: len(size) * CAP_HEIGHT,
                // Screen angles turn clockwise (y down); page angles counter-clockwise.
                angle: -f64::from(angle),
                align: Align::Left,
                text,
            },
        };
        d.items.push(item(ANNOTATION_LAYER, pen, prim, s.clipped));
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

/// A sheet as printed: the view of the sketch at the sheet's scale, clipped to the drawing
/// area, under the border, title block and revision table.
pub fn sheet_drawing(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    sheet: &Sheet,
    number: (usize, usize),
) -> Drawing {
    let mut d = Drawing::new(sheet.size());
    let area = drift_draft::sheet::drawing_area(sheet);
    d.clip = Some(area);
    let space = Space {
        world: sheet.center,
        page: area.center(),
        k: 1.0 / sheet.scale,
        n: sheet.scale,
        clipped: true,
    };
    view(ed, patterns, painter, &space, &mut d);
    drift_draft::sheet::draw_frame(sheet, number, &mut d);
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
        clipped: false,
    };
    view(ed, patterns, painter, &space, &mut d);
    let b = d.bounds();
    if !b.is_empty() {
        d.size = b.max - b.min;
    }
    d
}

/// Centre a sheet's view on `b` (the sketch with its dimension labels, in world mm) and pick
/// the largest preferred scale that fits it in the drawing area, clear of the title block.
pub fn fit_sheet(b: Bounds, sheet: &mut Sheet) {
    if b.is_empty() {
        return;
    }
    let area = drift_draft::sheet::drawing_area(sheet);
    let mut room = area.size();
    if let Some(tb) = drift_draft::sheet::title_block_bounds(sheet) {
        // Keep clear of the title block: use the space above it.
        room.y -= tb.size().y;
    }
    // A little air around the drawing.
    let extent = b.size() * 1.1;
    sheet.scale = drift_core::sheets::fit_scale(extent, room * 0.95);
    let shift = match drift_draft::sheet::title_block_bounds(sheet) {
        Some(tb) => DVec2::new(0.0, tb.size().y / 2.0) * sheet.scale,
        None => DVec2::ZERO,
    };
    sheet.center = b.center() - shift;
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
    let clip = d.clip.map(|c| {
        let (a, b) = (to_screen(c.min), to_screen(c.max));
        egui::Rect::from_two_pos(a, b).intersect(painter.clip_rect())
    });
    let clipped = clip.map(|r| painter.with_clip_rect(r));
    for it in &d.items {
        let p = if it.clipped {
            clipped.as_ref().unwrap_or(painter)
        } else {
            painter
        };
        let [r, g, b] = it.pen.color.0;
        let color = Color32::from_rgb(r, g, b);
        let stroke = egui::Stroke::new((it.pen.width as f32 * px).max(0.6), color);
        let dashes: Vec<f32> = drift_draft::page::dash_pattern(it.pen.line_type)
            .iter()
            .map(|v| (v * d.line_type_scale) as f32 * px)
            .collect();
        let line = |pts: Vec<Pos2>| {
            if dashes.len() >= 2 && dashes[0] > 1.0 {
                p.extend(egui::Shape::dashed_line(&pts, stroke, dashes[0], dashes[1]));
            } else {
                p.add(egui::Shape::line(pts, stroke));
            }
        };
        let arc_points = |c: DVec2, r: f64, start: f64, sweep: f64| -> Vec<Pos2> {
            let n = ((sweep.abs() * r * f64::from(px)).sqrt().ceil() as usize).clamp(8, 256);
            (0..=n)
                .map(|k| to_screen(c + DVec2::from_angle(start + sweep * k as f64 / n as f64) * r))
                .collect()
        };
        match &it.prim {
            Prim::Line(a, b) => line(vec![to_screen(*a), to_screen(*b)]),
            Prim::Polyline { points, closed } => {
                let mut pts: Vec<Pos2> = points.iter().map(|q| to_screen(*q)).collect();
                if *closed && let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                line(pts);
            }
            Prim::Circle { center, radius } => {
                line(arc_points(*center, *radius, 0.0, std::f64::consts::TAU))
            }
            Prim::Arc {
                center,
                radius,
                start,
                sweep,
            } => line(arc_points(*center, *radius, *start, *sweep)),
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
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
            } => {
                let size = (*height / CAP_HEIGHT) as f32 * px;
                if size < 1.5 {
                    continue;
                }
                let galley =
                    p.layout_no_wrap(text.clone(), egui::FontId::proportional(size), color);
                let Some(first) = galley.rows.first().and_then(|r| r.row.glyphs.first()) else {
                    continue;
                };
                let ascent = galley.rows[0].pos.y + first.pos.y;
                let shift = match align {
                    Align::Left => 0.0,
                    Align::Center => galley.size().x / 2.0,
                    Align::Right => galley.size().x,
                };
                let screen_angle = -(*angle as f32);
                let rot = egui::emath::Rot2::from_angle(screen_angle);
                let pos = to_screen(*at) - rot * egui::vec2(shift, ascent);
                p.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(screen_angle));
            }
        }
    }
}
