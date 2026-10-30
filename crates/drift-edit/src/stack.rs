//! What draws over what: one stacking order, followed alike by the canvas and by every
//! output (printing, PDF, SVG, DXF).
//!
//! With the drawing's rule "Fills cover what is beneath them" ([`Rules::stacked_fills`]),
//! layers stack in their order, and on each layer its fills, curves and placed pictures
//! stack by their `order` (higher on top; at the same order fills first, then copies'
//! fills, pictures, and curves). A fill draws with the curves of its outline, so it covers
//! everything beneath it and its own outline shows; a curve outlining several fills draws
//! with the topmost. Points, text and copies' curves go at the top of their layer. Pictures
//! of underlays not placed on a layer are beneath the whole drawing.
//!
//! Without the rule, every fill is beneath every curve (a CAD overlay): the canvas and the
//! outputs draw all fills first, then all curves, and this order isn't used.
//!
//! [`Rules::stacked_fills`]: drift_core::Rules

use std::collections::HashMap;
use std::rc::Rc;

use drift_core::{EntityId, FillId, InstanceId, LayerId, UnderlayId};

use crate::editor::Editor;

/// A fill as it is drawn: the sketch's own, or one of a copy of a block (the copy and the
/// fill's place in the block).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FillRef {
    Fill(FillId),
    Copy(InstanceId, usize),
}

/// One step of the stack: a fill (if any) and the curves drawn right over it, or pictures,
/// or a run of loose curves. Each draws over every group before it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackGroup {
    /// The layer it stacks on (`None`: beneath the drawing).
    pub layer: Option<LayerId>,
    pub fill: Option<FillRef>,
    pub pictures: Vec<UnderlayId>,
    pub curves: Vec<EntityId>,
}

/// The drawing's stacking order, bottom first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub groups: Vec<StackGroup>,
    /// The group each curve draws in.
    pub curve_group: HashMap<EntityId, usize>,
    /// Each layer's top group: where its points, text and copies' curves draw.
    pub tops: HashMap<LayerId, usize>,
}

impl Stack {
    /// The group something tagged with `entity` on `layer` draws in: the curve's own, else
    /// the top of the layer.
    pub fn group_of(&self, entity: Option<EntityId>, layer: LayerId) -> Option<usize> {
        entity
            .and_then(|e| self.curve_group.get(&e))
            .or_else(|| self.tops.get(&layer))
            .copied()
    }
}

/// Something that stacks on a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Unit {
    Fill(FillId),
    Copy(InstanceId, usize),
    Picture(UnderlayId),
    Curve(EntityId),
}

impl Unit {
    /// Its place on its layer: by order, then (at the same order) fills, copies' fills,
    /// pictures and curves, then by id.
    fn key(self, order: i64) -> (i64, Unit) {
        (order, self)
    }
}

/// A unit with the key it sorts by on its layer.
type Keyed = ((i64, Unit), Unit);

/// Where a curve without an order of its own stacks: over everything else on its layer
/// (above any order a fill can have).
const TOP: i64 = i32::MAX as i64 + 1;

/// A curve's place on its layer, as an order to sort by.
fn curve_order(order: Option<i32>) -> i64 {
    order.map_or(TOP, i64::from)
}

impl Editor {
    /// The drawing's stacking order (see the [module](self)), kept while the drawing
    /// doesn't change.
    pub fn stack(&mut self) -> Rc<Stack> {
        let revision = self.doc.revision();
        if let Some((r, stack)) = &self.stack
            && *r == revision
        {
            return stack.clone();
        }
        let stack = Rc::new(self.build_stack());
        self.stack = Some((revision, stack.clone()));
        stack
    }

