//! An edit keeps shapes (the solver audit's PR 6, #350): where the relations leave room, lines
//! keep their lengths and directions and arcs their radii, ranked below the relations and the
//! geometry a relation names.

use drift_core::{Constraint, DVec2};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> (EntityId, EntityId, EntityId) {
    let pa = ed.doc.add_point(a);
    let pb = ed.doc.add_point(b);
    (ed.doc.add_line(pa, pb).unwrap(), pa, pb)
}

fn length(ed: &Editor, a: EntityId, b: EntityId) -> f64 {
    ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap())
}

fn direction(ed: &Editor, a: EntityId, b: EntityId) -> DVec2 {
    (ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap()).normalize()
}

/// #350: a level line made Parallel to a slanted free line. Before, the slanted one (the
/// reference) was stretched from 10.4 to 27.04 mm, and with it active the level one from 20
/// to 52: named geometry had no length to keep, and the cheapest change took one. Now the
/// lines turn and neither stretches, with or without the level one held level, and whichever
/// is active (#333's rule: the active one stays).
#[test]
fn parallel_keeps_both_lines_lengths() {
    for held_level in [false, true] {
        for active in [None, Some(0), Some(1)] {
            let mut ed = Editor::default();
            ed.doc.begin();
            let level = line(&mut ed, v(0.0, 0.0), v(20.0, 0.0));
            let slanted = line(&mut ed, v(30.0, 0.0), v(34.0, 9.6));
            ed.commit("Lines");
            if held_level {
                ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: level.0 }])
                    .unwrap();
            }
            let lines = [level.0, slanted.0];
            ed.selection = lines.into_iter().collect();
            ed.set_active(active.map(|i| lines[i]));
            ed.add_picked_constraints(
                "Parallel",
                &[Constraint::Parallel {
                    a: level.0,
                    b: slanted.0,
                }],
            )
            .unwrap();
            let what = format!("held level {held_level}, active {active:?}");
            assert!(
                (length(&ed, level.1, level.2) - 20.0).abs() < 1e-6,
                "{what}"
            );
            assert!(
                (length(&ed, slanted.1, slanted.2) - 10.4).abs() < 1e-6,
                "{what}"
            );
            let (u, w) = (
                direction(&ed, level.1, level.2),
                direction(&ed, slanted.1, slanted.2),
            );
            assert!(u.perp_dot(w).abs() < 1e-9, "{what}: parallel");
        }
    }
}

/// A dimension typed on a side of a four-sided outline nothing holds square: the side moves
/// across, keeping its direction, and the two beside it stretch along themselves, keeping
/// theirs. (Before, the least change moved the side's far end alone, and the side beside it
/// tilted.)
#[test]
fn a_typed_size_moves_the_side_across_and_keeps_the_outline_square() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let corners = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 25.0), v(0.0, 25.0)];
    let p: Vec<EntityId> = corners.iter().map(|&c| ed.doc.add_point(c)).collect();
    let sides: Vec<EntityId> = (0..4)
        .map(|k| ed.doc.add_line(p[k], p[(k + 1) % 4]).unwrap())
        .collect();
    ed.doc
        .add_constraint(Constraint::Fix {
            point: p[0],
            at: corners[0],
        })
        .unwrap();
    let width = ed
        .doc
        .add_constraint(Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 40.0,
        })
        .unwrap();
    assert!(ed.commit("Outline"));
    let before: Vec<DVec2> = (0..4)
        .map(|k| direction(&ed, p[k], p[(k + 1) % 4]))
        .collect();
    ed.set_dimension(width, 50.0).unwrap();
    assert!((length(&ed, p[0], p[1]) - 50.0).abs() < 1e-6);
    for k in 0..4 {
        let now = direction(&ed, p[k], p[(k + 1) % 4]);
        assert!(
            now.perp_dot(before[k]).abs() < 1e-6 && now.dot(before[k]) > 0.0,
            "side {:?} turned: {:?} -> {now:?}",
            sides[k],
            before[k]
        );
    }
}

