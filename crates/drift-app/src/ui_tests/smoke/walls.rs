//! Walls: draw a room and a wall teed onto it, pick a wall by its poché and change its
//! thickness, explode one, save and open again with the walls as they were.

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
fn smoke_walls_room_tee_and_thickness() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    assert_eq!(h.state().editor.doc.walls().count(), 1);
    let ring = 110.0 * 70.0 - 90.0 * 50.0;
    assert!((poche(&mut h) - ring).abs() < 0.5, "{}", poche(&mut h));
    // A wall teed onto the room's right side, out to the right.
    h.key_press(Key::W);
    h.run();
    click_world(&mut h, DVec2::new(105.0, 30.0));
    click_world(&mut h, DVec2::new(150.0, 30.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().editor.doc.walls().count(), 2);
    assert_eq!(
        h.state().editor.doc.fills().count(),
        1,
        "one poché for both"
    );
    assert!((poche(&mut h) - (ring + 450.0)).abs() < 0.5);
    // Picked by its poché, re-thickened from the Inspector's value.
    click_world(&mut h, DVec2::new(50.0, 1.0));
    let picked: Vec<_> = h.state().editor.selected_walls().into_iter().collect();
    assert_eq!(picked.len(), 1);
    h.state_mut()
        .editor
        .set_wall_thickness(&picked, "20")
        .unwrap();
    h.run();
    let thicker = 120.0 * 80.0 - 80.0 * 40.0;
    assert!(
        (poche(&mut h) - (thicker + 400.0)).abs() < 0.5,
        "{}",
        poche(&mut h)
    );
    save_png("drift_smoke_walls.png", &mut h);
    // Undo, and the room is as drawn.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert!((poche(&mut h) - (ring + 450.0)).abs() < 0.5);
}

#[test]
fn smoke_walls_save_open_and_explode() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    let before = h.state().editor.doc.clone();
    let dir = scratch_dir("smoke-walls");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Walls.drift");
    h.state_mut().file.path = Some(path.clone());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(h.state().editor.doc.walls().count(), 0);
    h.state().files.open_path(&path);
    h.run_steps(3);
    assert!(h.state().editor.notice.is_none(), "nothing repaired");
    assert!(
        h.state().editor.doc.same_sketch(&before.sketch_copy()),
        "the walls as they were"
    );
    // Picked by a click in its poché and exploded (Edit → Explode): plain lines and a fill.
    // (Opening keeps the window repainting for a moment, so frames are stepped here.)
    let pos = screen(&h, DVec2::new(50.0, 1.0));
    h.hover_at(pos);
    h.run_steps(2);
    for pressed in [true, false] {
        button(&h, pos, pressed);
        h.run_steps(2);
    }
    assert_eq!(h.state().editor.selected_walls().len(), 1);
    h.state_mut().editor.explode_selected();
    h.run_steps(2);
    assert_eq!(h.state().editor.doc.walls().count(), 0);
    assert_eq!(h.state().editor.doc.fills().count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
