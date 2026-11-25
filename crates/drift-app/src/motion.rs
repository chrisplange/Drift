//! Solves that ease into place (#514).
//!
//! When a typed value, a new relation, an undo or a redo moves geometry, the canvas shows it
//! easing from where it was to where it now is over [`DURATION`], instead of jumping. Only
//! the drawing moves: the document holds the solved geometry from the start, and picking,
//! snapping, hover, readouts, the Inspector and exports all read it. The canvas draws each
//! frame of the animation from a throwaway copy of the document with the moving points and
//! radii part-way ([`SolveMotion::frame`]), and never writes that copy back.
//!
//! What moved is found by keeping each point's position and each circle's radius as last
//! seen, brought up to date from what the document says changed since
//! ([`Document::changes_since`]), so a frame costs nothing when nothing changed.
//!
//! No animation while something is dragged (on the canvas or a slider), while a tool is
//! part-way through (typed values while drawing), or inside a transaction: those already
//! move every frame. A change of more than [`MAX_POINTS`] points, or one that carries any
//! point further than [`MAX_TRAVEL`] of the canvas's diagonal on screen, lands at once.
//!
//! Walls ease too: their faces' points slide like any others, and each frame makes a wall's
//! poché again from its faces as shown (one map can't carry it: an opening sliding along a
//! wall leaves the rest of it where it is). A change that remakes walls or openings rather
//! than moving them (an opening added or taken out, walls joining: points that come or go)
//! lands at once, with nothing to slide between.

use std::collections::{HashMap, HashSet};

use drift_core::{DVec2, Document, EntityId, EntityKind, FillMesh, Shape, SketchItem};

/// How long a solve takes to ease into place, in seconds.
pub(crate) const DURATION: f64 = 0.12;
/// More points moved than this: the change lands at once.
pub(crate) const MAX_POINTS: usize = 1000;
/// A point carried further than this share of the canvas's diagonal on screen: the change
/// lands at once.
pub(crate) const MAX_TRAVEL: f64 = 0.5;

/// Cubic ease-out: fast away, slow into place.
pub(crate) fn ease(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// What of an entity a solve can change on its own: a point's position or a circle's radius
/// (every other curve is made from its points).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Datum {
    Point(DVec2),
    Radius(f64),
}

impl Datum {
    fn of(doc: &Document, id: EntityId) -> Option<Self> {
        match doc.get(id)?.kind {
            EntityKind::Point(at) => Some(Self::Point(at)),
            EntityKind::Circle { radius, .. } => Some(Self::Radius(radius)),
            _ => None,
        }
    }

    fn lerp(self, to: Self, t: f64) -> Self {
        match (self, to) {
            (Self::Point(a), Self::Point(b)) => Self::Point(a + (b - a) * t),
            (Self::Radius(a), Self::Radius(b)) => Self::Radius(a + (b - a) * t),
            _ => to,
        }
    }

    /// How far apart two values are, in world units.
    fn distance(self, to: Self) -> f64 {
        match (self, to) {
            (Self::Point(a), Self::Point(b)) => a.distance(b),
            (Self::Radius(a), Self::Radius(b)) => (a - b).abs(),
            _ => 0.0,
        }
    }
}

/// When a change may ease into place, and how big the canvas is to judge its size by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gate {
    /// The preference is on and the operating system doesn't ask for reduced motion.
    pub enabled: bool,
    /// Nothing is being dragged or drawn: the change came all at once.
    pub allowed: bool,
    /// Screen points per world unit.
    pub scale: f64,
    /// The canvas's diagonal, in screen points.
    pub diagonal: f64,
}

/// An animation under way.
struct Run {
    start: f64,
    /// Each moving entity: from where it was shown to where it is.
    moves: HashMap<EntityId, (Datum, Datum)>,
    /// The curves made from what moves (and what moves), whose drawing changes.
    affected: HashSet<EntityId>,
}

