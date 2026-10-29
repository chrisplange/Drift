//! What is picked: deleting, duplicating, chains, groups, construction, bounds and locks.

use super::*;

/// The curves drawn on top of earlier ones ([`Editor::overlapping`]), kept from one revision
/// to the next.
#[derive(Default)]
pub(crate) struct Overlaps {
    revision: u64,
    lineage: u64,
    /// Each curve on top, with the earlier curves it is over.
    over: BTreeMap<EntityId, BTreeSet<EntityId>>,
    /// Each curve under, with the later curves over it.
    under: HashMap<EntityId, BTreeSet<EntityId>>,
    found: std::rc::Rc<Vec<EntityId>>,
}

impl Overlaps {
    fn add(&mut self, upper: EntityId, lower: EntityId) {
        self.over.entry(upper).or_default().insert(lower);
        self.under.entry(lower).or_default().insert(upper);
    }
}

impl Editor {
    pub fn delete_selection(&mut self) {
        self.delete_selection_with(&[]);
    }

    /// Delete the selection and the picked text, annotations and dimensions, with
    /// `constraints` (the app's picked dimension or relation), as one undo step.
    pub fn delete_selection_with(&mut self, constraints: &[ConstraintId]) {
        let notes = self.picked_notes();
        let mut dims = self.picked_dimensions();
        dims.extend(constraints);
        if self.selection.is_empty()
            && self.selected_fills.is_empty()
            && notes.is_empty()
            && dims.is_empty()
            && self.selected_instances.is_empty()
            && self.selected_texts.is_empty()
        {
            return;
        }
        self.doc.begin();
        for id in std::mem::take(&mut self.selected_texts) {
            self.doc.remove_text(id);
        }
        for id in std::mem::take(&mut self.selected_instances) {
            self.doc.remove_instance(id);
        }
        for id in notes {
            self.doc.remove_annotation(id);
        }
        for id in dims {
            self.doc.remove_constraint(id);
        }
        self.selected_annotation = None;
        self.annotation_picks.clear();
        for id in std::mem::take(&mut self.selected_fills) {
            self.doc.remove_fill(id);
        }
        // Picked openings go (their walls close up), not their walls.
        let mut kept_out: BTreeSet<EntityId> = BTreeSet::new();
        for o in self.selected_openings() {
            kept_out.extend(self.opening_pick_set(o));
            if let Some(op) = self.doc.opening(o).cloned() {
                self.doc.remove_opening(o);
                self.doc.delete(op.made.entities().collect::<Vec<_>>());
            }
        }
        let curves: Vec<EntityId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| !kept_out.contains(id) && self.doc.contains(*id))
            .collect();
        self.delete_curves(curves);
        self.commit("Delete");
    }

    /// Copy the selection in place (Blender's Shift+D): the curves with their points, the
    /// relations and dimensions among them, fills and hatches of copied areas, and picked
    /// block copies, text and annotations. The copies become the selection. Returns how many
    /// things were copied.
    pub fn duplicate_selection(&mut self) -> usize {
        let picked = crate::clipboard::Picked {
            curves: self.selection.iter().copied().collect(),
            instances: self.selected_instances.iter().copied().collect(),
            texts: self.selected_texts.iter().copied().collect(),
            fills: self.selected_fills.iter().copied().collect(),
            annotations: self.picked_notes(),
        };
        if picked.curves.is_empty()
            && picked.instances.is_empty()
            && picked.texts.is_empty()
            && picked.annotations.is_empty()
        {
            return 0;
        }
        let source = self.doc.clone();
        self.doc.begin();
        let made = crate::clipboard::copy_into(&source, &mut self.doc, &picked, DVec2::ZERO, true);
        self.commit("Duplicate");
        self.selection = picked
            .curves
            .iter()
            .filter_map(|id| made.entities.get(id).copied())
            .collect();
        let n =
            self.selection.len() + made.instances.len() + made.texts.len() + made.annotations.len();
        self.selected_instances = made.instances.into_iter().collect();
        self.selected_fills = made.fills;
        self.selected_texts = made.texts.into_iter().collect();
        self.selected_region = None;
        self.selected_annotation = None;
        self.annotation_picks = made
            .annotations
            .into_iter()
            .map(drift_core::AnnotationRef::Annotation)
            .collect();
        n
    }

    /// Curves drawn on top of other curves (the later-made one of each pair): the same line
    /// twice, or walls of abutting shapes that run along each other. They are easy to make
    /// by accident and confuse trimming and selection. Found once per change.
    ///
    /// After an edit only the curves it changed are looked at again (with what they are over
    /// and what is over them): a big drawing isn't searched through after every edit (#253).
    pub fn overlapping(&self) -> std::rc::Rc<Vec<EntityId>> {
        let revision = self.doc.revision();
        let mut cache = self.overlap_cache.borrow_mut();
        if let Some(c) = &*cache
            && c.revision == revision
            && c.lineage == self.doc.lineage()
        {
            return c.found.clone();
        }
        // (The changes since, if few and the index has them.)
        let changes = cache
            .as_ref()
            .filter(|_| self.index.is_current(&self.doc))
            .and_then(|c| self.doc.changes_since(c.lineage, c.revision))
            .filter(|ch| !ch.layers && ch.entities.len() <= self.doc.len() / 16);
        match (cache.as_mut(), changes) {
            (Some(c), Some(changes)) => {
                // Curves on a changed point changed with it.
                let mut changed: BTreeSet<EntityId> = changes.entities.iter().copied().collect();
                for &id in &changes.entities {
                    changed.extend(self.index.users_of(id));
                }
                for &id in &changed {
                    for lower in c.over.remove(&id).into_iter().flatten() {
                        if let Some(u) = c.under.get_mut(&lower) {
                            u.remove(&id);
                        }
                    }
                    for upper in c.under.remove(&id).into_iter().flatten() {
                        if let Some(o) = c.over.get_mut(&upper) {
                            o.remove(&id);
                        }
                    }
                }
                for &id in &changed {
                    for (upper, lower) in self.overlaps_of(id) {
                        c.add(upper, lower);
                    }
                }
                c.revision = revision;
                c.found = std::rc::Rc::new(
                    c.over
                        .iter()
                        .filter(|(_, lower)| !lower.is_empty())
                        .map(|(&id, _)| id)
                        .collect(),
                );
                c.found.clone()
            }
            _ => {
                let mut c = Overlaps {
                    revision,
                    lineage: self.doc.lineage(),
                    ..Default::default()
                };
                for (id, e) in self.doc.iter() {
                    if e.kind.is_point() {
                        continue;
                    }
                    for lower in self.under(id) {
                        c.add(id, lower);
                    }
                }
                c.found = std::rc::Rc::new(c.over.keys().copied().collect());
                let found = c.found.clone();
                *cache = Some(c);
                found
            }
        }
    }

    /// The overlapping curves as last found (not looked for again): what the status bar
    /// shows in the middle of a drag.
    pub(crate) fn overlapping_as_last_found(&self) -> std::rc::Rc<Vec<EntityId>> {
        self.overlap_cache
            .borrow()
            .as_ref()
            .map(|c| c.found.clone())
            .unwrap_or_default()
    }

    /// The earlier curves `id` is drawn over, as the index has the shapes.
    fn under(&self, id: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        let shape = self.index.shape(id).filter(|s| {
            matches!(
                s,
                Shape::Line { .. } | Shape::Arc { .. } | Shape::Circle { .. }
            )
        });
        shape.into_iter().flat_map(move |shape| {
            self.index.query(shape.bounds()).filter(move |&other| {
                other < id
                    && self
                        .index
                        .shape(other)
                        .is_some_and(|s| drift_geom::intersect::overlapping(shape, s))
            })
        })
    }

    /// The pairs (the curve on top, the earlier one it is over) `id` is in, as the index
    /// has the shapes.
    fn overlaps_of(&self, id: EntityId) -> impl Iterator<Item = (EntityId, EntityId)> + '_ {
        let over = |s: &Shape| {
            matches!(
                s,
                Shape::Line { .. } | Shape::Arc { .. } | Shape::Circle { .. }
            )
        };
        let shape = self.index.shape(id);
        shape
            .into_iter()
            .flat_map(move |shape| self.index.query(shape.bounds()))
            .filter(move |&other| other != id)
            .filter_map(move |other| {
                let (upper, lower) = if other < id { (id, other) } else { (other, id) };
                let (a, b) = (self.index.shape(upper)?, self.index.shape(lower)?);
                (over(a) && drift_geom::intersect::overlapping(a, b)).then_some((upper, lower))
            })
    }

    /// The curves joined end to end with `start` (lines, arcs, elliptical arcs and pen curves
    /// meeting at shared end points; circles and ellipses stand alone), as Revit's Tab
    /// selects a chain of walls.
    pub fn chain_of(&self, start: EntityId) -> BTreeSet<EntityId> {
        let ends = |id: EntityId| -> Vec<EntityId> {
            self.doc
                .get(id)
                .and_then(|e| e.kind.ends())
                .map_or_else(Vec::new, Vec::from)
        };
        let mut chain = BTreeSet::new();
        if self.doc.get(start).is_none_or(|e| e.kind.is_point()) {
            return chain;
        }
        let mut queue = vec![start];
        chain.insert(start);
        while let Some(c) = queue.pop() {
            for p in ends(c) {
                for u in self.doc.users_of(p) {
                    if !chain.contains(&u) && !self.is_locked(u) && ends(u).contains(&p) {
                        chain.insert(u);
                        queue.push(u);
                    }
                }
            }
        }
        chain
    }

    /// Tab over a curve: highlight its chain, to be picked by a click (Revit's Tab); Tab
    /// again goes back to the one curve. Returns the chain's size, 0 for the one curve, or
    /// `None` if the pointer isn't on a curve.
    pub fn preview_chain(&mut self) -> Option<usize> {
        let h = self
            .hovered
            .filter(|h| self.doc.get(*h).is_some_and(|e| !e.kind.is_point()))?;
        if self
            .chain_preview
            .as_ref()
            .is_some_and(|(_, set)| set.contains(&h))
        {
            self.chain_preview = None;
            return Some(0);
        }
        let mut chain = self.chain_of(h);
        if let Some(g) = self.open_group.and_then(|g| self.doc.group(g)) {
            chain.retain(|id| g.members.contains(id));
        }
        if chain.len() < 2 {
            self.chain_preview = None;
            return Some(0);
        }
        let n = chain.len();
        self.chain_preview = Some((h, chain));
        Some(n)
    }

    /// Tab with nothing under the pointer: widen the selection to whole chains. Returns how
    /// many curves are selected.
    pub fn select_chain(&mut self) -> usize {
        let seeds: Vec<EntityId> = match self.hovered {
            Some(h) if self.doc.get(h).is_some_and(|e| !e.kind.is_point()) => vec![h],
            _ => self.selection.iter().copied().collect(),
        };
        let mut chain = BTreeSet::new();
        for s in seeds {
            chain.extend(self.chain_of(s));
        }
        if chain.is_empty() {
            return 0;
        }
        // Within an open group, stay inside it.
        if let Some(g) = self.open_group.and_then(|g| self.doc.group(g)) {
            chain.retain(|id| g.members.contains(id));
        }
        self.selected_fills.clear();
        self.selected_region = None;
        self.selection = chain;
        self.expand_groups();
        self.selection.len()
    }

    /// While a group is open for editing, what's drawn joins it: curves and standalone points
    /// created in the open transaction (a curve's own points ride along with it).
    /// Drawing construction geometry: the curves this transaction added become construction.
    pub(super) fn make_new_curves_construction(&mut self) {
        if !(self.draw_construction && self.drawing_curves) {
            return;
        }
        let generated = self.doc.generated_entities();
        for id in self.doc.created_in_transaction() {
            if !generated.contains(&id)
                && self
                    .doc
                    .get(id)
                    .is_some_and(|e| !e.kind.is_point() && e.style == LineKind::Solid)
            {
                self.doc.set_style(id, LineKind::Construction);
            }
        }
    }

    pub(super) fn adopt_into_open_group(&mut self) {
        let Some(g) = self.open_group else { return };
        if self.doc.group(g).is_none() {
            self.open_group = None;
            return;
        }
        let created = self.doc.created_in_transaction();
        // (Walls' faces belong to their walls, which belong through their centrelines.)
        let generated = self.doc.generated_entities();
        let new: Vec<EntityId> = created
            .into_iter()
            .filter(|id| !generated.contains(id))
            .filter(|id| {
                self.doc
                    .get(*id)
                    .is_some_and(|e| !e.kind.is_point() || self.doc.users_of(*id).next().is_none())
            })
            .collect();
        if !new.is_empty() {
            let _ = self.doc.add_group_members(g, new);
        }
    }

    /// What picking `id` selects: its whole group, unless that group is open (or the filter
    /// leaves out Blocks & groups: then the one curve).
    pub fn pick_set(&self, id: EntityId) -> BTreeSet<EntityId> {
        // An opening's symbol or jamb picks the opening.
        if let Some(o) = self.opening_of(id) {
            let set = self.opening_pick_set(o);
            if !set.is_empty() {
                return set;
            }
        }
        // A wall's face, corner or poché picks the wall: its centreline.
        if self.doc.has_walls()
            && self.wall_owners().generated(id)
            && let Some(w) = self.wall_of(id)
        {
            let set = self.wall_pick_set(w);
            if !set.is_empty() {
                return set;
            }
        }
        let set: BTreeSet<EntityId> = match self.doc.group_of(id) {
            Some(g) if self.open_group != Some(g) && self.pick_filter.blocks() => self
                .doc
                .group(g)
                .map_or_else(|| [id].into(), |g| g.members.clone()),
            _ => [id].into(),
        };
        // With a filter on, a group gives only what the filter lets through.
        set.into_iter()
            .filter(|e| *e == id || self.pick_filter.allows(&self.doc, *e))
            .collect()
    }

    /// Widen the selection to whole groups (except the open one).
    pub fn expand_groups(&mut self) {
        // (Walls' faces picked by a box are their walls.)
        if self.doc.has_walls() {
            let owners = self.wall_owners();
            let faces: Vec<EntityId> = self
                .selection
                .iter()
                .copied()
                .filter(|id| owners.generated(*id))
                .collect();
            for f in &faces {
                self.selection.remove(f);
            }
            let walls = self.walls_for_picks(faces);
            self.selection.extend(walls);
        }
        let extra: Vec<EntityId> = self
            .selection
            .iter()
            .flat_map(|id| self.pick_set(*id))
            .collect();
        self.selection.extend(extra);
        self.sync_group_fills();
    }

    /// The group (not the open one) a shape's fill goes with: the one its whole outline is
    /// in.
    pub fn fill_group(&self, fill: drift_core::FillId) -> Option<drift_core::GroupId> {
        let outline = self.shape_outline(fill)?;
        let g = self.doc.group_of(*outline.first()?)?;
        (self.open_group != Some(g) && outline.iter().all(|c| self.doc.group_of(*c) == Some(g)))
            .then_some(g)
    }

    /// A group picked whole picks its shapes' fills too (so a move, Duplicate, Copy or
    /// Delete takes them with it), and one no longer picked lets them go.
    pub fn sync_group_fills(&mut self) {
        let grouped: Vec<(drift_core::FillId, bool)> = self
            .doc
            .fills()
            .filter_map(|(f, _)| {
                self.fill_group(f)?;
                let outline = self.shape_outline(f)?;
                Some((f, outline.iter().all(|c| self.selection.contains(c))))
            })
            .collect();
        for (f, picked) in grouped {
            if picked && self.pick_filter.fills() {
                self.selected_fills.insert(f);
            } else {
                self.selected_fills.remove(&f);
            }
        }
    }

    /// Pick a shape's fill alone (its outline let go): its hatch's handles show without the
    /// transform box's.
    pub fn pick_fill_alone(&mut self, fill: drift_core::FillId) {
        self.fill_alone_picks = self.fill_alone_picks.saturating_add(1);
        self.selected_fills = [fill].into();
        self.selection.clear();
        self.selected_midpoints.clear();
        self.selected_region = None;
        self.selected_instances.clear();
        self.selected_texts.clear();
    }

    /// The fill of the shape picked whole (its outline, with or without its fill), when its
    /// fill can be picked on its own from here: a double-click inside, Alt-click or the
    /// menu's Edit fill. None inside a closed group (a double-click goes into the group
    /// first) and for anything else picked.
    pub fn picked_shape_fill(&self) -> Option<drift_core::FillId> {
        let first = *self.selection.first()?;
        if self.selected_fills.len() > 1
            || !self.selected_instances.is_empty()
            || !self.selected_texts.is_empty()
        {
            return None;
        }
        let fill = match self.selected_fills.first() {
            Some(f) => *f,
            None => {
                let path = self.doc.path_of(first)?;
                self.doc
                    .fills()
                    .find(|(_, f)| f.path == Some(path))
                    .map(|(id, _)| id)?
            }
        };
        let outline = self.shape_outline(fill)?;
        (outline.len() == self.selection.len()
            && outline.iter().all(|c| self.selection.contains(c))
            && self.fill_group(fill).is_none())
        .then_some(fill)
    }

    /// The group the selection is exactly, if any.
    pub fn selected_group(&self) -> Option<drift_core::GroupId> {
        let first = *self.selection.first()?;
        let g = self.doc.group_of(first)?;
        (self.doc.group(g)?.members == self.selection).then_some(g)
    }

    /// Group the selection (Ctrl/⌘+G). Curves' own points aren't members: the curves carry
    /// them.
    pub fn make_group(&mut self) -> Option<drift_core::GroupId> {
        let referenced = self.doc.defining_points(
            self.selection
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        let members: BTreeSet<EntityId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
        if members.len() < 2 {
            self.notice = Some("Select two or more things to group".into());
            return None;
        }
        let n = self.doc.groups().count() + 1;
        self.doc.begin();
        let id = self
            .doc
            .add_group(drift_core::Group {
                name: format!("Group {n}"),
                members,
            })
            .ok();
        self.commit("Group");
        self.open_group = None;
        self.expand_groups();
        id
    }

    /// Dissolve the groups the selection belongs to (Ctrl/⌘+Shift+G).
    pub fn ungroup(&mut self) {
        let groups: BTreeSet<drift_core::GroupId> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.group_of(*id))
            .collect();
        if groups.is_empty() {
            return;
        }
        self.doc.begin();
        for g in groups {
            self.doc.remove_group(g);
        }
        self.commit("Ungroup");
        self.open_group = None;
    }

    /// Toggle construction style on the selection: if any selected curve is not construction,
    /// all become construction; otherwise all become solid.
    pub fn toggle_construction(&mut self) {
        let curves: Vec<_> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        if curves.is_empty() || !self.guard_owned(&curves, crate::walls::Op::Construction) {
            return;
        }
        let make_construction = curves.iter().any(|id| {
            self.doc
                .get(*id)
                .is_some_and(|e| e.style != LineKind::Construction)
        });
        let style = if make_construction {
            LineKind::Construction
        } else {
            LineKind::Solid
        };
        self.doc.begin();
        for id in curves {
            self.doc.set_style(id, style);
        }
        self.commit("Toggle construction");
    }

    /// Select everything the filter lets through: every curve and standalone point, and (with
    /// their kinds on) the copies of blocks, sketch text and fills. Nothing hidden or locked.
    pub fn select_all(&mut self) {
        let filter = self.pick_filter;
        let shown = |doc: &drift_core::Document, layer: drift_core::LayerId| {
            doc.layer(layer).is_some_and(|l| l.visible && !l.locked)
        };
        self.selection = self
            .doc
            .iter()
            .filter(|(id, e)| {
                (!e.kind.is_point() || self.index.role(*id) == Some(PointRole::Standalone))
                    && filter.allows(&self.doc, *id)
                    && shown(&self.doc, e.layer)
            })
            .map(|(id, _)| id)
            .collect();
        self.selected_midpoints.clear();
        self.selected_region = None;
        if filter.blocks() {
            self.selected_instances = self
                .doc
                .instances()
                .filter(|(_, i)| shown(&self.doc, i.layer))
                .map(|(id, _)| id)
                .collect();
        }
        if filter.texts() {
            self.selected_texts = self
                .doc
                .texts()
                .filter(|(_, t)| shown(&self.doc, t.layer))
                .map(|(id, _)| id)
                .collect();
        }
        // Walls: by their centrelines (their faces aren't picked on their own).
        if self.doc.has_walls() {
            let owners = self.wall_owners();
            self.selection.retain(|id| !owners.generated(*id));
            let curves: Vec<EntityId> = self
                .doc
                .walls()
                .filter(|(_, w)| shown(&self.doc, w.layer))
                .flat_map(|(id, _)| self.wall_curves(id))
                .collect();
            self.selection.extend(curves);
        }
        if filter.fills() {
            let owners = self.wall_owners();
            self.selected_fills = self
                .doc
                .fills()
                .filter(|(id, _)| !owners.fills.contains_key(id))
                .filter(|(_, f)| shown(&self.doc, f.layer))
                .map(|(id, _)| id)
                .collect();
        }
    }

    pub fn bounds(&self) -> Bounds {
        let mut b: Bounds = self
            .doc
            .iter()
            .filter_map(|(id, _)| self.index.shape(id))
            .map(Shape::bounds)
            .collect();
        for (id, _) in self.doc.instances() {
            let i = self.instance_bounds(id);
            if !i.is_empty() {
                b.include(i.min);
                b.include(i.max);
            }
        }
        b
    }

    /// Points whose markers show when end points are only shown in use: those of hovered or
    /// picked curves, and hovered, picked or snapped-to points.
    pub fn touched_points(&self) -> BTreeSet<EntityId> {
        let mut out = BTreeSet::new();
        let active = self
            .selection
            .iter()
            .chain(self.hovered.iter())
            .chain(self.highlight.iter())
            .chain(self.snapped_point.iter());
        for &id in active {
            if let Some(e) = self.doc.get(id) {
                if e.kind.is_point() {
                    out.insert(id);
                } else {
                    out.extend(e.kind.points());
                }
            }
        }
        out
    }

    /// Render items for the document, with selection and hover emphasis.
    /// Render items with constraint-status colouring (see [`items_styled`](Self::items_styled)).
    pub fn items(&self) -> Vec<SketchItem> {
        self.items_styled(true, true)
    }

    /// True if the entity is on a locked layer (it can't be selected or modified).
    pub fn is_locked(&self, id: EntityId) -> bool {
        self.doc.layer_of(id).locked
    }

    /// Refuse to move curves of which some are on a locked layer (a picked area or hatch
    /// whose outline is locked): says so, and true if it refused.
    pub fn refuse_locked(&mut self, ids: &[EntityId]) -> bool {
        let locked = ids.iter().any(|id| self.is_locked(*id));
        if locked {
            self.notice =
                Some("Its outline is on a locked layer: unlock the layer to move it".into());
        }
        locked
    }

    /// Moving a picked area or hatch with automatic relations off: its outline lets go of
    /// curves that aren't moving with it (each point it shares with them is split, the
    /// outline taking its own copy), so they stay put instead of stretching. With automatic
    /// relations on, the joins hold. Inside the caller's transaction.
    pub fn detach_for_move(&mut self, ids: &[EntityId]) {
        if self.auto_relations {
            return;
        }
        let moving: BTreeSet<EntityId> = ids.iter().copied().collect();
        let mut split = false;
        for p in self.doc.defining_points(ids.iter().copied()) {
            let users: Vec<EntityId> = self.doc.users_of(p).collect();
            if users.iter().all(|u| moving.contains(u)) {
                continue;
            }
            let Ok(at) = self.doc.point(p) else {
                continue;
            };
            let copy = self.doc.add_point(at);
            for c in users.into_iter().filter(|u| moving.contains(u)) {
                if let Some(mut kind) = self.doc.get(c).map(|e| e.kind) {
                    kind.replace_point(p, copy);
                    let _ = self.doc.set_entity_kind(c, kind);
                }
            }
            split = true;
        }
        if split {
            self.index.update(&self.doc);
            // The points that let go are new: the scope of a drag from before is stale.
            *self.drag_scope.borrow_mut() = None;
        }
    }
}
