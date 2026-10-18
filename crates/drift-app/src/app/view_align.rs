//! Lining views up on a sheet: smart guides while a view is dragged (its frame's edges and
//! middle to other frames' and to the drawing area's), drawings at the same scale lining
//! up (so the floors of side-by-side elevations meet), and align commands.

use drift_core::{DVec2, PlacedView, Sheet};

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
fn offset(v: &PlacedView) -> DVec2 {
    v.frame.at - v.center / v.scale
}

pub(crate) fn same_scale(a: &PlacedView, b: &PlacedView) -> bool {
    (a.scale - b.scale).abs() <= 1e-9 * a.scale.max(1.0)
}

/// Snap a drag of view `index` of `views` (on `sheet`; as it `was`) by `paper` mm: whichever
/// of its frame's edges and middle (or, panning, its drawing) comes within `tol` of another
/// view's, or of the drawing area's, goes to it. Returns the drag snapped, and the lines it
/// snapped to.
pub(crate) fn snap_drag(
    sheet: &Sheet,
    views: &[PlacedView],
    index: usize,
    was: &PlacedView,
    grip: ViewGrip,
    paper: DVec2,
    tol: f64,
) -> (DVec2, Vec<AlignGuide>) {
    let moved = dragged_view(was, grip, paper);
    let frame = moved.frame.bounds();
    let mine = offset(&moved);
    let area = drift_draft::sheet::drawing_area(sheet);
    let others: Vec<&PlacedView> = views
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, v)| v)
        .collect();
    let mut paper = paper;
    let mut guides = Vec::new();
    for k in 0..2 {
        let side = |b: drift_core::Bounds| [b.min[k], b.center()[k], b.max[k]];
        // What moves on this axis: frame lines, and the drawing.
        let mut lines: Vec<f64> = Vec::new();
        let mut drawing = false;
        match grip {
            ViewGrip::Move => {
                lines.extend(side(frame));
                drawing = true;
            }
            ViewGrip::Pan => drawing = true,
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
            targets.extend(side(o.frame.bounds()));
        }
        // Drawings first: where a frame and a drawing would line up alike, the drawing's
        // guide is the one shown.
        if drawing {
            for o in others.iter().filter(|o| same_scale(o, &moved)) {
                let theirs = offset(o);
                consider(
                    theirs[k] - mine[k],
                    AlignGuide {
                        vertical: k == 0,
                        // Through the other view's middle: the model is the same there.
                        at: o.frame.at[k],
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

/// Align view `index` of `views` with view `other`: its frame moves, and its drawing with it.
/// False if it can't (drawings at different scales).
pub(crate) fn align_view(
    views: &mut [PlacedView],
    index: usize,
    other: usize,
    how: AlignTo,
) -> bool {
    if index == other || index >= views.len() || other >= views.len() {
        return false;
    }
    let (target, view) = (&views[other], &views[index]);
    let (theirs, mine) = (target.frame.bounds(), view.frame.bounds());
    let shift = match how {
        AlignTo::Left => DVec2::new(theirs.min.x - mine.min.x, 0.0),
        AlignTo::Centre => DVec2::new(theirs.center().x - mine.center().x, 0.0),
        AlignTo::Right => DVec2::new(theirs.max.x - mine.max.x, 0.0),
        AlignTo::Top => DVec2::new(0.0, theirs.max.y - mine.max.y),
        AlignTo::Middle => DVec2::new(0.0, theirs.center().y - mine.center().y),
        AlignTo::Bottom => DVec2::new(0.0, theirs.min.y - mine.min.y),
        AlignTo::DrawingsAcross | AlignTo::DrawingsDown => {
            if !same_scale(view, target) {
                return false;
            }
            let d = offset(target) - offset(view);
            if how == AlignTo::DrawingsAcross {
                DVec2::new(0.0, d.y)
            } else {
                DVec2::new(d.x, 0.0)
            }
        }
    };
    views[index].frame.at += shift;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn placed(scale: f64, center: DVec2, at: DVec2, size: DVec2) -> PlacedView {
        PlacedView {
            id: drift_core::ViewId(1),
            name: String::new(),
            scale,
            center,
            frame: drift_core::ViewFrame { at, size },
            dimensions: true,
            notes: true,
            title: false,
            border: false,
        }
    }

    /// Two views at 1 : 24 (two elevations), side by side and not quite level.
    fn two_elevations() -> (Sheet, Vec<PlacedView>) {
        let a = placed(24.0, v(0.0, 1000.0), v(150.0, 200.0), v(200.0, 120.0));
        let b = placed(24.0, v(5000.0, 1000.0), v(420.0, 203.0), v(150.0, 100.0));
        (Sheet::new("A-201"), vec![a, b])
    }

    #[test]
    fn dragging_snaps_to_edges_and_lines_drawings_up() {
        let (sheet, views) = two_elevations();
        let was = views[1].clone();
        let snap = |views: &[PlacedView], was: &PlacedView, grip, d| {
            snap_drag(&sheet, views, 1, was, grip, d, 1.0)
        };
        // Moving view 2 down by 2.5: its drawing is then 0.5 off the first's: it snaps level.
        let (snapped, guides) = snap(&views, &was, ViewGrip::Move, v(0.0, -2.5));
        assert!((snapped.y + 3.0).abs() < 1e-9, "{snapped:?}");
        assert!(guides.iter().any(|g| g.drawings && !g.vertical));
        let moved = dragged_view(&was, ViewGrip::Move, snapped);
        assert!((offset(&moved).y - offset(&views[0]).y).abs() < 1e-9);
        // Its left edge near the first's right edge: it goes to it.
        let (snapped, guides) = snap(&views, &was, ViewGrip::Move, v(-94.6, 40.0));
        let left = 420.0 - 75.0 + snapped.x;
        assert!((left - 250.0).abs() < 1e-9, "{snapped:?}");
        assert!(
            guides
                .iter()
                .any(|g| g.vertical && (g.at - 250.0).abs() < 1e-9)
        );
        // Far from anything: as dragged.
        let (snapped, guides) = snap(&views, &was, ViewGrip::Move, v(13.3, 31.7));
        assert_eq!((snapped, guides.len()), (v(13.3, 31.7), 0));
        // Resizing: the edge being dragged snaps; the others don't move.
        let (snapped, _) = snap(&views, &was, ViewGrip::Resize(0, 1), v(0.0, 6.5));
        let top = 203.0 + 50.0 + snapped.y;
        assert!((top - 260.0).abs() < 1e-9, "the first view's top: {top}");
        // At another scale, drawings don't line up (frames still do).
        let mut other = views.clone();
        other[1].scale = 48.0;
        let was = other[1].clone();
        let (snapped, guides) = snap(&other, &was, ViewGrip::Move, v(0.0, -2.5));
        assert!(!guides.iter().any(|g| g.drawings));
        assert_eq!(snapped.y, -3.0, "the frames' middles still level");
    }

    #[test]
    fn align_commands_move_frames_and_line_drawings_up() {
        let (_, mut views) = two_elevations();
        assert!(align_view(&mut views, 1, 0, AlignTo::Top));
        let top = |vs: &[PlacedView], i: usize| vs[i].frame.bounds().max.y;
        assert!((top(&views, 1) - top(&views, 0)).abs() < 1e-9);
        assert!(align_view(&mut views, 1, 0, AlignTo::DrawingsAcross));
        assert!((offset(&views[1]).y - offset(&views[0]).y).abs() < 1e-9);
        // Its x is untouched.
        assert_eq!(views[1].frame.at.x, 420.0);
        assert!(align_view(&mut views, 1, 0, AlignTo::Left));
        assert_eq!(views[1].frame.at.x, 50.0 + 75.0);
        // Drawings at different scales can't line up.
        views[1].scale = 48.0;
        assert!(!align_view(&mut views, 1, 0, AlignTo::DrawingsDown));
        assert!(
            !align_view(&mut views, 0, 0, AlignTo::Left),
            "not with itself"
        );
    }
}
