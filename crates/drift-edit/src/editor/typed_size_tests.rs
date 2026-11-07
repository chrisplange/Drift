//! A size typed for a distance or angle (a dimension's new value, or a length typed in the
//! Inspector) keeps one end where it is and moves the other the whole change, as a new
//! relation does since #333 (#348): the active element (the last picked) if the size names
//! it; else the line of a point-to-line distance; else the size's first element (a line's
//! start). When that can't hold, the size solves as before.

use drift_core::{Constraint, DVec2};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn at(ed: &Editor, p: EntityId) -> DVec2 {
    ed.doc.point(p).unwrap()
}

fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> (EntityId, EntityId, EntityId) {
    ed.doc.begin();
    let pa = ed.doc.add_point(a);
    let pb = ed.doc.add_point(b);
    let l = ed.doc.add_line(pa, pb).unwrap();
    ed.commit("Line");
    (l, pa, pb)
}

/// Two free points 100 apart with a Distance between them: the editor, the points, the
/// dimension.
fn two_points() -> (Editor, EntityId, EntityId, ConstraintId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let p = ed.doc.add_point(v(0.0, 0.0));
    let q = ed.doc.add_point(v(100.0, 0.0));
    ed.commit("Points");
    let d = ed
        .add_constraints(
            "Distance",
            &[Constraint::Distance {
                a: p,
                b: q,
                value: 100.0,
            }],
        )
        .unwrap()[0];
    (ed, p, q, d)
}

fn pick(ed: &mut Editor, ids: &[EntityId]) {
    ed.selection = ids.iter().copied().collect();
    ed.set_active(ids.last().copied());
}

fn unpick(ed: &mut Editor) {
    ed.selection.clear();
    ed.set_active(None);
}

/// The issue's case: two picked points, a new value typed: the one picked last stays.
#[test]
fn the_last_picked_point_stays() {
    for last in [1, 0] {
        let (mut ed, p, q, d) = two_points();
        let (p0, q0) = (at(&ed, p), at(&ed, q));
        if last == 1 {
            pick(&mut ed, &[p, q]);
        } else {
            pick(&mut ed, &[q, p]);
        }
        ed.set_dimension_text(d, "150").unwrap();
        assert!((at(&ed, p).distance(at(&ed, q)) - 150.0).abs() < 1e-9);
        if last == 1 {
            assert_eq!(at(&ed, q), q0, "the last picked stays exactly");
            assert_ne!(at(&ed, p), p0);
        } else {
            assert_eq!(at(&ed, p), p0, "the last picked stays exactly");
            assert_ne!(at(&ed, q), q0);
        }
    }
}

/// Nothing picked: the dimension's first point stays.
#[test]
fn with_nothing_picked_the_first_point_stays() {
    let (mut ed, p, q, d) = two_points();
    let p0 = at(&ed, p);
    unpick(&mut ed);
    ed.set_dimension_text(d, "60").unwrap();
    assert_eq!(at(&ed, p), p0);
    assert!((at(&ed, p).distance(at(&ed, q)) - 60.0).abs() < 1e-9);
}

/// A line's length typed in the Inspector, the first time (adding its dimension) and the
/// second (changing it), with the line picked or nothing picked, and with automatic
/// relations off (sized once, no dimension kept): its start stays every time.
#[test]
fn a_typed_line_length_keeps_its_start() {
    for (picked, auto) in [(false, true), (true, true), (false, false), (true, false)] {
        let mut ed = Editor::default();
        ed.prefs.auto_relations = auto;
        let (l, a, b) = line(&mut ed, v(-30.0, 0.0), v(20.0, 0.0));
        let start = at(&ed, a);
        let wanted = Constraint::Distance { a, b, value: 0.0 };
        for (text, length) in [("80", 80.0), ("2in", 50.8)] {
            if picked {
                pick(&mut ed, &[l]);
            } else {
                unpick(&mut ed);
            }
            if auto {
                ed.set_size(wanted, text).unwrap();
            } else {
                ed.resize_once(wanted, text).unwrap();
            }
            let what = format!("picked {picked}, auto {auto}, {text}");
            assert_eq!(at(&ed, a), start, "{what}");
            assert!(
                (at(&ed, a).distance(at(&ed, b)) - length).abs() < 1e-9,
                "{what}"
            );
            assert!((at(&ed, b).y).abs() < 1e-9, "{what}: along the line");
        }
    }
}

