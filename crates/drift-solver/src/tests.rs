use std::f64::consts::PI;

use drift_core::{Constraint, DVec2, Document, EntityId, EntityKind};
use proptest::prelude::*;

use super::*;
use crate::system::System;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn solved(doc: &mut Document) -> SolveReport {
    solve(
        doc,
        &SolveOptions {
            analyze: true,
            ..Default::default()
        },
    )
}

/// A slightly skewed quadrilateral: corners p[0..4], edges l[0..4] (bottom, right, top, left).
fn quad(doc: &mut Document, origin: DVec2) -> ([EntityId; 4], [EntityId; 4]) {
    let corners = [v(0.0, 0.0), v(31.0, 1.5), v(29.0, 22.0), v(-1.0, 19.0)];
    let p = corners.map(|c| doc.add_point(origin + c));
    let l = [0, 1, 2, 3].map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
    (p, l)
}

fn constrain_rectangle(doc: &mut Document, p: [EntityId; 4], l: [EntityId; 4], origin: DVec2) {
    let c = |doc: &mut Document, c| {
        doc.add_constraint(c).unwrap();
    };
    c(doc, Constraint::Horizontal { line: l[0] });
    c(doc, Constraint::Horizontal { line: l[2] });
    c(doc, Constraint::Vertical { line: l[1] });
    c(doc, Constraint::Vertical { line: l[3] });
    c(
        doc,
        Constraint::Fix {
            point: p[0],
            at: origin,
        },
    );
    c(
        doc,
        Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 30.0,
        },
    );
    c(
        doc,
        Constraint::Distance {
            a: p[1],
            b: p[2],
            value: 20.0,
        },
    );
}

#[test]
fn rectangle_is_solved_exactly_and_fully_defined() {
    let mut doc = Document::new();
    let (p, l) = quad(&mut doc, DVec2::ZERO);
    constrain_rectangle(&mut doc, p, l, DVec2::ZERO);
    let report = solved(&mut doc);
    assert!(report.converged, "{report:?}");
    assert_eq!(report.dof, Some(0));
    let expected = [v(0.0, 0.0), v(30.0, 0.0), v(30.0, 20.0), v(0.0, 20.0)];
    for (id, want) in p.iter().zip(expected) {
        let got = doc.point(*id).unwrap();
        // (Within the solver's bar: a billionth of a quarter of the sketch's size.)
        assert!((got - want).length() < 1e-8, "{got} vs {want}");
    }
    assert!(
        report.definitions.values().all(|d| *d == Definition::Full),
        "{:?}",
        report.definitions
    );
}

#[test]
fn rectangle_far_from_origin_keeps_full_precision() {
    let origin = v(1.0e6, -2.0e6);
    let mut doc = Document::new();
    let (p, l) = quad(&mut doc, origin);
    constrain_rectangle(&mut doc, p, l, origin);
    let report = solved(&mut doc);
    assert!(report.converged);
    let width = doc.point(p[1]).unwrap() - doc.point(p[0]).unwrap();
    assert!((width.x - 30.0).abs() < 1e-8 && width.y == 0.0, "{width}");
}

#[test]
fn dof_counts_remaining_freedom() {
    let mut doc = Document::new();
    let (_, l) = quad(&mut doc, DVec2::ZERO);
    // Four free points: 8 DOF. Horizontal removes one.
    assert_eq!(analyze(&doc).dof, Some(8));
    doc.add_constraint(Constraint::Horizontal { line: l[0] })
        .unwrap();
    let report = solved(&mut doc);
    assert_eq!(report.dof, Some(7));
    assert!(report.definitions.values().all(|d| *d == Definition::Under));
}

#[test]
fn conflicting_dimensions_are_reported() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 0.0));
    let line = doc.add_line(a, b).unwrap();
    let c1 = doc
        .add_constraint(Constraint::Distance { a, b, value: 10.0 })
        .unwrap();
    let c2 = doc
        .add_constraint(Constraint::Distance { a, b, value: 20.0 })
        .unwrap();
    let report = solved(&mut doc);
    assert!(!report.converged);
    assert_eq!(report.conflicts, vec![c1, c2]);
    assert_eq!(report.definitions[&line], Definition::Over);
}

#[test]
fn conflicting_fixes_through_coincidence_are_reported() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(5.0, 0.0));
    doc.add_constraint(Constraint::Fix {
        point: a,
        at: v(0.0, 0.0),
    })
    .unwrap();
    let fix_b = doc
        .add_constraint(Constraint::Fix {
            point: b,
            at: v(5.0, 0.0),
        })
        .unwrap();
    doc.add_constraint(Constraint::Coincident { a, b }).unwrap();
    let report = solved(&mut doc);
    assert!(!report.converged);
    assert!(report.conflicts.contains(&fix_b));
}

#[test]
fn two_fixes_on_one_point_agree_or_conflict() {
    // As when two fixed points are merged into one.
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    doc.add_constraint(Constraint::Fix {
        point: a,
        at: v(0.0, 0.0),
    })
    .unwrap();
    let same = doc
        .add_constraint(Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        })
        .unwrap();
    let report = solved(&mut doc);
    assert!(report.converged && report.conflicts.is_empty());
    doc.remove_constraint(same);
    let apart = doc
        .add_constraint(Constraint::Fix {
            point: a,
            at: v(0.2, 0.0),
        })
        .unwrap();
    let report = solved(&mut doc);
    assert!(!report.converged);
    assert!(report.conflicts.contains(&apart), "{report:?}");
}

#[test]
fn redundancy_is_detected() {
    let mut doc = Document::new();
    let (p, l) = quad(&mut doc, DVec2::ZERO);
    doc.add_constraint(Constraint::Horizontal { line: l[0] })
        .unwrap();
    let again = doc
        .add_constraint(Constraint::HorizontalPoints { a: p[1], b: p[0] })
        .unwrap();
    // Equal + both lengths: one equation is implied.
    doc.add_constraint(Constraint::Equal { a: l[1], b: l[3] })
        .unwrap();
    doc.add_constraint(Constraint::Distance {
        a: p[1],
        b: p[2],
        value: 20.0,
    })
    .unwrap();
    doc.add_constraint(Constraint::Distance {
        a: p[3],
        b: p[0],
        value: 20.0,
    })
    .unwrap();
    let report = solved(&mut doc);
    assert!(report.converged);
    assert_eq!(report.redundant, vec![again]);
    assert_eq!(report.dependent_equations, 1);
}

#[test]
fn dragging_keeps_the_dragged_point_and_moves_the_rest() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 0.0));
    doc.add_line(a, b).unwrap();
    doc.add_constraint(Constraint::Distance { a, b, value: 10.0 })
        .unwrap();
    // Drag b to (20, 0): the line can't stretch, so `a` follows.
    doc.set_point(b, v(20.0, 0.0)).unwrap();
    let report = solve(
        &mut doc,
        &SolveOptions {
            dragged: Param::point(b).to_vec(),
            ..Default::default()
        },
    );
    assert!(report.converged);
    let (pa, pb) = (doc.point(a).unwrap(), doc.point(b).unwrap());
    assert!((pa.distance(pb) - 10.0).abs() < 1e-9);
    assert!(
        pb.distance(v(20.0, 0.0)) < 1e-4,
        "dragged point stayed put: {pb}"
    );
    assert!(pa.distance(v(10.0, 0.0)) < 1e-3, "other end moved: {pa}");
}

#[test]
fn fixed_geometry_springs_back() {
    let mut doc = Document::new();
    let a = doc.add_point(v(1.0, 2.0));
    doc.add_constraint(Constraint::Fix {
        point: a,
        at: v(1.0, 2.0),
    })
    .unwrap();
    doc.set_point(a, v(9.0, 9.0)).unwrap();
    assert!(solved(&mut doc).converged);
    assert_eq!(doc.point(a).unwrap(), v(1.0, 2.0));
}

#[test]
fn angle_parallel_and_perpendicular() {
    let mut doc = Document::new();
    let o = doc.add_point(v(0.0, 0.0));
    let x = doc.add_point(v(10.0, 0.5));
    let y = doc.add_point(v(3.0, 8.0));
    let z = doc.add_point(v(-4.0, 6.0));
    let l1 = doc.add_line(o, x).unwrap();
    let l2 = doc.add_line(o, y).unwrap();
    let l3 = doc.add_line(y, z).unwrap();
    doc.add_constraint(Constraint::Horizontal { line: l1 })
        .unwrap();
    let angle = Constraint::Angle {
        a: l1,
        b: l2,
        degrees: 60.0,
    };
    doc.add_constraint(angle).unwrap();
    doc.add_constraint(Constraint::Perpendicular { a: l2, b: l3 })
        .unwrap();
    assert!(solved(&mut doc).converged);
    assert!((measure(&doc, &angle).unwrap() - 60.0).abs() < 1e-9);
    let d2 = doc.point(y).unwrap() - doc.point(o).unwrap();
    let d3 = doc.point(z).unwrap() - doc.point(y).unwrap();
    assert!(d2.dot(d3).abs() < 1e-9 * d2.length() * d3.length());

    doc.add_constraint(Constraint::Parallel { a: l1, b: l3 })
        .unwrap();
    // Parallel to l1 (horizontal) and perpendicular to l2 (60°) can't both hold.
    assert!(!solved(&mut doc).converged);
}

#[test]
fn arc_radius_dimension_keeps_end_on_circle() {
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(v(5.0, 0.0));
    let e = doc.add_point(v(0.0, 5.0));
    let arc = doc.add_arc(c, s, e).unwrap();
    doc.add_constraint(Constraint::Radius {
        curve: arc,
        value: 8.0,
    })
    .unwrap();
    let report = solved(&mut doc);
    assert!(report.converged);
    let (pc, ps, pe) = (
        doc.point(c).unwrap(),
        doc.point(s).unwrap(),
        doc.point(e).unwrap(),
    );
    assert!((pc.distance(ps) - 8.0).abs() < 1e-9);
    assert!((pc.distance(pe) - 8.0).abs() < 1e-9);
}

#[test]
fn equal_radius_between_circle_and_arc() {
    let mut doc = Document::new();
    let c1 = doc.add_point(v(0.0, 0.0));
    let circle = doc.add_circle(c1, 3.0).unwrap();
    let c2 = doc.add_point(v(20.0, 0.0));
    let s = doc.add_point(v(25.0, 0.0));
    let e = doc.add_point(v(20.0, 5.0));
    let arc = doc.add_arc(c2, s, e).unwrap();
    doc.add_constraint(Constraint::Equal { a: circle, b: arc })
        .unwrap();
    assert!(solved(&mut doc).converged);
    let EntityKind::Circle { radius, .. } = doc.get(circle).unwrap().kind else {
        panic!()
    };
    let arc_r = doc.point(c2).unwrap().distance(doc.point(s).unwrap());
    assert!((radius - arc_r).abs() < 1e-9);
    // Minimum-norm: they meet in the middle.
    assert!((radius - 4.0).abs() < 0.2, "{radius}");
}

#[test]
fn already_satisfied_system_does_not_move() {
    let mut doc = Document::new();
    let (p, l) = quad(&mut doc, DVec2::ZERO);
    constrain_rectangle(&mut doc, p, l, DVec2::ZERO);
    solved(&mut doc);
    let before = doc.clone();
    let rev = doc.revision();
    let report = solved(&mut doc);
    assert_eq!(report.iterations, 0);
    assert_eq!(doc, before);
    assert_eq!(doc.revision(), rev, "no writes when nothing changes");
}

#[test]
fn measure_reports_angles_modulo_180() {
    let mut doc = Document::new();
    let o = doc.add_point(v(0.0, 0.0));
    let a = doc.add_point(v(1.0, 0.0));
    let b = doc.add_point(v(-1.0, -1.0));
    let l1 = doc.add_line(o, a).unwrap();
    let l2 = doc.add_line(o, b).unwrap();
    let angle = Constraint::Angle {
        a: l1,
        b: l2,
        degrees: 0.0,
    };
    // Direction of l2 is 225°, i.e. 45° as an undirected line.
    assert!((measure(&doc, &angle).unwrap() - 45.0).abs() < 1e-12);
    assert!((PI / 4.0).to_degrees() - 45.0 < 1e-12);
}

