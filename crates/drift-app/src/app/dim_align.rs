//! Keeping dimensions tidy. A label being dragged snaps so its dimension line is level with
//! another's (a dashed guide shows which; Ctrl places freely), and several picked dimensions
//! can be aligned on one line or spaced evenly.
//!
//! A linear dimension's line runs through its label point (anchor + placement), square to
//! what it measures. Dimensions measuring along the same direction line up when their label
//! points are level across that direction.

use drift_core::{Constraint, ConstraintId, DVec2, Document};

use crate::*;

/// How close (screen points) a dragged dimension's line comes to another's before it snaps.
const SNAP_PT: f64 = 6.0;

/// Which way a linear dimension measures (unit, pointing right or up, so opposite
/// directions compare equal), or `None` for radii, angles and the like.
pub(crate) fn measure_dir(doc: &Document, c: &Constraint) -> Option<DVec2> {
    let p = |id| doc.ref_point(id);
    let u = match *c {
        Constraint::HorizontalDistance { .. } => DVec2::X,
        Constraint::VerticalDistance { .. } => DVec2::Y,
        Constraint::Distance { a, b, .. } => (p(b)? - p(a)?).try_normalize()?,
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point)?;
            (q - annotations::foot(doc, q, line)?).try_normalize()?
        }
        _ => return None,
    };
    Some(if u.x < -1e-12 || (u.x.abs() <= 1e-12 && u.y < 0.0) {
        -u
    } else {
        u
    })
}

fn parallel(u: DVec2, v: DVec2) -> bool {
    u.perp_dot(v).abs() <= 1e-6
}

/// A dimension's label point (where its line runs through) and its measuring direction.
fn label_and_dir(
    doc: &Document,
    id: ConstraintId,
    c: &Constraint,
    scale: f64,
) -> Option<(DVec2, DVec2)> {
    let u = measure_dir(doc, c)?;
    let anchor = annotations::dimension_anchor(doc, c)?;
    let offset = doc
        .placement(id)
        .unwrap_or_else(|| annotations::default_offset(doc, c, scale));
    Some((anchor + offset, u))
}

/// Where a dimension's line sits across its measuring direction `u`.
fn level(label: DVec2, u: DVec2) -> f64 {
    label.dot(u.perp())
}

/// Snap a dragged dimension's label offset so its line is level with the nearest parallel
/// dimension's within [`SNAP_PT`]: the offset to use, and a guide from its label to the
/// other's (world).
pub(crate) fn snap_label(
    doc: &Document,
    id: ConstraintId,
    c: &Constraint,
    offset: DVec2,
    scale: f64,
) -> (DVec2, Option<[DVec2; 2]>) {
    let (Some(u), Some(anchor)) = (measure_dir(doc, c), annotations::dimension_anchor(doc, c))
    else {
        return (offset, None);
    };
    let ours = level(anchor + offset, u);
    let best = doc
        .constraints()
        .filter(|(other, d)| *other != id && d.is_dimension())
        .filter_map(|(other, d)| label_and_dir(doc, other, d, scale))
        .filter(|(_, v)| parallel(u, *v))
        .map(|(label, _)| (label, level(label, u) - ours))
        .filter(|(_, shift)| shift.abs() * scale <= SNAP_PT)
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    match best {
        Some((label, shift)) => {
            let offset = offset + u.perp() * shift;
            (offset, Some([anchor + offset, label]))
        }
        None => (offset, None),
    }
}

/// The new label offsets that put `ids` level with the first (those not parallel to it are
/// left alone).
pub(crate) fn aligned(
    doc: &Document,
    ids: &[ConstraintId],
    scale: f64,
) -> Vec<(ConstraintId, DVec2)> {
    let placed = placed_parallel(doc, ids, scale);
    let Some((_, first, u, _)) = placed.first().copied() else {
        return Vec::new();
    };
    let target = level(first, u);
    placed
        .iter()
        .skip(1)
        .map(|&(id, label, _, anchor)| (id, label + u.perp() * (target - level(label, u)) - anchor))
        .collect()
}

/// The new label offsets that space `ids` evenly across their direction, keeping the two
/// outermost where they are and the order they're in.
pub(crate) fn spaced(
    doc: &Document,
    ids: &[ConstraintId],
    scale: f64,
) -> Vec<(ConstraintId, DVec2)> {
    let mut placed = placed_parallel(doc, ids, scale);
    let Some((_, _, u, _)) = placed.first().copied() else {
        return Vec::new();
    };
    if placed.len() < 3 {
        return Vec::new();
    }
    placed.sort_by(|a, b| level(a.1, u).total_cmp(&level(b.1, u)));
    let (lo, hi) = (level(placed[0].1, u), level(placed[placed.len() - 1].1, u));
    let step = (hi - lo) / (placed.len() - 1) as f64;
    placed
        .iter()
        .enumerate()
        .map(|(k, &(id, label, _, anchor))| {
            let target = lo + step * k as f64;
            (id, label + u.perp() * (target - level(label, u)) - anchor)
        })
        .collect()
}

/// The picked linear dimensions parallel to the first: (id, label point, direction, anchor).
fn placed_parallel(
    doc: &Document,
    ids: &[ConstraintId],
    scale: f64,
) -> Vec<(ConstraintId, DVec2, DVec2, DVec2)> {
    let mut out: Vec<(ConstraintId, DVec2, DVec2, DVec2)> = Vec::new();
    for &id in ids {
        let Some(c) = doc.constraint(id) else {
            continue;
        };
        let (Some((label, u)), Some(anchor)) = (
            label_and_dir(doc, id, c, scale),
            annotations::dimension_anchor(doc, c),
        ) else {
            continue;
        };
        if out.first().is_none_or(|f| parallel(f.2, u)) {
            out.push((id, label, u, anchor));
        }
    }
    out
}

