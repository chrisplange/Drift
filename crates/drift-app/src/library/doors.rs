//! Doors and openings in plan. The opening runs along +X from the base point, on the
//! room-side face of a wall that lies above it (from 0 up to the wall's thickness); doors
//! swing down into the room. Mirror a copy for the other hand.

use super::{Category, Entry, Pen, Sketch, inches};

/// A typical interior wall: 2×4 studs and drywall.
const INTERIOR_WALL: f64 = 4.5;
/// A typical exterior wall: 2×6 studs, sheathing and drywall.
const EXTERIOR_WALL: f64 = 6.5;
/// Leaf thickness of interior and exterior doors.
const INTERIOR_LEAF: f64 = 1.375;
const EXTERIOR_LEAF: f64 = 1.75;

/// The jambs: short lines across the wall at each end of the opening.
fn jambs(s: &mut Sketch, x0: f64, x1: f64, wall: f64) {
    s.pen(Pen::Normal);
    s.line((x0, 0.0), (x0, wall));
    s.line((x1, 0.0), (x1, wall));
}

/// A leaf hinged at `(hx, 0)`, `width` long and `thick`, opened `swing` degrees into the
/// room, with its swing as a thin arc. `left`: hinged on the left, closing towards +X.
fn leaf(s: &mut Sketch, hx: f64, width: f64, thick: f64, swing: f64, left: bool) {
    let a = swing.to_radians();
    let sign = if left { 1.0 } else { -1.0 };
    // Along the open leaf, and across it towards the opening.
    let d = (sign * a.cos(), -a.sin());
    let n = (sign * a.sin(), a.cos());
    let at = |t: f64, u: f64| (hx + d.0 * t + n.0 * u, d.1 * t + n.1 * u);
    s.pen(Pen::Normal);
    s.polyline(&[
        at(0.0, 0.0),
        at(width, 0.0),
        at(width, thick),
        at(0.0, thick),
        at(0.0, 0.0),
    ]);
    s.pen(Pen::Thin);
    if left {
        s.arc((hx, 0.0), width, -swing, swing);
    } else {
        s.arc((hx, 0.0), width, 180.0, swing);
    }
}

/// A single swing door, `width` wide in a `wall`, open `swing` degrees.
pub fn swing_door(width: f64, wall: f64, swing: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    leaf(&mut s, 0.0, width, INTERIOR_LEAF, swing, true);
    s
}

/// A pair of doors (double or French) meeting in the middle of a `width` opening.
pub fn double_door(width: f64, wall: f64, swing: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    leaf(&mut s, 0.0, width / 2.0, INTERIOR_LEAF, swing, true);
    leaf(&mut s, width, width / 2.0, INTERIOR_LEAF, swing, false);
    s
}

/// A pocket door: the leaf slides into the wall to the left, shown a third open, its
/// pocket hidden.
pub fn pocket_door(width: f64, wall: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    let (y0, y1) = (
        wall / 2.0 - INTERIOR_LEAF / 2.0,
        wall / 2.0 + INTERIOR_LEAF / 2.0,
    );
    s.rect(-width / 3.0, y0, width * 2.0 / 3.0, y1);
    s.pen(Pen::Hidden);
    s.polyline(&[
        (0.0, y1 + 0.5),
        (-width - 1.0, y1 + 0.5),
        (-width - 1.0, y0 - 0.5),
        (0.0, y0 - 0.5),
    ]);
    s
}

/// Bypass (sliding) closet doors: two leaves on two tracks, overlapping in the middle.
pub fn bypass_door(width: f64, wall: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    let mid = wall / 2.0;
    s.rect(0.0, mid, width / 2.0 + 1.0, mid + INTERIOR_LEAF);
    s.rect(width / 2.0 - 1.0, mid - INTERIOR_LEAF, width, mid);
    s
}

/// Bifold doors: two panels folding out from each jamb (one pair below 48").
pub fn bifold_door(width: f64, wall: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    let pairs = if width >= 48.0 { 2 } else { 1 };
    let span = width / pairs as f64;
    let panel = span / 2.0;
    let a = 35f64.to_radians();
    // Each pair folds to a V of two panels, its fold pulled into the room.
    for k in 0..pairs {
        let x0 = k as f64 * span;
        let reach = 2.0 * panel * a.cos();
        let (from, to) = if k == 0 {
            (x0, x0 + reach)
        } else {
            (x0 + span, x0 + span - reach)
        };
        let fold = ((from + to) / 2.0, -panel * a.sin());
        s.polyline(&[(from, 0.0), fold, (to, 0.0)]);
    }
    s
}

