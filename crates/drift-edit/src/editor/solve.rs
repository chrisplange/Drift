//! Solving and committing: the solver scope of an edit, undo/redo, and the moves and
//! turns that go through the solver.

use super::*;

/// What a line's length weighs in an edit's solve against its direction and the arcs' radii,
/// where they can't all be met (see [`Editor::kept_shapes`]): a line keeps its direction
/// first. (Weights, not a strict order: where a length must change, its direction and the
/// radii beside it give way by about this share of the change, micrometres in a change of
/// millimetres.)
const LENGTH_YIELD: f64 = 1e-3;

/// What the lengths of the lines a new relation names weigh against
/// the rest's lengths (see [`Editor::kept_shapes`]): the named take up the change.
const NAMED_YIELD: f64 = 1e-3;

/// The shapes an edit's solve keeps where the relations leave room (see
/// [`Editor::kept_shapes`]), as the solver takes them.
#[derive(Default)]
pub(crate) struct KeptShapes {
    pub lengths: HashMap<EntityId, f64>,
    pub length_ranks: HashMap<EntityId, f64>,
    /// As before the audit's PR 6 (see [`drift_solver::SolveOptions::shapes_as_before`]).
    pub as_before: bool,
    pub directions: HashMap<EntityId, DVec2>,
    pub radii: HashMap<EntityId, f64>,
}

impl Editor {
    /// The entities a drag of `ids` can move (cached while the make-up is unchanged: see
    /// [`drag_scope_and_parts`](Self::drag_scope_and_parts)).
    pub(crate) fn drag_scope(
        &self,
        ids: &[EntityId],
    ) -> std::rc::Rc<std::collections::HashSet<EntityId>> {
        self.drag_scope_and_parts(ids).0
    }

    /// [`drag_scope`](Self::drag_scope), and the same split into the parts nothing joins to
    /// one another. Kept while the drawing's make-up apart from fills is the same
    /// ([`Document::make_up`](drift_core::Document::make_up)): what joins what is the entities'
    /// points and the relations (walls' lowered ones too), none of it a fill, so a drag that
    /// carries a hatch along (rewriting it every frame) keeps it.
    pub(crate) fn drag_scope_and_parts(
        &self,
        ids: &[EntityId],
    ) -> (std::rc::Rc<std::collections::HashSet<EntityId>>, DragParts) {
        let structure = self.doc.make_up();
        if let Some((key, s, scope, parts)) = &*self.drag_scope.borrow()
            && key == ids
            && *s == structure
            // Still the same geometry: a move that let go of shared points and was then
            // applied again from the original geometry has new points, in the same number.
            && self
                .doc
                .defining_points(ids.iter().copied())
                .iter()
                .all(|p| scope.contains(p))
        {
            return (scope.clone(), parts.clone());
        }
        let parts: Vec<std::rc::Rc<std::collections::HashSet<EntityId>>> =
            drift_solver::connected_parts_with(
                &self.doc,
                ids.iter().copied(),
                self.lowering().as_deref(),
            )
            .into_iter()
            .map(|part| std::rc::Rc::new(part.into_iter().collect()))
            .collect();
        let scope: std::rc::Rc<std::collections::HashSet<EntityId>> =
            std::rc::Rc::new(parts.iter().flat_map(|p| p.iter().copied()).collect());
        let parts = std::rc::Rc::new(parts);
        *self.drag_scope.borrow_mut() =
            Some((ids.to_vec(), structure, scope.clone(), parts.clone()));
        (scope, parts)
    }

    pub(crate) fn solve_in(
        &mut self,
        dragged: &[Param],
        scope: std::rc::Rc<std::collections::HashSet<EntityId>>,
    ) -> SolveReport {
        let options = SolveOptions {
            dragged: dragged.to_vec(),
            scope: Some(scope),
            lowering: self.lowering(),
            ..Default::default()
        };
        drift_solver::solve(&mut self.doc, &options)
    }

    /// Solve with `dragged` held where they are, and no shape kept: what a drag of a circle's
    /// radius or of an offset's side solves each step ([`drag_radius`](Self::drag_radius),
    /// [`drag_offset`](Self::drag_offset)). An edit's solve is
    /// [`solve_commit`](Self::solve_commit).
    pub(crate) fn solve(&mut self, dragged: &[Param]) -> SolveReport {
        let options = SolveOptions {
            dragged: dragged.to_vec(),
            lowering: self.lowering(),
            ..Default::default()
        };
        drift_solver::solve(&mut self.doc, &options)
    }

