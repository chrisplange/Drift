//! Turning a document into a system of equations over unknowns.
//!
//! Every point contributes `x` and `y` parameters and every circle a radius. Equality-style
//! constraints (coincident, horizontal, vertical) don't become equations; they merge
//! parameters into groups (substitution), which shrinks the system and makes those relations
//! exact. Fixed values pin a group. The remaining constraints become [`Equation`]s, and
//! unknowns are split into independent components that are solved separately.

use std::collections::{HashMap, HashSet};

use drift_core::polygon::UnionFind;
use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};

use crate::Param;
use crate::equations::{Equation, Length, Offset};
use crate::lowering::{Lowered, Lowering};

/// The stand-in id of a lowering's extras while the system is built (their rows report as
/// no relation's).
const EXTRA: ConstraintId = ConstraintId(u32::MAX);

#[derive(Clone, Debug)]
pub struct Group {
    pub value: f64,
    pub fixed: bool,
    /// Inverse weight: how freely this group moves in a minimum-norm step.
    pub mobility: f64,
    pub members: Vec<usize>,
}

/// What an entity is made of, for reporting its definition state.
#[derive(Clone, Debug)]
pub enum EntityShape {
    Point,
    /// Defining points, and the entity itself if it has a radius parameter.
    Curve(Vec<EntityId>, Option<EntityId>),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub equation: Equation,
    /// The constraint the equation came from; `None` for internal equations (arc radius).
    pub source: Option<ConstraintId>,
    /// For an internal equation (no `source`), the entity whose own geometry it is: an arc
    /// (its end on the circle through its start), a line (its midpoint), an ellipse (its
    /// axes square), an elliptical arc (its end on its ellipse), or for a relation that comes
    /// with the geometry (a lowering's extra: a wall's end on the wall it meets) the entity
    /// of its first parameter. Stable from solve to solve, so what a solve leaves broken
    /// can be compared with what was broken before ([`crate::SolveReport::broken`]).
    pub owner: Option<EntityId>,
}

pub struct System {
    pub params: Vec<Param>,
    pub param_index: HashMap<Param, usize>,
    /// Param index → group index.
    pub group_of: Vec<usize>,
    pub groups: Vec<Group>,
    pub rows: Vec<Row>,
    /// Constraints contradicted by fixed values or by each other during substitution.
    pub conflicts: Vec<ConstraintId>,
    /// Constraints entirely implied by others during substitution.
    pub redundant: Vec<ConstraintId>,
    pub entities: Vec<(EntityId, EntityShape)>,
    /// The shapes to keep where the relations leave it open: arcs' radii
    /// ([`crate::SolveOptions::radii`], for arcs neither of whose ends is dragged), lines'
    /// lengths and directions. Not equations, one preference in the minimum-change step,
    /// ranked below the relations (see `newton`).
    pub keep: Vec<Shape>,
    /// How many of [`keep`](Self::keep), from the first, are arcs' radii (the rest are lines'
    /// lengths and directions): a drag's kept radii are held, not only preferred (see
    /// `newton`'s stop).
    pub radii_kept: usize,
    /// What each of [`keep`](Self::keep) weighs against the others in the second stage where
    /// they can't all be met (none past the end: 1; see
    /// [`crate::SolveOptions::length_ranks`]).
    pub keep_rank: Vec<f64>,
    /// Keep shapes as before the audit's PR 6 ([`crate::SolveOptions::shapes_as_before`]).
    pub shapes_as_before: bool,
    /// Per group, the indices of the [`keep`](Self::keep) shapes over it, in order (empty
    /// where `solve` hasn't indexed them: `reach` then looks through every shape).
    pub keep_of_group: Vec<Vec<usize>>,
    /// Per group, the length bar the last solve measured its component against (`run`; zero
    /// for a group no component solved): what a kept shape over it is met to.
    pub bars: Vec<f64>,
    /// Every arc in the system, as its centre, start and end: what the solver measures a
    /// component's size by is the arc as drawn (its ends and points along it), not its centre,
    /// which for a nearly straight arc lies far off the drawing.
    pub arcs: Vec<[EntityId; 3]>,
    /// The points that are only arcs' centres: no line, curve end or other curve's point
    /// uses them. They count towards no component's size (see [`System::arcs`]).
    pub centres_only: HashSet<EntityId>,
}

/// A shape to keep where the relations leave it open (see [`System::keep`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// The distance between two points (an arc's centre and start, a line's ends): x and y
    /// of each, and the distance.
    Distance([usize; 4], f64),
    /// A line's direction: its ends' x and y, the unit direction, and the length it is
    /// measured at (the line's as the solve began; `None`: as before the audit's PR 6, see
    /// [`crate::SolveOptions::shapes_as_before`]).
    Direction([usize; 4], DVec2, Option<f64>),
    /// A circle's radius: its radius parameter, and the radius.
    Radius(usize, f64),
}

impl Shape {
    /// The parameters it is over.
    pub fn params(&self) -> &[usize] {
        match self {
            Shape::Distance(k, _) | Shape::Direction(k, ..) => k,
            Shape::Radius(r, _) => std::slice::from_ref(r),
        }
    }

    /// How long the distance between two points is at `value`, over their x and y `k`: an
    /// arc's radius, a line's length (a direction's offset is a length too: one end off the
    /// line through the other).
    pub fn length(k: &[usize; 4], value: impl Fn(usize) -> f64) -> f64 {
        let [ax, ay, bx, by] = k.map(value);
        (bx - ax).hypot(by - ay)
    }

    /// How long the shape is at `value` ([`length`](Self::length); a circle's radius).
    pub fn size(&self, value: impl Fn(usize) -> f64) -> f64 {
        match self {
            Shape::Distance(k, _) | Shape::Direction(k, ..) => Self::length(k, value),
            Shape::Radius(r, _) => value(*r).abs(),
        }
    }

