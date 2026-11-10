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

/// C (fixed, #278): two tangencies on one arc, a fixed point: a drag of 0.06 moved a point 11.9.
/// The drag can't land (the arc tangent at both ends is held one way too many), and the solve
/// took up the misfit by sliding a free point along its line towards the pivot, crushing the
/// line from 15 to 3.1. The drag now stops, keeping the line. (`small_drag_is_continuous_heavy`,
/// seed 20.)
///
/// (Seed 20's run passes since #280's A1: the case it first failed on was an arc closed on
/// itself by round-off. This pin is its own minimal case, a wandering first solve: drag-277.)
#[test]
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

/// H (fixed, #284): equal arcs and points on curves: a hanging line went from 10 to 6 long.
/// Line 2's kept length holds line 1's too (`Equal(1, 2)`), and the step's first solve put
/// line 1's free end where its kept length has no first-order way back: a kept length at a
/// fold of its own. The preference's step closed next to nothing each iteration (1e-7 of the 4
/// left), and `SETTLE_PATIENCE` ended the solve at iteration 13 with the line at 6, the same at
/// any size. The frame went and landed, so it is tried once more with the fold steps from its
/// start (`Editor::keep_lengths`), which keeps the length: line 1's end swings round the
/// arc's circle instead. (`move_keeps_untouched_lengths_heavy`, seed 1, shrunk on e002052.)
#[test]
fn h_untouched_line_stretched() {
    let plan = h_plan();
    let pick = Pick::Curves(13773849351002921577);
    let offset = DVec2::new(16.377975070732383, 0.0);
    let steps = 1;
    let r = no_unforced_stretch(&plan, &pick, offset, steps);
    explained(r).unwrap();
}

/// `h_untouched_line_stretched`'s sketch.
fn h_plan() -> Plan {
    Plan {
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
    }
}

/// #284's case beside unrelated parts elsewhere in the drawing: a 2 m line 100 m away whose
/// dimension is met only to the solver's own test out there (off by 1e-8: within it, but far
/// coarser than the dragged part is held to), and a line pinned at both ends under a dimension
/// it can't meet (a conflict). The try at keeping a length reads only the dragged part's
/// relations, so the hanging line still keeps its length.
#[test]
fn h_untouched_line_stretched_beside_an_unrelated_part() {
    let sketch = h_plan().sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    built.ed.doc.begin();
    let a = built.ed.doc.add_point(DVec2::new(1e5, 1e5));
    let b = built.ed.doc.add_point(DVec2::new(1e5 + 2000.0 + 1e-8, 1e5));
    built.ed.doc.add_line(a, b).unwrap();
    built
        .ed
        .doc
        .add_constraint(Constraint::Distance {
            a,
            b,
            value: 2000.0,
        })
        .unwrap();
    let c = built.ed.doc.add_point(DVec2::new(-500.0, 300.0));
    let d = built.ed.doc.add_point(DVec2::new(-400.0, 300.0));
    built.ed.doc.add_line(c, d).unwrap();
    for r in [
        Constraint::Fix {
            point: c,
            at: DVec2::new(-500.0, 300.0),
        },
        Constraint::Fix {
            point: d,
            at: DVec2::new(-400.0, 300.0),
        },
        Constraint::Distance {
            a: c,
            b: d,
            value: 90.0,
        },
    ] {
        built.ed.doc.add_constraint(r).unwrap();
    }
    built.ed.doc.commit("Elsewhere");
    let ids = Pick::Curves(13773849351002921577).ids(&built);
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    let length = |built: &Built| {
        let p = built.positions();
        p[5].distance(p[6])
    };
    let before = length(&built);
    built.ed.doc.begin();
    assert!(
        built
            .ed
            .drag(&ids, DVec2::new(16.377975070732383, 0.0), &dragged)
    );
    let after = length(&built);
    assert!(
        (after - before).abs() <= LANDS * before,
        "line 2 went from {before} to {after} long"
    );
}

/// H (fixed, #284): the same sketch with both lines level (`move_keeps_untouched_lengths_heavy`
/// seed 1's own shrunk case on a561c30): the move turns a curve even in its smallest step,
/// so the turned curves are carried along, and in that carried solve line 2's kept length
/// crawled (1e-7 a step) until `SETTLE_PATIENCE` ended it at iteration 12, the line at 6. The
/// try at keeping the length (`Editor::keep_lengths`) keeps it.
#[test]
fn h_untouched_line_stretched_level() {
    let plan = h_level_plan();
    let pick = Pick::Curves(13767121785659795704);
    let offset = DVec2::new(16.377975070732383, 2.966956818320761);
    explained(no_unforced_stretch(&plan, &pick, offset, 1)).unwrap();
}

/// `h_untouched_line_stretched_level`'s sketch.
fn h_level_plan() -> Plan {
    Plan {
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
                len: 1,
            },
        ],
        extras: (0..48).map(|i| [0, 1, 3].contains(&i)).collect(),
        fix: None,
        length: None,
    }
}

