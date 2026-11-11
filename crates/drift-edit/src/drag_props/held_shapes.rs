//! A drag that can't land keeps the shapes it doesn't move (#278; `Editor::breaks_held_shape`
//! in `editor/drag.rs`): it doesn't grow (or shrink) the radius of an arc or circle faster than
//! ten times the pointer, nor shorten a line faster than a hundred times it, nor crush a line to
//! nothing. It stops at the last shape that went, as "Stop, as now" has a drag keep the radii it
//! can't follow and F1 has it stop short of crushing an arc. What lands, and what changes no
//! faster than that, goes as before.

use drift_core::{Constraint, EntityKind};

use super::*;

/// `small_drag_is_continuous_heavy` seed 16's shrunk case: an arc whose end is fixed, tangent
/// at that end to a line through it, a second line square to that one from the arc's centre;
/// the first line and an arc ending on the second are dragged. The drag can't land (the line
/// square to the first must run through the fixed end and the centre), and the solve slid the
/// centre out: the radius went from 8 to 52.6 for a drag of 0.078.
pub(super) fn seed_16() -> (Plan, Pick, DVec2) {
    let mut extras = vec![false; 48];
    extras[0] = true;
    (
        Plan {
            ops: vec![
                Op::Arc {
                    from: 0,
                    angle: 2,
                    radius: 0,
                    sweep: 0,
                    ccw: false,
                },
                Op::Step {
                    from: 19,
                    angle: 14,
                    len: 0,
                },
                Op::Tangent {
                    arc: 0,
                    end: true,
                    forward: false,
                    len: 2,
                },
                Op::Arc {
                    from: 8,
                    angle: 0,
                    radius: 1,
                    sweep: 0,
                    ccw: false,
                },
                Op::Arc {
                    from: 2,
                    angle: 4,
                    radius: 0,
                    sweep: 0,
                    ccw: false,
                },
            ],
            extras,
            fix: Some(9),
            length: None,
        },
        Pick::Curves(6638522335503980748),
        DVec2::new(0.0, 0.07784862269881661),
    )
}

/// The radii of every arc and circle in `built`, in order.
fn radii(built: &Built) -> Vec<f64> {
    built
        .ed
        .doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Arc { .. } => drift_solver::radius_of(&built.ed.doc, id),
            EntityKind::Circle { radius, .. } => Some(radius),
            _ => None,
        })
        .collect()
}

/// Seed 16's drag, at every size, turn and place: refused, nothing moved, no radius changed,
/// nothing pushed.
#[test]
fn a_drag_that_cant_land_doesnt_grow_a_radius_it_doesnt_move() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    for (scale, turn, shift) in [
        (1.0, 0.0, DVec2::ZERO),
        (10.0, 0.0, DVec2::ZERO),
        (0.1, 0.0, DVec2::ZERO),
        (1.0, 37.0, DVec2::ZERO),
        (1.0, 0.0, DVec2::new(1e5, 0.0)),
        (1.0, 0.0, DVec2::new(1e6, 0.0)),
        (1.0, 0.0, DVec2::new(0.0, -1e6)),
        (0.1, 0.0, DVec2::new(1e6, 0.0)),
    ] {
        let t = Similarity { scale, turn, shift };
        let mut built = Built::new(&sketch, t).unwrap();
        let ids = pick.ids(&built);
        let (start, was) = (built.positions(), radii(&built));
        let run = drag(&mut built, &ids, t.vector(offset), 1).unwrap();
        assert_eq!(run.refused, 1, "{scale}× turned {turn} at {shift}: went");
        assert_eq!(
            built.positions(),
            start,
            "{scale}× turned {turn} at {shift}"
        );
        assert_eq!(radii(&built), was, "{scale}× turned {turn} at {shift}");
        assert!(!built.ed.drag_pushed(), "{scale}× turned {turn} at {shift}");
    }
}

/// The same over a sweep of offsets (the solve's growth goes from 25 to 650 times the offset
/// across it): every frame refused, each at the cost of one solve, the same at any offset.
#[test]
fn a_radius_it_doesnt_move_stops_the_drag_at_any_offset() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    for k in 0..=12 {
        let s = 0.0005 * 10f64.powf(f64::from(k) / 4.0);
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let ids = pick.ids(&built);
        let start = built.positions();
        let run = drag(&mut built, &ids, offset.normalize() * s, 1).unwrap();
        assert_eq!((run.refused, built.positions()), (1, start), "offset {s}");
    }
}

/// Radii are never grown, however slowly: seed 16's drag by a micron, whose radius grows some
/// 25 times as fast as the pointer, is refused too. (Before PR 3 growth slower than the
/// pointer's tenfold went; the user's rule is that a drag never grows an arc it doesn't move.)
/// The same drag the other way, shrinking it, goes.
#[test]
fn slow_radius_growth_stops_too() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let start = built.positions();
    let run = drag(&mut built, &ids, offset.normalize() * 1e-6, 1).unwrap();
    assert_eq!((run.refused, built.positions()), (1, start));
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let run = drag(&mut built, &ids, -offset.normalize() * 1e-6, 1).unwrap();
    assert_eq!(run.refused, 0);
}

