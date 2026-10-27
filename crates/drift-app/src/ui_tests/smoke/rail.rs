//! The tool rail: every place on it in view at 1280 × 800 and in a narrow window; a family's
//! flyout by right-click and by holding the button down; the family then shows the tool
//! picked, by key too; the arcs' order; no tooltip covers an open flyout; Move and Rotate on
//! the rail act on the selection.

use crate::commands::{self, Command, RAIL};

use super::*;

fn harness_at(w: f32, h: f32) -> Option<Harness<'static, DriftApp>> {
    harness()?;
    let mut harness = Harness::builder()
        .with_size([w, h])
        .wgpu()
        .build_eframe(|cc| DriftApp::new(cc));
    harness.run();
    Some(harness)
}

/// The rail's widget named `label` (the menu bar and the context bar have some of the same
/// names: the rail's is below them, at the left).
fn on_rail(h: &Harness<'_, DriftApp>, label: &str) -> Option<egui::Rect> {
    h.query_all_by(|n| n.label().is_some_and(|l| l == label))
        .map(|n| n.rect())
        .find(|r| r.left() < 100.0 && r.top() > 66.0)
}

/// Every place on the rail is there, by the name of what it shows, and above the status bar
/// without scrolling.
fn rail_in_view(h: &Harness<'_, DriftApp>, size: &str) {
    let bottom = h.state().canvas_rect.bottom();
    for (_, slots) in RAIL {
        for slot in *slots {
            let shows = h.state().rail_shows(slot);
            let label = commands::spec(shows).unwrap().title.trim_end_matches('…');
            let rect =
                on_rail(h, label).unwrap_or_else(|| panic!("{size}: no {label} on the rail"));
            assert!(
                rect.bottom() <= bottom + 0.5,
                "{size}: {label} is below the fold ({} > {bottom})",
                rect.bottom()
            );
        }
    }
}

#[test]
fn smoke_rail_every_place_in_view() {
    let Some(mut h) = harness() else { return };
    h.run();
    rail_in_view(&h, "1280 × 800");
    save_png("drift_smoke_rail_1280x800.png", &mut h);
    // A narrow window: two columns of icons, all of them in view.
    let Some(mut h) = harness_at(720.0, 800.0) else {
        return;
    };
    h.run();
    rail_in_view(&h, "720 × 800");
    save_png("drift_smoke_rail_720x800.png", &mut h);
    // A short one: shorter rows, still all in view.
    let Some(mut h) = harness_at(1280.0, 700.0) else {
        return;
    };
    h.run();
    rail_in_view(&h, "1280 × 700");
}

