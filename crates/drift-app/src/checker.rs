//! The sketch checker: ends that nearly meet but don't, ends that stop just short of a curve,
//! and curves of no length, each with a fix (as SolidWorks' Check Sketch for Feature finds
//! the gaps that keep a profile from closing).
//!
//! An open end is the end of a solid curve that nothing else uses, joins or holds on to (a
//! coincidence, a point-on-curve or a midpoint relation). Construction geometry and locked
//! curves are left alone.

use std::collections::{BTreeMap, BTreeSet};

use drift_core::{Constraint, DVec2, EntityId, EntityKind, LineKind};
use drift_solver::Param;

use crate::editor::Editor;

/// What far-off geometry is, in words: "3 curves" (or points, where there are no curves).
pub fn far_off_count(doc: &drift_core::Document, ids: &[EntityId]) -> String {
    let curves = ids
        .iter()
        .filter(|id| doc.get(**id).is_some_and(|e| !e.kind.is_point()))
        .count();
    match (curves, ids.len()) {
        (1, _) => "1 curve".to_owned(),
        (0, 1) => "1 point".to_owned(),
        (0, n) => format!("{n} points"),
        (n, _) => format!("{n} curves"),
    }
}

/// Something the checker found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Issue {
    /// Two open ends `distance` apart that look meant to meet. Fixed by joining them into
    /// one point; `keep` is the one that stays where it is.
    Gap {
        keep: EntityId,
        join: EntityId,
        distance: f64,
    },
    /// An open end `distance` short of (or past) a curve. Fixed with a point-on-curve
    /// relation, which pulls it onto the curve.
    Short {
        point: EntityId,
        curve: EntityId,
        distance: f64,
    },
    /// A line or arc with no length. Fixed by removing it (joining its ends).
    ZeroLength { curve: EntityId },
}

impl Issue {
    pub fn label(&self) -> &'static str {
        match self {
            Issue::Gap { .. } => "Gap between ends",
            Issue::Short { .. } => "End short of a curve",
            Issue::ZeroLength { .. } => "Curve of no length",
        }
    }

    /// The geometry it concerns (to pick and show).
    pub fn entities(&self) -> Vec<EntityId> {
        match *self {
            Issue::Gap { keep, join, .. } => vec![keep, join],
            Issue::Short { point, curve, .. } => vec![point, curve],
            Issue::ZeroLength { curve } => vec![curve],
        }
    }

    pub fn distance(&self) -> Option<f64> {
        match *self {
            Issue::Gap { distance, .. } | Issue::Short { distance, .. } => Some(distance),
            Issue::ZeroLength { .. } => None,
        }
    }
}

/// The two ends of a curve that has ends.
fn ends(kind: &EntityKind) -> Option<[EntityId; 2]> {
    match *kind {
        EntityKind::Line { a, b } | EntityKind::Bezier { a, b, .. } => Some([a, b]),
        EntityKind::Arc { start, end, .. } | EntityKind::EllipseArc { start, end, .. } => {
            Some([start, end])
        }
        _ => None,
    }
}

impl Editor {
    /// Of `ids`, the geometry reaching more than [`drift_core::FAR_MM`] from the origin (or
    /// not finite): the curves, or where there are none, the points.
    pub fn far_off_of(&self, ids: impl IntoIterator<Item = EntityId>) -> Vec<EntityId> {
        let mut curves = Vec::new();
        let mut points = Vec::new();
        for id in ids {
            let Some(e) = self.doc.get(id) else {
                continue;
            };
            let Some(shape) = self.doc.shape(id) else {
                continue;
            };
            if shape.bounds().is_far_off() {
                if e.kind.is_point() {
                    points.push(id);
                } else {
                    curves.push(id);
                }
            }
        }
        if curves.is_empty() { points } else { curves }
    }

