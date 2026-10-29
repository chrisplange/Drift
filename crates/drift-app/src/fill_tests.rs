//! Fills and hatches follow their outlines through every kind of edit: moves made by the
//! solver, rotate, scale, edits that replace or split outline curves, copies, undo and redo,
//! and blocks.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use drift_core::{Constraint, Document, EntityId, EntityKind, FillId};
use glam::DVec2;

use crate::editor::Editor;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A closed polygon of lines: its corner points and its sides (side `i` runs from corner
/// `i` to corner `i + 1`).
fn polygon(ed: &mut Editor, corners: &[DVec2]) -> (Vec<EntityId>, Vec<EntityId>) {
    ed.doc.begin();
    let p: Vec<EntityId> = corners.iter().map(|c| ed.doc.add_point(*c)).collect();
    let sides = (0..p.len())
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % p.len()]).unwrap())
        .collect();
    ed.commit("Lines");
    (p, sides)
}

/// A 40 × 20 plate with its corner at the origin.
fn plate(ed: &mut Editor) -> (Vec<EntityId>, Vec<EntityId>) {
    polygon(
        ed,
        &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 20.0), v(0.0, 20.0)],
    )
}

/// The fill's current area: asserts it was found (not lost) and returns its area and centroid.
fn found(ed: &mut Editor, id: FillId) -> (f64, DVec2) {
    let shapes = ed.fill_shapes();
    let shape = shapes.get(&id).expect("the fill is shown");
    assert!(!shape.lost, "the fill lost its area");
    (shape.region.area, shape.region.centroid)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3 * b.abs().max(1.0)
}

/// A fill of the area around `p` as an import or a drawing made before #202 brings it: an
/// area fill, even where its outline would make a shape. Picked, as the Fill tool's is.
fn area_fill_at(ed: &mut Editor, p: DVec2) -> FillId {
    let regions = ed.regions();
    let ids = ed.region_ids();
    let r = drift_geom::regions::region_at(&regions, p).expect("an area");
    let mut boundary: Vec<EntityId> = r.outer.curves().into_iter().map(|i| ids[i]).collect();
    boundary.sort_unstable();
    boundary.dedup();
    let mut fill = drift_core::Fill::new(boundary, p, drift_core::FillKind::Solid);
    ed.fill_style.apply(&mut fill);
    ed.doc.begin();
    let id = ed.doc.add_fill(fill);
    assert!(ed.commit_import("Fill"));
    assert!(ed.doc.fill(id).unwrap().path.is_none());
    ed.selected_fills = [id].into();
    id
}

/// The area of a polygon of corner points.
fn shoelace(ed: &Editor, corners: &[EntityId]) -> f64 {
    let p: Vec<DVec2> = corners.iter().map(|c| ed.doc.point(*c).unwrap()).collect();
    (0..p.len())
        .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
        .sum::<f64>()
        .abs()
        / 2.0
}

/// The fill's seed is inside its area.
fn seed_inside(ed: &mut Editor, id: FillId) -> bool {
    let seed = ed.doc.fill(id).unwrap().seed;
    ed.fill_shapes()[&id].region.contains(seed)
}

/// Curves and defining points: what a rotate or a scale of `curves` moves.
fn with_points(ed: &Editor, curves: &[EntityId]) -> BTreeSet<EntityId> {
    ed.doc.defining_points(curves.iter().copied())
}

/// Undo and redo `steps`, checking the fill is found each time with `before` and `after`
/// areas.
fn undo_redo(ed: &mut Editor, id: FillId, before: f64, after: f64) {
    assert!(ed.undo());
    assert!(close(found(ed, id).0, before), "after undo");
    assert!(ed.redo());
    assert!(close(found(ed, id).0, after), "after redo");
}

const CORNER_ROUND: f64 = 25.0 - PI * 25.0 / 4.0;

#[test]
fn a_fillet_or_chamfer_at_the_corner_the_fill_was_picked_in() {
    // Picked right in the corner: the fillet cuts the point it was picked at away.
    for chamfer in [false, true] {
        let mut ed = Editor::default();
        let (p, _) = plate(&mut ed);
        let id = ed.add_fill_at(v(1.0, 1.0)).unwrap();
        let cut = if chamfer {
            ed.chamfer(p[0], 5.0, 5.0).unwrap()
        } else {
            ed.fillet(p[0], 5.0).unwrap()
        };
        let expected = 800.0 - if chamfer { 12.5 } else { CORNER_ROUND };
        assert!(close(found(&mut ed, id).0, expected), "chamfer {chamfer}");
        // The cut is part of its outline now.
        assert!(ed.doc.fill(id).unwrap().boundary.contains(&cut));
        assert!(seed_inside(&mut ed, id));
        undo_redo(&mut ed, id, 800.0, expected);
    }
}

#[test]
fn removing_a_fillet_keeps_the_fill() {
    let mut ed = Editor::default();
    let (p, _) = plate(&mut ed);
    let arc = ed.fillet(p[0], 5.0).unwrap();
    let id = ed.add_fill_at(v(20.0, 10.0)).unwrap();
    assert!(ed.doc.fill(id).unwrap().boundary.contains(&arc));
    ed.remove_corner_cut(arc).unwrap();
    assert!(ed.doc.fill(id).is_some(), "the fill went with the fillet");
    assert!(close(found(&mut ed, id).0, 800.0));
    undo_redo(&mut ed, id, 800.0 - CORNER_ROUND, 800.0);
}

#[test]
fn a_split_outline_stays_the_fills_outline() {
    let mut ed = Editor::default();
    let (_, sides) = plate(&mut ed);
    let id = ed.add_fill_at(v(20.0, 10.0)).unwrap();
    ed.split_at(sides[0], v(10.0, 0.0)).unwrap();
    assert!(close(found(&mut ed, id).0, 800.0));
    let boundary = ed.doc.fill(id).unwrap().boundary.clone();
    assert_eq!(boundary.len(), 5, "both pieces bound it");
    // A closed chain of free lines: a shape (#202). Deleting the new piece opens it, with
    // nothing closed round the fill: it keeps its shape closed across the gap, and says so.
    assert!(ed.shape_outline(id).is_some());
    let piece = *boundary.iter().find(|e| !sides.contains(e)).unwrap();
    ed.fill_changes.clear();
    ed.doc.begin();
    ed.doc.delete([piece]);
    ed.commit("Delete");
    assert!(ed.fill_shapes()[&id].lost);
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Opened { id }]
    );
}

#[test]
fn a_point_dragged_past_the_seed_then_a_split() {
    let mut ed = Editor::default();
    let (p, sides) = plate(&mut ed);
    let id = ed.add_fill_at(v(2.0, 2.0)).unwrap();
    // The corner moves in past where the fill was picked.
    assert!(ed.move_point_to(p[0], v(12.0, 10.0)));
    let area = shoelace(&ed, &p);
    assert!(close(found(&mut ed, id).0, area));
    assert!(seed_inside(&mut ed, id), "the seed stays inside");
    ed.split_at(sides[1], v(40.0, 10.0)).unwrap();
    assert!(close(found(&mut ed, id).0, area));
}

#[test]
fn a_dimension_change_then_a_fillet() {
    let mut ed = Editor::default();
    let (p, _) = plate(&mut ed);
    let id = ed.add_fill_at(v(38.0, 1.0)).unwrap();
    let dim = ed
        .add_constraints(
            "Distance",
            &[Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 40.0,
            }],
        )
        .unwrap()[0];
    ed.set_dimension(dim, 20.0).unwrap();
    let area = shoelace(&ed, &p);
    assert!(area < 800.0 - 1.0, "the plate got smaller: {area}");
    assert!(close(found(&mut ed, id).0, area));
    assert!(seed_inside(&mut ed, id));
    let arc = ed.fillet(p[2], 2.0).unwrap();
    assert!(ed.doc.fill(id).unwrap().boundary.contains(&arc));
    assert!(!ed.fill_shapes()[&id].lost);
}

#[test]
fn rotated_and_scaled_outlines_keep_their_fills_through_later_edits() {
    for scale in [false, true] {
        let mut ed = Editor::default();
        let (p, sides) = plate(&mut ed);
        let id = ed.add_fill_at(v(2.0, 2.0)).unwrap();
        let points = with_points(&ed, &sides);
        ed.doc.begin();
        if scale {
            assert!(ed.scale_geometry(&sides, &points, v(40.0, 20.0), DVec2::splat(0.5), false));
        } else {
            let (ok, _) = ed.rotate_geometry(&sides, &points, v(100.0, 0.0), 37.0);
            assert!(ok);
        }
        assert!(ed.commit("Turn"));
        let area = if scale { 200.0 } else { 800.0 };
        assert!(close(found(&mut ed, id).0, area));
        assert!(seed_inside(&mut ed, id), "scale {scale}");
        // An edit that changes the outline's curves afterwards.
        let mid = (ed.doc.point(p[0]).unwrap() + ed.doc.point(p[1]).unwrap()) / 2.0;
        ed.split_at(sides[0], mid).unwrap();
        assert!(close(found(&mut ed, id).0, area), "scale {scale}");
        ed.fillet(p[2], 1.0).unwrap();
        assert!(!ed.fill_shapes()[&id].lost);
    }
}

#[test]
fn a_plate_with_a_hole_keeps_its_hole_through_a_fillet() {
    let mut ed = Editor::default();
    let (p, _) = plate(&mut ed);
    ed.doc.begin();
    let c = ed.doc.add_point(v(20.0, 10.0));
    ed.doc.add_circle(c, 5.0).unwrap();
    ed.commit("Circle");
    let id = ed.add_fill_at(v(0.5, 0.5)).unwrap();
    ed.fillet(p[0], 4.0).unwrap();
    let corner = 16.0 - PI * 4.0;
    assert!(close(found(&mut ed, id).0, 800.0 - 25.0 * PI - corner));
    assert_eq!(ed.fill_shapes()[&id].region.holes.len(), 1);
}

#[test]
fn a_mixed_outline_of_lines_and_arcs() {
    // A slot: two lines and two half circles.
    let mut ed = Editor::default();
    ed.doc.begin();
    let [a, b, c, d] =
        [v(0.0, 0.0), v(30.0, 0.0), v(30.0, 10.0), v(0.0, 10.0)].map(|q| ed.doc.add_point(q));
    let (cr, cl) = (
        ed.doc.add_point(v(30.0, 5.0)),
        ed.doc.add_point(v(0.0, 5.0)),
    );
    let bottom = ed.doc.add_line(a, b).unwrap();
    let right = ed.doc.add_arc(cr, b, c).unwrap();
    ed.doc.add_line(c, d).unwrap();
    ed.doc.add_arc(cl, d, a).unwrap();
    ed.commit("Slot");
    let area = 300.0 + PI * 25.0;
    // Picked in the far end of the right half circle.
    let id = ed.add_fill_at(v(34.5, 5.0)).unwrap();
    assert!(close(found(&mut ed, id).0, area));
    // The right end comes in: the seed is outside the slot now.
    assert!(ed.move_point_to(cr, v(20.0, 5.0)));
    ed.doc.begin();
    ed.doc.set_point(b, v(20.0, 0.0)).unwrap();
    ed.doc.set_point(c, v(20.0, 10.0)).unwrap();
    ed.commit("Shorten");
    let shorter = 200.0 + PI * 25.0;
    assert!(close(found(&mut ed, id).0, shorter));
    ed.split_at(right, v(25.0, 5.0)).unwrap();
    assert!(close(found(&mut ed, id).0, shorter));
    ed.split_at(bottom, v(10.0, 0.0)).unwrap();
    assert!(close(found(&mut ed, id).0, shorter));
}

#[test]
fn a_shrunk_circle_keeps_its_seed_inside() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let circle = ed.doc.add_circle(c, 10.0).unwrap();
    ed.commit("Circle");
    let id = ed.add_fill_at(v(9.0, 0.0)).unwrap();
    ed.doc.begin();
    assert!(ed.drag_radius(circle, 4.0));
    ed.commit("Resize circle");
    assert!(close(found(&mut ed, id).0, PI * 16.0));
    assert!(seed_inside(&mut ed, id));
}

