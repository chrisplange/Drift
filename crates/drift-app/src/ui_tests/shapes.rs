//! A filled shape is one object in the Inspector: a Stroke (a line or none) and a Fill
//! (none, solid or hatch). Area fills keep the fill's own inspector.

use drift_core::{Appearance, EntityId, FillId, LineType, Rgb};

use super::*;

/// A rectangle from `min` to `max` (its sides, in order round it).
fn rectangle(h: &mut Harness<'_, DriftApp>, min: DVec2, max: DVec2) -> Vec<EntityId> {
    let doc = &mut h.state_mut().editor.doc;
    let p =
        [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)].map(|q| doc.add_point(q));
    (0..4)
        .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect()
}

/// A drawing with a filled shape (a rectangle recorded as one, with a red outline and a
/// blue fill), and two plain rectangles overlapping, their overlap filled as an area.
fn shapes(h: &mut Harness<'_, DriftApp>) -> (Vec<EntityId>, FillId, FillId) {
    h.state_mut().status_colors = false;
    let ed = &mut h.state_mut().editor;
    ed.fill_new_shapes = true;
    ed.fill_style.color = Some(Rgb([0, 0, 255]));
    ed.fill_style.opacity = 1.0;
    ed.doc.begin();
    let sides = rectangle(h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    let ed = &mut h.state_mut().editor;
    for s in &sides {
        ed.doc
            .set_appearance(
                *s,
                Appearance {
                    color: Some(Rgb([255, 0, 0])),
                    weight: Some(drift_core::LineWeight(100)),
                    ..Appearance::default()
                },
            )
            .unwrap();
    }
    ed.add_shape(&sides).expect("a shape");
    ed.fill_new_shapes = false;
    rectangle(h, DVec2::new(60.0, 0.0), DVec2::new(90.0, 30.0));
    rectangle(h, DVec2::new(75.0, 15.0), DVec2::new(105.0, 45.0));
    let ed = &mut h.state_mut().editor;
    ed.commit("Shapes");
    let shape = ed.doc.fills().next().map(|(id, _)| id).unwrap();
    let area = ed
        .add_fill_at(DVec2::new(80.0, 20.0))
        .expect("an area fill");
    assert!(ed.doc.fill(area).unwrap().path.is_none());
    ed.selected_fills.clear();
    h.state_mut().fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-5.0, -5.0),
        max: DVec2::new(110.0, 50.0),
    });
    h.run();
    h.state_mut().fit_target = None;
    (sides, shape, area)
}

/// The colour on screen at a world point.
fn pixel(h: &mut Harness<'_, DriftApp>, w: DVec2) -> [u8; 3] {
    let image = h.render().expect("render");
    let ppp = h.ctx.pixels_per_point();
    let p = screen(h, w);
    let px = image.get_pixel((p.x * ppp) as u32, (p.y * ppp) as u32).0;
    [px[0], px[1], px[2]]
}

