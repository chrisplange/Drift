//! #344: a drag should keep each Parallel, Perpendicular and Angle pair the way round it
//! started, and never fling geometry doing it.
//!
//! - **Pins** (ignored, `known failure: #344`): what #344's fix must do. The angle equation is
//!   undirected (its residual the sine of the angle error, zero both ways round), so a big step
//!   that turns a line past square to its partner heads for the other root.
//! - **Guards** (run normally, pass on main): drags and typed moves main gets right that a fix
//!   must not break. Two attempts are parked on branch `claude/pair-way-round-344`. The first
//!   (directed angle rows in every solve, `d30c126`) broke all seven, and refused the commit
//!   in `drift-solver`'s `a_commit_may_turn_a_pair_round`. The second (directed rows in drags
//!   only, with a stop rule, `0802570`) kept typed moves and nudges as main has them but broke
//!   the five drag guards: lines flung or stretched, a linkage jumping, a sliver refused.
//!   Keeping a way round needs the drag to follow its path from the start (the solver audit's
//!   PR 5), not one step at a time.
//!
//! ```text
//! cargo test --release -p drift-edit --lib drag_props::ways_round -- --include-ignored
//! ```

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
#[ignore = "known failure: #344"]
fn parallel_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(parallel_pair(), 3, DVec2::new(-30.0, 5.0), 1);
}

/// As the Parallel case, square: a free line perpendicular to a fixed one, its length kept,
/// dragged by its end 30 down (past its start, 10 below it).
#[test]
#[ignore = "known failure: #344"]
fn perpendicular_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(perpendicular_pair(), 3, DVec2::new(5.0, -30.0), 1);
}

/// As the Parallel case, at 45°: a free line at a 45° Angle to a fixed one, its length kept,
/// dragged by its end 20 left and 20 down (past its start, 10 away).
#[test]
#[ignore = "known failure: #344"]
fn angle_344_pair_keeps_its_way_round_on_a_big_drag() {
    keeps_its_way_round(angle_pair(), 3, DVec2::new(-20.0, -20.0), 1);
}

/// The Parallel drag in 40 small steps: each frame solves from the drag's start, so the way
/// round is the one the drag began with however the pointer got there.
#[test]
#[ignore = "known failure: #344"]
fn parallel_344_pair_keeps_its_way_round_in_many_steps() {
    keeps_its_way_round(parallel_pair(), 3, DVec2::new(-30.0, 5.0), 40);
}

/// The Perpendicular drag in 40 small steps.
#[test]
#[ignore = "known failure: #344"]
fn perpendicular_344_pair_keeps_its_way_round_in_many_steps() {
    keeps_its_way_round(perpendicular_pair(), 3, DVec2::new(5.0, -30.0), 40);
}

/// The Angle drag in 40 small steps.
#[test]
#[ignore = "known failure: #344"]
fn angle_344_pair_keeps_its_way_round_in_many_steps() {
    keeps_its_way_round(angle_pair(), 3, DVec2::new(-20.0, -20.0), 40);
}

/// Where only the other way round reaches the pointer, the drag should stop at the last shape
/// that keeps the pair (the binding drag rule). A leg square to a fixed line, its foot fixed
/// and its length free, dragged by its end down past the corner: main flips it through the
/// corner and follows the pointer. It should stop at the corner, never past it and never off
/// on the far side (a first attempt put the end at (10, 9.04) for a drag of 6 down).
#[test]
#[ignore = "known failure: #344"]
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
                close(end, v(10.0, end.y), 10.0) && (0.0..=5.0).contains(&end.y),
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

/// A free line parallel to a fixed one, its length free, dragged by its end past its start.
/// Keeping the pair the way round it began, nothing says where the start should go (a first
/// attempt flung it tens of millimetres ahead of the pointer, a different place for each
/// pointer position), so the drag should stop with the end at the start: never flung, never
/// the other way round. With the start fixed, the same.
#[test]
#[ignore = "known failure: #344"]
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

/// Guard: a move to a typed place (the inspector's coordinates, a typed line angle, the AI
/// link's move_point) may turn a pair round: the place typed may be the other way round
/// outright. The leg kept square to a fixed line is turned to -90°.
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
}

/// A pair the solve turns, rather than the dragged points: a free line Q→R parallel to a fixed
/// one, its length free, its start Q pulled along by a horizontal bar of kept length from P,
/// and P dragged right past R. Nothing dragged is on Q→R. Main turns the pair round; a first
/// attempt flung R 19 to 29 mm ahead of Q, a different place for each pointer position. The
/// drag should stop with Q short of R.
#[test]
#[ignore = "known failure: #344"]
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

