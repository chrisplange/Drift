//! Blocks in the editor: picking and placing copies, making a block from the selection,
//! exploding copies, editing a block in place, and a starter furniture library.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI};

use drift_core::{
    Appearance, Block, BlockId, BlockItem, Bounds, DVec2, EntityId, GroupId, Instance, InstanceId,
    LineKind, Shape,
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

impl Editor {
    /// A copy's extent in the world.
    pub fn instance_bounds(&self, id: InstanceId) -> Bounds {
        let mut b = Bounds::EMPTY;
        for item in self.doc.instance_items(id) {
            let s = item.shape.bounds();
            b.include(s.min);
            b.include(s.max);
        }
        b
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
        let on_edge = candidates.iter().rev().find(|(id, _)| {
            self.doc
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
    pub fn make_block(&mut self, name: &str) -> Result<InstanceId, String> {
        let referenced = self.doc.defining_points(
            self.selection
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        let ids: Vec<EntityId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
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
        let base = bounds.center();
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
        self.doc.begin();
        let block = self.doc.add_block(Block { name, items });
        self.doc.delete(ids);
        let id = self
            .doc
            .add_instance(Instance::new(block, base))
            .map_err(|e| e.to_string())?;
        if same_layer && let Some(mut i) = self.doc.instance(id).copied() {
            i.layer = layer;
            let _ = self.doc.set_instance(id, i);
        }
        self.commit("Make block");
        self.selection.clear();
        self.selected_instances = [id].into();
        Ok(id)
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
                self.notice = Some(e.to_string());
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
            let tol = 1e-9 * p.abs().max_element().max(1.0);
            if let Some((_, id)) = points.iter().find(|(q, _)| q.distance(p) <= tol) {
                return *id;
            }
            let id = doc.add_point(p);
            points.push((p, id));
            id
        };
        let mut created = Vec::new();
        for item in self.doc.instance_items(id) {
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
            }
        }
        created
    }

    /// Replace the picked copies with ordinary geometry (picked afterwards).
    pub fn explode_selected(&mut self) {
        let copies: Vec<InstanceId> = self.selected_instances.iter().copied().collect();
        if copies.is_empty() {
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
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Write the edited geometry back into the block (every copy updates) and remove the
    /// temporary geometry.
    pub fn finish_block_edit(&mut self) {
        let Some(edit) = self.block_edit.take() else {
            return;
        };
        self.open_group = None;
        let (Some(group), Some(copy), Some(mut block)) = (
            self.doc.group(edit.group).cloned(),
            self.doc.instance(edit.instance).copied(),
            self.doc.block(edit.block).cloned(),
        ) else {
            return;
        };
        let referenced = self.doc.defining_points(
            group
                .members
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        block.items = group
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
            .collect();
        self.doc.begin();
        self.doc.set_block(edit.block, block);
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

    /// Add the starter furniture blocks the document doesn't have yet. Returns how many.
    pub fn add_starter_blocks(&mut self) -> usize {
        let missing: Vec<Block> = starter_blocks()
            .into_iter()
            .filter(|b| self.doc.block_named(&b.name).is_none())
            .collect();
        if missing.is_empty() {
            return 0;
        }
        let n = missing.len();
        self.doc.begin();
        for b in missing {
            self.doc.add_block(b);
        }
        self.commit("Add furniture blocks");
        n
    }
}

// ---------------------------------------------------------------------------------------------
// Starter library (millimetres, base point at the centre, front facing down)

fn item(shape: Shape) -> BlockItem {
    BlockItem {
        shape,
        style: LineKind::Solid,
        appearance: Appearance::default(),
    }
}

fn line(a: (f64, f64), b: (f64, f64)) -> BlockItem {
    item(Shape::Line {
        a: DVec2::new(a.0, a.1),
        b: DVec2::new(b.0, b.1),
    })
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<BlockItem> {
    vec![
        line((x0, y0), (x1, y0)),
        line((x1, y0), (x1, y1)),
        line((x1, y1), (x0, y1)),
        line((x0, y1), (x0, y0)),
    ]
}

fn arc(c: (f64, f64), r: f64, start: f64, sweep: f64) -> BlockItem {
    item(Shape::Arc {
        center: DVec2::new(c.0, c.1),
        radius: r,
        start,
        sweep,
    })
}

/// A rectangle with rounded corners.
fn rounded(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Vec<BlockItem> {
    vec![
        line((x0 + r, y0), (x1 - r, y0)),
        arc((x1 - r, y0 + r), r, -FRAC_PI_2, FRAC_PI_2),
        line((x1, y0 + r), (x1, y1 - r)),
        arc((x1 - r, y1 - r), r, 0.0, FRAC_PI_2),
        line((x1 - r, y1), (x0 + r, y1)),
        arc((x0 + r, y1 - r), r, FRAC_PI_2, FRAC_PI_2),
        line((x0, y1 - r), (x0, y0 + r)),
        arc((x0 + r, y0 + r), r, PI, FRAC_PI_2),
    ]
}

/// The built-in furniture blocks.
pub fn starter_blocks() -> Vec<Block> {
    let mut chair = rounded(-225.0, -225.0, 225.0, 200.0, 40.0);
    chair.extend(rect(-225.0, 200.0, 225.0, 260.0));
    let mut desk = rect(-700.0, -350.0, 700.0, 350.0);
    desk.extend(rect(-650.0, -300.0, -250.0, 300.0));
    let mut bed = rect(-800.0, -1000.0, 800.0, 1000.0);
    bed.extend(rounded(-720.0, 650.0, -40.0, 930.0, 60.0));
    bed.extend(rounded(40.0, 650.0, 720.0, 930.0, 60.0));
    bed.push(line((-800.0, 450.0), (800.0, 450.0)));
    let mut sofa = rounded(-1000.0, -450.0, 1000.0, 450.0, 60.0);
    sofa.extend(rect(-1000.0, 200.0, 1000.0, 450.0));
    sofa.extend(rect(-1000.0, -450.0, -800.0, 200.0));
    sofa.extend(rect(800.0, -450.0, 1000.0, 200.0));
    sofa.push(line((0.0, -450.0), (0.0, 200.0)));
    let mut wc = rect(-200.0, 150.0, 200.0, 350.0);
    wc.push(arc((0.0, -50.0), 180.0, -PI, PI));
    wc.push(line((-180.0, -50.0), (-180.0, 150.0)));
    wc.push(line((180.0, -50.0), (180.0, 150.0)));
    let mut basin = rounded(-300.0, -225.0, 300.0, 225.0, 50.0);
    basin.push(item(Shape::Circle {
        center: DVec2::new(0.0, -20.0),
        radius: 170.0,
    }));
    basin.push(item(Shape::Circle {
        center: DVec2::new(0.0, 160.0),
        radius: 20.0,
    }));
    // A door: 900 leaf, open 90°, its swing a thin arc. Base point at the hinge.
    let door = vec![
        line((0.0, 0.0), (0.0, 900.0)),
        line((0.0, 900.0), (40.0, 900.0)),
        line((40.0, 900.0), (40.0, 0.0)),
        BlockItem {
            shape: Shape::Arc {
                center: DVec2::ZERO,
                radius: 900.0,
                start: 0.0,
                sweep: FRAC_PI_2,
            },
            style: LineKind::Solid,
            appearance: Appearance {
                weight: Some(drift_core::LineWeight(0)),
                ..Appearance::default()
            },
        },
    ];
    [
        ("Chair", chair),
        ("Desk 1400", desk),
        ("Double bed", bed),
        ("Sofa", sofa),
        ("WC", wc),
        ("Basin", basin),
        ("Door 900", door),
    ]
    .into_iter()
    .map(|(name, items)| Block {
        name: name.into(),
        items,
    })
    .collect()
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
    fn starter_blocks_are_closed_and_named() {
        let blocks = starter_blocks();
        assert!(blocks.len() >= 7);
        let names: BTreeSet<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names.len(), blocks.len());
        let (mut ed, _) = square();
        assert_eq!(ed.add_starter_blocks(), blocks.len());
        assert_eq!(ed.add_starter_blocks(), 0, "only once");
    }
}
