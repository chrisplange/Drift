//! Closed areas (regions) of the sketch, and how a move or turn in progress carries them.

use super::*;
use drift_geom::regions::Region;

impl Editor {
    /// The closed regions formed by solid curves (construction and centre lines are
    /// references, not boundaries), recomputed when the document changes.
    pub fn regions(&mut self) -> Regions {
        // They depend on the curves, the rule and the curves fills keep to, and nothing else:
        // an edit's fills following it (after the areas were found for them) keeps them.
        let key = self.regions_key();
        if let Some((k, regions)) = &self.regions
            && *k == key
        {
            return regions.clone();
        }
        let input = self.areas_input(&key);
        let prior = self.regions.as_ref().map(|(_, r)| r.clone());
        self.regions_found += 1;
        let check = check_on().then(|| input.find(&[]));
        let (ids, regions) = self.areas.found(input, prior.as_ref());
        if let Some((fresh_ids, fresh)) = check {
            assert!(
                *ids == fresh_ids && format!("{regions:?}") == format!("{fresh:?}"),
                "the areas as kept differ from the areas found afresh"
            );
        }
        self.regions = Some((key, regions.clone()));
        self.region_ids = ids;
        regions
    }

    pub(super) fn regions_key(&self) -> RegionsKey {
        RegionsKey {
            geometry: self.doc.geometry(),
            by_layer: self.doc.rules().regions_by_layer,
            edges: self.fill_edges(),
        }
    }

    /// The curves [`regions`](Self::regions) are found among, as of now.
    pub(super) fn areas_input(&self, key: &RegionsKey) -> AreasInput {
        let bounding = &key.edges;
        let curves = self
            .doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point() && (e.style == LineKind::Solid || bounding.contains(id))
            })
            .filter_map(|(id, e)| Some((id, self.doc.shape(id)?, e.layer)))
            .collect();
        AreasInput {
            by_layer: key.by_layer,
            curves,
        }
    }

    /// With `DRIFT_CHECK_REGIONS` set (a debugging aid), the analysis as kept (cluster by
    /// cluster) is checked against the drawing analysed afresh, and any difference panics;
    /// so are the areas, each time they are found (remembered, from a thread, with
    /// centroids reused). Slow on a big drawing.
    pub(crate) fn check_caches(&mut self) {
        if !check_on() || self.areas.pending.is_some() || self.doc.in_transaction() {
            return;
        }
        let mut full = drift_solver::analyze_with(&self.doc, self.lowering().as_deref());
        full.redundant.sort();
        let a = &self.analysis();
        assert!(
            self.analyzed == Some(self.doc.revision())
                && a.converged == full.converged
                && a.dof == full.dof
                && a.dependent_equations == full.dependent_equations
                && a.conflicts == full.conflicts
                && a.redundant == full.redundant
                && a.definitions == full.definitions,
            "the analysis kept differs from the drawing's analysed afresh"
        );
    }

    /// Construction curves that shape fills: an imported fill's edges, which don't print.
    /// Areas are found with them as with drawn curves. A fill keeps only its outline (its
    /// holes are whatever lies inside it), so on a layer where a fill is bounded by
    /// construction curves, all of that layer's construction curves count: its holes' too.
    pub fn fill_edges(&self) -> std::collections::BTreeSet<EntityId> {
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

/// Whether `DRIFT_CHECK_REGIONS` is set (see [`Editor::check_caches`]).
fn check_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DRIFT_CHECK_REGIONS").is_some())
}

/// [`Editor::regions`]: shared with the thread that finds them.
pub type Regions = std::sync::Arc<Vec<drift_geom::regions::Region>>;

/// The curves areas are found among: each with its shape and layer, in document order,
/// and whether each layer's are taken on their own.
pub(crate) struct AreasInput {
    by_layer: bool,
    curves: Vec<(EntityId, Shape, drift_core::LayerId)>,
}

impl AreasInput {
    /// The same curves, to the bit.
    fn same(&self, other: &AreasInput) -> bool {
        self.by_layer == other.by_layer
            && self.curves.len() == other.curves.len()
            && self
                .curves
                .iter()
                .zip(&other.curves)
                .all(|(a, b)| a.0 == b.0 && a.2 == b.2 && drift_geom::same_shape(&a.1, &b.1))
    }

    /// The closed areas among these curves (`prior`: an earlier result, for its centroids),
    /// and the entity of each curve index in their loops.
    fn find(&self, prior: &[Region]) -> (Vec<EntityId>, Vec<Region>) {
        if !self.by_layer {
            let shapes: Vec<Shape> = self.curves.iter().map(|c| c.1).collect();
            let ids = self.curves.iter().map(|c| c.0).collect();
            return (ids, drift_geom::regions::regions_reusing(&shapes, prior));
        }
        // Each layer's curves on their own: an outline on one layer isn't closed or cut by
        // curves on another.
        let mut by_layer: std::collections::BTreeMap<drift_core::LayerId, Vec<(EntityId, Shape)>> =
            Default::default();
        for &(id, shape, layer) in &self.curves {
            by_layer.entry(layer).or_default().push((id, shape));
        }
        let mut ids = Vec::new();
        let mut all = Vec::new();
        for (_, curves) in by_layer {
            let offset = ids.len();
            let shapes: Vec<Shape> = curves.iter().map(|(_, s)| *s).collect();
            ids.extend(curves.iter().map(|(id, _)| *id));
            for mut r in drift_geom::regions::regions_reusing(&shapes, prior) {
                // Curve indices into this layer's curves become indices into all of them.
                for l in std::iter::once(&mut r.outer).chain(&mut r.holes) {
                    for s in &mut l.sources {
                        *s += offset;
                    }
                }
                all.push(r);
            }
        }
        (ids, all)
    }
}

