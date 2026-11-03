//! The Inspector's first row, under the object's name, is its Layer (#300): for a line, a
//! filled shape, an area fill, a block copy, a wall, a text and a mixed pick, in the same
//! place each time, and picking a layer there moves all of it in one undo step.

use drift_core::{EntityId, LayerId, SketchText};

use egui_kittest::kittest::NodeT as _;

use super::*;

/// The layer the tests move things to.
const TO: &str = "Moved here";

/// The app with the Inspector shown, a layer named [`TO`], the drawing from `min` to `max`.
fn inspector_harness(min: DVec2, max: DVec2) -> Option<(Harness<'static, DriftApp>, LayerId)> {
    let mut h = harness()?;
    h.state_mut().grid_snap = false;
    let start = h.state().drawing.editor.doc.current_layer();
    let to = h.state_mut().drawing.editor.add_layer(TO);
    h.state_mut().drawing.editor.set_current_layer(start);
    h.state_mut().show_panel(crate::dock::Panel::Inspector);
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds { min, max });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    Some((h, to))
}

/// A closed rectangle of lines from `min` to `max`, its sides in order round it.
fn rectangle(h: &mut Harness<'_, DriftApp>, min: DVec2, max: DVec2) -> Vec<EntityId> {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let p =
        [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)].map(|q| ed.doc.add_point(q));
    let sides = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.commit("Rectangle");
    sides
}

/// The Inspector's Layer row: the "Layer" label in the column of the row labelled `next`
/// (a row the Inspector shows for this kind; a label elsewhere, the context bar's say, is
/// in another column). Asserts there is exactly one, above `next`: the first row.
fn layer_row_above(h: &Harness<'_, DriftApp>, next: &str) -> egui::Rect {
    let next = h
        .query_all_by_label(next)
        .map(|n| n.rect())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no {next} row"));
    let rows: Vec<egui::Rect> = h
        .query_all_by_label("Layer")
        .map(|n| n.rect())
        .filter(|r| (r.left() - next.left()).abs() < 2.0)
        .collect();
    assert_eq!(rows.len(), 1, "one Layer row in the Inspector: {rows:?}");
    assert!(
        rows[0].bottom() <= next.top(),
        "the Layer row comes first, above {next:?}: {:?}",
        rows[0]
    );
    rows[0]
}

/// Pick [`TO`] in the Layer row at `row` (its label), then check `on` says everything moved
/// there, and that one undo puts it all back on `from`.
fn move_by_the_row(
    h: &mut Harness<'_, DriftApp>,
    row: egui::Rect,
    to: LayerId,
    on: impl Fn(&Harness<'_, DriftApp>) -> Vec<LayerId>,
) {
    let from = on(h);
    assert!(from.iter().all(|l| *l != to), "{from:?}");
    let combo = layer_combo(h, row).rect().center();
    for pressed in [true, false] {
        button(h, combo, pressed);
        h.run();
    }
    // The layer's name in the menu: the one nearest the combo (the Layers panel lists it too).
    let item = h
        .query_all_by_label(TO)
        .map(|n| n.rect().center())
        .filter(|p| p.y > row.top())
        .min_by(|a, b| a.distance(combo).total_cmp(&b.distance(combo)))
        .expect("the layer in the menu");
    for pressed in [true, false] {
        button(h, item, pressed);
        h.run();
    }
    assert!(on(h).iter().all(|l| *l == to), "moved: {:?}", on(h));
    assert!(h.state_mut().drawing.editor.undo());
    h.run();
    assert_eq!(on(h), from, "one undo puts it all back");
}

/// The combo box on the Layer row whose label is at `row`.
fn layer_combo<'a>(h: &'a Harness<'_, DriftApp>, row: egui::Rect) -> egui_kittest::Node<'a> {
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox))
        .find(|n| {
            (n.rect().center().y - row.center().y).abs() < 6.0 && n.rect().left() > row.right()
        })
        .expect("the Layer row's combo box")
}

fn entity_layers(h: &Harness<'_, DriftApp>, ids: &[EntityId]) -> Vec<LayerId> {
    let doc = &h.state().drawing.editor.doc;
    ids.iter().map(|id| doc.get(*id).unwrap().layer).collect()
}

#[test]
fn a_line_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::ZERO);
    let b = ed.doc.add_point(DVec2::new(50.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    ed.selection = [line].into();
    h.run();
    let row = layer_row_above(&h, "Length");
    move_by_the_row(&mut h, row, to, |h| entity_layers(h, &[line]));
}

#[test]
fn several_curves_on_two_layers_show_mixed() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let sides = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    let ed = &mut h.state_mut().drawing.editor;
    let other = ed.add_layer("Other");
    ed.selection = [sides[0]].into();
    ed.move_selection_to_layer(other);
    ed.selection = sides.iter().copied().collect();
    h.run();
    // Several curves have no Geometry rows: Appearance follows, its kind switch first.
    let row = layer_row_above(&h, "Colour");
    let switch = h
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Geometry")
        .map(|n| n.rect())
        .find(|r| (r.left() - row.left()).abs() < 4.0)
        .expect("Appearance's Geometry / Construction switch");
    assert!(
        row.bottom() <= switch.top(),
        "above Appearance: {row:?} {switch:?}"
    );
    assert_eq!(
        layer_combo(&h, row).accesskit_node().value().as_deref(),
        Some("Mixed"),
        "two layers"
    );
    move_by_the_row(&mut h, row, to, |h| entity_layers(h, &sides));
}

