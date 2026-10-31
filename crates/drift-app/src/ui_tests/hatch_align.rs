//! Aligning a hatch on a shape (#158): the pattern's handles on a picked shape, the
//! Inspector's Pattern alignment rows, Align to edge, the pattern carried with its shape, and
//! Alt-click or double-click to pick the fill alone.

use drift_core::{EntityId, FillId, FillKind, Hatch};

use super::*;

/// A hatched shape: a parallelogram (0,0) (40,0) (50,30) (10,30), its right side slanted.
/// Returns its sides (in order round it) and its hatch.
fn hatched_shape(h: &mut Harness<'_, DriftApp>) -> (Vec<EntityId>, FillId) {
    h.state_mut().status_colors = false;
    let ed = &mut h.state_mut().drawing.editor;
    ed.prefs.fill_new_shapes = true;
    ed.prefs.fill_style.kind = FillKind::Hatch(Hatch::new("ANSI31"));
    ed.doc.begin();
    let corners = [(0.0, 0.0), (40.0, 0.0), (50.0, 30.0), (10.0, 30.0)];
    let p = corners.map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let sides: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.add_shape(&sides).expect("a shape");
    ed.prefs.fill_new_shapes = false;
    ed.commit("Shape");
    let fill = ed.doc.fills().next().map(|(id, _)| id).expect("its hatch");
    ed.selected_fills.clear();
    ed.selection.clear();
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-20.0, -20.0),
        max: DVec2::new(70.0, 50.0),
    });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    (sides, fill)
}

fn hatch(h: &Harness<'_, DriftApp>, fill: FillId) -> Hatch {
    match &h
        .state()
        .drawing
        .editor
        .doc
        .fill(fill)
        .expect("the fill")
        .kind
    {
        FillKind::Hatch(hatch) => hatch.clone(),
        other => panic!("{other:?}"),
    }
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-6
}

/// Press and release at `pos`, with `mods` held.
fn click_with(h: &mut Harness<'_, DriftApp>, pos: Pos2, mods: EguiModifiers) {
    h.hover_at(pos);
    h.run();
    h.event(Event::ModifiersChanged(mods));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: mods,
        });
        h.run();
    }
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
}

fn double_click(h: &mut Harness<'_, DriftApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for _ in 0..2 {
        button(h, pos, true);
        h.step();
        button(h, pos, false);
        h.step();
    }
    h.run();
}

#[test]
fn a_picked_hatched_shape_shows_its_pattern_handles() {
    let Some(mut h) = harness() else { return };
    let (sides, fill) = hatched_shape(&mut h);
    // A click inside picks the shape: its outline and its hatch.
    click_world(&mut h, DVec2::new(25.0, 15.0));
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert_eq!(ed.selection, sides.iter().copied().collect());
    }
    let map = screen_map(&h);
    let handles = h
        .state_mut()
        .hatch_handles(&map)
        .expect("the pattern's handles");
    assert!(
        h.state_mut().drawing.editor.transform_box().is_some(),
        "with the transform box"
    );
    // The origin (the sketch's, outside the shape) shows in the middle, and the angle
    // handle's arm stays inside the area.
    let region = h.state_mut().drawing.editor.fill_shapes()[&fill]
        .region
        .clone();
    assert!(region.contains(handles.at));
    assert!(region.contains(map.to_world(handles.angle)));
    h.run();
    save_png("drift_ui_hatch_shape_handles.png", &mut h);
    // Dragged onto the bottom-left corner, the origin snaps onto it; the shape stays picked.
    let corner = screen(&h, DVec2::ZERO);
    drag_steps(
        &mut h,
        handles.origin,
        corner + egui::vec2(2.0, -1.0),
        5,
        None,
    );
    assert!(
        close(hatch(&h, fill).origin, DVec2::ZERO),
        "{:?}",
        hatch(&h, fill).origin
    );
    assert_eq!(h.state().drawing.editor.selected_fills, [fill].into());
    assert_eq!(
        h.state().drawing.editor.selection.len(),
        4,
        "the shape is still picked"
    );
    // Nothing moved but the pattern.
    let ed = &h.state().drawing.editor;
    let first = ed.doc.get(sides[0]).unwrap().kind;
    let EntityKind::Line { a, .. } = first else {
        unreachable!()
    };
    assert_eq!(ed.doc.point(a).unwrap(), DVec2::ZERO);
}

