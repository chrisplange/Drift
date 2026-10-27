//! Walls, their property dimensions and their openings.

use super::*;

impl Document {
    pub fn walls(&self) -> impl Iterator<Item = (WallId, &Wall)> {
        self.walls.iter().map(|(id, w)| (*id, w))
    }

    pub fn wall(&self, id: WallId) -> Option<&Wall> {
        self.walls.get(&id)
    }

    pub fn has_walls(&self) -> bool {
        !self.walls.is_empty()
    }

    pub(super) fn check_wall(&self, wall: &Wall) -> Result<(), DocumentError> {
        if !self.paths.contains_key(&wall.centre) {
            return Err(DocumentError::BadName(
                "a wall's centreline must be a path".into(),
            ));
        }
        if self.layer(wall.layer).is_none() {
            return Err(DocumentError::MissingLayer(wall.layer));
        }
        if !(wall.thickness > 0.0 && wall.thickness.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        Ok(())
    }

    pub(super) fn next_wall(&mut self) -> u32 {
        let id = self.next_wall_id;
        self.next_wall_id += 1;
        id
    }

    /// Add a wall along an existing path. Its faces are made by the walls' regeneration
    /// (`drift_geom::walls`), not here.
    pub fn add_wall(&mut self, wall: Wall) -> Result<WallId, DocumentError> {
        self.check_wall(&wall)?;
        let id = WallId(self.next_wall());
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(id)
    }

    /// Insert a wall with a known id (loading a file).
    pub fn insert_wall(&mut self, id: WallId, wall: Wall) -> Result<(), DocumentError> {
        self.check_wall(&wall)?;
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(())
    }

    /// Replace a wall's record (its settings, joins or what it made).
    pub fn set_wall(&mut self, id: WallId, wall: Wall) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&id) || self.walls.get(&id) == Some(&wall) {
            return Ok(());
        }
        self.check_wall(&wall)?;
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(())
    }

    /// Remove a wall's record, and its property dimensions. What it made stays, as plain
    /// geometry (Explode), unless the caller deletes it.
    pub fn remove_wall(&mut self, id: WallId) {
        if self.walls.contains_key(&id) {
            self.touch_key(Key::Wall(id));
            self.walls.remove(&id);
        }
        let dims: Vec<PropertyDimId> = self
            .property_dims
            .iter()
            .filter(|(_, d)| d.target.wall() == id)
            .map(|(d, _)| *d)
            .collect();
        for d in dims {
            self.remove_property_dim(d);
        }
    }

    /// Who owns which entity, path and fill among the walls' (built afresh: callers that ask
    /// often cache it by revision).
    pub fn wall_owners(&self) -> Owners {
        let mut out = Owners::default();
        for (&id, w) in self.walls.iter() {
            for e in w.made.entities() {
                out.of.insert(e, Owner::Wall(id));
            }
            for p in w.made.outlines.values() {
                out.paths.insert(*p, id);
            }
            for f in w.made.fills.values() {
                out.fills.insert(*f, id);
            }
            out.centres.insert(w.centre, id);
            if let Some(path) = self.paths.get(&w.centre) {
                for c in &path.curves {
                    out.of.insert(*c, Owner::Centre(id));
                    if let Some(e) = self.entities.get(c) {
                        for p in e.kind.points() {
                            out.of.entry(p).or_insert(Owner::Centre(id));
                        }
                    }
                }
            }
        }
        for (&id, o) in self.openings.iter() {
            for e in o.made.entities() {
                out.of.insert(e, Owner::Opening(id, o.wall));
            }
            // Its jambs (the wall's edges across its cut) pick it too.
            if let Some(w) = self.walls.get(&o.wall) {
                for (k, e) in &w.made.curves {
                    if k.part == crate::Part::Jamb
                        && matches!(k.from, crate::Station::Jamb(x, _) if x == id)
                    {
                        out.of.insert(*e, Owner::Opening(id, o.wall));
                    }
                }
            }
        }
        out
    }

    /// The entities walls (and the openings in them) generated: never solver unknowns.
    pub fn generated_entities(&self) -> std::collections::HashSet<EntityId> {
        self.walls
            .values()
            .flat_map(|w| w.made.entities())
            .chain(self.openings.values().flat_map(|o| o.made.entities()))
            .collect()
    }

    // ----- Openings ------------------------------------------------------------------------

    pub fn openings(&self) -> impl Iterator<Item = (OpeningId, &Opening)> {
        self.openings.iter().map(|(id, o)| (*id, o))
    }

    pub fn opening(&self, id: OpeningId) -> Option<&Opening> {
        self.openings.get(&id)
    }

    pub fn has_openings(&self) -> bool {
        !self.openings.is_empty()
    }

    pub(super) fn check_opening(&self, o: &Opening) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&o.wall) {
            return Err(DocumentError::Degenerate("opening without its wall"));
        }
        if !(o.width > 0.0 && o.width.is_finite() && o.leaf.is_finite() && o.height.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        if let crate::OpeningAt::From { offset, .. } = o.at
            && !offset.is_finite()
        {
            return Err(DocumentError::InvalidValue);
        }
        if let Some(l) = o.layer
            && self.layer(l).is_none()
        {
            return Err(DocumentError::MissingLayer(l));
        }
        Ok(())
    }

    /// Add an opening to a wall. Its cut and symbol are made by the walls' regeneration.
    pub fn add_opening(&mut self, o: Opening) -> Result<OpeningId, DocumentError> {
        self.check_opening(&o)?;
        let id = OpeningId(self.next_wall());
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(id)
    }

    /// Insert an opening with a known id (loading a file).
    pub fn insert_opening(&mut self, id: OpeningId, o: Opening) -> Result<(), DocumentError> {
        self.check_opening(&o)?;
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(())
    }

    /// Replace an opening's record (where it is, its size and swing, or what it made).
    pub fn set_opening(&mut self, id: OpeningId, o: Opening) -> Result<(), DocumentError> {
        if !self.openings.contains_key(&id) || self.openings.get(&id) == Some(&o) {
            return Ok(());
        }
        self.check_opening(&o)?;
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(())
    }

    /// Remove an opening's record. What its symbol made stays, as plain geometry (callers
    /// delete it, or regenerating does).
    pub fn remove_opening(&mut self, id: OpeningId) {
        if self.openings.contains_key(&id) {
            self.touch_key(Key::Opening(id));
            self.openings.remove(&id);
        }
    }

    pub fn property_dims(&self) -> impl Iterator<Item = (PropertyDimId, &PropertyDim)> {
        self.property_dims.iter().map(|(id, d)| (*id, d))
    }

    pub fn property_dim(&self, id: PropertyDimId) -> Option<&PropertyDim> {
        self.property_dims.get(&id)
    }

    pub fn add_property_dim(&mut self, dim: PropertyDim) -> Result<PropertyDimId, DocumentError> {
        let id = PropertyDimId(self.next_wall());
        self.insert_property_dim(id, dim)?;
        Ok(id)
    }

    /// Insert a property dimension with a known id (loading a file).
    pub fn insert_property_dim(
        &mut self,
        id: PropertyDimId,
        dim: PropertyDim,
    ) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&dim.target.wall()) {
            return Err(DocumentError::BadName(
                "a property dimension needs its wall".into(),
            ));
        }
        if !(dim.along.is_finite() && dim.out.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::PropertyDim(id));
        self.property_dims.insert(id, dim);
        Ok(())
    }

    pub fn set_property_dim(&mut self, id: PropertyDimId, dim: PropertyDim) {
        if self.property_dims.contains_key(&id) && self.property_dims.get(&id) != Some(&dim) {
            self.touch_key(Key::PropertyDim(id));
            self.property_dims.insert(id, dim);
        }
    }

    pub fn remove_property_dim(&mut self, id: PropertyDimId) {
        if self.property_dims.contains_key(&id) {
            self.touch_key(Key::PropertyDim(id));
            self.property_dims.remove(&id);
        }
    }
}
