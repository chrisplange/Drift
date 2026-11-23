//! Assignable keys (#499): Preferences → Keyboard changes a command's key and the canvas uses
//! it; a key another command has is asked about; the keymap round-trips through the settings.

use super::*;
use crate::app::start_ui::PrefsPage;
use crate::keymap::{self, Active};

/// The app with Preferences open on Keyboard, searching for `query`.
fn keyboard_page(query: &str) -> Option<Harness<'static, DriftApp>> {
    let mut h = harness()?;
    keymap::install(keymap::Keymap::default());
    h.state_mut().show_settings = true;
    h.ctx
        .data_mut(|d| d.insert_temp(egui::Id::new("preferences_page"), PrefsPage::Keyboard));
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput).focus();
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text(query);
    h.run();
    Some(h)
}

/// Click the key cap `key` of the command titled `title`, to change it.
fn change(h: &mut Harness<'static, DriftApp>, title: &str, key: &str) {
    h.get_by_label(&format!("Change {title} key {key}")).click();
    h.run();
}

fn close_prefs(h: &mut Harness<'static, DriftApp>) {
    h.state_mut().show_settings = false;
    h.run();
}

#[test]
fn reassign_a_key_and_use_it() {
    let Some(mut h) = keyboard_page("rectangle") else {
        return;
    };
    change(&mut h, "Rectangle", "R");
    assert!(h.query_by_label_contains("press a key").is_some());
    h.key_press(Key::J);
    h.run();
    let rect = crate::commands::spec_by_id("draw.rectangle").unwrap();
    // In place of R: R no longer picks it.
    assert_eq!(rect.shortcuts().len(), 1);
    assert_eq!(rect.shortcuts()[0].key, Key::J);
    assert!(h.query_by_label_contains("Made your own preset").is_some());
    assert!(keymap::with(|m| matches!(m.active, Active::User(0))));
    // Shown with the new key, and the canvas runs it.
    assert_eq!(rect.key_text(false), "J");
    close_prefs(&mut h);
    h.key_press(Key::J);
    h.run();
    assert_eq!(
        h.state().tool.kind(),
        drift_edit::tools::ToolKind::Rectangle
    );
}

#[test]
fn a_clash_is_asked_about() {
    let Some(mut h) = keyboard_page("rectangle") else {
        return;
    };
    change(&mut h, "Rectangle", "R");
    h.key_press(Key::C);
    h.run();
    // Nothing changed yet: the user chooses.
    assert!(h.query_by_label_contains("is Circle's only key").is_some());
    let circle = crate::commands::spec_by_id("draw.circle").unwrap();
    assert_eq!(circle.shortcuts()[0].key, Key::C);
    h.get_by_label("Reassign").click();
    h.run();
    assert!(circle.shortcuts().is_empty());
    let rect = crate::commands::spec_by_id("draw.rectangle").unwrap();
    assert_eq!(rect.shortcuts()[0].key, Key::C);
    // A key the tools keep can't be taken.
    change(&mut h, "Rectangle", "C");
    h.key_press(Key::X);
    h.run();
    assert!(
        h.query_by_label_contains("is kept for locking to X")
            .is_some()
    );
}

#[test]
fn the_keymap_round_trips_through_the_settings() {
    let Some(mut h) = keyboard_page("rectangle") else {
        return;
    };
    change(&mut h, "Rectangle", "R");
    h.key_press(Key::J);
    h.run();
    let saved = keymap::current();
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    keymap::install(keymap::Keymap::default());
    keymap::load(&storage);
    assert_eq!(keymap::current(), saved);
    let rect = crate::commands::spec_by_id("draw.rectangle").unwrap();
    assert_eq!(rect.shortcuts()[0].key, Key::J);
}
