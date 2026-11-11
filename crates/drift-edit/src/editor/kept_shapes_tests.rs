//! An edit keeps shapes (the solver audit's PR 6, #350): where the relations leave room, lines
//! keep their lengths and directions and arcs their radii, ranked below the relations and the
//! geometry a relation names.

use drift_core::{Constraint, DVec2};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> (EntityId, EntityId, EntityId) {
    let pa = ed.doc.add_point(a);
    let pb = ed.doc.add_point(b);
    (ed.doc.add_line(pa, pb).unwrap(), pa, pb)
}

fn length(ed: &Editor, a: EntityId, b: EntityId) -> f64 {
    ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap())
}

fn direction(ed: &Editor, a: EntityId, b: EntityId) -> DVec2 {
    (ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap()).normalize()
}

/// #350: a level line made Parallel to a slanted free line. Before, the slanted one (the
/// reference) was stretched from 10.4 to 27.04 mm, and with it active the level one from 20
/// to 52: named geometry had no length to keep, and the cheapest change took one. Now the
/// lines turn and neither stretches, with or without the level one held level, and whichever
/// is active (#333's rule: the active one stays).
#[test]
fn parallel_keeps_both_lines_lengths() {
    for held_level in [false, true] {
        for active in [None, Some(0), Some(1)] {
            let mut ed = Editor::default();
            ed.doc.begin();
            let level = line(&mut ed, v(0.0, 0.0), v(20.0, 0.0));
            let slanted = line(&mut ed, v(30.0, 0.0), v(34.0, 9.6));
            ed.commit("Lines");
            if held_level {
                ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: level.0 }])
                    .unwrap();
            }
            let lines = [level.0, slanted.0];
            ed.selection = lines.into_iter().collect();
            ed.set_active(active.map(|i| lines[i]));
            ed.add_picked_constraints(
                "Parallel",
                &[Constraint::Parallel {
                    a: level.0,
                    b: slanted.0,
                }],
            )
            .unwrap();
            let what = format!("held level {held_level}, active {active:?}");
            assert!(
                (length(&ed, level.1, level.2) - 20.0).abs() < 1e-6,
                "{what}"
            );
            assert!(
                (length(&ed, slanted.1, slanted.2) - 10.4).abs() < 1e-6,
                "{what}"
            );
            let (u, w) = (
                direction(&ed, level.1, level.2),
                direction(&ed, slanted.1, slanted.2),
            );
            assert!(u.perp_dot(w).abs() < 1e-9, "{what}: parallel");
        }
    }
}

/// A dimension typed on a side of a four-sided outline nothing holds square: the side moves
/// across, keeping its direction, and the two beside it stretch along themselves, keeping
/// theirs. (Before, the least change moved the side's far end alone, and the side beside it
/// tilted.)
#[test]
fn a_typed_size_moves_the_side_across_and_keeps_the_outline_square() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let corners = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 25.0), v(0.0, 25.0)];
    let p: Vec<EntityId> = corners.iter().map(|&c| ed.doc.add_point(c)).collect();
    let sides: Vec<EntityId> = (0..4)
        .map(|k| ed.doc.add_line(p[k], p[(k + 1) % 4]).unwrap())
        .collect();
    ed.doc
        .add_constraint(Constraint::Fix {
            point: p[0],
            at: corners[0],
        })
        .unwrap();
    let width = ed
        .doc
        .add_constraint(Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 40.0,
        })
        .unwrap();
    assert!(ed.commit("Outline"));
    let before: Vec<DVec2> = (0..4)
        .map(|k| direction(&ed, p[k], p[(k + 1) % 4]))
        .collect();
    ed.set_dimension(width, 50.0).unwrap();
    assert!((length(&ed, p[0], p[1]) - 50.0).abs() < 1e-6);
    for k in 0..4 {
        let now = direction(&ed, p[k], p[(k + 1) % 4]);
        assert!(
            now.perp_dot(before[k]).abs() < 1e-6 && now.dot(before[k]) > 0.0,
            "side {:?} turned: {:?} -> {now:?}",
            sides[k],
            before[k]
        );
    }
}
