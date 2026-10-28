//! Openings in walls (`docs/walls-design.md` §6): where each cuts its wall, and where its
//! symbol is drawn.
//!
//! **The cut.** An opening is two stations on its wall's segment, `u0` and `u1` along the
//! centreline (along the arc for a curved wall). Each is a node with one end: at `u0` the
//! wall stops (an end leaving back along the segment), at `u1` it starts again (one leaving
//! forward), and each end's edge across the wall is a jamb (`Part::Jamb`, keyed by its
//! `Station::Jamb`). The faces are split there into pieces keyed by the stations as any other
//! station splits them, so pieces elsewhere keep their keys, and the outline goes round the
//! opening with no hole-punching: the poché opens with it.
//!
//! An opening that would reach past the corner a face stops at (a node, a T or another
//! opening) isn't cut, with a notice; the tools keep openings clear of that
//! ([`free_spans`]).
//!
//! **The symbol** is drawn in the opening's own frame ([`Frame`]): the library's convention,
//! origin on the face it swings to (the room side) at its hinge jamb, X along the wall to the
//! other jamb, Y into the wall. [`write_symbol`] writes its curves by key, as walls write
//! their faces.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use drift_core::{
    Appearance, CornerKey, DVec2, Dir, Document, EntityId, EntityKind, Hand, LayerId, LineKind,
    OpeningId, Part, Placement, Shape, Station, WallFace, WallId,
};

use super::{End, Gen, Node, Seg, WallIn};

/// An opening as the generator sees it: its wall's segment and the stretch of the centreline
/// it cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpeningIn {
    pub id: OpeningId,
    pub wall: WallId,
    pub seg: u16,
    pub u0: f64,
    pub u1: f64,
}

/// The openings of a document as the generator sees them, for the walls `walls` (those on
/// walls not among them, or on segments they don't have, are left out).
pub fn opening_inputs(doc: &Document, walls: &[WallIn]) -> Vec<OpeningIn> {
    if !doc.has_openings() {
        return Vec::new();
    }
    let by_id: BTreeMap<WallId, &WallIn> = walls.iter().map(|w| (w.id, w)).collect();
    doc.openings()
        .filter_map(|(id, o)| {
            let w = by_id.get(&o.wall)?;
            let s = w.segs.get(usize::from(o.seg))?;
            let (u0, u1) = o.span_on(s.length());
            Some(OpeningIn {
                id,
                wall: o.wall,
                seg: o.seg,
                u0,
                u1,
            })
        })
        .collect()
}

