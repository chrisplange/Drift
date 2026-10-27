//! Closed areas (regions) of the sketch, and how a move or turn in progress carries them.

use super::*;

impl Editor {
    /// The closed regions formed by solid curves (construction and centre lines are
    /// references, not boundaries), recomputed when the document changes.
    pub fn regions(&mut self) -> std::rc::Rc<Vec<drift_geom::regions::Region>> {
        // (Changing the rules is a change too: it bumps the revision.)
        let revision = self.doc.revision();
        if let Some((r, regions)) = &self.regions
            && *r == revision
        {
            return regions.clone();
        }
        if self.doc.rules().regions_by_layer {
            return self.regions_per_layer(revision);
        }
        let bounding = self.fill_edges();
        let (ids, curves): (Vec<EntityId>, Vec<Shape>) = self
            .doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point() && (e.style == LineKind::Solid || bounding.contains(id))
            })
            .filter_map(|(id, _)| Some((id, self.doc.shape(id)?)))
            .unzip();
        let regions = std::rc::Rc::new(drift_geom::regions::regions(&curves));
        self.regions = Some((revision, regions.clone()));
        self.region_ids = std::rc::Rc::new(ids);
        regions
    }

    /// Closed areas found from each layer's curves on their own: an outline on one layer
    /// isn't closed or cut by curves on another.
    pub(super) fn regions_per_layer(
        &mut self,
        revision: u64,
    ) -> std::rc::Rc<Vec<drift_geom::regions::Region>> {
        let mut by_layer: std::collections::BTreeMap<drift_core::LayerId, Vec<(EntityId, Shape)>> =
            Default::default();
        let bounding = self.fill_edges();
        for (id, e) in self.doc.iter() {
            if e.kind.is_point() || (e.style != LineKind::Solid && !bounding.contains(&id)) {
                continue;
            }
            if let Some(shape) = self.doc.shape(id) {
                by_layer.entry(e.layer).or_default().push((id, shape));
            }
        }
        let mut ids = Vec::new();
        let mut all = Vec::new();
        for (_, curves) in by_layer {
            let offset = ids.len();
            let shapes: Vec<Shape> = curves.iter().map(|(_, s)| *s).collect();
            ids.extend(curves.iter().map(|(id, _)| *id));
            for mut r in drift_geom::regions::regions(&shapes) {
                // Curve indices into this layer's curves become indices into all of them.
                for l in std::iter::once(&mut r.outer).chain(&mut r.holes) {
                    for s in &mut l.sources {
                        *s += offset;
                    }
                }
                all.push(r);
            }
        }
        let regions = std::rc::Rc::new(all);
        self.regions = Some((revision, regions.clone()));
        self.region_ids = std::rc::Rc::new(ids);
        regions
    }

    /// Construction curves that shape fills: an imported fill's edges, which don't print.
    /// Areas are found with them as with drawn curves. A fill keeps only its outline (its
    /// holes are whatever lies inside it), so on a layer where a fill is bounded by
    /// construction curves, all of that layer's construction curves count: its holes' too.
    pub(crate) fn fill_edges(&self) -> std::collections::BTreeSet<EntityId> {
        let construction = |id: &EntityId| {
            self.doc
                .get(*id)
                .filter(|e| e.style == LineKind::Construction)
                .map(|e| e.layer)
        };
        let layers: std::collections::BTreeSet<drift_core::LayerId> = self
            .doc
            .fills()
            .flat_map(|(_, f)| f.boundary.iter())
            .filter_map(construction)
            .collect();
        if layers.is_empty() {
            return Default::default();
        }
        self.doc
            .iter()
            .filter(|(_, e)| {
                !e.kind.is_point() && e.style == LineKind::Construction && layers.contains(&e.layer)
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// The entity of each curve index in [`regions`](Self::regions)' loops.
    pub fn region_ids(&mut self) -> std::rc::Rc<Vec<EntityId>> {
        self.regions();
        self.region_ids.clone()
    }

    /// The selected region: the one its point is in, or, when a change took the region
    /// away from its point, the one bounded by the same curves.
    pub fn region(&mut self) -> Option<drift_geom::regions::Region> {
        let Some(seed) = self.selected_region else {
            self.region_outline = None;
            return None;
        };
        let seed = self.region_motion.map_or(seed, |m| m.apply(seed));
        let regions = self.regions();
        // (One whose outline is hidden isn't picked while it can't be seen.)
        if let Some(found) = regions
            .iter()
            .filter(|r| r.contains(seed) && self.region_shown(r))
            .min_by(|a, b| a.area.total_cmp(&b.area))
        {
            let outline = self.outline_of(found);
            if self.region_outline.as_ref() != Some(&outline) {
                self.region_outline = Some(outline);
            }
            return Some(found.clone());
        }
        // Its point is outside it now: find it by its curves, and put the point back inside.
        let same = self.region_outline.as_ref().and_then(|outline| {
            regions
                .iter()
                .find(|r| self.outline_of(r) == *outline && self.region_shown(r))
                .cloned()
        });
        match same {
            Some(region) => {
                let inside = interior_point(&region, seed);
                match self.region_motion {
                    // Mid-move: the point follows the region until the move lands.
                    Some(_) => self.region_motion = Some(RegionMotion::At(inside)),
                    None => self.selected_region = Some(inside),
                }
                Some(region)
            }
            None => {
                // Mid-move the outline may cross itself for a moment: keep the pick until
                // it lands.
                if self.region_motion.is_none() {
                    self.selected_region = None;
                    self.region_outline = None;
                }
                None
            }
        }
    }

    /// The area a click at `p` picks: the smallest closed area around it whose outline can
    /// be seen. One outlined (in part) by curves on hidden layers is never picked, though it
    /// still bounds fills.
    pub fn visible_region_at(&mut self, p: DVec2) -> Option<drift_geom::regions::Region> {
        let regions = self.regions();
        regions
            .iter()
            .filter(|r| r.contains(p) && self.region_shown(r))
            .min_by(|a, b| a.area.total_cmp(&b.area))
            .cloned()
    }

    /// Whether every curve of a region's outer outline is on a shown layer (from the last
    /// [`regions`](Self::regions)).
    pub(super) fn region_shown(&self, region: &drift_geom::regions::Region) -> bool {
        region
            .outer
            .curves()
            .into_iter()
            .filter_map(|i| self.region_ids.get(i))
            .all(|id| self.doc.is_visible(*id))
    }

    /// The curves bounding a region (from the last [`regions`](Self::regions)).
    pub(super) fn outline_of(&self, region: &drift_geom::regions::Region) -> BTreeSet<EntityId> {
        std::iter::once(&region.outer)
            .chain(&region.holes)
            .flat_map(|l| l.curves())
            .filter_map(|i| self.region_ids.get(i).copied())
            .collect()
    }

    // ----- Parameters ------------------------------------------------------------------------

    /// Change the display units (an undoable document setting; no geometry moves).
    /// A click inside an empty closed area picks nothing (the drawing's rule, unless the
    /// assistant is picking).
    pub fn regions_need_fill(&self) -> bool {
        self.doc.rules().regions_need_fill && !self.pick_any_area
    }
}

/// A unit rotor for `degrees`, exact for multiples of 90°.
pub(super) fn exact_rotor(degrees: f64) -> DVec2 {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() <= 1e-12 {
        match (quarter.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::from_angle(degrees.to_radians())
    }
}

/// Whether a turn is a whole number of quarter turns.
pub fn is_quarter_turn(degrees: f64) -> bool {
    let q = degrees / 90.0;
    (q - q.round()).abs() <= 1e-9
}

/// How a move or a turn in progress has carried the geometry so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegionMotion {
    Move(DVec2),
    Turn {
        pivot: DVec2,
        degrees: f64,
    },
    Scale {
        anchor: DVec2,
        scale: DVec2,
    },
    /// Found again by its curves: the point is here for now.
    At(DVec2),
}

impl RegionMotion {
    pub fn apply(self, p: DVec2) -> DVec2 {
        match self {
            Self::Move(offset) => p + offset,
            Self::Turn { pivot, degrees } => {
                pivot + DVec2::from_angle(degrees.to_radians()).rotate(p - pivot)
            }
            Self::Scale { anchor, scale } => anchor + (p - anchor) * scale,
            Self::At(at) => at,
        }
    }
}

/// A point inside a region: `near` if it is, else its centroid, else the first of a grid
/// over its bounds that is.
pub(crate) fn interior_point(region: &drift_geom::regions::Region, near: DVec2) -> DVec2 {
    if region.contains(near) {
        return near;
    }
    if region.contains(region.centroid) {
        return region.centroid;
    }
    // Of a grid of points over its bounds, the inside one nearest `near`.
    let (b, n) = (region.bounds, 24);
    (1..n)
        .flat_map(|i| (1..n).map(move |j| (i, j)))
        .map(|(i, j)| {
            b.min + (b.max - b.min) * DVec2::new(f64::from(i), f64::from(j)) / f64::from(n)
        })
        .filter(|p| region.contains(*p))
        .min_by(|p, q| p.distance(near).total_cmp(&q.distance(near)))
        .unwrap_or(region.centroid)
}
