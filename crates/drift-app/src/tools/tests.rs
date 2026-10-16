use drift_core::{Document, EntityKind};
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A snapped pointer at `at`, as the canvas would produce it.
fn ptr(ed: &Editor, tool: &dyn Tool, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    if tool.wants_snap() {
        p.snap = ed.index.snap(
            at,
            SnapOptions {
                tolerance: TOL,
                grid: None,
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

fn count(doc: &Document, pred: impl Fn(&EntityKind) -> bool) -> usize {
    doc.iter().filter(|(_, e)| pred(&e.kind)).count()
}

fn lines(doc: &Document) -> usize {
    count(doc, |k| matches!(k, EntityKind::Line { .. }))
}

fn points(doc: &Document) -> usize {
    count(doc, EntityKind::is_point)
}

#[test]
fn polyline_shares_points_and_closes_on_start() {
    let mut ed = Editor::default();
    let mut tool = LineTool::default();
    for at in [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.1, 0.2)] {
        click(&mut ed, &mut tool, at);
    }
    // The last click snapped to the start point and closed a triangle.
    assert_eq!(lines(&ed.doc), 3);
    assert_eq!(points(&ed.doc), 3);
    assert!(tool.last.is_none(), "chain finished after closing");
    // Each segment is its own undo step.
    assert!(ed.undo());
    assert_eq!(lines(&ed.doc), 2);
}

fn line_relations(ed: &Editor) -> Vec<Constraint> {
    ed.doc
        .constraints()
        .map(|(_, c)| *c)
        .filter(|c| {
            matches!(
                c,
                Constraint::Horizontal { .. } | Constraint::Vertical { .. }
            )
        })
        .collect()
}

#[test]
fn axis_locks_apply_per_segment() {
    let mut ed = with_auto_relations();
    let mut tool = LineTool::default();
    assert!(
        !tool.lock_axis(Axis::X),
        "nothing to lock before the first point"
    );
    click(&mut ed, &mut tool, v(0.0, 0.0));
    assert!(tool.lock_axis(Axis::X));
    assert_eq!(tool.axis_lock(), Some((v(0.0, 0.0), Axis::X)));
    // Far off the axis: the end is projected onto it.
    click(&mut ed, &mut tool, v(30.0, 17.0));
    assert_eq!(tool.axis_lock(), None, "the lock ends with the segment");
    assert!(tool.lock_axis(Axis::Y));
    click(&mut ed, &mut tool, v(12.0, -20.0));
    // A third, unlocked segment gets no relation.
    click(&mut ed, &mut tool, v(50.0, 3.0));
    tool.handle(Input::Finish, &mut ed);
    let ends: Vec<_> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Line { a, b } => Some((ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(ends[0], (v(0.0, 0.0), v(30.0, 0.0)));
    assert_eq!(ends[1], (v(30.0, 0.0), v(30.0, -20.0)));
    assert_eq!(ends[2].1, v(50.0, 3.0));
    assert!(matches!(
        line_relations(&ed)[..],
        [Constraint::Horizontal { .. }, Constraint::Vertical { .. }]
    ));
}

#[test]
fn an_axis_lock_adds_no_relation_when_automatic_relations_are_off() {
    let mut ed = Editor::default();
    assert!(!ed.auto_relations);
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    assert!(tool.lock_axis(Axis::X));
    click(&mut ed, &mut tool, v(30.0, 17.0));
    assert!(tool.lock_axis(Axis::Y));
    // A typed length along the locked axis, too.
    let p = ptr(&ed, &tool, v(34.0, 9.0));
    tool.set_typed([Some(25.0), None]);
    tool.commit_typed(&p, &mut ed);
    tool.handle(Input::Finish, &mut ed);
    // The segments are still drawn exactly along their axes...
    let ends: Vec<_> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Line { a, b } => Some((ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(ends[0], (v(0.0, 0.0), v(30.0, 0.0)));
    assert_eq!(ends[1], (v(30.0, 0.0), v(30.0, 25.0)));
    // ...but nothing holds them there: only the typed length is recorded.
    assert!(line_relations(&ed).is_empty());
}

#[test]
fn axis_lock_toggles_and_works_with_typed_lengths() {
    let mut ed = with_auto_relations();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.0, 10.0));
    tool.lock_axis(Axis::Y);
    tool.lock_axis(Axis::Y);
    assert_eq!(tool.axis_lock(), None, "pressing the key again unlocks");
    tool.lock_axis(Axis::X);
    tool.lock_axis(Axis::Y);
    assert_eq!(
        tool.axis_lock().map(|l| l.1),
        Some(Axis::Y),
        "the other key switches"
    );
    // Pointer below and to the side: a typed length goes straight down.
    let p = ptr(&ed, &tool, v(14.0, 2.0));
    tool.set_typed([Some(25.0), None]);
    tool.commit_typed(&p, &mut ed);
    let b = ed
        .doc
        .iter()
        .find_map(|(_, e)| match e.kind {
            EntityKind::Line { b, .. } => Some(b),
            _ => None,
        })
        .unwrap();
    assert_eq!(ed.doc.point(b).unwrap(), v(10.0, -15.0));
    assert!(matches!(
        line_relations(&ed)[..],
        [Constraint::Vertical { .. }]
    ));
}

#[test]
fn a_locked_end_lines_up_with_a_snapped_point() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let marker = ed.doc.add_point(v(40.0, 25.0));
    ed.commit("Point");
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    tool.lock_axis(Axis::X);
    let p = ptr(&ed, &tool, v(40.1, 25.1)); // snaps to the marker point
    assert_eq!(p.pos(), v(40.0, 0.0));
    assert_eq!(p.snapped_point(), None, "an off-axis point isn't reused");
    assert_eq!(p.guide.unwrap().from[1], Some(v(40.0, 25.0)));
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    assert!(ed.doc.contains(marker));
    assert_eq!(points(&ed.doc), 3);
}

/// A selected line from (0, 0) to (10, 0) and an unselected one ending at (30, 20):
/// (editor, selected line's end points).
fn grab_fixture() -> (Editor, [EntityId; 2]) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, 0.0));
    let b = ed.doc.add_point(v(10.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    let c = ed.doc.add_point(v(50.0, 50.0));
    let d = ed.doc.add_point(v(30.0, 20.0));
    ed.doc.add_line(c, d).unwrap();
    ed.commit("Setup");
    ed.selection = [line].into();
    (ed, [a, b])
}

fn grab(ed: &mut Editor, tool: &mut SelectTool, at: DVec2) {
    let p = ptr(ed, tool, at);
    assert!(tool.key(ToolKey::G, Some(&p), ed));
    assert!(tool.is_modal());
}

fn hover(ed: &mut Editor, tool: &mut SelectTool, at: DVec2) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::Hover(p), ed);
}

fn ends(ed: &Editor, [a, b]: [EntityId; 2]) -> [DVec2; 2] {
    [ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()]
}

#[test]
fn grab_follows_the_pointer_and_confirms_in_one_undo_step() {
    let (mut ed, pts) = grab_fixture();
    let mut tool = SelectTool::default();
    grab(&mut ed, &mut tool, v(5.0, 0.0));
    hover(&mut ed, &mut tool, v(8.0, 3.0));
    assert_eq!(ends(&ed, pts), [v(3.0, 3.0), v(13.0, 3.0)], "live preview");
    click(&mut ed, &mut tool, v(8.0, 3.0));
    assert!(!tool.is_modal());
    assert_eq!(ends(&ed, pts), [v(3.0, 3.0), v(13.0, 3.0)]);
    assert!(ed.undo());
    assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
}

#[test]
fn grab_takes_axis_locks_and_typed_offsets() {
    let (mut ed, pts) = grab_fixture();
    let mut tool = SelectTool::default();
    grab(&mut ed, &mut tool, v(5.0, 0.0));
    assert!(tool.key(ToolKey::Y, None, &mut ed));
    assert_eq!(tool.fields(), ["Distance"]);
    hover(&mut ed, &mut tool, v(9.0, 7.0));
    assert_eq!(ends(&ed, pts)[0], v(0.0, 7.0), "locked to Y");
    tool.set_typed([Some(-4.25), None]);
    let p = ptr(&ed, &tool, v(9.0, 7.0));
    tool.commit_typed(&p, &mut ed);
    assert_eq!(ends(&ed, pts), [v(0.0, -4.25), v(10.0, -4.25)]);

    // Unlocked: dX and dY.
    grab(&mut ed, &mut tool, v(5.0, 0.0));
    assert_eq!(tool.fields(), ["dX", "dY"]);
    tool.set_typed([Some(0.1), Some(0.2)]);
    let p = ptr(&ed, &tool, v(40.0, 40.0));
    tool.commit_typed(&p, &mut ed);
    assert_eq!(ends(&ed, pts)[1], v(10.0 + 0.1, -4.25 + 0.2));
}

#[test]
fn grab_from_a_picked_base_point_lands_exactly_on_a_snap() {
    let (mut ed, pts) = grab_fixture();
    let mut tool = SelectTool::default();
    grab(&mut ed, &mut tool, v(4.0, 1.0));
    hover(&mut ed, &mut tool, v(20.0, 9.0));
    assert!(tool.key(ToolKey::B, None, &mut ed));
    // Picking the base point puts the geometry back and can snap to the selection's own end.
    hover(&mut ed, &mut tool, v(10.1, 0.1));
    assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
    click(&mut ed, &mut tool, v(10.1, 0.1));
    assert!(tool.is_modal(), "picking the base doesn't place the move");
    // Moving snaps to the other line's end, but not to the moving line.
    hover(&mut ed, &mut tool, v(30.2, 19.9));
    click(&mut ed, &mut tool, v(30.2, 19.9));
    assert_eq!(ends(&ed, pts), [v(20.0, 20.0), v(30.0, 20.0)]);
}

#[test]
fn cancelling_a_grab_restores_the_geometry() {
    let (mut ed, pts) = grab_fixture();
    let mut tool = SelectTool::default();
    grab(&mut ed, &mut tool, v(5.0, 0.0));
    hover(&mut ed, &mut tool, v(25.0, 13.0));
    assert!(tool.cancel(&mut ed));
    assert!(!tool.is_modal());
    assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
    // The selection survives the cancel.
    assert!(!ed.selection.is_empty());
    // No move was recorded: one undo removes the setup itself.
    assert!(ed.undo());
    assert_eq!(ed.doc.iter().count(), 0);
    // G with nothing selected only explains.
    let p = ptr(&ed, &tool, v(0.0, 0.0));
    assert!(tool.key(ToolKey::G, Some(&p), &mut ed));
    assert!(!tool.is_modal());
    assert!(ed.notice.take().is_some());
}

/// The single arc in the document: (centre, start, end) positions.
fn the_arc(ed: &Editor) -> (EntityId, [DVec2; 3]) {
    let arcs: Vec<_> = ed
        .doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Arc { center, start, end } => {
                Some((id, [center, start, end].map(|q| ed.doc.point(q).unwrap())))
            }
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 1, "expected one arc");
    arcs[0]
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-9
}

#[test]
fn three_point_arc_bulges_through_the_third_point() {
    for (third, center, start, end) in [
        // Above the chord: counter-clockwise from the end back to the start.
        (v(10.0, 5.0), v(10.0, -7.5), v(20.0, 0.0), v(0.0, 0.0)),
        // Below: counter-clockwise from the start.
        (v(10.0, -5.0), v(10.0, 7.5), v(0.0, 0.0), v(20.0, 0.0)),
    ] {
        let mut ed = Editor::default();
        let mut tool = ThreePointArcTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(20.0, 0.0));
        click(&mut ed, &mut tool, third);
        let (_, [c, s, e]) = the_arc(&ed);
        assert!(
            close(c, center) && close(s, start) && close(e, end),
            "{c} {s} {e}"
        );
        assert!(!tool.is_busy());
    }
}

#[test]
fn three_point_arc_takes_typed_values_and_refuses_collinear_points() {
    let mut ed = with_auto_relations();
    let mut tool = ThreePointArcTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let p = ptr(&ed, &tool, v(3.0, 1.0));
    tool.set_typed([Some(20.0), Some(0.0)]);
    tool.commit_typed(&p, &mut ed);
    // Collinear third point: refused, still waiting.
    click(&mut ed, &mut tool, v(40.0, 0.0));
    assert!(ed.notice.take().is_some());
    assert!(tool.is_busy());
    // Typed radius, bulging towards the pointer (above).
    let p = ptr(&ed, &tool, v(8.0, 30.0));
    tool.set_typed([Some(12.5), None]);
    tool.commit_typed(&p, &mut ed);
    let (arc, [c, ..]) = the_arc(&ed);
    assert!(close(c, v(10.0, -7.5)), "{c}");
    assert!(ed.doc.constraints().any(|(_, k)| *k
        == Constraint::Radius {
            curve: arc,
            value: 12.5
        }));
}

/// A line from (0, 0) to (10, 0): (editor, its end point).
fn line_end_fixture() -> (Editor, EntityId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, 0.0));
    let b = ed.doc.add_point(v(10.0, 0.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    (ed, b)
}

fn tangents(ed: &Editor) -> usize {
    ed.doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Tangent { .. }))
        .count()
}

#[test]
fn tangent_arcs_continue_lines_and_chain() {
    let (mut ed, end) = line_end_fixture();
    ed.auto_relations = true;
    let mut tool = TangentArcTool::default();
    click(&mut ed, &mut tool, v(10.1, 0.1)); // snaps to the line's end
    assert_eq!(tool.chain_end(), Some(end));
    click(&mut ed, &mut tool, v(20.0, 10.0));
    let (_, [c, s, e]) = the_arc(&ed);
    // Leaves along +X, turning left: a quarter circle about (10, 10).
    assert!(close(c, v(10.0, 10.0)) && close(s, v(10.0, 0.0)) && close(e, v(20.0, 10.0)));
    assert_eq!(tangents(&ed), 1);
    // Chained: the next arc leaves the first one's end going up, and turns right.
    click(&mut ed, &mut tool, v(30.0, 20.0));
    let centers: Vec<DVec2> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Arc { center, .. } => Some(ed.doc.point(center).unwrap()),
            _ => None,
        })
        .collect();
    assert!(
        centers.iter().any(|c| close(*c, v(30.0, 10.0))),
        "{centers:?}"
    );
    assert_eq!(tangents(&ed), 2);
    assert!(ed.analysis().conflicts.is_empty());
    tool.handle(Input::Finish, &mut ed);
    assert!(!tool.is_busy());
}

#[test]
fn tangent_arc_takes_a_typed_radius_and_sweep() {
    let (mut ed, _) = line_end_fixture();
    ed.auto_relations = true;
    let mut tool = TangentArcTool::default();
    click(&mut ed, &mut tool, v(10.0, 0.0));
    let p = ptr(&ed, &tool, v(14.0, 3.0));
    tool.set_typed([Some(5.0), Some(-90.0)]); // right turn
    tool.commit_typed(&p, &mut ed);
    let (arc, [c, s, e]) = the_arc(&ed);
    assert!(close(c, v(10.0, -5.0)), "{c}");
    // Clockwise from the line's end, so stored counter-clockwise from the far end.
    assert!(close(s, v(15.0, -5.0)) && close(e, v(10.0, 0.0)), "{s} {e}");
    assert!(ed.doc.constraints().any(|(_, k)| *k
        == Constraint::Radius {
            curve: arc,
            value: 5.0
        }));
}

#[test]
fn tangent_arc_needs_a_curve_end_to_start_from() {
    let mut ed = Editor::default();
    ed.doc.begin();
    ed.doc.add_point(v(0.0, 0.0));
    ed.commit("Point");
    let mut tool = TangentArcTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    assert!(!tool.is_busy());
    assert!(ed.notice.take().is_some());
}

#[test]
fn starting_a_line_on_an_existing_end_point_connects_to_it() {
    let mut ed = Editor::default();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    click(&mut ed, &mut tool, v(10.2, 0.1)); // snaps to the end point
    click(&mut ed, &mut tool, v(10.0, 5.0));
    assert_eq!(lines(&ed.doc), 2);
    assert_eq!(points(&ed.doc), 3);
}

#[test]
fn rectangle_is_four_connected_lines_in_one_undo_step() {
    let mut ed = Editor::default();
    let mut tool = RectangleTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(30.0, 20.0));
    assert_eq!((lines(&ed.doc), points(&ed.doc)), (4, 4));
    assert_eq!(ed.bounds().max, v(30.0, 20.0));
    assert!(ed.undo());
    assert!(ed.doc.is_empty());
}

#[test]
fn press_drag_release_draws_a_circle() {
    let mut ed = Editor::default();
    let mut tool = CircleTool::default();
    let m = Modifiers::default();
    let start = ptr(&ed, &tool, v(5.0, 5.0));
    tool.handle(Input::DragStart(start, m), &mut ed);
    let end = ptr(&ed, &tool, v(8.0, 9.0));
    tool.handle(Input::Drag(end), &mut ed);
    tool.handle(Input::DragEnd(end, m), &mut ed);
    let (_, circle) = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .unwrap();
    assert_eq!(
        circle.kind,
        EntityKind::Circle {
            center: drift_core::EntityId(1),
            radius: 5.0
        }
    );
}

#[test]
fn arc_direction_follows_the_pointer() {
    for (path, expect_ccw) in [
        (vec![v(7.0, 7.0), v(0.0, 10.0)], true), // up and over: counter-clockwise
        (vec![v(7.0, -7.0), v(0.0, -10.0)], false), // down: clockwise
    ] {
        let mut ed = Editor::default();
        let mut tool = ArcTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(10.0, 0.0));
        for at in &path[..path.len() - 1] {
            let p = ptr(&ed, &tool, *at);
            tool.handle(Input::Hover(p), &mut ed);
        }
        click(&mut ed, &mut tool, *path.last().unwrap());
        let (id, _) = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Arc { .. }))
            .unwrap();
        let Some(Shape::Arc { start, sweep, .. }) = ed.doc.shape(id) else {
            panic!()
        };
        assert!((sweep - PI / 2.0).abs() < 1e-9, "quarter arc, got {sweep}");
        let expected_start = if expect_ccw { 0.0 } else { -PI / 2.0 };
        assert!((start - expected_start).abs() < 1e-9, "start {start}");
    }
}

#[test]
fn select_click_box_and_move() {
    let mut ed = Editor::default();
    let mut rect = RectangleTool::default();
    click(&mut ed, &mut rect, v(0.0, 0.0));
    click(&mut ed, &mut rect, v(10.0, 10.0));
    let mut sel = SelectTool::default();
    let m = Modifiers::default();

    // Click the bottom edge.
    click(&mut ed, &mut sel, v(3.0, 0.1));
    assert_eq!(ed.selection.len(), 1);

    // Window-select everything (drag right).
    let a = ptr(&ed, &sel, v(-1.0, -1.0));
    sel.handle(Input::DragStart(a, m), &mut ed);
    let b = ptr(&ed, &sel, v(11.0, 11.0));
    sel.handle(Input::Drag(b), &mut ed);
    sel.handle(Input::DragEnd(b, m), &mut ed);
    assert_eq!(ed.selection.len(), 4);

    // Drag an edge: the whole selection moves as one undo step.
    let grab = ptr(&ed, &sel, v(5.0, 0.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    let to = ptr(&ed, &sel, v(8.0, 4.0));
    sel.handle(Input::Drag(to), &mut ed);
    sel.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(ed.bounds().min, v(3.0, 4.0));
    assert!(ed.undo());
    assert_eq!(ed.bounds().min, v(0.0, 0.0));
}

#[test]
fn dragging_an_end_point_onto_another_joins_them() {
    let mut ed = Editor::default();
    let mut line = LineTool::default();
    click(&mut ed, &mut line, v(0.0, 0.0));
    click(&mut ed, &mut line, v(10.0, 0.0));
    line.handle(Input::Finish, &mut ed);
    click(&mut ed, &mut line, v(20.0, 0.0));
    click(&mut ed, &mut line, v(20.0, 10.0));
    assert_eq!(points(&ed.doc), 4);

    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(&ed, &sel, v(20.0, 0.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    let to = ptr(&ed, &sel, v(10.3, 0.2)); // snaps onto the first line's end
    sel.handle(Input::Drag(to), &mut ed);
    sel.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(points(&ed.doc), 3, "points merged");
    assert!(ed.undo());
    assert_eq!(points(&ed.doc), 4);
}

#[test]
fn escape_during_move_restores_geometry() {
    let mut ed = Editor::default();
    let mut tool = CircleTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(5.0, 0.0));
    let before = ed.doc.clone();
    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(&ed, &sel, v(5.0, 0.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    sel.handle(Input::Drag(ptr(&ed, &sel, v(15.0, 3.0))), &mut ed);
    assert_ne!(ed.doc, before);
    assert!(sel.cancel(&mut ed));
    assert_eq!(ed.doc, before);
    assert!(!ed.history.can_undo() || ed.history.undo_label() == Some("Circle"));
}

// ---- M3: automatic relations, typed input, Smart Dimension ----

fn constraints(ed: &Editor) -> Vec<Constraint> {
    ed.doc.constraints().map(|(_, c)| *c).collect()
}

fn with_auto_relations() -> Editor {
    Editor::new(Document::new())
}

#[test]
fn near_horizontal_line_snaps_and_gets_a_relation() {
    let mut ed = with_auto_relations();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.3)); // within tolerance of horizontal
    let (_, line) = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .unwrap();
    let EntityKind::Line { a, b } = line.kind else {
        unreachable!()
    };
    assert_eq!(ed.doc.point(b).unwrap().y, ed.doc.point(a).unwrap().y);
    assert!(matches!(
        constraints(&ed)[..],
        [Constraint::Horizontal { .. }]
    ));
    // Without automatic relations the line is still drawn straight, but unconstrained.
    let mut ed = Editor::default();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.3));
    assert_eq!(ed.doc.constraint_count(), 0);
}

#[test]
fn typed_length_and_angle_make_an_exact_dimensioned_line() {
    let mut ed = with_auto_relations();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    tool.set_typed([Some(25.0), Some(90.0)]);
    let p = ptr(&ed, &tool, v(3.0, 3.0));
    tool.commit_typed(&p, &mut ed);
    assert_eq!(lines(&ed.doc), 1);
    let end = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Point(p) if p != v(0.0, 0.0) => Some(p),
            _ => None,
        })
        .next()
        .unwrap();
    assert_eq!(end, v(0.0, 25.0));
    assert!(
        ed.doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
    );
    let cs = constraints(&ed);
    assert!(cs.iter().any(|c| matches!(c, Constraint::Vertical { .. })));
    assert!(
        cs.iter()
            .any(|c| matches!(c, Constraint::Distance { value, .. } if *value == 25.0))
    );
    // The chain continues from the new end.
    assert!(tool.is_busy());
}