/// Dense (reference) and sparse rank analyses of every component of `doc` must agree.
fn assert_rank_analyses_agree(doc: &Document) -> Result<(), TestCaseError> {
    let sys = System::build(doc, &[], &Tolerances::default(), None);
    let (comps, _) = components(&sys, &Tolerances::default());
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    for comp in &comps {
        let n = comp.unknowns.len();
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = evaluate(&sys, &comp.rows, &values, &col_of);
        let (pivots, j) = dense_rref_for_test(&jac, n);
        let mut is_pivot = vec![false; n];
        for &p in &pivots {
            is_pivot[p] = true;
        }
        let mut dense_movable = vec![true; n];
        for (row, &pc) in pivots.iter().enumerate() {
            dense_movable[pc] = (0..n).any(|c| !is_pivot[c] && j.at(row, c).abs() > 1e-7);
        }
        let (rank, sparse_movable) = sparse_rank_for_test(&jac, n);
        prop_assert_eq!(rank, pivots.len());
        prop_assert_eq!(sparse_movable, dense_movable);
    }
    Ok(())
}

#[test]
fn sparse_and_dense_rank_analysis_agree() {
    // Chain of rectangles with a few free parts, analysed both ways.
    let mut doc = Document::new();
    let mut prev = None;
    for i in 0..12 {
        let origin = v(f64::from(i) * 40.0, 0.0);
        let (p, l) = quad(&mut doc, origin);
        doc.add_constraint(Constraint::Horizontal { line: l[0] })
            .unwrap();
        doc.add_constraint(Constraint::Perpendicular { a: l[0], b: l[1] })
            .unwrap();
        if i % 3 != 0 {
            doc.add_constraint(Constraint::Perpendicular { a: l[1], b: l[2] })
                .unwrap();
        }
        if let Some(q) = prev {
            doc.add_constraint(Constraint::Distance {
                a: q,
                b: p[0],
                value: 10.0,
            })
            .unwrap();
        }
        prev = Some(p[1]);
    }
    assert!(solved(&mut doc).converged);
    assert_rank_analyses_agree(&doc).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Property test: random sketches with constraints that the original geometry satisfies are
// perturbed, then solved. The solver must always find a configuration satisfying everything.

#[derive(Clone, Debug)]
struct Case {
    lines: Vec<(DVec2, DVec2)>,
    /// (kind, line index, other line index)
    constraints: Vec<(u8, usize, usize)>,
    noise: Vec<DVec2>,
}

fn case() -> impl Strategy<Value = Case> {
    let coord = -50.0..50.0f64;
    let point = (coord.clone(), coord).prop_map(|(x, y)| DVec2::new(x, y));
    (2usize..6)
        .prop_flat_map(move |n| {
            (
                prop::collection::vec((point.clone(), point.clone()), n),
                prop::collection::vec((0u8..6, 0..n, 0..n), 1..8),
                prop::collection::vec((-1.0..1.0f64, -1.0..1.0f64), 2 * n),
            )
        })
        .prop_map(|(lines, constraints, noise)| Case {
            lines,
            constraints,
            noise: noise.into_iter().map(|(x, y)| DVec2::new(x, y)).collect(),
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn perturbed_satisfiable_sketches_converge(case in case()) {
        let mut doc = Document::new();
        let mut ends = Vec::new();
        let mut lines = Vec::new();
        for (a, b) in &case.lines {
            if a.distance(*b) < 1.0 {
                continue;
            }
            let pa = doc.add_point(*a);
            let pb = doc.add_point(*b);
            lines.push(doc.add_line(pa, pb).unwrap());
            ends.push((pa, pb));
        }
        prop_assume!(lines.len() >= 2);
        // Constraints measured from the current geometry, so the original state satisfies them.
        let mut used = std::collections::HashSet::new();
        for &(kind, i, j) in &case.constraints {
            let (i, j) = (i % lines.len(), j % lines.len());
            if !used.insert((kind, i, j)) {
                continue;
            }
            let constraint = match kind {
                0 => {
                    let (a, b) = ends[i];
                    let value = doc.point(a).unwrap().distance(doc.point(b).unwrap());
                    Constraint::Distance { a, b, value }
                }
                1 if i != j => {
                    let probe = Constraint::Angle { a: lines[i], b: lines[j], degrees: 0.0 };
                    Constraint::Angle { a: lines[i], b: lines[j], degrees: measure(&doc, &probe).unwrap() }
                }
                2 if i != j => {
                    let (a, _) = ends[i];
                    let (c, _) = ends[j];
                    let value = doc.point(a).unwrap().distance(doc.point(c).unwrap());
                    if value < 1e-3 { continue; }
                    Constraint::Distance { a, b: c, value }
                }
                3 => {
                    // Make the line horizontal first so the relation holds.
                    let (a, b) = ends[i];
                    let (pa, pb) = (doc.point(a).unwrap(), doc.point(b).unwrap());
                    doc.set_point(b, DVec2::new(pb.x, pa.y)).unwrap();
                    if (pb.x - pa.x).abs() < 1.0 { continue; }
                    Constraint::Horizontal { line: lines[i] }
                }
                4 if i != j => {
                    let (a, _) = ends[i];
                    let (c, _) = ends[j];
                    let pa = doc.point(a).unwrap();
                    let pc = doc.point(c).unwrap();
                    doc.set_point(c, DVec2::new(pa.x, pc.y)).unwrap();
                    Constraint::VerticalPoints { a, b: c }
                }
                _ => continue,
            };
            // Earlier edits may have broken earlier constraints; only keep consistent sets.
            if doc.validate(&constraint).is_err() { continue; }
            doc.add_constraint(constraint).unwrap();
        }
        let original = doc.clone();
        // Perturb every point by up to 2 mm.
        for (k, (a, b)) in ends.iter().enumerate() {
            for (p, n) in [(*a, case.noise[2 * k]), (*b, case.noise[2 * k + 1])] {
                let at = doc.point(p).unwrap() + n * 2.0;
                doc.set_point(p, at).unwrap();
            }
        }
        let report = solved(&mut doc);
        // Construction edits (making lines horizontal) can invalidate earlier dimensions, so
        // an unsatisfiable set is possible; it must then be reported, never silently "solved".
        if report.converged {
            prop_assert!(analyze(&doc).converged);
        } else {
            prop_assert!(!report.conflicts.is_empty(), "{:?}\n{:?}", report, original);
        }
        assert_rank_analyses_agree(&doc)?;
    }
}

#[test]
fn nearly_parallel_independent_columns_keep_the_right_rank() {
    // Regression: two equations over six unknowns, with two nearly (but not exactly) parallel
    // columns. Rank decisions on JᵀJ got this wrong; QR must report rank 2.
    let jac = vec![
        vec![
            (0, 0.24190313028392463),
            (1, -0.9703004048019555),
            (2, -0.24190313028392463),
            (3, 0.9703004048019555),
        ],
        vec![
            (4, 0.9999999056644415),
            (5, 0.00043436287586993765),
            (0, -0.9999999056644415),
            (1, -0.00043436287586993765),
        ],
    ];
    let (rank, movable) = sparse_rank_for_test(&jac, 6);
    assert_eq!(rank, 2);
    assert_eq!(movable, vec![true; 6]);
}

#[test]
fn horizontal_vertical_and_point_line_distances() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(7.0, 3.0));
    let c = doc.add_point(v(-2.0, 0.0));
    let d = doc.add_point(v(10.0, 1.0));
    let line = doc.add_line(c, d).unwrap();
    let p = doc.add_point(v(4.0, 6.0));
    let center = doc.add_point(v(20.0, 0.0));
    let circle = doc.add_circle(center, 2.0).unwrap();
    for constraint in [
        Constraint::HorizontalDistance { a, b, value: 12.0 },
        Constraint::VerticalDistance { a, b, value: 5.0 },
        Constraint::PointLineDistance {
            point: p,
            line,
            value: 2.5,
        },
        Constraint::Diameter {
            curve: circle,
            value: 9.0,
        },
    ] {
        doc.add_constraint(constraint).unwrap();
    }
    assert!(solved(&mut doc).converged);
    let (pa, pb) = (doc.point(a).unwrap(), doc.point(b).unwrap());
    assert!(((pb.x - pa.x) - 12.0).abs() < 1e-9, "b stays to the right");
    assert!(((pb.y - pa.y) - 5.0).abs() < 1e-9, "b stays above");
    for (_, c) in doc.constraints() {
        let want = c.value().unwrap();
        let got = measure(&doc, c).unwrap();
        assert!((got - want).abs() < 1e-9, "{c:?}: {got}");
    }
    // The point stays on the same side of the line.
    let (pc, pd, pp) = (
        doc.point(c).unwrap(),
        doc.point(d).unwrap(),
        doc.point(p).unwrap(),
    );
    assert!((pd - pc).perp_dot(pp - pc) > 0.0);
}

#[test]
fn midpoint_relation_keeps_a_point_centred() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 4.0));
    let line = doc.add_line(a, b).unwrap();
    let c = doc.add_point(v(3.0, 9.0));
    let d = doc.add_point(v(-4.0, 9.0));
    doc.add_line(c, d).unwrap();
    let mid = Constraint::Midpoint { point: c, line };
    doc.add_constraint(mid).unwrap();
    let report = solved(&mut doc);
    assert!(report.converged);
    let m = (doc.point(a).unwrap() + doc.point(b).unwrap()) * 0.5;
    // (Within the solver's bar: a billionth of a quarter of the sketch's size.)
    assert!((doc.point(c).unwrap() - m).length() < 1e-8);
    // Two equations: removes two degrees of freedom (8 - 2).
    assert_eq!(report.dof, Some(6));
    // A line's own end point can't be its midpoint.
    assert!(
        doc.add_constraint(Constraint::Midpoint { point: a, line })
            .is_err()
    );
}

#[test]
fn dimensions_can_measure_to_midpoints() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(20.0, 0.0));
    let base = doc.add_line(a, b).unwrap();
    let c = doc.add_point(v(3.0, 9.0));
    let d = doc.add_point(v(15.0, 30.0));
    let other = doc.add_line(c, d).unwrap();
    let before = analyze(&doc).dof;
    for constraint in [
        // From the base's midpoint to a point, horizontally and vertically.
        Constraint::HorizontalDistance {
            a: base,
            b: c,
            value: 4.0,
        },
        Constraint::VerticalDistance {
            a: base,
            b: c,
            value: 12.0,
        },
        // Midpoint to midpoint.
        Constraint::Distance {
            a: base,
            b: other,
            value: 25.0,
        },
    ] {
        doc.add_constraint(constraint).unwrap();
    }
    let report = solved(&mut doc);
    assert!(report.converged, "{report:?}");
    // Three dimensions remove three degrees of freedom; the hidden midpoints add none.
    assert_eq!(report.dof, before.map(|n| n - 3));
    let mid = |l| doc.ref_point(l).unwrap();
    let pc = doc.point(c).unwrap();
    // The point started left of the midpoint and stays on that side.
    assert!(((pc.x - mid(base).x) + 4.0).abs() < 1e-9);
    assert!(((pc.y - mid(base).y) - 12.0).abs() < 1e-9);
    assert!((mid(base).distance(mid(other)) - 25.0).abs() < 1e-9);
    for (_, k) in doc.constraints() {
        assert!((measure(&doc, k).unwrap() - k.value().unwrap()).abs() < 1e-9);
    }
    // Midpoints of lines aren't valid in relations that need real points.
    assert!(
        doc.add_constraint(Constraint::Coincident { a: base, b: c })
            .is_err()
    );
}

// ---- M4 relations ----

