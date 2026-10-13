//! Editing state independent of the UI toolkit: document, history, selection, picking and
//! constraint solving.

use std::collections::BTreeSet;

use drift_core::params::{self, Evaluation};
use drift_core::{
    Bounds, Constraint, ConstraintId, DVec2, Document, DofState, Emphasis, EntityId, EntityKind,
    History, Kind, LineKind, MeasurementId, Shape, SketchItem, Units, Variable, VariableId,
    Variant, VariantId, dimension_kind,
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
    /// Whether snapping onto a midpoint or a curve should add a Midpoint / On curve relation
    /// (automatic relations).
    pub relations: bool,
    /// How to show lengths in readouts.
    pub units: Units,
    /// Alt is held (places freely, without snapping; the pen breaks a handle with it).
    pub alt: bool,
    /// Lines being drawn turn in steps of this many degrees, if angle snapping is on.
    pub angle_step: Option<f64>,
}

impl Pointer {
    pub fn new(raw: DVec2, tolerance: f64) -> Self {
        Self {
            raw,
            snap: None,
            tolerance,
            guide: None,
            relations: true,
            units: Units::default(),
            alt: false,
            angle_step: None,
        }
    }

    /// The line whose midpoint the pointer snapped to, if it should get a Midpoint relation.
    pub fn midpoint_of(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind == drift_geom::SnapKind::Midpoint && self.relations)
            .and_then(|s| s.entity)
    }

    /// The curve the pointer snapped onto (anywhere along it, or at a quadrant), if a new point
    /// there should get an On curve relation.
    pub fn on_curve(&self) -> Option<EntityId> {
        use drift_geom::SnapKind::{OnCurve, Quadrant};
        self.exact_snap()
            .filter(|s| matches!(s.kind, OnCurve | Quadrant) && self.relations)
            .and_then(|s| s.entity)
    }

    /// A snap to a specific spot on the geometry (a point, midpoint, quadrant or a point on a
    /// curve), which wins over inference guides. Grid snaps combine with guides instead.
    pub fn exact_snap(&self) -> Option<Snap> {
        self.snap.filter(|s| s.kind != drift_geom::SnapKind::Grid)
    }

    /// The effective position: an exact snap, else an inference guide (itself on the grid when
    /// grid snapping), else a grid snap, else the raw position. What the snap marker shows is
    /// where geometry goes.
    pub fn pos(&self) -> DVec2 {
        match (self.exact_snap(), self.guide, self.snap) {
            (Some(s), ..) => s.at,
            (None, Some(g), _) => g.at,
            (None, None, Some(grid)) => grid.at,
            (None, None, None) => self.raw,
        }
    }

    /// The automatic relation implied by the guide, if any (not when an exact snap decides
    /// the position).
    pub fn auto_relation(&self) -> Option<GuideKind> {
        self.guide
            .map(|g| g.kind)
            .filter(|k| *k != GuideKind::Aligned)
            .filter(|_| self.exact_snap().is_none())
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
            on_curve: self.on_curve(),
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
    /// A new point here should be kept on this curve.
    pub on_curve: Option<EntityId>,
}

impl Anchor {
    /// A new point at an exact position.
    pub fn at(pos: DVec2) -> Self {
        Self {
            pos,
            existing: None,
            midpoint_of: None,
            on_curve: None,
        }
    }

    /// The point entity for this anchor, creating it if needed (with its Midpoint or On curve
    /// relation, if it was snapped to a midpoint or curve). Call inside a transaction.
    pub fn resolve(&self, doc: &mut Document) -> EntityId {
        match self.existing {
            Some(id) if doc.contains(id) => id,
            _ => {
                let point = doc.add_point(self.pos);
                if let Some(line) = self.midpoint_of {
                    let _ = doc.add_constraint(Constraint::Midpoint { point, line });
                } else if let Some(curve) = self.on_curve {
                    let _ = doc.add_constraint(Constraint::PointOnCurve { point, curve });
                }
                point
            }
        }
    }
}

