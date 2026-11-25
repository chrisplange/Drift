//! The three pies together: the snap pie's switches (flick, click, letter, and the
//! perpendicular & tangent switch they share), what a flick chooses (what is lit, and nothing
//! for a tap however far the window's edge kept the hub from the pointer), switching between
//! pies, and none on a sheet.

use drift_edit::editor::{PickFilter, PickKind};
use drift_geom::{SnapKind, SnapKinds};
use egui_kittest::kittest::NodeT as _;

use super::*;
use crate::app::pie_style::{DEAD_ZONE, hot_place, pill_under, place_at};
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

/// Near the window's bottom the hub opens well above the pointer, which is then below every
/// pill. Hold the key and move left: the flick goes left (Quadrant), the way the pointer
/// moved, not towards the bottom pill that lies that way from the hub.
#[test]
fn a_flick_near_the_edge_follows_the_gesture() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let at = egui::pos2(canvas.center().x, canvas.bottom() - 20.0);
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let center = h.state().snap_pie.as_ref().expect("open").center;
    let to = at + egui::vec2(-60.0, 0.0);
    assert_eq!(pill_under(&h.state().pie_pills, Some(to)), None);
    assert_ne!(
        place_at(center, to),
        Some(4),
        "from the hub, the pointer is towards another place"
    );
    h.hover_at(to);
    h.run();
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert!(h.state().snap_pie.is_none(), "a flick closes the ring");
    assert_eq!(kinds(&h), SnapKinds::ALL.with(SnapKind::Quadrant, false));
    assert_eq!(status(&h), "Quadrant snapping off");
}

/// Near the window's bottom the hub opens well above the pointer. Hold the key and move
/// straight up, over the bottom pill (Perpendicular) and on: the top place (Grid) is lit all
/// the way, as the hand heads, and letting go turns Grid off, nothing else.
#[test]
fn a_flick_near_the_edge_goes_the_way_the_hand_moves_over_a_pill_behind() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let at = egui::pos2(canvas.center().x, canvas.bottom() - 20.0);
    h.hover_at(at);
    h.run();
    let (grid, perp) = (h.state().grid_snap, h.state().perp_tangent_snap);
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let pie = h.state().snap_pie.as_ref().expect("open");
    let (center, from) = (pie.center, pie.from);
    assert!((center.x - at.x).abs() < 1.0, "straight above");
    let bottom = pill(&h, 6);
    assert!(
        bottom.top() + 6.0 < at.y - DEAD_ZONE,
        "the bottom pill is above the pointer"
    );
    let mut crossed = false;
    let mut p = at;
    while p.y > bottom.top() - 20.0 {
        p.y -= 4.0;
        h.hover_at(p);
        h.run();
        crossed |= bottom.contains(p);
        if (p - from).length() >= DEAD_ZONE {
            let lit = hot_place(&h.state().pie_pills, center, Some(p), Some(from));
            assert_eq!(lit, Some(2), "Grid lit at {p:?}");
        }
    }
    assert!(crossed, "the gesture passed over the bottom pill");
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert!(h.state().snap_pie.is_none(), "a flick closes the ring");
    assert_eq!(kinds(&h), SnapKinds::ALL, "no kind touched");
    assert_eq!(h.state().grid_snap, !grid, "Grid turned over");
    assert_eq!(h.state().perp_tangent_snap, perp, "Perpendicular untouched");
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

