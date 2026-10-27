//! A 2 × 3 affine transform, as the importers read them from PDF and SVG files.

use glam::DVec2;

/// An affine transform `[a b c d e f]` in PDF's order: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2(pub [f64; 6]);

impl Affine2 {
    pub const IDENTITY: Affine2 = Affine2([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn apply(&self, p: DVec2) -> DVec2 {
        let [a, b, c, d, e, f] = self.0;
        DVec2::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// `self` applied first, then `then`.
    pub fn then(&self, then: &Affine2) -> Affine2 {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = then.0;
        Affine2([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }

    /// How much lengths grow (for stroke widths): the root of the area's growth.
    pub fn scale(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        (a * d - b * c).abs().sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn then_applies_the_first_transform_first() {
        let shift = Affine2([1.0, 0.0, 0.0, 1.0, 10.0, 0.0]);
        let double = Affine2([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let p = DVec2::new(1.0, 1.0);
        assert_eq!(shift.then(&double).apply(p), DVec2::new(22.0, 2.0));
        assert_eq!(double.then(&shift).apply(p), DVec2::new(12.0, 2.0));
        assert_eq!(shift.then(&double).apply(p), double.apply(shift.apply(p)));
        assert_eq!(double.scale(), 2.0);
    }
}
