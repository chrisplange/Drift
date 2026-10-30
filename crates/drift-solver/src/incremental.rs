//! The degrees-of-freedom analysis kept from edit to edit. A drawing falls apart into
//! clusters: entities joined through shared points and driving relations. Nothing in one
//! cluster's analysis depends on another's, so an edit analyses only the clusters it
//! changed (found from [`Document::changes_since`] and the relations) and keeps the rest,
//! and the report comes out as [`crate::analyze`]'s.

use std::collections::{BTreeSet, HashMap, HashSet};

use drift_core::polygon::UnionFind;
use drift_core::{Constraint, ConstraintId, Document, EntityId, LayerId};

use crate::system::{EntityShape, System};
use crate::{Definition, Param, SolveOptions, SolveReport};

/// What [`analyze_cached`] keeps: each cluster's share of the analysis, by its lowest
/// entity, and the report they make up.
#[derive(Default)]
pub struct AnalysisCache {
    /// The document analysed: its lineage and revision (None: nothing kept).
    at: Option<(u64, u64)>,
    /// Each entity's cluster (by its lowest entity) then, in id order.
    reps: Vec<(EntityId, EntityId)>,
    /// The driving relations then, in id order.
    driving: Vec<(ConstraintId, Constraint)>,
    locked: BTreeSet<LayerId>,
    parts: HashMap<EntityId, Part>,
    report: SolveReport,
    /// Parts that didn't converge, and parts with conflicts or redundant relations.
    unconverged: usize,
    problems: BTreeSet<EntityId>,
    misses: usize,
}

impl AnalysisCache {
    /// How many clusters were analysed again by the last [`analyze_cached`] (for tests and
    /// timing).
    pub fn last_misses(&self) -> usize {
        self.misses
    }

    /// The report of the last [`analyze_cached`].
    pub fn report(&self) -> &SolveReport {
        &self.report
    }

    fn remove(&mut self, rep: EntityId) {
        let Some(p) = self.parts.remove(&rep) else {
            return;
        };
        for (id, _) in &p.definitions {
            self.report.definitions.remove(id);
        }
        let r = &mut self.report;
        r.dof = Some(r.dof.unwrap_or(0) - p.dof);
        r.dependent_equations -= p.dependent;
        self.unconverged -= usize::from(!p.converged);
        self.problems.remove(&rep);
    }

    fn add(&mut self, rep: EntityId, p: Part) {
        let r = &mut self.report;
        r.definitions.extend(p.definitions.iter().copied());
        r.dof = Some(r.dof.unwrap_or(0) + p.dof);
        r.dependent_equations += p.dependent;
        self.unconverged += usize::from(!p.converged);
        if !p.conflicts.is_empty() || !p.redundant.is_empty() {
            self.problems.insert(rep);
        }
        self.parts.insert(rep, p);
    }

    /// Forget everything: the next analysis is of the whole drawing.
    fn clear(&mut self) {
        *self = Self {
            report: SolveReport {
                converged: true,
                dof: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
    }
}

/// One cluster's share of a [`SolveReport`].
#[derive(Clone, Default)]
struct Part {
    /// Its entities.
    size: usize,
    converged: bool,
    dof: usize,
    dependent: usize,
    conflicts: Vec<ConstraintId>,
    redundant: Vec<ConstraintId>,
    definitions: Vec<(EntityId, Definition)>,
}

/// [`crate::analyze_with`], analysing again only the clusters changed since `cache` was
/// last brought up to date (a drawing with walls, whose relations are lowered, is analysed
/// whole). The report is [`AnalysisCache::report`].
pub fn analyze_cached<'a>(
    doc: &Document,
    lowering: Option<&crate::Lowering>,
    cache: &'a mut AnalysisCache,
) -> &'a SolveReport {
    if lowering.is_some() || doc.has_walls() {
        cache.clear();
        cache.report = crate::analyze_with(doc, lowering);
        return &cache.report;
    }
    let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
    let index = |id: EntityId| ids.binary_search(&id).ok();
    let mut sets = UnionFind::new(ids.len());
    for (i, (_, e)) in doc.iter().enumerate() {
        for p in e.kind.points() {
            if let Some(j) = index(p) {
                sets.join(i, j);
            }
        }
    }
    let driving: Vec<(ConstraintId, Constraint)> = doc
        .constraints()
        .filter(|(id, _)| !doc.is_driven(*id))
        .map(|(id, c)| (id, *c))
        .collect();
    // Each relation goes with the cluster of the entities it joins (none: it holds nothing).
    let mut home: Vec<Option<usize>> = Vec::with_capacity(driving.len());
    for (_, c) in &driving {
        let mut at = c.entities().filter_map(index);
        let first = at.next();
        for j in at {
            sets.join(first.unwrap(), j);
        }
        home.push(first);
    }
    let locked: BTreeSet<LayerId> = doc
        .layers()
        .filter(|(_, l)| l.locked)
        .map(|(id, _)| id)
        .collect();
    let roots: Vec<usize> = (0..ids.len()).map(|i| sets.find(i)).collect();

