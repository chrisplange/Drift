//! Imported ends that stop just short of another curve, and curves drawn twice (#184):
//! `add_to_document` puts such ends on the curve, so the areas they close are found, and
//! brings a curve drawn twice on a layer in once, with the look of the copy that shows.

use std::f64::consts::{FRAC_PI_2, PI};

use drift_core::{DVec2, Document, LineKind, LineType, LineWeight, Shape};
use drift_geom::regions::regions;
use drift_io::dxf::{DxfEntity, DxfHatch, DxfImport, ImportReport, add_to_document};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line(a: DVec2, b: DVec2) -> Shape {
    Shape::Line { a, b }
}

fn on(layer: &str, shape: Shape) -> DxfEntity {
    DxfEntity {
        layer: layer.into(),
        shape,
        line_type: None,
        color: None,
        weight: None,
    }
}

fn import(entities: Vec<DxfEntity>) -> (Document, ImportReport) {
    let mut doc = Document::new();
    let report = add_to_document(
        &mut doc,
        &DxfImport {
            entities,
            ..Default::default()
        },
    );
    (doc, report)
}

fn curves(doc: &Document) -> Vec<Shape> {
    doc.iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| doc.shape(id).unwrap())
        .collect()
}

fn areas(doc: &Document) -> usize {
    regions(&curves(doc)).len()
}

/// A 1000 × 1000 square (a 1 m sheet: the join tolerance is 1 µm) with a curve `across` it
/// from the bottom, which stops `gap` short of the top edge.
fn square_with(top: Shape, across: impl Fn(f64) -> Shape, gap: f64) -> (Document, ImportReport) {
    import(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(1000.0, 0.0), v(1000.0, 1000.0))),
        on("0", top),
        on("0", line(v(0.0, 1000.0), v(0.0, 0.0))),
        on("0", across(gap)),
    ])
}

fn top_line() -> Shape {
    line(v(1000.0, 1000.0), v(0.0, 1000.0))
}

fn up_the_middle(gap: f64) -> Shape {
    line(v(500.0, 0.0), v(500.0, 1000.0 - gap))
}

#[test]
fn an_end_just_short_of_a_line_is_put_on_it_and_closes_two_areas() {
    let (doc, report) = square_with(top_line(), up_the_middle, 1e-5);
    assert_eq!(areas(&doc), 2);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.starts_with("1 end just short")),
        "{:?}",
        report.notes
    );
    // Moved onto the line, nowhere else.
    let top = doc
        .iter()
        .filter(|(_, e)| e.kind.is_point())
        .map(|(id, _)| doc.point(id).unwrap())
        .find(|p| p.x == 500.0 && p.y > 500.0)
        .unwrap();
    assert_eq!(top, v(500.0, 1000.0));
}

#[test]
fn an_end_just_short_of_an_arc_is_put_on_it() {
    // A half disc: the arc over the top, the diameter along the bottom, and a line up from
    // the diameter stopping short of the arc.
    let (doc, _) = import(vec![
        on(
            "0",
            Shape::Arc {
                center: v(500.0, 0.0),
                radius: 500.0,
                start: 0.0,
                sweep: PI,
            },
        ),
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(200.0, 0.0), v(200.0, 400.0 - 1e-5))),
    ]);
    assert_eq!(areas(&doc), 2);
}

#[test]
fn an_end_just_short_of_a_bezier_is_put_on_it() {
    // A bottom line and a Bézier over it, and a line up from the bottom stopping short of
    // the Bézier's middle (where it is at its highest, y = 750).
    let (doc, _) = import(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on(
            "0",
            Shape::Bezier {
                p: [
                    v(1000.0, 0.0),
                    v(1000.0, 1000.0),
                    v(0.0, 1000.0),
                    v(0.0, 0.0),
                ],
            },
        ),
        on("0", line(v(500.0, 0.0), v(500.0, 750.0 - 5e-4))),
    ]);
    assert_eq!(areas(&doc), 2);
}

#[test]
fn an_arc_end_just_short_of_a_line_is_put_on_it_and_keeps_its_radius() {
    // A quarter circle from the bottom edge, stopping short of the left edge, closes the
    // corner of the square with them.
    let (doc, _) = import(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(0.0, 1000.0), v(0.0, 0.0))),
        on(
            "0",
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 400.0,
                start: 0.0,
                sweep: FRAC_PI_2 - 2e-8,
            },
        ),
    ]);
    assert_eq!(areas(&doc), 1);
    let arc = curves(&doc)
        .into_iter()
        .find(|s| matches!(s, Shape::Arc { .. }))
        .unwrap();
    let Shape::Arc { radius, .. } = arc else {
        unreachable!()
    };
    assert!((radius - 400.0).abs() < 1e-12, "{radius}");
}

