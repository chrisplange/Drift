//! Constraints: validation, driven dimensions, label placement and extension lines.

use super::*;

impl Document {
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(&id)
    }

    /// All constraints in id order.
    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint)> {
        self.constraints.iter().map(|(id, c)| (*id, c))
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Constraints that refer to `entity`.
    pub fn constraints_on(&self, entity: EntityId) -> impl Iterator<Item = ConstraintId> + '_ {
        self.constraints
            .iter()
            .filter(move |(_, c)| c.entities().any(|e| e == entity))
            .map(|(id, _)| *id)
    }

    /// What makes a constraint refer to a curve's own points where that means nothing: a
    /// curve's point held on the curve, symmetry about a line through the points, a line's
    /// end at its own midpoint.
    pub(super) fn self_referencing(&self, constraint: &Constraint) -> Option<&'static str> {
        let points_of = |id: &EntityId| self.entities.get(id).map(|e| e.kind.points());
        match constraint {
            Constraint::PointOnCurve { point, curve }
                if points_of(curve).is_some_and(|mut ps| ps.any(|p| p == *point)) =>
            {
                Some("point on its own curve")
            }
            Constraint::Symmetric { a, b, axis }
                if points_of(axis).is_some_and(|mut ps| ps.any(|p| p == *a || p == *b)) =>
            {
                Some("symmetric about a line through the points")
            }
            Constraint::Midpoint { point, line }
                if matches!(
                    self.entities.get(line).map(|e| e.kind),
                    Some(EntityKind::Line { a, b }) if *point == a || *point == b
                ) =>
            {
                Some("midpoint")
            }
            _ => None,
        }
    }

    /// Check that a constraint refers to existing entities of the right kinds.
    pub fn validate(&self, constraint: &Constraint) -> Result<(), DocumentError> {
        if constraint.is_degenerate() {
            return Err(DocumentError::Degenerate("constraint"));
        }
        if !constraint.value_is_valid() {
            return Err(DocumentError::InvalidValue);
        }
        let mut kinds = Vec::new();
        for (id, slot) in constraint.slots() {
            let kind = self
                .entities
                .get(&id)
                .ok_or(DocumentError::Missing(id))?
                .kind;
            let ok = match slot {
                Slot::Point => kind.is_point(),
                Slot::PointOrMidpoint => kind.is_point() || matches!(kind, EntityKind::Line { .. }),
                Slot::Line => matches!(kind, EntityKind::Line { .. }),
                Slot::Circular => {
                    matches!(kind, EntityKind::Circle { .. } | EntityKind::Arc { .. })
                }
                Slot::LineOrCircular => matches!(
                    kind,
                    EntityKind::Line { .. } | EntityKind::Circle { .. } | EntityKind::Arc { .. }
                ),
                Slot::Curve => !kind.is_point(),
            };
            if !ok {
                return Err(DocumentError::WrongKind {
                    id,
                    expected: slot.describe(),
                });
            }
            kinds.push(kind);
        }
        // Tangency is meaningless between a curve and itself (caught by `is_degenerate`).
        if let Some(why) = self.self_referencing(constraint) {
            return Err(DocumentError::Degenerate(why));
        }
        // Tangent to a circle or arc, or at a shared end to an elliptical arc or Bézier.
        if let Constraint::Tangent { b, .. } = constraint {
            let ok = match kinds[1] {
                EntityKind::Circle { .. } | EntityKind::Arc { .. } => true,
                EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. } => {
                    kinds[0].shared_end(&kinds[1]).is_some()
                }
                _ => false,
            };
            if !ok {
                return Err(DocumentError::WrongKind {
                    id: *b,
                    expected: "a circle or arc, or an elliptical arc or curve ending where it does",
                });
            }
        }
        // Equal needs two lines or two circular curves.
        if let Constraint::Equal { b, .. } = constraint {
            let first_is_line = matches!(kinds[0], EntityKind::Line { .. });
            if first_is_line != matches!(kinds[1], EntityKind::Line { .. }) {
                return Err(DocumentError::WrongKind {
                    id: *b,
                    expected: if first_is_line {
                        "a line"
                    } else {
                        "a circle or arc"
                    },
                });
            }
        }
        Ok(())
    }

    pub fn add_constraint(
        &mut self,
        constraint: Constraint,
    ) -> Result<ConstraintId, DocumentError> {
        self.validate(&constraint)?;
        let id = ConstraintId(self.next_constraint_id);
        self.next_constraint_id += 1;
        self.touch_constraint(id);
        self.constraints.insert(id, constraint);
        Ok(id)
    }

    /// Replace a constraint (e.g. to change a dimension's value).
    pub fn set_constraint(
        &mut self,
        id: ConstraintId,
        constraint: Constraint,
    ) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        self.validate(&constraint)?;
        self.touch_constraint(id);
        self.constraints.insert(id, constraint);
        Ok(())
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) {
        if self.constraints.contains_key(&id) {
            self.touch_constraint(id);
            self.constraints.remove(&id);
        }
        if self.placements.contains_key(&id) {
            self.touch_key(Key::Placement(id));
            self.placements.remove(&id);
        }
        if self.extension_starts.contains_key(&id) {
            self.touch_key(Key::ExtensionStarts(id));
            self.extension_starts.remove(&id);
        }
        if self.driven.contains(&id) {
            self.touch_key(Key::Driven(id));
            self.driven.remove(&id);
        }
        self.set_expression(id, None);
        self.set_dim_note(id, None);
    }

    /// Whether a dimension is driven (a reference that reports but doesn't constrain).
    pub fn is_driven(&self, id: ConstraintId) -> bool {
        self.driven.contains(&id)
    }

    /// Make a dimension driven or driving. Only dimensions can be driven.
    pub fn set_driven(&mut self, id: ConstraintId, driven: bool) -> Result<(), DocumentError> {
        let constraint = self
            .constraints
            .get(&id)
            .ok_or(DocumentError::MissingConstraint(id))?;
        if driven && !constraint.is_dimension() {
            return Err(DocumentError::InvalidValue);
        }
        if self.driven.contains(&id) != driven {
            self.touch_key(Key::Driven(id));
            if driven {
                self.driven.insert(id);
            } else {
                self.driven.remove(&id);
            }
        }
        Ok(())
    }

    /// Where a dimension's label sits, as an offset from its anchor (see [`Document`]).
    pub fn placement(&self, id: ConstraintId) -> Option<DVec2> {
        self.placements.get(&id).copied()
    }

    pub fn set_placement(&mut self, id: ConstraintId, offset: DVec2) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        if !offset.is_finite() {
            return Err(DocumentError::InvalidValue);
        }
        if self.placements.get(&id) != Some(&offset) {
            self.touch_key(Key::Placement(id));
            self.placements.insert(id, offset);
        }
        Ok(())
    }

    /// Where a dimension's extension lines start, per end: a distance from the measured point
    /// along the extension line (toward the dimension line), or `None` for automatic.
    pub fn extension_starts(&self, id: ConstraintId) -> [Option<f64>; 2] {
        self.extension_starts.get(&id).copied().unwrap_or_default()
    }

    /// Set where one end's extension line starts (`end` 0 or 1), or `None` for automatic.
    pub fn set_extension_start(
        &mut self,
        id: ConstraintId,
        end: usize,
        start: Option<f64>,
    ) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        if end > 1 || start.is_some_and(|s| !s.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        let mut starts = self.extension_starts(id);
        if starts[end] != start {
            starts[end] = start;
            self.touch_key(Key::ExtensionStarts(id));
            if starts == [None, None] {
                self.extension_starts.remove(&id);
            } else {
                self.extension_starts.insert(id, starts);
            }
        }
        Ok(())
    }

    // ----- Layers ----------------------------------------------------------------------------
}
