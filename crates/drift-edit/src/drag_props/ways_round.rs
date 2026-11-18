//! #344: a drag keeps each Parallel, Perpendicular and Angle pair the way round it started.
//!
//! The angle equation was undirected (its residual the sine of the angle error, zero both ways
//! round), so a big step that turned a line past square to its partner headed for the other
//! root. Each sketch here: a fixed line, and a free line of kept length related to it. One end
//! of the free line is dragged past the other. Moving the free line along keeps the pair as it
//! was; the far root has its other end stay near where it was and the line turn round. Nothing
//! holds the dragged point, so #331's carry doesn't step in. (A drag solves each frame from
//! the drag's start, so it flipped the same in 1 step or 120.)

use super::*;

/// The signed angle (radians) from line `a` to line `b` of `sketch`, at `points`.
fn turn_between(sketch: &Sketch, points: &[DVec2], a: usize, b: usize) -> f64 {
    let dir = |l: usize| match sketch.curves[l] {
        Curve::Line(p, q) => points[q] - points[p],
        Curve::Arc(..) => unreachable!("lines only"),
    };
    let (u, v) = (dir(a), dir(b));
    u.perp_dot(v).atan2(u.dot(v))
}

/// Drags point `point` of `built` by `offset` in `steps` steps, and checks that the drag
/// committed with the point on its target, and that lines 0 and 1 are still the way round they
/// started: the signed angle between them changed by under 90°.
fn keeps_its_way_round(mut built: Built, point: usize, offset: DVec2, steps: u32) {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let id = built.points[point];
    let run = drag(&mut built, &[id], offset, steps).unwrap();
    let before = turn_between(&built.sketch, &run.start, 0, 1);
    let after = turn_between(&built.sketch, &run.end, 0, 1);
    let change = (after - before + PI).rem_euclid(TAU) - PI;
    assert!(
        change.abs() < FRAC_PI_2,
        "the pair turned round: {:.1}° to {:.1}° (committed: {}; points {:?} to {:?})",
        before.to_degrees(),
        after.to_degrees(),
        run.committed,
        run.start,
        run.end,
    );
    let want = run.start[point] + offset;
    assert!(
        run.committed && close(run.end[point], want, want.length()),
        "the dragged point ended at {} instead of {want} (committed: {})",
        run.end[point],
        run.committed,
    );
}

fn parallel_pair() -> Built {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, -10.0), v(20.0, -10.0), v(0.0, 0.0), v(10.0, 0.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
        rels: vec![
            Rel::Fix(0),
            Rel::Fix(1),
            Rel::Parallel(0, 1),
            Rel::Length(1),
        ],
    };
    Built::new(&sketch, Similarity::IDENTITY).unwrap()
}

fn perpendicular_pair() -> Built {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, -10.0), v(20.0, -10.0), v(0.0, 0.0), v(0.0, 10.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
        rels: vec![
            Rel::Fix(0),
            Rel::Fix(1),
            Rel::Perpendicular(0, 1),
            Rel::Length(1),
        ],
    };
    Built::new(&sketch, Similarity::IDENTITY).unwrap()
}

fn angle_pair() -> Built {
    let v = DVec2::new;
    let r = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let sketch = Sketch {
        points: vec![v(0.0, -10.0), v(20.0, -10.0), v(0.0, 0.0), v(r, r)],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Length(1)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let angle = Constraint::Angle {
        a: built.curves[0],
        b: built.curves[1],
        degrees: 45.0,
    };
    built.ed.add_constraints("Angle", &[angle]).unwrap();
    built
}

/// A fixed line and a free line parallel to it, its length kept, dragged by its end 30 to the
/// left (past its start, 10 away).
#[test]
fn parallel_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(parallel_pair(), 3, DVec2::new(-30.0, 5.0), 1);
}

/// As the Parallel case, square: a free line perpendicular to a fixed one, its length kept,
/// dragged by its end 30 down (past its start, 10 below it).
#[test]
fn perpendicular_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(perpendicular_pair(), 3, DVec2::new(5.0, -30.0), 1);
}

/// As the Parallel case, at 45°: a free line at a 45° Angle to a fixed one, its length kept,
/// dragged by its end 20 left and 20 down (past its start, 10 away).
#[test]
fn angle_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(angle_pair(), 3, DVec2::new(-20.0, -20.0), 1);
}

/// The same three drags in many small steps: each frame solves from the drag's start, so the
/// way round is the one the drag began with however the pointer got there.
#[test]
fn pairs_keep_their_way_round_in_many_steps() {
    keeps_its_way_round(parallel_pair(), 3, DVec2::new(-30.0, 5.0), 40);
    keeps_its_way_round(perpendicular_pair(), 3, DVec2::new(5.0, -30.0), 40);
    keeps_its_way_round(angle_pair(), 3, DVec2::new(-20.0, -20.0), 40);
}