#[test]
fn line_tangent_to_circle_keeps_its_side() {
    let mut doc = Document::new();
    let a = doc.add_point(v(-10.0, 6.0));
    let b = doc.add_point(v(10.0, 7.0));
    let line = doc.add_line(a, b).unwrap();
    let c = doc.add_point(v(0.0, 0.0));
    let circle = doc.add_circle(c, 4.0).unwrap();
    doc.add_constraint(Constraint::Tangent { a: line, b: circle })
        .unwrap();
    assert!(solved(&mut doc).converged);
    let (pa, pb, pc) = (
        doc.point(a).unwrap(),
        doc.point(b).unwrap(),
        doc.point(c).unwrap(),
    );
    let EntityKind::Circle { radius, .. } = doc.get(circle).unwrap().kind else {
        panic!()
    };
    let d = pb - pa;
    let signed = d.perp_dot(pc - pa) / d.length();
    assert!((signed.abs() - radius).abs() < 1e-9);
    assert!(signed < 0.0, "the circle stays below the line");
}

#[test]
fn fillet_arc_tangent_to_lines_at_shared_end_points() {
    // An L corner with a slightly-off fillet arc sharing the lines' end points.
    let mut doc = Document::new();
    let p0 = doc.add_point(v(0.0, 30.0));
    let t1 = doc.add_point(v(0.0, 10.0));
    let t2 = doc.add_point(v(10.5, 0.0));
    let p3 = doc.add_point(v(30.0, 0.0));
    let c = doc.add_point(v(9.0, 9.0));
    let vertical = doc.add_line(p0, t1).unwrap();
    let horizontal = doc.add_line(t2, p3).unwrap();
    let arc = doc.add_arc(c, t2, t1).unwrap();
    for k in [
        Constraint::Vertical { line: vertical },
        Constraint::Horizontal { line: horizontal },
        Constraint::Tangent {
            a: vertical,
            b: arc,
        },
        Constraint::Tangent {
            a: horizontal,
            b: arc,
        },
    ] {
        doc.add_constraint(k).unwrap();
    }
    assert!(solved(&mut doc).converged);
    let (pc, pt1, pt2) = (
        doc.point(c).unwrap(),
        doc.point(t1).unwrap(),
        doc.point(t2).unwrap(),
    );
    // Tangent at the shared points: the radius there is perpendicular to the line.
    assert!((pc.y - pt1.y).abs() < 1e-9, "radius to t1 is horizontal");
    assert!((pc.x - pt2.x).abs() < 1e-9, "radius to t2 is vertical");
}

#[test]
fn circles_tangent_outside_and_inside() {
    for (second_center, second_radius, outside) in
        [(v(9.5, 0.0), 4.0, true), (v(1.5, 0.0), 2.0, false)]
    {
        let mut doc = Document::new();
        let c1 = doc.add_point(v(0.0, 0.0));
        let big = doc.add_circle(c1, 5.0).unwrap();
        let c2 = doc.add_point(second_center);
        let small = doc.add_circle(c2, second_radius).unwrap();
        doc.add_constraint(Constraint::Tangent { a: big, b: small })
            .unwrap();
        assert!(solved(&mut doc).converged);
        let r = |id| radius_of(&doc, id).unwrap();
        let dist = doc.point(c1).unwrap().distance(doc.point(c2).unwrap());
        let want = if outside {
            r(big) + r(small)
        } else {
            r(big) - r(small)
        };
        assert!(
            (dist - want).abs() < 1e-9,
            "outside {outside}: {dist} vs {want}"
        );
    }
}

#[test]
fn concentric_coradial_collinear_on_curve_and_symmetric() {
    let mut doc = Document::new();
    let c1 = doc.add_point(v(0.0, 0.0));
    let circle = doc.add_circle(c1, 5.0).unwrap();
    let c2 = doc.add_point(v(1.0, 0.5));
    let s = doc.add_point(v(4.0, 0.5));
    let e = doc.add_point(v(1.0, 3.5));
    let arc = doc.add_arc(c2, s, e).unwrap();
    let a1 = doc.add_point(v(10.0, 0.0));
    let b1 = doc.add_point(v(20.0, 1.0));
    let l1 = doc.add_line(a1, b1).unwrap();
    let a2 = doc.add_point(v(25.0, 2.0));
    let b2 = doc.add_point(v(35.0, 1.0));
    let l2 = doc.add_line(a2, b2).unwrap();
    let q = doc.add_point(v(3.0, 4.5));
    let m1 = doc.add_point(v(-8.0, 3.0));
    let m2 = doc.add_point(v(-1.0, 12.0));
    let ax0 = doc.add_point(v(-10.0, -10.0));
    let ax1 = doc.add_point(v(10.0, 10.0));
    let axis = doc.add_line(ax0, ax1).unwrap();
    for k in [
        Constraint::Coradial { a: circle, b: arc },
        Constraint::Collinear { a: l1, b: l2 },
        Constraint::PointOnCurve {
            point: q,
            curve: circle,
        },
        Constraint::Symmetric { a: m1, b: m2, axis },
    ] {
        doc.add_constraint(k).unwrap();
    }
    let report = solved(&mut doc);
    assert!(report.converged, "{report:?}");
    let p = |id| doc.point(id).unwrap();
    assert!((p(c1) - p(c2)).length() < 1e-9);
    assert!((radius_of(&doc, circle).unwrap() - radius_of(&doc, arc).unwrap()).abs() < 1e-9);
    let d1 = p(b1) - p(a1);
    assert!(d1.perp_dot(p(a2) - p(a1)).abs() < 1e-9 * d1.length());
    assert!(d1.perp_dot(p(b2) - p(a1)).abs() < 1e-9 * d1.length());
    assert!((p(q).distance(p(c1)) - radius_of(&doc, circle).unwrap()).abs() < 1e-9);
    let axis_dir = p(ax1) - p(ax0);
    assert!((p(m2) - p(m1)).dot(axis_dir).abs() < 1e-8 * axis_dir.length());
    assert!(axis_dir.perp_dot((p(m1) + p(m2)) * 0.5 - p(ax0)).abs() < 1e-8 * axis_dir.length());

    // Concentric on an already-coradial pair adds nothing new.
    let again = doc
        .add_constraint(Constraint::Concentric { a: circle, b: arc })
        .unwrap();
    assert!(solved(&mut doc).redundant.contains(&again));
    // A curve's own points can't be put on it.
    assert!(
        doc.add_constraint(Constraint::PointOnCurve {
            point: a1,
            curve: l1
        })
        .is_err()
    );
}

#[test]
fn arcs_tangent_at_a_shared_end_point() {
    // Two arcs meeting at t, slightly kinked; tangency makes the centres and t collinear.
    let mut doc = Document::new();
    let c1 = doc.add_point(v(0.0, 0.0));
    let s1 = doc.add_point(v(5.0, 0.0));
    let t = doc.add_point(v(0.0, 5.0));
    let a1 = doc.add_arc(c1, s1, t).unwrap();
    let c2 = doc.add_point(v(0.5, 8.0));
    let e2 = doc.add_point(v(-3.0, 8.0));
    let a2 = doc.add_arc(c2, t, e2).unwrap();
    doc.add_constraint(Constraint::Tangent { a: a1, b: a2 })
        .unwrap();
    assert!(solved(&mut doc).converged);
    let p = |id| doc.point(id).unwrap();
    let (u, w) = (p(c1) - p(t), p(c2) - p(t));
    assert!(u.perp_dot(w).abs() < 1e-9 * u.length() * w.length());
}

#[test]
fn free_directions_follow_the_null_space() {
    let mut doc = Document::new();
    let lone = doc.add_point(v(50.0, 50.0));
    // A horizontal line with a fixed start: the end slides along X.
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 0.0));
    let line = doc.add_line(a, b).unwrap();
    doc.add_constraint(Constraint::Horizontal { line }).unwrap();
    doc.add_constraint(Constraint::Fix {
        point: a,
        at: v(0.0, 0.0),
    })
    .unwrap();
    // A fixed-length arm from a fixed pivot: the tip moves on a circle (tangent direction).
    let pivot = doc.add_point(v(0.0, 20.0));
    let tip = doc.add_point(v(3.0, 24.0));
    doc.add_line(pivot, tip).unwrap();
    doc.add_constraint(Constraint::Fix {
        point: pivot,
        at: v(0.0, 20.0),
    })
    .unwrap();
    doc.add_constraint(Constraint::Distance {
        a: pivot,
        b: tip,
        value: 5.0,
    })
    .unwrap();
    let free = free_directions(&doc);
    assert_eq!(free.get(&lone), Some(&Freedom::Plane));
    assert_eq!(free.get(&a), None, "fixed");
    assert_eq!(free.get(&pivot), None, "fixed");
    let Some(Freedom::Line(d)) = free.get(&b) else {
        panic!("end should slide: {:?}", free.get(&b))
    };
    assert!((d.x.abs() - 1.0).abs() < 1e-9 && d.y.abs() < 1e-9, "{d}");
    let Some(Freedom::Line(t)) = free.get(&tip) else {
        panic!("tip should swing: {:?}", free.get(&tip))
    };
    // Perpendicular to the arm (3, 4).
    assert!(t.dot(v(3.0, 4.0)).abs() < 1e-9, "{t}");
    // Fully define the slider: nothing left for it.
    doc.add_constraint(Constraint::Distance { a, b, value: 10.0 })
        .unwrap();
    assert_eq!(free_directions(&doc).get(&b), None);
}

#[test]
fn the_example_plate_is_fully_defined() {
    let mut doc = drift_core::demo::mounting_plate();
    let before = doc.clone();
    let report = solve(
        &mut doc,
        &SolveOptions {
            analyze: true,
            ..Default::default()
        },
    );
    assert!(report.converged, "conflicts: {:?}", report.conflicts);
    assert!(
        report.redundant.is_empty(),
        "redundant: {:?}",
        report.redundant
    );
    assert_eq!(report.dependent_equations, 0);
    assert_eq!(report.dof, Some(0));
    let loose: Vec<_> = report
        .definitions
        .iter()
        .filter(|(_, d)| **d != Definition::Full)
        .collect();
    assert!(loose.is_empty(), "{loose:?}");
    // It was drawn exactly as constrained: solving moves nothing.
    for (id, e) in before.iter() {
        if let drift_core::EntityKind::Point(p) = e.kind {
            assert!(doc.point(id).unwrap().distance(p) < 1e-9, "{id} moved");
        }
    }
}

#[test]
fn ellipses_have_five_freedoms_and_keep_their_shape() {
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let m = doc.add_point(v(20.0, 0.0));
    // Off the perpendicular: moved onto it when added.
    let n = doc.add_point(v(1.0, 10.0));
    let e = doc.add_ellipse(c, m, n).unwrap();
    assert!(doc.point(n).unwrap().distance(v(0.0, 10.0_f64.hypot(1.0))) < 1e-9);
    assert_eq!(analyze(&doc).dof, Some(5));
    // A point held on it.
    let p = doc.add_point(v(10.0, 9.0));
    doc.add_constraint(Constraint::PointOnCurve { point: p, curve: e })
        .unwrap();
    let report = solved(&mut doc);
    assert!(report.converged);
    assert_eq!(report.dof, Some(6));
    // On the ellipse as it is now (the solve may move it too).
    let [pc, pm, pn, q] = [c, m, n, p].map(|id| doc.point(id).unwrap());
    let (a, b) = (pm.distance(pc), pn.distance(pc));
    let u = (pm - pc).normalize();
    let d = q - pc;
    assert!(((d.dot(u) / a).powi(2) + (d.dot(u.perp()) / b).powi(2) - 1.0).abs() < 1e-9);

    // An elliptical arc: seven (its ends are free along it).
    let mut doc = Document::new();
    let [c, m, n, s, t] = [
        v(0.0, 0.0),
        v(30.0, 0.0),
        v(0.0, 10.0),
        v(30.0, 1.0),
        v(-1.0, 10.0),
    ]
    .map(|q| doc.add_point(q));
    doc.add_ellipse_arc(c, m, n, s, t).unwrap();
    assert_eq!(analyze(&doc).dof, Some(7));
    // Turning the major axis drags the minor one round with it.
    doc.set_point(m, v(0.0, 30.0)).unwrap();
    let report = solve(
        &mut doc,
        &SolveOptions {
            analyze: true,
            dragged: vec![Param::X(m), Param::Y(m)],
            ..Default::default()
        },
    );
    assert!(report.converged);
    let (pc, pm, pn) = (
        doc.point(c).unwrap(),
        doc.point(m).unwrap(),
        doc.point(n).unwrap(),
    );
    assert!((pm - pc).dot(pn - pc).abs() < 1e-6);
}

