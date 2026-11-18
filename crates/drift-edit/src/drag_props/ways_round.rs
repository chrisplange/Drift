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

/// A drag that turns the free line right round its fixed start (its end taken in a half
/// circle to the other side) can't keep the pair: the free line is held to its partner's
/// direction, so its end can only slide along it. Dragging the end straight across to the far
/// side asks the pair to flip, which the drag must not do: wherever it stops, the pair is still
/// the way round it began.
#[test]
fn a_drag_asking_for_the_flip_never_takes_it() {
    let mut built = parallel_pair();
    // Hold the free line's start too: now only its end can move, along the line.
    let start = built.points[2];
    let at = built.ed.doc.point(start).unwrap();
    built
        .ed
        .add_constraints("Fix", &[Constraint::Fix { point: start, at }])
        .unwrap();
    let id = built.points[3];
    let run = drag(&mut built, &[id], DVec2::new(-20.0, 0.0), 10).unwrap();
    let before = turn_between(&built.sketch, &run.start, 0, 1);
    let after = turn_between(&built.sketch, &run.end, 0, 1);
    let change = (after - before + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    assert!(
        change.abs() < std::f64::consts::FRAC_PI_2,
        "the pair turned round: {:.1}° to {:.1}°",
        before.to_degrees(),
        after.to_degrees(),
    );
}