    // What changed since: entities, and the entities of relations added, removed or changed.
    let since = cache
        .at
        .filter(|_| cache.locked == locked)
        .and_then(|(lineage, revision)| doc.changes_since(lineage, revision));
    let Some(since) = since else {
        // Nothing to go by: every cluster is analysed.
        cache.clear();
        return finish(doc, cache, &ids, &roots, &driving, &home, locked, None);
    };
    let mut changed: HashSet<EntityId> = since.entities.into_iter().collect();
    {
        let (old, new) = (&cache.driving, &driving);
        let (mut a, mut b) = (0, 0);
        while a < old.len() || b < new.len() {
            let take_old = b == new.len() || (a < old.len() && old[a].0 < new[b].0);
            let take_new = a == old.len() || (b < new.len() && new[b].0 < old[a].0);
            if take_old {
                changed.extend(old[a].1.entities());
                a += 1;
            } else if take_new {
                changed.extend(new[b].1.entities());
                b += 1;
            } else {
                if old[a].1 != new[b].1 {
                    changed.extend(old[a].1.entities());
                    changed.extend(new[b].1.entities());
                }
                a += 1;
                b += 1;
            }
        }
    }
    let old_rep = |id: EntityId| {
        cache
            .reps
            .binary_search_by_key(&id, |(e, _)| *e)
            .ok()
            .map(|k| cache.reps[k].1)
    };
    // Clusters a change was in before it.
    let dirty_before: HashSet<EntityId> = changed.iter().filter_map(|id| old_rep(*id)).collect();
    // A cluster is analysed again if a change is in it, or was in a cluster one of its
    // entities was in, or its entities were in another cluster, or it grew or shrank.
    let mut dirty = vec![false; ids.len()];
    let mut size = vec![0usize; ids.len()];
    for (i, id) in ids.iter().enumerate() {
        let root = roots[i];
        size[root] += 1;
        let was = old_rep(*id);
        if changed.contains(id) || was.is_none_or(|r| r != ids[root] || dirty_before.contains(&r)) {
            dirty[root] = true;
        }
    }
    for (i, &root) in roots.iter().enumerate() {
        if i == root
            && !dirty[root]
            && cache
                .parts
                .get(&ids[root])
                .is_none_or(|p| p.size != size[root])
        {
            dirty[root] = true;
        }
    }
    finish(
        doc,
        cache,
        &ids,
        &roots,
        &driving,
        &home,
        locked,
        Some(&dirty),
    )
}