#[test]
fn mirrored_and_patterned_copies_bring_their_fills() {
    let mut ed = Editor::default();
    let (_, sides) = plate(&mut ed);
    ed.doc.begin();
    let (m0, m1) = (
        ed.doc.add_point(v(50.0, -10.0)),
        ed.doc.add_point(v(50.0, 30.0)),
    );
    let axis = ed.doc.add_line(m0, m1).unwrap();
    ed.doc.set_style(axis, drift_core::LineKind::Construction);
    ed.commit("Axis");
    ed.add_fill_at(v(5.0, 5.0)).unwrap();
    ed.mirror(&sides, axis).unwrap();
    assert_eq!(ed.doc.fills().count(), 2);
    let fills: Vec<FillId> = ed.doc.fills().map(|(id, _)| id).collect();
    let copy = fills[1];
    let (area, centre) = found(&mut ed, copy);
    assert!(close(area, 800.0));
    assert!((centre - v(80.0, 10.0)).length() < 1e-6, "{centre}");
    // The hatch is mirrored too: its lines run the mirrored way.
    let angle = |ed: &Editor, id: FillId| match &ed.doc.fill(id).unwrap().kind {
        drift_core::FillKind::Hatch(h) => h.angle,
        drift_core::FillKind::Solid => f64::NAN,
    };
    assert!(close(angle(&ed, copy), 180.0), "{}", angle(&ed, copy));
    ed.undo();
    assert_eq!(ed.doc.fills().count(), 1);

    ed.linear_pattern(&sides, 3, v(0.0, 50.0)).unwrap();
    assert_eq!(ed.doc.fills().count(), 3);
    let mut centres: Vec<f64> = ed
        .fill_shapes()
        .values()
        .map(|s| {
            assert!(!s.lost);
            s.region.centroid.y
        })
        .collect();
    centres.sort_by(f64::total_cmp);
    assert!(close(centres[2], 110.0), "{centres:?}");
    ed.undo();

    ed.circular_pattern(&sides, 4, v(-10.0, -10.0), 360.0)
        .unwrap();
    assert_eq!(ed.doc.fills().count(), 4);
    assert!(
        ed.fill_shapes()
            .values()
            .all(|s| !s.lost && close(s.region.area, 800.0))
    );
    // Each copy's hatch turns with it.
    let mut angles: Vec<f64> = ed
        .doc
        .fills()
        .map(|(id, _)| angle(&ed, id).round())
        .collect();
    angles.sort_by(f64::total_cmp);
    assert_eq!(angles, [0.0, 90.0, 180.0, 270.0]);
}

#[test]
fn a_fill_in_a_block_follows_its_copies_and_edits() {
    let mut ed = Editor::default();
    let (_, sides) = plate(&mut ed);
    ed.add_fill_at(v(1.0, 1.0)).unwrap();
    ed.selection = sides.iter().copied().collect();
    let first = ed.make_block("Plate").unwrap();
    assert_eq!(ed.doc.fills().count(), 0, "the fill went into the block");
    let block = ed.doc.instance(first).unwrap().block;
    ed.place_instance(block, v(100.0, 0.0), 90.0, false);
    let copies = ed.copy_fills();
    assert_eq!(copies.len(), 2);
    assert!(copies.iter().all(|c| close(c.region.area, 800.0)));
    // Round the corner the fill was picked in, in the block.
    ed.edit_block(first);
    let corner = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Point(_)))
        .map(|(id, _)| id)
        .min_by(|a, b| {
            let (pa, pb) = (ed.doc.point(*a).unwrap(), ed.doc.point(*b).unwrap());
            (pa.x + pa.y).total_cmp(&(pb.x + pb.y))
        })
        .unwrap();
    ed.fillet(corner, 5.0).unwrap();
    ed.finish_block_edit();
    let copies = ed.copy_fills();
    assert_eq!(copies.len(), 2, "both copies keep the fill");
    assert!(
        copies
            .iter()
            .all(|c| close(c.region.area, 800.0 - CORNER_ROUND)),
        "{:?}",
        copies.iter().map(|c| c.region.area).collect::<Vec<_>>()
    );
}

#[test]
fn the_seed_follows_when_nothing_else_changes() {
    // A plain drag: the fill's seed moves into the new area, as part of the same step.
    let mut ed = Editor::new(Document::new());
    let (p, _) = plate(&mut ed);
    let id = ed.add_fill_at(v(1.0, 19.0)).unwrap();
    assert!(ed.move_point_to(p[3], v(20.0, 20.0)));
    assert!(seed_inside(&mut ed, id));
    ed.undo();
    assert_eq!(ed.doc.fill(id).unwrap().seed, v(1.0, 19.0));
}

/// An arrow of lines (a point on the left, a rectangle on the right), drawn in inches as
/// in the crash report, with its rectangle's right side at `x`; hatched.
fn hatched_arrow(ed: &mut Editor, x: f64) -> (Vec<EntityId>, Vec<EntityId>, FillId) {
    let (p, sides) = polygon(
        ed,
        &[
            v(x, 76.19999999999999),
            v(x - 228.6, 76.19999999999999),
            v(x - 330.2, 38.099999999999994),
            v(x - 228.6, 0.0),
            v(x, 0.0),
        ],
    );
    let id = ed.add_fill_at(v(x - 100.0, 38.1)).expect("a fill");
    (p, sides, id)
}

