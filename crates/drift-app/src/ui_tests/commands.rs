//! The command registry in the app: every command in the palette, every menu item from the
//! registry, the Draw / Modify / Annotate menus, AutoCAD names in the palette, and Enter or
//! Space repeating the last command (but not while typing, in a dialog, or mid-tool).

use std::collections::BTreeSet;

use egui_kittest::kittest::NodeT as _;

use crate::commands::{Command, registry};

use super::*;

/// The labels of every button and check box showing.
fn items(h: &Harness<'_, DriftApp>) -> BTreeSet<String> {
    h.query_all_by(|n| matches!(format!("{:?}", n.role()).as_str(), "Button" | "CheckBox"))
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

/// Whether a menu item's accessible label is `name` (the label has its keys after it:
/// "Undo  Ctrl+Z").
fn is_item(label: &str, name: &str) -> bool {
    label == name
        || label
            .strip_prefix(name)
            .is_some_and(|keys| keys.starts_with(' '))
}

fn has_item(h: &Harness<'_, DriftApp>, name: &str) -> bool {
    h.query_all_by(|n| n.label().is_some_and(|l| is_item(&l, name)))
        .next()
        .is_some()
}

fn press(h: &mut Harness<'_, DriftApp>, key: Key) {
    h.key_press(key);
    h.run();
    h.run();
}

/// A line in view, picked.
fn picked_line(h: &mut Harness<'_, DriftApp>) {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(40.0, 10.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.selection = [line].into_iter().collect();
    h.run();
}

#[test]
fn every_registry_command_is_in_the_palette() {
    let Some(mut h) = harness() else { return };
    let entries = h.state_mut().palette_entries();
    for spec in registry().iter().filter(|s| !s.when.hidden()) {
        assert!(
            entries.iter().any(|e| e.command == spec.command),
            "{} isn't in the palette",
            spec.id
        );
    }
}

/// Each menu's buttons and check boxes are the registry's commands, apart from submenus and
/// the few things that aren't commands (recent files, a setting, the appearance switch).
#[test]
fn every_menu_item_comes_from_the_registry() {
    let Some(mut h) = harness() else { return };
    picked_line(&mut h);
    let labels: BTreeSet<String> = {
        let app = h.state();
        registry().iter().map(|s| app.menu_label(s)).collect()
    };
    let not_commands = [
        "Open recent",
        "Export",
        "Panels",
        "Arrange",
        "Dimensions and notes",
        "System",
        "Dark",
        "Light",
    ];
    let menus = ["File", "Edit", "View", "Draw", "Modify", "Annotate", "Help"];
    for menu in menus {
        let before = items(&h);
        // (The first "View" is the menu; the toolbar has a View tool.)
        h.get_all_by_label(menu).next().unwrap().click();
        h.run();
        h.run();
        // Open the submenus too.
        for sub in ["Export", "Panels", "Arrange"] {
            if let Some(node) = h.query_by_label(sub) {
                node.hover();
                h.run();
                h.run();
            }
        }
        let shown: Vec<String> = items(&h).difference(&before).cloned().collect();
        assert!(!shown.is_empty(), "the {menu} menu didn't open");
        for label in shown {
            let ok = labels.iter().any(|l| is_item(&label, l))
                || not_commands.iter().any(|n| is_item(&label, n))
                || label.to_lowercase().contains("update");
            assert!(ok, "{menu} → {label} isn't a registry command");
        }
        press(&mut h, Key::Escape);
    }
}

#[test]
fn modify_and_view_menus_have_the_new_commands() {
    let Some(mut h) = harness() else { return };
    picked_line(&mut h);
    h.get_by_label("Modify").click();
    h.run();
    h.run();
    for label in [
        "Move",
        "Rotate",
        "Scale",
        "Mirror",
        "Offset",
        "Trim",
        "Extend",
        "Fillet",
        "Chamfer",
        "Linear pattern…",
        "Circular pattern…",
    ] {
        assert!(has_item(&h, label), "Modify → {label}");
    }
    save_png("drift_ui_modify_menu.png", &mut h);
    press(&mut h, Key::Escape);
    h.get_all_by_label("View").next().unwrap().click();
    h.run();
    assert!(has_item(&h, "Zoom to selection"));
    press(&mut h, Key::Escape);
    // Draw lists the drawing tools; picking one takes it.
    h.get_by_label("Draw").click();
    h.run();
    h.get_by(|n| n.label().is_some_and(|l| l.starts_with("Polygon ")))
        .click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Polygon);
}

