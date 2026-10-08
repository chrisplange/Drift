//! DXF: writing a [`Drawing`] as R12 (AC1009) ASCII DXF, which every CAD program reads, and
//! reading lines, circles, arcs, polylines (with bulges) and points from any ASCII DXF.
//!
//! Output keeps layers (colour and line type) and line types; R12 has no line weights, so
//! they are dropped. Fills become SOLID triangles. Text uses DXF's `%%c` / `%%d` / `%%p`
//! codes for Ø, ° and ±.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::fmt::Write as _;

use drift_core::{
    DVec2, Document, EntityId, Layer, LayerId, LengthFormat, LineType, LineWeight, Rgb, Shape,
    Units,
};
use drift_draft::page::{Align, Drawing, Prim, dash_pattern};

/// The AutoCAD Color Index entries we map to and from (index, colour).
const ACI: [(u8, [u8; 3]); 14] = [
    (1, [255, 0, 0]),
    (2, [255, 255, 0]),
    (3, [0, 255, 0]),
    (4, [0, 255, 255]),
    (5, [0, 0, 255]),
    (6, [255, 0, 255]),
    (7, [0, 0, 0]),
    (8, [128, 128, 128]),
    (9, [192, 192, 192]),
    (30, [255, 127, 0]),
    (250, [51, 51, 51]),
    (251, [80, 80, 80]),
    (252, [105, 105, 105]),
    (253, [130, 130, 130]),
];

/// The nearest ACI colour (7 = black / white for black).
pub fn aci(c: Rgb) -> u8 {
    let d = |a: [u8; 3]| -> i32 {
        (0..3)
            .map(|i| (i32::from(a[i]) - i32::from(c.0[i])).pow(2))
            .sum()
    };
    ACI.iter()
        .min_by_key(|(_, rgb)| d(*rgb))
        .map_or(7, |(i, _)| *i)
}

fn aci_rgb(index: i64) -> Option<Rgb> {
    ACI.iter()
        .find(|(i, _)| i64::from(*i) == index)
        .map(|(_, c)| Rgb(*c))
        // 7 is "black or white by background": layers of that colour use the default.
        .filter(|_| index != 7)
}

fn line_type_name(t: LineType) -> &'static str {
    match t {
        LineType::Continuous => "CONTINUOUS",
        LineType::Dashed => "DASHED",
        LineType::Hidden => "HIDDEN",
        LineType::Center => "CENTER",
        LineType::Phantom => "PHANTOM",
    }
}

fn parse_line_type(name: &str) -> LineType {
    match name.to_ascii_uppercase().as_str() {
        n if n.starts_with("DASHED") => LineType::Dashed,
        n if n.starts_with("HIDDEN") => LineType::Hidden,
        n if n.starts_with("CENTER") => LineType::Center,
        n if n.starts_with("PHANTOM") => LineType::Phantom,
        _ => LineType::Continuous,
    }
}

/// A layer name DXF accepts.
fn layer_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-$".contains(c) {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "0".into() } else { s }
}

fn dxf_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'Ø' | 'ø' => "%%c".to_owned(),
            '°' => "%%d".to_owned(),
            '±' => "%%p".to_owned(),
            '−' | '–' => "-".to_owned(),
            '×' => "x".to_owned(),
            c if c.is_ascii() && !c.is_ascii_control() => c.to_string(),
            _ => "?".to_owned(),
        })
        .collect()
}

struct Out(String);

impl Out {
    fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.0, "{code:>3}\n{value}\n");
    }

    fn num(&mut self, code: i32, v: f64) {
        let s = format!("{v:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        self.pair(code, if s == "-0" { "0" } else { s });
    }

    fn point(&mut self, base: i32, p: DVec2) {
        self.num(base, p.x);
        self.num(base + 10, p.y);
        self.num(base + 20, 0.0);
    }
}

