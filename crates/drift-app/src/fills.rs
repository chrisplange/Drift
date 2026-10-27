//! Fills and hatches: finding each fill's region as the geometry changes, creating and editing
//! fills, and turning them into GPU meshes and hatch lines.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use drift_core::{
    Bounds, DVec2, EntityId, Fill, FillId, FillKind, InstanceId, LayerId, PathId, Rgb, Shape,
    SketchItem,
};
use drift_draft::{Pattern, Placement};
use drift_geom::regions::{Region, region_at};
use drift_render::FillMesh;

use crate::editor::Editor;

/// Hatches denser than this on screen are drawn as a tint instead (no moiré).
const MIN_HATCH_SPACING_PX: f64 = 2.5;
/// Most hatch segments generated for one fill per frame.
const HATCH_SEGMENT_LIMIT: usize = 40_000;

/// A fill's current region. `lost` means its boundary curves no longer enclose an area, so the
/// region shown is the last one found (or the area around its seed).
#[derive(Clone, Debug)]
pub struct FillShape {
    pub region: Region,
    pub lost: bool,
    /// What moves with the fill: its outline curves and the curves around its holes. Not
    /// the curves of another shape whose edges run along its outline.
    pub curves: Vec<EntityId>,
    /// Kept whole across curves that cross it: it covers more than one area.
    pub crossed: bool,
    /// For a fill flagged as [`divided`](Fill::divided), how many areas it covers.
    pub parts: usize,
}

/// What an edit did to a fill beyond following its outline, for a notice and the agent's
/// result.
#[derive(Clone, Debug, PartialEq)]
pub enum FillChange {
    /// The edit left it covering `parts` areas: it was kept whole and flagged.
    Divided { id: FillId, parts: usize },
    /// It took the area around its seed (`before` is its area before the edit, if known).
    Refound {
        id: FillId,
        before: Option<f64>,
        after: f64,
    },
    /// Curves of its outline were deleted and no area was left around it: it went.
    Dropped { id: FillId },
    /// The edit brought it onto the same area as `into` (drawn on top), which kept it: it
    /// went.
    Merged { id: FillId, into: FillId },
    /// The edit cut its shape's outline open and what is left bounds no area around it: it
    /// keeps its last shape until the outline closes again.
    Opened { id: FillId },
}

/// Which fills and shapes each curve is in: what [`Editor::shape_check`] looks things up
/// in, made once for many checks.
#[derive(Default)]
pub(crate) struct CurveUsers {
    /// The fills whose outline has the curve.
    fills: HashMap<EntityId, Vec<FillId>>,
    /// The shape (path) the curve is in.
    paths: HashMap<EntityId, PathId>,
    /// The drawing's size (its largest coordinate), as the regions' tolerance is set by.
    extent: f64,
}

impl CurveUsers {
    pub(crate) fn of(doc: &drift_core::Document) -> Self {
        let mut users = Self::default();
        for (f, fill) in doc.fills() {
            for c in &fill.boundary {
                users.fills.entry(*c).or_default().push(f);
            }
        }
        for (pid, path) in doc.paths() {
            for c in &path.curves {
                users.paths.insert(*c, pid);
            }
        }
        users.extent = doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .filter_map(|(id, _)| doc.shape(id))
            .map(|s| {
                let b = s.bounds();
                b.min.abs().max(b.max.abs()).max_element()
            })
            .filter(|e| e.is_finite())
            .fold(0.0, f64::max);
        users
    }
}

/// Whether an area fill's outline makes a shape (see [`Editor::shape_check`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ShapeCheck {
    /// It is the outline of this closed shape.
    Path(PathId),
    /// It is this closed chain of free curves, in order round it.
    Chain(Vec<EntityId>),
    Not(NotAShape),
}

/// The last [`Editor::fill_kind`] found: the document revision, the fill, and its kind.
pub(crate) type FillKindFound = (u64, FillId, Result<(), NotAShape>);

/// Why a fill is an area fill and not a shape (#202).
#[derive(Clone, Debug, PartialEq)]
pub enum NotAShape {
    /// A wall's poché.
    Wall,
    /// Bounded by walls (a room's floor).
    Walls,
    /// Its shape's outline is cut open.
    Open,
    /// Curves of its outline are part of another shape.
    InAShape,
    /// This many curves of its outline also bound other fills (rooms sharing a wall).
    Shared(usize),
    /// Its outline's corners lie on each other but aren't joined: these end points.
    Corners(Vec<(EntityId, EntityId)>),
    /// Its outline runs along curves that cross it or go on past it.
    Crossing,
    /// It could be one, but hasn't been edited since it was opened or imported.
    NotEdited,
}

impl NotAShape {
    /// Why, in a few words, for the Inspector and the status line.
    pub fn reason(&self) -> String {
        match self {
            NotAShape::Wall => "a wall's poché".into(),
            NotAShape::Walls => "bounded by walls".into(),
            NotAShape::Open => "its shape's outline is cut open".into(),
            NotAShape::InAShape => "its lines are part of another shape".into(),
            NotAShape::Shared(1) => "1 line shared with other fills".into(),
            NotAShape::Shared(n) => format!("{n} lines shared with other fills"),
            NotAShape::Corners(p) if p.len() == 1 => "a corner not joined".into(),
            NotAShape::Corners(p) => format!("{} corners not joined", p.len()),
            NotAShape::Crossing => "lines run past it".into(),
            NotAShape::NotEdited => "opened or imported as an area: edit it to make a shape".into(),
        }
    }
}

/// The areas among `regions` that make up `whole` (a fill kept whole across curves that
/// divide it).
fn parts_of<'a>(whole: &Region, regions: &'a [Region]) -> Vec<&'a Region> {
    let b = whole.bounds;
    let pad = 1e-6 * b.min.abs().max(b.max.abs()).max_element().max(1.0);
    regions
        .iter()
        .filter(|r| {
            r.area <= whole.area * (1.0 + 1e-9)
                && r.bounds.min.x >= b.min.x - pad
                && r.bounds.min.y >= b.min.y - pad
                && r.bounds.max.x <= b.max.x + pad
                && r.bounds.max.y <= b.max.y + pad
                && whole.contains(crate::editor::interior_point(r, r.centroid))
        })
        .collect()
}

/// A fill's outline curves and the curves of its region's holes, sorted. `ids` is the entity
/// of each curve index of the curves `region` was found from.
fn curves_of(fill: &Fill, region: &Region, ids: &[EntityId]) -> Vec<EntityId> {
    let mut c: Vec<EntityId> = region
        .holes
        .iter()
        .flat_map(|lp| lp.sources.iter().filter_map(|&i| ids.get(i).copied()))
        .chain(fill.boundary.iter().copied())
        .collect();
    c.sort_unstable();
    c.dedup();
    c
}

/// A fill of a copy of a block, in the world: the block's `index`th fill, placed with the copy.
#[derive(Clone, Debug)]
pub struct CopyFill {
    pub instance: InstanceId,
    pub index: usize,
    pub fill: Fill,
    pub region: Region,
}

/// How new fills look (the Fill tool's settings), and the editable part of a fill: in
/// drift-core, where walls keep their poché in it too.
pub use drift_core::FillStyle;

/// The hatch patterns available: the built-in library plus the document's imported ones.
#[derive(Default)]
pub struct PatternLibrary {
    source: String,
    custom: Vec<Pattern>,
}

impl PatternLibrary {
    /// Refresh from the document's imported patterns (cheap when unchanged).
    pub fn update(&mut self, custom: &str) {
        if self.source != custom {
            self.source = custom.to_owned();
            self.custom = drift_draft::parse_pat(custom).unwrap_or_default();
        }
    }

    pub fn get(&self, name: &str) -> Option<&Pattern> {
        self.custom
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .or_else(|| drift_draft::find(name))
    }

    /// Imported patterns first, then the built-in ones.
    pub fn all(&self) -> impl Iterator<Item = &Pattern> {
        self.custom
            .iter()
            .chain(drift_draft::builtin().iter().filter(|b| {
                !self
                    .custom
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(&b.name))
            }))
    }

    pub fn custom(&self) -> &[Pattern] {
        &self.custom
    }
}

/// Regions with lookups by their outlines, so finding many fills' regions doesn't scan
/// every region for each fill. `ids` is the entity of each curve index of the curves the
/// regions were found from.
struct Areas<'a> {
    regions: &'a [Region],
    ids: &'a [EntityId],
    /// Regions (indices, in order) by their sorted outer curves.
    by_outline: HashMap<Vec<EntityId>, Vec<usize>>,
    /// Regions (indices, in order) by each curve of their outer boundary.
    by_curve: HashMap<EntityId, Vec<usize>>,
    /// Every curve the regions were found from.
    curves: HashSet<EntityId>,
}

impl<'a> Areas<'a> {
    fn new(regions: &'a [Region], ids: &'a [EntityId]) -> Self {
        let mut by_outline: HashMap<Vec<EntityId>, Vec<usize>> = HashMap::new();
        let mut by_curve: HashMap<EntityId, Vec<usize>> = HashMap::new();
        for (i, r) in regions.iter().enumerate() {
            let mut c: Vec<EntityId> = r.outer.curves().into_iter().map(|k| ids[k]).collect();
            c.sort_unstable();
            let mut distinct = c.clone();
            distinct.dedup();
            for id in distinct {
                by_curve.entry(id).or_default().push(i);
            }
            by_outline.entry(c).or_default().push(i);
        }
        Self {
            regions,
            ids,
            by_outline,
            by_curve,
            curves: ids.iter().copied().collect(),
        }
    }

    /// Of `candidates`, the one around `seed`, else the one whose centre is nearest it.
    fn nearest(&self, candidates: &[usize], seed: DVec2) -> Option<&'a Region> {
        let regions = self.regions;
        candidates
            .iter()
            .map(|&i| &regions[i])
            .find(|r| r.contains(seed))
            .or_else(|| {
                candidates.iter().map(|&i| &regions[i]).min_by(|a, b| {
                    a.centroid
                        .distance(seed)
                        .total_cmp(&b.centroid.distance(seed))
                })
            })
    }

    /// The region bounded by exactly a fill's curves: the one around its seed if several;
    /// else, when an edit cut a curve out of the fill's area (a shape trimmed against the
    /// one it overlapped), the area around the seed if its outline is part of the fill's
    /// curves; else the nearest.
    fn exact(&self, fill: &Fill) -> Option<&'a Region> {
        let candidates = self.by_outline.get(&fill.boundary)?;
        let (regions, ids) = (self.regions, self.ids);
        candidates
            .iter()
            .map(|&i| &regions[i])
            .find(|r| r.contains(fill.seed))
            .or_else(|| {
                region_at(regions, fill.seed).filter(|r| {
                    outline_of(r, ids)
                        .iter()
                        .all(|e| fill.boundary.binary_search(e).is_ok())
                })
            })
            .or_else(|| self.nearest(candidates, fill.seed))
    }

    /// The region a fill's outline became when an edit replaced or split some of its curves
    /// (a fillet or a chamfer cut into a corner, a side split in two): one whose outline
    /// still has every curve of the fill's outline that bounds areas, and more. The one
    /// around the seed if several, else the nearest.
    fn successor(&self, fill: &Fill) -> Option<&'a Region> {
        let kept: Vec<EntityId> = fill
            .boundary
            .iter()
            .copied()
            .filter(|e| self.curves.contains(e))
            .collect();
        let first = kept.first()?;
        let candidates: Vec<usize> = self
            .by_curve
            .get(first)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| {
                let outline = outline_of(&self.regions[i], self.ids);
                kept.iter().all(|e| outline.binary_search(e).is_ok())
            })
            .collect();
        self.nearest(&candidates, fill.seed)
    }

    /// Pick the region a fill belongs to: the one bounded by exactly its curves (nearest its
    /// seed if several), else the one its outline became, else the one around `near` (a
    /// point inside its last shape, so it follows a shape moving off the area it was in),
    /// else the one around its seed; the last two only if they share a boundary curve with
    /// it.
    fn find(&self, fill: &Fill, near: Option<DVec2>) -> Option<&'a Region> {
        self.exact(fill)
            .or_else(|| self.successor(fill))
            .or_else(|| near.and_then(|p| self.sharing_at(p, fill)))
            .or_else(|| self.sharing_at(fill.seed, fill))
    }

    /// The region around `p`, if its outline shares a curve with the fill's.
    fn sharing_at(&self, p: DVec2, fill: &Fill) -> Option<&'a Region> {
        region_at(self.regions, p).filter(|r| {
            r.outer
                .curves()
                .into_iter()
                .any(|i| fill.boundary.binary_search(&self.ids[i]).is_ok())
        })
    }
}

/// The end points of a curve (none for a circle or a whole ellipse).
fn curve_ends(doc: &drift_core::Document, c: EntityId) -> Option<[EntityId; 2]> {
    doc.get(c).and_then(|e| e.kind.ends())
}

/// A sorted, distinct copy of a path's curves: a fill's boundary.
fn sorted(curves: &[EntityId]) -> Vec<EntityId> {
    let mut c = curves.to_vec();
    c.sort_unstable();
    c.dedup();
    c
}