/// #284's two cases at other sizes and far from the origin: the hanging line keeps its length
/// at 0.1×, 1× and 10×, and a metre, 30 m, 100 m and a kilometre out. (The try at keeping a
/// length is held to the part's size, with a floor at what round-off can still tell, so it
/// works the same far out.)
#[test]
fn h_untouched_line_kept_at_any_size_and_place() {
    let cases = [
        (
            h_plan(),
            Pick::Curves(13773849351002921577),
            DVec2::new(16.377975070732383, 0.0),
        ),
        (
            h_level_plan(),
            Pick::Curves(13767121785659795704),
            DVec2::new(16.377975070732383, 2.966956818320761),
        ),
    ];
    for (n, (plan, pick, offset)) in cases.iter().enumerate() {
        let sketch = plan.sketch().unwrap();
        for (scale, at) in [
            (0.1, 0.0),
            (1.0, 0.0),
            (10.0, 0.0),
            (1.0, 1e3),
            (1.0, 3e4),
            (1.0, 1e5),
            (1.0, 1e6),
        ] {
            let t = Similarity {
                scale,
                turn: 0.0,
                shift: DVec2::new(at, at),
            };
            let mut built = Built::new(&sketch, t).unwrap();
            let ids = pick.ids(&built);
            let run = drag(&mut built, &ids, t.vector(*offset), 1).unwrap();
            let (before, after) = (
                run.start[5].distance(run.start[6]),
                run.end[5].distance(run.end[6]),
            );
            assert!(
                (after - before).abs() <= LANDS * before,
                "case {n} at {scale}×, ({at}, {at}): line 2 went from {before} to {after} long"
            );
        }
    }
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

/// e_result_depends_on_size_at_a_fold's case: its sketch, pick, move and steps.
fn at_a_fold_case() -> (Plan, Pick, DVec2, u32) {
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
    (plan, pick, offset, steps)
}

/// E (#280 A2): two dragged points held on an arc a diameter apart (f3's shape), so the arc's
/// centre has no first-order way to move: a kept radius at a fold. The solve's rows and the
/// kept radius both close like 1/k. At the move's last step, the 1× solve reaches the
/// iteration cap with its rows at 1.02 times the tolerance (unfinished, so the arc is carried
/// whole); at ¼ size far out the rows hold within the coordinates' looser tolerance and
/// `SETTLE_PATIENCE` ends the solve at iteration 13 (kept reshaped); 0.02 apart. The
/// tolerance goes with where the drawing is (round-off), so a solve that closes slowly holds
/// at one place and not at another (also similarity 6, 16, 17 and 18). See #280.
/// (`drag_is_invariant_under_similarity_heavy`, seed 20, shrunk with #282's carry.)
///
/// Passes since a drag's solve near the origin judges every stop against the part's size
/// (the solver's `Yardstick`): both sizes now stop at the same iteration.
#[test]
fn e_result_depends_on_size_at_a_fold() {
    let (plan, pick, offset, steps) = at_a_fold_case();
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

/// e_result_depends_on_size_after_277's case: its sketch, pick, move and steps.
fn after_277_case() -> (Plan, Pick, DVec2, u32) {
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
    (plan, pick, offset, steps)
}

/// E (#280 A3): an arc tangent to a line, a fixed point: the scaled drag lands elsewhere.
/// The solve wanders (#103) and `SETTLE_PATIENCE` ends it still moving, wherever round-off
/// has it by then; no decision of the drag differs between the two sizes. What
/// `drag_is_invariant_under_similarity_heavy` seed 5 shrinks to once #277's case (pinned
/// above, `i_hanging_end_depends_on_size`) is fixed; also similarity 14 and 15's mechanism.
///
/// Passes since a near drag solve judges its stops against the part's size and, once its
/// rows hold, no longer lets the round-off gain steer the damping (both the solver's
/// `Yardstick`): the patience cut then falls at the same point at both sizes. Without the
/// damping part it still fails.
#[test]
fn e_result_depends_on_size_after_277() {
    let (plan, pick, offset, steps) = after_277_case();
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

/// E (fixed, #280 A1): an arc with a line held on it, the arc and four lines moved: the
/// step's solve closed the 270° arc on itself, its end a hair past its start or a hair short
/// by round-off. Read as a sweep that is ~0 (turned: the step is halved) or ~360° (exactly
/// the quarter turn allowed: not turned), so the same drag went two ways at two sizes. Now
/// an arc a step closes counts as turned either way. (`drag_is_invariant_under_similarity_heavy`,
/// seed 7.) Also the scale test: 10×, 0.1×, ¼ far out, a few hundred millimetres along, and a
/// billionth larger, each the same drag transformed.
#[test]
fn e_result_depends_on_size_arc_ends_meet() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 18,
                radius: 2,
                sweep: 4,
                ccw: true,
            },
            Op::Step {
                from: 39,
                angle: 21,
                len: 1,
            },
            Op::Step {
                from: 4,
                angle: 18,
                len: 1,
            },
            Op::Step {
                from: 3,
                angle: 1,
                len: 2,
            },
            Op::Step {
                from: 27,
                angle: 2,
                len: 2,
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
    let pick = Pick::Curves(5826560194303214302);
    let offset = DVec2::new(18.609188948490686, -16.244391113917867);
    let steps = 1;
    let p = DVec2::new;
    for (scale, shift) in [
        (0.25, p(0.0, -135.96620630717226)),
        (10.0, p(0.0, 0.0)),
        (0.1, p(0.0, 0.0)),
        (0.25, p(-362.0, -288.0)),
        (1.0, p(0.0, 300.0)),
        (1.0 + 1e-9, p(0.0, 0.0)),
    ] {
        let t = Similarity {
            scale,
            turn: 0.0,
            shift,
        };
        if let Err(e) = explained(invariant(&plan, &pick, offset, steps, t)) {
            panic!("at {scale}× shifted {shift}: {e}");
        }
    }
}

/// The same (#280 A1), one ulp apart: similarity 7's sketch dragged at its size and a
/// billionth larger ends the same to a hundred-millionth (the scaling's own rounding), where
/// round-off used to take the arc either side of closing and the two drags 3.4 mm apart.
#[test]
fn e_result_depends_on_size_arc_ends_meet_one_ulp() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 18,
                radius: 2,
                sweep: 4,
                ccw: true,
            },
            Op::Step {
                from: 39,
                angle: 21,
                len: 1,
            },
            Op::Step {
                from: 4,
                angle: 18,
                len: 1,
            },
            Op::Step {
                from: 3,
                angle: 1,
                len: 2,
            },
            Op::Step {
                from: 27,
                angle: 2,
                len: 2,
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
    let pick = Pick::Curves(5826560194303214302);
    let offset = DVec2::new(18.609188948490686, -16.244391113917867);
    let sketch = plan.sketch().unwrap();
    let t = Similarity {
        scale: 1.0 + 1e-9,
        turn: 0.0,
        shift: DVec2::ZERO,
    };
    let mut plain = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let mut moved = Built::new(&sketch, t).unwrap();
    let (ids_a, ids_b) = (pick.ids(&plain), pick.ids(&moved));
    let a = drag(&mut plain, &ids_a, offset, 1).unwrap();
    let b = drag(&mut moved, &ids_b, t.vector(offset), 1).unwrap();
    assert_eq!((a.committed, a.refused), (b.committed, b.refused));
    for (i, (pa, pb)) in a.end.iter().zip(&b.end).enumerate() {
        let want = t.point(*pa);
        assert!(
            want.distance(*pb) <= 1e-8 * (pa.length() + offset.length()),
            "point {i}: {want} wanted, {pb} found"
        );
    }
}

/// E (fixed, #280 A1): an arc with two tangent lines, one held level, a point fixed, moved
/// at three times the size far from the origin: the step's solve crushed the arc to a point,
/// whose sweep (the angle between two vectors of next to no length) said turned at one size
/// and not at the other. A crushed arc now counts as turned at any size.
/// (`drag_is_invariant_under_similarity_heavy`, seed 1.)
#[test]
fn e_result_depends_on_size_arc_crushed() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 1,
                sweep: 1,
                ccw: true,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: true,
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
            true, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(14),
        length: None,
    };
    let pick = Pick::Curves(8377514915416449379);
    let offset = DVec2::new(3.1244537091556617, -14.019893432491916);
    let t = Similarity {
        scale: 3.2170815616173343,
        turn: 0.0,
        shift: DVec2::new(0.0, -411.16571173760167),
    };
    explained(invariant(&plan, &pick, offset, 1, t)).unwrap();
}