#[test]
fn a_dimension_from_a_fixed_point_reaches_far() {
    // A 1219 × 2726 mm panel fixed at a corner, its width dimensioned to 1500: the free
    // corner is the only unknown, and must still move 280 mm in a few iterations (it used
    // to crawl a millimetre per iteration and give up).
    let mut doc = Document::new();
    let p = [
        v(0.0, 0.0),
        v(1219.2, 0.0),
        v(1219.2, 2725.7),
        v(0.0, 2725.7),
    ]
    .map(|c| doc.add_point(c));
    for i in 0..4 {
        doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
    }
    doc.add_constraint(Constraint::Fix {
        point: p[0],
        at: v(0.0, 0.0),
    })
    .unwrap();
    doc.add_constraint(Constraint::HorizontalDistance {
        a: p[0],
        b: p[1],
        value: 1500.0,
    })
    .unwrap();
    let report = solved(&mut doc);
    assert!(
        report.converged && report.conflicts.is_empty(),
        "{report:?}"
    );
    assert!(report.iterations < 10, "{}", report.iterations);
    assert!((doc.point(p[1]).unwrap().x - 1500.0).abs() < 1e-6);
}

/// #94: the smallest case found where a drag's result depended on where the sketch was. An
/// arc and a line from its centre; moving the line lands the arc's ends in the same place
/// relative to the sketch wherever the sketch is.
fn move_line_from_arc_centre(shift: DVec2) -> Vec<DVec2> {
    let mut doc = Document::new();
    let c = doc.add_point(shift + v(8.0, 0.0));
    let s = doc.add_point(shift + v(2.34, 5.66));
    let e = doc.add_point(shift + v(0.0, 0.0));
    let arc = doc.add_arc(c, s, e).unwrap();
    let p = doc.add_point(shift + v(8.0, 0.0));
    let q = doc.add_point(shift + v(18.0, 0.0));
    let line = doc.add_line(p, q).unwrap();
    doc.add_constraint(Constraint::Coincident { a: p, b: c })
        .unwrap();
    let delta = v(-3.51, -3.68);
    doc.translate([line], delta);
    let report = solve(
        &mut doc,
        &SolveOptions {
            dragged: [Param::point(p), Param::point(q)].concat(),
            radii: HashMap::from([(arc, 8.0)]),
            lengths: HashMap::from([(line, 10.0)]),
            ..Default::default()
        },
    );
    assert!(report.converged, "{report:?}");
    [c, s, e, p, q]
        .iter()
        .map(|id| doc.point(*id).unwrap() - shift)
        .collect()
}

#[test]
fn drag_result_does_not_depend_on_where_the_sketch_is() {
    let here = move_line_from_arc_centre(DVec2::ZERO);
    for shift in [v(400.0, 0.0), v(-250.0, 370.0), v(0.0, 1000.0)] {
        let there = move_line_from_arc_centre(shift);
        for (i, (a, b)) in here.iter().zip(&there).enumerate() {
            assert!(
                a.distance(*b) < 1e-9,
                "shift {shift}: point {i} landed at {a} here and {b} there"
            );
        }
    }
}

/// #97: a tangency touching at an end the two curves don't share. The general
/// "distance = radius" form has a double root there (the end's own row has the same gradient,
/// or the line pivots about its end), so a solve only crawls to about √(tolerance·r): a move
/// landed 4e-5 off. Saying the tangency is at that end lands it exactly, in a few iterations.
///
/// The issue's shape: arcs R1 (centre c1) and R2 (centre c2, a half circle from s2 to c1), the
/// line L from c2 to s2 (so its extension passes through c1), and arc R3 ending at c1 with L
/// tangent to it — at c1, which L doesn't share. R2 is moved in one step.
#[test]
fn tangent_touching_at_an_arc_end_on_the_line_lands_exactly() {
    let d = |deg: f64| DVec2::from_angle(deg.to_radians());
    let mut doc = Document::new();
    let c1 = doc.add_point(v(8.0, 0.0));
    let s1 = doc.add_point(v(8.0, 0.0) + d(135.0) * 8.0);
    let p0 = doc.add_point(v(0.0, 0.0));
    let r1 = doc.add_arc(c1, s1, p0).unwrap();
    let c2 = doc.add_point(v(8.0, 0.0) + d(120.0) * 8.0);
    let s2 = doc.add_point(v(8.0, 0.0) + d(120.0) * 16.0);
    let r2 = doc.add_arc(c2, s2, c1).unwrap();
    let l = doc.add_line(c2, s2).unwrap();
    let c3 = doc.add_point(v(8.0, 0.0) + d(210.0) * 12.0);
    let s3 = doc.add_point(v(8.0, 0.0) + d(210.0) * 12.0 + d(-15.0) * 12.0);
    let r3 = doc.add_arc(c3, s3, c1).unwrap();
    doc.add_constraint(Constraint::Tangent { a: l, b: r3 })
        .unwrap();
    assert!(solved(&mut doc).converged);
    let (report, off) = move_curves(&mut doc, &[r2], v(0.0, -5.54), &[r1, r2, r3], &[l]);
    assert!(report.converged, "{report:?}");
    assert!(off < 1e-9, "the dragged points landed {off} off");
    assert!(report.iterations < 20, "{report:?}");
    assert!(unmet(&doc, 1e-9).is_empty());
    // The tangency is still at R3's end: c1 is on L, and the radius there is square to it.
    let (pc1, pc2, ps2, pc3) = (
        doc.point(c1).unwrap(),
        doc.point(c2).unwrap(),
        doc.point(s2).unwrap(),
        doc.point(c3).unwrap(),
    );
    let u = (ps2 - pc2).normalize();
    assert!(u.perp_dot(pc1 - pc2).abs() < 1e-9);
    assert!(u.dot(pc3 - pc1).abs() < 1e-9);
}

/// #97: the same double root where a line stands on an arc's circle (its end on the circle)
/// and is tangent to it. Moving the arc lands exactly.
#[test]
fn tangent_line_standing_on_a_circle_lands_exactly() {
    let d = |deg: f64| DVec2::from_angle(deg.to_radians());
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(d(200.0) * 8.0);
    let e = doc.add_point(d(330.0) * 8.0);
    let arc = doc.add_arc(c, s, e).unwrap();
    let foot = doc.add_point(d(60.0) * 8.0);
    let far = doc.add_point(d(60.0) * 8.0 + d(150.0) * 10.0);
    let line = doc.add_line(foot, far).unwrap();
    doc.add_constraint(Constraint::PointOnCurve {
        point: foot,
        curve: arc,
    })
    .unwrap();
    doc.add_constraint(Constraint::Tangent { a: line, b: arc })
        .unwrap();
    assert!(solved(&mut doc).converged);
    let (report, off) = move_curves(&mut doc, &[arc], v(7.3, -4.1), &[arc], &[line]);
    assert!(report.converged, "{report:?}");
    assert!(off < 1e-9, "the dragged points landed {off} off");
    assert!(report.iterations < 20, "{report:?}");
    assert!(unmet(&doc, 1e-9).is_empty());
}

/// Move `curves` by `delta` in one step the way `Editor::drag` does: their points dragged
/// to where the move puts them, arcs keeping their radii and lines their lengths where they
/// can. Returns the report and how far the dragged points landed from their targets.
fn move_curves(
    doc: &mut Document,
    curves: &[EntityId],
    delta: DVec2,
    arcs: &[EntityId],
    lines: &[EntityId],
) -> (SolveReport, f64) {
    let radii: HashMap<EntityId, f64> = arcs
        .iter()
        .map(|&a| (a, crate::radius_of(doc, a).unwrap()))
        .collect();
    let lengths: HashMap<EntityId, f64> = lines
        .iter()
        .map(|&l| {
            let EntityKind::Line { a, b } = doc.get(l).unwrap().kind else {
                panic!()
            };
            (l, doc.point(a).unwrap().distance(doc.point(b).unwrap()))
        })
        .collect();
    let dragged: Vec<EntityId> = doc
        .defining_points(curves.iter().copied())
        .into_iter()
        .collect();
    let targets: Vec<(EntityId, DVec2)> = dragged
        .iter()
        .map(|&id| (id, doc.point(id).unwrap() + delta))
        .collect();
    doc.translate(curves.iter().copied(), delta);
    for &(id, at) in &targets {
        doc.set_point(id, at).unwrap();
    }
    let report = solve(
        doc,
        &SolveOptions {
            dragged: dragged.iter().flat_map(|&p| Param::point(p)).collect(),
            radii,
            lengths,
            ..Default::default()
        },
    );
    let off = targets
        .iter()
        .map(|&(id, at)| doc.point(id).unwrap().distance(at))
        .fold(0.0, f64::max);
    (report, off)
}

/// A line of zero length (its ends dragged together) under each relation and dimension that
/// divides by a length: the solve may fail, but nothing that isn't a number is ever written
/// back (#215).
#[test]
fn zero_length_line_never_writes_non_finite() {
    let all_finite = |doc: &Document| doc.first_non_finite().is_none();
    // A relation on the zero-length line, given it, another line, a circle and a point.
    type Relation = fn(EntityId, EntityId, EntityId, EntityId) -> Constraint;
    let relations: Vec<Relation> = vec![
        |z, _, _, _| Constraint::Horizontal { line: z },
        |z, _, _, _| Constraint::Vertical { line: z },
        |z, o, _, _| Constraint::Parallel { a: z, b: o },
        |z, o, _, _| Constraint::Perpendicular { a: z, b: o },
        |z, o, _, _| Constraint::Equal { a: z, b: o },
        |z, o, _, _| Constraint::Collinear { a: z, b: o },
        |z, _, c, _| Constraint::Tangent { a: z, b: c },
        |z, _, _, x| Constraint::PointOnCurve { point: x, curve: z },
        |z, _, _, x| Constraint::Midpoint { point: x, line: z },
        |z, _, _, x| Constraint::PointLineDistance {
            point: x,
            line: z,
            value: 3.0,
        },
        |z, o, _, _| Constraint::Angle {
            a: z,
            b: o,
            degrees: 30.0,
        },
        |z, o, _, _| Constraint::Angle {
            a: o,
            b: z,
            degrees: 60.0,
        },
        |z, _, _, _| Constraint::Distance {
            a: z,
            b: z,
            value: 5.0,
        },
    ];
    for (i, relation) in relations.iter().enumerate() {
        let mut doc = Document::new();
        let (p, q) = (doc.add_point(v(0.0, 0.0)), doc.add_point(v(10.0, 1.0)));
        let z = doc.add_line(p, q).unwrap();
        let (r, s) = (doc.add_point(v(0.0, 20.0)), doc.add_point(v(12.0, 25.0)));
        let o = doc.add_line(r, s).unwrap();
        let center = doc.add_point(v(30.0, 0.0));
        let c = doc.add_circle(center, 4.0).unwrap();
        let x = doc.add_point(v(5.0, 7.0));
        // Symmetric about the line as well, for the axis equations.
        let (m, n) = (doc.add_point(v(-3.0, 4.0)), doc.add_point(v(4.0, -3.0)));
        let _ = doc.add_constraint(Constraint::Symmetric {
            a: m,
            b: n,
            axis: z,
        });
        if doc.add_constraint(relation(z, o, c, x)).is_err() {
            continue;
        }
        // Its ends together, as a drag can leave them.
        doc.set_point(q, v(0.0, 0.0)).unwrap();
        solved(&mut doc);
        assert!(
            all_finite(&doc),
            "relation {i}: a solve wrote a non-finite value"
        );
        // And dragged there, with the relations kept.
        doc.set_point(q, v(10.0, 1.0)).unwrap();
        solved(&mut doc);
        doc.set_point(q, v(0.0, 0.0)).unwrap();
        solve(
            &mut doc,
            &SolveOptions {
                dragged: Param::point(q).to_vec(),
                ..Default::default()
            },
        );
        assert!(
            all_finite(&doc),
            "relation {i}: a drag wrote a non-finite value"
        );
    }
}

