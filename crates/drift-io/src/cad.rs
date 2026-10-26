//! DWG and DXF R2018 (AC1032) export, through acadrust: one mapping from a [`Drawing`] into
//! acadrust's document, which then writes either format, so both carry the same thing.
//!
//! The drawing is model space at full size, made for CAD (see `output::cad_drawing` in the
//! app): curves stay true (LINE, ARC, CIRCLE, ELLIPSE, and SPLINE for Bézier curves, a run
//! of joined ones as one), fills and hatches stay areas (HATCH, solid or with the pattern's
//! own line families, so any reader draws them exactly), text is TEXT in a style per font,
//! and blocks are BLOCK and INSERT. Layers keep their names, true colours, line types and
//! weights; items say only what differs from their layer. Dimensions and notes come as
//! the lines, fills and text they are drawn with.
//!
//! [`CadWriter`] adds one kind of thing at a time (layers, line types, text styles, blocks,
//! entities), so other objects (walls, later) can add their own entities to the same
//! document before it is written.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use acadrust::entities::{
    BoundaryPath, BoundaryPathFlags, Hatch, HatchPattern, HatchPatternLine, HatchPatternType,
    Insert, PolylineEdge, TextHorizontalAlignment,
};
use acadrust::tables::LineTypeElement;
use acadrust::{
    Arc, BlockRecord, CadDocument, Circle, Color, DwgWriter, DxfVersion, DxfWriter, Ellipse,
    EntityType, Handle, Layer, Line, LineType as CadLineType, LineWeight, LwPolyline, Spline, Text,
    TextStyle, Transparency, Vector2, Vector3,
};
use drift_core::{DVec2, LineType, Rgb, TextFont};
use drift_draft::page::{Align, BlockDef, Drawing, HatchPrim, Item, Prim, dash_pattern};
use drift_geom::regions::{Edge, Loop};

use crate::dxf::{DxfUnits, line_type_name};

/// The version written: AutoCAD 2018 and later, and every program that reads it.
pub const VERSION: DxfVersion = DxfVersion::AC1032;

/// The line weights AutoCAD offers (hundredths of a millimetre).
const WEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

/// The line weight AutoCAD offers nearest to `mm`.
fn weight(mm: f64) -> LineWeight {
    let w = (mm * 100.0).round();
    let nearest = WEIGHTS
        .iter()
        .min_by(|a, b| {
            (f64::from(**a) - w)
                .abs()
                .total_cmp(&(f64::from(**b) - w).abs())
        })
        .copied()
        .unwrap_or(0);
    LineWeight::Value(nearest)
}

/// A colour as CAD has it: Drift's black is AutoCAD's colour 7 (black on white paper, white
/// on a dark screen); every other colour is kept exactly, as a true colour.
fn color(c: Rgb) -> Color {
    if c == Rgb([0, 0, 0]) {
        Color::Index(7)
    } else {
        let [r, g, b] = c.0;
        Color::Rgb { r, g, b }
    }
}

/// A name for a layer, block or style that AutoCAD accepts: the characters it refuses
/// (`<>/\":;?*|=,` and the backquote) become `_`; everything else is kept.
pub fn cad_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>/\\\":;?*|=,`".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(255)
        .collect();
    if s.is_empty() { "0".into() } else { s }
}

/// Text for TEXT: `%%` would start a control code (`%%c` is Ø), so a percent sign before
/// another is written `%%%`. Other characters go as they are (R2018 files are UTF-8).
fn text_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '%' if chars.peek() == Some(&'%') => out.push_str("%%%"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn v2(p: DVec2) -> Vector2 {
    Vector2::new(p.x, p.y)
}

fn v3(p: DVec2) -> Vector3 {
    Vector3::new(p.x, p.y, 0.0)
}

/// How a layer looks, for leaving out what its items share with it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    color: Rgb,
    width: f64,
    line_type: LineType,
}

/// Builds a DWG / DXF R2018 document from drawings. See the module notes.
pub struct CadWriter {
    doc: CadDocument,
    /// By the name written.
    layers: BTreeMap<String, Look>,
    /// Drift names to the names written (which are unique ignoring case, as CAD needs).
    layer_names: BTreeMap<String, String>,
    block_names: BTreeMap<String, String>,
    /// Text style by font (`None`: the drawing's font).
    styles: Vec<(Option<TextFont>, String)>,
}

