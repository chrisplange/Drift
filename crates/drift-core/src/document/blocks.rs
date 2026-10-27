//! Blocks and their placed copies.

use super::*;

impl Document {
    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &Block)> {
        self.blocks.iter().map(|(id, b)| (*id, b))
    }

    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(&id)
    }

    pub fn block_named(&self, name: &str) -> Option<BlockId> {
        self.blocks
            .iter()
            .find(|(_, b)| b.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
    }

    pub fn add_block(&mut self, block: Block) -> BlockId {
        let id = BlockId(self.next_drafting());
        self.touch_key(Key::Block(id));
        self.blocks.insert(id, block);
        id
    }

    /// Insert a block with a known id (loading a file).
    pub fn insert_block(&mut self, id: BlockId, block: Block) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Block(id));
        self.blocks.insert(id, block);
    }

    /// Change a block's definition: every copy updates.
    pub fn set_block(&mut self, id: BlockId, block: Block) {
        if self.blocks.get(&id).is_some_and(|b| *b != block) {
            self.touch_key(Key::Block(id));
            self.blocks.insert(id, block);
        }
    }

    /// Delete a block and every copy of it.
    pub fn remove_block(&mut self, id: BlockId) {
        let copies: Vec<InstanceId> = self
            .instances
            .iter()
            .filter(|(_, i)| i.block == id)
            .map(|(iid, _)| *iid)
            .collect();
        for i in copies {
            self.remove_instance(i);
        }
        if self.blocks.contains_key(&id) {
            self.touch_key(Key::Block(id));
            self.blocks.remove(&id);
        }
    }

    pub fn instances(&self) -> impl Iterator<Item = (InstanceId, &Instance)> {
        self.instances.iter().map(|(id, i)| (*id, i))
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(&id)
    }

    pub(super) fn check_instance(&self, i: &Instance) -> Result<(), DocumentError> {
        if !self.blocks.contains_key(&i.block) {
            return Err(DocumentError::BadName(format!(
                "block {} does not exist",
                i.block
            )));
        }
        if !self.layers.contains_key(&i.layer) {
            return Err(DocumentError::MissingLayer(i.layer));
        }
        if !(i.at.is_finite() && i.angle.is_finite() && i.scale.is_finite() && i.scale > 0.0) {
            return Err(DocumentError::InvalidValue);
        }
        Ok(())
    }

    /// Place a copy of a block (on the current layer).
    pub fn add_instance(&mut self, mut i: Instance) -> Result<InstanceId, DocumentError> {
        i.layer = self.current_layer;
        self.check_instance(&i)?;
        let id = InstanceId(self.next_drafting());
        self.touch_key(Key::Instance(id));
        self.instances.insert(id, i);
        Ok(id)
    }

    /// Insert a copy with a known id (loading a file).
    pub fn insert_instance(&mut self, id: InstanceId, i: Instance) -> Result<(), DocumentError> {
        self.check_instance(&i)?;
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Instance(id));
        self.instances.insert(id, i);
        Ok(())
    }

    pub fn set_instance(&mut self, id: InstanceId, i: Instance) -> Result<(), DocumentError> {
        self.check_instance(&i)?;
        if self.instances.get(&id).is_some_and(|x| *x != i) {
            self.touch_key(Key::Instance(id));
            self.instances.insert(id, i);
        }
        Ok(())
    }

    pub fn remove_instance(&mut self, id: InstanceId) {
        if self.instances.contains_key(&id) {
            self.touch_key(Key::Instance(id));
            self.instances.remove(&id);
        }
    }

    /// A copy's curves in the world, with their style and look.
    pub fn instance_items(&self, id: InstanceId) -> Vec<BlockItem> {
        let (Some(i), Some(b)) = (
            self.instances.get(&id),
            self.instances
                .get(&id)
                .and_then(|i| self.blocks.get(&i.block)),
        ) else {
            return Vec::new();
        };
        b.items
            .iter()
            .map(|item| BlockItem {
                shape: i.apply_shape(&item.shape),
                ..*item
            })
            .collect()
    }
}
