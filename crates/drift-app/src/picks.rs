//! Picking several kinds of things at once: what a selection box takes besides geometry
//! (sketch text here; dimensions and annotations in the app, which lays them out on screen),
//! and the text and annotations that move, turn, copy and mirror with the picked geometry.

use drift_core::{
    Annotation, AnnotationId, AnnotationRef, Bounds, ConstraintId, DVec2, EntityId, SketchText,
    TextId,
};
use eframe::egui::{self, Pos2, Rect};

use crate::annotations::{DimLayout, DimShape};
use crate::editor::Editor;
use crate::notes::{Mark, NoteLayout};

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
                let label = crate::notes::label_world(&self.doc, &a);
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
                && let Some(placed) = crate::notes::with_label_at(&self.doc, &moved, f(*label))
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

/// A piece of something drawn on screen, for testing it against a selection box.
#[derive(Clone, Debug)]
enum Piece {
    Rect(Rect),
    /// A polyline (a closed shape repeats its first point).
    Line(Vec<Pos2>),
}

fn segment_touches(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    // Liang–Barsky: clip the segment to the box.
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl Piece {
    fn inside(&self, r: Rect) -> bool {
        match self {
            Piece::Rect(p) => r.contains_rect(*p),
            Piece::Line(points) => points.iter().all(|p| r.contains(*p)),
        }
    }

    fn touches(&self, r: Rect) -> bool {
        match self {
            Piece::Rect(p) => r.intersects(*p),
            Piece::Line(points) => match points.as_slice() {
                [p] => r.contains(*p),
                _ => points.windows(2).any(|w| segment_touches(w[0], w[1], r)),
            },
        }
    }
}

/// Whether pieces are in a box: all inside it (a window), or any touching it (crossing).
fn in_box(pieces: &[Piece], r: Rect, crossing: bool) -> bool {
    !pieces.is_empty()
        && if crossing {
            pieces.iter().any(|p| p.touches(r))
        } else {
            pieces.iter().all(|p| p.inside(r))
        }
}

/// Whether a dimension as drawn is in a box (on screen): its label, dimension and extension
/// lines all inside it, or (crossing) any of them touching it.
pub(crate) fn dimension_in_box(d: &DimLayout, r: Rect, crossing: bool) -> bool {
    let mut pieces = vec![Piece::Rect(d.text_rect)];
    match &d.shape {
        DimShape::Linear { d: ends, .. } => {
            for i in 0..2 {
                if let Some(line) = d.shape.extension_line(i) {
                    pieces.push(Piece::Line(line.to_vec()));
                }
            }
            pieces.push(Piece::Line(ends.to_vec()));
        }
        DimShape::Radius { tip, from, .. } => pieces.push(Piece::Line(vec![*from, *tip])),
        DimShape::Diameter { ends } => pieces.push(Piece::Line(ends.to_vec())),
        DimShape::Angular {
            extensions,
            center,
            radius,
            start,
            sweep,
        } => {
            for e in extensions.iter().flatten() {
                pieces.push(Piece::Line(e.to_vec()));
            }
            let n = 24;
            let arc = (0..=n)
                .map(|i| {
                    let a = start + sweep * i as f32 / n as f32;
                    *center + egui::vec2(a.cos(), a.sin()) * *radius
                })
                .collect();
            pieces.push(Piece::Line(arc));
        }
    }
    in_box(&pieces, r, crossing)
}

/// Whether an annotation as drawn is in a box (on screen): its text, leaders and lines all
/// inside it, or (crossing) any of them touching it.
pub(crate) fn note_in_box(l: &NoteLayout, r: Rect, crossing: bool) -> bool {
    let mut pieces = Vec::new();
    if let Some(t) = &l.text {
        pieces.push(Piece::Rect(t.rect));
    }
    for leader in &l.leaders {
        pieces.push(Piece::Line(leader.points.clone()));
    }
    for [a, b] in &l.segments {
        pieces.push(Piece::Line(vec![*a, *b]));
    }
    for m in &l.marks {
        match m {
            Mark::Stroke(points) => pieces.push(Piece::Line(points.clone())),
            Mark::Solid(points) | Mark::Blank(points) => {
                let mut ring = points.clone();
                ring.extend(points.first().copied());
                pieces.push(Piece::Line(ring));
            }
            Mark::Text(t, _) => pieces.push(Piece::Rect(t.rect)),
        }
    }
    in_box(&pieces, r, crossing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_in_a_window_or_crossing_box() {
        let r = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0));
        let line = |a: (f32, f32), b: (f32, f32)| {
            Piece::Line(vec![egui::pos2(a.0, a.1), egui::pos2(b.0, b.1)])
        };
        // Through the box without an end in it: crossing only.
        let across = [line((-5.0, 5.0), (15.0, 5.0))];
        assert!(in_box(&across, r, true));
        assert!(!in_box(&across, r, false));
        // Past a corner: neither.
        let past = [line((-5.0, 4.0), (4.0, -5.0))];
        assert!(!in_box(&past, r, true));
        // All inside: both.
        let inside = [
            line((1.0, 1.0), (9.0, 9.0)),
            Piece::Rect(Rect::from_min_max(
                egui::pos2(2.0, 2.0),
                egui::pos2(4.0, 3.0),
            )),
        ];
        assert!(in_box(&inside, r, false) && in_box(&inside, r, true));
        assert!(!in_box(&[], r, true));
    }
}
