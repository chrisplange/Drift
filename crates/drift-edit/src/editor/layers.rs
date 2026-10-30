//! Moving what is picked to another layer: curves with their points, the fills of picked
//! shapes, picked fills, copies of blocks, texts, walls and openings, in one step (#266).

use std::collections::BTreeSet;

use drift_core::{
    DocumentError, EntityId, FillId, InstanceId, LayerId, OpeningId, Owner, TextId, WallId,
};

use super::Editor;

/// Filled shapes picked as objects: their fills, and the curves of their outlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PickedShapes {
    pub fills: Vec<FillId>,
    pub curves: Vec<EntityId>,
}

/// What a Move to layer takes: entities (a curve's points go with it), fills, copies of
/// blocks and texts. The fills of shapes whose whole outline is among the entities go too,
/// and so do walls and openings all of whose curves are.
#[derive(Clone, Debug, Default)]
pub struct LayerMove {
    pub entities: BTreeSet<EntityId>,
    pub fills: BTreeSet<FillId>,
    pub instances: BTreeSet<InstanceId>,
    pub texts: BTreeSet<TextId>,
}

impl Editor {
    /// The filled shapes in the selection: picked fills of shapes, and shapes whose whole
    /// outline is picked (a group of them, say).
    pub(crate) fn shapes_in_selection(&self) -> PickedShapes {
        let mut fills = BTreeSet::new();
        let mut curves = BTreeSet::new();
        for (id, _) in self.doc.fills() {
            let Some(outline) = self.shape_outline(id) else {
                continue;
            };
            let picked = self.selected_fills.contains(&id)
                || (!outline.is_empty() && outline.iter().all(|c| self.selection.contains(c)));
            if picked {
                fills.insert(id);
                curves.extend(outline);
            }
        }
        PickedShapes {
            fills: fills.into_iter().collect(),
            curves: curves.into_iter().collect(),
        }
    }

    /// What Move to layer takes from the selection: the picked curves, fills, copies and
    /// texts. (A fill picked on its own goes alone: its outline stays where it is.)
    pub fn selection_layer_move(&self) -> LayerMove {
        LayerMove {
            entities: self.selection.clone(),
            fills: self.selected_fills.clone(),
            instances: self.selected_instances.clone(),
            texts: self.selected_texts.clone(),
        }
    }

    /// Put `what` on `layer`, inside the open transaction: the entities with their points,
    /// the fills listed and those of shapes whose whole outline is among the entities, the
    /// copies and texts, and the walls and openings all of whose curves are among the
    /// entities (their faces, symbols and poché follow them when they are made again).
    pub(crate) fn put_on_layer(
        &mut self,
        layer: LayerId,
        what: &LayerMove,
    ) -> Result<(), DocumentError> {
        let picked = &what.entities;
        let mut fills = what.fills.clone();
        for (id, _) in self.doc.fills() {
            if let Some(outline) = self.shape_outline(id)
                && !outline.is_empty()
                && outline.iter().all(|c| picked.contains(c))
            {
                fills.insert(id);
            }
        }
        let walls = self.walls_among(picked);
        let openings = self.openings_among(picked);

        let mut ids: BTreeSet<EntityId> = self.doc.defining_points(picked.iter().copied());
        ids.extend(picked.iter().copied());
        for id in ids {
            self.doc.set_entity_layer(id, layer)?;
        }
        for f in fills {
            if let Some(mut fill) = self.doc.fill(f).cloned() {
                fill.layer = layer;
                self.doc.set_fill(f, fill);
            }
        }
        for i in &what.instances {
            if let Some(mut copy) = self.doc.instance(*i).copied() {
                copy.layer = layer;
                self.doc.set_instance(*i, copy)?;
            }
        }
        for t in &what.texts {
            if let Some(mut text) = self.doc.text(*t).cloned() {
                text.layer = layer;
                self.doc.set_text(*t, text);
            }
        }
        for w in &walls {
            if let Some(mut wall) = self.doc.wall(*w).cloned() {
                wall.layer = layer;
                self.doc.set_wall(*w, wall)?;
            }
        }
        for o in openings {
            if let Some(mut opening) = self.doc.opening(o).cloned() {
                // On its wall's layer, and the wall moves too: it follows the wall.
                if opening.layer.is_none() && walls.contains(&opening.wall) {
                    continue;
                }
                opening.layer = Some(layer);
                self.doc.set_opening(o, opening)?;
            }
        }
        Ok(())
    }

