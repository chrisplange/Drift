//! The regression smoke suite: one file per area of the app, each test a short run through it
//! as a user drives it (clicks, keys, files handed over as the system would). Every test's
//! name starts with `smoke_`, so `cargo test -p drift-app smoke_` runs the lot.
//!
//! `docs/regression-checklist.md` lists what each test covers. The rule: a regression the user
//! finds becomes a test in the matching file here before its fix merges.

use drift_core::{EntityId, FillId, FillKind};

use super::*;

/// Commands: the palette, the menus, repeat-last, view keys, dialogs over the canvas.
mod commands;
/// Dimensions and the solver: dimension a line, change it, drag it.
mod dimensions;
/// Drawing: closing a loop keeps its relations; snap markers, with an axis lock too.
mod drawing;
/// Save, close and open again; the recovered session; extras from a newer Drift.
mod files;
/// Fills and hatches: solid and hatched shapes, hatch handles, Arrange, Stroke: none.
mod fills;
/// Groups and blocks: picking a group by a fill, entering it, block and back, moving it.
mod groups;
/// Import…, the drop dialog, a file that can't be read.
mod import;
/// Openings: a door placed with Space for its swing, picked, deleted, undone, reopened.
mod openings;
/// Options: values and whole-sketch options, applied and undone.
mod options;
/// Picking where things overlap: hover, click, double-click and drag start alike.
mod picking;
/// The tool rail: everything in view, families and their flyouts, Move and Rotate.
mod rail;
/// Picking and moving: filters, G from a midpoint, axis locks, hidden areas, layer toggles.
mod selection;
/// Sheets: a sheet, its title block and logo, its view, print and export.
mod sheets;
/// Walls: a room and a T, picking and re-thickening, explode, save and open.
mod walls;

/// Show the drawing from `min` to `max`, with grid snapping off (pointer positions are exact).
fn view(h: &mut Harness<'_, DriftApp>, min: DVec2, max: DVec2) {
    h.state_mut().grid_snap = false;
    h.state_mut().fit_target = Some(drift_core::Bounds { min, max });
    h.run();
    h.state_mut().fit_target = None;
    h.run();
}

/// A painter for the output functions (PDF, SVG, DXF), which measure text with it.
fn painter(h: &Harness<'_, DriftApp>) -> egui::Painter {
    h.ctx.layer_painter(egui::LayerId::background())
}

/// The entities added since `before` was taken (from [`ids`]).
fn added(h: &Harness<'_, DriftApp>, before: &[EntityId]) -> Vec<EntityId> {
    ids(h)
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect()
}

fn ids(h: &Harness<'_, DriftApp>) -> Vec<EntityId> {
    h.state().editor.doc.iter().map(|(id, _)| id).collect()
}

/// The curves among `ids`.
fn curves(h: &Harness<'_, DriftApp>, ids: &[EntityId]) -> Vec<EntityId> {
    let doc = &h.state().editor.doc;
    ids.iter()
        .copied()
        .filter(|id| doc.get(*id).is_some_and(|e| !e.kind.is_point()))
        .collect()
}

/// How a rectangle drawn by [`rectangle`] comes out.
#[derive(Clone, Copy, PartialEq)]
enum Fill {
    No,
    Solid,
    Hatch,
}

/// Draw a rectangle from `a` to `b` with the toolbar's Rectangle tool, filled as `fill` asks
/// (the tool bar's Fill box and style), and go back to Select. Returns its four sides and its
/// fill.
fn rectangle(
    h: &mut Harness<'_, DriftApp>,
    a: DVec2,
    b: DVec2,
    fill: Fill,
) -> (Vec<EntityId>, Option<FillId>) {
    let before = ids(h);
    let fills_before: Vec<FillId> = h.state().editor.doc.fills().map(|(id, _)| id).collect();
    h.get_by_label("Rectangle").click();
    h.run();
    if (fill != Fill::No) != h.state().editor.prefs.fill_new_shapes {
        h.get_by_label("Fill").click();
        h.run();
    }
    if fill != Fill::No {
        let style = if fill == Fill::Solid {
            "Solid"
        } else {
            "Hatch"
        };
        h.get_all_by_label(style).next().unwrap().click();
        h.run();
    }
    click_world(h, a);
    click_world(h, b);
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Select").click();
    h.run();
    let sides = curves(h, &added(h, &before));
    assert_eq!(sides.len(), 4, "a rectangle drawn");
    let new_fill = h
        .state()
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .find(|id| !fills_before.contains(id));
    assert_eq!(new_fill.is_some(), fill != Fill::No, "filled as asked");
    let ed = &mut h.state_mut().editor;
    ed.selection.clear();
    ed.selected_fills.clear();
    h.run();
    (sides, new_fill)
}

