//! The three pies together: the snap pie's switches (flick, click, letter, and the
//! perpendicular & tangent switch they share), what a flick chooses (what is lit, and nothing
//! for a tap however far the window's edge kept the hub from the pointer), switching between
//! pies, and none on a sheet.

use drift_edit::editor::{PickFilter, PickKind};
use drift_geom::{SnapKind, SnapKinds};

use super::*;
use crate::app::pie_style::place_at;
use crate::commands::{Command, ViewFlag, spec};

const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;

fn kinds(h: &Harness<'_, DriftApp>) -> SnapKinds {
    h.state().drawing.editor.prefs.snap_kinds
}

fn status(h: &Harness<'_, DriftApp>) -> String {
    h.state()
        .status
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

/// The open pie's pill at `place`.
fn pill(h: &Harness<'_, DriftApp>, place: usize) -> egui::Rect {
    h.state().pie_pills[place]
}

/// A click at `p`.
fn click_at(h: &mut Harness<'_, DriftApp>, p: egui::Pos2) {
    h.hover_at(p);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

/// Over the canvas's middle, tap Shift+S: the snap pie, open to click.
fn tap_snap_pie(h: &mut Harness<'_, DriftApp>) {
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.key_press_modifiers(SHIFT, Key::S);
    h.run();
    assert!(h.state().snap_pie.as_ref().is_some_and(|p| !p.held));
}

#[test]
fn a_snap_flick_turns_one_kind_off_and_says_so() {
    let Some(mut h) = harness() else { return };
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let p = pill(&h, 1).center();
    h.hover_at(p);
    h.run();
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert!(h.state().snap_pie.is_none(), "a flick closes the ring");
    assert_eq!(kinds(&h), SnapKinds::ALL.with(SnapKind::Midpoint, false));
    assert_eq!(status(&h), "Midpoint snapping off");
    // Grid is the grid snap switch.
    let grid = h.state().grid_snap;
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let p = pill(&h, 2).center();
    h.hover_at(p);
    h.run();
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert_eq!(h.state().grid_snap, !grid);
}

/// Let go over the far end of a wide pill, which lies towards the next place round: the pill
/// that is lit (the one under the pointer) is the one chosen.
#[test]
fn a_flick_chooses_the_pill_under_the_pointer_not_the_next_place() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Edges));
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::Q);
    h.run();
    let center = h.state().select_pie.as_ref().expect("open").center;
    // All, at the top: its inner corner on the right.
    let all = pill(&h, 2);
    let corner = egui::pos2(all.right() - 2.0, all.bottom() - 2.0);
    assert_ne!(
        place_at(center, corner),
        Some(2),
        "the corner lies towards another place"
    );
    h.hover_at(corner);
    h.run();
    h.key_up_modifiers(SHIFT, Key::Q);
    h.run();
    assert!(h.state().select_pie.is_none());
    assert!(
        h.state().drawing.editor.prefs.pick_filter.is_all(),
        "All, the pill that was lit"
    );
}

/// Key down and up on separate frames near the window's bottom, where the hub opens well above
/// the pointer: a tap, which leaves the ring open and turns nothing on or off.
#[test]
fn a_tap_near_the_edge_leaves_the_ring_open() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let at = egui::pos2(canvas.center().x, canvas.bottom() - 20.0);
    h.hover_at(at);
    h.run();
    let before = (kinds(&h), h.state().grid_snap, h.state().perp_tangent_snap);
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let pie = h.state().snap_pie.as_ref().expect("open");
    assert!(pie.held);
    assert!(
        (pie.center - at).length() > 60.0,
        "the hub kept clear of the edge"
    );
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert!(
        h.state().snap_pie.as_ref().is_some_and(|p| !p.held),
        "open to click"
    );
    assert_eq!(
        (kinds(&h), h.state().grid_snap, h.state().perp_tangent_snap),
        before,
        "nothing changed"
    );
}

