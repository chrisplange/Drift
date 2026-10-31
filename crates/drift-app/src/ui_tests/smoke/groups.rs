//! Groups and blocks: two filled shapes grouped, picked by a click in a fill, entered with a
//! double-click, moved with G, made a block and exploded back to a group.

use std::collections::BTreeSet;

use super::*;

/// Two solid-filled rectangles, picked (a click in one, Shift-click in the other) and grouped
/// with Ctrl+G. Returns their sides and fills, and the group.
fn grouped(
    h: &mut Harness<'_, DriftApp>,
) -> (
    Vec<EntityId>,
    Vec<EntityId>,
    [FillId; 2],
    drift_core::GroupId,
) {
    view(h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 50.0));
    let (left, a) = rectangle(h, DVec2::ZERO, DVec2::new(30.0, 30.0), Fill::Solid);
    let (right, b) = rectangle(
        h,
        DVec2::new(50.0, 0.0),
        DVec2::new(80.0, 30.0),
        Fill::Solid,
    );
    click_world(h, DVec2::new(15.0, 15.0));
    let other = screen(h, DVec2::new(65.0, 15.0));
    click_with(h, other, EguiModifiers::SHIFT);
    assert_eq!(
        h.state().drawing.editor.selection.len(),
        8,
        "both shapes picked"
    );
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::G);
    h.run();
    let g = h.state().drawing.editor.selected_group().expect("grouped");
    h.key_press(Key::Escape);
    h.run();
    (left, right, [a.unwrap(), b.unwrap()], g)
}

/// #159: a click in a grouped shape's fill picks the whole group, both fills with it; G moves
/// it all, fills included; a double-click enters the group with just that shape picked.
#[test]
fn smoke_groups_fill_click_picks_the_group_and_g_moves_it() {
    let Some(mut h) = harness() else { return };
    let (left, _, fills, g) = grouped(&mut h);
    let members = h
        .state()
        .drawing
        .editor
        .doc
        .group(g)
        .unwrap()
        .members
        .clone();
    click_world(&mut h, DVec2::new(15.0, 15.0));
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selection, members, "the group");
        assert_eq!(ed.selected_fills, fills.into(), "with both fills");
    }
    // G, X, 40, Enter: the group and its fills move 40 along x.
    let corner = ends(&h, left[0]).0;
    let before = at(&h, corner);
    h.hover_at(screen(&h, DVec2::new(15.0, 15.0)));
    h.run();
    h.key_press(Key::G);
    h.run();
    h.hover_at(screen(&h, DVec2::new(20.0, 15.0)));
    h.run();
    h.key_press(Key::X);
    h.run();
    type_text(&mut h, "40");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(at(&h, corner), before + DVec2::new(40.0, 0.0));
    {
        let ed = &mut h.state_mut().drawing.editor;
        let shapes = ed.fill_shapes();
        for f in fills {
            assert!(!shapes[&f].lost, "the fills went along");
            assert!(ed.doc.fill(f).unwrap().seed.x > 40.0);
        }
    }
    // A double-click inside a shape enters the group with that shape picked.
    h.key_press(Key::Escape);
    h.run();
    let inside = screen(&h, DVec2::new(55.0, 15.0));
    double_click(&mut h, inside);
    let ed = &h.state().drawing.editor;
    assert_eq!(ed.open_group, Some(g), "inside the group");
    assert_eq!(ed.selection, left.iter().copied().collect());
    assert_eq!(ed.selected_fills, [fills[0]].into());
}

