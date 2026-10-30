//! Underlays in the editor: drawing them, snapping to them, importing PDF and DXF files as
//! underlays, calibrating their scale and turning them into geometry.

use drift_core::{
    Bounds, DVec2, EntityId, LineKind, LineType, Shape, SketchItem, Underlay, UnderlayId,
    underlays::MM_PER_POINT,
};
use drift_geom::{RefIndex, Snap, SnapKind, SnapOptions};

use crate::editor::Editor;

/// A snap index per underlay, rebuilt only when that underlay moves or changes.
#[derive(Default)]
pub struct UnderlayCache {
    entries: Vec<Cached>,
}

struct Cached {
    id: UnderlayId,
    /// The curves' and text's identity and the placement they were indexed with.
    key: (usize, usize, [f64; 4]),
    index: RefIndex,
    /// Its text's outlines, to draw (never snapped to).
    glyphs: RefIndex,
    /// Every curve's and glyph's item in one colour, in the order the indexes give them:
    /// what is drawn while all of it is in view (made once, not each frame).
    whole: std::cell::RefCell<Option<WholeItems>>,
}

/// An underlay's items, all of them, in a colour.
struct WholeItems {
    color: [u8; 3],
    curves: std::rc::Rc<Vec<SketchItem>>,
    glyphs: std::rc::Rc<Vec<SketchItem>>,
}

fn key_of(u: &Underlay) -> (usize, usize, [f64; 4]) {
    (
        std::sync::Arc::as_ptr(&u.prims) as usize,
        std::sync::Arc::as_ptr(&u.glyphs) as usize,
        [u.at.x, u.at.y, u.scale, u.angle],
    )
}

/// What an import made.
#[derive(Clone, Debug, PartialEq)]
pub struct UnderlayReport {
    pub id: UnderlayId,
    pub curves: usize,
    /// For a PDF: which page, of how many.
    pub page: Option<(u32, u32)>,
    pub skipped: String,
}

/// Neutral grey underlays, by theme.
fn default_color(dark: bool) -> [u8; 3] {
    if dark {
        [0xa0, 0xa6, 0xb0]
    } else {
        [0x60, 0x66, 0x70]
    }
}

/// The canvas background (see `drift_render::Palette`), to fade underlays towards.
fn background(dark: bool) -> [u8; 3] {
    if dark {
        [0x1b, 0x1d, 0x21]
    } else {
        [0xfa, 0xfa, 0xfb]
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| (f64::from(a[i]) * t + f64::from(b[i]) * (1.0 - t)).round() as u8)
}

/// What an import brought in: its curves and points, its text, and the underlay its
/// pictures came in on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub entities: Vec<EntityId>,
    pub texts: Vec<drift_core::TextId>,
    pub underlays: Vec<UnderlayId>,
}

/// An underlay of the pictures imported artwork has (in paper mm), drawn at 1 : `scale`
/// with the paper's lower-left corner at `at`: they stay where they are on the page, under
/// the drawing that came in with them. `None` when there are none.
pub fn picture_underlay(
    name: &str,
    images: &[drift_core::UnderlayImage],
    scale: f64,
    at: DVec2,
) -> Option<Underlay> {
    if images.is_empty() {
        return None;
    }
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut u = Underlay::new(format!("{stem} pictures"), name, Vec::new());
    u.images = std::sync::Arc::new(images.to_vec());
    u.scale = scale;
    u.at = at;
    // Pictures show as they are (an underlay's curves are faded).
    u.opacity = 1.0;
    u.snap = false;
    Some(u)
}

/// Give an import's pictures their place in the drawing's stack: on the lowest layer its
/// fills came in on, over those fills and under its lines (where a drawing program shows
/// them over a filled background). Without fills they stay beneath the drawing. `before`
/// are the fills the drawing had before the import.
pub fn place_pictures(
    doc: &mut drift_core::Document,
    id: UnderlayId,
    before: &std::collections::BTreeSet<drift_core::FillId>,
) {
    let ranks = doc.layer_ranks();
    let new: Vec<(drift_core::LayerId, i32)> = doc
        .fills()
        .filter(|(f, _)| !before.contains(f))
        .map(|(_, f)| (f.layer, f.order))
        .collect();
    let Some(layer) = new
        .iter()
        .map(|(l, _)| *l)
        .min_by_key(|l| ranks.get(l).copied().unwrap_or(usize::MAX))
    else {
        return;
    };
    let order = new
        .iter()
        .filter(|(l, _)| *l == layer)
        .map(|(_, o)| *o)
        .max()
        .unwrap_or(0);
    if let Some(mut u) = doc.underlay(id).cloned() {
        u.layer = Some(layer);
        u.order = order;
        doc.set_underlay(id, u);
    }
}