#[test]
fn ends_short_of_one_spot_from_either_side_become_one_point() {
    // A line across the middle, and lines from the top and the bottom edges that stop just
    // short of one spot on it, from either side: they meet there, at one point.
    let across = line(v(0.0, 500.0), v(1000.0, 500.0));
    let (doc, _) = import(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(1000.0, 0.0), v(1000.0, 1000.0))),
        on("0", line(v(1000.0, 1000.0), v(0.0, 1000.0))),
        on("0", line(v(0.0, 1000.0), v(0.0, 0.0))),
        on("0", across),
        // 0.9 µm short, 1.9 µm apart (so not one point to begin with), landing 0.5 µm apart.
        on("0", line(v(500.0, 0.0), v(500.0, 500.0 - 9e-4))),
        on(
            "0",
            line(v(500.0 + 5e-4, 1000.0), v(500.0 + 5e-4, 500.0 + 9e-4)),
        ),
    ]);
    assert_eq!(areas(&doc), 4);
    let ends: Vec<_> = doc
        .iter()
        .filter_map(|(_, e)| e.kind.ends())
        .flatten()
        .filter(|p| doc.point(*p).unwrap().y == 500.0)
        .collect();
    // The two ends on the line across, and that line's own two.
    assert_eq!(ends.len(), 4, "{ends:?}");
    assert_eq!(
        ends.iter().filter(|p| **p == ends[2]).count(),
        2,
        "{ends:?}"
    );
    // No point is left over from the one that went.
    assert_eq!(doc.iter().filter(|(_, e)| e.kind.is_point()).count(), 9);
}

#[test]
fn a_gap_of_two_micrometres_on_a_one_metre_sheet_stays_open() {
    let (doc, report) = square_with(top_line(), up_the_middle, 2e-3);
    assert_eq!(areas(&doc), 1);
    assert!(!report.notes.iter().any(|n| n.contains("just short")));
}

#[test]
fn a_short_curve_is_not_stretched_to_reach_a_curve() {
    // A line 2 µm long ending 0.8 µm short of the top edge: within the join tolerance, but
    // moving its end would take more than a quarter of it.
    let (doc, _) = square_with(
        top_line(),
        |gap| line(v(500.0, 1000.0 - gap - 2e-3), v(500.0, 1000.0 - gap)),
        8e-4,
    );
    let short = curves(&doc)
        .into_iter()
        .find(|s| matches!(s, Shape::Line { a, .. } if a.x == 500.0))
        .unwrap();
    let Shape::Line { a, b } = short else {
        unreachable!()
    };
    assert!((a.distance(b) - 2e-3).abs() < 1e-12, "{}", a.distance(b));
    assert_eq!(a.y.max(b.y), 1000.0 - 8e-4);
}

/// Two parts of a 21.6 m CAD drawing (a line along its bottom, 21.6 m long) that meet
/// across `gap` near (10 m, 10 m): ends of two lines, and an end stopping short of a line.
fn building(gap: f64, sheet_rounded: bool) -> Document {
    let mut doc = Document::new();
    let at = v(10_000.0, 10_000.0);
    add_to_document(
        &mut doc,
        &DxfImport {
            entities: vec![
                on("0", line(v(0.0, 0.0), v(21_600.0, 0.0))),
                on("0", line(at - v(500.0, 0.0), at)),
                on("0", line(at + v(gap, 0.0), at + v(500.0, 0.0))),
                on("0", line(at + v(0.0, 100.0), at + v(500.0, 100.0))),
                on("0", line(at + v(200.0, 100.0 + gap), at + v(200.0, 600.0))),
            ],
            sheet_rounded,
            ..Default::default()
        },
    );
    doc
}

fn points(doc: &Document) -> usize {
    doc.iter().filter(|(_, e)| e.kind.is_point()).count()
}