/// The points and curves of the sketch, by position.
fn geometry(ed: &Editor) -> Vec<(EntityId, DVec2)> {
    ed.doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Point(at) => Some((id, at)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_hatch_moved_over_another_leaves_the_other_outline_alone() {
    // Two hatched arrows, one shifted along the other so their top and bottom edges lie
    // along each other. Picking the copy's hatch and moving it used to bring the edges of
    // the other arrow lying under its own, stretching that arrow and joining the two.
    let mut ed = Editor::default();
    let (a, a_sides, _) = hatched_arrow(&mut ed, 0.0);
    ed.selection = a_sides.iter().copied().collect();
    ed.duplicate_selection();
    assert_eq!(ed.nudge(v(25.400000000000034, 0.0)), Some(true));
    let copy = *ed.selected_fills.iter().next().expect("the copy's hatch");
    let copy_sides: BTreeSet<EntityId> = ed.selection.iter().copied().collect();
    let still: Vec<DVec2> = a.iter().map(|p| ed.doc.point(*p).unwrap()).collect();

    ed.selection.clear();
    ed.selected_fills = [copy].into();
    let (curves, _) = ed.selected_fill_curves().expect("the hatch's curves");
    assert_eq!(curves.into_iter().collect::<BTreeSet<_>>(), copy_sides);
    // Drop it right on the other, then move it back off: the other arrow stays put.
    for d in [-25.4, 12.7, -12.7, 25.4] {
        let (curves, _) = ed.selected_fill_curves().expect("the hatch's curves");
        ed.selection = curves.into_iter().collect();
        assert_eq!(ed.nudge(v(d, 0.0)), Some(true));
        let now: Vec<DVec2> = a.iter().map(|p| ed.doc.point(*p).unwrap()).collect();
        assert_eq!(now, still, "the other arrow moved");
        found(&mut ed, copy);
    }
}

#[test]
fn moving_hatches_over_each_other_moves_only_their_own_curves() {
    // A deterministic sweep of offsets and moves over two hatched arrows.
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..40 {
        let mut ed = Editor::default();
        let (_, a_sides, a_fill) = hatched_arrow(&mut ed, 0.0);
        // Copies along the other, right on it, or a hair off it.
        let dx = [25.4, 0.0, 228.6, 101.6, 1.6e-5, 12.7][rnd(6) as usize];
        let dy = [0.0, 0.0, 38.1, 1e-9][rnd(4) as usize];
        ed.selection = a_sides.iter().copied().collect();
        ed.duplicate_selection();
        ed.nudge(v(dx, dy));
        let b_sides: BTreeSet<EntityId> = ed.selection.iter().copied().collect();
        let b_fill = *ed.selected_fills.iter().next().unwrap();
        for _ in 0..6 {
            let (fill, own, other): (FillId, BTreeSet<EntityId>, BTreeSet<EntityId>) =
                if rnd(2) == 0 {
                    (a_fill, a_sides.iter().copied().collect(), b_sides.clone())
                } else {
                    (b_fill, b_sides.clone(), a_sides.iter().copied().collect())
                };
            ed.selection.clear();
            ed.selected_fills = [fill].into();
            let Some((curves, _)) = ed.selected_fill_curves() else {
                continue;
            };
            assert!(
                curves.iter().all(|c| own.contains(c)),
                "{curves:?} are not all of {own:?}"
            );
            let before: Vec<_> = geometry(&ed)
                .into_iter()
                .filter(|(p, _)| ed.doc.defining_points(other.iter().copied()).contains(p))
                .collect();
            ed.selection = curves.into_iter().collect();
            let d = v(
                (rnd(9) as f64 - 4.0) * 12.7,
                [0.0, 0.0, 38.1, -38.1][rnd(4) as usize],
            );
            ed.nudge(d);
            let after: Vec<_> = geometry(&ed)
                .into_iter()
                .filter(|(p, _)| before.iter().any(|(q, _)| q == p))
                .collect();
            assert_eq!(after, before, "moving one hatch moved the other's outline");
            let _ = ed.fill_shapes();
        }
    }
}

/// A large import (thousands of curves, hundreds of fills, a lot of them crossed by long
/// grid lines) commits and finds its fills' areas in moments: each fill's area is looked up,
/// not found by going through every area, or every curve of the drawing (#84: a big PDF
/// took minutes).
#[test]
fn large_import_commits_quickly() {
    let import = crate::perf_tests::sheet_import(900, 2);
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    drift_io::dxf::add_to_document(&mut ed.doc, &import);
    let t = std::time::Instant::now();
    assert!(ed.commit_import("Import"));
    let shapes = ed.fill_shapes();
    let took = t.elapsed();
    assert_eq!(shapes.len(), 450);
    assert!(shapes.values().all(|s| !s.lost));
    // Fills the import made across its grid lines aren't flagged as divided.
    assert!(ed.doc.fills().all(|(_, f)| !f.divided));
    println!("large import committed in {took:?}");
    assert!(
        took < std::time::Duration::from_secs(10),
        "committing the import took {took:?}"
    );
}

/// A 100 × 100 square and, drawn on its own, a 50 × 50 square in its top right corner, so
/// the small square's right and top sides lie along the big square's (#120). Returns the
/// big square's corners and sides, then the small square's sides.
fn nested_squares(ed: &mut Editor) -> (Vec<EntityId>, Vec<EntityId>, Vec<EntityId>) {
    let (corners, big) = polygon(
        ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
    );
    let (_, small) = polygon(
        ed,
        &[
            v(50.0, 50.0),
            v(100.0, 50.0),
            v(100.0, 100.0),
            v(50.0, 100.0),
        ],
    );
    (corners, big, small)
}

fn sorted(ids: impl IntoIterator<Item = EntityId>) -> Vec<EntityId> {
    let mut ids: Vec<EntityId> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[test]
fn nested_squares_each_hatch_is_bounded_by_its_own_lines() {
    let mut ed = Editor::new(Document::new());
    let (corners, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    assert!(close(found(&mut ed, l_shape).0, 7500.0));
    assert!(close(found(&mut ed, corner).0, 2500.0));
    // The small square's hatch is bounded by the small square's own sides, not the big
    // square's lines its top and right lie along.
    assert_eq!(ed.doc.fill(corner).unwrap().boundary, sorted(small.clone()));
    // The L: the big square's sides and the small square's bottom and left.
    assert_eq!(
        ed.doc.fill(l_shape).unwrap().boundary,
        sorted(big.iter().copied().chain([small[0], small[3]]))
    );

    // Picking the small hatch brings only the small square, and moving it leaves the big
    // square where it is.
    ed.selection.clear();
    ed.selected_fills = [corner].into();
    let (curves, _) = ed.selected_fill_curves().expect("the hatch's curves");
    assert_eq!(sorted(curves.iter().copied()), sorted(small.clone()));
    let still: Vec<DVec2> = corners.iter().map(|p| ed.doc.point(*p).unwrap()).collect();
    ed.selection = curves.into_iter().collect();
    assert_eq!(ed.nudge(v(200.0, 0.0)), Some(true));
    let now: Vec<DVec2> = corners.iter().map(|p| ed.doc.point(*p).unwrap()).collect();
    assert_eq!(now, still, "moving the small hatch moved the big square");
    assert!(close(found(&mut ed, corner).0, 2500.0));
    // The big square, left on its own, is all the L's hatch's now.
    assert!(close(found(&mut ed, l_shape).0, 10_000.0));
}

#[test]
fn deleting_the_small_square_leaves_the_big_hatch_whole() {
    let mut ed = Editor::new(Document::new());
    let (_, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    ed.selected_fills.clear();
    ed.selection = small.iter().copied().collect();
    ed.delete_selection_with(&[]);
    // The small square's hatch goes with all of its outline; the L's hatch takes the area
    // its seed is in now, the whole big square, keeping its id and style.
    assert!(ed.doc.fill(corner).is_none());
    assert!(close(found(&mut ed, l_shape).0, 10_000.0));
    assert_eq!(ed.doc.fill(l_shape).unwrap().boundary, sorted(big));
    assert!(matches!(
        ed.doc.fill(l_shape).unwrap().kind,
        drift_core::FillKind::Hatch(_)
    ));
    // Undo brings both back as they were.
    assert!(ed.undo());
    assert!(close(found(&mut ed, l_shape).0, 7500.0));
    assert!(close(found(&mut ed, corner).0, 2500.0));
    assert!(ed.redo());
    assert!(close(found(&mut ed, l_shape).0, 10_000.0));
}

#[test]
fn deleting_part_of_an_outline_with_no_area_left_drops_the_fill() {
    let mut ed = Editor::new(Document::new());
    let (_, sides) = plate(&mut ed);
    let id = area_fill_at(&mut ed, v(5.0, 5.0));
    ed.selected_fills.clear();
    ed.selection = [sides[1]].into();
    ed.delete_selection_with(&[]);
    assert!(ed.doc.fill(id).is_none());
    assert!(ed.undo());
    assert!(close(found(&mut ed, id).0, 800.0));
}

/// The lower half of a circle, its diameter, and a lock: a curve from the arc's end back
/// onto the arc. The fill was made over the half disc and the lock as one area, as when the
/// lock's end on the arc wasn't a junction (#100), so its outline is the arc, the diameter and
/// the lock's curve; it is shown over the half disc. Returns the fill, the arc, the diameter
/// and the lock's curve.
fn fill_over_half_disc_and_lock(ed: &mut Editor) -> (FillId, EntityId, EntityId, EntityId) {
    ed.doc.begin();
    let p = |ed: &mut Editor, x: f64, y: f64| ed.doc.add_point(v(x, y));
    let (c, w, e) = (p(ed, 0.0, 0.0), p(ed, -100.0, 0.0), p(ed, 100.0, 0.0));
    let arc = ed.doc.add_arc(c, w, e).unwrap();
    let diameter = ed.doc.add_line(w, e).unwrap();
    let (c1, c2, tip) = (p(ed, 160.0, -20.0), p(ed, 150.0, -70.0), p(ed, 60.0, -80.0));
    let lock = ed.doc.add_bezier(e, c1, c2, tip).unwrap();
    let id = ed.doc.add_fill(drift_core::Fill::new(
        vec![arc, diameter, lock],
        v(0.0, -50.0),
        drift_core::FillKind::Solid,
    ));
    ed.commit("Shapes");
    assert!(close(found(ed, id).0, HALF_DISC));
    (id, arc, diameter, lock)
}

const HALF_DISC: f64 = PI * 5000.0;
/// The lock between the arc and the Bézier.
const LOCK: f64 = 2715.9;

#[test]
fn a_split_that_divides_a_fills_area_flags_it_and_keeps_it_whole() {
    // Splitting the arc where the lock meets it leaves the fill's outline closed around both
    // areas (#101): by default it is kept whole and flagged, and the notice says so.
    let mut ed = Editor::new(Document::new());
    let (id, arc, _, _) = fill_over_half_disc_and_lock(&mut ed);
    ed.notice = None;
    ed.split_at(arc, v(60.0, -80.0)).unwrap();
    let (area, _) = found(&mut ed, id);
    assert!((area - (HALF_DISC + LOCK)).abs() < 1.0, "{area}");
    assert!(ed.doc.fill(id).unwrap().divided);
    assert_eq!(ed.fill_shapes()[&id].parts, 2);
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Divided { id, parts: 2 }]
    );
    assert!(ed.notice.as_deref().is_some_and(|n| n.contains("2 areas")));
    // Undo takes the flag back with the split.
    assert!(ed.undo());
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert!(ed.redo());
    assert!(ed.doc.fill(id).unwrap().divided);

    // Split into one fill per area: the half disc keeps the fill.
    let fills = ed.split_fill(id).unwrap();
    assert_eq!(fills.len(), 2);
    assert_eq!(fills[0], id);
    assert!(close(found(&mut ed, id).0, HALF_DISC));
    assert!((found(&mut ed, fills[1]).0 - LOCK).abs() < 1.0);
    assert!(ed.doc.fills().all(|(_, f)| !f.divided));
    assert!(ed.undo());

    // Fit it to its seed's area.
    ed.fit_fill_to_seed(id).unwrap();
    assert!(close(found(&mut ed, id).0, HALF_DISC));
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert_eq!(ed.doc.fills().count(), 1);
    assert!(ed.undo());

    // Keep it whole: the flag goes, and a later edit doesn't raise it again.
    ed.keep_fill_whole(id);
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert!((found(&mut ed, id).0 - (HALF_DISC + LOCK)).abs() < 1.0);
    ed.fill_changes.clear();
    let far = ed.doc.add_point(v(500.0, 500.0));
    ed.doc.begin();
    ed.doc.set_point(far, v(510.0, 500.0)).unwrap();
    ed.commit("Move");
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert!(ed.fill_changes.is_empty());
}

#[test]
fn a_split_that_divides_a_fills_area_can_follow_its_seed() {
    // With "follow its seed" in Preferences, the fill takes the half disc around its seed.
    let mut ed = Editor::new(Document::new());
    ed.change_rules(|r| r.follow_divided_fills = true);
    let (id, arc, diameter, lock) = fill_over_half_disc_and_lock(&mut ed);
    ed.split_at(arc, v(60.0, -80.0)).unwrap();
    let (area, _) = found(&mut ed, id);
    assert!(close(area, HALF_DISC), "{area}");
    let fill = ed.doc.fill(id).unwrap();
    assert!(!fill.divided);
    assert!(fill.boundary.contains(&arc) && fill.boundary.contains(&diameter));
    assert!(!fill.boundary.contains(&lock), "{:?}", fill.boundary);
    assert!(seed_inside(&mut ed, id));
    assert!(matches!(
        ed.fill_changes[..],
        [crate::fills::FillChange::Refound { id: f, .. }] if f == id
    ));
}

#[test]
fn a_line_drawn_across_a_fill_is_flagged_and_kept_whole() {
    // Whichever way the divided-fill preference is set: a curve drawn across a fill never
    // changes its area (the preference is for edits that cut the outline itself).
    for follow in [false, true] {
        let mut ed = Editor::new(Document::new());
        ed.change_rules(|r| r.follow_divided_fills = follow);
        let (_, sides) = plate(&mut ed);
        let id = area_fill_at(&mut ed, v(5.0, 5.0));
        // A line right across it, past both sides.
        ed.doc.begin();
        let a = ed.doc.add_point(v(10.0, -5.0));
        let b = ed.doc.add_point(v(10.0, 25.0));
        let across = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        let (area, _) = found(&mut ed, id);
        assert!(close(area, 800.0), "follow {follow}: {area}");
        assert!(ed.doc.fill(id).unwrap().divided);
        assert_eq!(ed.fill_shapes()[&id].parts, 2);
        // Kept whole on purpose: splitting its side where the line crosses it changes
        // nothing more.
        ed.keep_fill_whole(id);
        ed.fill_changes.clear();
        ed.split_at(sides[0], v(10.0, 0.0)).unwrap();
        assert!(close(found(&mut ed, id).0, 800.0));
        assert!(!ed.doc.fill(id).unwrap().divided);
        assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
        // Taking the line away makes it one area again.
        ed.doc.begin();
        ed.doc.delete([across]);
        ed.commit("Delete");
        assert!(close(found(&mut ed, id).0, 800.0));
        assert!(!ed.doc.fill(id).unwrap().divided);
    }
}

#[test]
fn deleting_part_of_a_fills_outline_says_what_became_of_it() {
    let mut ed = Editor::new(Document::new());
    let (_, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    ed.notice = None;
    ed.selected_fills.clear();
    ed.selection = small.iter().copied().collect();
    ed.delete_selection_with(&[]);
    assert!(matches!(
        ed.fill_changes[..],
        [crate::fills::FillChange::Refound { id, after, .. }] if id == l_shape && close(after, 10_000.0)
    ));
    assert!(
        ed.notice
            .as_deref()
            .is_some_and(|n| n.contains("around its seed"))
    );
    // The big square's sides alone now bound it, a closed chain of free lines: it is that
    // square's shape (#202). A side deleted: nothing closes round it, so it keeps its last
    // shape, and says so.
    assert!(ed.shape_outline(l_shape).is_some());
    ed.fill_changes.clear();
    ed.selection = [big[0]].into();
    ed.delete_selection_with(&[]);
    assert!(ed.fill_shapes()[&l_shape].lost);
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Opened { id: l_shape }]
    );
}

/// Two filled circles touching, then a third circle drawn across both: each fill keeps its
/// whole disc, rather than shrinking to a part (filled lone circles are shapes, #202, so it
/// isn't flagged either).
#[test]
fn a_circle_drawn_across_two_filled_circles_leaves_them_whole() {
    for gap in [0.0, 20.0] {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        ed.doc.add_circle(a, 250.0).unwrap();
        let b = ed.doc.add_point(v(500.0 + gap, 0.0));
        ed.doc.add_circle(b, 250.0).unwrap();
        ed.commit("Circles");
        let fa = ed.add_fill_at(v(0.0, 0.0)).unwrap();
        let fb = ed.add_fill_at(v(500.0 + gap, 0.0)).unwrap();
        let disc = PI * 250.0 * 250.0;
        assert!(close(found(&mut ed, fa).0, disc));
        // The fills have been shown (so the editor knows they weren't crossed before).
        let _ = ed.fill_shapes();
        ed.doc.begin();
        let c = ed.doc.add_point(v(250.0 + gap / 2.0, 350.0));
        ed.doc.add_circle(c, 355.0).unwrap();
        ed.commit("Circle");
        let (area_a, _) = found(&mut ed, fa);
        let (area_b, _) = found(&mut ed, fb);
        assert!(
            close(area_a, disc),
            "gap {gap}: fill a now covers {area_a}, not {disc}"
        );
        assert!(
            close(area_b, disc),
            "gap {gap}: fill b now covers {area_b}, not {disc}"
        );
        // (Shapes, #202: a curve across a shape never changes or flags its fill.)
        assert!(!ed.doc.fill(fa).unwrap().divided);
        assert_eq!(ed.doc.fill(fa).unwrap().boundary.len(), 1);
    }
}

/// Two overlapping filled circles, trimmed to a peanut, then a circle drawn across the
/// peanut: the fill keeps the whole peanut (a shape, #202), whichever way the divided-fill
/// preference is set. With "Follow where it was first clicked", both fills shrank to the
/// part around their seeds (2026-10-21).
#[test]
fn a_circle_drawn_across_a_trimmed_peanut_leaves_its_fills_whole() {
    for (follow, seeds) in [
        (false, [v(-670.0, 600.0), v(-930.0, 600.0)]),
        (true, [v(-670.0, 600.0), v(-930.0, 600.0)]),
        (true, [v(-700.0, 650.0), v(-900.0, 650.0)]),
    ] {
        let mut ed = Editor::default();
        ed.change_rules(|r| r.follow_divided_fills = follow);
        ed.doc.begin();
        let a = ed.doc.add_point(v(-670.0, 600.0));
        let ca = ed.doc.add_circle(a, 150.0).unwrap();
        let b = ed.doc.add_point(v(-930.0, 600.0));
        let cb = ed.doc.add_circle(b, 150.0).unwrap();
        ed.commit("Circles");
        let fa = ed.add_fill_at(seeds[0]).unwrap();
        let fb = ed.add_fill_at(seeds[1]).unwrap();
        let _ = ed.fill_shapes();
        // Trim the inner arcs away: a peanut bounded by two 300-degree arcs, with one fill
        // (the fills came onto one area, and the one drawn on top kept it).
        ed.trim_at(ca, v(-800.0, 600.0)).unwrap();
        let _ = ed.fill_shapes();
        ed.trim_at(cb, v(-800.0, 600.0)).unwrap();
        assert!(ed.doc.fill(fa).is_none());
        let peanut = found(&mut ed, fb).0;
        assert!(!ed.doc.fill(fb).unwrap().divided);
        ed.fill_changes.clear();
        ed.doc.begin();
        let c = ed.doc.add_point(v(-800.0, 800.0));
        ed.doc.add_circle(c, 228.035085).unwrap();
        ed.commit("Circle");
        let f = fb;
        let (area, _) = found(&mut ed, f);
        assert!(
            close(area, peanut),
            "follow {follow} seeds {seeds:?}: fill {f} went from {peanut} to {area}"
        );
        // The peanut is a shape (#202): the circle across neither changes nor flags it.
        let fill = ed.doc.fill(f).unwrap();
        assert!(!fill.divided);
        assert!(fill.path.is_some());
        assert_eq!(fill.boundary.len(), 2, "its outline is still the two arcs");
        assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
    }
}

/// Two overlapping filled circles trimmed into each other (the user's second bug,
/// 2026-10-21): after the first trim each fill keeps its own part (one loses the lens, the
/// other keeps its whole disc); after the second the outline is one peanut, and the fill
/// drawn on top keeps it while the other is removed, with a notice. Undo brings it back.
#[test]
fn trimming_overlapping_filled_circles_leaves_one_fill_on_the_outline() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(-670.0, 600.0));
    let ca = ed.doc.add_circle(a, 150.0).unwrap();
    let b = ed.doc.add_point(v(-930.0, 600.0));
    let cb = ed.doc.add_circle(b, 150.0).unwrap();
    ed.commit("Circles");
    let fa = ed.add_fill_at(v(-670.0, 600.0)).unwrap();
    let fb = ed.add_fill_at(v(-930.0, 600.0)).unwrap();
    let _ = ed.fill_shapes();
    ed.trim_at(ca, v(-800.0, 600.0)).unwrap();
    let disc = PI * 150.0 * 150.0;
    let (area_a, _) = found(&mut ed, fa);
    assert!(
        area_a < disc && area_a > disc * 0.9,
        "{area_a}: a's crescent"
    );
    assert!(close(found(&mut ed, fb).0, disc), "b keeps its whole disc");
    assert_eq!(ed.doc.fill(fb).unwrap().seed, v(-930.0, 600.0));
    assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
    ed.notice = None;
    ed.trim_at(cb, v(-800.0, 600.0)).unwrap();
    assert!(ed.doc.fill(fa).is_none(), "the lower fill went");
    assert_eq!(ed.doc.fills().count(), 1);
    let (area, _) = found(&mut ed, fb);
    assert!(area > disc * 1.5, "{area}: the whole peanut");
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Merged { id: fa, into: fb }]
    );
    assert!(
        ed.notice.as_deref().is_some_and(|n| n.contains("removed")),
        "{:?}",
        ed.notice
    );
    assert!(ed.undo());
    assert_eq!(ed.doc.fills().count(), 2);
    assert!(close(found(&mut ed, fb).0, disc));
}