impl Editor {
    /// Re-index underlays that were added, moved or changed since the last call.
    pub fn update_underlays(&mut self) {
        let doc = &self.doc;
        let cache = &mut self.underlay_cache;
        cache
            .entries
            .retain(|c| doc.underlay(c.id).is_some_and(|u| key_of(u) == c.key));
        for (id, u) in doc.underlays() {
            if !cache.entries.iter().any(|c| c.id == id) {
                cache.entries.push(Cached {
                    id,
                    key: key_of(u),
                    index: RefIndex::new(u.shapes()),
                    glyphs: RefIndex::new(u.glyph_shapes()),
                    whole: Default::default(),
                });
            }
        }
        cache.entries.sort_by_key(|c| c.id);
    }

    fn cached(&self, id: UnderlayId) -> Option<&RefIndex> {
        self.cached_entry(id).map(|c| &c.index)
    }

    fn cached_entry(&self, id: UnderlayId) -> Option<&Cached> {
        let u = self.doc.underlay(id)?;
        self.underlay_cache
            .entries
            .iter()
            .find(|c| c.id == id && c.key == key_of(u))
    }

    /// The pictures of visible underlays, each with its underlay.
    pub fn underlay_pictures(&self) -> Vec<(UnderlayId, drift_core::CanvasImage)> {
        let mut out = Vec::new();
        for (id, u) in self.doc.underlays().filter(|(_, u)| u.visible) {
            for image in u.images.iter() {
                out.push((
                    id,
                    drift_core::CanvasImage {
                        pixels: image.rgba.clone(),
                        width: image.width,
                        height: image.height,
                        corners: image.corners().map(|p| u.to_world(p)),
                        opacity: u.opacity as f32,
                    },
                ));
            }
        }
        out
    }

    /// The visible underlays' items in view, in runs to draw one after another. An underlay
    /// wholly in view gives its items as kept (big ones have hundreds of thousands).
    pub fn underlay_items(&self, view: Bounds, dark: bool) -> Vec<std::rc::Rc<Vec<SketchItem>>> {
        let mut runs = Vec::new();
        for (id, u) in self.doc.underlays() {
            if !u.visible {
                continue;
            }
            let Some(cached) = self.cached_entry(id) else {
                continue;
            };
            let base = u.color.map_or_else(|| default_color(dark), |c| c.0);
            let color = mix(base, background(dark), u.opacity);
            let item =
                |s: &Shape| SketchItem::new(*s).with_look(Some(color), 0, LineType::Continuous);
            // Mostly in view (fitted, or panned a little): all of it, as kept. The canvas
            // leaves out what is off screen, and the tree gives the items in the same order
            // whatever the box, so it draws what a query of the view would.
            let mostly_in_view = |index: &RefIndex| {
                index.bounds().is_none_or(|b| {
                    let size = b.max - b.min;
                    let seen = (view.max.min(b.max) - view.min.max(b.min)).max(DVec2::ZERO);
                    seen.x * seen.y >= 0.5 * size.x * size.y
                        && seen.x >= 0.5 * size.x
                        && seen.y >= 0.5 * size.y
                })
            };
            if mostly_in_view(&cached.index) && mostly_in_view(&cached.glyphs) {
                let mut whole = cached.whole.borrow_mut();
                if whole.as_ref().is_none_or(|w| w.color != color) {
                    let all = |index: &RefIndex| {
                        let every = index.bounds().unwrap_or(view);
                        std::rc::Rc::new(index.query(every).map(item).collect())
                    };
                    *whole = Some(WholeItems {
                        color,
                        curves: all(&cached.index),
                        glyphs: all(&cached.glyphs),
                    });
                }
                if let Some(w) = &*whole {
                    runs.push(w.curves.clone());
                    runs.push(w.glyphs.clone());
                }
                continue;
            }
            runs.push(std::rc::Rc::new(
                cached
                    .index
                    .query(view)
                    .chain(cached.glyphs.query(view))
                    .map(item)
                    .collect(),
            ));
        }
        runs
    }

    /// The best snap onto a visible underlay with snapping on.
    pub fn underlay_snap(&self, p: DVec2, tolerance: f64) -> Option<Snap> {
        let mut best: Option<(Snap, f64)> = None;
        for (id, u) in self.doc.underlays() {
            if !(u.visible && u.snap) {
                continue;
            }
            if let Some(s) = self.cached(id).and_then(|i| i.snap(p, tolerance)) {
                let d = s.at.distance(p);
                if best
                    .as_ref()
                    .is_none_or(|(b, bd)| (s.kind.priority(), d) < (b.kind.priority(), *bd))
                {
                    best = Some((s, d));
                }
            }
        }
        best.map(|(s, _)| s)
    }

