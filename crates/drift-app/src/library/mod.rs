//! The built-in block library: doors, windows, kitchen, bath, furniture, and laundry and
//! utility items in US sizes.
//!
//! Every block is generated from its dimensions: one function per family takes that
//! family's sizes (`swing_door(width, wall, swing)`, `base_cabinet(width, depth)`, …) and
//! each category lists its standard types as a table of those sizes. Nothing is entered per
//! size, so a family can become a parametric block later with the same parameters. Names
//! are "Family – Type" (`Door – Single Swing – 36"`, `Base Cabinet – 24"`); every type of a
//! family shares its base point, so swapping one type for another keeps the placement.
//!
//! Conventions (block coordinates, in inches here, millimetres in the block):
//! - Plan view, the front facing down: a wall runs along the X axis and the room is below
//!   it (negative Y).
//! - Base points, the same for every type of a family:
//!   - Doors and openings: the hinge side (or left end) of the opening, on the room-side
//!     face of the wall. The opening runs along +X; the wall is above (0 to its thickness).
//!   - Windows: the left end of the opening on the room-side face of the wall.
//!   - Cabinets, appliances, counters, tubs, showers, washers and dryers: the back-left
//!     corner, on the wall.
//!   - Toilets, vanities, water heaters, beds, sofas, chairs, desks and dressers: the
//!     middle of the back, on the wall (a toilet or vanity stays on its plumbing line).
//!   - Tables and islands, free-standing: the centre.
//!   - Grab bars: the left end, on the wall.
//! - Swings are thin lines; things above the cut plane (wall cabinets, headers) are
//!   hidden lines; countertops have a light solid fill.

mod bath;
mod doors;
mod furniture;
pub mod hosted;
mod kitchen;
pub mod sizes;
mod utility;
mod windows;

use std::sync::OnceLock;

use drift_core::{
    Appearance, Block, BlockId, BlockItem, DVec2, Fill, FillKind, Instance, LineKind, LineType,
    LineWeight, Shape,
};

/// Millimetres in an inch.
pub const INCH: f64 = drift_core::units::MM_PER_INCH;

/// A category of the library, as the Blocks panel lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Doors,
    Windows,
    Kitchen,
    Bath,
    Furniture,
    Utility,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Doors,
        Category::Windows,
        Category::Kitchen,
        Category::Bath,
        Category::Furniture,
        Category::Utility,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Doors => "Doors",
            Category::Windows => "Windows",
            Category::Kitchen => "Kitchen",
            Category::Bath => "Bath",
            Category::Furniture => "Furniture",
            Category::Utility => "Laundry and utility",
        }
    }
}

/// One type of a family in the library.
#[derive(Clone, Debug)]
pub struct Entry {
    pub category: Category,
    /// The family, e.g. "Door – Single Swing".
    pub family: &'static str,
    /// The type within the family, e.g. `36"`.
    pub type_name: String,
    /// Named "Family – Type".
    pub block: Block,
    /// The nominal size in inches (width along X, depth along Y), for checking the drawing.
    pub nominal: (f64, f64),
}

impl Entry {
    fn new(
        category: Category,
        family: &'static str,
        type_name: impl Into<String>,
        nominal: (f64, f64),
        sketch: Sketch,
    ) -> Self {
        let type_name = type_name.into();
        let block = sketch.block(format!("{family} – {type_name}"));
        Self {
            category,
            family,
            type_name,
            block,
            nominal,
        }
    }

    /// Whether the entry matches a search: every word is in its name or category.
    pub fn matches(&self, search: &str) -> bool {
        let hay = format!("{} {}", self.block.name, self.category.label()).to_lowercase();
        search
            .split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }
}

/// Every block in the library, by category and family.
pub fn all() -> &'static [Entry] {
    static LIBRARY: OnceLock<Vec<Entry>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let mut v = doors::entries();
        v.extend(windows::entries());
        v.extend(kitchen::entries());
        v.extend(bath::entries());
        v.extend(furniture::entries());
        v.extend(utility::entries());
        v
    })
}

/// The library entry of a block name.
pub fn entry(name: &str) -> Option<&'static Entry> {
    all().iter().find(|e| e.block.name == name)
}