    /// What the checker finds, nearest first within each kind: zero-length curves, then
    /// gaps, then ends short of a curve. `tolerance` is in mm.
    pub fn check_sketch(&self, tolerance: f64) -> Vec<Issue> {
        let doc = &self.doc;
        let solid = |id: EntityId| {
            doc.get(id)
                .is_some_and(|e| !e.kind.is_point() && e.style == LineKind::Solid)
                && !self.is_locked(id)
        };
        // Which solid curves each point ends.
        let mut ending: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
        let mut issues = Vec::new();
        for (id, e) in doc.iter() {
            if !solid(id) {
                continue;
            }
            let Some([a, b]) = ends(&e.kind) else {
                continue;
            };
            ending.entry(a).or_default().push(id);
            ending.entry(b).or_default().push(id);
            let zero = match doc.shape(id) {
                Some(drift_core::Shape::Line { a, b }) => a.distance(b) < 1e-9,
                Some(drift_core::Shape::Arc { radius, sweep, .. }) => radius * sweep.abs() < 1e-9,
                _ => false,
            };
            if zero {
                issues.push(Issue::ZeroLength { curve: id });
            }
        }
        // Points held by a relation to something else are attached, not open.
        let attached: BTreeSet<EntityId> = doc
            .constraints()
            .flat_map(|(_, c)| match *c {
                Constraint::Coincident { a, b } => vec![a, b],
                Constraint::PointOnCurve { point, .. } | Constraint::Midpoint { point, .. } => {
                    vec![point]
                }
                _ => Vec::new(),
            })
            .collect();
        // Every other user of a point (a circle's centre, a curve's handle) counts too.
        let open: Vec<(EntityId, EntityId, DVec2)> = ending
            .iter()
            .filter(|(p, curves)| {
                curves.len() == 1 && !attached.contains(p) && doc.users_of(**p).count() == 1
            })
            .filter_map(|(p, curves)| Some((*p, curves[0], doc.point(*p).ok()?)))
            .collect();

        // Gaps: pairs of open ends within the tolerance, the closest first, each end in one
        // pair. Two ends of the same curve are its length, not a gap.
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for i in 0..open.len() {
            for j in i + 1..open.len() {
                let d = open[i].2.distance(open[j].2);
                if d <= tolerance && open[i].1 != open[j].1 {
                    pairs.push((d, i, j));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut used = vec![false; open.len()];
        let mut gaps = Vec::new();
        for (d, i, j) in pairs {
            if used[i] || used[j] {
                continue;
            }
            used[i] = true;
            used[j] = true;
            // The end more held by relations and dimensions stays; on a tie, the older.
            let held = |p: EntityId| doc.constraints_on(p).count();
            let (pi, pj) = (open[i].0, open[j].0);
            let (keep, join) = match held(pi).cmp(&held(pj)) {
                std::cmp::Ordering::Less => (pj, pi),
                std::cmp::Ordering::Greater => (pi, pj),
                std::cmp::Ordering::Equal if pi < pj => (pi, pj),
                std::cmp::Ordering::Equal => (pj, pi),
            };
            gaps.push(Issue::Gap {
                keep,
                join,
                distance: d,
            });
        }
        issues.extend(gaps);

        // Ends short of (or just past) another curve, among those not in a gap.
        let mut short = Vec::new();
        for (k, (p, own, at)) in open.iter().enumerate() {
            if used[k] {
                continue;
            }
            let nearest = self
                .index
                .near(*at, tolerance)
                .filter(|(id, _, d)| *id != *own && *d > 1e-9 && solid(*id))
                // Not a curve it already shares a point with (a corner).
                .filter(|(id, _, _)| {
                    doc.get(*id).is_some_and(|e| {
                        !e.kind.points().any(|q| {
                            doc.get(*own)
                                .is_some_and(|o| o.kind.points().any(|r| r == q))
                        })
                    })
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((curve, _, d)) = nearest {
                short.push(Issue::Short {
                    point: *p,
                    curve,
                    distance: d,
                });
            }
        }
        short.sort_by(|a, b| {
            a.distance()
                .unwrap_or(0.0)
                .total_cmp(&b.distance().unwrap_or(0.0))
        });
        issues.extend(short);
        issues
    }

    /// Fix one issue as one undo step. False (and nothing changed) if it can't be fixed
    /// without breaking a relation or dimension; the notice says why.
    pub fn fix_issue(&mut self, issue: &Issue) -> bool {
        let fixed = self.try_fix(issue, false);
        if !fixed {
            self.notice = Some(format!(
                "Couldn't fix the {}: it conflicts with the relations or dimensions there",
                issue.label().to_lowercase()
            ));
        }
        fixed
    }

    /// Fix every issue it can, as one undo step. Returns how many were fixed and how many
    /// were left (those that would break a relation or dimension, or were already gone).
    pub fn fix_issues(&mut self, issues: &[Issue]) -> (usize, usize) {
        let mut fixed = 0;
        for issue in issues {
            if self.try_fix(issue, fixed > 0) {
                fixed += 1;
            }
        }
        (fixed, issues.len() - fixed)
    }

    /// Apply a fix in its own transaction and keep it if the sketch still solves; `merge`
    /// folds it into the last undo step.
    fn try_fix(&mut self, issue: &Issue, merge: bool) -> bool {
        self.doc.begin();
        let applied = match *issue {
            Issue::Gap { keep, join, .. } => self.doc.merge_points(join, keep).is_ok(),
            Issue::Short { point, curve, .. } => {
                self.doc.contains(point)
                    && self.doc.contains(curve)
                    && self
                        .doc
                        .add_constraint(Constraint::PointOnCurve { point, curve })
                        .is_ok()
            }
            Issue::ZeroLength { curve } => match self.doc.get(curve).map(|e| e.kind) {
                Some(kind) => {
                    let [a, b] = ends(&kind).unwrap_or([curve, curve]);
                    self.release_from_fills(curve);
                    self.doc.delete([curve]);
                    // Whatever else met at its two ends now meets at one.
                    if self.doc.contains(a) && self.doc.contains(b) && a != b {
                        let _ = self.doc.merge_points(b, a);
                    }
                    true
                }
                None => false,
            },
        };
        if !applied {
            self.doc.rollback();
            self.sync();
            return false;
        }
        // What the fix moves onto stays where it is: the kept end, or the curve reached.
        let held: Vec<EntityId> = match *issue {
            Issue::Gap { keep, .. } => vec![keep],
            Issue::Short { curve, .. } => self.doc.defining_points([curve]).into_iter().collect(),
            Issue::ZeroLength { .. } => Vec::new(),
        };
        let mut hold: Vec<Param> = held.into_iter().flat_map(Param::point).collect();
        if let Issue::Short { curve, .. } = *issue
            && matches!(
                self.doc.get(curve).map(|e| e.kind),
                Some(EntityKind::Circle { .. })
            )
        {
            hold.push(Param::Radius(curve));
        }
        let report = self.solve(&hold);
        if self.new_conflicts(&report) {
            self.doc.rollback();
            self.sync();
            return false;
        }
        self.follow_fills();
        let tx = self.doc.commit("Fix sketch");
        if merge {
            self.history.merge_into_last(tx);
        } else {
            self.history.push(tx);
        }
        self.sync();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new drawing's gap tolerance (mm).
    const DEFAULT_TOLERANCE: f64 = drift_core::DrawingRules::DEFAULT_GAP_TOLERANCE;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A 10 × 10 square of four lines whose top-right corner is 0.2 apart, with
    /// a line to the right that stops 0.3 short of the right side.
    fn nearly_closed(ed: &mut Editor) -> [EntityId; 5] {
        ed.doc.begin();
        let p = |ed: &mut Editor, at| ed.doc.add_point(at);
        let (a, b, c, c2, d) = (
            p(ed, v(0.0, 0.0)),
            p(ed, v(10.0, 0.0)),
            p(ed, v(10.0, 10.0)),
            p(ed, v(10.0, 10.2)),
            p(ed, v(0.0, 10.0)),
        );
        ed.doc.add_line(a, b).unwrap();
        ed.doc.add_line(b, c).unwrap();
        ed.doc.add_line(c2, d).unwrap();
        ed.doc.add_line(d, a).unwrap();
        let (e, f) = (p(ed, v(20.0, 5.0)), p(ed, v(10.3, 5.0)));
        ed.doc.add_line(e, f).unwrap();
        ed.commit("Setup");
        [c, c2, f, b, a]
    }

    #[test]
    fn finds_gaps_and_short_ends_and_fixes_them() {
        let mut ed = Editor::default();
        let [c, c2, f, ..] = nearly_closed(&mut ed);
        assert!(ed.regions().is_empty(), "the gap keeps it from closing");
        let issues = ed.check_sketch(DEFAULT_TOLERANCE);
        assert_eq!(issues.len(), 2, "{issues:?}");
        let Issue::Gap {
            keep,
            join,
            distance,
        } = issues[0]
        else {
            panic!("{issues:?}");
        };
        assert_eq!((keep, join), (c, c2), "the older end stays");
        assert!((distance - 0.2).abs() < 1e-9);
        let Issue::Short {
            point, distance, ..
        } = issues[1]
        else {
            panic!("{issues:?}");
        };
        assert_eq!(point, f);
        assert!((distance - 0.3).abs() < 1e-9);
        // A tighter tolerance finds only the gap; a looser one nothing more.
        assert_eq!(ed.check_sketch(0.25).len(), 1);

        // Fixing all: one undo step; the square closes and the line reaches the side.
        assert_eq!(ed.fix_issues(&issues), (2, 0));
        assert!(!ed.doc.contains(c2), "joined into one point");
        assert_eq!(ed.regions().len(), 1, "closed");
        assert!(
            (ed.doc.point(f).unwrap().x - 10.0).abs() < 1e-6,
            "on the side"
        );
        assert!(ed.check_sketch(DEFAULT_TOLERANCE).is_empty());
        ed.undo();
        assert!(ed.doc.contains(c2));
        assert_eq!(ed.check_sketch(DEFAULT_TOLERANCE).len(), 2);
    }

    #[test]
    fn the_held_end_stays_and_conflicts_are_left() {
        let mut ed = Editor::default();
        let [c, c2, ..] = nearly_closed(&mut ed);
        // The newer end is fixed where it is: it stays, the older one moves to it.
        ed.doc.begin();
        ed.doc
            .add_constraint(Constraint::Fix {
                point: c2,
                at: v(10.0, 10.2),
            })
            .unwrap();
        ed.commit("Fix");
        let gap = ed.check_sketch(DEFAULT_TOLERANCE)[0];
        assert!(matches!(gap, Issue::Gap { keep, join, .. } if keep == c2 && join == c));
        assert!(ed.fix_issue(&gap));
        assert_eq!(ed.doc.point(c2).unwrap(), v(10.0, 10.2));
        ed.undo();

        // Both ends fixed apart: joining them can't work, so it is reported and left.
        ed.doc.begin();
        ed.doc
            .add_constraint(Constraint::Fix {
                point: c,
                at: v(10.0, 10.0),
            })
            .unwrap();
        ed.commit("Fix");
        let gap = ed.check_sketch(DEFAULT_TOLERANCE)[0];
        let before = ed.history.undo_label().map(str::to_owned);
        assert!(!ed.fix_issue(&gap));
        assert!(ed.notice.is_some());
        assert!(ed.doc.contains(c) && ed.doc.contains(c2), "nothing changed");
        assert_eq!(ed.history.undo_label().map(str::to_owned), before);
    }

    #[test]
    fn corners_relations_and_construction_are_not_issues() {
        let mut ed = Editor::default();
        ed.doc.begin();
        // A closed triangle.
        let t = [v(0.0, 0.0), v(10.0, 0.0), v(5.0, 8.0)].map(|p| ed.doc.add_point(p));
        for i in 0..3 {
            ed.doc.add_line(t[i], t[(i + 1) % 3]).unwrap();
        }
        // A line whose end sits on the triangle's base by a relation.
        let (p, q) = (
            ed.doc.add_point(v(5.0, -6.0)),
            ed.doc.add_point(v(5.0, 0.0)),
        );
        ed.doc.add_line(p, q).unwrap();
        let base = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Line { a, b } if a == t[0] && b == t[1]))
            .unwrap()
            .0;
        ed.doc
            .add_constraint(Constraint::PointOnCurve {
                point: q,
                curve: base,
            })
            .unwrap();
        // Two construction lines nearly meeting.
        let c =
            [v(30.0, 0.0), v(40.0, 0.0), v(40.1, 0.0), v(50.0, 0.0)].map(|p| ed.doc.add_point(p));
        for (a, b) in [(c[0], c[1]), (c[2], c[3])] {
            let l = ed.doc.add_line(a, b).unwrap();
            ed.doc.set_style(l, LineKind::Construction);
        }
        ed.commit("Setup");
        assert_eq!(ed.check_sketch(DEFAULT_TOLERANCE), Vec::new());
    }

    #[test]
    fn zero_length_lines_are_removed_and_their_ends_joined() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let pts =
            [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 1.0), v(10.0, 5.0)].map(|p| ed.doc.add_point(p));
        ed.doc.add_line(pts[0], pts[1]).unwrap();
        let nothing = ed.doc.add_line(pts[1], pts[2]).unwrap();
        ed.doc.add_line(pts[2], pts[3]).unwrap();
        // Dragged down onto the other end (a line can't be made with no length).
        ed.doc.set_point(pts[2], v(10.0, 0.0)).unwrap();
        ed.commit("Setup");
        let issues = ed.check_sketch(DEFAULT_TOLERANCE);
        assert_eq!(issues, vec![Issue::ZeroLength { curve: nothing }]);
        assert!(ed.fix_issue(&issues[0]));
        assert!(!ed.doc.contains(nothing));
        // The two lines now share their corner.
        assert_eq!(ed.doc.users_of(pts[1]).count(), 2);
        assert!(ed.check_sketch(DEFAULT_TOLERANCE).is_empty());
    }
}
