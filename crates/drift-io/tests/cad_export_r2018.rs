//! DWG and DXF R2018 export (through acadrust) round trips: what Drift writes comes back as
//! it was, through Drift's own DXF and DWG import:
//! - true curves: ellipses, arcs, circles, and Bézier curves as one exact spline per run;
//! - layers by their own names, in true colour, with their line types and weights;
//! - blocks and their copies, a mirrored one still mirrored;
//! - solid fills with holes, and hatches with their pattern, scale and angle;
//! - text in any script, as written (no `\U+` escapes);
//! - the drawing's units.
//!
//! The written files also open in AutoCAD's own engine: see `scripts/cad-check.ps1`.
#![cfg(feature = "dwg")]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use drift_core::{DVec2, Document, FillKind, LineType, Rgb, Shape, TextAlign, TextFont, Units};
use drift_draft::page::{Align, BlockDef, Drawing, HatchPrim, Item, LayerInfo, Pen, Prim};
use drift_draft::{Placement, find};
use drift_geom::regions::regions;
use drift_io::cad::{CadWriter, write_dwg, write_dxf};
use drift_io::dxf::{DxfImport, DxfUnits, add_to_document, read};

const TOL: f64 = 1e-6;

fn near(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < TOL
}

/// A drawing with one of everything the export maps.
fn drawing() -> Drawing {
    let mut d = Drawing::new(DVec2::new(1000.0, 1000.0));
    d.layer(LayerInfo {
        name: "Walls new".into(),
        color: Rgb([200, 30, 40]),
        width: 0.5,
        line_type: LineType::Continuous,
    });
    d.layer(LayerInfo {
        name: "Centre lines".into(),
        color: Rgb([0, 0, 0]),
        width: 0.18,
        line_type: LineType::Center,
    });
    let wall = Pen::new(0.5).with_color(Rgb([200, 30, 40]));
    d.line("Walls new", wall, DVec2::ZERO, DVec2::new(400.0, 0.0));
    d.push(
        "Walls new",
        wall,
        Prim::Arc {
            center: DVec2::new(400.0, 100.0),
            radius: 100.0,
            start: -FRAC_PI_2,
            sweep: PI,
        },
    );
    // Its own colour and weight on the layer.
    d.push(
        "Walls new",
        Pen::new(0.25).with_color(Rgb([10, 120, 250])),
        Prim::Circle {
            center: DVec2::new(100.0, 100.0),
            radius: 25.0,
        },
    );
    d.push(
        "Centre lines",
        Pen::new(0.18).with_type(LineType::Center),
        Prim::Ellipse {
            center: DVec2::new(200.0, 300.0),
            major: DVec2::new(80.0, 0.0),
            minor: 40.0,
            start: 0.0,
            sweep: TAU,
        },
    );
    // Taller than wide: CAD's major axis is the other one.
    d.push(
        "Centre lines",
        Pen::new(0.18).with_type(LineType::Center),
        Prim::Ellipse {
            center: DVec2::new(500.0, 300.0),
            major: DVec2::new(30.0, 0.0),
            minor: 60.0,
            start: 0.0,
            sweep: PI,
        },
    );
    // Two joined Bézier curves: one spline.
    let pen = Pen::new(0.25);
    d.push(
        "Curves",
        pen,
        Prim::Bezier {
            p: [
                DVec2::new(0.0, 500.0),
                DVec2::new(50.0, 600.0),
                DVec2::new(150.0, 600.0),
                DVec2::new(200.0, 500.0),
            ],
        },
    );
    d.push(
        "Curves",
        pen,
        Prim::Bezier {
            p: [
                DVec2::new(200.0, 500.0),
                DVec2::new(250.0, 400.0),
                DVec2::new(350.0, 400.0),
                DVec2::new(400.0, 500.0),
            ],
        },
    );
    d.text(
        "Notes",
        Pen::new(0.25),
        DVec2::new(10.0, 700.0),
        5.0,
        Align::Center,
        "Ø10 ±0.1 Größe 寸法 50%",
    );
    // Monospaced text (Drift's own Mono).
    d.push(
        "Notes",
        Pen::new(0.25),
        Prim::Text {
            at: DVec2::new(10.0, 650.0),
            height: 3.5,
            angle: 0.0,
            align: Align::Left,
            text: "PART-0042".into(),
            style: drift_draft::page::TextStyle::REGULAR,
            font: Some(TextFont::Mono),
        },
    );
    // A solid fill, a square with a round hole, half transparent.
    let square = [
        Shape::Line {
            a: DVec2::new(600.0, 0.0),
            b: DVec2::new(800.0, 0.0),
        },
        Shape::Line {
            a: DVec2::new(800.0, 0.0),
            b: DVec2::new(800.0, 200.0),
        },
        Shape::Line {
            a: DVec2::new(800.0, 200.0),
            b: DVec2::new(600.0, 200.0),
        },
        Shape::Line {
            a: DVec2::new(600.0, 200.0),
            b: DVec2::new(600.0, 0.0),
        },
        Shape::Circle {
            center: DVec2::new(700.0, 100.0),
            radius: 40.0,
        },
    ];
    let holed = regions(&square)
        .into_iter()
        .find(|r| !r.holes.is_empty())
        .expect("a square with a hole");
    d.push(
        "Fills",
        Pen::new(0.0).with_color(Rgb([90, 160, 90])),
        Prim::Hatch(Box::new(HatchPrim {
            region: holed,
            pattern: None,
            opacity: 0.5,
        })),
    );
    // ANSI31 at scale 2, turned 15°, over a rectangle.
    let rect =
        drift_geom::regions::Region::rectangle(DVec2::new(0.0, 800.0), DVec2::new(300.0, 900.0));
    d.push(
        "Fills",
        Pen::new(0.13),
        Prim::Hatch(Box::new(HatchPrim {
            region: rect,
            pattern: Some((
                find("ANSI31").expect("ANSI31").clone(),
                Placement {
                    scale: 2.0,
                    angle: 15.0,
                    origin: DVec2::new(0.0, 800.0),
                },
            )),
            opacity: 1.0,
        })),
    );
    // A block, copied once mirrored and turned.
    d.blocks.push(BlockDef {
        name: "Door 900".into(),
        items: vec![
            Item {
                layer: "0".into(),
                pen: Pen::new(0.25),
                prim: Prim::Line(DVec2::ZERO, DVec2::new(900.0, 0.0)),
                clip: None,
            },
            Item {
                layer: "0".into(),
                // Its own colour, which the copy's layer doesn't change.
                pen: Pen::new(0.25).with_color(Rgb([0, 128, 0])),
                prim: Prim::Arc {
                    center: DVec2::ZERO,
                    radius: 900.0,
                    start: 0.0,
                    sweep: FRAC_PI_2,
                },
                clip: None,
            },
        ],
    });
    d.push(
        "Doors",
        Pen::new(0.13),
        Prim::Insert {
            block: "Door 900".into(),
            at: DVec2::new(2000.0, 500.0),
            angle: 90.0,
            scale: 1.0,
            mirror: true,
        },
    );
    d
}

