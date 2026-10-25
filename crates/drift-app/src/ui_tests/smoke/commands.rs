//! Commands: the palette finds Move, Rotate and Scale; the Draw and Modify menus open;
//! Enter repeats the last command; Shift+Alt+W toggles thin lines; a dialog keeps canvas keys
//! from the drawing, and Esc closes only the dialog.

use crate::commands::Command;

use super::*;

/// A line in view, picked.
fn picked_line(h: &mut Harness<'_, DriftApp>) -> EntityId {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(40.0, 10.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.selection = [line].into_iter().collect();
    h.run();
    line
}

#[test]
fn smoke_commands_palette_and_menus() {
    let Some(mut h) = harness() else { return };
    picked_line(&mut h);
    for (words, command) in [
        ("move", Command::Move),
        ("rotate", Command::Rotate),
        ("scale", Command::Scale),
    ] {
        let best = h
            .state_mut()
            .palette_matches(words)
            .first()
            .map(|e| e.command);
        assert_eq!(best, Some(command), "\"{words}\"");
    }
    // Typed into the palette and run, Move grabs the picked line.
    h.event(Event::PointerMoved(screen(&h, DVec2::new(20.0, 5.0))));
    h.run();
    palette(&mut h, "Move");
    assert!(h.state().tool.is_modal(), "Move grabs the selection");
    h.key_press(Key::Escape);
    h.run();
    // The Draw and Modify menus open, with their commands. (The toolbar has tools of the
    // same names: count them before.)
    for (menu, items) in [
        ("Draw", &["Line", "Rectangle", "Polygon"][..]),
        ("Modify", &["Move", "Rotate", "Scale", "Offset"][..]),
    ] {
        let before: Vec<usize> = items.iter().map(|i| shown(&h, i)).collect();
        open_menu(&mut h, menu);
        for (item, before) in items.iter().zip(before) {
            assert!(shown(&h, item) > before, "{menu} → {item}");
        }
        save_png(
            &format!("drift_smoke_{}_menu.png", menu.to_lowercase()),
            &mut h,
        );
        h.key_press(Key::Escape);
        h.run();
    }
}

/// How many widgets show `item`, alone or with its keys after it ("Move  G").
fn shown(h: &Harness<'_, DriftApp>, item: &str) -> usize {
    h.query_all_by(|n| {
        n.label().is_some_and(|l| {
            l == item
                || l.strip_prefix(item)
                    .is_some_and(|keys| keys.starts_with(' '))
        })
    })
    .count()
}

#[test]
fn smoke_commands_repeat_last_and_thin_lines() {
    let Some(mut h) = harness() else { return };
    // L, Esc, then Enter: the line tool again.
    h.key_press(Key::L);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Line, "repeated");
    h.key_press(Key::Escape);
    h.run();
    // Shift+Alt+W toggles thin lines, both ways.
    let thin = h.state().thin_lines;
    let shift_alt = EguiModifiers {
        shift: true,
        alt: true,
        ..EguiModifiers::NONE
    };
    h.key_press_modifiers(shift_alt, Key::W);
    h.run();
    assert_eq!(h.state().thin_lines, !thin);
    h.key_press_modifiers(shift_alt, Key::W);
    h.run();
    assert_eq!(h.state().thin_lines, thin);
}

#[test]
fn smoke_commands_dialogs_keep_canvas_keys_and_esc_closes_them() {
    let Some(mut h) = harness() else { return };
    picked_line(&mut h);
    // Preferences (Ctrl+,) over the canvas: Delete and Ctrl+Z wait.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().show_settings, "Preferences open");
    h.key_press(Key::Delete);
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(lines(&h), 1, "the drawing is left alone");
    // Esc closes the dialog, and only that: the line stays picked.
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().show_settings);
    assert_eq!(h.state().editor.selection.len(), 1);
    // With it gone, Delete acts again.
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(lines(&h), 0);
}
