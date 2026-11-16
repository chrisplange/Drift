//! Modify tools: Fillet on several corners at once (#293).

use super::*;

fn radii(h: &Harness<'_, DriftApp>) -> Vec<f64> {
    let doc = &h.state().drawing.editor.doc;
    doc.iter()
        .filter_map(|(id, _)| match doc.shape(id) {
            Some(drift_core::Shape::Arc { radius, .. }) => Some((radius * 1e9).round() / 1e9),
            _ => None,
        })
        .collect()
}

/// #293: a picked rectangle, F, 3, Enter rounds all four corners in one undo step (a
/// right-click before it rounds none); then
/// Shift+click two corners, click a third, 2, Enter rounds those three.
#[test]
fn smoke_fillet_several_corners_in_one_undo() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(50.0, 40.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    h.state_mut().drawing.editor.selection = sides.iter().copied().collect();
    h.run();
    h.key_press(Key::F);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Fillet);
    // A right-click (the canvas menu) leaves the four corners picked and rounds none.
    let inside = screen(&h, DVec2::new(20.0, 15.0));
    right_click(&mut h, inside);
    assert!(radii(&h).is_empty(), "a right-click doesn't apply the set");
    assert!(
        h.state().tool.hint().starts_with("4 corners"),
        "{}",
        h.state().tool.hint()
    );
    type_text(&mut h, "3");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(radii(&h), [3.0; 4], "{}", status(&h));
    undo(&mut h);
    assert!(radii(&h).is_empty(), "one undo for all four");

    // Shift+click two corners, click a third: three sized together.
    h.state_mut().drawing.editor.selection.clear();
    h.run();
    for c in [DVec2::new(0.0, 0.0), DVec2::new(40.0, 0.0)] {
        let at = screen(&h, c);
        click_with(&mut h, at, EguiModifiers::SHIFT);
    }
    assert!(
        h.state().tool.hint().starts_with("2 corners"),
        "{}",
        h.state().tool.hint()
    );
    click_world(&mut h, DVec2::new(40.0, 30.0));
    assert!(
        h.state().tool.hint().contains("3 fillets"),
        "{}",
        h.state().tool.hint()
    );
    save_png("drift_ui_fillet_three_corners.png", &mut h);
    type_text(&mut h, "2");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(radii(&h), [2.0; 3], "{}", status(&h));
}

/// #327: lines picked with a text show the mixed Inspector, and it has the Array section
/// (it had none); Apply copies the text at each step with the lines, and one
/// undo takes it all back.
#[test]
fn smoke_array_takes_a_picked_text_along() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(80.0, 40.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(10.0, 10.0), Fill::No);
    let text = {
        let ed = &mut h.state_mut().drawing.editor;
        let text = ed.add_text(drift_core::SketchText::new(
            "Label",
            DVec2::new(2.0, 15.0),
            2.0,
        ));
        ed.selection = sides.iter().copied().collect();
        ed.selected_texts = [text].into();
        text
    };
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Array").is_some(),
        "the mixed Inspector has the Array section"
    );
    // Modify → Linear array opens it.
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Array(false));
    h.run();
    h.run();
    h.get_by_label("Apply").click();
    h.run();
    let at = |h: &Harness<'_, DriftApp>| {
        let mut xs: Vec<f64> = h
            .state()
            .drawing
            .editor
            .doc
            .texts()
            .map(|(_, t)| (t.at.x * 1e6).round() / 1e6)
            .collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    assert_eq!(at(&h), [2.0, 22.0, 42.0], "20 apart, the default spacing");
    assert_eq!(lines(&h), 12);
    undo(&mut h);
    assert_eq!(at(&h), [2.0]);
    assert!(h.state().drawing.editor.doc.text(text).is_some());
}