#[test]
fn smoke_rail_flyouts_and_families() {
    let Some(mut h) = harness() else { return };
    // Right-click on Trim: its flyout; Extend picks the Extend tool, and Trim's place on the
    // rail shows Extend from then on.
    let trim = on_rail(&h, "Trim").expect("Trim on the rail");
    h.get_all_by_label("Trim")
        .find(|n| n.rect() == trim)
        .unwrap()
        .click_secondary();
    h.run();
    assert!(
        h.state().rail_flyout.is_some(),
        "right-click opens the flyout"
    );
    h.get_by_label("Split");
    h.get_by_label("Extend").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Extend);
    assert!(h.state().rail_flyout.is_none(), "picking closes the flyout");
    assert!(on_rail(&h, "Extend").is_some(), "the family shows Extend");
    assert!(on_rail(&h, "Trim").is_none());
    // The bar switches within the family too.
    h.get_all_by_label("Trim")
        .next()
        .expect("Trim in the bar")
        .click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Trim);

    // Holding the Circle button down opens its flyout; letting go over Ellipse picks it.
    let circle = on_rail(&h, "Circle").expect("Circle on the rail");
    h.hover_at(circle.center());
    h.run();
    button(&h, circle.center(), true);
    for _ in 0..40 {
        h.step();
    }
    assert!(h.state().rail_flyout.is_some(), "holding opens the flyout");
    let ellipse = h
        .get_all_by_label("Ellipse")
        .map(|n| n.rect())
        .find(|r| r.left() > circle.right())
        .expect("Ellipse in the flyout");
    h.hover_at(ellipse.center());
    h.run();
    button(&h, ellipse.center(), false);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Ellipse);
    assert!(on_rail(&h, "Ellipse").is_some(), "the family shows Ellipse");
    save_png("drift_smoke_rail_family.png", &mut h);

    // C, from Select, is the circle again, and the family shows it.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::C);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Circle);
    assert!(
        on_rail(&h, "Circle").is_some(),
        "keys pick the family's tool too"
    );

    // Esc closes a flyout without picking anything.
    // The arcs lead with the 3-point arc, then Tangent, then Centre (#206).
    let arc = on_rail(&h, "3-point arc").expect("3-point arc heads the arcs");
    h.get_all_by_label("3-point arc")
        .find(|n| n.rect() == arc)
        .unwrap()
        .click_secondary();
    h.run();
    assert!(h.state().rail_flyout.is_some());
    let flyout: Vec<f32> = ["3-point arc", "Tangent arc", "Centre arc"]
        .iter()
        .map(|l| {
            h.get_all_by_label(l)
                .map(|n| n.rect())
                .find(|r| r.left() > arc.right())
                .unwrap_or_else(|| panic!("{l} in the flyout"))
                .top()
        })
        .collect();
    assert!(flyout.is_sorted(), "the arcs' flyout order: {flyout:?}");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().rail_flyout.is_none(), "Esc closes the flyout");
    assert_eq!(h.state().tool.kind(), ToolKind::Circle);
    // A is the 3-point arc, and A again steps to Tangent, then Centre.
    for expected in [ToolKind::ThreePointArc, ToolKind::TangentArc, ToolKind::Arc] {
        h.key_press(Key::A);
        h.run();
        assert_eq!(h.state().tool.kind(), expected);
    }
    assert!(on_rail(&h, "Centre arc").is_some(), "the family shows it");
}

/// The tooltips on screen.
fn tooltips(h: &Harness<'_, DriftApp>) -> Vec<egui::Rect> {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Tooltip)
            .filter_map(|l| m.area_rect(l.id))
            .collect()
    })
}

/// The open flyout's rect.
fn flyout_rect(h: &Harness<'_, DriftApp>) -> egui::Rect {
    assert!(h.state().rail_flyout.is_some(), "the flyout is open");
    h.ctx
        .memory(|m| m.area_rect(egui::Id::new("rail_flyout")))
        .expect("the flyout's area")
}

/// Wait long enough for a tooltip to show (the pointer still).
fn rest(h: &mut Harness<'_, DriftApp>) {
    for _ in 0..6 {
        h.step();
    }
    h.run();
}

fn assert_clear_of_flyout(h: &Harness<'_, DriftApp>, what: &str) {
    let flyout = flyout_rect(h);
    for tip in tooltips(h) {
        assert!(
            !tip.shrink(0.5).intersects(flyout),
            "{what}: a tooltip {tip:?} covers the flyout {flyout:?}"
        );
    }
}

