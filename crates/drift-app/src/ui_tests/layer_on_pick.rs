//! Picking an object and the layers (drift-releases#19): Preferences → Layers → "When I
//! select an object" shows its layer in the Layers panel (the default), makes it current, or
//! does nothing; and "Make its layer current" works whatever is chosen.

use drift_core::{EntityId, LayerId};

use super::*;
use crate::commands::Command;
use crate::layers_ui::LayerOnPick;

/// The app with the Layers panel shown, the current layer "0", and two lines: one on
/// layer "Walls" at y = 0 and one on layer "Notes" at y = 20, each from x = 0 to 40, both
/// layers apart from the current one. Returns (app, walls, notes, wall line, note line).
fn two_layers() -> Option<(
    Harness<'static, DriftApp>,
    LayerId,
    LayerId,
    EntityId,
    EntityId,
)> {
    let mut h = harness()?;
    h.state_mut().grid_snap = false;
    let (walls, notes, w, n) = {
        let ed = &mut h.state_mut().drawing.editor;
        let walls = ed.add_layer("Walls");
        let notes = ed.add_layer("Notes");
        ed.set_current_layer(LayerId::DEFAULT);
        ed.doc.begin();
        let p = [
            DVec2::new(0.0, 0.0),
            DVec2::new(40.0, 0.0),
            DVec2::new(0.0, 20.0),
            DVec2::new(40.0, 20.0),
        ]
        .map(|q| ed.doc.add_point(q));
        let w = ed.doc.add_line(p[0], p[1]).unwrap();
        let n = ed.doc.add_line(p[2], p[3]).unwrap();
        ed.doc.set_entity_layer(w, walls).unwrap();
        ed.doc.set_entity_layer(n, notes).unwrap();
        ed.commit("Lines");
        (walls, notes, w, n)
    };
    h.state_mut().show_panel(crate::dock::Panel::Layers);
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-20.0, -20.0),
        max: DVec2::new(60.0, 40.0),
    });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    Some((h, walls, notes, w, n))
}

fn current(h: &Harness<'_, DriftApp>) -> LayerId {
    h.state().drawing.editor.doc.current_layer()
}

/// The status bar's message.
fn status(h: &Harness<'_, DriftApp>) -> String {
    h.state()
        .status
        .as_ref()
        .map_or_else(String::new, |s| s.text.clone())
}

/// The default shows the picked line's layer in the panel; the current layer stays.
#[test]
fn by_default_a_pick_shows_its_layer() {
    let Some((mut h, walls, _, w, _)) = two_layers() else {
        return;
    };
    assert_eq!(h.state().layer_on_pick, LayerOnPick::Show);
    click_world(&mut h, DVec2::new(13.0, 0.0));
    h.run();
    assert!(h.state().drawing.editor.selection.contains(&w));
    assert_eq!(h.state().drawing.layer_list.shown, Some(walls));
    assert_eq!(current(&h), LayerId::DEFAULT, "the current layer stays");
    // Nothing picked: nothing outlined.
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert_eq!(h.state().drawing.layer_list.shown, None);
}

/// "Make its layer current": a pick makes its layer current, as no undo step and no change
/// to save. Undo and Redo of an earlier edit then leave it current (that edit didn't change
/// the current layer); a line drawn next goes on it; Save writes it.
#[test]
fn make_current_follows_the_pick_without_an_undo_step() {
    let Some((mut h, walls, notes, _, n)) = two_layers() else {
        return;
    };
    h.state_mut().layer_on_pick = LayerOnPick::MakeCurrent;
    // As if just saved.
    let revision = h.state().drawing.editor.doc.revision();
    h.state_mut().drawing.file.clean_revision = Some(revision);
    let steps = h.state().drawing.editor.history.undo_depth();
    click_world(&mut h, DVec2::new(13.0, 0.0));
    h.run();
    assert_eq!(current(&h), walls);
    assert_eq!(h.state().drawing.layer_list.shown, Some(walls));
    assert_eq!(
        h.state().drawing.editor.history.undo_depth(),
        steps,
        "no undo step"
    );
    assert!(!h.state().is_dirty(), "not a change to save");
    // The other line, picked: its layer.
    click_world(&mut h, DVec2::new(13.0, 20.0));
    h.run();
    assert!(h.state().drawing.editor.selection.contains(&n));
    assert_eq!(current(&h), notes);
    // Undo the lines (an earlier edit), and redo: no panic, the current layer stays.
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().undo();
    h.run();
    assert_eq!(current(&h), notes);
    h.state_mut().redo();
    h.run();
    assert_eq!(current(&h), notes);
    // Nothing reads the current layer from a stale copy: a line drawn now goes on it.
    let before: Vec<EntityId> = h
        .state()
        .drawing
        .editor
        .doc
        .iter()
        .map(|(id, _)| id)
        .collect();
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(0.0, -10.0));
    click_world(&mut h, DVec2::new(30.0, -10.0));
    h.key_press(Key::Escape);
    h.run();
    let drawn: Vec<EntityId> = h
        .state()
        .drawing
        .editor
        .doc
        .iter()
        .filter(|(id, e)| !before.contains(id) && !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    assert_eq!(drawn.len(), 1);
    assert_eq!(
        h.state().drawing.editor.doc.get(drawn[0]).unwrap().layer,
        notes
    );
    // Save writes the layer current at save.
    let text = drift_io::save(&h.state().drawing.editor.doc, &Default::default());
    let (read, _) = drift_io::load(&text).unwrap();
    assert_eq!(read.current_layer(), notes);
}

