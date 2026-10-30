//! Furniture in plan. Pieces that stand against a wall (beds, sofas, chairs, desks,
//! dressers) are based at the middle of their back, on the wall, facing down into the
//! room; tables and stools at their centre.

use super::{Category, Entry, Pen, Sketch, inches};

/// Nightstands beside a bed: their width, depth and the gap to the bed.
const NIGHTSTAND: (f64, f64) = (22.0, 18.0);
const NIGHTSTAND_GAP: f64 = 2.0;
/// The headboard's depth.
const HEADBOARD: f64 = 2.0;
/// A dining chair, and how far it stands off a table's edge.
const CHAIR: (f64, f64) = (18.0, 21.0);
const CHAIR_GAP: f64 = 2.0;

/// A bed `width` × `length` (the mattress), with its headboard, pillows and turned-down
/// sheet, and nightstands either side if asked.
pub fn bed(width: f64, length: f64, nightstands: bool) -> Sketch {
    let mut s = Sketch::new();
    let (x0, x1) = (-width / 2.0, width / 2.0);
    s.rect(x0, -HEADBOARD, x1, 0.0);
    s.rect(x0, -HEADBOARD - length, x1, -HEADBOARD);
    s.pen(Pen::Thin);
    let pillows = if width < 50.0 { 1 } else { 2 };
    let gap = 2.0;
    let pw = (width - 4.0 - gap * (pillows - 1) as f64) / pillows as f64;
    for k in 0..pillows {
        let px = x0 + 2.0 + k as f64 * (pw + gap);
        s.rounded(px, -HEADBOARD - 16.0, px + pw, -HEADBOARD - 2.0, 3.0);
    }
    // The sheet turned down, one corner folded back.
    let fold = -HEADBOARD - 22.0;
    s.line((x0, fold), (x1, fold));
    s.polyline(&[
        (x1 - 14.0, fold),
        (x1 - 8.0, fold - 10.0),
        (x1, fold - 10.0),
    ]);
    if nightstands {
        let (w, d) = NIGHTSTAND;
        for side in [-1.0, 1.0] {
            let near = side * (width / 2.0 + NIGHTSTAND_GAP);
            let far = side * (width / 2.0 + NIGHTSTAND_GAP + w);
            s.pen(Pen::Normal);
            s.rect(near, -d, far, 0.0);
            s.pen(Pen::Thin);
            s.circle(((near + far) / 2.0, -d / 2.0), 5.0);
        }
    }
    s
}

/// An upholstered seat — sofa, loveseat or lounge chair — `width` × `depth`, with arms,
/// a back and `seats` cushions.
pub fn upholstered(width: f64, depth: f64, seats: usize, arm: f64, back: f64) -> Sketch {
    let mut s = Sketch::new();
    let (x0, x1) = (-width / 2.0, width / 2.0);
    s.rounded(x0, -depth, x1, 0.0, 2.0);
    // Arms full depth, the back between them.
    let (a0, a1) = (x0 + arm, x1 - arm);
    s.line((a0, 0.0), (a0, -depth + 1.0));
    s.line((a1, 0.0), (a1, -depth + 1.0));
    s.line((a0, -back), (a1, -back));
    s.pen(Pen::Thin);
    let seats = seats.max(1);
    let cushion = (a1 - a0) / seats as f64;
    for k in 1..seats {
        let x = a0 + cushion * k as f64;
        s.line((x, -back), (x, -depth + 1.0));
    }
    s
}

/// A dining chair `width` × `depth`: the seat and its back.
pub fn dining_chair(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    let (x0, x1) = (-width / 2.0, width / 2.0);
    s.rect(x0, -2.5, x1, 0.0);
    s.rounded(x0 + 0.5, -depth, x1 - 0.5, -2.5, 2.0);
    s
}

/// A swivel desk chair `width` across: a round seat and a curved back.
pub fn desk_chair(width: f64) -> Sketch {
    let mut s = Sketch::new();
    let r = width / 2.0;
    s.circle((0.0, -r), r - 2.0);
    // The back: a band along the top of the seat.
    let a = 35.0;
    s.arc((0.0, -r), r, 90.0 - a, 2.0 * a);
    s.arc((0.0, -r), r - 2.0, 90.0 - a, 2.0 * a);
    let end = |deg: f64, rad: f64| {
        let t = deg.to_radians();
        (rad * t.cos(), -r + rad * t.sin())
    };
    s.line(end(90.0 - a, r - 2.0), end(90.0 - a, r));
    s.line(end(90.0 + a, r - 2.0), end(90.0 + a, r));
    s
}

/// A rectangular dining table `width` × `depth`, centred, with `per_side` chairs along
/// each long side and one at each end if `ends`.
pub fn dining_table(width: f64, depth: f64, per_side: usize, ends: bool) -> Sketch {
    let mut s = Sketch::new();
    s.rect(-width / 2.0, -depth / 2.0, width / 2.0, depth / 2.0);
    let chair = dining_chair(CHAIR.0, CHAIR.1);
    let out = CHAIR_GAP + CHAIR.1;
    let pitch = width / per_side as f64;
    for k in 0..per_side {
        let x = -width / 2.0 + pitch * (k as f64 + 0.5);
        s.place(&chair, (x, depth / 2.0 + out), 0.0);
        s.place(&chair, (x, -depth / 2.0 - out), 180.0);
    }
    if ends {
        s.place(&chair, (-width / 2.0 - out, 0.0), 90.0);
        s.place(&chair, (width / 2.0 + out, 0.0), -90.0);
    }
    s
}

