//! Dashed curves cut at a view's window (#258) keep their dashes where the whole curve had
//! them: each piece's PDF dash phase and SVG dash offset is how far along the whole curve it
//! starts.

use std::f64::consts::{FRAC_PI_2, PI};

use drift_core::{Bounds, DVec2, LineType};
use drift_draft::page::{Drawing, Item, Pen, Prim};

const PT_PER_MM: f64 = 72.0 / 25.4;

const WINDOW: Bounds = Bounds {
    min: DVec2::new(20.0, 20.0),
    max: DVec2::new(120.0, 70.0),
};

/// A dashed line, arc and closed polyline, each across the window's edge, and how far along
/// each a point of it lies (from its first point, the way it is drawn).
#[allow(clippy::type_complexity)]
fn dashed() -> (Drawing, Vec<Box<dyn Fn(DVec2) -> f64>>) {
    let mut d = Drawing::new(DVec2::new(200.0, 100.0));
    let pen = Pen::new(0.25).with_type(LineType::Dashed);
    let mut add = |prim| {
        d.items.push(Item {
            layer: "0".into(),
            pen,
            prim,
            clip: Some(WINDOW),
        })
    };
    // From x = 0, entering at x = 20.
    add(Prim::Line(DVec2::new(0.0, 40.0), DVec2::new(100.0, 40.0)));
    // From the top of a circle round 130, 45 anticlockwise: in where it crosses x = 120.
    add(Prim::Arc {
        center: DVec2::new(130.0, 45.0),
        radius: 15.0,
        start: FRAC_PI_2,
        sweep: PI,
    });
    // A square from inside, out across x = 20 and back: cut open, its run goes across its
    // first point.
    let square = [
        DVec2::new(30.0, 50.0),
        DVec2::new(30.0, 60.0),
        DVec2::new(10.0, 60.0),
        DVec2::new(10.0, 50.0),
    ];
    add(Prim::Polyline {
        points: square.to_vec(),
        closed: true,
    });
    let along_square = move |p: DVec2| {
        let mut along = 0.0;
        for i in 0..4 {
            let (a, b) = (square[i], square[(i + 1) % 4]);
            let t = (p - a).dot(b - a) / (b - a).length_squared();
            if (0.0..=1.0).contains(&t) && (a + (b - a) * t).distance(p) < 1e-3 {
                return along + a.distance(p);
            }
            along += a.distance(b);
        }
        panic!("{p} isn't on the square")
    };
    let along: Vec<Box<dyn Fn(DVec2) -> f64>> = vec![
        Box::new(|p: DVec2| p.x),
        Box::new(|p: DVec2| {
            let r = p - DVec2::new(130.0, 45.0);
            15.0 * (r.y.atan2(r.x).rem_euclid(2.0 * PI) - FRAC_PI_2)
        }),
        Box::new(along_square),
    ];
    (d, along)
}

/// Each dashed path's first point and dash phase (mm), in the order written.
fn pdf_dashes(pdf: &[u8]) -> Vec<(DVec2, f64)> {
    let text = String::from_utf8_lossy(pdf);
    let mut out = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    // In a dash array, and how many numbers it has.
    let (mut array, mut dashes) = (false, 0);
    let mut phase: Option<f64> = None;
    for token in text.split_whitespace() {
        if token.starts_with('[') {
            array = true;
            dashes = 0;
        }
        if let Ok(v) = token.trim_matches(['[', ']']).parse::<f64>() {
            if array {
                dashes += 1;
            } else {
                nums.push(v);
            }
        }
        if token.ends_with(']') {
            array = false;
            continue;
        }
        if array || token.parse::<f64>().is_ok() {
            continue;
        }
        match token {
            "d" => phase = (dashes > 0).then(|| nums[nums.len() - 1] / PT_PER_MM),
            "m" => {
                if let Some(ph) = phase.take() {
                    out.push((DVec2::new(nums[0], nums[1]) / PT_PER_MM, ph));
                }
            }
            _ => {}
        }
        nums.clear();
    }
    out
}

/// Each dashed path's first point and dash offset (mm, y up), in the order written.
fn svg_dashes(svg: &str, height: f64) -> Vec<(DVec2, f64)> {
    let doc = roxmltree::Document::parse(svg).expect("SVG parses");
    doc.descendants()
        .filter(|n| n.has_tag_name("path") && n.attribute("stroke-dasharray").is_some())
        .map(|n| {
            let d = n.attribute("d").unwrap();
            let nums: Vec<f64> = d
                .split_whitespace()
                .skip(1)
                .take(2)
                .map(|v| v.parse().unwrap())
                .collect();
            let offset = n
                .attribute("stroke-dashoffset")
                .map_or(0.0, |v| v.parse().unwrap());
            (DVec2::new(nums[0], height - nums[1]), offset)
        })
        .collect()
}

/// How close a written number is to the true one: the writers' own rounding (PDF 0.001 pt,
/// SVG 0.0001 mm) on the point and the phase.
const WRITTEN: f64 = 1e-3;

#[test]
fn cut_dashed_curves_keep_their_dashes_in_place() {
    let (mut d, along) = dashed();
    let whole = d.clone();
    d.crop_to_windows();
    assert_eq!(d.items.len(), 3, "{:?}", d.items);
    // The pieces themselves, exactly.
    for (it, along) in d.items.iter().zip(&along) {
        let first = match &it.prim {
            Prim::Line(a, _) => *a,
            Prim::Polyline { points, .. } => points[0],
            Prim::Arc {
                center,
                radius,
                start,
                ..
            } => *center + DVec2::from_angle(*start) * *radius,
            prim => panic!("{prim:?}"),
        };
        assert!(it.pen.dash_offset > 0.0);
        assert!(
            (it.pen.dash_offset - along(first)).abs() < 1e-6,
            "{} vs {}",
            it.pen.dash_offset,
            along(first)
        );
    }
    // As written: PDF phase and SVG offset where the whole curve has its dashes.
    let pdf = drift_io::pdf::write(std::slice::from_ref(&d), "Dashes");
    let svg = drift_io::svg::write(&d);
    for written in [pdf_dashes(&pdf), svg_dashes(&svg, d.size.y)] {
        assert_eq!(written.len(), 3, "{written:?}");
        for ((first, phase), along) in written.iter().zip(&along) {
            assert!(
                (phase - along(*first)).abs() < WRITTEN,
                "phase {phase} at {first}, {} along",
                along(*first)
            );
        }
    }
    // The whole curves, uncut, start their dashes at 0.
    let pdf = drift_io::pdf::write(std::slice::from_ref(&whole), "Dashes");
    assert!(pdf_dashes(&pdf).iter().all(|(_, phase)| *phase == 0.0));
    assert!(!drift_io::svg::write(&whole).contains("stroke-dashoffset"));
}

#[test]
fn undashed_pieces_write_no_phase() {
    let mut d = Drawing::new(DVec2::new(200.0, 100.0));
    d.items.push(Item {
        layer: "0".into(),
        pen: Pen::new(0.25),
        prim: Prim::Line(DVec2::new(0.0, 40.0), DVec2::new(100.0, 40.0)),
        clip: Some(WINDOW),
    });
    d.crop_to_windows();
    let pdf = String::from_utf8_lossy(&drift_io::pdf::write(&[d.clone()], "Plain")).into_owned();
    assert!(pdf.contains("[] 0 d"), "{pdf}");
    assert!(!drift_io::svg::write(&d).contains("stroke-dash"));
}
