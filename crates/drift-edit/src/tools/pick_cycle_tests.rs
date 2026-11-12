//! Click again to pick the next (PAPERCUTS 2026-11-11): a plain click on the spot of the last
//! one picks the next of what's under the pointer there, round and round.

use super::*;

const TOL: f64 = 1.0;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Three lines crossing at the origin, their ends and middles well away from it: across
/// (`a`), up (`b`) and on the diagonal (`c`).
fn three_stacked() -> (Editor, [EntityId; 3]) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let lines = [
        (v(-10.0, 0.0), v(30.0, 0.0)),
        (v(0.0, -10.0), v(0.0, 30.0)),
        (v(-10.0, -10.0), v(30.0, 30.0)),
    ]
    .map(|(from, to)| {
        let a = ed.doc.add_point(from);
        let b = ed.doc.add_point(to);
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

fn click(ed: &mut Editor, sel: &mut SelectTool, at: DVec2) {
    click_with(ed, sel, at, Modifiers::default());
}

fn picked(ed: &Editor) -> Vec<EntityId> {
    ed.selection.iter().copied().collect()
}

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

/// Nearest first: the diagonal (0.035 away), across (0.05), up (0.1).
const SPOT: DVec2 = DVec2::new(0.1, 0.05);

#[test]
fn clicking_again_picks_the_next_of_three_stacked_and_wraps_round() {
    let (mut ed, [a, b, c]) = three_stacked();
    let mut sel = SelectTool::default();
    // The first click picks what a single click always has: the nearest.
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
    assert_eq!(
        ed.out.info.take().as_deref(),
        Some("Picked 1 of 3 here (click again for the next)")
    );
    // Again, a pixel or so off: the next nearest, then the last.
    click(&mut ed, &mut sel, SPOT + v(0.2, 0.1));
    assert_eq!(picked(&ed), [a]);
    assert_eq!(
        ed.out.info.take().as_deref(),
        Some("Picked 2 of 3 here (click again for the next)")
    );
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [b]);
    assert_eq!(
        ed.out.info.take().as_deref(),
        Some("Picked 3 of 3 here (click again for the next)")
    );
    // And round to the first.
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
    assert_eq!(ed.active(), Some(c));
}

#[test]
fn moving_away_clicking_elsewhere_esc_or_shift_start_the_cycle_again() {
    let (mut ed, [a, _, c]) = three_stacked();
    let mut sel = SelectTool::default();
    click(&mut ed, &mut sel, SPOT);
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [a]);
    // The pointer went away and came back: the first again.
    sel.handle(Input::Hover(Pointer::new(v(5.0, 5.0), TOL)), &mut ed);
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
    // A click somewhere else in between (on the diagonal alone).
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [a]);
    click(&mut ed, &mut sel, v(20.0, 20.0));
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
    // Esc.
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [a]);
    sel.cancel(&mut ed);
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
    // Shift-click adds the nearest, as ever, and the next plain click starts again.
    click_with(&mut ed, &mut sel, SPOT, SHIFT);
    assert_eq!(picked(&ed), [c]);
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [c]);
}

#[test]
fn shift_and_ctrl_clicks_add_and_toggle_without_cycling() {
    let (mut ed, [_, _, c]) = three_stacked();
    let mut sel = SelectTool::default();
    click_with(&mut ed, &mut sel, SPOT, SHIFT);
    click_with(&mut ed, &mut sel, SPOT, SHIFT);
    assert_eq!(picked(&ed), [c]);
    click_with(&mut ed, &mut sel, SPOT, CTRL);
    assert!(ed.selection.is_empty());
    click_with(&mut ed, &mut sel, SPOT, CTRL);
    assert_eq!(picked(&ed), [c]);
}

#[test]
fn one_thing_under_the_pointer_picks_as_before() {
    let (mut ed, [_, _, c]) = three_stacked();
    let mut sel = SelectTool::default();
    for _ in 0..3 {
        click(&mut ed, &mut sel, v(20.0, 20.0));
        assert_eq!(picked(&ed), [c]);
        // (Nothing to say with one.)
        assert_eq!(ed.out.info.take(), None);
    }
}

#[test]
fn the_cycle_goes_on_to_the_fill_under_an_edge() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let p = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)].map(|q| ed.doc.add_point(q));
    let lines: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.commit("Rectangle");
    let fill = ed.add_fill_at(v(10.0, 5.0)).unwrap();
    ed.selection.clear();
    ed.selected_fills.clear();
    let mut sel = SelectTool::default();
    // Just inside the bottom edge: the edge, then the fill (with its outline).
    click(&mut ed, &mut sel, v(5.0, 0.3));
    assert_eq!(picked(&ed), [lines[0]]);
    assert!(ed.selected_fills.is_empty());
    click(&mut ed, &mut sel, v(5.0, 0.3));
    assert_eq!(ed.selected_fills, [fill].into());
    click(&mut ed, &mut sel, v(5.0, 0.3));
    assert_eq!(picked(&ed), [lines[0]]);
    assert!(ed.selected_fills.is_empty());
}

#[test]
fn a_double_click_on_a_grouped_line_with_another_over_it_still_opens_the_group() {
    let (mut ed, [a, b, c]) = three_stacked();
    // The diagonal and the upright in a group.
    ed.selection = [b, c].into();
    let g = ed.make_group().expect("grouped");
    ed.selection.clear();
    let mut sel = SelectTool::default();
    // A double-click comes as a click, then a click and a double-click together: the
    // second click goes on to the next (the line across), and the double-click goes back.
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(ed.selection, [b, c].into());
    click(&mut ed, &mut sel, SPOT);
    assert_eq!(picked(&ed), [a]);
    sel.handle(Input::DoubleClick(Pointer::new(SPOT, TOL)), &mut ed);
    assert_eq!(ed.open_group, Some(g));
    assert_eq!(picked(&ed), [c]);
}
