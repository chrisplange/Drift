//! Editing state independent of the UI toolkit: document, history, selection, picking and
//! constraint solving.

use std::collections::BTreeSet;

use drift_core::{
    Bounds, Constraint, ConstraintId, DVec2, Document, DofState, Emphasis, EntityId, EntityKind,
    History, LineKind, Shape, SketchItem,
};
use drift_geom::{PointRole, SceneIndex, Snap};
use drift_solver::{Definition, Param, SolveOptions, SolveReport};

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The constraint can't be satisfied together with the existing ones.
    Conflict,
    /// The constraint is already implied by existing ones.
    Redundant,
    /// Satisfying it would collapse geometry (e.g. a line to zero length).
    Degenerate,
    /// The selection doesn't fit the constraint, or a value is invalid.
    Invalid(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Conflict => write!(f, "conflicts with existing constraints"),
            EditError::Redundant => write!(f, "already implied by existing constraints"),
            EditError::Degenerate => write!(f, "would collapse geometry to zero size"),
            EditError::Invalid(why) => write!(f, "{why}"),
        }
    }
}

/// A relation or dimension that can be applied to the current selection.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintOption {
    pub label: &'static str,
    pub constraints: Vec<Constraint>,
}

/// Why the pointer was moved off its raw position other than by a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideKind {
    /// Aligned horizontally and/or vertically with recently hovered points (inference lines).
    Aligned,
    /// Made exactly horizontal from the previous point; the new line gets a Horizontal relation.
    Horizontal,
    /// Made exactly vertical from the previous point; the new line gets a Vertical relation.
    Vertical,
}

/// An inference: the adjusted position and the points the dotted guide lines run from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub at: DVec2,
    pub kind: GuideKind,
    /// Up to two reference points (for drawing guide lines).
    pub from: [Option<DVec2>; 2],
}

/// Where the pointer is, in world coordinates, with its snap target (if any).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// Unsnapped position.
    pub raw: DVec2,
    pub snap: Option<Snap>,
    /// Picking tolerance in world units.
    pub tolerance: f64,
    /// Inference adjustment, used unless the pointer snapped to an existing point.
    pub guide: Option<Guide>,
    /// Whether snapping onto a midpoint should add a Midpoint relation (automatic relations).
    pub relations: bool,
}

impl Pointer {
    pub fn new(raw: DVec2, tolerance: f64) -> Self {
        Self {
            raw,
            snap: None,
            tolerance,
            guide: None,
            relations: true,
        }
    }

    /// The line whose midpoint the pointer snapped to, if it should get a Midpoint relation.
    pub fn midpoint_of(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind == drift_geom::SnapKind::Midpoint && self.relations)
            .and_then(|s| s.entity)
    }

    /// The effective position: an existing point snapped to, else an inference, else a snap
    /// (curve / grid), else the raw position.
    pub fn pos(&self) -> DVec2 {
        match (self.snap, self.guide) {
            (Some(s), _) if s.kind.is_point() => s.at,
            (_, Some(g)) => g.at,
            (Some(s), None) => s.at,
            (None, None) => self.raw,
        }
    }

    /// The automatic relation implied by the guide, if any.
    pub fn auto_relation(&self) -> Option<GuideKind> {
        self.guide
            .map(|g| g.kind)
            .filter(|k| *k != GuideKind::Aligned)
            .filter(|_| self.snapped_point().is_none())
    }

    /// The existing point entity under the pointer, if it snapped to one.
    pub fn snapped_point(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind.is_point())
            .and_then(|s| s.entity)
    }

    pub fn anchor(&self) -> Anchor {
        Anchor {
            pos: self.pos(),
            existing: self.snapped_point(),
            midpoint_of: self.midpoint_of(),
        }
    }
}

/// A position a tool has captured: either an existing point to reuse or a new location.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub pos: DVec2,
    pub existing: Option<EntityId>,
    /// A new point here should be kept at this line's midpoint.
    pub midpoint_of: Option<EntityId>,
}

