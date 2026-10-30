//! Door and window families hosted in walls: the symbol drawn in an opening, at the wall's
//! thickness (`docs/walls-design.md` §6). Each family's generator is the library's (doors.rs,
//! windows.rs) without its jambs, which the wall's cut draws.
//!
//! A symbol is drawn in the opening's frame (the library's convention): the base point on the
//! face it swings to (the room side) at the hinge jamb, the opening along +X, the wall above
//! (0 to its thickness). The opening is the cut in the wall; a door's leaf is drawn between
//! its frames, centred in it.

use drift_core::{Appearance, Shape};

use super::{INCH, Sketch, doors, windows};

/// What a family is, for the tool bar's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FamilyKind {
    Door,
    Window,
    /// An opening with no leaf (a cased opening).
    Opening,
}

/// A family of hosted symbols.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Family {
    /// Its id, stored in drawings: `door.single`.
    pub id: &'static str,
    /// Named as the library names its blocks: `Door – Single Swing`.
    pub name: &'static str,
    pub kind: FamilyKind,
}

/// Every hosted family, in the order the tool bar lists them.
pub const FAMILIES: &[Family] = &[
    Family {
        id: "door.single",
        name: "Door – Single Swing",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.double",
        name: "Door – Double Swing",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.french",
        name: "Door – French",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.pocket",
        name: "Door – Pocket",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.bypass",
        name: "Door – Bypass Closet",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.bifold",
        name: "Door – Bifold",
        kind: FamilyKind::Door,
    },
    Family {
        id: "door.exterior",
        name: "Door – Exterior",
        kind: FamilyKind::Door,
    },
    Family {
        id: "opening.cased",
        name: "Cased Opening",
        kind: FamilyKind::Opening,
    },
    Family {
        id: "window.double-hung",
        name: "Window – Double Hung",
        kind: FamilyKind::Window,
    },
    Family {
        id: "window.single-hung",
        name: "Window – Single Hung",
        kind: FamilyKind::Window,
    },
    Family {
        id: "window.casement",
        name: "Window – Casement",
        kind: FamilyKind::Window,
    },
    Family {
        id: "window.casement-pair",
        name: "Window – Casement Pair",
        kind: FamilyKind::Window,
    },
    Family {
        id: "window.slider",
        name: "Window – Slider",
        kind: FamilyKind::Window,
    },
    Family {
        id: "window.fixed",
        name: "Window – Fixed",
        kind: FamilyKind::Window,
    },
];

/// The family of an id.
pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id == id)
}

/// The symbol of family `id` in an opening `width` wide whose leaf (a door's, or a window's
/// nominal width) is `leaf`, in a wall `wall` thick (all in mm): lines and arcs in the
/// opening's frame, with their look. Empty for a family this build doesn't know (the opening
/// is drawn as a plain cut).
pub fn symbol(id: &str, width: f64, leaf: f64, wall: f64) -> Vec<(Shape, Appearance)> {
    // (The library draws in inches.)
    let (w, l, t) = (width / INCH, leaf.clamp(0.0, width) / INCH, wall / INCH);
    // A door's leaf sits centred between its frames.
    let f = (w - l) / 2.0;
    let mut s = Sketch::new();
    match id {
        "door.single" => doors::leaf(&mut s, f, l, doors::INTERIOR_LEAF, 90.0, true),
        "door.double" | "door.french" => {
            doors::leaf(&mut s, f, l / 2.0, doors::INTERIOR_LEAF, 90.0, true);
            doors::leaf(&mut s, w - f, l / 2.0, doors::INTERIOR_LEAF, 90.0, false);
        }
        "door.pocket" => {
            s.place(&pocket(l, t), (f, 0.0), 0.0);
        }
        "door.bypass" => {
            let mut b = Sketch::new();
            doors::bypass(&mut b, l, t);
            s.place(&b, (f, 0.0), 0.0);
        }
        "door.bifold" => {
            let mut b = Sketch::new();
            doors::bifold(&mut b, l);
            s.place(&b, (f, 0.0), 0.0);
        }
        "door.exterior" => {
            doors::leaf(&mut s, f, l, doors::EXTERIOR_LEAF, 90.0, true);
            // The threshold, on the outside face.
            s.pen(super::Pen::Normal);
            s.line((0.0, t), (w, t));
        }
        "opening.cased" => doors::head(&mut s, 0.0, w, t),
        "window.double-hung" | "window.single-hung" => {
            windows::body(&mut s, w, t);
            windows::hung_sashes(&mut s, w, t);
        }
        "window.casement" | "window.casement-pair" => {
            windows::body(&mut s, w, t);
            let sashes = if id == "window.casement-pair" { 2 } else { 1 };
            windows::casement_sashes(&mut s, w, t, sashes);
        }
        "window.slider" => {
            windows::body(&mut s, w, t);
            windows::slider_sashes(&mut s, w, t);
        }
        "window.fixed" => {
            windows::body(&mut s, w, t);
            windows::fixed_glass(&mut s, w, t);
        }
        _ => {}
    }
    s.items
        .into_iter()
        .map(|i| (i.shape, i.appearance))
        .collect()
}

fn pocket(l: f64, t: f64) -> Sketch {
    let mut s = Sketch::new();
    doors::pocket(&mut s, l, t);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_draws_inside_its_opening_and_wall() {
        let (w, l, t) = (36.0 * INCH, 34.0 * INCH, 4.5 * INCH);
        for f in FAMILIES {
            let items = symbol(f.id, w, l, t);
            assert!(!items.is_empty(), "{}", f.id);
            for (shape, _) in &items {
                assert!(
                    matches!(shape, Shape::Line { .. } | Shape::Arc { .. }),
                    "{}: {shape:?}",
                    f.id
                );
                let b = shape.bounds();
                // Along the wall: within the opening (a pocket's leaf slides into the wall
                // beside it, a sill reaches past the jambs).
                assert!(
                    b.min.x >= -w - 2.0 * INCH && b.max.x <= w + 2.0 * INCH,
                    "{}",
                    f.id
                );
                // Across: a swing reaches into the room by its leaf, a sill or a casement's
                // sash out past the wall.
                assert!(b.min.y >= -l - 1e-6 && b.max.y <= t + w, "{}", f.id);
            }
        }
        assert!(symbol("door.unknown", w, l, t).is_empty());
    }

    #[test]
    fn a_swing_door_is_hinged_at_its_frame() {
        let (w, l) = (38.0 * INCH, 36.0 * INCH);
        let items = symbol("door.single", w, l, 4.5 * INCH);
        let swing = items
            .iter()
            .find_map(|(s, _)| match s {
                Shape::Arc { center, radius, .. } => Some((*center, *radius)),
                _ => None,
            })
            .expect("a swing");
        assert!((swing.0.x - INCH).abs() < 1e-9 && swing.0.y.abs() < 1e-9);
        assert!((swing.1 - l).abs() < 1e-9);
    }
}
