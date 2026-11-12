//! Known failures, pinned so each open bug has a test that fails on main: every test here is
//! ignored with its issue number, and fails when run with `--ignored`. The fix for an issue
//! un-ignores its tests (and moves them beside the others of their kind) as its proof.
//!
//! ```text
//! cargo test --release -p drift-edit --lib drag_props::known_failures -- --ignored
//! ```

use super::*;

/// A shrunk plan's `extras`: 48 flags, those at `on` set.
fn extras(on: &[usize]) -> Vec<bool> {
    let mut out = vec![false; 48];
    for &i in on {
        out[i] = true;
    }
    out
}

// #295: `face_dimensions_hold_through_drags` flakes on CI. Not a solver bug: a walls-join bug.
// On a large one-axis drag in five steps, a wall end that isn't dragged is rebuilt teeing into
// a different wall than before ("a join flipped"). The inputs are the minimal inputs the issue
// lists, run straight through the property's check, so they replay without a seed. They are
// not saved in `proptest-regressions/drag_props.txt`: the normal run replays that file, and it
// would turn red. (The issue's other form, kind 0 / which 2, a face relation lost at step 5,
// passes on main: its Windows input is `face_dimension_holds_dragging_a_wall_end_along_it`,
// and its Ubuntu input, dx = 1384.593412508991, passes too.)

/// #295, macOS CI run 40464700334 (main, 10-27): "a join flipped at step 5".
#[test]
#[ignore = "known failure: #295"]
fn walls_295_t_join_flips_on_a_long_drag_macos() {
    face_dimensions_hold(1, 3, DVec2::new(1277.8635978624748, 0.0), 5).unwrap();
}

/// #295, Windows CI run 40748487402 (main, 10-28): "a join flipped at step 5".
#[test]
#[ignore = "known failure: #295"]
fn walls_295_t_join_flips_on_a_long_drag_windows() {
    face_dimensions_hold(1, 3, DVec2::new(1493.6212663536342, 0.0), 5).unwrap();
}

/// #295, saved on main 554a995 (cc c641e0fc...): "a join flipped at step 5".
#[test]
#[ignore = "known failure: #295"]
fn walls_295_t_join_flips_dragging_the_host_end() {
    face_dimensions_hold(1, 0, DVec2::new(1296.6028121310637, 0.0), 5).unwrap();
}

/// #295, saved on main f9a1019 (cc c0d21904...): "a join flipped at step 5".
#[test]
#[ignore = "known failure: #295"]
fn walls_295_t_join_flips_on_a_diagonal_drag() {
    face_dimensions_hold(1, 4, DVec2::new(-826.3571639170342, 1052.2014842144613), 5).unwrap();
}

// #344: a big drag step flips a Parallel, Perpendicular or Angle pair to the other way round.
// The angle equation is undirected (its residual is the sine of the angle error, zero both
// ways round), and nothing in these sketches holds a dragged point, so #331's carry doesn't
// step in. Each sketch: a fixed line, and a free line of kept length related to it. One end
// of the free line is dragged past the other. Moving the free line along keeps the pair as it
// was, but the nearer root has its other end stay near where it was and the line turn round.
// (A drag solves each frame from the drag's start, so it flips the same in 1 step or 120.)

/// The signed angle (radians) from line `a` to line `b` of `sketch`, at `points`.
fn turn_between(sketch: &Sketch, points: &[DVec2], a: usize, b: usize) -> f64 {
    let dir = |l: usize| match sketch.curves[l] {
        Curve::Line(p, q) => points[q] - points[p],
        Curve::Arc(..) => unreachable!("lines only"),
    };
    let (u, v) = (dir(a), dir(b));
    u.perp_dot(v).atan2(u.dot(v))
}

/// Drags point `point` of `built` by `offset` in one step, and checks that lines 0 and 1 are
/// still the way round they started: the signed angle between them changed by under 90°.
fn keeps_its_way_round(mut built: Built, point: usize, offset: DVec2) {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let id = built.points[point];
    let run = drag(&mut built, &[id], offset, 1).unwrap();
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
}

/// A fixed line and a free line parallel to it, its length kept, dragged by its end 30 to the
/// left (past its start, 10 away).
#[test]
#[ignore = "known failure: #344"]
fn parallel_344_pair_keeps_its_way_round_on_a_big_drag() {
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
    let built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    keeps_its_way_round(built, 3, v(-30.0, 5.0));
}