#[test]
fn set_point_refuses_non_finite() {
    let mut doc = Document::new();
    let p = doc.add_point(v(1.0, 2.0));
    assert!(doc.set_point(p, v(f64::NAN, 0.0)).is_err());
    assert!(doc.set_point(p, v(0.0, f64::INFINITY)).is_err());
    assert_eq!(doc.point(p).unwrap(), v(1.0, 2.0));
}

/// #198: a tangency touching at an end as a drag begins (a line's end lying on an arc's end,
/// not joined) keeps that form through a step whose translated geometry no longer touches
/// there: lowered by the touching ends found at the start, the solve keeps the arc's end on
/// the line, where it is the tangent point. Lowered by the step's own geometry it took the
/// general "distance = radius" form, which only pins the touch point to about √(tol·r), and
/// left the arc ending off its tangency (which a look at the result then read as unmet).
#[test]
fn a_tangency_keeps_the_touching_end_it_began_with() {
    let mut doc = Document::new();
    let o = doc.add_point(v(0.0, 0.0));
    let end = doc.add_point(v(-15.0, 0.0));
    let line = doc.add_line(o, end).unwrap();
    let c = doc.add_point(v(-15.0, 8.0));
    let s = doc.add_point(v(-15.0, 8.0) + DVec2::from_angle(225f64.to_radians()) * 8.0);
    let arc_end = doc.add_point(v(-15.0, 0.0));
    let arc = doc.add_arc(c, s, arc_end).unwrap();
    doc.add_constraint(Constraint::Tangent { a: line, b: arc })
        .unwrap();
    assert!(unmet(&doc, 1e-9).is_empty());
    let touching = crate::touching_ends(&doc, &crate::relations(&doc, None));
    // The step: the line moved up off the arc's end, then solved with the line held.
    for p in [o, end] {
        let at = doc.point(p).unwrap();
        doc.set_point(p, at + v(0.0, 1.0)).unwrap();
    }
    let report = solve(
        &mut doc,
        &SolveOptions {
            dragged: [o, end].iter().flat_map(|&p| Param::point(p)).collect(),
            touching: Some(std::rc::Rc::new(touching)),
            ..Default::default()
        },
    );
    assert!(report.converged, "{report:?}");
    let off = doc.point(arc_end).unwrap().y - 1.0;
    assert!(off.abs() < 1e-9, "the arc's end is {off} off the line");
    assert!(unmet(&doc, 1e-8).is_empty(), "{:?}", unmet(&doc, 1e-8));
}

/// An arc whose end is held off the circle through its start fails on a row no relation
/// owns: the report names no conflict, and says so (#281). A conflict between relations
/// names them, and doesn't.
#[test]
fn a_failure_no_relation_owns_is_reported() {
    let mut doc = Document::new();
    doc.begin();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(v(10.0, 0.0));
    let e = doc.add_point(v(0.0, 10.0));
    doc.add_arc(c, s, e).unwrap();
    doc.set_point(e, v(0.0, 12.0)).unwrap();
    for (point, at) in [(c, v(0.0, 0.0)), (s, v(10.0, 0.0)), (e, v(0.0, 12.0))] {
        doc.add_constraint(Constraint::Fix { point, at }).unwrap();
    }
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(!report.converged);
    assert!(report.conflicts.is_empty());
    assert!(report.unsourced_failure);

    let mut doc = Document::new();
    doc.begin();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 0.0));
    let line = doc.add_line(a, b).unwrap();
    for c in [
        Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        },
        Constraint::Horizontal { line },
        Constraint::Vertical { line },
        Constraint::Distance { a, b, value: 10.0 },
    ] {
        doc.add_constraint(c).unwrap();
    }
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(!report.converged);
    assert!(!report.conflicts.is_empty());
    assert!(!report.unsourced_failure);
}

/// `points` dragged by `delta` in one step, as a drag of the curves they are on does, with
/// the `arcs` keeping their radii, with the fold steps or not. Returns the report and how
/// far the dragged points landed from their targets.
fn drag_points(
    doc: &mut Document,
    points: &[EntityId],
    delta: DVec2,
    arcs: &[EntityId],
    fold: bool,
) -> (SolveReport, f64) {
    let radii: HashMap<EntityId, f64> = arcs
        .iter()
        .map(|&a| (a, crate::radius_of(doc, a).unwrap()))
        .collect();
    let targets: Vec<(EntityId, DVec2)> = points
        .iter()
        .map(|&id| (id, doc.point(id).unwrap() + delta))
        .collect();
    for &(id, at) in &targets {
        doc.set_point(id, at).unwrap();
    }
    let report = solve(
        doc,
        &SolveOptions {
            dragged: points.iter().flat_map(|&p| Param::point(p)).collect(),
            radii,
            fold,
            ..Default::default()
        },
    );
    let off = targets
        .iter()
        .map(|&(id, at)| doc.point(id).unwrap().distance(at))
        .fold(0.0, f64::max);
    (report, off)
}

/// #282: two points held on an arc a diameter apart, moved together. The arc's kept radius
/// can be met only with its centre exactly midway between them, its least: a fold, where the
/// minimum-change step closes about the cube of what is left and the solve runs to its cap.
/// The fold steps meet it: the circle goes with the points, its radius kept.
#[test]
fn a_kept_radius_at_a_fold_is_met_with_the_fold_steps() {
    let at = |deg: f64| v(8.0, 0.0) + DVec2::from_angle(deg.to_radians()) * 8.0;
    let build = || {
        let mut doc = Document::new();
        let e = doc.add_point(v(0.0, 0.0));
        let c = doc.add_point(v(8.0, 0.0));
        let s = doc.add_point(at(135.0));
        let arc = doc.add_arc(c, s, e).unwrap();
        let held = [at(135.0), at(315.0)].map(|p| {
            let point = doc.add_point(p);
            doc.add_constraint(Constraint::PointOnCurve { point, curve: arc })
                .unwrap();
            point
        });
        (doc, arc, c, held)
    };
    let (mut doc, arc, _, held) = build();
    let (report, _) = drag_points(&mut doc, &held, v(0.0, -18.0), &[arc], false);
    assert!(
        !report.converged,
        "it finished without the fold steps: {report:?}"
    );
    let (mut doc, arc, c, held) = build();
    let (report, off) = drag_points(&mut doc, &held, v(0.0, -18.0), &[arc], true);
    assert!(report.converged, "{report:?}");
    assert!(off == 0.0, "the dragged points landed {off} off");
    let r = crate::radius_of(&doc, arc).unwrap();
    assert!((r - 8.0).abs() <= 8.0 * FOLD_MET, "radius {r}");
    // (Midway between the points to what a radius met that closely allows at its least:
    // √(2·r·its tolerance).)
    let centre = doc.point(c).unwrap().distance(v(8.0, -18.0));
    assert!(
        centre <= (2.0 * 8.0 * 8.0 * FOLD_MET).sqrt(),
        "centre {centre} off"
    );
    assert!(unmet(&doc, 1e-9).is_empty());
}

/// #283 (g14's shape): a point held on a line at its tangent end, moved with the arc's centre
/// so that it stays exactly a radius from it. The tangent line through it then touches only
/// there, and the line's end must sit on it: a double root, and the move landed off. With the
/// fold steps it lands.
#[test]
fn a_move_held_at_a_tangent_end_lands_with_the_fold_steps() {
    let d = |deg: f64| DVec2::from_angle(deg.to_radians());
    let build = || {
        let mut doc = Document::new();
        let c = doc.add_point(v(0.0, 0.0));
        let s = doc.add_point(d(200.0) * 8.0);
        let e = doc.add_point(d(330.0) * 8.0);
        let arc = doc.add_arc(c, s, e).unwrap();
        let far = doc.add_point(d(330.0) * 8.0 + d(60.0) * 10.0);
        let line = doc.add_line(e, far).unwrap();
        doc.add_constraint(Constraint::Tangent { a: line, b: arc })
            .unwrap();
        let p = doc.add_point(d(330.0) * 8.0);
        doc.add_constraint(Constraint::PointOnCurve {
            point: p,
            curve: line,
        })
        .unwrap();
        assert!(solved(&mut doc).converged);
        (doc, arc, [c, p])
    };
    let delta = v(0.0, 12.725262082906049);
    let (mut doc, arc, dragged) = build();
    let (report, off) = drag_points(&mut doc, &dragged, delta, &[arc], false);
    assert!(
        !(report.converged && off == 0.0),
        "it landed without the fold steps: {report:?}"
    );
    let (mut doc, arc, dragged) = build();
    let (report, off) = drag_points(&mut doc, &dragged, delta, &[arc], true);
    assert!(report.converged, "{report:?}");
    assert!(off == 0.0, "the dragged points landed {off} off");
    assert!(unmet(&doc, 1e-9).is_empty());
}

/// The rows' fold alone (move_lands 17's case, nothing kept): a line's midpoint dragged with
/// an arc ending at the line's end, another dragged line kept as long as that one, and a
/// point held on both lines where they meet. Moved, the solve converges with the dragged
/// heavy, but the landing pass (the dragged pinned) has a double root, halves its way in
/// until the minimum change outweighs what is left, and is refused 0.002 off. With the fold
/// steps the rows' minimum change comes down as they close, and it lands.
#[test]
fn a_landing_at_a_double_root_of_the_rows_lands_with_the_fold_steps() {
    let build = || {
        let mut doc = Document::new();
        let p = [
            v(0.0, 0.0),
            v(15.000000000000004, -25.980762113533157),
            v(-7.4999999999999964, 12.99038105676658),
            v(8.0, 0.0),
            v(2.3431457505076203, 5.656854249492381),
            v(-3.7499999999999982, 6.49519052838329),
            v(-17.892135623730947, 20.63732615211424),
            v(3.750000000000001, -6.495190528383289),
            v(16.74038105676658, 1.00480947161671),
            v(8.0, 0.0),
            v(2.3431457505076203, 5.656854249492381),
            v(16.74038105676658, -18.99519052838329),
        ]
        .map(|q| doc.add_point(q));
        let l0 = doc.add_line(p[0], p[1]).unwrap();
        let l1 = doc.add_line(p[0], p[2]).unwrap();
        doc.add_arc(p[3], p[4], p[0]).unwrap();
        doc.add_line(p[5], p[6]).unwrap();
        let l4 = doc.add_line(p[7], p[8]).unwrap();
        doc.add_arc(p[9], p[10], p[0]).unwrap();
        doc.add_line(p[8], p[11]).unwrap();
        for c in [
            Constraint::Midpoint {
                point: p[5],
                line: l1,
            },
            Constraint::PointOnCurve {
                point: p[7],
                curve: l0,
            },
            Constraint::Equal { a: l1, b: l4 },
            Constraint::PointOnCurve {
                point: p[7],
                curve: l1,
            },
        ] {
            doc.add_constraint(c).unwrap();
        }
        assert!(solved(&mut doc).converged);
        let dragged = [5, 6, 9, 10, 0, 8, 11].map(|i| p[i]);
        (doc, dragged)
    };
    let delta = v(0.0, -14.516466897443614);
    let (mut doc, dragged) = build();
    let (report, off) = drag_points(&mut doc, &dragged, delta, &[], false);
    assert!(
        report.converged && off > 1e-4,
        "{report:?}, landed {off} off"
    );
    let (mut doc, dragged) = build();
    let (report, off) = drag_points(&mut doc, &dragged, delta, &[], true);
    assert!(report.converged, "{report:?}");
    assert!(off == 0.0, "the dragged points landed {off} off");
    assert!(unmet(&doc, 1e-9).is_empty());
}