/// While the tool is part way through a shape, the relations and Selection filter pies wait:
/// their keys say so on the status line, from another pie (which stays) or from none.
#[test]
fn a_pie_waiting_for_the_tool_says_so() {
    let Some(mut h) = harness() else { return };
    h.state_mut().set_tool(ToolKind::Line);
    h.run();
    let at = h.state().drawing.view.canvas_rect.center();
    click_at(&mut h, at + egui::vec2(-80.0, 40.0));
    assert!(h.state().tool.is_busy(), "a line started");
    // The snap pie opens mid-line; the Selection filter pie's key from it is refused, aloud.
    tap_snap_pie(&mut h);
    h.state_mut().status = None;
    h.key_press_modifiers(SHIFT, Key::Q);
    h.run();
    assert!(h.state().snap_pie.is_some(), "the snap pie stays");
    assert!(h.state().select_pie.is_none());
    assert!(
        status(&h).starts_with("Finish or cancel what the tool is doing"),
        "{}",
        status(&h)
    );
    assert!(status(&h).ends_with("Shift+Q"), "{}", status(&h));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().snap_pie.is_none());
    assert!(h.state().tool.is_busy(), "Esc closed the pie, not the line");
    // With no pie open, the same for both.
    for (key, modifiers) in [(Key::Q, egui::Modifiers::NONE), (Key::Q, SHIFT)] {
        h.state_mut().status = None;
        h.key_press_modifiers(modifiers, key);
        h.run();
        assert!(h.state().pie.is_none() && h.state().select_pie.is_none());
        assert!(
            status(&h).starts_with("Finish or cancel what the tool is doing"),
            "{}",
            status(&h)
        );
    }
}

/// The snap pie's kinds are an app preference: they carry into a new drawing, and through
/// the saved preferences into the next start.
#[test]
fn snap_kinds_stay_for_every_drawing_and_the_next_start() {
    let Some(mut h) = harness() else { return };
    tap_snap_pie(&mut h);
    h.key_press(Key::M);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let off = SnapKinds::ALL.with(SnapKind::Midpoint, false);
    assert_eq!(kinds(&h), off);
    h.state_mut().perform(crate::Pending::New);
    h.run();
    assert_eq!(kinds(&h), off, "a new drawing keeps them");
    let saved = h.state().view_prefs();
    let Some(mut next) = harness() else { return };
    assert_eq!(kinds(&next), SnapKinds::ALL);
    next.state_mut().apply_view_prefs(&saved);
    assert_eq!(kinds(&next), off, "the next start keeps them");
}

/// Whether the status bar's Snap kinds button is lit.
fn snap_kinds_lit(h: &Harness<'_, DriftApp>) -> bool {
    h.get_by_label("Snap kinds").accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

/// The Snap kinds button is lit while a kind is off that no other status-bar switch shows:
/// not for Grid alone (grid snap's own switch), nor for Perpendicular and Tangent with the
/// perpendicular & tangent switch off.
#[test]
fn the_snap_kinds_button_is_lit_for_what_no_other_switch_shows() {
    let Some(mut h) = harness() else { return };
    h.run();
    let set = |h: &mut Harness<'_, DriftApp>, kinds: SnapKinds, grid: bool, pt: bool| {
        let s = h.state_mut();
        s.drawing.editor.prefs.snap_kinds = kinds;
        s.grid_snap = grid;
        s.perp_tangent_snap = pt;
        h.run();
    };
    set(&mut h, SnapKinds::ALL, true, true);
    assert!(!snap_kinds_lit(&h), "every kind on");
    let points_off = SnapKinds::ALL
        .with(SnapKind::Endpoint, false)
        .with(SnapKind::Point, false)
        .with(SnapKind::Origin, false);
    set(&mut h, points_off, true, true);
    assert!(snap_kinds_lit(&h), "Points off");
    set(&mut h, SnapKinds::ALL, false, true);
    assert!(
        !snap_kinds_lit(&h),
        "Grid off only: grid snap's switch shows it"
    );
    // A Grid kind bit off (no build writes one) lights nothing: there'd be no way back.
    set(
        &mut h,
        SnapKinds::ALL.with(SnapKind::Grid, false),
        true,
        true,
    );
    assert!(!snap_kinds_lit(&h), "a stray Grid bit");
    let perp_off = SnapKinds::ALL.with(SnapKind::Perpendicular, false);
    set(&mut h, perp_off, true, true);
    assert!(snap_kinds_lit(&h), "Perpendicular off, the switch on");
    // Perpendicular then Tangent off in the menu: the shared switch goes off and shows it.
    h.get_by_label("Snap kinds").click();
    h.run();
    h.get_by_label("Tangent").click();
    h.run();
    assert!(!h.state().perp_tangent_snap);
    assert!(
        !snap_kinds_lit(&h),
        "the perpendicular & tangent switch shows it"
    );
}

