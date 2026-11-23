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

/// An open or closed run of lines through `at`.
fn lines_through(ed: &mut Editor, at: &[DVec2], closed: bool) -> (Vec<EntityId>, Vec<EntityId>) {
    ed.doc.begin();
    let points: Vec<EntityId> = at.iter().map(|p| ed.doc.add_point(*p)).collect();
    let n = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let lines = (0..n)
        .map(|i| {
            ed.doc
                .add_line(points[i], points[(i + 1) % points.len()])
                .unwrap()
        })
        .collect();
    ed.commit("Lines");
    (points, lines)
}

/// An angle between `a` and `b` at the value it measures.
fn measured_angle(ed: &mut Editor, a: EntityId, b: EntityId) -> ConstraintId {
    let probe = Constraint::Angle { a, b, degrees: 0.0 };
    let degrees = drift_solver::measure(&ed.doc, &probe).unwrap();
    ed.add_constraints("Angle", &[Constraint::Angle { a, b, degrees }])
        .unwrap()[0]
}

/// Where a typed angle doesn't stretch the line it turns, the sketch stays square as before
/// (a second fix released the neighbours' directions on every angle: a rectangle's top
/// tilted 12.8° at 135°, a zigzag's third run 11.6°).
#[test]
fn a_typed_angle_that_stretches_nothing_keeps_the_rest_square() {
    let mut ed = Editor::default();
    let (p, l) = lines_through(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0), v(0.0, 50.0)],
        true,
    );
    let angle = measured_angle(&mut ed, l[0], l[1]);
    ed.set_dimension(angle, 135.0).unwrap();
    let (c, d) = (ed.doc.point(p[2]).unwrap(), ed.doc.point(p[3]).unwrap());
    assert!((c.y - d.y).abs() < 1e-6, "top tilted: {c:?} {d:?}");

    let mut ed = Editor::default();
    let (p, l) = lines_through(
        &mut ed,
        &[v(0.0, 0.0), v(50.0, 0.0), v(50.0, 40.0), v(100.0, 40.0)],
        false,
    );
    let angle = measured_angle(&mut ed, l[0], l[1]);
    ed.set_dimension(angle, 135.0).unwrap();
    let (c, d) = (ed.doc.point(p[2]).unwrap(), ed.doc.point(p[3]).unwrap());
    assert!((c.y - d.y).abs() < 1e-6, "third run tilted: {c:?} {d:?}");
}

/// An angle changed by more than 90° keeps the side it was on: 145° then 54° is a true
/// 54° (the turn the short way met it mirrored across the upright, its corner showing
/// 126°), and back to 90° the triangle is as it was.
#[test]
fn a_typed_angle_keeps_its_side_past_ninety_degrees() {
    let mut t = triangle(false);
    t.ed.set_dimension(t.angle, 145.0).unwrap();
    t.ed.set_dimension(t.angle, 54.0).unwrap();
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(turned_top(54.0)) < 1e-6, "{at:?}");
    t.ed.set_dimension(t.angle, 90.0).unwrap();
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(v(188.0, 77.0)) < 1e-6, "{at:?}");
    // Down to 30° and up to 119°: the closing side more than triples, and the turn stands.
    let mut t = triangle(false);
    t.ed.set_dimension(t.angle, 30.0).unwrap();
    t.ed.set_dimension(t.angle, 119.0).unwrap();
    let at = t.ed.doc.point(t.free).unwrap();
    assert!(at.distance(turned_top(119.0)) < 1e-6, "{at:?}");
}

/// A line hanging off the turned side's free end keeps its direction as well as its
/// length (a second fix swung it from 35.7° to -15.4°).
#[test]
fn a_typed_angle_carries_a_hanging_line_at_its_direction() {
    let mut t = triangle(false);
    t.ed.doc.begin();
    let end = t.ed.doc.add_point(v(220.0, 100.0));
    t.ed.doc.add_line(t.free, end).unwrap();
    t.ed.commit("Tail");
    let before = t.ed.doc.point(end).unwrap() - t.ed.doc.point(t.free).unwrap();
    t.ed.set_dimension(t.angle, 145.0).unwrap();
    let now = t.ed.doc.point(end).unwrap() - t.ed.doc.point(t.free).unwrap();
    assert!(now.distance(before) < 1e-6, "{before:?} -> {now:?}");
}

