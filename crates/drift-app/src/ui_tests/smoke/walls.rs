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

/// #202: a room's floor filled inside its walls is an area fill, never a shape made of the
/// walls' faces, and says why.
#[test]
fn smoke_walls_a_rooms_floor_fill_is_an_area_fill() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    let paths = h.state().editor.doc.paths().count();
    let fills = h.state().editor.doc.fills().count();
    h.key_press(Key::H);
    h.run();
    click_world(&mut h, DVec2::new(50.0, 30.0));
    h.key_press(Key::Escape);
    h.run();
    let ed = &h.state().editor;
    assert_eq!(ed.doc.fills().count(), fills + 1, "the floor filled");
    assert_eq!(ed.doc.paths().count(), paths, "no shape made of wall faces");
    let floor = ed
        .doc
        .fills()
        .map(|(id, _)| id)
        .find(|f| !ed.wall_owners().fills.contains_key(f))
        .expect("the floor");
    assert!(ed.doc.fill(floor).unwrap().path.is_none());
    assert_eq!(
        ed.fill_kind(floor),
        Err(drift_edit::fills::NotAShape::Walls)
    );
}

/// The canvas's index of shapes holds every curve where it is: what is drawn and picked.
fn index_follows(h: &Harness<'_, DriftApp>, when: &str) {
    let ed = &h.state().editor;
    for (id, _) in ed.doc.iter() {
        assert_eq!(
            ed.index.shape(id),
            ed.doc.shape(id).as_ref(),
            "{id:?} drawn where it was, {when}"
        );
    }
}

/// #249: a wall picked by its poché or a face and moved (G, or a drag) takes its faces
/// along, drawn and picked where they are, while it moves and after.
#[test]
fn smoke_walls_moved_by_poche_or_face_take_their_faces() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 120.0));
    room(&mut h);
    let before = positions(&h);
    let moved_by = |h: &Harness<'_, DriftApp>, d: DVec2, tol: f64, when: &str| {
        for ((id, a), (_, b)) in before.iter().zip(positions(h)) {
            assert!(
                (b - (*a + d)).length() < tol,
                "{id:?} {a:?} → {b:?}, {when}"
            );
        }
    };
    for (by, at) in [
        ("poché", DVec2::new(50.0, 1.0)),
        ("face", DVec2::new(50.0, -5.0)),
    ] {
        click_world(&mut h, at);
        assert_eq!(
            h.state().editor.selected_walls().len(),
            1,
            "picked by its {by}"
        );
        h.hover_at(screen(&h, at));
        h.run_steps(2);
        h.key_press(Key::G);
        h.run_steps(2);
        h.hover_at(screen(&h, at + DVec2::new(0.0, 10.0)));
        h.run_steps(2);
        h.key_press(Key::Y);
        h.run_steps(2);
        type_text(&mut h, "30");
        h.run_steps(2);
        index_follows(&h, &format!("moving, picked by its {by}"));
        h.key_press(Key::Enter);
        h.run_steps(2);
        moved_by(&h, DVec2::new(0.0, 30.0), 1e-9, by);
        index_follows(&h, &format!("moved, picked by its {by}"));
        h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
        h.run_steps(2);
        moved_by(&h, DVec2::ZERO, 1e-9, "undone");
        index_follows(&h, "undone");
        h.key_press(Key::Escape);
        h.run_steps(2);
    }
    // Picked by its poché, then dragged from there.
    click_world(&mut h, DVec2::new(50.0, 1.0));
    let (from, to) = (
        screen(&h, DVec2::new(50.0, 1.0)),
        screen(&h, DVec2::new(50.0, 31.0)),
    );
    h.hover_at(from);
    h.run_steps(2);
    button(&h, from, true);
    h.run_steps(2);
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run_steps(2);
    }
    index_follows(&h, "dragging");
    button(&h, to, false);
    h.run_steps(2);
    assert_eq!(h.state().editor.selected_walls().len(), 1, "still picked");
    // (Its base point snaps on the way, so the move is the first point's, about 30 up.)
    let d = positions(&h)[0].1 - before[0].1;
    assert!((d.y - 30.0).abs() < 1.0, "{d:?}");
    moved_by(&h, d, 1e-6, "dragged");
    index_follows(&h, "dragged");
}

/// #250: a pasted room of walls follows the pointer whole (centreline, faces and poché),
/// and lands whole.
#[test]
fn smoke_walls_pasted_room_moves_whole() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(260.0, 160.0));
    room(&mut h);
    click_world(&mut h, DVec2::new(50.0, 1.0));
    h.event(Event::Copy);
    h.run_steps(2);
    let text = h.state().editor.copy_selection().unwrap();
    let first = screen(&h, DVec2::new(150.0, 40.0));
    h.hover_at(first);
    h.run_steps(2);
    h.event(Event::Paste(text));
    h.run_steps(2);
    assert_eq!(h.state().editor.doc.walls().count(), 2, "pasted");
    for k in 1..=4 {
        h.hover_at(first + egui::vec2(10.0 * k as f32, 6.0 * k as f32));
        h.run_steps(2);
        index_follows(&h, "following the pointer");
    }
    let drop = first + egui::vec2(40.0, 24.0);
    button(&h, drop, true);
    h.run_steps(2);
    button(&h, drop, false);
    h.run_steps(2);
    index_follows(&h, "dropped");
    let ed = &mut h.state_mut().editor;
    let shapes = ed.fill_shapes();
    assert!(shapes.values().all(|s| !s.lost), "both pochés found");
    let ring = 110.0 * 70.0 - 90.0 * 50.0;
    let area: f64 = shapes.values().map(|s| s.region.area).sum();
    assert!((area - 2.0 * ring).abs() < 0.5, "{area}");
}

