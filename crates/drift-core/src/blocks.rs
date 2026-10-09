//! Blocks: geometry defined once and placed many times (furniture, fixtures, symbols).
//!
//! A [`Block`] holds rigid curves in its own coordinates, with its base point at the origin.
//! An [`Instance`] places a block with a position, rotation, scale, mirror and layer. Copies
//! aren't solved: they're drawn, picked and snapped through their transform, and editing the
//! block updates every copy.

use glam::DVec2;

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

    fn rotor(&self) -> DVec2 {
        exact_rotor(self.angle)
    }

    /// A block-coordinate point in the world.
    pub fn apply(&self, p: DVec2) -> DVec2 {
        let p = if self.mirror {
            DVec2::new(-p.x, p.y)
        } else {
            p
        };
        self.at + self.rotor().rotate(p * self.scale)
    }

    /// A world point in block coordinates.
    pub fn invert(&self, w: DVec2) -> DVec2 {
        let r = self.rotor();
        let p = DVec2::new(r.x, -r.y).rotate(w - self.at) / self.scale;
        if self.mirror {
            DVec2::new(-p.x, p.y)
        } else {
            p
        }
    }

    /// A block-coordinate shape in the world.
    pub fn apply_shape(&self, s: &Shape) -> Shape {
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
            } => {
                // Mirroring reverses the arc: angle φ becomes π − φ.
                let start = if self.mirror {
                    std::f64::consts::PI - (start + sweep)
                } else {
                    start
                };
                Shape::Arc {
                    center: self.apply(center),
                    radius: radius * self.scale,
                    start: start + self.angle.to_radians(),
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
                // Mirroring reverses the direction: parametric angle t becomes −t.
                let start = if self.mirror { -(start + sweep) } else { start };
                let c = self.apply(center);
                Shape::Ellipse {
                    center: c,
                    major: self.apply(center + major) - c,
                    minor: minor * self.scale,
                    start,
                    sweep,
                }
            }
        }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
