//! Options: the Parameters panel's Save current, as values only and as the whole sketch, then
//! each applied with a click and undone.

use drift_core::Constraint;

use super::*;

/// A horizontal line from the origin, its length a dimension driven by the variable `w` = 25.
fn dimensioned(h: &mut Harness<'_, DriftApp>) -> (EntityId, drift_core::VariableId) {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::ZERO);
    let b = ed.doc.add_point(DVec2::new(40.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
        .unwrap();
    ed.add_constraints(
        "Fix",
        &[Constraint::Fix {
            point: a,
            at: DVec2::ZERO,
        }],
    )
    .unwrap();
    let d = ed
        .add_dimension(Constraint::Distance { a, b, value: 40.0 }, None)
        .unwrap();
    let w = ed
        .add_variable("w", "25", drift_core::Kind::Length)
        .unwrap();
    ed.set_dimension_text(d, "w").unwrap();
    h.state_mut().show_panel(Panel::Parameters);
    h.run();
    h.run();
    (line, w)
}

fn length(h: &Harness<'_, DriftApp>, line: EntityId) -> f64 {
    let (a, b) = ends(h, line);
    at(h, a).distance(at(h, b))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn smoke_options_values_and_whole_sketch_apply_and_undo() {
    let Some(mut h) = harness() else { return };
    let (line, w) = dimensioned(&mut h);
    assert!(close(length(&h, line), 25.0));
    // Save current, values only (the default): "Option 1".
    h.get_by_label("Save current").click();
    h.run();
    // w = 60, and the whole sketch saved as "Option 2".
    h.state_mut().editor.set_variable(w, "60").unwrap();
    h.run();
    h.get_by_value("Values only").click();
    h.run();
    h.get_by_label("Whole sketch").click();
    h.run();
    h.get_by_label("Save current").click();
    h.run();
    let kinds: Vec<bool> = h
        .state()
        .editor
        .doc
        .variants()
        .map(|(_, v)| v.is_sketch())
        .collect();
    assert_eq!(kinds, [false, true], "one of each");
    // Something drawn after, which only the whole sketch takes away.
    let extra = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 30.0));
        let b = ed.doc.add_point(DVec2::new(30.0, 30.0));
        let extra = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        extra
    };
    h.run();
    save_png("drift_smoke_options.png", &mut h);
    // A click on Option 1 applies its values: w back to 25, the extra line kept.
    h.get_by_label("Option 1").click();
    h.run();
    assert!(close(length(&h, line), 25.0), "{}", length(&h, line));
    assert!(h.state().editor.doc.get(extra).is_some());
    h.state_mut().undo();
    h.run();
    assert!(close(length(&h, line), 60.0), "undone");
    // A click on Option 2 brings the whole sketch back as it was: no extra line.
    h.get_by_label("Option 2").click();
    h.run();
    assert!(h.state().editor.doc.get(extra).is_none());
    assert!(close(length(&h, line), 60.0));
    h.state_mut().undo();
    h.run();
    assert!(h.state().editor.doc.get(extra).is_some(), "undone");
}
