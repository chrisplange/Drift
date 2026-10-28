//! Dimensions and the solver: dimension a line and change its value; a drag repeated ends
//! where it ended before; a drag of an under-constrained chain leaves what it doesn't hold
//! where it was (#143); Fix on a partly defined line pins only what is still free (#203).

use drift_core::Constraint;

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

/// Fix on a level line whose length is dimensioned: accepted (it used to be refused as
/// "already implied"), it pins only the freedom left (one end; the length and the Horizontal
/// hold the other), the drawing is then fully defined, and a drag doesn't move it. Fix again
/// says it is already fully defined (#203).
#[test]
fn smoke_dimensions_fix_pins_what_is_still_free() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    view(&mut h, v(-40.0, -30.0), v(60.0, 40.0));
    let line = chain(&mut h, &[v(-20.0, 0.0), v(20.0, 0.0)])[0];
    let (a, b) = ends(&h, line);
    let ed = &mut h.state_mut().editor;
    assert!(
        ed.doc
            .constraints()
            .any(|(_, c)| matches!(c, Constraint::Horizontal { .. })),
        "drawn level"
    );
    ed.add_constraints("Length", &[Constraint::Distance { a, b, value: 40.0 }])
        .unwrap();
    // Pick the line, Fix.
    h.key_press(Key::S);
    h.run();
    click_world(&mut h, v(-10.0, 0.0));
    assert_eq!(h.state().editor.selection.len(), 1, "the line picked");
    let fix = h
        .state()
        .editor
        .relation_options()
        .into_iter()
        .find(|o| o.label == "Fix")
        .expect("Fix offered");
    h.state_mut().apply_constraint_option(&fix, false, 0.0);
    h.run();
    let ed = &h.state().editor;
    let pins: Vec<Constraint> = ed
        .doc
        .constraints()
        .filter(|(_, c)| c.is_point_fix())
        .map(|(_, c)| *c)
        .collect();
    assert!(
        matches!(pins.as_slice(), [Constraint::Fix { point, .. }] if *point == a),
        "one end pinned, nothing more: {pins:?}"
    );
    assert_eq!(ed.analysis().dof, Some(0), "fully defined");
    let before = positions(&h);
    drag(&mut h, v(-10.0, 0.0), v(0.0, 15.0), 5);
    h.key_press(Key::Escape);
    h.run();
    for ((_, was), (_, now)) in before.iter().zip(positions(&h)) {
        assert!(
            was.distance(now) < 1e-9,
            "a drag doesn't move it: {was} {now}"
        );
    }
    // Fix again: nothing left to pin.
    h.state_mut().editor.selection = [line].into();
    let err = h
        .state_mut()
        .editor
        .add_constraints("Fix", &fix.constraints);
    assert_eq!(
        err.unwrap_err().to_string(),
        "this line is already fully defined"
    );
    // The relations pie shows Fix on (one end pinned, the other held by the rest), and
    // choosing it takes the pin off.
    let slices = h.state().pie_slices();
    let place = slices
        .iter()
        .position(|s| s.label == "Fix")
        .expect("Fix in the pie");
    assert!(slices[place].on, "Fix shows as on");
    h.state_mut().pie_choose(place, 0.0);
    h.run();
    let ed = &h.state().editor;
    assert!(
        !ed.doc.constraints().any(|(_, c)| c.is_point_fix()),
        "the pin is off"
    );
    assert_eq!(ed.analysis().dof, Some(2), "free to move again");
}

/// #235: an offset of a rectangle leaves one offset dimension on the canvas, and a new value
/// typed into it (double-click its text) moves every side; one undo puts it back.
#[test]
fn smoke_dimensions_an_offset_has_one_dimension_for_all_its_sides() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(60.0, 50.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    let before = ids(&h);
    // O, a side, the outside, 2, Enter.
    palette(&mut h, "Offset");
    assert_eq!(h.state().tool.kind(), ToolKind::Offset);
    click_world(&mut h, DVec2::new(20.0, 0.0));
    h.hover_at(screen(&h, DVec2::new(20.0, -5.0)));
    h.run();
    type_text(&mut h, "2");
    h.key_press(Key::Enter);
    h.run();
    let offsets = curves(&h, &added(&h, &before));
    assert_eq!(offsets.len(), 4, "the offset rectangle");
    // Each offset side's distance from its original (from its middle).
    let gaps = |h: &Harness<'_, DriftApp>| -> Vec<f64> {
        let doc = &h.state().editor.doc;
        offsets
            .iter()
            .map(|o| {
                let (a, b) = ends(h, *o);
                let middle = (at(h, a) + at(h, b)) * 0.5;
                sides
                    .iter()
                    .filter_map(|s| doc.shape(*s))
                    .map(|s| drift_geom::distance(&s, middle))
                    .fold(f64::INFINITY, f64::min)
            })
            .collect()
    };
    assert!(
        gaps(&h).iter().all(|g| (g - 2.0).abs() < 1e-9),
        "{:?}",
        gaps(&h)
    );
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let shown = h.state().layout_dimensions(&painter, &screen_map(&h));
    assert_eq!(shown.len(), 1, "one offset dimension on the canvas");
    // A new value in it moves all four sides.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::S);
    h.run();
    double_click(&mut h, shown[0].text_rect.center());
    assert!(h.state().dimension_edit.is_some(), "editing the offset");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "3");
    h.key_press(Key::Enter);
    h.run();
    assert!(
        gaps(&h).iter().all(|g| (g - 3.0).abs() < 1e-9),
        "{:?}",
        gaps(&h)
    );
    undo(&mut h);
    assert!(
        gaps(&h).iter().all(|g| (g - 2.0).abs() < 1e-9),
        "{:?}",
        gaps(&h)
    );
    // A drag of a side changes the offset, in round steps for the zoom: let go 6.3 out, it
    // lands on a whole step; every side follows and the rectangle stays; one undo puts it
    // back. With Alt, freely: every side at 6.3.
    let corner = at(&h, ends(&h, sides[0]).0);
    h.key_press(Key::Escape);
    h.run();
    let from = screen(&h, DVec2::new(20.0, -2.0));
    let to = screen(&h, DVec2::new(20.0, -6.3));
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    drag_steps(&mut h, from, to, 6, None);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    assert!(
        gaps(&h).iter().all(|g| (g - 6.3).abs() < 1e-9),
        "freely: {:?}",
        gaps(&h)
    );
    assert_eq!(at(&h, ends(&h, sides[0]).0), corner, "the rectangle stays");
    undo(&mut h);
    drag_steps(&mut h, from, to, 6, None);
    let tolerance = h.state().last_pointer.as_ref().unwrap().tolerance;
    let step = crate::tools::corners::round_step(tolerance * 1.5, true);
    let expected = (6.3 / step).round() * step;
    assert!(step >= 0.1, "a step to see at this zoom: {step}");
    assert!(
        gaps(&h).iter().all(|g| (g - expected).abs() < 1e-9),
        "{:?}, step {step}",
        gaps(&h)
    );
    assert_eq!(at(&h, ends(&h, sides[0]).0), corner, "the rectangle stays");
    undo(&mut h);
    assert!(
        gaps(&h).iter().all(|g| (g - 2.0).abs() < 1e-9),
        "{:?}",
        gaps(&h)
    );
}