/// E (fixed, #280 A1): two arcs, a line held on one, a point fixed: the moved arc closed on
/// itself at ¼ size near the origin and not at full size; neither the scale nor the shift
/// alone does it, only both. (`drag_is_invariant_under_similarity_heavy`, seed 12.)
#[test]
fn e_result_depends_on_size_arc_ends_meet_scale_and_shift() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 0,
                radius: 1,
                sweep: 4,
                ccw: true,
            },
            Op::Arc {
                from: 1,
                angle: 0,
                radius: 0,
                sweep: 3,
                ccw: false,
            },
            Op::Step {
                from: 0,
                angle: 15,
                len: 2,
            },
            Op::Step {
                from: 0,
                angle: 0,
                len: 0,
            },
        ],
        extras: vec![
            false, false, true, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(19),
        length: None,
    };
    let pick = Pick::Curves(11632138330893999404);
    let offset = DVec2::new(-15.32900327832576, 10.88552382031287);
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 4.186292711986068),
    };
    explained(invariant(&plan, &pick, offset, 1, t)).unwrap();
}

/// E (#280 A3): a lone arc, its end dragged at ¼ size near the origin: the arc's kept shape is
/// off by some eight times the sketch, the step cap lets it close one sketch's width an
/// iteration, and `SETTLE_PATIENCE` leaves it 0.6 % short. At one size the dragged end then
/// sits on its target to the bit and `land` skips its pass; at the other it is an ulp off and
/// the landing pass closes the rest: 6e-5 apart. No relations at all.
/// (`drag_is_invariant_under_similarity_heavy`, seed 19.)
///
/// Still fails, by design: the ¼-size copy is a 0.55 mm part 29 mm out, some 54 times its
/// own size from the origin, so its solves are "far" (the solver's `Yardstick`) and run
/// today's code line for line, while the 1× copy is near. The far copy's result is main's,
/// bit for bit.
#[test]
#[ignore = "#280: a band pair: one copy near the origin, the other far (today's solve)"]
fn e_result_depends_on_size_patience_and_landing() {
    let plan = Plan {
        ops: vec![Op::Arc {
            from: 0,
            angle: 23,
            radius: 2,
            sweep: 2,
            ccw: true,
        }],
        extras: vec![
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: None,
    };
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(29.379660976701263, 0.0),
    };
    let offset = DVec2::new(-18.926134486396514, 6.980106084280833);
    explained(invariant(&plan, &Pick::Point(22), offset, 1, t)).unwrap();
}

/// E (#280): an arc, three lines and a tangent one, a point fixed, moved in seven steps at ¼
/// size far out: in the same step, after the same decisions, one size's solve crushes the
/// arc (its radius 3e-9 of the sketch: turned, so halved) and the other's leaves it whole;
/// the two drags end 1e-4 apart. Not a threshold (the arc is whole or gone), but two
/// solutions the solve picks between by round-off (rows with a double root: a tangent line
/// through a point at the circle's own radius). See #280.
/// (`drag_is_invariant_under_similarity_heavy`, seed 11, after #280's A1.)
///
/// Still fails, by design: the ¼-size copy sits 351 mm out, some 50 times its own size from
/// the origin, so its solves are "far" (the solver's `Yardstick`) and keep today's code: main
/// crushes the arc there, as before, while the 1× copy (near) keeps it.
#[test]
#[ignore = "#280: a band pair: one copy near the origin, the other far (today's solve)"]
fn e_result_depends_on_size_crushed_at_one_size() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 20,
                radius: 0,
                sweep: 2,
                ccw: false,
            },
            Op::Step {
                from: 42,
                angle: 5,
                len: 2,
            },
            Op::Step {
                from: 60,
                angle: 8,
                len: 2,
            },
            Op::Step {
                from: 2,
                angle: 5,
                len: 0,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 2,
            },
        ],
        extras: vec![
            false, false, false, false, false, true, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
        ],
        fix: Some(11),
        length: None,
    };
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 351.58307887362463),
    };
    let offset = DVec2::new(14.935527528458358, -14.684265805145367);
    explained(invariant(
        &plan,
        &Pick::Curves(447053154812012336),
        offset,
        7,
        t,
    ))
    .unwrap();
}

/// `e_result_depends_on_size_tangent_and_parallel`'s sketch.
fn tangent_and_parallel_plan() -> Plan {
    Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 2,
                radius: 1,
                sweep: 0,
                ccw: false,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 0,
            },
            Op::Step {
                from: 13,
                angle: 20,
                len: 0,
            },
        ],
        extras: (0..48).map(|i| i == 0).collect(),
        fix: Some(2),
        length: None,
    }
}

