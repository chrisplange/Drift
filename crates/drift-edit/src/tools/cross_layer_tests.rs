//! #428: geometry drawn or dropped onto another layer's geometry snaps to it but stays its
//! own (no shared point, no merged vertex): with automatic relations a Coincident relation
//! holds the two together. On the same layer, points join as they always did.

use drift_core::{Constraint, Document, EntityKind, LayerId};
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A snapped pointer at `at`, as the canvas makes it (Auto merge on, automatic relations as
/// the editor's preference says).
fn ptr(ed: &Editor, tool: &dyn Tool, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    p.relations = ed.prefs.auto_relations;
    if tool.wants_snap() {
        p.snap = ed.index.snap(
            at,
            SnapOptions {
                tolerance: TOL,
                grid: None,
                from: None,
            },
            |id| tool.snap_excludes(ed, id),
        );
    }
    tool.refine_pointer(&mut p);
    p
}

fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

fn points(doc: &Document) -> usize {
    doc.iter().filter(|(_, e)| e.kind.is_point()).count()
}

fn coincidents(ed: &Editor) -> Vec<(EntityId, EntityId)> {
    ed.doc
        .constraints()
        .filter_map(|(_, c)| match *c {
            Constraint::Coincident { a, b } => Some((a, b)),
            _ => None,
        })
        .collect()
}

/// The line's two end points.
fn ends(ed: &Editor, line: EntityId) -> (EntityId, EntityId) {
    match ed.doc.get(line).unwrap().kind {
        EntityKind::Line { a, b } => (a, b),
        k => panic!("not a line: {k:?}"),
    }
}

/// The line added last.
fn last_line(ed: &Editor) -> EntityId {
    ed.doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .max()
        .unwrap()
}

/// A line (0,0)-(10,0) on the first layer, then a second layer made current. Returns the
/// line and the new layer.
fn line_then_other_layer(relations: bool) -> (Editor, EntityId, LayerId) {
    let mut ed = Editor::default();
    ed.prefs.auto_relations = relations;
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    let first = last_line(&ed);
    let other = ed.add_layer("Other");
    ed.set_current_layer(other);
    (ed, first, other)
}

#[test]
fn a_line_ending_on_another_layers_vertex_keeps_its_own_point_held_by_a_relation() {
    let (mut ed, first, other) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.2, 0.1)); // snaps onto the first line's end
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let second = last_line(&ed);
    assert_eq!(ed.doc.get(second).unwrap().layer, other);
    let (start, _) = ends(&ed, second);
    assert_ne!(start, corner, "no shared point across layers");
    assert_eq!(points(&ed.doc), 4);
    assert_eq!(
        ed.doc.point(start).unwrap(),
        v(10.0, 0.0),
        "snapped onto it"
    );
    let held = coincidents(&ed);
    assert!(
        held.contains(&(start, corner)) || held.contains(&(corner, start)),
        "held together: {held:?}"
    );
    // Deleting either one leaves the other whole.
    ed.selection = [first].into();
    ed.delete_selection();
    assert!(ed.doc.get(second).is_some());
    assert!(ed.doc.point(start).is_ok());
}

#[test]
fn without_automatic_relations_a_line_on_another_layers_vertex_is_only_placed_there() {
    let (mut ed, first, _) = line_then_other_layer(false);
    let (_, corner) = ends(&ed, first);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.2, 0.1));
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let (start, _) = ends(&ed, last_line(&ed));
    assert_ne!(start, corner);
    assert_eq!(points(&ed.doc), 4);
    assert_eq!(ed.doc.point(start).unwrap(), v(10.0, 0.0));
    assert!(coincidents(&ed).is_empty(), "nothing holds them");
}

#[test]
fn on_the_same_layer_a_line_still_joins() {
    let mut ed = Editor::default();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    let (_, corner) = ends(&ed, last_line(&ed));
    click(&mut ed, &mut tool, v(10.2, 0.1));
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    assert_eq!(ends(&ed, last_line(&ed)).0, corner, "one shared point");
    assert_eq!(points(&ed.doc), 3);
}

#[test]
fn a_circle_centred_on_another_layers_point_is_only_placed_there() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let mut tool = CircleTool::default();
    click(&mut ed, &mut tool, v(10.1, 0.1));
    click(&mut ed, &mut tool, v(15.0, 0.0));
    let center = ed
        .doc
        .iter()
        .find_map(|(_, e)| match e.kind {
            EntityKind::Circle { center, .. } => Some(center),
            _ => None,
        })
        .expect("a circle");
    assert_ne!(center, corner);
    assert_eq!(ed.doc.point(center).unwrap(), v(10.0, 0.0));
    // A closed shape is only placed on what it snaps to, on any layer (Anchor::own): no
    // relation either.
    assert!(coincidents(&ed).is_empty());
}