/// A round table `diameter` across, centred, with `seats` chairs around it.
pub fn round_table(diameter: f64, seats: usize) -> Sketch {
    let mut s = Sketch::new();
    let r = diameter / 2.0;
    s.circle((0.0, 0.0), r);
    let chair = dining_chair(CHAIR.0, CHAIR.1);
    let out = r + CHAIR_GAP + CHAIR.1;
    for k in 0..seats {
        let phi = 90.0 + 360.0 * k as f64 / seats as f64;
        let t = phi.to_radians();
        s.place(&chair, (out * t.cos(), out * t.sin()), phi - 90.0);
    }
    s
}

/// A desk `width` × `depth` with a drawer pedestal at the right.
pub fn desk(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(-width / 2.0, -depth, width / 2.0, 0.0);
    s.pen(Pen::Thin);
    s.line((width / 2.0 - 16.0, 0.0), (width / 2.0 - 16.0, -depth));
    s
}

/// A case piece — dresser, nightstand, bookcase — `width` × `depth`, its top's edge drawn
/// in from the front.
pub fn case_piece(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(-width / 2.0, -depth, width / 2.0, 0.0);
    s.pen(Pen::Thin);
    s.line((-width / 2.0, -depth + 1.0), (width / 2.0, -depth + 1.0));
    s
}

/// A coffee or side table `width` × `depth`, centred, its corners rounded.
pub fn low_table(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rounded(-width / 2.0, -depth / 2.0, width / 2.0, depth / 2.0, 2.0);
    s
}

/// A stool `diameter` across, centred.
pub fn stool(diameter: f64) -> Sketch {
    let mut s = Sketch::new();
    s.circle((0.0, 0.0), diameter / 2.0);
    s.pen(Pen::Thin);
    s.circle((0.0, 0.0), diameter / 2.0 - 2.0);
    s
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Furniture;
    let mut v = Vec::new();
    let beds = [
        ("Twin", 39.0, 75.0),
        ("Twin XL", 39.0, 80.0),
        ("Full", 54.0, 75.0),
        ("Queen", 60.0, 80.0),
        ("King", 76.0, 80.0),
        ("California King", 72.0, 84.0),
    ];
    for (name, w, l) in beds {
        v.push(Entry::new(
            c,
            "Bed",
            name,
            (w, l + HEADBOARD),
            bed(w, l, false),
        ));
    }
    for (name, w, l) in beds {
        let stands = 2.0 * (NIGHTSTAND.0 + NIGHTSTAND_GAP);
        v.push(Entry::new(
            c,
            "Bed",
            format!("{name} with Nightstands"),
            (w + stands, l + HEADBOARD),
            bed(w, l, true),
        ));
    }
    for (w, seats) in [(84.0, 3), (96.0, 3)] {
        v.push(Entry::new(
            c,
            "Sofa",
            inches(w),
            (w, 36.0),
            upholstered(w, 36.0, seats, 6.0, 9.0),
        ));
    }
    v.push(Entry::new(
        c,
        "Loveseat",
        inches(64.0),
        (64.0, 36.0),
        upholstered(64.0, 36.0, 2, 6.0, 9.0),
    ));
    v.push(Entry::new(
        c,
        "Chair",
        "Dining",
        CHAIR,
        dining_chair(CHAIR.0, CHAIR.1),
    ));
    v.push(Entry::new(
        c,
        "Chair",
        "Lounge",
        (32.0, 34.0),
        upholstered(32.0, 34.0, 1, 5.0, 8.0),
    ));
    v.push(Entry::new(
        c,
        "Chair",
        "Desk",
        (20.0, 22.0),
        desk_chair(24.0),
    ));
    for (w, d, per_side, ends) in [
        (48.0, 36.0, 2, false),
        (72.0, 36.0, 2, true),
        (96.0, 42.0, 3, true),
    ] {
        let seats = 2 * per_side + if ends { 2 } else { 0 };
        v.push(Entry::new(
            c,
            "Dining Table",
            format!("{seats} Seats {} × {}", inches(w), inches(d)),
            (w, d),
            dining_table(w, d, per_side, ends),
        ));
    }
    for (dia, seats) in [(42.0, 4), (48.0, 4), (60.0, 6)] {
        v.push(Entry::new(
            c,
            "Dining Table",
            format!("{seats} Seats Round {}", inches(dia)),
            (dia, dia),
            round_table(dia, seats),
        ));
    }
    for (w, d) in [(48.0, 24.0), (60.0, 30.0), (72.0, 30.0)] {
        v.push(Entry::new(
            c,
            "Desk",
            format!("{} × {}", inches(w), inches(d)),
            (w, d),
            desk(w, d),
        ));
    }
    for (w, d) in [(36.0, 18.0), (60.0, 18.0), (72.0, 20.0)] {
        v.push(Entry::new(
            c,
            "Dresser",
            format!("{} × {}", inches(w), inches(d)),
            (w, d),
            case_piece(w, d),
        ));
    }
    v.push(Entry::new(
        c,
        "Nightstand",
        inches(NIGHTSTAND.0),
        NIGHTSTAND,
        case_piece(NIGHTSTAND.0, NIGHTSTAND.1),
    ));
    for w in [30.0, 36.0] {
        v.push(Entry::new(
            c,
            "Bookcase",
            inches(w),
            (w, 12.0),
            case_piece(w, 12.0),
        ));
    }
    for (name, w, d) in [("Coffee", 48.0, 24.0), ("Side", 22.0, 22.0)] {
        v.push(Entry::new(
            c,
            "Table",
            format!("{name} {} × {}", inches(w), inches(d)),
            (w, d),
            low_table(w, d),
        ));
    }
    v.push(Entry::new(
        c,
        "Stool",
        inches(16.0),
        (16.0, 16.0),
        stool(16.0),
    ));
    v
}