    /// Snap to the sketch, to copies of blocks and to underlays: the sketch wins ties, and a
    /// grid snap only applies when nothing else is closer.
    pub fn snap(
        &self,
        p: DVec2,
        options: SnapOptions,
        exclude: impl Fn(EntityId) -> bool,
    ) -> Option<Snap> {
        self.snap_excluding(p, options, exclude, |_| false)
    }

    /// [`snap`](Self::snap), passing over the copies of blocks `exclude_copy` names too
    /// (those moving with the selection).
    pub fn snap_excluding(
        &self,
        p: DVec2,
        options: SnapOptions,
        exclude: impl Fn(EntityId) -> bool,
        exclude_copy: impl Fn(drift_core::InstanceId) -> bool,
    ) -> Option<Snap> {
        // (Walls' hidden centrelines aren't snapped to.)
        let hidden = self.hidden_centrelines();
        let snap = self.snap_sketch_and_underlays(p, options, |id| {
            exclude(id) || hidden.contains(&id) || !self.snappable(id)
        });
        let Some(block) = self.block_snap(p, options.tolerance, exclude_copy) else {
            return snap;
        };
        match snap {
            Some(s) if s.kind != SnapKind::Grid => {
                let (ds, db) = (s.at.distance(p), block.at.distance(p));
                if (block.kind.priority(), db) < (s.kind.priority(), ds) {
                    Some(block)
                } else {
                    Some(s)
                }
            }
            _ => Some(block),
        }
    }

    /// Whether the pointer may snap to an entity: its layer is snappable (for a point, the
    /// layer of a curve it belongs to, if any is).
    pub fn snappable(&self, id: EntityId) -> bool {
        let on = |e: EntityId| self.doc.layer_of(e).snappable;
        match self.doc.get(id) {
            Some(e) if e.kind.is_point() => {
                let mut users = self.doc.users_of(id).peekable();
                if users.peek().is_none() {
                    on(id)
                } else {
                    users.any(on)
                }
            }
            _ => on(id),
        }
    }

    fn snap_sketch_and_underlays(
        &self,
        p: DVec2,
        options: SnapOptions,
        exclude: impl Fn(EntityId) -> bool,
    ) -> Option<Snap> {
        let sketch = self.index.snap(p, options, exclude);
        // The origin is always there to snap to. A point of the sketch at it comes first (it
        // can be joined to); anything else there gives way.
        let sketch = match sketch {
            Some(s) if s.kind.is_point() => Some(s),
            _ if p.length() <= options.tolerance => Some(Snap {
                at: DVec2::ZERO,
                kind: SnapKind::Origin,
                entity: None,
            }),
            other => other,
        };
        let Some(reference) = self.underlay_snap(p, options.tolerance) else {
            return sketch;
        };
        match sketch {
            Some(s) if s.kind != SnapKind::Grid => {
                let (ds, dr) = (s.at.distance(p), reference.at.distance(p));
                if (reference.kind.priority(), dr) < (s.kind.priority(), ds) {
                    Some(reference)
                } else {
                    Some(s)
                }
            }
            _ => Some(reference),
        }
    }

    /// Add an underlay as one undo step, and pick it.
    pub fn add_underlay(&mut self, underlay: Underlay) -> UnderlayId {
        self.doc.begin();
        let id = self.doc.add_underlay(underlay);
        self.commit("Add underlay");
        self.selected_underlay = Some(id);
        id
    }

    /// Import an SVG drawing (as read) as an underlay drawn at 1 : `scale`, the lower-left
    /// corner of its paper at the origin.
    pub fn import_svg_underlay(
        &mut self,
        name: &str,
        art: &drift_io::vector::Artwork,
        scale: f64,
    ) -> Result<UnderlayReport, String> {
        let prims = drift_io::vector::outlines(art);
        let glyphs = drift_io::vector::text_outlines(art);
        if prims.is_empty() && glyphs.is_empty() {
            return Err(format!("{name} has no drawing to trace over"));
        }
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let mut u = Underlay::new(stem.to_owned(), name.to_owned(), prims);
        u.glyphs = std::sync::Arc::new(glyphs);
        u.scale = scale;
        let curves = u.curve_count();
        let skipped: Vec<String> = art
            .skipped
            .iter()
            .map(|(what, n)| format!("{n} {what}"))
            .collect();
        let id = self.add_underlay(u);
        Ok(UnderlayReport {
            id,
            curves,
            page: None,
            skipped: skipped.join(", "),
        })
    }

