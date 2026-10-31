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
    let ed = &mut h.state_mut().drawing.editor;
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
        let ed = &h.state().drawing.editor;
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
    let FillKind::Hatch(aligned) = h
        .state()
        .drawing
        .editor
        .doc
        .fill(hatch)
        .unwrap()
        .kind
        .clone()
    else {
        unreachable!()
    };
    assert!(
        (aligned.angle.rem_euclid(180.0) - 90.0).abs() < 1e-6,
        "{}",
        aligned.angle
    );
    assert_eq!(
        h.state().drawing.editor.selection.len(),
        sides.len(),
        "still picked"
    );
    h.state_mut().undo();
    let FillKind::Hatch(back) = h
        .state()
        .drawing
        .editor
        .doc
        .fill(hatch)
        .unwrap()
        .kind
        .clone()
    else {
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
    assert_eq!(h.state().drawing.editor.selected_fills, [blue].into());
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
    let page = crate::output::model_page(
        &mut app.drawing.editor,
        &app.hatch_patterns,
        &painter,
        None,
        false,
    );
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
    assert_eq!(h.state().drawing.editor.selected_fills, [red].into());
    h.get_all_by_label("None").next().unwrap().click();
    h.run();
    let ed = &h.state().drawing.editor;
    assert!(
        under
            .iter()
            .all(|s| ed.doc.get(*s).unwrap().appearance.line_type == Some(LineType::None))
    );
    assert!(
        !h.state_mut().drawing.editor.fill_shapes()[&red].lost,
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
    assert_eq!(
        h.state().drawing.editor.selection.len(),
        8,
        "both outlines picked"
    );
    h.key_press(Key::T);
    h.run();
    assert!(
        h.state().tool.hint().contains("against 8 picked curves"),
        "{}",
        h.state().tool.hint()
    );
    let depth = h.state().drawing.editor.history.undo_depth();
    // Across the inner square's left and right sides, then its top and bottom.
    drag(&mut h, DVec2::new(15.0, 25.0), DVec2::new(45.0, 25.0), 6);
    assert_eq!(
        h.state().drawing.editor.history.undo_depth(),
        depth + 1,
        "one step"
    );
    drag(&mut h, DVec2::new(35.0, 45.0), DVec2::new(35.0, 15.0), 6);
    let ed = &mut h.state_mut().drawing.editor;
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
    // Each drag undoes on its own. (The first already left one area for both fills, so
    // one fill: the top one's.)
    h.state_mut().undo();
    assert_eq!(h.state().drawing.editor.doc.fills().count(), 1);
    assert!(h.state().drawing.editor.doc.fill(upper).is_some());
    h.state_mut().undo();
    let ed = &mut h.state_mut().drawing.editor;
    assert!(lower.is_some_and(|f| !ed.fill_shapes()[&f].lost));
    assert!(!ed.fill_shapes()[&upper].lost);
}

/// A loose line, not picked.
fn free_line(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) -> EntityId {
    let ed = &mut h.state_mut().drawing.editor;
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
    h.state_mut().drawing.editor.prefs.fill_new_shapes = false;
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
    let radius =
        |h: &Harness<'_, DriftApp>, c: EntityId| match h.state().drawing.editor.doc.shape(c) {
            Some(
                drift_core::Shape::Circle { radius, .. } | drift_core::Shape::Arc { radius, .. },
            ) => radius,
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
        .drawing
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .next()
        .expect("filled");
    assert!(
        h.state()
            .drawing
            .editor
            .doc
            .fill(fill)
            .unwrap()
            .path
            .is_some(),
        "the outer circle's fill"
    );
    // Solid red, to see where it is on screen.
    let ed = &mut h.state_mut().drawing.editor;
    let mut solid = ed.doc.fill(fill).unwrap().clone();
    solid.kind = FillKind::Solid;
    ed.update_fill(fill, solid);
    paint(&mut h, fill, RED);
    for x in [-5.0, 5.0] {
        free_line(&mut h, DVec2::new(x, -40.0), DVec2::new(x, -10.0));
    }
    let ring = std::f64::consts::PI * (r_out * r_out - r_in * r_in);
    let area = h.state_mut().drawing.editor.fill_shapes()[&fill]
        .region
        .area;
    assert!((area - ring).abs() < 1e-6, "{area}: the whole ring");
    // Trim, dragged down between the lines across both circles: one step.
    h.key_press(Key::T);
    h.run();
    let depth = h.state().drawing.editor.history.undo_depth();
    drag(&mut h, DVec2::new(0.0, -15.0), DVec2::new(0.0, -36.0), 6);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(
        h.state().drawing.editor.history.undo_depth(),
        depth + 1,
        "one step"
    );
    let ed = &mut h.state_mut().drawing.editor;
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
    // Saved and opened again: the same cut ring, an area fill.
    let cut_area = cut.area;
    let text = drift_io::save(&h.state().drawing.editor.doc, &Default::default());
    let (doc, _) = drift_io::load(&text).expect("opens");
    h.state_mut()
        .replace_document(doc, "ring".into(), None, Default::default(), true);
    h.run();
    let ed = &mut h.state_mut().drawing.editor;
    assert!(ed.doc.fill(fill).unwrap().path.is_none(), "an area fill");
    let reopened = ed.fill_shapes()[&fill].clone();
    assert!(!reopened.lost);
    assert!((reopened.region.area - cut_area).abs() < 1e-6);
    assert!(!reopened.region.contains(slot) && !reopened.region.contains(disc));
}

/// #202: a rectangle drawn from four lines and filled with H is a shape, as one drawn with
/// R is: the Inspector and the status line say "Shape", a click in it picks it with its
/// outline, Shift+D copies it with its fill, Delete takes it all and Ctrl+Z brings it back.
#[test]
fn smoke_fills_a_filled_outline_of_lines_is_a_shape() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 60.0));
    let corners = [
        DVec2::new(0.0, 0.0),
        DVec2::new(40.0, 0.0),
        DVec2::new(40.0, 30.0),
        DVec2::new(0.0, 30.0),
    ];
    let sides = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let p: Vec<EntityId> = corners.iter().map(|c| ed.doc.add_point(*c)).collect();
        let sides: Vec<EntityId> = (0..4)
            .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        ed.commit("Lines");
        sides
    };
    h.run();
    h.key_press(Key::H);
    h.run();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    h.key_press(Key::Escape);
    h.run();
    let fill = h
        .state()
        .drawing
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .next()
        .expect("filled");
    assert!(
        h.state().drawing.editor.shape_outline(fill).is_some(),
        "a shape"
    );
    // Hovered: the status line says so.
    let inside = screen(&h, DVec2::new(10.0, 10.0));
    h.hover_at(inside);
    h.run();
    h.run();
    assert!(
        h.query_all_by_label_contains("Shape · ").next().is_some(),
        "the status line"
    );
    // A click picks it with its outline; the Inspector shows a shape.
    click_world(&mut h, DVec2::new(10.0, 10.0));
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(
            sides.iter().all(|s| ed.selection.contains(s)),
            "with its outline"
        );
    }
    assert!(
        h.query_all_by_label("Stroke").next().is_some(),
        "the shape's Inspector"
    );
    // Shift+D: a copy, with its fill.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::D);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(
        h.state().drawing.editor.doc.fills().count(),
        2,
        "copied with its fill"
    );
    h.state_mut().undo();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.fills().count(), 1);
    // Delete: the fill and its outline go; Ctrl+Z brings them back.
    click_world(&mut h, DVec2::new(10.0, 10.0));
    h.key_press(Key::Delete);
    h.run();
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.doc.fills().count(), 0);
        assert!(
            sides.iter().all(|s| !ed.doc.contains(*s)),
            "with its outline"
        );
    }
    h.state_mut().undo();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.fills().count(), 1);
    assert!(
        sides
            .iter()
            .all(|s| h.state().drawing.editor.doc.contains(*s))
    );
}

