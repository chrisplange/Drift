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
    h.key_press(Key::P);
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
    h.get_by_label("Fillet").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Fillet);
}

#[test]
fn clicking_inside_closed_geometry_selects_its_region() {
    use std::f64::consts::PI;

    let Some(mut h) = harness() else { return };
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.run();
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
    h.state_mut().editor.fill_style.kind = drift_core::FillKind::Solid;
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
        [Annotation::Note { text, leader, .. }] => {
            assert_eq!(text, "DEBURR ALL EDGES");
            assert!(leader.is_some());
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
    let sheet = h.state().editor.doc.sheet(id).cloned().unwrap();
    assert!(h.state().sheet_preview);
    // The 120 × 80 plate with its dimensions fits A3 at 1:1, above the title block.
    assert_eq!(sheet.scale, 1.0);
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

    // Dragging on the sheet moves the drawing within it, as one undo step.
    let rect = h.state().canvas_rect;
    let from = rect.center();
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=4 {
        h.hover_at(from + egui::vec2(10.0 * k as f32, 0.0));
        h.run();
    }
    button(&h, from + egui::vec2(40.0, 0.0), false);
    h.run();
    let moved = h.state().editor.doc.sheet(id).unwrap().center;
    assert!(
        moved.x < sheet.center.x - 1.0,
        "dragging right moves the drawing right"
    );
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.sheet(id).unwrap().center, sheet.center);
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
    h.run();
    h.run();
    assert_eq!(h.state().layout.place(Panel::Layers), Place::Float);
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
    h.get_by_label("Add furniture library").click();
    h.run();
    let chair = h.state().editor.doc.block_named("Chair").expect("library");
    // Place two chairs (the second turned with R), then a bed.
    h.state_mut().start_placing(chair);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Block);
    click_world(&mut h, DVec2::new(15.0, 70.0));
    let at = screen(&h, DVec2::new(105.0, 70.0));
    h.hover_at(at);
    h.run();
    h.key_press(Key::R);
    h.run();
    click_world(&mut h, DVec2::new(105.0, 70.0));
    let bed = h.state().editor.doc.block_named("Double bed").unwrap();
    h.state_mut().start_placing(bed);
    click_world(&mut h, DVec2::new(60.0, 10.0));
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
    click_world(&mut h, DVec2::new(215.0, -130.0));
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
    h.state_mut().import_underlay_file("plan.pdf", bytes, 0.0);
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
    h.key_press(Key::B);
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
    h.get_by_label("Text").click();
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