/// Keep a copy of what was written where `DRIFT_CAD_OUT` says, to open in AutoCAD
/// (`scripts/cad-check.ps1`).
fn keep(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("DRIFT_CAD_OUT") {
        let _ = std::fs::write(std::path::Path::new(&dir).join(name), bytes);
    }
}

/// The checks every round trip passes (the import is in millimetres, whatever the file's
/// units).
fn check(back: &DxfImport) {
    let shapes: Vec<&Shape> = back.entities.iter().map(|e| &e.shape).collect();
    let end = DVec2::new(400.0, 0.0);
    assert!(
        shapes.iter().any(|s| matches!(s, Shape::Line { a, b }
            if (near(*a, DVec2::ZERO) && near(*b, end)) || (near(*b, DVec2::ZERO) && near(*a, end)))),
        "the line: {shapes:?}"
    );
    // Ellipses come back as ellipses, the tall one too.
    let ellipses: Vec<_> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Ellipse {
                center,
                major,
                minor,
                sweep,
                ..
            } => Some((*center, *major, *minor, *sweep)),
            _ => None,
        })
        .collect();
    assert_eq!(ellipses.len(), 2, "{shapes:?}");
    let tall = ellipses
        .iter()
        .find(|e| near(e.0, DVec2::new(500.0, 300.0)))
        .expect("the tall ellipse");
    assert!((tall.1.length().max(tall.2) - 60.0).abs() < TOL, "{tall:?}");
    assert!((tall.3 - PI).abs() < 1e-9, "half of it: {tall:?}");
    // The two Bézier curves, exactly.
    let beziers: Vec<_> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Bezier { p } => Some(*p),
            _ => None,
        })
        .collect();
    assert_eq!(beziers.len(), 2, "{shapes:?}");
    assert!(
        near(beziers[0][1], DVec2::new(50.0, 600.0))
            && near(beziers[1][3], DVec2::new(400.0, 500.0))
    );
    // Layers by their own names, true colour, line type, weight.
    let walls = back
        .layers
        .iter()
        .find(|l| l.name == "Walls new")
        .unwrap_or_else(|| panic!("{:?}", back.layers));
    assert_eq!(walls.color, Some(Rgb([200, 30, 40])));
    assert_eq!(walls.weight.map(|w| w.mm()), Some(0.5));
    let centre = back
        .layers
        .iter()
        .find(|l| l.name == "Centre lines")
        .unwrap();
    assert_eq!(centre.line_type, LineType::Center);
    // The circle's own colour.
    let circle = back
        .entities
        .iter()
        .find(|e| matches!(e.shape, Shape::Circle { .. }))
        .unwrap();
    assert_eq!(circle.color, Some(Rgb([10, 120, 250])));
    // Text as written.
    assert!(
        back.texts
            .iter()
            .any(|t| t.text == "Ø10 ±0.1 Größe 寸法 50%"
                && t.align == TextAlign::Center
                && near(t.at, DVec2::new(10.0, 700.0))
                && (t.height - 5.0).abs() < TOL),
        "{:?}",
        back.texts
    );
    // In the font it was written in: Mono stays Mono, the rest is the sans.
    for t in &back.texts {
        let want = if t.text == "PART-0042" {
            TextFont::Mono
        } else {
            TextFont::Sans
        };
        assert_eq!(t.font, want, "{}", t.text);
    }
    assert!(back.texts.iter().any(|t| t.text == "PART-0042"));
    // The block and its mirrored copy.
    let door = back
        .blocks
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case("Door 900"))
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                back.blocks.iter().map(|b| &b.name).collect::<Vec<_>>()
            )
        });
    assert_eq!(door.entities.len(), 2);
    assert!(
        door.entities
            .iter()
            .any(|e| matches!(e.shape, Shape::Arc { .. }) && e.color == Some(Rgb([0, 128, 0]))),
        "{:?}",
        door.entities
    );
    let copy = back.inserts.iter().find(|i| i.block == door.name).unwrap();
    assert!(
        copy.mirror && (copy.angle - 90.0).abs() < 1e-9 && near(copy.at, DVec2::new(2000.0, 500.0))
    );
    // The solid fill with its hole and opacity; the hatch with its pattern.
    let solid = back
        .hatches
        .iter()
        .find(|h| h.solid)
        .expect("the solid fill");
    assert_eq!(solid.loops.len(), 2, "outer and hole");
    // (By layer: the layer took the colour of the first thing on it.)
    let fills = back.layers.iter().find(|l| l.name == "Fills").unwrap();
    assert_eq!(solid.color.or(fills.color), Some(Rgb([90, 160, 90])));
    assert!(
        (solid.opacity.unwrap() - 0.5).abs() < 0.01,
        "{:?}",
        solid.opacity
    );
    let hatch = back.hatches.iter().find(|h| !h.solid).expect("the hatch");
    assert_eq!(hatch.pattern, "ANSI31");
    assert!((hatch.angle - 15.0).abs() < 1e-6, "{}", hatch.angle);
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
}

