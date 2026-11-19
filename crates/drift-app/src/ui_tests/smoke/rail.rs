//! The tool rail: every place on it in view without scrolling at 1280 × 800, in a narrow
//! window and in a short one, as the slim rail of icons and as the list with names; the
//! order within its groups; the "Show tool names" toggle and that it is kept; a family's
//! flyout by right-click and by holding the button down, to the right of the rail; the family
//! then shows the tool picked, by key too; the arcs' order; a place's tooltip beside the
//! rail, and none over an open flyout; Move and Rotate on the rail act on the selection; the
//! sheet's rail in both forms.

use crate::commands::{self, Command, RAIL};

use super::*;

fn harness_at(w: f32, h: f32) -> Option<Harness<'static, DriftApp>> {
    harness()?;
    let mut harness = Harness::builder()
        .with_size([w, h])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    harness.run();
    Some(harness)
}

/// The slim rail's width with one column of tiles and with two, and the list's with names.
const SLIM: f32 = 48.0;
const TWO_COLUMNS: f32 = 84.0;
const NAMES: f32 = 168.0;

/// The rail's widget named `label` (the top bar and the options bar have some of the same
/// names: the rail's is below them both, at the left).
fn on_rail(h: &Harness<'_, DriftApp>, label: &str) -> Option<egui::Rect> {
    h.query_all_by(|n| n.label().is_some_and(|l| l == label))
        .map(|n| n.rect())
        .find(|r| r.left() < 100.0 && r.top() > crate::theme::bar::TOP + crate::theme::bar::OPTIONS)
}

/// The rail's right edge: where the canvas starts (nothing is docked at the left).
fn rail_edge(h: &Harness<'_, DriftApp>) -> f32 {
    h.state().drawing.view.canvas_rect.left()
}

/// Switch between the slim rail and the list with names, by the toggle at the rail's foot.
fn toggle_names(h: &mut Harness<'_, DriftApp>) {
    h.get_by_label("Show tool names").click();
    h.run();
}

/// Every place on the rail is there, by the name of what it shows, inside the rail and above
/// the status bar without scrolling; the places read in the registry's order, group after
/// group, top to bottom (left to right where there are two columns). Returns their rects.
fn rail_in_view(h: &Harness<'_, DriftApp>, size: &str) -> Vec<egui::Rect> {
    let canvas = h.state().drawing.view.canvas_rect;
    let top = crate::theme::bar::TOP + crate::theme::bar::OPTIONS;
    let mut rects: Vec<egui::Rect> = Vec::new();
    for (_, slots) in RAIL {
        for slot in *slots {
            let shows = h.state().rail_shows(slot);
            let label = commands::spec(shows).unwrap().title.trim_end_matches('…');
            let rect =
                on_rail(h, label).unwrap_or_else(|| panic!("{size}: no {label} on the rail"));
            assert!(
                rect.bottom() <= canvas.bottom() + 0.5 && rect.top() >= top,
                "{size}: {label} is out of view ({rect:?}, the rail ends at {})",
                canvas.bottom()
            );
            assert!(
                rect.left() >= 0.0 && rect.right() <= canvas.left() + 0.5,
                "{size}: {label} is not inside the rail ({rect:?})"
            );
            if let Some(before) = rects.last() {
                assert!(
                    rect.top() >= before.bottom() - 0.5
                        || (rect.top() == before.top() && rect.left() >= before.right()),
                    "{size}: {label} is out of order ({rect:?} after {before:?})"
                );
            }
            rects.push(rect);
        }
    }
    // The toggle at the foot is in view too, under the last place.
    let foot = on_rail(h, "Show tool names").unwrap_or_else(|| panic!("{size}: no toggle"));
    assert!(
        foot.bottom() <= canvas.bottom() + 0.5 && foot.top() >= rects.last().unwrap().bottom(),
        "{size}: the toggle is out of view or over a place ({foot:?})"
    );
    rects
}