impl Gen<'_> {
    /// The ends at each opening's jambs, a node each (the hook [`Gen::nodes`] calls).
    pub(super) fn opening_ends(&self, nodes: &mut Vec<Node>, made: &[bool]) {
        if self.openings.is_empty() {
            return;
        }
        let index: BTreeMap<WallId, usize> = self
            .walls
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id, i))
            .collect();
        for o in &self.openings {
            let Some(&w) = index.get(&o.wall) else {
                continue;
            };
            let k = usize::from(o.seg);
            if !made[w] || k >= self.walls[w].segs.len() || !self.near(w, k) {
                continue;
            }
            let s = self.walls[w].segs[k];
            for (u, n, dir) in [(o.u0, 0, Dir::Back), (o.u1, 1, Dir::Fwd)] {
                let end: End = self.end(w, k, u, Station::Jamb(o.id, n), dir);
                nodes.push(Node {
                    at: s.at(u),
                    ends: vec![end],
                });
            }
        }
    }

    /// Leave out the cuts of openings that don't fit where they are: one reaching past the
    /// corner its face stops or starts at (a node, a T, another opening), or of no width. Run
    /// after the nodes are solved and before the faces are pieced.
    pub(super) fn check_openings(&mut self) {
        if self.openings.is_empty() {
            return;
        }
        let mut unfit: BTreeSet<OpeningId> = BTreeSet::new();
        for (&(w, k, _), list) in &self.events {
            let mut jambs = list
                .iter()
                .filter_map(|e| match e.station {
                    Station::Jamb(o, _) => Some(o),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .peekable();
            if jambs.peek().is_none() {
                continue;
            }
            let s = self.walls[w].segs[k];
            let mut sorted = list.clone();
            sorted.sort_by(|a, b| {
                a.order
                    .0
                    .total_cmp(&b.order.0)
                    .then(a.order.1.cmp(&b.order.1))
            });
            let at = |g: &Self, c: CornerKey| g.out.point(c).map(|p| s.param(p));
            for o in jambs {
                let stop = sorted
                    .iter()
                    .position(|e| e.station == Station::Jamb(o, 0) && !e.start);
                let start = sorted
                    .iter()
                    .position(|e| e.station == Station::Jamb(o, 1) && e.start);
                let fits = match (stop, start) {
                    (Some(i), Some(j)) if j == i + 1 && i > 0 && j + 1 < sorted.len() => {
                        let (before, after) = (&sorted[i - 1], &sorted[j + 1]);
                        let params = (
                            at(self, before.corner),
                            at(self, sorted[i].corner),
                            at(self, sorted[j].corner),
                            at(self, after.corner),
                        );
                        before.start
                            && !after.start
                            && match params {
                                (Some(a), Some(b), Some(c), Some(d)) => {
                                    a < b - self.eps && b < c - self.eps && c < d - self.eps
                                }
                                _ => false,
                            }
                    }
                    _ => false,
                };
                if !fits {
                    unfit.insert(o);
                }
            }
        }
        if unfit.is_empty() {
            return;
        }
        let gone = |st: Station| matches!(st, Station::Jamb(o, _) if unfit.contains(&o));
        for list in self.events.values_mut() {
            list.retain(|e| !gone(e.station));
        }
        self.edges.retain(|e| !gone(e.key.from));
        let corners: Vec<CornerKey> = self
            .out
            .points
            .keys()
            .copied()
            .filter(|k| matches!(k, CornerKey::Own(e, _) if gone(e.at)))
            .collect();
        for c in corners {
            self.out.points.remove(&c);
            self.corner_faces.remove(&c);
        }
        for o in &unfit {
            let wall = self
                .openings
                .iter()
                .find(|x| x.id == *o)
                .map_or(WallId(0), |x| x.wall);
            self.out.unfit.insert(*o);
            self.out.notices.push(format!(
                "Opening {o} doesn't fit in wall {wall} there: it isn't cut (move it clear of \
                 the corner)"
            ));
        }
    }
}

/// Where along segment `seg` of wall `w` an opening can go: the stretches (along the
/// centreline) where both faces run unbroken between their corners, from the faces the wall
/// made (`doc`'s generated entities). An opening's own cut counts as free for `but`, so it
/// can slide within the stretch it is in.
pub fn free_spans(doc: &Document, w: WallId, seg: u16, but: Option<OpeningId>) -> Vec<(f64, f64)> {
    let Some(wall) = doc.wall(w) else {
        return Vec::new();
    };
    let Ok((segs, ..)) = super::regen::centreline(doc, wall) else {
        return Vec::new();
    };
    let Some(s) = segs.get(usize::from(seg)).copied() else {
        return Vec::new();
    };
    let param = |id: EntityId| doc.point(id).ok().map(|p| s.param(p));
    let ends = |id: EntityId| -> Option<(f64, f64)> {
        let [a, b] = doc.get(id)?.kind.ends()?;
        let (pa, pb) = (param(a)?, param(b)?);
        Some((pa.min(pb), pa.max(pb)))
    };
    let side = |part: Part| -> Vec<(f64, f64)> {
        let mut v: Vec<(Station, (f64, f64))> = wall
            .made
            .curves
            .iter()
            .filter(|(k, _)| k.seg == seg && k.part == part)
            .filter_map(|(k, id)| Some((k.from, ends(*id)?)))
            .collect();
        v.sort_by(|a, b| a.1.0.total_cmp(&b.1.0));
        // Pieces either side of `but`'s cut are one stretch.
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (from, (a, b)) in v {
            match out.last_mut() {
                Some(last) if but.is_some_and(|o| from == Station::Jamb(o, 1)) => last.1 = b,
                _ => out.push((a, b)),
            }
        }
        out
    };
    let (left, right) = (side(Part::Left), side(Part::Right));
    let mut out = Vec::new();
    // (Within the segment too: an inside corner can lie past its end, where a wall meets it
    // at an angle off its face.)
    let len = s.length();
    for &(a0, a1) in &left {
        for &(b0, b1) in &right {
            let (lo, hi) = (a0.max(b0).max(0.0), a1.min(b1).min(len));
            if hi > lo {
                out.push((lo, hi));
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Where an opening's symbol is drawn: the library's frame. Origin on the face it swings to,
/// at its hinge jamb; `x` along the wall to the other jamb (`width` away, the chord for a
/// curved wall); `y` across into the wall, `thickness` deep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: DVec2,
    pub x: DVec2,
    pub y: DVec2,
    pub width: f64,
    pub thickness: f64,
    /// The same frame as a placement: symbols are placed by the one rule blocks use.
    pub place: Placement,
}

impl Frame {
    /// The frame of an opening from `u0` to `u1` along `seg`, in a wall whose faces lie
    /// `offsets` to the left of it, swinging to `facing` with its hinge on `hand`. None if
    /// the opening has no width.
    pub fn of(
        seg: &Seg,
        offsets: (f64, f64),
        (u0, u1): (f64, f64),
        hand: Hand,
        facing: WallFace,
    ) -> Option<Frame> {
        let (near, far) = match facing {
            WallFace::Left => (offsets.0, offsets.1),
            WallFace::Right => (offsets.1, offsets.0),
        };
        let on = |u: f64| seg.at(u) + seg.tangent(u).perp() * near;
        let (a, b) = (on(u0), on(u1));
        // Looking at the wall from its face, which jamb is on the left: from the right face
        // that is the one at `u0`, from the left face the one at `u1`.
        let left_jamb = match facing {
            WallFace::Right => (a, b),
            WallFace::Left => (b, a),
        };
        let (origin, other) = match hand {
            Hand::Left => left_jamb,
            Hand::Right => (left_jamb.1, left_jamb.0),
        };
        let x = (other - origin).try_normalize()?;
        let mid = seg.tangent((u0 + u1) / 2.0).perp();
        let y = if far < near { -mid } else { mid };
        // (Square to the chord: for a straight wall, `mid` already is.)
        let y = (y - x * y.dot(x)).try_normalize()?;
        let place = Placement::from_axes(origin, x, x.perp_dot(y) < 0.0);
        let (x, y) = place.axes();
        Some(Frame {
            origin,
            x,
            y,
            width: origin.distance(other),
            thickness: (offsets.0 - offsets.1).abs(),
            place,
        })
    }

    /// A point of the frame in the drawing.
    pub fn point(&self, p: DVec2) -> DVec2 {
        self.place.apply(p)
    }

    /// Whether the frame mirrors (turns counter-clockwise curves clockwise).
    pub fn mirrors(&self) -> bool {
        self.place.mirror
    }

    /// A shape of the frame in the drawing (lines and arcs; others are left out). An arc is
    /// made from its placed centre and ends, as the symbol's arc entity is: its ends land
    /// exactly where the placement puts them (exactly on the wall's axes for a quarter turn).
    pub fn shape(&self, s: &Shape) -> Option<Shape> {
        Some(match *s {
            Shape::Line { .. } => self.place.shape(s),
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let at = |t: f64| self.point(center + radius * DVec2::from_angle(t));
                let (s0, s1) = (at(start), at(start + sweep));
                let c = self.point(center);
                // A mirror runs it the other way round.
                if self.mirrors() {
                    drift_core::arc_shape(c, s1, s0)
                } else {
                    drift_core::arc_shape(c, s0, s1)
                }
            }
            _ => return None,
        })
    }
}

/// What writing a symbol did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SymbolReport {
    pub changed: bool,
    pub created: usize,
    pub deleted: usize,
}

/// Write an opening's symbol (`items`, in the drawing already: lines and arcs with their
/// look) as its own entities, keyed by their order, on `layer`: a key whose geometry is
/// unchanged isn't touched, a new key gets new entities, and keys that have gone delete
/// theirs.
pub fn write_symbol(
    doc: &mut Document,
    id: OpeningId,
    items: &[(Shape, Appearance)],
    layer: LayerId,
) -> SymbolReport {
    let mut report = SymbolReport::default();
    let Some(mut o) = doc.opening(id).cloned() else {
        return report;
    };
    let old = o.made.clone();
    let mut made = drift_core::OpeningMade::default();
    let mut place = |doc: &mut Document, key: u32, at: DVec2, report: &mut SymbolReport| {
        let id = match old
            .points
            .get(&key)
            .copied()
            .filter(|p| doc.get(*p).is_some_and(|e| e.kind.is_point()))
        {
            Some(p) => {
                if doc.point(p).ok() != Some(at) {
                    let _ = doc.set_point(p, at);
                    report.changed = true;
                }
                p
            }
            None => {
                report.created += 1;
                report.changed = true;
                doc.add_point(at)
            }
        };
        made.points.insert(key, id);
        id
    };
    let mut curves: BTreeMap<u32, (EntityKind, Appearance)> = BTreeMap::new();
    for (i, (shape, look)) in items.iter().enumerate() {
        let k = u32::try_from(i).unwrap_or(u32::MAX / 4);
        let kind = match *shape {
            Shape::Line { a, b } if a.distance(b) > drift_core::tolerance::SAME => {
                let (pa, pb) = (
                    place(doc, k * 3, a, &mut report),
                    place(doc, k * 3 + 1, b, &mut report),
                );
                EntityKind::Line { a: pa, b: pb }
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } if radius > drift_core::tolerance::SAME
                && sweep.abs() > drift_core::tolerance::NONE =>
            {
                let c = place(doc, k * 3, center, &mut report);
                let s = place(
                    doc,
                    k * 3 + 1,
                    center + radius * DVec2::from_angle(start),
                    &mut report,
                );
                let e = place(
                    doc,
                    k * 3 + 2,
                    center + radius * DVec2::from_angle(start + sweep),
                    &mut report,
                );
                EntityKind::Arc {
                    center: c,
                    start: s,
                    end: e,
                }
            }
            _ => continue,
        };
        curves.insert(k, (kind, *look));
    }
    for (k, (kind, look)) in curves {
        let reuse = old.curves.get(&k).copied().filter(|c| {
            doc.get(*c)
                .is_some_and(|e| std::mem::discriminant(&e.kind) == std::mem::discriminant(&kind))
        });
        let cid = match reuse {
            Some(c) => {
                if doc.get(c).map(|e| e.kind) != Some(kind)
                    && doc.set_entity_kind_exact(c, kind).is_ok()
                {
                    report.changed = true;
                }
                c
            }
            None => {
                let added = match kind {
                    EntityKind::Line { a, b } => doc.add_line(a, b),
                    EntityKind::Arc { center, start, end } => doc.add_arc_exact(center, start, end),
                    _ => continue,
                };
                let Ok(c) = added else {
                    continue;
                };
                report.created += 1;
                report.changed = true;
                c
            }
        };
        if let Some(e) = doc.get(cid).copied() {
            if e.layer != layer {
                let _ = doc.set_entity_layer(cid, layer);
                report.changed = true;
            }
            if e.style != LineKind::Solid {
                doc.set_style(cid, LineKind::Solid);
                report.changed = true;
            }
            if e.appearance != look {
                let _ = doc.set_appearance(cid, look);
                report.changed = true;
            }
        }
        made.curves.insert(k, cid);
    }
    for p in made.points.values() {
        if doc.get(*p).is_some_and(|e| e.layer != layer) {
            let _ = doc.set_entity_layer(*p, layer);
            report.changed = true;
        }
    }
    // What has gone: curves, then the points only they used.
    let kept: HashSet<EntityId> = made.entities().collect();
    let doomed_curves: HashSet<EntityId> = old
        .curves
        .values()
        .copied()
        .filter(|c| !kept.contains(c) && doc.get(*c).is_some())
        .collect();
    let mut doomed: Vec<EntityId> = doomed_curves.iter().copied().collect();
    for p in old.points.values().copied() {
        if !kept.contains(&p)
            && doc.get(p).is_some()
            && doc.users_of(p).all(|u| doomed_curves.contains(&u))
        {
            doomed.push(p);
        }
    }
    // (Points of a curve left out, as of no size: no curve uses them.)
    let used: HashSet<EntityId> = made
        .curves
        .values()
        .filter_map(|c| doc.get(*c))
        .flat_map(|e| e.kind.points())
        .collect();
    let unused: Vec<EntityId> = made
        .points
        .values()
        .copied()
        .filter(|p| !used.contains(p))
        .collect();
    made.points.retain(|_, p| used.contains(p));
    doomed.extend(unused);
    doomed.sort_unstable();
    doomed.dedup();
    if !doomed.is_empty() {
        report.deleted = doomed.len();
        report.changed = true;
        doc.delete(doomed);
    }
    if o.made != made {
        o.made = made;
        let _ = doc.set_opening(id, o);
        report.changed = true;
    }
    report
}