    fn build_stack(&mut self) -> Stack {
        let shapes = self.fill_shapes();
        let copies = self.copy_fills();
        let doc = &self.doc;
        let layers: Vec<LayerId> = doc.layers().map(|(id, _)| id).collect();
        let mut units: HashMap<LayerId, Vec<Keyed>> = HashMap::new();
        let mut put = |layer: LayerId, order: i64, unit: Unit| {
            units
                .entry(layer)
                .or_default()
                .push((unit.key(order), unit));
        };
        for (id, fill) in doc.fills() {
            put(fill.layer, fill.order.into(), Unit::Fill(id));
        }
        for c in copies.iter() {
            put(
                c.fill.layer,
                c.fill.order.into(),
                Unit::Copy(c.instance, c.index),
            );
        }
        let mut beneath = Vec::new();
        for (id, u) in doc.underlays() {
            match u.layer.filter(|l| doc.layer(*l).is_some()) {
                Some(layer) => put(layer, u.order.into(), Unit::Picture(id)),
                None => beneath.push(id),
            }
        }
        // Outline curves draw with their fill, so they aren't stacked on their own.
        let outlines: std::collections::BTreeSet<EntityId> = shapes
            .iter()
            .filter(|(id, _)| doc.fill(**id).is_some())
            .flat_map(|(_, s)| s.curves.iter().copied())
            .collect();
        for (id, e) in doc.iter() {
            if !e.kind.is_point() && !outlines.contains(&id) {
                put(e.layer, curve_order(e.order), Unit::Curve(id));
            }
        }

        let mut stack = Stack::default();
        if !beneath.is_empty() {
            stack.groups.push(StackGroup {
                pictures: beneath,
                ..StackGroup::default()
            });
        }
        // (Things on a layer that no longer exists, if any, go on top.)
        let mut order = layers.clone();
        order.extend(
            units
                .keys()
                .filter(|l| !layers.contains(l))
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
        );
        for layer in order {
            let mut list = units.remove(&layer).unwrap_or_default();
            list.sort_by_key(|(key, _)| *key);
            for (_, unit) in list {
                let loose = |g: &StackGroup| g.fill.is_none() && g.pictures.is_empty();
                match unit {
                    Unit::Fill(id) => stack.groups.push(StackGroup {
                        layer: Some(layer),
                        fill: Some(FillRef::Fill(id)),
                        ..StackGroup::default()
                    }),
                    Unit::Copy(i, k) => stack.groups.push(StackGroup {
                        layer: Some(layer),
                        fill: Some(FillRef::Copy(i, k)),
                        ..StackGroup::default()
                    }),
                    Unit::Picture(id) => stack.groups.push(StackGroup {
                        layer: Some(layer),
                        pictures: vec![id],
                        ..StackGroup::default()
                    }),
                    Unit::Curve(id) => {
                        if !stack
                            .groups
                            .last()
                            .is_some_and(|g| g.layer == Some(layer) && loose(g))
                        {
                            stack.groups.push(StackGroup {
                                layer: Some(layer),
                                ..StackGroup::default()
                            });
                        }
                        stack.curve_group.insert(id, stack.groups.len() - 1);
                        if let Some(g) = stack.groups.last_mut() {
                            g.curves.push(id);
                        }
                    }
                }
            }
            // The layer's top: a run of loose curves, made empty if needed.
            if !stack.groups.last().is_some_and(|g| {
                g.layer == Some(layer) && g.fill.is_none() && g.pictures.is_empty()
            }) {
                stack.groups.push(StackGroup {
                    layer: Some(layer),
                    ..StackGroup::default()
                });
            }
            stack.tops.insert(layer, stack.groups.len() - 1);
        }
        // Each outline curve draws with the topmost fill it outlines.
        for (k, g) in stack.groups.iter().enumerate() {
            if let Some(FillRef::Fill(id)) = g.fill
                && let Some(shape) = shapes.get(&id)
            {
                for c in &shape.curves {
                    stack.curve_group.insert(*c, k);
                }
            }
        }
        let owned: Vec<(EntityId, usize)> = stack
            .curve_group
            .iter()
            .filter(|(c, _)| outlines.contains(c))
            .map(|(c, k)| (*c, *k))
            .collect();
        let mut owned = owned;
        owned.sort();
        for (c, k) in owned {
            stack.groups[k].curves.push(c);
        }
        stack
    }
}