/// A kind turned off shows on the status bar: the Snap kinds button is lit and its menu
/// checks each kind on and off, and turns every kind back on.
#[test]
fn the_status_bar_shows_and_sets_the_snap_kinds() {
    let Some(mut h) = harness() else { return };
    h.run();
    let switches = |h: &Harness<'_, DriftApp>| {
        let s = h.state();
        (
            s.drawing.editor.prefs.snap_kinds,
            s.grid_snap,
            s.perp_tangent_snap,
        )
    };
    assert_eq!(kinds(&h), SnapKinds::ALL);
    // Points off, as a stray flick would leave it.
    h.state_mut().drawing.editor.prefs.snap_kinds = SnapKinds::ALL
        .with(SnapKind::Endpoint, false)
        .with(SnapKind::Point, false)
        .with(SnapKind::Origin, false);
    h.run();
    assert!(snap_kinds_lit(&h), "Points off lights it");
    h.get_by_label("Snap kinds").click();
    h.run();
    save_png("drift_ui_snap_kinds_menu.png", &mut h);
    // Its rows: the pie's kinds. Points checked back on, the menu staying open.
    h.get_by_label("Points").click();
    h.run();
    assert_eq!(kinds(&h), SnapKinds::ALL);
    h.get_by_label("Midpoint").click();
    h.run();
    assert_eq!(kinds(&h), SnapKinds::ALL.with(SnapKind::Midpoint, false));
    // Every kind back on, Grid and the perpendicular & tangent switch too.
    h.get_by_label("Snap to every kind").click();
    h.run();
    assert_eq!(switches(&h), (SnapKinds::ALL, true, true));
}

/// A repeat of the key held down, as the system sends while a key is held.
fn key_repeat(h: &mut Harness<'_, DriftApp>, modifiers: egui::Modifiers, key: Key) {
    h.event_modifiers(
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: true,
            modifiers,
        },
        modifiers,
    );
    h.run();
}

/// A right click where the pointer is.
fn right_click(h: &mut Harness<'_, DriftApp>, p: egui::Pos2) {
    h.hover_at(p);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

/// Hold a pie's key past the system's repeat delay and close the pie with a right click: the
/// repeats that keep coming while the key is down don't open it again.
#[test]
fn a_pie_closed_with_its_key_held_stays_closed_through_the_repeats() {
    let Some(mut h) = harness() else { return };
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    for key in [Key::S, Key::Q] {
        h.key_down_modifiers(SHIFT, key);
        h.run();
        key_repeat(&mut h, SHIFT, key);
        let open = |h: &Harness<'_, DriftApp>| {
            h.state().snap_pie.is_some() || h.state().select_pie.is_some()
        };
        assert!(open(&h), "{key:?}: open while held, repeats and all");
        right_click(&mut h, at);
        assert!(!open(&h), "{key:?}: a right click closes it");
        key_repeat(&mut h, SHIFT, key);
        key_repeat(&mut h, SHIFT, key);
        assert!(!open(&h), "{key:?}: the repeats don't open it again");
        h.key_up_modifiers(SHIFT, key);
        h.run();
        assert!(!open(&h), "{key:?}: still closed when the key comes up");
    }
}

/// Hold Shift+Q, let go of Shift a moment before Q: the plain-Q repeats that follow don't go
/// to the relations pie in the Selection filter pie's place.
#[test]
fn a_repeat_of_another_pies_key_does_not_switch_pies() {
    let Some(mut h) = harness() else { return };
    let at = h.state().drawing.view.canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::Q);
    h.run();
    assert!(h.state().select_pie.is_some());
    h.state_mut().status = None;
    key_repeat(&mut h, egui::Modifiers::NONE, Key::Q);
    assert!(
        h.state().select_pie.is_some(),
        "the Selection filter pie stays"
    );
    assert!(h.state().pie.is_none());
    assert_eq!(status(&h), "", "nothing said about the relations pie");
}

