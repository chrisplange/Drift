//! The sketch's items for the canvas, kept from frame to frame.
//!
//! Styling every entity and sorting it into its draw group each frame cost tens of
//! milliseconds on big imported drawings. The lists are made once for the drawing as it is,
//! with what is picked lit up but nothing hovered ("at rest"), and kept until something they
//! depend on changes. Each frame, only what hovering changes is patched in: the few entities
//! hovered, highlighted or snapped to (and their end points), which take their places in the
//! lists or join the lit-up items on top, in the order a full rebuild would give them. The
//! patches are undone once the frame is built.

use std::collections::{BTreeSet, HashMap};

use drift_core::{Emphasis, EntityId, InstanceId, LineKind, LineType, SketchItem};

use crate::editor::{Editor, TaggedItem};
use crate::stack::Stack;

/// Where an item goes among the lit-up ones: entities in draw order, then copies' items.
type Order = (u8, usize, usize);

/// What the lists were made for.
#[derive(Clone)]
struct Key {
    revision: u64,
    structure: u64,
    lineage: u64,
    /// Where each copy of a block was (copies moving don't change the structure).
    copies: Vec<(InstanceId, drift_core::Instance)>,
    analyzed: Option<u64>,
    status_colors: bool,
    dark: bool,
    /// The stack the lists are grouped by (none: one list).
    stack: Option<usize>,
    groups: usize,
    selection: BTreeSet<EntityId>,
    active: Option<EntityId>,
    selected_instances: BTreeSet<InstanceId>,
    hidden: BTreeSet<EntityId>,
    always_endpoints: bool,
    editing: Option<InstanceId>,
}

/// The sketch's items at rest, grouped for drawing (see the [module](self)).
#[derive(Default)]
pub struct ItemsCache {
    key: Option<Key>,
    /// One list per stack group, then the top list (the only one without a stack).
    lists: Vec<Vec<SketchItem>>,
    /// Each entity's place: its list and index there, and its place in draw order (sorted
    /// by entity, to look up the few that hovering changes).
    slots: Vec<(EntityId, (usize, usize, usize))>,
    /// Each copy's items' places, and its place among copies.
    copy_slots: HashMap<InstanceId, (Vec<(usize, usize)>, usize)>,
    /// The lit-up items at rest (what is picked), in order.
    lit: Vec<(Order, SketchItem)>,
    /// Items patched this frame, as they were.
    patched: Vec<((usize, usize), SketchItem)>,
    /// The lists' lengths before this frame added to them.
    lengths: Vec<usize>,
    /// How many times the items of what moved were made again in place (tests).
    remade: usize,
    /// The stack the lists are grouped by, as it was when they were made (a stack is made
    /// again with each revision, most often the same).
    stack: Option<Stack>,
}

/// A slot whose entity doesn't show (at rest, or this frame): an item that draws nothing.
fn blank() -> SketchItem {
    let mut item = SketchItem::new(drift_core::Shape::Point(drift_core::DVec2::ZERO));
    item.kind = LineKind::Solid;
    item.line_type = LineType::None;
    item
}