/// How an Arrange command moves what is picked in its layer's stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    /// Over the next thing above it that it overlaps.
    Forward,
    /// Under the next thing beneath it that it overlaps.
    Backward,
    /// Over everything on its layer.
    ToFront,
    /// Under everything on its layer.
    ToBack,
}

impl Arrange {
    pub const ALL: [Arrange; 4] = [
        Arrange::ToFront,
        Arrange::Forward,
        Arrange::Backward,
        Arrange::ToBack,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Arrange::Forward => "Bring forward",
            Arrange::Backward => "Send backward",
            Arrange::ToFront => "Bring to front",
            Arrange::ToBack => "Send to back",
        }
    }

    fn up(self) -> bool {
        matches!(self, Arrange::Forward | Arrange::ToFront)
    }
}

/// A movable thing on a layer, with its order and bounds.
type Placed = (Unit, i64, drift_core::Bounds);

impl Editor {
    /// What an Arrange command would move: the picked fills, the picked curves (a curve of
    /// a fill's outline stands for its fill: the shape moves as one), and the picked
    /// underlay's pictures where they are placed on a layer.
    fn arranged(&mut self) -> Vec<Unit> {
        let stack = self.stack();
        let mut out = std::collections::BTreeSet::new();
        for id in &self.selected_fills {
            if self.doc.fill(*id).is_some() {
                out.insert(Unit::Fill(*id));
            }
        }
        // A picked underlay's pictures, where they are placed on a layer.
        if let Some(id) = self.selected_underlay
            && self.doc.underlay(id).is_some_and(|u| u.layer.is_some())
        {
            out.insert(Unit::Picture(id));
        }
        // A picked wall arranges its cluster's poché (the walls joined to it too).
        let owners = self.wall_owners();
        for id in &self.selection {
            if self.doc.get(*id).is_none_or(|e| e.kind.is_point()) {
                continue;
            }
            if let Some(drift_core::Owner::Centre(w)) = owners.get(*id) {
                if let Some(f) = self.wall_fill(w) {
                    out.insert(Unit::Fill(f));
                }
                continue;
            }
            let of_fill = stack
                .curve_group
                .get(id)
                .and_then(|k| stack.groups[*k].fill);
            out.insert(match of_fill {
                Some(FillRef::Fill(f)) => Unit::Fill(f),
                _ => Unit::Curve(*id),
            });
        }
        out.into_iter().collect()
    }

    /// Whether an Arrange command has anything to move.
    pub fn can_arrange(&mut self) -> bool {
        !self.arranged().is_empty()
    }

