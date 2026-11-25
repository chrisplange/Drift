//! Openings: a door placed in a room's wall with the Opening tool (Shift+O, Space for
//! its swing), picked by its jamb and deleted, undone, then saved and opened again as it was.

use super::*;

/// A ring of walls round a 100 × 60 room, 10 thick, with the Wall tool; back to Select.
fn room(h: &mut Harness<'_, DriftApp>) {
    h.state_mut().drawing.editor.prefs.wall_thickness = 10.0;
    h.state_mut().drawing.editor.prefs.wall_justify = drift_core::Justify::Centre;
    h.state_mut().drawing.editor.prefs.wall_arc = false;
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
    let shapes = h.state_mut().drawing.editor.fill_shapes();
    shapes.values().map(|s| s.region.area).sum()
}

#[test]
fn smoke_openings_place_pick_delete_and_reopen() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    let ring = poche(&mut h);
    // The Opening tool, a door 30 wide (its leaf 28).
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::O);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Opening);
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.set_opening_family("door.single");
        ed.prefs.opening.width = 30.0;
        ed.prefs.opening.leaf = 28.0;
    }
    let swing = (
        h.state().drawing.editor.prefs.opening.hand,
        h.state().drawing.editor.prefs.opening.facing,
    );
    let at = screen(&h, DVec2::new(50.0, 1.0));
    h.hover_at(at);
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert_ne!(
        (
            h.state().drawing.editor.prefs.opening.hand,
            h.state().drawing.editor.prefs.opening.facing
        ),
        swing,
        "Space steps the swing"
    );
    click_world(&mut h, DVec2::new(50.0, 1.0));
    assert_eq!(h.state().drawing.editor.doc.openings().count(), 1);
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
        h.state().drawing.editor.selected_openings().len(),
        1,
        "the door is picked"
    );
    let started = h.state().solve_motion.started;
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(h.state().drawing.editor.doc.openings().count(), 0);
    assert_eq!(
        h.state().drawing.editor.doc.walls().count(),
        1,
        "its wall stays"
    );
    assert!((poche(&mut h) - ring).abs() < 0.5, "the wall closes up");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().drawing.editor.doc.openings().count(), 1);
    assert_eq!(
        h.state().solve_motion.started,
        started,
        "taken out and put back, the door lands at once: nothing to slide between"
    );
    // Saved, opened again: as it was, with nothing repaired.
    let before = h.state().drawing.editor.doc.clone();
    let dir = scratch_dir("smoke-openings");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Door.drift");
    h.state_mut().drawing.file.path = Some(path.clone());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(h.state().drawing.editor.doc.openings().count(), 0);
    h.state().files.open_path(&path);
    h.run_steps(3);
    assert!(
        h.state().drawing.editor.out.notice.is_none(),
        "nothing repaired"
    );
    assert!(
        h.state()
            .drawing
            .editor
            .doc
            .same_sketch(&before.sketch_copy()),
        "the door as it was"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A harness whose frames are 20 ms apart: several to a solve easing into place.
fn easing_harness() -> Option<Harness<'static, DriftApp>> {
    if !crate::test_gpu::available() {
        return None;
    }
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_step_dt(0.02)
        .with_max_steps(60)
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    Some(h)
}

