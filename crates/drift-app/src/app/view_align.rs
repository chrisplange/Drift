//! Lining views up on a sheet: smart guides while a view is dragged (its frame's edges and
//! middle to other frames' and to the drawing area's), drawings at the same scale lining
//! up (so the floors of side-by-side elevations meet), and align commands.

use drift_core::{DVec2, Sheet, SheetView, ViewFrame};

use super::sheets_ui::{ViewGrip, dragged_view};

/// A line a view snapped to, on the paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AlignGuide {
    /// Up the page (x is fixed), or across it.
    pub vertical: bool,
    /// Where on the paper (mm).
    pub at: f64,
    /// The drawings line up here (rather than frames' edges).
    pub drawings: bool,
}

/// How to align a view with another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AlignTo {
    Left,
    Centre,
    Right,
    Top,
    Middle,
    Bottom,
    /// The drawings level with each other (the same height of the model at the same height
    /// on the paper): side by side.
    DrawingsAcross,
    /// The drawings one above the other, the same x of the model at the same x.
    DrawingsDown,
}

impl AlignTo {
    pub const FRAMES: [(AlignTo, &'static str, &'static str); 6] = [
        (AlignTo::Left, "Left", "Left edges in line"),
        (
            AlignTo::Centre,
            "Centre",
            "Middles in line, one above the other",
        ),
        (AlignTo::Right, "Right", "Right edges in line"),
        (AlignTo::Top, "Top", "Top edges level"),
        (AlignTo::Middle, "Middle", "Middles level, side by side"),
        (AlignTo::Bottom, "Bottom", "Bottom edges level"),
    ];
}

/// Where a view's drawing sits on the paper: the paper point of the model's origin.
fn offset(sheet: &Sheet, v: &SheetView) -> DVec2 {
    drift_draft::sheet::view_frame(sheet, v).center() - v.center / v.scale
}

fn same_scale(a: &SheetView, b: &SheetView) -> bool {
    (a.scale - b.scale).abs() <= 1e-9 * a.scale.max(1.0)
}

/// Snap a drag of view `index` (as it `was`) by `paper` mm: whichever of its frame's edges
/// and middle (or, panning, its drawing) comes within `tol` of another view's, or of the
/// drawing area's, goes to it. Returns the drag snapped, and the lines it snapped to.
pub(crate) fn snap_drag(
    sheet: &Sheet,
    index: usize,
    was: &SheetView,
    grip: ViewGrip,
    paper: DVec2,
    tol: f64,
) -> (DVec2, Vec<AlignGuide>) {
    let moved = dragged_view(was, grip, paper);
    let frame = drift_draft::sheet::view_frame(sheet, &moved);
    let mine = offset(sheet, &moved);
    let area = drift_draft::sheet::drawing_area(sheet);
    let others: Vec<&SheetView> = sheet
        .views
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, v)| v)
        .collect();
    let framed = was.frame.is_some();
    let mut paper = paper;
    let mut guides = Vec::new();
    for k in 0..2 {
        let side = |b: drift_core::Bounds| [b.min[k], b.center()[k], b.max[k]];
        // What moves on this axis: frame lines, and the drawing.
        let mut lines: Vec<f64> = Vec::new();
        let mut drawing = false;
        match grip {
            ViewGrip::Move if framed => {
                lines.extend(side(frame));
                drawing = true;
            }
            ViewGrip::Move | ViewGrip::Pan => drawing = true,
            ViewGrip::Resize(dx, dy) => {
                let d = if k == 0 { dx } else { dy };
                match d {
                    -1 => lines.push(frame.min[k]),
                    1 => lines.push(frame.max[k]),
                    _ => {}
                }
            }
        }
        // What it can go to: other frames and the drawing area; other drawings at its scale.
        let mut best: Option<(f64, AlignGuide)> = None;
        // Drawings lining up count as twice as near as frame lines: they're what matters
        // most between views at one scale (frames line up with the align buttons too).
        let reach = |shift: f64, g: &AlignGuide| shift.abs() * if g.drawings { 0.5 } else { 1.0 };
        let mut consider = |shift: f64, guide: AlignGuide| {
            if shift.abs() <= tol && best.is_none_or(|(s, g)| reach(shift, &guide) < reach(s, &g)) {
                best = Some((shift, guide));
            }
        };
        let mut targets: Vec<f64> = side(area).to_vec();
        for o in &others {
            if o.frame.is_some() {
                targets.extend(side(drift_draft::sheet::view_frame(sheet, o)));
            }
        }
        // Drawings first: where a frame and a drawing would line up alike, the drawing's
        // guide is the one shown.
        if drawing {
            for o in others.iter().filter(|o| same_scale(o, &moved)) {
                let theirs = offset(sheet, o);
                consider(
                    theirs[k] - mine[k],
                    AlignGuide {
                        vertical: k == 0,
                        // Through the other view's middle: the model is the same there.
                        at: drift_draft::sheet::view_frame(sheet, o).center()[k],
                        drawings: true,
                    },
                );
            }
        }
        for m in &lines {
            for t in &targets {
                consider(
                    t - m,
                    AlignGuide {
                        vertical: k == 0,
                        at: *t,
                        drawings: false,
                    },
                );
            }
        }
        if let Some((shift, guide)) = best {
            paper[k] += shift;
            guides.push(guide);
        }
    }
    (paper, guides)
}

