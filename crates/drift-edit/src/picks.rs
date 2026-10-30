//! Picking several kinds of things at once: what a selection box takes besides geometry
//! (sketch text here; dimensions and annotations in the app, which lays them out on screen),
//! and the text and annotations that move, turn, copy and mirror with the picked geometry.

use crate::editor::Editor;
use drift_core::{
    Annotation, AnnotationId, AnnotationRef, Bounds, ConstraintId, DVec2, EntityId, SketchText,
    TextId,
};

/// A selection box just made: its corners (world), whether it is a crossing box (dragged
/// right to left), and whether it adds to what was picked (Shift or Ctrl).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxPick {
    pub rect: Bounds,
    pub crossing: bool,
    pub add: bool,
}

/// Sketch text and annotations picked with the geometry, as they were when a move, turn or
/// mirror began.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Riders {
    /// Each text, and the middle of its outline.
    pub texts: Vec<(TextId, SketchText, DVec2)>,
    /// Each annotation, and where its label was (world).
    pub notes: Vec<(AnnotationId, Annotation, Option<DVec2>)>,
}

impl Riders {
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty() && self.notes.is_empty()
    }

    /// Where the riders alone turn about: a lone text's anchor, or the middle of them all.
    pub fn centre(&self) -> Option<DVec2> {
        if let ([(_, t, _)], []) = (self.texts.as_slice(), self.notes.as_slice()) {
            return Some(t.at);
        }
        let mut b = Bounds::EMPTY;
        for (_, _, c) in &self.texts {
            b.include(*c);
        }
        for p in self.notes.iter().filter_map(|n| n.2) {
            b.include(p);
        }
        (!b.is_empty()).then(|| b.center())
    }

    /// Where a move of the riders alone goes from: the first text's anchor, or the first
    /// note's label.
    pub fn base(&self) -> Option<DVec2> {
        self.texts
            .first()
            .map(|t| t.1.at)
            .or_else(|| self.notes.iter().find_map(|n| n.2))
    }
}

/// The angle (degrees) a direction at `angle` turns to under `f`, from `at`.
fn mapped_angle(f: &dyn Fn(DVec2) -> DVec2, at: DVec2, angle: f64) -> f64 {
    let d = f(at + DVec2::from_angle(angle.to_radians())) - f(at);
    d.y.atan2(d.x).to_degrees().rem_euclid(360.0)
}

impl Editor {
    /// The annotations picked (not dimensions): the primary one and those added to it.
    pub fn picked_notes(&self) -> Vec<AnnotationId> {
        let mut out: Vec<AnnotationId> = self.selected_annotation.into_iter().collect();
        for r in &self.annotation_picks {
            if let AnnotationRef::Annotation(id) = r
                && !out.contains(id)
            {
                out.push(*id);
            }
        }
        out.retain(|id| self.doc.annotation(*id).is_some());
        out
    }

    /// The dimensions added to the pick (with Shift or a box), besides the app's own.
    pub fn picked_dimensions(&self) -> Vec<ConstraintId> {
        self.annotation_picks
            .iter()
            .filter_map(|r| match r {
                AnnotationRef::Dimension(id) => Some(*id),
                AnnotationRef::Annotation(_) => None,
            })
            .filter(|id| self.doc.constraint(*id).is_some())
            .collect()
    }

    /// The picked text and annotations, as they are now.
    pub fn riders(&self) -> Riders {
        let texts = self
            .selected_texts
            .iter()
            .filter_map(|id| {
                let t = self.doc.text(*id)?.clone();
                let centre = self
                    .text_outline(*id)
                    .map_or(t.at, |o| (o.bounds.min + o.bounds.max) * 0.5);
                Some((*id, t, centre))
            })
            .collect();
        let notes = self
            .picked_notes()
            .into_iter()
            .filter_map(|id| {
                let a = self.doc.annotation(id)?.clone();
                let label = crate::labels::label_world(&self.doc, &a);
                Some((id, a, label))
            })
            .collect();
        Riders { texts, notes }
    }