/// The drawing units a DXF file is written in, and how CAD programs should show lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfUnits {
    /// Millimetres per drawing unit.
    pub mm_per_unit: f64,
    /// `$INSUNITS`: 1 inches, 2 feet, 4 mm, 5 cm, 6 m.
    pub insunits: i32,
    /// `$LUNITS`: 2 decimal, 4 architectural.
    pub lunits: i32,
    /// `$LUPREC`: decimal places (or the fraction's power of two for architectural).
    pub luprec: i32,
    /// `$MEASUREMENT`: 0 imperial, 1 metric.
    pub measurement: i32,
}

impl DxfUnits {
    pub const MM: DxfUnits = DxfUnits {
        mm_per_unit: 1.0,
        insunits: 4,
        lunits: 2,
        luprec: 3,
        measurement: 1,
    };

    /// Drawing units matching a document's display units, so lengths read the same in other
    /// CAD programs (architectural documents are in inches, as AutoCAD's are).
    pub fn of(units: &Units) -> DxfUnits {
        let decimals = i32::from(units.decimals);
        let (insunits, lunits, measurement) = match units.length {
            LengthFormat::Millimetre => (4, 2, 1),
            LengthFormat::Centimetre => (5, 2, 1),
            LengthFormat::Metre => (6, 2, 1),
            LengthFormat::Inch => (1, 2, 0),
            LengthFormat::Foot => (2, 2, 0),
            LengthFormat::Architectural => (1, 4, 0),
        };
        DxfUnits {
            mm_per_unit: units.length.mm_per_unit(),
            insunits,
            lunits,
            luprec: if lunits == 4 {
                i32::from(units.fraction).trailing_zeros() as i32
            } else {
                decimals
            },
            measurement,
        }
    }
}

