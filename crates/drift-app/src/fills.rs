//! Fills and hatches: finding each fill's region as the geometry changes, creating and editing
//! fills, and turning them into GPU meshes and hatch lines.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use drift_core::{
    Bounds, DVec2, EntityId, Fill, FillId, FillKind, Hatch, InstanceId, LineWeight, Rgb, Shape,
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

/// How new fills look (the Fill tool's settings), and the editable part of a fill.
#[derive(Clone, Debug, PartialEq)]
pub struct FillStyle {
    pub kind: FillKind,
    pub color: Option<Rgb>,
    pub opacity: f64,
    pub weight: Option<LineWeight>,
}

impl Default for FillStyle {
    fn default() -> Self {
        Self {
            kind: FillKind::Hatch(Hatch::new("ANSI31")),
            color: None,
            opacity: 0.35,
            weight: None,
        }
    }
}

impl FillStyle {
    pub fn of(fill: &Fill) -> Self {
        Self {
            kind: fill.kind.clone(),
            color: fill.color,
            opacity: fill.opacity,
            weight: fill.weight,
        }
    }

    pub fn apply(&self, fill: &mut Fill) {
        fill.kind = self.kind.clone();
        fill.color = self.color;
        fill.opacity = self.opacity;
        fill.weight = self.weight;
    }
}

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

    /// The region bounded by exactly a fill's curves (nearest its seed if several).
    fn exact(&self, fill: &Fill) -> Option<&'a Region> {
        self.nearest(self.by_outline.get(&fill.boundary)?, fill.seed)
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
    /// seed if several), else the one its outline became, else the one around its seed if
    /// it still shares a boundary curve.
    fn find(&self, fill: &Fill) -> Option<&'a Region> {
        self.exact(fill)
            .or_else(|| self.successor(fill))
            .or_else(|| {
                region_at(self.regions, fill.seed).filter(|r| {
                    r.outer
                        .curves()
                        .into_iter()
                        .any(|i| fill.boundary.binary_search(&self.ids[i]).is_ok())
                })
            })
    }
}

/// The sorted, distinct entities of a region's outer boundary.
fn outline_of(region: &Region, ids: &[EntityId]) -> Vec<EntityId> {
    let mut c: Vec<EntityId> = region.outer.curves().into_iter().map(|i| ids[i]).collect();
    c.sort_unstable();
    c.dedup();
    c
}

/// The sketch's solid curves with their bounds, in document order: what
/// [`Editor::uncrossed_region`] finds a crossed fill's area among.
type SolidCurves = Vec<(EntityId, Shape, Bounds)>;

