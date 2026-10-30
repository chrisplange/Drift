//! Variables, expressions, drawing rules and units, and variants (options).

use super::*;

impl Editor {
    /// Variable and linked-dimension values.
    pub fn evaluation(&self) -> &Evaluation {
        &self.evaluation
    }

    /// Evaluate typed text as a value of `kind`, with the document's variables and units.
    pub fn evaluate_text(
        &self,
        text: &str,
        kind: Kind,
    ) -> Result<drift_core::expr::Evaluated, EditError> {
        // A leading `=` (as typed while drawing, `=width/2`) is allowed in every field.
        let text = text.trim();
        let text = text.strip_prefix('=').unwrap_or(text);
        params::evaluate_text(&self.doc, &self.evaluation, text, kind)
            .map_err(|e| EditError::Invalid(e.0))
    }

    /// Re-evaluate variables and push linked dimension values into the constraints (inside a
    /// transaction). Fails on any evaluation error.
    pub(super) fn apply_parameters(&mut self) -> Result<(), EditError> {
        let eval = params::evaluate(&self.doc);
        if let Some(e) = eval.first_error(&self.doc) {
            return Err(EditError::Invalid(e));
        }
        for (id, value) in &eval.dimensions {
            let (Ok(value), Some(c)) = (value, self.doc.constraint(*id).copied()) else {
                continue;
            };
            if c.value() != Some(*value) {
                self.doc
                    .set_constraint(*id, c.with_value(*value))
                    .map_err(|e| EditError::Invalid(e.to_string()))?;
            }
        }
        self.apply_wall_thickness()?;
        self.apply_opening_sizes()
    }

    /// Apply parameters, solve and commit the open transaction, or roll it back.
    pub(super) fn finish_parameter_edit(
        &mut self,
        label: &str,
        degenerate_before: &[EntityId],
    ) -> Result<(), EditError> {
        let result = self.apply_parameters().and_then(|()| {
            let report = self.solve(&[]);
            if self.new_conflicts(&report) {
                Err(EditError::Conflict)
            } else if self.new_degeneracy(degenerate_before) {
                Err(EditError::Degenerate)
            } else {
                Ok(())
            }
        });
        match result {
            Ok(()) => {
                self.commit(label);
                Ok(())
            }
            Err(e) => {
                self.doc.rollback();
                self.sync();
                Err(e)
            }
        }
    }