    /// Move the picked fills and shapes in their layers' stacks (one undo step). Returns
    /// whether anything moved.
    pub fn arrange(&mut self, how: Arrange) -> bool {
        let picked = self.arranged();
        if picked.is_empty() {
            return false;
        }
        let shapes = self.fill_shapes();
        let stack = self.stack();
        let doc = &self.doc;
        // Each layer's movable things, with their order and bounds.
        let mut layers: HashMap<LayerId, Vec<Placed>> = HashMap::new();
        for (id, f) in doc.fills() {
            let b = shapes.get(&id).map_or(drift_core::Bounds::EMPTY, |s| {
                crate::fills::region_bounds(&s.region)
            });
            layers
                .entry(f.layer)
                .or_default()
                .push((Unit::Fill(id), f.order.into(), b));
        }
        for (id, u) in doc.underlays() {
            if let Some(l) = u.layer {
                layers
                    .entry(l)
                    .or_default()
                    .push((Unit::Picture(id), u.order.into(), u.bounds()));
            }
        }
        for (id, e) in doc.iter() {
            let outline = stack
                .curve_group
                .get(&id)
                .is_some_and(|k| stack.groups[*k].fill.is_some());
            if !e.kind.is_point() && !outline {
                let b = self
                    .index
                    .shape(id)
                    .map_or(drift_core::Bounds::EMPTY, |s| s.bounds());
                layers
                    .entry(e.layer)
                    .or_default()
                    .push((Unit::Curve(id), curve_order(e.order), b));
            }
        }
        let mut changes: Vec<(Unit, i64)> = Vec::new();
        for list in layers.values_mut() {
            if !list.iter().any(|(u, ..)| picked.contains(u)) {
                continue;
            }
            list.sort_by_key(|(u, o, _)| u.key(*o));
            let moved = restack(list, &picked, how);
            if moved
                .iter()
                .map(|(u, ..)| u)
                .eq(list.iter().map(|(u, ..)| u))
            {
                continue;
            }
            // Whichever way changes fewer (and stays in range).
            let fewer = [renumber(&moved, how.up()), renumber(&moved, !how.up())]
                .into_iter()
                .flatten()
                .min_by_key(Vec::len);
            changes.extend(fewer.unwrap_or_default());
        }
        if changes.is_empty() {
            return false;
        }
        self.doc.begin();
        for (unit, order) in changes {
            match unit {
                Unit::Fill(id) => {
                    if let Some(mut f) = self.doc.fill(id).cloned() {
                        f.order = order as i32;
                        self.doc.set_fill(id, f);
                    }
                }
                Unit::Picture(id) => {
                    if let Some(mut u) = self.doc.underlay(id).cloned() {
                        u.order = order as i32;
                        self.doc.set_underlay(id, u);
                    }
                }
                Unit::Curve(id) => {
                    let _ = self
                        .doc
                        .set_order(id, (order != TOP).then_some(order as i32));
                }
                Unit::Copy(..) => {}
            }
        }
        self.commit(how.label())
    }
}

/// A layer's things in their new order, the picked ones moved as `how` says (keeping their
/// order among themselves).
fn restack(list: &[Placed], picked: &[Unit], how: Arrange) -> Vec<Placed> {
    let is_picked = |u: &Unit| picked.contains(u);
    let mut out = list.to_vec();
    match how {
        Arrange::ToFront | Arrange::ToBack => {
            let (moving, rest): (Vec<Placed>, Vec<Placed>) =
                out.into_iter().partition(|x| is_picked(&x.0));
            out = if how == Arrange::ToFront {
                rest.into_iter().chain(moving).collect()
            } else {
                moving.into_iter().chain(rest).collect()
            };
        }
        Arrange::Forward => {
            // From the top down, each picked thing goes just over the next unpicked thing
            // above it that it overlaps (or the next one, if it overlaps none).
            for i in (0..out.len()).rev() {
                if !is_picked(&out[i].0) {
                    continue;
                }
                let b = out[i].2;
                let above: Vec<usize> = (i + 1..out.len())
                    .filter(|j| !is_picked(&out[*j].0))
                    .collect();
                if let Some(&to) = above
                    .iter()
                    .find(|j| out[**j].2.intersects(&b))
                    .or(above.first())
                {
                    let x = out.remove(i);
                    out.insert(to, x);
                }
            }
        }
        Arrange::Backward => {
            // From the bottom up, each picked thing goes just under the next unpicked thing
            // beneath it that it overlaps (or the next one, if it overlaps none).
            for i in 0..out.len() {
                if !is_picked(&out[i].0) {
                    continue;
                }
                let b = out[i].2;
                let below: Vec<usize> = (0..i).rev().filter(|j| !is_picked(&out[*j].0)).collect();
                if let Some(&to) = below
                    .iter()
                    .find(|j| out[**j].2.intersects(&b))
                    .or(below.first())
                {
                    let x = out.remove(i);
                    out.insert(to, x);
                }
            }
        }
    }
    out
}