impl Editor {
    /// Every fill's region, as of the current document revision.
    pub fn fill_shapes(&mut self) -> Rc<BTreeMap<FillId, FillShape>> {
        let revision = self.doc.revision();
        if let Some((r, shapes)) = &self.fill_shapes
            && *r == revision
        {
            return shapes.clone();
        }
        let regions = self.regions();
        let ids = self.region_ids();
        let areas = Areas::new(&regions, &ids);
        let fills: Vec<(FillId, Fill)> = self.doc.fills().map(|(id, f)| (id, f.clone())).collect();
        let mut solid = None;
        let mut out = BTreeMap::new();
        for (id, fill) in fills {
            // A curve drawn across a fill cuts its outline into pieces: the fill keeps its
            // whole area, found again without the curves that cross it.
            let uncrossed = if areas.exact(&fill).is_none() {
                let solid = solid.get_or_insert_with(|| self.solid_curves());
                self.uncrossed_region(&fill, solid)
            } else {
                None
            };
            let found = uncrossed.or_else(|| {
                areas.find(&fill).map(|r| FillShape {
                    curves: curves_of(&fill, r, &ids),
                    region: r.clone(),
                    lost: false,
                })
            });
            if let Some(shape) = found {
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
                    },
                );
            }
        }
        let out = Rc::new(out);
        self.fill_shapes = Some((revision, out.clone()));
        out
    }

    /// Keep each fill on its area as an edit changes it, inside the edit's open transaction
    /// (so undo takes it back with the edit): when the edit replaced or split curves of a
    /// fill's outline, the area's new outline becomes the fill's, and a seed the edit left
    /// outside the area (a moved corner, a fillet over it) moves back in.
    pub(crate) fn follow_fills(&mut self) {
        if self.doc.fills().next().is_none() {
            return;
        }
        let regions = self.regions();
        let ids = self.region_ids();
        let areas = Areas::new(&regions, &ids);
        let fills: Vec<(FillId, Fill)> = self.doc.fills().map(|(id, f)| (id, f.clone())).collect();
        let mut solid = None;
        for (id, fill) in fills {
            let (region, outline) = if let Some(r) = areas.exact(&fill) {
                (r.clone(), None)
            } else if let Some(shape) =
                self.uncrossed_region(&fill, solid.get_or_insert_with(|| self.solid_curves()))
            {
                (shape.region, None)
            } else if let Some(r) = areas.successor(&fill) {
                (r.clone(), Some(outline_of(r, &ids)))
            } else {
                continue;
            };
            let mut followed = fill.clone();
            if let Some(outline) = outline {
                followed.boundary = outline;
            }
            followed.seed = crate::editor::interior_point(&region, fill.seed);
            self.doc.set_fill(id, followed);
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

    /// Copy the fills whose whole outline was copied onto the copies, inside the open
    /// transaction: `curves` gives each copied curve's copy, `place` moves a point as the
    /// copy was moved (turned or mirrored), and hatches turn with it.
    pub(crate) fn copy_fills_onto(
        &mut self,
        curves: impl Fn(EntityId) -> Option<EntityId>,
        place: impl Fn(DVec2) -> DVec2,
    ) {
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
        for (id, copy) in self.doc.instances() {
            let Some(block) = self.doc.block(copy.block) else {
                continue;
            };
            if Some(id) == editing || block.fills.is_empty() {
                continue;
            }
            let shapes: Vec<Shape> = self
                .doc
                .instance_items(id)
                .into_iter()
                .map(|item| item.shape)
                .collect();
            let regions = drift_geom::regions::regions(&shapes);
            for (index, f) in block.fills.iter().enumerate() {
                let fill = copy.apply_fill(f);
                if let Some(region) = region_at(&regions, fill.seed) {
                    out.push(CopyFill {
                        instance: id,
                        index,
                        region: region.clone(),
                        fill,
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
        let outline: Vec<Shape> = fill
            .boundary
            .iter()
            .filter_map(|id| self.doc.shape(*id))
            .collect();
        if outline.len() != fill.boundary.len() {
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
                fill.boundary.binary_search(id).is_ok()
                    || outline
                        .iter()
                        .all(|o| drift_geom::intersect::intersect(o, shape).is_empty())
            })
            .map(|(id, shape, _)| (*id, *shape))
            .unzip();
        let regions = drift_geom::regions::regions(&curves);
        // (Its curve indices are into these curves, not the sketch's regions'.)
        Areas::new(&regions, &ids).exact(fill).map(|r| FillShape {
            curves: curves_of(fill, r, &ids),
            region: r.clone(),
            lost: false,
        })
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

    pub fn reorder_fill(&mut self, id: FillId, front: bool) {
        self.doc.begin();
        self.doc.reorder_fill(id, front);
        self.commit(if front {
            "Bring to front"
        } else {
            "Send to back"
        });
    }
}

/// Tessellate a region (holes excluded) into world-space triangles. Arcs are flattened in
/// steps of at most `max_angle` radians.
pub fn tessellate(region: &Region, max_angle: f64) -> Option<(Vec<DVec2>, Vec<u32>)> {
    use lyon_tessellation::math::point;
    use lyon_tessellation::path::Path;
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    };

    // Tessellate relative to the region's centroid: lyon works in f32.
    let origin = region.centroid;
    let mut builder = Path::builder();
    for lp in std::iter::once(&region.outer).chain(&region.holes) {
        let poly = lp.polygon(max_angle);
        let Some(first) = poly.first() else {
            continue;
        };
        let to = |p: DVec2| {
            let d = p - origin;
            point(d.x as f32, d.y as f32)
        };
        builder.begin(to(*first));
        for p in &poly[1..] {
            builder.line_to(to(*p));
        }
        builder.close();
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<lyon_tessellation::math::Point, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default()
                .with_fill_rule(FillRule::EvenOdd)
                .with_tolerance(1e-4),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position()),
        )
        .ok()?;
    let vertices = buffers
        .vertices
        .iter()
        .map(|v| origin + DVec2::new(f64::from(v.x), f64::from(v.y)))
        .collect();
    Some((vertices, buffers.indices))
}

/// A tessellated fill: world vertices and triangle indices.
type Mesh = Rc<(Vec<DVec2>, Vec<u32>)>;

/// Which fill a tessellation is of: a fill, or a copy's fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MeshKey {
    Fill(FillId),
    Copy(InstanceId, usize),
}

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

/// What the canvas draws for fills: triangles under everything, and hatch lines (drawn
/// before the sketch's curves).
#[derive(Default)]
pub struct FillGraphics {
    pub meshes: Vec<FillMesh>,
    pub lines: Vec<SketchItem>,
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
        let own = fill.color.or(layer.and_then(|l| l.fill_colour()));
        let mut tint = |color: Rgb, alpha: f64, out: &mut FillGraphics| {
            let mesh = cache.get(revision, id, region);
            let [r, g, b] = color.0;
            out.meshes.push(FillMesh {
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
                    &mut out,
                );
            }
            FillKind::Hatch(h) => {
                if let Some(bg) = h.background {
                    tint(bg, 1.0, &mut out);
                }
                let line = own.unwrap_or(default_line_color(dark));
                let scale = h.scale * if h.model { 1.0 } else { draft_scale };
                let Some(pattern) = patterns.get(&h.pattern) else {
                    tint(line, 0.25, &mut out);
                    continue;
                };
                if pattern.min_spacing() * scale * px_per_mm < MIN_HATCH_SPACING_PX {
                    tint(line, 0.25, &mut out);
                    continue;
                }
                let placement = Placement {
                    scale,
                    angle: h.angle,
                    origin: h.origin,
                };
                let hatching = drift_draft::hatch(
                    region,
                    pattern,
                    &placement,
                    Some(view),
                    HATCH_SEGMENT_LIMIT,
                );
                if hatching.truncated {
                    tint(line, 0.25, &mut out);
                    continue;
                }
                let weight = fill.weight.unwrap_or_default().0;
                out.lines
                    .extend(hatching.segments.into_iter().map(|[a, b]| {
                        let mut item = SketchItem::new(Shape::Line { a, b });
                        item.color = own.map(|c| c.0);
                        item.weight = weight;
                        item
                    }));
            }
        }
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

        // Delete an edge: there is no area left, so the fill goes with it, and comes back
        // with it.
        ed.doc.begin();
        ed.doc.delete([lines[1]]);
        ed.commit("Delete");
        assert!(ed.doc.fill(id).is_none());
        assert!(!ed.fill_shapes().contains_key(&id));
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
        ed.fill_style.kind = FillKind::Hatch(Hatch::new("ANSI37"));
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
        assert!(g.meshes.is_empty());
        assert!(g.lines.len() > 10);
        // Zoomed far out: a tint instead.
        let g = fill_graphics(&mut ed, &mut cache, &patterns, view, 0.1, true);
        assert_eq!(g.meshes.len(), 1);
        assert!(g.lines.is_empty());
        let (vertices, indices) = (&g.meshes[0].vertices, &g.meshes[0].indices);
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
