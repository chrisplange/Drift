//! Dragging geometry: [`Editor::drag`], and the steps a drag frame takes when one solve won't
//! do.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use drift_core::{Constraint, ConstraintId, DVec2, EntityId, EntityKind};
use drift_solver::{Param, SolveOptions};

use super::{Editor, Geometry};

/// Badly mispredicted steps in a drag frame's solve (see `SolveReport::wandered`) from which
/// it is taken again in smaller steps ([`Editor::drag`]).
const WANDERED: usize = 1;

/// The most solves a drag frame runs for one part of what it drags ([`Editor::drag`]):
/// enough to go round a fold, or to push a squeezed arc along step after step, not so many
/// that a move of one part that can't be made at all costs a frame a second. (Per part: a
/// frame dragging several parts that each need many solves runs up to this many for each,
/// each over its own part only.)
const DRAG_SOLVES: u32 = 200;

/// The work a drag frame may spend, for one part of what it drags, on solves that don't
/// finish (see `Editor::unfinished`), counted as each one's scope times its iterations (the
/// way `drag_solved` counts scope): 4 such solves of a part of 16 curves and points, each at
/// the solver's iteration cap of 100 (enough to halve a small part's step a few times; more
/// buys few drags and costs refused frames of mid-sized parts several times what they were).
/// Such a step is taken again in halves, as a step that
/// turns the shape is, but those solves run to the cap, so without this a move that can't be
/// finished at any size would spend the frame's every solve ([`DRAG_SOLVES`]) at that cost
/// and be refused anyway (#281). Work, not solves: one of a part of thousands costs what
/// hundreds of a small one do. Once it is spent, a step whose solve doesn't finish is refused
/// as it is, not halved (that solve still runs: only it can tell), and the steps that do
/// finish go on; so past it a frame runs at most one more unfinished solve for each step
/// [`Editor::drag`] still tries: the whole move, then the first half at each of a stepped
/// step's 5 depths (0 to 4), again for each round of carried curves.
const UNFINISHED_WORK: usize = 4 * 16 * 100;

/// Lines no longer than this (mm) have no way round that a drag keeps (see
/// [`Editor::drag`]'s steps): a flattened curve's error, below anything drawn.
const NO_WAY_ROUND: f64 = drift_core::tolerance::FLATTEN_MM;

/// Solves a drag call may spend on tries at keeping a length (see `Editor::keep_lengths`),
/// all its parts together: its own, apart from today's steps' and the fold try's. A try is one
/// strict solve from the frame's start, split and carried like any frame; the hanging lines
/// #284 found keep their length within 32 (16 is too few for one of them). Most tries fail, the
/// length being one the relations let go, so the bound is what failing tries cost a frame,
/// however many parts it moves: a drag of a thousand parts that each try and fail costs at
/// most 32 solves more, not 32 a part. (So where many parts try, those first in the drag
/// spend the budget, and the rest are as today's steps leave them.)
const LENGTH_TRY_SOLVES: u32 = 32;

/// How far, as a share of its length, a hanging line must be off the length it keeps for a
/// frame to be tried again to keep it (see `Editor::keep_lengths`): a stretch, not what a
/// solve's tolerance leaves. Far from the origin a solve stops with kept lengths off by about
/// a billionth of themselves; tried again from the frame's start, the fold steps can then
/// land on the other side of the fold, so the line jumped out for that frame and back the
/// next (a census of drags a kilometre out). #284's lines were off by four tenths.
const LENGTH_TRY_STRETCH: f64 = 1e-6;

/// How many times a step that fails or turns the shape is halved before it is refused (or,
/// where the smallest step turns, the curves it turns are taken to be pushed along).
const MAX_SPLITS: u32 = 6;

/// How much faster than the pointer a drag that can't land may change the radius of an arc or
/// circle it doesn't move (see [`Editor::breaks_held_shape`]). Where the dragged points can't
/// all reach their targets, the solve lets them fall short only at a heavy cost, and takes up the
/// rest wherever moving other geometry is cheapest: a centre sliding out along its radius grows
/// the round many times faster than the pointer moves (#278: 8 to 52.6 for a drag of 0.08), a
/// free point sliding along its line towards a pivot shortens the line as fast (15 to 3.1 for a
/// drag of 0.06). A drag keeps the radii it can't follow, and doesn't crush the lines it doesn't
/// move, and stops instead ("Stop, as now"). Change slower than this is left as it was; a drag
/// that lands is never judged.
const SHAPE_HOLD: f64 = 10.0;

/// How much faster than the pointer a drag that can't land may shorten a line it doesn't move
/// (see [`Editor::breaks_held_shape`]): a hundredfold, where a radius is held to
/// [`SHAPE_HOLD`]. A line's length changes with the drag far more often than a round's radius,
/// through levers that are the geometry's own (a crank of arm `d` swinging a line `D` long
/// from a fixed point shortens it `D / d` times as fast as the pointer moves), and a drag of
/// those goes both ways up to this advantage. The escape it stops is faster still (#278: the
/// pin's line, 200 times the pointer), and a hundredfold is what `small_drag_is_continuous`
/// allows a small drag to move anything.
const LINE_HOLD: f64 = 100.0;

/// A line a drag doesn't move, shortened to this share of its length as the drag began, has
/// none left: crushed, as an arc is at the same share of its radius (`drift_solver`'s
/// `COLLAPSED_RADIUS`, F1). A drag that can't land stops short of it (see
/// [`Editor::breaks_held_shape`]).
const COLLAPSED_LINE: f64 = 1e-6;

/// Whether a relation holds geometry where it is, so that the part it is in can't be moved
/// whole. Every other relation is between the part's own geometry (or a size: a fixed
/// radius), and moving it all along keeps it.
fn pins(c: &Constraint) -> bool {
    match c {
        Constraint::Fix { .. } | Constraint::FixX { .. } | Constraint::FixY { .. } => true,
        Constraint::Coincident { .. }
        | Constraint::Horizontal { .. }
        | Constraint::Vertical { .. }
        | Constraint::HorizontalPoints { .. }
        | Constraint::VerticalPoints { .. }
        | Constraint::Parallel { .. }
        | Constraint::Perpendicular { .. }
        | Constraint::Equal { .. }
        | Constraint::Tangent { .. }
        | Constraint::Concentric { .. }
        | Constraint::Coradial { .. }
        | Constraint::Collinear { .. }
        | Constraint::PointOnCurve { .. }
        | Constraint::Symmetric { .. }
        | Constraint::Smooth { .. }
        | Constraint::Midpoint { .. }
        | Constraint::MidpointOf { .. }
        | Constraint::EqualDistance { .. }
        | Constraint::Corner { .. }
        | Constraint::FixRadius { .. }
        | Constraint::Distance { .. }
        | Constraint::Radius { .. }
        | Constraint::Diameter { .. }
        | Constraint::HorizontalDistance { .. }
        | Constraint::VerticalDistance { .. }
        | Constraint::PointLineDistance { .. }
        | Constraint::Angle { .. } => false,
    }
}

/// The point a relation holds on curves, and those curves: what a move of curves carries
/// along whole where the point is dragged and the step can't be made otherwise (see
/// `Editor::carry_holding`).
fn held_on(c: &Constraint) -> Option<(EntityId, [EntityId; 2])> {
    match *c {
        Constraint::PointOnCurve { point, curve } => Some((point, [curve, curve])),
        Constraint::Midpoint { point, line } => Some((point, [line, line])),
        Constraint::Corner { point, a, b } => Some((point, [a, b])),
        // Points held to points (merged, midway, mirrored, level, at a distance): no curve
        // to carry. A point held at a distance from a line is measured from it, not on it.
        Constraint::Coincident { .. }
        | Constraint::MidpointOf { .. }
        | Constraint::EqualDistance { .. }
        | Constraint::Symmetric { .. }
        | Constraint::HorizontalPoints { .. }
        | Constraint::VerticalPoints { .. }
        | Constraint::Distance { .. }
        | Constraint::HorizontalDistance { .. }
        | Constraint::VerticalDistance { .. }
        | Constraint::PointLineDistance { .. }
        | Constraint::Smooth { .. }
        | Constraint::Fix { .. }
        | Constraint::FixX { .. }
        | Constraint::FixY { .. } => None,
        // Between curves, or their sizes: no point held.
        Constraint::Horizontal { .. }
        | Constraint::Vertical { .. }
        | Constraint::Parallel { .. }
        | Constraint::Perpendicular { .. }
        | Constraint::Equal { .. }
        | Constraint::Tangent { .. }
        | Constraint::Concentric { .. }
        | Constraint::Coradial { .. }
        | Constraint::Collinear { .. }
        | Constraint::FixRadius { .. }
        | Constraint::Radius { .. }
        | Constraint::Diameter { .. }
        | Constraint::Angle { .. } => None,
    }
}

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
    /// Where the last frame left the scope's points and circles: a call that doesn't find the
    /// points there (something else moved them) starts a new drag. (Not in id order: built
    /// from what the frame wrote, part by part.)
    end: Geometry,
    /// The scope the last frame moved.
    scope: Rc<HashSet<EntityId>>,
    /// Which document, and its [`geometry`](drift_core::Document::geometry) as the last frame
    /// took `end`: unchanged since, with the same scope, nothing in the drawing has moved or
    /// changed, so the points are where `end` says without looking (see
    /// [`Editor::drag`]). None when the frame's own follow-ups (walls, offsets) changed
    /// geometry after `end` was taken: the next frame then looks.
    lineage: u64,
    geometry: Option<u64>,
    /// The drag began at the start of its transaction, the drawing as the transaction found
    /// it, and has been this one drag since, nothing else changing the drawing between its
    /// frames (see [`Editor::land_whole_move`]).
    fresh: bool,
    /// The document's revision as the last frame left it (its walls, offsets and hatches
    /// followed): anything else that changes the drawing before the next frame or the
    /// release (an edit of geometry the drag doesn't move, a fill, a dimension's label, a
    /// copy or a text the move carries) moves it, and the drag is fresh no longer.
    revision: u64,
    /// A frame's walls or offsets followed it (changed the drawing).
    followed: bool,
    /// The drawing's [`make_up`](drift_core::Document::make_up) as the drag began.
    make_up: u64,
    /// Whether the last frame pushed curves along that couldn't give way any further.
    pushed: bool,
    /// The parts of the scope, each moved on its own (the relations don't change during a
    /// drag, nor what joins what).
    parts: Rc<Vec<DragPart>>,
    /// What the parts that go exactly where they are moved move, together.
    whole: Rc<WholeMove>,
    /// Relations on walls' faces as lowered when it started (they don't change either).
    lowering: Option<Rc<drift_solver::Lowering>>,
    /// The walls' joins when it started, kept until it ends but for the ends it moves
    /// itself, so geometry it moves only by solving doesn't flip a join from frame to frame.
    joins: Option<Rc<drift_geom::walls::Joins>>,
}