/// A cased opening: jambs, and the head above as hidden lines.
pub fn cased_opening(width: f64, wall: f64) -> Sketch {
    let mut s = Sketch::new();
    jambs(&mut s, 0.0, width, wall);
    s.pen(Pen::Hidden);
    s.line((0.0, 0.0), (width, 0.0));
    s.line((0.0, wall), (width, wall));
    s
}

/// An exterior door with its threshold, and `sidelights` fixed glass panels each
/// `sidelight` wide: one to the right, or one each side.
pub fn exterior_door(width: f64, wall: f64, sidelight: f64, sidelights: usize) -> Sketch {
    let mut s = Sketch::new();
    let left = if sidelights >= 2 { -sidelight } else { 0.0 };
    let right = if sidelights >= 1 {
        width + sidelight
    } else {
        width
    };
    jambs(&mut s, left, right, wall);
    // Mullions between door and sidelights.
    if sidelights >= 2 {
        s.line((0.0, 0.0), (0.0, wall));
    }
    if sidelights >= 1 {
        s.line((width, 0.0), (width, wall));
    }
    leaf(&mut s, 0.0, width, EXTERIOR_LEAF, 90.0, true);
    s.pen(Pen::Normal);
    s.line((0.0, wall), (width, wall));
    // Sidelights: frame faces and the glass.
    let mut glass = |x0: f64, x1: f64| {
        s.pen(Pen::Normal);
        s.line((x0, 0.0), (x1, 0.0));
        s.line((x0, wall), (x1, wall));
        s.pen(Pen::Thin);
        s.line((x0, wall / 2.0), (x1, wall / 2.0));
    };
    if sidelights >= 1 {
        glass(width, width + sidelight);
    }
    if sidelights >= 2 {
        glass(-sidelight, 0.0);
    }
    s
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Doors;
    let mut v = Vec::new();
    for w in [24.0, 28.0, 30.0, 32.0, 36.0] {
        v.push(Entry::new(
            c,
            "Door – Single Swing",
            inches(w),
            (w, w + INTERIOR_WALL),
            swing_door(w, INTERIOR_WALL, 90.0),
        ));
    }
    for w in [48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Door – Double Swing",
            inches(w),
            (w, w / 2.0 + INTERIOR_WALL),
            double_door(w, INTERIOR_WALL, 90.0),
        ));
    }
    for w in [48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Door – French",
            inches(w),
            (w, w / 2.0 + EXTERIOR_WALL),
            double_door(w, EXTERIOR_WALL, 90.0),
        ));
    }
    for w in [24.0, 28.0, 30.0, 32.0, 36.0] {
        v.push(Entry::new(
            c,
            "Door – Pocket",
            inches(w),
            (w, INTERIOR_WALL),
            pocket_door(w, INTERIOR_WALL),
        ));
    }
    for w in [48.0, 60.0, 72.0, 96.0] {
        v.push(Entry::new(
            c,
            "Door – Bypass Closet",
            inches(w),
            (w, INTERIOR_WALL),
            bypass_door(w, INTERIOR_WALL),
        ));
    }
    for w in [24.0, 30.0, 36.0, 48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Door – Bifold",
            inches(w),
            (w, INTERIOR_WALL),
            bifold_door(w, INTERIOR_WALL),
        ));
    }
    for w in [30.0, 36.0, 48.0, 60.0, 72.0] {
        v.push(Entry::new(
            c,
            "Cased Opening",
            inches(w),
            (w, INTERIOR_WALL),
            cased_opening(w, INTERIOR_WALL),
        ));
    }
    for w in [32.0, 36.0] {
        v.push(Entry::new(
            c,
            "Door – Exterior",
            inches(w),
            (w, w + EXTERIOR_WALL),
            exterior_door(w, EXTERIOR_WALL, 0.0, 0),
        ));
    }
    for (w, side, n) in [
        (36.0, 12.0, 1),
        (36.0, 14.0, 1),
        (36.0, 12.0, 2),
        (36.0, 14.0, 2),
    ] {
        let name = if n == 1 {
            format!("{} + {} Sidelight", inches(w), inches(side))
        } else {
            format!("{} + Two {} Sidelights", inches(w), inches(side))
        };
        v.push(Entry::new(
            c,
            "Door – Exterior with Sidelight",
            name,
            (w + side * n as f64, w + EXTERIOR_WALL),
            exterior_door(w, EXTERIOR_WALL, side, n),
        ));
    }
    v
}
