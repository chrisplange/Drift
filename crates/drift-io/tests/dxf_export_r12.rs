//! DXF export writes R12 (AC1009) with only R12's own entities: AutoCAD discards a whole
//! AC1009 file at the first newer entity (it refused ELLIPSE and SPLINE: "Invalid or
//! incomplete DXF input -- drawing discarded"). Curves R12 lacks are written as polylines
//! close to them, and text outside ASCII as `\U+XXXX` escapes.
#![cfg(not(feature = "demo"))]

use std::f64::consts::{PI, TAU};

use drift_core::{DVec2, Shape, Units};
use drift_draft::page::{Align, BlockDef, Drawing, Item, Pen, Prim};
use drift_io::dxf::{CURVE_TOLERANCE, DxfUnits, read, write};

/// The entity and table types an R12 reader knows.
const R12: &[&str] = &[
    "SECTION",
    "ENDSEC",
    "TABLE",
    "ENDTAB",
    "EOF",
    "LTYPE",
    "LAYER",
    "STYLE",
    "VIEW",
    "UCS",
    "VPORT",
    "APPID",
    "DIMSTYLE",
    "BLOCK",
    "ENDBLK",
    "LINE",
    "POINT",
    "CIRCLE",
    "ARC",
    "TRACE",
    "SOLID",
    "TEXT",
    "SHAPE",
    "INSERT",
    "ATTDEF",
    "ATTRIB",
    "POLYLINE",
    "VERTEX",
    "SEQEND",
    "3DFACE",
    "VIEWPORT",
    "DIMENSION",
];

/// Every record type (group 0) in a DXF file.
fn kinds(dxf: &str) -> Vec<String> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    lines
        .chunks(2)
        .filter(|p| p.len() == 2 && p[0] == "0")
        .map(|p| p[1].to_owned())
        .collect()
}

const ELLIPSE_CENTRE: DVec2 = DVec2::new(500.0, 300.0);
const ELLIPSE_MAJOR: DVec2 = DVec2::new(400.0, 150.0);
const ELLIPSE_MINOR: f64 = 90.0;
const BEZIER: [DVec2; 4] = [
    DVec2::new(0.0, 0.0),
    DVec2::new(100.0, 400.0),
    DVec2::new(600.0, -300.0),
    DVec2::new(900.0, 100.0),
];

/// A drawing with every kind of thing the writer writes, curves R12 lacks among them.
fn everything() -> Drawing {
    let mut d = Drawing::new(DVec2::new(1000.0, 1000.0));
    let pen = Pen::new(0.25);
    d.line("Lines", pen, DVec2::ZERO, DVec2::new(100.0, 0.0));
    d.rect("Lines", pen, DVec2::ZERO, DVec2::splat(50.0));
    d.push(
        "Curves",
        pen,
        Prim::Circle {
            center: DVec2::splat(20.0),
            radius: 5.0,
        },
    );
    d.push(
        "Curves",
        pen,
        Prim::Arc {
            center: DVec2::splat(20.0),
            radius: 8.0,
            start: 0.0,
            sweep: PI,
        },
    );
    d.push(
        "Curves",
        pen,
        Prim::Ellipse {
            center: ELLIPSE_CENTRE,
            major: ELLIPSE_MAJOR,
            minor: ELLIPSE_MINOR,
            start: 0.0,
            sweep: TAU,
        },
    );
    d.push(
        "Curves",
        pen,
        Prim::Ellipse {
            center: DVec2::new(-200.0, 0.0),
            // The second axis the longer one.
            major: DVec2::new(0.0, 20.0),
            minor: 60.0,
            start: 0.5,
            sweep: 2.0,
        },
    );
    d.push("Curves", pen, Prim::Bezier { p: BEZIER });
    d.filled_rect("Fills", pen, DVec2::splat(60.0), DVec2::splat(80.0));
    d.text(
        "Notes",
        pen,
        DVec2::new(0.0, -20.0),
        3.5,
        Align::Left,
        "Plain",
    );
    d.blocks.push(BlockDef {
        name: "Knob".into(),
        items: vec![Item {
            layer: "0".into(),
            pen,
            prim: Prim::Bezier {
                p: [
                    DVec2::ZERO,
                    DVec2::new(0.0, 10.0),
                    DVec2::new(10.0, 10.0),
                    DVec2::new(10.0, 0.0),
                ],
            },
            clip: None,
        }],
    });
    d.push(
        "Blocks",
        pen,
        Prim::Insert {
            block: "Knob".into(),
            at: DVec2::new(300.0, 0.0),
            angle: 0.0,
            scale: 1.0,
            mirror: false,
        },
    );
    d
}