#[test]
fn smoke_rail_tooltips_clear_of_the_flyout() {
    let Some(mut h) = harness() else { return };
    let rect = on_rail(&h, "Rectangle").expect("Rectangle on the rail");
    // Resting on the row shows its tooltip.
    h.hover_at(rect.center());
    rest(&mut h);
    assert!(!tooltips(&h).is_empty(), "the row has a tooltip");
    h.hover_at(egui::pos2(640.0, 400.0));
    rest(&mut h);

    // Held open, the pointer still on the row: no tooltip over the flyout.
    h.hover_at(rect.center());
    h.run();
    button(&h, rect.center(), true);
    for _ in 0..12 {
        h.step();
    }
    h.run();
    assert_clear_of_flyout(&h, "held");
    button(&h, rect.center(), false);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().rail_flyout.is_none());

    // Right-clicked, the pointer moving a little on the row: still none.
    h.get_all_by_label("Rectangle")
        .find(|n| n.rect() == rect)
        .unwrap()
        .click_secondary();
    h.run();
    h.hover_at(rect.center() + egui::vec2(2.0, 1.0));
    rest(&mut h);
    assert_clear_of_flyout(&h, "right-clicked");

    // A flyout row's tooltip shows, beside the flyout.
    let polygon = h
        .get_all_by_label("Polygon")
        .map(|n| n.rect())
        .find(|r| r.left() > rect.right())
        .expect("Polygon in the flyout");
    h.hover_at(polygon.center());
    rest(&mut h);
    assert!(!tooltips(&h).is_empty(), "the flyout's row has a tooltip");
    assert_clear_of_flyout(&h, "a flyout row");
    save_png("drift_smoke_rail_flyout_tooltip.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
}

#[test]
fn smoke_rail_move_and_rotate() {
    let Some(mut h) = harness() else { return };
    // Nothing picked: Move does nothing.
    let mv = on_rail(&h, "Move").expect("Move on the rail");
    h.hover_at(mv.center());
    h.run();
    button(&h, mv.center(), true);
    h.run();
    button(&h, mv.center(), false);
    h.run();
    assert!(!h.state().tool.is_modal(), "Move without a selection");
    // A line picked: Move from the rail grabs it, where the pointer last was on the canvas.
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(40.0, 10.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.selection = [line].into_iter().collect();
    h.run();
    h.event(Event::PointerMoved(screen(&h, DVec2::new(20.0, 5.0))));
    h.run();
    h.hover_at(mv.center());
    h.run();
    button(&h, mv.center(), true);
    h.run();
    button(&h, mv.center(), false);
    h.run();
    assert!(h.state().tool.is_modal(), "Move grabs the picked line");
    assert_eq!(h.state().last_command, Some(Command::Move));
    h.key_press(Key::Escape);
    h.run();
    // Rotate too.
    let rotate = on_rail(&h, "Rotate").expect("Rotate on the rail");
    h.hover_at(rotate.center());
    h.run();
    button(&h, rotate.center(), true);
    h.run();
    button(&h, rotate.center(), false);
    h.run();
    assert!(h.state().tool.is_modal(), "Rotate turns the picked line");
    h.key_press(Key::Escape);
    h.run();
}

#[test]
fn smoke_rail_walls_in_build() {
    let Some(mut h) = harness() else { return };
    // Wall is on the rail, under Build, and W picks it.
    let wall = on_rail(&h, "Wall").expect("Wall on the rail");
    let block = on_rail(&h, "Place block").expect("Place block on the rail");
    let mirror = on_rail(&h, "Mirror").expect("Mirror on the rail");
    assert!(wall.top() < block.top() && block.top() < mirror.top());
    h.key_press(Key::W);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    // Its flyout has Walls from curves (dimmed with nothing picked).
    h.get_all_by_label("Wall")
        .find(|n| n.rect() == wall)
        .unwrap()
        .click_secondary();
    h.run();
    h.get_by_label("Walls from curves");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().rail_flyout.is_none());
    assert_eq!(h.state().tool.kind(), ToolKind::Wall);
    // The Build menu has the walls' commands.
    let before = h.query_all_by_label("Join walls").count();
    h.get_by_label("Build").click();
    h.run();
    for item in [
        "Wall",
        "Walls from curves",
        "Walls from lines",
        "Place block",
        "Join walls",
        "Unjoin walls",
        "Explode walls",
    ] {
        assert!(
            h.query_all_by(|n| n
                .label()
                .is_some_and(|l| l == item || l.starts_with(&format!("{item} "))))
                .count()
                > 0,
            "Build → {item}"
        );
    }
    assert!(h.query_all_by_label("Join walls").count() > before);
    save_png("drift_smoke_rail_build_menu.png", &mut h);
    h.key_press(Key::Escape);
    h.run();
}
