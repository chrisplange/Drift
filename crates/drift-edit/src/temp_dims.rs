//! Temporary dimensions, as in Revit: with one curve picked, dimensions from it to its
//! neighbours show (a line's length and the nearest parallel line on each side; a circle's
//! diameter or an arc's radius). Typing a value moves the picked curve, the other end
//! staying put; one can be made permanent; and its far end can be moved to other geometry.

use drift_core::{Constraint, ConstraintId, DVec2, EntityId, EntityKind, LineKind, Shape};
use drift_solver::Param;

use crate::editor::{EditError, Editor};

/// What typing a value into a temporary dimension changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempMove {
    /// Shift `curve` along `dir` (unit): a larger value moves it that way.
    Shift { curve: EntityId, dir: DVec2 },
    /// Move `point` (a line's end) along `dir`.
    End { point: EntityId, dir: DVec2 },
    /// A circle's or an arc's size (the value is a diameter when `diameter`).
    Size { curve: EntityId, diameter: bool },
}

/// One temporary dimension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempDim {
    /// The dimension it stands for (its value is measured when it's shown).
    pub constraint: Constraint,
    pub moves: TempMove,
    /// The far end's geometry, which can be swapped for other geometry (`None`: it can't).
    pub reference: Option<EntityId>,
}

impl TempDim {
    /// The dimension at the geometry's current value.
    pub fn measured(&self, ed: &Editor) -> Option<Constraint> {
        let value = drift_solver::measure(&ed.doc, &self.constraint)?;
        Some(self.constraint.with_value(value))
    }

    /// Where its far end is (for its handle).
    pub fn reference_point(&self, ed: &Editor) -> Option<DVec2> {
        match self.constraint {
            Constraint::PointLineDistance { point, .. } => ed.doc.ref_point(point),
            _ => None,
        }
    }
}

/// Two unit directions are parallel (or opposite).
fn parallel(u: DVec2, v: DVec2) -> bool {
    u.perp_dot(v).abs() <= 1e-6
}

impl Editor {
    fn line_ends(&self, line: EntityId) -> Option<(EntityId, EntityId, DVec2, DVec2)> {
        let Some(EntityKind::Line { a, b }) = self.doc.get(line).map(|e| e.kind) else {
            return None;
        };
        Some((a, b, self.doc.point(a).ok()?, self.doc.point(b).ok()?))
    }

    /// The single curve temporary dimensions are about: one curve picked, nothing else.
    pub fn temp_dims_subject(&self) -> Option<EntityId> {
        if self.selection.len() != 1
            || !self.selected_instances.is_empty()
            || !self.selected_fills.is_empty()
            || self.picks_annotations()
            || self.selected_region.is_some()
            || !self.selected_midpoints.is_empty()
        {
            return None;
        }
        let id = *self.selection.first()?;
        self.doc.get(id).filter(|e| !e.kind.is_point())?;
        Some(id)
    }

