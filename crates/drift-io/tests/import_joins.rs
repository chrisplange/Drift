//! Imported ends that stop just short of another curve, and curves drawn twice (#184):
//! `add_to_document` puts such ends on the curve (corners too, #271), so the areas they
//! close are found, and brings a curve drawn twice on a layer in once, with the look of the
//! copy that shows; a line drawn over part of another is trimmed to meet it (#269); and a
//! hatch's edges find the curves they were drawn on, in blocks too (#270).

use std::f64::consts::{FRAC_PI_2, PI};

use drift_core::{DVec2, Document, LineKind, LineType, LineWeight, Shape};
use drift_geom::regions::{region_at, regions};
use drift_io::dxf::{
    DxfBlock, DxfEntity, DxfHatch, DxfImport, DxfInsert, ImportReport, add_to_document,
};

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

/// A solid hatch on `loops`.
fn solid(loops: Vec<Vec<Shape>>) -> DxfHatch {
    DxfHatch {
        layer: "0".into(),
        loops,
        solid: true,
        pattern: "SOLID".into(),
        scale: 1.0,
        angle: 0.0,
        definition: Vec::new(),
        origin: DVec2::ZERO,
        color: None,
        opacity: None,
        seeds: Vec::new(),
    }
}

/// A 1000 × 1000 square whose left side is at `x`.
fn ring(x: f64) -> Vec<Shape> {
    vec![
        line(v(x, 0.0), v(1000.0, 0.0)),
        line(v(1000.0, 0.0), v(1000.0, 1000.0)),
        line(v(1000.0, 1000.0), v(x, 1000.0)),
        line(v(x, 1000.0), v(x, 0.0)),
    ]
}

// The outline's corners and the hatch's are 0.02 µm apart (one point each), on either side
// of a cell boundary of the 1 µm grid points are found by (x/tol rounds to 10 and 11).
const OUTLINE_X: f64 = 10.49e-3;
const HATCH_X: f64 = 10.51e-3;

#[test]
fn a_hatch_finds_its_outline_whichever_side_of_a_grid_cell_its_points_fall() {
    let mut doc = Document::new();
    let report = add_to_document(
        &mut doc,
        &DxfImport {
            entities: ring(OUTLINE_X).into_iter().map(|s| on("0", s)).collect(),
            hatches: vec![solid(vec![ring(HATCH_X)])],
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

#[test]
fn a_hatch_in_a_block_finds_its_outline_whichever_side_of_a_grid_cell_its_points_fall() {
    // (#270) The same square and hatch inside a block, placed once; the drawing's own copy
    // of the square makes it 1 m across, for the 1 µm join.
    let mut doc = Document::new();
    let report = add_to_document(
        &mut doc,
        &DxfImport {
            entities: ring(OUTLINE_X).into_iter().map(|s| on("0", s)).collect(),
            blocks: vec![DxfBlock {
                name: "SQUARE".into(),
                label: String::new(),
                entities: ring(OUTLINE_X).into_iter().map(|s| on("0", s)).collect(),
                texts: Vec::new(),
                hatches: vec![solid(vec![ring(HATCH_X)])],
            }],
            inserts: vec![DxfInsert {
                block: "SQUARE".into(),
                layer: "0".into(),
                at: v(2000.0, 0.0),
                angle: 0.0,
                scale: 1.0,
                mirror: false,
            }],
            fill_edges_construction: true,
            ..Default::default()
        },
    );
    assert_eq!(report.blocks, 1);
    let (_, block) = doc.blocks().next().unwrap();
    assert_eq!(block.fills.len(), 1);
    assert_eq!(block.items.len(), 4, "{:?}", block.items);
    assert!(block.items.iter().all(|it| it.style == LineKind::Solid));
}

#[test]
fn a_corner_just_short_of_a_line_is_put_on_it_with_both_its_curves() {
    // (#271) A V up from the bottom whose tip, where its two lines meet, stops 0.5 µm short
    // of the top edge: put on it, the V parts the square into three.
    let tip = |gap: f64| v(500.0, 1000.0 - gap);
    let (doc, report) = import(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(1000.0, 0.0), v(1000.0, 1000.0))),
        on("0", top_line()),
        on("0", line(v(0.0, 1000.0), v(0.0, 0.0))),
        on("0", line(v(300.0, 0.0), tip(5e-4))),
        on("0", line(tip(5e-4), v(700.0, 0.0))),
    ]);
    assert_eq!(areas(&doc), 3);
    assert!(
        doc.iter()
            .filter(|(_, e)| e.kind.is_point())
            .any(|(id, _)| doc.point(id).unwrap() == tip(0.0))
    );
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.starts_with("1 end just short")),
        "{:?}",
        report.notes
    );
}

