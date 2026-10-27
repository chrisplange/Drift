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
use drift_draft::page::{Align, CAP_HEIGHT, Drawing, HatchPrim, Item, LayerInfo, Pen, Prim};
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
/// Construction geometry on paper, as on the canvas: a light grey.
const CONSTRUCTION_GREY: Rgb = Rgb([0x8e, 0x9a, 0xb0]);
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
    /// For CAD formats with hatches of their own (DWG and DXF R2018): fills and hatches as
    /// areas ([`Prim::Hatch`]), and copies' fills in their blocks. Model space only.
    cad: bool,
    /// Whether dimensions, and notes and callouts, are drawn (a sheet can leave them out).
    dimensions: bool,
    notes: bool,
    /// Whether construction geometry is drawn (a view on a sheet can show it).
    construction: bool,
    /// Layers the view leaves out.
    hidden: &'a BTreeSet<LayerId>,
    /// The view drawn (dimensions and annotations drawn in other views only are left out);
    /// `None` draws them all.
    view: Option<drift_core::ViewId>,
    /// What is drawn, when it is only some of the sketch (a model export of the selection).
    only: Option<&'a Scope>,
}

/// What a model export is limited to: the picked curves, copies of blocks, text and fills, and
/// the dimensions and annotations picked or on picked curves only.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// The picked curves, with the points that define them.
    pub entities: BTreeSet<EntityId>,
    pub instances: BTreeSet<drift_core::InstanceId>,
    pub texts: BTreeSet<drift_core::TextId>,
    pub fills: BTreeSet<drift_core::FillId>,
    pub dimensions: BTreeSet<drift_core::ConstraintId>,
    pub annotations: BTreeSet<drift_core::AnnotationId>,
}

impl Scope {
    /// The editor's pick, with `dimension` (the app's picked one); `None` when nothing is
    /// picked.
    pub fn of_selection(ed: &Editor, dimension: Option<drift_core::ConstraintId>) -> Option<Scope> {
        let scope = Scope {
            entities: ed
                .doc
                .defining_points(ed.selection.iter().copied())
                .into_iter()
                .chain(ed.selection.iter().copied())
                .collect(),
            instances: ed.selected_instances.clone(),
            texts: ed.selected_texts.clone(),
            fills: ed.selected_fills.clone(),
            dimensions: ed
                .picked_dimensions()
                .into_iter()
                .chain(dimension)
                .collect(),
            annotations: ed.picked_notes().into_iter().collect(),
        };
        let empty = scope.entities.is_empty()
            && scope.instances.is_empty()
            && scope.texts.is_empty()
            && scope.fills.is_empty()
            && scope.dimensions.is_empty()
            && scope.annotations.is_empty();
        (!empty).then_some(scope)
    }

    /// Whether a dimension or annotation on `entities` is drawn: it is picked, or on picked
    /// curves only.
    fn takes(&self, r: drift_core::AnnotationRef, entities: &[EntityId]) -> bool {
        let picked = match r {
            drift_core::AnnotationRef::Dimension(id) => self.dimensions.contains(&id),
            drift_core::AnnotationRef::Annotation(id) => self.annotations.contains(&id),
        };
        picked || (!entities.is_empty() && entities.iter().all(|e| self.entities.contains(e)))
    }
}

