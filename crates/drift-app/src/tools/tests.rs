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
    let mut ed = Editor::default();
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
fn axis_lock_toggles_and_works_with_typed_lengths() {
    let mut ed = Editor::default();
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
    let mut ed = Editor::default();
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
                command: false,
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
    click(&mut ed, &mut tool, v(0.1, 0.1));
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
    assert_eq!(lines(&ed.doc), 3);
    assert!(
        !ed.doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
    );
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
        command: false,
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
