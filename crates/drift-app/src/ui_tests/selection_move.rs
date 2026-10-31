//! Picking and moving: G with a picked midpoint (#154), X / Y locks in a mouse drag (#155),
//! and areas outlined by hidden curves never picked (#153).

use drift_core::EntityId;

use super::*;

/// A line from `a` to `b`, with its end points.
fn line(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) -> (EntityId, EntityId, EntityId) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let (pa, pb) = (ed.doc.add_point(a), ed.doc.add_point(b));
    let l = ed.doc.add_line(pa, pb).unwrap();
    ed.commit("Line");
    (l, pa, pb)
}

fn at(h: &Harness<'_, DriftApp>, p: EntityId) -> DVec2 {
    h.state().drawing.editor.doc.point(p).unwrap()
}

/// Show the drawing from `min` to `max`, without grid snapping (pointer offsets are exact).
fn view(h: &mut Harness<'_, DriftApp>, min: DVec2, max: DVec2) {
    h.state_mut().grid_snap = false;
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds { min, max });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
}

#[test]
fn g_moves_a_line_picked_by_its_midpoint() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let (l, a, b) = line(&mut h, v(0.0, 0.0), v(40.0, 20.0));
    view(&mut h, v(-30.0, -30.0), v(90.0, 60.0));
    // Picked by its midpoint, nothing else.
    click_world(&mut h, v(20.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_midpoints, [l].into());
    assert!(h.state().drawing.editor.selection.is_empty());
    // G: the line follows the pointer from its midpoint; a click places it.
    h.hover_at(screen(&h, v(20.0, 10.0)));
    h.run();
    h.key_press(Key::G);
    h.run();
    assert!(h.state().tool.is_modal(), "a move began");
    h.hover_at(screen(&h, v(35.0, -5.0)));
    h.run();
    click_world(&mut h, v(35.0, -5.0));
    let (pa, pb) = (at(&h, a), at(&h, b));
    let moved = pa - v(0.0, 0.0);
    assert!(moved.distance(v(15.0, -15.0)) < 0.5, "{moved:?}");
    assert!(
        (pb - v(40.0, 20.0)).distance(moved) < 1e-9,
        "both ends, rigidly"
    );
    // G, X, a typed 12, Enter: exactly 12 along X.
    h.state_mut().drawing.editor.selected_midpoints = [l].into();
    h.state_mut().drawing.editor.selection.clear();
    h.hover_at(screen(&h, v(40.0, 0.0)));
    h.run();
    h.key_press(Key::G);
    h.run();
    h.hover_at(screen(&h, v(45.0, 3.0)));
    h.run();
    h.key_press(Key::X);
    h.run();
    type_text(&mut h, "12");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(at(&h, a), pa + v(12.0, 0.0));
    assert_eq!(at(&h, b), pb + v(12.0, 0.0));
    // One undo step each.
    h.state_mut().undo();
    assert_eq!(at(&h, a), pa);
}