impl CadWriter {
    /// An empty drawing in `units`, with Drift's line types.
    pub fn new(units: DxfUnits) -> CadWriter {
        let mut doc = CadDocument::with_version(VERSION);
        let h = &mut doc.header;
        h.insertion_units = units.insunits as i16;
        h.linear_unit_format = units.lunits as i16;
        h.linear_unit_precision = units.luprec as i16;
        h.measurement = units.measurement as i16;
        let mut w = CadWriter {
            doc,
            layers: BTreeMap::new(),
            layer_names: BTreeMap::new(),
            block_names: BTreeMap::new(),
            styles: Vec::new(),
        };
        w.line_types();
        w
    }

    /// The document built so far, to add more to.
    pub fn document_mut(&mut self) -> &mut CadDocument {
        &mut self.doc
    }

    pub fn into_document(self) -> CadDocument {
        self.doc
    }

    /// The document as a DWG file.
    pub fn dwg(&self) -> Result<Vec<u8>, String> {
        DwgWriter::write_to_vec(&self.doc).map_err(|e| format!("could not write the DWG: {e}"))
    }

    /// The document as an ASCII DXF file.
    pub fn dxf(&self) -> Result<Vec<u8>, String> {
        DxfWriter::new(&self.doc)
            .write_to_vec()
            .map_err(|e| format!("could not write the DXF: {e}"))
    }

    /// Drift's line types (lengths in mm on paper, scaled by `$LTSCALE`, as in R12).
    fn line_types(&mut self) {
        for t in [
            LineType::Dashed,
            LineType::Hidden,
            LineType::Center,
            LineType::Phantom,
        ] {
            let pattern = dash_pattern(t);
            let mut lt = CadLineType::new(line_type_name(t));
            lt.description = t.label().to_owned();
            for (k, v) in pattern.iter().enumerate() {
                lt.add_element(if k % 2 == 0 {
                    LineTypeElement::dash(*v)
                } else {
                    LineTypeElement::space(*v)
                });
            }
            lt.pattern_length = pattern.iter().sum();
            let _ = self.doc.line_types.add(lt);
        }
    }

    /// The line type an item names (empty: by layer).
    fn line_type(t: LineType) -> String {
        match t {
            LineType::Continuous | LineType::None => "Continuous".into(),
            t => line_type_name(t).into(),
        }
    }

    /// Add a drawing (in world millimetres): its layers, blocks and items, in `units`
    /// (those given to [`CadWriter::new`]).
    pub fn add_drawing(&mut self, d: &Drawing, units: DxfUnits) {
        let d = &d.scaled(1.0 / units.mm_per_unit);
        self.doc.header.linetype_scale = d.line_type_scale;
        let b = d.bounds();
        if !b.is_empty() {
            self.doc.header.model_space_extents_min = v3(b.min);
            self.doc.header.model_space_extents_max = v3(b.max);
        }
        self.add_layers(d);
        for block in &d.blocks {
            self.add_block(block);
        }
        let owner = self.doc.header.model_space_block_handle;
        self.add_items(&d.items, owner);
    }

    /// The drawing's layers, then any others its items use (in their first item's look).
    fn add_layers(&mut self, d: &Drawing) {
        for l in &d.layers {
            self.layer(
                &l.name,
                Look {
                    color: l.color,
                    width: l.width,
                    line_type: l.line_type,
                },
            );
        }
        // Layer 0 (blocks' items are on it, to take their copy's layer) keeps AutoCAD's own
        // look unless the drawing has a layer 0 of its own.
        if !self.layer_names.contains_key("0") {
            self.layer_names.insert("0".into(), "0".into());
            self.layers.insert(
                "0".into(),
                Look {
                    color: Rgb([0, 0, 0]),
                    width: f64::NAN,
                    line_type: LineType::Continuous,
                },
            );
        }
        for item in d
            .items
            .iter()
            .chain(d.blocks.iter().flat_map(|b| b.items.iter()))
        {
            if !self.layer_names.contains_key(&item.layer) {
                self.layer(
                    &item.layer,
                    Look {
                        color: item.pen.color,
                        width: item.pen.width,
                        line_type: LineType::Continuous,
                    },
                );
            }
        }
    }