type Found = (std::rc::Rc<Vec<EntityId>>, Regions);

/// Areas found lately, by the curves they were found among (undo and redo go back to
/// curves whose areas were found moments ago), and the thread finding the areas of an
/// edit's curves while the edit's fills wait to follow them.
#[derive(Default)]
pub(crate) struct AreasState {
    /// Most recent first.
    memo: std::collections::VecDeque<(std::sync::Arc<AreasInput>, Found)>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<(std::sync::Arc<AreasInput>, Worker)>,
    /// An edit committed before its fills followed it (see [`Editor::commit`]).
    pub(crate) pending: Option<PendingFollow>,
    /// Whether big edits find their areas off the edit path: `None` decides by the
    /// drawing's size.
    pub(crate) defer: Option<bool>,
}

#[cfg(not(target_arch = "wasm32"))]
type Worker = std::thread::JoinHandle<(Vec<EntityId>, Vec<Region>)>;

/// An edit committed with its fills still to follow (they then join its undo step).
pub(crate) struct PendingFollow {
    /// The history as the commit left it: the step to amend is still the last one.
    pub(crate) depth: usize,
    pub(crate) recorded: u64,
}

impl AreasState {
    /// How many results are kept: the current curves', and the last few before them.
    const MEMO: usize = 3;

    fn remember(&mut self, input: std::sync::Arc<AreasInput>, found: Found) {
        self.memo.retain(|(i, _)| !i.same(&input));
        self.memo.push_front((input, found));
        self.memo.truncate(Self::MEMO);
    }

    /// Whether the areas among `input` are known (found before).
    pub(crate) fn knows(&self, input: &AreasInput) -> bool {
        self.memo.iter().any(|(i, _)| i.same(input))
    }

    /// The areas among `input`: as found before, from the thread finding them, or found
    /// now (`prior`: the last areas found, for their centroids).
    fn found(&mut self, input: AreasInput, prior: Option<&Regions>) -> Found {
        if let Some(k) = self.memo.iter().position(|(i, _)| i.same(&input)) {
            let hit = self.memo.remove(k).unwrap();
            let out = hit.1.clone();
            self.memo.push_front(hit);
            return out;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.worker.as_ref().is_some_and(|(i, _)| i.same(&input)) {
            let (input, handle) = self.worker.take().unwrap();
            if let Ok((ids, regions)) = handle.join() {
                let out = (std::rc::Rc::new(ids), std::sync::Arc::new(regions));
                self.remember(input, out.clone());
                return out;
            }
        }
        let (ids, regions) = input.find(prior.map_or(&[][..], |r| &r[..]));
        let out = (std::rc::Rc::new(ids), std::sync::Arc::new(regions));
        self.remember(std::sync::Arc::new(input), out.clone());
        out
    }

    /// Start finding the areas among `input` on a thread (`prior`: the last areas found,
    /// for their centroids). On the web, where there are no threads, they are found when
    /// asked for.
    pub(crate) fn start(&mut self, input: AreasInput, prior: Option<Regions>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let input = std::sync::Arc::new(input);
            let work = input.clone();
            let handle = std::thread::Builder::new()
                .name("areas".into())
                .spawn(move || work.find(prior.as_deref().map_or(&[][..], |r| &r[..])));
            match handle {
                Ok(handle) => self.worker = Some((input, handle)),
                Err(e) => log::warn!("couldn't start finding areas on a thread: {e}"),
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (input, prior);
    }

    /// Whether no thread is at work any more (its areas, when it is done, are kept with
    /// the others).
    pub(crate) fn worker_done(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.worker.as_ref().is_some_and(|(_, h)| h.is_finished()) {
                let (input, handle) = self.worker.take().unwrap();
                if let Ok((ids, regions)) = handle.join() {
                    self.remember(input, (std::rc::Rc::new(ids), std::sync::Arc::new(regions)));
                }
            }
            self.worker.is_none()
        }
        #[cfg(target_arch = "wasm32")]
        true
    }
}

/// What [`Editor::regions`] were found from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RegionsKey {
    /// [`Document::geometry`](drift_core::Document::geometry).
    geometry: u64,
    by_layer: bool,
    /// [`Editor::fill_edges`].
    edges: BTreeSet<EntityId>,
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
            Self::Turn { pivot, degrees } => pivot + exact_rotor(degrees).rotate(p - pivot),
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