#[test]
fn x_or_y_locks_a_mouse_drag_to_an_axis() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let (_, a, b) = line(&mut h, v(0.0, 0.0), v(40.0, 20.0));
    view(&mut h, v(-30.0, -30.0), v(90.0, 60.0));
    // Drag the end point; X halfway through locks it to the X axis.
    let from = screen(&h, v(40.0, 20.0));
    let mid = screen(&h, v(48.0, 26.0));
    let to = screen(&h, v(55.0, 31.0));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=3 {
        h.hover_at(from + (mid - from) * (k as f32 / 3.0));
        h.run();
    }
    h.key_press(Key::X);
    h.run();
    assert!(
        h.state().tool.axis_lock().is_some(),
        "locked, with the guide"
    );
    assert!(
        h.state().tool.hint().contains("X locked"),
        "{}",
        h.state().tool.hint()
    );
    for k in 1..=3 {
        h.hover_at(mid + (to - mid) * (k as f32 / 3.0));
        h.run();
    }
    save_png("drift_ui_drag_axis_locked.png", &mut h);
    button(&h, to, false);
    h.run();
    let end = at(&h, b);
    assert_eq!(end.y, 20.0, "only x changed");
    assert!((end.x - 55.0).abs() < 0.5, "{end:?}");
    assert_eq!(at(&h, a), v(0.0, 0.0));
    // Y, then X again unlocks: a drag that ends free.
    h.state_mut().undo();
    assert_eq!(at(&h, b), v(40.0, 20.0));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    h.hover_at(mid);
    h.run();
    h.key_press(Key::Y);
    h.run();
    h.hover_at(to);
    h.run();
    assert_eq!(at(&h, b).x, 40.0, "Y locked: only y changes");
    h.key_press(Key::Y);
    h.run();
    assert!(h.state().tool.axis_lock().is_none(), "unlocked");
    h.hover_at(to + egui::vec2(1.0, 0.0));
    h.run();
    button(&h, to + egui::vec2(1.0, 0.0), false);
    h.run();
    assert!(at(&h, b).x > 50.0 && at(&h, b).y > 25.0, "{:?}", at(&h, b));
    // Locked, a typed distance and Enter place it exactly.
    h.state_mut().undo();
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    h.hover_at(mid);
    h.run();
    h.key_press(Key::X);
    h.run();
    type_text(&mut h, "7");
    h.key_press(Key::Enter);
    h.run();
    button(&h, mid, false);
    h.run();
    assert_eq!(at(&h, b), v(47.0, 20.0));
    assert!(!h.state().tool.is_busy());
}

#[test]
fn an_area_outlined_by_hidden_curves_is_not_picked() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // A rectangle on layer 0, a line on layer B away from it.
    let sides: Vec<EntityId> = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.change_rules(|r| r.regions_need_fill = false);
        ed.doc.begin();
        let p =
            [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].map(|q| ed.doc.add_point(q));
        let sides = (0..4)
            .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        ed.commit("Rectangle");
        sides
    };
    let (shelf, sa, sb) = line(&mut h, v(60.0, 10.0), v(90.0, 10.0));
    let b = h.state_mut().drawing.editor.add_layer("B");
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        for id in [shelf, sa, sb] {
            ed.doc.set_entity_layer(id, b).unwrap();
        }
        ed.commit("To B");
    }
    view(&mut h, v(-10.0, -10.0), v(100.0, 40.0));
    let picked = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().drawing.editor;
        (ed.selected_region.is_some(), ed.selection.len())
    };
    // Everything shown: a click inside the rectangle picks its area (and outline).
    click_world(&mut h, v(20.0, 15.0));
    assert_eq!(picked(&h), (true, 4));
    assert!(h.state_mut().drawing.editor.region().is_some());
    h.key_press(Key::Escape);
    h.run();
    // B isolated: the rectangle is hidden, and a click there picks nothing.
    let before = h.state_mut().drawing.editor.isolate_layer(b);
    h.run();
    click_world(&mut h, v(20.0, 15.0));
    assert_eq!(picked(&h), (false, 0));
    // Nor when its layer is hidden the usual way.
    h.state_mut()
        .drawing
        .editor
        .set_layers_visible(&before, "End isolate");
    let zero = h.state().drawing.editor.doc.layer_of(sides[0]).clone();
    let layer0 = h
        .state()
        .drawing
        .editor
        .doc
        .layers()
        .find(|(_, l)| **l == zero)
        .map(|(id, _)| id)
        .unwrap();
    h.state_mut()
        .drawing
        .editor
        .set_layers_visible(&[(layer0, false)], "Hide");
    h.run();
    click_world(&mut h, v(20.0, 15.0));
    assert_eq!(picked(&h), (false, 0));
    // A picked area whose layer is hidden afterwards isn't highlighted (or moved) either.
    h.state_mut()
        .drawing
        .editor
        .set_layers_visible(&[(layer0, true)], "Show");
    h.run();
    click_world(&mut h, v(20.0, 15.0));
    assert_eq!(picked(&h), (true, 4));
    h.state_mut()
        .drawing
        .editor
        .set_layers_visible(&[(layer0, false)], "Hide");
    assert!(h.state_mut().drawing.editor.region().is_none());
}
