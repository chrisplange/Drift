//! Blocks in the editor: picking and placing copies, making a block from the selection,
//! exploding copies, editing a block in place, and adding blocks from the library.

use std::collections::{BTreeMap, BTreeSet};

use drift_core::{
    Block, BlockId, BlockItem, Bounds, DVec2, EntityId, Fill, FillId, GroupId, Instance,
    InstanceId, Shape,
};

use crate::editor::Editor;

/// A block being edited in place: a copy turned into ordinary geometry (in a group that's
/// open, so whatever is drawn joins it) until editing finishes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockEdit {
    pub block: BlockId,
    pub instance: InstanceId,
    pub group: GroupId,
}

/// What F2 renames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenameTarget {
    Block(BlockId),
    Group(GroupId),
}

/// What the Base point tool's click picks the base point of.
#[derive(Clone, Debug, PartialEq)]
pub enum BasePointPick {
    /// The block of this copy: its base point moves there, and every copy stays put.
    Block(InstanceId),
    /// A block made of the selection, with this name.
    Make(String),
}

/// A block-coordinate shape or fill moved by `by` (the block's own coordinates shifted).
fn shifted(by: DVec2) -> Instance {
    Instance::new(BlockId(0), by)
}

impl Editor {
    /// A copy's extent in the world (worked out for every copy at once, and kept while the
    /// document's structure is unchanged: hover and snapping ask for them all each move).
    pub fn instance_bounds(&self, id: InstanceId) -> Bounds {
        let structure = self.doc.structure();
        if let Some((s, all)) = &*self.instance_bounds_cache.borrow()
            && *s == structure
        {
            return all.get(&id).copied().unwrap_or(Bounds::EMPTY);
        }
        let all: std::collections::HashMap<InstanceId, Bounds> = self
            .doc
            .instances()
            .map(|(i, _)| {
                let mut b = Bounds::EMPTY;
                for item in self.doc.instance_items(i) {
                    let s = item.shape.bounds();
                    b.include(s.min);
                    b.include(s.max);
                }
                (i, b)
            })
            .collect();
        let b = all.get(&id).copied().unwrap_or(Bounds::EMPTY);
        *self.instance_bounds_cache.borrow_mut() = Some((structure, std::rc::Rc::new(all)));
        b
    }