/// Deleting the wall between two filled rooms (#129): the merged room keeps one fill, the
/// one drawn on top, and the other is removed.
#[test]
fn deleting_the_wall_between_two_filled_rooms_leaves_one_fill() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let p: Vec<EntityId> = [
        (0.0, 0.0),
        (20.0, 0.0),
        (40.0, 0.0),
        (40.0, 10.0),
        (20.0, 10.0),
        (0.0, 10.0),
    ]
    .iter()
    .map(|&(x, y)| ed.doc.add_point(v(x, y)))
    .collect();
    for i in 0..6 {
        ed.doc.add_line(p[i], p[(i + 1) % 6]).unwrap();
    }
    let wall = ed.doc.add_line(p[1], p[4]).unwrap();
    ed.commit("Rooms");
    let left = ed.add_fill_at(v(10.0, 5.0)).unwrap();
    let right = ed.add_fill_at(v(30.0, 5.0)).unwrap();
    let _ = ed.fill_shapes();
    ed.doc.begin();
    ed.doc.delete([wall]);
    ed.commit("Delete");
    assert!(ed.doc.fill(left).is_none());
    assert_eq!(ed.doc.fills().count(), 1);
    assert!(close(found(&mut ed, right).0, 400.0));
    assert!(ed.fill_changes.contains(&crate::fills::FillChange::Merged {
        id: left,
        into: right
    }));
    assert!(ed.undo());
    assert_eq!(ed.doc.fills().count(), 2);
    assert!(close(found(&mut ed, left).0, 200.0));
}

/// A rectangle recorded as a shape (a path) with its fill: a line across it and a circle
/// inside it change nothing (no hole, no division); a split side and a fillet go into the
/// path; a deleted side opens it, so the fill takes the area left around it until undone.
#[test]
fn a_paths_fill_follows_its_own_outline_alone() {
    let mut ed = Editor::default();
    ed.fill_new_shapes = true;
    let (p, sides) = plate(&mut ed);
    ed.doc.begin();
    let path = ed.add_shape(&sides).unwrap();
    ed.commit("Shape");
    let (id, fill) = ed.doc.fills().next().map(|(i, f)| (i, f.clone())).unwrap();
    assert_eq!(fill.path, Some(path));
    assert!(close(found(&mut ed, id).0, 800.0));
    assert_eq!(ed.doc.path_of(sides[1]), Some(path));
    // A line across it doesn't cut it; a circle inside it is a hole.
    ed.doc.begin();
    let a = ed.doc.add_point(v(20.0, -5.0));
    let b = ed.doc.add_point(v(20.0, 25.0));
    ed.doc.add_line(a, b).unwrap();
    let c = ed.doc.add_point(v(10.0, 10.0));
    ed.doc.add_circle(c, 3.0).unwrap();
    ed.commit("Across and inside");
    let hole = PI * 9.0;
    assert!(close(found(&mut ed, id).0, 800.0 - hole));
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
    assert_eq!(ed.fill_shapes()[&id].region.holes.len(), 1);
    // Splitting a side puts the new piece in the path, in order.
    ed.split_at(sides[0], v(10.0, 0.0)).unwrap();
    let now = ed.doc.path(path).unwrap().clone();
    assert!(now.closed);
    assert_eq!(now.curves.len(), 5);
    assert_eq!(now.curves[0], sides[0]);
    assert_eq!(
        now.curves[2], sides[1],
        "the piece follows the side it was cut from"
    );
    assert!(close(found(&mut ed, id).0, 800.0 - hole));
    // A fillet too.
    ed.fillet(p[2], 5.0).unwrap();
    let now = ed.doc.path(path).unwrap().clone();
    assert!(now.closed);
    assert_eq!(now.curves.len(), 6);
    assert!(close(found(&mut ed, id).0, 800.0 - hole - CORNER_ROUND));
    assert_eq!(
        sorted(now.curves.clone()),
        ed.doc.fill(id).unwrap().boundary
    );
    // Deleting a side opens the path: the fill takes the area what is left of it and the
    // line across still close around its seed (#201), and both come back.
    ed.doc.begin();
    ed.doc.delete([sides[3]]);
    ed.commit("Delete");
    assert!(!ed.doc.path(path).unwrap().closed);
    assert!(
        close(found(&mut ed, id).0, 400.0 - CORNER_ROUND),
        "the right half"
    );
    assert_eq!(
        ed.doc.fill(id).unwrap().path,
        None,
        "an area fill now (#202)"
    );
    assert!(ed.undo());
    assert!(ed.doc.path(path).unwrap().closed);
    assert!(close(found(&mut ed, id).0, 800.0 - hole - CORNER_ROUND));
}

/// Two circles drawn as shapes with their fills, overlapping, trimmed into each other: the
/// first trim opens its path (its fill takes what is left closed around it); the second closes one path of both arcs,
/// with one fill, the topmost's.
#[test]
fn trimming_two_paths_into_one_outline_joins_them() {
    let mut ed = Editor::default();
    ed.fill_new_shapes = true;
    ed.doc.begin();
    let a = ed.doc.add_point(v(-670.0, 600.0));
    let ca = ed.doc.add_circle(a, 150.0).unwrap();
    let pa = ed.add_shape(&[ca]).unwrap();
    ed.commit("A");
    ed.doc.begin();
    let b = ed.doc.add_point(v(-930.0, 600.0));
    let cb = ed.doc.add_circle(b, 150.0).unwrap();
    let pb = ed.add_shape(&[cb]).unwrap();
    ed.commit("B");
    let fill_of = |ed: &Editor, p: drift_core::PathId| {
        ed.doc
            .fills()
            .find(|(_, f)| f.path == Some(p))
            .map(|(id, _)| id)
    };
    let (fa, fb) = (fill_of(&ed, pa).unwrap(), fill_of(&ed, pb).unwrap());
    let disc = PI * 150.0 * 150.0;
    assert!(close(found(&mut ed, fa).0, disc));
    assert!(close(found(&mut ed, fb).0, disc));
    ed.trim_at(ca, v(-800.0, 600.0)).unwrap();
    assert!(!ed.doc.path(pa).unwrap().closed);
    // Its fill takes what its arc and the other circle close around it (#201): its disc
    // less the lens the other covers.
    let lens = 2.0 * 150.0f64.powi(2) * (130.0f64 / 150.0).acos()
        - 130.0 * (4.0 * 150.0f64.powi(2) - 260.0f64.powi(2)).sqrt();
    assert!(close(found(&mut ed, fa).0, disc - lens));
    assert!(close(found(&mut ed, fb).0, disc));
    ed.notice = None;
    ed.trim_at(cb, v(-800.0, 600.0)).unwrap();
    let paths: Vec<(drift_core::PathId, drift_core::Path)> =
        ed.doc.paths().map(|(id, p)| (id, p.clone())).collect();
    assert_eq!(paths.len(), 1, "{paths:?}");
    let (pid, path) = &paths[0];
    assert!(path.closed);
    assert_eq!(sorted(path.curves.clone()), sorted(vec![ca, cb]));
    assert_eq!(ed.doc.fills().count(), 1);
    let (f, fill) = ed.doc.fills().next().map(|(i, f)| (i, f.clone())).unwrap();
    assert_eq!(
        (f, fill.path),
        (fb, Some(*pid)),
        "the topmost fill kept the shape"
    );
    assert!(found(&mut ed, f).0 > disc * 1.5, "the whole peanut");
    assert!(
        ed.fill_changes
            .contains(&crate::fills::FillChange::Merged { id: fa, into: fb })
    );
    assert!(ed.notice.as_deref().is_some_and(|n| n.contains("removed")));
    assert!(ed.undo());
    assert_eq!(ed.doc.paths().count(), 2);
    assert_eq!(ed.doc.fills().count(), 2);
}

/// Two filled squares overlapping at a corner, trimmed into each other one inner side at a
/// time. A square trimmed open doesn't take the other's sides into its path before the two
/// meet (which left the other's fill without its shape, and two fills stacked on the joined
/// outline): when they meet, one path holds both, with the top fill.
#[test]
fn squares_trimmed_open_one_side_at_a_time_join_with_the_top_fill() {
    let mut ed = Editor::default();
    ed.fill_new_shapes = true;
    let shape = |ed: &mut Editor, corners: &[DVec2]| {
        let (_, sides) = polygon(ed, corners);
        ed.doc.begin();
        let path = ed.add_shape(&sides).unwrap();
        ed.commit("Shape");
        let fill = ed
            .doc
            .fills()
            .find(|(_, f)| f.path == Some(path))
            .unwrap()
            .0;
        (path, fill)
    };
    let (pa, fa) = shape(
        &mut ed,
        &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 40.0), v(0.0, 40.0)],
    );
    let (pb, fb) = shape(
        &mut ed,
        &[v(20.0, 20.0), v(60.0, 20.0), v(60.0, 60.0), v(20.0, 60.0)],
    );
    let side_at = |ed: &Editor, p: DVec2| {
        ed.doc
            .iter()
            .find(|(id, _)| {
                ed.doc.shape(*id).is_some_and(|s| {
                    !matches!(s, drift_core::Shape::Point(_)) && drift_geom::distance(&s, p) < 1e-9
                })
            })
            .map(|(id, _)| id)
            .unwrap()
    };
    // A's right, B's left, A's top, B's bottom: each trim cuts only against the other square.
    for at in [v(40.0, 30.0), v(20.0, 30.0), v(30.0, 40.0), v(30.0, 20.0)] {
        let id = side_at(&ed, at);
        ed.trim_at(id, at).unwrap();
        // Until they meet, each path keeps its own sides (the fills of the shapes cut
        // open fill the area closed round them, and come onto one: the top one's).
        if ed.doc.paths().count() == 2 {
            let _ = (pa, pb, fa);
            assert_eq!(
                ed.doc.path(pa).unwrap().curves.len(),
                4,
                "A has only its own sides"
            );
        }
    }
    let paths: Vec<_> = ed.doc.paths().map(|(id, p)| (id, p.clone())).collect();
    assert_eq!(paths.len(), 1, "{paths:?}");
    assert!(paths[0].1.closed);
    assert_eq!(ed.doc.fills().count(), 1, "one fill on the joined outline");
    assert_eq!(
        ed.doc.fill(fb).unwrap().path,
        Some(paths[0].0),
        "the top one's"
    );
    assert!(close(found(&mut ed, fb).0, 2800.0));
}

