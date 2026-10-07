//! End-to-end UI tests: drive the real [`DriftApp`] headlessly with egui_kittest, rendering
//! through wgpu. Skipped when no graphics adapter is available.

use drift_core::{DVec2, EntityKind};
use eframe::egui::{self, Event, Key, Modifiers as EguiModifiers, PointerButton, Pos2};
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
    assert_eq!(driven[0].text, "(60)");
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
    assert_eq!(dims.text, "Σ2 1/2\"");
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