    /// Run `f` in a transaction and finish it as a parameter edit.
    pub(super) fn parameter_edit(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Document) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        self.doc.begin();
        if let Err(e) = f(&mut self.doc) {
            self.doc.rollback();
            self.sync();
            return Err(e);
        }
        self.finish_parameter_edit(label, &degenerate_before)
    }

    /// Add a variable from typed text (bare numbers in the document's unit).
    pub fn add_variable(
        &mut self,
        name: &str,
        text: &str,
        kind: Kind,
    ) -> Result<VariableId, EditError> {
        let evaluated = self.evaluate_text(text, kind)?;
        let mut id = None;
        self.parameter_edit("Add variable", |doc| {
            id = Some(
                doc.add_variable(Variable {
                    name: name.trim().to_owned(),
                    expr: evaluated.text,
                    kind,
                })
                .map_err(|e| EditError::Invalid(e.to_string()))?,
            );
            Ok(())
        })?;
        Ok(id.expect("added"))
    }

    /// Change a variable's expression; everything that uses it updates.
    pub fn set_variable(&mut self, id: VariableId, text: &str) -> Result<(), EditError> {
        let Some(var) = self.doc.variable(id).cloned() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
        let evaluated = self.evaluate_text(text, var.kind)?;
        if evaluated.references.contains(&var.name) {
            return Err(EditError::Invalid(format!(
                "`{}` can't use itself",
                var.name
            )));
        }
        let label = format!("Change {}", var.name);
        self.parameter_edit(&label, |doc| {
            doc.set_variable_expr(id, &evaluated.text)
                .map_err(|e| EditError::Invalid(e.to_string()))
        })
    }

    pub fn set_variable_kind(&mut self, id: VariableId, kind: Kind) -> Result<(), EditError> {
        self.parameter_edit("Change variable type", |doc| {
            doc.set_variable_kind(id, kind)
                .map_err(|e| EditError::Invalid(e.to_string()))
        })
    }

    pub fn rename_variable(&mut self, id: VariableId, name: &str) -> Result<(), EditError> {
        self.parameter_edit("Rename variable", |doc| {
            doc.rename_variable(id, name.trim())
                .map_err(|e| EditError::Invalid(e.to_string()))
        })
    }

    pub fn remove_variable(&mut self, id: VariableId) -> Result<(), EditError> {
        self.parameter_edit("Delete variable", |doc| {
            doc.remove_variable(id)
                .map_err(|e| EditError::Invalid(e.to_string()))
        })
    }

    /// True if a variable is a plain value (no references), so a slider can drive it.
    pub fn is_scrubbable(&self, id: VariableId) -> bool {
        self.doc.variable(id).is_some_and(|v| {
            drift_core::expr::parse(&v.expr).is_ok_and(|e| e.identifiers().is_empty())
        })
    }

    /// Set a variable to `value` (in its kind's stored unit) live while a slider is dragged.
    /// The first call opens a transaction that [`end_scrub`](Self::end_scrub) commits.
    /// Returns false (keeping the last good value) if the sketch can't take the value.
    pub fn scrub_variable(&mut self, id: VariableId, value: f64) -> bool {
        let Some(var) = self.doc.variable(id).cloned() else {
            return false;
        };
        if self.scrubbing.is_none() {
            self.doc.begin();
            self.scrubbing = Some((id, var.expr.clone()));
        }
        let degenerate_before = drift_solver::degenerate_entities(&self.doc);
        let previous = var.expr.clone();
        let literal = self.doc.units().literal(var.kind, value);
        let _ = self.doc.set_variable_expr(id, &literal);
        let ok = self.apply_parameters().is_ok() && {
            let report = self.solve(&[]);
            !self.new_conflicts(&report) && !self.new_degeneracy(&degenerate_before)
        };
        if !ok {
            let _ = self.doc.set_variable_expr(id, &previous);
            let _ = self.apply_parameters();
            self.solve(&[]);
        }
        self.index.update(&self.doc);
        self.evaluation = params::evaluate(&self.doc);
        ok
    }

    /// Finish a slider drag as one undo step.
    pub fn end_scrub(&mut self) {
        if let Some((id, _)) = self.scrubbing.take() {
            let name = self
                .doc
                .variable(id)
                .map_or("variable".into(), |v| v.name.clone());
            self.commit(&format!("Change {name}"));
        }
    }

    pub fn is_scrubbing(&self) -> bool {
        self.scrubbing.is_some()
    }

    /// Change the drawing's rules (how areas are found and picked, how fills stack and
    /// follow edits, the gap tolerance), as one undo step.
    pub fn change_rules(&mut self, change: impl FnOnce(&mut drift_core::DrawingRules)) {
        self.change_rules_merging(false, change);
    }

    /// [`Editor::change_rules`], merged into the last step when `continuing`: while one
    /// widget is still being edited (a tolerance dragged or typed), its changes make one step.
    pub fn change_rules_merging(
        &mut self,
        continuing: bool,
        change: impl FnOnce(&mut drift_core::DrawingRules),
    ) {
        const LABEL: &str = "Change drawing settings";
        let mut rules = self.doc.rules();
        change(&mut rules);
        if rules == self.doc.rules() {
            return;
        }
        self.doc.begin();
        self.doc.set_rules(rules);
        let tx = self.doc.commit(LABEL);
        if continuing && self.history.undo_label() == Some(LABEL) {
            self.history.merge_into_last(tx);
        } else {
            self.history.push(tx);
        }
        self.sync();
    }

    pub fn set_units(&mut self, units: Units) {
        self.doc.begin();
        self.doc.set_units(units);
        self.commit("Change units");
    }

    /// Save the current state as a new option: the variable values, or with `whole`, a copy
    /// of the whole sketch. One undo step.
    pub fn save_option(&mut self, name: &str, whole: bool) -> VariantId {
        let name = name.trim();
        let option = if whole {
            Variant::sketch(name, &self.doc)
        } else {
            Variant::values(name, self.current_values())
        };
        self.doc.begin();
        let id = self.doc.add_variant(option);
        self.commit("Save option");
        id
    }

    /// Save the current variable values as a new option.
    #[cfg(any(test, feature = "test-support"))]
    pub fn save_variant(&mut self, name: &str) -> VariantId {
        self.save_option(name, false)
    }

    pub(super) fn current_values(&self) -> BTreeMap<VariableId, String> {
        self.doc
            .variables()
            .map(|(id, v)| (id, v.expr.clone()))
            .collect()
    }

    /// Store the current state in an option: its values, or the whole sketch for a
    /// whole-sketch option. One undo step.
    pub fn update_variant(&mut self, id: VariantId) {
        let Some(old) = self.doc.variant(id).cloned() else {
            return;
        };
        let option = if old.is_sketch() {
            Variant::sketch(old.name, &self.doc)
        } else {
            Variant::values(old.name, self.current_values())
        };
        self.doc.begin();
        self.doc.set_variant(id, option);
        self.commit("Update option");
    }

    pub fn rename_variant(&mut self, id: VariantId, name: &str) {
        let Some(mut variant) = self.doc.variant(id).cloned() else {
            return;
        };
        variant.name = name.trim().to_owned();
        self.doc.begin();
        self.doc.set_variant(id, variant);
        self.commit("Rename option");
    }

    pub fn remove_variant(&mut self, id: VariantId) {
        self.doc.begin();
        self.doc.remove_variant(id);
        self.commit("Delete option");
    }

    /// Apply an option (one undo step): its variable values (refused if the sketch can't
    /// take them), or the whole sketch it holds.
    pub fn apply_variant(&mut self, id: VariantId) -> Result<(), EditError> {
        let Some(variant) = self.doc.variant(id).cloned() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
        let label = format!("Apply {}", variant.name);
        if let Some(sketch) = &variant.sketch {
            self.doc.begin();
            self.doc.restore_sketch(sketch);
            self.selection.clear();
            self.selected_midpoints.clear();
            self.selected_instances.clear();
            self.selected_fills.clear();
            if !self.commit(&label) {
                return Err(EditError::Conflict);
            }
            return Ok(());
        }
        self.parameter_edit(&label, |doc| {
            for (var, expr) in &variant.values {
                if doc.variable(*var).is_some() {
                    doc.set_variable_expr(*var, expr)
                        .map_err(|e| EditError::Invalid(e.to_string()))?;
                }
            }
            Ok(())
        })
    }

    /// The option the drawing is in: the values option whose values match the current
    /// variables, or the whole-sketch option the sketch is the same as.
    pub fn active_variant(&self) -> Option<VariantId> {
        let revision = self.doc.revision();
        if let Some((r, active)) = self.active_option.get()
            && r == revision
        {
            return active;
        }
        let active = self.doc.variants().find_map(|(id, v)| {
            let matches = match &v.sketch {
                Some(sketch) => self.doc.same_sketch(sketch),
                None => {
                    !v.values.is_empty()
                        && v.values
                            .iter()
                            .all(|(var, e)| self.doc.variable(*var).is_none_or(|x| x.expr == *e))
                }
            };
            matches.then_some(id)
        });
        self.active_option.set(Some((revision, active)));
        active
    }

    /// An option's geometry, for drawing it as a ghost: a whole-sketch option's curves, or
    /// the sketch solved with a values option's values (on a copy).
    pub fn variant_geometry(&self, id: VariantId) -> Option<Vec<SketchItem>> {
        let variant = self.doc.variant(id)?;
        let curves = |doc: &Document| -> Vec<SketchItem> {
            doc.iter()
                .filter(|(_, e)| !e.kind.is_point() && e.style != LineKind::Construction)
                .filter_map(|(id, _)| doc.shape(id))
                .map(SketchItem::new)
                .collect()
        };
        if let Some(sketch) = &variant.sketch {
            return Some(curves(sketch));
        }
        let mut copy = Editor {
            doc: self.doc.sketch_copy(),
            ..Default::default()
        };
        copy.doc.begin();
        for (var, expr) in &variant.values {
            let _ = copy.doc.set_variable_expr(*var, expr);
        }
        copy.apply_parameters().ok()?;
        let report = copy.solve(&[]);
        report.converged.then(|| curves(&copy.doc))
    }
}