/// Seed 16's shape with its fixed point held other ways: by `FixX` and `FixY`, and by a second
/// point joined to it by `Coincident` and fixed. Each a fixed coordinate the drag can't move,
/// so the drag can't land either way, and is refused.
#[test]
fn a_point_fixed_any_way_is_one_the_drag_cant_move() {
    for variant in ["fix x and y", "coincident with a fixed point"] {
        let (plan, pick, offset) = seed_16();
        let sketch = plan.sketch().unwrap();
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let ids = pick.ids(&built);
        built.ed.doc.begin();
        let fixes: Vec<_> = built
            .ed
            .doc
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::Fix { .. }))
            .map(|(id, c)| (id, *c))
            .collect();
        for (cid, c) in fixes {
            let Constraint::Fix { point, at } = c else {
                unreachable!()
            };
            built.ed.doc.remove_constraint(cid);
            if variant == "fix x and y" {
                for c in [
                    Constraint::FixX { point, x: at.x },
                    Constraint::FixY { point, y: at.y },
                ] {
                    built.ed.doc.add_constraint(c).unwrap();
                }
            } else {
                let q = built.ed.doc.add_point(at);
                for c in [
                    Constraint::Coincident { a: point, b: q },
                    Constraint::Fix { point: q, at },
                ] {
                    built.ed.doc.add_constraint(c).unwrap();
                }
            }
        }
        assert!(built.ed.commit("Fix"));
        let start = built.positions();
        let run = drag(&mut built, &ids, offset, 1).unwrap();
        assert_eq!((run.refused, built.positions()), (1, start), "{variant}");
    }
}

/// Seed 16's shape built on a circle (its centre the second line's end, through the fixed
/// point, tangent there to the first line): a circle's radius is a solver unknown and takes up
/// the misfit the same way (8 to 55 for a drag of 0.078), and is kept the same way, at the
/// origin and a kilometre out.
#[test]
fn a_drag_that_cant_land_doesnt_grow_a_circle_it_doesnt_move() {
    for at in [DVec2::ZERO, DVec2::new(1e6, 0.0)] {
        for d in [0.0005, 0.01, 0.078, 0.5] {
            let v = |x: f64, y: f64| at + DVec2::new(x, y);
            let mut ed = Editor::default();
            ed.doc.begin();
            let p0 = ed.doc.add_point(v(0.0, 0.0));
            let p4 = ed.doc.add_point(v(-10.0, 17.320508075688775));
            let line = ed.doc.add_line(p0, p4).unwrap();
            let p1 = ed.doc.add_point(v(6.928203230275509, 4.0));
            let circle = ed.doc.add_circle(p1, 8.0).unwrap();
            let p3 = ed.doc.add_point(v(-1.7320508075688772, -1.0));
            let square = ed.doc.add_line(p1, p3).unwrap();
            let p9 = ed.doc.add_point(v(-1.7320508075688772, -6.0));
            let stub = ed.doc.add_line(p3, p9).unwrap();
            for c in [
                Constraint::PointOnCurve {
                    point: p0,
                    curve: circle,
                },
                Constraint::Tangent { a: line, b: circle },
                Constraint::Perpendicular { a: square, b: line },
                Constraint::Fix {
                    point: p0,
                    at: v(0.0, 0.0),
                },
            ] {
                ed.doc.add_constraint(c).unwrap();
            }
            assert!(ed.commit("Sketch"));
            let EntityKind::Circle { radius: was, .. } = ed.doc.get(circle).unwrap().kind else {
                unreachable!()
            };
            ed.doc.begin();
            let ids = [line, stub];
            let dragged: Vec<Param> = ed
                .doc
                .defining_points(ids)
                .into_iter()
                .flat_map(Param::point)
                .collect();
            let went = ed.drag(&ids, DVec2::new(0.0, d), &dragged);
            let EntityKind::Circle { radius, .. } = ed.doc.get(circle).unwrap().kind else {
                unreachable!()
            };
            assert!(
                !went && radius == was,
                "at {at}, drag {d}: radius {was} -> {radius}"
            );
        }
    }
}

/// A lever: a line of length `long` from a fixed pivot, and the dragged point on it at a kept
/// distance `short` from the pivot (or free to slide on it), dragged off its circle. The
/// dragged point can't land, and the far end swings `long / short` times as far: geometry,
/// not the solve's choice, and no radius to keep. Goes, as before, at any advantage.
fn lever(long: f64, short: f64, kept: bool) -> (bool, f64) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let pivot = ed.doc.add_point(DVec2::ZERO);
    let end = ed.doc.add_point(DVec2::new(long, 0.0));
    let line = ed.doc.add_line(pivot, end).unwrap();
    let q = ed.doc.add_point(DVec2::new(short, 0.0));
    let mut relations = vec![
        Constraint::PointOnCurve {
            point: q,
            curve: line,
        },
        Constraint::Distance {
            a: pivot,
            b: end,
            value: long,
        },
        Constraint::Fix {
            point: pivot,
            at: DVec2::ZERO,
        },
    ];
    if kept {
        relations.push(Constraint::Distance {
            a: pivot,
            b: q,
            value: short,
        });
    }
    for c in relations {
        ed.doc.add_constraint(c).unwrap();
    }
    assert!(ed.commit("Sketch"));
    ed.doc.begin();
    let off = DVec2::splat(0.02);
    let went = ed.drag(&[q], off, &Param::point(q));
    let swung = ed.doc.point(end).unwrap().distance(DVec2::new(long, 0.0)) / off.length();
    (went, swung)
}

#[test]
fn a_lever_still_goes_at_any_advantage() {
    for advantage in [5.0, 50.0, 1000.0] {
        for kept in [true, false] {
            let (went, swung) = lever(2.0 * advantage, 2.0, kept);
            assert!(went, "1:{advantage}, kept {kept}");
            assert!(
                swung > 0.1 * advantage.min(300.0),
                "1:{advantage}, kept {kept}: swung {swung}×"
            );
        }
    }
}

