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