/// A door moved by its Offset in the Inspector eases into place without the walls
/// wobbling: the faces slide and the poché is made again from them each frame, so the walls
/// away from the door stay exactly as they are (carried by one map, the whole poché slid
/// askew for a moment).
#[test]
fn smoke_openings_moved_by_offset_ease_without_a_wobble() {
    let Some(mut h) = easing_harness() else {
        return;
    };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::O);
    h.run();
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.set_opening_family("door.single");
        ed.prefs.opening.width = 30.0;
        ed.prefs.opening.leaf = 28.0;
    }
    click_world(&mut h, DVec2::new(50.0, 1.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(35.0, 3.0));
    let o = *h
        .state()
        .drawing
        .editor
        .selected_openings()
        .iter()
        .next()
        .expect("the door is picked");
    h.state_mut().animate_solves = true;
    h.run();
    let before = h.render().expect("render");
    let started = h.state().solve_motion.started;
    h.state_mut()
        .drawing
        .editor
        .set_opening_offset(o, "10")
        .unwrap();
    h.step();
    assert_eq!(h.state().solve_motion.started, started + 1, "it eases");
    h.step();
    assert!(h.state().solve_motion.animating());
    let mid = h.render().expect("render");
    save_png("drift_smoke_opening_easing.png", &mut h);
    h.run();
    assert!(!h.state().solve_motion.animating());
    let settled = h.render().expect("render");
    // Patches of wall the door never comes near: the left and top walls' middles, and the
    // bottom wall's left end.
    for at in [
        DVec2::new(0.0, 30.0),
        DVec2::new(50.0, 60.0),
        DVec2::new(15.0, 0.0),
    ] {
        let c = screen(&h, at);
        let (x0, y0) = (c.x as u32 - 4, c.y as u32 - 4);
        for y in y0..y0 + 8 {
            for x in x0..x0 + 8 {
                assert_eq!(
                    mid.get_pixel(x, y),
                    settled.get_pixel(x, y),
                    "the wall at {at:?} stays put while the door slides"
                );
            }
        }
    }
    assert!(
        mid.as_raw() != settled.as_raw(),
        "part-way, the door is between"
    );
    // The poché's gap for the door, across the bottom wall: part-way too, between where it
    // was and where it lands (not cut where the door lands while its jambs slide).
    // (Off the wall's centreline, which is drawn dashed.)
    let at = screen(&h, DVec2::new(-3.0, 30.0));
    let poche = settled.get_pixel(at.x as u32, at.y as u32).0;
    let (left, right) = (
        screen(&h, DVec2::new(0.0, -3.0)),
        screen(&h, DVec2::new(100.0, -3.0)),
    );
    let gap = |pixel: &dyn Fn(u32, u32) -> [u8; 4]| {
        let y = left.y as u32;
        // The longest run of pixels that aren't poché.
        let (mut best, mut run) = ((0, 0), None);
        for x in left.x as u32..=right.x as u32 {
            if pixel(x, y) == poche {
                run = None;
                continue;
            }
            let from = *run.get_or_insert(x);
            if x - from > best.1 - best.0 {
                best = (from, x);
            }
        }
        best
    };
    let (was, shown, lands) = (
        gap(&|x, y| before.get_pixel(x, y).0),
        gap(&|x, y| mid.get_pixel(x, y).0),
        gap(&|x, y| settled.get_pixel(x, y).0),
    );
    assert!(lands.1 - lands.0 > 20, "a gap for the door: {lands:?}");
    let between = |a: u32, b: u32, c: u32| a.min(c) + 2 < b && b + 2 < a.max(c);
    assert!(
        between(was.0, shown.0, lands.0) && between(was.1, shown.1, lands.1),
        "the poché's gap part-way: was {was:?}, shown {shown:?}, lands {lands:?}"
    );
}

/// Walls easing off the canvas and back on: the poché is drawn where their faces are shown,
/// not where it lands. Kept while the room slides out of view, and not shown ahead of the
/// room while it slides back in.
#[test]
fn smoke_openings_walls_easing_across_the_canvas_edge_keep_their_poche_with_them() {
    let Some(mut h) = easing_harness() else {
        return;
    };
    view(&mut h, DVec2::new(-300.0, -100.0), DVec2::new(150.0, 160.0));
    room(&mut h);
    h.state_mut().animate_solves = true;
    // (The pointer away from the walls: no poché lit by hover.)
    h.hover_at(screen(&h, DVec2::new(-250.0, 140.0)));
    h.run();
    let right = {
        let app = h.state();
        let rect = app.drawing.view.canvas_rect;
        ScreenMap {
            rect,
            camera: app.drawing.view.camera,
        }
        .to_world(rect.right_center())
        .x
    };
    // The left wall's middle, off its dashed centreline.
    let at = screen(&h, DVec2::new(-3.0, 30.0));
    let pixel = |img: &dyn Fn(u32, u32) -> [u8; 4]| img(at.x as u32, at.y as u32);
    let on = h.render().expect("render");
    let poche = pixel(&|x, y| on.get_pixel(x, y).0);
    // The room moved past the canvas's right edge (its left face 10 beyond it).
    let started = h.state().solve_motion.started;
    {
        let ed = &mut h.state_mut().drawing.editor;
        let (_, wall) = ed.doc.walls().next().expect("a wall");
        let centre = ed
            .doc
            .path(wall.centre)
            .expect("its centreline")
            .curves
            .clone();
        ed.selection = centre
            .iter()
            .flat_map(|c| ed.doc.get(*c).unwrap().kind.points().collect::<Vec<_>>())
            .collect();
        assert_eq!(ed.nudge(DVec2::new(right + 15.0, 0.0)), Some(true));
    }
    h.step();
    assert_eq!(h.state().solve_motion.started, started + 1, "it eases");
    let leaving = h.render().expect("render");
    assert_eq!(
        pixel(&|x, y| leaving.get_pixel(x, y).0),
        poche,
        "the poché goes with its faces as they leave the canvas"
    );
    h.run();
    assert!(!h.state().solve_motion.animating());
    let off = h.render().expect("render");
    let bare = pixel(&|x, y| off.get_pixel(x, y).0);
    assert_ne!(bare, poche, "the room is out of view");
    // Undone: it slides back in from out of view.
    h.state_mut().drawing.editor.undo();
    h.step();
    assert_eq!(h.state().solve_motion.started, started + 2, "it eases back");
    let arriving = h.render().expect("render");
    assert_eq!(
        pixel(&|x, y| arriving.get_pixel(x, y).0),
        bare,
        "no poché where the room lands before its faces get there"
    );
}