/// Write a drawing (in world millimetres) as R12 DXF, in `units`.
pub fn write(d: &Drawing, units: DxfUnits) -> String {
    let d = &d.scaled(1.0 / units.mm_per_unit);
    let mut o = Out(String::new());
    let b = d.bounds();
    let (min, max) = if b.is_empty() {
        (DVec2::ZERO, d.size)
    } else {
        (b.min, b.max)
    };
    o.pair(0, "SECTION");
    o.pair(2, "HEADER");
    o.pair(9, "$ACADVER");
    o.pair(1, "AC1009");
    o.pair(9, "$INSUNITS");
    o.pair(70, units.insunits);
    o.pair(9, "$MEASUREMENT");
    o.pair(70, units.measurement);
    o.pair(9, "$LUNITS");
    o.pair(70, units.lunits);
    o.pair(9, "$LUPREC");
    o.pair(70, units.luprec);
    o.pair(9, "$LTSCALE");
    o.num(40, d.line_type_scale);
    o.pair(9, "$EXTMIN");
    o.point(10, min);
    o.pair(9, "$EXTMAX");
    o.point(10, max);
    o.pair(0, "ENDSEC");

    o.pair(0, "SECTION");
    o.pair(2, "TABLES");
    // Line types.
    o.pair(0, "TABLE");
    o.pair(2, "LTYPE");
    o.pair(70, 5);
    for t in [
        LineType::Continuous,
        LineType::Dashed,
        LineType::Hidden,
        LineType::Center,
        LineType::Phantom,
    ] {
        let pattern = dash_pattern(t);
        o.pair(0, "LTYPE");
        o.pair(2, line_type_name(t));
        o.pair(70, 0);
        o.pair(3, t.label());
        o.pair(72, 65);
        o.pair(73, pattern.len());
        o.num(40, pattern.iter().sum());
        for (k, v) in pattern.iter().enumerate() {
            o.num(49, if k % 2 == 0 { *v } else { -*v });
        }
    }
    o.pair(0, "ENDTAB");
    // Layers: those declared, then any others the items use.
    let mut layers: BTreeMap<String, (u8, LineType)> = BTreeMap::new();
    for l in &d.layers {
        layers.insert(layer_name(&l.name), (aci(l.color), l.line_type));
    }
    for item in &d.items {
        layers
            .entry(layer_name(&item.layer))
            .or_insert((aci(item.pen.color), LineType::Continuous));
    }
    layers
        .entry("0".into())
        .or_insert((7, LineType::Continuous));
    o.pair(0, "TABLE");
    o.pair(2, "LAYER");
    o.pair(70, layers.len());
    for (name, (color, t)) in &layers {
        o.pair(0, "LAYER");
        o.pair(2, name);
        o.pair(70, 0);
        o.pair(62, *color);
        o.pair(6, line_type_name(*t));
    }
    o.pair(0, "ENDTAB");
    o.pair(0, "ENDSEC");

    o.pair(0, "SECTION");
    o.pair(2, "ENTITIES");
    for item in &d.items {
        let layer = layer_name(&item.layer);
        let (layer_color, layer_type) = layers[&layer];
        let head = |o: &mut Out, kind: &str| {
            o.pair(0, kind);
            o.pair(8, &layer);
            let c = aci(item.pen.color);
            if c != layer_color {
                o.pair(62, c);
            }
            if item.pen.line_type != layer_type {
                o.pair(6, line_type_name(item.pen.line_type));
            }
        };
        match &item.prim {
            Prim::Line(a, b) => {
                head(&mut o, "LINE");
                o.point(10, *a);
                o.point(11, *b);
            }
            Prim::Polyline { points, closed } => {
                head(&mut o, "POLYLINE");
                o.pair(66, 1);
                o.point(10, DVec2::ZERO);
                o.pair(70, u8::from(*closed));
                for p in points {
                    o.pair(0, "VERTEX");
                    o.pair(8, &layer);
                    o.point(10, *p);
                }
                o.pair(0, "SEQEND");
                o.pair(8, &layer);
            }
            Prim::Circle { center, radius } => {
                head(&mut o, "CIRCLE");
                o.point(10, *center);
                o.num(40, *radius);
            }
            Prim::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                head(&mut o, "ARC");
                o.point(10, *center);
                o.num(40, *radius);
                o.num(50, start.to_degrees().rem_euclid(360.0));
                o.num(51, (start + sweep).to_degrees().rem_euclid(360.0));
            }
            Prim::Fill {
                triangles, mask, ..
            } => {
                if *mask {
                    continue;
                }
                for [a, b, c] in triangles {
                    head(&mut o, "SOLID");
                    o.point(10, *a);
                    o.point(11, *b);
                    o.point(12, *c);
                    o.point(13, *c);
                }
            }
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
            } => {
                head(&mut o, "TEXT");
                o.point(10, *at);
                o.num(40, *height);
                o.pair(1, dxf_text(text));
                if angle.abs() > 1e-9 {
                    o.num(50, angle.to_degrees());
                }
                let h = match align {
                    Align::Left => 0,
                    Align::Center => 1,
                    Align::Right => 2,
                };
                if h != 0 {
                    o.pair(72, h);
                    o.point(11, *at);
                }
            }
        }
    }
    o.pair(0, "ENDSEC");
    o.pair(0, "EOF");
    o.0
}

// ---------------------------------------------------------------------------------------------
// Reading

