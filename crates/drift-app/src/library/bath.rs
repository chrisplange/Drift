//! Bathroom fixtures in plan. Toilets and vanities are based at the middle of their back on
//! the wall, so they stay on the plumbing line; tubs, showers and grab bars at their
//! back-left corner on the wall; a free-standing tub at its centre.

use super::{Category, Entry, Pen, Sketch, inches};

/// A toilet reaching `depth` from the wall, its bowl `bowl` long (front to back): about
/// 18-1/2" elongated, 16-1/2" round.
pub fn toilet(depth: f64, bowl: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rounded(-10.0, -9.0, 10.0, 0.0, 1.5);
    let (rx, ry) = (7.25, bowl / 2.0 + 0.75);
    let cy = -depth + ry;
    s.ellipse((0.0, cy), rx, ry);
    // The neck joining tank and bowl.
    let nx = 5.5;
    let meet = cy + ry * (1.0 - (nx / rx).powi(2)).sqrt();
    s.line((-nx, -9.0), (-nx, meet));
    s.line((nx, -9.0), (nx, meet));
    s.pen(Pen::Thin);
    s.ellipse((0.0, cy - 0.75), rx - 2.25, ry - 3.0);
    s
}

/// A vanity `width` wide with its top and one or two sinks.
pub fn vanity(width: f64, depth: f64, sinks: usize) -> Sketch {
    let mut s = Sketch::new();
    let top = depth + 1.0;
    s.rect(-width / 2.0, -top, width / 2.0, 0.0);
    s.fill((-width / 2.0 + 0.5, -top + 0.5));
    let sinks = sinks.max(1);
    let pitch = width / sinks as f64;
    for k in 0..sinks {
        let cx = -width / 2.0 + pitch * (k as f64 + 0.5);
        s.pen(Pen::Normal);
        s.ellipse((cx, -top / 2.0 - 1.0), 8.5, 6.5);
        s.pen(Pen::Thin);
        s.circle((cx, -top / 2.0 - 1.0), 0.75);
        s.circle((cx, -2.25), 0.75);
    }
    s
}

/// An alcove tub, `length` × `width`, its drain at the left end.
pub fn tub(length: f64, width: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -width, length, 0.0);
    s.pen(Pen::Thin);
    s.rounded(3.0, -width + 3.0, length - 3.0, -4.0, 7.0);
    s.circle((7.0, -width / 2.0 - 0.5), 1.25);
    s
}

/// A free-standing oval tub, centred.
pub fn freestanding_tub(length: f64, width: f64) -> Sketch {
    let mut s = Sketch::new();
    s.ellipse((0.0, 0.0), length / 2.0, width / 2.0);
    s.pen(Pen::Thin);
    s.ellipse((0.0, 0.0), length / 2.0 - 3.0, width / 2.0 - 3.0);
    s.circle((0.0, 0.0), 1.25);
    s
}

/// A shower `width` × `depth`: the receptor, its rim and the drain.
pub fn shower(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.pen(Pen::Thin);
    s.rect(2.0, -depth + 2.0, width - 2.0, -2.0);
    let c = (width / 2.0, -depth / 2.0);
    s.circle(c, 2.0);
    // The pan slopes from its corners to the drain.
    for corner in [
        (2.0, -2.0),
        (width - 2.0, -2.0),
        (width - 2.0, -depth + 2.0),
        (2.0, -depth + 2.0),
    ] {
        let (dx, dy) = (corner.0 - c.0, corner.1 - c.1);
        let len = dx.hypot(dy);
        s.line(corner, (c.0 + dx / len * 2.0, c.1 + dy / len * 2.0));
    }
    s
}

/// A grab bar `length` long: the 1-1/2" bar 1-1/2" off the wall, on its flanges.
pub fn grab_bar(length: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rounded(0.0, -3.0, length, -1.5, 0.75);
    s.rect(1.5, -1.5, 3.0, 0.0);
    s.rect(length - 3.0, -1.5, length - 1.5, 0.0);
    s
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Bath;
    let mut v = vec![
        Entry::new(c, "Toilet", "Elongated", (20.0, 30.0), toilet(30.0, 18.5)),
        Entry::new(c, "Toilet", "Round", (20.0, 28.0), toilet(28.0, 16.5)),
    ];
    for (w, sinks) in [
        (24.0, 1),
        (30.0, 1),
        (36.0, 1),
        (48.0, 1),
        (60.0, 1),
        (60.0, 2),
        (72.0, 2),
    ] {
        let name = if sinks == 1 {
            inches(w)
        } else {
            format!("{} Double", inches(w))
        };
        v.push(Entry::new(
            c,
            "Vanity",
            name,
            (w, 21.0),
            vanity(w, 21.0, sinks),
        ));
    }
    for (l, w) in [(60.0, 30.0), (60.0, 32.0), (72.0, 36.0)] {
        v.push(Entry::new(
            c,
            "Tub",
            format!("{} × {}", inches(l), inches(w)),
            (l, w),
            tub(l, w),
        ));
    }
    v.push(Entry::new(
        c,
        "Tub – Freestanding",
        format!("{} × {}", inches(66.0), inches(32.0)),
        (66.0, 32.0),
        freestanding_tub(66.0, 32.0),
    ));
    for (w, d) in [(36.0, 36.0), (48.0, 36.0), (60.0, 36.0)] {
        v.push(Entry::new(
            c,
            "Shower",
            format!("{} × {}", inches(w), inches(d)),
            (w, d),
            shower(w, d),
        ));
    }
    for l in [18.0, 24.0, 36.0, 42.0, 48.0] {
        v.push(Entry::new(c, "Grab Bar", inches(l), (l, 3.0), grab_bar(l)));
    }
    v
}