impl Space<'_> {
    fn at(&self, w: DVec2) -> DVec2 {
        self.page + (w - self.world) * self.k
    }

    /// Whether `layer` is drawn: shown and printable, and not left out of the view.
    fn draws(&self, id: LayerId, layer: &drift_core::Layer) -> bool {
        layer.visible && layer.printable && !self.hidden.contains(&id)
    }

    /// Whether a dimension or annotation is drawn in this view.
    fn has(&self, doc: &drift_core::Document, r: drift_core::AnnotationRef) -> bool {
        let entities = || match r {
            drift_core::AnnotationRef::Dimension(id) => doc
                .constraint(id)
                .map(|c| c.entities().collect())
                .unwrap_or_default(),
            drift_core::AnnotationRef::Annotation(id) => {
                doc.annotation(id).map(|a| a.entities()).unwrap_or_default()
            }
        };
        self.view.is_none_or(|v| doc.in_view(r, v))
            && self.only.is_none_or(|o| o.takes(r, &entities()))
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

/// List the drawing's layers in their draw order (for formats that keep them).
fn declare_layers(ed: &Editor, d: &mut Drawing) {
    for (_, layer) in ed.doc.layers() {
        d.layer(LayerInfo {
            name: layer.name.clone(),
            color: layer.color.unwrap_or(Pen::BLACK),
            width: layer.weight.mm(),
            line_type: layer.line_type,
        });
    }
}

/// An import's fill edges: construction curves that never print, even where the view shows
/// construction.
fn fill_edges(ed: &Editor, s: &Space) -> BTreeSet<EntityId> {
    if s.construction {
        ed.fill_edges()
    } else {
        Default::default()
    }
}

/// A curve of the sketch as printed: not a point, nor a construction curve unless the view
/// shows them (never one of `fill_edges`), nor on a hidden or non-printing layer, or one
/// the view leaves out.
fn curve(ed: &Editor, s: &Space, fill_edges: &BTreeSet<EntityId>, id: EntityId, d: &mut Drawing) {
    let doc = &ed.doc;
    let Some(e) = doc.get(id) else { return };
    if e.kind.is_point()
        || (e.style == LineKind::Construction && (!s.construction || fill_edges.contains(&id)))
    {
        return;
    }
    let layer = doc.layer_of(id);
    if !s.draws(e.layer, layer)
        || s.only.is_some_and(|o| !o.entities.contains(&id))
        || !stroked(e.appearance, layer)
    {
        return;
    }
    let Some(shape) = doc.shape(id) else { return };
    if let Some(prim) = shape_prim(s, &shape) {
        let pen = pen_for(e.style, e.appearance, layer);
        d.items.push(item(&layer.name, pen, prim, s.clip));
    }
}

/// The sketch's curves as printed (see [`curve`]), layer by layer in their draw order,
/// then its text and the copies of blocks: all over every fill, as when fills don't cover
/// what is beneath them.
fn geometry(ed: &Editor, patterns: &PatternLibrary, s: &Space, d: &mut Drawing) {
    declare_layers(ed, d);
    let ranks = ed.doc.layer_ranks();
    let mut entities: Vec<_> = ed.doc.iter().collect();
    entities.sort_by_key(|(_, e)| ranks.get(&e.layer).copied().unwrap_or(0));
    let edges = fill_edges(ed, s);
    for (id, _) in entities {
        curve(ed, s, &edges, id, d);
    }
    texts(ed, s, None, d);
    copies(ed, patterns, s, None, &mut BTreeSet::new(), d);
}

/// Sketch text (all of it, or `on` one layer): as text, line by line in its own font, each
/// line from its own start (so nothing downstream needs to measure it to align it).
fn texts(ed: &Editor, s: &Space, on: Option<LayerId>, d: &mut Drawing) {
    let doc = &ed.doc;
    for (id, t) in doc.texts().filter(|(_, t)| on.is_none_or(|l| t.layer == l)) {
        let Some(layer) = doc.layer(t.layer) else {
            continue;
        };
        if !s.draws(t.layer, layer) || s.only.is_some_and(|o| !o.texts.contains(&id)) {
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
}

/// Copies of blocks (all of them, or `on` one layer): their curves (sheets), or block
/// definitions and inserts (DXF; `defined` are the blocks defined so far).
fn copies(
    ed: &Editor,
    patterns: &PatternLibrary,
    s: &Space,
    on: Option<LayerId>,
    defined: &mut BTreeSet<drift_core::BlockId>,
    d: &mut Drawing,
) {
    let doc = &ed.doc;
    for (id, copy) in doc
        .instances()
        .filter(|(_, c)| on.is_none_or(|l| c.layer == l))
    {
        let Some(layer) = doc.layer(copy.layer) else {
            continue;
        };
        if !s.draws(copy.layer, layer) || s.only.is_some_and(|o| !o.instances.contains(&id)) {
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
                // For CAD, the block's fills first, under its curves. What an item takes
                // from its layer is "by block", so each copy shows its own layer's look (the
                // pens here are this first copy's, for formats that explode copies).
                let mut items = if s.cad {
                    block_fills(ed, patterns, &local, block, copy.layer)
                } else {
                    Vec::new()
                };
                items.extend(
                    block
                        .items
                        .iter()
                        .filter(|it| {
                            (it.style != LineKind::Construction || s.construction)
                                && it.appearance.line_type.is_none_or(LineType::is_stroked)
                        })
                        .filter_map(|it| {
                            let prim = shape_prim(&local, &it.shape)?;
                            let pen = Pen {
                                by_block: by_block(it.style, it.appearance),
                                ..pen_for(it.style, it.appearance, layer)
                            };
                            Some(item("0", pen, prim, None))
                        }),
                );
                d.blocks.push(drift_draft::page::BlockDef {
                    name: block.name.clone(),
                    items,
                });
            }
            d.items.push(item(
                &layer.name,
                pen_for(LineKind::Solid, Default::default(), layer),
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
                if (it.style == LineKind::Construction && !s.construction)
                    || !stroked(it.appearance, layer)
                {
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

/// The drawing in its stack (see [`crate::stack`]), as the canvas draws it: layer by layer,
/// each fill drawn with its outline over what is beneath it, and each layer's text and
/// copies of blocks at its top. (Underlays' pictures never print.)
fn stacked(ed: &mut Editor, patterns: &PatternLibrary, s: &Space, d: &mut Drawing) {
    use crate::stack::FillRef;
    let stack = ed.stack();
    let shapes = ed.fill_shapes();
    let copy_fills = ed.copy_fills();
    let ed = &*ed;
    declare_layers(ed, d);
    let edges = fill_edges(ed, s);
    let mut defined = BTreeSet::new();
    for (k, g) in stack.groups.iter().enumerate() {
        match g.fill {
            Some(FillRef::Fill(id)) => {
                if s.only.is_none_or(|o| o.fills.contains(&id))
                    && let (Some(f), Some(shape)) = (ed.doc.fill(id), shapes.get(&id))
                {
                    fill(ed, patterns, s, f, &shape.region, d);
                }
            }
            Some(FillRef::Copy(instance, index)) => {
                if !(s.cad && s.inserts)
                    && s.only.is_none_or(|o| o.instances.contains(&instance))
                    && let Some(c) = copy_fills
                        .iter()
                        .find(|c| c.instance == instance && c.index == index)
                {
                    fill(ed, patterns, s, &c.fill, &c.region, d);
                }
            }
            None => {}
        }
        for id in &g.curves {
            curve(ed, s, &edges, *id, d);
        }
        if let Some(layer) = g.layer
            && stack.tops.get(&layer) == Some(&k)
        {
            texts(ed, s, Some(layer), d);
            copies(ed, patterns, s, Some(layer), &mut defined, d);
        }
    }
}

/// Whether a curve prints at all: one without a stroke doesn't (its fill does).
fn stroked(look: drift_core::Appearance, layer: &drift_core::Layer) -> bool {
    look.line_type.unwrap_or(layer.line_type).is_stroked()
}

/// How a curve prints: its own or its layer's colour, weight and line type (centre-style
/// curves as thin centre lines; construction curves as hairline dashes, in their own colour or
/// a light grey).
fn pen_for(style: LineKind, look: drift_core::Appearance, layer: &drift_core::Layer) -> Pen {
    if style == LineKind::Construction {
        return Pen::new(HAIRLINE)
            .with_color(look.color.unwrap_or(CONSTRUCTION_GREY))
            .with_type(LineType::Dashed);
    }
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

/// What a block item's pen takes from its copy: whatever [`pen_for`] would take from the
/// layer (construction curves keep their own look; centre-style ones their type and width).
fn by_block(style: LineKind, look: drift_core::Appearance) -> drift_draft::page::ByBlock {
    let solid = style == LineKind::Solid;
    drift_draft::page::ByBlock {
        color: style != LineKind::Construction && look.color.is_none(),
        width: solid && look.weight.is_none(),
        line_type: solid && look.line_type.is_none(),
    }
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

/// Fills and hatches, bottom first: all under every curve, as when fills don't cover what
/// is beneath them.
fn fills(ed: &mut Editor, patterns: &PatternLibrary, s: &Space, d: &mut Drawing) {
    let shapes = ed.fill_shapes();
    let copies = ed.copy_fills();
    let order = ed.fills_in_order();
    let ed = &*ed;
    // The sketch's fills, then copies' fills over them.
    let all = order
        .iter()
        .filter(|id| s.only.is_none_or(|o| o.fills.contains(id)))
        .filter_map(|id| Some((ed.doc.fill(*id)?, &shapes.get(id)?.region)))
        .chain(
            copies
                .iter()
                .filter(|_| !(s.cad && s.inserts))
                .filter(|c| s.only.is_none_or(|o| o.instances.contains(&c.instance)))
                .map(|c| (&c.fill, &c.region)),
        );
    for (f, region) in all {
        fill(ed, patterns, s, f, region, d);
    }
}

/// A fill or hatch as printed.
fn fill(
    ed: &Editor,
    patterns: &PatternLibrary,
    s: &Space,
    fill: &drift_core::Fill,
    region: &drift_geom::regions::Region,
    d: &mut Drawing,
) {
    let layer = ed.doc.layer(fill.layer).cloned();
    if layer.as_ref().is_some_and(|l| !s.draws(fill.layer, l)) {
        return;
    }
    let layer_name = layer.as_ref().map_or("0".to_owned(), |l| l.name.clone());
    let own = fill.color.or(layer.as_ref().and_then(|l| l.fill_colour()));
    if s.cad {
        cad_fill(patterns, s, fill, region, own, &layer_name, d);
        return;
    }
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
                return;
            };
            let placement = Placement {
                scale: h.scale * if h.model { 1.0 } else { s.n },
                angle: h.angle,
                origin: h.origin,
            };
            let hatching = drift_draft::hatch(region, pattern, &placement, None, HATCH_LIMIT);
            if hatching.truncated {
                solid(on_paper(color, 0.25), d);
                return;
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

/// A fill or hatch for CAD: kept as an area, its true colour and opacity (not faded on
/// paper), a hatch over its background colour if it has one. A pattern Drift doesn't have
/// is a light solid fill, as on paper.
fn cad_fill(
    patterns: &PatternLibrary,
    s: &Space,
    fill: &drift_core::Fill,
    region: &drift_geom::regions::Region,
    own: Option<Rgb>,
    layer: &str,
    d: &mut Drawing,
) {
    let mut area = |pen: Pen, pattern, opacity| {
        let prim = Prim::Hatch(Box::new(HatchPrim {
            region: region.clone(),
            pattern,
            opacity,
        }));
        d.items.push(item(layer, pen, prim, s.clip));
    };
    match &fill.kind {
        FillKind::Solid => area(
            Pen::new(0.0).with_color(own.unwrap_or(Rgb([0x80, 0x88, 0x94]))),
            None,
            fill.opacity,
        ),
        FillKind::Hatch(h) => {
            if let Some(bg) = h.background {
                area(Pen::new(0.0).with_color(bg), None, 1.0);
            }
            let color = own.unwrap_or(Pen::BLACK);
            match patterns.get(&h.pattern) {
                Some(pattern) => {
                    let placement = Placement {
                        scale: h.scale * if h.model { 1.0 } else { s.n },
                        angle: h.angle,
                        origin: h.origin,
                    };
                    let width = fill.weight.map_or(HAIRLINE, |w| w.mm().max(HAIRLINE));
                    area(
                        Pen::new(width).with_color(color),
                        Some((pattern.clone(), placement)),
                        1.0,
                    );
                }
                None => area(Pen::new(0.0).with_color(color), None, 0.25),
            }
        }
    }
}

/// A block's fills in its own coordinates, as a copy on `layer` draws them (for CAD, where
/// they go in the block's definition): each fills the area of the block's curves around its
/// seed. On layer 0, as the block's curves are, so they take the copy's layer.
fn block_fills(
    ed: &Editor,
    patterns: &PatternLibrary,
    s: &Space,
    block: &drift_core::Block,
    layer: LayerId,
) -> Vec<Item> {
    if block.fills.is_empty() {
        return Vec::new();
    }
    let shapes: Vec<Shape> = block.items.iter().map(|it| it.shape).collect();
    let areas = drift_geom::regions::regions(&shapes);
    // A fill in its layer's colour takes its copy's ("by block"), when the layer has no
    // fill colour of its own.
    let follows = ed.doc.layer(layer).is_some_and(|l| l.fill_color.is_none());
    let mut out = Vec::new();
    for f in &block.fills {
        if let Some(region) = drift_geom::regions::region_at(&areas, f.seed) {
            let mut d = Drawing::new(DVec2::ZERO);
            let by_block = follows && f.color.is_none();
            let f = drift_core::Fill { layer, ..f.clone() };
            fill(ed, patterns, s, &f, region, &mut d);
            out.extend(d.items.into_iter().map(|mut it| {
                it.layer = "0".into();
                it.pen.by_block.color = by_block;
                it
            }));
        }
    }
    out
}

/// Centre marks and centre lines.
fn centre_lines(ed: &Editor, s: &Space, d: &mut Drawing) {
    let pen = Pen::new(THIN).with_type(LineType::Center);
    let on_sheets = |id| {
        ed.doc
            .shown(drift_core::AnnotationRef::Annotation(id))
            .sheets
    };
    let in_view = |id| s.has(&ed.doc, drift_core::AnnotationRef::Annotation(id));
    for (_, a) in ed
        .doc
        .annotations()
        .filter(|(id, _)| on_sheets(*id) && in_view(*id))
    {
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
            || !s.has(doc, drift_core::AnnotationRef::Dimension(id))
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
    // Walls' thickness dimensions, as on screen.
    for (_, q) in doc.property_dims().filter(|_| s.dimensions) {
        let Some((c, at)) = crate::walls::thickness_dimension(doc, q) else {
            continue;
        };
        if s.leaves_out(doc, &c.entities().collect::<Vec<_>>()) {
            continue;
        }
        if let Some(layout) = annotations::layout_dimension(painter, doc, None, &c, Some(at), &map)
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
            || !s.has(doc, drift_core::AnnotationRef::Annotation(layout.id))
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

/// Everything in the view: the drawing stacked as the canvas draws it (or every fill under
/// every curve, as the drawing's rules say), then centre lines and annotations.
fn view(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    s: &Space,
    d: &mut Drawing,
) {
    if ed.doc.rules().stacked_fills {
        stacked(ed, patterns, s, d);
    } else {
        fills(ed, patterns, s, d);
        geometry(ed, patterns, s, d);
    }
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
            cad: false,
            dimensions: v.dimensions,
            notes: v.notes,
            construction: v.construction,
            hidden: &v.hidden_layers,
            view: Some(v.id),
            only: None,
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
        cad: false,
        dimensions: true,
        notes: true,
        construction: false,
        hidden: &ALL_LAYERS,
        view: None,
        only: None,
    };
    view(ed, patterns, painter, &space, &mut d);
    let b = d.bounds();
    if !b.is_empty() {
        d.size = b.max - b.min;
    }
    d
}

/// The CAD formats the model exports to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CadFormat {
    /// DWG R2018: AutoCAD 2018 and later, and the programs that read it.
    #[default]
    Dwg,
    /// DXF R2018: the same drawing as the DWG, as text.
    Dxf,
    /// DXF R12, which every program reads: laser cutters, CNC, older software.
    DxfR12,
}

impl CadFormat {
    pub const ALL: [CadFormat; 3] = [CadFormat::Dwg, CadFormat::Dxf, CadFormat::DxfR12];

    pub fn extension(self) -> &'static str {
        match self {
            CadFormat::Dwg => "dwg",
            CadFormat::Dxf | CadFormat::DxfR12 => "dxf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CadFormat::Dwg => "DWG (AutoCAD 2018 and later)",
            CadFormat::Dxf => "DXF (AutoCAD 2018)",
            CadFormat::DxfR12 => "DXF R12 (laser, CNC, older software)",
        }
    }

    /// What the format keeps.
    pub fn hint(self) -> &'static str {
        match self {
            CadFormat::Dwg => {
                "For AutoCAD, BricsCAD, DraftSight and the like: true curves, layers, blocks, \
                 fills and hatches, true colours and line weights"
            }
            CadFormat::Dxf => "The same drawing as the DWG, for programs that read DXF but not DWG",
            CadFormat::DxfR12 => {
                "Lines, arcs and polylines only, which every program reads: curves as fine \
                 polylines, fills as solid triangles, hatches as lines"
            }
        }
    }
}

/// The model at full size as a CAD file, in the document's units (`patterns` up to date
/// with the document's own).
#[cfg(not(feature = "demo"))]
pub fn cad_file(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    format: CadFormat,
) -> Result<Vec<u8>, String> {
    let units = drift_io::dxf::DxfUnits::of(&ed.doc.units());
    match format {
        CadFormat::DxfR12 => {
            let d = model_drawing(ed, patterns, painter);
            Ok(drift_io::dxf::write(&d, units).into_bytes())
        }
        #[cfg(feature = "dwg")]
        CadFormat::Dwg => drift_io::cad::write_dwg(&cad_drawing(ed, patterns, painter), units),
        #[cfg(feature = "dwg")]
        CadFormat::Dxf => drift_io::cad::write_dxf(&cad_drawing(ed, patterns, painter), units),
        #[cfg(not(feature = "dwg"))]
        _ => Err("this build of Drift writes DXF R12 only".into()),
    }
}

/// Model space at full size for CAD (DWG and DXF R2018), as [`model_drawing`] for DXF R12
/// but with fills and hatches kept as areas and blocks' fills in their blocks.
#[cfg(all(feature = "dwg", not(feature = "demo")))]
pub fn cad_drawing(ed: &mut Editor, patterns: &PatternLibrary, painter: &egui::Painter) -> Drawing {
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
        cad: true,
        dimensions: true,
        notes: true,
        construction: false,
        hidden: &ALL_LAYERS,
        view: None,
        only: None,
    };
    view(ed, patterns, painter, &space, &mut d);
    let b = d.bounds();
    if !b.is_empty() {
        d.size = b.max - b.min;
    }
    d
}

/// The margin (mm) round a model export's page.
pub const MODEL_MARGIN: f64 = 5.0;

/// Model space at true size (1 : 1) on a page fitted to it with a small margin, for SVG and
/// PDF: what prints of the sketch, or of `only` (the selection), with or without its
/// dimensions and notes. Annotations are sized for the document's drawing scale, as in DXF.
pub fn model_page(
    ed: &mut Editor,
    patterns: &PatternLibrary,
    painter: &egui::Painter,
    only: Option<&Scope>,
    annotate: bool,
) -> Drawing {
    let n = ed.doc.draft_style().scale;
    let mut d = Drawing::new(DVec2::ZERO);
    d.font = ed.drawing_font();
    drift_draft::font::set_drawing_font(d.font.clone());
    d.line_type_scale = n;
    let space = Space {
        world: DVec2::ZERO,
        page: DVec2::ZERO,
        k: 1.0,
        n,
        clip: None,
        inserts: false,
        cad: false,
        dimensions: annotate,
        notes: annotate,
        construction: false,
        hidden: &ALL_LAYERS,
        view: None,
        only,
    };
    view(ed, patterns, painter, &space, &mut d);
    // Only the layers drawn on, in their order, each a group of its own.
    d.layers
        .retain(|l| d.items.iter().any(|i| i.layer == l.name));
    d.fitted(MODEL_MARGIN)
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
        cad: false,
        dimensions,
        notes: true,
        construction: false,
        hidden: &ALL_LAYERS,
        view: None,
        only: None,
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
    let d = &d.exploded().without_hatches();
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
                crate::overlay::dashed(p, &pts, stroke, dashes[0], dashes[1]);
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
                fill_outlines(p, &contours, &to_screen, color);
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
                // Past a size egui's font atlas can't hold a glyph of, the text's outlines,
                // filled (as the PDF has them).
                if size > MAX_GLYPH_PX {
                    let builtin;
                    let face = match &d.font {
                        Some(f) => f.face(style.bold),
                        None => {
                            let data = drift_draft::outline::builtin_font(false);
                            let Some(f) = drift_draft::font::Face::new(data, 0) else {
                                continue;
                            };
                            builtin = f;
                            &builtin
                        }
                    };
                    let width = face.width(text, *height, style.tracking);
                    let shift = match align {
                        Align::Left => 0.0,
                        Align::Center => width / 2.0,
                        Align::Right => width,
                    };
                    let start = *at - DVec2::from_angle(*angle) * shift;
                    let contours = drift_draft::outline::contours_in(
                        &face.data,
                        face.index,
                        text,
                        *height,
                        start,
                        angle.to_degrees(),
                        style.tracking,
                    );
                    fill_outlines(p, &contours, &to_screen, color);
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
            Prim::Insert { .. } | Prim::Hatch(_) => {}
        }
    }
}

/// The largest font size (in screen points) text is laid out at with egui: a glyph much
/// bigger doesn't fit egui's font atlas (and panics it).
const MAX_GLYPH_PX: f32 = 128.0;

/// Glyph contours in page mm, filled.
fn fill_outlines(
    p: &egui::Painter,
    contours: &[Vec<drift_core::Shape>],
    to_screen: impl Fn(DVec2) -> Pos2,
    color: Color32,
) {
    let rings = drift_draft::outline::contour_rings(contours);
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

#[cfg(test)]
mod tests {
    use drift_core::{Appearance, Document, Fill, Layer, SheetColours, View, Viewport};

    use super::*;

    /// A painter for laying out text, on a context that never draws.
    fn painter() -> egui::Painter {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        // A first frame loads the fonts; its font atlas is never uploaded.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        egui::Painter::new(
            ctx,
            egui::LayerId::background(),
            egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
        )
    }

    fn line(doc: &mut Document, a: [f64; 2], b: [f64; 2], layer: LayerId) -> EntityId {
        let (a, b) = (
            doc.add_point(DVec2::from_array(a)),
            doc.add_point(DVec2::from_array(b)),
        );
        let l = doc.add_line(a, b).unwrap();
        for e in [a, b, l] {
            doc.set_entity_layer(e, layer).unwrap();
        }
        l
    }

    /// On `Guides`: a drawn line, a centre line, a construction line and circle, and a red
    /// construction line. On `Import`: a square of construction curves an import filled.
    fn sheet(construction: bool, colours: SheetColours) -> Drawing {
        let mut doc = Document::new();
        let guides = doc.add_layer(Layer::new("Guides"));
        let import = doc.add_layer(Layer::new("Import"));
        line(&mut doc, [0.0, 0.0], [100.0, 0.0], guides);
        let centre = line(&mut doc, [50.0, -10.0], [50.0, 60.0], guides);
        doc.set_style(centre, LineKind::Center);
        let diagonal = line(&mut doc, [0.0, 0.0], [100.0, 50.0], guides);
        doc.set_style(diagonal, LineKind::Construction);
        let middle = doc.add_point(DVec2::new(50.0, 25.0));
        doc.set_entity_layer(middle, guides).unwrap();
        let circle = doc.add_circle(middle, 20.0).unwrap();
        doc.set_entity_layer(circle, guides).unwrap();
        doc.set_style(circle, LineKind::Construction);
        let red = line(&mut doc, [0.0, 50.0], [100.0, 50.0], guides);
        doc.set_style(red, LineKind::Construction);
        let look = Appearance {
            color: Some(Rgb([200, 0, 0])),
            ..Default::default()
        };
        doc.set_appearance(red, look).unwrap();
        let c = [[110.0, 0.0], [130.0, 0.0], [130.0, 20.0], [110.0, 20.0]];
        let mut edges = Vec::new();
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            let edge = line(&mut doc, a, b, import);
            doc.set_style(edge, LineKind::Construction);
            edges.push(edge);
        }
        let mut fill = Fill::new(edges, DVec2::new(120.0, 10.0), FillKind::Solid);
        fill.layer = import;
        doc.add_fill(fill);
        let mut view = View::new(
            "Plan",
            Bounds {
                min: DVec2::new(-20.0, -20.0),
                max: DVec2::new(150.0, 80.0),
            },
            1.0,
        );
        view.construction = construction;
        let id = doc.add_view(view);
        let mut sheet = Sheet::new("A-101");
        sheet.colours = colours;
        sheet
            .viewports
            .push(Viewport::new(id, DVec2::new(200.0, 150.0)));
        doc.add_sheet(sheet.clone());
        let views = doc.placed_views(&sheet);
        let mut ed = Editor::new(doc);
        sheet_drawing(
            &mut ed,
            &PatternLibrary::default(),
            &painter(),
            &sheet,
            &views,
            (1, 1),
        )
    }

    fn on<'a>(d: &'a Drawing, layer: &'a str) -> impl Iterator<Item = &'a Item> {
        d.items.iter().filter(move |i| i.layer == layer)
    }

    /// Hairline dashes: how construction curves print.
    fn dashed(d: &Drawing, layer: &str) -> Vec<Item> {
        on(d, layer)
            .filter(|i| i.pen.line_type == LineType::Dashed && i.pen.width == HAIRLINE)
            .cloned()
            .collect()
    }

    #[test]
    fn the_model_exports_at_true_size_on_a_fitted_page() {
        let mut doc = Document::new();
        let profile = doc.add_layer(Layer::new("Profile"));
        let mut off = Layer::new("Off");
        off.printable = false;
        let off = doc.add_layer(off);
        let base = line(&mut doc, [0.0, 0.0], [100.0, 0.0], profile);
        let side = line(&mut doc, [100.0, 0.0], [100.0, 40.0], profile);
        let guide = line(&mut doc, [-500.0, 0.0], [500.0, 0.0], profile);
        doc.set_style(guide, LineKind::Construction);
        line(&mut doc, [0.0, 0.0], [0.0, 900.0], off);
        let ends: Vec<EntityId> = doc.defining_points([base]).into_iter().collect();
        doc.add_constraint(drift_core::Constraint::Distance {
            a: ends[0],
            b: ends[1],
            value: 100.0,
        })
        .unwrap();
        let mut ed = Editor::new(doc);
        let painter = painter();
        let patterns = PatternLibrary::default();
        let page = |ed: &mut Editor, only: Option<&Scope>, annotate| {
            model_page(ed, &patterns, &painter, only, annotate)
        };
        // Without its dimension: the two lines, 1 : 1, 5 mm in from the page's edges. The
        // construction line, points and the layer that doesn't print are left out.
        let bare = page(&mut ed, None, false);
        assert_eq!(bare.size, DVec2::new(110.0, 50.0));
        assert_eq!(bare.items.len(), 2);
        assert!(bare.items.iter().all(|i| i.layer == "Profile"));
        assert!(bare.items.contains(&item(
            "Profile",
            bare.items[0].pen,
            Prim::Line(DVec2::new(5.0, 5.0), DVec2::new(105.0, 5.0)),
            None,
        )));
        assert_eq!(
            bare.layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["Profile"]
        );
        // With it, on a layer of its own.
        let dimensioned = page(&mut ed, None, true);
        assert!(on(&dimensioned, ANNOTATION_LAYER).count() > 0);
        // Only the picked line (the dimension on the other one isn't on it).
        ed.selection = [side].into();
        let only = Scope::of_selection(&ed, None).expect("a pick");
        let picked = page(&mut ed, Some(&only), true);
        assert_eq!(picked.items.len(), 1, "{:?}", picked.items);
        assert_eq!(picked.size, DVec2::new(10.0, 50.0));
        ed.selection.clear();
        assert!(Scope::of_selection(&ed, None).is_none());
    }

    #[test]
    fn construction_stays_off_sheets_unless_the_view_shows_it() {
        let d = sheet(false, SheetColours::Colour);
        assert!(dashed(&d, "Guides").is_empty());
        // The drawn line and the centre line print as before.
        assert_eq!(on(&d, "Guides").count(), 2);
        assert!(on(&d, "Guides").any(|i| i.pen.line_type == LineType::Center));
        assert!(
            on(&d, "Import").all(|i| matches!(i.prim, Prim::Fill { .. })),
            "an import's fill, without its edges"
        );
    }

    #[test]
    fn a_view_can_show_construction_as_light_dashes() {
        let d = sheet(true, SheetColours::Colour);
        let guides = dashed(&d, "Guides");
        assert_eq!(guides.len(), 3, "the diagonal, the circle and the red line");
        assert!(guides.iter().any(|i| matches!(i.prim, Prim::Circle { .. })));
        let grey = guides
            .iter()
            .filter(|i| i.pen.color == CONSTRUCTION_GREY)
            .count();
        assert_eq!(grey, 2);
        assert!(
            guides.iter().any(|i| i.pen.color == Rgb([200, 0, 0])),
            "a curve's own colour is kept"
        );
        // Clipped to the view's frame like the rest of it.
        assert!(guides.iter().all(|i| i.clip.is_some()));
        // The drawn and centre lines are unchanged; points and an import's fill edges stay
        // out.
        assert_eq!(on(&d, "Guides").count(), 5);
        assert!(on(&d, "Import").all(|i| matches!(i.prim, Prim::Fill { .. })));
        assert!(
            !d.items
                .iter()
                .any(|i| matches!(i.prim, Prim::Line(a, b) if a == b))
        );
    }

    #[test]
    fn construction_follows_the_sheets_colours() {
        let mono = sheet(true, SheetColours::BlackWhite);
        let guides = dashed(&mono, "Guides");
        assert_eq!(guides.len(), 3);
        assert!(guides.iter().all(|i| i.pen.color == Pen::BLACK));
        let grey = sheet(true, SheetColours::Greyscale);
        let v = drift_core::sheets::luminance(CONSTRUCTION_GREY);
        assert!(
            dashed(&grey, "Guides")
                .iter()
                .any(|i| i.pen.color == Rgb([v, v, v]))
        );
    }

    /// Sheet text zoomed far in (issue #81): past what egui's font atlas holds, it is drawn
    /// as filled outlines, where the laid-out text would have been.
    #[test]
    fn text_zoomed_to_the_maximum_is_outlined() {
        let mut d = Drawing::new(DVec2::new(420.0, 297.0));
        let at = DVec2::new(100.0, 50.0);
        d.text("T", Pen::new(0.25), at, 5.0, Align::Center, "HELLO");
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        // The preview's largest zoom: 200 points per mm, so 5 mm capitals are ~1400 px.
        for zoom in [1.0_f32, 200.0] {
            let to_screen = |p: DVec2| {
                let v = (p - at) * f64::from(zoom);
                Pos2::new(500.0 + v.x as f32, 500.0 - v.y as f32)
            };
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint_drawing(ui.painter(), &d, to_screen, zoom);
            });
            out.textures_delta.clear();
            let mut text = 0;
            let mut points = Vec::new();
            for s in &out.shapes {
                match &s.shape {
                    egui::Shape::Text(t) => {
                        text += 1;
                        for r in &t.galley.rows {
                            for g in &r.row.glyphs {
                                assert!(g.font_height <= MAX_GLYPH_PX * 2.0, "{zoom}");
                            }
                        }
                    }
                    egui::Shape::Mesh(m) => points.extend(m.vertices.iter().map(|v| v.pos)),
                    _ => {}
                }
            }
            if zoom > 100.0 {
                assert_eq!(text, 0);
                assert!(!points.is_empty());
                // Centred on `at`, standing on its baseline.
                let r = egui::Rect::from_points(&points);
                assert!((r.center().x - 500.0).abs() < 0.02 * r.width(), "{r:?}");
                assert!((r.bottom() - 500.0).abs() < 0.05 * r.height(), "{r:?}");
            } else {
                assert_eq!(text, 1);
            }
        }
    }

    #[test]
    fn the_preview_prints_and_exports_as_shown() {
        // PDF, SVG and print are made from the sheet's drawing: the construction is in each.
        let d = sheet(true, SheetColours::Colour);
        let svg = drift_io::svg::write(&d);
        let dashes = svg.matches("stroke-dasharray").count();
        assert!(dashes >= 3, "{svg}");
        let off = drift_io::svg::write(&sheet(false, SheetColours::Colour));
        assert!(off.matches("stroke-dasharray").count() < dashes);
    }
}
