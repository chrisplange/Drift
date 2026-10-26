//! Wall: click a centreline point by point (or type lengths and angles), and each segment
//! becomes part of a run of wall at the tool bar's thickness, justification and poché. W
//! again (or A / L mid-run) switches between straight and arc segments; a click-drag bends
//! the next segment, as the pen pulls a handle; Space flips the wall to the other face.
//! Starting or ending on another wall joins it (on its end: an L; along it: a T), on another
//! curve stops against it, and back on the start closes a ring. Each segment is one undo
//! step; Backspace takes the last one back.

use drift_core::{LineKind, Path, Wall, WallId};
use drift_geom::walls::Seg;

use super::*;

/// How a run's end meets what is there.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Join {
    /// Along another wall's centreline (a T): its segment.
    Tee(WallId, u16),
    /// Against a curve that isn't a wall.
    Against(EntityId),
    /// In a wall's corner, or next to the run's start: the walls join where they are (an L,
    /// or the run closes), with no record.
    Corner,
}

#[derive(Default)]
pub struct WallTool {
    /// The run's corners so far (once placed, the points they are).
    corners: Vec<Anchor>,
    /// Its segments, for the preview.
    segs: Vec<Seg>,
    /// The wall being drawn (from its first segment on).
    wall: Option<WallId>,
    /// How the run's start meets what is there.
    start: Option<Join>,
    /// An arc's end, waiting for the point it bows through.
    arc_end: Option<(Anchor, Option<Join>)>,
    /// A click-drag from the next corner: the corner and how it meets what is there, and
    /// the way the drag pulls.
    bend: Option<(Anchor, Option<Join>, DVec2)>,
    typed: [Option<f64>; 2],
    lock: Option<Axis>,
    hover: Option<Pointer>,
}

/// The arc from `a` to `b` arriving at `b` going along `t`; None if that is a straight line.
fn bent(a: DVec2, b: DVec2, t: DVec2) -> Option<Seg> {
    let t = t.try_normalize()?;
    let n = t.perp();
    let d = b - a;
    let den = 2.0 * n.dot(d);
    if den.abs() <= 1e-9 * d.length().max(1.0) {
        return None;
    }
    let s = -d.length_squared() / den;
    let center = b + n * s;
    let radius = center.distance(b);
    // Counter-clockwise if the tangent at `b` runs counter-clockwise round the centre.
    let ccw = (b - center).perp().dot(t) > 0.0;
    Some(Seg::Arc {
        center,
        radius,
        a,
        b,
        ccw,
    })
}

impl WallTool {
    fn last(&self) -> Option<DVec2> {
        self.corners.last().map(|c| c.pos)
    }

    /// Where the next corner goes: typed length/angle from the last corner, else the pointer.
    fn target(&self, p: &Pointer) -> DVec2 {
        let Some(last) = self.last() else {
            return typed_point(self.typed, p);
        };
        if !any_typed(self.typed) {
            return p.pos();
        }
        let d = p.pos() - last;
        let len = self.typed[0].unwrap_or(d.length());
        let dir = match (self.typed[1], self.lock) {
            (Some(degrees), _) => exact_direction(degrees),
            (None, Some(axis)) => {
                let u = axis.dir();
                if d.dot(u) < 0.0 { -u } else { u }
            }
            (None, None) if d.length() > 0.0 => d.normalize(),
            (None, None) => DVec2::X,
        };
        last + dir * len
    }

