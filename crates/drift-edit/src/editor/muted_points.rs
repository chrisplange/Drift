//! Points drawn as muted construction crosses (#501): a corner a fillet or chamfer kept for
//! a dimension or relation (#238), and a fillet's centre. They show at full strength while
//! what they belong to (the fillet, its lines, or a dimension or relation on the point) is
//! picked or hovered. Only how they are drawn: they stay snappable and pickable.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::*;

/// What a muted point is, for the Inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutedKind {
    /// A corner kept for a dimension (or a relation).
    KeptCorner { dimension: bool },
    /// The centre of an arc rounding a corner (tangent to the curves at both its ends).
    FilletCentre,
}

impl MutedKind {
    pub fn label(self) -> &'static str {
        match self {
            MutedKind::KeptCorner { dimension: true } => "Corner kept for a dimension",
            MutedKind::KeptCorner { dimension: false } => "Corner kept for a relation",
            MutedKind::FilletCentre => "A fillet's centre",
        }
    }
}

/// The drawing's muted points, as of a document structure (they don't move with a drag).
#[derive(Default)]
pub(crate) struct MutedPoints {
    /// Each muted point: what it is, and the entities that light it up (the fillet's arc
    /// and lines).
    of: HashMap<EntityId, (MutedKind, Vec<EntityId>)>,
    /// The muted points each entity lights up.
    by: HashMap<EntityId, Vec<EntityId>>,
}

impl MutedPoints {
    fn find(ed: &Editor) -> Self {
        let doc = &ed.doc;
        let mut out = Self::default();
        let mut tangent: HashMap<EntityId, HashSet<EntityId>> = HashMap::new();
        let mut corners = Vec::new();
        let mut dimensioned = HashSet::new();
        for (_, c) in doc.constraints() {
            if c.is_dimension() {
                dimensioned.extend(c.entities());
            }
            match *c {
                Constraint::Corner { point, a, b } => corners.push((point, a, b)),
                Constraint::Tangent { a, b } => {
                    tangent.entry(a).or_default().insert(b);
                    tangent.entry(b).or_default().insert(a);
                }
                _ => {}
            }
        }
        if corners.is_empty() && tangent.is_empty() {
            return out;
        }
        let ends = |id: EntityId| doc.get(id).and_then(|e| e.kind.ends());
        for (point, a, b) in corners {
            // The fillet or chamfer: a curve from one of a's ends to one of b's.
            let (Some(ea), Some(eb)) = (ends(a), ends(b)) else {
                continue;
            };
            let mut lit = vec![a, b];
            for &e in &ea {
                for user in doc.users_of(e) {
                    if user != a
                        && user != b
                        && ends(user).is_some_and(|u| u.iter().any(|q| eb.contains(q)))
                    {
                        lit.push(user);
                    }
                }
            }
            let dimension = dimensioned.contains(&point);
            out.of
                .insert(point, (MutedKind::KeptCorner { dimension }, lit));
        }
        for (&arc, partners) in &tangent {
            let Some(EntityKind::Arc { center, start, end }) = doc.get(arc).map(|e| e.kind) else {
                continue;
            };
            // Rounds a corner: tangent to a curve at each end.
            let at = |p: EntityId| {
                partners
                    .iter()
                    .copied()
                    .find(|&q| ends(q).is_some_and(|e| e.contains(&p)))
            };
            let (Some(c1), Some(c2)) = (at(start), at(end)) else {
                continue;
            };
            if c1 == c2 || out.of.contains_key(&center) {
                continue;
            }
            out.of
                .insert(center, (MutedKind::FilletCentre, vec![arc, c1, c2]));
        }
        for (&point, (_, lit)) in &out.of {
            for &e in lit {
                out.by.entry(e).or_default().push(point);
            }
        }
        out
    }

    /// Whether `id` is muted, given what is lit (picked, hovered, highlighted or snapped
    /// to): muted unless it or what it belongs to is lit.
    pub(crate) fn muted(&self, id: EntityId, lit: impl Fn(EntityId) -> bool) -> bool {
        self.of
            .get(&id)
            .is_some_and(|(_, by)| !lit(id) && !by.iter().any(|&e| lit(e)))
    }
}

impl Editor {
    /// The drawing's muted points (found again only when the drawing's make-up changes,
    /// not as geometry moves).
    pub(crate) fn muted_points(&self) -> Rc<MutedPoints> {
        let revision = self.doc.structure();
        let mut kept = self.muted_points.borrow_mut();
        if let Some((r, m)) = kept.as_ref()
            && *r == revision
        {
            return m.clone();
        }
        let m = Rc::new(MutedPoints::find(self));
        *kept = Some((revision, m.clone()));
        m
    }

