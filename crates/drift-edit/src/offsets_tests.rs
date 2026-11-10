//! Offsets as objects (#247): one record with one distance, following its source (#239),
//! guarded, copied, detached; and the family of hidden dimensions files from before #247
//! hold, still driven as one.

use std::collections::BTreeSet;

use drift_core::{
    Constraint, DVec2, EntityId, EntityKind, Kind, OffsetId, PropertyRef, Shape, Side,
};
use drift_solver::Param;

use crate::editor::{EditError, Editor};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// An editor with automatic relations on, as the app starts.
fn related() -> Editor {
    let mut ed = Editor::default();
    ed.prefs.auto_relations = true;
    ed
}

/// A closed polyline through `points`, committed; its lines.
fn polyline(ed: &mut Editor, points: &[DVec2], closed: bool) -> Vec<EntityId> {
    ed.doc.begin();
    let ps: Vec<EntityId> = points.iter().map(|p| ed.doc.add_point(*p)).collect();
    let n = ps.len();
    let mut ids = Vec::new();
    for i in 0..n {
        if i + 1 == n && !closed {
            break;
        }
        ids.push(ed.doc.add_line(ps[i], ps[(i + 1) % n]).unwrap());
    }
    ed.commit("Polyline");
    ids
}

/// A 40 × 30 rectangle offset 2 outward through its bottom side.
fn rectangle_offset(ed: &mut Editor) -> (Vec<EntityId>, OffsetId) {
    let ids = polyline(
        ed,
        &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)],
        true,
    );
    let relations = ed.doc.constraint_count();
    let x = ed.offset(ids[0], 2.0, v(20.0, -5.0)).unwrap();
    assert_eq!(
        ed.doc.constraint_count(),
        relations,
        "an offset adds no relations"
    );
    (ids, x)
}