/// #202: corners drawn up to each other but not joined leave an area fill; the Inspector
/// says why and offers Join corners, which makes it a shape.
#[test]
fn smoke_fills_join_corners_makes_a_shape() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 60.0));
    {
        let ed = &mut h.state_mut().drawing.editor;
        let corners = [
            DVec2::new(0.0, 0.0),
            DVec2::new(40.0, 0.0),
            DVec2::new(40.0, 30.0),
            DVec2::new(0.0, 30.0),
        ];
        ed.doc.begin();
        for i in 0..4 {
            let a = ed.doc.add_point(corners[i]);
            let b = ed.doc.add_point(corners[(i + 1) % 4]);
            ed.doc.add_line(a, b).unwrap();
        }
        ed.commit("Lines");
    }
    h.run();
    h.key_press(Key::H);
    h.run();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    h.key_press(Key::Escape);
    h.run();
    let fill = h
        .state()
        .drawing
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .next()
        .expect("filled");
    assert!(
        h.state().drawing.editor.shape_outline(fill).is_none(),
        "an area fill"
    );
    click_world(&mut h, DVec2::new(10.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_fills, [fill].into());
    assert!(
        h.query_all_by_label_contains("Not a shape:")
            .next()
            .is_some()
    );
    h.get_by_label("Join corners").click();
    h.run();
    assert!(
        h.state().drawing.editor.shape_outline(fill).is_some(),
        "a shape now"
    );
    save_png("drift_smoke_join_corners.png", &mut h);
    // Undone, and the same from the command palette (it is in the Modify menu too).
    h.state_mut().undo();
    h.run();
    assert!(h.state().drawing.editor.shape_outline(fill).is_none());
    h.state_mut().drawing.editor.selected_fills = [fill].into();
    h.run();
    palette(&mut h, "Join corners");
    assert!(
        h.state().drawing.editor.shape_outline(fill).is_some(),
        "from the palette"
    );
}