#[test]
fn smoke_groups_make_a_block_and_explode_it() {
    let Some(mut h) = harness() else { return };
    let (_, _, _, g) = grouped(&mut h);
    click_world(&mut h, DVec2::new(15.0, 15.0));
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.get_by_label("Make block").click();
    h.run();
    h.run();
    let block = {
        let ed = &h.state().drawing.editor;
        assert!(ed.doc.group(g).is_none(), "the group became a block");
        let copy = *ed.selected_instances.first().expect("its copy, picked");
        let block = ed.doc.instance(copy).unwrap().block;
        assert_eq!(
            ed.doc.block(block).unwrap().fills.len(),
            2,
            "with its fills"
        );
        block
    };
    save_png("drift_smoke_block.png", &mut h);
    h.get_by_label("Explode to group").click();
    h.run();
    h.run();
    let ed = &h.state().drawing.editor;
    assert_eq!(ed.doc.instances().count(), 0);
    let (_, group) = ed.doc.groups().next().expect("a group again");
    assert_eq!(group.members.len(), 8);
    assert_eq!(
        ed.doc.fills().filter(|(_, f)| f.path.is_some()).count(),
        2,
        "both shapes filled again"
    );
    assert!(ed.doc.block(block).is_some(), "the block is kept");
}

/// #232: a block copy being moved doesn't snap to itself. The copy is picked and moved with G
/// from a point on its own box (the middle of its left side, a snap point), in small steps:
/// each step lands on where that snap point has moved to, which must not pull the copy.
#[test]
fn smoke_groups_a_moving_block_copy_does_not_snap_to_itself() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(80.0, -10.0), DVec2::new(160.0, 40.0));
    let (sides, _) = rectangle(
        &mut h,
        DVec2::new(100.0, 0.0),
        DVec2::new(140.0, 30.0),
        Fill::No,
    );
    let c = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = sides.iter().copied().collect();
        ed.make_block("Box").unwrap()
    };
    click_world(&mut h, DVec2::new(110.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_instances, [c].into());
    let from = h.state().drawing.editor.doc.instance(c).unwrap().at;
    let grip = DVec2::new(100.0, 15.0);
    let start = screen(&h, grip);
    h.hover_at(start);
    h.run();
    h.key_press(Key::G);
    h.run();
    for k in 1..=12 {
        let step = DVec2::new(0.25, 0.04) * f64::from(k);
        let at = screen(&h, grip + step);
        h.hover_at(at);
        h.run();
        let to = h.state().drawing.editor.doc.instance(c).unwrap().at;
        assert!(
            to.distance(from + step) < 0.05,
            "step {k}: moved with the pointer, not onto its own snap points: {from:?} → {to:?}"
        );
    }
    h.key_press(Key::Enter);
    h.run();
}

/// Every group, as (name, members).
fn groups_now(h: &Harness<'_, DriftApp>) -> Vec<(String, BTreeSet<EntityId>)> {
    h.state()
        .drawing
        .editor
        .doc
        .groups()
        .map(|(_, g)| (g.name.clone(), g.members.clone()))
        .collect()
}

/// After a copy of the group made by `g`: a second group named "Group 2" holding exactly the
/// copies (which are picked), the original untouched; `undos` undo steps take it all back
/// (the copy is one, a move that placed it another).
fn copied_as_a_group(
    h: &mut Harness<'_, DriftApp>,
    before: &[(String, BTreeSet<EntityId>)],
    entities: usize,
    undos: usize,
) {
    let now = groups_now(h);
    assert_eq!(now.len(), 2, "a new group: {now:?}");
    assert_eq!(now[0], before[0], "the original is untouched");
    let (name, members) = &now[1];
    assert_eq!(name, "Group 2");
    assert_eq!(members.len(), before[0].1.len());
    assert!(members.is_disjoint(&before[0].1));
    let ed = &h.state().drawing.editor;
    assert_eq!(&ed.selection, members, "the copy is picked");
    assert!(ed.selected_group().is_some());
    for _ in 0..undos {
        undo(h);
    }
    assert_eq!(groups_now(h), before);
    assert_eq!(h.state().drawing.editor.doc.len(), entities);
}

/// #227: Shift+D on a group gives a new group of the copies.
#[test]
fn smoke_groups_duplicating_a_group_makes_a_group() {
    let Some(mut h) = harness() else { return };
    grouped(&mut h);
    let (before, entities) = (groups_now(&h), h.state().drawing.editor.doc.len());
    click_world(&mut h, DVec2::new(15.0, 15.0));
    h.hover_at(screen(&h, DVec2::new(15.0, 15.0)));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    click_world(&mut h, DVec2::new(15.0, 40.0));
    copied_as_a_group(&mut h, &before, entities, 2);
}