fn circle_radius(ed: &Editor, circle: EntityId) -> f64 {
    match ed.doc.get(circle).unwrap().kind {
        EntityKind::Circle { radius, .. } => radius,
        _ => unreachable!(),
    }
}

/// A circle keeps its radius in an edit as an arc does (the main-fix PR's rule for drags,
/// carried to commits): a slanted line a circle is tangent to made Horizontal moves the
/// circle with it, and the circle keeps its size.
#[test]
fn a_relation_beside_a_circle_keeps_its_radius() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let (edge, _, _) = line(&mut ed, v(0.0, 0.0), v(30.0, 3.0));
    let centre = ed.doc.add_point(v(15.0, 12.0));
    let circle = ed.doc.add_circle(centre, 10.0).unwrap();
    ed.doc
        .add_constraint(Constraint::Tangent { a: edge, b: circle })
        .unwrap();
    assert!(ed.commit("Edge and circle"));
    let radius = circle_radius(&ed, circle);
    ed.selection = [edge].into_iter().collect();
    ed.add_picked_constraints("Horizontal", &[Constraint::Horizontal { line: edge }])
        .unwrap();
    assert!(
        (circle_radius(&ed, circle) - radius).abs() < 1e-6 * radius,
        "radius {radius} -> {}",
        circle_radius(&ed, circle)
    );
}

/// A triangle: an upright side (corner to foot), a top side (corner to a free end, 52 mm)
/// square to it, and a third side closing the free end back to the foot; the angle between
/// the upright and the top dimensioned at 90° (`swap`: named top first, at 270°).
struct Triangle {
    ed: Editor,
    corner: EntityId,
    foot: EntityId,
    free: EntityId,
    angle: ConstraintId,
}

fn triangle(swap: bool) -> Triangle {
    let mut ed = Editor::default();
    ed.doc.begin();
    let corner = ed.doc.add_point(v(136.0, 77.0));
    let foot = ed.doc.add_point(v(136.0, 14.0));
    let free = ed.doc.add_point(v(188.0, 77.0));
    let upright = ed.doc.add_line(corner, foot).unwrap();
    let top = ed.doc.add_line(corner, free).unwrap();
    ed.doc.add_line(free, foot).unwrap();
    ed.commit("Triangle");
    let (a, b, degrees) = if swap {
        (top, upright, 270.0)
    } else {
        (upright, top, 90.0)
    };
    let angle = ed
        .add_constraints("Angle", &[Constraint::Angle { a, b, degrees }])
        .unwrap()[0];
    Triangle {
        ed,
        corner,
        foot,
        free,
        angle,
    }
}

/// Where the top side's free end is with the top turned `degrees` from the upright about
/// the corner, at its 52 mm.
fn turned_top(degrees: f64) -> DVec2 {
    let t = degrees.to_radians();
    v(136.0 + 52.0 * t.sin(), 77.0 - 52.0 * t.cos())
}

/// The angle typed 90° to 145°. Keeping the third side's direction put the free end where
/// that direction meets the turned top side: nearly parallel at 145°, so 460 mm out on a
/// 52 mm side, flung far from the drawing. Now the top side turns about the corner and
/// keeps its length, and the third follows.
#[test]
fn a_typed_angle_turns_a_triangle_side_instead_of_flinging_it() {
    let mut t = triangle(false);
    t.ed.set_dimension(t.angle, 145.0).unwrap();
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(turned_top(145.0)) < 1e-6, "{at:?}");
    assert!((length(&t.ed, t.corner, t.free) - 52.0).abs() < 1e-6);
    assert!(t.ed.doc.point(t.corner).unwrap().distance(v(136.0, 77.0)) < 1e-9);
    assert!(t.ed.doc.point(t.foot).unwrap().distance(v(136.0, 14.0)) < 1e-9);
}