    /// Put the riders where the affine map `f` takes them from where they were, inside an
    /// open transaction (after the geometry has moved). A note's text lands where `f` takes
    /// it, whatever its leaders' geometry did; callouts follow their geometry.
    pub fn place_riders(&mut self, riders: &Riders, f: &dyn Fn(DVec2) -> DVec2) {
        for (id, t, _) in &riders.texts {
            let mut moved = t.clone();
            moved.at = f(t.at);
            moved.angle = mapped_angle(f, t.at, t.angle);
            self.doc.set_text(*id, moved);
        }
        for (id, a, label) in &riders.notes {
            let mut moved = a.mapped(f);
            if let (Annotation::Note { .. }, Some(label)) = (a, label)
                && let Some(placed) = crate::labels::with_label_at(&self.doc, &moved, f(*label))
            {
                moved = placed;
            }
            let _ = self.doc.set_annotation(*id, moved);
        }
    }

    /// Copies of the riders where `f` takes them, on the geometry `map` gives for each
    /// entity (inside an open transaction). Mirrored text still reads forwards: it keeps its
    /// angle, its middle landing where `f` takes it. Returns the copies.
    pub fn copy_riders(
        &mut self,
        riders: &Riders,
        map: &dyn Fn(EntityId) -> Option<EntityId>,
        f: &dyn Fn(DVec2) -> DVec2,
    ) -> (Vec<TextId>, Vec<AnnotationId>) {
        let o = f(DVec2::ZERO);
        let mirrored = (f(DVec2::X) - o).perp_dot(f(DVec2::Y) - o) < 0.0;
        let mut texts = Vec::new();
        for (_, t, centre) in &riders.texts {
            let mut copy = t.clone();
            if mirrored {
                copy.at = f(*centre) + (t.at - *centre);
            } else {
                copy.at = f(t.at);
                copy.angle = mapped_angle(f, t.at, t.angle);
            }
            let id = self.doc.add_text(copy.clone());
            self.doc.set_text(id, copy);
            texts.push(id);
        }
        let mut notes = Vec::new();
        for (id, a, _) in &riders.notes {
            let doc = &self.doc;
            let Some(on) = a.with_entities(map, &|anchor| doc.anchor_pos(anchor)) else {
                continue;
            };
            let mut copy = on.mapped(f);
            // A copied mark is a new mark: it isn't the view's.
            copy.set_view(None);
            let shown = self.doc.shown(AnnotationRef::Annotation(*id));
            let look = self.doc.style_override(AnnotationRef::Annotation(*id));
            if let Ok(new) = self.doc.add_annotation(copy) {
                self.doc.set_shown(AnnotationRef::Annotation(new), shown);
                self.doc
                    .set_style_override(AnnotationRef::Annotation(new), look);
                notes.push(new);
            }
        }
        (texts, notes)
    }

    /// The sketch texts in a box (world): all of their outline inside it, or (crossing) any
    /// of it. Text on hidden or locked layers is left out.
    pub fn texts_in(&self, rect: &Bounds, crossing: bool) -> Vec<TextId> {
        self.doc
            .texts()
            .filter(|(_, t)| {
                self.doc
                    .layer(t.layer)
                    .is_some_and(|l| l.visible && !l.locked)
            })
            .filter_map(|(id, _)| {
                let b = self.text_outline(id)?.bounds;
                let inside = b.min.x >= rect.min.x
                    && b.min.y >= rect.min.y
                    && b.max.x <= rect.max.x
                    && b.max.y <= rect.max.y;
                let touches = b.min.x <= rect.max.x
                    && b.max.x >= rect.min.x
                    && b.min.y <= rect.max.y
                    && b.max.y >= rect.min.y;
                (if crossing { touches } else { inside }).then_some(id)
            })
            .collect()
    }

    /// The one text picked, when it is the only one.
    pub fn sole_text(&self) -> Option<TextId> {
        match self.selected_texts.len() {
            1 => self.selected_texts.first().copied(),
            _ => None,
        }
    }

    /// Whether anything besides geometry is picked: text or annotations.
    pub fn picks_annotations(&self) -> bool {
        !self.selected_texts.is_empty()
            || self.selected_annotation.is_some()
            || !self.annotation_picks.is_empty()
    }
}