#[test]
fn a_hundredth_of_a_millimetre_between_parts_of_a_building_drawing_stays_open() {
    // The join there would be 21.6 µm (1e-6 of its size) without the cap.
    let doc = building(0.01, false);
    assert_eq!(points(&doc), 10);
    let ends: Vec<DVec2> = doc
        .iter()
        .filter(|(_, e)| e.kind.is_point())
        .map(|(id, _)| doc.point(id).unwrap())
        .collect();
    assert!(ends.contains(&v(10_200.0, 10_100.01)), "{ends:?}");
    // Within the cap, the ends join, and the end short of the line is put on it.
    let joined = |doc: &Document| {
        let on_line = doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .any(|(id, _)| doc.point(id).unwrap() == v(10_200.0, 10_100.0));
        (points(doc), on_line)
    };
    assert_eq!(joined(&building(5e-4, false)), (9, true));
    // As do 3 µm (a DXF written to 4 decimals of an inch rounds to 2.54 µm).
    assert_eq!(joined(&building(3e-3, false)), (9, true));
    // A PDF or SVG of that size isn't capped: its coordinates were rounded to the sheet.
    assert_eq!(joined(&building(0.01, true)), (9, true));
}

fn weighted(mut e: DxfEntity, weight: u16, line_type: LineType) -> DxfEntity {
    e.weight = Some(LineWeight(weight));
    e.line_type = Some(line_type);
    e
}

#[test]
fn a_curve_drawn_twice_comes_in_once_with_the_heavier_weight() {
    let edge = line(v(0.0, 0.0), v(1000.0, 0.0));
    let (doc, report) = import(vec![
        weighted(on("0", edge), 9, LineType::Continuous),
        // The same line, the other way round and a fraction of a micrometre off.
        weighted(
            on("0", line(v(1000.0, 1e-7), v(0.0, 0.0))),
            25,
            LineType::Continuous,
        ),
        // And once more, thinner again.
        weighted(on("0", edge), 13, LineType::Continuous),
    ]);
    assert_eq!(curves(&doc).len(), 1);
    let (_, e) = doc.iter().find(|(_, e)| !e.kind.is_point()).unwrap();
    assert_eq!(e.appearance.weight, Some(LineWeight(25)));
    assert_eq!(report.curves, 1);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n == "2 curves drawn twice in the file came in once."),
        "{:?}",
        report.notes
    );
}

#[test]
fn at_the_same_weight_a_continuous_copy_shows_through_a_dashed_one() {
    let edge = line(v(0.0, 0.0), v(1000.0, 0.0));
    let (doc, _) = import(vec![
        weighted(on("0", edge), 25, LineType::Continuous),
        weighted(on("0", edge), 25, LineType::Dashed),
    ]);
    assert_eq!(curves(&doc).len(), 1);
    let (_, e) = doc.iter().find(|(_, e)| !e.kind.is_point()).unwrap();
    assert_eq!(e.appearance.line_type, Some(LineType::Continuous));
}

#[test]
fn copies_on_different_layers_are_both_kept() {
    let edge = line(v(0.0, 0.0), v(1000.0, 0.0));
    let (doc, report) = import(vec![on("WALLS", edge), on("FURNITURE", edge)]);
    assert_eq!(curves(&doc).len(), 2);
    assert!(!report.notes.iter().any(|n| n.contains("drawn twice")));
}

#[test]
fn a_hatch_finds_its_outline_whichever_side_of_a_grid_cell_its_points_fall() {
    // The outline's corners and the hatch's are 0.02 µm apart (one point each), on either
    // side of a cell boundary of the 1 µm grid points are found by (x/tol rounds to 10 and
    // 11).
    let (a, b) = (10.49e-3, 10.51e-3);
    let ring = |x: f64| {
        vec![
            line(v(x, 0.0), v(1000.0, 0.0)),
            line(v(1000.0, 0.0), v(1000.0, 1000.0)),
            line(v(1000.0, 1000.0), v(x, 1000.0)),
            line(v(x, 1000.0), v(x, 0.0)),
        ]
    };
    let mut doc = Document::new();
    let report = add_to_document(
        &mut doc,
        &DxfImport {
            entities: ring(a).into_iter().map(|s| on("0", s)).collect(),
            hatches: vec![DxfHatch {
                layer: "0".into(),
                loops: vec![ring(b)],
                solid: true,
                pattern: "SOLID".into(),
                scale: 1.0,
                angle: 0.0,
                definition: Vec::new(),
                origin: DVec2::ZERO,
                color: None,
                opacity: None,
                seeds: Vec::new(),
            }],
            fill_edges_construction: true,
            ..Default::default()
        },
    );
    assert_eq!(report.fills, 1);
    assert_eq!(curves(&doc).len(), 4);
    assert!(
        doc.iter()
            .filter(|(_, e)| !e.kind.is_point())
            .all(|(_, e)| e.style == LineKind::Solid)
    );
}
