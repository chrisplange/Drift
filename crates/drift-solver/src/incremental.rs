//! The degrees-of-freedom analysis kept from edit to edit. A drawing falls apart into
//! clusters: entities joined through shared points and driving relations. Nothing in one
//! cluster's analysis depends on another's, so an edit analyses only the clusters it
//! changed (found from [`Document::changes_since`] and the relations) and keeps the rest,
//! and the report comes out as [`crate::analyze`]'s.

use std::collections::{BTreeSet, HashMap, HashSet};

use drift_core::polygon::UnionFind;
use drift_core::{
    Constraint, ConstraintId, DVec2, Document, Entity, EntityId, EntityKind, LayerId,
};

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
    /// Each entity then, with a point's place left out ([`unplaced`]), in the order of
    /// `reps`: what tells a change that only moved a point from any other (#371).
    unplaced: Vec<Entity>,
    /// The driving relations then, in id order.
    driving: Vec<(ConstraintId, Constraint)>,
    locked: BTreeSet<LayerId>,
    parts: HashMap<EntityId, Part>,
    report: SolveReport,
    /// Parts that didn't converge, and parts with conflicts, redundant relations or exposed
    /// geometry.
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
        if !p.conflicts.is_empty() || !p.redundant.is_empty() || !p.exposed.is_empty() {
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
    /// See [`SolveReport::exposed`] and [`SolveReport::broken`].
    exposed: Vec<EntityId>,
    broken: Vec<EntityId>,
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
    let entities: Vec<&Entity> = doc.iter().map(|(_, e)| e).collect();
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
    // The entities of relations added, removed or changed.
    let mut related: HashSet<EntityId> = HashSet::new();
    {
        let (old, new) = (&cache.driving, &driving);
        let (mut a, mut b) = (0, 0);
        while a < old.len() || b < new.len() {
            let take_old = b == new.len() || (a < old.len() && old[a].0 < new[b].0);
            let take_new = a == old.len() || (b < new.len() && new[b].0 < old[a].0);
            if take_old {
                related.extend(old[a].1.entities());
                a += 1;
            } else if take_new {
                related.extend(new[b].1.entities());
                b += 1;
            } else {
                if old[a].1 != new[b].1 {
                    related.extend(old[a].1.entities());
                    related.extend(new[b].1.entities());
                }
                a += 1;
                b += 1;
            }
        }
    }
    // Each entity's place in the cache's lists, by one walk of the two (both in id order).
    let mut old_of: Vec<Option<usize>> = vec![None; ids.len()];
    {
        let mut k = 0;
        for (i, id) in ids.iter().enumerate() {
            while k < cache.reps.len() && cache.reps[k].0 < *id {
                k += 1;
            }
            if k < cache.reps.len() && cache.reps[k].0 == *id {
                old_of[i] = Some(k);
            }
        }
    }
    let mut changed = vec![false; ids.len()];
    // Clusters a change was in before it.
    let mut dirty_before: HashSet<EntityId> = HashSet::new();
    let mut mark = |id: EntityId, at: Option<usize>| {
        if let Some(i) = at {
            changed[i] = true;
        }
        let k = match at {
            Some(i) => old_of[i],
            None => cache.reps.binary_search_by_key(&id, |(e, _)| *e).ok(),
        };
        if let Some(k) = k {
            dirty_before.insert(cache.reps[k].1);
        }
    };
    for &id in &related {
        mark(id, index(id));
    }
    // Entities that only moved or didn't change (the same entity apart from a point's
    // place, and no relation on it changed) in a cluster with nothing that reads where its
    // points are: no driving relation, no arc or ellipse (they keep a shape of their own),
    // nothing pinned on a locked layer. Such a cluster's analysis is its entities'
    // freedoms, whatever their places, so a move alone leaves it as it was (#371: a whole
    // move of a big drawing). (The changes are in id order, as the entities are.)
    let mut moved: Vec<usize> = Vec::new();
    {
        let mut i = 0;
        for &id in &since.entities {
            while i < ids.len() && ids[i] < id {
                i += 1;
            }
            let at = (i < ids.len() && ids[i] == id).then_some(i);
            let only_moved = !related.contains(&id)
                && at.is_some_and(|i| {
                    old_of[i].is_some_and(|k| unplaced(entities[i]) == cache.unplaced[k])
                });
            match at {
                Some(i) if only_moved => moved.push(i),
                _ => mark(id, at),
            }
        }
    }
    if !moved.is_empty() {
        let mut placed = vec![false; ids.len()];
        for (i, e) in entities.iter().enumerate() {
            if locked.contains(&e.layer)
                || matches!(
                    e.kind,
                    EntityKind::Arc { .. }
                        | EntityKind::Ellipse { .. }
                        | EntityKind::EllipseArc { .. }
                )
            {
                placed[roots[i]] = true;
            }
        }
        for i in home.iter().flatten() {
            placed[roots[*i]] = true;
        }
        for i in moved {
            if placed[roots[i]] {
                mark(ids[i], Some(i));
            }
        }
    }
    // A cluster is analysed again if a change is in it, or was in a cluster one of its
    // entities was in, or its entities were in another cluster, or it grew or shrank.
    let mut dirty = vec![false; ids.len()];
    let mut size = vec![0usize; ids.len()];
    for (i, was) in old_of.iter().enumerate() {
        let root = roots[i];
        size[root] += 1;
        let was = was.map(|k| cache.reps[k].1);
        if changed[i] || was.is_none_or(|r| r != ids[root] || dirty_before.contains(&r)) {
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
    let mut exposed: Vec<EntityId> = problems.iter().flat_map(|p| p.exposed.clone()).collect();
    let mut broken: Vec<EntityId> = problems.iter().flat_map(|p| p.broken.clone()).collect();
    conflicts.sort();
    redundant.sort();
    exposed.sort_unstable();
    exposed.dedup();
    broken.sort_unstable();
    broken.dedup();
    cache.report.conflicts = conflicts;
    cache.report.redundant = redundant;
    cache.report.exposed = exposed;
    cache.report.broken = broken;
    cache.report.converged = cache.unconverged == 0;
    cache.at = Some((doc.lineage(), doc.revision()));
    cache.reps = ids
        .iter()
        .zip(roots)
        .map(|(id, &r)| (*id, ids[r]))
        .collect();
    cache.unplaced = doc.iter().map(|(_, e)| unplaced(e)).collect();
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
    // (Each row as a solve judges it: in its component's frame, against its bar.)
    let holds = crate::rows_holding(sys, &comps, &constant_rows, tol);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let mut failed_rows: Vec<usize> = Vec::new();
    for &r in &constant_rows {
        if !holds[r] {
            failed_rows.push(r);
        } else if let Some(rep) = row_cluster(r) {
            entry(&mut parts, rep).dependent += 1;
        }
    }
    for comp in &comps {
        if comp.rows.iter().any(|&r| !holds[r]) {
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
            let Part {
                exposed, broken, ..
            } = p;
            crate::exposed_and_broken(sys, &[r], |r| holds[r], exposed, broken);
        }
    }
    for p in parts.values_mut() {
        for list in [&mut p.exposed, &mut p.broken] {
            list.sort_unstable();
            list.dedup();
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

/// `e` with a point's place left out: what a move leaves as it was.
fn unplaced(e: &Entity) -> Entity {
    match e.kind {
        EntityKind::Point(_) => Entity {
            kind: EntityKind::Point(DVec2::ZERO),
            ..*e
        },
        _ => *e,
    }
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
        match n[0] % 19 {
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
            // Ellipses and elliptical arcs keep a shape of their own, read from their points.
            16 => {
                let _ = doc.add_ellipse(
                    pick(&points, n[1]),
                    pick(&points, n[2]),
                    pick(&points, n[3]),
                );
            }
            17 => {
                let _ = doc.add_ellipse_arc(
                    pick(&points, n[1]),
                    pick(&points, n[2]),
                    pick(&points, n[3]),
                    pick(&points, n[4]),
                    pick(&points, n[4].wrapping_add(n[1])),
                );
            }
            // An entity changed in place but not moved: back on the default layer (off the
            // locked one, so its cluster may be free now).
            18 => {
                let _ = doc.set_entity_layer(pick(&ids, n[1]), drift_core::LayerId::DEFAULT);
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
        prop_assert_eq!(&a.exposed, &b.exposed);
        prop_assert_eq!(&a.broken, &b.broken);
        prop_assert_eq!(&a.definitions, &b.definitions);
        Ok(())
    }

    /// What a failed part leaves exposed and broken (#342) comes out of the cache as out of
    /// a whole analysis: a part failing on a conflict next to an arc, a part failing on an
    /// arc's own row alone, after an edit in another part (both kept from the cache), and
    /// after the broken arc is deleted.
    #[test]
    fn cached_exposed_and_broken_are_the_full_analysis() {
        let v = DVec2::new;
        let mut doc = Document::new();
        let mut cache = AnalysisCache::default();
        // A point held 10 from two fixed points 30 apart (a conflict), and an arc centred on
        // it with its end off its circle.
        let a = doc.add_point(v(0.0, 0.0));
        let f = doc.add_point(v(30.0, 0.0));
        let b = doc.add_point(v(15.0, 0.0));
        for (point, at) in [(a, v(0.0, 0.0)), (f, v(30.0, 0.0))] {
            doc.add_constraint(Constraint::Fix { point, at }).unwrap();
        }
        doc.add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        doc.add_constraint(Constraint::Distance {
            a: b,
            b: f,
            value: 10.0,
        })
        .unwrap();
        let s = doc.add_point(v(20.0, 0.0));
        let e = doc.add_point(v(15.0, 6.0));
        let mixed = doc.add_arc(b, s, e).unwrap();
        // (`add_arc` puts the end on the circle.)
        doc.set_point(e, v(15.0, 6.0)).unwrap();
        // An arc whose end fixes hold off its circle.
        let c = doc.add_point(v(100.0, 0.0));
        let s2 = doc.add_point(v(110.0, 0.0));
        let e2 = doc.add_point(v(100.0, 10.0));
        let alone = doc.add_arc(c, s2, e2).unwrap();
        doc.set_point(e2, v(100.0, 12.0)).unwrap();
        for (point, at) in [
            (c, v(100.0, 0.0)),
            (s2, v(110.0, 0.0)),
            (e2, v(100.0, 12.0)),
        ] {
            doc.add_constraint(Constraint::Fix { point, at }).unwrap();
        }
        let check = |doc: &Document, cache: &mut AnalysisCache| {
            let cached = analyze_cached(doc, None, cache).clone();
            let whole = crate::analyze(doc);
            assert_eq!(cached.exposed, whole.exposed);
            assert_eq!(cached.broken, whole.broken);
            cached
        };
        let r = check(&doc, &mut cache);
        assert_eq!(r.exposed, vec![mixed, alone]);
        assert_eq!(r.broken, vec![mixed, alone]);
        // An edit in a part of its own: both failing parts kept from the cache.
        let p = doc.add_point(v(500.0, 0.0));
        let q = doc.add_point(v(510.0, 0.0));
        doc.add_line(p, q).unwrap();
        let r = check(&doc, &mut cache);
        assert_eq!(cache.last_misses(), 1);
        assert_eq!(r.broken, vec![mixed, alone]);
        // The lone arc deleted.
        doc.delete([alone]);
        let r = check(&doc, &mut cache);
        assert_eq!(r.exposed, vec![mixed]);
        assert_eq!(r.broken, vec![mixed]);
    }

    /// #371: a whole move of free lines analyses nothing again, while a cluster with a
    /// relation, an arc, an ellipse, an elliptical arc or geometry on a locked layer is
    /// analysed again when it moves, and so
    /// is one whose entity changed in place (a line moved off the locked layer) or that was
    /// joined to another.
    #[test]
    fn a_move_keeps_only_free_clusters() {
        let v = DVec2::new;
        let mut doc = Document::new();
        let mut cache = AnalysisCache::default();
        let mut free = Vec::new();
        for k in 0..20 {
            let a = doc.add_point(v(k as f64 * 10.0, 0.0));
            let b = doc.add_point(v(k as f64 * 10.0 + 5.0, 3.0));
            free.push(doc.add_line(a, b).unwrap());
        }
        analyze_cached(&doc, None, &mut cache);
        let all: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
        doc.translate(all.clone(), v(2.0, 1.0));
        let r = analyze_cached(&doc, None, &mut cache).clone();
        assert_eq!(cache.last_misses(), 0, "free lines moved");
        assert_eq!(r.dof, crate::analyze(&doc).dof);
        // A fixed line, an arc and a line on a locked layer: each is analysed again.
        let p = doc.add_point(v(0.0, 50.0));
        let q = doc.add_point(v(10.0, 50.0));
        doc.add_line(p, q).unwrap();
        doc.add_constraint(Constraint::Fix {
            point: p,
            at: v(0.0, 50.0),
        })
        .unwrap();
        let c = doc.add_point(v(0.0, 80.0));
        let s = doc.add_point(v(10.0, 80.0));
        let e = doc.add_point(v(0.0, 90.0));
        doc.add_arc(c, s, e).unwrap();
        // An ellipse and an elliptical arc: their shape rows read their points' places.
        let ec = doc.add_point(v(50.0, 80.0));
        let em = doc.add_point(v(60.0, 80.0));
        let en = doc.add_point(v(50.0, 85.0));
        doc.add_ellipse(ec, em, en).unwrap();
        let ac = doc.add_point(v(100.0, 80.0));
        let am = doc.add_point(v(110.0, 80.0));
        let an = doc.add_point(v(100.0, 85.0));
        let a_start = doc.add_point(v(110.0, 80.0));
        let a_end = doc.add_point(v(100.0, 85.0));
        doc.add_ellipse_arc(ac, am, an, a_start, a_end).unwrap();
        let layer = doc.add_layer(Layer {
            locked: true,
            ..Layer::new("Locked")
        });
        let l1 = doc.add_point(v(0.0, 120.0));
        let l2 = doc.add_point(v(10.0, 120.0));
        let pinned = doc.add_line(l1, l2).unwrap();
        doc.set_entity_layer(pinned, layer).unwrap();
        analyze_cached(&doc, None, &mut cache);
        let all: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
        doc.translate(all, v(-1.0, 4.0));
        let r = analyze_cached(&doc, None, &mut cache).clone();
        assert_eq!(
            cache.last_misses(),
            5,
            "the fixed, arc, ellipse, elliptical arc and locked clusters"
        );
        let whole = crate::analyze(&doc);
        assert_eq!(
            (r.converged, r.dof, &r.conflicts, &r.definitions),
            (
                whole.converged,
                whole.dof,
                &whole.conflicts,
                &whole.definitions
            )
        );
        // The pinned line put back on a shown layer that isn't locked (the same entity but
        // its layer): its cluster is free now, and analysed again, not kept as pinned.
        analyze_cached(&doc, None, &mut cache);
        doc.set_entity_layer(pinned, drift_core::LayerId::DEFAULT)
            .unwrap();
        let r = analyze_cached(&doc, None, &mut cache).clone();
        assert_eq!(cache.last_misses(), 1, "the line off the locked layer");
        let whole = crate::analyze(&doc);
        assert_eq!((r.dof, &r.definitions), (whole.dof, &whole.definitions));
        // A free line joined to another (a change, not a move): its cluster is analysed
        // again.
        let Some(EntityKind::Line { b, .. }) = doc.get(free[0]).map(|e| e.kind) else {
            panic!("a line");
        };
        let Some(EntityKind::Line { a: next, .. }) = doc.get(free[1]).map(|e| e.kind) else {
            panic!("a line");
        };
        doc.add_line(b, next).unwrap();
        analyze_cached(&doc, None, &mut cache);
        assert_eq!(cache.last_misses(), 1);
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

        /// #371: moves kept cluster by cluster are still the whole drawing's analysis: points
        /// moved one by one, whole clusters and the whole drawing moved, between random edits
        /// (relations, fixes, arcs, circles, locked layers, kinds changed by deletes and new
        /// curves). A cluster whose analysis reads its points' places (a relation, an arc, a
        /// locked layer) is analysed again; only a free one is kept. Several changes can come
        /// between two analyses, as the editor analyses once per transaction.
        #[test]
        fn moves_kept_by_the_cache_are_the_full_analysis(
            steps in prop::collection::vec(
                (prop::array::uniform5(0u32..10_000), 0u32..6),
                1..80,
            ),
            every in 1usize..4,
        ) {
            let mut doc = Document::new();
            let locked = doc.add_layer(Layer {
                locked: true,
                ..Layer::new("Locked")
            });
            let mut cache = AnalysisCache::default();
            for (k, (n, how)) in steps.iter().enumerate() {
                let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
                let pick = |k: u32| ids[k as usize % ids.len().max(1)];
                let by = DVec2::new(f64::from(n[3] % 7) - 3.0, f64::from(n[4] % 5) - 2.0);
                match how {
                    // A move of one point, of an entity and what it uses, of every entity.
                    0 if !ids.is_empty() => doc.translate([pick(n[1])], by),
                    1 if !ids.is_empty() => {
                        let start = pick(n[1]);
                        let some: Vec<EntityId> =
                            ids.iter().copied().filter(|id| *id >= start).collect();
                        doc.translate(some, by);
                    }
                    2 => doc.translate(ids.clone(), by),
                    // Anything else.
                    _ => change(&mut doc, locked, n),
                }
                if k % every == 0 || k + 1 == steps.len() {
                    let cached = analyze_cached(&doc, None, &mut cache).clone();
                    same(&cached, &crate::analyze(&doc))?;
                    let fresh = analyze_cached(&doc, None, &mut AnalysisCache::default()).clone();
                    same(&cached, &fresh)?;
                }
            }
        }
    }
}