#[test]
fn typed_coordinates_place_points_exactly() {
    let mut ed = Editor::default();
    let mut tool = PointTool::default();
    tool.set_typed([Some(12.5), Some(-3.25)]);
    let p = ptr(&ed, &tool, v(100.0, 100.0));
    tool.commit_typed(&p, &mut ed);
    let (_, point) = ed.doc.iter().next().unwrap();
    assert_eq!(point.kind, EntityKind::Point(v(12.5, -3.25)));
}

#[test]
fn rectangle_is_constrained_square_and_typed_size_is_dimensioned() {
    let mut ed = with_auto_relations();
    let mut tool = RectangleTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    tool.set_typed([Some(40.0), Some(25.0)]);
    let p = ptr(&ed, &tool, v(10.0, 10.0));
    tool.commit_typed(&p, &mut ed);
    assert_eq!(ed.bounds().max, v(40.0, 25.0));
    let cs = constraints(&ed);
    assert_eq!(
        cs.iter()
            .filter(|c| matches!(c, Constraint::Horizontal { .. }))
            .count(),
        2
    );
    assert_eq!(
        cs.iter()
            .filter(|c| matches!(c, Constraint::Vertical { .. }))
            .count(),
        2
    );
    assert_eq!(
        cs.iter()
            .filter(|c| matches!(c, Constraint::Distance { .. }))
            .count(),
        2
    );
    // Only the position is left free.
    assert_eq!(ed.analysis().dof, Some(2));
}

#[test]
fn smart_dimension_picks_the_type_from_the_label_position() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, 0.0));
    let b = ed.doc.add_point(v(10.0, 5.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    let mut tool = DimensionTool::default();
    let on_line = v(2.0, 1.0); // on the line, away from its midpoint

    // Label above the line, within its x range: horizontal distance.
    click(&mut ed, &mut tool, on_line);
    click(&mut ed, &mut tool, v(5.0, 20.0));
    assert!(matches!(
        constraints(&ed)[..],
        [Constraint::HorizontalDistance { value, .. }] if value == 10.0
    ));
    let id = ed.pending_edit.take().expect("value editor requested");
    assert_eq!(ed.doc.placement(id), Some(v(0.0, 17.5)));

    // Label to the right, within its y range: vertical distance.
    click(&mut ed, &mut tool, on_line);
    click(&mut ed, &mut tool, v(30.0, 2.0));
    assert!(matches!(
        constraints(&ed)[1],
        Constraint::VerticalDistance { value, .. } if value == 5.0
    ));

    // Now the length is implied: an aligned dimension is refused as redundant.
    click(&mut ed, &mut tool, on_line);
    click(&mut ed, &mut tool, v(12.0, -4.0));
    assert_eq!(ed.doc.constraint_count(), 2);
    assert!(
        ed.notice
            .take()
            .is_some_and(|n| n.contains("already implied"))
    );
    // ...and offered as a driven dimension instead.
    let (driven, placement) = ed.pending_driven.take().expect("driven prompt");
    assert!(matches!(driven, Constraint::Distance { .. }));
    let id = ed.add_driven_dimension(driven, placement).unwrap();
    assert!(ed.doc.is_driven(id));
    assert!((driven.value().unwrap() - 125f64.sqrt()).abs() < 1e-9);
}

#[test]
fn smart_dimension_angles_diameters_and_point_line_distances() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let o = ed.doc.add_point(v(0.0, 0.0));
    let x = ed.doc.add_point(v(20.0, 0.0));
    let y = ed.doc.add_point(v(10.0, 10.0));
    let l1 = ed.doc.add_line(o, x).unwrap();
    let l2 = ed.doc.add_line(o, y).unwrap();
    let c = ed.doc.add_point(v(50.0, 0.0));
    let circle = ed.doc.add_circle(c, 4.0).unwrap();
    let q = ed.doc.add_point(v(30.0, 7.0));
    ed.commit("Setup");
    let mut tool = DimensionTool::default();

    click(&mut ed, &mut tool, v(14.0, 0.0)); // on l1
    click(&mut ed, &mut tool, v(7.0, 7.0)); // on l2
    click(&mut ed, &mut tool, v(8.0, 3.0)); // place
    assert!(constraints(&ed).iter().any(|c| matches!(c,
        Constraint::Angle { a, b, degrees } if *a == l1 && *b == l2 && (degrees - 45.0).abs() < 1e-9)));

    click(&mut ed, &mut tool, v(54.0, 0.0)); // on the circle
    click(&mut ed, &mut tool, v(60.0, 6.0));
    assert!(constraints(&ed).iter().any(|cc| matches!(cc,
        Constraint::Diameter { curve, value } if *curve == circle && *value == 8.0)));

    click(&mut ed, &mut tool, v(30.0, 7.0)); // the point
    click(&mut ed, &mut tool, v(16.0, 0.0)); // l1
    click(&mut ed, &mut tool, v(35.0, 3.0));
    assert!(constraints(&ed).iter().any(|cc| matches!(cc,
        Constraint::PointLineDistance { point, line, value } if *point == q && *line == l1 && *value == 7.0)));
}

// ---- Midpoints ----

/// A horizontal line 0..20 and a separate slanted line; returns (base, base midpoint pos,
/// slanted line's end points).
fn midpoint_fixture() -> (Editor, EntityId, [EntityId; 2]) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, 0.0));
    let b = ed.doc.add_point(v(20.0, 0.0));
    let base = ed.doc.add_line(a, b).unwrap();
    let c = ed.doc.add_point(v(13.0, 6.0));
    let d = ed.doc.add_point(v(25.0, 14.0));
    ed.doc.add_line(c, d).unwrap();
    ed.commit("Setup");
    (ed, base, [c, d])
}

#[test]
fn midpoints_can_be_hovered_picked_and_constrained() {
    let (mut ed, base, [c, _]) = midpoint_fixture();
    let mut sel = SelectTool::default();
    let near_mid = v(10.1, 0.1);
    sel.handle(Input::Hover(ptr(&ed, &sel, near_mid)), &mut ed);
    assert_eq!(ed.hovered_midpoint, Some(base));
    assert_eq!(ed.hovered, None);
    click(&mut ed, &mut sel, near_mid);
    assert_eq!(ed.selected_midpoints, [base].into());
    // Shift-click the other line's end point.
    let p = ptr(&ed, &sel, v(13.1, 6.1));
    sel.handle(
        Input::Click(
            p,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        ),
        &mut ed,
    );
    assert_eq!(ed.selection, [c].into());
    let midpoint = ed
        .relation_options()
        .into_iter()
        .find(|o| o.label == "Midpoint")
        .expect("Midpoint offered");
    ed.add_constraints("Midpoint", &midpoint.constraints)
        .unwrap();
    // Nothing is fixed, so the smallest change moves both: check the relation holds.
    let EntityKind::Line { a, b } = ed.doc.get(base).unwrap().kind else {
        unreachable!()
    };
    let mid = (ed.doc.point(a).unwrap() + ed.doc.point(b).unwrap()) * 0.5;
    assert!((ed.doc.point(c).unwrap() - mid).length() < 1e-12);
    // Esc clears both kinds of selection.
    assert!(sel.cancel(&mut ed));
    assert!(ed.selection.is_empty() && ed.selected_midpoints.is_empty());
}

#[test]
fn point_and_line_selection_offers_midpoint() {
    let (mut ed, base, [c, _]) = midpoint_fixture();
    ed.selection = [base, c].into();
    assert!(ed.relation_options().iter().any(|o| o.label == "Midpoint"));
}

#[test]
fn starting_a_line_on_a_midpoint_adds_the_relation() {
    let (mut ed, base, _) = midpoint_fixture();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(10.1, 0.2)); // snaps to the base's midpoint
    click(&mut ed, &mut tool, v(10.0, -15.0));
    let Some(Constraint::Midpoint { point, line }) = ed
        .doc
        .constraints()
        .map(|(_, c)| *c)
        .find(|c| matches!(c, Constraint::Midpoint { .. }))
    else {
        panic!("no midpoint relation")
    };
    assert_eq!(line, base);
    // Dragging the base (weighting its points, as the select tool does) carries the new
    // line's start along with its midpoint.
    let EntityKind::Line { a, b } = ed.doc.get(base).unwrap().kind else {
        unreachable!()
    };
    let dragged: Vec<Param> = [a, b].into_iter().flat_map(Param::point).collect();
    ed.doc.begin();
    assert!(ed.drag(&[base], v(4.0, 0.0), &dragged));
    ed.commit("Move");
    let mid = (ed.doc.point(a).unwrap() + ed.doc.point(b).unwrap()) * 0.5;
    assert!((mid - v(14.0, 0.0)).length() < 1e-5, "{mid}");
    assert!((ed.doc.point(point).unwrap() - mid).length() < 1e-9);
}

/// A circle of radius 10 at the origin: (editor, circle).
fn circle_fixture() -> (Editor, EntityId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let circle = ed.doc.add_circle(c, 10.0).unwrap();
    ed.commit("Circle");
    (ed, circle)
}

