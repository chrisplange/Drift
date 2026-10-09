//! Turning a document into a system of equations over unknowns.
//!
//! Every point contributes `x` and `y` parameters and every circle a radius. Equality-style
//! constraints (coincident, horizontal, vertical) don't become equations; they merge
//! parameters into groups (substitution), which shrinks the system and makes those relations
//! exact. Fixed values pin a group. The remaining constraints become [`Equation`]s, and
//! unknowns are split into independent components that are solved separately.

use std::collections::HashMap;

use drift_core::{Constraint, ConstraintId, Document, EntityId, EntityKind};

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
const DRAGGED_MOBILITY: f64 = 1e-6;

impl System {
    pub fn build(doc: &Document, dragged: &[Param], tol: &Tolerances) -> Self {
        // ---- Parameters ----
        let mut params = Vec::new();
        let mut values = Vec::new();
        let mut entities = Vec::new();
        for (id, entity) in doc.iter() {
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
        for (_, c) in doc.constraints() {
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
        // Collinear relations are built after all merges are known (see below).
        let mut collinear = Vec::new();

        let center = |id: EntityId| match doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. }) => center,
            _ => unreachable!("validated constraint refers to a circle or arc"),
        };
        for (cid, constraint) in doc.constraints() {
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
                        fixed.insert(param, (v, cid));
                    }
                }
                Constraint::FixRadius { curve, radius: r } => match radius(curve) {
                    Length::Param(p) => {
                        fixed.insert(p, (r, cid));
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
                    let shared = ends(a).into_iter().find(|p| ends(b).contains(p));
                    let kind_a = doc.get(a).map(|e| e.kind);
                    let at_shared = shared.map(|t| match kind_a {
                        Some(EntityKind::Line { .. }) => Equation::SymmetricPerpendicular {
                            a: point(t),
                            b: point(cb),
                            axis: line(a),
                        },
                        _ => {
                            let (t, ca) = (point(t), point(center(a)));
                            Equation::PointLine {
                                point: point(cb),
                                line: [t[0], t[1], ca[0], ca[1]],
                                sign: 1.0,
                                value: 0.0,
                            }
                        }
                    });
                    let equation = match kind_a {
                        _ if at_shared.is_some() => at_shared.expect("checked"),
                        Some(EntityKind::Line { .. }) => {
                            let ll = line(a);
                            let c = point(cb);
                            let cross = (values[ll[2]] - values[ll[0]])
                                * (values[c[1]] - values[ll[1]])
                                - (values[ll[3]] - values[ll[1]]) * (values[c[0]] - values[ll[0]]);
                            Equation::TangentLine {
                                center: c,
                                line: ll,
                                sign: if cross >= 0.0 { 1.0 } else { -1.0 },
                                radius: radius(b),
                            }
                        }
                        _ => {
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
                            let outside =
                                (dist - (r1 + r2)).abs() <= (dist - (r1 - r2).abs()).abs();
                            let (k1, k2) = if outside {
                                (-1.0, -1.0)
                            } else if r1 >= r2 {
                                (-1.0, 1.0)
                            } else {
                                (1.0, -1.0)
                            };
                            Equation::LengthSum {
                                terms: [(between, 1.0), (radius(a), k1), (radius(b), k2)],
                            }
                        }
                    };
                    rows.push(Row {
                        equation,
                        source: Some(cid),
                    });
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
        for id in doc.locked_entities() {
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
        for (_, entity) in doc.iter() {
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

        // Arcs keep their end on the circle through their start.
        for (_, entity) in doc.iter() {
            if let EntityKind::Arc { center, start, end } = entity.kind {
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
        for group in &mut groups {
            // Start from a weighted mean of the members: dragged members dominate, so merging
            // e.g. a dragged point with a coincident one moves the other point.
            let weight = |i: &usize| if dragged.contains(i) { 1e6 } else { 1.0 };
            let total: f64 = group.members.iter().map(weight).sum();
            group.value = group
                .members
                .iter()
                .map(|i| values[*i] * weight(i))
                .sum::<f64>()
                / total;
            if group.members.iter().any(|i| dragged.contains(i)) {
                group.mobility = DRAGGED_MOBILITY;
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
        }
    }

    pub fn value(&self, param: usize) -> f64 {
        self.groups[self.group_of[param]].value
    }
}
