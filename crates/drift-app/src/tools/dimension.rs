//! Smart Dimension: the dimension type follows the picks and the label position.

use super::select::{Target, pick_target_with};
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
    /// The row goes on with plain clicks (Chain or Baseline mode).
    continuous: bool,
    /// The last baseline step (for an overall placed when the row ends by a key).
    step: f64,
}

/// Lines within this of parallel are dimensioned by the distance between them: lines drawn
/// by eye as parallel, without a relation, are rarely exact.
const PARALLEL_DEGREES: f64 = 1.0;

/// Whether two lines are parallel, or nearly.
fn alongside(ed: &Editor, l1: EntityId, l2: EntityId) -> bool {
    let dir = |l| match ed.doc.shape(l) {
        Some(Shape::Line { a, b }) => (b - a).try_normalize(),
        _ => None,
    };
    match (dir(l1), dir(l2)) {
        (Some(u), Some(v)) => u.perp_dot(v).abs() <= PARALLEL_DEGREES.to_radians().sin(),
        _ => false,
    }
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
            // A curve: the distance between its ends.
            EntityKind::Bezier { a, b, .. } => Some(DimBase::Points(a, b)),
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
            // Two lines: the angle between them, or, if they run alongside each other, the
            // distance between them (from the first line's start to the second line).
            (.., Some((l1, a1, _)), Some((l2, ..))) if l1 != l2 => {
                if alongside(ed, l1, l2) {
                    Some(DimBase::PointLine(a1, l2))
                } else {
                    Some(DimBase::Angle(l1, l2))
                }
            }
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
            DimBase::Angle(a, b) => {
                let (a, b) = crate::annotations::corner_order(&ed.doc, a, b);
                Constraint::Angle { a, b, degrees: 0.0 }
            }
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
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c)
            .map(|anchor| crate::annotations::centred_offset(&ed.doc, &c, at - anchor));
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
                        baseline: ed.dim_mode == crate::editor::DimMode::Baseline,
                        count: 1,
                    }),
                    Constraint::VerticalDistance { a, b, .. } => Some(Run {
                        base: a,
                        last: b,
                        horizontal: false,
                        line: at.x,
                        baseline: ed.dim_mode == crate::editor::DimMode::Baseline,
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
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c)
            .map(|anchor| crate::annotations::centred_offset(&ed.doc, &c, at - anchor));
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

    /// End a chain: with Overall on and two or more dimensions in it, an overall dimension
    /// (a reference, since the chain already holds it) a step outside the row.
    fn end_run(&mut self, ed: &mut Editor, step: f64) {
        let Some(run) = self.run.take() else {
            return;
        };
        if !ed.dim_overall
            || ed.dim_mode != crate::editor::DimMode::Chain
            || run.baseline
            || run.count < 2
        {
            return;
        }
        let (Some(pb), Some(pl)) = (ed.doc.ref_point(run.base), ed.doc.ref_point(run.last)) else {
            return;
        };
        let across = if run.horizontal { pb.y } else { pb.x };
        let away = if run.line >= across { 1.0 } else { -1.0 };
        let line = run.line + away * step;
        let mid = (pb + pl) * 0.5;
        let (c, at) = if run.horizontal {
            (
                Constraint::HorizontalDistance {
                    a: run.base,
                    b: run.last,
                    value: 0.0,
                },
                DVec2::new(mid.x, line),
            )
        } else {
            (
                Constraint::VerticalDistance {
                    a: run.base,
                    b: run.last,
                    value: 0.0,
                },
                DVec2::new(line, mid.y),
            )
        };
        let Some(value) = drift_solver::measure(&ed.doc, &c) else {
            return;
        };
        let c = c.with_value(value);
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c)
            .map(|anchor| crate::annotations::centred_offset(&ed.doc, &c, at - anchor));
        if let Err(e) = ed.add_driven_dimension(c, offset) {
            ed.notice = Some(format!("Overall: {e}"));
        }
    }

    /// The point a pick adds to a row: a point or midpoint, or a line's end nearer `at`.
    fn run_point(ed: &Editor, t: Target, at: DVec2) -> Option<EntityId> {
        if let Some(p) = t.point_ref(ed) {
            return Some(p);
        }
        let Target::Entity(id) = t else { return None };
        let EntityKind::Line { a, b } = ed.doc.get(id)?.kind else {
            return None;
        };
        let (pa, pb) = (ed.doc.point(a).ok()?, ed.doc.point(b).ok()?);
        Some(if pa.distance(at) <= pb.distance(at) {
            a
        } else {
            b
        })
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
            return match (self.continuous, run.baseline) {
                (true, true) => "Click points or lines for baseline dimensions from the same \
                                 base · B: chain · Enter, Esc or an empty click ends"
                    .into(),
                (true, false) => "Click points or lines to chain dimensions along the row · \
                                  B: baseline · Enter, Esc or an empty click ends"
                    .into(),
                (false, true) => {
                    "Shift+click points for baseline dimensions from the same base (B: chain)"
                        .into()
                }
                (false, false) => {
                    "Shift+click points to chain dimensions along the row (B: baseline)".into()
                }
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
                match pick_target_with(ed, &p, crate::editor::PickFilter::All) {
                    Some(Target::Entity(id)) => ed.hovered = Some(id),
                    Some(Target::Midpoint(line)) => ed.hovered_midpoint = Some(line),
                    None => {}
                }
            }
            Input::Click(p, mods) => {
                let picked = pick_target_with(ed, &p, crate::editor::PickFilter::All);
                let step = p.tolerance * BASELINE_STEP;
                self.step = step;
                // Continuing a chain / baseline row: in Chain or Baseline mode a click on a
                // point or line adds the next one (Shift+click, in Single mode). A click on
                // nothing ends the row.
                self.continuous = ed.dim_mode != crate::editor::DimMode::Single;
                if self.run.is_some() && self.first.is_none() && self.base.is_none() {
                    if (mods.shift || self.continuous)
                        && let Some(q) = picked.and_then(|t| Self::run_point(ed, t, p.raw))
                        && self.extend_run(ed, q, step)
                    {
                        return;
                    }
                    self.end_run(ed, step);
                    if self.continuous && picked.is_none() {
                        return;
                    }
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
            Input::Finish => {
                self.end_run(ed, self.step);
                self.reset(ed);
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        // Esc ends a row (with its overall) and keeps the tool.
        let had = self.is_busy() || self.run.is_some();
        self.end_run(ed, self.step);
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
        Some((
            c,
            crate::annotations::centred_offset(&ed.doc, &c, at - anchor),
        ))
    }
}