impl Run {
    fn value(&self, id: EntityId, now: f64) -> Option<Datum> {
        let (from, to) = self.moves.get(&id)?;
        Some(from.lerp(*to, ease((now - self.start) / DURATION)))
    }
}

/// Watches the document for solves and runs their animations (see the [module](self)).
#[derive(Default)]
pub(crate) struct SolveMotion {
    /// What `known` was last brought up to date with (none: not kept, the preference is off).
    seen: Option<(u64, u64)>,
    known: HashMap<EntityId, Datum>,
    run: Option<Run>,
    /// Animations started (tests).
    pub(crate) started: usize,
}

/// One frame of an animation: the document as it is shown, and what to draw differently.
pub(crate) struct Frame {
    pub doc: Document,
    /// Each affected entity's shape as solved, by its bits, to the shape shown.
    shapes: HashMap<ShapeKey, Shape>,
    affected: HashSet<EntityId>,
}

impl SolveMotion {
    /// Look at the document as it is now, before it is drawn this frame, and start an
    /// animation for what a solve just moved (if `gate` lets it).
    pub(crate) fn observe(&mut self, doc: &Document, now: f64, gate: Gate) {
        if !gate.enabled {
            // Off: nothing is kept and nothing runs, as before the preference.
            self.seen = None;
            self.known = HashMap::new();
            self.run = None;
            return;
        }
        let at = (doc.lineage(), doc.revision());
        let changed = match self.seen {
            Some(seen) if seen == at => return,
            Some((lineage, revision)) => doc.changes_since(lineage, revision),
            None => None,
        };
        self.seen = Some(at);
        let Some(changed) = changed else {
            // Another document, or too far behind to tell: start again from it as it is.
            self.known = doc
                .iter()
                .filter_map(|(id, _)| Some((id, Datum::of(doc, id)?)))
                .collect();
            self.run = None;
            return;
        };
        if changed.entities.is_empty() {
            return;
        }
        let mut moves = HashMap::new();
        // Points that came or went: the drawing's make-up changed, not only where things are.
        let mut remade = false;
        for &id in &changed.entities {
            let now_is = Datum::of(doc, id);
            let was = match now_is {
                Some(d) => self.known.insert(id, d),
                None => self.known.remove(&id),
            };
            remade |= was.is_some() != now_is.is_some();
            if let (Some(was), Some(to)) = (was, now_is)
                && was != to
                && std::mem::discriminant(&was) == std::mem::discriminant(&to)
            {
                // From where it is shown: part-way, when a solve lands mid-animation.
                let from = self
                    .run
                    .as_ref()
                    .and_then(|r| r.value(id, now))
                    .unwrap_or(was);
                moves.insert(id, (from, to));
            }
        }
        if !gate.allowed {
            // A drag or a tool moves things every frame: what is under way stops where the
            // geometry is.
            self.run = None;
            return;
        }
        // What was still easing in carries on from where it is shown.
        if let Some(run) = self.run.take() {
            for (id, (_, to)) in &run.moves {
                if !moves.contains_key(id)
                    && self.known.get(id) == Some(to)
                    && let Some(shown) = run.value(*id, now)
                {
                    moves.insert(*id, (shown, *to));
                }
            }
        }
        moves.retain(|_, (from, to)| from != to);
        if moves.is_empty() {
            return;
        }
        let points = moves
            .values()
            .filter(|(f, _)| matches!(f, Datum::Point(_)))
            .count();
        let travel = moves
            .values()
            .map(|(f, t)| f.distance(*t))
            .fold(0.0, f64::max);
        if points > MAX_POINTS || travel * gate.scale > MAX_TRAVEL * gate.diagonal {
            return;
        }
        if remade && moves_what_walls_make(doc, &moves) {
            return;
        }
        let mut affected: HashSet<EntityId> = moves.keys().copied().collect();
        for (id, e) in doc.iter() {
            if !affected.contains(&id) && e.kind.points().any(|p| moves.contains_key(&p)) {
                affected.insert(id);
            }
        }
        self.started += 1;
        self.run = Some(Run {
            start: now,
            moves,
            affected,
        });
    }