/// The same with two pairs in one part: a free line T→S parallel to a fixed one, its length
/// free, and a leg S→U of kept length square to the fixed line. Dragging U can take the leg
/// along whole, while T→S would have to turn round or have T flung: the drag should stop.
#[test]
#[ignore = "known failure: #344"]
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

/// Guard: a nudge (a move of curves by a typed amount) may turn a pair round too: a cap line on
/// a leg kept square to a fixed line, nudged down past the corner.
#[test]
fn a_nudge_may_turn_a_pair_round() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(15.0, 5.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2), Curve::Line(2, 3)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Perpendicular(0, 1)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    built.ed.selection = [built.curves[2]].into_iter().collect();
    assert_eq!(
        built.ed.nudge(v(0.0, -8.0)),
        Some(true),
        "the nudge was refused"
    );
    let end = built.ed.doc.point(built.points[2]).unwrap();
    assert!(close(end, v(10.0, -3.0), 10.0), "nudged to {end}");
}

/// The free line dragged past its start with the pointer 5 to the side, in 20 frames: where
/// its other end ends should change a little when the pointer does. Main turns the pair round
/// and moves the end by jumps; a first attempt jumped it 6 mm for 0.25 mm of pointer.
#[test]
#[ignore = "known failure: #344"]
fn a_free_line_past_its_start_moves_continuously() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, -10.0), v(20.0, -10.0), v(0.0, 0.0), v(10.0, 0.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Parallel(0, 1)],
    };
    let mut starts = Vec::new();
    for dx in [-10.0, -10.25, -11.25, -11.5] {
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let id = built.points[3];
        let run = drag(&mut built, &[id], v(dx, 5.0), 20).unwrap();
        starts.push((dx, run.end[2]));
    }
    for w in starts.windows(2) {
        let ((dx0, s0), (dx1, s1)) = (w[0], w[1]);
        if dx1 - dx0 == -0.25 {
            assert!(
                s0.distance(s1) < 1.0,
                "the other end jumped {:.2} mm for 0.25 mm of pointer ({dx0} to {dx1})",
                s0.distance(s1)
            );
        }
    }
}

/// Guard (census case 169): a move of curves where no pair turns round. Line 2 (its far end
/// free) is 20 mm long; main leaves it 20.3 mm. A first attempt stretched it to 45 mm, its free
/// end 47 mm from where it began for a drag of 16.6 mm.
#[test]
fn a_hanging_line_is_not_stretched_where_no_pair_turns() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![
            v(0.0, 0.0),
            v(-14.488887394336023, 3.882285676537815),
            v(-17.32050807568877, -10.000000000000002),
            v(-12.144127173638356, 9.318516525781364),
            v(-7.4999999999999964, 12.99038105676658),
            v(-19.908698526713977, -19.659258262890685),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(0, 2),
            Curve::Line(2, 3),
            Curve::Line(0, 4),
            Curve::Line(2, 5),
        ],
        rels: vec![Rel::Perpendicular(0, 2), Rel::Parallel(2, 4), Rel::Fix(5)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [0, 1, 3, 4].map(|i| built.curves[i]);
    let off = v(16.32499277488565, -2.9653276420462706);
    let run = drag(&mut built, &ids, off, 18).unwrap();
    let (was, now) = (
        run.start[2].distance(run.start[3]),
        run.end[2].distance(run.end[3]),
    );
    assert!(now < 1.5 * was, "line 2 went from {was:.2} to {now:.2} mm");
}

/// Guard (census case 297, without its arc): two lines from a fixed corner, square to each
/// other, one of kept length, moved as curves. Main ends near the targets (turning the pair
/// round). A first attempt flung the far line's end 55 mm for a drag of 22 mm.
#[test]
fn a_move_about_a_fixed_corner_is_not_flung() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![
            v(0.0, 0.0),
            v(-5.000000000000005, -8.660254037844386),
            v(-25.98076211353316, 14.999999999999998),
        ],
        curves: vec![Curve::Line(0, 1), Curve::Line(0, 2)],
        rels: vec![Rel::Perpendicular(0, 1), Rel::Fix(0), Rel::Length(0)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[0], built.curves[1]];
    let off = v(17.680336465644732, 13.641959417546362);
    let run = drag(&mut built, &ids, off, 8).unwrap();
    let moved = run.start[2].distance(run.end[2]);
    assert!(
        moved < 1.5 * off.length(),
        "point 2 moved {moved:.1} mm for a drag of {:.1} mm",
        off.length()
    );
}

