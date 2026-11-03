//! Offsets in the editor (#247): keeping them regenerated as the drawing changes
//! ([`Editor::follow_offsets`], run by every commit and drag step after the walls), who owns
//! what, picking an offset by any of its pieces, and the offset commands (its distance,
//! its property dimension, Detach, copies).
//!
//! The geometry is `drift_geom::offsets`; the record is `drift_core::Offset`. Walls set the
//! protocol (`docs/editor-split-design.md` §6); this file is its second row.

use std::collections::{BTreeSet, HashSet};

use drift_core::{
    DVec2, Document, EntityId, EntityKind, Kind, Offset, OffsetId, PropertyDim, PropertyRef, Shape,
};

use crate::editor::{EditError, Editor};

/// The offset's distance drawn as a dimension: between a point of the source and the point
/// of the offset square to it, with where its label sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub from: DVec2,
    pub to: DVec2,
    pub value: f64,
    pub label: DVec2,
}

impl Editor {
    /// The offset an entity is a piece (or a point) of.
    pub fn offset_of(&self, id: EntityId) -> Option<OffsetId> {
        if !self.doc.has_offsets() {
            return None;
        }
        self.wall_owners().offset(id)
    }

    /// An offset's pieces (its curves).
    pub fn offset_curves(&self, x: OffsetId) -> Vec<EntityId> {
        self.doc
            .offset(x)
            .map(|o| o.made.curves.values().copied().collect())
            .unwrap_or_default()
    }

    /// What picking an offset selects: every piece of it.
    pub fn offset_pick_set(&self, x: OffsetId) -> BTreeSet<EntityId> {
        self.offset_curves(x).into_iter().collect()
    }

