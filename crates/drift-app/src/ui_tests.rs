//! End-to-end UI tests: drive the real [`DriftApp`] headlessly with egui_kittest, rendering
//! through wgpu. Skipped when no graphics adapter is available.

use drift_core::{DVec2, EntityKind};
use eframe::egui::{Event, Key, Modifiers as EguiModifiers, PointerButton, Pos2};
use eframe::egui_wgpu::wgpu;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

use crate::DriftApp;
use crate::overlay::ScreenMap;
use crate::tools::ToolKind;

fn gpu_available() -> bool {
    let instance = wgpu::Instance::default();
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).is_ok()
}

fn harness() -> Option<Harness<'static, DriftApp>> {
    if !gpu_available() {
        eprintln!("no GPU adapter available; skipping UI test");
        return None;
    }
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .wgpu()
        .build_eframe(|cc| DriftApp::new(cc));
    harness.run();
    Some(harness)
}

/// Screen position of a world point on the canvas.
fn screen(h: &Harness<'_, DriftApp>, p: DVec2) -> Pos2 {
    let app = h.state();
    ScreenMap {
        rect: app.canvas_rect,
        camera: app.camera,
    }
    .to_screen(p)
}

fn button(h: &Harness<'_, DriftApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: EguiModifiers::NONE,
    });
}

/// Move, press and release at a world position, one frame per event like a real user.
fn click_world(h: &mut Harness<'_, DriftApp>, p: DVec2) {
    let pos = screen(h, p);
    h.hover_at(pos);
    h.run();
    button(h, pos, true);
    h.run();
    button(h, pos, false);
    h.run();
}

fn count(h: &Harness<'_, DriftApp>, pred: impl Fn(&EntityKind) -> bool) -> usize {
    h.state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| pred(&e.kind))
        .count()
}

fn lines(h: &Harness<'_, DriftApp>) -> usize {
    count(h, |k| matches!(k, EntityKind::Line { .. }))
}

fn save_png(name: &str, h: &mut Harness<'_, DriftApp>) {
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    std::fs::create_dir_all(&dir).expect("create target/tmp");
    let path = dir.join(name);
    image.save(&path).expect("save png");
    eprintln!("wrote {}", path.display());
}

#[test]
fn toolbar_switches_tools_and_highlights_immediately() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Rectangle  R").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Rectangle);
    h.key_press(Key::L);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
}

#[test]
fn draw_select_delete_undo() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Rectangle  R").click();
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -20.0));
    click_world(&mut h, DVec2::new(40.0, 30.0));
    assert_eq!(lines(&h), 4, "rectangle drawn");
    let bounds = h.state().editor.bounds();
    assert!((bounds.min - DVec2::new(-40.0, -20.0)).length() < 0.5);
    assert!((bounds.max - DVec2::new(40.0, 30.0)).length() < 0.5);

    // Circle with its centre snapped exactly onto the rectangle's corner.
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::new(40.3, 30.2));
    click_world(&mut h, DVec2::new(50.0, 30.0));
    let circle_center = h.state().editor.doc.iter().find_map(|(_, e)| match e.kind {
        EntityKind::Circle { center, .. } => Some(center),
        _ => None,
    });
    let center = h.state().editor.doc.point(circle_center.unwrap()).unwrap();
    assert_eq!(center, bounds.max, "snapped to the corner exactly");

    save_png("drift_ui_drawn.png", &mut h);

    // Select the bottom edge and delete it.
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(0.0, -20.0));
    assert_eq!(h.state().editor.selection.len(), 1);
    save_png("drift_ui_selected.png", &mut h);
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(lines(&h), 3);

    // Undo restores it.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(lines(&h), 4);
}

#[test]
fn click_uses_release_position_when_events_are_batched() {
    // Regression: when a slow frame batches the release with later pointer motion, the click
    // must land where the button was released, not where the pointer ended up.
    let Some(mut h) = harness() else { return };
    h.key_press(Key::P);
    h.run();
    let target = DVec2::new(12.5, -7.5);
    let pos = screen(&h, target);
    let later = screen(&h, DVec2::new(60.0, 40.0));
    h.hover_at(pos);
    button(&h, pos, true);
    button(&h, pos, false);
    h.hover_at(later);
    h.run();
    let (_, point) = h
        .state()
        .editor
        .doc
        .iter()
        .next()
        .expect("a point was placed");
    let EntityKind::Point(at) = point.kind else {
        panic!("expected a point")
    };
    assert!((at - target).length() < 0.2, "placed at {at}");
}

#[test]
fn example_opens_and_fits() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    assert!(h.state().editor.doc.len() > 20);
    assert!(h.state().is_dirty(), "examples are unsaved documents");
    save_png("drift_ui_example.png", &mut h);
}

#[derive(Default)]
struct MemoryStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

#[test]
fn session_restores_document_and_unsaved_state() {
    let Some(mut h) = harness() else { return };
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(25.0, 0.0));
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);

    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert_eq!(fresh.state().editor.doc, h.state().editor.doc);
    assert!(
        fresh.state().is_dirty(),
        "unsaved work stays marked unsaved"
    );
    assert_eq!(fresh.state().file.name, crate::UNTITLED);
}

#[test]
fn constrain_a_line_from_the_properties_panel() {
    use eframe::egui::accesskit::Role;

    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, -10.0));
    click_world(&mut h, DVec2::new(20.0, 15.0));
    h.key_press(Key::Escape); // finish the chain
    h.run();
    h.key_press(Key::Escape); // back to Select
    h.run();
    click_world(&mut h, DVec2::new(-5.0, 2.5)); // middle of the line
    assert_eq!(h.state().editor.selection.len(), 1);

    h.get_by_role_and_label(Role::Button, "Horizontal").click();
    h.run();
    assert_eq!(h.state().editor.doc.constraint_count(), 1);

    h.get_by_role_and_label(Role::Button, "Length").click();
    h.run();
    assert_eq!(h.state().editor.doc.constraint_count(), 2);
    // The new dimension's field has focus: type a value and press Enter.
    h.event(Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.event(Event::Text("42".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();

    let app = h.state();
    let (_, line) = app
        .editor
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .unwrap();
    let EntityKind::Line { a, b } = line.kind else {
        unreachable!()
    };
    let (pa, pb) = (
        app.editor.doc.point(a).unwrap(),
        app.editor.doc.point(b).unwrap(),
    );
    assert!(
        (pa.distance(pb) - 42.0).abs() < 1e-9,
        "length {}",
        pa.distance(pb)
    );
    assert!((pa.y - pb.y).abs() < 1e-12);
    // Two DOF left: the line can still slide.
    assert_eq!(app.editor.analysis().dof, Some(2));
    save_png("drift_ui_constrained.png", &mut h);
}