    /// How far off the shape is at `value`, as a distance, and its gradient over
    /// [`params`](Self::params) (a unit vector per end for a distance). `None` where the shape
    /// has collapsed (no distance to keep). A direction's offset is the sine of the line's turn
    /// at the length it was kept at, `l₀·sin θ`, while it faces the kept way, rising to `2·l₀`
    /// turned end for end: the same at any length up to `l₀`, so no line is kept square by
    /// shrinking it (before, `l·sin θ` was: a line a relation must turn was shrunk to nothing
    /// to meet its direction), and never met reversed. Stretched past `l₀`, the sine is read at [`kept_at`] the length,
    /// which grows as the line does (about as `l`, as `l·sin θ` did): read at `l₀` alone, a
    /// line kept square that no solution lets face its way gained by being stretched without
    /// end along a line it nearly parallels, the turn's sine falling towards the parallel's,
    /// and the preferences chased that to wherever they were stopped (a point flung 238 mm
    /// out on a 49 mm sketch, landing where round-off had it: census case 18502).
    pub fn offset(&self, value: impl Fn(usize) -> f64) -> Option<([f64; 4], f64)> {
        match *self {
            Shape::Distance(k, d0) => {
                let [ax, ay, bx, by] = k.map(value);
                let d = (bx - ax).hypot(by - ay);
                (d > 0.0).then(|| {
                    let (ux, uy) = ((bx - ax) / d, (by - ay) / d);
                    ([-ux, -uy, ux, uy], d - d0)
                })
            }
            Shape::Direction(k, u, l0) => {
                let [ax, ay, bx, by] = k.map(value);
                let Some(l0) = l0 else {
                    return Some(([-u.y, u.x, u.y, -u.x], (bx - ax) * u.y - (by - ay) * u.x));
                };
                let d = DVec2::new(bx - ax, by - ay);
                let length = d.length();
                // (Collapsed, or not a number: no direction to keep.)
                if length.is_nan() || length <= 0.0 {
                    return None;
                }
                let along = d / length;
                // The sine of the turn, and its gradient over the line's far end.
                let n = DVec2::new(u.y, -u.x);
                let sine = along.dot(n);
                let g = (n - along * sine) / length;
                let (turn, g) = if along.dot(u) >= 0.0 {
                    (sine, g)
                } else {
                    // Facing the other way: 2 − |sin|, which meets sin where the line stands
                    // square to the kept direction and grows to twice its length turned end
                    // for end, so the shape always leads back the way it was kept. That is
                    // sin and an excess, 2 − 2|sin| signed as sin; the excess fades out as the
                    // line shrinks to nothing (`turned_fades`), so a line shrinking along its
                    // kept direction passes through itself at no cost on either side. Not
                    // faded, it jumped there from nothing to twice its kept length: a relation
                    // that pushes a line back along itself drove it onto that cliff, where
                    // every step past was refused and the solve stalled wherever round-off
                    // had it (census case 2281: 13 ends for nudges of 2 ulps a point), and
                    // whether it was left a sliver to give way (see the editor's
                    // `check_new_constraints`) was round-off's call too.
                    let sign = if sine < 0.0 { -1.0 } else { 1.0 };
                    let excess = sign * (2.0 - 2.0 * sine.abs());
                    let (fade, fades) = turned_fades(length, l0);
                    (
                        sine + excess * fade,
                        g * (1.0 - 2.0 * fade) + along * (excess * fades),
                    )
                };
                // Read at the length it is kept at (and that length's gradient, along the
                // line).
                let (at, grows) = kept_at(length, l0);
                let g = g * at + along * (turn * grows);
                Some(([-g.x, -g.y, g.x, g.y], turn * at))
            }
            // (The gradient's first entry is the radius's; its parameters are that one.)
            Shape::Radius(r, r0) => Some(([1.0, 0.0, 0.0, 0.0], value(r) - r0)),
        }
    }
}

/// How much of a turned-round line's excess cost counts at `length`, for a line kept at `l0`,
/// and its derivative (see [`Shape::offset`]): all of it from a quarter of `l0` up, fading to
/// none at no length by a smoothstep, `3t² − 2t³` of `t = 4·length/l0`, level at both ends.
fn turned_fades(length: f64, l0: f64) -> (f64, f64) {
    let t = 4.0 * length / l0;
    if t >= 1.0 {
        return (1.0, 0.0);
    }
    (t * t * (3.0 - 2.0 * t), 6.0 * t * (1.0 - t) * 4.0 / l0)
}

/// The length a kept direction's turn is read at, for a line `length` long kept at `l0`, and
/// its derivative: `l0` up to `l0` (no line is kept square by shrinking it), then
/// `l0 + (l − l0)² / (l + l0)`, which leaves `l0` level (its value and slope meet there) and
/// grows about as `l` far out (see [`Shape::offset`]).
fn kept_at(length: f64, l0: f64) -> (f64, f64) {
    if length <= l0 {
        return (l0, 0.0);
    }
    let (past, sum) = (length - l0, length + l0);
    (
        l0 + past * past / sum,
        past * (past + 4.0 * l0) / (sum * sum),
    )
}

/// Two fixes on one parameter (or on parameters held equal) agree when their values are this
/// close, relative to the value (at least 1 mm): the values are as stored or typed, absolute
/// coordinates, so this is a share of them, as it was before the solve's bar became a share of
/// a component's size (PR 2 of the solver audit), which has nothing to say about two pins.
const PINS_AGREE: f64 = 1e-12;

/// Inverse weight for dragged parameters: they barely move, so the rest of the sketch adapts.
/// The first rank of what stays put (see [`crate::SolveOptions`]).
pub(crate) const DRAGGED_MOBILITY: f64 = 1e-6;

/// Inverse weight for carried parameters ([`crate::SolveOptions::carried`]): they stay put
/// too, but give way before the dragged where the relations won't have both. The second rank.
pub(crate) const CARRIED_MOBILITY: f64 = 1e-4;

/// How a tangency touches at a curve's end (see [`touching_end`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Touching {
    /// The arc's end is on the line.
    OnLine,
    /// A curve's end (the line's, or an arc's) is on this circle or arc.
    OnCircle(EntityId),
}

/// How far, relative to the size of the coordinates, a curve's end may be from the other
/// curve of a tangency and still count as its touching point. Exact constructions (a line
/// through an arc's end, an arc ending on another's circle) are exact to round-off, and a
/// solve that says the tangency is there holds it to the solve tolerance; a tangency that
/// merely passes near an end (within √(2·r·this) of it along the curve) is left alone.
const TOUCHING_END: f64 = 1e-9;