    /// Import page `page` of a PDF as an underlay drawn at 1 : `scale` (1 for paper size), its
    /// lower-left corner at the origin, with its pictures or without them.
    pub fn import_pdf_underlay(
        &mut self,
        name: &str,
        bytes: &[u8],
        page: u32,
        scale: f64,
        images: bool,
    ) -> Result<UnderlayReport, String> {
        let mut read = drift_io::pdf_import::read_page(bytes, page)?;
        if !images {
            read.images.clear();
        }
        if read.prims.is_empty() && read.images.is_empty() && read.glyphs.is_empty() {
            return Err(format!(
                "page {page} of {name} has nothing Drift can show (no drawing, text or pictures)"
            ));
        }
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let title = if read.pages > 1 {
            format!("{stem} p{page}")
        } else {
            stem.to_owned()
        };
        let mut u = Underlay::new(title, format!("{name}, page {page}"), read.prims);
        u.images = std::sync::Arc::new(read.images);
        u.glyphs = std::sync::Arc::new(read.glyphs);
        u.scale = MM_PER_POINT * scale;
        // A scan, with no drawing over it, shows as it is.
        if u.prims.is_empty() && u.glyphs.is_empty() {
            u.opacity = 1.0;
        }
        let curves = u.curve_count();
        let mut skipped = Vec::new();
        if read.skipped_text > 0 {
            skipped.push(format!(
                "{} text run(s) in a font Drift can't draw",
                read.skipped_text
            ));
        }
        if read.skipped_images > 0 {
            skipped.push(format!(
                "{} image(s) Drift can't read (JPEG 2000, JBIG2 or fax)",
                read.skipped_images
            ));
        }
        let id = self.add_underlay(u);
        Ok(UnderlayReport {
            id,
            curves,
            page: Some((page, read.pages)),
            skipped: skipped.join(", "),
        })
    }

    /// A DXF or DWG drawing's curves (as read, in mm) as an underlay drawn at 1 : `scale`.
    pub fn import_cad_underlay(
        &mut self,
        name: &str,
        import: &drift_io::dxf::DxfImport,
        scale: f64,
    ) -> Result<UnderlayReport, String> {
        let prims = drift_io::dxf::underlay_prims(import);
        let glyphs = drift_io::dxf::underlay_glyphs(import);
        if prims.is_empty() && glyphs.is_empty() {
            return Err(format!("{name} has no lines, arcs, circles or text"));
        }
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let mut u = Underlay::new(stem, name, prims);
        u.glyphs = std::sync::Arc::new(glyphs);
        u.scale = scale;
        let curves = u.curve_count();
        let skipped: Vec<String> = import
            .skipped
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect();
        let id = self.add_underlay(u);
        Ok(UnderlayReport {
            id,
            curves,
            page: None,
            skipped: skipped.join(", "),
        })
    }

    /// Change an underlay as one undo step.
    pub fn update_underlay(&mut self, id: UnderlayId, label: &str, f: impl FnOnce(&mut Underlay)) {
        let Some(mut u) = self.doc.underlay(id).cloned() else {
            return;
        };
        f(&mut u);
        if self.doc.underlay(id) == Some(&u) {
            return;
        }
        self.doc.begin();
        self.doc.set_underlay(id, u);
        self.commit(label);
    }

    /// Change an underlay; `continuing` folds the change into the previous undo step (while a
    /// slider is dragged).
    pub fn adjust_underlay(
        &mut self,
        id: UnderlayId,
        continuing: bool,
        f: impl FnOnce(&mut Underlay),
    ) {
        let Some(mut u) = self.doc.underlay(id).cloned() else {
            return;
        };
        f(&mut u);
        self.doc.begin();
        self.doc.set_underlay(id, u);
        let tx = self.doc.commit("Edit underlay");
        if continuing {
            self.history.merge_into_last(tx);
        } else {
            self.history.push(tx);
        }
        self.sync();
    }

    pub fn remove_underlay(&mut self, id: UnderlayId) {
        self.doc.begin();
        self.doc.remove_underlay(id);
        self.commit("Delete underlay");
        if self.selected_underlay == Some(id) {
            self.selected_underlay = None;
        }
    }

    /// Scale an underlay so the sketch distance from `a` to `b` (points on it) becomes
    /// `distance`, keeping `a` in place.
    pub fn calibrate_underlay(
        &mut self,
        id: UnderlayId,
        a: DVec2,
        b: DVec2,
        distance: f64,
    ) -> Result<(), String> {
        let mut u = self
            .doc
            .underlay(id)
            .cloned()
            .ok_or("That underlay is gone")?;
        if u.locked {
            return Err("The underlay is locked".into());
        }
        u.calibrate(a, b, distance)?;
        self.doc.begin();
        self.doc.set_underlay(id, u);
        self.commit("Calibrate underlay");
        Ok(())
    }

