//! Annotations that belong to a sheet (#432): notes, leaders and dimensions placed in paper
//! millimetres on one sheet, shown only there. What they point at or measure is model
//! geometry, seen through one of the sheet's viewports, so it follows the geometry, the view
//! being moved on the paper and the view's scale; the text is a paper size.
//!
//! They are kept in their [`Sheet`](crate::Sheet), not among the model's annotations and
//! constraints: a sheet dimension is never a constraint, so the solver never sees it.

use glam::DVec2;

use crate::Constraint;
use crate::document::EntityId;
use crate::drafting::{Anchor, DimNote, Leader, NoteStyle, StyleOverride};

/// Stable identifier of a sheet annotation (`a3`), unique in the drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetAnnotationId(pub u32);

impl std::fmt::Display for SheetAnnotationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a{}", self.0)
    }
}

/// What a sheet annotation names a viewport by (`r5`): a sheet can hold the same view twice,
/// so the view's id isn't enough. A viewport gets one the first time something is tied to it
/// (see [`Document::tie_viewport`](crate::Document::tie_viewport)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortId(pub u32);

impl std::fmt::Display for PortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// A sheet note's leader: a [`Leader`], and the viewport its tip is seen through.
///
/// Through a viewport (`port`), `leader.to` means what it means in the model: a sketch point,
/// a spot on a curve, or (`Anchor::Free`) a fixed model point. Without one, `Anchor::Free` is
/// a point on the paper (mm) and nothing else is valid. A bent leader's bend is paper mm from
/// its tip either way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetLeader {
    pub leader: Leader,
    pub port: Option<PortId>,
}

impl SheetLeader {
    /// A leader to a point on the paper.
    pub fn on_paper(at: DVec2) -> Self {
        Self {
            leader: Leader::to(Anchor::Free(at)),
            port: None,
        }
    }