/// The stop rule (#344): where only the other way round reaches the pointer, or keeping the
/// pair would reshape its lines, the drag stops at the last shape that keeps the pair. A leg
/// square to a fixed line, its foot fixed and its length free, dragged by its end down past
/// the corner: on main it flipped through the corner and followed the pointer. Now it stops
/// at the corner, never past it and never off on the far side (a one-step drag past the
/// corner leaves the leg where it was; in steps it goes down to the corner and stays).
#[test]
fn a_leg_dragged_past_its_corner_stops_there() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Perpendicular(0, 1)],
    };
    for steps in [1, 20] {
        for dy in [-5.01, -6.0, -8.0, -10.0, -30.0] {
            let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
            let id = built.points[2];
            let run = drag(&mut built, &[id], v(0.0, dy), steps).unwrap();
            let end = run.end[2];
            assert!(run.committed, "{steps} steps, {dy}: not committed");
            assert!(
                (end.x - 10.0).abs() < 1e-9 && (0.0..=5.0).contains(&end.y),
                "{steps} steps, {dy}: the leg's end ended at {end}, not between the corner and \
                 where it began"
            );
            if steps == 20 {
                // (Within a frame's move of the corner: the drag stops at its last frame that
                // keeps the pair.)
                assert!(
                    end.y <= dy.abs() / 20.0 + 1e-9,
                    "{steps} steps, {dy}: stopped at {end}, short of the corner"
                );
            }
        }
    }
    // Short of the corner, the drag follows the pointer as before.
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let id = built.points[2];
    let run = drag(&mut built, &[id], v(0.0, -4.0), 1).unwrap();
    assert!(
        close(run.end[2], v(10.0, 1.0), 10.0),
        "ended at {}",
        run.end[2]
    );
}

/// The stop rule (#344): a free line parallel to a fixed one, its length free, dragged by its
/// end past its start. Keeping the pair the way round it began, nothing says where the start
/// should go (on a first build the solve's path flung it tens of millimetres ahead of the
/// pointer, a different place for each pointer position), so the drag stops with the end at
/// the start: never flung, never the other way round. With the start fixed, the same.
#[test]
fn a_free_line_dragged_past_its_start_stops_there() {
    let v = DVec2::new;
    for fixed_start in [false, true] {
        let mut rels = vec![Rel::Fix(0), Rel::Fix(1), Rel::Parallel(0, 1)];
        if fixed_start {
            rels.push(Rel::Fix(2));
        }
        let sketch = Sketch {
            points: vec![v(0.0, -10.0), v(20.0, -10.0), v(0.0, 0.0), v(10.0, 0.0)],
            curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
            rels,
        };
        for steps in [1, 20] {
            for dx in [-10.1, -11.0, -14.6, -20.0, -40.0] {
                let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
                let id = built.points[3];
                let run = drag(&mut built, &[id], v(dx, 0.5), steps).unwrap();
                let (start, end) = (run.end[2], run.end[3]);
                let what = format!("start fixed {fixed_start}, {steps} steps, {dx}");
                assert!(run.committed, "{what}: not committed");
                assert!(
                    end.x > start.x,
                    "{what}: the pair turned round: start {start}, end {end}"
                );
                assert!(
                    start.distance(v(0.0, 0.0)) < 1.0 && end.x <= 10.0,
                    "{what}: the line went off: start {start}, end {end}"
                );
            }
        }
    }
}

/// A move to a typed place (the inspector's coordinates, a typed line angle, the AI link's
/// move_point) keeps no pair's way round: the place typed may be the other way round outright,
/// and it is met as main met it. The leg kept square to a fixed line is turned to -90°.
#[test]
fn a_typed_move_may_turn_a_pair_round() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Perpendicular(0, 1)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let leg = built.curves[1];
    assert!(
        built.ed.set_line_angle(leg, -90.0),
        "the typed angle was refused"
    );
    let end = built.ed.doc.point(built.points[2]).unwrap();
    assert!(close(end, v(10.0, -5.0), 10.0), "the leg's end is at {end}");
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let id = built.points[2];
    assert!(
        built.ed.move_point_to(id, v(10.0, -3.0)),
        "the typed move was refused"
    );
    let end = built.ed.doc.point(id).unwrap();
    assert!(close(end, v(10.0, -3.0), 10.0), "the leg's end is at {end}");
    // And a drag after it keeps the pair again.
    let run = drag(&mut built, &[id], v(0.0, 10.0), 1).unwrap();
    assert!(
        run.end[2].y <= 0.0,
        "the drag after turned the pair: {}",
        run.end[2]
    );
}