fn on_curve_relations(ed: &Editor) -> Vec<(EntityId, EntityId)> {
    ed.doc
        .constraints()
        .filter_map(|(_, c)| match *c {
            Constraint::PointOnCurve { point, curve } => Some((point, curve)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_line_ending_on_a_curve_stays_on_it() {
    let (mut ed, circle) = circle_fixture();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(30.0, 0.0));
    click(&mut ed, &mut tool, v(6.1, 8.1)); // near (6, 8) on the circle
    tool.handle(Input::Finish, &mut ed);
    let [(point, curve)] = on_curve_relations(&ed)[..] else {
        panic!("expected one on-curve relation")
    };
    assert_eq!(curve, circle);
    assert!((ed.doc.point(point).unwrap().length() - 10.0).abs() < 1e-9);
    // Growing the circle carries the end along.
    ed.doc.begin();
    let _ = ed.doc.add_constraint(Constraint::Radius {
        curve: circle,
        value: 15.0,
    });
    ed.commit("Radius");
    let EntityKind::Circle { center, radius } = ed.doc.get(circle).unwrap().kind else {
        unreachable!()
    };
    assert!((radius - 15.0).abs() < 1e-9);
    let from_centre = ed.doc.point(point).unwrap() - ed.doc.point(center).unwrap();
    assert!((from_centre.length() - 15.0).abs() < 1e-9);

    // Without automatic relations the point is only placed there.
    let (mut ed, _) = circle_fixture();
    ed.auto_relations = false;
    let mut tool = LineTool::default();
    let mut p = ptr(&ed, &tool, v(30.0, 0.0));
    p.relations = false;
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    let mut p = ptr(&ed, &tool, v(6.1, 8.1));
    p.relations = false;
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    assert!(on_curve_relations(&ed).is_empty());
}

#[test]
fn dropping_a_point_on_a_curve_constrains_it() {
    let (mut ed, circle) = circle_fixture();
    ed.doc.begin();
    let p = ed.doc.add_point(v(20.0, 20.0));
    ed.commit("Point");
    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(&ed, &sel, v(20.0, 20.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    let to = ptr(&ed, &sel, v(-6.1, 8.1));
    assert_eq!(to.on_curve(), Some(circle));
    sel.handle(Input::Drag(to), &mut ed);
    sel.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(on_curve_relations(&ed), [(p, circle)]);
    assert!((ed.doc.point(p).unwrap().length() - 10.0).abs() < 1e-9);
}

#[test]
fn dropping_an_end_point_on_a_midpoint_constrains_it() {
    let (mut ed, base, [c, _]) = midpoint_fixture();
    let mut sel = SelectTool::default();
    let m = Modifiers::default();
    let grab = ptr(&ed, &sel, v(13.0, 6.0));
    sel.handle(Input::DragStart(grab, m), &mut ed);
    let to = ptr(&ed, &sel, v(10.2, 0.2));
    assert_eq!(to.midpoint_of(), Some(base));
    sel.handle(Input::Drag(to), &mut ed);
    sel.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(ed.doc.point(c).unwrap(), v(10.0, 0.0));
    assert!(ed.doc.constraints().any(|(_, k)| *k
        == Constraint::Midpoint {
            point: c,
            line: base
        }));
}

#[test]
fn smart_dimension_measures_to_midpoints() {
    // Base 0..20 on the x axis (midpoint (10, 0)); a second line from (13, 6) to (25, 14),
    // midpoint (19, 10).
    let (mut ed, base, [c, _]) = midpoint_fixture();
    let other = ed
        .doc
        .iter()
        .find(|(id, e)| matches!(e.kind, EntityKind::Line { .. }) && *id != base)
        .map(|(id, _)| id)
        .unwrap();
    let mut tool = DimensionTool::default();
    let base_mid = v(10.1, 0.1);

    // Midpoint -> point, label above both (inside their x range): horizontal distance.
    click(&mut ed, &mut tool, base_mid);
    assert_eq!(
        ed.selected_midpoints,
        [base].into(),
        "picked midpoint is marked"
    );
    click(&mut ed, &mut tool, v(13.1, 6.1));
    click(&mut ed, &mut tool, v(11.0, 20.0));
    assert!(constraints(&ed).contains(&Constraint::HorizontalDistance {
        a: base,
        b: c,
        value: 3.0
    }));
    assert!(
        ed.selected_midpoints.is_empty(),
        "pick marks cleared after placing"
    );

    // Midpoint -> midpoint, aligned.
    click(&mut ed, &mut tool, base_mid);
    click(&mut ed, &mut tool, v(19.05, 10.05));
    click(&mut ed, &mut tool, v(25.0, -5.0)); // outside both ranges: aligned
    let expected = v(10.0, 0.0).distance(v(19.0, 10.0));
    assert!(constraints(&ed).iter().any(|k| matches!(k,
        Constraint::Distance { a, b, value } if *a == base && *b == other
            && (value - expected).abs() < 1e-12)));

    // Midpoint -> another line: perpendicular distance.
    click(&mut ed, &mut tool, v(19.05, 10.05)); // other's midpoint
    click(&mut ed, &mut tool, v(4.0, 0.0)); // the base line (away from its midpoint)
    click(&mut ed, &mut tool, v(25.0, 5.0));
    assert!(constraints(&ed).iter().any(|k| matches!(k,
        Constraint::PointLineDistance { point, line, value } if *point == other
            && *line == base && (value - 10.0).abs() < 1e-12)));
}

#[test]
fn panel_offers_distances_to_selected_midpoints() {
    let (mut ed, base, [c, _]) = midpoint_fixture();
    ed.selected_midpoints = [base].into();
    ed.selection = [c].into();
    let labels: Vec<_> = ed.dimension_options().iter().map(|o| o.label).collect();
    assert_eq!(
        labels,
        ["Distance", "Horizontal distance", "Vertical distance"]
    );
    let distance = &ed.dimension_options()[0].constraints[0];
    assert_eq!(distance.value(), Some(v(10.0, 0.0).distance(v(13.0, 6.0))));
}

// ----- Modify tools -------------------------------------------------------------------------

/// Lines between point pairs, committed as one step.
fn modify_fixture(lines: &[(DVec2, DVec2)]) -> (Editor, Vec<EntityId>) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let ids = lines
        .iter()
        .map(|(a, b)| {
            let pa = ed.doc.add_point(*a);
            let pb = ed.doc.add_point(*b);
            ed.doc.add_line(pa, pb).unwrap()
        })
        .collect();
    ed.commit("Setup");
    (ed, ids)
}

fn line_ends(ed: &Editor, id: EntityId) -> (DVec2, DVec2) {
    match ed.doc.shape(id) {
        Some(Shape::Line { a, b }) => (a, b),
        other => panic!("{other:?}"),
    }
}

#[test]
fn trim_tool_previews_and_trims_pieces() {
    let (mut ed, ids) = modify_fixture(&[
        (v(0.0, 0.0), v(30.0, 0.0)),
        (v(10.0, -5.0), v(10.0, 5.0)),
        (v(20.0, -5.0), v(20.0, 5.0)),
    ]);
    let mut tool = TrimTool::default();
    let p = ptr(&ed, &tool, v(15.0, 0.2));
    tool.handle(Input::Hover(p), &mut ed);
    let removal = tool.editor_preview(&ed);
    assert!(
        matches!(removal[..], [SketchItem { shape: Shape::Line { a, b }, emphasis: Emphasis::Remove, .. }]
        if a == v(10.0, 0.0) && b == v(20.0, 0.0))
    );
    click(&mut ed, &mut tool, v(15.0, 0.2));
    assert_eq!(line_ends(&ed, ids[0]), (v(0.0, 0.0), v(10.0, 0.0)));
    // Drag across the two verticals below the bar: both lower pieces go.
    let m = Modifiers::default();
    let start = ptr(&ed, &tool, v(8.0, -3.0));
    tool.handle(Input::DragStart(start, m), &mut ed);
    for x in [10.0, 15.0, 20.0] {
        let p = ptr(&ed, &tool, v(x, -3.0));
        tool.handle(Input::Drag(p), &mut ed);
    }
    tool.handle(Input::DragEnd(ptr(&ed, &tool, v(22.0, -3.0)), m), &mut ed);
    assert_eq!(line_ends(&ed, ids[1]).0, v(10.0, 0.0));
    assert_eq!(line_ends(&ed, ids[2]).0, v(20.0, 0.0));
}

#[test]
fn extend_and_split_tools() {
    let (mut ed, ids) =
        modify_fixture(&[(v(0.0, 0.0), v(5.0, 0.0)), (v(10.0, -5.0), v(10.0, 5.0))]);
    let mut tool = ExtendTool::default();
    let p = ptr(&ed, &tool, v(4.0, 0.1));
    tool.handle(Input::Hover(p), &mut ed);
    assert!(
        matches!(tool.preview(None)[..], [SketchItem { shape: Shape::Line { a, b }, .. }]
        if a == v(5.0, 0.0) && b == v(10.0, 0.0))
    );
    click(&mut ed, &mut tool, v(4.0, 0.1));
    assert_eq!(line_ends(&ed, ids[0]).1, v(10.0, 0.0));
    let mut split = SplitTool::default();
    click(&mut ed, &mut split, v(10.0, 2.0));
    assert_eq!(lines(&ed.doc), 3);
}

/// Two lines meeting at the origin.
fn corner_fixture() -> Editor {
    let mut ed = Editor::default();
    ed.doc.begin();
    let p = ed.doc.add_point(v(0.0, 0.0));
    let a = ed.doc.add_point(v(20.0, 0.0));
    let b = ed.doc.add_point(v(0.0, 10.0));
    ed.doc.add_line(p, a).unwrap();
    ed.doc.add_line(p, b).unwrap();
    ed.commit("Corner");
    ed
}

#[test]
fn fillet_and_chamfer_tools_use_typed_sizes() {
    let mut ed = corner_fixture();
    let mut tool = FilletTool::default();
    tool.set_typed([Some(4.0), None]);
    let p = ptr(&ed, &tool, v(0.1, 0.1));
    tool.commit_typed(&p, &mut ed);
    tool.handle(Input::Hover(p), &mut ed);
    assert!(
        matches!(tool.editor_preview(&ed)[..], [SketchItem { shape: Shape::Arc { radius, .. }, .. }]
        if (radius - 4.0).abs() < 1e-12)
    );
    // The first click picks the corner (sizing begins at the last radius); a second click
    // without moving applies it.
    click(&mut ed, &mut tool, v(0.1, 0.1));
    assert!(tool.is_modal() && arcs(&ed.doc) == 0);
    click(&mut ed, &mut tool, v(0.1, 0.1));
    assert!(!tool.is_busy());
    let arc = ed
        .doc
        .iter()
        .find_map(|(_, e)| match e.kind {
            EntityKind::Arc { center, .. } => Some(ed.doc.point(center).unwrap()),
            _ => None,
        })
        .expect("a fillet arc");
    assert!(arc.distance(v(4.0, 4.0)) < 1e-9);
    // Not a corner: explained.
    click(&mut ed, &mut tool, v(10.0, 0.0));
    assert!(ed.notice.take().is_some());

    let mut ed = corner_fixture();
    let mut tool = ChamferTool::default();
    tool.set_typed([Some(2.0), None]);
    let p = ptr(&ed, &tool, v(0.0, 0.0));
    tool.commit_typed(&p, &mut ed);
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(0.0, 0.0));
    assert_eq!(lines(&ed.doc), 3);
    assert!(
        !ed.doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
    );
}

#[test]
fn fillet_and_chamfer_size_with_the_pointer_in_steps() {
    let radius = |ed: &Editor| {
        ed.doc.iter().find_map(|(id, e)| match e.kind {
            EntityKind::Arc { .. } => match ed.doc.shape(id) {
                Some(Shape::Arc { radius, .. }) => Some(radius),
                _ => None,
            },
            _ => None,
        })
    };
    let mut ed = corner_fixture();
    let mut tool = FilletTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    // The arc passes under the pointer: at a right angle its middle is r(sqrt 2 - 1) from
    // the corner, so 1.3 mm out is a radius near 3.14, stepped to a round 3.
    let p = ptr(&ed, &tool, v(0.92, 0.92));
    assert!(p.snap.is_none(), "no snapping while sizing");
    tool.handle(Input::Hover(p), &mut ed);
    let shown = tool.editor_preview(&ed);
    assert!(
        matches!(shown[..], [SketchItem { shape: Shape::Arc { radius, .. }, .. }]
            if (radius - 3.0).abs() < 1e-9),
        "{shown:?}"
    );
    assert_eq!(tool.measurement(&p).as_deref(), Some("R 3.0000 mm"));
    // Alt sizes freely; far out, the size stops where the shorter line runs out.
    let mut free = ptr(&ed, &tool, v(0.92, 0.92));
    free.alt = true;
    tool.handle(Input::Hover(free), &mut ed);
    let r = match tool.editor_preview(&ed)[..] {
        [
            SketchItem {
                shape: Shape::Arc { radius, .. },
                ..
            },
        ] => radius,
        _ => panic!("a preview"),
    };
    assert!((r - 0.92 * 2f64.sqrt() / (2f64.sqrt() - 1.0)).abs() < 1e-9);
    let far = ptr(&ed, &tool, v(40.0, 40.0));
    tool.handle(Input::Hover(far), &mut ed);
    assert!(!tool.editor_preview(&ed).is_empty(), "still fits the lines");
    // Esc leaves the corner alone; the tool stays.
    assert!(tool.cancel(&mut ed));
    assert!(radius(&ed).is_none() && !tool.is_modal());
    // Press on the corner, drag out and release: one gesture. The radius is remembered.
    let p = ptr(&ed, &tool, v(0.0, 0.0));
    tool.handle(Input::DragStart(p, Modifiers::default()), &mut ed);
    let p = ptr(&ed, &tool, v(1.5, 1.5));
    tool.handle(Input::Drag(p), &mut ed);
    tool.handle(Input::DragEnd(p, Modifiers::default()), &mut ed);
    assert!((radius(&ed).unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(tool.measurement(&p).as_deref(), Some("R 5.0000 mm"));
    // A radius typed while sizing goes on that corner.
    let mut ed = corner_fixture();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    tool.set_typed([Some(2.5), None]);
    let p = ptr(&ed, &tool, v(3.0, 3.0));
    tool.commit_typed(&p, &mut ed);
    assert!((radius(&ed).unwrap() - 2.5).abs() < 1e-9);

    // The chamfer's cut passes under the pointer: both distances the same round number.
    let mut ed = corner_fixture();
    let mut tool = ChamferTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let p = ptr(&ed, &tool, v(1.45, 1.45));
    tool.handle(Input::Hover(p), &mut ed);
    click(&mut ed, &mut tool, v(1.45, 1.45));
    let cut = ed
        .doc
        .iter()
        .filter_map(|(id, _)| match ed.doc.shape(id) {
            Some(Shape::Line { a, b }) if a.x != b.x && a.y != b.y => Some((a, b)),
            _ => None,
        })
        .next()
        .expect("the chamfer");
    let (on_x, on_y) = if cut.0.y == 0.0 { cut } else { (cut.1, cut.0) };
    assert!(on_x.distance(v(3.0, 0.0)) < 1e-9 && on_y.distance(v(0.0, 3.0)) < 1e-9);
}

/// The corner fixture, filleted at radius 4: the arc, and a point on its middle.
fn filleted_fixture(relations: bool) -> (Editor, EntityId, DVec2) {
    let mut ed = corner_fixture();
    ed.auto_relations = relations;
    let corner = ed
        .doc
        .iter()
        .find(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
        .map(|(id, _)| id)
        .unwrap();
    let arc = ed.fillet(corner, 4.0).unwrap();
    let middle = v(4.0, 4.0) - DVec2::splat(4.0 / 2f64.sqrt());
    (ed, arc, middle)
}

/// The fillet arc's centre and radius, and the far ends of the two lines.
fn fillet_state(ed: &Editor, arc: EntityId) -> (DVec2, f64, Vec<DVec2>) {
    let Some(Shape::Arc { center, radius, .. }) = ed.doc.shape(arc) else {
        panic!("an arc");
    };
    let mut far: Vec<DVec2> = ed
        .doc
        .iter()
        .filter_map(|(id, _)| match ed.doc.shape(id) {
            Some(Shape::Line { a, b }) => Some(if a.length() > b.length() { a } else { b }),
            _ => None,
        })
        .collect();
    far.sort_by(|a, b| a.x.total_cmp(&b.x));
    (center, radius, far)
}

#[test]
fn a_fillet_is_resized_in_place() {
    for relations in [false, true] {
        let (mut ed, arc, middle) = filleted_fixture(relations);
        let relations_before = ed.doc.constraint_count();
        assert!(ed.fillet_at(arc).is_some());
        // The fillet tool on a fillet that is already there: sizing starts from its radius.
        let mut tool = FilletTool::default();
        let p = ptr(&ed, &tool, middle);
        tool.handle(Input::Hover(p), &mut ed);
        assert_eq!(ed.hovered, Some(arc), "the fillet lights up");
        click(&mut ed, &mut tool, middle);
        assert!(tool.is_modal());
        assert_eq!(tool.measurement(&p).as_deref(), Some("R 4.0000 mm"));
        // Out to where a radius-5 arc would pass, and click.
        let p = ptr(&ed, &tool, v(1.5, 1.5));
        tool.handle(Input::Hover(p), &mut ed);
        assert!(
            matches!(tool.editor_preview(&ed)[..], [SketchItem { shape: Shape::Arc { radius, .. }, .. }]
                if (radius - 5.0).abs() < 1e-9)
        );
        click(&mut ed, &mut tool, v(1.5, 1.5));
        assert!(!tool.is_busy());
        // The arc is still tangent to both lines, which haven't turned or moved their far
        // ends; no relation was added, and a radius dimension took the new value.
        let (centre, radius, far) = fillet_state(&ed, arc);
        assert!(centre.distance(v(5.0, 5.0)) < 1e-7 && (radius - 5.0).abs() < 1e-7);
        assert_eq!(far, vec![v(0.0, 10.0), v(20.0, 0.0)]);
        assert_eq!(ed.doc.constraint_count(), relations_before);
        assert_eq!(lines(&ed.doc), 2);
        assert_eq!(arcs(&ed.doc), 1);
        if relations {
            assert!(ed.doc.constraints().any(
                |(_, c)| matches!(c, Constraint::Radius { value, .. } if (*value - 5.0).abs() < 1e-9)
            ));
            assert!(ed.analysis().conflicts.is_empty());
        }
        // One undo step.
        ed.undo();
        assert!((fillet_state(&ed, arc).1 - 4.0).abs() < 1e-9);

        // With the select tool, dragging the arc does the same.
        let mut select = SelectTool::default();
        let p = ptr(&ed, &select, middle);
        select.handle(Input::DragStart(p, Modifiers::default()), &mut ed);
        for at in [v(1.0, 1.0), v(0.7, 0.7), v(0.85, 0.85)] {
            let p = ptr(&ed, &select, at);
            select.handle(Input::Drag(p), &mut ed);
        }
        let p = ptr(&ed, &select, v(0.85, 0.85));
        select.handle(Input::DragEnd(p, Modifiers::default()), &mut ed);
        let (centre, radius, far) = fillet_state(&ed, arc);
        assert!((radius - 3.0).abs() < 1e-7, "{radius}");
        assert!(centre.distance(v(3.0, 3.0)) < 1e-7);
        assert_eq!(far, vec![v(0.0, 10.0), v(20.0, 0.0)]);
        ed.undo();
        assert!((fillet_state(&ed, arc).1 - 4.0).abs() < 1e-9);
        // A radius the lines have no room for is refused, with nothing changed.
        assert!(ed.refillet(arc, 12.0).is_err());
        assert!((fillet_state(&ed, arc).1 - 4.0).abs() < 1e-9);
    }
}

#[test]
fn an_arc_that_is_not_a_fillet_is_left_to_the_usual_tools() {
    // An arc on its own, and one that meets a line at an angle.
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let a = ed.doc.add_point(v(5.0, 0.0));
    let b = ed.doc.add_point(v(0.0, 5.0));
    let arc = ed.doc.add_arc(c, a, b).unwrap();
    ed.commit("Arc");
    assert!(ed.fillet_at(arc).is_none());
    ed.doc.begin();
    let far_a = ed.doc.add_point(v(12.0, 3.0));
    let far_b = ed.doc.add_point(v(-6.0, 5.0));
    ed.doc.add_line(a, far_a).unwrap();
    ed.doc.add_line(b, far_b).unwrap();
    ed.commit("Lines");
    assert!(ed.fillet_at(arc).is_none(), "the first line isn't tangent");
    assert!(ed.refillet(arc, 2.0).is_err());
}

#[test]
fn a_chamfer_is_resized_in_place_and_corners_can_be_put_back() {
    let corner_point = |ed: &Editor| {
        ed.doc
            .iter()
            .find(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
            .map(|(id, _)| id)
    };
    // The corner fixture, cut 4 along x and 2 along y.
    let mut ed = corner_fixture();
    let cut = ed.chamfer(corner_point(&ed).unwrap(), 4.0, 2.0).unwrap();
    let c = ed.chamfer_at(cut).expect("a chamfer");
    assert!(c.corner.length() < 1e-9);
    let mut legs = c.legs;
    legs.sort_by(f64::total_cmp);
    assert!((legs[0] - 2.0).abs() < 1e-9 && (legs[1] - 4.0).abs() < 1e-9);
    // A side of a plain triangle is not a chamfer: its neighbours already meet.
    let mut tri = Editor::default();
    tri.doc.begin();
    let p = [v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)].map(|at| tri.doc.add_point(at));
    let sides = [(0, 1), (1, 2), (2, 0)].map(|(a, b)| tri.doc.add_line(p[a], p[b]).unwrap());
    tri.commit("Triangle");
    assert!(sides.iter().all(|s| tri.chamfer_at(*s).is_none()));

    // The chamfer tool on it: it starts as it is; moved, both distances become the same.
    let mut tool = ChamferTool::default();
    let middle = v(2.0, 1.0);
    let p = ptr(&ed, &tool, middle);
    tool.handle(Input::Hover(p), &mut ed);
    assert_eq!(ed.hovered, Some(cut), "the chamfer lights up");
    click(&mut ed, &mut tool, middle);
    assert!(tool.is_modal());
    let p = ptr(&ed, &tool, v(2.5, 2.5));
    tool.handle(Input::Hover(p), &mut ed);
    click(&mut ed, &mut tool, v(2.5, 2.5));
    let c = ed.chamfer_at(cut).expect("still a chamfer");
    assert!(
        (c.legs[0] - 5.0).abs() < 1e-7 && (c.legs[1] - 5.0).abs() < 1e-7,
        "{:?}",
        c.legs
    );
    assert!(c.corner.length() < 1e-7);
    let mut far = c.far;
    far.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        far,
        [v(0.0, 10.0), v(20.0, 0.0)],
        "the lines' far ends stay"
    );
    assert_eq!(lines(&ed.doc), 3);
    // Too large for the shorter line: refused, nothing changed.
    assert!(ed.rechamfer(cut, 5.0, 12.0).is_err() || ed.rechamfer(cut, 12.0, 5.0).is_err());

    // Remove chamfer: the two lines run on to the corner again.
    ed.remove_corner_cut(cut).unwrap();
    assert_eq!(lines(&ed.doc), 2);
    let corner = corner_point(&ed).expect("the corner is back");
    assert!(ed.corner(corner).is_ok(), "two lines meet there");
    ed.undo();
    assert!(ed.chamfer_at(cut).is_some());

    // Remove fillet, with and without its relations.
    for relations in [false, true] {
        let (mut ed, arc, _) = filleted_fixture(relations);
        ed.selection = [arc].into();
        ed.remove_corner_cut(arc).unwrap();
        assert_eq!((lines(&ed.doc), arcs(&ed.doc)), (2, 0));
        assert_eq!(
            ed.doc.constraint_count(),
            0,
            "its tangents and radius went with it"
        );
        assert!(ed.selection.is_empty());
        let corner = corner_point(&ed).expect("the corner is back");
        assert!(ed.corner(corner).is_ok());
        // Only the lines' four ends are left (three points: the corner is shared).
        let points = ed.doc.iter().filter(|(_, e)| e.kind.is_point()).count();
        assert_eq!(points, 3);
        ed.undo();
        assert!(ed.fillet_at(arc).is_some());
    }
    // An arc on its own has no corner to put back.
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let a = ed.doc.add_point(v(5.0, 0.0));
    let b = ed.doc.add_point(v(0.0, 5.0));
    let arc = ed.doc.add_arc(c, a, b).unwrap();
    ed.commit("Arc");
    assert!(ed.remove_corner_cut(arc).is_err());
}

#[test]
fn typed_sizes_leave_no_dimension_when_automatic_relations_are_off() {
    for relations in [false, true] {
        let mut ed = Editor::default();
        ed.auto_relations = relations;
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        let p = ptr(&ed, &tool, v(8.0, 0.2));
        tool.set_typed([Some(25.0), Some(0.0)]);
        tool.commit_typed(&p, &mut ed);
        tool.handle(Input::Finish, &mut ed);
        // The line is 25 long either way.
        let line = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .unwrap();
        assert_eq!(line_ends(&ed, line), (v(0.0, 0.0), v(25.0, 0.0)));
        // Only with automatic relations is it held there, by a length (and a horizontal).
        let held = ed.doc.constraint_count();
        assert_eq!(held, if relations { 2 } else { 0 });

        // The inspector's length field: off, it resizes and keeps nothing; on, it adds the
        // dimension. One undo step either way.
        let EntityKind::Line { a, b } = ed.doc.get(line).unwrap().kind else {
            unreachable!()
        };
        let wanted = Constraint::Distance { a, b, value: 0.0 };
        if relations {
            assert!(
                ed.size_dimension(&wanted).is_some(),
                "the typed length is there to edit"
            );
        } else {
            ed.resize_once(wanted, "40").unwrap();
            assert_eq!(ed.doc.constraint_count(), 0);
            let (p, q) = line_ends(&ed, line);
            assert!((p.distance(q) - 40.0).abs() < 1e-7);
            ed.undo();
            let (p, q) = line_ends(&ed, line);
            assert!((p.distance(q) - 25.0).abs() < 1e-9);
            assert!(
                ed.resize_once(wanted, "-3").is_err(),
                "a bad size is refused"
            );
            assert_eq!(ed.doc.constraint_count(), 0);
        }
    }
}

#[test]
fn two_lines_dimension_by_distance_when_they_run_alongside() {
    use crate::tools::dimension::DimBase;
    use crate::tools::select::Target;
    let mut ed = Editor::default();
    ed.doc.begin();
    let line = |ed: &mut Editor, a: DVec2, b: DVec2| {
        let (pa, pb) = (ed.doc.add_point(a), ed.doc.add_point(b));
        ed.doc.add_line(pa, pb).unwrap()
    };
    let base = line(&mut ed, v(0.0, 0.0), v(40.0, 0.0));
    // Drawn by eye as parallel (a third of a degree off), exactly parallel but reversed,
    // and clearly at an angle.
    let nearly = line(&mut ed, v(0.0, 10.0), v(40.0, 10.25));
    let reversed = line(&mut ed, v(40.0, 20.0), v(0.0, 20.0));
    let skew = line(&mut ed, v(0.0, 30.0), v(40.0, 38.0));
    ed.commit("Lines");
    let pair =
        |ed: &Editor, other| DimensionTool::pair(ed, Target::Entity(base), Target::Entity(other));
    for other in [nearly, reversed] {
        match pair(&ed, other) {
            Some(DimBase::PointLine(point, to)) => {
                assert_eq!(to, other);
                assert_eq!(ed.doc.point(point).unwrap(), v(0.0, 0.0));
            }
            got => panic!("{got:?}"),
        }
    }
    assert!(matches!(pair(&ed, skew), Some(DimBase::Angle(..))));
    // The distance it measures is the gap between the lines.
    let Some(base_dim) = pair(&ed, reversed) else {
        unreachable!()
    };
    let c = DimensionTool::dimension(&ed, base_dim, v(20.0, 10.0)).unwrap();
    assert!(
        matches!(c, Constraint::PointLineDistance { value, .. } if (value - 20.0).abs() < 1e-9)
    );
}

#[test]
fn the_origin_is_a_snap_target() {
    use drift_geom::SnapKind;
    let mut ed = Editor::default();
    let options = SnapOptions {
        tolerance: TOL,
        grid: None,
    };
    // An empty sketch: near the origin snaps to it; further away, nothing.
    let s = ed
        .snap(v(0.2, -0.3), options, |_| false)
        .expect("the origin");
    assert_eq!(
        (s.at, s.kind, s.entity),
        (DVec2::ZERO, SnapKind::Origin, None)
    );
    assert!(ed.snap(v(3.0, 0.0), options, |_| false).is_none());
    // A line through it: the origin wins over a point along the line.
    ed.doc.begin();
    let a = ed.doc.add_point(v(-10.0, 0.0));
    let b = ed.doc.add_point(v(10.0, 0.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    let s = ed.snap(v(0.2, 0.1), options, |_| false).unwrap();
    assert_eq!(s.kind, SnapKind::Origin);
    // A point of the sketch at the origin comes first: it can be joined to.
    ed.doc.begin();
    let o = ed.doc.add_point(DVec2::ZERO);
    ed.commit("Point");
    let s = ed.snap(v(0.2, 0.1), options, |_| false).unwrap();
    assert_eq!((s.kind, s.entity), (SnapKind::Point, Some(o)));
    // A line drawn to it lands exactly on (0, 0).
    let mut ed = Editor::default();
    let mut tool = LineTool::default();
    let click_snapped = |ed: &mut Editor, tool: &mut LineTool, at: DVec2| {
        let mut p = Pointer::new(at, TOL);
        p.snap = ed.snap(at, options, |_| false);
        tool.handle(Input::Click(p, Modifiers::default()), ed);
    };
    click_snapped(&mut ed, &mut tool, v(20.0, 15.0));
    click_snapped(&mut ed, &mut tool, v(0.3, -0.2));
    tool.handle(Input::Finish, &mut ed);
    let line = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(line_ends(&ed, line), (v(20.0, 15.0), DVec2::ZERO));
}

#[test]
fn lines_turn_in_steps_with_angle_snapping() {
    let mut ed = with_auto_relations();
    let mut tool = LineTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let aim = |ed: &Editor, tool: &LineTool, at: DVec2, step: Option<f64>| {
        let mut p = Pointer::new(at, TOL);
        p.angle_step = step;
        if tool.wants_snap() {
            p.snap = ed.index.snap(
                at,
                SnapOptions {
                    tolerance: TOL,
                    grid: None,
                },
                |id| tool.snap_excludes(ed, id),
            );
        }
        tool.refine_pointer(&mut p);
        p
    };
    // 28 degrees up: with 15 degree steps the line goes at 30, as long as the pointer is
    // far along that direction.
    let at = DVec2::from_angle(28f64.to_radians()) * 20.0;
    let p = aim(&ed, &tool, at, Some(15.0));
    let got = p.pos();
    assert!(
        (got.y.atan2(got.x).to_degrees() - 30.0).abs() < 1e-9,
        "{got:?}"
    );
    assert!((got.length() - 20.0 * 2f64.to_radians().cos()).abs() < 1e-9);
    assert_eq!(p.auto_relation(), None, "30 degrees is no relation");
    // 45 degree steps from 50 degrees: 45. Off: where the pointer is.
    let at = DVec2::from_angle(50f64.to_radians()) * 20.0;
    let got = aim(&ed, &tool, at, Some(45.0)).pos();
    assert!((got.x - got.y).abs() < 1e-9 && got.x > 0.0);
    assert_eq!(aim(&ed, &tool, at, None).pos(), at);
    // 1 degree steps: 28.4 degrees goes at exactly 28.
    let at = DVec2::from_angle(28.4f64.to_radians()) * 20.0;
    let got = aim(&ed, &tool, at, Some(1.0)).pos();
    assert!(
        (got.y.atan2(got.x).to_degrees() - 28.0).abs() < 1e-9,
        "{got:?}"
    );
    // A step that lands on an axis is exactly on it, and gets its relation.
    let p = aim(&ed, &tool, v(-30.0, 1.0), Some(5.0));
    assert_eq!(p.pos().y, 0.0);
    assert!((p.pos().x + 30.0).abs() < 1e-9);
    assert_eq!(p.auto_relation(), Some(GuideKind::Horizontal));
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    assert!(matches!(
        line_relations(&ed)[..],
        [Constraint::Horizontal { .. }]
    ));
    // An exact snap still wins: the next line goes to the point, off the steps.
    ed.doc.begin();
    let target = ed.doc.add_point(v(-20.0, 13.0));
    ed.commit("Point");
    let p = aim(&ed, &tool, v(-20.1, 13.1), Some(45.0));
    assert_eq!(p.snapped_point(), Some(target));
    assert_eq!(p.pos(), v(-20.0, 13.0));
}

#[test]
fn sizing_steps_are_round_numbers() {
    use super::corners::round_step;
    for (min, step) in [
        (0.07, 0.1),
        (0.11, 0.2),
        (0.3, 0.5),
        (0.6, 1.0),
        (1.0, 1.0),
        (40.0, 50.0),
    ] {
        assert!((round_step(min, true) - step).abs() < 1e-12, "{min}");
    }
    let inch = drift_core::units::MM_PER_INCH;
    for (min, step) in [
        (0.1, 1.0 / 64.0),
        (3.0, 0.125),
        (7.0, 0.5),
        (300.0, 12.0),
        (2000.0, 120.0),
    ] {
        assert!((round_step(min, false) - step * inch).abs() < 1e-9, "{min}");
    }
}

#[test]
fn offset_tool_picks_a_chain_then_the_side() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let ps = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0)].map(|p| ed.doc.add_point(p));
    ed.doc.add_line(ps[0], ps[1]).unwrap();
    ed.doc.add_line(ps[1], ps[2]).unwrap();
    ed.commit("Chain");
    let mut tool = OffsetTool::default();
    click(&mut ed, &mut tool, v(10.0, 0.0));
    assert!(tool.is_busy());
    assert_eq!(ed.highlight.len(), 2, "the chain is highlighted");
    // The pointer 3 mm inside sets side and distance; the preview follows.
    let p = ptr(&ed, &tool, v(10.0, 3.0));
    tool.handle(Input::Hover(p), &mut ed);
    assert_eq!(tool.preview(None).len(), 2);
    // A typed distance wins.
    tool.set_typed([Some(2.0), None]);
    tool.commit_typed(&p, &mut ed);
    assert!(!tool.is_busy());
    assert_eq!(lines(&ed.doc), 4);
    assert!(
        ed.doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(v(18.0, 2.0)))
    );
}

#[test]
fn mirror_tool_mirrors_the_selection() {
    let (mut ed, ids) =
        modify_fixture(&[(v(2.0, 1.0), v(5.0, 4.0)), (v(0.0, -10.0), v(0.0, 10.0))]);
    let mut tool = MirrorTool;
    // Nothing selected: explained.
    click(&mut ed, &mut tool, v(0.0, 5.0));
    assert!(ed.notice.take().is_some());
    ed.selection = [ids[0]].into();
    click(&mut ed, &mut tool, v(0.0, 5.0));
    assert_eq!(lines(&ed.doc), 3);
    assert!(
        ed.doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(v(-5.0, 4.0)))
    );
}

// ----- Move base points and snapping -------------------------------------------------------

/// A pointer snapped like the canvas does, optionally to a grid.
fn ptr_grid(ed: &Editor, tool: &dyn Tool, at: DVec2, grid: Option<f64>) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    if tool.wants_snap() {
        p.snap = ed.index.snap(
            at,
            SnapOptions {
                tolerance: TOL,
                grid,
            },
            |id| tool.snap_excludes(ed, id),
        );
    }
    tool.refine_pointer(&mut p);
    p
}

/// A line from (0, 0) to (10, 0) and a separate target point at (30, 20).
fn move_fixture() -> (Editor, EntityId, [EntityId; 2], EntityId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, 0.0));
    let b = ed.doc.add_point(v(10.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    let target = ed.doc.add_point(v(30.0, 20.0));
    ed.commit("Setup");
    (ed, line, [a, b], target)
}

#[test]
fn grab_bases_a_line_on_its_midpoint_and_lands_it_on_snaps() {
    let (mut ed, line, [a, b], _) = move_fixture();
    ed.selection = [line].into();
    let mut tool = SelectTool::default();
    // G with the cursor well away from the line.
    let p = ptr(&ed, &tool, v(40.0, -15.0));
    assert!(tool.key(ToolKey::G, Some(&p), &mut ed));
    assert_eq!(tool.grab.as_ref().unwrap().base, v(5.0, 0.0));
    // Unsnapped, the line follows the cursor's motion.
    let p = ptr(&ed, &tool, v(42.0, -14.0));
    tool.handle(Input::Hover(p), &mut ed);
    assert_eq!(ed.doc.point(a).unwrap(), v(2.0, 1.0));
    // Snapped to the target point, the midpoint lands exactly on it.
    click(&mut ed, &mut tool, v(30.1, 19.9));
    assert_eq!(ed.doc.point(a).unwrap(), v(25.0, 20.0));
    assert_eq!(ed.doc.point(b).unwrap(), v(35.0, 20.0));
}

#[test]
fn grab_bases_a_point_on_itself() {
    let (mut ed, _, [a, _], _) = move_fixture();
    ed.selection = [a].into();
    let mut tool = SelectTool::default();
    let p = ptr(&ed, &tool, v(-20.0, 7.0));
    tool.key(ToolKey::G, Some(&p), &mut ed);
    assert_eq!(tool.grab.as_ref().unwrap().base, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(29.9, 20.2));
    assert_eq!(ed.doc.point(a).unwrap(), v(30.0, 20.0));
}

#[test]
fn grab_moves_a_selected_region_from_its_centroid() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    for i in 0..4 {
        ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap();
    }
    let target = ed.doc.add_point(v(50.0, 50.0));
    ed.commit("Setup");
    let mut tool = SelectTool::default();
    click(&mut ed, &mut tool, v(2.0, 2.0));
    assert!(ed.region().is_some());
    let p = ptr(&ed, &tool, v(2.0, 2.0));
    tool.key(ToolKey::G, Some(&p), &mut ed);
    assert_eq!(tool.grab.as_ref().unwrap().base, v(5.0, 3.0));
    assert_eq!(tool.grab.as_ref().unwrap().ids.len(), 4);
    click(&mut ed, &mut tool, v(50.2, 49.9));
    // The centroid lands on the target; the region stays selected.
    assert_eq!(ed.doc.point(c[0]).unwrap(), v(45.0, 47.0));
    let region = ed.region().expect("still selected");
    assert!(region.centroid.distance(ed.doc.point(target).unwrap()) < 1e-9);
}