/// How far each piece of an offset (lines) sits from the source chain, from its middle.
fn gaps(ed: &Editor, x: OffsetId) -> Vec<f64> {
    let o = ed.doc.offset(x).expect("the offset");
    o.made
        .curves
        .values()
        .map(|c| {
            let Some(Shape::Line { a, b }) = ed.doc.shape(*c) else {
                panic!("a line piece")
            };
            let middle = (a + b) * 0.5;
            o.source
                .iter()
                .filter_map(|s| ed.doc.shape(*s))
                .map(|s| drift_geom::distance(&s, middle))
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn all_at(gaps: &[f64], d: f64) -> bool {
    !gaps.is_empty() && gaps.iter().all(|g| (g - d).abs() < 1e-6)
}

fn pieces(ed: &Editor, x: OffsetId) -> Vec<EntityId> {
    ed.offset_curves(x)
}

#[test]
fn an_offset_is_one_record_with_one_dimension_and_no_relations() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    assert_eq!(ed.doc.offsets().count(), 1);
    let o = ed.doc.offset(x).unwrap().clone();
    // (A closed chain may start at any of its curves; it runs on in order.)
    assert_eq!(o.source.len(), 4);
    let at = o
        .source
        .iter()
        .position(|c| *c == ids[0])
        .expect("in the chain");
    for (k, id) in ids.iter().enumerate() {
        assert_eq!(o.source[(at + k) % 4], *id, "the chain in travel order");
    }
    assert_eq!(o.distance, 2.0);
    assert_eq!(o.made.curves.len(), 4);
    assert_eq!(o.made.points.len(), 4);
    assert!(all_at(&gaps(&ed, x), 2.0), "{:?}", gaps(&ed, x));
    // One property dimension, on the picked side, and no relations at all.
    let dims: Vec<_> = ed.doc.property_dims().collect();
    assert_eq!(dims.len(), 1);
    assert_eq!(dims[0].1.target, PropertyRef::OffsetDistance(x));
    // (On the picked curve, wherever it sits in the chain.)
    assert_eq!(
        usize::from(dims[0].1.seg),
        o.source.iter().position(|c| *c == ids[0]).unwrap()
    );
    assert_eq!(
        ed.doc.variables().count(),
        0,
        "nothing in Parameters: {:?}",
        ed.doc.variables().collect::<Vec<_>>()
    );
    // The pieces have the source's look and are never solver unknowns.
    let generated = ed.doc.generated_entities();
    assert!(o.made.entities().all(|e| generated.contains(&e)));
    for c in o.made.curves.values() {
        assert_eq!(
            ed.doc.get(*c).unwrap().layer,
            ed.doc.get(ids[0]).unwrap().layer
        );
    }
    // One undo step.
    assert!(ed.undo());
    assert_eq!(ed.doc.offsets().count(), 0);
    assert_eq!(ed.doc.property_dims().count(), 0);
    assert!(o.made.entities().all(|e| !ed.doc.contains(e)));
    assert!(ed.redo());
    assert_eq!(ed.doc.offsets().count(), 1);
    assert!(all_at(&gaps(&ed, x), 2.0));
}

/// #239: the offset of a plate follows the plate when a variable drives its width, and adds
/// no degrees of freedom of its own.
#[test]
fn the_offset_follows_its_source_and_adds_no_freedom() {
    let mut ed = related();
    let ids = polyline(
        &mut ed,
        &[v(0.0, 0.0), v(60.0, 0.0), v(60.0, 40.0), v(0.0, 40.0)],
        true,
    );
    let corner = |ed: &Editor, line: EntityId, first: bool| match ed.doc.get(line).unwrap().kind {
        EntityKind::Line { a, b } => {
            if first {
                a
            } else {
                b
            }
        }
        _ => unreachable!(),
    };
    let (p0, p1) = (corner(&ed, ids[0], true), corner(&ed, ids[0], false));
    let p3 = corner(&ed, ids[3], true);
    ed.add_constraints(
        "Rectangle",
        &[
            Constraint::Horizontal { line: ids[0] },
            Constraint::Horizontal { line: ids[2] },
            Constraint::Vertical { line: ids[1] },
            Constraint::Vertical { line: ids[3] },
            Constraint::Fix {
                point: p0,
                at: v(0.0, 0.0),
            },
        ],
    )
    .unwrap();
    let w = ed.add_variable("W", "60", Kind::Length).unwrap();
    let width = ed
        .add_dimension(
            Constraint::Distance {
                a: p0,
                b: p1,
                value: 60.0,
            },
            None,
        )
        .unwrap();
    ed.set_dimension_text(width, "W").unwrap();
    ed.add_dimension(
        Constraint::Distance {
            a: p0,
            b: p3,
            value: 40.0,
        },
        None,
    )
    .unwrap();
    assert_eq!(ed.analysis().dof, Some(0), "the plate is fully defined");
    let x = ed.offset(ids[0], 3.0, v(30.0, -5.0)).unwrap();
    assert_eq!(ed.analysis().dof, Some(0), "the offset adds no freedom");
    assert!(ed.analysis().conflicts.is_empty());
    assert!(all_at(&gaps(&ed, x), 3.0));
    let before: Vec<EntityId> = pieces(&ed, x);
    // The plate grows: the offset follows, same pieces.
    ed.set_variable(w, "100").unwrap();
    // (Within the solver's bar: a billionth of a quarter of the plate's size.)
    assert!(ed.doc.point(p1).unwrap().distance(v(100.0, 0.0)) < 1e-7);
    assert!(all_at(&gaps(&ed, x), 3.0), "{:?}", gaps(&ed, x));
    assert_eq!(pieces(&ed, x), before, "the same curves, moved");
    let right =
        ed.doc.offset(x).unwrap().made.curves[&drift_core::PieceKey::Of { of: ids[1], n: 0 }];
    let Some(Shape::Line { a, .. }) = ed.doc.shape(right) else {
        unreachable!()
    };
    assert!((a.x - 103.0).abs() < 1e-7, "{a}");
    // One undo step puts both back.
    assert!(ed.undo());
    assert_eq!(ed.doc.point(p1).unwrap(), v(60.0, 0.0));
    assert!(all_at(&gaps(&ed, x), 3.0));
}

#[test]
fn the_distance_is_set_by_text_or_expression_and_follows_a_variable() {
    let mut ed = related();
    let (_, x) = rectangle_offset(&mut ed);
    ed.set_offset_distance(&[x], "3").unwrap();
    assert!(all_at(&gaps(&ed, x), 3.0));
    assert_eq!(ed.doc.offset(x).unwrap().distance_expr, None);
    let t = ed.add_variable("t", "5", Kind::Length).unwrap();
    ed.set_offset_distance(&[x], "t").unwrap();
    assert!(all_at(&gaps(&ed, x), 5.0));
    assert_eq!(
        ed.doc.offset(x).unwrap().distance_expr.as_deref(),
        Some("t")
    );
    ed.set_variable(t, "7").unwrap();
    assert!(all_at(&gaps(&ed, x), 7.0), "follows the variable");
    assert!(ed.undo());
    assert!(all_at(&gaps(&ed, x), 5.0), "one undo step");
    assert!(ed.undo());
    assert!(all_at(&gaps(&ed, x), 3.0));
    // Nonsense is refused and changes nothing.
    assert!(ed.set_offset_distance(&[x], "0").is_err());
    assert!(ed.set_offset_distance(&[x], "-2").is_err());
    assert!(all_at(&gaps(&ed, x), 3.0));
    // A drag of a piece sets the distance too (inside a transaction, as the tool does).
    ed.doc.begin();
    assert!(ed.drag_offset_distance(x, 4.5));
    assert!(ed.commit("Change offset"));
    assert!(all_at(&gaps(&ed, x), 4.5));
    assert_eq!(ed.offset_distance_at(x, v(20.0, -9.0)), Some(9.0));
}

#[test]
fn dragging_a_source_corner_moves_the_pieces_as_it_goes() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    let EntityKind::Line { b, .. } = ed.doc.get(ids[0]).unwrap().kind else {
        unreachable!()
    };
    let bottom =
        ed.doc.offset(x).unwrap().made.curves[&drift_core::PieceKey::Of { of: ids[0], n: 0 }];
    ed.doc.begin();
    let dragged: Vec<Param> = Param::point(b).into_iter().collect();
    assert!(ed.drag(&[b], v(10.0, 0.0), &dragged));
    let Some(Shape::Line { b: end, .. }) = ed.doc.shape(bottom) else {
        unreachable!()
    };
    assert!(end.x > 50.0, "the piece followed mid-drag: {end}");
    ed.doc.rollback();
    ed.sync();
    assert!(all_at(&gaps(&ed, x), 2.0));
}

#[test]
fn deleting_the_source_takes_the_offset_and_undo_brings_both_back() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    let made: Vec<EntityId> = ed.doc.offset(x).unwrap().made.entities().collect();
    ed.selection = ids.iter().copied().collect();
    ed.delete_selection();
    assert_eq!(ed.doc.offsets().count(), 0);
    assert_eq!(ed.doc.property_dims().count(), 0);
    assert!(made.iter().all(|e| !ed.doc.contains(*e)));
    assert!(ed.undo());
    assert_eq!(ed.doc.offsets().count(), 1);
    assert!(made.iter().all(|e| ed.doc.contains(*e)));
    assert!(all_at(&gaps(&ed, x), 2.0));
    // Deleting the offset (picked by any piece) takes the record too, not the source.
    ed.selection = ed.pick_set(made[0]);
    assert_eq!(ed.selection.len(), 4, "a piece picks the offset");
    ed.delete_selection();
    assert_eq!(ed.doc.offsets().count(), 0);
    assert!(ids.iter().all(|c| ed.doc.contains(*c)));
}

