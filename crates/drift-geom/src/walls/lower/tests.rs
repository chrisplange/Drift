//! Each row of the lowering table, solved: the face ends up where the relation says, and
//! the wall keeps its thickness.

use std::rc::Rc;

use drift_core::{Constraint, DVec2, Document, EntityId, Justify, LayerId, Path, Wall, WallId};
use drift_solver::{SolveOptions, measure};

use super::*;
use crate::walls::regenerate;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A wall along `pts` (lines), `t` thick, justified `j`; its centreline points.
fn wall(doc: &mut Document, pts: &[DVec2], t: f64, j: Justify) -> (WallId, Vec<EntityId>) {
    let ids: Vec<EntityId> = pts.iter().map(|p| doc.add_point(*p)).collect();
    let curves: Vec<EntityId> = ids
        .windows(2)
        .map(|w| doc.add_line(w[0], w[1]).unwrap())
        .collect();
    let path = doc
        .add_path(Path {
            curves,
            closed: false,
        })
        .unwrap();
    let mut w = Wall::new(path, t, LayerId::DEFAULT);
    w.justify = j;
    (doc.add_wall(w).unwrap(), ids)
}

/// The piece of a wall's face (left or right) on segment `seg`.
fn face(doc: &Document, w: WallId, seg: u16, left: bool) -> EntityId {
    let part = if left { Part::Left } else { Part::Right };
    *doc.wall(w)
        .unwrap()
        .made
        .curves
        .iter()
        .find(|(k, _)| k.seg == seg && k.part == part)
        .unwrap()
        .1
}

fn fix(doc: &mut Document, p: EntityId) {
    let at = doc.point(p).unwrap();
    doc.add_constraint(Constraint::Fix { point: p, at })
        .unwrap();
}

/// Solve through the lowering, then make the faces again.
fn solve(doc: &mut Document) -> drift_solver::SolveReport {
    let lowering = lower(doc);
    let r = drift_solver::solve(
        doc,
        &SolveOptions {
            lowering: Some(Rc::new(lowering)),
            ..Default::default()
        },
    );
    regenerate(doc);
    r
}

fn y_of(doc: &Document, p: EntityId) -> f64 {
    doc.point(p).unwrap().y
}

