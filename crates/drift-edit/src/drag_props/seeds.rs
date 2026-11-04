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

/// F (fixed, #281 J2): a rigid move of the part meets every relation, yet every step size
/// turns an arc; the arc carried along, the carried step didn't finish whole and wasn't tried
/// smaller. (`free_move_is_accepted_heavy`, seed 10.)
#[test]
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

/// F (fixed, #282): a rigid move of the part meets every relation, yet no step finished at
/// any size: the two dragged points held on the arc are a diameter apart, so the arc's kept
/// radius sits on a fold and the solve stalls. The arc they are held on is carried along.
/// (`free_move_is_accepted_heavy`, seed 3.)
#[test]
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

/// I (fixed, #277): lines only (a vertical, a line perpendicular to it, a point on it): one
/// converged, unflagged step moved a free end 35.7 for a drag of 0.3. Once the dragged were
/// pinned to land, the right angle had only round-off gradient on the vertical's free end,
/// which slides along it, and scaled up its round-off residual flung the end as far as the
/// drawing is across. (`small_drag_is_continuous_heavy`, seed 7.)
#[test]
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

/// I (fixed, #277): lines only (H/V with a perpendicular): with the lines substituted level
/// and plumb, the right angle had only round-off gradient, and its round-off residual swung
/// the upright's free end the drawing's width back and forth until the solve's patience ran
/// out: where it landed depended on the size and place. (The upright's top end is merged
/// with a dragged point, so it is reshaped, not carried: 10 → 3.4 at both sizes.)
/// (`drag_is_invariant_under_similarity_heavy`, seed 5.)
#[test]
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
#[ignore = "#278: a small drag through tangent arcs jumps"]
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

/// E (fixed, #281 J1): an arc with one fixed point: steps were accepted unconverged with no
/// conflict named (an arc's end left off its circle), and the result differed with the
/// sketch's size.
/// (`drag_is_invariant_under_similarity_heavy`, seed 13.)
#[test]
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

/// E (fixed, #277's cause): an arc with a tangent line held vertical: the scaled drag landed
/// elsewhere. The tangency had only round-off gradient on what was left to move.
/// (`drag_is_invariant_under_similarity_heavy`, seed 2.)
#[test]
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

/// G (fixed, #283): arcs with a tangency and a point on one: the dragged point is held on a
/// line at the line's tangent end, a fold, and the move landed 6e-5 off. The line it is held
/// on is carried along. (`move_lands_exactly_heavy`, seed 14.)
#[test]
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
#[ignore = "#284: an untouched hanging line is stretched"]
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

/// G (fixed, #283): a move of an arc, a line and a line's midpoint coincident with the arc's
/// end, the arc tangent to that line: every step converged, yet on the far branch (a line
/// turned past its parallel's square), the dragged points 7 off target. The line holding the
/// midpoint is carried along. (`move_lands_exactly_heavy`, seed 7, unshrunk.)
#[test]
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

/// G (fixed, #283): the same, shrunk to one step.
/// (`move_lands_exactly_heavy`, seed 7, shrunk.)
#[test]
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

/// O (fixed, #282's fold steps): a dragged end held where a line is at its nearest to a
/// dragged point (the other end of a line kept equal to one it crosses): a double root, where
/// the landing pass stalled 0.002 short of the target and the line it is held on can't be
/// carried to it either. Tried with the fold steps, it lands. (`move_lands_exactly_heavy`,
/// seed 17, shrunk on main; the seed passes since #282's carry.)
#[test]
fn o_move_lands_off_at_a_fold() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 20,
                len: 3,
            },
            Op::Step {
                from: 30,
                angle: 8,
                len: 1,
            },
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Mid {
                line: 13,
                angle: 9,
                len: 2,
            },
            Op::OnLine {
                line: 0,
                at: 0,
                angle: 2,
                len: 1,
            },
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Step {
                from: 52,
                angle: 18,
                len: 2,
            },
        ],
        extras: vec![
            false, false, false, false, true, false, false, false, false, false, false, false,
            false, false, true, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(11724342438443041768);
    let offset = DVec2::new(0.0, -14.516466897443614);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

/// O (fixed, #282's fold steps): two arcs of equal radius sharing ends with a third, none of
/// whose points a relation holds on a curve: no curve to carry, and a kept radius the minimum
/// change all but rules out (it closed a thousandth of itself a step) kept the rows from
/// holding until the solve's cap. Tried with the fold steps, the move goes.
/// (`free_move_is_accepted_heavy`, seed 18, shrunk on main.)
#[test]
fn o_free_move_refused_at_a_fold() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 0,
                len: 0,
            },
            Op::Arc {
                from: 0,
                angle: 1,
                radius: 0,
                sweep: 3,
                ccw: false,
            },
            Op::Arc {
                from: 27,
                angle: 0,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
            Op::Arc {
                from: 6,
                angle: 0,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Arc {
                from: 35,
                angle: 0,
                radius: 1,
                sweep: 1,
                ccw: false,
            },
        ],
        extras: vec![
            true, true, false, false, false, true, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(262210794025958468);
    let offset = DVec2::new(7.434462470577808, 9.084806060733136);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

/// E: two dragged points held on an arc a diameter apart (f3's shape): at one size the stalled
/// solve reads as finished within the coordinates' tolerance and is kept reshaped, at the
/// other it is carried whole; 0.02 apart. On main the same shape was refused at one size only.
/// (`drag_is_invariant_under_similarity_heavy`, seed 20, shrunk with #282's carry.)
#[test]
#[ignore = "#280: the result depends on size (a fold read as finished at one size)"]
fn e_result_depends_on_size_at_a_fold() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 10,
                radius: 0,
                sweep: 2,
                ccw: false,
            },
            Op::OnArc {
                arc: 0,
                at: 3,
                angle: 1,
                len: 3,
            },
            Op::Step {
                from: 53,
                angle: 4,
                len: 3,
            },
            Op::OnArc {
                arc: 0,
                at: 15,
                angle: 23,
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
    let pick = Pick::Curves(10360524982465828250);
    let offset = DVec2::new(17.645954621075454, 15.209813272136655);
    let steps = 1;
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 231.67316288164798),
    };
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