/// E (#280): an arc, a line tangent to it and one parallel to that, a point fixed, a line
/// moved at ¼ size turned 232°: the step is halved for turning the arc; at depth 5 one half's
/// solve converges at ¼ size but stops unfinished at 1× (the coordinates' tolerance again),
/// and from there the two take different halvings and carries. See #280.
/// (`drag_is_invariant_under_similarity_heavy`, seed 3's own shrunk case on a561c30.)
///
/// Passes at this similarity since near drag solves judge their stops against the part's
/// size. **Not size-free in general:** at 0.3× and 3.7× it still differs. The frame's 8th
/// solve reaches the same scaled state at both sizes; once its rows hold, the rows' step
/// from a round-off residual, amplified by the fold's near-singular Jacobian (some 2,300 ε
/// of the coordinates), is above the part-size bar at one size (it runs to patience) and
/// below it at the other (it settles). Round-off at a fold, not a yardstick; similarity 3
/// still fails for it.
#[test]
fn e_result_depends_on_size_tangent_and_parallel() {
    let plan = tangent_and_parallel_plan();
    let t = Similarity {
        scale: 0.25,
        turn: 232.08152175323588,
        shift: DVec2::ZERO,
    };
    let offset = DVec2::new(15.457267808482761, -17.270854180887095);
    explained(invariant(
        &plan,
        &Pick::Curves(9929081795026096183),
        offset,
        1,
        t,
    ))
    .unwrap();
}

/// A guard (passes): a point on an arc and a length, dragged in four steps at ¼ size far out.
/// Widening the solver's strict fold try to every frame left with a kept shape off (tried for
/// #280 and #284, not kept) made the two sizes refuse different numbers of frames here.
/// (`drag_is_invariant_under_similarity_heavy`, seed 2, shrunk with that try.)
#[test]
fn e_result_depends_on_size_guard_for_a_wider_fold_try() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 10,
                radius: 0,
                sweep: 0,
                ccw: true,
            },
            Op::Step {
                from: 1,
                angle: 17,
                len: 0,
            },
            Op::OnArc {
                arc: 0,
                at: 1,
                angle: 21,
                len: 1,
            },
            Op::Dup {
                from: 2,
                angle: 10,
                len: 0,
            },
            Op::Arc {
                from: 49,
                angle: 17,
                radius: 2,
                sweep: 3,
                ccw: false,
            },
        ],
        extras: (0..48).map(|i| [0, 1, 8, 9].contains(&i)).collect(),
        fix: None,
        length: Some(11),
    };
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, -304.79968618558775),
    };
    let offset = DVec2::new(-12.812079005553306, -10.973312729997488);
    explained(invariant(&plan, &Pick::Point(14), offset, 4, t)).unwrap();
}

/// #280 A1: four tangencies, a midpoint, equal and perpendicular lines, a fixed point and a
/// length, three arcs and a line moved: halving the step, one half crushes an arc, and the next
/// half read the crushed arc's sweep (round-off) and its tangencies' sides as turned, so the
/// move was refused outright. An arc already crushed where a step begins isn't judged now, and
/// the move goes. (A census of random drags, case 470.)
#[test]
fn a_move_past_a_crushed_arc_goes() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 25,
                angle: 2,
                radius: 1,
                sweep: 1,
                ccw: true,
            },
            Op::Tangent {
                arc: 54,
                end: true,
                forward: true,
                len: 0,
            },
            Op::Arc {
                from: 19,
                angle: 22,
                radius: 0,
                sweep: 2,
                ccw: true,
            },
            Op::Tangent {
                arc: 19,
                end: false,
                forward: true,
                len: 3,
            },
            Op::Tangent {
                arc: 58,
                end: true,
                forward: false,
                len: 3,
            },
            Op::Tangent {
                arc: 53,
                end: true,
                forward: false,
                len: 3,
            },
            Op::Mid {
                line: 36,
                angle: 20,
                len: 0,
            },
        ],
        extras: vec![
            false, false, false, false, true, false, false, true, true, true, false, false, true,
            true, true, true, false, false, false, true, false, true, false, false, false, false,
            true, true, false, true, false, false, false, true, false, false, true, true, false,
            false, false, false, false, false, true, false, true, false,
        ],
        fix: Some(59),
        length: Some(25),
    };
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = Pick::Curves(16107383938640796786).ids(&built);
    let offset = DVec2::new(-14.789257937949609, 1.3311319113522229);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(
        run.committed && run.refused == 0,
        "refused {} of 1",
        run.refused
    );
}

/// J (fixed, #278): a drag that can't land took up the misfit by sliding an arc's centre out
/// along its radius: the radius went from 8 to 52.6 for a drag of 0.078. The drag now stops,
/// keeping the radius. (`small_drag_is_continuous_heavy`, seed 16.)
#[test]
fn j_a_drag_that_cant_land_keeps_a_radius_it_doesnt_move() {
    let (plan, pick, offset) = super::held_shapes::seed_16();
    explained(continuous(&plan, &pick, offset)).unwrap();
}

/// Seed 1's shrunk case (`small_drag_is_continuous_heavy`, #278): the dragged point starts on
/// the fixed end of the line it is held on, so the line has no direction until the drag gives
/// it one, and swings round to follow (point 0, on an arc about the line's other end, goes
/// 3.3 for a drag of 0.015). Forced geometry, not a jump: the drag lands exactly and the line
/// runs through the fixed end, and the property passes it on that account.
fn seed_1() -> (Plan, Pick, DVec2) {
    let mut extras = vec![false; 48];
    extras[2] = true;
    (
        Plan {
            ops: vec![
                Op::Step {
                    from: 0,
                    angle: 2,
                    len: 3,
                },
                Op::Arc {
                    from: 0,
                    angle: 17,
                    radius: 0,
                    sweep: 1,
                    ccw: true,
                },
                Op::Step {
                    from: 62,
                    angle: 7,
                    len: 2,
                },
                Op::Arc {
                    from: 2,
                    angle: 7,
                    radius: 2,
                    sweep: 1,
                    ccw: false,
                },
            ],
            extras,
            fix: Some(53),
            length: None,
        },
        Pick::Point(12),
        DVec2::new(0.0, -0.015479710606090684),
    )
}

