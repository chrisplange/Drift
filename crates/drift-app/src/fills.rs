//! Fills and hatches: finding each fill's region as the geometry changes, creating and editing
//! fills, and turning them into GPU meshes and hatch lines.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use drift_core::{
    Bounds, DVec2, EntityId, Fill, FillId, FillKind, Hatch, LineWeight, Rgb, Shape, SketchItem,
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

/// Pick the region a fill belongs to: the one bounded by exactly its curves (nearest its
/// seed if several), else the one around its seed if it still shares a boundary curve.
fn find_region<'a>(regions: &'a [Region], ids: &[EntityId], fill: &Fill) -> Option<&'a Region> {
    let curves = |r: &Region| -> Vec<EntityId> {
        let mut c: Vec<EntityId> = r.outer.curves().into_iter().map(|i| ids[i]).collect();
        c.sort_unstable();
        c
    };
    let exact: Vec<&Region> = regions
        .iter()
        .filter(|r| curves(r) == fill.boundary)
        .collect();
    if let Some(r) = exact.iter().find(|r| r.contains(fill.seed)) {
        return Some(r);
    }
    if let Some(r) = exact.iter().min_by(|a, b| {
        a.centroid
            .distance(fill.seed)
            .total_cmp(&b.centroid.distance(fill.seed))
    }) {
        return Some(r);
    }
    region_at(regions, fill.seed).filter(|r| {
        curves(r)
            .iter()
            .any(|c| fill.boundary.binary_search(c).is_ok())
    })
}

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
        let fills: Vec<(FillId, Fill)> = self.doc.fills().map(|(id, f)| (id, f.clone())).collect();
        let mut out = BTreeMap::new();
        for (id, fill) in fills {
            if let Some(r) = find_region(&regions, &ids, &fill) {
                self.last_fill_shapes.insert(id, r.clone());
                out.insert(
                    id,
                    FillShape {
                        region: r.clone(),
                        lost: false,
                    },
                );
            } else if let Some(r) = self
                .last_fill_shapes
                .get(&id)
                .or_else(|| region_at(&regions, fill.seed))
            {
                out.insert(
                    id,
                    FillShape {
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
    /// of the first one: what moves or rotates when a fill is moved.
    pub fn selected_fill_curves(&mut self) -> Option<(Vec<EntityId>, DVec2)> {
        let shapes = self.fill_shapes();
        let mut ids = std::collections::BTreeSet::new();
        let mut centre = None;
        for f in self.selected_fills.clone() {
            if let Some(shape) = shapes.get(&f) {
                ids.extend(self.region_curves(&shape.region));
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

/// Tessellations by fill, kept while the document doesn't change.
#[derive(Default)]
pub struct MeshCache {
    revision: u64,
    meshes: HashMap<FillId, Mesh>,
}

impl MeshCache {
    fn get(&mut self, revision: u64, id: FillId, region: &Region) -> Mesh {
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
    let revision = ed.doc.revision();
    let draft_scale = ed.doc.draft_style().scale;
    for id in ed.fills_in_order() {
        let (Some(fill), Some(shape)) = (ed.doc.fill(id), shapes.get(&id)) else {
            continue;
        };
        let layer = ed.doc.layer(fill.layer);
        if layer.is_some_and(|l| !l.visible) {
            continue;
        }
        let bounds = region_bounds(&shape.region);
        if !bounds.intersects(&view) {
            continue;
        }
        let own = fill.color.or(layer.and_then(|l| l.fill_colour()));
        let mut tint = |color: Rgb, alpha: f64, out: &mut FillGraphics| {
            let mesh = cache.get(revision, id, &shape.region);
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
                    &shape.region,
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
}