/// A length in inches as it's named: `36"`, `17-1/2"`, `8'`.
pub fn inches(v: f64) -> String {
    let whole = v.trunc();
    let frac = ((v - whole) * 8.0).round() as i64;
    let frac = match frac {
        0 => String::new(),
        8 => return inches(whole + 1.0),
        n => {
            let g = gcd(n, 8);
            format!("-{}/{}", n / g, 8 / g)
        }
    };
    format!("{}{frac}\"", whole as i64)
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// How a curve is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    /// The layer's line.
    Normal,
    /// The thinnest line: swings, glass, bedding.
    Thin,
    /// Hidden: above the cut plane or under a counter.
    Hidden,
}

/// Curves drawn in inches, kept as a block's items in millimetres.
#[derive(Clone, Debug, Default)]
pub struct Sketch {
    items: Vec<BlockItem>,
    fills: Vec<Fill>,
    pen: Option<Pen>,
}

fn p(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y) * INCH
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw what follows with `pen`.
    pub fn pen(&mut self, pen: Pen) -> &mut Self {
        self.pen = Some(pen);
        self
    }

    fn push(&mut self, shape: Shape) -> &mut Self {
        let appearance = match self.pen.unwrap_or(Pen::Normal) {
            Pen::Normal => Appearance::default(),
            Pen::Thin => Appearance {
                weight: Some(LineWeight::HAIRLINE),
                ..Appearance::default()
            },
            Pen::Hidden => Appearance {
                line_type: Some(LineType::Hidden),
                weight: Some(LineWeight::HAIRLINE),
                ..Appearance::default()
            },
        };
        self.items.push(BlockItem {
            shape,
            style: LineKind::Solid,
            appearance,
        });
        self
    }

    pub fn line(&mut self, a: (f64, f64), b: (f64, f64)) -> &mut Self {
        self.push(Shape::Line {
            a: p(a.0, a.1),
            b: p(b.0, b.1),
        })
    }

    /// Lines through `points`, in order.
    pub fn polyline(&mut self, points: &[(f64, f64)]) -> &mut Self {
        for w in points.windows(2) {
            self.line(w[0], w[1]);
        }
        self
    }

    /// A rectangle between two corners.
    pub fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> &mut Self {
        self.polyline(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)])
    }

    /// A rectangle with rounded corners of radius `r`.
    pub fn rounded(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> &mut Self {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        if r <= 0.0 {
            return self.rect(x0, y0, x1, y1);
        }
        let straight = |s: &mut Self, a: (f64, f64), b: (f64, f64)| {
            if (a.0 - b.0).abs() + (a.1 - b.1).abs() > 1e-9 {
                s.line(a, b);
            }
        };
        straight(self, (x0 + r, y0), (x1 - r, y0));
        self.arc((x1 - r, y0 + r), r, -90.0, 90.0);
        straight(self, (x1, y0 + r), (x1, y1 - r));
        self.arc((x1 - r, y1 - r), r, 0.0, 90.0);
        straight(self, (x1 - r, y1), (x0 + r, y1));
        self.arc((x0 + r, y1 - r), r, 90.0, 90.0);
        straight(self, (x0, y1 - r), (x0, y0 + r));
        self.arc((x0 + r, y0 + r), r, 180.0, 90.0)
    }

    /// A counter-clockwise arc from `start` degrees over `sweep` degrees.
    pub fn arc(&mut self, c: (f64, f64), r: f64, start: f64, sweep: f64) -> &mut Self {
        self.push(Shape::Arc {
            center: p(c.0, c.1),
            radius: r * INCH,
            start: start.to_radians(),
            sweep: sweep.to_radians(),
        })
    }

    pub fn circle(&mut self, c: (f64, f64), r: f64) -> &mut Self {
        self.push(Shape::Circle {
            center: p(c.0, c.1),
            radius: r * INCH,
        })
    }

    /// An ellipse with half-widths `rx` across and `ry` up.
    pub fn ellipse(&mut self, c: (f64, f64), rx: f64, ry: f64) -> &mut Self {
        if (rx - ry).abs() < 1e-9 {
            return self.circle(c, rx);
        }
        let (major, minor) = if rx >= ry {
            (DVec2::X * rx * INCH, ry * INCH)
        } else {
            (DVec2::Y * ry * INCH, rx * INCH)
        };
        self.push(Shape::Ellipse {
            center: p(c.0, c.1),
            major,
            minor,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        })
    }

    /// A light solid fill of the area around `seed`.
    pub fn fill(&mut self, seed: (f64, f64)) -> &mut Self {
        let mut f = Fill::new(Vec::new(), p(seed.0, seed.1), FillKind::Solid);
        f.opacity = 0.12;
        self.fills.push(f);
        self
    }

    /// Another sketch's curves and fills, turned by `angle` degrees about its base point
    /// and moved to `at`.
    pub fn place(&mut self, other: &Sketch, at: (f64, f64), angle: f64) -> &mut Self {
        let mut i = Instance::new(BlockId(0), p(at.0, at.1));
        i.angle = angle;
        for item in &other.items {
            self.items.push(BlockItem {
                shape: i.apply_shape(&item.shape),
                ..*item
            });
        }
        for f in &other.fills {
            self.fills.push(i.apply_fill(f));
        }
        self
    }

    pub fn block(self, name: String) -> Block {
        Block {
            name,
            items: self.items,
            fills: self.fills,
            profile: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn inch_names() {
        assert_eq!(inches(36.0), "36\"");
        assert_eq!(inches(17.5), "17-1/2\"");
        assert_eq!(inches(4.75), "4-3/4\"");
        assert_eq!(inches(1.999), "2\"");
    }

    #[test]
    fn library_blocks_are_named_sized_and_filled() {
        let lib = all();
        assert!(lib.len() > 100, "{}", lib.len());
        let names: BTreeSet<&str> = lib.iter().map(|e| e.block.name.as_str()).collect();
        assert_eq!(names.len(), lib.len(), "names are unique");
        for c in Category::ALL {
            assert!(lib.iter().any(|e| e.category == c), "{c:?}");
        }
        for e in lib {
            let b = &e.block;
            assert!(!b.items.is_empty(), "{}", b.name);
            assert_eq!(b.name, format!("{} – {}", e.family, e.type_name));
            assert_eq!(
                b.name.matches(" – ").count(),
                e.family.matches(" – ").count() + 1
            );
            // The drawing covers its nominal size, and not much more (swings and chairs
            // reach out further, but not beyond twice the size).
            let size = (b.bounds().max - b.bounds().min) / INCH;
            let (w, d) = e.nominal;
            let (lo, hi) = (size.x.min(size.y), size.x.max(size.y));
            let (nlo, nhi) = (w.min(d), w.max(d));
            assert!(
                hi >= nhi - 0.01 && lo >= nlo - 0.01,
                "{}: {size:?} smaller than {:?}",
                b.name,
                e.nominal
            );
            assert!(
                hi <= 2.0 * nhi + 30.0 && lo <= 2.0 * nhi + 30.0,
                "{}: {size:?} much larger than {:?}",
                b.name,
                e.nominal
            );
            // The base point is on or in the drawing's box.
            let bb = b.bounds();
            assert!(
                bb.min.x <= 1e-6 && bb.min.y <= 1e-6 && bb.max.x >= -1e-6 && bb.max.y >= -1e-6,
                "{}: base point outside {bb:?}",
                b.name
            );
            // Every fill finds its area among the block's curves.
            if !b.fills.is_empty() {
                let shapes: Vec<Shape> = b.items.iter().map(|i| i.shape).collect();
                let regions = drift_geom::regions::regions(&shapes);
                for f in &b.fills {
                    let r = drift_geom::regions::region_at(&regions, f.seed);
                    assert!(
                        r.is_some(),
                        "{}: fill at {:?} finds no area",
                        b.name,
                        f.seed
                    );
                }
            }
        }
    }

    #[test]
    fn families_share_their_base_point() {
        // Every type of a family sits the same way on its base point: either its back on
        // the same line (doors and windows on the wall face, beds against the head wall)
        // or centred on it (tables, islands).
        let mut by_family: std::collections::BTreeMap<&str, Vec<&Entry>> = Default::default();
        for e in all() {
            by_family.entry(e.family).or_default().push(e);
        }
        for (family, entries) in by_family {
            // Swings (thin lines) reach out by the size; leave them out.
            let top = |b: &Block| {
                b.items
                    .iter()
                    .filter(|i| i.appearance.weight != Some(LineWeight::HAIRLINE))
                    .map(|i| i.shape.bounds().max.y)
                    .fold(f64::MIN, f64::max)
            };
            let tops: BTreeSet<i64> = entries
                .iter()
                .map(|e| (top(&e.block) / INCH * 8.0).round() as i64)
                .collect();
            let centred = entries
                .iter()
                .all(|e| e.block.bounds().center().length() / INCH < 0.5);
            assert!(
                tops.len() == 1 || centred,
                "{family}: the back moves between types: {tops:?}"
            );
        }
    }
}
