//! Picking and moving: the selection filters, G from a picked midpoint, X and Y locks in a
//! mouse drag, an area on a hidden layer, and Alt-click on a layer's lock or snap.

use crate::editor::{PickFilter, PickKind};
use crate::ui_tests::layer_toggles::{LOCK, SNAP, alt_toggle};

use super::*;

/// A line from `a` to `b` drawn with the Line tool; back to Select. Returns it.
fn line(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) -> EntityId {
    let before = ids(h);
    h.key_press(Key::L);
    h.run();
    click_world(h, a);
    click_world(h, b);
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let new = curves(h, &added(h, &before));
    assert_eq!(new.len(), 1, "a line drawn");
    h.state_mut().editor.selection.clear();
    new[0]
}

/// drift-releases#9: each kind alone (Alt-click its toggle), two kinds together, and what a
/// click picks with them.
#[test]
fn smoke_selection_filters_one_kind_or_several() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 50.0));
    let (sides, fill) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let fill = fill.unwrap();
    let loose = line(&mut h, DVec2::new(60.0, 0.0), DVec2::new(100.0, 20.0));
    for kind in PickKind::ALL {
        pick_only(&mut h, kind.label());
        assert_eq!(
            h.state().editor.pick_filter,
            PickFilter::only(kind),
            "{kind:?}"
        );
        assert!(
            h.query_by_label("Selection filter on").is_some(),
            "the badge"
        );
    }
    // Fills alone: a click in the shape picks its fill, not its outline; the line isn't picked.
    pick_only(&mut h, "Fills");
    click_world(&mut h, DVec2::new(20.0, 15.0));
    assert_eq!(h.state().editor.selected_fills, [fill].into());
    assert!(h.state().editor.selection.is_empty());
    click_world(&mut h, DVec2::new(80.0, 10.0));
    assert!(h.state().editor.selection.is_empty(), "edges are off");
    // Edges as well: the line picks too.
    h.get_by_label("Pick Edges").click();
    h.run();
    assert_eq!(
        h.state().editor.pick_filter,
        PickFilter::only(PickKind::Fills).with(PickKind::Edges, true)
    );
    click_world(&mut h, DVec2::new(80.0, 10.0));
    assert_eq!(h.state().editor.selection, [loose].into());
    // All: everything again, and a click in the shape picks it whole.
    h.get_by_label("All").click();
    h.run();
    assert!(h.state().editor.pick_filter.is_all());
    assert!(h.query_by_label("Selection filter on").is_none());
    click_world(&mut h, DVec2::new(20.0, 15.0));
    assert_eq!(h.state().editor.selection, sides.iter().copied().collect());
}

