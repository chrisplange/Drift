//! End-to-end UI tests: drive the real [`DriftApp`] headlessly with egui_kittest, rendering
//! through wgpu. Skipped when no graphics adapter is available.

use drift_core::{DVec2, EntityKind};
use eframe::egui::{self, Event, Key, Modifiers as EguiModifiers, PointerButton, Pos2};
use eframe::egui_wgpu::wgpu;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

use crate::DriftApp;
use crate::dock::{Panel, Place, Side};
use crate::overlay::ScreenMap;
use crate::tools::{Overlay, ToolKind};

/// A visual audit of every surface, in both themes.
mod audit;
/// The sheets and views workflow, step by step.
mod audit_sheets;
/// Screenshots for the user guide.
mod docs;
/// One way in for drawings: Import…, its presets and dropped files.
mod import;
/// The live link: assistants editing the open drawing.
#[cfg(not(target_arch = "wasm32"))]
mod link;
/// A filled shape as one object in the Inspector: Stroke and Fill.
#[cfg(not(feature = "demo"))]
mod shapes;
/// Print matches the screen: one stacking order for the canvas and the exports.
#[cfg(not(feature = "demo"))]
mod stacking;
/// The update notice and Preferences → Updates.
#[cfg(not(target_arch = "wasm32"))]
mod updates;

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

/// The views on a sheet, with their model views.
fn placed(h: &Harness<'_, DriftApp>, id: drift_core::SheetId) -> Vec<drift_core::PlacedView> {
    let doc = &h.state().editor.doc;
    doc.placed_views(doc.sheet(id).expect("the sheet"))
}

/// Change a sheet's view `index` in one undo step (`label`).
fn set_placed(
    h: &mut Harness<'_, DriftApp>,
    id: drift_core::SheetId,
    index: usize,
    view: &drift_core::PlacedView,
    label: &str,
) {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    ed.doc.set_placed_view(id, index, view);
    ed.commit(label);
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
    h.get_by_label("Rectangle").click();
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
    h.get_by_label("Rectangle").click();
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
    click_world(&mut h, DVec2::new(-20.0, -20.0));
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
fn canvas_keys_wait_while_a_dialog_is_open_and_esc_closes_it() {
    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 0.0));
    click_world(&mut h, DVec2::new(30.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(lines(&h), 1, "line drawn");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    h.run();
    assert_eq!(h.state().editor.selection.len(), 1, "line picked");

    type Open = fn(&mut DriftApp);
    type IsOpen = fn(&DriftApp) -> bool;
    let dialogs: [(&str, Open, IsOpen); 7] = [
        (
            "Preferences",
            |a| a.show_settings = true,
            |a| a.show_settings,
        ),
        (
            "Drawing settings",
            |a| a.show_drawing_settings = true,
            |a| a.show_drawing_settings,
        ),
        ("About", |a| a.show_about = true, |a| a.show_about),
        (
            "Keyboard shortcuts",
            |a| a.show_shortcuts = true,
            |a| a.show_shortcuts,
        ),
        (
            "Check sketch",
            |a| a.open_sketch_check(),
            |a| a.sketch_check.is_some(),
        ),
        (
            "Import report",
            |a| {
                a.import_report = Some(crate::app::sheets_ui::ImportSummary::default());
            },
            |a| a.import_report.is_some(),
        ),
        (
            "Drop dialog",
            |a| a.dropped = Some("elsewhere.dxf".into()),
            |a| a.dropped.is_some(),
        ),
    ];
    for (name, open, is_open) in dialogs {
        open(h.state_mut());
        h.run();
        h.key_press(Key::Delete);
        h.run();
        h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
        h.run();
        assert_eq!(lines(&h), 1, "{name}: Delete and Ctrl+Z wait");
        assert!(is_open(h.state()), "{name}: still open");
        h.key_press(Key::Escape);
        h.run();
        assert!(!is_open(h.state()), "{name}: Esc closes it");
        assert_eq!(
            h.state().editor.selection.len(),
            1,
            "{name}: the same Esc keeps the pick"
        );
        assert_eq!(h.state().tool.kind(), ToolKind::Select);
    }
    // With the dialogs gone, Delete acts again.
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(lines(&h), 0);
}

#[test]
fn click_uses_release_position_when_events_are_batched() {
    // Regression: when a slow frame batches the release with later pointer motion, the click
    // must land where the button was released, not where the pointer ended up.
    let Some(mut h) = harness() else { return };
    h.key_press(Key::V);
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
pub(crate) struct MemoryStorage(std::collections::HashMap<String, String>);

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
    click_world(&mut h, DVec2::new(5.0, 7.5)); // on the line, away from its middle
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

// ---- M3: typed input, Smart Dimension, on-canvas annotations ----

/// Add view on the sheet shown: the canvas goes to the model with the View tool, where the
/// view's crop is dragged out from `a` to `b` (world points); the view then goes on the
/// sheet, which is shown again.
fn add_view_by_region(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) {
    h.get_all_by_label("Add view")
        .next()
        .expect("Add view")
        .click();
    h.run();
    h.run();
    assert!(!h.state().sheet_preview, "the model, to drag the crop on");
    assert_eq!(h.state().tool.kind(), ToolKind::View);
    let (pa, pb) = (screen(h, a), screen(h, b));
    h.hover_at(pa);
    h.run();
    button(h, pa, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(pa + (pb - pa) * (k as f32 / 5.0));
        h.run();
    }
    button(h, pb, false);
    h.run();
    h.run();
}

fn type_text(h: &mut Harness<'_, DriftApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run();
}

fn screen_map(h: &Harness<'_, DriftApp>) -> crate::overlay::ScreenMap {
    let app = h.state();
    crate::overlay::ScreenMap {
        rect: app.canvas_rect,
        camera: app.camera,
    }
}

fn the_line(h: &Harness<'_, DriftApp>) -> (DVec2, DVec2) {
    let doc = &h.state().editor.doc;
    let (_, line) = doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .expect("a line");
    let EntityKind::Line { a, b } = line.kind else {
        unreachable!()
    };
    (doc.point(a).unwrap(), doc.point(b).unwrap())
}

/// Layout of the first dimension, as the canvas draws it.
fn first_dimension(h: &Harness<'_, DriftApp>) -> crate::annotations::DimLayout {
    let app = h.state();
    let painter = egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), app.canvas_rect);
    app.layout_dimensions(&painter, &screen_map(h))
        .into_iter()
        .next()
        .expect("a dimension")
}

#[test]
fn type_a_start_point_and_a_length() {
    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    h.hover_at(screen(&h, DVec2::new(30.0, 20.0)));
    h.run();
    type_text(&mut h, "10");
    h.key_press(Key::Tab);
    h.run();
    type_text(&mut h, "5");
    h.key_press(Key::Enter);
    h.run();
    // Now type a length; the pointer (to the right) gives the direction.
    type_text(&mut h, "50");
    h.key_press(Key::Enter);
    h.run();
    let (a, b) = the_line(&h);
    assert_eq!(a, DVec2::new(10.0, 5.0));
    assert!((a.distance(b) - 50.0).abs() < 1e-9);
    assert!(h.state().editor.doc.constraints().any(|(_, c)| matches!(
        c,
        drift_core::Constraint::Distance { value, .. } if *value == 50.0
    )));
}

#[test]
fn smart_dimension_place_type_drag_and_delete_relation() {
    use crate::annotations::layout_glyphs;

    let Some(mut h) = harness() else { return };
    // A near-horizontal line gets an automatic horizontal relation.
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-20.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 0.5));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().editor.doc.constraint_count(), 1);

    // Smart Dimension: click the line, place above, type the value.
    h.key_press(Key::D);
    h.run();
    click_world(&mut h, DVec2::new(-10.0, 0.125)); // on the line, away from its midpoint
    click_world(&mut h, DVec2::new(5.0, 15.0));
    assert_eq!(h.state().editor.doc.constraint_count(), 2);
    assert!(h.state().dimension_edit.is_some(), "value editor opened");
    type_text(&mut h, "30");
    h.key_press(Key::Enter);
    h.run();
    let (a, b) = the_line(&h);
    assert!(
        (a.distance(b) - 30.0).abs() < 1e-9,
        "length {}",
        a.distance(b)
    );
    save_png("drift_ui_dimensioned.png", &mut h);

    // Drag the dimension label 60 px to the right: one undo step.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let dim = first_dimension(&h);
    let id = dim.id.unwrap();
    let before = h.state().editor.doc.placement(id);
    let from = dim.text_rect.center();
    let to = from + egui::vec2(60.0, 0.0);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    h.hover_at(from + egui::vec2(20.0, 0.0));
    h.run();
    h.hover_at(to);
    h.run();
    button(&h, to, false);
    h.run();
    let after = h.state().editor.doc.placement(id).unwrap();
    assert!(Some(after) != before, "label moved");
    assert!((first_dimension(&h).text_rect.center().x - to.x).abs() < 2.0);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().editor.doc.placement(id), before);

    // A double click just outside the corner of the text still edits it, with the select
    // tool and with the dimension tool. (The harness's frames are a quarter of a second
    // apart: allow two of them between clicks.)
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    for tool in [Key::S, Key::D] {
        h.key_press(tool);
        h.run();
        let rect = first_dimension(&h).text_rect;
        let near = rect.right_top() + egui::vec2(4.0, -4.0);
        h.hover_at(near);
        h.run();
        for _ in 0..2 {
            button(&h, near, true);
            h.step();
            button(&h, near, false);
            h.step();
        }
        h.run();
        assert_eq!(
            h.state().dimension_edit.as_ref().map(|e| e.id),
            Some(id),
            "{tool:?}"
        );
        assert_eq!(
            h.state().editor.doc.constraint_count(),
            2,
            "no new dimension"
        );
        h.key_press(Key::Escape);
        h.run();
        h.key_press(Key::Escape);
        h.run();
    }
    h.key_press(Key::S);
    h.run();

    // Pick the horizontal relation's glyph and delete it.
    let glyph = layout_glyphs(&h.state().editor.doc, &screen_map(&h))
        .into_iter()
        .next()
        .expect("a relation glyph");
    let at = glyph.rect.center();
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().selected_constraint, Some(glyph.id));
    save_png("drift_ui_relation_picked.png", &mut h);
    h.key_press(Key::Delete);
    h.run();
    assert!(h.state().editor.doc.constraint(glyph.id).is_none());
    assert_eq!(h.state().editor.doc.constraint_count(), 1);
}

#[test]
fn every_annotation_kind_renders() {
    use drift_core::Constraint as C;

    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let doc = &mut ed.doc;
        let p = [v(-60.0, -30.0), v(0.0, -30.0), v(0.0, 10.0), v(-60.0, 10.0)]
            .map(|q| doc.add_point(q));
        let l: Vec<_> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        let o = doc.add_point(v(20.0, -30.0));
        let tip = doc.add_point(v(50.0, -10.0));
        let slanted = doc.add_line(o, tip).unwrap();
        let base_end = doc.add_point(v(55.0, -30.0));
        let base = doc.add_line(o, base_end).unwrap();
        let c = doc.add_point(v(-30.0, 30.0));
        let circle = doc.add_circle(c, 8.0).unwrap();
        let ac = doc.add_point(v(30.0, 30.0));
        let s = doc.add_point(v(42.0, 30.0));
        let e = doc.add_point(v(30.0, 42.0));
        let arc = doc.add_arc(ac, s, e).unwrap();
        let q = doc.add_point(v(-30.0, -10.0));
        for constraint in [
            C::Horizontal { line: l[0] },
            C::Vertical { line: l[1] },
            C::Parallel { a: l[0], b: l[2] },
            C::Perpendicular { a: l[2], b: l[3] },
            C::Fix {
                point: p[0],
                at: v(-60.0, -30.0),
            },
            C::Distance {
                a: p[0],
                b: p[1],
                value: 60.0,
            },
            C::VerticalDistance {
                a: p[1],
                b: p[2],
                value: 40.0,
            },
            C::HorizontalDistance {
                a: p[2],
                b: o,
                value: 20.0,
            },
            C::PointLineDistance {
                point: q,
                line: l[0],
                value: 20.0,
            },
            C::Diameter {
                curve: circle,
                value: 16.0,
            },
            C::Radius {
                curve: arc,
                value: 12.0,
            },
            C::Angle {
                a: base,
                b: slanted,
                degrees: 33.690067525979785,
            },
            C::Equal { a: l[1], b: l[3] },
        ] {
            doc.add_constraint(constraint).unwrap();
        }
        ed.commit("Setup");
        let bounds = ed.bounds();
        h.state_mut().fit_target = Some(bounds);
    }
    h.run();
    assert!(h.state().editor.analysis().converged);
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    assert_eq!(
        h.state().layout_dimensions(&painter, &screen_map(&h)).len(),
        7
    );
    save_png("drift_ui_annotations.png", &mut h);
}

#[test]
fn pick_a_midpoint_and_constrain_an_end_point_to_it() {
    use eframe::egui::accesskit::Role;

    let Some(mut h) = harness() else { return };
    h.state_mut().auto_relations = false; // draw freely
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, 0.0));
    click_world(&mut h, DVec2::new(40.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(5.0, 20.0));
    click_world(&mut h, DVec2::new(30.0, 45.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();

    // Hover then click the first line's midpoint.
    click_world(&mut h, DVec2::new(0.5, 0.3));
    assert_eq!(h.state().editor.selected_midpoints.len(), 1);
    // Shift-click the second line's start point.
    let at = screen(&h, DVec2::new(5.0, 20.0));
    h.hover_at(at);
    h.run();
    h.event_modifiers(
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    h.run();
    h.event_modifiers(
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    h.run();
    assert_eq!(h.state().editor.selection.len(), 1);
    save_png("drift_ui_midpoint_picked.png", &mut h);

    h.get_by_role_and_label(Role::Button, "Midpoint").click();
    h.run();
    let doc = &h.state().editor.doc;
    let Some(drift_core::Constraint::Midpoint { point, line }) = doc
        .constraints()
        .map(|(_, c)| *c)
        .find(|c| matches!(c, drift_core::Constraint::Midpoint { .. }))
    else {
        panic!("no midpoint relation")
    };
    let EntityKind::Line { a, b } = doc.get(line).unwrap().kind else {
        unreachable!()
    };
    let mid = (doc.point(a).unwrap() + doc.point(b).unwrap()) * 0.5;
    assert!((doc.point(point).unwrap() - mid).length() < 1e-9);
    save_png("drift_ui_midpoint_constrained.png", &mut h);
}

#[test]
fn smart_dimension_from_a_midpoint() {
    let Some(mut h) = harness() else { return };
    h.state_mut().auto_relations = false;
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, 0.0));
    click_world(&mut h, DVec2::new(40.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::V);
    h.run();
    click_world(&mut h, DVec2::new(15.0, 25.0));

    h.key_press(Key::D);
    h.run();
    click_world(&mut h, DVec2::new(0.4, 0.2)); // the line's midpoint
    click_world(&mut h, DVec2::new(15.0, 25.0)); // the point
    click_world(&mut h, DVec2::new(30.0, 12.0)); // label to the right: vertical distance
    assert!(h.state().dimension_edit.is_some());
    type_text(&mut h, "30");
    h.key_press(Key::Enter);
    h.run();

    let doc = &h.state().editor.doc;
    let Some((_, dim)) = doc.constraints().next() else {
        panic!("no dimension")
    };
    let drift_core::Constraint::VerticalDistance { a, b, value } = *dim else {
        panic!("expected a vertical distance, got {dim:?}")
    };
    assert_eq!(value, 30.0);
    let (ma, pb) = (doc.ref_point(a).unwrap(), doc.ref_point(b).unwrap());
    assert!(((pb.y - ma.y) - 30.0).abs() < 1e-9);
    assert!(
        matches!(doc.get(a).unwrap().kind, EntityKind::Line { .. }),
        "measured from the midpoint"
    );
    save_png("drift_ui_midpoint_dimension.png", &mut h);
}

#[test]
fn curve_relations_and_driven_dimensions_render() {
    use drift_core::Constraint as C;
    use eframe::egui::accesskit::Role;

    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let hole;
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let doc = &mut ed.doc;
        // A slot: two lines tangent to two arcs at shared end points.
        let [lc, rc, lt, rt, rb, lb] = [
            v(-30.0, 0.0),
            v(30.0, 0.0),
            v(-30.0, 10.0),
            v(30.0, 10.0),
            v(30.0, -10.0),
            v(-30.0, -10.0),
        ]
        .map(|q| doc.add_point(q));
        let top = doc.add_line(lt, rt).unwrap();
        let bottom = doc.add_line(rb, lb).unwrap();
        let right = doc.add_arc(rc, rb, rt).unwrap();
        let left = doc.add_arc(lc, lt, lb).unwrap();
        let hc = doc.add_point(v(30.0, 0.0));
        hole = doc.add_circle(hc, 5.0).unwrap();
        let ext = [v(50.0, 10.0), v(70.0, 10.0)].map(|q| doc.add_point(q));
        let extension = doc.add_line(ext[0], ext[1]).unwrap();
        let axis_ends = [v(-50.0, 0.0), v(50.0, 0.0)].map(|q| doc.add_point(q));
        let axis = doc.add_line(axis_ends[0], axis_ends[1]).unwrap();
        let mirrored = [v(-10.0, 25.0), v(-10.0, -25.0)].map(|q| doc.add_point(q));
        let on_arc = doc.add_point(v(-30.0 - 50f64.sqrt(), 50f64.sqrt()));
        for constraint in [
            C::Horizontal { line: top },
            C::Tangent { a: top, b: right },
            C::Tangent { a: top, b: left },
            C::Tangent {
                a: bottom,
                b: right,
            },
            C::Tangent { a: bottom, b: left },
            C::Concentric { a: right, b: hole },
            C::Collinear {
                a: top,
                b: extension,
            },
            C::Symmetric {
                a: mirrored[0],
                b: mirrored[1],
                axis,
            },
            C::PointOnCurve {
                point: on_arc,
                curve: left,
            },
            C::Diameter {
                curve: hole,
                value: 10.0,
            },
        ] {
            doc.add_constraint(constraint).unwrap();
        }
        ed.commit("Setup");
        ed.add_driven_dimension(
            C::Distance {
                a: lc,
                b: rc,
                value: 0.0,
            },
            None,
        )
        .unwrap();
        let bounds = ed.bounds();
        h.state_mut().fit_target = Some(bounds);
    }
    h.run();
    assert!(h.state().editor.analysis().converged);
    assert!(h.state().editor.analysis().conflicts.is_empty());
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let dims = h.state().layout_dimensions(&painter, &screen_map(&h));
    let driven: Vec<_> = dims.iter().filter(|d| d.driven).collect();
    assert_eq!(driven.len(), 1);
    assert_eq!(driven[0].label.plain(), "(60)");
    save_png("drift_ui_curve_relations.png", &mut h);

    // A radius on the dimensioned hole over-defines it: the prompt offers a driven one.
    let radius = C::Radius {
        curve: hole,
        value: 5.0,
    };
    let refused = h.state_mut().editor.add_dimension(radius, None);
    assert!(refused.is_err());
    h.run();
    save_png("drift_ui_driven_prompt.png", &mut h);
    h.get_by_role_and_label(Role::Button, "Add as driven")
        .click();
    h.run();
    h.run();
    let ed = &h.state().editor;
    assert!(ed.pending_driven.is_none());
    let driven = ed
        .doc
        .constraints()
        .filter(|(id, _)| ed.doc.is_driven(*id))
        .count();
    assert_eq!(driven, 2);
}

#[test]
fn a_dragged_dimension_snaps_level_with_another() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let horizontal = |h: &Harness<'_, DriftApp>| {
        let app = h.state();
        let painter =
            egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), app.canvas_rect);
        let mut dims: Vec<_> = app
            .layout_dimensions(&painter, &screen_map(h))
            .into_iter()
            .filter(|d| {
                matches!(
                    d.constraint,
                    drift_core::Constraint::HorizontalDistance { .. }
                )
            })
            .collect();
        dims.sort_by(|a, b| a.handle.y.total_cmp(&b.handle.y));
        dims
    };
    let dims = horizontal(&h);
    assert!(dims.len() >= 2, "the plate's stacked horizontal dimensions");
    // Drag the lower one's label up to 3 px short of the one above's line.
    let (above, below) = (&dims[dims.len() - 2], &dims[dims.len() - 1]);
    let (id, from) = (below.id.unwrap(), below.handle);
    let to = egui::pos2(from.x + 10.0, above.handle.y + 3.0);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    assert!(
        h.state().dim_guide.is_some(),
        "a guide shows what it snapped to"
    );
    save_png("drift_ui_dimension_snap.png", &mut h);
    button(&h, to, false);
    h.run();
    let moved = horizontal(&h)
        .into_iter()
        .find(|d| d.id == Some(id))
        .unwrap();
    assert!(
        (moved.handle.y - above.handle.y).abs() < 0.5,
        "level: {} vs {}",
        moved.handle.y,
        above.handle.y
    );
    assert!(h.state().dim_guide.is_none());
}

#[test]
fn extension_lines_start_past_geometry_and_their_grips_drag_and_reset() {
    let Some(mut h) = harness() else { return };
    // Two upright lines of different lengths, dimensioned between their bottom ends with the
    // dimension line above both.
    let (id, bottom) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [(0.0, 0.0), (0.0, 100.0), (40.0, 0.0), (40.0, 60.0)]
            .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
        ed.doc.add_line(p[0], p[1]).unwrap();
        ed.doc.add_line(p[2], p[3]).unwrap();
        let id = ed
            .doc
            .add_constraint(drift_core::Constraint::HorizontalDistance {
                a: p[0],
                b: p[2],
                value: 40.0,
            })
            .unwrap();
        ed.doc.set_placement(id, DVec2::new(0.0, 130.0)).unwrap();
        ed.commit("Lines");
        (id, p[0])
    };
    h.run();
    h.state_mut().fit_target = None;
    h.state_mut().camera = drift_render::Camera {
        center: DVec2::new(20.0, 70.0),
        scale: 3.0,
    };
    h.run();
    h.run();
    save_png("drift_ui_extension_lines_auto.png", &mut h);

    // Pick the dimension: grips sit where its extension lines start, past each line's top.
    let dim = first_dimension(&h);
    let at = dim.text_rect.center();
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().selected_constraint, Some(id));
    let grips = dim.shape.extension_grips();
    assert_eq!(grips.len(), 2);
    let gap = dim.style.gap;
    let tops = [
        screen(&h, DVec2::new(0.0, 100.0)),
        screen(&h, DVec2::new(40.0, 60.0)),
    ];
    for (g, top) in grips.iter().zip(tops) {
        assert!(g.1.distance(top - egui::vec2(0.0, gap)) < 0.1, "{g:?}");
    }
    save_png("drift_ui_extension_lines_picked.png", &mut h);

    // Drag the left one's start down the line: it is set by hand, as a distance from the
    // measured point, in one undo step.
    let to = screen(&h, DVec2::new(0.0, 40.0));
    h.hover_at(grips[0].1);
    h.run();
    button(&h, grips[0].1, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(grips[0].1 + (to - grips[0].1) * (k as f32 / 4.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    let start = h.state().editor.doc.extension_starts(id);
    assert!(
        start[0].is_some_and(|t| (t - 40.0).abs() < 0.05),
        "{start:?}"
    );
    assert_eq!(start[1], None);
    assert_eq!(h.state().selected_constraint, Some(id), "still picked");
    save_png("drift_ui_extension_lines_dragged.png", &mut h);
    // It keeps its distance from the measured point when the geometry moves.
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        ed.doc.set_point(bottom, DVec2::new(0.0, -10.0)).unwrap();
        ed.commit("Move");
    }
    h.run();
    let grip = first_dimension(&h).shape.extension_grips()[0].1;
    assert!(
        grip.distance(screen(&h, DVec2::new(0.0, 30.0))) < 0.1,
        "{grip:?}"
    );
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().editor.doc.extension_starts(id), [None, None]);
    h.key_press_modifiers(EguiModifiers::COMMAND | EguiModifiers::SHIFT, Key::Z);
    h.run();
    assert!(h.state().editor.doc.extension_starts(id)[0].is_some());

    // A double-click on its grip starts it automatically again.
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    let grip = first_dimension(&h).shape.extension_grips()[0].1;
    h.hover_at(grip);
    h.run();
    for _ in 0..2 {
        button(&h, grip, true);
        h.step();
        button(&h, grip, false);
        h.step();
    }
    h.run();
    assert_eq!(h.state().editor.doc.extension_starts(id), [None, None]);
    assert_eq!(h.state().selected_constraint, Some(id), "still picked");
    assert!(h.state().dimension_edit.is_none(), "not the value editor");
}

#[test]
fn drawing_tools_show_drifts_own_crosshair() {
    let Some(mut h) = harness() else { return };
    h.state_mut().crosshair.size = 60.0;
    h.state_mut().crosshair.weight = 2.0;
    h.key_press(Key::L);
    h.run();
    let at = h.state().canvas_rect.center() + egui::vec2(-150.0, 80.0);
    h.hover_at(at);
    h.run();
    // The system cursor is hidden; Drift draws its own.
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::None
    );
    save_png("drift_ui_crosshair.png", &mut h);
    // The select tool keeps the system's arrow.
    h.key_press(Key::Escape);
    h.run();
    h.hover_at(at + egui::vec2(1.0, 0.0));
    h.run();
    assert_ne!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::None
    );
}

#[test]
fn auto_relations_and_end_points_live_in_the_status_bar_with_keys() {
    let Some(mut h) = harness() else { return };
    // One switch each, in the status bar at the bottom (not the top bar).
    let bottom = h.state().canvas_rect.bottom();
    for name in ["Auto relations", "End points"] {
        let r = h.get_by_label(name).rect();
        assert!(r.top() > bottom, "{name} at {r:?}");
    }
    assert!(h.state().auto_relations);
    let both = EguiModifiers::SHIFT | EguiModifiers::ALT;
    h.key_press_modifiers(both, Key::R);
    h.run();
    assert!(!h.state().auto_relations);
    assert_eq!(
        h.state().tool.kind(),
        ToolKind::Select,
        "not the rectangle tool"
    );
    h.key_press_modifiers(both, Key::R);
    h.run();
    assert!(h.state().auto_relations);
    let shown = h.state().always_endpoints;
    h.key_press_modifiers(both, Key::V);
    h.run();
    assert_eq!(h.state().always_endpoints, !shown);
    assert_eq!(
        h.state().tool.kind(),
        ToolKind::Select,
        "not the point tool"
    );
}

/// Press at `from`, move to `to` in steps (a frame each), and let go.
fn drag_steps(h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2, steps: u32, shot: Option<&str>) {
    h.hover_at(from);
    h.run_steps(2);
    button(h, from, true);
    h.run_steps(2);
    // (A window being dragged repaints every frame: step rather than wait for it to settle.)
    for k in 1..=steps {
        h.hover_at(from + (to - from) * (k as f32 / steps as f32));
        h.run_steps(2);
    }
    if let Some(name) = shot {
        save_png(name, h);
    }
    button(h, to, false);
    // (A window or tooltip appearing animates for a few frames.)
    h.run_steps(8);
}

#[test]
fn a_tab_slides_along_its_bar() {
    let Some(mut h) = harness() else { return };
    let order = |h: &Harness<'_, DriftApp>| h.state().layout.dock(Side::Right).to_vec();
    assert_eq!(order(&h)[0], Panel::Inspector);
    // Drag the Inspector tab along the bar to just past the Sheets tab.
    let from = h.get_by_label("Inspector").rect().center();
    let sheets = h.get_by_label("Sheets").rect();
    let to = egui::pos2(sheets.center().x + 6.0, from.y);
    drag_steps(&mut h, from, to, 8, Some("drift_ui_tab_sliding.png"));
    assert_eq!(
        order(&h),
        [
            Panel::Parameters,
            Panel::Layers,
            Panel::Sheets,
            Panel::Inspector,
            Panel::Blocks
        ]
    );
    // Still docked, and in front.
    assert_eq!(h.state().layout.active[1], Some(Panel::Inspector));
}

#[test]
fn a_tab_nudged_out_of_its_dock_stays_docked() {
    let Some(mut h) = harness() else { return };
    let before = h.state().layout.clone();
    // 25 px onto the canvas from the dock's edge: not far enough to come away.
    let from = h.get_by_label("Layers").rect().center();
    let edge = h.state().canvas_rect.right();
    let down = from + egui::vec2(0.0, 120.0);
    let to = egui::pos2(edge - 25.0, down.y);
    h.hover_at(from);
    h.run_steps(2);
    button(&h, from, true);
    h.run_steps(2);
    // Straight down out of the tab bar, then left onto the canvas.
    for p in [
        from + egui::vec2(0.0, 40.0),
        down,
        (down + to.to_vec2()) / 2.0,
        to,
    ] {
        h.hover_at(p);
        h.run_steps(2);
    }
    button(&h, to, false);
    h.run_steps(8);
    assert_eq!(h.state().layout.floating, before.floating);
    assert_eq!(h.state().layout.dock(Side::Right), before.dock(Side::Right));
}

#[test]
fn floating_panels_stack_and_come_apart() {
    let Some(mut h) = harness() else { return };
    for p in [Panel::Layers, Panel::Blocks] {
        h.state_mut().layout.move_to(p, Place::Float, None);
    }
    h.run();
    h.run();
    // Drag the Blocks window by its header onto the lower part of the Layers panel.
    let header = h.state().header_rect(Panel::Blocks).unwrap();
    let layers = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(60.0, 0.0);
    let to = egui::pos2(layers.center().x, layers.top() + 300.0);
    drag_steps(&mut h, from, to, 10, Some("drift_ui_stacking.png"));
    assert_eq!(
        h.state().layout.floating,
        [vec![Panel::Layers, Panel::Blocks]]
    );
    h.run();
    save_png("drift_ui_stacked.png", &mut h);
    // Pull Blocks out of the stack by its header, onto the canvas: it floats on its own.
    let header = h.state().header_rect(Panel::Blocks).unwrap();
    let from = header.left_center() + egui::vec2(60.0, 0.0);
    let to = h.state().canvas_rect.left_center() + egui::vec2(120.0, 0.0);
    drag_steps(&mut h, from, to, 10, None);
    assert_eq!(
        h.state().layout.floating,
        [vec![Panel::Layers], vec![Panel::Blocks]]
    );
}

#[test]
fn a_floating_stack_stays_inside_a_short_canvas() {
    let Some(mut h) = harness() else { return };
    let stack = [Panel::Layers, Panel::Inspector, Panel::Parameters];
    h.state_mut().layout.move_to(stack[0], Place::Float, None);
    for w in stack.windows(2) {
        h.state_mut().layout.stack(w[1], w[0], true);
    }
    h.run_steps(4);
    // The window shrinks to a canvas shorter than the stack wants (and than it was).
    h.set_size(egui::vec2(1280.0, 380.0));
    h.run_steps(6);
    save_png("drift_ui_floating_short.png", &mut h);
    let canvas = h.state().canvas_rect;
    let (_, window) = h.state().floating_rects(Panel::Layers).unwrap();
    let header = h.state().header_rect(Panel::Layers).unwrap();
    assert!(
        window.top() >= canvas.top() - 0.5 && header.top() >= canvas.top() - 0.5,
        "window {window:?} or header {header:?} above the canvas {canvas:?}"
    );
    assert!(
        window.height() <= canvas.height() + 0.5,
        "{window:?} in {canvas:?}"
    );
    // And back to a tall window: still all there.
    h.set_size(egui::vec2(1280.0, 800.0));
    h.run_steps(6);
    let canvas = h.state().canvas_rect;
    let (_, window) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(window.top() >= canvas.top() - 0.5 && window.bottom() <= canvas.bottom() + 0.5);
}

#[test]
fn a_floating_stack_shares_its_height_by_content_and_keeps_dragged_sizes() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.state_mut()
        .layout
        .stack(Panel::Inspector, Panel::Layers, true);
    h.run_steps(6);
    save_png("drift_ui_stack_fit.png", &mut h);
    let (layers, _) = h.state().floating_rects(Panel::Layers).unwrap();
    let (inspector, _) = h.state().floating_rects(Panel::Inspector).unwrap();
    // The Layers panel takes what it needs (up to an even share), the Inspector the rest.
    assert!(
        layers.height() <= inspector.height() + 1.0,
        "layers {layers:?}, inspector {inspector:?}"
    );
    // Dragging the divider down gives Layers more, and the shares are saved.
    let from = egui::pos2(layers.center().x, layers.bottom() + 3.5);
    drag_steps(&mut h, from, from + egui::vec2(0.0, 60.0), 6, None);
    let (dragged, _) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (dragged.height() - layers.height() - 60.0).abs() < 2.0,
        "{layers:?} → {dragged:?}"
    );
    let prefs = h.state().layout.prefs();
    assert!(
        prefs
            .iter()
            .any(|p| p.starts_with("dock_weights=inspector:")),
        "{prefs:?}"
    );
    // The top header moves the whole stack.
    let header = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(80.0, 0.0);
    drag_steps(&mut h, from, from + egui::vec2(-200.0, 40.0), 8, None);
    assert_eq!(
        h.state().layout.floating,
        [vec![Panel::Layers, Panel::Inspector]]
    );
    let moved = h.state().header_rect(Panel::Layers).unwrap();
    assert!(
        (moved.left() - (header.left() - 200.0)).abs() < 2.0,
        "{header:?} → {moved:?}"
    );
}

#[test]
fn stacked_panels_fold_to_their_headers() {
    let Some(mut h) = harness() else { return };
    let stack = [Panel::Layers, Panel::Inspector, Panel::Parameters];
    h.state_mut().layout.move_to(stack[0], Place::Float, None);
    for w in stack.windows(2) {
        h.state_mut().layout.stack(w[1], w[0], true);
    }
    h.run_steps(6);
    let height = |h: &Harness<'_, DriftApp>, p| h.state().floating_rects(p).unwrap().0.height();
    let (_, window) = h.state().floating_rects(Panel::Layers).unwrap();
    let inspector = height(&h, Panel::Inspector);
    // Click the Parameters header (away from its buttons): it folds to just the header, and
    // the window keeps its size, the others sharing the room.
    let click = |h: &mut Harness<'_, DriftApp>, p| {
        let at = h.state().header_rect(p).unwrap().left_center() + egui::vec2(40.0, 0.0);
        h.hover_at(at);
        h.run_steps(2);
        button(h, at, true);
        h.run_steps(2);
        button(h, at, false);
        h.run_steps(6);
    };
    click(&mut h, Panel::Parameters);
    save_png("drift_ui_stack_folded.png", &mut h);
    assert!(h.state().layout.folded.contains(&Panel::Parameters));
    assert!((height(&h, Panel::Parameters) - 34.0).abs() < 0.5);
    assert!(height(&h, Panel::Inspector) > inspector + 50.0);
    let (_, after) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (after.height() - window.height()).abs() < 1.0,
        "{window:?} → {after:?}"
    );
    // The top one folds too, and then the last open one: the window shrinks to the three
    // headers, where it was.
    click(&mut h, Panel::Layers);
    click(&mut h, Panel::Inspector);
    assert_eq!(h.state().layout.folded, stack.into_iter().collect());
    let (_, shut) = h.state().floating_rects(Panel::Layers).unwrap();
    save_png("drift_ui_stack_all_folded.png", &mut h);
    assert!(
        (shut.height() - (3.0 * 34.0 + 2.0 * 7.0)).abs() < 12.0,
        "{shut:?}"
    );
    assert!(
        (shut.left_top() - window.left_top()).length() < 1.0,
        "{window:?} → {shut:?}"
    );
    // One opens again, and the window has its size back.
    click(&mut h, Panel::Parameters);
    assert!(!h.state().layout.folded.contains(&Panel::Parameters));
    let (_, again) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (again.height() - window.height()).abs() < 1.0,
        "{window:?} → {again:?}"
    );
    assert_eq!(
        h.state().layout.floating,
        [stack.to_vec()],
        "clicks don't move panels"
    );
}

#[test]
fn a_panel_floating_alone_folds_its_window_to_the_header() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(6);
    let window = |h: &Harness<'_, DriftApp>| h.state().floating_rects(Panel::Layers).unwrap().1;
    let open = window(&h);
    let click = |h: &mut Harness<'_, DriftApp>| {
        let at =
            h.state().header_rect(Panel::Layers).unwrap().left_center() + egui::vec2(40.0, 0.0);
        h.hover_at(at);
        h.run_steps(2);
        button(h, at, true);
        h.run_steps(2);
        button(h, at, false);
        h.run_steps(6);
    };
    // A click on its header folds it: the window shrinks to the header, in place.
    click(&mut h);
    assert!(h.state().layout.folded.contains(&Panel::Layers));
    let shut = window(&h);
    save_png("drift_ui_float_folded.png", &mut h);
    assert!(shut.height() < 50.0, "{shut:?}");
    assert!(
        (shut.left_top() - open.left_top()).length() < 1.0,
        "{open:?} → {shut:?}"
    );
    assert!((shut.width() - open.width()).abs() < 1.0);
    // Another opens it again at the size it had.
    click(&mut h);
    assert!(!h.state().layout.folded.contains(&Panel::Layers));
    let again = window(&h);
    assert!(
        (again.height() - open.height()).abs() < 1.0,
        "{open:?} → {again:?}"
    );
    assert!((again.left_top() - open.left_top()).length() < 1.0);
}