#[test]
fn the_palette_takes_autocad_names_and_runs_modify_commands() {
    let Some(mut h) = harness() else { return };
    picked_line(&mut h);
    // RO is Rotate; "rotate" no longer finds "Report a problem".
    h.state_mut().open_palette();
    h.run();
    h.event(Event::Text("rotate".into()));
    h.run();
    h.run();
    assert!(
        h.query_by_label("Report a problem or ask for a feature")
            .is_none()
    );
    press(&mut h, Key::Escape);
    let best = |h: &mut Harness<'_, DriftApp>, q: &str| {
        h.state_mut()
            .palette_matches(q)
            .first()
            .map(|e| e.command)
            .unwrap()
    };
    assert_eq!(best(&mut h, "RO"), Command::Rotate);
    assert_eq!(best(&mut h, "move"), Command::Move);
    assert_eq!(best(&mut h, "m"), Command::Tool(ToolKind::Mirror));
    assert_eq!(best(&mut h, "AR"), Command::Pattern(false));
    // MOVE grabs the picked line (with the pointer last on the canvas, as it would be).
    let over = screen(&h, DVec2::new(20.0, 5.0));
    h.event(Event::PointerMoved(over));
    h.run();
    h.state_mut().open_palette();
    h.run();
    h.event(Event::Text("MOVE".into()));
    h.run();
    press(&mut h, Key::Enter);
    assert!(h.state().tool.is_modal(), "Move grabs the selection");
    press(&mut h, Key::Escape);
    // Linear pattern opens the Inspector's Pattern section.
    let ctx = h.ctx.clone();
    h.state_mut().run_command(&ctx, Command::Pattern(false));
    h.run();
    h.run();
    assert!(h.query_by_label("Make linear pattern").is_some());
}

#[test]
fn enter_or_space_repeats_the_last_command() {
    let Some(mut h) = harness() else { return };
    // Nothing done yet: Enter does nothing.
    press(&mut h, Key::Enter);
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    // L, then Esc back to Select: Enter takes the line tool again, and so does Space.
    press(&mut h, Key::L);
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    press(&mut h, Key::Escape);
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    press(&mut h, Key::Enter);
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    press(&mut h, Key::Escape);
    press(&mut h, Key::Space);
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    // With a tool in hand, Enter finishes it rather than repeating anything.
    press(&mut h, Key::R);
    assert_eq!(h.state().tool.kind(), ToolKind::Rectangle);
    press(&mut h, Key::L);
    press(&mut h, Key::Enter);
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    // Commands from a menu count too.
    press(&mut h, Key::Escape);
    picked_line(&mut h);
    h.get_by_label("Modify").click();
    h.run();
    h.get_by_label("Linear pattern…").click();
    h.run();
    assert_eq!(h.state().last_command, Some(Command::Pattern(false)));
    h.state_mut().editor.selection.clear();
    h.state_mut().last_command = Some(Command::Tool(ToolKind::Offset));
    press(&mut h, Key::Enter);
    assert_eq!(h.state().tool.kind(), ToolKind::Offset);
    press(&mut h, Key::Escape);
    // Not while a dialog is open (#152's gate) …
    h.state_mut().show_settings = true;
    h.run();
    press(&mut h, Key::Enter);
    press(&mut h, Key::Space);
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    h.state_mut().show_settings = false;
    h.run();
    // … nor while typing into the palette.
    h.state_mut().open_palette();
    h.run();
    h.event(Event::Text("zzzz".into()));
    h.run();
    press(&mut h, Key::Space);
    press(&mut h, Key::Enter);
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert!(h.state().palette.is_some());
    press(&mut h, Key::Escape);
    // A command that can't run now says why instead (Move with nothing picked).
    h.state_mut().last_command = Some(Command::Move);
    press(&mut h, Key::Enter);
    assert!(!h.state().tool.is_modal());
}