impl Anchor {
    /// A new point at an exact position.
    pub fn at(pos: DVec2) -> Self {
        Self {
            pos,
            existing: None,
            midpoint_of: None,
        }
    }

    /// The point entity for this anchor, creating it if needed (with its Midpoint relation, if
    /// it was snapped to a midpoint). Call inside a transaction.
    pub fn resolve(&self, doc: &mut Document) -> EntityId {
        match self.existing {
            Some(id) if doc.contains(id) => id,
            _ => {
                let point = doc.add_point(self.pos);
                if let Some(line) = self.midpoint_of {
                    let _ = doc.add_constraint(Constraint::Midpoint { point, line });
                }
                point
            }
        }
    }
}

#[derive(Default)]
pub struct Editor {
    pub doc: Document,
    pub history: History,
    pub selection: BTreeSet<EntityId>,
    pub hovered: Option<EntityId>,
    /// Lines whose midpoints are selected (midpoints aren't entities, but can be picked).
    pub selected_midpoints: BTreeSet<EntityId>,
    /// The line whose midpoint is under the pointer.
    pub hovered_midpoint: Option<EntityId>,
    pub index: SceneIndex,
    /// Degrees-of-freedom analysis of the document at `analyzed`.
    analysis: SolveReport,
    analyzed: Option<u64>,
    /// A message for the UI explaining a refused edit.
    pub notice: Option<String>,
    /// Entities to emphasise because a related constraint is hovered or selected.
    pub highlight: BTreeSet<EntityId>,
    /// A dimension just created by a tool, whose value the UI should let the user type.
    pub pending_edit: Option<ConstraintId>,
    /// Whether drawing tools add relations they infer (horizontal / vertical lines, rectangles).
    pub auto_relations: bool,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut editor = Self {
            doc,
            auto_relations: true,
            ..Default::default()
        };
        editor.sync();
        editor
    }

    /// Refresh caches after document changes. Cheap when nothing changed.
    pub fn sync(&mut self) {
        self.index.update(&self.doc);
        if self.analyzed != Some(self.doc.revision()) && !self.doc.in_transaction() {
            self.analysis = drift_solver::analyze(&self.doc);
            self.analyzed = Some(self.doc.revision());
        }
        let doc = &self.doc;
        self.selection.retain(|id| doc.contains(*id));
        let is_line =
            |id: &EntityId| matches!(doc.get(*id).map(|e| e.kind), Some(EntityKind::Line { .. }));
        self.selected_midpoints.retain(is_line);
        if self.hovered_midpoint.is_some_and(|id| !is_line(&id)) {
            self.hovered_midpoint = None;
        }
        if self.hovered.is_some_and(|id| !doc.contains(id)) {
            self.hovered = None;
        }
    }

    /// The degrees-of-freedom analysis of the document as of the last [`sync`](Self::sync).
    pub fn analysis(&self) -> &SolveReport {
        &self.analysis
    }

    fn solve(&mut self, dragged: &[Param]) -> SolveReport {
        drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                dragged: dragged.to_vec(),
                ..Default::default()
            },
        )
    }

    /// True if the edit collapsed geometry that wasn't collapsed before.
    fn new_degeneracy(&self, before: &[EntityId]) -> bool {
        drift_solver::degenerate_entities(&self.doc)
            .iter()
            .any(|id| !before.contains(id))
    }

    /// True if `report` has conflicts that weren't there before this edit (a document that was
    /// already conflicting shouldn't make every later edit fail).
    fn new_conflicts(&self, report: &SolveReport) -> bool {
        !report.converged
            && report
                .conflicts
                .iter()
                .any(|c| !self.analysis.conflicts.contains(c))
    }

    /// Solve the constraints, then finish the open transaction and record it for undo. If the
    /// edit made the constraints unsatisfiable it is rolled back and `false` returned.
    pub fn commit(&mut self, label: &str) -> bool {
        let report = self.solve(&[]);
        if self.new_conflicts(&report) {
            self.doc.rollback();
            self.notice = Some(format!("{label} undone: it {}", EditError::Conflict));
            self.sync();
            return false;
        }
        let tx = self.doc.commit(label);
        self.history.push(tx);
        self.sync();
        true
    }

    /// Move entities by `delta` inside an open transaction, keeping constraints satisfied.
    /// `dragged` are the parameters that should follow the pointer exactly. Returns false (and
    /// leaves the geometry where it was) if the move is impossible.
    pub fn drag(&mut self, ids: &[EntityId], delta: DVec2, dragged: &[Param]) -> bool {
        self.doc.translate(ids.iter().copied(), delta);
        let report = self.solve(dragged);
        let ok = !self.new_conflicts(&report);
        if !ok {
            self.doc.translate(ids.iter().copied(), -delta);
            self.solve(dragged);
        }
        self.index.update(&self.doc);
        ok
    }

    /// Add constraints as one undoable step. Refused if they conflict with the existing ones
    /// or add nothing new.
    pub fn add_constraints(
        &mut self,
        label: &str,
        constraints: &[Constraint],
    ) -> Result<Vec<ConstraintId>, EditError> {
        let before = self.analysis.clone();
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
        }
        let report = drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                analyze: true,
                ..Default::default()
            },
        );
        let error = if self.new_conflicts(&report) {
            Some(EditError::Conflict)
        } else if self.new_degeneracy(&degenerate_before) {
            Some(EditError::Degenerate)
        } else if ids.iter().all(|id| report.redundant.contains(id))
            || report.dependent_equations > before.dependent_equations
            || report.dof == before.dof
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
        Ok(ids)
    }

    /// Add a dimension with its label placed at `placement` (offset from its anchor), as one
    /// undoable step.
    pub fn add_dimension(
        &mut self,
        constraint: Constraint,
        placement: Option<DVec2>,
    ) -> Result<ConstraintId, EditError> {
        let id = *self
            .add_constraints(constraint.type_name(), &[constraint])?
            .first()
            .expect("one constraint added");
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

    /// Move a dimension's label (inside an open transaction while dragging).
    pub fn place_dimension(&mut self, id: ConstraintId, offset: DVec2) {
        let _ = self.doc.set_placement(id, offset);
    }

    /// Change a dimension's value, moving geometry to match.
    pub fn set_dimension(&mut self, id: ConstraintId, value: f64) -> Result<(), EditError> {
        let Some(current) = self.doc.constraint(id).copied() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
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

    /// Relations that fit the current selection.
    pub fn relation_options(&self) -> Vec<ConstraintOption> {
        let sel = self.classify_selection();
        let mut out = Vec::new();
        let opt = |label, constraints: Vec<Constraint>| ConstraintOption { label, constraints };
        if let ([point], [line], [], []) | ([point], [], [], [line]) = (
            sel.points.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
            sel.midpoints.as_slice(),
        ) {
            out.push(opt(
                "Midpoint",
                vec![Constraint::Midpoint {
                    point: *point,
                    line: *line,
                }],
            ));
        }
        if !sel.midpoints.is_empty() {
            return out;
        }
        match (
            sel.points.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
        ) {
            ([a, b], [], []) => {
                out.push(opt(
                    "Coincident",
                    vec![Constraint::Coincident { a: *a, b: *b }],
                ));
                out.push(opt(
                    "Horizontal",
                    vec![Constraint::HorizontalPoints { a: *a, b: *b }],
                ));
                out.push(opt(
                    "Vertical",
                    vec![Constraint::VerticalPoints { a: *a, b: *b }],
                ));
            }
            ([], lines, []) if !lines.is_empty() => {
                out.push(opt(
                    "Horizontal",
                    lines
                        .iter()
                        .map(|l| Constraint::Horizontal { line: *l })
                        .collect(),
                ));
                out.push(opt(
                    "Vertical",
                    lines
                        .iter()
                        .map(|l| Constraint::Vertical { line: *l })
                        .collect(),
                ));
                if let [a, b] = lines {
                    out.push(opt("Parallel", vec![Constraint::Parallel { a: *a, b: *b }]));
                    out.push(opt(
                        "Perpendicular",
                        vec![Constraint::Perpendicular { a: *a, b: *b }],
                    ));
                }
                if lines.len() >= 2 {
                    out.push(opt(
                        "Equal",
                        lines
                            .windows(2)
                            .map(|w| Constraint::Equal { a: w[0], b: w[1] })
                            .collect(),
                    ));
                }
            }
            ([], [], circulars) if circulars.len() >= 2 => {
                out.push(opt(
                    "Equal",
                    circulars
                        .windows(2)
                        .map(|w| Constraint::Equal { a: w[0], b: w[1] })
                        .collect(),
                ));
            }
            _ => {}
        }
        if !self.selection.is_empty() {
            let fixes = self.fix_constraints();
            if !fixes.is_empty() {
                out.push(opt("Fix", fixes));
            }
        }
        out
    }

    /// Dimensions that fit the current selection, at the geometry's current values.
    pub fn dimension_options(&self) -> Vec<ConstraintOption> {
        let sel = self.classify_selection();
        // Points and midpoints (a line id standing for its midpoint) both count as points.
        let mut refs = sel.points.clone();
        refs.extend(sel.midpoints.iter().copied());
        let lines = if sel.midpoints.is_empty() {
            sel.lines.clone()
        } else {
            // A picked midpoint's own line in the selection is a reference, not a line to
            // measure.
            sel.lines
                .iter()
                .copied()
                .filter(|l| !sel.midpoints.contains(l))
                .collect()
        };
        let measured =
            |c: Constraint| drift_solver::measure(&self.doc, &c).map(|v| c.with_value(v));
        let mut out = Vec::new();
        let mut push = |label, c: Constraint| {
            if let Some(c) = measured(c)
                && c.value_is_valid()
            {
                out.push(ConstraintOption {
                    label,
                    constraints: vec![c],
                });
            }
        };
        match (refs.as_slice(), lines.as_slice(), sel.circulars.as_slice()) {
            ([a, b], [], []) => {
                let (a, b) = (*a, *b);
                push("Distance", Constraint::Distance { a, b, value: 0.0 });
                push(
                    "Horizontal distance",
                    Constraint::HorizontalDistance { a, b, value: 0.0 },
                );
                push(
                    "Vertical distance",
                    Constraint::VerticalDistance { a, b, value: 0.0 },
                );
            }
            ([point], [line], []) => push(
                "Distance",
                Constraint::PointLineDistance {
                    point: *point,
                    line: *line,
                    value: 0.0,
                },
            ),
            ([], [line], []) => {
                if let Some(EntityKind::Line { a, b }) = self.doc.get(*line).map(|e| e.kind) {
                    push("Length", Constraint::Distance { a, b, value: 0.0 });
                }
            }
            ([], [], [curve])
                if matches!(
                    self.doc.get(*curve).map(|e| e.kind),
                    Some(EntityKind::Circle { .. })
                ) =>
            {
                push(
                    "Diameter",
                    Constraint::Diameter {
                        curve: *curve,
                        value: 0.0,
                    },
                )
            }
            ([], [a, b], []) => push(
                "Angle",
                Constraint::Angle {
                    a: *a,
                    b: *b,
                    degrees: 0.0,
                },
            ),
            ([], [], [curve]) => push(
                "Radius",
                Constraint::Radius {
                    curve: *curve,
                    value: 0.0,
                },
            ),
            _ => {}
        }
        out
    }

    /// Fix constraints that pin the selection where it is.
    fn fix_constraints(&self) -> Vec<Constraint> {
        let mut out = Vec::new();
        for point in self.doc.defining_points(self.selection.iter().copied()) {
            if let Ok(at) = self.doc.point(point) {
                out.push(Constraint::Fix { point, at });
            }
        }
        for id in &self.selection {
            if let Some(EntityKind::Circle { radius, .. }) = self.doc.get(*id).map(|e| e.kind) {
                out.push(Constraint::FixRadius { curve: *id, radius });
            }
        }
        out
    }

    fn classify_selection(&self) -> SelectionKinds {
        let mut kinds = SelectionKinds {
            midpoints: self.selected_midpoints.iter().copied().collect(),
            ..Default::default()
        };
        for id in &self.selection {
            match self.doc.get(*id).map(|e| e.kind) {
                Some(EntityKind::Point(_)) => kinds.points.push(*id),
                Some(EntityKind::Line { .. }) => kinds.lines.push(*id),
                Some(EntityKind::Circle { .. } | EntityKind::Arc { .. }) => {
                    kinds.circulars.push(*id)
                }
                None => {}
            }
        }
        kinds
    }

    pub fn undo(&mut self) -> bool {
        let done = self.history.undo(&mut self.doc);
        self.sync();
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.history.redo(&mut self.doc);
        self.sync();
        done
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.doc.begin();
        self.doc.delete(self.selection.iter().copied());
        self.commit("Delete");
    }

    /// Toggle construction style on the selection: if any selected curve is not construction,
    /// all become construction; otherwise all become solid.
    pub fn toggle_construction(&mut self) {
        let curves: Vec<_> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        if curves.is_empty() {
            return;
        }
        let make_construction = curves.iter().any(|id| {
            self.doc
                .get(*id)
                .is_some_and(|e| e.style != LineKind::Construction)
        });
        let style = if make_construction {
            LineKind::Construction
        } else {
            LineKind::Solid
        };
        self.doc.begin();
        for id in curves {
            self.doc.set_style(id, style);
        }
        self.commit("Toggle construction");
    }

    /// Select every curve and standalone point.
    pub fn select_all(&mut self) {
        self.selection = self
            .doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point() || self.index.role(*id) == Some(PointRole::Standalone)
            })
            .map(|(id, _)| id)
            .collect();
    }

    pub fn bounds(&self) -> Bounds {
        self.doc
            .iter()
            .filter_map(|(id, _)| self.index.shape(id))
            .map(Shape::bounds)
            .collect()
    }

    /// Render items for the document, with selection and hover emphasis.
    pub fn items(&self) -> Vec<SketchItem> {
        let mut items = Vec::with_capacity(self.doc.len());
        let mut emphasized = Vec::new();
        for (id, entity) in self.doc.iter() {
            let Some(shape) = self.index.shape(id) else {
                continue;
            };
            let dof = match self.analysis.definitions.get(&id) {
                Some(Definition::Under) => DofState::Under,
                Some(Definition::Full) => DofState::Full,
                Some(Definition::Over) => DofState::Over,
                None => DofState::Unknown,
            };
            let mut item = SketchItem::new(*shape)
                .with_kind(entity.style)
                .with_dof(dof);
            let role = self.index.role(id);
            if role.is_some_and(|r| r != PointRole::Standalone) {
                item = item.as_vertex();
            }
            let emphasis = if self.selection.contains(&id) {
                Emphasis::Selected
            } else if self.hovered == Some(id) || self.highlight.contains(&id) {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            if emphasis == Emphasis::Normal {
                items.push(item);
            } else {
                // Draw emphasized items last so they sit on top.
                emphasized.push(item.with_emphasis(emphasis));
            }
        }
        items.extend(emphasized);
        items
    }
}