/// A click with the Fill tool inside a shape drawn as one makes the fill the shape's: a
/// line across it leaves it whole, and re-picking its boundary makes it an area fill again.
#[test]
fn the_fill_tool_binds_to_the_path_it_clicks_in() {
    let mut ed = Editor::default();
    let (_, sides) = plate(&mut ed);
    ed.doc.begin();
    let path = ed.add_shape(&sides).unwrap();
    ed.commit("Shape");
    assert_eq!(ed.doc.fills().count(), 0, "the Fill switch was off");
    let id = ed.add_fill_at(v(5.0, 5.0)).unwrap();
    assert_eq!(ed.doc.fill(id).unwrap().path, Some(path));
    ed.doc.begin();
    let a = ed.doc.add_point(v(20.0, -5.0));
    let b = ed.doc.add_point(v(20.0, 25.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Across");
    assert!(close(found(&mut ed, id).0, 800.0));
    assert!(!ed.doc.fill(id).unwrap().divided);
    ed.repick_fill_at(id, v(5.0, 5.0)).unwrap();
    assert_eq!(ed.doc.fill(id).unwrap().path, None);
    assert!(close(found(&mut ed, id).0, 400.0), "the half the line left");
}

// --- Close-out of #120 and #101 (PR #128): the edges of "a fill is bounded by its own
// lines, and follows the area around its seed". ---------------------------------------

/// The curves a picked fill brings with it (what moving it moves).
fn fill_curves(ed: &mut Editor, id: FillId) -> Vec<EntityId> {
    ed.selection.clear();
    ed.selected_fills = [id].into();
    let (curves, _) = ed.selected_fill_curves().expect("the fill's curves");
    ed.selected_fills.clear();
    sorted(curves)
}

/// Points of `curves`, where they are now.
fn positions(ed: &Editor, curves: &[EntityId]) -> Vec<DVec2> {
    ed.doc
        .defining_points(curves.iter().copied())
        .into_iter()
        .map(|p| ed.doc.point(p).unwrap())
        .collect()
}

/// Move a fill by picking it, as on the canvas: its curves move, and nothing else should.
fn move_fill(ed: &mut Editor, id: FillId, by: DVec2) {
    let curves = fill_curves(ed, id);
    ed.selection = curves.into_iter().collect();
    assert_eq!(ed.nudge(by), Some(true));
    ed.selection.clear();
}

#[test]
fn staggered_rectangles_sharing_part_of_a_side_keep_their_own_lines() {
    // A on the bottom, B on top shifted right: A's top and B's bottom overlap for half
    // their length, and each is cut once by the other's corner, a tie for "fewest pieces".
    let mut ed = Editor::new(Document::new());
    let (_, a) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
    );
    let (_, b) = polygon(
        &mut ed,
        &[
            v(50.0, 100.0),
            v(150.0, 100.0),
            v(150.0, 200.0),
            v(50.0, 200.0),
        ],
    );
    let fa = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let fb = ed.add_fill_at(v(125.0, 175.0)).unwrap();
    assert!(close(found(&mut ed, fa).0, 10_000.0));
    assert!(close(found(&mut ed, fb).0, 10_000.0));
    assert!(
        fill_curves(&mut ed, fa).iter().all(|c| a.contains(c)),
        "A grabbed B's lines"
    );
    assert!(
        fill_curves(&mut ed, fb).iter().all(|c| b.contains(c)),
        "B grabbed A's lines"
    );
    // Moving either leaves the other where it is.
    let a_at = positions(&ed, &a);
    move_fill(&mut ed, fb, v(0.0, 300.0));
    assert_eq!(positions(&ed, &a), a_at, "moving B's hatch moved A");
    assert!(close(found(&mut ed, fa).0, 10_000.0));
    assert!(close(found(&mut ed, fb).0, 10_000.0));
}

#[test]
fn splitting_the_big_squares_side_inside_the_overlap_keeps_the_corner_hatch_its_own() {
    // The nested squares, then the big square's top split inside the stretch the small
    // square's top lies along. The big top's right piece is now uncut, and the small top is
    // cut at the split point: "fewest pieces" could hand the overlap back to the big square.
    let mut ed = Editor::new(Document::new());
    let (corners, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    ed.split_at(big[2], v(75.0, 100.0)).unwrap();
    assert!(close(found(&mut ed, l_shape).0, 7500.0));
    assert!(close(found(&mut ed, corner).0, 2500.0));
    let curves = fill_curves(&mut ed, corner);
    assert!(
        curves.iter().all(|c| small.contains(c)),
        "the corner hatch took the big square's lines again: {curves:?} vs {small:?}"
    );
    // So moving it leaves the big square whole.
    let still: Vec<DVec2> = corners.iter().map(|p| ed.doc.point(*p).unwrap()).collect();
    move_fill(&mut ed, corner, v(300.0, 0.0));
    let now: Vec<DVec2> = corners.iter().map(|p| ed.doc.point(*p).unwrap()).collect();
    assert_eq!(now, still, "moving the corner hatch moved the big square");
}

#[test]
fn a_rectangle_drawn_twice_keeps_its_fill_when_either_copy_goes() {
    for keep_first in [true, false] {
        let mut ed = Editor::new(Document::new());
        let square = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
        let (_, first) = polygon(&mut ed, &square);
        let (_, second) = polygon(&mut ed, &square);
        let id = area_fill_at(&mut ed, v(50.0, 50.0));
        assert!(close(found(&mut ed, id).0, 10_000.0));
        // Delete one copy: the other still encloses the seed.
        let (gone, kept) = if keep_first {
            (second.clone(), first.clone())
        } else {
            (first.clone(), second.clone())
        };
        ed.selection = gone.iter().copied().collect();
        ed.selected_fills.clear();
        ed.selected_fills.clear();
        ed.delete_selection_with(&[]);
        assert!(ed.doc.fill(id).is_some(), "the fill went with one copy");
        assert!(close(found(&mut ed, id).0, 10_000.0));
        assert!(fill_curves(&mut ed, id).iter().all(|c| kept.contains(c)));
        // The last copy taken away takes the fill with it.
        ed.selection = kept.iter().copied().collect();
        ed.selected_fills.clear();
        ed.selected_fills.clear();
        ed.delete_selection_with(&[]);
        assert!(ed.doc.fill(id).is_none());
    }
}

#[test]
fn deleting_the_big_square_keeps_the_corner_hatch() {
    let mut ed = Editor::new(Document::new());
    let (_, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    ed.selection = big.iter().copied().collect();
    ed.selected_fills.clear();
    ed.delete_selection_with(&[]);
    // The corner hatch is bounded by the small square's own sides, so it stays as it was.
    assert!(close(found(&mut ed, corner).0, 2500.0));
    assert_eq!(ed.doc.fill(corner).unwrap().boundary, sorted(small));
    // The L's seed is in no area now: it goes, said so, rather than moving onto the small
    // square the rest of its outline bounds (#135).
    assert!(ed.doc.fill(l_shape).is_none());
    assert!(
        ed.fill_changes
            .contains(&crate::fills::FillChange::Dropped { id: l_shape }),
        "{:?}",
        ed.fill_changes
    );
    assert!(
        ed.notice
            .as_deref()
            .is_some_and(|n| n.contains("no area is left around it")),
        "{:?}",
        ed.notice
    );
    assert_eq!(
        ed.doc.fills().count(),
        1,
        "nothing stacked on the corner hatch"
    );
    assert!(ed.undo());
    assert!(close(found(&mut ed, l_shape).0, 7500.0));
    assert!(close(found(&mut ed, corner).0, 2500.0));
}

#[test]
fn deleting_the_overlapped_big_side_keeps_the_corner_hatch() {
    // The big square's top, the line the corner hatch used to grab (#120), deleted alone.
    let mut ed = Editor::new(Document::new());
    let (_, big, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    ed.selection = [big[2]].into();
    ed.selected_fills.clear();
    ed.delete_selection_with(&[]);
    assert!(close(found(&mut ed, corner).0, 2500.0));
    assert!(
        fill_curves(&mut ed, corner)
            .iter()
            .all(|c| small.contains(c))
    );
    // The L is open at the top-left now: no area around its seed.
    assert!(ed.doc.fill(l_shape).is_none());
}

#[test]
fn removing_the_inner_square_of_a_ring_fills_the_whole_square() {
    // A square with a square hole that touches nothing: the ring is filled, and the hole.
    let mut ed = Editor::new(Document::new());
    polygon(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
    );
    let (_, inner) = polygon(
        &mut ed,
        &[v(40.0, 40.0), v(60.0, 40.0), v(60.0, 60.0), v(40.0, 60.0)],
    );
    let ring = ed.add_fill_at(v(10.0, 10.0)).unwrap();
    let hole = ed.add_fill_at(v(50.0, 50.0)).unwrap();
    assert!(close(found(&mut ed, ring).0, 9600.0));
    assert!(close(found(&mut ed, hole).0, 400.0));
    ed.selection = inner.iter().copied().collect();
    ed.selected_fills.clear();
    ed.delete_selection_with(&[]);
    assert!(ed.doc.fill(hole).is_none());
    assert!(
        close(found(&mut ed, ring).0, 10_000.0),
        "the ring kept its hole"
    );
}

#[test]
fn a_moved_hatch_stays_on_its_square_through_later_edits() {
    // The seed moves with its square, so a later edit that makes the fill look again for
    // the area around its seed finds the square where it is now, not where it was.
    let mut ed = Editor::new(Document::new());
    let (_, sides) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
    );
    let id = ed.add_fill_at(v(10.0, 10.0)).unwrap();
    move_fill(&mut ed, id, v(500.0, 0.0));
    assert!(seed_inside(&mut ed, id));
    ed.split_at(sides[0], v(550.0, 0.0)).unwrap();
    let (area, centroid) = found(&mut ed, id);
    assert!(close(area, 10_000.0));
    assert!((centroid - v(550.0, 50.0)).length() < 1e-6, "{centroid:?}");
    // And with another square drawn where it used to be, deleting a side elsewhere doesn't
    // send the fill back there.
    let (_, _old_place) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
    );
    let fills = ed.doc.fills().count();
    ed.selection = [sides[1]].into();
    ed.selected_fills.clear();
    ed.delete_selection_with(&[]);
    // (A shape: cut open with nothing closed round it, it keeps its shape, closed across the gap.)
    assert_eq!(ed.doc.fills().count(), fills);
    let shape = ed.fill_shapes()[&id].clone();
    assert!(shape.lost);
    assert!(
        (shape.region.centroid - v(550.0, 50.0)).length() < 1e-6,
        "the fill jumped to the square at its old place"
    );
}

#[test]
fn undoing_a_line_across_a_fill_clears_its_mark() {
    let mut ed = Editor::new(Document::new());
    plate(&mut ed);
    let id = area_fill_at(&mut ed, v(5.0, 5.0));
    ed.doc.begin();
    let a = ed.doc.add_point(v(10.0, -5.0));
    let b = ed.doc.add_point(v(10.0, 25.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    assert!(ed.doc.fill(id).unwrap().divided);
    assert!(ed.undo());
    assert!(!ed.doc.fill(id).unwrap().divided);
    assert!(ed.redo());
    assert!(ed.doc.fill(id).unwrap().divided);
}

#[test]
fn fills_and_their_marks_survive_save_and_load() {
    let mut ed = Editor::new(Document::new());
    let (_, _, small) = nested_squares(&mut ed);
    let l_shape = ed.add_fill_at(v(25.0, 25.0)).unwrap();
    let corner = ed.add_fill_at(v(75.0, 75.0)).unwrap();
    // A line across the L marks it as covering two areas.
    ed.doc.begin();
    let a = ed.doc.add_point(v(25.0, -5.0));
    let b = ed.doc.add_point(v(25.0, 40.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    let marked = ed.doc.fill(l_shape).unwrap().divided;
    let text = drift_io::save(&ed.doc, &drift_io::Metadata::default());
    let (doc, _) = drift_io::load(&text).expect("loads");
    let mut back = Editor::new(doc);
    assert_eq!(back.doc.fill(l_shape).unwrap().divided, marked);
    assert_eq!(back.doc.fill(corner).unwrap().boundary, sorted(small));
    assert!(close(found(&mut back, corner).0, 2500.0));
    assert!(close(
        found(&mut back, l_shape).0,
        found(&mut ed, l_shape).0
    ));
}

#[test]
fn deleting_the_wall_between_two_filled_areas_leaves_one_fill() {
    let mut ed = Editor::new(Document::new());
    let (_, sides) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(200.0, 0.0), v(200.0, 100.0), v(0.0, 100.0)],
    );
    ed.doc.begin();
    let a = ed.doc.add_point(v(100.0, 0.0));
    let b = ed.doc.add_point(v(100.0, 100.0));
    let wall = ed.doc.add_line(a, b).unwrap();
    ed.commit("Wall");
    let _ = sides;
    ed.add_fill_at(v(50.0, 50.0)).unwrap();
    ed.add_fill_at(v(150.0, 50.0)).unwrap();
    ed.selection = [wall].into();
    ed.selected_fills.clear();
    ed.delete_selection_with(&[]);
    assert_eq!(ed.doc.fills().count(), 1, "two fills stacked on one area");
}

/// Two filled shapes, one a shape drawn filled (with a path) and one an area filled with
/// the Fill tool, grouped as "Desk".
fn desk_group(ed: &mut Editor) -> (drift_core::GroupId, Vec<EntityId>) {
    let (_, top) = plate(ed);
    ed.fill_new_shapes = true;
    ed.doc.begin();
    ed.fill_new_shape(&top).unwrap();
    ed.commit("Shape");
    ed.fill_new_shapes = false;
    let (_, leg) = polygon(
        ed,
        &[v(50.0, 0.0), v(60.0, 0.0), v(60.0, 10.0), v(50.0, 10.0)],
    );
    ed.add_fill_at(v(55.0, 5.0)).unwrap();
    let curves: Vec<EntityId> = top.iter().chain(&leg).copied().collect();
    ed.selection = curves.iter().copied().collect();
    let g = ed.make_group().unwrap();
    ed.rename(crate::blocks::RenameTarget::Group(g), "Desk")
        .unwrap();
    (g, curves)
}

/// The fills' areas, smallest first.
fn fill_areas(ed: &mut Editor) -> Vec<f64> {
    let shapes = ed.fill_shapes();
    let mut areas: Vec<f64> = shapes
        .values()
        .inspect(|s| assert!(!s.lost, "a fill lost its area"))
        .map(|s| s.region.area)
        .collect();
    areas.sort_by(f64::total_cmp);
    areas
}

#[test]
fn a_group_becomes_a_block_with_its_fills_and_comes_back_as_a_group() {
    let mut ed = Editor::default();
    let (g, _) = desk_group(&mut ed);
    assert_eq!(ed.convertible_group(), Some(g));
    let copy = ed.block_from_group(g).unwrap();
    assert_eq!(ed.history.undo_label(), Some("Make block from group"));
    let i = *ed.doc.instance(copy).unwrap();
    let block = ed.doc.block(i.block).unwrap();
    assert_eq!(block.name, "Desk");
    assert_eq!(
        block.fills.len(),
        2,
        "the shapes' fills went into the block"
    );
    assert!(close(i.at.x, 30.0) && close(i.at.y, 10.0), "{:?}", i.at);
    assert!(ed.doc.group(g).is_none() && ed.doc.fills().count() == 0);
    let areas: Vec<f64> = ed.copy_fills().iter().map(|c| c.region.area).collect();
    assert_eq!(areas.len(), 2);
    assert_eq!(ed.selected_instances, [copy].into());
    // One step back: the group and its fills.
    ed.undo();
    assert_eq!(ed.doc.group(g).unwrap().name, "Desk");
    assert_eq!(fill_areas(&mut ed).len(), 2);
    assert_eq!(ed.doc.blocks().count(), 0);
    ed.redo();

    // And back: a group named after the block, carrying the fills, the shape still a shape.
    ed.selected_instances = [copy].into();
    let groups = ed.explode_to_groups();
    assert_eq!(ed.history.undo_label(), Some("Explode to group"));
    assert_eq!(groups.len(), 1);
    let group = ed.doc.group(groups[0]).unwrap().clone();
    assert_eq!(group.name, "Desk");
    assert_eq!(group.members.len(), 8, "the curves, not their points");
    assert_eq!(ed.selection, group.members);
    assert!(ed.doc.instance(copy).is_none());
    let areas = fill_areas(&mut ed);
    assert!(
        close(areas[0], 100.0) && close(areas[1], 800.0),
        "{areas:?}"
    );
    let shape = ed.doc.fills().find(|(_, f)| f.path.is_some());
    let (_, shape) = shape.expect("the shape drawn filled is a shape again");
    let path = ed.doc.path(shape.path.unwrap()).unwrap();
    assert!(path.closed && path.curves.len() == 4);
    assert!(
        ed.doc
            .fills()
            .all(|(_, f)| f.boundary.iter().all(|e| group.members.contains(e)))
    );
    ed.undo();
    assert!(ed.doc.instance(copy).is_some() && ed.doc.groups().count() == 0);
    assert_eq!(ed.doc.fills().count(), 0);
}

#[test]
fn blocks_from_groups_take_free_names_and_copies_explode_to_a_group_each() {
    let mut ed = Editor::default();
    let (g, _) = desk_group(&mut ed);
    let first = ed.block_from_group(g).unwrap();
    let block = ed.doc.instance(first).unwrap().block;
    // A second group of the same name: the block name is taken.
    ed.place_instance(block, v(0.0, 100.0), 90.0, false);
    ed.explode_to_groups();
    let again = ed.doc.groups().next().unwrap().0;
    let second = ed.block_from_group(again).unwrap();
    let second_block = ed.doc.instance(second).unwrap().block;
    assert_eq!(ed.doc.block(second_block).unwrap().name, "Desk 2");
    assert_eq!(ed.doc.block(second_block).unwrap().fills.len(), 2);
    // Several copies at once: one group each, one undo step.
    ed.place_instance(block, v(200.0, 0.0), 0.0, true);
    ed.selected_instances = ed.doc.instances().map(|(id, _)| id).collect();
    let groups = ed.explode_to_groups();
    assert_eq!(groups.len(), 3);
    assert_eq!(fill_areas(&mut ed).len(), 6);
    let mut names: Vec<String> = groups
        .iter()
        .map(|g| ed.doc.group(*g).unwrap().name.clone())
        .collect();
    names.sort();
    assert_eq!(names, ["Desk", "Desk", "Desk 2"]);
    ed.undo();
    assert_eq!(ed.doc.instances().count(), 3);
    assert_eq!(ed.doc.groups().count(), 0);
}

#[test]
fn only_one_whole_group_converts() {
    let mut ed = Editor::default();
    let (g, curves) = desk_group(&mut ed);
    // Part of a group, or two groups (groups don't nest), isn't a group to convert.
    ed.selection = curves[..2].iter().copied().collect();
    assert_eq!(ed.convertible_group(), None);
    let (_, other) = polygon(
        &mut ed,
        &[v(0.0, 50.0), v(10.0, 50.0), v(10.0, 60.0), v(0.0, 60.0)],
    );
    ed.selection = other.iter().copied().collect();
    ed.make_group().unwrap();
    ed.selection = curves.iter().chain(&other).copied().collect();
    assert_eq!(ed.convertible_group(), None);
    // The group being edited, with nothing picked in it, converts.
    ed.selection.clear();
    ed.open_group = Some(g);
    assert_eq!(ed.convertible_group(), Some(g));
    ed.block_from_group(g).unwrap();
    assert_eq!(ed.open_group, None);
    assert_eq!(ed.doc.groups().count(), 1, "the other group stays");
}

#[test]
fn a_duplicated_group_brings_its_fills() {
    let mut ed = Editor::default();
    let (g, _) = desk_group(&mut ed);
    ed.selection = ed.doc.group(g).unwrap().members.clone();
    assert!(ed.duplicate_selection() > 0);
    let areas = fill_areas(&mut ed);
    assert_eq!(areas.len(), 4, "{areas:?}");
}

/// During a drag a click-fill keeps the area found before unless the drag moves its own
/// outline (finding the drawing's areas each frame is too slow on big drawings); it's found
/// again at the commit, and undo and redo bring back the right areas.
#[test]
fn click_fills_keep_their_areas_during_a_drag_and_refind_them_after() {
    let mut ed = Editor::default();
    let (p, _) = plate(&mut ed);
    let (q, _) = polygon(
        &mut ed,
        &[v(100.0, 0.0), v(120.0, 0.0), v(120.0, 10.0), v(100.0, 10.0)],
    );
    let plate_fill = ed.add_fill_at(v(2.0, 2.0)).unwrap();
    let other = ed.add_fill_at(v(110.0, 5.0)).unwrap();
    assert!(close(found(&mut ed, plate_fill).0, 800.0));
    assert!(close(found(&mut ed, other).0, 200.0));

    // Drag the plate's corner (5 a frame): the plate's fill follows each frame, the other
    // is kept.
    ed.doc.begin();
    for k in 1..=3 {
        assert!(ed.drag(&[p[2]], v(5.0, 0.0), &[]));
        ed.sync();
        let shapes = ed.fill_shapes();
        let grown = 800.0 + 0.5 * 5.0 * k as f64 * 20.0;
        assert!(close(shapes[&plate_fill].region.area, grown), "frame {k}");
        assert!(close(shapes[&other].region.area, 200.0));
    }
    // A curve added in the drag's transaction is a change of structure: the other fill is
    // found again (cut in two by the new line, it keeps its whole area).
    let a = ed.doc.add_point(v(110.0, -5.0));
    let b = ed.doc.add_point(v(110.0, 15.0));
    ed.doc.add_line(a, b).unwrap();
    assert!(close(found(&mut ed, other).0, 200.0));
    assert!(ed.commit("Drag"));
    assert!(close(found(&mut ed, plate_fill).0, 950.0));
    assert!(close(found(&mut ed, other).0, 200.0));

    // Drag the other outline's corner (outside the plate): only its fill changes.
    ed.doc.begin();
    assert!(ed.drag(&[q[2]], v(0.0, 10.0), &[]));
    ed.sync();
    assert!(close(found(&mut ed, plate_fill).0, 950.0));
    assert!(ed.commit("Drag"));
    let moved = found(&mut ed, other).0;
    assert!(
        moved > 200.0 + 1.0,
        "the other fill follows its outline: {moved}"
    );
    assert!(ed.undo());
    assert!(close(found(&mut ed, other).0, 200.0));
    assert!(ed.undo());
    assert!(close(found(&mut ed, plate_fill).0, 800.0));
    assert!(ed.redo());
    assert!(close(found(&mut ed, plate_fill).0, 950.0));
}

/// #201: a ring (a circle inside another, lower, not concentric) filled with the Fill tool,
/// and two lines down across its bottom. Returns the fill and the outer and inner circles.
fn filled_ring_and_two_lines(ed: &mut Editor, shapes: bool) -> (FillId, EntityId, EntityId) {
    ed.doc.begin();
    let o = ed.doc.add_point(v(0.0, 0.0));
    let outer = ed.doc.add_circle(o, 30.0).unwrap();
    let i = ed.doc.add_point(v(0.0, -8.0));
    let inner = ed.doc.add_circle(i, 15.0).unwrap();
    if shapes {
        // Drawn with the Circle tool: each is a shape.
        ed.add_shape(&[outer]);
        ed.add_shape(&[inner]);
    }
    ed.commit("Circles");
    let id = ed.add_fill_at(v(0.0, 25.0)).unwrap();
    // A lone circle is a shape whether drawn as one or not (#202).
    assert!(ed.doc.fill(id).unwrap().path.is_some());
    ed.doc.begin();
    for x in [-5.0, 5.0] {
        let a = ed.doc.add_point(v(x, -40.0));
        let b = ed.doc.add_point(v(x, -10.0));
        ed.doc.add_line(a, b).unwrap();
    }
    ed.commit("Lines");
    assert!(
        close(found(ed, id).0, RING),
        "the whole ring, across the lines"
    );
    (id, outer, inner)
}

const RING: f64 = PI * (30.0 * 30.0 - 15.0 * 15.0);
/// The ring less the slot between the lines.
const CUT_RING: f64 = 2049.1438;

/// The fill covers the ring but not the slot between the lines nor the inner circle's disc.
fn is_cut_ring(ed: &mut Editor, id: FillId) {
    let (area, _) = found(ed, id);
    assert!(close(area, CUT_RING), "{area}: the ring less the slot");
    let region = ed.fill_shapes()[&id].region.clone();
    assert!(region.contains(v(0.0, 25.0)) && region.contains(v(-20.0, -10.0)));
    assert!(!region.contains(v(0.0, -26.0)), "the slot cut out is empty");
    assert!(!region.contains(v(0.0, -8.0)), "the inner disc is empty");
}

/// #201: trimming the ring's two circles between the lines (in either order, as a drag
/// across them does) leaves a fill cut like the ring, never a disc. Whether the circles are
/// shapes or loose curves.
#[test]
fn trimming_a_slot_out_of_a_filled_ring_cuts_the_fill_too() {
    for shapes in [false, true] {
        for inner_first in [false, true] {
            let mut ed = Editor::new(Document::new());
            let (id, outer, inner) = filled_ring_and_two_lines(&mut ed, shapes);
            let trims = [(outer, v(0.0, -30.0)), (inner, v(0.0, -23.0))];
            let order = if inner_first {
                [trims[1], trims[0]]
            } else {
                trims
            };
            for (curve, at) in order {
                ed.trim_at(curve, at).unwrap();
            }
            assert!(
                matches!(ed.doc.shape(outer), Some(drift_core::Shape::Arc { .. }))
                    && matches!(ed.doc.shape(inner), Some(drift_core::Shape::Arc { .. }))
            );
            is_cut_ring(&mut ed, id);
            assert!(
                seed_inside(&mut ed, id),
                "shapes {shapes}, inner first {inner_first}"
            );
            // Both trims undo and redo with it.
            ed.undo();
            ed.undo();
            assert!(close(found(&mut ed, id).0, RING));
            ed.redo();
            ed.redo();
            is_cut_ring(&mut ed, id);
        }
    }
}

/// #201: a fill whose hole is trimmed open doesn't take the hole in: it takes the part of
/// its area still apart from the hole, whether it is an area's fill or a shape's. A shape's
/// keeps that area through later edits while it lasts, and fills its own outline again once
/// it is gone.
#[test]
fn a_hole_trimmed_open_stays_out_of_its_fill() {
    for shapes in [false, true] {
        let mut ed = Editor::new(Document::new());
        let (id, outer, inner) = filled_ring_and_two_lines(&mut ed, shapes);
        ed.notice = None;
        ed.trim_at(inner, v(0.0, -23.0)).unwrap();
        is_cut_ring(&mut ed, id);
        if !shapes {
            continue;
        }
        assert!(
            ed.notice
                .as_deref()
                .is_some_and(|n| n.contains("area around its seed")),
            "{:?}",
            ed.notice
        );
        // An unrelated edit: it keeps the area.
        ed.doc.begin();
        let p = ed.doc.add_point(v(50.0, 50.0));
        let q = ed.doc.add_point(v(60.0, 50.0));
        ed.doc.add_line(p, q).unwrap();
        ed.commit("Line");
        is_cut_ring(&mut ed, id);
        // Undone, it is the ring again, the inside of its own outline.
        ed.undo();
        ed.undo();
        assert!(close(found(&mut ed, id).0, RING));
        assert_eq!(ed.doc.fill(id).unwrap().boundary, vec![outer]);
    }
}

/// A shape trimmed open with no area left around its fill keeps its shape, closed straight
/// across the gap, and the notice says so; saved and opened again, it shows the same.
#[test]
fn a_shape_cut_open_with_no_area_left_keeps_its_fill_and_says_so() {
    let mut ed = Editor::new(Document::new());
    let (_, sides) = plate(&mut ed);
    ed.doc.begin();
    ed.fill_new_shapes = true;
    let id = ed.fill_new_shape(&sides).unwrap();
    ed.commit("Rectangle");
    assert!(close(found(&mut ed, id).0, 800.0));
    ed.notice = None;
    // Nothing cuts the side: the whole of it goes.
    ed.trim_at(sides[1], v(40.0, 10.0)).unwrap();
    assert!(!ed.doc.contains(sides[1]));
    let shape = ed.fill_shapes()[&id].clone();
    assert!(
        shape.lost && close(shape.region.area, 800.0),
        "its last shape"
    );
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Opened { id }]
    );
    assert!(
        ed.notice
            .as_deref()
            .is_some_and(|n| n.contains("keeps its shape")),
        "{:?}",
        ed.notice
    );
    // Saved and opened again: found from the file alone, the same.
    let text = drift_io::save(&ed.doc, &Default::default());
    let (doc, _) = drift_io::load(&text).unwrap();
    let mut opened = Editor::new(doc);
    let again = opened.fill_shapes()[&id].clone();
    assert!(
        again.lost && close(again.region.area, 800.0),
        "after reopening"
    );
}

/// #201, with nothing drawn between edits (the assistant's edits, or the first edit after a
/// drawing is opened): a shape fill's hole trimmed open still stays out of it.
#[test]
fn a_hole_trimmed_open_stays_out_with_no_frame_drawn() {
    let mut ed = Editor::new(Document::new());
    let (id, _, inner) = filled_ring_and_two_lines(&mut ed, true);
    // As just opened: nothing known of its holes or its last shape.
    ed.shape_holes.clear();
    ed.last_fill_shapes.clear();
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    is_cut_ring(&mut ed, id);
}

/// A rectangle drawn as a shape, filled, with a circle drawn as a shape inside it (a hole),
/// and a line far off. Returns the fill, the circle's centre and the line.
fn plate_with_a_hole(ed: &mut Editor) -> (FillId, EntityId, EntityId) {
    let (_, sides) = plate(ed);
    ed.fill_new_shapes = true;
    ed.doc.begin();
    let id = ed.fill_new_shape(&sides).unwrap();
    ed.commit("Rectangle");
    ed.fill_new_shapes = false;
    ed.doc.begin();
    let c = ed.doc.add_point(v(30.0, 10.0));
    let circle = ed.doc.add_circle(c, 5.0).unwrap();
    ed.add_shape(&[circle]);
    let a = ed.doc.add_point(v(500.0, 0.0));
    let b = ed.doc.add_point(v(520.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Hole and line");
    assert!(close(found(ed, id).0, 800.0 - PI * 25.0));
    (id, c, line)
}

/// Review of #201: an edit far from a shape with a hole doesn't make the commit find the
/// drawing's areas (a drawing of shapes alone never needs them).
#[test]
fn an_edit_far_from_a_shape_with_a_hole_finds_no_areas() {
    let mut ed = Editor::new(Document::new());
    let (_, _, line) = plate_with_a_hole(&mut ed);
    ed.regions = None;
    // Split the far line: an edit that reshapes a curve.
    ed.split_at(line, v(510.0, 0.0)).unwrap();
    assert!(ed.regions.is_none(), "the drawing's areas were found");
}

/// Review of #201: a hole moved across its shape's outline isn't cut open: the shape's fill
/// stays the inside of its own outline, whatever crosses it.
#[test]
fn a_hole_moved_across_the_outline_leaves_the_shapes_fill_whole() {
    let mut ed = Editor::new(Document::new());
    let (id, c, _) = plate_with_a_hole(&mut ed);
    ed.fill_changes.clear();
    ed.doc.begin();
    ed.doc.translate([c], v(10.0, 0.0));
    ed.commit("Move");
    assert!(close(found(&mut ed, id).0, 800.0), "the whole rectangle");
    assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
    assert!(ed.shape_outline(id).is_some(), "still a shape");
}

/// Review of #201: the cut ring is an area fill (#202); once its area is gone too (the lines
/// deleted), it goes as an area fill does, and the notice says so.
#[test]
fn a_cut_rings_area_gone_drops_it_and_says_so() {
    let mut ed = Editor::new(Document::new());
    let (id, outer, inner) = filled_ring_and_two_lines(&mut ed, true);
    ed.trim_at(outer, v(0.0, -30.0)).unwrap();
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    is_cut_ring(&mut ed, id);
    let lines: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(e, _)| e)
        .collect();
    assert_eq!(lines.len(), 2);
    ed.fill_changes.clear();
    ed.notice = None;
    ed.doc.begin();
    ed.doc.delete(lines);
    ed.commit("Delete");
    assert!(
        ed.doc.fill(id).is_none(),
        "an area fill with no area left goes"
    );
    assert_eq!(
        ed.fill_changes,
        vec![crate::fills::FillChange::Dropped { id }]
    );
    assert!(
        ed.notice
            .as_deref()
            .is_some_and(|n| n.contains("no area is left")),
        "{:?}",
        ed.notice
    );
    // Another edit doesn't say it again.
    ed.fill_changes.clear();
    ed.doc.begin();
    let p = ed.doc.add_point(v(80.0, 80.0));
    let q = ed.doc.add_point(v(90.0, 80.0));
    ed.doc.add_line(p, q).unwrap();
    ed.commit("Line");
    assert!(ed.fill_changes.is_empty(), "{:?}", ed.fill_changes);
}

/// Review of #201: a shape whose fill took an area (a hole of it trimmed open) picks,
/// moves and copies as an area fill: it isn't half a shape.
#[test]
fn a_shape_fill_that_took_an_area_is_not_a_shape() {
    let mut ed = Editor::new(Document::new());
    let (id, outer, inner) = filled_ring_and_two_lines(&mut ed, true);
    assert_eq!(ed.shape_outline(id), Some(vec![outer]));
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    is_cut_ring(&mut ed, id);
    assert_eq!(ed.shape_outline(id), None);
}

/// #202: a fill made in a closed chain of free lines makes it a shape (A′): its outline is
/// a path, it picks as one, and a line drawn across it leaves it whole.
#[test]
fn a_fill_in_a_loop_of_free_lines_makes_a_shape() {
    let mut ed = Editor::new(Document::new());
    let (_, sides) = plate(&mut ed);
    let id = ed.add_fill_at(v(5.0, 5.0)).unwrap();
    let path = ed.doc.fill(id).unwrap().path.expect("a shape");
    assert_eq!(
        sorted(ed.doc.path(path).unwrap().curves.clone()),
        sorted(sides.clone())
    );
    assert!(ed.doc.path(path).unwrap().closed);
    assert_eq!(ed.fill_kind(id), Ok(()));
    assert_eq!(sorted(ed.shape_outline(id).unwrap()), sorted(sides));
    ed.doc.begin();
    let a = ed.doc.add_point(v(10.0, -5.0));
    let b = ed.doc.add_point(v(10.0, 25.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    assert!(close(found(&mut ed, id).0, 800.0));
    assert!(!ed.doc.fill(id).unwrap().divided);
}

/// #202: outlines that can't be shapes stay area fills, and say why: rooms sharing a wall,
/// lines running past the corners, and corners drawn up to each other but not joined (Join
/// corners makes that one a shape).
#[test]
fn area_fills_say_why_they_are_not_shapes() {
    // Two rooms sharing a wall.
    let mut ed = Editor::new(Document::new());
    let (p, _) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(80.0, 0.0), v(80.0, 40.0), v(0.0, 40.0)],
    );
    ed.doc.begin();
    let m0 = ed.doc.add_point(v(40.0, 0.0));
    let m1 = ed.doc.add_point(v(40.0, 40.0));
    ed.doc.add_line(m0, m1).unwrap();
    ed.commit("Wall");
    let _ = p;
    let left = ed.add_fill_at(v(10.0, 10.0)).unwrap();
    let right = ed.add_fill_at(v(70.0, 10.0)).unwrap();
    for f in [left, right] {
        assert!(ed.doc.fill(f).unwrap().path.is_none());
        assert!(matches!(
            ed.fill_kind(f),
            Err(crate::fills::NotAShape::Crossing | crate::fills::NotAShape::Shared(_))
        ));
    }
    // Lines past the corners.
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    for (a, b) in [
        (v(-5.0, 0.0), v(45.0, 0.0)),
        (v(-5.0, 20.0), v(45.0, 20.0)),
        (v(0.0, -5.0), v(0.0, 25.0)),
        (v(40.0, -5.0), v(40.0, 25.0)),
    ] {
        let a = ed.doc.add_point(a);
        let b = ed.doc.add_point(b);
        ed.doc.add_line(a, b).unwrap();
    }
    ed.commit("Lines");
    let id = ed.add_fill_at(v(5.0, 5.0)).unwrap();
    assert_eq!(ed.fill_kind(id), Err(crate::fills::NotAShape::Crossing));
    assert!(
        ed.fill_kind(id)
            .unwrap_err()
            .reason()
            .contains("lines run past it")
    );
    // Corners drawn up to each other, each line with its own end points.
    let mut ed = Editor::new(Document::new());
    let corners = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 20.0), v(0.0, 20.0)];
    ed.doc.begin();
    let lines: Vec<EntityId> = (0..4)
        .map(|i| {
            let a = ed.doc.add_point(corners[i]);
            let b = ed.doc.add_point(corners[(i + 1) % 4]);
            ed.doc.add_line(a, b).unwrap()
        })
        .collect();
    ed.commit("Lines");
    let id = ed.add_fill_at(v(5.0, 5.0)).unwrap();
    let Err(crate::fills::NotAShape::Corners(pairs)) = ed.fill_kind(id) else {
        panic!("{:?}", ed.fill_kind(id));
    };
    assert_eq!(pairs.len(), 4);
    assert_eq!(ed.join_corners(id), Ok(4));
    assert_eq!(ed.fill_kind(id), Ok(()), "a shape now");
    assert_eq!(sorted(ed.shape_outline(id).unwrap()), sorted(lines.clone()));
    assert!(close(found(&mut ed, id).0, 800.0));
    // One undo step.
    assert!(ed.undo());
    assert!(matches!(
        ed.fill_kind(id),
        Err(crate::fills::NotAShape::Corners(_))
    ));
}

/// #202: opening a drawing makes no shapes: an area fill whose outline would make one stays
/// an area fill, says so, and becomes a shape when an edit touches it.
#[test]
fn opening_a_drawing_makes_no_shapes() {
    let mut doc = Document::new();
    let p =
        [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)].map(|(x, y)| doc.add_point(v(x, y)));
    let mut sides: Vec<EntityId> = (0..4)
        .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    sides.sort_unstable();
    let id = doc.add_fill(drift_core::Fill::new(
        sides.clone(),
        v(5.0, 5.0),
        drift_core::FillKind::Solid,
    ));
    let text = drift_io::save(&doc, &Default::default());
    let (doc, _) = drift_io::load(&text).unwrap();
    let mut ed = Editor::new(doc);
    assert!(close(found(&mut ed, id).0, 800.0));
    assert!(ed.doc.fill(id).unwrap().path.is_none());
    assert_eq!(ed.fill_kind(id), Err(crate::fills::NotAShape::NotEdited));
    // Moved: an edit touched it.
    ed.doc.begin();
    ed.doc.translate(sides.clone(), v(10.0, 0.0));
    ed.commit("Move");
    assert!(ed.doc.fill(id).unwrap().path.is_some());
    assert_eq!(ed.fill_kind(id), Ok(()));
}

