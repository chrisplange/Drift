//! Leader tips on a spot of a curve (#460): an edit that changes a curve's extent (a split,
//! a trim, an extension, a fillet or a chamfer) keeps each such tip where it was in the
//! drawing, on whichever piece now holds that spot.
//!
//! A tip on a curve is kept as `Anchor::Curve { curve, t }`, where `t` is a fraction of a
//! line, an angle of an arc or circle, a parametric angle of an ellipse, or a Bézier's `t`.
//! Changing the curve's ends without re-reading `t` would move a line's tip, and leave an
//! arc's tip on a piece that no longer runs through it.

use drift_core::tolerance;
use drift_core::{Anchor, Annotation, AnnotationId, DVec2, EntityId, Shape, SheetAnnotation};
use drift_core::{SheetAnnotationId, SheetId};
use drift_geom::{nearest_on_bezier, nearest_on_ellipse, nearest_point};

use crate::editor::Editor;

/// Which leader a tip belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TipOf {
    /// A model note's leader `leader`.
    Note(AnnotationId, usize),
    /// A sheet note's leader `leader` (through a viewport, so `to` means the model).
    Sheet(SheetId, SheetAnnotationId, usize),
}

/// A leader tip on a curve, and where it was.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tip {
    of: TipOf,
    curve: EntityId,
    /// The curve's shape then: a curve the edit didn't reshape keeps its tips as they are.
    shape: Shape,
    at: DVec2,
}

/// Whether `piece` lies on the same line (for a line) or circle (for an arc or circle) as
/// `whole`: a piece a split or trim could have made of it. Other kinds never match.
fn same_path(whole: &Shape, piece: &Shape, tol: f64) -> bool {
    let off_line = |a: DVec2, b: DVec2, p: DVec2| {
        let d = b - a;
        let len = d.length();
        len > 0.0 && (d.perp_dot(p - a) / len).abs() <= tol
    };
    let circle = |s: &Shape| match *s {
        Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => {
            Some((center, radius))
        }
        _ => None,
    };
    match (*whole, *piece) {
        (Shape::Line { a, b }, Shape::Line { a: p, b: q }) => {
            off_line(a, b, p) && off_line(a, b, q)
        }
        _ => match (circle(whole), circle(piece)) {
            (Some((c1, r1)), Some((c2, r2))) => c1.distance(c2) <= tol && (r1 - r2).abs() <= tol,
            _ => false,
        },
    }
}