/// #264 (lead 2): picked lines, Shift+D, then Esc during the move leaves the drawing as it
/// was: the unmoved copy is removed, not left stacked on the originals, and the originals
/// are picked again.
#[test]
fn smoke_shift_d_then_esc_leaves_entity_count_unchanged() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(50.0, 40.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    let picked: std::collections::BTreeSet<_> = sides.iter().copied().collect();
    h.state_mut().drawing.editor.selection = picked.clone();
    h.run();
    let count = |h: &Harness<'_, DriftApp>| h.state().drawing.editor.doc.iter().count();
    let before = count(&h);
    h.hover_at(screen(&h, DVec2::new(20.0, 0.0)));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    assert!(count(&h) > before, "duplicated, and being moved");
    assert!(h.state().tool.is_busy());
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(count(&h), before, "Esc removed the unmoved copy");
    assert!(!h.state().tool.is_busy());
    assert_eq!(h.state().drawing.editor.selection, picked);
    assert!(status(&h).contains("Copy removed"), "{}", status(&h));
}

/// #264 follow-up: Shift+D with the pointer off the canvas (on a panel, or gone from the
/// window) still leaves no copy stacked on the originals. The move starts from where the
/// pointer last was on the canvas, so Esc removes the copy; with no pointer ever on the
/// canvas, nothing is copied and the status says why.
#[test]
fn smoke_shift_d_with_the_pointer_off_the_canvas_leaves_no_stacked_copy() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(50.0, 40.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    let picked: std::collections::BTreeSet<_> = sides.iter().copied().collect();
    h.state_mut().drawing.editor.selection = picked.clone();
    h.run();
    let count = |h: &Harness<'_, DriftApp>| h.state().drawing.editor.doc.iter().count();
    let before = count(&h);
    h.hover_at(screen(&h, DVec2::new(20.0, 0.0)));
    h.run();
    h.event(egui::Event::PointerGone);
    h.run();
    assert!(
        h.state().drawing.view.pointer.is_none(),
        "pointer off the canvas"
    );
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    // (The rectangle is the whole drawing, so the copy doubles the count.)
    assert_eq!(count(&h), 2 * before, "duplicated");
    assert!(
        h.state().tool.is_busy(),
        "the copy is being moved, from the last pointer"
    );
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(count(&h), before, "no copy left stacked on the originals");
    assert!(!h.state().tool.is_busy());
    assert_eq!(h.state().drawing.editor.selection, picked);
    // No pointer on the canvas at all yet: nothing is copied, and it says why.
    h.state_mut().drawing.view.last_pointer = None;
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    assert_eq!(count(&h), before, "nothing copied without a pointer");
    assert!(!h.state().tool.is_busy());
    assert!(status(&h).contains("point at the canvas"), "{}", status(&h));
    assert!(!status_is_error(&h), "a plain hint, as for Move");
}

/// The array preview's copies on the canvas now, if any.
fn array_preview(h: &Harness<'_, DriftApp>) -> Option<Vec<drift_core::SketchItem>> {
    h.state()
        .arrays
        .preview_items(h.ctx.cumulative_frame_nr())
        .map(|items| items.to_vec())
}

/// The left edge of the preview's copies (mm), to see where they went.
fn preview_left(items: &[drift_core::SketchItem]) -> f64 {
    let b: drift_core::Bounds = items.iter().map(|i| i.shape.bounds()).collect();
    (b.min.x * 1e6).round() / 1e6
}