#[test]
fn detach_leaves_plain_curves_with_their_look() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    let layer = ed.add_layer("Trim");
    ed.selection = ed.offset_pick_set(x);
    ed.move_selection_to_layer(layer);
    let made: Vec<EntityId> = pieces(&ed, x);
    assert!(made.iter().all(|c| ed.doc.get(*c).unwrap().layer == layer));
    assert!(ed.detach_offsets(&[x]));
    assert_eq!(ed.doc.offsets().count(), 0);
    assert_eq!(ed.doc.property_dims().count(), 0);
    assert!(made.iter().all(|c| ed.doc.contains(*c)));
    assert!(made.iter().all(|c| ed.doc.get(*c).unwrap().layer == layer));
    assert!(ed.doc.generated_entities().is_empty(), "plain curves now");
    assert_eq!(ed.selection, made.iter().copied().collect::<BTreeSet<_>>());
    // They no longer follow.
    let EntityKind::Line { b, .. } = ed.doc.get(ids[0]).unwrap().kind else {
        unreachable!()
    };
    ed.doc.begin();
    ed.doc.set_point(b, v(50.0, 0.0)).unwrap();
    ed.commit("Move");
    let Some(Shape::Line { b: end, .. }) = ed.doc.shape(made[0]) else {
        unreachable!()
    };
    assert!(end.x <= 42.0 + 1e-9, "{end}");
    assert!(ed.undo() && ed.undo());
    assert_eq!(ed.doc.offsets().count(), 1, "undo restores the offset");
}