impl ItemsCache {
    /// Bring the lists up to date for this frame, patch in what hovering changes, and return
    /// the lit-up items (drawn over everything). The lists are then [`lists`](Self::lists)
    /// until [`restore`](Self::restore).
    pub fn prepare(
        &mut self,
        ed: &Editor,
        status_colors: bool,
        dark: bool,
        stack: Option<&Stack>,
    ) -> Vec<SketchItem> {
        // What the lists are made from, compared where it is (cloned only when the lists are
        // made again): with tens of thousands of curves picked, copying the picks every
        // frame cost more than the rest of this.
        let mut hidden = ed.hidden_handles();
        hidden.extend(ed.preview_hidden.iter().copied());
        #[cfg(debug_assertions)]
        assert_eq!(
            &hidden,
            ed.item_look(status_colors, dark, true).hidden(),
            "the lists' hidden set is the look's"
        );
        let stack_at = stack.map(|s| std::ptr::from_ref(s) as usize);
        let groups = stack.map_or(0, |s| s.groups.len());
        let editing = ed.block_edit.map(|e| e.instance);
        // All but the revision and the stack as the lists were made for.
        let same_but_revision = |k: &Key| {
            k.analyzed == ed.analyzed()
                && k.status_colors == status_colors
                && k.dark == dark
                && k.groups == groups
                && k.active == ed.active()
                && k.always_endpoints == ed.prefs.always_endpoints
                && k.editing == editing
                && k.selection == ed.selection
                && k.selected_instances == ed.selected_instances
                && k.hidden == hidden
        };
        // (At the same revision the editor hands out the same stack: the one at that
        // address.)
        let current = self.key.as_ref().is_some_and(|k| {
            k.revision == ed.doc.revision() && k.stack == stack_at && same_but_revision(k)
        });
        if !current {
            let rest = ed.item_look(status_colors, dark, true);
            // Only geometry moved since (a drag): only the items of what moved are made
            // again, in their places.
            let moved = self
                .key
                .as_ref()
                .filter(|k| {
                    k.structure == ed.doc.structure()
                        && k.lineage == ed.doc.lineage()
                        && same_but_revision(k)
                        // A stack made again for this revision that groups as the lists'
                        // did, judged by what it holds: a new one can come at the old one's
                        // address.
                        && stack == self.stack.as_ref()
                        && ed
                            .doc
                            .instances()
                            .map(|(c, i)| (c, *i))
                            .eq(k.copies.iter().copied())
                })
                .and_then(|k| ed.doc.changes_since(k.lineage, k.revision))
                .filter(|ch| !ch.layers && ch.entities.len() <= ed.doc.len() / 8);
            let remade = moved.is_some_and(|ch| self.remake(ed, &rest, stack, &ch.entities));
            self.remade += usize::from(remade);
            if !remade {
                self.build(ed, &rest, stack);
                self.stack = stack.cloned();
            }
            self.key = Some(Key {
                revision: ed.doc.revision(),
                structure: ed.doc.structure(),
                lineage: ed.doc.lineage(),
                copies: ed.doc.instances().map(|(c, i)| (c, *i)).collect(),
                analyzed: ed.analyzed(),
                status_colors,
                dark,
                stack: stack_at,
                groups,
                selection: ed.selection.clone(),
                active: ed.active(),
                selected_instances: ed.selected_instances.clone(),
                hidden,
                always_endpoints: ed.prefs.always_endpoints,
                editing,
            });
        }
        self.lengths = self.lists.iter().map(Vec::len).collect();

        // What hovering changes: hovered, highlighted and snapped-to entities, a chain
        // being previewed, a hovered wall's parts, and all their points (which show
        // when their curve is lit up).
        // (The look with hovering, made only when something is hovered or lit: it copies the
        // picks.)
        let mut now = None;
        let mut lit_now: Vec<(Order, SketchItem)> = Vec::new();
        let mut moving: BTreeSet<EntityId> = BTreeSet::new();
        moving.extend(ed.hovered);
        moving.extend(ed.highlight.iter().copied());
        moving.extend(ed.snapped_point);
        moving.extend(ed.reach_centre());
        if let Some((_, chain)) = &ed.chain_preview {
            moving.extend(chain.iter().copied());
        }
        if let (_, Some(wall)) = ed.lit_walls() {
            let owners = ed.wall_owners();
            moving.extend(
                owners
                    .of
                    .iter()
                    .filter(|(_, o)| matches!(o, drift_core::Owner::Wall(w) if *w == wall))
                    .map(|(id, _)| *id),
            );
        }
        if let Some(x) = ed.hovered_offset() {
            moving.extend(ed.offset_curves(x));
        }
        let points = ed.points_of(moving.iter().copied());
        moving.extend(points);
        for id in moving {
            let Ok(at) = self.slots.binary_search_by_key(&id, |(e, _)| *e) else {
                continue;
            };
            let (list, index, order) = self.slots[at].1;
            let Some(entity) = ed.doc.get(id) else {
                continue;
            };
            let look = now.get_or_insert_with(|| ed.item_look(status_colors, dark, false));
            let item = ed.entity_item(look, id, entity, &mut None);
            let in_list = match item {
                Some(TaggedItem { item, .. }) if item.emphasis == Emphasis::Normal => item,
                _ => blank(),
            };
            if let Some(TaggedItem { item, .. }) = item
                && item.emphasis == Emphasis::Hovered
            {
                lit_now.push(((0, order, 0), item));
            }
            let slot = &mut self.lists[list][index];
            if *slot != in_list {
                self.patched.push(((list, index), *slot));
                *slot = in_list;
            }
        }
        if let Some(copy) = ed.hovered_instance
            && !ed.selected_instances.contains(&copy)
            && let Some((places, order)) = self.copy_slots.get(&copy)
        {
            let look = now.get_or_insert_with(|| ed.item_look(status_colors, dark, false));
            for (k, t) in ed.instance_tagged(look, copy).into_iter().enumerate() {
                lit_now.push(((1, *order, k), t.item));
            }
            for &(list, index) in places {
                let slot = &mut self.lists[list][index];
                self.patched.push(((list, index), *slot));
                *slot = blank();
            }
        }
        // Lit up at rest and now, merged in order.
        lit_now.sort_by_key(|(order, _)| *order);
        let mut lit = Vec::with_capacity(self.lit.len() + lit_now.len());
        let (mut a, mut b) = (self.lit.iter().peekable(), lit_now.iter().peekable());
        loop {
            let next = match (a.peek(), b.peek()) {
                (Some(x), Some(y)) if y.0 < x.0 => b.next(),
                (Some(_), _) => a.next(),
                (None, Some(_)) => b.next(),
                (None, None) => break,
            };
            lit.extend(next.map(|(_, item)| *item));
        }
        lit
    }