/// Known gap (#344, second review; PR 5's continuation): a frozen pair the solve turns, rather
/// than the dragged points, isn't judged by the stop rule. A free line Q→R parallel to a fixed
/// one, its length free, its start Q pulled along by a horizontal bar of kept length from P,
/// and P dragged right past R: nothing dragged is on Q→R, so the moved geometry doesn't lie
/// the other way round, and the solve flings R 19 to 29 mm ahead of Q, a different place for
/// each pointer position (main turned the pair round instead). A check after the solve (an end
/// past where the other was) also stopped a Perpendicular pair swinging round whole
/// (`seeds::l_a_point_on_a_fixed_centre_swings_its_radius_round`): one step can't tell the two
/// apart.
#[test]
#[ignore = "known failure: #344's stop rule misses a pair the solve turns (PR 5)"]
fn a_pair_the_solve_turns_stops_the_drag_too() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![
            v(0.0, -10.0),
            v(20.0, -10.0),
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(-5.0, 0.0),
        ],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3), Curve::Line(4, 2)],
        rels: vec![
            Rel::Fix(0),
            Rel::Fix(1),
            Rel::Parallel(0, 1),
            Rel::Horizontal(2),
            Rel::Length(2),
        ],
    };
    for steps in [1, 20] {
        for dx in [12.0, 20.0, 40.0] {
            let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
            let id = built.points[4];
            let run = drag(&mut built, &[id], v(dx, 0.5), steps).unwrap();
            let (q, r) = (run.end[2], run.end[3]);
            assert!(run.committed, "{steps} steps, {dx}: not committed");
            assert!(
                r.x > q.x && r.x <= 11.0,
                "{steps} steps, {dx}: Q at {q}, R at {r}"
            );
        }
    }
}

/// The same gap with two pairs in one part: a free line T→S parallel to a fixed one, its length
/// free, and a leg S→U of kept length square to the fixed line. Dragging U takes the leg along
/// whole (a pair the step asks to turn, met by a move along), while T→S, which the step doesn't
/// ask, has T flung.
#[test]
#[ignore = "known failure: #344's stop rule misses a pair the solve turns (PR 5)"]
fn a_second_pair_turned_by_the_solve_stops_the_drag() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![
            v(0.0, -10.0),
            v(20.0, -10.0),
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(10.0, 5.0),
        ],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3), Curve::Line(3, 4)],
        rels: vec![
            Rel::Fix(0),
            Rel::Fix(1),
            Rel::Parallel(0, 1),
            Rel::Perpendicular(0, 2),
            Rel::Length(2),
        ],
    };
    for steps in [1, 20] {
        for off in [v(-15.0, -10.0), v(-30.0, -10.0), v(-15.0, 2.0)] {
            let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
            let id = built.points[4];
            let run = drag(&mut built, &[id], off, steps).unwrap();
            let (t, s) = (run.end[2], run.end[3]);
            assert!(run.committed, "{steps} steps, {off}: not committed");
            assert!(
                s.x > t.x && t.x >= -1.0,
                "{steps} steps, {off}: T at {t}, S at {s}"
            );
        }
    }
}

/// Moves of curves by a typed amount are moves to a typed place too (#344, second review):
/// a nudge, and a move as the AI link makes it (`Editor::as_typed` around one drag), of a cap
/// line on a leg kept square to a fixed line, down past the corner, are met as on main.
#[test]
fn typed_moves_of_curves_may_turn_a_pair_round() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(15.0, 5.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2), Curve::Line(2, 3)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Perpendicular(0, 1)],
    };
    // A nudge.
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    built.ed.selection = [built.curves[2]].into_iter().collect();
    assert_eq!(
        built.ed.nudge(v(0.0, -8.0)),
        Some(true),
        "the nudge was refused"
    );
    let end = built.ed.doc.point(built.points[2]).unwrap();
    assert!(close(end, v(10.0, -3.0), 10.0), "nudged to {end}");
    // As the AI link's move: one drag, refused means rolled back.
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[2]];
    let dragged = dragged_by(&built.ed, &ids);
    built.ed.doc.begin();
    assert!(
        built
            .ed
            .as_typed(|ed| ed.drag(&ids, v(0.0, -8.0), &dragged)),
        "the AI link's move was refused"
    );
    let end = built.ed.doc.point(built.points[2]).unwrap();
    assert!(close(end, v(10.0, -3.0), 10.0), "moved to {end}");
    built.ed.doc.rollback();
    // The same move as a pointer's drag keeps the pair: it stops at the corner.
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[2]];
    let run = drag(&mut built, &ids, v(0.0, -8.0), 20).unwrap();
    assert!(
        (0.0..=5.0).contains(&run.end[2].y),
        "the drag went past the corner: {}",
        run.end[2]
    );
}