/// Every angle turns the top the same way, with no jump between near values (a first
/// fix took the turn only past a 3× stretch: 125° flung the end to (259, 163), 126° put it
/// at (178, 108)); typed in steps it lands where typed at once, and typed back to 90° the
/// triangle is as it was.
#[test]
fn every_typed_angle_turns_the_side_alike() {
    for degrees in [95.0, 120.0, 125.0, 126.0, 160.0, 179.0, 60.0, 30.0] {
        let mut t = triangle(false);
        t.ed.set_dimension(t.angle, degrees).unwrap();
        let at = t.ed.doc.point(t.free).unwrap();
        assert!(
            at.distance(turned_top(degrees)) < 1e-6,
            "{degrees}°: {at:?}"
        );
    }
    let mut t = triangle(false);
    for degrees in (19..=29).map(|k| f64::from(k) * 5.0) {
        t.ed.set_dimension(t.angle, degrees).unwrap();
    }
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(turned_top(145.0)) < 1e-6, "in steps: {at:?}");
    t.ed.set_dimension(t.angle, 90.0).unwrap();
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(v(188.0, 77.0)) < 1e-6, "back: {at:?}");
}

/// A fillet at the foot keeps its radius while the top turns (a first fix ranked the
/// turned side's length above the radii: R8 grew to 17.11 at 160°).
#[test]
fn a_typed_angle_turning_a_side_grows_no_fillet() {
    let mut t = triangle(false);
    let arc = t.ed.fillet(t.foot, 8.0).unwrap();
    t.ed.set_dimension(t.angle, 160.0).unwrap();
    let r = drift_solver::radius_of(&t.ed.doc, arc).unwrap();
    assert!((r - 8.0).abs() < 1e-6, "R {r}");
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(turned_top(160.0)) < 1e-6, "{at:?}");
}

/// A line hanging off the free end is carried along at its length (a first fix shortened
/// a 39.4 mm one by 2.2 mm at 155°; keeping shapes alone left a sliver, and the solve as
/// before then flung the top to 507 mm).
#[test]
fn a_typed_angle_carries_a_line_hanging_off_the_turned_side() {
    for degrees in [145.0, 155.0] {
        let mut t = triangle(false);
        t.ed.doc.begin();
        let end = t.ed.doc.add_point(v(220.0, 100.0));
        t.ed.doc.add_line(t.free, end).unwrap();
        t.ed.commit("Tail");
        let tail = length(&t.ed, t.free, end);
        t.ed.set_dimension(t.angle, degrees).unwrap();
        assert!(
            (length(&t.ed, t.free, end) - tail).abs() < 1e-6,
            "{degrees}°"
        );
        let at = t.ed.doc.point(t.free).unwrap();
        assert!(
            at.distance(turned_top(degrees)) < 1e-6,
            "{degrees}°: {at:?}"
        );
    }
}

/// With the top side fixed, the upright turns at its length about the corner, whichever
/// line the angle names first (the first fix only turned a line it could keep the other
/// in place for: named upright first, the upright stretched from 63 to 200.8 mm).
#[test]
fn a_typed_angle_turns_the_free_side_whichever_is_named_first() {
    for swap in [false, true] {
        let mut t = triangle(swap);
        let fixes = [
            Constraint::Fix {
                point: t.corner,
                at: v(136.0, 77.0),
            },
            Constraint::Fix {
                point: t.free,
                at: v(188.0, 77.0),
            },
        ];
        t.ed.add_constraints("Fix", &fixes).unwrap();
        t.ed.set_dimension(t.angle, if swap { 215.0 } else { 145.0 })
            .unwrap();
        assert!(
            (length(&t.ed, t.corner, t.foot) - 63.0).abs() < 1e-6,
            "swap {swap}: upright {}",
            length(&t.ed, t.corner, t.foot)
        );
    }
}