#[test]
fn smoke_rail_every_place_in_view() {
    // The slim rail: one column of icon tiles, 48 wide.
    let Some(mut h) = harness() else { return };
    h.run();
    assert!(!h.state().rail_names, "the rail starts slim");
    assert_eq!(rail_edge(&h), SLIM);
    let tiles = rail_in_view(&h, "1280 × 800");
    assert!(
        tiles.iter().all(|r| r.left() == tiles[0].left()
            && r.width() == 36.0
            && (26.0..=30.0).contains(&r.height())),
        "one column of 36-wide tiles, 26 to 30 high: {tiles:?}"
    );
    save_png("drift_smoke_rail_1280x800.png", &mut h);
    // With names: rows as wide as the list.
    toggle_names(&mut h);
    assert_eq!(rail_edge(&h), NAMES);
    let rows = rail_in_view(&h, "1280 × 800, names");
    assert!(rows.iter().all(|r| r.width() > 140.0), "rows: {rows:?}");
    save_png("drift_smoke_rail_1280x800_names.png", &mut h);

    // A narrow window: the same slim column, and the list when asked for.
    let Some(mut h) = harness_at(720.0, 800.0) else {
        return;
    };
    h.run();
    assert_eq!(rail_edge(&h), SLIM);
    rail_in_view(&h, "720 × 800");
    save_png("drift_smoke_rail_720x800.png", &mut h);
    toggle_names(&mut h);
    assert_eq!(rail_edge(&h), NAMES);
    rail_in_view(&h, "720 × 800, names");

    // A short one: too short for one column of tiles, so two, all in view.
    let Some(mut h) = harness_at(1280.0, 700.0) else {
        return;
    };
    h.run();
    assert_eq!(rail_edge(&h), TWO_COLUMNS);
    let tiles = rail_in_view(&h, "1280 × 700");
    assert!(
        tiles.iter().any(|r| r.left() > tiles[0].left()),
        "two columns: {tiles:?}"
    );
    save_png("drift_smoke_rail_1280x700.png", &mut h);
    // With names: shorter rows, still all in view.
    toggle_names(&mut h);
    assert_eq!(rail_edge(&h), NAMES);
    rail_in_view(&h, "1280 × 700, names");
    save_png("drift_smoke_rail_1280x700_names.png", &mut h);
}

/// The toggle at the rail's foot shows the tools' names and hides them again; either way the
/// same tool is picked by the same name, and the choice is kept with the view preferences.
#[test]
fn smoke_rail_names_toggle_is_kept() {
    let Some(mut h) = harness() else { return };
    h.run();
    assert!(h.state().view_prefs().ends_with("railnames=0"));
    let tile = on_rail(&h, "Line").expect("Line on the rail");
    toggle_names(&mut h);
    assert!(h.state().rail_names);
    assert!(h.state().view_prefs().ends_with("railnames=1"));
    let row = on_rail(&h, "Line").expect("Line on the rail, with names");
    assert!(
        row.width() > 3.0 * tile.width(),
        "a row, not a tile: {row:?}"
    );
    // The row picks the tool as the tile does.
    h.get_all_by_label("Line")
        .find(|n| n.rect() == row)
        .unwrap()
        .click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    // Back to the slim rail: the tile is where it was, and the tool is still the Line.
    toggle_names(&mut h);
    assert!(!h.state().rail_names);
    assert_eq!(on_rail(&h, "Line"), Some(tile));
    assert_eq!(h.state().tool.kind(), ToolKind::Line);
    assert!(h.state().view_prefs().ends_with("railnames=0"));

    // Read back at the next start: the list with names, and the slim rail again.
    let Some(mut fresh) = harness() else { return };
    fresh.state_mut().apply_view_prefs("railnames=1");
    fresh.run();
    assert!(fresh.state().rail_names);
    assert_eq!(rail_edge(&fresh), NAMES);
    let saved = fresh.state().view_prefs();
    fresh.state_mut().apply_view_prefs("railnames=0");
    fresh.run();
    assert_eq!(rail_edge(&fresh), SLIM);
    fresh.state_mut().apply_view_prefs(&saved);
    fresh.run();
    assert_eq!(rail_edge(&fresh), NAMES);
    // Preferences saved before there was a toggle: the slim rail.
    let Some(mut old) = harness() else { return };
    old.state_mut().apply_view_prefs("grid=1;axes=1");
    old.run();
    assert!(!old.state().rail_names);
}

