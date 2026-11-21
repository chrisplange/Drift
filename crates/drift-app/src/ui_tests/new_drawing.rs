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

/// Imperial in Preferences: the untouched blank drawing open now and File → New start in feet
/// and inches, a drawing with something in it and a file opened keep their own units, and the
/// choice is kept with the app's settings for the next start.
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
    // The blank drawing open now, nothing done to it yet, follows (and stays clean).
    assert_eq!(units(&h), Units::architectural(), "the untouched blank");
    assert!(!h.state().is_dirty());
    // One with something drawn keeps its units.
    draw_a_point(&mut h);
    h.get_by_label("Metric (mm)").click();
    h.run();
    assert_eq!(h.state().new_drawing.units, NewUnits::Metric);
    assert_eq!(units(&h), Units::architectural(), "drawn in: left as it is");
    h.get_by_label("Imperial (ft-in)").click();
    h.run();
    h.state_mut().show_settings = false;
    h.run();
    h.state_mut().drawing.file.clean_revision = Some(h.state().drawing.editor.doc.revision());
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

    // Kept with the settings, and a blank drawing kept from last time (in millimetres) starts
    // in feet and inches, as does the next File → New.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    assert_eq!(
        storage.get_string("drift.new_units").as_deref(),
        Some("imperial")
    );
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&MemoryStorage::default());
    assert_eq!(fresh.state().new_drawing.units, NewUnits::Metric);
    keep_session(&mut storage, &drift_core::Document::new(), None);
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert_eq!(fresh.state().new_drawing.units, NewUnits::Imperial);
    assert_eq!(units(&fresh), Units::architectural(), "the blank drawing");
    assert!(!fresh.state().is_dirty());
    fresh.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    fresh.run_steps(2);
    assert_eq!(units(&fresh), Units::architectural());
}

/// Draw a point (one undo step): the drawing is no longer an untouched new one.
fn draw_a_point(h: &mut Harness<'_, DriftApp>) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    ed.doc.add_point(DVec2::new(5.0, 5.0));
    ed.commit("Point");
    h.run();
}

/// Keep `doc` in `storage` as last session's drawing, clean, from `path` (or none).
fn keep_session(storage: &mut MemoryStorage, doc: &drift_core::Document, path: Option<&str>) {
    storage.set_string(
        crate::KEY_DOCUMENT,
        drift_io::save(doc, &Default::default()),
    );
    storage.set_string(crate::KEY_NAME, "Kept.drift".into());
    storage.set_string(crate::KEY_PATH, path.unwrap_or_default().into());
    storage.set_string(crate::KEY_DIRTY, "0".into());
}

/// A drawing kept from last time that isn't an untouched new one (something in it, or a file
/// of its own) keeps its own units and logo, whatever new drawings start with now.
#[test]
fn a_kept_drawing_keeps_its_units_and_logo() {
    let mut storage = MemoryStorage::default();
    storage.set_string("drift.new_units", "imperial".into());
    storage.set_string("drift.default_logo", LOGO_SVG.into());
    let mut drawn = drift_core::Document::new();
    drawn.begin();
    drawn.add_point(DVec2::new(5.0, 5.0));
    let _ = drawn.commit("Point");
    let path = std::env::temp_dir().join("drift-tests").join("Kept.drift");
    let path = path.to_string_lossy().into_owned();
    for (doc, path) in [
        (drawn, None),
        (drift_core::Document::new(), Some(path.as_str())),
    ] {
        keep_session(&mut storage, &doc, path);
        let Some(mut h) = harness() else { return };
        h.state_mut().restore_session(&storage);
        h.run();
        assert_eq!(h.state().new_drawing.units, NewUnits::Imperial);
        assert_eq!(units(&h), Units::metric(), "{path:?}");
        assert!(logo(&h).is_empty(), "{path:?}");
        assert!(!h.state().is_dirty());
    }
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
    h.state_mut().replace_document(
        doc,
        "Theirs.drift".into(),
        Some("Theirs.drift".into()),
        Default::default(),
        true,
    );
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

/// #429: the blank drawing open now, nothing done to it yet, follows the default logo: one
/// kept from last time gets it at start, and Clear takes it off again.
#[test]
fn the_untouched_blank_follows_the_default_logo() {
    let mut storage = MemoryStorage::default();
    storage.set_string("drift.default_logo", LOGO_SVG.into());
    keep_session(&mut storage, &drift_core::Document::new(), None);
    let Some(mut h) = harness() else { return };
    h.state_mut().restore_session(&storage);
    h.run();
    assert_eq!(logo(&h), LOGO_SVG, "the kept blank drawing gets it");
    assert!(!h.state().is_dirty());
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    h.get_by_label("Clear").click();
    h.run();
    assert!(h.state().new_drawing.logo.is_empty());
    assert!(logo(&h).is_empty(), "and loses it with Clear");
    assert!(!h.state().is_dirty());
}