#[test]
fn k_a_point_on_its_lines_fixed_end_swings_the_line_round() {
    let (plan, pick, offset) = seed_1();
    explained(continuous(&plan, &pick, offset)).unwrap();
    // The swing itself: landed, and the line through the fixed end and the dragged point.
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(forced_swing(&built, &ids, &run, offset).is_some());
}

/// L: seed 1's third shrunk case (`small_drag_is_continuous_heavy`, #278): the dragged point,
/// the far end of a line square to a tangent of an arc, starts on the arc's fixed centre. The
/// line must run along the radius, so through the centre, and its direction is free until the
/// drag gives it one: the arc's end swings round (a point moves 24.4 for a drag of 0.18). The
/// drag lands exactly, and the line runs through the fixed centre after it: forced geometry, as
/// in K.
fn seed_1_third() -> (Plan, Pick, DVec2) {
    let mut extras = vec![false; 48];
    extras[2] = true;
    extras[4] = true;
    (
        Plan {
            ops: vec![
                Op::Step {
                    from: 0,
                    angle: 7,
                    len: 1,
                },
                Op::Mid {
                    line: 0,
                    angle: 13,
                    len: 0,
                },
                Op::Step {
                    from: 62,
                    angle: 7,
                    len: 2,
                },
                Op::Arc {
                    from: 2,
                    angle: 7,
                    radius: 2,
                    sweep: 1,
                    ccw: false,
                },
                Op::Step {
                    from: 0,
                    angle: 8,
                    len: 0,
                },
            ],
            extras,
            fix: Some(53),
            length: None,
        },
        Pick::Point(12),
        DVec2::new(-0.17943552999752596, 0.0),
    )
}

#[test]
fn l_a_point_on_a_fixed_centre_swings_its_radius_round() {
    let (plan, pick, offset) = seed_1_third();
    explained(continuous(&plan, &pick, offset)).unwrap();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(forced_swing(&built, &ids, &run, offset).is_some());
}

/// The property passes a jump on that account only where the swing is the mechanism: a dragged
/// point on a fixed point that no line of its runs through is still judged (seed 1's sketch
/// without the relation that holds the point on its line: it shares a fixed point's place, held
/// by nothing that joins them), and so is a swing start whose drag didn't land.
#[test]
fn k_a_point_merely_on_another_is_still_judged() {
    let (mut plan, pick, offset) = seed_1();
    plan.extras[2] = false;
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let i = built.points.iter().position(|p| *p == ids[0]).unwrap();
    assert!(
        (0..sketch.points.len()).any(|j| j != i && sketch.points[j] == sketch.points[i]),
        "the dragged point still shares its place"
    );
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(forced_swing(&built, &ids, &run, offset).is_none());
    // Seed 1's own swing, as if it hadn't landed: judged.
    let (plan, pick, offset) = seed_1();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let mut run = drag(&mut built, &ids, offset, 1).unwrap();
    run.applied = offset / 2.0;
    assert!(forced_swing(&built, &ids, &run, offset).is_none());
}

/// In a forced swing a point is excused only as far as what hangs on the swinging line goes
/// (`SWING_MIRROR` times the line's reach): every point of seed 1's real swing passes, judged
/// with the property's own rounding allowance; a point pushed out to 2.5 times the reach fails,
/// and so does a jump of a point far from the swing.
#[test]
fn k_a_point_that_jumps_beyond_the_swing_is_still_judged() {
    let (plan, pick, offset) = seed_1();
    let sketch = plan.sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    let reach = forced_swing(&built, &ids, &run, offset).expect("seed 1 swings");
    let allowed =
        |run: &Run, k: usize| 100.0 * offset.length() + LANDS * run.start[k].length().max(1.0);
    for k in 0..run.end.len() {
        assert!(
            judged_move(&run, k, Some(reach)) <= allowed(&run, k),
            "point {k}"
        );
    }
    // Out to 2.5 times the reach, along the way it went.
    let mut pushed = run.clone();
    let k = (0..run.end.len())
        .max_by(|&a, &b| {
            run.start[a]
                .distance(run.end[a])
                .total_cmp(&run.start[b].distance(run.end[b]))
        })
        .unwrap();
    let way = (run.end[k] - run.start[k]).normalize();
    pushed.end[k] = run.start[k] + way * (2.5 * reach);
    assert!(judged_move(&pushed, k, Some(reach)) > allowed(&pushed, k));
    // A point far from the swing jumping: the farthest from the dragged point, moved 50.
    let i = built.points.iter().position(|p| *p == ids[0]).unwrap();
    let far = (0..run.end.len())
        .max_by(|&a, &b| {
            run.start[a]
                .distance(run.start[i])
                .total_cmp(&run.start[b].distance(run.start[i]))
        })
        .unwrap();
    let mut jumped = run.clone();
    jumped.end[far] += DVec2::new(0.0, 50.0);
    assert!(judged_move(&jumped, far, Some(reach)) > allowed(&jumped, far));
}