    /// Scale what the last import brought in about `a` so the distance from `a` to `b` is
    /// `distance` (one undo step): its curves, text and, through their outlines, its fills.
    /// Returns the factor it was scaled by.
    pub fn calibrate_import(&mut self, a: DVec2, b: DVec2, distance: f64) -> Result<f64, String> {
        let Some(set) = self.last_import.clone() else {
            return Err("Nothing imported to calibrate".into());
        };
        let measured = a.distance(b);
        if measured < 1e-9 {
            return Err("Pick two different points".into());
        }
        if distance.is_nan() || distance <= 0.0 {
            return Err("The distance must be more than zero".into());
        }
        let k = distance / measured;
        let to = |p: DVec2| a + (p - a) * k;
        let entities: Vec<EntityId> = set
            .entities
            .iter()
            .copied()
            .filter(|id| self.doc.get(*id).is_some())
            .collect();
        let points = self.doc.defining_points(entities.iter().copied());
        self.doc.begin();
        for p in &points {
            if let Ok(at) = self.doc.point(*p) {
                let _ = self.doc.set_point(*p, to(at));
            }
        }
        for id in &entities {
            if let Some(drift_core::EntityKind::Circle { radius, .. }) =
                self.doc.get(*id).map(|e| e.kind)
            {
                let _ = self.doc.set_radius(*id, radius * k);
            }
        }
        self.doc.normalize_arcs_using(&points);
        for id in &set.texts {
            if let Some(mut t) = self.doc.text(*id).cloned() {
                t.at = to(t.at);
                t.height *= k;
                t.wrap = t.wrap.map(|w| w * k);
                self.doc.set_text(*id, t);
            }
        }
        // Its pictures, with it.
        for id in &set.underlays {
            if let Some(mut u) = self.doc.underlay(*id).cloned() {
                u.at = to(u.at);
                u.scale *= k;
                self.doc.set_underlay(*id, u);
            }
        }
        self.commit("Calibrate import");
        Ok(k)
    }

    /// Move an underlay so the point `from` lands on `to`.
    pub fn move_underlay(&mut self, id: UnderlayId, from: DVec2, to: DVec2) -> Result<(), String> {
        let locked = self.doc.underlay(id).ok_or("That underlay is gone")?.locked;
        if locked {
            return Err("The underlay is locked".into());
        }
        self.update_underlay(id, "Move underlay", |u| u.at += to - from);
        Ok(())
    }

    /// Replace an underlay with ordinary geometry on the current layer (picked afterwards).
    /// Polylines become connected lines.
    pub fn convert_underlay(&mut self, id: UnderlayId) -> usize {
        let Some(u) = self.doc.underlay(id).cloned() else {
            return 0;
        };
        self.doc.begin();
        let mut points: std::collections::HashMap<(i64, i64), EntityId> = Default::default();
        // Points closer than a micrometre are shared, so traced outlines stay connected.
        let mut point = |doc: &mut drift_core::Document, p: DVec2| -> EntityId {
            let key = ((p.x * 1e3).round() as i64, (p.y * 1e3).round() as i64);
            *points.entry(key).or_insert_with(|| doc.add_point(p))
        };
        let mut created = Vec::new();
        for shape in u.shapes() {
            let made = match shape {
                Shape::Point(_) => None,
                Shape::Line { a, b } => {
                    let (pa, pb) = (point(&mut self.doc, a), point(&mut self.doc, b));
                    (pa != pb).then(|| self.doc.add_line(pa, pb).ok()).flatten()
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
                // Underlay curves are never ellipses (they're flattened on import).
                Shape::Ellipse { .. } | Shape::Bezier { .. } => None,
            };
            if let Some(e) = made {
                self.doc.set_style(e, LineKind::Solid);
                created.push(e);
            }
        }
        // Its pictures and text stay, as an underlay of their own.
        if u.images.is_empty() && u.glyphs.is_empty() {
            self.doc.remove_underlay(id);
        } else {
            self.doc.set_underlay(
                id,
                Underlay {
                    prims: std::sync::Arc::new(Vec::new()),
                    ..u
                },
            );
        }
        if !self.commit("Convert underlay") {
            return 0;
        }
        self.selected_underlay = None;
        self.selection = created.iter().copied().collect();
        created.len()
    }

    /// The underlay to act on: the picked one, else the only one.
    pub fn active_underlay(&self) -> Option<UnderlayId> {
        self.selected_underlay
            .filter(|id| self.doc.underlay(*id).is_some())
            .or_else(|| {
                let mut all = self.doc.underlays();
                match (all.next(), all.next()) {
                    (Some((id, _)), None) => Some(id),
                    _ => None,
                }
            })
    }
}

/// Read a DWG drawing, like a DXF one (see `drift_io::dwg_import`).
pub fn read_dwg(bytes: &[u8], mm_per_unit: f64) -> Result<drift_io::dxf::DxfImport, String> {
    read_dwg_at(bytes, mm_per_unit, 1.0)
}

/// Read a DWG drawing drawn at 1 : `drawn_at`.
#[cfg(feature = "dwg")]
pub fn read_dwg_at(
    bytes: &[u8],
    mm_per_unit: f64,
    drawn_at: f64,
) -> Result<drift_io::dxf::DxfImport, String> {
    drift_io::dwg_import::read_at(bytes, mm_per_unit, drawn_at)
}

/// Read a DWG drawing: not in builds without the `dwg` feature.
#[cfg(not(feature = "dwg"))]
pub fn read_dwg_at(
    _bytes: &[u8],
    _mm_per_unit: f64,
    _drawn_at: f64,
) -> Result<drift_io::dxf::DxfImport, String> {
    Err("this build of Drift can't read DWG files; save the drawing as DXF".into())
}

#[cfg(test)]
mod tests {
    use drift_core::{EntityKind, RefPrim};
    use drift_geom::SnapOptions;