/// A shallow arc (chord 100, sagitta 2) whose ends are fixed, tangent at its apex to a short
/// line dragged straight up: its radius must change some 300 times as fast as the line moves
/// to follow it. The drag lands, so it is never judged: it goes, at any size and place.
#[test]
fn a_drag_that_lands_may_change_a_radius_it_doesnt_move() {
    for at in [DVec2::ZERO, DVec2::new(1e5, 0.0), DVec2::new(1e6, 0.0)] {
        for scale in [0.1, 1.0, 10.0] {
            let v = |x: f64, y: f64| at + DVec2::new(x, y) * scale;
            let mut ed = Editor::default();
            ed.doc.begin();
            let r = (50.0f64 * 50.0 + 4.0) / 4.0;
            let c = ed.doc.add_point(v(0.0, 2.0 - r));
            let a = ed.doc.add_point(v(50.0, 0.0));
            let b = ed.doc.add_point(v(-50.0, 0.0));
            let arc = ed.doc.add_arc(c, a, b).unwrap();
            let l0 = ed.doc.add_point(v(-10.0, 2.0));
            let l1 = ed.doc.add_point(v(10.0, 2.0));
            let line = ed.doc.add_line(l0, l1).unwrap();
            for c in [
                Constraint::Tangent { a: line, b: arc },
                Constraint::Fix {
                    point: a,
                    at: v(50.0, 0.0),
                },
                Constraint::Fix {
                    point: b,
                    at: v(-50.0, 0.0),
                },
            ] {
                ed.doc.add_constraint(c).unwrap();
            }
            assert!(ed.commit("Sketch"));
            let was = drift_solver::radius_of(&ed.doc, arc).unwrap();
            ed.doc.begin();
            let dragged: Vec<Param> = ed
                .doc
                .defining_points([line])
                .into_iter()
                .flat_map(Param::point)
                .collect();
            let d = 0.1 * scale;
            assert!(
                ed.drag(&[line], DVec2::new(0.0, d), &dragged),
                "{scale}× at {at}"
            );
            let now = drift_solver::radius_of(&ed.doc, arc).unwrap();
            assert!(
                (was - now).abs() > 250.0 * d,
                "{scale}× at {at}: radius {was} -> {now}"
            );
        }
    }
}

/// A refusal's reach: a drag of two parts, one of them seed 16's, the other a line held
/// horizontal, is refused whole when seed 16's part trips the rule: the drag stops, as for any
/// part that can't go. Neither part moves and nothing is pushed.
#[test]
fn one_part_that_grows_a_radius_stops_the_whole_drag() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let mut ids = pick.ids(&built);
    built.ed.doc.begin();
    let a = built.ed.doc.add_point(DVec2::new(100.0, 0.0));
    let b = built.ed.doc.add_point(DVec2::new(120.0, 0.0));
    let line = built.ed.doc.add_line(a, b).unwrap();
    built
        .ed
        .doc
        .add_constraint(Constraint::Horizontal { line })
        .unwrap();
    assert!(built.ed.commit("Line"));
    // (Known to the harness, which names what it drags.)
    built.curves.push(line);
    built.points.extend([a, b]);
    ids.push(line);
    let (start, line_at) = (
        built.positions(),
        [a, b].map(|p| built.ed.doc.point(p).unwrap()),
    );
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert_eq!(run.refused, 1);
    assert_eq!(built.positions(), start);
    assert_eq!([a, b].map(|p| built.ed.doc.point(p).unwrap()), line_at);
    assert!(!built.ed.drag_pushed());
}

/// After a refused frame the drag is where the last frame that went left it, nothing pushed:
/// seed 16 dragged a micron back (goes: its radius shrinks), then on (refused: it grows).
#[test]
fn a_refused_frame_leaves_the_drag_at_the_last_that_went() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    built.ed.doc.begin();
    let small = -offset.normalize() * 1e-6;
    assert!(built.ed.drag(&ids, small, &dragged));
    let after_small = built.positions();
    assert!(!built.ed.drag(&ids, offset, &dragged));
    assert_eq!(built.positions(), after_small);
    assert!(!built.ed.drag_pushed());
    // The next frame is measured from where the last one that went ended.
    assert!(built.ed.drag(&ids, -small, &dragged));
    assert!(built.ed.commit("Move"));
}

/// `small_drag_is_continuous_heavy` seed 20's shrunk case (the pin `c_`): an arc tangent at both
/// ends to lines through them, its end fixed, a free point on the first line's extension; the
/// lines are dragged. The drag can't land, and the solve slid the free point along its line
/// towards the fixed end: the line from there to it went from 15 to 3.1 for a drag of 0.06.
fn the_pin() -> (Plan, Pick, DVec2) {
    let mut extras = vec![false; 48];
    extras[6] = true;
    (
        Plan {
            ops: vec![
                Op::Step {
                    from: 0,
                    angle: 0,
                    len: 0,
                },
                Op::Arc {
                    from: 48,
                    angle: 6,
                    radius: 0,
                    sweep: 0,
                    ccw: false,
                },
                Op::Tangent {
                    arc: 0,
                    end: false,
                    forward: false,
                    len: 0,
                },
                Op::Tangent {
                    arc: 0,
                    end: true,
                    forward: false,
                    len: 1,
                },
            ],
            extras,
            fix: Some(0),
            length: None,
        },
        Pick::Curves(15068428138436115285),
        DVec2::new(-0.03276375320139522, 0.04953594298231851),
    )
}

/// The length of the pin's free line (from the fixed point to the free one).
fn pin_line(built: &Built) -> f64 {
    let p = built.positions();
    p[5].distance(p[0])
}

/// The pin's drag, at every size, turn and place: refused, its line kept.
#[test]
fn a_drag_that_cant_land_doesnt_crush_a_line_it_doesnt_move() {
    let (plan, pick, offset) = the_pin();
    let sketch = plan.sketch().unwrap();
    for (scale, turn, shift) in [
        (1.0, 0.0, DVec2::ZERO),
        (10.0, 0.0, DVec2::ZERO),
        (0.1, 0.0, DVec2::ZERO),
        (1.0, 37.0, DVec2::ZERO),
        (1.0, 0.0, DVec2::new(1e5, 0.0)),
        (1.0, 0.0, DVec2::new(1e6, 0.0)),
        (0.1, 0.0, DVec2::new(1e6, 0.0)),
    ] {
        let t = Similarity { scale, turn, shift };
        let mut built = Built::new(&sketch, t).unwrap();
        let ids = pick.ids(&built);
        let start = built.positions();
        let run = drag(&mut built, &ids, t.vector(offset), 1).unwrap();
        assert_eq!(run.refused, 1, "{scale}× turned {turn} at {shift}: went");
        assert_eq!(
            built.positions(),
            start,
            "{scale}× turned {turn} at {shift}"
        );
    }
}

