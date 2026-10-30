//! Laundry and mechanical equipment in plan. Washers, dryers, furnaces and utility sinks
//! are based at their back-left corner on the wall; water heaters at the middle of their
//! back, on the wall.

use super::{Category, Entry, Pen, Sketch, inches};

/// A front-loading washer: its control panel along the back and the drum.
pub fn washer(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.line((0.0, -4.0), (width, -4.0));
    s.pen(Pen::Thin);
    let r = (width / 2.0 - 3.0).min((depth - 4.0) / 2.0 - 2.0);
    s.circle((width / 2.0, -4.0 - (depth - 4.0) / 2.0), r);
    s
}

/// A dryer: its control panel along the back and its door.
pub fn dryer(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.line((0.0, -4.0), (width, -4.0));
    s.pen(Pen::Thin);
    s.rounded(3.0, -depth + 3.0, width - 3.0, -7.0, 3.0);
    s
}

/// A stacked washer and dryer: one cabinet, marked with an X.
pub fn stacked_laundry(width: f64, depth: f64) -> Sketch {
    let mut s = washer(width, depth);
    s.pen(Pen::Thin);
    s.line((0.0, -4.0), (width, -depth));
    s.line((0.0, -depth), (width, -4.0));
    s
}

/// A tank water heater `diameter` across, against the wall.
pub fn tank_water_heater(diameter: f64) -> Sketch {
    let mut s = Sketch::new();
    let r = diameter / 2.0;
    s.circle((0.0, -r), r);
    s.pen(Pen::Thin);
    s.circle((0.0, -r), 2.0);
    s
}

/// A wall-hung tankless water heater `width` × `depth`.
pub fn tankless_water_heater(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(-width / 2.0, -depth, width / 2.0, 0.0);
    s.pen(Pen::Thin);
    s.line((-width / 2.0, -depth), (width / 2.0, 0.0));
    s
}

/// A furnace or air handler `width` × `depth`, marked with an X.
pub fn furnace(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.pen(Pen::Thin);
    s.line((0.0, 0.0), (width, -depth));
    s.line((0.0, -depth), (width, 0.0));
    s
}

/// A utility (laundry) sink `width` × `depth`: the tub, its basin and drain.
pub fn utility_sink(width: f64, depth: f64) -> Sketch {
    let mut s = Sketch::new();
    s.rect(0.0, -depth, width, 0.0);
    s.pen(Pen::Thin);
    s.rounded(2.0, -depth + 2.0, width - 2.0, -4.0, 2.5);
    s.circle((width / 2.0, -depth / 2.0 - 1.0), 1.0);
    s.circle((width / 2.0, -2.0), 0.75);
    s
}

pub fn entries() -> Vec<Entry> {
    let c = Category::Utility;
    let mut v = vec![
        Entry::new(c, "Washer", inches(27.0), (27.0, 30.0), washer(27.0, 30.0)),
        Entry::new(c, "Dryer", inches(27.0), (27.0, 30.0), dryer(27.0, 30.0)),
        Entry::new(
            c,
            "Washer and Dryer – Stacked",
            inches(27.0),
            (27.0, 30.0),
            stacked_laundry(27.0, 30.0),
        ),
    ];
    for (gal, dia) in [(40, 20.0), (50, 22.0), (80, 24.0)] {
        v.push(Entry::new(
            c,
            "Water Heater",
            format!("{gal} gal"),
            (dia, dia),
            tank_water_heater(dia),
        ));
    }
    v.push(Entry::new(
        c,
        "Water Heater",
        "Tankless",
        (18.0, 10.0),
        tankless_water_heater(18.0, 10.0),
    ));
    for w in [17.5, 21.0, 24.5] {
        v.push(Entry::new(
            c,
            "Furnace",
            inches(w),
            (w, 29.0),
            furnace(w, 29.0),
        ));
    }
    v.push(Entry::new(
        c,
        "Utility Sink",
        inches(24.0),
        (24.0, 22.0),
        utility_sink(24.0, 22.0),
    ));
    v
}