/// Analyse the clusters `dirty` marks (by root index; all when None), drop the parts of
/// clusters that are gone or analysed again, and keep the rest.
#[allow(clippy::too_many_arguments)]
fn finish<'a>(
    doc: &Document,
    cache: &'a mut AnalysisCache,
    ids: &[EntityId],
    roots: &[usize],
    driving: &[(ConstraintId, Constraint)],
    home: &[Option<usize>],
    locked: BTreeSet<LayerId>,
    dirty: Option<&[bool]>,
) -> &'a SolveReport {
    let is_dirty = |root: usize| dirty.is_none_or(|d| d[root]);
    // Parts kept: clusters left as they were.
    let kept: HashSet<EntityId> = roots
        .iter()
        .enumerate()
        .filter(|&(i, &root)| i == root && !is_dirty(root))
        .map(|(_, &root)| ids[root])
        .collect();
    let gone: Vec<EntityId> = cache
        .parts
        .keys()
        .filter(|rep| !kept.contains(rep))
        .copied()
        .collect();
    for rep in gone {
        cache.remove(rep);
    }
    let mut cluster_of: HashMap<EntityId, EntityId> = HashMap::new();
    let mut of_constraint: HashMap<ConstraintId, EntityId> = HashMap::new();
    let mut sizes: HashMap<EntityId, usize> = HashMap::new();
    for (i, &root) in roots.iter().enumerate() {
        if is_dirty(root) {
            cluster_of.insert(ids[i], ids[root]);
            *sizes.entry(ids[root]).or_default() += 1;
        }
    }
    for (k, (id, _)) in driving.iter().enumerate() {
        if let Some(i) = home[k]
            && is_dirty(roots[i])
        {
            of_constraint.insert(*id, ids[roots[i]]);
        }
    }
    cache.misses = sizes.len();
    if !sizes.is_empty() {
        let scope: HashSet<EntityId> = cluster_of.keys().copied().collect();
        let options = SolveOptions {
            analyze: true,
            ..Default::default()
        };
        let sys = System::build_in(
            doc,
            &[],
            &[],
            &options.tolerances,
            Some(&scope),
            &HashMap::new(),
            None,
            None,
            None,
        );
        let Some(mut found) = analyze_parts(&sys, &options, &cluster_of, &of_constraint) else {
            // (Something no cluster owns: the drawing is analysed whole, and nothing kept.)
            cache.clear();
            cache.report = crate::analyze_with(doc, None);
            return &cache.report;
        };
        for (rep, size) in sizes {
            let mut part = found.remove(&rep).unwrap_or_else(|| Part {
                converged: true,
                ..Default::default()
            });
            part.size = size;
            cache.add(rep, part);
        }
    }
    let problems: Vec<&Part> = cache.problems.iter().map(|rep| &cache.parts[rep]).collect();
    let mut conflicts: Vec<ConstraintId> =
        problems.iter().flat_map(|p| p.conflicts.clone()).collect();
    let mut redundant: Vec<ConstraintId> =
        problems.iter().flat_map(|p| p.redundant.clone()).collect();
    conflicts.sort();
    redundant.sort();
    cache.report.conflicts = conflicts;
    cache.report.redundant = redundant;
    cache.report.converged = cache.unconverged == 0;
    cache.at = Some((doc.lineage(), doc.revision()));
    cache.reps = ids
        .iter()
        .zip(roots)
        .map(|(id, &r)| (*id, ids[r]))
        .collect();
    cache.driving = driving.to_vec();
    cache.locked = locked;
    &cache.report
}

