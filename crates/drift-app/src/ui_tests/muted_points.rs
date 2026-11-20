//! Corners a fillet or chamfer kept for a dimension, and fillet centres, drawn as muted
//! construction crosses until what they belong to is lit up (#501).

use drift_core::{Constraint, DVec2, EntityId};
use egui_kittest::kittest::Queryable;

use super::*;

/// A 100 × 50 plate whose width is measured to its right corners, both cut: the bottom one
/// filleted, the top one chamfered. Returns (bottom corner, bottom line) at 3440 × 1440.
fn cut_plate() -> Option<(Harness<'static, DriftApp>, EntityId, EntityId)> {
    if !crate::test_gpu::available() {
        return None;
    }
    let mut h = Harness::builder()
        .with_size([3440.0, 1440.0])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    let ed = &mut h.state_mut().drawing.editor;
    ed.prefs.auto_relations = true;
    ed.doc.begin();
    let p = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let bottom = ed.doc.add_line(p[0], p[1]).unwrap();
    for i in 1..4 {
        ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
    }
    ed.doc
        .add_constraint(Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 100.0,
        })
        .unwrap();
    ed.doc
        .add_constraint(Constraint::Distance {
            a: p[3],
            b: p[2],
            value: 100.0,
        })
        .unwrap();
    assert!(ed.commit("Plate"));
    ed.fillet(p[1], 12.0).unwrap();
    ed.chamfer(p[2], 10.0, 10.0).unwrap();
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-20.0, -25.0),
        max: DVec2::new(120.0, 70.0),
    });
    h.run();
    h.run();
    Some((h, p[1], bottom))
}

#[test]
fn kept_corners_are_muted_until_their_lines_are_lit() {
    let Some((mut h, corner, bottom)) = cut_plate() else {
        return;
    };
    save_png("drift_ui_501_kept_corners_rest.png", &mut h);
    // Hovering the bottom line shows its kept corner at full strength.
    let mid = screen(&h, DVec2::new(50.0, 0.0));
    h.hover_at(mid);
    h.run();
    h.run();
    save_png("drift_ui_501_kept_corners_line_hovered.png", &mut h);
    assert_eq!(h.state().drawing.editor.hovered, Some(bottom));
    let ed = &h.state().drawing.editor;
    let item = |id| {
        ed.items_tagged(true, true)
            .into_iter()
            .find(|t| t.entity == Some(id))
            .unwrap()
            .item
    };
    assert!(!item(corner).marker);
    h.hover_at(screen(&h, DVec2::new(50.0, 25.0)));
    h.run();
    let ed = &h.state().drawing.editor;
    let item = ed
        .items_tagged(true, true)
        .into_iter()
        .find(|t| t.entity == Some(corner))
        .unwrap()
        .item;
    assert!(item.marker, "muted again once nothing of it is lit");
    // Still pickable, and the Inspector says what it is.
    click_world(&mut h, DVec2::new(100.0, 0.0));
    assert_eq!(
        h.state().drawing.editor.selection,
        [corner].into(),
        "the kept corner picks"
    );
    assert!(
        h.query_by_label_contains("Corner kept for a dimension")
            .is_some()
    );
}