#[test]
fn a_selected_region_stays_lit_while_it_moves() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    for i in 0..4 {
        ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap();
    }
    ed.commit("Setup");
    let mut tool = SelectTool::default();
    let m = Modifiers::default();
    let centroid = |ed: &mut Editor| ed.region().expect("still lit").centroid;
    // Where the outline's middle is now.
    let middle = |ed: &Editor| ed.doc.point(c[0]).unwrap() + v(5.0, 3.0);
    click(&mut ed, &mut tool, v(2.0, 2.0));
    assert_eq!(centroid(&mut ed), v(5.0, 3.0));

    // G: well clear of where it was, the region is still picked, where it now is.
    let p = ptr(&ed, &tool, v(2.0, 2.0));
    tool.key(ToolKey::G, Some(&p), &mut ed);
    let to = ptr(&ed, &tool, v(40.0, 30.0));
    tool.handle(Input::Hover(to), &mut ed);
    assert!(middle(&ed).distance(v(5.0, 3.0)) > 30.0, "moved well clear");
    assert!(centroid(&mut ed).distance(middle(&ed)) < 1e-9);
    // Esc puts it back, still picked.
    tool.cancel(&mut ed);
    assert_eq!(centroid(&mut ed), v(5.0, 3.0));

    // Dragging its outline: the same, and it stays picked where it lands.
    let start = ptr(&ed, &tool, v(5.0, 0.0));
    tool.handle(Input::DragStart(start, m), &mut ed);
    let to = ptr(&ed, &tool, v(65.0, 0.0));
    tool.handle(Input::Drag(to), &mut ed);
    assert!(middle(&ed).distance(v(5.0, 3.0)) > 30.0, "moved well clear");
    assert!(centroid(&mut ed).distance(middle(&ed)) < 1e-9);
    tool.handle(Input::DragEnd(to, m), &mut ed);
    assert!(ed.region_motion.is_none());
    let landed = middle(&ed);
    assert!(centroid(&mut ed).distance(landed) < 1e-9);

    // R: it turns with its outline.
    let p = ptr(&ed, &tool, landed - v(3.0, 1.0));
    tool.key(ToolKey::R, Some(&p), &mut ed);
    let spin = tool.spin.as_mut().unwrap();
    spin.typed = Some(90.0);
    let pivot = spin.pivot;
    spin.apply(&mut ed);
    let turned = pivot + DVec2::from_angle(90f64.to_radians()).rotate(landed - pivot);
    assert!(centroid(&mut ed).distance(turned) < 1e-6);
    tool.confirm_spin(&mut ed);
    assert!(ed.region_motion.is_none());
    assert!(centroid(&mut ed).distance(turned) < 1e-6);
}

/// Drag from `from` to `to` with the select tool, holding (Shift, Ctrl, Alt) on the way.
fn drag_with(
    ed: &mut Editor,
    tool: &mut SelectTool,
    from: DVec2,
    to: DVec2,
    keys: (bool, bool, bool),
) {
    let m = Modifiers::default();
    let start = ptr(ed, tool, from);
    tool.handle(Input::DragStart(start, m), ed);
    let mut end = ptr(ed, tool, to);
    (end.shift, end.command, end.alt) = keys;
    tool.handle(Input::Drag(end), ed);
    tool.handle(Input::DragEnd(end, m), ed);
}

#[test]
fn transform_handles_scale_stretch_and_turn() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
    for (i, line) in l.iter().enumerate() {
        let r = if i % 2 == 0 {
            Constraint::Horizontal { line: *line }
        } else {
            Constraint::Vertical { line: *line }
        };
        ed.doc.add_constraint(r).unwrap();
    }
    ed.commit("Setup");
    ed.selection = l.iter().copied().collect();
    let mut tool = SelectTool::default();
    let at = |ed: &Editor, i: usize| ed.doc.point(c[i]).unwrap();
    let none = (false, false, false);
    // The handles sit a pick tolerance outside the geometry (TOL here).
    let (right, corner) = (v(10.5, 3.0), v(10.5, 6.5));

    // The right side stretches sideways, from the left side; still a rectangle.
    drag_with(&mut ed, &mut tool, right, v(15.5, 3.0), none);
    let sx = 15.5 / 10.5;
    assert!(
        at(&ed, 2).distance(v(10.0 * sx, 6.0)) < 1e-6,
        "{:?}",
        at(&ed, 2)
    );
    assert_eq!(at(&ed, 0), v(0.0, 0.0));
    assert!(
        (at(&ed, 1).x - at(&ed, 2).x).abs() < 1e-9,
        "the side stays vertical"
    );
    ed.undo();
    assert_eq!(at(&ed, 2), v(10.0, 6.0), "one undo step");

    // A corner with Shift keeps the shape (the larger factor).
    drag_with(
        &mut ed,
        &mut tool,
        corner,
        v(21.0, 7.0),
        (true, false, false),
    );
    assert!(
        at(&ed, 2).distance(v(20.0, 12.0)) < 1e-6,
        "{:?}",
        at(&ed, 2)
    );
    ed.undo();
    // With Ctrl, about the middle.
    drag_with(
        &mut ed,
        &mut tool,
        corner,
        v(16.0, 9.5),
        (false, true, false),
    );
    let sy = 6.5 / 3.5;
    assert!(
        at(&ed, 2).distance(v(15.0, 3.0 + 3.0 * sy)) < 1e-6,
        "{:?}",
        at(&ed, 2)
    );
    assert!(
        at(&ed, 0).distance(v(-5.0, 3.0 - 3.0 * sy)) < 1e-6,
        "{:?}",
        at(&ed, 0)
    );
    ed.undo();

    // A dimensioned width holds; with Alt it scales too.
    ed.doc.begin();
    let width = ed
        .doc
        .add_constraint(Constraint::Distance {
            a: c[0],
            b: c[1],
            value: 10.0,
        })
        .unwrap();
    ed.commit("Width");
    drag_with(&mut ed, &mut tool, corner, v(21.0, 13.0), none);
    assert!(
        at(&ed, 2).distance(v(10.0, 12.0)) < 1e-6,
        "{:?}",
        at(&ed, 2)
    );
    assert_eq!(ed.doc.constraint(width).unwrap().value(), Some(10.0));
    ed.undo();
    drag_with(
        &mut ed,
        &mut tool,
        corner,
        v(21.0, 13.0),
        (false, false, true),
    );
    assert!(
        at(&ed, 2).distance(v(20.0, 12.0)) < 1e-6,
        "{:?}",
        at(&ed, 2)
    );
    assert_eq!(ed.doc.constraint(width).unwrap().value(), Some(20.0));
    ed.undo();

    // Esc mid-drag puts it back.
    let start = ptr(&ed, &tool, corner);
    tool.handle(Input::DragStart(start, Modifiers::default()), &mut ed);
    tool.handle(Input::Drag(ptr(&ed, &tool, v(30.0, 30.0))), &mut ed);
    assert_ne!(at(&ed, 2), v(10.0, 6.0));
    tool.cancel(&mut ed);
    assert_eq!(at(&ed, 2), v(10.0, 6.0));

    // Just outside a corner turns about the middle; letting go places it.
    let from = v(11.2, 7.2);
    let to = v(5.0, 3.0) + DVec2::from_angle(90f64.to_radians()).rotate(from - v(5.0, 3.0));
    drag_with(&mut ed, &mut tool, from, to, none);
    assert!(tool.spin.is_none(), "placed on letting go");
    assert!(at(&ed, 0).distance(v(8.0, -2.0)) < 1e-6, "{:?}", at(&ed, 0));
    ed.undo();
    assert_eq!(at(&ed, 0), v(0.0, 0.0));
}

#[test]
fn a_held_stretch_keeps_the_area_picked_and_alt_scales_only_along_it() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c =
        [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)].map(|p| ed.doc.add_point(p));
    let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
    for (i, line) in l.iter().enumerate() {
        let r = if i % 2 == 0 {
            Constraint::Horizontal { line: *line }
        } else {
            Constraint::Vertical { line: *line }
        };
        ed.doc.add_constraint(r).unwrap();
    }
    let width = ed
        .doc
        .add_constraint(Constraint::HorizontalDistance {
            a: c[0],
            b: c[1],
            value: 100.0,
        })
        .unwrap();
    let height = ed
        .doc
        .add_constraint(Constraint::VerticalDistance {
            a: c[1],
            b: c[2],
            value: 100.0,
        })
        .unwrap();
    ed.commit("Setup");
    let mut tool = SelectTool::default();
    // Picked near its right side, so a stretch that the width holds back would leave the
    // point it is remembered by far outside it.
    click(&mut ed, &mut tool, v(90.0, 50.0));
    assert!(ed.region().is_some());
    let side = v(100.5, 50.0);

    // Held: drag the right side far out. It stays 100 wide, and stays picked throughout.
    let m = Modifiers::default();
    let start = ptr(&ed, &tool, side);
    tool.handle(Input::DragStart(start, m), &mut ed);
    let far = ptr(&ed, &tool, v(400.0, 50.0));
    tool.handle(Input::Drag(far), &mut ed);
    assert!(ed.region().is_some(), "lit while dragging");
    tool.handle(Input::DragEnd(far, m), &mut ed);
    assert!(ed.region().is_some(), "still picked after");
    assert!((ed.doc.point(c[1]).unwrap().x - 100.0).abs() < 1e-6);

    // With Alt, the width follows the stretch and the height doesn't.
    drag_with(
        &mut ed,
        &mut tool,
        side,
        v(300.0, 50.0),
        (false, false, true),
    );
    // A round width (whole millimetres at this zoom), not 298.507.
    let w = ed.doc.constraint(width).unwrap().value().unwrap();
    assert!((w - 299.0).abs() < 1e-6, "{w}");
    assert_eq!(ed.doc.constraint(height).unwrap().value(), Some(100.0));
    assert!((ed.doc.point(c[2]).unwrap().y - 100.0).abs() < 1e-6);
    assert!(ed.region().is_some(), "picked where it landed");

    // With round sizes turned off, the width is whatever the pointer makes it.
    ed.undo();
    let start = ptr(&ed, &tool, side);
    tool.handle(Input::DragStart(start, m), &mut ed);
    let mut end = ptr(&ed, &tool, v(300.0, 50.0));
    end.alt = true;
    end.round_scale = false;
    tool.handle(Input::Drag(end), &mut ed);
    tool.handle(Input::DragEnd(end, m), &mut ed);
    let w = ed.doc.constraint(width).unwrap().value().unwrap();
    assert!((w - 100.0 * 300.0 / 100.5).abs() < 1e-6, "{w}");
}

#[test]
fn dragging_inside_a_picked_area_moves_it() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    for i in 0..4 {
        ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap();
    }
    ed.commit("Setup");
    let mut tool = SelectTool::default();
    // Not picked: a drag inside box-selects (and moves nothing).
    drag_with(
        &mut ed,
        &mut tool,
        v(3.0, 3.0),
        v(13.0, 3.0),
        (false, false, false),
    );
    assert_eq!(ed.doc.point(c[0]).unwrap(), v(0.0, 0.0));
    // Picked: the drag moves it, and it stays picked.
    click(&mut ed, &mut tool, v(2.0, 2.0));
    assert!(ed.region().is_some());
    drag_with(
        &mut ed,
        &mut tool,
        v(3.0, 3.0),
        v(13.0, 3.0),
        (false, false, false),
    );
    assert_eq!(ed.doc.point(c[0]).unwrap(), v(10.0, 0.0));
    assert!(
        ed.region()
            .expect("still picked")
            .centroid
            .distance(v(15.0, 3.0))
            < 1e-9
    );
    ed.undo();
    assert_eq!(ed.doc.point(c[0]).unwrap(), v(0.0, 0.0));
}

#[test]
fn dragging_a_line_snaps_the_end_nearest_the_grab() {
    let (mut ed, line, [_, b], _) = move_fixture();
    let mut tool = SelectTool::default();
    let m = Modifiers::default();
    // Grab near the right end, drag so the cursor snaps to the target point.
    let start = ptr(&ed, &tool, v(9.0, 0.1));
    tool.handle(Input::DragStart(start, m), &mut ed);
    assert_eq!(ed.selection, [line].into());
    let to = ptr(&ed, &tool, v(30.2, 20.1));
    assert!(to.snap.is_some());
    tool.handle(Input::Drag(to), &mut ed);
    tool.handle(Input::DragEnd(to, m), &mut ed);
    assert_eq!(ed.doc.point(b).unwrap(), v(30.0, 20.0));
}