    /// The lists: one per stack group, then the top list.
    pub fn lists(&self) -> &[Vec<SketchItem>] {
        &self.lists
    }

    /// Add an item to a list for this frame (text, which isn't kept).
    pub fn push(&mut self, list: usize, item: SketchItem) {
        self.lists[list].push(item);
    }

    /// The top list's index.
    pub fn top(&self) -> usize {
        self.lists.len() - 1
    }

    /// Undo this frame's patches and additions.
    pub fn restore(&mut self) {
        for ((list, index), item) in self.patched.drain(..).rev() {
            self.lists[list][index] = item;
        }
        for (list, len) in self.lists.iter_mut().zip(&self.lengths) {
            list.truncate(*len);
        }
    }

    /// Make again, in their places, the items of `changed` and of the curves on changed
    /// points: what a [`build`](Self::build) would give when only geometry changed since
    /// the lists were made. False (the lists then half made) if something would go
    /// elsewhere: an item that shows or hides, lights up or goes out, or changes list.
    fn remake(
        &mut self,
        ed: &Editor,
        rest: &crate::editor::ItemLook,
        stack: Option<&Stack>,
        changed: &[EntityId],
    ) -> bool {
        let groups = stack.map_or(0, |s| s.groups.len());
        let mut ids: BTreeSet<EntityId> = changed.iter().copied().collect();
        for &id in changed {
            ids.extend(ed.index.users_of(id));
        }
        let mut layer_of = None;
        for id in ids {
            let entity = ed.doc.get(id);
            let item = entity.and_then(|e| ed.entity_item_any_ends(rest, id, e, &mut layer_of));
            let Ok(at) = self.slots.binary_search_by_key(&id, |(e, _)| *e) else {
                if item.is_some() {
                    return false;
                }
                continue;
            };
            let (Some(entity), Some(t)) = (entity, item) else {
                return false;
            };
            let (list, index, order) = self.slots[at].1;
            let list_now = stack
                .and_then(|s| s.group_of(t.entity, t.layer))
                .filter(|k| *k < groups)
                .unwrap_or(groups);
            let lit = self.lit.binary_search_by_key(&(0, order, 0), |(o, _)| *o);
            if list_now != list {
                return false;
            }
            if ed.end_hidden(rest, id, entity) {
                if lit.is_ok() {
                    return false;
                }
                self.lists[list][index] = blank();
            } else if t.item.emphasis == Emphasis::Normal {
                if lit.is_ok() {
                    return false;
                }
                self.lists[list][index] = t.item;
            } else {
                let Ok(k) = lit else {
                    return false;
                };
                self.lists[list][index] = blank();
                self.lit[k].1 = t.item;
            }
        }
        true
    }