#[test]
fn copies_keep_the_offset_when_the_source_comes_too_else_plain_curves() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    // Duplicate the plate with its offset: a second offset following the copy.
    ed.selection = ids.iter().copied().collect();
    ed.selection.extend(ed.offset_pick_set(x));
    ed.duplicate_selection();
    assert_eq!(ed.doc.offsets().count(), 2);
    let copy = ed
        .doc
        .offsets()
        .map(|(id, _)| id)
        .find(|id| *id != x)
        .unwrap();
    assert!(all_at(&gaps(&ed, copy), 2.0), "{:?}", gaps(&ed, copy));
    assert!(all_at(&gaps(&ed, x), 2.0));
    let EntityKind::Line { b, .. } = ed
        .doc
        .get(ed.doc.offset(copy).unwrap().source[0])
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    ed.doc.begin();
    ed.doc.set_point(b, v(70.0, 0.0)).unwrap();
    ed.commit("Move");
    assert!(
        all_at(&gaps(&ed, copy), 2.0),
        "the copy follows its own source"
    );
    assert!(ed.undo() && ed.undo());
    assert_eq!(ed.doc.offsets().count(), 1);
    // Duplicate the offset alone: plain curves, no second offset.
    let before = ed.doc.len();
    ed.selection = ed.offset_pick_set(x);
    ed.duplicate_selection();
    assert_eq!(ed.doc.offsets().count(), 1);
    assert_eq!(ed.doc.len(), before + 8, "four lines and their corners");
    assert_eq!(
        ed.doc.generated_entities().len(),
        8,
        "the original's pieces only"
    );
    assert!(ed.undo());
    // Mirror the plate with its offset: the image's offset lies on the other side.
    let axis = polyline(&mut ed, &[v(50.0, -10.0), v(50.0, 50.0)], false)[0];
    ed.selection = ids.iter().copied().collect();
    ed.selection.extend(ed.offset_pick_set(x));
    ed.mirror(&ids, axis).unwrap();
    assert_eq!(ed.doc.offsets().count(), 2);
    let image = ed
        .doc
        .offsets()
        .map(|(id, _)| id)
        .find(|id| *id != x)
        .unwrap();
    assert_eq!(ed.doc.offset(image).unwrap().side, Side::Left, "flipped");
    assert_eq!(ed.doc.offset(x).unwrap().side, Side::Right);
    assert!(all_at(&gaps(&ed, image), 2.0), "{:?}", gaps(&ed, image));
    // Outward still: its pieces lie outside the mirrored plate (x from 60 to 100).
    for c in pieces(&ed, image) {
        let Some(Shape::Line { a, b }) = ed.doc.shape(c) else {
            unreachable!()
        };
        let m = (a + b) * 0.5;
        assert!(m.x < 60.0 || m.x > 100.0 || m.y < 0.0 || m.y > 30.0, "{m}");
    }
}