/// As the Parallel case, square: a free line perpendicular to a fixed one, its length kept,
/// dragged by its end 30 down (past its start, 10 below it).
#[test]
#[ignore = "known failure: #344"]
fn perpendicular_344_pair_keeps_its_way_round_on_a_big_drag() {
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
    let built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    keeps_its_way_round(built, 3, v(5.0, -30.0));
}

/// As the Parallel case, at 45°: a free line at a 45° Angle to a fixed one, its length kept,
/// dragged by its end 20 left and 20 down (past its start, 10 away).
#[test]
#[ignore = "known failure: #344"]
fn angle_344_pair_keeps_its_way_round_on_a_big_drag() {
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
    keeps_its_way_round(built, 3, v(-20.0, -20.0));
}

// #362: `small_drag_is_continuous_heavy` (release, `--exact`) failed on seeds 23, 32, 34, 36,
// 44, 69 and 86 of seeds 21-100. Replayed on main 4d64b69 with `PROPTEST_RNG_SEED`: 32, 34,
// 44 and 69 still fail, and these are their shrunk cases, one test each. 23, 36 and 86 pass
// there now, so they have no pin.

/// #362, seed 32: three lines from one point, the third horizontal with its far end fixed and
/// the second parallel to it. The second's far end is dragged 0.49 up, and the shared point
/// moves 358.
#[test]
#[ignore = "known failure: #362"]
fn continuity_362_seed_32() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 0,
                len: 0,
            },
            Op::Step {
                from: 0,
                angle: 12,
                len: 1,
            },
            Op::Step {
                from: 0,
                angle: 0,
                len: 1,
            },
        ],
        extras: extras(&[2, 5]),
        fix: Some(11),
        length: None,
    };
    let offset = DVec2::new(0.0, 0.49296783970832514);
    explained(continuous(&plan, &Pick::Point(42), offset)).unwrap();
}

/// #362, seed 34: two arcs and a line, the line's start on the first arc and that arc's start
/// on the line and fixed. The line and the second arc are moved 0.39 down, and a point moves
/// 274.
#[test]
#[ignore = "known failure: #362"]
fn continuity_362_seed_34() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 3,
                radius: 1,
                sweep: 4,
                ccw: true,
            },
            Op::OnArc {
                arc: 0,
                at: 15,
                angle: 18,
                len: 0,
            },
            Op::Arc {
                from: 32,
                angle: 1,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
        ],
        extras: extras(&[3]),
        fix: Some(7),
        length: None,
    };
    let pick = Pick::Curves(2458043547849449814);
    explained(continuous(
        &plan,
        &pick,
        DVec2::new(0.0, -0.3856136008032266),
    ))
    .unwrap();
}

/// #362, seed 44: a vertical line and a second line along it from the same point, its far end
/// fixed and the vertical's far end on it. The vertical's far end is dragged 0.35 across, and
/// the shared point moves 88.
#[test]
#[ignore = "known failure: #362"]
fn continuity_362_seed_44() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 18,
                len: 1,
            },
            Op::Step {
                from: 6,
                angle: 18,
                len: 0,
            },
        ],
        extras: extras(&[0, 3]),
        fix: Some(44),
        length: None,
    };
    let offset = DVec2::new(0.3538489864837486, 0.0);
    explained(continuous(&plan, &Pick::Point(10), offset)).unwrap();
}

/// #362, seed 69: an arc with a line from its start to its centre, a vertical from the centre
/// whose end is on that line, and a line from the arc's end tangent to it; the arc's start is
/// fixed. The arc is moved 0.075 left, and a point moves 15.
#[test]
#[ignore = "known failure: #362"]
fn continuity_362_seed_69() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 12,
                radius: 0,
                sweep: 1,
                ccw: false,
            },
            Op::Close { from: 11, to: 55 },
            Op::Step {
                from: 28,
                angle: 6,
                len: 1,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 0,
            },
        ],
        extras: extras(&[1, 7]),
        fix: Some(27),
        length: None,
    };
    let pick = Pick::Curves(4641844928173606864);
    explained(continuous(
        &plan,
        &pick,
        DVec2::new(-0.07504944408625547, 0.0),
    ))
    .unwrap();
}