/// #284's case with an angle relation in its part (its two level lines held parallel, which
/// they already are), the sketch turned 37° (lines exactly level would leave a sine of exactly
/// nothing anywhere): the hanging line keeps its length at 0.1×, 1× and 10×, and 30 m, 100 m
/// and a kilometre out. The try's check holds angle rows to the solver's own (size-free)
/// tolerance; held to the length slack, a sine is finer than round-off can tell far out.
#[test]
fn h_untouched_line_kept_with_an_angle_relation_at_any_size_and_place() {
    let sketch = h_plan().sketch().unwrap();
    for (scale, at) in [
        (0.1, 0.0),
        (1.0, 0.0),
        (10.0, 0.0),
        (1.0, 3e4),
        (1.0, 1e5),
        (1.0, 1e6),
    ] {
        let t = Similarity {
            scale,
            turn: 37.0,
            shift: DVec2::new(at, at),
        };
        let mut built = Built::new(&sketch, t).unwrap();
        built.ed.doc.begin();
        built
            .ed
            .doc
            .add_constraint(Constraint::Parallel {
                a: built.curves[1],
                b: built.curves[2],
            })
            .unwrap();
        built.ed.doc.commit("Parallel");
        let ids = Pick::Curves(13773849351002921577).ids(&built);
        let run = drag(
            &mut built,
            &ids,
            t.vector(DVec2::new(16.377975070732383, 0.0)),
            1,
        )
        .unwrap();
        let (before, after) = (
            run.start[5].distance(run.start[6]),
            run.end[5].distance(run.end[6]),
        );
        assert!(
            (after - before).abs() <= LANDS * before,
            "at {scale}×, ({at}, {at}): line 2 went from {before} to {after} long"
        );
    }
}

/// A drag a kilometre out (a census case: a line hanging from a midpoint, held perpendicular
/// and equal to another, a curve and points on curves, dragged in 17 frames), where each
/// frame's solve stops with the kept lengths off by about a billionth of themselves. That isn't
/// a stretch, and isn't tried again: every frame is exactly as with no try at all. (Taken for
/// one, the try landed across the fold on some frames, and the line jumped out by up to 2.3 mm
/// for a frame and back the next.)
#[test]
fn a_solves_leftover_far_out_is_not_taken_for_a_stretch() {
    let plan = Plan {
        ops: vec![
            Op::Dup {
                from: 37,
                angle: 4,
                len: 0,
            },
            Op::OnLine {
                line: 34,
                at: 3,
                angle: 7,
                len: 2,
            },
            Op::OnArc {
                arc: 33,
                at: 10,
                angle: 0,
                len: 3,
            },
            Op::Step {
                from: 30,
                angle: 23,
                len: 3,
            },
            Op::Mid {
                line: 15,
                angle: 4,
                len: 1,
            },
            Op::Arc {
                from: 39,
                angle: 2,
                radius: 2,
                sweep: 0,
                ccw: false,
            },
            Op::Step {
                from: 56,
                angle: 10,
                len: 0,
            },
        ],
        extras: vec![
            false, true, true, false, true, false, true, false, false, true, false, false, true,
            false, false, true, true, true, false, true, true, false, false, true, true, false,
            false, true, false, false, true, true, false, true, false, true, true, false, true,
            false, true, false, false, false, true, false, true, false,
        ],
        fix: None,
        length: None,
    };
    let sketch = plan.sketch().unwrap();
    let t = Similarity {
        scale: 1.0,
        turn: 0.0,
        shift: DVec2::new(1e6, 1e6),
    };
    let offset = DVec2::new(3.1715384551452965, 2.422796244862606);
    let frames = |no_try: bool| {
        let mut built = Built::new(&sketch, t).unwrap();
        built.ed.drag_no_length_try = no_try;
        let ids = Pick::Curves(10082016649297418272).ids(&built);
        let dragged: Vec<Param> = built
            .ed
            .doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .flat_map(Param::point)
            .collect();
        built.ed.doc.begin();
        let mut applied = DVec2::ZERO;
        let mut out = Vec::new();
        for k in 1..=17 {
            let target = offset * (f64::from(k) / 17.0);
            if built.ed.drag(&ids, target - applied, &dragged) {
                applied = target;
            }
            out.push(built.positions());
        }
        out
    };
    let (with, without) = (frames(false), frames(true));
    for (k, (a, b)) in with.iter().zip(&without).enumerate() {
        assert_eq!(a, b, "frame {}: differs from the drag with no try", k + 1);
    }
}

/// #280: near the origin a drag gives the same result at other sizes and turns too, not only
/// at the similarity its pin was found at: `at_a_fold`'s and `after_277`'s cases at 0.3×,
/// 3.7×, turned 37°, at 2× turned 113° and shifted, and at 0.1× shifted. Each fails without
/// the size-free yardstick (`after_277`'s also without its damping part).
#[test]
fn size_free_near_the_origin_at_other_similarities() {
    let ts = [
        Similarity {
            scale: 0.3,
            turn: 0.0,
            shift: DVec2::ZERO,
        },
        Similarity {
            scale: 3.7,
            turn: 0.0,
            shift: DVec2::ZERO,
        },
        Similarity {
            scale: 1.0,
            turn: 37.0,
            shift: DVec2::ZERO,
        },
        Similarity {
            scale: 2.0,
            turn: 113.0,
            shift: DVec2::new(-300.0, 200.0),
        },
        Similarity {
            scale: 0.1,
            turn: 0.0,
            shift: DVec2::new(40.0, -40.0),
        },
    ];
    for (name, (plan, pick, offset, steps)) in [
        ("at_a_fold", at_a_fold_case()),
        ("after_277", after_277_case()),
    ] {
        for t in ts {
            if let Err(e) = invariant(&plan, &pick, offset, steps, t) {
                panic!("{name} at {t:?}: {e}");
            }
        }
    }
}

/// Similarity 4's shrunk case: an arc whose centre a 14 mm drag of two curves flings metres
/// away. Main flings it in exact proportion at every size (8.8 m at 1×), so the property held;
/// a near solve flings it less (0.68 m at 1×) but not in proportion (also at 3.7×, both copies
/// near), and the ¼ copy, 305 mm out (38 part-sizes), is far and main's bit for bit. Round-off
/// at a degenerate fold, and a band pair. See #280.
#[test]
#[ignore = "#280: round-off at a degenerate fold (and a band pair at ¼ size, 305 mm out)"]
fn e_result_depends_on_size_flung_centre() {
    let plan = Plan {
        ops: vec![
            Op::Arc {
                from: 0,
                angle: 5,
                radius: 0,
                sweep: 2,
                ccw: false,
            },
            Op::Tangent {
                arc: 0,
                end: false,
                forward: false,
                len: 0,
            },
            Op::OnLine {
                line: 0,
                at: 0,
                angle: 11,
                len: 0,
            },
            Op::Step {
                from: 38,
                angle: 10,
                len: 2,
            },
        ],
        extras: vec![false; 48],
        fix: Some(7),
        length: None,
    };
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 305.4097759673246),
    };
    let offset = DVec2::new(13.614670273256806, -4.64045524551462);
    explained(invariant(
        &plan,
        &Pick::Curves(6110175917806659770),
        offset,
        1,
        t,
    ))
    .unwrap();
}