#[test]
fn dragging_with_grid_snapping_puts_the_base_on_the_grid() {
    let (mut ed, line, [a, _], _) = move_fixture();
    ed.selection = [line].into();
    let mut tool = SelectTool::default();
    let m = Modifiers::default();
    let start = ptr_grid(&ed, &tool, v(0.4, 0.1), Some(5.0));
    tool.handle(Input::DragStart(start, m), &mut ed);
    let to = ptr_grid(&ed, &tool, v(13.3, 6.6), Some(5.0));
    tool.handle(Input::Drag(to), &mut ed);
    tool.handle(Input::DragEnd(to, m), &mut ed);
    // The end nearest the grab (a) is on the grid point nearest the cursor.
    assert_eq!(ed.doc.point(a).unwrap(), v(15.0, 5.0));
}

#[test]
fn exact_snaps_win_over_inference_guides() {
    use crate::editor::{Guide, GuideKind};
    use drift_geom::{Snap, SnapKind};

    let mut p = Pointer::new(v(5.2, 0.3), TOL);
    p.snap = Some(Snap {
        at: v(5.0, 0.0),
        kind: SnapKind::Midpoint,
        entity: None,
    });
    p.guide = Some(Guide {
        at: v(5.2, 3.0),
        kind: GuideKind::Horizontal,
        from: [None, None],
    });
    assert_eq!(p.pos(), v(5.0, 0.0), "the midpoint, as the marker shows");
    assert_eq!(p.auto_relation(), None);
    // A grid snap combines with the guide (the guide decides).
    p.snap = Some(Snap {
        at: v(5.0, 0.0),
        kind: SnapKind::Grid,
        entity: None,
    });
    assert_eq!(p.pos(), v(5.2, 3.0));
    assert_eq!(p.auto_relation(), Some(GuideKind::Horizontal));
}

fn shift_click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::Hover(p), ed);
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    tool.handle(Input::Click(p, shift), ed);
}

#[test]
fn chain_and_baseline_dimensions_continue_a_row() {
    let mut doc = Document::new();
    let pts: Vec<EntityId> = [0.0, 10.0, 25.0, 45.0]
        .iter()
        .map(|x| doc.add_point(v(*x, 0.0)))
        .collect();
    let mut ed = Editor::new(doc);
    let mut tool = DimensionTool::default();
    // A horizontal dimension 0 → 10 with its label below.
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    click(&mut ed, &mut tool, v(5.0, -8.0));
    let horizontal = |ed: &Editor| -> Vec<(EntityId, EntityId, f64, bool)> {
        ed.doc
            .constraints()
            .filter_map(|(id, c)| match *c {
                Constraint::HorizontalDistance { a, b, value } => {
                    Some((a, b, value, ed.doc.is_driven(id)))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(horizontal(&ed).len(), 1);
    // Chain: Shift+click 25 for 10 → 25, on the same line.
    shift_click(&mut ed, &mut tool, v(25.0, 0.0));
    let dims = horizontal(&ed);
    assert_eq!(dims.len(), 2);
    assert_eq!((dims[1].0, dims[1].1), (pts[1], pts[2]));
    assert!((dims[1].2 - 15.0).abs() < 1e-12);
    // Baseline: 0 → 45, a row further out.
    assert!(tool.key(ToolKey::B, None, &mut ed));
    shift_click(&mut ed, &mut tool, v(45.0, 0.0));
    let dims = horizontal(&ed);
    assert_eq!(dims.len(), 3);
    assert_eq!((dims[2].0, dims[2].1), (pts[0], pts[3]));
    let (first, last) = (
        ed.doc
            .constraints()
            .find(|(_, c)| matches!(c, Constraint::HorizontalDistance { b, .. } if *b == pts[1]))
            .unwrap()
            .0,
        ed.doc
            .constraints()
            .find(|(_, c)| matches!(c, Constraint::HorizontalDistance { b, .. } if *b == pts[3]))
            .unwrap()
            .0,
    );
    let label_y = |id| {
        let c = *ed.doc.constraint(id).unwrap();
        crate::annotations::dimension_anchor(&ed.doc, &c).unwrap().y
            + ed.doc.placement(id).unwrap().y
    };
    assert!(
        label_y(last) < label_y(first) - 1.0,
        "baseline rows step outward"
    );
    // A plain click starts a new dimension instead.
    click(&mut ed, &mut tool, v(25.0, 0.0));
    assert!(tool.run.is_none());
    assert_eq!(horizontal(&ed).len(), 3);
}

#[test]
fn chained_dimensions_over_a_defined_sketch_become_reference() {
    let mut doc = Document::new();
    let a = doc.add_point(v(0.0, 0.0));
    let b = doc.add_point(v(10.0, 0.0));
    let c = doc.add_point(v(20.0, 0.0));
    doc.add_constraint(Constraint::Fix {
        point: a,
        at: v(0.0, 0.0),
    })
    .unwrap();
    doc.add_constraint(Constraint::Fix {
        point: c,
        at: v(20.0, 0.0),
    })
    .unwrap();
    doc.add_constraint(Constraint::HorizontalPoints { a, b })
        .unwrap();
    let mut ed = Editor::new(doc);
    let mut tool = DimensionTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    click(&mut ed, &mut tool, v(5.0, -8.0));
    // 10 → 20 is now implied: it's added as a driven dimension, without a prompt.
    shift_click(&mut ed, &mut tool, v(20.0, 0.0));
    let driven = ed
        .doc
        .constraints()
        .filter(|(id, _)| ed.doc.is_driven(*id))
        .count();
    assert_eq!(driven, 1);
    assert!(ed.pending_driven.is_none());
}

#[test]
fn a_drawn_circle_takes_a_diameter_dimension() {
    let mut ed = Editor::default();
    let mut circle = CircleTool::default();
    click(&mut ed, &mut circle, v(0.0, 0.0));
    click(&mut ed, &mut circle, v(10.0, 0.0));
    let id = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .map(|(id, _)| id)
        .expect("circle");
    let mut dim = DimensionTool::default();
    click(&mut ed, &mut dim, v(0.0, 10.0));
    assert_eq!(
        dim.base,
        Some(dimension::DimBase::Circular(id, true)),
        "{:?}",
        dim.first
    );
    click(&mut ed, &mut dim, v(15.0, 15.0));
    let d = ed
        .doc
        .constraints()
        .find(|(_, c)| matches!(c, Constraint::Diameter { .. }))
        .map(|(id, _)| id)
        .expect("diameter dimension");
    ed.set_dimension(d, 30.0).unwrap();
    assert_eq!(drift_solver::radius_of(&ed.doc, id), Some(15.0));
}

fn rotated_rectangle_fixture() -> (Editor, Vec<EntityId>) {
    let mut ed = Editor::default();
    ed.auto_relations = true;
    let mut rect = RectangleTool::default();
    click(&mut ed, &mut rect, v(0.0, 0.0));
    click(&mut ed, &mut rect, v(20.0, 10.0));
    let ids: Vec<EntityId> = ed.doc.iter().map(|(id, _)| id).collect();
    ed.select_all();
    (ed, ids)
}

fn hv_count(ed: &Editor) -> (usize, usize) {
    let h = ed
        .doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Horizontal { .. }))
        .count();
    let v_ = ed
        .doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Vertical { .. }))
        .count();
    (h, v_)
}

#[test]
fn rotate_turns_the_selection_about_its_centre() {
    let (mut ed, ids) = rotated_rectangle_fixture();
    assert_eq!(hv_count(&ed), (2, 2));
    let mut tool = SelectTool::default();
    // R with the pointer to the right of the centre (10, 5); turn to 30° up.
    let start = ptr(&ed, &tool, v(30.0, 5.0));
    assert!(tool.key(ToolKey::R, Some(&start), &mut ed));
    assert!(tool.is_modal());
    let to = v(10.0, 5.0) + DVec2::from_angle(30f64.to_radians()) * 20.0;
    tool.handle(Input::Hover(ptr(&ed, &tool, to)), &mut ed);
    tool.handle(
        Input::Click(ptr(&ed, &tool, to), Modifiers::default()),
        &mut ed,
    );
    assert!(!tool.is_modal());
    // The corner (0, 0) turned 30° about (10, 5); sizes kept; H/V relations gone.
    let expected = v(10.0, 5.0) + DVec2::from_angle(30f64.to_radians()).rotate(v(-10.0, -5.0));
    assert!(
        ids.iter()
            .filter_map(|id| ed.doc.point(*id).ok())
            .any(|p| p.distance(expected) < 1e-6),
        "the corner (0, 0) lands at {expected:?}"
    );
    assert_eq!(hv_count(&ed), (0, 0));
    assert!(ed.notice.take().is_some_and(|n| n.contains("removed")));
    // One undo step puts it back, relations and all.
    ed.undo();
    assert_eq!(hv_count(&ed), (2, 2));
}

#[test]
fn quarter_turns_keep_relations_and_typed_angles_are_exact() {
    let (mut ed, ids) = rotated_rectangle_fixture();
    let mut tool = SelectTool::default();
    let start = ptr(&ed, &tool, v(30.0, 5.0));
    tool.key(ToolKey::R, Some(&start), &mut ed);
    tool.set_typed([Some(90.0), None]);
    tool.commit_typed(&start, &mut ed);
    // Horizontal and vertical swapped, so the rectangle is still constrained square.
    assert_eq!(hv_count(&ed), (2, 2));
    assert!(
        ed.doc
            .constraints()
            .all(|(id, _)| !ed.analysis().conflicts.contains(&id))
    );
    let pts: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    // Now 10 wide and 20 tall, about the same centre, exactly.
    assert!(
        pts.contains(&v(15.0, -5.0)) && pts.contains(&v(5.0, 15.0)),
        "{pts:?}"
    );

    // B picks a pivot; Esc cancels everything.
    let before: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    tool.key(ToolKey::R, Some(&start), &mut ed);
    tool.key(ToolKey::B, None, &mut ed);
    tool.handle(
        Input::Click(ptr(&ed, &tool, v(5.0, -5.0)), Modifiers::default()),
        &mut ed,
    );
    tool.handle(Input::Hover(ptr(&ed, &tool, v(40.0, 0.0))), &mut ed);
    assert!(tool.cancel(&mut ed));
    let after: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    assert_eq!(before, after);
}

#[test]
fn groups_pick_together_and_open_for_editing() {
    let (mut ed, _) = rotated_rectangle_fixture();
    let lines: BTreeSet<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .collect();
    // Grouping the selection keeps the curves (their points ride along).
    let g = ed.make_group().expect("grouped");
    assert_eq!(ed.doc.group(g).unwrap().members, lines);
    assert_eq!(ed.selected_group(), Some(g));
    let mut tool = SelectTool::default();
    // Clicking one edge picks the whole group.
    ed.selection.clear();
    click(&mut ed, &mut tool, v(4.0, 0.0));
    assert_eq!(ed.selection, lines);
    // Double-click opens it: now one edge at a time.
    tool.handle(Input::DoubleClick(ptr(&ed, &tool, v(4.0, 0.0))), &mut ed);
    assert_eq!(ed.open_group, Some(g));
    click(&mut ed, &mut tool, v(20.0, 2.0));
    assert_eq!(ed.selection.len(), 1);
    // Esc leaves the group and picks it whole again.
    assert!(tool.cancel(&mut ed));
    assert_eq!(ed.open_group, None);
    assert_eq!(ed.selection, lines);
    // Rotating the group turns all of it.
    let start = ptr(&ed, &tool, v(30.0, 5.0));
    tool.key(ToolKey::R, Some(&start), &mut ed);
    tool.set_typed([Some(180.0), None]);
    tool.commit_typed(&start, &mut ed);
    let pts: Vec<DVec2> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Point(p) => Some(p),
            _ => None,
        })
        .collect();
    assert!(
        pts.contains(&v(20.0, 10.0)) && pts.contains(&v(0.0, 0.0)),
        "{pts:?}"
    );
    // Ungroup dissolves it; one undo brings it back.
    ed.ungroup();
    assert!(ed.doc.groups().next().is_none());
    ed.undo();
    assert_eq!(ed.doc.group(g).unwrap().members, lines);
}

#[test]
fn a_note_leader_can_point_at_a_line_anywhere_along_it() {
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    ed.auto_relations = false;
    let mut tool = NoteTool::default();
    // On the bottom edge, away from its ends and middle.
    let on_edge = ptr(&ed, &tool, v(6.0, 0.3));
    tool.handle(Input::Hover(on_edge), &mut ed);
    tool.handle(Input::Click(on_edge, Modifiers::default()), &mut ed);
    assert!(tool.is_busy(), "a leader started on the line");
    let text = ptr(&ed, &tool, v(6.0, -20.0));
    tool.handle(Input::Hover(text), &mut ed);
    tool.handle(Input::Click(text, Modifiers::default()), &mut ed);
    let (_, note) = ed.doc.annotations().next().expect("a note");
    let drift_core::Annotation::Note { leaders, .. } = note else {
        panic!("a note");
    };
    let tip = ed.doc.anchor_pos(&leaders[0].to).unwrap();
    assert!(matches!(leaders[0].to, drift_core::Anchor::Curve { .. }));
    assert!((tip - v(6.0, 0.0)).length() < 1e-9, "{tip:?}");
}

#[test]
fn the_note_tool_takes_several_leaders() {
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    let mut tool = NoteTool::default();
    // A corner, then Shift+another corner, then the text in open space.
    let first = ptr(&ed, &tool, v(0.0, 0.0));
    tool.handle(Input::Hover(first), &mut ed);
    tool.handle(Input::Click(first, Modifiers::default()), &mut ed);
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    let second = ptr(&ed, &tool, v(20.0, 0.0));
    tool.handle(Input::Hover(second), &mut ed);
    tool.handle(Input::Click(second, shift), &mut ed);
    assert!(tool.is_busy());
    let text = ptr(&ed, &tool, v(10.0, -15.0));
    tool.handle(Input::Hover(text), &mut ed);
    tool.handle(Input::Click(text, Modifiers::default()), &mut ed);
    let (_, note) = ed.doc.annotations().next().expect("a note");
    let drift_core::Annotation::Note { leaders, at, .. } = note else {
        panic!("a note");
    };
    assert_eq!(leaders.len(), 2);
    assert!(
        leaders
            .iter()
            .all(|l| matches!(l.to, drift_core::Anchor::Point(_)))
    );
    assert_eq!(ed.doc.note_text_pos(*at, leaders), Some(v(10.0, -15.0)));
    assert!(!tool.is_busy());
}

#[test]
fn the_fill_tools_eyedropper_picks_up_a_style() {
    use drift_core::FillKind;
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    if let FillKind::Hatch(h) = &mut ed.fill_style.kind {
        h.angle = 45.0;
    }
    let hatched = ed.fill_style.clone();
    ed.add_fill_at(v(5.0, 5.0)).unwrap();
    ed.fill_style.kind = FillKind::Solid;
    let mut tool = FillTool::default();
    // Alt+click a fill: its style, and nothing filled.
    let p = ptr(&ed, &tool, v(5.0, 5.0));
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    tool.handle(Input::Click(p, alt), &mut ed);
    assert_eq!(ed.fill_style, hatched);
    assert_eq!(ed.doc.fills().count(), 1);
    // The eyedropper button: the next click picks up a style, then filling resumes.
    ed.fill_style.kind = FillKind::Solid;
    ed.fill_eyedropper = true;
    click(&mut ed, &mut tool, v(500.0, 500.0));
    assert!(ed.notice.take().is_some(), "nothing there");
    assert!(ed.fill_eyedropper, "still picking");
    click(&mut ed, &mut tool, v(5.0, 5.0));
    assert_eq!(ed.fill_style, hatched);
    assert!(!ed.fill_eyedropper);
    // Escape leaves it.
    ed.fill_eyedropper = true;
    assert!(tool.cancel(&mut ed));
    assert!(!ed.fill_eyedropper);
}

/// A 20 × 10 rectangle at the origin with a tail line from its corner (20, 10) out to
/// (40, 10), and a hatch in the rectangle (picked).
fn hatched_rectangle_with_tail(auto_relations: bool) -> (Editor, EntityId) {
    let mut doc = Document::new();
    let p =
        [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)].map(|(x, y)| doc.add_point(v(x, y)));
    for i in 0..4 {
        doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
    }
    let end = doc.add_point(v(40.0, 10.0));
    let tail = doc.add_line(p[2], end).unwrap();
    let mut ed = Editor::new(doc);
    ed.auto_relations = auto_relations;
    ed.add_fill_at(v(5.0, 5.0)).unwrap();
    (ed, tail)
}

fn grab_by(ed: &mut Editor, offset: DVec2) {
    let mut tool = SelectTool::default();
    let start = ptr(ed, &tool, v(5.0, 5.0));
    assert!(tool.key(ToolKey::G, Some(&start), ed));
    tool.set_typed([Some(offset.x), Some(offset.y)]);
    tool.commit_typed(&start, ed);
}

fn tail_ends(ed: &Editor, tail: EntityId) -> (DVec2, DVec2) {
    match ed.doc.shape(tail) {
        Some(Shape::Line { a, b }) => (a, b),
        _ => panic!("the tail is a line"),
    }
}

#[test]
fn a_line_drawn_across_a_fill_leaves_it_whole() {
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    let fill = ed.add_fill_at(v(5.0, 5.0)).unwrap();
    let area = |ed: &mut Editor| ed.fill_shapes()[&fill].clone();
    assert!((area(&mut ed).region.area - 200.0).abs() < 1e-6);
    // A line right across it, past both sides.
    ed.doc.begin();
    let a = ed.doc.add_point(v(10.0, -5.0));
    let b = ed.doc.add_point(v(10.0, 15.0));
    let across = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    let shape = area(&mut ed);
    assert!(!shape.lost);
    assert!(
        (shape.region.area - 200.0).abs() < 1e-6,
        "{}",
        shape.region.area
    );
    // Its own outline still reshapes it; removing the line changes nothing.
    ed.doc.begin();
    ed.doc.delete([across]);
    ed.commit("Delete");
    assert!((area(&mut ed).region.area - 200.0).abs() < 1e-6);
}

#[test]
fn a_hatch_on_a_locked_outline_does_not_move_it() {
    let (mut ed, tail) = hatched_rectangle_with_tail(false);
    let layer = ed.add_layer("Walls");
    let curves: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(id, e)| !e.kind.is_point() && *id != tail)
        .map(|(id, _)| id)
        .collect();
    ed.doc.begin();
    for c in &curves {
        ed.doc.set_entity_layer(*c, layer).unwrap();
    }
    ed.commit("Layer");
    let mut locked = ed.doc.layer(layer).cloned().unwrap();
    locked.locked = true;
    ed.update_layer(layer, locked);
    let before: Vec<_> = ed.doc.iter().map(|(id, e)| (id, *e)).collect();
    // G refuses, and says why.
    grab_by(&mut ed, v(30.0, 0.0));
    assert!(ed.notice.take().is_some_and(|n| n.contains("locked")));
    // Dragging inside the picked hatch refuses too.
    let mut tool = SelectTool::default();
    tool.handle(
        Input::DragStart(ptr(&ed, &tool, v(5.0, 5.0)), Modifiers::default()),
        &mut ed,
    );
    tool.handle(Input::Drag(ptr(&ed, &tool, v(25.0, 5.0))), &mut ed);
    tool.handle(
        Input::DragEnd(ptr(&ed, &tool, v(25.0, 5.0)), Modifiers::default()),
        &mut ed,
    );
    let after: Vec<_> = ed.doc.iter().map(|(id, e)| (id, *e)).collect();
    assert_eq!(before, after, "nothing moved");
    assert!(ed.transform_box().is_none() || !ed.selection.is_empty());
}

#[test]
fn a_moved_hatch_lets_go_of_neighbours_with_auto_relations_off() {
    // Off: the rectangle moves, the tail stays where it was.
    let (mut ed, tail) = hatched_rectangle_with_tail(false);
    grab_by(&mut ed, v(0.0, 30.0));
    assert_eq!(tail_ends(&ed, tail), (v(20.0, 10.0), v(40.0, 10.0)));
    let shape = ed.fill_shapes().values().next().cloned().unwrap();
    assert!((shape.region.centroid - v(10.0, 35.0)).length() < 1e-6);
    // One undo puts it back, joined.
    ed.undo();
    let corner = ed.doc.iter().filter(|(_, e)| e.kind.is_point()).count();
    assert_eq!(corner, 5, "the split corner is gone again");
    // On: the join holds and the tail stretches.
    let (mut ed, tail) = hatched_rectangle_with_tail(true);
    grab_by(&mut ed, v(0.0, 30.0));
    let (a, b) = tail_ends(&ed, tail);
    assert!((a - v(20.0, 40.0)).length() < 1e-6 && b == v(40.0, 10.0));
}