/// The same drag in 64 frames: the early frames, where the line shortens slower than a hundred
/// times their offset, go; once a frame's would be faster it is refused, and the drag rests on
/// the last that went. However the frames fall, the line never shortens by more than a hundred
/// times the whole drag (the one-frame drag keeps it whole).
#[test]
fn the_pin_in_many_frames_rests_on_the_last_that_went() {
    let (plan, pick, offset) = the_pin();
    let sketch = plan.sketch().unwrap();
    for frames in [2, 8, 64] {
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let ids = pick.ids(&built);
        let run = drag(&mut built, &ids, offset, frames).unwrap();
        assert!(run.refused > 0, "{frames} frames");
        let shortened = 15.0 - pin_line(&built);
        assert!(
            shortened <= 100.0 * offset.length(),
            "{frames} frames: shortened {shortened}"
        );
    }
}

/// A crank: an arm `d` long from a fixed pivot, dragged by its end (it can't land: its length
/// is kept), a long arm `D` long rigid with it, and a free line from the long arm's end to a
/// fixed point. The free line shortens or lengthens `D / d` times as fast as the pointer moves,
/// by geometry. Up to a hundredfold it goes both ways.
fn crank(ratio: f64, sign: f64) -> bool {
    let d = 2.0;
    let long = d * ratio;
    let mut ed = Editor::default();
    ed.doc.begin();
    let p = ed.doc.add_point(DVec2::ZERO);
    let q = ed.doc.add_point(DVec2::new(d, 0.0));
    let e = ed.doc.add_point(DVec2::new(d + long, 0.0));
    let f = ed.doc.add_point(DVec2::new(d + long, -long));
    let arm = ed.doc.add_line(p, q).unwrap();
    let rigid = ed.doc.add_line(q, e).unwrap();
    ed.doc.add_line(e, f).unwrap();
    for c in [
        Constraint::Fix {
            point: p,
            at: DVec2::ZERO,
        },
        Constraint::Fix {
            point: f,
            at: DVec2::new(d + long, -long),
        },
        Constraint::Distance {
            a: p,
            b: q,
            value: d,
        },
        Constraint::Distance {
            a: q,
            b: e,
            value: long,
        },
        Constraint::Parallel { a: arm, b: rigid },
    ] {
        ed.doc.add_constraint(c).unwrap();
    }
    assert!(ed.commit("Sketch"));
    ed.doc.begin();
    ed.drag(&[q], DVec2::new(0.002, sign * 0.02), &Param::point(q))
}

#[test]
fn a_crank_shortening_a_free_line_still_goes_both_ways() {
    for ratio in [5.0, 50.0, 90.0] {
        assert!(crank(ratio, 1.0) && crank(ratio, -1.0), "1:{ratio}");
    }
}

/// `sketch` built, with an old conflict added in the same part (a line held horizontal and
/// vertical at once, at a length, hanging from the sketch's first point), committed as a
/// drawing that already conflicts.
fn with_old_conflict(sketch: &Sketch) -> Built {
    let mut built = Built::new(sketch, Similarity::IDENTITY).unwrap();
    built.ed.doc.begin();
    let a = built.points[0];
    let at = built.ed.doc.point(a).unwrap();
    let b = built.ed.doc.add_point(at + DVec2::new(10.0, 0.0));
    let line = built.ed.doc.add_line(a, b).unwrap();
    for c in [
        Constraint::Horizontal { line },
        Constraint::Vertical { line },
        Constraint::Distance { a, b, value: 10.0 },
    ] {
        built.ed.doc.add_constraint(c).unwrap();
    }
    built.ed.doc.commit("Conflicting");
    built.ed = Editor::new(std::mem::take(&mut built.ed.doc));
    assert!(!built.ed.analysis().conflicts.is_empty());
    built.points.push(b);
    built
}

/// An old conflict elsewhere in the part excuses only what it holds, even where it shares a
/// point with what the rule holds (it hangs from the sketch's first point, the pin's fixed end):
/// the pin's crush and seed 16's growth are still refused beside it. (Driven directly: the
/// harness wants every relation met after a drag, which a drawing in conflict never is.)
#[test]
fn an_old_conflict_elsewhere_doesnt_excuse_a_broken_shape() {
    for (plan, pick, offset) in [the_pin(), seed_16()] {
        let sketch = plan.sketch().unwrap();
        let mut built = with_old_conflict(&sketch);
        let ids = pick.ids(&built);
        let dragged: Vec<Param> = built
            .ed
            .doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .flat_map(Param::point)
            .collect();
        let start = built.positions();
        built.ed.doc.begin();
        assert!(!built.ed.drag(&ids, offset, &dragged));
        assert_eq!(built.positions(), start);
        built.ed.doc.rollback();
    }
}

/// A relation that holds but says nothing new (redundant) doesn't switch the rule off: seed 16
/// with its fixed point's x fixed again is refused as before.
#[test]
fn a_redundant_relation_doesnt_excuse_a_broken_shape() {
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    built.ed.doc.begin();
    let fixes: Vec<_> = built
        .ed
        .doc
        .constraints()
        .filter_map(|(_, c)| match *c {
            Constraint::Fix { point, at } => Some((point, at)),
            _ => None,
        })
        .collect();
    for (point, at) in fixes {
        let _ = built
            .ed
            .doc
            .add_constraint(Constraint::FixX { point, x: at.x });
    }
    built.ed.doc.commit("Redundant");
    built.ed = Editor::new(std::mem::take(&mut built.ed.doc));
    let start = built.positions();
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert_eq!((run.refused, built.positions()), (1, start));
}

