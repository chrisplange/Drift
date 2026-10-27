//! Layers, and each entity's layer, order and appearance.

use super::*;

impl Document {
    /// All layers in id order (layer "0" first).
    /// The layers in draw order, bottom first.
    pub fn layers(&self) -> impl Iterator<Item = (LayerId, &Layer)> {
        let mut layers: Vec<(LayerId, &Layer)> =
            self.layers.iter().map(|(id, l)| (*id, l)).collect();
        layers.sort_by_key(|(id, l)| (l.order, *id));
        layers.into_iter()
    }

    /// Each layer's place in the draw order (0 at the bottom).
    pub fn layer_ranks(&self) -> std::collections::HashMap<LayerId, usize> {
        self.layers()
            .enumerate()
            .map(|(i, (id, _))| (id, i))
            .collect()
    }

    /// Move a layer to `index` in the draw order (0 at the bottom).
    pub fn reorder_layer(&mut self, id: LayerId, index: usize) {
        let mut order: Vec<LayerId> = self.layers().map(|(l, _)| l).collect();
        let Some(from) = order.iter().position(|l| *l == id) else {
            return;
        };
        order.remove(from);
        order.insert(index.min(order.len()), id);
        for (rank, l) in order.into_iter().enumerate() {
            if let Some(mut layer) = self.layers.get(&l).cloned() {
                layer.order = i32::try_from(rank).unwrap_or(i32::MAX);
                self.set_layer(l, layer);
            }
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }

    /// The layer an entity is on (layer "0" if its layer is missing).
    pub fn layer_of(&self, entity: EntityId) -> &Layer {
        let id = self
            .entities
            .get(&entity)
            .map_or(LayerId::DEFAULT, |e| e.layer);
        self.layers
            .get(&id)
            .or_else(|| self.layers.get(&LayerId::DEFAULT))
            .expect("layer 0 exists")
    }

    /// Add a layer, on top of the others.
    pub fn add_layer(&mut self, mut layer: Layer) -> LayerId {
        layer.order = self.layers.values().map(|l| l.order + 1).max().unwrap_or(0);
        let id = LayerId(self.next_param());
        self.touch_key(Key::Layer(id));
        self.layers.insert(id, layer);
        id
    }

    /// Add or replace a layer with a given id (when loading, or editing its settings).
    pub fn set_layer(&mut self, id: LayerId, layer: Layer) {
        self.reserve_param(id.0);
        if self.layers.get(&id) != Some(&layer) {
            self.touch_key(Key::Layer(id));
            self.layers.insert(id, layer);
        }
    }

    /// Delete a layer, moving its entities to layer "0". Layer "0" can't be deleted.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        if id == LayerId::DEFAULT {
            return Err(DocumentError::BadName("layer 0 can't be deleted".into()));
        }
        if !self.layers.contains_key(&id) {
            return Err(DocumentError::MissingLayer(id));
        }
        let on: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| e.layer == id)
            .map(|(eid, _)| *eid)
            .collect();
        for e in on {
            self.touch(e);
            if let Some(x) = self.entities.get_mut(&e) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let placed: Vec<InstanceId> = self
            .instances
            .iter()
            .filter(|(_, i)| i.layer == id)
            .map(|(iid, _)| *iid)
            .collect();
        for i in placed {
            self.touch_key(Key::Instance(i));
            if let Some(x) = self.instances.get_mut(&i) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let texts: Vec<TextId> = self
            .texts
            .iter()
            .filter(|(_, t)| t.layer == id)
            .map(|(tid, _)| *tid)
            .collect();
        for t in texts {
            self.touch_key(Key::Text(t));
            if let Some(x) = self.texts.get_mut(&t) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let fills: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| f.layer == id)
            .map(|(fid, _)| *fid)
            .collect();
        for f in fills {
            self.touch_key(Key::Fill(f));
            if let Some(x) = self.fills.get_mut(&f) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let views: Vec<ViewId> = self
            .views
            .iter()
            .filter(|(_, v)| v.hidden_layers.contains(&id))
            .map(|(vid, _)| *vid)
            .collect();
        for v in views {
            self.touch_key(Key::View(v));
            if let Some(x) = self.views.get_mut(&v) {
                x.hidden_layers.remove(&id);
            }
        }
        if self.current_layer == id {
            self.set_current_layer(LayerId::DEFAULT)?;
        }
        self.touch_key(Key::Layer(id));
        self.layers.remove(&id);
        Ok(())
    }

    pub fn current_layer(&self) -> LayerId {
        self.current_layer
    }

    pub fn set_current_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&id) {
            return Err(DocumentError::MissingLayer(id));
        }
        if self.current_layer != id {
            self.touch_key(Key::CurrentLayer);
            self.current_layer = id;
        }
        Ok(())
    }

    /// Move an entity to a layer.
    pub fn set_entity_layer(&mut self, id: EntityId, layer: LayerId) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&layer) {
            return Err(DocumentError::MissingLayer(layer));
        }
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.layer != layer {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.layer = layer;
            }
        }
        Ok(())
    }

    /// Set where an entity stacks on its layer (see [`Entity::order`]).
    pub fn set_order(&mut self, id: EntityId, order: Option<i32>) -> Result<(), DocumentError> {
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.order != order {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.order = order;
            }
        }
        Ok(())
    }

    /// Set an entity's appearance overrides.
    pub fn set_appearance(
        &mut self,
        id: EntityId,
        appearance: Appearance,
    ) -> Result<(), DocumentError> {
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.appearance != appearance {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.appearance = appearance;
            }
        }
        Ok(())
    }

    /// Give `to` the same line role, layer and appearance as `from` (for copies made by modify
    /// tools).
    pub fn copy_look(&mut self, from: EntityId, to: EntityId) {
        let Some(src) = self.entities.get(&from).copied() else {
            return;
        };
        if let Some(dst) = self.entities.get(&to).copied()
            && (dst.style, dst.layer, dst.appearance, dst.order)
                != (src.style, src.layer, src.appearance, src.order)
        {
            self.touch(to);
            if let Some(x) = self.entities.get_mut(&to) {
                x.style = src.style;
                x.layer = src.layer;
                x.appearance = src.appearance;
                x.order = src.order;
            }
        }
    }

    /// Entities on a locked layer, and the points they use (for the solver to keep fixed).
    pub fn locked_entities(&self) -> BTreeSet<EntityId> {
        let locked: BTreeSet<LayerId> = self
            .layers
            .iter()
            .filter(|(_, l)| l.locked)
            .map(|(id, _)| *id)
            .collect();
        if locked.is_empty() {
            return BTreeSet::new();
        }
        let mut out = BTreeSet::new();
        for (id, e) in &self.entities {
            if locked.contains(&e.layer) {
                out.insert(*id);
                out.extend(e.kind.points());
            }
        }
        out
    }

    /// True if the entity's layer is visible.
    pub fn is_visible(&self, id: EntityId) -> bool {
        self.layer_of(id).visible
    }

    // ----- Parameters: units, variables, expressions, variants, measurements -----------------
}