/// O (fixed, #282's fold steps): an arc with a line tangent at its start and a line and a
/// point held on it: the move's solve converged by halving to a double root of its rows, then
/// stalled there against the minimum change, a hair from holding, until its cap. Tried with
/// the fold steps, the move goes. (`free_move_is_accepted_heavy`, seed 20, shrunk on main.)
#[test]
fn o_free_move_refused_at_a_double_root() {
    let plan = Plan {
        ops: vec![
            Op::Dup {
                from: 0,
                angle: 10,
                len: 0,
            },
            Op::Arc {
                from: 21,
                angle: 9,
                radius: 0,
                sweep: 0,
                ccw: false,
            },
            Op::Dup {
                from: 5,
                angle: 19,
                len: 1,
            },
            Op::Tangent {
                arc: 0,
                end: false,
                forward: true,
                len: 2,
            },
            Op::OnLine {
                line: 29,
                at: 0,
                angle: 22,
                len: 1,
            },
            Op::OnArc {
                arc: 0,
                at: 18,
                angle: 0,
                len: 0,
            },
        ],
        extras: (0..48).map(|i| i == 16 || i == 19).collect(),
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(2255998535123590272);
    let offset = DVec2::new(0.0, 3.5165293026948543);
    explained(free_moves_go(&plan, &pick, offset, 1)).unwrap();
}

/// D (fixed, #277's cause; #279): tangencies, a level line square to another: out and back in
/// one drag, a line's free end came back 66 away. Once the dragged were pinned to land, the
/// right angle had only round-off gradient on what was left to move, and its round-off
/// residual flung the end. (`drag_out_and_back_does_not_creep_heavy`, seed 5.)
#[test]
fn d_out_and_back_through_a_redundant_angle_creeps() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 6,
                radius: 0,
                sweep: 1,
                ccw: false,
            },
            Op::Close { from: 31, to: 5 },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 3,
            },
            Op::Step {
                from: 0,
                angle: 0,
                len: 3,
            },
            Op::Tangent {
                arc: 0,
                end: false,
                forward: false,
                len: 0,
            },
        ],
        extras: vec![
            false, true, false, false, false, false, false, false, false, true, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(15512489140483433875);
    let offset = DVec2::new(0.0, 0.0);
    let steps = 1;
    let r = no_creep(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// C (fixed, #277's cause; one of #278's): an arc, a line on it held vertical and square to
/// another: a drag of next to nothing moved a free end 88. The right angle had only
/// round-off gradient on what was left to move. (`small_drag_is_continuous_heavy`, seed 19.)
#[test]
fn c_no_drag_moves_a_free_end() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 2,
                radius: 2,
                sweep: 1,
                ccw: false,
            },
            Op::OnArc {
                arc: 0,
                at: 16,
                angle: 0,
                len: 0,
            },
            Op::Close { from: 0, to: 1 },
            Op::Step {
                from: 47,
                angle: 6,
                len: 3,
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
    let pick = Pick::Curves(4262113848630934323);
    let offset = DVec2::new(0.0, 0.0);
    let r = continuous(&plan, &pick, offset);
    explained(r).unwrap();
}

/// E: an arc tangent to a line, a fixed point: the scaled drag lands elsewhere. What
/// `drag_is_invariant_under_similarity_heavy` seed 5 shrinks to once #277's case (pinned
/// above, `i_hanging_end_depends_on_size`) is fixed.
#[test]
#[ignore = "#280: the result depends on size (similarity 5 after #277: arc, tangency, fix)"]
fn e_result_depends_on_size_after_277() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 4,
                len: 0,
            },
            Op::Arc {
                from: 6,
                angle: 6,
                radius: 0,
                sweep: 0,
                ccw: true,
            },
            Op::Step {
                from: 1,
                angle: 15,
                len: 2,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 1,
            },
            Op::Step {
                from: 4,
                angle: 12,
                len: 0,
            },
        ],
        extras: vec![
            false, false, true, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(35),
        length: None,
    };
    let pick = Pick::Curves(2970465637109295241);
    let offset = DVec2::new(0.0, -15.38195792032203);
    let steps = 1;
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 107.82855999007688),
    };
    let r = invariant(&plan, &pick, offset, steps, t);
    explained(r).unwrap();
}

