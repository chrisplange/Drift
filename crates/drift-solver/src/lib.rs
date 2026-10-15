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
mod sparse;
mod system;

use std::collections::{HashMap, HashSet};

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};

use crate::linalg::{Matrix, rref};
use crate::sparse::{Skyline, qr_rank};
use crate::system::{EntityShape, System};

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

#[derive(Clone, Debug, Default)]
pub struct SolveOptions {
    /// Parameters that should stay where they are if at all possible (e.g. the point under the
    /// mouse while dragging).
    pub dragged: Vec<Param>,
    /// Also run the degrees-of-freedom analysis.
    pub analyze: bool,
    pub tolerances: Tolerances,
    /// Solve only these entities (closed under shared points and constraints; see
    /// [`connected`]): a drag in a big sketch then costs what the dragged part costs.
    pub scope: Option<std::rc::Rc<HashSet<EntityId>>>,
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

/// Solve the document's constraints in place. Changes go through the document's normal edit
/// methods, so they are recorded by an open transaction.
pub fn solve(doc: &mut Document, options: &SolveOptions) -> SolveReport {
    let mut sys = System::build_in(
        doc,
        &options.dragged,
        &options.tolerances,
        options.scope.as_deref(),
    );
    let report = run(&mut sys, options, true);
    write_back(doc, &sys);
    report
}

/// Evaluate constraints and degrees of freedom without changing the document.
pub fn analyze(doc: &Document) -> SolveReport {
    let options = SolveOptions {
        analyze: true,
        ..Default::default()
    };
    let mut sys = System::build(doc, &[], &options.tolerances);
    run(&mut sys, &options, false)
}

/// The entities that can move with `seeds`: everything joined to them through shared points
/// and (non-driven) constraints.
pub fn connected(doc: &Document, seeds: impl IntoIterator<Item = EntityId>) -> HashSet<EntityId> {
    let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
    let index: HashMap<EntityId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let join = |parent: &mut Vec<usize>, a: EntityId, b: EntityId| {
        if let (Some(&i), Some(&j)) = (index.get(&a), index.get(&b)) {
            let (ri, rj) = (find(parent, i), find(parent, j));
            if ri != rj {
                parent[ri.max(rj)] = ri.min(rj);
            }
        }
    };
    for (id, e) in doc.iter() {
        for p in e.kind.points() {
            join(&mut parent, id, p);
        }
    }
    for (cid, c) in doc.constraints() {
        if doc.is_driven(cid) {
            continue;
        }
        let mut ents = c.entities();
        if let Some(first) = ents.next() {
            for other in ents {
                join(&mut parent, first, other);
            }
        }
    }
    let roots: HashSet<usize> = seeds
        .into_iter()
        .filter_map(|id| index.get(&id).copied())
        .map(|i| find(&mut parent, i))
        .collect();
    (0..ids.len())
        .filter(|&i| roots.contains(&find(&mut parent, i)))
        .map(|i| ids[i])
        .collect()
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
fn components(sys: &System) -> (Vec<Component>, Vec<usize>) {
    let n = sys.groups.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
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
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
        row_groups.push(free);
    }
    let mut by_root: HashMap<usize, Component> = HashMap::new();
    for (r, free) in row_groups.iter().enumerate() {
        if let Some(&g) = free.first() {
            let root = find(&mut parent, g);
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
            && let Some(c) = by_root.get_mut(&find(&mut parent, g))
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

fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>()
}

struct ComponentOutcome {
    converged: bool,
    iterations: usize,
}

/// Damped weighted minimum-norm Gauss–Newton on one component.
fn newton(
    sys: &System,
    comp: &Component,
    values: &mut [f64],
    tol: &Tolerances,
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
    for ns in &mut neighbors {
        ns.sort_unstable();
        ns.dedup();
    }
    let mut normal = Skyline::new(n, &neighbors);
    let _ = m;

    let mut lambda = 0.0f64;
    for iteration in 0..MAX_ITERATIONS {
        if converged(&res, values) {
            return ComponentOutcome {
                converged: true,
                iterations: iteration,
            };
        }
        // N = JᵀJ and the right-hand side −JᵀF.
        normal.clear();
        let mut rhs = vec![0.0; n];
        for (row, f) in jac.iter().zip(&res) {
            for (k, &(c1, g1)) in row.iter().enumerate() {
                rhs[c1] -= g1 * f;
                for &(c2, g2) in &row[..=k] {
                    normal.add(c1, c2, g1 * g2);
                }
            }
        }
        let max_diag = (0..n)
            .map(|c| normal.diagonal(c))
            .fold(0.0, f64::max)
            .max(1e-300);
        // μ·M⁻¹ picks the smallest weighted change among the solutions (and keeps
        // under-constrained systems solvable); λ adds Levenberg–Marquardt damping.
        let mu = 1e-10 * max_diag;

        let mut solved = None;
        for _ in 0..30 {
            let shift: Vec<f64> = mobility.iter().map(|m| (mu + lambda) / m).collect();
            let mut step = rhs.clone();
            if normal.solve(&shift, &mut step) {
                solved = Some(step);
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
        let Some(mut delta) = solved else { break };
        let (lo, hi) = values
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let max_step = (hi - lo).max(1.0);
        let biggest = delta.iter().fold(0.0f64, |acc, d| acc.max(d.abs()));
        if biggest > max_step {
            let k = max_step / biggest;
            delta.iter_mut().for_each(|d| *d *= k);
        }

        let mut trial = values.to_vec();
        for (c, g) in comp.unknowns.iter().enumerate() {
            trial[*g] += delta[c];
        }
        let (trial_res, trial_jac) = evaluate(sys, &comp.rows, &trial, &col_of);
        if norm2(&trial_res) < norm2(&res) || converged(&trial_res, &trial) {
            values.copy_from_slice(&trial);
            res = trial_res;
            jac = trial_jac;
            lambda *= 0.1;
            if lambda < 1e-12 * max_diag {
                lambda = 0.0;
            }
        } else {
            lambda = if lambda == 0.0 {
                1e-6 * max_diag
            } else {
                lambda * 10.0
            };
            if lambda > 1e12 * max_diag {
                break;
            }
        }
    }
    ComponentOutcome {
        converged: converged(&res, values),
        iterations: MAX_ITERATIONS,
    }
}

fn run(sys: &mut System, options: &SolveOptions, iterate: bool) -> SolveReport {
    let tol = &options.tolerances;
    let mut report = SolveReport {
        conflicts: sys.conflicts.clone(),
        redundant: sys.redundant.clone(),
        ..Default::default()
    };
    let (comps, constant_rows) = components(sys);
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
    for comp in &comps {
        if iterate {
            let outcome = newton(sys, comp, &mut values, tol);
            report.iterations = report.iterations.max(outcome.iterations);
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
pub fn free_directions(doc: &Document) -> HashMap<EntityId, Freedom> {
    let options = SolveOptions::default();
    let sys = System::build(doc, &[], &options.tolerances);
    let (comps, _) = components(&sys);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    // Null-space basis vectors, as group → component, indexed by the groups they touch.
    let mut basis: Vec<HashMap<usize, f64>> = Vec::new();
    let mut skipped: HashSet<usize> = HashSet::new();
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    for g in 0..sys.groups.len() {
        if !sys.groups[g].fixed && !in_component.contains(&g) {
            basis.push(HashMap::from([(g, 1.0)]));
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
        let pivot_row: HashMap<usize, usize> =
            pivots.iter().enumerate().map(|(r, c)| (*c, r)).collect();
        for free in (0..n).filter(|c| !pivot_row.contains_key(c)) {
            // x_free = 1, x_pivot = -R[row, free]; undo the column scaling.
            let mut v = HashMap::new();
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

fn write_back(doc: &mut Document, sys: &System) {
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
