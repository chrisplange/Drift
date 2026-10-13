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
        assert!((got - want).length() < 1e-12, "{got} vs {want}");
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
    let sys = System::build(doc, &[], &Tolerances::default());
    let (comps, _) = components(&sys);
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
    assert!((doc.point(c).unwrap() - m).length() < 1e-12);
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
    assert!((p(m2) - p(m1)).dot(axis_dir).abs() < 1e-9);
    assert!(axis_dir.perp_dot((p(m1) + p(m2)) * 0.5 - p(ax0)).abs() < 1e-9);

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