/// The sorted, distinct entities of a region's outer boundary.
fn outline_of(region: &Region, ids: &[EntityId]) -> Vec<EntityId> {
    let mut c: Vec<EntityId> = region.outer.curves().into_iter().map(|i| ids[i]).collect();
    c.sort_unstable();
    c.dedup();
    c
}

/// [`Editor::fill_shapes`] as last found, and what it was found for.
pub(crate) struct FillShapesCache {
    revision: u64,
    structure: u64,
    /// The transaction it was found in, if any.
    serial: Option<u64>,
    shapes: Rc<BTreeMap<FillId, FillShape>>,
}

/// The sketch's solid curves with their bounds, in document order: what
/// [`Editor::uncrossed_region`] finds a crossed fill's area among.
type SolidCurves = Vec<(EntityId, Shape, Bounds)>;

impl Editor {
    /// Every fill's region, as of the current document revision.
    pub fn fill_shapes(&mut self) -> Rc<BTreeMap<FillId, FillShape>> {
        let revision = self.doc.revision();
        if let Some(cache) = &self.fill_shapes
            && cache.revision == revision
        {
            return cache.shapes.clone();
        }
        let fills: Vec<(FillId, Fill)> = self.doc.fills().map(|(id, f)| (id, f.clone())).collect();
        let walls = self.doc.has_walls().then(|| self.wall_owners());
        // During a drag the revision changes every frame. A click-fill none of whose curves
        // moved keeps the area found before (finding the drawing's areas takes far longer
        // than a frame); every area is found again once the transaction ends.
        let mut kept = self.kept_fill_shapes();
        // The drawing's areas are found only when a fill needs them (not for shapes' fills
        // and walls' pochés, which know their own).
        let needs_areas = fills.iter().any(|(id, f)| {
            f.path.is_none()
                && !walls.as_ref().is_some_and(|w| w.fills.contains_key(id))
                && !kept.contains_key(id)
        });
        let (regions, ids) = if needs_areas {
            (self.regions(), self.region_ids())
        } else {
            (Rc::new(Vec::new()), Rc::new(Vec::new()))
        };
        let areas = Areas::new(&regions, &ids);
        let mut solid = None;
        let mut out = BTreeMap::new();
        for (id, fill) in fills {
            // A wall cluster's poché: its outline and holes, known exactly (nothing else in
            // the drawing is looked at).
            if walls.as_ref().is_some_and(|w| w.fills.contains_key(&id)) {
                if let Some(region) = self.wall_fill_region(id) {
                    let mut curves: Vec<EntityId> = region_curves_of(&self.doc, id);
                    curves.sort_unstable();
                    let shape = FillShape {
                        region,
                        lost: false,
                        curves,
                        crossed: false,
                        parts: 0,
                    };
                    self.last_fill_shapes.insert(id, shape.clone());
                    out.insert(id, shape);
                } else if let Some(last) = self.last_fill_shapes.get(&id) {
                    out.insert(
                        id,
                        FillShape {
                            lost: true,
                            ..last.clone()
                        },
                    );
                }
                continue;
            }
            // A shape's fill (of a path) is the area inside that path alone.
            if let Some(pid) = fill.path
                && let Some(curves) = self.doc.path(pid).map(|p| sorted(&p.curves))
            {
                let solid = solid.get_or_insert_with(|| self.solid_curves());
                let shape = self
                    .path_region_and_holes(pid, solid)
                    .map(|(region, holes)| {
                        // (Its holes as of the last edit, not part-way through one.)
                        if !self.doc.in_transaction() {
                            self.shape_holes.insert(id, holes);
                        }
                        FillShape {
                            region,
                            lost: false,
                            curves,
                            crossed: false,
                            parts: 0,
                        }
                    });
                if let Some(shape) = shape {
                    self.fill_crossed.insert(id, false);
                    self.last_fill_shapes.insert(id, shape.clone());
                    out.insert(id, shape);
                } else if let Some(region) = self.open_path_region(pid) {
                    // Its path is open with nothing closed round it: it shows its outline
                    // closed straight across the gap, found from the drawing alone (so the
                    // same once saved and opened again) until the ends meet.
                    let curves =
                        sorted(&self.doc.path(pid).map_or(Vec::new(), |p| p.curves.clone()));
                    out.insert(
                        id,
                        FillShape {
                            region,
                            lost: true,
                            curves,
                            crossed: false,
                            parts: 0,
                        },
                    );
                } else if let Some(last) = self.last_fill_shapes.get(&id) {
                    // (An outline open in more than one place: its last shape.)
                    out.insert(
                        id,
                        FillShape {
                            lost: true,
                            ..last.clone()
                        },
                    );
                }
                continue;
            }
            if let Some(shape) = kept.remove(&id) {
                out.insert(id, shape);
                continue;
            }
            // A curve drawn across a fill cuts its outline into pieces: the fill keeps its
            // whole area, found again without the curves that cross it.
            let uncrossed = if areas.exact(&fill).is_none() {
                let solid = solid.get_or_insert_with(|| self.solid_curves());
                self.uncrossed_region(&fill, solid)
            } else {
                None
            };
            let near = self
                .last_fill_shapes
                .get(&id)
                .map(|s| crate::editor::interior_point(&s.region, s.region.centroid));
            let found = uncrossed.or_else(|| {
                areas.find(&fill, near).map(|r| FillShape {
                    curves: curves_of(&fill, r, &ids),
                    region: r.clone(),
                    lost: false,
                    crossed: false,
                    parts: 0,
                })
            });
            if let Some(mut shape) = found {
                if fill.divided {
                    shape.parts = parts_of(&shape.region, &regions).len();
                }
                self.fill_crossed.insert(id, shape.crossed);
                self.last_fill_shapes.insert(id, shape.clone());
                out.insert(id, shape);
            } else if let Some(last) = self.last_fill_shapes.get(&id) {
                out.insert(
                    id,
                    FillShape {
                        lost: true,
                        ..last.clone()
                    },
                );
            } else if let Some(r) = region_at(&regions, fill.seed) {
                out.insert(
                    id,
                    FillShape {
                        curves: curves_of(&fill, r, &ids),
                        region: r.clone(),
                        lost: true,
                        crossed: false,
                        parts: 0,
                    },
                );
            }
        }
        let out = Rc::new(out);
        self.fill_shapes = Some(FillShapesCache {
            revision,
            structure: self.doc.structure(),
            serial: self
                .doc
                .in_transaction()
                .then(|| self.doc.transaction_serial()),
            shapes: out.clone(),
        });
        out
    }

    /// The click-fills' shapes from the last [`fill_shapes`](Self::fill_shapes) that still
    /// hold inside the open transaction: found in it or just before it began, with nothing
    /// added, removed or restyled since and none of the fill's curves moved.
    fn kept_fill_shapes(&self) -> BTreeMap<FillId, FillShape> {
        let Some(cache) = &self.fill_shapes else {
            return BTreeMap::new();
        };
        let current = self.doc.in_transaction()
            && cache.structure == self.doc.structure()
            && (cache.serial == Some(self.doc.transaction_serial())
                || (cache.serial.is_none() && cache.revision == self.doc.revision_at_begin()));
        if !current {
            return BTreeMap::new();
        }
        let moved = |curves: &[EntityId]| {
            curves.iter().any(|c| self.doc.touched_in_transaction(*c))
                || self
                    .doc
                    .defining_points(curves.iter().copied())
                    .iter()
                    .any(|p| self.doc.touched_in_transaction(*p))
        };
        // (A shape's area also changes with what is drawn or moved inside it: kept only when
        // nothing near it changed.)
        let boxes = self.touched_boxes();
        cache
            .shapes
            .iter()
            .filter(|(id, s)| {
                self.doc.fill(**id).is_some_and(|f| {
                    f.path.is_none()
                        || (!s.lost && self.shape_untouched(&s.curves, &s.region.bounds, &boxes))
                })
            })
            .filter(|(_, s)| !moved(&s.curves))
            .map(|(id, s)| (*id, s.clone()))
            .collect()
    }