/// #455: Modify → Linear array previews the copies on the canvas with nothing committed;
/// count and spacing (an expression, or a length with units) redraw it; Esc leaves the
/// drawing exactly as it was; Apply makes it as one undo step, and undo takes it back.
#[test]
fn smoke_array_previews_until_applied() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(120.0, 40.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(10.0, 10.0), Fill::No);
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.add_variable("W", "60", drift_core::Kind::Length)
            .unwrap();
        ed.selection = sides.iter().copied().collect();
    }
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    let before = positions(&h);
    let entities = ids(&h).len();
    let steps = h.state().drawing.editor.history.undo_depth();
    let unchanged = |h: &Harness<'_, DriftApp>| {
        assert_eq!(positions(h), before, "every point where it was");
        assert_eq!(ids(h).len(), entities, "nothing added");
        assert_eq!(h.state().drawing.editor.history.undo_depth(), steps);
    };
    assert!(
        array_preview(&h).is_none(),
        "no preview before the array is opened"
    );

    // Modify → Linear array: 3 (two copies of 4 sides) at the default 20 mm.
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Array(false));
    h.run();
    h.run();
    let items = array_preview(&h).expect("the preview shows");
    assert_eq!(items.len(), 8);
    assert!(
        items
            .iter()
            .all(|i| i.emphasis == drift_core::Emphasis::Preview)
    );
    assert_eq!(preview_left(&items), 20.0);
    unchanged(&h);

    // A count of 5: four copies.
    h.state_mut().arrays.linear_count = 5;
    h.run();
    assert_eq!(array_preview(&h).unwrap().len(), 16);
    unchanged(&h);

    // Typed into Spacing X: W/3 is 20 mm, 3 m is 3000 mm (the preview follows each).
    for (text, left) in [("W/3", 20.0), ("3 m", 3000.0), ("W/2", 30.0)] {
        h.get_by_label("Spacing X").click();
        h.run();
        h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
        type_text(&mut h, text);
        assert_eq!(h.state().arrays.dx, text);
        assert_eq!(preview_left(&array_preview(&h).unwrap()), left, "{text}");
        unchanged(&h);
    }
    // A spacing that doesn't work out keeps the last good preview.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "W/");
    assert_eq!(preview_left(&array_preview(&h).unwrap()), 30.0);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "W/2");

    // Esc (in the field) ends the preview; the drawing is exactly as it was.
    h.key_press(Key::Escape);
    h.run();
    assert!(array_preview(&h).is_none(), "Esc ends the preview");
    unchanged(&h);

    // Open it again; Esc on the canvas ends it too.
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Array(false));
    h.run();
    h.run();
    assert!(array_preview(&h).is_some());
    h.key_press(Key::Escape);
    h.run();
    assert!(
        array_preview(&h).is_none(),
        "Esc on the canvas ends the preview"
    );
    unchanged(&h);
    assert!(
        !h.state().drawing.editor.selection.is_empty(),
        "that Esc kept the picks"
    );

    // Apply: five rectangles 30 mm apart, as one undo step.
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Array(false));
    h.run();
    h.run();
    h.get_by_label("Apply").click();
    h.run();
    assert_eq!(lines(&h), 20, "{}", status(&h));
    assert!(array_preview(&h).is_none(), "applying ends the preview");
    assert_eq!(h.state().drawing.editor.history.undo_depth(), steps + 1);
    let xs: std::collections::BTreeSet<i64> = positions(&h)
        .iter()
        .map(|(_, p)| p.x.round() as i64)
        .collect();
    assert_eq!(
        xs.into_iter().collect::<Vec<_>>(),
        [0, 10, 30, 40, 60, 70, 90, 100, 120, 130]
    );
    undo(&mut h);
    unchanged(&h);
}

/// #455: the circular array previews round its centre, and the Linear / Circular switch
/// changes what's previewed.
#[test]
fn smoke_circular_array_previews_round_its_centre() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-40.0, -40.0), DVec2::new(40.0, 40.0));
    let (sides, _) = rectangle(
        &mut h,
        DVec2::new(10.0, -2.0),
        DVec2::new(20.0, 2.0),
        Fill::No,
    );
    h.state_mut().drawing.editor.selection = sides.iter().copied().collect();
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Array(true));
    h.run();
    h.run();
    // Six round the origin: five copies; the one half way round sits at x -20..-10.
    let items = array_preview(&h).expect("the preview shows");
    assert_eq!(items.len(), 20);
    assert_eq!(preview_left(&items), -20.0);
    // Four over 90°: the copies turn 30° each, none past the y axis.
    h.state_mut().arrays.circular_count = 4;
    h.get_by_label("Angle").click();
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "90°");
    let items = array_preview(&h).unwrap();
    assert_eq!(items.len(), 12);
    assert!(preview_left(&items) > -2.5, "{}", preview_left(&items));
    // The switch to Linear previews the row instead.
    h.get_by_label("Linear").click();
    h.run();
    assert_eq!(preview_left(&array_preview(&h).unwrap()), 30.0);
    assert_eq!(lines(&h), 4, "nothing made yet");
}