/// I (fixed, #277), far from the origin: a level line, a plumb one with an end held on the
/// level one, a line and an arc, the plumb line and one other moved a fifth of a millimetre,
/// the whole sketch 100 m out. There (not at the origin) the round-off left of an angle's
/// gradient once the dragged were pinned flung a free end 84 away. The rows left with no
/// gradient are found against their own gradient, which round-off far out doesn't reach.
/// (A census of small drags 100 m out.)
#[test]
fn i_hanging_end_flies_far_from_the_origin() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 49,
                angle: 12,
                len: 3,
            },
            Op::Tangent {
                arc: 29,
                end: false,
                forward: false,
                len: 2,
            },
            Op::Tangent {
                arc: 9,
                end: false,
                forward: true,
                len: 1,
            },
            Op::OnLine {
                line: 18,
                at: 0,
                angle: 6,
                len: 1,
            },
            Op::Step {
                from: 45,
                angle: 15,
                len: 3,
            },
            Op::Arc {
                from: 42,
                angle: 4,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
        ],
        extras: vec![
            true, true, false, false, true, true, true, false, true, false, true, false, true,
            false, false, false, false, true, false, true, false, false, true, false, true, false,
            false, false, false, false, false, false, false, false, true, true, false, false,
            false, false, false, false, true, true, true, true, false, false,
        ],
        fix: None,
        length: None,
    };
    let pick = Pick::Curves(6866880060382879132);
    let offset = DVec2::new(0.09964781793467078, 0.1845802573484448);
    let sketch = plan.sketch().unwrap();
    let t = Similarity {
        scale: 1.0,
        turn: 0.0,
        shift: DVec2::new(1e5, 1e5),
    };
    let mut built = Built::new(&sketch, t).unwrap();
    let ids = pick.ids(&built);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(run.committed && run.refused == 0);
    for i in 0..run.start.len() {
        let moved = run.start[i].distance(run.end[i]);
        assert!(
            moved <= 2.0 * offset.length(),
            "point {i} moved {moved} for a drag of {}",
            offset.length()
        );
    }
}

/// #277: a point held on a line whose ends the drag moves, another held on a line through a
/// fixed point, a midpoint and an arc, dragged in five steps: every step goes. Rows like
/// these, held on a line with no gradient left over what is still free but not holding,
/// keep their scale: left out of the step as a holding one is, every step was refused.
/// (A census of drags.)
#[test]
fn i_unheld_row_with_no_gradient_left_still_goes() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 39,
                angle: 12,
                len: 3,
            },
            Op::Mid {
                line: 25,
                angle: 12,
                len: 0,
            },
            Op::Tangent {
                arc: 57,
                end: false,
                forward: false,
                len: 0,
            },
            Op::Step {
                from: 0,
                angle: 23,
                len: 1,
            },
            Op::OnLine {
                line: 25,
                at: 0,
                angle: 19,
                len: 3,
            },
            Op::Arc {
                from: 45,
                angle: 9,
                radius: 1,
                sweep: 4,
                ccw: true,
            },
        ],
        extras: vec![
            false, true, false, false, false, false, false, true, false, false, true, false, false,
            false, false, false, true, false, false, false, true, false, true, false, false, true,
            false, true, true, true, false, true, false, false, false, false, false, false, true,
            false, true, false, false, false, false, false, true, false,
        ],
        fix: Some(32),
        length: None,
    };
    let pick = Pick::Curves(2049299765421540780);
    let offset = DVec2::new(-4.5718023607860125, 13.942411822201512);
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let run = drag(&mut built, &ids, offset, 5).unwrap();
    assert!(
        run.committed && run.refused == 0,
        "refused {} of 5 steps",
        run.refused
    );
}