    /// What the pointer is on: a wall's centreline point (an L or n-way join), another wall
    /// along its length (a T), a curve to stop against, or nothing. Generous: anywhere in a
    /// wall counts as on it, and anywhere within half the new wall's thickness of one of its
    /// corners (or, at a free end, within that wall's thickness more) as in that corner, so a
    /// click there makes an L, not a T with a stub: the end goes on the centreline where it
    /// was clicked (not on the corner's point, which would turn the wall) and the walls join
    /// by where they are. The run being drawn counts too (in its start's corner, or next to
    /// it, it closes; along it, a T into itself), except its last segment.
    fn join_at(&self, ed: &mut Editor, p: &Pointer, at: DVec2) -> (Anchor, Option<Join>) {
        let owners = ed.wall_owners();
        if let Some(pt) = p.snapped_point()
            && !owners.generated(pt)
            && let Ok(pos) = ed.doc.point(pt)
        {
            return (Anchor::existing(pos, pt), None);
        }
        let probe = p
            .snapped_point()
            .and_then(|q| ed.doc.point(q).ok())
            .unwrap_or(p.raw);
        let own = self.wall.zip(self.segs.len().checked_sub(1));
        let half = ed.wall_thickness_now() / 2.0 + p.tolerance;
        if let Some((w, seg, on)) = ed.wall_under(probe, p.tolerance, own)
            && let Some(wall) = ed.doc.wall(w).cloned()
            && let Ok((_, points, closed)) = drift_geom::walls::regen::centreline(&ed.doc, &wall)
        {
            let n = points.len();
            let k = usize::from(seg);
            // Its corners at either end of the segment, nearest first.
            let mut corners: Vec<(f64, EntityId, DVec2, bool)> = Vec::new();
            for i in [k, if closed { (k + 1) % n } else { k + 1 }] {
                let Some(&pt) = points.get(i) else {
                    continue;
                };
                let Ok(pos) = ed.doc.point(pt) else {
                    continue;
                };
                let free = !closed && (i == 0 || i + 1 == n);
                let reach = half + if free { wall.thickness } else { 0.0 };
                // (Not the corner the run is at now.)
                let current = self.corners.last().and_then(|c| c.existing) == Some(pt);
                let d = pos.distance(on);
                if d <= reach && !current {
                    corners.push((d, pt, pos, d <= p.tolerance));
                }
            }
            corners.sort_by(|a, b| a.0.total_cmp(&b.0));
            match corners.first() {
                Some(&(_, pt, pos, true)) => return (Anchor::existing(pos, pt), None),
                Some(_) => return (Anchor::at(on), Some(Join::Corner)),
                None => {}
            }
            return (Anchor::at(on), Some(Join::Tee(w, seg)));
        }
        // Next to the run's start, outside its wall: closes it.
        if self.segs.len() >= 2
            && let Some(first) = self.corners.first()
            && first.pos.distance(probe) <= half
        {
            return (Anchor::at(at), Some(Join::Corner));
        }
        if p.snapped_point().is_none()
            && let Some(c) = ed.against_under(p.raw, p.tolerance)
        {
            return (Anchor::at(at), Some(Join::Against(c)));
        }
        (Anchor::at(at), None)
    }

    /// The segment from the last corner to `to`.
    fn seg_to(&self, to: DVec2, through: Option<DVec2>, bend: Option<DVec2>) -> Option<Seg> {
        let a = self.last()?;
        if a.distance(to) <= 1e-9 {
            return None;
        }
        if let Some(t) = bend
            && let Some(s) = bent(a, to, t)
        {
            return Some(s);
        }
        match through {
            Some(m) => Seg::arc_through(a, m, to),
            None => Some(Seg::Line { a, b: to }),
        }
    }

    fn click(&mut self, p: &Pointer, ed: &mut Editor) {
        let at = if self.arc_end.is_some() {
            p.pos()
        } else {
            self.target(p)
        };
        if self.corners.is_empty() {
            let (anchor, join) = self.join_at(ed, p, at);
            self.corners.push(anchor);
            self.start = join;
            return;
        }
        if ed.wall_arc {
            match self.arc_end.take() {
                None => {
                    let (anchor, join) = self.join_at(ed, p, at);
                    self.arc_end = Some((anchor, join));
                }
                Some((end, join)) => {
                    if let Some(seg) = self.seg_to(end.pos, Some(p.pos()), None) {
                        self.place(ed, end, join, seg, None);
                    }
                }
            }
            return;
        }
        let (anchor, join) = if any_typed(self.typed) {
            (Anchor::at(at), None)
        } else {
            self.join_at(ed, p, at)
        };
        if let Some(seg) = self.seg_to(anchor.pos, None, None) {
            let relation = self.lock.map(Axis::relation).or(p.auto_relation());
            self.place(ed, anchor, join, seg, relation);
        }
    }