#[test]
fn the_guards_refuse_edits_of_the_pieces_and_name_detach() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    let made = pieces(&ed, x);
    let piece = made[0];
    let says_detach = |r: Result<(), EditError>| match r {
        Err(EditError::Invalid(why)) => why.contains("Detach"),
        _ => false,
    };
    assert!(says_detach(ed.trim_at(piece, v(20.0, -2.0))));
    let EntityKind::Line { a, .. } = ed.doc.get(piece).unwrap().kind else {
        unreachable!()
    };
    assert!(says_detach(ed.fillet(a, 1.0).map(|_| ())));
    assert!(says_detach(
        ed.offset(piece, 1.0, v(20.0, -5.0)).map(|_| ())
    ));
    assert!(ed.walls_from_curves(&made).is_err());
    assert!(says_detach(
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: piece }])
            .map(|_| ())
    ));
    // A dimension to a piece is a reference dimension, with a notice.
    let dim = ed
        .add_dimension(
            Constraint::PointLineDistance {
                point: a,
                line: ids[2],
                value: 34.0,
            },
            None,
        )
        .unwrap();
    assert!(ed.doc.is_driven(dim));
    assert!(ed.out.notice.take().is_some_and(|n| n.contains("Detach")));
    // A nudge of the offset alone moves nothing and says so; with the source picked too,
    // the source moves and the offset follows.
    ed.selection = ed.offset_pick_set(x);
    assert_eq!(ed.nudge(v(5.0, 0.0)), Some(false));
    assert!(
        ed.out
            .notice
            .take()
            .is_some_and(|n| n.contains("move the source"))
    );
    assert!(all_at(&gaps(&ed, x), 2.0));
    ed.selection.extend(ids.iter().copied());
    assert_eq!(ed.nudge(v(5.0, 0.0)), Some(true));
    assert_eq!(
        ed.doc.point(a).unwrap().x,
        3.0 + 0.0 - 2.0 + 2.0,
        "{}",
        ed.doc.point(a).unwrap()
    );
    assert!(all_at(&gaps(&ed, x), 2.0));
    assert_eq!(ed.doc.offsets().count(), 1);
    // Picked: the Inspector's offsets.
    ed.selection = ed.offset_pick_set(x);
    assert_eq!(ed.selected_offsets(), [x].into());
    assert!(
        ed.transform_box().is_none(),
        "no handles round an offset alone"
    );
}

#[test]
fn a_wall_face_can_be_offset_and_follows_the_wall() {
    let mut ed = related();
    let line = polyline(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)], false)[0];
    let w = ed.walls_from_curves(&[line]).unwrap()[0];
    let face = ed
        .doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .iter()
        .find(|(k, _)| k.part == drift_core::Part::Left)
        .map(|(_, id)| *id)
        .unwrap();
    let x = ed.offset(face, 100.0, v(2000.0, 500.0)).unwrap();
    assert!(all_at(&gaps(&ed, x), 100.0));
    ed.set_wall_thickness(&[w], "300").unwrap();
    assert!(
        all_at(&gaps(&ed, x), 100.0),
        "follows the re-thickened face"
    );
}

#[test]
fn an_offset_round_trips_through_a_file_and_is_repaired_on_opening() {
    let mut ed = related();
    let (ids, x) = rectangle_offset(&mut ed);
    ed.set_offset_distance(&[x], "3").unwrap();
    let text = drift_io::save(&ed.doc, &drift_io::Metadata::default());
    assert!(text.contains("\"offsets\": ["), "{text}");
    let (doc, _) = drift_io::load(&text).unwrap();
    assert_eq!(doc, ed.doc);
    let again = Editor::new(doc);
    assert!(!again.out.has_message(), "nothing to repair");
    assert!(all_at(&gaps(&again, x), 3.0));
    // A piece deleted by hand (an older build, say) comes back on opening.
    let mut doc = ed.doc.clone();
    let piece = pieces(&ed, x)[1];
    doc.begin();
    doc.delete([piece]);
    doc.commit("Hand edit");
    let repaired = Editor::new(doc);
    assert!(
        repaired
            .out
            .info
            .as_deref()
            .is_some_and(|n| n.contains("Offsets were repaired"))
    );
    assert_eq!(pieces(&repaired, x).len(), 4);
    assert!(all_at(&gaps(&repaired, x), 3.0));
    let _ = ids;
}