/// A drag census case 1 m from the origin whose part sits 17.1 times its own size out, just
/// inside where drag solves stop being judged by the part's size: on its last frame the near
/// solve lands a deformed member of the shape's family (points moving 0.06 to 13.8 mm) where
/// main, and the same drag at the origin, carry the part rigidly 18.9 mm with the pointer: a
/// branch choice at the edge, not a stall (holding its capped solve changes nothing). See
/// #280.
#[test]
#[ignore = "#280: a branch choice at the hand-over (17.1 part-sizes out), 19 mm on the last frame"]
fn e_result_depends_on_size_at_the_hand_over() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 60,
                angle: 3,
                len: 0,
            },
            Op::Arc {
                from: 45,
                angle: 5,
                radius: 0,
                sweep: 2,
                ccw: true,
            },
            Op::Step {
                from: 51,
                angle: 0,
                len: 2,
            },
            Op::Arc {
                from: 13,
                angle: 5,
                radius: 2,
                sweep: 4,
                ccw: false,
            },
            Op::Step {
                from: 7,
                angle: 8,
                len: 0,
            },
            Op::OnArc {
                arc: 18,
                at: 9,
                angle: 5,
                len: 1,
            },
        ],
        extras: vec![
            true, false, true, false, false, false, false, true, false, false, false, false, false,
            false, false, false, false, false, false, false, false, false, false, false, false,
            true, false, false, true, false, true, false, false, false, false, true, false, true,
            false, true, true, false, false, false, false, false, false, false,
        ],
        fix: None,
        length: Some(57),
    };
    let t = Similarity {
        scale: 1.0,
        turn: 0.0,
        shift: DVec2::new(707.0, 707.0),
    };
    let offset = DVec2::new(-13.261484784801961, -13.40162004384744);
    explained(invariant(
        &plan,
        &Pick::Curves(17952534259162593868),
        offset,
        5,
        t,
    ))
    .unwrap();
}

/// `e_result_depends_on_size_crawl_at_the_cap`'s sketch.
fn crawl_at_the_cap_plan() -> Plan {
    Plan {
        ops: vec![
            Op::Arc {
                from: 57,
                angle: 16,
                radius: 2,
                sweep: 0,
                ccw: true,
            },
            Op::Arc {
                from: 31,
                angle: 15,
                radius: 0,
                sweep: 2,
                ccw: true,
            },
            Op::Step {
                from: 41,
                angle: 22,
                len: 2,
            },
            Op::Tangent {
                arc: 6,
                end: true,
                forward: true,
                len: 3,
            },
            Op::Arc {
                from: 1,
                angle: 7,
                radius: 1,
                sweep: 3,
                ccw: true,
            },
            Op::Tangent {
                arc: 52,
                end: true,
                forward: false,
                len: 0,
            },
            Op::Arc {
                from: 14,
                angle: 7,
                radius: 2,
                sweep: 0,
                ccw: true,
            },
        ],
        extras: vec![
            true, false, false, true, true, false, false, true, false, true, false, true, false,
            true, false, false, true, true, false, false, true, true, false, false, false, true,
            false, false, true, false, false, false, true, false, false, false, false, false,
            false, true, false, false, true, false, false, true, true, true,
        ],
        fix: None,
        length: None,
    }
}

/// A drag census case at the origin: near solves at a fold crawl to a residual 1.4 times the
/// part-size bar (under today's, which main's solve reads as holding) and stop at the
/// iteration cap, so the drag ends 1 to 7 µm apart at 0.1×, 1×, 3.7× and 10×, where main
/// ends the same at every size. Round-off at a fold deciding. See #280.
#[test]
#[ignore = "#280: a fold's crawl cut by the cap at one size, 1-7 um"]
fn e_result_depends_on_size_crawl_at_the_cap() {
    let plan = crawl_at_the_cap_plan();
    let t = Similarity {
        scale: 3.7,
        turn: 0.0,
        shift: DVec2::ZERO,
    };
    let offset = DVec2::new(-15.45277997518099, 2.1188318980890206);
    explained(invariant(
        &plan,
        &Pick::Curves(4751599557133155727),
        offset,
        15,
        t,
    ))
    .unwrap();
}

/// Every solve of a drag frame is judged against one part size, the part's as the frame
/// began (`DragFrame::size`: the part as the frame's first solve starts from it, the dragged
/// points at their targets, never a retake's), however its retakes and steps fling the
/// geometry:
/// `tangent_and_parallel`'s drag, whose one frame takes dozens of solves (the arc flung to
/// metres mid-try), reads one size in every solve. (Read per solve, its sizes ran from 7 mm to
/// 610 m within the frame, each judged "near" by its own.)
#[test]
fn every_solve_of_a_frame_reads_the_frames_size() {
    let sketch = tangent_and_parallel_plan().sketch().unwrap();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = Pick::Curves(9929081795026096183).ids(&built);
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    // The part as the frame's first solve starts from it: the dragged points moved by the
    // frame's offset, the rest where the frame found them. Twice its farthest point from its
    // points' middle (it has arcs, no circles), as the solver's `drawing_across` reads it.
    let offset = DVec2::new(15.457267808482761, -17.270854180887095);
    let moved: std::collections::HashSet<EntityId> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .collect();
    let points: Vec<DVec2> = built
        .ed
        .doc
        .iter()
        .filter(|(_, e)| e.kind.is_point())
        .map(|(id, _)| {
            built.ed.doc.point(id).unwrap()
                + if moved.contains(&id) {
                    offset
                } else {
                    DVec2::ZERO
                }
        })
        .collect();
    let middle = points.iter().sum::<DVec2>() / points.len() as f64;
    let start = 2.0
        * points
            .iter()
            .map(|p| p.distance(middle))
            .fold(0.0, f64::max);
    built.ed.doc.begin();
    built.ed.drag_sizes = Some(Vec::new());
    built.ed.drag(
        &ids,
        DVec2::new(15.457267808482761, -17.270854180887095),
        &dragged,
    );
    let sizes: Vec<Option<f64>> = built
        .ed
        .drag_sizes
        .as_ref()
        .unwrap()
        .iter()
        .map(|s| s.0)
        .collect();
    assert!(sizes.len() >= 3, "only {} solves", sizes.len());
    assert!(
        sizes[0].is_some_and(|a| (a - start).abs() <= 1e-12 * start),
        "{:?}, the starting part {start}",
        sizes[0]
    );
    assert!(sizes.iter().all(|a| *a == sizes[0]), "{sizes:?}");
}