/// A room of walls 20 thick, justified `justify`, round a centreline from (0, 0) along
/// to (100, 0), up to (100, 60), over a half-round bay (radius 50) to (0, 60) and down, with
/// its floor filled (an area fill inside the walls' faces). Returns the floor.
fn bay_room(h: &mut Harness<'_, DriftApp>, justify: drift_core::Justify) -> FillId {
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let [a, b, c, d, o] = [
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 60.0),
            (0.0, 60.0),
            (50.0, 60.0),
        ]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
        let curves = vec![
            ed.doc.add_line(a, b).unwrap(),
            ed.doc.add_line(b, c).unwrap(),
            ed.doc.add_arc(o, c, d).unwrap(),
            ed.doc.add_line(d, a).unwrap(),
        ];
        for c in &curves {
            ed.doc.set_style(*c, drift_core::LineKind::Construction);
        }
        let path = ed
            .doc
            .add_path(drift_core::Path {
                curves,
                closed: true,
            })
            .unwrap();
        let mut w = drift_core::Wall::new(path, 20.0, ed.doc.current_layer());
        w.justify = justify;
        ed.doc.add_wall(w).unwrap();
        assert!(ed.commit("Wall"));
    }
    h.run_steps(2);
    let fills = h.state().editor.doc.fills().count();
    h.key_press(Key::H);
    h.run_steps(2);
    click_world(h, DVec2::new(50.0, 30.0));
    h.key_press(Key::Escape);
    h.run_steps(2);
    let ed = &h.state().editor;
    assert_eq!(ed.doc.fills().count(), fills + 1, "the floor filled");
    ed.doc
        .fills()
        .map(|(id, _)| id)
        .find(|f| !ed.wall_owners().fills.contains_key(f))
        .expect("the floor")
}

/// The radii of the arcs walls made, sorted.
fn face_radii(h: &Harness<'_, DriftApp>, w: drift_core::WallId) -> Vec<f64> {
    let doc = &h.state().editor.doc;
    let mut r: Vec<f64> = doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .values()
        .filter_map(|c| match doc.shape(*c) {
            Some(drift_core::Shape::Arc { radius, .. }) => Some(radius),
            _ => None,
        })
        .collect();
    r.sort_by(f64::total_cmp);
    r
}