#[test]
fn a_kinds_letter_turns_it_on_or_off_with_the_ring_open() {
    let Some(mut h) = harness() else { return };
    tap_snap_pie(&mut h);
    h.key_press(Key::M);
    h.run();
    assert!(!kinds(&h).has(SnapKind::Midpoint));
    assert_eq!(status(&h), "Midpoint snapping off");
    assert!(h.state().snap_pie.is_some(), "the ring stays");
    assert_eq!(h.state().tool.kind(), ToolKind::Select, "M is the pie's");
    h.key_press(Key::M);
    h.run();
    assert_eq!(kinds(&h), SnapKinds::ALL);
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().snap_pie.is_none());
    // The Selection filter pie's letters the same way.
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.key_press_modifiers(SHIFT, Key::Q);
    h.run();
    h.key_press(Key::E);
    h.run();
    assert!(
        !h.state()
            .drawing
            .editor
            .prefs
            .pick_filter
            .has(PickKind::Edges)
    );
    assert!(h.state().select_pie.is_some());
}

/// The perpendicular & tangent switch, as F3 (in the AutoCAD keys), the palette and the
/// context menu flip it.
fn toggle_perp_tangent(h: &mut Harness<'_, DriftApp>) {
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, Command::Toggle(ViewFlag::PerpTangent));
    h.run();
}

#[test]
fn perpendicular_and_tangent_share_one_switch() {
    let Some(mut h) = harness() else { return };
    assert!(h.state().perp_tangent_snap);
    // Both off in the pie: the switch goes off (the bar's button, the guides).
    tap_snap_pie(&mut h);
    let p = pill(&h, 6).center();
    click_at(&mut h, p);
    assert!(h.state().perp_tangent_snap, "Tangent is still on");
    let p = pill(&h, 7).center();
    click_at(&mut h, p);
    assert!(!h.state().perp_tangent_snap, "neither is on");
    h.key_press(Key::Escape);
    h.run();
    // Turned on again (the bar, the palette): both.
    toggle_perp_tangent(&mut h);
    assert!(h.state().perp_tangent_snap);
    assert_eq!(kinds(&h), SnapKinds::ALL);
    // With it off, Tangent turned on in the pie comes on alone; the switch off and on brings both.
    toggle_perp_tangent(&mut h);
    tap_snap_pie(&mut h);
    let p = pill(&h, 7).center();
    click_at(&mut h, p);
    assert!(h.state().perp_tangent_snap);
    assert!(kinds(&h).has(SnapKind::Tangent) && !kinds(&h).has(SnapKind::Perpendicular));
    h.key_press(Key::Escape);
    h.run();
    toggle_perp_tangent(&mut h);
    toggle_perp_tangent(&mut h);
    assert!(h.state().perp_tangent_snap);
    assert_eq!(kinds(&h), SnapKinds::ALL, "Perpendicular is back");
}

/// A pie's key whose pie can't open now leaves the open pie as it is.
#[test]
fn a_pie_that_refuses_to_open_leaves_the_open_one() {
    let Some(mut h) = harness() else { return };
    assert!(h.state().drawing.editor.selection.is_empty());
    tap_snap_pie(&mut h);
    h.key_press(Key::Q);
    h.run();
    assert!(h.state().snap_pie.is_some(), "the snap pie stays");
    assert!(h.state().pie.is_none());
    assert!(status(&h).starts_with("Select what to relate first"));
    // One that opens takes its place.
    h.key_press_modifiers(SHIFT, Key::Q);
    h.run();
    assert!(h.state().select_pie.is_some() && h.state().snap_pie.is_none());
}

#[test]
fn no_pie_on_a_sheet() {
    let Some(mut h) = harness() else { return };
    for command in [
        Command::RelationsPie,
        Command::SelectionFilterPie,
        Command::SnapPie,
    ] {
        assert!(spec(command).unwrap().when.holds(h.state_mut()));
    }
    tap_snap_pie(&mut h);
    h.state_mut().new_sheet_shown();
    h.run();
    h.run();
    assert!(h.state().drawing.view.sheet_preview);
    assert!(h.state().snap_pie.is_none(), "the open pie went");
    for command in [
        Command::RelationsPie,
        Command::SelectionFilterPie,
        Command::SnapPie,
    ] {
        assert!(
            !spec(command).unwrap().when.holds(h.state_mut()),
            "{command:?} isn't offered on a sheet"
        );
    }
}