    /// A leader to model geometry (or a model point) through a viewport.
    pub fn through(port: PortId, to: Anchor) -> Self {
        Self {
            leader: Leader::to(to),
            port: Some(port),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SheetAnnotation {
    /// Text at `at` (its middle, paper mm), from any number of leaders. The text stays where
    /// it was put whatever its leaders' tips do.
    Note {
        text: String,
        at: DVec2,
        leaders: Vec<SheetLeader>,
        style: NoteStyle,
        /// Its own settings over the drawing's drafting style.
        look: StyleOverride,
    },
    /// A reference dimension of model geometry, seen through viewport `port`. `measure` is a
    /// dimension [`Constraint`] used only to say what is measured (its value is never read:
    /// the label shows the geometry's current measurement); it is not among the drawing's
    /// constraints. `label` is the label's offset from the dimension's anchor in paper mm
    /// (`None`: where a new dimension's goes).
    Dimension {
        port: PortId,
        measure: Constraint,
        label: Option<DVec2>,
        note: DimNote,
        look: StyleOverride,
    },
}

impl SheetAnnotation {
    /// A note without leaders.
    pub fn note(text: impl Into<String>, at: DVec2) -> Self {
        SheetAnnotation::Note {
            text: text.into(),
            at,
            leaders: Vec::new(),
            style: NoteStyle::default(),
            look: StyleOverride::default(),
        }
    }

    /// A dimension of `measure` through `port`, its label `label` paper mm from its anchor.
    pub fn dimension(port: PortId, measure: Constraint, label: Option<DVec2>) -> Self {
        SheetAnnotation::Dimension {
            port,
            measure,
            label,
            note: DimNote::default(),
            look: StyleOverride::default(),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            SheetAnnotation::Note { leaders, .. } if leaders.is_empty() => "Sheet note",
            SheetAnnotation::Note { .. } => "Sheet leader note",
            SheetAnnotation::Dimension { .. } => "Sheet dimension",
        }
    }

    /// The viewports it is tied to.
    pub fn ports(&self) -> Vec<PortId> {
        match self {
            SheetAnnotation::Note { leaders, .. } => {
                leaders.iter().filter_map(|l| l.port).collect()
            }
            SheetAnnotation::Dimension { port, .. } => vec![*port],
        }
    }

    /// The sketch entities it is tied to.
    pub fn entities(&self) -> Vec<EntityId> {
        match self {
            SheetAnnotation::Note { leaders, .. } => leaders
                .iter()
                .filter(|l| l.port.is_some())
                .filter_map(|l| l.leader.to.entity())
                .collect(),
            SheetAnnotation::Dimension { measure, .. } => measure.entities().collect(),
        }
    }

    /// Its own settings over the drafting style.
    pub fn look(&self) -> StyleOverride {
        match self {
            SheetAnnotation::Note { look, .. } | SheetAnnotation::Dimension { look, .. } => *look,
        }
    }

    pub fn set_look(&mut self, to: StyleOverride) {
        match self {
            SheetAnnotation::Note { look, .. } | SheetAnnotation::Dimension { look, .. } => {
                *look = to;
            }
        }
    }

    /// Whether it is well formed apart from what it refers to: a dimension measures with a
    /// dimension, a leader without a viewport points at the paper, and its numbers are finite.
    pub fn is_valid(&self) -> bool {
        let look_ok = |look: &StyleOverride| {
            [
                look.text_height,
                look.arrow_size,
                look.extension_gap,
                look.extension_overshoot,
            ]
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.0)
        };
        match self {
            SheetAnnotation::Note {
                at,
                leaders,
                style,
                look,
                ..
            } => {
                at.is_finite()
                    && look_ok(look)
                    && style.height.is_none_or(|h| h.is_finite() && h > 0.0)
                    && style.wrap.is_none_or(|w| w.is_finite() && w > 0.0)
                    && leaders.iter().all(|l| {
                        let to_ok = match l.leader.to {
                            Anchor::Free(p) => p.is_finite(),
                            Anchor::Point(_) => l.port.is_some(),
                            Anchor::Curve { t, .. } => l.port.is_some() && t.is_finite(),
                        };
                        let shape_ok = match l.leader.shape {
                            crate::LeaderShape::Straight => true,
                            crate::LeaderShape::Bent(b) => b.is_finite(),
                            crate::LeaderShape::Curved(k) => k.is_finite(),
                        };
                        to_ok
                            && shape_ok
                            && l.leader.shoulder_len.is_finite()
                            && l.leader.shoulder_len >= 0.0
                    })
            }
            SheetAnnotation::Dimension {
                measure,
                label,
                look,
                ..
            } => measure.is_dimension() && label.is_none_or(|l| l.is_finite()) && look_ok(look),
        }
    }

    /// What is left of it when some of what it is tied to has gone (`port` and `entity` say
    /// what is still there): a dimension goes with its viewport or any of its geometry
    /// (`None`), as a model dimension goes with its geometry; a note keeps its text where it
    /// is and loses only the leaders whose viewport or geometry went, as a model note does.
    pub fn surviving(
        &self,
        port: &dyn Fn(PortId) -> bool,
        entity: &dyn Fn(EntityId) -> bool,
    ) -> Option<SheetAnnotation> {
        match self {
            SheetAnnotation::Dimension {
                port: p, measure, ..
            } => (port(*p) && measure.entities().all(entity)).then(|| self.clone()),
            SheetAnnotation::Note {
                text,
                at,
                leaders,
                style,
                look,
            } => {
                let leaders = leaders
                    .iter()
                    .filter(|l| match l.port {
                        Some(p) => port(p) && l.leader.to.entity().is_none_or(entity),
                        None => true,
                    })
                    .copied()
                    .collect();
                Some(SheetAnnotation::Note {
                    text: text.clone(),
                    at: *at,
                    leaders,
                    style: *style,
                    look: *look,
                })
            }
        }
    }

    /// Point what was tied to entity `from` at `into` (two points merged).
    pub(crate) fn replace_entity(&mut self, from: EntityId, into: EntityId) {
        let swap = |e: EntityId| if e == from { into } else { e };
        match self {
            SheetAnnotation::Note { leaders, .. } => {
                for l in leaders.iter_mut().filter(|l| l.port.is_some()) {
                    match &mut l.leader.to {
                        Anchor::Point(e) | Anchor::Curve { curve: e, .. } => *e = swap(*e),
                        Anchor::Free(_) => {}
                    }
                }
            }
            SheetAnnotation::Dimension { measure, .. } => *measure = measure.map_entities(swap),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, Document, History, Sheet, SheetId, View, ViewId, Viewport};

    const V: fn(f64, f64) -> DVec2 = DVec2::new;

    /// A 100 × 50 rectangle with a circle in it, a view of it at 1:10 placed at (200, 150)
    /// on a sheet, and a second, empty sheet.
    struct Fixture {
        doc: Document,
        history: History,
        lines: Vec<EntityId>,
        points: Vec<EntityId>,
        circle: EntityId,
        view: ViewId,
        sheet: SheetId,
        other: SheetId,
        port: PortId,
    }

    fn fixture() -> Fixture {
        let mut doc = Document::new();
        let mut history = History::default();
        doc.begin();
        let points: Vec<EntityId> = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]
            .into_iter()
            .map(|(x, y)| doc.add_point(V(x, y)))
            .collect();
        let lines: Vec<EntityId> = (0..4)
            .map(|i| doc.add_line(points[i], points[(i + 1) % 4]).unwrap())
            .collect();
        let c = doc.add_point(V(50.0, 25.0));
        let circle = doc.add_circle(c, 10.0).unwrap();
        let view = doc.add_view(View::new(
            "Plan",
            Bounds {
                min: V(-50.0, -50.0),
                max: V(150.0, 100.0),
            },
            10.0,
        ));
        let mut s = Sheet::new("A-101");
        s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
        let sheet = doc.add_sheet(s);
        let other = doc.add_sheet(Sheet::new("A-102"));
        let port = doc.tie_viewport(sheet, 0).unwrap();
        history.push(doc.commit("Set up"));
        Fixture {
            doc,
            history,
            lines,
            points,
            circle,
            view,
            sheet,
            other,
            port,
        }
    }

    impl Fixture {
        fn step<R>(&mut self, f: impl FnOnce(&mut Document) -> R) -> R {
            self.doc.begin();
            let r = f(&mut self.doc);
            self.history.push(self.doc.commit("Step"));
            r
        }

        fn width(&mut self) -> SheetAnnotationId {
            let (sheet, port, a, b) = (self.sheet, self.port, self.points[0], self.points[1]);
            self.step(|doc| {
                doc.add_sheet_annotation(
                    sheet,
                    SheetAnnotation::dimension(
                        port,
                        Constraint::Distance { a, b, value: 0.0 },
                        Some(V(0.0, -8.0)),
                    ),
                )
                .unwrap()
            })
        }

        /// A note at (300, 200) with a leader to the bottom edge's middle through the view
        /// and one to the paper point (310, 180).
        fn note(&mut self) -> SheetAnnotationId {
            let (sheet, port, line) = (self.sheet, self.port, self.lines[0]);
            self.step(|doc| {
                doc.add_sheet_annotation(
                    sheet,
                    SheetAnnotation::Note {
                        text: "Edge".into(),
                        at: V(300.0, 200.0),
                        leaders: vec![
                            SheetLeader::through(
                                port,
                                Anchor::Curve {
                                    curve: line,
                                    t: 0.5,
                                },
                            ),
                            SheetLeader::on_paper(V(310.0, 180.0)),
                        ],
                        style: NoteStyle::default(),
                        look: StyleOverride::default(),
                    },
                )
                .unwrap()
            })
        }

        fn get(&self, id: SheetAnnotationId) -> Option<&SheetAnnotation> {
            self.doc.sheet_annotation(self.sheet, id)
        }

        fn leaders(&self, id: SheetAnnotationId) -> Vec<SheetLeader> {
            match self.get(id) {
                Some(SheetAnnotation::Note { leaders, .. }) => leaders.clone(),
                other => panic!("not a note: {other:?}"),
            }
        }

        fn text_at(&self, id: SheetAnnotationId) -> DVec2 {
            match self.get(id) {
                Some(SheetAnnotation::Note { at, .. }) => *at,
                other => panic!("not a note: {other:?}"),
            }
        }

        fn tip(&self, id: SheetAnnotationId, k: usize) -> Option<DVec2> {
            let sheet = self.doc.sheet(self.sheet).unwrap();
            self.doc.sheet_leader_tip(sheet, &self.leaders(id)[k])
        }
    }

    #[test]
    fn a_sheet_annotation_belongs_to_its_sheet_alone() {
        let mut f = fixture();
        let dim = f.width();
        let note = f.note();
        // On its sheet; not on the other, not in the model, and not a constraint (so the
        // solver, which reads constraints, never sees a sheet dimension).
        assert!(f.get(dim).is_some() && f.get(note).is_some());
        assert!(f.doc.sheet(f.other).unwrap().annotations.is_empty());
        assert!(f.doc.sheet_annotation(f.other, dim).is_none());
        assert_eq!(f.doc.annotations().count(), 0);
        assert_eq!(f.doc.constraints().count(), 0);
        assert_eq!(f.doc.measurements().count(), 0);
        // The dimension's unused value is kept as 1, whatever it was made with.
        assert!(matches!(
            f.get(dim),
            Some(SheetAnnotation::Dimension { measure, .. }) if measure.value() == Some(1.0)
        ));
        // One undo step each.
        assert!(f.history.undo(&mut f.doc));
        assert!(f.get(note).is_none() && f.get(dim).is_some());
        assert!(f.history.undo(&mut f.doc));
        assert!(f.get(dim).is_none());
        assert!(f.history.redo(&mut f.doc) && f.history.redo(&mut f.doc));
        assert!(f.get(dim).is_some() && f.get(note).is_some());
    }

    #[test]
    fn a_tied_tip_follows_the_geometry_the_view_and_its_scale_and_the_text_stays() {
        let mut f = fixture();
        let note = f.note();
        // The bottom edge's middle (50, 0) through a 1:10 view centred on (50, 25) at
        // (200, 150): 2.5 mm below the view's middle.
        assert_eq!(f.tip(note, 0), Some(V(200.0, 147.5)));
        assert_eq!(f.tip(note, 1), Some(V(310.0, 180.0)));
        // The geometry moves: the tip follows.
        let p = f.points[1];
        f.step(|doc| doc.set_point(p, V(200.0, 0.0)).unwrap());
        assert_eq!(f.tip(note, 0), Some(V(205.0, 147.5)));
        // The view moves on the sheet: the tip follows.
        let (sheet, view) = (f.sheet, f.view);
        f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            s.viewports[0].at = V(100.0, 100.0);
            doc.set_sheet(sheet, s);
        });
        assert_eq!(f.tip(note, 0), Some(V(105.0, 97.5)));
        // Its scale changes: the tip follows.
        f.step(|doc| {
            let mut v = doc.view(view).unwrap().clone();
            v.scale = 20.0;
            doc.set_view(view, v);
        });
        assert_eq!(f.tip(note, 0), Some(V(102.5, 98.75)));
        // Its crop is panned: the tip follows.
        f.step(|doc| {
            let mut v = doc.view(view).unwrap().clone();
            v.center = V(0.0, 0.0);
            doc.set_view(view, v);
        });
        assert_eq!(f.tip(note, 0), Some(V(105.0, 100.0)));
        // The text and the paper tip stayed where they were put through all of it.
        assert_eq!(f.text_at(note), V(300.0, 200.0));
        assert_eq!(f.tip(note, 1), Some(V(310.0, 180.0)));
    }