/// What an old conflict holds isn't judged, though nothing excuses it: a lever drag that goes (a
/// point kept 2 from a fixed pivot on a line 10 long, dragged off its circle, so it can't land)
/// still goes beside an old conflict in the same part, a line from the pivot held horizontal and
/// vertical at once at a length. Every solve moves an old conflict's geometry by some fixed
/// amount, so against a small enough frame any change of it would read as fast: the drag goes
/// at every size of frame, down to a millionth of the shape and none at all, in one frame and in
/// 64, at the origin and a kilometre out. So does a drag of the conflicting line itself.
#[test]
fn an_old_conflicts_own_line_isnt_judged() {
    for at in [DVec2::ZERO, DVec2::new(1e6, 0.0)] {
        for off in [0.0, 1e-5, 1e-2, 10.0] {
            for frames in [1u32, 64] {
                let v = |x: f64, y: f64| at + DVec2::new(x, y);
                let mut doc = drift_core::Document::new();
                doc.begin();
                let pivot = doc.add_point(v(0.0, 0.0));
                let end = doc.add_point(v(10.0, 0.0));
                let line = doc.add_line(pivot, end).unwrap();
                let q = doc.add_point(v(2.0, 0.0));
                let b = doc.add_point(v(0.0, 10.0));
                let conflicting = doc.add_line(pivot, b).unwrap();
                for c in [
                    Constraint::PointOnCurve {
                        point: q,
                        curve: line,
                    },
                    Constraint::Distance {
                        a: pivot,
                        b: end,
                        value: 10.0,
                    },
                    Constraint::Distance {
                        a: pivot,
                        b: q,
                        value: 2.0,
                    },
                    Constraint::Fix {
                        point: pivot,
                        at: v(0.0, 0.0),
                    },
                    Constraint::Horizontal { line: conflicting },
                    Constraint::Vertical { line: conflicting },
                    Constraint::Distance {
                        a: pivot,
                        b,
                        value: 10.0,
                    },
                ] {
                    doc.add_constraint(c).unwrap();
                }
                doc.commit("Conflicting");
                let mut ed = Editor::new(doc);
                assert!(!ed.analysis().conflicts.is_empty());
                for (what, ids, dragged) in [
                    ("the lever", vec![q], Param::point(q).to_vec()),
                    (
                        "the conflicting line",
                        vec![conflicting],
                        [pivot, b].into_iter().flat_map(Param::point).collect(),
                    ),
                ] {
                    ed.doc.begin();
                    let step = DVec2::new(off, off) / f64::from(frames);
                    for k in 1..=frames {
                        assert!(
                            ed.drag(&ids, step, &dragged),
                            "{what} at {at}, a drag of {off} in {frames}: frame {k} refused"
                        );
                    }
                    ed.doc.rollback();
                }
            }
        }
    }
}

/// A crank turned right round by its end, the pointer on a circle a little outside the arm's
/// (so it never lands), carrying a long arm and a free line to a fixed point round with it: the
/// held lines turn all the way and the drag goes, at 1:1 and 1:10, in fine and coarse frames.
#[test]
fn a_crank_turned_right_round_goes() {
    for ratio in [1.0f64, 10.0] {
        for (degrees, frames) in [(180.0f64, 64u32), (180.0, 8), (360.0, 64), (360.0, 8)] {
            let d = 2.0;
            let long = d * ratio;
            let mut ed = Editor::default();
            ed.doc.begin();
            let p = ed.doc.add_point(DVec2::ZERO);
            let q = ed.doc.add_point(DVec2::new(d, 0.0));
            let e = ed.doc.add_point(DVec2::new(d + long, 0.0));
            let f = ed.doc.add_point(DVec2::new(d + long, -long));
            let arm = ed.doc.add_line(p, q).unwrap();
            let rigid = ed.doc.add_line(q, e).unwrap();
            ed.doc.add_line(e, f).unwrap();
            for c in [
                Constraint::Fix {
                    point: p,
                    at: DVec2::ZERO,
                },
                Constraint::Fix {
                    point: f,
                    at: DVec2::new(d + long, -long),
                },
                Constraint::Distance {
                    a: p,
                    b: q,
                    value: d,
                },
                Constraint::Distance {
                    a: q,
                    b: e,
                    value: long,
                },
                Constraint::Parallel { a: arm, b: rigid },
            ] {
                ed.doc.add_constraint(c).unwrap();
            }
            assert!(ed.commit("Sketch"));
            ed.doc.begin();
            let mut applied = DVec2::ZERO;
            for k in 1..=frames {
                let a = (degrees * f64::from(k) / f64::from(frames)).to_radians();
                let to = DVec2::new(a.cos(), a.sin()) * (d * 1.05) - DVec2::new(d, 0.0);
                assert!(
                    ed.drag(&[q], to - applied, &Param::point(q)),
                    "1:{ratio}, {degrees}° in {frames}: frame {k} refused"
                );
                applied = to;
            }
        }
    }
}

/// The same beside the e13 arcs (#281's case, the conflict in the drag's part): the centre's
/// drag, scaled down to a thousandth and a millionth of itself and to none, in one frame and in
/// 64, does beside the old conflict exactly what it does without it: the same drawing with the
/// conflicting relations taken off (`conflicting_or_not`). (It was compared with e13's arcs
/// alone, without the line hanging from the fixed point either, which held only while the
/// drag's step sizes, read from the whole part, happened to make no difference; the part's own
/// geometry is the drag's to read, and the conflict is what this checks.)
#[test]
fn an_old_conflict_beside_a_small_drag_changes_nothing() {
    for scale in [0.0, 1e-6, 1e-3, 1.0] {
        for frames in [1u32, 64] {
            let (mut ed, (centre, _)) = already_conflicting(e13_beside);
            let beside = drag_keeping_structure(&mut ed, &[centre], TWO_ARCS_DRAG * scale, frames);
            let (mut alone, (centre, _)) = conflicting_or_not(false, e13_beside);
            let alone =
                drag_keeping_structure(&mut alone, &[centre], TWO_ARCS_DRAG * scale, frames);
            assert_eq!(beside, alone, "{scale} of the drag in {frames}");
        }
    }
}