#[test]
fn a_folded_stack_opens_again_where_it_was() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.state_mut()
        .layout
        .stack(Panel::Inspector, Panel::Layers, true);
    h.run_steps(6);
    let window = |h: &Harness<'_, DriftApp>| h.state().floating_rects(Panel::Layers).unwrap().1;
    // Move the window by its top header, away from where it starts.
    let header = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(40.0, 0.0);
    drag_steps(&mut h, from, from + egui::vec2(-300.0, 60.0), 10, None);
    h.run_steps(6);
    let open = window(&h);
    let click = |h: &mut Harness<'_, DriftApp>, p| {
        let at = h.state().header_rect(p).unwrap().left_center() + egui::vec2(40.0, 0.0);
        h.hover_at(at);
        h.run_steps(2);
        button(h, at, true);
        h.run_steps(2);
        button(h, at, false);
        h.run_steps(6);
    };
    click(&mut h, Panel::Inspector);
    click(&mut h, Panel::Layers);
    let shut = window(&h);
    assert!(
        (shut.left_top() - open.left_top()).length() < 1.0,
        "{open:?} → {shut:?}"
    );
    click(&mut h, Panel::Layers);
    let again = window(&h);
    assert!(
        (again.left_top() - open.left_top()).length() < 1.0,
        "{open:?} → {again:?}"
    );
    // Moved while folded, it opens where it was moved to (kept inside the canvas, so moved
    // across: lower down, the open window would be nudged up to fit).
    click(&mut h, Panel::Layers);
    let header = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(40.0, 0.0);
    drag_steps(&mut h, from, from + egui::vec2(200.0, 0.0), 10, None);
    h.run_steps(6);
    let moved = window(&h);
    click(&mut h, Panel::Layers);
    let again = window(&h);
    assert!(
        (again.left_top() - moved.left_top()).length() < 1.0,
        "{moved:?} → {again:?}"
    );
}

#[test]
fn a_tab_dropped_on_another_docks_tabs_goes_there() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Sheets, Place::Dock(Side::Left), None);
    h.run();
    h.run();
    // The Layers tab from the right dock onto the left dock's tab bar, just before Sheets.
    let from = h.get_by_label("Layers").rect().center();
    let sheets = h.get_by_label("Sheets").rect();
    let to = egui::pos2(sheets.left() + 4.0, sheets.center().y);
    drag_steps(&mut h, from, to, 12, Some("drift_ui_tab_to_dock.png"));
    assert_eq!(
        h.state().layout.dock(Side::Left),
        [Panel::Layers, Panel::Sheets]
    );
}

#[test]
fn a_leaders_shoulder_and_curve_are_set_by_dragging() {
    use drift_core::{Anchor, Annotation, Leader, LeaderShape};
    let Some(mut h) = harness() else { return };
    let id = {
        let doc = &mut h.state_mut().editor.doc;
        doc.begin();
        let mut leader = Leader::to(Anchor::Free(DVec2::new(0.0, 0.0)));
        leader.shape = LeaderShape::Curved(0.2);
        let id = doc
            .add_annotation(Annotation::Note {
                text: "NOTE".into(),
                at: DVec2::new(40.0, 20.0),
                leaders: vec![leader],
                style: Default::default(),
            })
            .unwrap();
        let _ = doc.commit("Note");
        id
    };
    h.state_mut().editor.selected_annotation = Some(id);
    h.run();
    let leader_path = |h: &Harness<'_, DriftApp>| {
        let app = h.state();
        let painter =
            egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), app.canvas_rect);
        app.layout_notes(&painter, &screen_map(h))
            .into_iter()
            .find(|l| l.id == id)
            .unwrap()
            .leaders[0]
            .clone()
    };
    let leader = |h: &Harness<'_, DriftApp>| match h.state().editor.doc.annotation(id) {
        Some(Annotation::Note { leaders, .. }) => leaders[0],
        _ => panic!("a note"),
    };
    let drag = |h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2| {
        h.hover_at(from);
        h.run_steps(2);
        button(h, from, true);
        h.run_steps(2);
        for k in 1..=6 {
            h.hover_at(from + (to - from) * (k as f32 / 6.0));
            h.run_steps(2);
        }
        button(h, to, false);
        h.run_steps(4);
    };
    // The shoulder's grip, dragged 30 px further out: the shoulder is that much longer.
    let path = leader_path(&h);
    let grip = path.shoulder.expect("a shoulder grip");
    let out = (grip.x - path.landing.x).signum();
    let before = (grip.x - path.landing.x).abs();
    drag(&mut h, grip, grip + egui::vec2(30.0 * out, 0.0));
    let len = leader(&h).shoulder_len * f64::from(crate::annotations::PT_PER_MM);
    assert!(
        (len - f64::from(before + 30.0)).abs() < 1.5,
        "{len} vs {}",
        before + 30.0
    );
    // The curve's grip, dragged across the chord: it bows the other way.
    let path = leader_path(&h);
    let (mid, [a, b]) = path.curve.expect("a curve grip");
    let across = (mid - (a + (b - a) * 0.5)) * -2.0;
    drag(&mut h, mid, mid + across);
    let LeaderShape::Curved(bow) = leader(&h).shape else {
        panic!("still curved");
    };
    assert!(bow < -0.1, "bows the other way: {bow}");
}

#[test]
fn note_text_stays_in_its_box_whatever_the_alignment() {
    use crate::annotations::{AnnotationColors, AnnotationState, DimStyle, Recorded, Recorder};
    use drift_core::{Annotation, NoteFrame, NoteStyle, TextAlign};
    let Some(mut h) = harness() else { return };
    let id = {
        let doc = &mut h.state_mut().editor.doc;
        doc.begin();
        let id = doc
            .add_annotation(Annotation::Note {
                text: "SMOOTH FINISH
ALL OVER"
                    .into(),
                at: DVec2::new(10.0, 10.0),
                leaders: Vec::new(),
                style: Default::default(),
            })
            .unwrap();
        let _ = doc.commit("Note");
        id
    };
    for align in TextAlign::ALL {
        {
            let doc = &mut h.state_mut().editor.doc;
            doc.begin();
            doc.set_annotation(
                id,
                Annotation::Note {
                    text: "SMOOTH FINISH
ALL OVER"
                        .into(),
                    at: DVec2::new(10.0, 10.0),
                    leaders: Vec::new(),
                    style: NoteStyle {
                        align,
                        frame: NoteFrame::Rounded,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
            let _ = doc.commit("Align");
        }
        h.run();
        let app = h.state();
        let painter =
            egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), app.canvas_rect);
        let layout = app
            .layout_notes(&painter, &screen_map(&h))
            .into_iter()
            .find(|l| l.id == id)
            .unwrap();
        let rect = layout.text.as_ref().unwrap().rect;
        let recorder = Recorder::new(&painter);
        let style = DimStyle::of(&app.editor.doc.draft_style());
        crate::notes::paint_annotation(
            &recorder,
            &layout,
            AnnotationState::Normal,
            &AnnotationColors::new(true),
            &style,
        );
        let lines: Vec<(f32, String)> = recorder
            .take()
            .into_iter()
            .filter_map(|r| match r {
                Recorded::Text { baseline, text, .. } => Some((baseline.x, text)),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 2, "{align:?}");
        for (x, text) in &lines {
            assert!(
                *x >= rect.left() - 0.5 && *x <= rect.right(),
                "{align:?}: {text:?} starts at {x}, box {rect:?}"
            );
        }
        // The shorter line sits as aligned.
        let (long, short) = (lines[0].0, lines[1].0);
        match align {
            TextAlign::Left => assert!((short - long).abs() < 1.0, "{align:?}"),
            TextAlign::Center | TextAlign::Right => assert!(short > long + 5.0, "{align:?}"),
        }
    }
}

#[test]
fn notes_take_leaders_from_their_grips_and_lose_them_with_delete() {
    use drift_core::{Annotation, LeaderShape, NoteFrame};
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A note with a leader to the plate's bottom edge.
    h.get_by_label("Note").click();
    h.run();
    click_world(&mut h, DVec2::new(30.0, 0.0));
    click_world(&mut h, DVec2::new(-30.0, -25.0));
    h.event(Event::Text("TYP.".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.get_by_label("Select").click();
    h.run();
    let note = |h: &Harness<'_, DriftApp>| {
        let (id, a) = h
            .state()
            .editor
            .doc
            .annotations()
            .find(|(_, a)| matches!(a, Annotation::Note { .. }))
            .map(|(id, a)| (id, a.clone()))
            .unwrap();
        let Annotation::Note { leaders, .. } = a else {
            unreachable!()
        };
        (id, leaders)
    };
    let (id, leaders) = note(&h);
    assert_eq!(leaders.len(), 1);
    // Pick it; drag from the + on its right to further along the bottom edge.
    h.state_mut().editor.selected_annotation = Some(id);
    h.run();
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let layout = h
        .state()
        .layout_notes(&painter, &screen_map(&h))
        .into_iter()
        .find(|l| l.id == id)
        .unwrap();
    let text = layout.text.as_ref().unwrap().rect;
    let plus = egui::pos2(text.right() + 14.0, text.top() - 14.0 * 0.6);
    let corner = screen(&h, DVec2::new(90.0, 0.0));
    h.hover_at(plus);
    h.run_steps(2);
    button(&h, plus, true);
    h.run_steps(2);
    for k in 1..=8 {
        h.hover_at(plus + (corner - plus) * (k as f32 / 8.0));
        h.run_steps(2);
    }
    button(&h, corner, false);
    h.run_steps(4);
    let (_, leaders) = note(&h);
    assert_eq!(leaders.len(), 2, "a second leader from the + grip");
    assert!(
        matches!(leaders[1].to, drift_core::Anchor::Curve { .. }),
        "attached to the edge"
    );
    // Style it for the picture: a rounded frame, a mask, bold italic, a bent and a curved
    // leader with an open arrow.
    {
        let doc = &mut h.state_mut().editor.doc;
        let Some(Annotation::Note {
            text,
            at,
            mut leaders,
            mut style,
        }) = doc.annotation(id).cloned()
        else {
            unreachable!()
        };
        style.frame = NoteFrame::Rounded;
        style.mask = true;
        style.bold = true;
        style.italic = true;
        leaders[0].shape = LeaderShape::Bent(DVec2::new(-12.0, -6.0));
        leaders[1].shape = LeaderShape::Curved(0.25);
        leaders[1].end = drift_core::LeaderEnd::OpenArrow;
        doc.begin();
        doc.set_annotation(
            id,
            Annotation::Note {
                text,
                at,
                leaders,
                style,
            },
        )
        .unwrap();
        let _ = doc.commit("Style");
    }
    h.run();
    h.run();
    save_png("drift_ui_note_leaders.png", &mut h);
    // Click the second leader's line, then Delete: that leader goes, the text stays.
    let layout = h
        .state()
        .layout_notes(&painter, &screen_map(&h))
        .into_iter()
        .find(|l| l.id == id)
        .unwrap();
    let path = &layout.leaders[1].points;
    let on = path[path.len() / 2];
    let before = layout.text.as_ref().unwrap().rect;
    h.hover_at(on);
    h.run();
    button(&h, on, true);
    h.run();
    button(&h, on, false);
    h.run();
    assert_eq!(h.state().selected_leader, Some((id, 1)));
    h.key_press(Key::Delete);
    h.run();
    let (_, leaders) = note(&h);
    assert_eq!(leaders.len(), 1);
    assert!(
        h.state().editor.doc.annotation(id).is_some(),
        "the note stays"
    );
    let after = h
        .state()
        .layout_notes(&painter, &screen_map(&h))
        .into_iter()
        .find(|l| l.id == id)
        .unwrap()
        .text
        .unwrap()
        .rect;
    assert!((after.center() - before.center()).length() < 0.5);
}

#[test]
fn alt_c_draws_and_makes_construction_geometry() {
    use drift_core::LineKind;
    let Some(mut h) = harness() else { return };
    let style_of_newest = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        doc.iter()
            .filter(|(_, e)| !e.kind.is_point())
            .max_by_key(|(id, _)| *id)
            .map(|(_, e)| e.style)
    };
    h.key_press(Key::L);
    h.run();
    // The line tool's bar offers the line type; Alt+C switches it.
    assert!(h.query_by_label("Construction").is_some());
    h.key_press_modifiers(EguiModifiers::ALT, Key::C);
    h.run();
    assert!(h.state().editor.draw_construction);
    assert_eq!(h.state().tool.kind(), ToolKind::Line, "not the circle tool");
    click_world(&mut h, DVec2::new(-30.0, -10.0));
    click_world(&mut h, DVec2::new(10.0, -10.0));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(style_of_newest(&h), Some(LineKind::Construction));
    // Back to geometry from the bar.
    h.get_by_label("Geometry").click();
    h.run();
    assert!(!h.state().editor.draw_construction);
    click_world(&mut h, DVec2::new(-30.0, 20.0));
    click_world(&mut h, DVec2::new(10.0, 20.0));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(style_of_newest(&h), Some(LineKind::Solid));
    // With the select tool, Alt+C makes the picked line construction; X no longer does.
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    click_world(&mut h, DVec2::new(-20.0, 20.0));
    assert_eq!(h.state().editor.selection.len(), 1);
    h.key_press(Key::X);
    h.run();
    assert_eq!(style_of_newest(&h), Some(LineKind::Solid));
    h.key_press_modifiers(EguiModifiers::ALT, Key::C);
    h.run();
    assert_eq!(style_of_newest(&h), Some(LineKind::Construction));
}

#[test]
fn x_and_y_keys_lock_line_segments() {
    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, -10.0));
    h.key_press(Key::X);
    h.run();
    h.hover_at(screen(&h, DVec2::new(10.0, 4.0)));
    h.run();
    save_png("drift_ui_axis_lock.png", &mut h);
    click_world(&mut h, DVec2::new(10.0, 4.0));
    h.key_press(Key::Y);
    h.run();
    click_world(&mut h, DVec2::new(25.0, 20.0));
    h.key_press(Key::Escape);
    h.run();
    let doc = &h.state().editor.doc;
    let ends: Vec<_> = doc
        .iter()
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Line { a, b } => Some((doc.point(a).unwrap(), doc.point(b).unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 2);
    assert_eq!(ends[0].0.y, ends[0].1.y, "first segment horizontal");
    assert_eq!(ends[1].0.x, ends[1].1.x, "second segment vertical");
    // X went to the tool, not to the construction toggle.
    assert!(
        doc.iter()
            .all(|(_, e)| e.style == drift_core::LineKind::Solid)
    );
}

#[test]
fn g_moves_the_selection_with_typed_offsets_and_base_points() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let a = ed.doc.add_point(v(-20.0, 0.0));
        let b = ed.doc.add_point(v(0.0, 10.0));
        let line = ed.doc.add_line(a, b).unwrap();
        let c = ed.doc.add_point(v(15.0, -12.0));
        let target = ed.doc.add_point(v(30.0, 25.0));
        ed.doc.add_line(c, target).unwrap();
        ed.commit("Setup");
        ed.selection = [line].into();
    }
    h.run();
    let line_ends = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        let (_, e) = doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .unwrap();
        let EntityKind::Line { a, b } = e.kind else {
            unreachable!()
        };
        (doc.point(a).unwrap(), doc.point(b).unwrap())
    };

    // G, X, type 15, Enter: moved exactly 15 mm along X.
    h.hover_at(screen(&h, v(-5.0, 2.0)));
    h.run();
    h.key_press(Key::G);
    h.run();
    h.hover_at(screen(&h, v(3.0, 9.0)));
    h.run();
    h.key_press(Key::X);
    h.run();
    type_text(&mut h, "15");
    save_png("drift_ui_grab_typed.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(line_ends(&h), (v(-5.0, 0.0), v(15.0, 10.0)));

    // G, B on the line's top end, then onto the other line's end: lands exactly.
    h.key_press(Key::G);
    h.run();
    h.key_press(Key::B);
    h.run();
    click_world(&mut h, v(15.2, 10.1));
    h.hover_at(screen(&h, v(29.8, 25.1)));
    h.run();
    save_png("drift_ui_grab_base.png", &mut h);
    click_world(&mut h, v(29.8, 25.1));
    assert_eq!(line_ends(&h), (v(10.0, 15.0), v(30.0, 25.0)));

    // Right-click cancels; Shift+G still toggles grid snapping.
    h.key_press(Key::G);
    h.run();
    h.hover_at(screen(&h, v(0.0, -30.0)));
    h.run();
    let pos = screen(&h, v(0.0, -30.0));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
    assert_eq!(line_ends(&h), (v(10.0, 15.0), v(30.0, 25.0)));
    let grid = h.state().grid_snap;
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::G);
    h.run();
    assert_ne!(h.state().grid_snap, grid);
    // Shift+C switches between status colours and layer colours (not the circle tool).
    let status = h.state().status_colors;
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::C);
    h.run();
    assert_ne!(h.state().status_colors, status);
    assert_ne!(h.state().tool.kind(), ToolKind::Circle);

    // Shift+D duplicates in place and grabs the copy; a click drops it, the original stays.
    let lines = |h: &Harness<'_, DriftApp>| {
        h.state()
            .editor
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .count()
    };
    let before = lines(&h);
    h.hover_at(screen(&h, v(20.0, 20.0)));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    assert_eq!(lines(&h), before + 1);
    assert_ne!(h.state().tool.kind(), ToolKind::Dimension);
    click_world(&mut h, v(20.0, 0.0));
    assert_eq!(line_ends(&h), (v(10.0, 15.0), v(30.0, 25.0)));
    assert!(!h.state().tool.is_modal());
}

#[test]
fn a_cycles_arc_tools_and_switches_mid_polyline() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    for expected in [
        ToolKind::Arc,
        ToolKind::ThreePointArc,
        ToolKind::TangentArc,
        ToolKind::Arc,
    ] {
        h.key_press(Key::A);
        h.run();
        assert_eq!(h.state().tool.kind(), expected);
    }

    // Polyline, then A: a tangent arc from the last point, then L: lines again.
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, v(-40.0, 0.0));
    click_world(&mut h, v(0.0, 0.0));
    h.key_press(Key::A);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::TangentArc);
    h.hover_at(screen(&h, v(15.0, 15.0)));
    h.run();
    save_png("drift_ui_tangent_arc.png", &mut h);
    click_world(&mut h, v(15.0, 15.0));
    h.key_press(Key::L);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    click_world(&mut h, v(15.0, 40.0));
    h.key_press(Key::Escape);
    h.run();

    let doc = &h.state().editor.doc;
    let count = |pred: fn(&EntityKind) -> bool| doc.iter().filter(|(_, e)| pred(&e.kind)).count();
    assert_eq!(count(|k| matches!(k, EntityKind::Line { .. })), 2);
    assert_eq!(count(|k| matches!(k, EntityKind::Arc { .. })), 1);
    // One connected chain: 4 curve end points shared → 4 points plus the arc's centre.
    assert_eq!(count(EntityKind::is_point), 5);
    assert!(
        doc.constraints()
            .any(|(_, c)| matches!(c, drift_core::Constraint::Tangent { .. }))
    );
    assert!(h.state().editor.analysis().conflicts.is_empty());
    save_png("drift_ui_line_arc_line.png", &mut h);
}

#[test]
fn variables_drive_typed_dimensions_and_units_change_display() {
    use eframe::egui::accesskit::Role;

    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let w = h
        .state_mut()
        .editor
        .add_variable("w", "40", drift_core::Kind::Length)
        .unwrap();
    // Draw a line whose length is typed as the variable: `=` starts typing a name.
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, v(-20.0, 0.0));
    h.hover_at(screen(&h, v(10.0, 1.0)));
    h.run();
    type_text(&mut h, "=");
    type_text(&mut h, "w");
    save_png("drift_ui_typed_variable.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let (a, b) = the_line(&h);
    assert!((a.distance(b) - 40.0).abs() < 1e-9, "{}", a.distance(b));
    let linked = h
        .state()
        .editor
        .doc
        .expressions()
        .map(|(_, e)| e.to_owned())
        .collect::<Vec<_>>();
    assert_eq!(linked, ["w"]);

    // Changing the variable moves the line.
    h.state_mut().editor.set_variable(w, "2.5 in").unwrap();
    h.run();
    let (a, b) = the_line(&h);
    assert!((a.distance(b) - 63.5).abs() < 1e-9);

    // The Parameters tab, in architectural units, with free directions shown.
    h.state_mut()
        .editor
        .set_units(drift_core::Units::architectural());
    h.state_mut().params.show_free = true;
    h.get_by_role_and_label(Role::Button, "Parameters").click();
    h.run();
    h.run();
    let dims = first_dimension(&h);
    assert_eq!(dims.label.plain(), "Σ2 1/2\"");
    save_png("drift_ui_parameters.png", &mut h);

    // Variants: the other one shows as a ghost.
    let small = h.state_mut().editor.save_variant("Small");
    h.state_mut().editor.set_variable(w, "4 in").unwrap();
    h.state_mut().editor.save_variant("Large");
    h.state_mut().editor.apply_variant(small).unwrap();
    h.state_mut().params.show_ghosts = true;
    h.run();
    h.run();
    let (a, b) = the_line(&h);
    assert!((a.distance(b) - 63.5).abs() < 1e-9);
    save_png("drift_ui_variant_ghosts.png", &mut h);
}

#[test]
fn modify_tools_from_the_toolbar() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        for (a, b) in [
            (v(-30.0, 0.0), v(30.0, 0.0)),
            (v(-10.0, -15.0), v(-10.0, 15.0)),
            (v(10.0, -15.0), v(10.0, 15.0)),
        ] {
            let pa = ed.doc.add_point(a);
            let pb = ed.doc.add_point(b);
            ed.doc.add_line(pa, pb).unwrap();
        }
        ed.commit("Setup");
    }
    h.run();
    h.key_press(Key::T);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Trim);
    h.hover_at(screen(&h, v(0.0, 0.3)));
    h.run();
    save_png("drift_ui_trim_preview.png", &mut h);
    click_world(&mut h, v(0.0, 0.3));
    assert_eq!(lines(&h), 4, "the middle piece went, leaving two");
    // Fillet is past the bottom of the tool list at this window height: scroll to it.
    h.get_by_label("Fillet").scroll_to_me();
    h.run();
    h.get_by_label("Fillet").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Fillet);
}

#[test]
fn clicking_inside_closed_geometry_selects_its_region() {
    use std::f64::consts::PI;

    let Some(mut h) = harness() else { return };
    // Preferences kept on the editor survive opening a drawing.
    h.state_mut().editor.pick_filter = crate::editor::PickFilter::Points;
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.run();
    assert_eq!(
        h.state().editor.pick_filter,
        crate::editor::PickFilter::Points
    );
    h.state_mut().editor.pick_filter = crate::editor::PickFilter::All;
    // By default an area is picked only where it is filled: the plate is empty.
    assert!(h.state().editor.regions_need_fill());
    click_world(&mut h, DVec2::new(100.0, 10.0));
    assert!(
        h.state_mut().editor.region().is_none(),
        "an empty outline picks nothing"
    );
    assert!(h.state().view_prefs().contains("rfill=1"));
    // With that off, the inside of any closed outline picks it.
    h.state_mut()
        .editor
        .change_rules(|r| r.regions_need_fill = false);
    click_world(&mut h, DVec2::new(100.0, 10.0));
    let region = h.state_mut().editor.region().expect("the plate region");
    assert_eq!(region.holes.len(), 6, "bore, four bolt holes and the slot");
    let corners = 4.0 * (100.0 - PI * 100.0 / 4.0);
    let holes = PI * 14.0 * 14.0 + 4.0 * PI * 3.2 * 3.2 + (8.0 * 30.0 + PI * 16.0);
    let expected = 120.0 * 80.0 - corners - holes;
    assert!(
        (region.area - expected).abs() < 1e-6,
        "{} vs {expected}",
        region.area
    );
    h.run();
    save_png("drift_ui_region.png", &mut h);
    // Clicking inside a hole (clear of the centre lines, the 45° line and the glyphs)
    // picks the hole's own region.
    click_world(&mut h, DVec2::new(63.0, 47.0));
    let bore = h.state_mut().editor.region().expect("the bore");
    assert!((bore.area - PI * 196.0).abs() < 1e-9);
}

#[test]
fn layers_colour_weight_and_thin_lines() {
    use eframe::egui::accesskit::Role;

    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    {
        let app = h.state_mut();
        let ed = &mut app.editor;
        let outline = ed.add_layer("Outline");
        let mut look = ed.doc.layer(outline).unwrap().clone();
        look.color = Some(drift_core::Rgb([0xe0, 0x50, 0x40]));
        look.weight = drift_core::LineWeight(70);
        ed.update_layer(outline, look);
        // The outer boundary: lines and the corner arcs (the curves not inside the plate).
        ed.selection = ed
            .doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point()
                    && e.style == drift_core::LineKind::Solid
                    && ed.doc.shape(*id).is_some_and(|s| {
                        let b = s.bounds();
                        b.min.x < 12.0 || b.max.x > 108.0 || b.min.y < 12.0 || b.max.y > 68.0
                    })
            })
            .map(|(id, _)| id)
            .collect();
        ed.move_selection_to_layer(outline);
        ed.selection.clear();
        app.status_colors = false;
    }
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    h.run();
    save_png("drift_ui_layers.png", &mut h);
    h.key_press(Key::W);
    h.run();
    assert!(h.state().thin_lines);
    save_png("drift_ui_thin_lines.png", &mut h);
}

#[test]
fn light_theme_toast_and_grid_toggle() {
    let Some(mut h) = harness() else { return };
    h.ctx.set_theme(egui::Theme::Light);
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let now = h.ctx.input(|i| i.time);
    h.state_mut()
        .set_status("Pinned (see the Parameters tab)".into(), false, now);
    // A toast schedules its own fade, so step frames rather than waiting for idle.
    h.run_steps(3);
    save_png("drift_ui_light.png", &mut h);
    assert!(h.state().show_grid);
    h.event(Event::Key {
        key: Key::Quote,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run_steps(2);
    assert!(!h.state().show_grid, "Ctrl+' hides the grid");
    // The preference survives a restart.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    assert!(!fresh.state().show_grid);
    fresh.ctx.set_theme(egui::Theme::Dark);
    fresh.run();
    fresh.run();
    save_png("drift_ui_no_grid.png", &mut fresh);
}

#[test]
fn parameters_and_layers_tabs_render() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Parameters);
    h.run();
    save_png("drift_ui_parameters.png", &mut h);
    let id = h.state_mut().editor.add_layer("Notes");
    let mut layer = h.state().editor.doc.layer(id).cloned().expect("added");
    layer.visible = false;
    layer.locked = true;
    h.state_mut().editor.update_layer(id, layer);
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    save_png("drift_ui_layers.png", &mut h);
}

#[test]
fn layer_rows_pick_and_drag_to_reorder() {
    let Some(mut h) = harness() else { return };
    let (walls, doors, notes) = {
        let ed = &mut h.state_mut().editor;
        (
            ed.add_layer("Walls"),
            ed.add_layer("Doors"),
            ed.add_layer("Notes"),
        )
    };
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    // The list reads top down: Notes, Doors, Walls (then 0).
    let y = |h: &Harness<'_, DriftApp>, name: &str| h.get_by_label(name).rect().center().y;
    assert!(y(&h, "Notes") < y(&h, "Doors") && y(&h, "Doors") < y(&h, "Walls"));
    // A click in the row's empty space (right of the name) makes the layer current.
    let row = |h: &Harness<'_, DriftApp>, name: &str| {
        let r = h.get_by_label(name).rect();
        egui::pos2(r.right() + 40.0, r.center().y)
    };
    let at = row(&h, "Walls");
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().editor.doc.current_layer(), walls);
    save_png("drift_ui_layer_rows.png", &mut h);
    // Drag Notes to just below Walls: second from the bottom of the draw order.
    let (from, to) = (row(&h, "Notes"), row(&h, "Walls") + egui::vec2(0.0, 8.0));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=8 {
        h.hover_at(from + (to - from) * (k as f32 / 8.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    h.run();
    let order: Vec<_> = h.state().editor.doc.layers().map(|(id, _)| id).collect();
    assert_eq!(order, [drift_core::LayerId::DEFAULT, notes, walls, doors]);
}

/// A drawing with `n` layers named like an imported DWG's (A-WALL-007, ...), opened in the
/// Layers panel. Returns the layers bottom up.
fn many_layers(h: &mut Harness<'_, DriftApp>, n: usize) -> Vec<drift_core::LayerId> {
    let kinds = ["WALL", "DOOR", "GLAZ", "FURN", "ANNO"];
    let ids = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let ids = (0..n)
            .map(|i| {
                let name = format!("A-{}-{i:03}", kinds[i % kinds.len()]);
                ed.doc.add_layer(drift_core::Layer::new(&name))
            })
            .collect();
        ed.commit("Layers");
        ids
    };
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    h.run();
    ids
}

/// The layer rows drawn this frame.
fn drawn_rows(h: &Harness<'_, DriftApp>) -> Vec<String> {
    let doc = &h.state().editor.doc;
    doc.layers()
        .map(|(_, l)| l.name.clone())
        // (Layer 0's name is also a curve count.)
        .filter(|name| name != "0" && h.query_all_by_label(name).next().is_some())
        .collect()
}

#[test]
fn hundreds_of_layers_draw_only_the_rows_in_view() {
    let Some(mut h) = harness() else { return };
    let ids = many_layers(&mut h, 500);
    // The top of the stack is in view; only a screenful of rows is drawn.
    let rows = drawn_rows(&h);
    assert!(rows.contains(&"A-ANNO-499".to_owned()), "{rows:?}");
    assert!(rows.len() < 40, "{} rows drawn", rows.len());
    save_png("drift_ui_many_layers.png", &mut h);
    // Scrolled to the bottom, the first layers are drawn (and the top ones not).
    let at = h.get_by_label("A-ANNO-499").rect().center();
    for _ in 0..40 {
        h.hover_at(at);
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        });
        // (Rows pass under the pointer, their tooltips coming and going.)
        h.step();
    }
    h.hover_at(h.state().canvas_rect.center());
    h.run();
    let rows = drawn_rows(&h);
    assert!(rows.contains(&"A-WALL-000".to_owned()), "{rows:?}");
    assert!(!rows.contains(&"A-ANNO-499".to_owned()));
    assert!(rows.len() < 40, "{} rows drawn", rows.len());
    // Opening a row there (taller) keeps the rows below it in place, and clicking one
    // still makes it current.
    let walls = ids[0];
    h.state_mut().expanded_layers.insert(ids[3]);
    h.run();
    h.run();
    let r = h.get_by_label("A-WALL-000").rect();
    let at = egui::pos2(r.right() + 40.0, r.center().y);
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().editor.doc.current_layer(), walls);
    save_png("drift_ui_many_layers_bottom.png", &mut h);
}

#[test]
fn the_layer_filter_finds_layers_and_keeps_the_current_one() {
    let Some(mut h) = harness() else { return };
    let ids = many_layers(&mut h, 500);
    let current = ids[1]; // A-DOOR-001
    h.state_mut().editor.set_current_layer(current);
    let field =
        egui_kittest::kittest::By::new().predicate(|n| n.placeholder() == Some("Filter layers"));
    h.get(field).click();
    h.run();
    type_text(&mut h, "glaz-01");
    h.run();
    // A-GLAZ-012 and -017, of the 501 layers (with 0).
    assert!(h.query_by_label("2 of 501").is_some());
    let rows = drawn_rows(&h);
    assert!(
        rows.iter()
            .all(|r| r.starts_with("A-GLAZ-01") || r == "A-DOOR-001")
    );
    assert!(rows.contains(&"A-GLAZ-017".to_owned()));
    // The current layer doesn't match but stays in sight above the list.
    assert!(rows.contains(&"A-DOOR-001".to_owned()));
    assert!(
        h.query_by_label("Current layer (not matching the filter)")
            .is_some()
    );
    save_png("drift_ui_layer_filter.png", &mut h);
    // Any case; cleared by its button.
    h.state_mut().layer_list.filter = "WALL-49".into();
    h.run();
    h.run();
    assert!(h.query_by_label("2 of 501").is_some());
    h.get_by_label("×").click();
    h.run();
    h.run();
    assert!(h.state().layer_list.filter.is_empty());
    assert!(drawn_rows(&h).contains(&"A-ANNO-499".to_owned()));
}

#[test]
fn long_layer_names_lose_their_middle_and_show_whole_in_the_tooltip() {
    let Some(mut h) = harness() else { return };
    let long = "A-WALL-FULL-PATT-EXTERIOR-CONCRETE-200MM-HATCH";
    h.state_mut().editor.add_layer(long);
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    h.run();
    assert!(h.query_by_label(long).is_none());
    // (The row's name, shortened; "Import PDF or DXF…" is the other.)
    let shown = h
        .query_all_by_label_contains("…")
        .find(|n| n.value().is_some_and(|v| v.contains("HATCH")))
        .expect("the shortened name");
    let label = shown.value().unwrap_or_default();
    assert!(
        label.starts_with("A-WALL-") && label.ends_with("HATCH"),
        "{label}"
    );
    let at = shown.rect().center();
    h.hover_at(at);
    for _ in 0..30 {
        h.step();
    }
    assert!(h.query_by_label_contains(long).is_some());
    save_png("drift_ui_long_layer_name.png", &mut h);
}

#[test]
fn a_layer_is_isolated_and_the_isolate_ended_in_one_undo_step_each() {
    let Some(mut h) = harness() else { return };
    let ids = many_layers(&mut h, 12);
    let visible = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        doc.layers()
            .filter(|(_, l)| l.visible)
            .map(|(id, _)| id)
            .collect::<Vec<_>>()
    };
    // One layer hidden beforehand, to come back hidden.
    let hidden = ids[5];
    {
        let ed = &mut h.state_mut().editor;
        let mut l = ed.doc.layer(hidden).cloned().unwrap();
        l.visible = false;
        ed.update_layer(hidden, l);
    }
    h.run();
    let before = visible(&h);
    let target = ids[9]; // A-ANNO-009
    // Right-click its row: Isolate.
    let r = h.get_by_label("A-ANNO-009").rect();
    let at = egui::pos2(r.right() + 30.0, r.center().y);
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
    h.get_by_label("Isolate").click();
    h.run();
    h.run();
    assert_eq!(visible(&h), [target]);
    assert_eq!(h.state().editor.doc.current_layer(), target);
    assert!(h.query_by_label("Isolating A-ANNO-009").is_some());
    save_png("drift_ui_layer_isolated.png", &mut h);
    // One undo step: undone, the layers are as they were and the bar goes.
    h.state_mut().undo();
    h.run();
    assert_eq!(visible(&h), before);
    assert!(h.query_by_label("Isolating A-ANNO-009").is_none());
    // Alt-click on another layer's eye isolates it; End isolate puts every layer back,
    // the one hidden before still hidden.
    let other = ids[2]; // A-GLAZ-002
    let name = h.get_by_label("A-GLAZ-002").rect();
    let eye = h
        .query_all_by_label("Show or hide · Alt-click: show this layer alone")
        .map(|n| n.rect())
        .find(|e| (e.center().y - name.center().y).abs() < 4.0)
        .expect("the row's eye");
    h.hover_at(eye.center());
    h.run();
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: eye.center(),
            button: PointerButton::Primary,
            pressed,
            modifiers: EguiModifiers::ALT,
        });
        h.run();
    }
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    assert_eq!(visible(&h), [other]);
    h.get_by_label("End isolate").click();
    h.run();
    h.run();
    assert_eq!(visible(&h), before);
    assert!(!visible(&h).contains(&hidden));
    assert!(h.query_by_label("End isolate").is_none());
    // Ending is one undo step too: undone, the layer is isolated again.
    h.state_mut().undo();
    h.run();
    assert_eq!(visible(&h), [other]);
}

#[test]
fn fill_tool_hatches_and_fills_regions() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.get_by_label("Fill / hatch").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Fill);
    // Hatch the plate (the bore and holes stay open), then fill the bore solid.
    click_world(&mut h, DVec2::new(100.0, 15.0));
    assert_eq!(h.state().editor.doc.fills().count(), 1);
    // With the new hatch picked, the tool's bar and inspector still set the next fill's
    // style, and the eyedropper is at hand.
    h.run();
    assert_eq!(h.state().editor.selected_fills.len(), 1);
    assert!(h.query_by_label("New fills").is_some());
    // (Solid is in the tool's bar and in the inspector.)
    assert_eq!(h.query_all_by_label("Solid").count(), 2);
    h.query_all_by_label("Solid").next().unwrap().click();
    h.run();
    assert_eq!(
        h.state().editor.fill_style.kind,
        drift_core::FillKind::Solid
    );
    assert!(h.query_all_by_label("Pick up a style").count() >= 1);
    click_world(&mut h, DVec2::new(75.0, 45.0));
    assert_eq!(h.state().editor.doc.fills().count(), 2);
    let shapes = h.state_mut().editor.fill_shapes();
    assert!(shapes.values().all(|s| !s.lost));
    assert!(
        shapes.values().any(|s| s.region.holes.len() >= 5),
        "the plate's hatch excludes the bore, slot and bolt holes"
    );
    h.run();
    save_png("drift_ui_fills.png", &mut h);
    // Pick the hatch with the select tool: the inspector shows it.
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    assert_eq!(h.state().editor.selected_fills.len(), 1);
    assert!(h.state().editor.selection.is_empty());
    h.run();
    save_png("drift_ui_hatch_selected.png", &mut h);
    // Delete removes the fill, undo brings it back.
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(h.state().editor.doc.fills().count(), 1);
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.fills().count(), 2);
}

#[test]
fn a_hatch_follows_its_corner_moved_in_and_a_side_split() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A 120 x 60 plate.
    let mut doc = drift_core::Document::new();
    let corners = [(0.0, 0.0), (120.0, 0.0), (120.0, 60.0), (0.0, 60.0)]
        .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
    let sides: Vec<_> = (0..4)
        .map(|i| doc.add_line(corners[i], corners[(i + 1) % 4]).unwrap())
        .collect();
    h.state_mut()
        .replace_document(doc, "plate".into(), None, Default::default(), true);
    h.run();
    h.run();
    // Hatch it, clicking right in its bottom-left corner.
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(4.0, 4.0));
    let fill = h
        .state()
        .editor
        .selected_fills
        .first()
        .copied()
        .expect("a hatch");
    // Move that corner in (as typed in the inspector), past where the hatch was picked.
    assert!(
        h.state_mut()
            .editor
            .move_point_to(corners[0], DVec2::new(30.0, 20.0))
    );
    h.run();
    // Split the right side.
    h.get_by_label("Split").scroll_to_me();
    h.run();
    h.get_by_label("Split").click();
    h.run();
    click_world(&mut h, DVec2::new(120.0, 30.0));
    let ed = &mut h.state_mut().editor;
    assert_eq!(
        ed.doc.fill(fill).unwrap().boundary.len(),
        5,
        "the split side's pieces"
    );
    let p: Vec<DVec2> = corners.iter().map(|c| ed.doc.point(*c).unwrap()).collect();
    let area = (0..4)
        .map(|i| p[i].perp_dot(p[(i + 1) % 4]))
        .sum::<f64>()
        .abs()
        / 2.0;
    let shape = ed.fill_shapes()[&fill].clone();
    assert!(!shape.lost, "the hatch kept its area");
    assert!((shape.region.area - area).abs() < 1e-6);
    assert!(
        sides
            .iter()
            .all(|s| ed.doc.fill(fill).unwrap().boundary.contains(s))
    );
    h.run();
    save_png("drift_ui_hatch_follows.png", &mut h);
    // Undo both: the hatch is back on the plate as drawn.
    h.state_mut().undo();
    h.state_mut().undo();
    let ed = &mut h.state_mut().editor;
    let shape = ed.fill_shapes()[&fill].clone();
    assert!(!shape.lost);
    assert!((shape.region.area - 7200.0).abs() < 1e-6);
}