    /// The offsets picked: those all of whose pieces are in the selection.
    pub fn selected_offsets(&self) -> BTreeSet<OffsetId> {
        if !self.doc.has_offsets() {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let candidates: BTreeSet<OffsetId> = self
            .selection
            .iter()
            .filter_map(|id| owners.offset(*id))
            .collect();
        candidates
            .into_iter()
            .filter(|x| {
                let set = self.offset_pick_set(*x);
                !set.is_empty() && set.iter().all(|e| self.selection.contains(e))
            })
            .collect()
    }

    /// The offset lit up under the pointer.
    pub(crate) fn hovered_offset(&self) -> Option<OffsetId> {
        self.hovered.and_then(|h| self.offset_of(h))
    }

    /// Whether every one of `ids` is an offset's piece (and there is one): a move, turn or
    /// scale of them alone does nothing to the offset.
    pub(crate) fn only_offset_pieces(&self, ids: &[EntityId]) -> bool {
        if ids.is_empty() || !self.doc.has_offsets() {
            return false;
        }
        let owners = self.wall_owners();
        ids.iter().all(|id| owners.offset(*id).is_some())
    }

    /// `ids` without offsets' pieces (they follow their source).
    pub(crate) fn without_offset_pieces(&self, ids: &[EntityId]) -> Vec<EntityId> {
        if !self.doc.has_offsets() {
            return ids.to_vec();
        }
        let owners = self.wall_owners();
        ids.iter()
            .copied()
            .filter(|id| owners.offset(*id).is_none())
            .collect()
    }

    /// Among curves being copied, the offsets copied as records (every source curve is
    /// among them) and the pieces to leave out of the plain copy (theirs, and walls' parts,
    /// which the walls make again). An offset whose source isn't all copied has its pieces
    /// copied as plain curves: a detached copy.
    pub(crate) fn copied_as_records(
        &self,
        curves: &BTreeSet<EntityId>,
    ) -> (BTreeSet<OffsetId>, HashSet<EntityId>) {
        copied_as_records(&self.doc, curves)
    }

    // ----- Keeping offsets generated -----------------------------------------------------

    /// Regenerate the offsets the open transaction touched: their records, their source
    /// curves (or those curves' points), or their pieces. Run after the walls (a source may
    /// be a wall's face). Nothing to do, and nothing paid, on a drawing without offsets.
    pub(crate) fn follow_offsets(&mut self) {
        if !self.doc.has_offsets() || !self.doc.in_transaction() {
            return;
        }
        let mut which: BTreeSet<OffsetId> = self
            .doc
            .offsets_touched_in_transaction()
            .into_iter()
            .filter(|x| self.doc.offset(*x).is_some())
            .collect();
        let touched: HashSet<EntityId> = self
            .doc
            .entities_touched_in_transaction()
            .into_iter()
            .collect();
        if !touched.is_empty() {
            for (id, o) in self.doc.offsets() {
                if which.contains(&id) {
                    continue;
                }
                let source = o.source.iter().any(|c| {
                    touched.contains(c)
                        || self
                            .doc
                            .get(*c)
                            .is_some_and(|e| e.kind.points().any(|p| touched.contains(&p)))
                });
                if source || o.made.entities().any(|e| touched.contains(&e)) {
                    which.insert(id);
                }
            }
        }
        if which.is_empty() {
            return;
        }
        let report = drift_geom::offsets::regenerate_some(&mut self.doc, &which);
        if self.out.notice.is_none()
            && let Some(n) = report.notices.first()
        {
            self.out.notice = Some(n.clone());
        }
    }

    /// On opening a file: every offset made again, so what an older build edited outside
    /// the offset is put back. How many pieces and points had to be rebuilt, made or
    /// removed.
    pub(crate) fn repair_offsets(&mut self) -> usize {
        if !self.doc.has_offsets() {
            return 0;
        }
        let r = drift_geom::offsets::regenerate(&mut self.doc);
        r.rebuilt + r.created + r.deleted
    }

    /// Re-evaluate offsets' distance expressions (inside a transaction, before a solve).
    pub(crate) fn apply_offset_distance(&mut self) -> Result<(), EditError> {
        let offsets: Vec<(OffsetId, Offset)> = self
            .doc
            .offsets()
            .filter(|(_, o)| o.distance_expr.is_some())
            .map(|(id, o)| (id, o.clone()))
            .collect();
        if offsets.is_empty() {
            return Ok(());
        }
        let eval = drift_core::params::evaluate(&self.doc);
        for (id, o) in offsets {
            let expr = o.distance_expr.clone().unwrap_or_default();
            let v = drift_core::params::evaluate_text(&self.doc, &eval, &expr, Kind::Length)
                .map_err(|e| EditError::Invalid(format!("Offset {id}'s distance: {}", e.0)))?
                .value;
            if v > 0.0 && v.is_finite() && v != o.distance {
                let _ = self.doc.set_offset(id, Offset { distance: v, ..o });
            }
        }
        Ok(())
    }

    // ----- Commands --------------------------------------------------------------------------

    /// Change offsets' records (one undo step), as `edit_walls` does.
    pub fn edit_offsets(
        &mut self,
        offsets: &[OffsetId],
        label: &str,
        mut f: impl FnMut(&mut Offset),
    ) -> bool {
        self.doc.begin();
        let mut any = false;
        for x in offsets {
            if let Some(mut o) = self.doc.offset(*x).cloned() {
                f(&mut o);
                if self.doc.offset(*x) != Some(&o) && self.doc.set_offset(*x, o).is_ok() {
                    any = true;
                }
            }
        }
        if !any {
            self.doc.rollback();
            self.sync();
            return false;
        }
        self.commit(label)
    }

    /// Set offsets' distance from typed text (a length, or an expression of variables, which
    /// is kept and re-evaluated when they change).
    pub fn set_offset_distance(
        &mut self,
        offsets: &[OffsetId],
        text: &str,
    ) -> Result<(), EditError> {
        let e = self.evaluate_text(text, Kind::Length)?;
        if !(e.value > 0.0 && e.value.is_finite()) {
            return Err(EditError::Invalid(
                "An offset's distance must be more than 0".into(),
            ));
        }
        let expr = (!e.references.is_empty()).then(|| e.text.clone());
        self.edit_offsets(offsets, "Offset distance", |o| {
            o.distance = e.value;
            o.distance_expr = expr.clone();
        });
        Ok(())
    }

    /// Set an offset's distance inside an open transaction, as a drag of one of its pieces
    /// does: the record changes and the offset is made again at once. False, with nothing
    /// changed, if the offset can't be made at that distance.
    pub fn drag_offset_distance(&mut self, x: OffsetId, distance: f64) -> bool {
        if !(distance > 0.0 && distance.is_finite()) {
            return false;
        }
        let Some(o) = self.doc.offset(x).cloned() else {
            return false;
        };
        if o.distance == distance {
            return true;
        }
        let was = o.clone();
        // (A value set here replaces an expression the distance was linked to.)
        if self
            .doc
            .set_offset(
                x,
                Offset {
                    distance,
                    distance_expr: None,
                    ..o
                },
            )
            .is_err()
        {
            return false;
        }
        let report = drift_geom::offsets::regenerate_some(&mut self.doc, &[x].into());
        if !report.notices.is_empty() {
            let _ = self.doc.set_offset(x, was);
            return false;
        }
        self.index.update(&self.doc);
        true
    }

    /// The pointer's distance from an offset's source chain (what the Offset tool measures
    /// when it is placed).
    pub fn offset_distance_at(&self, x: OffsetId, at: DVec2) -> Option<f64> {
        let o = self.doc.offset(x)?;
        o.source
            .iter()
            .filter_map(|c| self.doc.shape(*c))
            .map(|s| drift_geom::distance(&s, at))
            .fold(None, |best: Option<f64>, d| {
                Some(best.map_or(d, |b| b.min(d)))
            })
    }

    /// Detach offsets: their pieces become plain curves (looks kept) and the records go
    /// with their distance dimensions, one undo step. The pieces are picked after.
    pub fn detach_offsets(&mut self, offsets: &[OffsetId]) -> bool {
        if offsets.is_empty() {
            return false;
        }
        self.doc.begin();
        let mut pieces: BTreeSet<EntityId> = BTreeSet::new();
        for x in offsets {
            pieces.extend(self.offset_curves(*x));
            self.doc.remove_offset(*x);
        }
        let ok = self.commit("Detach offset");
        if ok {
            self.selection = pieces;
            self.selected_fills.clear();
            self.selected_region = None;
        }
        ok
    }

    /// Show an offset's distance as a dimension on its first source curve (one undo step),
    /// if it has none.
    pub fn show_offset_distance(&mut self, x: OffsetId) -> bool {
        if self.doc.offset(x).is_none()
            || self
                .doc
                .property_dims()
                .any(|(_, d)| d.target == PropertyRef::OffsetDistance(x))
        {
            return false;
        }
        self.doc.begin();
        let _ = self.doc.add_property_dim(PropertyDim {
            target: PropertyRef::OffsetDistance(x),
            seg: 0,
            along: 0.5,
            out: 0.0,
        });
        self.commit("Show offset distance")
    }

    /// Copies of offsets for copied curves, inside the open transaction: an offset all of
    /// whose source curves were copied (`curves` gives each one's copy) gets a copy along
    /// them, made when the transaction commits. A `mirrored` copy lies on the other side
    /// of its chain (a reflection swaps left and right); a whole circle's or ellipse's
    /// offset stays outward or inward.
    pub(crate) fn copy_offsets_onto(
        &mut self,
        curves: &dyn Fn(EntityId) -> Option<EntityId>,
        mirrored: bool,
    ) {
        let records: Vec<Offset> = self.doc.offsets().map(|(_, o)| o.clone()).collect();
        copy_offsets(&records, &mut self.doc, curves, mirrored);
    }
}

/// See [`Editor::copied_as_records`].
pub(crate) fn copied_as_records(
    doc: &Document,
    curves: &BTreeSet<EntityId>,
) -> (BTreeSet<OffsetId>, HashSet<EntityId>) {
    let owners = doc.wall_owners();
    let mut records = BTreeSet::new();
    let mut left_out: HashSet<EntityId> = curves
        .iter()
        .copied()
        .filter(|id| owners.by_wall(*id))
        .collect();
    for (id, o) in doc.offsets() {
        if o.source.iter().all(|c| curves.contains(c)) {
            records.insert(id);
            left_out.extend(o.made.entities());
        }
    }
    (records, left_out)
}

/// Copies of the `records` whose source curves `curves` maps, into `dst`, with nothing made
/// yet (see [`Editor::copy_offsets_onto`]).
pub(crate) fn copy_offsets(
    records: &[Offset],
    dst: &mut Document,
    curves: &dyn Fn(EntityId) -> Option<EntityId>,
    mirrored: bool,
) {
    for o in records {
        let Some(source) = o
            .source
            .iter()
            .map(|c| curves(*c))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let whole = o.source.len() == 1
            && matches!(
                dst.get(source[0]).map(|e| e.kind),
                Some(EntityKind::Circle { .. } | EntityKind::Ellipse { .. })
            );
        let mut copy = Offset::new(source, o.side, o.distance);
        copy.distance_expr = o.distance_expr.clone();
        if mirrored && !whole {
            copy.side = o.side.flipped();
        }
        let _ = dst.add_offset(copy);
    }
}

/// An offset's distance dimension as drawn: from the source curve the dimension sits on
/// (`seg`, at `along`) to the offset square to it, the label `out` beside the gap. None when
/// the offset or that curve is gone.
pub fn distance_dimension(doc: &Document, d: &PropertyDim) -> Option<Gap> {
    let PropertyRef::OffsetDistance(x) = d.target else {
        return None;
    };
    let o = doc.offset(x)?;
    let curve = o
        .source
        .get(usize::from(d.seg))
        .or(o.source.first())
        .copied()?;
    let shape = doc.shape(curve)?;
    let along = d.along.clamp(0.0, 1.0);
    let from = match shape {
        Shape::Line { a, b } => a + (b - a) * along,
        Shape::Circle { center, radius } => {
            center + DVec2::from_angle(along * std::f64::consts::TAU) * radius
        }
        Shape::Arc { start, sweep, .. } | Shape::Ellipse { start, sweep, .. } => {
            shape.point_at(start + sweep * along)
        }
        Shape::Bezier { .. } => shape.point_at(along),
        Shape::Point(p) => p,
    };
    // The offset's point square to it: the nearest point of its pieces.
    let to = o
        .made
        .curves
        .values()
        .filter_map(|c| doc.shape(*c))
        .map(|s| drift_geom::nearest_point(&s, from))
        .min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))?;
    // The label beside the gap: `out` to the left of from → to (negative: the right).
    let n = (to - from).perp().normalize_or_zero();
    Some(Gap {
        from,
        to,
        value: o.distance,
        label: (from + to) * 0.5 + n * d.out,
    })
}