/// The 1000 × 1000 square with its bottom side drawn as `bottom` instead.
fn square_on(bottom: Vec<DxfEntity>) -> (Document, ImportReport) {
    let mut entities = bottom;
    entities.extend([
        on("0", line(v(1000.0, 0.0), v(1000.0, 1000.0))),
        on("0", top_line()),
        on("0", line(v(0.0, 1000.0), v(0.0, 0.0))),
    ]);
    import(entities)
}

/// The lines of `doc` along the bottom, as (start x, end x, weight) left to right.
fn bottom(doc: &Document) -> Vec<(f64, f64, Option<LineWeight>)> {
    let mut lines: Vec<_> = doc
        .iter()
        .filter_map(|(id, e)| match doc.shape(id) {
            Some(Shape::Line { a, b }) if a.y.abs() < 1e-2 && b.y.abs() < 1e-2 => {
                Some((a.x.min(b.x), a.x.max(b.x), e.appearance.weight))
            }
            _ => None,
        })
        .collect();
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

/// Areas narrower than 0.1 µm (twice the area over the perimeter).
fn slivers(doc: &Document) -> usize {
    regions(&curves(doc))
        .iter()
        .filter(|r| 2.0 * r.area / r.perimeter < 1e-4)
        .count()
}

#[test]
fn a_line_stroked_over_part_of_another_is_trimmed_to_meet_it() {
    // (#269) The bottom drawn as two strokes, 0.05 µm apart, overlapping from 400 to 600.
    let (doc, report) = square_on(vec![
        on("0", line(v(0.0, 0.0), v(600.0, 0.0))),
        on("0", line(v(400.0, 5e-5), v(1000.0, 5e-5))),
    ]);
    assert_eq!(slivers(&doc), 0);
    assert_eq!(areas(&doc), 1);
    let lines = bottom(&doc);
    assert_eq!(lines.len(), 2, "{lines:?}");
    // As long as each other and alike: the first drawn stays whole, the other meets it.
    assert_eq!((lines[0].0, lines[0].1), (0.0, 600.0));
    assert_eq!(lines[1].0, 600.0);
    assert_eq!(points(&doc), 5);
    assert_eq!(report.curves, 5);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n == "1 line along another on the same layer was trimmed to meet it."),
        "{:?}",
        report.notes
    );
}

#[test]
fn a_line_covered_by_another_goes() {
    let (doc, report) = square_on(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(700.0, -3e-5), v(200.0, 3e-5))),
    ]);
    assert_eq!(bottom(&doc).len(), 1);
    assert_eq!(points(&doc), 4);
    assert_eq!(report.curves, 4);
    assert_eq!(slivers(&doc), 0);
}

#[test]
fn a_heavy_line_inside_a_thin_one_splits_it() {
    let (doc, report) = square_on(vec![
        weighted(
            on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
            13,
            LineType::Continuous,
        ),
        weighted(
            on("0", line(v(400.0, 2e-5), v(600.0, 2e-5))),
            50,
            LineType::Continuous,
        ),
    ]);
    let lines = bottom(&doc);
    let thin = Some(LineWeight(13));
    assert_eq!(
        lines,
        vec![
            (0.0, 400.0, thin),
            (400.0, 600.0, Some(LineWeight(50))),
            (600.0, 1000.0, thin)
        ]
    );
    // The bottom's four points: its corners, and the heavy line's ends shared by the thin
    // pieces.
    assert_eq!(points(&doc), 6);
    assert_eq!(report.curves, curves(&doc).len());
    assert_eq!(areas(&doc), 1);
    assert_eq!(slivers(&doc), 0);
}

#[test]
fn lines_far_apart_or_on_other_layers_stay_whole() {
    let (doc, report) = square_on(vec![
        on("0", line(v(0.0, 0.0), v(600.0, 0.0))),
        on("0", line(v(400.0, 2e-3), v(1000.0, 2e-3))),
        on("1", line(v(300.0, 0.0), v(700.0, 0.0))),
    ]);
    assert_eq!(bottom(&doc).len(), 3);
    assert!(!report.notes.iter().any(|n| n.contains("trimmed")));
}