    /// Keep each fill on its area as an edit changes it, inside the edit's open transaction
    /// (so undo takes it back with the edit): when the edit replaced or split curves of a
    /// fill's outline, the area's new outline becomes the fill's, and a seed the edit left
    /// outside the area (a moved corner, a fillet over it) moves back in.
    ///
    /// A fill the edit left covering several areas (a curve drawn across it, a split that
    /// made a junction) is kept whole and flagged as [`divided`](Fill::divided), or, with
    /// [`follow_divided_fills`](drift_core::DrawingRules::follow_divided_fills), takes the area around its
    /// seed. One that was already kept whole across curves stays so. When the edit deleted
    /// or reshaped curves of its outline and no area has what is left of it, the fill takes
    /// the area around its seed (#120, #101), or goes if there is none. A shape's fill whose
    /// outline, or a hole's, the edit cut open takes the area around it that what is left of
    /// its outline still bounds (#201), or keeps its last shape. What happened beyond
    /// following an outline is noted in [`fill_changes`](Editor::fill_changes) and the
    /// notice.
    pub(crate) fn follow_fills(&mut self) {
        if self.doc.fills().next().is_none() {
            return;
        }
        let fills: Vec<(FillId, Fill)> = self.doc.fills().map(|(id, f)| (id, f.clone())).collect();
        let walls = self.doc.has_walls().then(|| self.wall_owners());
        // The inside of each closed shape's outline, with its holes, and whether the edit
        // cut one of them open.
        // (A shape the edit left alone, with nothing changed near it, is as last found:
        // only the others are found again, among the drawing's solid curves found once.)
        let boxes = self.touched_boxes();
        let mut shape_solid: Option<SolidCurves> = None;
        let mut insides: HashMap<FillId, (Region, Vec<EntityId>, bool)> = HashMap::new();
        for (id, f) in &fills {
            if walls.as_ref().is_some_and(|w| w.fills.contains_key(id)) {
                continue;
            }
            let Some(path) = f
                .path
                .and_then(|pid| self.doc.path(pid))
                .filter(|p| p.closed)
            else {
                continue;
            };
            let curves = sorted(&path.curves);
            if let (Some(last), Some(holes)) =
                (self.last_fill_shapes.get(id), self.shape_holes.get(id))
                && !last.lost
                && last.curves == curves
                && self.shape_untouched(&curves, &last.region.bounds, &boxes)
            {
                insides.insert(*id, (last.region.clone(), holes.clone(), false));
                continue;
            }
            let solid = shape_solid.get_or_insert_with(|| self.solid_curves());
            if let Some((region, holes)) = self.path_region_and_holes(f.path.unwrap(), solid) {
                let opened = self.shape_hole_opened(*id, &region, &holes);
                insides.insert(*id, (region, holes, opened));
            }
        }
        // (Closed shapes' fills, unless the edit cut a hole of theirs open, and walls'
        // pochés don't need the drawing's areas.)
        let needs_areas = fills.iter().any(|(id, _)| {
            !walls.as_ref().is_some_and(|w| w.fills.contains_key(id))
                && insides.get(id).is_none_or(|(_, _, opened)| *opened)
        });
        let (regions, ids) = if needs_areas {
            (self.regions(), self.region_ids())
        } else {
            (Rc::new(Vec::new()), Rc::new(Vec::new()))
        };
        let areas = Areas::new(&regions, &ids);
        let mut solid = None;
        let mut created: Option<Vec<EntityId>> = None;
        let mut changes = Vec::new();
        // Fills whose outline this edit changed.
        let mut moved: HashSet<FillId> = HashSet::new();
        // Area fills the edit made or touched: they become shapes where their outline makes
        // one (#202).
        let mut touched: Vec<FillId> = Vec::new();
        for (id, fill) in fills {
            // (Walls keep their pochés themselves.)
            if walls.as_ref().is_some_and(|w| w.fills.contains_key(&id)) {
                continue;
            }
            // A shape's fill keeps to its path: its outline is the path's curves, it is
            // never divided, and its seed stays inside.
            if let Some(pid) = fill.path
                && let Some(curves) = self.doc.path(pid).map(|p| sorted(&p.curves))
            {
                let inside = insides.get(&id);
                if let Some((_, holes, _)) = inside {
                    self.shape_holes.insert(id, holes.clone());
                }
                let mut followed = fill.clone();
                followed.divided = false;
                let opened = inside.is_none_or(|(_, _, opened)| *opened);
                // Its outline, or a hole's, cut open (a ring trimmed through, #201): no
                // longer a shape, it is the area fill of the area that what is left still
                // closes around it.
                if opened && let Some(r) = self.open_shape_area(id, &fill, &curves, &regions, &ids)
                {
                    followed.path = None;
                    followed.boundary = outline_of(r, &ids);
                    followed.seed = crate::editor::interior_point(r, fill.seed);
                    changes.push(FillChange::Refound {
                        id,
                        before: self.last_fill_shapes.get(&id).map(|s| s.region.area),
                        after: r.area,
                    });
                    self.last_fill_shapes.insert(
                        id,
                        FillShape {
                            curves: curves_of(&followed, r, &ids),
                            region: r.clone(),
                            lost: false,
                            crossed: false,
                            parts: 0,
                        },
                    );
                    self.doc.set_fill(id, followed);
                    self.fill_crossed.insert(id, false);
                    moved.insert(id);
                    touched.push(id);
                    continue;
                }
                followed.boundary = curves.clone();
                if let Some((region, _, _)) = inside {
                    followed.seed = crate::editor::interior_point(region, fill.seed);
                    // Kept for the next edit whether or not a frame is drawn in between
                    // (the assistant's edits).
                    self.last_fill_shapes.insert(
                        id,
                        FillShape {
                            region: region.clone(),
                            lost: false,
                            curves,
                            crossed: false,
                            parts: 0,
                        },
                    );
                } else if self
                    .doc
                    .path_before_transaction(pid)
                    .is_some_and(|p| p.closed)
                {
                    // Cut open with nothing closed around it: it keeps its last shape until
                    // the ends meet again, and says so.
                    changes.push(FillChange::Opened { id });
                }
                self.doc.set_fill(id, followed);
                self.fill_crossed.insert(id, false);
                continue;
            }
            let reshaped = fill
                .boundary
                .iter()
                .any(|e| self.doc.reshaped_in_transaction(*e));
            let deleted = fill.boundary.iter().any(|e| !self.doc.contains(*e));
            // Or its curves were moved: a point of theirs was.
            let shifted = reshaped
                || self
                    .doc
                    .defining_points(fill.boundary.iter().copied())
                    .iter()
                    .any(|p| self.doc.reshaped_in_transaction(*p));
            let before = self.last_fill_shapes.get(&id).map(|s| s.region.area);
            // A point inside its last shape: what it follows when its outline is gone.
            let near = self
                .last_fill_shapes
                .get(&id)
                .map(|s| crate::editor::interior_point(&s.region, s.region.centroid));
            // Whether it was kept whole across curves before this edit (unknown for a fill
            // not looked at since it was made or opened).
            let was_crossed = self.fill_crossed.get(&id).copied();
            // The curves round its holes before this edit, and whether the edit cut one of
            // them open and the area `curves` bounds took that hole in: the hole's area was
            // never the fill's (#201).
            let holes: Vec<EntityId> = self
                .last_fill_shapes
                .get(&id)
                .map(|s| {
                    s.curves
                        .iter()
                        .copied()
                        .filter(|c| fill.boundary.binary_search(c).is_err())
                        .collect()
                })
                .unwrap_or_default();
            let floods_hole = |curves: &[EntityId]| {
                holes.iter().any(|c| {
                    self.doc.reshaped_in_transaction(*c)
                        && self.doc.contains(*c)
                        && curves.binary_search(c).is_err()
                })
            };
            let mut followed = fill.clone();
            let mut crossed = false;
            let region = if let Some(r) = areas.exact(&fill) {
                followed.boundary = outline_of(r, &ids);
                followed.divided = false;
                r.clone()
            } else if let Some((whole, outline)) = self
                .uncrossed_region(&fill, solid.get_or_insert_with(|| self.solid_curves()))
                .filter(|s| !floods_hole(&s.curves))
                .map(|s| (s.region, None))
                .or_else(|| {
                    reshaped
                        .then(|| {
                            let created = created.get_or_insert_with(|| {
                                self.doc
                                    .created_in_transaction()
                                    .into_iter()
                                    .filter(|e| {
                                        self.doc.get(*e).is_some_and(|e| !e.kind.is_point())
                                    })
                                    .collect()
                            });
                            let solid = solid.get_or_insert_with(|| self.solid_curves());
                            self.reshaped_whole(&fill, solid, created)
                        })
                        .flatten()
                        .map(|(r, o)| (r, Some(o)))
                })
            {
                let at_seed =
                    region_at(&regions, fill.seed).filter(|r| r.area < whole.area * (1.0 - 1e-9));
                let newly = at_seed.is_some()
                    && was_crossed == Some(false)
                    && !fill.divided
                    && !self.doc.fill_created_in_transaction(id);
                // A curve drawn across an intact outline never shrinks its fill: it keeps
                // its whole area, marked as covering several. The preference to follow the
                // seed applies only when the edit cut the outline itself (`reshaped`).
                if newly
                    && self.doc.rules().follow_divided_fills
                    && reshaped
                    && let Some(part) = at_seed
                {
                    followed.boundary = outline_of(part, &ids);
                    changes.push(FillChange::Refound {
                        id,
                        before,
                        after: part.area,
                    });
                    part.clone()
                } else {
                    if let Some(outline) = outline {
                        followed.boundary = outline;
                    }
                    crossed = at_seed.is_some();
                    if !crossed {
                        followed.divided = false;
                        if deleted {
                            changes.push(FillChange::Refound {
                                id,
                                before,
                                after: whole.area,
                            });
                        }
                    } else if newly {
                        followed.divided = true;
                        changes.push(FillChange::Divided {
                            id,
                            parts: parts_of(&whole, &regions).len(),
                        });
                    }
                    whole
                }
            } else if let Some(r) = areas
                .successor(&fill)
                // What is left of an outline the edit deleted curves of may bound another
                // area (a shape in its corner): not one the fill's seed isn't in (#135).
                .filter(|r| !deleted || r.contains(fill.seed))
            {
                followed.boundary = outline_of(r, &ids);
                followed.divided = false;
                r.clone()
            } else if shifted && let Some(r) = near.and_then(|p| areas.sharing_at(p, &fill)) {
                // Its outline is gone (a shape it was bounded by moved off the area, or it
                // moved off another): it follows the shape it was in.
                followed.boundary = outline_of(r, &ids);
                followed.divided = false;
                if deleted {
                    changes.push(FillChange::Refound {
                        id,
                        before,
                        after: r.area,
                    });
                }
                r.clone()
            } else if reshaped && let Some(r) = region_at(&regions, fill.seed) {
                followed.boundary = outline_of(r, &ids);
                followed.divided = false;
                changes.push(FillChange::Refound {
                    id,
                    before,
                    after: r.area,
                });
                r.clone()
            } else {
                if reshaped && deleted {
                    // Curves of its outline were deleted and no area is left around it.
                    self.doc.remove_fill(id);
                    changes.push(FillChange::Dropped { id });
                }
                continue;
            };
            followed.seed = crate::editor::interior_point(&region, fill.seed);
            if followed.boundary != fill.boundary {
                moved.insert(id);
            }
            if shifted
                || deleted
                || followed.boundary != fill.boundary
                || self.doc.fill_created_in_transaction(id)
            {
                touched.push(id);
            }
            self.doc.set_fill(id, followed);
            self.fill_crossed.insert(id, crossed);
        }
        // Fills the edit brought onto one area (two filled shapes trimmed into one outline,
        // the wall between two filled rooms deleted): the one drawn on top keeps it, the
        // others go.
        let mut by_outline: BTreeMap<Vec<EntityId>, Vec<FillId>> = BTreeMap::new();
        for id in self.fills_in_order() {
            if let Some(f) = self.doc.fill(id)
                && f.path.is_none()
            {
                by_outline.entry(f.boundary.clone()).or_default().push(id);
            }
        }
        for ids in by_outline.into_values() {
            let Some((&into, below)) = ids.split_last() else {
                continue;
            };
            if below.is_empty() || !ids.iter().any(|id| moved.contains(id)) {
                continue;
            }
            for &id in below {
                self.doc.remove_fill(id);
                changes.push(FillChange::Merged { id, into });
            }
        }
        self.make_shapes(&touched, walls.as_deref(), &mut changes);
        if !changes.is_empty() {
            if self.notice.is_none() {
                self.notice = Some(self.describe_fill_changes(&changes));
            }
            self.fill_changes.extend(changes);
        }
    }

    /// Delete entities inside an open transaction, as [`Document::delete`] does. A fill or
    /// hatch whose whole outline goes with them stays when the same outline is drawn again
    /// (a shape drawn twice, #134): it takes the copy that is left. Otherwise it goes with
    /// them.
    ///
    /// [`Document::delete`]: drift_core::Document::delete
    pub(crate) fn delete_curves(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        // (Shapes' fills keep to their paths, which a delete takes apart: not those.)
        let shapes = self.fill_shapes();
        let areas: Vec<(FillId, Region)> = self
            .doc
            .fills()
            .filter(|(_, f)| f.path.is_none())
            .filter_map(|(id, _)| Some((id, shapes.get(&id)?.region.clone())))
            .collect();
        let fills: Vec<(FillId, Fill)> = areas
            .iter()
            .filter_map(|(id, _)| Some((*id, self.doc.fill(*id)?.clone())))
            .collect();
        self.doc.delete(ids);
        let gone: Vec<(FillId, Fill, Region)> = fills
            .into_iter()
            .zip(areas)
            .filter(|((id, _), _)| self.doc.fill(*id).is_none())
            .map(|((id, fill), (_, region))| (id, fill, region))
            .collect();
        if gone.is_empty() {
            return;
        }
        let regions = self.regions();
        let ids = self.region_ids();
        for (id, mut fill, was) in gone {
            let Some(twin) = region_at(&regions, fill.seed) else {
                continue;
            };
            let size = was.area.abs().sqrt().max(1.0);
            let same = (twin.area - was.area).abs() <= 1e-9 * size * size
                && twin.centroid.distance(was.centroid) <= 1e-9 * size;
            if same {
                fill.boundary = outline_of(twin, &ids);
                let _ = self.doc.insert_fill(id, fill);
            }
        }
    }

    /// A notice for what edits did to fills.
    fn describe_fill_changes(&self, changes: &[FillChange]) -> String {
        let units = self.doc.units();
        let what = |id: FillId| match self.doc.fill(id).map(|f| &f.kind) {
            Some(FillKind::Hatch(_)) => "A hatch",
            _ => "A fill",
        };
        let first = match &changes[0] {
            FillChange::Divided { id, parts } => format!(
                "{} now covers {parts} areas: it was kept whole. Pick it to fit it to one area \
                 or split it",
                what(*id)
            ),
            FillChange::Refound { id, before, after } => match before {
                Some(b) if (b - after).abs() > 1e-9 * after.max(1.0) => format!(
                    "{} now fills the area around its seed ({} → {})",
                    what(*id),
                    units.format_area(*b),
                    units.format_area(*after)
                ),
                _ => format!("{} now fills the area around its seed", what(*id)),
            },
            FillChange::Dropped { .. } => {
                "A fill or hatch was removed: no area is left around it".to_owned()
            }
            FillChange::Merged { id, into } => format!(
                "{} was removed: it came to cover the same area as {into}",
                what(*id)
            ),
            FillChange::Opened { id } => format!(
                "{} keeps its shape, closed straight across the gap: its outline was cut open                  and bounds no area",
                what(*id)
            ),
        };
        match changes.len() {
            1 => first,
            n => format!("{first} (and {} more fill changes)", n - 1),
        }
    }

    /// Find each closed shape's fill's area and holes again from the drawing, as after undo
    /// or redo: what the next edit tells a hole cut open by, and a shape left alone by, is
    /// then the drawing as it is, whether or not a frame is drawn in between.
    pub(crate) fn refresh_shape_fills(&mut self) {
        self.shape_holes.clear();
        let solid = self.solid_curves();
        let shapes: Vec<(FillId, PathId, Vec<EntityId>)> = self
            .doc
            .fills()
            .filter_map(|(id, f)| {
                let pid = f.path?;
                let path = self.doc.path(pid).filter(|p| p.closed)?;
                Some((id, pid, sorted(&path.curves)))
            })
            .collect();
        for (id, pid, curves) in shapes {
            match self.path_region_and_holes(pid, &solid) {
                Some((region, holes)) => {
                    self.shape_holes.insert(id, holes);
                    self.last_fill_shapes.insert(
                        id,
                        FillShape {
                            region,
                            lost: false,
                            curves,
                            crossed: false,
                            parts: 0,
                        },
                    );
                }
                None => {
                    self.last_fill_shapes.remove(&id);
                }
            }
        }
    }