/// One part of a drag's scope ([`drift_solver::connected_parts_with`]): nothing in it is
/// joined to anything in another part, so each goes through [`Editor::drag`]'s steps on its
/// own, as if it alone were dragged. A part that can't go the whole way stops the drag, as
/// one connected drag that couldn't would; one that has to go in smaller steps (round a
/// fold, past an arc squeezed by the move) takes them alone, not the whole drawing with it
/// (#253).
#[derive(Clone)]
struct DragPart {
    scope: Rc<HashSet<EntityId>>,
    /// The dragged ids in it.
    ids: Vec<EntityId>,
    /// The dragged parameters in it.
    dragged: Vec<Param>,
    dragged_set: HashSet<Param>,
    /// Its geometry as the drag found it, and what the drag keeps of it.
    start: Geometry,
    /// Whether it needs solving at all (see [`DragFrame::needs_solve`]).
    needs_solve: bool,
    /// Nothing in it to solve and no line hanging from it to carry along: it goes exactly
    /// where it is moved, with the others like it, in one move.
    moves_whole: bool,
    /// Its smooth anchors (see [`DragFrame::smooth`]).
    smooth: Vec<(EntityId, EntityId, EntityId)>,
    /// Its relations as the solver reads them, its stored constraints, and the lines its
    /// lowered relations measure from: found once, not looked for among the whole
    /// drawing's at each of its solves (see [`DragFrame::relations`]).
    relations: Rc<drift_solver::Relations>,
    constraints: Vec<(ConstraintId, Constraint)>,
    lowered_lines: Vec<EntityId>,
    /// Where its tangencies touched as the drag began: every step lowers them so.
    touching: Rc<drift_solver::TouchingEnds>,
    /// For a move of curves in a part nothing pins, the curves not moved that relations hold
    /// its dragged points on (see [`DragFrame::holding`]).
    holding: Vec<EntityId>,
    /// A move of curves in a part nothing pins (see [`DragFrame::unpinned`]).
    unpinned: bool,
    /// Its dragged points that a drag can move, where the drag found them, and which of
    /// their coordinates it can move (x, y): a fixed coordinate never reaches a target, and
    /// says nothing of whether the drag landed (see [`Editor::breaks_held_shape`]).
    free_dragged: Vec<(EntityId, DVec2, [bool; 2])>,
    /// Its arcs and circles that the drag doesn't move (none of their points is dragged, or
    /// coincident with a dragged point, where the drag can move it), and their radii as the
    /// drag found them (see [`Editor::breaks_held_shape`]).
    held_radii: Vec<(EntityId, f64)>,
    /// Its lines that the drag doesn't move (neither end), and where each one's ends were as
    /// the drag found them (see [`Editor::breaks_held_shape`]).
    held_lines: Vec<(EntityId, DVec2, DVec2)>,
}

/// The parts of a drag that go exactly where they are moved ([`DragPart::moves_whole`]), as
/// one move: the points it moves, where the drag found them, and all they take with them.
#[derive(Default)]
struct WholeMove {
    points: Vec<(EntityId, DVec2)>,
    moved: std::collections::BTreeSet<EntityId>,
    /// The parts' entities (the index looks at these).
    scope: Vec<EntityId>,
    /// The parts' other points, where the drag found them (they stay there).
    kept: Vec<(EntityId, DVec2)>,
    /// The parts' circles and their radii (a move keeps them).
    circles: Vec<(EntityId, f64)>,
    /// The points of the parts' arcs and ellipses: `normalize_arcs_using` may set them a
    /// little off where they were moved to, so a frame reads them back.
    rounds: HashSet<EntityId>,
}

/// What one call of [`Editor::drag`] works with for one part: the same for each step it
/// takes.
struct DragFrame<'a> {
    ids: &'a [EntityId],
    dragged: &'a [Param],
    dragged_set: &'a HashSet<Param>,
    /// What the drag can move.
    scope: &'a Rc<HashSet<EntityId>>,
    /// Whole curves are moved (not points dragged).
    curves: bool,
    /// Relations, rounds or locked geometry in the scope: without them a move goes exactly
    /// where it is put, with no solve.
    needs_solve: bool,
    lowering: Option<Rc<drift_solver::Lowering>>,
    /// The smooth anchors in the scope: the anchor and its two handles.
    smooth: &'a [(EntityId, EntityId, EntityId)],
    /// The relations of the scope as the solver reads them (lowered), the stored constraints
    /// on it (for its orientation), and the lines its lowered relations measure from: those
    /// of the drawing that relate something in the scope, so that a drag of many small parts
    /// costs what the parts do, not the drawing's every relation per solve (#253).
    relations: &'a Rc<drift_solver::Relations>,
    constraints: &'a [(ConstraintId, Constraint)],
    lowered_lines: &'a [EntityId],
    /// Where the scope's tangencies touched as the drag began ([`DragPart::touching`]).
    touching: &'a Rc<drift_solver::TouchingEnds>,
    /// Curves not moved that relations hold dragged points on, where nothing pins the part:
    /// carried along whole when a step can't be made or doesn't land otherwise (see
    /// `Editor::carry_holding`). Empty for a point dragged, or a part something pins.
    holding: &'a [EntityId],
    /// The circles' radii as the drag began, for judging whether a step crushed one (as
    /// `Geometry::radii` is for arcs; `Geometry::circles` follows each step, to restore it).
    start_circles: &'a [(EntityId, f64)],
    /// A move of curves in a part nothing pins: it can always land, every kept radius and
    /// length met (moving the whole part along does it), so a step that can't be made, or
    /// doesn't land, is the solve failing. Such a step is tried once more with the solver's
    /// fold steps (see `Editor::drag_frame`).
    unpinned: bool,
    /// The steps being solved with the fold steps ([`drift_solver::SolveOptions::fold`]).
    fold: bool,
    /// Solves this frame may still run.
    solves_left: u32,
    /// The work it may still spend on solves that don't finish ([`UNFINISHED_WORK`]).
    unfinished_left: usize,
    /// The same two for the tries with the fold steps, all of a frame's together (they run
    /// in place of the two above while they last). A try begins only while a solve that
    /// runs to the solver's cap still fits in what is left of the work, so a part of more
    /// than [`UNFINISHED_WORK`] over that cap (64 curves and points) is never tried with
    /// them, and a frame that can't be made costs at most that much more than it did: a
    /// frame of a part runs at most [`DRAG_SOLVES`] solves of either kind, so at most twice
    /// that in all.
    fold_solves_left: u32,
    fold_unfinished_left: usize,
    /// The same two for the tries at keeping a length ([`LENGTH_TRY_SOLVES`], [`UNFINISHED_WORK`]),
    /// what the drag call's parts before this one left of them.
    length_solves_left: u32,
    length_unfinished_left: usize,
    /// The most its solves have wandered (see `SolveReport::wandered`).
    wandered: usize,
    /// A step pushed curves along.
    pushed: bool,
    /// The length bar its last solve was held to (`SolveReport::bar`): what a dragged point
    /// is judged landed against (`drift_solver::landed_within`).
    bar: f64,
    /// The bar the frame's first solve was held to (zero before one): the coarsest any later
    /// solve of the frame is held to (`SolveOptions::bar_cap`), whatever a try has flung.
    first_bar: f64,
}

