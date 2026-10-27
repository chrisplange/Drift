//! Groups and paths: named sets of entities, and runs of joined curves.

use super::*;

impl Document {
    pub fn groups(&self) -> impl Iterator<Item = (GroupId, &Group)> {
        self.groups.iter().map(|(id, g)| (*id, g))
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    /// The group an entity belongs to.
    pub fn group_of(&self, entity: EntityId) -> Option<GroupId> {
        self.groups
            .iter()
            .find(|(_, g)| g.members.contains(&entity))
            .map(|(id, _)| *id)
    }

    /// Take `members` out of any group they are in (dropping groups left empty).
    pub(super) fn ungroup_members(&mut self, members: &BTreeSet<EntityId>) {
        let touched: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| !g.members.is_disjoint(members))
            .map(|(id, _)| *id)
            .collect();
        for id in touched {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.retain(|e| !members.contains(e));
                if g.members.is_empty() {
                    self.groups.remove(&id);
                }
            }
        }
    }

    /// Make a group. Its members leave any group they were in.
    pub fn add_group(&mut self, group: Group) -> Result<GroupId, DocumentError> {
        if let Some(e) = group
            .members
            .iter()
            .find(|e| !self.entities.contains_key(e))
        {
            return Err(DocumentError::Missing(*e));
        }
        self.ungroup_members(&group.members);
        let id = GroupId(self.next_drafting());
        self.touch_key(Key::Group(id));
        self.groups.insert(id, group);
        Ok(id)
    }

    /// Insert a group with a known id (loading a file).
    pub fn insert_group(&mut self, id: GroupId, group: Group) -> Result<(), DocumentError> {
        if let Some(e) = group
            .members
            .iter()
            .find(|e| !self.entities.contains_key(e))
        {
            return Err(DocumentError::Missing(*e));
        }
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Group(id));
        self.groups.insert(id, group);
        Ok(())
    }

    /// Add entities to a group (they leave any other group).
    pub fn add_group_members(
        &mut self,
        id: GroupId,
        members: impl IntoIterator<Item = EntityId>,
    ) -> Result<(), DocumentError> {
        let members: BTreeSet<EntityId> = members.into_iter().collect();
        if let Some(e) = members.iter().find(|e| !self.entities.contains_key(e)) {
            return Err(DocumentError::Missing(*e));
        }
        if !self.groups.contains_key(&id) || members.is_empty() {
            return Ok(());
        }
        let others: BTreeSet<EntityId> = members
            .iter()
            .copied()
            .filter(|e| self.group_of(*e).is_some_and(|g| g != id))
            .collect();
        self.ungroup_members(&others);
        self.touch_key(Key::Group(id));
        if let Some(g) = self.groups.get_mut(&id) {
            g.members.extend(members);
        }
        Ok(())
    }

    pub fn rename_group(&mut self, id: GroupId, name: &str) {
        if let Some(g) = self.groups.get(&id)
            && g.name != name
        {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.name = name.to_owned();
            }
        }
    }

    /// Dissolve a group (its members stay).
    pub fn remove_group(&mut self, id: GroupId) {
        if self.groups.contains_key(&id) {
            self.touch_key(Key::Group(id));
            self.groups.remove(&id);
        }
    }

    pub fn paths(&self) -> impl Iterator<Item = (PathId, &Path)> {
        self.paths.iter().map(|(id, p)| (*id, p))
    }

    pub fn path(&self, id: PathId) -> Option<&Path> {
        self.paths.get(&id)
    }

    /// The path a curve belongs to.
    pub fn path_of(&self, curve: EntityId) -> Option<PathId> {
        self.paths
            .iter()
            .find(|(_, p)| p.curves.contains(&curve))
            .map(|(id, _)| *id)
    }

    /// Take `curves` out of any path they are in (a path left with none goes, and its fill
    /// is unbound).
    pub(super) fn unpath_curves(&mut self, curves: &[EntityId]) {
        let touched: Vec<PathId> = self
            .paths
            .iter()
            .filter(|(_, p)| p.curves.iter().any(|c| curves.contains(c)))
            .map(|(id, _)| *id)
            .collect();
        for id in touched {
            self.touch_key(Key::Path(id));
            let empty = match self.paths.get_mut(&id) {
                Some(p) => {
                    p.curves.retain(|c| !curves.contains(c));
                    p.curves.is_empty()
                }
                None => false,
            };
            if empty {
                self.paths.remove(&id);
                self.unbind_fills(id);
            }
        }
    }

    pub(super) fn check_path(&self, path: &Path) -> Result<(), DocumentError> {
        if path.curves.is_empty() {
            return Err(DocumentError::Degenerate("path"));
        }
        match path
            .curves
            .iter()
            .find(|c| !self.entities.get(c).is_some_and(|e| !e.kind.is_point()))
        {
            Some(c) => Err(DocumentError::Missing(*c)),
            None => Ok(()),
        }
    }

    /// Make a path: curves drawn as one shape, in order. They leave any path they were in.
    pub fn add_path(&mut self, path: Path) -> Result<PathId, DocumentError> {
        self.check_path(&path)?;
        self.unpath_curves(&path.curves);
        let id = PathId(self.next_drafting());
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(id)
    }

    /// Insert a path with a known id (loading a file).
    pub fn insert_path(&mut self, id: PathId, path: Path) -> Result<(), DocumentError> {
        self.check_path(&path)?;
        self.unpath_curves(&path.curves);
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(())
    }

    /// Replace a path's curves and closure. Curves it takes leave any other path.
    pub fn set_path(&mut self, id: PathId, path: Path) -> Result<(), DocumentError> {
        if !self.paths.contains_key(&id) || self.paths.get(&id) == Some(&path) {
            return Ok(());
        }
        self.check_path(&path)?;
        let taken: Vec<EntityId> = path
            .curves
            .iter()
            .copied()
            .filter(|c| self.path_of(*c).is_some_and(|p| p != id))
            .collect();
        self.unpath_curves(&taken);
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(())
    }

    /// Remove a path (its curves stay); its fill fills an area among the sketch's curves
    /// again.
    pub fn remove_path(&mut self, id: PathId) {
        if self.paths.contains_key(&id) {
            self.touch_key(Key::Path(id));
            self.paths.remove(&id);
            self.unbind_fills(id);
        }
    }

    // ----- Walls ---------------------------------------------------------------------------
}