    /// Move what is picked to a layer, one undo step (the right-click menu, the context
    /// bar, the Layers panel and the shape inspector all do it this way).
    pub fn move_selection_to_layer(&mut self, layer: LayerId) {
        let what = self.selection_layer_move();
        self.doc.begin();
        if self.put_on_layer(layer, &what).is_err() {
            self.doc.rollback();
            self.sync();
            return;
        }
        self.commit("Move to layer");
    }

    /// The walls all of whose centreline curves are among `ids`.
    fn walls_among(&self, ids: &BTreeSet<EntityId>) -> BTreeSet<WallId> {
        if !self.doc.has_walls() {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let candidates: BTreeSet<WallId> = ids
            .iter()
            .filter_map(|id| match owners.get(*id) {
                Some(Owner::Centre(w)) => Some(w),
                _ => None,
            })
            .collect();
        candidates
            .into_iter()
            .filter(|w| {
                let curves = self.wall_curves(*w);
                !curves.is_empty() && curves.iter().all(|c| ids.contains(c))
            })
            .collect()
    }

    /// The openings whose symbol and jambs are all among `ids`.
    fn openings_among(&self, ids: &BTreeSet<EntityId>) -> BTreeSet<OpeningId> {
        if !self.doc.has_openings() {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let candidates: BTreeSet<OpeningId> =
            ids.iter().filter_map(|id| owners.opening(*id)).collect();
        candidates
            .into_iter()
            .filter(|o| {
                let set = self.opening_pick_set(*o);
                !set.is_empty() && set.iter().all(|e| ids.contains(e))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{Block, DVec2, Instance, SketchText};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A closed square of lines from `at`, `size` across, as a filled shape: its sides, its
    /// corners and its fill.
    fn filled_square(
        ed: &mut Editor,
        at: DVec2,
        size: f64,
    ) -> (Vec<EntityId>, Vec<EntityId>, FillId) {
        let corners = [at, at + v(size, 0.0), at + v(size, size), at + v(0.0, size)];
        ed.fill_new_shapes = true;
        ed.doc.begin();
        let p: Vec<EntityId> = corners.iter().map(|c| ed.doc.add_point(*c)).collect();
        let sides: Vec<EntityId> = (0..4)
            .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        let fill = ed.fill_new_shape(&sides).unwrap();
        ed.commit("Square");
        (sides, p, fill)
    }

    fn layer_of(ed: &Editor, id: EntityId) -> LayerId {
        ed.doc.get(id).unwrap().layer
    }

    /// Move to layer takes everything picked to the layer, a shape's fill with its outline,
    /// in one undo step (#266).
    #[test]
    fn move_to_layer_takes_shapes_fills_copies_and_texts() {
        let mut ed = Editor::default();
        let a = ed.doc.current_layer();
        let (sides, corners, fill) = filled_square(&mut ed, v(0.0, 0.0), 10.0);
        // A filled shape in a group: picking a member picks the group.
        let (g_sides, _, g_fill) = filled_square(&mut ed, v(20.0, 0.0), 10.0);
        ed.selection = g_sides.iter().copied().collect();
        ed.make_group().unwrap();
        ed.doc.begin();
        let block = ed.doc.add_block(Block {
            name: "B1".into(),
            items: Vec::new(),
            fills: Vec::new(),
            profile: false,
        });
        let copy = ed
            .doc
            .add_instance(Instance::new(block, v(50.0, 0.0)))
            .unwrap();
        let text = ed.doc.add_text(SketchText::new("NOTE", v(0.0, 30.0), 5.0));
        ed.commit("Copy and text");
        let b = ed.add_layer("B");
        ed.selection = [sides[0], g_sides[0]].into();
        ed.expand_groups();
        ed.selection.extend(sides.iter().copied());
        ed.selected_instances = [copy].into();
        ed.selected_texts = [text].into();

        ed.move_selection_to_layer(b);
        for id in sides.iter().chain(&corners).chain(&g_sides) {
            assert_eq!(layer_of(&ed, *id), b, "{id}");
        }
        assert_eq!(ed.doc.fill(fill).unwrap().layer, b, "the shape's fill");
        assert_eq!(
            ed.doc.fill(g_fill).unwrap().layer,
            b,
            "the grouped shape's fill"
        );
        assert_eq!(ed.doc.instance(copy).unwrap().layer, b);
        assert_eq!(ed.doc.text(text).unwrap().layer, b);

        assert!(ed.undo());
        for id in sides.iter().chain(&corners).chain(&g_sides) {
            assert_eq!(layer_of(&ed, *id), a, "{id}");
        }
        assert_eq!(ed.doc.fill(fill).unwrap().layer, a);
        assert_eq!(ed.doc.fill(g_fill).unwrap().layer, a);
        assert_eq!(ed.doc.instance(copy).unwrap().layer, a);
        assert_eq!(ed.doc.text(text).unwrap().layer, a);
    }

    /// A fill picked on its own goes alone: its outline stays on its layer.
    #[test]
    fn a_fill_picked_alone_moves_alone() {
        let mut ed = Editor::default();
        let a = ed.doc.current_layer();
        let (sides, _, fill) = filled_square(&mut ed, v(0.0, 0.0), 10.0);
        let b = ed.add_layer("B");
        ed.pick_fill_alone(fill);
        ed.move_selection_to_layer(b);
        assert_eq!(ed.doc.fill(fill).unwrap().layer, b);
        assert!(sides.iter().all(|s| layer_of(&ed, *s) == a));
    }

    /// A wall picked whole moves its record: its faces, poché and openings follow it when
    /// they are made again, instead of going back (#266).
    #[test]
    fn a_picked_wall_moves_with_its_faces_poche_and_openings() {
        let mut ed = Editor::default();
        let a = ed.doc.current_layer();
        ed.doc.begin();
        let p0 = ed.doc.add_point(v(0.0, 0.0));
        let p1 = ed.doc.add_point(v(4000.0, 0.0));
        let line = ed.doc.add_line(p0, p1).unwrap();
        ed.commit("Line");
        let w = ed.walls_from_curves(&[line]).unwrap()[0];
        ed.opening.family = "door.single".into();
        ed.opening.width = 900.0;
        ed.opening.leaf = 800.0;
        let o = ed.place_opening(w, 0, 1500.0).expect("a door");
        let b = ed.add_layer("B");
        ed.selection = ed.wall_pick_set(w);
        ed.selection.extend(ed.opening_pick_set(o));

        ed.move_selection_to_layer(b);
        let wall = ed.doc.wall(w).unwrap().clone();
        assert_eq!(wall.layer, b);
        assert!(!wall.made.curves.is_empty());
        for id in wall.made.curves.values() {
            assert_eq!(layer_of(&ed, *id), b, "face {id}");
        }
        for f in wall.made.fills.values() {
            assert_eq!(ed.doc.fill(*f).unwrap().layer, b, "poché {f}");
        }
        // On its wall's layer, it follows the wall.
        let door = ed.doc.opening(o).unwrap().clone();
        assert_eq!(door.layer, None);
        assert!(!door.made.curves.is_empty());
        for id in door.made.curves.values() {
            assert_eq!(layer_of(&ed, *id), b, "symbol {id}");
        }

        assert!(ed.undo());
        let wall = ed.doc.wall(w).unwrap();
        assert_eq!(wall.layer, a);
        assert!(wall.made.curves.values().all(|id| layer_of(&ed, *id) == a));

        // The door alone goes to a layer of its own.
        ed.selection = ed.opening_pick_set(o);
        ed.move_selection_to_layer(b);
        assert_eq!(ed.doc.opening(o).unwrap().layer, Some(b));
        assert_eq!(ed.doc.wall(w).unwrap().layer, a);
        let door = ed.doc.opening(o).unwrap();
        assert!(door.made.curves.values().all(|id| layer_of(&ed, *id) == b));
    }
}