/// #227: Copy and Paste (and Enter) of a group gives a new group of the copies.
#[test]
fn smoke_groups_copy_and_paste_a_group_makes_a_group() {
    let Some(mut h) = harness() else { return };
    grouped(&mut h);
    let (before, entities) = (groups_now(&h), h.state().drawing.editor.doc.len());
    click_world(&mut h, DVec2::new(15.0, 15.0));
    h.event(Event::Copy);
    h.run();
    let text = h.state().drawing.editor.copy_selection().unwrap();
    h.hover_at(screen(&h, DVec2::new(40.0, 40.0)));
    h.run();
    h.event(Event::Paste(text));
    h.run();
    // (Enter leaves the copy where it landed; Esc would remove it again, #264.)
    h.key_press(Key::Enter);
    h.run();
    copied_as_a_group(&mut h, &before, entities, 1);
}

/// #227: Mirror of a group gives a new group of the mirror images.
#[test]
fn smoke_groups_mirroring_a_group_makes_a_group() {
    let Some(mut h) = harness() else { return };
    grouped(&mut h);
    let axis = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(95.0, -5.0));
        let b = ed.doc.add_point(DVec2::new(95.0, 45.0));
        let axis = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        axis
    };
    let (before, entities) = (groups_now(&h), h.state().drawing.editor.doc.len());
    click_world(&mut h, DVec2::new(15.0, 15.0));
    palette(&mut h, "Mirror");
    assert_eq!(h.state().tool.kind(), ToolKind::Mirror);
    let on_axis = h
        .state()
        .drawing
        .editor
        .doc
        .shape(axis)
        .unwrap()
        .bounds()
        .center();
    click_world(&mut h, on_axis);
    copied_as_a_group(&mut h, &before, entities, 1);
}

/// An asymmetric shape (a box with one diagonal) drawn at 100..140 × 0..30 and made a block
/// named "Flag"; its one copy, picked.
fn flag_block(h: &mut Harness<'_, DriftApp>) -> drift_core::InstanceId {
    view(h, DVec2::new(80.0, -20.0), DVec2::new(200.0, 50.0));
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let c = [
        DVec2::new(100.0, 0.0),
        DVec2::new(140.0, 0.0),
        DVec2::new(140.0, 30.0),
        DVec2::new(100.0, 30.0),
    ]
    .map(|p| ed.doc.add_point(p));
    let mut sides: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap())
        .collect();
    sides.push(ed.doc.add_line(c[0], c[2]).unwrap());
    ed.commit("Flag");
    ed.selection = sides.into_iter().collect();
    let copy = ed.make_block("Flag").unwrap();
    h.run();
    copy
}

fn copies(h: &Harness<'_, DriftApp>) -> Vec<(drift_core::InstanceId, drift_core::Instance)> {
    h.state()
        .drawing
        .editor
        .doc
        .instances()
        .map(|(id, i)| (id, *i))
        .collect()
}

/// #260: Mirror with a block copy picked mirrors the copy: a new copy on the far side of the
/// line, mirrored, whose points are the mirror images of the original's.
#[test]
fn smoke_groups_mirroring_a_block_copy_makes_a_mirrored_copy() {
    let Some(mut h) = harness() else { return };
    let copy = flag_block(&mut h);
    let axis = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(160.0, -10.0));
        let b = ed.doc.add_point(DVec2::new(160.0, 40.0));
        let axis = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        axis
    };
    click_world(&mut h, DVec2::new(110.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_instances, [copy].into());
    let original = *h.state().drawing.editor.doc.instance(copy).unwrap();
    palette(&mut h, "Mirror");
    assert_eq!(h.state().tool.kind(), ToolKind::Mirror);
    click_world(&mut h, DVec2::new(160.0, 15.0));
    let _ = axis;
    let now = copies(&h);
    assert_eq!(now.len(), 2, "a mirrored copy was placed: {now:?}");
    let (id, image) = now.into_iter().find(|(id, _)| *id != copy).unwrap();
    let reflect = |p: DVec2| DVec2::new(320.0 - p.x, p.y);
    assert!(image.at.distance(reflect(original.at)) < 1e-9, "{image:?}");
    assert!(image.mirror && !original.mirror);
    assert!(
        image.angle.abs() < 1e-9,
        "a vertical axis keeps the turn: {image:?}"
    );
    // The box's corner (a snap point) lands on its mirror image.
    let corner = original.invert(DVec2::new(100.0, 0.0));
    assert!(image.apply(corner).distance(DVec2::new(220.0, 0.0)) < 1e-9);
    assert_eq!(
        h.state().drawing.editor.selected_instances,
        [id].into(),
        "the new copy is picked, as mirrored curves are"
    );
    save_png("drift_smoke_block_mirrored.png", &mut h);
    undo(&mut h);
    assert_eq!(copies(&h).len(), 1);
}