/// The user's rule for a drag the kept radii can't follow ("stop, as now"): e13's two arcs
/// sharing an end, the first's centre fixed, the second's dragged until the centres are
/// farther apart than the radii add up to. Meeting the relations takes both radii growing;
/// with the fold steps the solve isn't finished there (a kept radius isn't met), and it
/// leaves everything as it found it.
#[test]
fn a_fold_solve_that_would_grow_a_radius_is_unfinished_and_changes_nothing() {
    let mut doc = Document::new();
    let p = [
        v(0.0, 0.0),
        v(8.0, 0.0),
        v(2.3431457505076203, -5.65685424949238),
        v(-9.656854249492381, -5.65685424949238),
        v(-1.1715728752538102, -14.142135623730951),
    ]
    .map(|q| doc.add_point(q));
    let first = doc.add_arc(p[1], p[0], p[2]).unwrap();
    let second = doc.add_arc(p[3], p[4], p[2]).unwrap();
    doc.add_constraint(Constraint::Fix {
        point: p[1],
        at: v(8.0, 0.0),
    })
    .unwrap();
    doc.set_point(
        p[3],
        v(-9.656854249492381, -5.65685424949238 - 8.147331698170428),
    )
    .unwrap();
    let found: Vec<DVec2> = p.iter().map(|&q| doc.point(q).unwrap()).collect();
    let report = solve(
        &mut doc,
        &SolveOptions {
            dragged: Param::point(p[3]).to_vec(),
            radii: HashMap::from([(first, 8.0), (second, 12.0)]),
            fold: true,
            ..Default::default()
        },
    );
    assert!(!report.converged, "{report:?}");
    // Nothing written, so nothing reported broken or exposed (#342).
    assert!(report.unwritten, "{report:?}");
    assert!(
        report.broken.is_empty() && report.exposed.is_empty(),
        "{report:?}"
    );
    let now: Vec<DVec2> = p.iter().map(|&q| doc.point(q).unwrap()).collect();
    assert_eq!(now, found);
}

/// #277: a line held vertical and square to a level one, a point held on it, and the level
/// line and another moved up a little (`small_drag_is_continuous_heavy` seed 7). Once the
/// dragged are pinned to land, only the vertical's free end is left to move, and it slides
/// along the line, which neither the angle nor the point on it feels: their gradients there
/// are round-off. Scaled up to a unit gradient, the angle's round-off residual flung the end
/// 35.7 away (as far as the drawing is across). It stays where it was.
#[test]
fn a_row_only_round_off_ties_to_what_is_left_moves_nothing() {
    let mut doc = Document::new();
    let p = [
        v(0.0, 0.0),
        v(-10.0, 0.0),
        v(0.0, 10.0),
        v(0.0, 2.5),
        v(19.318516525781366, 7.676380902050415),
    ]
    .map(|q| doc.add_point(q));
    let level = doc.add_line(p[0], p[1]).unwrap();
    let upright = doc.add_line(p[0], p[2]).unwrap();
    doc.add_line(p[3], p[4]).unwrap();
    for c in [
        Constraint::PointOnCurve {
            point: p[3],
            curve: upright,
        },
        Constraint::Vertical { line: upright },
        Constraint::Perpendicular {
            a: level,
            b: upright,
        },
    ] {
        doc.add_constraint(c).unwrap();
    }
    let free_end = doc.point(p[2]).unwrap();
    let (report, off) = drag_points(
        &mut doc,
        &[p[0], p[1], p[3], p[4]],
        v(0.0, 0.2975643529800304),
        &[],
        false,
    );
    // (Landed to the bar: an ulp off its target needs no landing pass.)
    assert!(
        report.converged && off <= 1e-12,
        "{report:?}, landed {off} off"
    );
    assert_eq!(doc.point(p[2]).unwrap(), free_end);
}

/// #277: a redundant right angle between a level line and an upright one whose ends are
/// joined (`drag_is_invariant_under_similarity_heavy` seed 5): with the level and the upright
/// substituted, the angle's gradient is round-off on everything, and it is the only row.
/// Its round-off residual swung the upright's free end the drawing's width back and forth
/// until the solve's patience ran out, so where it ended depended on the drawing's size and
/// place. Moving the level line leaves the free end where it was, at any size.
#[test]
fn a_redundant_angle_with_nothing_left_to_move_moves_nothing() {
    for (scale, shift) in [(1.0, 0.0), (0.25, -133.47216040592113)] {
        let at = |x: f64, y: f64| v(x * scale, y * scale + shift);
        let mut doc = Document::new();
        let p =
            [at(0.0, 0.0), at(0.0, 0.0), at(15.0, 0.0), at(0.0, -10.0)].map(|q| doc.add_point(q));
        let level = doc.add_line(p[1], p[2]).unwrap();
        let upright = doc.add_line(p[0], p[3]).unwrap();
        for c in [
            Constraint::Coincident { a: p[1], b: p[0] },
            Constraint::Horizontal { line: level },
            Constraint::Vertical { line: upright },
            Constraint::Perpendicular {
                a: level,
                b: upright,
            },
        ] {
            doc.add_constraint(c).unwrap();
        }
        let free_end = doc.point(p[3]).unwrap();
        let (report, off) = drag_points(
            &mut doc,
            &[p[1], p[2]],
            v(0.0, -6.606701489569068 * scale),
            &[],
            false,
        );
        assert!(
            report.converged && off == 0.0,
            "{report:?}, landed {off} off"
        );
        assert_eq!(doc.point(p[3]).unwrap(), free_end, "at {scale}×");
    }
}

/// #277: a row with no gradient left that doesn't hold keeps its scale (only one that holds
/// is left out of the step), and is still a conflict: two level lines held square to each
/// other. With both substituted level, nothing left to move turns either, so no step can
/// mend the angle; the solve doesn't converge, names it, and leaves the drawing as it was.
#[test]
fn a_row_with_no_gradient_left_that_cannot_hold_is_still_a_conflict() {
    let mut doc = Document::new();
    let p = [v(0.0, 0.0), v(10.0, 0.0), v(0.0, 5.0), v(10.0, 5.0)].map(|q| doc.add_point(q));
    let a = doc.add_line(p[0], p[1]).unwrap();
    let b = doc.add_line(p[2], p[3]).unwrap();
    doc.add_constraint(Constraint::Horizontal { line: a })
        .unwrap();
    doc.add_constraint(Constraint::Horizontal { line: b })
        .unwrap();
    let square = doc
        .add_constraint(Constraint::Perpendicular { a, b })
        .unwrap();
    let found: Vec<DVec2> = p.iter().map(|&q| doc.point(q).unwrap()).collect();
    let report = solved(&mut doc);
    assert!(!report.converged, "{report:?}");
    assert!(report.conflicts.contains(&square), "{report:?}");
    let now: Vec<DVec2> = p.iter().map(|&q| doc.point(q).unwrap()).collect();
    assert_eq!(now, found);
}

/// An arc about `c` of radius `r` from `from` to `to` (degrees, counter-clockwise), with its
/// three points.
fn arc_at(doc: &mut Document, c: DVec2, r: f64, from: f64, to: f64) -> (EntityId, [EntityId; 3]) {
    let at = |deg: f64| c + r * DVec2::new(deg.to_radians().cos(), deg.to_radians().sin());
    let pc = doc.add_point(c);
    let ps = doc.add_point(at(from));
    let pe = doc.add_point(at(to));
    (doc.add_arc(pc, ps, pe).unwrap(), [pc, ps, pe])
}

/// #280: an arc a step closes on itself (its ends together) counts as turned whichever way
/// round-off leaves it. Read as a sweep, a 270° arc whose end lands a hair past its start
/// sweeps ~0 (turned) and one a hair short sweeps ~360°, exactly the quarter turn
/// `SWEEP_JUMP` allows (not turned): the same drag was halved at one size and not another.
#[test]
fn an_arc_closed_on_itself_counts_as_turned_either_side_of_round_off() {
    for side in [1.0, -1.0] {
        let mut doc = Document::new();
        let (arc, [_, _, e]) = arc_at(&mut doc, DVec2::ZERO, 8.0, 0.0, -90.0);
        let before = Orientation::of(&doc, None);
        doc.set_point(e, v(8.0, side * 1e-13)).unwrap();
        assert_eq!(
            before.turned_curves(&doc),
            vec![arc],
            "end on the {side} side of the start"
        );
    }
}

/// #280: an arc a step crushes to a point counts as turned, at any size and anywhere (its
/// sweep, the angle between two vectors of next to no length, says nothing). Crushed evenly
/// towards its centre, its sweep reads unchanged.
#[test]
fn an_arc_crushed_to_a_point_counts_as_turned_at_any_size() {
    for scale in [1.0, 0.01, 100.0] {
        for c in [DVec2::ZERO, v(1e5, 1e5)] {
            let mut doc = Document::new();
            let (arc, [pc, ps, pe]) = arc_at(&mut doc, c, 8.0 * scale, 0.0, 90.0);
            let before = Orientation::of(&doc, None);
            for p in [ps, pe] {
                let at = doc.point(p).unwrap();
                doc.set_point(p, c + (at - c) * 1e-9).unwrap();
            }
            assert_eq!(doc.point(pc).unwrap(), c);
            assert_eq!(
                before.turned_curves(&doc),
                vec![arc],
                "at {scale}× about {c}"
            );
        }
    }
}

/// #280: a small real arc far from the origin is judged by its sweep like any other: a small
/// change is no turn, and turning it inside out is. (Its radius is next to nothing against
/// its coordinates, but not against its own sketch; nothing F1 decides depends on where a
/// drawing is.)
#[test]
fn a_small_arc_far_out_is_judged_by_its_sweep() {
    for far in [1e6, 1e9] {
        let c = v(far, far);
        let mut doc = Document::new();
        let (arc, [_, _, pe]) = arc_at(&mut doc, c, 0.5, 0.0, 90.0);
        let before = Orientation::of(&doc, None);
        let at = |deg: f64| c + 0.5 * DVec2::new(deg.to_radians().cos(), deg.to_radians().sin());
        doc.set_point(pe, at(100.0)).unwrap();
        assert!(before.turned_curves(&doc).is_empty(), "at {far}");
        doc.set_point(pe, at(270.0)).unwrap();
        assert_eq!(before.turned_curves(&doc), vec![arc], "at {far}");
    }
}

/// #280: an arc a drag already crushed (against the radius it had as the drag began) has no
/// way round to keep where the next halved step begins and isn't judged, nor is a tangency to
/// it, at the origin or far from it alike: its sweep and the side its centre is on are
/// round-off, and swing at any touch.
#[test]
fn an_arc_already_crushed_is_not_judged_at_the_origin_or_far_out() {
    for c in [DVec2::ZERO, v(1e5, 1e5)] {
        let mut doc = Document::new();
        let a = doc.add_point(c + v(-20.0, 5.0));
        let b = doc.add_point(c + v(20.0, 5.0));
        let line = doc.add_line(a, b).unwrap();
        let (arc, [pc, ps, pe]) = arc_at(&mut doc, c, 5.0, 0.0, 90.0);
        doc.add_constraint(Constraint::Tangent { a: line, b: arc })
            .unwrap();
        // Crushed to a millionth of a millimetre since the drag began, a sweep of 90° as it lies.
        doc.set_point(ps, c + v(1e-6, 0.0)).unwrap();
        doc.set_point(pe, c + v(0.0, 1e-6)).unwrap();
        let started = HashMap::from([(arc, 5.0)]);
        let before = Orientation::among_kept(&doc, None, &[], &started, &[]);
        let constraints: Vec<_> = doc.constraints().map(|(id, c)| (id, *c)).collect();
        let before_with_tangency = Orientation::among_kept(&doc, None, &constraints, &started, &[]);
        // Its end swings round (a sweep of 270°) and its centre crosses the line.
        doc.set_point(pe, c + v(0.0, -1e-6)).unwrap();
        for p in [pc, ps, pe] {
            let at = doc.point(p).unwrap();
            doc.set_point(p, at + v(0.0, 10.0)).unwrap();
        }
        assert!(before.turned_curves(&doc).is_empty(), "about {c}");
        assert!(
            before_with_tangency.turned_curves(&doc).is_empty(),
            "about {c}, with its tangency"
        );
    }
}

