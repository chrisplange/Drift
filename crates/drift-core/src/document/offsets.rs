//! Offsets (#247): the records, kept beside the walls they follow the protocol of.

use super::*;

impl Document {
    pub fn offsets(&self) -> impl Iterator<Item = (OffsetId, &Offset)> {
        self.offsets.iter().map(|(id, o)| (*id, o))
    }

    pub fn offset(&self, id: OffsetId) -> Option<&Offset> {
        self.offsets.get(&id)
    }

    pub fn has_offsets(&self) -> bool {
        !self.offsets.is_empty()
    }

    pub(super) fn check_offset(&self, o: &Offset) -> Result<(), DocumentError> {
        if o.source.is_empty() {
            return Err(DocumentError::BadName(
                "an offset needs a curve to offset".into(),
            ));
        }
        if !(o.distance > 0.0 && o.distance.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        Ok(())
    }

    /// Add an offset of existing curves. Its pieces are made by the offsets' regeneration
    /// (`drift_geom::offsets`), not here.
    pub fn add_offset(&mut self, o: Offset) -> Result<OffsetId, DocumentError> {
        self.check_offset(&o)?;
        let id = OffsetId(self.next_wall());
        self.touch_key(Key::Offset(id));
        self.offsets.insert(id, o);
        Ok(id)
    }

    /// Insert an offset with a known id (loading a file).
    pub fn insert_offset(&mut self, id: OffsetId, o: Offset) -> Result<(), DocumentError> {
        self.check_offset(&o)?;
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::Offset(id));
        self.offsets.insert(id, o);
        Ok(())
    }

    /// Replace an offset's record (its distance, side, source or what it made).
    pub fn set_offset(&mut self, id: OffsetId, o: Offset) -> Result<(), DocumentError> {
        if !self.offsets.contains_key(&id) || self.offsets.get(&id) == Some(&o) {
            return Ok(());
        }
        self.check_offset(&o)?;
        self.touch_key(Key::Offset(id));
        self.offsets.insert(id, o);
        Ok(())
    }

    /// Remove an offset's record, and its property dimension. What it made stays, as plain
    /// geometry (Detach), unless the caller deletes it.
    pub fn remove_offset(&mut self, id: OffsetId) {
        if self.offsets.contains_key(&id) {
            self.touch_key(Key::Offset(id));
            self.offsets.remove(&id);
        }
        let dims: Vec<PropertyDimId> = self
            .property_dims
            .iter()
            .filter(|(_, d)| d.target.offset() == Some(id))
            .map(|(d, _)| *d)
            .collect();
        for d in dims {
            self.remove_property_dim(d);
        }
    }

    /// The offsets the open transaction added, changed or removed (sorted).
    pub fn offsets_touched_in_transaction(&self) -> Vec<OffsetId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut out: Vec<OffsetId> = journal
            .keys()
            .filter_map(|k| match k {
                Key::Offset(id) => Some(*id),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// Offset `id` as it was when the open transaction began.
    pub fn offset_before_transaction(&self, id: OffsetId) -> Option<Offset> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Offset(id))) {
            Some(Some(Record::Offset(o))) => Some(o.clone()),
            Some(_) => None,
            None => self.offsets.get(&id).cloned(),
        }
    }

    /// Offsets whose source curves are all gone, or whose pieces are all gone (the offset
    /// was deleted), go with whatever they still hold (and come back with it on undo).
    pub(super) fn prune_offsets(&mut self) {
        if self.offsets.is_empty() {
            return;
        }
        let gone: Vec<OffsetId> = self
            .offsets
            .iter()
            .filter(|(_, o)| {
                let source = o.source.iter().any(|c| self.entities.contains_key(c));
                let pieces = o.made.curves.is_empty()
                    || o.made
                        .curves
                        .values()
                        .any(|c| self.entities.contains_key(c));
                !source || !pieces
            })
            .map(|(id, _)| *id)
            .collect();
        let mut made: Vec<EntityId> = Vec::new();
        for id in &gone {
            if let Some(o) = self.offsets.get(id) {
                made.extend(o.made.entities());
            }
            self.remove_offset(*id);
        }
        let made: Vec<EntityId> = made
            .into_iter()
            .filter(|e| self.entities.contains_key(e))
            .collect();
        if !made.is_empty() {
            self.delete(made);
        }
    }
}
