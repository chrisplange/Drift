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

/// #327: lines picked with a text show the mixed Inspector, and it has the Pattern section
/// (it had none); Make linear pattern copies the text at each step with the lines, and one
/// undo takes it all back.
#[test]
fn smoke_pattern_takes_a_picked_text_along() {
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
        h.query_by_label("Pattern").is_some(),
        "the mixed Inspector has the Pattern section"
    );
    // Modify → Linear pattern opens it.
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, crate::commands::Command::Pattern(false));
    h.run();
    h.run();
    h.get_by_label("Make linear pattern").click();
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