/// Files from before #247 hold offsets as plain curves with hidden distance dimensions, one
/// shown: those are still found as a family and changed together.
#[test]
fn a_family_of_hidden_dimensions_from_before_247_still_moves_as_one() {
    let mut ed = related();
    let base = polyline(&mut ed, &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0)], false);
    let sides = polyline(
        &mut ed,
        &[v(0.0, -2.0), v(42.0, -2.0), v(42.0, 30.0)],
        false,
    );
    let point_of = |ed: &Editor, line: EntityId| match ed.doc.get(line).unwrap().kind {
        EntityKind::Line { a, .. } => a,
        _ => unreachable!(),
    };
    ed.doc.begin();
    let mut dims = Vec::new();
    for (side, orig) in sides.iter().zip(&base) {
        ed.doc
            .add_constraint(Constraint::Parallel { a: *orig, b: *side })
            .unwrap();
        dims.push(
            ed.doc
                .add_constraint(Constraint::PointLineDistance {
                    point: point_of(&ed, *side),
                    line: *orig,
                    value: 2.0,
                })
                .unwrap(),
        );
    }
    ed.doc.set_shown(
        drift_core::AnnotationRef::Dimension(dims[1]),
        drift_core::Shown {
            canvas: false,
            sheets: false,
        },
    );
    ed.commit("Old offset");
    assert_eq!(ed.doc.offsets().count(), 0, "no record: plain curves");
    let (family, from) = ed.offset_side_of(sides[0]).expect("a family");
    assert_eq!(family.len(), 2);
    assert_eq!(from, base[0]);
    assert_eq!(ed.offset_family(dims[1]).len(), 2);
    ed.set_offset(&family, 3.0).unwrap();
    for d in &dims {
        assert_eq!(ed.doc.constraint(*d).unwrap().value(), Some(3.0));
    }
    assert!(ed.undo());
    assert_eq!(ed.doc.constraint(dims[0]).unwrap().value(), Some(2.0));
}

/// Before #247 a circle's offset shared the centre only (a radius change left it behind, and
/// it had a degree of freedom of its own) and an ellipse's was a fitted copy with 60 free
/// degrees. Now both follow and add nothing.
#[test]
fn a_circles_and_an_ellipses_offset_add_no_freedom_and_follow() {
    let mut ed = related();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let circle = ed.doc.add_circle(c, 20.0).unwrap();
    ed.commit("Circle");
    ed.add_constraints(
        "Pin",
        &[Constraint::Fix {
            point: c,
            at: v(0.0, 0.0),
        }],
    )
    .unwrap();
    let radius = ed
        .add_dimension(
            Constraint::Radius {
                curve: circle,
                value: 20.0,
            },
            None,
        )
        .unwrap();
    assert_eq!(ed.analysis().dof, Some(0));
    let x = ed.offset(circle, 3.0, v(30.0, 0.0)).unwrap();
    assert_eq!(ed.analysis().dof, Some(0), "the offset adds no freedom");
    let r = |ed: &Editor| match ed.doc.shape(pieces(ed, x)[0]) {
        Some(Shape::Circle { radius, .. }) => radius,
        other => panic!("{other:?}"),
    };
    assert!((r(&ed) - 23.0).abs() < 1e-9);
    ed.set_dimension_text(radius, "30").unwrap();
    // (Within the solver's bar: a billionth of a quarter of the sketch's size.)
    assert!(
        (r(&ed) - 33.0).abs() < 1e-7,
        "follows the radius: {}",
        r(&ed)
    );
    // An ellipse: a fitted run that is re-fitted when the ellipse changes.
    let mut ed = related();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let major = ed.doc.add_point(v(20.0, 0.0));
    let minor = ed.doc.add_point(v(0.0, 10.0));
    let ellipse = ed.doc.add_ellipse(c, major, minor).unwrap();
    ed.commit("Ellipse");
    let dof = ed.analysis().dof;
    let x = ed.offset(ellipse, 3.0, v(30.0, 0.0)).unwrap();
    assert_eq!(ed.analysis().dof, dof, "the offset adds no freedom");
    let far = |ed: &Editor| -> f64 {
        pieces(ed, x)
            .iter()
            .filter_map(|m| match ed.doc.shape(*m) {
                Some(Shape::Bezier { p }) => Some(p[0].x.max(p[3].x)),
                _ => None,
            })
            .fold(f64::NEG_INFINITY, f64::max)
    };
    assert!((far(&ed) - 23.0).abs() < 1e-3, "{}", far(&ed));
    ed.doc.begin();
    ed.doc.set_point(major, v(30.0, 0.0)).unwrap();
    ed.commit("Stretch");
    assert!(
        (far(&ed) - 33.0).abs() < 1e-3,
        "follows the stretch: {}",
        far(&ed)
    );
}
