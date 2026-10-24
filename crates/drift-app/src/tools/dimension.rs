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

/// What a row of dimensions goes from and to: a point (or a line's midpoint), or a line
/// (measured square to it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Link {
    Point(EntityId),
    Line(EntityId),
}

/// How a row's dimensions run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Way {
    /// Horizontal (or vertical) distances between points; `line` is the label line's
    /// coordinate across them (y for horizontal ones).
    Axis { horizontal: bool, line: f64 },
    /// Distances square to parallel lines (walls), `dir` along them; the labels stand
    /// abreast of `at`.
    Square { dir: DVec2, at: DVec2 },
}

/// A row of dimensions continuing from the last one placed: chained end to end, or all
/// from the same base (baseline), each a step further out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Run {
    base: Link,
    last: Link,
    way: Way,
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

/// How far apart two links of a row are: between points, or from a point to a line.
fn link_distance(ed: &Editor, a: Link, b: Link) -> Option<f64> {
    let at = |l: Link| match l {
        Link::Point(p) => ed.doc.ref_point(p),
        Link::Line(l) => line_start(ed, l).and_then(|p| ed.doc.ref_point(p)),
    };
    match (a, b) {
        (Link::Line(l), other) | (other, Link::Line(l)) => {
            let p = at(other)?;
            Some(p.distance(crate::annotations::foot(&ed.doc, p, l)?))
        }
        (Link::Point(_), Link::Point(_)) => Some(at(a)?.distance(at(b)?)),
    }
}

fn is_line(ed: &Editor, id: EntityId) -> bool {
    matches!(
        ed.doc.get(id).map(|e| e.kind),
        Some(EntityKind::Line { .. })
    )
}

/// A line's direction (unit).
fn line_dir(ed: &Editor, l: EntityId) -> Option<DVec2> {
    match ed.doc.shape(l)? {
        Shape::Line { a, b } => (b - a).try_normalize(),
        _ => None,
    }
}

