//! Shrunk cases the heavy runs found (#198), pinned one or two per cause: each builds the
//! sketch from the shrunk plan and runs the property it failed. Fixed causes run with the
//! suite; the rest are ignored, naming their cause, until fixed (then un-ignore them).
//! Causes and every failing seed: the green-baseline notes for #198.

use super::*;

/// A (fixed): a tangency touching at an unshared end as the drag began kept that form only
/// while the step's geometry touched there too; the step's solve took the general form
/// and left the arc ending off its tangency (found unmet after an accepted drag).
/// (`drag_is_path_independent_heavy`, seed 14.)
#[test]
fn a_tangency_touching_at_an_end_keeps_its_form_through_a_drag() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 12,
                len: 1,
            },
            Op::Step {
                from: 32,
                angle: 12,
                len: 1,
            },
            Op::Arc {
                from: 52,
                angle: 6,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Step {
                from: 0,
                angle: 0,
                len: 2,
            },
        ],
        extras: vec![
            true, false, false, false, false, false, false, false, true, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(13072636494364047962);
    let offset = DVec2::new(0.0, 0.0);
    let via = DVec2::new(0.0, 0.9541043195245882);
    let steps = 1;
    let r = path_independent(&plan, &pick, offset, via, steps);
    explained(r).unwrap();
}

/// F: a rigid move of the part meets every relation, yet every step size turns an arc.
/// (`free_move_is_accepted_heavy`, seed 10.)
#[test]
#[ignore = "#198 F: a free move is refused (an arc turns at every step size)"]
fn f_free_move_refused_as_an_arc_turns() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 13,
                len: 0,
            },
            Op::Arc {
                from: 30,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Mid {
                line: 0,
                angle: 0,
                len: 0,
            },
            Op::Arc {
                from: 0,
                angle: 9,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
        ],
        extras: vec![
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(12627255342864666180);
    let offset = DVec2::new(-17.99131491084439, 0.0);
    let steps = 1;
    let r = free_moves_go(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// F: a rigid move of the part meets every relation, yet the full step doesn't converge
/// (and a conflict isn't tried smaller).
/// (`free_move_is_accepted_heavy`, seed 3.)
#[test]
#[ignore = "#198 F: a free move is refused (the solve doesn't converge at the full step)"]
fn f_free_move_refused_unconverged() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::OnArc {
                arc: 0,
                at: 9,
                angle: 13,
                len: 0,
            },
            Op::Tangent {
                arc: 0,
                end: false,
                forward: false,
                len: 0,
            },
            Op::Step {
                from: 0,
                angle: 0,
                len: 1,
            },
            Op::OnArc {
                arc: 0,
                at: 21,
                angle: 1,
                len: 1,
            },
        ],
        extras: vec![
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(11930778343405870226);
    let offset = DVec2::new(0.0, -18.003814381075802);
    let steps = 1;
    let r = free_moves_go(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// I: lines only (a vertical, a line perpendicular to it, a point on it): one converged,
/// unflagged step moves a free end 35.7 for a drag of 0.3.
/// (`small_drag_is_continuous_heavy`, seed 7.)
#[test]
#[ignore = "#198 I: a hanging line's free end flies on a small drag"]
fn i_hanging_end_flies_on_a_small_drag() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 12,
                len: 0,
            },
            Op::Step {
                from: 0,
                angle: 6,
                len: 0,
            },
            Op::OnLine {
                line: 7,
                at: 0,
                angle: 1,
                len: 2,
            },
        ],
        extras: vec![
            false, true, true, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(1425658458094055557);
    let offset = DVec2::new(0.0, 0.2975643529800304);
    let r = continuous(&plan, &pick, offset);
    explained(r).unwrap();
}

/// I: lines only (H/V with a perpendicular): the carried line shrinks 10 → 3.4 at one size
/// and is stretched at a quarter of it, shifted.
/// (`drag_is_invariant_under_similarity_heavy`, seed 5.)
#[test]
#[ignore = "#198 I: a hanging line kept at one size, stretched at another"]
fn i_hanging_end_depends_on_size() {
    let plan = Plan {
        ops: vec![
            Op::Dup {
                from: 0,
                angle: 0,
                len: 1,
            },
            Op::Step {
                from: 0,
                angle: 18,
                len: 0,
            },
        ],
        extras: vec![
            true, true, true, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(7381477773832675244);
    let offset = DVec2::new(0.0, -6.606701489569068);
    let steps = 1;
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, -133.47216040592113),
    };
    let r = invariant(&plan, &pick, offset, steps, t);
    explained(r).unwrap();
}

/// C: two tangencies on one arc, a fixed point: a drag of 0.06 moves a point 11.9.
/// (`small_drag_is_continuous_heavy`, seed 20.)
#[test]
#[ignore = "#198 C: a small drag through tangent arcs jumps"]
fn c_small_drag_jumps_through_tangencies() {
    let plan = Plan {
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
        extras: vec![
            false, false, false, false, false, false, true, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(0),
        length: None,
    };
    let pick = Pick::Curves(15068428138436115285);
    let offset = DVec2::new(-0.03276375320139522, 0.04953594298231851);
    let r = continuous(&plan, &pick, offset);
    explained(r).unwrap();
}

/// E: an arc with one fixed point: steps are accepted unconverged with no conflict named,
/// and the result differs with the sketch's size.
/// (`drag_is_invariant_under_similarity_heavy`, seed 13.)
#[test]
#[ignore = "#198 E: the result depends on size (steps accepted unconverged)"]
fn e_result_depends_on_size_unconverged() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: true,
            },
            Op::Arc {
                from: 5,
                angle: 12,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
        ],
        extras: vec![
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(46),
        length: None,
    };
    let pick = Pick::Point(33);
    let offset = DVec2::new(0.0, -8.147331698170428);
    let steps = 1;
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 435.56956075701453),
    };
    let r = invariant(&plan, &pick, offset, steps, t);
    explained(r).unwrap();
}