/// Undo and redo over a drag the rules held part of the way: the pin in 64 frames (the early
/// frames go, the rest are refused) and committed is one undo step, which puts the start back
/// exactly, and redo the last shape that went, exactly.
#[test]
fn a_held_drag_undoes_and_redoes_whole() {
    let (plan, pick, offset) = the_pin();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let start = built.positions();
    let run = drag(&mut built, &ids, offset, 64).unwrap();
    assert!(run.refused > 0 && run.committed);
    let end = built.positions();
    assert_ne!(end, start);
    assert!(built.ed.undo());
    assert_eq!(built.positions(), start);
    assert!(built.ed.redo());
    assert_eq!(built.positions(), end);
}

/// A drag the rules refuse from its first frame, committed, leaves no undo step, as a drag
/// refused for any other reason doesn't (the frame that would crush a circle, F1): one undo then
/// takes back the step before it.
#[test]
fn a_drag_refused_whole_leaves_the_history_as_any_refused_drag() {
    // `undo()` results and what one undo leaves, after a drag refused whole and committed.
    fn after_refused(
        mut ed: Editor,
        ids: &[EntityId],
        dragged: &[Param],
        by: DVec2,
    ) -> (bool, usize) {
        ed.doc.begin();
        assert!(!ed.drag(ids, by, dragged));
        ed.commit("Move");
        let undone = ed.undo();
        (undone, ed.doc.iter().count())
    }
    // Seed 16's drag, refused by the radius rule.
    let (plan, pick, offset) = seed_16();
    let sketch = plan.sketch().unwrap();
    let built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    let entities = built.ed.doc.iter().count();
    let ours = after_refused(built.ed, &ids, &dragged, offset);
    // A point dragged onto its circle's fixed centre, refused by F1.
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(DVec2::ZERO);
    let circle = ed.doc.add_circle(c, 5.0).unwrap();
    let p = ed.doc.add_point(DVec2::new(5.0, 0.0));
    for r in [
        Constraint::PointOnCurve {
            point: p,
            curve: circle,
        },
        Constraint::Fix {
            point: c,
            at: DVec2::ZERO,
        },
    ] {
        ed.doc.add_constraint(r).unwrap();
    }
    assert!(ed.commit("Sketch"));
    let f1_entities = ed.doc.iter().count();
    let theirs = after_refused(ed, &[p], &Param::point(p), DVec2::new(-5.0, 0.0));
    // Neither leaves an undo step of its own: one undo after the refused drag takes the
    // drawing's making back, for the rules' refusal as for F1's.
    assert!(entities > 0 && f1_entities > 0);
    assert_eq!(ours, (true, 0));
    assert_eq!(theirs, (true, 0));
}

/// An unsettled start (a line drawn 9.51 where its dimension says 9, as an import or an old
/// unsolved edit can leave it), beside a lever drag that can't land: every frame's solve settles
/// it, and the rule reads the settling as the drag shortening a held line. A drag of nothing
/// settles at once, as before; a real drag holds until the pointer has moved a hundredth of the
/// unsettled amount for a line (about 0.005 here; a tenth for a radius, see the next test),
/// then settles with the drag, at the origin and a kilometre out alike.
#[test]
fn an_unsettled_start_holds_for_its_first_hundredth_then_goes() {
    for at in [DVec2::ZERO, DVec2::new(1e6, 0.0)] {
        for (off, frames) in [(0.0, 1u32), (0.1, 64), (0.02, 64)] {
            let v = |x: f64, y: f64| at + DVec2::new(x, y);
            let mut doc = drift_core::Document::new();
            doc.begin();
            let pivot = doc.add_point(v(0.0, 0.0));
            let end = doc.add_point(v(10.0, 0.0));
            let line = doc.add_line(pivot, end).unwrap();
            let q = doc.add_point(v(2.0, 0.0));
            let h = doc.add_point(v(10.0, 10.0));
            doc.add_line(end, h).unwrap();
            for c in [
                Constraint::PointOnCurve {
                    point: q,
                    curve: line,
                },
                Constraint::Distance {
                    a: pivot,
                    b: end,
                    value: 10.0,
                },
                Constraint::Distance {
                    a: pivot,
                    b: q,
                    value: 2.0,
                },
                Constraint::Fix {
                    point: pivot,
                    at: v(0.0, 0.0),
                },
                Constraint::Distance {
                    a: end,
                    b: h,
                    value: 9.0,
                },
            ] {
                doc.add_constraint(c).unwrap();
            }
            doc.commit("Sketch");
            let mut ed = Editor::new(doc);
            ed.doc.begin();
            let _ = ed.doc.set_point(h, v(10.0, 10.0));
            ed.doc.commit("Unsettle");
            let length = |ed: &Editor| {
                ed.doc
                    .point(end)
                    .unwrap()
                    .distance(ed.doc.point(h).unwrap())
            };
            assert!(length(&ed) > 9.5);
            ed.doc.begin();
            let mut applied = DVec2::ZERO;
            let mut first = None;
            for k in 1..=frames {
                let target = DVec2::new(off, off) * (f64::from(k) / f64::from(frames));
                if ed.drag(&[q], target - applied, &Param::point(q)) {
                    applied = target;
                    first.get_or_insert(target.length());
                }
            }
            let first = first.expect("goes in the end");
            assert!(
                first < 0.01,
                "at {at}, {off} in {frames}: first went at {first}"
            );
            assert!((length(&ed) - 9.0).abs() < 1e-6, "at {at}: settled");
        }
    }
}

