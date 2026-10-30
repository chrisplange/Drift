//! Adding, checking and removing relations and dimensions.

use super::*;

impl Editor {
    /// Add constraints as one undoable step. Refused if they conflict with the existing ones
    /// or add nothing new.
    pub fn add_constraints(
        &mut self,
        label: &str,
        constraints: &[Constraint],
    ) -> Result<Vec<ConstraintId>, EditError> {
        // A wall's faces hold relations that can be lowered onto its centreline; its
        // corners follow its joins.
        if let Some(why) = constraints.iter().find_map(|c| self.refused_on_walls(c)) {
            return Err(EditError::Invalid(why));
        }
        // Fix where it is (a fix that moves geometry, from a script say, is solved as below).
        if !constraints.is_empty() && constraints.iter().all(|c| self.fixes_in_place(c)) {
            return self.add_fixes(label, constraints);
        }
        let before = self.analysis().clone();
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        self.doc.begin();
        let mut ids = Vec::new();
        for c in constraints {
            match self.doc.add_constraint(*c) {
                Ok(id) => ids.push(id),
                Err(e) => {
                    self.doc.rollback();
                    self.sync();
                    return Err(EditError::Invalid(e.to_string()));
                }
            }
            self.settle_fillet(c);
        }
        self.check_new_constraints(label, &ids, &before, &degenerate_before)?;
        Ok(ids)
    }

    /// Fix geometry where it is (one undoable step), pinning only what is still free (#203):
    /// each point fully if nothing holds it yet, else just its x or y if one is still free,
    /// and each radius not yet set. A pin the relations and dimensions already there imply is
    /// left out, so Fix never over-defines. Refused, saying so, if nothing is left to pin, or
    /// if a pin conflicts (naming what it conflicts with).
    pub(super) fn add_fixes(
        &mut self,
        label: &str,
        fixes: &[Constraint],
    ) -> Result<Vec<ConstraintId>, EditError> {
        let lowering = self.lowering();
        let analyze = |doc: &Document| drift_solver::analyze_with(doc, lowering.as_deref());
        // Measured against the drawing as it is now (inside a script's batch it may have
        // changed since the last sync).
        let base = analyze(&self.doc);
        let dependent = base.dependent_equations;
        // (A pin merged with one already there may show either as the redundant one.)
        let new_redundant =
            |r: &SolveReport| r.redundant.iter().any(|c| !base.redundant.contains(c));
        let new_conflicts = |r: &SolveReport| {
            !r.converged && r.conflicts.iter().any(|c| !base.conflicts.contains(c))
        };
        self.doc.begin();
        // Usually nothing holds what is fixed yet: all of it is kept, in one go.
        let ids: Vec<ConstraintId> = fixes
            .iter()
            .filter_map(|c| self.doc.add_constraint(*c).ok())
            .collect();
        let report = analyze(&self.doc);
        if ids.len() == fixes.len()
            && !new_conflicts(&report)
            && report.dependent_equations == dependent
            && !new_redundant(&report)
        {
            self.commit(label);
            return Ok(ids);
        }
        self.doc.rollback();
        // Else one pin at a time, keeping those that hold something new.
        self.doc.begin();
        let mut kept = Vec::new();
        let mut conflict = None;
        for c in fixes {
            let tries = match *c {
                Constraint::Fix { point, at } => vec![
                    *c,
                    Constraint::FixX { point, x: at.x },
                    Constraint::FixY { point, y: at.y },
                ],
                other => vec![other],
            };
            for (i, pin) in tries.into_iter().enumerate() {
                let Ok(id) = self.doc.add_constraint(pin) else {
                    continue;
                };
                let report = analyze(&self.doc);
                if new_conflicts(&report) {
                    conflict = conflict.or_else(|| {
                        report
                            .conflicts
                            .iter()
                            .find(|c| **c != id && !kept.contains(*c))
                            .copied()
                    });
                    self.doc.remove_constraint(id);
                } else if new_redundant(&report) || report.dependent_equations > dependent {
                    self.doc.remove_constraint(id);
                } else {
                    kept.push(id);
                    if i == 0 {
                        // The whole point: its x and y alone are not tried.
                        break;
                    }
                }
            }
        }
        if let Some(other) = conflict {
            self.doc.rollback();
            self.sync();
            let what = self
                .doc
                .constraint(other)
                .map_or("a relation".to_owned(), |c| {
                    let kind = if c.is_dimension() {
                        "dimension"
                    } else {
                        "relation"
                    };
                    format!("the {} {kind}", c.type_name())
                });
            return Err(EditError::Invalid(format!("it conflicts with {what}")));
        }
        if kept.is_empty() {
            self.doc.rollback();
            self.sync();
            return Err(EditError::FullyDefined(self.fixed_name()));
        }
        self.commit(label);
        Ok(kept)
    }

