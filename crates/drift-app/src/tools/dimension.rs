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

#[derive(Default)]
pub struct DimensionTool {
    /// The first entity (or midpoint) clicked, waiting for a possible second.
    pub(super) first: Option<Target>,
    /// The dimension being placed.
    pub(super) base: Option<DimBase>,
}

impl DimensionTool {
    /// The base implied by clicking one thing alone (a midpoint needs a second pick).
    pub(super) fn single(ed: &Editor, t: Target) -> Option<DimBase> {
        let Target::Entity(e) = t else { return None };
        match ed.doc.get(e)?.kind {
            EntityKind::Line { a, b } => Some(DimBase::Points(a, b)),
            EntityKind::Circle { .. } => Some(DimBase::Circular(e, true)),
            EntityKind::Arc { .. } => Some(DimBase::Circular(e, false)),
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
            Ok(id) => ed.pending_edit = Some(id),
            Err(e) => ed.notice = Some(format!("{}: {e}", c.type_name())),
        }
    }

    pub(super) fn reset(&mut self, ed: &mut Editor) {
        self.first = None;
        self.base = None;
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
            Input::Click(p, _) => {
                let picked = pick_target(ed, &p);
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