#[test]
fn moving_a_picked_hatch_moves_its_area() {
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    ed.add_fill_at(v(5.0, 5.0)).unwrap();
    assert!(ed.selection.is_empty() && ed.selected_fills.len() == 1);
    let mut tool = SelectTool::default();
    // G with only the hatch picked moves the rectangle around it.
    let start = ptr(&ed, &tool, v(5.0, 5.0));
    assert!(tool.key(ToolKey::G, Some(&start), &mut ed));
    tool.set_typed([Some(30.0), Some(0.0)]);
    tool.commit_typed(&start, &mut ed);
    let xs: Vec<f64> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Point(p) => Some(p.x),
            _ => None,
        })
        .collect();
    assert!(xs.iter().all(|x| *x >= 30.0 - 1e-9), "{xs:?}");
    let shape = ed.fill_shapes().values().next().cloned().unwrap();
    assert!(!shape.lost && (shape.region.centroid.x - 40.0).abs() < 1e-6);
    // Dragging inside the picked hatch moves it too.
    let from = ptr(&ed, &tool, v(40.0, 5.0));
    tool.handle(Input::DragStart(from, Modifiers::default()), &mut ed);
    tool.handle(Input::Drag(ptr(&ed, &tool, v(40.0, 25.0))), &mut ed);
    tool.handle(
        Input::DragEnd(ptr(&ed, &tool, v(40.0, 25.0)), Modifiers::default()),
        &mut ed,
    );
    let shape = ed.fill_shapes().values().next().cloned().unwrap();
    assert!(
        (shape.region.centroid.y - 25.0).abs() < 1e-6,
        "{:?}",
        shape.region.centroid
    );
}

#[test]
fn rotate_pivot_from_b_and_absolute_angles() {
    let (mut ed, ids) = rotated_rectangle_fixture();
    let mut tool = SelectTool::default();
    // R with the pointer anywhere: nothing turns until the pointer moves, and then the angle
    // is the pointer's direction from 0° (here straight right of the centre: no turn).
    let start = ptr(&ed, &tool, v(13.0, 27.0));
    tool.key(ToolKey::R, Some(&start), &mut ed);
    assert_eq!(tool.spin.as_ref().unwrap().pivot, v(10.0, 5.0));
    tool.handle(Input::Hover(ptr(&ed, &tool, v(40.0, 5.5))), &mut ed);
    let flat: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    assert!(
        flat.contains(&v(20.0, 10.0)),
        "near 0° sticks to 0°: {flat:?}"
    );
    // B, then pick the corner (0, 0) as the pivot; point straight up: a quarter turn.
    tool.key(ToolKey::B, None, &mut ed);
    tool.handle(Input::Hover(ptr(&ed, &tool, v(0.0, 0.0))), &mut ed);
    tool.handle(
        Input::Click(ptr(&ed, &tool, v(0.0, 0.0)), Modifiers::default()),
        &mut ed,
    );
    tool.handle(Input::Hover(ptr(&ed, &tool, v(0.0, 30.0))), &mut ed);
    tool.handle(
        Input::Click(ptr(&ed, &tool, v(0.0, 30.0)), Modifiers::default()),
        &mut ed,
    );
    let turned: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    assert!(
        turned.iter().any(|p| p.distance(v(0.0, 20.0)) < 1e-6)
            && turned.iter().any(|p| p.distance(v(-10.0, 20.0)) < 1e-6),
        "{turned:?}"
    );
}

#[test]
fn a_rotates_new_pivot_snaps_to_geometry_and_the_grid() {
    let (mut ed, _) = rotated_rectangle_fixture();
    let mut tool = SelectTool::default();
    let start = ptr(&ed, &tool, v(13.0, 27.0));
    tool.key(ToolKey::R, Some(&start), &mut ed);
    // Near a corner of the selection: onto the corner.
    tool.key(ToolKey::B, None, &mut ed);
    let near = ptr(&ed, &tool, v(0.3, -0.2));
    tool.handle(Input::Hover(near), &mut ed);
    tool.handle(Input::Click(near, Modifiers::default()), &mut ed);
    assert_eq!(tool.spin.as_ref().unwrap().pivot, v(0.0, 0.0));
    // In open space with grid snapping: onto the grid.
    tool.key(ToolKey::B, None, &mut ed);
    let open = ptr_grid(&ed, &tool, v(31.2, 18.6), Some(5.0));
    tool.handle(Input::Hover(open), &mut ed);
    tool.handle(Input::Click(open, Modifiers::default()), &mut ed);
    assert_eq!(tool.spin.as_ref().unwrap().pivot, v(30.0, 20.0));
    // Turning follows the pointer itself, not the grid.
    let turn = ptr_grid(&ed, &tool, v(31.0, 31.0), Some(5.0));
    tool.handle(Input::Hover(turn), &mut ed);
    let angle = tool.spin.as_ref().unwrap().angle;
    let raw = DVec2::X.angle_to(v(1.0, 11.0)).to_degrees();
    assert!((angle - raw).abs() < 1e-9, "{angle} vs {raw}");
}

#[test]
fn rotate_by_reference_turns_an_edge_onto_a_line() {
    // The 20 × 10 rectangle, and a target line from its corner (0, 0) out to (-10, 40).
    let (mut ed, ids) = rotated_rectangle_fixture();
    ed.doc.begin();
    let o = ed.doc.add_point(v(0.0, -0.0001));
    let far = ed.doc.add_point(v(-10.0, 40.0));
    ed.doc.add_line(o, far).unwrap();
    ed.commit("Target");
    let mut tool = SelectTool::default();
    let start = ptr(&ed, &tool, v(13.0, 27.0));
    tool.key(ToolKey::R, Some(&start), &mut ed);
    // B: the pivot at the corner.
    tool.key(ToolKey::B, None, &mut ed);
    let at = ptr(&ed, &tool, v(0.2, 0.2));
    tool.handle(Input::Click(at, Modifiers::default()), &mut ed);
    assert_eq!(tool.spin.as_ref().unwrap().pivot, v(0.0, 0.0));
    // R again: the reference is the far corner (20, 10), along the diagonal.
    assert!(tool.key(ToolKey::R, None, &mut ed));
    assert!(tool.spin.as_ref().unwrap().picking_reference);
    let reference = ptr(&ed, &tool, v(19.8, 10.1));
    tool.handle(Input::Hover(reference), &mut ed);
    tool.handle(Input::Click(reference, Modifiers::default()), &mut ed);
    // Turn onto the target line's end: the diagonal now lies along the line.
    let target = ptr(&ed, &tool, v(-9.9, 39.8));
    tool.handle(Input::Hover(target), &mut ed);
    tool.handle(Input::Click(target, Modifiers::default()), &mut ed);
    let corner = v(-10.0, 40.0).normalize() * v(20.0, 10.0).length();
    let points: Vec<DVec2> = ids.iter().filter_map(|id| ed.doc.point(*id).ok()).collect();
    assert!(
        points.iter().any(|p| p.distance(corner) < 1e-6),
        "{corner:?} in {points:?}"
    );
}

#[test]
fn clicking_inside_a_closed_area_picks_its_outline() {
    let (mut ed, _) = rotated_rectangle_fixture();
    ed.selection.clear();
    let lines: BTreeSet<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .collect();
    let mut tool = SelectTool::default();
    click(&mut ed, &mut tool, v(5.0, 5.0));
    assert!(ed.selected_region.is_some());
    assert_eq!(ed.selection, lines);
    // Moving it takes the region along.
    let start = ptr(&ed, &tool, v(5.0, 5.0));
    tool.key(ToolKey::G, Some(&start), &mut ed);
    tool.set_typed([Some(0.0), Some(50.0)]);
    tool.commit_typed(&start, &mut ed);
    let r = ed.region().expect("still picked");
    assert!(
        (r.centroid - v(10.0, 55.0)).length() < 1e-6,
        "{:?}",
        r.centroid
    );
}

#[test]
fn tab_selects_a_chain_of_connected_curves() {
    let mut ed = Editor::default();
    let mut line = LineTool::default();
    // An L-shaped polyline, a separate line, and a circle touching the end.
    for at in [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)] {
        click(&mut ed, &mut line, at);
    }
    line.handle(Input::Finish, &mut ed);
    for at in [v(30.0, 0.0), v(40.0, 0.0)] {
        click(&mut ed, &mut line, at);
    }
    line.handle(Input::Finish, &mut ed);
    let lines: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(lines.len(), 3);
    let c = ed.doc.add_point(v(10.0, 10.0));
    ed.doc.add_circle(c, 2.0).unwrap();
    ed.sync();
    // Pointing at the first segment, Tab picks both segments of the L (not the others).
    ed.hovered = Some(lines[0]);
    assert_eq!(ed.select_chain(), 2);
    assert_eq!(ed.selection, [lines[0], lines[1]].into());
    // With nothing under the pointer, it widens the selection instead.
    ed.hovered = None;
    ed.selection = [lines[2]].into();
    assert_eq!(ed.select_chain(), 1);
}

#[test]
fn drawing_inside_an_open_group_adds_to_it() {
    let (mut ed, _) = rotated_rectangle_fixture();
    let g = ed.make_group().expect("grouped");
    let before = ed.doc.group(g).unwrap().members.len();
    // Open the group and draw a line and a point: both join it, in the same undo step.
    ed.open_group = Some(g);
    let mut line = LineTool::default();
    click(&mut ed, &mut line, v(2.0, 2.0));
    click(&mut ed, &mut line, v(8.0, 8.0));
    line.handle(Input::Finish, &mut ed);
    let mut point = PointTool::default();
    click(&mut ed, &mut point, v(15.0, 3.0));
    let members = ed.doc.group(g).unwrap().members.clone();
    assert_eq!(members.len(), before + 2, "{members:?}");
    // The line's own end points aren't members; the standalone point is.
    assert!(
        members
            .iter()
            .filter(|id| ed.doc.get(**id).is_some_and(|e| e.kind.is_point()))
            .count()
            == 1
    );
    ed.undo();
    assert_eq!(ed.doc.group(g).unwrap().members.len(), before + 1);
    // Closed again, new geometry stays out of it.
    ed.open_group = None;
    let mut line = LineTool::default();
    click(&mut ed, &mut line, v(50.0, 0.0));
    click(&mut ed, &mut line, v(60.0, 0.0));
    assert_eq!(ed.doc.group(g).unwrap().members.len(), before + 1);
}

#[test]
fn duplicate_copies_curves_and_their_relations_then_grabs_the_copy() {
    let (mut ed, line, [a, b], _) = move_fixture();
    ed.doc.begin();
    ed.doc
        .add_constraint(drift_core::Constraint::Horizontal { line })
        .unwrap();
    // A fixed original: its copy isn't pinned there too.
    ed.doc
        .add_constraint(drift_core::Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        })
        .unwrap();
    ed.commit("Relate");
    ed.selection = [line].into();
    assert_eq!(ed.duplicate_selection(), 1);
    assert_eq!(lines(&ed.doc), 2);
    assert_eq!(
        ed.doc.constraints().count(),
        3,
        "the relation came along, not the fix"
    );
    let copy = *ed.selection.iter().next().unwrap();
    assert_ne!(copy, line);
    let mut tool = SelectTool::default();
    let p = ptr(&ed, &tool, v(5.0, 0.0));
    assert!(tool.key(ToolKey::G, Some(&p), &mut ed));
    click(&mut ed, &mut tool, v(5.0, 40.0));
    // The original stays; the copy moved.
    assert_eq!(ed.doc.point(a).unwrap(), v(0.0, 0.0));
    assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 0.0));
    let EntityKind::Line { a: ca, .. } = ed.doc.get(copy).unwrap().kind else {
        panic!()
    };
    assert_eq!(ed.doc.point(ca).unwrap(), v(0.0, 40.0));
    // One undo step removes the duplicate entirely.
    ed.undo();
    ed.undo();
    assert_eq!(lines(&ed.doc), 1);
}

#[test]
fn duplicate_brings_the_fill_of_a_copied_area_and_it_follows_the_copy() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    let edges: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap())
        .collect();
    ed.commit("Setup");
    let fill = ed.add_fill_at(v(5.0, 3.0)).unwrap();
    // Drawing on another layer now: the copy stays on the fill's own layer.
    let other = ed.add_layer("Other");
    ed.set_current_layer(other);
    // The fill and its outline are picked, as clicking the hatched area does.
    ed.selection = edges.iter().copied().collect();
    ed.selected_fills = [fill].into();
    ed.duplicate_selection();
    assert_eq!(ed.doc.fills().count(), 2);
    let copy = *ed.selected_fills.iter().next().unwrap();
    assert_ne!(copy, fill);
    let mut tool = SelectTool::default();
    let p = ptr(&ed, &tool, v(5.0, 3.0));
    assert!(tool.key(ToolKey::G, Some(&p), &mut ed));
    click(&mut ed, &mut tool, v(45.0, 3.0));
    let shapes = ed.fill_shapes();
    assert!(
        shapes[&fill].region.contains(v(5.0, 3.0)),
        "the original stays"
    );
    assert!(!shapes[&copy].lost);
    assert!(
        shapes[&copy].region.contains(v(45.0, 3.0)),
        "the copy's fill moved"
    );
    assert_eq!(
        ed.doc.fill(copy).unwrap().layer,
        ed.doc.fill(fill).unwrap().layer
    );
}

#[test]
fn toolbar_sections_list_every_tool_once() {
    let listed: Vec<ToolKind> = ToolKind::SECTIONS
        .iter()
        .flat_map(|(_, tools)| tools.iter().copied())
        .collect();
    assert_eq!(listed.len(), ToolKind::ALL.len());
    for kind in ToolKind::ALL {
        assert_eq!(listed.iter().filter(|k| **k == kind).count(), 1, "{kind:?}");
    }
}

fn arcs(doc: &Document) -> usize {
    count(doc, |k| matches!(k, EntityKind::Arc { .. }))
}

#[test]
fn polygon_is_regular_with_four_freedoms() {
    let mut ed = with_auto_relations();
    let mut tool = PolygonTool::default();
    // Default: six sides, inscribed; the pointer off any 15° step.
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let corner = v(10.0, 0.0).rotate(DVec2::from_angle(0.3));
    click(&mut ed, &mut tool, corner);
    assert_eq!(lines(&ed.doc), 6);
    assert_eq!(ed.analysis().dof, Some(4), "centre, size and turn");
    let circle = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .map(|(id, e)| (id, e.style))
        .unwrap();
    assert_eq!(circle.1, drift_core::LineKind::Construction);
    // Every side is the same length and every corner on the circle.
    let shapes: Vec<_> = ed
        .doc
        .iter()
        .filter_map(|(id, _)| ed.doc.shape(id))
        .collect();
    for s in &shapes {
        if let Shape::Line { a, b } = s {
            assert!((a.distance(*b) - 10.0).abs() < 1e-6);
            assert!((a.length() - 10.0).abs() < 1e-6);
        }
    }

    // Circumscribed square with a typed radius (the apothem): sides 20 long, dimensioned.
    let mut ed = Editor::default();
    ed.auto_relations = true;
    ed.polygon_circumscribed = true;
    let mut tool = PolygonTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let p = ptr(&ed, &tool, v(0.0, 7.0));
    tool.handle(Input::Hover(p), &mut ed);
    tool.set_typed([Some(10.0), Some(4.0)]);
    tool.commit_typed(&p, &mut ed);
    assert_eq!(lines(&ed.doc), 4);
    assert_eq!(ed.polygon_sides, 4, "remembered");
    for (id, _) in ed.doc.iter() {
        if let Some(Shape::Line { a, b }) = ed.doc.shape(id) {
            assert!((a.distance(b) - 20.0).abs() < 1e-6);
        }
    }
    // Straight up from the centre: the top side is level, so it gets Horizontal.
    let kinds: Vec<_> = ed.doc.constraints().map(|(_, c)| c.type_name()).collect();
    assert_eq!(
        ed.analysis().dof,
        Some(2),
        "only the centre moves: {kinds:?}"
    );
}

#[test]
fn slots_keep_their_shape_and_freedoms() {
    // Straight: two end centres, then the width.
    let mut ed = with_auto_relations();
    let mut tool = SlotTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(30.0, 10.0));
    click(&mut ed, &mut tool, v(15.0, 9.0));
    assert_eq!(
        (lines(&ed.doc), arcs(&ed.doc)),
        (3, 2),
        "two sides, an axis, two ends"
    );
    assert_eq!(ed.analysis().dof, Some(5), "two centres and a width");

    // Centre slot, horizontal, typed length and width: only the middle moves.
    let mut ed = with_auto_relations();
    ed.auto_relations = true;
    ed.slot_mode = SlotMode::Centre;
    let mut tool = SlotTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    let p = ptr(&ed, &tool, v(20.0, 0.0));
    tool.handle(Input::Hover(p), &mut ed);
    tool.set_typed([Some(40.0), None]);
    tool.commit_typed(&p, &mut ed);
    let p = ptr(&ed, &tool, v(5.0, 3.0));
    tool.handle(Input::Hover(p), &mut ed);
    tool.set_typed([Some(8.0), None]);
    tool.commit_typed(&p, &mut ed);
    assert_eq!((lines(&ed.doc), arcs(&ed.doc)), (3, 2));
    assert_eq!(ed.analysis().dof, Some(2), "{:?}", ed.analysis());
    let ends: Vec<DVec2> = ed
        .doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Arc { center, .. } => ed.doc.point(center).ok().map(|c| (id, c)),
            _ => None,
        })
        .map(|(_, c)| c)
        .collect();
    assert!(
        ends.iter().any(|c| c.distance(v(-20.0, 0.0)) < 1e-6),
        "{ends:?}"
    );
    assert!(
        ends.iter().any(|c| c.distance(v(20.0, 0.0)) < 1e-6),
        "{ends:?}"
    );

    // Arc slot: centre, start, end (drawn clockwise), width.
    let mut ed = with_auto_relations();
    ed.slot_mode = SlotMode::Arc;
    let mut tool = SlotTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(0.0, 50.0));
    click(&mut ed, &mut tool, v(40.0, 30.0));
    click(&mut ed, &mut tool, v(0.0, 54.0));
    assert_eq!((lines(&ed.doc), arcs(&ed.doc)), (0, 4));
    assert_eq!(
        ed.analysis().dof,
        Some(6),
        "centre, radius, two angles and a width"
    );
    // The short way round: no arc sweeps more than half a turn.
    for (id, _) in ed.doc.iter() {
        if let Some(Shape::Arc { sweep, .. }) = ed.doc.shape(id) {
            assert!(sweep <= PI + 1e-9, "{sweep}");
        }
    }
}

#[test]
fn ellipses_draw_snap_and_fill() {
    let mut ed = Editor::default();
    ed.auto_relations = true;
    let mut tool = EllipseTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(20.0, 0.0));
    click(&mut ed, &mut tool, v(7.0, 10.0));
    let (id, shape) = ed
        .doc
        .iter()
        .find_map(|(id, e)| {
            matches!(e.kind, EntityKind::Ellipse { .. }).then(|| (id, ed.doc.shape(id).unwrap()))
        })
        .expect("an ellipse");
    let Shape::Ellipse {
        major,
        minor,
        sweep,
        ..
    } = shape
    else {
        panic!()
    };
    assert_eq!((major, minor, sweep), (v(20.0, 0.0), 10.0, TAU));
    // Five freedoms, less one for the level axis.
    assert_eq!(ed.analysis().dof, Some(4));
    // The far end of the minor axis snaps (as a quadrant of the ellipse).
    let p = ptr(&ed, &tool, v(0.1, -9.9));
    let s = p.snap.unwrap();
    assert_eq!(
        (s.kind, s.entity),
        (drift_geom::SnapKind::Quadrant, Some(id))
    );
    assert!(s.at.distance(v(0.0, -10.0)) < 1e-9);
    // Fill inside it.
    let fill = ed
        .add_fill_at(v(5.0, 2.0))
        .expect("an area inside the ellipse");
    let shapes = ed.fill_shapes();
    assert!(!shapes[&fill].lost);
    assert!(
        !shapes[&fill].region.contains(v(19.0, 9.0)),
        "outside the curve"
    );

    // A partial ellipse, counter-clockwise from t = 0.3 to t = 2.5 (clicks on the curve).
    let mut ed = Editor::default();
    let mut tool = EllipseTool::partial();
    let on = |t: f64| v(30.0 * t.cos(), 12.0 * t.sin());
    for at in [v(0.0, 0.0), v(30.0, 0.0), v(0.0, 12.0), on(0.3), on(2.5)] {
        click(&mut ed, &mut tool, at);
    }
    let shape = ed
        .doc
        .iter()
        .find_map(|(id, e)| {
            matches!(e.kind, EntityKind::EllipseArc { .. }).then(|| ed.doc.shape(id).unwrap())
        })
        .expect("an elliptical arc");
    let Shape::Ellipse { start, sweep, .. } = shape else {
        panic!()
    };
    assert!(
        (start - 0.3).abs() < 1e-9 && (sweep - 2.2).abs() < 1e-9,
        "{start} {sweep}"
    );
    assert_eq!(ed.analysis().dof, Some(7));
}