    /// Whether `c` pins a point or a radius where it already is.
    pub(super) fn fixes_in_place(&self, c: &Constraint) -> bool {
        let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
        let at = |point| self.doc.point(point).ok();
        match *c {
            Constraint::Fix { point, at: to } => {
                at(point).is_some_and(|p| same(p.x, to.x) && same(p.y, to.y))
            }
            Constraint::FixX { point, x } => at(point).is_some_and(|p| same(p.x, x)),
            Constraint::FixY { point, y } => at(point).is_some_and(|p| same(p.y, y)),
            Constraint::FixRadius { curve, radius } => {
                drift_solver::radius_of(&self.doc, curve).is_some_and(|r| same(r, radius))
            }
            _ => false,
        }
    }

    /// What is being fixed, for messages: "this line is", "these 3 are", ….
    pub fn fixed_name(&self) -> String {
        let curves: Vec<EntityKind> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.get(*id).map(|e| e.kind))
            .collect();
        match curves.as_slice() {
            [one] => format!("this {} is", one.type_name().to_lowercase()),
            [] => "it is".to_owned(),
            many => format!("these {} are", many.len()),
        }
    }

    /// Solve after constraints `ids` were made active in the open transaction, and commit it,
    /// or roll it back if they conflict, collapse geometry or add nothing new.
    pub(super) fn check_new_constraints(
        &mut self,
        label: &str,
        ids: &[ConstraintId],
        before: &SolveReport,
        degenerate_before: &[EntityId],
    ) -> Result<(), EditError> {
        // The geometry the new constraints name takes up the change. The rest of the sketch
        // moves only where it has to, and its lines keep their directions and its arcs their
        // sizes where they can: a relation reshapes what it relates instead of spreading the
        // change thinly over everything.
        let named: HashSet<EntityId> = ids
            .iter()
            .filter_map(|id| self.doc.constraint(*id))
            .flat_map(|c| c.entities())
            .collect();
        let mut directions = HashMap::new();
        let mut radii = HashMap::new();
        for (id, e) in self.doc.iter().filter(|(id, _)| !named.contains(id)) {
            match e.kind {
                EntityKind::Line { a, b } => {
                    if let (Ok(pa), Ok(pb)) = (self.doc.point(a), self.doc.point(b))
                        && let Some(u) = (pb - pa).try_normalize()
                    {
                        directions.insert(id, u);
                    }
                }
                EntityKind::Arc { .. } => {
                    if let Some(r) = drift_solver::radius_of(&self.doc, id) {
                        radii.insert(id, r);
                    }
                }
                _ => {}
            }
        }
        // (Relations on walls' faces take up the change on the centrelines they lower to.)
        let lowering = self.lowering();
        let mut named = named;
        if let Some(l) = &lowering {
            for id in ids {
                for e in
                    l.of.get(id)
                        .into_iter()
                        .flatten()
                        .flat_map(|x| x.entities())
                {
                    named.insert(e);
                }
            }
        }
        let focus = self.params_of(named);
        let report = drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                analyze: true,
                focus,
                directions,
                radii,
                lowering,
                ..Default::default()
            },
        );
        let error = if self.new_conflicts(&report) {
            Some(EditError::Conflict)
        } else if self.new_degeneracy(degenerate_before) {
            Some(EditError::Degenerate)
        } else if ids.iter().all(|id| report.redundant.contains(id))
            || report.dof == before.dof
            || (report.dependent_equations > before.dependent_equations
                && !self.only_relations(ids))
        {
            Some(EditError::Redundant)
        } else {
            None
        };
        if let Some(error) = error {
            self.doc.rollback();
            self.sync();
            return Err(error);
        }
        self.commit(label);
        Ok(())
    }

    /// The solver parameters of entities: their defining points' coordinates and, for
    /// circles, the radius.
    pub(super) fn params_of(&self, ids: HashSet<EntityId>) -> Vec<Param> {
        let mut params: Vec<Param> = self
            .doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .flat_map(Param::point)
            .collect();
        params.extend(ids.into_iter().filter_map(|id| {
            matches!(
                self.doc.get(id).map(|e| e.kind),
                Some(EntityKind::Circle { .. })
            )
            .then_some(Param::Radius(id))
        }));
        params
    }

    /// Whether constraints are all relations of shape (no dimension and no fix). A relation
    /// of several equations may have one already implied — collinear between two lines
    /// already held level, say — and still hold something new; that is fine, since relations
    /// can't disagree in value. A dimension or a fix that leaves another one implied
    /// over-defines the sketch instead.
    pub(super) fn only_relations(&self, ids: &[ConstraintId]) -> bool {
        ids.iter().all(|id| {
            self.doc
                .constraint(*id)
                .is_some_and(|c| !c.is_dimension() && !c.is_point_fix())
        })
    }

    /// Add a dimension with its label placed at `placement` (offset from its anchor), as one
    /// undoable step.
    pub fn add_dimension(
        &mut self,
        constraint: Constraint,
        placement: Option<DVec2>,
    ) -> Result<ConstraintId, EditError> {
        // On a wall's corner (or a face, where it can't hold it): a reference dimension.
        if let Some(why) = self.refused_on_walls(&constraint) {
            let id = self.add_driven_dimension(constraint, placement)?;
            self.notice = Some(format!("{why} (it is a reference dimension)"));
            return Ok(id);
        }
        let id = match self.add_constraints(constraint.type_name(), &[constraint]) {
            Ok(ids) => ids[0],
            Err(e @ (EditError::Redundant | EditError::Conflict)) => {
                // Over-defining: offer it as a reference dimension instead.
                self.pending_driven = Some((constraint, placement));
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        if let Some(offset) = placement {
            // Fold the placement into the step that added the dimension.
            let tx_label = constraint.type_name();
            self.doc.begin();
            let _ = self.doc.set_placement(id, offset);
            let tx = self.doc.commit(tx_label);
            self.history.merge_into_last(tx);
            self.sync();
        }
        Ok(id)
    }

    /// Add a driven (reference) dimension at the geometry's current measurement, as one
    /// undoable step. It never conflicts: it reports rather than constrains.
    pub fn add_driven_dimension(
        &mut self,
        constraint: Constraint,
        placement: Option<DVec2>,
    ) -> Result<ConstraintId, EditError> {
        let constraint = drift_solver::measure(&self.doc, &constraint)
            .map(|v| constraint.with_value(v))
            .unwrap_or(constraint);
        self.doc.begin();
        let added = self.doc.add_constraint(constraint).and_then(|id| {
            self.doc.set_driven(id, true)?;
            if let Some(offset) = placement {
                self.doc.set_placement(id, offset)?;
            }
            Ok(id)
        });
        match added {
            Ok(id) => {
                self.commit(&format!("{} (driven)", constraint.type_name()));
                Ok(id)
            }
            Err(e) => {
                self.doc.rollback();
                self.sync();
                Err(EditError::Invalid(e.to_string()))
            }
        }
    }

    /// Make a dimension driven (a reference) or driving. Switching to driving keeps the current
    /// measurement and is refused if the dimension would over-define the sketch.
    pub fn set_driven(&mut self, id: ConstraintId, driven: bool) -> Result<(), EditError> {
        if !driven
            && let Some(why) = self
                .doc
                .constraint(id)
                .and_then(|c| self.refused_on_walls(c))
        {
            return Err(EditError::Invalid(why));
        }
        if self.doc.is_driven(id) == driven {
            return Ok(());
        }
        let label = if driven {
            "Make driven"
        } else {
            "Make driving"
        };
        if driven {
            self.doc.begin();
            self.doc.set_expression(id, None);
            if let Err(e) = self.doc.set_driven(id, true) {
                self.doc.rollback();
                return Err(EditError::Invalid(e.to_string()));
            }
            self.commit(label);
            return Ok(());
        }
        let before = self.analysis().clone();
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        self.doc.begin();
        if let Err(e) = self.doc.set_driven(id, false) {
            self.doc.rollback();
            return Err(EditError::Invalid(e.to_string()));
        }
        self.check_new_constraints(label, &[id], &before, &degenerate_before)
    }

    /// Move a dimension's label (inside an open transaction while dragging).
    pub fn place_dimension(&mut self, id: ConstraintId, offset: DVec2) {
        let _ = self.doc.set_placement(id, offset);
    }

    // ----- Regions ---------------------------------------------------------------------------

    /// Set a dimension from typed text: a constant sets the value; an expression that uses
    /// variables links the dimension to it.
    pub fn set_dimension_text(&mut self, id: ConstraintId, text: &str) -> Result<(), EditError> {
        let Some(c) = self.doc.constraint(id).copied() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
        if self.doc.is_driven(id) {
            return Err(EditError::Invalid(
                "a driven dimension follows the geometry; make it driving to set it".into(),
            ));
        }
        let evaluated = self.evaluate_text(text, dimension_kind(&c))?;
        // An offset's distance: all its sides take the new value (or expression).
        let family = self.offset_family(id);
        if family.len() > 1 {
            if evaluated.references.is_empty() {
                return self.set_offset(&family, evaluated.value);
            }
            return self.parameter_edit("Change offset", |doc| {
                for d in &family {
                    doc.set_expression(*d, Some(evaluated.text.clone()));
                }
                Ok(())
            });
        }
        if evaluated.references.is_empty() {
            self.doc.begin();
            self.doc.set_expression(id, None);
            let tx = self.doc.commit("Unlink");
            let unlinked = !tx.is_empty();
            let result = self.set_dimension(id, evaluated.value);
            if unlinked {
                match &result {
                    Ok(()) => self.history.merge_into_last(tx),
                    // Nothing else changed: keep the unlink as its own step.
                    Err(_) => self.history.push(tx),
                }
            }
            return result;
        }
        self.parameter_edit("Change dimension", |doc| {
            doc.set_expression(id, Some(evaluated.text));
            Ok(())
        })
    }

    /// The distance dimensions of the offset whose side `line` is (see [`Editor::offset`]),
    /// and the curve this side is measured from (and kept parallel to). An offset's sides
    /// are the chain through `line`; its distances are the dimensions from their points to
    /// curves outside the chain, two or more, all one value, with at most one shown.
    pub fn offset_side_of(&self, line: EntityId) -> Option<(Vec<ConstraintId>, EntityId)> {
        let Some(EntityKind::Line { a, b }) = self.doc.get(line).map(|e| e.kind) else {
            return None;
        };
        let (chain, _) = self.chain(line);
        let sides: BTreeSet<EntityId> = chain.iter().map(|(c, _)| *c).collect();
        let points = self.doc.defining_points(sides.iter().copied());
        let dims: BTreeSet<ConstraintId> = points
            .iter()
            .flat_map(|p| self.doc.constraints_on(*p))
            .filter(|c| {
                matches!(self.doc.constraint(*c),
                    Some(Constraint::PointLineDistance { point, line, .. })
                        if points.contains(point) && !sides.contains(line))
            })
            .collect();
        let value = self.doc.constraint(*dims.first()?)?.value()?;
        let shown = dims
            .iter()
            .filter(|d| {
                let s = self.doc.shown(drift_core::AnnotationRef::Dimension(**d));
                s.canvas || s.sheets
            })
            .count();
        let same = dims.iter().all(|d| {
            self.doc
                .constraint(*d)
                .and_then(Constraint::value)
                .is_some_and(|v| (v - value).abs() <= 1e-9 * value.abs().max(1.0))
        });
        if dims.len() < 2 || shown > 1 || !same {
            return None;
        }
        // This side's own distance: from one of its ends, to the curve it runs along.
        let from = dims.iter().find_map(|d| match *self.doc.constraint(*d)? {
            Constraint::PointLineDistance {
                point, line: from, ..
            } if (point == a || point == b)
                && self
                    .existing_relation(&Constraint::Parallel { a: from, b: line })
                    .is_some() =>
            {
                Some(from)
            }
            _ => None,
        })?;
        Some((dims.into_iter().collect(), from))
    }

    /// The distance dimensions of the offset `dim` is one of (itself included), or none: they
    /// hold one distance, changed together (see [`Editor::offset_side_of`]).
    pub fn offset_family(&self, dim: ConstraintId) -> Vec<ConstraintId> {
        let Some(Constraint::PointLineDistance { point, .. }) = self.doc.constraint(dim).copied()
        else {
            return Vec::new();
        };
        self.doc
            .users_of(point)
            .filter(|u| {
                matches!(
                    self.doc.get(*u).map(|e| e.kind),
                    Some(EntityKind::Line { .. })
                )
            })
            .find_map(|side| {
                self.offset_side_of(side)
                    .map(|(dims, _)| dims)
                    .filter(|dims| dims.contains(&dim))
            })
            .unwrap_or_default()
    }

    /// Set an offset's distance (its dimensions, `family`): one undo step.
    pub fn set_offset(&mut self, family: &[ConstraintId], distance: f64) -> Result<(), EditError> {
        self.doc.begin();
        if self.drag_offset(family, distance) {
            self.commit("Change offset");
            Ok(())
        } else {
            self.doc.rollback();
            self.sync();
            Err(EditError::Conflict)
        }
    }

    /// The text to edit a dimension with: its expression if linked, else its value.
    pub fn dimension_edit_text(&self, id: ConstraintId) -> Option<String> {
        if let Some(e) = self.doc.expression(id) {
            return Some(e.to_owned());
        }
        let c = self.doc.constraint(id)?;
        let units = self.doc.units();
        Some(match dimension_kind(c) {
            Kind::Length => units.format_length(c.value()?),
            kind => units.format(kind, c.value()?),
        })
    }

    /// Pin a live measurement (a dimension definition that is only measured).
    pub fn pin_measurement(&mut self, c: Constraint) -> Result<MeasurementId, EditError> {
        self.doc.begin();
        match self.doc.add_measurement(c) {
            Ok(id) => {
                self.commit("Pin measurement");
                Ok(id)
            }
            Err(e) => {
                self.doc.rollback();
                Err(EditError::Invalid(e.to_string()))
            }
        }
    }

    pub fn unpin_measurement(&mut self, id: MeasurementId) {
        self.doc.begin();
        self.doc.remove_measurement(id);
        self.commit("Unpin measurement");
    }

    /// Add a dimension a tool created from typed field `field`, linked to the typed
    /// expression if it used variables. Call inside a transaction.
    pub fn add_typed_dimension(&mut self, field: usize, c: Constraint) -> Option<ConstraintId> {
        // With automatic relations off a typed value only sizes what is being drawn (the
        // tools place the geometry at that size already): nothing is left to hold it.
        if !self.auto_relations {
            return None;
        }
        let id = self.doc.add_constraint(c).ok()?;
        if let Some(expr) = self.typed_exprs.get(field).cloned().flatten() {
            // Only if it still evaluates to this value (tools may take an absolute value).
            let matches = self
                .evaluate_text(&expr, dimension_kind(&c))
                .is_ok_and(|e| Some(e.value) == c.value());
            if matches {
                self.doc.set_expression(id, Some(expr));
            }
        }
        Some(id)
    }

    /// Change a dimension's value, moving geometry to match.
    pub fn set_dimension(&mut self, id: ConstraintId, value: f64) -> Result<(), EditError> {
        let Some(current) = self.doc.constraint(id).copied() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
        if self.doc.is_driven(id) {
            return Err(EditError::Invalid(
                "a driven dimension follows the geometry; make it driving to set it".into(),
            ));
        }
        if current.value() == Some(value) {
            return Ok(());
        }
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        self.doc.begin();
        if let Err(e) = self.doc.set_constraint(id, current.with_value(value)) {
            self.doc.rollback();
            self.sync();
            return Err(EditError::Invalid(e.to_string()));
        }
        self.settle_fillet(&current.with_value(value));
        let report = self.solve(&[]);
        let error = if self.new_conflicts(&report) {
            Some(EditError::Conflict)
        } else if self.new_degeneracy(&degenerate_before) {
            Some(EditError::Degenerate)
        } else {
            None
        };
        if let Some(error) = error {
            self.doc.rollback();
            self.sync();
            return Err(error);
        }
        self.commit("Change dimension");
        Ok(())
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) {
        self.doc.begin();
        self.doc.remove_constraint(id);
        self.commit("Delete constraint");
    }

    /// The relation like `wanted` that is already on the sketch: the same kind, on the same
    /// geometry.
    pub(super) fn existing_relation(&self, wanted: &Constraint) -> Option<ConstraintId> {
        let on = |c: &Constraint| {
            let mut ids: Vec<EntityId> = c.entities().collect();
            ids.sort_unstable();
            ids
        };
        let target = on(wanted);
        let first = *target.first()?;
        self.doc.constraints_on(first).find(|id| {
            self.doc.constraint(*id).is_some_and(|c| {
                std::mem::discriminant(c) == std::mem::discriminant(wanted) && on(c) == target
            })
        })
    }

    /// Whether `option` is Fix: pins of points and radii where they are.
    pub fn is_fix_option(option: &ConstraintOption) -> bool {
        !option.constraints.is_empty()
            && option
                .constraints
                .iter()
                .all(|c| c.is_point_fix() || matches!(c, Constraint::FixRadius { .. }))
    }

    /// The pins (a point's Fix, or its x or y alone; a radius's) on what `fix` pins.
    pub(super) fn pins_on(&self, fix: &Constraint) -> impl Iterator<Item = ConstraintId> + '_ {
        let target = fix.entities().next();
        target.into_iter().flat_map(move |e| {
            self.doc.constraints_on(e).filter(move |id| {
                self.doc.constraint(*id).is_some_and(|c| {
                    (c.is_point_fix() || matches!(c, Constraint::FixRadius { .. }))
                        && c.entities().next() == Some(e)
                })
            })
        })
    }

    /// The relations of `option` that are already on the sketch. For Fix, every pin on what
    /// it pins (Fix may have pinned only a point's x or y, or left a point others hold).
    pub fn existing_relations(&self, option: &ConstraintOption) -> Vec<ConstraintId> {
        if Self::is_fix_option(option) {
            let mut ids: Vec<ConstraintId> = option
                .constraints
                .iter()
                .flat_map(|c| self.pins_on(c))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            return ids;
        }
        option
            .constraints
            .iter()
            .filter_map(|c| self.existing_relation(c))
            .collect()
    }

    /// The relations of `option` that aren't on the sketch yet. For Fix, the pins of what is
    /// neither pinned nor already held fully by other relations and dimensions (#203).
    pub fn missing_relations(&self, option: &ConstraintOption) -> Vec<Constraint> {
        if Self::is_fix_option(option) {
            return option
                .constraints
                .iter()
                .filter(|c| {
                    self.pins_on(c).next().is_none()
                        && c.entities().next().is_none_or(|e| {
                            self.analysis().definitions.get(&e)
                                != Some(&drift_solver::Definition::Full)
                        })
                })
                .copied()
                .collect();
        }
        option
            .constraints
            .iter()
            .filter(|c| self.existing_relation(c).is_none())
            .copied()
            .collect()
    }

    /// Whether everything `option` would add is already there (Fix: something picked is
    /// pinned, and nothing is left free).
    pub fn has_relations(&self, option: &ConstraintOption) -> bool {
        !option.constraints.is_empty()
            && self.missing_relations(option).is_empty()
            && (!Self::is_fix_option(option) || !self.existing_relations(option).is_empty())
    }

    /// Take relations off (one undo step).
    pub fn remove_relations(&mut self, ids: &[ConstraintId]) {
        if ids.is_empty() {
            return;
        }
        self.doc.begin();
        for id in ids {
            self.doc.remove_constraint(*id);
        }
        self.commit("Remove relation");
    }

    /// The relations and the dimensions on the selection: on its curves, their points, and
    /// picked midpoints (so also those holding it to geometry that isn't picked).
    pub fn selection_constraints(&self) -> (Vec<ConstraintId>, Vec<ConstraintId>) {
        let mut on = self.doc.defining_points(self.selection.iter().copied());
        on.extend(self.selection.iter().copied());
        on.extend(self.selected_midpoints.iter().copied());
        if on.is_empty() {
            return (Vec::new(), Vec::new());
        }
        self.doc
            .constraints()
            .filter(|(_, c)| c.entities().any(|e| on.contains(&e)))
            .map(|(id, c)| (id, c.is_dimension()))
            .fold((Vec::new(), Vec::new()), |(mut r, mut d), (id, dim)| {
                if dim {
                    d.push(id);
                } else {
                    r.push(id);
                }
                (r, d)
            })
    }

    /// Remove constraints as one undo step; how many went.
    pub fn clear_constraints(&mut self, ids: &[ConstraintId], label: &str) -> usize {
        if ids.is_empty() {
            return 0;
        }
        self.doc.begin();
        for id in ids {
            self.doc.remove_constraint(*id);
        }
        if self.commit(label) { ids.len() } else { 0 }
    }

    /// Whether adding `constraints` would over-define the sketch. Nothing is changed.
    pub fn would_conflict(&mut self, constraints: &[Constraint]) -> bool {
        if constraints.is_empty() {
            return false;
        }
        self.doc.begin();
        let added = constraints
            .iter()
            .all(|c| self.doc.add_constraint(*c).is_ok());
        let bad = !added || {
            let lowering = self.lowering();
            let report = drift_solver::solve(
                &mut self.doc,
                &SolveOptions {
                    analyze: true,
                    lowering,
                    ..Default::default()
                },
            );
            self.new_conflicts(&report)
        };
        self.doc.rollback();
        self.sync();
        bad
    }

    /// Add a relation that an operation infers (a trimmed end kept on the curve that cut it,
    /// mirrored points kept symmetric, a fillet kept tangent...), inside its transaction. With
    /// automatic relations off nothing is added: the geometry is left as plain geometry.
    /// Relations the user asks for by name don't come through here.
    pub fn relate(&mut self, relation: Constraint) -> Option<ConstraintId> {
        if !self.auto_relations {
            return None;
        }
        self.doc.add_constraint(relation).ok()
    }

    /// The dimension that already sets what `wanted` would (a line's length is the distance
    /// between its ends, either way round; a circle's radius and diameter are one size).
    pub fn size_dimension(&self, wanted: &Constraint) -> Option<ConstraintId> {
        let same = |c: &Constraint| match (c, wanted) {
            (
                Constraint::Distance { a: a1, b: b1, .. },
                Constraint::Distance { a: a2, b: b2, .. },
            ) => (a1 == a2 && b1 == b2) || (a1 == b2 && b1 == a2),
            (
                Constraint::Radius { curve: c1, .. } | Constraint::Diameter { curve: c1, .. },
                Constraint::Radius { curve: c2, .. } | Constraint::Diameter { curve: c2, .. },
            ) => c1 == c2,
            _ => false,
        };
        self.doc
            .constraints()
            .find(|(_, c)| same(c))
            .map(|(id, _)| id)
    }

    /// Set a size typed into the inspector (`text` may use units and variables): changes the
    /// dimension that already sets it, or adds one.
    pub fn set_size(&mut self, wanted: Constraint, text: &str) -> Result<ConstraintId, EditError> {
        let evaluated = self.evaluate_text(text, dimension_kind(&wanted))?;
        if let Some(id) = self.size_dimension(&wanted) {
            let existing = self.doc.constraint(id).copied().expect("found above");
            // A radius typed where the diameter is dimensioned (or the reverse).
            let factor = match (&existing, &wanted) {
                (Constraint::Diameter { .. }, Constraint::Radius { .. }) => 2.0,
                (Constraint::Radius { .. }, Constraint::Diameter { .. }) => 0.5,
                _ => 1.0,
            };
            if factor == 1.0 {
                self.set_dimension_text(id, text)?;
            } else {
                let value = self.doc.units().format_length(evaluated.value * factor);
                self.set_dimension_text(id, &value)?;
            }
            return Ok(id);
        }
        let c = wanted.with_value(evaluated.value);
        let id = self.add_constraints(c.type_name(), &[c])?[0];
        if !evaluated.references.is_empty() {
            // Keep the link to the variables it was typed with.
            self.set_dimension_text(id, text)?;
        }
        Ok(id)
    }

    /// Give the geometry the size `wanted` describes, once: as [`Editor::set_size`], but
    /// without leaving a dimension behind to hold it (one undo step). For sizes typed in
    /// the inspector while automatic relations are off.
    pub fn resize_once(&mut self, wanted: Constraint, text: &str) -> Result<(), EditError> {
        let evaluated = self.evaluate_text(text, dimension_kind(&wanted))?;
        let c = wanted.with_value(evaluated.value);
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        self.doc.begin();
        let id = match self.doc.add_constraint(c) {
            Ok(id) => id,
            Err(e) => {
                self.doc.rollback();
                self.sync();
                return Err(EditError::Invalid(e.to_string()));
            }
        };
        self.settle_fillet(&c);
        let report = self.solve(&[]);
        let error = if self.new_conflicts(&report) {
            Some(EditError::Conflict)
        } else if self.new_degeneracy(&degenerate_before) {
            Some(EditError::Degenerate)
        } else {
            None
        };
        if let Some(error) = error {
            self.doc.rollback();
            self.sync();
            return Err(error);
        }
        // The geometry is where the size put it; the size itself isn't kept.
        self.doc.remove_constraint(id);
        self.commit("Resize");
        Ok(())
    }
}

impl Editor {
    /// Whether a constraint names a wall's generated face or corner.
    pub(crate) fn on_wall_faces(&self, c: &Constraint) -> bool {
        self.doc.has_walls() && {
            let owners = self.wall_owners();
            c.entities().any(|e| owners.generated(e))
        }
    }

    /// Why a relation (or driving dimension) on walls' generated geometry can't hold it: on
    /// a corner or an end edge, or of a kind a face can't take (see
    /// `drift_geom::walls::lower`). None when it can, or names none.
    pub(crate) fn refused_on_walls(&self, c: &Constraint) -> Option<String> {
        if !self.on_wall_faces(c) {
            return None;
        }
        drift_geom::walls::lower::lower_constraint(&self.doc, c)?.err()
    }
}