/// New orders for a layer's things in a new stacking sequence (bottom first), changing as
/// few as it can: going up, each keeps its order unless it must rise over the one beneath;
/// going down, each keeps it unless it must sink under the one above. Returns the changes,
/// or `None` if that would take an order out of range (only a curve can be at [`TOP`]).
fn renumber(seq: &[Placed], up: bool) -> Option<Vec<(Unit, i64)>> {
    let mut changes = Vec::new();
    let mut last: Option<(i64, Unit)> = None;
    let steps: Vec<&Placed> = if up {
        seq.iter().collect()
    } else {
        seq.iter().rev().collect()
    };
    for &&(unit, order, _) in &steps {
        let fits = |o: i64| last.is_none_or(|l| if up { unit.key(o) > l } else { unit.key(o) < l });
        let new = match last {
            Some((lo, lu)) if !fits(order) => match (up, if up { unit > lu } else { unit < lu }) {
                (_, true) => lo,
                (true, false) => lo + 1,
                (false, false) => lo - 1,
            },
            _ => order,
        };
        let curve = matches!(unit, Unit::Curve(_));
        if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&new) && !(curve && new == TOP) {
            return None;
        }
        if new != order {
            changes.push((unit, new));
        }
        last = Some(unit.key(new));
    }
    Some(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::{DVec2, Document, Fill, FillKind};

    /// A filled square at `x`, its curves, and its fill.
    fn square(doc: &mut Document, x: f64) -> (Vec<EntityId>, FillId) {
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .map(|(px, py)| doc.add_point(DVec2::new(x + px, py)));
        let lines: Vec<EntityId> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        let fill = doc.add_fill(Fill::new(
            lines.clone(),
            DVec2::new(x + 5.0, 5.0),
            FillKind::Solid,
        ));
        (lines, fill)
    }

    fn order(ed: &mut Editor) -> Vec<(Option<FillRef>, Vec<EntityId>)> {
        ed.stack()
            .groups
            .iter()
            .filter(|g| g.fill.is_some() || !g.curves.is_empty())
            .map(|g| (g.fill, g.curves.clone()))
            .collect()
    }

    #[test]
    fn fills_stack_with_their_outlines_and_loose_curves_on_top() {
        let mut doc = Document::new();
        let (a_lines, a) = square(&mut doc, 0.0);
        let (b_lines, b) = square(&mut doc, 5.0);
        let p = doc.add_point(DVec2::new(-5.0, 5.0));
        let q = doc.add_point(DVec2::new(20.0, 5.0));
        let across = doc.add_line(p, q).unwrap();
        let mut ed = Editor::new(doc);
        assert_eq!(
            order(&mut ed),
            [
                (Some(FillRef::Fill(a)), a_lines.clone()),
                (Some(FillRef::Fill(b)), b_lines.clone()),
                (None, vec![across]),
            ]
        );

        // The first square brought forward: over the second, still under the loose line.
        ed.selected_fills = [a].into();
        assert!(ed.arrange(Arrange::Forward));
        assert_eq!(
            order(&mut ed),
            [
                (Some(FillRef::Fill(b)), b_lines.clone()),
                (Some(FillRef::Fill(a)), a_lines.clone()),
                (None, vec![across]),
            ]
        );
        // Picked by an outline curve, the shape moves as one: to the front, over the line.
        ed.selected_fills.clear();
        ed.selection = [b_lines[2]].into();
        assert!(ed.arrange(Arrange::ToFront));
        assert_eq!(
            order(&mut ed),
            [
                (Some(FillRef::Fill(a)), a_lines.clone()),
                (None, vec![across]),
                (Some(FillRef::Fill(b)), b_lines.clone()),
            ]
        );
        // The line sent to the back; again: nothing moves, no undo step.
        ed.selection = [across].into();
        assert!(ed.arrange(Arrange::ToBack));
        assert!(!ed.arrange(Arrange::ToBack));
        assert_eq!(order(&mut ed)[0], (None, vec![across]));
        // Only the line's order changed for that.
        assert!(ed.doc.get(across).unwrap().order.is_some());
        assert_eq!(ed.doc.get(a_lines[0]).unwrap().order, None);
        // One undo step each.
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(order(&mut ed)[2], (None, vec![across]));
    }
}
