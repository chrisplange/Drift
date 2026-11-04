//! The active element the select tool picks (#333): the element picked last by a click.

use super::*;

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Three lines apart, level, at y = 0, 10 and 20.
fn three_lines() -> (Editor, [EntityId; 3]) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let lines = [0.0, 10.0, 20.0].map(|y| {
        let a = ed.doc.add_point(v(0.0, y));
        let b = ed.doc.add_point(v(20.0, y));
        ed.doc.add_line(a, b).unwrap()
    });
    ed.commit("Lines");
    (ed, lines)
}

fn click_with(ed: &mut Editor, sel: &mut SelectTool, at: DVec2, mods: Modifiers) {
    let p = Pointer::new(at, TOL);
    sel.handle(Input::Hover(p), ed);
    sel.handle(Input::Click(p, mods), ed);
}

const PLAIN: Modifiers = Modifiers {
    shift: false,
    command: false,
    alt: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    command: false,
    alt: false,
};
const CTRL: Modifiers = Modifiers {
    shift: false,
    command: true,
    alt: false,
};

#[test]
fn the_element_picked_last_is_active() {
    let (mut ed, [a, b, c]) = three_lines();
    let mut sel = SelectTool::default();
    click_with(&mut ed, &mut sel, v(5.0, 0.0), PLAIN);
    assert_eq!(ed.active(), Some(a));
    click_with(&mut ed, &mut sel, v(5.0, 10.0), SHIFT);
    assert_eq!((ed.selection.len(), ed.active()), (2, Some(b)));
    click_with(&mut ed, &mut sel, v(5.0, 20.0), CTRL);
    assert_eq!((ed.selection.len(), ed.active()), (3, Some(c)));
    // Shift-click of one already picked makes it the active one, and picks nothing new.
    click_with(&mut ed, &mut sel, v(5.0, 0.0), SHIFT);
    assert_eq!((ed.selection.len(), ed.active()), (3, Some(a)));
    // Ctrl-click takes it away: none is active.
    click_with(&mut ed, &mut sel, v(5.0, 0.0), CTRL);
    assert_eq!((ed.selection.len(), ed.active()), (2, None));
    // A plain click on empty space: nothing picked, none active.
    click_with(&mut ed, &mut sel, v(50.0, 50.0), PLAIN);
    assert_eq!((ed.selection.len(), ed.active()), (0, None));
}

#[test]
fn a_box_picks_no_active_element() {
    let (mut ed, [_, b, _]) = three_lines();
    let mut sel = SelectTool::default();
    click_with(&mut ed, &mut sel, v(5.0, 10.0), PLAIN);
    assert_eq!(ed.active(), Some(b));
    // Shift and a box round all three: added, and none active.
    let from = Pointer::new(v(-1.0, -1.0), TOL);
    let to = Pointer::new(v(21.0, 21.0), TOL);
    sel.handle(Input::DragStart(from, SHIFT), &mut ed);
    sel.handle(Input::Drag(to), &mut ed);
    sel.handle(Input::DragEnd(to, SHIFT), &mut ed);
    assert_eq!((ed.selection.len(), ed.active()), (3, None));
}

#[test]
fn a_whole_group_picked_has_no_active_element() {
    let (mut ed, [a, b, c]) = three_lines();
    ed.selection = [a, b].into();
    ed.make_group().expect("grouped");
    ed.selection.clear();
    let mut sel = SelectTool::default();
    click_with(&mut ed, &mut sel, v(5.0, 0.0), PLAIN);
    assert_eq!((ed.selection.len(), ed.active()), (2, None));
    click_with(&mut ed, &mut sel, v(5.0, 20.0), SHIFT);
    assert_eq!(ed.active(), Some(c));
}

#[test]
fn the_active_element_goes_with_its_entity() {
    let (mut ed, [a, b, _]) = three_lines();
    let mut sel = SelectTool::default();
    click_with(&mut ed, &mut sel, v(5.0, 0.0), PLAIN);
    click_with(&mut ed, &mut sel, v(5.0, 10.0), SHIFT);
    assert_eq!(ed.active(), Some(b));
    // Undoing the lines takes the active element with them.
    assert!(ed.undo());
    assert!(!ed.doc.contains(b));
    assert_eq!(ed.active(), None);
    assert!(ed.redo());
    assert_eq!(ed.active(), None, "a redo doesn't pick");
    // Picked again, then deselected by code: no longer active.
    ed.selection = [a, b].into();
    ed.set_active(Some(b));
    ed.selection.remove(&b);
    assert_eq!(ed.active(), None);
}