    #[test]
    fn deleting_the_geometry_takes_the_dimension_and_the_leader_and_undo_brings_them_back() {
        let mut f = fixture();
        let dim = f.width();
        let note = f.note();
        let before = f.doc.clone();
        // The bottom edge and its ends go.
        let line = f.lines[0];
        f.step(|doc| doc.delete([line]));
        assert!(f.get(dim).is_some(), "its points are still there");
        let point = f.points[0];
        f.step(|doc| doc.delete([point]));
        assert!(
            f.get(dim).is_none(),
            "a dimension goes with what it measures"
        );
        // The note keeps its text where it was and its paper leader; the tied one is gone.
        assert_eq!(f.text_at(note), V(300.0, 200.0));
        assert_eq!(f.leaders(note), [SheetLeader::on_paper(V(310.0, 180.0))]);
        // Nothing left on the sheet names anything that is gone.
        let sheet = f.doc.sheet(f.sheet).unwrap();
        for a in sheet.annotations.values() {
            assert!(a.entities().iter().all(|e| f.doc.get(*e).is_some()));
        }
        // One undo per delete: both back as they were.
        assert!(f.history.undo(&mut f.doc) && f.history.undo(&mut f.doc));
        assert_eq!(f.doc, before);
        assert!(f.history.redo(&mut f.doc) && f.history.redo(&mut f.doc));
        assert!(f.get(dim).is_none());
        assert_eq!(f.leaders(note).len(), 1);
    }