    /// A unique name (ignoring case) for `name` among `taken`.
    fn unique(name: &str, taken: &BTreeMap<String, String>) -> String {
        let base = cad_name(name);
        let clash = |n: &str| taken.values().any(|t| t.eq_ignore_ascii_case(n));
        if !clash(&base) {
            return base;
        }
        (2..)
            .map(|k| format!("{base} ({k})"))
            .find(|n| !clash(n))
            .unwrap_or(base)
    }

    /// Add a layer, unless one of the name is there (the first look wins).
    fn layer(&mut self, name: &str, look: Look) -> String {
        if let Some(n) = self.layer_names.get(name) {
            return n.clone();
        }
        let written = if name == "0" {
            "0".to_owned()
        } else {
            Self::unique(name, &self.layer_names)
        };
        self.layer_names.insert(name.to_owned(), written.clone());
        self.layers.insert(written.clone(), look);
        let layer = match self.doc.layers.get_mut(&written) {
            Some(l) => l,
            None => {
                let _ = self.doc.layers.add(Layer::new(written.clone()));
                match self.doc.layers.get_mut(&written) {
                    Some(l) => l,
                    None => return written,
                }
            }
        };
        layer.color = color(look.color);
        layer.line_type = Self::line_type(look.line_type);
        layer.line_weight = weight(look.width);
        // AutoCAD refuses a drawing whose Defpoints layer plots.
        layer.is_plottable = !written.eq_ignore_ascii_case("Defpoints");
        written
    }

    /// A text style per font: Drift's own fonts as the nearest fonts every Windows and macOS
    /// computer has; a system font by its family name.
    fn style(&mut self, font: Option<&TextFont>) -> String {
        if let Some((_, s)) = self.styles.iter().find(|(f, _)| f.as_ref() == font) {
            return s.clone();
        }
        let (name, file, family) = match font {
            None => ("Standard".to_owned(), "arial.ttf", "Arial".to_owned()),
            Some(TextFont::Sans) => ("Drift Sans".to_owned(), "arial.ttf", "Arial".to_owned()),
            Some(TextFont::Mono) => (
                "Drift Mono".to_owned(),
                "cour.ttf",
                "Courier New".to_owned(),
            ),
            Some(TextFont::System(family)) => (cad_name(family), "arial.ttf", family.clone()),
        };
        let mut style = match self.doc.text_styles.get(&name) {
            Some(s) => s.clone(),
            None => TextStyle::new(name.clone()),
        };
        style.font_file = file.to_owned();
        style.true_type_font = family;
        self.doc.text_styles.add_or_replace(style);
        self.styles.push((font.cloned(), name.clone()));
        name
    }

    /// A block definition, and its items (on layer 0 they take the copy's layer).
    fn add_block(&mut self, block: &BlockDef) {
        let name = Self::unique(&block.name, &self.block_names);
        self.block_names.insert(block.name.clone(), name.clone());
        let mut record = BlockRecord::new(name);
        let handle = self.doc.allocate_handle();
        record.handle = handle;
        record.block_entity_handle = self.doc.allocate_handle();
        record.block_end_handle = self.doc.allocate_handle();
        if self.doc.block_records.add(record).is_err() {
            return;
        }
        self.add_items(&block.items, handle);
    }

    /// Entities for `items`, owned by the block record `owner` (model space or a block).
    fn add_items(&mut self, items: &[Item], owner: Handle) {
        let mut k = 0;
        while k < items.len() {
            // A run of joined Bézier curves in one pen is one spline.
            if let Prim::Bezier { p } = &items[k].prim {
                let mut points = vec![p[0], p[1], p[2], p[3]];
                let mut end = k + 1;
                while let Some(next) = items.get(end)
                    && let Prim::Bezier { p: q } = &next.prim
                    && next.layer == items[k].layer
                    && next.pen == items[k].pen
                    && points.last().is_some_and(|l| l.distance(q[0]) < 1e-9)
                {
                    points.extend_from_slice(&q[1..]);
                    end += 1;
                }
                self.spline(&items[k], &points, owner);
                k = end;
                continue;
            }
            self.item(&items[k], owner);
            k += 1;
        }
    }

