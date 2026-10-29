//! Solving and committing: the solver scope of an edit, undo/redo, and the moves and
//! turns that go through the solver.

use super::*;

impl Editor {
    /// The entities a drag of `ids` can move (cached while the structure is unchanged).
    pub(crate) fn drag_scope(
        &self,
        ids: &[EntityId],
    ) -> std::rc::Rc<std::collections::HashSet<EntityId>> {
        self.drag_scope_and_parts(ids).0
    }

    /// [`drag_scope`](Self::drag_scope), and the same split into the parts nothing joins to
    /// one another.
    pub(crate) fn drag_scope_and_parts(
        &self,
        ids: &[EntityId],
    ) -> (std::rc::Rc<std::collections::HashSet<EntityId>>, DragParts) {
        let structure = self.doc.structure();
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
        let lowering = self.lowering();
        drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                dragged: dragged.to_vec(),
                scope: Some(scope),
                lowering,
                ..Default::default()
            },
        )
    }

    pub(crate) fn solve(&mut self, dragged: &[Param]) -> SolveReport {
        let lowering = self.lowering();
        drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                dragged: dragged.to_vec(),
                lowering,
                ..Default::default()
            },
        )
    }

    /// Relations on walls' faces lowered onto their centrelines, for the solver (None when
    /// no relation names a wall's generated geometry). Worked out again whenever the
    /// document changes, so a thickness edit in the same transaction is seen.
    pub(crate) fn lowering(&self) -> Option<std::rc::Rc<drift_solver::Lowering>> {
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
    /// already conflicting shouldn't make every later edit fail).
    pub(crate) fn new_conflicts(&self, report: &SolveReport) -> bool {
        !report.converged
            && report
                .conflicts
                .iter()
                .any(|c| !self.analysis().conflicts.contains(c))
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
            self.solve(&[])
        } else {
            SolveReport {
                converged: true,
                ..Default::default()
            }
        };
        if self.new_conflicts(&report) {
            self.doc.rollback();
            self.notice = Some(format!("{label} undone: it {}", EditError::Conflict));
            self.sync();
            return false;
        }
        self.refresh_driven();
        self.adopt_into_open_group();
        self.make_new_curves_construction();
        self.follow_walls();
        self.follow_paths();
        // On a big drawing the edit is done before the areas its fills follow are found:
        // they are found on a thread, and the fills then follow in this edit's step.
        if self.defer_follow() {
            let tx = self.doc.commit(label);
            self.far_off = self.far_off_of(tx.entities());
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
        self.far_off = self.far_off_of(tx.entities());
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
            self.doc.reopen(&tx);
            self.follow_fills();
            self.carry_hatches();
            let tx = self.doc.commit(tx.label.clone());
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
        let ids: Vec<EntityId> = self.selection.iter().copied().collect();
        let instances = self.picked_instances();
        let riders = self.riders();
        if ids.is_empty() && instances.is_empty() && riders.is_empty() {
            return None;
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