#[test]
fn the_inspector_aligns_the_pattern_to_corners_an_edge_and_back() {
    let Some(mut h) = harness() else { return };
    let (sides, fill) = hatched_shape(&mut h);
    click_world(&mut h, DVec2::new(25.0, 15.0));
    h.run();
    assert!(h.query_by_label("Pattern alignment").is_some());
    // The centre of the area, then a corner.
    h.get_by_label("Origin Centre").click();
    h.run();
    assert!(close(hatch(&h, fill).origin, DVec2::new(25.0, 15.0)));
    h.get_by_label("Origin Top right").click();
    h.run();
    assert!(close(hatch(&h, fill).origin, DVec2::new(50.0, 30.0)));
    // Align to edge…, then a click on the slanted side: the pattern runs along it from its
    // start.
    h.get_by_label("Align to edge…").click();
    h.run();
    assert_eq!(h.state().drawing.align_edge, Some(fill));
    click_world(&mut h, DVec2::new(45.0, 15.0));
    h.run();
    let aligned = hatch(&h, fill);
    let slope = 30.0_f64.atan2(10.0).to_degrees();
    assert!((aligned.angle - slope).abs() < 1e-6, "{}", aligned.angle);
    assert!(
        close(aligned.origin, DVec2::new(40.0, 0.0)),
        "{:?}",
        aligned.origin
    );
    assert_eq!(h.state().drawing.align_edge, None, "done after one click");
    assert_eq!(
        h.state().drawing.editor.selection,
        sides.iter().copied().collect(),
        "the click on the edge didn't change what is picked"
    );
    save_png("drift_ui_hatch_aligned_to_edge.png", &mut h);
    // One undo step.
    h.state_mut().undo();
    assert!(close(hatch(&h, fill).origin, DVec2::new(50.0, 30.0)));
    h.state_mut().redo();
    // Reset: the drawing's origin and 0°.
    h.run();
    h.get_by_label("Reset").click();
    h.run();
    let reset = hatch(&h, fill);
    assert_eq!((reset.origin, reset.angle), (DVec2::ZERO, 0.0));
    // Esc leaves Align to edge without changing anything.
    h.get_by_label("Align to edge…").click();
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().drawing.align_edge, None);
    assert_eq!(
        h.state().drawing.editor.selected_fills,
        [fill].into(),
        "still picked"
    );
}

#[test]
fn a_shape_moved_or_turned_carries_its_pattern() {
    let Some(mut h) = harness() else { return };
    let (sides, fill) = hatched_shape(&mut h);
    h.state_mut().grid_snap = false;
    {
        let ed = &mut h.state_mut().drawing.editor;
        assert!(ed.align_hatch_to_edge(fill, sides[1]));
    }
    let before = hatch(&h, fill);
    // Dragged by its inside: the pattern goes with it.
    click_world(&mut h, DVec2::new(18.0, 6.0));
    let from = screen(&h, DVec2::new(18.0, 6.0));
    let to = screen(&h, DVec2::new(30.0, 0.0));
    drag_steps(&mut h, from, to, 6, None);
    let corner = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().drawing.editor;
        let EntityKind::Line { a, .. } = ed.doc.get(sides[1]).unwrap().kind else {
            unreachable!()
        };
        ed.doc.point(a).unwrap()
    };
    let moved = corner(&h) - DVec2::new(40.0, 0.0);
    assert!(moved.length() > 1.0, "the shape moved: {moved:?}");
    let after = hatch(&h, fill);
    assert!(close(after.origin, before.origin + moved), "{after:?}");
    assert_eq!(after.angle, before.angle);
    // Turned a quarter about the corner: the pattern turns with it.
    {
        let ed = &mut h.state_mut().drawing.editor;
        let points = ed.doc.defining_points(sides.iter().copied());
        let pivot = corner(&h);
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let (ok, _) = ed.rotate_geometry(&sides, &points, pivot, 90.0);
        assert!(ok);
        ed.commit("Rotate");
    }
    let turned = hatch(&h, fill);
    assert!(
        (turned.angle - (before.angle + 90.0)).abs() < 1e-6,
        "{} -> {}",
        before.angle,
        turned.angle
    );
    assert!(close(turned.origin, corner(&h)), "still on the corner");
    // Undone, the pattern is back where it was.
    h.state_mut().undo();
    h.state_mut().undo();
    assert_eq!(hatch(&h, fill), before);
}

#[test]
fn alt_click_or_double_click_picks_the_fill_alone() {
    let Some(mut h) = harness() else { return };
    let (sides, fill) = hatched_shape(&mut h);
    let inside = screen(&h, DVec2::new(18.0, 6.0));
    // Alt-click: the hatch alone, with its handles and no transform box.
    click_with(&mut h, inside, EguiModifiers::ALT);
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty(), "{:?}", ed.selection);
    }
    let map = screen_map(&h);
    assert!(h.state_mut().hatch_handles(&map).is_some());
    assert!(h.state_mut().drawing.editor.transform_box().is_none());
    // A plain click picks the whole shape again; a double-click, the fill alone.
    click_world(&mut h, DVec2::new(18.0, 6.0));
    assert_eq!(h.state().drawing.editor.selection.len(), 4);
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    double_click(&mut h, inside);
    assert!(h.state().drawing.editor.selection.is_empty());
    assert_eq!(h.state().drawing.editor.selected_fills, [fill].into());

    // In a group: a click picks the group, a double-click enters it with the shape picked,
    // and another the fill alone (#159's steps, then this one).
    h.key_press(Key::Escape);
    h.run();
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = sides.iter().copied().collect();
        ed.make_group().expect("grouped");
        ed.selection.clear();
        ed.selected_fills.clear();
    }
    h.run();
    click_world(&mut h, DVec2::new(18.0, 6.0));
    assert!(h.state().drawing.editor.open_group.is_none());
    assert!(h.state().drawing.editor.selection.len() >= 4, "the group");
    double_click(&mut h, inside);
    {
        let ed = &h.state().drawing.editor;
        assert!(ed.open_group.is_some(), "into the group");
        assert_eq!(ed.selection, sides.iter().copied().collect());
    }
    double_click(&mut h, inside);
    let ed = &h.state().drawing.editor;
    assert!(ed.open_group.is_some());
    assert!(ed.selection.is_empty());
    assert_eq!(ed.selected_fills, [fill].into());
}