#[test]
fn a_clear_width_between_faces_moves_the_centrelines() {
    let mut doc = Document::new();
    let (a, pa) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(4000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let (b, pb) = wall(
        &mut doc,
        &[v(0.0, 3000.0), v(4000.0, 3000.0)],
        100.0,
        Justify::Centre,
    );
    regenerate(&mut doc);
    fix(&mut doc, pa[0]);
    fix(&mut doc, pa[1]);
    // Wall a's upper face to wall b's lower face: 3600 clear.
    let (fa, fb) = (face(&doc, a, 0, true), face(&doc, b, 0, false));
    let c = Constraint::PointLineDistance {
        point: fa,
        line: fb,
        value: 3600.0,
    };
    doc.add_constraint(c).unwrap();
    let l = lower(&doc);
    assert_eq!(l.of.len(), 1, "the dimension is lowered");
    assert!(solve(&mut doc).converged);
    assert!((y_of(&doc, pb[0]) - 3700.0).abs() < 1e-6);
    assert!(
        (y_of(&doc, pb[1]) - 3700.0).abs() < 1e-6,
        "b stays level: it was"
    );
    let fb = face(&doc, b, 0, false);
    let c = Constraint::PointLineDistance {
        point: face(&doc, a, 0, true),
        line: fb,
        value: 0.0,
    };
    assert!((measure(&doc, &c).unwrap() - 3600.0).abs() < 1e-6);
}

#[test]
fn a_thicker_wall_keeps_its_dimensioned_face_and_moves_its_centreline() {
    let mut doc = Document::new();
    let (_, pa) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(4000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let (b, pb) = wall(
        &mut doc,
        &[v(0.0, 3000.0), v(4000.0, 3000.0)],
        100.0,
        Justify::Centre,
    );
    regenerate(&mut doc);
    for p in [pa[0], pa[1]] {
        fix(&mut doc, p);
    }
    let centre = doc.path(doc.wall(b).unwrap().centre).unwrap().curves[0];
    doc.add_constraint(Constraint::Horizontal { line: centre })
        .unwrap();
    // A point on wall a's line: from it to b's lower face, 3000.
    let fb = face(&doc, b, 0, false);
    doc.add_constraint(Constraint::PointLineDistance {
        point: pa[0],
        line: fb,
        value: 3000.0,
    })
    .unwrap();
    assert!(solve(&mut doc).converged);
    assert!((y_of(&doc, pb[0]) - 3050.0).abs() < 1e-6);
    // Twice as thick: the lower face stays at 3000, the centreline moves up.
    let mut w = doc.wall(b).unwrap().clone();
    w.thickness = 200.0;
    doc.set_wall(b, w).unwrap();
    assert!(solve(&mut doc).converged);
    assert!((y_of(&doc, pb[0]) - 3100.0).abs() < 1e-6);
    assert!((y_of(&doc, pb[1]) - 3100.0).abs() < 1e-6);
}

#[test]
fn both_faces_held_conflict_with_a_thickness_change() {
    let mut doc = Document::new();
    let (b, _) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(4000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let p = doc.add_point(v(0.0, -1000.0));
    let q = doc.add_point(v(0.0, 1000.0));
    regenerate(&mut doc);
    fix(&mut doc, p);
    fix(&mut doc, q);
    for (pt, left) in [(p, false), (q, true)] {
        let f = face(&doc, b, 0, left);
        doc.add_constraint(Constraint::PointLineDistance {
            point: pt,
            line: f,
            value: 950.0,
        })
        .unwrap();
    }
    assert!(solve(&mut doc).converged, "100 thick fits between them");
    let mut w = doc.wall(b).unwrap().clone();
    w.thickness = 300.0;
    doc.set_wall(b, w).unwrap();
    let r = solve(&mut doc);
    assert!(!r.converged, "both faces held: a thicker wall can't fit");
}

#[test]
fn a_point_on_a_face_and_relations_of_direction() {
    let mut doc = Document::new();
    let (a, pa) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(1000.0, 100.0)],
        100.0,
        Justify::Left,
    );
    regenerate(&mut doc);
    // Its right face (the wall is on the right of a left-justified centreline) level.
    let f = face(&doc, a, 0, false);
    doc.add_constraint(Constraint::Horizontal { line: f })
        .unwrap();
    // And through a fixed point.
    let p = doc.add_point(v(500.0, -300.0));
    fix(&mut doc, p);
    let f = face(&doc, a, 0, false);
    doc.add_constraint(Constraint::PointOnCurve { point: p, curve: f })
        .unwrap();
    assert!(solve(&mut doc).converged);
    assert!(
        (y_of(&doc, pa[0]) - -200.0).abs() < 1e-6,
        "{}",
        y_of(&doc, pa[0])
    );
    assert!(
        (y_of(&doc, pa[1]) - -200.0).abs() < 1e-6,
        "the centreline is level"
    );
}

#[test]
fn collinear_with_a_line_and_parallel() {
    let mut doc = Document::new();
    let (a, pa) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(1000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let l0 = doc.add_point(v(0.0, 500.0));
    let l1 = doc.add_point(v(800.0, 520.0));
    let line = doc.add_line(l0, l1).unwrap();
    regenerate(&mut doc);
    fix(&mut doc, l0);
    fix(&mut doc, l1);
    fix(&mut doc, pa[0]);
    let f = face(&doc, a, 0, true);
    doc.add_constraint(Constraint::Parallel { a: f, b: line })
        .unwrap();
    assert!(solve(&mut doc).converged);
    let d = doc.point(pa[1]).unwrap() - doc.point(pa[0]).unwrap();
    assert!(
        (d.y / d.x - 20.0 / 800.0).abs() < 1e-9,
        "the centreline is parallel"
    );
    // Now collinear (the start let go): its upper face on the line.
    let fixes: Vec<_> = doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Fix { point, .. } if *point == pa[0]))
        .map(|(id, _)| id)
        .collect();
    for id in fixes {
        doc.remove_constraint(id);
    }
    let f = face(&doc, a, 0, true);
    doc.add_constraint(Constraint::Collinear { a: f, b: line })
        .unwrap();
    assert!(solve(&mut doc).converged);
    let f = face(&doc, a, 0, true);
    let Some(drift_core::Shape::Line { a: fa, b: fb }) = doc.shape(f) else {
        panic!()
    };
    let (p0, p1) = (v(0.0, 500.0), v(800.0, 520.0));
    let off = |p: DVec2| (p1 - p0).normalize().perp_dot(p - p0);
    assert!(
        off(fa).abs() < 1e-6 && off(fb).abs() < 1e-6,
        "the face is on the line"
    );
}