    /// An edit's solve: the relations met, and every shape kept where they leave room (see
    /// [`kept_shapes`](Self::kept_shapes)). From geometry where the relations already hold
    /// (the commit after a drag, or after a relation's own solve) it takes no step.
    pub(crate) fn solve_commit(&mut self) -> SolveReport {
        let kept = self.kept_shapes(&HashSet::new());
        let options = SolveOptions {
            lowering: self.lowering(),
            lengths: kept.lengths,
            length_ranks: kept.length_ranks,
            shapes_as_before: kept.as_before,
            directions: kept.directions,
            radii: kept.radii,
            ..Default::default()
        };
        drift_solver::solve(&mut self.doc, &options)
    }

    /// The shapes an edit's solve keeps where the relations leave room, as the geometry
    /// stands (the audit's PR 6, #350). The solver meets the relations first and only then,
    /// among their solutions, comes as near these as it can, so a relation that sets a size
    /// or turns a line changes it, and nothing changes that it leaves free:
    /// - every arc keeps its radius, and every line its direction (the sketch stays square);
    /// - a line keeps its length where keeping it moves nothing else: where it hangs from the
    ///   rest (an end no other curve and no relation uses, as in a drag). A line between two
    ///   ends the rest of the sketch holds keeps its direction and stretches along itself where
    ///   they part (a dimension typed on an outline's side moves the side across and stretches
    ///   the two beside it), rather than carry the rest of the sketch along to keep its length:
    ///   a relation moves what it relates, not the whole sketch;
    /// - geometry a new relation names (`turning`) takes up its change by moving and turning:
    ///   its lines keep their lengths but not their directions (a line made parallel to another
    ///   turns, and neither stretches), and its arcs take up the change in their radii (a
    ///   fillet made concentric with a round grows inside lines that stay square, #75);
    /// - a size a dimension sets is the dimension's, not a preference.
    ///
    /// Where they can't all be met, a length ranks below a direction or a radius
    /// ([`LENGTH_YIELD`]), and a length of what the relation names lower still
    /// ([`NAMED_YIELD`]). What a solve holds where it is (the active element, carried) ranks
    /// above all of these.
    pub(crate) fn kept_shapes(&self, turning: &HashSet<EntityId>) -> KeptShapes {
        let mut kept = KeptShapes::default();
        // Only what the relations reach (through shared points and relations) can move in an
        // edit's solve: the rest of a big drawing (imported linework, say) is left out, which
        // changes nothing but what the solve has to look through.
        let seeds: Vec<EntityId> = self
            .doc
            .constraints()
            .filter(|(id, _)| !self.doc.is_driven(*id))
            .flat_map(|(_, c)| c.entities().collect::<Vec<_>>())
            .collect();
        if seeds.is_empty() {
            return kept;
        }
        let lowering = self.lowering();
        let reached = drift_solver::connected_with(&self.doc, seeds, lowering.as_deref());
        let curves: Vec<(EntityId, EntityKind)> = reached
            .iter()
            .filter_map(|id| self.doc.get(*id).map(|e| (*id, e.kind)))
            .filter(|(_, kind)| !kind.is_point())
            .collect();
        // How many curves use each point, and which points relations name: a line with an
        // end used by it alone, and by no relation, hangs.
        let mut uses: HashMap<EntityId, usize> = HashMap::new();
        for (_, kind) in &curves {
            for p in kind.points() {
                *uses.entry(p).or_default() += 1;
            }
        }
        let related: HashSet<EntityId> = self
            .doc
            .constraints()
            .flat_map(|(_, c)| c.entities().collect::<Vec<_>>())
            .collect();
        let hangs = |p: EntityId| uses.get(&p) == Some(&1) && !related.contains(&p);
        // Sizes a dimension sets are the dimension's, not preferences: the lines whose ends a
        // distance holds apart, and the arcs a radius or diameter sizes.
        let mut sized: HashSet<(EntityId, EntityId)> = HashSet::new();
        let mut sized_arcs: HashSet<EntityId> = HashSet::new();
        for (_, c) in self
            .doc
            .constraints()
            .filter(|(id, _)| !self.doc.is_driven(*id))
        {
            match *c {
                Constraint::Distance { a, b, .. } => {
                    sized.insert((a.min(b), a.max(b)));
                }
                Constraint::Radius { curve, .. }
                | Constraint::Diameter { curve, .. }
                | Constraint::FixRadius { curve, .. } => {
                    sized_arcs.insert(curve);
                }
                _ => {}
            }
        }
        for &(id, kind) in &curves {
            match kind {
                EntityKind::Line { a, b } => {
                    let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b)) else {
                        continue;
                    };
                    let turns = turning.contains(&id);
                    if (turns || hangs(a) || hangs(b)) && !sized.contains(&(a.min(b), a.max(b))) {
                        kept.lengths.insert(id, pa.distance(pb));
                        let rank = if turns {
                            LENGTH_YIELD * NAMED_YIELD
                        } else {
                            LENGTH_YIELD
                        };
                        kept.length_ranks.insert(id, rank);
                    }
                    if !turns && let Some(u) = (pb - pa).try_normalize() {
                        kept.directions.insert(id, u);
                    }
                }
                EntityKind::Arc { .. } if !turning.contains(&id) && !sized_arcs.contains(&id) => {
                    if let Some(r) = drift_solver::radius_of(&self.doc, id) {
                        kept.radii.insert(id, r);
                    }
                }
                _ => {}
            }
        }
        kept
    }

    /// The shapes a relation's solve kept before the audit's PR 6: the directions of the
    /// lines and the radii of the arcs the relation doesn't name (`named`), no lengths. What
    /// a relation is solved with where keeping every shape ([`kept_shapes`](Self::kept_shapes))
    /// would leave it refused (see `Editor::check_new_constraints`).
    pub(crate) fn kept_shapes_as_before(&self, named: &HashSet<EntityId>) -> KeptShapes {
        let mut kept = KeptShapes {
            as_before: true,
            ..Default::default()
        };
        for (id, e) in self.doc.iter().filter(|(id, _)| !named.contains(id)) {
            match e.kind {
                EntityKind::Line { a, b } => {
                    if let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b))
                        && let Some(u) = (pb - pa).try_normalize()
                    {
                        kept.directions.insert(id, u);
                    }
                }
                EntityKind::Arc { .. } => {
                    if let Some(r) = drift_solver::radius_of(&self.doc, id) {
                        kept.radii.insert(id, r);
                    }
                }
                _ => {}
            }
        }
        kept
    }

    /// Relations on walls' faces lowered onto their centrelines, for the solver (None when
    /// no relation names a wall's generated geometry). Worked out again whenever the
    /// document changes, so a thickness edit in the same transaction is seen.
    pub fn lowering(&self) -> Option<std::rc::Rc<drift_solver::Lowering>> {
        if !self.doc.has_walls() {
            return None;
        }
        let revision = self.doc.revision();
        if let Some((r, l)) = &*self.lowering_cache.borrow()
            && *r == revision
        {
            return l.clone();
        }
        let l = drift_geom::walls::lower(&self.doc);
        let l = (!l.is_empty()).then(|| std::rc::Rc::new(l));
        *self.lowering_cache.borrow_mut() = Some((revision, l.clone()));
        l
    }

    /// True if the edit collapsed geometry that wasn't collapsed before.
    pub(super) fn new_degeneracy(&self, before: &[EntityId]) -> bool {
        drift_solver::degenerate_entities(&self.doc)
            .iter()
            .any(|id| !before.contains(id))
    }

    /// True if `report` has conflicts that weren't there before this edit (a document that was
    /// already conflicting shouldn't make every later edit fail), or left geometry broken that
    /// wasn't in a failing part before it (an arc's end off its circle, a line's midpoint off
    /// it: rows no relation owns, #342), or wrote nothing (a value that isn't a number).
    ///
    /// Geometry in a part the drawing's analysis already finds failing isn't blamed on the
    /// edit, even if the edit breaks it further ([`SolveReport::exposed`]): a solve of such a
    /// part shares its misfit with it whatever the edit did.
    pub(crate) fn new_conflicts(&self, report: &SolveReport) -> bool {
        !report.converged
            && (report.unwritten || self.new_conflict_named(report) || self.newly_failing(report))
    }

    /// Whether `report` names a conflict the drawing didn't have before this edit.
    fn new_conflict_named(&self, report: &SolveReport) -> bool {
        report
            .conflicts
            .iter()
            .any(|c| !self.analysis().conflicts.contains(c))
    }

    /// Whether `report` puts geometry with rows no relation owns (an arc, a midpoint line, an
    /// ellipse, a wall end) into a failed part that no failing part held before this edit
    /// ([`SolveReport::exposed`], against the drawing's analysis). No residual is compared:
    /// in a failed part the solver stops at its iteration cap, not at the compromise, so how
    /// far such a row is left off is where it stopped, anywhere from round-off to thousands
    /// of times its tolerance (#342).
    fn newly_failing(&self, report: &SolveReport) -> bool {
        let before = &self.analysis().exposed;
        report
            .exposed
            .iter()
            .any(|e| before.binary_search(e).is_err())
    }

    /// True if `report`'s solve didn't finish: it didn't converge, and not only on conflicts
    /// the document already had (which shouldn't stop every later drag, as for
    /// [`new_conflicts`](Self::new_conflicts)). A solve that fails naming nothing, or on rows
    /// no relation owns (an arc's end off its circle), counts too: it left geometry broken
    /// (#281); so does one that newly breaks such geometry in a part that already conflicted,
    /// or wrote nothing (#342).
    pub(crate) fn unfinished(&self, report: &SolveReport) -> bool {
        !report.converged
            && (report.unsourced_failure
                || report.unwritten
                || report.conflicts.is_empty()
                || self.new_conflict_named(report)
                || self.newly_failing(report))
    }

    /// Solve the constraints, then finish the open transaction and record it for undo. If the
    /// edit made the constraints unsatisfiable it is rolled back and `false` returned.
    /// [`commit`](Self::commit) an import: its fills come in as they were drawn, area fills
    /// staying area fills (a CAD hatch is a boundary hatch), as when a drawing is opened.
    pub fn commit_import(&mut self, label: &str) -> bool {
        self.importing = true;
        let done = self.commit(label);
        self.importing = false;
        done
    }

    pub fn commit(&mut self, label: &str) -> bool {
        // An edit begun while the last one's fills were still to follow it: they follow
        // first, in that edit's own step.
        if self.areas.pending.is_some() && self.doc.in_transaction() {
            self.settle_before_open_edit();
        }
        // (Free lines alone have nothing to solve: no relations, nothing pinned or round.)
        let report = if drift_solver::anything_to_solve(&self.doc) {
            self.solve_commit()
        } else {
            SolveReport {
                converged: true,
                ..Default::default()
            }
        };
        if self.new_conflicts(&report) {
            self.doc.rollback();
            self.out.notice = Some(format!("{label} undone: it {}", EditError::Conflict));
            self.sync();
            return false;
        }
        self.refresh_driven();
        // (Before adopting: a copy made in a block edit joins it as curves.)
        self.explode_copies_in_block_edit();
        self.adopt_into_open_group();
        self.make_new_curves_construction();
        self.follow_walls();
        // (After the walls: an offset's source may be a wall's face.)
        self.follow_offsets();
        self.follow_paths();
        // On a big drawing the edit is done before the areas its fills follow are found:
        // they are found on a thread, and the fills then follow in this edit's step.
        if self.defer_follow() {
            let tx = self.doc.commit(label);
            self.out.far_off = self.far_off_of(tx.entities());
            self.history.push(tx);
            self.areas.pending = Some(regions::PendingFollow {
                depth: self.history.undo_depth(),
                recorded: self.history.recorded(),
            });
            self.sync();
            return true;
        }
        self.follow_fills();
        self.carry_hatches();
        let tx = self.doc.commit(label);
        debug_assert!(
            self.doc
                .constraints()
                .all(|(_, c)| self.doc.validate(c).is_ok()),
            "{label}: a relation refers to geometry of the wrong kind: {:?}",
            drift_solver::invalid_constraints(&self.doc)
        );
        self.out.far_off = self.far_off_of(tx.entities());
        self.history.push(tx);
        self.sync();
        self.check_caches();
        true
    }

    /// Update driven dimensions to the geometry's current measurements (inside a transaction).
    pub(super) fn refresh_driven(&mut self) {
        let driven: Vec<_> = self
            .doc
            .constraints()
            .filter(|(id, _)| self.doc.is_driven(*id))
            .map(|(id, c)| (id, *c))
            .collect();
        for (id, c) in driven {
            if let Some(value) = drift_solver::measure(&self.doc, &c)
                && c.value() != Some(value)
            {
                let measured = c.with_value(value);
                if measured.value_is_valid() {
                    let _ = self.doc.set_constraint(id, measured);
                }
            }
        }
    }

    /// The value to show for a dimension: its own for a driving one, the current measurement for
    /// a driven one (which may be mid-drag, before the commit refreshes it).
    pub fn dimension_value(&self, id: ConstraintId) -> Option<f64> {
        let c = self.doc.constraint(id)?;
        if self.doc.is_driven(id) {
            drift_solver::measure(&self.doc, c).or(c.value())
        } else {
            c.value()
        }
    }

    /// Where the points of `scope` are and the sizes of its circles and arcs, for
    /// [`restore_geometry`](Self::restore_geometry) and to keep rounds their size.
    pub(super) fn geometry_of(&self, scope: &std::collections::HashSet<EntityId>) -> Geometry {
        let mut out = Geometry::default();
        for &id in scope {
            match self.doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Point(p)) => out.points.push((id, p)),
                Some(EntityKind::Circle { radius, .. }) => out.circles.push((id, radius)),
                Some(EntityKind::Arc { .. }) => {
                    if let Some(r) = drift_solver::radius_of(&self.doc, id) {
                        out.radii.insert(id, r);
                    }
                }
                _ => {}
            }
        }
        // (In id order, not the set's: sums over them come out the same every run.)
        out.points.sort_unstable_by_key(|&(id, _)| id);
        out.circles.sort_unstable_by_key(|&(id, _)| id);
        out
    }

    pub(super) fn restore_geometry(&mut self, geometry: &Geometry) {
        for &(id, p) in &geometry.points {
            if self.doc.point(id).ok() != Some(p) {
                let _ = self.doc.set_point(id, p);
            }
        }
        for &(id, r) in &geometry.circles {
            if drift_solver::radius_of(&self.doc, id) != Some(r) {
                let _ = self.doc.set_radius(id, r);
            }
        }
    }

    /// Resize a circle inside an open transaction, keeping constraints satisfied (the radius
    /// follows the pointer; constraints move the rest). Returns false, leaving it as it was, if
    /// the size is impossible (e.g. fixed by a dimension).
    pub fn drag_radius(&mut self, circle: EntityId, radius: f64) -> bool {
        let Some(old) = drift_solver::radius_of(&self.doc, circle) else {
            return false;
        };
        if self.doc.set_radius(circle, radius).is_err() {
            return false;
        }
        let dragged = [Param::Radius(circle)];
        let report = self.solve(&dragged);
        let ok = !self.new_conflicts(&report)
            && drift_solver::radius_of(&self.doc, circle)
                .is_some_and(|r| (r - radius).abs() <= 1e-6 * radius.max(1.0));
        if !ok {
            let _ = self.doc.set_radius(circle, old);
            self.solve(&dragged);
        }
        self.index.update(&self.doc);
        ok
    }

    /// Set an offset's distance (its dimensions, `family`; see [`Editor::offset_family`])
    /// inside an open transaction, as a drag of one of its sides does: every side moves, and
    /// the curves they are measured from stay. False, with nothing changed, if the drawing
    /// can't take it.
    pub fn drag_offset(&mut self, family: &[ConstraintId], distance: f64) -> bool {
        if !(distance > 0.0 && distance.is_finite()) {
            return false;
        }
        let was: Vec<(ConstraintId, Constraint, Option<String>)> = family
            .iter()
            .filter_map(|d| {
                let c = *self.doc.constraint(*d)?;
                Some((*d, c, self.doc.expression(*d).map(str::to_owned)))
            })
            .collect();
        let from = was.iter().filter_map(|(_, c, _)| match c {
            Constraint::PointLineDistance { line, .. } => Some(*line),
            _ => None,
        });
        let held: Vec<Param> = self
            .doc
            .defining_points(from)
            .into_iter()
            .flat_map(Param::point)
            .collect();
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        // (A value set here replaces an expression the distance was linked to.)
        let set = was.iter().all(|(d, c, _)| {
            self.doc.set_expression(*d, None);
            self.doc.set_constraint(*d, c.with_value(distance)).is_ok()
        });
        let ok = set && {
            let report = self.solve(&held);
            !self.new_conflicts(&report) && !self.new_degeneracy(&degenerate_before)
        };
        if !ok {
            for (d, c, e) in &was {
                let _ = self.doc.set_constraint(*d, *c);
                self.doc.set_expression(*d, e.clone());
            }
            self.solve(&held);
        }
        self.index.update(&self.doc);
        ok
    }

    /// Rotate geometry about `pivot` by `degrees` (counter-clockwise), inside an open
    /// transaction. `points` are the selection's defining points; they turn rigidly while
    /// constraints move the rest.
    ///
    /// Horizontal / vertical relations and dimensions entirely within the rotated set are
    /// removed (they can't hold at an arbitrary angle) and returned, so a quarter-turn can put
    /// them back; fixed points are re-fixed where they land. Returns false if the solve
    /// conflicts.
    pub fn rotate_geometry(
        &mut self,
        ids: &[EntityId],
        points: &BTreeSet<EntityId>,
        pivot: DVec2,
        degrees: f64,
    ) -> (bool, Vec<Constraint>) {
        let rotor = exact_rotor(degrees);
        let turn = |p: DVec2| pivot + rotor.rotate(p - pivot);
        let inside: BTreeSet<EntityId> =
            ids.iter().copied().chain(points.iter().copied()).collect();
        // Only what's connected to the turned geometry is solved (worked out before any
        // relations are lifted, so it's the same every move).
        let seeds: Vec<EntityId> = inside.iter().copied().collect();
        let scope = self.drag_scope(&seeds);
        let internal: Vec<(ConstraintId, Constraint)> = self
            .doc
            .constraints()
            .filter(|(_, c)| c.entities().all(|e| inside.contains(&e)))
            .map(|(id, c)| (id, *c))
            .collect();
        let mut removed = Vec::new();
        for (id, c) in internal {
            match c {
                Constraint::Horizontal { .. }
                | Constraint::Vertical { .. }
                | Constraint::HorizontalPoints { .. }
                | Constraint::VerticalPoints { .. }
                | Constraint::HorizontalDistance { .. }
                | Constraint::VerticalDistance { .. } => {
                    self.doc.remove_constraint(id);
                    removed.push(c);
                }
                Constraint::Fix { point, at } => {
                    let _ = self.doc.set_constraint(
                        id,
                        Constraint::Fix {
                            point,
                            at: turn(at),
                        },
                    );
                }
                // A point held by its x or y alone keeps that, where it lands.
                Constraint::FixX { point, .. } | Constraint::FixY { point, .. } => {
                    if let Ok(at) = self.doc.point(point) {
                        let at = turn(at);
                        let pin = match c {
                            Constraint::FixX { .. } => Constraint::FixX { point, x: at.x },
                            _ => Constraint::FixY { point, y: at.y },
                        };
                        let _ = self.doc.set_constraint(id, pin);
                    }
                }
                _ => {}
            }
        }
        for p in points {
            if let Ok(at) = self.doc.point(*p) {
                let _ = self.doc.set_point(*p, turn(at));
            }
        }
        let dragged: Vec<Param> = points.iter().copied().flat_map(Param::point).collect();
        let report = self.solve_in(&dragged, scope.clone());
        let ok = !self.new_conflicts(&report);
        self.index.update_moved(&self.doc, scope.iter().copied());
        self.carry_hatches();
        (ok, removed)
    }

    /// Put back relations removed by [`rotate_geometry`](Self::rotate_geometry) after a turn by
    /// a multiple of 90° (horizontal and vertical swap on a quarter turn). Dimensions take the
    /// geometry's current values.
    pub fn restore_after_rotation(&mut self, removed: &[Constraint], degrees: f64) {
        let quarters = (degrees / 90.0).round() as i64;
        let swap = quarters.rem_euclid(2) == 1;
        for c in removed {
            let c = if swap {
                match *c {
                    Constraint::Horizontal { line } => Constraint::Vertical { line },
                    Constraint::Vertical { line } => Constraint::Horizontal { line },
                    Constraint::HorizontalPoints { a, b } => Constraint::VerticalPoints { a, b },
                    Constraint::VerticalPoints { a, b } => Constraint::HorizontalPoints { a, b },
                    Constraint::HorizontalDistance { a, b, value } => {
                        Constraint::VerticalDistance { a, b, value }
                    }
                    Constraint::VerticalDistance { a, b, value } => {
                        Constraint::HorizontalDistance { a, b, value }
                    }
                    other => other,
                }
            } else {
                *c
            };
            let c = match drift_solver::measure(&self.doc, &c) {
                Some(v) if c.is_dimension() => c.with_value(v),
                _ => c,
            };
            let _ = self.doc.add_constraint(c);
        }
    }

    /// Whether the drawing is big enough that its areas are found off the edit path: as
    /// `areas.defer` says, or by its size (never on the web).
    pub(crate) fn areas_off_the_edit_path(&self) -> bool {
        /// Entities (points and curves) from which a drawing is big.
        const BIG: usize = 30_000;
        !cfg!(target_arch = "wasm32") && self.areas.defer.unwrap_or(self.doc.len() >= BIG)
    }

    /// Whether the open edit commits before its fills follow it (see
    /// [`settle_areas`](Self::settle_areas)): on a big drawing (or as `areas.defer` says),
    /// when it has fills that may follow, and the areas of its curves aren't known yet. Their
    /// finding is then started on a thread.
    fn defer_follow(&mut self) -> bool {
        if self.importing
            || !self.areas_off_the_edit_path()
            || self.doc.fills().all(|(_, f)| f.path.is_some())
        {
            return false;
        }
        let key = self.regions_key();
        if self.regions.as_ref().is_some_and(|(k, _)| *k == key) {
            return false;
        }
        let input = self.areas_input(&key);
        if self.areas.knows(&input) {
            return false;
        }
        let prior = self.regions.as_ref().map(|(_, r)| r.clone());
        self.areas.start(input, prior);
        true
    }

    /// Let the fills of an edit committed before its areas were found follow it now (waiting
    /// for the areas if need be), in that edit's undo step.
    ///
    /// Steps pushed on top of it since (an edit made of several commits) are set aside
    /// first and made again over its followed fills, each still a step of its own. Where
    /// steps were squashed or merged into it, its fills follow in the step that holds it.
    pub fn settle_areas(&mut self) {
        let Some(pending) = self.areas.pending.take() else {
            return;
        };
        if self.doc.in_transaction() {
            self.areas.pending = Some(pending);
            return;
        }
        let depth = self.history.undo_depth();
        let pushed = self.history.recorded().checked_sub(pending.recorded);
        let later = match depth.checked_sub(pending.depth) {
            Some(n) if n > 0 && pushed == Some(n as u64) => n,
            _ => 0,
        };
        let mut aside = Vec::new();
        for _ in 0..later {
            let Some(tx) = self.history.take_last() else {
                break;
            };
            let mut back = History::default();
            back.put_back(tx.clone());
            back.undo(&mut self.doc);
            aside.push(tx);
        }
        if let Some(tx) = self.history.take_last() {
            let label = tx.label.clone();
            self.doc.reopen(tx);
            self.follow_fills();
            self.carry_hatches();
            let tx = self.doc.commit(label);
            self.history.put_back(tx);
        }
        for tx in aside.into_iter().rev() {
            self.doc.begin();
            self.doc.replay(&tx);
            let tx = self.doc.commit(tx.label.clone());
            self.history.put_back(tx);
        }
        // (The shapes shown meanwhile were the fills' before the edit.)
        self.fill_shapes = None;
        self.sync();
        self.check_caches();
    }

    /// Once a frame: the fills of an edit committed before its areas were found follow it
    /// as soon as the areas are in. True while they still wait (draw again soon).
    pub fn poll_areas(&mut self) -> bool {
        if self.areas.pending.is_none() {
            return false;
        }
        if self.areas.worker_done() {
            self.settle_areas();
        }
        self.areas.pending.is_some()
    }

    /// [`settle_areas`](Self::settle_areas) under an edit already begun: the edit is set
    /// aside, the last one's fills follow it, and the edit is made again over them.
    fn settle_before_open_edit(&mut self) {
        let open = self.doc.commit("");
        let mut aside = History::default();
        aside.push(open.clone());
        aside.undo(&mut self.doc);
        self.settle_areas();
        self.doc.begin();
        self.doc.replay(&open);
    }

    pub fn undo(&mut self) -> bool {
        self.settle_areas();
        let done = self.history.undo(&mut self.doc);
        // (Past the start of a block edit or a group's making, nothing is open any more;
        // back past its end, the block edit is open again.)
        self.drop_lost_edit();
        self.resume_block_edit();
        // (Fills are back as they were: whether each was kept whole across curves is found
        // again when it is next looked at, and shapes' areas and holes now.)
        self.fill_crossed.clear();
        self.refresh_shape_fills();
        self.sync();
        self.check_caches();
        done
    }

    pub fn redo(&mut self) -> bool {
        self.settle_areas();
        let done = self.history.redo(&mut self.doc);
        // (Past the start of a block edit or a group's making, nothing is open any more;
        // back past its end, the block edit is open again.)
        self.drop_lost_edit();
        self.resume_block_edit();
        // (Fills are back as they were: whether each was kept whole across curves is found
        // again when it is next looked at, and shapes' areas and holes now.)
        self.fill_crossed.clear();
        self.refresh_shape_fills();
        self.sync();
        self.check_caches();
        done
    }

    /// Move a point to typed coordinates (one undo step), its relations pulling the rest
    /// along. False, with nothing changed, if they hold it somewhere else.
    pub fn move_point_to(&mut self, point: EntityId, to: DVec2) -> bool {
        let Ok(from) = self.doc.point(point) else {
            return false;
        };
        if from == to {
            return true;
        }
        self.doc.begin();
        let dragged: Vec<Param> = Param::point(point).into_iter().collect();
        if !self.drag(&[point], to - from, &dragged) {
            // Too far for one solve: go in steps.
            const STEPS: u32 = 16;
            for _ in 0..STEPS {
                self.drag(&[point], (to - from) / f64::from(STEPS), &dragged);
            }
        }
        // A drag follows the target closely rather than exactly: close what's left.
        for _ in 0..4 {
            let Ok(now) = self.doc.point(point) else {
                break;
            };
            if now.distance(to) < 1e-10 {
                break;
            }
            self.drag(&[point], to - now, &dragged);
        }
        let allowed = 1e-6 * (1.0 + (to - from).length());
        let reached = self
            .doc
            .point(point)
            .is_ok_and(|p| p.distance(to) <= allowed);
        if !reached {
            self.doc.rollback();
            self.sync();
            return false;
        }
        self.commit("Move point")
    }

    /// Turn a line about its start to an angle (degrees anticlockwise from +X), keeping its
    /// length. False if its relations hold its direction.
    pub fn set_line_angle(&mut self, line: EntityId, degrees: f64) -> bool {
        let Some(EntityKind::Line { a, b }) = self.doc.get(line).map(|e| e.kind) else {
            return false;
        };
        let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b)) else {
            return false;
        };
        let to = pa + DVec2::from_angle(degrees.to_radians()) * pa.distance(pb);
        self.move_point_to(b, to)
    }

    /// Move the selection (curves, block copies, the picked text and annotations) by `delta`
    /// as one undo step. `None` with nothing picked; `Some(false)` if relations hold it in
    /// place.
    pub fn nudge(&mut self, delta: DVec2) -> Option<bool> {
        let picked: Vec<EntityId> = self.selection.iter().copied().collect();
        let instances = self.picked_instances();
        let riders = self.riders();
        if picked.is_empty() && instances.is_empty() && riders.is_empty() {
            return None;
        }
        // (An offset's pieces follow its source; alone, they can't be moved.)
        let ids = self.without_offset_pieces(&picked);
        if ids.is_empty()
            && instances.is_empty()
            && riders.is_empty()
            && !self.guard_owned(&picked, crate::walls::Op::Move)
        {
            return Some(false);
        }
        self.doc.begin();
        self.move_instances(&instances, delta);
        if !ids.is_empty() {
            let dragged: Vec<Param> = self
                .doc
                .defining_points(ids.iter().copied())
                .iter()
                .copied()
                .flat_map(Param::point)
                .collect();
            if !self.drag(&ids, delta, &dragged) {
                self.doc.rollback();
                self.sync();
                return Some(false);
            }
        }
        self.place_riders(&riders, &|p| p + delta);
        if let Some(seed) = &mut self.selected_region {
            *seed += delta;
        }
        Some(self.commit("Nudge"))
    }
}