/// The touching point of a tangency between `a` and `b` where it is at an end of one curve
/// lying on the other (#97): an arc's end on the line, the line's end on the circle, or an
/// arc's end on the other circle — held there by a point-on-curve relation (`on_curve`), or
/// just there. Two rows then say so (the end on the other curve, unless a relation already
/// does, and the tangency there), as for a shared end: the general "distance = radius" form
/// has a double root there (the end's own row has the same gradient, or the line pivots about
/// its end), so Gauss–Newton only converges to about √(tolerance·r), and a drag lands off or
/// stalls.
fn touching_end(
    doc: &Document,
    a: EntityId,
    b: EntityId,
    on_curve: &HashSet<(EntityId, EntityId)>,
) -> Option<(EntityId, Touching)> {
    let kind = |id: EntityId| doc.get(id).map(|e| e.kind);
    let at = |id: EntityId| doc.point(id).ok();
    let ends = |id: EntityId| match kind(id) {
        Some(EntityKind::Line { a, b }) => vec![a, b],
        Some(EntityKind::Arc { start, end, .. }) => vec![start, end],
        _ => Vec::new(),
    };
    let touching = |curve: EntityId| match kind(curve) {
        Some(EntityKind::Line { .. }) => Touching::OnLine,
        _ => Touching::OnCircle(curve),
    };
    // Held on the other curve by a relation.
    let held = |curve: EntityId, other: EntityId| {
        ends(curve)
            .into_iter()
            .find(|&e| on_curve.contains(&(e, other)))
            .map(|e| (e, touching(other)))
    };
    if let Some(found) = held(a, b).or_else(|| held(b, a)) {
        return Some(found);
    }
    // Or lying on it.
    let near = |d: f64, points: &[DVec2]| {
        let scale = points
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs()])
            .fold(1.0, f64::max);
        d.abs() <= TOUCHING_END * scale
    };
    let on = |e: EntityId, curve: EntityId| {
        let p = at(e)?;
        let off = match kind(curve)? {
            EntityKind::Line { a: la, b: lb } => {
                let (pa, pb) = (at(la)?, at(lb)?);
                near((pb - pa).try_normalize()?.perp_dot(p - pa), &[pa, pb, p])
            }
            EntityKind::Arc { .. } | EntityKind::Circle { .. } => {
                let c = center_of(doc, curve)?;
                near(p.distance(c) - crate::radius_of(doc, curve)?, &[p, c])
            }
            _ => return None,
        };
        off.then(|| (e, touching(curve)))
    };
    // A line's end on the line itself is no touching point (only an arc's end is).
    let lying = |curve: EntityId, other: EntityId| {
        (!matches!(kind(other), Some(EntityKind::Line { .. }))
            || matches!(kind(curve), Some(EntityKind::Arc { .. })))
        .then(|| ends(curve).into_iter().find_map(|e| on(e, other)))
        .flatten()
    };
    lying(b, a).or_else(|| lying(a, b))
}

/// Where each tangency of a set of relations touched at a curve's end (see
/// `touching_end`), found once on the geometry where they held: what a drag's every step
/// lowers its tangencies by ([`crate::SolveOptions::touching`]), so a step whose translated
/// geometry happens to touch elsewhere (or nowhere) keeps the form the drag began with, and
/// the solve and a later look at the result read the tangency alike.
#[derive(Clone, Debug, Default)]
pub struct TouchingEnds(HashMap<(EntityId, EntityId), Option<(EntityId, Touching)>>);

/// The [`TouchingEnds`] of the tangencies among `relations`, on `doc` as it is now.
pub fn touching_ends(
    doc: &Document,
    relations: &[(Option<ConstraintId>, Lowered)],
) -> TouchingEnds {
    let constraints = || {
        relations.iter().filter_map(|(_, l)| match l {
            Lowered::Relation(c) => Some(*c),
            _ => None,
        })
    };
    let on_curve = on_curve_of(constraints());
    TouchingEnds(
        constraints()
            .filter_map(|c| match c {
                Constraint::Tangent { a, b } if doc.validate(&c).is_ok() => {
                    Some(((a, b), touching_end(doc, a, b, &on_curve)))
                }
                _ => None,
            })
            .collect(),
    )
}

/// Points held on curves by `relations`, for tangencies touching at an end held that way.
fn on_curve_of(relations: impl Iterator<Item = Constraint>) -> HashSet<(EntityId, EntityId)> {
    relations
        .filter_map(|c| match c {
            Constraint::PointOnCurve { point, curve } => Some((point, curve)),
            // A line's midpoint is on it too: a tangency touching there takes its form
            // from the relation, not from where the point happens to be (#148).
            Constraint::Midpoint { point, line } => Some((point, line)),
            _ => None,
        })
        .collect()
}

/// The centre point of a circle or arc.
fn center_of(doc: &Document, curve: EntityId) -> Option<DVec2> {
    match doc.get(curve)?.kind {
        EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. } => {
            doc.point(center).ok()
        }
        _ => None,
    }
}

impl System {
    pub fn build(doc: &Document, dragged: &[Param], lowering: Option<&Lowering>) -> Self {
        Self::build_in(
            doc,
            dragged,
            &[],
            None,
            &HashMap::new(),
            lowering,
            None,
            None,
        )
    }