    #[test]
    fn deleted_and_remade_geometry_is_new_geometry() {
        let mut f = fixture();
        let dim = f.width();
        let (a, b) = (f.points[0], f.points[1]);
        f.step(|doc| doc.delete([a, b]));
        assert!(f.get(dim).is_none());
        // Drawn again in the same place: other entities, and the dimension stays gone.
        f.step(|doc| {
            let a = doc.add_point(V(0.0, 0.0));
            let b = doc.add_point(V(100.0, 0.0));
            doc.add_line(a, b).unwrap();
        });
        assert!(f.doc.sheet(f.sheet).unwrap().annotations.is_empty());
        // Undoing both brings the very dimension back.
        assert!(f.history.undo(&mut f.doc) && f.history.undo(&mut f.doc));
        assert!(f.get(dim).is_some());
    }

    #[test]
    fn taking_the_view_off_the_sheet_or_deleting_it_takes_what_is_tied_to_it() {
        for delete_view in [false, true] {
            let mut f = fixture();
            let dim = f.width();
            let note = f.note();
            let before = f.doc.clone();
            let (sheet, view) = (f.sheet, f.view);
            f.step(|doc| {
                if delete_view {
                    doc.remove_view(view);
                } else {
                    let mut s = doc.sheet(sheet).unwrap().clone();
                    s.viewports.clear();
                    doc.set_sheet(sheet, s);
                }
            });
            assert!(f.get(dim).is_none(), "delete_view {delete_view}");
            assert_eq!(f.leaders(note), [SheetLeader::on_paper(V(310.0, 180.0))]);
            assert_eq!(f.text_at(note), V(300.0, 200.0));
            // In the same step: one undo brings the view, the dimension and the leader back.
            assert!(f.history.undo(&mut f.doc));
            assert_eq!(f.doc, before, "delete_view {delete_view}");
            assert!(f.history.redo(&mut f.doc));
            assert!(f.get(dim).is_none());
        }
    }