/// A right-click at a screen position.
fn right_click(h: &mut Harness<'_, DriftApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
}

/// A circle drawn with the Circle tool (unfilled), from its centre to a point on it; back
/// to Select with nothing picked. Returns it.
fn circle(h: &mut Harness<'_, DriftApp>, centre: DVec2, radius: f64) -> EntityId {
    h.state_mut().editor.prefs.fill_new_shapes = false;
    let before = ids(h);
    h.key_press(Key::C);
    h.run();
    click_world(h, centre);
    click_world(h, centre + DVec2::new(radius, 0.0));
    for _ in 0..2 {
        h.key_press(Key::Escape);
        h.run();
    }
    let new = curves(h, &added(h, &before));
    assert_eq!(new.len(), 1, "a circle drawn");
    h.state_mut().editor.selection.clear();
    h.run();
    new[0]
}

/// The status bar's message (what the app said last), or "".
fn status(h: &Harness<'_, DriftApp>) -> String {
    h.state()
        .status
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

/// Press and release at `pos` with `mods` held.
fn click_with(h: &mut Harness<'_, DriftApp>, pos: Pos2, mods: EguiModifiers) {
    h.hover_at(pos);
    h.run();
    h.event(Event::ModifiersChanged(mods));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: mods,
        });
        h.run();
    }
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
}

/// Two clicks at `pos`, close enough together for a double-click (the harness's frames are a
/// quarter of a second apart).
fn double_click(h: &mut Harness<'_, DriftApp>, pos: Pos2) {
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    h.hover_at(pos);
    h.run();
    for _ in 0..2 {
        button(h, pos, true);
        h.step();
        button(h, pos, false);
        h.step();
    }
    h.run();
}

/// Press, move in `steps` and release, from one world point to another.
fn drag(h: &mut Harness<'_, DriftApp>, from: DVec2, to: DVec2, steps: u32) {
    let (a, b) = (screen(h, from), screen(h, to));
    drag_steps(h, a, b, steps, None);
}

/// Undo (Ctrl/⌘+Z).
fn undo(h: &mut Harness<'_, DriftApp>) {
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
}

/// Open the command palette (Ctrl+K), type `text`, and run the best match with Enter.
fn palette(h: &mut Harness<'_, DriftApp>, text: &str) {
    h.event(Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run();
    h.run();
    type_text(h, text);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
}

/// Click a menu in the menu bar (the first widget of that name: the toolbar has a View tool).
fn open_menu(h: &mut Harness<'_, DriftApp>, menu: &str) {
    h.get_all_by_label(menu).next().unwrap().click();
    h.run();
    h.run();
}

/// A file of this test's own in the temporary folder, holding `contents`.
fn temp_file(test: &str, name: &str, contents: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("drift-smoke-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// A point's position.
fn at(h: &Harness<'_, DriftApp>, p: EntityId) -> DVec2 {
    h.state().editor.doc.point(p).unwrap()
}

/// A line's end points.
fn ends(h: &Harness<'_, DriftApp>, line: EntityId) -> (EntityId, EntityId) {
    match h.state().editor.doc.get(line).unwrap().kind {
        EntityKind::Line { a, b } => (a, b),
        ref other => panic!("not a line: {other:?}"),
    }
}

/// Every point's position, in id order.
fn positions(h: &Harness<'_, DriftApp>) -> Vec<(EntityId, DVec2)> {
    let doc = &h.state().editor.doc;
    doc.iter()
        .filter(|(_, e)| e.kind.is_point())
        .map(|(id, _)| (id, doc.point(id).unwrap()))
        .collect()
}

fn is_hatch(h: &Harness<'_, DriftApp>, fill: FillId) -> bool {
    matches!(
        h.state().editor.doc.fill(fill).unwrap().kind,
        FillKind::Hatch(_)
    )
}