#[derive(Default)]
struct SelectionKinds {
    midpoints: Vec<EntityId>,
    points: Vec<EntityId>,
    lines: Vec<EntityId>,
    circulars: Vec<EntityId>,
}

#[cfg(test)]
mod tests {
    use drift_core::{Constraint, DVec2};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// An editor with one slanted line; returns (editor, line, start, end).
    fn with_line() -> (Editor, EntityId, EntityId, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 3.0));
        let line = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        (ed, line, a, b)
    }

    fn length(ed: &Editor, a: EntityId, b: EntityId) -> f64 {
        ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap())
    }

    #[test]
    fn adding_a_relation_moves_geometry_and_undoes_in_one_step() {
        let (mut ed, line, a, b) = with_line();
        assert_eq!(ed.analysis().dof, Some(4));
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pa.y - pb.y).abs() < 1e-12);
        // Minimum change: both ends meet in the middle.
        assert!((pa.y - 1.5).abs() < 1e-9);
        assert_eq!(ed.analysis().dof, Some(3));
        assert!(ed.undo());
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 3.0));
        assert_eq!(ed.doc.constraint_count(), 0);
    }

    #[test]
    fn conflicting_and_redundant_constraints_are_refused() {
        let (mut ed, line, ..) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let before = ed.doc.clone();
        assert_eq!(
            ed.add_constraints("Vertical", &[Constraint::Vertical { line }]),
            Err(EditError::Degenerate)
        );
        assert_eq!(
            ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }]),
            Err(EditError::Redundant)
        );
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn dimensions_drive_geometry() {
        let (mut ed, _, a, b) = with_line();
        let ids = ed
            .add_constraints(
                "Length",
                &[Constraint::Distance {
                    a,
                    b,
                    value: length(&ed, a, b),
                }],
            )
            .unwrap();
        ed.set_dimension(ids[0], 25.0).unwrap();
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
        // An impossible value is refused and changes nothing.
        ed.add_constraints(
            "Fix",
            &[
                Constraint::Fix {
                    point: a,
                    at: ed.doc.point(a).unwrap(),
                },
                Constraint::Fix {
                    point: b,
                    at: ed.doc.point(b).unwrap(),
                },
            ],
        )
        .unwrap_err(); // fixing both ends of a dimensioned line is redundant
        assert!(ed.set_dimension(ids[0], -3.0).is_err());
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn dragging_respects_constraints() {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Length", &[Constraint::Distance { a, b, value: 10.0 }])
            .unwrap();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        // Drag the end point 5 mm right and 4 mm up: it stays horizontal and 10 long, so the
        // whole line slides.
        ed.doc.begin();
        let dragged = Param::point(b).to_vec();
        assert!(ed.drag(&[b], v(5.0, 4.0), &dragged));
        ed.commit("Move");
        assert!((length(&ed, a, b) - 10.0).abs() < 1e-9);
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pa.y - pb.y).abs() < 1e-12);
    }

    #[test]
    fn fixed_geometry_does_not_move() {
        let (mut ed, line, a, b) = with_line();
        ed.selection = [line].into();
        let fix = ed
            .relation_options()
            .into_iter()
            .find(|o| o.label == "Fix")
            .unwrap();
        ed.add_constraints("Fix", &fix.constraints).unwrap();
        assert_eq!(ed.analysis().dof, Some(0));
        let before = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        ed.doc.begin();
        ed.drag(&[line], v(3.0, 3.0), &[]);
        ed.commit("Move");
        assert_eq!((ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()), before);
        // Fully defined geometry is reported as such.
        let items = ed.items();
        assert!(items.iter().all(|i| i.dof == DofState::Full));
    }

    #[test]
    fn options_fit_the_selection() {
        let (mut ed, line, a, b) = with_line();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 5.0));
        let d = ed.doc.add_point(v(4.0, 9.0));
        let other = ed.doc.add_line(c, d).unwrap();
        ed.commit("Line");
        let labels = |ed: &Editor| -> Vec<&'static str> {
            ed.relation_options()
                .iter()
                .chain(ed.dimension_options().iter())
                .map(|o| o.label)
                .collect()
        };
        ed.selection = [line, other].into();
        assert_eq!(
            labels(&ed),
            [
                "Horizontal",
                "Vertical",
                "Parallel",
                "Perpendicular",
                "Equal",
                "Fix",
                "Angle"
            ]
        );
        ed.selection = [a, b].into();
        assert_eq!(
            labels(&ed),
            [
                "Coincident",
                "Horizontal",
                "Vertical",
                "Fix",
                "Distance",
                "Horizontal distance",
                "Vertical distance"
            ]
        );
        // The dimension is created at the current value, so adding it changes nothing.
        let dim = &ed.dimension_options()[0].constraints[0];
        assert!((dim.value().unwrap() - length(&ed, a, b)).abs() < 1e-12);
    }
}