#[test]
fn temporary_dimensions_show_and_move_the_picked_wall() {
    let Some(mut h) = harness() else { return };
    // (Past the start screen.)
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // Two walls 100 apart and a short one above.
    let mut doc = drift_core::Document::new();
    let mut wall = |y: f64, x0: f64, x1: f64| {
        let a = doc.add_point(DVec2::new(x0, y));
        let b = doc.add_point(DVec2::new(x1, y));
        doc.add_line(a, b).unwrap()
    };
    let (low, mid) = (wall(0.0, 0.0, 400.0), wall(100.0, 0.0, 400.0));
    wall(250.0, 100.0, 300.0);
    h.state_mut()
        .replace_document(doc, "walls".into(), None, Default::default(), true);
    h.run();
    h.run();
    // (Off its middle, which picks the midpoint.)
    click_world(&mut h, DVec2::new(120.0, 100.0));
    h.run();
    assert_eq!(h.state().editor.selection, [mid].into());
    assert_eq!(h.state().temp_dims.dims.len(), 3, "length, above, below");
    save_png("drift_ui_temp_dims.png", &mut h);
    // Click the value between the walls and type a new one: the picked wall moves.
    let below = h.state().temp_dims.values[2].1;
    h.hover_at(below);
    h.run();
    button(&h, below, true);
    h.run();
    button(&h, below, false);
    h.run();
    assert!(
        h.state().temp_dims.edit.is_some(),
        "the value editor opened"
    );
    h.run();
    h.event(egui::Event::Text("120".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let y = |h: &Harness<'_, DriftApp>, l| match h.state().editor.doc.shape(l) {
        Some(drift_core::Shape::Line { a, .. }) => a.y,
        _ => f64::NAN,
    };
    assert!((y(&h, mid) - 120.0).abs() < 1e-6, "{}", y(&h, mid));
    assert_eq!(y(&h, low), 0.0);
    assert_eq!(h.state().editor.selection, [mid].into(), "still picked");
    h.run();
    save_png("drift_ui_temp_dims_moved.png", &mut h);
    // The preference turns them off.
    h.state_mut().show_temp_dims = false;
    h.run();
    assert!(h.state().temp_dims.dims.is_empty());
}

#[test]
fn drafting_tools_annotate_the_plate() {
    use drift_core::Annotation;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let annotations = |h: &Harness<'_, DriftApp>| -> Vec<Annotation> {
        h.state()
            .editor
            .doc
            .annotations()
            .map(|(_, a)| a.clone())
            .collect()
    };

    // A note with a leader from the top edge; type its text and press Return.
    h.get_by_label("Note").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 80.0));
    click_world(&mut h, DVec2::new(25.0, 95.0));
    assert!(h.state().note_edit.is_some(), "the note opens for typing");
    h.event(Event::Text("DEBURR ALL EDGES".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().note_edit.is_none());
    match annotations(&h).as_slice() {
        [Annotation::Note { text, leaders, .. }] => {
            assert_eq!(text, "DEBURR ALL EDGES");
            assert_eq!(leaders.len(), 1);
        }
        other => panic!("{other:?}"),
    }

    // A centre mark on the bore.
    h.get_by_label("Centre mark").click();
    h.run();
    click_world(&mut h, DVec2::new(84.0, 40.0));
    // A hole callout on a bolt hole.
    h.get_by_label("Callout").click();
    h.run();
    let hole = DVec2::new(70.0, 40.0) + DVec2::splat(26.0 / 2f64.sqrt());
    click_world(&mut h, hole + DVec2::new(3.2, 0.0));
    click_world(&mut h, DVec2::new(108.0, 92.0));
    // Ordinates from the bottom-left fillet's end.
    h.get_by_label("Ordinate").click();
    h.run();
    click_world(&mut h, DVec2::new(10.0, 0.0));
    click_world(&mut h, DVec2::new(110.0, 0.0));
    let all = annotations(&h);
    assert!(
        all.iter()
            .any(|a| matches!(a, Annotation::CentreMark { .. }))
    );
    let callout = all
        .iter()
        .find_map(|a| match a {
            Annotation::HoleCallout { curve, suffix, .. } => Some((*curve, suffix.clone())),
            _ => None,
        })
        .expect("a hole callout");
    assert_eq!(
        crate::notes::hole_callout_text(&h.state().editor.doc, callout.0, &callout.1).as_deref(),
        Some("4× Ø6.4 THRU")
    );
    assert_eq!(
        all.iter()
            .filter(|a| matches!(a, Annotation::Ordinate { .. }))
            .count(),
        2,
        "the origin (0) and one point"
    );
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Select").click();
    h.run();
    h.run();
    save_png("drift_ui_drafting.png", &mut h);

    // Pick the note by its text and delete it.
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let map = screen_map(&h);
    let note = crate::notes::layout_annotations(&painter, &h.state().editor.doc, &map)
        .into_iter()
        .find(|l| {
            l.text
                .as_ref()
                .is_some_and(|t| t.text == "DEBURR ALL EDGES")
        })
        .expect("note laid out");
    let at = note.text.unwrap().center;
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert!(h.state().editor.selected_annotation.is_some());
    h.run();
    save_png("drift_ui_note_selected.png", &mut h);
    h.key_press(Key::Delete);
    h.run();
    assert!(
        !annotations(&h)
            .iter()
            .any(|a| matches!(a, Annotation::Note { .. }))
    );
}

#[test]
fn drafting_styles_render() {
    use drift_core::{Constraint, ConstraintId, DimNote, DraftStyle, LengthFormat, Tolerance};
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let dims: Vec<(ConstraintId, Constraint)> = h
        .state()
        .editor
        .doc
        .constraints()
        .filter(|(_, c)| c.is_dimension())
        .map(|(id, c)| (id, *c))
        .collect();
    let find = |pred: &dyn Fn(&Constraint) -> bool| {
        dims.iter()
            .find(|(_, c)| pred(c))
            .map(|(id, _)| *id)
            .unwrap()
    };
    let bore =
        find(&|c| matches!(c, Constraint::Diameter { value, .. } if (value - 28.0).abs() < 1e-9));
    let width = find(
        &|c| matches!(c, Constraint::HorizontalDistance { value, .. } if (value - 120.0).abs() < 1e-9),
    );
    let height = find(
        &|c| matches!(c, Constraint::VerticalDistance { value, .. } if (value - 80.0).abs() < 1e-9),
    );
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    ed.doc.set_dim_note(
        bore,
        Some(DimNote {
            tolerance: Tolerance::Fit("H7".into()),
            ..DimNote::default()
        }),
    );
    ed.doc.set_dim_note(
        width,
        Some(DimNote {
            tolerance: Tolerance::Symmetric(0.1),
            ..DimNote::default()
        }),
    );
    ed.doc.set_dim_note(
        height,
        Some(DimNote {
            tolerance: Tolerance::Deviation {
                upper: 0.05,
                lower: 0.02,
            },
            ..DimNote::default()
        }),
    );
    ed.commit("Tolerances");
    h.run();
    save_png("drift_ui_iso_tolerances.png", &mut h);

    let mut asme = DraftStyle::asme();
    asme.alt_units = Some(LengthFormat::Inch);
    asme.arrow = drift_core::ArrowStyle::Open;
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    ed.doc.set_draft_style(asme);
    ed.commit("ASME");
    h.state_mut().show_panel(Panel::Parameters);
    h.run();
    h.run();
    save_png("drift_ui_asme.png", &mut h);
    // Pick the height dimension: the inspector edits its tolerance.
    h.state_mut().show_panel(Panel::Inspector);
    h.state_mut().selected_constraint = Some(height);
    h.run();
    h.run();
    save_png("drift_ui_dimension_inspector.png", &mut h);
}

#[test]
fn selection_box_released_over_a_side_panel_ends() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let start = screen(&h, DVec2::new(-20.0, -20.0));
    h.hover_at(start);
    h.run();
    button(&h, start, true);
    h.run();
    // Drag across the canvas and out over the inspector, then release there.
    for x in [start.x + 40.0, start.x + 300.0, 1150.0] {
        h.hover_at(Pos2::new(x, start.y - 200.0));
        h.run();
    }
    assert!(
        matches!(h.state().tool.overlay(), Some(Overlay::SelectionBox { .. })),
        "the box follows the pointer off the canvas"
    );
    let off = Pos2::new(1150.0, start.y - 200.0);
    button(&h, off, false);
    h.run();
    h.run();
    assert!(h.state().tool.overlay().is_none(), "no box left on screen");
    assert!(!h.state().tool.is_busy());
    assert!(
        !h.state().editor.selection.is_empty(),
        "the box selected what it covered"
    );
}

#[test]
fn drawn_circle_can_be_picked_and_dimensioned() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    h.get_by_label("Circle").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 0.0));
    let circle = h
        .state()
        .editor
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .map(|(id, _)| id)
        .expect("circle drawn");
    // Select it by clicking its edge.
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 20.0));
    assert!(
        h.state().editor.selection.contains(&circle),
        "click picks the circle"
    );
    // Dimension it.
    h.get_by_label("Dimension").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 20.0));
    click_world(&mut h, DVec2::new(30.0, 30.0));
    let has = h
        .state()
        .editor
        .doc
        .constraints()
        .any(|(_, c)| matches!(c, Constraint::Diameter { .. }));
    assert!(has, "diameter added");
    h.run();
    h.event(Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.event(Event::Text("50".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(
        drift_solver::radius_of(&h.state().editor.doc, circle),
        Some(25.0)
    );
}

#[test]
fn dragging_a_drawn_circles_edge_resizes_it() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Circle").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 0.0));
    let circle = h
        .state()
        .editor
        .doc
        .iter()
        .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .map(|(id, _)| id)
        .unwrap();
    h.get_by_label("Select").click();
    h.run();
    let from = screen(&h, DVec2::new(0.0, 20.0));
    let to = screen(&h, DVec2::new(0.0, 35.0));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(from + (to - from) * (k as f32 / 5.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    let r = drift_solver::radius_of(&h.state().editor.doc, circle).unwrap();
    assert!((r - 35.0).abs() < 1.0, "radius {r}");
}

#[test]
fn a_sheet_prints_in_greyscale_or_black_and_white() {
    use drift_core::{Rgb, SheetColours};
    use drift_draft::page::Prim;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A solid dark blue fill under the plate's outline, on a red layer.
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    h.get_by_label("Select").click();
    h.run();
    {
        let doc = &mut h.state_mut().editor.doc;
        doc.begin();
        let fills: Vec<_> = doc.fills().map(|(id, _)| id).collect();
        for id in fills {
            let mut fill = doc.fill(id).unwrap().clone();
            fill.kind = drift_core::FillKind::Solid;
            fill.color = Some(Rgb([0x20, 0x40, 0x90]));
            fill.opacity = 1.0;
            doc.set_fill(id, fill);
        }
        let (layer, mut red) = doc.layers().next().map(|(id, l)| (id, l.clone())).unwrap();
        red.color = Some(Rgb([0xe0, 0x20, 0x20]));
        doc.set_layer(layer, red);
    }
    h.state_mut().editor.commit("Colour it");
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    assert_eq!(
        h.state().editor.doc.sheet(id).unwrap().colours,
        SheetColours::Colour
    );
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    // What the preview shows, and what the PDF writes.
    let preview = |h: &Harness<'_, DriftApp>| h.state().preview_cache.as_ref().unwrap().2.clone();
    let grey = |c: Rgb| c.0[0] == c.0[1] && c.0[1] == c.0[2];
    let colour = preview(&h);
    assert!(colour.items.iter().any(|i| !grey(i.pen.color)), "in colour");
    save_png("drift_ui_sheet_colour.png", &mut h);

    h.get_by_label("Black & white").click();
    h.run();
    h.run();
    assert_eq!(
        h.state().editor.doc.sheet(id).unwrap().colours,
        SheetColours::BlackWhite
    );
    assert_eq!(h.state().editor.history.undo_label(), Some("Edit sheet"));
    let mono = preview(&h);
    assert_eq!(mono.items.len(), colour.items.len());
    for it in &mono.items {
        if matches!(it.prim, Prim::Fill { .. }) {
            assert!(grey(it.pen.color), "{:?}", it.pen.color);
        } else {
            // Black, but for the sheet number knocked out of the title block's black box.
            let white = it.pen.color == Rgb([255, 255, 255]);
            assert!(it.pen.color == Rgb([0, 0, 0]) || white, "{:?}", it.prim);
        }
    }
    // The blue fill prints as its grey, not black, so the lines on it still show.
    let blue = Rgb([drift_core::sheets::luminance(Rgb([0x20, 0x40, 0x90])); 3]);
    assert!(
        mono.items
            .iter()
            .any(|i| matches!(i.prim, Prim::Fill { .. }) && i.pen.color == blue),
        "the fill's grey"
    );
    let pages = h.state_mut().pages(&painter);
    assert_eq!(pages[0], mono, "what prints is what the preview shows");
    save_png("drift_ui_sheet_mono.png", &mut h);

    h.get_by_label("Greyscale").click();
    h.run();
    h.run();
    let greys = preview(&h);
    assert!(greys.items.iter().all(|i| grey(i.pen.color)));
    // Lines keep their own grey (the red layer isn't black).
    assert!(
        greys
            .items
            .iter()
            .any(|i| !matches!(i.prim, Prim::Fill { .. }) && i.pen.color != Rgb([0, 0, 0]))
    );
    save_png("drift_ui_sheet_greyscale.png", &mut h);
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""colours":"greyscale""#));
    // Undo goes back to black & white, then colour.
    h.state_mut().undo();
    h.state_mut().undo();
    assert_eq!(
        h.state().editor.doc.sheet(id).unwrap().colours,
        SheetColours::Colour
    );
}

/// A picked 400 square made into the block "Table": the app, with its one copy picked.
fn table_block(h: &mut Harness<'static, DriftApp>) -> drift_core::BlockId {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [(0.0, 0.0), (400.0, 0.0), (400.0, 400.0), (0.0, 400.0)]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let lines: Vec<drift_core::EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.commit("Square");
    ed.selection = lines.into_iter().collect();
    let copy = ed.make_block("Table").unwrap();
    ed.selection.clear();
    ed.selected_instances = [copy].into();
    let block = ed.doc.instance(copy).unwrap().block;
    h.run();
    block
}

#[test]
fn f2_renames_the_picked_blocks_copies() {
    let Some(mut h) = harness() else { return };
    let block = table_block(&mut h);
    h.key_press(Key::F2);
    h.run();
    // The dialog opens on the old name, selected: typing replaces it.
    assert!(h.query_by_label("Rename block").is_some(), "the dialog");
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("Desk");
    h.run();
    save_png("drift_ui_rename_block.png", &mut h);
    // Letters typed into the dialog don't start tools.
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().editor.doc.block(block).unwrap().name, "Desk");
    assert!(h.query_by_label("Rename block").is_none(), "closed");
}

#[test]
fn a_blocks_menu_stays_open_while_its_name_is_typed() {
    let Some(mut h) = harness() else { return };
    let block = table_block(&mut h);
    // Nothing picked, so the only name field is the menu's.
    h.state_mut().editor.selected_instances.clear();
    h.state_mut().show_panel(Panel::Blocks);
    h.run();
    // Right-click the block's row, then click into the menu's Name field (#130).
    h.get_by_label("Table").click_secondary();
    h.run();
    let field = |n: &egui_kittest::kittest::AccessKitNode<'_>| {
        n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("Table")
    };
    h.get_by(field).click();
    h.run();
    assert!(
        h.query_by_label("Delete block and copies").is_some(),
        "the menu stays open"
    );
    h.get_by(field).type_text(" 2");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().editor.doc.block(block).unwrap().name, "Table 2");
}

#[test]
fn a_blocks_base_point_is_picked_when_made_and_set_later() {
    let Some(mut h) = harness() else { return };
    // A 400 square, picked.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [(0.0, 0.0), (400.0, 0.0), (400.0, 400.0), (0.0, 400.0)]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let lines: Vec<drift_core::EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.commit("Square");
    ed.selection = lines.into_iter().collect();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::F);
    h.run();
    h.state_mut().show_panel(Panel::Blocks);
    h.run();
    // Make asks for the base point: a click on the corner snaps to it.
    h.get_by_label("Make").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::BasePoint);
    assert!(h.state().editor.doc.instances().next().is_none(), "not yet");
    click_world(&mut h, DVec2::new(1.0, -1.0));
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    let (copy, i) = h.state().editor.doc.instances().next().expect("the copy");
    assert_eq!(i.at, DVec2::ZERO);
    let block = i.block;
    // The Inspector's Set base point, then Enter: the block's middle.
    h.state_mut().editor.selected_instances = [copy].into();
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.get_by_label("Set base point").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::BasePoint);
    save_png("drift_ui_block_base_point.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert_eq!(
        h.state().editor.doc.instance(copy).unwrap().at,
        DVec2::new(200.0, 200.0)
    );
    let b = h.state().editor.doc.block(block).unwrap().bounds();
    assert_eq!(b.center(), DVec2::ZERO);
    assert_eq!(
        h.state().editor.history.undo_label(),
        Some("Set base point")
    );
    // Editing the block: its base point shows, and can be set from the banner.
    h.state_mut().editor.edit_block(copy);
    h.run();
    assert!(h.query_by_label("Set base point").is_some());
    h.state_mut().editor.finish_block_edit();
    h.run();
}

#[test]
fn a_profile_block_open_for_editing_is_written_live_without_saving() {
    let Some(mut h) = harness() else { return };
    let dir = std::env::temp_dir().join(format!("drift-ui-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let beside = dir.join("Live.drift-profiles.json");
    let app = h.state_mut();
    app.file.path = Some(dir.join("Live.drift"));
    let ed = &mut app.editor;
    ed.doc.begin();
    let block = ed.doc.add_block(drift_core::Block {
        name: "Bead".into(),
        items: vec![drift_core::BlockItem {
            shape: drift_core::Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(10.0, 0.0),
            },
            style: drift_core::LineKind::Solid,
            appearance: Default::default(),
        }],
        profile: true,
        ..Default::default()
    });
    let copy = ed
        .doc
        .add_instance(drift_core::Instance::new(block, DVec2::ZERO))
        .unwrap();
    ed.commit("Block");
    let far_end = || {
        let text = std::fs::read_to_string(&beside).ok()?;
        let file = drift_io::profiles::parse(&text).unwrap();
        Some(file.profiles[0].loops[0].knots[1].co[0])
    };
    // Not while the block is closed: only saving writes then.
    assert_eq!(h.state_mut().live_profiles(0.0), None);
    assert!(!beside.exists());
    // Opened for editing: written straight away, then as it changes.
    h.state_mut().editor.edit_block(copy);
    assert_eq!(h.state_mut().live_profiles(1.0), None);
    assert_eq!(far_end(), Some(0.01));
    let ed = &mut h.state_mut().editor;
    let end = ed
        .doc
        .iter()
        .map(|(id, _)| id)
        .find(|id| ed.doc.point(*id).is_ok_and(|p| p.x > 5.0))
        .expect("the line's far end");
    ed.doc.set_point(end, DVec2::new(25.0, 0.0)).unwrap();
    // Within 0.1 s of the last write it waits (the frame asks to come back)…
    let wait = h.state_mut().live_profiles(1.05).expect("a wait");
    assert!((wait - 0.05).abs() < 1e-9, "{wait}");
    assert_eq!(far_end(), Some(0.01));
    // …then writes the block as it is now.
    assert_eq!(h.state_mut().live_profiles(1.11), None);
    assert_eq!(far_end(), Some(0.025));
    // Nothing more to write until it changes again.
    assert_eq!(h.state_mut().live_profiles(2.0), None);
    h.state_mut().editor.finish_block_edit();
    assert_eq!(h.state_mut().live_profiles(3.0), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_block_marked_as_a_blender_profile_is_written_beside_the_drawing_on_save() {
    let Some(mut h) = harness() else { return };
    // A block with a copy, picked.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let line = |a: (f64, f64), b: (f64, f64)| drift_core::BlockItem {
        shape: drift_core::Shape::Line {
            a: DVec2::new(a.0, a.1),
            b: DVec2::new(b.0, b.1),
        },
        style: drift_core::LineKind::Solid,
        appearance: Default::default(),
    };
    let block = ed.doc.add_block(drift_core::Block {
        name: "Crown".into(),
        items: vec![
            line((0.0, 0.0), (40.0, 0.0)),
            line((40.0, 0.0), (0.0, 30.0)),
            line((0.0, 30.0), (0.0, 0.0)),
        ],
        ..Default::default()
    });
    let copy = ed
        .doc
        .add_instance(drift_core::Instance::new(block, DVec2::new(500.0, 500.0)))
        .unwrap();
    ed.commit("Block");
    assert!(h.state().profiles_text().is_none(), "nothing marked yet");
    h.state_mut().editor.selected_instances = [copy].into();
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    // The Inspector's checkbox marks the block.
    h.get_by_label("Blender profile").click();
    h.run();
    assert!(h.state().editor.doc.block(block).unwrap().profile);
    assert_eq!(
        h.state().editor.history.undo_label(),
        Some("Mark as Blender profile")
    );
    save_png("drift_ui_blender_profile_inspector.png", &mut h);
    // The Blocks panel says so.
    h.state_mut().show_panel(Panel::Blocks);
    h.run();
    assert!(
        h.query_by_label_contains("1 copy · Blender profile")
            .is_some()
    );
    // In block coordinates, in metres, whatever the copy's placement.
    let text = h.state().profiles_text().expect("a profile");
    let file = drift_io::profiles::parse(&text).unwrap();
    assert_eq!(file.profiles[0].name, "Crown");
    assert_eq!(file.profiles[0].loops[0].knots[1].co, [0.04, 0.0]);
    // Saving writes the file beside the drawing; unmarking and saving removes it.
    let dir = std::env::temp_dir().join(format!("drift-ui-profiles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let drawing = dir.join("Trim.drift");
    let beside = dir.join("Trim.drift-profiles.json");
    h.state_mut().file.path = Some(drawing.clone());
    h.state_mut().save();
    h.run();
    assert!(drawing.exists());
    let written = drift_io::profiles::parse(&std::fs::read_to_string(&beside).unwrap()).unwrap();
    assert_eq!(written.profiles, file.profiles);
    assert_eq!(written.source, "Trim.drift");
    assert!(
        h.state()
            .status
            .as_ref()
            .unwrap()
            .text
            .contains("1 Blender profile")
    );
    h.state_mut().editor.set_block_profile(block, false);
    h.state_mut().save();
    h.run();
    assert!(!beside.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_model_exports_as_svg_and_pdf_from_the_file_menu() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.run();
    h.get_by_label("File").click();
    h.run();
    h.get_by_label_contains("Export").click();
    h.run();
    assert!(h.query_by_label("Model as SVG…").is_some());
    assert!(h.query_by_label("Model as PDF…").is_some());
    save_png("drift_ui_export_model_menu.png", &mut h);
    // Dimensions and notes go with it until the box is cleared.
    assert!(h.state().model_annotations);
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let with = h.state_mut().model_page(&painter).expect("the plate");
    assert!(with.layers.iter().any(|l| l.name == "ANNOTATIONS"));
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/drift_model.svg"),
        drift_io::svg::write(&with),
    )
    .unwrap();
    h.get_by_label("Dimensions and notes").click();
    h.run();
    assert!(!h.state().model_annotations);
    let without = h.state_mut().model_page(&painter).expect("the plate");
    assert!(without.layers.iter().all(|l| l.name != "ANNOTATIONS"));
    // The 120 × 80 plate, at true size with 5 mm all round.
    assert_eq!(without.size, DVec2::new(130.0, 90.0));
    let svg = drift_io::svg::write(&without);
    assert!(svg.contains(r#"width="130mm" height="90mm""#), "{svg}");
    let pdf = drift_io::pdf::write(&[without], "Plate");
    assert!(String::from_utf8_lossy(&pdf).contains("/MediaBox [0 0 368.504 255.118]"));
}

#[test]
fn a_sheets_title_block_can_be_border_only_or_none() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    let frame_items = |h: &mut Harness<'_, DriftApp>| {
        let painter = h.ctx.layer_painter(egui::LayerId::background());
        let pages = h.state_mut().pages(&painter);
        pages[0]
            .items
            .iter()
            .filter(|i| i.layer == drift_draft::sheet::FRAME_LAYER)
            .count()
    };
    let modern = frame_items(&mut h);
    // The choices, in order, with today's "None" now "Border only".
    h.get_by_value("Modern").click();
    h.run();
    assert!(h.query_by_label("Border only").is_some());
    save_png("drift_ui_title_block_choices.png", &mut h);
    h.get_by_label("Border only").click();
    h.run();
    let sheet = h.state().editor.doc.sheet(id).unwrap().clone();
    assert_eq!(sheet.title_block, drift_core::TitleBlock::Border);
    let border = frame_items(&mut h);
    assert!(border > 0 && border < modern, "{border} of {modern}");
    // None: blank paper.
    h.get_by_value("Border only").click();
    h.run();
    h.get_by_label("None").click();
    h.run();
    h.run();
    let sheet = h.state().editor.doc.sheet(id).unwrap().clone();
    assert_eq!(sheet.title_block, drift_core::TitleBlock::None);
    save_png("drift_ui_title_block_none.png", &mut h);
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pages = h.state_mut().pages(&painter);
    let b = drift_draft::sheet::border(&sheet);
    assert!(
        !pages[0].items.iter().any(|i| matches!(
            &i.prim,
            drift_draft::page::Prim::Polyline { points, closed: true } if points.contains(&b.min)
        )),
        "no border"
    );
    assert!(frame_items(&mut h) < border);
}

#[test]
fn sheets_preview_and_export() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // Hatch the plate so fills export too.
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    h.get_by_label("Select").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    let views = placed(&h, id);
    assert!(h.state().sheet_preview);
    // The 120 × 80 plate with its dimensions fits A3 at 1:1, above the title block, in a
    // new view of the model.
    assert_eq!(views[0].scale, 1.0);
    assert_eq!(views[0].name, "View 1");
    assert_eq!(h.state().editor.doc.views().count(), 1);
    save_png("drift_ui_sheet.png", &mut h);

    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.starts_with("%PDF-1.4"));
    assert!(
        text.contains("/MediaBox [0 0 1190.551 841.89]"),
        "A3 landscape"
    );
    assert!(text.contains("(120) Tj"), "dimension text exported");
    assert!(text.contains("(1:1) Tj"), "scale in the title block");
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/drift_sheet.pdf"),
        &pdf,
    )
    .unwrap();
    let svg = h.state_mut().svg_text(&painter);
    assert!(svg.contains(r#"width="420mm" height="297mm""#));
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/drift_sheet.svg"),
        &svg,
    )
    .unwrap();
    let dxf = h.state_mut().dxf_text(&painter);
    assert!(dxf.contains("AC1009"));
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/drift_model.dxf"),
        &dxf,
    )
    .unwrap();
    // Re-import the DXF: the plate's curves come back at full size.
    let import = drift_io::dxf::read(&dxf, 1.0).unwrap();
    let mut doc = drift_core::Document::new();
    doc.begin();
    let report = drift_io::dxf::add_to_document(&mut doc, &import);
    doc.commit("Import");
    assert!(report.curves > 30, "{report:?}");
    let circles = doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
        .count();
    assert_eq!(
        circles, 5,
        "the bore and four bolt holes (the bolt circle is construction)"
    );

    // A view can leave the dimensions out (a presentation copy); the model keeps them.
    let mut plain = views[0].clone();
    plain.dimensions = false;
    set_placed(&mut h, id, 0, &plain, "Hide dimensions");
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(!text.contains("(120) Tj") && text.contains("(1:1) Tj"));
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""dimensions":false"#) && !saved.contains(r#""notes""#));
    let (loaded, _) = drift_io::load(&saved).unwrap();
    let back = loaded.placed_views(loaded.sheets().next().unwrap().1);
    assert!(!back[0].dimensions && back[0].notes);
    h.state_mut().undo();
    assert!(placed(&h, id)[0].dimensions);
    h.run();

    // A plain drag on the view that fills the sheet leaves it be (a drag moves a view, and
    // this one has nowhere to move).
    let rect = h.state().canvas_rect;
    let from = rect.center();
    let drag = |h: &mut Harness<'_, DriftApp>| {
        h.hover_at(from);
        h.run();
        button(h, from, true);
        h.run();
        for k in 1..=4 {
            h.hover_at(from + egui::vec2(10.0 * k as f32, 0.0));
            h.run();
        }
        button(h, from + egui::vec2(40.0, 0.0), false);
        h.run();
    };
    drag(&mut h);
    assert_eq!(placed(&h, id)[0].center, views[0].center);
    // Alt-drag slides the drawing within it, as one undo step.
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    h.run();
    drag(&mut h);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    let moved = placed(&h, id)[0].center;
    assert!(
        moved.x < views[0].center.x - 1.0,
        "dragging right moves the drawing right"
    );
    h.state_mut().undo();
    assert_eq!(placed(&h, id)[0].center, views[0].center);
    // Esc returns to the model.
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().sheet_preview);
    // An ANSI B sheet with an ASME title block and revisions.
    let mut ansi = h.state().editor.doc.sheet(id).cloned().unwrap();
    ansi.paper = drift_core::PaperSize::AnsiB;
    ansi.title_block = drift_core::TitleBlock::Ansi;
    ansi.fields.number = "DR-001".into();
    ansi.fields.author = "CP".into();
    ansi.fields.company = "Drift".into();
    ansi.fields.material = "6061-T6".into();
    for (rev, what) in [("A", "First issue"), ("B", "Bore H7")] {
        ansi.revisions.push(drift_core::Revision {
            rev: rev.into(),
            date: "2026-10-08".into(),
            description: what.into(),
            by: "CP".into(),
        });
    }
    h.state_mut().editor.doc.set_sheet(id, ansi);
    let svg = h.state_mut().svg_text(&painter);
    assert!(svg.contains("Bore H7") && svg.contains(">DWG NO<"));
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/drift_sheet_ansi.svg"),
        &svg,
    )
    .unwrap();
}

#[test]
fn panels_float_dock_and_switch_layers() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let walls = h.state_mut().editor.add_layer("Walls");
    h.state_mut().editor.add_layer("Hatching");
    // Every panel starts in the right dock with the inspector in front.
    assert_eq!(
        h.state().layout.place(Panel::Layers),
        Place::Dock(Side::Right)
    );
    assert!(!h.state().layout.is_shown(Panel::Layers));
    // Ctrl+L brings the layers to the front.
    h.event(Event::Key {
        key: Key::L,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run();
    h.run();
    assert!(h.state().layout.is_shown(Panel::Layers));
    save_png("drift_ui_layers_panel.png", &mut h);
    // Clicking a layer's name makes it the current layer.
    h.get_by_label("Walls").click();
    h.run();
    assert_eq!(h.state().editor.doc.current_layer(), walls);
    // Drag the Layers tab onto the canvas: it floats there.
    let tab = h.get_by_label("Layers").rect().center();
    let canvas = h.state().canvas_rect.center();
    h.hover_at(tab);
    h.run();
    button(&h, tab, true);
    h.run();
    for t in 1..=8 {
        let p = tab + (canvas - tab) * (t as f32 / 8.0);
        h.hover_at(p);
        h.run();
    }
    save_png("drift_ui_panel_dragging.png", &mut h);
    button(&h, canvas, false);
    // (The new window appears under the pointer, where it was dropped, and stays there.)
    h.run_steps(8);
    assert_eq!(h.state().layout.place(Panel::Layers), Place::Float);
    let window = h.state().floating_rects(Panel::Layers).unwrap().1;
    assert!(window.contains(canvas), "{window:?} under {canvas:?}");
    // The right dock shows the next tab.
    assert_eq!(h.state().layout.active[1], Some(Panel::Sheets));
    save_png("drift_ui_layers_floating.png", &mut h);
    // Docked left, beside the toolbar; the layout survives a restart.
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Left), None);
    h.run();
    h.run();
    save_png("drift_ui_layers_docked.png", &mut h);
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    assert_eq!(
        fresh.state().layout.place(Panel::Layers),
        Place::Dock(Side::Left)
    );
    // Ctrl+L hides it, and again brings it back where it was.
    fresh.state_mut().toggle_panel(Panel::Layers);
    assert_eq!(fresh.state().layout.place(Panel::Layers), Place::Hidden);
    fresh.state_mut().toggle_panel(Panel::Layers);
    assert_eq!(
        fresh.state().layout.place(Panel::Layers),
        Place::Dock(Side::Left)
    );
}

#[test]
fn command_palette_finds_and_runs_commands() {
    let Some(mut h) = harness() else { return };
    h.event(Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run();
    h.run();
    assert!(h.state().palette.is_some());
    h.event(Event::Text("rect".into()));
    h.run();
    save_png("drift_ui_palette.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().palette.is_none());
    assert_eq!(h.state().tool.kind(), ToolKind::Rectangle);
    // Settings toggle; Esc closes without doing anything.
    let grid = h.state().show_grid;
    h.state_mut().open_palette();
    h.run();
    h.event(Event::Text("grid".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().show_grid, !grid);
    h.state_mut().open_palette();
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().palette.is_none());
    assert_eq!(h.state().tool.kind(), ToolKind::Rectangle);
}

#[test]
fn group_and_rotate_on_the_canvas() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // Pick the four bolt holes and group them.
    let holes: std::collections::BTreeSet<drift_core::EntityId> = {
        let doc = &h.state().editor.doc;
        doc.iter()
            .filter(|(id, e)| {
                matches!(e.kind, EntityKind::Circle { .. })
                    && drift_solver::radius_of(doc, *id).is_some_and(|r| (r - 3.2).abs() < 1e-9)
            })
            .map(|(id, _)| id)
            .collect()
    };
    assert_eq!(holes.len(), 4);
    h.state_mut().editor.selection = holes.clone();
    h.event(Event::Key {
        key: Key::G,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run();
    let g = h.state().editor.selected_group().expect("grouped");
    assert_eq!(h.state().editor.doc.group(g).unwrap().members, holes);
    // Clicking any hole picks all four.
    h.state_mut().editor.selection.clear();
    let hole = DVec2::new(70.0, 40.0) + DVec2::splat(26.0 / 2f64.sqrt());
    click_world(&mut h, hole + DVec2::new(0.0, 3.2));
    assert_eq!(h.state().editor.selection, holes);
    h.run();
    save_png("drift_ui_group.png", &mut h);
    // R rotates the group about its centre: show the guide mid-turn.
    let start = screen(&h, DVec2::new(110.0, 40.0));
    h.hover_at(start);
    h.run();
    h.key_press(Key::R);
    h.run();
    assert!(h.state().tool.is_modal());
    h.hover_at(screen(
        &h,
        DVec2::new(70.0, 40.0) + DVec2::from_angle(0.5) * 40.0,
    ));
    h.run();
    save_png("drift_ui_rotate.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().tool.is_modal());
}

#[test]
fn furniture_blocks_place_edit_and_export() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Blocks);
    h.run();
    // Find a dining chair in the library: clicking it adds it and starts placing it.
    h.state_mut().block_search = "chair dining".into();
    h.run();
    h.get_by_label("Chair – Dining").click();
    h.run();
    let chair = h
        .state()
        .editor
        .doc
        .block_named("Chair – Dining")
        .expect("library");
    assert_eq!(h.state().editor.doc.blocks().count(), 1, "only that one");
    // Place two chairs (the second turned with R), then a bed.
    assert_eq!(h.state().tool.kind(), ToolKind::Block);
    click_world(&mut h, DVec2::new(15.0, 70.0));
    let at = screen(&h, DVec2::new(105.0, 70.0));
    h.hover_at(at);
    h.run();
    h.key_press(Key::R);
    h.run();
    click_world(&mut h, DVec2::new(105.0, 70.0));
    let queen = crate::library::entry("Bed – Queen").unwrap();
    let bed = h.state_mut().editor.library_block(queen);
    // Clear of the chairs (the example is small; a bed isn't).
    h.state_mut()
        .editor
        .place_instance(bed, DVec2::new(5000.0, 0.0), 0.0, false);
    h.state_mut().editor.selected_instances.clear();
    assert_eq!(h.state().editor.doc.instances().count(), 3);
    let turned = h
        .state()
        .editor
        .doc
        .instances()
        .filter(|(_, i)| i.block == chair)
        .map(|(_, i)| i.angle)
        .collect::<Vec<_>>();
    assert!(
        turned.contains(&0.0) && turned.contains(&90.0),
        "{turned:?}"
    );
    h.key_press(Key::Escape);
    h.run();
    // Fit and pick a chair: the inspector shows the copy.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::F);
    h.run();
    h.run();
    h.get_by_label("Select").click();
    h.run();
    let first = h
        .state()
        .editor
        .doc
        .instances()
        .find(|(_, i)| i.block == chair && i.angle == 0.0)
        .map(|(_, i)| i.at)
        .unwrap();
    // On the left edge of the chair's seat, clear of the example.
    let seat = 9.0 * crate::library::INCH - 0.5 * crate::library::INCH;
    click_world(&mut h, first + DVec2::new(-seat, -300.0));
    assert_eq!(h.state().editor.selected_instances.len(), 1);
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    save_png("drift_ui_blocks.png", &mut h);
    // Edit the chair block in place: every copy updates.
    let copy = *h.state().editor.selected_instances.first().unwrap();
    h.state_mut().editor.edit_block(copy);
    h.run();
    let a = h.state_mut().editor.doc.add_point(DVec2::new(15.0, 70.0));
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let c = ed.doc.add_point(DVec2::new(15.0, 75.0));
    ed.doc.add_line(a, c).unwrap();
    ed.commit("Line");
    let before = h.state().editor.doc.block(chair).unwrap().items.len();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().editor.block_edit.is_none());
    assert_eq!(
        h.state().editor.doc.block(chair).unwrap().items.len(),
        before + 1
    );
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let dxf = h.state_mut().dxf_text(&painter);
    assert!(
        dxf.contains("BLOCKS") && dxf.matches("\nINSERT\n").count() == 3,
        "three inserts"
    );
    let pdf = h.state_mut().pdf_bytes(&painter);
    assert!(pdf.len() > 1000);
}

#[test]
fn pdf_underlay_calibrate_and_trace() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // A 1 : 50 plan on A3: a 100 × 60 mm room (5 × 3 m) with a door swing.
    let mut d = drift_draft::page::Drawing::new(v(420.0, 297.0));
    let pen = drift_draft::page::Pen::new(0.35);
    let corners = [
        v(50.0, 50.0),
        v(150.0, 50.0),
        v(150.0, 110.0),
        v(50.0, 110.0),
    ];
    for i in 0..4 {
        d.push(
            "0",
            pen,
            drift_draft::page::Prim::Line(corners[i], corners[(i + 1) % 4]),
        );
    }
    d.push(
        "0",
        pen,
        drift_draft::page::Prim::Arc {
            center: v(70.0, 50.0),
            radius: 16.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        },
    );
    let bytes = drift_io::pdf::write(&[d], "Plan");
    h.state_mut().begin_import(
        "plan.pdf",
        bytes,
        Some(crate::app::import_ui::ImportAs::Underlay),
        0.0,
    );
    h.run();
    h.get_by_label("Import").click();
    h.run();
    // No status toast: it would take the clicks below.
    h.state_mut().status = None;
    h.run();
    let (id, u) = {
        let doc = &h.state().editor.doc;
        let (id, u) = doc.underlays().next().expect("an underlay");
        (id, u.clone())
    };
    assert!((u.scale - drift_core::underlays::MM_PER_POINT).abs() < 1e-12);
    assert!(h.state().layout.is_shown(Panel::Layers));
    // The new underlay is picked; calibrate from its panel.
    h.get_by_label("Calibrate…").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Calibrate);
    click_world(&mut h, v(50.2, 49.9));
    click_world(&mut h, v(149.8, 50.1));
    type_text(&mut h, "5000");
    h.key_press(Key::Enter);
    h.run();
    let u = h.state().editor.doc.underlay(id).unwrap().clone();
    assert!(
        (u.scale / drift_core::underlays::MM_PER_POINT - 50.0).abs() < 1e-3,
        "{}",
        u.scale
    );
    // Fit, then trace the room's bottom wall with a line snapped to its corners.
    h.key_press(Key::Escape);
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::F);
    h.run();
    h.run();
    // The room's bottom wall, as the underlay has it (PDF coordinates are rounded).
    let Some(drift_core::Shape::Line { a, b }) = u.shapes().first().copied() else {
        panic!("the bottom wall")
    };
    assert!((a.distance(b) - 5000.0).abs() < 1e-6);
    h.key_press(Key::L);
    h.run();
    let near = |p: DVec2| p + v(30.0, -20.0);
    click_world(&mut h, near(a));
    click_world(&mut h, near(b));
    h.key_press(Key::Escape);
    h.run();
    let line = h
        .state()
        .editor
        .doc
        .iter()
        .find_map(|(_, e)| match e.kind {
            EntityKind::Line { a, b } => Some((a, b)),
            _ => None,
        })
        .expect("a traced line");
    let doc = &h.state().editor.doc;
    assert!(doc.point(line.0).unwrap().distance(a) < 1e-6);
    assert!(doc.point(line.1).unwrap().distance(b) < 1e-6);
    h.key_press(Key::S);
    h.run();
    save_png("drift_ui_underlay.png", &mut h);
}

