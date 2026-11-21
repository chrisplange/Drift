//! What new drawings start with on this computer: Preferences → New drawings use (#495).

use drift_core::Units;
use eframe::Storage as _;

use super::*;
use crate::app::new_drawing::NewUnits;

fn units(h: &Harness<'_, DriftApp>) -> Units {
    h.state().drawing.editor.doc.units()
}

/// Imperial in Preferences: File → New starts in feet and inches, a file opened keeps its own
/// units, and the choice is kept with the app's settings for the next start.
#[test]
fn imperial_new_drawings_from_preferences() {
    let Some(mut h) = harness() else { return };
    assert_eq!(h.state().new_drawing.units, NewUnits::Metric);
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    save_png("drift_ui_prefs_new_drawings.png", &mut h);
    h.get_by_label("Imperial (ft-in)").click();
    h.run();
    assert_eq!(h.state().new_drawing.units, NewUnits::Imperial);
    // The drawing open now keeps its units: only new ones start in the chosen ones.
    assert_eq!(units(&h), Units::metric());
    h.state_mut().show_settings = false;
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(units(&h), Units::architectural(), "File → New");
    assert!(!h.state().is_dirty(), "a new drawing has nothing to save");
    // A file opened keeps its own units.
    let text = drift_io::save(&drift_core::demo::mounting_plate(), &Default::default());
    let (doc, meta) = drift_io::load(&text).unwrap();
    assert_eq!(doc.units(), Units::metric());
    h.state_mut()
        .replace_document(doc, "Plate.drift".into(), None, meta, true);
    h.run();
    assert_eq!(units(&h), Units::metric(), "opened as it was saved");

    // Kept with the settings: the next start's blank drawing, and its File → New, are imperial.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    assert_eq!(
        storage.get_string("drift.new_units").as_deref(),
        Some("imperial")
    );
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&MemoryStorage::default());
    assert_eq!(fresh.state().new_drawing.units, NewUnits::Metric);
    // (A session to restore would replace the blank drawing: start from none.)
    storage.set_string(crate::KEY_DOCUMENT, String::new());
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert_eq!(fresh.state().new_drawing.units, NewUnits::Imperial);
    assert_eq!(units(&fresh), Units::architectural(), "the blank drawing");
    assert!(!fresh.state().is_dirty());
    fresh.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    fresh.run_steps(2);
    assert_eq!(units(&fresh), Units::architectural());
}

/// The start screen's Metric / Imperial switch sets the blank drawing's units and the
/// preference, so the next new drawing starts in them too.
#[test]
fn start_screen_units_switch_sets_the_preference() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_start = true;
    h.run();
    h.run();
    h.get_by_label("Imperial").click();
    h.run();
    assert_eq!(units(&h), Units::architectural());
    assert_eq!(h.state().new_drawing.units, NewUnits::Imperial);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(units(&h), Units::architectural(), "the next new drawing");
    h.state_mut().show_start = true;
    h.run();
    h.run();
    h.get_by_label("Metric").click();
    h.run();
    assert_eq!(units(&h), Units::metric());
    assert_eq!(h.state().new_drawing.units, NewUnits::Metric);
}