    /// Whether an area fill's outline (`boundary`, sorted) makes a shape (#202): it is one
    /// closed shape's outline, or a closed chain of free curves (each meeting the next at a
    /// shared end point; none a wall's, in another shape or round another fill). Else why
    /// not.
    pub(crate) fn shape_check(
        &self,
        index: &CurveUsers,
        id: FillId,
        boundary: &[EntityId],
    ) -> ShapeCheck {
        let Some(&first) = boundary.first() else {
            return ShapeCheck::Not(NotAShape::Crossing);
        };
        if self.doc.has_walls() && boundary.iter().any(|c| self.wall_of(*c).is_some()) {
            return ShapeCheck::Not(NotAShape::Walls);
        }
        if let Some(&pid) = index.paths.get(&first)
            && let Some(path) = self.doc.path(pid)
        {
            return if path.closed && sorted(&path.curves) == boundary {
                ShapeCheck::Path(pid)
            } else {
                ShapeCheck::Not(NotAShape::InAShape)
            };
        }
        if boundary.iter().any(|c| index.paths.contains_key(c)) {
            return ShapeCheck::Not(NotAShape::InAShape);
        }
        let shared = boundary
            .iter()
            .filter(|c| {
                index
                    .fills
                    .get(c)
                    .is_some_and(|fs| fs.iter().any(|f| *f != id))
            })
            .count();
        if shared > 0 {
            return ShapeCheck::Not(NotAShape::Shared(shared));
        }
        if let Some(chain) = self.closed_chain(boundary) {
            return ShapeCheck::Chain(chain);
        }
        // Ends that lie on each other but aren't one point: corners drawn up to each other
        // and not joined.
        let mut ends: Vec<EntityId> = boundary
            .iter()
            .filter_map(|c| curve_ends(&self.doc, *c))
            .flatten()
            .collect();
        ends.sort_unstable();
        let mut open: Vec<EntityId> = Vec::new();
        for chunk in ends.chunk_by(|a, b| a == b) {
            if chunk.len() == 1 {
                open.push(chunk[0]);
            }
        }
        let at = |p: EntityId| self.doc.point(p).ok();
        // (The regions' snap, for the whole drawing: a gap they close is a corner here.)
        let tol = drift_geom::regions::RegionTolerance::for_extent(index.extent).snap;
        let mut pairs = Vec::new();
        let mut left = open.clone();
        while let Some(a) = left.pop() {
            let Some(pa) = at(a) else { break };
            let Some(k) = left
                .iter()
                .position(|b| at(*b).is_some_and(|pb| pb.distance(pa) <= tol))
            else {
                return ShapeCheck::Not(NotAShape::Crossing);
            };
            pairs.push((a, left.swap_remove(k)));
        }
        if pairs.is_empty() || !left.is_empty() {
            ShapeCheck::Not(NotAShape::Crossing)
        } else {
            ShapeCheck::Not(NotAShape::Corners(pairs))
        }
    }