/// #248: Mirror of a room of walls with a floor fill gives the mirror image: a wall with
/// the same faces (arcs the same radii, whichever face it is justified to), one poché, and
/// the floor filled. Picked by its poché, or every curve picked (faces too: one poché, and no
/// loose copies of them).
#[test]
fn smoke_walls_mirror_copies_the_floor_and_the_faces() {
    for (justify, all) in [
        (drift_core::Justify::Centre, false),
        (drift_core::Justify::Left, false),
        (drift_core::Justify::Right, true),
    ] {
        let Some(mut h) = harness() else { return };
        // (Docks folded away: the Inspector growing for a wall would move the canvas.)
        h.state_mut().layout.collapsed = [true, true];
        view(&mut h, DVec2::new(-30.0, -30.0), DVec2::new(330.0, 150.0));
        let floor = bay_room(&mut h, justify);
        let (axis, original, curves) = {
            let ed = &mut h.state_mut().editor;
            let original = ed.doc.walls().next().unwrap().0;
            let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
            ed.doc.begin();
            let a = ed.doc.add_point(DVec2::new(150.0, -20.0));
            let b = ed.doc.add_point(DVec2::new(150.0, 130.0));
            let axis = ed.doc.add_line(a, b).unwrap();
            ed.commit("Line");
            (axis, original, curves)
        };
        let radii = face_radii(&h, original);
        assert_eq!(radii.len(), 2, "two faces round the bay");
        if all {
            // Every curve, the walls' faces too (as a script can pick them), but the axis.
            let ed = &mut h.state_mut().editor;
            ed.selection = ed
                .doc
                .iter()
                .filter(|(id, e)| !e.kind.is_point() && *id != axis)
                .map(|(id, _)| id)
                .collect();
        } else {
            // (In the poché, clear of the centreline and faces.)
            let y = match justify {
                drift_core::Justify::Centre => -5.0,
                drift_core::Justify::Left => -10.0,
                drift_core::Justify::Right => 10.0,
            };
            click_world(&mut h, DVec2::new(50.0, y));
        }
        assert_eq!(h.state().editor.selected_walls().len(), 1);
        palette(&mut h, "Mirror");
        assert_eq!(h.state().tool.kind(), ToolKind::Mirror);
        let on_axis = h.state().editor.doc.shape(axis).unwrap().bounds().center();
        click_world(&mut h, on_axis);
        h.key_press(Key::Escape);
        h.run_steps(2);
        let what = format!("{justify:?}, all picked: {all}");
        let ed = &mut h.state_mut().editor;
        let copy = ed
            .doc
            .walls()
            .map(|(id, _)| id)
            .find(|w| *w != original)
            .unwrap_or_else(|| panic!("a copy, {what}"));
        let owners = ed.wall_owners();
        assert_eq!(owners.fills.len(), 2, "one poché each, {what}");
        assert_eq!(ed.doc.fills().count(), 4, "pochés and floors, {what}");
        assert_eq!(
            ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count(),
            2 * curves + 1,
            "the walls' curves twice and the axis, nothing loose, {what}"
        );
        let shapes = ed.fill_shapes();
        let area = |f: drift_core::FillId| shapes[&f].region.area;
        let copied_floor = ed
            .doc
            .fills()
            .map(|(id, _)| id)
            .find(|f| *f != floor && !owners.fills.contains_key(f))
            .unwrap_or_else(|| panic!("the floor copied, {what}"));
        assert!(!shapes[&copied_floor].lost, "{what}");
        assert!(
            (area(copied_floor) - area(floor)).abs() < 1e-6,
            "{} vs {}, {what}",
            area(copied_floor),
            area(floor)
        );
        let seed = ed.doc.fill(copied_floor).unwrap().seed;
        assert!(seed.x > 150.0, "on the mirrored side, {what}");
        let mut pochés: Vec<f64> = owners.fills.keys().map(|f| area(*f)).collect();
        pochés.sort_by(f64::total_cmp);
        assert!((pochés[0] - pochés[1]).abs() < 1e-6, "{pochés:?}, {what}");
        drop(owners);
        let copied = face_radii(&h, copy);
        assert_eq!(copied.len(), radii.len(), "{what}");
        for (a, b) in radii.iter().zip(&copied) {
            assert!((a - b).abs() < 1e-9, "radii {radii:?} → {copied:?}, {what}");
        }
    }
}

/// The floors (fills that aren't pochés): each found, by area.
fn floors(h: &mut Harness<'_, DriftApp>) -> Vec<f64> {
    let ed = &mut h.state_mut().editor;
    let owners = ed.wall_owners();
    let shapes = ed.fill_shapes();
    let mut out: Vec<f64> = ed
        .doc
        .fills()
        .map(|(id, _)| id)
        .filter(|f| !owners.fills.contains_key(f))
        .map(|f| {
            assert!(!shapes[&f].lost, "{f:?} found");
            shapes[&f].region.area
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

/// #248: the other copies of a room of walls take its floor too: Copy and Paste, Shift+D
/// and moved, a linear pattern.
#[test]
fn smoke_walls_copies_take_the_floor() {
    for how in ["paste", "duplicate", "pattern"] {
        let Some(mut h) = harness() else { return };
        h.state_mut().layout.collapsed = [true, true];
        view(&mut h, DVec2::new(-30.0, -30.0), DVec2::new(430.0, 200.0));
        bay_room(&mut h, drift_core::Justify::Centre);
        let floor = floors(&mut h)[0];
        click_world(&mut h, DVec2::new(50.0, -5.0));
        assert_eq!(h.state().editor.selected_walls().len(), 1);
        match how {
            "paste" => {
                h.event(Event::Copy);
                h.run_steps(2);
                let text = h.state().editor.copy_selection().unwrap();
                h.hover_at(screen(&h, DVec2::new(250.0, 50.0)));
                h.run_steps(2);
                h.event(Event::Paste(text));
                h.run_steps(2);
                click_world(&mut h, DVec2::new(250.0, 50.0));
            }
            "duplicate" => {
                h.hover_at(screen(&h, DVec2::new(50.0, -5.0)));
                h.run_steps(2);
                h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
                h.run_steps(2);
                h.hover_at(screen(&h, DVec2::new(100.0, -5.0)));
                h.run_steps(2);
                h.key_press(Key::X);
                h.run_steps(2);
                type_text(&mut h, "200");
                h.key_press(Key::Enter);
                h.run_steps(2);
            }
            _ => {
                let ed = &mut h.state_mut().editor;
                let ids: Vec<_> = ed.selection.iter().copied().collect();
                ed.linear_pattern(&ids, 2, DVec2::new(200.0, 0.0)).unwrap();
                h.run_steps(2);
            }
        }
        assert_eq!(h.state().editor.doc.walls().count(), 2, "{how}");
        let found = floors(&mut h);
        assert_eq!(found.len(), 2, "the floor copied, {how}");
        assert!((found[1] - floor).abs() < 1e-6, "{found:?}, {how}");
        assert!((found[0] - floor).abs() < 1e-6, "{found:?}, {how}");
        index_follows(&h, how);
    }
}
