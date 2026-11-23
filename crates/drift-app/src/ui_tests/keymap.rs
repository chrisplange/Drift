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

/// Click the Change button on the row of the command titled `title`.
fn change(h: &mut Harness<'static, DriftApp>, title: &str) {
    // (The tool rail has a button by the same name: the row's is the one beside a Change.)
    let rows: Vec<f32> = h
        .get_all_by_label(title)
        .map(|n| n.rect().center().y)
        .collect();
    let gap = |y: f32| rows.iter().map(|r| (r - y).abs()).fold(f32::MAX, f32::min);
    h.get_all_by_label("Change")
        .min_by(|a, b| gap(a.rect().center().y).total_cmp(&gap(b.rect().center().y)))
        .unwrap()
        .click();
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
    change(&mut h, "Rectangle");
    assert!(h.query_by_label_contains("Press a key").is_some());
    h.key_press(Key::J);
    h.run();
    let rect = crate::commands::spec_by_id("draw.rectangle").unwrap();
    assert_eq!(rect.shortcuts()[0].key, Key::J);
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
    change(&mut h, "Rectangle");
    h.key_press(Key::C);
    h.run();
    // Nothing changed yet: the user chooses.
    assert!(h.query_by_label_contains("is already Circle").is_some());
    let circle = crate::commands::spec_by_id("draw.circle").unwrap();
    assert_eq!(circle.shortcuts()[0].key, Key::C);
    h.get_by_label("Reassign").click();
    h.run();
    assert!(circle.shortcuts().is_empty());
    let rect = crate::commands::spec_by_id("draw.rectangle").unwrap();
    assert_eq!(rect.shortcuts()[0].key, Key::C);
    // A key the tools keep can't be taken.
    change(&mut h, "Rectangle");
    h.key_press(Key::X);
    h.run();
    assert!(h.query_by_label_contains("can't run a command").is_some());
}

#[test]
fn the_keymap_round_trips_through_the_settings() {
    let Some(mut h) = keyboard_page("rectangle") else {
        return;
    };
    change(&mut h, "Rectangle");
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