    use super::*;
    use crate::editor::Pointer;
    use crate::tools::{Input, Modifiers, Tool, ToolKind};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A pointer snapped the way the canvas snaps (sketch and underlays).
    fn ptr(ed: &Editor, at: DVec2) -> Pointer {
        let mut p = Pointer::new(at, 0.5);
        p.snap = ed.snap(
            at,
            SnapOptions {
                tolerance: 0.5,
                grid: None,
                from: None,
            },
            |_| false,
        );
        p
    }

    fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
        let p = ptr(ed, at);
        tool.handle(Input::Hover(p), ed);
        tool.handle(Input::Click(p, Modifiers::default()), ed);
    }

    /// A 100 × 50 rectangle (as a closed polyline) and a diagonal line.
    fn plan() -> Underlay {
        Underlay::new(
            "Plan",
            "plan.pdf, page 1",
            vec![
                RefPrim::Polyline(vec![
                    v(0.0, 0.0),
                    v(100.0, 0.0),
                    v(100.0, 50.0),
                    v(0.0, 50.0),
                    v(0.0, 0.0),
                ]),
                RefPrim::Line(v(0.0, 0.0), v(100.0, 50.0)),
            ],
        )
    }

    #[test]
    fn lines_snap_onto_underlays_and_start_free() {
        let mut ed = Editor::default();
        let id = ed.add_underlay(plan());
        ed.update_underlays();
        let s = ed.snap(
            v(99.8, 0.3),
            SnapOptions {
                tolerance: 0.5,
                grid: None,
                from: None,
            },
            |_| false,
        );
        assert_eq!(
            s.map(|s| (s.kind, s.at, s.entity)),
            Some((SnapKind::Endpoint, v(100.0, 0.0), None))
        );
        // A line drawn from corner to corner lands exactly on them, as new points.
        let mut tool = ToolKind::Line.create();
        click(&mut ed, tool.as_mut(), v(0.2, 0.1));
        click(&mut ed, tool.as_mut(), v(99.9, 49.8));
        tool.handle(Input::Finish, &mut ed);
        let (_, line) = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .expect("a line");
        let EntityKind::Line { a, b } = line.kind else {
            unreachable!()
        };
        assert_eq!(ed.doc.point(a).unwrap(), v(0.0, 0.0));
        assert_eq!(ed.doc.point(b).unwrap(), v(100.0, 50.0));
        // Hidden or not snappable: no snap.
        ed.update_underlay(id, "Hide", |u| u.snap = false);
        ed.update_underlays();
        assert!(ed.underlay_snap(v(99.8, 0.3), 0.5).is_none());
        // Drawing items are culled to the view.
        let items = ed.underlay_items(
            Bounds::from_center_half_size(v(100.0, 25.0), v(1.0, 1.0)),
            true,
        );
        // The right-hand side, and the diagonal (whose bounds span the view); not the rest.
        assert_eq!(items.iter().map(|r| r.len()).sum::<usize>(), 2);
        // Wholly in view, as kept: the same items, in the same order, as a query gives.
        let whole = Bounds::from_center_half_size(v(50.0, 25.0), v(500.0, 500.0));
        let kept = ed.underlay_items(whole, true);
        let u = ed.doc.underlay(id).unwrap().clone();
        let queried: Vec<Shape> = {
            let c = ed.cached_entry(id).unwrap();
            c.index
                .query(whole)
                .chain(c.glyphs.query(whole))
                .copied()
                .collect()
        };
        let drawn: Vec<Shape> = kept
            .iter()
            .flat_map(|r| r.iter().map(|i| i.shape))
            .collect();
        assert_eq!(drawn, queried);
        assert!(!drawn.is_empty() && u.visible);
    }