    #[test]
    fn the_same_view_placed_twice_keeps_each_annotation_with_its_own_viewport() {
        let mut f = fixture();
        let (sheet, view) = (f.sheet, f.view);
        // A second viewport of the same view, and a dimension through it.
        let second = f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            s.viewports.push(Viewport::new(view, V(350.0, 150.0)));
            doc.set_sheet(sheet, s);
            doc.tie_viewport(sheet, 1).unwrap()
        });
        assert_ne!(second, f.port);
        let (a, b) = (f.points[0], f.points[1]);
        let on_second = f.step(|doc| {
            doc.add_sheet_annotation(
                sheet,
                SheetAnnotation::dimension(
                    second,
                    Constraint::HorizontalDistance { a, b, value: 0.0 },
                    None,
                ),
            )
            .unwrap()
        });
        let on_first = f.width();
        // The first viewport goes: only what was tied to it does.
        f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            s.viewports.remove(0);
            doc.set_sheet(sheet, s);
        });
        assert!(f.get(on_first).is_none());
        assert!(f.get(on_second).is_some());
        let s = f.doc.sheet(sheet).unwrap();
        assert_eq!(
            f.doc.port_view(s, second).unwrap().frame.at,
            V(350.0, 150.0)
        );
        assert!(f.doc.port_view(s, f.port).is_none());
    }

    #[test]
    fn a_copied_sheet_gets_its_own_annotations_and_a_moved_sheet_keeps_its_own() {
        let mut f = fixture();
        let dim = f.width();
        let note = f.note();
        let (sheet, view) = (f.sheet, f.view);
        // A copy with copies of its views (as Duplicate sheet makes one).
        let copy = f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            for p in &mut s.viewports {
                let v = doc.view(p.view).unwrap().clone();
                p.view = doc.add_view(v);
            }
            doc.add_sheet(s)
        });
        let c = f.doc.sheet(copy).unwrap().clone();
        assert_eq!(c.annotations.len(), 2);
        // Under ids of their own, tied to the copy's own viewport, which shows the copy view.
        assert!(!c.annotations.contains_key(&dim) && !c.annotations.contains_key(&note));
        let port = c.viewports[0].id.unwrap();
        assert_ne!(port, f.port);
        assert_ne!(c.viewports[0].view, view);
        for a in c.annotations.values() {
            assert!(a.ports().iter().all(|p| *p == port), "{a:?}");
        }
        // The first sheet's view goes: the copy keeps everything.
        f.step(|doc| doc.remove_view(view));
        assert!(f.get(dim).is_none());
        assert_eq!(f.doc.sheet(copy).unwrap().annotations.len(), 2);
        // A copy without views: only what is on the paper alone comes along.
        let bare = f.step(|doc| {
            let mut s = doc.sheet(copy).unwrap().clone();
            s.viewports.clear();
            doc.add_sheet(s)
        });
        let kept: Vec<_> = f.doc.sheet(bare).unwrap().annotations.values().collect();
        assert_eq!(kept.len(), 1);
        assert!(matches!(
            kept[0],
            SheetAnnotation::Note { leaders, .. }
                if leaders[..] == [SheetLeader::on_paper(V(310.0, 180.0))]
        ));
        // Reordering sheets moves their contents between ids: the annotations go along.
        let was = f.doc.sheet(copy).unwrap().clone();
        let went = f.step(|doc| doc.move_sheet(copy, 0));
        assert_eq!(f.doc.sheet(went[&copy]).unwrap(), &was);
    }

    #[test]
    fn merged_points_carry_what_is_tied_to_them() {
        let mut f = fixture();
        let dim = f.width();
        let (sheet, port, corner) = (f.sheet, f.port, f.points[2]);
        let note = f.step(|doc| {
            doc.add_sheet_annotation(
                sheet,
                SheetAnnotation::Note {
                    text: "Corner".into(),
                    at: V(250.0, 250.0),
                    leaders: vec![SheetLeader::through(port, Anchor::Point(corner))],
                    style: NoteStyle::default(),
                    look: StyleOverride::default(),
                },
            )
            .unwrap()
        });
        // A loose point merged with the corner, either way round: still tied.
        let loose = f.step(|doc| doc.add_point(V(100.0, 50.0)));
        f.step(|doc| doc.merge_points(corner, loose).unwrap());
        assert_eq!(f.leaders(note)[0].leader.to, Anchor::Point(loose));
        assert_eq!(f.tip(note, 0), Some(V(205.0, 152.5)));
        // The dimension's two ends merged into one: it measures nothing, and goes.
        let (a, b, bottom) = (f.points[0], f.points[1], f.lines[0]);
        f.step(|doc| {
            // (A degenerate bottom edge can't be merged: take it out first.)
            let s = doc.sheet(sheet).unwrap().clone();
            assert!(s.annotations.contains_key(&dim));
            doc.delete([bottom]);
        });
        if f.doc.get(a).is_some() && f.doc.get(b).is_some() {
            f.step(|doc| doc.merge_points(a, b).unwrap());
            assert!(f.get(dim).is_none());
        }
    }

    #[test]
    fn what_cannot_be_tied_is_refused() {
        let mut f = fixture();
        let (sheet, other, port) = (f.sheet, f.other, f.port);
        let (a, b, line, circle) = (f.points[0], f.points[1], f.lines[0], f.circle);
        let distance = Constraint::Distance { a, b, value: 0.0 };
        let dim = |port, c| SheetAnnotation::dimension(port, c, None);
        let before = f.doc.clone();
        // A viewport that isn't this sheet's (none, or another sheet's).
        assert!(
            f.doc
                .add_sheet_annotation(sheet, dim(PortId(999), distance))
                .is_err()
        );
        assert!(
            f.doc
                .add_sheet_annotation(other, dim(port, distance))
                .is_err()
        );
        // A relation, not a dimension; a radius of a line; geometry that isn't there.
        assert!(
            f.doc
                .add_sheet_annotation(sheet, dim(port, Constraint::Horizontal { line }))
                .is_err()
        );
        let radius = Constraint::Radius {
            curve: line,
            value: 0.0,
        };
        assert!(
            f.doc
                .add_sheet_annotation(sheet, dim(port, radius))
                .is_err()
        );
        let gone = Constraint::Distance {
            a,
            b: EntityId(9999),
            value: 0.0,
        };
        assert!(f.doc.add_sheet_annotation(sheet, dim(port, gone)).is_err());
        // A leader to geometry without a viewport to see it through; a place that isn't one.
        let mut note = SheetAnnotation::note("x", V(10.0, 10.0));
        if let SheetAnnotation::Note { leaders, .. } = &mut note {
            leaders.push(SheetLeader {
                leader: Leader::to(Anchor::Point(a)),
                port: None,
            });
        }
        assert!(f.doc.add_sheet_annotation(sheet, note).is_err());
        assert!(
            f.doc
                .add_sheet_annotation(sheet, SheetAnnotation::note("x", V(f64::NAN, 0.0)))
                .is_err()
        );
        assert!(
            f.doc
                .add_sheet_annotation(SheetId(4242), SheetAnnotation::note("x", V(1.0, 1.0)))
                .is_err()
        );
        assert_eq!(f.doc, before, "a refused annotation changes nothing");
        // What can be: a radius of the circle.
        let radius = Constraint::Radius {
            curve: circle,
            value: 0.0,
        };
        assert!(f.doc.add_sheet_annotation(sheet, dim(port, radius)).is_ok());
    }

    #[test]
    fn a_sheet_handed_over_inconsistent_is_settled() {
        let mut f = fixture();
        let dim = f.width();
        let (sheet, view) = (f.sheet, f.view);
        // Two viewports sharing an id (a careless copy): the later one loses it, so the
        // dimension stays with the first.
        f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            let mut twin = s.viewports[0].clone();
            twin.at = V(50.0, 50.0);
            twin.view = view;
            s.viewports.push(twin);
            doc.set_sheet(sheet, s);
        });
        let s = f.doc.sheet(sheet).unwrap();
        assert_eq!(s.viewports[0].id, Some(f.port));
        assert_eq!(s.viewports[1].id, None);
        assert!(f.get(dim).is_some());
        // An annotation slipped in naming a viewport that isn't there is dropped on the way in.
        let (a, b) = (f.points[0], f.points[1]);
        f.step(|doc| {
            let mut s = doc.sheet(sheet).unwrap().clone();
            let stray = SheetAnnotation::dimension(
                PortId(777),
                Constraint::Distance { a, b, value: 1.0 },
                None,
            );
            s.annotations.insert(SheetAnnotationId(5000), stray);
            doc.set_sheet(sheet, s);
        });
        assert!(f.get(SheetAnnotationId(5000)).is_none());
        assert!(f.get(dim).is_some());
    }
}
