//! Geometric constraint solver for Drift.
//!
//! [`solve`] adjusts a document's geometry so every constraint holds, moving things as little
//! as possible: each step solves `(JᵀJ + μ·M⁻¹) Δ = −JᵀF`, which for small μ is the weighted
//! minimum-norm Gauss–Newton step, with Levenberg–Marquardt damping when progress stalls.
//! Parameters being dragged are given a heavy weight, so the rest of the sketch adapts around
//! them. [`analyze`] reports degrees of freedom and which entities are fully defined.
//!
//! Pipeline: parameters → substitution (coincident / horizontal / vertical / fix merge or pin
//! parameters) → equations → independent components → Newton per component → write back.

pub mod bench;
mod equations;
mod linalg;
mod lowering;
mod sparse;
mod system;

use std::collections::{BTreeMap, HashMap, HashSet};

use drift_core::polygon::UnionFind;
use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};

pub use crate::lowering::{Lowered, Lowering};

use crate::linalg::{Matrix, rref};
use crate::sparse::{Dd, Skyline, qr_rank};
use crate::system::{EntityShape, Shape, System};

/// A solver unknown: a point coordinate or a circle radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Param {
    X(EntityId),
    Y(EntityId),
    Radius(EntityId),
    /// Midpoint of a line that a dimension measures to (internal; never written back).
    MidX(EntityId),
    MidY(EntityId),
}

impl Param {
    /// Parameters of a point.
    pub fn point(id: EntityId) -> [Param; 2] {
        [Param::X(id), Param::Y(id)]
    }
}

/// Convergence tolerances.
#[derive(Clone, Copy, Debug)]
pub struct Tolerances {
    /// Length residual tolerance, relative to the magnitude of the coordinates involved.
    pub length: f64,
    /// Angle residual tolerance (a sine).
    pub angle: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            length: 1e-12,
            angle: 1e-12,
        }
    }
}

/// What a solve keeps, in rank: the relations always hold; among the geometry that meets
/// them, the [`dragged`](Self::dragged) stay put first, then the [`carried`](Self::carried),
/// then (with a [`focus`](Self::focus)) everything else; then the shapes
/// ([`radii`](Self::radii), [`lengths`](Self::lengths), [`directions`](Self::directions))
/// are kept where that still leaves them open, as one preference. A lower rank never
/// moves a higher one.
#[derive(Clone, Debug, Default)]
pub struct SolveOptions {
    /// Parameters that should stay where they are if at all possible (e.g. the point under the
    /// mouse while dragging).
    pub dragged: Vec<Param>,
    /// Parameters a drag carries along with it (the points of a curve a move pushes rather
    /// than reshapes): they stay where they are asked to like [`dragged`](Self::dragged), but
    /// give way before the dragged where the relations won't have both, and no one dragged
    /// them, so a line or arc they end keeps its length or radius preference like any other
    /// (dragging an end reshapes its curve; being carried doesn't).
    pub carried: Vec<Param>,
    /// Also run the degrees-of-freedom analysis.
    pub analyze: bool,
    pub tolerances: Tolerances,
    /// Solve only these entities (closed under shared points and constraints; see
    /// [`connected`]): a drag in a big sketch then costs what the dragged part costs.
    pub scope: Option<std::rc::Rc<HashSet<EntityId>>>,
    /// Arcs and the radii they should keep where they can (typically their sizes before a
    /// drag moved some points): a move that could go either way follows a round with its
    /// lines rather than shrinking it. Ignored for arcs with a dragged end.
    pub radii: HashMap<EntityId, f64>,
    /// Parameters that should take up the change (e.g. the geometry a new relation names):
    /// when set, everything else moves only as far as it has to, so meeting a relation
    /// reshapes what it relates rather than spreading the change thinly over the sketch.
    pub focus: Vec<Param>,
    /// Lines and the directions (unit vectors) they should keep where they can, as
    /// [`radii`](Self::radii) does for arcs.
    pub directions: HashMap<EntityId, DVec2>,
    /// Lines and the lengths they should keep where they can, as [`radii`](Self::radii) does
    /// for arcs: moving what a line hangs from then carries it along rather than stretching
    /// it. Where the relations change a length anyway, the preference is let go. Ignored for
    /// lines with a dragged end: dragging an end reshapes its line.
    pub lengths: HashMap<EntityId, f64>,
    /// Relations read in the form they are lowered to (see [`Lowering`]).
    pub lowering: Option<std::rc::Rc<Lowering>>,
}

/// How completely an entity's position is determined by constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Definition {
    /// Can still move.
    Under,
    /// Completely determined.
    Full,
    /// Involved in constraints that can't all be satisfied.
    Over,
}

#[derive(Clone, Debug, Default)]
pub struct SolveReport {
    /// Every constraint is satisfied within tolerance.
    pub converged: bool,
    /// Newton iterations used by the slowest component.
    pub iterations: usize,
    /// The most steps any component took on its way to the relations that went badly (their
    /// linearisation promised far more than they did, or they were refused): the mark of a
    /// solve that wanders round a fold between solutions rather than heading for the nearest
    /// one (#103), where a drag should go in smaller steps.
    pub wandered: usize,
    /// Constraints that could not be satisfied.
    pub conflicts: Vec<ConstraintId>,
    /// Constraints already implied by others (found during substitution).
    pub redundant: Vec<ConstraintId>,
    /// Equations that are linearly dependent on others (redundant but consistent).
    pub dependent_equations: usize,
    /// Remaining degrees of freedom (with [`SolveOptions::analyze`]).
    pub dof: Option<usize>,
    /// Per-entity definition state (with [`SolveOptions::analyze`]).
    pub definitions: HashMap<EntityId, Definition>,
}

const MAX_ITERATIONS: usize = 100;

/// How strongly a shape is kept in a minimum-change step, relative to moving a point: being
/// off a radius, length or direction by some distance costs as much as moving a point that
/// far, this many times over.
const KEEP_SHAPE: f64 = 100.0;

/// Inverse weight for parameters outside [`SolveOptions::focus`] when it is set: moving them
/// costs this many times as much as moving the focus. The third rank of what stays put,
/// after the dragged and the carried (`system::DRAGGED_MOBILITY`, `system::CARRIED_MOBILITY`).
const UNFOCUSED_MOBILITY: f64 = 1e-3;

/// A step that delivers less than this share of what its linearisation promised went badly
/// (see [`SolveReport::wandered`]).
const WANDERING_GAIN: f64 = 0.25;

/// How far off the rows a preference's step may leave them, as a fraction of its size: what a
/// step along a curved relation leaves (second order, mended by the next step) rather than
/// one that pulls against them (first order).
const SECOND_ORDER: f64 = 0.25;

/// At most this many passes take out of a preference's step what the relations feel (see
/// `newton`), and no more once a pass changes it by less than [`FELT_ROUND_OFF`] of it.
const FELT_PASSES: usize = 6;
const FELT_ROUND_OFF: f64 = 1e-14;

/// Iterations a solve goes on for after its rows hold, for its soft preferences to settle
/// (see `newton`). Not a safety net but what bounds how far a shape is kept: each step closes
/// a share of what is left of a preference that goes as the square of how much of its
/// gradient the relations leave free (unitless, so the same at any size or turn). One on
/// freely moving geometry closes in a few steps; one the relations nearly rule out could
/// only be met by swinging geometry far (its offset over that share), and crawls toward
/// that a step at a time, so after this many it is left where it got to, at most some ten
/// times its offset away. (Without the bound the heavy drag properties creep and fling.)
const SETTLE_PATIENCE: usize = 10;