impl Editor {
    /// Move entities by `delta` inside an open transaction, keeping constraints satisfied.
    /// `dragged` are the parameters that should follow the pointer exactly. Returns false (and
    /// leaves the geometry where it was) if the move is impossible.
    ///
    /// A move that would turn an arc inside out or put a tangent curve on the other side of
    /// its line (a solve can meet every relation that way) is taken in smaller steps, which
    /// follow the shape round; if even small steps turn it, the move is refused. So is a step
    /// whose solve doesn't finish: it is taken in smaller steps, and refused if even those
    /// don't (never kept part way, with an arc off its circle).
    ///
    /// Moving whole curves carries the lines hanging from them along where it can, at their
    /// length, rather than stretching them; dragging a point reshapes what it ends.
    /// Where nothing pins the part moved (no fixed point, nothing locked, generated or out of
    /// the group being edited), a move of curves that can't be made, or that leaves what it
    /// drags off where it was put, carries along whole the curves its dragged points are held
    /// on (on a curve, at a line's middle, at a corner), and keeps that if it lands. One that
    /// still can't, in a part of up to 64 curves and points, is tried once more with the
    /// solver's fold steps ([`drift_solver::SolveOptions::fold`]), kept only if it lands with
    /// every kept radius and length met: a drag the kept sizes can't follow still stops.
    ///
    /// Successive calls in one transaction, on the same geometry, are one drag. Each solves
    /// from the geometry as the drag found it, moved by the whole way so far, keeping its
    /// sizes and lengths from then: where a drag is depends only on how far it
    /// has gone, not the way there, and out and back puts everything back.
    ///
    /// Geometry joined to nothing else that is dragged goes on its own: each part of what is
    /// dragged (with what it's joined to) moves as if it alone were dragged.
    pub fn drag(&mut self, ids: &[EntityId], delta: DVec2, dragged: &[Param]) -> bool {
        // (An offset's pieces aren't dragged: they follow their source.)
        let ids = &self.without_offset_pieces(ids);
        if ids.is_empty() {
            return true;
        }
        let curves = ids
            .iter()
            .all(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()));
        // Anchors carry their curve handles.
        let ids = &self.with_handles(ids);
        // Only what's connected to the dragged geometry is solved, part by part.
        let (scope, scope_parts) = self.drag_scope_and_parts(ids);
        let serial = self.doc.transaction_serial();
        let session = self.drag_session.take().filter(|s| {
            self.doc.in_transaction() && s.serial == serial && s.ids == *ids && s.dragged == dragged
        });
        // The same drag goes on when its points are where the last frame left them. Nothing
        // in the drawing changed since that frame took `end` (its geometry counter moves with
        // every change to any entity), with the same scope: they are, without looking.
        // Otherwise they are looked at, one by one.
        let unchanged = session.as_ref().is_some_and(|s| {
            !self.drag_always_look
                && s.lineage == self.doc.lineage()
                && s.geometry == Some(self.doc.geometry())
                && (Rc::ptr_eq(&s.scope, &scope) || *s.scope == *scope)
        });
        let (session, now) = if unchanged {
            let mut s = session.expect("a drag going on");
            let now = std::mem::take(&mut s.end);
            #[cfg(debug_assertions)]
            {
                let looked = self.geometry_of(&scope);
                assert!(
                    now.points.len() == looked.points.len()
                        && now
                            .points
                            .iter()
                            .all(|&(id, p)| self.doc.point(id).ok() == Some(p)),
                    "a drag's points, not looked at, aren't where its last frame left them"
                );
            }
            (Some(s), now)
        } else {
            let now = self.geometry_of(&scope);
            let session = session.filter(|s| {
                s.end.points.len() == now.points.len()
                    && s.end
                        .points
                        .iter()
                        .all(|&(id, p)| self.doc.point(id).ok() == Some(p))
            });
            (session, now)
        };
        // (Changed since the last frame by anything but this drag: not fresh any more. A
        // release then rolls back and moves again, as it always did, so what changed between
        // frames goes as it always went.)
        let carried = session.as_ref().map(|s| {
            (
                s.fresh && s.revision == self.doc.revision(),
                s.followed,
                s.make_up,
            )
        });
        // (A drag starting now starts with its transaction when nothing changed since it began.)
        let at_begin = self.doc.revision() == self.doc.revision_at_begin();
        let (start, offset, parts, whole, lowering, joins) = match session {
            Some(s) => (s.start, s.offset, s.parts, s.whole, s.lowering, s.joins),
            None => {
                let lowering = self.lowering();
                let start = self.drag_start(&scope, &now, curves, dragged);
                let parts = self.drag_parts(
                    &scope,
                    &scope_parts,
                    ids,
                    dragged,
                    &start,
                    lowering.as_deref(),
                    curves,
                );
                let whole = self.whole_move(&parts);
                (
                    start,
                    DVec2::ZERO,
                    Rc::new(parts),
                    Rc::new(whole),
                    lowering,
                    self.drag_joins(ids),
                )
            }
        };
        let to = offset + delta;
        let mut ok = true;
        let mut pushed = false;
        self.drag_solves = 0;
        self.drag_unfinished = 0;
        self.drag_folds = 0;
        self.drag_length_tries = (0, 0, 0);
        self.drag_solved = 0;
        // What goes exactly where it is moved goes in one move, from where the drag found
        // it (as `Document::translate` would move it: arcs' ends kept on their circles).
        if !whole.points.is_empty() {
            for &(id, p) in &whole.points {
                let at = p + to;
                if self.doc.point(id).ok() != Some(at) {
                    let _ = self.doc.set_point(id, at);
                }
            }
            if to != DVec2::ZERO {
                self.doc.normalize_arcs_using(&whole.moved);
            }
            self.index
                .update_moved(&self.doc, whole.scope.iter().copied());
        }
        for part in parts.iter().filter(|p| !p.moves_whole) {
            self.restore_geometry(&part.start);
        }
        // (The tries at keeping a length share one budget across the parts.)
        let mut length_left = (LENGTH_TRY_SOLVES, UNFINISHED_WORK);
        for part in parts.iter().filter(|p| !p.moves_whole) {
            let mut frame = DragFrame {
                start_circles: &part.start.circles,
                ids: &part.ids,
                dragged: &part.dragged,
                dragged_set: &part.dragged_set,
                scope: &part.scope,
                curves,
                needs_solve: part.needs_solve,
                lowering: lowering.clone(),
                smooth: &part.smooth,
                relations: &part.relations,
                constraints: &part.constraints,
                lowered_lines: &part.lowered_lines,
                touching: &part.touching,
                holding: &part.holding,
                unpinned: part.unpinned,
                fold: false,
                solves_left: DRAG_SOLVES,
                unfinished_left: UNFINISHED_WORK,
                fold_solves_left: DRAG_SOLVES,
                fold_unfinished_left: UNFINISHED_WORK,
                length_solves_left: length_left.0,
                length_unfinished_left: length_left.1,
                wandered: 0,
                pushed: false,
                bar: 0.0,
                first_bar: 0.0,
            };
            // (Every frame tries one solve first, even where the last one wandered: skipping
            // it would make where the drag is depend on the way there.)
            ok = self.drag_from_start(&mut frame, to, &part.start)
                && !self.breaks_held_shape(part, to, frame.bar);
            self.drag_solves += 2 * DRAG_SOLVES + length_left.0
                - frame.solves_left
                - frame.fold_solves_left
                - frame.length_solves_left;
            length_left = (frame.length_solves_left, frame.length_unfinished_left);
            pushed |= frame.pushed;
            if !ok {
                break;
            }
        }
        self.drag_length_tries.2 = LENGTH_TRY_SOLVES - length_left.0;
        let pushed = ok && pushed;
        if !ok {
            // Where the last frame left it.
            self.restore_geometry(&now);
            self.index.update_moved(&self.doc, scope.iter().copied());
        }
        self.index.rebuild_if_many_moved(&self.doc);
        // Where this frame left the scope, from what it wrote: the parts that went whole at
        // their start plus the offset (their rounds read back), the others as they are.
        let end = if ok {
            self.drag_end(&parts, &whole, to)
        } else {
            now
        };
        let geometry = self.doc.geometry();
        let (fresh, followed, make_up) =
            carried.unwrap_or_else(|| (at_begin, false, self.doc.make_up()));
        // Walls follow their centrelines as they go (keeping their joins), and a hatch whose
        // shape moved as one goes with it.
        let mut follows_changed = false;
        if self.doc.has_walls() {
            let before = self.doc.revision();
            self.follow_walls_with(joins.as_deref());
            follows_changed |= self.doc.revision() != before;
            // (The faces they made again are out of the scope: the index is told of them too,
            // or it would look over the whole drawing for what changed. Only their points were
            // touched, so the index adds the faces on them.)
            if self.doc.revision() != before {
                self.index.update_moved_with_curves(
                    &self.doc,
                    self.doc.entities_touched_in_transaction(),
                );
            }
        }
        if self.doc.has_offsets() {
            let before = self.doc.revision();
            self.follow_offsets();
            follows_changed |= self.doc.revision() != before;
            if self.doc.revision() != before {
                self.index.update_moved_with_curves(
                    &self.doc,
                    self.doc.entities_touched_in_transaction(),
                );
            }
        }
        // (What the follow-ups wrote after `end` was taken isn't in it: if they changed any
        // entity, the next frame looks.)
        let unmoved_since = self.doc.geometry() == geometry;
        self.drag_session = Some(DragSession {
            serial,
            ids: ids.clone(),
            dragged: dragged.to_vec(),
            start,
            offset: if ok { to } else { offset },
            end,
            scope,
            lineage: self.doc.lineage(),
            geometry: unmoved_since.then_some(geometry),
            fresh,
            revision: 0,
            followed: followed || follows_changed,
            make_up,
            pushed,
            parts,
            whole,
            lowering,
            joins: joins.clone(),
        });
        self.carry_hatches();
        let revision = self.doc.revision();
        if let Some(s) = &mut self.drag_session {
            s.revision = revision;
        }
        ok
    }

    /// Land the move in progress at `offset` from where it began, keeping it open, as
    /// [`drag`](Self::drag) from the drawing as the transaction found it would, in one call
    /// of `offset`: what the modal move's confirm ([`crate::tools`]' `confirm_grab`) does
    /// when this returns true, instead of rolling the move back and dragging it again from
    /// the start. (A mouse drag's release already keeps its move as it is.)
    ///
    /// Only when that comes out the same, bit for bit: the drag of `ids` going on is the
    /// one that began with this transaction ([`DragSession::fresh`]) and nothing has changed
    /// since its last frame (as [`drag`](Self::drag) tells without looking); every part of it
    /// goes whole (nothing solved, refused or stepped, on any frame); no frame's walls or
    /// offsets followed it; and the drawing's make-up is as it began. Each point it moves is
    /// then set to where the drag found it plus `offset` (as that one call puts it: not the
    /// frames' offsets added up, which can differ in the last bit), its rounds normalized,
    /// the index told, and walls, offsets and hatches followed, as that call does. False,
    /// with nothing changed, otherwise.
    pub fn land_whole_move(&mut self, ids: &[EntityId], offset: DVec2, dragged: &[Param]) -> bool {
        if self.land_whole_off {
            return false;
        }
        if offset == DVec2::ZERO {
            return false;
        }
        let ids = &self.without_offset_pieces(ids);
        if ids.is_empty() {
            return false;
        }
        let ids = &self.with_handles(ids);
        let Some(s) = &self.drag_session else {
            return false;
        };
        let (scope, _) = self.drag_scope_and_parts(ids);
        let lands = self.doc.in_transaction()
            && s.serial == self.doc.transaction_serial()
            && s.ids == *ids
            && s.dragged == dragged
            && s.lineage == self.doc.lineage()
            && s.geometry == Some(self.doc.geometry())
            && (Rc::ptr_eq(&s.scope, &scope) || *s.scope == *scope)
            && s.fresh
            && s.revision == self.doc.revision()
            && !s.followed
            && s.make_up == self.doc.make_up()
            && !s.whole.points.is_empty()
            && s.parts.iter().all(|p| p.moves_whole);
        if !lands {
            return false;
        }
        let s = self.drag_session.take().expect("a drag going on");
        self.drag_solves = 0;
        self.drag_unfinished = 0;
        self.drag_folds = 0;
        self.drag_length_tries = (0, 0, 0);
        self.drag_solved = 0;
        for &(id, p) in &s.whole.points {
            let at = p + offset;
            if self.doc.point(id).ok() != Some(at) {
                let _ = self.doc.set_point(id, at);
            }
        }
        self.doc.normalize_arcs_using(&s.whole.moved);
        self.index
            .update_moved(&self.doc, s.whole.scope.iter().copied());
        self.index.rebuild_if_many_moved(&self.doc);
        if self.doc.has_walls() {
            let before = self.doc.revision();
            self.follow_walls_with(s.joins.as_deref());
            if self.doc.revision() != before {
                self.index.update_moved_with_curves(
                    &self.doc,
                    self.doc.entities_touched_in_transaction(),
                );
            }
        }
        if self.doc.has_offsets() {
            let before = self.doc.revision();
            self.follow_offsets();
            if self.doc.revision() != before {
                self.index.update_moved_with_curves(
                    &self.doc,
                    self.doc.entities_touched_in_transaction(),
                );
            }
        }
        self.carry_hatches();
        self.landed_whole += 1;
        true
    }

    /// Where a frame that went left the scope (see [`DragSession::end`]): the parts that go
    /// whole at their start moved by `to` (the points of their rounds, which normalizing may
    /// set a little off that, read back), their other points where they were, and the parts
    /// that solve as they are now.
    fn drag_end(&self, parts: &[DragPart], whole: &WholeMove, to: DVec2) -> Geometry {
        let mut end = Geometry::default();
        end.points.reserve(whole.points.len() + whole.kept.len());
        for &(id, p) in whole.points.iter().chain(&whole.kept) {
            let at = if whole.rounds.contains(&id) {
                self.doc.point(id).unwrap_or(p)
            } else if whole.moved.contains(&id) {
                p + to
            } else {
                p
            };
            end.points.push((id, at));
        }
        end.circles.extend_from_slice(&whole.circles);
        for part in parts.iter().filter(|p| !p.moves_whole) {
            let g = self.geometry_of(&part.scope);
            end.points.extend(g.points);
            end.circles.extend(g.circles);
        }
        end
    }

    /// The parts of a drag's scope as it starts (see [`DragPart`]): the dragged ids and
    /// parameters, the starting geometry and smooth anchors of each, and whether it has
    /// anything to solve.
    #[allow(clippy::too_many_arguments)]
    fn drag_parts(
        &self,
        scope: &HashSet<EntityId>,
        parts: &[Rc<HashSet<EntityId>>],
        ids: &[EntityId],
        dragged: &[Param],
        start: &Geometry,
        lowering: Option<&drift_solver::Lowering>,
        curves: bool,
    ) -> Vec<DragPart> {
        let part_of: HashMap<EntityId, usize> = parts
            .iter()
            .enumerate()
            .flat_map(|(k, p)| p.iter().map(move |&id| (id, k)))
            .collect();
        // (A round whose every point is dragged moves whole, keeping its shape: nothing to
        // solve for it. Moving whole curves drags all their points.)
        let all_dragged: HashSet<Param> = dragged.iter().copied().collect();
        let reasons = drift_solver::solve_reasons_with(&self.doc, scope, lowering, |id| {
            self.doc.get(id).is_some_and(|e| {
                e.kind
                    .points()
                    .all(|p| Param::point(p).iter().all(|q| all_dragged.contains(q)))
            })
        });
        let mut out: Vec<DragPart> = parts
            .iter()
            .map(|p| DragPart {
                scope: p.clone(),
                ids: Vec::new(),
                dragged: Vec::new(),
                dragged_set: HashSet::new(),
                start: Geometry::default(),
                needs_solve: false,
                moves_whole: false,
                smooth: Vec::new(),
                relations: Rc::new(Vec::new()),
                constraints: Vec::new(),
                lowered_lines: Vec::new(),
                touching: Rc::default(),
                holding: Vec::new(),
                unpinned: false,
                free_dragged: Vec::new(),
                held_radii: Vec::new(),
                held_lines: Vec::new(),
            })
            .collect();
        for id in &reasons {
            if let Some(&k) = part_of.get(id) {
                out[k].needs_solve = true;
            }
        }
        let at = |id: &EntityId| part_of.get(id).copied();
        for id in ids {
            if let Some(k) = at(id) {
                out[k].ids.push(*id);
            }
        }
        for &q in dragged {
            let (Param::X(id)
            | Param::Y(id)
            | Param::Radius(id)
            | Param::MidX(id)
            | Param::MidY(id)) = q;
            if let Some(k) = at(&id) {
                out[k].dragged.push(q);
                out[k].dragged_set.insert(q);
            }
        }
        for &(id, p) in &start.points {
            if let Some(k) = at(&id) {
                out[k].start.points.push((id, p));
            }
        }
        for &(id, r) in &start.circles {
            if let Some(k) = at(&id) {
                out[k].start.circles.push((id, r));
            }
        }
        for (&id, &r) in &start.radii {
            if let Some(k) = at(&id) {
                out[k].start.radii.insert(id, r);
            }
        }
        for (&id, &l) in &start.lengths {
            if let Some(k) = at(&id) {
                out[k].start.lengths.insert(id, l);
            }
        }
        for &id in &start.lines {
            if let Some(k) = at(&id) {
                out[k].start.lines.push(id);
            }
        }
        // Each relation, constraint and lowered line to its part (in the drawing's order, as
        // the solver would find them).
        let mut relations: Vec<drift_solver::Relations> = vec![Vec::new(); out.len()];
        for (id, l) in drift_solver::relations(&self.doc, lowering) {
            if let Some(k) = l.entities().iter().find_map(at) {
                relations[k].push((id, l));
            }
        }
        for (part, r) in out.iter_mut().zip(relations) {
            // (The drawing is as the drag found it: where the relations hold.)
            part.touching = Rc::new(drift_solver::touching_ends(&self.doc, &r));
            part.relations = Rc::new(r);
        }
        for (id, c) in self.doc.constraints() {
            let mut touched: Vec<usize> = c.entities().filter_map(|e| at(&e)).collect();
            touched.sort_unstable();
            touched.dedup();
            for k in touched {
                out[k].constraints.push((id, *c));
            }
        }
        for line in lowering
            .iter()
            .flat_map(|l| l.of.values().flatten().chain(&l.extra))
            .flat_map(drift_solver::Lowered::entities)
        {
            if let Some(k) = at(&line) {
                out[k].lowered_lines.push(line);
            }
        }
        for anchor in self.smooth_anchors(scope) {
            if let Some(k) = at(&anchor.0) {
                out[k].smooth.push(anchor);
            }
        }
        for part in &mut out {
            part.moves_whole = !part.needs_solve
                && !self.stretches_hanging_line(&part.ids, &part.dragged_set, &part.start);
        }
        // (The locked geometry and what walls, openings and offsets made: each a walk over the
        // whole drawing, so found once for the drag, not once per part that solves.)
        let solving = out.iter().any(|p| p.needs_solve);
        let locked = if solving {
            self.doc.locked_entities()
        } else {
            Default::default()
        };
        let made: HashSet<EntityId> = if solving && (self.doc.has_walls() || self.doc.has_offsets())
        {
            self.doc.wall_owners().of.into_keys().collect()
        } else {
            HashSet::new()
        };
        for part in out.iter_mut().filter(|p| p.needs_solve) {
            self.find_held_shapes(part, &locked, &made);
        }
        // (Only parts that solve can fail to land; the rest pay nothing for it.)
        if curves && solving {
            // What walls, openings and offsets made, the walls' centrelines, and the offsets'
            // sources: geometry with a record that remakes it, which a part isn't moved with.
            let owned: HashSet<EntityId> = if self.doc.has_walls() || self.doc.has_offsets() {
                made.iter()
                    .copied()
                    .chain(
                        self.doc
                            .offsets()
                            .flat_map(|(_, o)| o.source.iter().copied()),
                    )
                    .collect()
            } else {
                HashSet::new()
            };
            let edit_scope = self.edit_scope();
            for part in out.iter_mut().filter(|p| p.needs_solve) {
                let pinned = part.constraints.iter().any(|(_, c)| pins(c))
                    || part.scope.iter().any(|id| {
                        locked.contains(id)
                            || owned.contains(id)
                            || edit_scope.as_ref().is_some_and(|e| !e.contains(id))
                    });
                if pinned {
                    continue;
                }
                part.unpinned = true;
                let dragged =
                    |p: EntityId| Param::point(p).iter().all(|q| part.dragged_set.contains(q));
                let mut holding: Vec<EntityId> = part
                    .constraints
                    .iter()
                    .filter(|&&(id, _)| !self.doc.is_driven(id))
                    .filter_map(|(_, c)| held_on(c))
                    .filter(|&(point, _)| dragged(point))
                    .flat_map(|(_, on)| on)
                    .filter(|curve| !part.ids.contains(curve))
                    .collect();
                holding.sort_unstable();
                holding.dedup();
                part.holding = holding;
            }
        }
        out
    }

    /// `part`'s [`free_dragged`](DragPart::free_dragged),
    /// [`held_radii`](DragPart::held_radii) and [`held_lines`](DragPart::held_lines), from its
    /// relations as the drag begins (driven ones hold nothing). A coordinate is fixed by `Fix`,
    /// `FixX` or `FixY`, by being locked, or by coinciding with a point so fixed, through any
    /// chain of `Coincident`s; a curve is moved by the drag where one of its points is dragged,
    /// or coincides with a dragged point, and the drag can move it, or where its radius itself
    /// is dragged.
    ///
    /// Not held, so never judged: what walls and offsets make (they are made again from their
    /// sources after the drag moves), and a line no longer than [`NO_WAY_ROUND`], which has no
    /// way round to keep.
    ///
    /// `locked` is the drawing's locked geometry ([`Document::locked_entities`]) and `made`
    /// what walls, openings and offsets made (the keys of [`Document::wall_owners`], empty
    /// without walls or offsets): found once for the whole drag by [`Self::drag_parts`].
    ///
    /// [`Document::locked_entities`]: drift_core::Document::locked_entities
    /// [`Document::wall_owners`]: drift_core::Document::wall_owners
    fn find_held_shapes(
        &self,
        part: &mut DragPart,
        locked: &std::collections::BTreeSet<EntityId>,
        made: &HashSet<EntityId>,
    ) {
        let mut fixed: HashMap<EntityId, [bool; 2]> = HashMap::new();
        for id in part.scope.iter().filter(|id| locked.contains(id)) {
            if let Some(e) = self.doc.get(*id) {
                for p in e.kind.points() {
                    fixed.insert(p, [true, true]);
                }
            }
        }
        let mut coincident = Vec::new();
        for (_, c) in part
            .constraints
            .iter()
            .filter(|&&(id, _)| !self.doc.is_driven(id))
        {
            match *c {
                Constraint::Fix { point, .. } => {
                    fixed.insert(point, [true, true]);
                }
                Constraint::FixX { point, .. } => fixed.entry(point).or_default()[0] = true,
                Constraint::FixY { point, .. } => fixed.entry(point).or_default()[1] = true,
                Constraint::Coincident { a, b } => coincident.push((a, b)),
                _ => {}
            }
        }
        // (Coincident points share their coordinates: what fixes one fixes all it is joined to.)
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &coincident {
                let fa = fixed.get(&a).copied().unwrap_or_default();
                let fb = fixed.get(&b).copied().unwrap_or_default();
                let joined = [fa[0] || fb[0], fa[1] || fb[1]];
                for p in [a, b] {
                    let f = fixed.entry(p).or_default();
                    if *f != joined {
                        *f = joined;
                        changed = true;
                    }
                }
            }
        }
        let mut moved: HashSet<EntityId> = HashSet::new();
        for &(id, p) in &part.start.points {
            if !part.dragged_set.contains(&Param::X(id)) {
                continue;
            }
            let f = fixed.get(&id).copied().unwrap_or_default();
            if !(f[0] && f[1]) {
                part.free_dragged.push((id, p, [!f[0], !f[1]]));
                moved.insert(id);
            }
        }
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &coincident {
                if moved.contains(&a) != moved.contains(&b) {
                    moved.insert(a);
                    moved.insert(b);
                    changed = true;
                }
            }
        }
        let unheld = |id: EntityId, points: &[EntityId]| {
            made.contains(&id) || points.iter().any(|p| moved.contains(p))
        };
        let mut held: Vec<(EntityId, f64)> = Vec::new();
        let mut lines: Vec<(EntityId, DVec2, DVec2)> = Vec::new();
        for &id in part.scope.iter() {
            let (points, radius) = match self.doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Arc { center, start, end }) => {
                    (vec![center, start, end], part.start.radii.get(&id).copied())
                }
                Some(EntityKind::Circle { center, .. }) => (
                    vec![center],
                    part.start
                        .circles
                        .binary_search_by_key(&id, |&(c, _)| c)
                        .ok()
                        .map(|i| part.start.circles[i].1),
                ),
                Some(EntityKind::Line { a, b }) => {
                    if !unheld(id, &[a, b])
                        && let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b))
                        && pa.distance(pb) > NO_WAY_ROUND
                    {
                        lines.push((id, pa, pb));
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(r) = radius
                && !unheld(id, &points)
                && !part.dragged_set.contains(&Param::Radius(id))
            {
                held.push((id, r));
            }
        }
        held.sort_unstable_by_key(|&(id, _)| id);
        lines.sort_unstable_by_key(|&(id, ..)| id);
        part.held_radii = held;
        part.held_lines = lines;
    }

    /// [`find_held_shapes`](Self::find_held_shapes) for one part, finding the locked
    /// geometry and what walls and offsets made for it alone (as it did before they were
    /// found once per drag): for the tests, to show both give the same.
    #[cfg(test)]
    fn find_held_shapes_alone(&self, part: &mut DragPart) {
        let locked = self.doc.locked_entities();
        let made: HashSet<EntityId> = if self.doc.has_walls() || self.doc.has_offsets() {
            self.doc.wall_owners().of.into_keys().collect()
        } else {
            HashSet::new()
        };
        self.find_held_shapes(part, &locked, &made);
    }

    /// Whether `part`, moved by `offset` from where the drag found it, didn't land and broke a
    /// shape the drag doesn't move: changed the radius of a held arc or circle by more than
    /// [`SHAPE_HOLD`] times the offset, shortened a held line by more than half [`LINE_HOLD`]
    /// times the offset while moving an end of it more than that times it, or left a held line
    /// at [`COLLAPSED_LINE`] of its length. A frame [`drag`](Self::drag) refuses, so the drag
    /// stops at the last shape that went. Judged on the frame's result against the drag's start
    /// and the frame's offset from it, not step by step: how the frame's own steps were split
    /// doesn't enter.
    ///
    /// Landed: every coordinate of a dragged point that the drag can move is at its target, to
    /// within the length bar the frame's last solve was held to (`bar`, read through
    /// `drift_solver::landed_within`): what a solve that lands leaves, far below any misfit
    /// that matters.
    fn breaks_held_shape(&self, part: &DragPart, offset: DVec2, bar: f64) -> bool {
        if part.held_radii.is_empty() && part.held_lines.is_empty() {
            return false;
        }
        let landed = part
            .free_dragged
            .iter()
            .all(|&(id, start, [free_x, free_y])| {
                let to = start + offset;
                self.doc.point(id).is_ok_and(|at| {
                    let off = at - to;
                    let off = DVec2::new(
                        if free_x { off.x } else { 0.0 },
                        if free_y { off.y } else { 0.0 },
                    );
                    off.length() <= drift_solver::landed_within(bar, to)
                })
            });
        if landed {
            return false;
        }
        let most = SHAPE_HOLD * offset.length();
        let radius_broken = part.held_radii.iter().any(|&(id, was)| {
            let now = match self.doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Circle { radius, .. }) => Some(radius),
                Some(EntityKind::Arc { .. }) => drift_solver::radius_of(&self.doc, id),
                _ => None,
            };
            now.is_some_and(|r| (r - was).abs() > most)
        });
        radius_broken
            || part.held_lines.iter().any(|&(id, a0, b0)| {
                let Some(EntityKind::Line { a, b }) = self.doc.get(id).map(|e| e.kind) else {
                    return false;
                };
                let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b)) else {
                    return false;
                };
                let (was, now) = (a0.distance(b0), pa.distance(pb));
                // (Judged by how far its ends went, which is at least how much it shortened: a
                // frame let go never moves a point of a line it shortens fast more than the
                // threshold. The half only tells a line sliding along itself from one that
                // swings by a lever's own geometry, not a second limit: a line that shortens by
                // less while an end swings farther goes, as it always did.)
                let flung = pa.distance(a0).max(pb.distance(b0)) > LINE_HOLD * offset.length();
                (was - now > LINE_HOLD / 2.0 * offset.length() && flung)
                    || now <= COLLAPSED_LINE * was
            })
    }

    /// The parts of `parts` that go exactly where they are moved, as one move.
    fn whole_move(&self, parts: &[DragPart]) -> WholeMove {
        let mut whole = WholeMove::default();
        for part in parts.iter().filter(|p| p.moves_whole) {
            let moved = self.doc.defining_points(part.ids.iter().copied());
            for &(id, p) in &part.start.points {
                if moved.contains(&id) {
                    whole.points.push((id, p));
                } else {
                    whole.kept.push((id, p));
                }
            }
            whole.circles.extend_from_slice(&part.start.circles);
            for &id in part.scope.iter() {
                if let Some(
                    e @ drift_core::Entity {
                        kind:
                            EntityKind::Arc { .. }
                            | EntityKind::Ellipse { .. }
                            | EntityKind::EllipseArc { .. },
                        ..
                    },
                ) = self.doc.get(id)
                {
                    whole.rounds.extend(e.kind.points());
                }
            }
            whole.moved.extend(moved);
            whole.scope.extend(part.scope.iter().copied());
        }
        whole
    }

    /// Whether moving `ids` (dragging `dragged` of their points) would stretch a line
    /// hanging from them that the drag keeps at its length (see [`Self::drag_start`]): a
    /// solve carries it along instead. (A line with a dragged end is reshaped by it, solve
    /// or not.)
    fn stretches_hanging_line(
        &self,
        ids: &[EntityId],
        dragged: &HashSet<Param>,
        before: &Geometry,
    ) -> bool {
        let moved = self.doc.defining_points(ids.iter().copied());
        let carried = |p: EntityId| {
            moved.contains(&p) && !Param::point(p).iter().all(|q| dragged.contains(q))
        };
        before.lengths.keys().any(|&line| {
            matches!(self.doc.get(line).map(|e| e.kind),
                Some(EntityKind::Line { a, b })
                    if moved.contains(&a) != moved.contains(&b) && (carried(a) || carried(b)))
        })
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

    /// One call's worth of [`drag`](Self::drag), keeping what `before` says to. A move of
    /// curves in a part nothing pins that today's steps can't make, or that doesn't land, is
    /// tried once more from `before` with the solver's fold steps (a kept radius or length
    /// stalled at a fold, rows with a double root: #282): the same steps, carries and all,
    /// each solve finishing only where it meets everything. It is kept only if it goes and
    /// lands; otherwise the frame is exactly as today's steps left it.
    ///
    /// Then, where the frame went and landed but left a line hanging from what moves short of
    /// the length it keeps, one more strict try (see [`keep_lengths`](Self::keep_lengths)).
    fn drag_frame(&mut self, frame: &mut DragFrame<'_>, delta: DVec2, before: &Geometry) -> bool {
        let pushed = frame.pushed;
        let ok = self.drag_frame_today(frame, delta, before);
        if ok {
            self.keep_lengths(frame, delta, before, pushed);
        }
        ok
    }

    /// [`drag_frame`](Self::drag_frame) without the try at keeping a length.
    fn drag_frame_today(
        &mut self,
        frame: &mut DragFrame<'_>,
        delta: DVec2,
        before: &Geometry,
    ) -> bool {
        let pushed = frame.pushed;
        let ok = self.drag_frame_steps(frame, delta, before);
        if frame.fold
            || !frame.unpinned
            || self.drag_no_fold
            || frame.fold_solves_left == 0
            || frame.scope.len() * drift_solver::MAX_ITERATIONS > frame.fold_unfinished_left
            || (ok && self.landed(frame, delta, before))
        {
            return ok;
        }
        // What today's steps leave, to put back if the try fails. A kept try's geometry is
        // its own solves' alone, so what it pushed is its, from where the frame began. How far
        // the frame's solves wandered stays what today's steps made it either way: the try
        // never changes which way `drag_from_start` goes on (a whole move that wandered is
        // taken again in steps, and one that didn't is kept), so a frame that goes today
        // isn't sent down a stepped path today's steps never took.
        let ended = self.geometry_of(frame.scope);
        let pushed = std::mem::replace(&mut frame.pushed, pushed);
        let wandered = frame.wandered;
        self.restore_geometry(before);
        self.drag_folds += 1;
        std::mem::swap(&mut frame.solves_left, &mut frame.fold_solves_left);
        std::mem::swap(&mut frame.unfinished_left, &mut frame.fold_unfinished_left);
        frame.fold = true;
        let folded =
            self.drag_frame_steps(frame, delta, before) && self.landed(frame, delta, before);
        frame.fold = false;
        frame.wandered = wandered;
        std::mem::swap(&mut frame.solves_left, &mut frame.fold_solves_left);
        std::mem::swap(&mut frame.unfinished_left, &mut frame.fold_unfinished_left);
        if folded {
            return true;
        }
        self.restore_geometry(&ended);
        self.index
            .update_moved(&self.doc, frame.scope.iter().copied());
        frame.pushed = pushed;
        ok
    }

    /// After a frame that went and landed (#284): a line hanging from what moves (one of
    /// `before.lengths`) may be left short of, or past, the length it keeps (by more than
    /// [`LENGTH_TRY_STRETCH`] of it), where the step's
    /// first solve put its free end at a fold and the kept length has no first-order way back
    /// (the minimum-change step closes next to nothing of it, and the solve stops). The frame
    /// is tried once more from `before` with the fold steps, the same steps and carries as
    /// any frame, each solve finishing only where it meets everything. It is kept only if it
    /// goes, lands, keeps every hanging line's length (to `FOLD_MET` of it) and meets every
    /// relation of the part as a solve holds it (`drift_solver::meets_within`, the solver's
    /// own bar in the part's frame). Otherwise the frame is exactly as today's
    /// steps left it. So a hanging line swings round to keep its length, where it can,
    /// instead of being stretched. It can't make a drag stop: it only ever replaces a frame
    /// that went with one that went. The try has its own budgets, and isn't made for a part
    /// too big for one more solve to run to its cap in [`UNFINISHED_WORK`].
    fn keep_lengths(
        &mut self,
        frame: &mut DragFrame<'_>,
        delta: DVec2,
        before: &Geometry,
        pushed_before: bool,
    ) {
        if self.drag_no_length_try
            || frame.fold
            || frame.length_solves_left == 0
            || frame.scope.len() * drift_solver::MAX_ITERATIONS > frame.length_unfinished_left
        {
            return;
        }
        let length = |ed: &Self, line: EntityId| match ed.doc.get(line).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => match (ed.doc.point(a), ed.doc.point(b)) {
                (Ok(a), Ok(b)) => Some(a.distance(b)),
                _ => None,
            },
            _ => None,
        };
        let kept_to = |ed: &Self, share: f64| {
            before.lengths.iter().all(|(&line, &was)| {
                length(ed, line).is_some_and(|now| (now - was).abs() <= share * was)
            })
        };
        if kept_to(self, LENGTH_TRY_STRETCH) || !self.landed(frame, delta, before) {
            return;
        }
        let ended = self.geometry_of(frame.scope);
        // (A kept try's geometry is its own solves' alone, so what it pushed is its, from
        // where the frame began; `pushed_before`.)
        let pushed = std::mem::replace(&mut frame.pushed, pushed_before);
        let wandered = frame.wandered;
        self.drag_length_tries.0 += 1;
        self.restore_geometry(before);
        std::mem::swap(&mut frame.solves_left, &mut frame.length_solves_left);
        std::mem::swap(
            &mut frame.unfinished_left,
            &mut frame.length_unfinished_left,
        );
        frame.fold = true;
        let went = self.drag_frame_steps(frame, delta, before) && self.landed(frame, delta, before);
        frame.fold = false;
        std::mem::swap(&mut frame.solves_left, &mut frame.length_solves_left);
        std::mem::swap(
            &mut frame.unfinished_left,
            &mut frame.length_unfinished_left,
        );
        frame.wandered = wandered;
        if went
            && kept_to(self, drift_solver::FOLD_MET)
            && drift_solver::meets_within(
                &self.doc,
                frame.scope,
                frame.relations,
                frame.lowering.as_deref(),
                Some(frame.touching),
            )
        {
            self.drag_length_tries.1 += 1;
            return;
        }
        self.restore_geometry(&ended);
        self.index
            .update_moved(&self.doc, frame.scope.iter().copied());
        frame.pushed = pushed;
    }

    /// [`drag_frame`](Self::drag_frame)'s steps, solved with the fold steps or not as
    /// `frame.fold` says.
    fn drag_frame_steps(
        &mut self,
        frame: &mut DragFrame<'_>,
        delta: DVec2,
        before: &Geometry,
    ) -> bool {
        // Free geometry (no relations, nothing pinned, no arcs keeping their shape) goes exactly
        // where it is moved, as in a drawing program: no solve to nudge it. Unless the move
        // stretches a line hanging from moved curves, which a solve carries along instead.
        if !frame.needs_solve && !self.stretches_hanging_line(frame.ids, frame.dragged_set, before)
        {
            self.doc
                .translate_within(frame.ids.iter().copied(), delta, frame.scope);
            self.index
                .update_moved(&self.doc, frame.scope.iter().copied());
            return true;
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
        // In a part nothing pins, a move of curves can always be made, and land: moving the
        // whole part along meets every relation. A step there that can't be made, or that
        // leaves the dragged off their targets, is the solve failing, as it does where a
        // dragged point is held on a curve at a fold (#282, #283).
        if !frame.holding.is_empty()
            && !self.drag_no_carry
            && !(ok && self.landed(frame, delta, before))
        {
            ok = self.carry_holding(frame, &ids, &mut carried_points, delta, before, ok);
        }
        // Only the connected part can have moved.
        self.index
            .update_moved(&self.doc, frame.scope.iter().copied());
        frame.pushed |= ok && !carried_points.is_empty();
        ok
    }

    /// Whether the step from `before` left every dragged point where it put it, moved by
    /// `delta`, to within the length bar the frame's last solve was held to, read through
    /// `drift_solver::landed_within` (halves add up to the whole only to rounding, which its
    /// floor covers).
    fn landed(&self, frame: &DragFrame<'_>, delta: DVec2, before: &Geometry) -> bool {
        before.points.iter().all(|&(id, p)| {
            let to = p + delta;
            !frame.dragged_set.contains(&Param::X(id))
                || self
                    .doc
                    .point(id)
                    .is_ok_and(|at| at.distance(to) <= drift_solver::landed_within(frame.bar, to))
        })
    }

    /// The step from `before` by `delta` once more, with the curves [`DragFrame::holding`]
    /// its dragged points on (those `ids` doesn't move already) carried along whole: their
    /// points start moved with it and give way only after everything else. The result is
    /// kept only if it lands; otherwise the step is as it was (`ok`: kept where it ended,
    /// else refused), with nothing carried. The frame's budgets are the step's own.
    fn carry_holding(
        &mut self,
        frame: &mut DragFrame<'_>,
        ids: &[EntityId],
        carried_points: &mut Vec<Param>,
        delta: DVec2,
        before: &Geometry,
        ok: bool,
    ) -> bool {
        let holding: Vec<EntityId> = frame
            .holding
            .iter()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        if holding.is_empty() {
            return ok;
        }
        let ended = ok.then(|| self.geometry_of(frame.scope));
        self.restore_geometry(before);
        let mut carried = carried_points.clone();
        for p in self.doc.defining_points(holding.iter().copied()) {
            for q in Param::point(p) {
                if !frame.dragged_set.contains(&q) && !carried.contains(&q) {
                    carried.push(q);
                }
            }
        }
        let ids: Vec<EntityId> = ids.iter().copied().chain(holding).collect();
        let mut turned = Vec::new();
        if self.drag_in_steps(frame, &ids, delta, &carried, before, 0, &mut turned)
            && self.landed(frame, delta, before)
        {
            *carried_points = carried;
            return true;
        }
        self.restore_geometry(ended.as_ref().unwrap_or(before));
        ok
    }

    /// One step of [`drag`](Self::drag) from `before` (the scope's geometry now), split in
    /// halves (up to `MAX_SPLITS` deep) where its solve fails or it turns the shape. False,
    /// with the geometry part way, if it can't go (or the frame has run all the solves it
    /// may); then `turned` holds the curves even the smallest step turned, where that step's
    /// solve finished.
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
        // (Crushed is judged against the drag's starting radii, which `before` carries.)
        let orientation = drift_solver::Orientation::among_kept(
            &self.doc,
            Some(frame.scope),
            frame.constraints,
            &before.radii,
            frame.start_circles,
        );
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
            // (A line too short to see has no way round to keep: any move, however small,
            // could turn it, and every step would be split for nothing. Imports bring in
            // such slivers.)
            .filter(|&(.., was)| was.length() > NO_WAY_ROUND)
            .collect();
        // Lines a lowered relation measures a signed offset from (walls' centrelines under
        // dimensioned faces) keep their way round too: turned, which side is the left face
        // would swap, and the relation would hold the other face.
        let lowered_lines: Vec<(EntityId, EntityId, EntityId, DVec2)> = frame
            .lowered_lines
            .iter()
            .filter_map(|&line| match self.doc.get(line)?.kind {
                EntityKind::Line { a, b } => Some((
                    line,
                    a,
                    b,
                    self.doc.point(b).ok()? - self.doc.point(a).ok()?,
                )),
                _ => None,
            })
            .collect();
        self.doc
            .translate_within(ids.iter().copied(), delta, frame.scope);
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
        self.keep_smooth_anchors(frame.smooth, ids, delta);
        frame.solves_left -= 1;
        self.drag_solved += frame.scope.len();
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
                relations: Some(frame.relations.clone()),
                touching: Some(frame.touching.clone()),
                fold: frame.fold,
                bar_cap: (frame.first_bar > 0.0).then_some(frame.first_bar),
                ..Default::default()
            },
        );
        frame.wandered = frame.wandered.max(report.wandered);
        frame.bar = report.bar;
        if frame.first_bar == 0.0 {
            frame.first_bar = report.bar;
        }
        #[cfg(test)]
        if let Some(bars) = &mut self.drag_bars {
            bars.push(report.bar);
        }
        let unfinished = self.unfinished(&report);
        if unfinished {
            frame.unfinished_left = frame
                .unfinished_left
                .saturating_sub(frame.scope.len() * report.iterations.max(1));
            self.drag_unfinished += 1;
        }
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
        if !unfinished && flipped.is_empty() && !orientation.turned(&self.doc) {
            return true;
        }
        if depth >= MAX_SPLITS && !unfinished {
            // Even the smallest step turns these. (A solve that didn't finish left the
            // geometry part way, which says nothing of what turns.)
            *turned = orientation.turned_curves(&self.doc);
            turned.append(&mut flipped);
        }
        self.restore_geometry(before);
        // (A solve that doesn't finish at one size may at a smaller one: the drag's relations
        // held where it began.)
        if depth >= MAX_SPLITS
            || (unfinished && frame.unfinished_left == 0)
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