#[test]
fn a_fill_on_a_line_cut_back_keeps_its_area() {
    // A heavier stroke along the bottom from outside the square to its middle, and a hatch
    // on the square: its bottom edge is the drawn bottom, cut back to meet the stroke.
    let mut doc = Document::new();
    let mut entities: Vec<DxfEntity> = ring(0.0).into_iter().map(|s| on("0", s)).collect();
    entities.push(weighted(
        on("0", line(v(-200.0, 3e-5), v(500.0, 3e-5))),
        50,
        LineType::Continuous,
    ));
    let report = add_to_document(
        &mut doc,
        &DxfImport {
            entities,
            hatches: vec![solid(vec![ring(0.0)])],
            ..Default::default()
        },
    );
    assert_eq!(report.fills, 1);
    assert_eq!(slivers(&doc), 0);
    let (_, fill) = doc.fills().next().unwrap();
    assert!(fill.boundary.iter().all(|e| doc.contains(*e)));
    let found = regions(&curves(&doc));
    let area = region_at(&found, fill.seed).unwrap().area;
    // (The stroke is 0.03 µm off the square: 0.02 mm² of its million.)
    assert!((area - 1e6).abs() < 0.1, "{area}");
}

#[test]
fn trimming_comes_out_the_same_every_time() {
    let run = || {
        let (doc, _) = square_on(vec![
            on("0", line(v(0.0, 0.0), v(600.0, 0.0))),
            on("0", line(v(400.0, 5e-5), v(1000.0, 5e-5))),
            on("0", line(v(300.0, -5e-5), v(800.0, -5e-5))),
        ]);
        doc.iter().map(|(id, e)| (id, e.kind)).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn where_curves_met_a_line_that_goes_they_meet_the_line_covering_it() {
    // The bottom drawn whole, and again as two halves 0.03 µm above it, where three lines
    // up to the top meet: the halves go, and the point they met at is put on the bottom.
    let joint = v(500.0, 3e-5);
    let (doc, _) = square_on(vec![
        on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
        on("0", line(v(0.0, 3e-5), joint)),
        on("0", line(joint, v(1000.0, 3e-5))),
        on("0", line(joint, v(250.0, 1000.0))),
        on("0", line(joint, v(500.0, 1000.0))),
        on("0", line(joint, v(750.0, 1000.0))),
    ]);
    assert_eq!(bottom(&doc).len(), 1);
    assert!(
        doc.iter()
            .filter(|(_, e)| e.kind.is_point())
            .any(|(id, _)| doc.point(id).unwrap() == v(500.0, 0.0))
    );
    assert_eq!(areas(&doc), 4);
    assert_eq!(slivers(&doc), 0);
}

#[test]
fn a_line_is_cut_only_by_lines_along_it() {
    // Three strokes along the bottom of a 1 m sheet (1 µm join), 0.9 µm apart, staggered:
    // the outer two are each along the middle one but not along each other. A line up from
    // where the top one starts meets it there.
    let tol = 1e-3;
    let start = v(400.0, 1.8 * tol);
    let (doc, report) = import(vec![
        on("0", line(v(0.0, 1000.0), v(1000.0, 1000.0))),
        on("0", line(v(0.0, 0.0), v(600.0, 0.0))),
        on("0", line(v(300.0, 0.9 * tol), v(900.0, 0.9 * tol))),
        on("0", line(start, v(1000.0, 1.8 * tol))),
        on("0", line(start, v(400.0, 1000.0))),
    ]);
    // The first stays whole; the middle one keeps what it doesn't share with it; the top
    // one is cut only where the middle one lies along it, so it stays where it runs beside
    // the first (more than the join apart from it) and keeps its start.
    let lines = bottom(&doc);
    let spans: Vec<(f64, f64)> = lines.iter().map(|(a, b, _)| (*a, *b)).collect();
    assert_eq!(
        spans,
        vec![
            (0.0, 600.0),
            (400.0, 600.0),
            (600.0, 900.0),
            (900.0, 1000.0)
        ],
        "{lines:?}"
    );
    // The line up still starts on a stroke along the bottom.
    let on_a_stroke = doc.iter().any(|(id, e)| match (e.kind, doc.shape(id)) {
        (_, Some(Shape::Line { a, b })) if a.y < 0.01 && b.y < 0.01 => {
            a.distance(start) < 1e-9 || b.distance(start) < 1e-9
        }
        _ => false,
    });
    assert!(on_a_stroke);
    assert_eq!(report.curves, curves(&doc).len());
}

#[test]
fn the_report_counts_every_piece_a_split_line_makes() {
    // Two heavy strokes inside a thin bottom: it comes in as three pieces.
    let heavy = |x: f64| {
        weighted(
            on("0", line(v(x, 2e-5), v(x + 100.0, 2e-5))),
            50,
            LineType::Continuous,
        )
    };
    let (doc, report) = square_on(vec![
        weighted(
            on("0", line(v(0.0, 0.0), v(1000.0, 0.0))),
            13,
            LineType::Continuous,
        ),
        heavy(200.0),
        heavy(600.0),
    ]);
    assert_eq!(bottom(&doc).len(), 5);
    assert_eq!(report.curves, curves(&doc).len());
}