#[test]
fn polygons_and_slots_from_the_toolbar() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    h.get_by_label("Polygon").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Polygon);
    click_world(&mut h, v(-60.0, 20.0));
    h.hover_at(screen(&h, v(-45.0, 26.0)));
    h.run();
    save_png("drift_ui_polygon_preview.png", &mut h);
    click_world(&mut h, v(-45.0, 26.0));
    // U: straight slot; U again: centre; again: arc.
    h.key_press(Key::U);
    h.run();
    click_world(&mut h, v(-20.0, -20.0));
    click_world(&mut h, v(20.0, -20.0));
    click_world(&mut h, v(0.0, -14.0));
    h.key_press(Key::U);
    h.run();
    h.key_press(Key::U);
    h.run();
    assert_eq!(h.state().editor.slot_mode, crate::tools::SlotMode::Arc);
    click_world(&mut h, v(40.0, 0.0));
    click_world(&mut h, v(70.0, 0.0));
    click_world(&mut h, v(40.0, 30.0));
    h.hover_at(screen(&h, v(74.0, 3.0)));
    h.run();
    click_world(&mut h, v(74.0, 3.0));
    let curves = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .count();
    // Hexagon (6 + circle), straight slot (2 lines, 2 arcs, axis), arc slot (4 arcs).
    assert_eq!(curves, 7 + 5 + 4);
    h.key_press(Key::Escape);
    h.run();
    save_png("drift_ui_polygon_slots.png", &mut h);
}

#[test]
fn c_cycles_to_ellipses_and_draws_them() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    for expected in [
        ToolKind::Circle,
        ToolKind::Ellipse,
        ToolKind::EllipseArc,
        ToolKind::Circle,
    ] {
        h.key_press(Key::C);
        h.run();
        assert_eq!(h.state().tool.kind(), expected);
    }
    h.key_press(Key::C);
    h.run();
    for at in [v(-40.0, 10.0), v(-10.0, 22.0), v(-36.0, 24.0)] {
        click_world(&mut h, at);
    }
    h.key_press(Key::C);
    h.run();
    for at in [v(30.0, -10.0), v(60.0, -10.0), v(30.0, 2.0), v(59.0, -6.0)] {
        click_world(&mut h, at);
    }
    h.hover_at(screen(&h, v(5.0, -5.0)));
    h.run();
    save_png("drift_ui_ellipse_preview.png", &mut h);
    click_world(&mut h, v(5.0, -5.0));
    let kinds: Vec<&str> = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(_, e)| e.kind.type_name())
        .collect();
    assert_eq!(kinds, ["Ellipse", "Elliptical arc"]);
    h.key_press(Key::Escape);
    h.run();
    save_png("drift_ui_ellipses.png", &mut h);
}

#[test]
fn pen_draws_curves_like_illustrator() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    h.key_press(Key::P);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Pen);
    click_world(&mut h, v(-60.0, -10.0));
    // Press, drag out the handles, and look mid-drag.
    let at = screen(&h, v(-20.0, 20.0));
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(screen(&h, v(-20.0 + 4.0 * k as f64, 20.0)));
        h.run();
    }
    save_png("drift_ui_pen_drag.png", &mut h);
    button(&h, screen(&h, v(-4.0, 20.0)), false);
    h.run();
    // The rubber band to the pointer.
    h.hover_at(screen(&h, v(30.0, -15.0)));
    h.run();
    save_png("drift_ui_pen_rubber_band.png", &mut h);
    click_world(&mut h, v(30.0, -15.0));
    h.key_press(Key::Enter);
    h.run();
    let curves = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Bezier { .. }))
        .count();
    assert_eq!(curves, 2);
    // Pick the first curve: its handles and comb show.
    h.key_press(Key::Escape);
    h.run();
    let on_curve = {
        let doc = &h.state().editor.doc;
        let (_, e) = doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Bezier { .. }))
            .unwrap();
        let EntityKind::Bezier { a, c1, c2, b } = e.kind else {
            unreachable!()
        };
        let p = [a, c1, c2, b].map(|q| doc.point(q).unwrap());
        drift_core::shape::bezier_point(&p, 0.5)
    };
    click_world(&mut h, on_curve);
    assert_eq!(h.state().editor.selection.len(), 1);
    assert_eq!(h.state().editor.handle_lines([0, 0, 0]).len(), 2);
    save_png("drift_ui_pen_picked.png", &mut h);
}

#[test]
fn end_points_light_up_and_hide_until_used() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    assert!(!h.state().always_endpoints, "Illustrator-like by default");
    // Untouched: no end point markers.
    let markers = |h: &Harness<'_, DriftApp>| {
        h.state()
            .editor
            .items_styled(true, true)
            .iter()
            .filter(|i| i.vertex)
            .count()
    };
    assert_eq!(markers(&h), 0);
    // Hover a corner where a line meets a fillet: it lights up.
    let corner = DVec2::new(10.0, 0.0);
    h.hover_at(screen(&h, corner));
    h.run();
    h.run();
    let hovered = h.state().editor.hovered;
    assert!(hovered.is_some_and(|id| {
        h.state()
            .editor
            .doc
            .get(id)
            .is_some_and(|e| e.kind.is_point())
    }));
    assert!(markers(&h) >= 1);
    save_png("drift_ui_endpoint_hover.png", &mut h);
    // Always show: every end point.
    h.state_mut().always_endpoints = true;
    h.run();
    assert!(markers(&h) > 10);
}

#[test]
fn view_and_tool_hotkeys() {
    let Some(mut h) = harness() else { return };
    // Alt+D / Alt+R hide and show dimensions and relations.
    let (dims, rels) = (h.state().show_dimensions, h.state().show_relations);
    h.key_press_modifiers(EguiModifiers::ALT, Key::D);
    h.run();
    h.key_press_modifiers(EguiModifiers::ALT, Key::R);
    h.run();
    assert_eq!(h.state().show_dimensions, !dims);
    assert_eq!(h.state().show_relations, !rels);
    assert_eq!(
        h.state().tool.kind(),
        ToolKind::Select,
        "Alt+D isn't the dimension tool"
    );
    // F: fillet, then chamfer, then fillet again.
    for expected in [ToolKind::Fillet, ToolKind::Chamfer, ToolKind::Fillet] {
        h.key_press(Key::F);
        h.run();
        assert_eq!(h.state().tool.kind(), expected);
    }
    // Shift+F fits (and doesn't change the tool).
    h.state_mut().fit_target = None;
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::F);
    h.run();
    assert!(h.state().fit_target.is_some());
    assert_eq!(h.state().tool.kind(), ToolKind::Fillet);
}

#[test]
fn text_tool_places_and_types_live() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // The toolbar's Text (the select tool's filter has a Text too).
    h.get_all_by_label("Text").last().unwrap().click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Text);
    click_world(&mut h, v(-40.0, 0.0));
    h.run();
    // The inspector's field is focused: typing goes straight in.
    type_text(&mut h, "Drift 2026");
    h.run();
    let (id, t) = {
        let doc = &h.state().editor.doc;
        let (id, t) = doc.texts().next().expect("a text");
        (id, t.clone())
    };
    assert_eq!(t.text, "Drift 2026");
    assert!(h.state().editor.text_items(true, true).len() > 20);
    // One undo step for the typing, one for placing it.
    // Drag it by its body with the select tool.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::S);
    h.run();
    let b = h.state().editor.text_outline(id).unwrap().bounds;
    let grab = b.center();
    let from = screen(&h, grab);
    let to = screen(&h, grab + v(0.0, -20.0));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(from + (to - from) * (k as f32 / 5.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    let moved = h.state().editor.doc.text(id).unwrap().at;
    assert!(moved.distance(t.at + v(0.0, -20.0)) < 1.0, "{moved:?}");
    save_png("drift_ui_text.png", &mut h);
}

#[test]
fn context_bar_canvas_menu_and_shortcuts() {
    let Some(mut h) = harness() else { return };
    // The polygon's options are in the context bar.
    h.key_press(Key::K);
    h.run();
    save_png("drift_ui_context_polygon.png", &mut h);
    h.get_by_label("Circumscribed").click();
    h.run();
    assert!(h.state().editor.polygon_circumscribed);
    // Draw a rectangle, then right-click one side: it's picked and the menu offers actions.
    h.key_press(Key::R);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -20.0));
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let at = screen(&h, DVec2::new(20.0, -20.0));
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
    h.run();
    assert_eq!(h.state().editor.selection.len(), 1);
    let line = *h.state().editor.selection.iter().next().unwrap();
    save_png("drift_ui_canvas_menu.png", &mut h);
    h.get_by_label_contains("Toggle construction").click();
    h.run();
    assert_eq!(
        h.state().editor.doc.get(line).unwrap().style,
        drift_core::LineKind::Construction
    );
    // The selection's actions are in the context bar.
    save_png("drift_ui_context_selection.png", &mut h);
    // ? opens the shortcuts, filtered as you type.
    h.event(Event::Text("?".into()));
    h.run();
    h.run();
    assert!(h.state().show_shortcuts);
    h.event(Event::Text("dimension".into()));
    h.run();
    h.run();
    save_png("drift_ui_shortcuts.png", &mut h);
}

#[test]
fn exports_never_become_the_documents_file() {
    use eframe::Storage as _;
    let Some(mut h) = harness() else { return };
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(25.0, 0.0));
    // A session left by a build that took an exported PDF for the sketch's own file.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    storage.set_string(crate::KEY_NAME, "Plan.pdf".into());
    storage.set_string(crate::KEY_PATH, "/home/me/Desktop/Plan.pdf".into());
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    // The name is a sketch's again, and Save can't overwrite the PDF.
    assert_eq!(fresh.state().file.name, "Plan.drift");
    assert!(fresh.state().file.path.is_none());
}

#[test]
fn long_text_and_long_histories_are_bounded() {
    let Some(mut h) = harness() else { return };
    let ed = &mut h.state_mut().editor;
    let id = ed.add_text(drift_core::SketchText::new("", DVec2::ZERO, 5.0));
    ed.type_text(id, &"x".repeat(50_000), false);
    assert_eq!(
        ed.doc.text(id).unwrap().text.chars().count(),
        crate::texts::MAX_TEXT_CHARS
    );
    // Far more edits than the history keeps: the oldest steps are forgotten.
    for k in 0..drift_core::History::MAX_STEPS + 50 {
        ed.update_text(id, |t| t.height = 5.0 + k as f64);
    }
    let mut undone = 0;
    while ed.undo() {
        undone += 1;
    }
    assert_eq!(undone, drift_core::History::MAX_STEPS);
}

#[test]
fn copy_paste_and_nudge_on_the_canvas() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Rectangle").click();
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -20.0));
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().editor.select_all();
    h.run();
    // Ctrl+C then Ctrl+V with the pointer elsewhere: a second rectangle centred there.
    h.event(Event::Copy);
    h.run();
    let copied = h.state().clipboard.clone().expect("copied");
    let copied = drift_io::save(&copied, &Default::default());
    let at = DVec2::new(70.0, 45.0);
    h.hover_at(screen(&h, at));
    h.run();
    h.event(Event::Paste(copied));
    h.run();
    assert_eq!(lines(&h), 8);
    let pasted: drift_core::Bounds = h
        .state()
        .editor
        .selection
        .iter()
        .filter_map(|id| h.state().editor.doc.shape(*id))
        .map(|s| s.bounds())
        .collect();
    assert!(
        ((pasted.min + pasted.max) * 0.5 - at).length() < 1.0,
        "{pasted:?}"
    );
    // The paste went straight into a move; Esc leaves the copy where it landed.
    assert!(h.state().tool.is_modal(), "moving the pasted copy");
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().tool.is_modal());
    assert_eq!(lines(&h), 8);
    // Arrow keys nudge the pasted copy by a grid step; Shift by ten.
    let step = drift_render::grid_info(&h.state().camera, false).minor;
    h.key_press(Key::ArrowRight);
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::ArrowUp);
    h.run();
    let moved: drift_core::Bounds = h
        .state()
        .editor
        .selection
        .iter()
        .filter_map(|id| h.state().editor.doc.shape(*id))
        .map(|s| s.bounds())
        .collect();
    assert!((moved.min - pasted.min - DVec2::new(step, 10.0 * step)).length() < 1e-6);
    // Plain text pastes as sketch text.
    h.event(Event::Paste("Kitchen".into()));
    h.run();
    assert_eq!(h.state().editor.doc.texts().count(), 1);
    save_png("drift_ui_pasted.png", &mut h);
}

#[test]
fn sizes_typed_in_the_inspector_become_dimensions() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let (line, a, b) = h
        .state()
        .editor
        .doc
        .iter()
        .find_map(|(id, e)| match e.kind {
            EntityKind::Line { a, b } => Some((id, a, b)),
            _ => None,
        })
        .unwrap();
    let ed = &mut h.state_mut().editor;
    let wanted = Constraint::Distance { a, b, value: 0.0 };
    // No dimension yet: typing a length adds one; typing again changes it.
    assert!(ed.size_dimension(&wanted).is_none());
    let id = ed.set_size(wanted, "80").unwrap();
    assert!((ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap()) - 80.0).abs() < 1e-6);
    assert_eq!(ed.set_size(wanted, "2in").unwrap(), id);
    assert!((ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap()) - 50.8).abs() < 1e-6);
    assert_eq!(
        ed.doc
            .constraints()
            .filter(|(_, c)| c.value().is_some())
            .count(),
        1
    );
    // The field is in the inspector.
    ed.selection = [line].into();
    h.run();
    h.run();
    save_png("drift_ui_inspector_sizes.png", &mut h);
    // A circle's radius typed where its diameter is dimensioned changes that dimension.
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 40.0));
    click_world(&mut h, DVec2::new(10.0, 40.0));
    h.key_press(Key::Escape);
    h.run();
    let circle = h
        .state()
        .editor
        .doc
        .iter()
        .find_map(|(id, e)| matches!(e.kind, EntityKind::Circle { .. }).then_some(id))
        .unwrap();
    let ed = &mut h.state_mut().editor;
    let d = ed
        .set_size(
            Constraint::Diameter {
                curve: circle,
                value: 0.0,
            },
            "30",
        )
        .unwrap();
    let r = ed
        .set_size(
            Constraint::Radius {
                curve: circle,
                value: 0.0,
            },
            "25",
        )
        .unwrap();
    assert_eq!(d, r);
    assert!((drift_solver::radius_of(&ed.doc, circle).unwrap() - 25.0).abs() < 1e-6);
}

#[test]
fn start_panel_recent_files_and_the_quit_prompt() {
    use eframe::Storage as _;
    let Some(mut h) = harness() else { return };
    h.state_mut().show_start = true;
    // A sketch on disk, opened by its path as from the command line.
    let dir = std::env::temp_dir().join("drift-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Recent test.drift");
    let text = drift_io::save(&drift_core::demo::mounting_plate(), &Default::default());
    std::fs::write(&path, text).unwrap();
    h.state_mut().remember(path.clone());
    h.run();
    h.run();
    save_png("drift_ui_start.png", &mut h);
    // The panel lists it; clicking opens it and the panel goes.
    h.get_by_label("Recent test").click();
    h.run();
    h.run();
    assert!(!h.state().editor.doc.is_empty());
    assert_eq!(h.state().file.path.as_ref(), Some(&path));
    assert!(!h.state().show_start);
    // Closing with unsaved changes asks; "Don't save" lets the window go and drops the
    // session, keeping the recent list.
    h.state_mut().editor.select_all();
    h.state_mut().editor.delete_selection();
    assert!(h.state().is_dirty());
    h.state_mut().pending = Some(crate::Pending::Quit);
    h.run();
    save_png("drift_ui_quit_prompt.png", &mut h);
    h.get_by_label("Don't save").click();
    h.run();
    assert!(h.state().quit_confirmed && h.state().discard_session);
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    assert_eq!(storage.get_string(crate::KEY_DOCUMENT).as_deref(), Some(""));
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    assert!(fresh.state().editor.doc.is_empty());
    assert_eq!(fresh.state().recent, std::slice::from_ref(&path));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn text_is_typed_in_place_and_windows_open() {
    let Some(mut h) = harness() else { return };
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::T);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Text);
    click_world(&mut h, DVec2::new(-20.0, 10.0));
    h.run();
    assert!(h.state().editor.editing_text.is_some());
    h.event(Event::Text("Hall".into()));
    h.run();
    h.run();
    save_png("drift_ui_text_in_place.png", &mut h);
    let texts: Vec<String> = h
        .state()
        .editor
        .doc
        .texts()
        .map(|(_, t)| t.text.clone())
        .collect();
    assert_eq!(texts, ["Hall"]);
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().editor.editing_text.is_none());
    // Zoom presets, and zoom to the selection.
    h.state_mut().zoom_to_percent(100.0);
    assert!((h.state().camera.scale - 96.0 / 25.4).abs() < 1e-9);
    assert!(h.state_mut().zoom_to_selection());
    // Preferences and About open from their commands.
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    save_png("drift_ui_preferences.png", &mut h);
    h.state_mut().show_settings = false;
    h.state_mut().show_about = true;
    h.run();
    h.run();
    save_png("drift_ui_about.png", &mut h);
    // About → Licences shows the third-party notices.
    h.get_by_label("Licences…").click();
    h.run();
    h.run();
    h.run();
    h.run();
    assert!(h.state().show_licences);
    h.get_by_label("Ubuntu Light (Ubuntu-Light.ttf)");
    save_png("drift_ui_licences.png", &mut h);
}

#[test]
fn inspector_fields_take_typed_values() {
    use eframe::egui::accesskit::Role;
    let Some(mut h) = harness() else { return };
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 10.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(5.0, 7.0));
    h.run();
    assert_eq!(h.state().editor.selection.len(), 1);
    // The line's fields, in order: Length, Angle, Start X, Start Y, End X, End Y.
    fn type_into(h: &mut Harness<'_, DriftApp>, field: usize, text: &str, finish: Key) {
        let fields: Vec<_> = h.get_all_by_role(Role::TextInput).collect();
        // (Once the length is dimensioned, that dimension's own row adds a seventh below.)
        assert!(fields.len() >= 6, "a line's six fields");
        fields[field].click();
        h.run();
        h.event(Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: EguiModifiers::COMMAND,
        });
        h.event(Event::Text(text.into()));
        h.run();
        h.key_press(finish);
        h.run();
        h.run();
    }
    let ends = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        doc.iter()
            .find_map(|(_, e)| match e.kind {
                EntityKind::Line { a, b } => Some((doc.point(a).unwrap(), doc.point(b).unwrap())),
                _ => None,
            })
            .unwrap()
    };
    // A length, leaving the field with Tab rather than Return: it still applies, as a
    // dimension.
    type_into(&mut h, 0, "80", Key::Tab);
    let (a, b) = ends(&h);
    assert!(
        (a.distance(b) - 80.0).abs() < 1e-6,
        "length {}",
        a.distance(b)
    );
    assert_eq!(h.state().editor.doc.constraint_count(), 1);
    // An angle turns the line about its start, keeping the length.
    type_into(&mut h, 1, "90", Key::Enter);
    let (a2, b2) = ends(&h);
    assert!((a2 - a).length() < 1e-6, "the start stays");
    assert!((b2 - (a + DVec2::new(0.0, 80.0))).length() < 1e-5, "{b2:?}");
    // A coordinate moves that end; the dimensioned length holds, so the line follows.
    type_into(&mut h, 2, "10", Key::Enter);
    let (a3, b3) = ends(&h);
    assert!((a3.x - 10.0).abs() < 1e-6, "{a3:?}");
    assert!((a3.distance(b3) - 80.0).abs() < 1e-6);
    save_png("drift_ui_field_typed.png", &mut h);
    // Esc leaves things as they were.
    type_into(&mut h, 3, "55", Key::Escape);
    assert_eq!(ends(&h), (a3, b3));
    assert_eq!(
        h.state().editor.selection.len(),
        1,
        "Esc in a field keeps the selection"
    );
    // In feet and inches a bare number is inches.
    let mut units = h.state().editor.doc.units();
    units.length = drift_core::LengthFormat::Architectural;
    h.state_mut().editor.set_units(units);
    h.run();
    type_into(&mut h, 0, "30", Key::Enter);
    let (a4, b4) = ends(&h);
    assert!(
        (a4.distance(b4) - 762.0).abs() < 1e-6,
        "{}",
        a4.distance(b4)
    );
    type_into(&mut h, 0, "2'-6 1/2\"", Key::Enter);
    let (a5, b5) = ends(&h);
    assert!(
        (a5.distance(b5) - 774.7).abs() < 1e-6,
        "{}",
        a5.distance(b5)
    );
}

#[test]
fn offsets_and_splits_of_ellipses_and_pen_curves_draw() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let [c, m, n] = [v(-45.0, 0.0), v(-15.0, 10.0), v(-50.0, 15.0)].map(|p| ed.doc.add_point(p));
    let ellipse = ed.doc.add_ellipse(c, m, n).unwrap();
    let pts =
        [v(15.0, -20.0), v(15.0, 25.0), v(60.0, 30.0), v(70.0, -5.0)].map(|p| ed.doc.add_point(p));
    let arch = ed.doc.add_bezier(pts[0], pts[1], pts[2], pts[3]).unwrap();
    let tail = ed.doc.add_point(v(90.0, -30.0));
    let line = ed.doc.add_line(pts[3], tail).unwrap();
    ed.commit("Shapes");
    // Offsets on both sides of each, then a split of the ellipse and of the pen curve.
    let centre = ed.doc.point(c).unwrap();
    ed.offset(ellipse, 6.0, centre + v(80.0, 0.0)).unwrap();
    ed.offset(ellipse, 5.0, centre).unwrap();
    ed.offset(line, 5.0, v(40.0, 40.0)).unwrap();
    ed.offset(line, 4.0, v(40.0, 0.0)).unwrap();
    ed.split_at(ellipse, centre + v(30.0, 10.0)).unwrap();
    let at = ed
        .doc
        .shape(arch)
        .map(|s| crate::modify::point_at(&s, 0.4))
        .unwrap();
    ed.split_at(arch, at).unwrap();
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    h.state_mut().always_endpoints = true;
    h.state_mut().status_colors = false;
    h.run();
    h.run();
    save_png("drift_ui_offset_split_curves.png", &mut h);
}

#[test]
fn modern_sheet_layout() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().add_sheet();
    let id = h.state().current_sheet().expect("a sheet");
    let mut sheet = h.state().editor.doc.sheet(id).cloned().unwrap();
    assert_eq!(sheet.title_block, drift_core::TitleBlock::Modern);
    sheet.fields.title = "Mounting plate".into();
    sheet.fields.number = "DR-0142".into();
    sheet.fields.company = "Northlight Works".into();
    sheet.fields.author = "C. Lange".into();
    sheet.fields.checked = "M. Okafor".into();
    sheet.fields.material = "6061-T6, anodised".into();
    sheet.fields.date = "2026-10-11".into();
    sheet.name = "General arrangement".into();
    for (rev, text) in [
        ("A", "First issue"),
        ("B", "Slot lengthened to 30; holes on Ø52"),
    ] {
        sheet.revisions.push(drift_core::Revision {
            rev: rev.into(),
            date: "2026-10-11".into(),
            description: text.into(),
            by: "CL".into(),
        });
    }
    h.state_mut().update_sheet(id, sheet, None, false);
    h.state_mut().sheet_preview = true;
    h.state_mut().preview.zoom = 0.0;
    h.state_mut().dock_view.hide_all = true;
    h.run();
    h.run();
    save_png("drift_ui_sheet_modern.png", &mut h);
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/BaseFont /Helvetica-Bold"));
    assert!(text.contains("(Mounting plate) Tj") && text.contains("(DR-0142) Tj"));
    assert!(
        text.contains("(NORTHLIGHT WORKS) Tj"),
        "the company, in tracked capitals"
    );
    std::fs::write(dir.join("drift_sheet_modern.pdf"), &pdf).unwrap();
    std::fs::write(
        dir.join("drift_sheet_modern.svg"),
        h.state_mut().svg_text(&painter),
    )
    .unwrap();
    // The same on A4 upright, where the strip has far less room.
    let mut narrow = h.state().editor.doc.sheet(id).cloned().unwrap();
    narrow.paper = drift_core::PaperSize::A4;
    narrow.portrait = true;
    let bounds = h.state().fit_bounds();
    // Its view filling the new drawing area, the drawing fitted in it.
    let mut view = placed(&h, id)[0].clone();
    let area = drift_draft::sheet::drawing_area(&narrow);
    view.frame = drift_core::ViewFrame {
        at: area.center(),
        size: area.size(),
    };
    crate::output::fit_view(bounds, &mut view, false);
    h.state_mut()
        .update_sheet(id, narrow, Some((0, view)), false);
    h.run();
    std::fs::write(
        dir.join("drift_sheet_modern_a4.pdf"),
        h.state_mut().pdf_bytes(&painter),
    )
    .unwrap();
}

#[test]
fn control_bar_overlap_warning_and_corner_angles() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let ed = &mut h.state_mut().editor;
    // Two rooms side by side, each with its own copy of the wall between them.
    ed.doc.begin();
    let mut walls = Vec::new();
    for x in [-60.0, -20.0] {
        let c = [
            v(x, -20.0),
            v(x + 40.0, -20.0),
            v(x + 40.0, 20.0),
            v(x, 20.0),
        ]
        .map(|p| ed.doc.add_point(p));
        for i in 0..4 {
            walls.push(ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
        }
    }
    // A corner whose second line is stored pointing into it: a 40° corner.
    let apex = ed.doc.add_point(v(40.0, -20.0));
    let along = ed.doc.add_point(v(90.0, -20.0));
    let up = ed
        .doc
        .add_point(v(40.0, -20.0) + 50.0 * DVec2::from_angle(40f64.to_radians()));
    let base = ed.doc.add_line(apex, along).unwrap();
    let arm = ed.doc.add_line(up, apex).unwrap();
    ed.commit("Rooms");
    // Each room fills on its own, though the wall between them is doubled.
    let regions = ed.regions();
    let mut areas: Vec<f64> = regions.iter().map(|r| r.area).collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!(areas.iter().all(|a| (a - 1600.0).abs() < 1e-6));
    // The doubled wall is reported: the later-made of the pair.
    let overlaps = ed.overlapping();
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0], walls[7], "the second room's left wall");
    // An angle dimension measures the corner as drawn (40°, not 140°), whichever way the
    // lines are stored, and sits between them.
    let (a, b) = crate::annotations::corner_order(&ed.doc, base, arm);
    let angle = Constraint::Angle { a, b, degrees: 0.0 };
    let measured = drift_solver::measure(&ed.doc, &angle).unwrap();
    assert!((measured - 40.0).abs() < 1e-9, "{measured}");
    let id = ed
        .add_dimension(
            Constraint::Angle {
                a,
                b,
                degrees: measured,
            },
            None,
        )
        .unwrap();
    let sectors = crate::annotations::angle_sectors(&ed.doc, a, b, measured).unwrap();
    let inside = DVec2::from_angle(20f64.to_radians());
    assert!(
        sectors[0].bisector().dot(inside) > 0.999,
        "{:?}",
        sectors[0]
    );
    assert!(ed.doc.constraint(id).is_some());
    // Pick a wall: the bar shows its layer, stroke and fill, and the settings at the right.
    ed.selection = [walls[0]].into();
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    h.run();
    h.run();
    save_png("drift_ui_control_bar.png", &mut h);
    {
        use eframe::egui::accesskit::Role;
        h.get_by_role_and_label(Role::Button, "Stroke");
        h.get_by_role_and_label(Role::Button, "Fill");
    }
    // The tool's hint, in the middle of the status bar, stops short of every switch, however
    // wide the right-hand group is (with this warning showing, and wider fonts, it used to lie
    // over the leftmost ones and take their clicks).
    let hint = h.get_by_label(&h.state().tool.hint()).rect();
    for name in [
        "Grid",
        "Origin axes",
        "End points",
        "Auto relations",
        "Status colours",
    ] {
        let r = h.get_by_label(name).rect();
        assert!(
            hint.right() <= r.left(),
            "the hint {hint:?} over {name} {r:?}"
        );
    }
    // The setting is one click away, and the warning selects the doubled wall.
    h.get_by_label("Auto relations").click();
    h.run();
    assert!(!h.state().auto_relations);
    h.state_mut().select_overlapping(0.0);
    assert_eq!(
        h.state()
            .editor
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [walls[7]]
    );
}

#[test]
fn a_wheel_zooms_and_a_trackpad_pans() {
    use crate::app::view::WheelMode;
    let Some(mut h) = harness() else { return };
    let at = h.state().canvas_rect.center() + egui::vec2(60.0, 40.0);
    let scroll = |h: &mut Harness<'static, DriftApp>, unit, delta| {
        h.hover_at(at);
        h.run();
        let before = h.state().camera;
        h.event(Event::MouseWheel {
            unit,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        });
        // The scroll is smoothed over a few frames.
        h.run();
        h.run();
        (before, h.state().camera)
    };
    use egui::MouseWheelUnit::{Line, Point};
    // A wheel notch zooms about the pointer.
    let (before, after) = scroll(&mut h, Line, egui::vec2(0.0, 1.0));
    assert!(after.scale > before.scale * 1.01, "zoomed in");
    // Two fingers on a trackpad (points; or fractional, sideways lines) pan: right and down
    // moves the drawing right and down.
    for (unit, delta) in [
        (Point, egui::vec2(30.0, 20.0)),
        (Line, egui::vec2(0.4, 0.3)),
    ] {
        let (before, after) = scroll(&mut h, unit, delta);
        assert_eq!(after.scale, before.scale, "{unit:?}");
        assert!(after.center.x < before.center.x && after.center.y > before.center.y);
    }
    // The preference overrides the guess either way.
    h.state_mut().wheel.mode = WheelMode::Zoom;
    let (before, after) = scroll(&mut h, Point, egui::vec2(0.0, 30.0));
    assert!(after.scale > before.scale && h.state().view_prefs().contains("wheel=zoom"));
    h.state_mut().wheel.mode = WheelMode::Pan;
    let (before, after) = scroll(&mut h, Line, egui::vec2(0.0, 1.0));
    assert_eq!(after.scale, before.scale);
    assert!(after.center.y > before.center.y);
    // Ctrl + scroll still zooms.
    h.hover_at(at);
    let before = h.state().camera.scale;
    h.event(Event::MouseWheel {
        unit: Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: EguiModifiers::CTRL,
    });
    h.run();
    h.run();
    assert!(h.state().camera.scale > before);
    h.state_mut().apply_view_prefs("wheel=auto");
    assert_eq!(h.state().wheel.mode, WheelMode::Auto);
}

#[test]
fn hatch_handles_move_and_turn_the_pattern() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Rectangle").click();
    h.run();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    click_world(&mut h, DVec2::new(70.0, 50.0));
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.run();
    let hatch = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().editor;
        let id = *ed
            .selected_fills
            .iter()
            .next()
            .expect("the hatch is picked");
        match &ed.doc.fill(id).unwrap().kind {
            drift_core::FillKind::Hatch(hatch) => hatch.clone(),
            other => panic!("{other:?}"),
        }
    };
    let drag = |h: &mut Harness<'static, DriftApp>, from: Pos2, to: Pos2| {
        h.hover_at(from);
        h.run();
        button(h, from, true);
        h.run();
        for k in 1..=5 {
            h.hover_at(from + (to - from) * (k as f32 / 5.0));
            h.run();
        }
        button(h, to, false);
        h.run();
    };
    // A new hatch's origin is the sketch's, outside the area: the handle shows in the
    // middle. Dragged to a corner, the origin snaps onto it.
    assert_eq!(hatch(&h).origin, DVec2::ZERO);
    let map = screen_map(&h);
    let handles = h.state_mut().hatch_handles(&map).expect("handles");
    assert!(
        handles.at.distance(DVec2::new(40.0, 30.0)) < 1e-3,
        "{:?}",
        handles.at
    );
    let corner = screen(&h, DVec2::new(10.0, 10.0));
    drag(&mut h, handles.origin, corner + egui::vec2(2.0, -1.0));
    let origin = hatch(&h).origin;
    assert!(origin.distance(DVec2::new(10.0, 10.0)) < 1e-3, "{origin:?}");
    assert!(
        h.state()
            .editor
            .doc
            .iter()
            .any(|(_, e)| e.kind == EntityKind::Point(origin)),
        "exactly on the corner point"
    );
    assert_eq!(h.state().editor.selected_fills.len(), 1, "still picked");
    // The round handle turns the pattern, in 15 degree steps.
    let map = screen_map(&h);
    let handles = h.state_mut().hatch_handles(&map).expect("handles");
    assert_eq!(handles.at, origin);
    let before = hatch(&h).angle;
    let to = handles.origin + egui::vec2(40.0, -22.0);
    drag(&mut h, handles.angle, to);
    let after = hatch(&h).angle;
    assert!(
        (after - 30.0).abs() < 1e-9 && after != before,
        "{before} -> {after}"
    );
    h.run();
    save_png("drift_ui_hatch_handles.png", &mut h);
    // Each drag is one undo step.
    h.state_mut().undo();
    assert_eq!(hatch(&h).angle, before);
    h.state_mut().undo();
    assert_eq!(hatch(&h).origin, DVec2::ZERO);
}

#[test]
fn control_bar_sets_stroke_weight_and_line_type() {
    use egui_kittest::kittest::Queryable as _;
    let Some(mut h) = harness() else { return };
    h.get_by_label("Line").click();
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 0.0));
    click_world(&mut h, DVec2::new(30.0, 0.0));
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(12.0, 0.0));
    h.run();
    let look = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().editor;
        let id = *ed.selection.iter().next().expect("the line is picked");
        ed.doc.get(id).unwrap().appearance
    };
    assert!(look(&h).is_by_layer());
    save_png("drift_ui_control_bar_stroke.png", &mut h);
    // The bar's menus, in order: layer, stroke weight, line type.
    let menu = |h: &mut Harness<'static, DriftApp>, index: usize, item: &str| {
        let mut combos: Vec<_> = h
            .get_all_by_role(egui::accesskit::Role::ComboBox)
            .filter(|c| c.rect().top() < 64.0)
            .collect();
        combos.sort_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
        combos[index].click();
        h.run();
        // (The inspector has "By layer" menus too: the bar's is the one on the left.)
        h.get_all_by_label(item)
            .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
            .expect("the menu's row")
            .click();
        h.run();
        h.run();
    };
    menu(&mut h, 1, "0.50 mm");
    assert_eq!(look(&h).weight, Some(drift_core::LineWeight(50)));
    menu(&mut h, 2, "Hidden");
    assert_eq!(look(&h).line_type, Some(drift_core::LineType::Hidden));
    save_png("drift_ui_control_bar_stroke_set.png", &mut h);
    // Back to the layer's.
    menu(&mut h, 1, "By layer");
    assert_eq!(look(&h).weight, None);
    assert_eq!(look(&h).line_type, Some(drift_core::LineType::Hidden));
}