    /// Fill in what an entity shares with every other: its layer, and its colour, line
    /// type and weight where they aren't its layer's.
    fn common(&mut self, entity: &mut EntityType, item: &Item, owner: Handle, stroke: bool) {
        let layer = self.layer(
            &item.layer,
            Look {
                color: item.pen.color,
                width: item.pen.width,
                line_type: LineType::Continuous,
            },
        );
        // In a block, everything is said: "by layer" there would be the copy's layer, and
        // a block's items carry their own look.
        let look = self
            .layers
            .get(&layer)
            .copied()
            .filter(|_| owner == self.doc.header.model_space_block_handle);
        let c = entity.common_mut();
        c.owner_handle = owner;
        if look.is_none_or(|l| l.color != item.pen.color) {
            c.color = color(item.pen.color);
        }
        if stroke {
            if look.is_none_or(|l| l.line_type != item.pen.line_type) {
                c.linetype = Self::line_type(item.pen.line_type);
            }
            if look.is_none_or(|l| l.width.is_nan() || (l.width - item.pen.width).abs() > 1e-9) {
                c.line_weight = weight(item.pen.width);
            }
        }
        c.layer = layer;
    }

    fn add(&mut self, mut entity: EntityType, item: &Item, owner: Handle, stroke: bool) {
        self.common(&mut entity, item, owner, stroke);
        let _ = self.doc.add_entity(entity);
    }

