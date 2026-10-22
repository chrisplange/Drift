//! Turning a document into a system of equations over unknowns.
//!
//! Every point contributes `x` and `y` parameters and every circle a radius. Equality-style
//! constraints (coincident, horizontal, vertical) don't become equations; they merge
//! parameters into groups (substitution), which shrinks the system and makes those relations
//! exact. Fixed values pin a group. The remaining constraints become [`Equation`]s, and
//! unknowns are split into independent components that are solved separately.

use std::collections::{HashMap, HashSet};

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};

use crate::equations::{Equation, Length};
use crate::{Param, Tolerances};

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
}

/// A shape to keep where the relations leave it open (see [`System::keep`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// The distance between two points (an arc's centre and start, a line's ends): x and y
    /// of each, and the distance.
    Distance([usize; 4], f64),
    /// A line's direction: its ends' x and y, and the unit direction.
    Direction([usize; 4], DVec2),
}

impl Shape {
    /// The parameters it is over.
    pub fn params(&self) -> &[usize; 4] {
        match self {
            Shape::Distance(k, _) | Shape::Direction(k, _) => k,
        }
    }

    /// How far off the shape is at `value`, as a distance (a direction's: how far one end is
    /// off the line through the other), and its gradient over [`params`](Self::params), a
    /// unit vector per end. `None` where the shape has collapsed (no distance to keep).
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
            Shape::Direction(k, u) => {
                let [ax, ay, bx, by] = k.map(value);
                Some(([-u.y, u.x, u.y, -u.x], (bx - ax) * u.y - (by - ay) * u.x))
            }
        }
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    /// Returns false if already joined.
    fn union(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        self.parent[ra.max(rb)] = ra.min(rb);
        true
    }
}

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
    pub fn build(doc: &Document, dragged: &[Param], tol: &Tolerances) -> Self {
        Self::build_in(doc, dragged, &[], tol, None, &HashMap::new())
    }

    /// Build the system for the entities in `scope` only (everything if `None`). The scope
    /// must be closed under shared points and constraints (see [`crate::connected`]).
    /// `dragged` and `carried` are the parameters held where they are, in that order of rank.
    pub fn build_in(
        doc: &Document,
        dragged: &[Param],
        carried: &[Param],
        tol: &Tolerances,
        scope: Option<&std::collections::HashSet<EntityId>>,
        radii: &HashMap<EntityId, f64>,
    ) -> Self {
        let held: Vec<Param> = dragged.iter().chain(carried).copied().collect();
        let keep = |id: EntityId| scope.is_none_or(|s| s.contains(&id));
        // Relations whose geometry is in scope and still of the kinds they need (a trim or
        // a split can change a curve's kind under one): the rest are left out rather than
        // break the build. [`crate::invalid_constraints`] reports them.
        let usable = |c: &Constraint| c.entities().all(keep) && doc.validate(c).is_ok();
        // ---- Parameters ----
        let mut params = Vec::new();
        let mut values = Vec::new();
        let mut entities = Vec::new();
        for (id, entity) in doc.iter().filter(|(id, _)| keep(*id)) {
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
        for (_, c) in doc.constraints().filter(|(_, c)| usable(c)) {
            for (id, slot) in c.slots() {
                if slot == drift_core::Slot::PointOrMidpoint
                    && let Some(EntityKind::Line { .. }) = doc.get(id).map(|e| e.kind)
                    && !midpoint_lines.contains(&id)
                {
                    midpoint_lines.push(id);
                }
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
        let length_tol = tol.length;
        let pin = |fixed: &mut HashMap<usize, (f64, ConstraintId)>,
                   problems: (&mut Vec<ConstraintId>, &mut Vec<ConstraintId>),
                   param: usize,
                   v: f64,
                   cid: ConstraintId| {
            let (conflicts, redundant) = problems;
            match fixed.get(&param) {
                Some(&(old, _)) if (old - v).abs() > length_tol * old.abs().max(1.0) => {
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
            for (cid, c) in doc.constraints().filter(|(_, c)| usable(c)) {
                if let Constraint::Coincident { a, b } = *c
                    && !doc.is_driven(cid)
                {
                    joined.union(x(a), x(b));
                }
            }
            (0..params.len()).map(|i| joined.find(i)).collect()
        };

        let center = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. }) => center,
            _ => unreachable!("validated constraint refers to a circle or arc"),
        };
        // Points held on curves, for tangencies touching at an end held that way.
        let on_curve: HashSet<(EntityId, EntityId)> = doc
            .constraints()
            .filter(|(cid, c)| usable(c) && !doc.is_driven(*cid))
            .filter_map(|(_, c)| match *c {
                Constraint::PointOnCurve { point, curve } => Some((point, curve)),
                // A line's midpoint is on it too: a tangency touching there takes its form
                // from the relation, not from where the point happens to be (#148).
                Constraint::Midpoint { point, line } => Some((point, line)),
                _ => None,
            })
            .collect();
        for (cid, constraint) in doc.constraints().filter(|(_, c)| usable(c)) {
            if doc.is_driven(cid) {
                // Driven (reference) dimensions only report; they don't constrain.
                continue;
            }
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
                Constraint::FixRadius { curve, radius: r } => match radius(curve) {
                    Length::Param(p) => {
                        pin(&mut fixed, (&mut conflicts, &mut redundant), p, r, cid)
                    }
                    length => rows.push(Row {
                        equation: Equation::LengthValue { length, value: r },
                        source: Some(cid),
                    }),
                },
                Constraint::Parallel { a, b } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), 0.0),
                    source: Some(cid),
                }),
                Constraint::Perpendicular { a, b } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), std::f64::consts::FRAC_PI_2),
                    source: Some(cid),
                }),
                Constraint::Angle { a, b, degrees } => rows.push(Row {
                    equation: Equation::angle(line(a), line(b), degrees.to_radians()),
                    source: Some(cid),
                }),
                Constraint::Equal { a, b } => rows.push(Row {
                    equation: Equation::EqualLength {
                        a: length(a),
                        b: length(b),
                    },
                    source: Some(cid),
                }),
                Constraint::Distance { a, b, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: Length::Points([x(a), y(a), x(b), y(b)]),
                        value,
                    },
                    source: Some(cid),
                }),
                Constraint::Radius { curve, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: radius(curve),
                        value,
                    },
                    source: Some(cid),
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
                    });
                    rows.push(Row {
                        equation: Equation::SymmetricMidpoint {
                            a: pa,
                            b: pb,
                            axis: ax,
                        },
                        source: Some(cid),
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
                    } else if let Some((t, on)) = touching_end(doc, a, b, &on_curve) {
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
                        });
                    }
                }
                Constraint::Diameter { curve, value } => rows.push(Row {
                    equation: Equation::LengthValue {
                        length: radius(curve),
                        value: value / 2.0,
                    },
                    source: Some(cid),
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
                    });
                }
            }
            for (p, q) in &equalities {
                if uf.union(*p, *q) {
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
        for id in doc.locked_entities().into_iter().filter(|id| keep(*id)) {
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
                    });
                    added += 1;
                }
            }
            if added == 0 {
                // Both ends shared: the lines coincide already.
                redundant.push(cid);
            }
        }

        for &l in &midpoint_lines {
            let [ax, ay, bx, by] = line(l);
            for (p, a, b) in [
                (param_index[&Param::MidX(l)], ax, bx),
                (param_index[&Param::MidY(l)], ay, by),
            ] {
                rows.push(Row {
                    equation: Equation::Midpoint { p, a, b },
                    source: None,
                });
            }
        }

        // Ellipses keep their minor axis perpendicular to the major one, and elliptical arcs
        // their ends on the ellipse.
        for (_, entity) in doc.iter().filter(|(id, _)| keep(*id)) {
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
                });
            }
        }

        // Arcs keep their end on the circle through their start. (Which arcs keep their
        // radius, `solve` decides: dragging an end reshapes the arc, dragging its centre
        // carries it along.)
        let mut keep_shape = Vec::new();
        for (id, entity) in doc.iter().filter(|(id, _)| keep(*id)) {
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
                for &(other, cid) in &pins[1..] {
                    if (other - v).abs() > tol.length * scale && cid.0 != 0 {
                        conflicts.push(cid);
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
            keep: keep_shape,
        }
    }

    pub fn value(&self, param: usize) -> f64 {
        self.groups[self.group_of[param]].value
    }
}
