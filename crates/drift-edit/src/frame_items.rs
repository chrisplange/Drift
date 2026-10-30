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
#[derive(Clone, PartialEq)]
struct Key {
    revision: u64,
    analyzed: Option<u64>,
    status_colors: bool,
    dark: bool,
    /// The stack the lists are grouped by (none: one list).
    stack: Option<usize>,
    groups: usize,
    selection: BTreeSet<EntityId>,
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
        let rest = ed.item_look(status_colors, dark, true);
        let key = Key {
            revision: ed.doc.revision(),
            analyzed: ed.analyzed(),
            status_colors,
            dark,
            stack: stack.map(|s| std::ptr::from_ref(s) as usize),
            groups: stack.map_or(0, |s| s.groups.len()),
            selection: ed.selection.clone(),
            selected_instances: ed.selected_instances.clone(),
            hidden: rest.hidden().clone(),
            always_endpoints: ed.always_endpoints,
            editing: ed.block_edit.map(|e| e.instance),
        };
        if self.key.as_ref() != Some(&key) {
            self.build(ed, &rest, stack);
            self.key = Some(key);
        }
        self.lengths = self.lists.iter().map(Vec::len).collect();

        // What hovering changes: hovered, highlighted and snapped-to entities, a chain
        // being previewed, a hovered wall's parts, and all their points (which show
        // when their curve is lit up).
        let now = ed.item_look(status_colors, dark, false);
        let mut lit_now: Vec<(Order, SketchItem)> = Vec::new();
        let mut moving: BTreeSet<EntityId> = BTreeSet::new();
        moving.extend(ed.hovered);
        moving.extend(ed.highlight.iter().copied());
        moving.extend(ed.snapped_point);
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
            let item = ed.entity_item(&now, id, entity, &mut None);
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
            for (k, t) in ed.instance_tagged(&now, copy).into_iter().enumerate() {
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
        ed.hovered_instance = Some(copies[1]);
        check(&mut ed, &mut cache, "a copy hovered");
        ed.selected_instances = [copies[0]].into_iter().collect();
        ed.hovered_instance = Some(copies[0]);
        check(&mut ed, &mut cache, "a picked copy hovered");
        ed.always_endpoints = true;
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
