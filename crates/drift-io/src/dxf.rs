//! DXF: writing a [`Drawing`] as R12 (AC1009) ASCII DXF, which every CAD program reads.
//! Reading any ASCII DXF is in `dxf_import` (re-exported here).
//!
//! Output keeps layers (colour and line type) and line types; R12 has no line weights, so
//! they are dropped. Fills become SOLID triangles. Ellipses and Bézier curves are written
//! exactly as ELLIPSE and SPLINE entities, which are newer than R12: most readers take
//! them in an AC1009 file, but a strict R12 reader may skip them. Text uses DXF's `%%c` / `%%d` / `%%p`
//! codes for Ø, ° and ±.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::fmt::Write as _;

use drift_core::{DVec2, LengthFormat, LineType, Rgb, Shape, Units};
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

/// The colour of an AutoCAD Color Index entry (1 to 255). 7 is "black or white by
/// background", which is Drift's default colour: `None`.
pub(crate) fn aci_rgb(index: i64) -> Option<Rgb> {
    if !(1..=255).contains(&index) || index == 7 {
        return None;
    }
    if let Some((_, c)) = ACI.iter().find(|(i, _)| i64::from(*i) == index) {
        return Some(Rgb(*c));
    }
    if index >= 250 {
        let v = [51, 80, 105, 130, 190, 255][(index - 250) as usize];
        return Some(Rgb([v, v, v]));
    }
    // 10 to 249: twenty-four hues, each in five shades, each shade full and half strength.
    let hue = ((index - 10) / 10) as f64 * 15.0;
    let shade = (index % 10) / 2;
    let value = [1.0, 0.8, 0.6, 0.5, 0.3][shade as usize];
    let saturation = if index % 2 == 0 { 1.0 } else { 0.5 };
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as i64 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    let byte = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Rgb([byte(r), byte(g), byte(b)]))
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

