//! Weld: picked points made one point, at the active point (1) or at their centre (Shift+1).

use drift_core::{DVec2, EntityId};

use crate::editor::Editor;

/// Where welded points end up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeldAt {
    /// The active point (the one picked last): the others move onto it.
    Active,
    /// The average of the picked points' positions.
    Centre,
}

impl Editor {
    /// The picked points, in id order.
    pub fn picked_points(&self) -> Vec<EntityId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| self.doc.get(*id).is_some_and(|e| e.kind.is_point()))
            .collect()
    }

    /// Whether a weld can run: two or more points picked (and, to weld to the active point,
    /// one of them active).
    pub fn can_weld(&self, at: WeldAt) -> bool {
        self.picked_points().len() >= 2 && (at == WeldAt::Centre || self.active_point().is_some())
    }

    /// Whether point `p` is fixed in place (wholly or along one axis).
    fn pinned(&self, p: EntityId) -> bool {
        use drift_core::Constraint::{Fix, FixX, FixY};
        self.doc.constraints_on(p).any(|c| {
            matches!(
                self.doc.constraint(c),
                Some(Fix { .. } | FixX { .. } | FixY { .. })
            )
        })
    }

    /// The active element, if it is a picked point.
    fn active_point(&self) -> Option<EntityId> {
        self.active()
            .filter(|id| self.doc.get(*id).is_some_and(|e| e.kind.is_point()))
    }

    /// Weld the picked points into one, at the active point or at their centre: every curve
    /// that ended at one of them ends at the one point left, which stays picked. One undo
    /// step; on failure nothing changes and the reason comes back.
    pub fn weld_points(&mut self, at: WeldAt) -> Result<EntityId, String> {
        let points = self.picked_points();
        if points.len() < 2 {
            return Err("Pick two or more points to weld".into());
        }
        let keep = match (at, self.active_point()) {
            (_, Some(active)) => active,
            (WeldAt::Active, None) => {
                return Err("Pick the point to weld to last (it becomes the active point)".into());
            }
            (WeldAt::Centre, None) => points[0],
        };
        // A pin would pull the weld to where it holds, not where it was asked to go.
        let moved = |p: &EntityId| *p != keep || at == WeldAt::Centre;
        if points.iter().filter(|p| moved(p)).any(|p| self.pinned(*p)) {
            return Err("A picked point is fixed in place: unfix it to weld".into());
        }
        self.doc.begin();
        if at == WeldAt::Centre {
            let sum: DVec2 = points.iter().filter_map(|p| self.doc.point(*p).ok()).sum();
            let centre = sum / points.len() as f64;
            if let Err(e) = self.doc.set_point(keep, centre) {
                self.doc.rollback();
                self.sync();
                return Err(e.to_string());
            }
        }
        for &p in points.iter().filter(|p| **p != keep) {
            if self.doc.merge_points(p, keep).is_err() {
                self.doc.rollback();
                self.sync();
                return Err(
                    "Can't weld the two ends of one line or arc: it would shrink to nothing".into(),
                );
            }
        }
        let label = match at {
            WeldAt::Active => "Weld",
            WeldAt::Centre => "Weld at centre",
        };
        if !self.commit(label) {
            return Err(self
                .out
                .notice
                .take()
                .unwrap_or_else(|| "The weld doesn't fit the drawing's relations".into()));
        }
        self.selection = [keep].into();
        self.set_active(Some(keep));
        Ok(keep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Two lines apart: (0,0)-(10,0) and (12,2)-(20,10). Returns the editor and the ends.
    fn two_lines() -> (Editor, [EntityId; 4]) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 0.0));
        let c = ed.doc.add_point(v(12.0, 2.0));
        let d = ed.doc.add_point(v(20.0, 10.0));
        ed.doc.add_line(a, b).unwrap();
        ed.doc.add_line(c, d).unwrap();
        assert!(ed.commit("Lines"));
        (ed, [a, b, c, d])
    }

    fn point_count(ed: &Editor) -> usize {
        ed.doc.iter().filter(|(_, e)| e.kind.is_point()).count()
    }

    #[test]
    fn weld_moves_the_picked_points_onto_the_active_one() {
        let (mut ed, [_, b, c, _]) = two_lines();
        ed.selection = [b, c].into();
        ed.set_active(Some(b));
        assert!(ed.can_weld(WeldAt::Active));
        assert_eq!(ed.weld_points(WeldAt::Active), Ok(b));
        assert_eq!(point_count(&ed), 3);
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 0.0));
        assert_eq!(ed.selection, [b].into());
        assert!(ed.undo());
        assert_eq!(point_count(&ed), 4);
    }

    #[test]
    fn weld_at_centre_meets_halfway() {
        let (mut ed, [_, b, c, _]) = two_lines();
        ed.selection = [b, c].into();
        ed.set_active(Some(c));
        let kept = ed.weld_points(WeldAt::Centre).unwrap();
        assert_eq!(kept, c);
        assert_eq!(point_count(&ed), 3);
        assert!(ed.doc.point(c).unwrap().distance(v(11.0, 1.0)) < 1e-9);
    }

    #[test]
    fn weld_at_centre_needs_no_active_point() {
        let (mut ed, [a, b, c, _]) = two_lines();
        ed.selection = [b, c].into();
        ed.set_active(None);
        assert!(!ed.can_weld(WeldAt::Active));
        assert!(ed.weld_points(WeldAt::Active).is_err());
        assert!(ed.can_weld(WeldAt::Centre));
        ed.weld_points(WeldAt::Centre).unwrap();
        assert_eq!(point_count(&ed), 3);
        let _ = a;
    }

    #[test]
    fn welding_a_lines_own_ends_is_refused_and_changes_nothing() {
        let (mut ed, [a, b, _, _]) = two_lines();
        let before = ed.doc.clone();
        ed.selection = [a, b].into();
        ed.set_active(Some(a));
        assert!(ed.weld_points(WeldAt::Active).is_err());
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn a_fixed_point_that_would_move_is_refused() {
        let (mut ed, [_, b, c, _]) = two_lines();
        ed.doc.begin();
        ed.doc
            .add_constraint(drift_core::Constraint::Fix {
                point: c,
                at: v(12.0, 2.0),
            })
            .unwrap();
        assert!(ed.commit("Fix"));
        let before = ed.doc.clone();
        ed.selection = [b, c].into();
        // c would move onto b: refused.
        ed.set_active(Some(b));
        assert!(ed.weld_points(WeldAt::Active).is_err());
        assert_eq!(ed.doc, before);
        // Onto the fixed point itself is fine.
        ed.set_active(Some(c));
        assert_eq!(ed.weld_points(WeldAt::Active), Ok(c));
    }

    #[test]
    fn three_points_weld_into_one() {
        let (mut ed, [a, b, c, _]) = two_lines();
        ed.doc.begin();
        let e = ed.doc.add_point(v(11.0, -3.0));
        let f = ed.doc.add_point(v(30.0, -3.0));
        ed.doc.add_line(e, f).unwrap();
        assert!(ed.commit("Line"));
        ed.selection = [b, c, e].into();
        ed.set_active(Some(e));
        ed.weld_points(WeldAt::Active).unwrap();
        assert_eq!(point_count(&ed), 4);
        assert_eq!(ed.doc.point(e).unwrap(), v(11.0, -3.0));
        let _ = a;
    }
}