/// [`crate::run`]'s analysis without iterating, shared out among clusters: each entity's,
/// relation's, row's and component's share goes to the cluster (`cluster_of`,
/// `of_constraint`) its entities are in.
fn analyze_parts(
    sys: &System,
    options: &SolveOptions,
    cluster_of: &HashMap<EntityId, EntityId>,
    of_constraint: &HashMap<ConstraintId, EntityId>,
) -> Option<HashMap<EntityId, Part>> {
    let tol = &options.tolerances;
    let mut parts: HashMap<EntityId, Part> = HashMap::new();
    let entity_of = |p: Param| match p {
        Param::X(id) | Param::Y(id) | Param::Radius(id) | Param::MidX(id) | Param::MidY(id) => id,
    };
    // Each group's cluster, from any of its parameters (merged ones are all in one).
    let mut group_cluster: Vec<Option<EntityId>> = vec![None; sys.groups.len()];
    for (i, p) in sys.params.iter().enumerate() {
        group_cluster[sys.group_of[i]] = cluster_of.get(&entity_of(*p)).copied();
    }
    let row_cluster = |r: usize| {
        sys.rows[r]
            .source
            .and_then(|c| of_constraint.get(&c).copied())
            .or_else(|| {
                sys.rows[r]
                    .equation
                    .params()
                    .into_iter()
                    .find_map(|p| group_cluster[sys.group_of[p]])
            })
    };
    for &c in &sys.conflicts {
        if let Some(&rep) = of_constraint.get(&c) {
            entry(&mut parts, rep).conflicts.push(c);
        }
    }
    for &c in &sys.redundant {
        if let Some(&rep) = of_constraint.get(&c) {
            entry(&mut parts, rep).redundant.push(c);
        }
    }
    let (comps, constant_rows) = crate::components(sys, tol);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let mut failed_rows: Vec<usize> = Vec::new();
    for &r in &constant_rows {
        let res = sys.rows[r]
            .equation
            .eval(&|p| values[sys.group_of[p]], &mut Vec::new());
        if res.abs() > crate::row_tolerance(sys, r, &values, tol) {
            failed_rows.push(r);
        } else if let Some(rep) = row_cluster(r) {
            entry(&mut parts, rep).dependent += 1;
        }
    }
    for comp in &comps {
        let (res, _) = crate::evaluate(sys, &comp.rows, &values, &HashMap::new());
        if res
            .iter()
            .zip(&comp.rows)
            .any(|(f, r)| f.abs() > crate::row_tolerance(sys, *r, &values, tol))
        {
            failed_rows.extend(&comp.rows);
        }
    }
    for &r in &failed_rows {
        // (A failing row no cluster owns: the caller analyses the drawing whole.)
        let rep = row_cluster(r)?;
        {
            let p = entry(&mut parts, rep);
            p.converged = false;
            if let Some(cid) = sys.rows[r].source
                && !p.conflicts.contains(&cid)
            {
                p.conflicts.push(cid);
            }
        }
    }
    for &c in &sys.conflicts {
        if let Some(&rep) = of_constraint.get(&c) {
            entry(&mut parts, rep).converged = false;
        }
    }

    // Degrees of freedom, as `analyze_dof` counts them.
    let mut determined: Vec<bool> = sys.groups.iter().map(|g| g.fixed).collect();
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    for (g, rep) in group_cluster.iter().enumerate() {
        if !sys.groups[g].fixed
            && !in_component.contains(&g)
            && let Some(rep) = *rep
        {
            entry(&mut parts, rep).dof += 1;
        }
    }
    for comp in &comps {
        let n = comp.unknowns.len();
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = crate::evaluate(sys, &comp.rows, &values, &col_of);
        let (rank, movable) = crate::sparse_rank(&jac, n);
        if let Some(rep) = comp.unknowns.first().and_then(|g| group_cluster[*g]) {
            let p = entry(&mut parts, rep);
            p.dof += n - rank;
            p.dependent += comp.rows.len().saturating_sub(rank);
        }
        for (c, g) in comp.unknowns.iter().enumerate() {
            if !movable[c] {
                determined[*g] = true;
            }
        }
    }
    let param_determined = |p: Param| {
        sys.param_index
            .get(&p)
            .is_some_and(|&i| determined[sys.group_of[i]])
    };
    let point_full =
        |id: EntityId| param_determined(Param::X(id)) && param_determined(Param::Y(id));
    let mut failing_params: HashSet<usize> = HashSet::new();
    for &r in &failed_rows {
        failing_params.extend(sys.rows[r].equation.params());
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
        let definition = if over {
            Definition::Over
        } else if full {
            Definition::Full
        } else {
            Definition::Under
        };
        if let Some(&rep) = cluster_of.get(&id) {
            entry(&mut parts, rep).definitions.push((id, definition));
        }
    }
    Some(parts)
}