    /// Whether an animation is under way at `now`; one that has run its course ends here.
    pub(crate) fn tick(&mut self, now: f64) -> bool {
        if self.run.as_ref().is_some_and(|r| now - r.start >= DURATION) {
            self.run = None;
        }
        self.run.is_some()
    }

    #[cfg(test)]
    pub(crate) fn animating(&self) -> bool {
        self.run.is_some()
    }

    /// Where a point is shown at `now`, while it eases into place.
    #[cfg(test)]
    pub(crate) fn shown_point(&self, id: EntityId, now: f64) -> Option<DVec2> {
        match self.run.as_ref()?.value(id, now)? {
            Datum::Point(p) => Some(p),
            Datum::Radius(_) => None,
        }
    }

    /// The frame to draw at `now`, while an animation is under way.
    pub(crate) fn frame(&self, doc: &Document, now: f64) -> Option<Frame> {
        let run = self.run.as_ref()?;
        let mut shown = doc.clone();
        for &id in run.moves.keys() {
            // (A value the document refuses, e.g. a radius through zero, shows as solved.)
            let _ = match run.value(id, now)? {
                Datum::Point(p) => shown.set_point(id, p),
                Datum::Radius(r) => shown.set_radius(id, r),
            };
        }
        let shapes = run
            .affected
            .iter()
            .filter_map(|&id| {
                let solved = doc.shape(id)?;
                let now = shown.shape(id)?;
                (solved != now).then(|| (ShapeKey::of(&solved), now))
            })
            .collect();
        Some(Frame {
            doc: shown,
            shapes,
            affected: run.affected.clone(),
        })
    }
}

impl Frame {
    /// Draw the items of what is easing in where it is shown. Items are matched to the
    /// entities they draw by their shapes as solved (the items carry no entity).
    pub(crate) fn patch_items(&self, items: &mut [SketchItem]) {
        if self.shapes.is_empty() {
            return;
        }
        for item in items {
            if let Some(shown) = self.shapes.get(&ShapeKey::of(&item.shape)) {
                item.shape = *shown;
            }
        }
    }

    /// Whether any of `curves` is drawn differently this frame.
    pub(crate) fn touches(&self, curves: &[EntityId]) -> bool {
        curves.iter().any(|c| self.affected.contains(c))
    }

    /// The map that carries a fill drawn on `curves` (as solved) to where they are shown:
    /// the affine map that fits them best, exact for a fill moved, turned or stretched. None
    /// if none of them moves.
    pub(crate) fn fill_map(&self, solved: &Document, curves: &[EntityId]) -> Option<Affine> {
        if !curves.iter().any(|c| self.affected.contains(c)) {
            return None;
        }
        let mut pairs = Vec::new();
        for &c in curves {
            let (Some(a), Some(b)) = (solved.shape(c), self.doc.shape(c)) else {
                continue;
            };
            for t in samples(&a) {
                pairs.push((a.point_at(t), b.point_at(t)));
            }
        }
        Affine::fit(&pairs)
    }
}

impl crate::DriftApp {
    /// Look for a solve to ease into place, after this frame's input and before it is
    /// drawn, and return the frame to draw while one does (#514). Asks for the next frame
    /// while an animation runs, and for none once it has ended.
    pub(crate) fn solve_frame(
        &mut self,
        ctx: &eframe::egui::Context,
        canvas: &eframe::egui::Response,
        rect: eframe::egui::Rect,
    ) -> Option<Frame> {
        let now = ctx.input(|i| i.time);
        let enabled = self.animate_solves && !drift_platform::reduce_motion();
        // Dragged on the canvas or a slider, or clicked on the canvas (a tool's edit), or a
        // tool part-way through (typed values while drawing): those move as they go.
        let allowed = ctx.dragged_id().is_none()
            && ctx.drag_stopped_id().is_none()
            && !canvas.is_pointer_button_down_on()
            && !canvas.clicked()
            && !canvas.drag_stopped()
            && !self.tool.is_busy()
            && !self.drawing.editor.doc.in_transaction();
        let gate = Gate {
            enabled,
            allowed,
            scale: self.drawing.view.camera.scale,
            diagonal: f64::from(rect.size().length()),
        };
        self.solve_motion
            .observe(&self.drawing.editor.doc, now, gate);
        if !self.solve_motion.tick(now) {
            return None;
        }
        ctx.request_repaint();
        self.solve_motion.frame(&self.drawing.editor.doc, now)
    }
}