/// On a sheet the rail holds what can be done on paper, as tiles or as rows with names, each
/// by the same name.
#[test]
fn smoke_rail_on_a_sheet_in_both_forms() {
    let Some(mut h) = harness() else { return };
    h.state_mut().new_sheet();
    h.run();
    h.run();
    assert!(h.state().drawing.view.sheet_preview, "the sheet is shown");
    let places = ["Select", "Note", "Dimension", "Add view", "Back to model"];
    let in_view = |h: &Harness<'_, DriftApp>, what: &str| -> Vec<egui::Rect> {
        let canvas = h.state().drawing.view.canvas_rect;
        let rects: Vec<egui::Rect> = places
            .iter()
            .map(|l| on_rail(h, l).unwrap_or_else(|| panic!("{what}: no {l} on the rail")))
            .collect();
        assert!(
            rects.is_sorted_by(|a, b| a.bottom() <= b.top() + 0.5),
            "{what}: top to bottom: {rects:?}"
        );
        assert!(
            rects
                .iter()
                .all(|r| r.right() <= canvas.left() + 0.5 && r.bottom() <= canvas.bottom()),
            "{what}: inside the rail: {rects:?}"
        );
        rects
    };
    assert_eq!(rail_edge(&h), SLIM);
    let tiles = in_view(&h, "slim");
    assert!(tiles.iter().all(|r| r.width() == 36.0), "tiles: {tiles:?}");
    save_png("drift_smoke_rail_sheet.png", &mut h);
    toggle_names(&mut h);
    assert_eq!(rail_edge(&h), NAMES);
    let rows = in_view(&h, "names");
    assert!(rows.iter().all(|r| r.width() > 140.0), "rows: {rows:?}");
    save_png("drift_smoke_rail_sheet_names.png", &mut h);
    // A row acts as its tile does: Note is the tool, and back in the model the rail is the
    // model's, still with names.
    let note = rows[1];
    h.get_all_by_label("Note")
        .find(|n| n.rect() == note)
        .unwrap()
        .click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Note);
    let back = on_rail(&h, "Back to model").unwrap();
    h.get_all_by_label("Back to model")
        .find(|n| n.rect() == back)
        .unwrap()
        .click();
    h.run();
    assert!(!h.state().drawing.view.sheet_preview);
    assert!(h.state().rail_names);
    rail_in_view(&h, "back in the model, names");
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
    // To the right of the rail, level with the tile.
    let flyout = flyout_rect(&h);
    assert!(
        flyout.left() >= rail_edge(&h) && flyout.left() < rail_edge(&h) + 12.0,
        "the flyout opens beside the rail: {flyout:?}"
    );
    assert!(
        flyout.top() <= trim.top() && flyout.bottom() >= trim.bottom(),
        "level with its tile: {flyout:?} / {trim:?}"
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
    let flyout: Vec<f32> = ["3-point arc", "Tangent arc", "Center arc"]
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
    assert!(on_rail(&h, "Center arc").is_some(), "the family shows it");

    // With names shown the flyout still opens to the right of the rail, clear of the rows.
    toggle_names(&mut h);
    let trim = on_rail(&h, "Trim").expect("Trim on the rail, with names");
    h.get_all_by_label("Trim")
        .find(|n| n.rect() == trim)
        .unwrap()
        .click_secondary();
    h.run();
    let flyout = flyout_rect(&h);
    assert!(
        flyout.left() >= rail_edge(&h) && flyout.left() >= trim.right(),
        "the flyout opens beside the rail: {flyout:?}"
    );
    let extend = h
        .get_all_by_label("Extend")
        .map(|n| n.rect())
        .find(|r| r.left() > trim.right())
        .expect("Extend in the flyout");
    assert!(flyout.contains_rect(extend));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().rail_flyout.is_none());
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
    // Resting on the tile shows its tooltip: to the right of the rail, level with the tile,
    // over none of the rail's other places.
    h.hover_at(rect.center());
    rest(&mut h);
    let tips = tooltips(&h);
    assert_eq!(tips.len(), 1, "the tile has a tooltip: {tips:?}");
    assert!(
        tips[0].left() >= rail_edge(&h) && (tips[0].top() - rect.top()).abs() < 12.0,
        "the tooltip is beside its tile: {:?} / {rect:?}",
        tips[0]
    );
    // It says what else is in the tile's family.
    assert_eq!(
        h.query_all_by_label_contains("Right-click or hold for")
            .count(),
        1
    );
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
    let ed = &mut h.state_mut().drawing.editor;
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
