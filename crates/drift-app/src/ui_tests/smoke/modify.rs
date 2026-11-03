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