fn beziers(doc: &Document) -> Vec<[DVec2; 4]> {
    doc.iter()
        .filter_map(|(id, _)| match doc.shape(id) {
            Some(Shape::Bezier { p }) => Some(p),
            _ => None,
        })
        .collect()
}

/// Press at `at`, drag to `to` (Alt held if `alt`), release.
fn pen_drag(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2, to: DVec2, alt: bool) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::DragStart(p, Modifiers::default()), ed);
    let mut q = Pointer::new(to, TOL);
    q.alt = alt;
    tool.handle(Input::Drag(q), ed);
    tool.handle(Input::DragEnd(q, Modifiers::default()), ed);
}

#[test]
fn pen_paths_chain_with_tab_and_their_areas_move() {
    // A closed pen path: a corner, a smooth point dragged out, another, and back to the start.
    let mut ed = Editor::default();
    ed.regions_need_fill = false;
    let mut tool = PenTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    pen_drag(&mut ed, &mut tool, v(20.0, 12.0), v(28.0, 12.0), false);
    pen_drag(&mut ed, &mut tool, v(40.0, 0.0), v(40.0, -6.0), false);
    click(&mut ed, &mut tool, v(0.1, 0.1));
    assert!(!tool.is_busy(), "closed");
    let curves: BTreeSet<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    assert!(beziers(&ed.doc).len() >= 2);
    // Tab's chain: the whole path, from any of its curves.
    for c in &curves {
        assert_eq!(&ed.chain_of(*c), &curves);
    }
    // Pick the area inside it, then G: it moves.
    let mut select = SelectTool::default();
    click(&mut ed, &mut select, v(20.0, 4.0));
    let region = ed.region().expect("the area is picked");
    assert_eq!(
        ed.region_curves(&region)
            .into_iter()
            .collect::<BTreeSet<_>>(),
        curves
    );
    let start = ptr(&ed, &select, v(20.0, 4.0));
    assert!(select.key(ToolKey::G, Some(&start), &mut ed));
    select.set_typed([Some(0.0), Some(30.0)]);
    select.commit_typed(&start, &mut ed);
    let ys: Vec<f64> = ed
        .doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Point(p) => Some(p.y),
            _ => None,
        })
        .collect();
    assert!(ys.iter().all(|y| *y >= 23.0), "{ys:?}");
}

#[test]
fn pen_clicks_make_corners_and_drags_make_smooth_points() {
    let mut ed = Editor::default();
    let mut tool = PenTool::default();
    // Corner, smooth (dragged right), corner.
    click(&mut ed, &mut tool, v(0.0, 0.0));
    pen_drag(&mut ed, &mut tool, v(20.0, 10.0), v(28.0, 10.0), false);
    click(&mut ed, &mut tool, v(40.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    let curves = beziers(&ed.doc);
    assert_eq!(curves.len(), 2);
    assert_eq!(lines(&ed.doc), 0);
    // The first curve arrives along the mirrored handle; the second leaves along the drag.
    let first = curves.iter().find(|p| p[0] == v(0.0, 0.0)).unwrap();
    let second = curves.iter().find(|p| p[0] == v(20.0, 10.0)).unwrap();
    assert_eq!(first[2], v(12.0, 10.0));
    assert_eq!(second[1], v(28.0, 10.0));
    assert_eq!(first[3], second[0], "they share the point");
    let smooth = ed
        .doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Smooth { .. }))
        .count();
    assert_eq!(smooth, 1);
    assert!(!tool.is_busy());

    // Straight clicks are lines; clicking the first point closes the path.
    let mut ed = Editor::default();
    let mut tool = PenTool::default();
    for at in [v(0.0, 0.0), v(30.0, 0.0), v(15.0, 20.0), v(0.1, 0.1)] {
        click(&mut ed, &mut tool, at);
    }
    assert_eq!(lines(&ed.doc), 3);
    assert_eq!(
        count(&ed.doc, |k| k.is_point()),
        3,
        "closed onto the first point"
    );
    assert!(!tool.is_busy(), "closing ends the path");
}

#[test]
fn pen_alt_breaks_handles_and_toggles_points() {
    let mut ed = Editor::default();
    let mut tool = PenTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    // Alt during the drag: the incoming handle stays where it was, a corner.
    pen_drag(&mut ed, &mut tool, v(20.0, 0.0), v(20.0, 10.0), true);
    click(&mut ed, &mut tool, v(40.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    assert_eq!(
        beziers(&ed.doc).len(),
        1,
        "the first segment has no handles: a line"
    );
    assert_eq!(lines(&ed.doc), 1);
    assert!(
        ed.doc
            .constraints()
            .all(|(_, c)| !matches!(c, Constraint::Smooth { .. })),
        "a broken handle isn't smooth"
    );

    // An S-curve's middle point: Alt-click makes it a corner, and again smooth.
    let mut ed = Editor::default();
    let mut tool = PenTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    pen_drag(&mut ed, &mut tool, v(20.0, 0.0), v(25.0, 5.0), false);
    click(&mut ed, &mut tool, v(40.0, 0.0));
    tool.handle(Input::Finish, &mut ed);
    let middle = ed
        .doc
        .iter()
        .find(|(_, e)| e.kind == EntityKind::Point(v(20.0, 0.0)))
        .unwrap()
        .0;
    assert!(ed.smooth_at(middle).is_some());
    let mut p = ptr(&ed, &tool, v(20.0, 0.0));
    p.alt = true;
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    assert!(ed.smooth_at(middle).is_none());
    tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
    assert!(ed.smooth_at(middle).is_some());
}

/// Click at `at` as the canvas would: snapped, then steered by the tool, with the document's
/// angle step.
fn click_steered(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2, step: Option<f64>) {
    let mut p = ptr(ed, tool, at);
    p.angle_step = step;
    tool.refine_pointer(&mut p);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

fn last_mark(ed: &Editor) -> drift_core::Annotation {
    ed.doc.annotations().last().map(|(_, a)| a.clone()).unwrap()
}

#[test]
fn section_lines_follow_axis_locks_and_angle_snap() {
    use super::annotate::SectionTool;
    use drift_core::Annotation;
    let mut ed = Editor::default();
    let mut tool = SectionTool::default();
    let ends = |ed: &Editor| match last_mark(ed) {
        Annotation::Section { a, b, .. } => (a, b),
        other => panic!("{other:?}"),
    };
    // X before the first click isn't claimed (it keeps its usual meaning).
    assert!(!tool.key(ToolKey::X, None, &mut ed));
    // X: the cutting line runs along X, wherever the pointer is.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    assert!(tool.key(ToolKey::X, None, &mut ed));
    assert_eq!(tool.axis_lock(), Some((v(0.0, 0.0), Axis::X)));
    click_steered(&mut ed, &mut tool, v(40.0, 13.0), None);
    assert_eq!(ends(&ed), (v(0.0, 0.0), v(40.0, 0.0)));
    assert_eq!(tool.axis_lock(), None, "the lock ends with the mark");
    // Y.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    tool.key(ToolKey::Y, None, &mut ed);
    click_steered(&mut ed, &mut tool, v(7.0, -30.0), None);
    assert_eq!(ends(&ed), (v(0.0, 0.0), v(0.0, -30.0)));
    // With angle snapping at 45°, a cut drawn at about 40° lands on 45°.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), Some(45.0));
    click_steered(&mut ed, &mut tool, v(30.0, 25.0), Some(45.0));
    let (a, b) = ends(&ed);
    let d = b - a;
    assert!((d.y.atan2(d.x).to_degrees() - 45.0).abs() < 1e-9, "{d:?}");
    // Without it, the cut goes where it is clicked.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    click_steered(&mut ed, &mut tool, v(30.0, 25.0), None);
    assert_eq!(ends(&ed), (v(0.0, 0.0), v(30.0, 25.0)));
}

#[test]
fn elevations_aim_along_axis_locks_and_angle_snap() {
    use super::annotate::ElevationTool;
    use drift_core::Annotation;
    let mut ed = Editor::default();
    let mut tool = ElevationTool::default();
    let angle = |ed: &Editor| match last_mark(ed) {
        Annotation::Elevation { angle, .. } => angle,
        other => panic!("{other:?}"),
    };
    // Y: it looks straight down, though aimed well off it.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    assert!(tool.key(ToolKey::Y, None, &mut ed));
    click_steered(&mut ed, &mut tool, v(9.0, -20.0), None);
    assert_eq!(angle(&ed), -90.0);
    // X, aimed left.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    tool.key(ToolKey::X, None, &mut ed);
    click_steered(&mut ed, &mut tool, v(-20.0, 6.0), None);
    assert_eq!(angle(&ed).abs(), 180.0);
    // The document's angle step: 30° steps take 40° to 30°.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), Some(30.0));
    let toward = DVec2::from_angle(40f64.to_radians()) * 20.0;
    click_steered(&mut ed, &mut tool, toward, Some(30.0));
    assert!((angle(&ed) - 30.0).abs() < 1e-6, "{}", angle(&ed));
    // With angle snapping off, it aims where it is pointed.
    click_steered(&mut ed, &mut tool, v(0.0, 0.0), None);
    click_steered(&mut ed, &mut tool, toward, None);
    assert!((angle(&ed) - 40.0).abs() < 0.05, "{}", angle(&ed));
}

#[test]
fn a_fixed_panel_stretches_by_drag_and_by_dimension() {
    use super::rectangle::RectangleTool;
    // A 4 ft × 8 ft 11 1/16 in panel drawn from the origin (horizontal and vertical sides),
    // its corner fixed there and its height dimensioned.
    let mut ed = with_auto_relations();
    let mut tool = RectangleTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(1219.2, 2725.7));
    let point_at = |ed: &Editor, p: DVec2| {
        ed.doc
            .iter()
            .find(|(id, e)| e.kind.is_point() && ed.doc.point(*id).unwrap().distance(p) < 1e-6)
            .unwrap()
            .0
    };
    let (a, b, c, d) = (
        point_at(&ed, v(0.0, 0.0)),
        point_at(&ed, v(1219.2, 0.0)),
        point_at(&ed, v(1219.2, 2725.7)),
        point_at(&ed, v(0.0, 2725.7)),
    );
    let line = |ed: &Editor, p: EntityId, q: EntityId| {
        ed.doc
            .iter()
            .find(|(_, e)| {
                matches!(e.kind, EntityKind::Line { a, b } if (a, b) == (p, q) || (a, b) == (q, p))
            })
            .unwrap()
            .0
    };
    ed.add_constraints(
        "Fix",
        &[Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        }],
    )
    .unwrap();
    ed.add_constraints(
        "Height",
        &[Constraint::Distance {
            a: d,
            b: a,
            value: 2725.7,
        }],
    )
    .unwrap();

    // Dragging the right side half a metre in one go, the pointer also 150 mm higher (as a
    // snap to a coarse imperial grid can take it): the side can only slide sideways, and
    // does.
    let right = line(&ed, b, c);
    let mut select = SelectTool::default();
    let m = Modifiers::default();
    let start = ptr(&ed, &select, v(1219.2, 1200.0));
    select.handle(Input::DragStart(start, m), &mut ed);
    let to = ptr(&ed, &select, v(1719.2, 1350.0));
    select.handle(Input::Drag(to), &mut ed);
    select.handle(Input::DragEnd(to, m), &mut ed);
    assert!(
        (ed.doc.point(b).unwrap().x - 1719.2).abs() < 1e-6,
        "{:?}",
        ed.doc.point(b)
    );
    assert_eq!(
        ed.doc.point(a).unwrap(),
        v(0.0, 0.0),
        "the fixed corner stays"
    );
    let _ = right;

    // A width dimension of 1500 takes it there.
    ed.add_constraints(
        "Width",
        &[Constraint::HorizontalDistance {
            a,
            b,
            value: 1500.0,
        }],
    )
    .expect("a width from the fixed corner");
    assert!((ed.doc.point(c).unwrap().x - 1500.0).abs() < 1e-6);
    assert!((ed.doc.point(c).unwrap().y - 2725.7).abs() < 1e-6);
}

#[test]
fn collinear_between_level_lines_is_not_taken_as_implied() {
    use super::rectangle::RectangleTool;
    use crate::editor::EditError;
    // Two rectangles with level sides, the left one held: its corner fixed and its height
    // dimensioned. Their tops are level but at different heights.
    let mut ed = with_auto_relations();
    let mut tool = RectangleTool::default();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(40.0, 30.0));
    let mut tool = RectangleTool::default();
    click(&mut ed, &mut tool, v(60.0, 0.0));
    click(&mut ed, &mut tool, v(90.0, 50.0));
    let point_at = |ed: &Editor, p: DVec2| {
        ed.doc
            .iter()
            .find(|(id, e)| e.kind.is_point() && ed.doc.point(*id).unwrap().distance(p) < 1e-6)
            .unwrap()
            .0
    };
    let line_between = |ed: &Editor, p: EntityId, q: EntityId| {
        ed.doc
            .iter()
            .find(|(_, e)| {
                matches!(e.kind, EntityKind::Line { a, b } if (a, b) == (p, q) || (a, b) == (q, p))
            })
            .unwrap()
            .0
    };
    let (a, d) = (point_at(&ed, v(0.0, 0.0)), point_at(&ed, v(0.0, 30.0)));
    let left_top = line_between(&ed, point_at(&ed, v(40.0, 30.0)), d);
    let right_top = line_between(
        &ed,
        point_at(&ed, v(90.0, 50.0)),
        point_at(&ed, v(60.0, 50.0)),
    );
    ed.add_constraints(
        "Fix",
        &[Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        }],
    )
    .unwrap();
    ed.add_constraints(
        "Height",
        &[Constraint::Distance {
            a: d,
            b: a,
            value: 30.0,
        }],
    )
    .unwrap();
    let collinear = Constraint::Collinear {
        a: left_top,
        b: right_top,
    };
    ed.add_constraints("Collinear", &[collinear])
        .expect("collinear holds something new: the heights");
    let p = point_at(&ed, v(90.0, 30.0));
    assert!((ed.doc.point(p).unwrap().y - 30.0).abs() < 1e-6);
    // Now it is implied.
    assert!(matches!(
        ed.add_constraints("Collinear", &[collinear]),
        Err(EditError::Redundant)
    ));
}

#[test]
fn selection_filters_pick_only_edges_or_only_points() {
    use crate::editor::PickFilter;
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
    let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
    ed.commit("Setup");
    let mut tool = SelectTool::default();
    let m = Modifiers::default();
    let boxed = |ed: &mut Editor, tool: &mut SelectTool| {
        ed.selection.clear();
        let start = ptr(ed, tool, v(-2.0, -2.0));
        tool.handle(Input::DragStart(start, m), ed);
        let end = ptr(ed, tool, v(12.0, 8.0));
        tool.handle(Input::Drag(end), ed);
        tool.handle(Input::DragEnd(end, m), ed);
        ed.selection.clone()
    };
    // Anything: a box takes the curves (their end points come with them).
    assert_eq!(boxed(&mut ed, &mut tool), l.iter().copied().collect());
    // Edges only: the box takes the four lines; a click on a corner takes a line, not the
    // point; a click inside picks no area.
    ed.pick_filter = PickFilter::Edges;
    let picked = boxed(&mut ed, &mut tool);
    assert_eq!(picked, l.iter().copied().collect());
    ed.selection.clear();
    click(&mut ed, &mut tool, v(0.0, 0.0));
    assert!(ed.selection.len() == 1 && l.iter().any(|id| ed.selection.contains(id)));
    click(&mut ed, &mut tool, v(5.0, 3.0));
    assert!(ed.selected_region.is_none(), "no areas with a filter on");
    // Points only: the box takes the four corners; a click on a line takes nothing.
    ed.pick_filter = PickFilter::Points;
    let picked = boxed(&mut ed, &mut tool);
    assert_eq!(picked, c.iter().copied().collect());
    ed.selection.clear();
    click(&mut ed, &mut tool, v(5.0, 0.0));
    assert!(ed.selection.is_empty());
    click(&mut ed, &mut tool, v(10.0, 6.0));
    assert_eq!(ed.selection, [c[2]].into());
    // A grouped rectangle gives only what the filter allows.
    ed.selection = l.iter().copied().collect();
    ed.pick_filter = PickFilter::All;
    ed.make_group();
    ed.selection.clear();
    ed.pick_filter = PickFilter::Edges;
    click(&mut ed, &mut tool, v(5.0, 0.0));
    assert_eq!(ed.selection, l.iter().copied().collect());
}

#[test]
fn chain_mode_runs_dimensions_along_a_row_and_ends_with_an_overall() {
    use super::dimension::DimensionTool;
    use crate::editor::DimMode;
    // Three wall ends along y = 0, and a wall standing at x = 40.
    let mut ed = Editor::default();
    ed.doc.begin();
    let p = [0.0, 10.0, 25.0].map(|x| ed.doc.add_point(v(x, 0.0)));
    let (foot, top) = (
        ed.doc.add_point(v(40.0, 0.0)),
        ed.doc.add_point(v(40.0, 20.0)),
    );
    ed.doc.add_line(foot, top).unwrap();
    ed.commit("Setup");
    let rows = |ed: &Editor| -> Vec<(EntityId, EntityId, f64, bool)> {
        ed.doc
            .constraints()
            .filter_map(|(id, c)| match *c {
                Constraint::HorizontalDistance { a, b, value } => {
                    Some((a, b, value, ed.doc.is_driven(id)))
                }
                _ => None,
            })
            .collect()
    };
    ed.dim_mode = DimMode::Chain;
    ed.dim_overall = true;
    let mut tool = DimensionTool::default();
    // The first: two points, placed above them.
    click(&mut ed, &mut tool, v(0.0, 0.0));
    click(&mut ed, &mut tool, v(10.0, 0.0));
    click(&mut ed, &mut tool, v(5.0, 8.0));
    ed.pending_edit = None;
    assert_eq!(rows(&ed), vec![(p[0], p[1], 10.0, false)]);
    // Plain clicks go on along the row: a point, then the wall (its nearer end, the foot).
    click(&mut ed, &mut tool, v(25.0, 0.0));
    click(&mut ed, &mut tool, v(40.0, 3.0));
    assert_eq!(
        rows(&ed)[1..],
        [(p[1], p[2], 15.0, false), (p[2], foot, 15.0, false)]
    );
    // A click on nothing ends it, with the overall outside the row, as a reference.
    click(&mut ed, &mut tool, v(20.0, -30.0));
    let all = rows(&ed);
    assert_eq!(all.len(), 4, "{all:?}");
    assert_eq!(all[3], (p[0], foot, 40.0, true));
    let overall = ed.doc.constraints().last().unwrap().0;
    let chain = ed
        .doc
        .constraints()
        .find(|(_, c)| matches!(c, Constraint::HorizontalDistance { .. }))
        .unwrap()
        .0;
    let (oy, cy) = (
        ed.doc.placement(overall).unwrap().y,
        ed.doc.placement(chain).unwrap().y,
    );
    assert!(oy > cy, "outside the chain: {oy} {cy}");
    assert!(tool.run.is_none());

    // Baseline: each from the first point, each a step further out; no overall.
    let mut ed2 = Editor::default();
    ed2.doc.begin();
    let q = [0.0, 10.0, 25.0, 40.0].map(|x| ed2.doc.add_point(v(x, 0.0)));
    ed2.commit("Setup");
    ed2.dim_mode = DimMode::Baseline;
    ed2.dim_overall = true;
    let mut tool = DimensionTool::default();
    click(&mut ed2, &mut tool, v(0.0, 0.0));
    click(&mut ed2, &mut tool, v(10.0, 0.0));
    click(&mut ed2, &mut tool, v(5.0, 8.0));
    click(&mut ed2, &mut tool, v(25.0, 0.0));
    click(&mut ed2, &mut tool, v(40.0, 0.0));
    tool.handle(Input::Finish, &mut ed2);
    let all = rows(&ed2);
    assert_eq!(
        all.iter().map(|r| (r.0, r.1, r.2)).collect::<Vec<_>>(),
        vec![(q[0], q[1], 10.0), (q[0], q[2], 25.0), (q[0], q[3], 40.0)]
    );

    // Single: a plain click starts afresh (Shift+click still goes on).
    let mut ed3 = Editor::default();
    ed3.doc.begin();
    for x in [0.0, 10.0, 25.0] {
        ed3.doc.add_point(v(x, 0.0));
    }
    ed3.commit("Setup");
    let mut tool = DimensionTool::default();
    click(&mut ed3, &mut tool, v(0.0, 0.0));
    click(&mut ed3, &mut tool, v(10.0, 0.0));
    click(&mut ed3, &mut tool, v(5.0, 8.0));
    click(&mut ed3, &mut tool, v(25.0, 0.0));
    assert_eq!(rows(&ed3).len(), 1, "a new pick, not a chain");
}

