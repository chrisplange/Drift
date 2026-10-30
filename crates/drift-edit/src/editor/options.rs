//! The relation and dimension options the selection fits.

use super::*;

/// A relation or dimension that can be applied to the current selection.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintOption {
    pub label: &'static str,
    pub constraints: Vec<Constraint>,
}

impl Editor {
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
        if let (points, [curve], []) | (points, [], [curve]) = (
            sel.points.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
        ) && !points.is_empty()
        {
            out.push(opt(
                "On curve",
                points
                    .iter()
                    .map(|point| Constraint::PointOnCurve {
                        point: *point,
                        curve: *curve,
                    })
                    .collect(),
            ));
        }
        match (
            sel.points.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
        ) {
            (points, [], []) if points.len() >= 2 => {
                out.push(opt(
                    "Coincident",
                    self.joining(points, Self::coincident_pair, |a, b| {
                        Constraint::Coincident { a, b }
                    }),
                ));
                out.push(opt(
                    "Horizontal",
                    self.joining(points, Self::horizontal_pair, |a, b| {
                        Constraint::HorizontalPoints { a, b }
                    }),
                ));
                out.push(opt(
                    "Vertical",
                    self.joining(points, Self::vertical_pair, |a, b| {
                        Constraint::VerticalPoints { a, b }
                    }),
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
                if lines.len() >= 2 {
                    out.push(opt(
                        "Parallel",
                        self.joining(lines, Self::parallel_pair, |a, b| Constraint::Parallel {
                            a,
                            b,
                        }),
                    ));
                }
                if let [a, b] = lines {
                    out.push(opt(
                        "Perpendicular",
                        vec![Constraint::Perpendicular { a: *a, b: *b }],
                    ));
                }
                if lines.len() >= 2 {
                    out.push(opt(
                        "Collinear",
                        self.joining(lines, Self::collinear_pair, |a, b| Constraint::Collinear {
                            a,
                            b,
                        }),
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
                }
                out.push(opt(
                    "Concentric",
                    self.joining(circulars, Self::concentric_pair, |a, b| {
                        Constraint::Concentric { a, b }
                    }),
                ));
                out.push(opt(
                    "Coradial",
                    self.joining(circulars, Self::coradial_pair, |a, b| {
                        Constraint::Coradial { a, b }
                    }),
                ));
                out.push(opt(
                    "Equal",
                    circulars
                        .windows(2)
                        .map(|w| Constraint::Equal { a: w[0], b: w[1] })
                        .collect(),
                ));
            }
            // Lines tangent to one circle, or circles tangent to one line.
            ([], lines, [curve]) if !lines.is_empty() => {
                out.push(opt(
                    "Tangent",
                    lines
                        .iter()
                        .map(|line| Constraint::Tangent {
                            a: *line,
                            b: *curve,
                        })
                        .collect(),
                ));
            }
            ([], [line], curves) if !curves.is_empty() => {
                out.push(opt(
                    "Tangent",
                    curves
                        .iter()
                        .map(|curve| Constraint::Tangent {
                            a: *line,
                            b: *curve,
                        })
                        .collect(),
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
        // A line or arc tangent to an elliptical arc or Bézier, at the end the two share.
        if let ([], [curve], [other], []) | ([], [curve], [], [other]) = (
            sel.points.as_slice(),
            sel.open_curves.as_slice(),
            sel.lines.as_slice(),
            sel.circulars.as_slice(),
        ) {
            let tangent = Constraint::Tangent {
                a: *other,
                b: *curve,
            };
            if self.doc.validate(&tangent).is_ok() {
                out.push(opt("Tangent", vec![tangent]));
            }
        }
        // A relation that already holds across the whole selection isn't offered again.
        out.retain(|o| !o.constraints.is_empty());
        if !self.selection.is_empty() {
            let fixes = self.fix_constraints();
            if !fixes.is_empty() {
                out.push(opt("Fix", fixes));
            }
        }
        out
    }

    /// Relations of one kind that tie all of `items` together: those of the kind already
    /// among them (so the option shows as on, and can be taken off), then one to the first for
    /// each that isn't tied in yet. `related` says which pairs a relation already holds
    /// together this way, so nothing is said twice.
    pub(super) fn joining(
        &self,
        items: &[EntityId],
        related: fn(&Constraint) -> Option<(EntityId, EntityId)>,
        make: impl Fn(EntityId, EntityId) -> Constraint,
    ) -> Vec<Constraint> {
        // Which group each item is in, merged along the relations already there.
        let mut group: std::collections::HashMap<EntityId, usize> =
            items.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        fn merge(group: &mut std::collections::HashMap<EntityId, usize>, a: usize, b: usize) {
            for g in group.values_mut() {
                if *g == b {
                    *g = a;
                }
            }
        }
        let kind = std::mem::discriminant(&make(items[0], items[1]));
        let mut out = Vec::new();
        for (_, c) in self.doc.constraints() {
            if let Some((a, b)) = related(c)
                && let (Some(&ga), Some(&gb)) = (group.get(&a), group.get(&b))
            {
                if std::mem::discriminant(c) == kind {
                    out.push(*c);
                }
                if ga != gb {
                    merge(&mut group, ga, gb);
                }
            }
        }
        let first = items[0];
        for id in &items[1..] {
            let (g0, g) = (group[&first], group[id]);
            if g0 != g {
                out.push(make(first, *id));
                merge(&mut group, g0, g);
            }
        }
        out
    }

    pub(super) fn coincident_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::Coincident { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn horizontal_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::HorizontalPoints { a, b } | Constraint::Coincident { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn vertical_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::VerticalPoints { a, b } | Constraint::Coincident { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn parallel_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::Parallel { a, b } | Constraint::Collinear { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn collinear_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::Collinear { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn concentric_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::Concentric { a, b } | Constraint::Coradial { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub(super) fn coradial_pair(c: &Constraint) -> Option<(EntityId, EntityId)> {
        match *c {
            Constraint::Coradial { a, b } => Some((a, b)),
            _ => None,
        }
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
    pub(super) fn fix_constraints(&self) -> Vec<Constraint> {
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

    pub(super) fn classify_selection(&self) -> SelectionKinds {
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
                Some(EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. }) => {
                    kinds.open_curves.push(*id)
                }
                // No relation buttons for ellipses yet.
                Some(EntityKind::Ellipse { .. }) | None => {}
            }
        }
        kinds
    }
}

#[derive(Default)]
pub(super) struct SelectionKinds {
    midpoints: Vec<EntityId>,
    points: Vec<EntityId>,
    lines: Vec<EntityId>,
    circulars: Vec<EntityId>,
    /// Elliptical arcs and Béziers (only tangency at an end so far).
    open_curves: Vec<EntityId>,
}
