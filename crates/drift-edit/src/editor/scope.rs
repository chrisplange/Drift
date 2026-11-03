//! What tools see while a block is edited in place or a group is open for editing: the
//! group's members alone (#265, #298, #299). The rest of the drawing is there to look at.

use super::*;

/// The entities tools may see and change while a group is open: its members, the points of
/// their curves, and what walls, openings and offsets made of them generate.
pub type EditScope = std::rc::Rc<HashSet<EntityId>>;

impl Editor {
    /// The group being edited: a block's while one is edited in place (whatever else is
    /// open), else the open group. None when it no longer exists.
    pub fn edit_group(&self) -> Option<drift_core::GroupId> {
        let g = self.block_edit.map(|e| e.group).or(self.open_group)?;
        self.doc.group(g).is_some().then_some(g)
    }

    /// While a group or block is open for editing, what tools see (found from the group as
    /// it is now, kept while the drawing's make-up stays the same). None: everything.
    pub fn edit_scope(&self) -> Option<EditScope> {
        if self.block_edit.is_none() && self.open_group.is_none() {
            return None;
        }
        let g = self.edit_group()?;
        let structure = self.doc.structure();
        if let Some((cg, cs, scope)) = &*self.edit_scope_cache.borrow()
            && (*cg, *cs) == (g, structure)
        {
            return Some(scope.clone());
        }
        let scope = std::rc::Rc::new(self.group_scope(g)?);
        *self.edit_scope_cache.borrow_mut() = Some((g, structure, scope.clone()));
        Some(scope)
    }

    /// What a group open for editing holds, found now: its members, the points of their
    /// curves, and what walls, openings and offsets made of them generate.
    pub(crate) fn group_scope(&self, g: drift_core::GroupId) -> Option<HashSet<EntityId>> {
        let members = &self.doc.group(g)?.members;
        let mut scope: HashSet<EntityId> = members.iter().copied().collect();
        scope.extend(self.doc.defining_points(members.iter().copied()));
        // Generated geometry belongs through its record: a wall's faces (and its openings')
        // when its centreline is in, an offset's curves when its source is.
        let mut walls = HashSet::new();
        for (id, w) in self.doc.walls() {
            let centre = self.doc.path(w.centre).map_or(&[][..], |p| &p.curves[..]);
            if !centre.is_empty() && centre.iter().all(|c| scope.contains(c)) {
                walls.insert(id);
                scope.extend(w.made.entities());
            }
        }
        let made: Vec<EntityId> = self
            .doc
            .openings()
            .filter(|(_, o)| walls.contains(&o.wall))
            .flat_map(|(_, o)| o.made.entities())
            .chain(
                self.doc
                    .offsets()
                    .filter(|(_, o)| !o.source.is_empty())
                    .filter(|(_, o)| o.source.iter().all(|c| scope.contains(c)))
                    .flat_map(|(_, o)| o.made.entities()),
            )
            .collect();
        scope.extend(self.doc.defining_points(made.iter().copied()));
        scope.extend(made);
        Some(scope)
    }

    /// Whether tools may see entity `id`: always, unless a group or block is open for
    /// editing and it isn't part of it.
    pub fn in_scope(&self, id: EntityId) -> bool {
        self.edit_scope().is_none_or(|s| s.contains(&id))
    }

    /// Whether tools may see a fill: one whose whole outline is in the
    /// [`edit_scope`](Self::edit_scope) (what finishing a block edit takes into the block).
    pub fn fill_in_scope(&self, fill: drift_core::FillId) -> bool {
        let Some(scope) = self.edit_scope() else {
            return true;
        };
        self.doc.fill(fill).is_some_and(|f| {
            let path = f.path.and_then(|p| self.doc.path(p));
            let outline = match path {
                Some(p) if f.boundary.is_empty() => &p.curves[..],
                _ => &f.boundary[..],
            };
            !outline.is_empty() && outline.iter().all(|c| scope.contains(c))
        })
    }

    /// Whether a dimension or relation may be picked: every entity it is on is in the
    /// [`edit_scope`](Self::edit_scope).
    pub fn constraint_in_scope(&self, c: &drift_core::Constraint) -> bool {
        self.edit_scope()
            .is_none_or(|s| c.entities().all(|e| s.contains(&e)))
    }

    /// Whether an annotation may be picked: it points at something, all of it in the
    /// [`edit_scope`](Self::edit_scope) (a note on nothing isn't part of a group).
    pub fn annotation_in_scope(&self, id: drift_core::AnnotationId) -> bool {
        let Some(scope) = self.edit_scope() else {
            return true;
        };
        self.doc.annotation(id).is_some_and(|a| {
            let on = a.entities();
            !on.is_empty() && on.iter().all(|e| scope.contains(e))
        })
    }

    /// Keep only the picks inside a group or block open for editing (after a pick made from a
    /// list of the whole drawing's: overlaps, a check's issue, a relation's curves).
    pub fn retain_in_scope(&mut self) {
        let Some(scope) = self.edit_scope() else {
            return;
        };
        self.selection.retain(|id| scope.contains(id));
        self.selected_instances.clear();
        self.selected_texts.clear();
        let fills: Vec<_> = self.selected_fills.iter().copied().collect();
        for f in fills {
            if !self.fill_in_scope(f) {
                self.selected_fills.remove(&f);
            }
        }
    }

    /// Refuse to change what's outside a group or block open for editing (the tools can't
    /// reach it; scripts and the assistant can).
    pub(crate) fn refuse_outside(&self, ids: &[EntityId]) -> Result<(), EditError> {
        if ids.iter().all(|id| self.in_scope(*id)) {
            Ok(())
        } else {
            Err(EditError::Invalid(
                "that is outside what's being edited (Esc finishes editing)".into(),
            ))
        }
    }

    /// What snapping sees: only the block's own geometry while one is edited in place (a
    /// join to anything else would be lost when the edit is written back). A plain open
    /// group snaps to the whole drawing: its geometry stays in it.
    pub(crate) fn snap_scope(&self) -> Option<EditScope> {
        self.block_edit.and(self.edit_scope())
    }

    /// Let go of an edit whose group is gone (an undo past "Edit block", say): nothing is
    /// open any more, and the copy picks again. With nothing open, the last open group's
    /// areas go too.
    pub(crate) fn drop_lost_edit(&mut self) {
        if let Some(g) = self.block_edit.map(|e| e.group).or(self.open_group)
            && self.doc.group(g).is_none()
        {
            self.block_edit = None;
            self.open_group = None;
        }
        if self.edit_group().is_none() {
            self.scoped_regions = None;
            self.region_moving = None;
        }
    }

    /// After an undo or redo: back in the block edit whose group is there again (redo of
    /// "Edit block", undo of "Finish editing block", #319), when its copy still places its
    /// block. Otherwise nothing is open.
    pub(crate) fn resume_block_edit(&mut self) {
        if self.block_edit.is_some() {
            return;
        }
        let holds = |e: &crate::blocks::BlockEdit| {
            self.doc.group(e.group).is_some()
                && self.doc.block(e.block).is_some()
                && self
                    .doc
                    .instance(e.instance)
                    .is_some_and(|i| i.block == e.block)
        };
        // (One at most: an edit's group goes when it finishes.)
        let Some(edit) = self
            .block_edits
            .values()
            .filter(|e| holds(e))
            .max_by_key(|e| e.group)
            .copied()
        else {
            return;
        };
        self.block_edit = Some(edit);
        self.open_group = Some(edit.group);
        self.scoped_regions = None;
        self.region_moving = None;
        // (No copies stay picked inside a block edit: the one edited among them.)
        self.retain_in_scope();
    }
}
