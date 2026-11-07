//! The layer of what is picked (drift-releases#19): the active element's; else the one
//! layer everything picked is on; an end point counts as on its curve's layer.

use drift_core::{DVec2, Layer, LayerId};

use super::*;

#[test]
fn the_picked_layer_is_the_active_ones_or_the_only_one() {
    let mut doc = Document::new();
    let a = doc.add_layer(Layer::new("A"));
    let b = doc.add_layer(Layer::new("B"));
    let p = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)]
        .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
    let on_a = doc.add_line(p[0], p[1]).unwrap();
    let on_b = doc.add_line(p[2], p[3]).unwrap();
    doc.set_entity_layer(on_a, a).unwrap();
    doc.set_entity_layer(on_b, b).unwrap();
    let mut ed = Editor::new(doc);
    ed.sync();
    // Both picked, the line on A last: A, and the pick is on 2 layers.
    ed.selection = [on_a, on_b].into_iter().collect();
    ed.set_active(Some(on_a));
    assert_eq!(
        ed.picked_layer(),
        PickedLayer {
            layer: Some(a),
            layers: 2
        }
    );
    // The same with none active (a box pick): no one layer.
    ed.set_active(None);
    assert_eq!(ed.picked_layer().layer, None);
    // Only the line on B, by a box: B.
    ed.selection = [on_b].into_iter().collect();
    assert_eq!(ed.picked_layer().layer, Some(b));
    // An end point (on layer 0) counts as on its line's layer.
    assert_eq!(ed.doc.get(p[0]).unwrap().layer, LayerId::DEFAULT);
    ed.selection = [p[0]].into_iter().collect();
    ed.set_active(Some(p[0]));
    assert_eq!(ed.picked_layer().layer, Some(a));
    // Nothing picked.
    ed.selection.clear();
    ed.set_active(None);
    assert_eq!(ed.picked_layer(), PickedLayer::default());
}
