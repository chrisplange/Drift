//! Dragging geometry: [`Editor::drag`], and the steps a drag frame takes when one solve won't
//! do.

use std::collections::HashSet;
use std::rc::Rc;

use drift_core::{Constraint, DVec2, EntityId, EntityKind};
use drift_solver::{Param, SolveOptions};

use super::{Editor, Geometry};

/// Badly mispredicted steps in a drag frame's solve (see `SolveReport::wandered`) from which
/// it is taken again in smaller steps ([`Editor::drag`]).
const WANDERED: usize = 1;

/// The most solves a drag frame runs ([`Editor::drag`]): enough to go round a fold, or to push
/// a squeezed arc along step after step, not so many that a move that can't be made at all
/// costs a frame a second.
const DRAG_SOLVES: u32 = 200;

/// How many times a step that turns the shape is halved before the curves it turns are taken
/// to be pushed along.
const MAX_SPLITS: u32 = 6;

/// The drag [`Editor::drag`] is in the middle of. Every frame solves from the geometry as
/// the drag found it, moved by the whole way so far, so where the drag is depends on that
/// alone, not on the way the pointer went there.
pub(super) struct DragSession {
    /// The transaction it's in.
    serial: u64,
    ids: Vec<EntityId>,
    dragged: Vec<Param>,
    /// The geometry as the drag found it, and the sizes and lengths it keeps.
    start: Geometry,
    /// How far the drag has gone (the last frame that could go).
    offset: DVec2,
    /// Where the last frame left the points: a call that doesn't find them there (something
    /// else moved them) starts a new drag.
    end: Vec<(EntityId, DVec2)>,
    /// Whether the last frame pushed curves along that couldn't give way any further.
    pushed: bool,
    /// Whether what is dragged needs solving at all (see [`DragFrame::needs_solve`]); the
    /// relations don't change during a drag.
    needs_solve: bool,
    /// Relations on walls' faces as lowered when it started (they don't change either).
    lowering: Option<Rc<drift_solver::Lowering>>,
    /// The walls' joins when it started, kept until it ends but for the ends it moves
    /// itself, so geometry it moves only by solving doesn't flip a join from frame to frame.
    joins: Option<Rc<drift_geom::walls::Joins>>,
}

/// What one call of [`Editor::drag`] works with: the same for each step it takes.
struct DragFrame<'a> {
    ids: &'a [EntityId],
    dragged: &'a [Param],
    dragged_set: HashSet<Param>,
    /// What the drag can move.
    scope: &'a Rc<HashSet<EntityId>>,
    /// Whole curves are moved (not points dragged).
    curves: bool,
    /// Relations, rounds or locked geometry in the scope: without them a move goes exactly
    /// where it is put, with no solve.
    needs_solve: bool,
    lowering: Option<Rc<drift_solver::Lowering>>,
    /// The smooth anchors in the scope: the anchor and its two handles.
    smooth: Vec<(EntityId, EntityId, EntityId)>,
    /// Solves this frame may still run.
    solves_left: u32,
    /// The most its solves have wandered (see `SolveReport::wandered`).
    wandered: usize,
    /// A step pushed curves along.
    pushed: bool,
}