/// A locked or hidden layer is shown but never made current by a pick.
#[test]
fn a_locked_layer_is_not_made_current() {
    let Some((mut h, walls, _, w, _)) = two_layers() else {
        return;
    };
    h.state_mut().layer_on_pick = LayerOnPick::MakeCurrent;
    // (A locked object can't be picked by a click; picked first, then locked, before the
    // canvas drops it from the pick.)
    let app = h.state_mut();
    let mut layer = app.drawing.editor.doc.layer(walls).unwrap().clone();
    layer.locked = true;
    app.drawing.editor.update_layer(walls, layer);
    app.drawing.editor.selection = [w].into_iter().collect();
    app.drawing.editor.set_active(Some(w));
    app.follow_pick(0.0);
    assert_eq!(app.drawing.editor.doc.current_layer(), LayerId::DEFAULT);
    assert_eq!(
        app.drawing.layer_list.shown,
        Some(walls),
        "shown all the same"
    );
    assert!(
        app.status
            .as_ref()
            .is_some_and(|s| s.text.contains("Walls is locked")),
        "{:?}",
        app.status.as_ref().map(|s| &s.text)
    );
}

/// "Do nothing": a pick neither shows nor changes the layer.
#[test]
fn do_nothing_leaves_the_layers_alone() {
    let Some((mut h, _, _, _, _)) = two_layers() else {
        return;
    };
    h.state_mut().layer_on_pick = LayerOnPick::Nothing;
    click_world(&mut h, DVec2::new(13.0, 0.0));
    h.run();
    assert_eq!(h.state().drawing.layer_list.shown, None);
    assert_eq!(current(&h), LayerId::DEFAULT);
}

/// "Make its layer current" from the palette works under "Do nothing": the layer becomes
/// current (one undo step), and the Layers panel shows it. With objects on two layers
/// picked and none active (a box pick), it does nothing and says so.
#[test]
fn the_command_works_whatever_the_preference() {
    let Some((mut h, walls, _, w, n)) = two_layers() else {
        return;
    };
    h.state_mut().layer_on_pick = LayerOnPick::Nothing;
    click_world(&mut h, DVec2::new(13.0, 0.0));
    h.run();
    let steps = h.state().drawing.editor.history.undo_depth();
    let ctx = h.ctx.clone();
    h.state_mut()
        .run_command(&ctx, Command::MakePickedLayerCurrent);
    h.run();
    assert_eq!(current(&h), walls);
    assert_eq!(h.state().drawing.layer_list.shown, Some(walls));
    assert_eq!(
        h.state().drawing.editor.history.undo_depth(),
        steps + 1,
        "an undo step"
    );
    // Both lines, none active.
    let app = h.state_mut();
    app.drawing.editor.set_current_layer(LayerId::DEFAULT);
    app.drawing.editor.selection = [w, n].into_iter().collect();
    app.drawing.editor.set_active(None);
    h.run();
    h.state_mut()
        .run_command(&ctx, Command::MakePickedLayerCurrent);
    h.run();
    assert_eq!(current(&h), LayerId::DEFAULT, "nothing done");
    assert!(
        status(&h).contains("Objects on 2 layers picked"),
        "{}",
        status(&h)
    );
}

/// The preference is kept with the app's other preferences.
#[test]
fn the_preference_is_kept() {
    let Some(mut h) = harness() else { return };
    h.state_mut().layer_on_pick = LayerOnPick::MakeCurrent;
    let prefs = h.state().view_prefs();
    assert!(prefs.contains("layerpick=c"), "{prefs}");
    h.state_mut().layer_on_pick = LayerOnPick::Nothing;
    h.state_mut().apply_view_prefs(&prefs);
    assert_eq!(h.state().layer_on_pick, LayerOnPick::MakeCurrent);
}