/// #201 with #202: the cut ring is an area fill, saved and opened again as it was (no state
/// of its own beyond the file).
#[test]
fn a_cut_ring_saves_and_opens_as_it_was() {
    let mut ed = Editor::new(Document::new());
    let (id, outer, inner) = filled_ring_and_two_lines(&mut ed, true);
    ed.trim_at(outer, v(0.0, -30.0)).unwrap();
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    is_cut_ring(&mut ed, id);
    assert!(ed.doc.fill(id).unwrap().path.is_none());
    let text = drift_io::save(&ed.doc, &Default::default());
    let (doc, _) = drift_io::load(&text).unwrap();
    let mut opened = Editor::new(doc);
    is_cut_ring(&mut opened, id);
    let (was, now) = (ed.doc.fill(id).unwrap(), opened.doc.fill(id).unwrap());
    assert_eq!(
        (&now.boundary, now.seed, now.path),
        (&was.boundary, was.seed, was.path)
    );
}

/// Review of the #202 rework: a hole trimmed open, undone, and trimmed again with no frame
/// drawn in between (the assistant's undo then trim) still makes the cut ring, not the disc.
#[test]
fn a_hole_trimmed_again_after_undo_with_no_frame_drawn() {
    let mut ed = Editor::new(Document::new());
    let (id, _, inner) = filled_ring_and_two_lines(&mut ed, true);
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    is_cut_ring(&mut ed, id);
    assert!(ed.undo());
    assert!(ed.doc.fill(id).unwrap().path.is_some());
    ed.trim_at(inner, v(0.0, -23.0)).unwrap();
    assert!(ed.doc.fill(id).unwrap().path.is_none());
    is_cut_ring(&mut ed, id);
}