/// A triangle of three separate lines joined by Coincident relations turns like one that
/// shares its points (the third side wasn't seen as joined to the turned end: flung to
/// 459 mm, as before the fix).
#[test]
fn a_typed_angle_turns_a_side_joined_by_coincident_relations() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let line = |ed: &mut Editor, a: DVec2, b: DVec2| {
        let (pa, pb) = (ed.doc.add_point(a), ed.doc.add_point(b));
        (ed.doc.add_line(pa, pb).unwrap(), pa, pb)
    };
    let (upright, corner, foot) = line(&mut ed, v(136.0, 77.0), v(136.0, 14.0));
    let (top, corner2, free) = line(&mut ed, v(136.0, 77.0), v(188.0, 77.0));
    let (_, free2, foot2) = line(&mut ed, v(188.0, 77.0), v(136.0, 14.0));
    ed.commit("Lines");
    ed.add_constraints(
        "Coincident",
        &[
            Constraint::Coincident {
                a: corner,
                b: corner2,
            },
            Constraint::Coincident { a: free, b: free2 },
            Constraint::Coincident { a: foot, b: foot2 },
        ],
    )
    .unwrap();
    let angle = measured_angle(&mut ed, upright, top);
    ed.set_dimension(angle, 145.0).unwrap();
    let at = ed.doc.point(free).unwrap();
    assert!(at.distance(turned_top(145.0)) < 1e-6, "{at:?}");
}

/// Where the line that would turn can't, the other one does: the top held level, 145°
/// turns the upright (keeping the top turning shrank it from 52 to 7 mm, the upright met
/// mirrored); the top fixed in place, 145° then 35° turns the upright at its 63 mm.
#[test]
fn a_typed_angle_turns_the_other_line_where_one_is_held() {
    let mut t = triangle(false);
    let top =
        t.ed.doc
            .users_of(t.free)
            .find(|u| {
                matches!(t.ed.doc.get(*u).map(|e| e.kind),
                Some(EntityKind::Line { a, .. }) if a == t.corner)
            })
            .unwrap();
    t.ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: top }])
        .unwrap();
    t.ed.set_dimension(t.angle, 145.0).unwrap();
    assert!((length(&t.ed, t.corner, t.free) - 52.0).abs() < 1e-6);
    assert!((length(&t.ed, t.corner, t.foot) - 63.0).abs() < 1e-6);
    let m = drift_solver::measure(&t.ed.doc, t.ed.doc.constraint(t.angle).unwrap()).unwrap();
    assert!((m - 145.0).abs() < 1e-6, "reads {m}");

    let mut t = triangle(false);
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
    for degrees in [145.0, 35.0] {
        t.ed.set_dimension(t.angle, degrees).unwrap();
        assert!(
            (length(&t.ed, t.corner, t.foot) - 63.0).abs() < 1e-6,
            "{degrees}°: upright {}",
            length(&t.ed, t.corner, t.foot)
        );
        let foot = t.ed.doc.point(t.foot).unwrap();
        // The foot below the top: the angle not met mirrored above it.
        assert!(foot.y < 77.0, "{degrees}°: foot {foot:?}");
    }
}

/// Two lines sharing an end and nothing else: 145° then 30° is a true 30°, not met
/// mirrored (the visible corner 150°), though nothing stretched. (The lines' rays,
/// first to second, start at 270° for the 90° the dimension reads: they stay on that
/// half, at the value plus 180°.)
#[test]
fn a_typed_angle_on_an_open_corner_keeps_its_side() {
    let mut ed = Editor::default();
    let (p, l) = lines_through(&mut ed, &[v(0.0, -60.0), v(0.0, 0.0), v(50.0, 0.0)], false);
    let angle = measured_angle(&mut ed, l[0], l[1]);
    for degrees in [145.0, 30.0] {
        ed.set_dimension(angle, degrees).unwrap();
        let (c, e) = (ed.doc.point(p[1]).unwrap(), ed.doc.point(p[2]).unwrap());
        let d0 = c - ed.doc.point(p[0]).unwrap();
        let ray = d0
            .perp_dot(e - c)
            .atan2(d0.dot(e - c))
            .to_degrees()
            .rem_euclid(360.0);
        let off = (ray - degrees - 180.0).rem_euclid(360.0);
        assert!(off.min(360.0 - off) < 1e-6, "{degrees}: rays at {ray}");
    }
}

