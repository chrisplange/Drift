//! Whether dimensions and annotations as drawn on screen are in a selection box. The Editor
//! side of box picking (`BoxPick`, the riders) is `drift_edit::picks`; this part tests the
//! app's laid-out labels, which are made with egui.

use eframe::egui::{self, Pos2, Rect};

use crate::annotations::{DimLayout, DimShape};
use crate::notes::{Mark, NoteLayout};

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
    let f = |p: Pos2| drift_core::DVec2::new(f64::from(p.x), f64::from(p.y));
    let rect = drift_core::Bounds {
        min: f(r.min),
        max: f(r.max),
    };
    rect.clip_segment(f(a), f(b)).is_some()
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