    #[test]
    fn calibrate_and_move_with_the_tools() {
        let mut ed = Editor::default();
        let id = ed.add_underlay(plan());
        ed.update_underlays();
        // The bottom edge is really 5 m.
        let mut tool = ToolKind::Calibrate.create();
        click(&mut ed, tool.as_mut(), v(0.1, 0.1));
        click(&mut ed, tool.as_mut(), v(100.1, -0.1));
        assert_eq!(tool.fields(), ["Distance"]);
        tool.set_typed([Some(5000.0), None]);
        let p = ptr(&ed, v(100.0, 0.0));
        tool.commit_typed(&p, &mut ed);
        let u = ed.doc.underlay(id).unwrap();
        assert!((u.scale - 50.0).abs() < 1e-9, "{}", u.scale);
        assert_eq!(
            u.to_world(v(0.0, 0.0)),
            v(0.0, 0.0),
            "the first point stays"
        );
        assert!(ed.notice.as_deref().is_some_and(|n| n.contains("1 : ")));
        // Move its corner onto (1000, 2000).
        ed.update_underlays();
        let mut tool = ToolKind::MoveUnderlay.create();
        click(&mut ed, tool.as_mut(), v(5000.2, 0.1));
        let mut p = ptr(&ed, v(1000.0, 2000.0));
        p.snap = None;
        tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
        let u = ed.doc.underlay(id).unwrap();
        assert!(u.to_world(v(100.0, 0.0)).distance(v(1000.0, 2000.0)) < 1e-9);
        // Undo puts it back where it was calibrated.
        ed.undo();
        let u = ed.doc.underlay(id).unwrap();
        assert!(u.to_world(v(100.0, 0.0)).distance(v(5000.0, 0.0)) < 1e-9);
        // A locked underlay refuses.
        ed.update_underlay(id, "Lock", |u| u.locked = true);
        assert!(
            ed.calibrate_underlay(id, v(0.0, 0.0), v(1.0, 0.0), 2.0)
                .is_err()
        );
    }

    #[test]
    fn convert_to_connected_geometry() {
        let mut ed = Editor::default();
        let id = ed.add_underlay(plan());
        let n = ed.convert_underlay(id);
        assert_eq!(n, 5);
        assert!(ed.doc.underlay(id).is_none());
        let points = ed.doc.iter().filter(|(_, e)| e.kind.is_point()).count();
        assert_eq!(points, 4, "corners are shared");
        assert_eq!(ed.selection.len(), 5);
        ed.undo();
        assert!(ed.doc.underlay(id).is_some() && ed.doc.is_empty());
    }

    #[test]
    fn imports_pdf_pages_and_dxf_files() {
        let mut d = drift_draft::page::Drawing::new(v(297.0, 210.0));
        d.push(
            "0",
            drift_draft::page::Pen::new(0.35),
            drift_draft::page::Prim::Line(v(10.0, 10.0), v(110.0, 10.0)),
        );
        let bytes = drift_io::pdf::write(&[d.clone(), d.clone()], "Plan");
        let mut ed = Editor::default();
        let report = ed
            .import_pdf_underlay("plan.pdf", &bytes, 2, 1.0, true)
            .unwrap();
        assert_eq!(report.page, Some((2, 2)));
        let u = ed.doc.underlay(report.id).unwrap();
        assert_eq!(u.name, "plan p2");
        // At paper size: the 100 mm line is 100 mm.
        let s = u.shapes();
        let Shape::Line { a, b } = s[0] else { panic!() };
        assert!((a.distance(b) - 100.0).abs() < 0.01);
        assert!(
            ed.import_pdf_underlay("plan.pdf", &bytes, 3, 1.0, true)
                .is_err()
        );

        let dxf = drift_io::dxf::write(&d, drift_io::dxf::DxfUnits::MM);
        let import = drift_io::dxf::read(&dxf, 1.0).unwrap();
        let report = ed.import_cad_underlay("plan.dxf", &import, 1.0).unwrap();
        assert_eq!(report.curves, 1);
        assert_eq!(ed.doc.underlays().count(), 2);
    }