#[test]
fn relations_pie_toggles_relations_on_the_selection() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // Two lines, a little off parallel.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [
        v(-40.0, -10.0),
        v(20.0, -8.0),
        v(-40.0, 15.0),
        v(25.0, 20.0),
    ]
    .map(|at| ed.doc.add_point(at));
    let a = ed.doc.add_line(p[0], p[1]).unwrap();
    let b = ed.doc.add_line(p[2], p[3]).unwrap();
    ed.commit("Lines");
    h.run();
    let has = |h: &Harness<'_, DriftApp>, want: fn(&Constraint) -> bool| {
        h.state().editor.doc.constraints().any(|(_, c)| want(c))
    };
    let parallel = |c: &Constraint| matches!(c, Constraint::Parallel { .. });
    let equal = |c: &Constraint| matches!(c, Constraint::Equal { .. });
    let at = h.state().canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.state_mut().editor.selection = [a, b].into();
    h.run();

    // Hold Q, move up and to the right (Parallel's place), let go: a flick.
    h.key_down(Key::Q);
    h.run();
    let center = h.state().pie.as_ref().expect("the pie opens").center;
    assert_eq!(center, at);
    let slices = h.state().pie_slices();
    let labels: Vec<_> = slices
        .iter()
        .map(|s| (s.label, s.option.is_some(), s.on))
        .collect();
    assert_eq!(
        labels,
        [
            ("Horizontal", true, false),
            ("Parallel", true, false),
            ("Vertical", true, false),
            ("Perpendicular", true, false),
            ("Equal", true, false),
            ("Fix", true, false),
            ("Collinear", true, false),
            ("Midpoint", false, false),
        ]
    );
    h.hover_at(center + egui::vec2(60.0, -60.0));
    h.run();
    save_png("drift_ui_relations_pie.png", &mut h);
    h.key_up(Key::Q);
    h.run();
    assert!(h.state().pie.is_none());
    assert!(has(&h, parallel), "parallel added");
    assert_eq!(h.state().editor.selection.len(), 2, "still picked");

    // A tap of Q leaves the ring open: Parallel is lit, and Perpendicular would now
    // over-define the sketch (it says so; choosing it is refused and nothing changes).
    h.hover_at(at);
    h.run();
    h.key_press(Key::Q);
    h.run();
    assert!(h.state().pie.as_ref().is_some_and(|p| !p.held));
    assert!(h.state().pie_slices()[1].on);
    // While it is open, other keys are its own: D doesn't start the dimension tool.
    h.key_press(Key::D);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert!(h.state().pie.is_some());
    // Click Equal (to the left).
    let left = at - egui::vec2(110.0, 0.0);
    h.hover_at(left);
    h.run();
    button(&h, left, true);
    h.run();
    button(&h, left, false);
    h.run();
    assert!(h.state().pie.is_none());
    assert!(has(&h, equal), "equal added");
    assert_eq!(
        h.state().editor.selection.len(),
        2,
        "the click didn't reach the canvas"
    );

    // Flick towards Parallel again: it comes off (one undo step puts it back).
    h.hover_at(at);
    h.run();
    h.key_down(Key::Q);
    h.run();
    h.hover_at(at + egui::vec2(60.0, -60.0));
    h.run();
    h.key_up(Key::Q);
    h.run();
    assert!(!has(&h, parallel) && has(&h, equal));
    h.state_mut().undo();
    assert!(has(&h, parallel));

    // Esc closes it, and so does Q again; neither changes anything.
    let before = h.state().editor.doc.constraint_count();
    h.key_press(Key::Q);
    h.run();
    assert!(h.state().pie.is_some());
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().pie.is_none());
    h.key_press(Key::Q);
    h.run();
    h.key_press(Key::Q);
    h.run();
    assert!(h.state().pie.is_none());
    assert_eq!(h.state().editor.doc.constraint_count(), before);
    assert_eq!(h.state().editor.selection.len(), 2);

    // A relation's letter chooses it outright: tap Q, then E takes Equal off; hold Q and
    // press E to put it back, and let go of Q without choosing anything else.
    h.key_press(Key::Q);
    h.run();
    h.key_press(Key::E);
    h.run();
    assert!(h.state().pie.is_none());
    assert!(!has(&h, equal) && has(&h, parallel));
    h.key_down(Key::Q);
    h.run();
    h.hover_at(at + egui::vec2(60.0, -60.0));
    h.run();
    h.key_press(Key::E);
    h.run();
    h.key_up(Key::Q);
    h.run();
    assert!(has(&h, equal) && has(&h, parallel), "only Equal changed");
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    // One that doesn't fit leaves the ring open and says so.
    h.key_press(Key::Q);
    h.run();
    h.key_press(Key::C);
    h.run();
    assert!(h.state().pie.is_some());
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    h.key_press(Key::Escape);
    h.run();
}

#[test]
fn a_radius_typed_for_a_fillet_keeps_its_lines() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // A plain corner, filleted with automatic relations off: nothing holds the lines.
    let ed = &mut h.state_mut().editor;
    ed.auto_relations = false;
    ed.doc.begin();
    let corner = ed.doc.add_point(v(0.0, 0.0));
    let ends = [v(60.0, 10.0), v(-5.0, 40.0)].map(|at| ed.doc.add_point(at));
    ed.doc.add_line(corner, ends[0]).unwrap();
    ed.doc.add_line(corner, ends[1]).unwrap();
    ed.commit("Corner");
    let arc = ed.fillet(corner, 6.0).unwrap();
    assert_eq!(ed.doc.constraint_count(), 0);
    // The radius typed in the inspector becomes a dimension; the fillet takes that size
    // where it is.
    let wanted = drift_core::Constraint::Radius {
        curve: arc,
        value: 0.0,
    };
    let id = ed.set_size(wanted, "10").unwrap();
    let f = ed.fillet_at(arc).expect("still a fillet");
    assert!((f.radius - 10.0).abs() < 1e-7);
    assert!(
        f.corner.length() < 1e-7,
        "the lines still meet where they did"
    );
    assert!(kept(&f));
    // And editing that dimension afterwards, the same.
    ed.set_dimension(id, 4.0).unwrap();
    let f = ed.fillet_at(arc).expect("still a fillet");
    assert!((f.radius - 4.0).abs() < 1e-7 && f.corner.length() < 1e-7);
    assert!(kept(&f));
    /// The lines' far ends are where they were (in either order).
    fn kept(f: &crate::modify::FilletInfo) -> bool {
        [DVec2::new(60.0, 10.0), DVec2::new(-5.0, 40.0)]
            .iter()
            .all(|end| {
                f.arms.iter().any(|a| {
                    let far = f.corner + a.dir(f.corner) * a.length;
                    far.distance(*end) < 1e-9
                })
            })
    }
}

#[test]
fn a_sheet_holds_several_views() {
    use crate::app::sheets_ui::{ViewGrip, dragged_view};
    use drift_core::ViewFrame;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    let sheet = |h: &Harness<'_, DriftApp>| h.state().editor.doc.sheet(id).cloned().unwrap();
    assert_eq!(placed(&h, id).len(), 1);
    assert!(
        drift_draft::sheet::fills_drawing_area(&sheet(&h), placed(&h, id)[0].frame.bounds()),
        "the first view fills the sheet"
    );

    // Add view: its crop is dragged out in the model (round the bore) with the View tool; it
    // goes on the sheet in a frame of its own where there is room, titled, and picked.
    add_view_by_region(&mut h, DVec2::new(55.0, 25.0), DVec2::new(100.0, 58.0));
    assert!(h.state().sheet_preview, "back on the sheet");
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    let s = sheet(&h);
    let views = placed(&h, id);
    assert_eq!(views.len(), 2);
    let added = views[1].clone();
    assert!(added.title && added.name == "View 2");
    assert_eq!(h.state().sheet_view, 1);
    assert_eq!(h.state().picked_view, Some(added.id));
    let area = drift_draft::sheet::view_area(&s);
    let b = added.frame.bounds();
    assert!(
        b.min.x >= area.min.x && b.max.x <= area.max.x,
        "on the sheet"
    );
    assert!(b.min.y >= area.min.y && b.max.y <= area.max.y);

    // A detail at another scale: the title block says so.
    let mut detail = added.clone();
    detail.scale = 0.5;
    detail.name = "Detail A".into();
    detail.frame = ViewFrame {
        at: DVec2::new(330.0, 200.0),
        size: DVec2::new(100.0, 80.0),
    };
    detail.center = DVec2::new(84.0, 40.0);
    detail.border = true;
    set_placed(&mut h, id, 1, &detail, "Detail");
    // Its crop in the model: the frame at 2:1.
    let crop = h.state().editor.doc.view(detail.id).unwrap().crop();
    assert_eq!(crop.size(), DVec2::new(50.0, 40.0));
    h.run();
    h.run();
    save_png("drift_ui_sheet_views.png", &mut h);
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("(Detail A) Tj"), "the view's title");
    assert!(text.contains("(SCALE 2:1) Tj"), "its scale under it");
    assert!(
        text.contains("(As shown) Tj"),
        "mixed scales in the title block"
    );
    // Each view is seen through its own window.
    let windows = text.matches(" re W n").count();
    assert!(windows >= 2, "{windows} clip windows");
    let svg = h.state_mut().svg_text(&painter);
    assert!(svg.contains(r#"<clipPath id="view2">"#));

    // On the sheet: a press inside the detail takes hold of it (to move it on the paper);
    // with Alt, of the drawing inside it. Its handles crop it.
    let rect = h.state().canvas_rect;
    let s = sheet(&h);
    let views = placed(&h, id);
    let app = h.state();
    let zoom = f64::from(app.preview.zoom);
    let to_screen = |p: DVec2| {
        let d = (p - app.preview.center) * zoom;
        rect.center() + egui::vec2(d.x as f32, -d.y as f32)
    };
    let inside = to_screen(DVec2::new(330.0, 200.0));
    assert_eq!(
        app.view_grip_at(rect, &s, &views, inside, false),
        Some((1, ViewGrip::Move))
    );
    assert_eq!(
        app.view_grip_at(rect, &s, &views, inside, true),
        Some((1, ViewGrip::Pan))
    );
    let corner = to_screen(DVec2::new(380.0, 240.0));
    assert_eq!(
        app.view_grip_at(rect, &s, &views, corner, false),
        Some((1, ViewGrip::Resize(1, 1)))
    );
    // Outside it: the view that fills the sheet, which stays put; Alt-drag pans it.
    let elsewhere = to_screen(DVec2::new(100.0, 150.0));
    assert_eq!(
        app.view_grip_at(rect, &s, &views, elsewhere, false),
        Some((0, ViewGrip::Fixed))
    );
    assert_eq!(
        app.view_grip_at(rect, &s, &views, elsewhere, true),
        Some((0, ViewGrip::Pan))
    );

    // Drag the detail 30 mm to the left on the paper (Ctrl held: freely, not snapping).
    let from = inside;
    let to = inside - egui::vec2((30.0 * zoom) as f32, 0.0);
    h.event(Event::ModifiersChanged(EguiModifiers::COMMAND));
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    let moved = placed(&h, id)[1].clone();
    let at = moved.frame.at;
    assert!(
        (at.x - 300.0).abs() < 0.5 && (at.y - 200.0).abs() < 0.5,
        "{at:?}"
    );
    assert_eq!(moved.center, detail.center, "the drawing moved with it");
    h.state_mut().undo();
    assert_eq!(placed(&h, id)[1].frame, detail.frame);

    // Cropping: the upper right corner pulled in by (20, 10) mm. The frame shrinks about
    // its other corner, and the drawing stays where it was on the paper.
    let was = &detail;
    let cropped = dragged_view(was, ViewGrip::Resize(1, 1), DVec2::new(-20.0, -10.0));
    let f = cropped.frame;
    assert_eq!(f.size, DVec2::new(80.0, 70.0));
    assert_eq!(f.bounds().min, was.frame.bounds().min);
    let on_paper =
        |v: &drift_core::PlacedView, world: DVec2| v.frame.at + (world - v.center) / v.scale;
    let probe = DVec2::new(90.0, 45.0);
    assert!(on_paper(&cropped, probe).distance(on_paper(was, probe)) < 1e-9);
    // Never inside out.
    let tiny = dragged_view(was, ViewGrip::Resize(1, 1), DVec2::new(-500.0, -500.0));
    assert_eq!(tiny.frame.size, DVec2::splat(10.0));

    // Delete takes the picked view off the sheet, down to none; the views stay in the model.
    assert_eq!(h.state().sheet_view, 1);
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(placed(&h, id).len(), 1);
    h.key_press(Key::Delete);
    h.run();
    assert!(placed(&h, id).is_empty(), "a sheet can be empty");
    assert_eq!(h.state().editor.doc.views().count(), 2);
    h.run();
    assert_eq!(
        h.state().editor.doc.sheet_scale_text(&sheet(&h)),
        "-",
        "no scale"
    );
    h.state_mut().undo();
    h.state_mut().undo();
    assert_eq!(placed(&h, id).len(), 2);
}

#[test]
fn sheets_from_before_views_open_as_they_were() {
    // A v17 sheet: one view written as the sheet's own scale and centre, with dimensions off.
    let text = r#"{
  "format": "drift",
  "version": 17,
  "meta": {"units":"mm"},
  "entities": [],
  "sheets": [
    {"id":"s1","name":"Plate","paper":"A3","scale":2.5,"center":[60.0,40.0],"title_block":"modern","dimensions":false}
  ]
}"#;
    let (doc, _) = drift_io::load(text).unwrap();
    let sheet = doc.sheets().next().unwrap().1;
    let views = doc.placed_views(sheet);
    assert_eq!(views.len(), 1);
    let v = &views[0];
    assert_eq!((v.scale, v.center), (2.5, DVec2::new(60.0, 40.0)));
    assert!(!v.dimensions && v.notes && !v.title);
    // It fills the drawing area, as it did, and is named after its sheet.
    assert_eq!(v.frame.bounds(), drift_draft::sheet::drawing_area(sheet));
    assert_eq!(v.name, "Plate");
    assert_eq!(doc.sheet_scale_text(sheet), "1:2.5");
    // Saved again, it is written as a model view, placed on the sheet.
    let saved = drift_io::save(&doc, &Default::default());
    assert!(
        saved.contains(r#""name":"Plate","scale":2.5,"center":[60.0,40.0],"size":"#),
        "{saved}"
    );
    assert!(saved.contains(r#""viewports":[{"view":"w"#), "{saved}");
    let (again, _) = drift_io::load(&saved).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn section_and_elevation_marks() {
    use drift_core::Annotation;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let marks = |h: &Harness<'_, DriftApp>| -> Vec<Annotation> {
        h.state()
            .editor
            .doc
            .annotations()
            .map(|(_, a)| a.clone())
            .collect()
    };
    // A section across the plate: two clicks. It gets the first free letter.
    h.get_by_label("Section mark").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Section);
    click_world(&mut h, DVec2::new(0.0, 40.0));
    assert!(h.state().tool.is_busy());
    click_world(&mut h, DVec2::new(120.0, 40.0));
    match marks(&h).as_slice() {
        [
            Annotation::Section {
                a,
                b,
                label,
                sheet,
                flip,
                view: None,
            },
        ] => {
            assert!(
                a.distance(DVec2::new(0.0, 40.0)) < 1.0
                    && b.distance(DVec2::new(120.0, 40.0)) < 1.0
            );
            assert_eq!((label.as_str(), sheet.as_str(), *flip), ("A", "", false));
        }
        other => panic!("{other:?}"),
    }
    click_world(&mut h, DVec2::new(60.0, 0.0));
    click_world(&mut h, DVec2::new(60.0, 80.0));
    assert!(matches!(&marks(&h)[1], Annotation::Section { label, .. } if label == "B"));

    // An elevation mark: where it goes, then towards what it looks at (as pointed, with
    // angle snapping off).
    h.get_by_label("Elevation mark").click();
    h.run();
    click_world(&mut h, DVec2::new(130.0, -10.0));
    click_world(&mut h, DVec2::new(110.0, 10.0));
    let id = match marks(&h).as_slice() {
        [
            ..,
            Annotation::Elevation {
                at, angle, label, ..
            },
        ] => {
            assert!(at.distance(DVec2::new(130.0, -10.0)) < 1.0);
            assert!((*angle - 135.0).abs() < 0.05, "{angle}");
            assert_eq!(label.as_str(), "1");
            h.state()
                .editor
                .selected_annotation
                .expect("the new mark is picked")
        }
        other => panic!("{other:?}"),
    };
    // The inspector names the sheet it is drawn on.
    let mut named = h.state().editor.doc.annotation(id).cloned().unwrap();
    if let Annotation::Elevation { sheet, .. } = &mut named {
        *sheet = "A-201".into();
    }
    h.state_mut().editor.doc.begin();
    h.state_mut()
        .editor
        .doc
        .set_annotation(id, named.clone())
        .unwrap();
    h.state_mut().editor.commit("Sheet");
    h.get_by_label("Select").click();
    h.run();
    h.state_mut().fit_target = Some(h.state().fit_bounds());
    h.run();
    h.run();
    save_png("drift_ui_marks.png", &mut h);

    // With the select tool a mark is dragged whole.
    let from = screen(&h, DVec2::new(40.0, 40.0));
    let to = from + egui::vec2(0.0, -30.0);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    match &marks(&h)[0] {
        Annotation::Section { a, b, .. } => {
            assert!(
                a.y > 41.0 && (a.y - b.y).abs() < 1e-9,
                "moved up, still level"
            );
            assert!((b.x - a.x - 120.0).abs() < 1.0, "the same length");
        }
        other => panic!("{other:?}"),
    }
    h.state_mut().undo();

    // They are saved, and print with the notes.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""type":"section""#) && saved.contains(r#""sheet":"A-201""#));
    let (loaded, _) = drift_io::load(&saved).unwrap();
    let back: Vec<Annotation> = loaded.annotations().map(|(_, a)| a.clone()).collect();
    assert_eq!(back, marks(&h));
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("(A-201) Tj") && text.contains("(B) Tj"),
        "marks print"
    );
}

#[test]
fn a_logo_goes_in_the_title_block() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let before = h.state_mut().svg_text(&painter);
    assert!(!before.contains("#ff6600"));
    // A mark and a wordmark's worth of shapes: a disc with a hole, and a bar.
    let logo = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 60 20">
  <path fill="#ff6600" fill-rule="evenodd" d="M10 0a10 10 0 1 0 .001 0zM10 5a5 5 0 1 1-.001 0z"/>
  <rect x="26" y="6" width="34" height="8" fill="#111114"/>
</svg>"##;
    // What can't be read is refused, and nothing changes.
    assert!(
        h.state_mut()
            .set_logo("<svg><text>Acme</text></svg>")
            .is_err()
    );
    assert!(h.state().editor.doc.logo().is_empty());
    h.state_mut().set_logo(logo).unwrap();
    h.run();
    h.run();
    save_png("drift_ui_sheet_logo.png", &mut h);
    let svg = h.state_mut().svg_text(&painter);
    assert!(
        svg.contains(r##"fill="#ff6600" fill-rule="evenodd""##),
        "the logo is drawn"
    );
    // In the strip, at the left, as large as its room (16 mm tall, 44 mm wide) allows.
    let page = h.state_mut().editor.doc.sheets().next().unwrap().1.clone();
    let border = drift_draft::sheet::border(&page);
    let parsed = drift_draft::logo::parse(logo).unwrap();
    let mut d = drift_draft::page::Drawing::new(page.size());
    drift_draft::sheet::draw_frame(
        &page,
        &Default::default(),
        "1:1",
        (1, 1),
        Some(&parsed),
        &mut d,
    );
    let orange = d
        .items
        .iter()
        .find(|i| i.pen.color == drift_core::Rgb([0xff, 0x66, 0x00]))
        .expect("the mark");
    let drift_draft::page::Prim::Fill { rings, .. } = &orange.prim else {
        panic!("a fill");
    };
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for q in rings.iter().flatten() {
        lo = lo.min(*q);
        hi = hi.max(*q);
    }
    // (This one is three times as wide as tall, so the 44 mm of width decides.)
    assert!((hi.y - lo.y - 44.0 / 3.0).abs() < 0.05, "{lo:?} {hi:?}");
    assert!(lo.x >= border.min.x + 4.9 && lo.y >= border.min.y + 3.9);
    // It is kept in the file, and is one undo step.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert_eq!(loaded.logo(), logo);
    h.state_mut().undo();
    assert!(h.state().editor.doc.logo().is_empty());
}

#[test]
fn text_can_use_the_computers_fonts() {
    use crate::Editor;
    use drift_core::{SketchText, TextFont};
    let mut ed = Editor::default();
    let outline_of = |ed: &mut Editor, font: TextFont| {
        ed.doc.begin();
        let mut text = SketchText::new("Rag", DVec2::ZERO, 10.0);
        text.font = font;
        let id = ed.doc.add_text(text);
        ed.commit("Text");
        ed.text_outline(id).expect("an outline")
    };
    let sans = outline_of(&mut ed, TextFont::Sans);

    // A font that isn't installed: the text is drawn in the built-in sans, and says so.
    let missing = TextFont::System("No Such Typeface 123".into());
    let fallback = outline_of(&mut ed, missing.clone());
    assert_eq!(fallback.contours, sans.contours);
    assert_eq!(
        drift_draft::outline::font_known("No Such Typeface 123"),
        Some(false)
    );
    // It keeps its name in the file all the same.
    let saved = drift_io::save(&ed.doc, &Default::default());
    assert!(
        saved.contains(r#""font":"system:No Such Typeface 123""#),
        "{saved}"
    );
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert!(loaded.texts().any(|(_, t)| t.font == missing));
    assert_eq!(TextFont::from_name("system:"), None);
    assert_eq!(TextFont::from_name("serif"), None);

    // One that is: its own outlines. (Whatever this computer has; none in a bare CI image.)
    let families = drift_platform::fonts::families();
    let Some(family) = ["Arial", "Helvetica", "DejaVu Sans", "Liberation Sans"]
        .into_iter()
        .find(|f| families.iter().any(|x| x == f))
        .map(str::to_owned)
        .or_else(|| families.first().cloned())
    else {
        eprintln!("no system fonts here; skipping the rest");
        return;
    };
    assert!(
        families
            .windows(2)
            .all(|w| w[0].to_lowercase() <= w[1].to_lowercase())
    );
    let system = outline_of(&mut ed, TextFont::System(family.clone()));
    assert_eq!(
        drift_draft::outline::font_known(&family),
        Some(true),
        "{family}"
    );
    assert!(!system.contours.is_empty());
    assert_ne!(
        system.contours, sans.contours,
        "{family} is not the built-in sans"
    );
    // Capitals are as tall as asked, whatever the font.
    let height = system.bounds.max.y - system.bounds.min.y;
    assert!(height > 9.0 && height < 16.0, "{height}");
}

#[test]
fn angle_snap_is_on_the_status_bar() {
    let Some(mut h) = harness() else { return };
    assert_eq!(h.state().angle_snap, None);
    h.get_by_label("Angle snap").click();
    h.run();
    save_png("drift_ui_angle_snap.png", &mut h);
    h.get_by_label("Every 15°").click();
    h.run();
    assert_eq!(h.state().angle_snap, Some(15.0));
    assert!(h.state().view_prefs().contains("asnap=15"));
    // A line drawn with it on goes at a multiple of 15 degrees.
    h.get_by_label("Line").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(40.0, 21.0));
    h.key_press(Key::Escape);
    h.run();
    let ed = &h.state().editor;
    let (a, b) = ed
        .doc
        .iter()
        .find_map(|(id, _)| match ed.doc.shape(id) {
            Some(drift_core::Shape::Line { a, b }) => Some((a, b)),
            _ => None,
        })
        .expect("a line");
    let degrees = (b - a).y.atan2((b - a).x).to_degrees();
    assert!((degrees - 30.0).abs() < 1e-6, "{degrees}");
    // Off again from the same menu.
    h.get_by_label("Angle snap").click();
    h.run();
    h.get_by_label("Off").click();
    h.run();
    assert_eq!(h.state().angle_snap, None);
}

/// Geometry an import puts far off is listed in its report, with a way to pick it; an edit
/// that puts geometry far off (a slipped exponent) is warned about in the status bar.
#[test]
fn far_off_geometry_is_reported() {
    let Some(mut h) = harness() else { return };
    let line = |x: &'static str| {
        [
            (0, "LINE"),
            (8, "0"),
            (10, x),
            (20, "0"),
            (11, "100"),
            (21, "50"),
        ]
    };
    let mut pairs = vec![(0, "SECTION"), (2, "ENTITIES")];
    pairs.extend(line("0"));
    pairs.extend(line("5e12"));
    pairs.extend([(0, "ENDSEC"), (0, "EOF")]);
    let text: String = pairs
        .iter()
        .map(|(c, v)| {
            format!(
                "{c:>3}
{v}
"
            )
        })
        .collect();
    h.state_mut()
        .import_now("stray.dxf", text.clone().into_bytes(), 0.0);
    h.run_steps(3);
    let report = h.state().import_report.clone().expect("a report");
    assert_eq!(report.far.len(), 1);
    let stray = report.far[0];
    // The import says so in its report, not also in the status bar.
    let status = h.state().status.as_ref().map(|s| s.text.clone());
    assert!(
        status
            .as_ref()
            .is_some_and(|t| t.starts_with("Imported 2 curves")),
        "{status:?}"
    );
    h.get_by_label("Select them").click();
    h.run_steps(2);
    assert!(h.state().import_report.is_none());
    assert_eq!(
        h.state()
            .editor
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![stray]
    );

    // An edit: a circle typed 1e12 across.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let c = ed.doc.add_point(DVec2::ZERO);
    ed.doc.add_circle(c, 1e12).unwrap();
    assert!(ed.commit("Circle"));
    h.run();
    let status = h.state().status.as_ref().expect("a warning");
    assert!(status.error);
    assert!(
        status
            .text
            .contains("100,000 km from the origin now: 1 curve."),
        "{}",
        status.text
    );
    // Ordinary edits don't warn.
    h.state_mut().status = None;
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let c = ed.doc.add_point(DVec2::new(10.0, 10.0));
    ed.doc.add_circle(c, 5.0).unwrap();
    assert!(ed.commit("Circle"));
    h.run();
    assert!(h.state().editor.far_off.is_empty());
    assert!(h.state().status.is_none());
}

#[test]
fn a_dxf_from_a_live_project_comes_in_whole() {
    let Some(mut h) = harness() else { return };
    let pairs: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$INSUNITS"),
        (70, "4"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (2, "*D1"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (8, "0"),
        (10, "0"),
        (20, "-10"),
        (11, "60"),
        (21, "-10"),
        (0, "MTEXT"),
        (8, "0"),
        (10, "30"),
        (20, "-9"),
        (40, "2.5"),
        (71, "8"),
        (1, "60"),
        (0, "ENDBLK"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        // A room, hatched, with a round column left clear.
        (0, "LWPOLYLINE"),
        (8, "WALLS"),
        (90, "4"),
        (70, "1"),
        (10, "0"),
        (20, "0"),
        (10, "60"),
        (20, "0"),
        (10, "60"),
        (20, "40"),
        (10, "0"),
        (20, "40"),
        (0, "CIRCLE"),
        (8, "WALLS"),
        (10, "30"),
        (20, "20"),
        (40, "6"),
        (0, "HATCH"),
        (8, "FLOOR"),
        (2, "ANSI31"),
        (70, "0"),
        (91, "2"),
        (92, "3"),
        (72, "0"),
        (73, "1"),
        (93, "4"),
        (10, "0"),
        (20, "0"),
        (10, "60"),
        (20, "0"),
        (10, "60"),
        (20, "40"),
        (10, "0"),
        (20, "40"),
        (97, "0"),
        (92, "16"),
        (93, "1"),
        (72, "2"),
        (10, "30"),
        (20, "20"),
        (40, "6"),
        (50, "0"),
        (51, "360"),
        (73, "1"),
        (97, "0"),
        (75, "0"),
        (76, "1"),
        (52, "0"),
        (41, "1"),
        (98, "0"),
        (0, "SPLINE"),
        (8, "WALLS"),
        (71, "3"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (10, "70"),
        (20, "0"),
        (10, "80"),
        (20, "20"),
        (10, "90"),
        (20, "20"),
        (10, "100"),
        (20, "0"),
        (0, "MTEXT"),
        (8, "NOTES"),
        (10, "0"),
        (20, "55"),
        (40, "3"),
        (71, "7"),
        (1, "ROOM 101\\PSee detail"),
        (0, "DIMENSION"),
        (8, "DIMS"),
        (2, "*D1"),
        (70, "32"),
        (0, "3DSOLID"),
        (8, "0"),
        // On the layout: a title, and a window onto the room at 1:2.
        (0, "TEXT"),
        (67, "1"),
        (8, "TITLE"),
        (10, "10"),
        (20, "10"),
        (40, "5"),
        (1, "PLAN"),
        (0, "VIEWPORT"),
        (67, "1"),
        (10, "150"),
        (20, "100"),
        (40, "60"),
        (41, "40"),
        (69, "2"),
        (68, "2"),
        (12, "30"),
        (22, "20"),
        (45, "80"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    let text: String = pairs
        .iter()
        .map(|(c, v)| format!("{c:>3}\n{v}\n"))
        .collect();
    h.state_mut()
        .import_now("room.dxf", text.clone().into_bytes(), 0.0);
    h.run_steps(3);
    let app = h.state_mut();
    let doc = &app.editor.doc;
    // Text: the paragraph's two lines and the dimension's number.
    let words: Vec<String> = doc.texts().map(|(_, t)| t.text.clone()).collect();
    assert!(
        words.contains(&"ROOM 101\nSee detail".to_owned()),
        "{words:?}"
    );
    assert!(words.contains(&"60".to_owned()));
    // The layout is a sheet with one view at 1:2; its title text isn't in the model.
    assert!(!words.contains(&"PLAN".to_owned()));
    let sheet = doc.sheets().next().expect("a sheet").1.clone();
    let views = doc.placed_views(&sheet);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].scale, 2.0);
    assert_eq!(views[0].center, DVec2::new(30.0, 20.0));
    // The hatch is a live fill: the app finds its area among the sketch's regions, with
    // the column as a hole, and it follows the walls.
    let fills: Vec<drift_core::FillId> = doc.fills().map(|(id, _)| id).collect();
    assert_eq!(fills.len(), 1);
    let shapes = app.editor.fill_shapes();
    let shape = &shapes[&fills[0]];
    assert!(!shape.lost, "the fill found its boundary");
    assert_eq!(shape.region.holes.len(), 1);
    let floor = 60.0 * 40.0 - std::f64::consts::PI * 36.0;
    assert!(
        (shape.region.area - floor).abs() < 1e-3,
        "{}",
        shape.region.area
    );
    // The room's outline is there once, not doubled by the hatch.
    assert!(app.editor.overlapping().is_empty());
    // What couldn't come in is reported, and stays up.
    let report = app.import_report.clone().expect("a report");
    assert!(report.summary.contains("1 hatch or fill") && report.summary.contains("1 sheet"));
    assert_eq!(report.skipped, vec!["1 × 3DSOLID".to_owned()]);
    assert!(report.notes.iter().any(|n| n.contains("1 dimension")));
    assert!(report.notes.iter().any(|n| n.contains("drawn on layouts")));
    h.run_steps(2);
    save_png("drift_ui_dxf_import.png", &mut h);
    // One undo takes the whole import out.
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.texts().count(), 0);
    assert_eq!(h.state().editor.doc.sheets().count(), 0);
}

#[test]
fn the_drawing_font_is_kept_in_the_drawing_and_used_everywhere() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A font file added to the drawing (the built-in monospaced one stands in for a file
    // the user picked): it is kept under its own family name.
    let hack = drift_draft::outline::builtin_font(true);
    let family = h.state_mut().editor.add_font_file(hack.to_vec()).unwrap();
    assert_eq!(family, "Hack");
    assert!(
        h.state_mut()
            .editor
            .add_font_file(b"not a font".to_vec())
            .is_err()
    );
    // A second one that nothing will be set in.
    let ubuntu = drift_draft::outline::builtin_font(false);
    assert_eq!(
        h.state_mut().editor.add_font_file(ubuntu.to_vec()).unwrap(),
        "Ubuntu"
    );
    // Nothing is set in it yet: the standard look.
    assert!(h.state().editor.drawing_font().is_none());
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let standard = h.state_mut().pdf_bytes(&painter);
    assert!(String::from_utf8_lossy(&standard).contains("/BaseFont /Helvetica"));

    // Make it the drawing's font: dimensions, notes and the title block are set in it.
    assert_eq!(
        h.state_mut().editor.set_drawing_font(Some("Hack".into())),
        None
    );
    let font = h.state().editor.drawing_font().expect("the drawing's font");
    assert_eq!(font.regular.family, "Hack");
    // The screen's text renderer takes it over a frame.
    h.run();
    h.run();
    assert!(matches!(
        h.state().screen_font,
        crate::fonts::ScreenFont::Ready(_)
    ));
    assert_eq!(
        crate::fonts::drafting_font(12.0, false).family,
        egui::FontFamily::Name("drawing".into())
    );
    save_png("drift_ui_drawing_font_sheet.png", &mut h);
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.starts_with("%PDF-1.6") && text.contains("/BaseFont /Hack"));
    assert!(text.contains("/Subtype /Type0") && !text.contains("/Helvetica"));
    assert!(
        pdf.len() > standard.len() + 50_000,
        "the font is in the file"
    );
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/drift_font.pdf"),
        &pdf,
    )
    .unwrap();
    let svg = h.state_mut().svg_text(&painter);
    assert!(svg.contains("font-family=\"'Hack', Helvetica"));
    // In the model too: a dimension's number is laid out in it.
    h.key_press(Key::Escape);
    h.run();
    h.run();
    save_png("drift_ui_drawing_font_model.png", &mut h);

    // Saved with the drawing: the font in use goes into the file, the unused one doesn't.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""drawing":"Hack""#));
    assert!(saved.contains(r#""family":"Hack""#) && !saved.contains(r#""family":"Ubuntu""#));
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert_eq!(loaded.fonts().drawing.as_deref(), Some("Hack"));
    assert_eq!(loaded.fonts().embedded["Hack"].regular.data, hack);
    // Opened where the font isn't installed, it is still there to draw with.
    let mut elsewhere = crate::Editor::new(loaded);
    assert_eq!(elsewhere.drawing_font().unwrap().regular.family, "Hack");
    assert_eq!(elsewhere.drawing_font_missing(), None);
    // A name with no font behind it falls back to the standard look, and says so.
    let mut fonts = elsewhere.doc.fonts().clone();
    fonts.drawing = Some("No Such Typeface 123".into());
    elsewhere.doc.begin();
    elsewhere.doc.set_fonts(fonts);
    elsewhere.commit("Font");
    assert!(elsewhere.drawing_font().is_none());
    assert_eq!(
        elsewhere.drawing_font_missing().as_deref(),
        Some("No Such Typeface 123")
    );

    // Back to the standard look is one undo step, and one choice.
    h.state_mut().undo();
    assert!(h.state().editor.drawing_font().is_none());
    h.state_mut().redo();
    h.state_mut().editor.set_drawing_font(None);
    h.run();
    h.run();
    assert_eq!(h.state().screen_font, crate::fonts::ScreenFont::Standard);
    assert_eq!(
        crate::fonts::drafting_font(12.0, false).family,
        egui::FontFamily::Proportional
    );
}

#[test]
fn sketch_text_runs_to_several_lines() {
    use drift_core::{SketchText, TextAlign};
    let Some(mut h) = harness() else { return };
    // Place a text and type two lines: Shift+Enter starts the second, Enter finishes.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::T);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Text);
    click_world(&mut h, DVec2::new(-20.0, 10.0));
    h.run();
    let id = h.state().editor.editing_text.expect("typing in place");
    h.event(Event::Text("PLAN".into()));
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::Enter);
    h.run();
    assert!(h.state().editor.editing_text.is_some(), "still typing");
    h.event(Event::Text("Level 2".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().editor.editing_text.is_none(), "Enter finishes");
    let text = h.state().editor.doc.text(id).cloned().unwrap();
    assert_eq!(text.text, "PLAN\nLevel 2");
    assert_eq!(
        (text.align, text.spacing, text.wrap),
        (TextAlign::Left, SketchText::SPACING, None)
    );
    // The outlines cover both lines: the second a line spacing below the first.
    let one_line = text.height;
    let bounds = h.state().editor.text_outline(id).unwrap().bounds;
    assert!(bounds.max.y - bounds.min.y > one_line * 2.0);
    assert!(
        bounds.min.y < text.at.y - one_line,
        "the second line hangs below the anchor"
    );

    // Centred: the lines sit about the anchor. Wrapped: more lines, within the width.
    let width_of = |h: &Harness<'_, DriftApp>| {
        let b = h.state().editor.text_outline(id).unwrap().bounds;
        (b.min.x, b.max.x, b.min.y)
    };
    let (left, right, low) = width_of(&h);
    h.state_mut()
        .editor
        .update_text(id, |t| t.align = TextAlign::Center);
    let (cl, cr, _) = width_of(&h);
    assert!(
        ((cl + cr) / 2.0 - text.at.x).abs() < 0.5,
        "centred on the anchor"
    );
    assert!(((cr - cl) - (right - left)).abs() < 1e-6);
    h.state_mut().editor.update_text(id, |t| {
        t.align = TextAlign::Left;
        t.text = "one two three four five six".into();
        t.wrap = Some((right - left) * 0.9);
    });
    let (wl, wr, wrapped_low) = width_of(&h);
    assert!(wr - wl <= (right - left) * 0.9 + 1e-6);
    assert!(wrapped_low < low, "wrapping made more lines");
    h.run();
    save_png("drift_ui_multiline_text.png", &mut h);

    // Saved and loaded as it is.
    h.state_mut().editor.update_text(id, |t| {
        t.align = TextAlign::Right;
        t.spacing = 2.0;
    });
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""align":"right""#) && saved.contains(r#""spacing":2.0"#));
    assert!(saved.contains(r#""wrap":"#));
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert_eq!(loaded.text(id), h.state().editor.doc.text(id));
    // A plain one-line text writes none of it.
    let mut plain = drift_core::Document::new();
    plain.add_text(SketchText::new("A", DVec2::ZERO, 3.0));
    let saved = drift_io::save(&plain, &Default::default());
    assert!(!saved.contains("align") && !saved.contains("spacing") && !saved.contains("wrap"));
}

/// The middle of the caret before character `gap` of a text, in the drawing.
fn text_gap(h: &Harness<'_, DriftApp>, id: drift_core::TextId, gap: usize) -> DVec2 {
    let t = h.state().editor.doc.text(id).unwrap();
    let [foot, head] = drift_draft::outline::Paragraph {
        text: &t.text,
        font: &t.font,
        height: t.height,
        at: t.at,
        angle: t.angle,
        align: t.align,
        spacing: t.spacing,
        wrap: t.wrap,
    }
    .caret(gap);
    (foot + head) / 2.0
}

#[test]
fn text_is_typed_straight_onto_the_canvas() {
    let Some(mut h) = harness() else { return };
    let words = |h: &Harness<'_, DriftApp>, id| h.state().editor.doc.text(id).unwrap().text.clone();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::T);
    h.run();
    click_world(&mut h, DVec2::new(-60.0, 10.0));
    h.run();
    let id = h.state().editor.editing_text.expect("typing in place");
    assert_eq!(
        h.state().text_cursor,
        Some((0, 0)),
        "a caret in the empty text"
    );
    h.event(Event::Text("HELLO WORLD".into()));
    h.run();
    assert_eq!(words(&h, id), "HELLO WORLD");
    assert_eq!(
        h.state().text_cursor,
        Some((11, 11)),
        "the caret follows the typing"
    );
    // No field shows: the keyboard's field is out of sight.
    assert!(h.query_by_label_contains("Type the text").is_none());

    // The arrow keys move the caret; typing goes in where it is.
    for _ in 0..5 {
        h.key_press(Key::ArrowLeft);
        h.run();
    }
    assert_eq!(h.state().text_cursor, Some((6, 6)));
    h.event(Event::Text("BIG ".into()));
    h.run();
    assert_eq!(words(&h, id), "HELLO BIG WORLD");

    // A click in the text puts the caret at the nearest gap, and the typing goes on.
    let at = text_gap(&h, id, 5) + DVec2::new(0.3, 0.0);
    click_world(&mut h, at);
    h.run();
    assert_eq!(h.state().editor.editing_text, Some(id), "still typing");
    assert_eq!(h.state().text_cursor, Some((5, 5)));
    h.event(Event::Text("!".into()));
    h.run();
    assert_eq!(words(&h, id), "HELLO! BIG WORLD");
    assert_eq!(
        h.state().editor.doc.texts().count(),
        1,
        "the click made no new text"
    );

    // End, a new line, and Up and Down by the lines on the canvas.
    h.key_press(Key::End);
    h.run();
    assert_eq!(h.state().text_cursor, Some((16, 16)));
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::Enter);
    h.run();
    h.event(Event::Text("ab".into()));
    h.run();
    assert_eq!(words(&h, id), "HELLO! BIG WORLD\nab");
    assert_eq!(h.state().text_cursor, Some((19, 19)));
    h.key_press(Key::ArrowUp);
    h.run();
    let (up, _) = h.state().text_cursor.unwrap();
    assert!(
        (1..=3).contains(&up),
        "above the end of the second line: {up}"
    );
    h.key_press(Key::Home);
    h.run();
    assert_eq!(h.state().text_cursor, Some((0, 0)));
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::ArrowDown);
    h.run();
    assert_eq!(
        h.state().text_cursor,
        Some((17, 0)),
        "Shift selects as it goes"
    );
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        h.state().text_cursor,
        Some((19, 19)),
        "off the bottom: the end"
    );

    // A double click picks a word, and typing replaces it.
    let at = screen(&h, text_gap(&h, id, 8));
    h.hover_at(at);
    h.run();
    // (The harness's frames are a quarter of a second apart: allow that between clicks.)
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 5.0);
    for _ in 0..2 {
        button(&h, at, true);
        h.step();
        button(&h, at, false);
        h.step();
    }
    h.run();
    assert_eq!(h.state().text_cursor, Some((10, 7)), "BIG");
    assert_eq!(h.state().editor.editing_text, Some(id));
    h.event(Event::Text("SMALL".into()));
    h.run();
    assert_eq!(words(&h, id), "HELLO! SMALL WORLD\nab");

    // A drag selects from where it began to where it is.
    let (from, to) = (
        screen(&h, text_gap(&h, id, 7) + DVec2::new(0.3, 0.0)),
        screen(&h, text_gap(&h, id, 12) + DVec2::new(0.3, 0.0)),
    );
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    h.hover_at(to);
    h.run();
    assert_eq!(h.state().text_cursor, Some((12, 7)));
    button(&h, to, false);
    h.run();
    h.run();
    assert_eq!(
        h.state().text_cursor,
        Some((12, 7)),
        "kept when the button comes up"
    );
    assert_eq!(h.state().editor.editing_text, Some(id));
    save_png("drift_ui_live_text.png", &mut h);
    // Shift and a click selects up to the click.
    let at = screen(&h, text_gap(&h, id, 18) + DVec2::new(0.3, 0.0));
    h.hover_at(at);
    h.run();
    h.event_modifiers(
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().text_cursor, Some((18, 7)));

    // Enter finishes. All of it was one undo step after the placing.
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().editor.editing_text, None);
    assert_eq!(
        words(&h, id),
        "HELLO! SMALL WORLD\nab",
        "Enter adds no line"
    );
    h.state_mut().undo();
    assert_eq!(words(&h, id), "");
    h.state_mut().redo();

    // The text tool on a text types into it, from the click; a press elsewhere finishes.
    let at = text_gap(&h, id, 3) + DVec2::new(0.3, 0.0);
    click_world(&mut h, at);
    h.run();
    assert_eq!(h.state().editor.editing_text, Some(id));
    assert_eq!(h.state().text_cursor, Some((3, 3)));
    assert_eq!(h.state().editor.doc.texts().count(), 1);
    h.key_press(Key::S);
    h.event(Event::Text("S".into()));
    h.run();
    assert_eq!(
        h.state().tool.kind(),
        ToolKind::Text,
        "S was typed, not a shortcut"
    );
    assert_eq!(words(&h, id), "HELSLO! SMALL WORLD\nab");
    let away = screen(&h, DVec2::new(40.0, -60.0));
    h.hover_at(away);
    h.run();
    button(&h, away, true);
    h.run();
    assert_ne!(
        h.state().editor.editing_text,
        Some(id),
        "the press ended the typing"
    );
    button(&h, away, false);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert_eq!(h.state().editor.editing_text, None);

    // Too small on screen to aim a caret at: a field under the text takes the typing.
    h.state_mut().camera.scale = 0.2;
    h.run();
    h.state_mut().editor.selected_texts = [id].into();
    h.state_mut().editor.editing_text = Some(id);
    h.run();
    h.run();
    assert_eq!(h.state().editor.editing_text, Some(id));
    h.event(Event::Text("!".into()));
    h.run();
    assert_eq!(words(&h, id), "HELSLO! SMALL WORLD\nab!");
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().editor.editing_text, None);
}

