//! Fills and hatches: solid and hatched shapes drawn with the Rectangle tool, the pattern's
//! handles on a picked hatched shape, Align to edge, Arrange, Stroke: none, and print stacking
//! as the screen does.

use drift_core::{LineType, Rgb};

use super::*;
use crate::ui_tests::stacking::first_painted;

const RED: Rgb = Rgb([255, 0, 0]);
const BLUE: Rgb = Rgb([0, 0, 255]);

/// The colour on screen at a world point.
fn pixel(h: &mut Harness<'_, DriftApp>, w: DVec2) -> [u8; 3] {
    let image = h.render().expect("render");
    let ppp = h.ctx.pixels_per_point();
    let p = screen(h, w);
    let px = image.get_pixel((p.x * ppp) as u32, (p.y * ppp) as u32).0;
    [px[0], px[1], px[2]]
}

fn near(px: [u8; 3], c: Rgb) -> bool {
    px.iter().zip(c.0).all(|(a, b)| a.abs_diff(b) < 40)
}

fn paint(h: &mut Harness<'_, DriftApp>, fill: FillId, color: Rgb) {
    let ed = &mut h.state_mut().editor;
    let mut f = ed.doc.fill(fill).unwrap().clone();
    f.color = Some(color);
    f.opacity = 1.0;
    ed.update_fill(fill, f);
}

/// #158: a hatched shape picked by a click inside shows its pattern's handles and the
/// Inspector's Pattern alignment, and Align to edge turns the pattern along a side.
#[test]
fn smoke_fills_solid_and_hatched_shapes_and_hatch_handles() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 50.0));
    let (_, solid) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let (sides, hatch) = rectangle(
        &mut h,
        DVec2::new(60.0, 0.0),
        DVec2::new(100.0, 30.0),
        Fill::Hatch,
    );
    let (solid, hatch) = (solid.unwrap(), hatch.unwrap());
    assert!(!is_hatch(&h, solid) && is_hatch(&h, hatch));
    // A click inside the hatched shape picks the shape: outline and hatch.
    click_world(&mut h, DVec2::new(80.0, 15.0));
    {
        let ed = &h.state().editor;
        assert_eq!(ed.selected_fills, [hatch].into());
        assert_eq!(ed.selection.len(), 4, "with its outline");
    }
    let map = screen_map(&h);
    assert!(
        h.state_mut().hatch_handles(&map).is_some(),
        "the pattern's origin and angle handles show"
    );
    assert!(h.query_by_label("Pattern alignment").is_some());
    save_png("drift_smoke_hatch_handles.png", &mut h);
    // Align to edge…, then a click on the right side: the pattern runs up it.
    h.get_by_label("Align to edge…").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    let FillKind::Hatch(aligned) = h.state().editor.doc.fill(hatch).unwrap().kind.clone() else {
        unreachable!()
    };
    assert!(
        (aligned.angle.rem_euclid(180.0) - 90.0).abs() < 1e-6,
        "{}",
        aligned.angle
    );
    assert_eq!(
        h.state().editor.selection.len(),
        sides.len(),
        "still picked"
    );
    h.state_mut().undo();
    let FillKind::Hatch(back) = h.state().editor.doc.fill(hatch).unwrap().kind.clone() else {
        unreachable!()
    };
    assert_eq!(back.angle, 0.0, "one undo step");
}

