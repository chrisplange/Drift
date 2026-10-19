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
    // Deleting the new piece opens the outline: the fill goes, as for any outline curve.
    let piece = *boundary.iter().find(|e| !sides.contains(e)).unwrap();
    ed.doc.begin();
    ed.doc.delete([piece]);
    ed.commit("Delete");
    assert!(ed.doc.fill(id).is_none());
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