#[test]
fn transform_handles_on_the_canvas() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    // A 40 × 24 rectangle with square corners, picked.
    let c = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let c = [
            DVec2::new(-20.0, -12.0),
            DVec2::new(20.0, -12.0),
            DVec2::new(20.0, 12.0),
            DVec2::new(-20.0, 12.0),
        ]
        .map(|p| ed.doc.add_point(p));
        let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
        for (i, line) in l.iter().enumerate() {
            let r = if i % 2 == 0 {
                Constraint::Horizontal { line: *line }
            } else {
                Constraint::Vertical { line: *line }
            };
            ed.doc.add_constraint(r).unwrap();
        }
        ed.commit("Setup");
        ed.selection = l.iter().copied().collect();
        c
    };
    h.run();
    save_png("drift_ui_transform_box.png", &mut h);
    let cursor = |h: &Harness<'_, DriftApp>| h.output().platform_output.cursor_icon;

    // The top-right handle sits the pick radius outside the corner, on screen.
    let handle = screen(&h, DVec2::new(20.0, 12.0)) + egui::vec2(8.0, -8.0);
    h.hover_at(handle);
    h.run();
    assert_eq!(cursor(&h), egui::CursorIcon::ResizeNeSw);
    let side = screen(&h, DVec2::new(-20.0, 0.0)) + egui::vec2(-8.0, 0.0);
    h.hover_at(side);
    h.run();
    assert_eq!(cursor(&h), egui::CursorIcon::ResizeHorizontal);

    // Dragged half as far again from the opposite corner: it grows by half about it.
    let corner = screen(&h, DVec2::new(-20.0, -12.0));
    let to = corner + (handle - corner) * 1.5;
    h.hover_at(handle);
    h.run();
    button(&h, handle, true);
    h.run();
    h.hover_at(to);
    h.run();
    assert!(h.state().tool.is_busy(), "scaling");
    button(&h, to, false);
    h.run();
    h.run();
    let at = |h: &Harness<'_, DriftApp>, i: usize| h.state().editor.doc.point(c[i]).unwrap();
    assert!(
        at(&h, 0).distance(DVec2::new(-20.0, -12.0)) < 1e-6,
        "the anchor stays"
    );
    assert!(
        at(&h, 2).distance(DVec2::new(40.0, 24.0)) < 0.05,
        "{:?}",
        at(&h, 2)
    );
    assert_eq!(h.state().editor.selection.len(), 4, "still picked");
    save_png("drift_ui_transform_scaled.png", &mut h);
    h.state_mut().undo();
    assert_eq!(at(&h, 2), DVec2::new(20.0, 12.0), "one undo step");
}

#[test]
fn check_sketch_finds_and_fixes_gaps() {
    let Some(mut h) = harness() else { return };
    // A 40 × 30 outline whose last corner misses by 0.3, and a line stopping 0.2 short.
    let (corner, end) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [
            DVec2::new(-20.0, -15.0),
            DVec2::new(20.0, -15.0),
            DVec2::new(20.0, 15.0),
            DVec2::new(-20.0, 15.0),
            DVec2::new(-20.0, -14.7),
        ]
        .map(|at| ed.doc.add_point(at));
        for i in 0..4 {
            ed.doc.add_line(p[i], p[i + 1]).unwrap();
        }
        let (a, b) = (
            ed.doc.add_point(DVec2::new(40.0, 0.0)),
            ed.doc.add_point(DVec2::new(20.2, 0.0)),
        );
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Setup");
        (p[4], b)
    };
    h.run();
    assert!(h.state_mut().editor.regions().is_empty(), "not closed");

    // Edit → Check sketch…: two issues, marked on the canvas.
    h.get_by_label("Edit").click();
    h.run();
    h.get_by_label("Check sketch…").click();
    h.run();
    h.run();
    assert!(h.state().sketch_check.is_some());
    assert_eq!(h.state_mut().sketch_issues().len(), 2);
    assert!(h.query_by_label("Fix all (2)").is_some());
    save_png("drift_ui_check_sketch.png", &mut h);

    // Show picks the first and zooms to it.
    h.get_all_by_label("Show").next().unwrap().click();
    h.run();
    h.run();
    assert!(h.state().editor.selection.contains(&corner));

    // Fix all: one undo step; the outline closes and the line reaches it.
    h.get_by_label("Fix all (2)").click();
    h.run();
    h.run();
    assert!(!h.state().editor.doc.contains(corner), "joined");
    assert_eq!(h.state_mut().editor.regions().len(), 1, "closed");
    let x = h.state().editor.doc.point(end).unwrap().x;
    assert!((x - 20.0).abs() < 1e-6, "{x}");
    assert!(h.state_mut().sketch_issues().is_empty());
    h.state_mut().undo();
    assert!(h.state().editor.doc.contains(corner));
    h.run();
    assert_eq!(h.state_mut().sketch_issues().len(), 2);

    // The tolerance is kept with the drawing, not the preferences.
    h.state_mut()
        .editor
        .change_rules(|r| r.gap_tolerance = 0.25);
    assert_eq!(h.state().editor.doc.rules().gap_tolerance, 0.25);
    assert!(h.state().view_prefs().contains("gaptol=0.5"));
    assert_eq!(
        h.state_mut().sketch_issues().len(),
        1,
        "only the closer one"
    );
    // (Laid out again with one row fewer before the Close button is found.)
    h.run();
    h.get_by_label("Close").click();
    h.run();
    assert!(h.state().sketch_check.is_none());
}

#[test]
fn g_moves_a_picked_text() {
    let Some(mut h) = harness() else { return };
    // Type a text; the text tool is still in hand, the text picked.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::T);
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 10.0));
    h.run();
    let id = h.state().editor.editing_text.expect("typing in place");
    h.event(Event::Text("MOVE ME".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().editor.sole_text(), Some(id));
    let from = h.state().editor.doc.text(id).unwrap().at;
    // G: the select tool takes over and the text follows the pointer; a click places it.
    let start = screen(&h, DVec2::new(-30.0, 10.0));
    h.hover_at(start);
    h.run();
    h.key_press(Key::G);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert!(h.state().tool.is_modal(), "moving the text");
    let to = screen(&h, DVec2::new(10.0, -15.0));
    h.hover_at(to);
    h.run();
    let moving = h.state().editor.doc.text(id).unwrap().at;
    assert!(
        (moving - from - DVec2::new(40.0, -25.0)).length() < 1.0,
        "{moving:?}"
    );
    button(&h, to, true);
    h.run();
    button(&h, to, false);
    h.run();
    assert!(!h.state().tool.is_modal());
    let placed = h.state().editor.doc.text(id).unwrap().at;
    assert!((placed - from - DVec2::new(40.0, -25.0)).length() < 1.0);
    assert_eq!(h.state().editor.sole_text(), Some(id), "still picked");
    // One undo step; and Esc during a move puts it back.
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.text(id).unwrap().at, from);
    h.hover_at(start);
    h.run();
    h.key_press(Key::G);
    h.run();
    h.hover_at(to);
    h.run();
    assert_ne!(h.state().editor.doc.text(id).unwrap().at, from);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().editor.doc.text(id).unwrap().at, from);
    // Y locks the move to an axis, as for anything else.
    h.hover_at(start);
    h.run();
    h.key_press(Key::G);
    h.run();
    h.key_press(Key::Y);
    h.run();
    h.hover_at(to);
    h.run();
    let locked = h.state().editor.doc.text(id).unwrap().at;
    assert!((locked.x - from.x).abs() < 1e-9 && (locked.y - from.y + 25.0).abs() < 1.0);
    h.key_press(Key::Escape);
    h.run();
}

#[test]
fn r_turns_a_picked_text_about_its_anchor() {
    let Some(mut h) = harness() else { return };
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let id = ed.doc.add_text(drift_core::SketchText::new(
        "TURN",
        DVec2::new(10.0, 5.0),
        6.0,
    ));
    ed.commit("Text");
    ed.selected_texts = [id].into();
    h.run();
    // R with the select tool: the text turns about its anchor as the pointer goes round.
    h.hover_at(screen(&h, DVec2::new(40.0, 5.0)));
    h.run();
    h.key_press(Key::R);
    h.run();
    assert!(h.state().tool.is_modal(), "rotating the text");
    // Straight up from the anchor: a quarter turn.
    let up = screen(&h, DVec2::new(10.0, 45.0));
    h.hover_at(up);
    h.run();
    button(&h, up, true);
    h.run();
    button(&h, up, false);
    h.run();
    let text = h.state().editor.doc.text(id).cloned().unwrap();
    assert!((text.angle - 90.0).abs() < 1e-6, "{}", text.angle);
    assert!(
        text.at.distance(DVec2::new(10.0, 5.0)) < 1e-9,
        "about its own anchor"
    );
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.text(id).unwrap().angle, 0.0);
}

#[test]
fn dimensions_hide_leave_sheets_and_take_text() {
    use drift_core::{AnnotationRef, Constraint};
    let Some(mut h) = harness() else { return };
    // A 40 × 24 rectangle with its width and height dimensioned.
    let (width, height) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [
            DVec2::new(-20.0, -12.0),
            DVec2::new(20.0, -12.0),
            DVec2::new(20.0, 12.0),
            DVec2::new(-20.0, 12.0),
        ]
        .map(|at| ed.doc.add_point(at));
        for i in 0..4 {
            ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
        }
        ed.commit("Setup");
        let w = ed
            .add_dimension(
                Constraint::HorizontalDistance {
                    a: p[0],
                    b: p[1],
                    value: 40.0,
                },
                Some(DVec2::new(0.0, -10.0)),
            )
            .unwrap();
        let ht = ed
            .add_dimension(
                Constraint::VerticalDistance {
                    a: p[1],
                    b: p[2],
                    value: 24.0,
                },
                Some(DVec2::new(10.0, 0.0)),
            )
            .unwrap();
        (w, ht)
    };
    h.run();
    let painter = |h: &Harness<'_, DriftApp>| {
        egui::Painter::new(
            h.ctx.clone(),
            egui::LayerId::background(),
            h.state().canvas_rect,
        )
    };
    let laid_out = |h: &Harness<'_, DriftApp>| -> Vec<drift_core::ConstraintId> {
        h.state()
            .layout_dimensions(&painter(h), &screen_map(h))
            .iter()
            .filter_map(|d| d.id)
            .collect()
    };
    let dim_text = |h: &Harness<'_, DriftApp>, id| {
        h.state()
            .layout_dimensions(&painter(h), &screen_map(h))
            .into_iter()
            .find(|d| d.id == Some(id))
            .unwrap()
            .text_rect
            .center()
    };
    let on_sheets = |h: &mut Harness<'_, DriftApp>| -> Vec<String> {
        let p = painter(h);
        let app = h.state_mut();
        let d = crate::output::model_drawing(&mut app.editor, &app.hatch_patterns, &p);
        d.items
            .iter()
            .filter_map(|i| match &i.prim {
                drift_draft::page::Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(laid_out(&h).len(), 2);

    // Pick the width: the inspector has Driven, and Show.
    let at = dim_text(&h, width);
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert_eq!(h.state().selected_constraint, Some(width));
    h.get_by_label("Driven (reference)").click();
    h.run();
    assert!(h.state().editor.doc.is_driven(width));
    h.get_by_label("Driven (reference)").click();
    h.run();
    assert!(!h.state().editor.doc.is_driven(width), "driving again");

    // Off sheets: still on the canvas, gone from output.
    assert!(on_sheets(&mut h).iter().any(|t| t.contains("40")));
    h.get_by_label("On sheets, in print and export").click();
    h.run();
    assert!(
        !h.state()
            .editor
            .doc
            .shown(AnnotationRef::Dimension(width))
            .sheets
    );
    assert_eq!(laid_out(&h).len(), 2);
    let texts = on_sheets(&mut h);
    assert!(!texts.iter().any(|t| t.contains("40")) && texts.iter().any(|t| t.contains("24")));

    // Hidden on the canvas: not drawn or pickable; Alt+H shows it, faded, to pick again.
    h.get_by_label("On the canvas").click();
    h.run();
    assert_eq!(laid_out(&h), vec![height]);
    assert!(
        h.query_by_label("1 hidden annotation").is_some(),
        "the status bar counts it"
    );
    save_png("drift_ui_dimension_hidden.png", &mut h);
    h.key_press_modifiers(EguiModifiers::ALT, Key::H);
    h.run();
    assert!(h.state().show_hidden_annotations);
    assert_eq!(laid_out(&h).len(), 2);
    save_png("drift_ui_dimension_hidden_shown.png", &mut h);
    // It still holds: the width can't be dragged out of 40.
    let x = h.state().editor.doc.constraint(width).unwrap().value();
    assert_eq!(x, Some(40.0));
    // Undo brings it back.
    h.state_mut().undo();
    h.state_mut().undo();
    assert!(
        h.state()
            .editor
            .doc
            .shown(AnnotationRef::Dimension(width))
            .is_default()
    );

    // Shift+click picks both; hiding applies to both. (The message about hiding has faded.)
    h.state_mut().status = None;
    let (a, b) = (dim_text(&h, width), dim_text(&h, height));
    h.hover_at(a);
    h.run();
    button(&h, a, true);
    h.run();
    button(&h, a, false);
    h.run();
    h.hover_at(b);
    h.run();
    h.event_modifiers(
        Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    // (One frame: the harness's frames are a quarter of a second, and a press held much
    // longer is no click.)
    h.step();
    h.event_modifiers(
        Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    h.run();
    assert_eq!(h.state().picked_annotations().len(), 2);
    assert!(h.query_by_label_contains("For the 2 picked").is_some());
    h.get_by_label("On the canvas").click();
    h.run();
    let doc = &h.state().editor.doc;
    assert!(!doc.shown(AnnotationRef::Dimension(width)).canvas);
    assert!(!doc.shown(AnnotationRef::Dimension(height)).canvas);
    h.state_mut().undo();
    h.state_mut().show_hidden_annotations = false;
    h.state_mut().status = None;
    h.run();

    // Double-click the value: TYP and EQ are a click away; the line under is in the
    // inspector.
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    let at = dim_text(&h, width);
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        button(&h, at, true);
        h.step();
        button(&h, at, false);
        h.step();
    }
    h.run();
    assert!(h.state().dimension_edit.is_some());
    // (The inspector has the same chips: this one is by the dimension.)
    h.get_all_by_label("TYP")
        .min_by(|p, q| {
            let d = |n: &egui_kittest::Node<'_>| n.rect().center().distance(at);
            d(p).total_cmp(&d(q))
        })
        .unwrap()
        .click();
    h.run();
    assert_eq!(h.state().editor.doc.dim_note(width).unwrap().suffix, "TYP");
    assert!(h.state().dimension_edit.is_some(), "still open");
    save_png("drift_ui_dimension_chips.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(h.state().dimension_edit.is_none());
    let label = |h: &Harness<'_, DriftApp>| {
        h.state()
            .layout_dimensions(&painter(h), &screen_map(h))
            .into_iter()
            .find(|d| d.id == Some(width))
            .unwrap()
            .label
    };
    assert_eq!(label(&h).plain(), "40 TYP");
    let mut note = h.state().editor.doc.dim_note(width).cloned().unwrap();
    note.text = "EQ".into();
    note.below = "TO FACE OF STUD".into();
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    ed.doc.set_dim_note(width, Some(note));
    ed.commit("Text");
    h.run();
    let l = label(&h);
    assert_eq!(
        (l.plain().as_str(), l.below.as_str()),
        ("EQ TYP", "TO FACE OF STUD")
    );
    let texts = on_sheets(&mut h);
    assert!(texts.iter().any(|t| t == "TO FACE OF STUD"), "{texts:?}");
    save_png("drift_ui_dimension_text_below.png", &mut h);

    // Notes too: hidden on the canvas they aren't laid out; off sheets they aren't output.
    let note = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let id = ed
            .doc
            .add_annotation(drift_core::Annotation::Note {
                text: "SEE DETAIL".into(),
                at: DVec2::new(30.0, 20.0),
                leaders: Vec::new(),
                style: Default::default(),
            })
            .unwrap();
        ed.commit("Note");
        id
    };
    h.run();
    let notes =
        |h: &Harness<'_, DriftApp>| h.state().layout_notes(&painter(h), &screen_map(h)).len();
    assert_eq!(notes(&h), 1);
    assert!(on_sheets(&mut h).iter().any(|t| t == "SEE DETAIL"));
    let r = AnnotationRef::Annotation(note);
    h.state_mut()
        .set_annotations_shown(&[r], Some(false), Some(false), 0.0);
    h.state_mut().status = None;
    h.run();
    assert_eq!(notes(&h), 0);
    assert!(!on_sheets(&mut h).iter().any(|t| t == "SEE DETAIL"));
    // Saved and loaded as it is.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert!(!loaded.shown(r).canvas && !loaded.shown(r).sheets);
}

#[test]
fn top_bar_switches_for_dimensions_selection_and_sheet_scales() {
    let Some(mut h) = harness() else { return };
    // The dimension tool's row mode and overall.
    h.key_press(Key::D);
    h.run();
    h.get_by_label("Chain").click();
    h.run();
    assert_eq!(h.state().editor.dim_mode, crate::editor::DimMode::Chain);
    h.get_by_label("Overall").click();
    h.run();
    assert!(h.state().editor.dim_overall);
    save_png("drift_ui_dimension_chain_bar.png", &mut h);
    // The select tool's filter.
    h.key_press(Key::S);
    h.run();
    h.get_by_label("Edges").click();
    h.run();
    assert_eq!(
        h.state().editor.pick_filter,
        crate::editor::PickFilter::Edges
    );
    h.get_by_label("All").click();
    h.run();
    assert_eq!(h.state().editor.pick_filter, crate::editor::PickFilter::All);
    // Sheet scales in feet and inches: the architectural ones, written as they are.
    let mut units = h.state().editor.doc.units();
    units.length = drift_core::LengthFormat::Architectural;
    h.state_mut().editor.set_units(units);
    assert_eq!(drift_core::sheets::scale_text([48.0]), "1/4\" = 1'-0\"");
}

#[test]
fn tab_highlights_a_chain_and_a_click_picks_it() {
    let Some(mut h) = harness() else { return };
    // An L of two joined lines, and a line on its own.
    let (l1, l2) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [
            DVec2::new(-20.0, 0.0),
            DVec2::new(20.0, 0.0),
            DVec2::new(20.0, 20.0),
        ]
        .map(|at| ed.doc.add_point(at));
        let l1 = ed.doc.add_line(p[0], p[1]).unwrap();
        let l2 = ed.doc.add_line(p[1], p[2]).unwrap();
        let (a, b) = (
            ed.doc.add_point(DVec2::new(-20.0, -20.0)),
            ed.doc.add_point(DVec2::new(20.0, -20.0)),
        );
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Setup");
        (l1, l2)
    };
    h.run();
    let on = screen(&h, DVec2::new(-5.0, 0.0));
    h.hover_at(on);
    h.run();
    // Tab: highlighted, not picked.
    h.key_press(Key::Tab);
    h.run();
    assert!(h.state().editor.selection.is_empty());
    let preview = h.state().editor.chain_preview.clone().expect("highlighted");
    assert_eq!(preview.1, [l1, l2].into());
    assert!(
        h.ctx.memory(|m| m.focused()).is_none(),
        "Tab didn't move to a button"
    );
    save_png("drift_ui_tab_chain.png", &mut h);
    // Tab again: back to the one line.
    h.key_press(Key::Tab);
    h.run();
    assert!(h.state().editor.chain_preview.is_none());
    h.key_press(Key::Tab);
    h.run();
    // A click picks the chain.
    button(&h, on, true);
    h.run();
    button(&h, on, false);
    h.run();
    assert_eq!(h.state().editor.selection, [l1, l2].into());
    assert!(h.state().editor.chain_preview.is_none());
    // Moving off the chain clears a highlight.
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().editor.selection.clear();
    h.key_press(Key::Tab);
    h.run();
    assert!(h.state().editor.chain_preview.is_some());
    h.hover_at(screen(&h, DVec2::new(0.0, 40.0)));
    h.run();
    assert!(h.state().editor.chain_preview.is_none());
}

/// Export a drawing's sheets to PDF and SVG, for looking at by hand:
/// `DRIFT_FILE=path DRIFT_OUT=dir cargo test -p drift-app probe_export -- --ignored`.
#[test]
#[ignore]
fn probe_export() {
    let (Ok(path), Ok(out)) = (std::env::var("DRIFT_FILE"), std::env::var("DRIFT_OUT")) else {
        return;
    };
    let Some(mut h) = harness() else { return };
    let text = std::fs::read_to_string(&path).unwrap();
    let (doc, meta) = drift_io::load(&text).unwrap();
    h.state_mut()
        .replace_document(doc, "probe".into(), None, meta, true);
    h.run();
    h.run();
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let pages = h.state_mut().pages(&painter);
    let out = std::path::Path::new(&out);
    std::fs::write(out.join("probe.pdf"), drift_io::pdf::write(&pages, "probe")).unwrap();
    for (i, p) in pages.iter().enumerate() {
        std::fs::write(out.join(format!("probe-{i}.svg")), drift_io::svg::write(p)).unwrap();
    }
    eprintln!("wrote {} page(s)", pages.len());
}

#[test]
fn finding_a_font_keeps_the_menu_open_and_a_pick_closes_it() {
    use egui::accesskit::Role;
    let families = drift_platform::fonts::families();
    let Some(family) = ["Arial", "Verdana", "DejaVu Sans", "Liberation Sans"]
        .into_iter()
        .find(|f| families.iter().any(|x| x == f))
        .map(str::to_owned)
    else {
        eprintln!("no familiar system fonts here; skipping");
        return;
    };
    let Some(mut h) = harness() else { return };
    let id = {
        let ed = &mut h.state_mut().editor;
        ed.add_text(drift_core::SketchText::new("ROOM", DVec2::ZERO, 10.0))
    };
    h.run();
    h.run();
    // The inspector's Font menu: the combo box on the Font row.
    let row = h.get_by_label("Font").rect().center().y;
    h.get_all_by_role(Role::ComboBox)
        .find(|c| (c.rect().center().y - row).abs() < 12.0)
        .expect("the font menu")
        .click();
    h.run();
    assert!(h.query_by_label("On this computer").is_some(), "open");
    // Click into "Find a font…" and type: the menu stays open.
    h.get_all_by_role(Role::TextInput)
        .filter(|n| n.rect().center().y > row)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("the find field")
        .click();
    h.run();
    assert!(
        h.query_by_label("On this computer").is_some(),
        "still open after the click"
    );
    h.event(Event::Text(family[..4].to_owned()));
    h.run();
    assert!(
        h.query_by_label("On this computer").is_some(),
        "still open while typing"
    );
    // A pick closes it.
    h.get_all_by_label(&family)
        .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("the family in the list")
        .click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("On this computer").is_none(),
        "closed by the pick"
    );
    assert_eq!(
        h.state().editor.doc.text(id).unwrap().font,
        drift_core::TextFont::System(family)
    );
}

#[test]
fn sketch_text_goes_out_as_text() {
    use drift_draft::page::Prim;
    let Some(mut h) = harness() else { return };
    {
        let ed = &mut h.state_mut().editor;
        let mut t = drift_core::SketchText::new("ROOM 101\nSee detail", DVec2::new(5.0, 5.0), 4.0);
        t.align = drift_core::TextAlign::Center;
        ed.add_text(t);
    }
    h.run();
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let app = h.state_mut();
    let d = crate::output::model_drawing(&mut app.editor, &app.hatch_patterns, &painter);
    // Two lines of text in the text's font, and no curves.
    let texts: Vec<(String, f64, Option<drift_core::TextFont>)> = d
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Text {
                text, height, font, ..
            } => Some((text.clone(), *height, font.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            ("ROOM 101".into(), 4.0, Some(drift_core::TextFont::Sans)),
            ("See detail".into(), 4.0, Some(drift_core::TextFont::Sans)),
        ]
    );
    assert!(
        !d.items
            .iter()
            .any(|i| matches!(i.prim, Prim::Line(..) | Prim::Bezier { .. })),
        "no outline curves"
    );
    // DXF: TEXT entities. SVG: text in its font. PDF: filled glyphs, never stroked.
    let dxf = drift_io::dxf::write(&d, drift_io::dxf::DxfUnits::MM);
    assert!(dxf.contains("ROOM 101") && dxf.contains("See detail"));
    let svg = drift_io::svg::write(&d);
    assert!(
        svg.contains(">ROOM 101</text>") && svg.contains("'Ubuntu'"),
        "{svg}"
    );
    let pdf = String::from_utf8_lossy(&drift_io::pdf::write(std::slice::from_ref(&d), "Text"))
        .into_owned();
    assert!(!pdf.contains(" B\n") && !pdf.contains("2 Tr"));
}

#[test]
fn chain_dimensions_in_the_window() {
    let Some(mut h) = harness() else { return };
    // Four vertical walls along a floor, as in an elevation.
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        for x in [-30.0, -10.0, 15.0, 30.0] {
            let a = ed.doc.add_point(DVec2::new(x, 0.0));
            let b = ed.doc.add_point(DVec2::new(x, 20.0));
            ed.doc.add_line(a, b).unwrap();
        }
        ed.commit("Setup");
        // A filter left on in the select tool doesn't hold the dimension tool back.
        ed.pick_filter = crate::editor::PickFilter::Edges;
    }
    h.run();
    h.key_press(Key::D);
    h.run();
    h.get_by_label("Chain").click();
    h.run();
    let values = |h: &Harness<'_, DriftApp>| -> Vec<f64> {
        h.state()
            .editor
            .doc
            .constraints()
            .filter_map(|(_, c)| c.value())
            .collect()
    };
    // The first: two wall tops, placed above. Its value opens to type, alone: nothing
    // around it covers the next wall.
    click_world(&mut h, DVec2::new(-30.0, 20.0));
    click_world(&mut h, DVec2::new(-10.0, 20.0));
    click_world(&mut h, DVec2::new(-20.0, 26.0));
    h.run();
    assert!(h.state().dimension_edit.as_ref().is_some_and(|e| !e.extras));
    assert!(
        h.query_by_label("TYP").is_none(),
        "no chips under a value just placed"
    );
    // Then each next wall along the row: a top, then a wall clicked partway up.
    click_world(&mut h, DVec2::new(15.0, 20.0));
    h.run();
    click_world(&mut h, DVec2::new(30.0, 12.0));
    h.run();
    assert_eq!(values(&h), vec![20.0, 25.0, 15.0]);
    save_png("drift_ui_chain_window.png", &mut h);
}

#[test]
fn views_line_up_on_the_sheet() {
    use drift_core::ViewFrame;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    // Two framed elevations at the same scale, side by side and not level.
    let mut a = placed(&h, id)[0].clone();
    a.frame = ViewFrame {
        at: DVec2::new(130.0, 160.0),
        size: DVec2::new(160.0, 120.0),
    };
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        ed.doc.set_placed_view(id, 0, &a);
        let mut north = ed.doc.view(a.id).unwrap().clone();
        north.name = "North".into();
        north.center += DVec2::new(10.0, -7.0);
        north.size = DVec2::new(140.0, 100.0) * north.scale;
        let north = ed.doc.add_view(north);
        let mut s = ed.doc.sheet(id).unwrap().clone();
        s.viewports
            .push(drift_core::Viewport::new(north, DVec2::new(320.0, 150.0)));
        ed.doc.set_sheet(id, s);
        ed.commit("Views");
    }
    h.state_mut().sheet_view = 1;
    h.state_mut().sheet_preview = true;
    // The picked view's settings are in the Inspector.
    h.state_mut().sheet_pick = crate::app::sheet_editor::SheetPick::View;
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    let offset = |h: &Harness<'_, DriftApp>, i: usize| {
        let v = &placed(h, id)[i];
        v.frame.at - v.center / v.scale
    };
    assert!(
        (offset(&h, 0).y - offset(&h, 1).y).abs() > 0.1,
        "not level yet"
    );
    // "Drawings level": the second's drawing moves to the first's height.
    // (The Sheets tab is long: brought into sight first.)
    h.get_by_label("Drawings level").scroll_to_me();
    h.run();
    h.get_by_label("Drawings level").click();
    h.run();
    assert!((offset(&h, 0).y - offset(&h, 1).y).abs() < 1e-9);
    // Top: the frames' tops level; one undo step each.
    h.get_by_label("Top").click();
    h.run();
    let top = |h: &Harness<'_, DriftApp>, i: usize| placed(h, id)[i].frame.bounds().max.y;
    assert!((top(&h, 0) - top(&h, 1)).abs() < 1e-9);
    h.state_mut().undo();
    assert!(
        (offset(&h, 0).y - offset(&h, 1).y).abs() < 1e-9,
        "back to level drawings"
    );

    // Dragging the second view a little off level snaps it back, with a guide.
    let mid = {
        let f = placed(&h, id)[1].frame.bounds();
        let app = h.state();
        let r = app.canvas_rect;
        let d = (f.center() - app.preview.center) * f64::from(app.preview.zoom);
        r.center() + egui::vec2(d.x as f32, -d.y as f32)
    };
    h.hover_at(mid);
    h.run();
    button(&h, mid, true);
    h.run();
    h.hover_at(mid + egui::vec2(10.0, 0.0));
    h.run();
    h.hover_at(mid + egui::vec2(25.0, 3.0));
    h.run();
    assert!(!h.state().view_guides.is_empty(), "a guide while snapped");
    save_png("drift_ui_view_guides.png", &mut h);
    button(&h, mid + egui::vec2(25.0, 3.0), false);
    h.run();
    assert!(
        (offset(&h, 0).y - offset(&h, 1).y).abs() < 1e-9,
        "snapped level again"
    );
    assert!(h.state().view_guides.is_empty());
}