    /// Add a segment to `to` (one undo step), and finish the run if it joined another wall,
    /// stopped against a curve or closed.
    fn place(
        &mut self,
        ed: &mut Editor,
        to: Anchor,
        join: Option<Join>,
        seg: Seg,
        relation: Option<GuideKind>,
    ) {
        let first = self.corners[0];
        let gap = ed.doc.rules().gap_tolerance;
        let closes = self.corners.len() >= 2
            && join.is_none()
            && (to.existing.is_some() && to.existing == first.existing
                || to.pos.distance(first.pos) <= gap);
        ed.doc.begin();
        let a = match self.corners.last().and_then(|c| c.existing) {
            Some(id) if ed.doc.contains(id) => id,
            _ => ed.doc.add_point(self.corners.last().expect("a corner").pos),
        };
        if let Some(c) = self.corners.last_mut() {
            c.existing = Some(a);
        }
        let b = if closes {
            match first.existing {
                Some(id) if ed.doc.contains(id) => id,
                _ => ed.doc.add_point(first.pos),
            }
        } else {
            match to.existing {
                Some(id) if ed.doc.contains(id) => id,
                _ => ed.doc.add_point(to.pos),
            }
        };
        let curve = match seg {
            Seg::Line { .. } => ed.doc.add_line(a, b),
            Seg::Arc { center, ccw, .. } => {
                let c = ed.doc.add_point(center);
                if ccw {
                    ed.doc.add_arc(c, a, b)
                } else {
                    ed.doc.add_arc(c, b, a)
                }
            }
        };
        let Ok(curve) = curve else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        ed.doc.set_style(curve, LineKind::Construction);
        if matches!(seg, Seg::Line { .. }) {
            auto_relation(ed, relation, curve);
        }
        let end = |j: Option<Join>| drift_core::WallEnd {
            join: true,
            tee: match j {
                Some(Join::Tee(w, s)) => Some((w, s)),
                _ => None,
            },
            against: match j {
                Some(Join::Against(c)) => Some(c),
                _ => None,
            },
        };
        let wall = match self.wall.filter(|w| ed.doc.wall(*w).is_some()) {
            None => {
                let Ok(path) = ed.doc.add_path(Path {
                    curves: vec![curve],
                    closed: false,
                }) else {
                    ed.doc.rollback();
                    ed.sync();
                    return;
                };
                let mut w = Wall::new(path, ed.wall_thickness_now(), ed.doc.current_layer());
                w.thickness_expr = ed.wall_thickness_expr.clone();
                w.justify = ed.wall_justify;
                w.poche = ed.wall_poche_now();
                w.ends[0] = end(self.start);
                match ed.doc.add_wall(w) {
                    Ok(id) => id,
                    Err(e) => {
                        ed.doc.rollback();
                        ed.notice = Some(format!("No wall: {e}"));
                        ed.sync();
                        return;
                    }
                }
            }
            Some(id) => {
                let centre = ed.doc.wall(id).expect("checked").centre;
                let mut path = ed.doc.path(centre).cloned().unwrap_or_default();
                path.curves.push(curve);
                path.closed = closes;
                let _ = ed.doc.set_path(centre, path);
                id
            }
        };
        if join.is_some()
            && let Some(mut w) = ed.doc.wall(wall).cloned()
        {
            w.ends[1] = end(join);
            let _ = ed.doc.set_wall(wall, w);
        }
        if !ed.commit("Wall") {
            return;
        }
        self.wall = Some(wall);
        self.segs.push(seg);
        let pos = ed.doc.point(b).unwrap_or(to.pos);
        self.corners.push(Anchor::existing(pos, b));
        self.lock = None;
        if closes || join.is_some() {
            self.finish();
        }
    }

    fn finish(&mut self) {
        *self = Self {
            hover: self.hover,
            ..Self::default()
        };
    }

    /// Take the last segment back (Backspace).
    fn back(&mut self, ed: &mut Editor) -> bool {
        if self.arc_end.take().is_some() {
            return true;
        }
        if self.corners.len() <= 1 {
            let busy = !self.corners.is_empty();
            self.finish();
            return busy;
        }
        if self.wall.is_some() {
            ed.history.discard_last(1, &mut ed.doc);
            ed.sync();
        }
        self.corners.pop();
        self.segs.pop();
        if self.corners.len() == 1 {
            self.wall = None;
        }
        true
    }

    /// The next segment's faces as they would be made, and the pieces of the walls it joins
    /// that the preview stands in for (see `Editor::wall_preview`).
    fn next_preview(
        &self,
        ed: &Editor,
    ) -> Option<(Vec<Shape>, std::collections::BTreeSet<EntityId>)> {
        if self.corners.is_empty() {
            return None;
        }
        let segs = self.preview_segs(self.hover.as_ref());
        let next = *segs.get(self.segs.len())?;
        let from = self.corners.last().and_then(|c| c.existing);
        let to = match &self.bend {
            Some((to, ..)) => to.existing,
            None => self
                .hover
                .as_ref()
                .and_then(Pointer::snapped_point)
                .filter(|p| !ed.wall_owners().generated(*p)),
        };
        let offsets = ed.wall_justify.offsets(ed.wall_thickness_now());
        Some(ed.wall_preview(next, offsets, from, to))
    }

    /// The faces of the run so far with the next segment to the pointer, for the preview.
    fn preview_segs(&self, p: Option<&Pointer>) -> Vec<Seg> {
        let mut segs = self.segs.clone();
        let next = match (&self.bend, &self.arc_end, p) {
            (Some((to, _, t)), ..) => self.seg_to(to.pos, None, Some(*t)),
            (None, Some((end, _)), Some(p)) => self
                .seg_to(end.pos, Some(p.pos()), None)
                .or_else(|| self.seg_to(end.pos, None, None)),
            (None, None, Some(p)) => self.seg_to(self.target(p), None, None),
            _ => None,
        };
        segs.extend(next);
        segs
    }
}

