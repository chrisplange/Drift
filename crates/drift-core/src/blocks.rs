//! Blocks: geometry defined once and placed many times (furniture, fixtures, symbols).
//!
//! A [`Block`] holds rigid curves in its own coordinates, with its base point at the origin.
//! An [`Instance`] places a block with a position, rotation, scale, mirror and layer. Copies
//! aren't solved: they're drawn, picked and snapped through their transform, and editing the
//! block updates every copy. A block can also hold fills and hatches of areas its curves
//! enclose.

use glam::DVec2;

use crate::drafting::{Fill, FillKind};
use crate::layers::{Appearance, LayerId};
use crate::shape::{LineKind, Shape};

/// Stable identifier of a block definition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b{}", self.0)
    }
}

/// Stable identifier of a placed copy of a block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub u32);

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "i{}", self.0)
    }
}

/// In a block, the `path` of a fill that filled a shape (curves drawn as one) where the
/// block was made: exploded, its curves become a shape again.
pub const SHAPE_FILL: crate::PathId = crate::PathId(0);

/// One curve of a block, in block coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockItem {
    pub shape: Shape,
    pub style: LineKind,
    pub appearance: Appearance,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub name: String,
    pub items: Vec<BlockItem>,
    /// Fills and hatches, bottom first. Each fills the area of the block's curves around
    /// its `seed` (in block coordinates); its `boundary`, `layer` and `order` aren't used
    /// (a copy's fills are on the copy's layer).
    pub fills: Vec<Fill>,
    /// Written to the drawing's Blender profiles file on save, to sweep or lathe there.
    pub profile: bool,
}

impl Block {
    /// The block's extent in its own coordinates.
    pub fn bounds(&self) -> crate::Bounds {
        let mut b = crate::Bounds::EMPTY;
        for item in &self.items {
            let s = item.shape.bounds();
            b.include(s.min);
            b.include(s.max);
        }
        b
    }
}

/// A unit rotor for `degrees`, exact for multiples of 90°.
pub fn exact_rotor(degrees: f64) -> DVec2 {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() <= 1e-12 {
        match (quarter.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::from_angle(degrees.to_radians())
    }
}

/// Whether a turn is a whole number of quarter turns.
pub fn is_quarter_turn(degrees: f64) -> bool {
    let q = degrees / 90.0;
    (q - q.round()).abs() <= 1e-9
}

/// How a shape is placed: mirrored across its own vertical axis, scaled, turned, then moved.
/// The one rule for block copies, exported inserts and placed regions, with quarter turns
/// exact (see [`exact_rotor`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Where the origin lands.
    pub at: DVec2,
    /// Rotation in degrees, counter-clockwise.
    pub angle: f64,
    pub scale: f64,
    /// Mirrored across the vertical axis (before rotating).
    pub mirror: bool,
}

impl Placement {
    pub const IDENTITY: Placement = Placement {
        at: DVec2::ZERO,
        angle: 0.0,
        scale: 1.0,
        mirror: false,
    };

    pub fn rotor(&self) -> DVec2 {
        exact_rotor(self.angle)
    }

    /// The turn in radians (as arc and text angles are kept).
    pub fn radians(&self) -> f64 {
        self.angle.to_radians()
    }

    /// A local point in the world.
    pub fn apply(&self, p: DVec2) -> DVec2 {
        let p = if self.mirror {
            DVec2::new(-p.x, p.y)
        } else {
            p
        };
        self.at + self.rotor().rotate(p * self.scale)
    }

    /// A world point in local coordinates.
    pub fn invert(&self, w: DVec2) -> DVec2 {
        let r = self.rotor();
        let p = DVec2::new(r.x, -r.y).rotate(w - self.at) / self.scale;
        if self.mirror {
            DVec2::new(-p.x, p.y)
        } else {
            p
        }
    }

    /// The start angle of a placed arc: mirroring reverses it (φ becomes π − φ), and the
    /// turn is added. Its sweep is unchanged.
    pub fn arc_start(&self, start: f64, sweep: f64) -> f64 {
        let start = if self.mirror {
            std::f64::consts::PI - (start + sweep)
        } else {
            start
        };
        start + self.radians()
    }

    /// The start of a placed ellipse's parameter range: mirroring reverses the direction
    /// (t becomes −t); the turn is in the placed major axis.
    pub fn ellipse_start(&self, start: f64, sweep: f64) -> f64 {
        if self.mirror { -(start + sweep) } else { start }
    }