/// #202: two rooms sharing a wall stay area fills, and the Inspector and the status line
/// say why.
#[test]
fn smoke_fills_rooms_sharing_a_wall_stay_area_fills_and_say_why() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 60.0));
    {
        let ed = &mut h.state_mut().drawing.editor;
        let at = [
            DVec2::new(0.0, 0.0),
            DVec2::new(40.0, 0.0),
            DVec2::new(80.0, 0.0),
            DVec2::new(80.0, 40.0),
            DVec2::new(40.0, 40.0),
            DVec2::new(0.0, 40.0),
        ];
        ed.doc.begin();
        let p: Vec<EntityId> = at.iter().map(|c| ed.doc.add_point(*c)).collect();
        for i in 0..6 {
            ed.doc.add_line(p[i], p[(i + 1) % 6]).unwrap();
        }
        // The wall between them, from corner to corner.
        ed.doc.add_line(p[1], p[4]).unwrap();
        ed.commit("Rooms");
    }
    h.run();
    h.key_press(Key::H);
    h.run();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    let first = h
        .state()
        .drawing
        .editor
        .doc
        .fills()
        .map(|(id, _)| id)
        .next()
        .unwrap();
    // Filled first, with nothing else round its lines: a shape.
    assert!(h.state().drawing.editor.shape_outline(first).is_some());
    click_world(&mut h, DVec2::new(70.0, 10.0));
    h.key_press(Key::Escape);
    h.run();
    let ed = &h.state().drawing.editor;
    assert_eq!(ed.doc.fills().count(), 2);
    let second = ed
        .doc
        .fills()
        .map(|(id, _)| id)
        .find(|f| *f != first)
        .unwrap();
    // The second room's wall is the first's shape's side: an area fill, and the first
    // stays a shape (order matters; the Inspector says which is which).
    assert!(ed.shape_outline(second).is_none(), "an area fill");
    assert!(ed.shape_outline(first).is_some(), "still a shape");
    click_world(&mut h, DVec2::new(70.0, 10.0));
    assert_eq!(h.state().drawing.editor.selected_fills, [second].into());
    assert!(h.query_all_by_label_contains("Area").next().is_some());
    assert!(
        h.query_all_by_label_contains("Not a shape:")
            .next()
            .is_some(),
        "the reason"
    );
    let inside = screen(&h, DVec2::new(70.0, 10.0));
    h.hover_at(inside);
    h.run();
    h.run();
    assert!(
        h.query_all_by_label_contains("Area fill: ")
            .next()
            .is_some(),
        "the status line"
    );
}