/// A drag's cached scope: the dragged ids, the entity and constraint counts it was worked out
/// for, and the connected entities.
type DragScope = (
    Vec<EntityId>,
    (usize, usize),
    std::rc::Rc<std::collections::HashSet<EntityId>>,
);

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
    /// A dimension refused because it would over-define the sketch, with its label placement:
    /// the UI offers to add it as a driven (reference) dimension instead.
    pub pending_driven: Option<(Constraint, Option<DVec2>)>,
    /// Whether drawing tools add relations they infer (horizontal / vertical lines, rectangles).
    pub auto_relations: bool,
    /// Variable and linked-dimension values as of the last [`sync`](Self::sync).
    evaluation: Evaluation,
    evaluated: Option<u64>,
    /// Expressions typed into a tool's fields (index = field) that use variables, so the
    /// dimensions the tool creates can stay linked to them.
    pub typed_exprs: [Option<String>; 2],
    /// Named copies of the document to return to (kept for the session).
    pub snapshots: Vec<(String, Document)>,
    /// A variable being scrubbed with a slider: its expression before the scrub started.
    scrubbing: Option<(VariableId, String)>,
    /// Closed regions of the solid curves, as of a document revision.
    regions: Option<(u64, std::rc::Rc<Vec<drift_geom::regions::Region>>)>,
    /// A point inside the selected region (regions are recomputed, so they're picked by a
    /// point rather than an id).
    pub selected_region: Option<DVec2>,
    /// The entity each curve passed to the region finder came from (by index).
    region_ids: std::rc::Rc<Vec<EntityId>>,
    pub selected_fills: BTreeSet<drift_core::FillId>,
    pub hovered_fill: Option<drift_core::FillId>,
    /// How the Fill tool fills.
    pub fill_style: crate::fills::FillStyle,
    /// A fill whose boundary the Fill tool's next click re-picks.
    pub repick_fill: Option<drift_core::FillId>,
    pub selected_annotation: Option<drift_core::AnnotationId>,
    pub hovered_annotation: Option<drift_core::AnnotationId>,
    /// A note just placed by a tool, whose text the UI should let the user type.
    pub pending_note_edit: Option<drift_core::AnnotationId>,
    /// The group being edited (its members pick one by one).
    pub open_group: Option<drift_core::GroupId>,
    /// Picked and hovered copies of blocks.
    pub selected_instances: BTreeSet<drift_core::InstanceId>,
    pub hovered_instance: Option<drift_core::InstanceId>,
    /// A block being edited in place.
    pub block_edit: Option<crate::blocks::BlockEdit>,
    /// The block the Place block tool places.
    pub place_block: Option<drift_core::BlockId>,
    pub(crate) fill_shapes: Option<(
        u64,
        std::rc::Rc<std::collections::BTreeMap<drift_core::FillId, crate::fills::FillShape>>,
    )>,
    /// Each fill's last region, kept for when its boundary is lost.
    pub(crate) last_fill_shapes:
        std::collections::HashMap<drift_core::FillId, drift_geom::regions::Region>,
    /// Sides of the next polygon (0: the default), and whether it's drawn around its circle.
    pub polygon_sides: usize,
    pub polygon_circumscribed: bool,
    /// Which slot the Slot tool draws (U cycles).
    pub slot_mode: crate::tools::SlotMode,
    /// The underlay picked in the Layers tab (calibrate and move act on it).
    pub selected_underlay: Option<drift_core::UnderlayId>,
    pub(crate) underlay_cache: crate::underlays::UnderlayCache,
    /// Show every end and centre point, or (Illustrator-like) only those of hovered or picked
    /// curves.
    pub always_endpoints: bool,
    /// The point the pointer has snapped to while drawing (lit up like a hovered one).
    pub snapped_point: Option<EntityId>,
    /// What a drag of these entities can move (connected through points and relations), kept
    /// while the sketch's structure stays the same: (dragged ids, entity and constraint
    /// counts, the connected set).
    drag_scope: std::cell::RefCell<Option<DragScope>>,
    /// The sketch text picked and under the pointer.
    pub selected_text: Option<drift_core::TextId>,
    pub hovered_text: Option<drift_core::TextId>,
    pub(crate) text_cache: std::cell::RefCell<crate::texts::TextCache>,
    /// Overlapping curves, as of a document revision.
    pub(crate) overlap_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<Vec<EntityId>>)>>,
    /// The text being typed in place on the canvas (just placed, or double-clicked).
    pub editing_text: Option<drift_core::TextId>,
    /// Cap height for new text (0: the default).
    pub text_height: f64,
    /// Bézier handles and their owners, by document revision.
    pub(crate) handle_cache:
        std::cell::RefCell<Option<(u64, std::rc::Rc<crate::curves::HandleOwners>)>>,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut editor = Self {
            doc,
            auto_relations: true,
            ..Default::default()
        };
        // Bring linked dimensions up to date with their expressions (not an undo step).
        editor.doc.begin();
        if editor.apply_parameters().is_ok() {
            editor.solve(&[]);
        }
        editor.doc.commit("Load");
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
        if self.evaluated != Some(self.doc.revision()) {
            self.evaluation = params::evaluate(&self.doc);
            self.evaluated = Some(self.doc.revision());
        }
        let doc = &self.doc;
        // Hidden or locked geometry can't stay selected.
        self.selection
            .retain(|id| doc.contains(*id) && doc.is_visible(*id) && !doc.layer_of(*id).locked);
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

    /// The entities a drag of `ids` can move (cached while the structure is unchanged).
    fn drag_scope(&self, ids: &[EntityId]) -> std::rc::Rc<std::collections::HashSet<EntityId>> {
        let structure = (self.doc.len(), self.doc.constraints().count());
        if let Some((key, s, scope)) = &*self.drag_scope.borrow()
            && key == ids
            && *s == structure
        {
            return scope.clone();
        }
        let scope = std::rc::Rc::new(drift_solver::connected(&self.doc, ids.iter().copied()));
        *self.drag_scope.borrow_mut() = Some((ids.to_vec(), structure, scope.clone()));
        scope
    }

    fn solve_in(
        &mut self,
        dragged: &[Param],
        scope: std::rc::Rc<std::collections::HashSet<EntityId>>,
    ) -> SolveReport {
        drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                dragged: dragged.to_vec(),
                scope: Some(scope),
                ..Default::default()
            },
        )
    }

    pub(crate) fn solve(&mut self, dragged: &[Param]) -> SolveReport {
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
    pub(crate) fn new_conflicts(&self, report: &SolveReport) -> bool {
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
        self.refresh_driven();
        self.adopt_into_open_group();
        let tx = self.doc.commit(label);
        self.history.push(tx);
        self.sync();
        true
    }

    /// Update driven dimensions to the geometry's current measurements (inside a transaction).
    fn refresh_driven(&mut self) {
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

    /// Move entities by `delta` inside an open transaction, keeping constraints satisfied.
    /// `dragged` are the parameters that should follow the pointer exactly. Returns false (and
    /// leaves the geometry where it was) if the move is impossible.
    pub fn drag(&mut self, ids: &[EntityId], delta: DVec2, dragged: &[Param]) -> bool {
        // Anchors carry their curve handles.
        let ids = &self.with_handles(ids);
        // Only what's connected to the dragged geometry is solved.
        let scope = self.drag_scope(ids);
        self.doc.translate(ids.iter().copied(), delta);
        let report = self.solve_in(dragged, scope.clone());
        let ok = !self.new_conflicts(&report);
        if !ok {
            self.doc.translate(ids.iter().copied(), -delta);
            self.solve_in(dragged, scope.clone());
        }
        // Only the connected part can have moved.
        self.index.update_moved(&self.doc, scope.iter().copied());
        ok
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
            self.settle_fillet(c);
        }
        self.check_new_constraints(label, &ids, &before, &degenerate_before)?;
        Ok(ids)
    }

    /// Solve after constraints `ids` were made active in the open transaction, and commit it,
    /// or roll it back if they conflict, collapse geometry or add nothing new.
    fn check_new_constraints(
        &mut self,
        label: &str,
        ids: &[ConstraintId],
        before: &SolveReport,
        degenerate_before: &[EntityId],
    ) -> Result<(), EditError> {
        let report = drift_solver::solve(
            &mut self.doc,
            &SolveOptions {
                analyze: true,
                ..Default::default()
            },
        );
        let error = if self.new_conflicts(&report) {
            Some(EditError::Conflict)
        } else if self.new_degeneracy(degenerate_before) {
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
        Ok(())
    }

    /// Add a dimension with its label placed at `placement` (offset from its anchor), as one
    /// undoable step.
    pub fn add_dimension(
        &mut self,
        constraint: Constraint,
        placement: Option<DVec2>,
    ) -> Result<ConstraintId, EditError> {
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
        let before = self.analysis.clone();
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

    /// The closed regions formed by solid curves (construction and centre lines are
    /// references, not boundaries), recomputed when the document changes.
    pub fn regions(&mut self) -> std::rc::Rc<Vec<drift_geom::regions::Region>> {
        let revision = self.doc.revision();
        if let Some((r, regions)) = &self.regions
            && *r == revision
        {
            return regions.clone();
        }
        let (ids, curves): (Vec<EntityId>, Vec<Shape>) = self
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point() && e.style == LineKind::Solid)
            .filter_map(|(id, _)| Some((id, self.doc.shape(id)?)))
            .unzip();
        let regions = std::rc::Rc::new(drift_geom::regions::regions(&curves));
        self.regions = Some((revision, regions.clone()));
        self.region_ids = std::rc::Rc::new(ids);
        regions
    }

    /// The entity of each curve index in [`regions`](Self::regions)' loops.
    pub fn region_ids(&mut self) -> std::rc::Rc<Vec<EntityId>> {
        self.regions();
        self.region_ids.clone()
    }

    /// The selected region, if its seed point is still inside one.
    pub fn region(&mut self) -> Option<drift_geom::regions::Region> {
        let seed = self.selected_region?;
        let regions = self.regions();
        let found = drift_geom::regions::region_at(&regions, seed).cloned();
        if found.is_none() {
            self.selected_region = None;
        }
        found
    }

    // ----- Parameters ------------------------------------------------------------------------

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
        params::evaluate_text(&self.doc, &self.evaluation, text, kind)
            .map_err(|e| EditError::Invalid(e.0))
    }

    /// Re-evaluate variables and push linked dimension values into the constraints (inside a
    /// transaction). Fails on any evaluation error.
    fn apply_parameters(&mut self) -> Result<(), EditError> {
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
        Ok(())
    }

    /// Apply parameters, solve and commit the open transaction, or roll it back.
    fn finish_parameter_edit(
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
    fn parameter_edit(
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

    /// Change the display units (an undoable document setting; no geometry moves).
    pub fn set_units(&mut self, units: Units) {
        self.doc.begin();
        self.doc.set_units(units);
        self.commit("Change units");
    }

    /// Save the current variable values as a new variant.
    pub fn save_variant(&mut self, name: &str) -> VariantId {
        let values = self
            .doc
            .variables()
            .map(|(id, v)| (id, v.expr.clone()))
            .collect();
        self.doc.begin();
        let id = self.doc.add_variant(Variant {
            name: name.trim().to_owned(),
            values,
        });
        self.commit("Save variant");
        id
    }

    /// Overwrite a variant with the current variable values.
    pub fn update_variant(&mut self, id: VariantId) {
        let Some(mut variant) = self.doc.variant(id).cloned() else {
            return;
        };
        variant.values = self
            .doc
            .variables()
            .map(|(v, var)| (v, var.expr.clone()))
            .collect();
        self.doc.begin();
        self.doc.set_variant(id, variant);
        self.commit("Update variant");
    }

    pub fn rename_variant(&mut self, id: VariantId, name: &str) {
        let Some(mut variant) = self.doc.variant(id).cloned() else {
            return;
        };
        variant.name = name.trim().to_owned();
        self.doc.begin();
        self.doc.set_variant(id, variant);
        self.commit("Rename variant");
    }

    pub fn remove_variant(&mut self, id: VariantId) {
        self.doc.begin();
        self.doc.remove_variant(id);
        self.commit("Delete variant");
    }

    /// Apply a variant's variable values (one undo step; refused if the sketch can't take them).
    pub fn apply_variant(&mut self, id: VariantId) -> Result<(), EditError> {
        let Some(variant) = self.doc.variant(id).cloned() else {
            return Err(EditError::Invalid(format!("{id} no longer exists")));
        };
        let label = format!("Apply {}", variant.name);
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

    /// The variant whose values match the current variables, if any.
    pub fn active_variant(&self) -> Option<VariantId> {
        self.doc.variants().find_map(|(id, v)| {
            (!v.values.is_empty()
                && v.values
                    .iter()
                    .all(|(var, e)| self.doc.variable(*var).is_none_or(|x| x.expr == *e)))
            .then_some(id)
        })
    }

    /// The geometry of each variant (solved on a copy), for drawing ghosts.
    pub fn variant_geometry(&self, id: VariantId) -> Option<Vec<SketchItem>> {
        let variant = self.doc.variant(id)?;
        let mut copy = Editor {
            doc: self.doc.clone(),
            ..Default::default()
        };
        copy.doc.begin();
        for (var, expr) in &variant.values {
            let _ = copy.doc.set_variable_expr(*var, expr);
        }
        copy.apply_parameters().ok()?;
        let report = copy.solve(&[]);
        report.converged.then(|| {
            copy.doc
                .iter()
                .filter(|(_, e)| !e.kind.is_point() && e.style != LineKind::Construction)
                .filter_map(|(id, _)| copy.doc.shape(id))
                .map(SketchItem::new)
                .collect()
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

    /// Keep a named copy of the document to return to later.
    pub fn take_snapshot(&mut self, name: &str) {
        self.snapshots
            .push((name.trim().to_owned(), self.doc.clone()));
    }

    /// Return the document to a snapshot (one undo step).
    pub fn restore_snapshot(&mut self, index: usize) {
        let Some((name, snapshot)) = self.snapshots.get(index).cloned() else {
            return;
        };
        self.doc.begin();
        self.doc.restore_from(&snapshot);
        self.commit(&format!("Restore {name}"));
        self.selection.clear();
        self.selected_midpoints.clear();
        self.sync();
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
    fn existing_relation(&self, wanted: &Constraint) -> Option<ConstraintId> {
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

    /// The relations of `option` that are already on the sketch.
    pub fn existing_relations(&self, option: &ConstraintOption) -> Vec<ConstraintId> {
        option
            .constraints
            .iter()
            .filter_map(|c| self.existing_relation(c))
            .collect()
    }

    /// The relations of `option` that aren't on the sketch yet.
    pub fn missing_relations(&self, option: &ConstraintOption) -> Vec<Constraint> {
        option
            .constraints
            .iter()
            .filter(|c| self.existing_relation(c).is_none())
            .copied()
            .collect()
    }

    /// Whether everything `option` would add is already there.
    pub fn has_relations(&self, option: &ConstraintOption) -> bool {
        !option.constraints.is_empty() && self.missing_relations(option).is_empty()
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
            let report = drift_solver::solve(
                &mut self.doc,
                &SolveOptions {
                    analyze: true,
                    ..Default::default()
                },
            );
            self.new_conflicts(&report)
        };
        self.doc.rollback();
        self.sync();
        bad
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
        if let ([point], [curve], []) | ([point], [], [curve]) = (
            sel.points.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
        ) {
            out.push(opt(
                "On curve",
                vec![Constraint::PointOnCurve {
                    point: *point,
                    curve: *curve,
                }],
            ));
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
                    out.push(opt(
                        "Collinear",
                        vec![Constraint::Collinear { a: *a, b: *b }],
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
                if let [a, b] = circulars {
                    let (a, b) = (*a, *b);
                    out.push(opt("Tangent", vec![Constraint::Tangent { a, b }]));
                    out.push(opt("Concentric", vec![Constraint::Concentric { a, b }]));
                    out.push(opt("Coradial", vec![Constraint::Coradial { a, b }]));
                }
                out.push(opt(
                    "Equal",
                    circulars
                        .windows(2)
                        .map(|w| Constraint::Equal { a: w[0], b: w[1] })
                        .collect(),
                ));
            }
            ([], [line], [curve]) => {
                out.push(opt(
                    "Tangent",
                    vec![Constraint::Tangent {
                        a: *line,
                        b: *curve,
                    }],
                ));
            }
            ([a, b], [axis], []) => {
                out.push(opt(
                    "Symmetric",
                    vec![Constraint::Symmetric {
                        a: *a,
                        b: *b,
                        axis: *axis,
                    }],
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
            ([], [a, b], []) => {
                let (a, b) = crate::annotations::corner_order(&self.doc, *a, *b);
                push("Angle", Constraint::Angle { a, b, degrees: 0.0 })
            }
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
                // No relation buttons for ellipses yet.
                Some(
                    EntityKind::Ellipse { .. }
                    | EntityKind::EllipseArc { .. }
                    | EntityKind::Bezier { .. },
                )
                | None => {}
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
        if self.selection.is_empty()
            && self.selected_fills.is_empty()
            && self.selected_annotation.is_none()
            && self.selected_instances.is_empty()
            && self.selected_text.is_none()
        {
            return;
        }
        self.doc.begin();
        if let Some(id) = self.selected_text.take() {
            self.doc.remove_text(id);
        }
        for id in std::mem::take(&mut self.selected_instances) {
            self.doc.remove_instance(id);
        }
        if let Some(id) = self.selected_annotation.take() {
            self.doc.remove_annotation(id);
        }
        for id in std::mem::take(&mut self.selected_fills) {
            self.doc.remove_fill(id);
        }
        self.doc.delete(self.selection.iter().copied());
        self.commit("Delete");
    }

    /// Copy the selection in place (Blender's Shift+D): the curves with their points, the
    /// relations and dimensions among them, fills and hatches of copied areas, and picked
    /// block copies. The copies become the
    /// selection. Returns how many things were copied.
    pub fn duplicate_selection(&mut self) -> usize {
        let picked = crate::clipboard::Picked {
            curves: self.selection.iter().copied().collect(),
            instances: self.selected_instances.iter().copied().collect(),
            texts: Vec::new(),
            fills: self.selected_fills.iter().copied().collect(),
        };
        if picked.curves.is_empty() && picked.instances.is_empty() {
            return 0;
        }
        let source = self.doc.clone();
        self.doc.begin();
        let made = crate::clipboard::copy_into(&source, &mut self.doc, &picked, DVec2::ZERO, true);
        self.commit("Duplicate");
        self.selection = picked
            .curves
            .iter()
            .filter_map(|id| made.entities.get(id).copied())
            .collect();
        let n = self.selection.len() + made.instances.len();
        self.selected_instances = made.instances.into_iter().collect();
        self.selected_fills = made.fills;
        self.selected_region = None;
        self.selected_annotation = None;
        n
    }

    /// Curves drawn on top of other curves (the later-made one of each pair): the same line
    /// twice, or walls of abutting shapes that run along each other. They are easy to make
    /// by accident and confuse trimming and selection. Found once per change.
    pub fn overlapping(&self) -> std::rc::Rc<Vec<EntityId>> {
        let revision = self.doc.revision();
        if let Some((r, found)) = &*self.overlap_cache.borrow()
            && *r == revision
        {
            return found.clone();
        }
        let mut found = Vec::new();
        for (id, e) in self.doc.iter() {
            if e.kind.is_point() {
                continue;
            }
            let Some(shape) = self.index.shape(id) else {
                continue;
            };
            if !matches!(
                shape,
                Shape::Line { .. } | Shape::Arc { .. } | Shape::Circle { .. }
            ) {
                continue;
            }
            let under = self.index.query(shape.bounds()).into_iter().any(|other| {
                other < id
                    && self
                        .index
                        .shape(other)
                        .is_some_and(|s| drift_geom::intersect::overlapping(shape, s))
            });
            if under {
                found.push(id);
            }
        }
        let found = std::rc::Rc::new(found);
        *self.overlap_cache.borrow_mut() = Some((revision, found.clone()));
        found
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

    /// Move the selection (curves, block copies, the picked text) by `delta` as one undo
    /// step. `None` with nothing picked; `Some(false)` if relations hold it in place.
    pub fn nudge(&mut self, delta: DVec2) -> Option<bool> {
        let ids: Vec<EntityId> = self.selection.iter().copied().collect();
        let instances = self.picked_instances();
        let text = self.selected_text;
        if ids.is_empty() && instances.is_empty() && text.is_none() {
            return None;
        }
        self.doc.begin();
        self.move_instances(&instances, delta);
        if let Some(id) = text
            && let Some(mut t) = self.doc.text(id).cloned()
        {
            t.at += delta;
            self.doc.set_text(id, t);
        }
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
        if let Some(seed) = &mut self.selected_region {
            *seed += delta;
        }
        Some(self.commit("Nudge"))
    }

    /// The curves joined end to end with `start` (lines and arcs meeting at shared end
    /// points; circles stand alone), as Revit's Tab selects a chain of walls.
    pub fn chain_of(&self, start: EntityId) -> BTreeSet<EntityId> {
        let ends = |id: EntityId| -> Vec<EntityId> {
            match self.doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Line { a, b }) => vec![a, b],
                Some(EntityKind::Arc { start, end, .. }) => vec![start, end],
                _ => Vec::new(),
            }
        };
        let mut chain = BTreeSet::new();
        if self.doc.get(start).is_none_or(|e| e.kind.is_point()) {
            return chain;
        }
        let mut queue = vec![start];
        chain.insert(start);
        while let Some(c) = queue.pop() {
            for p in ends(c) {
                for u in self.doc.users_of(p) {
                    if !chain.contains(&u) && !self.is_locked(u) && ends(u).contains(&p) {
                        chain.insert(u);
                        queue.push(u);
                    }
                }
            }
        }
        chain
    }

    /// Tab: select the chain under the pointer, or widen the selection to whole chains.
    /// Returns how many curves are selected.
    pub fn select_chain(&mut self) -> usize {
        let seeds: Vec<EntityId> = match self.hovered {
            Some(h) if self.doc.get(h).is_some_and(|e| !e.kind.is_point()) => vec![h],
            _ => self.selection.iter().copied().collect(),
        };
        let mut chain = BTreeSet::new();
        for s in seeds {
            chain.extend(self.chain_of(s));
        }
        if chain.is_empty() {
            return 0;
        }
        // Within an open group, stay inside it.
        if let Some(g) = self.open_group.and_then(|g| self.doc.group(g)) {
            chain.retain(|id| g.members.contains(id));
        }
        self.selected_fills.clear();
        self.selected_region = None;
        self.selection = chain;
        self.expand_groups();
        self.selection.len()
    }

    /// While a group is open for editing, what's drawn joins it: curves and standalone points
    /// created in the open transaction (a curve's own points ride along with it).
    fn adopt_into_open_group(&mut self) {
        let Some(g) = self.open_group else { return };
        if self.doc.group(g).is_none() {
            self.open_group = None;
            return;
        }
        let created = self.doc.created_in_transaction();
        let new: Vec<EntityId> = created
            .into_iter()
            .filter(|id| {
                self.doc
                    .get(*id)
                    .is_some_and(|e| !e.kind.is_point() || self.doc.users_of(*id).next().is_none())
            })
            .collect();
        if !new.is_empty() {
            let _ = self.doc.add_group_members(g, new);
        }
    }

    /// What picking `id` selects: its whole group, unless that group is open.
    pub fn pick_set(&self, id: EntityId) -> BTreeSet<EntityId> {
        match self.doc.group_of(id) {
            Some(g) if self.open_group != Some(g) => self
                .doc
                .group(g)
                .map_or_else(|| [id].into(), |g| g.members.clone()),
            _ => [id].into(),
        }
    }

    /// Widen the selection to whole groups (except the open one).
    pub fn expand_groups(&mut self) {
        let extra: Vec<EntityId> = self
            .selection
            .iter()
            .flat_map(|id| self.pick_set(*id))
            .collect();
        self.selection.extend(extra);
    }

    /// The group the selection is exactly, if any.
    pub fn selected_group(&self) -> Option<drift_core::GroupId> {
        let first = *self.selection.first()?;
        let g = self.doc.group_of(first)?;
        (self.doc.group(g)?.members == self.selection).then_some(g)
    }

    /// Group the selection (Ctrl/⌘+G). Curves' own points aren't members: the curves carry
    /// them.
    pub fn make_group(&mut self) -> Option<drift_core::GroupId> {
        let referenced = self.doc.defining_points(
            self.selection
                .iter()
                .copied()
                .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point())),
        );
        let members: BTreeSet<EntityId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect();
        if members.len() < 2 {
            self.notice = Some("Select two or more things to group".into());
            return None;
        }
        let n = self.doc.groups().count() + 1;
        self.doc.begin();
        let id = self
            .doc
            .add_group(drift_core::Group {
                name: format!("Group {n}"),
                members,
            })
            .ok();
        self.commit("Group");
        self.open_group = None;
        self.expand_groups();
        id
    }

    /// Dissolve the groups the selection belongs to (Ctrl/⌘+Shift+G).
    pub fn ungroup(&mut self) {
        let groups: BTreeSet<drift_core::GroupId> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.group_of(*id))
            .collect();
        if groups.is_empty() {
            return;
        }
        self.doc.begin();
        for g in groups {
            self.doc.remove_group(g);
        }
        self.commit("Ungroup");
        self.open_group = None;
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
        let mut b: Bounds = self
            .doc
            .iter()
            .filter_map(|(id, _)| self.index.shape(id))
            .map(Shape::bounds)
            .collect();
        for (id, _) in self.doc.instances() {
            let i = self.instance_bounds(id);
            if !i.is_empty() {
                b.include(i.min);
                b.include(i.max);
            }
        }
        b
    }

    /// Points whose markers show when end points are only shown in use: those of hovered or
    /// picked curves, and hovered, picked or snapped-to points.
    pub fn touched_points(&self) -> BTreeSet<EntityId> {
        let mut out = BTreeSet::new();
        let active = self
            .selection
            .iter()
            .chain(self.hovered.iter())
            .chain(self.highlight.iter())
            .chain(self.snapped_point.iter());
        for &id in active {
            if let Some(e) = self.doc.get(id) {
                if e.kind.is_point() {
                    out.insert(id);
                } else {
                    out.extend(e.kind.points());
                }
            }
        }
        out
    }

    /// Render items for the document, with selection and hover emphasis.
    /// Render items with constraint-status colouring (see [`items_styled`](Self::items_styled)).
    pub fn items(&self) -> Vec<SketchItem> {
        self.items_styled(true, true)
    }

    /// True if the entity is on a locked layer (it can't be selected or modified).
    pub fn is_locked(&self, id: EntityId) -> bool {
        self.doc.layer_of(id).locked
    }

    /// Render items: each entity with its layer's (or its own) weight and line type, and either
    /// constraint-status colours or layer colours (adjusted for contrast with the theme).
    pub fn items_styled(&self, status_colors: bool, dark: bool) -> Vec<SketchItem> {
        let mut items = Vec::with_capacity(self.doc.len());
        let mut emphasized = Vec::new();
        let hidden = self.hidden_handles();
        let shown_ends = (!self.always_endpoints).then(|| self.touched_points());
        for (id, entity) in self.doc.iter() {
            if hidden.contains(&id) {
                continue;
            }
            // End and centre points of untouched curves stay out of the way.
            if let Some(shown) = &shown_ends
                && entity.kind.is_point()
                && matches!(
                    self.index.role(id),
                    Some(PointRole::Endpoint | PointRole::Center)
                )
                && !shown.contains(&id)
            {
                continue;
            }
            let Some(shape) = self.index.shape(id) else {
                continue;
            };
            let dof = match self.analysis.definitions.get(&id) {
                Some(Definition::Under) => DofState::Under,
                Some(Definition::Full) => DofState::Full,
                Some(Definition::Over) => DofState::Over,
                None => DofState::Unknown,
            };
            let layer = self.doc.layer_of(id);
            let look = entity.appearance;
            let color = if status_colors {
                None
            } else {
                look.color.or(layer.color).map(|c| contrast(c.0, dark))
            };
            let dof = if status_colors || dof == DofState::Over {
                dof
            } else {
                DofState::Unknown
            };
            let mut item = SketchItem::new(*shape)
                .with_kind(entity.style)
                .with_dof(dof)
                .with_look(
                    color.flatten(),
                    look.weight.unwrap_or(layer.weight).0,
                    look.line_type.unwrap_or(layer.line_type),
                );
            let role = self.index.role(id);
            // End and centre points are small markers; handles and lone points full size.
            if role.is_some_and(|r| r != PointRole::Standalone && r != PointRole::Handle) {
                item = item.as_vertex();
            }
            let emphasis = if self.selection.contains(&id) {
                Emphasis::Selected
            } else if self.hovered == Some(id)
                || self.highlight.contains(&id)
                || self.snapped_point == Some(id)
            {
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
        // Copies of blocks (not the one being edited in place).
        let editing = self.block_edit.map(|e| e.instance);
        for (id, i) in self.doc.instances() {
            let Some(layer) = self.doc.layer(i.layer) else {
                continue;
            };
            if !layer.visible || Some(id) == editing {
                continue;
            }
            let emphasis = if self.selected_instances.contains(&id) {
                Emphasis::Selected
            } else if self.hovered_instance == Some(id) {
                Emphasis::Hovered
            } else {
                Emphasis::Normal
            };
            for item in self.doc.instance_items(id) {
                if matches!(item.shape, Shape::Point(_)) {
                    continue;
                }
                let look = item.appearance;
                let color = if status_colors {
                    None
                } else {
                    look.color.or(layer.color).map(|c| contrast(c.0, dark))
                };
                let sketch = SketchItem::new(item.shape)
                    .with_kind(item.style)
                    .with_look(
                        color.flatten(),
                        look.weight.unwrap_or(layer.weight).0,
                        look.line_type.unwrap_or(layer.line_type),
                    )
                    .with_emphasis(emphasis);
                if emphasis == Emphasis::Normal {
                    items.push(sketch);
                } else {
                    emphasized.push(sketch);
                }
            }
        }
        items.extend(emphasized);
        items
    }
}

/// A layer colour that would be hard to see on the theme's background (WCAG contrast ratio
/// under 2:1, e.g. black on the dark theme) is shown as the theme's foreground instead
/// (`None`), like AutoCAD's colour 7.
pub(crate) fn contrast(c: [u8; 3], dark: bool) -> Option<[u8; 3]> {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2]);
    let background: f64 = if dark { 0.012 } else { 0.95 };
    let ratio = (lum.max(background) + 0.05) / (lum.min(background) + 0.05);
    (ratio >= 2.0).then_some(c)
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
                "Collinear",
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

    /// A line, a circle beside it and a second circle: (editor, line, circle, circle2, centre).
    fn with_curves() -> (Editor, EntityId, EntityId, EntityId, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(20.0, 0.0));
        let line = ed.doc.add_line(a, b).unwrap();
        let c = ed.doc.add_point(v(10.0, 7.0));
        let circle = ed.doc.add_circle(c, 4.0).unwrap();
        let c2 = ed.doc.add_point(v(30.0, 8.0));
        let circle2 = ed.doc.add_circle(c2, 2.0).unwrap();
        ed.commit("Setup");
        (ed, line, circle, circle2, c)
    }

    fn labels(ed: &Editor) -> Vec<&'static str> {
        ed.relation_options().iter().map(|o| o.label).collect()
    }

    fn apply(ed: &mut Editor, label: &str) {
        let option = ed
            .relation_options()
            .into_iter()
            .find(|o| o.label == label)
            .unwrap_or_else(|| panic!("no {label} option"));
        ed.add_constraints(label, &option.constraints).unwrap();
    }

    #[test]
    fn curve_relations_are_offered_and_applied() {
        let (mut ed, line, circle, circle2, c) = with_curves();
        ed.selection = [line, circle].into();
        assert_eq!(labels(&ed), ["Tangent", "Fix"]);
        apply(&mut ed, "Tangent");
        let centre = ed.doc.point(c).unwrap();
        let radius = drift_solver::radius_of(&ed.doc, circle).unwrap();
        let (a, b) = match ed.doc.get(line).unwrap().kind {
            EntityKind::Line { a, b } => (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()),
            _ => unreachable!(),
        };
        let distance = (b - a).perp_dot(centre - a).abs() / (b - a).length();
        assert!((distance - radius).abs() < 1e-9, "{distance} vs {radius}");
        // Still on the side it started.
        assert!(centre.y > a.y);

        ed.selection = [circle, circle2].into();
        assert_eq!(
            labels(&ed),
            ["Tangent", "Concentric", "Coradial", "Equal", "Fix"]
        );
        apply(&mut ed, "Coradial");
        let centres = |ed: &Editor| {
            [circle, circle2].map(|e| match ed.doc.get(e).unwrap().kind {
                EntityKind::Circle { center, radius } => (ed.doc.point(center).unwrap(), radius),
                _ => unreachable!(),
            })
        };
        let [(c1, r1), (c2, r2)] = centres(&ed);
        assert!(c1.distance(c2) < 1e-9 && (r1 - r2).abs() < 1e-9);
        // Coradial implies concentric.
        let concentric = Constraint::Concentric {
            a: circle,
            b: circle2,
        };
        assert_eq!(
            ed.add_constraints("Concentric", &[concentric]),
            Err(EditError::Redundant)
        );
    }

    #[test]
    fn point_relations_are_offered() {
        let (mut ed, line, circle, ..) = with_curves();
        ed.doc.begin();
        let p = ed.doc.add_point(v(5.0, 20.0));
        let q = ed.doc.add_point(v(8.0, -12.0));
        ed.commit("Points");
        ed.selection = [p, circle].into();
        assert_eq!(labels(&ed), ["On curve", "Fix"]);
        apply(&mut ed, "On curve");
        let (centre, radius) = match ed.doc.get(circle).unwrap().kind {
            EntityKind::Circle { center, radius } => (ed.doc.point(center).unwrap(), radius),
            _ => unreachable!(),
        };
        assert!((ed.doc.point(p).unwrap().distance(centre) - radius).abs() < 1e-9);

        ed.selection = [q, line].into();
        assert_eq!(labels(&ed), ["Midpoint", "On curve", "Fix"]);

        ed.selection = [p, q, line].into();
        assert_eq!(labels(&ed), ["Symmetric", "Fix"]);
        apply(&mut ed, "Symmetric");
        // The line is the x axis (it may have moved, but stays horizontal-ish): q mirrors p.
        let (a, b) = match ed.doc.get(line).unwrap().kind {
            EntityKind::Line { a, b } => (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()),
            _ => unreachable!(),
        };
        let (pp, qq) = (ed.doc.point(p).unwrap(), ed.doc.point(q).unwrap());
        let u = (b - a).normalize();
        assert!((pp - qq).dot(u).abs() < 1e-9);
        assert!(((pp + qq) * 0.5 - a).perp_dot(u).abs() < 1e-9);
    }

    #[test]
    fn over_defining_dimensions_can_be_driven() {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let length = ed
            .add_dimension(Constraint::Distance { a, b, value: 12.0 }, None)
            .unwrap();
        // The horizontal distance is now implied: refused, with the driven option offered.
        let horizontal = Constraint::HorizontalDistance { a, b, value: 0.0 };
        let horizontal =
            horizontal.with_value(drift_solver::measure(&ed.doc, &horizontal).unwrap());
        let placement = Some(v(0.0, 4.0));
        assert_eq!(
            ed.add_dimension(horizontal, placement),
            Err(EditError::Redundant)
        );
        assert_eq!(ed.pending_driven, Some((horizontal, placement)));
        let (constraint, placement) = ed.pending_driven.take().unwrap();
        let driven = ed.add_driven_dimension(constraint, placement).unwrap();
        assert!(ed.doc.is_driven(driven));
        assert_eq!(ed.doc.placement(driven), placement);
        // It measures rather than imposes its value.
        let measured = |ed: &Editor| ed.doc.constraint(driven).unwrap().value().unwrap().abs();
        assert!((measured(&ed) - 12.0).abs() < 1e-9);
        assert!(ed.analysis().conflicts.is_empty());

        // Driven values follow changes to the geometry.
        ed.set_dimension(length, 20.0).unwrap();
        assert!((measured(&ed) - 20.0).abs() < 1e-9);
        assert!(matches!(
            ed.set_dimension(driven, 3.0),
            Err(EditError::Invalid(_))
        ));
        // Making it driving would over-define, until the length is gone.
        assert_eq!(ed.set_driven(driven, false), Err(EditError::Redundant));
        assert!(ed.doc.is_driven(driven));
        ed.remove_constraint(length);
        ed.set_driven(driven, false).unwrap();
        assert!(!ed.doc.is_driven(driven));
        ed.set_dimension(driven, 7.0).unwrap();
        assert!((ed.doc.point(b).unwrap().x - ed.doc.point(a).unwrap().x).abs() - 7.0 < 1e-9);
        // Each switch is one undo step.
        ed.undo();
        ed.undo();
        assert!(ed.doc.is_driven(driven));
    }

    /// A horizontal line with a length dimension: (editor, dimension, start, end).
    fn dimensioned_line() -> (Editor, ConstraintId, EntityId, EntityId) {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let length = length(&ed, a, b);
        let d = ed
            .add_dimension(
                Constraint::Distance {
                    a,
                    b,
                    value: length,
                },
                None,
            )
            .unwrap();
        (ed, d, a, b)
    }

    #[test]
    fn variables_drive_linked_dimensions() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        assert_eq!(
            ed.doc.variable(w).unwrap().expr,
            "25 mm",
            "unit made explicit"
        );
        ed.set_dimension_text(d, "w * 2").unwrap();
        assert_eq!(ed.doc.expression(d), Some("w * 2"));
        assert!((length(&ed, a, b) - 50.0).abs() < 1e-9);
        ed.set_variable(w, "30mm").unwrap();
        assert!((length(&ed, a, b) - 60.0).abs() < 1e-9);
        assert_eq!(ed.doc.constraint(d).unwrap().value(), Some(60.0));
        assert_eq!(ed.dimension_edit_text(d).as_deref(), Some("w * 2"));
        // Each edit is one undo step.
        ed.undo();
        assert!((length(&ed, a, b) - 50.0).abs() < 1e-9);
        // A constant unlinks it.
        ed.set_dimension_text(d, "12").unwrap();
        assert_eq!(ed.doc.expression(d), None);
        assert!((length(&ed, a, b) - 12.0).abs() < 1e-9);
        // Now the variable can be deleted.
        ed.remove_variable(w).unwrap();
    }

    #[test]
    fn bad_parameter_edits_are_refused_and_rolled_back() {
        let (mut ed, d, ..) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        let before = ed.doc.clone();
        let err = |r: Result<(), EditError>| match r {
            Err(EditError::Invalid(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(err(ed.set_variable(w, "0")).contains("isn't a valid value"));
        assert!(err(ed.set_variable(w, "5deg")).contains("expected a length"));
        assert!(err(ed.set_variable(w, "w + 1mm")).contains("itself"));
        let v = ed.add_variable("v", "w", Kind::Length).unwrap();
        assert!(err(ed.set_variable(w, "v")).contains("circular"));
        assert!(matches!(ed.remove_variable(w), Err(EditError::Invalid(_))));
        assert!(err(ed.set_dimension_text(d, "nope")).contains("unknown variable"));
        assert!(
            ed.add_variable("w", "1", Kind::Length).is_err(),
            "duplicate name"
        );
        ed.remove_variable(v).unwrap();
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn sliders_scrub_in_one_undo_step() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        assert!(ed.is_scrubbable(w));
        for value in [26.0, 30.0, 41.5] {
            assert!(ed.scrub_variable(w, value));
            assert!((length(&ed, a, b) - value).abs() < 1e-9);
        }
        // An impossible value is skipped, keeping the last good one.
        assert!(!ed.scrub_variable(w, 0.0));
        assert!((length(&ed, a, b) - 41.5).abs() < 1e-9);
        ed.end_scrub();
        assert_eq!(ed.doc.variable(w).unwrap().expr, "41.5 mm");
        ed.undo();
        assert_eq!(ed.doc.variable(w).unwrap().expr, "25 mm");
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn variants_store_and_apply_variable_sets() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        let small = ed.save_variant("Small");
        assert_eq!(ed.active_variant(), Some(small));
        ed.set_variable(w, "80").unwrap();
        assert_eq!(ed.active_variant(), None);
        let large = ed.save_variant("Large");
        ed.apply_variant(small).unwrap();
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
        assert_eq!(ed.active_variant(), Some(small));
        // Ghost geometry of the other variant.
        let ghost = ed.variant_geometry(large).unwrap();
        assert!(ghost.iter().any(|item| matches!(item.shape,
            Shape::Line { a, b } if (a.distance(b) - 80.0).abs() < 1e-9)));
        ed.undo();
        assert!((length(&ed, a, b) - 80.0).abs() < 1e-9);
    }

    #[test]
    fn snapshots_restore_as_an_undoable_step() {
        let (mut ed, d, a, b) = dimensioned_line();
        let original = length(&ed, a, b);
        ed.take_snapshot("Before");
        ed.set_dimension(d, 40.0).unwrap();
        ed.add_variable("w", "1", Kind::Length).unwrap();
        ed.restore_snapshot(0);
        assert_eq!(length(&ed, a, b), original);
        assert_eq!(ed.doc.variables().count(), 0);
        ed.undo();
        assert!((length(&ed, a, b) - 40.0).abs() < 1e-9);
        assert_eq!(ed.doc.variables().count(), 1);
    }

    #[test]
    fn units_change_display_only() {
        let (mut ed, d, a, b) = dimensioned_line();
        ed.set_dimension(d, 25.4 * 18.5).unwrap();
        let before = length(&ed, a, b);
        ed.set_units(Units::architectural());
        assert_eq!(length(&ed, a, b), before);
        assert_eq!(ed.dimension_edit_text(d).as_deref(), Some("1'-6 1/2\""));
        // Bare numbers are now inches.
        ed.set_dimension_text(d, "12").unwrap();
        assert!((length(&ed, a, b) - 12.0 * 25.4).abs() < 1e-9);
        ed.set_dimension_text(d, "2'-0\"").unwrap();
        assert!((length(&ed, a, b) - 24.0 * 25.4).abs() < 1e-9);
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.units(), Units::default());
    }

    #[test]
    fn pinned_measurements_follow_geometry() {
        let (mut ed, d, a, b) = dimensioned_line();
        let m = ed
            .pin_measurement(Constraint::Distance { a, b, value: 0.0 })
            .unwrap();
        ed.set_dimension(d, 33.0).unwrap();
        let (_, c) = ed.doc.measurements().find(|(id, _)| *id == m).unwrap();
        assert!((drift_solver::measure(&ed.doc, c).unwrap() - 33.0).abs() < 1e-9);
        // Deleting the geometry removes the measurement.
        ed.selection = ed.doc.iter().map(|(id, _)| id).collect();
        ed.delete_selection();
        assert_eq!(ed.doc.measurements().count(), 0);
        ed.undo();
        assert_eq!(ed.doc.measurements().count(), 1);
    }
}

#[cfg(test)]
mod collinear_tests {
    use super::*;

    /// Line (0,0)–(10,0) and a second line from `c` (a new point, or the first line's end
    /// when `connected`) to (20, 4). Returns the editor, both lines and all end points.
    fn two_lines(connected: bool) -> (Editor, [EntityId; 2], [EntityId; 4]) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(10.0, 0.0));
        let l1 = ed.doc.add_line(a, b).unwrap();
        let c = if connected {
            b
        } else {
            ed.doc.add_point(DVec2::new(12.0, 1.0))
        };
        let d = ed.doc.add_point(DVec2::new(20.0, 4.0));
        let l2 = ed.doc.add_line(c, d).unwrap();
        ed.commit("Lines");
        (ed, [l1, l2], [a, b, c, d])
    }

    fn assert_collinear(ed: &Editor, pts: [EntityId; 4]) {
        let p = pts.map(|id| ed.doc.point(id).unwrap());
        let u = (p[1] - p[0]).normalize();
        for q in &p[2..] {
            assert!((*q - p[0]).perp_dot(u).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn collinear_works_for_separate_and_connected_lines() {
        for connected in [false, true] {
            let (mut ed, [l1, l2], pts) = two_lines(connected);
            let dof = ed.analysis().dof.unwrap();
            ed.add_constraints("Collinear", &[Constraint::Collinear { a: l1, b: l2 }])
                .unwrap_or_else(|e| panic!("connected = {connected}: {e}"));
            assert_collinear(&ed, pts);
            // Separate lines lose two freedoms (direction and offset); connected ones one.
            let lost = if connected { 1 } else { 2 };
            assert_eq!(
                ed.analysis().dof,
                Some(dof - lost),
                "connected = {connected}"
            );
            // Adding it again is redundant.
            assert_eq!(
                ed.add_constraints("Collinear", &[Constraint::Collinear { a: l2, b: l1 }]),
                Err(EditError::Redundant)
            );
        }
    }

    #[test]
    fn collinear_works_for_lines_joined_by_a_coincident_relation() {
        let (mut ed, [l1, l2], [a, b, c, d]) = two_lines(false);
        ed.add_constraints("Coincident", &[Constraint::Coincident { a: b, b: c }])
            .unwrap();
        ed.add_constraints("Collinear", &[Constraint::Collinear { a: l1, b: l2 }])
            .unwrap();
        assert_collinear(&ed, [a, b, c, d]);
    }
}

/// A unit rotor for `degrees`, exact for multiples of 90°.
fn exact_rotor(degrees: f64) -> DVec2 {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() <= 1e-12 {
        match (quarter.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::from_angle(degrees.to_radians())
    }
}

/// Whether a turn is a whole number of quarter turns.
pub fn is_quarter_turn(degrees: f64) -> bool {
    let q = degrees / 90.0;
    (q - q.round()).abs() <= 1e-9
}
