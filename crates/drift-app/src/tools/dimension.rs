//! Smart Dimension: the dimension type follows the picks and the label position.

use super::select::{Target, pick_target};
use super::*;

/// What a dimension measures, before its exact type is chosen by where the label goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DimBase {
    /// Two points (or a line's ends): aligned, horizontal or vertical by label position.
    Points(EntityId, EntityId),
    /// A circle (diameter) or an arc (radius).
    Circular(EntityId, bool),
    Angle(EntityId, EntityId),
    PointLine(EntityId, EntityId),
}

/// A row of horizontal or vertical dimensions continuing from the last one placed: chained
/// end to end, or all from the same base point (baseline), each a step further out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Run {
    base: EntityId,
    last: EntityId,
    horizontal: bool,
    /// The label line's coordinate across the dimensions (y for horizontal ones).
    line: f64,
    baseline: bool,
    count: u32,
}

/// Baseline rows step out by this many pick radii.
const BASELINE_STEP: f64 = 3.5;

#[derive(Default)]
pub struct DimensionTool {
    /// The first entity (or midpoint) clicked, waiting for a possible second.
    pub(super) first: Option<Target>,
    /// The dimension being placed.
    pub(super) base: Option<DimBase>,
    /// Chain / baseline dimensioning after a horizontal or vertical dimension.
    pub(super) run: Option<Run>,
}

impl DimensionTool {
    /// The base implied by clicking one thing alone (a midpoint needs a second pick).
    pub(super) fn single(ed: &Editor, t: Target) -> Option<DimBase> {
        let Target::Entity(e) = t else { return None };
        match ed.doc.get(e)?.kind {
            EntityKind::Line { a, b } => Some(DimBase::Points(a, b)),
            EntityKind::Circle { .. } => Some(DimBase::Circular(e, true)),
            EntityKind::Arc { .. } => Some(DimBase::Circular(e, false)),
            // An ellipse: its first semi-axis (centre to major point).
            EntityKind::Ellipse { center, major, .. }
            | EntityKind::EllipseArc { center, major, .. } => Some(DimBase::Points(center, major)),
            EntityKind::Point(_) => None,
        }
    }

    /// The base implied by clicking `first` then `second`. Midpoints act as points.
    pub(super) fn pair(ed: &Editor, first: Target, second: Target) -> Option<DimBase> {
        let line_of = |t: Target| match t {
            Target::Entity(id) => match ed.doc.get(id)?.kind {
                EntityKind::Line { a, b } => Some((id, a, b)),
                _ => None,
            },
            Target::Midpoint(_) => None,
        };
        match (
            first.point_ref(ed),
            second.point_ref(ed),
            line_of(first),
            line_of(second),
        ) {
            (Some(p), Some(q), ..) if p != q => Some(DimBase::Points(p, q)),
            (.., Some((l1, ..)), Some((l2, ..))) => Some(DimBase::Angle(l1, l2)),
            (Some(p), None, None, Some((l, a, b))) | (None, Some(p), Some((l, a, b)), None)
                if p != l && p != a && p != b =>
            {
                Some(DimBase::PointLine(p, l))
            }
            _ => None,
        }
    }

    /// The dimension for `base` with its label at `at`, at the geometry's current value.
    pub(super) fn dimension(ed: &Editor, base: DimBase, at: DVec2) -> Option<Constraint> {
        let c = match base {
            DimBase::Points(a, b) => {
                let (pa, pb) = (ed.doc.ref_point(a)?, ed.doc.ref_point(b)?);
                let (lo, hi) = (pa.min(pb), pa.max(pb));
                let inside_x = at.x >= lo.x && at.x <= hi.x;
                let inside_y = at.y >= lo.y && at.y <= hi.y;
                if inside_x && !inside_y {
                    Constraint::HorizontalDistance { a, b, value: 0.0 }
                } else if inside_y && !inside_x {
                    Constraint::VerticalDistance { a, b, value: 0.0 }
                } else {
                    Constraint::Distance { a, b, value: 0.0 }
                }
            }
            DimBase::Circular(curve, true) => Constraint::Diameter { curve, value: 0.0 },
            DimBase::Circular(curve, false) => Constraint::Radius { curve, value: 0.0 },
            DimBase::Angle(a, b) => Constraint::Angle { a, b, degrees: 0.0 },
            DimBase::PointLine(point, line) => Constraint::PointLineDistance {
                point,
                line,
                value: 0.0,
            },
        };
        let value = drift_solver::measure(&ed.doc, &c)?;
        Some(c.with_value(value))
    }