/// Whether any of `moves` is a point walls or openings made.
fn moves_what_walls_make(doc: &Document, moves: &HashMap<EntityId, (Datum, Datum)>) -> bool {
    if !doc.has_walls() {
        return false;
    }
    let walls = doc.walls().flat_map(|(_, w)| {
        w.made
            .points
            .values()
            .chain(w.made.curves.values())
            .copied()
    });
    let openings = doc
        .openings()
        .flat_map(|(_, o)| o.made.entities().collect::<Vec<_>>());
    walls.chain(openings).any(|id| moves.contains_key(&id))
}

/// The parameters a curve is sampled at to fit a fill's map.
fn samples(shape: &Shape) -> Vec<f64> {
    use std::f64::consts::TAU;
    const N: usize = 8;
    let span = |from: f64, sweep: f64| (0..=N).map(move |k| from + sweep * k as f64 / N as f64);
    match *shape {
        Shape::Point(_) => vec![0.0],
        Shape::Line { .. } | Shape::Bezier { .. } => span(0.0, 1.0).collect(),
        Shape::Circle { .. } => span(0.0, TAU).collect(),
        Shape::Arc { start, sweep, .. } | Shape::Ellipse { start, sweep, .. } => {
            span(start, sweep).collect()
        }
    }
}

/// `p ↦ m·(p − from) + to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine {
    from: DVec2,
    to: DVec2,
    m: [[f64; 2]; 2],
}

impl Affine {
    /// The least-squares affine map taking each pair's first point to its second; a plain
    /// move when the points lie on a line.
    pub(crate) fn fit(pairs: &[(DVec2, DVec2)]) -> Option<Self> {
        if pairs.is_empty() {
            return None;
        }
        let n = pairs.len() as f64;
        let from = pairs.iter().map(|p| p.0).sum::<DVec2>() / n;
        let to = pairs.iter().map(|p| p.1).sum::<DVec2>() / n;
        // m = (Σ dy·dxᵀ)(Σ dx·dxᵀ)⁻¹
        let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
        let mut c = [[0.0; 2]; 2];
        for (a, b) in pairs {
            let (dx, dy) = (*a - from, *b - to);
            xx += dx.x * dx.x;
            xy += dx.x * dx.y;
            yy += dx.y * dx.y;
            c[0][0] += dy.x * dx.x;
            c[0][1] += dy.x * dx.y;
            c[1][0] += dy.y * dx.x;
            c[1][1] += dy.y * dx.y;
        }
        let det = xx * yy - xy * xy;
        let m = if det.abs() <= 1e-12 * (xx * yy).max(f64::MIN_POSITIVE) {
            [[1.0, 0.0], [0.0, 1.0]]
        } else {
            let inv = [[yy / det, -xy / det], [-xy / det, xx / det]];
            [
                [
                    c[0][0] * inv[0][0] + c[0][1] * inv[1][0],
                    c[0][0] * inv[0][1] + c[0][1] * inv[1][1],
                ],
                [
                    c[1][0] * inv[0][0] + c[1][1] * inv[1][0],
                    c[1][0] * inv[0][1] + c[1][1] * inv[1][1],
                ],
            ]
        };
        Some(Self { from, to, m })
    }