    /// The temporary dimensions for what's picked (none unless it's one curve). Those a
    /// dimension already shows are left out.
    pub fn temp_dims(&self) -> Vec<TempDim> {
        let Some(id) = self.temp_dims_subject() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match self.doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => {
                if let Some((_, _, pa, pb)) = self.line_ends(id)
                    && let Some(u) = (pb - pa).try_normalize()
                {
                    out.push(TempDim {
                        constraint: Constraint::Distance { a, b, value: 0.0 },
                        moves: TempMove::End { point: b, dir: u },
                        reference: None,
                    });
                    for other in self.nearest_parallels(id).into_iter().flatten() {
                        out.extend(self.wall_dim(id, other));
                    }
                }
            }
            Some(EntityKind::Circle { .. }) => out.push(TempDim {
                constraint: Constraint::Diameter {
                    curve: id,
                    value: 0.0,
                },
                moves: TempMove::Size {
                    curve: id,
                    diameter: true,
                },
                reference: None,
            }),
            Some(EntityKind::Arc { .. }) => out.push(TempDim {
                constraint: Constraint::Radius {
                    curve: id,
                    value: 0.0,
                },
                moves: TempMove::Size {
                    curve: id,
                    diameter: false,
                },
                reference: None,
            }),
            _ => {}
        }
        out.retain(|t| !self.dimension_shown(&t.constraint));
        out
    }

    /// Whether a dimension already measures what `c` does (the same geometry).
    fn dimension_shown(&self, c: &Constraint) -> bool {
        let mut want: Vec<EntityId> = c.entities().collect();
        want.sort_unstable();
        self.doc.constraints().any(|(_, d)| {
            let mut has: Vec<EntityId> = d.entities().collect();
            has.sort_unstable();
            d.is_dimension()
                && std::mem::discriminant(d) == std::mem::discriminant(c)
                && has == want
        })
    }

    /// The nearest line parallel to `line` on each side of it (left of its direction, then
    /// right) that runs alongside it (overlapping it, measured along it). One pass over the
    /// drawing, looking at lines only (a big drawing has tens of thousands of entities).
    fn nearest_parallels(&self, line: EntityId) -> [Option<EntityId>; 2] {
        let Some((a, b, pa, pb)) = self.line_ends(line) else {
            return [None; 2];
        };
        let len = pa.distance(pb);
        let u = (pb - pa) / len;
        let n = u.perp();
        let tol = 1e-9 * (1.0 + len);
        let visible = |e: &drift_core::Entity| {
            e.style != LineKind::Construction && self.doc.layer(e.layer).is_none_or(|l| l.visible)
        };
        let mut best: [Option<(EntityId, f64)>; 2] = [None; 2];
        for (other, e) in self.doc.iter() {
            let EntityKind::Line { a: oa, b: ob } = e.kind else {
                continue;
            };
            if other == line || [oa, ob].iter().any(|p| *p == a || *p == b) {
                continue;
            }
            let (Ok(qa), Ok(qb)) = (self.doc.point(oa), self.doc.point(ob)) else {
                continue;
            };
            if !(qb - qa).try_normalize().is_some_and(|d| parallel(u, d)) {
                continue;
            }
            let s = (qa - pa).dot(n);
            let (ta, tb) = ((qa - pa).dot(u), (qb - pa).dot(u));
            if !(ta.max(tb) > tol && ta.min(tb) < len - tol) || s.abs() <= tol || !visible(e) {
                continue;
            }
            let k = usize::from(s < 0.0);
            // (Ties go to the first in id order, as `min_by` gave.)
            if best[k].is_none_or(|(_, d)| s.abs() < d) {
                best[k] = Some((other, s.abs()));
            }
        }
        best.map(|b| b.map(|(id, _)| id))
    }

    /// The distance from `line` to a parallel `other`: from `other`'s middle if that is
    /// alongside `line` (where the dimension then shows), else from its end nearer the middle
    /// of where they run alongside.
    fn wall_dim(&self, line: EntityId, other: EntityId) -> Option<TempDim> {
        let (_, _, pa, pb) = self.line_ends(line)?;
        let (oa, ob, qa, qb) = self.line_ends(other)?;
        let len = pa.distance(pb);
        let u = (pb - pa) / len;
        let (ta, tb) = ((qa - pa).dot(u), (qb - pa).dot(u));
        let middle = (ta.min(tb).max(0.0) + ta.max(tb).min(len)) * 0.5;
        let point = if (0.0..=len).contains(&((ta + tb) * 0.5)) {
            other
        } else if (ta - middle).abs() <= (tb - middle).abs() {
            oa
        } else {
            ob
        };
        self.point_dim(line, point)
    }

    /// The distance from `line` to `point` (a point, or a line standing for its middle).
    fn point_dim(&self, line: EntityId, point: EntityId) -> Option<TempDim> {
        let (_, _, pa, pb) = self.line_ends(line)?;
        let n = (pb - pa).try_normalize()?.perp();
        let q = self.doc.ref_point(point)?;
        let s = (q - pa).dot(n);
        if s.abs() <= 1e-9 * (1.0 + pa.distance(pb)) {
            return None;
        }
        Some(TempDim {
            constraint: Constraint::PointLineDistance {
                point,
                line,
                value: 0.0,
            },
            // A larger distance moves the picked line away from the other end.
            moves: TempMove::Shift {
                curve: line,
                dir: -n * s.signum(),
            },
            reference: Some(point),
        })
    }

    /// The same temporary dimension measured to `target` instead (a point, or a line: one
    /// parallel to the picked one, else its middle). `None` if it can't be.
    pub fn retarget_temp_dim(&self, t: &TempDim, target: EntityId) -> Option<TempDim> {
        let TempMove::Shift { curve, .. } = t.moves else {
            return None;
        };
        if target == curve {
            return None;
        }
        let ends = self.line_ends(curve)?;
        if target == ends.0 || target == ends.1 {
            return None;
        }
        match self.doc.get(target)?.kind {
            EntityKind::Line { .. } => {
                let (_, _, qa, qb) = self.line_ends(target)?;
                let u = (ends.3 - ends.2).try_normalize()?;
                if parallel(u, (qb - qa).try_normalize()?) {
                    self.wall_dim(curve, target)
                } else {
                    self.point_dim(curve, target)
                }
            }
            k if k.is_point() => self.point_dim(curve, target),
            _ => None,
        }
    }

    /// Type a value into a temporary dimension: the picked curve moves (or resizes) to suit,
    /// one undo step. Nothing is kept holding it.
    pub fn set_temp_dim(&mut self, t: &TempDim, value: f64) -> Result<(), String> {
        let current =
            drift_solver::measure(&self.doc, &t.constraint).ok_or("what it measures is gone")?;
        let positive = matches!(t.moves, TempMove::End { .. } | TempMove::Size { .. });
        if !value.is_finite() || value < 0.0 || (positive && value <= 0.0) {
            return Err("it must be more than zero".into());
        }
        let delta = value - current;
        if delta.abs() <= 1e-12 * (1.0 + value) {
            return Ok(());
        }
        let held = || "its relations hold it in place".to_owned();
        match t.moves {
            TempMove::End { point, dir } => {
                let to = self.doc.point(point).map_err(|e| e.to_string())? + dir * delta;
                if !self.move_point_to(point, to) {
                    return Err(held());
                }
            }
            TempMove::Size { curve, diameter } => {
                let radius = if diameter { value / 2.0 } else { value };
                self.doc.begin();
                if !self.drag_radius(curve, radius) {
                    self.doc.rollback();
                    self.sync();
                    return Err(held());
                }
                self.commit("Resize");
            }
            TempMove::Shift { curve, dir } => {
                self.doc.begin();
                let dragged: Vec<Param> = self
                    .doc
                    .defining_points([curve])
                    .iter()
                    .copied()
                    .flat_map(Param::point)
                    .collect();
                if !self.drag(&[curve], dir * delta, &dragged) {
                    // Too far for one solve: go in steps.
                    const STEPS: u32 = 16;
                    for _ in 0..STEPS {
                        self.drag(&[curve], dir * delta / f64::from(STEPS), &dragged);
                    }
                }
                // A drag follows closely rather than exactly: close what's left.
                for _ in 0..4 {
                    let Some(now) = drift_solver::measure(&self.doc, &t.constraint) else {
                        break;
                    };
                    if (value - now).abs() < 1e-10 {
                        break;
                    }
                    self.drag(&[curve], dir * (value - now), &dragged);
                }
                let reached = drift_solver::measure(&self.doc, &t.constraint)
                    .is_some_and(|now| (now - value).abs() <= 1e-6 * (1.0 + value));
                if !reached {
                    self.doc.rollback();
                    self.sync();
                    return Err(held());
                }
                self.commit("Move");
            }
        }
        Ok(())
    }

    /// Make a temporary dimension a real one, its label `offset` from its anchor. If it would
    /// over-define the sketch it's added as driven (a reference); the flag says so.
    pub fn make_temp_dim_permanent(
        &mut self,
        t: &TempDim,
        offset: Option<DVec2>,
    ) -> Result<(ConstraintId, bool), String> {
        let c = t.measured(self).ok_or("what it measures is gone")?;
        match self.add_dimension(c, offset) {
            Ok(id) => Ok((id, false)),
            Err(EditError::Redundant | EditError::Conflict) => {
                self.pending_driven = None;
                self.add_driven_dimension(c, offset)
                    .map(|id| (id, true))
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The shape a temporary dimension's label sits beside (for placing it).
pub fn label_spot(ed: &Editor, t: &TempDim, out: f64) -> Option<DVec2> {
    match t.constraint {
        Constraint::Distance { a, b, .. } => {
            let (pa, pb) = (ed.doc.point(a).ok()?, ed.doc.point(b).ok()?);
            let n = (pb - pa).try_normalize()?.perp();
            // On the side away from a neighbour, if one is close.
            Some((pa + pb) * 0.5 + n * out)
        }
        Constraint::PointLineDistance { point, line, .. } => {
            let q = ed.doc.ref_point(point)?;
            let foot = crate::labels::foot(&ed.doc, q, line)?;
            // Halfway between, where the lines run alongside.
            let Some(Shape::Line { a, b }) = ed.doc.shape(line) else {
                return Some((q + foot) * 0.5);
            };
            let len = a.distance(b);
            let u = (b - a) / len;
            let t = (foot - a).dot(u).clamp(0.0, len);
            Some(a + u * t + (q - foot) * 0.5)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::Document;

    /// Two walls 100 apart (y = 0 and y = 100), 400 long, and a third further up.
    fn walls() -> (Editor, [EntityId; 3]) {
        let mut doc = Document::new();
        let mut line = |y: f64, x0: f64, x1: f64| {
            let a = doc.add_point(DVec2::new(x0, y));
            let b = doc.add_point(DVec2::new(x1, y));
            doc.add_line(a, b).unwrap()
        };
        let lines = [
            line(0.0, 0.0, 400.0),
            line(100.0, 0.0, 400.0),
            line(250.0, 100.0, 300.0),
        ];
        (Editor::new(doc), lines)
    }

    fn value(ed: &Editor, t: &TempDim) -> f64 {
        t.measured(ed).unwrap().value().unwrap()
    }

    #[test]
    fn a_picked_line_shows_its_length_and_its_neighbours() {
        let (mut ed, [low, mid, high]) = walls();
        ed.selection = [mid].into();
        let dims = ed.temp_dims();
        let values: Vec<f64> = dims.iter().map(|t| value(&ed, t)).collect();
        assert_eq!(values, [400.0, 150.0, 100.0]);
        assert_eq!(
            dims[1]
                .reference
                .and_then(|p| ed.doc.ref_point(p))
                .unwrap()
                .y,
            250.0
        );
        let _ = (low, high);
        // Nothing else picked only.
        ed.selected_fills.insert(drift_core::FillId(1));
        assert!(ed.temp_dims().is_empty());
    }

    #[test]
    fn typing_a_value_moves_the_picked_line_only() {
        let (mut ed, [low, mid, _]) = walls();
        ed.selection = [mid].into();
        let below = ed.temp_dims()[2];
        ed.set_temp_dim(&below, 120.0).unwrap();
        assert!((value(&ed, &below) - 120.0).abs() < 1e-9);
        let y = |ed: &Editor, l| match ed.doc.shape(l) {
            Some(Shape::Line { a, .. }) => a.y,
            _ => f64::NAN,
        };
        assert_eq!(y(&ed, low), 0.0, "the other end stays");
        assert!((y(&ed, mid) - 120.0).abs() < 1e-9);
        assert!(ed.doc.constraints().next().is_none(), "nothing is kept");
        // The length moves its far end.
        let length = ed.temp_dims()[0];
        ed.set_temp_dim(&length, 250.0).unwrap();
        assert!((value(&ed, &length) - 250.0).abs() < 1e-9);
        // One undo each.
        ed.undo();
        ed.undo();
        assert_eq!(y(&ed, mid), 100.0);
        assert!(ed.set_temp_dim(&length, -5.0).is_err());
    }

    #[test]
    fn a_held_line_says_so() {
        let (mut ed, [_, mid, _]) = walls();
        let Some(EntityKind::Line { a, b }) = ed.doc.get(mid).map(|e| e.kind) else {
            unreachable!()
        };
        ed.doc.begin();
        for p in [a, b] {
            ed.doc
                .add_constraint(Constraint::Fix {
                    point: p,
                    at: ed.doc.point(p).unwrap(),
                })
                .unwrap();
        }
        ed.commit("Fix");
        ed.selection = [mid].into();
        let below = ed.temp_dims()[2];
        assert!(ed.set_temp_dim(&below, 120.0).is_err());
        assert!((value(&ed, &below) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_far_end_can_move_and_it_can_be_made_permanent() {
        let (mut ed, [low, mid, high]) = walls();
        ed.selection = [mid].into();
        let above = ed.temp_dims()[1];
        // Onto the lower wall instead, then onto a point.
        let to_low = ed.retarget_temp_dim(&above, low).unwrap();
        assert_eq!(value(&ed, &to_low), 100.0);
        let Some(EntityKind::Line { a: corner, .. }) = ed.doc.get(high).map(|e| e.kind) else {
            unreachable!()
        };
        assert_eq!(
            value(&ed, &ed.retarget_temp_dim(&above, corner).unwrap()),
            150.0
        );
        assert!(ed.retarget_temp_dim(&above, mid).is_none(), "not itself");
        // Permanent: a real dimension; then it isn't shown as a temporary one.
        let (id, driven) = ed.make_temp_dim_permanent(&to_low, None).unwrap();
        assert!(!driven);
        assert_eq!(ed.doc.constraint(id).unwrap().value(), Some(100.0));
        assert_eq!(ed.temp_dims().len(), 2);
    }

    #[test]
    fn circles_and_arcs_resize() {
        let mut doc = Document::new();
        let c = doc.add_point(DVec2::ZERO);
        let circle = doc.add_circle(c, 10.0).unwrap();
        let mut ed = Editor::new(doc);
        ed.selection = [circle].into();
        let dims = ed.temp_dims();
        assert_eq!(dims.len(), 1);
        assert_eq!(value(&ed, &dims[0]), 20.0);
        ed.set_temp_dim(&dims[0], 30.0).unwrap();
        assert!((drift_solver::radius_of(&ed.doc, circle).unwrap() - 15.0).abs() < 1e-9);
    }
}