/// Two lines, one per layer: (0,0)-(10,0) on the first, (20,0)-(20,10) on the second.
fn two_layers(relations: bool) -> (Editor, EntityId, EntityId) {
    let (mut ed, first, _) = line_then_other_layer(relations);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(20.0, 0.0));
    click(&mut ed, &mut tool, v(20.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let second = last_line(&ed);
    (ed, first, second)
}

fn drag(ed: &mut Editor, from: DVec2, to: DVec2) {
    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(ed, &sel, from);
    sel.handle(Input::DragStart(grab, m), ed);
    let p = ptr(ed, &sel, to);
    sel.handle(Input::Drag(p), ed);
    sel.handle(Input::DragEnd(p, m), ed);
}

#[test]
fn a_point_dropped_on_another_layers_point_is_held_not_merged() {
    let (mut ed, first, second) = two_layers(true);
    let (_, corner) = ends(&ed, first);
    let (dropped, _) = ends(&ed, second);
    drag(&mut ed, v(20.0, 0.0), v(10.3, 0.2));
    assert_eq!(points(&ed.doc), 4, "nothing merged");
    assert!(ed.doc.contains(dropped));
    assert_eq!(ends(&ed, second).0, dropped);
    assert!(ed.doc.point(dropped).unwrap().distance(v(10.0, 0.0)) < 1e-6);
    let held = coincidents(&ed);
    assert!(
        held.contains(&(dropped, corner)) || held.contains(&(corner, dropped)),
        "held together: {held:?}"
    );
}

#[test]
fn a_point_dropped_on_another_layers_point_without_relations_is_only_placed() {
    let (mut ed, first, second) = two_layers(false);
    let (dropped, _) = ends(&ed, second);
    drag(&mut ed, v(20.0, 0.0), v(10.3, 0.2));
    assert_eq!(points(&ed.doc), 4, "nothing merged");
    assert!(ed.doc.point(dropped).unwrap().distance(v(10.0, 0.0)) < 1e-6);
    assert!(coincidents(&ed).is_empty());
    let _ = first;
}

#[test]
fn moving_one_layers_line_leaves_the_others_where_they_are() {
    // Drawn onto it without relations: nothing ties the two.
    let (mut ed, first, _) = line_then_other_layer(false);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.2, 0.1));
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let second = last_line(&ed);
    let (start, end) = ends(&ed, second);
    // Drag the first layer's line by its middle.
    drag(&mut ed, v(5.0, 0.0), v(5.0, -20.0));
    let (_, corner) = ends(&ed, first);
    assert!(ed.doc.point(corner).unwrap().distance(v(10.0, -20.0)) < 1e-6);
    assert_eq!(ed.doc.point(start).unwrap(), v(10.0, 0.0), "stays put");
    assert_eq!(ed.doc.point(end).unwrap(), v(10.0, 10.0));
}

#[test]
fn moving_one_layers_line_takes_the_other_along_only_through_the_relation() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.2, 0.1));
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let (start, end) = ends(&ed, last_line(&ed));
    drag(&mut ed, v(5.0, 0.0), v(5.0, -20.0));
    let (_, corner) = ends(&ed, first);
    let at = ed.doc.point(corner).unwrap();
    assert!(
        ed.doc.point(start).unwrap().distance(at) < 1e-6,
        "the relation holds them"
    );
    assert!(ed.doc.contains(end));
    assert_ne!(start, corner);
}

/// A pointer at `at` snapped onto the point `onto` itself, whichever of the points stacked
/// there the index would have offered (it picks between them by its own order).
fn ptr_onto(ed: &Editor, tool: &dyn Tool, at: DVec2, onto: EntityId) -> Pointer {
    let mut p = ptr(ed, tool, at);
    let mut snap = p.snap.expect("snapped");
    snap.entity = Some(onto);
    snap.at = ed.doc.point(onto).unwrap();
    p.snap = Some(snap);
    p
}

fn click_onto(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2, onto: EntityId) {
    let p = ptr_onto(ed, tool, at, onto);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

/// On the second layer, a line from (0,10) drawn onto the first layer's corner at (10,0).
/// Returns the editor, that corner and the second layer's own point there.
fn held_vertex(relations: bool) -> (Editor, EntityId, EntityId) {
    let (mut ed, first, _) = line_then_other_layer(relations);
    let (_, corner) = ends(&ed, first);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 10.0));
    click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
    tool.handle(Input::Finish, &mut ed);
    let (_, own) = ends(&ed, last_line(&ed));
    assert_ne!(own, corner);
    (ed, corner, own)
}