    /// The nearest snap point of a copy of a block within `tol` of `p`: its box's corners,
    /// edge middles and centre, its insertion point, and its curves' ends, middles and
    /// centres. Copies `exclude` names are passed over, and so is any copy whose box is
    /// further than `tol` from `p`.
    pub fn block_snap(
        &self,
        p: DVec2,
        tol: f64,
        exclude: impl Fn(InstanceId) -> bool,
    ) -> Option<drift_geom::Snap> {
        use drift_geom::{Snap, SnapKind};
        let editing = self.block_edit.map(|e| e.instance);
        let mut best: Option<(Snap, f64)> = None;
        let mut offer = |at: DVec2, kind: SnapKind| {
            let d = at.distance(p);
            if d <= tol
                && best
                    .as_ref()
                    .is_none_or(|(b, bd)| (kind.priority(), d) < (b.kind.priority(), *bd))
            {
                best = Some((
                    Snap {
                        at,
                        kind,
                        entity: None,
                    },
                    d,
                ));
            }
        };
        for (id, copy) in self.doc.instances() {
            if Some(id) == editing
                || exclude(id)
                || !self.instance_visible(copy)
                || !self.instance_snappable(copy)
            {
                continue;
            }
            let b = self.instance_bounds(id);
            if b.is_empty()
                || p.x < b.min.x - tol
                || p.x > b.max.x + tol
                || p.y < b.min.y - tol
                || p.y > b.max.y + tol
            {
                continue;
            }
            // The box: corners, the middles of its sides, its centre.
            let (lo, hi, c) = (b.min, b.max, b.center());
            for corner in [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)] {
                offer(corner, SnapKind::Endpoint);
            }
            for mid in [
                DVec2::new(c.x, lo.y),
                DVec2::new(hi.x, c.y),
                DVec2::new(c.x, hi.y),
                DVec2::new(lo.x, c.y),
            ] {
                offer(mid, SnapKind::Midpoint);
            }
            offer(c, SnapKind::Center);
            offer(copy.at, SnapKind::Point);
            // Its own geometry.
            for item in self.doc.instance_items(id) {
                match item.shape {
                    Shape::Line { a, b } => {
                        offer(a, SnapKind::Endpoint);
                        offer(b, SnapKind::Endpoint);
                        offer((a + b) * 0.5, SnapKind::Midpoint);
                    }
                    Shape::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => {
                        offer(center, SnapKind::Center);
                        offer(
                            center + DVec2::from_angle(start) * radius,
                            SnapKind::Endpoint,
                        );
                        offer(
                            center + DVec2::from_angle(start + sweep) * radius,
                            SnapKind::Endpoint,
                        );
                    }
                    Shape::Circle { center, .. } => offer(center, SnapKind::Center),
                    Shape::Bezier { p: q } => {
                        offer(q[0], SnapKind::Endpoint);
                        offer(q[3], SnapKind::Endpoint);
                    }
                    Shape::Point(q) => offer(q, SnapKind::Point),
                    _ => {}
                }
            }
        }
        best.map(|(s, _)| s)
    }

    fn instance_snappable(&self, i: &Instance) -> bool {
        self.doc.layer(i.layer).is_none_or(|l| l.snappable)
    }

    fn instance_visible(&self, i: &Instance) -> bool {
        self.doc.layer(i.layer).is_none_or(|l| l.visible)
    }

    /// The copy under `p`: one of its curves within `tol`, or else the smallest copy whose
    /// outline contains `p`. Locked layers and the copy being edited are skipped.
    pub fn instance_at(&self, p: DVec2, tol: f64) -> Option<InstanceId> {
        let editing = self.block_edit.map(|e| e.instance);
        let candidates: Vec<(InstanceId, Instance)> = self
            .doc
            .instances()
            .filter(|(id, i)| {
                Some(*id) != editing
                    && self.instance_visible(i)
                    && self.doc.layer(i.layer).is_none_or(|l| !l.locked)
            })
            .map(|(id, i)| (id, *i))
            .collect();
        // (A copy's curves are looked at only when `p` is near its box.)
        let near_box = |id: InstanceId| {
            let b = self.instance_bounds(id);
            !b.is_empty()
                && p.x >= b.min.x - tol
                && p.x <= b.max.x + tol
                && p.y >= b.min.y - tol
                && p.y <= b.max.y + tol
        };
        let on_edge = candidates.iter().rev().find(|(id, _)| {
            near_box(*id)
                && self
                    .doc
                    .instance_items(*id)
                    .iter()
                    .any(|item| drift_geom::distance(&item.shape, p) <= tol)
        });
        if let Some((id, _)) = on_edge {
            return Some(*id);
        }
        candidates
            .iter()
            .map(|(id, _)| (*id, self.instance_bounds(*id)))
            .filter(|(_, b)| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
            .min_by(|a, b| {
                let area = |b: &Bounds| (b.max - b.min).x * (b.max - b.min).y;
                area(&a.1).total_cmp(&area(&b.1))
            })
            .map(|(id, _)| id)
    }

    /// Copies inside a selection box (all of a copy inside it, or any part for a crossing box).
    pub fn instances_in(&self, rect: &Bounds, crossing: bool) -> Vec<InstanceId> {
        self.doc
            .instances()
            .filter(|(_, i)| self.instance_visible(i))
            .map(|(id, _)| id)
            .filter(|id| {
                let b = self.instance_bounds(*id);
                if crossing {
                    b.intersects(rect)
                } else {
                    b.min.x >= rect.min.x
                        && b.min.y >= rect.min.y
                        && b.max.x <= rect.max.x
                        && b.max.y <= rect.max.y
                }
            })
            .collect()
    }

    /// Make a block from the selection and put a copy where the geometry was (the selection
    /// becomes that copy). The base point is the centre of the selection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn make_block(&mut self, name: &str) -> Result<InstanceId, String> {
        self.make_block_at(name, None)
    }

    /// Whether [`make_block_at`](Self::make_block_at) would work: something is picked and the
    /// name is free.
    pub fn check_make_block(&self, name: &str) -> Result<(), String> {
        self.block_source(name).map(|_| ())
    }

    /// The curves a block made now would hold, their extent, and its name.
    fn block_source(&self, name: &str) -> Result<(Vec<EntityId>, Bounds, String), String> {
        let referenced = self.doc.defining_points(
            self.selection
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        // Walls go in exploded: their faces (and poché), not their centrelines.
        let walls = self.selected_walls();
        let centrelines: std::collections::BTreeSet<EntityId> =
            walls.iter().flat_map(|w| self.wall_curves(*w)).collect();
        let mut ids: Vec<EntityId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| !referenced.contains(id) && !centrelines.contains(id))
            .collect();
        for w in &walls {
            if let Some(wall) = self.doc.wall(*w) {
                ids.extend(wall.made.curves.values().copied());
            }
        }
        let mut bounds = Bounds::EMPTY;
        for id in &ids {
            if let Some(s) = self.doc.shape(*id) {
                let b = s.bounds();
                bounds.include(b.min);
                bounds.include(b.max);
            }
        }
        if ids.is_empty() || bounds.is_empty() {
            return Err("Select the geometry to make into a block".into());
        }
        let name = name.trim();
        let name = if name.is_empty() {
            format!("Block {}", self.doc.blocks().count() + 1)
        } else {
            name.to_owned()
        };
        if self.doc.block_named(&name).is_some() {
            return Err(format!("There is already a block called {name}"));
        }
        Ok((ids, bounds, name))
    }

    /// Make a block from the selection, with its base point at `base` (a world point), or
    /// the centre of the selection.
    pub fn make_block_at(&mut self, name: &str, base: Option<DVec2>) -> Result<InstanceId, String> {
        self.make_block_labelled(name, base, "Make block")
    }

    fn make_block_labelled(
        &mut self,
        name: &str,
        base: Option<DVec2>,
        label: &str,
    ) -> Result<InstanceId, String> {
        let (ids, bounds, name) = self.block_source(name)?;
        let centrelines: Vec<EntityId> = self
            .selected_walls()
            .into_iter()
            .flat_map(|w| self.wall_curves(w))
            .collect();
        let base = base.unwrap_or_else(|| bounds.center());
        let place = Instance::new(BlockId(0), base);
        let items: Vec<BlockItem> = ids
            .iter()
            .filter_map(|id| {
                let e = self.doc.get(*id)?;
                let shape = self.doc.shape(*id)?;
                Some(BlockItem {
                    shape: place.invert_shape(&shape),
                    style: e.style,
                    appearance: e.appearance,
                })
            })
            .collect();
        let layer = self.doc.get(ids[0]).map(|e| e.layer).unwrap_or_default();
        let same_layer = ids
            .iter()
            .all(|id| self.doc.get(*id).is_some_and(|e| e.layer == layer));
        let (fills, filled) = self.capture_fills(&ids.iter().copied().collect(), &place);
        self.doc.begin();
        let block = self.doc.add_block(Block {
            name,
            items,
            fills,
            profile: false,
        });
        for f in filled {
            self.doc.remove_fill(f);
        }
        self.doc.delete(ids);
        // (The walls go with their centrelines.)
        self.doc.delete(centrelines);
        let id = self
            .doc
            .add_instance(Instance::new(block, base))
            .map_err(|e| e.to_string())?;
        if same_layer && let Some(mut i) = self.doc.instance(id).copied() {
            i.layer = layer;
            let _ = self.doc.set_instance(id, i);
        }
        self.commit(label);
        self.selection.clear();
        self.selected_instances = [id].into();
        Ok(id)
    }

    /// `name`, or `name 2`, `name 3`… if a block already has it.
    fn free_block_name(&self, name: &str) -> String {
        let name = name.trim();
        (1..)
            .map(|n| {
                if n == 1 {
                    name.to_owned()
                } else {
                    format!("{name} {n}")
                }
            })
            .find(|n| self.doc.block_named(n).is_none())
            .expect("a free name")
    }

    /// Make a block from a group (one undo step): named after the group, with its base point
    /// at the group's centre, and its fills. A copy takes the group's place, picked.
    pub fn block_from_group(&mut self, group: GroupId) -> Result<InstanceId, String> {
        if self.block_edit.is_some() {
            return Err("Finish editing the block first".into());
        }
        let g = self.doc.group(group).cloned().ok_or("That group is gone")?;
        let name = self.free_block_name(&g.name);
        let (selection, open) = (self.selection.clone(), self.open_group);
        self.selection = g.members;
        self.open_group = None;
        let made = self.make_block_labelled(&name, None, "Make block from group");
        if made.is_err() {
            (self.selection, self.open_group) = (selection, open);
        }
        made
    }

    /// The group the picks are, if they are all of one group (the picked group, or the
    /// one being edited).
    pub fn convertible_group(&self) -> Option<GroupId> {
        self.selected_group().or_else(|| {
            let g = self.open_group?;
            (self.selection.is_empty() && self.block_edit.is_none()).then_some(g)
        })
    }

    /// Turn the picked block copies back into geometry, each copy a group named after its
    /// block (one undo step). The groups' members are picked afterwards.
    pub fn explode_to_groups(&mut self) -> Vec<GroupId> {
        let copies: Vec<InstanceId> = self.selected_instances.iter().copied().collect();
        if copies.is_empty() {
            return Vec::new();
        }
        self.doc.begin();
        let mut groups = Vec::new();
        let mut picked = BTreeSet::new();
        for id in copies {
            let Some(name) = self
                .doc
                .instance(id)
                .and_then(|i| self.doc.block(i.block))
                .map(|b| b.name.clone())
            else {
                continue;
            };
            let created = self.add_copy_geometry(id);
            self.doc.remove_instance(id);
            let referenced = self.doc.defining_points(
                created
                    .iter()
                    .copied()
                    .filter(|e| self.doc.get(*e).is_some_and(|e| !e.kind.is_point())),
            );
            let members: BTreeSet<EntityId> = created
                .into_iter()
                .filter(|e| !referenced.contains(e))
                .collect();
            if let Ok(g) = self.doc.add_group(drift_core::Group {
                name,
                members: members.clone(),
            }) {
                groups.push(g);
            }
            picked.extend(members);
        }
        self.commit("Explode to group");
        self.selected_instances.clear();
        self.open_group = None;
        self.selection = picked;
        groups
    }

    /// Move the base point of `instance`'s block to `world` (a point on the drawing, where
    /// that copy is): the block's curves and fills shift in its own coordinates, and every
    /// copy's insertion point moves so that nothing moves on the drawing. One undo step.
    pub fn set_base_point(&mut self, instance: InstanceId, world: DVec2) -> Result<(), String> {
        let copy = self
            .doc
            .instance(instance)
            .copied()
            .ok_or("That copy is gone")?;
        let mut block = self
            .doc
            .block(copy.block)
            .cloned()
            .ok_or("That block is gone")?;
        // The new base point in the block's coordinates.
        let q = copy.invert(world);
        if q.length() <= drift_core::tolerance::same(world.abs().max_element()) {
            return Ok(());
        }
        let shift = shifted(-q);
        for item in &mut block.items {
            item.shape = shift.apply_shape(&item.shape);
        }
        for fill in &mut block.fills {
            let layer = fill.layer;
            *fill = shift.apply_fill(fill);
            fill.layer = layer;
        }
        let copies: Vec<(InstanceId, Instance)> = self
            .doc
            .instances()
            .filter(|(_, i)| i.block == copy.block)
            .map(|(id, i)| (id, *i))
            .collect();
        self.doc.begin();
        self.doc.set_block(copy.block, block);
        for (id, i) in copies {
            let at = i.apply(q);
            let _ = self.doc.set_instance(id, Instance { at, ..i });
        }
        self.commit("Set base point");
        Ok(())
    }

    /// The middle of `instance`'s block, on the drawing (where Set base point's Enter puts
    /// it).
    pub fn block_centre(&self, instance: InstanceId) -> Option<DVec2> {
        let copy = self.doc.instance(instance)?;
        let b = self.doc.block(copy.block)?.bounds();
        (!b.is_empty()).then(|| copy.apply(b.center()))
    }

    /// Where the base point shows as a cross: on the copy being edited in place, or the copy
    /// whose base point is being picked.
    pub fn base_point_cross(&self, picking: bool) -> Option<DVec2> {
        let copy = match (&self.base_point_pick, self.block_edit) {
            (Some(BasePointPick::Block(id)), _) if picking => *id,
            (_, Some(edit)) => edit.instance,
            _ => return None,
        };
        self.doc.instance(copy).map(|i| i.at)
    }

    /// Place a copy of a block (one undo step), picking it.
    pub fn place_instance(&mut self, block: BlockId, at: DVec2, angle: f64, mirror: bool) {
        let mut i = Instance::new(block, at);
        i.angle = angle;
        i.mirror = mirror;
        self.doc.begin();
        match self.doc.add_instance(i) {
            Ok(id) => {
                self.commit("Place block");
                self.selection.clear();
                self.selected_instances = [id].into();
            }
            Err(e) => {
                self.doc.rollback();
                self.out.notice = Some(e.to_string());
            }
        }
    }

    /// Turn a copy's curves into entities (inside the caller's transaction). Ends that meet
    /// share a point, so outlines stay connected.
    fn add_copy_geometry(&mut self, id: InstanceId) -> Vec<EntityId> {
        let Some(layer) = self.doc.instance(id).map(|i| i.layer) else {
            return Vec::new();
        };
        let mut points: Vec<(DVec2, EntityId)> = Vec::new();
        let mut point = |doc: &mut drift_core::Document, p: DVec2| -> EntityId {
            let tol = drift_core::tolerance::same(p.abs().max_element());
            if let Some((_, id)) = points.iter().find(|(q, _)| q.distance(p) <= tol) {
                return *id;
            }
            let id = doc.add_point(p);
            points.push((p, id));
            id
        };
        let mut created = Vec::new();
        let items = self.doc.instance_items(id);
        let mut made_from = vec![None; items.len()];
        for (k, item) in items.iter().enumerate() {
            let made = match item.shape {
                Shape::Point(p) => Some(point(&mut self.doc, p)),
                Shape::Line { a, b } => {
                    let (pa, pb) = (point(&mut self.doc, a), point(&mut self.doc, b));
                    self.doc.add_line(pa, pb).ok()
                }
                Shape::Circle { center, radius } => {
                    let c = point(&mut self.doc, center);
                    self.doc.add_circle(c, radius).ok()
                }
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    let c = point(&mut self.doc, center);
                    let s = point(&mut self.doc, center + DVec2::from_angle(start) * radius);
                    let e = point(
                        &mut self.doc,
                        center + DVec2::from_angle(start + sweep) * radius,
                    );
                    self.doc.add_arc(c, s, e).ok()
                }
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    start,
                    sweep,
                } => {
                    let (axes, ends) =
                        drift_core::shape::ellipse_handles(center, major, minor, start, sweep);
                    let [c, m, n] = axes.map(|q| point(&mut self.doc, q));
                    match ends {
                        Some([s, e]) => {
                            let (s, e) = (point(&mut self.doc, s), point(&mut self.doc, e));
                            self.doc.add_ellipse_arc(c, m, n, s, e).ok()
                        }
                        None => self.doc.add_ellipse(c, m, n).ok(),
                    }
                }
                Shape::Bezier { p } => {
                    let [a, c1, c2, b] = p.map(|q| point(&mut self.doc, q));
                    self.doc.add_bezier(a, c1, c2, b).ok()
                }
            };
            if let Some(e) = made {
                let _ = self.doc.set_entity_layer(e, layer);
                self.doc.set_style(e, item.style);
                let _ = self.doc.set_appearance(e, item.appearance);
                created.push(e);
                made_from[k] = Some(e);
            }
        }
        self.add_copy_fills(id, &items, &made_from);
        created
    }

    /// Fill what a copy's fills fill, now that its curves are entities (`made_from[k]` is
    /// the entity the copy's `k`th curve became).
    fn add_copy_fills(
        &mut self,
        id: InstanceId,
        items: &[BlockItem],
        made_from: &[Option<EntityId>],
    ) {
        let Some(copy) = self.doc.instance(id).copied() else {
            return;
        };
        let fills = self
            .doc
            .block(copy.block)
            .map(|b| b.fills.clone())
            .unwrap_or_default();
        if fills.is_empty() {
            return;
        }
        let shapes: Vec<Shape> = items.iter().map(|item| item.shape).collect();
        let regions = drift_geom::regions::regions(&shapes);
        for f in &fills {
            let mut fill = copy.apply_fill(f);
            let Some(region) = drift_geom::regions::region_at(&regions, fill.seed) else {
                continue;
            };
            let mut boundary: Vec<EntityId> = region
                .outer
                .curves()
                .into_iter()
                .filter_map(|i| made_from.get(i).copied().flatten())
                .collect();
            boundary.sort_unstable();
            boundary.dedup();
            if boundary.is_empty() {
                continue;
            }
            fill.boundary = boundary;
            // A shape's fill (the block keeps its old path id as a mark) fills the shape the
            // curves make again, as a path of their own.
            fill.path = None;
            if f.path.is_some() {
                let mut curves: Vec<EntityId> = Vec::new();
                for e in region
                    .outer
                    .curves()
                    .into_iter()
                    .filter_map(|i| made_from.get(i).copied().flatten())
                {
                    if !curves.contains(&e) {
                        curves.push(e);
                    }
                }
                fill.path = self
                    .doc
                    .add_path(drift_core::Path {
                        curves,
                        closed: true,
                    })
                    .ok();
            }
            let fid = self.doc.add_fill(fill.clone());
            // On the copy's layer, over what is there.
            if let Some(mut added) = self.doc.fill(fid).cloned() {
                added.layer = copy.layer;
                self.doc.set_fill(fid, added);
            }
        }
    }

    /// The fills of areas that `curves` enclose (each whose outline is all among them), in
    /// block coordinates for a copy placed as `place`, bottom first; and those fills.
    fn capture_fills(
        &mut self,
        curves: &BTreeSet<EntityId>,
        place: &Instance,
    ) -> (Vec<Fill>, Vec<FillId>) {
        let shapes = self.fill_shapes();
        let mut fills = Vec::new();
        let mut ids = Vec::new();
        for id in self.fills_in_order() {
            let (Some(fill), Some(shape)) = (self.doc.fill(id), shapes.get(&id)) else {
                continue;
            };
            if shape.lost
                || fill.boundary.is_empty()
                || !fill.boundary.iter().all(|e| curves.contains(e))
            {
                continue;
            }
            let mut f = fill.clone();
            f.seed = crate::editor::interior_point(&shape.region, fill.seed);
            fills.push(place.invert_fill(&f));
            ids.push(id);
        }
        (fills, ids)
    }

    /// Replace the picked copies with ordinary geometry (picked afterwards).
    pub fn explode_selected(&mut self) {
        let copies: Vec<InstanceId> = self.selected_instances.iter().copied().collect();
        if copies.is_empty() {
            // (Picked walls explode into plain geometry.)
            let walls: Vec<drift_core::WallId> = self.selected_walls().into_iter().collect();
            self.explode_walls(&walls);
            return;
        }
        self.doc.begin();
        let mut created = Vec::new();
        for id in copies {
            created.extend(self.add_copy_geometry(id));
            self.doc.remove_instance(id);
        }
        self.commit("Explode");
        self.selected_instances.clear();
        self.selection = created.into_iter().collect();
    }

    /// Start editing a copy's block in place: the copy becomes geometry in an open group;
    /// what's drawn joins it. [`finish_block_edit`](Self::finish_block_edit) writes it back.
    pub fn edit_block(&mut self, instance: InstanceId) {
        if self.block_edit.is_some() {
            self.finish_block_edit();
        }
        let Some(block) = self.doc.instance(instance).map(|i| i.block) else {
            return;
        };
        let name = self
            .doc
            .block(block)
            .map_or_else(String::new, |b| b.name.clone());
        self.doc.begin();
        let created = self.add_copy_geometry(instance);
        let group = self.doc.add_group(drift_core::Group {
            name: format!("Editing {name}"),
            members: created.iter().copied().collect(),
        });
        self.commit("Edit block");
        match group {
            Ok(group) => {
                self.block_edit = Some(BlockEdit {
                    block,
                    instance,
                    group,
                });
                self.open_group = Some(group);
                self.selected_instances.clear();
                self.selection.clear();
            }
            Err(e) => self.out.notice = Some(e.to_string()),
        }
    }

    /// The curves of a block being edited in place, as they are now, in block coordinates.
    fn edited_items(&self, edit: &BlockEdit) -> Option<Vec<BlockItem>> {
        let group = self.doc.group(edit.group)?;
        let copy = self.doc.instance(edit.instance)?;
        let referenced = self.doc.defining_points(
            group
                .members
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        Some(
            group
                .members
                .iter()
                .filter(|id| !referenced.contains(id))
                .filter_map(|id| {
                    let e = self.doc.get(*id)?;
                    let shape = self.doc.shape(*id)?;
                    Some(BlockItem {
                        shape: copy.invert_shape(&shape),
                        style: e.style,
                        appearance: e.appearance,
                    })
                })
                .collect(),
        )
    }

    /// The drawing's blocks, with a block being edited in place as it is now: what the
    /// Blender profiles file is written from.
    pub fn blocks_as_drawn(&self) -> Vec<(BlockId, Block)> {
        self.doc
            .blocks()
            .map(|(id, b)| {
                let mut b = b.clone();
                if let Some(edit) = self.block_edit.filter(|e| e.block == id)
                    && let Some(items) = self.edited_items(&edit)
                {
                    b.items = items;
                }
                (id, b)
            })
            .collect()
    }

    /// Write the edited geometry back into the block (every copy updates) and remove the
    /// temporary geometry.
    pub fn finish_block_edit(&mut self) {
        let Some(edit) = self.block_edit.take() else {
            return;
        };
        self.open_group = None;
        let (Some(group), Some(copy), Some(mut block), Some(items)) = (
            self.doc.group(edit.group).cloned(),
            self.doc.instance(edit.instance).copied(),
            self.doc.block(edit.block).cloned(),
            self.edited_items(&edit),
        ) else {
            return;
        };
        block.items = items;
        let (fills, filled) = self.capture_fills(&group.members, &copy);
        block.fills = fills;
        self.doc.begin();
        self.doc.set_block(edit.block, block);
        for f in filled {
            self.doc.remove_fill(f);
        }
        self.doc.delete(group.members.iter().copied());
        self.doc.remove_group(edit.group);
        self.commit("Finish editing block");
        self.selection.clear();
        self.selected_instances = [edit.instance].into();
    }

    /// Leave the open group, finishing a block edit if that's what it is.
    pub fn close_open_group(&mut self) {
        if self.block_edit.is_some() {
            self.finish_block_edit();
        } else {
            self.open_group = None;
        }
    }

    /// Change the picked copies' block.
    pub fn replace_block(&mut self, block: BlockId) {
        let copies: Vec<InstanceId> = self.selected_instances.iter().copied().collect();
        self.doc.begin();
        for id in copies {
            if let Some(mut i) = self.doc.instance(id).copied() {
                i.block = block;
                let _ = self.doc.set_instance(id, i);
            }
        }
        self.commit("Replace block");
    }

    /// Pick every copy of a block.
    pub fn select_copies(&mut self, block: BlockId) {
        self.selection.clear();
        self.selected_instances = self
            .doc
            .instances()
            .filter(|(_, i)| i.block == block)
            .map(|(id, _)| id)
            .collect();
    }

    /// Copies of each block.
    pub fn copy_counts(&self) -> BTreeMap<BlockId, usize> {
        let mut counts = BTreeMap::new();
        for (_, i) in self.doc.instances() {
            *counts.entry(i.block).or_default() += 1;
        }
        counts
    }

    /// Set copies to `originals` moved by `offset` (inside an open transaction).
    pub fn move_instances(&mut self, originals: &[(InstanceId, Instance)], offset: DVec2) {
        for (id, i) in originals {
            let _ = self.doc.set_instance(
                *id,
                Instance {
                    at: i.at + offset,
                    ..*i
                },
            );
        }
    }

    /// Set copies to `originals` turned about `pivot` by `degrees`.
    pub fn rotate_instances(
        &mut self,
        originals: &[(InstanceId, Instance)],
        pivot: DVec2,
        degrees: f64,
    ) {
        let rotor = drift_core::blocks::exact_rotor(degrees);
        for (id, i) in originals {
            let _ = self.doc.set_instance(
                *id,
                Instance {
                    at: pivot + rotor.rotate(i.at - pivot),
                    angle: (i.angle + degrees).rem_euclid(360.0),
                    ..*i
                },
            );
        }
    }

    /// The picked copies as they are now.
    pub fn picked_instances(&self) -> Vec<(InstanceId, Instance)> {
        self.selected_instances
            .iter()
            .filter_map(|id| self.doc.instance(*id).map(|i| (*id, *i)))
            .collect()
    }

    /// Add the library's blocks the document doesn't have yet (by name), in one step.
    /// Returns how many.
    pub fn add_library_blocks<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a crate::library::Entry>,
    ) -> usize {
        let missing: Vec<Block> = entries
            .into_iter()
            .filter(|e| self.doc.block_named(&e.block.name).is_none())
            .map(|e| e.block.clone())
            .collect();
        if missing.is_empty() {
            return 0;
        }
        let n = missing.len();
        self.doc.begin();
        for b in missing {
            self.doc.add_block(b);
        }
        self.commit(if n == 1 { "Add block" } else { "Add blocks" });
        n
    }

    /// A library block in the document: the one of that name, or added now.
    pub fn library_block(&mut self, entry: &crate::library::Entry) -> BlockId {
        self.add_library_blocks([entry]);
        self.doc.block_named(&entry.block.name).expect("just added")
    }

    /// Mark a block as a Blender profile, or not (one undo step).
    pub fn set_block_profile(&mut self, id: BlockId, profile: bool) {
        let Some(mut block) = self.doc.block(id).cloned() else {
            return;
        };
        if block.profile == profile {
            return;
        }
        block.profile = profile;
        self.doc.begin();
        self.doc.set_block(id, block);
        self.commit(if profile {
            "Mark as Blender profile"
        } else {
            "Unmark Blender profile"
        });
    }

    /// What F2 renames: the picked group, or the block whose copies are all that's picked.
    pub fn rename_target(&self) -> Option<RenameTarget> {
        if let Some(g) = self.selected_group() {
            return Some(RenameTarget::Group(g));
        }
        if !self.selection.is_empty() || !self.selected_texts.is_empty() {
            return None;
        }
        let mut blocks = self
            .selected_instances
            .iter()
            .filter_map(|i| self.doc.instance(*i).map(|x| x.block));
        let first = blocks.next()?;
        blocks
            .all(|b| b == first)
            .then_some(RenameTarget::Block(first))
    }

    /// The current name of a block or group.
    pub fn name_of(&self, target: RenameTarget) -> Option<String> {
        match target {
            RenameTarget::Block(b) => self.doc.block(b).map(|x| x.name.clone()),
            RenameTarget::Group(g) => self.doc.group(g).map(|x| x.name.clone()),
        }
    }

    /// Rename a block or group, as one undo step. A block's name must be new among blocks;
    /// neither may be blank. Says why when it refuses.
    pub fn rename(&mut self, target: RenameTarget, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A name can't be blank".into());
        }
        if self.name_of(target).ok_or("That's gone")? == name {
            return Ok(());
        }
        match target {
            RenameTarget::Block(id) => {
                if self.doc.block_named(name).is_some_and(|b| b != id) {
                    return Err(format!("Another block is already named {name}"));
                }
                let mut b = self.doc.block(id).cloned().ok_or("That block is gone")?;
                b.name = name.to_owned();
                self.doc.begin();
                self.doc.set_block(id, b);
                self.commit("Rename block");
            }
            RenameTarget::Group(id) => {
                self.doc.begin();
                self.doc.rename_group(id, name);
                self.commit("Rename group");
            }
        }
        Ok(())
    }

    /// Delete the blocks that have no copies (but not one being edited). Returns how many.
    pub fn purge_unused_blocks(&mut self) -> usize {
        let counts = self.copy_counts();
        let unused: Vec<BlockId> = self
            .doc
            .blocks()
            .map(|(id, _)| id)
            .filter(|id| counts.get(id).copied().unwrap_or(0) == 0)
            .filter(|id| self.block_edit.is_none_or(|e| e.block != *id))
            .collect();
        if unused.is_empty() {
            return 0;
        }
        self.doc.begin();
        for id in &unused {
            self.doc.remove_block(*id);
        }
        self.commit("Purge unused blocks");
        if self.place_block.is_some_and(|b| unused.contains(&b)) {
            self.place_block = None;
        }
        unused.len()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use drift_core::Document;

    fn square() -> (Editor, BTreeSet<EntityId>) {
        let mut doc = Document::new();
        let p = [(0.0, 0.0), (400.0, 0.0), (400.0, 400.0), (0.0, 400.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let lines: BTreeSet<EntityId> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        (Editor::new(doc), lines)
    }

    /// #264: copies placed exactly on another copy of the same block (same turn, scale and
    /// mirroring) count as overlapping, the later-made one of each pair; moving one apart
    /// clears it.
    #[test]
    fn stacked_copies_are_overlapping() {
        let (mut ed, lines) = square();
        ed.selection = lines;
        let first = ed.make_block("Table").unwrap();
        let block = ed.doc.instance(first).unwrap().block;
        assert!(ed.overlapping_instances().is_empty());
        let at = DVec2::new(1000.0, 0.0);
        ed.place_instance(block, at, 0.0, false);
        let a = *ed.selected_instances.iter().next().unwrap();
        ed.place_instance(block, at, 0.0, false);
        let b = *ed.selected_instances.iter().next().unwrap();
        // Alike in place only: not stacked.
        ed.place_instance(block, at, 90.0, false);
        ed.place_instance(block, at, 0.0, true);
        ed.place_instance(block, DVec2::new(2000.0, 0.0), 0.0, false);
        assert_eq!(*ed.overlapping_instances(), vec![b]);
        // Found once per change.
        assert!(std::rc::Rc::ptr_eq(
            &ed.overlapping_instances(),
            &ed.overlapping_instances()
        ));
        // Moved apart: no longer stacked.
        let moved = *ed.doc.instance(b).unwrap();
        ed.doc.begin();
        ed.move_instances(&[(b, moved)], DVec2::new(50.0, 0.0));
        ed.commit("Move");
        assert!(ed.overlapping_instances().is_empty());
        ed.undo();
        assert_eq!(*ed.overlapping_instances(), vec![b]);
        let _ = a;
    }

    #[test]
    fn blocks_and_groups_are_renamed_by_what_is_picked() {
        let (mut ed, lines) = square();
        ed.selection = lines;
        let copy = ed.make_block("Table").unwrap();
        let block = ed.doc.instance(copy).unwrap().block;
        ed.place_instance(block, DVec2::new(1000.0, 0.0), 0.0, false);
        // Nothing picked: nothing to rename.
        ed.selection.clear();
        ed.selected_instances.clear();
        assert_eq!(ed.rename_target(), None);
        // Copies of one block: that block.
        ed.selected_instances = ed.doc.instances().map(|(id, _)| id).collect();
        assert_eq!(ed.rename_target(), Some(RenameTarget::Block(block)));
        let target = RenameTarget::Block(block);
        ed.rename(target, "  Desk ").unwrap();
        assert_eq!(ed.name_of(target).as_deref(), Some("Desk"));
        // One undo step brings the old name back.
        ed.undo();
        assert_eq!(ed.name_of(target).as_deref(), Some("Table"));
        ed.redo();
        assert_eq!(ed.name_of(target).as_deref(), Some("Desk"));
        // Blank and taken names are refused, saying why.
        assert!(ed.rename(target, "   ").is_err());
        ed.selection = square_lines(&mut ed);
        ed.selected_instances.clear();
        ed.make_block("Chair").unwrap();
        let err = ed.rename(target, "Chair").unwrap_err();
        assert!(err.contains("Chair"), "{err}");
        assert_eq!(ed.name_of(target).as_deref(), Some("Desk"));
        // A picked group: the group.
        ed.selection = square_lines(&mut ed);
        let g = ed.make_group().unwrap();
        assert_eq!(ed.rename_target(), Some(RenameTarget::Group(g)));
        ed.rename(RenameTarget::Group(g), "Room A").unwrap();
        assert_eq!(ed.doc.group(g).unwrap().name, "Room A");
    }

    /// Another 400 square, drawn as one undo step: its lines.
    fn square_lines(ed: &mut Editor) -> BTreeSet<EntityId> {
        ed.doc.begin();
        let p = [(0.0, 0.0), (400.0, 0.0), (400.0, 400.0), (0.0, 400.0)]
            .map(|(x, y)| ed.doc.add_point(DVec2::new(x + 5000.0, y)));
        let lines = (0..4)
            .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        ed.commit("Square");
        lines
    }

    #[test]
    fn make_place_explode_and_edit_blocks() {
        let (mut ed, lines) = square();
        ed.selection = lines;
        let copy = ed.make_block("Table").unwrap();
        // The geometry became one copy, based at its centre.
        assert_eq!(ed.doc.iter().count(), 0);
        let i = *ed.doc.instance(copy).unwrap();
        assert_eq!(i.at, DVec2::new(200.0, 200.0));
        assert_eq!(ed.instance_at(DVec2::new(400.0, 100.0), 1.0), Some(copy));
        assert_eq!(
            ed.instance_at(DVec2::new(250.0, 250.0), 1.0),
            Some(copy),
            "inside"
        );
        assert!(ed.make_block("Table").is_err(), "names are unique");
        // A second copy, turned and mirrored.
        let b = i.block;
        ed.place_instance(b, DVec2::new(1000.0, 0.0), 90.0, true);
        assert_eq!(ed.copy_counts()[&b], 2);
        // Editing the block in place changes every copy.
        ed.edit_block(copy);
        assert!(ed.open_group.is_some());
        assert_eq!(ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count(), 4);
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let c = ed.doc.add_point(DVec2::new(400.0, 400.0));
        ed.doc.begin();
        let diag = ed.doc.add_line(a, c).unwrap();
        ed.commit("Line");
        assert!(
            ed.doc
                .group(ed.open_group.unwrap())
                .unwrap()
                .members
                .contains(&diag)
        );
        ed.finish_block_edit();
        assert_eq!(ed.doc.iter().count(), 0, "temporary geometry removed");
        assert_eq!(ed.doc.block(b).unwrap().items.len(), 5);
        let second = ed.doc.instances().map(|(id, _)| id).max().unwrap();
        assert_eq!(ed.doc.instance_items(second).len(), 5);
        // Explode the second copy: connected geometry, the copy gone.
        ed.selected_instances = [second].into();
        ed.explode_selected();
        assert_eq!(ed.doc.instances().count(), 1);
        assert_eq!(ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count(), 5);
        assert_eq!(
            ed.doc.iter().filter(|(_, e)| e.kind.is_point()).count(),
            4,
            "shared corners"
        );
        ed.undo();
        assert_eq!(ed.doc.instances().count(), 2);
    }

    #[test]
    fn blocks_keep_their_fills() {
        use drift_core::FillKind;
        let (mut ed, lines) = square();
        if let FillKind::Hatch(h) = &mut ed.prefs.fill_style.kind {
            h.angle = 30.0;
        }
        ed.add_fill_at(DVec2::new(100.0, 100.0)).unwrap();
        ed.selected_fills.clear();
        ed.selection = lines;
        let copy = ed.make_block("Rug").unwrap();
        // The hatch went into the block, about its base point.
        assert_eq!(ed.doc.fills().count(), 0);
        let b = ed.doc.instance(copy).unwrap().block;
        let kept = ed.doc.block(b).unwrap().fills.clone();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].seed, DVec2::new(-100.0, -100.0));
        // Each copy hatches its own square, the pattern turned with it.
        ed.place_instance(b, DVec2::new(1000.0, 0.0), 90.0, false);
        let fills = ed.copy_fills();
        assert_eq!(fills.len(), 2);
        assert!(
            fills
                .iter()
                .all(|c| (c.region.area - 160_000.0).abs() < 1e-6)
        );
        let turned = fills.iter().find(|c| c.instance != copy).unwrap();
        let FillKind::Hatch(h) = &turned.fill.kind else {
            panic!("a hatch");
        };
        assert!((h.angle - 120.0).abs() < 1e-9, "{}", h.angle);
        // The eyedropper picks up a copy's hatch as it shows.
        ed.prefs.fill_style.kind = FillKind::Solid;
        ed.pick_up_fill_style(DVec2::new(1000.0, 10.0)).unwrap();
        assert!(
            matches!(&ed.prefs.fill_style.kind, FillKind::Hatch(h) if (h.angle - 120.0).abs() < 1e-9)
        );
        // Exploding a copy hatches the area of its new curves.
        let turned = turned.instance;
        ed.selected_instances = [copy].into();
        ed.explode_selected();
        let (exploded, f) = ed.doc.fills().next().unwrap();
        assert_eq!(f.boundary.len(), 4);
        assert!(!ed.fill_shapes()[&exploded].lost);
        // Editing in place: the copy's hatch is an ordinary one until editing finishes.
        ed.edit_block(turned);
        assert_eq!(ed.doc.fills().count(), 2);
        assert!(ed.copy_fills().is_empty(), "the copy being edited");
        ed.finish_block_edit();
        assert_eq!(ed.doc.fills().count(), 1);
        assert_eq!(ed.doc.block(b).unwrap().fills.len(), 1);
        assert_eq!(ed.copy_fills().len(), 1);
    }

    /// Every copy's curves on the drawing, by copy.
    fn drawn(ed: &Editor) -> Vec<Vec<Shape>> {
        ed.doc
            .instances()
            .map(|(id, _)| ed.doc.instance_items(id).iter().map(|i| i.shape).collect())
            .collect()
    }

    fn same(a: &[Vec<Shape>], b: &[Vec<Shape>]) -> bool {
        let close = |p: DVec2, q: DVec2| p.distance(q) < 1e-9;
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(s, t)| match (s, t) {
                        (Shape::Line { a, b }, Shape::Line { a: c, b: d }) => {
                            close(*a, *c) && close(*b, *d)
                        }
                        _ => s == t,
                    })
            })
    }

    #[test]
    fn setting_the_base_point_keeps_every_copy_in_place() {
        use drift_core::FillKind;
        let (mut ed, lines) = square();
        ed.add_fill_at(DVec2::new(100.0, 100.0)).unwrap();
        ed.selected_fills.clear();
        ed.selection = lines;
        // Made with its base point at a corner, not the middle.
        let first = ed.make_block_at("Plate", Some(DVec2::ZERO)).unwrap();
        let b = ed.doc.instance(first).unwrap().block;
        assert_eq!(ed.doc.instance(first).unwrap().at, DVec2::ZERO);
        assert_eq!(ed.doc.block(b).unwrap().bounds().min, DVec2::ZERO);
        // A second copy turned, mirrored and scaled.
        ed.place_instance(b, DVec2::new(1000.0, 0.0), 90.0, true);
        let second = ed.doc.instances().map(|(id, _)| id).max().unwrap();
        let mut i = *ed.doc.instance(second).unwrap();
        i.scale = 0.5;
        ed.doc.begin();
        ed.doc.set_instance(second, i).unwrap();
        ed.commit("Scale");
        let before = drawn(&ed);
        let fills_before: Vec<DVec2> = ed
            .copy_fills()
            .iter()
            .map(|c| c.region.outer.polygon(drift_core::tolerance::FLATTEN_MM)[0])
            .collect();
        let steps = ed.history.undo_label().map(str::to_owned);
        // The far corner of the second copy becomes the base point.
        let corner = ed
            .doc
            .instance(second)
            .unwrap()
            .apply(DVec2::new(400.0, 400.0));
        ed.set_base_point(second, corner).unwrap();
        assert_eq!(ed.history.undo_label(), Some("Set base point"));
        // The block's own coordinates moved; the copies' insertion points followed.
        let block = ed.doc.block(b).unwrap();
        assert_eq!(block.bounds().max, DVec2::ZERO);
        assert_eq!(block.fills[0].seed, DVec2::new(-300.0, -300.0));
        assert!(ed.doc.instance(second).unwrap().at.distance(corner) < 1e-9);
        assert!(
            ed.doc
                .instance(first)
                .unwrap()
                .at
                .distance(DVec2::new(400.0, 400.0))
                < 1e-9
        );
        // Nothing moved on the drawing, fills included.
        assert!(
            same(&before, &drawn(&ed)),
            "{before:?}
{:?}",
            drawn(&ed)
        );
        let fills_after: Vec<DVec2> = ed
            .copy_fills()
            .iter()
            .map(|c| c.region.outer.polygon(drift_core::tolerance::FLATTEN_MM)[0])
            .collect();
        assert_eq!(fills_before.len(), fills_after.len());
        assert!(
            fills_before
                .iter()
                .zip(&fills_after)
                .all(|(a, b)| a.distance(*b) < 1e-6)
        );
        assert!(matches!(ed.copy_fills()[0].fill.kind, FillKind::Hatch(_)));
        // One undo step brings the old base point back.
        ed.undo();
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
        assert_eq!(ed.doc.block(b).unwrap().bounds().min, DVec2::ZERO);
        assert_eq!(ed.doc.instance(first).unwrap().at, DVec2::ZERO);
        // The same base point again changes nothing (and adds no step).
        ed.redo();
        ed.set_base_point(first, DVec2::new(400.0, 400.0)).unwrap();
        assert_eq!(ed.history.undo_label(), Some("Set base point"));
        ed.undo();
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
    }

    #[test]
    fn the_base_point_moves_while_a_block_is_edited() {
        let (mut ed, lines) = square();
        ed.selection = lines;
        let copy = ed.make_block("Table").unwrap();
        let b = ed.doc.instance(copy).unwrap().block;
        ed.place_instance(b, DVec2::new(1000.0, 0.0), 90.0, false);
        let before = drawn(&ed);
        ed.edit_block(copy);
        assert_eq!(ed.base_point_cross(false), Some(DVec2::new(200.0, 200.0)));
        ed.set_base_point(copy, DVec2::ZERO).unwrap();
        assert_eq!(ed.base_point_cross(false), Some(DVec2::ZERO));
        ed.finish_block_edit();
        assert_eq!(ed.doc.block(b).unwrap().bounds().min, DVec2::ZERO);
        assert!(same(&before, &drawn(&ed)));
        assert_eq!(ed.base_point_cross(false), None);
    }

    #[test]
    fn library_blocks_are_added_once_and_purged() {
        let lib = crate::library::all();
        let doors: Vec<_> = lib
            .iter()
            .filter(|e| e.category == crate::library::Category::Doors)
            .collect();
        let (mut ed, _) = square();
        assert_eq!(ed.add_library_blocks(doors.iter().copied()), doors.len());
        assert_eq!(ed.add_library_blocks(doors.iter().copied()), 0, "only once");
        let door = ed.library_block(doors[0]);
        assert_eq!(ed.doc.blocks().count(), doors.len(), "already there");
        ed.place_instance(door, DVec2::ZERO, 0.0, false);
        assert_eq!(ed.purge_unused_blocks(), doors.len() - 1);
        assert_eq!(ed.doc.blocks().count(), 1);
        assert_eq!(ed.purge_unused_blocks(), 0, "the placed one stays");
        ed.undo();
        assert_eq!(ed.doc.blocks().count(), doors.len());
    }
}