/// Near the window's bottom the hub opens well above the pointer. With the key held, move
/// onto a pill other than the one a flick would choose and click: the click chooses what is
/// lit (and captioned), not the pill under the pointer. Letting go then just closes the ring.
#[test]
fn a_click_with_the_key_held_near_the_edge_chooses_what_is_lit() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let at = egui::pos2(canvas.center().x, canvas.bottom() - 20.0);
    // The snap pie.
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::S);
    h.run();
    let pie = h.state().snap_pie.as_ref().expect("open");
    let (center, from) = (pie.center, pie.from);
    let quadrant = pill(&h, 4).center();
    let lit = hot_place(&h.state().pie_pills, center, Some(quadrant), Some(from));
    assert_eq!(lit, Some(3), "over Quadrant, the gesture lights Center");
    let grid = h.state().grid_snap;
    click_at(&mut h, quadrant);
    assert_eq!(
        kinds(&h),
        SnapKinds::ALL.with(SnapKind::Center, false),
        "Center, the lit pill, turned off; Quadrant untouched"
    );
    assert_eq!(h.state().grid_snap, grid);
    h.key_up_modifiers(SHIFT, Key::S);
    h.run();
    assert!(h.state().snap_pie.is_none(), "letting go closes the ring");
    assert_eq!(kinds(&h), SnapKinds::ALL.with(SnapKind::Center, false));
    // The Selection filter pie the same way: Points lit over Areas.
    h.hover_at(at);
    h.run();
    h.key_down_modifiers(SHIFT, Key::Q);
    h.run();
    let pie = h.state().select_pie.as_ref().expect("open");
    let (center, from) = (pie.center, pie.from);
    let areas = pill(&h, 4).center();
    let lit = hot_place(&h.state().pie_pills, center, Some(areas), Some(from));
    assert_eq!(lit, Some(3), "over Areas, the gesture lights Points");
    click_at(&mut h, areas);
    let filter = h.state().drawing.editor.prefs.pick_filter;
    assert!(!filter.has(PickKind::Points), "Points, the lit pill, off");
    assert!(filter.has(PickKind::Areas), "Areas untouched");
}

/// The relations pie the same way, where it matters most: the caption and the conflict warning
/// are for the lit pill. Two lines picked, Q held near the window's bottom, a click over Equal
/// while the gesture lights Perpendicular adds Perpendicular, not Equal.
#[test]
fn a_click_with_q_held_near_the_edge_adds_the_lit_relation() {
    let Some(mut h) = harness() else { return };
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let p = [
        DVec2::new(-40.0, -10.0),
        DVec2::new(20.0, -8.0),
        DVec2::new(-40.0, 15.0),
        DVec2::new(25.0, 20.0),
    ]
    .map(|at| ed.doc.add_point(at));
    let a = ed.doc.add_line(p[0], p[1]).unwrap();
    let b = ed.doc.add_line(p[2], p[3]).unwrap();
    ed.commit("Lines");
    h.state_mut().drawing.editor.selection = [a, b].into();
    h.run();
    let canvas = h.state().drawing.view.canvas_rect;
    let at = egui::pos2(canvas.center().x, canvas.bottom() - 20.0);
    h.hover_at(at);
    h.run();
    h.key_down(Key::Q);
    h.run();
    let pie = h.state().pie.as_ref().expect("open");
    let (center, from) = (pie.center, pie.from);
    let slices = h.state().pie_slices();
    assert_eq!(slices[3].label, "Perpendicular");
    assert_eq!(slices[4].label, "Equal");
    let equal = pill(&h, 4).center();
    let lit = hot_place(&h.state().pie_pills, center, Some(equal), Some(from));
    assert_eq!(lit, Some(3), "over Equal, the gesture lights Perpendicular");
    click_at(&mut h, equal);
    let added: Vec<&str> = h
        .state()
        .drawing
        .editor
        .doc
        .constraints()
        .map(|(_, c)| c.type_name())
        .collect();
    assert_eq!(added, ["Perpendicular"], "the lit relation, not Equal");
    h.key_up(Key::Q);
    h.run();
}