#[test]
fn export_holds_only_r12_entities() {
    for units in [DxfUnits::MM, DxfUnits::of(&Units::architectural())] {
        let dxf = write(&everything(), units);
        assert!(dxf.contains("AC1009"));
        let newer: Vec<String> = kinds(&dxf)
            .into_iter()
            .filter(|k| !R12.contains(&k.as_str()))
            .collect();
        assert!(newer.is_empty(), "not R12: {newer:?}");
        // Everything is still there: lines, polylines for the curves, the fill, the text,
        // the block and its copy.
        let all = kinds(&dxf);
        for k in [
            "LINE", "POLYLINE", "CIRCLE", "ARC", "SOLID", "TEXT", "BLOCK", "INSERT",
        ] {
            assert!(all.iter().any(|x| x == k), "no {k}");
        }
    }
}

/// The nearest distance from `p` to points along a curve.
fn distance_to(p: DVec2, curve: &[DVec2]) -> f64 {
    curve
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let t = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-300)).clamp(0.0, 1.0);
            p.distance(a + (b - a) * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The curves read back from the export as their segments' ends and middles (mm).
fn read_back_points(dxf: &str, layer: &str) -> Vec<DVec2> {
    let back = read(dxf, 1.0).unwrap();
    let mut out = Vec::new();
    for e in back.entities.iter().filter(|e| e.layer == layer) {
        if let Shape::Line { a, b } = e.shape {
            out.extend([a, b, (a + b) * 0.5]);
        }
    }
    out
}

#[test]
fn curves_come_back_within_the_tolerance() {
    let ellipse: Vec<DVec2> = (0..=5_000)
        .map(|k| {
            drift_core::shape::ellipse_point(
                ELLIPSE_CENTRE,
                ELLIPSE_MAJOR,
                ELLIPSE_MINOR,
                TAU * k as f64 / 5_000.0,
            )
        })
        .collect();
    let bezier: Vec<DVec2> = (0..=5_000)
        .map(|k| {
            let t = k as f64 / 5_000.0;
            let u = 1.0 - t;
            BEZIER[0] * (u * u * u)
                + BEZIER[1] * (3.0 * u * u * t)
                + BEZIER[2] * (3.0 * u * t * t)
                + BEZIER[3] * (t * t * t)
        })
        .collect();
    for units in [DxfUnits::MM, DxfUnits::of(&Units::architectural())] {
        let mut d = Drawing::new(DVec2::splat(1000.0));
        let pen = Pen::new(0.25);
        d.push(
            "E",
            pen,
            Prim::Ellipse {
                center: ELLIPSE_CENTRE,
                major: ELLIPSE_MAJOR,
                minor: ELLIPSE_MINOR,
                start: 0.0,
                sweep: TAU,
            },
        );
        d.push("B", pen, Prim::Bezier { p: BEZIER });
        let dxf = write(&d, units);
        // Six decimals in the file's unit, plus the sampling of the reference curve.
        let slack = 1e-6 * units.mm_per_unit + 0.002;
        let e = read_back_points(&dxf, "E");
        assert!(e.len() > 30, "{}", e.len());
        let worst = e
            .iter()
            .map(|p| distance_to(*p, &ellipse))
            .fold(0.0, f64::max);
        assert!(worst <= CURVE_TOLERANCE + slack, "ellipse off by {worst}");
        let b = read_back_points(&dxf, "B");
        let worst = b
            .iter()
            .map(|p| distance_to(*p, &bezier))
            .fold(0.0, f64::max);
        assert!(worst <= CURVE_TOLERANCE + slack, "Bézier off by {worst}");
        // The whole ellipse is closed: every end is shared by two segments.
        let back = read(&dxf, 1.0).unwrap();
        let ends: Vec<DVec2> = back
            .entities
            .iter()
            .filter(|e| e.layer == "E")
            .flat_map(|e| match e.shape {
                Shape::Line { a, b } => vec![a, b],
                _ => vec![],
            })
            .collect();
        for p in &ends {
            let shared = ends.iter().filter(|q| q.distance(*p) < 1e-6).count();
            assert_eq!(shared, 2, "an open end at {p}");
        }
    }
}

#[test]
fn text_outside_ascii_comes_back() {
    let samples = [
        "Café façade – 25 m² ½",
        "“Quoted” Ø50 ±1 90°",
        "Größe Žluťoučký Привет 中文",
        "100%% done, 5%",
    ];
    let mut d = Drawing::new(DVec2::splat(100.0));
    for (k, s) in samples.iter().enumerate() {
        d.text(
            "Notes",
            Pen::new(0.25),
            DVec2::new(0.0, k as f64 * 10.0),
            3.5,
            Align::Left,
            *s,
        );
    }
    let dxf = write(&d, DxfUnits::MM);
    assert!(dxf.is_ascii(), "the file itself stays ASCII");
    assert!(!dxf.contains('?'), "nothing became a question mark");
    let back = read(&dxf, 1.0).unwrap();
    let texts: Vec<&str> = back.texts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, samples);
}