    pub(crate) fn apply(&self, p: DVec2) -> DVec2 {
        let d = p - self.from;
        self.to
            + DVec2::new(
                self.m[0][0] * d.x + self.m[0][1] * d.y,
                self.m[1][0] * d.x + self.m[1][1] * d.y,
            )
    }

    /// A fill's triangles and hatch lines, carried by the map.
    pub(crate) fn carry(&self, meshes: &mut [FillMesh], lines: &mut [SketchItem]) {
        for mesh in meshes {
            for v in &mut mesh.vertices {
                *v = self.apply(*v);
            }
        }
        for item in lines {
            match &mut item.shape {
                Shape::Point(p) => *p = self.apply(*p),
                Shape::Line { a, b } => {
                    *a = self.apply(*a);
                    *b = self.apply(*b);
                }
                Shape::Bezier { p } => {
                    for q in p {
                        *q = self.apply(*q);
                    }
                }
                // (Hatches are lines and dots.)
                Shape::Circle { .. } | Shape::Arc { .. } | Shape::Ellipse { .. } => {}
            }
        }
    }
}

/// A shape's exact bits, to find the items that draw it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShapeKey(u8, [u64; 8]);

impl ShapeKey {
    fn of(shape: &Shape) -> Self {
        let mut k = [0u64; 8];
        let mut put = |vals: &[f64]| {
            for (slot, v) in k.iter_mut().zip(vals) {
                *slot = v.to_bits();
            }
        };
        let tag = match *shape {
            Shape::Point(p) => {
                put(&[p.x, p.y]);
                0
            }
            Shape::Line { a, b } => {
                put(&[a.x, a.y, b.x, b.y]);
                1
            }
            Shape::Circle { center, radius } => {
                put(&[center.x, center.y, radius]);
                2
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                put(&[center.x, center.y, radius, start, sweep]);
                3
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                put(&[center.x, center.y, major.x, major.y, minor, start, sweep]);
                4
            }
            Shape::Bezier { p } => {
                put(&[
                    p[0].x, p[0].y, p[1].x, p[1].y, p[2].x, p[2].y, p[3].x, p[3].y,
                ]);
                5
            }
        };
        Self(tag, k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Gate = Gate {
        enabled: true,
        allowed: true,
        scale: 1.0,
        diagonal: 1000.0,
    };

    /// A line from (0, 0) to (10, 0): its ends and itself.
    fn line() -> (Document, EntityId, EntityId, EntityId) {
        let mut doc = Document::default();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(10.0, 0.0));
        let l = doc.add_line(a, b).unwrap();
        (doc, a, b, l)
    }

    #[test]
    fn a_moved_point_eases_from_where_it_was_and_ends_where_it_is() {
        let (mut doc, _, b, l) = line();
        let mut m = SolveMotion::default();
        m.observe(&doc, 0.0, ON);
        doc.set_point(b, DVec2::new(20.0, 0.0)).unwrap();
        m.observe(&doc, 1.0, ON);
        assert!(m.tick(1.0));
        assert_eq!(m.shown_point(b, 1.0), Some(DVec2::new(10.0, 0.0)));
        let half = m.shown_point(b, 1.0 + DURATION / 2.0).unwrap();
        assert!(half.x > 15.0 && half.x < 20.0, "eased out: {half}");
        // The document is as solved; only the frame's copy is part-way.
        assert_eq!(doc.point(b).unwrap(), DVec2::new(20.0, 0.0));
        let frame = m.frame(&doc, 1.0 + DURATION / 2.0).unwrap();
        assert_eq!(frame.doc.point(b).unwrap(), half);
        // The line's item, drawn as solved, is drawn where it is shown.
        let mut items = [SketchItem::new(doc.shape(l).unwrap())];
        frame.patch_items(&mut items);
        assert_eq!(items[0].shape, frame.doc.shape(l).unwrap());
        // Over once its time is up: nothing more to draw.
        assert!(!m.tick(1.0 + DURATION));
        assert!(m.frame(&doc, 1.0 + DURATION).is_none());
    }

    #[test]
    fn off_keeps_nothing_and_runs_nothing() {
        let (mut doc, _, b, _) = line();
        let mut m = SolveMotion::default();
        let off = Gate {
            enabled: false,
            ..ON
        };
        m.observe(&doc, 0.0, off);
        doc.set_point(b, DVec2::new(20.0, 0.0)).unwrap();
        m.observe(&doc, 1.0, off);
        assert!(!m.tick(1.0));
        assert!(m.known.is_empty());
    }

    #[test]
    fn a_drag_or_a_tool_moves_without_easing_and_stops_what_runs() {
        let (mut doc, _, b, _) = line();
        let mut m = SolveMotion::default();
        m.observe(&doc, 0.0, ON);
        doc.set_point(b, DVec2::new(20.0, 0.0)).unwrap();
        m.observe(&doc, 1.0, ON);
        assert!(m.animating());
        doc.set_point(b, DVec2::new(21.0, 0.0)).unwrap();
        let dragging = Gate {
            allowed: false,
            ..ON
        };
        m.observe(&doc, 1.01, dragging);
        assert!(!m.animating());
        // And what the drag did isn't eased in afterwards.
        m.observe(&doc, 1.02, ON);
        assert!(!m.animating());
    }

    #[test]
    fn a_solve_mid_animation_starts_from_where_things_are_shown() {
        let (mut doc, _, b, _) = line();
        let mut m = SolveMotion::default();
        m.observe(&doc, 0.0, ON);
        doc.set_point(b, DVec2::new(20.0, 0.0)).unwrap();
        m.observe(&doc, 1.0, ON);
        let t = 1.0 + DURATION / 2.0;
        let shown = m.shown_point(b, t).unwrap();
        doc.set_point(b, DVec2::new(30.0, 0.0)).unwrap();
        m.observe(&doc, t, ON);
        assert_eq!(m.shown_point(b, t), Some(shown));
        assert!(m.tick(t + DURATION * 0.9), "a fresh run");
    }

    #[test]
    fn a_big_change_lands_at_once() {
        let (mut doc, _, b, _) = line();
        let mut m = SolveMotion::default();
        m.observe(&doc, 0.0, ON);
        // Further than half the diagonal on screen.
        doc.set_point(b, DVec2::new(10.0 + 501.0, 0.0)).unwrap();
        m.observe(&doc, 1.0, ON);
        assert!(!m.animating());
        // More points than the limit.
        let points: Vec<_> = (0..=MAX_POINTS)
            .map(|k| doc.add_point(DVec2::new(k as f64, 5.0)))
            .collect();
        m.observe(&doc, 2.0, ON);
        for p in &points {
            let at = doc.point(*p).unwrap();
            doc.set_point(*p, at + DVec2::Y).unwrap();
        }
        m.observe(&doc, 3.0, ON);
        assert!(!m.animating());
        assert_eq!(m.started, 0);
    }

    #[test]
    fn a_fill_map_moves_turns_and_stretches_exactly() {
        let pairs: Vec<(DVec2, DVec2)> = [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)]
            .map(|(x, y)| {
                let p = DVec2::new(x, y);
                (p, DVec2::new(p.x * 2.0 - p.y, p.y + 3.0))
            })
            .to_vec();
        let a = Affine::fit(&pairs).unwrap();
        for (p, q) in &pairs {
            assert!(a.apply(*p).distance(*q) < 1e-9);
        }
        // On a line: a plain move.
        let a =
            Affine::fit(&[(DVec2::ZERO, DVec2::ONE), (DVec2::X, DVec2::X + DVec2::ONE)]).unwrap();
        assert!(a.apply(DVec2::Y).distance(DVec2::Y + DVec2::ONE) < 1e-9);
    }

    #[test]
    fn ease_out_starts_fast_and_lands_exactly() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.5) > 0.5);
        assert_eq!(ease(2.0), 1.0);
    }
}