    fn build(&mut self, ed: &Editor, rest: &crate::editor::ItemLook, stack: Option<&Stack>) {
        let groups = stack.map_or(0, |s| s.groups.len());
        self.lists = vec![Vec::new(); groups + 1];
        self.slots.clear();
        self.copy_slots.clear();
        self.lit.clear();
        let top = groups;
        let list_of = |t: &TaggedItem| {
            stack
                .and_then(|s| s.group_of(t.entity, t.layer))
                .filter(|k| *k < groups)
                .unwrap_or(top)
        };
        let mut layer_of = None;
        let entities = ed.in_draw_order();
        self.slots.reserve(entities.len());
        for (order, (id, entity)) in entities.into_iter().enumerate() {
            // A place for its item whether or not it shows at rest (an end point shows
            // when its curve is hovered), blank where it doesn't.
            let Some(t) = ed.entity_item_any_ends(rest, id, entity, &mut layer_of) else {
                continue;
            };
            let list = list_of(&t);
            self.slots.push((id, (list, self.lists[list].len(), order)));
            if ed.end_hidden(rest, id, entity) {
                self.lists[list].push(blank());
            } else if t.item.emphasis == Emphasis::Normal {
                self.lists[list].push(t.item);
            } else {
                self.lists[list].push(blank());
                self.lit.push(((0, order, 0), t.item));
            }
        }
        for (order, (copy, _)) in ed.doc.instances().enumerate() {
            let mut places = Vec::new();
            for (k, t) in ed.instance_tagged(rest, copy).into_iter().enumerate() {
                if t.item.emphasis == Emphasis::Normal {
                    let list = list_of(&t);
                    places.push((list, self.lists[list].len()));
                    self.lists[list].push(t.item);
                } else {
                    self.lit.push(((1, order, k), t.item));
                }
            }
            self.copy_slots.insert(copy, (places, order));
        }
        self.slots.sort_unstable_by_key(|(id, _)| *id);
    }
}

#[cfg(test)]
mod tests {
    use drift_core::blocks::{Block, BlockItem, Instance};
    use drift_core::{DVec2, Shape};

    use super::*;

    /// What gets drawn, list by list: items that draw nothing left out.
    fn drawn(items: &[SketchItem]) -> Vec<SketchItem> {
        items
            .iter()
            .filter(|i| {
                !(i.line_type == LineType::None
                    && i.kind == LineKind::Solid
                    && i.emphasis == Emphasis::Normal
                    && i.dof != drift_core::DofState::Over)
            })
            .copied()
            .collect()
    }

    /// The lists as the canvas made them every frame before they were kept.
    fn as_before(ed: &Editor, stack: Option<&Stack>) -> Vec<Vec<SketchItem>> {
        let groups = stack.map_or(0, |s| s.groups.len());
        let mut lists = vec![Vec::new(); groups + 1];
        for t in ed.items_tagged(true, false) {
            let at = (t.item.emphasis == Emphasis::Normal)
                .then(|| stack.and_then(|s| s.group_of(t.entity, t.layer)))
                .flatten()
                .filter(|k| *k < groups);
            lists[at.unwrap_or(groups)].push(t.item);
        }
        lists.iter().map(|l| drawn(l)).collect()
    }

    fn kept(cache: &mut ItemsCache, ed: &Editor, stack: Option<&Stack>) -> Vec<Vec<SketchItem>> {
        let lit = cache.prepare(ed, true, false, stack);
        let mut lists: Vec<Vec<SketchItem>> = cache.lists().iter().map(|l| drawn(l)).collect();
        lists.last_mut().unwrap().extend(drawn(&lit));
        cache.restore();
        lists
    }

    fn check(ed: &mut Editor, cache: &mut ItemsCache, what: &str) {
        for stacked in [false, true] {
            let stack = stacked.then(|| ed.stack());
            assert!(
                kept(cache, ed, stack.as_deref()) == as_before(ed, stack.as_deref()),
                "{what} (stacked: {stacked})"
            );
        }
    }

