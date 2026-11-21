//! What new drawings start with on this computer: Preferences → New drawings use (#495) and
//! the title block logo (#429).

use drift_core::Units;
use eframe::Storage as _;

use super::*;
use crate::app::new_drawing::NewUnits;
use crate::ui_tests::import::LOGO_SVG;

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

/// A second logo, to tell apart from `LOGO_SVG`.
const OTHER_LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
  <rect x="0" y="0" width="20" height="20" fill="#2266ff"/>
</svg>"##;

fn logo<'a>(h: &'a Harness<'_, DriftApp>) -> &'a str {
    h.state().drawing.editor.doc.logo()
}

/// #429: a logo chosen for a drawing is the one new drawings start with; Remove on a drawing
/// leaves it for the next ones.
#[test]
fn a_chosen_logo_is_kept_for_new_drawings() {
    let Some(mut h) = harness() else { return };
    assert!(h.state().new_drawing.logo.is_empty());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert!(logo(&h).is_empty(), "none until one is chosen");
    h.state_mut().set_logo(LOGO_SVG).unwrap();
    assert_eq!(h.state().new_drawing.logo, LOGO_SVG);
    // A logo that can't be read is refused, and the default stays.
    assert!(
        h.state_mut()
            .set_logo("<svg><text>Acme</text></svg>")
            .is_err()
    );
    assert_eq!(h.state().new_drawing.logo, LOGO_SVG);
    h.state_mut().drawing.file.clean_revision = Some(h.state().drawing.editor.doc.revision());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(logo(&h), LOGO_SVG, "the new drawing has it");
    assert!(!h.state().is_dirty(), "and nothing to save yet");
    // Remove, in a sheet's title block settings: gone from this drawing only.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    h.get_by_label("Remove").click();
    h.run();
    assert!(logo(&h).is_empty(), "removed from the drawing");
    assert_eq!(h.state().new_drawing.logo, LOGO_SVG, "the default stays");
    h.state_mut().drawing.file.clean_revision = Some(h.state().drawing.editor.doc.revision());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(logo(&h), LOGO_SVG, "the next new drawing still has it");
    // Choosing another replaces it.
    h.state_mut().set_logo(OTHER_LOGO).unwrap();
    assert_eq!(h.state().new_drawing.logo, OTHER_LOGO);
}

/// #429: Preferences → General clears the default logo, or takes the open drawing's; the
/// default is kept with the app's settings.
#[test]
fn the_default_logo_is_cleared_or_replaced_in_preferences() {
    let Some(mut h) = harness() else { return };
    h.state_mut().set_logo(LOGO_SVG).unwrap();
    // A drawing opened with a logo of its own doesn't change the default...
    let mut doc = drift_core::Document::new();
    doc.set_logo(OTHER_LOGO.into());
    h.state_mut()
        .replace_document(doc, "Theirs.drift".into(), None, Default::default(), true);
    assert_eq!(h.state().new_drawing.logo, LOGO_SVG);
    // ...until Use this drawing's.
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    save_png("drift_ui_prefs_default_logo.png", &mut h);
    h.get_by_label("Use this drawing's").click();
    h.run();
    assert_eq!(h.state().new_drawing.logo, OTHER_LOGO);

    // Kept with the settings, for the next start.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    storage.set_string(crate::KEY_DOCUMENT, String::new());
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert_eq!(fresh.state().new_drawing.logo, OTHER_LOGO);
    assert_eq!(logo(&fresh), OTHER_LOGO, "the blank drawing at startup");
    assert!(!fresh.state().is_dirty());

    // Clear: new drawings start without one; the open drawing keeps its own.
    h.run();
    h.get_by_label("Clear").click();
    h.run();
    assert!(h.state().new_drawing.logo.is_empty());
    assert_eq!(logo(&h), OTHER_LOGO);
    h.state_mut().show_settings = false;
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert!(logo(&h).is_empty());
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let Some(mut next) = harness() else { return };
    next.state_mut().restore_session(&storage);
    assert!(next.state().new_drawing.logo.is_empty(), "cleared for good");
    // A saved logo this Drift can't read is dropped.
    storage.set_string("drift.default_logo", "<svg><text>Acme</text></svg>".into());
    next.state_mut().restore_session(&storage);
    assert!(next.state().new_drawing.logo.is_empty());
}