/// #154: G with a line picked by its midpoint moves the line. #155: X locks a mouse drag to
/// the x axis.
#[test]
fn smoke_selection_g_from_a_midpoint_and_axis_lock_in_a_drag() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-30.0, -30.0), DVec2::new(90.0, 60.0));
    let l = line(&mut h, DVec2::ZERO, DVec2::new(40.0, 20.0));
    let (a, b) = ends(&h, l);
    // (Clicked, its ends are where the pointer was: within a pixel's rounding.)
    let (a0, b0) = (at(&h, a), at(&h, b));
    click_world(&mut h, DVec2::new(20.0, 10.0));
    assert_eq!(h.state().editor.selected_midpoints, [l].into());
    h.hover_at(screen(&h, DVec2::new(20.0, 10.0)));
    h.run();
    h.key_press(Key::G);
    h.run();
    assert!(h.state().tool.is_modal(), "G moves it");
    h.hover_at(screen(&h, DVec2::new(25.0, 12.0)));
    h.run();
    h.key_press(Key::X);
    h.run();
    type_text(&mut h, "12");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(at(&h, a), a0 + DVec2::new(12.0, 0.0));
    assert_eq!(at(&h, b), b0 + DVec2::new(12.0, 0.0));
    // A mouse drag of the end point, X pressed on the way: only x changes.
    h.key_press(Key::Escape);
    h.run();
    let (from, mid, to) = (
        screen(&h, DVec2::new(52.0, 20.0)),
        screen(&h, DVec2::new(58.0, 26.0)),
        screen(&h, DVec2::new(65.0, 31.0)),
    );
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
    assert!(h.state().tool.axis_lock().is_some(), "locked mid-drag");
    for k in 1..=3 {
        h.hover_at(mid + (to - mid) * (k as f32 / 3.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    let end = at(&h, b);
    assert_eq!(end.y, b0.y, "only x changed: {end:?}");
    assert!((end.x - 65.0).abs() < 0.5, "{end:?}");
    assert_eq!(at(&h, a), a0 + DVec2::new(12.0, 0.0));
}

/// #153: an area whose outline is on a hidden layer can't be picked.
#[test]
fn smoke_selection_hidden_layer_area_is_not_picked() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .editor
        .change_rules(|r| r.regions_need_fill = false);
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    click_world(&mut h, DVec2::new(20.0, 15.0));
    assert!(h.state().editor.selected_region.is_some(), "the area");
    h.key_press(Key::Escape);
    h.run();
    let layer = h.state().editor.doc.current_layer();
    h.state_mut()
        .editor
        .set_layers_visible(&[(layer, false)], "Hide");
    h.run();
    click_world(&mut h, DVec2::new(20.0, 15.0));
    let ed = &h.state().editor;
    assert!(ed.selected_region.is_none() && ed.selection.is_empty());
}

/// drift-releases#10: Alt-click a layer's lock locks every other layer; its snap, snaps to
/// that layer only.
#[test]
fn smoke_selection_alt_click_layer_lock_and_snap() {
    let Some(mut h) = harness() else { return };
    let layers = many_layers(&mut h, 3);
    let others = |h: &Harness<'_, DriftApp>, f: fn(&drift_core::Layer) -> bool| {
        h.state()
            .editor
            .doc
            .layers()
            .filter(|(id, l)| *id != layers[1] && f(l))
            .count()
    };
    let total = h.state().editor.doc.layers().count();
    let name = h.state().editor.doc.layer(layers[1]).unwrap().name.clone();
    alt_toggle(&mut h, &name, LOCK);
    assert_eq!(others(&h, |l| l.locked), total - 1, "the others locked");
    assert!(!h.state().editor.doc.layer(layers[1]).unwrap().locked);
    h.state_mut().undo();
    h.run();
    assert_eq!(others(&h, |l| l.locked), 0);
    alt_toggle(&mut h, &name, SNAP);
    assert_eq!(others(&h, |l| l.snappable), 0, "snapping to this one only");
    assert!(h.state().editor.doc.layer(layers[1]).unwrap().snappable);
}

/// The radius of a circle.
fn radius(h: &Harness<'_, DriftApp>, c: EntityId) -> f64 {
    match h.state().editor.doc.shape(c) {
        Some(drift_core::Shape::Circle { radius, .. }) => radius,
        other => panic!("not a circle: {other:?}"),
    }
}

/// Press at `from`, move to `to` and let go, with `mods` held throughout.
fn handle_drag(h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2, mods: EguiModifiers) {
    h.hover_at(from);
    h.run();
    h.event(Event::ModifiersChanged(mods));
    let press = |h: &mut Harness<'_, DriftApp>, pos: Pos2, pressed: bool| {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: mods,
        });
        h.run();
    };
    press(h, from, true);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run();
    }
    press(h, to, false);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
}