    /// A local shape in the world.
    pub fn shape(&self, s: &Shape) -> Shape {
        match *s {
            Shape::Point(p) => Shape::Point(self.apply(p)),
            Shape::Line { a, b } => Shape::Line {
                a: self.apply(a),
                b: self.apply(b),
            },
            Shape::Circle { center, radius } => Shape::Circle {
                center: self.apply(center),
                radius: radius * self.scale,
            },
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => Shape::Arc {
                center: self.apply(center),
                radius: radius * self.scale,
                start: self.arc_start(start, sweep),
                sweep,
            },
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let c = self.apply(center);
                Shape::Ellipse {
                    center: c,
                    major: self.apply(center + major) - c,
                    minor: minor * self.scale,
                    start: self.ellipse_start(start, sweep),
                    sweep,
                }
            }
            Shape::Bezier { p } => Shape::Bezier {
                p: p.map(|q| self.apply(q)),
            },
        }
    }
}

/// A placed copy of a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub block: BlockId,
    /// Where the block's base point lands.
    pub at: DVec2,
    /// Rotation in degrees, counter-clockwise.
    pub angle: f64,
    pub scale: f64,
    /// Mirrored across the block's vertical axis (before rotating).
    pub mirror: bool,
    pub layer: LayerId,
}

impl Instance {
    pub fn new(block: BlockId, at: DVec2) -> Self {
        Self {
            block,
            at,
            angle: 0.0,
            scale: 1.0,
            mirror: false,
            layer: LayerId::DEFAULT,
        }
    }

    /// How the copy places the block's geometry.
    pub fn placement(&self) -> Placement {
        Placement {
            at: self.at,
            angle: self.angle,
            scale: self.scale,
            mirror: self.mirror,
        }
    }

    /// A block-coordinate point in the world.
    pub fn apply(&self, p: DVec2) -> DVec2 {
        self.placement().apply(p)
    }

    /// A world point in block coordinates.
    pub fn invert(&self, w: DVec2) -> DVec2 {
        self.placement().invert(w)
    }

    /// A block-coordinate shape in the world.
    pub fn apply_shape(&self, s: &Shape) -> Shape {
        self.placement().shape(s)
    }

    /// A block's fill in the world, on the copy's layer: its seed and its hatch go with the
    /// copy (the pattern turns and mirrors with it, and a model pattern scales with it).
    pub fn apply_fill(&self, f: &Fill) -> Fill {
        let mut f = f.clone();
        f.seed = self.apply(f.seed);
        f.layer = self.layer;
        f.path = None;
        if let FillKind::Hatch(h) = &mut f.kind {
            h.origin = self.apply(h.origin);
            let angle = if self.mirror {
                180.0 - h.angle
            } else {
                h.angle
            };
            h.angle = (angle + self.angle).rem_euclid(360.0);
            if h.model {
                h.scale *= self.scale;
            }
        }
        f
    }

    /// A world fill in block coordinates (the inverse of [`apply_fill`](Self::apply_fill)),
    /// without its boundary, layer and order.
    pub fn invert_fill(&self, f: &Fill) -> Fill {
        let mut f = f.clone();
        f.seed = self.invert(f.seed);
        f.boundary.clear();
        f.layer = LayerId::DEFAULT;
        f.order = 0;
        f.path = f.path.map(|_| SHAPE_FILL);
        if let FillKind::Hatch(h) = &mut f.kind {
            h.origin = self.invert(h.origin);
            let angle = h.angle - self.angle;
            h.angle = if self.mirror { 180.0 - angle } else { angle }.rem_euclid(360.0);
            if h.model {
                h.scale /= self.scale;
            }
        }
        f
    }

