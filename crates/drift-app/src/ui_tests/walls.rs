//! The Wall tool as a user drives it (docs/walls-design.md §5.2, §9.3): W, W again and A / L
//! mid-run, Space flipping (and Space typed into a value, and Space repeating the tool from
//! Select), drawing and joining walls, bending a segment by dragging, Backspace and undo.

use super::*;

/// A drawing with walls 10 thick, centred, shown from (-20, -40) to (140, 100).
fn walls_harness() -> Option<Harness<'static, DriftApp>> {
    let mut h = harness()?;
    h.state_mut().grid_snap = false;
    h.state_mut().editor.prefs.wall_thickness = 10.0;
    h.state_mut().editor.prefs.wall_justify = drift_core::Justify::Centre;
    h.state_mut().editor.prefs.wall_arc = false;
    h.state_mut().fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-20.0, -40.0),
        max: DVec2::new(140.0, 100.0),
    });
    h.run();
    h.state_mut().fit_target = None;
    h.run();
    Some(h)
}

fn walls(h: &Harness<'_, DriftApp>) -> usize {
    h.state().editor.doc.walls().count()
}

fn poche(h: &mut Harness<'_, DriftApp>) -> f64 {
    let shapes = h.state_mut().editor.fill_shapes();
    shapes.values().map(|s| s.region.area).sum()
}

#[test]
fn w_draws_walls_that_join_and_undo_per_segment() {
    let Some(mut h) = walls_harness() else { return };
    h.key_press(Key::W);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    for p in [
        DVec2::new(0.0, 0.0),
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 60.0),
    ] {
        click_world(&mut h, p);
    }
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(walls(&h), 1);
    assert!((poche(&mut h) - 1600.0).abs() < 0.5, "{}", poche(&mut h));
    // A T: from the first wall's upper face, up.
    click_world(&mut h, DVec2::new(50.0, 5.0));
    click_world(&mut h, DVec2::new(50.0, 80.0));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(walls(&h), 2);
    assert_eq!(h.state().editor.doc.fills().count(), 1, "one poché");
    assert!(
        (poche(&mut h) - (1600.0 + 750.0)).abs() < 0.5,
        "{}",
        poche(&mut h)
    );
    save_png("drift_walls_t.png", &mut h);
    // Each segment was a step: undo takes the T's back, then the L's second segment.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(walls(&h), 1);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert!((poche(&mut h) - 1000.0).abs() < 0.5);
}

#[test]
fn w_again_arcs_a_and_l_switch_and_backspace_takes_back() {
    let Some(mut h) = walls_harness() else { return };
    h.key_press(Key::W);
    h.run();
    // W again: arcs, without leaving the tool.
    h.key_press(Key::W);
    h.run();
    assert!(h.state().editor.prefs.wall_arc);
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    h.key_press(Key::W);
    h.run();
    assert!(!h.state().editor.prefs.wall_arc);
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(100.0, 0.0));
    // A mid-run: an arc next (not the Arc tool).
    h.key_press(Key::A);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    assert!(h.state().editor.prefs.wall_arc);
    // Its end, then a point it curves through.
    click_world(&mut h, DVec2::new(100.0, 60.0));
    click_world(&mut h, DVec2::new(130.0, 30.0));
    let w = h.state().editor.doc.walls().next().unwrap().0;
    assert_eq!(h.state().editor.wall_curves(w).len(), 2);
    // L: straight again.
    h.key_press(Key::L);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    assert!(!h.state().editor.prefs.wall_arc);
    click_world(&mut h, DVec2::new(0.0, 60.0));
    assert_eq!(h.state().editor.wall_curves(w).len(), 3);
    // Backspace: the last segment goes, the run goes on.
    h.key_press(Key::Backspace);
    h.run();
    assert_eq!(h.state().editor.wall_curves(w).len(), 2);
    assert!(h.state().tool.is_busy());
    h.key_press(Key::Escape);
    h.run();
    save_png("drift_walls_arc.png", &mut h);
}

#[test]
fn space_flips_types_and_repeats() {
    let Some(mut h) = walls_harness() else { return };
    h.key_press(Key::W);
    h.run();
    h.state_mut().editor.prefs.wall_justify = drift_core::Justify::Left;
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(100.0, 0.0));
    // Space flips the run to its other face.
    h.key_press(Key::Space);
    h.run();
    let w = h.state().editor.doc.walls().next().unwrap().0;
    assert_eq!(
        h.state().editor.doc.wall(w).unwrap().justify,
        drift_core::Justify::Right
    );
    assert_eq!(h.state().tool.kind(), ToolKind::Wall, "Space didn't repeat");
    // Space while typing a length goes into it: 6 1/2" is one value.
    h.state_mut().editor.doc.set_units(drift_core::Units {
        length: drift_core::LengthFormat::Architectural,
        ..drift_core::Units::default()
    });
    // (Typed a character at a time, Space as a key and its text, as a keyboard sends them.)
    type_text(&mut h, "6");
    h.event(Event::Key {
        key: Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::NONE,
    });
    type_text(&mut h, " ");
    type_text(&mut h, "1/2\"");
    let typing = h.state().typing.as_ref().map(|t| t.values[0].clone());
    assert_eq!(typing.as_deref(), Some("6 1/2\""));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    // Space with nothing in hand: the Wall tool again.
    h.key_press(Key::Space);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    // Shift+Alt+W is still thin lines.
    let thin = h.state().thin_lines;
    h.key_press_modifiers(EguiModifiers::SHIFT | EguiModifiers::ALT, Key::W);
    h.run();
    assert_ne!(h.state().thin_lines, thin);
}

#[test]
fn a_drag_bends_the_next_segment() {
    let Some(mut h) = walls_harness() else { return };
    h.key_press(Key::W);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    // Press at the next corner and pull: the segment arrives curving along the pull.
    let (a, b) = (
        screen(&h, DVec2::new(100.0, 0.0)),
        screen(&h, DVec2::new(100.0, -30.0)),
    );
    drag_steps(&mut h, a, b, 6, None);
    h.key_press(Key::Enter);
    h.run();
    let arcs = count(&h, |k| matches!(k, EntityKind::Arc { .. }));
    assert_eq!(arcs, 3, "a bent centreline and its two faces");
    save_png("drift_walls_bent.png", &mut h);
}

#[test]
fn picking_a_wall_shows_its_inspector() {
    let Some(mut h) = walls_harness() else { return };
    h.key_press(Key::W);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(100.0, 0.0));
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    // A click inside its poché.
    click_world(&mut h, DVec2::new(50.0, 2.0));
    assert_eq!(h.state().editor.selected_walls().len(), 1);
    h.state_mut().show_panel(crate::dock::Panel::Inspector);
    h.run();
    assert!(
        h.query_by_label("Show thickness").is_some(),
        "the wall's Inspector"
    );
    h.get_by_label("Show thickness").click();
    h.run();
    assert_eq!(h.state().editor.doc.property_dims().count(), 1);
    save_png("drift_walls_inspector.png", &mut h);
}