    /// One item (a run of Bézier curves is [`CadWriter::spline`]'s).
    fn item(&mut self, item: &Item, owner: Handle) {
        let entity = match &item.prim {
            Prim::Line(a, b) => EntityType::Line(Line::from_points(v3(*a), v3(*b))),
            Prim::Polyline { points, closed } => {
                let mut pl = LwPolyline::from_points(points.iter().map(|p| v2(*p)).collect());
                pl.is_closed = *closed;
                EntityType::LwPolyline(pl)
            }
            Prim::Circle { center, radius } => {
                EntityType::Circle(Circle::from_center_radius(v3(*center), *radius))
            }
            Prim::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if *sweep >= TAU - 1e-12 {
                    EntityType::Circle(Circle::from_center_radius(v3(*center), *radius))
                } else {
                    EntityType::Arc(Arc {
                        center: v3(*center),
                        radius: *radius,
                        start_angle: start.rem_euclid(TAU),
                        end_angle: (start + sweep).rem_euclid(TAU),
                        ..Arc::new()
                    })
                }
            }
            Prim::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                // CAD's major axis is the longer one.
                let (axis, ratio, start) = if *minor > major.length() {
                    (
                        major.perp().normalize_or_zero() * *minor,
                        major.length() / minor,
                        start - PI / 2.0,
                    )
                } else {
                    (*major, minor / major.length().max(1e-300), *start)
                };
                let whole = *sweep >= TAU - 1e-12;
                let mut e = Ellipse::from_center_axes(v3(*center), v3(axis), ratio.min(1.0));
                (e.start_parameter, e.end_parameter) = if whole {
                    (0.0, TAU)
                } else {
                    let s = start.rem_euclid(TAU);
                    (s, (s + sweep).rem_euclid(TAU))
                };
                EntityType::Ellipse(e)
            }
            Prim::Bezier { p } => {
                self.spline(item, p, owner);
                return;
            }
            Prim::Fill { rings, mask, .. } => {
                if *mask {
                    return;
                }
                let mut hatch = Hatch::solid();
                for (k, ring) in rings.iter().enumerate() {
                    let mut path = BoundaryPath::with_flags(outer_flag(k == 0));
                    path.add_edge(acadrust::entities::BoundaryEdge::Polyline(
                        PolylineEdge::new(ring.iter().map(|p| v2(*p)).collect(), true),
                    ));
                    hatch.add_path(path);
                }
                if hatch.paths.is_empty() {
                    return;
                }
                EntityType::Hatch(Box::new(hatch))
            }
            Prim::Hatch(h) => {
                let Some(hatch) = hatch(h) else { return };
                self.add(
                    EntityType::Hatch(Box::new(hatch)),
                    item,
                    owner,
                    h.pattern.is_some(),
                );
                return;
            }
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
                style,
                font,
            } => {
                let value = text_value(text);
                if value.trim().is_empty() {
                    return;
                }
                let mut t = Text::with_value(value, v3(*at)).with_height(*height);
                t.rotation = *angle;
                t.style = self.style(font.as_ref());
                if style.italic {
                    t.oblique_angle = drift_draft::page::ITALIC_SLANT.atan();
                }
                t.horizontal_alignment = match align {
                    Align::Left => TextHorizontalAlignment::Left,
                    Align::Center => TextHorizontalAlignment::Center,
                    Align::Right => TextHorizontalAlignment::Right,
                };
                if *align != Align::Left {
                    t.alignment_point = Some(v3(*at));
                }
                EntityType::Text(t)
            }
            Prim::Insert {
                block,
                at,
                angle,
                scale,
                mirror,
            } => {
                let name = self
                    .block_names
                    .get(block)
                    .cloned()
                    .unwrap_or_else(|| cad_name(block));
                // A mirrored copy is a negative X scale (mirroring before turning).
                let x = if *mirror { -*scale } else { *scale };
                let insert = Insert::new(name, v3(*at))
                    .with_scale(x, *scale, *scale)
                    .with_rotation(angle.to_radians());
                let mut entity = EntityType::Insert(insert);
                // A copy takes its layer's look; its block's items carry their own.
                let layer = self.layer(
                    &item.layer,
                    Look {
                        color: item.pen.color,
                        width: item.pen.width,
                        line_type: LineType::Continuous,
                    },
                );
                let c = entity.common_mut();
                c.owner_handle = owner;
                c.layer = layer;
                let _ = self.doc.add_entity(entity);
                return;
            }
        };
        let stroke = !matches!(item.prim, Prim::Fill { .. } | Prim::Text { .. });
        self.add(entity, item, owner, stroke);
    }

    /// A cubic spline through joined Bézier curves (`points`: the first curve's four, then
    /// three for each next), exact: each curve is one span, its ends knots of full
    /// multiplicity.
    fn spline(&mut self, item: &Item, points: &[DVec2], owner: Handle) {
        let spans = (points.len() - 1) / 3;
        let mut knots = vec![0.0; 4];
        for k in 1..spans {
            knots.extend([k as f64; 3]);
        }
        knots.extend([spans as f64; 4]);
        let mut s = Spline::from_control_points(3, points.iter().map(|p| v3(*p)).collect());
        s.knots = knots;
        s.flags.planar = true;
        s.normal = Vector3::new(0.0, 0.0, 1.0);
        self.add(EntityType::Spline(s), item, owner, true);
    }
}

/// Boundary path flags: the first loop of a fill is its outer one (holes follow).
fn outer_flag(outer: bool) -> BoundaryPathFlags {
    let mut f = BoundaryPathFlags::new();
    f.set_external(outer);
    f
}

/// A loop as a closed polyline with bulges (arcs over a half turn are split, so no bulge
/// is unbounded).
fn polyline_edge(lp: &Loop) -> PolylineEdge {
    let mut edge = PolylineEdge::new(Vec::new(), true);
    for e in &lp.edges {
        match *e {
            Edge::Line { a, .. } => edge.add_vertex(v2(a), 0.0),
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = (sweep.abs() / PI).ceil().max(1.0) as usize;
                let step = sweep / n as f64;
                for k in 0..n {
                    let a = start + step * k as f64;
                    edge.add_vertex(
                        v2(center + DVec2::from_angle(a) * radius),
                        (step / 4.0).tan(),
                    );
                }
            }
        }
    }
    edge
}