/// Into a document: the fills and hatches as Drift's, the copy as a copy.
fn into_document(back: &DxfImport) {
    let mut doc = Document::new();
    doc.begin();
    let report = add_to_document(&mut doc, back);
    doc.commit("Import");
    assert_eq!((report.blocks, report.copies), (1, 1));
    let kinds: Vec<_> = doc.fills().map(|(_, f)| f.kind.clone()).collect();
    assert!(
        kinds.iter().any(|k| matches!(k, FillKind::Solid)),
        "{kinds:?}"
    );
    let hatch = kinds
        .iter()
        .find_map(|k| match k {
            FillKind::Hatch(h) => Some(h.clone()),
            _ => None,
        })
        .expect("a hatch");
    assert_eq!(hatch.pattern, "ANSI31");
    assert!((hatch.angle - 15.0).abs() < 1e-6, "{hatch:?}");
    assert!((hatch.scale - 2.0).abs() < 1e-6, "{hatch:?}");
    assert!(doc.layers().any(|(_, l)| l.name == "Walls new"));
}

#[test]
fn dxf_r2018_round_trips() {
    let bytes = write_dxf(&drawing(), DxfUnits::MM).unwrap();
    keep("r2018.dxf", &bytes);
    let text = String::from_utf8(bytes).expect("R2018 DXF is UTF-8");
    assert!(text.contains("AC1032"));
    assert!(!text.contains("\\U+"), "no escapes in R2018");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    for kind in ["ELLIPSE", "SPLINE", "HATCH", "INSERT"] {
        assert!(lines.contains(&kind), "{kind}");
    }
    let back = read(&text, 1.0).unwrap();
    check(&back);
    into_document(&back);
}