/// Solve the document's constraints in place. Changes go through the document's normal edit
/// methods, so they are recorded by an open transaction.
pub fn solve(doc: &mut Document, options: &SolveOptions) -> SolveReport {
    let mut sys = System::build_in(
        doc,
        &options.dragged,
        &options.carried,
        &options.tolerances,
        options.scope.as_deref(),
        &options.radii,
        options.lowering.as_deref(),
    );
    // Sorted, so the preferences' rows come in the same order every run (a hash map's order
    // would change the floating-point sums, and with them the result's last bits).
    let mut directions: Vec<_> = options.directions.iter().collect();
    directions.sort_unstable_by_key(|(id, _)| **id);
    for (&line, &u) in directions {
        if let Some(EntityKind::Line { a, b }) = doc.get(line).map(|e| e.kind) {
            let ends = [Param::X(a), Param::Y(a), Param::X(b), Param::Y(b)]
                .map(|p| sys.param_index.get(&p).copied());
            if let [Some(ax), Some(ay), Some(bx), Some(by)] = ends {
                sys.keep.push(Shape::Direction([ax, ay, bx, by], u));
            }
        }
    }
    // A point merged with a dragged one (coincident with it) is dragged too: a preference
    // on a line or arc it ends would only pull against the drag, and slowly, the dragged
    // being heavy, so the solve would end part way along, wherever its tolerance let it.
    // (Both coordinates: a point level with a dragged one still moves freely across. And
    // only what the user drags: a carried point's curves keep their preferences.)
    let dragged_groups: HashSet<usize> = options
        .dragged
        .iter()
        .filter_map(|q| sys.param_index.get(q).map(|&i| sys.group_of[i]))
        .collect();
    let dragged_group = |p: EntityId| {
        Param::point(p).iter().all(|q| {
            sys.param_index
                .get(q)
                .is_some_and(|&i| dragged_groups.contains(&sys.group_of[i]))
        })
    };
    for &arc in options.radii.keys() {
        if let Some(EntityKind::Arc { center, start, end }) = doc.get(arc).map(|e| e.kind)
            && (dragged_group(start) || dragged_group(end))
        {
            let key = [
                Param::X(center),
                Param::Y(center),
                Param::X(start),
                Param::Y(start),
            ]
            .map(|p| sys.param_index.get(&p).copied());
            sys.keep
                .retain(|s| !matches!(s, Shape::Distance(k, _) if k.map(Some) == key));
        }
    }
    let mut lengths: Vec<_> = options.lengths.iter().collect();
    lengths.sort_unstable_by_key(|(id, _)| **id);
    for (&line, &length) in lengths {
        if let Some(EntityKind::Line { a, b }) = doc.get(line).map(|e| e.kind)
            && !dragged_group(a)
            && !dragged_group(b)
        {
            let ends = [Param::X(a), Param::Y(a), Param::X(b), Param::Y(b)]
                .map(|p| sys.param_index.get(&p).copied());
            if let [Some(ax), Some(ay), Some(bx), Some(by)] = ends {
                sys.keep.push(Shape::Distance([ax, ay, bx, by], length));
            }
        }
    }
    if !options.focus.is_empty() {
        let focus: HashSet<usize> = options
            .focus
            .iter()
            .filter_map(|p| sys.param_index.get(p).map(|&i| sys.group_of[i]))
            .collect();
        for (g, group) in sys.groups.iter_mut().enumerate() {
            if !focus.contains(&g) {
                group.mobility = group.mobility.min(UNFOCUSED_MOBILITY);
            }
        }
    }
    // Where the dragged, then the carried, parameters were asked to be.
    let targets: Vec<Vec<(usize, f64)>> = [&options.dragged, &options.carried]
        .iter()
        .map(|params| {
            let mut groups: Vec<usize> = params
                .iter()
                .filter_map(|p| sys.param_index.get(p).map(|&i| sys.group_of[i]))
                .filter(|&g| !sys.groups[g].fixed)
                .collect();
            groups.sort_unstable();
            groups.dedup();
            groups
                .into_iter()
                .map(|g| (g, sys.groups[g].value))
                .collect()
        })
        .collect();
    let mut report = run(&mut sys, options, true);
    if report.converged {
        report = land(&mut sys, options, &targets, report);
    }
    // A result that isn't a number (a degenerate configuration the equations divide by) is
    // never written: the drawing stays as it was and the solve counts as failed.
    if !write_back(doc, &sys) {
        report.converged = false;
    }
    report
}

/// After a drag's solve: the dragged and carried parameters are only heavy, not pinned, so
/// where other geometry has to follow them they give a little (and, steps being measured from
/// where the solve is rather than from the targets, nothing pulls them back). Pin them where
/// they were asked to be and solve again, rank by rank (the dragged first, then the carried
/// with them); keep each that leaves the relations met, so a move lands exactly wherever it
/// can, and what it pushes along lands with it where that still leaves room.
fn land(
    sys: &mut System,
    options: &SolveOptions,
    targets: &[Vec<(usize, f64)>],
    mut report: SolveReport,
) -> SolveReport {
    let mut pinned: Vec<(usize, f64)> = Vec::new();
    for rank in targets {
        pinned.extend(rank);
        if rank.is_empty() || pinned.iter().all(|&(g, v)| sys.groups[g].value == v) {
            continue;
        }
        let solved: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
        for &(g, v) in &pinned {
            sys.groups[g].value = v;
            sys.groups[g].fixed = true;
        }
        let landed = run(sys, options, true);
        for &(g, _) in &pinned {
            sys.groups[g].fixed = false;
        }
        if landed.converged {
            report = SolveReport {
                iterations: landed.iterations.max(report.iterations),
                wandered: landed.wandered.max(report.wandered),
                ..landed
            };
        } else {
            for (group, v) in sys.groups.iter_mut().zip(solved) {
                group.value = v;
            }
            return report;
        }
    }
    report
}

/// The constraints the geometry doesn't meet as it stands (nothing is moved): those with an
/// equation off by more than `slack`, relative to the size of the coordinates involved (a sine,
/// for angles).
pub fn unmet(doc: &Document, slack: f64) -> HashSet<ConstraintId> {
    unmet_with(doc, slack, None)
}

/// [`unmet`], reading relations through `lowering`.
pub fn unmet_with(
    doc: &Document,
    slack: f64,
    lowering: Option<&Lowering>,
) -> HashSet<ConstraintId> {
    let tol = Tolerances {
        length: slack,
        angle: slack,
    };
    let sys = System::build(doc, &[], &tol, lowering);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let val = |p: usize| values[sys.group_of[p]];
    let mut grad = Vec::new();
    let mut out = HashSet::new();
    for (r, row) in sys.rows.iter().enumerate() {
        let Some(id) = row.source else {
            continue;
        };
        grad.clear();
        if row.equation.eval(&val, &mut grad).abs() > row_tolerance(&sys, r, &values, &tol) {
            out.insert(id);
        }
    }
    out
}

/// Evaluate constraints and degrees of freedom without changing the document.
pub fn analyze(doc: &Document) -> SolveReport {
    analyze_with(doc, None)
}

/// [`analyze`], reading relations through `lowering`.
pub fn analyze_with(doc: &Document, lowering: Option<&Lowering>) -> SolveReport {
    let options = SolveOptions {
        analyze: true,
        ..Default::default()
    };
    let mut sys = System::build(doc, &[], &options.tolerances, lowering);
    run(&mut sys, &options, false)
}

/// The entities that can move with `seeds`: everything joined to them through shared points
/// and (non-driven) constraints.
pub fn connected(doc: &Document, seeds: impl IntoIterator<Item = EntityId>) -> HashSet<EntityId> {
    connected_with(doc, seeds, None)
}

/// [`connected`], reading relations through `lowering` (a lowered relation joins what it was
/// lowered to, and the lowering's extras join theirs).
pub fn connected_with(
    doc: &Document,
    seeds: impl IntoIterator<Item = EntityId>,
    lowering: Option<&Lowering>,
) -> HashSet<EntityId> {
    let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
    let index: HashMap<EntityId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parent = UnionFind::new(ids.len());
    let join = |parent: &mut UnionFind, a: EntityId, b: EntityId| {
        if let (Some(&i), Some(&j)) = (index.get(&a), index.get(&b)) {
            parent.join(i, j);
        }
    };
    for (id, e) in doc.iter() {
        for p in e.kind.points() {
            join(&mut parent, id, p);
        }
    }
    for (_, l) in lowering::relations(doc, lowering) {
        let ents = l.entities();
        if let Some((&first, rest)) = ents.split_first() {
            for &other in rest {
                join(&mut parent, first, other);
            }
        }
    }
    let roots: HashSet<usize> = seeds
        .into_iter()
        .filter_map(|id| index.get(&id).copied())
        .map(|i| parent.find(i))
        .collect();
    (0..ids.len())
        .filter(|&i| roots.contains(&parent.find(i)))
        .map(|i| ids[i])
        .collect()
}

/// Relations whose geometry is gone or no longer of the kinds they need (a trim or a
/// split changed a curve under them), with why. The solver leaves them out.
pub fn invalid_constraints(doc: &Document) -> Vec<(drift_core::ConstraintId, String)> {
    doc.constraints()
        .filter_map(|(id, c)| doc.validate(c).err().map(|e| (id, e.to_string())))
        .collect()
}

/// Whether solving `scope` (closed as [`connected`] closes it) can do anything a plain move
/// doesn't: it holds a relation or dimension, geometry pinned on a locked layer, or a curve
/// that keeps a shape of its own (an arc's ends on its circle, an ellipse's axes square).
/// Relations the solver leaves out (see [`invalid_constraints`]) don't count. Free points,
/// lines, circles and Béziers go exactly where they are put, so a drag of them needn't solve
/// at all.
pub fn needs_solve(doc: &Document, scope: &HashSet<EntityId>) -> bool {
    needs_solve_with(doc, scope, None)
}