#[test]
fn chain_dimensions_from_wall_to_wall() {
    let Some(mut h) = harness() else { return };
    // Four walls standing on a floor, as in an elevation; picked by the walls themselves.
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        for x in [-30.0, -10.0, 15.0, 30.0] {
            let a = ed.doc.add_point(DVec2::new(x, 0.0));
            let b = ed.doc.add_point(DVec2::new(x, 20.0));
            ed.doc.add_line(a, b).unwrap();
        }
        ed.commit("Setup");
    }
    h.run();
    h.key_press(Key::D);
    h.run();
    h.get_by_label("Chain").click();
    h.run();
    h.get_by_label("Overall").click();
    h.run();
    let values = |h: &Harness<'_, DriftApp>| -> Vec<(f64, bool)> {
        let doc = &h.state().editor.doc;
        doc.constraints()
            .filter_map(|(id, c)| c.value().map(|v| (v, doc.is_driven(id))))
            .collect()
    };
    // Two walls, clicked partway up (not at their ends), placed above.
    click_world(&mut h, DVec2::new(-30.0, 7.0));
    click_world(&mut h, DVec2::new(-10.0, 9.0));
    click_world(&mut h, DVec2::new(-20.0, 26.0));
    h.run();
    // Then the next walls, each clicked anywhere along it.
    click_world(&mut h, DVec2::new(15.0, 4.0));
    h.run();
    click_world(&mut h, DVec2::new(30.0, 13.0));
    h.run();
    // An empty click ends it, with the overall as a reference.
    click_world(&mut h, DVec2::new(0.0, -30.0));
    h.run();
    assert_eq!(
        values(&h),
        vec![(20.0, false), (25.0, false), (15.0, false), (60.0, true)]
    );
    // The row's labels stand level with the first's.
    let doc = &h.state().editor.doc;
    let ys: Vec<f64> = doc
        .constraints()
        .filter_map(|(id, c)| {
            let anchor = crate::annotations::dimension_anchor(doc, c)?;
            Some((anchor + doc.placement(id)?).y)
        })
        .collect();
    assert!(ys[..3].iter().all(|y| (y - ys[0]).abs() < 1e-6), "{ys:?}");
    assert!(ys[3] > ys[0], "the overall outside the row: {ys:?}");
    save_png("drift_ui_chain_walls.png", &mut h);
}

#[test]
fn clear_relations_on_a_selection() {
    use drift_core::{Constraint, EntityKind};
    let Some(mut h) = harness() else { return };
    // A rectangle (level and plumb sides), its corner fixed and its height dimensioned.
    let (left, bottom, corner, top_left) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [
            DVec2::new(-20.0, -10.0),
            DVec2::new(20.0, -10.0),
            DVec2::new(20.0, 10.0),
            DVec2::new(-20.0, 10.0),
        ]
        .map(|at| ed.doc.add_point(at));
        let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
        for (i, line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                Constraint::Horizontal { line: *line }
            } else {
                Constraint::Vertical { line: *line }
            };
            ed.doc.add_constraint(c).unwrap();
        }
        ed.commit("Setup");
        let _ = EntityKind::Point(DVec2::ZERO);
        (l[3], l[0], p[0], p[3])
    };
    {
        let ed = &mut h.state_mut().editor;
        ed.add_constraints(
            "Fix",
            &[Constraint::Fix {
                point: corner,
                at: DVec2::new(-20.0, -10.0),
            }],
        )
        .unwrap();
        ed.add_constraints(
            "Height",
            &[Constraint::Distance {
                a: corner,
                b: top_left,
                value: 20.0,
            }],
        )
        .unwrap();
        ed.selection = [left, bottom].into();
    }
    h.run();
    let count = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        (
            doc.constraints().filter(|(_, c)| !c.is_dimension()).count(),
            doc.constraints().filter(|(_, c)| c.is_dimension()).count(),
        )
    };
    assert_eq!(
        count(&h),
        (5, 1),
        "four sides level or plumb, a fix; a height"
    );
    // The two picked sides touch: their own level and plumb, and the fix on their corner.
    // (At the foot of a long inspector: brought into view first.)
    h.get_by_label("Clear relations (3)").scroll_to_me();
    h.run();
    h.get_by_label("Clear relations (3)").click();
    h.run();
    assert_eq!(
        count(&h),
        (2, 1),
        "the other two sides keep theirs; the height stays"
    );
    // Corners stay joined: the two sides still share their corner point.
    let doc = &h.state().editor.doc;
    assert_eq!(doc.users_of(corner).count(), 2);
    // One undo step brings them all back.
    h.state_mut().undo();
    assert_eq!(count(&h), (5, 1));
    // Dimensions have their own button.
    h.state_mut().editor.selection = [left, bottom].into();
    h.run();
    // (At the foot of a long inspector: brought into view first.)
    h.get_by_label("Clear dimensions (1)").scroll_to_me();
    h.run();
    h.get_by_label("Clear dimensions (1)").click();
    h.run();
    assert_eq!(count(&h), (5, 0));
}

/// Sheets of every library block, a category at a time at 1/4" = 1'-0" on a 96 dpi screen,
/// to judge them by eye (`target/tmp/drift_library_*.png`).
#[test]
#[ignore]
fn library_contact_sheets() {
    use crate::library::{self, Category, INCH};
    let Some(mut h) = harness() else { return };
    // 1/4" on screen (24 px at 96 dpi) to a foot.
    let scale = 24.0 / (12.0 * INCH);
    for c in Category::ALL {
        let entries: Vec<_> = library::all().iter().filter(|e| e.category == c).collect();
        let mut page = 0;
        let mut i = 0;
        while i < entries.len() {
            {
                let ed = &mut h.state_mut().editor;
                ed.set_units(drift_core::Units::architectural());
                let ids: Vec<_> = ed.doc.blocks().map(|(id, _)| id).collect();
                ed.doc.begin();
                for id in ids {
                    ed.doc.remove_block(id);
                }
                ed.commit("Clear");
                ed.selected_instances.clear();
            }
            h.run();
            let canvas = h.state().canvas_rect;
            let width = canvas.width() as f64 / scale - 24.0 * INCH;
            let height = canvas.height() as f64 / scale - 24.0 * INCH;
            let (mut x, mut y, mut row) = (0.0, 0.0, 0.0f64);
            while i < entries.len() {
                let e = entries[i];
                let b = e.block.bounds();
                let size = b.max - b.min;
                if x > 0.0 && x + size.x > width {
                    x = 0.0;
                    y -= row + 18.0 * INCH;
                    row = 0.0;
                }
                if -y + size.y > height && y < 0.0 {
                    break;
                }
                let id = h.state_mut().editor.library_block(e);
                let at = DVec2::new(x, y - size.y) - b.min;
                h.state_mut().editor.place_instance(id, at, 0.0, false);
                x += size.x + 18.0 * INCH;
                row = row.max(size.y);
                i += 1;
            }
            let ed = &h.state().editor;
            let mut bounds = drift_core::Bounds::EMPTY;
            for (id, _) in ed.doc.instances() {
                let b = ed.instance_bounds(id);
                bounds.include(b.min);
                bounds.include(b.max);
            }
            h.state_mut().editor.selected_instances.clear();
            h.state_mut().fit_target = None;
            h.state_mut().camera = drift_render::Camera {
                center: bounds.center(),
                scale,
            };
            h.run();
            h.run();
            page += 1;
            save_png(
                &format!("drift_library_{:?}_{page}.png", c).to_lowercase(),
                &mut h,
            );
        }
    }
}

#[test]
fn svg_artwork_is_imported_as_curves_on_its_layers_at_a_scale() {
    let Some(mut h) = harness() else { return };
    // As Illustrator writes it: a stroked rectangle on "Walls", a filled ring with no
    // outline and a label on "Furniture"; 72 units to the inch.
    let svg = r##"<?xml version="1.0" encoding="utf-8"?>
<!-- Generator: Adobe Illustrator 28.0.0, SVG Export Plug-In -->
<svg version="1.1" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 720 360">
<style type="text/css">
	.st0{fill:none;stroke:#000000;stroke-width:1.5;}
	.st1{fill:#9AB3D5;}
</style>
<g id="Walls"><rect x="72" y="72" class="st0" width="288" height="144"/></g>
<g id="Furniture">
	<path class="st1" d="M432,72h144v144H432V72z M468,108v72h72v-72H468z"/>
	<text transform="matrix(1 0 0 1 80 300)" font-size="18">LIVING</text>
</g>
</svg>"##;
    h.state_mut()
        .begin_import("plan.svg", svg.as_bytes().to_vec(), None, 0.0);
    h.run_steps(3);
    assert!(h.state().pending_import.is_some(), "the dialog is up");
    // At 1:50, then calibrated.
    let pending = h.state_mut().pending_import.as_mut().unwrap();
    pending.scale = 50.0;
    pending.calibrate = true;
    h.run_steps(2);
    save_png("drift_ui_vector_import.png", &mut h);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert!(app.pending_import.is_none());
    let doc = &app.editor.doc;
    let layer = |name: &str| {
        doc.layers()
            .find(|(_, l)| l.name == name)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("no layer {name}"))
    };
    let (walls, furniture) = (layer("Walls"), layer("Furniture"));
    // The rectangle: 4 inches by 2 on paper, 1:50 in the sketch, outlined, on Walls.
    let on = |l| {
        doc.iter()
            .filter(|(_, e)| e.layer == l && !e.kind.is_point())
            .map(|(id, e)| (id, e.style))
            .collect::<Vec<_>>()
    };
    let wall_lines = on(walls);
    assert_eq!(wall_lines.len(), 4);
    assert!(
        wall_lines
            .iter()
            .all(|(_, s)| *s == drift_core::LineKind::Solid)
    );
    let bounds = wall_lines.iter().filter_map(|(id, _)| doc.shape(*id)).fold(
        drift_core::Bounds::EMPTY,
        |mut b, s| {
            let sb = s.bounds();
            b.include(sb.min);
            b.include(sb.max);
            b
        },
    );
    let size = bounds.max - bounds.min;
    assert!((size.x - 101.6 * 50.0).abs() < 1e-6, "{size}");
    assert!((size.y - 50.8 * 50.0).abs() < 1e-6, "{size}");
    // The ring: a live fill with its hole, its edges construction (they don't print).
    let edges = on(furniture);
    assert_eq!(edges.len(), 8);
    assert!(
        edges
            .iter()
            .all(|(_, s)| *s == drift_core::LineKind::Construction)
    );
    let fills: Vec<drift_core::FillId> = doc.fills().map(|(id, _)| id).collect();
    assert_eq!(fills.len(), 1);
    assert_eq!(
        doc.fills().next().unwrap().1.color,
        Some(drift_core::Rgb([0x9a, 0xb3, 0xd5]))
    );
    let shapes = app.editor.fill_shapes();
    let ring = &shapes[&fills[0]];
    assert!(!ring.lost);
    assert_eq!(ring.region.holes.len(), 1);
    // The label.
    let (_, text) = app.editor.doc.texts().next().expect("the label");
    let text = text.clone();
    assert_eq!(text.text, "LIVING");
    assert_eq!(text.layer, furniture);
    // Artwork comes in free: no relations or dimensions (adding them is up to the user).
    assert_eq!(app.editor.doc.constraint_count(), 0);

    // Calibrating: the import stays picked and the calibrate tool is on. The rectangle's
    // bottom edge, said to be 6 m, scales everything about its first corner.
    assert_eq!(app.tool.kind(), ToolKind::Calibrate);
    assert!(app.tool.hint().contains("import"), "{}", app.tool.hint());
    assert_eq!(app.editor.selection.len(), 12);
    let (a, b) = (bounds.min, DVec2::new(bounds.max.x, bounds.min.y));
    let k = app.editor.calibrate_import(a, b, 6000.0).unwrap();
    assert!((k - 6000.0 / (101.6 * 50.0)).abs() < 1e-9);
    let after = app
        .editor
        .doc
        .iter()
        .filter(|(_, e)| e.layer == walls && !e.kind.is_point())
        .filter_map(|(id, _)| app.editor.doc.shape(id))
        .fold(drift_core::Bounds::EMPTY, |mut bb, s| {
            let sb = s.bounds();
            bb.include(sb.min);
            bb.include(sb.max);
            bb
        });
    assert!(after.min.distance(a) < 1e-6);
    assert!((after.max.x - after.min.x - 6000.0).abs() < 1e-6);
    let (_, label) = app.editor.doc.texts().next().unwrap();
    assert!((label.height - text.height * k).abs() < 1e-9);
    assert_eq!(app.editor.doc.constraint_count(), 0);
    // The fill follows its edges.
    let shapes = app.editor.fill_shapes();
    assert!(!shapes[&fills[0]].lost);
    // One undo puts the scale back; another takes the import away.
    app.undo();
    app.undo();
    assert_eq!(app.editor.doc.fills().count(), 0);
}

#[test]
fn the_import_dialog_says_what_wont_come_across_before_importing() {
    let Some(mut h) = harness() else { return };
    // A gradient fill, a dimensioned line, and art off the artboard.
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <defs><linearGradient id="g"><stop offset="0" stop-color="#336699"/><stop offset="1" stop-color="#fff"/></linearGradient></defs>
  <rect x="10" y="10" width="30" height="20" fill="url(#g)"/>
  <line x1="90" y1="50" x2="150" y2="50" stroke="black"/>
</svg>"##;
    h.state_mut()
        .begin_import("poster.svg", svg.as_bytes().to_vec(), None, 0.0);
    h.run_steps(3);
    save_png("drift_ui_import_heads_up.png", &mut h);
    assert!(h.query_by_label("Before you import").is_some());
    assert!(
        h.query_by_label_contains("presentation artwork will look different")
            .is_some()
    );
    assert!(h.query_by_label_contains("Clipping is applied").is_some());
    h.state_mut().pending_import = None;
    h.run_steps(2);

    // Too big to edit: the import is refused (an underlay is offered instead).
    let mut d = String::from("M 0 0");
    for k in 1..=401_000 {
        d.push_str(if k % 2 == 1 { " L 1 1" } else { " L 0 0" });
    }
    let big = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="{d}" stroke="black" fill="none"/></svg>"#
    );
    h.state_mut()
        .begin_import("traced.svg", big.into_bytes(), None, 0.0);
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("more than a sketch can work with")
            .is_some()
    );
    h.get_by_label("Import").click();
    h.run_steps(2);
    assert!(h.state().pending_import.is_some(), "refused: still asking");
    assert_eq!(h.state().editor.doc.iter().count(), 0);
    // As an underlay it's fine.
    h.state_mut().pending_import.as_mut().unwrap().mode = crate::app::import_ui::ImportAs::Underlay;
    h.run_steps(2);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert!(h.state().pending_import.is_none());
    assert_eq!(h.state().editor.doc.underlays().count(), 1);
}

/// Press at one world point, move there in steps and let go, one frame per event.
fn drag_world(h: &mut Harness<'_, DriftApp>, from: DVec2, to: DVec2) {
    let (a, b) = (screen(h, from), screen(h, to));
    h.hover_at(a);
    h.run();
    button(h, a, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(a + (b - a) * (k as f32 / 5.0));
        h.run();
    }
    button(h, b, false);
    h.run();
}

#[test]
fn views_are_drawn_on_the_canvas_and_placed_on_sheets() {
    use crate::app::views_ui::CropGrip;
    use drift_core::sheets::scale_label;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let near = |a: DVec2, b: DVec2| a.distance(b) < 0.01;

    // The View tool: drag a crop round the plate's left half (Alt: free of snaps).
    h.state_mut().set_tool(ToolKind::View);
    h.run();
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    drag_world(&mut h, DVec2::new(-20.0, -15.0), DVec2::new(70.0, 95.0));
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    let views: Vec<(drift_core::ViewId, drift_core::View)> = h
        .state()
        .editor
        .doc
        .views()
        .map(|(id, v)| (id, v.clone()))
        .collect();
    assert_eq!(views.len(), 1);
    let (id, view) = views[0].clone();
    assert_eq!(view.name, "View 1");
    assert!(near(view.crop().min, DVec2::new(-20.0, -15.0)), "{view:?}");
    assert!(near(view.crop().max, DVec2::new(70.0, 95.0)), "{view:?}");
    assert_eq!(view.scale, h.state().editor.doc.draft_style().scale);
    // Drawn: picked, back with the select tool, its name and scale on the bar.
    assert_eq!(h.state().picked_view, Some(id));
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    h.run();
    assert!(h.query_by_label("Sheets…").is_some(), "the view's bar");
    save_png("drift_ui_view_crop.png", &mut h);
    // One undo step.
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.views().count(), 0);
    h.state_mut().redo();
    assert_eq!(h.state().editor.doc.view(id), Some(&view));
    h.state_mut().picked_view = Some(id);
    h.run();

    // Its grips reframe it: the right edge out by 30.
    let crop_grip = |h: &Harness<'_, DriftApp>, at: DVec2| {
        let app = h.state();
        let map = ScreenMap {
            rect: app.canvas_rect,
            camera: app.camera,
        };
        let painter = h.ctx.layer_painter(egui::LayerId::background());
        app.crop_grip_at(&painter, &map, map.to_screen(at))
    };
    assert_eq!(
        crop_grip(&h, DVec2::new(70.0, 40.0)),
        Some((id, CropGrip::Resize(1, 0)))
    );
    drag_world(&mut h, DVec2::new(70.0, 40.0), DVec2::new(100.0, 40.0));
    let crop = h.state().editor.doc.view(id).unwrap().crop();
    assert!((crop.max.x - 100.0).abs() < 0.01 && (crop.min.x + 20.0).abs() < 0.01);
    assert_eq!(h.state().editor.history.undo_label(), Some("Reframe view"));

    // A click elsewhere lets it go; a click on its edge picks it again.
    click_world(&mut h, DVec2::new(108.0, 90.0));
    assert_eq!(h.state().picked_view, None);
    assert_eq!(
        crop_grip(&h, DVec2::new(-20.0, 40.0)),
        Some((id, CropGrip::Move))
    );
    click_world(&mut h, DVec2::new(-20.0, 40.0));
    assert_eq!(h.state().picked_view, Some(id));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().picked_view, None);

    // Hidden, its crop is neither drawn nor picked: on its own (Crop off), or all of them
    // (View → View crops).
    let mut hidden = h.state().editor.doc.view(id).unwrap().clone();
    hidden.crop_visible = false;
    h.state_mut().update_view(id, hidden, false);
    assert_eq!(crop_grip(&h, DVec2::new(-20.0, 40.0)), None);
    h.state_mut().undo();
    assert!(crop_grip(&h, DVec2::new(-20.0, 40.0)).is_some());
    // (Shift+Alt+B, or View → View crops.)
    let shift_alt = EguiModifiers::SHIFT | EguiModifiers::ALT;
    h.key_press_modifiers(shift_alt, Key::B);
    h.run();
    assert!(!h.state().show_view_crops);
    assert_eq!(crop_grip(&h, DVec2::new(-20.0, 40.0)), None);
    assert!(h.state().view_prefs().contains("crops=0"));
    save_png("drift_ui_view_crops_hidden.png", &mut h);
    h.key_press_modifiers(shift_alt, Key::B);
    h.run();
    assert!(h.state().show_view_crops);
    assert!(crop_grip(&h, DVec2::new(-20.0, 40.0)).is_some());

    // On a sheet: a new sheet brings a view of its own; Place on sheet adds this one, at its
    // scale, so its frame is its crop on the paper.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().expect("a sheet");
    assert_eq!(placed(&h, sheet).len(), 1);
    h.state_mut().picked_view = Some(id);
    // (New sheet shows the sheet in the Inspector; the Views list is in the Sheets panel.)
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("Place on sheet").scroll_to_me();
    h.run();
    h.get_by_label("Place on sheet").click();
    h.run();
    let on = placed(&h, sheet);
    assert_eq!(on.len(), 2);
    assert_eq!(on[1].id, id);
    assert!(on[1].title);
    let crop = h.state().editor.doc.view(id).unwrap().crop();
    assert!(near(on[1].frame.size, crop.size() / on[1].scale));
    // And on a second sheet: one view on two sheets.
    h.state_mut().add_sheet();
    let other = h.state().current_sheet().unwrap();
    assert_ne!(other, sheet);
    h.state_mut().place_view(other, id, None);
    assert_eq!(h.state().editor.doc.sheets_showing(id), [sheet, other]);
    h.run();
    // (Sheets by number and name.)
    let row = format!(
        "View 1\n{} · on A-101  Sheet 1, A-102  Sheet 2",
        scale_label(view.scale)
    );
    assert!(
        h.query_by_label(&row).is_some(),
        "the Views list says where"
    );

    // Reframed on the canvas, it changes on both sheets.
    h.state_mut().sheet_preview = false;
    h.state_mut().picked_view = Some(id);
    h.run();
    let top = DVec2::new(40.0, 95.0);
    assert_eq!(crop_grip(&h, top), Some((id, CropGrip::Resize(0, 1))));
    drag_world(&mut h, top, top + DVec2::new(0.0, 25.0));
    for s in [sheet, other] {
        let v = placed(&h, s).into_iter().find(|v| v.id == id).unwrap();
        assert!(
            (v.frame.size.y - 135.0 / v.scale).abs() < 0.01,
            "taller on both sheets: {v:?}"
        );
    }

    // Dragged from the Views list onto the sheet, a view goes where it is let go.
    h.state_mut().active_sheet = Some(other);
    h.state_mut().sheet_preview = true;
    h.state_mut().preview.zoom = 0.0;
    h.run();
    h.run();
    let two = h
        .state()
        .editor
        .doc
        .views()
        .find(|(_, v)| v.name == "View 2")
        .map(|(id, _)| id)
        .unwrap();
    assert!(!h.state().editor.doc.sheets_showing(two).contains(&other));
    let rows = h.get_by_label_contains("View 2\n");
    rows.scroll_to_me();
    h.run();
    let from = h.get_by_label_contains("View 2\n").rect().center();
    let rect = h.state().canvas_rect;
    let to = rect.center() + egui::vec2(-120.0, 60.0);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=8 {
        h.hover_at(from + (to - from) * (k as f32 / 8.0));
        h.run();
    }
    save_png("drift_ui_view_drop.png", &mut h);
    button(&h, to, false);
    h.run();
    h.run();
    let on = placed(&h, other);
    let dropped = on
        .iter()
        .find(|v| v.id == two)
        .expect("dropped on the sheet");
    let app = h.state();
    let at = app.preview.center
        + DVec2::new(
            f64::from(to.x - rect.center().x),
            -f64::from(to.y - rect.center().y),
        ) / f64::from(app.preview.zoom);
    assert!(dropped.frame.at.distance(at) < 0.5, "{dropped:?} at {at:?}");
    save_png("drift_ui_view_dropped.png", &mut h);

    // Deleting a view on sheets asks first, then takes it off them all; undo brings it back.
    h.state_mut().picked_view = Some(id);
    h.run();
    h.get_by_label("Delete view").scroll_to_me();
    h.run();
    h.get_by_label("Delete view").click();
    h.run();
    assert!(h.state().editor.doc.view(id).is_some(), "not yet");
    assert_eq!(h.state().confirm_delete_view, Some(id));
    h.get_by_label("Delete it").click();
    h.run();
    assert!(h.state().editor.doc.view(id).is_none());
    assert!(h.state().editor.doc.sheets_showing(id).is_empty());
    assert_eq!(placed(&h, sheet).len(), 1);
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.sheets_showing(id), [sheet, other]);
}

#[test]
fn a_picked_dimension_sets_its_own_style_and_resets_it() {
    use drift_core::{AnnotationRef, ArrowStyle, Constraint as C};
    let Some(mut h) = harness() else { return };
    let dim = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(-20.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(20.0, 0.0));
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        ed.add_dimension(
            C::HorizontalDistance { a, b, value: 40.0 },
            Some(DVec2::new(0.0, -10.0)),
        )
        .unwrap()
    };
    h.state_mut().selected_constraint = Some(dim);
    h.run();
    h.run();
    let r = AnnotationRef::Dimension(dim);
    // Arrows follow the drawing until a different one is picked here.
    assert!(h.state().editor.doc.style_override(r).is_empty());
    let arrows = h
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|c| c.value().as_deref() == Some("Closed arrow"))
        .expect("the Arrows menu");
    arrows.click();
    h.run();
    h.get_by_label("Tick").click();
    h.run();
    h.run();
    assert_eq!(
        h.state().editor.doc.style_override(r).arrow,
        Some(ArrowStyle::Tick)
    );
    assert_eq!(
        h.state().editor.doc.draft_style_for(r).arrow,
        ArrowStyle::Tick
    );
    save_png("drift_ui_dimension_own_style.png", &mut h);
    // The reset arrow beside it puts the drawing's back.
    h.get_by_label("Reset").click();
    h.run();
    assert!(h.state().editor.doc.style_override(r).is_empty());
    // And the drawing's style is one click away (below the fold: scrolled to first).
    h.get_by_label("Drawing style…").scroll_to_me();
    h.run();
    h.run();
    h.get_by_label("Drawing style…").click();
    h.run();
    h.run();
    assert!(h.state().show_drawing_settings);
    save_png("drift_ui_drawing_settings.png", &mut h);
}

#[test]
fn sheets_are_tabs_with_their_own_tools_and_inspector() {
    use crate::app::sheet_editor::SheetPick;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // The tabs over the canvas: the model, and + for a sheet.
    assert!(h.query_by_label("Model").is_some());
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let first = h.state().current_sheet().expect("a sheet");
    let doc_sheet =
        |h: &Harness<'_, DriftApp>, id| h.state().editor.doc.sheet(id).cloned().unwrap();
    assert_eq!(doc_sheet(&h, first).fields.number, "A-101");
    assert!(h.state().sheet_preview);
    assert_eq!(
        h.state().sheet_pick,
        SheetPick::Paper,
        "the sheet itself, in the Inspector"
    );
    assert!(h.query_by_label("Number").is_some());
    // The next one counts on.
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let second = h.state().current_sheet().unwrap();
    assert_ne!(second, first);
    assert_eq!(doc_sheet(&h, second).fields.number, "A-102");
    // Its tab, and the first's.
    h.get_by_label("A-101  Sheet 1").click();
    h.run();
    assert_eq!(h.state().current_sheet(), Some(first));

    // On a sheet the rail holds sheet tools; a drawing tool (from a menu or the palette)
    // takes you back to the model with it.
    assert!(
        h.query_by_label("Line").is_none(),
        "no drawing tools on a sheet"
    );
    assert!(h.query_by_label("Add view").is_some());
    h.state_mut().set_tool(ToolKind::Line);
    h.run();
    assert!(!h.state().sheet_preview);
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("A-101  Sheet 1").click();
    h.run();
    h.run();
    assert!(h.state().sheet_preview);

    // A plain drag on the view that fills the sheet leaves it be.
    let before = placed(&h, first)[0].center;
    let mid = h.state().canvas_rect.center();
    h.hover_at(mid);
    h.run();
    button(&h, mid, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(mid + egui::vec2(15.0 * k as f32, 0.0));
        h.run();
    }
    button(&h, mid + egui::vec2(60.0, 0.0), false);
    h.run();
    assert_eq!(placed(&h, first)[0].center, before);

    // A view drawn for this sheet: it lands on free paper, picked, and stays inside the
    // border when dragged far out.
    add_view_by_region(&mut h, DVec2::new(55.0, 25.0), DVec2::new(100.0, 58.0));
    assert_eq!(h.state().sheet_pick, SheetPick::View);
    let s = doc_sheet(&h, first);
    let area = drift_draft::sheet::drawing_area(&s);
    let frame = placed(&h, first)[1].frame;
    let to_screen = |h: &Harness<'_, DriftApp>, p: DVec2| {
        let app = h.state();
        let d = (p - app.preview.center) * f64::from(app.preview.zoom);
        app.canvas_rect.center() + egui::vec2(d.x as f32, -d.y as f32)
    };
    let from = to_screen(&h, frame.at);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(from + egui::vec2(-150.0 * k as f32, 0.0));
        h.run();
    }
    button(&h, from + egui::vec2(-750.0, 0.0), false);
    h.run();
    let moved = placed(&h, first)[1].frame.bounds();
    assert!(
        moved.min.x >= area.min.x - 1e-9,
        "kept inside the border: {moved:?}"
    );
    // A click on bare paper picks the sheet.
    let paper = to_screen(&h, area.max - DVec2::splat(5.0));
    h.hover_at(paper);
    h.run();
    button(&h, paper, true);
    h.run();
    button(&h, paper, false);
    h.run();
    assert_eq!(h.state().sheet_pick, SheetPick::Paper);
    // The Model tab goes back.
    h.get_by_label("Model").click();
    h.run();
    assert!(!h.state().sheet_preview);
    save_png("drift_ui_sheet_tabs.png", &mut h);
}

#[test]
fn place_on_sheet_finds_free_paper() {
    use crate::app::sheet_editor::drawing_extent;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().expect("a sheet");
    // A view drawn for the sheet, then placed again from the Views list: the second copy
    // goes on free paper at the view's scale, clear of what is drawn there already.
    add_view_by_region(&mut h, DVec2::new(55.0, 25.0), DVec2::new(100.0, 58.0));
    let id = placed(&h, sheet)[1].id;
    let scale = h.state().editor.doc.view(id).unwrap().scale;
    h.state_mut().picked_view = Some(id);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("Place again").scroll_to_me();
    h.run();
    h.get_by_label("Place again").click();
    h.run();
    let on = placed(&h, sheet);
    assert_eq!(on.len(), 3);
    let new = &on[2];
    assert_eq!(new.id, id);
    assert_eq!(new.scale, scale, "at the scale the view has");
    let model = h.state().fit_bounds();
    let frame = new.frame.bounds();
    for v in &on[..2] {
        let drawn = drawing_extent(v, model);
        assert!(
            !frame.intersects(&drawn),
            "{frame:?} is clear of the drawing at {drawn:?}"
        );
    }
    assert!(!frame.intersects(&on[1].frame.bounds()));
}

#[test]
fn a_view_leaves_out_the_layers_it_hides() {
    use crate::app::sheet_editor::SheetPick;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A dimensioned line on a layer of its own.
    let layer = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let layer = ed.doc.add_layer(drift_core::Layer::new("Setting out"));
        let a = ed.doc.add_point(DVec2::new(20.0, 10.0));
        let b = ed.doc.add_point(DVec2::new(57.0, 10.0));
        let l = ed.doc.add_line(a, b).unwrap();
        for e in [a, b, l] {
            ed.doc.set_entity_layer(e, layer).unwrap();
        }
        ed.commit("Setup");
        ed.add_dimension(
            drift_core::Constraint::HorizontalDistance { a, b, value: 37.0 },
            Some(DVec2::new(38.0, 4.0)),
        )
        .unwrap();
        layer
    };
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().expect("a sheet");
    let view = placed(&h, sheet)[0].id;
    h.state_mut().picked_view = Some(view);
    h.state_mut().sheet_pick = SheetPick::View;
    h.run();
    let preview = |h: &Harness<'_, DriftApp>| h.state().preview_cache.as_ref().unwrap().2.clone();
    let on_layer = |d: &drift_draft::page::Drawing| {
        d.items.iter().filter(|i| i.layer == "Setting out").count()
    };
    let dimension = |d: &drift_draft::page::Drawing| {
        d.items.iter().any(|i| {
            matches!(&i.prim, drift_draft::page::Prim::Text { text, .. } if text.contains("37"))
        })
    };
    let before = preview(&h);
    assert!(on_layer(&before) > 0 && dimension(&before));
    save_png("drift_ui_view_layers_on.png", &mut h);

    // Off in the view's Inspector: gone from the sheet, dimension and all, as one step.
    h.get_by_label("Setting out").scroll_to_me();
    h.get_by_label("Setting out").click();
    h.run();
    h.run();
    assert!(placed(&h, sheet)[0].hidden_layers.contains(&layer));
    assert_eq!(h.state().editor.history.undo_label(), Some("Edit sheet"));
    let after = preview(&h);
    assert_eq!(on_layer(&after), 0);
    assert!(!dimension(&after), "its dimension goes with it");
    assert!(
        after.items.len() > before.items.len() / 2,
        "the rest still draws"
    );
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    assert_eq!(h.state_mut().pages(&painter)[0], after, "and prints so");
    save_png("drift_ui_view_layers.png", &mut h);
    // Saved with the view, and read back.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(
        saved.contains(&format!(r#""hidden_layers":["{layer}"]"#)),
        "{saved}"
    );
    let (read, _) = drift_io::load(&saved).unwrap();
    assert!(read.view(view).unwrap().hidden_layers.contains(&layer));
    // Deleting the layer forgets it.
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        ed.doc.remove_layer(layer).unwrap();
        ed.commit("Delete layer");
    }
    assert!(placed(&h, sheet)[0].hidden_layers.is_empty());
    h.state_mut().undo();
    assert!(placed(&h, sheet)[0].hidden_layers.contains(&layer));
    // Undo brings it back on the sheet.
    h.state_mut().undo();
    h.run();
    h.run();
    assert!(placed(&h, sheet)[0].hidden_layers.is_empty());
    assert!(on_layer(&preview(&h)) > 0);
}

#[test]
fn a_view_can_show_construction_geometry() {
    use crate::app::sheet_editor::SheetPick;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().expect("a sheet");
    let view = placed(&h, sheet)[0].id;
    h.state_mut().picked_view = Some(view);
    h.state_mut().sheet_pick = SheetPick::View;
    h.run();
    let preview = |h: &Harness<'_, DriftApp>| h.state().preview_cache.as_ref().unwrap().2.clone();
    // The example's construction curves print as hairline dashes.
    let guides = |d: &drift_draft::page::Drawing| {
        d.items
            .iter()
            .filter(|i| i.pen.line_type == drift_core::LineType::Dashed && i.pen.width == 0.13)
            .count()
    };
    let before = preview(&h);
    assert_eq!(guides(&before), 0, "off by default");
    assert!(!placed(&h, sheet)[0].construction);

    // On in the view's Inspector: on the sheet, as one step.
    h.get_by_label("Construction").scroll_to_me();
    h.get_by_label("Construction").click();
    h.run();
    h.run();
    assert!(placed(&h, sheet)[0].construction);
    assert_eq!(h.state().editor.history.undo_label(), Some("Edit sheet"));
    let after = preview(&h);
    assert!(guides(&after) > 0);
    assert_eq!(
        after.items.len(),
        before.items.len() + guides(&after),
        "nothing else changes"
    );
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    assert_eq!(h.state_mut().pages(&painter)[0], after, "and prints so");
    save_png("drift_ui_view_construction.png", &mut h);
    // Saved with the view, and read back.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(r#""construction":true"#), "{saved}");
    let (read, _) = drift_io::load(&saved).unwrap();
    assert!(read.view(view).unwrap().construction);
    // Undo takes it off the sheet again.
    h.state_mut().undo();
    h.run();
    h.run();
    assert!(!placed(&h, sheet)[0].construction);
    assert_eq!(guides(&preview(&h)), 0);
    // The switch is the view's: in the Views list's Inspector too.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("Construction").scroll_to_me();
    h.get_by_label("Construction").click();
    h.run();
    h.run();
    assert!(h.state().editor.doc.view(view).unwrap().construction);
    assert!(guides(&preview(&h)) > 0);
    save_png("drift_ui_model_view_construction.png", &mut h);
}

#[test]
fn sheets_reorder_duplicate_and_choose_for_pdf() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    for _ in 0..2 {
        h.get_by_label("Add a sheet").click();
        h.run();
        h.run();
    }
    let order = |h: &Harness<'_, DriftApp>| -> Vec<String> {
        h.state()
            .editor
            .doc
            .sheets()
            .map(|(_, s)| crate::app::sheet_editor::sheet_label(s))
            .collect()
    };
    assert_eq!(order(&h), ["A-101  Sheet 1", "A-102  Sheet 2"]);

    // Duplicating A-101 puts the copy right after it, numbered on past the numbers taken,
    // with copies of its views and none of its revisions.
    let first = h.state().editor.doc.sheets().next().unwrap().0;
    {
        let doc = &mut h.state_mut().editor.doc;
        let mut s = doc.sheet(first).unwrap().clone();
        s.revisions.push(drift_core::Revision {
            rev: "A".into(),
            date: String::new(),
            description: "Issued".into(),
            by: String::new(),
        });
        doc.begin();
        doc.set_sheet(first, s);
        doc.commit("Revise");
    }
    h.state_mut().duplicate_sheet(first, true);
    h.run();
    assert_eq!(
        order(&h),
        ["A-101  Sheet 1", "A-103  Sheet 1 copy", "A-102  Sheet 2"]
    );
    assert_eq!(
        h.state().editor.history.undo_label(),
        Some("Duplicate sheet")
    );
    let copy = h.state().current_sheet().unwrap();
    let doc = &h.state().editor.doc;
    let (a, b) = (doc.sheet(first).unwrap(), doc.sheet(copy).unwrap());
    assert!(b.revisions.is_empty());
    assert_ne!(
        a.viewports[0].view, b.viewports[0].view,
        "a view of its own"
    );
    assert_eq!(doc.view(b.viewports[0].view).unwrap().name, "View 1 copy");
    // Empty: the paper and title block alone.
    h.state_mut().duplicate_sheet(first, false);
    h.run();
    let empty = h.state().current_sheet().unwrap();
    assert!(
        h.state()
            .editor
            .doc
            .sheet(empty)
            .unwrap()
            .viewports
            .is_empty()
    );
    assert_eq!(order(&h)[1], "A-104  Sheet 1 copy");
    h.state_mut().undo();
    h.run();
    assert_eq!(order(&h).len(), 3);

    // Drag A-102's tab in front of A-101: the sheets reorder, and the one shown stays shown.
    let canvas = h.state().canvas_rect;
    let tab = |h: &Harness<'_, DriftApp>, label: &str| {
        h.get_all_by_label(label)
            .map(|n| n.rect())
            .find(|r| r.bottom() <= canvas.top() + 1.0 && r.left() < canvas.right())
            .expect("a canvas tab")
    };
    let at = tab(&h, "A-102  Sheet 2").center();
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    h.run();
    let from = tab(&h, "A-102  Sheet 2").center();
    let target = tab(&h, "A-101  Sheet 1");
    let to = egui::pos2(target.left() + 10.0, from.y);
    drag_steps(&mut h, from, to, 8, Some("drift_ui_sheet_tab_drag.png"));
    assert_eq!(
        order(&h),
        ["A-102  Sheet 2", "A-101  Sheet 1", "A-103  Sheet 1 copy"]
    );
    assert_eq!(h.state().editor.history.undo_label(), Some("Move sheet"));
    let shown = h.state().current_sheet().unwrap();
    assert_eq!(
        crate::app::sheet_editor::sheet_label(h.state().editor.doc.sheet(shown).unwrap()),
        "A-102  Sheet 2"
    );
    h.state_mut().undo();
    h.run();
    assert_eq!(order(&h)[0], "A-101  Sheet 1");

    // In the Sheets list, rows drag too: A-101 to the end.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    let row = |h: &Harness<'_, DriftApp>, label: &str| {
        h.get_all_by_label(label)
            .map(|n| n.rect())
            .find(|r| r.left() > canvas.right())
            .expect("a row in the Sheets list")
    };
    let from = row(&h, "A-101  Sheet 1").center();
    let to = egui::pos2(from.x, row(&h, "A-102  Sheet 2").bottom() - 2.0);
    drag_steps(&mut h, from, to, 8, Some("drift_ui_sheet_row_drag.png"));
    assert_eq!(
        order(&h),
        ["A-103  Sheet 1 copy", "A-102  Sheet 2", "A-101  Sheet 1"]
    );

    // PDF… with several sheets asks which: all, or just the one shown.
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pages = |h: &mut Harness<'_, DriftApp>| {
        let pdf = h.state_mut().pdf_bytes(&painter);
        String::from_utf8_lossy(&pdf)
            .matches("/Type /Page ")
            .count()
    };
    assert_eq!(pages(&mut h), 3);
    h.get_by_label("PDF…").click();
    h.run();
    h.run();
    assert_eq!(
        h.state().output_dialog,
        Some(crate::app::sheets_ui::Output::Pdf)
    );
    h.get_by_label("This sheet").click();
    h.run();
    h.run();
    assert_eq!(h.state().pdf_left_out.len(), 2);
    save_png("drift_ui_pdf_sheets.png", &mut h);
    assert_eq!(pages(&mut h), 1);
    h.get_by_label("All").click();
    h.run();
    assert_eq!(pages(&mut h), 3);
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().output_dialog, None);
}

