//! The points of curves on a hidden layer are hidden with them, though the points are on a
//! shown layer (layer 0, as an imported PDF leaves them, #359): not drawn, not picked, not
//! taken by Select all, and not kept picked, so Delete can't take the hidden curves along.

use drift_core::{DVec2, Layer, LayerId};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Two lines on layer "Walls" sharing an end, a third on layer 0 from one of their ends,
/// every point on layer 0: the editor, the walls layer, the points, the lines.
fn drawing() -> (Editor, LayerId, [EntityId; 4], [EntityId; 3]) {
    let mut doc = Document::new();
    let walls = doc.add_layer(Layer::new("Walls"));
    let p =
        [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(200.0, 100.0)].map(|q| doc.add_point(q));
    let lines = [
        doc.add_line(p[0], p[1]).unwrap(),
        doc.add_line(p[1], p[2]).unwrap(),
        doc.add_line(p[2], p[3]).unwrap(),
    ];
    doc.set_entity_layer(lines[0], walls).unwrap();
    doc.set_entity_layer(lines[1], walls).unwrap();
    let mut ed = Editor::new(doc);
    ed.prefs.always_endpoints = true;
    ed.sync();
    (ed, walls, p, lines)
}

fn drawn_points(ed: &Editor) -> Vec<EntityId> {
    ed.items_tagged(false, false)
        .into_iter()
        .filter_map(|t| t.entity)
        .filter(|id| ed.doc.get(*id).is_some_and(|e| e.kind.is_point()))
        .collect()
}

#[test]
fn points_of_hidden_curves_are_not_drawn_picked_or_selected() {
    let (mut ed, walls, p, _) = drawing();
    let mut shown = drawn_points(&ed);
    shown.sort_unstable();
    assert_eq!(shown, p.to_vec(), "all four, end points on");
    ed.set_layers_visible(&[(walls, false)], "Hide layer");
    ed.sync();
    // p0 and p1 only end hidden lines; p2 also ends the line on layer 0.
    let mut shown = drawn_points(&ed);
    shown.sort_unstable();
    assert_eq!(shown, vec![p[2], p[3]]);
    assert_eq!(ed.index.pick(v(0.0, 0.0), 2.0, |_| true), None);
    assert_eq!(ed.index.pick(v(100.0, 0.0), 2.0, |_| true), None);
    assert_eq!(ed.index.pick(v(100.0, 100.0), 2.0, |_| true), Some(p[2]));
    ed.select_all();
    assert!(
        !ed.selection.contains(&p[0]) && !ed.selection.contains(&p[1]),
        "{:?}",
        ed.selection
    );
}

/// A picked end point whose curves' layer is then hidden is no longer picked: Delete leaves
/// the hidden curves alone (they'd go with their point).
#[test]
fn a_picked_point_of_curves_then_hidden_is_dropped() {
    let (mut ed, walls, p, lines) = drawing();
    ed.selection = [p[0]].into_iter().collect();
    ed.set_layers_visible(&[(walls, false)], "Hide layer");
    ed.sync();
    assert!(ed.selection.is_empty(), "{:?}", ed.selection);
    ed.delete_selection();
    assert!(ed.doc.contains(lines[0]) && ed.doc.contains(lines[1]));
    assert!(ed.doc.contains(p[0]));
}
