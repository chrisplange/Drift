//! Dimensions and the solver: dimension a line and change its value; a drag repeated ends
//! where it ended before; a drag of an under-constrained chain leaves what it doesn't hold
//! where it was (#143).

use super::*;

fn length(h: &Harness<'_, DriftApp>, line: EntityId) -> f64 {
    let (a, b) = ends(h, line);
    at(h, a).distance(at(h, b))
}

/// Lines through `points` with the Line tool, one chain; back to Select. Returns them.
fn chain(h: &mut Harness<'_, DriftApp>, points: &[DVec2]) -> Vec<EntityId> {
    let before = ids(h);
    h.key_press(Key::L);
    h.run();
    for p in points {
        click_world(h, *p);
    }
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let new = curves(h, &added(h, &before));
    assert_eq!(new.len(), points.len() - 1, "the chain drawn");
    h.state_mut().editor.selection.clear();
    new
}

#[test]
fn smoke_dimensions_dimension_a_line_and_change_its_value() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-40.0, -30.0), DVec2::new(60.0, 40.0));
    let line = chain(&mut h, &[DVec2::new(-20.0, 0.0), DVec2::new(20.0, 0.0)])[0];
    // D, the line, a place above it, a typed value.
    h.key_press(Key::D);
    h.run();
    click_world(&mut h, DVec2::new(-10.0, 0.0));
    click_world(&mut h, DVec2::new(5.0, 15.0));
    assert!(h.state().dimension_edit.is_some(), "its value to type");
    type_text(&mut h, "30");
    h.key_press(Key::Enter);
    h.run();
    assert!(
        (length(&h, line) - 30.0).abs() < 1e-9,
        "{}",
        length(&h, line)
    );
    save_png("drift_smoke_dimension.png", &mut h);
    // A double-click on its text with the select tool, a new value.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::S);
    h.run();
    let label = first_dimension(&h).text_rect.center();
    double_click(&mut h, label);
    assert!(h.state().dimension_edit.is_some(), "editing it again");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "45");
    h.key_press(Key::Enter);
    h.run();
    assert!(
        (length(&h, line) - 45.0).abs() < 1e-9,
        "{}",
        length(&h, line)
    );
    h.state_mut().undo();
    assert!((length(&h, line) - 30.0).abs() < 1e-9, "one undo step");
}

/// The same drag twice from the same drawing ends in the same place to the last bit, and a
/// drag of one line of a chain moves only that line's ends: the next line's far end, and a
/// line nothing links to, stay where they were (#143).
#[test]
fn smoke_dimensions_drags_are_repeatable_and_leave_the_rest() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-20.0, -40.0), v(120.0, 70.0));
    let lines = chain(
        &mut h,
        &[v(0.0, 0.0), v(30.0, 12.0), v(45.0, 42.0), v(80.0, 55.0)],
    );
    let apart = chain(&mut h, &[v(90.0, -20.0), v(110.0, -5.0)])[0];
    let start = positions(&h);
    let (_, c) = ends(&h, lines[1]);
    let (_, d) = ends(&h, lines[2]);
    let (fa, fb) = ends(&h, apart);
    let kept = [c, d, fa, fb].map(|p| at(&h, p));
    let pull = |h: &mut Harness<'_, DriftApp>| {
        drag(h, v(10.0, 4.0), v(10.0, -16.0), 6);
        h.key_press(Key::Escape);
        h.run();
        positions(h)
    };
    let first = pull(&mut h);
    assert_ne!(first, start, "the drag moved something");
    assert_eq!(
        [c, d, fa, fb].map(|p| at(&h, p)),
        kept,
        "the next line's far end, the rest of the chain and the line apart stayed"
    );
    h.state_mut().undo();
    h.run();
    assert_eq!(positions(&h), start, "undone");
    let again = pull(&mut h);
    assert_eq!(again, first, "the same drag, the same result");
}