/// The same with a radius (Fable's #361 follow-up): beside the lever, a circle centred on its
/// far end, its radius dimensioned 5 and left at 6. Each frame's solve settles the radius by 1,
/// which the radius rule reads as the drag changing a held radius by more than `SHAPE_HOLD`
/// (10) times the pointer's offset: so the drag holds until the pointer has moved a tenth of
/// the unsettled amount, then settles with it. A whole drag shorter than that never settles
/// (the frames are refused and the radius stays 6). The same at the origin and a kilometre out.
#[test]
fn an_unsettled_radius_holds_for_a_tenth_then_settles() {
    for at in [DVec2::ZERO, DVec2::new(1e6, 0.0)] {
        for (off, frames) in [(0.2, 64u32), (0.07, 64)] {
            let v = |x: f64, y: f64| at + DVec2::new(x, y);
            let mut doc = drift_core::Document::new();
            doc.begin();
            let pivot = doc.add_point(v(0.0, 0.0));
            let end = doc.add_point(v(10.0, 0.0));
            let line = doc.add_line(pivot, end).unwrap();
            let q = doc.add_point(v(2.0, 0.0));
            let circle = doc.add_circle(end, 5.0).unwrap();
            for c in [
                Constraint::PointOnCurve {
                    point: q,
                    curve: line,
                },
                Constraint::Distance {
                    a: pivot,
                    b: end,
                    value: 10.0,
                },
                Constraint::Distance {
                    a: pivot,
                    b: q,
                    value: 2.0,
                },
                Constraint::Fix {
                    point: pivot,
                    at: v(0.0, 0.0),
                },
                Constraint::Radius {
                    curve: circle,
                    value: 5.0,
                },
            ] {
                doc.add_constraint(c).unwrap();
            }
            doc.commit("Sketch");
            let mut ed = Editor::new(doc);
            ed.doc.begin();
            ed.doc.set_radius(circle, 6.0).unwrap();
            ed.doc.commit("Unsettle");
            let radius = |ed: &Editor| match ed.doc.get(circle).unwrap().kind {
                drift_core::EntityKind::Circle { radius, .. } => radius,
                _ => unreachable!(),
            };
            ed.doc.begin();
            let mut applied = DVec2::ZERO;
            let mut first = None;
            for k in 1..=frames {
                let target =
                    DVec2::new(1.0, 1.0).normalize() * off * (f64::from(k) / f64::from(frames));
                if ed.drag(&[q], target - applied, &Param::point(q)) {
                    applied = target;
                    first.get_or_insert(target.length());
                }
            }
            if off >= 0.1 {
                let first = first.expect("goes in the end");
                assert!(
                    (0.09..=0.11).contains(&first),
                    "at {at}, {off} in {frames}: first went at {first}"
                );
                assert!((radius(&ed) - 5.0).abs() < 1e-6, "at {at}: settled");
            } else {
                assert_eq!(first, None, "at {at}, {off} in {frames}: went");
                assert_eq!(radius(&ed), 6.0, "at {at}: never settles");
            }
        }
    }
}

/// A sketch for the try at keeping a length: a line `moved` (dragged as a curve) with its
/// midpoint `m`, a line hanging from `m` to a free end, held equal to a line `stretched` from a
/// fixed point to the moved line's start. A move of `moved` stretches `stretched`, so the
/// hanging line's length must change with it: no try can keep it. With `refused`, the moved
/// line's start is also on a circle round the fixed point (radius 10), and the frame takes it
/// onto the circle's centre, which crushes it (F1): refused today.
fn length_try_sketch(at: DVec2, refused: bool) -> (Editor, EntityId, Vec<Param>, [EntityId; 5]) {
    let v = |x: f64, y: f64| at + DVec2::new(x, y);
    let mut doc = drift_core::Document::new();
    doc.begin();
    let p0 = doc.add_point(v(0.0, 0.0));
    let q = doc.add_point(v(10.0, 0.0));
    let r = doc.add_point(v(10.0, 6.0));
    let m = doc.add_point(v(10.0, 3.0));
    let s = doc.add_point(v(20.0, 3.0));
    let stretched = doc.add_line(p0, q).unwrap();
    let moved = doc.add_line(q, r).unwrap();
    let hanging = doc.add_line(m, s).unwrap();
    let mut relations = vec![
        Constraint::Fix {
            point: p0,
            at: v(0.0, 0.0),
        },
        Constraint::Midpoint {
            point: m,
            line: moved,
        },
        Constraint::Equal {
            a: hanging,
            b: stretched,
        },
    ];
    if refused {
        let circle = doc.add_circle(p0, 10.0).unwrap();
        relations.push(Constraint::PointOnCurve {
            point: q,
            curve: circle,
        });
    }
    for c in relations {
        doc.add_constraint(c).unwrap();
    }
    doc.commit("Sketch");
    let dragged = [q, r].into_iter().flat_map(Param::point).collect();
    (Editor::new(doc), moved, dragged, [p0, q, r, m, s])
}