/// Review of the #202 rework: a shape cut open keeps its own fill when its outline closes
/// again; an area fill of part of it (a strip beside a line across it) stays that strip.
#[test]
fn a_shape_closed_again_keeps_its_own_fill() {
    let mut ed = Editor::new(Document::new());
    ed.fill_new_shapes = true;
    let (corners, sides) = polygon(
        &mut ed,
        &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 40.0), v(0.0, 40.0)],
    );
    ed.doc.begin();
    let path = ed.add_shape(&sides).unwrap();
    ed.commit("Shape");
    let own = ed
        .doc
        .fills()
        .find(|(_, f)| f.path == Some(path))
        .unwrap()
        .0;
    ed.doc.begin();
    let a = ed.doc.add_point(v(15.0, -5.0));
    let b = ed.doc.add_point(v(15.0, 45.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Across");
    let strip = area_fill_at(&mut ed, v(5.0, 5.0));
    assert!(close(found(&mut ed, strip).0, 600.0));
    ed.doc.begin();
    ed.doc.delete([sides[1]]);
    ed.commit("Delete");
    ed.doc.begin();
    ed.doc.add_line(corners[1], corners[2]).unwrap();
    ed.commit("Close");
    assert_eq!(ed.doc.fill(own).unwrap().path, Some(path), "its own fill");
    assert!(close(found(&mut ed, own).0, 1600.0));
    assert!(ed.doc.fill(strip).unwrap().path.is_none());
    assert!(
        close(found(&mut ed, strip).0, 600.0),
        "the strip stays a strip"
    );
}

/// Review of the #202 rework: restyling an area fill from an older drawing doesn't make it a
/// shape (only edits to its outline do).
#[test]
fn restyling_an_old_area_fill_leaves_it_an_area_fill() {
    let mut ed = Editor::new(Document::new());
    plate(&mut ed);
    let id = area_fill_at(&mut ed, v(5.0, 5.0));
    let mut fill = ed.doc.fill(id).unwrap().clone();
    fill.opacity = 0.5;
    ed.update_fill(id, fill);
    assert!(ed.doc.fill(id).unwrap().path.is_none());
    assert_eq!(ed.fill_kind(id), Err(crate::fills::NotAShape::NotEdited));
}

/// Review of the #202 rework: a drawing of 1000 filled rectangles, all moved at once (which
/// makes them all shapes), commits in moments, and so do later edits: shapes the edit didn't
/// touch aren't found again, and nothing is found over the whole drawing per shape.
#[test]
fn many_filled_rectangles_made_shapes_commit_quickly() {
    let mut doc = Document::new();
    for k in 0..1000 {
        let (x0, y0) = ((k % 40) as f64 * 50.0, (k / 40) as f64 * 50.0);
        let p = [
            (x0, y0),
            (x0 + 40.0, y0),
            (x0 + 40.0, y0 + 20.0),
            (x0, y0 + 20.0),
        ]
        .map(|(x, y)| doc.add_point(v(x, y)));
        let mut sides: Vec<EntityId> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        sides.sort_unstable();
        doc.add_fill(drift_core::Fill::new(
            sides,
            v(x0 + 5.0, y0 + 5.0),
            drift_core::FillKind::Solid,
        ));
    }
    let mut ed = Editor::new(doc);
    let _ = ed.fill_shapes();
    let curves: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    let t = std::time::Instant::now();
    ed.doc.begin();
    ed.doc.translate(curves, v(1.0, 0.0));
    assert!(ed.commit("Move all"));
    let moved = t.elapsed();
    assert_eq!(
        ed.doc.fills().filter(|(_, f)| f.path.is_some()).count(),
        1000
    );
    let _ = ed.fill_shapes();
    let t = std::time::Instant::now();
    ed.doc.begin();
    let p = ed.doc.add_point(v(-1000.0, -1000.0));
    let q = ed.doc.add_point(v(-900.0, -1000.0));
    ed.doc.add_line(p, q).unwrap();
    assert!(ed.commit("Line"));
    let small = t.elapsed();
    println!("1000 rectangles: moved {moved:?}, a later edit {small:?}");
    // (Debug build; it took 8 s and 3.7 s before.)
    assert!(moved < std::time::Duration::from_secs(4), "{moved:?}");
    assert!(small < std::time::Duration::from_secs(2), "{small:?}");
}

/// A drag moving fills' outlines and everything round them as one moves the fills along,
/// without finding the drawing's areas again each frame (#253); with anything else near (a
/// curve the move brings across one) the fill's area is found again.
#[test]
fn a_drag_moves_fills_along_with_everything_round_them() {
    let mut ed = Editor::new(Document::new());
    let (_, a) = plate(&mut ed);
    let (_, b) = polygon(
        &mut ed,
        &[v(100.0, 0.0), v(140.0, 0.0), v(140.0, 20.0), v(100.0, 20.0)],
    );
    // A line that stays, just clear of the first plate until it moves over it.
    polygon(&mut ed, &[v(42.0, -10.0), v(42.0, 40.0)]);
    let fa = area_fill_at(&mut ed, v(10.0, 10.0));
    let fb = area_fill_at(&mut ed, v(110.0, 10.0));
    let at_rest = ed.fill_shapes();
    let ids: Vec<EntityId> = a.iter().chain(&b).copied().collect();
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    let areas = |ed: &Editor| ed.regions.as_ref().map(|(_, r)| r.clone());

    // A small move: nothing else comes near either plate.
    let before = areas(&ed);
    ed.doc.begin();
    assert!(ed.drag(&ids, v(1.0, 3.0), &dragged));
    let shapes = ed.fill_shapes();
    assert!(
        matches!((&before, &areas(&ed)), (Some(x), Some(y)) if std::sync::Arc::ptr_eq(x, y)),
        "the areas were found again"
    );
    for f in [fa, fb] {
        let (was, now) = (&at_rest[&f].region, &shapes[&f].region);
        assert!(close(now.area, was.area));
        assert!(now.centroid.distance(was.centroid + v(1.0, 3.0)) < 1e-9);
        assert!(!shapes[&f].lost);
    }

    // On over the line that stays: not moved along, but found again (a fill a curve
    // crosses keeps its whole area, found without the curves across it).
    let before = areas(&ed);
    assert!(ed.drag(&ids, v(4.0, 0.0), &dragged));
    let shapes = ed.fill_shapes();
    assert!(
        !matches!((&before, &areas(&ed)), (Some(x), Some(y)) if std::sync::Arc::ptr_eq(x, y)),
        "moved along though a line it now crosses stayed"
    );
    assert!(shapes[&fa].crossed);
    assert!(close(shapes[&fb].region.area, at_rest[&fb].region.area));
    let mid_drag = shapes;
    assert!(ed.commit("Move"));
    // As a fresh look at the moved drawing finds them.
    let found = ed.fill_shapes();
    for f in [fa, fb] {
        assert!(close(found[&f].region.area, mid_drag[&f].region.area));
        assert!(
            found[&f]
                .region
                .centroid
                .distance(mid_drag[&f].region.centroid)
                < 1e-6
        );
    }
}

/// On a big drawing (its areas found off the edit path) a fill that had no area when a drag
/// began isn't looked for until the drag ends: it doesn't make every frame find the
/// drawing's areas (#253).
#[test]
fn a_drag_on_a_big_drawing_leaves_fills_without_an_area_until_it_ends() {
    let mut ed = Editor::new(Document::new());
    ed.areas.defer = Some(true);
    let (_, a) = plate(&mut ed);
    polygon(
        &mut ed,
        &[v(100.0, 0.0), v(140.0, 0.0), v(140.0, 20.0), v(100.0, 20.0)],
    );
    let fb = area_fill_at(&mut ed, v(110.0, 10.0));
    // A fill whose area is gone: nothing round its seed.
    let mut gone = drift_core::Fill::new(Vec::new(), v(-50.0, -50.0), drift_core::FillKind::Solid);
    ed.fill_style.apply(&mut gone);
    ed.doc.begin();
    let gone = ed.doc.add_fill(gone);
    assert!(ed.commit_import("Fill"));
    ed.settle_areas();
    let at_rest = ed.fill_shapes();
    assert!(!at_rest.contains_key(&gone), "it has no area");
    // Only the first plate moves: the second's fill stays as it was.
    let dragged: Vec<drift_solver::Param> = ed
        .doc
        .defining_points(a.iter().copied())
        .into_iter()
        .flat_map(drift_solver::Param::point)
        .collect();
    let before = ed.regions.as_ref().map(|(_, r)| r.clone());
    ed.doc.begin();
    assert!(ed.drag(&a, v(3.0, 1.0), &dragged));
    let shapes = ed.fill_shapes();
    assert!(
        matches!((&before, &ed.regions), (Some(x), Some((_, y))) if std::sync::Arc::ptr_eq(x, y)),
        "the areas were found again"
    );
    assert!(!shapes.contains_key(&gone));
    assert!(close(shapes[&fb].region.area, at_rest[&fb].region.area));
    assert!(ed.commit("Move"));
}