    /// Area fills that become shapes' fills after an edit, inside its open transaction
    /// (#202): of `touched` (made or changed by the edit), those whose outline is one closed
    /// shape, or a closed chain of free curves ([`shape_check`](Self::shape_check)), which
    /// becomes a shape; and any whose shape's outline the edit closed round it again (two
    /// shapes trimmed into one, #201). A shape keeps one fill: the one drawn on top. Only
    /// edits make shapes, never opening a drawing.
    fn make_shapes(
        &mut self,
        touched: &[FillId],
        walls: Option<&drift_core::walls::Owners>,
        changes: &mut Vec<FillChange>,
    ) {
        if self.importing {
            return;
        }
        let wall_fill = |f: FillId| walls.is_some_and(|w| w.fills.contains_key(&f));
        // Shapes the edit closed (again): their inside.
        let closed: Vec<(PathId, Vec<EntityId>, Region)> = self
            .doc
            .paths()
            .filter(|(pid, p)| {
                p.closed
                    && !walls
                        .is_some_and(|w| w.paths.contains_key(pid) || w.centres.contains_key(pid))
                    && (self.doc.path_created_in_transaction(*pid)
                        || self
                            .doc
                            .path_before_transaction(*pid)
                            .is_some_and(|b| !b.closed))
            })
            .map(|(pid, p)| (pid, sorted(&p.curves)))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(pid, curves)| Some((pid, curves, self.path_region(pid)?)))
            .collect();
        // (Only shapes with no fill of their own take one: a shape cut open keeps its fill,
        // and an area fill inside it stays its own when it closes again.)
        let filled: HashSet<PathId> = self.doc.fills().filter_map(|(_, f)| f.path).collect();
        let closed: Vec<(PathId, Vec<EntityId>, Region)> = closed
            .into_iter()
            .filter(|(pid, _, _)| !filled.contains(pid))
            .collect();
        let mut candidates: Vec<FillId> = touched.to_vec();
        if !closed.is_empty() {
            candidates.extend(self.doc.fills().filter_map(|(f, fill)| {
                (fill.path.is_none()
                    && closed.iter().any(|(_, curves, _)| {
                        fill.boundary
                            .iter()
                            .any(|c| curves.binary_search(c).is_ok())
                    }))
                .then_some(f)
            }));
        }
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            return;
        }
        let mut index = CurveUsers::of(&self.doc);
        for id in candidates {
            let Some(fill) = self.doc.fill(id).cloned().filter(|f| f.path.is_none()) else {
                continue;
            };
            if wall_fill(id) {
                continue;
            }
            let reclosed = closed
                .iter()
                .find(|(_, curves, inside)| {
                    inside.contains(fill.seed)
                        && fill
                            .boundary
                            .iter()
                            .any(|c| curves.binary_search(c).is_ok())
                })
                .map(|(pid, _, _)| *pid);
            let path = match reclosed {
                Some(pid) => pid,
                None => match self.shape_check(&index, id, &fill.boundary) {
                    ShapeCheck::Path(pid) => pid,
                    ShapeCheck::Chain(chain) => {
                        let Ok(pid) = self.doc.add_path(drift_core::Path {
                            curves: chain.clone(),
                            closed: true,
                        }) else {
                            continue;
                        };
                        for c in chain {
                            index.paths.insert(c, pid);
                        }
                        pid
                    }
                    ShapeCheck::Not(_) => continue,
                },
            };
            self.bind_to_shape(id, fill, path, changes);
        }
    }

    /// `curves` (sorted) in order round a closed chain, each meeting the next at a shared end
    /// point and every end shared by exactly two of them; or one whole circle or ellipse.
    fn closed_chain(&self, curves: &[EntityId]) -> Option<Vec<EntityId>> {
        let ends: Vec<(EntityId, Option<[EntityId; 2]>)> = curves
            .iter()
            .map(|c| (*c, curve_ends(&self.doc, *c)))
            .collect();
        if let [(c, None)] = ends[..] {
            return Some(vec![c]);
        }
        let mut at: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
        for (c, e) in &ends {
            for p in (*e)? {
                at.entry(p).or_default().push(*c);
            }
        }
        if at.values().any(|cs| cs.len() != 2) {
            return None;
        }
        let ends: HashMap<EntityId, [EntityId; 2]> = ends
            .into_iter()
            .filter_map(|(c, e)| Some((c, e?)))
            .collect();
        let first = curves[0];
        let [start, mut point] = ends[&first];
        let mut chain = vec![first];
        let mut prev = first;
        while point != start {
            let next = *at[&point].iter().find(|c| **c != prev)?;
            if chain.len() > curves.len() {
                return None;
            }
            chain.push(next);
            let [a, b] = ends[&next];
            point = if a == point { b } else { a };
            prev = next;
        }
        (chain.len() == curves.len()).then_some(chain)
    }

    /// Make area fill `id` the fill of closed shape `path`, inside the open transaction. A
    /// shape keeps one fill: if it has one, the one drawn on top stays and the other goes.
    fn bind_to_shape(
        &mut self,
        id: FillId,
        mut fill: Fill,
        path: PathId,
        changes: &mut Vec<FillChange>,
    ) {
        let other = self
            .doc
            .fills()
            .find(|(f, o)| *f != id && o.path == Some(path))
            .map(|(f, _)| f);
        if let Some(other) = other {
            let order = self.fills_in_order();
            let pos = |f: FillId| order.iter().position(|o| *o == f);
            if pos(other) > pos(id) {
                self.doc.remove_fill(id);
                changes.push(FillChange::Merged { id, into: other });
                return;
            }
            self.doc.remove_fill(other);
            changes.push(FillChange::Merged {
                id: other,
                into: id,
            });
        }
        let Some(curves) = self.doc.path(path).map(|p| sorted(&p.curves)) else {
            return;
        };
        fill.path = Some(path);
        fill.boundary = curves;
        fill.divided = false;
        // (Its seed is already inside: its area is the shape's inside.)
        self.doc.set_fill(id, fill);
        self.fill_crossed.insert(id, false);
    }

    /// What kind of filled thing fill `id` is, for the Inspector and the status line: a
    /// shape, or an area fill and why it isn't a shape.
    pub fn fill_kind(&self, id: FillId) -> Result<(), NotAShape> {
        // (Asked every frame by the status line and the Inspector: kept per revision.)
        let revision = self.doc.revision();
        if let Some((r, f, kind)) = &*self.fill_kind_cache.borrow()
            && *r == revision
            && *f == id
        {
            return kind.clone();
        }
        let kind = self.find_fill_kind(id);
        *self.fill_kind_cache.borrow_mut() = Some((revision, id, kind.clone()));
        kind
    }

    fn find_fill_kind(&self, id: FillId) -> Result<(), NotAShape> {
        let fill = self.doc.fill(id).ok_or(NotAShape::Crossing)?;
        if self.doc.has_walls() && self.wall_owners().fills.contains_key(&id) {
            return Err(NotAShape::Wall);
        }
        if let Some(pid) = fill.path {
            return match self.doc.path(pid) {
                Some(p) if p.closed => Ok(()),
                _ => Err(NotAShape::Open),
            };
        }
        match self.shape_check(&CurveUsers::of(&self.doc), id, &fill.boundary) {
            ShapeCheck::Not(why) => Err(why),
            // (Found that way before shapes were made of outlines, and not edited since.)
            ShapeCheck::Path(_) | ShapeCheck::Chain(_) => Err(NotAShape::NotEdited),
        }
    }

    /// The picked fill, if it is the one fill picked and an area fill whose corners touch but
    /// aren't joined ([`join_corners`](Self::join_corners) applies).
    pub fn joinable_fill(&self) -> Option<FillId> {
        let [f] = self.selected_fills.iter().copied().collect::<Vec<_>>()[..] else {
            return None;
        };
        matches!(self.fill_kind(f), Err(NotAShape::Corners(_))).then_some(f)
    }

    /// Join the corners of fill `id`'s outline that lie on each other but aren't one point
    /// (one undo step): its outline then makes a shape. Returns how many it joined.
    pub fn join_corners(&mut self, id: FillId) -> Result<usize, String> {
        let Err(NotAShape::Corners(pairs)) = self.fill_kind(id) else {
            return Err("Its outline has no corners to join".into());
        };
        self.doc.begin();
        for (a, b) in &pairs {
            if let Err(e) = self.doc.merge_points(*a, *b) {
                self.doc.rollback();
                return Err(e.to_string());
            }
        }
        if !self.commit("Join corners") {
            return Err(self
                .notice
                .take()
                .unwrap_or_else(|| "Joining its corners failed".into()));
        }
        Ok(pairs.len())
    }

    /// Whether the open edit cut open the outline of a hole in shape fill `id`: a curve that
    /// was round one of its holes when last found ([`shape_holes`](Editor::shape_holes)) was
    /// reshaped (trimmed, split) and is round none of `holes` now, the holes of the inside of
    /// its outline, `inside`. A hole moved, even across the outline, isn't cut open. With
    /// none found yet (a drawing just opened, no frame drawn), a whole circle or ellipse
    /// inside it that the edit made an arc of.
    fn shape_hole_opened(&self, id: FillId, inside: &Region, holes: &[EntityId]) -> bool {
        let opened = |c: &EntityId| {
            self.doc.reshaped_in_transaction(*c)
                && self.doc.contains(*c)
                && holes.binary_search(c).is_err()
        };
        if let Some(before) = self.shape_holes.get(&id) {
            return before.iter().any(opened);
        }
        let b = inside.bounds;
        self.doc
            .entities_touched_in_transaction()
            .into_iter()
            .filter(opened)
            .filter(|e| {
                self.doc
                    .entity_before_transaction(*e)
                    .is_some_and(|was| !was.kind.is_point() && was.kind.ends().is_none())
            })
            .filter_map(|e| self.doc.shape(e))
            .map(|s| s.bounds())
            .any(|s| {
                s.min.x >= b.min.x && s.min.y >= b.min.y && s.max.x <= b.max.x && s.max.y <= b.max.y
            })
    }

    /// The area a shape's fill takes when an edit cut the shape's outline (`curves`, sorted)
    /// or a hole's open: the one around its seed, else around a point inside its last shape,
    /// if that area's outline still has some of the shape's curves.
    fn open_shape_area<'r>(
        &self,
        id: FillId,
        fill: &Fill,
        curves: &[EntityId],
        regions: &'r [Region],
        ids: &[EntityId],
    ) -> Option<&'r Region> {
        let near = self
            .last_fill_shapes
            .get(&id)
            .map(|s| crate::editor::interior_point(&s.region, s.region.centroid));
        let bounded_by_it = |r: &&Region| {
            r.outer
                .curves()
                .into_iter()
                .any(|i| curves.binary_search(&ids[i]).is_ok())
        };
        region_at(regions, fill.seed)
            .filter(bounded_by_it)
            .or_else(|| {
                near.and_then(|p| region_at(regions, p))
                    .filter(bounded_by_it)
            })
    }

    /// Fit a fill flagged as covering several areas to the one around its seed (one undo
    /// step).
    pub fn fit_fill_to_seed(&mut self, id: FillId) -> Result<(), String> {
        let fill = self
            .doc
            .fill(id)
            .cloned()
            .ok_or("That fill no longer exists")?;
        let regions = self.regions();
        let ids = self.region_ids();
        let r = region_at(&regions, fill.seed).ok_or("There is no closed area at its seed")?;
        let mut fitted = fill;
        fitted.boundary = outline_of(r, &ids);
        fitted.seed = crate::editor::interior_point(r, fitted.seed);
        fitted.divided = false;
        self.doc.begin();
        self.doc.set_fill(id, fitted);
        self.commit("Fit fill to its area");
        Ok(())
    }

    /// Split a fill covering several areas into one fill per area, in its style (one undo
    /// step). The area around its seed keeps the fill; the new fills and it are selected.
    pub fn split_fill(&mut self, id: FillId) -> Result<Vec<FillId>, String> {
        let fill = self
            .doc
            .fill(id)
            .cloned()
            .ok_or("That fill no longer exists")?;
        let shapes = self.fill_shapes();
        let whole = &shapes.get(&id).ok_or("That fill has no area")?.region;
        let regions = self.regions();
        let ids = self.region_ids();
        let parts = parts_of(whole, &regions);
        if parts.len() < 2 {
            return Err("It covers only one area".into());
        }
        let home = parts
            .iter()
            .position(|r| r.contains(fill.seed))
            .unwrap_or(0);
        self.doc.begin();
        let mut out = vec![id];
        for (k, part) in parts.iter().enumerate() {
            let mut piece = fill.clone();
            piece.boundary = outline_of(part, &ids);
            piece.seed = crate::editor::interior_point(part, part.centroid);
            piece.divided = false;
            if k == home {
                piece.seed = crate::editor::interior_point(part, fill.seed);
                self.doc.set_fill(id, piece);
            } else {
                let new = self.doc.add_fill(piece.clone());
                // (Its layer and look as the original's; `add_fill` put it on top.)
                piece.order = self.doc.fill(new).map_or(piece.order, |f| f.order);
                self.doc.set_fill(new, piece);
                out.push(new);
            }
        }
        self.commit("Split fill");
        self.selection.clear();
        self.selected_region = None;
        self.selected_fills = out.iter().copied().collect();
        Ok(out)
    }

    /// Keep a fill flagged as covering several areas whole, and clear the flag (one undo
    /// step).
    pub fn keep_fill_whole(&mut self, id: FillId) {
        if let Some(mut fill) = self.doc.fill(id).cloned() {
            fill.divided = false;
            self.update_fill(id, fill);
        }
    }

    /// Before curve `id` is deleted while the area it bounds stays closed (a fillet or a
    /// chamfer taken off a corner, a curve of no length): take it out of fills' outlines so
    /// they stay, inside the open transaction.
    pub(crate) fn release_from_fills(&mut self, id: EntityId) {
        let holding: Vec<(FillId, Fill)> = self
            .doc
            .fills()
            .filter(|(_, f)| f.boundary.contains(&id))
            .map(|(fid, f)| (fid, f.clone()))
            .collect();
        for (fid, mut fill) in holding {
            fill.boundary.retain(|e| *e != id);
            if !fill.boundary.is_empty() {
                self.doc.set_fill(fid, fill);
            }
        }
    }

    /// Hatches whose whole outline was moved or turned as one in the open transaction move
    /// and turn with it, so the pattern stays where it was on the shape (a stretched or
    /// mirrored outline leaves its hatch alone). Worked out from the fills and outlines as
    /// the transaction found them, so it can run after every step of a drag.
    pub(crate) fn carry_hatches(&mut self) {
        if !self.doc.in_transaction() {
            return;
        }
        let hatches: Vec<(FillId, Fill)> = self
            .doc
            .fills()
            .filter(|(_, f)| matches!(f.kind, FillKind::Hatch(_)) && !f.boundary.is_empty())
            .map(|(id, f)| (id, f.clone()))
            .collect();
        for (id, mut fill) in hatches {
            let Some(FillKind::Hatch(was)) = self.doc.fill_before_transaction(id).map(|f| f.kind)
            else {
                continue;
            };
            let Some((pivot, turn, shift)) = self.outline_motion(&fill.boundary) else {
                continue;
            };
            let rotor = DVec2::from_angle(turn);
            let FillKind::Hatch(h) = &mut fill.kind else {
                continue;
            };
            let origin = pivot + rotor.rotate(was.origin - pivot) + shift;
            let mut angle = was.angle + turn.to_degrees();
            if turn != 0.0 {
                // Kept in (-180, 180], rounded off the last bit a turn leaves.
                angle = (angle + 180.0).rem_euclid(360.0) - 180.0;
                if angle <= -180.0 {
                    angle += 360.0;
                }
                angle = (angle * 1e9).round() / 1e9;
            }
            if h.origin != origin || h.angle != angle {
                h.origin = origin;
                h.angle = angle;
                self.doc.set_fill(id, fill);
            }
        }
    }

    /// Set a hatch's pattern origin and angle (those given), as one undo step. False if
    /// `fill` isn't a hatch or nothing changed.
    pub fn align_hatch(&mut self, fill: FillId, origin: Option<DVec2>, angle: Option<f64>) -> bool {
        let Some(mut f) = self.doc.fill(fill).cloned() else {
            return false;
        };
        let FillKind::Hatch(h) = &mut f.kind else {
            return false;
        };
        let was = (h.origin, h.angle);
        if let Some(o) = origin {
            h.origin = o;
        }
        if let Some(a) = angle {
            let a = (a + 180.0).rem_euclid(360.0) - 180.0;
            h.angle = (if a <= -180.0 { a + 360.0 } else { a } * 1e9).round() / 1e9;
        }
        if (h.origin, h.angle) == was {
            return false;
        }
        self.doc.begin();
        self.doc.set_fill(fill, f);
        self.commit("Align hatch pattern")
    }

    /// Align a hatch to one of its outline's curves: the pattern runs along it, its origin
    /// on the curve's start (as a brick or tile course is set out along a wall). False if
    /// `curve` isn't one of the hatch's outline curves.
    pub fn align_hatch_to_edge(&mut self, fill: FillId, curve: EntityId) -> bool {
        let on_outline = self
            .doc
            .fill(fill)
            .is_some_and(|f| f.boundary.contains(&curve));
        let Some(shape) = self.doc.shape(curve).filter(|_| on_outline) else {
            return false;
        };
        let (start, along) = match shape {
            Shape::Line { a, b } => (a, b - a),
            other => match other.cubics(std::f64::consts::FRAC_PI_2).first() {
                Some(c) => (c[0], drift_core::shape::bezier_tangent(c, 0.0)),
                None => return false,
            },
        };
        if along.length_squared() <= 0.0 {
            return false;
        }
        let angle = along.y.atan2(along.x).to_degrees();
        self.align_hatch(fill, Some(start), Some(angle))
    }

    /// How `curves` were moved as one in the open transaction: turned by an angle (radians)
    /// about a pivot, then shifted. None if they didn't move, or didn't move rigidly (or
    /// were reshaped or made in it).
    fn outline_motion(&self, curves: &[EntityId]) -> Option<(DVec2, f64, DVec2)> {
        let position = |e: Option<drift_core::Entity>| match e?.kind {
            drift_core::EntityKind::Point(p) => Some(p),
            _ => None,
        };
        let mut pairs: Vec<(DVec2, DVec2)> = Vec::new();
        for c in curves {
            let before = self.doc.entity_before_transaction(*c)?;
            let now = self.doc.get(*c)?;
            if std::mem::discriminant(&before.kind) != std::mem::discriminant(&now.kind) {
                return None;
            }
            for (p, q) in before.kind.points().zip(now.kind.points()) {
                let from = position(self.doc.entity_before_transaction(p))?;
                pairs.push((from, self.doc.point(q).ok()?));
            }
        }
        let n = pairs.len() as f64;
        if n == 0.0 {
            return None;
        }
        let extent = pairs
            .iter()
            .map(|(a, _)| a.abs().max_element())
            .fold(1.0, f64::max);
        let tol = 1e-7 * extent;
        if pairs.iter().all(|(a, b)| a.distance(*b) <= tol * 1e-3) {
            return None;
        }
        let ca = pairs.iter().map(|(a, _)| *a).sum::<DVec2>() / n;
        let cb = pairs.iter().map(|(_, b)| *b).sum::<DVec2>() / n;
        let (mut dot, mut cross) = (0.0, 0.0);
        for (a, b) in &pairs {
            let (u, v) = (*a - ca, *b - cb);
            dot += u.dot(v);
            cross += u.perp_dot(v);
        }
        let turn = if dot.abs() + cross.abs() > tol * tol {
            cross.atan2(dot)
        } else {
            0.0
        };
        // A turn too small to tell from rounding is a plain move.
        let turn = if turn.abs() < 1e-12 { 0.0 } else { turn };
        let rotor = DVec2::from_angle(turn);
        let rigid = pairs
            .iter()
            .all(|(a, b)| (ca + rotor.rotate(*a - ca) + (cb - ca)).distance(*b) <= tol);
        rigid.then_some((ca, turn, cb - ca))
    }

    /// The fills a selection box takes: a window those wholly inside it, a crossing box those
    /// too and the ones whose outline it touches (not one it merely lies inside). Only fills
    /// on shown, unlocked layers.
    pub fn fills_in(&mut self, rect: &Bounds, crossing: bool) -> Vec<FillId> {
        let shapes = self.fill_shapes();
        // (A wall's poché is picked with its walls.)
        let walls = self.wall_owners();
        let touched: HashSet<EntityId> = if crossing {
            self.index.select_in_rect(rect, true).into_iter().collect()
        } else {
            HashSet::new()
        };
        self.doc
            .fills()
            .filter(|(id, f)| {
                !walls.fills.contains_key(id)
                    && self
                        .doc
                        .layer(f.layer)
                        .is_some_and(|l| l.visible && !l.locked)
            })
            .filter(|(id, f)| {
                let inside = shapes.get(id).is_some_and(|s| {
                    let b = &s.region.bounds;
                    b.min.x >= rect.min.x
                        && b.min.y >= rect.min.y
                        && b.max.x <= rect.max.x
                        && b.max.y <= rect.max.y
                });
                inside || f.boundary.iter().any(|c| touched.contains(c))
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// The curves of a shape's outline, if `fill` is a shape's own (a closed path's): a
    /// shape is picked, moved and copied as one with its fill.
    pub fn shape_outline(&self, fill: FillId) -> Option<Vec<EntityId>> {
        let path = self.doc.fill(fill)?.path?;
        // (A wall's poché spans its cluster: picking, moving and copying act on the walls.)
        if self.doc.has_walls() && self.wall_owners().fills.contains_key(&fill) {
            return None;
        }
        let path = self.doc.path(path).filter(|p| p.closed)?;
        Some(path.curves.clone())
    }

    /// Copy the fills whose whole outline was copied onto the copies, inside the open
    /// transaction: `curves` gives each copied curve's copy, `place` moves a point as the
    /// copy was moved (turned or mirrored), and hatches turn with it.
    pub(crate) fn copy_fills_onto(
        &mut self,
        curves: impl Fn(EntityId) -> Option<EntityId>,
        place: impl Fn(DVec2) -> DVec2,
    ) {
        // Paths whose curves were all copied are paths of the copies too.
        let paths: Vec<(PathId, drift_core::Path)> = self
            .doc
            .paths()
            .filter(|(_, p)| p.curves.iter().all(|c| curves(*c).is_some()))
            .map(|(id, p)| (id, p.clone()))
            .collect();
        let mut copied_paths: HashMap<PathId, PathId> = HashMap::new();
        for (id, p) in paths {
            let copy = drift_core::Path {
                curves: p.curves.iter().filter_map(|c| curves(*c)).collect(),
                closed: p.closed,
            };
            if let Ok(new) = self.doc.add_path(copy) {
                copied_paths.insert(id, new);
            }
        }
        let fills: Vec<Fill> = self
            .doc
            .fills()
            .filter(|(_, f)| {
                !f.boundary.is_empty() && f.boundary.iter().all(|e| curves(*e).is_some())
            })
            .map(|(_, f)| f.clone())
            .collect();
        for mut fill in fills {
            fill.boundary = fill.boundary.iter().filter_map(|e| curves(*e)).collect();
            fill.boundary.sort_unstable();
            fill.boundary.dedup();
            fill.path = fill.path.and_then(|p| copied_paths.get(&p).copied());
            fill.seed = place(fill.seed);
            if let FillKind::Hatch(h) = &mut fill.kind {
                let along = DVec2::from_angle(h.angle.to_radians());
                let o = place(h.origin);
                let turned = place(h.origin + along) - o;
                h.origin = o;
                h.angle = turned.to_angle().to_degrees().rem_euclid(360.0);
            }
            // On the original's layer, just above it.
            let copy = self.doc.add_fill(fill.clone());
            self.doc.set_fill(copy, fill);
        }
    }

    /// The fills of every copy of a block (but the one being edited), in the world, as of
    /// the current document revision. A fill whose area the block's curves no longer
    /// enclose is left out.
    pub fn copy_fills(&mut self) -> Rc<Vec<CopyFill>> {
        let revision = self.doc.revision();
        if let Some((r, fills)) = &self.copy_fills
            && *r == revision
        {
            return fills.clone();
        }
        let editing = self.block_edit.map(|e| e.instance);
        let mut out = Vec::new();
        // Each block's areas are found among its own curves, once until they change, and
        // placed for each copy (a drawing from CAD can have hundreds of copies of blocks with
        // thousands of curves and hatches in them).
        let areas = &mut self.block_areas;
        areas.retain(|id, _| self.doc.block(*id).is_some());
        for (id, copy) in self.doc.instances() {
            let Some(block) = self.doc.block(copy.block) else {
                continue;
            };
            if Some(id) == editing || block.fills.is_empty() {
                continue;
            }
            let known = areas
                .get(&copy.block)
                .filter(|(items, _)| *items == block.items)
                .map(|(_, regions)| regions.clone());
            let regions = match known {
                Some(regions) => regions,
                None => {
                    let shapes: Vec<Shape> = block.items.iter().map(|item| item.shape).collect();
                    let regions = Rc::new(drift_geom::regions::regions(&shapes));
                    areas.insert(copy.block, (block.items.clone(), regions.clone()));
                    regions
                }
            };
            for (index, f) in block.fills.iter().enumerate() {
                if let Some(region) = region_at(&regions, f.seed) {
                    out.push(CopyFill {
                        instance: id,
                        index,
                        region: region.placed(copy),
                        fill: copy.apply_fill(f),
                    });
                }
            }
        }
        let out = Rc::new(out);
        self.copy_fills = Some((revision, out.clone()));
        out
    }

    /// The style of the topmost visible fill of a copy of a block under `p`.
    pub fn instance_fill_style_at(&mut self, p: DVec2) -> Option<FillStyle> {
        let fills = self.copy_fills();
        fills
            .iter()
            .rev()
            .find(|c| {
                self.doc.layer(c.fill.layer).is_none_or(|l| l.visible) && c.region.contains(p)
            })
            .map(|c| FillStyle::of(&c.fill))
    }

    /// The sketch's solid curves with their bounds, in document order.
    fn solid_curves(&self) -> SolidCurves {
        self.doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point() && e.style == drift_core::LineKind::Solid)
            .filter_map(|(id, _)| {
                let shape = self.doc.shape(id)?;
                Some((id, shape, shape.bounds()))
            })
            .collect()
    }

    /// A fill's area among its own outline curves and the curves that don't cross them (so
    /// holes inside it stay), when curves crossing it have cut its outline.
    /// `solid` is [`solid_curves`](Self::solid_curves).
    fn uncrossed_region(&self, fill: &Fill, solid: &SolidCurves) -> Option<FillShape> {
        if fill.boundary.iter().any(|e| !self.doc.contains(*e)) {
            return None;
        }
        let (regions, ids) = self.regions_among(&fill.boundary, solid)?;
        // (Its curve indices are into these curves, not the sketch's regions'.)
        Areas::new(&regions, &ids).exact(fill).map(|r| FillShape {
            curves: curves_of(fill, r, &ids),
            region: r.clone(),
            lost: false,
            crossed: true,
            parts: 0,
        })
    }

    /// A fill's whole area after an edit reshaped its outline (split a side where a curve
    /// crosses it, cut a fillet into it): the area around its seed among what is left of its
    /// outline, the curves the edit made (`created`) and the curves that don't cross them,
    /// if that area's outline is all of the fill's curves left plus new ones. Returns the
    /// area and its outline.
    fn reshaped_whole(
        &self,
        fill: &Fill,
        solid: &SolidCurves,
        created: &[EntityId],
    ) -> Option<(Region, Vec<EntityId>)> {
        let kept: Vec<EntityId> = fill
            .boundary
            .iter()
            .copied()
            .filter(|e| self.doc.contains(*e))
            .collect();
        let near: Bounds = kept
            .iter()
            .filter_map(|e| self.doc.shape(*e))
            .map(|s| s.bounds())
            .collect();
        let mut outline: Vec<EntityId> = kept.clone();
        outline.extend(created.iter().copied().filter(|e| {
            self.doc
                .shape(*e)
                .is_some_and(|s| s.bounds().intersects(&near))
        }));
        outline.sort_unstable();
        outline.dedup();
        let (regions, ids) = self.regions_among(&outline, solid)?;
        let r = region_at(&regions, fill.seed)?;
        let found = outline_of(r, &ids);
        (kept.iter().all(|e| found.binary_search(e).is_ok())
            && found.iter().all(|e| outline.binary_search(e).is_ok()))
        .then(|| (r.clone(), found))
    }

    /// The areas among `outline_ids` (sorted curves) and the solid curves near them that
    /// don't cross them, with the entity of each of their curve indices.
    fn regions_among(
        &self,
        outline_ids: &[EntityId],
        solid: &SolidCurves,
    ) -> Option<(Vec<Region>, Vec<EntityId>)> {
        let outline: Vec<Shape> = outline_ids
            .iter()
            .filter_map(|id| self.doc.shape(*id))
            .collect();
        if outline.is_empty() {
            return None;
        }
        // Curves clear of the outline's box can't be inside its area or change it: only the
        // ones near it are looked at.
        let mut near: Bounds = outline.iter().map(Shape::bounds).collect();
        let pad = 1e-6 * near.min.abs().max(near.max.abs()).max_element().max(1.0);
        near.min -= DVec2::splat(pad);
        near.max += DVec2::splat(pad);
        let (ids, curves): (Vec<EntityId>, Vec<Shape>) = solid
            .iter()
            .filter(|(_, _, bounds)| bounds.intersects(&near))
            .filter(|(id, shape, _)| {
                outline_ids.binary_search(id).is_ok()
                    || outline
                        .iter()
                        .all(|o| drift_geom::intersect::intersect(o, shape).is_empty())
            })
            .map(|(id, shape, _)| (*id, *shape))
            .unzip();
        Some((drift_geom::regions::regions(&curves), ids))
    }

    /// Fills in draw order (bottom first): by layer, then each fill's order.
    pub fn fills_in_order(&self) -> Vec<FillId> {
        let layer_pos: HashMap<_, usize> = self
            .doc
            .layers()
            .enumerate()
            .map(|(i, (id, _))| (id, i))
            .collect();
        let mut fills: Vec<(FillId, &Fill)> = self.doc.fills().collect();
        fills.sort_by_key(|(id, f)| (layer_pos.get(&f.layer).copied().unwrap_or(0), f.order, *id));
        fills.into_iter().map(|(id, _)| id).collect()
    }

    /// The curves that make up the picked fills' areas (outlines and holes), and the centre
    /// of the first one: what moves or rotates when a fill is moved. Only the curves the
    /// areas were found from: another shape's edge lying along an outline stays put.
    pub fn selected_fill_curves(&mut self) -> Option<(Vec<EntityId>, DVec2)> {
        let shapes = self.fill_shapes();
        let mut ids = std::collections::BTreeSet::new();
        let mut centre = None;
        for f in self.selected_fills.clone() {
            if let Some(shape) = shapes.get(&f) {
                // (A lost fill's last curves may have been deleted since.)
                ids.extend(
                    shape
                        .curves
                        .iter()
                        .filter(|id| self.doc.get(**id).is_some()),
                );
                centre.get_or_insert(shape.region.centroid);
            }
        }
        (!ids.is_empty()).then(|| (ids.into_iter().collect(), centre.unwrap_or_default()))
    }

    /// The topmost visible fill under `p`.
    pub fn fill_at(&mut self, p: DVec2) -> Option<FillId> {
        let shapes = self.fill_shapes();
        self.fills_in_order().into_iter().rev().find(|id| {
            let visible = self
                .doc
                .fill(*id)
                .and_then(|f| self.doc.layer(f.layer))
                .is_none_or(|l| l.visible);
            visible && shapes.get(id).is_some_and(|s| s.region.contains(p))
        })
    }

    /// The eyedropper: make the style of the fill under `p` the Fill tool's style.
    pub fn pick_up_fill_style(&mut self, p: DVec2) -> Result<(), String> {
        let style = self
            .fill_at(p)
            .and_then(|id| self.doc.fill(id))
            .map(FillStyle::of)
            .or_else(|| self.instance_fill_style_at(p))
            .ok_or("No fill or hatch here to pick up")?;
        self.fill_style = style;
        self.fill_eyedropper = false;
        Ok(())
    }

    /// The outer boundary curves of the region at `p`.
    fn boundary_at(&mut self, p: DVec2) -> Option<Vec<EntityId>> {
        let regions = self.regions();
        let ids = self.region_ids();
        let r = region_at(&regions, p)?;
        Some(r.outer.curves().into_iter().map(|i| ids[i]).collect())
    }

    /// Fill the closed area around `p` with the current fill style, and select the fill.
    pub fn add_fill_at(&mut self, p: DVec2) -> Result<FillId, String> {
        let boundary = self
            .boundary_at(p)
            .ok_or("Click inside a closed area to fill it")?;
        let mut fill = Fill::new(boundary, p, FillKind::Solid);
        self.fill_style.apply(&mut fill);
        // Inside a shape drawn as one (the area is its inside): the fill is the shape's.
        let walls = self.wall_owners();
        fill.path = self
            .doc
            .paths()
            .filter(|(id, _)| !walls.paths.contains_key(id) && !walls.centres.contains_key(id))
            .find(|(_, path)| path.closed && sorted(&path.curves) == fill.boundary)
            .map(|(id, _)| id);
        // An area that is already filled isn't filled twice: its fill takes the current style
        // (so clicking again with another pattern swaps it) and is selected.
        let shapes = self.fill_shapes();
        let existing = self.fills_in_order().into_iter().rev().find(|id| {
            shapes
                .get(id)
                .is_some_and(|s| !s.lost && s.region.contains(p))
                && self
                    .doc
                    .fill(*id)
                    .is_some_and(|f| f.boundary == fill.boundary)
        });
        let id = if let Some(id) = existing {
            if let Some(mut current) = self.doc.fill(id).cloned()
                && FillStyle::of(&current) != self.fill_style
            {
                self.fill_style.apply(&mut current);
                self.update_fill(id, current);
            }
            id
        } else {
            self.doc.begin();
            let id = self.doc.add_fill(fill);
            let label = match self.fill_style.kind {
                FillKind::Solid => "Fill",
                FillKind::Hatch(_) => "Hatch",
            };
            self.commit(label);
            id
        };
        self.selection.clear();
        self.selected_region = None;
        self.selected_fills = [id].into();
        Ok(id)
    }

    /// Record a closed shape a drawing tool has just made as a path (its `curves` in order
    /// round it), inside the open transaction, and fill it when new shapes are to be filled:
    /// one undo step with the shape. Returns the path.
    pub(crate) fn add_shape(&mut self, curves: &[EntityId]) -> Option<PathId> {
        let path = self
            .doc
            .add_path(drift_core::Path {
                curves: curves.to_vec(),
                closed: true,
            })
            .ok()?;
        if self.fill_new_shapes
            && let Some(region) = self.path_region(path)
        {
            let seed = crate::editor::interior_point(&region, region.centroid);
            let mut fill = Fill::new(curves.to_vec(), seed, FillKind::Solid);
            fill.path = Some(path);
            self.fill_style.apply(&mut fill);
            self.doc.add_fill(fill);
        }
        Some(path)
    }

    /// [`add_shape`](Self::add_shape), returning the fill it made.
    #[cfg(test)]
    pub(crate) fn fill_new_shape(&mut self, curves: &[EntityId]) -> Option<FillId> {
        let path = self.add_shape(curves)?;
        self.doc
            .fills()
            .find(|(_, f)| f.path == Some(path))
            .map(|(id, _)| id)
    }

    /// The area inside a closed path: bounded by its curves alone (whatever crosses them
    /// is ignored), with the shapes lying inside it left out as holes, as an area's are.
    /// A path that crosses itself bounds several areas: the biggest. None for an open path.
    pub(crate) fn path_region(&self, id: PathId) -> Option<Region> {
        self.path_region_and_holes(id, &self.solid_curves())
            .map(|(r, _)| r)
    }

    /// Where the open edit changed the drawing: the boxes of the points it moved (where
    /// they were and are), and of the curves it touched or drew. A shape whose area is clear
    /// of them, and whose own curves and points it didn't touch, is as it was.
    fn touched_boxes(&self) -> Vec<Bounds> {
        let mut boxes = Vec::new();
        for e in self.doc.entities_touched_in_transaction() {
            if let Ok(p) = self.doc.point(e) {
                boxes.push(Bounds { min: p, max: p });
            } else if let Some(shape) = self.doc.shape(e) {
                boxes.push(shape.bounds());
            }
            if let Some(drift_core::Entity {
                kind: drift_core::EntityKind::Point(p),
                ..
            }) = self.doc.entity_before_transaction(e)
            {
                boxes.push(Bounds { min: p, max: p });
            }
        }
        boxes
    }

    /// Whether the open edit left shape `curves` (sorted) and the area `bounds` round them as
    /// they were (see [`touched_boxes`](Self::touched_boxes)).
    fn shape_untouched(&self, curves: &[EntityId], bounds: &Bounds, boxes: &[Bounds]) -> bool {
        let pad = 1e-6
            * bounds
                .min
                .abs()
                .max(bounds.max.abs())
                .max_element()
                .max(1.0);
        let near = Bounds {
            min: bounds.min - DVec2::splat(pad),
            max: bounds.max + DVec2::splat(pad),
        };
        !curves.iter().any(|c| self.doc.touched_in_transaction(*c))
            && !self
                .doc
                .defining_points(curves.iter().copied())
                .iter()
                .any(|p| self.doc.touched_in_transaction(*p))
            && !boxes.iter().any(|b| b.intersects(&near))
    }

    /// The area an open shape's outline bounds closed straight across its two open ends:
    /// what its fill shows while nothing closes round it. None if the path is closed, or
    /// open in more than one place.
    pub(crate) fn open_path_region(&self, id: PathId) -> Option<Region> {
        let path = self.doc.path(id).filter(|p| !p.closed)?;
        let mut shapes: Vec<Shape> = Vec::new();
        let mut ends: HashMap<EntityId, usize> = HashMap::new();
        for c in &path.curves {
            shapes.push(self.doc.shape(*c)?);
            for p in curve_ends(&self.doc, *c).into_iter().flatten() {
                *ends.entry(p).or_default() += 1;
            }
        }
        let open: Vec<DVec2> = ends
            .into_iter()
            .filter(|(_, n)| *n == 1)
            .filter_map(|(p, _)| self.doc.point(p).ok())
            .collect();
        let [a, b] = open[..] else {
            return None;
        };
        shapes.push(Shape::Line { a, b });
        drift_geom::regions::regions(&shapes)
            .into_iter()
            .max_by(|x, y| x.area.total_cmp(&y.area))
    }

    /// [`path_region`](Self::path_region), with the curves round its holes (sorted).
    /// `solid` is [`solid_curves`](Self::solid_curves), found once for many paths.
    fn path_region_and_holes(
        &self,
        id: PathId,
        solid: &SolidCurves,
    ) -> Option<(Region, Vec<EntityId>)> {
        let path = self.doc.path(id)?;
        if !path.closed {
            return None;
        }
        let outline = sorted(&path.curves);
        // Its curves with the ones inside that don't cross them: the islands are holes.
        if let Some((regions, ids)) = self.regions_among(&outline, solid)
            && let Some(candidates) = Areas::new(&regions, &ids).by_outline.get(&outline)
            && let Some(r) = candidates
                .iter()
                .map(|&i| &regions[i])
                .max_by(|a, b| a.area.total_cmp(&b.area))
        {
            let holes = sorted(
                &r.holes
                    .iter()
                    .flat_map(|lp| lp.sources.iter().filter_map(|&i| ids.get(i).copied()))
                    .collect::<Vec<_>>(),
            );
            return Some((r.clone(), holes));
        }
        // Crossing itself (no area has all its curves as outline): the biggest it bounds.
        let shapes: Vec<Shape> = path
            .curves
            .iter()
            .map(|c| self.doc.shape(*c))
            .collect::<Option<_>>()?;
        drift_geom::regions::regions(&shapes)
            .into_iter()
            .max_by(|a, b| a.area.total_cmp(&b.area))
            .map(|r| (r, Vec::new()))
    }

    /// Follow curves end to end from `first` (through the end points they share): the
    /// chain through the curves `rank` allows, the lowest rank first at a junction, and
    /// whether it closes. A lone circle or whole ellipse is a closed chain of one.
    pub(crate) fn chain_from(
        &self,
        first: EntityId,
        rank: &dyn Fn(EntityId) -> Option<u8>,
    ) -> (Vec<EntityId>, bool) {
        let ends = |c: EntityId| self.doc.get(c).and_then(|e| e.kind.ends());
        let Some([start, mut at]) = ends(first) else {
            return (vec![first], true);
        };
        let mut chain = vec![first];
        let mut used: HashSet<EntityId> = HashSet::from([first]);
        // Forward from the first curve's end; if that doesn't close, back from its start.
        for forward in [true, false] {
            if !forward {
                at = start;
            }
            loop {
                if forward && at == start {
                    return (chain, true);
                }
                let next = self
                    .doc
                    .users_of(at)
                    .filter(|c| !used.contains(c))
                    .filter(|c| ends(*c).is_some_and(|e| e.contains(&at)))
                    .filter_map(|c| rank(c).map(|r| (r, c)))
                    .min();
                let Some((_, c)) = next else {
                    break;
                };
                let Some([s, e]) = ends(c) else {
                    break;
                };
                used.insert(c);
                if forward {
                    chain.push(c);
                } else {
                    chain.insert(0, c);
                }
                at = if s == at { e } else { s };
            }
        }
        (chain, false)
    }

    /// Keep each path a shape as an edit changes its curves, inside the edit's open
    /// transaction: a side split, filleted or chamfered takes the new curves into the path
    /// in order; a path cut open (a side deleted or trimmed) stays open until its ends meet
    /// again; and open paths whose ends meet (two shapes trimmed into one outline) join
    /// into one closed path, whose fill is the topmost of theirs (the others go, as
    /// [`FillChange::Merged`]).
    pub(crate) fn follow_paths(&mut self) {
        if self.doc.paths().next().is_none() {
            return;
        }
        let created: HashSet<EntityId> = self
            .doc
            .created_in_transaction()
            .into_iter()
            .filter(|e| self.doc.get(*e).is_some_and(|en| !en.kind.is_point()))
            .collect();
        // (Walls keep their centrelines and outlines themselves.)
        let walls = self.wall_owners();
        let paths: Vec<(PathId, drift_core::Path)> = self
            .doc
            .paths()
            .filter(|(id, _)| !walls.paths.contains_key(id) && !walls.centres.contains_key(id))
            .map(|(id, p)| (id, p.clone()))
            .collect();
        let mut joined: HashSet<PathId> = HashSet::new();
        let mut changes = Vec::new();
        for (id, path) in &paths {
            if joined.contains(id) || self.doc.path_created_in_transaction(*id) {
                continue;
            }
            let own: Vec<EntityId> = path
                .curves
                .iter()
                .copied()
                .filter(|c| self.doc.contains(*c))
                .collect();
            let Some(&first) = own.first() else {
                continue;
            };
            // Only a path the edit touched: cut open, a curve reshaped, or a new curve
            // meeting one of its ends.
            let own_ends: HashSet<EntityId> = own
                .iter()
                .filter_map(|c| curve_ends(&self.doc, *c))
                .flat_map(|e| e.into_iter())
                .collect();
            let touched = !path.closed
                || own.iter().any(|c| self.doc.reshaped_in_transaction(*c))
                || created.iter().any(|c| {
                    curve_ends(&self.doc, *c)
                        .is_some_and(|e| e.iter().any(|p| own_ends.contains(p)))
                });
            if !touched {
                continue;
            }
            let others: HashMap<EntityId, PathId> = paths
                .iter()
                .filter(|(o, p)| o != id && !p.closed && !joined.contains(o))
                .flat_map(|(o, p)| p.curves.iter().map(move |c| (*c, *o)))
                .collect();
            let rank = |c: EntityId| {
                if own.contains(&c) {
                    Some(0)
                } else if created.contains(&c) {
                    Some(1)
                } else if others.contains_key(&c) {
                    Some(2)
                } else {
                    None
                }
            };
            let (chain, closed) = self.chain_from(first, &rank);
            let followed = if closed {
                // Open paths it closed through join it: their fills become one (the
                // topmost's), as fills brought onto one area do.
                let absorbed: BTreeSet<PathId> = chain
                    .iter()
                    .filter_map(|c| others.get(c).copied())
                    .collect();
                if !absorbed.is_empty() {
                    let mut fills: Vec<FillId> = self
                        .fills_in_order()
                        .into_iter()
                        .filter(|f| {
                            self.doc.fill(*f).is_some_and(|fill| {
                                fill.path == Some(*id)
                                    || fill.path.is_some_and(|p| absorbed.contains(&p))
                            })
                        })
                        .collect();
                    if let Some(keep) = fills.pop() {
                        if let Some(mut fill) = self.doc.fill(keep).cloned() {
                            fill.path = Some(*id);
                            self.doc.set_fill(keep, fill);
                        }
                        for f in fills {
                            self.doc.remove_fill(f);
                            changes.push(FillChange::Merged { id: f, into: keep });
                        }
                    }
                    for a in absorbed {
                        joined.insert(a);
                        self.doc.remove_path(a);
                    }
                }
                drift_core::Path {
                    curves: chain,
                    closed: true,
                }
            } else {
                // Open: its chain through its own curves and new ones only. Another open
                // path's curves join it only when they close it, so a shape trimmed open
                // never takes a neighbour's outline (and its fill) before the two meet.
                let (chain, _) = self.chain_from(first, &|c| rank(c).filter(|r| *r < 2));
                // Then any of its curves the chain doesn't reach.
                let mut curves = chain;
                let rest: Vec<EntityId> = own
                    .iter()
                    .copied()
                    .filter(|c| !curves.contains(c))
                    .collect();
                curves.extend(rest);
                drift_core::Path {
                    curves,
                    closed: false,
                }
            };
            let _ = self.doc.set_path(*id, followed);
        }
        if !changes.is_empty() {
            if self.notice.is_none() {
                self.notice = Some(self.describe_fill_changes(&changes));
            }
            self.fill_changes.extend(changes);
        }
    }

    /// Give a fill the boundary of the area around `p` (after its own boundary was lost).
    pub fn repick_fill_at(&mut self, id: FillId, p: DVec2) -> Result<(), String> {
        let boundary = self
            .boundary_at(p)
            .ok_or("Click inside a closed area for the fill's new boundary")?;
        let Some(mut fill) = self.doc.fill(id).cloned() else {
            return Err("That fill no longer exists".into());
        };
        let mut sorted = boundary;
        sorted.sort_unstable();
        sorted.dedup();
        fill.boundary = sorted;
        fill.seed = p;
        fill.path = None;
        self.doc.begin();
        self.doc.set_fill(id, fill);
        self.commit("Re-pick fill boundary");
        self.last_fill_shapes.remove(&id);
        self.selected_fills = [id].into();
        Ok(())
    }

    /// Replace a fill's settings (one undo step).
    pub fn update_fill(&mut self, id: FillId, fill: Fill) {
        if self.doc.fill(id) == Some(&fill) {
            return;
        }
        self.doc.begin();
        self.doc.set_fill(id, fill);
        self.commit("Edit fill");
    }
}

/// The curves of a wall poché's region: its outline and its holes' outlines.
fn region_curves_of(doc: &drift_core::Document, fill: FillId) -> Vec<EntityId> {
    let Some(f) = doc.fill(fill) else {
        return Vec::new();
    };
    let mut out = f.boundary.clone();
    for (_, w) in doc.walls() {
        let Some(key) = w
            .made
            .fills
            .iter()
            .find(|(_, x)| **x == fill)
            .map(|(k, _)| *k)
        else {
            continue;
        };
        for (k, p) in &w.made.outlines {
            if k.outer == key.outer
                && k.hole != 0
                && let Some(path) = doc.path(*p)
            {
                out.extend(path.curves.iter().copied());
            }
        }
    }
    out
}

pub use drift_draft::tessellate;

/// A tessellated fill: world vertices and triangle indices.
type Mesh = Rc<(Vec<DVec2>, Vec<u32>)>;

/// Which fill a tessellation is of: a fill, or a copy's fill.
use crate::stack::FillRef as MeshKey;

/// Tessellations by fill, kept while the document doesn't change.
#[derive(Default)]
pub struct MeshCache {
    revision: u64,
    meshes: HashMap<MeshKey, Mesh>,
}

impl MeshCache {
    fn get(&mut self, revision: u64, id: MeshKey, region: &Region) -> Mesh {
        if self.revision != revision {
            self.revision = revision;
            self.meshes.clear();
        }
        self.meshes
            .entry(id)
            .or_insert_with(|| Rc::new(tessellate(region, 0.04).unwrap_or_default()))
            .clone()
    }
}

/// What the canvas draws for one fill: its triangles (a solid fill, a hatch's background,
/// or a tint standing in for too dense a hatch) and its hatch lines.
#[derive(Clone, Debug)]
pub struct FillGroup {
    pub layer: LayerId,
    /// The fill this is: the sketch's, or a copy's.
    pub fill: crate::stack::FillRef,
    pub meshes: Vec<FillMesh>,
    pub lines: Vec<SketchItem>,
}

/// What the canvas draws for fills, in draw order (bottom first).
#[derive(Default)]
pub struct FillGraphics {
    pub groups: Vec<FillGroup>,
}

impl FillGraphics {
    /// Every fill's triangles, in draw order.
    pub fn meshes(&self) -> Vec<FillMesh> {
        self.groups.iter().flat_map(|g| g.meshes.clone()).collect()
    }

    /// Every hatch's lines, in draw order.
    pub fn lines(&self) -> Vec<SketchItem> {
        self.groups.iter().flat_map(|g| g.lines.clone()).collect()
    }
}

/// The default colour of solid fills without a colour of their own or their layer's.
fn default_fill_color(dark: bool) -> Rgb {
    if dark {
        Rgb([0x9a, 0xa6, 0xb8])
    } else {
        Rgb([0x5a, 0x66, 0x78])
    }
}

/// The default hatch line colour (the theme's line colour).
fn default_line_color(dark: bool) -> Rgb {
    if dark {
        Rgb([0xd8, 0xdc, 0xe2])
    } else {
        Rgb([0x30, 0x34, 0x3a])
    }
}

/// Build the canvas graphics for every visible fill. `view` is the visible world area and
/// `px_per_mm` the zoom, so hatches are only generated where they show, and too-dense ones
/// become tints.
pub fn fill_graphics(
    ed: &mut Editor,
    cache: &mut MeshCache,
    patterns: &PatternLibrary,
    view: Bounds,
    px_per_mm: f64,
    dark: bool,
) -> FillGraphics {
    let mut out = FillGraphics::default();
    let shapes = ed.fill_shapes();
    let copies = ed.copy_fills();
    let order = ed.fills_in_order();
    let ed = &*ed;
    let revision = ed.doc.revision();
    let draft_scale = ed.doc.draft_style().scale;
    // The sketch's fills, then copies' fills over them.
    let fills = order
        .iter()
        .filter_map(|id| {
            Some((
                MeshKey::Fill(*id),
                ed.doc.fill(*id)?,
                &shapes.get(id)?.region,
            ))
        })
        .chain(
            copies
                .iter()
                .map(|c| (MeshKey::Copy(c.instance, c.index), &c.fill, &c.region)),
        );
    for (id, fill, region) in fills {
        let layer = ed.doc.layer(fill.layer);
        if layer.is_some_and(|l| !l.visible) {
            continue;
        }
        let bounds = region_bounds(region);
        if !bounds.intersects(&view) {
            continue;
        }
        let mut group = FillGroup {
            layer: fill.layer,
            fill: id,
            meshes: Vec::new(),
            lines: Vec::new(),
        };
        let own = fill.color.or(layer.and_then(|l| l.fill_colour()));
        let mut tint = |color: Rgb, alpha: f64, group: &mut FillGroup| {
            let mesh = cache.get(revision, id, region);
            let [r, g, b] = color.0;
            group.meshes.push(FillMesh {
                vertices: mesh.0.clone(),
                indices: mesh.1.clone(),
                color: [r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8],
            });
        };
        match &fill.kind {
            FillKind::Solid => {
                tint(
                    own.unwrap_or(default_fill_color(dark)),
                    fill.opacity,
                    &mut group,
                );
            }
            FillKind::Hatch(h) => {
                if let Some(bg) = h.background {
                    tint(bg, 1.0, &mut group);
                }
                let line = own.unwrap_or(default_line_color(dark));
                let scale = h.scale * if h.model { 1.0 } else { draft_scale };
                let placement = Placement {
                    scale,
                    angle: h.angle,
                    origin: h.origin,
                };
                let hatching = patterns
                    .get(&h.pattern)
                    .filter(|p| p.min_spacing() * scale * px_per_mm >= MIN_HATCH_SPACING_PX)
                    .map(|pattern| {
                        drift_draft::hatch(
                            region,
                            pattern,
                            &placement,
                            Some(view),
                            HATCH_SEGMENT_LIMIT,
                        )
                    })
                    .filter(|hatching| !hatching.truncated);
                match hatching {
                    // No pattern, too dense to see, or too much of it: a tint instead.
                    None => tint(line, 0.25, &mut group),
                    Some(hatching) => {
                        let weight = fill.weight.unwrap_or_default().0;
                        group
                            .lines
                            .extend(hatching.segments.into_iter().map(|[a, b]| {
                                let mut item = SketchItem::new(Shape::Line { a, b });
                                item.color = own.map(|c| c.0);
                                item.weight = weight;
                                item
                            }));
                    }
                }
            }
        }
        out.groups.push(group);
    }
    out
}

pub fn region_bounds(region: &Region) -> Bounds {
    let mut b = Bounds::EMPTY;
    for p in region.outer.polygon(0.1) {
        b.include(p);
    }
    // Flattening can cut inside an arc: pad a little.
    let pad = b.size().max_element() * 0.02;
    b.min -= DVec2::splat(pad);
    b.max += DVec2::splat(pad);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::Document;

    fn plate() -> (Editor, Vec<EntityId>) {
        let mut doc = Document::new();
        let p = [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let lines = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        let c = doc.add_point(DVec2::new(20.0, 10.0));
        doc.add_circle(c, 5.0).unwrap();
        (Editor::new(doc), lines)
    }

    #[test]
    fn fills_find_and_follow_their_region() {
        let (mut ed, lines) = plate();
        let id = ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        let fill = ed.doc.fill(id).unwrap().clone();
        let mut expected = lines.clone();
        expected.sort();
        assert_eq!(fill.boundary, expected);
        let shape = ed.fill_shapes()[&id].clone();
        assert!(!shape.lost);
        assert_eq!(shape.region.holes.len(), 1, "the hole is excluded");
        assert_eq!(ed.fill_at(DVec2::new(5.0, 5.0)), Some(id));
        assert_eq!(ed.fill_at(DVec2::new(20.0, 10.0)), None, "inside the hole");

        // Move the whole plate: the fill follows.
        ed.doc.begin();
        let all: Vec<EntityId> = ed.doc.iter().map(|(e, _)| e).collect();
        ed.doc.translate(all, DVec2::new(100.0, 0.0));
        ed.commit("Move");
        let shape = ed.fill_shapes()[&id].clone();
        assert!(!shape.lost);
        assert!((shape.region.centroid.x - 120.0).abs() < 1e-6);

        // Delete an edge: a shape (#202) with nothing closed round it, the fill keeps its
        // last shape until undone.
        assert!(ed.shape_outline(id).is_some());
        ed.doc.begin();
        ed.doc.delete([lines[1]]);
        ed.commit("Delete");
        assert!(ed.fill_shapes()[&id].lost);
        ed.undo();
        assert!(!ed.fill_shapes()[&id].lost);
    }

    #[test]
    fn filling_a_filled_area_again_restyles_instead_of_stacking() {
        let (mut ed, _) = plate();
        let first = ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        // Same style again: nothing new, nothing to undo.
        let steps = ed.history.can_undo();
        assert_eq!(ed.add_fill_at(DVec2::new(30.0, 15.0)).unwrap(), first);
        assert_eq!(ed.doc.fills().count(), 1);
        assert_eq!(ed.history.can_undo(), steps);
        // Another pattern: the existing hatch changes to it.
        ed.fill_style.kind = FillKind::Hatch(drift_core::Hatch::new("ANSI37"));
        assert_eq!(ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap(), first);
        assert_eq!(ed.doc.fills().count(), 1);
        assert!(matches!(
            &ed.doc.fill(first).unwrap().kind,
            FillKind::Hatch(h) if h.pattern == "ANSI37"
        ));
        assert_eq!(ed.selected_fills, [first].into());
        // A different area (the disc inside the hole) still gets its own fill.
        ed.add_fill_at(DVec2::new(20.0, 11.0)).unwrap();
        assert_eq!(ed.doc.fills().count(), 2);
    }

    #[test]
    fn a_picked_fill_brings_its_boundary_and_holes() {
        let (mut ed, lines) = plate();
        let id = ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        // As an import brings it: an area fill (a shape's fill brings its outline alone).
        let mut fill = ed.doc.fill(id).unwrap().clone();
        let path = fill.path.take();
        ed.doc.begin();
        ed.doc.set_fill(id, fill);
        if let Some(p) = path {
            ed.doc.remove_path(p);
        }
        assert!(ed.commit_import("Area fill"));
        assert_eq!(ed.selected_fills, [id].into());
        let (curves, centre) = ed.selected_fill_curves().expect("curves");
        // The four edges and the hole's circle.
        assert_eq!(curves.len(), 5);
        assert!(lines.iter().all(|l| curves.contains(l)));
        assert!((centre - DVec2::new(20.0, 10.0)).length() < 1e-6);
    }

    #[test]
    fn nothing_to_fill_outside_closed_areas() {
        let (mut ed, _) = plate();
        assert!(ed.add_fill_at(DVec2::new(-10.0, -10.0)).is_err());
        assert_eq!(ed.doc.fills().count(), 0);
        // The disc inside the plate is its own area.
        let disc = ed.add_fill_at(DVec2::new(20.0, 11.0)).unwrap();
        assert_eq!(ed.doc.fill(disc).unwrap().boundary.len(), 1);
    }

    #[test]
    fn hatch_graphics_and_density_fallback() {
        let (mut ed, _) = plate();
        ed.add_fill_at(DVec2::new(5.0, 5.0)).unwrap();
        let mut cache = MeshCache::default();
        let mut patterns = PatternLibrary::default();
        patterns.update(ed.doc.custom_patterns());
        let view = Bounds {
            min: DVec2::splat(-10.0),
            max: DVec2::splat(60.0),
        };
        // Zoomed in: real hatch lines.
        let g = fill_graphics(&mut ed, &mut cache, &patterns, view, 10.0, true);
        assert_eq!(g.groups.len(), 1);
        assert!(g.meshes().is_empty());
        assert!(g.lines().len() > 10);
        // Zoomed far out: a tint instead.
        let g = fill_graphics(&mut ed, &mut cache, &patterns, view, 0.1, true);
        let meshes = g.meshes();
        assert_eq!(meshes.len(), 1);
        assert!(g.lines().is_empty());
        let (vertices, indices) = (&meshes[0].vertices, &meshes[0].indices);
        // The tessellation covers the plate minus the hole.
        let area: f64 = indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| vertices[t[k] as usize]);
                (b - a).perp_dot(c - a).abs() / 2.0
            })
            .sum();
        let expected = 800.0 - std::f64::consts::PI * 25.0;
        assert!((area - expected).abs() < 0.5, "{area}");
    }

    #[test]
    fn imported_patterns_override_and_list() {
        let mut lib = PatternLibrary::default();
        lib.update("*ANSI31, Mine\n0, 0,0, 0,5\n*ZIG, Zig\n45, 0,0, 0,1\n");
        assert_eq!(lib.get("ansi31").unwrap().description, "Mine");
        assert!(lib.get("ZIG").is_some());
        assert_eq!(lib.all().filter(|p| p.name == "ANSI31").count(), 1);
    }

    #[test]
    fn overlapping_and_coincident_outlines_find_tessellate_and_hatch() {
        // Two arrows of lines, the second shifted along the first: edges along each other,
        // corners on each other's edges, right on top, or a hair off. Finding the areas,
        // tessellating and hatching them must not panic.
        use drift_core::Shape;
        let mut seed: u64 = 99;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1_000_000) as f64 / 1_000_000.0
        };
        let patterns = PatternLibrary::default();
        let pattern = patterns.get("ANSI31").unwrap().clone();
        let tiny = [0.0, 1e-5, -1.6e-5, 1e-7, 1e-9, 1e-12, 3e-4, -2e-6];
        for _ in 0..3000 {
            let s = 0.5 + rnd() * 400.0;
            let o = DVec2::new(rnd() * 400.0 - 200.0, rnd() * 400.0 - 200.0);
            let a = [(0.0, 0.0), (0.6, 0.2), (1.0, 0.2), (1.0, -0.2), (0.6, -0.2)]
                .map(|(x, y)| o + DVec2::new(x, y) * s);
            let along = match (rnd() * 5.0) as u32 {
                0 => 0.15 * s,
                1 => rnd() * s,
                2 => 0.4 * s,
                3 => 0.0,
                _ => (rnd() * 20.0).round() * 0.05 * s,
            };
            let across = match (rnd() * 4.0) as u32 {
                0 => 0.4 * s,
                1 => 0.2 * s,
                _ => 0.0,
            };
            let d = DVec2::new(
                along + tiny[(rnd() * 8.0) as usize],
                across + tiny[(rnd() * 8.0) as usize],
            );
            let b = a.map(|p| p + d);
            let curves: Vec<Shape> = [a, b]
                .iter()
                .flat_map(|p| {
                    (0..5).map(|i| Shape::Line {
                        a: p[i],
                        b: p[(i + 1) % 5],
                    })
                })
                .collect();
            let placement = Placement {
                scale: 1.0,
                angle: 0.0,
                origin: DVec2::ZERO,
            };
            for region in &drift_geom::regions::regions(&curves) {
                let _ = tessellate(region, 0.04);
                let _ = drift_draft::hatch(region, &pattern, &placement, None, HATCH_SEGMENT_LIMIT);
            }
        }
    }
}