#[test]
fn a_section_mark_makes_its_view_and_reads_its_number() {
    use egui::accesskit::Role;
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    // A section across the plate.
    h.get_by_label("Section mark").click();
    h.run();
    click_world(&mut h, DVec2::new(-10.0, 40.0));
    click_world(&mut h, DVec2::new(130.0, 40.0));
    let mark = h.state().editor.selected_annotation.expect("the new mark");
    h.get_by_label("Select").click();
    h.run();
    let reads = |h: &Harness<'_, DriftApp>| {
        let doc = &h.state().editor.doc;
        crate::notes::mark_reference(doc, doc.annotation(mark).unwrap()).unwrap()
    };
    let link = |h: &Harness<'_, DriftApp>| h.state().editor.doc.annotation(mark).unwrap().view();
    assert_eq!(reads(&h), ("A".to_owned(), String::new()), "its own label");
    h.state_mut().fit_target = Some(h.state().fit_bounds());
    h.run();
    h.run();
    save_png("drift_ui_mark_view_before.png", &mut h);

    // Right-click it: Make view. The view is named after it, crops what it looks at, and is
    // picked in the Views list, on no sheet yet: the flag reads – / –.
    let on = screen(&h, DVec2::new(-5.0, 40.0));
    h.hover_at(on);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: on,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
    h.get_by_label("Make view").click();
    h.run();
    h.run();
    let view = link(&h).expect("linked to its new view");
    {
        let doc = &h.state().editor.doc;
        let v = doc.view(view).unwrap();
        assert_eq!(v.name, "Section A");
        let crop = v.crop();
        assert!(
            (crop.min.x + 10.0).abs() < 1.0 && (crop.max.x - 130.0).abs() < 1.0,
            "along the cut: {crop:?}"
        );
        assert!(
            (crop.min.y - 40.0).abs() < 1.0 && crop.max.y > 60.0,
            "{crop:?}"
        );
        assert!(drift_core::sheets::standard_scales(false).contains(&v.scale));
        assert!(doc.sheets_showing(view).is_empty());
    }
    assert_eq!(h.state().picked_view, Some(view));
    assert!(h.query_by_label("Zoom to view").is_some());
    assert_eq!(reads(&h), ("–".to_owned(), "–".to_owned()));
    // One undo step.
    h.state_mut().undo();
    assert!(h.state().editor.doc.view(view).is_none());
    assert_eq!(link(&h), None);
    h.state_mut().redo();
    assert_eq!(link(&h), Some(view));

    // Placed on a sheet (after the sheet's own view), the flag reads its number there.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().expect("a sheet");
    h.state_mut().picked_view = Some(view);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("Place on sheet").scroll_to_me();
    h.run();
    h.get_by_label("Place on sheet").click();
    h.run();
    assert_eq!(reads(&h), ("2".to_owned(), "A-101".to_owned()));
    // The view's Inspector on the sheet lists the mark.
    h.state_mut().sheet_pick = crate::app::sheet_editor::SheetPick::View;
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    assert!(h.query_by_label("Section A mark").is_some());

    // Renumbering the sheet renumbers the flag.
    let mut renumbered = h.state().editor.doc.sheet(sheet).unwrap().clone();
    renumbered.fields.number = "A-501".into();
    h.state_mut().editor.doc.begin();
    h.state_mut().editor.doc.set_sheet(sheet, renumbered);
    h.state_mut().editor.commit("Renumber");
    assert_eq!(reads(&h), ("2".to_owned(), "A-501".to_owned()));

    // Referenced by: clicking the mark picks it in the model; its Inspector says what the
    // flag reads.
    h.get_by_label("Section A mark").click();
    h.run();
    h.run();
    assert!(!h.state().sheet_preview);
    assert_eq!(h.state().editor.selected_annotation, Some(mark));
    assert!(h.query_by_label("2 / A-501").is_some());
    h.state_mut().fit_target = Some(h.state().fit_bounds());
    h.run();
    h.run();
    save_png("drift_ui_mark_view_after.png", &mut h);

    // Unlinked in its Inspector, it reads its own label again; linked back the same way.
    h.get_all_by_role(Role::ComboBox)
        .find(|c| c.value().as_deref() == Some("Section A"))
        .expect("the View menu")
        .click();
    h.run();
    h.get_all_by_label("None").last().expect("None").click();
    h.run();
    h.run();
    assert_eq!(link(&h), None);
    assert_eq!(reads(&h), ("A".to_owned(), String::new()));
    h.get_all_by_role(Role::ComboBox)
        .find(|c| c.value().as_deref() == Some("None"))
        .expect("the View menu")
        .click();
    h.run();
    h.get_all_by_label("Section A")
        .last()
        .expect("the view")
        .click();
    h.run();
    h.run();
    assert_eq!(link(&h), Some(view));

    // Saved and loaded, it still points at its view.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(saved.contains(&format!(r#""view":"{view}""#)), "{saved}");
    let (loaded, _) = drift_io::load(&saved).unwrap();
    assert_eq!(
        loaded.annotation(mark),
        h.state().editor.doc.annotation(mark)
    );
    assert_eq!(loaded.view(view), h.state().editor.doc.view(view));

    // Deleting the view unlinks the mark in the same step; undo links it again.
    let before = h.state().editor.history.undo_label().map(str::to_owned);
    h.state_mut().delete_view(view);
    assert!(h.state().editor.doc.view(view).is_none());
    assert_eq!(link(&h), None);
    assert_eq!(reads(&h), ("A".to_owned(), String::new()));
    h.state_mut().undo();
    assert_eq!(link(&h), Some(view));
    assert_eq!(reads(&h), ("2".to_owned(), "A-501".to_owned()));
    assert_eq!(
        h.state().editor.history.undo_label().map(str::to_owned),
        before
    );
}

#[test]
fn every_sheet_shares_the_project() {
    use drift_core::{FieldSet, TitleBlock};
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    for _ in 0..2 {
        h.get_by_label("Add a sheet").click();
        h.run();
        h.run();
    }
    // The first sheet named the project after the file; the sheets leave their titles to it.
    let project = |h: &Harness<'_, DriftApp>| h.state().editor.doc.project().clone();
    let stem = h.state().file_stem();
    assert_eq!(project(&h).name, stem);
    let ids: Vec<_> = h.state().editor.doc.sheets().map(|(id, _)| id).collect();
    for id in &ids {
        assert!(
            h.state()
                .editor
                .doc
                .sheet(*id)
                .unwrap()
                .fields
                .title
                .is_empty()
        );
    }
    let texts = |h: &Harness<'_, DriftApp>| -> Vec<String> {
        h.state()
            .preview_cache
            .as_ref()
            .unwrap()
            .2
            .items
            .iter()
            .filter_map(|i| match &i.prim {
                drift_draft::page::Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    assert!(texts(&h).contains(&"MATERIAL".to_owned()));
    save_png("drift_ui_project_before.png", &mut h);

    // Drawing settings → Project: the architectural fields, as one undo step.
    h.state_mut().show_drawing_settings = true;
    h.run();
    h.get_by_label("Architectural").click();
    h.run();
    assert_eq!(project(&h).fields, FieldSet::Architectural);
    assert_eq!(h.state().editor.history.undo_label(), Some("Edit project"));
    {
        let ed = &mut h.state_mut().editor;
        let mut p = ed.doc.project().clone();
        p.name = "Riverside house".into();
        p.number = "2026-014".into();
        p.client = "Harbour Trust".into();
        p.address = "4 Quay Lane, Bristol".into();
        p.company = "Plange Studio".into();
        ed.doc.begin();
        ed.doc.set_project(p);
        let mut s = ed.doc.sheet(ids[1]).unwrap().clone();
        s.fields.status = "For permit".into();
        ed.doc.set_sheet(ids[1], s);
        ed.commit("Project");
    }
    h.run();
    h.run();
    save_png("drift_ui_project_settings.png", &mut h);
    h.state_mut().show_drawing_settings = false;
    h.run();
    h.run();
    let shown = texts(&h);
    for want in [
        "Riverside house",
        "Harbour Trust",
        "2026-014",
        "4 Quay Lane, Bristol",
        "For permit",
        "PLANGE STUDIO",
        "STATUS",
    ] {
        assert!(shown.iter().any(|t| t == want), "{want} in {shown:?}");
    }
    assert!(!shown.contains(&"MATERIAL".to_owned()));
    save_png("drift_ui_project_after.png", &mut h);

    // A sheet's own title wins; the others keep the project's.
    {
        let ed = &mut h.state_mut().editor;
        let mut s = ed.doc.sheet(ids[1]).unwrap().clone();
        s.fields.title = "Ground floor plan".into();
        s.title_block = TitleBlock::Iso;
        ed.doc.begin();
        ed.doc.set_sheet(ids[1], s);
        ed.commit("Own title");
    }
    h.run();
    h.run();
    let iso = texts(&h);
    assert!(iso.iter().any(|t| t == "Ground floor plan"), "{iso:?}");
    assert!(iso.iter().any(|t| t == "Project no."), "{iso:?}");
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pdf = h.state_mut().pdf_bytes(&painter);
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(
        pdf.contains("(Riverside house) Tj"),
        "the other sheet's title"
    );
    // Saved and read back.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    let (read, _) = drift_io::load(&saved).unwrap();
    assert_eq!(read.project(), h.state().editor.doc.project());
}

#[test]
fn view_crops_snap_like_anything_placed() {
    use crate::app::views_ui::CropGrip;
    let Some(mut h) = harness() else { return };
    // Four loose points, and no grid, so only the points pull.
    let [a, b, c, d] = [
        DVec2::new(0.0, 0.0),
        DVec2::new(20.0, 12.0),
        DVec2::new(32.0, 18.0),
        DVec2::new(-30.0, -20.0),
    ];
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        for p in [a, b, c, d] {
            ed.doc.add_point(p);
        }
        ed.commit("Setup");
    }
    h.state_mut().grid_snap = false;
    h.run();
    // A couple of pixels off each point, in world units.
    let off = |h: &Harness<'_, DriftApp>| DVec2::new(2.0, -1.5) / h.state().camera.scale;
    let crop = |h: &Harness<'_, DriftApp>| h.state().editor.doc.views().next().unwrap().1.crop();
    let near = |p: DVec2, q: DVec2| p.distance(q) < 1e-9;

    // The View tool's corners land on the points.
    h.state_mut().set_tool(ToolKind::View);
    h.run();
    let o = off(&h);
    drag_world(&mut h, a + o, b - o);
    let id = h.state().editor.doc.views().next().unwrap().0;
    assert!(
        near(crop(&h).min, a) && near(crop(&h).max, b),
        "{:?}",
        crop(&h)
    );

    // A dragged corner grip lands on a point.
    h.state_mut().picked_view = Some(id);
    h.run();
    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let map = screen_map(&h);
    assert_eq!(
        h.state().crop_grip_at(&painter, &map, map.to_screen(b)),
        Some((id, CropGrip::Resize(1, 1)))
    );
    let (from, to) = (screen(&h, b), screen(&h, c + o));
    drag_steps(&mut h, from, to, 6, Some("drift_ui_crop_snap.png"));
    assert!(near(crop(&h).max, c), "{:?}", crop(&h));
    assert!(near(crop(&h).min, a));

    // Moved by its edge, the corner nearest a point lands on it, keeping its size.
    let size = crop(&h).size();
    // (Along its edge, clear of the grip at the edge's middle.)
    let grab = DVec2::new(a.x, a.y + (c.y - a.y) * 0.25);
    let map = screen_map(&h);
    assert_eq!(
        h.state().crop_grip_at(&painter, &map, map.to_screen(grab)),
        Some((id, CropGrip::Move))
    );
    drag_world(&mut h, grab, grab + (d - a) + o);
    assert!(near(crop(&h).min, d), "{:?}", crop(&h));
    assert!(crop(&h).size().distance(size) < 1e-9);
    assert_eq!(
        h.state().editor.history.undo_label(),
        Some("Move view crop")
    );

    // With Alt held it goes where it's let go.
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    h.run();
    let corner = crop(&h).max;
    drag_world(&mut h, corner, b + o);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    assert!(!near(crop(&h).max, b), "free of snaps: {:?}", crop(&h));
}

#[test]
fn an_annotation_can_be_drawn_in_some_views_only() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let sheet = h.state().current_sheet().unwrap();
    // A second view of the whole drawing beside the first, at half the size.
    let (first, second) = {
        let app = h.state_mut();
        let b = app.fit_bounds();
        let first = app
            .editor
            .doc
            .placed_views(app.editor.doc.sheet(sheet).unwrap())[0]
            .id;
        app.editor.doc.begin();
        let crop = drift_core::Bounds {
            min: b.min - DVec2::splat(5.0),
            max: b.max + DVec2::splat(5.0),
        };
        let v = drift_core::View::new("Detail", crop, 2.0);
        let second = app.editor.doc.add_view(v);
        app.editor.commit("View");
        (first, second)
    };
    h.state_mut()
        .place_view(sheet, second, Some(DVec2::new(330.0, 220.0)));
    h.run();
    assert_eq!(placed(&h, sheet).len(), 2);
    // The overall width, dimensioned once in the model: drawn in both views.
    let width = h
        .state()
        .editor
        .doc
        .constraints()
        .find(|(_, c)| c.is_dimension() && c.value() == Some(120.0))
        .map(|(id, _)| id)
        .expect("the 120 dimension");
    let count_120 = |h: &Harness<'_, DriftApp>| {
        h.state()
            .preview_cache
            .as_ref()
            .unwrap()
            .2
            .items
            .iter()
            .filter(
                |i| matches!(&i.prim, drift_draft::page::Prim::Text { text, .. } if text == "120"),
            )
            .count()
    };
    assert_eq!(count_120(&h), 2);

    // Picked in the model, its Inspector says where it's drawn: All views, or only Detail.
    h.state_mut().show_sheet(None);
    h.state_mut().selected_constraint = Some(width);
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    // (The menu sits beside its row's label.)
    let row = h.get_by_label("In views").rect();
    let at = egui::pos2(row.left() + 160.0, row.center().y);
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    h.run();
    assert!(h.query_by_label("All views").is_some(), "the menu is open");
    let name = h.state().editor.doc.view(first).unwrap().title().to_owned();
    h.get_by_label(&name).click();
    h.run();
    h.run();
    let only = h
        .state()
        .editor
        .doc
        .only_in(drift_core::AnnotationRef::Dimension(width))
        .cloned();
    assert_eq!(only, Some([second].into()));
    assert_eq!(h.state().editor.history.undo_label(), Some("Views it's in"));
    save_png("drift_ui_annotation_views.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().show_sheet(Some(sheet));
    h.run();
    h.run();
    assert_eq!(count_120(&h), 1, "in the detail alone");
    save_png("drift_ui_annotation_views_sheet.png", &mut h);

    // Saved and read back; deleting the detail leaves it off sheets rather than everywhere.
    let saved = drift_io::save(&h.state().editor.doc, &Default::default());
    assert!(
        saved.contains(&format!(r#""views":["{second}"]"#)),
        "{saved}"
    );
    let (read, _) = drift_io::load(&saved).unwrap();
    assert_eq!(
        read.only_in(drift_core::AnnotationRef::Dimension(width)),
        only.as_ref()
    );
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        ed.doc.remove_view(second);
        ed.commit("Delete view");
    }
    let r = drift_core::AnnotationRef::Dimension(width);
    assert_eq!(h.state().editor.doc.only_in(r), None);
    assert!(!h.state().editor.doc.shown(r).sheets);
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.only_in(r).cloned(), only);
    assert!(h.state().editor.doc.shown(r).sheets);
}

#[test]
fn lines_snap_tangent_to_a_circle_and_square_onto_a_line() {
    use drift_core::Constraint;
    use drift_geom::SnapKind;
    let Some(mut h) = harness() else { return };
    h.state_mut().auto_relations = true;
    // A circle about the origin, radius 20, and a line below it.
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 0.0));
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -40.0));
    click_world(&mut h, DVec2::new(40.0, -40.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::L);
    h.run();

    // From (40, 30) the line touches the circle 50 away: at the angle of (40, 30) less
    // acos(20 / 50).
    let from = DVec2::new(40.0, 30.0);
    click_world(&mut h, from);
    let toward = from.y.atan2(from.x);
    let touch = DVec2::from_angle(toward + (20.0f64 / 50.0).acos()) * 20.0;
    h.hover_at(screen(&h, touch));
    h.run();
    let snap = h.state().pointer.and_then(|p| p.snap).expect("a snap");
    assert_eq!(snap.kind, SnapKind::Tangent);
    save_png("drift_ui_tangent_snap.png", &mut h);
    click_world(&mut h, touch);
    let tangent = |h: &Harness<'_, DriftApp>| {
        h.state()
            .editor
            .doc
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::Tangent { .. }))
            .count()
    };
    assert_eq!(tangent(&h), 1, "kept tangent");

    // On from the touching point, square down onto the line.
    let foot = DVec2::new(touch.x, -40.0);
    h.hover_at(screen(&h, foot + DVec2::new(0.4, 0.4)));
    h.run();
    let snap = h.state().pointer.and_then(|p| p.snap).expect("a snap");
    assert_eq!(snap.kind, SnapKind::Perpendicular);
    // Straight below the solved touching point, on the line.
    assert!((snap.at - foot).length() < 0.01);
    save_png("drift_ui_perpendicular_snap.png", &mut h);

    // Turned off in the status bar: only on the curve.
    h.get_by_label("Perpendicular & tangent").click();
    h.run();
    assert!(!h.state().perp_tangent_snap);
    h.hover_at(screen(&h, foot + DVec2::new(0.4, 0.4)));
    h.run();
    let snap = h.state().pointer.and_then(|p| p.snap).expect("a snap");
    assert_eq!(snap.kind, SnapKind::OnCurve);
}

#[test]
fn boxes_pick_annotations_and_text_with_the_filter() {
    use crate::editor::PickFilter;
    use drift_core::{Annotation, AnnotationRef, Constraint};
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    // A 60 × 40 rectangle with its width dimensioned, a note and a line of text beside it.
    let (lines, width, note, text) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let c =
            [v(0.0, 0.0), v(60.0, 0.0), v(60.0, 40.0), v(0.0, 40.0)].map(|p| ed.doc.add_point(p));
        let lines: Vec<_> = (0..4)
            .map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap())
            .collect();
        let width = ed
            .doc
            .add_constraint(Constraint::Distance {
                a: c[0],
                b: c[1],
                value: 60.0,
            })
            .unwrap();
        let note = ed
            .doc
            .add_annotation(Annotation::Note {
                text: "STAIR".into(),
                at: v(110.0, 20.0),
                leaders: Vec::new(),
                style: Default::default(),
            })
            .unwrap();
        let text = ed
            .doc
            .add_text(drift_core::SketchText::new("HALL", v(100.0, -20.0), 5.0));
        ed.commit("Setup");
        let mut fit = drift_core::Bounds::from_point(v(-40.0, -60.0));
        fit.include(v(170.0, 80.0));
        h.state_mut().fit_target = Some(fit);
        (lines, width, note, text)
    };
    h.run();
    let everything = |h: &mut Harness<'_, DriftApp>| {
        drag_world(h, v(-38.0, -58.0), v(168.0, 78.0));
        h.run();
    };
    let picked = |h: &Harness<'_, DriftApp>| h.state().picked_annotations();

    // Annotations only (the select tool's options bar): a window takes the dimension and the
    // note, not the geometry or the text.
    h.get_by_label("Annotations").click();
    h.run();
    assert_eq!(h.state().editor.pick_filter, PickFilter::Annotations);
    everything(&mut h);
    let refs = picked(&h);
    assert!(refs.contains(&AnnotationRef::Dimension(width)), "{refs:?}");
    assert!(refs.contains(&AnnotationRef::Annotation(note)), "{refs:?}");
    assert!(h.state().editor.selection.is_empty());
    assert!(h.state().editor.selected_texts.is_empty());
    // Delete takes both, as one undo step.
    h.key_press(Key::Delete);
    h.run();
    assert!(h.state().editor.doc.constraint(width).is_none());
    assert!(h.state().editor.doc.annotation(note).is_none());
    assert_eq!(h.state().editor.doc.texts().count(), 1);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|l| h.state().editor.doc.get(*l).is_some()));
    h.state_mut().undo();
    h.run();
    assert!(h.state().editor.doc.annotation(note).is_some());

    // A narrow box across the note's text: a crossing box (dragged right to left) takes it,
    // a window the same size doesn't (the text isn't all inside it).
    let at = v(110.0, 20.0);
    drag_world(&mut h, at + v(-0.3, -12.0), at + v(0.3, 12.0));
    h.run();
    assert!(picked(&h).is_empty(), "window: {:?}", picked(&h));
    drag_world(&mut h, at + v(0.3, 12.0), at + v(-0.3, -12.0));
    h.run();
    assert_eq!(picked(&h), vec![AnnotationRef::Annotation(note)]);

    // All: the same window takes everything, and the Inspector says what.
    h.get_by_label("All").click();
    h.run();
    everything(&mut h);
    let ed = &h.state().editor;
    assert!(lines.iter().all(|l| ed.selection.contains(l)));
    assert_eq!(ed.selected_texts, [text].into());
    assert_eq!(picked(&h).len(), 2);
    assert!(
        h.query_by_label_contains("1 dimension").is_some(),
        "the Inspector sums up a mixed pick"
    );
    save_png("drift_ui_box_annotations.png", &mut h);

    // Text only: the window takes just the text. (With something picked the bar shows the
    // filter only while one is on: Esc first. The toolbar's Text tool comes after it.)
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.get_all_by_label("Text").next().unwrap().click();
    h.run();
    assert_eq!(h.state().editor.pick_filter, PickFilter::Text);
    everything(&mut h);
    let ed = &h.state().editor;
    assert!(ed.selection.is_empty());
    assert_eq!(ed.selected_texts, [text].into());
    assert!(picked(&h).is_empty());
}

#[test]
fn dragging_a_hatch_over_another_moves_only_its_own_outline() {
    // The drawing from the crash report: two hatched arrows, the second shifted an inch
    // along the first so their top and bottom edges lie along each other. Dragging the
    // second's hatch used to drag the first's top and bottom edges along too.
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    let text = r#"{"format":"drift","version":31,"meta":{"units":"mm"},"display":{"length":"fractional_in"},
"entities":[
{"type":"point","id":"e164","at":[0.0,76.19999999999999]},{"type":"point","id":"e165","at":[-228.6,76.19999999999999]},
{"type":"line","id":"e166","a":"e164","b":"e165"},{"type":"point","id":"e167","at":[-330.2,38.099999999999994]},
{"type":"line","id":"e168","a":"e165","b":"e167"},{"type":"point","id":"e169","at":[-228.6,0.0]},
{"type":"line","id":"e170","a":"e167","b":"e169"},{"type":"point","id":"e171","at":[0.0,0.0]},
{"type":"line","id":"e172","a":"e169","b":"e171"},{"type":"line","id":"e173","a":"e171","b":"e164"},
{"type":"point","id":"e174","at":[25.400000000000034,76.19999999999999]},{"type":"point","id":"e175","at":[-203.19999999999996,76.19999999999999]},
{"type":"point","id":"e176","at":[-304.79999999999995,38.099999999999994]},{"type":"point","id":"e177","at":[-203.19999999999996,0.0]},
{"type":"point","id":"e178","at":[25.400000000000034,0.0]},
{"type":"line","id":"e179","a":"e174","b":"e175"},{"type":"line","id":"e180","a":"e175","b":"e176"},
{"type":"line","id":"e181","a":"e176","b":"e177"},{"type":"line","id":"e182","a":"e177","b":"e178"},
{"type":"line","id":"e183","a":"e178","b":"e174"}],
"constraints":[],
"fills":[
{"id":"f9","type":"hatch","boundary":["e166","e168","e170","e172","e173"],"seed":[-158.40076079135008,46.29783990313652],"pattern":"ANSI31"},
{"id":"f10","type":"hatch","boundary":["e179","e180","e181","e182","e183"],"seed":[-115.83939393939389,38.099999999999994],"pattern":"ANSI31"}]}"#;
    let (doc, _) = drift_io::load(text).unwrap();
    h.state_mut()
        .replace_document(doc, "arrows".into(), None, Default::default(), true);
    h.run();
    h.state_mut().fit_target = None;
    h.state_mut().camera = drift_render::Camera {
        center: v(-150.0, 38.0),
        scale: 1.8,
    };
    h.run();
    let first = |h: &Harness<'_, DriftApp>| -> Vec<DVec2> {
        [164, 165, 167, 169, 171]
            .map(|i| h.state().editor.doc.point(drift_core::EntityId(i)).unwrap())
            .to_vec()
    };
    let before = first(&h);
    // Pick the second arrow's hatch where only it is, and drag it back and forth over the
    // first, right onto it too. (Each press is well clear of the first arrow's right side at
    // x = 0, so it lands on the hatch, not the line.)
    let mut at = v(10.0, 38.0);
    click_world(&mut h, at);
    assert_eq!(
        h.state().editor.selected_fills,
        [drift_core::FillId(10)].into()
    );
    for d in [-25.4, -12.7, 38.1, -50.8] {
        click_world(&mut h, at);
        drag_world(&mut h, at, at + v(d, 0.0));
        at += v(d, 0.0);
        assert_eq!(first(&h), before, "the first arrow moved");
        let shapes = h.state_mut().editor.fill_shapes();
        assert!(shapes.values().all(|s| !s.lost));
    }
    let moved = h
        .state()
        .editor
        .doc
        .point(drift_core::EntityId(174))
        .unwrap();
    assert!((moved.x - 25.4).abs() > 1.0, "the second arrow moved");
}

#[test]
fn tangent_arcs_and_lines_start_on_a_partial_ellipse_and_a_pen_curve() {
    use drift_core::Constraint;
    let Some(mut h) = harness() else { return };
    h.state_mut().auto_relations = true;
    // A partial ellipse (semi-axes 40 and 20) from -30° to 135°, and a pen curve below it.
    let ellipse_at = |degrees: f64| {
        let t = degrees.to_radians();
        DVec2::new(40.0 * t.cos(), 20.0 * t.sin())
    };
    let (ellipse, end, curve, curve_end) = {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let c = ed.doc.add_point(DVec2::ZERO);
        let major = ed.doc.add_point(DVec2::new(40.0, 0.0));
        let minor = ed.doc.add_point(DVec2::new(0.0, 20.0));
        let s = ed.doc.add_point(ellipse_at(-30.0));
        let e = ed.doc.add_point(ellipse_at(135.0));
        let ellipse = ed.doc.add_ellipse_arc(c, major, minor, s, e).unwrap();
        let a = ed.doc.add_point(DVec2::new(-50.0, -40.0));
        let c1 = ed.doc.add_point(DVec2::new(-40.0, -20.0));
        let c2 = ed.doc.add_point(DVec2::new(-10.0, -60.0));
        let b = ed.doc.add_point(DVec2::new(0.0, -40.0));
        let curve = ed.doc.add_bezier(a, c1, c2, b).unwrap();
        ed.commit("Shapes");
        (ellipse, e, curve, b)
    };
    let tangents = |h: &Harness<'_, DriftApp>| -> Vec<Constraint> {
        h.state()
            .editor
            .doc
            .constraints()
            .map(|(_, c)| *c)
            .filter(|c| matches!(c, Constraint::Tangent { .. }))
            .collect()
    };
    let leaving = |h: &Harness<'_, DriftApp>, c: drift_core::EntityId, p| {
        h.state()
            .editor
            .doc
            .end_direction(c, p)
            .expect("ends there")
    };

    // The reported case: the tangent arc tool on the ellipse's end (A three times).
    for _ in 0..3 {
        h.key_press(Key::A);
        h.run();
    }
    assert_eq!(h.state().tool.kind(), ToolKind::TangentArc);
    let from = ellipse_at(135.0);
    click_world(&mut h, from);
    assert!(
        h.state().tool.is_busy(),
        "started on the partial ellipse's end"
    );
    let dir = leaving(&h, ellipse, end);
    let to = from + dir * 25.0 + dir.perp() * 15.0;
    h.hover_at(screen(&h, to));
    h.run();
    save_png("drift_ui_tangent_arc_on_partial_ellipse.png", &mut h);
    click_world(&mut h, to);
    let arc = h.state().editor.doc.iter().map(|(id, _)| id).max().unwrap();
    assert!(tangents(&h).contains(&Constraint::Tangent { a: arc, b: ellipse }));
    assert!((leaving(&h, arc, end) + dir).length() < 1e-9, "tangent");
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();

    // A line on from the pen curve's end, along its tangent guide, then A for a tangent arc.
    h.key_press(Key::L);
    h.run();
    let from = DVec2::new(0.0, -40.0);
    click_world(&mut h, from);
    let dir = leaving(&h, curve, curve_end);
    let on_guide = from + dir * 20.0;
    h.hover_at(screen(&h, on_guide + dir.perp() * 0.2));
    h.run();
    let guide = h
        .state()
        .pointer
        .and_then(|p| p.guide)
        .expect("the tangent guide");
    assert_eq!(guide.kind, crate::editor::GuideKind::Tangent(curve));
    click_world(&mut h, on_guide + dir.perp() * 0.2);
    let line = h.state().editor.doc.iter().map(|(id, _)| id).max().unwrap();
    assert!(tangents(&h).contains(&Constraint::Tangent { a: line, b: curve }));
    assert!(
        (leaving(&h, line, curve_end) + dir).length() < 1e-6,
        "tangent"
    );
    h.key_press(Key::A);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::TangentArc);
    let next = on_guide + dir * 15.0 - dir.perp() * 15.0;
    h.hover_at(screen(&h, next));
    h.run();
    save_png("drift_ui_tangent_line_and_arc_from_curve.png", &mut h);
    click_world(&mut h, next);
    assert_eq!(tangents(&h).len(), 3);
    assert!(h.state().editor.analysis().conflicts.is_empty());
}

#[test]
fn view_switches_say_so_and_hidden_dimensions_and_relations_come_back_each_start() {
    let Some(mut h) = harness() else { return };
    let status = |h: &Harness<'static, DriftApp>| {
        h.state()
            .status
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default()
    };
    h.key_press_modifiers(EguiModifiers::ALT, Key::D);
    h.run();
    h.run();
    assert!(!h.state().show_dimensions);
    assert!(
        status(&h).starts_with("Dimensions and notes hidden"),
        "{}",
        status(&h)
    );
    h.key_press_modifiers(EguiModifiers::ALT, Key::R);
    h.run();
    h.run();
    assert!(!h.state().show_relations);
    assert!(status(&h).starts_with("Relations hidden"), "{}", status(&h));
    h.key_press(Key::W);
    h.run();
    h.run();
    assert!(h.state().thin_lines);
    assert_eq!(status(&h), "Thin lines");
    save_png("drift_ui_dimensions_relations_hidden.png", &mut h);
    // While they're hidden the status bar says so; a click shows them again.
    h.get_by_label("Relations off · Show").click();
    h.run();
    h.run();
    assert!(h.state().show_relations);
    assert_eq!(status(&h), "Relations shown");
    assert!(h.query_by_label("Relations off · Show").is_none());
    h.state_mut().show_relations = false;
    h.run();

    // Hidden dimensions and relations aren't kept; thin lines are.
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let prefs = h.state().view_prefs();
    let keys: Vec<&str> = prefs
        .split(';')
        .filter_map(|p| p.split('=').next())
        .collect();
    assert!(
        !keys.contains(&"dims") && !keys.contains(&"rels"),
        "{prefs}"
    );
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert!(fresh.state().show_dimensions && fresh.state().show_relations);
    assert!(fresh.state().thin_lines);
    // Preferences saved by an older build that hid them don't either.
    fresh.state_mut().apply_view_prefs("dims=0;rels=0");
    assert!(fresh.state().show_dimensions && fresh.state().show_relations);
}

#[test]
fn drawing_rules_are_the_drawings_and_older_files_get_this_computers() {
    let Some(mut h) = harness() else { return };
    // Preferences from before format 39 held the rules: they become those of new drawings.
    h.state_mut()
        .apply_view_prefs("rlayer=1;rfill=0;stack=0;dfollow=1;gaptol=0.75");
    let mine = drift_core::DrawingRules {
        regions_by_layer: true,
        regions_need_fill: false,
        stacked_fills: false,
        follow_divided_fills: true,
        gap_tolerance: 0.75,
    };
    assert_eq!(h.state().new_rules, mine);
    h.state_mut().perform(crate::Pending::New);
    h.run();
    assert_eq!(h.state().editor.doc.rules(), mine);
    assert!(!h.state().is_dirty(), "a new drawing is clean");

    // An older file opens with them (nothing changes on day one) ...
    let plate = drift_core::demo::mounting_plate();
    let current = drift_io::save(&plate, &drift_io::Metadata::default());
    let older = current.replace(
        &format!(r#""version": {}"#, drift_io::FORMAT_VERSION),
        r#""version": 38"#,
    );
    assert_ne!(older, current);
    let (doc, meta) = drift_io::load(&older).unwrap();
    h.state_mut()
        .replace_document(doc, "old.drift".into(), None, meta, true);
    assert_eq!(h.state().editor.doc.rules(), mine);
    // ... and says so, once.
    assert_eq!(
        h.state().status_next.as_deref(),
        Some(crate::app::document::OLDER_RULES)
    );
    h.run();
    assert!(h.state().status_next.is_none());
    // ... and a current one with its own, whatever this computer's are.
    let (doc, meta) = drift_io::load(&current).unwrap();
    h.state_mut()
        .replace_document(doc, "new.drift".into(), None, meta, true);
    assert_eq!(
        h.state().editor.doc.rules(),
        drift_core::DrawingRules::default()
    );

    // Drawing settings changes them as an undo step, and saves them with the drawing.
    h.state_mut().show_drawing_settings = true;
    h.run();
    h.run();
    save_png("drift_ui_drawing_settings_rules.png", &mut h);
    h.get_by_label("Find closed areas on each layer separately")
        .click();
    h.run();
    h.run();
    assert!(h.state().editor.doc.rules().regions_by_layer);
    assert!(h.state().is_dirty());
    let text = drift_io::save(&h.state().editor.doc, &drift_io::Metadata::default());
    let (loaded, _) = drift_io::load(&text).unwrap();
    assert!(loaded.rules().regions_by_layer);
    h.get_by_label("Use these area and fill rules for new drawings")
        .scroll_to_me();
    h.run();
    h.run();
    h.get_by_label("Use these area and fill rules for new drawings")
        .click();
    h.run();
    assert_eq!(h.state().new_rules, h.state().editor.doc.rules());
    assert!(h.state().view_prefs().contains("rlayer=1"));
    h.state_mut().undo();
    assert!(!h.state().editor.doc.rules().regions_by_layer);
}

#[test]
fn a_dragged_or_typed_tolerance_is_one_undo_step_and_clears_redo() {
    let Some(mut h) = harness() else { return };
    let ed = &mut h.state_mut().editor;
    let start = ed.doc.rules().gap_tolerance;
    // A tick box, then a tolerance edited in three frames of one drag.
    ed.change_rules(|r| r.stacked_fills = !r.stacked_fills);
    ed.change_rules_merging(false, |r| r.gap_tolerance = 1.0);
    ed.change_rules_merging(true, |r| r.gap_tolerance = 1.5);
    ed.change_rules_merging(true, |r| r.gap_tolerance = 2.0);
    ed.undo();
    assert_eq!(ed.doc.rules().gap_tolerance, start, "the drag was one step");
    assert_ne!(
        ed.doc.rules().stacked_fills,
        drift_core::DrawingRules::default().stacked_fills,
        "the tick box was a step of its own"
    );
    // A new drag after an undo is its own step, and what was undone can't be redone over.
    ed.undo();
    assert!(ed.history.can_redo());
    ed.change_rules_merging(false, |r| r.gap_tolerance = 3.0);
    ed.change_rules_merging(true, |r| r.gap_tolerance = 4.0);
    assert!(!ed.history.can_redo());
    ed.undo();
    assert_eq!(ed.doc.rules(), drift_core::DrawingRules::default());
}

#[test]
fn preferences_has_three_pages() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    h.get_by_label("Full screen");
    h.get_by_label("Pan");
    assert!(h.query_by_label("Show the grid").is_none(), "a View switch");
    assert!(h.query_by_label("Reset panel layout").is_none());
    h.get_by_label("Drawing aids").click();
    h.run();
    h.run();
    h.get_by_label("Add relations automatically while drawing");
    // Snapping to the grid and end points are in the View menu only.
    assert!(h.query_by_label("Always show end points").is_none());
    assert!(h.query_by_label("Snap to the grid").is_none());
    assert!(
        h.query_by_label("Find closed areas on each layer separately")
            .is_none(),
        "saved with the drawing"
    );
    save_png("drift_ui_preferences_drawing_aids.png", &mut h);
    h.get_by_label("Updates & Labs").click();
    h.run();
    h.run();
    h.get_by_label("Check for updates when Drift starts");
    h.get_by_label("Let AI assistants edit the open drawing");
}