impl Editor {
    /// Move entities by `delta` inside an open transaction, keeping constraints satisfied.
    /// `dragged` are the parameters that should follow the pointer exactly. Returns false (and
    /// leaves the geometry where it was) if the move is impossible.
    ///
    /// A move that would turn an arc inside out or put a tangent curve on the other side of
    /// its line (a solve can meet every relation that way) is taken in smaller steps, which
    /// follow the shape round; if even small steps turn it, the move is refused.
    ///
    /// Moving whole curves carries the lines hanging from them along where it can, at their
    /// length, rather than stretching them; dragging a point reshapes what it ends.
    ///
    /// Successive calls in one transaction, on the same geometry, are one drag. Each solves
    /// from the geometry as the drag found it, moved by the whole way so far, keeping its
    /// sizes and lengths from then: where a drag is depends only on how far it
    /// has gone, not the way there, and out and back puts everything back.
    pub fn drag(&mut self, ids: &[EntityId], delta: DVec2, dragged: &[Param]) -> bool {
        let curves = ids
            .iter()
            .all(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()));
        // Anchors carry their curve handles.
        let ids = &self.with_handles(ids);
        // Only what's connected to the dragged geometry is solved.
        let scope = self.drag_scope(ids);
        let now = self.geometry_of(&scope);
        let serial = self.doc.transaction_serial();
        let session = self.drag_session.take().filter(|s| {
            self.doc.in_transaction()
                && s.serial == serial
                && s.ids == *ids
                && s.dragged == dragged
                && s.end.len() == now.points.len()
                && s.end
                    .iter()
                    .all(|&(id, p)| self.doc.point(id).ok() == Some(p))
        });
        let (start, offset, needs_solve, lowering, joins) = match session {
            Some(s) => (s.start, s.offset, s.needs_solve, s.lowering, s.joins),
            None => {
                let lowering = self.lowering();
                (
                    self.drag_start(&scope, &now, curves, dragged),
                    DVec2::ZERO,
                    drift_solver::needs_solve_with(&self.doc, &scope, lowering.as_deref()),
                    lowering,
                    self.drag_joins(ids),
                )
            }
        };
        self.restore_geometry(&start);
        let mut frame = DragFrame {
            ids,
            dragged,
            dragged_set: dragged.iter().copied().collect(),
            scope: &scope,
            curves,
            needs_solve,
            lowering: lowering.clone(),
            smooth: self.smooth_anchors(&scope),
            solves_left: DRAG_SOLVES,
            wandered: 0,
            pushed: false,
        };
        let to = offset + delta;
        // (Every frame tries one solve first, even where the last one wandered: skipping it
        // would make where the drag is depend on the way there.)
        let ok = self.drag_from_start(&mut frame, to, &start);
        self.drag_solves = DRAG_SOLVES - frame.solves_left;
        let pushed = ok && frame.pushed;
        if !ok {
            // Where the last frame left it.
            self.restore_geometry(&now);
            self.index.update_moved(&self.doc, scope.iter().copied());
        }
        let end = self.geometry_of(&scope).points;
        self.drag_session = Some(DragSession {
            serial,
            ids: ids.clone(),
            dragged: dragged.to_vec(),
            start,
            offset: if ok { to } else { offset },
            end,
            pushed,
            needs_solve,
            lowering,
            joins: joins.clone(),
        });
        // Walls follow their centrelines as they go (keeping their joins), and a hatch whose
        // shape moved as one goes with it.
        if self.doc.has_walls() {
            self.follow_walls_with(joins.as_deref());
        }
        self.carry_hatches();
        ok
    }

    /// The walls' joins as a drag of `ids` finds them, the ends it moves itself left live.
    fn drag_joins(&self, ids: &[EntityId]) -> Option<Rc<drift_geom::walls::Joins>> {
        if !self.doc.has_walls() {
            return None;
        }
        let (inputs, _) = drift_geom::walls::regen::wall_inputs(&self.doc);
        let mut joins =
            drift_geom::walls::infer_joins(&inputs, drift_geom::walls::regen::snap(&self.doc));
        joins.live = self
            .doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .collect();
        Some(Rc::new(joins))
    }

    /// The joins the drag in progress keeps (see `drift_geom::walls::Joins`).
    #[cfg(test)]
    pub(crate) fn drag_kept_joins(&self) -> Option<Rc<drift_geom::walls::Joins>> {
        self.drag_session.as_ref().and_then(|s| s.joins.clone())
    }

    /// Whether the drag in progress pushes curves along that couldn't give way any further
    /// (an arc squeezed as small as it goes, a line that would turn round): they move though
    /// nothing relates them to what is dragged.
    pub fn drag_pushed(&self) -> bool {
        self.drag_session.as_ref().is_some_and(|s| s.pushed)
    }

    /// Move the geometry, as the drag found it (`start`), by `offset`: in steps of an eighth
    /// of its size at most, so a long way goes round the shape as the pointer would have
    /// taken it, not across a fold to whichever other solution one big solve happens to land
    /// on (#103). The steps depend on the offset and the shape alone, so the drag still ends
    /// where its target says, at any size, and moves on smoothly as the target does (each
    /// further step starts where the last one ended). Free geometry, which a solve doesn't
    /// touch, goes in one.
    fn drag_from_start(
        &mut self,
        frame: &mut DragFrame<'_>,
        offset: DVec2,
        start: &Geometry,
    ) -> bool {
        let ok = self.drag_frame(frame, offset, start);
        // (Or couldn't go at all: in steps it may, going round what one solve can't.)
        if ok && frame.wandered < WANDERED {
            return true;
        }
        // (Its size as the farthest point from its middle: the same however it's turned.)
        let middle =
            start.points.iter().map(|&(_, p)| p).sum::<DVec2>() / start.points.len().max(1) as f64;
        let size = start
            .points
            .iter()
            .map(|&(_, p)| p.distance(middle))
            .fold(0.0, f64::max);
        let whole = offset.length();
        // (At most 16 of them: a small part dragged a long way needn't crawl.)
        let step = (size / 4.0).max(whole / 16.0);
        if step <= 0.0 || (ok && whole <= step) {
            return ok;
        }
        self.restore_geometry(start);
        frame.pushed = false;
        let along = offset / whole;
        let mut at = DVec2::ZERO;
        let mut k = 1;
        loop {
            // (The last step ends at the offset itself, not at a rounding of it.)
            let last = step * f64::from(k) >= whole;
            let next = if last {
                offset
            } else {
                along * (step * f64::from(k))
            };
            if !self.drag_towards(frame, at, next, start, 0) {
                return false;
            }
            if last {
                return true;
            }
            at = next;
            k += 1;
        }
    }

    /// One step of [`drag_from_start`](Self::drag_from_start), from `at` to `to` (offsets
    /// from the start), in halves where a solve can't make it whole (up to 4 deep).
    fn drag_towards(
        &mut self,
        frame: &mut DragFrame<'_>,
        at: DVec2,
        to: DVec2,
        start: &Geometry,
        depth: u32,
    ) -> bool {
        if frame.solves_left == 0 {
            return false;
        }
        let mut here = self.geometry_of(frame.scope);
        here.radii = start.radii.clone();
        here.lengths = start.lengths.clone();
        here.lines = start.lines.clone();
        if self.drag_frame(frame, to - at, &here) {
            return true;
        }
        let half = (at + to) / 2.0;
        depth < 4
            && self.drag_towards(frame, at, half, start, depth + 1)
            && self.drag_towards(frame, half, to, start, depth + 1)
    }

    /// The geometry as a drag starting now finds it, with what the drag keeps: the arcs'
    /// radii and, for a move of curves, the lengths of the lines hanging from what moves.
    fn drag_start(
        &self,
        scope: &HashSet<EntityId>,
        now: &Geometry,
        curves: bool,
        dragged: &[Param],
    ) -> Geometry {
        let dragged_set: HashSet<Param> = dragged.iter().copied().collect();
        let dragged_point = |p: EntityId| Param::point(p).iter().any(|q| dragged_set.contains(q));
        let mut before = now.clone();
        if !curves {
            return before;
        }
        let mut lines: Vec<EntityId> = scope
            .iter()
            .copied()
            .filter(|&id| {
                matches!(self.doc.get(id).map(|e| e.kind),
                    Some(EntityKind::Line { a, b }) if !dragged_point(a) && !dragged_point(b))
            })
            .collect();
        lines.sort_unstable();
        // A line hangs from the moved geometry when its other end is free: on no other curve,
        // in no relation. Only such a line is carried along at its length (as it would be in
        // a drawing program); one whose far end holds on to more geometry stretches instead,
        // so a move doesn't shove everything beyond it along (#143).
        let mut uses: std::collections::HashMap<EntityId, usize> = std::collections::HashMap::new();
        for &id in scope {
            if let Some(e) = self.doc.get(id)
                && !e.kind.is_point()
            {
                for p in e.kind.points() {
                    *uses.entry(p).or_default() += 1;
                }
            }
        }
        let related: HashSet<EntityId> = self
            .doc
            .constraints()
            .flat_map(|(_, c)| c.entities().collect::<Vec<_>>())
            .collect();
        let free = |p: EntityId| uses.get(&p) == Some(&1) && !related.contains(&p);
        for &id in &lines {
            if let Some(EntityKind::Line { a, b }) = self.doc.get(id).map(|e| e.kind)
                && (free(a) || free(b))
                && let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b))
            {
                before.lengths.insert(id, pa.distance(pb));
            }
        }
        before.lines = lines;
        before
    }

    /// One call's worth of [`drag`](Self::drag), keeping what `before` says to.
    fn drag_frame(&mut self, frame: &mut DragFrame<'_>, delta: DVec2, before: &Geometry) -> bool {
        // Free geometry (no relations, nothing pinned, no arcs keeping their shape) goes exactly
        // where it is moved, as in a drawing program: no solve to nudge it. Unless the move
        // stretches a line hanging from moved curves, which a solve carries along instead.
        if !frame.needs_solve {
            // (A line with a dragged end is reshaped by it, solve or not.)
            let moved = self.doc.defining_points(frame.ids.iter().copied());
            let carried = |p: EntityId| {
                moved.contains(&p)
                    && !Param::point(p)
                        .iter()
                        .all(|q| frame.dragged_set.contains(q))
            };
            let stretched = before.lengths.keys().any(|&line| {
                matches!(self.doc.get(line).map(|e| e.kind),
                    Some(EntityKind::Line { a, b })
                        if moved.contains(&a) != moved.contains(&b) && (carried(a) || carried(b)))
            });
            if !stretched {
                self.doc.translate(frame.ids.iter().copied(), delta);
                self.index
                    .update_moved(&self.doc, frame.scope.iter().copied());
                return true;
            }
        }
        let mut turned = Vec::new();
        let mut ok = self.drag_in_steps(frame, frame.ids, delta, &[], before, 0, &mut turned);
        if !ok {
            self.restore_geometry(before);
        }
        // A move of curves that would turn an arc inside out (or flip a tangency) even in
        // small steps carries that curve along rather than stopping: an arc squeezed by a
        // moved end is pushed once it can't shrink any further. (Carried, not dragged: a
        // line hanging from the pushed arc still keeps its length, as one hanging from the
        // selection does.)
        let mut ids = frame.ids.to_vec();
        let mut carried_points = Vec::new();
        while !ok && frame.curves && !turned.is_empty() {
            let carried: Vec<EntityId> = std::mem::take(&mut turned)
                .into_iter()
                .filter(|id| !ids.contains(id))
                .collect();
            if carried.is_empty() {
                break;
            }
            for p in self.doc.defining_points(carried.iter().copied()) {
                for q in Param::point(p) {
                    if !frame.dragged_set.contains(&q) && !carried_points.contains(&q) {
                        carried_points.push(q);
                    }
                }
            }
            ids.extend(carried);
            ok = self.drag_in_steps(frame, &ids, delta, &carried_points, before, 0, &mut turned);
            if !ok {
                self.restore_geometry(before);
            }
        }
        // Only the connected part can have moved.
        self.index
            .update_moved(&self.doc, frame.scope.iter().copied());
        frame.pushed |= ok && !carried_points.is_empty();
        ok
    }

    /// One step of [`drag`](Self::drag) from `before` (the scope's geometry now), split in
    /// halves (up to `MAX_SPLITS` deep) where it turns the shape. False, with the geometry
    /// part way, if it can't go (or the frame has run all the solves it may); then `turned`
    /// holds the curves even the smallest step turned.
    #[allow(clippy::too_many_arguments)]
    fn drag_in_steps(
        &mut self,
        frame: &mut DragFrame<'_>,
        ids: &[EntityId],
        delta: DVec2,
        carried: &[Param],
        before: &Geometry,
        depth: u32,
        turned: &mut Vec<EntityId>,
    ) -> bool {
        if frame.solves_left == 0 {
            return false;
        }
        let orientation = drift_solver::Orientation::of(&self.doc, Some(frame.scope));
        // Lines carried along at their length point the same way after the step: keeping a
        // length holds just as well with the line turned round (or let go through zero),
        // which a big step can land on as the nearer fit. A line with a dragged end is
        // reshaped by it, and may turn.
        let dragged_point = |p: EntityId| {
            Param::point(p)
                .iter()
                .any(|q| frame.dragged_set.contains(q))
        };
        let carried_lines: Vec<(EntityId, EntityId, EntityId, DVec2)> = before
            .lines
            .iter()
            .filter_map(|&line| match self.doc.get(line)?.kind {
                EntityKind::Line { a, b } if !dragged_point(a) && !dragged_point(b) => Some((
                    line,
                    a,
                    b,
                    self.doc.point(b).ok()? - self.doc.point(a).ok()?,
                )),
                _ => None,
            })
            .collect();
        // Lines a lowered relation measures a signed offset from (walls' centrelines under
        // dimensioned faces) keep their way round too: turned, which side is the left face
        // would swap, and the relation would hold the other face.
        let lowered_lines: Vec<(EntityId, EntityId, EntityId, DVec2)> = frame
            .lowering
            .iter()
            .flat_map(|l| l.of.values().flatten().chain(&l.extra))
            .flat_map(drift_solver::Lowered::entities)
            .filter_map(|line| match self.doc.get(line)?.kind {
                EntityKind::Line { a, b } => Some((
                    line,
                    a,
                    b,
                    self.doc.point(b).ok()? - self.doc.point(a).ok()?,
                )),
                _ => None,
            })
            .collect();
        self.doc.translate(ids.iter().copied(), delta);
        // Translating keeps the arcs it touches circular by putting their ends back on their
        // circles, which can take a moved point off its target (the end of an arc that isn't
        // moved with it): put those back, and the solve reshapes the arc instead.
        for &(id, p) in &before.points {
            if (frame.dragged_set.contains(&Param::X(id)) || carried.contains(&Param::X(id)))
                && self.doc.point(id).is_ok_and(|now| now != p + delta)
            {
                let _ = self.doc.set_point(id, p + delta);
            }
        }
        self.keep_smooth_anchors(&frame.smooth, ids, delta);
        frame.solves_left -= 1;
        let report = drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                dragged: frame.dragged.to_vec(),
                carried: carried.to_vec(),
                scope: Some(frame.scope.clone()),
                // Rounds keep their size where the move leaves it open (moving a centre on
                // its own would otherwise shrink or swell its arc as a first guess).
                radii: before.radii.clone(),
                // Lines carried along by a move of curves keep their length the same way.
                lengths: before.lengths.clone(),
                lowering: frame.lowering.clone(),
                ..Default::default()
            },
        );
        frame.wandered = frame.wandered.max(report.wandered);
        let conflicts = self.new_conflicts(&report);
        let mut flipped: Vec<EntityId> = carried_lines
            .iter()
            .chain(&lowered_lines)
            .filter(
                |&&(_, a, b, was)| match (self.doc.point(a), self.doc.point(b)) {
                    (Ok(a), Ok(b)) => (b - a).dot(was) <= 0.0,
                    _ => false,
                },
            )
            .map(|&(line, ..)| line)
            .collect();
        if !conflicts && flipped.is_empty() && !orientation.turned(&self.doc) {
            return true;
        }
        if depth >= MAX_SPLITS {
            // Even the smallest step turns these. (Or can't be solved: a curve the move has
            // left behind until nothing is left of it, a tangent line squeezed to a point,
            // turns as it goes through zero and the solve fails on it.)
            *turned = orientation.turned_curves(&self.doc);
            turned.append(&mut flipped);
        }
        self.restore_geometry(before);
        // (Only a turn is worth smaller steps: a conflict is the same at any size.)
        if conflicts
            || depth >= MAX_SPLITS
            || !self.drag_in_steps(frame, ids, delta / 2.0, carried, before, depth + 1, turned)
        {
            return false;
        }
        let mut halfway = self.geometry_of(frame.scope);
        // What to keep is still what it was before the move.
        halfway.radii = before.radii.clone();
        halfway.lengths = before.lengths.clone();
        halfway.lines = before.lines.clone();
        self.drag_in_steps(
            frame,
            ids,
            delta / 2.0,
            carried,
            &halfway,
            depth + 1,
            turned,
        )
    }

    /// The smooth anchors among `scope` (each with its two handles), for
    /// [`keep_smooth_anchors`](Self::keep_smooth_anchors): found once a drag frame.
    fn smooth_anchors(&self, scope: &HashSet<EntityId>) -> Vec<(EntityId, EntityId, EntityId)> {
        self.doc
            .constraints()
            .filter_map(|(id, c)| match *c {
                Constraint::Smooth { point, a, b }
                    if scope.contains(&point) && !self.doc.is_driven(id) =>
                {
                    Some((point, a, b))
                }
                _ => None,
            })
            .collect()
    }

    /// After `ids` moved by `delta`: keep the `smooth` anchors smooth the way a drawing
    /// program does, before any solve. A handle dragged on its own swings the other one round
    /// to stay opposite, at its own length, with the anchor where it is; an anchor moved with
    /// one handle takes the other along. The relation then holds as it is, so a solve has
    /// nothing to nudge (it would otherwise move the anchor and both handles a little each,
    /// and the curves on either side with them).
    fn keep_smooth_anchors(
        &mut self,
        smooth: &[(EntityId, EntityId, EntityId)],
        ids: &[EntityId],
        delta: DVec2,
    ) {
        if smooth.is_empty() {
            return;
        }
        let moved = self.doc.defining_points(ids.iter().copied());
        for &(anchor, a, b) in smooth {
            let at = |id| moved.contains(&id);
            for (dragged, other) in [(a, b), (b, a)] {
                if !at(dragged) || at(other) {
                    continue;
                }
                let (Ok(p), Ok(h), Ok(o)) = (
                    self.doc.point(anchor),
                    self.doc.point(dragged),
                    self.doc.point(other),
                ) else {
                    continue;
                };
                let to = if at(anchor) {
                    o + delta
                } else {
                    match (p - h).try_normalize() {
                        Some(away) => p + away * o.distance(p),
                        None => continue,
                    }
                };
                let _ = self.doc.set_point(other, to);
            }
        }
    }
}