    /// Build the system for the entities in `scope` only (everything if `None`). The scope
    /// must be closed under shared points and constraints (see [`crate::connected`]).
    /// `dragged` and `carried` are the parameters held where they are, in that order of rank.
    /// Relations are read through `lowering` where one is given. Tangencies `touching`
    /// names are lowered as it says, not as the geometry now touches.
    #[allow(clippy::too_many_arguments)]
    pub fn build_in(
        doc: &Document,
        dragged: &[Param],
        carried: &[Param],
        scope: Option<&std::collections::HashSet<EntityId>>,
        radii: &HashMap<EntityId, f64>,
        lowering: Option<&Lowering>,
        relations: Option<&[(Option<ConstraintId>, Lowered)]>,
        touching: Option<&TouchingEnds>,
    ) -> Self {
        // The driving relations as lowered (or as given), with the extras under a stand-in
        // id.
        let all;
        let rels: Vec<(ConstraintId, Lowered)> = match relations {
            Some(given) => given,
            None => {
                all = crate::lowering::relations(doc, lowering);
                &all
            }
        }
        .iter()
        .map(|&(id, l)| (id.unwrap_or(EXTRA), l))
        .collect();
        let held: Vec<Param> = dragged.iter().chain(carried).copied().collect();
        // Walls' faces and corners are generated from their centrelines: never unknowns. A
        // corner someone's own curve was drawn from is in, pinned where it is.
        let generated = doc.generated_entities();
        let pinned: std::collections::HashSet<EntityId> = if generated.is_empty() {
            Default::default()
        } else {
            doc.iter()
                .filter(|(id, _)| !generated.contains(id) && scope.is_none_or(|s| s.contains(id)))
                .flat_map(|(_, e)| e.kind.points())
                .filter(|p| generated.contains(p))
                .collect()
        };
        let keep = |id: EntityId| {
            (!generated.contains(&id) || pinned.contains(&id))
                && scope.is_none_or(|s| s.contains(&id))
        };
        // Relations whose geometry is in scope and still of the kinds they need (a trim or
        // a split can change a curve's kind under one): the rest are left out rather than
        // break the build. [`crate::invalid_constraints`] reports them.
        let usable = |c: &Constraint| c.entities().all(keep) && doc.validate(c).is_ok();
        let usable_offset = |point: EntityId, curve: EntityId| {
            keep(point)
                && keep(curve)
                && matches!(
                    doc.get(point).map(|e| e.kind),
                    Some(EntityKind::Point(_) | EntityKind::Line { .. })
                )
                && matches!(
                    doc.get(curve).map(|e| e.kind),
                    Some(
                        EntityKind::Line { .. }
                            | EntityKind::Arc { .. }
                            | EntityKind::Circle { .. }
                    )
                )
        };
        let relations = || {
            rels.iter().filter_map(|(id, l)| match l {
                Lowered::Relation(c) if usable(c) => Some((*id, *c)),
                _ => None,
            })
        };
        let offsets: Vec<(ConstraintId, EntityId, EntityId, f64)> = rels
            .iter()
            .filter_map(|(id, l)| match *l {
                Lowered::OffsetFrom {
                    point,
                    curve,
                    offset,
                } if usable_offset(point, curve) => Some((*id, point, curve, offset)),
                _ => None,
            })
            .collect();
        // ---- Parameters ----
        let mut params = Vec::new();
        let mut values = Vec::new();
        let mut entities = Vec::new();
        // (A scope's own entities, in id order as the drawing keeps them: a drag solving a
        // small part of a big drawing doesn't look through the rest.)
        let in_scope: Vec<(EntityId, &drift_core::Entity)> = match scope {
            Some(scope) => {
                let mut ids: Vec<EntityId> = scope.iter().copied().filter(|&id| keep(id)).collect();
                ids.sort_unstable();
                ids.into_iter()
                    .filter_map(|id| Some((id, doc.get(id)?)))
                    .collect()
            }
            None => doc.iter().filter(|(id, _)| keep(*id)).collect(),
        };
        let mut arcs = Vec::new();
        let mut centres = HashSet::new();
        let mut drawn = HashSet::new();
        for &(_, entity) in &in_scope {
            match entity.kind {
                EntityKind::Arc { center, start, end } => {
                    arcs.push([center, start, end]);
                    centres.insert(center);
                    drawn.extend([start, end]);
                }
                kind => drawn.extend(kind.points()),
            }
        }
        let centres_only: HashSet<EntityId> = centres.difference(&drawn).copied().collect();
        for &(id, entity) in &in_scope {
            entities.push((
                id,
                match entity.kind {
                    EntityKind::Point(_) => EntityShape::Point,
                    EntityKind::Circle { center, .. } => EntityShape::Curve(vec![center], Some(id)),
                    kind => EntityShape::Curve(kind.points().collect(), None),
                },
            ));
            match entity.kind {
                EntityKind::Point(p) => {
                    params.push(Param::X(id));
                    values.push(p.x);
                    params.push(Param::Y(id));
                    values.push(p.y);
                }
                EntityKind::Circle { radius, .. } => {
                    params.push(Param::Radius(id));
                    values.push(radius);
                }
                _ => {}
            }
        }
        // Lines that dimensions measure to the midpoint of get a hidden midpoint: two unknowns
        // tied to the line's ends by two equations, so they add no freedom.
        let mut midpoint_lines = Vec::new();
        for (_, c) in relations() {
            for (id, slot) in c.slots() {
                if slot == drift_core::Slot::PointOrMidpoint
                    && let Some(EntityKind::Line { .. }) = doc.get(id).map(|e| e.kind)
                    && !midpoint_lines.contains(&id)
                {
                    midpoint_lines.push(id);
                }
            }
        }
        for &(_, point, ..) in &offsets {
            if let Some(EntityKind::Line { .. }) = doc.get(point).map(|e| e.kind)
                && !midpoint_lines.contains(&point)
            {
                midpoint_lines.push(point);
            }
        }
        for &line in &midpoint_lines {
            let m = doc.ref_point(line).unwrap_or_default();
            params.push(Param::MidX(line));
            values.push(m.x);
            params.push(Param::MidY(line));
            values.push(m.y);
        }
        let param_index: HashMap<Param, usize> =
            params.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        // A point's coordinates, or (for a line in a dimension slot) its hidden midpoint's.
        let x = |id: EntityId| {
            param_index
                .get(&Param::X(id))
                .copied()
                .unwrap_or_else(|| param_index[&Param::MidX(id)])
        };
        let y = |id: EntityId| {
            param_index
                .get(&Param::Y(id))
                .copied()
                .unwrap_or_else(|| param_index[&Param::MidY(id)])
        };
        let point = |id: EntityId| [x(id), y(id)];
        let line = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => [x(a), y(a), x(b), y(b)],
            _ => unreachable!("validated constraint refers to a line"),
        };
        // A circle's radius parameter, or an arc's centre→start distance.
        let radius = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Circle { .. }) => Length::Param(param_index[&Param::Radius(id)]),
            Some(EntityKind::Arc { center, start, .. }) => {
                Length::Points([x(center), y(center), x(start), y(start)])
            }
            _ => unreachable!("validated constraint refers to a circle or arc"),
        };
        let length = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => Length::Points([x(a), y(a), x(b), y(b)]),
            _ => radius(id),
        };

        // ---- Substitution: merge equal parameters, pin fixed ones ----
        let mut uf = UnionFind::new(params.len());
        let mut fixed: HashMap<usize, (f64, ConstraintId)> = HashMap::new();
        let mut conflicts = Vec::new();
        let mut redundant = Vec::new();
        let mut rows = Vec::new();
        // Pin a parameter. A second pin on the same parameter (two fixes on one point, as
        // when two fixed points are merged) agrees, or conflicts.
        let pin = |fixed: &mut HashMap<usize, (f64, ConstraintId)>,
                   problems: (&mut Vec<ConstraintId>, &mut Vec<ConstraintId>),
                   param: usize,
                   v: f64,
                   cid: ConstraintId| {
            let (conflicts, redundant) = problems;
            match fixed.get(&param) {
                Some(&(old, _)) if (old - v).abs() > PINS_AGREE * old.abs().max(1.0) => {
                    conflicts.push(cid);
                }
                Some(_) => redundant.push(cid),
                None => {
                    fixed.insert(param, (v, cid));
                }
            }
        };
        // Collinear relations are built after all merges are known (see below).
        let mut collinear = Vec::new();
        // Points joined by coincident relations, by the root of their x parameter: curves
        // whose ends are joined that way share that end as much as curves sharing a point.
        let joined_ends: Vec<usize> = {
            let mut joined = UnionFind::new(params.len());
            for (_, c) in relations() {
                if let Constraint::Coincident { a, b } = c {
                    joined.join(x(a), x(b));
                }
            }
            (0..params.len()).map(|i| joined.find(i)).collect()
        };

        let center = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. }) => center,
            _ => unreachable!("validated constraint refers to a circle or arc"),
        };
        // Points held on curves, for tangencies touching at an end held that way.
        let on_curve = on_curve_of(relations().map(|(_, c)| c));
        // (Driven (reference) dimensions only report; they aren't among the relations.)
        for (cid, constraint) in relations() {
            let constraint = &constraint;
            let rows_before = rows.len();
            let mut joined = 0;
            let mut equalities = Vec::new();
            match *constraint {
                Constraint::Coincident { a, b } => {
                    equalities.push((x(a), x(b)));
                    equalities.push((y(a), y(b)));
                }
                Constraint::Horizontal { line: l } => {
                    let [_, ay, _, by] = line(l);
                    equalities.push((ay, by));
                }
                Constraint::Vertical { line: l } => {
                    let [ax, _, bx, _] = line(l);
                    equalities.push((ax, bx));
                }
                Constraint::HorizontalPoints { a, b } => equalities.push((y(a), y(b))),
                Constraint::VerticalPoints { a, b } => equalities.push((x(a), x(b))),
                Constraint::Fix { point: p, at } => {
                    for (param, v) in point(p).into_iter().zip([at.x, at.y]) {
                        pin(&mut fixed, (&mut conflicts, &mut redundant), param, v, cid);
                    }
                }
                Constraint::FixX { point: p, x: v } => {
                    pin(&mut fixed, (&mut conflicts, &mut redundant), x(p), v, cid);
                }
                Constraint::FixY { point: p, y: v } => {
                    pin(&mut fixed, (&mut conflicts, &mut redundant), y(p), v, cid);
                }
                Constraint::FixRadius { curve, radius: r } => match radius(curve) {
                    Length::Param(p) => {
                        pin(&mut fixed, (&mut conflicts, &mut redundant), p, r, cid)
                    }
                    length => rows.push(Row {
                        equation: Equation::LengthValue { length, value: r },
                        source: Some(cid),
                        owner: None,
                    }),
                },
                Constraint::Parallel { a, b } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), 0.0),
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Perpendicular { a, b } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), std::f64::consts::FRAC_PI_2),
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Angle { a, b, degrees } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), degrees.to_radians()),
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Equal { a, b } => rows.push(Row {
                    equation: Equation::EqualLength {
                        a: length(a),
                        b: length(b),
                    },
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Distance { a, b, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: Length::Points([x(a), y(a), x(b), y(b)]),
                        value,
                    },
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Radius { curve, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: radius(curve),
                        value,
                    },
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::Concentric { a, b } | Constraint::Coradial { a, b } => {
                    let (ca, cb) = (center(a), center(b));
                    equalities.push((x(ca), x(cb)));
                    equalities.push((y(ca), y(cb)));
                    if matches!(constraint, Constraint::Coradial { .. }) {
                        rows.push(Row {
                            equation: Equation::EqualLength {
                                a: radius(a),
                                b: radius(b),
                            },
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::Collinear { a, b } => collinear.push((cid, a, b)),
                Constraint::Smooth { point: p, a, b } => {
                    // The two handles on one line through the anchor (sin of the angle
                    // between anchor→a and anchor→b is zero).
                    let (pp, pa, pb) = (point(p), point(a), point(b));
                    rows.push(Row {
                        equation: Equation::angle(
                            [pp[0], pp[1], pa[0], pa[1]],
                            [pp[0], pp[1], pb[0], pb[1]],
                            std::f64::consts::PI,
                        ),
                        source: Some(cid),
                        owner: None,
                    });
                }
                Constraint::PointOnCurve { point: p, curve } => {
                    let equation = match doc.get(curve).map(|e| e.kind) {
                        Some(EntityKind::Line { .. }) => Equation::PointLine {
                            point: point(p),
                            line: line(curve),
                            sign: 1.0,
                            value: 0.0,
                        },
                        Some(
                            EntityKind::Ellipse {
                                center: c,
                                major,
                                minor,
                            }
                            | EntityKind::EllipseArc {
                                center: c,
                                major,
                                minor,
                                ..
                            },
                        ) => Equation::OnEllipse {
                            point: point(p),
                            center: point(c),
                            major: point(major),
                            minor: point(minor),
                        },
                        Some(EntityKind::Bezier { a, c1, c2, b }) => Equation::OnBezier {
                            point: point(p),
                            ctrl: [point(a), point(c1), point(c2), point(b)],
                        },
                        _ => {
                            let c = center(curve);
                            Equation::EqualLength {
                                a: Length::Points([x(c), y(c), x(p), y(p)]),
                                b: radius(curve),
                            }
                        }
                    };
                    rows.push(Row {
                        equation,
                        source: Some(cid),
                        owner: None,
                    });
                }
                Constraint::Symmetric { a, b, axis } => {
                    let (pa, pb, ax) = (point(a), point(b), line(axis));
                    rows.push(Row {
                        equation: Equation::SymmetricPerpendicular {
                            a: pa,
                            b: pb,
                            axis: ax,
                        },
                        source: Some(cid),
                        owner: None,
                    });
                    rows.push(Row {
                        equation: Equation::SymmetricMidpoint {
                            a: pa,
                            b: pb,
                            axis: ax,
                        },
                        source: Some(cid),
                        owner: None,
                    });
                }
                Constraint::Tangent { a, b }
                    if matches!(
                        doc.get(b).map(|e| e.kind),
                        Some(EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. })
                    ) =>
                {
                    // Tangent at the end the two share: each curve's direction there, as a
                    // line along it (a line, a Bézier's handle) or square to it (an arc's
                    // radius), or the ellipse itself. Curves pulled apart since the relation
                    // was added have no shared end, and the relation then holds nothing.
                    let kind = |id: EntityId| doc.get(id).map(|e| e.kind);
                    if let (Some(ka), Some(kb)) = (kind(a), kind(b))
                        && let Some(t) = ka.shared_end(&kb)
                    {
                        let pt = point(t);
                        // `a`: (direction line, whether it is square to the curve).
                        let (dir, normal) = match ka {
                            EntityKind::Line { a: la, b: lb } => {
                                let o = if la == t { lb } else { la };
                                ([x(o), y(o), pt[0], pt[1]], false)
                            }
                            _ => {
                                let c = point(center(a));
                                ([c[0], c[1], pt[0], pt[1]], true)
                            }
                        };
                        let equation = match kb {
                            EntityKind::EllipseArc {
                                center: c,
                                major,
                                minor,
                                ..
                            } => Equation::EllipseTangent {
                                at: pt,
                                center: point(c),
                                major: point(major),
                                minor: point(minor),
                                dir,
                                normal,
                            },
                            EntityKind::Bezier { a: s, c1, c2, b: e } => {
                                // The handle at the shared end, or the next control point where
                                // that handle has no length.
                                let (ctrl, at_end) = ([s, c1, c2, e], s != t);
                                let handle = match ctrl.map(|q| doc.point(q)) {
                                    [Ok(p0), Ok(p1), Ok(p2), Ok(p3)] => {
                                        drift_core::shape::bezier_end_handle(
                                            &[p0, p1, p2, p3],
                                            at_end,
                                        )
                                    }
                                    _ if at_end => 2,
                                    _ => 1,
                                };
                                // A handle with no length stays on its anchor (unless it is being
                                // pulled out), or the curve's direction there would follow
                                // wherever rounding left it rather than the point used here.
                                let own = if at_end { 2 } else { 1 };
                                if handle != own
                                    && !Param::point(ctrl[own]).iter().any(|q| held.contains(q))
                                {
                                    let [hx, hy] = point(ctrl[own]);
                                    equalities.push((hx, pt[0]));
                                    equalities.push((hy, pt[1]));
                                }
                                let h = point(ctrl[handle]);
                                let along = [h[0], h[1], pt[0], pt[1]];
                                let phi = if normal {
                                    std::f64::consts::FRAC_PI_2
                                } else {
                                    0.0
                                };
                                Equation::angle(dir, along, phi)
                            }
                            _ => unreachable!("matched above"),
                        };
                        rows.push(Row {
                            equation,
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::Tangent { a, b } => {
                    let cb = center(b);
                    // Curves that share an end point (fillets) are tangent *at* that point.
                    // Saying so directly (radius ⟂ line, or both centres in line with the
                    // point) is well conditioned; the general "distance = radius" form has a
                    // double root there and only converges to about √(tolerance·r).
                    let ends = |id: EntityId| -> Vec<EntityId> {
                        match doc.get(id).map(|e| e.kind) {
                            Some(EntityKind::Line { a, b }) => vec![a, b],
                            Some(EntityKind::Arc { start, end, .. }) => vec![start, end],
                            _ => Vec::new(),
                        }
                    };
                    let shared = ends(a).into_iter().find(|&p| {
                        ends(b)
                            .into_iter()
                            .any(|q| joined_ends[x(p)] == joined_ends[x(q)])
                    });
                    let kind_a = doc.get(a).map(|e| e.kind);
                    let is_line = matches!(kind_a, Some(EntityKind::Line { .. }));
                    // Tangent at `t`, an end of one curve lying on the other: the radius there
                    // is square to the line, or both centres are in line with it.
                    let at_end = |t: EntityId| {
                        if is_line {
                            Equation::SymmetricPerpendicular {
                                a: point(t),
                                b: point(cb),
                                axis: line(a),
                            }
                        } else {
                            let (t, ca) = (point(t), point(center(a)));
                            Equation::PointLine {
                                point: point(cb),
                                line: [t[0], t[1], ca[0], ca[1]],
                                sign: 1.0,
                                value: 0.0,
                            }
                        }
                    };
                    let mut equations = Vec::new();
                    if let Some(t) = shared {
                        equations.push(at_end(t));
                    } else if let Some((t, on)) = match touching.and_then(|t| t.0.get(&(a, b))) {
                        Some(&found) => found,
                        None => touching_end(doc, a, b, &on_curve),
                    } {
                        // The touching point is at an end of one curve that the other
                        // doesn't share: the same double root (#97). Keep the end on the
                        // other curve (unless a relation already does) and say the tangency
                        // is there.
                        let other = match on {
                            Touching::OnLine => a,
                            Touching::OnCircle(curve) => curve,
                        };
                        if !on_curve.contains(&(t, other)) {
                            equations.push(match on {
                                Touching::OnLine => Equation::PointLine {
                                    point: point(t),
                                    line: line(a),
                                    sign: 1.0,
                                    value: 0.0,
                                },
                                Touching::OnCircle(curve) => {
                                    let c = point(center(curve));
                                    let t = point(t);
                                    Equation::EqualLength {
                                        a: Length::Points([c[0], c[1], t[0], t[1]]),
                                        b: radius(curve),
                                    }
                                }
                            });
                        }
                        equations.push(at_end(t));
                    } else if is_line {
                        let ll = line(a);
                        let c = point(cb);
                        let cross = (values[ll[2]] - values[ll[0]])
                            * (values[c[1]] - values[ll[1]])
                            - (values[ll[3]] - values[ll[1]]) * (values[c[0]] - values[ll[0]]);
                        equations.push(Equation::TangentLine {
                            center: c,
                            line: ll,
                            sign: if cross >= 0.0 { 1.0 } else { -1.0 },
                            radius: radius(b),
                        });
                    } else {
                        // Two circles: outside (|c₁c₂| = r₁ + r₂) or inside
                        // (|c₁c₂| = |r₁ − r₂|), whichever the geometry is closer to.
                        let ca = center(a);
                        let r1 = crate::radius_of(doc, a).unwrap_or(0.0);
                        let r2 = crate::radius_of(doc, b).unwrap_or(0.0);
                        let dist = doc
                            .point(ca)
                            .ok()
                            .zip(doc.point(cb).ok())
                            .map_or(0.0, |(p, q)| p.distance(q));
                        let between = Length::Points([x(ca), y(ca), x(cb), y(cb)]);
                        let outside = (dist - (r1 + r2)).abs() <= (dist - (r1 - r2).abs()).abs();
                        let (k1, k2) = if outside {
                            (-1.0, -1.0)
                        } else if r1 >= r2 {
                            (-1.0, 1.0)
                        } else {
                            (1.0, -1.0)
                        };
                        equations.push(Equation::LengthSum {
                            terms: [(between, 1.0), (radius(a), k1), (radius(b), k2)],
                        });
                    }
                    for equation in equations {
                        rows.push(Row {
                            equation,
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::Midpoint { point: m, line: l } => {
                    let [ax, ay, bx, by] = line(l);
                    let [mx, my] = point(m);
                    for (p, a, b) in [(mx, ax, bx), (my, ay, by)] {
                        rows.push(Row {
                            equation: Equation::Midpoint { p, a, b },
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::MidpointOf { point: m, a, b } => {
                    let ([mx, my], [ax, ay], [bx, by]) = (point(m), point(a), point(b));
                    for (p, a, b) in [(mx, ax, bx), (my, ay, by)] {
                        rows.push(Row {
                            equation: Equation::Midpoint { p, a, b },
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::EqualDistance { a, b, c, d } => rows.push(Row {
                    equation: Equation::EqualLength {
                        a: Length::Points([x(a), y(a), x(b), y(b)]),
                        b: Length::Points([x(c), y(c), x(d), y(d)]),
                    },
                    source: Some(cid),
                    owner: None,
                }),
                // On both curves: the rows an On curve relation gives for each.
                Constraint::Corner { point: p, a, b } => {
                    for curve in [a, b] {
                        let equation = match doc.get(curve).map(|e| e.kind) {
                            Some(EntityKind::Line { .. }) => Equation::PointLine {
                                point: point(p),
                                line: line(curve),
                                sign: 1.0,
                                value: 0.0,
                            },
                            _ => {
                                let c = center(curve);
                                Equation::EqualLength {
                                    a: Length::Points([x(c), y(c), x(p), y(p)]),
                                    b: radius(curve),
                                }
                            }
                        };
                        rows.push(Row {
                            equation,
                            source: Some(cid),
                            owner: None,
                        });
                    }
                }
                Constraint::Diameter { curve, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: radius(curve),
                        value: value / 2.0,
                    },
                    source: Some(cid),
                    owner: None,
                }),
                Constraint::HorizontalDistance { a, b, value }
                | Constraint::VerticalDistance { a, b, value } => {
                    let (pa, pb) = if matches!(constraint, Constraint::HorizontalDistance { .. }) {
                        (x(a), x(b))
                    } else {
                        (y(a), y(b))
                    };
                    let sign = if values[pb] >= values[pa] { 1.0 } else { -1.0 };
                    rows.push(Row {
                        equation: Equation::Difference {
                            a: pa,
                            b: pb,
                            sign,
                            value,
                        },
                        source: Some(cid),
                        owner: None,
                    });
                }
                Constraint::PointLineDistance {
                    point: p,
                    line: l,
                    value,
                } => {
                    let pp = point(p);
                    let ll = line(l);
                    let cross = (values[ll[2]] - values[ll[0]]) * (values[pp[1]] - values[ll[1]])
                        - (values[ll[3]] - values[ll[1]]) * (values[pp[0]] - values[ll[0]]);
                    let sign = if cross >= 0.0 { 1.0 } else { -1.0 };
                    rows.push(Row {
                        equation: Equation::PointLine {
                            point: pp,
                            line: ll,
                            sign,
                            value,
                        },
                        source: Some(cid),
                        owner: None,
                    });
                }
            }
            for (p, q) in &equalities {
                if uf.join(*p, *q) {
                    joined += 1;
                }
            }
            // Entirely implied: every equality was already merged and nothing else was added.
            if !equalities.is_empty() && joined == 0 && rows.len() == rows_before {
                redundant.push(cid);
            }
        }

        // Geometry on locked layers is pinned where it is (marked with constraint id 0, which
        // is never reported as a conflict).
        let mut stay: Vec<EntityId> = doc.locked_entities().into_iter().collect();
        stay.extend(pinned.iter().copied());
        stay.sort_unstable();
        stay.dedup();
        for id in stay.into_iter().filter(|id| keep(*id)) {
            let pins: Vec<(usize, f64)> = match doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Point(p)) => vec![(x(id), p.x), (y(id), p.y)],
                Some(EntityKind::Circle { radius: r, .. }) => {
                    vec![(param_index[&Param::Radius(id)], r)]
                }
                _ => Vec::new(),
            };
            for (param, v) in pins {
                fixed.entry(param).or_insert((v, ConstraintId(0)));
            }
        }

        // Collinear: each end of `b` not shared with (or merged into) an end of `a` lies on
        // `a`'s line. Connected lines (a polyline corner) need one equation, separate lines
        // two; an equation for a shared end would be identically zero and read as redundant.
        for (cid, a, b) in collinear {
            let la = line(a);
            let lb = line(b);
            let same = |uf: &mut UnionFind, p: [usize; 2], q: [usize; 2]| {
                uf.find(p[0]) == uf.find(q[0]) && uf.find(p[1]) == uf.find(q[1])
            };
            let mut added = 0;
            for end in [[lb[0], lb[1]], [lb[2], lb[3]]] {
                let shared =
                    same(&mut uf, end, [la[0], la[1]]) || same(&mut uf, end, [la[2], la[3]]);
                if !shared {
                    rows.push(Row {
                        equation: Equation::PointLine {
                            point: end,
                            line: la,
                            sign: 1.0,
                            value: 0.0,
                        },
                        source: Some(cid),
                        owner: None,
                    });
                    added += 1;
                }
            }
            if added == 0 {
                // Both ends shared: the lines coincide already.
                redundant.push(cid);
            }
        }

        // Signed offsets from a line or a circle (lowered relations).
        for &(cid, p, curve, offset) in &offsets {
            let from = match doc.get(curve).map(|e| e.kind) {
                Some(EntityKind::Line { .. }) => Offset::Line(line(curve)),
                _ => Offset::Circle {
                    center: point(center(curve)),
                    radius: radius(curve),
                },
            };
            rows.push(Row {
                equation: Equation::OffsetFrom {
                    point: point(p),
                    from,
                    value: offset,
                },
                source: Some(cid),
                owner: None,
            });
        }
        // (The geometry's own relations report as no one's: each is owned instead by the
        // entity of its first parameter, a wall end's point.)
        for row in &mut rows {
            if row.source == Some(EXTRA) {
                row.source = None;
                row.owner = row.equation.params().first().map(|&i| match params[i] {
                    Param::X(id)
                    | Param::Y(id)
                    | Param::Radius(id)
                    | Param::MidX(id)
                    | Param::MidY(id) => id,
                });
            }
        }
        conflicts.retain(|c| *c != EXTRA);
        redundant.retain(|c| *c != EXTRA);

        for &l in &midpoint_lines {
            let [ax, ay, bx, by] = line(l);
            for (p, a, b) in [
                (param_index[&Param::MidX(l)], ax, bx),
                (param_index[&Param::MidY(l)], ay, by),
            ] {
                rows.push(Row {
                    equation: Equation::Midpoint { p, a, b },
                    source: None,
                    owner: Some(l),
                });
            }
        }

        // Ellipses keep their minor axis perpendicular to the major one, and elliptical arcs
        // their ends on the ellipse.
        for &(id, entity) in &in_scope {
            let (c, m, n, ends) = match entity.kind {
                EntityKind::Ellipse {
                    center,
                    major,
                    minor,
                } => (center, major, minor, None),
                EntityKind::EllipseArc {
                    center,
                    major,
                    minor,
                    start,
                    end,
                } => (center, major, minor, Some([start, end])),
                _ => continue,
            };
            let (pc, pm, pn) = (point(c), point(m), point(n));
            rows.push(Row {
                equation: Equation::angle(
                    [pc[0], pc[1], pm[0], pm[1]],
                    [pc[0], pc[1], pn[0], pn[1]],
                    std::f64::consts::FRAC_PI_2,
                ),
                source: None,
                owner: Some(id),
            });
            for end in ends.into_iter().flatten() {
                rows.push(Row {
                    equation: Equation::OnEllipse {
                        point: point(end),
                        center: pc,
                        major: pm,
                        minor: pn,
                    },
                    source: None,
                    owner: Some(id),
                });
            }
        }

        // Arcs keep their end on the circle through their start. (Which arcs keep their
        // radius, `solve` decides: dragging an end reshapes the arc, dragging its centre
        // carries it along.) Circles given a radius keep it as arcs do (dragging the rim drags
        // the radius itself, and `solve` lets it go).
        let mut keep_shape = Vec::new();
        for &(id, entity) in &in_scope {
            if let EntityKind::Circle { .. } = entity.kind
                && let Some(&r) = radii.get(&id)
                && let Some(&k) = param_index.get(&Param::Radius(id))
            {
                keep_shape.push(Shape::Radius(k, r));
            }
            if let EntityKind::Arc { center, start, end } = entity.kind {
                if let Some(&r) = radii.get(&id) {
                    keep_shape.push(Shape::Distance(
                        [x(center), y(center), x(start), y(start)],
                        r,
                    ));
                }
                rows.push(Row {
                    equation: Equation::EqualLength {
                        a: Length::Points([x(center), y(center), x(end), y(end)]),
                        b: Length::Points([x(center), y(center), x(start), y(start)]),
                    },
                    source: None,
                    owner: Some(id),
                });
            }
        }

        // ---- Groups ----
        let mut root_to_group = HashMap::new();
        let mut group_of = vec![0; params.len()];
        let mut groups: Vec<Group> = Vec::new();
        for (i, slot) in group_of.iter_mut().enumerate() {
            let root = uf.find(i);
            let g = *root_to_group.entry(root).or_insert_with(|| {
                groups.push(Group {
                    value: 0.0,
                    fixed: false,
                    mobility: 1.0,
                    members: Vec::new(),
                });
                groups.len() - 1
            });
            *slot = g;
            groups[g].members.push(i);
        }
        let dragged: Vec<usize> = dragged
            .iter()
            .filter_map(|p| param_index.get(p).copied())
            .collect();
        let held: Vec<usize> = held
            .iter()
            .filter_map(|p| param_index.get(p).copied())
            .collect();
        for group in &mut groups {
            // Start from the held members where there are any (so merging e.g. a dragged
            // point with a coincident one moves the other point, and the drag lands where it
            // was asked to), else from the mean of the members.
            let has_held = group.members.iter().any(|i| held.contains(i));
            let weight = |i: &usize| {
                if !has_held || held.contains(i) {
                    1.0
                } else {
                    0.0
                }
            };
            let total: f64 = group.members.iter().map(weight).sum();
            group.value = group
                .members
                .iter()
                .map(|i| values[*i] * weight(i))
                .sum::<f64>()
                / total;
            if group.members.iter().any(|i| dragged.contains(i)) {
                group.mobility = DRAGGED_MOBILITY;
            } else if has_held {
                group.mobility = CARRIED_MOBILITY;
            }
            let pins: Vec<(f64, ConstraintId)> = group
                .members
                .iter()
                .filter_map(|i| fixed.get(i).copied())
                .collect();
            if let Some(&(v, _)) = pins.first() {
                group.value = v;
                group.fixed = true;
                let scale = v.abs().max(1.0);
                // Further pins on parameters held equal (a point's y fixed, its line's other
                // end's y too, the line level) agree, implied by the first, or conflict.
                for &(other, cid) in &pins[1..] {
                    if cid.0 == 0 {
                        continue;
                    }
                    if (other - v).abs() > PINS_AGREE * scale {
                        conflicts.push(cid);
                    } else if !redundant.contains(&cid) {
                        redundant.push(cid);
                    }
                }
            }
        }

        Self {
            params,
            param_index,
            group_of,
            groups,
            rows,
            conflicts,
            redundant,
            entities,
            radii_kept: keep_shape.len(),
            keep_rank: Vec::new(),
            shapes_as_before: false,
            keep_of_group: Vec::new(),
            keep: keep_shape,
            bars: Vec::new(),
            arcs,
            centres_only,
        }
    }

    pub fn value(&self, param: usize) -> f64 {
        self.groups[self.group_of[param]].value
    }
}