fn entry(parts: &mut HashMap<EntityId, Part>, rep: EntityId) -> &mut Part {
    parts.entry(rep).or_insert_with(|| Part {
        converged: true,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use drift_core::{DVec2, Layer};
    use proptest::prelude::*;

    use super::*;

    /// One random change to a sketch, by numbers picked from `n` (an op, then its choices).
    fn change(doc: &mut Document, locked: drift_core::LayerId, n: &[u32; 5]) {
        let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
        let points: Vec<EntityId> = doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let lines: Vec<EntityId> = doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .collect();
        let pick = |list: &[EntityId], k: u32| list[k as usize % list.len().max(1)];
        let at = |k: u32| DVec2::new(f64::from(k % 97), f64::from(k / 97 % 89));
        let cons: Vec<ConstraintId> = doc.constraints().map(|(id, _)| id).collect();
        match n[0] % 16 {
            0 | 1 => {
                doc.add_point(at(n[1]));
            }
            _ if points.len() < 3 => {
                doc.add_point(at(n[1]));
            }
            2 | 3 => {
                let _ = doc.add_line(pick(&points, n[1]), pick(&points, n[2]));
            }
            4 => {
                let _ = doc.add_circle(pick(&points, n[1]), 1.0 + f64::from(n[2] % 9));
            }
            5 => {
                let _ = doc.add_arc(
                    pick(&points, n[1]),
                    pick(&points, n[2]),
                    pick(&points, n[3]),
                );
            }
            6 => {
                let _ = doc.set_point(pick(&points, n[1]), at(n[2]));
            }
            7 => doc.delete([pick(&ids, n[1])]),
            8 => {
                let _ = doc.add_constraint(Constraint::Coincident {
                    a: pick(&points, n[1]),
                    b: pick(&points, n[2]),
                });
            }
            9 if !lines.is_empty() => {
                let line = pick(&lines, n[1]);
                let _ = doc.add_constraint(if n[2].is_multiple_of(2) {
                    Constraint::Horizontal { line }
                } else {
                    Constraint::Vertical { line }
                });
            }
            10 => {
                let _ = doc.add_constraint(Constraint::Distance {
                    a: pick(&points, n[1]),
                    b: pick(&points, n[2]),
                    value: f64::from(n[3] % 40),
                });
            }
            11 => {
                let _ = doc.add_constraint(Constraint::Fix {
                    point: pick(&points, n[1]),
                    at: at(n[2]),
                });
            }
            12 if lines.len() > 1 => {
                let (a, b) = (pick(&lines, n[1]), pick(&lines, n[2]));
                let _ = doc.add_constraint(match n[3] % 3 {
                    0 => Constraint::Parallel { a, b },
                    1 => Constraint::Perpendicular { a, b },
                    _ => Constraint::Equal { a, b },
                });
            }
            13 if !cons.is_empty() => doc.remove_constraint(cons[n[1] as usize % cons.len()]),
            14 if !cons.is_empty() => {
                let id = cons[n[1] as usize % cons.len()];
                let _ = doc.set_driven(id, !doc.is_driven(id));
            }
            15 if n[2].is_multiple_of(3) => {
                // The layer locked or unlocked (its geometry pinned or let go).
                let mut layer = doc.layer(locked).unwrap().clone();
                layer.locked = !layer.locked;
                doc.set_layer(locked, layer);
            }
            15 => {
                let _ = doc.set_entity_layer(pick(&ids, n[1]), locked);
            }
            _ => {
                doc.add_point(at(n[4]));
            }
        }
    }

    fn same(a: &SolveReport, b: &SolveReport) -> Result<(), TestCaseError> {
        prop_assert_eq!(a.converged, b.converged);
        prop_assert_eq!(a.dof, b.dof);
        prop_assert_eq!(a.dependent_equations, b.dependent_equations);
        prop_assert_eq!(&a.conflicts, &b.conflicts);
        prop_assert_eq!(&a.redundant, &b.redundant);
        prop_assert_eq!(&a.definitions, &b.definitions);
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Kept cluster by cluster, the analysis is the whole drawing's after any edits.
        #[test]
        fn cached_analysis_is_the_full_analysis(
            steps in prop::collection::vec(prop::array::uniform5(0u32..10_000), 1..60),
            every in 1usize..4,
        ) {
            let mut doc = Document::new();
            let locked = doc.add_layer(Layer {
                locked: true,
                ..Layer::new("Locked")
            });
            let mut cache = AnalysisCache::default();
            for (k, n) in steps.iter().enumerate() {
                change(&mut doc, locked, n);
                if k % every == 0 {
                    same(&analyze_cached(&doc, None, &mut cache).clone(), &crate::analyze(&doc))?;
                }
            }
            same(&analyze_cached(&doc, None, &mut cache).clone(), &crate::analyze(&doc))?;
            // Nothing changed: nothing analysed again.
            analyze_cached(&doc, None, &mut cache);
            prop_assert_eq!(cache.last_misses(), 0);
        }
    }
}