/// #223: a picked filled shape's status line says a double-click inside edits its fill on
/// its own, and its right-click menu's Edit fill does it (as does the palette). Once the fill
/// has been picked alone a few times, the hint goes.
#[test]
fn smoke_fills_picked_shape_says_how_to_edit_its_fill() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 50.0));
    let (sides, fill) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let fill = fill.unwrap();
    h.state_mut().drawing.editor.prefs.fill_alone_picks = 0;
    const HINT: &str = "Double-click inside to edit its fill on its own";
    let inside = DVec2::new(20.0, 15.0);
    let whole = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().drawing.editor;
        ed.selection == sides.iter().copied().collect() && ed.selected_fills == [fill].into()
    };
    let alone = |h: &Harness<'_, DriftApp>| {
        let ed = &h.state().drawing.editor;
        ed.selection.is_empty() && ed.selected_fills == [fill].into()
    };
    // Picked whole: the hint.
    click_world(&mut h, inside);
    assert!(whole(&h));
    assert!(h.query_by_label_contains(HINT).is_some(), "the hint");

    // Right-click, Edit fill: the fill alone, and no hint for it.
    let pos = screen(&h, inside);
    right_click(&mut h, pos);
    h.get_by_label("Edit fill").click();
    h.run();
    assert!(alone(&h));
    assert!(h.query_by_label_contains(HINT).is_none());
    assert_eq!(h.state().drawing.editor.prefs.fill_alone_picks, 1);

    // One side picked isn't the shape: no hint. The palette's Edit fill picks the fill alone
    // too.
    // (Off its middle, which would pick its midpoint.)
    click_world(&mut h, DVec2::new(10.0, 0.0));
    assert_eq!(h.state().drawing.editor.selection.len(), 1, "one side");
    assert!(
        h.query_by_label_contains(HINT).is_none(),
        "one side isn't the shape"
    );
    // (Esc first: the side's temporary dimensions lie across the shape.)
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, inside);
    assert!(whole(&h));
    palette(&mut h, "Edit fill");
    assert!(alone(&h));

    // A double-click inside is the third time: no more hints.
    click_world(&mut h, inside);
    assert!(whole(&h));
    assert!(h.query_by_label_contains(HINT).is_some());
    let pos = screen(&h, inside);
    double_click(&mut h, pos);
    assert!(alone(&h));
    assert_eq!(h.state().drawing.editor.prefs.fill_alone_picks, 3);
    click_world(&mut h, inside);
    assert!(whole(&h));
    assert!(h.query_by_label_contains(HINT).is_none(), "learnt");
    // Kept with the view preferences.
    let prefs = h.state().view_prefs();
    assert!(prefs.contains("fillalone=3"), "{prefs}");
}

/// #266: right-click Move to layer on a filled shape takes its fill along, and one undo puts
/// both back.
#[test]
fn smoke_fills_move_to_layer_takes_the_shapes_fill() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0));
    let (sides, fill) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let fill = fill.unwrap();
    let a = h.state().drawing.editor.doc.current_layer();
    let b = h.state_mut().drawing.editor.add_layer("Moved here");
    h.run();
    let on = |h: &Harness<'_, DriftApp>, layer| {
        let doc = &h.state().drawing.editor.doc;
        sides.iter().all(|s| doc.get(*s).unwrap().layer == layer)
            && doc.fill(fill).unwrap().layer == layer
    };
    let inside = DVec2::new(20.0, 15.0);
    click_world(&mut h, inside);
    let pos = screen(&h, inside);
    right_click(&mut h, pos);
    h.get_by_label_contains("Move to layer").click();
    h.run();
    h.get_by_label("Moved here").click();
    h.run();
    assert!(on(&h, b), "the outline and its fill on the new layer");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run();
    assert!(on(&h, a), "one undo puts both back");
}