/// A kilometre line, for a part much larger than the arcs beside it.
fn kilometre_line(doc: &mut Document) {
    let a = doc.add_point(v(-5e5, -10.0));
    let b = doc.add_point(v(5e5, -10.0));
    doc.add_line(a, b).unwrap();
}

/// #280: a small real arc in a large part is judged against its own size, not the part's: a
/// 0.3 mm arc beside a kilometre line turned inside out is a turn.
#[test]
fn a_small_arc_in_a_large_part_turned_inside_out_is_a_turn() {
    let mut doc = Document::new();
    kilometre_line(&mut doc);
    let (arc, [_, _, pe]) = arc_at(&mut doc, DVec2::ZERO, 0.3, 0.0, 90.0);
    let before = Orientation::of(&doc, None);
    doc.set_point(pe, v(0.0, -0.3)).unwrap();
    assert_eq!(before.turned_curves(&doc), vec![arc]);
}

/// #280: the same, the other way: a 1 mm arc beside a kilometre line shrunk evenly to 0.4 mm
/// is not crushed.
#[test]
fn a_small_arc_in_a_large_part_shrunk_is_not_crushed() {
    let mut doc = Document::new();
    kilometre_line(&mut doc);
    let (_, [pc, ps, pe]) = arc_at(&mut doc, DVec2::ZERO, 1.0, 0.0, 90.0);
    let before = Orientation::of(&doc, None);
    for p in [ps, pe] {
        let at = doc.point(p).unwrap();
        doc.set_point(p, at * 0.4).unwrap();
    }
    assert_eq!(doc.point(pc).unwrap(), DVec2::ZERO);
    assert!(before.turned_curves(&doc).is_empty());
}

/// #280: a circle a step crushes to a point counts as turned, as a crushed arc does: a
/// tangency to it would read its side off a centre lying on the line. (The drag properties
/// sketch no circles; this is its only guard.)
#[test]
fn a_circle_crushed_to_a_point_counts_as_turned() {
    for c in [DVec2::ZERO, v(1e5, 1e5)] {
        let mut doc = Document::new();
        let a = doc.add_point(c + v(-20.0, 5.0));
        let b = doc.add_point(c + v(20.0, 5.0));
        let line = doc.add_line(a, b).unwrap();
        let center = doc.add_point(c);
        let circle = doc.add_circle(center, 5.0).unwrap();
        doc.add_constraint(Constraint::Tangent { a: line, b: circle })
            .unwrap();
        let before = Orientation::of(&doc, None);
        doc.set_radius(circle, 1e-9).unwrap();
        assert_eq!(before.turned_curves(&doc), vec![circle], "about {c}");
    }
}

/// An arc, its centre fixed, with a radius of 10 and its end held 12 from its centre: the
/// two relations conflict, and the arc's own row (its end on the circle through its start)
/// is in the same failed part. The least-squares compromise shares the misfit with it, so
/// the arc is left broken; the report names the two relations and, since #342, the arc.
/// The same at any size and place: the same entity broken, by the same share of its size.
#[test]
fn a_failed_part_reports_the_geometry_it_leaves_broken() {
    let mut shares = Vec::new();
    for (scale, at) in [
        (1.0, v(0.0, 0.0)),
        (10.0, v(0.0, 0.0)),
        (0.1, v(0.0, 0.0)),
        (1.0, v(1e5, 1e5)),
    ] {
        let mut doc = Document::new();
        let c = doc.add_point(at);
        let s = doc.add_point(at + v(10.0, 0.0) * scale);
        let e = doc.add_point(at + v(0.0, 10.0) * scale);
        let arc = doc.add_arc(c, s, e).unwrap();
        doc.add_constraint(Constraint::Fix { point: c, at })
            .unwrap();
        let radius = doc
            .add_constraint(Constraint::Radius {
                curve: arc,
                value: 10.0 * scale,
            })
            .unwrap();
        let distance = doc
            .add_constraint(Constraint::Distance {
                a: c,
                b: e,
                value: 12.0 * scale,
            })
            .unwrap();
        let report = solve(&mut doc, &SolveOptions::default());
        assert!(!report.converged, "{report:?}");
        assert_eq!(report.conflicts, vec![radius, distance]);
        assert!(!report.unsourced_failure && !report.unwritten);
        assert_eq!(report.exposed, vec![arc], "{scale}× at {at}");
        assert_eq!(report.broken, vec![arc], "{scale}× at {at}");
        let length = |q: EntityId| (doc.point(q).unwrap() - doc.point(c).unwrap()).length();
        shares.push((length(e) - length(s)) / (10.0 * scale));
        // The analysis of what was written says the same.
        let analysis = analyze(&doc);
        assert_eq!(analysis.exposed, vec![arc]);
        assert_eq!(analysis.broken, vec![arc]);
    }
    for share in &shares {
        assert!(
            (share - shares[0]).abs() <= 1e-9 && shares[0] > 0.05,
            "the arc's end off its circle by {shares:?} of its radius"
        );
    }
}

/// Judged row by row: an arc in a failed part whose own row is met is exposed, not broken;
/// an arc in a part that converged is neither. And a failure no relation owns
/// (#281) names the arc as broken.
#[test]
fn only_the_rows_left_off_are_broken() {
    let mut doc = Document::new();
    // A point held 10 from two fixed points 30 apart: a conflict. An arc centred on it, its
    // own ends free: in the same failed part, its row met.
    let a = doc.add_point(v(0.0, 0.0));
    let f = doc.add_point(v(30.0, 0.0));
    let b = doc.add_point(v(12.0, 0.0));
    for c in [
        Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        },
        Constraint::Fix {
            point: f,
            at: v(30.0, 0.0),
        },
        Constraint::Distance { a, b, value: 10.0 },
        Constraint::Distance {
            a: b,
            b: f,
            value: 10.0,
        },
        // (On the line through them: the compromise is reached, not crawled towards.)
        Constraint::FixY { point: b, y: 0.0 },
    ] {
        doc.add_constraint(c).unwrap();
    }
    let (s, e) = (doc.add_point(v(17.0, 0.0)), doc.add_point(v(12.0, 5.0)));
    let joined = doc.add_arc(b, s, e).unwrap();
    // An arc in a part of its own, with a dimension it can meet.
    let c = doc.add_point(v(50.0, 0.0));
    let (s2, e2) = (doc.add_point(v(55.0, 0.0)), doc.add_point(v(50.0, 5.0)));
    let apart = doc.add_arc(c, s2, e2).unwrap();
    doc.add_constraint(Constraint::Radius {
        curve: apart,
        value: 5.0,
    })
    .unwrap();
    // Analysed as it stands, the arc's row is met: exposed, not broken.
    let analysis = analyze(&doc);
    assert!(!analysis.converged, "{analysis:?}");
    assert_eq!(analysis.exposed, vec![joined]);
    assert!(analysis.broken.is_empty(), "{analysis:?}");
    // Solved, it is still exposed, the other arc neither. (How far the arc is left off is
    // where the compromise's crawl stopped: not asserted.)
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(
        !report.converged && !report.conflicts.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.exposed, vec![joined]);
    assert!(!report.broken.contains(&apart));

    // An arc's end held off its circle by fixes: only its own row fails.
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(v(10.0, 0.0));
    let e = doc.add_point(v(0.0, 10.0));
    let arc = doc.add_arc(c, s, e).unwrap();
    doc.set_point(e, v(0.0, 12.0)).unwrap();
    for (point, at) in [(c, v(0.0, 0.0)), (s, v(10.0, 0.0)), (e, v(0.0, 12.0))] {
        doc.add_constraint(Constraint::Fix { point, at }).unwrap();
    }
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(report.unsourced_failure && report.conflicts.is_empty());
    assert_eq!(report.broken, vec![arc]);
    assert_eq!(report.exposed, vec![arc]);
}

/// The other rows no relation owns have owners too: a line's hidden midpoint (a distance
/// measured to it that its fixed ends can't give) and an ellipse's axes (held askew).
#[test]
fn midpoints_and_ellipses_are_reported_broken_by_their_own_entity() {
    let mut doc = Document::new();
    let (a, b) = (doc.add_point(v(0.0, 0.0)), doc.add_point(v(20.0, 0.0)));
    let line = doc.add_line(a, b).unwrap();
    let p = doc.add_point(v(10.0, 10.0));
    for (point, at) in [(a, v(0.0, 0.0)), (b, v(20.0, 0.0)), (p, v(10.0, 10.0))] {
        doc.add_constraint(Constraint::Fix { point, at }).unwrap();
    }
    // The midpoint is 10 from the fixed point, the dimension asks 5.
    doc.add_constraint(Constraint::Distance {
        a: line,
        b: p,
        value: 5.0,
    })
    .unwrap();
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(!report.converged, "{report:?}");
    assert_eq!(report.broken, vec![line], "{report:?}");

    let mut doc = Document::new();
    let (c, m, n) = (
        doc.add_point(v(0.0, 0.0)),
        doc.add_point(v(10.0, 0.0)),
        doc.add_point(v(0.0, 5.0)),
    );
    let ellipse = doc.add_ellipse(c, m, n).unwrap();
    doc.set_point(n, v(2.0, 5.0)).unwrap();
    for (point, at) in [(c, v(0.0, 0.0)), (m, v(10.0, 0.0)), (n, v(2.0, 5.0))] {
        doc.add_constraint(Constraint::Fix { point, at }).unwrap();
    }
    let report = solve(&mut doc, &SolveOptions::default());
    assert!(!report.converged, "{report:?}");
    assert_eq!(report.broken, vec![ellipse], "{report:?}");
    assert_eq!(analyze(&doc).broken, vec![ellipse]);
}

/// A value that isn't a number is never written, and `write_back` says so, which is what
/// makes a solve report nothing written (`unwritten`) and nothing broken (#342; #215).
#[test]
fn a_value_that_isnt_a_number_is_not_written() {
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(v(10.0, 0.0));
    let e = doc.add_point(v(0.0, 10.0));
    doc.add_arc(c, s, e).unwrap();
    let mut sys = System::build(&doc, &[], &Tolerances::default(), None);
    let k = sys.param_index[&Param::X(s)];
    let g = sys.group_of[k];
    sys.groups[g].value = f64::NAN;
    assert!(!write_back(&mut doc, &sys));
    assert_eq!(doc.point(s).unwrap(), v(10.0, 0.0));
}

/// The step limit's size of the drawing: points only, unless a drag's solve asks for circles
/// too, where a circle reaches its centre's distance from the middle plus its radius. A
/// non-drag solve's limit is exactly as before. (A centre at the origin and a point at
/// (−0.5, 0) on a circle of radius 5: points only, 0.5 across; with the circle, 10.5.)
#[test]
fn a_drags_step_limit_counts_how_far_a_circle_reaches() {
    for (scale, at) in [(1.0, v(0.0, 0.0)), (10.0, v(0.0, 0.0)), (0.1, v(1e5, 1e5))] {
        let mut doc = Document::new();
        doc.begin();
        let c = doc.add_point(at);
        doc.add_circle(c, 5.0 * scale).unwrap();
        doc.add_point(at + v(-0.5, 0.0) * scale);
        let sys = System::build(&doc, &[], &Tolerances::default(), None);
        let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
        let points = drawing_across(&sys, &values, false);
        let reach = drawing_across(&sys, &values, true);
        assert!(
            (points - 0.5 * scale).abs() <= 1e-9 * scale,
            "{scale}× about {at}: {points}"
        );
        assert!(
            (reach - 10.5 * scale).abs() <= 1e-9 * scale,
            "{scale}× about {at}: {reach}"
        );
    }
}

