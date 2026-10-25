//! Groups and blocks: two filled shapes grouped, picked by a click in a fill, entered with a
//! double-click, moved with G, made a block and exploded back to a group.

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
    assert_eq!(h.state().editor.selection.len(), 8, "both shapes picked");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::G);
    h.run();
    let g = h.state().editor.selected_group().expect("grouped");
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
    let members = h.state().editor.doc.group(g).unwrap().members.clone();
    click_world(&mut h, DVec2::new(15.0, 15.0));
    {
        let ed = &h.state().editor;
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
        let ed = &mut h.state_mut().editor;
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
    let ed = &h.state().editor;
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
        let ed = &h.state().editor;
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
    let ed = &h.state().editor;
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