#[test]
fn closed_areas_can_be_found_per_layer() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let other = ed.doc.add_layer(drift_core::Layer::new("Furniture"));
    // A 20 × 10 room, and a line across it on another layer.
    let c = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)].map(|p| ed.doc.add_point(p));
    for i in 0..4 {
        ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap();
    }
    let (a, b) = (
        ed.doc.add_point(v(10.0, -5.0)),
        ed.doc.add_point(v(10.0, 15.0)),
    );
    let across = ed.doc.add_line(a, b).unwrap();
    ed.doc.set_entity_layer(across, other).unwrap();
    ed.commit("Setup");
    // All together, the line cuts the room in two.
    assert_eq!(ed.regions().len(), 2);
    // Per layer, the room is whole.
    ed.regions_by_layer = true;
    let regions = ed.regions();
    assert_eq!(regions.len(), 1);
    assert!((regions[0].area - 200.0).abs() < 1e-6);
    // Its outline is the room's four lines (the curve indices still name the right ones).
    let outline = ed.region_curves(&regions[0]);
    assert_eq!(outline.len(), 4);
    assert!(!outline.contains(&across));
    // A wall moved to the other layer leaves the room open.
    let wall = ed
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Line { a, .. } if a == c[0]))
        .unwrap()
        .0;
    ed.doc.begin();
    ed.doc.set_entity_layer(wall, other).unwrap();
    ed.commit("Layer");
    assert!(ed.regions().is_empty());
    ed.regions_by_layer = false;
    assert_eq!(ed.regions().len(), 2);
}

#[test]
fn fractional_inches_read_write_and_save() {
    use drift_core::{LengthFormat, Units};
    let mut ed = Editor::default();
    let units = Units {
        length: LengthFormat::FractionalInch,
        ..Units::default()
    };
    ed.set_units(units);
    let inch = drift_core::units::MM_PER_INCH;
    for (typed, inches) in [
        ("35 1/2", 35.5),
        ("108", 108.0),
        ("3/8", 0.375),
        ("9'", 108.0),
    ] {
        let v = ed
            .evaluate_text(typed, drift_core::Kind::Length)
            .unwrap_or_else(|e| panic!("{typed}: {e:?}"))
            .value;
        assert!((v - inches * inch).abs() < 1e-9, "{typed}: {v}");
    }
    assert_eq!(ed.doc.units().format_length(35.5 * inch), "35 1/2\"");
    let saved = drift_io::save(&ed.doc, &Default::default());
    assert!(saved.contains("fractional_in"), "{saved}");
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert_eq!(loaded.units().length, LengthFormat::FractionalInch);
}

#[test]
fn areas_are_picked_only_where_filled_when_asked() {
    let mut ed = Editor::default();
    ed.doc.begin();
    // Two rooms side by side; the right one hatched.
    let p = [
        v(0.0, 0.0),
        v(10.0, 0.0),
        v(20.0, 0.0),
        v(20.0, 10.0),
        v(10.0, 10.0),
        v(0.0, 10.0),
    ]
    .map(|q| ed.doc.add_point(q));
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0), (1, 4)] {
        ed.doc.add_line(p[a], p[b]).unwrap();
    }
    ed.commit("Setup");
    ed.add_fill_at(v(15.0, 5.0)).unwrap();
    ed.regions_need_fill = true;
    let mut tool = SelectTool::default();
    // The empty room: nothing.
    click(&mut ed, &mut tool, v(5.0, 5.0));
    assert!(ed.selected_region.is_none() && ed.selected_fills.is_empty());
    // The filled one: its fill.
    click(&mut ed, &mut tool, v(15.0, 5.0));
    assert_eq!(ed.selected_fills.len(), 1);
    // Off: the empty room's area.
    ed.regions_need_fill = false;
    click(&mut ed, &mut tool, v(5.0, 5.0));
    assert!(ed.region().is_some());
}

#[test]
fn copies_of_blocks_offer_snap_points() {
    use drift_core::{Block, BlockItem, Instance};
    use drift_geom::SnapKind;
    let mut ed = Editor::default();
    // A table: a 20 × 10 top with a round sink, placed at (100, 50) turned a quarter turn.
    let line = |a: DVec2, b: DVec2| BlockItem {
        shape: Shape::Line { a, b },
        style: drift_core::LineKind::Solid,
        appearance: Default::default(),
    };
    ed.doc.begin();
    let block = ed.doc.add_block(Block {
        name: "Table".into(),
        fills: Vec::new(),
        items: vec![
            line(v(0.0, 0.0), v(20.0, 0.0)),
            line(v(20.0, 0.0), v(20.0, 10.0)),
            line(v(20.0, 10.0), v(0.0, 10.0)),
            line(v(0.0, 10.0), v(0.0, 0.0)),
            BlockItem {
                shape: Shape::Circle {
                    center: v(14.0, 5.0),
                    radius: 2.0,
                },
                style: drift_core::LineKind::Solid,
                appearance: Default::default(),
            },
        ],
    });
    ed.doc
        .add_instance(Instance {
            block,
            at: v(100.0, 50.0),
            angle: 90.0,
            scale: 1.0,
            mirror: false,
            layer: Default::default(),
        })
        .unwrap();
    ed.commit("Setup");
    let options = SnapOptions {
        tolerance: 1.0,
        grid: None,
    };
    let snap = |ed: &Editor, p: DVec2| ed.snap(p, options, |_| false);
    // Turned a quarter turn about its base point: the top runs from (90, 50) to (100, 70).
    let b = ed.instance_bounds(ed.doc.instances().next().unwrap().0);
    assert!(b.min.distance(v(90.0, 50.0)) < 1e-9 && b.max.distance(v(100.0, 70.0)) < 1e-9);
    // A corner of the box (and of the top), the middle of a side, the centre, the sink's
    // centre.
    let s = snap(&ed, v(90.4, 69.7)).expect("a corner");
    assert!(s.at.distance(v(90.0, 70.0)) < 1e-9 && s.kind == SnapKind::Endpoint);
    let s = snap(&ed, v(95.3, 69.6)).expect("a side's middle");
    assert!(s.at.distance(v(95.0, 70.0)) < 1e-9 && s.kind == SnapKind::Midpoint);
    let s = snap(&ed, v(95.4, 59.7)).expect("the centre");
    assert!(s.at.distance(v(95.0, 60.0)) < 1e-9);
    let sink = v(95.0, 64.0);
    let s = snap(&ed, sink + v(0.3, 0.2)).expect("the sink's centre");
    assert!(s.at.distance(sink) < 1e-9 && s.kind == SnapKind::Center);
    // Away from it: nothing.
    assert!(snap(&ed, v(130.0, 30.0)).is_none());
}

/// Two circles' outer arcs, meeting at notches top and bottom: (0, ±8).
fn notched_fixture() -> (Editor, EntityId, EntityId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let top = ed.doc.add_point(v(0.0, 8.0));
    let bottom = ed.doc.add_point(v(0.0, -8.0));
    let ca = ed.doc.add_point(v(-6.0, 0.0));
    let cb = ed.doc.add_point(v(6.0, 0.0));
    ed.doc.add_arc(ca, top, bottom).unwrap();
    ed.doc.add_arc(cb, bottom, top).unwrap();
    ed.commit("Arcs");
    (ed, top, bottom)
}

#[test]
fn two_arcs_meeting_at_a_notch_are_filleted() {
    let (mut ed, top, _) = notched_fixture();
    assert!(ed.corner(top).is_err(), "not two lines");
    let arc = ed.fillet(top, 2.0).unwrap();
    // Tangent outside both circles: 10 + 2 from each centre.
    let Some(Shape::Arc { center, radius, .. }) = ed.doc.shape(arc) else {
        panic!("an arc");
    };
    let expect = v(0.0, (144.0f64 - 36.0).sqrt());
    assert!(center.distance(expect) < 1e-9, "{center}");
    assert!((radius - 2.0).abs() < 1e-9);
    assert_eq!(arcs(&ed.doc), 3);
    // Still one closed outline.
    assert_eq!(ed.regions().len(), 1);
    // It is a fillet: re-sized in place, and its corner put back.
    let f = ed.fillet_at(arc).expect("a fillet");
    assert!(f.corner.distance(v(0.0, 8.0)) < 1e-9, "{}", f.corner);
    ed.refillet(arc, 3.0).unwrap();
    let Some(Shape::Arc { center, radius, .. }) = ed.doc.shape(arc) else {
        panic!("an arc");
    };
    assert!(
        center.distance(v(0.0, (169.0f64 - 36.0).sqrt())) < 1e-7,
        "{center}"
    );
    assert!((radius - 3.0).abs() < 1e-7);
    ed.remove_corner_cut(arc).unwrap();
    assert_eq!(arcs(&ed.doc), 2);
    assert_eq!(ed.regions().len(), 1);
    let corner = ed
        .doc
        .iter()
        .find(|(id, e)| {
            e.kind.is_point()
                && ed
                    .doc
                    .point(*id)
                    .is_ok_and(|p| p.distance(v(0.0, 8.0)) < 1e-9)
        })
        .map(|(id, _)| id)
        .expect("the corner is back");
    assert!(ed.fillet_corner(corner).is_ok());
}

#[test]
fn a_line_meeting_an_arc_is_filleted_with_the_tool() {
    let mut ed = Editor::default();
    ed.auto_relations = true;
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let s = ed.doc.add_point(v(10.0, 0.0));
    let corner = ed.doc.add_point(v(0.0, 10.0));
    let far = ed.doc.add_point(v(-10.0, 20.0));
    let quarter = ed.doc.add_arc(c, s, corner).unwrap();
    let line = ed.doc.add_line(corner, far).unwrap();
    ed.commit("Line and arc");
    // Click the corner, move out, click: a fillet the size the pointer gives.
    let mut tool = FilletTool::default();
    click(&mut ed, &mut tool, v(0.0, 10.0));
    assert!(tool.is_modal(), "the corner is taken");
    tool.handle(Input::Finish, &mut ed);
    let arc = ed
        .doc
        .iter()
        .find(|(id, e)| matches!(e.kind, EntityKind::Arc { .. }) && *id != quarter)
        .map(|(id, _)| id)
        .expect("a fillet");
    let Some(Shape::Arc { center, radius, .. }) = ed.doc.shape(arc) else {
        panic!("an arc");
    };
    // Tangent to both: the line's distance, and outside the quarter circle.
    let Some(Shape::Line { a, b }) = ed.doc.shape(line) else {
        panic!("a line");
    };
    let to_line = (center - a).perp_dot((b - a).normalize()).abs();
    assert!((to_line - radius).abs() < 1e-7, "{to_line} vs {radius}");
    assert!((center.length() - (10.0 + radius)).abs() < 1e-7);
    assert!(ed.fillet_at(arc).is_some());
    assert!(ed.analysis().conflicts.is_empty());
    // One undo step.
    ed.undo();
    assert_eq!(arcs(&ed.doc), 1);
}

#[test]
fn a_smooth_join_has_nothing_to_round() {
    // A line running on tangent from an arc's end.
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, 0.0));
    let s = ed.doc.add_point(v(10.0, 0.0));
    let corner = ed.doc.add_point(v(0.0, 10.0));
    let far = ed.doc.add_point(v(-10.0, 10.0));
    ed.doc.add_arc(c, s, corner).unwrap();
    ed.doc.add_line(corner, far).unwrap();
    ed.commit("Smooth");
    assert!(ed.fillet(corner, 1.0).is_err());
}

fn relate_all(ed: &mut Editor, label: &str) -> usize {
    let option = ed
        .relation_options()
        .into_iter()
        .find(|o| o.label == label)
        .unwrap_or_else(|| panic!("no {label} option"));
    // As the menus do: only what isn't there yet.
    let missing = ed.missing_relations(&option);
    ed.add_constraints(label, &missing).unwrap();
    missing.len()
}

/// Lines at (x0, y0)–(x1, y1).
fn some_lines(ed: &mut Editor, ends: &[[f64; 4]]) -> Vec<EntityId> {
    ed.doc.begin();
    let ids = ends
        .iter()
        .map(|[x0, y0, x1, y1]| {
            let a = ed.doc.add_point(v(*x0, *y0));
            let b = ed.doc.add_point(v(*x1, *y1));
            ed.doc.add_line(a, b).unwrap()
        })
        .collect();
    ed.commit("Lines");
    ids
}

fn direction(ed: &Editor, line: EntityId) -> DVec2 {
    let Some(Shape::Line { a, b }) = ed.doc.shape(line) else {
        panic!("a line");
    };
    (b - a).normalize()
}

#[test]
fn three_lines_are_made_parallel_or_collinear_at_once() {
    let mut ed = Editor::default();
    let lines = some_lines(
        &mut ed,
        &[
            [0.0, 0.0, 10.0, 1.0],
            [0.0, 5.0, 10.0, 4.0],
            [0.0, 9.0, 10.0, 11.0],
        ],
    );
    ed.selection = lines.iter().copied().collect();
    let labels: Vec<_> = ed.relation_options().iter().map(|o| o.label).collect();
    assert!(labels.contains(&"Parallel") && labels.contains(&"Collinear"));
    assert!(
        !labels.contains(&"Perpendicular"),
        "three lines can't all be"
    );
    assert_eq!(relate_all(&mut ed, "Parallel"), 2);
    let d0 = direction(&ed, lines[0]);
    assert!(
        lines
            .iter()
            .all(|l| direction(&ed, *l).perp_dot(d0).abs() < 1e-9)
    );
    ed.undo();

    // With two already parallel, only the third is tied in, so nothing is said twice.
    ed.selection = [lines[1], lines[2]].into();
    relate_all(&mut ed, "Parallel");
    ed.selection = lines.iter().copied().collect();
    assert_eq!(relate_all(&mut ed, "Parallel"), 1);
    assert!(ed.analysis().conflicts.is_empty());
    // All parallel now: shown as on (the pie lights it, and choosing it takes all three off).
    let parallel = ed
        .relation_options()
        .into_iter()
        .find(|o| o.label == "Parallel")
        .unwrap();
    assert!(ed.has_relations(&parallel));
    assert_eq!(ed.existing_relations(&parallel).len(), 2);

    ed.undo();
    ed.undo();
    assert_eq!(relate_all(&mut ed, "Collinear"), 2);
    let Some(Shape::Line { a, .. }) = ed.doc.shape(lines[0]) else {
        panic!()
    };
    let d0 = direction(&ed, lines[0]);
    for l in &lines {
        let Some(Shape::Line { a: p, b: q }) = ed.doc.shape(*l) else {
            panic!()
        };
        assert!((p - a).perp_dot(d0).abs() < 1e-7 && (q - a).perp_dot(d0).abs() < 1e-7);
    }
}

#[test]
fn several_points_and_curves_are_related_at_once() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let points: Vec<_> = [v(0.0, 0.0), v(5.0, 1.0), v(9.0, -2.0)]
        .map(|p| ed.doc.add_point(p))
        .into();
    let c = ed.doc.add_point(v(5.0, 20.0));
    let circle = ed.doc.add_circle(c, 4.0).unwrap();
    ed.commit("Points");
    // Three points level.
    ed.selection = points.iter().copied().collect();
    assert_eq!(relate_all(&mut ed, "Horizontal"), 2);
    let y = ed.doc.point(points[0]).unwrap().y;
    assert!(
        points
            .iter()
            .all(|p| (ed.doc.point(*p).unwrap().y - y).abs() < 1e-9)
    );
    ed.undo();
    // All three on the circle.
    ed.selection = points.iter().copied().chain([circle]).collect();
    assert_eq!(relate_all(&mut ed, "On curve"), 3);
    let (centre, r) = (ed.doc.point(c).unwrap(), 4.0);
    let r_now = drift_solver::radius_of(&ed.doc, circle).unwrap_or(r);
    assert!(
        points
            .iter()
            .all(|p| (ed.doc.point(*p).unwrap().distance(centre) - r_now).abs() < 1e-7)
    );
    // Two lines tangent to the circle at once.
    ed.undo();
    let lines = some_lines(
        &mut ed,
        &[[-10.0, 10.0, 20.0, 12.0], [-10.0, 30.0, 20.0, 29.0]],
    );
    ed.selection = lines.iter().copied().chain([circle]).collect();
    assert_eq!(relate_all(&mut ed, "Tangent"), 2);
    assert!(ed.analysis().conflicts.is_empty());
}

/// Three level lines held equal and parallel, the middle one joined by an arc to the top
/// one's left end, as drawn: (editor, [bottom, middle, top]).
fn equal_ladder() -> (Editor, [EntityId; 3]) {
    let mut ed = Editor::default();
    let lines = some_lines(
        &mut ed,
        &[
            [0.0, 0.0, 80.0, 0.0],
            [-20.0, 25.0, 60.0, 25.0],
            [50.0, 60.0, 130.0, 60.0],
        ],
    );
    let [bottom, middle, top] = [lines[0], lines[1], lines[2]];
    ed.doc.begin();
    let ends = |ed: &Editor, l| match ed.doc.get(l).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => (a, b),
        _ => panic!(),
    };
    let (_, middle_end) = ends(&ed, middle);
    let (top_start, _) = ends(&ed, top);
    // An arc from the middle line's right end (60, 25) up to the top line's left end
    // (50, 60), its centre as far from each.
    let c = ed.doc.add_point(v(10.0, 2075.0 / 70.0));
    ed.doc.add_arc(c, middle_end, top_start).unwrap();
    for (a, b) in [(bottom, middle), (middle, top)] {
        ed.doc.add_constraint(Constraint::Equal { a, b }).unwrap();
        ed.doc
            .add_constraint(Constraint::Parallel { a, b })
            .unwrap();
    }
    ed.doc
        .add_constraint(Constraint::Horizontal { line: bottom })
        .unwrap();
    ed.commit("Ladder");
    (ed, [bottom, middle, top])
}

fn ends_at(ed: &Editor, line: EntityId) -> (DVec2, DVec2) {
    let Some(Shape::Line { a, b }) = ed.doc.shape(line) else {
        panic!("a line");
    };
    (a, b)
}

#[test]
fn trimming_a_line_held_equal_leaves_the_rest_where_it_is() {
    let (mut ed, [bottom, middle, top]) = equal_ladder();
    // Something to cut the middle line at x = 20.
    ed.doc.begin();
    let p = ed.doc.add_point(v(20.0, 10.0));
    let q = ed.doc.add_point(v(20.0, 40.0));
    ed.doc.add_line(p, q).unwrap();
    ed.commit("Cutter");
    let before = (ends_at(&ed, bottom), ends_at(&ed, top));
    // Trim the middle line's left piece.
    ed.trim_at(middle, v(0.0, 25.0)).unwrap();
    // The others haven't moved, and the middle line is cut where it was clicked.
    assert_eq!((ends_at(&ed, bottom), ends_at(&ed, top)), before);
    let (a, b) = ends_at(&ed, middle);
    assert!(
        a.distance(v(20.0, 25.0)) < 1e-9 && b.distance(v(60.0, 25.0)) < 1e-9,
        "{a} {b}"
    );
    // Its equal lengths came off (it's shorter now); parallel stays, since it still holds.
    let on_middle = |ed: &Editor, f: fn(&Constraint) -> bool| {
        ed.doc
            .constraints_on(middle)
            .filter(|id| ed.doc.constraint(*id).is_some_and(f))
            .count()
    };
    assert_eq!(on_middle(&ed, |c| matches!(c, Constraint::Equal { .. })), 0);
    assert_eq!(
        on_middle(&ed, |c| matches!(c, Constraint::Parallel { .. })),
        2
    );
    assert!(
        ed.notice.as_deref().is_some_and(|n| n.contains("Equal ×2")),
        "{:?}",
        ed.notice
    );
    // One undo brings the line and its relations back.
    ed.undo();
    assert_eq!(on_middle(&ed, |c| matches!(c, Constraint::Equal { .. })), 2);
}

#[test]
fn a_dimension_that_a_reshape_would_break_refuses_it() {
    let mut ed = Editor::default();
    let lines = some_lines(&mut ed, &[[0.0, 0.0, 10.0, 0.0], [20.0, -5.0, 20.0, 5.0]]);
    let (a, b) = match ed.doc.get(lines[0]).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => (a, b),
        _ => panic!(),
    };
    ed.add_dimension(Constraint::Distance { a, b, value: 10.0 }, None)
        .unwrap();
    let err = ed.extend_at(lines[0], v(9.0, 0.0)).unwrap_err();
    assert!(err.to_string().contains("distance"), "{err}");
    assert_eq!(ends_at(&ed, lines[0]).1, v(10.0, 0.0), "nothing changed");
}