/// Guard: a parallelogram linkage (base fixed, three kept lengths, crank Parallel to rocker),
/// the crank's end dragged to places on a circle of radius 12 about its pivot (the crank is 10
/// long, so the point can't land; no pair turns). Where the crank ends should move a little
/// when the pointer does: on main at most 2.4° for 2.5° of pointer, and that at the first
/// sample, from the start pose, where all four points lie on the x axis: the parallelogram's
/// collinear change-point, the one place a path-following drag may rightly pick its branch
/// differently (check there first if this fails only from 0° to 2.5°). The second attempt
/// jumped 31.5° (at 77.5°).
#[test]
fn a_linkage_follows_the_pointer_continuously() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(20.0, 0.0), v(10.0, 0.0), v(30.0, 0.0)],
        curves: vec![Curve::Line(0, 2), Curve::Line(2, 3), Curve::Line(1, 3)],
        rels: vec![
            Rel::Fix(0),
            Rel::Fix(1),
            Rel::Parallel(0, 2),
            Rel::Length(0),
            Rel::Length(1),
            Rel::Length(2),
        ],
    };
    let mut last: Option<f64> = None;
    let mut worst = (0.0f64, 0.0f64);
    let mut d = 0.0f64;
    while d <= 90.0 {
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let id = built.points[2];
        let a = d.to_radians();
        let off = v(12.0 * a.cos(), 12.0 * a.sin()) - v(10.0, 0.0);
        let run = drag(&mut built, &[id], off, 1).unwrap();
        let at = run.end[2].y.atan2(run.end[2].x).to_degrees();
        if let Some(was) = last
            && (at - was).abs() > worst.0
        {
            worst = ((at - was).abs(), d);
        }
        last = Some(at);
        d += 2.5;
    }
    assert!(
        worst.0 < 10.0,
        "the crank jumped {:.1}° for 2.5° of pointer, at {}°",
        worst.0,
        worst.1
    );
}

/// Guard: a sliver has no way round to keep. A leg 1e-9 long square to a fixed line is dragged
/// up and down by 2 in 10 frames: main goes both ways. The second attempt held it to a way
/// round and refused every frame down. The threshold is the part's solve bar, not ε: for this
/// 10 mm part the bar is 1e-9 × a quarter of its size, 2.5e-9 mm (solver decision 2), so the
/// leg is under it; a line no longer than the bar has no way round to keep (a test scaled to
/// ε, about 1e-13 here, would wrongly keep this one's).
#[test]
fn a_sliver_goes_either_way() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 1e-9)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2)],
        rels: vec![Rel::Fix(0), Rel::Fix(1), Rel::Perpendicular(0, 1)],
    };
    for dy in [2.0, -2.0] {
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let id = built.points[2];
        let run = drag(&mut built, &[id], v(0.0, dy), 10).unwrap();
        assert!(
            run.refused == 0 && (run.end[2].y - dy).abs() < 1e-6,
            "dragged by {dy}: refused {} of 10 frames, ended at {}",
            run.refused,
            run.end[2]
        );
    }
}

/// Guard: a free run of four lines, each square to the next, nothing fixed, its second corner
/// dragged down 1 mm a frame. Main moves everything under 3 mm a frame (turning a pair round
/// as the corner passes the first one); a first attempt threw the first line 7 mm in a frame.
#[test]
fn a_run_of_square_lines_is_not_thrown() {
    let v = DVec2::new;
    let sketch = Sketch {
        points: vec![
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(10.0, 5.0),
            v(30.0, 5.0),
            v(30.0, 20.0),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(1, 2),
            Curve::Line(2, 3),
            Curve::Line(3, 4),
        ],
        rels: vec![
            Rel::Perpendicular(0, 1),
            Rel::Perpendicular(1, 2),
            Rel::Perpendicular(2, 3),
        ],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let id = built.points[2];
    let dragged: Vec<Param> = Param::point(id).into_iter().collect();
    built.ed.doc.begin();
    let mut applied = DVec2::ZERO;
    let mut last = built.positions();
    for k in 1..=8 {
        let target = v(0.0, -f64::from(k));
        if built.ed.drag(&[id], target - applied, &dragged) {
            applied = target;
        }
        let now = built.positions();
        let jump = last
            .iter()
            .zip(&now)
            .map(|(a, b)| a.distance(*b))
            .fold(0.0, f64::max);
        assert!(
            jump < 3.0,
            "frame {k}: a point jumped {jump:.2} mm for 1 mm of pointer"
        );
        last = now;
    }
}