    /// What point `id` is, if it is drawn muted (a corner kept for a dimension, a fillet's
    /// centre), for the Inspector.
    pub fn muted_kind(&self, id: EntityId) -> Option<MutedKind> {
        self.muted_points().of.get(&id).map(|(k, _)| *k)
    }

    /// The muted points that `ids` light up, which hovering them draws again.
    pub(crate) fn muted_lit_by(&self, ids: &std::collections::BTreeSet<EntityId>) -> Vec<EntityId> {
        let m = self.muted_points();
        ids.iter()
            .filter_map(|id| m.by.get(id))
            .flatten()
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{DVec2, Shape};
    use drift_geom::SnapOptions;

    use super::*;
    use crate::frame_items::ItemsCache;

    /// Two lines meeting at a corner the width is measured to, filleted: the corner stays
    /// (#238). Returns (editor, corner, bottom line, arc).
    fn filleted() -> (Editor, EntityId, EntityId, EntityId) {
        let mut ed = Editor::default();
        ed.prefs.auto_relations = true;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(100.0, 0.0));
        let c = ed.doc.add_point(DVec2::new(100.0, 50.0));
        let bottom = ed.doc.add_line(a, b).unwrap();
        ed.doc.add_line(b, c).unwrap();
        ed.doc
            .add_constraint(Constraint::Distance { a, b, value: 100.0 })
            .unwrap();
        ed.commit("Corner");
        let arc = ed.fillet(b, 10.0).unwrap();
        (ed, b, bottom, arc)
    }

    fn item_of(ed: &Editor, id: EntityId) -> drift_core::SketchItem {
        ed.items_tagged(true, true)
            .into_iter()
            .find(|t| t.entity == Some(id))
            .expect("drawn")
            .item
    }

    fn centre_of(ed: &Editor, arc: EntityId) -> EntityId {
        match ed.doc.get(arc).unwrap().kind {
            EntityKind::Arc { center, .. } => center,
            _ => unreachable!(),
        }
    }

    #[test]
    fn kept_corners_and_fillet_centres_are_muted() {
        let (mut ed, corner, bottom, arc) = filleted();
        let centre = centre_of(&ed, arc);
        assert_eq!(
            ed.muted_kind(corner),
            Some(MutedKind::KeptCorner { dimension: true })
        );
        assert_eq!(
            ed.muted_kind(corner).unwrap().label(),
            "Corner kept for a dimension"
        );
        assert_eq!(ed.muted_kind(centre), Some(MutedKind::FilletCentre));
        assert_eq!(ed.muted_kind(bottom), None);
        // At rest the corner is a muted cross.
        assert!(item_of(&ed, corner).marker);
        // Full strength while the fillet or one of its lines is picked or hovered.
        for lit in [bottom, arc] {
            ed.selection = [lit].into();
            assert!(!item_of(&ed, corner).marker, "{lit} picked");
            ed.selection.clear();
            ed.hovered = Some(lit);
            assert!(!item_of(&ed, corner).marker, "{lit} hovered");
            ed.hovered = None;
        }
        // And while a dimension on it is hovered (which highlights the point).
        ed.highlight = [corner].into();
        assert!(!item_of(&ed, corner).marker);
        ed.highlight.clear();
        // The fillet's centre: picked, it shows picked; shown with its hovered arc, at
        // full strength.
        ed.selection = [centre].into();
        let item = item_of(&ed, centre);
        assert_eq!(item.emphasis, drift_core::Emphasis::Selected);
        ed.selection.clear();
        ed.hovered = Some(arc);
        assert!(!item_of(&ed, centre).marker);
    }

    /// Still snappable: drawing from the kept corner snaps to it.
    #[test]
    fn muted_points_stay_snappable() {
        let (ed, corner, _, _) = filleted();
        let at = ed.doc.point(corner).unwrap();
        let snap = ed.index.snap(
            at + DVec2::new(0.1, 0.1),
            SnapOptions {
                tolerance: 0.5,
                grid: None,
                from: None,
            },
            |_| false,
        );
        assert_eq!(snap.and_then(|s| s.entity), Some(corner));
    }

    /// The canvas's kept lists draw the corner again when its line is hovered.
    #[test]
    fn hovering_a_line_lights_its_kept_corner_in_the_kept_lists() {
        let (mut ed, corner, bottom, _) = filleted();
        let at = ed.doc.point(corner).unwrap();
        let mut cache = ItemsCache::default();
        let marker_at = |cache: &ItemsCache| {
            cache
                .lists()
                .iter()
                .flatten()
                .find(|i| i.shape == Shape::Point(at))
                .map(|i| i.marker)
        };
        cache.prepare(&ed, true, true, None);
        assert_eq!(marker_at(&cache), Some(true));
        cache.restore();
        ed.hovered = Some(bottom);
        cache.prepare(&ed, true, true, None);
        assert_eq!(marker_at(&cache), Some(false));
        cache.restore();
    }
}