/// The first point held by a fix: the other end moves, as before. Both held: refused and
/// undone, the geometry as it was.
#[test]
fn a_held_end_falls_back_to_the_solve_as_before() {
    let (mut ed, p, q, d) = two_points();
    let q0 = at(&ed, q);
    ed.add_constraints("Fix", &[Constraint::Fix { point: q, at: q0 }])
        .unwrap();
    unpick(&mut ed);
    ed.set_dimension_text(d, "40").unwrap();
    assert_eq!(at(&ed, q), q0);
    assert!((at(&ed, p).distance(q0) - 40.0).abs() < 1e-9);
    // (Fixed fully, as an old file can have it: Fix itself pins only what is still free.)
    let p0 = at(&ed, p);
    ed.doc.begin();
    ed.doc
        .add_constraint(Constraint::Fix { point: p, at: p0 })
        .unwrap();
    ed.commit("Fix");
    let steps = ed.history.undo_depth();
    assert!(ed.set_dimension_text(d, "70").is_err());
    assert_eq!((at(&ed, p), at(&ed, q)), (p0, q0));
    assert_eq!(ed.history.undo_depth(), steps, "refused: no step");
}

/// A point-to-line distance typed with nothing picked: the line stays, the point moves (the
/// line is the reference); with the point picked last, the point stays.
#[test]
fn a_point_to_line_distance_keeps_the_line() {
    for point_picked in [false, true] {
        let mut ed = Editor::default();
        let (l, a, b) = line(&mut ed, v(0.0, 0.0), v(100.0, 0.0));
        ed.doc.begin();
        let p = ed.doc.add_point(v(50.0, 30.0));
        ed.commit("Point");
        let d = ed
            .add_constraints(
                "Distance",
                &[Constraint::PointLineDistance {
                    point: p,
                    line: l,
                    value: 30.0,
                }],
            )
            .unwrap()[0];
        let (a0, b0, p0) = (at(&ed, a), at(&ed, b), at(&ed, p));
        if point_picked {
            pick(&mut ed, &[l, p]);
        } else {
            unpick(&mut ed);
        }
        ed.set_dimension_text(d, "45").unwrap();
        if point_picked {
            assert_eq!(at(&ed, p), p0);
        } else {
            assert_eq!((at(&ed, a), at(&ed, b)), (a0, b0), "the line stays");
            assert!((at(&ed, p).y - 45.0).abs() < 1e-9);
        }
    }
}

/// An angle between two lines, the second picked last: it keeps its direction, the first
/// turns.
#[test]
fn an_angle_keeps_the_last_picked_line() {
    let mut ed = Editor::default();
    let (l1, a1, b1) = line(&mut ed, v(0.0, 0.0), v(100.0, 0.0));
    let (l2, a2, b2) = line(&mut ed, v(0.0, 50.0), v(70.0, 120.0));
    let d = ed
        .add_constraints(
            "Angle",
            &[Constraint::Angle {
                a: l1,
                b: l2,
                degrees: 45.0,
            }],
        )
        .unwrap()[0];
    let (a20, b20) = (at(&ed, a2), at(&ed, b2));
    let dir1 = (at(&ed, b1) - at(&ed, a1)).normalize();
    pick(&mut ed, &[l1, l2]);
    ed.set_dimension_text(d, "60").unwrap();
    assert_eq!(
        (at(&ed, a2), at(&ed, b2)),
        (a20, b20),
        "the last picked stays"
    );
    let turned = (at(&ed, b1) - at(&ed, a1)).normalize();
    assert!(turned.dot(dir1) < 1.0 - 1e-6, "the first line turned");
}