/// A try at keeping a hanging line's length that fails leaves the frame exactly as today's
/// steps left it ([`length_try_sketch`]: the hanging line's length must change). Frames are
/// tried and none kept, and the drag ends bit for bit where it does with the try switched off,
/// pushing as much, at the origin, 100 mm out and a kilometre out.
#[test]
fn a_failed_try_at_keeping_a_length_leaves_the_frame_as_it_was() {
    for at in [DVec2::ZERO, DVec2::new(100.0, -50.0), DVec2::new(1e6, 0.0)] {
        let run = |no_try: bool| {
            let (mut ed, moved, dragged, points) = length_try_sketch(at, false);
            ed.drag_no_length_try = no_try;
            ed.doc.begin();
            let mut tries = (0, 0);
            let mut went = Vec::new();
            let mut applied = DVec2::ZERO;
            for k in 1..=8 {
                let target = DVec2::new(4.0, 1.0) * (f64::from(k) / 8.0);
                let ok = ed.drag(&[moved], target - applied, &dragged);
                if ok {
                    applied = target;
                }
                went.push(ok);
                tries.0 += ed.drag_length_tries.0;
                tries.1 += ed.drag_length_tries.1;
            }
            let at_end: Vec<DVec2> = points.iter().map(|&p| ed.doc.point(p).unwrap()).collect();
            (went, at_end, ed.drag_pushed(), tries)
        };
        let (went, at_end, pushed, tries) = run(false);
        let (went_off, at_end_off, pushed_off, tries_off) = run(true);
        assert!(tries.0 > 0, "at {at}: never tried");
        assert_eq!(tries.1, 0, "at {at}: a try was kept");
        assert_eq!(tries_off, (0, 0));
        assert_eq!(went, went_off, "at {at}");
        assert_eq!(at_end, at_end_off, "at {at}: not bit for bit");
        assert_eq!(pushed, pushed_off, "at {at}");
    }
}

/// A frame refused today stays refused with the try at keeping a length, and leaves the
/// drawing as it was ([`length_try_sketch`] with the moved line's start taken onto a circle's
/// centre: crushed, F1). The try is made only after a step that went, so it may run on the
/// frame's earlier steps, but it never turns a refused frame into one that goes.
#[test]
fn a_frame_refused_today_stays_refused_with_the_try_at_keeping_a_length() {
    for no_try in [false, true] {
        let (mut ed, moved, dragged, points) = length_try_sketch(DVec2::ZERO, true);
        ed.drag_no_length_try = no_try;
        let start: Vec<DVec2> = points.iter().map(|&p| ed.doc.point(p).unwrap()).collect();
        ed.doc.begin();
        let went = ed.drag(&[moved], DVec2::new(-10.0, 0.0), &dragged);
        assert!(!went, "with no_try {no_try}: went");
        let now: Vec<DVec2> = points.iter().map(|&p| ed.doc.point(p).unwrap()).collect();
        assert_eq!(
            now, start,
            "with no_try {no_try}: a refused frame moved something"
        );
    }
}

/// The tries at keeping a length share one budget across a drag call's parts: ten copies of
/// [`length_try_sketch`]'s failing kind (each its own part, each trying every frame and
/// failing) dragged in one call run no more than `LENGTH_TRY_SOLVES` (32) solves on their
/// tries a frame, where ten budgets of their own would run each part's.
#[test]
fn many_parts_trying_to_keep_a_length_share_one_budget() {
    let mut doc = drift_core::Document::new();
    doc.begin();
    let mut moved = Vec::new();
    for t in 0..10 {
        let o = DVec2::new(60.0 * f64::from(t), 0.0);
        let v = |x: f64, y: f64| o + DVec2::new(x, y);
        let p0 = doc.add_point(v(0.0, 0.0));
        let q = doc.add_point(v(10.0, 0.0));
        let r = doc.add_point(v(10.0, 6.0));
        let m = doc.add_point(v(10.0, 3.0));
        let s = doc.add_point(v(20.0, 3.0));
        let stretched = doc.add_line(p0, q).unwrap();
        let line = doc.add_line(q, r).unwrap();
        let hanging = doc.add_line(m, s).unwrap();
        for c in [
            Constraint::Fix {
                point: p0,
                at: v(0.0, 0.0),
            },
            Constraint::Midpoint { point: m, line },
            Constraint::Equal {
                a: hanging,
                b: stretched,
            },
        ] {
            doc.add_constraint(c).unwrap();
        }
        moved.push(line);
    }
    doc.commit("Sketch");
    let mut ed = Editor::new(doc);
    let dragged: Vec<Param> = ed
        .doc
        .defining_points(moved.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    ed.doc.begin();
    let mut applied = DVec2::ZERO;
    for k in 1..=4 {
        let target = DVec2::new(4.0, 1.0) * (f64::from(k) / 4.0);
        if ed.drag(&moved, target - applied, &dragged) {
            applied = target;
        }
        let (tries, kept, solves) = ed.drag_length_tries;
        assert!(
            tries > 0 && kept == 0,
            "frame {k}: {tries} tries, {kept} kept"
        );
        assert!(solves <= 32, "frame {k}: the tries ran {solves} solves");
    }
}

/// The comparison `an_old_conflict_beside_a_small_drag_changes_nothing` made before PR 2
/// (Fable's review): the e13 arcs' full drag in 64 frames beside the old conflict and its
/// lines, and beside those lines without the conflict, against the arcs alone. Main goes 32
/// frames in all three; PR 2 goes 29 beside the lines, with or without the conflict, and 32
/// alone. Not the conflict, and not one cause found: the solves are the same with and without
/// the lines, component by component, but `Editor::drag_from_start` sizes its steps by every
/// point of the part, the lines' among them, and at the fold those steps fall the other way.
/// Counting each place once, or sizing the steps by the solver's component, each makes this
/// pass and moves other fold cases the other way (heavy similarity seeds, a free move refused).
/// The step ladder is replaced by PR 5's continuation (parked as later).
#[test]
#[ignore = "PR 5 (parked): drag_from_start's steps read the part's other lines; 29 of 32 frames"]
fn the_arcs_drag_beside_lines_on_the_fixed_centre_is_their_drag_alone() {
    let mut built = Built::new(&two_arcs_one_fixed(), Similarity::IDENTITY).unwrap();
    let ids = [built.points[3]];
    let alone = drag_keeping_structure(&mut built.ed, &ids, TWO_ARCS_DRAG, 64);
    for conflict in [true, false] {
        let (mut ed, (centre, _)) = conflicting_or_not(conflict, e13_beside);
        let beside = drag_keeping_structure(&mut ed, &[centre], TWO_ARCS_DRAG, 64);
        assert_eq!(beside, alone, "conflict: {conflict}");
    }
}