    /// The letters' box: lower-left and upper-right corners.
    fn glyph_bounds(u: &Underlay) -> Bounds {
        u.glyph_shapes().iter().map(Shape::bounds).collect()
    }

    #[test]
    fn dxf_and_svg_underlays_show_their_text() {
        // A DXF with a 5 mm label at (20, 30), beside its line.
        let mut d = drift_draft::page::Drawing::new(v(297.0, 210.0));
        let pen = drift_draft::page::Pen::new(0.35);
        d.push(
            "0",
            pen,
            drift_draft::page::Prim::Line(v(10.0, 10.0), v(110.0, 10.0)),
        );
        d.text(
            "0",
            pen,
            v(20.0, 30.0),
            5.0,
            drift_draft::page::Align::Left,
            "KITCHEN",
        );
        let dxf = drift_io::dxf::write(&d, drift_io::dxf::DxfUnits::MM);
        let import = drift_io::dxf::read(&dxf, 1.0).unwrap();
        let mut ed = Editor::default();
        let report = ed.import_cad_underlay("plan.dxf", &import, 1.0).unwrap();
        // Its letters are drawn, not counted as curves to snap to.
        assert_eq!(report.curves, 1);
        let u = ed.doc.underlay(report.id).unwrap();
        let b = glyph_bounds(u);
        assert!(b.min.distance(v(20.0, 30.0)) < 1.0, "{b:?}");
        assert!((b.max.y - 35.0).abs() < 0.5 && b.max.x > 40.0, "{b:?}");
        // An SVG's text, in paper mm: 10 px type is 10 × 25.4/96 mm, its baseline 20 px up
        // from the bottom of a 100 px page.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"
            viewBox="0 0 100 100"><path d="M10 50 L60 50" stroke="black"/>
            <text x="10" y="80" font-size="10">HALL</text></svg>"#;
        let art = drift_io::svg_import::read(svg, "plan.svg").unwrap();
        let report = ed.import_svg_underlay("plan.svg", &art, 1.0).unwrap();
        let u = ed.doc.underlay(report.id).unwrap();
        let b = glyph_bounds(u);
        let mm = drift_core::units::MM_PER_INCH / 96.0;
        assert!(b.min.distance(v(10.0, 20.0) * mm) < 0.5, "{b:?}");
        assert!(b.max.x > 30.0 * mm && b.max.y < 30.0 * mm, "{b:?}");
    }

    #[test]
    fn pictures_follow_their_import_and_outlast_converted_curves() {
        let picture = drift_core::UnderlayImage {
            width: 1,
            height: 1,
            rgba: std::sync::Arc::new(vec![10, 20, 30, 255]),
            at: v(0.0, 10.0),
            across: v(20.0, 0.0),
            down: v(0.0, -10.0),
        };
        // As an import at 1:50 places them: paper mm, the paper's corner where it goes.
        let u = picture_underlay(
            "plan.pdf",
            std::slice::from_ref(&picture),
            50.0,
            v(100.0, 0.0),
        )
        .expect("pictures");
        assert_eq!((u.name.as_str(), u.opacity), ("plan pictures", 1.0));
        let mut ed = Editor::default();
        ed.doc.begin();
        let id = ed.doc.add_underlay(u);
        ed.commit("Import");
        let b = ed.doc.underlay(id).unwrap().bounds();
        assert!(b.min.distance(v(100.0, 0.0)) < 1e-9 && b.max.distance(v(1100.0, 500.0)) < 1e-9);
        // Calibrating the import (twice as big, about its corner) takes them along.
        ed.last_import = Some(Imported {
            underlays: vec![id],
            ..Default::default()
        });
        ed.calibrate_import(v(100.0, 0.0), v(1100.0, 0.0), 2000.0)
            .unwrap();
        let b = ed.doc.underlay(id).unwrap().bounds();
        assert!(b.min.distance(v(100.0, 0.0)) < 1e-9 && b.max.distance(v(2100.0, 1000.0)) < 1e-9);
        // Converting an underlay's curves leaves its pictures, as an underlay.
        let mut both = plan();
        both.images = std::sync::Arc::new(vec![picture]);
        let id = ed.add_underlay(both);
        assert_eq!(ed.convert_underlay(id), 5);
        let left = ed.doc.underlay(id).expect("the pictures stay");
        assert!(left.prims.is_empty() && left.images.len() == 1);
        assert!(picture_underlay("plan.pdf", &[], 1.0, DVec2::ZERO).is_none());
    }
}