#[test]
fn a_reversed_centreline_segment_offsets_the_right_way() {
    let mut doc = Document::new();
    // Two segments, the second's line drawn end to start.
    let ids: Vec<EntityId> = [v(0.0, 0.0), v(1000.0, 0.0), v(1000.0, 1000.0)]
        .iter()
        .map(|p| doc.add_point(*p))
        .collect();
    let c0 = doc.add_line(ids[0], ids[1]).unwrap();
    let c1 = doc.add_line(ids[2], ids[1]).unwrap();
    let path = doc
        .add_path(Path {
            curves: vec![c0, c1],
            closed: false,
        })
        .unwrap();
    let w = doc
        .add_wall(Wall::new(path, 100.0, LayerId::DEFAULT))
        .unwrap();
    regenerate(&mut doc);
    let p = doc.add_point(v(0.0, 500.0));
    fix(&mut doc, p);
    fix(&mut doc, ids[0]);
    doc.add_constraint(Constraint::Vertical { line: c1 })
        .unwrap();
    doc.add_constraint(Constraint::Horizontal { line: c0 })
        .unwrap();
    // The run turns left at (1000, 0): its left face on the second segment faces the point.
    let f = face(&doc, w, 1, true);
    doc.add_constraint(Constraint::PointLineDistance {
        point: p,
        line: f,
        value: 2000.0,
    })
    .unwrap();
    assert!(solve(&mut doc).converged);
    let x = doc.point(ids[1]).unwrap().x;
    assert!(
        (x - 2050.0).abs() < 1e-6,
        "the left face is at 2000: the centreline at {x}"
    );
}

#[test]
fn an_arc_face_radius_is_the_centrelines_less_the_offset() {
    let mut doc = Document::new();
    let c = doc.add_point(v(0.0, 0.0));
    let s = doc.add_point(v(1000.0, 0.0));
    let e = doc.add_point(v(0.0, 1000.0));
    let arc = doc.add_arc(c, s, e).unwrap();
    let path = doc
        .add_path(Path {
            curves: vec![arc],
            closed: false,
        })
        .unwrap();
    let w = doc
        .add_wall(Wall::new(path, 100.0, LayerId::DEFAULT))
        .unwrap();
    regenerate(&mut doc);
    fix(&mut doc, c);
    // Counter-clockwise: the left face is the inner one, 950.
    let inner = face(&doc, w, 0, true);
    assert!((drift_solver::radius_of(&doc, inner).unwrap() - 950.0).abs() < 1e-6);
    doc.add_constraint(Constraint::Radius {
        curve: inner,
        value: 1450.0,
    })
    .unwrap();
    assert!(solve(&mut doc).converged);
    assert!((drift_solver::radius_of(&doc, arc).unwrap() - 1500.0).abs() < 1e-6);
    let inner = face(&doc, w, 0, true);
    assert!((drift_solver::radius_of(&doc, inner).unwrap() - 1450.0).abs() < 1e-6);
    // The outer face's radius too tight for the wall (its centreline's would be negative):
    // refused.
    let outer = face(&doc, w, 0, false);
    let too = Constraint::Radius {
        curve: outer,
        value: 30.0,
    };
    assert_eq!(
        lower_constraint(&doc, &too),
        Some(Err(TOO_TIGHT.to_owned()))
    );
}