/// [`needs_solve`], reading relations through `lowering`. Its extras (relations of the
/// geometry itself) don't count: alone they move nothing a plain move doesn't.
pub fn needs_solve_with(
    doc: &Document,
    scope: &HashSet<EntityId>,
    lowering: Option<&Lowering>,
) -> bool {
    let constrained = doc.constraints().any(|(cid, c)| {
        !doc.is_driven(cid)
            && doc.validate(c).is_ok()
            && match lowering.and_then(|l| l.of.get(&cid)) {
                Some(list) => list
                    .iter()
                    .any(|l| l.entities().iter().any(|e| scope.contains(e))),
                None => c.entities().any(|e| scope.contains(&e)),
            }
    });
    // (A wall's arc faces are generated, not solved.)
    let generated = doc.generated_entities();
    let shaped = scope
        .iter()
        .filter(|id| !generated.contains(id))
        .any(|&id| {
            matches!(
                doc.get(id).map(|e| e.kind),
                Some(
                    EntityKind::Arc { .. }
                        | EntityKind::Ellipse { .. }
                        | EntityKind::EllipseArc { .. }
                )
            )
        });
    constrained || shaped || doc.locked_entities().iter().any(|id| scope.contains(id))
}

/// Whether solving the whole drawing can do anything (see [`needs_solve`]): it holds a
/// relation or dimension, geometry on a locked layer, or a curve that keeps a shape of its
/// own. A drawing of free lines (a plan of walls with no relations) has nothing to solve.
pub fn anything_to_solve(doc: &Document) -> bool {
    if doc
        .constraints()
        .any(|(cid, c)| !doc.is_driven(cid) && doc.validate(c).is_ok())
    {
        return true;
    }
    if doc.layers().any(|(_, l)| l.locked) && !doc.locked_entities().is_empty() {
        return true;
    }
    let generated = doc.generated_entities();
    doc.iter().any(|(id, e)| {
        matches!(
            e.kind,
            EntityKind::Arc { .. } | EntityKind::Ellipse { .. } | EntityKind::EllipseArc { .. }
        ) && !generated.contains(&id)
    })
}

/// Entities whose geometry has collapsed: zero-length lines, zero-radius circles and arcs.
/// Constraints can be satisfied by collapsing geometry (e.g. horizontal + vertical on one
/// line), which a sketcher should refuse.
pub fn degenerate_entities(doc: &Document) -> Vec<EntityId> {
    let point = |id| doc.point(id).ok();
    let tiny = |len: f64, scale: f64| len <= 1e-9 * scale.max(1.0);
    doc.iter()
        .filter(|(_, e)| match e.kind {
            EntityKind::Point(_) => false,
            EntityKind::Line { a, b } => match (point(a), point(b)) {
                (Some(pa), Some(pb)) => tiny(pa.distance(pb), pa.abs().max_element()),
                _ => false,
            },
            EntityKind::Circle { radius, center } => {
                point(center).is_some_and(|c| tiny(radius, c.abs().max_element()))
            }
            EntityKind::Arc { center, start, end } => {
                match (point(center), point(start), point(end)) {
                    (Some(c), Some(s), Some(e)) => {
                        let scale = c.abs().max_element();
                        tiny(c.distance(s), scale) || tiny(s.distance(e), scale)
                    }
                    _ => false,
                }
            }
            EntityKind::Bezier { a, c1, c2, b } => {
                match (point(a), point(c1), point(c2), point(b)) {
                    (Some(pa), Some(p1), Some(p2), Some(pb)) => {
                        let scale = pa.abs().max_element();
                        tiny(pa.distance(pb), scale)
                            && tiny(pa.distance(p1), scale)
                            && tiny(pa.distance(p2), scale)
                    }
                    _ => false,
                }
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            }
            | EntityKind::EllipseArc {
                center,
                major,
                minor,
                ..
            } => match (point(center), point(major), point(minor)) {
                (Some(c), Some(m), Some(n)) => {
                    let scale = c.abs().max_element();
                    let arc_collapsed = match e.kind {
                        EntityKind::EllipseArc { start, end, .. } => {
                            match (point(start), point(end)) {
                                (Some(s), Some(t)) => tiny(s.distance(t), scale),
                                _ => false,
                            }
                        }
                        _ => false,
                    };
                    tiny(c.distance(m), scale) || tiny(c.distance(n), scale) || arc_collapsed
                }
                _ => false,
            },
        })
        .map(|(id, _)| id)
        .collect()
}

/// Which way round the curved pieces of a sketch sit: how far each arc sweeps, and on which
/// side each tangency touches. A solve can meet every constraint by turning an arc inside out
/// (its centre carried past its ends, so it runs the long way round) or by moving a tangent
/// curve over to the line's other side; comparing [`Orientation::of`] before and after shows
/// it ([`Orientation::turned`]), so a drag can take smaller steps or refuse.
#[derive(Clone, Debug, Default)]
pub struct Orientation {
    /// Arcs and their counter-clockwise sweeps (radians).
    arcs: Vec<(EntityId, f64)>,
    /// Tangent pairs and their side: which side of the line a curve's centre is on (±1), for
    /// two curves whether they touch from outside (+1) or one inside the other (−1), and at a
    /// shared end whether one carries on from the other (−1) or turns back in a cusp (+1).
    sides: Vec<((EntityId, EntityId), f64)>,
    /// Distances whose sign a solve could flip, and the side each is on: which way a
    /// horizontal or vertical distance runs, which side of its line a point at a distance from
    /// it is (±1). The distance holds just as well the other way, mirrored, and one big step
    /// lands there whenever it's the nearer fit.
    signs: Vec<((EntityId, EntityId), Constraint, f64)>,
}

/// A sweep that changes by more than this in one solve has jumped, not turned.
const SWEEP_JUMP: f64 = std::f64::consts::FRAC_PI_2;

impl Orientation {
    /// The orientation of the arcs and tangencies among `scope` (everything if `None`).
    pub fn of(doc: &Document, scope: Option<&HashSet<EntityId>>) -> Self {
        let keep = |id: EntityId| scope.is_none_or(|s| s.contains(&id));
        let arcs = match scope {
            Some(ids) => ids
                .iter()
                .filter_map(|&id| Some((id, arc_sweep(doc, id)?)))
                .collect(),
            None => doc
                .iter()
                .filter_map(|(id, _)| Some((id, arc_sweep(doc, id)?)))
                .collect(),
        };
        let sides = doc
            .constraints()
            .filter_map(|(id, c)| match *c {
                Constraint::Tangent { a, b } if keep(a) && keep(b) && !doc.is_driven(id) => {
                    Some(((a, b), tangent_side(doc, a, b)?))
                }
                _ => None,
            })
            .collect();
        let signs = doc
            .constraints()
            .filter_map(|(id, c)| {
                let (a, b) = match *c {
                    Constraint::HorizontalDistance { a, b, value }
                    | Constraint::VerticalDistance { a, b, value } => {
                        (value != 0.0).then_some((a, b))?
                    }
                    Constraint::PointLineDistance { point, line, value } => {
                        (value != 0.0).then_some((point, line))?
                    }
                    _ => return None,
                };
                if !(keep(a) || keep(b)) || doc.is_driven(id) {
                    return None;
                }
                Some(((a, b), *c, distance_sign(doc, c)?))
            })
            .collect();
        Self { arcs, sides, signs }
    }

    /// True if `doc` has an arc turned inside out, or a tangency moved to the other side,
    /// since this orientation was taken. Entities added since don't count.
    pub fn turned(&self, doc: &Document) -> bool {
        !self.turned_curves(doc).is_empty()
    }