    /// A world shape in block coordinates (the inverse of [`apply_shape`](Self::apply_shape)).
    pub fn invert_shape(&self, s: &Shape) -> Shape {
        match *s {
            Shape::Point(p) => Shape::Point(self.invert(p)),
            Shape::Line { a, b } => Shape::Line {
                a: self.invert(a),
                b: self.invert(b),
            },
            Shape::Circle { center, radius } => Shape::Circle {
                center: self.invert(center),
                radius: radius / self.scale,
            },
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let start = start - self.angle.to_radians();
                let start = if self.mirror {
                    std::f64::consts::PI - (start + sweep)
                } else {
                    start
                };
                Shape::Arc {
                    center: self.invert(center),
                    radius: radius / self.scale,
                    start,
                    sweep,
                }
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let start = if self.mirror { -(start + sweep) } else { start };
                let c = self.invert(center);
                Shape::Ellipse {
                    center: c,
                    major: self.invert(center + major) - c,
                    minor: minor / self.scale,
                    start,
                    sweep,
                }
            }
            Shape::Bezier { p } => Shape::Bezier {
                p: p.map(|q| self.invert(q)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_place_exactly_and_mirrored_arcs_reverse() {
        let p = Placement {
            at: DVec2::new(10.0, 20.0),
            angle: 90.0,
            scale: 2.0,
            mirror: false,
        };
        assert_eq!(p.apply(DVec2::new(1.0, 0.0)), DVec2::new(10.0, 22.0));
        assert_eq!(p.apply(DVec2::new(0.0, 1.0)), DVec2::new(8.0, 20.0));
        assert_eq!(
            p.invert(p.apply(DVec2::new(3.0, -4.0))),
            DVec2::new(3.0, -4.0)
        );
        assert!(is_quarter_turn(270.0) && !is_quarter_turn(45.0));
        let m = Placement {
            mirror: true,
            ..Placement::IDENTITY
        };
        let arc = Shape::Arc {
            center: DVec2::ZERO,
            radius: 1.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        // A quarter arc from +x to +y, mirrored across y, runs from +y to −x.
        let Shape::Arc { start, sweep, .. } = m.shape(&arc) else {
            panic!("an arc")
        };
        assert!((start - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(sweep, std::f64::consts::FRAC_PI_2);
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn transforms_round_trip() {
        let mut i = Instance::new(BlockId(1), DVec2::new(100.0, 50.0));
        i.angle = 90.0;
        i.scale = 2.0;
        assert!(close(
            i.apply(DVec2::new(10.0, 0.0)),
            DVec2::new(100.0, 70.0)
        ));
        i.mirror = true;
        assert!(close(
            i.apply(DVec2::new(10.0, 0.0)),
            DVec2::new(100.0, 30.0)
        ));
        for p in [DVec2::new(3.0, -4.0), DVec2::new(-7.5, 2.25)] {
            assert!(close(i.invert(i.apply(p)), p));
        }
        // A quarter arc from 0° to 90°, mirrored then turned 90°: its ends follow its points.
        let arc = Shape::Arc {
            center: DVec2::ZERO,
            radius: 5.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        let world = i.apply_shape(&arc);
        let Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } = world
        else {
            panic!()
        };
        let ends = [
            center + DVec2::from_angle(start) * radius,
            center + DVec2::from_angle(start + sweep) * radius,
        ];
        for p in [DVec2::new(5.0, 0.0), DVec2::new(0.0, 5.0)] {
            assert!(ends.iter().any(|e| close(*e, i.apply(p))), "{ends:?}");
        }
        assert_eq!(i.invert_shape(&world), arc);
    }

    #[test]
    fn blocks_and_copies_in_a_document() {
        use crate::{Document, History};
        let mut doc = Document::new();
        let mut history = History::default();
        let chair = Block {
            name: "Chair".into(),
            fills: Vec::new(),
            profile: false,
            items: vec![BlockItem {
                shape: Shape::Line {
                    a: DVec2::new(-200.0, 0.0),
                    b: DVec2::new(200.0, 0.0),
                },
                style: LineKind::Solid,
                appearance: Appearance::default(),
            }],
        };
        doc.begin();
        let b = doc.add_block(chair);
        let mut i = Instance::new(b, DVec2::new(1000.0, 500.0));
        i.angle = 90.0;
        let id = doc.add_instance(i).unwrap();
        history.push(doc.commit("Place"));
        assert_eq!(doc.block_named("chair"), Some(b));
        let items = doc.instance_items(id);
        assert_eq!(
            items[0].shape,
            Shape::Line {
                a: DVec2::new(1000.0, 300.0),
                b: DVec2::new(1000.0, 700.0)
            }
        );
        assert!(
            doc.add_instance(Instance::new(BlockId(99), DVec2::ZERO))
                .is_err()
        );
        // Deleting the block deletes its copies; undo brings both back.
        doc.begin();
        doc.remove_block(b);
        history.push(doc.commit("Delete block"));
        assert!(doc.instance(id).is_none());
        history.undo(&mut doc);
        assert!(doc.instance(id).is_some() && doc.block(b).is_some());
    }
}