/// #264: Shift+D then Esc leaves no copy stacked on the original (the copy is removed, Redo
/// brings it back); the same for Ctrl+V then Esc.
#[test]
fn smoke_groups_shift_d_then_esc_leaves_no_stacked_copy() {
    let Some(mut h) = harness() else { return };
    let copy = flag_block(&mut h);
    click_world(&mut h, DVec2::new(110.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_instances, [copy].into());
    h.hover_at(screen(&h, DVec2::new(110.0, 10.0)));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    assert_eq!(copies(&h).len(), 2, "duplicated, and being moved");
    assert!(h.state().tool.is_busy());
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(copies(&h).len(), 1, "Esc removed the unmoved copy");
    assert_eq!(h.state().drawing.editor.selected_instances, [copy].into());
    assert!(status(&h).contains("Copy removed"), "{}", status(&h));
    h.key_press_modifiers(EguiModifiers::COMMAND | EguiModifiers::SHIFT, Key::Z);
    h.run();
    assert_eq!(copies(&h).len(), 2, "Redo brings the copy back");
    undo(&mut h);
    assert_eq!(copies(&h).len(), 1);
    // Paste under the pointer, then Esc: nothing left behind either.
    click_world(&mut h, DVec2::new(110.0, 10.0));
    h.event(Event::Copy);
    h.run();
    let text = h.state().drawing.editor.copy_selection().unwrap();
    h.hover_at(screen(&h, DVec2::new(170.0, 10.0)));
    h.run();
    h.event(Event::Paste(text));
    h.run();
    assert_eq!(copies(&h).len(), 2, "pasted, and being moved");
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(copies(&h).len(), 1, "Esc removed the pasted copy");
    // Shift+D and a click still place a copy, in two undo steps (Duplicate, Move).
    click_world(&mut h, DVec2::new(110.0, 10.0));
    h.hover_at(screen(&h, DVec2::new(110.0, 10.0)));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    click_world(&mut h, DVec2::new(170.0, 10.0));
    assert_eq!(copies(&h).len(), 2);
    undo(&mut h);
    undo(&mut h);
    assert_eq!(copies(&h).len(), 1);
}

/// #264: two copies at the same place and transform count as overlapping: the status bar's
/// pill says so, Select overlapping picks the later one, Delete removes it.
#[test]
fn smoke_groups_stacked_copies_show_as_overlapping() {
    let Some(mut h) = harness() else { return };
    let copy = flag_block(&mut h);
    let second = {
        let ed = &mut h.state_mut().drawing.editor;
        let i = *ed.doc.instance(copy).unwrap();
        ed.place_instance(i.block, i.at, i.angle, i.mirror);
        *ed.selected_instances.iter().next().unwrap()
    };
    h.key_press(Key::Escape);
    h.run();
    h.run();
    h.get_by_label("1 overlapping");
    palette(&mut h, "Select overlapping");
    assert_eq!(h.state().drawing.editor.selected_instances, [second].into());
    assert!(h.state().drawing.editor.selection.is_empty());
    h.key_press(Key::Delete);
    h.run();
    let left = copies(&h);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, copy, "the earlier copy stays");
    assert!(h.state().drawing.editor.overlapping_instances().is_empty());
}
