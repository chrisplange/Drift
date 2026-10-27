//! Drawing: a loop closed onto its start keeps the closing side's direction relation (#204),
//! and the snap marker shows what a click will use, with an axis lock too (#200).

use drift_core::Constraint;
use drift_geom::SnapKind;

use super::*;

use crate::editor::GuideKind;

/// Hover `p` and run, so the canvas works out the pointer there.
fn hover_world(h: &mut Harness<'_, DriftApp>, p: DVec2) {
    let pos = screen(h, p);
    h.hover_at(pos);
    h.run();
}

/// A rectangle drawn side by side with the Line tool, closed by clicking its start again:
/// the closing side gets its Horizontal relation like the others (the end-point snap used to
/// replace it), the HUD says so before the click, and changing the height keeps all four sides
/// level and plumb.
#[test]
fn smoke_drawing_closing_a_loop_keeps_its_direction_relation() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-20.0, -20.0), v(60.0, 50.0));
    let before = ids(&h);
    h.key_press(Key::L);
    h.run();
    for p in [v(0.0, 0.0), v(0.0, 30.0), v(40.0, 30.0), v(40.0, 0.0)] {
        click_world(&mut h, p);
    }
    // On the start point: the end-point snap, and the relation the side will get.
    hover_world(&mut h, v(0.0, 0.0));
    let p = h.state().pointer.expect("a pointer");
    assert_eq!(p.snap.map(|s| s.kind), Some(SnapKind::Endpoint));
    assert_eq!(
        p.auto_relation(),
        Some(GuideKind::Horizontal),
        "Endpoint · Horizontal"
    );
    click_world(&mut h, v(0.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    let sides = curves(&h, &added(&h, &before));
    assert_eq!(sides.len(), 4, "a closed loop of four sides");
    let doc = &h.state().editor.doc;
    let count = |f: fn(&Constraint) -> bool| doc.constraints().filter(|(_, c)| f(c)).count();
    assert_eq!(count(|c| matches!(c, Constraint::Horizontal { .. })), 2);
    assert_eq!(count(|c| matches!(c, Constraint::Vertical { .. })), 2);
    assert_eq!(h.state().editor.notice, None, "nothing was left out");
    save_png("drift_smoke_closed_loop.png", &mut h);

    // A height on the right side: the bottom stays level.
    let right = sides[2];
    let (a, b) = ends(&h, right);
    let ed = &mut h.state_mut().editor;
    let id = ed
        .add_constraints("Length", &[Constraint::Distance { a, b, value: 30.0 }])
        .unwrap()[0];
    ed.set_dimension(id, 45.0).unwrap();
    h.run();
    for side in &sides {
        let (a, b) = ends(&h, *side);
        let (p, q) = (at(&h, a), at(&h, b));
        assert!(
            (p.x - q.x).abs() < 1e-9 || (p.y - q.y).abs() < 1e-9,
            "every side still level or plumb: {p} {q}"
        );
    }
}

/// With X locked, hovering an end point off the axis marks it and the point on the axis the
/// click will use (the lock used to drop the snap, leaving no marker); in every draw tool an
/// end point hovered shows its marker.
#[test]
fn smoke_drawing_snap_markers_show_with_an_axis_lock() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-20.0, -20.0), v(60.0, 50.0));
    // Something to snap to: a line from (30, 10) to (30, 25).
    let before = ids(&h);
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, v(30.0, 10.0));
    click_world(&mut h, v(30.0, 25.0));
    h.key_press(Key::Escape);
    h.run();
    let line = curves(&h, &added(&h, &before))[0];
    let (low, high) = ends(&h, line);
    let (low, high) = (at(&h, low), at(&h, high));
    // A new line from the origin, locked to X.
    click_world(&mut h, v(0.0, 0.0));
    h.key_press(Key::X);
    h.run();
    let start = h.state().tool.snap_from().expect("the line started");
    hover_world(&mut h, low);
    let p = h.state().pointer.expect("a pointer");
    assert_eq!(p.snap, None, "the lock decides where the point goes");
    assert_eq!(
        p.overridden.map(|s| (s.kind, s.at)),
        Some((SnapKind::Endpoint, low)),
        "the end point it lines up with is marked"
    );
    assert_eq!(
        p.pos(),
        v(low.x, start.y),
        "and the click lands on the axis below it"
    );
    save_png("drift_smoke_snap_with_x_lock.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();

    for tool in [
        ToolKind::Line,
        ToolKind::Rectangle,
        ToolKind::Pen,
        ToolKind::Arc,
    ] {
        h.state_mut().set_tool(tool);
        h.run();
        hover_world(&mut h, high);
        let p = h.state().pointer.expect("a pointer");
        assert_eq!(
            p.snap.map(|s| (s.kind, s.at)),
            Some((SnapKind::Endpoint, high)),
            "{tool:?}"
        );
    }

    // The centre-point arc ends on its circle: an end point off it is marked, with where the
    // click lands, on the circle toward it.
    h.state_mut().set_tool(ToolKind::Arc);
    h.run();
    click_world(&mut h, v(0.0, 0.0));
    click_world(&mut h, v(10.0, 0.0));
    hover_world(&mut h, high);
    let p = h.state().pointer.expect("a pointer");
    assert_eq!(p.snap, None, "the arc's circle decides where it ends");
    assert_eq!(
        p.overridden.map(|s| (s.kind, s.at)),
        Some((SnapKind::Endpoint, high))
    );
    let landing = p.pos();
    assert!(
        (landing.length() - 10.0).abs() < 0.05,
        "on the circle: {landing}"
    );
    assert!(
        landing.normalize().dot(high.normalize()) > 0.9999,
        "toward the end point: {landing}"
    );
    save_png("drift_smoke_arc_snap_off_circle.png", &mut h);
}

/// #222: trimming a circle that one line ends inside (crossed once) is refused with what is
/// wrong and what to do: it must be crossed in two places, it's crossed in 1, use Delete.
/// Hovering it rings the one crossing; nothing changes.
#[test]
fn smoke_drawing_trimming_a_circle_crossed_once_says_why() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-40.0, -40.0), v(40.0, 40.0));
    let c = circle(&mut h, DVec2::ZERO, 10.0);
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, -30.0));
        let b = ed.doc.add_point(v(0.0, -4.0));
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
    }
    h.run();
    let before = positions(&h);
    let depth = h.state().editor.history.undo_depth();
    h.key_press(Key::T);
    h.run();
    hover_world(&mut h, v(0.0, 10.0));
    let rings: Vec<DVec2> = h
        .state()
        .tool
        .editor_preview(&h.state().editor)
        .iter()
        .filter_map(|item| match item.shape {
            drift_core::Shape::Circle { center, .. } => Some(center),
            _ => None,
        })
        .collect();
    assert_eq!(rings.len(), 1, "the one crossing ringed");
    assert!(rings[0].distance(v(0.0, -10.0)) < 1e-6, "{rings:?}");
    click_world(&mut h, v(0.0, 10.0));
    let said = status(&h);
    assert!(
        said.contains(
            "a circle must be crossed in two places to trim a piece; it's crossed in 1 here"
        ) && said.contains("Use Delete to remove it all"),
        "{said}"
    );
    assert!(h.state().editor.doc.contains(c));
    assert_eq!(positions(&h), before, "nothing changed");
    assert_eq!(h.state().editor.history.undo_depth(), depth, "no step");
}