    /// The curves [`turned`](Self::turned) would blame: arcs turned inside out, and both
    /// curves of a tangency that moved to the other side.
    pub fn turned_curves(&self, doc: &Document) -> Vec<EntityId> {
        let mut out: Vec<EntityId> = self
            .arcs
            .iter()
            .filter(|&&(id, before)| {
                arc_sweep(doc, id).is_some_and(|now| (now - before).abs() > SWEEP_JUMP)
            })
            .map(|&(id, _)| id)
            .collect();
        for &((a, b), before) in &self.sides {
            if tangent_side(doc, a, b).is_some_and(|now| now != before) {
                out.extend([a, b]);
            }
        }
        for &((a, b), c, before) in &self.signs {
            if distance_sign(doc, &c).is_some_and(|now| now != before) {
                out.extend([a, b]);
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Which way a horizontal or vertical distance runs, or which side of its line a point at a
/// distance from it is (see [`Orientation`]); `None` where it's undecided (no distance).
fn distance_sign(doc: &Document, c: &Constraint) -> Option<f64> {
    // (A distance may be to a line's midpoint.)
    let at = |id: EntityId| match doc.get(id)?.kind {
        EntityKind::Line { a, b } => Some((doc.point(a).ok()? + doc.point(b).ok()?) / 2.0),
        _ => doc.point(id).ok(),
    };
    let sign = |v: f64| (v != 0.0).then(|| v.signum());
    match *c {
        Constraint::HorizontalDistance { a, b, .. } => sign(at(b)?.x - at(a)?.x),
        Constraint::VerticalDistance { a, b, .. } => sign(at(b)?.y - at(a)?.y),
        Constraint::PointLineDistance { point, line, .. } => {
            let EntityKind::Line { a, b } = doc.get(line)?.kind else {
                return None;
            };
            let (pa, pb) = (doc.point(a).ok()?, doc.point(b).ok()?);
            sign((pb - pa).perp_dot(at(point)? - pa))
        }
        _ => None,
    }
}

/// An arc's counter-clockwise sweep from start to end, in (0, 2π].
fn arc_sweep(doc: &Document, arc: EntityId) -> Option<f64> {
    let EntityKind::Arc { center, start, end } = doc.get(arc)?.kind else {
        return None;
    };
    let c = doc.point(center).ok()?;
    let (s, e) = (doc.point(start).ok()? - c, doc.point(end).ok()? - c);
    let turn = s.perp_dot(e).atan2(s.dot(e));
    Some(if turn <= 0.0 {
        turn + std::f64::consts::TAU
    } else {
        turn
    })
}

/// The side a tangency touches on (see [`Orientation`]); `None` where it is undecided (a
/// centre on the line, or circles equally near both kinds of contact).
fn tangent_side(doc: &Document, a: EntityId, b: EntityId) -> Option<f64> {
    let center = |id: EntityId| match doc.get(id)?.kind {
        EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. } => {
            doc.point(center).ok()
        }
        _ => None,
    };
    let sign = |v: f64| (v != 0.0).then(|| v.signum());
    if let kind_b @ (EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. }) = doc.get(b)?.kind {
        // Tangent at a shared end: one carries on from the other (−1), or turns back along
        // it in a cusp (+1).
        let t = doc.get(a)?.kind.shared_end(&kind_b)?;
        return sign(doc.end_direction(a, t)?.dot(doc.end_direction(b, t)?));
    }
    match doc.get(a)?.kind {
        EntityKind::Line { a: la, b: lb } => {
            let (pa, pb) = (doc.point(la).ok()?, doc.point(lb).ok()?);
            sign((pb - pa).perp_dot(center(b)? - pa))
        }
        _ => {
            let d = center(a)?.distance(center(b)?);
            let (r1, r2) = (radius_of(doc, a)?, radius_of(doc, b)?);
            sign((d - (r1 - r2).abs()).abs() - (d - (r1 + r2)).abs())
        }
    }
}

/// Radius of a circle or arc.
pub fn radius_of(doc: &Document, curve: EntityId) -> Option<f64> {
    match doc.get(curve)?.kind {
        EntityKind::Circle { radius, .. } => Some(radius),
        EntityKind::Arc { center, start, .. } => {
            Some(doc.point(center).ok()?.distance(doc.point(start).ok()?))
        }
        _ => None,
    }
}

/// The current value of a dimension in the document's geometry (mm, or degrees in [0, 180)).
pub fn measure(doc: &Document, constraint: &Constraint) -> Option<f64> {
    let point = |id| doc.ref_point(id);
    let line = |id| match doc.get(id)?.kind {
        EntityKind::Line { a, b } => Some(point(b)? - point(a)?),
        _ => None,
    };
    match *constraint {
        Constraint::Distance { a, b, .. } => Some(point(a)?.distance(point(b)?)),
        Constraint::Radius { curve, .. } => radius_of(doc, curve),
        Constraint::Diameter { curve, .. } => radius_of(doc, curve).map(|r| 2.0 * r),
        Constraint::HorizontalDistance { a, b, .. } => Some((point(b)?.x - point(a)?.x).abs()),
        Constraint::VerticalDistance { a, b, .. } => Some((point(b)?.y - point(a)?.y).abs()),
        Constraint::PointLineDistance {
            point: p, line: l, ..
        } => {
            let EntityKind::Line { a, b } = doc.get(l)?.kind else {
                return None;
            };
            let (pa, pb, pp) = (point(a)?, point(b)?, point(p)?);
            let d = pb - pa;
            (d.length() > 0.0).then(|| d.perp_dot(pp - pa).abs() / d.length())
        }
        Constraint::Angle { a, b, .. } => {
            let (d1, d2) = (line(a)?, line(b)?);
            let degrees = d1.perp_dot(d2).atan2(d1.dot(d2)).to_degrees();
            Some(degrees.rem_euclid(180.0))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------

struct Component {
    /// Free groups solved here (column order).
    unknowns: Vec<usize>,
    rows: Vec<usize>,
}

/// Split free groups into independent components connected by equations. Rows with no free
/// unknowns form their own (constant) components.
fn components(sys: &System, tol: &Tolerances) -> (Vec<Component>, Vec<usize>) {
    let n = sys.groups.len();
    let mut parent = UnionFind::new(n);
    let mut constant_rows = Vec::new();
    let mut row_groups = Vec::with_capacity(sys.rows.len());
    for (r, row) in sys.rows.iter().enumerate() {
        let mut free: Vec<usize> = row
            .equation
            .params()
            .into_iter()
            .map(|p| sys.group_of[p])
            .filter(|g| !sys.groups[*g].fixed)
            .collect();
        free.sort_unstable();
        free.dedup();
        if free.is_empty() {
            constant_rows.push(r);
        }
        for w in free.windows(2) {
            parent.join(w[0], w[1]);
        }
        row_groups.push(free);
    }
    // A preference joins its parameters too (a line kept to its length joins its ends, so a
    // free end can follow the other), and one that is off makes a component of them where
    // no equation does: with the other end pinned (the landing pass of a drag, say) the
    // free end has no equation left, and would otherwise not be solved for at all. (Only
    // one that is off: a rigid move of a big sketch has thousands that aren't.)
    let value = |p: usize| sys.groups[sys.group_of[p]].value;
    let scale = |k: &[usize; 4]| k.iter().map(|p| value(*p).abs()).fold(1.0, f64::max);
    let mut soft_roots: Vec<usize> = Vec::new();
    for shape in &sys.keep {
        let k = shape.params();
        let off = shape.offset(value).map_or(0.0, |(_, off)| off);
        let free: Vec<usize> = k
            .iter()
            .map(|p| sys.group_of[*p])
            .filter(|g| !sys.groups[*g].fixed)
            .collect();
        for w in free.windows(2) {
            parent.join(w[0], w[1]);
        }
        if let Some(&g) = free.first()
            && off.abs() > tol.length * scale(k)
        {
            soft_roots.push(g);
        }
    }
    let mut by_root: BTreeMap<usize, Component> = BTreeMap::new();
    for g in soft_roots {
        by_root.entry(parent.find(g)).or_insert_with(|| Component {
            unknowns: Vec::new(),
            rows: Vec::new(),
        });
    }
    for (r, free) in row_groups.iter().enumerate() {
        if let Some(&g) = free.first() {
            let root = parent.find(g);
            by_root
                .entry(root)
                .or_insert_with(|| Component {
                    unknowns: Vec::new(),
                    rows: Vec::new(),
                })
                .rows
                .push(r);
        }
    }
    for g in 0..n {
        if !sys.groups[g].fixed
            && let Some(c) = by_root.get_mut(&parent.find(g))
        {
            c.unknowns.push(g);
        }
    }
    let mut comps: Vec<Component> = by_root.into_values().collect();
    comps.sort_by_key(|c| c.unknowns.first().copied());
    (comps, constant_rows)
}

/// Residual tolerance for one row, given the magnitude of the coordinates involved.
fn row_tolerance(sys: &System, r: usize, values: &[f64], tol: &Tolerances) -> f64 {
    let eq = &sys.rows[r].equation;
    if eq.is_angular() {
        tol.angle
    } else {
        let scale = eq
            .params()
            .into_iter()
            .map(|p| values[sys.group_of[p]].abs())
            .fold(1.0, f64::max);
        tol.length * scale
    }
}

/// Evaluate rows at `values` (group values), returning residuals and sparse Jacobian rows over
/// component columns.
fn evaluate(
    sys: &System,
    rows: &[usize],
    values: &[f64],
    col_of: &HashMap<usize, usize>,
) -> (Vec<f64>, Vec<Vec<(usize, f64)>>) {
    let val = |p: usize| values[sys.group_of[p]];
    let mut residuals = Vec::with_capacity(rows.len());
    let mut jacobian = Vec::with_capacity(rows.len());
    let mut grad = Vec::new();
    for &r in rows {
        grad.clear();
        residuals.push(sys.rows[r].equation.eval(&val, &mut grad));
        let mut row: Vec<(usize, f64)> = Vec::with_capacity(grad.len());
        for &(p, g) in &grad {
            if let Some(&c) = col_of.get(&sys.group_of[p]) {
                match row.iter_mut().find(|(cc, _)| *cc == c) {
                    Some(entry) => entry.1 += g,
                    None => row.push((c, g)),
                }
            }
        }
        jacobian.push(row);
    }
    (residuals, jacobian)
}

/// A row whose gradient is this small next to the largest is left unscaled by
/// [`scaled_rows`]: it is round-off (an angle between lines whose directions substitution
/// has pinned, say), and scaled up to a unit gradient its residual, round-off too, would
/// be an equation of its own.
const NEGLIGIBLE_ROW: f64 = 1e-9;

/// Per row, the factor that scales its gradient to unit length (one, for a row with next
/// to no gradient: it has nothing to say about the step).
fn row_scales(jac: &[Vec<(usize, f64)>]) -> Vec<f64> {
    let norms: Vec<f64> = jac
        .iter()
        .map(|row| row.iter().map(|(_, g)| g * g).sum::<f64>().sqrt())
        .collect();
    let largest = norms.iter().copied().fold(0.0, f64::max);
    norms
        .into_iter()
        .map(|norm| {
            if norm > NEGLIGIBLE_ROW * largest {
                1.0 / norm
            } else {
                1.0
            }
        })
        .collect()
}

/// The Jacobian rows and residuals scaled by [`row_scales`].
fn scaled_rows(
    jac: &[Vec<(usize, f64)>],
    res: &[f64],
    scales: &[f64],
) -> (Vec<Vec<(usize, f64)>>, Vec<f64>) {
    let rows = jac
        .iter()
        .zip(scales)
        .map(|(row, scale)| row.iter().map(|&(c, g)| (c, g * scale)).collect())
        .collect();
    let residuals = res.iter().zip(scales).map(|(f, scale)| f * scale).collect();
    (rows, residuals)
}

/// Squared length of the residuals scaled by [`row_scales`].
fn scaled_norm2(res: &[f64], scales: &[f64]) -> f64 {
    res.iter().zip(scales).map(|(f, s)| (f * s).powi(2)).sum()
}

struct ComponentOutcome {
    converged: bool,
    iterations: usize,
    /// Steps on the way to the relations that its linearisation badly mispredicted (or that
    /// were refused): see [`SolveReport::wandered`].
    wandered: usize,
}

/// Damped weighted minimum-norm Gauss–Newton on one component.
///
/// `dragging` is a drag's solve (the landing passes too, where the dragged parameters are
/// pinned): there the solve goes on until nothing moves, not only until the rows hold.
fn newton(
    sys: &System,
    comp: &Component,
    values: &mut [f64],
    tol: &Tolerances,
    dragging: bool,
    across: f64,
) -> ComponentOutcome {
    let n = comp.unknowns.len();
    let m = comp.rows.len();
    let col_of: HashMap<usize, usize> = comp
        .unknowns
        .iter()
        .enumerate()
        .map(|(c, g)| (*g, c))
        .collect();
    let mobility: Vec<f64> = comp
        .unknowns
        .iter()
        .map(|g| sys.groups[*g].mobility)
        .collect();
    let converged = |res: &[f64], values: &[f64]| {
        res.iter()
            .zip(&comp.rows)
            .all(|(f, r)| f.abs() <= row_tolerance(sys, *r, values, tol))
    };

    let (mut res, mut jac) = evaluate(sys, &comp.rows, values, &col_of);
    // Normal matrix N = JᵀJ over the unknowns. Its sparsity is fixed for the component (two
    // unknowns interact when an equation uses both), so build the bandwidth-reduced structure
    // once. Working over unknowns rather than equations keeps a widely shared unknown a single
    // "hub" node instead of coupling every equation that uses it.
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for row in &jac {
        for &(c1, _) in row {
            for &(c2, _) in row {
                if c1 != c2 {
                    neighbors[c1].push(c2);
                }
            }
        }
    }
    // The shapes kept (see below) couple their parameters too.
    for shape in &sys.keep {
        let cols: Vec<usize> = shape
            .params()
            .iter()
            .filter_map(|p| col_of.get(&sys.group_of[*p]).copied())
            .collect();
        for &c1 in &cols {
            for &c2 in &cols {
                if c1 != c2 {
                    neighbors[c1].push(c2);
                }
            }
        }
    }
    for ns in &mut neighbors {
        ns.sort_unstable();
        ns.dedup();
    }
    let mut normal = Skyline::new(n, &neighbors);
    let _ = m;
    // The columns of each point (x, then y) and of each radius, for the scale of the
    // regularisation: the largest diagonal of JᵀJ averaged over a point's two columns. (One
    // column's diagonal alone would turn with the sketch: a row's gradient at a point is a
    // vector, and the diagonals are its components squared. The regularisation would then
    // differ for the same sketch turned, and with it, slightly, the result; #94.)
    let mut scales: BTreeMap<(EntityId, bool), [Option<usize>; 2]> = BTreeMap::new();
    let mut radii_cols: Vec<usize> = Vec::new();
    for (c, g) in comp.unknowns.iter().enumerate() {
        for &i in &sys.groups[*g].members {
            match sys.params[i] {
                Param::X(id) => scales.entry((id, false)).or_default()[0] = Some(c),
                Param::Y(id) => scales.entry((id, false)).or_default()[1] = Some(c),
                Param::MidX(id) => scales.entry((id, true)).or_default()[0] = Some(c),
                Param::MidY(id) => scales.entry((id, true)).or_default()[1] = Some(c),
                Param::Radius(_) => radii_cols.push(c),
            }
        }
    }
    let scales: Vec<[Option<usize>; 2]> = scales
        .into_values()
        .chain(radii_cols.into_iter().map(|c| [Some(c), None]))
        .collect();

    let mut lambda = 0.0f64;
    let mut wandered = 0;
    // Whether the soft preferences have settled: the rows holding isn't the end while a
    // preference is still closing (each step takes most, not all, of what is left of it
    // within the relations' null space), or the result would depend on how many steps the
    // relations happened to take, so on the tolerance and with it on where the sketch is,
    // and what a preference keeps would creep a little every step of a drag (#94). So once
    // the rows hold, keep stepping until a step moves nothing (at the tolerance). A
    // preference the relations rule out has no step left in the null space, so it stops at
    // once; without preferences there is nothing to settle.
    let mut settled = sys.keep.is_empty();
    let mut settling = 0;
    // How far the last step moved anything. A drag's solve isn't over while a step still
    // moves something, even with the rows holding: where the drag can't land, the dragged
    // give way and converge only linearly, and the rows hold long before that has settled;
    // the solve would stop wherever the tolerance let it, so at a point that depended on
    // the sketch's size and place (#104).
    let mut last_moved = f64::INFINITY;
    // Levenberg–Marquardt damping, raised by `nu` on a refused step and lowered by how well
    // the step did what its linearisation promised (Nielsen).
    let mut nu = 2.0f64;
    for iteration in 0..MAX_ITERATIONS {
        let rows_hold = converged(&res, values);
        let at_rest = !dragging || last_moved <= still_at(values, tol);
        if rows_hold {
            if (settled && at_rest) || settling >= SETTLE_PATIENCE {
                return ComponentOutcome {
                    converged: true,
                    iterations: iteration,
                    wandered,
                };
            }
            settling += 1;
        }
        // The shapes kept: arcs' radii, lines' lengths and directions, each as how far off
        // it is and the gradient of that over its four parameters.
        let soft_terms: Vec<(&[usize; 4], [f64; 4], f64)> = sys
            .keep
            .iter()
            .filter_map(|shape| {
                let (gradient, off) = shape.offset(|p| values[sys.group_of[p]])?;
                Some((shape.params(), gradient, off))
            })
            .collect();
        // What the preferences have left to do, at the tolerance: nothing where the rows
        // hold and none is off (a move that carries everything along as it is), and then
        // there is nothing to settle and no step to build.
        let (lo, hi) = values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let still = tol.length * hi.abs().max(lo.abs()).max(1.0);
        if soft_terms.iter().all(|(_, _, off)| off.abs() <= still) {
            settled = true;
            if rows_hold && at_rest {
                return ComponentOutcome {
                    converged: true,
                    iterations: iteration,
                    wandered,
                };
            }
        }
        // N = JᵀJ and the right-hand side −JᵀF, with every row scaled to a unit gradient: an
        // angle's residual is a sine and its gradient goes as one over the lines' lengths,
        // a distance's residual is a length and its gradient a unit vector, so unscaled the
        // rows would be weighted against each other by the size of the sketch wherever the
        // step isn't exact (its regularisation isn't negligible for heavy unknowns, every
        // unknown of a dragged part among them), and the same drag would come out
        // differently on the same sketch at another size (#94). Scaled, every row counts as
        // the distance it is off by.
        let row_scale = row_scales(&jac);
        let (scaled_jac, scaled_res) = scaled_rows(&jac, &res, &row_scale);
        normal.clear();
        let mut rhs = vec![Dd::default(); n];
        // The preferences' own right-hand side, solved for apart (see below).
        let mut soft_rhs = vec![Dd::default(); n];
        for (row, f) in scaled_jac.iter().zip(&scaled_res) {
            for (k, &(c1, g1)) in row.iter().enumerate() {
                rhs[c1] = rhs[c1].add_product(-g1, *f);
                for &(c2, g2) in &row[..=k] {
                    normal.add_product(c1, c2, g1, g2);
                }
            }
        }
        let max_diag = scales
            .iter()
            .map(|cols| {
                let cols: Vec<usize> = cols.iter().flatten().copied().collect();
                cols.iter().map(|&c| normal.diagonal(c)).sum::<f64>() / cols.len() as f64
            })
            .fold(0.0, f64::max);
        // (Without equations, preferences alone: their gradients are unit vectors.)
        let max_diag = if max_diag > 0.0 { max_diag } else { 1.0 };
        // μ·M⁻¹ picks the smallest weighted change among the solutions (and keeps
        // under-constrained systems solvable); λ adds Levenberg–Marquardt damping.
        let mu = 1e-10 * max_diag;
        // Part of that change: the shapes are kept where they can be. Where a move could go
        // either way (a concentric round's lines follow it, or the round shrinks), being
        // off a radius, length or direction costs as much as moving a point that far,
        // KEEP_SHAPE times over. A round that only its points pin down would otherwise give
        // way a little every step of a drag, until it collapses. (A soft pull: μ is tiny, so
        // it only chooses among the solutions.)
        for &(k, gradient, off) in &soft_terms {
            let mut grad: Vec<(usize, f64)> = Vec::with_capacity(4);
            for (p, g) in k.iter().zip(gradient) {
                if let Some(&c) = col_of.get(&sys.group_of[*p]) {
                    match grad.iter_mut().find(|(cc, _)| *cc == c) {
                        Some(entry) => entry.1 += g,
                        None => grad.push((c, g)),
                    }
                }
            }
            for (i, &(c1, g1)) in grad.iter().enumerate() {
                soft_rhs[c1] = soft_rhs[c1].add_product(-mu * KEEP_SHAPE * g1, off);
                for &(c2, g2) in &grad[..=i] {
                    normal.add_product(c1, c2, mu * KEEP_SHAPE * g1, g2);
                }
            }
        }
        let soft = soft_rhs.iter().any(|r| r.hi != 0.0);

        // The relations' step, and the preferences' apart from it. The preferences choose
        // among the solutions, so their step is kept to the relations' null space: solved
        // for on its own, the part of it that the relations feel (what a preference they
        // rule out pulls against them, a first-order change to the rows of its weight
        // times its offset) is taken out again. Left in, it would hold the rows off by that
        // much: not within the tolerance for a far-off preference, so the solve never ends
        // on its own, or within it for a sketch far from the origin, where the tolerance
        // is looser, so the same drag would come out differently there (#94).
        let mut solved = None;
        for _ in 0..30 {
            let shift: Vec<f64> = mobility.iter().map(|m| (mu + lambda) / m).collect();
            if let Some(factor) = normal.factor(&shift) {
                let step = factor.solve_refined(&rhs);
                let soft_step = soft.then(|| {
                    let mut soft_step = factor.solve_refined(&soft_rhs);
                    // Not the dragged or carried: they are where the user put them. A
                    // preference only they could meet (through the relations) would
                    // otherwise inch them along, heavy as they are, for thousands of
                    // iterations, and the solve would stop part way, wherever its patience
                    // ran out (#104).
                    for (s, m) in soft_step.iter_mut().zip(&mobility) {
                        if *m <= system::CARRIED_MOBILITY {
                            *s = 0.0;
                        }
                    }
                    // Until nothing is left of it but round-off: the matrix regularises the
                    // heavy unknowns (the dragged, at 1e-4 of the rows) as well as the null
                    // space, so one pass leaves that much of what the relations feel, and
                    // each further pass that much of what the last left.
                    let size = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let whole = size(&soft_step);
                    for _ in 0..FELT_PASSES {
                        let mut felt = vec![Dd::default(); n];
                        for row in &scaled_jac {
                            let change = row.iter().fold(Dd::default(), |sum, &(c, g)| {
                                sum.add_product(g, soft_step[c])
                            });
                            for &(c, g) in row {
                                felt[c] = felt[c].add_product(g, change.hi).add(g * change.lo);
                            }
                        }
                        let felt = factor.solve_refined(&felt);
                        for (s, f) in soft_step.iter_mut().zip(&felt) {
                            *s -= f;
                        }
                        if size(&felt) <= FELT_ROUND_OFF * whole {
                            break;
                        }
                    }
                    soft_step
                });
                solved = Some((step, soft_step));
                break;
            }
            lambda = if lambda == 0.0 {
                1e-9 * max_diag
            } else {
                lambda * 10.0
            };
        }
        // Limit the step so a single iteration can't fling geometry onto another branch: no
        // further than the drawing is across. (Not the spread of this component's own
        // unknowns: a lone unknown, such as the free end of a dimension from a fixed point,
        // has none, and would crawl a millimetre at a time.)
        let Some((mut delta, soft_delta)) = solved else {
            break;
        };
        let max_step = across;
        // (How far a point moves, not a coordinate: the same however the drawing is turned.)
        let limit = |delta: &mut Vec<f64>| {
            let biggest = scales
                .iter()
                .map(|cols| {
                    cols.iter()
                        .flatten()
                        .map(|&c| delta[c] * delta[c])
                        .sum::<f64>()
                        .sqrt()
                })
                .fold(0.0f64, f64::max);
            if biggest > max_step {
                let k = max_step / biggest;
                delta.iter_mut().for_each(|d| *d *= k);
            }
            biggest.min(max_step)
        };
        let rows_moved = limit(&mut delta);
        // (A step cut short is one its linearisation got badly wrong too.)
        if rows_moved >= max_step && !converged(&res, values) {
            wandered += 1;
        }

        let mut trial = values.to_vec();
        for (c, g) in comp.unknowns.iter().enumerate() {
            trial[*g] += delta[c];
        }
        let (trial_res, trial_jac) = evaluate(sys, &comp.rows, &trial, &col_of);
        // (Judged in the step's own scaling of the rows.)
        let before = scaled_norm2(&res, &row_scale);
        let after = scaled_norm2(&trial_res, &row_scale);
        if after < before || converged(&trial_res, &trial) {
            // How much of the drop the linearisation promised the step delivered.
            let promised = before
                - scaled_jac
                    .iter()
                    .zip(&scaled_res)
                    .map(|(row, f)| {
                        (f + row.iter().map(|&(c, g)| g * delta[c]).sum::<f64>()).powi(2)
                    })
                    .sum::<f64>();
            let gain = if promised > 0.0 {
                (before - after) / promised
            } else {
                1.0
            };
            if !rows_hold && gain < WANDERING_GAIN {
                wandered += 1;
            }
            last_moved = rows_moved;
            values.copy_from_slice(&trial);
            res = trial_res;
            jac = trial_jac;
            // Then the preferences' step, which the relations don't feel to first order:
            // what it leaves them off by (second order) the next step mends. Judged with
            // the relations' step it would be refused whenever that is more than the rows
            // are off by, and a damped step taken instead, over and over. It may not cost
            // the rows what this step gained, though (far from the relations a big
            // preference step can throw them right off): where it would, it is halved.
            if let Some(mut soft_delta) = soft_delta {
                // (Settled once nothing moves at all: a relations' step that still moves
                // something changes what the preferences have left to do.)
                let mut moved = limit(&mut soft_delta);
                for _ in 0..4 {
                    let mut trial = values.to_vec();
                    for (c, g) in comp.unknowns.iter().enumerate() {
                        trial[*g] += soft_delta[c];
                    }
                    let (trial_res, trial_jac) = evaluate(sys, &comp.rows, &trial, &col_of);
                    // Or what it leaves them off by is second order, a fraction of the step
                    // (it went along a curved relation, which the next step mends). Judged
                    // only against the tolerance, a step along a curved relation that already
                    // holds would be refused or not by where the sketch is (the tolerance
                    // goes with the coordinates), and the preference crawl or close
                    // accordingly: the same drag would end differently there (#103).
                    let after = scaled_norm2(&trial_res, &row_scale);
                    let gained = after < before
                        || after.sqrt() <= before.sqrt() + SECOND_ORDER * moved
                        || converged(&trial_res, &trial);
                    if gained {
                        values.copy_from_slice(&trial);
                        res = trial_res;
                        jac = trial_jac;
                        break;
                    }
                    soft_delta.iter_mut().for_each(|d| *d *= 0.5);
                    moved *= 0.5;
                }
                settled = moved.max(rows_moved) <= still;
                last_moved = last_moved.max(moved);
            }
            lambda *= (1.0 - (2.0 * gain - 1.0).powi(3)).max(1.0 / 3.0);
            nu = 2.0;
            if lambda < 1e-12 * max_diag {
                lambda = 0.0;
            }
        } else {
            if !rows_hold {
                wandered += 1;
            }
            lambda = lambda.max(1e-6 * max_diag) * nu;
            nu *= 2.0;
            if lambda > 1e12 * max_diag {
                break;
            }
        }
    }
    ComponentOutcome {
        converged: converged(&res, values),
        iterations: MAX_ITERATIONS,
        wandered,
    }
}

/// How far the drawing is across, for the step limit in [`newton`]: twice its farthest point
/// from its middle, which is the same however the drawing is turned or wherever it is. (The
/// spread of the coordinates, x and y together, grows as the drawing moves away from the
/// origin, so the same step would be cut short at one place and not another, and the same
/// drag land elsewhere: #103.) Infinite for a drawing of one point.
fn drawing_across(sys: &System, values: &[f64]) -> f64 {
    let mut points: BTreeMap<EntityId, [Option<f64>; 2]> = BTreeMap::new();
    for (i, p) in sys.params.iter().enumerate() {
        match *p {
            Param::X(id) => points.entry(id).or_default()[0] = Some(values[sys.group_of[i]]),
            Param::Y(id) => points.entry(id).or_default()[1] = Some(values[sys.group_of[i]]),
            _ => {}
        }
    }
    let points: Vec<DVec2> = points
        .into_values()
        .filter_map(|[x, y]| Some(DVec2::new(x?, y?)))
        .collect();
    let middle = points.iter().sum::<DVec2>() / points.len().max(1) as f64;
    let size = points
        .iter()
        .map(|p| p.distance(middle))
        .fold(0.0, f64::max);
    if size > 0.0 {
        2.0 * size
    } else {
        f64::INFINITY
    }
}

/// A move below this, at the scale of the coordinates, is no move at all (the length
/// tolerance, as `row_tolerance` takes it).
fn still_at(values: &[f64], tol: &Tolerances) -> f64 {
    let (lo, hi) = values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    tol.length * hi.abs().max(lo.abs()).max(1.0)
}

fn run(sys: &mut System, options: &SolveOptions, iterate: bool) -> SolveReport {
    let tol = &options.tolerances;
    let mut report = SolveReport {
        conflicts: sys.conflicts.clone(),
        redundant: sys.redundant.clone(),
        ..Default::default()
    };
    let (comps, constant_rows) = components(sys, tol);
    let mut values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let mut failed_rows: Vec<usize> = Vec::new();

    for &r in &constant_rows {
        let res = sys.rows[r]
            .equation
            .eval(&|p| values[sys.group_of[p]], &mut Vec::new());
        if res.abs() > row_tolerance(sys, r, &values, tol) {
            failed_rows.push(r);
        } else {
            // Satisfied, but everything it involves is pinned: it adds nothing.
            report.dependent_equations += 1;
        }
    }
    let across = drawing_across(sys, &values);
    for comp in &comps {
        if iterate {
            let outcome = newton(
                sys,
                comp,
                &mut values,
                tol,
                !options.dragged.is_empty(),
                across,
            );
            report.iterations = report.iterations.max(outcome.iterations);
            report.wandered = report.wandered.max(outcome.wandered);
            if !outcome.converged {
                failed_rows.extend(&comp.rows);
            }
        } else {
            let col_of = HashMap::new();
            let (res, _) = evaluate(sys, &comp.rows, &values, &col_of);
            for (f, r) in res.iter().zip(&comp.rows) {
                if f.abs() > row_tolerance(sys, *r, &values, tol) {
                    failed_rows.extend(&comp.rows);
                    break;
                }
            }
        }
    }
    for (g, v) in values.iter().enumerate() {
        sys.groups[g].value = *v;
    }
    for &r in &failed_rows {
        if let Some(cid) = sys.rows[r].source
            && !report.conflicts.contains(&cid)
        {
            report.conflicts.push(cid);
        }
    }
    report.conflicts.sort();
    report.converged = failed_rows.is_empty() && sys.conflicts.is_empty();

    if options.analyze {
        analyze_dof(sys, &comps, &values, &failed_rows, &mut report);
    }
    report
}

/// Reduced row echelon form of the equilibrated Jacobian, with its pivot columns. Used as the
/// reference for the sparse QR analysis in tests.
#[cfg(test)]
fn dense_rref(jac: &[Vec<(usize, f64)>], n: usize) -> (Vec<usize>, Matrix) {
    let eq = equilibrate(jac, n);
    let mut j = Matrix::zeros(eq.len(), n);
    for (i, row) in eq.iter().enumerate() {
        for &(c, g) in row {
            *j.at_mut(i, c) += g;
        }
    }
    let pivots = rref(&mut j, 1e-9);
    (pivots, j)
}

/// Rank of the sparse Jacobian (rows over `n` columns) and, per column, whether that unknown
/// can still move (has a component in the null space). Uses Givens QR, which costs about
/// O(n · b²) for envelope width b.
fn sparse_rank(jac: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    qr_rank(&equilibrate(jac, n), n)
}

/// Prepare a Jacobian for rank decisions: drop negligible entries, scale rows to unit length,
/// then columns to unit length. Scaling changes neither the rank nor which unknowns are free,
/// but it makes one tolerance meaningful for sketches mixing tiny and huge features.
///
/// An entry below [`NEGLIGIBLE`] of its row's largest is dropped: round-off (e.g. gradients of
/// two members of one substitution group that cancel), or geometry that is all but degenerate
/// (two points a few nanometres off level, an angle of 1e-8 rad). Kept, column scaling would
/// blow it up to unit size and rank decisions would turn on rounding.
fn equilibrate(jac: &[Vec<(usize, f64)>], n: usize) -> Vec<Vec<(usize, f64)>> {
    const NEGLIGIBLE: f64 = 1e-7;
    let mut rows: Vec<Vec<(usize, f64)>> = jac
        .iter()
        .map(|row| {
            let max = row.iter().fold(0.0f64, |m, (_, g)| m.max(g.abs()));
            let kept: Vec<(usize, f64)> = row
                .iter()
                .copied()
                .filter(|(_, g)| g.abs() > NEGLIGIBLE * max)
                .collect();
            let norm = kept.iter().map(|(_, g)| g * g).sum::<f64>().sqrt();
            kept.into_iter().map(|(c, g)| (c, g / norm)).collect()
        })
        .collect();
    let mut col_norm = vec![0.0f64; n];
    for row in &rows {
        for &(c, g) in row {
            col_norm[c] += g * g;
        }
    }
    for row in &mut rows {
        for (c, g) in row.iter_mut() {
            *g /= col_norm[*c].sqrt();
        }
    }
    rows
}

/// Rank analysis of each component's Jacobian at the solution.
fn analyze_dof(
    sys: &System,
    comps: &[Component],
    values: &[f64],
    failed_rows: &[usize],
    report: &mut SolveReport,
) {
    // Groups that are pinned or determined by equations.
    let mut determined: Vec<bool> = sys.groups.iter().map(|g| g.fixed).collect();
    let mut dof = 0;
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    dof += (0..sys.groups.len())
        .filter(|g| !sys.groups[*g].fixed && !in_component.contains(g))
        .count();

    for comp in comps {
        let n = comp.unknowns.len();
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = evaluate(sys, &comp.rows, values, &col_of);
        let (rank, movable) = sparse_rank(&jac, n);
        dof += n - rank;
        report.dependent_equations += comp.rows.len().saturating_sub(rank);
        for (c, g) in comp.unknowns.iter().enumerate() {
            if !movable[c] {
                determined[*g] = true;
            }
        }
    }
    report.dof = Some(dof);

    let param_determined = |p: Param| {
        sys.param_index
            .get(&p)
            .is_some_and(|&i| determined[sys.group_of[i]])
    };
    let point_full =
        |id: EntityId| param_determined(Param::X(id)) && param_determined(Param::Y(id));
    // Entities touched by failing equations are over-defined.
    let mut failing_params: HashSet<usize> = HashSet::new();
    for &r in failed_rows {
        for p in sys.rows[r].equation.params() {
            failing_params.insert(p);
        }
    }
    let failing = |p: Param| {
        sys.param_index
            .get(&p)
            .is_some_and(|i| failing_params.contains(i))
    };
    for (id, entity) in &sys.entities {
        let id = *id;
        let (full, over) = match entity {
            EntityShape::Point => (
                point_full(id),
                failing(Param::X(id)) || failing(Param::Y(id)),
            ),
            EntityShape::Curve(points, radius) => {
                let full = points.iter().all(|p| point_full(*p))
                    && radius.is_none_or(|r| param_determined(Param::Radius(r)));
                let over = points
                    .iter()
                    .any(|p| failing(Param::X(*p)) || failing(Param::Y(*p)))
                    || radius.is_some_and(|r| failing(Param::Radius(r)));
                (full, over)
            }
        };
        report.definitions.insert(
            id,
            if over {
                Definition::Over
            } else if full {
                Definition::Full
            } else {
                Definition::Under
            },
        );
    }
}

/// How a point can still move with every constraint kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Freedom {
    /// Free along one direction (unit vector; a rotation shows as the tangent direction).
    Line(DVec2),
    /// Free in every direction.
    Plane,
}

/// Components larger than this are skipped by [`free_directions`] (dense analysis).
const FREEDOM_LIMIT: usize = 400;

/// The directions in which each under-defined point can move, from the null space of the
/// constraint Jacobian. Fully defined points are omitted, as are points in very large
/// components (see [`FREEDOM_LIMIT`]).
#[cfg(test)]
pub fn free_directions(doc: &Document) -> HashMap<EntityId, Freedom> {
    free_directions_with(doc, None)
}

/// [`free_directions`], reading relations through `lowering`.
pub fn free_directions_with(
    doc: &Document,
    lowering: Option<&Lowering>,
) -> HashMap<EntityId, Freedom> {
    let options = SolveOptions::default();
    let sys = System::build(doc, &[], &options.tolerances, lowering);
    let (comps, _) = components(&sys, &options.tolerances);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    // Null-space basis vectors, as group → component, indexed by the groups they touch.
    let mut basis: Vec<BTreeMap<usize, f64>> = Vec::new();
    let mut skipped: HashSet<usize> = HashSet::new();
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    for g in 0..sys.groups.len() {
        if !sys.groups[g].fixed && !in_component.contains(&g) {
            basis.push(BTreeMap::from([(g, 1.0)]));
        }
    }
    for comp in &comps {
        let n = comp.unknowns.len();
        if n > FREEDOM_LIMIT {
            skipped.extend(&comp.unknowns);
            continue;
        }
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = evaluate(&sys, &comp.rows, &values, &col_of);
        // Equilibrate like the rank analysis, keeping the column scales to undo them.
        let rows: Vec<Vec<(usize, f64)>> = jac
            .iter()
            .map(|row| {
                let max = row.iter().fold(0.0f64, |m, (_, g)| m.max(g.abs()));
                let kept: Vec<_> = row
                    .iter()
                    .copied()
                    .filter(|(_, g)| g.abs() > 1e-12 * max)
                    .collect();
                let norm = kept.iter().map(|(_, g)| g * g).sum::<f64>().sqrt();
                kept.into_iter().map(|(c, g)| (c, g / norm)).collect()
            })
            .collect();
        let mut col_norm = vec![0.0f64; n];
        for row in &rows {
            for &(c, g) in row {
                col_norm[c] += g * g;
            }
        }
        let col_norm: Vec<f64> = col_norm
            .iter()
            .map(|s| if *s > 0.0 { s.sqrt() } else { 1.0 })
            .collect();
        let mut m = Matrix::zeros(rows.len(), n);
        for (i, row) in rows.iter().enumerate() {
            for &(c, g) in row {
                *m.at_mut(i, c) += g / col_norm[c];
            }
        }
        let pivots = rref(&mut m, 1e-9);
        let pivot_row: BTreeMap<usize, usize> =
            pivots.iter().enumerate().map(|(r, c)| (*c, r)).collect();
        for free in (0..n).filter(|c| !pivot_row.contains_key(c)) {
            // x_free = 1, x_pivot = -R[row, free]; undo the column scaling.
            let mut v = BTreeMap::new();
            v.insert(comp.unknowns[free], 1.0 / col_norm[free]);
            for (&pc, &r) in &pivot_row {
                let x = -m.at(r, free);
                if x != 0.0 {
                    v.insert(comp.unknowns[pc], x / col_norm[pc]);
                }
            }
            basis.push(v);
        }
    }
    let mut by_group: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, v) in basis.iter().enumerate() {
        for g in v.keys() {
            by_group.entry(*g).or_default().push(i);
        }
    }
    let group = |p: Param| sys.param_index.get(&p).map(|&i| sys.group_of[i]);
    let mut out = HashMap::new();
    for (id, entity) in doc.iter() {
        if !entity.kind.is_point() {
            continue;
        }
        let (Some(gx), Some(gy)) = (group(Param::X(id)), group(Param::Y(id))) else {
            continue;
        };
        if skipped.contains(&gx) || skipped.contains(&gy) {
            continue;
        }
        let mut vectors: Vec<usize> = by_group
            .get(&gx)
            .into_iter()
            .chain(by_group.get(&gy))
            .flatten()
            .copied()
            .collect();
        vectors.sort_unstable();
        vectors.dedup();
        // A = M Mᵀ for the 2 × k matrix of motions; its eigenvalues give the rank.
        let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
        for i in vectors {
            let mx = basis[i].get(&gx).copied().unwrap_or(0.0);
            let my = basis[i].get(&gy).copied().unwrap_or(0.0);
            let norm = basis[i].values().map(|x| x * x).sum::<f64>();
            let (mx, my) = (mx / norm.sqrt(), my / norm.sqrt());
            a += mx * mx;
            b += mx * my;
            c += my * my;
        }
        let trace = a + c;
        if trace < 1e-12 {
            continue;
        }
        let disc = (((a - c) / 2.0).powi(2) + b * b).sqrt();
        let (l1, l2) = (trace / 2.0 + disc, trace / 2.0 - disc);
        let freedom = if l2 > 1e-6 * l1 {
            Freedom::Plane
        } else {
            // Eigenvector of the larger eigenvalue.
            let dir = if b.abs() > 1e-15 {
                DVec2::new(l1 - c, b)
            } else if a >= c {
                DVec2::X
            } else {
                DVec2::Y
            };
            Freedom::Line(dir.normalize())
        };
        out.insert(id, freedom);
    }
    out
}

/// Write the solved values into the document, unless any of them isn't finite: then write
/// nothing and return false.
fn write_back(doc: &mut Document, sys: &System) -> bool {
    if (0..sys.params.len()).any(|i| !sys.value(i).is_finite()) {
        return false;
    }
    let mut points: HashMap<EntityId, DVec2> = HashMap::new();
    for (i, param) in sys.params.iter().enumerate() {
        let v = sys.value(i);
        match *param {
            Param::X(id) => points.entry(id).or_insert(DVec2::ZERO).x = v,
            Param::Y(id) => points.entry(id).or_insert(DVec2::ZERO).y = v,
            Param::MidX(_) | Param::MidY(_) => {}
            Param::Radius(id) => {
                if let Some(EntityKind::Circle { radius, .. }) = doc.get(id).map(|e| e.kind)
                    && radius != v
                {
                    let _ = doc.set_radius(id, v);
                }
            }
        }
    }
    let mut ids: Vec<_> = points.into_iter().collect();
    ids.sort_by_key(|(id, _)| *id);
    for (id, at) in ids {
        if doc.point(id).is_ok_and(|p| p != at) {
            let _ = doc.set_point(id, at);
        }
    }
    true
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn dense_rref_for_test(jac: &[Vec<(usize, f64)>], n: usize) -> (Vec<usize>, Matrix) {
    dense_rref(jac, n)
}

#[cfg(test)]
pub(crate) fn sparse_rank_for_test(jac: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    sparse_rank(jac, n)
}