/// A curve read from a DXF file (world millimetres).
#[derive(Clone, Debug, PartialEq)]
pub struct DxfEntity {
    pub layer: String,
    pub shape: Shape,
    pub line_type: Option<LineType>,
    pub color: Option<Rgb>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DxfLayer {
    pub name: String,
    pub color: Option<Rgb>,
    pub line_type: LineType,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfImport {
    pub entities: Vec<DxfEntity>,
    pub layers: Vec<DxfLayer>,
    /// Entity types that were skipped, with counts (text, splines, blocks…).
    pub skipped: BTreeMap<String, usize>,
}

/// Millimetres per drawing unit from `$INSUNITS` (`None` for unitless files).
fn unit_scale(code: i64) -> Option<f64> {
    Some(match code {
        1 => 25.4,
        2 => 304.8,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        8 => 2.54e-5,
        9 => 0.0254,
        10 => 914.4,
        14 => 100.0,
        _ => return None,
    })
}

/// An arc from `a` to `b` with DXF bulge (tan of a quarter of the included angle; positive is
/// counter-clockwise).
fn bulge_arc(a: DVec2, b: DVec2, bulge: f64) -> Shape {
    let chord = b - a;
    let theta = 4.0 * bulge.atan();
    let r = chord.length() / (2.0 * (theta / 2.0).sin().abs());
    // The centre lies off the chord's midpoint, on the left for a positive bulge.
    let h = r * (theta / 2.0).cos();
    let mid = (a + b) * 0.5;
    let n = chord.perp().normalize();
    let center = mid + n * h * theta.signum();
    let (from, to) = if theta > 0.0 { (a, b) } else { (b, a) };
    let start = (from - center).y.atan2((from - center).x);
    let end = (to - center).y.atan2((to - center).x);
    Shape::Arc {
        center,
        radius: r,
        start,
        sweep: (end - start).rem_euclid(TAU),
    }
}

/// Read the curves of an ASCII DXF file. Files that don't say their units (`$INSUNITS`
/// missing or 0) are read in `default_mm_per_unit` (the document's display unit).
pub fn read(text: &str, default_mm_per_unit: f64) -> Result<DxfImport, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if lines.len() < 2 {
        return Err("empty file".into());
    }
    let mut pairs: Vec<(i32, &str)> = Vec::with_capacity(lines.len() / 2);
    for chunk in lines.chunks(2) {
        let [code, value] = chunk else {
            break;
        };
        let code: i32 = code
            .parse()
            .map_err(|_| format!("not an ASCII DXF file (bad group code {code:?})"))?;
        pairs.push((code, value));
    }
    let mut out = DxfImport::default();
    // Units come from the header, which precedes the entities.
    let mut scale = default_mm_per_unit;
    let mut section = "";
    let mut i = 0;
    // Collect one entity's group codes from `i` (just after its `0` pair) to the next `0`.
    let take = |i: &mut usize| -> Vec<(i32, &str)> {
        let start = *i;
        while *i < pairs.len() && pairs[*i].0 != 0 {
            *i += 1;
        }
        pairs[start..*i].to_vec()
    };
    let f = |groups: &[(i32, &str)], code: i32| -> Option<f64> {
        groups
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.parse().ok())
    };
    let s = |groups: &[(i32, &str)], code: i32| -> Option<String> {
        groups
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| (*v).to_owned())
    };
    while i < pairs.len() {
        let (code, value) = pairs[i];
        i += 1;
        if code == 9 && value == "$INSUNITS" && section == "HEADER" {
            if let Some((70, v)) = pairs.get(i).copied() {
                scale = unit_scale(v.parse().unwrap_or(0)).unwrap_or(default_mm_per_unit);
            }
            continue;
        }
        if code != 0 {
            continue;
        }
        match value {
            "SECTION" => {
                section = pairs.get(i).map_or("", |p| p.1);
                i += 1;
            }
            "ENDSEC" => section = "",
            "EOF" => break,
            "LAYER" if section == "TABLES" => {
                let g = take(&mut i);
                if let Some(name) = s(&g, 2) {
                    out.layers.push(DxfLayer {
                        name,
                        color: f(&g, 62).and_then(|c| aci_rgb(c.abs() as i64)),
                        line_type: s(&g, 6).map_or(LineType::Continuous, |t| parse_line_type(&t)),
                    });
                }
            }
            kind if section == "ENTITIES" => {
                let g = take(&mut i);
                let layer = s(&g, 8).unwrap_or_else(|| "0".into());
                let line_type = s(&g, 6)
                    .filter(|t| !t.eq_ignore_ascii_case("BYLAYER"))
                    .map(|t| parse_line_type(&t));
                let color = f(&g, 62).and_then(|c| aci_rgb(c as i64));
                let p = |x: i32| -> Option<DVec2> {
                    Some(DVec2::new(f(&g, x)?, f(&g, x + 10)?) * scale)
                };
                let mut push = |shape: Shape| {
                    out.entities.push(DxfEntity {
                        layer: layer.clone(),
                        shape,
                        line_type,
                        color,
                    });
                };
                match kind {
                    "LINE" => {
                        if let (Some(a), Some(b)) = (p(10), p(11)) {
                            push(Shape::Line { a, b });
                        }
                    }
                    "POINT" => {
                        if let Some(a) = p(10) {
                            push(Shape::Point(a));
                        }
                    }
                    "CIRCLE" => {
                        if let (Some(c), Some(r)) = (p(10), f(&g, 40)) {
                            push(Shape::Circle {
                                center: c,
                                radius: r * scale,
                            });
                        }
                    }
                    "ARC" => {
                        if let (Some(c), Some(r), Some(a0), Some(a1)) =
                            (p(10), f(&g, 40), f(&g, 50), f(&g, 51))
                        {
                            let start = a0.to_radians();
                            push(Shape::Arc {
                                center: c,
                                radius: r * scale,
                                start,
                                sweep: (a1.to_radians() - start).rem_euclid(TAU),
                            });
                        }
                    }
                    "LWPOLYLINE" => {
                        let closed = f(&g, 70).is_some_and(|v| (v as i64) & 1 == 1);
                        // Vertices: each 10/20, optionally followed by a 42 bulge.
                        let mut verts: Vec<(DVec2, f64)> = Vec::new();
                        let mut x = None;
                        for (code, v) in &g {
                            let v: f64 = v.parse().unwrap_or(0.0);
                            match code {
                                10 => x = Some(v),
                                20 => {
                                    if let Some(x) = x.take() {
                                        verts.push((DVec2::new(x, v) * scale, 0.0));
                                    }
                                }
                                42 => {
                                    if let Some(last) = verts.last_mut() {
                                        last.1 = v;
                                    }
                                }
                                _ => {}
                            }
                        }
                        polyline(&verts, closed, &mut push);
                    }
                    "POLYLINE" => {
                        let closed = f(&g, 70).is_some_and(|v| (v as i64) & 1 == 1);
                        let mut verts = Vec::new();
                        while i < pairs.len() && pairs[i] == (0, "VERTEX") {
                            i += 1;
                            let v = take(&mut i);
                            if let (Some(x), Some(y)) = (f(&v, 10), f(&v, 20)) {
                                verts.push((DVec2::new(x, y) * scale, f(&v, 42).unwrap_or(0.0)));
                            }
                        }
                        if i < pairs.len() && pairs[i] == (0, "SEQEND") {
                            i += 1;
                            take(&mut i);
                        }
                        polyline(&verts, closed, &mut push);
                    }
                    other => *out.skipped.entry(other.to_owned()).or_insert(0) += 1,
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn polyline(verts: &[(DVec2, f64)], closed: bool, push: &mut impl FnMut(Shape)) {
    let n = verts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    for k in 0..segments {
        let (a, bulge) = verts[k];
        let b = verts[(k + 1) % n].0;
        if a.distance(b) <= 1e-12 {
            continue;
        }
        push(if bulge.abs() > 1e-12 {
            bulge_arc(a, b, bulge)
        } else {
            Shape::Line { a, b }
        });
    }
}

/// What an import added.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub curves: usize,
    pub layers: usize,
    pub skipped: BTreeMap<String, usize>,
}

/// Add an import's curves to a document (inside the caller's transaction), on layers of the
/// same names. Ends that coincide (within 1e-6 of the drawing's size) share one point, so the
/// imported outlines stay connected.
pub fn add_to_document(doc: &mut Document, import: &DxfImport) -> ImportReport {
    let mut report = ImportReport {
        skipped: import.skipped.clone(),
        ..Default::default()
    };
    let mut extent = drift_core::Bounds::EMPTY;
    for e in &import.entities {
        let b = e.shape.bounds();
        extent.include(b.min);
        extent.include(b.max);
    }
    let tol = if extent.is_empty() {
        1e-9
    } else {
        (extent.max - extent.min).max_element().max(1.0) * 1e-6
    };
    // Layers by name ("0" is the default layer).
    let mut layer_ids: BTreeMap<String, LayerId> = doc
        .layers()
        .map(|(id, l)| (l.name.to_ascii_uppercase(), id))
        .collect();
    let mut layer_of = |doc: &mut Document, name: &str, report: &mut ImportReport| -> LayerId {
        let key = name.to_ascii_uppercase();
        if let Some(id) = layer_ids.get(&key) {
            return *id;
        }
        let info = import
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name));
        let mut layer = Layer::new(name);
        layer.color = info.and_then(|l| l.color);
        layer.line_type = info.map_or(LineType::Continuous, |l| l.line_type);
        layer.weight = LineWeight::default();
        let id = doc.add_layer(layer);
        layer_ids.insert(key, id);
        report.layers += 1;
        id
    };
    // Shared points, found through a grid of cells `tol` wide.
    let mut points: BTreeMap<(i64, i64), Vec<(DVec2, EntityId)>> = BTreeMap::new();
    let mut point = |doc: &mut Document, p: DVec2| -> EntityId {
        let cell = ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = points.get(&(cell.0 + dx, cell.1 + dy))
                    && let Some((_, id)) = list.iter().find(|(q, _)| q.distance(p) <= tol)
                {
                    return *id;
                }
            }
        }
        let id = doc.add_point(p);
        points.entry(cell).or_default().push((p, id));
        id
    };
    for e in &import.entities {
        let layer = layer_of(doc, &e.layer, &mut report);
        let id = match e.shape {
            Shape::Line { a, b } => {
                let (pa, pb) = (point(doc, a), point(doc, b));
                if pa == pb {
                    continue;
                }
                doc.add_line(pa, pb).ok()
            }
            Shape::Circle { center, radius } => {
                let c = point(doc, center);
                doc.add_circle(c, radius).ok()
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let c = point(doc, center);
                let s = point(doc, center + DVec2::from_angle(start) * radius);
                let e2 = point(doc, center + DVec2::from_angle(start + sweep) * radius);
                if s == e2 {
                    doc.add_circle(c, radius).ok()
                } else {
                    doc.add_arc(c, s, e2).ok()
                }
            }
            Shape::Point(p) => Some(point(doc, p)),
        };
        let Some(id) = id else {
            continue;
        };
        let _ = doc.set_entity_layer(id, layer);
        if e.color.is_some() || e.line_type.is_some() {
            let mut look = doc.get(id).map(|x| x.appearance).unwrap_or_default();
            look.color = e.color.or(look.color);
            look.line_type = e.line_type.or(look.line_type);
            let _ = doc.set_appearance(id, look);
        }
        report.curves += 1;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_draft::page::{LayerInfo, Pen};

    #[test]
    fn round_trips_curves_layers_and_line_types() {
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        d.layer(LayerInfo {
            name: "Centre lines".into(),
            color: Rgb([255, 0, 0]),
            width: 0.25,
            line_type: LineType::Center,
        });
        d.line("Outline", Pen::new(0.5), DVec2::ZERO, DVec2::new(40.0, 0.0));
        d.line(
            "Outline",
            Pen::new(0.5),
            DVec2::new(40.0, 0.0),
            DVec2::new(40.0, 30.0),
        );
        d.push(
            "Outline",
            Pen::new(0.5),
            Prim::Arc {
                center: DVec2::new(20.0, 30.0),
                radius: 20.0,
                start: 0.0,
                sweep: std::f64::consts::PI,
            },
        );
        d.push(
            "Centre lines",
            Pen::new(0.25)
                .with_type(LineType::Center)
                .with_color(Rgb([255, 0, 0])),
            Prim::Circle {
                center: DVec2::new(20.0, 10.0),
                radius: 5.0,
            },
        );
        d.text(
            "Notes",
            Pen::new(0.25),
            DVec2::new(5.0, 5.0),
            3.5,
            Align::Center,
            "Ø10 ±0.1",
        );
        let text = write(&d, DxfUnits::MM);
        assert!(text.contains("AC1009"));
        assert!(text.contains("CENTRE_LINES"));
        assert!(text.contains("%%c10 %%p0.1"));
        let back = read(&text, 1.0).unwrap();
        assert_eq!(back.entities.len(), 4, "{:?}", back.skipped);
        assert_eq!(back.skipped.get("TEXT"), Some(&1));
        let centre = back
            .layers
            .iter()
            .find(|l| l.name == "CENTRE_LINES")
            .unwrap();
        assert_eq!(centre.line_type, LineType::Center);
        assert_eq!(centre.color, Some(Rgb([255, 0, 0])));
        match back.entities[2].shape {
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                assert!((center - DVec2::new(20.0, 30.0)).length() < 1e-9);
                assert!((radius - 20.0).abs() < 1e-9);
                assert!(start.abs() < 1e-9 && (sweep - std::f64::consts::PI).abs() < 1e-9);
            }
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_bulged_polylines_and_units_into_a_document() {
        // A 2 × 1 inch slot: two lines and two half-circle bulges, in inches.
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n\
                   0\nSECTION\n2\nENTITIES\n\
                   0\nLWPOLYLINE\n8\nSLOT\n90\n4\n70\n1\n\
                   10\n0\n20\n0\n10\n2\n20\n0\n42\n1\n10\n2\n20\n1\n10\n0\n20\n1\n42\n1\n\
                   0\nENDSEC\n0\nEOF\n";
        let import = read(dxf, 1.0).unwrap();
        assert_eq!(import.entities.len(), 4);
        match import.entities[1].shape {
            Shape::Arc {
                center,
                radius,
                sweep,
                ..
            } => {
                assert!((center - DVec2::new(50.8, 12.7)).length() < 1e-9);
                assert!((radius - 12.7).abs() < 1e-9);
                assert!((sweep - std::f64::consts::PI).abs() < 1e-9);
            }
            ref other => panic!("{other:?}"),
        }
        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &import);
        doc.commit("Import");
        assert_eq!(report.curves, 4);
        assert_eq!(report.layers, 1);
        // Connected: 4 ends shared by 4 curves, plus 2 arc centres.
        let points = doc.iter().filter(|(_, e)| e.kind.is_point()).count();
        assert_eq!(points, 6);
        assert!(doc.layers().any(|(_, l)| l.name == "SLOT"));
        assert!(read("not a dxf\nfile\n", 1.0).is_err());
    }

    #[test]
    fn writes_in_the_documents_units() {
        // A 4 inch line in an architectural document is 4 units in the file, in inches.
        let mut d = Drawing::new(DVec2::new(200.0, 100.0));
        d.line("0", Pen::new(0.25), DVec2::ZERO, DVec2::new(101.6, 0.0));
        d.line_type_scale = 2.0;
        let units = DxfUnits::of(&Units::architectural());
        let text = write(&d, units);
        assert!(text.contains("$INSUNITS\n 70\n1\n"), "inches");
        assert!(text.contains("$LUNITS\n 70\n4\n"), "architectural");
        assert!(text.contains("$MEASUREMENT\n 70\n0\n"), "imperial");
        assert!(text.contains(" 11\n4\n"), "the line ends at x = 4");
        // Read back: inches → millimetres again.
        let back = read(&text, 1.0).unwrap();
        assert_eq!(
            back.entities[0].shape,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(101.6, 0.0)
            }
        );
        // A unitless file is read in the document's unit.
        let unitless = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n4\n21\n0\n0\nENDSEC\n0\nEOF\n";
        let inches = read(unitless, 25.4).unwrap();
        assert_eq!(
            inches.entities[0].shape,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(101.6, 0.0)
            }
        );
        assert_eq!(DxfUnits::of(&Units::default()), DxfUnits::MM);
    }
}