/// No stall is read as landed, at any size: the drag of `crawl_at_the_cap`, whose near solves
/// crawl at a fold and stop at the iteration cap a hair off the part-size bar (15 times in its
/// frames), at 0.1×, 1× and 10× about the origin. Every frame that goes meets every relation,
/// and every refused one changes nothing (`drag`'s own checks). Counting such a capped solve
/// as converged would let frames go with their relations unmet, and this fails.
#[test]
fn no_stall_is_read_as_landed_at_any_size() {
    let sketch = crawl_at_the_cap_plan().sketch().unwrap();
    for scale in [0.1, 1.0, 10.0] {
        let t = Similarity {
            scale,
            turn: 0.0,
            shift: DVec2::ZERO,
        };
        let mut built = Built::new(&sketch, t).unwrap();
        let ids = Pick::Curves(4751599557133155727).ids(&built);
        let offset = t.vector(DVec2::new(-15.45277997518099, 2.1188318980890206));
        if let Err(e) = drag(&mut built, &ids, offset, 15) {
            panic!("{scale}×: {e}");
        }
    }
}

/// A drag census case 10 m from the origin, its part some 100 times its own size out, so
/// every solve of every frame is far and today's: none is judged by the part's size, not
/// even the landing pass of a frame's first solve after its run has flung a point some
/// 0.9 m (read afresh there, the flung geometry measured 889 mm, a "near" part). It reads the
/// size its run began with.
#[test]
fn a_landing_pass_reads_the_size_its_solve_began_with() {
    let plan = Plan {
        ops: vec![
            Op::OnLine {
                line: 45,
                at: 0,
                angle: 10,
                len: 1,
            },
            Op::Arc {
                from: 27,
                angle: 10,
                radius: 0,
                sweep: 0,
                ccw: true,
            },
            Op::Step {
                from: 38,
                angle: 1,
                len: 3,
            },
            Op::OnLine {
                line: 8,
                at: 2,
                angle: 15,
                len: 2,
            },
            Op::Tangent {
                arc: 29,
                end: false,
                forward: true,
                len: 1,
            },
            Op::Step {
                from: 48,
                angle: 19,
                len: 3,
            },
            Op::Tangent {
                arc: 3,
                end: false,
                forward: false,
                len: 2,
            },
        ],
        extras: vec![
            false, true, false, false, false, false, false, true, true, false, true, true, false,
            true, false, false, true, false, false, true, false, true, false, true, false, true,
            true, false, false, false, true, false, true, false, true, false, false, true, true,
            true, false, true, false, true, false, true, false, false,
        ],
        fix: None,
        length: None,
    };
    let sketch = plan.sketch().unwrap();
    let t = Similarity {
        scale: 1.0,
        turn: 0.0,
        shift: DVec2::new(7071.0, 7071.0),
    };
    let mut built = Built::new(&sketch, t).unwrap();
    let ids = Pick::Curves(9750649514805135734).ids(&built);
    built.ed.drag_sizes = Some(Vec::new());
    drag(
        &mut built,
        &ids,
        DVec2::new(0.6120504876783168, -14.22643746812479),
        16,
    )
    .unwrap();
    let sizes = built.ed.drag_sizes.as_ref().unwrap();
    assert!(!sizes.is_empty());
    assert!(
        sizes.iter().all(|(_, near, _, _)| !near),
        "a solve judged near: {sizes:?}"
    );
}

/// Similarity seed 15's shrunk case at 3.7×, near the origin at both sizes: a frame refused
/// at one size and not the other. It passed with near patience cuts counted as converged
/// whatever their rows; with a near patience cut off today's bar reported unfinished (which
/// bounds the work a refused drag on a big part takes), one size's frame is refused. That
/// is the one gain the rule gives back (also at 10×); main fails it at both. See #280.
#[test]
#[ignore = "#280: a patience cut off today's bar at one size only (3.7x, 10x)"]
fn e_result_depends_on_size_patience_cut_at_one_size() {
    let plan = Plan {
        ops: vec![
            Op::Step {
                from: 0,
                angle: 7,
                len: 0,
            },
            Op::Arc {
                from: 0,
                angle: 6,
                radius: 0,
                sweep: 3,
                ccw: false,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: true,
                len: 2,
            },
            Op::Tangent {
                arc: 0,
                end: true,
                forward: false,
                len: 0,
            },
            Op::Step {
                from: 5,
                angle: 6,
                len: 1,
            },
            Op::Arc {
                from: 53,
                angle: 2,
                radius: 1,
                sweep: 1,
                ccw: true,
            },
        ],
        extras: (0..48).map(|i| i == 1).collect(),
        fix: Some(0),
        length: Some(45),
    };
    let t = Similarity {
        scale: 3.7,
        turn: 0.0,
        shift: DVec2::ZERO,
    };
    let offset = DVec2::new(-0.5016234454192522, -6.8859008828541945);
    explained(invariant(
        &plan,
        &Pick::Curves(16880784305534777784),
        offset,
        10,
        t,
    ))
    .unwrap();
}