#[test]
fn dwg_r2018_round_trips() {
    let bytes = write_dwg(&drawing(), DxfUnits::MM).unwrap();
    keep("r2018.dwg", &bytes);
    assert!(bytes.starts_with(b"AC1032"));
    let back = drift_io::dwg_import::read(&bytes, 1.0).unwrap();
    check(&back);
    into_document(&back);
}

#[test]
fn inches_round_trip_at_full_size() {
    let units = DxfUnits::of(&Units::architectural());
    let dxf = write_dxf(&drawing(), units).unwrap();
    let dwg = write_dwg(&drawing(), units).unwrap();
    keep("r2018-inches.dxf", &dxf);
    keep("r2018-inches.dwg", &dwg);
    for bytes in [dxf, dwg] {
        let back = if bytes.starts_with(b"AC10") {
            drift_io::dwg_import::read(&bytes, 1.0).unwrap()
        } else {
            read(&String::from_utf8(bytes).unwrap(), 1.0).unwrap()
        };
        check(&back);
    }
}

#[test]
fn more_can_be_added_before_writing() {
    // What walls will do: their own entities in the same document.
    let mut w = CadWriter::new(DxfUnits::MM);
    w.add_drawing(&drawing(), DxfUnits::MM);
    let doc = w.document_mut();
    doc.add_entity(acadrust::EntityType::Line(acadrust::Line::from_points(
        acadrust::Vector3::new(0.0, -100.0, 0.0),
        acadrust::Vector3::new(10.0, -100.0, 0.0),
    )))
    .unwrap();
    let back = read(&String::from_utf8(w.dxf().unwrap()).unwrap(), 1.0).unwrap();
    assert!(back.entities.iter().any(|e| matches!(e.shape,
        Shape::Line { a, .. } if near(a, DVec2::new(0.0, -100.0)) || near(a, DVec2::new(10.0, -100.0)))));
}