/// `meets_within` judges a part as a solve does, in its frame against the solver's own bar: a
/// solved part with a Perpendicular between a 20 mm and a 17 mm line (turned by a 3-4-5
/// angle) meets it at the origin and 1 m, 10 m and 1 km out.
#[test]
fn meets_within_holds_a_solved_part_at_any_place() {
    for at in [0.0, 1e3, 1e4, 1e6] {
        let o = v(at, at);
        let (c, s) = (0.8, 0.6);
        let mut doc = Document::new();
        doc.begin();
        let p0 = doc.add_point(o);
        let p1 = doc.add_point(o + v(20.0 * c, 20.0 * s));
        let p2 = doc.add_point(o + v(-17.0 * s + 0.01, 17.0 * c));
        let a = doc.add_line(p0, p1).unwrap();
        let b = doc.add_line(p0, p2).unwrap();
        doc.add_constraint(Constraint::Perpendicular { a, b })
            .unwrap();
        doc.add_constraint(Constraint::Fix { point: p0, at: o })
            .unwrap();
        assert!(solved(&mut doc).converged);
        let scope: HashSet<EntityId> = doc.iter().map(|(id, _)| id).collect();
        let relations = relations(&doc, None);
        assert!(
            meets_within(&doc, &scope, &relations, None, None),
            "{at} out: the solved part fails"
        );
    }
}

/// A sketch whose relations hold commits wherever it is (#387): a 10 mm line held square to a
/// 12 mm one, a third at 30° to it, with lengths, a kilometre and ten kilometres out. Held to
/// a sine of 1e-12 in absolute coordinates, a 10 mm line's direction is known only to about
/// 1e-11 there and the commit failed as a conflict; in its frame it is met.
#[test]
fn a_sketch_commits_far_from_the_origin() {
    for at in [0.0, 707_106.78, 7_071_067.8] {
        let o = v(at, -at);
        let mut doc = Document::new();
        doc.begin();
        let p =
            [v(0.0, 0.0), v(10.0, 0.3), v(-0.2, 12.0), v(8.0, 6.0)].map(|q| doc.add_point(o + q));
        let a = doc.add_line(p[0], p[1]).unwrap();
        let b = doc.add_line(p[0], p[2]).unwrap();
        let c = doc.add_line(p[0], p[3]).unwrap();
        for k in [
            Constraint::Perpendicular { a, b },
            Constraint::Angle {
                a,
                b: c,
                degrees: 30.0,
            },
            Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 10.0,
            },
            Constraint::Distance {
                a: p[0],
                b: p[2],
                value: 12.0,
            },
            Constraint::Distance {
                a: p[0],
                b: p[3],
                value: 9.0,
            },
        ] {
            doc.add_constraint(k).unwrap();
        }
        let report = solved(&mut doc);
        assert!(report.converged, "{at} out: {report:?}");
        assert!(report.conflicts.is_empty(), "{at} out: {report:?}");
        assert!(
            unmet(&doc, BAR).is_empty(),
            "{at} out: {:?}",
            unmet(&doc, BAR)
        );
    }
}

/// A solve whose rows already hold writes nothing (#280): a solved sketch a kilometre and a
/// thousand kilometres out (where the bar meets its floor of ulps), solved again, keeps every
/// coordinate to the bit.
#[test]
fn a_solve_whose_rows_hold_writes_nothing() {
    for at in [0.0, 1e6, 1e9] {
        let o = v(at, at);
        let mut doc = Document::new();
        doc.begin();
        let (p, l) = quad(&mut doc, o);
        constrain_rectangle(&mut doc, p, l, o);
        assert!(solved(&mut doc).converged);
        let before: Vec<(u64, u64)> = p
            .iter()
            .map(|&q| doc.point(q).unwrap())
            .map(|q| (q.x.to_bits(), q.y.to_bits()))
            .collect();
        for _ in 0..3 {
            let report = solved(&mut doc);
            assert!(report.converged, "{at}");
            assert_eq!(report.iterations, 0, "{at}");
            let after: Vec<(u64, u64)> = p
                .iter()
                .map(|&q| doc.point(q).unwrap())
                .map(|q| (q.x.to_bits(), q.y.to_bits()))
                .collect();
            assert_eq!(before, after, "{at} out: a solve that held moved something");
        }
    }
}

/// The frame is exact: values a kilometre out translated into a component's frame and back,
/// untouched, are what was stored, bit for bit; and a value changed in the frame goes back by
/// the same shift.
#[test]
fn a_frame_round_trips_exactly() {
    let mut doc = Document::new();
    doc.begin();
    let at = v(707_106.781_186_547_5, -123_456.789_012_345_6);
    let (p, l) = quad(&mut doc, at);
    constrain_rectangle(&mut doc, p, l, at);
    let tol = Tolerances::default();
    let sys = System::build(&doc, &[], &tol, None);
    let (comps, _) = components(&sys, &tol);
    let mut values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let stored = values.clone();
    let centres = circle_centres(&sys);
    for comp in &comps {
        let (_, groups, asked) = reach(&sys, &comp.unknowns, &comp.rows);
        let frame = Frame::enter(&sys, &groups, asked, &mut values, &tol, &centres);
        // (In the frame, the part's coordinates are about its own size.)
        assert!(
            groups.iter().all(|&g| values[g].abs() <= 64.0),
            "{:?}",
            groups.iter().map(|&g| values[g]).collect::<Vec<_>>()
        );
        frame.leave(&mut values);
    }
    assert_eq!(
        stored.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        values.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
}

/// The same drag's solve, the same at any size and place (#280, #366): two lines held square,
/// one end dragged, at 0.1×, 1× and 10×, and 100 m and 1 km out. Each takes the same number
/// of iterations, and lands within a billionth of the part's size of the same place.
#[test]
fn a_drag_solve_is_the_same_at_any_size_and_place() {
    let run = |scale: f64, out: f64| {
        let mut doc = Document::new();
        doc.begin();
        let o = v(out, out);
        let a = doc.add_point(o);
        let b = doc.add_point(o + v(10.0, 0.0) * scale);
        let c = doc.add_point(o + v(1.0, 9.0) * scale);
        let l = doc.add_line(a, b).unwrap();
        let m = doc.add_line(a, c).unwrap();
        doc.add_constraint(Constraint::Perpendicular { a: l, b: m })
            .unwrap();
        doc.add_constraint(Constraint::Distance {
            a,
            b: c,
            value: 9.0 * scale,
        })
        .unwrap();
        doc.set_point(b, o + v(9.0, 3.0) * scale).unwrap();
        let report = solve(
            &mut doc,
            &SolveOptions {
                dragged: Param::point(b).to_vec(),
                lengths: [(m, 9.0 * scale)].into_iter().collect(),
                ..Default::default()
            },
        );
        assert!(report.converged, "{scale}× {out} out");
        let at = [a, b, c].map(|q| (doc.point(q).unwrap() - o) / scale);
        (report.iterations, at)
    };
    let (iterations, origin) = run(1.0, 0.0);
    for (scale, out) in [(0.1, 0.0), (10.0, 0.0), (1.0, 1e5), (1.0, 1e6)] {
        let (n, at) = run(scale, out);
        assert_eq!(n, iterations, "{scale}× {out} out");
        for (p, q) in at.iter().zip(&origin) {
            assert!(
                p.distance(*q) <= 1e-9 * 10.0,
                "{scale}× {out} out: {p} against {q}"
            );
        }
    }
}

/// `unmet` and `meets_within` read each component's own bar: a point held on a 1 mm line and
/// left 1e-9 off it is unmet at the solver's own slack (a quarter of a billionth of its
/// component's 1 mm), beside a 100 mm part whose bar would let it pass; and 3e-12 off, it
/// meets it. The same 10 m out (10,000 times its size; a million times out, the floor of ulps
/// of its coordinates is the coarser).
#[test]
fn unmet_and_meets_within_read_each_components_bar() {
    for at in [0.0, 1e4] {
        for (off, meets) in [(1e-9, false), (3e-12, true)] {
            let mut doc = Document::new();
            doc.begin();
            let a = doc.add_point(v(at - 50.0, at));
            let b = doc.add_point(v(at + 50.0, at));
            let l = doc.add_line(a, b).unwrap();
            let c = doc.add_point(v(at - 0.5, at));
            let d = doc.add_point(v(at + 0.5, at));
            let m = doc.add_line(c, d).unwrap();
            let q = doc.add_point(v(at, at + off));
            let on = doc
                .add_constraint(Constraint::PointOnCurve { point: q, curve: m })
                .unwrap();
            assert_eq!(!unmet(&doc, BAR).contains(&on), meets, "{at}: {off:e}");
            assert_eq!(
                !unmet_with(&doc, BAR, None).contains(&on),
                meets,
                "{at}: {off:e}"
            );
            let scope: HashSet<EntityId> = [a, b, l, c, d, m, q].into_iter().collect();
            assert_eq!(
                meets_within(&doc, &scope, &relations(&doc, None), None, None),
                meets,
                "{at}: {off:e}"
            );
        }
    }
}

/// No stall is read as landed: a drag's solve whose rows can't hold (a triangle held to sides
/// 10, 3 and 3, which no triangle has) is never reported converged, however its iterations
/// end (the cap, or the damping giving up), at 0.1×, 1× and 10×, near the origin and a
/// kilometre out.
#[test]
fn a_drag_solve_whose_rows_cannot_hold_is_never_converged() {
    for (scale, at) in [
        (1.0, v(0.0, 0.0)),
        (0.1, v(0.0, 0.0)),
        (10.0, v(0.0, 0.0)),
        (1.0, v(40.0, -40.0)),
        (1.0, v(1e6, -1e6)),
    ] {
        let mut doc = Document::new();
        doc.begin();
        let p = [v(0.0, 0.0), v(10.0, 0.0), v(5.0, 2.0)].map(|q| doc.add_point(at + q * scale));
        for (i, j, d) in [(0, 1, 10.0), (1, 2, 3.0), (2, 0, 3.0)] {
            doc.add_constraint(Constraint::Distance {
                a: p[i],
                b: p[j],
                value: d * scale,
            })
            .unwrap();
        }
        doc.set_point(p[2], at + v(5.0, 2.5) * scale).unwrap();
        let report = solve(
            &mut doc,
            &SolveOptions {
                dragged: Param::point(p[2]).to_vec(),
                ..Default::default()
            },
        );
        assert!(!report.converged, "{scale}× about {at}: {report:?}");
    }
}

/// The fold steps meet a kept radius at a fold whatever the round-off (macOS CI, PR 2):
/// `a_kept_radius_at_a_fold_is_met_with_the_fold_steps`' drag, its offset nudged by up to
/// 4 ulps either way in x and y, lands and keeps its radius every time. Held to the part's
/// bar, a fold solve's rows hold sooner than they did against the coordinates, and patience
/// cut 38 of 289 such nudges mid-crawl until a fold solve was let run on to the patience
/// share and every kept shape met.
#[test]
fn a_kept_radius_at_a_fold_is_met_whatever_the_round_off() {
    let at = |deg: f64| v(8.0, 0.0) + DVec2::from_angle(deg.to_radians()) * 8.0;
    for kx in -4i32..=4 {
        for ky in -4i32..=4 {
            let mut doc = Document::new();
            let e = doc.add_point(v(0.0, 0.0));
            let c = doc.add_point(v(8.0, 0.0));
            let s = doc.add_point(at(135.0));
            let arc = doc.add_arc(c, s, e).unwrap();
            let held = [at(135.0), at(315.0)].map(|p| {
                let point = doc.add_point(p);
                doc.add_constraint(Constraint::PointOnCurve { point, curve: arc })
                    .unwrap();
                point
            });
            let delta = v(
                f64::from(kx) * f64::EPSILON,
                -18.0 + f64::from(ky) * 18.0 * f64::EPSILON,
            );
            let (report, off) = drag_points(&mut doc, &held, delta, &[arc], true);
            assert!(
                report.converged && off == 0.0,
                "{kx}, {ky}: {report:?}, {off} off"
            );
            let r = crate::radius_of(&doc, arc).unwrap();
            assert!((r - 8.0).abs() <= 8.0 * FOLD_MET, "{kx}, {ky}: radius {r}");
        }
    }
}
