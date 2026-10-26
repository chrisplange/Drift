//! Openings: a door placed in a room's wall with the Door / window tool (Shift+O, Space for
//! its swing), picked by its jamb and deleted, undone, then saved and opened again as it was.

use super::*;

/// A ring of walls round a 100 × 60 room, 10 thick, with the Wall tool; back to Select.
fn room(h: &mut Harness<'_, DriftApp>) {
    h.state_mut().editor.wall_thickness = 10.0;
    h.state_mut().editor.wall_justify = drift_core::Justify::Centre;
    h.state_mut().editor.wall_arc = false;
    h.key_press(Key::W);
    h.run();
    for p in [
        DVec2::new(0.0, 0.0),
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 60.0),
        DVec2::new(0.0, 60.0),
        DVec2::new(0.0, 0.0),
    ] {
        click_world(h, p);
    }
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
}

fn poche(h: &mut Harness<'_, DriftApp>) -> f64 {
    let shapes = h.state_mut().editor.fill_shapes();
    shapes.values().map(|s| s.region.area).sum()
}

#[test]
fn smoke_openings_place_pick_delete_and_reopen() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    let ring = poche(&mut h);
    // The Door / window tool, a door 30 wide (its leaf 28).
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::O);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Opening);
    {
        let ed = &mut h.state_mut().editor;
        ed.set_opening_family("door.single");
        ed.opening.width = 30.0;
        ed.opening.leaf = 28.0;
    }
    let swing = (
        h.state().editor.opening.hand,
        h.state().editor.opening.facing,
    );
    let at = screen(&h, DVec2::new(50.0, 1.0));
    h.hover_at(at);
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert_ne!(
        (
            h.state().editor.opening.hand,
            h.state().editor.opening.facing
        ),
        swing,
        "Space steps the swing"
    );
    click_world(&mut h, DVec2::new(50.0, 1.0));
    assert_eq!(h.state().editor.doc.openings().count(), 1);
    assert!(
        (ring - poche(&mut h) - 30.0 * 10.0).abs() < 0.5,
        "the door cuts the poché: {}",
        poche(&mut h)
    );
    save_png("drift_smoke_openings.png", &mut h);
    // Back to Select; a click on its jamb picks the door, Delete takes it out.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(35.0, 3.0));
    assert_eq!(
        h.state().editor.selected_openings().len(),
        1,
        "the door is picked"
    );
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(h.state().editor.doc.openings().count(), 0);
    assert_eq!(h.state().editor.doc.walls().count(), 1, "its wall stays");
    assert!((poche(&mut h) - ring).abs() < 0.5, "the wall closes up");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().editor.doc.openings().count(), 1);
    // Saved, opened again: as it was, with nothing repaired.
    let before = h.state().editor.doc.clone();
    let dir = scratch_dir("smoke-openings");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Door.drift");
    h.state_mut().file.path = Some(path.clone());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(h.state().editor.doc.openings().count(), 0);
    h.state().files.open_path(&path);
    h.run_steps(3);
    assert!(h.state().editor.notice.is_none(), "nothing repaired");
    assert!(
        h.state().editor.doc.same_sketch(&before.sketch_copy()),
        "the door as it was"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