/// The point a line starts at.
fn line_start(ed: &Editor, l: EntityId) -> Option<EntityId> {
    match ed.doc.get(l)?.kind {
        EntityKind::Line { a, .. } => Some(a),
        _ => None,
    }
}

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
                // Horizontal and vertical dimensions, and those square to a line (between
                // walls), can continue as a chain or baseline row.
                let baseline = ed.dim_mode == crate::editor::DimMode::Baseline;
                let run = |base, last, way| Run {
                    base,
                    last,
                    way,
                    baseline,
                    count: 1,
                };
                self.run = match c {
                    Constraint::HorizontalDistance { a, b, .. } => Some(run(
                        Link::Point(a),
                        Link::Point(b),
                        Way::Axis {
                            horizontal: true,
                            line: at.y,
                        },
                    )),
                    Constraint::VerticalDistance { a, b, .. } => Some(run(
                        Link::Point(a),
                        Link::Point(b),
                        Way::Axis {
                            horizontal: false,
                            line: at.x,
                        },
                    )),
                    Constraint::PointLineDistance { point, line, .. } => {
                        line_dir(ed, line).map(|dir| {
                            // Between two walls: the row goes from the first wall (the line
                            // the point is the end of, alongside the second).
                            let from = if is_line(ed, point) && alongside(ed, point, line) {
                                // A wall picked at its midpoint is the wall.
                                Link::Line(point)
                            } else {
                                ed.doc
                                    .users_of(point)
                                    .find(|l| {
                                        *l != line && is_line(ed, *l) && alongside(ed, *l, line)
                                    })
                                    .map_or(Link::Point(point), Link::Line)
                            };
                            run(from, Link::Line(line), Way::Square { dir, at })
                        })
                    }
                    _ => None,
                };
            }
            Err(e) => ed.notice = Some(format!("{}: {e}", c.type_name())),
        }
    }

    /// The dimension of a row from `from` to `to`, and where its label goes: `out` steps
    /// further from the geometry (a baseline row's later ones, or an overall).
    fn link_dimension(
        ed: &Editor,
        run: &Run,
        from: Link,
        to: Link,
        out: f64,
    ) -> Option<(Constraint, DVec2)> {
        if from == to {
            return None;
        }
        let (c, at) = match run.way {
            Way::Axis { horizontal, line } => {
                let (Link::Point(f), Link::Point(q)) = (from, to) else {
                    return None;
                };
                let (pf, pq) = (ed.doc.ref_point(f)?, ed.doc.ref_point(q)?);
                let base = match run.base {
                    Link::Point(b) => ed.doc.ref_point(b)?,
                    Link::Line(_) => pf,
                };
                // Rows step away from the geometry.
                let across = if horizontal { base.y } else { base.x };
                let away = if line >= across { 1.0 } else { -1.0 };
                let line = line + away * out;
                let mid = (pf + pq) * 0.5;
                if horizontal {
                    (
                        Constraint::HorizontalDistance {
                            a: f,
                            b: q,
                            value: 0.0,
                        },
                        DVec2::new(mid.x, line),
                    )
                } else {
                    (
                        Constraint::VerticalDistance {
                            a: f,
                            b: q,
                            value: 0.0,
                        },
                        DVec2::new(line, mid.y),
                    )
                }
            }
            Way::Square { dir, at } => {
                // Square to the lines: a point (or a line's start) to a line.
                let (point, line) = match (from, to) {
                    (Link::Line(lf), Link::Line(lq)) if alongside(ed, lf, lq) => {
                        (line_start(ed, lf)?, lq)
                    }
                    (Link::Line(lf), Link::Point(q)) => (q, lf),
                    (Link::Point(f), Link::Line(lq)) => (f, lq),
                    _ => return None,
                };
                let p = ed.doc.ref_point(point)?;
                let foot = crate::annotations::foot(&ed.doc, p, line)?;
                let mid = (p + foot) * 0.5;
                // Abreast of where the row was placed, further along the lines for `out`.
                let along = (at - mid).dot(dir);
                let away = if along >= 0.0 { 1.0 } else { -1.0 };
                (
                    Constraint::PointLineDistance {
                        point,
                        line,
                        value: 0.0,
                    },
                    mid + dir * (along + away * out),
                )
            }
        };
        let c = c.with_value(drift_solver::measure(&ed.doc, &c)?);
        Some((c, at))
    }

    /// The next dimension of a run, to `q`, and where its label goes, and whether it goes
    /// on from the row's first end (a chain goes on from whichever end `q` is nearer).
    fn run_dimension(
        ed: &Editor,
        run: &Run,
        q: Link,
        step: f64,
    ) -> Option<(Constraint, DVec2, bool)> {
        if run.baseline {
            let (c, at) = Self::link_dimension(ed, run, run.base, q, step * f64::from(run.count))?;
            return Some((c, at, false));
        }
        let near = |l: Link| link_distance(ed, l, q).unwrap_or(f64::INFINITY);
        let from_base = run.count > 0 && near(run.base) < near(run.last);
        let from = if from_base { run.base } else { run.last };
        let (c, at) = Self::link_dimension(ed, run, from, q, 0.0)?;
        Some((c, at, from_base))
    }

    /// Continue the run to `q`. Dimensions that would over-define the sketch are added as
    /// driven (reference) dimensions.
    fn extend_run(&mut self, ed: &mut Editor, q: Link, step: f64) -> bool {
        let Some(mut run) = self.run else {
            return false;
        };
        let Some((c, at, from_base)) = Self::run_dimension(ed, &run, q, step) else {
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
        if from_base {
            run.base = q;
        } else {
            run.last = q;
        }
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
        let Some((c, at)) = Self::link_dimension(ed, &run, run.base, run.last, step) else {
            return;
        };
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c)
            .map(|anchor| crate::annotations::centred_offset(&ed.doc, &c, at - anchor));
        if let Err(e) = ed.add_driven_dimension(c, offset) {
            ed.notice = Some(format!("Overall: {e}"));
        }
    }

    /// What a pick adds to a row: for a row between points, a point or midpoint, or a line's
    /// end nearer `at`; for a row square to lines, a line alongside them, or a point.
    fn run_link(ed: &Editor, run: &Run, t: Target, at: DVec2) -> Option<Link> {
        match run.way {
            Way::Axis { .. } => Self::run_point(ed, t, at).map(Link::Point),
            Way::Square { .. } => match t {
                Target::Entity(id) | Target::Midpoint(id) if is_line(ed, id) => {
                    let (Link::Line(last) | Link::Point(last)) = run.last;
                    let parallel = match run.last {
                        Link::Line(l) => alongside(ed, l, id),
                        Link::Point(_) => match run.base {
                            Link::Line(b) => alongside(ed, b, id),
                            Link::Point(_) => true,
                        },
                    };
                    (parallel && id != last).then_some(Link::Line(id))
                }
                _ => t.point_ref(ed).map(Link::Point),
            },
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
                match pick_target_with(ed, &p, crate::editor::PickFilter::ALL) {
                    Some(Target::Entity(id)) => ed.hovered = Some(id),
                    Some(Target::Midpoint(line)) => ed.hovered_midpoint = Some(line),
                    None => {}
                }
            }
            Input::Click(p, mods) => {
                let picked = pick_target_with(ed, &p, crate::editor::PickFilter::ALL);
                let step = p.tolerance * BASELINE_STEP;
                self.step = step;
                // Continuing a chain / baseline row: in Chain or Baseline mode a click on a
                // point or line adds the next one (Shift+click, in Single mode). A click on
                // nothing ends the row.
                self.continuous = ed.dim_mode != crate::editor::DimMode::Single;
                if self.run.is_some() && self.first.is_none() && self.base.is_none() {
                    if (mods.shift || self.continuous)
                        && let Some(run) = self.run
                        && let Some(q) = picked.and_then(|t| Self::run_link(ed, &run, t, p.raw))
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