/// A fillet at the free end keeps its radius while the top turns: the round is carried
/// round with the top, and the third side beyond it turns to meet it (the turning solve
/// only released lines met at the top's own ends, so the third side kept its direction past
/// the round, the turn was refused, and the solve before it grew R8 to 12.88 at 120°, 17.00
/// at 145° and 21.91 at 160°).
#[test]
fn a_typed_angle_turning_a_filleted_side_grows_no_fillet() {
    for degrees in [120.0, 145.0, 160.0] {
        let mut t = triangle(false);
        let arc = t.ed.fillet(t.free, 8.0).unwrap();
        let Some(EntityKind::Arc {
            center, start, end, ..
        }) = t.ed.doc.get(arc).map(|e| e.kind)
        else {
            panic!("no round");
        };
        let top = length(&t.ed, t.corner, end);
        t.ed.set_dimension(t.angle, degrees).unwrap();
        let r = drift_solver::radius_of(&t.ed.doc, arc).unwrap();
        assert!((r - 8.0).abs() < 1e-6, "{degrees}°: R {r}");
        let now = length(&t.ed, t.corner, end);
        assert!((now - top).abs() < 1e-6, "{degrees}°: top {top} -> {now}");
        assert!(t.ed.doc.point(t.corner).unwrap().distance(v(136.0, 77.0)) < 1e-9);
        assert!(t.ed.doc.point(t.foot).unwrap().distance(v(136.0, 14.0)) < 1e-9);
        // (Still tangent to the top where they meet.)
        let (c, e, k) = (
            t.ed.doc.point(center).unwrap(),
            t.ed.doc.point(end).unwrap(),
            t.ed.doc.point(t.corner).unwrap(),
        );
        let off = (e - c).normalize().dot((e - k).normalize()).abs();
        assert!(off < 1e-6, "{degrees}°: off tangent {off}");
        // (And to the third side where that meets it: still a fillet, not a corner. The
        // first fix left it 20°-46° off: Fable review of #520, finding 2.)
        let (s, f) = (
            t.ed.doc.point(start).unwrap(),
            t.ed.doc.point(t.foot).unwrap(),
        );
        let off = (s - c).normalize().dot((s - f).normalize()).abs();
        assert!(
            off < 1e-6,
            "{degrees}°: off tangent to the third side {off}"
        );
    }
}

/// A line hanging off the far end of the round is carried round with it, as it is (the
/// first fix stretched a 29 mm tail to 56 / 77 mm at 120 / 145°, turned about its free end:
/// Fable review of #520, finding 1).
#[test]
fn a_typed_angle_carries_a_line_hanging_off_the_round() {
    for degrees in [120.0, 145.0] {
        let mut ed = Editor::default();
        ed.doc.begin();
        let corner = ed.doc.add_point(v(136.0, 77.0));
        let foot = ed.doc.add_point(v(136.0, 14.0));
        let free = ed.doc.add_point(v(188.0, 77.0));
        let tail = ed.doc.add_point(v(188.0, 40.0));
        let upright = ed.doc.add_line(corner, foot).unwrap();
        let top = ed.doc.add_line(corner, free).unwrap();
        ed.doc.add_line(free, tail).unwrap();
        ed.commit("Chain");
        let angle = measured_angle(&mut ed, upright, top);
        let arc = ed.fillet(free, 8.0).unwrap();
        let Some(EntityKind::Arc { start, .. }) = ed.doc.get(arc).map(|e| e.kind) else {
            panic!("no round");
        };
        let was = length(&ed, start, tail);
        ed.set_dimension(angle, degrees).unwrap();
        let r = drift_solver::radius_of(&ed.doc, arc).unwrap();
        assert!((r - 8.0).abs() < 1e-6, "{degrees}°: R {r}");
        let now = length(&ed, start, tail);
        assert!((now - was).abs() < 1e-6, "{degrees}°: tail {was} -> {now}");
    }
}