/// E: an arc with a tangent line held vertical: the scaled drag lands elsewhere.
/// (`drag_is_invariant_under_similarity_heavy`, seed 2.)
#[test]
#[ignore = "#198 E: the result depends on size (arc, tangency)"]
fn e_result_depends_on_size_tangent() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 20,
                radius: 1,
                sweep: 2,
                ccw: false,
            },
            Op::OnArc {
                arc: 0,
                at: 11,
                angle: 9,
                len: 0,
            },
            Op::Tangent {
                arc: 0,
                end: false,
                forward: false,
                len: 1,
            },
        ],
        extras: vec![
            true, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(1095089637664586799);
    let offset = DVec2::new(0.0, 4.020708885845459);
    let steps = 1;
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, -166.30376374136168),
    };
    let r = invariant(&plan, &pick, offset, steps, t);
    explained(r).unwrap();
}

/// G: arcs with a tangency and a point on one: the dragged point lands 6e-5 off.
/// (`move_lands_exactly_heavy`, seed 14.)
#[test]
#[ignore = "#198 G: a move lands off its target"]
fn g_move_lands_off() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 2,
                len: 0,
            },
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
            Op::Arc {
                from: 0,
                angle: 18,
                radius: 0,
                sweep: 3,
                ccw: true,
            },
            Op::Tangent {
                arc: 1,
                end: true,
                forward: false,
                len: 0,
            },
            Op::Arc {
                from: 39,
                angle: 18,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Step {
                from: 0,
                angle: 1,
                len: 1,
            },
        ],
        extras: vec![
            false, false, false, false, true, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(6824835927813463344);
    let offset = DVec2::new(0.0, 12.725262082906049);
    let steps = 1;
    let r = moves_land(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// H: equal arcs and points on curves: a hanging line goes from 10 to 6 long.
/// (`move_keeps_untouched_lengths_heavy`, seed 1.)
#[test]
#[ignore = "#198 H: an untouched hanging line is stretched"]
fn h_untouched_line_stretched() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::OnArc {
                arc: 0,
                at: 0,
                angle: 12,
                len: 0,
            },
            Op::Mid {
                line: 0,
                angle: 0,
                len: 0,
            },
            Op::Arc {
                from: 7,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 2,
                sweep: 0,
                ccw: false,
            },
            Op::Step {
                from: 26,
                angle: 1,
                len: 0,
            },
        ],
        extras: vec![
            false, false, false, true, false, false, false, false, true, false, false, false,
            false, false, false, false, false, false, false, true, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(13773849351002921577);
    let offset = DVec2::new(16.377975070732383, 0.0);
    let steps = 1;
    let r = no_unforced_stretch(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// G: a move of an arc, a line and a line's midpoint coincident with the arc's end, the arc
/// tangent to that line: every step converges, yet the dragged points end 7 off target. Main
/// passes it only through steps accepted unconverged (108 of 132 solves at the iteration cap,
/// no conflict named); the form the tangency touched in as the drag began exposes it.
/// (`move_lands_exactly_heavy`, seed 7, unshrunk.)
#[test]
#[ignore = "#198 G: exposed by the touching-end form; main passes only through unconverged steps"]
fn g_move_lands_off_with_a_touching_tangency() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 2,
                angle: 0,
                len: 2,
            },
            Op::Mid {
                line: 33,
                angle: 8,
                len: 0,
            },
            Op::Step {
                from: 41,
                angle: 21,
                len: 0,
            },
            Op::Dup {
                from: 12,
                angle: 21,
                len: 0,
            },
            Op::Close { from: 60, to: 9 },
            Op::Arc {
                from: 54,
                angle: 6,
                radius: 2,
                sweep: 4,
                ccw: false,
            },
        ],
        extras: vec![
            false, false, false, true, true, true, false, false, false, true, false, false, false,
            false, false, false, false, true, false, false, false, false, false, false, false,
            true, true, false, true, false, false, false, false, false, false, false, false, true,
            true, false, false, true, false, true, true, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(16925403780017561720);
    let offset = DVec2::new(-2.7019888370729976, 17.00429950861392);
    let steps = 16;
    let r = moves_land(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// G: the same, shrunk to one step (it fails on main too).
/// (`move_lands_exactly_heavy`, seed 7, shrunk.)
#[test]
#[ignore = "#198 G: exposed by the touching-end form; main passes only through unconverged steps"]
fn g_move_lands_off_with_a_touching_tangency_in_one_step() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 0,
                len: 1,
            },
            Op::Mid {
                line: 0,
                angle: 1,
                len: 0,
            },
            Op::Step {
                from: 41,
                angle: 21,
                len: 0,
            },
            Op::Dup {
                from: 12,
                angle: 21,
                len: 0,
            },
            Op::Close { from: 53, to: 9 },
            Op::Arc {
                from: 51,
                angle: 6,
                radius: 1,
                sweep: 4,
                ccw: false,
            },
        ],
        extras: (0..48).map(|i| (3..=5).contains(&i)).collect(),
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(16791124099217428280);
    let offset = DVec2::new(-2.7019888370729976, 17.00429950861392);
    explained(moves_land(&plan, &pick, offset, 1)).unwrap();
}
