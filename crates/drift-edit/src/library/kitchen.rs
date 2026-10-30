//! Kitchen cabinets and appliances in plan. Each is based at its back-left corner on the
//! wall and reaches down into the room; islands are based at their centre.

use super::{Category, Entry, Pen, Sketch, inches};

/// How far a countertop runs past the cabinet fronts.
const OVERHANG: f64 = 1.5;
/// Standard depths.
const BASE_DEPTH: f64 = 24.0;
const WALL_DEPTH: f64 = 12.0;

/// A countertop `width` × `depth` (front edge included) with its light fill, at `x0`.
fn counter(s: &mut Sketch, x0: f64, width: f64, depth: f64) {
    s.pen(Pen::Normal);
    s.rect(x0, -depth, x0 + width, 0.0);
    s.fill((x0 + 0.5, -depth + 0.5));
}

/// A sink in a counter: its rim, one or two bowls, and the faucet behind. `cx` is the
/// middle of the sink, `back` how far its rim sits from the counter's back edge.
fn sink(s: &mut Sketch, cx: f64, back: f64, width: f64, depth: f64, bowls: usize) {
    let (x0, x1) = (cx - width / 2.0, cx + width / 2.0);
    let (y1, y0) = (-back, -back - depth);
    s.pen(Pen::Normal);
    s.rounded(x0, y0, x1, y1, 1.5);
    s.pen(Pen::Thin);
    let bowls = bowls.max(1);
    let gap = 2.0;
    let bowl = (width - 2.0 - gap * (bowls - 1) as f64) / bowls as f64;
    for k in 0..bowls {
        let bx = x0 + 1.0 + k as f64 * (bowl + gap);
        s.rounded(bx, y0 + 1.0, bx + bowl, y1 - 1.0, 2.5);
        s.circle((bx + bowl / 2.0, (y0 + y1) / 2.0), 1.0);
    }
    s.circle((cx, y1 + back / 2.0), 0.75);
}

/// A base cabinet with its countertop.
pub fn base_cabinet(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    counter(&mut s, 0.0, width, depth + OVERHANG);
    s
}

/// A wall cabinet: above the cut plane, so hidden.
pub fn wall_cabinet(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.pen(Pen::Hidden);
    s.rect(0.0, -depth, width, 0.0);
    s
}

/// A tall (pantry or oven) cabinet: full height, marked with an X.
pub fn tall_cabinet(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.pen(Pen::Thin);
    s.line((0.0, 0.0), (width, -depth));
    s.line((0.0, -depth), (width, 0.0));
    s
}

/// A sink base: a base cabinet with a sink of `bowls` bowls centred on it.
pub fn sink_base(width: f64, depth: f64, bowls: usize) -> Sketch {
    let mut s = Sketch::new();
    let top = depth + OVERHANG;
    counter(&mut s, 0.0, width, top);
    sink(&mut s, width / 2.0, 3.5, width - 6.0, top - 7.0, bowls);
    s
}

/// A dishwasher under the counter: the counter, and the machine below it hidden.
pub fn dishwasher(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    counter(&mut s, 0.0, width, depth + OVERHANG);
    s.pen(Pen::Hidden);
    s.rect(1.0, -depth + 1.0, width - 1.0, -1.0);
    s.fill((width / 2.0, -depth / 2.0));
    s
}

/// A free-standing or slide-in range: the backguard and four or six burners.
pub fn range(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.line((0.0, -3.0), (width, -3.0));
    s.pen(Pen::Thin);
    let cols = if width >= 36.0 { 3 } else { 2 };
    let pitch = width / cols as f64;
    for c in 0..cols {
        let x = pitch * (c as f64 + 0.5);
        for (row, y) in [-9.0, -19.5].into_iter().enumerate() {
            let r: f64 = if (c + row) % 2 == 0 { 4.5 } else { 3.5 };
            s.circle((x, y), r.min(pitch / 2.0 - 1.0));
        }
    }
    s
}

/// A refrigerator: the cabinet, and its doors (French doors from 36") 2" in front.
pub fn refrigerator(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    let body = depth - 2.0;
    s.rect(0.0, -body, width, 0.0);
    s.rect(0.0, -depth, width, -body);
    if width >= 36.0 {
        s.line((width / 2.0, -depth), (width / 2.0, -body));
    }
    s
}

/// An island `width` × `depth`, centred: cabinets along the back, and the seating
/// overhang in front (its cabinet face hidden). With a sink if `bowls` > 0.
pub fn island(width: f64, depth: f64, overhang: f64, bowls: usize) -> Sketch {
    let mut s = Sketch::new();
    let (x0, y0, x1, y1) = (-width / 2.0, -depth / 2.0, width / 2.0, depth / 2.0);
    s.rect(x0, y0, x1, y1);
    s.fill((x0 + 0.5, y1 - 0.5));
    s.pen(Pen::Hidden);
    s.line((x0, y0 + overhang), (x1, y0 + overhang));
    s.fill((x0 + 0.5, y0 + 0.5));
    if bowls > 0 {
        // In the cabinets along the back, its faucet behind it.
        let mut sk = Sketch::new();
        sink(&mut sk, 0.0, 3.5, 30.0, 18.0, bowls);
        s.place(&sk, (0.0, y1), 0.0);
    }
    s
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Kitchen;
    let mut v = Vec::new();
    let widths = [9.0, 12.0, 15.0, 18.0, 21.0, 24.0, 27.0, 30.0, 33.0, 36.0];
    for w in widths {
        v.push(Entry::new(
            c,
            "Base Cabinet",
            inches(w),
            (w, BASE_DEPTH),
            base_cabinet(w, BASE_DEPTH),
        ));
    }
    for w in widths {
        v.push(Entry::new(
            c,
            "Wall Cabinet",
            inches(w),
            (w, WALL_DEPTH),
            wall_cabinet(w, WALL_DEPTH),
        ));
    }
    for w in [18.0, 24.0, 30.0, 36.0] {
        v.push(Entry::new(
            c,
            "Tall Cabinet",
            inches(w),
            (w, BASE_DEPTH),
            tall_cabinet(w, BASE_DEPTH),
        ));
    }
    for (w, bowls) in [(30.0, 1), (33.0, 2), (36.0, 2)] {
        let name = if bowls == 1 {
            inches(w)
        } else {
            format!("{} Double Bowl", inches(w))
        };
        v.push(Entry::new(
            c,
            "Sink Base",
            name,
            (w, BASE_DEPTH),
            sink_base(w, BASE_DEPTH, bowls),
        ));
    }
    v.push(Entry::new(
        c,
        "Dishwasher",
        inches(24.0),
        (24.0, BASE_DEPTH),
        dishwasher(24.0, BASE_DEPTH),
    ));
    for w in [30.0, 36.0] {
        v.push(Entry::new(c, "Range", inches(w), (w, 26.0), range(w, 26.0)));
    }
    for w in [30.0, 36.0] {
        v.push(Entry::new(
            c,
            "Refrigerator",
            inches(w),
            (w, 30.0),
            refrigerator(w, 30.0),
        ));
    }
    for (w, d, over, bowls) in [
        (48.0, 36.0, 12.0, 0),
        (72.0, 36.0, 12.0, 0),
        (96.0, 42.0, 15.0, 2),
    ] {
        let mut name = format!("{} × {}", inches(w), inches(d));
        if bowls > 0 {
            name.push_str(" with Sink");
        }
        v.push(Entry::new(
            c,
            "Island",
            name,
            (w, d),
            island(w, d, over, bowls),
        ));
    }
    v
}