/// A fillet at the angle's own corner stays as it is, and the top keeps its length: the
/// two lines share no point then, the top turns about its middle, and the round, tangent to
/// the top, ends on the upright. Carrying it round tilted the upright and the turn was
/// refused: with auto-relations the top flung from 46 to 106 / 634 / 401 mm at 120 / 145 /
/// 160° (Opus review of #520, finding 1). (Without them, 60–145° reshape on main too: a
/// follow-up; 160° is clean.)
#[test]
fn a_typed_angle_carries_no_fillet_at_its_own_corner() {
    for (auto, degrees) in [
        (true, 60.0),
        (true, 120.0),
        (true, 145.0),
        (true, 160.0),
        (false, 160.0),
    ] {
        let mut t = triangle(false);
        t.ed.prefs.auto_relations = auto;
        let arc = t.ed.fillet(t.corner, 6.0).unwrap();
        let top =
            t.ed.doc
                .users_of(t.free)
                .find(|l| {
                    matches!(t.ed.doc.get(*l).map(|e| e.kind),
                    Some(EntityKind::Line { b, .. }) if b == t.free)
                })
                .unwrap();
        let Some(EntityKind::Line { a: near, .. }) = t.ed.doc.get(top).map(|e| e.kind) else {
            panic!("no top");
        };
        let was = length(&t.ed, near, t.free);
        t.ed.set_dimension(t.angle, degrees).unwrap();
        let r = drift_solver::radius_of(&t.ed.doc, arc).unwrap();
        assert!((r - 6.0).abs() < 1e-6, "auto {auto}, {degrees}°: R {r}");
        let c = *t.ed.doc.constraint(t.angle).unwrap();
        let read = drift_solver::measure(&t.ed.doc, &c).unwrap();
        assert!(
            (read - degrees).abs() < 1e-6,
            "auto {auto}, {degrees}°: reads {read}"
        );
        let now = length(&t.ed, near, t.free);
        assert!(
            (now - was).abs() < 1e-6,
            "auto {auto}, {degrees}°: top {was} -> {now}"
        );
    }
}

/// A fillet at the angle's own corner, auto-relations off, at `degrees`: the round's radius
/// and where the free end lands.
fn vertex_fillet_off(degrees: f64) -> (f64, DVec2) {
    let mut t = triangle(false);
    t.ed.prefs.auto_relations = false;
    let arc = t.ed.fillet(t.corner, 6.0).unwrap();
    t.ed.set_dimension(t.angle, degrees).unwrap();
    (
        drift_solver::radius_of(&t.ed.doc, arc).unwrap(),
        t.ed.doc.point(t.free).unwrap(),
    )
}

/// Guard: where a fillet at the angle's own corner (auto-relations off) still reshapes, it
/// does so exactly as on main (b92d400, with #518): this PR makes none of it worse. #522 is
/// to fix it (see `a_typed_angle_reshapes_no_corner_fillet_without_relations`); then this
/// guard goes.
#[test]
fn a_vertex_fillet_without_relations_reshapes_no_worse_than_main() {
    for (degrees, r, at) in [
        (
            60.0,
            6.000272528222731,
            v(172.14922385105382, 57.79600184454344),
        ),
        (
            120.0,
            6.000372893709087,
            v(206.96320363636167, 99.97656634817018),
        ),
        (
            145.0,
            10.31942489362502,
            v(506.0334971709158, 563.9542461181143),
        ),
    ] {
        let (r1, at1) = vertex_fillet_off(degrees);
        assert!((r1 - r).abs() <= 1e-9 * r, "{degrees}°: R {r1}, main {r}");
        assert!(
            at1.distance(at) <= 1e-9 * at.length(),
            "{degrees}°: {at1:?}, main {at:?}"
        );
    }
}

/// What #522 wants of the same sketch: no fling and the round's radius unchanged (the top,
/// 46 mm from the round to the free end, keeps its length), the angle reading its value.
#[test]
#[ignore = "#522"]
fn a_typed_angle_reshapes_no_corner_fillet_without_relations() {
    for degrees in [60.0, 120.0, 145.0] {
        let mut t = triangle(false);
        t.ed.prefs.auto_relations = false;
        let arc = t.ed.fillet(t.corner, 6.0).unwrap();
        let near =
            t.ed.doc
                .users_of(t.free)
                .find_map(|l| match t.ed.doc.get(l).map(|e| e.kind) {
                    Some(EntityKind::Line { a, b }) if b == t.free => Some(a),
                    _ => None,
                })
                .unwrap();
        let was = length(&t.ed, near, t.free);
        t.ed.set_dimension(t.angle, degrees).unwrap();
        let r = drift_solver::radius_of(&t.ed.doc, arc).unwrap();
        assert!((r - 6.0).abs() < 1e-6, "{degrees}°: R {r}");
        let now = length(&t.ed, near, t.free);
        assert!((now - was).abs() < 1e-6, "{degrees}°: top {was} -> {now}");
        let c = *t.ed.doc.constraint(t.angle).unwrap();
        let read = drift_solver::measure(&t.ed.doc, &c).unwrap();
        assert!((read - degrees).abs() < 1e-6, "{degrees}°: reads {read}");
    }
}