impl Tool for WallTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Wall
    }

    fn hint(&self) -> String {
        if self.corners.is_empty() {
            "Click where the wall starts (on another wall to join it) · W: arc · Space: other face"
                .into()
        } else if self.arc_end.is_some() {
            "Click a point the wall curves through".into()
        } else {
            "Click the next corner, drag to bend, or type a length · W: arc · Space: other face \
             · Backspace: undo a segment · Enter: finish"
                .into()
        }
    }

    fn is_busy(&self) -> bool {
        !self.corners.is_empty()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => self.hover = Some(p),
            Input::Click(p, _) => {
                self.hover = Some(p);
                self.click(&p, ed);
            }
            Input::DragStart(p, _) => {
                self.hover = Some(p);
                if self.corners.is_empty() || ed.wall_arc {
                    self.click(&p, ed);
                } else {
                    let (anchor, join) = self.join_at(ed, &p, self.target(&p));
                    self.bend = Some((anchor, join, DVec2::ZERO));
                }
            }
            Input::Drag(p) => {
                self.hover = Some(p);
                if let Some((to, _, t)) = &mut self.bend {
                    *t = p.raw - to.pos;
                }
            }
            Input::DragEnd(p, _) => {
                self.hover = Some(p);
                if let Some((to, join, t)) = self.bend.take() {
                    let pull = t.length() > p.tolerance;
                    if let Some(seg) = self.seg_to(to.pos, None, pull.then_some(t)) {
                        self.place(ed, to, join, seg, None);
                    }
                }
            }
            Input::DoubleClick(_) | Input::Finish => self.finish(),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        // Esc keeps what was drawn, as the Line tool does.
        let busy = self.is_busy();
        self.finish();
        busy
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        if let Some(last) = self.last()
            && self.arc_end.is_none()
        {
            line::steer(last, self.lock, p);
        }
    }

    fn snap_from(&self) -> Option<DVec2> {
        self.last()
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.corners.is_empty() {
            XY
        } else {
            &["Length", "Angle"]
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let at = self.target(p);
        if self.typed[1].is_some() {
            self.lock = None;
        }
        let relation = self.typed[1].and_then(relation_for_angle);
        self.typed = [None; 2];
        if self.corners.is_empty() {
            self.corners.push(Anchor::at(at));
            return;
        }
        if let Some(seg) = self.seg_to(at, None, None) {
            self.place(ed, Anchor::at(at), None, seg, relation);
        }
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, ed: &mut Editor) -> bool {
        match key {
            ToolKey::Space => {
                ed.wall_justify = ed.wall_justify.flipped();
                // The run being drawn flips too (in its last step).
                if let Some(w) = self.wall
                    && let Some(mut wall) = ed.doc.wall(w).cloned()
                    && wall.justify != ed.wall_justify
                {
                    wall.justify = ed.wall_justify;
                    ed.doc.begin();
                    let _ = ed.doc.set_wall(w, wall);
                    if ed.commit("Wall") {
                        ed.history.squash_last(2, "Wall");
                    }
                }
                true
            }
            ToolKey::Again => {
                ed.wall_arc = !ed.wall_arc;
                self.arc_end = None;
                true
            }
            ToolKey::ArcNext | ToolKey::LineNext => {
                ed.wall_arc = key == ToolKey::ArcNext;
                self.arc_end = None;
                true
            }
            ToolKey::Backspace => self.back(ed),
            ToolKey::X | ToolKey::Y if !self.corners.is_empty() => {
                let axis = if key == ToolKey::X { Axis::X } else { Axis::Y };
                self.lock = if self.lock == Some(axis) {
                    None
                } else {
                    Some(axis)
                };
                true
            }
            _ => false,
        }
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        Some((self.last()?, self.lock?))
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        if self.corners.is_empty() {
            return Vec::new();
        }
        let segs = self.preview_segs(self.hover.as_ref());
        // The next segment, joined to the run it continues and to what it lands on (the
        // placed segments are walls already), and the walls it joins as they would be.
        let mut out: Vec<SketchItem> = self
            .next_preview(ed)
            .map(|(shapes, _)| shapes.into_iter().map(preview).collect())
            .unwrap_or_default();
        // The centreline, faintly.
        out.extend(segs.iter().map(|s| {
            SketchItem::new(s.shape())
                .with_kind(drift_core::LineKind::Construction)
                .with_emphasis(Emphasis::Preview)
        }));
        out.extend(self.corners.iter().map(|c| preview_vertex(c.pos)));
        if let Some((end, _)) = &self.arc_end {
            out.push(preview_vertex(end.pos));
        }
        out
    }

    fn preview_hides(&self, ed: &Editor) -> std::collections::BTreeSet<EntityId> {
        self.next_preview(ed).map(|(_, h)| h).unwrap_or_default()
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let last = self.last()?;
        let d = self.target(p) - last;
        Some(format!(
            "L {}  A {:.2}°",
            p.units.format_readout(d.length()),
            d.y.atan2(d.x).to_degrees()
        ))
    }
}