    pub(super) fn place(&mut self, ed: &mut Editor, base: DimBase, at: DVec2) {
        self.reset(ed);
        let Some(c) = Self::dimension(ed, base, at) else {
            return;
        };
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c).map(|anchor| at - anchor);
        match ed.add_dimension(c, offset) {
            Ok(id) => {
                ed.pending_edit = Some(id);
                // Horizontal and vertical dimensions can continue as a chain or baseline row.
                self.run = match c {
                    Constraint::HorizontalDistance { a, b, .. } => Some(Run {
                        base: a,
                        last: b,
                        horizontal: true,
                        line: at.y,
                        baseline: false,
                        count: 1,
                    }),
                    Constraint::VerticalDistance { a, b, .. } => Some(Run {
                        base: a,
                        last: b,
                        horizontal: false,
                        line: at.x,
                        baseline: false,
                        count: 1,
                    }),
                    _ => None,
                };
            }
            Err(e) => ed.notice = Some(format!("{}: {e}", c.type_name())),
        }
    }

    /// The next dimension of a run, to point `q`, and where its label goes.
    fn run_dimension(
        ed: &Editor,
        run: &Run,
        q: EntityId,
        step: f64,
    ) -> Option<(Constraint, DVec2)> {
        let from = if run.baseline { run.base } else { run.last };
        if from == q {
            return None;
        }
        let (pf, pq) = (ed.doc.ref_point(from)?, ed.doc.ref_point(q)?);
        let pb = ed.doc.ref_point(run.base)?;
        // Baseline rows step away from the geometry.
        let across = if run.horizontal { pb.y } else { pb.x };
        let away = if run.line >= across { 1.0 } else { -1.0 };
        let line = if run.baseline {
            run.line + away * step * f64::from(run.count)
        } else {
            run.line
        };
        let mid = (pf + pq) * 0.5;
        let (c, at) = if run.horizontal {
            (
                Constraint::HorizontalDistance {
                    a: from,
                    b: q,
                    value: 0.0,
                },
                DVec2::new(mid.x, line),
            )
        } else {
            (
                Constraint::VerticalDistance {
                    a: from,
                    b: q,
                    value: 0.0,
                },
                DVec2::new(line, mid.y),
            )
        };
        let c = c.with_value(drift_solver::measure(&ed.doc, &c)?);
        Some((c, at))
    }

    /// Continue the run to point `q`. Dimensions that would over-define the sketch are added
    /// as driven (reference) dimensions.
    fn extend_run(&mut self, ed: &mut Editor, q: EntityId, step: f64) -> bool {
        let Some(mut run) = self.run else {
            return false;
        };
        let Some((c, at)) = Self::run_dimension(ed, &run, q, step) else {
            return false;
        };
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c).map(|anchor| at - anchor);
        let added = match ed.add_dimension(c, offset) {
            Ok(id) => Some(id),
            Err(crate::editor::EditError::Redundant | crate::editor::EditError::Conflict) => {
                ed.pending_driven = None;
                ed.add_driven_dimension(c, offset).ok()
            }
            Err(e) => {
                ed.notice = Some(format!("{}: {e}", c.type_name()));
                None
            }
        };
        if added.is_none() {
            return false;
        }
        run.last = q;
        run.count += 1;
        self.run = Some(run);
        true
    }

    pub(super) fn reset(&mut self, ed: &mut Editor) {
        self.first = None;
        self.base = None;
        self.run = None;
        ed.highlight.clear();
        ed.selected_midpoints.clear();
    }

    /// Show what has been picked: entities highlighted, midpoints marked.
    pub(super) fn show_pick(ed: &mut Editor, t: Target, first: bool) {
        if first {
            ed.highlight.clear();
            ed.selected_midpoints.clear();
        }
        match t {
            Target::Entity(id) => {
                ed.highlight.insert(id);
            }
            Target::Midpoint(line) => {
                ed.selected_midpoints.insert(line);
            }
        }
    }
}

impl Tool for DimensionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Dimension
    }

    fn is_busy(&self) -> bool {
        self.first.is_some() || self.base.is_some()
    }

    fn hint(&self) -> String {
        if let Some(run) = self.run
            && self.first.is_none()
            && self.base.is_none()
        {
            return if run.baseline {
                "Shift+click points for baseline dimensions from the same base (B: chain)".into()
            } else {
                "Shift+click points to chain dimensions along the row (B: baseline)".into()
            };
        }
        match (self.first, self.base) {
            (None, None) => {
                "Click a line, circle or arc, or two points / lines / a point and a line".into()
            }
            (Some(_), None) => "Click a second point or line".into(),
            _ => "Click to place the dimension, or click another entity to change it".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                ed.hovered = None;
                ed.hovered_midpoint = None;
                match pick_target(ed, &p) {
                    Some(Target::Entity(id)) => ed.hovered = Some(id),
                    Some(Target::Midpoint(line)) => ed.hovered_midpoint = Some(line),
                    None => {}
                }
            }
            Input::Click(p, mods) => {
                let picked = pick_target(ed, &p);
                // Continuing a chain / baseline row: Shift+click on a point adds the next one.
                if self.run.is_some() && self.first.is_none() && self.base.is_none() {
                    if mods.shift
                        && let Some(q) = picked.and_then(|t| t.point_ref(ed))
                        && self.extend_run(ed, q, p.tolerance * BASELINE_STEP)
                    {
                        return;
                    }
                    self.run = None;
                }
                // A second pick combines with the first; otherwise a click places.
                if let (Some(first), Some(t)) = (self.first, picked)
                    && t != first
                    && let Some(base) = Self::pair(ed, first, t)
                {
                    self.base = Some(base);
                    self.first = None;
                    Self::show_pick(ed, t, false);
                    return;
                }
                if let Some(base) = self.base {
                    self.place(ed, base, p.raw);
                    return;
                }
                if let Some(t) = picked {
                    self.first = Some(t);
                    self.base = Self::single(ed, t);
                    Self::show_pick(ed, t, true);
                }
            }
            Input::Finish => self.reset(ed),
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        let had = self.is_busy();
        self.reset(ed);
        had
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match (&mut self.run, key) {
            (Some(run), ToolKey::B) => {
                run.baseline = !run.baseline;
                true
            }
            _ => false,
        }
    }

    fn pending_dimension(
        &self,
        ed: &Editor,
        pointer: Option<&Pointer>,
    ) -> Option<(Constraint, DVec2)> {
        let at = pointer?.raw;
        let c = Self::dimension(ed, self.base?, at)?;
        let anchor = crate::annotations::dimension_anchor(&ed.doc, &c)?;
        Some((c, at - anchor))
    }
}