#[test]
fn smoke_fills_arrange_stroke_none_and_print_order() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(70.0, 70.0));
    let (under, red) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 40.0), Fill::Solid);
    let (_, blue) = rectangle(
        &mut h,
        DVec2::new(20.0, 20.0),
        DVec2::new(60.0, 60.0),
        Fill::Solid,
    );
    let (red, blue) = (red.unwrap(), blue.unwrap());
    paint(&mut h, red, RED);
    paint(&mut h, blue, BLUE);
    h.run();
    // Drawn last, the blue square is on top.
    let overlap = DVec2::new(30.0, 30.0);
    assert!(near(pixel(&mut h, overlap), BLUE));
    // Picked and sent backward (Ctrl+[): the red one shows over it.
    click_world(&mut h, DVec2::new(50.0, 50.0));
    assert_eq!(h.state().editor.selected_fills, [blue].into());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::OpenBracket);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(
        near(pixel(&mut h, overlap), RED),
        "{:?}",
        pixel(&mut h, overlap)
    );
    save_png("drift_smoke_arrange.png", &mut h);
    // The PDF and SVG paint it in the same order: blue first, then red over it.
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
    );
    let app = h.state_mut();
    let page =
        crate::output::model_page(&mut app.editor, &app.hatch_patterns, &painter, None, false);
    let svg = drift_io::svg::write(&page);
    let pdf = drift_io::pdf::write(std::slice::from_ref(&page), "Arrange");
    for (format, art) in [
        ("SVG", drift_io::svg_import::read(&svg, "a").expect("reads")),
        (
            "PDF",
            drift_io::pdf_import::read_artwork(&pdf, 1, "a").expect("reads"),
        ),
    ] {
        let first = first_painted(&art, &[BLUE, RED]);
        assert!(first.is_sorted(), "{format}: {first:?}");
    }
    // Stroke: none on the red square, from the Inspector: its outline stays, undrawn.
    click_world(&mut h, DVec2::new(10.0, 10.0));
    assert_eq!(h.state().editor.selected_fills, [red].into());
    h.get_all_by_label("None").next().unwrap().click();
    h.run();
    let ed = &h.state().editor;
    assert!(
        under
            .iter()
            .all(|s| ed.doc.get(*s).unwrap().appearance.line_type == Some(LineType::None))
    );
    assert!(
        !h.state_mut().editor.fill_shapes()[&red].lost,
        "still bounds its fill"
    );
}

/// Two overlapping filled squares, both picked, joined by trimming them against each other
/// (the shapes picked are the only cutting curves): each drag across pieces is one undo
/// step, and the joined outline keeps one fill, the top one's.
#[test]
fn smoke_trim_two_filled_shapes_into_one() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(70.0, 70.0));
    // A line across both (clear of the drags below), which mustn't cut them while they are
    // picked.
    let (la, lb) = (DVec2::new(30.0, -2.5), DVec2::new(65.0, 50.0));
    let line = free_line(&mut h, la, lb);
    let (_, lower) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 40.0), Fill::Solid);
    let (_, upper) = rectangle(
        &mut h,
        DVec2::new(20.0, 20.0),
        DVec2::new(60.0, 60.0),
        Fill::Solid,
    );
    let upper = upper.unwrap();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    let other = screen(&h, DVec2::new(50.0, 50.0));
    click_with(&mut h, other, EguiModifiers::SHIFT);
    assert_eq!(h.state().editor.selection.len(), 8, "both outlines picked");
    h.key_press(Key::T);
    h.run();
    assert!(
        h.state().tool.hint().contains("against 8 picked curves"),
        "{}",
        h.state().tool.hint()
    );
    let depth = h.state().editor.history.undo_depth();
    // Across the inner square's left and right sides, then its top and bottom.
    drag(&mut h, DVec2::new(15.0, 25.0), DVec2::new(45.0, 25.0), 6);
    assert_eq!(h.state().editor.history.undo_depth(), depth + 1, "one step");
    drag(&mut h, DVec2::new(35.0, 45.0), DVec2::new(35.0, 15.0), 6);
    let ed = &mut h.state_mut().editor;
    assert_eq!(ed.history.undo_depth(), depth + 2);
    assert_eq!(ed.doc.fills().count(), 1, "one fill");
    assert!(ed.doc.fill(upper).is_some(), "the top one's");
    let area = ed.fill_shapes()[&upper].region.area;
    assert!((area - 2800.0).abs() < 1e-6, "{area}: the whole outline");
    assert_eq!(
        ed.doc.shape(line),
        Some(drift_core::Shape::Line { a: la, b: lb }),
        "the line wasn't cut"
    );
    save_png("drift_smoke_trim_union.png", &mut h);
    // Each drag undoes on its own.
    h.state_mut().undo();
    assert_eq!(h.state().editor.doc.fills().count(), 2);
    h.state_mut().undo();
    let ed = &mut h.state_mut().editor;
    assert!(lower.is_some_and(|f| !ed.fill_shapes()[&f].lost));
    assert!(!ed.fill_shapes()[&upper].lost);
}

/// A loose line, not picked.
fn free_line(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) -> EntityId {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = ed.doc.add_point(a);
    let q = ed.doc.add_point(b);
    let line = ed.doc.add_line(p, q).unwrap();
    ed.commit("Line");
    h.run();
    line
}