#[test]
fn a_second_stroke_onto_a_held_vertex_joins_its_layers_point() {
    for relations in [true, false] {
        let (mut ed, corner, own) = held_vertex(relations);
        let mut tool = LineTool::default();
        // Snapped onto the other layer's corner, not onto the point of this layer there.
        click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
        click(&mut ed, &mut tool, v(20.0, 10.0));
        tool.handle(Input::Finish, &mut ed);
        assert_eq!(ends(&ed, last_line(&ed)).0, own, "relations {relations}");
        assert_eq!(points(&ed.doc), 5, "relations {relations}");
        assert!(coincidents(&ed).len() <= 1);
    }
}

#[test]
fn a_point_dropped_on_a_held_vertex_joins_its_layers_point() {
    let (mut ed, corner, own) = held_vertex(true);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(30.0, 0.0));
    click(&mut ed, &mut tool, v(30.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let moved = last_line(&ed);
    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(&ed, &sel, v(30.0, 0.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    let to = ptr_onto(&ed, &sel, v(10.2, 0.1), corner);
    sel.handle(Input::Drag(to), &mut ed);
    assert!(sel.would_join(&ed, corner), "the marker says Join");
    sel.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(ends(&ed, moved).0, own, "merged into this layer's point");
    assert_eq!(points(&ed.doc), 5);
}

#[test]
fn a_chain_begun_on_another_layers_vertex_closes_on_its_own_start() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let mut tool = LineTool::default();
    click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
    click(&mut ed, &mut tool, v(10.0, 10.0));
    click(&mut ed, &mut tool, v(20.0, 10.0));
    click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
    assert!(tool.last.is_none(), "closed");
    // Two points on the first layer, three on the second (a closed triangle).
    assert_eq!(points(&ed.doc), 5);
}

#[test]
fn the_join_marker_shows_only_for_a_join_on_the_layer() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let line = LineTool::default();
    assert!(
        !line.would_join(&ed, corner),
        "another layer's point: no join"
    );
    let first_layer = ed.doc.get(first).unwrap().layer;
    ed.set_current_layer(first_layer);
    assert!(line.would_join(&ed, corner), "on its layer: a join");
    // A point of the second layer held to it: that one is joined.
    let (ed2, corner2, _) = held_vertex(true);
    assert!(line.would_join(&ed2, corner2));
}

#[test]
fn continuing_from_another_layers_point_without_relations_holds_nothing() {
    let (mut ed, first, _) = line_then_other_layer(false);
    let (_, corner) = ends(&ed, first);
    let mut tool = LineTool::default();
    tool.continue_from(corner, &mut ed);
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.handle(Input::Finish, &mut ed);
    let (start, _) = ends(&ed, last_line(&ed));
    assert_ne!(start, corner);
    assert!(coincidents(&ed).is_empty());
}

#[test]
fn a_tangent_arc_begun_on_another_layers_end_has_its_own_point() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let mut tool = TangentArcTool::default();
    click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
    click(&mut ed, &mut tool, v(20.0, 10.0));
    let (start, end) = ed
        .doc
        .iter()
        .find_map(|(_, e)| match e.kind {
            EntityKind::Arc { start, end, .. } => Some((start, end)),
            _ => None,
        })
        .expect("an arc");
    assert!(start != corner && end != corner, "no shared point");
    let held = coincidents(&ed);
    assert!(
        held.iter()
            .any(|&(a, b)| (a == corner && (b == start || b == end))
                || (b == corner && (a == start || a == end))),
        "held together: {held:?}"
    );
    assert_eq!(ed.doc.users_of(corner).count(), 1, "the first line alone");
}

#[test]
fn a_three_point_arc_from_another_layers_point_has_its_own_point() {
    let (mut ed, first, _) = line_then_other_layer(true);
    let (_, corner) = ends(&ed, first);
    let mut tool = ThreePointArcTool::default();
    click_onto(&mut ed, &mut tool, v(10.1, 0.1), corner);
    click(&mut ed, &mut tool, v(20.0, 10.0));
    click(&mut ed, &mut tool, v(18.0, 2.0));
    assert!(
        ed.doc
            .iter()
            .any(|(_, e)| matches!(e.kind, EntityKind::Arc { .. })),
        "an arc"
    );
    assert_eq!(ed.doc.users_of(corner).count(), 1, "the first line alone");
    let held = coincidents(&ed);
    assert!(
        held.iter().any(|&(a, b)| a == corner || b == corner),
        "held together: {held:?}"
    );
}
