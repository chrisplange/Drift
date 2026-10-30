//! Windows in plan, in an exterior wall. The opening runs along +X from the base point on
//! the room-side face of the wall; the wall lies above it, the outside above that.

use super::{Category, Entry, Pen, Sketch, inches};

/// A typical exterior wall: 2×6 studs, sheathing and drywall.
const WALL: f64 = 6.5;

/// What every window has: jambs, the stool line on the room side, the exterior face, and a
/// sill projecting outside.
fn frame(width: f64, wall: f64) -> Sketch {
    let mut s = Sketch::new();
    s.line((0.0, 0.0), (0.0, wall));
    s.line((width, 0.0), (width, wall));
    body(&mut s, width, wall);
    s
}

/// A window without its jambs: the stool line, the exterior face and the sill (a wall's
/// opening draws the jambs).
pub(super) fn body(s: &mut Sketch, width: f64, wall: f64) {
    s.pen(Pen::Normal);
    s.line((0.0, 0.0), (width, 0.0));
    s.line((0.0, wall), (width, wall));
    s.polyline(&[
        (-1.0, wall),
        (-1.0, wall + 1.5),
        (width + 1.0, wall + 1.5),
        (width + 1.0, wall),
    ]);
}

/// A fixed (picture) window: one pane of glass.
pub fn fixed_window(width: f64, wall: f64) -> Sketch {
    let mut s = frame(width, wall);
    fixed_glass(&mut s, width, wall);
    s
}

pub(super) fn fixed_glass(s: &mut Sketch, width: f64, wall: f64) {
    s.pen(Pen::Thin);
    s.line((0.0, wall / 2.0), (width, wall / 2.0));
}

/// A single or double hung window: two sashes, one behind the other.
pub fn hung_window(width: f64, wall: f64) -> Sketch {
    let mut s = frame(width, wall);
    hung_sashes(&mut s, width, wall);
    s
}

pub(super) fn hung_sashes(s: &mut Sketch, width: f64, wall: f64) {
    s.pen(Pen::Thin);
    s.line((0.0, wall / 2.0 - 0.75), (width, wall / 2.0 - 0.75));
    s.line((0.0, wall / 2.0 + 0.75), (width, wall / 2.0 + 0.75));
}

/// A horizontal slider: two sashes overlapping in the middle.
pub fn slider_window(width: f64, wall: f64) -> Sketch {
    let mut s = frame(width, wall);
    slider_sashes(&mut s, width, wall);
    s
}

pub(super) fn slider_sashes(s: &mut Sketch, width: f64, wall: f64) {
    s.pen(Pen::Thin);
    s.line(
        (0.0, wall / 2.0 + 0.75),
        (width / 2.0 + 1.0, wall / 2.0 + 0.75),
    );
    s.line(
        (width / 2.0 - 1.0, wall / 2.0 - 0.75),
        (width, wall / 2.0 - 0.75),
    );
}

/// Casement sashes (one, or a pair meeting in the middle), shown opened 30° outwards with
/// their swing.
pub fn casement_window(width: f64, wall: f64, sashes: usize) -> Sketch {
    let mut s = frame(width, wall);
    casement_sashes(&mut s, width, wall, sashes);
    s
}

pub(super) fn casement_sashes(s: &mut Sketch, width: f64, wall: f64, sashes: usize) {
    let y = wall / 2.0;
    s.pen(Pen::Thin);
    s.line((0.0, y), (width, y));
    let sash = width / sashes.max(1) as f64;
    let a = 30f64.to_radians();
    for k in 0..sashes.max(1) {
        let (hinge, dir) = if k == 0 { (0.0, 1.0) } else { (width, -1.0) };
        s.line(
            (hinge, y),
            (hinge + dir * sash * a.cos(), y + sash * a.sin()),
        );
        if dir > 0.0 {
            s.arc((hinge, y), sash, 0.0, 30.0);
        } else {
            s.arc((hinge, y), sash, 150.0, 30.0);
        }
    }
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Windows;
    let mut v = Vec::new();
    for w in [24.0, 36.0, 48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Window – Fixed",
            inches(w),
            (w, WALL),
            fixed_window(w, WALL),
        ));
    }
    for w in [24.0, 28.0, 32.0, 36.0] {
        v.push(Entry::new(
            c,
            "Window – Single Hung",
            inches(w),
            (w, WALL),
            hung_window(w, WALL),
        ));
    }
    for w in [24.0, 28.0, 32.0, 36.0, 40.0] {
        v.push(Entry::new(
            c,
            "Window – Double Hung",
            inches(w),
            (w, WALL),
            hung_window(w, WALL),
        ));
    }
    for (w, n) in [(24.0, 1), (30.0, 1), (48.0, 2), (60.0, 2)] {
        let name = if n == 1 {
            inches(w)
        } else {
            format!("{} Pair", inches(w))
        };
        v.push(Entry::new(
            c,
            "Window – Casement",
            name,
            (w, WALL),
            casement_window(w, WALL, n),
        ));
    }
    for w in [36.0, 48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Window – Slider",
            inches(w),
            (w, WALL),
            slider_window(w, WALL),
        ));
    }
    v
}