pub(crate) fn parse_line_type(name: &str) -> LineType {
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
            // $LUNITS 5: fractional.
            LengthFormat::FractionalInch => (1, 5, 0),
        };
        DxfUnits {
            mm_per_unit: units.length.mm_per_unit(),
            insunits,
            lunits,
            luprec: if lunits == 4 || lunits == 5 {
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
    for item in d
        .items
        .iter()
        .chain(d.blocks.iter().flat_map(|b| b.items.iter()))
    {
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

    // Block definitions (their items on layer 0 take the copy's layer).
    if !d.blocks.is_empty() {
        o.pair(0, "SECTION");
        o.pair(2, "BLOCKS");
        for b in &d.blocks {
            let name = layer_name(&b.name);
            o.pair(0, "BLOCK");
            o.pair(8, "0");
            o.pair(2, &name);
            o.pair(70, 0);
            o.point(10, DVec2::ZERO);
            o.pair(3, &name);
            for item in &b.items {
                write_item(&mut o, item, &layers);
            }
            o.pair(0, "ENDBLK");
            o.pair(8, "0");
        }
        o.pair(0, "ENDSEC");
    }
    o.pair(0, "SECTION");
    o.pair(2, "ENTITIES");
    for item in &d.items {
        write_item(&mut o, item, &layers);
    }
    o.pair(0, "ENDSEC");
    o.pair(0, "EOF");
    o.0
}

/// One entity (an `Insert` becomes an INSERT of its block).
fn write_item(
    o: &mut Out,
    item: &drift_draft::page::Item,
    layers: &BTreeMap<String, (u8, LineType)>,
) {
    let layer = layer_name(&item.layer);
    let (layer_color, layer_type) = layers
        .get(&layer)
        .copied()
        .unwrap_or((7, LineType::Continuous));
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
            head(o, "LINE");
            o.point(10, *a);
            o.point(11, *b);
        }
        Prim::Polyline { points, closed } => {
            head(o, "POLYLINE");
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
            head(o, "CIRCLE");
            o.point(10, *center);
            o.num(40, *radius);
        }
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            head(o, "ARC");
            o.point(10, *center);
            o.num(40, *radius);
            o.num(50, start.to_degrees().rem_euclid(360.0));
            o.num(51, (start + sweep).to_degrees().rem_euclid(360.0));
        }
        Prim::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            // DXF wants the longer axis first (ratio ≤ 1): swap them if needed, which shifts
            // parametric angles by a quarter turn.
            let (axis, ratio, shift) = if *minor <= major.length() {
                (*major, minor / major.length(), 0.0)
            } else {
                (
                    major.perp().normalize() * *minor,
                    major.length() / minor,
                    std::f64::consts::FRAC_PI_2,
                )
            };
            head(o, "ELLIPSE");
            o.point(10, *center);
            o.point(11, axis);
            o.num(40, ratio);
            let t0 = start - shift;
            o.num(41, t0.rem_euclid(TAU));
            let t1 = if *sweep >= TAU - 1e-12 {
                t0.rem_euclid(TAU) + TAU
            } else {
                (t0 + sweep).rem_euclid(TAU)
            };
            o.num(42, t1);
        }
        Prim::Bezier { p } => {
            // A degree-3 spline with a clamped knot vector is exactly this Bézier.
            head(o, "SPLINE");
            o.pair(70, 8);
            o.pair(71, 3);
            o.pair(72, 8);
            o.pair(73, 4);
            o.pair(74, 0);
            for k in [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
                o.num(40, k);
            }
            for q in p {
                o.point(10, *q);
            }
        }
        Prim::Fill {
            triangles, mask, ..
        } => {
            if *mask {
                return;
            }
            for [a, b, c] in triangles {
                head(o, "SOLID");
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
            ..
        } => {
            head(o, "TEXT");
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
        Prim::Insert {
            block,
            at,
            angle,
            scale,
            mirror,
        } => {
            head(o, "INSERT");
            o.pair(2, layer_name(block));
            o.point(10, *at);
            // A mirrored copy is a negative X scale (mirroring before rotating).
            o.num(41, if *mirror { -*scale } else { *scale });
            o.num(42, *scale);
            o.num(43, *scale);
            if angle.abs() > 1e-12 {
                o.num(50, angle.rem_euclid(360.0));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reading (see `dxf_import`)

pub use crate::dxf_import::{
    DxfBlock, DxfEntity, DxfHatch, DxfImport, DxfInsert, DxfLayer, DxfLayout, DxfText, DxfViewport,
    ImportReport, add_to_document, decode, read,
};

/// The curves of a DXF import as underlay curves (in mm), with copies of blocks expanded.
/// Points are left out.
pub fn underlay_prims(import: &DxfImport) -> Vec<drift_core::RefPrim> {
    use drift_core::RefPrim;
    let prim = |s: &Shape| -> Option<RefPrim> {
        Some(match *s {
            Shape::Point(_) => return None,
            Shape::Line { a, b } => RefPrim::Line(a, b),
            Shape::Circle { center, radius } => RefPrim::Circle { center, radius },
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => RefPrim::Arc {
                center,
                radius,
                start,
                sweep,
            },
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => RefPrim::Polyline(drift_core::shape::flatten_ellipse(
                center, major, minor, start, sweep, 48,
            )),
            Shape::Bezier { p } => RefPrim::Polyline(drift_core::shape::flatten_bezier(&p, 24)),
        })
    };
    let mut out: Vec<RefPrim> = import
        .entities
        .iter()
        .filter_map(|e| prim(&e.shape))
        .collect();
    for insert in &import.inserts {
        let Some(block) = import.blocks.iter().find(|b| b.name == insert.block) else {
            continue;
        };
        let mut copy = drift_core::Instance::new(drift_core::BlockId::default(), insert.at);
        copy.angle = insert.angle;
        copy.scale = insert.scale;
        copy.mirror = insert.mirror;
        out.extend(
            block
                .entities
                .iter()
                .filter_map(|e| prim(&copy.apply_shape(&e.shape))),
        );
    }
    out
}

/// Millimetres per drawing unit from `$INSUNITS` (`None` for unitless files).
pub(crate) fn unit_scale(code: i64) -> Option<f64> {
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
pub(crate) fn bulge_arc(a: DVec2, b: DVec2, bulge: f64) -> Shape {
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

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::Document;
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
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
        // The centred text comes back as text, its codes decoded, still centred on its point.
        assert_eq!(back.texts.len(), 1);
        assert_eq!(back.texts[0].text, "Ø10 ±0.1");
        assert!((back.texts[0].height - 3.5).abs() < 1e-9);
        assert_eq!(back.texts[0].align, drift_core::TextAlign::Center);
        assert!(back.texts[0].at.distance(DVec2::new(5.0, 5.0)) < 1e-6);
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

    #[test]
    fn blocks_round_trip_as_block_and_insert() {
        use drift_draft::page::{BlockDef, Item};
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        d.blocks.push(BlockDef {
            name: "Chair".into(),
            items: vec![
                Item {
                    layer: "0".into(),
                    pen: Pen::new(0.25),
                    prim: Prim::Line(DVec2::new(-200.0, 0.0), DVec2::new(200.0, 0.0)),
                    clip: None,
                },
                Item {
                    layer: "0".into(),
                    pen: Pen::new(0.25),
                    prim: Prim::Arc {
                        center: DVec2::ZERO,
                        radius: 200.0,
                        start: 0.0,
                        sweep: std::f64::consts::FRAC_PI_2,
                    },
                    clip: None,
                },
            ],
        });
        d.push(
            "Furniture",
            Pen::new(0.25),
            Prim::Insert {
                block: "Chair".into(),
                at: DVec2::new(1000.0, 500.0),
                angle: 90.0,
                scale: 1.0,
                mirror: true,
            },
        );
        let text = write(&d, DxfUnits::MM);
        assert!(text.contains("BLOCKS") && text.contains("INSERT"));
        assert!(text.contains(" 41\n-1\n"), "mirrored as a negative X scale");
        // Extents include the copy.
        assert!(d.bounds().max.x >= 1000.0);
        let back = read(&text, 1.0).unwrap();
        assert_eq!(back.blocks.len(), 1);
        assert_eq!(back.blocks[0].entities.len(), 2);
        assert_eq!(back.inserts.len(), 1);
        let i = &back.inserts[0];
        assert!(i.mirror && (i.angle - 90.0).abs() < 1e-9 && i.at == DVec2::new(1000.0, 500.0));
        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &back);
        doc.commit("Import");
        assert_eq!((report.blocks, report.copies), (1, 1));
        let (id, copy) = doc.instances().next().unwrap();
        assert!(copy.mirror);
        // The copy's line lands where the exploded drawing puts it.
        let world = doc.instance_items(id);
        let exploded = d.exploded();
        let Prim::Line(a, b) = exploded.items[0].prim else {
            panic!()
        };
        assert!(world.iter().any(|it| match it.shape {
            Shape::Line { a: p, b: q } => {
                (p.distance(a) < 1e-9 && q.distance(b) < 1e-9)
                    || (p.distance(b) < 1e-9 && q.distance(a) < 1e-9)
            }
            _ => false,
        }));
        assert!(doc.layers().any(|(_, l)| l.name == "FURNITURE"));
    }

    #[test]
    fn ellipses_round_trip_as_ellipse_entities() {
        let mut d = Drawing::new(DVec2::new(200.0, 100.0));
        let cases = [
            // A whole one, turned.
            (DVec2::new(20.0, 10.0), 4.0, 0.0, TAU),
            // Part of one whose second axis is the longer: written with the axes swapped.
            (DVec2::new(0.0, 5.0), 12.0, 0.5, 2.0),
        ];
        for (major, minor, start, sweep) in cases {
            d.push(
                "Ellipses",
                Pen::new(0.25),
                Prim::Ellipse {
                    center: DVec2::new(50.0, 40.0),
                    major,
                    minor,
                    start,
                    sweep,
                },
            );
        }
        let text = write(&d, DxfUnits::MM);
        assert_eq!(
            text.matches(
                "
ELLIPSE
"
            )
            .count(),
            2,
            "{text}"
        );
        let back = read(&text, 1.0).unwrap();
        assert_eq!(back.entities.len(), 2);
        for ((major, minor, start, sweep), e) in cases.iter().zip(&back.entities) {
            let Shape::Ellipse {
                major: m2,
                minor: n2,
                start: s2,
                sweep: w2,
                ..
            } = e.shape
            else {
                panic!("{:?}", e.shape)
            };
            // The same curve: compare points along it.
            for k in 0..=8 {
                let f = k as f64 / 8.0;
                let a = drift_core::shape::ellipse_point(
                    DVec2::ZERO,
                    *major,
                    *minor,
                    start + sweep * f,
                );
                let b = drift_core::shape::ellipse_point(DVec2::ZERO, m2, n2, s2 + w2 * f);
                // DXF numbers carry six decimals.
                assert!(a.distance(b) < 1e-4, "{a} vs {b}");
            }
        }
        // Into a document: an ellipse and an elliptical arc.
        let mut doc = Document::new();
        doc.begin();
        add_to_document(&mut doc, &back);
        doc.commit("Import");
        let kinds: Vec<_> = doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(_, e)| e.kind.type_name())
            .collect();
        assert_eq!(kinds, ["Ellipse", "Elliptical arc"]);
    }
}