#[test]
fn a_filled_shape_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let sides = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    let ed = &mut h.state_mut().drawing.editor;
    ed.prefs.fill_new_shapes = true;
    ed.doc.begin();
    ed.add_shape(&sides).expect("a shape");
    ed.commit("Shape");
    let fill = ed.doc.fills().next().map(|(id, _)| id).unwrap();
    ed.selection = sides.iter().copied().collect();
    h.run();
    let row = layer_row_above(&h, "Colour");
    move_by_the_row(&mut h, row, to, |h| {
        let mut on = entity_layers(h, &sides);
        on.push(h.state().drawing.editor.doc.fill(fill).unwrap().layer);
        on
    });
}

#[test]
fn an_area_fill_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(110.0, 50.0))
    else {
        return;
    };
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    rectangle(&mut h, DVec2::new(20.0, 15.0), DVec2::new(60.0, 45.0));
    let ed = &mut h.state_mut().drawing.editor;
    let fill = ed
        .add_fill_at(DVec2::new(30.0, 20.0))
        .expect("an area fill");
    ed.selection.clear();
    ed.selected_fills = [fill].into();
    h.run();
    // (A hatch, the default: its first row is the Pattern.)
    let row = layer_row_above(&h, "Pattern");
    move_by_the_row(&mut h, row, to, |h| {
        vec![h.state().drawing.editor.doc.fill(fill).unwrap().layer]
    });
}

#[test]
fn a_block_copy_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let sides = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    let ed = &mut h.state_mut().drawing.editor;
    ed.selection = sides.into_iter().collect();
    let copy = ed.make_block("Box").unwrap();
    ed.selection.clear();
    ed.selected_instances = [copy].into();
    h.run();
    let row = layer_row_above(&h, "Name");
    move_by_the_row(&mut h, row, to, |h| {
        vec![h.state().drawing.editor.doc.instance(copy).unwrap().layer]
    });
}

#[test]
fn a_wall_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-20.0, -40.0), DVec2::new(140.0, 100.0))
    else {
        return;
    };
    h.state_mut().drawing.editor.prefs.wall_thickness = 10.0;
    h.key_press(Key::W);
    h.run();
    click_world(&mut h, DVec2::new(0.0, 0.0));
    click_world(&mut h, DVec2::new(100.0, 0.0));
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(50.0, 2.0));
    let walls = h.state().drawing.editor.selected_walls();
    assert_eq!(walls.len(), 1);
    let wall = *walls.first().unwrap();
    let row = layer_row_above(&h, "Thickness");
    move_by_the_row(&mut h, row, to, |h| {
        let ed = &h.state().drawing.editor;
        let mut on = vec![ed.doc.wall(wall).unwrap().layer];
        on.extend(entity_layers(h, &ed.wall_curves(wall)));
        on
    });
}

#[test]
fn a_text_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let ed = &mut h.state_mut().drawing.editor;
    let text = ed.add_text(SketchText::new("NOTE", DVec2::ZERO, 5.0));
    ed.selection.clear();
    ed.selected_texts = [text].into();
    h.run();
    // Texts had no Layer row at all.
    let row = layer_row_above(&h, "Height");
    move_by_the_row(&mut h, row, to, |h| {
        vec![h.state().drawing.editor.doc.text(text).unwrap().layer]
    });
}

#[test]
fn a_mixed_pick_with_a_text_shows_its_layer_first() {
    let Some((mut h, to)) = inspector_harness(DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0))
    else {
        return;
    };
    let sides = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0));
    let ed = &mut h.state_mut().drawing.editor;
    let text = ed.add_text(SketchText::new("NOTE", DVec2::new(0.0, 35.0), 5.0));
    ed.selection = [sides[0]].into();
    ed.selected_texts = [text].into();
    h.run();
    assert!(
        h.query_by_label("1 line · 1 text").is_some(),
        "the mixed Inspector"
    );
    let hint = h
        .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::Label))
        .find(|n| {
            n.accesskit_node()
                .value()
                .is_some_and(|v| v.starts_with("Delete, Move (G)"))
        })
        .expect("the mixed Inspector's hint")
        .rect();
    let row = h
        .query_all_by_label("Layer")
        .map(|n| n.rect())
        .find(|r| (r.left() - hint.left()).abs() < 2.0)
        .expect("a Layer row");
    assert!(row.bottom() <= hint.top(), "the Layer row first");
    move_by_the_row(&mut h, row, to, |h| {
        let mut on = entity_layers(h, &[sides[0]]);
        on.push(h.state().drawing.editor.doc.text(text).unwrap().layer);
        on
    });
}