/// The anchor for a spot on curve `curve` (shape `shape`) nearest `at`, and that spot.
fn anchor_on(curve: EntityId, shape: &Shape, at: DVec2) -> Option<(Anchor, DVec2)> {
    let t = match *shape {
        Shape::Line { a, b } => {
            let d = b - a;
            if d.length_squared() > 0.0 {
                ((at - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        Shape::Circle { center, .. } | Shape::Arc { center, .. } => {
            let v = nearest_point(shape, at) - center;
            v.y.atan2(v.x)
        }
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => nearest_on_ellipse(center, major, minor, start, sweep, at).1,
        Shape::Bezier { p } => nearest_on_bezier(&p, at).1,
        Shape::Point(_) => return None,
    };
    t.is_finite().then(|| {
        let a = Anchor::Curve { curve, t };
        (a, nearest_point(shape, at))
    })
}

impl Editor {
    /// Every leader tip on a curve, and where it is now: call before changing curves.
    pub(crate) fn curve_tips(&self) -> Vec<Tip> {
        let mut tips = Vec::new();
        let mut take = |of: TipOf, to: &Anchor| {
            if let Anchor::Curve { curve, .. } = *to
                && let Some(at) = self.doc.anchor_pos(to)
                && let Some(shape) = self.doc.shape(curve)
            {
                tips.push(Tip {
                    of,
                    curve,
                    shape,
                    at,
                });
            }
        };
        for (id, a) in self.doc.annotations() {
            if let Annotation::Note { leaders, .. } = a {
                for (k, l) in leaders.iter().enumerate() {
                    take(TipOf::Note(id, k), &l.to);
                }
            }
        }
        for (sheet, s) in self.doc.sheets() {
            for (id, a) in &s.annotations {
                if let SheetAnnotation::Note { leaders, .. } = a {
                    for (k, l) in leaders.iter().enumerate() {
                        if l.port.is_some() {
                            take(TipOf::Sheet(sheet, *id, k), &l.leader.to);
                        }
                    }
                }
            }
        }
        tips
    }

    /// Put each tip of `tips` (from [`Editor::curve_tips`] before the edit) back where it
    /// was: on its curve, or on a piece of the same line or circle the edit made, whichever
    /// runs through the spot. A spot no piece holds any more (trimmed away, cut off by a
    /// fillet) takes the nearest spot of any of them. A tip on a curve the edit didn't reshape
    /// is left exactly as it is, and so is one whose curve is gone (that's the deletion's).
    pub(crate) fn keep_curve_tips(&mut self, tips: &[Tip]) {
        if tips.is_empty() {
            return;
        }
        let made: Vec<EntityId> = self
            .doc
            .created_in_transaction()
            .into_iter()
            .filter(|c| self.doc.get(*c).is_some_and(|e| !e.kind.is_point()))
            .collect();
        for tip in tips {
            let Some(own) = self.doc.shape(tip.curve) else {
                continue;
            };
            let Some(now) = self.tip_anchor(tip.of) else {
                continue;
            };
            if !matches!(now, Anchor::Curve { curve, .. } if curve == tip.curve) {
                continue;
            }
            if drift_geom::same_shape(&own, &tip.shape) {
                // Its curve wasn't reshaped: the tip stays exactly as it was.
                continue;
            }
            let tol = self.tol_for(&own).max(tolerance::TINY_MM);
            let off = |b: &Option<(Anchor, DVec2)>| {
                b.map_or(f64::INFINITY, |(_, p): (Anchor, DVec2)| p.distance(tip.at))
            };
            let mut best = anchor_on(tip.curve, &own, tip.at);
            if off(&best) > tol {
                // Another piece of the same line or circle, made by this edit.
                for c in &made {
                    let Some(shape) = self.doc.shape(*c) else {
                        continue;
                    };
                    if !same_path(&tip.shape, &shape, tol) {
                        continue;
                    }
                    let here = anchor_on(*c, &shape, tip.at);
                    if off(&here) < off(&best) {
                        best = here;
                    }
                }
                // (Held by no piece, it takes the nearest spot of any of them.)
            }
            let Some((to, spot)) = best else {
                continue;
            };
            let unmoved = self
                .doc
                .anchor_pos(&now)
                .is_some_and(|p| p.distance(spot) <= tol);
            if to != now
                && !(unmoved && matches!(to, Anchor::Curve { curve, .. } if curve == tip.curve))
            {
                self.set_tip_anchor(tip.of, to);
            }
        }
    }

    fn tip_anchor(&self, of: TipOf) -> Option<Anchor> {
        match of {
            TipOf::Note(id, k) => match self.doc.annotation(id)? {
                Annotation::Note { leaders, .. } => leaders.get(k).map(|l| l.to),
                _ => None,
            },
            TipOf::Sheet(sheet, id, k) => match self.doc.sheet_annotation(sheet, id)? {
                SheetAnnotation::Note { leaders, .. } => leaders.get(k).map(|l| l.leader.to),
                _ => None,
            },
        }
    }

    fn set_tip_anchor(&mut self, of: TipOf, to: Anchor) {
        match of {
            TipOf::Note(id, k) => {
                if let Some(mut a) = self.doc.annotation(id).cloned() {
                    if let Annotation::Note { leaders, .. } = &mut a
                        && let Some(l) = leaders.get_mut(k)
                    {
                        l.to = to;
                    }
                    // The curve is there and `t` finite: it validates.
                    let _ = self.doc.set_annotation(id, a);
                }
            }
            TipOf::Sheet(sheet, id, k) => {
                if let Some(mut a) = self.doc.sheet_annotation(sheet, id).cloned() {
                    if let SheetAnnotation::Note { leaders, .. } = &mut a
                        && let Some(l) = leaders.get_mut(k)
                    {
                        l.leader.to = to;
                    }
                    let _ = self.doc.set_sheet_annotation(sheet, id, a);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{Bounds, Document, Leader, Sheet, SheetLeader, View, Viewport};

    use super::*;

    const V: fn(f64, f64) -> DVec2 = DVec2::new;

    /// A drawing with the curve `make` makes, a 1:10 view of it on a sheet, a model note whose
    /// leader points at `t` on the curve, and a sheet note whose leader points at the same
    /// spot through the view. Returns the editor, the curve and the two tips.
    fn noted(
        make: impl FnOnce(&mut Document) -> EntityId,
        t: f64,
    ) -> (Editor, EntityId, [TipOf; 2]) {
        let mut doc = Document::new();
        let curve = make(&mut doc);
        let view = doc.add_view(View::new(
            "Plan",
            Bounds {
                min: V(-1250.0, -1250.0),
                max: V(1250.0, 1250.0),
            },
            10.0,
        ));
        let mut s = Sheet::new("A-101");
        s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
        let sheet = doc.add_sheet(s);
        let mut ed = Editor::new(doc);
        ed.doc.begin();
        let to = Anchor::Curve { curve, t };
        let note = ed
            .doc
            .add_annotation(Annotation::Note {
                text: "Note".into(),
                at: V(0.0, 300.0),
                leaders: vec![Leader::to(to)],
                style: Default::default(),
            })
            .unwrap();
        let port = ed.doc.tie_viewport(sheet, 0).unwrap();
        let mut on_sheet = SheetAnnotation::note("Sheet note", V(250.0, 250.0));
        if let SheetAnnotation::Note { leaders, .. } = &mut on_sheet {
            leaders.push(SheetLeader::through(port, to));
        }
        let sheet_note = ed.doc.add_sheet_annotation(sheet, on_sheet).unwrap();
        assert!(ed.commit("Notes"));
        let tips = [TipOf::Note(note, 0), TipOf::Sheet(sheet, sheet_note, 0)];
        (ed, curve, tips)
    }

    fn line(a: DVec2, b: DVec2) -> impl FnOnce(&mut Document) -> EntityId {
        move |doc| {
            let (pa, pb) = (doc.add_point(a), doc.add_point(b));
            doc.add_line(pa, pb).unwrap()
        }
    }

    /// A line from (0, 0) to (`length`, 0), and a line across it at x = 150.
    fn with_wall(length: f64) -> impl FnOnce(&mut Document) -> EntityId {
        move |doc| {
            let (a, b) = (
                doc.add_point(V(150.0, -50.0)),
                doc.add_point(V(150.0, 50.0)),
            );
            doc.add_line(a, b).unwrap();
            line(V(0.0, 0.0), V(length, 0.0))(doc)
        }
    }

    /// Where tip `of` is, and the curve it is on.
    fn tip(ed: &Editor, of: TipOf) -> (DVec2, EntityId) {
        let to = ed.tip_anchor(of).expect("the leader");
        let Anchor::Curve { curve, .. } = to else {
            panic!("still on a curve: {to:?}");
        };
        (ed.doc.anchor_pos(&to).unwrap(), curve)
    }

    /// Each tip is at `at`, on a curve that runs through it (`on`, when given).
    #[track_caller]
    fn tips_at(ed: &Editor, tips: [TipOf; 2], at: DVec2, on: Option<EntityId>) {
        for of in tips {
            let (p, curve) = tip(ed, of);
            assert!(p.distance(at) < 1e-6, "{of:?}: at {p}, not {at}");
            let shape = ed.doc.shape(curve).unwrap();
            assert!(
                nearest_point(&shape, at).distance(at) < 1e-6,
                "{of:?}: its curve does not run through {at}"
            );
            if let Some(on) = on {
                assert_eq!(curve, on, "{of:?}");
            }
        }
    }

    /// #460: splitting a line keeps a model note's and a sheet note's leader tips where they
    /// were, whether the spot stays on the piece that keeps the line's id or falls on the new
    /// one; and after undo and redo.
    #[test]
    fn splitting_a_line_keeps_leader_tips_in_place() {
        let spot = V(900.0, 0.0);
        // Cut past the tip: it stays on the piece that keeps the id, at a new fraction.
        let (mut ed, l, tips) = noted(line(V(0.0, 0.0), V(1000.0, 0.0)), 0.9);
        tips_at(&ed, tips, spot, Some(l));
        ed.split_at(l, V(950.0, 0.0)).unwrap();
        tips_at(&ed, tips, spot, Some(l));
        assert!(ed.undo());
        tips_at(&ed, tips, spot, Some(l));
        assert!(ed.redo());
        tips_at(&ed, tips, spot, Some(l));
        // Cut before the tip: it moves onto the new piece.
        let (mut ed, l, tips) = noted(line(V(0.0, 0.0), V(1000.0, 0.0)), 0.9);
        ed.split_at(l, V(500.0, 0.0)).unwrap();
        tips_at(&ed, tips, spot, None);
        assert_ne!(tip(&ed, tips[0]).1, l, "on the new piece");
        assert!(ed.undo());
        tips_at(&ed, tips, spot, Some(l));
        assert!(ed.redo());
        tips_at(&ed, tips, spot, None);
        assert_ne!(tip(&ed, tips[1]).1, l, "on the new piece");
    }

    /// #460, arcs and circles: a tip's angle is still right after a split, but it has to be
    /// on the piece that holds it.
    #[test]
    fn splitting_an_arc_or_circle_keeps_leader_tips_on_the_piece_holding_them() {
        let arc = |doc: &mut Document| {
            let c = doc.add_point(V(0.0, 0.0));
            let s = doc.add_point(V(100.0, 0.0));
            let e = doc.add_point(V(-100.0, 0.0));
            doc.add_arc(c, s, e).unwrap()
        };
        let angle = 120f64.to_radians();
        let spot = DVec2::from_angle(angle) * 100.0;
        for (cut, keeps) in [(150.0, true), (30.0, false)] {
            let (mut ed, a, tips) = noted(arc, angle);
            ed.split_at(a, DVec2::from_angle(f64::to_radians(cut)) * 100.0)
                .unwrap();
            tips_at(&ed, tips, spot, keeps.then_some(a));
            assert_eq!(tip(&ed, tips[0]).1 == a, keeps, "cut at {cut} degrees");
            assert!(ed.undo());
            tips_at(&ed, tips, spot, Some(a));
            assert!(ed.redo());
            tips_at(&ed, tips, spot, keeps.then_some(a));
        }
        let circle = |doc: &mut Document| {
            let c = doc.add_point(V(0.0, 0.0));
            doc.add_circle(c, 100.0).unwrap()
        };
        let angle = -60f64.to_radians();
        let spot = DVec2::from_angle(angle) * 100.0;
        let (mut ed, c, tips) = noted(circle, angle);
        ed.split_at(c, V(100.0, 0.0)).unwrap();
        tips_at(&ed, tips, spot, None);
        assert!(ed.undo());
        tips_at(&ed, tips, spot, Some(c));
    }

    /// #460: trim and extend change a line's ends and keep its id; a tip on it stays put. A
    /// tip on the part trimmed away goes to the nearest spot left.
    #[test]
    fn trim_and_extend_keep_leader_tips_in_place() {
        // Trimmed back to the wall: 0.3 was at 60, 0.9 at 180 (cut away, so to 150).
        for (t, now) in [(0.3, V(60.0, 0.0)), (0.9, V(150.0, 0.0))] {
            let (mut ed, l, tips) = noted(with_wall(200.0), t);
            ed.trim_at(l, V(180.0, 0.0)).unwrap();
            tips_at(&ed, tips, now, Some(l));
            assert!(ed.undo());
            tips_at(&ed, tips, V(200.0 * t, 0.0), Some(l));
        }
        // The middle trimmed out between walls at 100 and 200: a tip at 190 goes to 200, on
        // the far piece, the nearest spot left.
        let walls = |doc: &mut Document| {
            for x in [100.0, 200.0] {
                line(V(x, -50.0), V(x, 50.0))(doc);
            }
            line(V(0.0, 0.0), V(300.0, 0.0))(doc)
        };
        let (mut ed, l, tips) = noted(walls, 190.0 / 300.0);
        ed.trim_at(l, V(150.0, 0.0)).unwrap();
        tips_at(&ed, tips, V(200.0, 0.0), None);
        assert_ne!(tip(&ed, tips[0]).1, l, "on the far piece");
        // Extended to the wall: 0.6 of 100 stays at 60.
        let (mut ed, l, tips) = noted(with_wall(100.0), 0.6);
        ed.extend_at(l, V(95.0, 0.0)).unwrap();
        assert!(ed.doc.shape(l).unwrap().bounds().max.x > 149.0, "extended");
        tips_at(&ed, tips, V(60.0, 0.0), Some(l));
        assert!(ed.undo());
        tips_at(&ed, tips, V(60.0, 0.0), Some(l));
    }

    /// #460: a tip on a curve the edit doesn't reshape is left exactly as it is, even one
    /// past its arc's end (where an arc shortened after the note was placed leaves it).
    #[test]
    fn tips_on_other_curves_are_left_alone() {
        let other = std::cell::Cell::new(None);
        let arc_and_line = |doc: &mut Document| {
            other.set(Some(line(V(0.0, -300.0), V(1000.0, -300.0))(doc)));
            let c = doc.add_point(V(0.0, 0.0));
            let s = doc.add_point(V(100.0, 0.0));
            let e = doc.add_point(DVec2::from_angle(150f64.to_radians()) * 100.0);
            doc.add_arc(c, s, e).unwrap()
        };
        let past_end = 170f64.to_radians();
        let (mut ed, _, tips) = noted(arc_and_line, past_end);
        let before = tips.map(|of| ed.tip_anchor(of));
        let other = other.get().unwrap();
        ed.split_at(other, V(500.0, -300.0)).unwrap();
        assert_eq!(tips.map(|of| ed.tip_anchor(of)), before);
    }

    /// #460: a fillet or chamfer shortens both lines at the corner; a tip on one stays put,
    /// and a tip on the corner cut off goes to the nearest spot left on its line.
    #[test]
    fn fillet_and_chamfer_keep_leader_tips_in_place() {
        let corner = |doc: &mut Document| {
            let a = doc.add_point(V(0.0, 0.0));
            let b = doc.add_point(V(100.0, 0.0));
            let c = doc.add_point(V(100.0, 100.0));
            doc.add_line(b, c).unwrap();
            doc.add_line(a, b).unwrap()
        };
        for chamfer in [false, true] {
            for (t, now) in [(0.4, V(40.0, 0.0)), (0.95, V(90.0, 0.0))] {
                let (mut ed, l, tips) = noted(corner, t);
                let drift_core::EntityKind::Line { b: p, .. } = ed.doc.get(l).unwrap().kind else {
                    unreachable!("a line");
                };
                if chamfer {
                    ed.chamfer(p, 10.0, 10.0).unwrap();
                } else {
                    ed.fillet(p, 10.0).unwrap();
                }
                tips_at(&ed, tips, now, Some(l));
                assert!(ed.undo());
                tips_at(&ed, tips, V(100.0 * t, 0.0), Some(l));
            }
        }
    }
}