fn shows(h: &Harness<'_, DriftApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

#[test]
fn a_filled_shape_is_one_object_with_a_stroke_and_a_fill() {
    let Some(mut h) = harness() else { return };
    let (sides, shape, area) = shapes(&mut h);

    // Picked by its fill: one shape, with its Stroke and its Fill.
    h.state_mut().editor.selected_fills = [shape].into();
    h.run();
    assert!(shows(&h, "Shape"), "titled as a shape");
    assert!(shows(&h, "Line") && shows(&h, "Solid") && shows(&h, "Hatch"));
    assert_eq!(
        h.query_all_by_label("None").count(),
        2,
        "Stroke: none, Fill: none"
    );
    save_png("drift_shape_inspector.png", &mut h);

    // Picked by its whole outline (as a group of it would be): the same.
    h.state_mut().editor.selected_fills.clear();
    h.state_mut().editor.selection = sides.iter().copied().collect();
    h.run();
    assert!(shows(&h, "Shape"));

    // Stroke: none. The outline isn't drawn or exported, but still bounds the fill.
    h.state_mut().editor.selection.clear();
    h.run();
    let red = pixel(&mut h, DVec2::new(20.0, 30.0));
    assert!(
        red[0] > 150 && red[2] < 100,
        "the red outline first: {red:?}"
    );
    h.state_mut().editor.selected_fills = [shape].into();
    h.run();
    // (Picked, the outline is lit up: look with nothing picked.)
    h.get_all_by_label("None").next().unwrap().click();
    h.run();
    let sides_now: Vec<_> = sides
        .iter()
        .map(|s| h.state().editor.doc.get(*s).unwrap().appearance.line_type)
        .collect();
    assert!(
        sides_now.iter().all(|t| *t == Some(LineType::None)),
        "{sides_now:?}"
    );
    assert!(!h.state_mut().editor.fill_shapes()[&shape].lost);
    h.state_mut().editor.selection.clear();
    h.state_mut().editor.selected_fills.clear();
    h.run();
    let edge = pixel(&mut h, DVec2::new(20.0, 30.0));
    assert!(edge[0] < 100, "no red outline: {edge:?}");
    save_png("drift_shape_no_stroke.png", &mut h);
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
    );
    let app = h.state_mut();
    let page =
        crate::output::model_page(&mut app.editor, &app.hatch_patterns, &painter, None, false);
    let svg = drift_io::svg::write(&page);
    assert!(!svg.contains("#ff0000"), "no outline printed: {svg}");
    assert!(svg.contains("#0000ff"), "the fill printed: {svg}");
    // Saved and read again, it is still without a stroke.
    let (doc, _) = drift_io::load(&drift_io::save(&app.editor.doc, &Default::default())).unwrap();
    assert_eq!(
        doc.get(sides[0]).unwrap().appearance.line_type,
        Some(LineType::None)
    );

    // A line again: back to its look.
    h.state_mut().editor.selected_fills = [shape].into();
    h.run();
    // (The last "Line": the toolbar has the Line tool.)
    h.get_all_by_label("Line").last().unwrap().click();
    h.run();
    assert_eq!(
        h.state()
            .editor
            .doc
            .get(sides[0])
            .unwrap()
            .appearance
            .line_type,
        None
    );

    // Fill: none. The fill goes; the shape stays, picked by its outline.
    h.get_all_by_label("None").nth(1).unwrap().click();
    h.run();
    assert!(h.state().editor.doc.fill(shape).is_none());
    assert_eq!(h.state().editor.selection.len(), 4);
    h.state_mut().editor.undo();

    // An area fill keeps the fill's own inspector.
    h.state_mut().editor.selection.clear();
    h.state_mut().editor.selected_fills = [area].into();
    h.run();
    assert!(
        !shows(&h, "Shape") && !shows(&h, "None"),
        "no Stroke, no Fill: none"
    );
    assert!(shows(&h, "Solid"));
}

#[test]
fn a_group_with_a_filled_shape_shows_its_fill() {
    let Some(mut h) = harness() else { return };
    let (sides, shape, _) = shapes(&mut h);
    // The shape and a loose line grouped: picking the group picks their curves.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 40.0));
    let b = ed.doc.add_point(DVec2::new(40.0, 40.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.selection = sides.iter().copied().chain([line]).collect();
    ed.make_group();
    h.run();
    assert!(h.state().editor.selected_group().is_some());
    assert!(
        !shows(&h, "Shape"),
        "more than the shape: the curves' inspector"
    );
    assert!(
        shows(&h, "Solid") && shows(&h, "Hatch"),
        "with the shape's Fill"
    );
    // A hatch for the grouped shape, from there.
    h.get_by_label("Hatch").click();
    h.run();
    assert!(matches!(
        h.state().editor.doc.fill(shape).unwrap().kind,
        drift_core::FillKind::Hatch(_)
    ));
}

#[test]
fn clicking_a_shapes_fill_picks_the_shape_and_shift_d_duplicates_it() {
    let Some(mut h) = harness() else { return };
    let (sides, shape, area) = shapes(&mut h);

    // A click inside the fill picks the shape: its fill and its outline.
    click_world(&mut h, DVec2::new(20.0, 15.0));
    let ed = &h.state().editor;
    assert_eq!(ed.selected_fills, [shape].into());
    assert_eq!(ed.selection, sides.iter().copied().collect());
    assert!(shows(&h, "Shape"));

    // Shift+D: a copy of the whole shape, a shape of its own with its own fill, picked and
    // following the pointer; a click puts it down.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    click_world(&mut h, DVec2::new(30.0, 60.0));
    let ed = &mut h.state_mut().editor;
    assert_eq!(ed.doc.fills().count(), 3, "the copy has its fill");
    let copy = *ed
        .selected_fills
        .iter()
        .next()
        .expect("the copy's fill picked");
    assert_ne!(copy, shape);
    let outline = ed.shape_outline(copy).expect("the copy is a shape");
    assert_eq!(outline.len(), 4);
    assert!(
        outline.iter().all(|c| !sides.contains(c)),
        "its own outline"
    );
    assert_eq!(ed.selection, outline.iter().copied().collect());
    assert_ne!(
        ed.doc.fill(copy).unwrap().path,
        ed.doc.fill(shape).unwrap().path
    );
    assert!(!ed.fill_shapes()[&copy].lost);

    // An area fill is still picked alone.
    click_world(&mut h, DVec2::new(80.0, 20.0));
    let ed = &h.state().editor;
    assert_eq!(ed.selected_fills, [area].into());
    assert!(ed.selection.is_empty());
}