/// Align view `index` with view `other`. Frames move (and their drawings with them); a view
/// that fills the sheet slides its drawing instead when lining drawings up. False if it
/// can't (frames for a view that fills the sheet; drawings at different scales).
pub(crate) fn align_view(sheet: &mut Sheet, index: usize, other: usize, how: AlignTo) -> bool {
    if index == other || index >= sheet.views.len() || other >= sheet.views.len() {
        return false;
    }
    let target = sheet.views[other].clone();
    let view = sheet.views[index].clone();
    let (theirs, mine) = (
        drift_draft::sheet::view_frame(sheet, &target),
        drift_draft::sheet::view_frame(sheet, &view),
    );
    let shift = match how {
        AlignTo::Left => DVec2::new(theirs.min.x - mine.min.x, 0.0),
        AlignTo::Centre => DVec2::new(theirs.center().x - mine.center().x, 0.0),
        AlignTo::Right => DVec2::new(theirs.max.x - mine.max.x, 0.0),
        AlignTo::Top => DVec2::new(0.0, theirs.max.y - mine.max.y),
        AlignTo::Middle => DVec2::new(0.0, theirs.center().y - mine.center().y),
        AlignTo::Bottom => DVec2::new(0.0, theirs.min.y - mine.min.y),
        AlignTo::DrawingsAcross | AlignTo::DrawingsDown => {
            if !same_scale(&view, &target) {
                return false;
            }
            let d = offset(sheet, &target) - offset(sheet, &view);
            if how == AlignTo::DrawingsAcross {
                DVec2::new(0.0, d.y)
            } else {
                DVec2::new(d.x, 0.0)
            }
        }
    };
    let drawings = matches!(how, AlignTo::DrawingsAcross | AlignTo::DrawingsDown);
    let v = &mut sheet.views[index];
    match v.frame {
        Some(f) => {
            v.frame = Some(ViewFrame {
                at: f.at + shift,
                ..f
            });
        }
        // A view that fills the sheet has no frame to move: its drawing slides.
        None if drawings => v.center -= shift * v.scale,
        None => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Two framed views at 1 : 24 (two elevations), side by side and not quite level.
    fn two_elevations() -> Sheet {
        let mut sheet = Sheet::new("A-201");
        let mut a = SheetView::new(24.0, v(0.0, 1000.0));
        a.frame = Some(ViewFrame {
            at: v(150.0, 200.0),
            size: v(200.0, 120.0),
        });
        let mut b = SheetView::new(24.0, v(5000.0, 1000.0));
        b.frame = Some(ViewFrame {
            at: v(420.0, 203.0),
            size: v(150.0, 100.0),
        });
        sheet.views = vec![a, b];
        sheet
    }

    #[test]
    fn dragging_snaps_to_edges_and_lines_drawings_up() {
        let sheet = two_elevations();
        let was = sheet.views[1].clone();
        // Moving view 2 down by 2.5: its drawing is then 0.5 off the first's: it snaps level.
        let (snapped, guides) = snap_drag(&sheet, 1, &was, ViewGrip::Move, v(0.0, -2.5), 1.0);
        assert!((snapped.y + 3.0).abs() < 1e-9, "{snapped:?}");
        assert!(guides.iter().any(|g| g.drawings && !g.vertical));
        let mut moved = sheet.clone();
        moved.views[1] = dragged_view(&was, ViewGrip::Move, snapped);
        assert!(
            (offset(&moved, &moved.views[1]).y - offset(&moved, &moved.views[0]).y).abs() < 1e-9
        );
        // Its left edge near the first's right edge: it goes to it.
        let (snapped, guides) = snap_drag(&sheet, 1, &was, ViewGrip::Move, v(-94.6, 40.0), 1.0);
        let left = 420.0 - 75.0 + snapped.x;
        assert!((left - 250.0).abs() < 1e-9, "{snapped:?}");
        assert!(
            guides
                .iter()
                .any(|g| g.vertical && (g.at - 250.0).abs() < 1e-9)
        );
        // Far from anything: as dragged.
        let (snapped, guides) = snap_drag(&sheet, 1, &was, ViewGrip::Move, v(13.3, 31.7), 1.0);
        assert_eq!((snapped, guides.len()), (v(13.3, 31.7), 0));
        // Resizing: the edge being dragged snaps; the others don't move.
        let (snapped, _) = snap_drag(&sheet, 1, &was, ViewGrip::Resize(0, 1), v(0.0, 6.5), 1.0);
        let top = 203.0 + 50.0 + snapped.y;
        assert!((top - 260.0).abs() < 1e-9, "the first view's top: {top}");
        // At another scale, drawings don't line up (frames still do).
        let mut other = sheet.clone();
        other.views[1].scale = 48.0;
        let (snapped, guides) = snap_drag(
            &other,
            1,
            &other.views[1].clone(),
            ViewGrip::Move,
            v(0.0, -2.5),
            1.0,
        );
        assert!(!guides.iter().any(|g| g.drawings));
        assert_eq!(snapped.y, -3.0, "the frames' middles still level");
    }

    #[test]
    fn align_commands_move_frames_and_line_drawings_up() {
        let mut sheet = two_elevations();
        assert!(align_view(&mut sheet, 1, 0, AlignTo::Top));
        let top = |s: &Sheet, i: usize| drift_draft::sheet::view_frame(s, &s.views[i]).max.y;
        assert!((top(&sheet, 1) - top(&sheet, 0)).abs() < 1e-9);
        assert!(align_view(&mut sheet, 1, 0, AlignTo::DrawingsAcross));
        assert!(
            (offset(&sheet, &sheet.views[1]).y - offset(&sheet, &sheet.views[0]).y).abs() < 1e-9
        );
        // Its x is untouched.
        assert_eq!(sheet.views[1].frame.unwrap().at.x, 420.0);
        assert!(align_view(&mut sheet, 1, 0, AlignTo::Left));
        assert_eq!(sheet.views[1].frame.unwrap().at.x, 50.0 + 75.0);
        // Drawings at different scales can't line up; a view filling the sheet has no frame
        // to align, but slides its drawing to line up.
        sheet.views[1].scale = 48.0;
        assert!(!align_view(&mut sheet, 1, 0, AlignTo::DrawingsDown));
        let mut whole = two_elevations();
        whole.views[1].frame = None;
        assert!(!align_view(&mut whole, 1, 0, AlignTo::Left));
        assert!(align_view(&mut whole, 1, 0, AlignTo::DrawingsAcross));
        assert!(
            (offset(&whole, &whole.views[1]).y - offset(&whole, &whole.views[0]).y).abs() < 1e-9
        );
        assert!(
            !align_view(&mut whole, 0, 0, AlignTo::Left),
            "not with itself"
        );
    }
}