/// #221: one picked circle shows the transform box; its edge still resizes it (the box sits
/// clear of it), its corner and side handles scale it as a circle, and one picked line has no
/// box. A circle whose radius is dimensioned keeps it and says why; with Alt the dimension
/// scales too.
#[test]
fn smoke_selection_one_circle_has_a_box_and_one_line_none() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-40.0, -40.0), v(40.0, 40.0));
    let c = circle(&mut h, DVec2::ZERO, 10.0);
    click_world(&mut h, v(10.0, 0.0));
    assert_eq!(h.state().editor.selection, [c].into());
    let frame = h.state_mut().editor.transform_box().expect("a box");
    assert!(frame.lone && frame.uniform, "{frame:?}");
    save_png("drift_smoke_circle_box.png", &mut h);
    let cursor = |h: &Harness<'_, DriftApp>| h.output().platform_output.cursor_icon;
    // The top-right corner handle sits twice the pick radius out from the circle's bounds.
    let handle = screen(&h, v(10.0, 10.0)) + egui::vec2(16.0, -16.0);
    h.hover_at(handle);
    h.run();
    assert_eq!(cursor(&h), egui::CursorIcon::ResizeNeSw);

    // Its edge, at the top where the box's middle handle is nearest, still resizes it.
    let top = screen(&h, v(0.0, 10.0));
    let out = screen(&h, v(0.0, 14.0));
    handle_drag(&mut h, top, out, EguiModifiers::NONE);
    assert!((radius(&h, c) - 14.0).abs() < 0.2, "{}", radius(&h, c));
    h.state_mut().undo();
    h.run();
    assert_eq!(radius(&h, c), 10.0);

    // The corner handle, half as far again from the opposite corner: half as big again,
    // about that corner.
    let corner = screen(&h, v(-10.0, -10.0));
    handle_drag(
        &mut h,
        handle,
        corner + (handle - corner) * 1.5,
        EguiModifiers::NONE,
    );
    assert!((radius(&h, c) - 15.0).abs() < 0.1, "{}", radius(&h, c));
    let centre = |h: &Harness<'_, DriftApp>| match h.state().editor.doc.shape(c) {
        Some(drift_core::Shape::Circle { center, .. }) => center,
        other => panic!("{other:?}"),
    };
    assert!(centre(&h).distance(v(5.0, 5.0)) < 0.1, "{:?}", centre(&h));
    h.state_mut().undo();
    h.run();

    // A side handle scales it the same way both ways: still a circle, not left as it was.
    let side = screen(&h, v(10.0, 0.0)) + egui::vec2(16.0, 0.0);
    let left = screen(&h, v(-10.0, 0.0));
    handle_drag(
        &mut h,
        side,
        left + (side - left) * 1.5,
        EguiModifiers::NONE,
    );
    assert!((radius(&h, c) - 15.0).abs() < 0.1, "{}", radius(&h, c));
    h.state_mut().undo();
    h.run();

    // One line: no box.
    let l = line(&mut h, v(-30.0, -30.0), v(-15.0, -25.0));
    // (A third of the way along: its middle would pick its midpoint.)
    click_world(&mut h, v(-25.0, -30.0 + 5.0 / 3.0));
    assert_eq!(h.state().editor.selection, [l].into());
    assert!(h.state_mut().editor.transform_box().is_none());

    // The circle's radius dimensioned at 10: the box keeps it, and says so.
    let dim = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let dim = ed
            .doc
            .add_constraint(drift_core::Constraint::Radius {
                curve: c,
                value: 10.0,
            })
            .unwrap();
        ed.commit("Radius");
        dim
    };
    click_world(&mut h, v(10.0, 0.0));
    assert_eq!(h.state().editor.selection, [c].into());
    let depth = h.state().editor.history.undo_depth();
    handle_drag(
        &mut h,
        handle,
        corner + (handle - corner) * 1.5,
        EguiModifiers::NONE,
    );
    assert_eq!(radius(&h, c), 10.0);
    assert_eq!(h.state().editor.history.undo_depth(), depth, "no step");
    assert!(
        status(&h).contains("held by a dimension") && status(&h).contains("Alt"),
        "{}",
        status(&h)
    );
    // With Alt, the dimension scales with it.
    handle_drag(
        &mut h,
        handle,
        corner + (handle - corner) * 1.5,
        EguiModifiers::ALT,
    );
    assert!((radius(&h, c) - 15.0).abs() < 0.1, "{}", radius(&h, c));
    let value = h
        .state()
        .editor
        .doc
        .constraint(dim)
        .unwrap()
        .value()
        .unwrap();
    assert!((value - radius(&h, c)).abs() < 1e-6, "{value}");
}