#[test]
fn corners_and_end_edges_are_refused() {
    let mut doc = Document::new();
    let (a, _) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(1000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    regenerate(&mut doc);
    let corners: Vec<EntityId> = doc.wall(a).unwrap().made.points.values().copied().collect();
    let c = Constraint::Distance {
        a: corners[0],
        b: corners[1],
        value: 10.0,
    };
    assert_eq!(
        lower_constraint(&doc, &c),
        Some(Err(CORNER_REFUSED.to_owned()))
    );
    let cap = *doc
        .wall(a)
        .unwrap()
        .made
        .curves
        .iter()
        .find(|(k, _)| matches!(k.part, Part::Cap(_)))
        .unwrap()
        .1;
    assert!(matches!(
        lower_constraint(&doc, &Constraint::Horizontal { line: cap }),
        Some(Err(_))
    ));
    // A face's length (its corners) is a corner dimension too; tangency isn't lowered.
    let f = face(&doc, a, 0, true);
    let p = doc.add_point(v(0.0, 500.0));
    assert!(matches!(
        lower_constraint(
            &doc,
            &Constraint::Distance {
                a: f,
                b: p,
                value: 1.0
            }
        ),
        Some(Err(_))
    ));
    // Nothing of a wall's generated: not lowered at all.
    assert_eq!(
        lower_constraint(
            &doc,
            &Constraint::Distance {
                a: p,
                b: p,
                value: 1.0
            }
        ),
        None
    );
}

#[test]
fn flipping_the_justification_keeps_the_held_face() {
    let mut doc = Document::new();
    let (b, pb) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(1000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let p = doc.add_point(v(0.0, 1000.0));
    regenerate(&mut doc);
    fix(&mut doc, p);
    let f = face(&doc, b, 0, true);
    doc.add_constraint(Constraint::PointLineDistance {
        point: p,
        line: f,
        value: 900.0,
    })
    .unwrap();
    doc.add_constraint(Constraint::Horizontal {
        line: doc.path(doc.wall(b).unwrap().centre).unwrap().curves[0],
    })
    .unwrap();
    assert!(solve(&mut doc).converged);
    assert!((y_of(&doc, pb[0]) - 50.0).abs() < 1e-6);
    // The centreline becomes the right face: the left face, held, stays at 100.
    let mut w = doc.wall(b).unwrap().clone();
    w.justify = Justify::Right;
    doc.set_wall(b, w).unwrap();
    assert!(solve(&mut doc).converged);
    assert!((y_of(&doc, pb[0]) - 0.0).abs() < 1e-6);
    let f = face(&doc, b, 0, true);
    let Some(drift_core::Shape::Line { a, .. }) = doc.shape(f) else {
        panic!()
    };
    assert!((a.y - 100.0).abs() < 1e-6);
}

#[test]
fn a_teed_end_of_a_held_wall_stays_on_its_host() {
    let mut doc = Document::new();
    let (host, ph) = wall(
        &mut doc,
        &[v(0.0, 0.0), v(2000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    let (branch, pb) = wall(
        &mut doc,
        &[v(1000.0, 1000.0), v(1000.0, 0.0)],
        100.0,
        Justify::Centre,
    );
    regenerate(&mut doc);
    assert_eq!(doc.wall(branch).unwrap().ends[1].tee, Some((host, 0)));
    // The host held by a dimension on its face; the branch's foot follows it as one solve.
    fix(&mut doc, ph[0]);
    let p = doc.add_point(v(3000.0, 600.0));
    fix(&mut doc, p);
    let f = face(&doc, host, 0, true);
    doc.add_constraint(Constraint::PointLineDistance {
        point: p,
        line: f,
        value: 450.0,
    })
    .unwrap();
    let l = lower(&doc);
    assert_eq!(l.extra.len(), 1, "the tee of a held host is lowered");
    assert!(solve(&mut doc).converged);
    let foot = doc.point(pb[1]).unwrap();
    let (h0, h1) = (doc.point(ph[0]).unwrap(), doc.point(ph[1]).unwrap());
    assert!(
        (h1 - h0).normalize().perp_dot(foot - h0).abs() < 1e-6,
        "the branch's end is on the host's centreline"
    );
}