/// A HATCH for a fill or hatch: its loops (outer first), and a hatch's pattern line
/// families placed in the drawing as AutoCAD stores them (scaled and turned, each family
/// through its own base point, its step and dashes in drawing units).
fn hatch(h: &HatchPrim) -> Option<Hatch> {
    let loops = std::iter::once(&h.region.outer).chain(&h.region.holes);
    let mut hatch = match &h.pattern {
        None => Hatch::solid(),
        Some((pattern, placement)) => {
            let s = placement.scale;
            let turn = DVec2::from_angle(placement.angle.to_radians());
            let mut def = HatchPattern::new(cad_name(&pattern.name).replace(' ', "_"));
            def.description = pattern.description.clone();
            for l in &pattern.lines {
                let a = (l.angle + placement.angle).to_radians();
                def.add_line(HatchPatternLine {
                    angle: a,
                    base_point: v2(placement.origin + turn.rotate(l.origin * s)),
                    offset: v2(DVec2::from_angle(a).rotate(l.delta * s)),
                    dash_lengths: l.dashes.iter().map(|d| d * s).collect(),
                });
            }
            let mut hatch = Hatch::with_pattern(def);
            hatch.is_solid = false;
            hatch.pattern_type = if drift_draft::find(&pattern.name).is_some() {
                HatchPatternType::Predefined
            } else {
                HatchPatternType::Custom
            };
            hatch.pattern_angle = placement.angle.to_radians();
            hatch.pattern_scale = s;
            hatch
        }
    };
    for (k, lp) in loops.enumerate() {
        if lp.edges.is_empty() {
            continue;
        }
        let mut path = BoundaryPath::with_flags(outer_flag(k == 0));
        path.add_edge(acadrust::entities::BoundaryEdge::Polyline(polyline_edge(
            lp,
        )));
        hatch.add_path(path);
    }
    if hatch.paths.is_empty() {
        return None;
    }
    if h.pattern.is_none() && h.opacity < 1.0 {
        hatch.common.transparency =
            Transparency::Explicit(((1.0 - h.opacity.clamp(0.0, 1.0)) * 255.0).round() as u8);
    }
    Some(hatch)
}

/// A drawing (in world millimetres) as a DWG R2018 file, in `units`.
pub fn write_dwg(d: &Drawing, units: DxfUnits) -> Result<Vec<u8>, String> {
    let mut w = CadWriter::new(units);
    w.add_drawing(d, units);
    w.dwg()
}

/// A drawing (in world millimetres) as a DXF R2018 file, in `units`.
pub fn write_dxf(d: &Drawing, units: DxfUnits) -> Result<Vec<u8>, String> {
    let mut w = CadWriter::new(units);
    w.add_drawing(d, units);
    w.dxf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_weights_are_ones_autocad_takes() {
        assert_eq!(cad_name("Walls: new/old"), "Walls_ new_old");
        assert_eq!(cad_name("  "), "0");
        assert_eq!(cad_name("Möbel"), "Möbel");
        assert_eq!(weight(0.13), LineWeight::Value(13));
        assert_eq!(weight(0.36), LineWeight::Value(35));
        assert_eq!(weight(3.0), LineWeight::Value(211));
        assert_eq!(text_value("50%%c"), "50%%%%c");
        assert_eq!(text_value("Ø10 ±0.1 €"), "Ø10 ±0.1 €");
        assert_eq!(color(Rgb([0, 0, 0])), Color::Index(7));
        assert_eq!(color(Rgb([1, 2, 3])), Color::Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn defpoints_never_plots() {
        // AutoCAD discards a DXF whose Defpoints layer plots.
        let mut d = Drawing::new(DVec2::ZERO);
        for name in ["Defpoints", "Walls"] {
            d.layer(drift_draft::page::LayerInfo {
                name: name.into(),
                color: Rgb([0, 0, 0]),
                width: 0.25,
                line_type: LineType::Continuous,
            });
        }
        let mut w = CadWriter::new(DxfUnits::MM);
        w.add_drawing(&d, DxfUnits::MM);
        let doc = w.into_document();
        assert!(!doc.layers.get("Defpoints").unwrap().is_plottable);
        assert!(doc.layers.get("Walls").unwrap().is_plottable);
    }
}