/// #201: a ring filled with the Fill tool (two circles drawn with the Circle tool, the inner
/// one lower), two lines down across its bottom, and a trim dragged down between them across
/// both circles. The fill is cut like the ring: the slot is empty and the inner disc stays
/// a hole. (It was the outer circle's fill, which kept its last shape, a solid disc.)
#[test]
fn smoke_fills_trimming_a_slot_out_of_a_filled_ring() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-45.0, -50.0), DVec2::new(45.0, 40.0));
    h.state_mut().editor.fill_new_shapes = false;
    let before = ids(&h);
    h.key_press(Key::C);
    h.run();
    for (centre, rim) in [
        (DVec2::ZERO, DVec2::new(30.0, 0.0)),
        (DVec2::new(0.0, -8.0), DVec2::new(15.0, -8.0)),
    ] {
        click_world(&mut h, centre);
        click_world(&mut h, rim);
    }
    for _ in 0..2 {
        h.key_press(Key::Escape);
        h.run();
    }
    let circles = curves(&h, &added(&h, &before));
    assert_eq!(circles.len(), 2, "two circles drawn");
    let radius = |h: &Harness<'_, DriftApp>, c: EntityId| match h.state().editor.doc.shape(c) {
        Some(drift_core::Shape::Circle { radius, .. } | drift_core::Shape::Arc { radius, .. }) => {
            radius
        }
        other => panic!("{other:?}"),
    };
    let (outer, inner) = if radius(&h, circles[0]) > radius(&h, circles[1]) {
        (circles[0], circles[1])
    } else {
        (circles[1], circles[0])
    };
    let (r_out, r_in) = (radius(&h, outer), radius(&h, inner));
    assert!(
        (r_out - 30.0).abs() < 0.5 && (r_in - 15.0).abs() < 0.5,
        "{r_out} {r_in}"
    );
    // The Fill tool, inside the ring.
    h.key_press(Key::H);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 25.0));
    h.key_press(Key::Escape);
    h.run();
    let fill = h
        .state()
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .next()
        .expect("filled");
    assert!(
        h.state().editor.doc.fill(fill).unwrap().path.is_some(),
        "the outer circle's fill"
    );
    // Solid red, to see where it is on screen.
    let ed = &mut h.state_mut().editor;
    let mut solid = ed.doc.fill(fill).unwrap().clone();
    solid.kind = FillKind::Solid;
    ed.update_fill(fill, solid);
    paint(&mut h, fill, RED);
    for x in [-5.0, 5.0] {
        free_line(&mut h, DVec2::new(x, -40.0), DVec2::new(x, -10.0));
    }
    let ring = std::f64::consts::PI * (r_out * r_out - r_in * r_in);
    let area = h.state_mut().editor.fill_shapes()[&fill].region.area;
    assert!((area - ring).abs() < 1e-6, "{area}: the whole ring");
    // Trim, dragged down between the lines across both circles: one step.
    h.key_press(Key::T);
    h.run();
    let depth = h.state().editor.history.undo_depth();
    drag(&mut h, DVec2::new(0.0, -15.0), DVec2::new(0.0, -36.0), 6);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().editor.history.undo_depth(), depth + 1, "one step");
    let ed = &mut h.state_mut().editor;
    for c in [outer, inner] {
        assert!(
            matches!(ed.doc.shape(c), Some(drift_core::Shape::Arc { .. })),
            "both circles trimmed"
        );
    }
    let shape = ed.fill_shapes()[&fill].clone();
    assert!(!shape.lost, "the fill found its area");
    let (slot, disc, ring_top) = (
        DVec2::new(0.0, -26.0),
        DVec2::new(0.0, -8.0),
        DVec2::new(0.0, 25.0),
    );
    // The ring less the slot between the lines: the drawing's area there.
    let regions = ed.regions();
    let cut = drift_geom::regions::region_at(&regions, ring_top).expect("an area");
    assert!(cut.area < ring - 50.0);
    assert!(
        (shape.region.area - cut.area).abs() < 1e-6,
        "{} {}",
        shape.region.area,
        cut.area
    );
    assert!(shape.region.contains(ring_top));
    assert!(!shape.region.contains(slot), "the slot cut out is empty");
    assert!(!shape.region.contains(disc), "the inner disc is a hole");
    // And on screen (not picked, so not tinted).
    ed.selection.clear();
    ed.selected_fills.clear();
    h.run();
    save_png("drift_smoke_trim_ring.png", &mut h);
    assert!(
        near(pixel(&mut h, ring_top), RED),
        "{:?}",
        pixel(&mut h, ring_top)
    );
    assert!(!near(pixel(&mut h, slot), RED), "{:?}", pixel(&mut h, slot));
    assert!(!near(pixel(&mut h, disc), RED), "{:?}", pixel(&mut h, disc));
}