impl DriftApp {
    /// The picked dimensions, the primary first.
    fn picked_dimensions(&self) -> Vec<ConstraintId> {
        self.picked_annotations()
            .into_iter()
            .filter_map(|r| match r {
                drift_core::AnnotationRef::Dimension(id) => Some(id),
                drift_core::AnnotationRef::Annotation(_) => None,
            })
            .collect()
    }

    /// Align the picked dimensions on the first one's line, or (`spread`) space them evenly.
    /// One undo step; says what happened.
    pub(crate) fn align_picked_dimensions(&mut self, spread: bool, now: f64) {
        let ids = self.picked_dimensions();
        let scale = self.camera.scale;
        let moves = if spread {
            spaced(&self.editor.doc, &ids, scale)
        } else {
            aligned(&self.editor.doc, &ids, scale)
        };
        if moves.is_empty() {
            let why = if spread {
                "Pick three or more parallel dimensions (Shift+click) to space them evenly"
            } else {
                "Pick two or more parallel dimensions (Shift+click) to align them"
            };
            self.set_status(why.into(), true, now);
            return;
        }
        self.editor.doc.begin();
        for &(id, offset) in &moves {
            self.editor.place_dimension(id, offset);
        }
        let label = if spread {
            "Space dimensions"
        } else {
            "Align dimensions"
        };
        self.editor.commit(label);
        let done = moves.len() + usize::from(!spread);
        let left_out = ids.len() - done;
        let mut text = format!(
            "{} {done} dimensions",
            if spread { "Spaced" } else { "Aligned" }
        );
        if left_out > 0 {
            text.push_str(&format!(" ({left_out} not parallel, left as they were)"));
        }
        self.set_status(text, false, now);
    }

    /// Whether Align / Space evenly apply: two (or three) or more parallel dimensions picked.
    pub(crate) fn can_align_dimensions(&self) -> (bool, bool) {
        let ids = self.picked_dimensions();
        let n = placed_parallel(&self.editor.doc, &ids, self.camera.scale).len();
        (n >= 2, n >= 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three horizontal distances along the x axis, their lines at y = 10, 13 and 30.
    fn three() -> (Document, [ConstraintId; 3]) {
        let mut doc = Document::new();
        let p: Vec<_> = [0.0, 10.0, 20.0, 30.0]
            .iter()
            .map(|x| doc.add_point(DVec2::new(*x, 0.0)))
            .collect();
        let mut ids = Vec::new();
        for (k, y) in [10.0, 13.0, 30.0].into_iter().enumerate() {
            let c = Constraint::HorizontalDistance {
                a: p[k],
                b: p[k + 1],
                value: 10.0,
            };
            let id = doc.add_constraint(c).unwrap();
            let anchor = annotations::dimension_anchor(&doc, &c).unwrap();
            doc.set_placement(id, DVec2::new(anchor.x, y) - anchor)
                .unwrap();
            ids.push(id);
        }
        (doc, ids.try_into().unwrap())
    }

    fn line_y(doc: &Document, id: ConstraintId) -> f64 {
        let c = doc.constraint(id).unwrap();
        annotations::dimension_anchor(doc, c).unwrap().y + doc.placement(id).unwrap().y
    }

    #[test]
    fn a_dragged_label_snaps_level_with_another() {
        let (doc, [a, b, _]) = three();
        let c = *doc.constraint(b).unwrap();
        let anchor = annotations::dimension_anchor(&doc, &c).unwrap();
        // Dragged to y = 10.3 at 10 pt per mm: 3 pt from the first's line, so it snaps.
        let (offset, guide) = snap_label(&doc, b, &c, DVec2::new(0.0, 10.3) - anchor, 10.0);
        assert!(((anchor + offset).y - line_y(&doc, a)).abs() < 1e-9);
        assert!(guide.is_some());
        // Too far (2 mm = 20 pt): left where it is.
        let (offset, guide) = snap_label(&doc, b, &c, DVec2::new(0.0, 12.0) - anchor, 10.0);
        assert!(((anchor + offset).y - 12.0).abs() < 1e-9);
        assert!(guide.is_none());
    }

    #[test]
    fn picked_dimensions_align_and_space_evenly() {
        let (mut doc, ids) = three();
        for (id, offset) in aligned(&doc, &ids, 10.0) {
            doc.set_placement(id, offset).unwrap();
        }
        for id in ids {
            assert!((line_y(&doc, id) - 10.0).abs() < 1e-9);
        }
        let (mut doc, ids) = three();
        for (id, offset) in spaced(&doc, &ids, 10.0) {
            doc.set_placement(id, offset).unwrap();
        }
        let ys: Vec<f64> = ids.iter().map(|id| line_y(&doc, *id)).collect();
        assert!((ys[0] - 10.0).abs() < 1e-9 && (ys[1] - 20.0).abs() < 1e-9);
        assert!((ys[2] - 30.0).abs() < 1e-9);
        // A vertical dimension picked with them isn't parallel: left alone.
        let q = doc.add_point(DVec2::new(0.0, 50.0));
        let origin = doc.iter().next().unwrap().0;
        let vertical = doc
            .add_constraint(Constraint::VerticalDistance {
                a: q,
                b: origin,
                value: 50.0,
            })
            .unwrap();
        let moved = aligned(&doc, &[ids[0], vertical, ids[2]], 10.0);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].0, ids[2]);
    }
}
