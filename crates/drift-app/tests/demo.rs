//! The browser demo (`--features demo`): every way a drawing could leave the app shows the
//! "Not in the demo" note instead, and copies stay inside it. The code behind those ways
//! isn't compiled in, so this also checks the build: run it with
//! `cargo test -p drift-app --features demo --test demo`.

#![cfg(feature = "demo")]

use drift_app::DriftApp;
use eframe::egui::{self, Event, Key, Modifiers};
use eframe::egui_wgpu::wgpu;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

fn harness() -> Option<Harness<'static, DriftApp>> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    if adapter.is_err() {
        eprintln!("no GPU adapter available; skipping UI test");
        return None;
    }
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .wgpu()
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    // Something to save: the example drawing, from the start panel.
    h.get_by_label("Open the example").click();
    h.run();
    h.run();
    Some(h)
}

/// The menu item `label`, its shortcut ("Ctrl+S", "⌘S") or not.
fn item<'h>(h: &'h Harness<'_, DriftApp>, label: &'h str) -> egui_kittest::Node<'h> {
    h.query_all_by_label_contains(label)
        .find(|n| {
            let text = n.accesskit_node().label().unwrap_or_default();
            text.strip_prefix(label)
                .is_some_and(|rest| !rest.trim().contains(' '))
        })
        .unwrap_or_else(|| panic!("no menu item {label}"))
}

/// The note is up (naming `what`); close it.
fn note_shown(h: &mut Harness<'_, DriftApp>, what: &str) {
    assert!(
        h.query_by_label("Not in the demo").is_some(),
        "{what}: no demo note"
    );
    assert!(
        h.query_by_label_contains(&format!("{what} isn't in the browser demo"))
            .is_some(),
        "{what}: the note doesn't say what"
    );
    // The note's button, besides the banner's link.
    assert_eq!(h.query_all_by_label("Download Drift").count(), 2);
    h.get_by_label("Close").click();
    h.run();
    assert!(h.query_by_label("Not in the demo").is_none());
}

#[test]
fn the_banner_says_nothing_is_saved_and_closes() {
    let Some(mut h) = harness() else { return };
    assert!(h.query_by_label("Demo: nothing is saved.").is_some());
    assert!(h.query_by_label("Download Drift").is_some());
    h.get_by_label("×").click();
    h.run();
    assert!(h.query_by_label("Demo: nothing is saved.").is_none());
}

#[test]
fn file_menu_saves_exports_and_prints_show_the_note() {
    let Some(mut h) = harness() else { return };
    let items: [(&[&str], &str); 9] = [
        (&["Save"], "Saving"),
        (&["Save as…"], "Saving"),
        (&["Export", "PDF…"], "Exporting a PDF"),
        (&["Export", "SVG…"], "Exporting an SVG"),
        (&["Export", "DWG / DXF…"], "Exporting DWG or DXF"),
        (&["Export", "Model as SVG…"], "Exporting an SVG"),
        (&["Export", "Model as PDF…"], "Exporting a PDF"),
        (
            &["Export", "Blender profiles…"],
            "Exporting Blender profiles",
        ),
        (&["Print…"], "Printing"),
    ];
    for (path, what) in items {
        h.get_by_label("File").click();
        h.run();
        for label in path {
            item(&h, label).click();
            h.run();
        }
        note_shown(&mut h, what);
    }
}

#[test]
fn shortcuts_show_the_note() {
    let Some(mut h) = harness() else { return };
    for (modifiers, key, what) in [
        (Modifiers::COMMAND, Key::S, "Saving"),
        (Modifiers::COMMAND | Modifiers::SHIFT, Key::S, "Saving"),
        (Modifiers::COMMAND, Key::P, "Printing"),
    ] {
        h.key_press_modifiers(modifiers, key);
        h.run();
        note_shown(&mut h, what);
    }
}

#[test]
fn copies_stay_in_the_demo() {
    let Some(mut h) = harness() else { return };
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    h.event(Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(_)));
    assert!(!copied, "the copy went to the system clipboard");
    h.run();
    // It was copied here, though: Edit → Paste takes it.
    h.get_by_label("Edit").click();
    h.run();
    assert!(!item(&h, "Paste").accesskit_node().is_disabled());
}