#[cfg(test)]
mod held_radii_tests {
    use drift_core::{Constraint, DVec2};
    use drift_solver::Param;

    use super::Editor;

    /// The locked geometry and what walls and offsets made, found once for a drag, hold
    /// what finding them for each part did: the same moved points, held radii and held lines
    /// for every part, on a drawing with a locked layer, a wall, an offset and parts that
    /// solve (pinned, related, round).
    #[test]
    fn held_shapes_found_once_a_drag_are_found_per_part() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let v = DVec2::new;
        let mut dragged_ids = Vec::new();
        for k in 0..6 {
            let x = k as f64 * 40.0;
            let c = ed.doc.add_point(v(x, 0.0));
            let s = ed.doc.add_point(v(x + 10.0, 0.0));
            let e = ed.doc.add_point(v(x, 10.0));
            let arc = ed.doc.add_arc(c, s, e).unwrap();
            let p = ed.doc.add_point(v(x + 10.0, 20.0));
            let line = ed.doc.add_line(s, p).unwrap();
            let q = ed.doc.add_point(v(x + 20.0, 25.0));
            let held = ed.doc.add_line(p, q).unwrap();
            if k % 2 == 0 {
                ed.doc
                    .add_constraint(Constraint::Fix {
                        point: q,
                        at: v(x + 20.0, 25.0),
                    })
                    .unwrap();
            }
            ed.doc
                .add_constraint(Constraint::Horizontal { line: held })
                .unwrap();
            let _ = arc;
            dragged_ids.push(line);
        }
        // A locked layer with a line joined to the second part's dragged point (its line's
        // start): locked, that point is fixed.
        let locked_pt = ed.doc.add_point(v(40.0 + 10.0, -15.0));
        let locked_line = {
            let Some(drift_core::EntityKind::Line { a: s, .. }) =
                ed.doc.get(dragged_ids[1]).map(|e| e.kind)
            else {
                unreachable!()
            };
            ed.doc.add_line(s, locked_pt).unwrap()
        };
        assert!(ed.commit("Sketch"));
        let layer = ed.add_layer("Locked");
        ed.doc.begin();
        ed.doc.set_entity_layer(locked_line, layer).unwrap();
        assert!(ed.commit("Layer"));
        let mut l = ed.doc.layer(layer).unwrap().clone();
        l.locked = true;
        ed.update_layer(layer, l);
        // A wall, and an offset of the last part's line.
        ed.doc.begin();
        let (a, b) = (
            ed.doc.add_point(v(0.0, 100.0)),
            ed.doc.add_point(v(200.0, 100.0)),
        );
        let centre = ed.doc.add_line(a, b).unwrap();
        ed.doc.set_style(centre, drift_core::LineKind::Construction);
        let path = ed
            .doc
            .add_path(drift_core::Path {
                curves: vec![centre],
                closed: false,
            })
            .unwrap();
        ed.doc
            .add_wall(drift_core::Wall::new(
                path,
                10.0,
                drift_core::LayerId::DEFAULT,
            ))
            .unwrap();
        assert!(ed.commit("Wall"));
        let last = *dragged_ids.last().unwrap();
        let _ = ed.offset(last, 3.0, v(215.0, 10.0));
        assert!(ed.doc.has_walls() && ed.doc.has_offsets());
        assert!(!ed.doc.locked_entities().is_empty());
        let dragged: Vec<Param> = ed
            .doc
            .defining_points(dragged_ids.iter().copied())
            .into_iter()
            .flat_map(Param::point)
            .collect();
        ed.doc.begin();
        ed.drag(&dragged_ids, v(0.5, 0.25), &dragged);
        // (Found from the drawing as the drag found it.)
        ed.doc.rollback();
        let session = ed.drag_session.as_ref().unwrap();
        let mut solving = 0;
        for part in session.parts.iter().filter(|p| p.needs_solve) {
            solving += 1;
            let mut alone = part.clone();
            alone.free_dragged.clear();
            alone.held_radii.clear();
            alone.held_lines.clear();
            ed.find_held_shapes_alone(&mut alone);
            assert_eq!(alone.free_dragged, part.free_dragged);
            assert_eq!(alone.held_radii, part.held_radii);
            assert_eq!(alone.held_lines, part.held_lines);
        }
        assert!(solving >= 6, "{solving} parts solve");
        // (The lock reached a part: one of its dragged points is fixed by it.)
        let locked = ed.doc.locked_entities();
        assert!(
            session
                .parts
                .iter()
                .any(|p| p.scope.iter().any(|id| locked.contains(id))
                    && p.free_dragged.len() < p.dragged.len() / 2)
        );
        assert!(
            session.parts.iter().any(|p| !p.held_lines.is_empty()),
            "something held"
        );
    }

    /// What [`Editor::breaks_held_shape`] judges: an arc whose end is a separate point joined
    /// by `Coincident` to the dragged point is moved by the drag, not held; an arc tied to the
    /// drag only through relations is held; a dragged point's coordinate fixed by `FixX` is
    /// one the drag can't move.
    #[test]
    fn what_the_drag_moves_and_what_it_holds() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let s = ed.doc.add_point(DVec2::new(10.0, 0.0));
        let e = ed.doc.add_point(DVec2::new(0.0, 10.0));
        let moved = ed.doc.add_arc(c, s, e).unwrap();
        let c2 = ed.doc.add_point(DVec2::new(30.0, 0.0));
        let s2 = ed.doc.add_point(DVec2::new(35.0, 0.0));
        let e2 = ed.doc.add_point(DVec2::new(30.0, 5.0));
        let held = ed.doc.add_arc(c2, s2, e2).unwrap();
        let q = ed.doc.add_point(DVec2::new(0.0, 10.0));
        for r in [
            Constraint::Coincident { a: e, b: q },
            Constraint::Fix {
                point: s,
                at: DVec2::new(10.0, 0.0),
            },
            Constraint::Equal { a: moved, b: held },
            Constraint::FixX { point: q, x: 0.0 },
        ] {
            ed.doc.add_constraint(r).unwrap();
        }
        assert!(ed.commit("Sketch"));
        ed.doc.begin();
        ed.drag(&[q], DVec2::new(0.0, 0.5), &Param::point(q));
        let session = ed.drag_session.as_ref().unwrap();
        let part = session.parts.iter().find(|p| p.scope.contains(&q)).unwrap();
        let held_ids: Vec<_> = part.held_radii.iter().map(|&(id, _)| id).collect();
        assert_eq!(held_ids, vec![held]);
        assert_eq!(
            part.free_dragged
                .iter()
                .map(|&(id, _, free)| (id, free))
                .collect::<Vec<_>>(),
            vec![(q, [false, true])]
        );
    }

    /// Through a chain of two `Coincident`s: an arc whose end is joined to the dragged point by
    /// way of a third point is moved by the drag; a dragged point joined by way of a third
    /// point to a fixed one can't be moved at all (it has no free coordinate).
    #[test]
    fn coincident_chains_carry_what_moves_and_what_is_fixed() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let s = ed.doc.add_point(DVec2::new(10.0, 0.0));
        let e = ed.doc.add_point(DVec2::new(0.0, 10.0));
        let arc = ed.doc.add_arc(c, s, e).unwrap();
        let m = ed.doc.add_point(DVec2::new(0.0, 10.0));
        let q = ed.doc.add_point(DVec2::new(0.0, 10.0));
        let f = ed.doc.add_point(DVec2::new(20.0, 0.0));
        let g = ed.doc.add_point(DVec2::new(20.0, 0.0));
        let pinned = ed.doc.add_point(DVec2::new(20.0, 0.0));
        let line = ed.doc.add_line(q, pinned).unwrap();
        for r in [
            Constraint::Coincident { a: e, b: m },
            Constraint::Coincident { a: m, b: q },
            Constraint::Coincident { a: f, b: g },
            Constraint::Coincident { a: g, b: pinned },
            Constraint::Fix {
                point: f,
                at: DVec2::new(20.0, 0.0),
            },
            Constraint::Fix {
                point: s,
                at: DVec2::new(10.0, 0.0),
            },
        ] {
            ed.doc.add_constraint(r).unwrap();
        }
        assert!(ed.commit("Sketch"));
        ed.doc.begin();
        let dragged: Vec<Param> = [q, pinned].into_iter().flat_map(Param::point).collect();
        ed.drag(&[line], DVec2::new(0.0, 0.5), &dragged);
        let session = ed.drag_session.as_ref().unwrap();
        let part = session.parts.iter().find(|p| p.scope.contains(&q)).unwrap();
        assert!(part.held_radii.iter().all(|&(id, _)| id != arc));
        assert_eq!(
            part.free_dragged
                .iter()
                .map(|&(id, ..)| id)
                .collect::<Vec<_>>(),
            vec![q]
        );
    }

    /// A circle whose radius is itself dragged is moved by the drag, not held.
    #[test]
    fn a_dragged_radius_is_moved_not_held() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(DVec2::ZERO);
        let circle = ed.doc.add_circle(c, 5.0).unwrap();
        let p = ed.doc.add_point(DVec2::new(5.0, 0.0));
        ed.doc
            .add_constraint(Constraint::PointOnCurve {
                point: p,
                curve: circle,
            })
            .unwrap();
        assert!(ed.commit("Sketch"));
        ed.doc.begin();
        let dragged = [Param::X(p), Param::Y(p), Param::Radius(circle)];
        ed.drag(&[p], DVec2::new(0.5, 0.0), &dragged);
        let session = ed.drag_session.as_ref().unwrap();
        let part = session
            .parts
            .iter()
            .find(|p| p.scope.contains(&circle))
            .unwrap();
        assert!(part.held_radii.is_empty());
    }
}
