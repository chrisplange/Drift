//! Moving what is picked to another layer: curves with their points, the fills of picked
//! shapes, picked fills, copies of blocks, texts, walls and openings, in one step (#266).

use std::collections::BTreeSet;

use drift_core::{
    Appearance, DocumentError, EntityId, FillId, InstanceId, Layer, LayerId, OpeningId, Owner,
    TextId, WallId,
};

use super::Editor;

/// Filled shapes picked as objects: their fills, and the curves of their outlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickedShapes {
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
    pub fn shapes_in_selection(&self) -> PickedShapes {
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
    pub fn put_on_layer(&mut self, layer: LayerId, what: &LayerMove) -> Result<(), DocumentError> {
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

/// An on/off setting on a layer row (other than the eye, whose Alt-click isolates) that
/// Alt-click sets the other way on every other layer, as in Illustrator; Alt-click again puts
/// them back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerFlag {
    Locked,
    Snappable,
}

impl LayerFlag {
    pub fn get(self, layer: &Layer) -> bool {
        match self {
            LayerFlag::Locked => layer.locked,
            LayerFlag::Snappable => layer.snappable,
        }
    }

    pub fn set(self, layer: &mut Layer, on: bool) {
        match self {
            LayerFlag::Locked => layer.locked = on,
            LayerFlag::Snappable => layer.snappable = on,
        }
    }

    /// What the Alt-clicked layer gets: unlocked, snapped to (the others the opposite).
    pub fn alone(self) -> bool {
        match self {
            LayerFlag::Locked => false,
            LayerFlag::Snappable => true,
        }
    }

    /// The undo step's name for an Alt-click, and for the one that puts the layers back.
    pub fn labels(self) -> (&'static str, &'static str) {
        match self {
            LayerFlag::Locked => ("Lock other layers", "Put layer locks back"),
            LayerFlag::Snappable => ("Snap to one layer", "Put layer snapping back"),
        }
    }
}

impl Editor {
    pub fn add_layer(&mut self, name: &str) -> LayerId {
        self.doc.begin();
        let id = self.doc.add_layer(Layer::new(name.trim()));
        self.commit("Add layer");
        id
    }

    /// Change a layer's settings (name, colour, weight, line type, visibility, locking).
    pub fn update_layer(&mut self, id: LayerId, layer: Layer) {
        if self.doc.layer(id) == Some(&layer) {
            return;
        }
        self.doc.begin();
        self.doc.set_layer(id, layer);
        self.commit("Change layer");
    }

    /// Show or hide layers (the rest stay as they are), one undo step; nothing when none
    /// change.
    pub fn set_layers_visible(&mut self, visible: &[(LayerId, bool)], label: &str) {
        let changed: Vec<(LayerId, Layer)> = visible
            .iter()
            .filter_map(|&(id, on)| {
                let layer = self.doc.layer(id)?;
                (layer.visible != on).then(|| {
                    (
                        id,
                        Layer {
                            visible: on,
                            ..layer.clone()
                        },
                    )
                })
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        self.doc.begin();
        for (id, layer) in changed {
            self.doc.set_layer(id, layer);
        }
        self.commit(label);
    }

    /// Set a setting on layers (the rest stay as they are), one undo step; nothing when none
    /// change.
    pub fn set_layers_flag(&mut self, flag: LayerFlag, values: &[(LayerId, bool)], label: &str) {
        let changed: Vec<(LayerId, Layer)> = values
            .iter()
            .filter_map(|&(id, on)| {
                let mut layer = self.doc.layer(id)?.clone();
                (flag.get(&layer) != on).then(|| {
                    flag.set(&mut layer, on);
                    (id, layer)
                })
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        self.doc.begin();
        for (id, layer) in changed {
            self.doc.set_layer(id, layer);
        }
        self.commit(label);
    }

    /// Set a setting on every layer but `id` the other way from `id` (lock every other
    /// layer, or snap to `id` alone), as one undo step. Locking the others makes `id`
    /// current, so drawing goes on there. Returns how each layer was, to put back with
    /// [`Editor::set_layers_flag`].
    pub fn solo_layer_flag(&mut self, flag: LayerFlag, id: LayerId) -> Vec<(LayerId, bool)> {
        let before: Vec<(LayerId, bool)> = self
            .doc
            .layers()
            .map(|(l, layer)| (l, flag.get(layer)))
            .collect();
        let alone = flag.alone();
        self.doc.begin();
        for (l, was) in &before {
            let on = if *l == id { alone } else { !alone };
            if *was != on
                && let Some(mut layer) = self.doc.layer(*l).cloned()
            {
                flag.set(&mut layer, on);
                self.doc.set_layer(*l, layer);
            }
        }
        if flag == LayerFlag::Locked {
            let _ = self.doc.set_current_layer(id);
        }
        self.commit(flag.labels().0);
        before
    }

    /// Show `id` alone, made current, hiding every other layer: one undo step. Returns how
    /// each layer was, to put back with [`Editor::set_layers_visible`].
    pub fn isolate_layer(&mut self, id: LayerId) -> Vec<(LayerId, bool)> {
        let before: Vec<(LayerId, bool)> = self
            .doc
            .layers()
            .map(|(l, layer)| (l, layer.visible))
            .collect();
        self.doc.begin();
        for (l, _) in &before {
            if let Some(layer) = self.doc.layer(*l) {
                let visible = *l == id;
                if layer.visible != visible {
                    let layer = Layer {
                        visible,
                        ..layer.clone()
                    };
                    self.doc.set_layer(*l, layer);
                }
            }
        }
        let _ = self.doc.set_current_layer(id);
        self.commit("Isolate layer");
        before
    }

    /// Delete a layer; its geometry moves to layer "0".
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), String> {
        self.doc.begin();
        if let Err(e) = self.doc.remove_layer(id) {
            self.doc.rollback();
            return Err(e.to_string());
        }
        self.commit("Delete layer");
        Ok(())
    }

    /// Move a layer to `index` in the draw order (0 at the bottom), one undo step.
    pub fn reorder_layer(&mut self, id: LayerId, index: usize) {
        self.doc.begin();
        self.doc.reorder_layer(id, index);
        self.commit("Reorder layers");
    }

    pub fn set_current_layer(&mut self, id: LayerId) {
        self.doc.begin();
        let _ = self.doc.set_current_layer(id);
        self.commit("Set current layer");
    }

    /// Select every curve and standalone point on a layer.
    pub fn select_layer(&mut self, layer: LayerId) {
        let referenced = self.doc.referenced_points();
        self.selection = self
            .doc
            .iter()
            .filter(|(id, e)| e.layer == layer && !referenced.contains(id))
            .map(|(id, _)| id)
            .collect();
    }

    /// Change the appearance overrides of the selected curves with `f`.
    pub fn update_selection_appearance(&mut self, f: impl Fn(&mut Appearance)) {
        let ids: Vec<EntityId> = self.selection.iter().copied().collect();
        self.update_appearance(&ids, f);
    }

    /// Change the appearance overrides of curves with `f` (one undo step).
    pub fn update_appearance(&mut self, ids: &[EntityId], f: impl Fn(&mut Appearance)) {
        self.doc.begin();
        for &id in ids {
            if let Some(mut a) = self.doc.get(id).map(|e| e.appearance) {
                f(&mut a);
                let _ = self.doc.set_appearance(id, a);
            }
        }
        self.commit("Change appearance");
    }
}

/// The layer edits (moved from drift-app's layers_ui.rs with them).
#[cfg(test)]
mod layer_edit_tests {
    use drift_core::{Constraint, DVec2, EntityKind, LineKind, LineType, LineWeight, Rgb, Shape};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Two separate lines; the first on a new layer. Returns (editor, layer, lines).
    fn two_layers() -> (Editor, LayerId, [EntityId; 2]) {
        let mut ed = Editor::default();
        let layer = ed.add_layer("Frame");
        ed.set_current_layer(layer);
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 0.0));
        let l1 = ed.doc.add_line(a, b).unwrap();
        ed.doc.set_current_layer(LayerId::DEFAULT).unwrap();
        let c = ed.doc.add_point(v(0.0, 5.0));
        let d = ed.doc.add_point(v(10.0, 7.0));
        let l2 = ed.doc.add_line(c, d).unwrap();
        ed.commit("Lines");
        (ed, layer, [l1, l2])
    }

    #[test]
    fn layers_reorder_and_keep_their_order_in_files() {
        let (mut ed, frame, lines) = two_layers();
        let notes = ed.add_layer("Notes");
        let order = |ed: &Editor| ed.doc.layers().map(|(id, _)| id).collect::<Vec<_>>();
        // New layers go on top.
        assert_eq!(order(&ed), [LayerId::DEFAULT, frame, notes]);
        ed.reorder_layer(notes, 0);
        assert_eq!(order(&ed), [notes, LayerId::DEFAULT, frame]);
        ed.undo();
        assert_eq!(order(&ed), [LayerId::DEFAULT, frame, notes]);
        ed.redo();
        let text = drift_io::save(&ed.doc, &Default::default());
        let (loaded, _) = drift_io::load(&text).unwrap();
        assert_eq!(
            loaded.layers().map(|(id, _)| id).collect::<Vec<_>>(),
            [notes, LayerId::DEFAULT, frame]
        );
        // Curves draw layer by layer: Frame's line (on top of layer 0 now) comes last.
        let [l1, _] = lines;
        let frame_line = ed.doc.shape(l1);
        let last = ed
            .items()
            .into_iter()
            .rev()
            .find(|i| !matches!(i.shape, drift_core::Shape::Point(_)))
            .map(|i| i.shape);
        assert_eq!(last, frame_line);
    }

    #[test]
    fn non_snappable_layers_are_not_snapped_to() {
        let (mut ed, frame, _) = two_layers();
        let options = drift_geom::SnapOptions {
            tolerance: 1.0,
            grid: None,
            from: None,
        };
        let near = |ed: &Editor, p: DVec2| ed.snap(p, options, |_| false).map(|s| s.at);
        assert_eq!(near(&ed, v(10.2, 0.1)), Some(v(10.0, 0.0)));
        let mut off = ed.doc.layer(frame).cloned().unwrap();
        off.snappable = false;
        ed.update_layer(frame, off);
        assert_ne!(near(&ed, v(10.2, 0.1)), Some(v(10.0, 0.0)));
        // Layer 0 still snaps; Frame's line can still be picked.
        assert_eq!(near(&ed, v(10.2, 6.9)), Some(v(10.0, 7.0)));
    }

    fn ends(ed: &Editor, line: EntityId) -> [EntityId; 2] {
        match ed.doc.get(line).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => [a, b],
            _ => panic!(),
        }
    }

    #[test]
    fn new_geometry_goes_on_the_current_layer() {
        let (ed, layer, [l1, l2]) = two_layers();
        assert_eq!(ed.doc.get(l1).unwrap().layer, layer);
        assert_eq!(ed.doc.get(ends(&ed, l1)[0]).unwrap().layer, layer);
        assert_eq!(ed.doc.get(l2).unwrap().layer, LayerId::DEFAULT);
    }

    #[test]
    fn locked_layers_stay_put() {
        let (mut ed, layer, [l1, l2]) = two_layers();
        let mut locked = ed.doc.layer(layer).unwrap().clone();
        locked.locked = true;
        ed.update_layer(layer, locked);
        // Joining the free line's start to the locked line's end moves only the free line.
        let [_, b] = ends(&ed, l1);
        let [c, _] = ends(&ed, l2);
        ed.add_constraints("Coincident", &[Constraint::Coincident { a: b, b: c }])
            .unwrap();
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 0.0));
        assert!((ed.doc.point(c).unwrap() - v(10.0, 0.0)).length() < 1e-9);
        // Constraints the lock can't satisfy are refused, and the lock itself is never named.
        let [a, _] = ends(&ed, l1);
        let r = ed.add_constraints("Length", &[Constraint::Distance { a, b, value: 3.0 }]);
        assert!(r.is_err());
        assert!(ed.analysis().conflicts.iter().all(|c| c.0 != 0));
        // Locked geometry can't be selected.
        ed.selection = [l1].into();
        ed.sync();
        assert!(ed.selection.is_empty());
    }

    #[test]
    fn hidden_layers_are_not_drawn_or_picked() {
        let (mut ed, layer, [l1, _]) = two_layers();
        let mut hidden = ed.doc.layer(layer).unwrap().clone();
        hidden.visible = false;
        ed.update_layer(layer, hidden);
        assert!(ed.index.pick(v(5.0, 0.0), 0.5, |_| true).is_none());
        assert!(
            ed.items()
                .iter()
                .all(|i| i.shape != ed.doc.shape(l1).unwrap())
        );
    }

    #[test]
    fn appearance_follows_the_layer_unless_overridden() {
        let (mut ed, layer, [l1, l2]) = two_layers();
        let mut look = ed.doc.layer(layer).unwrap().clone();
        look.color = Some(Rgb([220, 40, 40]));
        look.weight = LineWeight(70);
        look.line_type = LineType::Dashed;
        ed.update_layer(layer, look);
        let item = |ed: &Editor, id: EntityId, status: bool| {
            let shape = ed.doc.shape(id).unwrap();
            ed.items_styled(status, true)
                .into_iter()
                .find(|i| i.shape == shape)
                .unwrap()
        };
        let i = item(&ed, l1, false);
        assert_eq!(
            (i.color, i.weight, i.line_type),
            (Some([220, 40, 40]), 70, LineType::Dashed)
        );
        // Constraint-status colours ignore layer colours (weights still apply).
        let i = item(&ed, l1, true);
        assert_eq!((i.color, i.weight), (None, 70));
        // Overrides win.
        ed.selection = [l1].into();
        ed.update_selection_appearance(|a| a.weight = Some(LineWeight(13)));
        assert_eq!(item(&ed, l1, false).weight, 13);
        // Moving to another layer takes its look; one undo step each.
        ed.selection = [l2].into();
        ed.move_selection_to_layer(layer);
        assert_eq!(item(&ed, l2, false).line_type, LineType::Dashed);
        assert!(ed.undo());
        assert_eq!(item(&ed, l2, false).line_type, LineType::Continuous);
    }

    #[test]
    fn modified_copies_keep_their_source_layer() {
        let (mut ed, layer, [l1, _]) = two_layers();
        ed.set_current_layer(LayerId::DEFAULT);
        let m = ed.split_at(l1, v(4.0, 0.0)).unwrap();
        let pieces: Vec<EntityId> = ed.doc.users_of(m).collect();
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces
                .iter()
                .all(|p| ed.doc.get(*p).unwrap().layer == layer)
        );
        let _ = (LineKind::Solid, Shape::Point(v(0.0, 0.0)));
    }

    #[test]
    fn deleting_a_layer_moves_its_geometry_to_layer_zero() {
        let (mut ed, layer, [l1, _]) = two_layers();
        ed.remove_layer(layer).unwrap();
        assert_eq!(ed.doc.get(l1).unwrap().layer, LayerId::DEFAULT);
        assert!(ed.remove_layer(LayerId::DEFAULT).is_err());
        ed.undo();
        assert_eq!(ed.doc.get(l1).unwrap().layer, layer);
    }
}