    #[test]
    fn kept_items_draw_as_a_full_rebuild_does() {
        let (mut doc, _) = crate::test_sketches::torture_sketch();
        let block = doc.add_block(Block {
            name: "Desk".into(),
            items: vec![BlockItem {
                shape: Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::new(40.0, 10.0),
                },
                style: LineKind::Solid,
                appearance: Default::default(),
            }],
            ..Default::default()
        });
        let copies: Vec<InstanceId> = (0..3)
            .map(|k| {
                doc.add_instance(Instance::new(block, DVec2::new(-200.0, k as f64 * 60.0)))
                    .unwrap()
            })
            .collect();
        let mut ed = Editor::new(doc);
        ed.sync();
        let curves: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let points: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let mut cache = ItemsCache::default();
        check(&mut ed, &mut cache, "at rest");
        for k in 0..20 {
            ed.hovered = Some(curves[k * 37 % curves.len()]);
            check(&mut ed, &mut cache, "a curve hovered");
            ed.hovered = Some(points[k * 53 % points.len()]);
            check(&mut ed, &mut cache, "a point hovered");
        }
        ed.hovered = None;
        ed.highlight = curves.iter().step_by(101).copied().collect();
        ed.snapped_point = Some(points[7]);
        check(&mut ed, &mut cache, "highlight and a snap");
        ed.chain_preview = Some((curves[3], curves[3..9].iter().copied().collect()));
        check(&mut ed, &mut cache, "a chain previewed");
        ed.selection = curves.iter().step_by(13).copied().collect();
        ed.hovered = Some(curves[13]);
        check(&mut ed, &mut cache, "picked and hovered");
        ed.hovered = Some(curves[14]);
        check(&mut ed, &mut cache, "picked, another hovered");
        // The active element is drawn apart (#333): another one, the selection the same,
        // lights at once (not only when something else makes the lists again).
        ed.set_active(Some(curves[26]));
        check(&mut ed, &mut cache, "an active element");
        let _ = kept(&mut cache, &ed, None);
        ed.set_active(Some(curves[39]));
        assert!(
            kept(&mut cache, &ed, None) == as_before(&ed, None),
            "another active element"
        );
        ed.hovered_instance = Some(copies[1]);
        check(&mut ed, &mut cache, "a copy hovered");
        ed.selected_instances = [copies[0]].into_iter().collect();
        ed.hovered_instance = Some(copies[0]);
        check(&mut ed, &mut cache, "a picked copy hovered");
        ed.prefs.always_endpoints = true;
        check(&mut ed, &mut cache, "end points always shown");
        // An edit: the lists are made again.
        ed.doc.begin();
        ed.doc
            .set_point(points[0], DVec2::new(-50.0, -50.0))
            .unwrap();
        ed.doc.commit("Move");
        ed.sync();
        check(&mut ed, &mut cache, "after an edit");
    }

    #[test]
    fn items_made_again_where_things_moved_draw_as_a_full_rebuild_does() {
        // Random steps on the torture sketch with a hidden and a locked layer: points moved
        // inside a move (some, or many), picks changed mid-move, hovering, a move ended or
        // undone, a layer shown or hidden. After each, the kept lists draw as a full rebuild.
        let (doc, _) = crate::test_sketches::torture_sketch();
        let mut ed = Editor::new(doc);
        ed.sync();
        let curves: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let points: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let hidden = ed.doc.add_layer(drift_core::Layer::new("Hidden"));
        let locked = ed.doc.add_layer(drift_core::Layer::new("Locked"));
        ed.doc.begin();
        for (k, &c) in curves.iter().enumerate() {
            match k % 7 {
                1 => ed.doc.set_entity_layer(c, hidden).unwrap(),
                2 => ed.doc.set_entity_layer(c, locked).unwrap(),
                _ => {}
            }
        }
        for (id, visible, locked_now) in [(hidden, false, false), (locked, true, true)] {
            let mut layer = ed.doc.layer(id).unwrap().clone();
            layer.visible = visible;
            layer.locked = locked_now;
            ed.doc.set_layer(id, layer);
        }
        ed.doc.commit("Layers");
        ed.sync();
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut rnd = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let mut cache = ItemsCache::default();
        for step in 0..250 {
            let what = match rnd(20) {
                0..=9 => "points moved",
                10 | 11 => "picks changed",
                12 | 13 => "hovered",
                14 => "move ended",
                15 => "move undone",
                16 => "a layer shown or hidden",
                _ => "many points moved",
            };
            match what {
                "points moved" | "many points moved" => {
                    if !ed.doc.in_transaction() {
                        ed.doc.begin();
                    }
                    let n = if what == "points moved" {
                        1 + rnd(15)
                    } else {
                        points.len() / 12
                    };
                    let d = DVec2::new(rnd(9) as f64 - 4.0, rnd(9) as f64 - 4.0) * 0.5;
                    let mut moved = Vec::new();
                    for _ in 0..n {
                        let p = points[rnd(points.len())];
                        if let Ok(at) = ed.doc.point(p) {
                            ed.doc.set_point(p, at + d).unwrap();
                            moved.push(p);
                        }
                    }
                    // (As a drag tells the index: the curves on the points too.)
                    ed.index.update_moved_with_curves(&ed.doc, moved);
                }
                "picks changed" => {
                    let (nc, np) = (rnd(60), rnd(4));
                    let mut picks = BTreeSet::new();
                    for _ in 0..nc {
                        picks.insert(curves[rnd(curves.len())]);
                    }
                    for _ in 0..np {
                        picks.insert(points[rnd(points.len())]);
                    }
                    ed.selection = picks;
                }
                "hovered" => {
                    ed.hovered = (rnd(3) > 0).then(|| curves[rnd(curves.len())]);
                }
                "move ended" => {
                    if ed.doc.in_transaction() {
                        ed.doc.commit("Move");
                        ed.sync();
                    }
                }
                "move undone" => {
                    if ed.doc.in_transaction() {
                        ed.doc.rollback();
                        ed.sync();
                    }
                }
                _ => {
                    if ed.doc.in_transaction() {
                        ed.doc.commit("Move");
                    }
                    let mut layer = ed.doc.layer(hidden).unwrap().clone();
                    layer.visible = !layer.visible;
                    ed.doc.begin();
                    ed.doc.set_layer(hidden, layer);
                    ed.doc.commit("Layer");
                    ed.sync();
                }
            }
            // (Stacked in every other run of 25 steps: a stack made again each revision.)
            let stack = ((step / 25) % 2 == 1).then(|| ed.stack());
            assert!(
                kept(&mut cache, &ed, stack.as_deref()) == as_before(&ed, stack.as_deref()),
                "step {step}: {what}"
            );
        }
        assert!(
            cache.remade > 30,
            "made again in place {} times",
            cache.remade
        );
    }

    #[test]
    fn items_regrouped_by_a_stack_made_again_draw_as_a_full_rebuild_does() {
        // Inside a move only a point moves, but the stack made again for it draws another
        // curve in another group (a fill's outline found otherwise): the lists follow it.
        // Three squares, two of them filled: a group for each fill and the layer's top.
        let mut doc = drift_core::Document::new();
        for k in 0..3 {
            let x = k as f64 * 30.0;
            let p = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]
                .map(|(a, b)| doc.add_point(DVec2::new(x + a, b)));
            for i in 0..4 {
                doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
            }
        }
        let mut ed = Editor::new(doc);
        ed.sync();
        ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        ed.add_fill_at(DVec2::new(35.0, 5.0)).unwrap();
        ed.settle_areas();
        ed.sync();
        let mut cache = ItemsCache::default();
        let stack = ed.stack();
        assert!(kept(&mut cache, &ed, Some(&stack)) == as_before(&ed, Some(&stack)));
        let (curve, p) = ed
            .doc
            .iter()
            .find_map(|(id, e)| match e.kind {
                drift_core::EntityKind::Line { a, .. } => Some((id, a)),
                _ => None,
            })
            .unwrap();
        ed.doc.begin();
        let at = ed.doc.point(p).unwrap();
        ed.doc.set_point(p, at + DVec2::new(0.5, 0.0)).unwrap();
        ed.index.update_moved_with_curves(&ed.doc, [p]);
        let mut regrouped = (*ed.stack()).clone();
        // (A curve that didn't move: only the stack says it draws in another of its groups.)
        let _ = curve;
        assert!(regrouped.groups.len() >= 2);
        let other = ed
            .doc
            .iter()
            .find(|(id, e)| {
                !e.kind.is_point() && *id != curve && !ed.index.users_of(p).any(|u| u == *id)
            })
            .map(|(id, _)| id)
            .unwrap();
        let to = usize::from(
            ed.stack()
                .group_of(Some(other), ed.doc.get(other).unwrap().layer)
                == Some(0),
        );
        regrouped.curve_group.insert(other, to);
        assert!(
            kept(&mut cache, &ed, Some(&regrouped)) == as_before(&ed, Some(&regrouped)),
            "regrouped"
        );
        let same = (*ed.stack()).clone();
        assert!(
            kept(&mut cache, &ed, Some(&same)) == as_before(&ed, Some(&same)),
            "back"
        );
    }

    #[test]
    fn a_stack_made_again_at_the_old_ones_address_is_judged_by_what_it_holds() {
        // Inside a move a point moves and the stack made again for the new revision comes
        // at the old one's address (as an allocator can hand it back), grouping an unmoved
        // curve elsewhere: the lists follow what the stack holds, not where it is.
        let mut doc = drift_core::Document::new();
        for k in 0..3 {
            let x = k as f64 * 30.0;
            let p = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]
                .map(|(a, b)| doc.add_point(DVec2::new(x + a, b)));
            for i in 0..4 {
                doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
            }
        }
        let mut ed = Editor::new(doc);
        ed.sync();
        ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        ed.add_fill_at(DVec2::new(35.0, 5.0)).unwrap();
        ed.settle_areas();
        ed.sync();
        let mut cache = ItemsCache::default();
        // One stack value, kept at one address throughout.
        let mut stack = (*ed.stack()).clone();
        assert!(stack.groups.len() >= 2);
        assert!(kept(&mut cache, &ed, Some(&stack)) == as_before(&ed, Some(&stack)));
        let (p, other) = {
            let mut lines = ed.doc.iter().filter(|(_, e)| !e.kind.is_point());
            let (first, e) = lines.next().unwrap();
            let p = e.kind.points().next().unwrap();
            let other = lines
                .map(|(id, _)| id)
                .find(|id| *id != first && !ed.index.users_of(p).any(|u| u == *id))
                .unwrap();
            (p, other)
        };
        ed.doc.begin();
        let at = ed.doc.point(p).unwrap();
        ed.doc.set_point(p, at + DVec2::new(0.5, 0.0)).unwrap();
        ed.index.update_moved_with_curves(&ed.doc, [p]);
        let layer = ed.doc.get(other).unwrap().layer;
        let to = usize::from(stack.group_of(Some(other), layer) == Some(0));
        stack.curve_group.insert(other, to);
        assert!(
            kept(&mut cache, &ed, Some(&stack)) == as_before(&ed, Some(&stack)),
            "regrouped at the same address"
        );
    }

    #[test]
    fn kept_items_light_up_a_hovered_wall_as_a_full_rebuild_does() {
        let (doc, _) = crate::perf_tests::wall_floor();
        let mut ed = Editor::new(doc);
        ed.sync();
        let owners = ed.wall_owners();
        let mut walled: Vec<EntityId> = owners
            .of
            .iter()
            .filter(|(_, o)| matches!(o, drift_core::Owner::Wall(_)))
            .map(|(id, _)| *id)
            .collect();
        walled.sort_unstable();
        let mut cache = ItemsCache::default();
        check(&mut ed, &mut cache, "walls at rest");
        for k in 0..5 {
            ed.hovered = Some(walled[k * 97 % walled.len()]);
            check(&mut ed, &mut cache, "a wall hovered");
        }
        ed.selection = [walled[5]].into_iter().collect();
        check(&mut ed, &mut cache, "a wall picked");
    }
}
